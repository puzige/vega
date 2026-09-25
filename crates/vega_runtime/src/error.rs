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
    /// Context budget or compaction hook failure.  The nested type is
    /// metadata-only and deliberately excludes historical prompt content.
    #[error(transparent)]
    Context(#[from] crate::ContextRuntimeError),
}

impl fmt::Debug for VegaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
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
            Self::Context(error) => formatter.debug_tuple("Context").field(error).finish(),
        }
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
    fn cancelled_display_is_stable() {
        assert_eq!(VegaError::Cancelled.to_string(), "operation cancelled");
    }
}
