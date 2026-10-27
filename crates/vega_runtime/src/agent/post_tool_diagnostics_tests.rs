use super::*;
use crate::{
    ContextEstimateError, ContextRuntimeError, RuntimeFailureDetail,
    RuntimeFailureReason as Reason, RuntimeFailureStage as Stage,
};

#[test]
fn safe_runtime_details_survive_outer_stage_and_hide_error_bodies() {
    let error = VegaError::Store(rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error::new(1811),
        Some("PRIVATE_SQL_CANARY".into()),
    ))
    .at_runtime_stage(Stage::PendingClaim)
    .at_runtime_stage(Stage::EventPipeline);
    let metrics = error.diagnostic_metrics();
    assert_eq!(metrics.runtime_stage, Some("pending_claim"));
    assert_eq!(metrics.runtime_reason, Some("database"));
    assert_eq!(metrics.failure_kind, Some("runtime_failure"));
    assert_eq!(metrics.sqlite_code, Some(19));
    assert_eq!(metrics.sqlite_extended_code, Some(1811));
    assert!(!format!("{error:?} {error}").contains("PRIVATE_SQL_CANARY"));
    let io = VegaError::Io(std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        "PRIVATE_PATH_CANARY",
    ))
    .at_runtime_stage(Stage::PendingReferences);
    assert_eq!(io.diagnostic_metrics().io_kind, Some("permission_denied"));
    assert!(!format!("{io:?} {io}").contains("PRIVATE_PATH_CANARY"));
    assert!(matches!(
        VegaError::Cancelled.at_runtime_stage(Stage::PendingRead),
        VegaError::Cancelled
    ));
}

#[test]
fn invalid_budget_and_arithmetic_have_distinct_safe_reasons() {
    for (failure, reason) in [
        (ContextEstimateError::InvalidBudget, "context_budget"),
        (ContextEstimateError::Overflow, "context_arithmetic"),
    ] {
        let error = VegaError::Context(failure.into()).at_runtime_stage(Stage::RequestPreparation);
        assert_eq!(error.diagnostic_metrics().runtime_reason, Some(reason));
        assert_eq!(
            error.diagnostic_metrics().runtime_stage,
            Some("request_preparation")
        );
    }
    assert!(matches!(
        VegaError::Context(ContextEstimateError::InputTooLarge.into())
            .at_runtime_stage(Stage::RequestPreparation),
        VegaError::Context(ContextRuntimeError::Estimate(
            ContextEstimateError::InputTooLarge
        ))
    ));
}

#[test]
fn primary_failure_event_keeps_database_and_stage_details() {
    let error = VegaError::RuntimeFailure(RuntimeFailureDetail {
        stage: Stage::PendingClaim,
        reason: Reason::Database,
        sqlite_code: Some(5),
        sqlite_extended_code: Some(5),
        io_kind: None,
    });
    let event = super::primary_attempt_event(
        "attempt",
        RuntimeDiagnosticState::Failed,
        Some(super::diagnostic_failure(&error)),
        std::time::Instant::now(),
        &RuntimeDiagnosticMetrics::default(),
        Some(&error),
    );
    let RuntimeEvent::DiagnosticAttempt(attempt) = event else {
        panic!("missing diagnostic attempt")
    };
    assert_eq!(
        attempt.failure,
        Some(RuntimeDiagnosticFailure::RuntimeFailure)
    );
    assert_eq!(attempt.metrics.runtime_stage, Some("pending_claim"));
    assert_eq!(attempt.metrics.sqlite_code, Some(5));
}
