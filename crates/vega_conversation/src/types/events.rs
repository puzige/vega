use super::*;

/// Why a conversation message finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversationStopReason {
    /// Provider natural end.
    End,
    /// Provider generation limit.
    Length,
    /// Runtime tool-call safety limit.
    ToolLimit,
}

/// Runtime-to-UI/store unique event stream (tech-spec §3).
#[derive(Clone)]
pub enum ConversationEvent {
    /// A streaming assistant row was created.
    MessageStarted {
        /// Assistant message id.
        message_id: MessageId,
        /// Monotonic thread-local sequence.
        seq: u64,
    },
    /// Visible assistant delta.
    TextDelta {
        /// Assistant message id.
        message_id: MessageId,
        /// Incremental visible text.
        delta: String,
    },
    /// Reasoning delta.
    ThinkingDelta {
        /// Assistant message id.
        message_id: MessageId,
        /// Incremental reasoning text.
        delta: String,
    },
    /// Tool proposal awaiting the placeholder permission hook.
    ToolCallProposed {
        /// Complete proposal.
        call: ToolCall,
    },
    /// Tool approval.
    ToolCallApproved {
        /// Provider call id.
        call_id: CallId,
        /// Permission decision.
        approval: Approval,
    },
    /// Tool output chunk.
    ToolCallOutput {
        /// Provider call id.
        call_id: CallId,
        /// Truncated display output.
        chunk: ToolOutputChunk,
    },
    /// Terminal tool result.
    ToolCallFinished {
        /// Provider call id.
        call_id: CallId,
        /// Terminal result.
        result: ToolResult,
    },
    /// Provider usage and integer cost.
    UsageUpdated {
        /// Assistant message id.
        message_id: MessageId,
        /// Provider token counts.
        usage: TokenUsage,
        /// Integer cost (`0` in S4; priced-zero rows keep `0` with
        /// provenance).
        cost: Microcents,
        /// Exact pricing provenance (S7-T38); `None` keeps the S4
        /// legacy/unpriced semantics.
        pricing: Option<UsagePricing>,
    },
    /// Assistant message converged.
    MessageFinished {
        /// Assistant message id.
        message_id: MessageId,
        /// Convergence reason.
        stop_reason: ConversationStopReason,
    },
    /// Runtime/provider error.
    Error {
        /// Assistant message id, when a message had started.
        message_id: Option<MessageId>,
        /// Safe display error.
        error: Arc<vega_runtime::VegaError>,
    },
    /// Cancellation was observed.
    Interrupted {
        /// Interrupted assistant message id.
        message_id: MessageId,
    },
}

impl std::fmt::Debug for ConversationEvent {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MessageStarted { message_id, seq } => formatter
                .debug_struct("MessageStarted")
                .field("message_id_bytes", &message_id.len())
                .field("seq", seq)
                .finish(),
            Self::TextDelta { message_id, delta } => formatter
                .debug_struct("TextDelta")
                .field("message_id_bytes", &message_id.len())
                .field("delta_bytes", &delta.len())
                .finish(),
            Self::ThinkingDelta { message_id, delta } => formatter
                .debug_struct("ThinkingDelta")
                .field("message_id_bytes", &message_id.len())
                .field("delta_bytes", &delta.len())
                .finish(),
            Self::ToolCallProposed { call } => formatter
                .debug_struct("ToolCallProposed")
                .field("call", call)
                .finish(),
            Self::ToolCallApproved { call_id, approval } => formatter
                .debug_struct("ToolCallApproved")
                .field("call_id_bytes", &call_id.len())
                .field("approval", approval)
                .finish(),
            Self::ToolCallOutput { call_id, chunk } => formatter
                .debug_struct("ToolCallOutput")
                .field("call_id_bytes", &call_id.len())
                .field("chunk", chunk)
                .finish(),
            Self::ToolCallFinished { call_id, result } => formatter
                .debug_struct("ToolCallFinished")
                .field("call_id_bytes", &call_id.len())
                .field("result", result)
                .finish(),
            Self::UsageUpdated {
                message_id,
                usage,
                cost,
                pricing,
            } => formatter
                .debug_struct("UsageUpdated")
                .field("message_id_bytes", &message_id.len())
                .field("usage", usage)
                .field("cost", cost)
                .field("priced", &pricing.is_some())
                .finish(),
            Self::MessageFinished {
                message_id,
                stop_reason,
            } => formatter
                .debug_struct("MessageFinished")
                .field("message_id_bytes", &message_id.len())
                .field("stop_reason", stop_reason)
                .finish(),
            Self::Error {
                message_id,
                error: _,
            } => formatter
                .debug_struct("Error")
                .field("message_id_bytes", &message_id.as_ref().map(String::len))
                .finish(),
            Self::Interrupted { message_id } => formatter
                .debug_struct("Interrupted")
                .field("message_id_bytes", &message_id.len())
                .finish(),
        }
    }
}

/// Converts one headless runtime event into the shared conversation event.
/// Runtime-only `ToolCallRunning` is persisted but has no UI event in §3.
pub(crate) fn from_runtime_event(
    message_id: &str,
    event: &vega_runtime::RuntimeEvent,
) -> Option<ConversationEvent> {
    use vega_runtime::{RuntimeEvent, RuntimeFinishReason, RuntimeToolStatus};

    match event {
        RuntimeEvent::TextDelta(delta) => Some(ConversationEvent::TextDelta {
            message_id: message_id.to_string(),
            delta: delta.clone(),
        }),
        RuntimeEvent::ThinkingDelta(delta) => Some(ConversationEvent::ThinkingDelta {
            message_id: message_id.to_string(),
            delta: delta.clone(),
        }),
        RuntimeEvent::ToolCallProposed(call) => Some(ConversationEvent::ToolCallProposed {
            call: safe_runtime_tool_call(call)?,
        }),
        RuntimeEvent::ToolCallValidationRejected { call, result } => {
            let invalid = validate_runtime_validation_rejection(call, result)?;
            Some(ConversationEvent::ToolCallFinished {
                call_id: result.call_id.clone(),
                result: ToolResult {
                    status: ToolCallStatus::Rejected,
                    output: result.output.clone(),
                    reused: result.reused,
                    exit_code: result.exit_code,
                    duration_ms: result.duration_ms,
                    truncated: result.truncated,
                    invalid: Some(invalid),
                },
            })
        }
        RuntimeEvent::ToolCallConflict { result, .. } => {
            Some(ConversationEvent::ToolCallFinished {
                call_id: result.call_id.clone(),
                result: ToolResult {
                    status: ToolCallStatus::Failed,
                    output: result.output.clone(),
                    reused: result.reused,
                    exit_code: result.exit_code,
                    duration_ms: result.duration_ms,
                    truncated: result.truncated,
                    invalid: None,
                },
            })
        }
        RuntimeEvent::ToolCallApproved { call_id, audit, .. } => {
            Some(ConversationEvent::ToolCallApproved {
                call_id: call_id.clone(),
                approval: approval_from_runtime(audit.decision),
            })
        }
        RuntimeEvent::ToolCallRunning { .. } => None,
        RuntimeEvent::ToolCallOutput { call_id, chunk } => {
            Some(ConversationEvent::ToolCallOutput {
                call_id: call_id.clone(),
                chunk: ToolOutputChunk(chunk.clone()),
            })
        }
        RuntimeEvent::ToolCallFinished(result) => Some(ConversationEvent::ToolCallFinished {
            call_id: result.call_id.clone(),
            result: ToolResult {
                status: match result.status {
                    RuntimeToolStatus::Rejected => ToolCallStatus::Rejected,
                    RuntimeToolStatus::Success => ToolCallStatus::Success,
                    RuntimeToolStatus::Failed => ToolCallStatus::Failed,
                    RuntimeToolStatus::Cancelled => ToolCallStatus::Cancelled,
                },
                output: result.output.clone(),
                reused: result.reused,
                exit_code: result.exit_code,
                duration_ms: result.duration_ms,
                truncated: result.truncated,
                invalid: None,
            },
        }),
        RuntimeEvent::UsageUpdated {
            usage,
            cost_microcents,
            pricing,
        } => Some(ConversationEvent::UsageUpdated {
            message_id: message_id.to_string(),
            usage: TokenUsage {
                input: usage.input,
                output: usage.output,
                cache_read: usage.cache_read,
                cache_write: usage.cache_write,
            },
            cost: Microcents(*cost_microcents),
            pricing: pricing.as_ref().map(|pricing| UsagePricing {
                version: pricing.version.clone(),
                profile: pricing.profile.clone(),
                call_started_at: pricing.call_started_at,
            }),
        }),
        RuntimeEvent::Finished(reason) => Some(ConversationEvent::MessageFinished {
            message_id: message_id.to_string(),
            stop_reason: match reason {
                RuntimeFinishReason::End => ConversationStopReason::End,
                RuntimeFinishReason::Length => ConversationStopReason::Length,
                RuntimeFinishReason::ToolLimit => ConversationStopReason::ToolLimit,
            },
        }),
        RuntimeEvent::Interrupted => Some(ConversationEvent::Interrupted {
            message_id: message_id.to_string(),
        }),
        RuntimeEvent::Error(error) => Some(ConversationEvent::Error {
            message_id: Some(message_id.to_string()),
            error: error.clone(),
        }),
    }
}

pub(crate) fn safe_runtime_tool_call(call: &vega_runtime::RuntimeToolCall) -> Option<ToolCall> {
    if matches!(call.name.as_str(), "write" | "edit") {
        let audit = vega_tools::WriteEditAudit::from_json(&call.input_json).ok()?;
        if audit.tool().as_str() != call.name {
            return None;
        }
    } else if !matches!(call.name.as_str(), "read" | "glob" | "grep" | "bash")
        && call.input_json != "{}"
    {
        return None;
    }
    Some(ToolCall {
        id: call.id.clone(),
        tool: call.name.clone(),
        input_json: call.input_json.clone(),
    })
}

pub(crate) fn validate_runtime_validation_rejection(
    call: &vega_runtime::RuntimeToolCall,
    result: &vega_runtime::RuntimeToolResult,
) -> Option<InvalidToolProjection> {
    let audit = vega_tools::InvalidWriteEditAudit::from_json(&call.input_json).ok()?;
    let approval = result.approval.as_ref()?;
    let expected = format!(
        "Tool error: invalid {} input ({})",
        call.name,
        audit.validation_error_code().as_str()
    );
    if audit.tool().as_str() == call.name
        && call.id == result.call_id
        && result.status == vega_runtime::RuntimeToolStatus::Rejected
        && result.output == expected
        && result.exit_code.is_none()
        && result.duration_ms.is_none()
        && result.remember_rule.is_none()
        && approval.decision == vega_runtime::RuntimeApprovalDecision::Deny
        && approval.source == vega_runtime::RuntimeApprovalSource::Validation
    {
        Some(InvalidToolProjection {
            tool: match audit.tool() {
                vega_tools::MutationTool::Write => InvalidToolKind::Write,
                vega_tools::MutationTool::Edit => InvalidToolKind::Edit,
            },
            code: invalid_tool_code(audit.validation_error_code())?,
        })
    } else {
        None
    }
}

pub(crate) fn invalid_tool_code(code: vega_tools::MutationErrorCode) -> Option<InvalidToolCode> {
    use vega_tools::MutationErrorCode as Code;
    Some(match code {
        Code::MalformedJson => InvalidToolCode::MalformedJson,
        Code::InputNotObject => InvalidToolCode::InputNotObject,
        Code::UnexpectedField => InvalidToolCode::UnexpectedField,
        Code::MissingPath => InvalidToolCode::MissingPath,
        Code::WrongPathType => InvalidToolCode::WrongPathType,
        Code::MissingContent => InvalidToolCode::MissingContent,
        Code::WrongContentType => InvalidToolCode::WrongContentType,
        Code::MissingOldString => InvalidToolCode::MissingOldString,
        Code::WrongOldStringType => InvalidToolCode::WrongOldStringType,
        Code::MissingNewString => InvalidToolCode::MissingNewString,
        Code::WrongNewStringType => InvalidToolCode::WrongNewStringType,
        Code::PathAbsolute => InvalidToolCode::PathAbsolute,
        Code::PathParent => InvalidToolCode::PathParent,
        Code::PathRoot => InvalidToolCode::PathRoot,
        Code::PathSymlink => InvalidToolCode::PathSymlink,
        Code::PathHardlink => InvalidToolCode::PathHardlink,
        Code::PathGit => InvalidToolCode::PathGit,
        Code::PathNotFile => InvalidToolCode::PathNotFile,
        Code::ParentNotFound => InvalidToolCode::ParentNotFound,
        Code::TargetNotFound => InvalidToolCode::TargetNotFound,
        Code::CheckpointIdInvalid => InvalidToolCode::CheckpointIdInvalid,
        Code::CheckpointUnavailable => InvalidToolCode::CheckpointUnavailable,
        Code::CheckpointSymlink => InvalidToolCode::CheckpointSymlink,
        Code::EditEmptyOldString => InvalidToolCode::EditEmptyOldString,
        Code::FilesystemError => InvalidToolCode::FilesystemError,
        Code::CheckpointExists
        | Code::CheckpointMetadataInvalid
        | Code::EditNoMatch
        | Code::EditMultipleMatches
        | Code::TargetChanged
        | Code::AtomicWriteFailed
        | Code::CodecInvalid
        | Code::PreparedScopeMismatch => return None,
    })
}
