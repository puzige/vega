use super::*;

/// Why a conversation message finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversationStopReason {
    /// Provider natural end.
    End,
    /// Provider generation limit.
    Length,
    TurnLimit,
}

/// Provider-side facts known when a run failed: bounded allowlisted metadata
/// only, never a provider response body.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProviderFailureFact {
    /// HTTP status of the response that was already established, when known.
    pub http_status: Option<u16>,
    /// Retries already spent before this terminal failure.
    pub retries: Option<u32>,
}

impl ProviderFailureFact {
    /// Localized, verifiable facts joined by a separator; `None` when empty.
    /// An unknown status and a zero retry count produce nothing rather than a
    /// placeholder or a misleading `0`.
    pub fn summary(self) -> Option<String> {
        let mut parts = Vec::new();
        if let Some(status) = self.http_status {
            parts.push(vega_i18n::t_args(
                "domain.failure.provider_fact_http",
                &[("status", &status.to_string())],
            ));
        }
        if let Some(retries) = self.retries.filter(|retries| *retries > 0) {
            parts.push(vega_i18n::t_args(
                "domain.failure.provider_fact_retries",
                &[("retries", &retries.to_string())],
            ));
        }
        (!parts.is_empty()).then(|| parts.join(" · "))
    }
}

/// Content-free explanation of a failed run. The provider's raw diagnostic
/// may contain arbitrary response text, so only an allowlisted error code or
/// HTTP metadata is allowed to cross into rendered conversation state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunFailureKind {
    ProviderUnknown,
    ProviderProtocol,
    ProviderRejected,
    ProviderOutputLimit,
    /// The provider explicitly reported an exhausted account quota.
    ProviderQuota,
    /// The provider rejected the local credential or account permissions.
    ProviderAuthorization,
    /// The provider rate-limited this request.
    ProviderRateLimited,
    /// Another HTTP response rejected the request.
    ProviderHttp(u16),
    /// No HTTP response was received: DNS, TCP/TLS or redirect failure.
    ProviderTransport(ProviderFailureFact),
    /// An established response stream ended before its terminal event.
    ProviderStreamInterrupted(ProviderFailureFact),
    /// An established response stream or request exceeded its deadline.
    ProviderTimeout(ProviderFailureFact),
    /// The provider reported inside an established response that it cannot
    /// serve the request right now.
    ProviderOverloaded(ProviderFailureFact),
    /// An in-process runtime error rather than a provider HTTP failure.
    Runtime,
    RuntimeStorage,
    RuntimeStorageBusy,
    RuntimeApprovalAudit,
    RuntimeState,
    RuntimeIo,
    RuntimePipeline,
    RuntimeReferences,
    RuntimeCapability,
    RuntimeContextBudget,
    RuntimeContextArithmetic,
    #[allow(missing_docs)]
    CredentialExposureBlocked,
    CredentialCheckUnavailable,
    ProjectInstructionsUnavailable,
    ProjectPathInvalid,
    CredentialContinuationUnavailable,
    CredentialProjectionInvalid(usize),
    CredentialOutcomeUnknown,
    ContextOverLimit,
    #[allow(missing_docs)]
    Compaction(ContextCompactionFailureCode),
    #[allow(missing_docs)]
    ReasoningLimit(Option<vega_runtime::ReasoningBudgetScope>),
    ReasoningReplay,
    ModelImagesUnsupported,
    ReasoningSelectionInvalid,
    /// A durable failed row whose original reason was not persisted.
    Persisted,
}

impl RunFailureKind {
    pub(crate) fn from_compaction_diagnostic_code(
        code: vega_store::run_diagnostics::DiagnosticFailureCode,
    ) -> Option<Self> {
        use vega_store::run_diagnostics::DiagnosticFailureCode as Code;
        match code {
            Code::ContextOverLimit => Some(Self::ContextOverLimit),
            Code::SummaryTimeout => Some(Self::Compaction(ContextCompactionFailureCode::TimedOut)),
            Code::SummaryTruncated
            | Code::SummaryEmpty
            | Code::SummaryFormatInvalid
            | Code::SummaryProjectionInvalid => Some(Self::Compaction(
                ContextCompactionFailureCode::InvalidSummary,
            )),
            Code::SummarySourceChanged => Some(Self::Compaction(
                ContextCompactionFailureCode::SourceChanged,
            )),
            Code::SummarySourceTooLarge | Code::SummaryAggregateTooLarge => {
                Some(Self::Compaction(ContextCompactionFailureCode::TooLarge))
            }
            Code::SummaryImagesUnsupported => Some(Self::Compaction(
                ContextCompactionFailureCode::ImagesUnsupported,
            )),
            Code::SummaryNoCompactablePrefix => Some(Self::Compaction(
                ContextCompactionFailureCode::NoCompactablePrefix,
            )),
            Code::SummaryAlreadyAttempted => {
                Some(Self::Compaction(ContextCompactionFailureCode::Unavailable))
            }
            _ => None,
        }
    }

    pub(crate) fn from_diagnostic(
        metrics: &vega_store::run_diagnostics::DiagnosticMetrics,
    ) -> Self {
        if let Some(reason) = metrics.runtime_reason.as_deref() {
            return match reason {
                "database" if matches!(metrics.sqlite_code, Some(5 | 6)) => {
                    Self::RuntimeStorageBusy
                }
                "database" => Self::RuntimeStorage,
                "approval_audit" => Self::RuntimeApprovalAudit,
                "state" | "admission" | "pending_decode" => Self::RuntimeState,
                "references" => Self::RuntimeReferences,
                "capability" => Self::RuntimeCapability,
                "context_budget" => Self::RuntimeContextBudget,
                "context_arithmetic" => Self::RuntimeContextArithmetic,
                "io" => Self::RuntimeIo,
                "pipeline" | "worker" => Self::RuntimePipeline,
                _ => Self::Runtime,
            };
        }
        match metrics.failure_kind.as_deref() {
            Some("quota") => Self::ProviderQuota,
            Some("authorization") => Self::ProviderAuthorization,
            Some("rate_limited") => Self::ProviderRateLimited,
            Some("http") => metrics
                .http_status
                .map(Self::ProviderHttp)
                .unwrap_or(Self::ProviderRejected),
            // Rows written before the split merged connect failures and
            // interrupted streams under one kind. A recorded HTTP status proves
            // a response was established, so such a row is an interrupted
            // stream rather than a connection failure.
            Some("transport") => match metrics.http_status {
                Some(_) => Self::ProviderStreamInterrupted(ProviderFailureFact {
                    http_status: metrics.http_status,
                    retries: metrics.retry_count,
                }),
                None => Self::ProviderTransport(ProviderFailureFact {
                    http_status: None,
                    retries: metrics.retry_count,
                }),
            },
            Some("stream_interrupted") => Self::ProviderStreamInterrupted(ProviderFailureFact {
                http_status: metrics.http_status,
                retries: metrics.retry_count,
            }),
            Some("timeout") => Self::ProviderTimeout(ProviderFailureFact {
                http_status: metrics.http_status,
                retries: metrics.retry_count,
            }),
            Some("overloaded") => Self::ProviderOverloaded(ProviderFailureFact {
                http_status: metrics.http_status,
                retries: metrics.retry_count,
            }),
            Some("protocol")
                if metrics.failure_source.as_deref() == Some("runtime")
                    && metrics.failure_scope.as_deref() == Some("response") =>
            {
                Self::ReasoningReplay
            }
            Some("protocol") => Self::ProviderProtocol,
            Some("rejected") => Self::ProviderRejected,
            Some("output_limit") => Self::ProviderOutputLimit,
            Some("context_over_limit") => Self::ContextOverLimit,
            Some("model_images") => Self::ModelImagesUnsupported,
            Some("reasoning_selection") => Self::ReasoningSelectionInvalid,
            Some("reasoning_limit") => {
                Self::ReasoningLimit(match metrics.failure_scope.as_deref() {
                    Some("delta") => Some(vega_runtime::ReasoningBudgetScope::Delta),
                    Some("response") => Some(vega_runtime::ReasoningBudgetScope::Turn),
                    Some("run") => Some(vega_runtime::ReasoningBudgetScope::Run),
                    _ => None,
                })
            }
            Some("credential_exposure") => Self::CredentialExposureBlocked,
            Some("credential_check") => Self::CredentialCheckUnavailable,
            Some("project_instructions") => Self::ProjectInstructionsUnavailable,
            Some("project_path") => Self::ProjectPathInvalid,
            Some("credential_continuation") => Self::CredentialContinuationUnavailable,
            Some("credential_projection") => metrics
                .failure_tool_index
                .and_then(|value| usize::try_from(value).ok())
                .map(Self::CredentialProjectionInvalid)
                .unwrap_or(Self::Runtime),
            Some("unknown") if metrics.failure_source.as_deref() == Some("provider") => {
                Self::ProviderUnknown
            }
            _ => Self::Runtime,
        }
    }

    /// Reduces the typed runtime error without retaining or displaying its
    /// provider-supplied body. A quota diagnosis requires the exact structured
    /// code in an HTTP 400 response; free-form prose is never trusted.
    pub fn from_runtime(error: &vega_runtime::VegaError) -> Self {
        if error.context_input_overflow().is_some() {
            return Self::ContextOverLimit;
        }
        match error {
            vega_runtime::VegaError::RuntimeFailure(detail) => match detail.reason {
                vega_runtime::RuntimeFailureReason::Database
                    if matches!(detail.sqlite_code, Some(5 | 6)) =>
                {
                    Self::RuntimeStorageBusy
                }
                vega_runtime::RuntimeFailureReason::Database => Self::RuntimeStorage,
                vega_runtime::RuntimeFailureReason::ApprovalAudit => Self::RuntimeApprovalAudit,
                vega_runtime::RuntimeFailureReason::State
                | vega_runtime::RuntimeFailureReason::Admission
                | vega_runtime::RuntimeFailureReason::PendingDecode => Self::RuntimeState,
                vega_runtime::RuntimeFailureReason::References => Self::RuntimeReferences,
                vega_runtime::RuntimeFailureReason::Capability => Self::RuntimeCapability,
                vega_runtime::RuntimeFailureReason::ContextBudget => Self::RuntimeContextBudget,
                vega_runtime::RuntimeFailureReason::ContextArithmetic => {
                    Self::RuntimeContextArithmetic
                }
                vega_runtime::RuntimeFailureReason::Io => Self::RuntimeIo,
                vega_runtime::RuntimeFailureReason::Pipeline
                | vega_runtime::RuntimeFailureReason::Worker => Self::RuntimePipeline,
            },
            vega_runtime::VegaError::Store(_) | vega_runtime::VegaError::Io(_) => {
                let detail = error.diagnostic_metrics();
                if detail.runtime_reason == Some("database") {
                    if matches!(detail.sqlite_code, Some(5 | 6)) {
                        Self::RuntimeStorageBusy
                    } else {
                        Self::RuntimeStorage
                    }
                } else {
                    Self::RuntimeIo
                }
            }
            vega_runtime::VegaError::Context(vega_runtime::ContextRuntimeError::Estimate(
                vega_runtime::ContextEstimateError::InvalidBudget,
            )) => Self::RuntimeContextBudget,
            vega_runtime::VegaError::Context(vega_runtime::ContextRuntimeError::Estimate(
                vega_runtime::ContextEstimateError::Overflow,
            )) => Self::RuntimeContextArithmetic,
            vega_runtime::VegaError::ProviderPayloadTooLarge { .. } => Self::ProviderProtocol,
            vega_runtime::VegaError::ProviderDiagnostic {
                kind: vega_runtime::ProviderFailureKind::OutputLimit,
                ..
            } => Self::ProviderOutputLimit,
            vega_runtime::VegaError::ProviderDiagnostic {
                kind: vega_runtime::ProviderFailureKind::Protocol,
                ..
            } => Self::ProviderProtocol,
            // Dispatch on the structured kind before any status-based arm: an
            // established response that broke mid-stream can carry a 2xx
            // status and must not be reported as an HTTP rejection.
            vega_runtime::VegaError::ProviderDiagnostic {
                kind: vega_runtime::ProviderFailureKind::Transport,
                status,
                retry_count,
                ..
            } => Self::ProviderTransport(ProviderFailureFact {
                http_status: *status,
                retries: *retry_count,
            }),
            vega_runtime::VegaError::ProviderDiagnostic {
                kind: vega_runtime::ProviderFailureKind::StreamInterrupted,
                status,
                retry_count,
                ..
            } => Self::ProviderStreamInterrupted(ProviderFailureFact {
                http_status: *status,
                retries: *retry_count,
            }),
            vega_runtime::VegaError::ProviderDiagnostic {
                kind: vega_runtime::ProviderFailureKind::Timeout,
                status,
                retry_count,
                ..
            } => Self::ProviderTimeout(ProviderFailureFact {
                http_status: *status,
                retries: *retry_count,
            }),
            vega_runtime::VegaError::ProviderDiagnostic {
                kind: vega_runtime::ProviderFailureKind::Overloaded,
                status,
                retry_count,
                ..
            } => Self::ProviderOverloaded(ProviderFailureFact {
                http_status: *status,
                retries: *retry_count,
            }),
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
            vega_runtime::VegaError::Provider { status: None, .. } => Self::ProviderUnknown,
            vega_runtime::VegaError::ProviderDiagnostic {
                status: Some(400),
                message,
                ..
            } if provider_has_quota_code(message) => Self::ProviderQuota,
            vega_runtime::VegaError::ProviderDiagnostic {
                status: Some(401 | 403),
                ..
            } => Self::ProviderAuthorization,
            vega_runtime::VegaError::ProviderDiagnostic {
                status: Some(429), ..
            } => Self::ProviderRateLimited,
            vega_runtime::VegaError::ProviderDiagnostic {
                status: Some(status),
                ..
            } => Self::ProviderHttp(*status),
            vega_runtime::VegaError::ProviderDiagnostic {
                kind: vega_runtime::ProviderFailureKind::Rejected,
                ..
            } => Self::ProviderRejected,
            vega_runtime::VegaError::CredentialExposureBlocked => Self::CredentialExposureBlocked,
            vega_runtime::VegaError::CredentialCheckUnavailable => Self::CredentialCheckUnavailable,
            vega_runtime::VegaError::ProjectInstructions(error)
                if error.failure() == vega_tools::ProjectInstructionsFailure::InvalidRootPath =>
            {
                Self::ProjectPathInvalid
            }
            vega_runtime::VegaError::ProjectInstructions(_) => Self::ProjectInstructionsUnavailable,
            vega_runtime::VegaError::CredentialContinuationUnavailable => {
                Self::CredentialContinuationUnavailable
            }
            vega_runtime::VegaError::CredentialProjectionInvalid { tool_index } => {
                Self::CredentialProjectionInvalid(*tool_index)
            }
            vega_runtime::VegaError::Context(
                vega_runtime::ContextRuntimeError::OverLimit { .. }
                | vega_runtime::ContextRuntimeError::SummaryInputOverLimit { .. }
                | vega_runtime::ContextRuntimeError::ResultOverLimit { .. },
            ) => Self::ContextOverLimit,
            vega_runtime::VegaError::Context(vega_runtime::ContextRuntimeError::SourceChanged) => {
                Self::Compaction(ContextCompactionFailureCode::SourceChanged)
            }
            vega_runtime::VegaError::Context(
                vega_runtime::ContextRuntimeError::SummaryTimedOut,
            ) => Self::Compaction(ContextCompactionFailureCode::TimedOut),
            vega_runtime::VegaError::Context(
                vega_runtime::ContextRuntimeError::SummaryOutputTruncated { .. }
                | vega_runtime::ContextRuntimeError::SummaryEmpty
                | vega_runtime::ContextRuntimeError::InvalidSummary
                | vega_runtime::ContextRuntimeError::SummaryFormatInvalid
                | vega_runtime::ContextRuntimeError::SummaryProjectionInvalid
                | vega_runtime::ContextRuntimeError::InvalidProjection
                | vega_runtime::ContextRuntimeError::SystemMessageInResult,
            ) => Self::Compaction(ContextCompactionFailureCode::InvalidSummary),
            vega_runtime::VegaError::Context(
                vega_runtime::ContextRuntimeError::NoCompactablePrefix,
            ) => Self::Compaction(ContextCompactionFailureCode::NoCompactablePrefix),
            vega_runtime::VegaError::Context(
                vega_runtime::ContextRuntimeError::SourceTooLarge
                | vega_runtime::ContextRuntimeError::AggregateTooLarge,
            ) => Self::Compaction(ContextCompactionFailureCode::TooLarge),
            vega_runtime::VegaError::Context(
                vega_runtime::ContextRuntimeError::ImagesUnsupported,
            ) => Self::Compaction(ContextCompactionFailureCode::ImagesUnsupported),
            vega_runtime::VegaError::Context(
                vega_runtime::ContextRuntimeError::AlreadyAttempted,
            ) => Self::Compaction(ContextCompactionFailureCode::Unavailable),
            vega_runtime::VegaError::Context(vega_runtime::ContextRuntimeError::Estimate(
                vega_runtime::ContextEstimateError::InputTooLarge,
            )) => Self::ContextOverLimit,
            vega_runtime::VegaError::ReasoningBudgetExceeded { scope, .. }
            | vega_runtime::VegaError::ReasoningReplayBudgetExceeded { scope, .. } => {
                Self::ReasoningLimit(Some(*scope))
            }
            vega_runtime::VegaError::ModelImagesUnsupported => Self::ModelImagesUnsupported,
            vega_runtime::VegaError::ReasoningSelectionInvalid { .. } => {
                Self::ReasoningSelectionInvalid
            }
            vega_runtime::VegaError::ReasoningReplayUnsupported
            | vega_runtime::VegaError::ReasoningReplayIncomplete => Self::ReasoningReplay,
            _ => Self::Runtime,
        }
    }

    /// Actionable text made entirely from local vocabulary and HTTP metadata.
    pub fn message(self) -> String {
        match self {
            Self::ProviderUnknown => vega_i18n::t("domain.failure.provider_unknown").into(),
            Self::ProviderProtocol => vega_i18n::t("domain.failure.provider_protocol").into(),
            Self::ProviderRejected => vega_i18n::t("domain.failure.provider_rejected").into(),
            Self::ProviderOutputLimit => {
                vega_i18n::t("domain.failure.provider_output_limit").into()
            }
            Self::ProviderQuota => vega_i18n::t("domain.failure.provider_quota").into(),
            Self::ProviderAuthorization => {
                vega_i18n::t("domain.failure.provider_authorization").into()
            }
            Self::ProviderRateLimited => vega_i18n::t("domain.failure.provider_rate_limit").into(),
            Self::ProviderHttp(status) => vega_i18n::t_args(
                "domain.failure.provider_http",
                &[("status", &status.to_string())],
            ),
            Self::ProviderTransport(fact) => {
                provider_failure_message("domain.failure.provider_transport", fact)
            }
            Self::ProviderStreamInterrupted(fact) => {
                provider_failure_message("domain.failure.provider_stream_interrupted", fact)
            }
            Self::ProviderTimeout(fact) => {
                provider_failure_message("domain.failure.provider_timeout", fact)
            }
            Self::ProviderOverloaded(fact) => {
                provider_failure_message("domain.failure.provider_overloaded", fact)
            }
            Self::CredentialExposureBlocked => {
                vega_i18n::t("domain.failure.credential_block").into()
            }
            Self::CredentialCheckUnavailable => {
                vega_i18n::t("domain.failure.credential_check").into()
            }
            Self::ProjectInstructionsUnavailable => {
                vega_i18n::t("domain.failure.project_instructions").into()
            }
            Self::ProjectPathInvalid => vega_i18n::t("domain.failure.project_path").into(),
            Self::CredentialProjectionInvalid(index) => vega_i18n::t_args(
                "domain.failure.credential_projection",
                &[("index", &(index + 1).to_string())],
            ),
            Self::CredentialOutcomeUnknown => {
                vega_i18n::t("domain.failure.credential_outcome").into()
            }
            Self::CredentialContinuationUnavailable => {
                vega_i18n::t("domain.failure.credential_continuation").into()
            }
            Self::ContextOverLimit => vega_i18n::t("domain.failure.context_limit").into(),
            Self::Compaction(failure) => failure.message().into(),
            Self::ReasoningLimit(_) => vega_i18n::t("domain.failure.reasoning_limit").into(),
            Self::ModelImagesUnsupported => vega_i18n::t("domain.failure.model_images").into(),
            Self::ReasoningSelectionInvalid => {
                vega_i18n::t("domain.failure.reasoning_selection").into()
            }
            Self::ReasoningReplay => vega_i18n::t("domain.failure.reasoning_replay").into(),
            Self::Runtime => vega_i18n::t("domain.failure.runtime").into(),
            Self::RuntimeStorage => vega_i18n::t("domain.failure.storage").into(),
            Self::RuntimeStorageBusy => vega_i18n::t("domain.failure.storage_busy").into(),
            Self::RuntimeApprovalAudit => vega_i18n::t("domain.failure.audit").into(),
            Self::RuntimeState => vega_i18n::t("domain.failure.state").into(),
            Self::RuntimeIo => vega_i18n::t("domain.failure.io").into(),
            Self::RuntimePipeline => vega_i18n::t("domain.failure.pipeline").into(),
            Self::RuntimeReferences => vega_i18n::t("domain.failure.references").into(),
            Self::RuntimeCapability => vega_i18n::t("domain.failure.capability").into(),
            Self::RuntimeContextBudget => vega_i18n::t("domain.failure.context_budget").into(),
            Self::RuntimeContextArithmetic => {
                vega_i18n::t("domain.failure.context_arithmetic").into()
            }
            Self::Persisted => vega_i18n::t("domain.failure.persisted").into(),
        }
    }
}

/// Joins the localized failure advice with the facts that were actually
/// recorded, so the user can check the claim against the run instead of being
/// told to fix a network that never failed.
fn provider_failure_message(key: &'static str, fact: ProviderFailureFact) -> String {
    let advice = vega_i18n::t(key);
    match fact.summary() {
        Some(facts) => format!("{advice} ({facts})"),
        None => advice.into(),
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

pub(crate) fn unix_now_ms() -> i64 {
    now_unix_millis()
}

pub(crate) fn valid_message_timestamp(value: Option<i64>, now_ms: i64) -> Option<i64> {
    value.filter(|value| *value > 0 && *value <= now_ms && *value <= 253_402_300_799_999)
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
    UserMessageAccepted {
        message_id: MessageId,
        seq: u64,
    },
    TimestampBreakBefore {
        message_id: String,
    },
    MessageTimestamp {
        message_id: String,
        sent_at_ms: Option<i64>,
    },
    AssistantResponseStarted {
        message_id: MessageId,
        response_id: String,
        text_start_bytes: usize,
    },
    ReasoningCompleted {
        message_id: MessageId,
        response_id: String,
    },
    ResponseRetrying {
        message_id: MessageId,
        text_start_bytes: usize,
        attempt: u32,
        max_retries: u32,
    },
    PendingMessageDelivered {
        pending_id: String,
        message_id: String,
        seq: u64,
        content: String,
        images: Vec<ImageAttachment>,
    },
    ScreenshotDelivered {
        message_id: MessageId,
        call_id: CallId,
        image: super::ImageAttachment,
    },
    RunStarted {
        message_id: MessageId,
        started_at: std::time::Instant,
        work_clock: Option<super::WorkClock>,
    },
    RunContinued {
        previous_assistant_id: MessageId,
        user_message_id: MessageId,
        next_assistant_id: MessageId,
    },
    AssistantResponseCompleted {
        message_id: MessageId,
        response: AssistantResponseMetadata,
    },
    GeneratedNotice {
        message_id: MessageId,
        notice: super::GeneratedNoticeMetadata,
    },
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
    SummaryDelta {
        message_id: MessageId,
        delta: String,
    },
    /// One persisted activation receipt, never Skill body or file path.
    SkillActivated {
        message_id: MessageId,
        skill: ActiveSkillView,
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
    /// Tool execution began after durable approval.
    ToolCallRunning {
        /// Provider call id; no arguments or output cross this boundary.
        call_id: CallId,
        bash_started_at: Option<vega_runtime::BashStartClock>,
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
        cost: Microcents,
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
    /// Primary context accounting for the active assistant run only.
    ContextAccounting {
        message_id: MessageId,
        record: ContextAccountingRecord,
    },
    /// Assistant message converged.
    MessageFinished {
        /// Assistant message id.
        message_id: MessageId,
        /// Convergence reason.
        stop_reason: ConversationStopReason,
        execution_duration_ms: Option<u64>,
    },
    /// Runtime/provider error.
    Error {
        /// Assistant message id, when a message had started.
        message_id: Option<MessageId>,
        /// Safe display error.
        error: Arc<vega_runtime::VegaError>,
        execution_duration_ms: Option<u64>,
    },
    /// Cancellation was observed.
    Interrupted {
        /// Interrupted assistant message id.
        message_id: MessageId,
        execution_duration_ms: Option<u64>,
    },
}

impl std::fmt::Debug for ConversationEvent {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UserMessageAccepted { message_id, seq } => formatter
                .debug_struct("UserMessageAccepted")
                .field("message_id_bytes", &message_id.len())
                .field("seq", seq)
                .finish(),
            Self::TimestampBreakBefore { message_id } => formatter
                .debug_struct("TimestampBreakBefore")
                .field("message_id_bytes", &message_id.len())
                .finish(),
            Self::MessageTimestamp {
                message_id,
                sent_at_ms,
            } => formatter
                .debug_struct("MessageTimestamp")
                .field("message_id_bytes", &message_id.len())
                .field("sent_at_ms", sent_at_ms)
                .finish(),
            Self::AssistantResponseStarted {
                response_id,
                text_start_bytes,
                ..
            } => formatter
                .debug_struct("AssistantResponseStarted")
                .field("response_id_bytes", &response_id.len())
                .field("text_start_bytes", text_start_bytes)
                .finish_non_exhaustive(),
            Self::ReasoningCompleted {
                message_id,
                response_id,
            } => formatter
                .debug_struct("ReasoningCompleted")
                .field("message_id_bytes", &message_id.len())
                .field("response_id_bytes", &response_id.len())
                .finish(),
            Self::ResponseRetrying {
                text_start_bytes,
                attempt,
                max_retries,
                ..
            } => formatter
                .debug_struct("ResponseRetrying")
                .field("text_start_bytes", text_start_bytes)
                .field("attempt", attempt)
                .field("max_retries", max_retries)
                .finish_non_exhaustive(),
            Self::PendingMessageDelivered { .. } => formatter.write_str("PendingMessageDelivered"),
            Self::ScreenshotDelivered { image, .. } => formatter
                .debug_struct("ScreenshotDelivered")
                .field("image_bytes", &image.bytes().len())
                .finish_non_exhaustive(),
            Self::RunContinued { .. } => formatter.write_str("RunContinued"),
            Self::RunStarted { message_id, .. } => formatter
                .debug_struct("RunStarted")
                .field("message_id_bytes", &message_id.len())
                .finish(),
            Self::AssistantResponseCompleted {
                message_id,
                response,
            } => formatter
                .debug_struct("AssistantResponseCompleted")
                .field("message_id_bytes", &message_id.len())
                .field("response", response)
                .finish(),
            Self::GeneratedNotice { message_id, notice } => formatter
                .debug_struct("GeneratedNotice")
                .field("message_id_bytes", &message_id.len())
                .field("notice", notice)
                .finish(),
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
            Self::SummaryDelta { message_id, delta } => formatter
                .debug_struct("SummaryDelta")
                .field("message_id_bytes", &message_id.len())
                .field("delta_bytes", &delta.len())
                .finish(),
            Self::SkillActivated { message_id, skill } => formatter
                .debug_struct("SkillActivated")
                .field("message_id_bytes", &message_id.len())
                .field("name", &skill.name)
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
            Self::ToolCallRunning { call_id, .. } => formatter
                .debug_struct("ToolCallRunning")
                .field("call_id_bytes", &call_id.len())
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
            Self::ContextAccounting { message_id, record } => formatter
                .debug_struct("ContextAccounting")
                .field("message_id_bytes", &message_id.len())
                .field("record", record)
                .finish(),
            Self::MessageFinished {
                message_id,
                stop_reason,
                execution_duration_ms,
            } => formatter
                .debug_struct("MessageFinished")
                .field("message_id_bytes", &message_id.len())
                .field("stop_reason", stop_reason)
                .field("execution_duration_ms", execution_duration_ms)
                .finish(),
            Self::Error {
                message_id,
                error: _,
                execution_duration_ms,
            } => formatter
                .debug_struct("Error")
                .field("message_id_bytes", &message_id.as_ref().map(String::len))
                .field("execution_duration_ms", execution_duration_ms)
                .finish(),
            Self::Interrupted {
                message_id,
                execution_duration_ms,
            } => formatter
                .debug_struct("Interrupted")
                .field("message_id_bytes", &message_id.len())
                .field("execution_duration_ms", execution_duration_ms)
                .finish(),
        }
    }
}

/// Converts one headless runtime event into the shared conversation event.
pub(crate) fn from_runtime_event(
    message_id: &str,
    event: &vega_runtime::RuntimeEvent,
) -> Option<ConversationEvent> {
    use vega_runtime::{RuntimeEvent, RuntimeFinishReason, RuntimeToolStatus};

    match event {
        RuntimeEvent::AssistantResponseStarted {
            response_id,
            text_start_bytes,
        } => Some(ConversationEvent::AssistantResponseStarted {
            message_id: message_id.into(),
            response_id: response_id.clone(),
            text_start_bytes: *text_start_bytes,
        }),
        RuntimeEvent::ReasoningCompleted { response_id } => {
            Some(ConversationEvent::ReasoningCompleted {
                message_id: message_id.into(),
                response_id: response_id.clone(),
            })
        }
        RuntimeEvent::ResponseRetrying {
            text_start_bytes,
            attempt,
            max_retries,
        } => Some(ConversationEvent::ResponseRetrying {
            message_id: message_id.into(),
            text_start_bytes: *text_start_bytes,
            attempt: *attempt,
            max_retries: *max_retries,
        }),
        RuntimeEvent::SteeringAccepted(_)
        | RuntimeEvent::ToolRoundCompleted { .. }
        | RuntimeEvent::DiagnosticAttempt(_)
        | RuntimeEvent::ReasoningRetentionReleased
        | RuntimeEvent::BuiltinSkillActivated(_) => None,
        RuntimeEvent::ScreenshotDelivered { call_id, image, .. } => {
            Some(ConversationEvent::ScreenshotDelivered {
                message_id: message_id.into(),
                call_id: call_id.clone(),
                image: image.clone(),
            })
        }
        RuntimeEvent::AssistantResponseCompleted {
            response_id,
            text_start_bytes,
            text_end_bytes,
            kind,
        } => Some(ConversationEvent::AssistantResponseCompleted {
            message_id: message_id.to_string(),
            response: AssistantResponseMetadata {
                response_id: response_id.clone(),
                text_start_bytes: *text_start_bytes,
                text_end_bytes: *text_end_bytes,
                kind: match kind {
                    vega_runtime::RuntimeAssistantResponseKind::Commentary => {
                        AssistantResponseKind::Commentary
                    }
                    vega_runtime::RuntimeAssistantResponseKind::Final => {
                        AssistantResponseKind::Final
                    }
                },
            },
        }),
        RuntimeEvent::GeneratedNotice {
            kind,
            text_start_bytes,
            text_end_bytes,
        } => Some(ConversationEvent::GeneratedNotice {
            message_id: message_id.to_string(),
            notice: super::GeneratedNoticeMetadata::from_runtime(
                *kind,
                *text_start_bytes,
                *text_end_bytes,
            ),
        }),
        RuntimeEvent::TextDelta(delta) => Some(ConversationEvent::TextDelta {
            message_id: message_id.to_string(),
            delta: delta.clone(),
        }),
        RuntimeEvent::ThinkingDelta(delta) => Some(ConversationEvent::ThinkingDelta {
            message_id: message_id.to_string(),
            delta: delta.clone(),
        }),
        RuntimeEvent::SummaryDelta(delta) => Some(ConversationEvent::SummaryDelta {
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
                    codex_display: None,
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
                    codex_display: None,
                },
            })
        }
        RuntimeEvent::ToolCallApproved { call_id, audit, .. } => {
            Some(ConversationEvent::ToolCallApproved {
                call_id: call_id.clone(),
                approval: approval_from_runtime(audit.decision),
            })
        }
        RuntimeEvent::ToolCallRunning {
            call_id,
            bash_started_at,
        } => Some(ConversationEvent::ToolCallRunning {
            call_id: call_id.clone(),
            bash_started_at: bash_started_at.clone(),
        }),
        RuntimeEvent::ToolCallOutput { call_id, chunk } => {
            Some(ConversationEvent::ToolCallOutput {
                call_id: call_id.clone(),
                chunk: ToolOutputChunk(chunk.clone()),
            })
        }
        RuntimeEvent::ToolCallFinished(result) | RuntimeEvent::ToolReadFinished { result, .. } => {
            Some(ConversationEvent::ToolCallFinished {
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
                    codex_display: None,
                },
            })
        }
        RuntimeEvent::SkillActivation { audit, .. } if audit.status == "loaded" => {
            Some(ConversationEvent::SkillActivated {
                message_id: message_id.to_string(),
                skill: ActiveSkillView {
                    name: audit.name.clone(),
                    source_label: audit.source_label.clone()?,
                    content_sha256: audit.content_sha256.clone()?,
                },
            })
        }
        RuntimeEvent::SkillActivation { .. } | RuntimeEvent::SkillSnapshot { .. } => None,
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
        RuntimeEvent::ContextAccountingUpdated(decision) => {
            Some(ConversationEvent::ContextAccounting {
                message_id: message_id.to_string(),
                record: ContextAccountingRecord {
                    source: match decision.source {
                        vega_runtime::ContextAccountingSource::Estimated => {
                            ContextAccountingSource::Estimated
                        }
                        vega_runtime::ContextAccountingSource::UsageAnchored => {
                            ContextAccountingSource::UsageAnchored
                        }
                    },
                    stage: match decision.stage {
                        vega_runtime::ContextAccountingStage::PrimaryPreflight => {
                            ContextAccountingStage::PrimaryPreflight
                        }
                        vega_runtime::ContextAccountingStage::SkillProspect => {
                            ContextAccountingStage::SkillProspect
                        }
                        vega_runtime::ContextAccountingStage::PostSummaryTarget => {
                            ContextAccountingStage::PostSummaryTarget
                        }
                    },
                    provider_input_baseline: decision.provider_input_baseline,
                    incremental_estimate: decision.incremental_estimate,
                    predicted_input: decision.predicted_input,
                    input_budget: decision.input_budget,
                    trigger_tokens: decision.trigger_tokens,
                    target_tokens: decision.target_tokens,
                    revision: decision.revision,
                    covered_messages: decision.covered_messages,
                },
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
                Some(vega_runtime::ContextCompactionStatusFailure::TimedOut) => {
                    Some(ContextCompactionFailureCode::TimedOut)
                }
                Some(vega_runtime::ContextCompactionStatusFailure::ImagesUnsupported) => {
                    Some(ContextCompactionFailureCode::ImagesUnsupported)
                }
                Some(vega_runtime::ContextCompactionStatusFailure::OverLimit) => {
                    Some(ContextCompactionFailureCode::OverLimit)
                }
                Some(vega_runtime::ContextCompactionStatusFailure::ReasoningLimit) => {
                    Some(ContextCompactionFailureCode::ReasoningLimit)
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
        RuntimeEvent::StageHandoff { .. } => None,
        RuntimeEvent::Finished(reason) => Some(ConversationEvent::MessageFinished {
            message_id: message_id.to_string(),
            execution_duration_ms: None,
            stop_reason: match reason {
                RuntimeFinishReason::End => ConversationStopReason::End,
                RuntimeFinishReason::Length => ConversationStopReason::Length,
                RuntimeFinishReason::TurnLimit => ConversationStopReason::TurnLimit,
            },
        }),
        RuntimeEvent::Interrupted => Some(ConversationEvent::Interrupted {
            message_id: message_id.to_string(),
            execution_duration_ms: None,
        }),
        RuntimeEvent::Error(error) => Some(ConversationEvent::Error {
            message_id: Some(message_id.to_string()),
            error: error.clone(),
            execution_duration_ms: None,
        }),
    }
}

pub(crate) fn safe_runtime_tool_call(call: &vega_runtime::RuntimeToolCall) -> Option<ToolCall> {
    let projected = ToolCall {
        id: call.id.clone(),
        tool: call.name.clone(),
        input_json: call.input_json.clone(),
        description: call.description.clone(),
        codex_display: None,
    };
    if call.name == "read_conversation" {
        return crate::agent::valid_history_call_projection(&call.input_json).then_some(projected);
    }
    if vega_runtime::is_subagent_tool(&call.name) {
        return (call.input_json == "{}"
            || vega_runtime::valid_subagent_input(&call.name, &call.input_json))
        .then_some(projected);
    }
    if call.name == vega_runtime::USER_QUESTION_TOOL {
        let valid = vega_runtime::UserQuestionInput::from_json(&call.input_json).is_ok()
            || vega_runtime::user_question_validation_error(&call.input_json).is_some();
        return valid.then_some(projected);
    }
    if matches!(call.name.as_str(), "load_skill" | "read_skill_resource") {
        return crate::agent::valid_skill_call_projection(&call.name, &call.input_json)
            .then_some(projected);
    }
    if vega_runtime::computer_use::ComputerUseRun::tool_capability(&call.name).is_some() {
        return vega_runtime::computer_use::ComputerUseAudit::valid(&call.input_json)
            .then_some(projected);
    }
    if matches!(
        call.name.as_str(),
        "get_goal" | "update_goal" | "schedule_wakeup"
    ) {
        return crate::agent::valid_automation_call_projection(&call.name, &call.input_json)
            .then_some(projected);
    }
    if call.name.starts_with("mcp_") {
        // The runtime has already replaced raw MCP arguments with this
        // value-free identity. Keep its exact alias binding at the live UI
        // boundary; the generic unknown-tool rule below would drop every
        // valid MCP proposal before the approval card can appear.
        return McpCallIdentity::from_tool_call(&projected).map(|_| projected);
    }
    if matches!(call.name.as_str(), "Write" | "Edit" | "write" | "edit") {
        let audit = vega_tools::WriteEditAudit::from_json(&call.input_json).ok()?;
        if !audit.tool().as_str().eq_ignore_ascii_case(&call.name) {
            return None;
        }
    } else if !matches!(
        call.name.as_str(),
        "Read" | "read" | "glob" | "grep" | "bash"
    ) && call.input_json != "{}"
    {
        return None;
    }
    Some(projected)
}

pub(crate) fn validate_runtime_validation_rejection(
    call: &vega_runtime::RuntimeToolCall,
    result: &vega_runtime::RuntimeToolResult,
) -> Option<InvalidToolProjection> {
    if call.name == "bash" {
        let approval = result.approval.as_ref()?;
        return (vega_runtime::InvalidBashAudit::from_json(&call.input_json).is_some()
            && call.id == result.call_id
            && result.status == vega_runtime::RuntimeToolStatus::Rejected
            && vega_runtime::InvalidBashAudit::from_json(&call.input_json)
                .is_some_and(|audit| audit.matches_feedback(&result.output))
            && result.exit_code.is_none()
            && result.duration_ms.is_none()
            && result.truncated.is_none()
            && result.remember_rule.is_none()
            && approval.decision == vega_runtime::RuntimeApprovalDecision::Deny
            && approval.source == vega_runtime::RuntimeApprovalSource::Validation
            && approval.note.is_none()
            && approval.danger.is_none())
        .then_some(InvalidToolProjection::new(
            InvalidToolKind::Bash,
            InvalidToolCode::InvalidInput,
        ));
    }
    let audit = vega_tools::InvalidWriteEditAudit::from_json(&call.input_json).ok()?;
    let approval = result.approval.as_ref()?;
    let expected = format!(
        "Tool error: invalid {} input ({})",
        call.name,
        audit.validation_error_code().as_str()
    );
    if audit.tool().as_str().eq_ignore_ascii_case(&call.name)
        && call.id == result.call_id
        && result.status == vega_runtime::RuntimeToolStatus::Rejected
        && (result.output == expected
            || audit
                .validation_error_code()
                .matches_validation_result(audit.tool().as_str(), &result.output))
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
        Code::FileNotRead => InvalidToolCode::FileNotRead,
        Code::FileTooLarge => InvalidToolCode::FileTooLarge,
        Code::EditNoChange => InvalidToolCode::EditNoChange,
        Code::WrongReplaceAllType => InvalidToolCode::WrongReplaceAllType,
        Code::InvalidEdits => InvalidToolCode::InvalidEdits,
        Code::InvalidEditItem => InvalidToolCode::InvalidEditItem,
        Code::EditOverlap => InvalidToolCode::EditOverlap,
        Code::UnsupportedEncoding => InvalidToolCode::UnsupportedEncoding,
        Code::EditNoMatch => InvalidToolCode::EditNoMatch,
        Code::EditMultipleMatches => InvalidToolCode::EditMultipleMatches,
        Code::TargetChanged => InvalidToolCode::TargetChanged,
        Code::CheckpointExists
        | Code::CheckpointMetadataInvalid
        | Code::AtomicWriteFailed
        | Code::CodecInvalid
        | Code::PreparedScopeMismatch => return None,
    })
}
