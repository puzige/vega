use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use futures::future::BoxFuture;
use tokio_util::sync::CancellationToken;
use vega_runtime::{
    AgentRequest, ChatMessage, ChatRequest, ChatRole, ContextBudget, ContextCompactionFailure,
    ContextCompactionHook, ContextCompactionRequest, ContextCompactionResult, ContextRuntimeError,
    EventStream, Provider, ProviderEvent, RuntimeToolConfig, StopReason, VegaError, run_agent,
};

struct RefillingProvider(AtomicUsize);

impl Provider for RefillingProvider {
    fn chat_stream(
        &self,
        _request: ChatRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        let round = self.0.fetch_add(1, Ordering::SeqCst);
        assert!(
            round < 3,
            "rapid refill must stop before a fourth primary request"
        );
        Box::pin(async move {
            let stream: EventStream = Box::pin(futures::stream::iter([
                Ok(ProviderEvent::TextDelta("large result ".repeat(50_000))),
                Ok(ProviderEvent::ToolUse {
                    id: format!("call-{round}"),
                    name: "unavailable_tool".into(),
                    input_json: "{}".into(),
                }),
                Ok(ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                }),
            ]));
            Ok(stream)
        })
    }
}

struct CompactingHook(AtomicUsize);

impl ContextCompactionHook for CompactingHook {
    fn compact<'a>(
        &'a self,
        request: ContextCompactionRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'a, Result<ContextCompactionResult, ContextCompactionFailure>> {
        let installed = self.0.fetch_add(1, Ordering::SeqCst) + 1;
        Box::pin(async move {
            Ok(ContextCompactionResult {
                messages: vec![
                    ChatMessage::new(ChatRole::User, "Historical task summary"),
                    ChatMessage::new(ChatRole::User, "Continue the task"),
                ],
                source_version: request.source_version + 1,
                source_fingerprint: Some(format!("source-{installed}")),
                replay_cold_prefix_messages: Some(1),
                replay_interleaved_user_boundary: None,
                reactive_retained_groups: None,
                usages: Vec::new(),
                usage_complete: true,
            })
        })
    }
}

#[tokio::test]
async fn rapid_refill_stops_the_real_loop_before_a_fourth_summary()
-> Result<(), Box<dyn std::error::Error>> {
    let hook = Arc::new(CompactingHook(AtomicUsize::new(0)));
    let provider = RefillingProvider(AtomicUsize::new(0));
    let tools = vega_tools::Tools::new(std::env::current_dir()?)?;
    let request = AgentRequest {
        model: "synthetic".into(),
        system_prompt: "Complete the task".into(),
        history: vec![
            ChatMessage::new(ChatRole::User, "Original task"),
            ChatMessage::new(ChatRole::Assistant, "old history ".repeat(50_000)),
            ChatMessage::new(ChatRole::User, "Continue the task"),
        ],
        max_tokens: None,
        completed_tool_results: Default::default(),
        tool_config: RuntimeToolConfig::default(),
        pricing_catalog: None,
        reasoning: None,
        model_snapshot: None,
        replay_frame: None,
        context_budget: Some(ContextBudget::new(100_000, 10_000, true)?),
        context_source_version: Some(1),
        context_source_fingerprint: Some("initial".into()),
        context_operation_id: None,
        context_compaction_hook: Some(hook.clone()),
    };
    let result = run_agent(&provider, &tools, request, CancellationToken::new()).await;
    assert!(matches!(
        result,
        Err(VegaError::Context(ContextRuntimeError::OverLimit { .. }))
    ));
    assert_eq!(provider.0.load(Ordering::SeqCst), 3);
    assert_eq!(hook.0.load(Ordering::SeqCst), 3);
    Ok(())
}
