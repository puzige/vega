use super::*;

#[test]
fn poisoned_configuration_requires_restart_without_retry_action() {
    use vega_conversation::submission_diagnostics::{
        PreparationFailure, PreparationReason, PreparationStage,
    };
    let mut detail = PreparationFailure::new(
        PreparationStage::Launch,
        PreparationReason::CredentialSafety,
    );
    detail.credential_reason = Some("configuration_poisoned");
    let controls = OrdinaryStatus::Failed(
        SubmissionFailure::Preparation(detail),
        RetryEligibility::Uncommitted,
    )
    .controls(RetryEligibility::Uncommitted, None)
    .unwrap();
    assert_eq!(controls.message, "domain.failure.configuration_restart");
    assert!(controls.action.is_none());
    detail.credential_reason = Some("configuration_parse");
    let controls = OrdinaryStatus::Failed(
        SubmissionFailure::Preparation(detail),
        RetryEligibility::Uncommitted,
    )
    .controls(RetryEligibility::Uncommitted, None)
    .unwrap();
    assert_eq!(controls.message, "domain.failure.configuration_repair");
    assert!(matches!(controls.action, Some((_, OrdinaryAction::Retry))));
    let blocked = OrdinaryStatus::Failed(
        SubmissionFailure::Preparation(detail),
        RetryEligibility::Uncommitted,
    )
    .controls(
        RetryEligibility::Blocked(RetryBlocker::PermissionInvalidated),
        None,
    )
    .unwrap();
    assert_eq!(blocked.message, "ui.ordinary_send.permission_invalidated");
    assert!(blocked.action.is_none());
}

#[test]
fn run_notice_resumes_only_repairable_configuration_failures() {
    use super::super::run_notice::{RunNotice, RunNoticeKind};
    for (failure, resumable) in [
        (RunFailureKind::ConfigurationRestartRequired, false),
        (RunFailureKind::ConfigurationRepairRequired, true),
        (RunFailureKind::ConfigurationAccessUnavailable, true),
    ] {
        for kind in [
            RunNoticeKind::Failure(failure),
            RunNoticeKind::FailureWithCompaction(failure, None),
        ] {
            let notice = RunNotice {
                message_id: "synthetic".into(),
                kind,
            };
            assert_eq!(notice.can_resume_credentials(), resumable);
            assert!(!notice.text().is_empty());
        }
    }
}
