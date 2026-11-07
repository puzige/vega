use super::{Action, ComputerUseControl, Request, Snapshot, Source, TaskContext, pixels};
use crate::nsh::ProjectionId;
use nsh_core::element::{Component, NoEvent, Root, Scope};
use nsh_core::prelude as ns;
use nsh_core::{
    AccessibilityRole, ApproxTextEngine, Core, EventKind, EventPayload, MountBatch, MountItem,
};
use std::cell::RefCell;
use tokio::sync::mpsc;
use vega_conversation::types::{Capability, ControlPhase, PermissionMode, Target};

fn source(phase: ControlPhase) -> Source {
    Source {
        presentation_epoch: ProjectionId::new(),
        control: ComputerUseControl::new(),
        task: TaskContext::new("displayed-thread"),
        thread: "displayed-thread".into(),
        phase,
        epoch: 41,
        capability: Some(Capability::Browser),
        target: Some(Target::Browser {
            connection: "displayed-connection".into(),
            index: 7,
            title: "Displayed tab".into(),
            url: "https://example.invalid/displayed".into(),
        }),
        target_label: "Displayed tab — https://example.invalid/displayed".into(),
        generation: 12,
        active_message: Some(("displayed-message".into(), 3)),
        permission: PermissionMode::Confirm,
        language: vega_i18n::language(),
    }
}

type SourceChange = (&'static str, fn(&mut Source));

#[test]
fn displayed_source_rejects_replacement_even_when_visible_target_is_unchanged() {
    let observed = source(ControlPhase::AwaitingApproval);
    assert!(observed.matches(&observed.clone()));
    let changes: &[SourceChange] = &[
        ("presentation epoch", |value| {
            value.presentation_epoch = ProjectionId::new();
        }),
        ("control instance", |value| {
            value.control = ComputerUseControl::new();
        }),
        ("task generation in the same thread", |value| {
            value.task = TaskContext::new("displayed-thread");
        }),
        ("thread", |value| value.thread = "replacement-thread".into()),
        ("phase", |value| value.phase = ControlPhase::Paused),
        ("runtime epoch", |value| value.epoch += 1),
        ("capability", |value| {
            value.capability = Some(Capability::Desktop);
        }),
        ("missing target", |value| value.target = None),
        ("browser connection", |value| {
            let Some(Target::Browser { connection, .. }) = &mut value.target else {
                panic!("browser fixture");
            };
            *connection = "replacement-connection".into();
        }),
        ("browser tab index", |value| {
            let Some(Target::Browser { index, .. }) = &mut value.target else {
                panic!("browser fixture");
            };
            *index += 1;
        }),
        ("browser title", |value| {
            let Some(Target::Browser { title, .. }) = &mut value.target else {
                panic!("browser fixture");
            };
            *title = "Replacement tab".into();
        }),
        ("browser URL", |value| {
            let Some(Target::Browser { url, .. }) = &mut value.target else {
                panic!("browser fixture");
            };
            *url = "https://example.invalid/replacement".into();
        }),
        ("target label", |value| {
            value.target_label = "Replacement target label".into();
        }),
        ("control generation", |value| value.generation += 1),
        ("active message identity", |value| {
            value.active_message = Some(("replacement-message".into(), 3));
        }),
        ("active message index", |value| {
            value.active_message = Some(("displayed-message".into(), 4));
        }),
        ("removed active message", |value| {
            value.active_message = None
        }),
        ("permission", |value| {
            value.permission = PermissionMode::ReadOnly;
        }),
        ("language", |value| {
            value.language = if value.language == "en" { "zh" } else { "en" };
        }),
    ];
    for (name, change) in changes {
        let mut current = observed.clone();
        change(&mut current);
        assert!(!observed.matches(&current), "stale {name} accepted");
        assert!(!current.matches(&observed), "replacement {name} accepted");
    }
    let mut restored = observed.clone();
    restored.presentation_epoch = ProjectionId::new();
    assert!(!observed.matches(&restored));
    assert!(restored.matches(&restored.clone()));
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
                acknowledge
                    .send(next)
                    .await
                    .expect("projection acknowledgement");
            }
        });
        pixels::render(state, sender)
    }
}

fn mount(snapshot: Snapshot, width: f32) -> (Core<ApproxTextEngine>, MountBatch, Harness) {
    INITIAL.with(|slot| *slot.borrow_mut() = Some(snapshot));
    let mut core = Core::new(ApproxTextEngine, ns::size(ns::px(width), ns::px(600.)));
    core.mount_root(&Root::of::<NoticeFixture>());
    let batch = core.commit().expect("production notice mount");
    let harness = HARNESS.with(|slot| slot.borrow_mut().take().expect("mounted notice harness"));
    (core, batch, harness)
}

fn activations(receiver: &mut mpsc::Receiver<Request>) -> Vec<(Box<Snapshot>, Action)> {
    let mut commands = Vec::new();
    while let Ok(request) = receiver.try_recv() {
        if let Request::Activate { observed, action } = request {
            commands.push((observed, action));
        }
    }
    commands
}

#[test]
fn production_mount_exposes_each_phase_text_action_and_disabled_state() {
    let cases = [
        (
            ControlPhase::Running,
            None,
            Some("ui.take_over_computer_control"),
        ),
        (
            ControlPhase::AwaitingApproval,
            Some("ui.confirm_that_this_task_may_access_this_browser"),
            Some("ui.allow_this_tab"),
        ),
        (
            ControlPhase::Paused,
            Some("ui.computer_control_is_paused_resume_this_task_when"),
            Some("ui.continue"),
        ),
        (ControlPhase::Pausing, Some("ui.returning_control"), None),
    ];
    for (phase, guidance, action_label) in cases {
        for (width, colors) in [(180., vega_theme::LIGHT), (640., vega_theme::DARK)] {
            for enabled in [true, false] {
                let shown = Snapshot::new(source(phase), colors, enabled);
                let (mut core, batch, mut harness) = mount(shown, width);
                let mut expected = guidance
                    .into_iter()
                    .chain(action_label)
                    .map(|key| vega_i18n::t(key).to_owned())
                    .collect::<Vec<_>>();
                if phase == ControlPhase::AwaitingApproval {
                    expected.push("Displayed tab — https://example.invalid/displayed".into());
                }
                let mut actual = batch
                    .items
                    .iter()
                    .filter_map(|item| match item {
                        MountItem::Text { text, .. } => Some(text.text.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                expected.sort();
                actual.sort();
                assert_eq!(actual, expected, "visible text for {phase:?}");
                let buttons = batch
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
                assert_eq!(buttons.len(), usize::from(action_label.is_some()));
                if let Some(key) = action_label {
                    let (button, props) = buttons[0];
                    assert_eq!(
                        props.accessibility.label.as_deref(),
                        Some(vega_i18n::t(key))
                    );
                    assert_eq!(props.accessibility.state.disabled, !enabled);
                    assert!(batch.items.iter().any(|item| {
                        matches!(item, MountItem::Frame { id, frame } if *id == button
                            && frame.size.width.0 > 0. && frame.size.height.0 > 0.)
                    }));
                    if !enabled {
                        assert!(core.component_event(
                            button,
                            EventKind::Press,
                            &EventPayload::None
                        ));
                        core.run_component_tasks();
                        assert!(activations(&mut harness.requests).is_empty());
                    }
                }
            }
        }
    }
}

fn assert_same_source(actual: &Source, expected: &Source) {
    assert!(
        actual
            .presentation_epoch
            .matches(&expected.presentation_epoch)
    );
    assert!(actual.control.same_instance(&expected.control));
    assert_eq!(actual.task, expected.task);
    assert_eq!(actual.thread, expected.thread);
    assert_eq!(actual.phase, expected.phase);
    assert_eq!(actual.epoch, expected.epoch);
    assert_eq!(actual.capability, expected.capability);
    assert!(actual.target == expected.target);
    assert_eq!(actual.target_label, expected.target_label);
    assert_eq!(actual.generation, expected.generation);
    assert_eq!(actual.active_message, expected.active_message);
    assert_eq!(actual.permission, expected.permission);
    assert_eq!(actual.language, expected.language);
}

#[test]
fn production_press_retains_displayed_authority_across_model_and_projection_replacement() {
    for (phase, expected_action) in [
        (ControlPhase::Running, Action::TakeOver),
        (ControlPhase::AwaitingApproval, Action::AllowBrowser),
        (ControlPhase::Paused, Action::Resume),
    ] {
        let shown = Snapshot::new(source(phase), vega_theme::LIGHT, true);
        let (mut core, batch, mut harness) = mount(shown.clone(), 480.);
        let button = batch
            .items
            .iter()
            .find_map(|item| match item {
                MountItem::Events { id, events } if events.contains(EventKind::Press) => Some(*id),
                _ => None,
            })
            .expect("production notice press callback");
        let mut current = Snapshot::new(source(ControlPhase::Pausing), vega_theme::DARK, false);
        current.source.thread = "replacement-thread".into();
        current.source.task = TaskContext::new("replacement-thread");
        current.source.epoch = 99;
        current.source.capability = Some(Capability::Desktop);
        current.source.target = Some(Target::Browser {
            connection: "replacement-connection".into(),
            index: 19,
            title: "Replacement tab".into(),
            url: "https://example.invalid/replacement".into(),
        });
        current.source.target_label = "Replacement target label".into();
        current.source.generation = 18;
        current.source.active_message = Some(("replacement-message".into(), 8));
        current.source.permission = PermissionMode::ReadOnly;
        current.source.language = if shown.source.language == "en" {
            "zh"
        } else {
            "en"
        };
        harness
            .updates
            .try_send(current.clone())
            .expect("new model projection delivery");
        assert!(core.component_event(button, EventKind::Press, &EventPayload::None));
        core.run_component_tasks();
        let applied = harness
            .applied
            .try_recv()
            .expect("projection applied inside engine");
        assert_same_source(&applied.source, &current.source);
        let replaced = core.commit().expect("replacement projection mount");
        let commands = activations(&mut harness.requests);
        assert_eq!(commands.len(), 1, "exactly one production activation");
        let (observed, action) = &commands[0];
        assert!(*action == expected_action);
        assert!(observed.id.matches(&shown.id));
        assert!(!observed.id.matches(&applied.id));
        assert_same_source(&observed.source, &shown.source);
        assert_eq!(observed.text, shown.text);
        assert_eq!(observed.target, shown.target);
        assert_eq!(observed.label, shown.label);
        assert_eq!(observed.enabled, shown.enabled);
        assert_eq!(observed.selected, shown.selected);
        assert!(!observed.source.matches(&applied.source));
        assert!(!applied.source.matches(&observed.source));
        assert!(replaced.items.iter().any(|item| {
            matches!(item, MountItem::Text { text, .. } if text.text == vega_i18n::t("ui.returning_control"))
        }));
        assert_eq!(
            shown.source.control.status().phase,
            ControlPhase::Disconnected
        );
        core.component_event(button, EventKind::Press, &EventPayload::None);
        core.run_component_tasks();
        assert!(
            activations(&mut harness.requests).is_empty(),
            "removed production callback replayed"
        );
    }
}
