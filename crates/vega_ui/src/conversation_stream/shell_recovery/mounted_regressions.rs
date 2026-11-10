use super::{Action, Snapshot};
use crate::conversation_stream::ConversationStream;
use crate::conversation_stream::ShellRecoveryAction;
use crate::conversation_stream::{ShellRecoveryOutcome, ShellRecoveryRequested};
use gpui_kit::{prelude::*, *};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use vega_conversation::shell_recovery::ShellRecoveryService;
use vega_conversation::types::ShellRecoveryError;
use vega_conversation::types::{ShellRecoveryConfirmation, Thread};
use vega_store::Store;

const TIMEOUT: Duration = Duration::from_secs(10);
const DRAIN_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_DRAIN_TICKS: usize = 512;
const MAX_DIAGNOSTIC_STAGES: usize = 1024;

thread_local! {
    static DIAGNOSTIC_STAGES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

struct Stage {
    id: usize,
    case: &'static str,
    name: String,
    started: Instant,
}

impl Stage {
    fn begin(case: &'static str, name: impl Into<String>) -> Option<Self> {
        let id = DIAGNOSTIC_STAGES.with(|count| {
            let current = count.get();
            if current == MAX_DIAGNOSTIC_STAGES {
                return None;
            }
            count.set(current + 1);
            Some(current + 1)
        })?;
        let name = name.into();
        eprintln!("[nsh-recovery][{case}] BEGIN id={id} {name}");
        Some(Self {
            id,
            case,
            name,
            started: Instant::now(),
        })
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        eprintln!(
            "[nsh-recovery][{}] END id={} {} elapsed={:?} unwinding={}",
            self.case,
            self.id,
            self.name,
            self.started.elapsed(),
            std::thread::panicking(),
        );
    }
}

fn traced<R>(case: &'static str, name: impl Into<String>, run: impl FnOnce() -> R) -> R {
    let _stage = Stage::begin(case, name);
    run()
}

type NativeBounds = nsh_core::geometry::Bounds<nsh_core::geometry::Points>;

#[derive(Clone)]
struct Panel {
    observed: Snapshot,
    bounds: NativeBounds,
}

#[derive(Clone)]
struct NativeControl {
    observed: Snapshot,
    action: Action,
    bounds: NativeBounds,
}

static PANELS: Mutex<Vec<Panel>> = Mutex::new(Vec::new());
static CONTROLS: Mutex<Vec<NativeControl>> = Mutex::new(Vec::new());

pub(super) fn record_panel(observed: &Snapshot, bounds: NativeBounds) {
    let mut panels = PANELS.lock().unwrap();
    panels.retain(|panel| !panel.observed.id.matches(&observed.id));
    panels.push(Panel {
        observed: observed.clone(),
        bounds,
    });
}

pub(super) fn record_control(observed: &Snapshot, action: &Action, bounds: NativeBounds) {
    let mut controls = CONTROLS.lock().unwrap();
    controls
        .retain(|control| !control.observed.id.matches(&observed.id) || control.action != *action);
    controls.push(NativeControl {
        observed: observed.clone(),
        action: action.clone(),
        bounds,
    });
}

struct Database {
    directory: PathBuf,
    store: Store,
    draft: Thread,
    persisted: Thread,
}

impl Database {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "vega-mounted-recovery-{}-{}",
            std::process::id(),
            vega_conversation::threads::new_thread_id(),
        ));
        std::fs::create_dir(&directory).unwrap();
        let workspace = directory.join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let store = Store::open(directory.join("recovery.sqlite")).unwrap();
        store.migrate().unwrap();
        let project = vega_store::projects::create(
            store.conn(),
            workspace.to_str().unwrap(),
            "Mounted recovery",
            None,
        )
        .unwrap();
        let persisted =
            vega_conversation::threads::create_thread(&store, &project.id, "fixture", "confirm")
                .unwrap();
        let draft =
            vega_conversation::threads::draft_thread(Some(&project.id), "fixture", "confirm");
        Self {
            directory,
            store,
            draft,
            persisted,
        }
    }

    fn service(&self, draft: bool) -> ShellRecoveryService {
        let path = self.directory.join("recovery.sqlite");
        if draft {
            ShellRecoveryService::for_draft(path)
        } else {
            ShellRecoveryService::new(path)
        }
    }

    fn block(&self) {
        use vega_store::shell_operations::{ShellFacts, ShellOperationRecord};
        let conn = self.store.conn();
        vega_store::messages::insert(
            conn,
            &vega_store::messages::MessageRow {
                id: "mounted-shell-run".into(),
                thread_id: self.persisted.id.clone(),
                seq: 1,
                role: "assistant".into(),
                kind: "text".into(),
                content: String::new(),
                status: "streaming".into(),
                created_at: 1,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .unwrap();
        vega_store::tool_calls::insert(
            conn,
            vega_store::tool_calls::NewToolCall {
                id: "mounted-shell-call",
                thread_id: &self.persisted.id,
                message_id: "mounted-shell-run",
                seq: 1,
                tool: "bash",
                input_json: r#"{"command":"printf mounted-recovery"}"#,
                description: None,
                status: "running",
                created_at: 1,
            },
        )
        .unwrap();
        let workspace = std::fs::canonicalize(self.directory.join("workspace")).unwrap();
        let mut record = ShellOperationRecord {
            operation_id: "mounted-shell-operation",
            thread_id: &self.persisted.id,
            run_id: "mounted-shell-run",
            call_id: "mounted-shell-call",
            generation: "mounted-retired-generation",
            workspace: workspace.to_str().unwrap(),
            concurrent: false,
            state: "prepared",
            exit_code: None,
            error: None,
            duration_ms: None,
        };
        vega_store::shell_operations::record(conn, &record, 1).unwrap();
        record.state = "outcome_unknown";
        vega_store::shell_operations::record_with_facts(
            conn,
            &record,
            &ShellFacts {
                cleanup_state: "unresolved".into(),
                output_incomplete: true,
                resources_closed: true,
                process_resolved: true,
                cleanup_reasons: vec!["output_incomplete".into()],
                ..Default::default()
            },
            2,
        )
        .unwrap();
        self.assert_blocked(true);
    }

    fn assert_blocked(&self, expected: bool) {
        let blockers = vega_store::shell_operations::list_blockers(self.store.conn()).unwrap();
        assert_eq!(blockers.len(), usize::from(expected));
        assert!(
            vega_store::threads::find(self.store.conn(), &self.draft.id)
                .unwrap()
                .is_none()
        );
    }
}

impl Drop for Database {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.directory).unwrap();
    }
}

struct Root {
    case: &'static str,
    stream: Entity<ConversationStream>,
    requests: VecDeque<ShellRecoveryRequested>,
    _events: Subscription,
}

impl Render for Root {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let _stage = Stage::begin(self.case, "Root::render.build");
        div()
            .id("mounted-recovery-root")
            .debug_selector(|| "mounted-recovery-root".to_owned())
            .size_full()
            .flex()
            .child(self.stream.clone())
    }
}

struct Fixture {
    case: &'static str,
    visual: VisualTestContext,
    root: Entity<Root>,
    stream: Entity<ConversationStream>,
    database: Database,
}

impl Fixture {
    fn new(cx: &mut TestAppContext, name: &'static str) -> Self {
        let _stage = Stage::begin(name, "Fixture::new");
        traced(name, "new.clear_observations", || {
            PANELS.lock().unwrap().clear();
            CONTROLS.lock().unwrap().clear();
        });
        traced(name, "new.globals.update", || {
            cx.update(|cx| {
                let _callback = Stage::begin(name, "new.globals.callback");
                assert!(!cx.text_system().all_font_names().is_empty());
                cx.set_global(vega_theme::Theme::dark());
                cx.set_global(crate::settings::SettingsOpen(false));
            });
        });
        let database = traced(name, "new.Database::new", Database::new);
        let thread = database.draft.clone();
        traced(name, "new.route_global.update", || {
            cx.update(|cx| cx.set_global(crate::sidebar::OpenedThread(Some(thread.clone()))));
        });
        let window = traced(name, "new.open_window", || {
            cx.open_window(size(px(720.), px(680.)), |_, cx| {
                let _building = Stage::begin(name, "new.open_window.build_root");
                let stream = traced(name, "new.ConversationStream::new", || {
                    cx.new(|cx| ConversationStream::new(thread, cx))
                });
                let events = cx.subscribe(
                    &stream,
                    |root: &mut Root, _, request: &ShellRecoveryRequested, _| {
                        let _stage = Stage::begin(root.case, "service.request.capture");
                        root.requests.push_back(request.clone());
                    },
                );
                traced(name, "new.set_draft_route", || {
                    stream.update(cx, |stream, cx| stream.set_draft_route(true, cx));
                });
                Root {
                    case: name,
                    stream,
                    requests: VecDeque::new(),
                    _events: events,
                }
            })
        });
        let mut visual = traced(name, "new.VisualTestContext::from_window", || {
            VisualTestContext::from_window(window.into(), cx)
        });
        let (root, stream) = traced(name, "new.activate.visual.update", || {
            visual.update(|window, cx| {
                let _callback = Stage::begin(name, "new.activate.callback");
                window.activate_window();
                let root = window.root::<Root>().flatten().unwrap();
                let stream = root.read(cx).stream.clone();
                (root, stream)
            })
        });
        let mut fixture = Self {
            case: name,
            visual,
            root,
            stream,
            database,
        };
        fixture.frame();
        fixture.complete_read();
        fixture.frame();
        fixture
    }

    fn update<R>(
        &mut self,
        run: impl FnOnce(&mut ConversationStream, &mut Window, &mut Context<ConversationStream>) -> R,
    ) -> R {
        let stream = self.stream.clone();
        let case = self.case;
        traced(case, "fixture.update", || {
            self.visual.update(|window, cx| {
                let _callback = Stage::begin(case, "fixture.update.callback");
                stream.update(cx, |stream, cx| run(stream, window, cx))
            })
        })
    }

    fn drain(&mut self) {
        let _stage = Stage::begin(self.case, "drain");
        let started = Instant::now();
        for tick in 0..MAX_DRAIN_TICKS {
            let queued = traced(
                self.case,
                format!("drain.pending_task_counts tick={tick}"),
                || self.visual.cx.dispatcher.scheduler().pending_task_counts(),
            );
            let worked = traced(
                self.case,
                format!("drain.tick={tick} queued={queued:?}"),
                || self.visual.cx.background_executor.tick(),
            );
            assert!(
                started.elapsed() < DRAIN_TIMEOUT,
                "recovery scheduler drain deadline tick={tick} queued={:?}",
                self.visual.cx.dispatcher.scheduler().pending_task_counts()
            );
            if !worked {
                return;
            }
        }
        panic!("recovery scheduler did not quiesce after {MAX_DRAIN_TICKS} ticks");
    }

    fn frame(&mut self) {
        let case = self.case;
        let _stage = Stage::begin(case, "frame");
        self.drain();
        traced(case, "frame.visual.update", || {
            self.visual.update(|window, cx| {
                let _callback = Stage::begin(case, "frame.callback");
                traced(case, "frame.window.refresh", || window.refresh());
                let arena = traced(case, "frame.window.draw", || window.draw(cx));
                traced(case, "frame.arena.clear", || arena.clear(cx));
            });
        });
        self.drain();
    }

    fn wait(&mut self, reason: &str, mut ready: impl FnMut(&mut Self) -> bool) {
        let _stage = Stage::begin(self.case, format!("wait {reason}"));
        let started = Instant::now();
        loop {
            self.frame();
            if ready(self) {
                return;
            }
            assert!(
                started.elapsed() < TIMEOUT,
                "recovery readiness timeout: {reason}"
            );
            std::thread::sleep(Duration::from_millis(10));
            self.visual
                .cx
                .background_executor
                .advance_clock(Duration::from_millis(10));
        }
    }

    fn take(&mut self) -> ShellRecoveryRequested {
        self.drain();
        let root = self.root.clone();
        self.visual.update(|_, cx| {
            root.update(cx, |root, _| {
                root.requests
                    .pop_front()
                    .expect("production recovery request")
            })
        })
    }

    fn assert_no_requests(&mut self) {
        self.drain();
        let root = self.root.clone();
        self.visual.update(|_, cx| {
            assert!(
                root.read(cx).requests.is_empty(),
                "unexpected service action"
            )
        });
    }

    fn finish(
        &mut self,
        request: &ShellRecoveryRequested,
        outcome: Result<ShellRecoveryOutcome, ShellRecoveryError>,
    ) {
        self.update(|stream, _, cx| stream.finish_shell_recovery(request, outcome, cx));
    }

    fn complete_read(&mut self) {
        let _stage = Stage::begin(self.case, "complete_read");
        let request = self.take();
        assert!(matches!(request.action, ShellRecoveryAction::Read));
        let projection = traced(self.case, "service.read", || {
            self.database
                .service(request.draft)
                .read(&request.source)
                .unwrap()
        });
        self.finish(&request, Ok(ShellRecoveryOutcome::Projection(projection)));
    }

    fn read(&mut self) {
        self.update(|stream, _, cx| stream.refresh_shell_recovery(cx));
        self.complete_read();
    }

    fn keys(&mut self, key: &str) {
        let case = self.case;
        let _stage = Stage::begin(case, format!("keys {key}"));
        traced(case, "keys.visual.update", || {
            self.visual.update(|window, cx| {
                let _callback = Stage::begin(case, "keys.callback");
                window.dispatch_keystroke(Keystroke::parse(key).unwrap(), cx);
            });
        });
        self.drain();
    }

    fn activate(&mut self, action: Action) {
        let _stage = Stage::begin(self.case, format!("activate {action:?}"));
        self.update(|stream, window, cx| {
            stream.shell_focus().focus(window, cx);
            cx.notify();
        });
        self.wait("initial selection reveal", |fixture| {
            fixture.acknowledged(None)
        });
        for _ in 0..12 {
            if self.update(|stream, _, _| {
                stream.shell_page().unwrap().selected.as_ref() == Some(&action)
            }) {
                self.wait("selected control reveal", |fixture| {
                    fixture.acknowledged(Some(&action))
                });
                self.keys("enter");
                return;
            }
            let case = self.case;
            let key = self.update(|stream, window, cx| {
                let page = stream.shell_page().unwrap();
                let current = stream.shell_snapshot(page.id.clone(), page.recovery, cx);
                let enabled: Vec<_> = current
                    .controls()
                    .into_iter()
                    .filter(|control| control.enabled)
                    .map(|control| control.action)
                    .collect();
                let target = enabled.iter().position(|candidate| candidate == &action);
                let selected = page
                    .selected
                    .as_ref()
                    .and_then(|selected| enabled.iter().position(|candidate| candidate == selected));
                eprintln!(
                    "[nsh-recovery][{}] traversal target={action:?} selected={:?} enabled={enabled:?} confirmation={} pending={:?} submitted={} focused={}",
                    case,
                    page.selected,
                    current.has_confirmation,
                    current.pending,
                    current.submitted,
                    stream.shell_focus().is_focused(window),
                );
                let target = target.expect("requested control must be enabled");
                let selected = selected.expect("acknowledged selection must be enabled");
                if target < selected { "shift-tab" } else { "tab" }
            });
            self.keys(key);
            self.wait("Tab selection reveal", |fixture| fixture.acknowledged(None));
        }
        panic!("enabled control not reachable by Tab: {action:?}");
    }

    fn acknowledged(&mut self, action: Option<&Action>) -> bool {
        self.update(|stream, window, cx| {
            let Some(page) = stream.shell_page() else {
                return false;
            };
            let Some(displayed) = &page.displayed else {
                return false;
            };
            let Some(selected) = page.selected.as_ref() else {
                return false;
            };
            let mut current = stream.shell_snapshot(page.id.clone(), page.recovery, cx);
            current.keyboard_active = stream.shell_focus().is_focused(window);
            action.is_none_or(|action| action == selected)
                && current.accepts_selection(displayed, selected)
                && stream.shell_accessibility(&current, window, cx).is_some()
        })
    }

    fn panel(&mut self, summary: bool) -> Option<Panel> {
        self.update(|stream, _, cx| {
            let page = if summary {
                stream.shell_recovery.summary.as_ref()
            } else {
                stream.shell_recovery.presentation.as_ref()
            }?;
            let mut current = stream.shell_snapshot(page.id.clone(), page.recovery, cx);
            current.summary = summary;
            PANELS
                .lock()
                .unwrap()
                .iter()
                .find(|panel| panel.observed.same_authority(&current))
                .cloned()
        })
    }

    fn assert_summary(&mut self, unavailable: bool) {
        let _stage = Stage::begin(
            self.case,
            format!("assert_summary unavailable={unavailable}"),
        );
        self.wait("summary NSH layout", |fixture| {
            fixture.panel(true).is_some()
        });
        let panel = self.panel(true).unwrap();
        assert!(panel.bounds.size.width.0 > 0. && panel.bounds.size.height.0 > 0.);
        assert_eq!(panel.observed.error.is_some(), unavailable);
        assert!(panel.observed.summary);
        self.update(|stream, _, _| {
            assert!(!stream.shell_recovery.opened);
            assert!(stream.shell_recovery.presentation.is_none());
            assert!(stream.shell_recovery.summary.is_some());
        });
        assert!(
            self.visual
                .debug_bounds("shell-recovery-native-host")
                .is_none()
        );
        let host = self
            .visual
            .debug_bounds("shell-recovery-summary-native-host")
            .unwrap();
        assert!(host.size.width > px(0.) && host.size.height > px(0.));
        let controls = CONTROLS.lock().unwrap();
        let current: Vec<_> = controls
            .iter()
            .filter(|control| control.observed.same_authority(&panel.observed))
            .collect();
        assert_eq!(current.len(), 1);
        assert_eq!(current[0].action, Action::Review);
        assert!(current[0].bounds.size.width.0 > 0. && current[0].bounds.size.height.0 > 0.);
    }

    fn mount_persisted(&mut self) {
        let _stage = Stage::begin(self.case, "mount_persisted");
        let thread = self.database.persisted.clone();
        let root = self.root.clone();
        self.stream = self.visual.update(|_, cx| {
            cx.set_global(crate::sidebar::OpenedThread(Some(thread.clone())));
            root.update(cx, |root, cx| {
                assert!(root.requests.is_empty());
                let stream = cx.new(|cx| ConversationStream::new(thread, cx));
                root._events =
                    cx.subscribe(&stream, |root, _, request: &ShellRecoveryRequested, _| {
                        root.requests.push_back(request.clone());
                    });
                root.stream = stream.clone();
                cx.notify();
                stream
            })
        });
        self.read();
        self.wait("persisted inline layout", |fixture| {
            fixture.panel(false).is_some()
        });
    }

    fn open(&mut self) {
        let _stage = Stage::begin(self.case, "open_overlay");
        self.activate(Action::Review);
        self.complete_read();
        self.wait("separate overlay NSH layout and safe focus", |fixture| {
            fixture.panel(false).is_some()
                && fixture.panel(true).is_some()
                && fixture.acknowledged(Some(&Action::NotNow))
        });
        self.update(|stream, window, _| {
            assert!(stream.shell_recovery.safe_focus.is_focused(window));
            assert!(!stream.shell_recovery.review_focus.is_focused(window));
            assert!(stream.shell_recovery.summary.is_some());
            assert!(stream.shell_recovery.presentation.is_some());
        });
        let details = self
            .visual
            .debug_bounds("shell-recovery-native-host")
            .unwrap();
        let summary = self.panel(true).unwrap();
        let panel = self.panel(false).unwrap();
        assert!(!panel.observed.summary && panel.observed.opened);
        assert!(!panel.observed.id.matches(&summary.observed.id));
        assert!(details.size.width > px(0.) && details.size.height > px(0.));
        let root = self.visual.debug_bounds("mounted-recovery-root").unwrap();
        assert!(
            (details.center().y - root.center().y).abs() < px(2.),
            "details must be centered in the separate root overlay"
        );
    }

    fn assert_closed_blocked(&mut self) {
        let _stage = Stage::begin(self.case, "assert_closed_blocked");
        self.frame();
        self.update(|stream, window, _| {
            assert!(!stream.shell_recovery.opened);
            assert!(stream.shell_recovery.review_focus.is_focused(window));
            assert!(!stream.shell_recovery.decision.submitted);
            assert!(stream.shell_recovery.pending.is_none());
            assert!(stream.shell_recovery.decision.confirmation.is_none());
            assert_eq!(
                stream
                    .shell_recovery
                    .projection
                    .as_ref()
                    .unwrap()
                    .rows
                    .len(),
                1
            );
        });
        assert!(
            self.visual
                .debug_bounds("shell-recovery-native-host")
                .is_none()
        );
        self.database.assert_blocked(true);
        self.assert_no_requests();
    }

    fn prepare(&mut self) -> (ShellRecoveryRequested, Box<ShellRecoveryConfirmation>) {
        let _stage = Stage::begin(self.case, "prepare");
        self.activate(Action::Prepare);
        let request = self.take();
        let ShellRecoveryAction::Prepare(expected) = &request.action else {
            panic!("expected Prepare");
        };
        let confirmation = traced(self.case, "service.prepare", || {
            self.database
                .service(request.draft)
                .prepare(&request.source, expected)
                .unwrap()
        });
        (request, Box::new(confirmation))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _stage = Stage::begin(self.case, "Fixture::drop.remove_window");
        self.visual.update(|window, _| window.remove_window());
    }
}

pub fn draft_summary_opens_separate_overlay_with_real_focus(cx: &mut TestAppContext) {
    let _stage = Stage::begin("summary-overlay-focus", "case");
    let mut fixture = Fixture::new(cx, "summary-overlay-focus");
    for _ in 0..3 {
        fixture.frame();
    }
    fixture.update(|stream, _, _| {
        assert!(stream.shell_recovery.summary.is_none());
        assert!(stream.shell_recovery.presentation.is_none());
        assert!(!stream.shell_recovery.opened);
    });
    assert!(
        fixture
            .visual
            .debug_bounds("shell-recovery-summary-native-host")
            .is_none()
    );
    assert!(
        fixture
            .visual
            .debug_bounds("shell-recovery-native-host")
            .is_none()
    );
    fixture.update(|stream, _, cx| stream.request_shell_recovery(ShellRecoveryAction::Read, cx));
    let unavailable = fixture.take();
    fixture.finish(&unavailable, Err(ShellRecoveryError::Unavailable));
    fixture.assert_summary(true);
    fixture.database.block();
    fixture.read();
    fixture.assert_summary(false);
    fixture.open();
    fixture.activate(Action::NotNow);
    fixture.assert_closed_blocked();
    fixture.mount_persisted();
    fixture.update(|stream, _, _| {
        assert!(!stream.draft_route);
        assert!(stream.shell_recovery.summary.is_none());
        assert!(stream.shell_recovery.presentation.is_some());
        assert!(!stream.shell_recovery.opened);
    });
    assert!(
        fixture
            .visual
            .debug_bounds("shell-recovery-summary-native-host")
            .is_none()
    );
    fixture.activate(Action::Review);
    fixture.complete_read();
    fixture.wait("persisted expanded inline layout", |fixture| {
        fixture
            .panel(false)
            .is_some_and(|panel| !panel.observed.draft && panel.observed.opened)
            && fixture.acknowledged(None)
    });
    let inline = fixture
        .visual
        .debug_bounds("shell-recovery-native-host")
        .unwrap();
    let root = fixture
        .visual
        .debug_bounds("mounted-recovery-root")
        .unwrap();
    assert!(
        inline.center().y > root.center().y,
        "persisted details remain in composer, not centered overlay"
    );
    fixture.keys("escape");
    fixture.frame();
    fixture.update(|stream, _, _| {
        assert!(!stream.shell_recovery.opened);
        assert!(stream.shell_recovery.presentation.is_some());
    });
    fixture.database.assert_blocked(true);
    fixture.assert_no_requests();
}

pub fn pending_draft_close_escape_and_authority_keep_recovery_fenced(cx: &mut TestAppContext) {
    let _stage = Stage::begin("pending-close-authority", "case");
    let mut fixture = Fixture::new(cx, "pending-close-authority");
    fixture.database.block();
    fixture.read();
    fixture.assert_summary(false);
    fixture.open();
    let (prepare, confirmation) = fixture.prepare();
    fixture.activate(Action::NotNow);
    fixture.assert_closed_blocked();
    fixture.finish(
        &prepare,
        Ok(ShellRecoveryOutcome::Confirmation(confirmation)),
    );
    fixture.assert_closed_blocked();
    fixture.open();
    let (prepare, confirmation) = fixture.prepare();
    fixture.keys("escape");
    fixture.assert_closed_blocked();
    fixture.finish(
        &prepare,
        Ok(ShellRecoveryOutcome::Confirmation(confirmation)),
    );
    fixture.assert_closed_blocked();
    fixture.open();
    let (prepare, confirmation) = fixture.prepare();
    fixture.update(|stream, _, cx| {
        let mut changed = stream.thread_snapshot();
        changed.permission_revision += 1;
        stream.apply_thread(changed, cx);
    });
    fixture.finish(
        &prepare,
        Ok(ShellRecoveryOutcome::Confirmation(confirmation)),
    );
    fixture.update(|stream, _, _| assert!(stream.shell_recovery.decision.confirmation.is_none()));
    fixture.database.assert_blocked(true);
    fixture.complete_read();
    fixture.assert_summary(false);
    fixture.open();
    let (prepare, confirmation) = fixture.prepare();
    fixture.finish(
        &prepare,
        Ok(ShellRecoveryOutcome::Confirmation(confirmation)),
    );
    fixture.update(|stream, _, _| {
        assert_eq!(stream.shell_recovery.sequence, prepare.sequence);
        assert!(stream.shell_recovery.pending.is_none());
        assert!(stream.shell_recovery.error.is_none());
        assert!(stream.shell_recovery.decision.confirmation.is_some());
        assert!(!stream.shell_recovery.decision.submitted);
    });
    fixture.wait("confirmation safe selection reveal", |fixture| {
        fixture.acknowledged(Some(&Action::KeepPaused))
    });
    fixture.activate(Action::Accept);
    let resume = fixture.take();
    let ShellRecoveryAction::Resume(confirmation) = &resume.action else {
        panic!("expected Resume");
    };
    fixture.keys("escape");
    fixture.frame();
    fixture.update(|stream, window, _| {
        assert!(!stream.shell_recovery.opened);
        assert!(stream.shell_recovery.review_focus.is_focused(window));
        assert!(stream.shell_recovery.decision.submitted);
        assert_eq!(
            stream.shell_recovery.pending,
            Some("ui.shell_recovery.resuming")
        );
        assert_eq!(stream.shell_recovery.sequence, resume.sequence);
    });
    fixture.database.assert_blocked(true);
    fixture.assert_no_requests();
    let result = traced(fixture.case, "service.resume", || {
        fixture.database.service(true).resume(confirmation).unwrap()
    });
    fixture.finish(&resume, Ok(ShellRecoveryOutcome::Projection(result)));
    fixture.frame();
    fixture.database.assert_blocked(false);
    fixture.update(|stream, _, _| {
        assert!(!stream.shell_recovery.decision.submitted);
        assert!(stream.shell_recovery.pending.is_none());
        assert!(stream.shell_recovery.summary.is_none());
    });
}
