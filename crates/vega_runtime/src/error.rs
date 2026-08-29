//! Unified error model (tech-spec §7, A3-01 / S4-T19).

use thiserror::Error;

/// Unified Vega error (tech-spec §7).
///
/// `Send + Sync` so it can cross await points and thread boundaries inside
/// the headless runtime and into the conversation/UI layers. Provider errors
/// never carry the API key (red line: keys never reach logs or errors).
#[derive(Debug, Error)]
pub enum VegaError {
    /// Provider (LLM API) failure. `status` is the HTTP status code when the
    /// failure came from an HTTP response (`None` for transport-level or
    /// protocol-level failures); `retryable` hints whether an automatic
    /// retry may succeed (`false` once retries are exhausted).
    #[error("provider error (status={status:?}, retryable={retryable}): {message}")]
    Provider {
        status: Option<u16>,
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

#[cfg(test)]
mod tests {
    use super::VegaError;

    #[test]
    fn vega_error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<VegaError>();
    }

    #[test]
    fn provider_error_display_hides_nothing_but_carries_status_and_retryable() {
        let err = VegaError::Provider {
            status: Some(503),
            message: "overloaded".to_string(),
            retryable: true,
        };
        let rendered = err.to_string();
        assert!(rendered.contains("503"), "{rendered}");
        assert!(rendered.contains("retryable=true"), "{rendered}");
        assert!(rendered.contains("overloaded"), "{rendered}");
    }

    #[test]
    fn cancelled_display_is_stable() {
        assert_eq!(VegaError::Cancelled.to_string(), "operation cancelled");
    }
}
