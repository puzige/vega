use super::*;

struct FailingSummary {
    calls: Arc<AtomicUsize>,
    terminal: bool,
}

impl ContextCompactionHook for FailingSummary {
    fn compact<'a>(
        &'a self,
        _request: ContextCompactionRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'a, Result<ContextCompactionResult, ContextCompactionFailure>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            Err(ContextCompactionFailure::new(
                if self.terminal {
                    VegaError::Provider {
                        status: None,
                        message: "owner credential appeared in provider projection".into(),
                        retryable: false,
                    }
                } else {
                    VegaError::Context(ContextRuntimeError::SummaryTimedOut)
                },
                None,
            ))
        })
    }
}

fn failing_request(calls: Arc<AtomicUsize>, terminal: bool, hard: bool) -> AgentRequest {
    let mut req = request(vec![
        ChatMessage::new(ChatRole::User, "old ".repeat(20_000)),
        ChatMessage::new(ChatRole::Assistant, "prior answer"),
        ChatMessage::new(ChatRole::User, "continue"),
    ]);
    let definitions = RunCapabilitySnapshot::freeze(
        req.tool_config.run_mode,
        req.tool_config.permission_mode,
        req.tool_config.mcp_candidates.clone(),
    )
    .unwrap();
    let estimate =
        crate::estimate_chat_context(&req.system_prompt, &req.history, definitions.definitions())
            .unwrap();
    let limit = if hard {
        estimate.input_tokens - 1
    } else {
        estimate.input_tokens * 11 / 10
    };
    req.context_budget = Some(ContextBudget::new(limit + 1000, 1000, true).unwrap());
    req.context_compaction_hook = Some(Arc::new(FailingSummary { calls, terminal }));
    req
}

#[tokio::test]
async fn claude_compaction_soft_failure_continues_and_stops_after_three_then_new_run_retries() {
    let project = tempdir().unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    for run in 1..=2 {
        let mut rounds = Vec::new();
        for index in 0..5 {
            rounds.push(vec![ScriptStep::events(vec![
                ProviderEvent::ToolUse {
                    id: format!("read-{index}"),
                    name: "read".into(),
                    input_json: r#"{"path":"missing"}"#.into(),
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                },
            ])]);
        }
        rounds.push(vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])]);
        let provider = MockProvider::new_rounds(rounds);
        let req = failing_request(calls.clone(), false, false);
        let original = req.history.clone();
        let outcome = run_agent(&provider, &tools, req, CancellationToken::new())
            .await
            .expect("sendable original context must continue");
        assert!(!outcome.failed);
        assert_eq!(provider.requests().len(), 6);
        assert_eq!(&provider.requests()[0].messages[1..], original);
        assert_eq!(calls.load(Ordering::SeqCst), run * 3);
        let phases: Vec<_> = outcome
            .events
            .iter()
            .filter_map(|event| match event {
                RuntimeEvent::ContextCompactionStatusUpdated { status } => Some(status.phase),
                _ => None,
            })
            .collect();
        assert_eq!(
            phases
                .iter()
                .filter(|p| **p == crate::ContextCompactionPhase::Failed)
                .count(),
            3
        );
        assert!(!phases.contains(&crate::ContextCompactionPhase::Succeeded));
    }
}

#[tokio::test]
async fn claude_compaction_hard_failure_and_credential_guard_never_continue() {
    let project = tempdir().unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    for (terminal, hard) in [(false, true), (true, false)] {
        let calls = Arc::new(AtomicUsize::new(0));
        let provider = MockProvider::new(vec![]);
        let result = run_agent(
            &provider,
            &tools,
            failing_request(calls.clone(), terminal, hard),
            CancellationToken::new(),
        )
        .await;
        assert!(result.is_err());
        assert!(provider.requests().is_empty());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}

struct BoundaryFailure(fn() -> VegaError);

impl ContextCompactionHook for BoundaryFailure {
    fn compact<'a>(
        &'a self,
        _request: ContextCompactionRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'a, Result<ContextCompactionResult, ContextCompactionFailure>> {
        Box::pin(async move { Err(ContextCompactionFailure::new((self.0)(), None)) })
    }
}

#[tokio::test]
async fn claude_compaction_failure_classification_preserves_terminal_safety() {
    let project = tempdir().unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    type Case = (fn() -> VegaError, bool);
    let cases: &[Case] = &[
        (
            || VegaError::Context(ContextRuntimeError::InvalidSummary),
            true,
        ),
        (
            || {
                VegaError::Context(ContextRuntimeError::SummaryOutputTruncated {
                    visible_bytes: 12,
                    thinking_bytes: 0,
                    output_tokens: Some(5),
                })
            },
            true,
        ),
        (
            || VegaError::Provider {
                status: Some(429),
                message: "rate limited".into(),
                retryable: false,
            },
            true,
        ),
        (
            || VegaError::Provider {
                status: Some(503),
                message: "unavailable".into(),
                retryable: false,
            },
            true,
        ),
        (
            || VegaError::Provider {
                status: Some(401),
                message: "unauthorized".into(),
                retryable: false,
            },
            false,
        ),
        (
            || VegaError::Provider {
                status: Some(403),
                message: "forbidden".into(),
                retryable: false,
            },
            false,
        ),
        (
            || VegaError::Provider {
                status: None,
                message: "unknown transport or guard error".into(),
                retryable: false,
            },
            false,
        ),
        (
            || VegaError::Context(ContextRuntimeError::SourceChanged),
            false,
        ),
        (
            || VegaError::Context(ContextRuntimeError::InvalidProjection),
            false,
        ),
        (
            || VegaError::Context(ContextRuntimeError::SystemMessageInResult),
            false,
        ),
        (|| VegaError::Store(rusqlite::Error::InvalidQuery), false),
        (|| VegaError::Cancelled, false),
    ];
    for &(error, recoverable) in cases {
        let provider = MockProvider::new(vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])]);
        let mut req = failing_request(Arc::new(AtomicUsize::new(0)), false, false);
        req.context_compaction_hook = Some(Arc::new(BoundaryFailure(error)));
        let result = run_agent(&provider, &tools, req, CancellationToken::new()).await;
        assert_eq!(
            result.is_ok(),
            recoverable,
            "classification for {:?}",
            error()
        );
        assert_eq!(provider.requests().len(), usize::from(recoverable));
    }
}
