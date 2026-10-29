use super::*;
use std::collections::VecDeque;
use std::sync::Mutex;
use vega_runtime::model_compat::{
    ModelApi, ModelIdentity, ModelRoute, ResolvedModel, lookup_model_definition, resolve_model,
};

struct ForkProvider {
    snapshot: ResolvedModel,
    replies: Mutex<VecDeque<&'static str>>,
    requests: Mutex<Vec<ChatRequest>>,
}

impl ForkProvider {
    fn new(
        replies: impl IntoIterator<Item = &'static str>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let definition = lookup_model_definition(
            &ModelRoute::new(ModelApi::Responses, "https://api.openai.com/v1")?,
            &ModelIdentity {
                provider: "openai".into(),
                model: "gpt-4.1".into(),
            },
        )?;
        Ok(Self {
            snapshot: resolve_model(&definition, None)?,
            replies: Mutex::new(replies.into_iter().collect()),
            requests: Mutex::new(Vec::new()),
        })
    }
}

impl Provider for ForkProvider {
    fn model_snapshot(&self) -> Option<ResolvedModel> {
        Some(self.snapshot.clone())
    }

    fn chat_stream(
        &self,
        request: ChatRequest,
        _cancel: CancellationToken,
    ) -> futures::future::BoxFuture<'static, Result<vega_runtime::EventStream, VegaError>> {
        self.requests.lock().unwrap().push(request);
        let reply = self
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected request");
        Box::pin(async move {
            let mut events = vec![Ok(ProviderEvent::Usage {
                input: 100,
                output: 10,
                cache_read: Some(0),
                cache_write: Some(0),
            })];
            match reply {
                "error" => events.push(Err(VegaError::Provider {
                    status: Some(503),
                    message: "Synthetic unavailable".into(),
                    retryable: false,
                })),
                "tool" => events.push(Ok(ProviderEvent::ToolUse {
                    id: "must-not-run".into(),
                    name: "bash".into(),
                    input_json: "{}".into(),
                })),
                "cancel" => events.push(Err(VegaError::Cancelled)),
                "credential" => events.push(Err(VegaError::CredentialCheckUnavailable)),
                "empty" => events.push(Ok(ProviderEvent::Done {
                    stop_reason: StopReason::End,
                })),
                _ => {
                    events.push(Ok(ProviderEvent::TextDelta("FORK_SUMMARY".into())));
                    events.push(Ok(ProviderEvent::Done {
                        stop_reason: StopReason::End,
                    }));
                }
            }
            Ok(Box::pin(futures::stream::iter(events)) as vega_runtime::EventStream)
        })
    }
}

#[tokio::test]
async fn summary_fork_preserves_parent_prefix_and_accounts_each_fallback_attempt() -> TestResult {
    for mode in ["success", "error", "empty", "tool", "cancel", "credential"] {
        let (_directory, path, store) = source_fixture()?;
        store
            .conn()
            .execute("UPDATE threads SET model='gpt-4.1' WHERE id='thread'", [])?;
        let (source, mut request) = input(&store)?;
        request.system_prompt = "PARENT_SYSTEM".into();
        request.tools = vec![ToolDefinition {
            name: "bash".into(),
            description: "Synthetic tool".into(),
            input_schema: serde_json::json!({"type":"object","properties":{}}),
            strict: false,
        }];
        let provider = ForkProvider::new(if matches!(mode, "error" | "empty" | "tool") {
            vec![mode, "success"]
        } else {
            vec![mode]
        })?;
        let hook =
            ConversationCompactionHook::new(&provider, path, "thread", "gpt-4.1", None, None)
                .with_full_on_threshold(true);
        let generated = hook
            .generate_compaction(request.clone(), CancellationToken::new(), &mut None)
            .await;
        if matches!(mode, "cancel" | "credential") {
            let failure = generated.err().ok_or("expected failure")?;
            assert!(matches!(
                failure.error.as_ref(),
                VegaError::Cancelled | VegaError::CredentialCheckUnavailable
            ));
            assert_eq!(failure.usages.len(), 1);
        } else {
            let (generated, _guard) = generated.map_err(|failure| *failure.error)?;
            assert_eq!(generated.checkpoint.summary, "FORK_SUMMARY");
            assert_eq!(
                generated.result.usages.len(),
                if mode == "success" { 1 } else { 2 }
            );
        }
        let requests = provider.requests.lock().unwrap();
        let first = &requests[0];
        assert_eq!(first.messages[0].content, request.system_prompt);
        assert_eq!(
            &first.messages[1..1 + request.messages.len()],
            request.messages.as_slice()
        );
        assert_eq!(first.tools, request.tools);
        assert_eq!(first.tool_choice, Some(vega_runtime::ToolChoice::None));
        assert_eq!(first.model_snapshot.as_deref(), Some(&provider.snapshot));
        assert_eq!(first.reasoning, None);
        if requests.len() == 2 {
            assert!(requests[1].tools.is_empty());
            assert_eq!(requests[1].messages[0].content, SUMMARY_SYSTEM_PROMPT);
        }
        assert!(provider.replies.lock().unwrap().is_empty());
        assert_eq!(checkpoints(&store)?, 0);
        assert_eq!(load_source(store.conn(), "thread")?, source);
    }
    Ok(())
}

#[tokio::test]
async fn summary_fork_declines_rewritten_prefix_and_oversized_tool_schema() -> TestResult {
    for mode in ["rewrite", "schema"] {
        let (_directory, path, store) = source_fixture()?;
        let (_, mut request) = input(&store)?;
        let provider = ForkProvider::new([])?;
        if mode == "schema" {
            request.tools = vec![ToolDefinition {
                name: "large".into(),
                description: "x".repeat(1_000_000),
                input_schema: serde_json::json!({}),
                strict: false,
            }];
        }
        let mut history = request.messages.clone();
        if mode == "rewrite" {
            history[0].content.push_str("Changed");
        }
        let stage = vec![history];
        let task = ChatMessage::new(ChatRole::User, "Task");
        let cancel = CancellationToken::new();
        let execution = SummaryExecution {
            request: &request,
            task: &task,
            cancel: &cancel,
            deadline: tokio::time::Instant::now() + SUMMARY_OPERATION_TIMEOUT,
            attempts: 0,
            usages: Vec::new(),
            usage_complete: true,
            rejected_inputs: Vec::new(),
        };
        let direct = direct_summary_request(
            &request,
            "model",
            None,
            &stage,
            Some(&task),
            SummaryVariant::from_environment(),
        );
        let hook =
            ConversationCompactionHook::new(&provider, path, "thread", "gpt-4.1", None, None);
        assert!(
            hook.summary_fork_request(&stage, &direct, &execution)
                .await?
                .is_none()
        );
        assert!(provider.requests.lock().unwrap().is_empty());
    }
    Ok(())
}
