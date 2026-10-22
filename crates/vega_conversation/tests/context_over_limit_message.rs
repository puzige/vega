use vega_conversation::types::RunFailureKind;
use vega_runtime::{ContextRuntimeError, VegaError};

#[test]
fn over_limit_message_offers_current_input_recovery_and_preserves_conversation() {
    let error = VegaError::Context(ContextRuntimeError::OverLimit {
        estimated_tokens: 300_001,
        input_budget: 300_000,
    });
    let failure = RunFailureKind::from_runtime(&error);
    assert_eq!(failure, RunFailureKind::ContextOverLimit);
    assert_eq!(
        failure.message(),
        "Context exceeds available capacity. The conversation is preserved. Reduce the current input or remove images, then retry. For long tasks, narrow tool queries or start a new conversation."
    );
}
