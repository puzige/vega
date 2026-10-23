use super::*;
use std::{cell::RefCell, f32::consts::FRAC_PI_2, rc::Rc, sync::Arc, time::Duration};

use crate::disclosure_paint_probe::{self as probe, GlyphPaint};
use crate::icons::Icon;
use gpui_kit::{
    AppContext, Bounds, HeadlessAppContext, InputEvent, Keystroke, Modifiers, MouseMoveEvent,
    NoopTextSystem, Pixels, Point, WindowHandle, point, size,
};
use vega_conversation::types::{
    AssistantResponseKind, AssistantResponseMetadata, PermissionMode, TaskBackend, ThreadMode,
    ThreadStatus, ToolCall, ToolCallStatus, ToolResult,
};

const TURN: Duration = Duration::from_millis(120);
const EPSILON: f32 = 0.003;
const SOURCE: &str =
    "Thinking source remains selectable and complete.\nSecond line remains available.\n";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Entry {
    Thinking,
    Tool,
    ToolGroup,
    Run,
    Stage,
}

const ENTRIES: [Entry; 5] = [
    Entry::Thinking,
    Entry::Tool,
    Entry::ToolGroup,
    Entry::Run,
    Entry::Stage,
];

#[derive(Default)]
struct Geometry {
    surface: Option<Bounds<Pixels>>,
    root_renders: usize,
}

struct ProductionHeader {
    entry: Entry,
    thinking: Entity<ThinkingBlock>,
    tool: Entity<ToolCard>,
    tools: Entity<ToolActivityGroup>,
    run: Entity<RunActivityGroup>,
    geometry: Rc<RefCell<Geometry>>,
    mounted: bool,
    width: f32,
    clipped_offscreen: bool,
    hidden: bool,
}

fn tool(call_id: &str, cx: &mut App) -> Entity<ToolCard> {
    let call = ToolCall {
        id: call_id.into(),
        tool: "bash".into(),
        input_json: r#"{"command":"printf disclosure-output"}"#.into(),
    };
    let mut card = ToolCard::proposed(&call);
    assert!(card.apply_finished(&ToolResult {
        status: ToolCallStatus::Success,
        output: "disclosure-output\n".repeat(80),
        reused: true,
        exit_code: Some(0),
        duration_ms: Some(30),
        truncated: None,
        invalid: None,
    }));
    cx.new(|_| card)
}

impl ProductionHeader {
    fn new(entry: Entry, expanded: bool, cx: &mut Context<Self>) -> Self {
        let thinking = cx.new(|cx| {
            let mut card = ThinkingBlock::new(cx);
            card.append(SOURCE, false, THINKING_VIEW_BYTES);
            card.progress_finished = true;
            card.set_expanded(expanded, cx);
            card
        });
        let first = tool("first", cx);
        let second = tool("second", cx);
        first.update(cx, |card, cx| card.set_expanded(expanded, cx));
        let tools =
            cx.new(|_| ToolActivityGroup::from_children(vec![first.clone(), second], expanded));
        let run = cx.new(|cx| {
            let mut group =
                RunActivityGroup::new(RunActivityStatus::Completed, Some(1200), expanded, cx);
            let segment = group.start_segment(cx);
            if matches!(entry, Entry::Run | Entry::Stage) {
                group.append_thinking(segment, thinking.clone(), cx);
            }
            if entry == Entry::Run {
                group.confirm_final(HashSet::new(), false, cx);
                if expanded {
                    group.reveal(cx);
                }
            } else if entry == Entry::Stage && expanded {
                assert!(group.reveal_segment(segment, cx));
            }
            group
        });
        Self {
            entry,
            thinking,
            tool: first,
            tools,
            run,
            geometry: Rc::new(RefCell::new(Geometry::default())),
            mounted: true,
            width: 480.,
            clipped_offscreen: false,
            hidden: false,
        }
    }

    fn owner(&self) -> String {
        match self.entry {
            Entry::Thinking => format!("thinking-disclosure-{}", self.thinking.entity_id()),
            Entry::Tool => format!("tool-disclosure-{}", self.tool.entity_id()),
            Entry::ToolGroup => format!("tool-group-disclosure-{}", self.tools.entity_id()),
            Entry::Run => format!("run-disclosure-{}", self.run.entity_id()),
            Entry::Stage => format!("run-stage-{}-0", self.run.entity_id()),
        }
    }

    fn expanded(&self, cx: &App) -> bool {
        match self.entry {
            Entry::Thinking | Entry::Stage => self.thinking.read(cx).expanded,
            Entry::Tool => self.tool.read(cx).is_expanded(),
            Entry::ToolGroup => self.tools.read(cx).expanded(),
            Entry::Run => self.run.read(cx).expanded(),
        }
    }

    fn body_rows(&self, cx: &App) -> usize {
        match self.entry {
            Entry::Thinking | Entry::Stage => self.thinking.read(cx).row_count(),
            Entry::Tool => self.tool.read(cx).row_count(),
            Entry::ToolGroup => self.tools.read(cx).row_count(cx),
            Entry::Run => self.run.read(cx).children().len() * usize::from(self.expanded(cx)),
        }
    }
}

impl Render for ProductionHeader {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.geometry.borrow_mut().root_renders += 1;
        let geometry = self.geometry.clone();
        let content = if !self.mounted {
            div().into_any_element()
        } else {
            match self.entry {
                Entry::Thinking => self.thinking.clone().into_any_element(),
                Entry::Tool => ToolCard::render(self.tool.clone(), false, window, cx),
                Entry::ToolGroup => ToolActivityGroup::render(self.tools.clone(), window, cx),
                Entry::Run => RunActivityGroup::render(self.run.clone(), window, cx),
                Entry::Stage => RunActivityGroup::render_segment(self.run.clone(), 0, window, cx),
            }
        };
        div()
            .w(px(self.width))
            .h(px(720.))
            .overflow_hidden()
            .child(gpui_kit::base::TextSelectionLayer)
            .child(
                div()
                    .relative()
                    .top(px(if self.clipped_offscreen { 900. } else { 0. }))
                    .when(self.hidden, |content| content.invisible())
                    .child(MeasuredContent {
                        inner: content,
                        geometry,
                    }),
            )
    }
}

struct MeasuredContent {
    inner: AnyElement,
    geometry: Rc<RefCell<Geometry>>,
}

impl IntoElement for MeasuredContent {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl gpui_kit::Element for MeasuredContent {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<gpui_kit::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&gpui_kit::GlobalElementId>,
        _: Option<&gpui_kit::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (gpui_kit::LayoutId, Self::RequestLayoutState) {
        (self.inner.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&gpui_kit::GlobalElementId>,
        _: Option<&gpui_kit::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.geometry.borrow_mut().surface = Some(bounds);
        self.inner.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&gpui_kit::GlobalElementId>,
        _: Option<&gpui_kit::InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.inner.paint(window, cx);
    }
}

struct Fixture {
    cx: HeadlessAppContext,
    window: WindowHandle<ProductionHeader>,
    owner: String,
}

impl Fixture {
    fn open(entry: Entry, expanded: bool, dark: bool) -> Self {
        let mut cx = HeadlessAppContext::with_asset_source(
            Arc::new(NoopTextSystem),
            Arc::new(gpui_kit::assets::Assets),
        );
        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_global(if dark {
                vega_theme::Theme::dark()
            } else {
                vega_theme::Theme::light()
            });
        });
        let window = cx
            .open_window(size(px(640.), px(720.)), |_, cx| {
                cx.new(|cx| ProductionHeader::new(entry, expanded, cx))
            })
            .expect("production header window");
        let owner = window
            .update(&mut cx, |view, window, _| {
                window.activate_window();
                view.owner()
            })
            .expect("production header owner");
        cx.run_until_parked();
        Self { cx, window, owner }
    }

    fn draw(&mut self) -> GlyphPaint {
        probe::take();
        self.cx
            .update_window(self.window.into(), |_, window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
            })
            .expect("draw real production tree");
        probe::take()
            .into_iter()
            .rev()
            .find(|paint| paint.scope.owner.as_ref() == self.owner)
            .expect("WIRING: production disclosure glyph never reached delegated paint")
    }

    fn expanded(&mut self) -> bool {
        self.window
            .update(&mut self.cx, |view, _, cx| view.expanded(cx))
            .unwrap()
    }

    fn body_rows(&mut self) -> usize {
        self.window
            .update(&mut self.cx, |view, _, cx| view.body_rows(cx))
            .unwrap()
    }

    fn root_renders(&mut self) -> usize {
        self.window
            .update(&mut self.cx, |view, _, _| {
                view.geometry.borrow().root_renders
            })
            .unwrap()
    }

    fn geometry(&mut self) -> Bounds<Pixels> {
        self.window
            .update(&mut self.cx, |view, _, _| {
                view.geometry
                    .borrow()
                    .surface
                    .expect("painted production surface")
            })
            .unwrap()
    }

    fn key(&mut self, key: &str) {
        let focus = self.draw().scope.focus;
        self.cx
            .update_window(self.window.into(), |_, window, cx| {
                focus.focus(window, cx);
                assert!(
                    focus.is_focused(window),
                    "WIRING: actual production header focus"
                );
                window.dispatch_keystroke(Keystroke::parse(key).unwrap(), cx);
            })
            .unwrap();
    }

    fn hover(&mut self, position: Point<Pixels>) {
        self.cx
            .update_window(self.window.into(), |_, window, cx| {
                window.dispatch_event(
                    MouseMoveEvent {
                        position,
                        pressed_button: None,
                        modifiers: Modifiers::default(),
                    }
                    .to_platform_input(),
                    cx,
                );
            })
            .unwrap();
    }

    fn click(&mut self, paint: &GlyphPaint) {
        self.cx
            .update_window(self.window.into(), |_, window, cx| {
                let position = paint.bounds.center();
                window.dispatch_event(
                    gpui_kit::MouseDownEvent {
                        button: MouseButton::Left,
                        position,
                        modifiers: Modifiers::default(),
                        click_count: 1,
                        first_mouse: false,
                    }
                    .to_platform_input(),
                    cx,
                );
                window.dispatch_event(
                    MouseUpEvent {
                        button: MouseButton::Left,
                        position,
                        modifiers: Modifiers::default(),
                        click_count: 1,
                    }
                    .to_platform_input(),
                    cx,
                );
            })
            .unwrap();
    }

    fn tick(&mut self, elapsed: Duration) -> GlyphPaint {
        self.cx.advance_clock(elapsed);
        self.cx
            .update_window(self.window.into(), |_, window, cx| {
                window.simulate_next_frame(cx);
            })
            .unwrap();
        self.draw()
    }
}

fn displayed_angle(paint: &GlyphPaint) -> f32 {
    match paint.asset {
        Icon::ChevronRight => paint.rotation,
        Icon::ChevronDown => FRAC_PI_2 + paint.rotation,
        other => panic!("WIRING: unrelated glyph {other:?}"),
    }
}

fn assert_angle(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < EPSILON,
        "angle {actual}, expected {expected}"
    );
}

#[gpui_kit::test]
fn production_glyph_probe_reaches_both_helpers_and_real_endpoint_assets(
    _: &mut gpui_kit::TestAppContext,
) {
    for entry in ENTRIES {
        let mut fixture = Fixture::open(entry, false, false);
        let collapsed = fixture.draw();
        assert!(
            collapsed.scope.enabled,
            "WIRING: {entry:?} fixture is not expandable"
        );
        assert!(!collapsed.scope.expanded);
        assert!(collapsed.asset_path.ends_with("chevron-right.svg"));
        assert!(collapsed.bounds.size.width > px(0.));
        assert!(collapsed.bounds.size.height > px(0.));
        assert_angle(displayed_angle(&collapsed), 0.);
        fixture.click(&collapsed);
        assert!(
            fixture.expanded(),
            "WIRING: real {entry:?} header rejected click"
        );
        let expanded = fixture.tick(TURN);
        assert!(expanded.scope.expanded);
        assert_angle(displayed_angle(&expanded), FRAC_PI_2);
        let expected_size = if entry == Entry::Thinking {
            Layout::ICON_SIZE
        } else {
            Layout::CONVERSATION_CHEVRON_SIZE
        };
        assert_eq!(
            expanded.bounds.size,
            size(px(expected_size), px(expected_size))
        );
        assert_eq!(expanded.scope.owner, collapsed.scope.owner);
    }
}

#[gpui_kit::test]
fn disclosure_turns_while_production_content_changes_in_the_same_event(
    _: &mut gpui_kit::TestAppContext,
) {
    for entry in ENTRIES {
        let mut fixture = Fixture::open(entry, false, false);
        let collapsed = fixture.draw();
        let rows = fixture.body_rows();
        fixture.click(&collapsed);
        assert!(
            fixture.expanded(),
            "{entry:?} expansion must not wait for motion"
        );
        assert!(
            fixture.body_rows() > rows,
            "{entry:?} real body not expanded"
        );
        let start = fixture.draw();
        assert!(start.scope.expanded);
        fixture.cx.run_until_parked();
        fixture.draw();
        let expanded_geometry = fixture.geometry();
        let middle = fixture.tick(Duration::from_millis(60));
        assert_eq!(middle.scope.owner, collapsed.scope.owner);
        assert_eq!(
            fixture.geometry(),
            expanded_geometry,
            "only the arrow may animate"
        );
        assert_eq!(middle.bounds, start.bounds, "fixed arrow slot");
        assert!(
            displayed_angle(&middle) > 0. && displayed_angle(&middle) < FRAC_PI_2,
            "BEHAVIOR: {entry:?} has no intermediate painted orientation: {middle:?}"
        );
        assert_angle(displayed_angle(&middle), FRAC_PI_2 * 0.875);
        let end = fixture.tick(Duration::from_millis(60));
        assert_eq!(
            end.asset,
            Icon::ChevronRight,
            "one glyph, not Right/Down replacement"
        );
        assert_angle(end.rotation, FRAC_PI_2);
        fixture.key("space");
        assert!(!fixture.expanded());
        assert_eq!(fixture.body_rows(), rows);
        let closing_start = fixture.draw();
        let closing_middle = fixture.tick(Duration::from_millis(60));
        assert_eq!(closing_start.bounds, closing_middle.bounds);
        assert_angle(displayed_angle(&closing_middle), FRAC_PI_2 * 0.125);
        assert_angle(
            displayed_angle(&fixture.tick(Duration::from_millis(60))),
            0.,
        );
    }
}

#[gpui_kit::test]
fn disclosure_retargets_from_the_painted_angle_without_queuing_or_resetting(
    _: &mut gpui_kit::TestAppContext,
) {
    for entry in ENTRIES {
        let mut fixture = Fixture::open(entry, false, false);
        fixture.draw();
        fixture.key("enter");
        fixture.draw();
        let before_reverse = fixture.tick(Duration::from_millis(40));
        fixture.key("space");
        let reversed = fixture.draw();
        assert_angle(displayed_angle(&reversed), displayed_angle(&before_reverse));
        let closing = fixture.tick(Duration::from_millis(20));
        assert!(displayed_angle(&closing) < displayed_angle(&reversed));
        fixture.key("enter");
        let second_reverse = fixture.draw();
        assert_angle(displayed_angle(&second_reverse), displayed_angle(&closing));
        assert_eq!(second_reverse.scope.owner, before_reverse.scope.owner);
        assert!(fixture.expanded());
        let settled = fixture.tick(TURN);
        assert_angle(displayed_angle(&settled), FRAC_PI_2);
        fixture
            .cx
            .update_window(fixture.window.into(), |_, window, cx| {
                window.simulate_next_frame(cx);
            })
            .unwrap();
        let pending = fixture
            .cx
            .update_window(fixture.window.into(), |_, window, cx| {
                window.simulate_next_frame(cx)
            })
            .unwrap();
        assert_eq!(
            pending, 0,
            "completed disclosure must not keep decorative callbacks alive"
        );
    }
}

#[gpui_kit::test]
fn initial_mount_remount_and_unrelated_stream_redraw_adopt_current_direction(
    _: &mut gpui_kit::TestAppContext,
) {
    for entry in ENTRIES {
        for expanded in [false, true] {
            let mut fixture = Fixture::open(entry, expanded, false);
            let initial = fixture.draw();
            assert_angle(
                displayed_angle(&initial),
                if expanded { FRAC_PI_2 } else { 0. },
            );
            let geometry = fixture.geometry();
            assert_eq!(fixture.draw().bounds, initial.bounds);
            assert_eq!(fixture.geometry(), geometry);
            fixture
                .window
                .update(&mut fixture.cx, |view, _, cx| {
                    view.mounted = false;
                    cx.notify();
                })
                .unwrap();
            fixture
                .cx
                .update_window(fixture.window.into(), |_, window, cx| {
                    window.refresh();
                    window.draw(cx).clear(cx);
                })
                .unwrap();
            fixture
                .window
                .update(&mut fixture.cx, |view, _, cx| {
                    view.mounted = true;
                    cx.notify();
                })
                .unwrap();
            let remount = fixture.draw();
            assert_eq!(remount.scope.owner, initial.scope.owner);
            assert_angle(displayed_angle(&remount), displayed_angle(&initial));
            assert_eq!(fixture.geometry(), geometry);
            let pending = fixture
                .cx
                .update_window(fixture.window.into(), |_, window, cx| {
                    window.simulate_next_frame(cx)
                })
                .unwrap();
            assert_eq!(
                pending, 0,
                "mounting a settled disclosure must not replay it"
            );
        }
    }
}

#[gpui_kit::test]
fn reduced_motion_and_offscreen_disclosures_settle_without_replay(
    _: &mut gpui_kit::TestAppContext,
) {
    for entry in ENTRIES {
        let mut fixture = Fixture::open(entry, false, false);
        fixture.draw();
        fixture.cx.update(|cx| cx.set_reduce_motion(true));
        fixture.key("enter");
        let reduced = fixture.draw();
        assert!(fixture.expanded());
        assert_angle(displayed_angle(&reduced), FRAC_PI_2);
        assert_eq!(
            fixture
                .cx
                .update_window(fixture.window.into(), |_, window, cx| window
                    .simulate_next_frame(cx))
                .unwrap(),
            0
        );
        fixture.cx.update(|cx| cx.set_reduce_motion(false));
        assert_angle(displayed_angle(&fixture.draw()), FRAC_PI_2);
        fixture
            .window
            .update(&mut fixture.cx, |view, _, cx| {
                view.mounted = false;
                cx.notify();
            })
            .unwrap();
        fixture.cx.advance_clock(TURN);
        fixture
            .cx
            .update_window(fixture.window.into(), |_, window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
                window.simulate_next_frame(cx);
            })
            .unwrap();
        fixture
            .window
            .update(&mut fixture.cx, |view, _, cx| {
                view.mounted = true;
                cx.notify();
            })
            .unwrap();
        assert_angle(displayed_angle(&fixture.draw()), FRAC_PI_2);
    }
}

#[gpui_kit::test]
fn long_bodies_keep_geometry_and_owner_identity_across_theme_and_window_width(
    _: &mut gpui_kit::TestAppContext,
) {
    for (entry, width, dark) in ENTRIES.into_iter().flat_map(|entry| {
        [240., 480.].into_iter().flat_map(move |width| {
            [false, true]
                .into_iter()
                .map(move |dark| (entry, width, dark))
        })
    }) {
        let mut fixture = Fixture::open(entry, false, dark);
        fixture
            .window
            .update(&mut fixture.cx, |view, _, cx| {
                view.width = width;
                cx.notify();
            })
            .unwrap();
        fixture.draw();
        fixture.key("enter");
        fixture.draw();
        fixture.cx.run_until_parked();
        let start = fixture.draw();
        let body = fixture.geometry();
        for _ in 0..8 {
            let frame = fixture.tick(Duration::from_millis(15));
            assert_eq!(frame.bounds, start.bounds);
            assert_eq!(fixture.geometry(), body);
            assert!(frame.scope.expanded);
        }
        assert_angle(displayed_angle(&fixture.draw()), FRAC_PI_2);
    }
}

#[gpui_kit::test]
fn empty_thinking_has_no_expand_action_or_keyboard_trap(_: &mut gpui_kit::TestAppContext) {
    let mut fixture = Fixture::open(Entry::Thinking, false, false);
    fixture
        .window
        .update(&mut fixture.cx, |view, _, cx| {
            view.thinking = cx.new(ThinkingBlock::new);
            cx.notify();
        })
        .unwrap();
    fixture.owner = fixture
        .window
        .update(&mut fixture.cx, |view, _, _| view.owner())
        .unwrap();
    let paint = fixture.draw();
    assert!(!paint.scope.enabled);
    assert_eq!(paint.opacity, 0.);
    fixture.click(&paint);
    fixture.key("space");
    assert!(!fixture.expanded());
    assert_eq!(fixture.body_rows(), 1);
}

#[gpui_kit::test]
fn collapsed_arrows_remain_hover_and_keyboard_discoverable_without_extra_focus_targets(
    _: &mut gpui_kit::TestAppContext,
) {
    for entry in ENTRIES {
        let mut fixture = Fixture::open(entry, false, false);
        let initial = fixture.draw();
        fixture.hover(point(px(600.), px(650.)));
        assert_eq!(fixture.draw().opacity, 0.);
        fixture.hover(initial.bounds.center());
        assert_eq!(fixture.draw().opacity, 1.);
        assert!(
            !fixture.expanded(),
            "hover must not toggle production state"
        );
        fixture.hover(point(px(600.), px(650.)));
        assert_eq!(fixture.draw().opacity, 0.);
        fixture
            .cx
            .update_window(fixture.window.into(), |_, window, cx| {
                initial.scope.focus.focus(window, cx);
                window.dispatch_keystroke(Keystroke::parse("shift").unwrap(), cx);
            })
            .unwrap();
        let keyboard = fixture.draw();
        assert_eq!(keyboard.opacity, 1.);
        assert_eq!(keyboard.bounds, initial.bounds);
        assert_eq!(keyboard.scope.focus, initial.scope.focus);
        fixture.key("space");
        assert!(fixture.expanded());
        fixture.hover(point(px(600.), px(650.)));
        assert_eq!(fixture.draw().opacity, 1.);
    }
}

const COMMENTARY: &str = "Process commentary that must collapse.\n";
const FINAL_TEXT: &str = "Final answer keeps its exact **source** and ending.\n";

fn thread() -> Thread {
    Thread {
        parent_thread_id: None,
        id: "disclosure-thread".into(),
        backend: TaskBackend::Native,
        project_id: String::new(),
        title: "Disclosure behavior".into(),
        mode: ThreadMode::Ask,
        permission_mode: PermissionMode::ReadOnly,
        model: String::new(),
        status: ThreadStatus::Active,
        pinned: false,
        unread: false,
        created_at: 0,
        updated_at: 0,
    }
}

fn response(
    id: &str,
    kind: AssistantResponseKind,
    start: usize,
    end: usize,
) -> AssistantResponseMetadata {
    AssistantResponseMetadata {
        response_id: id.into(),
        text_start_bytes: start,
        text_end_bytes: end,
        kind,
    }
}

fn begin_process(stream: &Entity<ConversationStream>, id: &str, cx: &mut App) {
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: id.into(),
                seq: 1,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::AssistantResponseStarted {
                message_id: id.into(),
                response_id: "commentary".into(),
                text_start_bytes: 0,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ThinkingDelta {
                message_id: id.into(),
                delta: SOURCE.repeat(12),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: id.into(),
                delta: COMMENTARY.into(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::AssistantResponseCompleted {
                message_id: id.into(),
                response: response(
                    "commentary",
                    AssistantResponseKind::Commentary,
                    0,
                    COMMENTARY.len(),
                ),
            },
            cx,
        );
    });
}

fn stage_final_text(stream: &Entity<ConversationStream>, id: &str, cx: &mut App) {
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::AssistantResponseStarted {
                message_id: id.into(),
                response_id: "final".into(),
                text_start_bytes: COMMENTARY.len(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: id.into(),
                delta: FINAL_TEXT.into(),
            },
            cx,
        );
    });
}

fn append_final(stream: &Entity<ConversationStream>, id: &str, end: usize, cx: &mut App) {
    stage_final_text(stream, id, cx);
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::AssistantResponseCompleted {
                message_id: id.into(),
                response: response("final", AssistantResponseKind::Final, COMMENTARY.len(), end),
            },
            cx,
        );
    });
}

struct StreamSurface {
    stream: Entity<ConversationStream>,
    artifact_preview: Option<Entity<ArtifactCard>>,
}

impl Render for StreamSurface {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let surface = div().size_full().child(gpui_kit::base::TextSelectionLayer);
        if let Some(card) = self.artifact_preview.clone() {
            let count = card.read(cx).row_count();
            let preview =
                gpui_kit::uniform_list("workspace-preview", count, move |range, window, cx| {
                    range
                        .map(|row| ArtifactCard::render_row(card.clone(), row, window, cx))
                        .collect()
                })
                .size_full();
            surface.child(
                div()
                    .size_full()
                    .flex()
                    .child(div().flex_1().min_w_0().h_full().child(self.stream.clone()))
                    .child(div().flex_1().min_w_0().h_full().child(preview)),
            )
        } else {
            surface.child(self.stream.clone())
        }
    }
}

struct StreamFixture {
    stream: Entity<ConversationStream>,
    window: WindowHandle<StreamSurface>,
    cx: HeadlessAppContext,
}

impl StreamFixture {
    fn open() -> Self {
        let mut cx = HeadlessAppContext::with_asset_source(
            Arc::new(NoopTextSystem),
            Arc::new(gpui_kit::assets::Assets),
        );
        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_global(vega_theme::Theme::light());
        });
        let stream = cx.new(|cx| ConversationStream::new(thread(), cx));
        let window = cx
            .open_window(size(px(640.), px(1200.)), |_, cx| {
                cx.new(|_| StreamSurface {
                    stream: stream.clone(),
                    artifact_preview: None,
                })
            })
            .unwrap();
        cx.update_window(window.into(), |_, window, _| window.activate_window())
            .unwrap();
        cx.run_until_parked();
        Self { cx, window, stream }
    }

    fn draw(&mut self) -> Vec<GlyphPaint> {
        probe::take();
        self.cx
            .update_window(self.window.into(), |_, window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
            })
            .unwrap();
        probe::take()
    }

    fn begin(&mut self, id: &str) -> (Entity<RunActivityGroup>, Entity<ThinkingBlock>) {
        self.cx.update(|cx| begin_process(&self.stream, id, cx));
        self.expand_thinking_stage(id)
    }

    fn expand_thinking_stage(
        &mut self,
        id: &str,
    ) -> (Entity<RunActivityGroup>, Entity<ThinkingBlock>) {
        let (group, thinking) = self.cx.update(|cx| {
            let stream = self.stream.read(cx);
            let group = stream.run_activity_groups[id].clone();
            let thinking = group
                .read(cx)
                .children()
                .into_iter()
                .find_map(|child| match child {
                    RunActivityChild::Thinking(card) => Some(card),
                    _ => None,
                })
                .expect("real ThinkingDelta produced source card");
            (group, thinking)
        });
        self.draw();
        let paint = self
            .draw()
            .into_iter()
            .find(|paint| {
                paint.scope.owner.as_ref() == format!("run-stage-{}-0", group.entity_id())
            })
            .expect("WIRING: actual process stage header is visible");
        self.cx
            .update_window(self.window.into(), |_, window, cx| {
                paint.scope.focus.focus(window, cx);
                window.dispatch_keystroke(Keystroke::parse("enter").unwrap(), cx);
            })
            .unwrap();
        self.draw();
        self.cx.run_until_parked();
        self.draw();
        self.cx.update(|cx| {
            assert!(
                thinking.read(cx).expanded,
                "WIRING: stage really expanded before Final"
            )
        });
        (group, thinking)
    }

    fn final_event(&mut self, id: &str, end: usize) {
        self.cx.update(|cx| append_final(&self.stream, id, end, cx));
        self.draw();
    }

    fn drag(&mut self, start: Point<Pixels>, end: Point<Pixels>) {
        self.cx
            .update_window(self.window.into(), |_, window, cx| {
                window.dispatch_event(
                    gpui_kit::MouseDownEvent {
                        button: MouseButton::Left,
                        position: start,
                        modifiers: Modifiers::default(),
                        click_count: 1,
                        first_mouse: false,
                    }
                    .to_platform_input(),
                    cx,
                );
                window.dispatch_event(
                    MouseMoveEvent {
                        position: end,
                        pressed_button: Some(MouseButton::Left),
                        modifiers: Modifiers::default(),
                    }
                    .to_platform_input(),
                    cx,
                );
                window.dispatch_event(
                    MouseUpEvent {
                        button: MouseButton::Left,
                        position: end,
                        modifiers: Modifiers::default(),
                        click_count: 1,
                    }
                    .to_platform_input(),
                    cx,
                );
            })
            .unwrap();
        self.draw();
    }

    fn selected_text(&mut self) -> String {
        self.cx
            .update_window(self.window.into(), |_, window, cx| {
                gpui_kit::base::TextSelection::selected_text(window, cx)
            })
            .unwrap()
    }

    fn select_entry(&mut self, index: usize) -> String {
        let bounds = self.cx.update(|cx| {
            self.stream
                .read(cx)
                .list
                .bounds_for_item(index)
                .expect("WIRING: selected real message must be in viewport")
        });
        let start = point(
            bounds.left() + px(8.),
            bounds.top() + px(Layout::CONVERSATION_BODY_TOP + 12.),
        );
        self.drag(start, start + point(px(110.), px(0.)));
        let selected = self.selected_text();
        assert!(
            !selected.is_empty(),
            "WIRING: actual visible message drag must select text"
        );
        selected
    }
}

#[gpui_kit::test]
fn first_trusted_final_collapses_reading_selection_and_focus_then_enter_reopens(
    _: &mut gpui_kit::TestAppContext,
) {
    let mut fixture = StreamFixture::open();
    let (group, thinking) = fixture.begin("current");
    let source_paint = fixture
        .draw()
        .into_iter()
        .find(|paint| paint.scope.owner.as_ref() == format!("run-stage-{}-0", group.entity_id()))
        .expect("WIRING: expanded production stage actually mounted");
    let start = point(
        px(36.),
        source_paint.bounds.bottom() + px(Layout::PROCESS_ITEM_GAP + 14.),
    );
    fixture.drag(start, start + point(px(120.), px(0.)));
    assert!(
        !fixture.selected_text().is_empty(),
        "WIRING: real mouse drag must select visible source before Final"
    );
    fixture
        .cx
        .update_window(fixture.window.into(), |_, window, cx| {
            assert!(
                thinking.read(cx).contains_focus(window, cx),
                "WIRING: selection owns real process focus"
            );
            fixture.stream.update(cx, |stream, _| {
                stream.list.set_follow_mode(gpui_kit::FollowMode::Normal)
            });
        })
        .unwrap();
    let anchor = fixture
        .cx
        .update(|cx| fixture.stream.read(cx).scroll_anchor_snapshot());
    fixture.final_event("current", COMMENTARY.len() + FINAL_TEXT.len());
    assert!(fixture.selected_text().is_empty());
    fixture.cx.update(|cx| {
        let stream = fixture.stream.read(cx);
        assert!(!group.read(cx).expanded());
        assert!(group.read(cx).process_in_details());
        assert_eq!(
            stream
                .completion_response(&["current".into()], cx)
                .as_deref(),
            Some(FINAL_TEXT)
        );
        assert_eq!(stream.scroll_anchor_snapshot(), anchor);
        assert!(!thinking.read(cx).source_copy.has_selected_text());
    });
    let header = fixture
        .draw()
        .into_iter()
        .find(|paint| paint.scope.owner.as_ref() == format!("run-disclosure-{}", group.entity_id()))
        .expect("visible run header after collapse");
    fixture
        .cx
        .update_window(fixture.window.into(), |_, window, cx| {
            assert!(
                header.scope.focus.is_focused(window),
                "focus must return to visible production run header"
            );
            window.dispatch_keystroke(Keystroke::parse("enter").unwrap(), cx);
            assert!(
                group.read(cx).expanded(),
                "Enter on recovered real focus must reopen immediately"
            );
        })
        .unwrap();
    fixture.draw();
    fixture.cx.update(|cx| {
        fixture.stream.update(cx, |stream, cx| {
            stream.apply_event(
                ConversationEvent::AssistantResponseCompleted {
                    message_id: "current".into(),
                    response: response(
                        "final",
                        AssistantResponseKind::Final,
                        COMMENTARY.len(),
                        COMMENTARY.len() + FINAL_TEXT.len(),
                    ),
                },
                cx,
            );
            stream.apply_event(
                ConversationEvent::MessageFinished {
                    message_id: "current".into(),
                    stop_reason: vega_conversation::types::ConversationStopReason::End,
                    execution_duration_ms: Some(1200),
                },
                cx,
            );
        })
    });
    for _ in 0..4 {
        fixture.cx.advance_clock(Duration::from_millis(40));
        fixture.draw();
    }
    fixture.cx.update(|cx| {
        assert!(
            group.read(cx).expanded(),
            "manual reopen survives repeated Final/completion/redraw"
        );
        assert_eq!(
            fixture
                .stream
                .read(cx)
                .completion_response(&["current".into()], cx)
                .as_deref(),
            Some(FINAL_TEXT)
        );
    });
}

#[gpui_kit::test]
fn final_does_not_steal_composer_or_other_turn_focus_and_selection(
    _: &mut gpui_kit::TestAppContext,
) {
    for composer in [false, true] {
        let mut fixture = StreamFixture::open();
        let (previous_group, _) = fixture.begin("previous");
        fixture.final_event("previous", COMMENTARY.len() + FINAL_TEXT.len());
        fixture.cx.update(|cx| {
            fixture.stream.update(cx, |stream, cx| {
                stream.apply_event(
                    ConversationEvent::MessageFinished {
                        message_id: "previous".into(),
                        stop_reason: vega_conversation::types::ConversationStopReason::End,
                        execution_duration_ms: Some(800),
                    },
                    cx,
                )
            })
        });
        let (group, _) = fixture.begin("current");
        let previous_header = fixture
            .draw()
            .into_iter()
            .find(|paint| {
                paint.scope.owner.as_ref()
                    == format!("run-disclosure-{}", previous_group.entity_id())
            })
            .expect("WIRING: previous visible run header");
        let before = fixture
            .cx
            .update_window(fixture.window.into(), |_, window, cx| {
                if composer {
                    fixture
                        .stream
                        .update(cx, |stream, cx| stream.focus_composer(window, cx));
                } else {
                    previous_header.scope.focus.focus(window, cx);
                }
                window
                    .focused(cx)
                    .expect("unrelated real focus established")
            })
            .unwrap();
        fixture.final_event("current", COMMENTARY.len() + FINAL_TEXT.len());
        fixture
            .cx
            .update_window(fixture.window.into(), |_, window, cx| {
                assert!(!group.read(cx).expanded());
                assert_eq!(
                    window.focused(cx),
                    Some(before),
                    "Final must not steal unrelated focus"
                );
            })
            .unwrap();
    }
}

#[gpui_kit::test]
fn invalid_or_missing_final_and_failed_or_cancelled_turns_keep_unknown_body_visible(
    _: &mut gpui_kit::TestAppContext,
) {
    for end in [COMMENTARY.len(), COMMENTARY.len() + FINAL_TEXT.len() + 1] {
        let mut fixture = StreamFixture::open();
        let (group, thinking) = fixture.begin("current");
        fixture.final_event("current", end);
        fixture.cx.update(|cx| {
            assert!(!group.read(cx).process_in_details());
            assert!(thinking.read(cx).expanded);
            let stream = fixture.stream.read(cx);
            assert!(stream.entries.iter().enumerate().any(|(index, entry)| matches!(entry, StreamEntry::Assistant { copy, .. } if copy.original_text().contains(FINAL_TEXT)) && !stream.process_entry_hidden(index, cx)));
        });
    }
    for terminal in [0, 1, 2] {
        let mut fixture = StreamFixture::open();
        let (group, thinking) = fixture.begin("current");
        fixture.cx.update(|cx| {
            fixture.stream.update(cx, |stream, cx| {
                if terminal == 0 {
                    stream.apply_event(
                        ConversationEvent::Interrupted {
                            message_id: "current".into(),
                            execution_duration_ms: Some(500),
                        },
                        cx,
                    );
                } else if terminal == 1 {
                    stream.apply_event(
                        ConversationEvent::MessageFinished {
                            message_id: "current".into(),
                            stop_reason: vega_conversation::types::ConversationStopReason::End,
                            execution_duration_ms: Some(500),
                        },
                        cx,
                    );
                } else {
                    stream.apply_event(
                        ConversationEvent::ToolCallProposed {
                            call: ToolCall {
                                id: "failed-tool".into(),
                                tool: "bash".into(),
                                input_json: r#"{"command":"false"}"#.into(),
                            },
                        },
                        cx,
                    );
                    stream.apply_event(
                        ConversationEvent::ToolCallFinished {
                            call_id: "failed-tool".into(),
                            result: ToolResult {
                                status: ToolCallStatus::Failed,
                                output: "disclosure failure remains visible".into(),
                                reused: true,
                                exit_code: None,
                                duration_ms: None,
                                truncated: None,
                                invalid: None,
                            },
                        },
                        cx,
                    );
                }
            })
        });
        fixture.draw();
        fixture.cx.update(|cx| {
            assert!(!group.read(cx).process_in_details());
            assert!(thinking.read(cx).expanded);
            assert_eq!(
                thinking.read(cx).source_copy.original_text(),
                SOURCE.repeat(12)
            );
            if terminal == 2 {
                assert_eq!(
                    fixture.stream.read(cx).tool_cards["failed-tool"]
                        .read(cx)
                        .activity_state(),
                    crate::tool_card::ToolActivityState::Failed
                );
            }
        });
        if terminal == 2 {
            let failed = fixture
                .cx
                .update(|cx| fixture.stream.read(cx).tool_cards["failed-tool"].clone());
            let glyph = fixture
                .draw()
                .into_iter()
                .find(|paint| {
                    paint.scope.owner.as_ref() == format!("tool-disclosure-{}", failed.entity_id())
                })
                .expect("WIRING: failed tool header reached real paint");
            assert!(glyph.scope.enabled);
            assert!(!glyph.scope.expanded);
            fixture
                .cx
                .update_window(fixture.window.into(), |_, window, cx| {
                    glyph.scope.focus.focus(window, cx);
                    assert!(glyph.scope.focus.is_focused(window));
                    window.dispatch_keystroke(Keystroke::parse("enter").unwrap(), cx);
                    assert!(failed.read(cx).is_expanded());
                })
                .unwrap();
            fixture.draw();
            fixture
                .cx
                .update_window(fixture.window.into(), |_, window, cx| {
                    let id = ("tool-activity-toggle", failed.entity_id()).into();
                    let mut headers = gpui_kit::base::test_support::snapshots(window)
                        .into_iter()
                        .filter(|snapshot| snapshot.path().last() == Some(&id));
                    let header = headers
                        .next()
                        .expect("WIRING: expanded failed tool header is rendered");
                    assert!(headers.next().is_none());
                    assert!(header.visible());
                    assert!(header.bounds().size.height > px(0.));
                    assert_eq!(header.expanded(), Some(true));
                    assert_eq!(header.focused(), Some(true));
                    assert_eq!(
                        failed.read(cx).test_detail_output(),
                        Some(vec!["disclosure failure remains visible".to_string()])
                    );
                })
                .unwrap();
        }
    }
}

#[gpui_kit::test]
fn historical_final_missing_and_corrupt_metadata_do_not_hide_unknown_source(
    _: &mut gpui_kit::TestAppContext,
) {
    use vega_conversation::history::AssistantStatus;
    for trusted in [None, Some(false), Some(true)] {
        let mut fixture = StreamFixture::open();
        let mut entries = Vec::new();
        for turn in 0..3 {
            let id = format!("historical-{turn}");
            entries.push(HistoryEntry::AssistantText {
                seq: turn * 4 + 1,
                message_id: id.clone(),
                content: COMMENTARY.into(),
                status: AssistantStatus::Done,
                execution_duration_ms: Some(300),
            });
            entries.push(HistoryEntry::AssistantResponseMetadata {
                seq: turn * 4 + 2,
                message_id: id.clone(),
                response: response(
                    "commentary",
                    AssistantResponseKind::Commentary,
                    0,
                    COMMENTARY.len(),
                ),
            });
            entries.push(HistoryEntry::AssistantText {
                seq: turn * 4 + 3,
                message_id: id.clone(),
                content: FINAL_TEXT.into(),
                status: AssistantStatus::Done,
                execution_duration_ms: Some(300),
            });
            if let Some(valid) = trusted {
                entries.push(HistoryEntry::AssistantResponseMetadata {
                    seq: turn * 4 + 4,
                    message_id: id,
                    response: response(
                        "final",
                        AssistantResponseKind::Final,
                        COMMENTARY.len(),
                        COMMENTARY.len() + FINAL_TEXT.len() + usize::from(!valid),
                    ),
                });
            }
        }
        fixture.cx.update(|cx| {
            fixture.stream.update(cx, |stream, cx| {
                stream.apply_history_page(
                    HistoryPage {
                        subagents: Vec::new(),
                        entries,
                        older_cursor: None,
                        newer_cursor: None,
                        newest_seq: Some(12),
                    },
                    cx,
                )
            })
        });
        fixture.draw();
        fixture.cx.update(|cx| {
            let stream = fixture.stream.read(cx);
            for turn in 0..3 {
                let id = format!("historical-{turn}");
                let sources = stream
                    .entries
                    .iter()
                    .zip(&stream.entry_identities)
                    .enumerate()
                    .filter_map(|(index, (entry, identity))| {
                        if identity.message_id.as_deref() != Some(id.as_str()) {
                            return None;
                        }
                        if let StreamEntry::Assistant { copy, .. } = entry {
                            Some((index, copy.original_text()))
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();
                assert_eq!(
                    sources
                        .iter()
                        .map(|(_, source)| source.as_str())
                        .collect::<String>(),
                    format!("{COMMENTARY}{FINAL_TEXT}")
                );
                if trusted != Some(true) {
                    assert!(
                        sources
                            .iter()
                            .all(|(index, _)| !stream.process_entry_hidden(*index, cx))
                    );
                } else {
                    assert_eq!(
                        stream.completion_response(&[id], cx).as_deref(),
                        Some(FINAL_TEXT)
                    );
                }
            }
        });
    }
}

#[gpui_kit::test]
fn settled_arrow_frames_do_not_rematerialize_messages_or_change_the_reading_anchor(
    _: &mut gpui_kit::TestAppContext,
) {
    let mut fixture = StreamFixture::open();
    let (group, _) = fixture.begin("current");
    fixture.final_event("current", COMMENTARY.len() + FINAL_TEXT.len());
    let header = fixture
        .draw()
        .into_iter()
        .find(|paint| paint.scope.owner.as_ref() == format!("run-disclosure-{}", group.entity_id()))
        .unwrap();
    fixture
        .cx
        .update_window(fixture.window.into(), |_, window, cx| {
            fixture.stream.update(cx, |stream, _| {
                stream.list.set_follow_mode(gpui_kit::FollowMode::Normal)
            });
            header.scope.focus.focus(window, cx);
            window.dispatch_keystroke(Keystroke::parse("enter").unwrap(), cx);
        })
        .unwrap();
    fixture.draw();
    fixture.cx.run_until_parked();
    fixture.draw();
    let snapshot = |stream: &ConversationStream| {
        (
            stream.scroll_anchor_snapshot(),
            stream.measured_entry_heights.clone(),
            stream
                .counters
                .frozen_rematerializations
                .load(Ordering::Relaxed),
            stream
                .counters
                .pending_materializations
                .load(Ordering::Relaxed),
            (0..stream.entries.len())
                .map(|index| stream.list.bounds_for_item(index))
                .collect::<Vec<_>>(),
        )
    };
    let before = fixture.cx.update(|cx| snapshot(fixture.stream.read(cx)));
    for _ in 0..8 {
        fixture.cx.advance_clock(Duration::from_millis(15));
        fixture
            .cx
            .update_window(fixture.window.into(), |_, window, cx| {
                window.simulate_next_frame(cx);
            })
            .unwrap();
        fixture.draw();
        fixture.cx.update(|cx| {
            assert_eq!(
                snapshot(fixture.stream.read(cx)),
                before,
                "decorative frame changed real message materialization/geometry/anchor"
            );
            assert!(group.read(cx).expanded());
        });
    }
}

#[gpui_kit::test]
fn visible_final_and_other_turn_selections_survive_first_collapse(
    _: &mut gpui_kit::TestAppContext,
) {
    for other_turn in [false, true] {
        let mut fixture = StreamFixture::open();
        if other_turn {
            fixture.begin("previous");
            fixture.final_event("previous", COMMENTARY.len() + FINAL_TEXT.len());
            fixture.cx.update(|cx| {
                fixture.stream.update(cx, |stream, cx| {
                    stream.apply_event(
                        ConversationEvent::MessageFinished {
                            message_id: "previous".into(),
                            stop_reason: vega_conversation::types::ConversationStopReason::End,
                            execution_duration_ms: Some(200),
                        },
                        cx,
                    )
                })
            });
        }
        let (current, _) = fixture.begin("current");
        fixture
            .cx
            .update(|cx| stage_final_text(&fixture.stream, "current", cx));
        fixture.draw();
        let selected_message = if other_turn { "previous" } else { "current" };
        let index = fixture.cx.update(|cx| {
            let stream = fixture.stream.read(cx);
            stream.entries.iter().zip(&stream.entry_identities).position(|(entry, identity)| {
                identity.message_id.as_deref() == Some(selected_message)
                    && matches!(entry, StreamEntry::Assistant { copy, .. } if copy.original_text().contains(FINAL_TEXT))
            }).expect("WIRING: actual visible Final source entry")
        });
        let selected = fixture.select_entry(index);
        let focus = fixture
            .cx
            .update_window(fixture.window.into(), |_, window, cx| {
                window
                    .focused(cx)
                    .expect("selection focus on visible Final")
            })
            .unwrap();
        fixture.cx.update(|cx| {
            fixture.stream.update(cx, |stream, cx| {
                stream.apply_event(
                    ConversationEvent::AssistantResponseCompleted {
                        message_id: "current".into(),
                        response: response(
                            "final",
                            AssistantResponseKind::Final,
                            COMMENTARY.len(),
                            COMMENTARY.len() + FINAL_TEXT.len(),
                        ),
                    },
                    cx,
                )
            })
        });
        fixture.draw();
        assert_eq!(fixture.selected_text(), selected);
        fixture
            .cx
            .update_window(fixture.window.into(), |_, window, cx| {
                assert!(!current.read(cx).expanded());
                assert_eq!(window.focused(cx), Some(focus));
                assert_eq!(
                    fixture
                        .stream
                        .read(cx)
                        .completion_response(&["current".into()], cx)
                        .as_deref(),
                    Some(FINAL_TEXT)
                );
            })
            .unwrap();
    }
}

#[gpui_kit::test]
fn independent_disclosure_owners_never_share_the_transition_channel(
    _: &mut gpui_kit::TestAppContext,
) {
    let mut fixture = Fixture::open(Entry::Thinking, false, false);
    let second = fixture
        .cx
        .new(|cx| ProductionHeader::new(Entry::Thinking, true, cx));
    let second_window = fixture
        .cx
        .open_window(size(px(640.), px(720.)), |_, _| second.clone())
        .unwrap();
    let first = fixture.draw();
    fixture.key("enter");
    fixture.draw();
    fixture.cx.advance_clock(Duration::from_millis(40));
    let moving = fixture.draw();
    assert!(displayed_angle(&moving) > 0. && displayed_angle(&moving) < FRAC_PI_2);
    probe::take();
    fixture
        .cx
        .update_window(second_window.into(), |_, window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        })
        .unwrap();
    let other = probe::take()
        .into_iter()
        .next()
        .expect("second real owner painted");
    assert_ne!(other.scope.owner, first.scope.owner);
    assert_angle(displayed_angle(&other), FRAC_PI_2);
    assert!(other.scope.expanded);
    fixture.key("space");
    fixture.tick(TURN);
    assert!(!fixture.expanded());
    fixture
        .cx
        .update(|cx| assert!(second.read(cx).expanded(cx)));
}

#[gpui_kit::test]
fn inactive_disclosure_adopts_truth_and_does_not_replay_when_reactivated(
    _: &mut gpui_kit::TestAppContext,
) {
    for entry in ENTRIES {
        let mut fixture = Fixture::open(entry, false, false);
        fixture.draw();
        fixture.key("enter");
        fixture.draw();
        fixture.tick(Duration::from_millis(30));
        let blocker = fixture
            .cx
            .open_window(size(px(100.), px(100.)), |_, cx| {
                cx.new(|_| gpui_kit::Empty)
            })
            .unwrap();
        fixture
            .cx
            .update_window(blocker.into(), |_, window, _| window.activate_window())
            .unwrap();
        fixture.cx.run_until_parked();
        fixture
            .cx
            .update_window(fixture.window.into(), |_, window, _| {
                assert!(
                    !window.is_window_active(),
                    "WIRING: real deactivation delivered"
                )
            })
            .unwrap();
        assert_angle(displayed_angle(&fixture.draw()), FRAC_PI_2);
        fixture
            .cx
            .update_window(fixture.window.into(), |_, window, cx| {
                window.simulate_next_frame(cx);
            })
            .unwrap();
        assert_eq!(
            fixture
                .cx
                .update_window(fixture.window.into(), |_, window, cx| window
                    .simulate_next_frame(cx))
                .unwrap(),
            0
        );
        fixture
            .cx
            .update_window(fixture.window.into(), |_, window, _| {
                window.activate_window()
            })
            .unwrap();
        fixture.cx.run_until_parked();
        assert_angle(displayed_angle(&fixture.draw()), FRAC_PI_2);
    }
}

struct ConfiguredRotationSurface {
    radians: f32,
    focus: gpui_kit::FocusHandle,
}

impl Render for ConfiguredRotationSurface {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let _scope = probe::scope(
            "configured-rotation-control".into(),
            false,
            true,
            &self.focus,
        );
        div()
            .size(px(40.))
            .child(crate::icons::configured_chevron_paint_control(
                gpui_kit::rgb(0x123456),
                16.,
                self.radians,
            ))
    }
}

#[gpui_kit::test]
fn configured_rotation_probe_records_multiple_real_kit_rotations_without_using_target(
    _: &mut gpui_kit::TestAppContext,
) {
    let mut cx = HeadlessAppContext::with_asset_source(
        Arc::new(NoopTextSystem),
        Arc::new(gpui_kit::assets::Assets),
    );
    cx.update(gpui_kit::init);
    let window = cx
        .open_window(size(px(160.), px(160.)), |_, cx| {
            cx.new(|cx| ConfiguredRotationSurface {
                radians: 0.,
                focus: cx.focus_handle(),
            })
        })
        .unwrap();
    let mut observed = Vec::new();
    let mut slot = None;
    for radians in [0., 0.17, 0.83, FRAC_PI_2, -0.43, 0.17] {
        window
            .update(&mut cx, |view, _, cx| {
                view.radians = radians;
                cx.notify();
            })
            .unwrap();
        probe::take();
        cx.update_window(window.into(), |_, window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        })
        .unwrap();
        let paints = probe::take();
        assert_eq!(paints.len(), 1, "one actual delegated SVG glyph");
        let paint = &paints[0];
        assert!(
            !paint.scope.expanded,
            "constant false target cannot supply the recorded angle"
        );
        assert_eq!(paint.asset, Icon::ChevronRight);
        assert!(paint.asset_path.ends_with("chevron-right.svg"));
        assert_angle(paint.rotation, radians);
        assert_eq!(paint.bounds.size, size(px(16.), px(16.)));
        if let Some(slot) = slot {
            assert_eq!(paint.bounds, slot);
        }
        slot = Some(paint.bounds);
        observed.push(paint.rotation);
    }
    assert!(observed.windows(2).all(|pair| pair[0] != pair[1]));
    assert_eq!(observed[1], observed[5]);
}

impl Fixture {
    fn native_header(&mut self) -> gpui_kit::base::test_support::ElementSnapshot {
        self.window
            .update(&mut self.cx, |view, window, _| {
                let id: gpui_kit::ElementId = match view.entry {
                    Entry::Thinking => "thinking-toggle".into(),
                    Entry::Tool => ("tool-activity-toggle", view.tool.entity_id()).into(),
                    Entry::ToolGroup => {
                        ("tool-activity-group-toggle", view.tools.entity_id()).into()
                    }
                    Entry::Run => ("run-activity-toggle", view.run.entity_id()).into(),
                    Entry::Stage => ("run-stage-toggle", 0usize).into(),
                };
                let mut snapshots = gpui_kit::base::test_support::snapshots(window)
                    .into_iter()
                    .filter(|snapshot| snapshot.path().last() == Some(&id));
                let snapshot = snapshots
                    .next()
                    .expect("WIRING: real header native Element snapshot");
                assert!(
                    snapshots.next().is_none(),
                    "WIRING: unambiguous real header"
                );
                assert!(snapshot.visible());
                assert!(snapshot.bounds().size.height > px(0.));
                snapshot
            })
            .unwrap()
    }
}

#[gpui_kit::test]
fn native_element_expanded_changes_with_body_in_the_same_event_not_at_animation_end(
    _: &mut gpui_kit::TestAppContext,
) {
    for entry in ENTRIES {
        let mut fixture = Fixture::open(entry, false, false);
        let glyph = fixture.draw();
        let rows = fixture.body_rows();
        assert_eq!(fixture.native_header().expanded(), Some(false));
        fixture.click(&glyph);
        fixture.draw();
        assert!(fixture.body_rows() > rows);
        assert_eq!(fixture.native_header().expanded(), Some(true));
        fixture.tick(Duration::from_millis(35));
        assert_eq!(fixture.native_header().expanded(), Some(true));
        fixture.key("space");
        fixture.draw();
        assert_eq!(fixture.body_rows(), rows);
        assert_eq!(fixture.native_header().expanded(), Some(false));
        assert_eq!(fixture.native_header().focused(), Some(true));
    }
}

#[gpui_kit::test]
fn visible_artifact_preview_and_real_control_focus_survive_first_final_collapse(
    _: &mut gpui_kit::TestAppContext,
) {
    use crate::artifact_card::{ArtifactActivate, ArtifactPreviewRequested};
    use gpui_kit::Focusable as _;

    let mut fixture = StreamFixture::open();
    let preview_text = "Artifact preview remains visible.\nSecond artifact row.\n";
    let (artifact, owner, expected_request) = fixture.cx.update(|cx| {
        cx.bind_keys([gpui_kit::KeyBinding::new(
            "enter",
            ArtifactActivate,
            Some("ArtifactCard"),
        )]);
        begin_process(&fixture.stream, "previous", cx);
        let (projection, preview) =
            vega_conversation::ArtifactService::in_memory_ui_fixture(preview_text)
                .expect("WIRING: bounded memory projection, not filesystem authority");
        let expected_request = ArtifactPreviewRequested {
            thread_id: "disclosure-thread".into(),
            project_id: String::new(),
            card_id: projection.id,
            file_id: projection
                .current_file_id
                .expect("WIRING: bounded fixture has a preview capability"),
        };
        let artifact = cx
            .new(|cx| ArtifactCard::new("disclosure-thread".into(), String::new(), projection, cx));
        artifact.update(cx, |card, cx| assert!(card.apply_preview(preview, cx)));
        let owner = fixture.stream.update(cx, |stream, cx| {
            assert!(stream.register_artifact_call("artifact-owner"));
            assert!(stream.apply_artifact_card("artifact-owner", artifact.clone(), cx));
            assert!(
                !stream
                    .entries
                    .iter()
                    .any(|entry| matches!(entry, StreamEntry::ArtifactGroup { .. }))
            );
            stream
                .artifact_capture_owner("artifact-owner")
                .expect("WIRING: real registration retains the originating turn")
        });
        assert_eq!(owner.0, "previous");
        (artifact, owner, expected_request)
    });
    let preview_requests = Rc::new(RefCell::new(Vec::new()));
    let _subscription = fixture.cx.update(|cx| {
        let requests = preview_requests.clone();
        cx.subscribe(
            &artifact,
            move |card, event: &ArtifactPreviewRequested, _| {
                requests
                    .borrow_mut()
                    .push((card.entity_id(), event.clone()));
            },
        )
    });
    fixture.final_event("previous", COMMENTARY.len() + FINAL_TEXT.len());
    let artifact_group = fixture.cx.update(|cx| {
        fixture.stream.update(cx, |stream, cx| {
            stream.apply_event(
                ConversationEvent::MessageFinished {
                    message_id: "previous".into(),
                    stop_reason: vega_conversation::types::ConversationStopReason::End,
                    execution_duration_ms: Some(800),
                },
                cx,
            );
            stream.artifact_groups["previous"].clone()
        })
    });
    let assert_artifact_owner = |fixture: &mut StreamFixture| {
        fixture.cx.update(|cx| {
            let stream = fixture.stream.read(cx);
            assert_eq!(stream.artifact_cards.get("artifact-owner"), Some(&artifact));
            assert_eq!(
                stream.artifact_capture_owner("artifact-owner"),
                Some(owner.clone())
            );
            assert_eq!(artifact_group.read(cx).message_id(), "previous");
            assert_eq!(artifact_group.read(cx).row_count(), 1);
            let groups = stream
                .entries
                .iter()
                .filter_map(|entry| match entry {
                    StreamEntry::ArtifactGroup { group }
                        if group.read(cx).message_id() == "previous" =>
                    {
                        Some(group)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(groups.len(), 1);
            assert_eq!(groups[0], &artifact_group);
        });
    };
    assert_artifact_owner(&mut fixture);
    fixture
        .window
        .update(&mut fixture.cx, |surface, _, cx| {
            surface.artifact_preview = Some(artifact.clone());
            cx.notify();
        })
        .unwrap();
    let (group, _) = fixture.begin("current");
    fixture.cx.update(|cx| {
        assert!(!group.read(cx).process_in_details());
    });
    fixture.cx.run_until_parked();
    fixture.draw();
    let painted_rows = |fixture: &mut StreamFixture| {
        fixture
            .cx
            .update_window(fixture.window.into(), |_, window, cx| {
                let count = artifact.read(cx).row_count();
                assert_eq!(count, 4);
                assert_eq!(
                    artifact.read(cx).test_preview_lines(),
                    ["Artifact preview remains visible.", "Second artifact row."]
                );
                let snapshots = gpui_kit::base::test_support::snapshots(window);
                (0..count)
                    .map(|row| {
                        let id: gpui_kit::ElementId = gpui_kit::SharedString::from(format!(
                            "artifact-row-{}-{row}",
                            artifact.entity_id()
                        ))
                        .into();
                        let snapshot = snapshots
                            .iter()
                            .find(|snapshot| snapshot.path().last() == Some(&id))
                            .expect("WIRING: actual ArtifactCard::render_row reached paint");
                        assert!(snapshot.visible());
                        assert!(snapshot.bounds().size.width > px(0.));
                        assert!(snapshot.bounds().size.height > px(0.));
                        assert!(snapshot.bounds().left() >= px(0.));
                        assert!(snapshot.bounds().right() <= window.viewport_size().width);
                        assert!(snapshot.bounds().top() >= px(0.));
                        assert!(snapshot.bounds().bottom() <= window.viewport_size().height);
                        snapshot.bounds()
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap()
    };
    let before_rows = painted_rows(&mut fixture);
    assert!(preview_requests.borrow().is_empty());
    let focus = fixture
        .cx
        .update_window(fixture.window.into(), |_, window, cx| {
            let focus = artifact.read(cx).focus_handle(cx);
            focus.focus(window, cx);
            assert!(focus.is_focused(window));
            window.dispatch_keystroke(Keystroke::parse("enter").unwrap(), cx);
            assert!(focus.is_focused(window));
            focus
        })
        .unwrap();
    assert_eq!(
        preview_requests.borrow().as_slice(),
        [(artifact.entity_id(), expected_request.clone())]
    );
    fixture.final_event("current", COMMENTARY.len() + FINAL_TEXT.len());
    fixture.cx.run_until_parked();
    fixture.draw();
    fixture
        .cx
        .update_window(fixture.window.into(), |_, window, cx| {
            assert!(!group.read(cx).expanded());
            assert!(group.read(cx).process_in_details());
            assert_eq!(
                fixture
                    .stream
                    .read(cx)
                    .completion_response(&["current".into()], cx)
                    .as_deref(),
                Some(FINAL_TEXT)
            );
            assert!(focus.is_focused(window));
            assert_eq!(artifact.read(cx).focus_handle(cx), focus);
        })
        .unwrap();
    assert_artifact_owner(&mut fixture);
    let after_rows = painted_rows(&mut fixture);
    assert_eq!(after_rows.len(), before_rows.len());
    for (before, after) in before_rows.iter().zip(&after_rows) {
        assert_eq!(before.size, after.size);
    }
    fixture
        .cx
        .update_window(fixture.window.into(), |_, window, cx| {
            assert!(focus.is_focused(window));
            window.dispatch_keystroke(Keystroke::parse("enter").unwrap(), cx);
            assert!(focus.is_focused(window));
        })
        .unwrap();
    assert_eq!(
        preview_requests.borrow().as_slice(),
        [
            (artifact.entity_id(), expected_request.clone()),
            (artifact.entity_id(), expected_request),
        ]
    );
}

#[derive(Debug, PartialEq, Eq)]
struct LayoutCounts {
    invalidates: Vec<usize>,
    remeasures: Vec<Option<usize>>,
    layouts: std::collections::BTreeMap<usize, usize>,
}

fn measured_work(owner: gpui_kit::EntityId) -> LayoutCounts {
    use crate::disclosure_layout_probe::{self as layout, Observation};
    let mut counts = LayoutCounts {
        invalidates: Vec::new(),
        remeasures: Vec::new(),
        layouts: Default::default(),
    };
    for event in layout::take() {
        match event {
            Observation::Invalidate(index) => counts.invalidates.push(index),
            Observation::Remeasure(index) => counts.remeasures.push(index),
            Observation::Layout(entity, index) if entity == owner => {
                *counts.layouts.entry(index).or_default() += 1
            }
            _ => {}
        }
    }
    counts
}

#[gpui_kit::test]
fn decorative_frames_do_no_more_message_measurement_than_identical_settled_redraws(
    _: &mut gpui_kit::TestAppContext,
) {
    use crate::disclosure_layout_probe as layout;
    let mut fixture = StreamFixture::open();
    let (group, thinking) = fixture.begin("current");
    fixture.cx.update(|cx| {
        thinking.update(cx, |card, cx| {
            card.progress_finished = true;
            card.shimmer_epoch = None;
            cx.notify();
        })
    });
    fixture.final_event("current", COMMENTARY.len() + FINAL_TEXT.len());
    fixture.cx.run_until_parked();
    fixture.draw();
    let header = fixture
        .draw()
        .into_iter()
        .find(|paint| paint.scope.owner.as_ref() == format!("run-disclosure-{}", group.entity_id()))
        .unwrap();
    layout::take();
    fixture
        .cx
        .update_window(fixture.window.into(), |_, window, cx| {
            header.scope.focus.focus(window, cx);
            window.dispatch_keystroke(Keystroke::parse("enter").unwrap(), cx);
        })
        .unwrap();
    fixture.cx.run_until_parked();
    fixture.draw();
    let fold = measured_work(fixture.stream.entity_id());
    assert!(
        !fold.invalidates.is_empty(),
        "WIRING: real fold must retain necessary message invalidation"
    );
    assert_eq!(
        fold.invalidates.len(),
        1,
        "one real fold, not decorative invalidation"
    );
    assert_eq!(
        fold.remeasures,
        fold.invalidates
            .iter()
            .copied()
            .map(Some)
            .collect::<Vec<_>>()
    );
    let body = fixture
        .cx
        .update(|cx| fixture.stream.read(cx).list.bounds_for_item(0).unwrap());
    let mut moving_work = Vec::new();
    for step in 1..=8 {
        layout::take();
        fixture.cx.advance_clock(Duration::from_millis(15));
        fixture
            .cx
            .update_window(fixture.window.into(), |_, window, cx| {
                window.simulate_next_frame(cx);
            })
            .unwrap();
        let paints = fixture.draw();
        let glyph = paints
            .iter()
            .find(|paint| paint.scope.owner == header.scope.owner)
            .unwrap();
        if step < 8 {
            assert!(displayed_angle(glyph) > 0. && displayed_angle(glyph) < FRAC_PI_2);
        }
        assert_eq!(
            fixture
                .cx
                .update(|cx| fixture.stream.read(cx).list.bounds_for_item(0).unwrap()),
            body
        );
        let counts = measured_work(fixture.stream.entity_id());
        assert!(
            counts.invalidates.is_empty(),
            "decoration must not invalidate a message"
        );
        assert!(
            counts.remeasures.is_empty(),
            "decoration must not explicitly remeasure a message"
        );
        moving_work.push(counts);
    }
    for expected in moving_work {
        layout::take();
        fixture.cx.advance_clock(Duration::from_millis(15));
        fixture
            .cx
            .update_window(fixture.window.into(), |_, window, cx| {
                window.simulate_next_frame(cx);
            })
            .unwrap();
        fixture.draw();
        let settled = measured_work(fixture.stream.entity_id());
        assert!(
            !settled.layouts.is_empty(),
            "WIRING: ordinary List layout measures visible messages"
        );
        assert_eq!(
            expected, settled,
            "same input/window/clock redraw: no extra decorative message layout"
        );
    }
}

struct KeyboardSurface {
    header: Entity<ProductionHeader>,
    before: gpui_kit::FocusHandle,
    after: gpui_kit::FocusHandle,
}

impl Render for KeyboardSurface {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .id("before-header")
                    .track_focus(&self.before)
                    .tab_stop(true)
                    .h(px(24.))
                    .child("Before"),
            )
            .child(self.header.clone())
            .child(
                div()
                    .id("after-header")
                    .track_focus(&self.after)
                    .tab_stop(true)
                    .h(px(24.))
                    .child("After"),
            )
    }
}

#[gpui_kit::test]
fn tab_and_shift_tab_visit_only_enabled_real_headers_and_not_the_decorative_glyph(
    _: &mut gpui_kit::TestAppContext,
) {
    for (entry, disabled) in ENTRIES
        .into_iter()
        .map(|entry| (entry, false))
        .chain([(Entry::Thinking, true), (Entry::Tool, true)])
    {
        let mut cx = HeadlessAppContext::with_asset_source(
            Arc::new(NoopTextSystem),
            Arc::new(gpui_kit::assets::Assets),
        );
        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_global(vega_theme::Theme::light());
        });
        let header = cx.new(|cx| ProductionHeader::new(entry, false, cx));
        if disabled {
            header.update(&mut cx, |view, cx| {
                if entry == Entry::Thinking {
                    view.thinking = cx.new(ThinkingBlock::new);
                } else {
                    let call = ToolCall {
                        id: "no-output".into(),
                        tool: "read".into(),
                        input_json: r#"{"path":"README.md"}"#.into(),
                    };
                    view.tool = cx.new(|_| ToolCard::proposed(&call));
                }
                cx.notify();
            });
        }
        let surface = cx.new(|cx| KeyboardSurface {
            header: header.clone(),
            before: cx.focus_handle().tab_stop(true),
            after: cx.focus_handle().tab_stop(true),
        });
        let window = cx
            .open_window(size(px(640.), px(720.)), |window, cx| {
                cx.new(|cx| gpui_kit::component::Root::new(surface.clone(), window, cx))
            })
            .unwrap();
        cx.run_until_parked();
        probe::take();
        cx.update_window(window.into(), |_, window, cx| {
            window.activate_window();
            window.refresh();
            window.draw(cx).clear(cx);
        })
        .unwrap();
        let owner = cx.update(|cx| header.read(cx).owner());
        let glyph = probe::take()
            .into_iter()
            .find(|paint| paint.scope.owner.as_ref() == owner)
            .unwrap();
        assert_eq!(glyph.scope.enabled, !disabled);
        let (before, after) = cx.update(|cx| {
            let surface = surface.read(cx);
            (surface.before.clone(), surface.after.clone())
        });
        cx.update_window(window.into(), |_, window, cx| {
            before.focus(window, cx);
            window.dispatch_keystroke(Keystroke::parse("tab").unwrap(), cx);
            if disabled {
                assert!(
                    after.is_focused(window),
                    "no-content header must be skipped by Tab"
                );
            } else {
                assert!(
                    glyph.scope.focus.is_focused(window),
                    "Tab reaches real header, not glyph"
                );
                window.dispatch_keystroke(Keystroke::parse("space").unwrap(), cx);
                assert!(header.read(cx).expanded(cx));
                window.dispatch_keystroke(Keystroke::parse("enter").unwrap(), cx);
                assert!(!header.read(cx).expanded(cx));
            }
        })
        .unwrap();
        cx.update_window(window.into(), |_, window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        })
        .unwrap();
        cx.update_window(window.into(), |_, window, cx| {
            if !disabled {
                window.dispatch_keystroke(Keystroke::parse("tab").unwrap(), cx);
                assert!(after.is_focused(window), "no extra arrow Tab stop");
            }
            window.dispatch_keystroke(Keystroke::parse("shift-tab").unwrap(), cx);
            if disabled {
                assert!(before.is_focused(window));
                glyph.scope.focus.focus(window, cx);
                window.dispatch_keystroke(Keystroke::parse("space").unwrap(), cx);
                window.dispatch_keystroke(Keystroke::parse("enter").unwrap(), cx);
                assert!(!header.read(cx).expanded(cx));
            } else {
                assert!(glyph.scope.focus.is_focused(window));
                window.dispatch_keystroke(Keystroke::parse("shift-tab").unwrap(), cx);
                assert!(before.is_focused(window));
            }
        })
        .unwrap();
        if disabled {
            let position = glyph.bounds.center();
            cx.update_window(window.into(), |_, window, cx| {
                window.dispatch_event(
                    MouseUpEvent {
                        button: MouseButton::Left,
                        position,
                        modifiers: Modifiers::default(),
                        click_count: 1,
                    }
                    .to_platform_input(),
                    cx,
                );
                assert!(!header.read(cx).expanded(cx));
            })
            .unwrap();
        }
    }
}

#[gpui_kit::test]
fn rapid_reactivation_without_an_inactive_draw_does_not_replay_missed_motion(
    _: &mut gpui_kit::TestAppContext,
) {
    for entry in ENTRIES {
        let mut fixture = Fixture::open(entry, false, false);
        let before_draw = fixture.root_renders();
        fixture.draw();
        assert!(
            fixture.root_renders() > before_draw,
            "WIRING: real root refresh/draw must advance the render counter"
        );
        fixture.key("enter");
        fixture.draw();
        let moving = fixture.tick(Duration::from_millis(25));
        assert!(displayed_angle(&moving) > 0. && displayed_angle(&moving) < FRAC_PI_2);
        let blocker = fixture
            .cx
            .open_window(size(px(100.), px(100.)), |_, cx| {
                cx.new(|_| gpui_kit::Empty)
            })
            .unwrap();
        let before_deactivation = fixture.root_renders();
        fixture
            .cx
            .update_window(blocker.into(), |_, window, _| window.activate_window())
            .unwrap();
        fixture.cx.run_until_parked();
        fixture
            .cx
            .update_window(fixture.window.into(), |_, window, _| {
                assert!(!window.is_window_active())
            })
            .unwrap();
        fixture.cx.advance_clock(Duration::from_millis(1));
        assert_eq!(
            fixture.root_renders(),
            before_deactivation,
            "WIRING: automatic inactive root render invalidates the no-inactive-draw precondition"
        );
        fixture
            .cx
            .update_window(fixture.window.into(), |_, window, _| {
                window.activate_window()
            })
            .unwrap();
        fixture.cx.run_until_parked();
        assert_eq!(
            fixture.root_renders(),
            before_deactivation,
            "WIRING: root must not render between deactivation and the explicit reactivation draw"
        );
        assert_angle(displayed_angle(&fixture.draw()), FRAC_PI_2);
        assert_angle(
            displayed_angle(&fixture.tick(Duration::from_millis(1))),
            FRAC_PI_2,
        );
        fixture
            .cx
            .update_window(fixture.window.into(), |_, window, cx| {
                window.simulate_next_frame(cx);
            })
            .unwrap();
        assert_eq!(
            fixture
                .cx
                .update_window(fixture.window.into(), |_, window, cx| window
                    .simulate_next_frame(cx))
                .unwrap(),
            0
        );
    }
}

#[gpui_kit::test]
fn mounted_but_clipped_or_hidden_arrows_stop_frames_and_resume_at_truth(
    _: &mut gpui_kit::TestAppContext,
) {
    for (entry, hidden) in ENTRIES
        .into_iter()
        .flat_map(|entry| [false, true].map(|hidden| (entry, hidden)))
    {
        let mut fixture = Fixture::open(entry, false, false);
        fixture.draw();
        fixture.key("enter");
        fixture.draw();
        let moving = fixture.tick(Duration::from_millis(30));
        assert!(displayed_angle(&moving) > 0. && displayed_angle(&moving) < FRAC_PI_2);
        fixture
            .window
            .update(&mut fixture.cx, |view, _, cx| {
                view.hidden = hidden;
                view.clipped_offscreen = !hidden;
                assert!(view.mounted, "not an unmount/remount substitute");
                cx.notify();
            })
            .unwrap();
        fixture.draw_without_glyph();
        fixture
            .window
            .update(&mut fixture.cx, |view, window, cx| {
                assert!(view.mounted);
                assert!(view.expanded(cx));
                window.simulate_next_frame(cx);
            })
            .unwrap();
        fixture.draw_without_glyph();
        assert_eq!(
            fixture
                .cx
                .update_window(fixture.window.into(), |_, window, cx| window
                    .simulate_next_frame(cx))
                .unwrap(),
            0
        );
        fixture.cx.advance_clock(Duration::from_millis(1));
        fixture
            .window
            .update(&mut fixture.cx, |view, _, cx| {
                view.hidden = false;
                view.clipped_offscreen = false;
                cx.notify();
            })
            .unwrap();
        let restored = fixture.draw();
        assert_angle(displayed_angle(&restored), FRAC_PI_2);
        assert_eq!(restored.scope.owner, moving.scope.owner);
        assert_angle(
            displayed_angle(&fixture.tick(Duration::from_millis(15))),
            FRAC_PI_2,
        );
    }
}

impl Fixture {
    fn draw_without_glyph(&mut self) {
        probe::take();
        self.cx
            .update_window(self.window.into(), |_, window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
            })
            .unwrap();
        probe::take();
    }
}

#[gpui_kit::test]
fn thinking_shimmer_keeps_its_own_active_visible_reduced_and_finished_frame_gates(
    _: &mut gpui_kit::TestAppContext,
) {
    let mut fixture = Fixture::open(Entry::Thinking, false, false);
    fixture
        .window
        .update(&mut fixture.cx, |view, _, cx| {
            let card = view.thinking.clone();
            view.run = cx.new(|cx| {
                let mut group = RunActivityGroup::new(RunActivityStatus::Running, None, false, cx);
                let segment = group.start_segment(cx);
                group.append_thinking(segment, card.clone(), cx);
                group.set_thinking(card.clone(), true, cx);
                group
            });
            card.update(cx, |card, cx| {
                card.progress_finished = false;
                card.set_ancestor_visible(view.mounted && !view.hidden);
                card.shimmer_epoch = Some(Instant::now());
                cx.notify();
            });
        })
        .unwrap();
    fixture.draw();
    let frame_count = |fixture: &mut Fixture| {
        fixture
            .cx
            .update_window(fixture.window.into(), |_, window, cx| {
                window.simulate_next_frame(cx)
            })
            .unwrap()
    };
    assert!(
        frame_count(&mut fixture) > 0,
        "WIRING: real Thinking shimmer schedules frames"
    );
    fixture.cx.update(|cx| cx.set_reduce_motion(true));
    fixture.draw();
    frame_count(&mut fixture);
    fixture.draw();
    assert_eq!(frame_count(&mut fixture), 0);
    fixture.cx.update(|cx| cx.set_reduce_motion(false));
    fixture.draw();
    assert!(
        frame_count(&mut fixture) > 0,
        "arrow settlement must not suppress active shimmer"
    );
    fixture
        .window
        .update(&mut fixture.cx, |view, _, cx| {
            view.clipped_offscreen = true;
            cx.notify();
        })
        .unwrap();
    fixture.draw_without_glyph();
    frame_count(&mut fixture);
    fixture.draw_without_glyph();
    assert_eq!(frame_count(&mut fixture), 0);
    fixture
        .window
        .update(&mut fixture.cx, |view, _, cx| {
            view.clipped_offscreen = false;
            cx.notify();
        })
        .unwrap();
    fixture.draw();
    assert!(frame_count(&mut fixture) > 0);
    let blocker = fixture
        .cx
        .open_window(size(px(100.), px(100.)), |_, cx| {
            cx.new(|_| gpui_kit::Empty)
        })
        .unwrap();
    fixture
        .cx
        .update_window(blocker.into(), |_, window, _| window.activate_window())
        .unwrap();
    fixture.cx.run_until_parked();
    fixture.draw();
    frame_count(&mut fixture);
    fixture.draw();
    assert_eq!(frame_count(&mut fixture), 0);
    fixture
        .cx
        .update_window(fixture.window.into(), |_, window, _| {
            window.activate_window()
        })
        .unwrap();
    fixture.cx.run_until_parked();
    fixture.draw();
    assert!(frame_count(&mut fixture) > 0);
    fixture
        .window
        .update(&mut fixture.cx, |view, _, cx| {
            view.run.update(cx, |group, cx| group.finish_thinking(cx));
            assert!(view.thinking.read(cx).progress_finished);
            assert!(view.thinking.read(cx).shimmer_epoch.is_none());
        })
        .unwrap();
    fixture.draw();
    frame_count(&mut fixture);
    fixture.draw();
    assert_eq!(frame_count(&mut fixture), 0);
}

#[gpui_kit::test]
fn losing_source_mid_turn_disables_click_keyboard_and_decorative_frames(
    _: &mut gpui_kit::TestAppContext,
) {
    let mut fixture = Fixture::open(Entry::Thinking, false, false);
    fixture.draw();
    fixture.key("enter");
    fixture.draw();
    fixture.tick(Duration::from_millis(20));
    fixture
        .window
        .update(&mut fixture.cx, |view, _, cx| {
            view.thinking.update(cx, ThinkingBlock::evict);
            cx.notify();
        })
        .unwrap();
    let disabled = fixture.draw();
    assert!(!disabled.scope.enabled);
    assert!(!disabled.scope.expanded);
    assert_eq!(disabled.opacity, 0.);
    fixture.click(&disabled);
    fixture.key("space");
    assert!(!fixture.expanded());
    fixture.draw();
    fixture
        .cx
        .update_window(fixture.window.into(), |_, window, cx| {
            window.simulate_next_frame(cx);
        })
        .unwrap();
    assert_eq!(
        fixture
            .cx
            .update_window(fixture.window.into(), |_, window, cx| window
                .simulate_next_frame(cx))
            .unwrap(),
        0
    );
    fixture
        .window
        .update(&mut fixture.cx, |view, _, cx| {
            view.thinking.update(cx, |card, cx| {
                card.append(SOURCE, false, THINKING_VIEW_BYTES);
                cx.notify();
            });
            cx.notify();
        })
        .unwrap();
    let restored = fixture.draw();
    assert!(restored.scope.enabled);
    assert_eq!(restored.scope.owner, disabled.scope.owner);
    assert_angle(displayed_angle(&restored), 0.);
}

impl StreamFixture {
    fn finish_message(&mut self, id: &str) {
        self.cx.update(|cx| {
            self.stream.update(cx, |stream, cx| {
                stream.apply_event(
                    ConversationEvent::MessageFinished {
                        message_id: id.into(),
                        stop_reason: vega_conversation::types::ConversationStopReason::End,
                        execution_duration_ms: Some(500),
                    },
                    cx,
                )
            })
        });
    }

    fn scroll_up(&mut self, pixels: f32) {
        self.cx
            .update_window(self.window.into(), |_, window, cx| {
                window.dispatch_event(
                    gpui_kit::ScrollWheelEvent {
                        position: point(px(320.), px(240.)),
                        delta: gpui_kit::ScrollDelta::Pixels(point(px(0.), px(pixels))),
                        modifiers: Modifiers::default(),
                        touch_phase: gpui_kit::TouchPhase::Moved,
                    }
                    .to_platform_input(),
                    cx,
                );
            })
            .unwrap();
        self.draw();
    }
}

#[gpui_kit::test]
fn first_final_overrides_independent_manual_focus_and_real_scrolled_reading_states_once(
    _: &mut gpui_kit::TestAppContext,
) {
    for reading in ["manual", "focus", "scrolled"] {
        let mut fixture = StreamFixture::open();
        if reading == "scrolled" {
            for index in 0..16 {
                let id = format!("history-{index}");
                fixture
                    .cx
                    .update(|cx| begin_process(&fixture.stream, &id, cx));
                fixture.final_event(&id, COMMENTARY.len() + FINAL_TEXT.len());
                fixture.finish_message(&id);
            }
        }
        let (group, thinking) = fixture.begin("current");
        let stage = fixture
            .draw()
            .into_iter()
            .find(|paint| {
                paint.scope.owner.as_ref() == format!("run-stage-{}-0", group.entity_id())
            })
            .expect("WIRING: actual expanded current stage");
        fixture
            .cx
            .update_window(fixture.window.into(), |_, window, cx| {
                if reading == "focus" {
                    stage.scope.focus.focus(window, cx);
                    assert!(stage.scope.focus.is_focused(window));
                } else {
                    fixture
                        .stream
                        .update(cx, |stream, cx| stream.focus_composer(window, cx));
                }
            })
            .unwrap();
        let before_scroll = fixture
            .cx
            .update(|cx| fixture.stream.read(cx).scroll_anchor_snapshot());
        if reading == "scrolled" {
            fixture.scroll_up(72.);
            fixture.cx.update(|cx| {
                let stream = fixture.stream.read(cx);
                assert!(
                    !stream.following_tail(),
                    "WIRING: actual wheel input detached tail follow"
                );
                assert_ne!(
                    stream.scroll_anchor_snapshot(),
                    before_scroll,
                    "WIRING: actual scroll changed reading position"
                );
            });
        }
        assert!(
            fixture.selected_text().is_empty(),
            "independent reading precondition, no selection"
        );
        let anchor = fixture
            .cx
            .update(|cx| fixture.stream.read(cx).scroll_anchor_snapshot());
        let before_focus = fixture
            .cx
            .update_window(fixture.window.into(), |_, window, cx| {
                window.focused(cx).unwrap()
            })
            .unwrap();
        fixture.final_event("current", COMMENTARY.len() + FINAL_TEXT.len());
        fixture.cx.update(|cx| {
            assert!(!group.read(cx).expanded());
            assert!(group.read(cx).process_in_details());
            assert!(!thinking.read(cx).expanded);
            if reading == "scrolled" {
                assert_eq!(fixture.stream.read(cx).scroll_anchor_snapshot(), anchor);
            }
        });
        let header = fixture
            .draw()
            .into_iter()
            .find(|paint| {
                paint.scope.owner.as_ref() == format!("run-disclosure-{}", group.entity_id())
            })
            .expect("WIRING: current run header stays visible after first Final");
        fixture
            .cx
            .update_window(fixture.window.into(), |_, window, cx| {
                if reading == "focus" {
                    assert!(
                        header.scope.focus.is_focused(window),
                        "hidden process focus returns to visible run header"
                    );
                } else {
                    assert_eq!(
                        window.focused(cx),
                        Some(before_focus),
                        "manual/scrolled reading must not steal Composer focus"
                    );
                    header.scope.focus.focus(window, cx);
                }
                window.dispatch_keystroke(Keystroke::parse("enter").unwrap(), cx);
                assert!(group.read(cx).expanded());
            })
            .unwrap();
        fixture.cx.update(|cx| {
            fixture.stream.update(cx, |stream, cx| {
                stream.apply_event(
                    ConversationEvent::AssistantResponseCompleted {
                        message_id: "current".into(),
                        response: response(
                            "final",
                            AssistantResponseKind::Final,
                            COMMENTARY.len(),
                            COMMENTARY.len() + FINAL_TEXT.len(),
                        ),
                    },
                    cx,
                )
            })
        });
        fixture.finish_message("current");
        fixture.cx.advance_clock(TURN);
        fixture.draw();
        fixture.cx.update(|cx| {
            assert!(
                group.read(cx).expanded(),
                "manual reopen wins after first forced collapse"
            )
        });
    }
}

#[gpui_kit::test]
fn pending_permission_remains_visible_and_usable_after_trusted_final_collapses_process(
    _: &mut gpui_kit::TestAppContext,
) {
    use std::{
        future::Future,
        pin::Pin,
        task::{Context as TaskContext, Poll, Waker},
    };
    use vega_conversation::{
        agent::PermissionHook as _,
        types::{PermissionDecision, PermissionRequest},
    };
    let mut fixture = StreamFixture::open();
    let (group, _) = fixture.begin("current");
    let queue = fixture
        .cx
        .update(|cx| fixture.stream.read(cx).permission_queue.clone());
    fixture.cx.update(|cx| {
        fixture.stream.update(cx, |stream, cx| {
            stream.apply_event(
                ConversationEvent::ToolCallProposed {
                    call: ToolCall {
                        id: "pending-permission".into(),
                        tool: "bash".into(),
                        input_json: r#"{"command":"printf requires-human"}"#.into(),
                    },
                },
                cx,
            )
        })
    });
    let mut decision = queue.request(
        PermissionRequest {
            call_id: "pending-permission".into(),
            tool: "bash".into(),
            display_target: "printf requires-human".into(),
            danger_rule_id: None,
            danger_reason: None,
            external: None,
            acp_options: None,
        },
        tokio_util::sync::CancellationToken::new(),
    );
    fixture.cx.run_until_parked();
    fixture.cx.update(|cx| {
        fixture
            .stream
            .update(cx, |stream, cx| stream.install_pending_permission(cx))
    });
    fixture.draw();
    let permission = fixture.cx.update(|cx| {
        fixture
            .stream
            .read(cx)
            .active_permission
            .clone()
            .expect("WIRING: real queue request matched actual ToolCard and installed a lease")
    });
    let entry =
        fixture.cx.update(|cx| {
            fixture.stream.read(cx).entries.iter().position(|entry|
        matches!(entry, StreamEntry::Permission { card } if card == &permission)
    ).unwrap()
        });
    let before = fixture.cx.update(|cx| {
        fixture
            .stream
            .read(cx)
            .list
            .bounds_for_item(entry)
            .expect("WIRING: actual permission entry visible")
    });
    assert!(before.size.height > px(0.));
    assert!(queue.has_pending());
    fixture.final_event("current", COMMENTARY.len() + FINAL_TEXT.len());
    fixture.cx.run_until_parked();
    fixture.draw();
    fixture
        .cx
        .update_window(fixture.window.into(), |_, window, cx| {
            let stream = fixture.stream.read(cx);
            assert!(!group.read(cx).expanded());
            assert_eq!(stream.active_permission.as_ref(), Some(&permission));
            let bounds = stream
                .list
                .bounds_for_item(entry)
                .expect("permission cannot hide inside process details");
            assert!(bounds.size.height > px(0.));
            assert!(bounds.bottom() <= window.viewport_size().height);
            assert!(
                window.focused(cx).is_some(),
                "actual permission control obtained initial focus"
            );
            window.dispatch_action(Box::new(crate::permission_card::PermissionEnter), cx);
        })
        .unwrap();
    fixture.cx.run_until_parked();
    fixture.draw();
    let mut task = TaskContext::from_waker(Waker::noop());
    match Future::poll(Pin::as_mut(&mut decision), &mut task) {
        Poll::Ready(Ok(PermissionDecision::Once)) => {}
        _ => panic!(
            "actual visible permission action must resolve the real queue lease as AllowOnce"
        ),
    }
    assert!(!queue.has_pending());
    fixture
        .cx
        .update(|cx| assert!(fixture.stream.read(cx).active_permission.is_none()));
}

#[gpui_kit::test]
fn long_thinking_stream_interleaves_reversals_and_real_composer_character_input(
    _: &mut gpui_kit::TestAppContext,
) {
    for (width, dark) in [(240., false), (480., true)] {
        let mut fixture = StreamFixture::open();
        fixture.cx.update(|cx| {
            cx.set_global(if dark {
                vega_theme::Theme::dark()
            } else {
                vega_theme::Theme::light()
            });
            fixture
                .stream
                .update(cx, |stream, cx| stream.set_workspace_width(width, cx));
        });
        fixture.cx.update(|cx| {
            fixture.stream.update(cx, |stream, cx| {
                stream.apply_event(
                    ConversationEvent::MessageStarted {
                        message_id: "current".into(),
                        seq: 1,
                    },
                    cx,
                );
                stream.apply_event(
                    ConversationEvent::AssistantResponseStarted {
                        message_id: "current".into(),
                        response_id: "commentary".into(),
                        text_start_bytes: 0,
                    },
                    cx,
                );
                stream.apply_event(
                    ConversationEvent::ThinkingDelta {
                        message_id: "current".into(),
                        delta: SOURCE.repeat(12),
                    },
                    cx,
                );
            });
        });
        let (group, thinking) = fixture.expand_thinking_stage("current");
        fixture.cx.update(|cx| {
            assert_eq!(
                fixture.stream.read(cx).active_thinking.as_ref(),
                Some(&thinking)
            );
            assert!(!thinking.read(cx).progress_finished);
        });
        let mut expected_source = SOURCE.repeat(12);
        let mut typed = String::new();
        let input = fixture
            .cx
            .update(|cx| fixture.stream.read(cx).input.clone());
        for (step, key) in ["a", "b", "c", "d", "e", "f", "g", "h"]
            .into_iter()
            .enumerate()
        {
            let glyph = fixture
                .draw()
                .into_iter()
                .find(|paint| {
                    paint.scope.owner.as_ref() == format!("run-stage-{}-0", group.entity_id())
                })
                .unwrap();
            fixture
                .cx
                .update_window(fixture.window.into(), |_, window, cx| {
                    glyph.scope.focus.focus(window, cx);
                    window.dispatch_keystroke(Keystroke::parse("space").unwrap(), cx);
                    fixture
                        .stream
                        .update(cx, |stream, cx| stream.focus_composer(window, cx));
                })
                .unwrap();
            fixture.draw();
            let delta = format!("stream chunk {step}: {}", SOURCE.repeat(80));
            expected_source.push_str(&delta);
            fixture.cx.update(|cx| {
                fixture.stream.update(cx, |stream, cx| {
                    stream.apply_event(
                        ConversationEvent::ThinkingDelta {
                            message_id: "current".into(),
                            delta,
                        },
                        cx,
                    )
                })
            });
            fixture.draw();
            fixture
                .cx
                .update_window(fixture.window.into(), |_, window, cx| {
                    use gpui_kit::Focusable as _;
                    assert!(input.read(cx).focus_handle(cx).is_focused(window));
                    window.dispatch_keystroke(Keystroke::parse(key).unwrap(), cx);
                    typed.push_str(key);
                    assert_eq!(
                        input.read(cx).text(),
                        typed,
                        "input must update in the event cycle, never after animation"
                    );
                })
                .unwrap();
            fixture.cx.advance_clock(Duration::from_millis(15));
            fixture
                .cx
                .update_window(fixture.window.into(), |_, window, cx| {
                    window.simulate_next_frame(cx);
                })
                .unwrap();
            let next = fixture
                .draw()
                .into_iter()
                .find(|paint| paint.scope.owner == glyph.scope.owner)
                .unwrap();
            assert_eq!(next.bounds.size, glyph.bounds.size);
            fixture.cx.update(|cx| {
                assert_eq!(thinking.read(cx).text, expected_source);
                assert_eq!(input.read(cx).text(), typed);
                assert_eq!(
                    fixture.stream.read(cx).active_thinking.as_ref(),
                    Some(&thinking)
                );
                assert!(!thinking.read(cx).progress_finished);
            });
        }
        fixture.cx.update(|cx| {
            fixture.stream.update(cx, |stream, cx| {
                stream.apply_event(
                    ConversationEvent::TextDelta {
                        message_id: "current".into(),
                        delta: COMMENTARY.into(),
                    },
                    cx,
                );
                stream.apply_event(
                    ConversationEvent::AssistantResponseCompleted {
                        message_id: "current".into(),
                        response: response(
                            "commentary",
                            AssistantResponseKind::Commentary,
                            0,
                            COMMENTARY.len(),
                        ),
                    },
                    cx,
                );
            });
        });
        fixture.final_event("current", COMMENTARY.len() + FINAL_TEXT.len());
        fixture
            .cx
            .update_window(fixture.window.into(), |_, window, cx| {
                use gpui_kit::Focusable as _;
                assert!(!group.read(cx).expanded());
                assert!(input.read(cx).focus_handle(cx).is_focused(window));
                assert_eq!(input.read(cx).text(), typed);
                assert_eq!(thinking.read(cx).text, expected_source);
                assert_eq!(
                    fixture
                        .stream
                        .read(cx)
                        .completion_response(&["current".into()], cx)
                        .as_deref(),
                    Some(FINAL_TEXT)
                );
            })
            .unwrap();
    }
}
