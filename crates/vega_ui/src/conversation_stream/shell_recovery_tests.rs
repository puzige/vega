use super::RecoveryDecision;
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
    let mut changed = thread.clone();
    changed.permission_revision += 1;
    assert!(!request.matches_route(&changed, true, 3));
    changed = thread.clone();
    changed.permission_mode = vega_conversation::types::PermissionMode::FullAccess;
    assert!(!request.matches_route(&changed, true, 3));
}

#[test]
fn closing_pending_prepare_rejects_late_confirmation_after_permission_change() {
    let thread = draft_thread(Some("project-a"), "model", "confirm");
    let request = ShellRecoveryRequested {
        source: thread.clone(),
        sequence: 3,
        draft: true,
        action: ShellRecoveryAction::Prepare(vega_conversation::types::ShellRecoveryProjection {
            workspace: "/workspace".into(),
            epoch: vega_store::shell_operations::ShellRecoveryVersion {
                epoch: 1,
                scopes: Vec::new(),
            },
            rows: Vec::new(),
            active: Vec::new(),
            commands: Vec::new(),
        }),
    };
    let mut decision: RecoveryDecision<()> = RecoveryDecision::default();
    let mut sequence = 3;
    let mut pending = Some("ui.shell_recovery.checking");
    decision.dismiss(&mut sequence, &mut pending);
    assert!(pending.is_none());
    assert!(decision.confirmation.is_none());
    assert!(!request.matches_route(&thread, true, sequence));
    let mut changed = thread;
    changed.permission_revision += 1;
    assert!(!request.matches_route(&changed, true, sequence));
}

#[test]
fn closing_submitted_resume_does_not_claim_to_cancel_it() {
    let mut decision = RecoveryDecision {
        confirmation: Some(Box::new(())),
        submitted: true,
    };
    let mut sequence = 3;
    let mut pending = Some("ui.shell_recovery.resuming");
    decision.dismiss(&mut sequence, &mut pending);
    assert_eq!(sequence, 3);
    assert!(pending.is_some());
    assert!(decision.confirmation.is_some());
}
