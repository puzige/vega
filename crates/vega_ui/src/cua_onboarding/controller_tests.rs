use super::{CardAction, CardController, CardEffect, CheckState};
use std::path::Path;
use vega_conversation::types::{
    ComponentState, ComputerUseError, DirectCapture, ExtensionState, PermissionState, Preparation,
};

#[test]
fn recheck_deduplicates_pending_work_and_ignores_cancelled_tickets() {
    let mut controller = CardController::new("original-task", Path::new("/tmp/vega"));
    let effects = controller.act(CardAction::Recheck);
    let [
        CardEffect::Inspect {
            ticket: stale,
            prompt: false,
            probe_direct_capture: false,
        },
    ] = effects.as_slice()
    else {
        panic!("recheck must request one non-prompting, non-capturing inspection");
    };
    let stale = stale.clone();
    assert!(!controller.view().can_recheck);
    assert!(controller.act(CardAction::Recheck).is_empty());

    controller.cancel_inspection();
    assert!(controller.view().can_recheck);
    let effects = controller.act(CardAction::Recheck);
    let [
        CardEffect::Inspect {
            ticket: current,
            prompt: false,
            probe_direct_capture: false,
        },
    ] = effects.as_slice()
    else {
        panic!("cancelled inspection must permit one fresh read-only inspection");
    };
    assert_ne!(&stale, current);
    assert!(
        controller
            .complete(stale, Err(ComputerUseError::Unavailable))
            .is_empty()
    );
    assert_eq!(controller.view().check_state, CheckState::Checking);
    assert!(!controller.view().can_recheck);

    assert!(
        controller
            .complete(
                current.clone(),
                Ok(Preparation {
                    component: ComponentState::Available,
                    accessibility: PermissionState::Granted,
                    screen_recording: PermissionState::Granted,
                    direct_capture: DirectCapture::NotChecked,
                    browser_extension: ExtensionState::NotEvaluated,
                }),
            )
            .is_empty()
    );
    let view = controller.view();
    assert_eq!(view.check_state, CheckState::Observed);
    assert_eq!(view.accessibility, PermissionState::Granted);
    assert_eq!(view.screen_recording, PermissionState::Granted);
    assert_eq!(view.direct_capture, DirectCapture::NotChecked);
    assert!(!view.desktop_ready);
    assert!(view.open);
    assert!(view.can_recheck);
}

#[test]
fn returning_to_task_cancels_before_closing_and_ignores_late_results() {
    let mut controller = CardController::new("original-task", Path::new("/tmp/vega"));
    let effects = controller.act(CardAction::Recheck);
    let [CardEffect::Inspect { ticket, .. }] = effects.as_slice() else {
        panic!("recheck must create one inspection ticket");
    };
    assert_eq!(
        controller.act(CardAction::ReturnToTask),
        vec![
            CardEffect::CancelPending,
            CardEffect::CloseWindow,
            CardEffect::ReturnToTask("original-task".into()),
        ]
    );
    let closed = controller.view();
    assert!(!closed.open);
    assert!(!closed.following);
    assert!(!closed.can_recheck);
    assert!(
        controller
            .complete(ticket.clone(), Err(ComputerUseError::Unavailable))
            .is_empty()
    );
    assert_eq!(controller.view(), closed);
    assert!(controller.act(CardAction::Recheck).is_empty());
}
