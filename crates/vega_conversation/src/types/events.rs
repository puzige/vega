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

/// Content-free explanation of a failed run. The provider's raw diagnostic
/// may contain arbitrary response text, so only an allowlisted error code or
/// HTTP metadata is allowed to cross into rendered conversation state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunFailureKind {
    /// The provider explicitly reported an exhausted account quota.
    ProviderQuota,
    /// The provider rejected the local credential or account permissions.
    ProviderAuthorization,
    /// The provider rate-limited this request.
    ProviderRateLimited,
    /// Another HTTP response rejected the request.
    ProviderHttp(u16),
    /// No HTTP response was received.
    ProviderTransport,
    /// An in-process runtime error rather than a provider HTTP failure.
    Runtime,
    /// A durable failed row whose original reason was not persisted.
    Persisted,
}

impl RunFailureKind {
    /// Reduces the typed runtime error without retaining or displaying its
    /// provider-supplied body. A quota diagnosis requires the exact structured
    /// code in an HTTP 400 response; free-form prose is never trusted.
    pub fn from_runtime(error: &vega_runtime::VegaError) -> Self {
        match error {
            vega_runtime::VegaError::Provider {
                status: Some(400),
                message,
                ..
            } if provider_has_quota_code(message) => Self::ProviderQuota,
            vega_runtime::VegaError::Provider {
                status: Some(401 | 403),
                ..
            } => Self::ProviderAuthorization,
            vega_runtime::VegaError::Provider {
                status: Some(429), ..
            } => Self::ProviderRateLimited,
            vega_runtime::VegaError::Provider {
                status: Some(status),
                ..
            } => Self::ProviderHttp(*status),
            vega_runtime::VegaError::Provider { status: None, .. } => Self::ProviderTransport,
            _ => Self::Runtime,
        }
    }

    /// Actionable text made entirely from local vocabulary and HTTP metadata.
    pub fn message(self) -> String {
        match self {
            Self::ProviderQuota => {
                "供应商额度不足；请充值或在设置 → Providers 切换可用供应商后重试".into()
            }
            Self::ProviderAuthorization => {
                "供应商拒绝了凭据或账号权限；请在设置 → Providers 检查 API Key 后重试".into()
            }
            Self::ProviderRateLimited => "供应商限流；请稍后重试或切换可用供应商".into(),
            Self::ProviderHttp(status) => {
                format!("供应商请求失败（HTTP {status}）；请检查供应商状态、模型和额度后重试")
            }
            Self::ProviderTransport => "无法连接供应商；请检查网络和供应商地址后重试".into(),
            Self::Runtime => "任务执行失败；请检查运行环境后重试".into(),
            Self::Persisted => "这条回复执行失败；请检查供应商状态、额度或运行环境后重试".into(),
        }
    }
}

fn provider_has_quota_code(message: &str) -> bool {
    // OpenAiProvider prefixes a bounded response snippet with `...): `.
    // Parsing only the JSON body avoids interpreting arbitrary prose (or a
    // secret that happens to contain the marker) as a quota error. CPA sends
    // `code` at the top level; other compatible providers nest it in `error`.
    let Some((_, body)) = message.split_once("): ") else {
        return false;
    };
    let Ok(response) = serde_json::from_str::<serde_json::Value>(body) else {
        return false;
    };
    ["/error/code", "/code"].into_iter().any(|pointer| {
        response.pointer(pointer).and_then(|code| code.as_str()) == Some("insufficient_user_quota")
    })
}

fn now_unix_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(0)
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
    /// Usage from a historical-summary provider call. It is separate from
    /// ordinary assistant usage because a manual/automatic summary has no
    /// assistant prose message id.
    ContextCompactionUsageUpdated {
        /// Token counts actually supplied by the summary provider.
        usage: TokenUsage,
        /// Integer cost when the frozen catalog could price the model.
        cost: Option<Microcents>,
        /// Frozen pricing provenance, absent for unpriced/unknown models.
        pricing: Option<UsagePricing>,
    },
    /// Content-free lifecycle state for a historical-summary operation.
    /// `Unknown` usage is intentionally observable so a priced primary call
    /// cannot make an incompletely accounted run look fully priced.
    ContextCompactionStatus {
        /// Safe status metadata; no transcript or provider payloads.
        record: ContextCompactionStatusRecord,
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
            Self::ContextCompactionUsageUpdated {
                usage,
                cost,
                pricing,
            } => formatter
                .debug_struct("ContextCompactionUsageUpdated")
                .field("usage", usage)
                .field("cost", cost)
                .field("priced", &pricing.is_some())
                .finish(),
            Self::ContextCompactionStatus { record } => formatter
                .debug_struct("ContextCompactionStatus")
                .field("generation", &record.generation)
                .field("status", &record.status)
                .field("usage", &record.usage)
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
        RuntimeEvent::ContextCompactionUsageUpdated { usage } => {
            Some(ConversationEvent::ContextCompactionUsageUpdated {
                usage: TokenUsage {
                    input: usage.usage.input,
                    output: usage.usage.output,
                    cache_read: usage.usage.cache_read,
                    cache_write: usage.usage.cache_write,
                },
                cost: usage
                    .pricing
                    .is_some()
                    .then_some(Microcents(usage.cost_microcents)),
                pricing: usage.pricing.as_ref().map(|pricing| UsagePricing {
                    version: pricing.version.clone(),
                    profile: pricing.profile.clone(),
                    call_started_at: pricing.call_started_at,
                }),
            })
        }
        RuntimeEvent::ContextCompactionStatusUpdated { status } => {
            let usage = match status.usage {
                vega_runtime::ContextCompactionUsageState::Pending => {
                    ContextCompactionUsageState::Pending
                }
                vega_runtime::ContextCompactionUsageState::Known { priced } => {
                    ContextCompactionUsageState::Known { priced }
                }
                vega_runtime::ContextCompactionUsageState::Unknown => {
                    ContextCompactionUsageState::Unknown
                }
            };
            let failure = match status.failure {
                None => None,
                Some(vega_runtime::ContextCompactionStatusFailure::Cancelled) => {
                    Some(ContextCompactionFailureCode::Cancelled)
                }
                Some(vega_runtime::ContextCompactionStatusFailure::SourceChanged) => {
                    Some(ContextCompactionFailureCode::SourceChanged)
                }
                Some(vega_runtime::ContextCompactionStatusFailure::NoCompactablePrefix) => {
                    Some(ContextCompactionFailureCode::NoCompactablePrefix)
                }
                Some(vega_runtime::ContextCompactionStatusFailure::TooLarge) => {
                    Some(ContextCompactionFailureCode::TooLarge)
                }
                Some(vega_runtime::ContextCompactionStatusFailure::InvalidSummary) => {
                    Some(ContextCompactionFailureCode::InvalidSummary)
                }
                Some(vega_runtime::ContextCompactionStatusFailure::ImagesUnsupported) => {
                    Some(ContextCompactionFailureCode::ImagesUnsupported)
                }
                Some(vega_runtime::ContextCompactionStatusFailure::OverLimit) => {
                    Some(ContextCompactionFailureCode::OverLimit)
                }
                Some(vega_runtime::ContextCompactionStatusFailure::Unavailable) => {
                    Some(ContextCompactionFailureCode::Unavailable)
                }
            };
            Some(ConversationEvent::ContextCompactionStatus {
                record: ContextCompactionStatusRecord {
                    generation: status.generation,
                    status: match status.phase {
                        vega_runtime::ContextCompactionPhase::Started => {
                            ContextCompactionStatus::Compacting
                        }
                        vega_runtime::ContextCompactionPhase::Succeeded => {
                            ContextCompactionStatus::Succeeded
                        }
                        vega_runtime::ContextCompactionPhase::Failed => {
                            ContextCompactionStatus::Failed
                        }
                        vega_runtime::ContextCompactionPhase::Cancelled => {
                            ContextCompactionStatus::Cancelled
                        }
                    },
                    updated_at: now_unix_millis(),
                    estimated_tokens: Some(status.estimated_tokens),
                    input_budget: Some(status.input_budget),
                    target_tokens: Some(status.target_tokens),
                    source_version: Some(status.source_version),
                    failure,
                    usage,
                },
            })
        }
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
    let projected = ToolCall {
        id: call.id.clone(),
        tool: call.name.clone(),
        input_json: call.input_json.clone(),
    };
    if call.name.starts_with("mcp_") {
        // The runtime has already replaced raw MCP arguments with this
        // value-free identity. Keep its exact alias binding at the live UI
        // boundary; the generic unknown-tool rule below would drop every
        // valid MCP proposal before the approval card can appear.
        return McpCallIdentity::from_tool_call(&projected).map(|_| projected);
    }
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
    Some(projected)
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

#[cfg(test)]
mod run_failure_tests {
    use super::RunFailureKind;
    use vega_runtime::VegaError;

    fn provider(status: Option<u16>, message: &str) -> VegaError {
        VegaError::Provider {
            status,
            message: message.into(),
            retryable: false,
        }
    }

    #[test]
    fn quota_needs_exact_structured_code_and_never_projects_response_body() {
        const SENTINEL: &str = "PRIVATE_RESPONSE_AND_KEY_SENTINEL";
        let error = provider(
            Some(400),
            &format!(
                "chat/completions request failed (HTTP 400): {{\"error\":{{\"code\":\"insufficient_user_quota\",\"message\":\"{SENTINEL}\"}}}}"
            ),
        );
        let kind = RunFailureKind::from_runtime(&error);
        assert_eq!(kind, RunFailureKind::ProviderQuota);
        assert!(kind.message().contains("额度不足"));
        assert!(!kind.message().contains(SENTINEL));

        let spoofed = provider(
            Some(400),
            "chat/completions request failed (HTTP 400): insufficient_user_quota PRIVATE_RESPONSE_AND_KEY_SENTINEL",
        );
        assert_eq!(
            RunFailureKind::from_runtime(&spoofed),
            RunFailureKind::ProviderHttp(400)
        );
        assert!(
            !RunFailureKind::from_runtime(&spoofed)
                .message()
                .contains(SENTINEL)
        );
    }

    #[test]
    fn quota_accepts_cpa_top_level_code_without_exposing_response_body() {
        const SENTINEL: &str = "PRIVATE_CPA_RESPONSE_SENTINEL";
        let response = format!(
            "chat/completions request failed (HTTP 400): {{\"message\":\"credit insufficient {SENTINEL}\",\"type\":\"api_error\",\"param\":\"\",\"code\":\"insufficient_user_quota\"}}"
        );
        let error = provider(Some(400), &response);
        let kind = RunFailureKind::from_runtime(&error);
        assert_eq!(kind, RunFailureKind::ProviderQuota);
        assert!(kind.message().contains("额度不足"));
        assert!(!kind.message().contains(SENTINEL));

        let wrong_status = provider(Some(503), &response);
        assert_eq!(
            RunFailureKind::from_runtime(&wrong_status),
            RunFailureKind::ProviderHttp(503)
        );
    }

    #[test]
    fn provider_and_runtime_failure_categories_keep_only_safe_metadata() {
        for (status, expected) in [
            (Some(401), RunFailureKind::ProviderAuthorization),
            (Some(403), RunFailureKind::ProviderAuthorization),
            (Some(429), RunFailureKind::ProviderRateLimited),
            (Some(503), RunFailureKind::ProviderHttp(503)),
            (None, RunFailureKind::ProviderTransport),
        ] {
            let kind = RunFailureKind::from_runtime(&provider(status, "SECRET_SENTINEL"));
            assert_eq!(kind, expected);
            assert!(!kind.message().contains("SECRET_SENTINEL"));
        }
        assert_eq!(
            RunFailureKind::from_runtime(&VegaError::ReasoningSelectionInvalid {
                message: "SECRET_SENTINEL".into(),
            }),
            RunFailureKind::Runtime
        );
        assert!(
            !RunFailureKind::Runtime
                .message()
                .contains("SECRET_SENTINEL")
        );
    }
}
