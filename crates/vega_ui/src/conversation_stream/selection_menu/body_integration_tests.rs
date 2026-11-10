use super::*;
use crate::conversation_stream::{
    ConversationStream, RunActivityGroup, StreamEntry, ThinkingBlock,
};
use crate::link_preview::{MarkdownLinkPreviewLayer, MarkdownReadingScope, integration_support};
use gpui_kit::{TestAppContext, VisualTestContext};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use vega_conversation::types::{
    ConversationEvent, LinkPreviewData, LinkPreviewFailure, LinkSiteKind, PermissionMode,
    TaskBackend, Thread, ThreadMode, ThreadStatus,
};

const TARGET: &str = "https://example.com/reading?original=1#kept";
const TIMEOUT: Duration = Duration::from_secs(10);
const DRAIN_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_DRAIN_TICKS: usize = 512;

pub(super) struct Stage {
    case: &'static str,
    name: String,
    started: Instant,
}

impl Stage {
    pub(super) fn begin(case: &'static str, name: impl Into<String>) -> Self {
        let name = name.into();
        eprintln!("[nsh-body][{case}] BEGIN {name}");
        Self {
            case,
            name,
            started: Instant::now(),
        }
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        eprintln!(
            "[nsh-body][{}] END {} elapsed={:?} unwinding={}",
            self.case,
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

type PreviewResult = Result<LinkPreviewData, LinkPreviewFailure>;

struct PendingRequest {
    target: String,
    retry: bool,
    cancellation: CancellationToken,
    completion: oneshot::Sender<PreviewResult>,
    completed: Arc<AtomicBool>,
}

#[derive(Clone, Default)]
struct Requests(Arc<Mutex<VecDeque<PendingRequest>>>);

impl Requests {
    fn install(&self, cx: &mut App) {
        let pending = self.0.clone();
        integration_support::install_request(
            cx,
            Arc::new(move |target, retry, cancellation| {
                let (completion, receiver) = oneshot::channel();
                let completed = Arc::new(AtomicBool::new(false));
                pending.lock().unwrap().push_back(PendingRequest {
                    target,
                    retry,
                    cancellation,
                    completion,
                    completed: completed.clone(),
                });
                Box::pin(async move {
                    let result = receiver.await.unwrap_or(Err(LinkPreviewFailure::Cancelled));
                    completed.store(true, Ordering::SeqCst);
                    result
                })
            }),
        );
    }

    fn take(&self) -> PendingRequest {
        self.0.lock().unwrap().pop_front().expect("preview request")
    }

    fn len(&self) -> usize {
        self.0.lock().unwrap().len()
    }
}

#[derive(Clone)]
struct SourceProjection {
    content: Content,
    copy: MessageCopy,
    width: f32,
    mounted: bool,
}

struct FixtureRoot {
    stream: Option<Entity<ConversationStream>>,
    focus: FocusHandle,
    projection: Option<SourceProjection>,
}

fn owns_stream(stream: &WeakEntity<ConversationStream>, window: &Window, cx: &App) -> bool {
    let Some(stream) = stream.upgrade() else {
        return false;
    };
    let Some(root) = window.root::<FixtureRoot>().flatten() else {
        return false;
    };
    root.read(cx).stream.as_ref() == Some(&stream)
        && window.is_window_active()
        && cx
            .try_global::<crate::sidebar::OpenedThread>()
            .and_then(|route| route.0.as_ref())
            .is_some_and(|thread| thread.id == stream.read(cx).thread.id)
}

impl Render for FixtureRoot {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let stream = self.stream.clone();
        let content = stream.map(|stream| {
            let groups = stream
                .read(cx)
                .entries
                .iter()
                .filter_map(|entry| match entry {
                    StreamEntry::RunActivity { group }
                    | StreamEntry::RunActivitySegment { group, .. } => Some(group.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            stream.update(cx, |stream, cx| {
                for group in &groups {
                    stream.rebind_thinking_presentation(group, cx);
                }
            });
            let state = stream.read(cx).list.clone();
            let source = stream.clone();
            MarkdownReadingScope::new("mounted-thinking-reading", self.focus.clone())
                .track_list(state.clone())
                .fill_height()
                .child(super::super::mounted::ReadingList::new(
                    stream.downgrade(),
                    list(state, move |index, window, cx| {
                        let row = match &source.read(cx).entries[index] {
                            StreamEntry::RunActivity { group } => Some((group.clone(), None)),
                            StreamEntry::RunActivitySegment { group, segment } => {
                                Some((group.clone(), Some(*segment)))
                            }
                            _ => None,
                        };
                        match row {
                            Some((group, None)) => RunActivityGroup::render(group, window, cx),
                            Some((group, Some(segment))) => {
                                RunActivityGroup::render_segment(group, segment, window, cx)
                            }
                            None => div().h(px(1.)).into_any_element(),
                        }
                    })
                    .size_full(),
                ))
                .into_any_element()
        });
        div()
            .size_full()
            .relative()
            .child(div().size_full().children(content))
            .child(MarkdownLinkPreviewLayer)
    }
}

fn thread() -> Thread {
    Thread {
        id: "mounted-thinking-reading".into(),
        backend: TaskBackend::Native,
        project_id: String::new(),
        title: "Reading".into(),
        mode: ThreadMode::Ask,
        permission_mode: PermissionMode::ReadOnly,
        permission_revision: 0,
        draft_authority_epoch: None,
        full_access_grant: None,
        default_permission: None,
        model: "fixture".into(),
        status: ThreadStatus::Active,
        pinned: false,
        unread: false,
        created_at: 0,
        updated_at: 0,
        parent_thread_id: None,
        forked_from_thread_id: None,
        forked_from_message_id: None,
        reasoning_preferences: None,
        last_response_failed: false,
    }
}

fn source() -> String {
    let label =
        "Wi office e\u{301} 😀 wrapped measured reading link across actual shaped fragments";
    let mut source = format!("[{label}]({TARGET})\n\n");
    for row in 0..60 {
        source.push_str(&format!(
            "reading row {row}: actual native scrolling content\n\n"
        ));
    }
    source
}

pub(super) fn projected_body(
    copy: &MessageCopy,
    window: &Window,
    cx: &App,
) -> Option<(Content, f32, bool)> {
    let root = window.root::<FixtureRoot>().flatten()?;
    let projection = root.read(cx).projection.as_ref()?;
    (projection.copy.id == copy.id).then(|| {
        (
            projection.content.clone(),
            projection.width,
            projection.mounted,
        )
    })
}

struct ProjectionDiagnostic {
    case: &'static str,
    checks: Vec<(&'static str, bool)>,
    snapshot: String,
}

impl Drop for ProjectionDiagnostic {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!(
                "[nsh-body][{}] source projection checks={:?} snapshot={}",
                self.case, self.checks, self.snapshot,
            );
        }
    }
}

struct AnchorTrace {
    copy: u64,
    events: Vec<(&'static str, usize, String, String)>,
}

thread_local! {
    static ANCHOR_TRACE: RefCell<Option<AnchorTrace>> = const { RefCell::new(None) };
}

pub(super) fn trace_anchor(copy: u64, event: &'static str, detail: impl FnOnce() -> String) {
    ANCHOR_TRACE.with_borrow_mut(|trace| {
        let Some(trace) = trace.as_mut().filter(|trace| trace.copy == copy) else {
            return;
        };
        if let Some((_, count, _, last)) = trace.events.iter_mut().find(|entry| entry.0 == event) {
            *count += 1;
            *last = detail();
        } else if trace.events.len() < 24 {
            let detail = detail();
            trace.events.push((event, 1, detail.clone(), detail));
        }
    });
}

pub(super) fn trace_anchor_state(
    state: &BodyState,
    event: &'static str,
    window: &Window,
    cx: &App,
) {
    trace_anchor(state.copy.id, event, || {
        let host = state.lease.upgrade().and_then(|lease| lease.geometry.get());
        let saved = state.source_anchor.as_ref().map(|saved| {
            format!(
                "anchor={:?} pending={} current={} session={} intent={}/{} observation={}/{} mount={} bounds={:?} clip={:?} area={}",
                saved.anchor,
                saved.pending,
                saved.current(state),
                saved.session.matches(&state.projection.session),
                saved.intent,
                state.scroll_intent,
                saved.observation,
                state.observation.get(),
                saved.mount.get(),
                saved.host.bounds,
                saved.host.clip,
                saved.area,
            )
        });
        let alpha = state
            .surface
            .as_ref()
            .and_then(|surface| surface.entity().read(cx).integration_scroll_alpha());
        format!(
            "valid={} owner={} revision={} source={} ack={} follow={} area={:?} node={:?} mount={} layout_width={:?} host={:?} saved={saved:?} alpha={alpha:?}",
            state.valid(window, cx),
            state
                .owner
                .as_ref()
                .is_some_and(|owner| owner.current_for_frame(state.copy.id, window, cx)),
            state.copy.content_generation() == state.projection.revision,
            state.copy.display_text() == state.projection.source,
            state
                .acknowledged
                .as_ref()
                .is_some_and(|session| session.matches(&state.projection.session)),
            state.follow,
            state.area,
            state.scroll_node,
            state.source_mount.as_ref().is_some_and(|mount| mount.get()),
            state.source_layout_width,
            host.map(|host| (host.bounds, host.clip, host.list)),
        )
    });
}

struct AnchorDiagnostic(&'static str);

impl Drop for AnchorDiagnostic {
    fn drop(&mut self) {
        let trace = ANCHOR_TRACE.with_borrow_mut(Option::take);
        if std::thread::panicking()
            && let Some(trace) = trace
        {
            eprintln!(
                "[nsh-body][{}] source caret restore copy={} events(count,first,last)={:?}",
                self.0, trace.copy, trace.events,
            );
        }
    }
}

struct Fixture {
    case: &'static str,
    anchor_diagnostic: Option<AnchorDiagnostic>,
    visual: VisualTestContext,
    root: Entity<FixtureRoot>,
    stream: Entity<ConversationStream>,
    card: Entity<ThinkingBlock>,
    mounted_body: Option<Entity<BodyState>>,
    requests: Requests,
}

impl Fixture {
    fn new(cx: &mut TestAppContext, case: &'static str) -> Self {
        let _stage = Stage::begin(case, "Fixture::new");
        traced(case, "new.real_fonts.cx.update", || {
            cx.update(|cx| assert!(!cx.text_system().all_font_names().is_empty()));
        });
        let requests = Requests::default();
        let install = requests.clone();
        traced(case, "new.globals.cx.update", || {
            cx.update(|cx| {
                cx.set_global(vega_theme::Theme::dark());
                cx.set_global(crate::settings::SettingsOpen(false));
                cx.set_global(crate::sidebar::OpenedThread(Some(thread())));
                install.install(cx);
            });
        });
        let opening = Stage::begin(case, "new.open_window");
        let window = cx.open_window(size(px(360.), px(440.)), |_, cx| {
            let _building = Stage::begin(case, "new.open_window.build_root");
            let stream = traced(case, "new.ConversationStream::new", || {
                cx.new(|cx| ConversationStream::new(thread(), cx))
            });
            let configuring = Stage::begin(case, "new.stream.update_events_and_reveal");
            stream.update(cx, |stream, cx| {
                stream.set_reading_navigation_authority(owns_stream, cx);
                stream.apply_event(
                    ConversationEvent::MessageStarted {
                        message_id: "reasoning".into(),
                        seq: 1,
                    },
                    cx,
                );
                stream.apply_event(
                    ConversationEvent::ThinkingDelta {
                        message_id: "reasoning".into(),
                        delta: source(),
                    },
                    cx,
                );
                let segments = stream
                    .entries
                    .iter()
                    .filter_map(|entry| match entry {
                        StreamEntry::RunActivitySegment { group, segment } => {
                            Some((group.clone(), *segment))
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                assert!(!segments.is_empty());
                for (group, segment) in segments {
                    assert!(group.update(cx, |group, cx| group.reveal_segment(segment, cx)));
                    stream.rebind_thinking_presentation(&group, cx);
                }
            });
            drop(configuring);
            FixtureRoot {
                stream: Some(stream),
                focus: cx.focus_handle().tab_stop(true),
                projection: None,
            }
        });
        drop(opening);
        let mut visual = traced(case, "new.VisualTestContext::from_window", || {
            VisualTestContext::from_window(window.into(), cx)
        });
        let activating = Stage::begin(case, "new.activate.visual.update");
        let (root, stream, card) = visual.update(|window, cx| {
            window.activate_window();
            let root = window.root::<FixtureRoot>().flatten().unwrap();
            let stream = root.read(cx).stream.clone().unwrap();
            let card = stream.read(cx).active_thinking.clone().unwrap();
            assert!(card.read(cx).expanded);
            (root, stream, card)
        });
        drop(activating);
        let mut fixture = Self {
            case,
            anchor_diagnostic: None,
            visual,
            root,
            stream,
            card,
            mounted_body: None,
            requests,
        };
        fixture.wait("real BodyState / Engine / measured link mount", |this| {
            this.visual.update(|window, cx| {
                let copy = this.card.read(cx).integration_source_copy();
                let binding = copy.nsh_reading.borrow().clone();
                let Some(state) = binding.and_then(|binding| binding.state.upgrade()) else {
                    return false;
                };
                let state = state.read(cx);
                assert!(state.projection.thinking);
                assert!(state.surface.as_ref().unwrap().errors().borrow().is_none());
                state.valid(window, cx)
                    && state.mounted_content_current(cx)
                    && state
                        .surface
                        .as_ref()
                        .zip(state.area)
                        .and_then(|(surface, area)| {
                            let view = surface.entity();
                            let view = view.read(cx);
                            let snapshot = view.reading_snapshot(area)?;
                            view.reading_geometry(&snapshot)
                        })
                        .is_some()
                    && integration_support::observe(window, cx).is_some_and(|state| {
                        state.visible.iter().any(|link| {
                            link.target == TARGET && link.current && link.text_bounds.len() >= 2
                        })
                    })
            })
        });
        fixture.mounted_body = Some(fixture.body());
        fixture
    }

    fn drain(&mut self, reason: &str) {
        let _stage = Stage::begin(self.case, format!("drain {reason}"));
        let started = Instant::now();
        for tick in 0..MAX_DRAIN_TICKS {
            let scheduler = self.visual.cx.dispatcher.scheduler();
            let queued = scheduler.pending_task_counts();
            let worked = traced(
                self.case,
                format!("drain {reason} tick={tick} queued={queued:?}"),
                || self.visual.cx.background_executor.tick(),
            );
            assert!(
                started.elapsed() < DRAIN_TIMEOUT,
                "scheduler drain deadline: case={} stage={reason} tick={tick} queued={:?}",
                self.case,
                scheduler.pending_task_counts(),
            );
            if !worked {
                return;
            }
        }
        panic!(
            "scheduler did not quiesce: case={} stage={reason} ticks={MAX_DRAIN_TICKS} elapsed={:?} queued={:?}",
            self.case,
            started.elapsed(),
            self.visual.cx.dispatcher.scheduler().pending_task_counts(),
        );
    }

    fn frame(&mut self) {
        let _stage = Stage::begin(self.case, "frame");
        self.drain("before draw");
        let case = self.case;
        traced(case, "frame.visual.update", || {
            self.visual.update(|window, cx| {
                traced(case, "frame.window.refresh", || window.refresh());
                let arena = traced(case, "frame.window.draw", || window.draw(cx));
                traced(case, "frame.arena.clear", || arena.clear(cx));
            });
        });
        self.drain("after draw");
    }

    fn step(&mut self, duration: Duration) {
        let _stage = Stage::begin(self.case, format!("step {duration:?}"));
        std::thread::sleep(duration);
        traced(self.case, "step.advance_clock", || {
            self.visual.cx.background_executor.advance_clock(duration);
        });
        self.frame();
    }

    fn wait(&mut self, reason: &str, mut ready: impl FnMut(&mut Self) -> bool) {
        let _stage = Stage::begin(self.case, format!("wait {reason}"));
        let start = Instant::now();
        loop {
            self.frame();
            if traced(self.case, format!("wait.predicate {reason}"), || {
                ready(self)
            }) {
                return;
            }
            assert!(start.elapsed() < TIMEOUT, "timeout: {reason}");
            self.step(Duration::from_millis(10));
        }
    }

    fn input<E: InputEvent>(&mut self, reason: &str, event: E) {
        traced(self.case, format!("input.visual.update {reason}"), || {
            self.visual.update(|window, cx| {
                window.dispatch_event(event.to_platform_input(), cx);
            });
        });
        self.drain(reason);
    }

    fn keys(&mut self, keys: &str) {
        for key in keys.split(' ') {
            traced(self.case, format!("keys.visual.update {key}"), || {
                self.visual.update(|window, cx| {
                    window.dispatch_keystroke(Keystroke::parse(key).unwrap(), cx);
                });
            });
        }
        self.drain(keys);
    }

    fn body(&mut self) -> Entity<BodyState> {
        if let Some(body) = &self.mounted_body {
            return body.clone();
        }
        self.visual.update(|_, cx| {
            let copy = self.card.read(cx).integration_source_copy();
            let binding = copy.nsh_reading.borrow().clone().unwrap();
            binding.state.upgrade().unwrap()
        })
    }

    fn target_point(&mut self) -> Point<Pixels> {
        self.visual.update(|window, cx| {
            let state = integration_support::observe(window, cx).unwrap();
            let target = state
                .visible
                .iter()
                .find(|link| link.target == TARGET && link.current)
                .expect("current production LinkedBody target");
            assert!(target.text_bounds.len() >= 2);
            let rect = target.text_bounds[0];
            rect.origin + point(rect.size.width / 2., rect.size.height / 2.)
        })
    }

    fn hover_after_wheel(&mut self) -> nsh_platform_gpui::reading::ReadingGeometry {
        let _stage = Stage::begin(self.case, "hover_after_wheel");
        let body = self.body();
        let before = self.visual.update(|_, cx| {
            let state = body.read(cx);
            let view = state.surface.as_ref().unwrap().entity();
            let view = view.read(cx);
            view.reading_geometry(&view.reading_snapshot(state.area.unwrap()).unwrap())
                .unwrap()
        });
        let point = self.target_point();
        self.input(
            "wheel",
            ScrollWheelEvent {
                position: point,
                delta: ScrollDelta::Lines(gpui_kit::point(0., -0.125)),
                touch_phase: TouchPhase::Moved,
                ..Default::default()
            },
        );
        self.wait("real wheel scroll revokes previous geometry", |this| {
            this.visual.update(|_, cx| {
                !body
                    .read(cx)
                    .surface
                    .as_ref()
                    .unwrap()
                    .entity()
                    .read(cx)
                    .reading_geometry_is_current(&before)
            })
        });
        self.wait("real wheel enables actual scrollbar alpha", |this| {
            this.visual.update(|_, cx| {
                body.read(cx)
                    .surface
                    .as_ref()
                    .unwrap()
                    .entity()
                    .read(cx)
                    .integration_scroll_alpha()
                    .is_some_and(|alpha| alpha > 0.)
            })
        });
        let point = self.target_point();
        self.input(
            "hover",
            MouseMoveEvent {
                position: point,
                pressed_button: None,
                modifiers: Modifiers::default(),
            },
        );
        let geometry = self.visual.update(|window, cx| {
            let state = body.read(cx);
            assert!(state.valid(window, cx));
            let view = state.surface.as_ref().unwrap().entity();
            let view = view.read(cx);
            view.reading_geometry(&view.reading_snapshot(state.area.unwrap()).unwrap())
                .unwrap()
        });
        let started = Instant::now();
        while started.elapsed() < Duration::from_millis(1150) {
            self.step(Duration::from_millis(25));
            self.visual.update(|window, cx| {
                assert_eq!(window.mouse_position(), point);
                let state = body.read(cx);
                assert!(state.valid(window, cx));
                assert!(state.mounted_content_current(cx));
                assert!(
                    state
                        .surface
                        .as_ref()
                        .unwrap()
                        .entity()
                        .read(cx)
                        .reading_geometry_is_current(&geometry)
                );
                let preview = integration_support::observe(window, cx).unwrap();
                assert!(
                    preview
                        .visible
                        .iter()
                        .any(|target| target.target == TARGET && target.current)
                );
            });
        }
        self.visual.update(|window, cx| {
            assert_eq!(
                body.read(cx)
                    .surface
                    .as_ref()
                    .unwrap()
                    .entity()
                    .read(cx)
                    .integration_scroll_alpha(),
                Some(0.)
            );
            let state = integration_support::observe(window, cx).unwrap();
            let active = state
                .active
                .expect("stationary hover preview survives fade");
            assert!(active.current);
            assert_eq!(active.target, TARGET);
            assert!(active.card_bounds.is_some_and(|bounds| !bounds.is_empty()));
        });
        assert_eq!(self.requests.len(), 1);
        geometry
    }

    fn fail_initial_request(&mut self) {
        let request = self.requests.take();
        assert_eq!(request.target, TARGET);
        assert!(!request.retry);
        assert!(!request.cancellation.is_cancelled());
        request
            .completion
            .send(Err(LinkPreviewFailure::Network))
            .unwrap();
        self.wait("real preview failure completion", |this| {
            this.visual.update(|window, cx| {
                integration_support::observe(window, cx)
                    .and_then(|state| state.active)
                    .is_some_and(|active| {
                        active.current && active.data == Some(Err(LinkPreviewFailure::Network))
                    })
            })
        });
    }

    fn focus_open(&mut self) {
        self.visual.update(|window, cx| {
            let state = integration_support::observe(window, cx).unwrap();
            let active = state.active.unwrap();
            assert!(active.current);
            window.focus(&active.open, cx);
        });
    }

    fn retry(&mut self) -> PendingRequest {
        self.focus_open();
        self.keys("tab");
        self.visual.update(|window, cx| {
            let state = integration_support::observe(window, cx).unwrap();
            assert!(state.active.unwrap().retry.is_focused(window));
        });
        self.keys("enter");
        self.wait("real retry starts a request", |this| {
            this.requests.len() == 1
        });
        let request = self.requests.take();
        assert_eq!(request.target, TARGET);
        assert!(request.retry);
        assert!(!request.cancellation.is_cancelled());
        request
    }

    fn project_source(&mut self, source: &str, start: usize, domain: u64, width: f32) {
        use crate::conversation_stream::model::{StreamCounters, StreamModel};
        let copy = self
            .visual
            .update(|_, cx| self.card.read(cx).integration_source_copy());
        let mut markdown = vega_markdown::MarkdownStream::new();
        markdown.append(source);
        let mut model = StreamModel::default();
        model.sync(&markdown.snapshot(), &StreamCounters::default());
        let content = Content::from_model(&model, copy.id)
            .with_source(source.to_owned(), 0, start)
            .with_source_domain(domain);
        self.visual.update(|_, cx| {
            self.root.update(cx, |root, cx| {
                root.projection = Some(SourceProjection {
                    content,
                    copy,
                    width,
                    mounted: true,
                });
                cx.notify();
            });
            self.card.update(cx, |_, cx| cx.notify());
            self.stream.read(cx).list.remeasure();
        });
        let mut diagnostic = ProjectionDiagnostic {
            case: self.case,
            checks: Vec::new(),
            snapshot: String::new(),
        };
        self.wait("current real source projection", |this| {
            let body = this.body();
            this.visual.update(|window, cx| {
                let state = body.read(cx);
                let owner = state.owner.as_ref();
                let lease = state.lease.upgrade();
                let host = lease.as_ref().and_then(|lease| lease.geometry.get());
                let binding = state.copy.nsh_reading.borrow();
                let snapshot = state
                    .surface
                    .as_ref()
                    .zip(state.area)
                    .and_then(|(surface, area)| surface.entity().read(cx).reading_snapshot(area));
                diagnostic.checks = vec![
                    (
                        "body_binding",
                        binding
                            .as_ref()
                            .and_then(|binding| binding.state.upgrade())
                            .as_ref()
                            == Some(&body),
                    ),
                    (
                        "owner_root",
                        owner.is_some_and(|owner| owner.root_current(window, cx)),
                    ),
                    (
                        "owner_source",
                        owner.is_some_and(|owner| {
                            owner.current_for_frame(state.copy.id, window, cx)
                        }),
                    ),
                    (
                        "revision",
                        state.copy.content_generation() == state.projection.revision,
                    ),
                    (
                        "source",
                        state.copy.display_text() == state.projection.source,
                    ),
                    (
                        "acknowledged",
                        state
                            .acknowledged
                            .as_ref()
                            .is_some_and(|session| session.matches(&state.projection.session)),
                    ),
                    (
                        "surface",
                        state
                            .surface
                            .as_ref()
                            .is_some_and(|surface| surface.errors().borrow().is_none()),
                    ),
                    (
                        "lease_live",
                        lease.as_ref().is_some_and(|lease| lease.live.get()),
                    ),
                    (
                        "lease_frame",
                        lease.as_ref().zip(owner).is_some_and(|(lease, owner)| {
                            super::super::mounted::lease_current(lease, &owner.stream, window, cx)
                        }),
                    ),
                    (
                        "viewport",
                        host.is_some_and(|host| {
                            host.viewport == window.viewport_size()
                                && host.scale == window.scale_factor()
                        }),
                    ),
                    (
                        "clip",
                        host.is_some_and(|host| !host.bounds.intersect(&host.clip).is_empty()),
                    ),
                    (
                        "list",
                        host.zip(owner).is_some_and(|(host, owner)| {
                            owner.stream.upgrade().is_some_and(|stream| {
                                let stream = stream.read(cx);
                                host.list
                                    == super::super::mounted::list_bounds(&owner.stream, window, cx)
                                        .unwrap_or_else(|| stream.list.viewport_bounds())
                                    && host.foreground == stream.foreground_height
                            })
                        }),
                    ),
                    (
                        "mount",
                        state.source_mount.as_ref().is_some_and(|mount| mount.get()),
                    ),
                    ("valid", state.valid(window, cx)),
                    ("snapshot", snapshot.is_some()),
                    ("content", state.mounted_content_current(cx)),
                    ("scroll_node", state.scroll_node.is_some()),
                    (
                        "source_start",
                        state.projection.content.source_start == start,
                    ),
                    (
                        "source_domain",
                        state.projection.content.source_domain == domain,
                    ),
                    (
                        "source_window",
                        state.projection.content.source.as_deref() == Some(source),
                    ),
                ];
                if let Some(snapshot) = &snapshot {
                    let expected = state
                        .projection
                        .content
                        .fragments
                        .iter()
                        .filter(|fragment| !fragment.text.is_empty())
                        .collect::<Vec<_>>();
                    let actual = snapshot
                        .texts
                        .iter()
                        .flat_map(|text| {
                            text.content.test_ids.iter().map(move |tag| {
                                let value = utf16_byte(&text.content.text, tag.start)
                                    .zip(utf16_byte(&text.content.text, tag.end))
                                    .and_then(|(start, end)| text.content.text.get(start..end));
                                (tag.test_id.as_ref(), value)
                            })
                        })
                        .collect::<Vec<_>>();
                    let mismatch =
                        expected
                            .iter()
                            .zip(&actual)
                            .position(|(fragment, (id, value))| {
                                fragment.id != *id || Some(fragment.text.as_str()) != *value
                            });
                    diagnostic.snapshot = format!(
                        "texts={} expected_tags={} actual_tags={} first_mismatch={mismatch:?}",
                        snapshot.texts.len(),
                        expected.len(),
                        actual.len(),
                    );
                } else {
                    diagnostic.snapshot = "missing".into();
                }
                state.valid(window, cx)
                    && state.mounted_content_current(cx)
                    && state.scroll_node.is_some()
                    && state.projection.content.source_start == start
                    && state.projection.content.source_domain == domain
                    && state.projection.content.source.as_deref() == Some(source)
            })
        });
        self.mounted_body = Some(self.body());
    }

    fn resize_source(&mut self, width: f32) {
        self.begin_anchor_diagnostic();
        self.visual.update(|_, cx| {
            self.root.update(cx, |root, cx| {
                root.projection.as_mut().unwrap().width = width;
                cx.notify();
            });
            self.card.update(cx, |_, cx| cx.notify());
            self.stream.read(cx).list.remeasure();
        });
    }

    fn source_sample(&mut self) -> Option<SourceSample> {
        let body = self.body();
        self.visual.update(|window, cx| {
            let state = body.read(cx);
            if !state.valid(window, cx) || !state.mounted_content_current(cx) {
                return None;
            }
            let host = state.lease.upgrade()?.geometry.get()?;
            let clipped = host.bounds.intersect(&host.clip).intersect(&host.list);
            let viewport = [
                f32::from(clipped.origin.x - host.bounds.origin.x),
                f32::from(clipped.origin.y - host.bounds.origin.y),
                f32::from(clipped.size.width),
                f32::from(clipped.size.height),
            ];
            let surface = state.surface.as_ref()?.entity();
            let snapshot = surface.read(cx).reading_snapshot(state.area?)?;
            let geometry = surface.read(cx).reading_geometry(&snapshot)?;
            let anchor = source_anchor::Anchor::capture(
                &state.projection.content,
                &state.projection.source,
                &snapshot,
                &geometry,
                viewport,
            )?;
            Some(SourceSample {
                anchor,
                geometry,
                snapshot,
                viewport,
            })
        })
    }

    fn baseline(&mut self) -> source_anchor::Anchor {
        self.begin_anchor_diagnostic();
        let body = self.body();
        self.wait("captured source caret baseline", |this| {
            this.visual.update(|_, cx| {
                let state = body.read(cx);
                state
                    .source_anchor
                    .as_ref()
                    .is_some_and(|saved| !saved.pending && saved.current(state))
            })
        });
        self.visual
            .update(|_, cx| body.read(cx).source_anchor.as_ref().unwrap().anchor.clone())
    }

    fn move_source(&mut self, delta: f32) {
        let body = self.body();
        let sample = self.source_sample().expect("real mounted source geometry");
        let intent = self.visual.update(|_, cx| {
            let state = body.read(cx);
            assert!(
                state
                    .surface
                    .as_ref()
                    .unwrap()
                    .entity()
                    .read(cx)
                    .scroll_reading_anchor(&sample.geometry, state.scroll_node.unwrap(), delta)
            );
            state.scroll_intent
        });
        self.wait("native scroll report reaches production body", |this| {
            this.visual
                .update(|_, cx| body.read(cx).scroll_intent != intent)
        });
        self.wait("native scrollbar becomes idle", |this| {
            this.visual.update(|_, cx| {
                body.read(cx)
                    .surface
                    .as_ref()
                    .unwrap()
                    .entity()
                    .read(cx)
                    .integration_scroll_alpha()
                    == Some(0.)
            })
        });
        self.baseline();
    }

    fn begin_anchor_diagnostic(&mut self) {
        if self.anchor_diagnostic.is_some() {
            return;
        }
        let body = self.body();
        let copy = self.visual.update(|_, cx| body.read(cx).copy.id);
        ANCHOR_TRACE.with_borrow_mut(|trace| {
            *trace = Some(AnchorTrace {
                copy,
                events: Vec::new(),
            });
        });
        self.anchor_diagnostic = Some(AnchorDiagnostic(self.case));
    }

    fn assert_caret(&mut self, expected: &source_anchor::Anchor) {
        self.begin_anchor_diagnostic();
        let body = self.body();
        let copy = self.visual.update(|_, cx| body.read(cx).copy.id);
        self.wait("same source caret at retained signed inset", |this| {
            let Some(sample) = this.source_sample() else {
                trace_anchor(copy, "sample.missing", String::new);
                return false;
            };
            let body = this.body();
            this.visual.update(|_, cx| {
                let state = body.read(cx);
                let width = this.root.read(cx).projection.as_ref().unwrap().width;
                trace_anchor(copy, "sample.viewport", || {
                    format!(
                        "expected={expected:?} captured={:?} viewport={:?} width={width}",
                        sample.anchor, sample.viewport,
                    )
                });
                if (sample.viewport[2] - width).abs() > 0.75 {
                    return false;
                }
                let inset = measured_inset(
                    &state.projection.content,
                    &state.projection.source,
                    expected,
                    &sample,
                );
                trace_anchor(copy, "sample.inset", || {
                    format!("expected={} actual={inset:?}", expected.inset)
                });
                let view = state.surface.as_ref().unwrap().entity();
                let retained = state.source_anchor.as_ref().is_some_and(|saved| {
                    saved.anchor == *expected
                        && !saved.pending
                        && saved.current(state)
                        && view.read(cx).reading_geometry_is_current(&saved.geometry)
                });
                let reported = state.anchor_scroll.is_none();
                let idle = view.read(cx).integration_scroll_alpha() == Some(0.);
                trace_anchor(copy, "sample.settled", || {
                    format!("retained={retained} reported={reported} idle={idle}")
                });
                inset.is_some_and(|inset| (inset - expected.inset).abs() <= 0.75)
                    && retained
                    && reported
                    && idle
            })
        });
        let body = self.body();
        self.visual.update(|_, cx| assert!(!body.read(cx).follow));
    }

    fn stop(&mut self) {
        self.anchor_diagnostic.take();
        let _stage = Stage::begin(self.case, "stop");
        let body = self.body();
        self.visual.update(|_, cx| {
            body.read(cx)
                .surface
                .as_ref()
                .unwrap()
                .entity()
                .read(cx)
                .stop();
            self.root.update(cx, |root, cx| {
                root.stream.take();
                cx.notify();
            });
        });
        self.frame();
    }
}

struct SourceSample {
    anchor: source_anchor::Anchor,
    geometry: nsh_platform_gpui::reading::ReadingGeometry,
    snapshot: ReadingSnapshot,
    viewport: [f32; 4],
}

fn measured_inset(
    content: &Content,
    source: &str,
    expected: &source_anchor::Anchor,
    sample: &SourceSample,
) -> Option<f32> {
    let byte = content
        .mapping(source)
        .iter()
        .find_map(|span| span.rendered_position(&expected.position))?;
    for (index, text) in sample.snapshot.texts.iter().enumerate() {
        for tag in &text.content.test_ids {
            let Some(fragment) = content.fragments.iter().find(|fragment| {
                fragment.id == tag.test_id.as_ref()
                    && match expected.affinity {
                        nsh_core::Affinity::Upstream => {
                            fragment.range.start < byte && byte <= fragment.range.end
                        }
                        nsh_core::Affinity::Downstream => {
                            fragment.range.start <= byte && byte < fragment.range.end
                        }
                    }
            }) else {
                continue;
            };
            let start = utf16_byte(&text.content.text, tag.start)?;
            let end = utf16_byte(&text.content.text, tag.end)?;
            assert_eq!(
                text.content.text.get(start..end),
                Some(fragment.text.as_str())
            );
            let prefix = fragment.text.get(..byte - fragment.range.start)?;
            let offset = tag
                .start
                .checked_add(u32::try_from(prefix.encode_utf16().count()).ok()?)?;
            let caret = sample.geometry.caret_rect(
                index,
                nsh_core::TextPosition::new(offset, expected.affinity),
            )?;
            return Some(caret[1] - sample.viewport[1]);
        }
    }
    None
}

fn caret_source() -> String {
    (0..96).map(|row| format!(
        "source {row:03}: Wi office e\u{301} 😀 &amp; עברית العربية measured proportional text retained within a single wrapped paragraph. "
    )).collect()
}

pub fn source_caret_reflow_preserves_position_affinity_and_signed_inset(cx: &mut TestAppContext) {
    let mut fixture = Fixture::new(
        cx,
        "source_caret_reflow_preserves_position_affinity_and_signed_inset",
    );
    fixture.project_source(&caret_source(), 0, 41, 320.);
    fixture.move_source(473.25);
    let anchor = fixture.baseline();
    assert!(anchor.position.source.start > 0);
    assert!(anchor.inset < 0.);
    let before = fixture.source_sample().unwrap();
    fixture.resize_source(218.);
    fixture.assert_caret(&anchor);
    let after = fixture.source_sample().unwrap();
    assert!(after.viewport[2] < before.viewport[2]);
    let body = fixture.body();
    fixture.visual.update(|_, cx| {
        assert!(
            !body
                .read(cx)
                .surface
                .as_ref()
                .unwrap()
                .entity()
                .read(cx)
                .reading_geometry_is_current(&before.geometry)
        )
    });
    fixture.resize_source(302.);
    fixture.assert_caret(&anchor);
    fixture.stop();
}

pub fn source_caret_trim_preserves_absolute_position_inside_paragraph(cx: &mut TestAppContext) {
    let mut fixture = Fixture::new(
        cx,
        "source_caret_trim_preserves_absolute_position_inside_paragraph",
    );
    let source = caret_source();
    fixture.project_source(&source, 0, 42, 320.);
    fixture.move_source(617.5);
    let anchor = fixture.baseline();
    let cut = source.find("source 001:").unwrap();
    assert!(cut < anchor.position.source.start);
    fixture.project_source(&source[cut..], cut, 42, 320.);
    fixture.assert_caret(&anchor);
    let body = fixture.body();
    fixture.visual.update(|_, cx| {
        let state = body.read(cx);
        assert!(
            state
                .projection
                .content
                .mapping(&state.projection.source)
                .iter()
                .any(|span| span.rendered_position(&anchor.position).is_some())
        );
    });
    fixture.stop();
}

pub fn source_caret_removed_trim_clamps_forward_with_zero_inset(cx: &mut TestAppContext) {
    let mut fixture = Fixture::new(
        cx,
        "source_caret_removed_trim_clamps_forward_with_zero_inset",
    );
    let source = caret_source();
    fixture.project_source(&source, 0, 43, 320.);
    fixture.move_source(271.25);
    let anchor = fixture.baseline();
    let cut = source.find("source 040:").unwrap();
    assert!(cut > anchor.position.source.end);
    fixture.project_source(&source[cut..], cut, 43, 320.);
    let expected = source_anchor::Anchor {
        position: vega_markdown::SourcePosition {
            source: cut..cut,
            offset: 0,
        },
        affinity: nsh_core::Affinity::Downstream,
        inset: 0.,
    };
    fixture.assert_caret(&expected);
    assert_eq!(
        fixture.source_sample().unwrap().anchor.position,
        expected.position
    );
    fixture.stop();
}

pub fn source_caret_replacement_does_not_restore_equal_offsets(cx: &mut TestAppContext) {
    let mut fixture = Fixture::new(
        cx,
        "source_caret_replacement_does_not_restore_equal_offsets",
    );
    let source = caret_source();
    fixture.project_source(&source, 0, 44, 320.);
    fixture.move_source(351.5);
    let old = fixture.baseline();
    let body = fixture.body();
    let old_session = fixture
        .visual
        .update(|_, cx| body.read(cx).projection.session.clone());
    fixture.project_source(&source, 0, 45, 218.);
    fixture.wait("replacement has its own nonpending baseline", |this| {
        this.visual.update(|_, cx| {
            body.read(cx)
                .source_anchor
                .as_ref()
                .is_some_and(|saved| !saved.pending && !saved.session.matches(&old_session))
        })
    });
    let sample = fixture.source_sample().unwrap();
    let old_inset = fixture.visual.update(|_, cx| {
        let state = body.read(cx);
        measured_inset(
            &state.projection.content,
            &state.projection.source,
            &old,
            &sample,
        )
        .unwrap()
    });
    assert!((old_inset - old.inset).abs() > 1.);
    fixture.stop();
}

pub fn source_caret_owner_and_mount_aba_reject_old_geometry(cx: &mut TestAppContext) {
    let mut fixture = Fixture::new(cx, "source_caret_owner_and_mount_aba_reject_old_geometry");
    fixture.project_source(&caret_source(), 0, 46, 320.);
    fixture.move_source(317.25);
    let old = fixture.source_sample().unwrap();
    let body = fixture.body();
    let (session, mount) = fixture.visual.update(|_, cx| {
        let state = body.read(cx);
        (
            state.projection.session.clone(),
            state.source_mount.clone().unwrap(),
        )
    });
    fixture.visual.update(|_, cx| {
        fixture
            .stream
            .update(cx, |stream, cx| stream.suspend_reading_navigation(cx));
        fixture.root.update(cx, |root, cx| {
            root.projection.as_mut().unwrap().mounted = false;
            cx.notify();
        });
        fixture.card.update(cx, |_, cx| cx.notify());
        fixture.stream.read(cx).list.remeasure();
    });
    fixture.frame();
    fixture.frame();
    assert!(!mount.get());
    fixture.visual.update(|_, cx| {
        fixture.root.update(cx, |root, cx| {
            root.projection.as_mut().unwrap().mounted = true;
            cx.notify();
        });
        fixture.card.update(cx, |_, cx| cx.notify());
        fixture.stream.read(cx).list.remeasure();
    });
    fixture.wait(
        "same numeric layout remount has fresh owner session",
        |this| {
            this.visual.update(|window, cx| {
                let state = body.read(cx);
                state.valid(window, cx)
                    && state.mounted_content_current(cx)
                    && !state.projection.session.matches(&session)
                    && state
                        .source_mount
                        .as_ref()
                        .is_some_and(|current| current.get() && !Rc::ptr_eq(current, &mount))
            })
        },
    );
    fixture.visual.update(|_, cx| {
        let state = body.read(cx);
        let view = state.surface.as_ref().unwrap().entity();
        assert!(!view.read(cx).reading_geometry_is_current(&old.geometry));
        assert!(!view.read(cx).scroll_reading_anchor(
            &old.geometry,
            state.scroll_node.unwrap(),
            73.
        ));
    });
    fixture.stop();
}

pub fn source_caret_manual_scroll_rejects_pending_and_delayed_follow(cx: &mut TestAppContext) {
    let mut fixture = Fixture::new(
        cx,
        "source_caret_manual_scroll_rejects_pending_and_delayed_follow",
    );
    fixture.project_source(&caret_source(), 0, 47, 320.);
    fixture.move_source(381.25);
    let old = fixture.baseline();
    let body = fixture.body();
    let (session, ticket, area, node) = fixture.visual.update(|_, cx| {
        let state = body.read(cx);
        (
            state.projection.session.clone(),
            state.projection.scroll_ticket.clone(),
            state.area.unwrap(),
            state.scroll_node.unwrap(),
        )
    });
    fixture.resize_source(218.);
    let point = fixture.visual.update(|_, cx| {
        let host = body
            .read(cx)
            .lease
            .upgrade()
            .unwrap()
            .geometry
            .get()
            .unwrap();
        host.bounds
            .intersect(&host.clip)
            .intersect(&host.list)
            .center()
    });
    traced(
        fixture.case,
        "input.visual.update later real manual wheel",
        || {
            fixture.visual.update(|window, cx| {
                window.dispatch_event(
                    ScrollWheelEvent {
                        position: point,
                        delta: ScrollDelta::Lines(gpui_kit::point(0., -3.)),
                        touch_phase: TouchPhase::Moved,
                        ..Default::default()
                    }
                    .to_platform_input(),
                    cx,
                );
                body.update(cx, |state, _| {
                    assert!(state.source_anchor.is_none());
                    assert!(state.anchor_scroll.is_none());
                    assert!(!state.projection.scroll_ticket.matches(&ticket));
                    assert!(!state.observe_scroll(&session, area, node, &ticket, true));
                    assert!(!state.follow);
                });
            });
        },
    );
    fixture.drain("later real manual wheel");
    fixture.visual.update(|_, cx| {
        body.update(cx, |state, _| {
            let intent = state.scroll_intent;
            assert!(!state.observe_scroll(&session, area, node, &ticket, true));
            assert_eq!(state.scroll_intent, intent);
            assert!(!state.follow);
        })
    });
    fixture.wait("manual wheel captures a new source intent", |this| {
        this.visual.update(|_, cx| {
            body.read(cx)
                .source_anchor
                .as_ref()
                .is_some_and(|saved| saved.anchor.position != old.position && !saved.pending)
        })
    });
    let sample = fixture.source_sample().unwrap();
    let inset = fixture.visual.update(|_, cx| {
        let state = body.read(cx);
        measured_inset(
            &state.projection.content,
            &state.projection.source,
            &old,
            &sample,
        )
        .unwrap()
    });
    assert!((inset - old.inset).abs() > 1.);
    fixture.stop();
}

pub fn source_caret_follow_uses_actual_extent_after_append(cx: &mut TestAppContext) {
    let mut fixture = Fixture::new(cx, "source_caret_follow_uses_actual_extent_after_append");
    let source = caret_source();
    fixture.project_source(&source, 0, 48, 320.);
    fixture.baseline();
    let body = fixture.body();
    let sample = fixture.source_sample().unwrap();
    fixture.visual.update(|_, cx| {
        let state = body.read(cx);
        assert!(
            state
                .surface
                .as_ref()
                .unwrap()
                .entity()
                .read(cx)
                .scroll_reading_anchor(&sample.geometry, state.scroll_node.unwrap(), 1_000_000.)
        );
    });
    fixture.wait("actual native extent enables follow", |this| {
        this.visual.update(|_, cx| body.read(cx).follow)
    });
    fixture.wait("tail scrollbar idle", |this| {
        this.visual.update(|_, cx| {
            body.read(cx)
                .surface
                .as_ref()
                .unwrap()
                .entity()
                .read(cx)
                .integration_scroll_alpha()
                == Some(0.)
        })
    });
    fixture.project_source(&format!("{source}{}", caret_source()), 0, 48, 320.);
    fixture.wait("follow reaches appended native tail", |this| {
        let Some(sample) = this.source_sample() else {
            return false;
        };
        this.visual.update(|_, cx| {
            let state = body.read(cx);
            state.follow
                && state.source_anchor.is_none()
                && sample.anchor.position.source.start > source.len()
                && state
                    .surface
                    .as_ref()
                    .unwrap()
                    .entity()
                    .read(cx)
                    .integration_scroll_alpha()
                    == Some(0.)
                && sample.snapshot.texts.last().is_some_and(|text| {
                    let offset = text.content.text.encode_utf16().count() as u32;
                    sample
                        .geometry
                        .caret_rect(
                            sample.snapshot.texts.len() - 1,
                            nsh_core::TextPosition::new(offset, nsh_core::Affinity::Upstream),
                        )
                        .is_some_and(|caret| {
                            caret[1] >= sample.viewport[1]
                                && caret[1] + caret[3]
                                    <= sample.viewport[1] + sample.viewport[3] + 0.75
                        })
                })
                && !state
                    .surface
                    .as_ref()
                    .unwrap()
                    .entity()
                    .read(cx)
                    .scroll_reading_anchor(&sample.geometry, state.scroll_node.unwrap(), 1.)
        })
    });
    fixture.stop();
}

pub fn source_caret_selection_revokes_pending_reflow(cx: &mut TestAppContext) {
    let mut fixture = Fixture::new(cx, "source_caret_selection_revokes_pending_reflow");
    fixture.project_source(&caret_source(), 0, 49, 320.);
    fixture.move_source(281.25);
    fixture.baseline();
    let body = fixture.body();
    let sample = fixture.source_sample().unwrap();
    fixture.visual.update(|_, cx| {
        let state = body.read(cx);
        let text = &sample.snapshot.texts[0];
        let tag = &text.content.test_ids[0];
        let start = tag.start;
        let end = text.content.text.encode_utf16().count() as u32;
        assert!(end > start);
        assert!(
            state
                .surface
                .as_ref()
                .unwrap()
                .entity()
                .read(cx)
                .install_reading_selection(&sample.snapshot, (0, start), (0, end))
                .is_some()
        );
    });
    fixture.resize_source(218.);
    fixture.wait("real native selection blocks source restoration", |this| {
        this.visual.update(|_, cx| {
            let state = body.read(cx);
            let view = state.surface.as_ref().unwrap().entity();
            let snapshot = view.read(cx).reading_snapshot(state.area.unwrap()).unwrap();
            snapshot.ranges.iter().any(|(_, range)| !range.is_empty())
                && state.source_anchor.is_none()
                && !state.follow
        })
    });
    fixture.stop();
}

pub fn source_caret_new_projection_rejects_delayed_session_report(cx: &mut TestAppContext) {
    let mut fixture = Fixture::new(
        cx,
        "source_caret_new_projection_rejects_delayed_session_report",
    );
    let source = caret_source();
    fixture.project_source(&source, 0, 50, 320.);
    fixture.move_source(311.25);
    let anchor = fixture.baseline();
    let body = fixture.body();
    let (session, ticket, area, node) = fixture.visual.update(|_, cx| {
        let state = body.read(cx);
        (
            state.projection.session.clone(),
            state.projection.scroll_ticket.clone(),
            state.area.unwrap(),
            state.scroll_node.unwrap(),
        )
    });
    fixture.project_source(&format!("{source} extra retained source"), 0, 50, 320.);
    fixture.visual.update(|_, cx| {
        body.update(cx, |state, _| {
            assert!(!state.projection.session.matches(&session));
            let intent = state.scroll_intent;
            assert!(!state.observe_scroll(&session, area, node, &ticket, true));
            assert_eq!(state.scroll_intent, intent);
            assert!(!state.follow);
        })
    });
    fixture.assert_caret(&anchor);
    fixture.stop();
}

fn success() -> LinkPreviewData {
    LinkPreviewData {
        original_target: TARGET.into(),
        effective_target: TARGET.into(),
        host: "example.com".into(),
        site: LinkSiteKind::Web,
        title: Some("Real mounted reading preview".into()),
        description: None,
        image: None,
    }
}

pub fn stationary_thinking_hover_survives_fade_open_and_retry(cx: &mut TestAppContext) {
    let case = "stationary_thinking_hover_survives_fade_open_and_retry";
    let _stage = Stage::begin(case, "case");
    let mut fixture = Fixture::new(cx, case);
    let geometry = fixture.hover_after_wheel();
    fixture.fail_initial_request();
    let request = fixture.retry();
    request.completion.send(Ok(success())).unwrap();
    fixture.wait("real retry success card", |this| {
        this.visual.update(|window, cx| {
            integration_support::observe(window, cx)
                .and_then(|state| state.active)
                .is_some_and(|active| active.current && active.data == Some(Ok(success())))
        })
    });
    let body = fixture.body();
    fixture.visual.update(|window, cx| {
        assert!(body.read(cx).valid(window, cx));
        assert!(
            body.read(cx)
                .surface
                .as_ref()
                .unwrap()
                .entity()
                .read(cx)
                .reading_geometry_is_current(&geometry)
        );
    });
    fixture.focus_open();
    fixture.keys("enter");
    assert_eq!(fixture.visual.cx.opened_url().as_deref(), Some(TARGET));
    assert_eq!(fixture.requests.len(), 0);
    fixture.stop();
}

pub fn revoked_thinking_root_rejects_open_retry_and_delayed_completion(cx: &mut TestAppContext) {
    let case = "revoked_thinking_root_rejects_open_retry_and_delayed_completion";
    let _stage = Stage::begin(case, "case");
    let mut fixture = Fixture::new(cx, case);
    fixture.hover_after_wheel();
    fixture.fail_initial_request();
    let request = fixture.retry();
    let body = fixture.body();
    fixture.focus_open();
    fixture.visual.update(|window, cx| {
        assert!(body.read(cx).valid(window, cx));
        fixture.root.update(cx, |root, _| {
            assert_eq!(root.stream.take(), Some(fixture.stream.clone()));
        });
        assert!(!body.read(cx).valid(window, cx));
        let state = integration_support::observe(window, cx).unwrap();
        assert!(!state.active.unwrap().current);
        assert!(state.visible.iter().all(|target| !target.current));
    });
    let opened_before = fixture.visual.cx.opened_url();
    fixture.keys("enter tab enter");
    assert_eq!(fixture.visual.cx.opened_url(), opened_before);
    assert_eq!(fixture.requests.len(), 0);
    assert!(request.cancellation.is_cancelled());
    request.completion.send(Ok(success())).unwrap();
    fixture.wait(
        "revoked retry completion consumed without resurrection",
        |this| {
            let completed = request.completed.load(Ordering::SeqCst);
            let rejected = this.visual.update(|window, cx| {
                assert!(!body.read(cx).valid(window, cx));
                integration_support::observe(window, cx).is_none_or(|state| state.active.is_none())
            });
            completed && rejected
        },
    );
    assert_eq!(fixture.requests.len(), 0);
    assert_eq!(fixture.visual.cx.opened_url(), opened_before);
    fixture.stop();
}
