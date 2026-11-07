use super::{Action, Request, RunFailureKind, RunNotice, RunNoticeKind, Snapshot, Source, pixels};
use crate::conversation_stream::model::StreamEntryIdentity;
use crate::nsh::ProjectionId;
use nsh_core::element::{Component, NoEvent, Root, Scope};
use nsh_core::prelude as ns;
use nsh_core::{
    AccessibilityRole, ApproxTextEngine, Core, EventKind, EventPayload, MountBatch, MountItem,
    NodeId,
};
use std::cell::RefCell;
use tokio::sync::mpsc;

fn snapshot(kind: RunNoticeKind, enabled: bool) -> Snapshot {
    let notice = RunNotice {
        message_id: "displayed-message".into(),
        kind,
    };
    Snapshot {
        id: ProjectionId::new(),
        text: notice.text(),
        source: Source {
            epoch: ProjectionId::new(),
            thread_id: "displayed-thread".into(),
            index: 7,
            identity: StreamEntryIdentity {
                key: "displayed-entry".into(),
                message_id: Some(notice.message_id.clone()),
                pending_id: None,
                echo_kind: None,
                sequence: Some(41),
            },
            notice,
            reading_preview: false,
        },
        colors: vega_theme::LIGHT,
        enabled,
        selected: None,
    }
}

struct Harness {
    updates: mpsc::Sender<Snapshot>,
    applied: mpsc::Receiver<Snapshot>,
    requests: mpsc::Receiver<Request>,
}

thread_local! {
    static INITIAL: RefCell<Option<Snapshot>> = const { RefCell::new(None) };
    static HARNESS: RefCell<Option<Harness>> = const { RefCell::new(None) };
}

struct NoticeFixture;

impl Component for NoticeFixture {
    type Props = ();
    type Event = NoEvent;

    fn view(_: (), _: Scope<NoEvent>) -> impl ns::IntoElement {
        let initial = INITIAL.with(|slot| slot.borrow_mut().take().expect("notice fixture"));
        let state = ns::signal_with(initial, ns::SignalEquals::Never);
        let (sender, requests) = mpsc::channel(crate::nsh::COMMAND_CAPACITY);
        let (updates, mut incoming) = mpsc::channel::<Snapshot>(1);
        let (acknowledge, applied) = mpsc::channel(1);
        HARNESS.with(|slot| {
            *slot.borrow_mut() = Some(Harness {
                updates,
                applied,
                requests,
            });
        });
        ns::spawn(async move {
            while let Some(next) = incoming.recv().await {
                state.set(next.clone());
                acknowledge.send(next).await.expect("projection applied");
            }
        });
        pixels::render(state, sender)
    }
}

fn mount(snapshot: Snapshot, width: f32) -> (Core<ApproxTextEngine>, MountBatch, Harness) {
    INITIAL.with(|slot| *slot.borrow_mut() = Some(snapshot));
    let mut core = Core::new(ApproxTextEngine, ns::size(ns::px(width), ns::px(600.)));
    core.mount_root(&Root::of::<NoticeFixture>());
    let batch = core.commit().expect("production run notice mount");
    let harness = HARNESS.with(|slot| slot.borrow_mut().take().expect("mounted notice harness"));
    (core, batch, harness)
}

fn visible_text(batch: &MountBatch) -> Vec<String> {
    batch
        .items
        .iter()
        .filter_map(|item| match item {
            MountItem::Text { text, .. } => Some(text.text.clone()),
            _ => None,
        })
        .collect()
}

fn buttons(batch: &MountBatch) -> Vec<(NodeId, String)> {
    batch
        .items
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
                        .expect("action AX name")
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

fn assert_observed(actual: &Snapshot, expected: &Snapshot) {
    assert!(actual.id.matches(&expected.id));
    assert!(actual.source.epoch.matches(&expected.source.epoch));
    assert_eq!(actual.source.thread_id, expected.source.thread_id);
    assert_eq!(
        actual.source.notice.message_id,
        expected.source.notice.message_id
    );
    assert_eq!(actual.source.index, expected.source.index);
    assert_eq!(actual.source.identity, expected.source.identity);
    assert_eq!(actual.source.notice.kind, expected.source.notice.kind);
    assert_eq!(
        actual.source.reading_preview,
        expected.source.reading_preview
    );
    assert_eq!(actual.text, expected.text);
    assert_eq!(actual.enabled, expected.enabled);
    assert!(actual.selected == expected.selected);
    assert!(actual.source.matches(&expected.source));
}

#[test]
fn production_mount_preserves_notice_text_and_credential_action_names() {
    let cases = [
        (
            RunNoticeKind::Retrying {
                attempt: 2,
                max_retries: 5,
            },
            Vec::new(),
        ),
        (RunNoticeKind::McpUnavailable(3), Vec::new()),
        (RunNoticeKind::ContextFailure, Vec::new()),
        (RunNoticeKind::CompactionFailure(None), Vec::new()),
        (
            RunNoticeKind::Failure(RunFailureKind::CredentialStorageUnavailable),
            vec!["ui.continue", "ui.check_credential_settings"],
        ),
        (
            RunNoticeKind::FailureWithCompaction(RunFailureKind::CredentialExposureBlocked, None),
            vec!["ui.continue", "ui.check_credential_settings"],
        ),
        (
            RunNoticeKind::Failure(RunFailureKind::ConfigurationRepairRequired),
            vec!["ui.continue"],
        ),
    ];
    for (kind, action_keys) in cases {
        for (width, colors) in [(180., vega_theme::LIGHT), (640., vega_theme::DARK)] {
            for enabled in [true, false] {
                let mut shown = snapshot(kind.clone(), enabled);
                shown.colors = colors;
                let source_text = shown.source.notice.text();
                let (mut core, batch, mut harness) = mount(shown, width);
                let labels = action_keys
                    .iter()
                    .filter(|_| enabled)
                    .map(|key| vega_i18n::t(key).to_owned())
                    .collect::<Vec<_>>();
                let mut expected = vec![source_text];
                expected.extend(labels.iter().cloned());
                assert_eq!(visible_text(&batch), expected, "mounted {kind:?}");
                let controls = buttons(&batch);
                assert_eq!(
                    controls
                        .iter()
                        .map(|(_, label)| label.clone())
                        .collect::<Vec<_>>(),
                    labels,
                    "accessible actions for {kind:?}"
                );
                for (id, _) in controls {
                    assert!(batch.items.iter().any(|item| {
                        matches!(item, MountItem::Frame { id: node, frame }
                            if *node == id && frame.size.width.0 > 0. && frame.size.height.0 > 0.)
                    }));
                }
                if labels.is_empty() {
                    assert!(
                        batch.items.iter().all(|item| {
                            !matches!(item, MountItem::Events { events, .. }
                            if events.contains(EventKind::Press))
                        }),
                        "noninteractive {kind:?} exposed a Press handler"
                    );
                    core.run_component_tasks();
                    assert!(activations(&mut harness.requests).is_empty());
                }
            }
        }
    }
}

type Replacement = (&'static str, fn(&mut Snapshot));

#[test]
fn production_press_keeps_displayed_identity_when_replacement_is_pending_or_applied() {
    let replacements: &[Replacement] = &[
        ("projection", |value| value.id = ProjectionId::new()),
        ("epoch", |value| value.source.epoch = ProjectionId::new()),
        ("thread", |value| {
            value.source.thread_id = "replacement-thread".into()
        }),
        ("message", |value| {
            value.source.notice.message_id = "replacement-message".into()
        }),
        ("row", |value| value.source.index += 1),
        ("entry", |value| {
            value.source.identity.key = "replacement-entry".into()
        }),
        ("notice", |value| {
            value.source.notice.kind = RunNoticeKind::FailureWithCompaction(
                RunFailureKind::CredentialStorageUnavailable,
                None,
            );
            value.text = value.source.notice.text();
        }),
        ("preview", |value| {
            value.source.reading_preview = true;
            value.enabled = false;
        }),
    ];
    for (name, replace) in replacements {
        for apply_before_press in [false, true] {
            let shown = snapshot(
                RunNoticeKind::Failure(RunFailureKind::CredentialStorageUnavailable),
                true,
            );
            let (mut core, batch, mut harness) = mount(shown.clone(), 480.);
            let controls = buttons(&batch);
            let mut replacement = shown.clone();
            replace(&mut replacement);
            harness
                .updates
                .try_send(replacement.clone())
                .expect("replacement delivery");
            if apply_before_press {
                core.run_component_tasks();
                let applied = harness.applied.try_recv().expect("replacement applied");
                assert_observed(&applied, &replacement);
            }
            for (id, _) in &controls {
                assert!(core.component_event(*id, EventKind::Press, &EventPayload::None));
            }
            core.run_component_tasks();
            if !apply_before_press {
                let applied = harness
                    .applied
                    .try_recv()
                    .expect("pending replacement applied");
                assert_observed(&applied, &replacement);
            }
            let requests = activations(&mut harness.requests);
            if apply_before_press {
                assert!(
                    requests.is_empty(),
                    "disposed {name} listeners emitted actions"
                );
                let mut current_controls = controls.clone();
                if let Some(reconciled) = core.commit() {
                    for item in &reconciled.items {
                        match item {
                            MountItem::Remove { id } | MountItem::Delete { id } => {
                                current_controls.retain(|(node, _)| node != id);
                            }
                            MountItem::ViewProps { id, props } => {
                                current_controls.retain(|(node, _)| node != id);
                                if props.accessibility.role == AccessibilityRole::Button {
                                    current_controls.push((
                                        *id,
                                        props
                                            .accessibility
                                            .label
                                            .as_deref()
                                            .expect("current action AX name")
                                            .to_owned(),
                                    ));
                                }
                            }
                            _ => {}
                        }
                    }
                }
                assert_eq!(current_controls.len(), replacement.actions().len());
                let expected_labels = replacement
                    .actions()
                    .into_iter()
                    .map(|action| action.label().to_owned())
                    .collect::<Vec<_>>();
                assert_eq!(
                    current_controls
                        .iter()
                        .map(|(_, label)| label.clone())
                        .collect::<Vec<_>>(),
                    expected_labels
                );
                for (id, _) in current_controls {
                    assert!(core.component_event(id, EventKind::Press, &EventPayload::None));
                }
                core.run_component_tasks();
                let current = activations(&mut harness.requests);
                assert_eq!(current.len(), replacement.actions().len());
                for ((observed, action), expected) in current.iter().zip(replacement.actions()) {
                    assert!(*action == expected);
                    assert_observed(observed, &replacement);
                }
                continue;
            }
            assert_eq!(
                requests.len(),
                2,
                "both pending displayed actions for {name}"
            );
            for ((observed, action), expected_action) in
                requests.iter().zip([Action::Continue, Action::Settings])
            {
                assert!(*action == expected_action);
                assert_observed(observed, &shown);
                assert!(
                    !observed.id.matches(&replacement.id)
                        || !observed.source.matches(&replacement.source),
                    "old {name} action borrowed replacement authority"
                );
            }
        }
    }
}

#[test]
fn reconciled_retry_removes_credential_callbacks_without_replaying_them() {
    let shown = snapshot(
        RunNoticeKind::Failure(RunFailureKind::CredentialStorageUnavailable),
        true,
    );
    let (mut core, batch, mut harness) = mount(shown, 480.);
    let old_controls = buttons(&batch);
    let retry = snapshot(
        RunNoticeKind::Retrying {
            attempt: 3,
            max_retries: 5,
        },
        true,
    );
    harness
        .updates
        .try_send(retry.clone())
        .expect("retry projection delivery");
    core.run_component_tasks();
    let applied = harness
        .applied
        .try_recv()
        .expect("retry projection applied");
    assert_observed(&applied, &retry);
    let replaced = core.commit().expect("retry reconciliation");
    assert!(replaced.items.iter().any(|item| {
        matches!(item, MountItem::Text { text, .. } if text.text == retry.source.notice.text())
    }));
    assert!(buttons(&replaced).is_empty());
    for (id, _) in old_controls {
        core.component_event(id, EventKind::Press, &EventPayload::None);
    }
    core.run_component_tasks();
    assert!(activations(&mut harness.requests).is_empty());
}
