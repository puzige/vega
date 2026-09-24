//! Owner-only final provider boundary for previously persisted untrusted data.

use std::sync::Arc;

use futures::future::BoxFuture;
use tokio_util::sync::CancellationToken;
use vega_runtime::{ChatRequest, EventStream, Provider, VegaError};

/// Re-reads owner-held credentials immediately before every model request.
/// The reader never enters a Debug value, request, event, audit, or log.
pub struct OwnerCredentialProvider {
    provider: Arc<dyn Provider>,
    known: Arc<dyn Fn() -> Result<Vec<String>, ()> + Send + Sync>,
}

impl OwnerCredentialProvider {
    /// Wrap a provider with the current owner-only credential authority.
    pub fn new(
        provider: Arc<dyn Provider>,
        known: Arc<dyn Fn() -> Result<Vec<String>, ()> + Send + Sync>,
    ) -> Self {
        Self { provider, known }
    }

    /// The same final projection check for transports with internal retries.
    /// OpenAI calls it again immediately before every HTTP attempt, while the
    /// outer wrapper still protects other providers and auxiliary requests.
    pub fn pre_attempt_guard(
        known: Arc<dyn Fn() -> Result<Vec<String>, ()> + Send + Sync>,
    ) -> impl Fn(&ChatRequest) -> Result<(), VegaError> + Send + Sync + 'static {
        move |request| reject_owner_secret(request, known.as_ref())
    }
}

fn blocked() -> VegaError {
    VegaError::Provider {
        status: None,
        message: "owner credential appeared in provider projection".into(),
        retryable: false,
    }
}

fn contains_json_secret(value: &serde_json::Value, secret: &str) -> bool {
    match value {
        serde_json::Value::String(text) => text.contains(secret),
        serde_json::Value::Array(items) => {
            items.iter().any(|item| contains_json_secret(item, secret))
        }
        serde_json::Value::Object(fields) => fields
            .iter()
            .any(|(key, value)| key.contains(secret) || contains_json_secret(value, secret)),
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {
            false
        }
    }
}

fn reject_owner_secret(
    request: &ChatRequest,
    reader: &(dyn Fn() -> Result<Vec<String>, ()> + Send + Sync),
) -> Result<(), VegaError> {
    let known = reader().map_err(|_| blocked())?;
    for secret in known.iter().filter(|secret| !secret.is_empty()) {
        if request.messages.iter().any(|message| {
            message.content.contains(secret)
                || message
                    .tool_call_id
                    .as_deref()
                    .is_some_and(|id| id.contains(secret))
                || message
                    .reasoning_content
                    .as_deref()
                    .is_some_and(|reasoning| reasoning.contains(secret))
                || message.tool_calls.iter().any(|call| {
                    call.id.contains(secret)
                        || call.name.contains(secret)
                        || call.input_json.contains(secret)
                })
        }) || request.tools.iter().any(|tool| {
            tool.name.contains(secret)
                || tool.description.contains(secret)
                || contains_json_secret(&tool.input_schema, secret)
                || serde_json::to_string(&tool.input_schema)
                    .map_or(true, |schema| schema.contains(secret))
        }) {
            return Err(blocked());
        }
    }
    Ok(())
}

impl Provider for OwnerCredentialProvider {
    fn chat_stream_once(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        let provider = self.provider.clone();
        let known = self.known.clone();
        Box::pin(async move {
            reject_owner_secret(&request, known.as_ref())?;
            provider.chat_stream_once(request, cancel).await
        })
    }

    fn chat_stream(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        let provider = self.provider.clone();
        let known = self.known.clone();
        Box::pin(async move {
            reject_owner_secret(&request, known.as_ref())?;
            provider.chat_stream(request, cancel).await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vega_runtime::{ChatMessage, MockProvider, ScriptStep};

    #[tokio::test]
    async fn issue73_rotated_keystore_is_rechecked_before_each_provider_attempt() {
        const ROTATED: &str = "fake-key-rotated-after-429-73";
        let config = tempfile::tempdir().unwrap();
        vega_store::keystore::set_key(config.path(), "provider-owned", "fake-initial-key-73")
            .unwrap();
        let reader_root = config.path().to_path_buf();
        let reader: Arc<dyn Fn() -> Result<Vec<String>, ()> + Send + Sync> = Arc::new(move || {
            vega_store::keystore::get_key(&reader_root, "provider-owned")
                .map(|key| vec![key])
                .map_err(|_| ())
        });
        let inner = Arc::new(MockProvider::new(vec![ScriptStep::text("safe")]));
        let guarded = OwnerCredentialProvider::new(inner.clone(), reader.clone());
        let guard = OwnerCredentialProvider::pre_attempt_guard(reader);
        let request = ChatRequest {
            model: "fixture-model".into(),
            messages: vec![ChatMessage::tool_result("historical-mcp", ROTATED)],
            ..ChatRequest::default()
        };
        assert!(guard(&request).is_ok());
        assert!(
            guarded
                .chat_stream(request.clone(), CancellationToken::new())
                .await
                .is_ok()
        );
        vega_store::keystore::set_key(config.path(), "provider-owned", ROTATED).unwrap();
        assert!(matches!(
            guard(&request),
            Err(VegaError::Provider {
                retryable: false,
                ..
            })
        ));
        assert!(matches!(
            guarded.chat_stream(request, CancellationToken::new()).await,
            Err(VegaError::Provider {
                retryable: false,
                ..
            })
        ));
        assert_eq!(inner.requests().len(), 1);
    }
}
