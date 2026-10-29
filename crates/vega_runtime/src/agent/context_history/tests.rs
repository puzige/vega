use super::super::tools_exec::{
    PreparedRuntimeCall, execute_prepared_waiting, prepare_runtime_call,
};
use super::*;

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct HistoryProvider(Mutex<Vec<ChatRequest>>);
impl Provider for HistoryProvider {
    fn chat_stream(
        &self,
        chat_request: ChatRequest,
        _: CancellationToken,
    ) -> BoxFuture<'static, Result<crate::EventStream, VegaError>> {
        let mut requests = self.0.lock().unwrap();
        let first = requests.is_empty();
        requests.push(chat_request);
        Box::pin(async move {
            let events = if first {
                vec![
                    Ok(ProviderEvent::ToolUse {
                        id: "read-history".into(),
                        name: TOOL.into(),
                        input_json: serde_json::to_string(&request()).unwrap(),
                    }),
                    Ok(ProviderEvent::Done {
                        stop_reason: StopReason::ToolUse,
                    }),
                ]
            } else {
                vec![
                    Ok(ProviderEvent::TextDelta("continued".into())),
                    Ok(ProviderEvent::Done {
                        stop_reason: StopReason::End,
                    }),
                ]
            };
            Ok(Box::pin(futures::stream::iter(events)) as crate::EventStream)
        })
    }
}

#[tokio::test]
async fn actual_model_roundtrip_exposes_only_filtered_historical_details() -> TestResult {
    let directory =
        std::env::temp_dir().join(format!("vega-history-wire-{}", ulid::Ulid::generate()));
    std::fs::create_dir(&directory)?;
    let tools = vega_tools::Tools::new(&directory)?;
    let provider = HistoryProvider(Mutex::new(Vec::new()));
    let outcome = run_agent(&provider,&tools,AgentRequest {
        model:"synthetic".into(),system_prompt:"system".into(),history:vec![ChatMessage::new(ChatRole::User,"continue")],max_tokens:None,completed_tool_results:Default::default(),
        tool_config:RuntimeToolConfig::default().with_conversation_history_reader(Arc::new(|_| Ok(serde_json::json!({"records":[{"content":"historical-detail synthetic-private-key"}]})))).with_credential_reader(Arc::new(|| Ok(vec!["synthetic-private-key".into()]))),
        pricing_catalog:None,reasoning:None,model_snapshot:None,replay_frame:None,context_budget:None,context_source_version:None,context_source_fingerprint:None,context_operation_id:None,context_compaction_hook:None,
    },CancellationToken::new()).await?;
    assert!(!outcome.failed);
    assert_eq!(outcome.executed_tool_call_count, 1);
    let requests = provider.0.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(
        requests[0]
            .tools
            .iter()
            .any(|definition| definition.name == TOOL)
    );
    let output = requests[1]
        .messages
        .iter()
        .find(|message| message.tool_call_id.as_deref() == Some("read-history"))
        .ok_or("tool result")?;
    assert!(output.content.contains("historical-detail"));
    assert!(output.content.contains("conversation_details"));
    assert!(!output.content.contains("synthetic-private-key"));
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

fn request() -> ConversationReadRequest {
    ConversationReadRequest {
        kind: "messages".into(),
        source_id: None,
        after_sequence: 0,
        limit: 2,
        content_offset: 0,
    }
}

#[test]
fn history_filters_complete_fields_before_unicode_slicing_and_enforces_bounds() -> TestResult {
    let config = RuntimeToolConfig::default()
        .with_credential_reader(Arc::new(|| Ok(vec!["synthetic-sensitive-key".into()])));
    let mut input = request();
    input.content_offset = 1018;
    let text = format!("{}synthetic-sensitive-key尾", "界".repeat(1020));
    let output = project_page(
        serde_json::json!({"records":[{"content":text}]}),
        &input,
        &config,
    )?;
    assert!(!output.contains("synthetic-sensitive-key"));
    assert!(!output.contains("ensitive-key"));
    let value: Value = serde_json::from_str(&output)?;
    assert_eq!(value["vega_history_observation"]["authority"], "none");
    assert_eq!(
        value["vega_history_observation"]["page"]["records"][0]["content"]["complete"],
        false
    );
    let value: Value = serde_json::from_str(&project_page(
        serde_json::json!({"records":[{"content":"x".repeat(2000),"output":null}]}),
        &request(),
        &config,
    )?)?;
    let row = &value["vega_history_observation"]["page"]["records"][0];
    assert_eq!(row["content"]["text"].as_str().ok_or("text")?.len(), 1024);
    assert_eq!(row["content"]["next_offset"], 1024);
    assert_eq!(row["output"]["unavailable"], true);
    assert!(
        project_page(
            serde_json::json!({"records":[{},{},{}]}),
            &request(),
            &config
        )
        .is_err()
    );
    assert!(
        project_page(
            serde_json::json!({"records":[{"source_id":"x".repeat(65536)}]}),
            &request(),
            &config
        )
        .is_err()
    );
    let unavailable = config.with_credential_reader(Arc::new(|| Err(())));
    assert!(
        project_page(
            serde_json::json!({"records":[{"content":"safe"}]}),
            &request(),
            &unavailable
        )
        .is_err()
    );
    for bad in [
        "{}",
        "{\"kind\":\"messages\",\"source_id\":null,\"after_sequence\":0,\"limit\":9,\"content_offset\":0}",
        "{\"kind\":\"messages\",\"source_id\":null,\"after_sequence\":0,\"limit\":1,\"content_offset\":0,\"thread_id\":\"other\"}",
    ] {
        assert!(ConversationReadRequest::parse(bad).is_none());
    }
    Ok(())
}

#[tokio::test]
async fn history_dispatch_is_conditional_readonly_filtered_and_cancellable() -> TestResult {
    let directory = std::env::temp_dir().join(format!("vega-history-{}", ulid::Ulid::generate()));
    std::fs::create_dir(&directory)?;
    let tools = vega_tools::Tools::new(&directory)?;
    let config = RuntimeToolConfig::default()
        .with_conversation_history_reader(Arc::new(|_| {
            Ok(serde_json::json!({"records":[{"content":"private-history-value"}]}))
        }))
        .with_credential_reader(Arc::new(|| Ok(vec!["private-history-value".into()])));
    let capabilities =
        RunCapabilitySnapshot::freeze(config.run_mode, config.permission_mode, Vec::new())?
            .with_conversation_history(true);
    assert!(
        capabilities
            .definitions()
            .iter()
            .any(|definition| definition.name == TOOL)
    );
    let call = || RuntimeToolCall {
        id: "history-call".into(),
        name: TOOL.into(),
        input_json: serde_json::to_string(&request()).unwrap(),
        description: None,
    };
    let prepared = prepare_runtime_call(&tools, &config, &capabilities, call())?;
    assert!(matches!(
        prepared,
        PreparedRuntimeCall::ConversationHistory { .. }
    ));
    let cancel = CancellationToken::new();
    assert!(matches!(
        super::super::tools_exec::authorize_call(
            &prepared,
            &config,
            &HashSet::new(),
            &super::super::RejectPermissionHook,
            &cancel
        )
        .await?,
        super::super::tools_exec::Authorization::Approved { .. }
    ));
    let (result, cancelled) = execute_prepared_waiting(
        prepared,
        &tools,
        &super::super::RejectPermissionHook,
        &cancel,
    )
    .await;
    assert!(!cancelled);
    assert_eq!(result.status, RuntimeToolStatus::Success);
    assert!(!result.output.contains("private-history-value"));
    assert!(result.output.contains("conversation_details"));
    let disabled =
        RunCapabilitySnapshot::freeze(config.run_mode, config.permission_mode, Vec::new())?;
    assert!(matches!(
        prepare_runtime_call(&tools, &config, &disabled, call())?,
        PreparedRuntimeCall::Unknown(_)
    ));
    let cancel = CancellationToken::new();
    cancel.cancel();
    let (result, cancelled) = execute_prepared_waiting(
        prepare_runtime_call(&tools, &config, &capabilities, call())?,
        &tools,
        &super::super::RejectPermissionHook,
        &cancel,
    )
    .await;
    assert!(cancelled);
    assert_eq!(result.status, RuntimeToolStatus::Cancelled);
    std::fs::remove_dir_all(directory)?;
    Ok(())
}
