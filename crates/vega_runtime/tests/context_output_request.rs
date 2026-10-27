use futures::future::BoxFuture;
use tokio_util::sync::CancellationToken;
use vega_runtime::model_compat::{
    ModelApi, ModelIdentity, ModelRoute, ResolvedModel, lookup_model_definition, resolve_model,
};
use vega_runtime::{
    AgentRequest, ChatMessage, ChatRequest, ChatRole, ContextBudget, EventStream, Provider,
    ProviderEvent, RuntimeToolConfig, StopReason, VegaError, run_agent,
};

struct OutputBoundProvider {
    model: ResolvedModel,
    expected: u32,
}

impl Provider for OutputBoundProvider {
    fn model_snapshot(&self) -> Option<ResolvedModel> {
        Some(self.model.clone())
    }

    fn chat_stream(
        &self,
        request: ChatRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        assert_eq!(request.max_tokens, Some(self.expected));
        assert!(request.model_snapshot.is_some());
        Box::pin(async {
            let events: EventStream = Box::pin(futures::stream::iter([
                Ok(ProviderEvent::TextDelta("Done".into())),
                Ok(ProviderEvent::Done {
                    stop_reason: StopReason::End,
                }),
            ]));
            Ok(events)
        })
    }
}

#[tokio::test]
async fn primary_request_enforces_main_reserve_without_summary_cap()
-> Result<(), Box<dyn std::error::Error>> {
    let route = ModelRoute::new(ModelApi::ChatCompletions, "https://example.invalid/v1")?;
    let identity = ModelIdentity {
        provider: "development".into(),
        model: "bounded-model".into(),
    };
    let mut definition = lookup_model_definition(&route, &identity)?;
    definition.context_window = Some(200_000);
    definition.max_output_tokens = Some(32_000);
    definition.reasoning = Some(false);
    let model = resolve_model(&definition, None)?;
    let tools = vega_tools::Tools::new(std::env::current_dir()?)?;
    for (requested, expected) in [(None, 32_000), (Some(4_000), 4_000), (Some(64_000), 32_000)] {
        let provider = OutputBoundProvider {
            model: model.clone(),
            expected,
        };
        let request = AgentRequest {
            model: identity.model.clone(),
            system_prompt: "Complete the task without tools.".into(),
            history: vec![ChatMessage::new(ChatRole::User, "Say done")],
            max_tokens: requested,
            completed_tool_results: Default::default(),
            tool_config: RuntimeToolConfig::default(),
            pricing_catalog: None,
            reasoning: None,
            model_snapshot: None,
            replay_frame: None,
            context_budget: Some(ContextBudget::new(428_000, 128_000, false)?),
            context_source_version: None,
            context_source_fingerprint: None,
            context_operation_id: None,
            context_compaction_hook: None,
        };
        run_agent(&provider, &tools, request, CancellationToken::new()).await?;
    }
    Ok(())
}
