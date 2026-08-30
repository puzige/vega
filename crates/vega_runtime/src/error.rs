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
