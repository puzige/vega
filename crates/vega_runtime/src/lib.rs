mod agent;
#[cfg(feature = "question-test-support")]
pub fn question_test_event_stream(events: Vec<ProviderEvent>) -> EventStream {
    Box::pin(futures::stream::iter(events.into_iter().map(Ok)))
}
mod chat_compat;
mod chat_reasoning;
pub use chat_reasoning::{ChatRawState, ChatReasoningSource};
mod context;
mod error;
pub mod model_compat;
mod openai;
mod permission;
mod provider;
mod retry;
mod tool_context;

pub use agent::{
    AgentOutcome, AgentRequest, BASH_INVALID_INPUT_OUTPUT, CALL_ID_CONFLICT_OUTPUT,
    CANCELLED_BEFORE_EXECUTION_OUTPUT, CompletedToolCall, ContextAccountingDecision,
    ContextAccountingSource, ContextAccountingStage, CredentialConfidence,
    CredentialExceptionFilter, CredentialFinding, CredentialReader, GREP_INVALID_INPUT_OUTPUT,
    INVALID_READ_INPUT_OUTPUT, InvalidBashAudit, LEGACY_BASH_INVALID_INPUT_OUTPUT, McpReadyServer,
    McpRevocationLease, PERMISSION_TIMEOUT, PROVIDER_CREDENTIAL_REDACTION_MARKER,
    REASONING_DELTA_MAX_BYTES, REASONING_RUN_MAX_BYTES, REASONING_TURN_MAX_BYTES,
    RuntimeAssistantResponseKind, RuntimeAutomationHook, RuntimeDiagnosticAttempt,
    RuntimeDiagnosticFailure, RuntimeDiagnosticMetrics, RuntimeDiagnosticPhase,
    RuntimeDiagnosticState, RuntimeEvent, RuntimeExactRule, RuntimeFinishReason,
    RuntimePermissionHook, RuntimeSteeringInput, RuntimeSteeringSource, RuntimeSubagentHook,
    RuntimeTokenUsage, RuntimeToolCall, RuntimeToolConfig, RuntimeToolResult, RuntimeToolStatus,
    RuntimeUsagePricing, STAGE_BATCH_REJECTED, SUBAGENT_CANCELLED_OUTPUT, SUBAGENT_RESULT_BYTES,
    contains_sensitive_credential, credential_stream_safe_end, inspect_credential_text,
    is_invalid_bash_feedback, is_subagent_tool, read_identity_with_permission,
    redact_sensitive_credential_text, redact_sensitive_credential_text_with_offsets,
    redact_sensitive_credential_text_with_offsets_and_policy,
    redact_sensitive_credential_text_with_policy, run_agent, run_agent_with_permission_sink,
    run_agent_with_permission_sink_and_context, run_agent_with_sink, tool_definitions,
    tool_names_equivalent, valid_subagent_input,
};
pub use chat_compat::{ChatCompatibility, REASONING_EFFORTS, ReasoningSupport, chat_compatibility};
pub use context::{
    CONTEXT_ESTIMATOR_VERSION, ContextBudget, ContextCheck, ContextCompactionFailure,
    ContextCompactionHook, ContextCompactionPhase, ContextCompactionRequest,
    ContextCompactionResult, ContextCompactionStatusFailure, ContextCompactionStatusUpdate,
    ContextCompactionUsage, ContextCompactionUsageState, ContextEstimate, ContextEstimateError,
    ContextRuntimeError, SharedContextCompactionHook, classify_context, estimate_chat_context,
    estimate_wire_context,
};
pub use error::{
    RuntimeFailureDetail, RuntimeFailureReason, RuntimeFailureStage, RuntimeIoKind, VegaError,
};
pub mod computer_use;
pub mod images;
pub use images::ImageAttachment;
pub use openai::OpenAiProvider;
pub use permission::{
    RuntimeApprovalAudit, RuntimeApprovalDecision, RuntimeApprovalSource, RuntimeCapabilityOutcome,
    RuntimeDangerAudit, RuntimeDangerFacts, RuntimeExecuteEligibility, RuntimeExecutePermission,
    RuntimeMcpPermissionPrompt, RuntimeMutatingTool, RuntimePermissionError, RuntimePermissionMode,
    RuntimePermissionOutcome, RuntimePermissionPrompt, RuntimePermissionTarget, RuntimeRunMode,
    RuntimeToolClass, RuntimeUserDecision, decide_capability, decide_execute_permission,
};
pub use provider::{
    ChatMessage, ChatReasoningPresence, ChatRequest, ChatRole, ChatToolCall, EventStream,
    FrozenReasoning, Provider, ProviderEvent, ProviderFailureKind, ProviderResponseMetadata,
    ProviderStream, ReasoningBudgetScope, ReasoningChoice, ReasoningDisabledWire,
    ReasoningProtocol, ReasoningReplayPolicy, ReplayFrame, ResponseReplayLayout,
    ResponseReplaySlot, StopReason, ToolChoice, ToolDefinition, project_model_messages,
};
pub use retry::RetryPolicy;

pub mod provider_check;
pub mod skills;

pub use agent::{
    RuntimeUserQuestionHook, RuntimeUserQuestionRequest, RuntimeUserQuestionResponse,
    USER_QUESTION_BATCH_REJECTED, USER_QUESTION_CANCELLED, USER_QUESTION_MAX_BYTES,
    USER_QUESTION_TOOL, USER_QUESTION_UNAVAILABLE, USER_QUESTION_UNSAFE_RESPONSE, UserQuestion,
    UserQuestionAnswer, UserQuestionAnswers, UserQuestionInput, UserQuestionOption,
    user_question_validation_audit, user_question_validation_error,
    user_question_validation_output,
};
