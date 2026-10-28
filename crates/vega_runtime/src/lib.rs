mod agent;
mod chat_compat;
mod chat_reasoning;
pub use chat_reasoning::{ChatRawState, ChatReasoningSource};
mod context;
mod context_micro;
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
    CredentialExceptionFilter, CredentialFinding, CredentialReader, DANGER_DENIED_OUTPUT,
    GREP_INVALID_INPUT_OUTPUT, INVALID_READ_INPUT_OUTPUT, InvalidBashAudit,
    LEGACY_BASH_INVALID_INPUT_OUTPUT, McpReadyServer, McpRevocationLease,
    PERMISSION_CANCELLED_OUTPUT, PERMISSION_DENIED_OUTPUT, PERMISSION_TIMEOUT,
    PERMISSION_TIMEOUT_OUTPUT, PERMISSION_UNAVAILABLE_OUTPUT, PROVIDER_CREDENTIAL_REDACTION_MARKER,
    RuntimeAssistantResponseKind, RuntimeAutomationHook, RuntimeDiagnosticAttempt,
    RuntimeDiagnosticFailure, RuntimeDiagnosticMetrics, RuntimeDiagnosticPhase,
    RuntimeDiagnosticState, RuntimeEvent, RuntimeExactRule, RuntimeFinishReason,
    RuntimeGeneratedNoticeKind, RuntimePermissionHook, RuntimeSteeringInput, RuntimeSteeringSource,
    RuntimeSubagentHook, RuntimeTokenUsage, RuntimeToolCall, RuntimeToolConfig, RuntimeToolResult,
    RuntimeToolStatus, RuntimeUsagePricing, STAGE_BATCH_REJECTED, SUBAGENT_CANCELLED_OUTPUT,
    SUBAGENT_RESULT_BYTES, contains_sensitive_credential, credential_stream_safe_end,
    inspect_credential_text, is_invalid_bash_feedback, is_subagent_tool,
    read_identity_with_permission, redact_sensitive_credential_text,
    redact_sensitive_credential_text_with_offsets,
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
    ContextCompactionTrigger, ContextCompactionUsage, ContextCompactionUsageState, ContextEstimate,
    ContextEstimateError, ContextRuntimeError, SharedContextCompactionHook, classify_context,
    estimate_chat_context, estimate_wire_context, validate_compaction_replay,
};
pub use context_micro::{
    MICRO_KEEP_RECENT, MICRO_MIN_SAVINGS, MicroCompactionRequest, MicroCompactionResult,
    MicroCompactionTarget, apply_micro_compaction, micro_result_content,
};
pub use error::{
    ProviderProtocolReason, RuntimeFailureDetail, RuntimeFailureReason, RuntimeFailureStage,
    RuntimeIoKind, VegaError,
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
