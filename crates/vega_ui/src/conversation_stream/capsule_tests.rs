use super::*;
use gpui_kit as gpui;
use gpui_kit::{
    AppContext, Bounds, Element, ElementId, Focusable, Global, GlobalElementId, Hsla,
    InspectorElementId, LayoutId, ScaledPixels, SharedString, TestAppContext, TextLayout,
    VisualTestContext, point, size, test::TestWindowExt,
};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};
use vega_conversation::types::ThreadStatus;
use vega_theme::Theme;

#[derive(Clone, Debug)]
struct PaintedChild {
    bounds: Bounds<Pixels>,
    mask: Bounds<Pixels>,
    viewport: Bounds<Pixels>,
    ink: Hsla,
    text: Option<String>,
    icon: Option<(crate::icons::Icon, Rgba)>,
}

#[derive(Default)]
struct PaintedChildren(BTreeMap<String, PaintedChild>);

impl Global for PaintedChildren {}

pub(super) struct ObservedChild<E: Element> {
    inner: E,
    key: String,
    text: Option<fn(&E::RequestLayoutState) -> String>,
    icon: Option<(crate::icons::Icon, Rgba)>,
}

pub(super) fn observe_capsule_text<T: Into<SharedString>>(
    key: impl Into<String>,
    text: T,
) -> ObservedChild<SharedString> {
    ObservedChild {
        inner: text.into(),
        key: key.into(),
        text: Some(TextLayout::text),
        icon: None,
    }
}

pub(super) fn observe_capsule_icon(
    key: &'static str,
    glyph: crate::icons::Icon,
    ink: Rgba,
) -> ObservedChild<AnyElement> {
    ObservedChild {
        inner: crate::icons::icon(glyph, ink),
        key: key.into(),
        text: None,
        icon: Some((glyph, ink)),
    }
}

impl<E: Element> IntoElement for ObservedChild<E> {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl<E: Element> Element for ObservedChild<E> {
    type RequestLayoutState = E::RequestLayoutState;
    type PrepaintState = E::PrepaintState;

    fn id(&self) -> Option<ElementId> {
        self.inner.id()
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        self.inner.source_location()
    }

    fn a11y_role(&self) -> Option<gpui_kit::Role> {
        self.inner.a11y_role()
    }

    fn write_a11y_info(&self, node: &mut gpui_kit::accesskit::Node) {
        self.inner.write_a11y_info(node);
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.inner.request_layout(id, inspector, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.inner
            .prepaint(id, inspector, bounds, layout, window, cx)
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let child = PaintedChild {
            bounds,
            mask: window.content_mask().bounds,
            viewport: Bounds {
                origin: point(px(0.), px(0.)),
                size: window.viewport_size(),
            },
            ink: window.text_style().color,
            text: self.text.map(|read| read(layout)),
            icon: self.icon,
        };
        self.inner
            .paint(id, inspector, bounds, layout, prepaint, window, cx);
        cx.update_global::<PaintedChildren, _>(|children, _| {
            children.0.insert(self.key.clone(), child);
        });
    }
}

struct ComposerHarness {
    stream: Entity<ConversationStream>,
    full_stream: bool,
}

impl Render for ComposerHarness {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.full_stream {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .child(self.stream.clone());
        }
        let composer = self
            .stream
            .update(cx, |stream, cx| stream.render_composer(window, cx));
        div()
            .size_full()
            .flex()
            .flex_col()
            .justify_end()
            .bg(theme(cx).colors.bg_base)
            .child(composer)
    }
}

struct Fixture {
    stream: Entity<ConversationStream>,
    cx: VisualTestContext,
    models: Rc<RefCell<Vec<ThreadModelSelectionRequested>>>,
    permissions: Rc<RefCell<Vec<ThreadSettingsRequested>>>,
    colors: ThemeColors,
}

impl Fixture {
    fn new(cx: &mut TestAppContext, palette: Theme, width: f32, model: &str) -> Self {
        Self::with_render_mode(cx, palette, width, model, false)
    }

    fn full_stream(cx: &mut TestAppContext, palette: Theme, width: f32, model: &str) -> Self {
        Self::with_render_mode(cx, palette, width, model, true)
    }

    fn with_render_mode(
        cx: &mut TestAppContext,
        palette: Theme,
        width: f32,
        model: &str,
        full_stream: bool,
    ) -> Self {
        let colors = palette.colors;
        cx.update(|cx| {
            if !cx.has_global::<PaintedChildren>() {
                gpui_kit::init(cx);
                crate::init(cx);
            }
            cx.set_global(palette);
            cx.set_global(PaintedChildren::default());
            cx.set_reduce_motion(true);
        });
        let thread = Thread {
            parent_thread_id: None,
            id: "capsule-thread".into(),
            backend: TaskBackend::Native,
            project_id: String::new(),
            title: "Composer controls".into(),
            mode: ThreadMode::Execute,
            permission_mode: PermissionMode::Confirm,
            model: model.into(),
            status: ThreadStatus::Active,
            pinned: false,
            unread: false,
            created_at: 0,
            updated_at: 0,
        };
        let models = Rc::new(RefCell::new(Vec::new()));
        let permissions = Rc::new(RefCell::new(Vec::new()));
        let model_events = models.clone();
        let permission_events = permissions.clone();
        let window = cx.open_window(
            size(px(width + 2. * Layout::CONVERSATION_PADDING), px(600.)),
            |_, cx| {
                let stream = cx.new(|cx| ConversationStream::new(thread, cx));
                cx.observe(&stream, |_, _, cx| cx.notify()).detach();
                cx.subscribe(
                    &stream,
                    move |_, _, event: &ThreadModelSelectionRequested, _| {
                        model_events.borrow_mut().push(event.clone());
                    },
                )
                .detach();
                cx.subscribe(&stream, move |_, _, event: &ThreadSettingsRequested, _| {
                    permission_events.borrow_mut().push(event.clone());
                })
                .detach();
                ComposerHarness {
                    stream,
                    full_stream,
                }
            },
        );
        let root = window.root(cx).expect("composer harness");
        let stream = root.read_with(cx, |root, _| root.stream.clone());
        let mut fixture = Self {
            stream,
            cx: VisualTestContext::from_window(window.into(), cx),
            models,
            permissions,
            colors,
        };
        fixture.change(|stream, _, cx| {
            stream.apply_model_options(vec![model.into(), "alternate-model".into()], cx);
        });
        fixture.move_out();
        fixture
    }

    fn frame(&mut self) {
        self.cx.run_until_parked();
        self.cx.update(|window, cx| {
            cx.update_global::<PaintedChildren, _>(|children, _| children.0.clear());
            window.render_frame(cx);
        });
    }

    fn change(
        &mut self,
        change: impl FnOnce(&mut ConversationStream, &mut Window, &mut Context<ConversationStream>),
    ) {
        self.cx.update(|window, cx| {
            self.stream
                .update(cx, |stream, cx| change(stream, window, cx));
        });
        self.frame();
    }

    fn read<R>(&mut self, read: impl FnOnce(&ConversationStream, &App) -> R) -> R {
        self.stream.read_with(&self.cx, read)
    }

    fn bounds(&mut self, selector: &'static str) -> Bounds<Pixels> {
        self.cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("missing rendered control {selector}"))
    }

    fn child(&mut self, key: &str) -> PaintedChild {
        let child = self.cx.update(|_, cx| {
            cx.global::<PaintedChildren>()
                .0
                .get(key)
                .unwrap_or_else(|| panic!("missing painted child {key}"))
                .clone()
        });
        assert_visible(key, child.bounds, child.mask, child.viewport);
        child
    }

    fn move_out(&mut self) {
        self.cx
            .simulate_mouse_move(point(px(1.), px(1.)), None, Default::default());
        self.frame();
    }

    fn hover(&mut self, selector: &'static str) {
        self.move_out();
        let center = self.bounds(selector).center();
        self.cx
            .simulate_mouse_move(center, None, Default::default());
        self.frame();
    }

    fn click(&mut self, selector: &'static str) {
        self.hover(selector);
        let center = self.bounds(selector).center();
        self.cx.simulate_click(center, Default::default());
        self.frame();
    }

    fn key(&mut self, key: &str) {
        self.cx.simulate_keystrokes(key);
        self.frame();
    }

    fn focus_model(&mut self) {
        self.change(|stream, window, cx| stream.model_focus.focus(window, cx));
        self.assert_model_focused();
    }

    fn assert_model_focused(&mut self) {
        assert!(
            self.cx
                .update(|window, cx| { self.stream.read(cx).model_focus.is_focused(window) })
        );
    }

    fn focus_input(&mut self) {
        self.change(|stream, window, cx| stream.focus_composer(window, cx));
        self.assert_input_focused();
    }

    fn assert_input_focused(&mut self) {
        assert!(self.cx.update(|window, cx| {
            self.stream
                .read(cx)
                .input
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
        }));
    }

    fn click_outside_composer(&mut self) {
        let outside = point(px(1.), px(1.));
        assert!(!self.bounds("capsule-composer").contains(&outside));
        self.cx.simulate_click(outside, Default::default());
        self.frame();
    }

    fn tiers(&mut self) {
        self.change(|stream, _, cx| {
            let model = stream.composer_defaults.model.clone();
            stream.thinking_slider.update(cx, |slider, cx| {
                slider.set_model_and_tiers(
                    model,
                    vec!["low".into(), "high".into()],
                    false,
                    "low",
                    "low",
                    cx,
                );
            });
            cx.notify();
        });
    }

    fn acknowledge_model(&mut self) {
        let event = self.models.borrow().last().expect("model request").clone();
        self.change(|stream, _, cx| {
            stream.apply_thread_model_acknowledged(
                &event.thread_id,
                event.request_id,
                &event.model,
                stream.thread.clone(),
                cx,
            );
        });
    }

    fn surface(&mut self, selector: &'static str) -> PaintedSurface {
        let bounds = self.bounds(selector);
        self.cx.update(|window, _| {
            let scale = window.scale_factor();
            let surfaces: Vec<_> = window
                .painted_quads()
                .into_iter()
                .filter_map(|quad| {
                    let logical = logical_bounds(quad.bounds, scale);
                    let fill = quad.background.as_solid()?;
                    (same_bounds(logical, bounds) && fill.a > 0.).then_some(PaintedSurface {
                        bounds: logical,
                        mask: logical_bounds(quad.content_mask.bounds, scale),
                        viewport: Bounds { origin: point(px(0.), px(0.)), size: window.viewport_size() },
                        fill,
                        radii: [quad.corner_radii.top_left.0, quad.corner_radii.top_right.0, quad.corner_radii.bottom_right.0, quad.corner_radii.bottom_left.0].map(|radius| radius / scale),
                        borders: [quad.border_widths.top.0, quad.border_widths.right.0, quad.border_widths.bottom.0, quad.border_widths.left.0].map(|border| border / scale),
                    })
                })
                .collect();
            assert_eq!(surfaces.len(), 1, "{selector}: expected exactly one actual filled trigger quad at {bounds:?}, got {surfaces:?}");
            surfaces.into_iter().next().expect("filled trigger quad")
        })
    }
}

#[derive(Debug)]
struct PaintedSurface {
    bounds: Bounds<Pixels>,
    mask: Bounds<Pixels>,
    viewport: Bounds<Pixels>,
    fill: Hsla,
    radii: [f32; 4],
    borders: [f32; 4],
}

fn logical_bounds(bounds: Bounds<ScaledPixels>, scale: f32) -> Bounds<Pixels> {
    Bounds {
        origin: point(px(bounds.origin.x.0 / scale), px(bounds.origin.y.0 / scale)),
        size: size(
            px(bounds.size.width.0 / scale),
            px(bounds.size.height.0 / scale),
        ),
    }
}

fn assert_visible(
    key: &str,
    bounds: Bounds<Pixels>,
    mask: Bounds<Pixels>,
    viewport: Bounds<Pixels>,
) {
    let visible = mask.intersect(&viewport);
    assert!(
        bounds.size.width > px(0.) && bounds.size.height > px(0.),
        "{key}: empty painted bounds {bounds:?}"
    );
    assert!(
        bounds.left() >= visible.left()
            && bounds.top() >= visible.top()
            && bounds.right() <= visible.right()
            && bounds.bottom() <= visible.bottom(),
        "{key}: {bounds:?} clipped by mask {mask:?} or viewport {viewport:?}"
    );
}

fn near(actual: Pixels, expected: f32) {
    assert!(
        (f32::from(actual) - expected).abs() <= 0.5,
        "expected {expected}px, got {actual:?}"
    );
}

fn same_bounds(left: Bounds<Pixels>, right: Bounds<Pixels>) -> bool {
    [
        left.origin.x - right.origin.x,
        left.origin.y - right.origin.y,
        left.size.width - right.size.width,
        left.size.height - right.size.height,
    ]
    .into_iter()
    .all(|delta| f32::from(delta).abs() <= 0.5)
}

fn assert_capsule(surface: &PaintedSurface, color: Rgba) {
    assert_visible(
        "capsule background",
        surface.bounds,
        surface.mask,
        surface.viewport,
    );
    near(surface.bounds.size.height, 28.);
    assert!(surface.bounds.size.width >= px(28.));
    assert_eq!(
        surface.radii, [14.; 4],
        "all four painted corners must be semicircular"
    );
    assert_eq!(surface.fill, Hsla::from(color));
    assert_eq!(surface.borders, [0.; 4]);
}

fn assert_trigger_visibility(fixture: &mut Fixture) {
    for selector in ["capsule-model", "capsule-permission"] {
        let surface = fixture.surface(selector);
        assert_visible(selector, surface.bounds, surface.mask, surface.viewport);
    }
    for key in [
        "model-label",
        "model-chevron",
        "permission-label",
        "permission-icon",
    ] {
        fixture.child(key);
    }
}

fn assert_padding(fixture: &mut Fixture) {
    let model = fixture.bounds("capsule-model");
    let label = fixture.child("model-label");
    let chevron = fixture.child("model-chevron");
    near(label.bounds.left() - model.left(), 12.);
    near(model.right() - chevron.bounds.right(), 12.);
    let permission = fixture.bounds("capsule-permission");
    let glyph = fixture.child("permission-icon");
    let label = fixture.child("permission-label");
    near(glyph.bounds.left() - permission.left(), 12.);
    near(permission.right() - label.bounds.right(), 12.);
}

fn resting_capsules(cx: &mut TestAppContext, palette: Theme) {
    let mut fixture = Fixture::new(cx, palette, 768., "alpha");
    for selector in ["capsule-model", "capsule-permission"] {
        let surface = fixture.surface(selector);
        assert_capsule(&surface, fixture.colors.bg_hover);
    }
    assert_padding(&mut fixture);
    assert_eq!(fixture.child("model-label").text.as_deref(), Some("alpha"));
    assert_eq!(
        fixture.child("model-label").ink,
        Hsla::from(fixture.colors.text_secondary)
    );
}

#[gpui_kit::test]
fn light_composer_triggers_paint_filled_symmetric_capsules(cx: &mut TestAppContext) {
    resting_capsules(cx, Theme::light());
}

#[gpui_kit::test]
fn dark_composer_triggers_paint_filled_symmetric_capsules(cx: &mut TestAppContext) {
    resting_capsules(cx, Theme::dark());
}

#[gpui_kit::test]
fn hover_press_and_open_keep_capsule_geometry_and_active_fill(cx: &mut TestAppContext) {
    let mut fixture = Fixture::new(cx, Theme::light(), 768., "alpha");
    fixture.tiers();
    for selector in ["capsule-model", "capsule-permission"] {
        fixture.move_out();
        let resting = fixture.bounds(selector);
        fixture.hover(selector);
        assert!(
            !fixture
                .read(|stream, _| stream.model_picker_level.is_open()
                    || stream.permission_picker_open)
        );
        assert_capsule(&fixture.surface(selector), fixture.colors.bg_active);
        assert!(same_bounds(fixture.bounds(selector), resting));
        assert_padding(&mut fixture);
        let center = resting.center();
        fixture
            .cx
            .simulate_mouse_down(center, MouseButton::Left, Default::default());
        fixture.frame();
        assert_capsule(&fixture.surface(selector), fixture.colors.bg_active);
        assert!(same_bounds(fixture.bounds(selector), resting));
        assert_padding(&mut fixture);
        fixture
            .cx
            .simulate_mouse_up(center, MouseButton::Left, Default::default());
        fixture.frame();
        assert!(fixture.read(|stream, _| {
            if selector == "capsule-model" {
                stream.model_picker_level == ModelPickerLevel::Slider
            } else {
                stream.permission_picker_open
            }
        }));
        fixture.move_out();
        assert_capsule(&fixture.surface(selector), fixture.colors.bg_active);
        assert!(same_bounds(fixture.bounds(selector), resting));
        assert_padding(&mut fixture);
        fixture.click(selector);
        fixture.move_out();
        assert!(
            !fixture
                .read(|stream, _| stream.model_picker_level.is_open()
                    || stream.permission_picker_open)
        );
        assert_capsule(&fixture.surface(selector), fixture.colors.bg_hover);
    }
}

#[derive(Clone, Copy, Debug)]
enum BusyState {
    Pending,
    Saving,
    TrustedAction,
}

fn make_busy(fixture: &mut Fixture, state: BusyState) {
    fixture.change(|stream, _, cx| match state {
        BusyState::Pending => stream.request_model_selection("alternate-model", cx),
        BusyState::Saving => {
            stream.request_model_selection("alternate-model", cx);
            let request_id = stream
                .model_selection_pending
                .as_ref()
                .expect("pending model")
                .0;
            assert!(stream.install_model_selection_save_owner(request_id, cx));
        }
        BusyState::TrustedAction => stream.set_trusted_action_busy(true, cx),
    });
}

#[gpui_kit::test]
fn busy_model_controls_reject_mouse_and_keyboard_without_new_requests(cx: &mut TestAppContext) {
    for state in [
        BusyState::Pending,
        BusyState::Saving,
        BusyState::TrustedAction,
    ] {
        let mut fixture = Fixture::new(cx, Theme::light(), 768., "alpha");
        make_busy(&mut fixture, state);
        let events = fixture.models.borrow().clone();
        let bounds = fixture.bounds("capsule-model");
        fixture.click("capsule-model");
        fixture.focus_model();
        fixture.key("space enter");
        assert_eq!(
            fixture.read(|stream, _| stream.model_picker_level),
            ModelPickerLevel::Closed,
            "{state:?}"
        );
        assert!(fixture.cx.debug_bounds("capsule-model-menu").is_none());
        assert_eq!(*fixture.models.borrow(), events, "{state:?}");
        assert!(same_bounds(fixture.bounds("capsule-model"), bounds));
        fixture
            .cx
            .update(|window, _| assert_eq!(window.find("composer-model").disabled(), Some(true)));
    }
}

#[gpui_kit::test]
fn disabled_model_keeps_resting_fill_and_geometry_under_pointer_events(cx: &mut TestAppContext) {
    for state in [
        BusyState::Pending,
        BusyState::Saving,
        BusyState::TrustedAction,
    ] {
        let mut fixture = Fixture::new(cx, Theme::dark(), 768., "alpha");
        let enabled = fixture.bounds("capsule-model");
        make_busy(&mut fixture, state);
        let busy = fixture.bounds("capsule-model");
        let label = fixture.child("model-label");
        if matches!(state, BusyState::TrustedAction) {
            assert!(same_bounds(busy, enabled), "{state:?}");
            assert_eq!(label.text.as_deref(), Some("alpha"));
        } else {
            assert_eq!(label.text.as_deref(), Some("保存中…"));
        }
        assert_eq!(label.ink, Hsla::from(fixture.colors.text_tertiary));
        let assert_busy_geometry = |fixture: &mut Fixture| {
            assert_capsule(&fixture.surface("capsule-model"), fixture.colors.bg_hover);
            assert!(
                same_bounds(fixture.bounds("capsule-model"), busy),
                "{state:?}"
            );
            assert_padding(fixture);
        };
        assert_busy_geometry(&mut fixture);
        fixture.hover("capsule-model");
        assert_busy_geometry(&mut fixture);
        let center = busy.center();
        fixture
            .cx
            .simulate_mouse_down(center, MouseButton::Left, Default::default());
        fixture.frame();
        assert_busy_geometry(&mut fixture);
        fixture
            .cx
            .simulate_mouse_up(center, MouseButton::Left, Default::default());
        fixture.frame();
        assert_busy_geometry(&mut fixture);
        fixture.move_out();
        assert_busy_geometry(&mut fixture);
    }
}

#[gpui_kit::test]
fn model_mouse_and_keyboard_keep_capability_layers_and_selection_requests(cx: &mut TestAppContext) {
    let mut fixture = Fixture::full_stream(cx, Theme::light(), 768., "alpha");
    fixture.click("capsule-model");
    assert_eq!(
        fixture.read(|stream, _| stream.model_picker_level),
        ModelPickerLevel::List
    );
    fixture.bounds("capsule-model-menu");
    fixture.click("capsule-model-option-1");
    assert_eq!(fixture.models.borrow().len(), 1);
    assert_eq!(fixture.models.borrow()[0].model, "alternate-model");
    assert_eq!(
        fixture.read(|stream, _| stream.thread.model.clone()),
        "alpha"
    );
    assert_eq!(
        fixture.read(|stream, _| stream.model_picker_level),
        ModelPickerLevel::Closed
    );
    fixture.acknowledge_model();
    assert_eq!(
        fixture.child("model-label").text.as_deref(),
        Some("alternate-model")
    );
    fixture.focus_input();
    fixture.key("tab");
    assert!(
        fixture
            .cx
            .update(|window, cx| fixture.stream.read(cx).action_focus[0].is_focused(window))
    );
    fixture.key("tab");
    fixture.assert_model_focused();
    fixture.key("space");
    fixture.bounds("capsule-model-menu");
    fixture.key("up enter");
    assert_eq!(fixture.models.borrow().len(), 2);
    assert_eq!(fixture.models.borrow()[1].model, "alpha");
    fixture.acknowledge_model();
    fixture.focus_model();
    fixture.key("enter escape");
    assert_eq!(fixture.models.borrow().len(), 2);
    assert_eq!(
        fixture.read(|stream, _| stream.model_picker_level),
        ModelPickerLevel::Closed
    );
    fixture.tiers();
    let closed = fixture.bounds("capsule-model");
    fixture.click("capsule-model");
    assert_eq!(
        fixture.read(|stream, _| stream.model_picker_level),
        ModelPickerLevel::Slider
    );
    assert_eq!(
        fixture.child("model-label").text.as_deref(),
        Some("选择强度")
    );
    assert!(same_bounds(fixture.bounds("capsule-model"), closed));
    fixture.click("capsule-model");
    assert_eq!(
        fixture.read(|stream, _| stream.model_picker_level),
        ModelPickerLevel::Closed
    );
    fixture.focus_model();
    fixture.key("enter");
    assert_eq!(
        fixture.read(|stream, _| stream.model_picker_level),
        ModelPickerLevel::Slider
    );
    fixture.key("escape");
    assert_eq!(
        fixture.read(|stream, _| stream.model_picker_level),
        ModelPickerLevel::Closed
    );
    fixture
        .cx
        .update(|window, _| assert_eq!(window.find("composer-model").label(), Some("alpha")));
    fixture.click("capsule-model");
    assert_eq!(
        fixture.read(|stream, _| stream.model_picker_level),
        ModelPickerLevel::Slider
    );
    fixture.click("capsule-slider-title");
    assert_eq!(
        fixture.read(|stream, _| stream.model_picker_level),
        ModelPickerLevel::List
    );
    fixture.bounds("capsule-model-menu");
    fixture.click_outside_composer();
    assert_eq!(
        fixture.read(|stream, _| stream.model_picker_level),
        ModelPickerLevel::Closed
    );
    assert!(fixture.cx.debug_bounds("capsule-model-menu").is_none());
    assert_eq!(fixture.models.borrow().len(), 2);
}

#[gpui_kit::test]
fn permission_menu_preserves_modes_warning_ink_and_pending_request_guard(cx: &mut TestAppContext) {
    let mut fixture = Fixture::full_stream(cx, Theme::light(), 768., "alpha");
    let modes = [
        (
            PermissionMode::ReadOnly,
            "只读",
            crate::icons::Icon::Hand,
            false,
        ),
        (
            PermissionMode::Confirm,
            "确认",
            crate::icons::Icon::Shield,
            false,
        ),
        (
            PermissionMode::Auto,
            "自动",
            crate::icons::Icon::Warning,
            true,
        ),
        (
            PermissionMode::FullAccess,
            "完全访问",
            crate::icons::Icon::Warning,
            true,
        ),
    ];
    for (mode, label, glyph, warning) in modes {
        fixture.change(|stream, _, cx| {
            stream.thread.permission_mode = mode;
            cx.notify();
        });
        let ink = if warning {
            fixture.colors.warning
        } else {
            fixture.colors.text_secondary
        };
        assert_eq!(
            fixture.child("permission-label").text.as_deref(),
            Some(label)
        );
        assert_eq!(fixture.child("permission-label").ink, Hsla::from(ink));
        assert_eq!(fixture.child("permission-icon").icon, Some((glyph, ink)));
    }
    fixture.change(|stream, _, cx| {
        stream.thread.permission_mode = PermissionMode::Confirm;
        cx.notify();
    });
    fixture.focus_model();
    fixture.click("capsule-permission");
    assert!(fixture.read(|stream, _| stream.permission_picker_open));
    fixture.assert_input_focused();
    for (index, (_, label, _, warning)) in modes.into_iter().enumerate() {
        let child = fixture.child(&format!("permission-option-label-{index}"));
        assert_eq!(child.text.as_deref(), Some(label));
        assert_eq!(
            child.ink,
            Hsla::from(if warning {
                fixture.colors.warning
            } else {
                fixture.colors.text_primary
            })
        );
    }
    assert_eq!(
        fixture
            .child("permission-option-description-3")
            .text
            .as_deref(),
        Some("不使用沙箱，危险操作仍需确认")
    );
    fixture.focus_model();
    fixture.click("capsule-permission-option-3");
    fixture.assert_input_focused();
    assert_eq!(
        *fixture.permissions.borrow(),
        vec![ThreadSettingsRequested {
            thread_id: "capsule-thread".into(),
            mode: None,
            permission_mode: Some(PermissionMode::FullAccess),
        }]
    );
    assert_eq!(
        fixture.read(|stream, _| stream.thread.permission_mode),
        PermissionMode::Confirm
    );
    assert!(!fixture.read(|stream, _| stream.permission_picker_open));
    fixture.click("capsule-permission");
    fixture.assert_input_focused();
    fixture.click("capsule-permission-option-1");
    fixture.assert_input_focused();
    assert_eq!(fixture.permissions.borrow().len(), 1);
    make_busy(&mut fixture, BusyState::Pending);
    fixture.click("capsule-permission");
    assert!(fixture.read(|stream, _| stream.permission_picker_open));
    fixture.assert_input_focused();
    fixture.click("capsule-permission-option-3");
    fixture.assert_input_focused();
    assert_eq!(fixture.permissions.borrow().len(), 1);
    assert_eq!(
        fixture.read(|stream, _| stream.thread.permission_mode),
        PermissionMode::Confirm
    );
    fixture.click("capsule-permission");
    assert!(fixture.read(|stream, _| stream.permission_picker_open));
    fixture.assert_input_focused();
    fixture.key("escape");
    assert!(!fixture.read(|stream, _| stream.permission_picker_open));
    fixture.assert_input_focused();
    fixture.click("capsule-permission");
    assert!(fixture.read(|stream, _| stream.permission_picker_open));
    fixture.click_outside_composer();
    assert!(!fixture.read(|stream, _| stream.permission_picker_open));
    assert_eq!(fixture.permissions.borrow().len(), 1);
}

#[gpui_kit::test]
fn narrow_and_long_model_layout_preserves_primary_controls_and_reserved_label_width(
    cx: &mut TestAppContext,
) {
    let long_model = "provider-with-a-very-long-model-name-that-must-not-remove-the-model-chevron-or-send-control";
    for (width, model) in [
        (768., "alpha"),
        (480., "alpha"),
        (768., long_model),
        (480., long_model),
    ] {
        let mut fixture = Fixture::new(cx, Theme::light(), width, model);
        fixture.tiers();
        let shell = fixture.bounds("capsule-composer");
        near(shell.size.width, width);
        assert_trigger_visibility(&mut fixture);
        let controls = [
            fixture.bounds("capsule-add"),
            fixture.bounds("capsule-permission"),
            fixture.bounds("capsule-model"),
            fixture.bounds("capsule-send"),
        ];
        for control in controls {
            assert!(
                control.is_contained_within(&shell),
                "{width}: {control:?} outside {shell:?}"
            );
            assert!(control.size.width >= px(28.));
        }
        for pair in controls.windows(2) {
            assert!(
                pair[0].right() < pair[1].left(),
                "{width}: overlapping controls {pair:?}"
            );
        }
        let label = fixture.child("model-label");
        let chevron = fixture.child("model-chevron");
        assert_eq!(
            chevron.icon,
            Some((
                crate::icons::Icon::ChevronDown,
                fixture.colors.text_tertiary
            ))
        );
        assert!(label.bounds.right() <= chevron.bounds.left());
        assert!(chevron.bounds.right() <= controls[2].right());
        if model == long_model {
            let rendered = label.text.expect("painted model label");
            assert_ne!(
                rendered, long_model,
                "long labels must actually truncate at {width}px"
            );
            assert!(
                rendered.ends_with('…'),
                "expected painted ellipsis, got {rendered:?}"
            );
        } else {
            assert_eq!(label.text.as_deref(), Some(model));
        }
        fixture.click("capsule-model");
        assert_eq!(
            fixture.read(|stream, _| stream.model_picker_level),
            ModelPickerLevel::Slider
        );
        let opened = [
            fixture.bounds("capsule-add"),
            fixture.bounds("capsule-permission"),
            fixture.bounds("capsule-model"),
            fixture.bounds("capsule-send"),
        ];
        for (before, after) in controls.into_iter().zip(opened) {
            assert!(
                same_bounds(before, after),
                "{width}: opening model shifted {before:?} to {after:?}"
            );
        }
        assert_trigger_visibility(&mut fixture);
        fixture.cx.update(|window, _| {
            assert_eq!(window.find("composer-model").label(), Some("选择强度"))
        });
    }
}
