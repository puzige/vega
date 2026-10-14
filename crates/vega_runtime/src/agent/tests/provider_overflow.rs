use super::*;
use crate::EventStream;

fn overflow() -> ScriptStep {
    ScriptStep::Error {
        status: Some(400),
        message:
            r#"responses request failed (HTTP 400): {"error":{"code":"context_length_exceeded"}}"#
                .into(),
        retryable: false,
    }
}

#[tokio::test]
async fn issue375_confirmed_overflow_compacts_once_and_preserves_goal_progress() {
    for repeated in [false, true] {
        let dir = tempdir().unwrap();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let provider = MockProvider::new_rounds(vec![
            vec![overflow()],
            if repeated {
                vec![overflow()]
            } else {
                vec![ScriptStep::events(vec![
                    ProviderEvent::TextDelta("continued".into()),
                    ProviderEvent::Done {
                        stop_reason: StopReason::End,
                    },
                ])]
            },
        ]);
        let calls = Arc::new(AtomicUsize::new(0));
        let mut req = request(vec![
            ChatMessage::new(ChatRole::User, "Task goal; progress; pending work"),
            ChatMessage::new(ChatRole::Assistant, "completed work ".repeat(100)),
            ChatMessage::new(ChatRole::User, "continue"),
        ]);
        req.context_budget = Some(ContextBudget::new(100_000, 1_000, true).unwrap());
        req.context_compaction_hook = Some(Arc::new(RecordingCompactionHook {
            calls: calls.clone(),
            messages: vec![ChatMessage::new(
                ChatRole::User,
                "Task goal; progress; pending work; continue",
            )],
            source_version: 1,
        }));
        let outcome = run_agent(&provider, &tools, req, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(outcome.failed, repeated);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(provider.requests().len(), 2);
        assert_eq!(outcome.executed_tool_call_count, 0);
        assert!(
            provider.requests()[1]
                .messages
                .iter()
                .any(|m| m.content.contains("Task goal; progress; pending work"))
        );
        if !repeated {
            assert_eq!(outcome.final_text, "continued");
        }
    }
}

#[tokio::test]
async fn issue375_generic_disabled_and_partial_response_never_trigger_overflow_recovery() {
    for (automatic, generic, partial) in [
        (false, false, false),
        (true, true, false),
        (true, false, true),
    ] {
        let dir = tempdir().unwrap();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let mut steps = Vec::new();
        if partial {
            steps.push(ScriptStep::events(vec![ProviderEvent::TextDelta(
                "visible".into(),
            )]));
        }
        steps.push(if generic {
            ScriptStep::Error {
                status: Some(400),
                message: "invalid request".into(),
                retryable: false,
            }
        } else {
            overflow()
        });
        let provider = MockProvider::new(steps);
        let calls = Arc::new(AtomicUsize::new(0));
        let mut req = request(vec![ChatMessage::new(ChatRole::User, "Task goal")]);
        req.context_budget = Some(ContextBudget::new(100_000, 1_000, automatic).unwrap());
        req.context_compaction_hook = Some(Arc::new(RecordingCompactionHook {
            calls: calls.clone(),
            messages: vec![ChatMessage::new(ChatRole::User, "summary")],
            source_version: 1,
        }));
        let outcome = run_agent(&provider, &tools, req, CancellationToken::new())
            .await
            .unwrap();
        assert!(outcome.failed);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(provider.requests().len(), 1);
        if partial {
            assert_eq!(outcome.final_text, "visible");
        }
    }
}

#[tokio::test]
async fn issue375_unchanged_overflow_compaction_does_not_retry_primary() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new(vec![overflow()]);
    let history = vec![ChatMessage::new(ChatRole::User, "Task goal and progress")];
    let calls = Arc::new(AtomicUsize::new(0));
    let mut req = request(history.clone());
    req.context_budget = Some(ContextBudget::new(100_000, 1_000, true).unwrap());
    req.context_compaction_hook = Some(Arc::new(RecordingCompactionHook {
        calls: calls.clone(),
        messages: history,
        source_version: 1,
    }));
    let result = run_agent(&provider, &tools, req, CancellationToken::new()).await;
    assert!(result.is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(provider.requests().len(), 1);
}

struct OpenOverflowProvider {
    opens: Arc<AtomicUsize>,
    inner: MockProvider,
}

impl Provider for OpenOverflowProvider {
    fn chat_stream(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        if self.opens.fetch_add(1, Ordering::SeqCst) == 0 {
            return Box::pin(async {
                Err(VegaError::Provider {
                    status: Some(413),
                    message: r#"{"error":{"code":"context_length_exceeded"}}"#.into(),
                    retryable: false,
                })
            });
        }
        self.inner.chat_stream(request, cancel)
    }
}

#[tokio::test]
async fn issue375_open_rejection_recovers_before_stream_with_one_compaction() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let opens = Arc::new(AtomicUsize::new(0));
    let provider = OpenOverflowProvider {
        opens: opens.clone(),
        inner: MockProvider::new(vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("complete".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])]),
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let mut req = request(vec![
        ChatMessage::new(ChatRole::User, "goal"),
        ChatMessage::new(ChatRole::Assistant, "progress ".repeat(1000)),
        ChatMessage::new(ChatRole::User, "continue"),
    ]);
    req.context_budget = Some(ContextBudget::new(100_000, 1_000, true).unwrap());
    req.context_compaction_hook = Some(Arc::new(RecordingCompactionHook {
        calls: calls.clone(),
        messages: vec![ChatMessage::new(ChatRole::User, "goal; progress; continue")],
        source_version: 1,
    }));
    let result = run_agent(&provider, &tools, req, CancellationToken::new())
        .await
        .unwrap();
    assert!(!result.failed);
    assert_eq!(result.final_text, "complete");
    assert_eq!(opens.load(Ordering::SeqCst), 2);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(result.executed_tool_call_count, 0);
}
