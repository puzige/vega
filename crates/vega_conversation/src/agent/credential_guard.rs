//! Owner-only final provider boundary for previously persisted untrusted data.

use std::sync::Arc;

use futures::{StreamExt, future::BoxFuture};
use tokio_util::sync::CancellationToken;
use vega_runtime::{ChatRequest, EventStream, Provider, VegaError};

/// Re-reads owner-held credentials immediately before every model request.
/// The reader never enters a Debug value, request, event, audit, or log.
pub struct OwnerCredentialProvider {
    provider: Arc<dyn Provider>,
    known: Arc<dyn Fn() -> Result<Vec<String>, ()> + Send + Sync>,
    exceptions: Option<vega_runtime::CredentialExceptionFilter>,
}

impl OwnerCredentialProvider {
    /// Wrap a provider with the current owner-only credential authority.
    pub fn new(
        provider: Arc<dyn Provider>,
        known: Arc<dyn Fn() -> Result<Vec<String>, ()> + Send + Sync>,
    ) -> Self {
        Self {
            provider,
            known,
            exceptions: None,
        }
    }

    pub fn with_credential_exceptions(
        mut self,
        filter: vega_runtime::CredentialExceptionFilter,
    ) -> Self {
        self.exceptions = Some(filter);
        self
    }

    pub fn pre_attempt_guard_with_exceptions(
        known: vega_runtime::CredentialReader,
        filter: vega_runtime::CredentialExceptionFilter,
    ) -> impl Fn(&ChatRequest) -> Result<(), VegaError> + Send + Sync + 'static {
        move |request| reject_owner_secret_with_policy(request, known.as_ref(), Some(&filter))
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

fn contains_json_secret(
    value: &serde_json::Value,
    credentials: &[String],
    exceptions: Option<&vega_runtime::CredentialExceptionFilter>,
) -> bool {
    match value {
        serde_json::Value::String(text) => contains_with_policy(text, credentials, exceptions),
        serde_json::Value::Array(items) => items
            .iter()
            .any(|item| contains_json_secret(item, credentials, exceptions)),
        serde_json::Value::Object(fields) => fields.iter().any(|(key, value)| {
            contains_with_policy(key, credentials, exceptions)
                || contains_json_secret(value, credentials, exceptions)
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
    reject_owner_secret_with_policy(request, reader, None)
}

fn contains_with_policy(
    text: &str,
    known: &[String],
    exceptions: Option<&vega_runtime::CredentialExceptionFilter>,
) -> bool {
    vega_runtime::redact_sensitive_credential_text_with_policy(text, known, exceptions) != text
}

fn reject_owner_secret_with_policy(
    request: &ChatRequest,
    reader: &(dyn Fn() -> Result<Vec<String>, ()> + Send + Sync),
    exceptions: Option<&vega_runtime::CredentialExceptionFilter>,
) -> Result<(), VegaError> {
    let known = reader().map_err(|_| VegaError::CredentialCheckUnavailable)?;
    if request.messages.iter().any(|message| {
        contains_with_policy(&message.content, &known, exceptions)
            || message
                .tool_call_id
                .as_deref()
                .is_some_and(|id| contains_with_policy(id, &known, exceptions))
            || message
                .reasoning_content
                .as_deref()
                .is_some_and(|reasoning| contains_with_policy(reasoning, &known, exceptions))
            || message.tool_calls.iter().any(|call| {
                contains_with_policy(&call.id, &known, exceptions)
                    || contains_with_policy(&call.name, &known, exceptions)
                    || contains_with_policy(&call.input_json, &known, exceptions)
            })
    }) || request.tools.iter().any(|tool| {
        contains_with_policy(&tool.name, &known, exceptions)
            || contains_with_policy(&tool.description, &known, exceptions)
            || contains_json_secret(&tool.input_schema, &known, exceptions)
            || serde_json::to_string(&tool.input_schema).map_or(true, |schema| {
                contains_with_policy(&schema, &known, exceptions)
            })
    }) {
        return Err(blocked());
    }
    Ok(())
}

fn project_json(
    value: &mut serde_json::Value,
    known: &[String],
    exceptions: Option<&vega_runtime::CredentialExceptionFilter>,
) -> Result<(), VegaError> {
    match value {
        serde_json::Value::String(text) => {
            *text =
                vega_runtime::redact_sensitive_credential_text_with_policy(text, known, exceptions)
        }
        serde_json::Value::Array(items) => {
            for item in items {
                project_json(item, known, exceptions)?;
            }
        }
        serde_json::Value::Object(fields) => {
            for (key, value) in fields {
                if contains_with_policy(key, known, exceptions) {
                    return Err(blocked());
                }
                project_json(value, known, exceptions)?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn project_owner_secrets(
    request: ChatRequest,
    reader: &(dyn Fn() -> Result<Vec<String>, ()> + Send + Sync),
) -> Result<ChatRequest, VegaError> {
    project_owner_secrets_with_policy(request, reader, None)
}

pub(crate) fn project_owner_secrets_with_policy(
    mut request: ChatRequest,
    reader: &(dyn Fn() -> Result<Vec<String>, ()> + Send + Sync),
    exceptions: Option<&vega_runtime::CredentialExceptionFilter>,
) -> Result<ChatRequest, VegaError> {
    let known = reader().map_err(|_| VegaError::CredentialCheckUnavailable)?;
    for message in &mut request.messages {
        message.content = vega_runtime::redact_sensitive_credential_text_with_policy(
            &message.content,
            &known,
            exceptions,
        );
        if let Some(reasoning) = &mut message.reasoning_content {
            *reasoning = vega_runtime::redact_sensitive_credential_text_with_policy(
                reasoning, &known, exceptions,
            );
        }
        for call in &mut message.tool_calls {
            match serde_json::from_str::<serde_json::Value>(&call.input_json) {
                Ok(mut value) => {
                    project_json(&mut value, &known, exceptions)?;
                    call.input_json = serde_json::to_string(&value).map_err(|_| blocked())?;
                }
                Err(_) if contains_with_policy(&call.input_json, &known, exceptions) => {
                    return Err(blocked());
                }
                Err(_) => {}
            }
        }
    }
    for (tool_index, tool) in request.tools.iter_mut().enumerate() {
        tool.description = vega_runtime::redact_sensitive_credential_text_with_policy(
            &tool.description,
            &known,
            exceptions,
        );
        if contains_json_secret(&tool.input_schema, &known, exceptions) {
            return Err(VegaError::CredentialProjectionInvalid { tool_index });
        }
    }
    Ok(request)
}

fn protected_stream(
    stream: EventStream,
    known: Vec<String>,
    exceptions: Option<vega_runtime::CredentialExceptionFilter>,
) -> EventStream {
    use vega_runtime::ProviderEvent;
    let known = Arc::new(known);
    let state = (
        stream,
        [String::new(), String::new(), String::new()],
        std::collections::VecDeque::new(),
        false,
    );
    Box::pin(futures::stream::unfold(
        state,
        move |(mut stream, mut pending, mut ready, mut ended)| {
            let known = known.clone();
            let exceptions = exceptions.clone();
            async move {
                loop {
                    if let Some(event) = ready.pop_front() {
                        return Some((event, (stream, pending, ready, ended)));
                    }
                    if ended {
                        return None;
                    }
                    let event = stream.next().await;
                    let part = match &event {
                        Some(Ok(ProviderEvent::TextDelta(text))) => Some((0, text)),
                        Some(Ok(ProviderEvent::ThinkingDelta(text))) => Some((1, text)),
                        Some(Ok(ProviderEvent::SummaryDelta(text))) => Some((2, text)),
                        _ => None,
                    };
                    if let Some((index, text)) = part {
                        pending[index].push_str(text);
                        pending[index] = vega_runtime::redact_sensitive_credential_text_with_policy(
                            &pending[index],
                            &known,
                            exceptions.as_ref(),
                        );
                        let end = vega_runtime::credential_stream_safe_end(&pending[index], &known);
                        if end > 0 {
                            let output: String = pending[index].drain(..end).collect();
                            ready.push_back(Ok(match index {
                                0 => ProviderEvent::TextDelta(output),
                                1 => ProviderEvent::ThinkingDelta(output),
                                _ => ProviderEvent::SummaryDelta(output),
                            }));
                        }
                        continue;
                    }
                    let flush = event.is_none()
                        || matches!(&event, Some(Err(_)) | Some(Ok(ProviderEvent::Done { .. })));
                    if flush {
                        for (index, text) in pending.iter_mut().enumerate() {
                            if !text.is_empty() {
                                let output =
                                    vega_runtime::redact_sensitive_credential_text_with_policy(
                                        text,
                                        &known,
                                        exceptions.as_ref(),
                                    );
                                text.clear();
                                ready.push_back(Ok(match index {
                                    0 => ProviderEvent::TextDelta(output),
                                    1 => ProviderEvent::ThinkingDelta(output),
                                    _ => ProviderEvent::SummaryDelta(output),
                                }));
                            }
                        }
                    }
                    match event {
                        Some(Ok(ProviderEvent::ResponseMetadata(mut metadata))) => {
                            if let Some(id) = &mut metadata.request_id {
                                *id = vega_runtime::redact_sensitive_credential_text_with_policy(
                                    id,
                                    &known,
                                    exceptions.as_ref(),
                                );
                            }
                            ready.push_back(Ok(ProviderEvent::ResponseMetadata(metadata)));
                        }
                        Some(Ok(ProviderEvent::ToolUse {
                            id,
                            name,
                            input_json,
                        })) => {
                            let projected = (|| {
                                if contains_with_policy(&id, &known, exceptions.as_ref())
                                    || contains_with_policy(&name, &known, exceptions.as_ref())
                                {
                                    return Err(blocked());
                                }
                                let input_json = if contains_with_policy(
                                    &input_json,
                                    &known,
                                    exceptions.as_ref(),
                                ) {
                                    let mut value: serde_json::Value =
                                        serde_json::from_str(&input_json).map_err(|_| blocked())?;
                                    project_json(&mut value, &known, exceptions.as_ref())?;
                                    serde_json::to_string(&value).map_err(|_| blocked())?
                                } else {
                                    input_json
                                };
                                Ok(ProviderEvent::ToolUse {
                                    id,
                                    name,
                                    input_json,
                                })
                            })();
                            if projected.is_err() {
                                ended = true;
                            }
                            ready.push_back(projected);
                        }
                        Some(Ok(ProviderEvent::ReasoningReplay(values))) => {
                            if values.iter().any(|value| {
                                contains_json_secret(value, &known, exceptions.as_ref())
                            }) {
                                ready.push_back(Err(blocked()));
                                ended = true;
                            } else {
                                ready.push_back(Ok(ProviderEvent::ReasoningReplay(values)));
                            }
                        }
                        Some(event) => {
                            if event.is_err() {
                                ended = true;
                            }
                            ready.push_back(event);
                        }
                        None => ended = true,
                    }
                }
            }
        },
    ))
}

impl Provider for OwnerCredentialProvider {
    fn chat_stream_recovering(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<vega_runtime::ProviderStream, VegaError>> {
        let provider = self.provider.clone();
        let known = self.known.clone();
        let exceptions = self.exceptions.clone();
        Box::pin(async move {
            let request =
                project_owner_secrets_with_policy(request, known.as_ref(), exceptions.as_ref())?;
            reject_owner_secret_with_policy(&request, known.as_ref(), exceptions.as_ref())?;
            let credentials = known().map_err(|_| VegaError::CredentialCheckUnavailable)?;
            let mut response = provider.chat_stream_recovering(request, cancel).await?;
            if let Some(id) = &mut response.metadata.request_id {
                *id = vega_runtime::redact_sensitive_credential_text_with_policy(
                    id,
                    &credentials,
                    exceptions.as_ref(),
                );
            }
            response.events = protected_stream(response.events, credentials, exceptions);
            Ok(response)
        })
    }
    fn chat_stream_once(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        let provider = self.provider.clone();
        let known = self.known.clone();
        let exceptions = self.exceptions.clone();
        Box::pin(async move {
            let request =
                project_owner_secrets_with_policy(request, known.as_ref(), exceptions.as_ref())?;
            reject_owner_secret_with_policy(&request, known.as_ref(), exceptions.as_ref())?;
            let credentials = known().map_err(|_| VegaError::CredentialCheckUnavailable)?;
            let stream = provider.chat_stream_once(request, cancel).await?;
            Ok(protected_stream(stream, credentials, exceptions))
        })
    }

    fn chat_stream(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        let provider = self.provider.clone();
        let known = self.known.clone();
        let exceptions = self.exceptions.clone();
        Box::pin(async move {
            let request =
                project_owner_secrets_with_policy(request, known.as_ref(), exceptions.as_ref())?;
            reject_owner_secret_with_policy(&request, known.as_ref(), exceptions.as_ref())?;
            let credentials = known().map_err(|_| VegaError::CredentialCheckUnavailable)?;
            let stream = provider.chat_stream(request, cancel).await?;
            Ok(protected_stream(stream, credentials, exceptions))
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
        assert!(
            project_json(
                &mut serde_json::json!({(secret): "value"}),
                &[secret.into()],
                None
            )
            .is_err()
        );
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
    #[test]
    fn schema_enum_const_required_and_property_keys_are_never_rewritten() {
        let secret = "synthetic-schema-secret";
        let reader = || Ok(vec![secret.to_string()]);
        for schema in [
            serde_json::json!({"enum": [secret]}),
            serde_json::json!({"const": secret}),
            serde_json::json!({"required": [secret]}),
            serde_json::json!({"properties": {(secret): {"type": "string"}}}),
        ] {
            let request = ChatRequest {
                tools: vec![vega_runtime::ToolDefinition {
                    name: "fixture".into(),
                    description: "safe".into(),
                    input_schema: schema.clone(),
                    strict: false,
                }],
                ..Default::default()
            };
            assert!(matches!(
                project_owner_secrets(request.clone(), &reader),
                Err(VegaError::CredentialProjectionInvalid { tool_index: 0 })
            ));
            assert_eq!(request.tools[0].input_schema, schema);
        }
    }
}

#[cfg(test)]
mod output_boundary_tests {
    use super::*;
    use vega_runtime::ProviderEvent;
    #[tokio::test]
    async fn ordinary_delta_is_delivered_before_waiting_for_the_next_chunk() {
        let source = futures::stream::once(async {
            Ok(ProviderEvent::TextDelta("ordinary first chunk ".into()))
        })
        .chain(futures::stream::pending());
        let mut stream = protected_stream(
            Box::pin(source),
            vec!["synthetic-unrelated-owner-secret".into()],
            None,
        );
        let event = tokio::time::timeout(std::time::Duration::from_secs(1), stream.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(matches!(event, ProviderEvent::TextDelta(text) if text == "ordinary first chunk "));
    }
    #[tokio::test]
    async fn split_unknown_token_is_filtered_without_buffering_ordinary_prose() {
        let token = "sk-proj-synthetic0123456789abcdef";
        let source = [
            ProviderEvent::TextDelta("ordinary text ".into()),
            ProviderEvent::TextDelta("sk-pr".into()),
            ProviderEvent::TextDelta("oj-synthetic0123456789abcdef".into()),
            ProviderEvent::Done {
                stop_reason: vega_runtime::StopReason::End,
            },
        ];
        let stream = Box::pin(futures::stream::iter(source.into_iter().map(Ok)));
        let events = protected_stream(stream, Vec::new(), None)
            .collect::<Vec<_>>()
            .await;
        let output = events
            .into_iter()
            .filter_map(|event| match event.unwrap() {
                ProviderEvent::TextDelta(text) => Some(text),
                _ => None,
            })
            .collect::<String>();
        assert!(output.starts_with("ordinary text "));
        assert!(!output.contains(token));
        assert!(output.contains(vega_runtime::PROVIDER_CREDENTIAL_REDACTION_MARKER));
    }
    #[tokio::test]
    async fn response_metadata_preserves_status_without_exposing_owner_value() {
        let secret = "synthetic-request-identifier-secret".to_string();
        let source = [ProviderEvent::ResponseMetadata(
            vega_runtime::ProviderResponseMetadata {
                http_status: Some(200),
                request_id: Some(format!("request-{secret}")),
                retry_count: Some(2),
            },
        )];
        let stream: EventStream = Box::pin(futures::stream::iter(source.into_iter().map(Ok)));
        let events = protected_stream(stream, vec![secret.clone()], None)
            .collect::<Vec<_>>()
            .await;
        let ProviderEvent::ResponseMetadata(metadata) = events.into_iter().next().unwrap().unwrap()
        else {
            panic!("expected response metadata");
        };
        assert_eq!(metadata.http_status, Some(200));
        assert_eq!(metadata.retry_count, Some(2));
        let request_id = metadata.request_id.unwrap();
        assert!(!request_id.contains(&secret));
        assert!(request_id.starts_with("request-"));
    }
    #[tokio::test]
    async fn split_owner_value_never_reaches_visible_events() {
        let secret = "known-stream-value-with-quotation\"and-slash\\".to_string();
        let source = [
            ProviderEvent::TextDelta("先说 ".into()),
            ProviderEvent::TextDelta(secret[..10].into()),
            ProviderEvent::ThinkingDelta(secret.clone()),
            ProviderEvent::TextDelta(secret[10..].into()),
            ProviderEvent::Done {
                stop_reason: vega_runtime::StopReason::End,
            },
        ];
        let stream: EventStream = Box::pin(futures::stream::iter(source.into_iter().map(Ok)));
        let events = protected_stream(stream, vec![secret.clone()], None)
            .collect::<Vec<_>>()
            .await;
        let mut visible = String::new();
        for event in events {
            match event.unwrap() {
                ProviderEvent::TextDelta(text) | ProviderEvent::ThinkingDelta(text) => {
                    visible.push_str(&text)
                }
                _ => {}
            }
        }
        assert!(!visible.contains(&secret));
        assert!(visible.contains("先说"));
    }
}
