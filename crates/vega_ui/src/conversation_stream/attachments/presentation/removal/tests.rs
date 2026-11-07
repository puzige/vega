use super::{GroupMount, Snapshot, Source, accepts_group_mount, render};
use crate::nsh::ProjectionId;
use nsh_core::element::{Component, NoEvent, Root, Scope};
use nsh_core::prelude as ns;
use nsh_core::{AccessibilityRole, ApproxTextEngine, Core, EventKind, EventPayload, MountItem};
use std::cell::RefCell;
use std::rc::Rc;
use tokio::sync::{mpsc, watch};

thread_local! {
    static INITIAL: RefCell<Option<Snapshot>> = const { RefCell::new(None) };
    static REQUESTS: RefCell<Option<mpsc::Receiver<Snapshot>>> = const { RefCell::new(None) };
}

struct RemoveFixture;

impl Component for RemoveFixture {
    type Props = ();
    type Event = NoEvent;

    fn view(_: (), _: Scope<NoEvent>) -> impl ns::IntoElement {
        let snapshot = INITIAL.with(|slot| slot.borrow_mut().take().expect("remove snapshot"));
        let state = ns::signal_with(snapshot, ns::SignalEquals::Never);
        let (sender, requests) = mpsc::channel(crate::nsh::COMMAND_CAPACITY);
        REQUESTS.with(|slot| *slot.borrow_mut() = Some(requests));
        render(state, sender)
    }
}

fn shown() -> Snapshot {
    Snapshot {
        id: ProjectionId::new(),
        source: Source {
            epoch: ProjectionId::new(),
            thread: "displayed-thread".into(),
            generation: 41,
            attachment: 37,
            image: 23,
            bytes: 17,
            length: 11,
        },
        geometry: None,
        focused: false,
        colors: vega_theme::LIGHT,
    }
}

fn queued_remove(snapshot: &Snapshot) -> mpsc::Receiver<Snapshot> {
    INITIAL.with(|slot| *slot.borrow_mut() = Some(snapshot.clone()));
    let mut core = Core::new(ApproxTextEngine, ns::size(ns::px(24.), ns::px(24.)));
    core.mount_root(&Root::of::<RemoveFixture>());
    let batch = core.commit().expect("production remove render");
    let controls = batch
        .items
        .iter()
        .filter_map(|item| match item {
            MountItem::ViewProps { id, props }
                if props.accessibility.role == AccessibilityRole::Button =>
            {
                Some((*id, props))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(controls.len(), 1);
    assert_eq!(
        controls[0].1.accessibility.label.as_deref(),
        Some(vega_i18n::t_args("ui.remove_image_id", &[("id", "37")]).as_str())
    );
    assert!(core.component_event(controls[0].0, EventKind::Press, &EventPayload::None));
    core.run_component_tasks();
    REQUESTS.with(|slot| slot.borrow_mut().take().expect("remove requests"))
}

#[test]
fn mounted_remove_callback_carries_displayed_attachment_authority() {
    let snapshot = shown();
    let mut requests = queued_remove(&snapshot);
    let request = requests.try_recv().expect("rendered remove callback");
    assert!(request.id.matches(&snapshot.id));
    assert!(request.source.matches(&snapshot.source));
    assert!(request.geometry.is_none());
    assert!(requests.try_recv().is_err());
}

#[test]
fn queued_remove_rejects_group_failure_before_async_mount_revocation() {
    let snapshot = shown();
    let mount = Rc::new(GroupMount::default());
    mount.active.set(true);
    let parent = Rc::downgrade(&mount);
    let current = parent.clone();
    let (failures, errors) = watch::channel(None);
    let mut watcher = errors.clone();
    assert!(accepts_group_mount(&parent, &current, Some(errors.clone())));
    let mut requests = queued_remove(&snapshot);

    failures.send_replace(Some("group surface failed".to_owned()));
    assert!(watcher.has_changed().expect("pending group error"));
    assert!(mount.active.get());
    let request = requests.try_recv().expect("remove queued before failure");
    assert!(request.id.matches(&snapshot.id));
    assert!(request.source.matches(&snapshot.source));
    assert!(!accepts_group_mount(
        &parent,
        &current,
        Some(errors.clone())
    ));
    assert!(requests.try_recv().is_err());

    assert!(watcher.borrow_and_update().is_some());
    mount.active.set(false);
    assert!(!accepts_group_mount(&parent, &current, Some(errors)));
}

#[test]
fn group_dispatch_rejects_replaced_missing_inactive_and_released_mounts() {
    let old = Rc::new(GroupMount::default());
    old.active.set(true);
    let parent = Rc::downgrade(&old);
    let replacement = Rc::new(GroupMount::default());
    replacement.active.set(true);
    let current = Rc::downgrade(&replacement);
    let (_, errors) = watch::channel(None);
    assert!(accepts_group_mount(&parent, &parent, Some(errors.clone())));
    assert!(!accepts_group_mount(
        &parent,
        &current,
        Some(errors.clone())
    ));
    assert!(!accepts_group_mount(&current, &current, None));
    replacement.active.set(false);
    assert!(!accepts_group_mount(
        &current,
        &current,
        Some(errors.clone())
    ));
    replacement.active.set(true);
    drop(replacement);
    assert!(!accepts_group_mount(&current, &current, Some(errors)));
}

#[test]
fn attachment_authority_rejects_each_replaced_identity_dimension() {
    let snapshot = shown();
    let replacements: [fn(&mut Source); 8] = [
        |value| value.epoch = ProjectionId::new(),
        |value| value.thread = "replacement-thread".into(),
        |value| value.generation += 1,
        |value| value.attachment += 1,
        |value| value.image += 1,
        |value| value.bytes += 1,
        |value| value.length += 1,
        |value| {
            value.image += 1;
            value.attachment += 1;
        },
    ];
    for replace in replacements {
        let mut replacement = snapshot.source.clone();
        replace(&mut replacement);
        assert!(!snapshot.source.matches(&replacement));
    }
    assert!(snapshot.source.matches(&snapshot.source.clone()));
}
