//! Unified error model (tech-spec §7, A3-01 / S4-T19).

use std::fmt;

use thiserror::Error;

/// Unified Vega error (tech-spec §7).
///
/// `Send + Sync` so it can cross await points and thread boundaries inside
/// the headless runtime and into the conversation/UI layers. Provider errors
/// never carry the API key (red line: keys never reach logs or errors).
#[derive(Error)]
pub enum VegaError {
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
        "this model returned reasoning state that cannot be replayed; choose a matching advanced protocol template and retry"
    )]
    ReasoningReplayUnsupported,
    #[error(
        "the model response omitted required reasoning replay state; retry after checking the advanced protocol template"
    )]
    ReasoningReplayIncomplete,
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
    pub fn diagnostic_metrics(&self) -> crate::RuntimeDiagnosticMetrics {
        let mut metrics = crate::RuntimeDiagnosticMetrics::default();
        let (source, kind, scope) = match self {
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
            || lower.starts_with("this model's maximum context length is ");
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

#[cfg(test)]
mod tests {
    use super::VegaError;

    #[test]
    fn vega_error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<VegaError>();
    }

    #[test]
    fn provider_error_debug_and_display_hide_payload() {
        const MESSAGE_SENTINEL: &str = "VEGA_PROVIDER_MESSAGE_SENTINEL";
        let err = VegaError::Provider {
            status: Some(503),
            message: MESSAGE_SENTINEL.to_string(),
            retryable: true,
        };
        for rendered in [format!("{err:?}"), err.to_string()] {
            assert!(rendered.contains("503"), "provider status metadata missing");
            assert!(
                rendered.contains("retryable"),
                "provider retry metadata missing"
            );
            assert!(
                !rendered.contains(MESSAGE_SENTINEL),
                "provider formatting leaked payload"
            );
        }
        let VegaError::Provider {
            status,
            message,
            retryable,
        } = err
        else {
            unreachable!()
        };
        assert_eq!(status, Some(503));
        assert!(
            message == MESSAGE_SENTINEL,
            "typed provider message changed"
        );
        assert!(retryable);
    }

    #[test]
    fn provider_diagnostic_debug_and_display_hide_body_and_request_id() {
        const MESSAGE_SENTINEL: &str = "VEGA_PROVIDER_BODY_CANARY";
        const REQUEST_ID_SENTINEL: &str = "VEGA_REQUEST_ID_CANARY";
        let err = VegaError::ProviderDiagnostic {
            kind: crate::provider::ProviderFailureKind::Rejected,
            status: Some(429),
            message: MESSAGE_SENTINEL.to_string(),
            retryable: false,
            retry_count: Some(2),
            request_id: Some(REQUEST_ID_SENTINEL.to_string()),
        };
        for rendered in [format!("{err:?}"), err.to_string()] {
            assert!(rendered.contains("429"));
            assert!(rendered.contains("retryable"));
            assert!(!rendered.contains(MESSAGE_SENTINEL));
            assert!(!rendered.contains(REQUEST_ID_SENTINEL));
        }
    }

    #[test]
    fn error_debug_redacts_io_store_and_tool_payloads() {
        const IO_SENTINEL: &str = "VEGA_IO_SENTINEL";
        const TOOL_SENTINEL: &str = "VEGA_TOOL_SENTINEL";
        const TOOL_MESSAGE_SENTINEL: &str = "VEGA_TOOL_MESSAGE_SENTINEL";
        let values = [
            VegaError::Io(std::io::Error::other(IO_SENTINEL)),
            VegaError::Store(rusqlite::Error::InvalidParameterName(IO_SENTINEL.into())),
            VegaError::Tool {
                tool: TOOL_SENTINEL.into(),
                message: TOOL_MESSAGE_SENTINEL.into(),
            },
        ];
        for value in values {
            let rendered = format!("{value:?}");
            for sentinel in [IO_SENTINEL, TOOL_SENTINEL, TOOL_MESSAGE_SENTINEL] {
                assert!(!rendered.contains(sentinel), "error Debug leaked payload");
            }
        }
    }

    #[test]
    fn provider_context_diagnostics_preserve_only_explicit_token_measurements() {
        for prefix in [
            "",
            "provider request failed (HTTP 400): ",
            "chat/completions request failed (HTTP 400): ",
        ] {
            let error = VegaError::Provider {
                status: Some(400),
                message: format!(
                    "{prefix}{}",
                    r#"{"error":{"code":"context_length_exceeded","requested_tokens":130000,"max_context_tokens":128000,"message":"PRIVATE_BODY"}}"#
                ),
                retryable: false,
            };
            let metrics = error.diagnostic_metrics();
            assert_eq!(error.context_input_overflow(), Some(Some(2000)));
            assert_eq!(metrics.failure_source, Some("provider"));
            assert_eq!(metrics.failure_kind, Some("context_over_limit"));
            assert_eq!(metrics.failure_scope, Some("request_context"));
            assert_eq!(metrics.failure_observed, Some(130000));
            assert_eq!(metrics.failure_limit, Some(128000));
            assert_eq!(metrics.failure_units, Some("tokens"));
            assert!(!format!("{metrics:?}").contains("PRIVATE_BODY"));
        }
        for body in [
            r#"{"error":{"code":"context_length_exceeded","max_tokens":128000}}"#,
            r#"{"error":{"code":"context_length_exceeded","requested_tokens":"130000","max_context_tokens":-1}}"#,
        ] {
            let error = VegaError::ProviderDiagnostic {
                kind: crate::ProviderFailureKind::Rejected,
                status: Some(400),
                message: body.into(),
                retryable: false,
                retry_count: None,
                request_id: None,
            };
            let metrics = error.diagnostic_metrics();
            assert_eq!(metrics.failure_kind, Some("context_over_limit"));
            assert_eq!(metrics.failure_observed, None);
            assert_eq!(metrics.failure_limit, None);
            assert_eq!(metrics.failure_units, None);
            assert_eq!(error.context_input_overflow(), Some(None));
        }
    }

    #[test]
    fn cancelled_display_is_stable() {
        assert_eq!(VegaError::Cancelled.to_string(), "operation cancelled");
    }
}
