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
    VegaError::CredentialExposureBlocked
}

fn contains_json_secret(value: &serde_json::Value, credentials: &[String]) -> bool {
    match value {
        serde_json::Value::String(text) => {
            vega_runtime::contains_sensitive_credential(text, credentials)
        }
        serde_json::Value::Array(items) => items
            .iter()
            .any(|item| contains_json_secret(item, credentials)),
        serde_json::Value::Object(fields) => fields.iter().any(|(key, value)| {
            vega_runtime::contains_sensitive_credential(key, credentials)
                || contains_json_secret(value, credentials)
        }),
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {
            false
        }
    }
}

pub(crate) fn reject_owner_secret(
    request: &ChatRequest,
    reader: &(dyn Fn() -> Result<Vec<String>, ()> + Send + Sync),
) -> Result<(), VegaError> {
    let known = reader().map_err(|_| blocked())?;
    if request.messages.iter().any(|message| {
        vega_runtime::contains_sensitive_credential(&message.content, &known)
            || message
                .tool_call_id
                .as_deref()
                .is_some_and(|id| vega_runtime::contains_sensitive_credential(id, &known))
            || message
                .reasoning_content
                .as_deref()
                .is_some_and(|reasoning| {
                    vega_runtime::contains_sensitive_credential(reasoning, &known)
                })
            || message.tool_calls.iter().any(|call| {
                vega_runtime::contains_sensitive_credential(&call.id, &known)
                    || vega_runtime::contains_sensitive_credential(&call.name, &known)
                    || vega_runtime::contains_sensitive_credential(&call.input_json, &known)
            })
    }) || request.tools.iter().any(|tool| {
        vega_runtime::contains_sensitive_credential(&tool.name, &known)
            || vega_runtime::contains_sensitive_credential(&tool.description, &known)
            || contains_json_secret(&tool.input_schema, &known)
            || serde_json::to_string(&tool.input_schema).map_or(true, |schema| {
                vega_runtime::contains_sensitive_credential(&schema, &known)
            })
    }) {
        return Err(blocked());
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
            Err(VegaError::CredentialExposureBlocked)
        ));
        assert!(matches!(
            guarded.chat_stream(request, CancellationToken::new()).await,
            Err(VegaError::CredentialExposureBlocked)
        ));
        assert_eq!(inner.requests().len(), 1);
    }

    #[tokio::test]
    async fn issue170_historical_credential_block_is_not_provider_transport_failure() {
        const SECRET: &str = "canary-credential-redaction-170-abcdefghijklmnopqrstuvwxyz";
        let inner = Arc::new(MockProvider::new(vec![ScriptStep::text("safe")]));
        let reader: Arc<dyn Fn() -> Result<Vec<String>, ()> + Send + Sync> =
            Arc::new(|| Ok(vec![SECRET.to_string()]));
        let guarded = OwnerCredentialProvider::new(inner.clone(), reader);
        let request = ChatRequest {
            model: "fixture-model".into(),
            messages: vec![ChatMessage::tool_result("historical-call", SECRET)],
            ..ChatRequest::default()
        };

        let error = match guarded.chat_stream(request, CancellationToken::new()).await {
            Ok(_) => panic!("legacy credential must be blocked before provider dispatch"),
            Err(error) => error,
        };

        assert!(!matches!(error, VegaError::Provider { status: None, .. }));
        assert!(inner.requests().is_empty());
    }
}
