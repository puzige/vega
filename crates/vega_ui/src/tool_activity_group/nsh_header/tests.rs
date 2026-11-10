use super::{Snapshot, render};
use crate::icons::Icon;
use crate::nsh::ProjectionId;
use gpui_kit::EntityId;
use nsh_core::prelude as ns;
use nsh_core::{AccessibilityRole, ApproxTextEngine, Core, MountItem};
use tokio::sync::mpsc;
use vega_theme::{Layout, Theme};

thread_local! {
    static HEADER_STATE: std::cell::RefCell<Option<mpsc::Sender<Snapshot>>> = const { std::cell::RefCell::new(None) };
}

fn snapshot() -> Snapshot {
    let colors = Theme::light().colors;
    Snapshot {
        id: ProjectionId::new(),
        children: vec![EntityId::from(1_u64), EntityId::from(2_u64)],
        expanded: false,
        summary: "Read example.rs, searched example".into(),
        label: "Read example.rs, searched example, expand".into(),
        icon: Icon::Wrench,
        tint: colors.text_secondary,
        colors,
        keyboard_focus: false,
    }
}

#[test]
fn disclosure_rejects_replaced_reordered_and_removed_children() {
    let observed = snapshot();
    assert!(observed.accepts(&observed));
    let mut current = observed.clone();
    current.children.reverse();
    assert!(!current.accepts(&observed));
    current = observed.clone();
    current.children[0] = EntityId::from(3_u64);
    assert!(!current.accepts(&observed));
    current = observed.clone();
    current.children.pop();
    assert!(!current.accepts(&observed));
}

#[test]
fn disclosure_rejects_superseded_expansion_and_headline() {
    let observed = snapshot();
    let mut current = observed.clone();
    current.expanded = true;
    assert!(!current.accepts(&observed));
    current = observed.clone();
    current.summary = "Read different.rs".into();
    assert!(!current.accepts(&observed));
    current = observed.clone();
    current.icon = Icon::ChevronDown;
    assert!(!current.accepts(&observed));
    current = observed.clone();
    current.tint = current.colors.accent;
    assert!(!current.accepts(&observed));
}

#[test]
fn recycled_mount_and_restored_content_cannot_replay_old_disclosure() {
    let observed = snapshot();
    let mut current = observed.clone();
    current.id = ProjectionId::new();
    assert!(current.same_content(&observed));
    assert!(!current.accepts(&observed));
    assert!(current.accepts(&current));
    let mut focused = observed.clone();
    focused.keyboard_focus = true;
    assert!(focused.accepts(&observed));
}

#[test]
fn static_header_presentation_converges_but_focus_theme_and_recycle_still_repaint() {
    let initial = snapshot();
    for _ in 0..64 {
        assert!(initial.same_presentation(&initial.clone()));
    }
    let mut current = initial.clone();
    current.keyboard_focus = true;
    assert!(!current.same_presentation(&initial));
    assert!(current.accepts(&initial));
    current = initial.clone();
    current.colors.bg_hover = current.colors.accent;
    assert!(!current.same_presentation(&initial));
    current = initial.clone();
    current.id = ProjectionId::new();
    assert!(!current.same_presentation(&initial));
    assert!(!current.accepts(&initial));
    current = initial.clone();
    current.children.reverse();
    assert!(!current.same_presentation(&initial));
}

#[ns::component]
fn HeaderFixture() -> impl ns::IntoElement {
    let (commands, _) = mpsc::channel(crate::nsh::COMMAND_CAPACITY);
    let state = ns::signal_with(snapshot(), ns::SignalEquals::Never);
    let (sender, mut updates) = mpsc::channel(1);
    HEADER_STATE.with(|slot| *slot.borrow_mut() = Some(sender));
    ns::spawn(async move {
        while let Some(next) = updates.recv().await {
            state.set(next);
        }
    });
    render(state, commands)
}

#[test]
fn production_header_preserves_height_and_button_state_in_narrow_and_wide_layouts() {
    for width in [110_f32, 640_f32] {
        let mut core = Core::new(
            ApproxTextEngine,
            ns::size(ns::px(width), ns::px(Layout::CONVERSATION_TOOL_HEIGHT)),
        );
        core.mount_root(&nsh_core::element::Root::of::<HeaderFixture>());
        let batch = core.commit().expect("header mount batch");
        let (id, props) = batch
            .items
            .iter()
            .find_map(|item| match item {
                MountItem::ViewProps { id, props }
                    if props.accessibility.role == AccessibilityRole::Button =>
                {
                    Some((*id, props))
                }
                _ => None,
            })
            .expect("production NSH disclosure button");
        assert_eq!(props.accessibility.state.expanded, Some(false));
        assert_eq!(
            props.accessibility.label.as_deref(),
            Some("Read example.rs, searched example, expand")
        );
        let frame = batch
            .items
            .iter()
            .find_map(|item| match item {
                MountItem::Frame { id: target, frame } if *target == id => Some(frame),
                _ => None,
            })
            .expect("disclosure frame");
        assert!((frame.size.height.0 - Layout::CONVERSATION_TOOL_HEIGHT).abs() < 0.01);
        assert!((frame.size.width.0 - width).abs() < 0.01);
        HEADER_STATE.with(|slot| {
            let sender = slot.borrow();
            let sender = sender.as_ref().expect("mounted header projection channel");
            let mut next = snapshot();
            next.id = ProjectionId::new();
            next.expanded = true;
            next.label = "Read example.rs, searched example, collapse".into();
            sender.try_send(next).expect("header projection delivery");
        });
        core.run_component_tasks();
        let updated = core.commit().expect("expanded header projection");
        assert!(updated.items.iter().any(|item| {
            matches!(item, MountItem::ViewProps { id: target, props }
                if *target == id
                    && props.accessibility.state.expanded == Some(true)
                    && props.accessibility.label.as_deref() == Some("Read example.rs, searched example, collapse"))
        }));
        assert!(
            !updated
                .items
                .iter()
                .any(|item| { matches!(item, MountItem::Delete { id: target } if *target == id) })
        );
        HEADER_STATE.with(|slot| *slot.borrow_mut() = None);
    }
}
