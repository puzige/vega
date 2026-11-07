use super::{
    model::{Observation, Request, ResultPhase, Snapshot},
    view,
};
use nsh_core::element::{Component, NoEvent, Root, Scope};
use nsh_core::prelude as ns;
use nsh_core::{AccessibilityRole, ApproxTextEngine, Core, EventKind, EventPayload, MountItem};
use std::{cell::RefCell, time::Instant};
use tokio::sync::mpsc;
use vega_theme::Appearance;
use vega_ui::nsh::ProjectionId;

fn failure() -> Snapshot {
    Snapshot {
        revision: ProjectionId::new(),
        observed: Observation {
            thread_id: Some("loading-thread".into()),
            generation: 17,
            started: Some(Instant::now()),
            worker_started: true,
            result: ResultPhase::Failed,
            visit_started: None,
            opening: None,
        },
        colors: vega_theme::LIGHT,
        appearance: Appearance::Light,
        language: vega_i18n::language(),
        focused: false,
        show_logo: false,
        phase: None,
    }
}

thread_local! {
    static INPUT: RefCell<Option<Snapshot>> = const { RefCell::new(None) };
    static OUTPUT: RefCell<Option<mpsc::Receiver<Request>>> = const { RefCell::new(None) };
    static UPDATES: RefCell<Option<mpsc::Sender<Snapshot>>> = const { RefCell::new(None) };
}

struct LoadingFixture;

impl Component for LoadingFixture {
    type Props = ();
    type Event = NoEvent;

    fn view(_: (), _: Scope<NoEvent>) -> impl ns::IntoElement {
        let snapshot = INPUT.with(|slot| slot.borrow_mut().take().expect("fixture projection"));
        let (sender, receiver) = mpsc::channel(64);
        OUTPUT.with(|slot| *slot.borrow_mut() = Some(receiver));
        let state = ns::signal_with(snapshot, ns::SignalEquals::Never);
        let (updates, mut receiver) = mpsc::channel(1);
        UPDATES.with(|slot| *slot.borrow_mut() = Some(updates));
        ns::spawn(async move {
            while let Some(snapshot) = receiver.recv().await {
                state.set(snapshot);
            }
        });
        view::render(state, sender)
    }
}

#[test]
fn production_retry_retains_the_displayed_loading_attempt() {
    let shown = failure();
    INPUT.with(|slot| *slot.borrow_mut() = Some(shown.clone()));
    let mut core = Core::new(ApproxTextEngine, ns::size(ns::px(480.), ns::px(360.)));
    core.mount_root(&Root::of::<LoadingFixture>());
    let batch = core.commit().expect("loading mount");
    let button = batch
        .items
        .iter()
        .find_map(|item| match item {
            MountItem::ViewProps { id, props }
                if props.accessibility.role == AccessibilityRole::Button =>
            {
                assert_eq!(
                    props.accessibility.label.as_deref(),
                    Some(vega_i18n::t("ui.retry"))
                );
                Some(*id)
            }
            _ => None,
        })
        .expect("actual retry button");
    let mut unpublished = shown.clone();
    unpublished.observed.thread_id = Some("another-thread".into());
    unpublished.observed.generation += 1;
    unpublished.revision = ProjectionId::new();
    UPDATES.with(|slot| {
        slot.borrow()
            .as_ref()
            .expect("mounted updates")
            .try_send(unpublished.clone())
            .expect("competing projection");
    });
    assert!(core.component_event(button, EventKind::Press, &EventPayload::None));
    core.run_component_tasks();
    let mut receiver = OUTPUT.with(|slot| slot.borrow_mut().take().expect("commands"));
    let request = std::iter::from_fn(|| receiver.try_recv().ok())
        .find_map(|request| match request {
            Request::Retry(snapshot) => Some(snapshot),
            _ => None,
        })
        .expect("production retry callback");
    assert!(request.revision.matches(&shown.revision));
    assert!(request.observed == shown.observed);
    assert!(!request.same_frame(&unpublished));
    let _ = core.commit();
    assert!(core.component_event(button, EventKind::Press, &EventPayload::None));
    core.run_component_tasks();
    let replacement = std::iter::from_fn(|| receiver.try_recv().ok())
        .find_map(|request| match request {
            Request::Retry(snapshot) => Some(snapshot),
            _ => None,
        })
        .expect("reconciled retry callback");
    assert!(replacement.same_frame(&unpublished));
    assert!(replacement.observed == unpublished.observed);
}

#[test]
fn pending_and_sidebar_visit_loading_have_no_retry_action() {
    let mut pending = failure();
    pending.observed.result = ResultPhase::Pending;
    let mut visiting = failure();
    visiting.observed.visit_started = Some(Instant::now());
    for snapshot in [pending, visiting] {
        INPUT.with(|slot| *slot.borrow_mut() = Some(snapshot));
        let mut core = Core::new(ApproxTextEngine, ns::size(ns::px(160.), ns::px(240.)));
        core.mount_root(&Root::of::<LoadingFixture>());
        let batch = core.commit().expect("loading mount");
        assert!(!batch.items.iter().any(|item| matches!(item,
            MountItem::ViewProps { props, .. } if props.accessibility.role == AccessibilityRole::Button
        )));
        OUTPUT.with(|slot| *slot.borrow_mut() = None);
    }
}

#[test]
fn loading_frame_identity_rejects_authority_changes_but_not_shimmer_phase() {
    let displayed = failure();
    for change in 0..10 {
        let mut current = displayed.clone();
        match change {
            0 => current.revision = ProjectionId::new(),
            1 => current.observed.thread_id = Some("replacement".into()),
            2 => current.observed.generation += 1,
            3 => current.observed.result = ResultPhase::Ready,
            4 => current.observed.visit_started = Some(Instant::now()),
            5 => current.observed.worker_started = false,
            6 => current.focused = true,
            7 => current.show_logo = true,
            8 => current.language = "changed-language",
            _ => current.appearance = Appearance::Dark,
        }
        assert!(!current.same_frame(&displayed));
    }
    let mut animation = displayed.clone();
    animation.phase = Some(0.5);
    assert!(animation.same_frame(&displayed));
}
