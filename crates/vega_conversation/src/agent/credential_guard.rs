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
    let known = reader().map_err(|_| VegaError::CredentialCheckUnavailable)?;
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

fn project_json(value: &mut serde_json::Value, known: &[String]) -> Result<(), VegaError> {
    match value {
        serde_json::Value::String(text) => {
            *text = vega_runtime::redact_sensitive_credential_text(text, known)
        }
        serde_json::Value::Array(items) => {
            for item in items {
                project_json(item, known)?;
            }
        }
        serde_json::Value::Object(fields) => {
            for (key, value) in fields {
                if vega_runtime::contains_sensitive_credential(key, known) {
                    return Err(blocked());
                }
                project_json(value, known)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub(crate) fn project_owner_secrets(
    mut request: ChatRequest,
    reader: &(dyn Fn() -> Result<Vec<String>, ()> + Send + Sync),
) -> Result<ChatRequest, VegaError> {
    let known = reader().map_err(|_| VegaError::CredentialCheckUnavailable)?;
    for message in &mut request.messages {
        message.content = vega_runtime::redact_sensitive_credential_text(&message.content, &known);
        if let Some(reasoning) = &mut message.reasoning_content {
            *reasoning = vega_runtime::redact_sensitive_credential_text(reasoning, &known);
        }
        for call in &mut message.tool_calls {
            match serde_json::from_str::<serde_json::Value>(&call.input_json) {
                Ok(mut value) => {
                    project_json(&mut value, &known)?;
                    call.input_json = serde_json::to_string(&value).map_err(|_| blocked())?;
                }
                Err(_) if vega_runtime::contains_sensitive_credential(&call.input_json, &known) => {
                    return Err(blocked());
                }
                Err(_) => {}
            }
        }
    }
    for (tool_index, tool) in request.tools.iter_mut().enumerate() {
        tool.description =
            vega_runtime::redact_sensitive_credential_text(&tool.description, &known);
        if contains_json_secret(&tool.input_schema, &known) {
            return Err(VegaError::CredentialProjectionInvalid { tool_index });
        }
    }
    Ok(request)
}

impl Provider for OwnerCredentialProvider {
    fn chat_stream_recovering(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<vega_runtime::ProviderStream, VegaError>> {
        let provider = self.provider.clone();
        let known = self.known.clone();
        Box::pin(async move {
            let request = project_owner_secrets(request, known.as_ref())?;
            reject_owner_secret(&request, known.as_ref())?;
            provider.chat_stream_recovering(request, cancel).await
        })
    }
    fn chat_stream_once(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        let provider = self.provider.clone();
        let known = self.known.clone();
        Box::pin(async move {
            let request = project_owner_secrets(request, known.as_ref())?;
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
            let request = project_owner_secrets(request, known.as_ref())?;
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
        let inner = Arc::new(MockProvider::new(vec![
            ScriptStep::text("safe"),
            ScriptStep::text("safe"),
        ]));
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
        assert!(
            guarded
                .chat_stream(request, CancellationToken::new())
                .await
                .is_ok()
        );
        let requests = inner.requests();
        assert_eq!(requests.len(), 2);
        assert!(!requests[1].messages[0].content.contains(ROTATED));
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

        let original = request.clone();
        assert!(
            guarded
                .chat_stream(request, CancellationToken::new())
                .await
                .is_ok()
        );
        assert_eq!(original.messages[0].content, SECRET);
        assert_eq!(inner.requests().len(), 1);
        assert!(!inner.requests()[0].messages[0].content.contains(SECRET));
    }

    struct RecoveryOnlyProvider(std::sync::atomic::AtomicUsize);

    impl Provider for RecoveryOnlyProvider {
        fn chat_stream(
            &self,
            _: ChatRequest,
            _: CancellationToken,
        ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
            panic!("primary request must use recovery entry point")
        }

        fn chat_stream_recovering(
            &self,
            _: ChatRequest,
            _: CancellationToken,
        ) -> BoxFuture<'static, Result<vega_runtime::ProviderStream, VegaError>> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Box::pin(async {
                Ok(vega_runtime::ProviderStream {
                    events: Box::pin(futures::stream::empty()),
                    metadata: vega_runtime::ProviderResponseMetadata {
                        http_status: Some(200),
                        request_id: None,
                        retry_count: Some(2),
                    },
                })
            })
        }
    }

    #[tokio::test]
    async fn issue350_owner_wrapper_delegates_recovery_and_blocks_credentials_before_dispatch() {
        let inner = Arc::new(RecoveryOnlyProvider(std::sync::atomic::AtomicUsize::new(0)));
        let wrapped = OwnerCredentialProvider::new(
            inner.clone(),
            Arc::new(|| Ok(vec!["owned-secret-fixture".into()])),
        );
        let call = wrapped
            .chat_stream_recovering(ChatRequest::default(), CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(call.metadata.retry_count, Some(2));
        let request = ChatRequest {
            messages: vec![ChatMessage::new(
                vega_runtime::ChatRole::User,
                "owned-secret-fixture",
            )],
            ..Default::default()
        };
        assert!(
            wrapped
                .chat_stream_recovering(request, CancellationToken::new())
                .await
                .is_ok()
        );
        assert_eq!(inner.0.load(std::sync::atomic::Ordering::SeqCst), 2);
    }
    #[test]
    fn projection_preserves_json_structure_and_refuses_schema_semantic_changes() {
        let secret = "synthetic-quoted-\"secret";
        let reader = || Ok(vec![secret.to_string()]);
        let mut request = ChatRequest::default();
        request.messages.push(ChatMessage::assistant_with_tools(
            "safe",
            vec![vega_runtime::ChatToolCall {
                id: "call-1".into(),
                name: "bash".into(),
                input_json: serde_json::json!({"command": format!("echo {secret}"), "count": 7})
                    .to_string(),
            }],
        ));
        request
            .messages
            .push(ChatMessage::tool_result("call-1", secret));
        let projected = project_owner_secrets(request.clone(), &reader).unwrap();
        assert_eq!(
            projected.messages[1].tool_call_id.as_deref(),
            Some("call-1")
        );
        let arguments: serde_json::Value =
            serde_json::from_str(&projected.messages[0].tool_calls[0].input_json).unwrap();
        assert_eq!(arguments["count"], 7);
        assert!(!arguments["command"].as_str().unwrap().contains(secret));
        assert!(request.messages[1].content.contains(secret));
        assert!(project_json(&mut serde_json::json!({secret: "value"}), &[secret.into()]).is_err());
    }

    #[tokio::test]
    async fn unavailable_authority_never_dispatches_or_becomes_transport_failure() {
        let inner = Arc::new(MockProvider::new(vec![]));
        let guarded = OwnerCredentialProvider::new(inner.clone(), Arc::new(|| Err(())));
        assert!(matches!(
            guarded
                .chat_stream(ChatRequest::default(), CancellationToken::new())
                .await,
            Err(VegaError::CredentialCheckUnavailable)
        ));
        assert!(inner.requests().is_empty());
    }
}
