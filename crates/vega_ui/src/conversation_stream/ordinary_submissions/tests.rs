use super::{OrdinaryAction, OrdinaryStatus};
use vega_conversation::types::{RetryBlocker, RetryEligibility, SubmissionFailure};

#[test]
fn revoked_submission_never_offers_retry_or_hides_its_reason_behind_status_results() {
    let failed = OrdinaryStatus::Failed(
        SubmissionFailure::PreparationFailed,
        RetryEligibility::Uncommitted,
    );
    let original = failed
        .controls(RetryEligibility::Uncommitted, None)
        .expect("original retry");
    assert!(matches!(
        original.action,
        Some(("ui.retry", OrdinaryAction::Retry))
    ));
    for checked in [
        None,
        Some("ui.ordinary_send.check_absent"),
        Some("ui.ordinary_send.check_matching"),
    ] {
        let revoked = failed
            .controls(
                RetryEligibility::Blocked(RetryBlocker::PermissionInvalidated),
                checked,
            )
            .expect("revoked send reason");
        assert_eq!(revoked.message, "ui.ordinary_send.permission_invalidated");
        assert!(revoked.action.is_none());
        assert!(revoked.detail.is_none());
    }
}

#[test]
fn revoked_unknown_send_preserves_uncertainty_and_history_check_without_reauthorizing_retry() {
    for checked in [
        None,
        Some("ui.ordinary_send.check_absent"),
        Some("ui.ordinary_send.check_matching"),
    ] {
        let controls = OrdinaryStatus::Unknown
            .controls(
                RetryEligibility::Blocked(RetryBlocker::PermissionInvalidated),
                checked,
            )
            .expect("unknown revoked send");
        assert_eq!(
            controls.message,
            "ui.ordinary_send.permission_invalidated_unknown"
        );
        assert_eq!(controls.detail, checked);
        assert!(matches!(
            controls.action,
            Some(("ui.ordinary_send.check_status", OrdinaryAction::CheckStatus))
        ));
    }
    for state in [OrdinaryStatus::Pending, OrdinaryStatus::Committed] {
        assert!(
            state
                .controls(
                    RetryEligibility::Blocked(RetryBlocker::PermissionInvalidated),
                    None
                )
                .is_none()
        );
    }
}
