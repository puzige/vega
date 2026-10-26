use std::fmt;

use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeFailureStage {
    RunPreparation,
    Snapshot,
    ToolProposal,
    ToolApproval,
    ToolRunning,
    ToolTerminal,
    EventPipeline,
}

impl RuntimeFailureStage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RunPreparation => "run_preparation",
            Self::Snapshot => "snapshot",
            Self::ToolProposal => "tool_proposal",
            Self::ToolApproval => "tool_approval",
            Self::ToolRunning => "tool_running",
            Self::ToolTerminal => "tool_terminal",
            Self::EventPipeline => "event_pipeline",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeFailureReason {
    Database,
    ApprovalAudit,
    State,
    Io,
    Pipeline,
}

impl RuntimeFailureReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Database => "database",
            Self::ApprovalAudit => "approval_audit",
            Self::State => "state",
            Self::Io => "io",
            Self::Pipeline => "pipeline",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeIoKind {
    NotFound,
    PermissionDenied,
    AlreadyExists,
    InvalidInput,
    InvalidData,
    TimedOut,
    Interrupted,
    Unsupported,
    OutOfMemory,
    StorageFull,
    Other,
}

impl RuntimeIoKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotFound => "not_found",
            Self::PermissionDenied => "permission_denied",
            Self::AlreadyExists => "already_exists",
            Self::InvalidInput => "invalid_input",
            Self::InvalidData => "invalid_data",
            Self::TimedOut => "timed_out",
            Self::Interrupted => "interrupted",
            Self::Unsupported => "unsupported",
            Self::OutOfMemory => "out_of_memory",
            Self::StorageFull => "storage_full",
            Self::Other => "other",
        }
    }
}

impl From<std::io::ErrorKind> for RuntimeIoKind {
    fn from(kind: std::io::ErrorKind) -> Self {
        match kind {
            std::io::ErrorKind::NotFound => Self::NotFound,
            std::io::ErrorKind::PermissionDenied => Self::PermissionDenied,
            std::io::ErrorKind::AlreadyExists => Self::AlreadyExists,
            std::io::ErrorKind::InvalidInput => Self::InvalidInput,
            std::io::ErrorKind::InvalidData => Self::InvalidData,
            std::io::ErrorKind::TimedOut => Self::TimedOut,
            std::io::ErrorKind::Interrupted => Self::Interrupted,
            std::io::ErrorKind::Unsupported => Self::Unsupported,
            std::io::ErrorKind::OutOfMemory => Self::OutOfMemory,
            std::io::ErrorKind::StorageFull => Self::StorageFull,
            _ => Self::Other,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeFailureDetail {
    pub stage: RuntimeFailureStage,
    pub reason: RuntimeFailureReason,
    pub sqlite_code: Option<i32>,
    pub sqlite_extended_code: Option<i32>,
    pub io_kind: Option<RuntimeIoKind>,
}

impl RuntimeFailureDetail {
    pub fn new(reason: RuntimeFailureReason) -> Self {
        Self {
            stage: RuntimeFailureStage::EventPipeline,
            reason,
            sqlite_code: None,
            sqlite_extended_code: None,
            io_kind: None,
        }
    }
}

/// Unified Vega error (tech-spec §7).
///
/// `Send + Sync` so it can cross await points and thread boundaries inside
/// the headless runtime and into the conversation/UI layers. Provider errors
/// never carry the API key (red line: keys never reach logs or errors).
#[derive(Error)]
pub enum VegaError {
    #[error("runtime operation failed")]
    RuntimeFailure(RuntimeFailureDetail),
    #[error("provider frame exceeded parser capacity")]
    ProviderPayloadTooLarge {
        observed_bytes: usize,
        limit_bytes: usize,
    },
    /// Provider (LLM API) failure. `status` is the HTTP status code when the
    /// failure came from an HTTP response (`None` for transport-level or
    /// protocol-level failures); `retryable` hints whether an automatic
    /// retry may succeed (`false` once retries are exhausted).
    #[error("provider error (status={status:?}, retryable={retryable})")]
    Provider {
        status: Option<u16>,
        /// Raw provider diagnostic for typed internal handling only. Default
        /// `Debug`/`Display` intentionally never renders this payload.
        message: String,
        retryable: bool,
    },
    #[error("provider error (status={status:?}, retryable={retryable})")]
    ProviderDiagnostic {
        kind: crate::provider::ProviderFailureKind,
        status: Option<u16>,
        message: String,
        retryable: bool,
        retry_count: Option<u32>,
        request_id: Option<String>,
    },
    /// Filesystem / local IO failure.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// SQLite store failure (tech-spec §2 store layer).
    #[error(transparent)]
    Store(#[from] rusqlite::Error),
    /// Built-in tool failure; surfaces on the tool card without aborting the
    /// session (tech-spec §7 UI presentation).
    #[error("tool '{tool}' failed: {message}")]
    Tool { tool: String, message: String },
    /// Operation was cancelled through its `CancellationToken`.
    #[error("operation cancelled")]
    Cancelled,
    #[error("local credential safety block")]
    CredentialExposureBlocked,
    #[error("credential safety check unavailable")]
    CredentialCheckUnavailable,
    #[error(transparent)]
    ProjectInstructions(#[from] vega_tools::ProjectInstructionsError),
    #[error("credential continuation is unavailable")]
    CredentialContinuationUnavailable,
    #[error("credential projection requires tool configuration repair ({tool_index})")]
    CredentialProjectionInvalid { tool_index: usize },
    #[error(
        "this model returned reasoning state that cannot be replayed; retry or start a new conversation"
    )]
    ReasoningReplayUnsupported,
    #[error(
        "the model response omitted required reasoning replay state; retry or start a new conversation"
    )]
    ReasoningReplayIncomplete,
    #[error(
        "this model does not accept images; select a vision model or enable image input in model settings"
    )]
    ModelImagesUnsupported,
    /// A frozen provider/model thinking selection violated its explicit
    /// capability declaration before any request was sent.
    #[error("reasoning selection is invalid")]
    ReasoningSelectionInvalid {
        /// Content-safe internal diagnostic; `Display`/`Debug` omit it.
        message: String,
    },
    /// The provider streamed more reasoning content than the bounded runtime
    /// contract can retain and replay safely.
    #[error("reasoning content exceeded the {scope:?} budget of {limit_bytes} bytes")]
    ReasoningBudgetExceeded {
        /// Which bound was exceeded.
        scope: crate::provider::ReasoningBudgetScope,
        /// Frozen limit in UTF-8 bytes.
        limit_bytes: usize,
        /// Observed count at failure (metadata only).
        observed_bytes: usize,
    },
    #[allow(missing_docs)]
    #[error("reasoning replay exceeded the {scope:?} retention budget")]
    ReasoningReplayBudgetExceeded {
        scope: crate::provider::ReasoningBudgetScope,
        limit_bytes: usize,
        observed_bytes: usize,
        limit_items: usize,
        observed_items: usize,
    },
    /// Context budget or compaction hook failure.  The nested type is
    /// metadata-only and deliberately excludes historical prompt content.
    #[error(transparent)]
    Context(#[from] crate::ContextRuntimeError),
}

impl fmt::Debug for VegaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RuntimeFailure(detail) => formatter
                .debug_tuple("RuntimeFailure")
                .field(detail)
                .finish(),
            Self::ProviderPayloadTooLarge {
                observed_bytes,
                limit_bytes,
            } => formatter
                .debug_struct("ProviderPayloadTooLarge")
                .field("observed_bytes", observed_bytes)
                .field("limit_bytes", limit_bytes)
                .finish(),
            Self::Provider {
                status,
                message,
                retryable,
            } => formatter
                .debug_struct("Provider")
                .field("status", status)
                .field("message_bytes", &message.len())
                .field("retryable", retryable)
                .finish(),
            Self::ProviderDiagnostic {
                kind,
                status,
                message,
                retryable,
                retry_count,
                request_id,
            } => formatter
                .debug_struct("ProviderDiagnostic")
                .field("kind", kind)
                .field("status", status)
                .field("message_bytes", &message.len())
                .field("retryable", retryable)
                .field("retry_count", retry_count)
                .field("has_request_id", &request_id.is_some())
                .finish(),
            Self::Io(error) => formatter
                .debug_struct("Io")
                .field("kind", &error.kind())
                .finish(),
            Self::Store(_) => formatter.write_str("Store([redacted])"),
            Self::Tool { tool, message } => formatter
                .debug_struct("Tool")
                .field("tool_bytes", &tool.len())
                .field("message_bytes", &message.len())
                .finish(),
            Self::Cancelled => formatter.write_str("Cancelled"),
            Self::CredentialExposureBlocked => formatter.write_str("CredentialExposureBlocked"),
            Self::CredentialCheckUnavailable => formatter.write_str("CredentialCheckUnavailable"),
            Self::ProjectInstructions(error) => formatter
                .debug_tuple("ProjectInstructions")
                .field(error)
                .finish(),
            Self::CredentialContinuationUnavailable => {
                formatter.write_str("CredentialContinuationUnavailable")
            }
            Self::CredentialProjectionInvalid { tool_index } => formatter
                .debug_struct("CredentialProjectionInvalid")
                .field("tool_index", tool_index)
                .finish(),
            Self::ReasoningReplayUnsupported => formatter.write_str("ReasoningReplayUnsupported"),
            Self::ReasoningReplayIncomplete => formatter.write_str("ReasoningReplayIncomplete"),
            Self::ModelImagesUnsupported => formatter.write_str("ModelImagesUnsupported"),
            Self::ReasoningSelectionInvalid { message } => formatter
                .debug_struct("ReasoningSelectionInvalid")
                .field("message_bytes", &message.len())
                .finish(),
            Self::ReasoningBudgetExceeded {
                scope,
                limit_bytes,
                observed_bytes,
            } => formatter
                .debug_struct("ReasoningBudgetExceeded")
                .field("scope", scope)
                .field("limit_bytes", limit_bytes)
                .field("observed_bytes", observed_bytes)
                .finish(),
            Self::ReasoningReplayBudgetExceeded {
                scope,
                limit_bytes,
                observed_bytes,
                limit_items,
                observed_items,
            } => formatter
                .debug_struct("ReasoningReplayBudgetExceeded")
                .field("scope", scope)
                .field("limit_bytes", limit_bytes)
                .field("observed_bytes", observed_bytes)
                .field("limit_items", limit_items)
                .field("observed_items", observed_items)
                .finish(),
            Self::Context(error) => formatter.debug_tuple("Context").field(error).finish(),
        }
    }
}

impl VegaError {
    pub fn at_runtime_stage(self, stage: RuntimeFailureStage) -> Self {
        let mut detail = match self {
            Self::RuntimeFailure(detail) if detail.stage != RuntimeFailureStage::EventPipeline => {
                return Self::RuntimeFailure(detail);
            }
            Self::RuntimeFailure(detail) => detail,
            Self::Store(error) => {
                let mut detail = RuntimeFailureDetail::new(RuntimeFailureReason::Database);
                if let Some(error) = error.sqlite_error() {
                    detail.sqlite_code = Some(error.extended_code & 255);
                    detail.sqlite_extended_code = Some(error.extended_code);
                }
                detail
            }
            Self::Io(error) => {
                let mut detail = RuntimeFailureDetail::new(RuntimeFailureReason::Io);
                detail.io_kind = Some(error.kind().into());
                detail
            }
            error => return error,
        };
        detail.stage = stage;
        Self::RuntimeFailure(detail)
    }

    pub fn acknowledgement_error(&self) -> Self {
        match self {
            Self::Cancelled => Self::Cancelled,
            Self::RuntimeFailure(detail) => Self::RuntimeFailure(*detail),
            _ => Self::RuntimeFailure(RuntimeFailureDetail::new(RuntimeFailureReason::Pipeline)),
        }
    }

    pub fn diagnostic_metrics(&self) -> crate::RuntimeDiagnosticMetrics {
        let mut metrics = crate::RuntimeDiagnosticMetrics::default();
        let (source, kind, scope) = match self {
            Self::RuntimeFailure(detail) => {
                metrics.runtime_stage = Some(detail.stage.as_str());
                metrics.runtime_reason = Some(detail.reason.as_str());
                metrics.sqlite_code = detail.sqlite_code;
                metrics.sqlite_extended_code = detail.sqlite_extended_code;
                metrics.io_kind = detail.io_kind.map(RuntimeIoKind::as_str);
                ("runtime", "unknown", None)
            }
            Self::ProviderPayloadTooLarge {
                observed_bytes,
                limit_bytes,
            } => {
                metrics.failure_observed = u64::try_from(*observed_bytes).ok();
                metrics.failure_limit = u64::try_from(*limit_bytes).ok();
                metrics.failure_units = Some("bytes");
                ("provider", "protocol", Some("sse_frame"))
            }
            Self::ProviderDiagnostic {
                kind,
                status,
                retry_count,
                request_id,
                ..
            } => {
                metrics.provider.http_status = *status;
                metrics.provider.retry_count = *retry_count;
                metrics.provider.request_id = request_id.clone();
                (
                    "provider",
                    match kind {
                        crate::ProviderFailureKind::Http => "http",
                        crate::ProviderFailureKind::Transport => "transport",
                        crate::ProviderFailureKind::StreamInterrupted => "stream_interrupted",
                        crate::ProviderFailureKind::Timeout => "timeout",
                        crate::ProviderFailureKind::Protocol => "protocol",
                        crate::ProviderFailureKind::Rejected => "rejected",
                        crate::ProviderFailureKind::OutputLimit => "output_limit",
                    },
                    Some("response"),
                )
            }
            Self::Provider { status, .. } => {
                metrics.provider.http_status = *status;
                (
                    "provider",
                    if status.is_some() { "http" } else { "unknown" },
                    None,
                )
            }
            Self::ModelImagesUnsupported => ("runtime", "model_images", None),
            Self::ReasoningSelectionInvalid { .. } => ("runtime", "reasoning_selection", None),
            Self::ReasoningBudgetExceeded {
                scope,
                limit_bytes,
                observed_bytes,
            }
            | Self::ReasoningReplayBudgetExceeded {
                scope,
                limit_bytes,
                observed_bytes,
                ..
            } => {
                metrics.failure_observed = u64::try_from(*observed_bytes).ok();
                metrics.failure_limit = u64::try_from(*limit_bytes).ok();
                metrics.failure_units = Some("bytes");
                if let Self::ReasoningReplayBudgetExceeded {
                    observed_items,
                    limit_items,
                    ..
                } = self
                {
                    metrics.failure_observed_items = u64::try_from(*observed_items).ok();
                    metrics.failure_limit_items = u64::try_from(*limit_items).ok();
                }
                (
                    "runtime",
                    "reasoning_limit",
                    Some(match scope {
                        crate::ReasoningBudgetScope::Delta => "delta",
                        crate::ReasoningBudgetScope::Turn => "response",
                        crate::ReasoningBudgetScope::Run => "run",
                    }),
                )
            }
            Self::Context(
                crate::ContextRuntimeError::OverLimit {
                    estimated_tokens,
                    input_budget,
                }
                | crate::ContextRuntimeError::SummaryInputOverLimit {
                    estimated_tokens,
                    input_budget,
                },
            ) => {
                metrics.failure_observed = Some(*estimated_tokens);
                metrics.failure_limit = Some(*input_budget);
                metrics.failure_units = Some("tokens");
                (
                    "runtime",
                    "context_over_limit",
                    Some(
                        if matches!(
                            self,
                            Self::Context(crate::ContextRuntimeError::SummaryInputOverLimit { .. })
                        ) {
                            "summary_input"
                        } else {
                            "request_input"
                        },
                    ),
                )
            }
            Self::Context(crate::ContextRuntimeError::ResultOverLimit {
                estimated_tokens,
                target_tokens,
            }) => {
                metrics.failure_observed = Some(*estimated_tokens);
                metrics.failure_limit = Some(*target_tokens);
                metrics.failure_units = Some("tokens");
                ("runtime", "context_over_limit", Some("compaction_result"))
            }
            Self::CredentialExposureBlocked => ("runtime", "credential_exposure", None),
            Self::CredentialCheckUnavailable => ("runtime", "credential_check", None),
            Self::ProjectInstructions(error)
                if error.failure() == vega_tools::ProjectInstructionsFailure::InvalidRootPath =>
            {
                ("runtime", "project_path", None)
            }
            Self::ProjectInstructions(_) => ("runtime", "project_instructions", None),
            Self::CredentialContinuationUnavailable => ("runtime", "credential_continuation", None),
            Self::CredentialProjectionInvalid { tool_index } => {
                metrics.failure_tool_index = u64::try_from(*tool_index).ok();
                ("runtime", "credential_projection", None)
            }
            Self::ReasoningReplayUnsupported | Self::ReasoningReplayIncomplete => {
                ("runtime", "protocol", Some("response"))
            }
            Self::Cancelled => ("runtime", "cancelled", None),
            _ => ("runtime", "unknown", None),
        };
        metrics.failure_source = Some(source);
        metrics.failure_kind = Some(kind);
        metrics.failure_scope = scope;
        if let Some((observed, limit)) = self.context_overflow_token_counts() {
            metrics.failure_kind = Some("context_over_limit");
            metrics.failure_scope = Some("request_context");
            metrics.failure_observed = observed;
            metrics.failure_limit = limit;
            metrics.failure_units = (observed.is_some() || limit.is_some()).then_some("tokens");
        }
        metrics
    }

    pub fn context_input_overflow(&self) -> Option<Option<u64>> {
        self.context_overflow_token_counts()
            .map(|(requested, limit)| {
                requested.zip(limit).and_then(|(requested, limit)| {
                    requested.checked_sub(limit).filter(|gap| *gap > 0)
                })
            })
    }

    fn context_overflow_token_counts(&self) -> Option<(Option<u64>, Option<u64>)> {
        let (status, message) = match self {
            VegaError::Provider {
                status: Some(status @ (400 | 413)),
                message,
                ..
            }
            | VegaError::ProviderDiagnostic {
                status: Some(status @ (400 | 413)),
                message,
                ..
            } => (*status, message),
            _ => return None,
        };
        if message.len() > 4_096 {
            return None;
        }
        let prefix = format!("chat/completions request failed (HTTP {status}): ");
        let responses_prefix = format!("responses request failed (HTTP {status}): ");
        let provider_prefix = format!("provider request failed (HTTP {status}): ");
        let body = message
            .strip_prefix(&prefix)
            .or_else(|| message.strip_prefix(&responses_prefix))
            .or_else(|| message.strip_prefix(&provider_prefix))
            .unwrap_or(message)
            .trim();
        let parsed = serde_json::from_str::<serde_json::Value>(body).ok();
        let details = parsed
            .as_ref()
            .map(|value| value.get("error").unwrap_or(value));
        let known_code = details.is_some_and(|details| {
            ["code", "type"].iter().any(|field| {
                matches!(
                    details.get(*field).and_then(serde_json::Value::as_str),
                    Some("context_length_exceeded" | "prompt_too_long" | "input_too_long")
                )
            })
        });
        let diagnostic = details
            .and_then(|details| details.get("message"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_else(|| {
                body.strip_prefix("{\"error\":{\"message\":\"")
                    .unwrap_or(body)
            });
        let lower = diagnostic.to_ascii_lowercase();
        let known_phrase = lower.starts_with("prompt is too long")
            || lower.starts_with("prompt too long")
            || lower.starts_with("this model's maximum context length is ")
            || (lower.starts_with("the input (")
                && lower.contains("is longer than the model's context length"))
            || (lower.starts_with("input length ")
                && (lower.contains("exceeds model's maximum context length")
                    || lower.contains("exceeds the maximum allowed input length")))
            || (lower.starts_with("prompt has ")
                && lower.contains("tokens, but the configured context size is "));
        if !known_code && !known_phrase {
            return None;
        }
        Some((
            details
                .and_then(|details| details.get("requested_tokens"))
                .and_then(serde_json::Value::as_u64),
            details
                .and_then(|details| details.get("max_context_tokens"))
                .and_then(serde_json::Value::as_u64),
        ))
    }
}
