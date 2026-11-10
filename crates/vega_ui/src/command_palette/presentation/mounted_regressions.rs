use super::*;
use nsh_platform_gpui::{
    AppView,
    editing::{InputEdit, InputKey},
};
use std::time::{Duration, Instant};

static PROJECTIONS: Mutex<Vec<(ProjectionId, Rendered)>> = Mutex::new(Vec::new());

struct Rendered {
    projection: ProjectionId,
    selected: usize,
    scope: PaletteScope,
    revision: u64,
    composing: bool,
    focused: bool,
}

pub(super) fn observe_projection(snapshot: &Snapshot, composing: bool, focused: bool) {
    let Some(draft) = snapshot.draft.read() else {
        return;
    };
    let rendered = Rendered {
        projection: snapshot.projection.clone(),
        selected: snapshot.selected,
        scope: snapshot.scope,
        revision: draft.revision,
        composing,
        focused,
    };
    let mut projections = PROJECTIONS.lock().expect("palette projection observations");
    if let Some((_, projection)) = projections
        .iter_mut()
        .find(|(session, _)| session.matches(&snapshot.observed.session))
    {
        *projection = rendered;
    } else {
        projections.push((snapshot.observed.session.clone(), rendered));
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Events {
    queries: Vec<String>,
    activated: Vec<PaletteTarget>,
    closed: usize,
}

struct Root {
    palette: Entity<CommandPalette>,
    shown: bool,
    events: Events,
}

impl Render for Root {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .when(self.shown, |root| root.child(self.palette.clone()))
    }
}

struct Fixture {
    visual: VisualTestContext,
    root: Entity<Root>,
    palette: Entity<CommandPalette>,
    app: Entity<AppView>,
    node: u32,
    focus: InputEdit,
}

fn results() -> PaletteSearch {
    PaletteSearch {
        tasks: (0..2)
            .map(|index| PaletteTask {
                id: format!("task-{index}"),
                project_id: "project".into(),
                title: format!("日本語 task {index}"),
                project_name: "Project".into(),
            })
            .collect(),
        files: vec!["src/first.rs".into(), "src/second.rs".into()],
        files_unavailable: false,
    }
}

fn key(virtual_key: u16, shift: bool, down: bool, repeat: bool) -> InputKey {
    InputKey {
        virtual_key,
        shift,
        control: false,
        alt: false,
        meta: false,
        down,
        repeat,
        context: None,
    }
}

fn gesture(app: &AppView, virtual_key: u16, shift: bool, native: bool) {
    for (down, repeat) in [(true, false), (true, true), (true, false), (false, false)] {
        let handled = app.integration_edit_key(key(virtual_key, shift, down, repeat));
        if native {
            assert!(
                !handled,
                "composition key consumed: {virtual_key:#x}, shift={shift}, down={down}, repeat={repeat}"
            );
        } else if down {
            assert!(handled, "ordinary palette key unhandled: {virtual_key:#x}");
        }
    }
}

fn drain(visual: &mut VisualTestContext) {
    let start = Instant::now();
    for tick in 0..512 {
        let worked = visual.cx.background_executor.tick();
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "palette scheduler deadline tick={tick}"
        );
        if !worked {
            return;
        }
    }
    panic!("palette scheduler did not quiesce in 512 ticks");
}

fn frame(visual: &mut VisualTestContext) {
    drain(visual);
    visual.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    drain(visual);
}

fn wait(visual: &mut VisualTestContext, stage: &str, mut ready: impl FnMut(&App) -> bool) {
    let start = Instant::now();
    loop {
        frame(visual);
        if visual.cx.read(&mut ready) {
            return;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "palette stage: {stage}"
        );
        std::thread::sleep(Duration::from_millis(10));
        visual
            .cx
            .background_executor
            .advance_clock(Duration::from_millis(10));
    }
}

impl Fixture {
    fn new(cx: &mut TestAppContext) -> Self {
        cx.update(|cx| {
            assert!(!cx.text_system().all_font_names().is_empty());
            cx.set_global(vega_theme::Theme::dark());
        });
        let window = cx.open_window(size(px(960.), px(720.)), |_, cx| {
            let palette = cx.new(|cx| {
                let mut palette = CommandPalette::new(
                    vec![
                        PaletteAction::NewTask,
                        PaletteAction::Settings,
                        PaletteAction::Terminal,
                    ],
                    cx,
                );
                palette.apply(Ok(results()), cx);
                palette
            });
            cx.subscribe(
                &palette,
                |root: &mut Root, _, event: &PaletteQueryChanged, _| {
                    root.events.queries.push(event.0.clone());
                },
            )
            .detach();
            cx.subscribe(
                &palette,
                |root: &mut Root, _, event: &PaletteActivated, _| {
                    root.events.activated.push(event.0.clone());
                },
            )
            .detach();
            cx.subscribe(&palette, |root: &mut Root, _, _: &PaletteClosed, _| {
                root.events.closed += 1;
            })
            .detach();
            Root {
                palette,
                shown: true,
                events: Events::default(),
            }
        });
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let root = visual.update(|window, _| {
            window.activate_window();
            window
                .root::<Root>()
                .flatten()
                .expect("palette fixture root")
        });
        let palette = root.read_with(&visual.cx, |root, _| root.palette.clone());
        wait(&mut visual, "production Surface and real input", |cx| {
            palette
                .read(cx)
                .presentation
                .as_ref()
                .is_some_and(|presentation| {
                    let app = presentation.surface.entity();
                    (0..4096).any(|node| app.read(cx).integration_edit_snapshot(node).is_some())
                })
        });
        let app = palette.read_with(&visual.cx, |palette, _| {
            palette
                .presentation
                .as_ref()
                .expect("mounted palette")
                .surface
                .entity()
        });
        let nodes: Vec<_> = visual.cx.read(|cx| {
            (0..4096)
                .filter(|node| app.read(cx).integration_edit_snapshot(*node).is_some())
                .collect()
        });
        assert_eq!(
            nodes.len(),
            1,
            "expected exactly one production Input in bounded node range"
        );
        let node = nodes[0];
        visual.update(|window, cx| {
            window.focus(&palette.read(cx).focus_handle(cx), cx);
            assert!(app.read(cx).integration_edit_focus(node));
        });
        wait(&mut visual, "focused production render", |cx| {
            projection_current(&palette, cx)
        });
        let focus = app.read_with(&visual.cx, |app, _| {
            app.integration_edit_snapshot(node).expect("native input")
        });
        let mut fixture = Self {
            visual,
            root,
            palette,
            app,
            node,
            focus,
        };
        fixture.press(0x28, false);
        fixture.wait_state(1, PaletteScope::All);
        fixture.press(0x26, false);
        fixture.wait_state(0, PaletteScope::All);
        fixture.assert_events(Events::default());
        fixture
    }

    fn snapshot(&self) -> InputEdit {
        self.app.read_with(&self.visual.cx, |app, _| {
            app.integration_edit_snapshot(self.node)
                .expect("live production input")
        })
    }

    fn assert_focus(&mut self) {
        assert!(
            self.focus.identity.same_focus(&self.snapshot().identity),
            "native input focus or mount changed"
        );
        self.visual.update(|window, cx| {
            assert!(
                self.palette.read(cx).focus_handle(cx).is_focused(window),
                "actual GPUI palette focus changed"
            );
        });
    }

    fn assert_events(&self, expected: Events) {
        self.root
            .read_with(&self.visual.cx, |root, _| assert_eq!(root.events, expected));
    }

    fn wait_state(&mut self, selected: usize, scope: PaletteScope) {
        let palette = self.palette.clone();
        wait(
            &mut self.visual,
            "controller and rendered projection",
            |cx| {
                let state = palette.read(cx);
                state.selected == selected
                    && state.scope == scope
                    && projection_current(&palette, cx)
            },
        );
        self.assert_focus();
    }

    fn press(&mut self, virtual_key: u16, shift: bool) {
        self.visual.update(|_, cx| {
            let app = self.app.read(cx);
            assert!(
                app.integration_edit_key(key(virtual_key, shift, true, false)),
                "ordinary production binding missing"
            );
            app.integration_edit_key(key(virtual_key, shift, false, false));
        });
    }

    fn pulse(&mut self, virtual_key: u16, shift: bool) {
        self.visual
            .update(|_, cx| gesture(self.app.read(cx), virtual_key, shift, false));
    }

    fn settle(&mut self) {
        let palette = self.palette.clone();
        wait(&mut self.visual, "current production projection", |cx| {
            projection_current(&palette, cx)
        });
        for _ in 0..3 {
            frame(&mut self.visual);
        }
        self.assert_focus();
    }

    fn commit_composition(&mut self) {
        self.visual
            .update(|_, cx| self.app.read(cx).integration_edit_unmark());
        let palette = self.palette.clone();
        let root = self.root.clone();
        wait(&mut self.visual, "committed composition query", |cx| {
            palette.read(cx).query() == "日本語"
                && root.read(cx).events.queries.len() == 1
                && projection_current(&palette, cx)
        });
        self.settle();
        assert!(!self.snapshot().composing);
        self.palette.read_with(&self.visual.cx, |palette, _| {
            assert!(!palette.draft.read().expect("draft").composing);
        });
        assert_eq!(self.snapshot().text, "日本語");
        self.assert_events(Events {
            queries: vec!["日本語".into()],
            ..Events::default()
        });
        self.palette.update(&mut self.visual.cx, |palette, cx| {
            palette.apply(Ok(results()), cx)
        });
        self.wait_state(0, PaletteScope::All);
    }

    fn recovered_keys(&mut self) {
        self.press(0x28, false);
        self.wait_state(1, PaletteScope::All);
        self.press(0x26, false);
        self.wait_state(0, PaletteScope::All);
        self.pulse(0x09, false);
        self.wait_state(0, PaletteScope::Actions);
        self.pulse(0x09, true);
        self.wait_state(0, PaletteScope::All);
        self.pulse(0x0d, false);
        let root = self.root.clone();
        wait(&mut self.visual, "one post-composition activation", |cx| {
            root.read(cx).events.activated.len() == 1
        });
        self.settle();
        self.assert_events(Events {
            queries: vec!["日本語".into()],
            activated: vec![PaletteTarget::Task(results().tasks.remove(0))],
            closed: 0,
        });
    }

    fn capture(&self, operation: Operation) -> Request {
        self.palette.read_with(&self.visual.cx, |palette, _| {
            let observed = palette.draft.read().expect("captured draft");
            Request {
                session: observed.session.clone(),
                projection: palette.projection.clone(),
                observed,
                operation,
            }
        })
    }

    fn deliver(&mut self, request: Request) {
        self.visual.update(|window, cx| {
            self.palette
                .update(cx, |palette, cx| palette.receive(request, window, cx))
        });
    }
}

fn projection_current(palette: &Entity<CommandPalette>, cx: &App) -> bool {
    let palette = palette.read(cx);
    let Some(presentation) = palette.presentation.as_ref() else {
        return false;
    };
    assert!(!presentation.failed(), "production palette Surface failed");
    PROJECTIONS
        .lock()
        .expect("palette projection observations")
        .iter()
        .any(|(session, rendered)| {
            session.matches(&presentation.session)
                && rendered.projection.matches(&palette.projection)
                && rendered.selected == palette.selected
                && rendered.scope == palette.scope
                && rendered.focused
                && palette.draft.read().is_some_and(|draft| {
                    rendered.revision == draft.revision && rendered.composing == draft.composing
                })
        })
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let session = self.palette.read_with(&self.visual.cx, |palette, _| {
            palette
                .presentation
                .as_ref()
                .map(|presentation| presentation.session.clone())
        });
        self.visual.update(|window, cx| {
            if let Some(presentation) = &self.palette.read(cx).presentation {
                presentation.stop(cx);
            }
            window.remove_window();
        });
        if let Some(session) = session {
            PROJECTIONS
                .lock()
                .expect("palette projection observations")
                .retain(|(observed, _)| !observed.matches(&session));
        }
    }
}

pub fn marked_palette_keys_leave_controller_and_focus_unchanged(cx: &mut TestAppContext) {
    for (virtual_key, shift) in [
        (0x26, false),
        (0x28, false),
        (0x0d, false),
        (0x1b, false),
        (0x09, false),
        (0x09, true),
    ] {
        let mut fixture = Fixture::new(cx);
        fixture.press(0x28, false);
        fixture.wait_state(1, PaletteScope::All);
        fixture.settle();
        let shared = fixture
            .palette
            .read_with(&fixture.visual.cx, |palette, _| palette.draft.clone());
        fixture.visual.update(|window, cx| {
            let guard = shared
                .locked()
                .expect("hold composition callback before keymap update");
            assert!(!guard.composing);
            let app = fixture.app.read(cx);
            let before = app
                .integration_edit_snapshot(fixture.node)
                .expect("before mark");
            assert!(!before.composing);
            assert!(app.integration_edit_mark(fixture.node, "日本語"));
            let marked = app
                .integration_edit_snapshot(fixture.node)
                .expect("immediate native composition");
            assert!(marked.composing);
            assert_eq!(marked.text, "日本語");
            assert!(before.identity.same_focus(&marked.identity));
            gesture(app, virtual_key, shift, true);
            assert!(
                !guard.composing,
                "engine callback advanced while draft lock held"
            );
            let palette = fixture.palette.read(cx);
            assert_eq!(palette.selected, 1);
            assert_eq!(palette.scope, PaletteScope::All);
            assert_eq!(palette.query(), "");
            assert!(palette.focus_handle(cx).is_focused(window));
            assert_eq!(fixture.root.read(cx).events, Events::default());
        });
        let palette = fixture.palette.clone();
        wait(
            &mut fixture.visual,
            "composition reaches production shared draft",
            |cx| palette.read(cx).draft.read().expect("draft").composing,
        );
        fixture.settle();
        fixture.wait_state(1, PaletteScope::All);
        fixture.assert_events(Events::default());
        fixture
            .visual
            .update(|_, cx| gesture(fixture.app.read(cx), virtual_key, shift, true));
        fixture.settle();
        assert!(fixture.snapshot().composing);
        assert_eq!(fixture.snapshot().text, "日本語");
        fixture.palette.read_with(&fixture.visual.cx, |palette, _| {
            assert_eq!(palette.query(), "")
        });
        fixture.wait_state(1, PaletteScope::All);
        fixture.assert_events(Events::default());
        fixture.commit_composition();
        fixture.recovered_keys();
    }
}

pub fn ordinary_palette_keys_repeat_only_navigation_and_emit_exact_events(cx: &mut TestAppContext) {
    let mut fixture = Fixture::new(cx);
    fixture.pulse(0x28, false);
    fixture.wait_state(3, PaletteScope::All);
    fixture.pulse(0x26, false);
    fixture.wait_state(0, PaletteScope::All);
    for scope in [
        PaletteScope::Actions,
        PaletteScope::Tasks,
        PaletteScope::Files,
        PaletteScope::All,
    ] {
        fixture.pulse(0x09, false);
        fixture.wait_state(0, scope);
    }
    fixture.pulse(0x09, true);
    fixture.wait_state(0, PaletteScope::Files);
    fixture.pulse(0x09, false);
    fixture.wait_state(0, PaletteScope::All);
    fixture.pulse(0x0d, false);
    let root = fixture.root.clone();
    wait(&mut fixture.visual, "single ordinary activation", |cx| {
        root.read(cx).events.activated.len() == 1
    });
    fixture.pulse(0x1b, false);
    let root = fixture.root.clone();
    wait(&mut fixture.visual, "single ordinary close", |cx| {
        root.read(cx).events.closed == 1
    });
    fixture.settle();
    fixture.assert_events(Events {
        queries: Vec::new(),
        activated: vec![PaletteTarget::Action(PaletteAction::NewTask)],
        closed: 1,
    });
    fixture.visual.update(|_, cx| {
        assert!(
            fixture
                .app
                .read(cx)
                .integration_edit_type(fixture.node, "日本語")
        )
    });
    let palette = fixture.palette.clone();
    wait(&mut fixture.visual, "native typed query", |cx| {
        palette.read(cx).query() == "日本語" && projection_current(&palette, cx)
    });
    fixture.settle();
    fixture.assert_events(Events {
        queries: vec!["日本語".into()],
        activated: vec![PaletteTarget::Action(PaletteAction::NewTask)],
        closed: 1,
    });
    assert_eq!(fixture.snapshot().text, "日本語");
}

pub fn retired_palette_requests_cannot_mutate_mounted_controller(cx: &mut TestAppContext) {
    let mut fixture = Fixture::new(cx);
    let stale_draft = fixture.capture(Operation::Accept);
    fixture.visual.update(|_, cx| {
        assert!(
            fixture
                .app
                .read(cx)
                .integration_edit_type(fixture.node, "日本語")
        )
    });
    let palette = fixture.palette.clone();
    wait(&mut fixture.visual, "new draft query", |cx| {
        palette.read(cx).query() == "日本語" && projection_current(&palette, cx)
    });
    fixture.deliver(stale_draft);
    let stale_projection = fixture.capture(Operation::Dismiss);
    fixture
        .palette
        .update(&mut fixture.visual.cx, |palette, cx| {
            palette.apply(Ok(results()), cx)
        });
    fixture.wait_state(0, PaletteScope::All);
    fixture.deliver(stale_projection);
    fixture.settle();
    fixture.assert_events(Events {
        queries: vec!["日本語".into()],
        ..Events::default()
    });
    let operations = [
        Operation::Accept,
        Operation::Dismiss,
        Operation::Next,
        Operation::CycleScope(false),
    ];
    let requests: Vec<_> = operations
        .into_iter()
        .map(|operation| fixture.capture(operation))
        .collect();
    fixture.root.update(&mut fixture.visual.cx, |root, cx| {
        root.shown = false;
        cx.notify();
    });
    let palette = fixture.palette.clone();
    wait(&mut fixture.visual, "actual mounted lease disposal", |cx| {
        palette
            .read(cx)
            .presentation
            .as_ref()
            .expect("retained controller")
            .lease
            .upgrade()
            .is_none()
    });
    for request in requests {
        fixture.deliver(request);
    }
    for _ in 0..3 {
        frame(&mut fixture.visual);
    }
    fixture.palette.read_with(&fixture.visual.cx, |palette, _| {
        assert_eq!(palette.selected, 0);
        assert_eq!(palette.scope, PaletteScope::All);
    });
    fixture.assert_events(Events {
        queries: vec!["日本語".into()],
        ..Events::default()
    });
}
