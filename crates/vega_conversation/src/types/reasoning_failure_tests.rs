use super::*;

#[test]
fn live_and_restored_reasoning_failures_offer_recovery_without_protocol_templates() {
    let live = RunFailureKind::from_runtime(&vega_runtime::VegaError::ReasoningReplayIncomplete);
    let metrics = vega_store::run_diagnostics::DiagnosticMetrics {
        failure_source: Some("runtime".into()),
        failure_kind: Some("protocol".into()),
        failure_scope: Some("response".into()),
        ..Default::default()
    };
    let persisted = RunFailureKind::from_diagnostic(&metrics);
    assert_eq!(live, RunFailureKind::ReasoningReplay);
    assert_eq!(persisted, live);
    let message = live.message();
    assert_eq!(persisted.message(), message);
    assert!(!message.to_lowercase().contains("template"));
    assert!(message.to_lowercase().contains("reasoning"));
    assert!(
        message.to_lowercase().contains("retry") || message.to_lowercase().contains("new message")
    );
}
