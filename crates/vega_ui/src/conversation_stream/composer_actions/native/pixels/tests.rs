use super::super::{Kind, Request, Snapshot};
use super::{render, reveal_destination, visible};
use nsh_core::element::{Component, NoEvent, Root, Scope};
use nsh_core::prelude as ns;
use nsh_core::prelude::px;
use nsh_core::{AccessibilityRole, ApproxTextEngine, Core, MountItem};
use nsh_core::{Bounds, Measurement, Point, Size};
use std::cell::RefCell;
use tokio::sync::mpsc;

fn measurement(top: f32, height: f32) -> Measurement {
    Measurement {
        frame: Bounds {
            origin: Point {
                x: px(0.),
                y: px(0.),
            },
            size: Size {
                width: px(200.),
                height: px(height),
            },
        },
        page: Point {
            x: px(0.),
            y: px(top),
        },
    }
}

#[test]
fn clipped_action_requires_reveal_before_activation() {
    let viewport = measurement(20., 100.);
    assert!(visible(measurement(20., 32.), viewport));
    assert!(visible(measurement(88., 32.), viewport));
    assert!(!visible(measurement(19., 32.), viewport));
    assert!(!visible(measurement(89., 32.), viewport));
}

#[test]
fn oversized_action_is_available_only_at_viewport_start() {
    let viewport = measurement(20., 100.);
    assert!(visible(measurement(20., 140.), viewport));
    assert!(!visible(measurement(21., 140.), viewport));
}

#[test]
fn missing_and_nonfinite_geometry_cannot_enable_activation() {
    let viewport = measurement(20., 100.);
    assert!(!visible(measurement(f32::NAN, 32.), viewport));
    assert!(!visible(measurement(20., 0.), viewport));
    assert!(!visible(measurement(20., 32.), measurement(20., 0.)));
    assert!(!visible(measurement(20., f32::INFINITY), viewport));
}

#[test]
fn oversized_reveal_aligns_top_directly_and_settles_after_clamping() {
    let viewport = measurement(20., 100.);
    assert_eq!(
        reveal_destination(measurement(70., 240.), viewport, 0.),
        Some(50.)
    );
    assert_eq!(
        reveal_destination(measurement(20., 240.), viewport, 50.),
        None
    );
    assert_eq!(
        reveal_destination(measurement(-20., 240.), viewport, 30.),
        Some(0.)
    );
    assert_eq!(
        reveal_destination(measurement(19.75, 240.), viewport, 0.),
        None
    );
    assert!(!visible(measurement(19.75, 240.), viewport));
    assert_eq!(
        reveal_destination(measurement(f32::NAN, 240.), viewport, 0.),
        None
    );
}

thread_local! {
    static INITIAL: RefCell<Option<Snapshot>> = const { RefCell::new(None) };
    static REQUESTS: RefCell<Option<mpsc::Receiver<Request>>> = const { RefCell::new(None) };
}

struct ComposerFixture;

impl Component for ComposerFixture {
    type Props = ();
    type Event = NoEvent;

    fn view(_: (), _: Scope<NoEvent>) -> impl ns::IntoElement {
        let snapshot = INITIAL.with(|slot| slot.borrow_mut().take().expect("composer snapshot"));
        let (sender, requests) = mpsc::channel(crate::nsh::COMMAND_CAPACITY);
        REQUESTS.with(|slot| *slot.borrow_mut() = Some(requests));
        render(ns::signal_with(snapshot, ns::SignalEquals::Never), sender)
    }
}

#[test]
fn production_popup_mount_keeps_skill_state_reason_and_input_highlight_without_activation() {
    let mut snapshot = super::super::tests::snapshot();
    snapshot.highlight = 11;
    snapshot.rows[11].description = Some("Skill description on a long selected row".into());
    snapshot.rows[11].source = Some("Project skill".into());
    snapshot.rows[11].checked = Some(true);
    snapshot.rows[10].reason = Some("ui.command_busy");
    snapshot.rows[10].enabled = false;
    let disabled_name = snapshot.rows[10].accessible.clone();
    INITIAL.with(|slot| *slot.borrow_mut() = Some(snapshot));
    let mut core = Core::new(ApproxTextEngine, ns::size(ns::px(220.), ns::px(120.)));
    core.mount_root(&Root::of::<ComposerFixture>());
    let batch = core.commit().expect("production composer popup");
    let text = batch
        .items
        .iter()
        .filter_map(|item| match item {
            MountItem::Text { text, .. } => Some(text.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(text.contains(&"Skill description on a long selected row"));
    assert!(text.contains(&"Project skill"));
    assert!(text.contains(&vega_i18n::t("ui.command_busy")));
    assert!(batch.items.iter().any(|item| matches!(item,
        MountItem::ViewProps { props, .. }
            if props.accessibility.role == AccessibilityRole::Checkbox
                && props.accessibility.label.as_deref() == Some(disabled_name.as_str())
                && props.accessibility.state.disabled)));
    core.run_component_tasks();
    let mut requests = REQUESTS.with(|slot| slot.borrow_mut().take().expect("composer requests"));
    while let Ok(request) = requests.try_recv() {
        assert!(
            !matches!(request, Request::Activate(..)),
            "mount must not execute highlight"
        );
    }
}

#[test]
fn production_stop_mount_remains_enabled_without_assistant_permission() {
    let mut snapshot = super::super::tests::snapshot();
    snapshot.kind = Kind::Stop;
    snapshot.source.state[6] = true;
    snapshot.rows.truncate(1);
    snapshot.rows[0].label = "Stop".into();
    snapshot.rows[0].accessible = "Stop".into();
    snapshot.rows[0].checked = None;
    INITIAL.with(|slot| *slot.borrow_mut() = Some(snapshot));
    let mut core = Core::new(ApproxTextEngine, ns::size(ns::px(32.), ns::px(32.)));
    core.mount_root(&Root::of::<ComposerFixture>());
    let batch = core.commit().expect("production stop chrome");
    assert!(batch.items.iter().any(|item| matches!(item,
        MountItem::ViewProps { props, .. }
            if props.accessibility.role == AccessibilityRole::Button
                && props.accessibility.label.as_deref() == Some("Stop")
                && !props.accessibility.state.disabled)));
}
