use super::{ProjectionId, Request, Snapshot, pixels};
use crate::nsh::color;
use nsh_core::canvas::{CanvasOp, Drawing, LineCap, LineJoin, Num, Paint, PathOp};
use nsh_core::element::{JsEvent, Root};
use nsh_core::prelude as ns;
use nsh_core::{
    AccessibilityRole, ApproxTextEngine, Core, EventKind, EventPayload, MountItem, NodeId,
};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use vega_theme::{Layout, Theme};

thread_local! {
    static INITIAL: RefCell<Option<Snapshot>> = const { RefCell::new(None) };
    static CHANNELS: RefCell<Option<Channels>> = const { RefCell::new(None) };
}

struct Channels {
    updates: mpsc::Sender<Snapshot>,
    requests: mpsc::Receiver<Request>,
}

#[ns::component]
fn Fixture() -> impl ns::IntoElement {
    let initial = INITIAL.with(|slot| slot.borrow_mut().take().expect("initial snapshot"));
    let state = ns::signal_with(initial, ns::SignalEquals::Never);
    let (updates, mut incoming) = mpsc::channel(8);
    let (sender, requests) = mpsc::channel(1);
    CHANNELS.with(|slot| *slot.borrow_mut() = Some(Channels { updates, requests }));
    ns::spawn(async move {
        while let Some(next) = incoming.recv().await {
            state.set(next);
        }
    });
    pixels::render(state, sender)
}

struct Node {
    parent: Option<NodeId>,
    frame: Option<[f32; 4]>,
    style: Option<nsh_core::ViewStyle>,
    props: nsh_core::ViewProps,
    press: bool,
    drawing: Option<Arc<Drawing>>,
}

impl Node {
    fn new() -> Self {
        Self {
            parent: None,
            frame: None,
            style: None,
            props: nsh_core::ViewProps::default(),
            press: false,
            drawing: None,
        }
    }
}

struct Tree(BTreeMap<NodeId, Node>);

impl Tree {
    fn apply(&mut self, items: Vec<MountItem>) {
        for item in items {
            match item {
                MountItem::Create { id, .. } => {
                    assert!(self.0.insert(id, Node::new()).is_none());
                }
                MountItem::Insert { parent, child, .. } => {
                    assert!(parent == nsh_core::ROOT_ID || self.0.contains_key(&parent));
                    self.node_mut(child).parent = Some(parent);
                }
                MountItem::Remove { id } => self.node_mut(id).parent = None,
                MountItem::Delete { id } => {
                    self.0.remove(&id).expect("deleted mounted view");
                }
                MountItem::Frame { id, frame } => {
                    self.node_mut(id).frame = Some([
                        frame.origin.x.0,
                        frame.origin.y.0,
                        frame.size.width.0,
                        frame.size.height.0,
                    ]);
                }
                MountItem::ViewStyle { id, style } => self.node_mut(id).style = Some(*style),
                MountItem::ViewProps { id, props } => self.node_mut(id).props = *props,
                MountItem::AccessibilityLabel { id, label } => {
                    self.node_mut(id).props.accessibility.label = label;
                }
                MountItem::Events { id, events } => {
                    self.node_mut(id).press = events.contains(EventKind::Press);
                }
                MountItem::Canvas { id, drawing } => self.node_mut(id).drawing = Some(drawing),
                _ => {}
            }
        }
    }

    fn node_mut(&mut self, id: NodeId) -> &mut Node {
        self.0.get_mut(&id).expect("created view before update")
    }

    fn control(&self) -> (NodeId, &Node) {
        let controls: Vec<_> = self.0.iter().filter(|(_, node)| node.press).collect();
        assert_eq!(controls.len(), 1, "one real production Press listener");
        let (id, node) = controls[0];
        (*id, node)
    }

    fn assert_hidden_ancestor(&self, id: NodeId) {
        let mut cursor = id;
        let mut hidden = false;
        for _ in 0..self.0.len() {
            let node = self.0.get(&cursor).expect("mounted ancestor");
            assert_eq!(node.props.accessibility.role, AccessibilityRole::None);
            assert!(node.props.accessibility.actions.is_empty());
            if node.props.accessibility.hidden {
                assert_eq!(node.props.accessibility.accessible, Some(false));
                assert!(node.style.as_ref().expect("outer style").overflow_hidden);
                hidden = true;
            }
            let parent = node.parent.expect("attached production subtree");
            if parent == nsh_core::ROOT_ID {
                assert!(
                    hidden,
                    "production subtree must be hidden from duplicate AX"
                );
                return;
            }
            cursor = parent;
        }
        panic!("mounted ancestry did not reach synthetic root");
    }
}

struct Harness {
    core: Core<ApproxTextEngine>,
    channels: Channels,
    tree: Tree,
}

impl Harness {
    fn mount(snapshot: Snapshot, width: f32) -> Self {
        INITIAL.with(|slot| *slot.borrow_mut() = Some(snapshot));
        let mut core = Core::new(
            ApproxTextEngine,
            ns::size(ns::px(width), ns::px(Layout::READING_NAVIGATION_HEIGHT)),
        );
        core.mount_root(&Root::of::<Fixture>());
        let batch = core.commit().expect("production renderer mount");
        let channels = CHANNELS.with(|slot| slot.borrow_mut().take().expect("fixture channels"));
        let mut tree = Tree(BTreeMap::new());
        tree.apply(batch.items);
        let mut harness = Self {
            core,
            channels,
            tree,
        };
        harness.settle();
        harness
    }

    fn settle(&mut self) {
        self.core.run_component_tasks();
        for _ in 0..16 {
            let batch = self.core.commit();
            let mut events = Vec::new();
            self.core.take_events(&mut events);
            if batch.is_none() && events.is_empty() {
                return;
            }
            if let Some(batch) = batch {
                self.tree.apply(batch.items);
            }
            for event in events {
                match event {
                    JsEvent::Event(id, kind, payload) => {
                        self.core.component_event(id, kind, &payload);
                    }
                    JsEvent::LayoutListener {
                        host,
                        id,
                        listener,
                        frame,
                    } => self.core.component_layout(host, id, &listener, frame),
                    _ => {}
                }
            }
            self.core.run_component_tasks();
        }
        panic!("production resume-tail renderer did not settle");
    }

    fn drain(&mut self) -> Vec<Request> {
        let mut requests = Vec::new();
        for _ in 0..32 {
            self.core.run_component_tasks();
            match self.channels.requests.try_recv() {
                Ok(request) => requests.push(request),
                Err(mpsc::error::TryRecvError::Empty) => return requests,
                Err(mpsc::error::TryRecvError::Disconnected) => panic!("live renderer channel"),
            }
        }
        panic!("renderer command queue did not drain");
    }

    fn project(&mut self, snapshot: Snapshot) {
        self.channels
            .updates
            .try_send(snapshot)
            .expect("projection");
        self.settle();
    }

    fn press(&mut self, id: NodeId) {
        assert!(
            self.core
                .component_event(id, EventKind::Press, &EventPayload::None)
        );
        self.core.run_component_tasks();
    }
}

fn snapshot(dark: bool, language: &'static str) -> Snapshot {
    let theme = if dark { Theme::dark() } else { Theme::light() };
    Snapshot {
        id: ProjectionId::new(),
        focused: false,
        hovered: false,
        pressed: false,
        label: if language == "zh-CN" {
            "回到底部"
        } else {
            "Back to bottom"
        }
        .into(),
        colors: theme.colors,
        appearance: theme.appearance,
        language,
    }
}

fn assert_observed(actual: &Snapshot, expected: &Snapshot) {
    assert!(actual.id.matches(&expected.id));
    assert_eq!(actual.focused, expected.focused);
    assert_eq!(actual.hovered, expected.hovered);
    assert_eq!(actual.pressed, expected.pressed);
    assert_eq!(actual.label, expected.label);
    assert_eq!(actual.language, expected.language);
    assert_eq!(actual.appearance, expected.appearance);
}

fn assert_layout(requests: &[Request], expected: &Snapshot, width: f32) {
    let layouts: Vec<_> = requests
        .iter()
        .map(|request| match request {
            Request::Layout(observed, root, control) => (observed, root, control),
            Request::Activate(_) => panic!("layout alone must not activate business"),
        })
        .collect();
    assert!(
        !layouts.is_empty(),
        "actual layout callbacks must send acknowledgement"
    );
    for (observed, root, control) in &layouts {
        assert_observed(observed, expected);
        assert!(
            root.iter()
                .chain(control.iter())
                .all(|value| value.is_finite())
        );
        assert!(root[2] > 0. && root[3] > 0.);
        assert_eq!(control[2], Layout::SCROLL_TO_BOTTOM_SIZE);
        assert_eq!(control[3], Layout::SCROLL_TO_BOTTOM_SIZE);
    }
    let (_, root, control) = layouts.last().expect("settled layout acknowledgement");
    assert_eq!(**root, [0., 0., width, Layout::READING_NAVIGATION_HEIGHT]);
    assert_eq!(
        **control,
        [
            (width - Layout::SCROLL_TO_BOTTOM_SIZE) / 2.,
            Layout::READING_NAVIGATION_PADDING,
            Layout::SCROLL_TO_BOTTOM_SIZE,
            Layout::SCROLL_TO_BOTTOM_SIZE,
        ]
    );
}

fn assert_control_style(node: &Node, snapshot: &Snapshot) {
    let style = node.style.as_ref().expect("production control style");
    assert_eq!(style.border_radius.0, Layout::SCROLL_TO_BOTTOM_SIZE / 2.);
    let (width, border) = style.borders.uniform().expect("uniform circular border");
    assert_eq!(width.0, 1.);
    assert_eq!(
        border,
        color(if snapshot.focused {
            snapshot.colors.accent
        } else {
            snapshot.colors.border_subtle
        })
        .into()
    );
    assert_eq!(
        style.background_color,
        color(if snapshot.pressed {
            snapshot.colors.bg_active
        } else if snapshot.hovered {
            snapshot.colors.bg_hover
        } else {
            snapshot.colors.bg_elevated
        })
        .into()
    );
}

fn assert_arrow(node: &Node, snapshot: &Snapshot) {
    assert_eq!(
        node.frame.expect("icon frame"),
        [8., 8., Layout::ICON_SIZE, Layout::ICON_SIZE]
    );
    let drawing = node.drawing.as_ref().expect("actual SVG Canvas drawing");
    let paths: Vec<_> = drawing
        .ops
        .iter()
        .filter_map(|op| match op {
            CanvasOp::Stroke(Some(path)) => Some(path),
            _ => None,
        })
        .collect();
    assert_eq!(paths.len(), 1);
    assert_eq!(
        paths[0].as_ref(),
        &[
            PathOp::MoveTo(Num::Const(12.), Num::Const(5.)),
            PathOp::LineTo(Num::Const(12.), Num::Const(19.)),
            PathOp::MoveTo(Num::Const(5.), Num::Const(12.)),
            PathOp::LineTo(Num::Const(12.), Num::Const(19.)),
            PathOp::LineTo(Num::Const(19.), Num::Const(12.)),
        ]
    );
    assert!(drawing.ops.contains(&CanvasOp::LineWidth(Num::Const(2.))));
    assert!(drawing.ops.contains(&CanvasOp::LineCap(LineCap::Round)));
    assert!(drawing.ops.contains(&CanvasOp::LineJoin(LineJoin::Round)));
    assert!(drawing.ops.contains(&CanvasOp::StrokeStyle(Paint::Color(
        color(snapshot.colors.text_secondary).into()
    ))));
    let factor = Layout::ICON_SIZE / 24.;
    assert!(drawing.ops.contains(&CanvasOp::Transform(
        [factor, 0., 0., factor, 0., 0.].map(Num::Const)
    )));
}

#[test]
fn production_pixels_draw_token_sized_arrow_under_hidden_ax_ancestor_and_report_layout() {
    for dark in [false, true] {
        for language in ["en", "zh-CN"] {
            let shown = snapshot(dark, language);
            let mut harness = Harness::mount(shown.clone(), 320.);
            let (id, node) = harness.tree.control();
            harness.tree.assert_hidden_ancestor(id);
            assert_control_style(node, &shown);
            assert_eq!(node.frame, Some([144., 4., 32., 32.]));
            let icons: Vec<_> = harness
                .tree
                .0
                .iter()
                .filter(|(_, node)| node.drawing.is_some())
                .collect();
            assert_eq!(icons.len(), 1, "one actual visible ArrowDown drawing");
            let (icon_id, icon) = icons[0];
            assert_eq!(icon.parent, Some(id));
            harness.tree.assert_hidden_ancestor(*icon_id);
            assert_arrow(icon, &shown);
            assert_layout(&harness.drain(), &shown, 320.);
            harness.core.set_viewport(ns::size(
                ns::px(160.),
                ns::px(Layout::READING_NAVIGATION_HEIGHT),
            ));
            harness.settle();
            assert_eq!(harness.tree.control().1.frame, Some([64., 4., 32., 32.]));
            assert_layout(&harness.drain(), &shown, 160.);
        }
    }
}

#[test]
fn production_projection_updates_focus_hover_pressed_theme_and_localized_press_observation() {
    let initial = snapshot(false, "en");
    let mut harness = Harness::mount(initial, 320.);
    let _ = harness.drain();
    for (focused, hovered, pressed) in [
        (true, false, false),
        (false, true, false),
        (true, true, true),
        (false, false, false),
    ] {
        let mut next = snapshot(true, "zh-CN");
        next.focused = focused;
        next.hovered = hovered;
        next.pressed = pressed;
        harness.project(next.clone());
        assert_layout(&harness.drain(), &next, 320.);
        let (id, node) = harness.tree.control();
        assert_control_style(node, &next);
        harness.tree.assert_hidden_ancestor(id);
        harness.press(id);
        let requests = harness.drain();
        let [Request::Activate(observed)] = requests.as_slice() else {
            panic!("one production activation after layout acknowledgement");
        };
        assert_observed(observed, &next);
    }
}

#[test]
fn disposed_dynamic_listener_cannot_emit_or_refresh_to_replacement_projection() {
    let shown = snapshot(false, "en");
    let mut harness = Harness::mount(shown, 320.);
    let _ = harness.drain();
    let old = harness.tree.control().0;
    let replacement = snapshot(true, "zh-CN");
    harness
        .channels
        .updates
        .try_send(replacement.clone())
        .expect("replacement");
    harness.core.run_component_tasks();
    harness
        .core
        .component_event(old, EventKind::Press, &EventPayload::None);
    assert!(
        harness
            .drain()
            .iter()
            .all(|request| matches!(request, Request::Layout(..)))
    );
    harness.settle();
    let _ = harness.drain();
    let current = harness.tree.control().0;
    harness.press(current);
    let requests = harness.drain();
    let [Request::Activate(observed)] = requests.as_slice() else {
        panic!("current listener still works after old owner disposal");
    };
    assert_observed(observed, &replacement);
}

#[test]
fn pending_ordered_press_survives_dynamic_owner_disposal_without_adopting_new_snapshot() {
    let shown = snapshot(false, "en");
    let mut harness = Harness::mount(shown.clone(), 320.);
    let _ = harness.drain();
    let old = harness.tree.control().0;
    harness.press(old);
    assert_eq!(harness.channels.requests.len(), 1);
    harness.press(old);
    assert_eq!(
        harness.channels.requests.len(),
        1,
        "second send must face real backpressure"
    );
    let replacement = snapshot(true, "zh-CN");
    harness.project(replacement.clone());
    let requests = harness.drain();
    assert!(
        requests.len() >= 3,
        "two old presses followed by replacement layout"
    );
    for request in &requests[..2] {
        let Request::Activate(observed) = request else {
            panic!("ordered old activation must precede new layout");
        };
        assert_observed(observed, &shown);
        assert!(!observed.id.matches(&replacement.id));
    }
    assert_layout(&requests[2..], &replacement, 320.);
    let current = harness.tree.control().0;
    harness.press(current);
    let requests = harness.drain();
    let [Request::Activate(observed)] = requests.as_slice() else {
        panic!("original renderer owner remains usable");
    };
    assert_observed(observed, &replacement);
}
