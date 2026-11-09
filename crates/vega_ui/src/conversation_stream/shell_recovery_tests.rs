use super::{
    RecoveryPresentation, ShellRecoveryAction, ShellRecoveryRequested, recovery_presentation,
};
use vega_conversation::threads::draft_thread;

#[test]
fn clean_new_conversation_has_no_recovery_content() {
    assert_eq!(
        recovery_presentation(true, false, false),
        RecoveryPresentation::Hidden
    );
}

#[test]
fn blocked_new_conversation_gets_only_actionable_summary() {
    assert_eq!(
        recovery_presentation(true, true, false),
        RecoveryPresentation::Summary("ui.shell_recovery.paused")
    );
}

#[test]
fn failed_new_conversation_check_is_not_hidden_or_shown_as_full_panel() {
    assert_eq!(
        recovery_presentation(true, false, true),
        RecoveryPresentation::Summary("ui.shell_recovery.status_unavailable")
    );
    assert_eq!(
        recovery_presentation(true, true, true),
        RecoveryPresentation::Summary("ui.shell_recovery.status_unavailable")
    );
}

#[test]
fn persisted_conversation_keeps_existing_detail_presentation() {
    assert_eq!(
        recovery_presentation(false, true, false),
        RecoveryPresentation::Details
    );
    assert_eq!(
        recovery_presentation(false, false, true),
        RecoveryPresentation::Details
    );
}

#[test]
fn recovery_receipt_requires_current_project_draft_and_sequence() {
    let thread = draft_thread(Some("project-a"), "model", "confirm");
    let request = ShellRecoveryRequested {
        source: thread.clone(),
        sequence: 3,
        draft: true,
        action: ShellRecoveryAction::Read,
    };
    assert!(request.matches_route(&thread, true, 3));
    assert!(!request.matches_route(&thread, false, 3));
    assert!(!request.matches_route(&thread, true, 4));
    let mut rebound = thread.clone();
    rebound.project_id = "project-b".into();
    assert!(!request.matches_route(&rebound, true, 3));
    assert!(request.matches_route(&thread, true, 3));
    let other = draft_thread(Some("project-a"), "model", "confirm");
    assert!(!request.matches_route(&other, true, 3));
}
