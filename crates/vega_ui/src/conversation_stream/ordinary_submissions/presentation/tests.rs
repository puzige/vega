use super::*;
use core::prelude::v1::test;
use nsh_core::element::{JsEvent, Root};
use nsh_core::prelude as ns;
use nsh_core::{
    AccessibilityRole, ApproxTextEngine, Core, EventKind, EventPayload, MountItem, NodeId,
};
use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::mpsc;
use vega_conversation::types::{RawSubmission, RetryBlocker};

fn failed_submission() -> OrdinarySubmission {
    let store = vega_store::Store::open(":memory:").expect("in-memory submission store");
    let database = store.database_identity();
    let admission = vega_store::thread_admission::try_acquire(database, "displayed-thread")
        .expect("admission result")
        .expect("submission admission");
    let (submission, worker) = OrdinarySubmission::begin(
        database,
        "displayed-thread".into(),
        RawSubmission::new(
            "original message".into(),
            Vec::new(),
            Vec::new(),
            1,
            "model".into(),
            None,
            None,
        ),
        admission,
    )
    .expect("ordinary submission");
    let attempt = worker.attempt_id();
    worker
        .complete(Err(SubmissionFailure::SpawnFailed))
        .expect("failed preparation");
    submission.seal(attempt).expect("sealed attempt");
    submission
}

fn snapshot(
    submission: &OrdinarySubmission,
    state: OrdinaryStatus,
    retry: RetryEligibility,
    checked: Option<&'static str>,
    diagnostics: bool,
    width: f32,
) -> Snapshot {
    let facts = Facts::capture(&submission.snapshot()).expect("actual submission facts");
    let source = Source {
        epoch: ProjectionId::new(),
        thread: "displayed-thread".into(),
        index: 7,
        identity: StreamEntryIdentity {
            key: "displayed-entry".into(),
            message_id: Some(submission.user_message_id().into()),
            pending_id: None,
            echo_kind: None,
            sequence: Some(41),
        },
        submission: submission.id(),
        attempt: facts.latest,
        state,
        checked,
        controls: state.controls(retry, checked),
        diagnostics,
        facts,
    };
    let mut snapshot = Snapshot::content(
        source,
        true,
        ".SystemUIFont".into(),
        vega_theme::LIGHT,
        vega_theme::Appearance::Light,
    );
    snapshot.width = width;
    snapshot
}

struct Harness {
    updates: mpsc::Sender<Snapshot>,
    requests: mpsc::Receiver<Request>,
    queued: mpsc::Sender<Vec<RetainedPress>>,
    invoked: mpsc::Receiver<Vec<bool>>,
}

#[derive(Clone)]
struct RetainedPress {
    id: ProjectionId,
    owner: nsh_core::element::Owner,
    action: Action,
    callback: std::rc::Rc<dyn Fn()>,
}

thread_local! {
    static INITIAL: RefCell<Option<Snapshot>> = const { RefCell::new(None) };
    static HARNESS: RefCell<Option<Harness>> = const { RefCell::new(None) };
    static PRESSES: RefCell<Vec<RetainedPress>> = const { RefCell::new(Vec::new()) };
}

pub(super) fn retain_press(
    id: ProjectionId,
    owner: nsh_core::element::Owner,
    action: Action,
    callback: impl Fn() + 'static,
) -> impl Fn() {
    let callback: std::rc::Rc<dyn Fn()> = std::rc::Rc::new(callback);
    PRESSES.with(|slot| {
        slot.borrow_mut().push(RetainedPress {
            id,
            owner,
            action,
            callback: callback.clone(),
        });
    });
    move || callback()
}

#[ns::component]
fn ControlsFixture() -> impl ns::IntoElement {
    let initial = INITIAL.with(|slot| slot.borrow_mut().take().expect("ordinary snapshot"));
    let state = ns::signal_with(initial, ns::SignalEquals::Never);
    let (sender, requests) = mpsc::channel(crate::nsh::COMMAND_CAPACITY);
    let (updates, mut incoming) = mpsc::channel(1);
    let (queued, mut callbacks) = mpsc::channel::<Vec<RetainedPress>>(1);
    let (completed, invoked) = mpsc::channel(1);
    HARNESS.with(|slot| {
        *slot.borrow_mut() = Some(Harness {
            updates,
            requests,
            queued,
            invoked,
        });
    });
    ns::spawn(async move {
        while let Some(callbacks) = callbacks.recv().await {
            let mut live_owners = Vec::new();
            for saved in callbacks {
                live_owners.push(saved.owner.is_live());
                (saved.callback)();
            }
            completed
                .try_send(live_owners)
                .expect("retained invocation report");
        }
    });
    ns::spawn(async move {
        while let Some(next) = incoming.recv().await {
            state.set(next);
        }
    });
    pixels::render(state, sender)
}

fn mount(snapshot: Snapshot) -> (Core<ApproxTextEngine>, Vec<MountItem>, Harness) {
    let width = snapshot.width;
    PRESSES.with(|slot| slot.borrow_mut().clear());
    INITIAL.with(|slot| *slot.borrow_mut() = Some(snapshot));
    let mut core = Core::new(ApproxTextEngine, ns::size(ns::px(width), ns::px(36.)));
    core.mount_root(&Root::of::<ControlsFixture>());
    let items = settle(&mut core);
    let harness = HARNESS.with(|slot| slot.borrow_mut().take().expect("ordinary harness"));
    (core, items, harness)
}

fn settle(core: &mut Core<ApproxTextEngine>) -> Vec<MountItem> {
    let notified = Arc::new(AtomicBool::new(false));
    let task_notified = notified.clone();
    core.set_task_notify(Arc::new(move || {
        task_notified.store(true, Ordering::SeqCst);
    }));
    let mut items = Vec::new();
    for _ in 0..32 {
        notified.store(false, Ordering::SeqCst);
        core.run_component_tasks();
        let batch = core.commit();
        let mut events = Vec::new();
        core.take_events(&mut events);
        if batch.is_none()
            && events.is_empty()
            && !core.has_due_answers()
            && !notified.load(Ordering::SeqCst)
        {
            return items;
        }
        if let Some(batch) = batch {
            items.extend(batch.items);
        }
        for event in events {
            match event {
                JsEvent::Event(id, kind, payload) => {
                    core.component_event(id, kind, &payload);
                }
                JsEvent::LayoutListener {
                    host,
                    id,
                    listener,
                    frame,
                } => {
                    core.component_layout(host, id, &listener, frame);
                }
                _ => {}
            }
        }
    }
    panic!("ordinary renderer did not settle");
}

fn buttons(items: &[MountItem]) -> Vec<(NodeId, String)> {
    items
        .iter()
        .filter_map(|item| match item {
            MountItem::ViewProps { id, props }
                if props.accessibility.role == AccessibilityRole::Button =>
            {
                Some((
                    *id,
                    props
                        .accessibility
                        .label
                        .as_deref()
                        .expect("button name")
                        .to_owned(),
                ))
            }
            _ => None,
        })
        .collect()
}

fn activations(receiver: &mut mpsc::Receiver<Request>) -> Vec<(Box<Snapshot>, Action)> {
    let mut result = Vec::new();
    while let Ok(request) = receiver.try_recv() {
        if let Request::Activate { observed, action } = request {
            result.push((observed, action));
        }
    }
    result
}

#[test]
fn production_controls_keep_exceptional_status_detail_action_and_diagnostics_content() {
    let submission = failed_submission();
    let mut poisoned = vega_conversation::submission_diagnostics::PreparationFailure::new(
        vega_conversation::submission_diagnostics::PreparationStage::Preflight,
        vega_conversation::submission_diagnostics::PreparationReason::Unknown,
    );
    poisoned.credential_reason = Some("configuration_poisoned");
    let cases = [
        (
            OrdinaryStatus::Failed(
                SubmissionFailure::Preparation(poisoned),
                RetryEligibility::Uncommitted,
            ),
            RetryEligibility::Uncommitted,
            None,
            true,
            vec![
                "domain.failure.configuration_restart",
                "ui.ordinary_send.copy_diagnostics",
            ],
        ),
        (
            OrdinaryStatus::Failed(
                SubmissionFailure::SpawnFailed,
                RetryEligibility::Uncommitted,
            ),
            RetryEligibility::Uncommitted,
            None,
            true,
            vec![
                "ui.ordinary_send.spawn_failed",
                "ui.retry",
                "ui.ordinary_send.copy_diagnostics",
            ],
        ),
        (
            OrdinaryStatus::Unknown,
            RetryEligibility::Blocked(RetryBlocker::PermissionInvalidated),
            Some("ui.ordinary_send.history_changed"),
            true,
            vec![
                "ui.ordinary_send.permission_invalidated_unknown",
                "ui.ordinary_send.history_changed",
                "ui.ordinary_send.check_status",
                "ui.ordinary_send.copy_diagnostics",
            ],
        ),
        (
            OrdinaryStatus::Failed(
                SubmissionFailure::PreparationFailed,
                RetryEligibility::Blocked(RetryBlocker::PermissionInvalidated),
            ),
            RetryEligibility::Blocked(RetryBlocker::PermissionInvalidated),
            None,
            false,
            vec!["ui.ordinary_send.permission_invalidated"],
        ),
        (
            OrdinaryStatus::Pending,
            RetryEligibility::Uncommitted,
            None,
            true,
            vec!["ui.ordinary_send.copy_diagnostics"],
        ),
        (
            OrdinaryStatus::Unknown,
            RetryEligibility::Blocked(RetryBlocker::WorkersActive),
            Some("ui.ordinary_send.finishing"),
            false,
            vec![
                "ui.ordinary_send.finishing",
                "ui.ordinary_send.check_status",
            ],
        ),
    ];
    for (state, retry, checked, diagnostics, keys) in cases {
        for width in [220., 760.] {
            let shown = snapshot(&submission, state, retry, checked, diagnostics, width);
            let expected_buttons = shown
                .buttons
                .iter()
                .map(|(_, name)| name.clone())
                .collect::<Vec<_>>();
            let (_core, items, mut harness) = mount(shown);
            let text = items
                .iter()
                .filter_map(|item| match item {
                    MountItem::Text { text, .. } => Some(text.text.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(
                text,
                keys.iter()
                    .map(|key| vega_i18n::t(key).to_owned())
                    .collect::<Vec<_>>()
            );
            assert_eq!(
                buttons(&items)
                    .into_iter()
                    .map(|(_, name)| name)
                    .collect::<Vec<_>>(),
                expected_buttons
            );
            assert!(
                activations(&mut harness.requests).is_empty(),
                "mount must not dispatch"
            );
        }
    }
}

#[test]
fn production_press_retains_displayed_attempt_and_replaced_handlers_cannot_launder_authority() {
    let submission = failed_submission();
    let shown = snapshot(
        &submission,
        OrdinaryStatus::Unknown,
        RetryEligibility::Uncommitted,
        None,
        true,
        760.,
    );
    let (mut core, items, mut harness) = mount(shown.clone());
    let controls = buttons(&items);
    assert_eq!(controls.len(), 2);
    let original = PRESSES.with(|slot| slot.borrow().clone());
    assert_eq!(original.len(), controls.len());
    for (saved, (action, _)) in original.iter().zip(&shown.buttons) {
        assert!(saved.id.matches(&shown.id));
        assert_eq!(saved.action, *action);
    }
    assert!(core.component_event(controls[0].0, EventKind::Press, &EventPayload::None));
    let _ = settle(&mut core);
    let requests = activations(&mut harness.requests);
    assert_eq!(
        requests.len(),
        1,
        "press must finish its real measurement/command task; button={:?} geometry={:?} due_answers={}",
        controls[0],
        core.measure(controls[0].0),
        core.has_due_answers()
    );
    let (observed, action) = &requests[0];
    assert!(shown.same(observed));
    assert_eq!(observed.source.attempt, shown.source.attempt);
    assert_eq!(*action, Action::Submission(OrdinaryAction::CheckStatus));
    assert!(shown.accepts(observed, *action, true));
    assert!(!shown.accepts(observed, *action, false));
    assert!(core.component_event(controls[1].0, EventKind::Press, &EventPayload::None));
    let _ = settle(&mut core);
    let diagnostic_requests = activations(&mut harness.requests);
    assert_eq!(diagnostic_requests.len(), 1);
    let (diagnostic, diagnostic_action) = &diagnostic_requests[0];
    assert_eq!(*diagnostic_action, Action::Diagnostics);
    assert!(shown.same(diagnostic));
    assert_eq!(diagnostic.source.attempt, shown.source.attempt);
    assert!(shown.accepts(diagnostic, *diagnostic_action, true));
    let worker = submission
        .try_retry(shown.source.attempt)
        .expect("new real attempt");
    let mut replacement = shown.clone();
    replacement.id = ProjectionId::new();
    replacement.source.facts = Facts::capture(&submission.snapshot()).expect("new attempt facts");
    replacement.source.attempt = worker.attempt_id();
    assert!(!replacement.accepts(observed, *action, true));
    harness
        .updates
        .try_send(replacement.clone())
        .expect("replacement queued");
    assert!(core.component_event(controls[1].0, EventKind::Press, &EventPayload::None));
    let replacement_items = settle(&mut core);
    assert!(
        activations(&mut harness.requests)
            .iter()
            .all(|(observed, action)| { !replacement.accepts(observed, *action, true) })
    );
    let mut current = controls.clone();
    for item in &replacement_items {
        match item {
            MountItem::ViewProps { id, props } => {
                if props.accessibility.role == AccessibilityRole::Button {
                    let name = props
                        .accessibility
                        .label
                        .as_deref()
                        .expect("current button name");
                    if let Some((_, previous)) = current.iter_mut().find(|(node, _)| node == id) {
                        *previous = name.to_owned();
                    } else {
                        current.push((*id, name.to_owned()));
                    }
                } else {
                    current.retain(|(node, _)| node != id);
                }
            }
            MountItem::Delete { id } => current.retain(|(node, _)| node != id),
            _ => {}
        }
    }
    assert_eq!(current.len(), controls.len());
    for (id, _) in &current {
        assert!(
            core.measure(*id).is_some(),
            "replacement button must be mounted"
        );
    }
    let retained = PRESSES.with(|slot| slot.borrow().clone());
    let installed = retained
        .iter()
        .filter(|saved| saved.id.matches(&replacement.id))
        .collect::<Vec<_>>();
    assert_eq!(installed.len(), current.len());
    for (old, new) in original.iter().zip(&installed) {
        assert!(old.owner != new.owner, "replacement must own new listeners");
    }
    eprintln!(
        "ordinary press original_nodes={controls:?} replacement_nodes={current:?} original_callbacks={} replacement_callbacks={}",
        original.len(),
        installed.len()
    );
    for (id, _) in &controls {
        let routed = core.component_event(*id, EventKind::Press, &EventPayload::None);
        assert_eq!(routed, current.iter().any(|(current, _)| current == id));
    }
    let _ = settle(&mut core);
    let routed_requests = activations(&mut harness.requests);
    let reused = controls
        .iter()
        .filter(|(id, _)| current.iter().any(|(current, _)| current == id))
        .count();
    assert_eq!(routed_requests.len(), reused);
    for (observed, action) in &routed_requests {
        assert!(replacement.same(observed));
        assert_eq!(observed.source.attempt, worker.attempt_id());
        assert!(replacement.accepts(observed, *action, true));
    }
    harness
        .queued
        .try_send(original)
        .expect("retained callbacks queued");
    let _ = settle(&mut core);
    assert_eq!(
        harness
            .invoked
            .try_recv()
            .expect("all original callbacks actually returned"),
        vec![false; controls.len()],
        "original presentation owners must be disposed before invocation"
    );
    assert!(
        activations(&mut harness.requests).is_empty(),
        "disposed listeners cannot activate"
    );
    worker
        .complete(Err(SubmissionFailure::Cancelled))
        .expect("finish retry fixture");
}

#[test]
fn production_natural_height_wraps_long_detail_and_buttons_without_truncating_content() {
    let submission = failed_submission();
    let shown = snapshot(
        &submission,
        OrdinaryStatus::Unknown,
        RetryEligibility::Blocked(RetryBlocker::PermissionInvalidated),
        Some("ui.ordinary_send.history_changed"),
        true,
        180.,
    );
    let (core, items, mut harness) = mount(shown.clone());
    let mut heights = Vec::new();
    while let Ok(request) = harness.requests.try_recv() {
        if let Request::Layout { id, width, height } = request
            && id.matches(&shown.id)
            && width == shown.width
        {
            heights.push(height);
        }
    }
    assert!(
        heights
            .iter()
            .any(|height| *height > Typography::BODY * Typography::BODY_LINE_HEIGHT + 8.)
    );
    for (id, _) in buttons(&items) {
        let bounds = core.measure(id).expect("visible button geometry");
        assert!(bounds.frame.size.width.0 <= shown.width + 0.5);
        assert!(bounds.frame.size.height.0 > 0.);
    }
    for item in &items {
        if let MountItem::Text { text, .. } = item {
            assert_eq!(text.max_lines, 0);
            assert!(!text.no_wrap);
        }
    }
}

#[test]
fn production_acceptance_rejects_changed_route_entry_facts_presentation_and_eligibility() {
    let submission = failed_submission();
    let shown = snapshot(
        &submission,
        OrdinaryStatus::Unknown,
        RetryEligibility::Uncommitted,
        None,
        true,
        760.,
    );
    let action = Action::Diagnostics;
    let changes: &[fn(&mut Snapshot)] = &[
        |value| value.id = ProjectionId::new(),
        |value| value.source.epoch = ProjectionId::new(),
        |value| value.source.thread = "replacement-thread".into(),
        |value| value.source.index += 1,
        |value| value.source.identity.key = "replacement-entry".into(),
        |value| value.source.facts.workers += 1,
        |value| value.source.facts.sealed = !value.source.facts.sealed,
        |value| value.source.diagnostics = false,
        |value| value.buttons.clear(),
        |value| value.enabled = false,
        |value| value.width += 1.,
        |value| value.selected = Some(Action::Diagnostics),
    ];
    for change in changes {
        let mut replacement = shown.clone();
        change(&mut replacement);
        assert!(!replacement.accepts(&shown, action, true));
    }
}
