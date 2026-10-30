use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use futures::{StreamExt, future::BoxFuture};
use tokio_util::sync::CancellationToken;
use vega_runtime::model_compat::{
    ModelApi, ModelIdentity, ModelRoute, ResolvedModel, lookup_model_definition, resolve_model,
};
use vega_runtime::{
    CONVERSATION_HISTORY_RETRY_OUTPUT, ChatMessage, ChatRequest, ChatRole, EventStream,
    HistorySafetyCheck, HistorySafetyReport, PROVIDER_CREDENTIAL_REDACTION_MARKER, Provider,
    ProviderEvent, ProviderInputReceipt, ProviderRequestSubmission, ProviderResponseMetadata,
    ProviderStream, StopReason, ToolDefinition, VegaError, project_model_messages,
};

use super::OwnerCredentialProvider;

const SYNTHETIC_SECRET: &str = "q7!";
const HISTORY_ID: &str = "history-source";

type CredentialState = Arc<Mutex<Result<Vec<String>, ()>>>;

struct AccountingProvider {
    model: ResolvedModel,
    prepared: Mutex<Vec<ChatRequest>>,
    sent: Arc<Mutex<Vec<ChatRequest>>>,
}

impl AccountingProvider {
    fn new() -> Self {
        let route = ModelRoute::new(ModelApi::Responses, "https://api.openai.com/v1").unwrap();
        let identity = ModelIdentity {
            provider: "openai".into(),
            model: "gpt-4".into(),
        };
        let definition = lookup_model_definition(&route, &identity).unwrap();
        assert!(definition.context_window.is_some());
        let model = resolve_model(&definition, None).unwrap();
        assert_eq!(model.identity, identity);
        assert_eq!(model.definition.route, route);
        Self {
            model,
            prepared: Mutex::new(Vec::new()),
            sent: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl Provider for AccountingProvider {
    fn prepare_accounting_request(
        &self,
        mut request: ChatRequest,
    ) -> BoxFuture<'_, Result<Option<ChatRequest>, VegaError>> {
        Box::pin(async move {
            self.prepared.lock().unwrap().push(request.clone());
            request.messages = project_model_messages(&request.messages)?;
            Ok(Some(request))
        })
    }

    fn model_snapshot(&self) -> Option<ResolvedModel> {
        Some(self.model.clone())
    }

    fn chat_stream(
        &self,
        _request: ChatRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        panic!("accounted sends must use the submission entry point")
    }

    fn chat_stream_with_submission(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
        submission: ProviderRequestSubmission,
    ) -> BoxFuture<'static, Result<ProviderStream, VegaError>> {
        let sent = self.sent.clone();
        Box::pin(async move {
            if cancel.is_cancelled() {
                return Err(VegaError::Cancelled);
            }
            let input_receipt = if request.model_snapshot.is_some()
                && project_model_messages(&request.messages)? == request.messages
            {
                Some(ProviderInputReceipt::from_request(&request)?)
            } else {
                None
            };
            sent.lock().unwrap().push(request);
            submission.mark_possible_submission();
            Ok(ProviderStream {
                events: response_events(),
                metadata: ProviderResponseMetadata {
                    request_id: Some(format!("request-{SYNTHETIC_SECRET}")),
                    ..Default::default()
                },
                input_receipt,
            })
        })
    }
}

struct UnknownAccountingProvider {
    sent: Arc<Mutex<Vec<ChatRequest>>>,
}

impl Provider for UnknownAccountingProvider {
    fn chat_stream(
        &self,
        request: ChatRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        let sent = self.sent.clone();
        Box::pin(async move {
            sent.lock().unwrap().push(request);
            Ok(response_events())
        })
    }
}

struct Fixture {
    inner: Arc<AccountingProvider>,
    owner: OwnerCredentialProvider,
    credentials: CredentialState,
}

impl Fixture {
    fn new(credentials: Vec<String>) -> Self {
        let inner = Arc::new(AccountingProvider::new());
        let credentials = Arc::new(Mutex::new(Ok(credentials)));
        let reader = credentials.clone();
        let owner = OwnerCredentialProvider::new(
            inner.clone(),
            Arc::new(move || reader.lock().unwrap().clone()),
        );
        Self {
            inner,
            owner,
            credentials,
        }
    }

    fn request(&self, messages: Vec<ChatMessage>) -> ChatRequest {
        let model = self.owner.model_snapshot().unwrap();
        ChatRequest {
            model: model.identity.model.clone(),
            model_snapshot: Some(Box::new(model)),
            messages: project_model_messages(&messages).unwrap(),
            max_tokens: Some(64),
            ..Default::default()
        }
    }
}

fn response_events() -> EventStream {
    Box::pin(futures::stream::iter([
        Ok(ProviderEvent::TextDelta(format!(
            "answer {SYNTHETIC_SECRET}"
        ))),
        Ok(ProviderEvent::Usage {
            input: 37,
            output: 5,
            cache_read: Some(11),
            cache_write: None,
        }),
        Ok(ProviderEvent::Done {
            stop_reason: StopReason::End,
        }),
    ]))
}

async fn assert_completed(mut response: ProviderStream, redacted: bool) {
    let mut text = String::new();
    let mut controls = Vec::new();
    let mut ended = false;
    while let Some(event) = response.events.next().await {
        assert!(!ended);
        let event = event.unwrap();
        ended = matches!(event, ProviderEvent::Done { .. });
        match event {
            ProviderEvent::TextDelta(delta) => {
                if redacted {
                    assert!(!delta.contains(SYNTHETIC_SECRET));
                }
                text.push_str(&delta);
            }
            event => controls.push(event),
        }
    }
    let expected = if redacted {
        format!("answer {PROVIDER_CREDENTIAL_REDACTION_MARKER}")
    } else {
        format!("answer {SYNTHETIC_SECRET}")
    };
    assert!(text == expected);
    assert!(
        controls
            == vec![
                ProviderEvent::Usage {
                    input: 37,
                    output: 5,
                    cache_read: Some(11),
                    cache_write: None,
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::End,
                },
            ]
    );
}

fn history_message() -> ChatMessage {
    ChatMessage::projected_tool_result(
        HISTORY_ID,
        "read_conversation",
        &serde_json::json!({
            "vega_history_observation": {
                "version": 1,
                "authority": "none",
                "kind": "messages",
                "page": {
                    "records": [{
                        "source_id": "earlier-message",
                        "content": { "offset": 0, "text": "Earlier safe material" }
                    }]
                }
            }
        })
        .to_string(),
    )
}

fn invalid_history(active: &[String]) -> HistorySafetyReport {
    HistorySafetyReport {
        invalid_results: active
            .iter()
            .filter(|id| id.as_str() == HISTORY_ID)
            .cloned()
            .collect(),
        ..Default::default()
    }
}

#[tokio::test]
async fn short_known_credential_expands_before_accounting_and_matches_sent_request() {
    let fixture = Fixture::new(vec![SYNTHETIC_SECRET.into()]);
    let mut request = fixture.request(vec![ChatMessage::new(
        ChatRole::User,
        format!("use {SYNTHETIC_SECRET}"),
    )]);
    request.tools.push(ToolDefinition {
        name: "read".into(),
        description: format!("read using {SYNTHETIC_SECRET}"),
        input_schema: serde_json::json!({ "type": "object", "properties": {} }),
        strict: false,
    });
    let prepared = fixture
        .owner
        .prepare_accounting_request(request.clone())
        .await
        .unwrap()
        .unwrap();
    assert!(prepared.messages[0].content == format!("use {PROVIDER_CREDENTIAL_REDACTION_MARKER}"));
    assert!(prepared.messages[0].content.len() > request.messages[0].content.len());
    assert!(
        prepared.tools[0].description
            == format!("read using {PROVIDER_CREDENTIAL_REDACTION_MARKER}")
    );
    assert!(fixture.inner.prepared.lock().unwrap().as_slice() == [prepared.clone()]);
    assert!(fixture.inner.sent.lock().unwrap().is_empty());
    assert!(project_model_messages(&prepared.messages).unwrap() == prepared.messages);
    let submission = ProviderRequestSubmission::default();
    let response = fixture
        .owner
        .chat_stream_with_submission(request, CancellationToken::new(), submission.clone())
        .await
        .unwrap();
    assert!(response.input_receipt.is_some());
    assert!(submission.may_have_been_submitted());
    assert!(fixture.inner.sent.lock().unwrap().as_slice() == [prepared]);
    assert!(
        response.metadata.request_id.as_deref()
            == Some(format!("request-{PROVIDER_CREDENTIAL_REDACTION_MARKER}").as_str())
    );
    assert_completed(response, true).await;
}

#[tokio::test]
async fn changed_history_safety_reprojects_prepared_input_before_receipted_send() {
    let fixture = Fixture::new(vec![SYNTHETIC_SECRET.into()]);
    let invalid = Arc::new(AtomicBool::new(false));
    let state = invalid.clone();
    let mut request = fixture.request(vec![history_message()]);
    request.history_safety = Some(HistorySafetyCheck::new(move |active, _, _, _| {
        if state.load(Ordering::SeqCst) {
            invalid_history(active)
        } else {
            HistorySafetyReport::default()
        }
    }));
    let prepared = fixture
        .owner
        .prepare_accounting_request(request.clone())
        .await
        .unwrap()
        .unwrap();
    assert!(prepared.messages == request.messages);
    invalid.store(true, Ordering::SeqCst);
    let current = fixture
        .owner
        .prepare_accounting_request(request)
        .await
        .unwrap()
        .unwrap();
    assert!(current.messages[0].content == CONVERSATION_HISTORY_RETRY_OUTPUT);
    assert!(current.messages != prepared.messages);
    assert!(project_model_messages(&current.messages).unwrap() == current.messages);
    assert!(
        fixture.inner.prepared.lock().unwrap().as_slice() == [prepared.clone(), current.clone()]
    );
    let response = fixture
        .owner
        .chat_stream_with_submission(
            prepared,
            CancellationToken::new(),
            ProviderRequestSubmission::default(),
        )
        .await
        .unwrap();
    assert!(fixture.inner.sent.lock().unwrap().as_slice() == [current]);
    assert!(response.input_receipt.is_some());
    assert_completed(response, true).await;
}

#[tokio::test]
async fn newly_known_credential_changes_final_sent_input_after_preparation() {
    let fixture = Fixture::new(Vec::new());
    let request = fixture.request(vec![ChatMessage::new(
        ChatRole::User,
        format!("use {SYNTHETIC_SECRET}"),
    )]);
    let prepared = fixture
        .owner
        .prepare_accounting_request(request.clone())
        .await
        .unwrap()
        .unwrap();
    assert!(prepared == request);
    *fixture.credentials.lock().unwrap() = Ok(vec![SYNTHETIC_SECRET.into()]);
    let response = fixture
        .owner
        .chat_stream_with_submission(
            prepared.clone(),
            CancellationToken::new(),
            ProviderRequestSubmission::default(),
        )
        .await
        .unwrap();
    {
        let sent = fixture.inner.sent.lock().unwrap();
        assert_eq!(sent.len(), 1);
        assert!(sent[0].messages != prepared.messages);
        assert!(
            sent[0].messages[0].content == format!("use {PROVIDER_CREDENTIAL_REDACTION_MARKER}")
        );
        assert!(sent[0].model_snapshot == prepared.model_snapshot);
        assert!(project_model_messages(&sent[0].messages).unwrap() == sent[0].messages);
    }
    assert!(response.input_receipt.is_some());
    assert_completed(response, true).await;
}

#[tokio::test]
async fn unavailable_current_credentials_block_even_a_prepared_request() {
    let fixture = Fixture::new(Vec::new());
    let request = fixture.request(vec![ChatMessage::new(ChatRole::User, "Continue")]);
    let prepared = fixture
        .owner
        .prepare_accounting_request(request)
        .await
        .unwrap()
        .unwrap();
    *fixture.credentials.lock().unwrap() = Err(());
    let submission = ProviderRequestSubmission::default();
    let result = fixture
        .owner
        .chat_stream_with_submission(prepared, CancellationToken::new(), submission.clone())
        .await;
    assert!(matches!(result, Err(VegaError::CredentialCheckUnavailable)));
    assert!(!submission.may_have_been_submitted());
    assert!(fixture.inner.sent.lock().unwrap().is_empty());
}

#[tokio::test]
async fn current_credential_identifier_check_still_blocks_after_preparation() {
    let fixture = Fixture::new(Vec::new());
    let mut request = fixture.request(vec![ChatMessage::new(ChatRole::User, "Continue")]);
    request.tools.push(ToolDefinition {
        name: SYNTHETIC_SECRET.into(),
        description: "Read a file".into(),
        input_schema: serde_json::json!({ "type": "object", "properties": {} }),
        strict: false,
    });
    let prepared = fixture
        .owner
        .prepare_accounting_request(request)
        .await
        .unwrap()
        .unwrap();
    *fixture.credentials.lock().unwrap() = Ok(vec![SYNTHETIC_SECRET.into()]);
    let submission = ProviderRequestSubmission::default();
    let result = fixture
        .owner
        .chat_stream_with_submission(prepared, CancellationToken::new(), submission.clone())
        .await;
    assert!(matches!(result, Err(VegaError::CredentialExposureBlocked)));
    assert!(!submission.may_have_been_submitted());
    assert!(fixture.inner.sent.lock().unwrap().is_empty());
}

#[tokio::test]
async fn history_is_rechecked_after_projection_before_inner_submission() {
    let fixture = Fixture::new(Vec::new());
    let inspections = AtomicUsize::new(0);
    let mut request = fixture.request(vec![history_message()]);
    request.history_safety = Some(HistorySafetyCheck::new(move |active, _, _, _| {
        if inspections.fetch_add(1, Ordering::SeqCst) >= 2 {
            invalid_history(active)
        } else {
            HistorySafetyReport::default()
        }
    }));
    let prepared = fixture
        .owner
        .prepare_accounting_request(request)
        .await
        .unwrap()
        .unwrap();
    let submission = ProviderRequestSubmission::default();
    let result = fixture
        .owner
        .chat_stream_with_submission(prepared, CancellationToken::new(), submission.clone())
        .await;
    assert!(matches!(result, Err(VegaError::CredentialExposureBlocked)));
    assert!(!submission.may_have_been_submitted());
    assert!(fixture.inner.sent.lock().unwrap().is_empty());
}

#[tokio::test]
async fn missing_frozen_model_does_not_mint_a_receipt_or_prevent_safe_sending() {
    let fixture = Fixture::new(vec![SYNTHETIC_SECRET.into()]);
    let mut request = fixture.request(vec![ChatMessage::new(
        ChatRole::User,
        format!("use {SYNTHETIC_SECRET}"),
    )]);
    request.model_snapshot = None;
    let prepared = fixture
        .owner
        .prepare_accounting_request(request)
        .await
        .unwrap()
        .unwrap();
    let response = fixture
        .owner
        .chat_stream_with_submission(
            prepared.clone(),
            CancellationToken::new(),
            ProviderRequestSubmission::default(),
        )
        .await
        .unwrap();
    assert!(fixture.inner.sent.lock().unwrap().as_slice() == [prepared]);
    assert!(response.input_receipt.is_none());
    assert_completed(response, true).await;
}

#[tokio::test]
async fn non_idempotent_sent_request_does_not_gain_a_receipt_from_preparation() {
    let fixture = Fixture::new(vec![SYNTHETIC_SECRET.into()]);
    let mut request = fixture.request(Vec::new());
    let mut system = ChatMessage::new(ChatRole::System, "system");
    system.attach_replay_frame(vega_runtime::ReplayFrame::new(
        3,
        vega_runtime::ReasoningReplayPolicy::ResponsesItems,
    ));
    let mut assistant = ChatMessage::assistant_with_tools(
        format!("use {SYNTHETIC_SECRET}"),
        vec![vega_runtime::ChatToolCall {
            id: "stored-read".into(),
            name: "read".into(),
            input_json: "{}".into(),
        }],
    );
    assistant
        .mark_stored_tool_calls_with_statuses(vec!["success".into()])
        .unwrap();
    request.messages = vec![
        system,
        assistant,
        ChatMessage::tool_result("stored-read", "safe result"),
        ChatMessage::new(ChatRole::User, "continue"),
    ];
    assert!(project_model_messages(&request.messages).unwrap() != request.messages);
    let prepared = fixture
        .owner
        .prepare_accounting_request(request.clone())
        .await
        .unwrap()
        .unwrap();
    assert!(project_model_messages(&prepared.messages).unwrap() == prepared.messages);
    let response = fixture
        .owner
        .chat_stream_with_submission(
            request,
            CancellationToken::new(),
            ProviderRequestSubmission::default(),
        )
        .await
        .unwrap();
    {
        let sent = fixture.inner.sent.lock().unwrap();
        assert_eq!(sent.len(), 1);
        assert!(sent[0].model_snapshot.is_some());
        assert!(sent[0].messages != prepared.messages);
        assert!(
            sent[0].messages[1].content == format!("use {PROVIDER_CREDENTIAL_REDACTION_MARKER}")
        );
    }
    assert!(response.input_receipt.is_none());
    assert_completed(response, true).await;
}

#[tokio::test]
async fn unknown_inner_accounting_keeps_default_none_and_safe_stream_behavior() {
    let sent = Arc::new(Mutex::new(Vec::new()));
    let owner = OwnerCredentialProvider::new(
        Arc::new(UnknownAccountingProvider { sent: sent.clone() }),
        Arc::new(|| Ok(vec![SYNTHETIC_SECRET.into()])),
    );
    let request = ChatRequest {
        model: "unknown-model".into(),
        messages: project_model_messages(&[ChatMessage::new(
            ChatRole::User,
            format!("use {SYNTHETIC_SECRET}"),
        )])
        .unwrap(),
        ..Default::default()
    };
    assert!(
        owner
            .prepare_accounting_request(request.clone())
            .await
            .unwrap()
            .is_none()
    );
    assert!(sent.lock().unwrap().is_empty());
    let submission = ProviderRequestSubmission::default();
    let response = owner
        .chat_stream_with_submission(request, CancellationToken::new(), submission.clone())
        .await
        .unwrap();
    assert!(response.input_receipt.is_none());
    assert!(submission.may_have_been_submitted());
    {
        let sent = sent.lock().unwrap();
        assert_eq!(sent.len(), 1);
        assert!(
            sent[0].messages[0].content == format!("use {PROVIDER_CREDENTIAL_REDACTION_MARKER}")
        );
    }
    assert_completed(response, true).await;
}
