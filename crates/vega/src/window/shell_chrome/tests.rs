use super::{Action, Control, Kind, Observation, ProjectionId, Snapshot};
use super::{Request, view};
use nsh_core::element::{Component, NoEvent, Root, Scope};
use nsh_core::prelude as ns;
use nsh_core::{AccessibilityRole, ApproxTextEngine, Core, EventKind, EventPayload, MountItem};
use std::cell::RefCell;
use tokio::sync::mpsc;
use vega_conversation::types::UpdatePhase;

fn snapshot() -> Snapshot {
    Snapshot {
        revision: ProjectionId::new(),
        observation: Observation {
            kind: Kind::Header,
            thread: Some("task".into()),
            project: Some("project".into()),
            phase: UpdatePhase::Available,
            version: Some("next".into()),
            dismissed: false,
            settings: false,
            blocked: false,
            mutation: Default::default(),
            title: "Task".into(),
            controls: vec![Control {
                action: Action::Update,
                label: "Update".into(),
                enabled: true,
                selected: false,
            }],
        },
        focused: None,
        colors: vega_theme::Theme::light().colors,
        appearance: vega_theme::Appearance::Light,
        language: "en",
    }
}

thread_local! {
    static INPUT: RefCell<Option<Snapshot>> = const { RefCell::new(None) };
    static OUTPUT: RefCell<Option<mpsc::Receiver<Request>>> = const { RefCell::new(None) };
    static UPDATES: RefCell<Option<mpsc::Sender<Snapshot>>> = const { RefCell::new(None) };
}
struct Fixture;
impl Component for Fixture {
    type Props = ();
    type Event = NoEvent;
    fn view(_: (), _: Scope<NoEvent>) -> impl ns::IntoElement {
        let initial = INPUT.with(|slot| slot.borrow_mut().take().expect("fixture projection"));
        let (sender, receiver) = mpsc::channel(64);
        OUTPUT.with(|slot| *slot.borrow_mut() = Some(receiver));
        let state = ns::signal_with(initial, ns::SignalEquals::Never);
        let (updates, mut receiver) = mpsc::channel(1);
        UPDATES.with(|slot| *slot.borrow_mut() = Some(updates));
        ns::spawn(async move {
            while let Some(next) = receiver.recv().await {
                state.set(next);
            }
        });
        view::render(state, sender)
    }
}

#[test]
fn production_update_callback_retains_displayed_route_before_reconciliation() {
    let shown = snapshot();
    INPUT.with(|slot| *slot.borrow_mut() = Some(shown.clone()));
    let mut core = Core::new(ApproxTextEngine, ns::size(ns::px(640.), ns::px(48.)));
    core.mount_root(&Root::of::<Fixture>());
    let batch = core.commit().expect("production header mount");
    let button = batch
        .items
        .iter()
        .find_map(|item| match item {
            MountItem::ViewProps { id, props }
                if props.accessibility.role == AccessibilityRole::Button =>
            {
                assert_eq!(props.accessibility.label.as_deref(), Some("Update"));
                Some(*id)
            }
            _ => None,
        })
        .expect("actual update control");
    let mirror = batch
        .items
        .iter()
        .find_map(|item| match item {
            MountItem::Insert { parent, child, .. } if *child == button => Some(*parent),
            _ => None,
        })
        .expect("mirror accessibility container");
    assert!(batch.items.iter().any(|item| matches!(item,
        MountItem::ViewProps { id, props } if *id == mirror && props.accessibility.hidden && props.accessibility.accessible == Some(false))));
    let mut replacement = shown.clone();
    replacement.revision = ProjectionId::new();
    replacement.observation.thread = Some("replacement".into());
    replacement.observation.phase = UpdatePhase::Ready;
    UPDATES.with(|slot| {
        slot.borrow()
            .as_ref()
            .expect("updates")
            .try_send(replacement.clone())
            .expect("publish replacement")
    });
    assert!(core.component_event(button, EventKind::Press, &EventPayload::None));
    core.run_component_tasks();
    let mut receiver = OUTPUT.with(|slot| slot.borrow_mut().take().expect("commands"));
    let old = std::iter::from_fn(|| receiver.try_recv().ok())
        .filter_map(|request| match request {
            Request::Activate(snapshot, action) => Some((snapshot, action)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(old.len(), 1);
    assert_eq!(old[0].1, Action::Update);
    assert!(old[0].0.matches(&shown));
    assert!(!old[0].0.matches(&replacement));
    let _ = core.commit();
    assert!(core.component_event(button, EventKind::Press, &EventPayload::None));
    core.run_component_tasks();
    let current = std::iter::from_fn(|| receiver.try_recv().ok())
        .filter_map(|request| match request {
            Request::Activate(snapshot, action) => Some((snapshot, action)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(current.len(), 1);
    assert_eq!(current[0].1, Action::Update);
    assert!(current[0].0.matches(&replacement));
}

#[test]
fn displayed_authority_does_not_follow_route_version_or_visibility_changes() {
    let displayed = snapshot();
    assert!(displayed.matches(&displayed));
    assert!(displayed.allows(Action::Update));
    let mut current = displayed.clone();
    current.observation.thread = Some("other".into());
    assert!(!current.matches(&displayed));
    current = displayed.clone();
    current.observation.project = None;
    assert!(!current.matches(&displayed));
    current = displayed.clone();
    current.observation.version = Some("different".into());
    assert!(!current.matches(&displayed));
    current = displayed.clone();
    current.observation.phase = UpdatePhase::Ready;
    assert!(!current.matches(&displayed));
    current = displayed.clone();
    current.observation.dismissed = true;
    assert!(!current.matches(&displayed));
    current = displayed.clone();
    current.observation.settings = true;
    assert!(!current.matches(&displayed));
    assert!(!current.allows(Action::Update));
    current = displayed.clone();
    current.observation.blocked = true;
    assert!(!current.allows(Action::Update));
}

#[test]
fn remount_focus_and_availability_revoke_old_control_authority() {
    let displayed = snapshot();
    let mut current = displayed.clone();
    current.revision = ProjectionId::new();
    assert!(!current.matches(&displayed));
    current = displayed.clone();
    current.focused = Some(Action::Update);
    assert!(!current.matches(&displayed));
    current = displayed.clone();
    current.observation.controls[0].enabled = false;
    assert!(!current.matches(&displayed));
    assert!(!current.allows(Action::Update));
    current = displayed.clone();
    current.observation.controls.clear();
    assert!(!current.allows(Action::Update));
    assert!(!displayed.allows(Action::Terminal));
    current = displayed.clone();
    current.language = "zh-CN";
    assert!(!current.matches(&displayed));
    current = displayed.clone();
    current.appearance = vega_theme::Appearance::Dark;
    assert!(!current.matches(&displayed));
}
