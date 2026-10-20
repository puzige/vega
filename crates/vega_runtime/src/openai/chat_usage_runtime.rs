use super::http::{HttpFixture, Reply};
use super::*;
use crate::model_compat::{
    ModelApi, ModelIdentity, ModelRoute, ResolvedModel, freeze_choice, lookup_model_definition,
    resolve_model,
};
use crate::{
    AgentRequest, ChatMessage, ChatRole, CompletedToolCall, OpenAiProvider, ReasoningChoice,
    RuntimeEvent, RuntimePermissionMode, RuntimeRunMode, RuntimeToolConfig, RuntimeToolStatus,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

fn bound_provider(base: &str) -> (OpenAiProvider, ResolvedModel) {
    let definition = lookup_model_definition(
        &ModelRoute::new(ModelApi::ChatCompletions, base).unwrap(),
        &ModelIdentity {
            provider: "synthetic".into(),
            model: "deepseek-v4.1-flash".into(),
        },
    )
    .unwrap();
    let model = resolve_model(&definition, None).unwrap();
    let provider = OpenAiProvider::new(base, "synthetic-key")
        .unwrap()
        .with_retry_policy(crate::RetryPolicy {
            max_retries: 0,
            ..Default::default()
        })
        .with_model_snapshot(model.clone())
        .unwrap();
    (provider, model)
}

fn request(model: &ResolvedModel, root: &std::path::Path) -> AgentRequest {
    let reasoning = freeze_choice(model, ReasoningChoice::ProviderDefault).unwrap();
    let mut frame = crate::ReplayFrame::new(1, reasoning.replay_policy);
    frame.attach_route_fingerprint([7; 32]);
    AgentRequest {
        model: model.identity.model.clone(),
        system_prompt: "Read only the owned workspace.".into(),
        history: vec![ChatMessage::new(
            ChatRole::User,
            "Read alpha.txt and beta.txt.",
        )],
        max_tokens: Some(256),
        completed_tool_results: HashMap::new(),
        tool_config: RuntimeToolConfig::new(
            RuntimeRunMode::Execute,
            RuntimePermissionMode::ReadOnly,
            "synthetic-project".into(),
            "synthetic-thread".into(),
            root.join("checkpoints"),
            Vec::new(),
        ),
        pricing_catalog: None,
        reasoning: Some(reasoning),
        model_snapshot: Some(Box::new(model.clone())),
        replay_frame: Some(frame),
        context_budget: None,
        context_source_version: None,
        context_source_fingerprint: None,
        context_operation_id: None,
        context_compaction_hook: None,
    }
}

fn final_reply(text: &str) -> Reply {
    Reply::events(&[chunk(json!({"content":text}), json!("stop"))])
}

fn owned_files() -> (tempfile::TempDir, vega_tools::Tools) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("alpha.txt"), "alpha owned result\n").unwrap();
    std::fs::write(dir.path().join("beta.txt"), "beta owned result\n").unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    (dir, tools)
}

#[tokio::test]
async fn cache_alias_handoff_reads_owned_files_and_reuses_real_results_on_followup() {
    let (dir, tools) = owned_files();
    let round = two_reads(alias_usage());
    let mut fixture = HttpFixture::new(vec![
        Reply::events(&round),
        final_reply("Complete."),
        Reply::events(&round),
        final_reply("Again complete."),
    ])
    .await;
    let (provider, model) = bound_provider(&fixture.base);
    let thinking = Arc::new(Mutex::new(String::new()));
    let observed = thinking.clone();
    let first = tokio::time::timeout(
        Duration::from_secs(10),
        crate::run_agent_with_sink(
            &provider,
            &tools,
            request(&model, dir.path()),
            CancellationToken::new(),
            move |event| {
                if let RuntimeEvent::ThinkingDelta(delta) = event {
                    observed.lock().unwrap().push_str(&delta);
                }
                async { Ok(()) }
            },
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(!first.failed && !first.interrupted);
    assert_eq!(first.final_text, "Inspecting files. Complete.");
    assert_eq!(*thinking.lock().unwrap(), "plan reads");
    assert_eq!(first.tool_call_count, 2);
    assert_eq!(first.executed_tool_call_count, 2);
    let proposals: Vec<_> = first
        .events
        .iter()
        .filter_map(|event| match event {
            RuntimeEvent::ToolCallProposed(call) => Some(call.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        proposals
            .iter()
            .map(|call| (
                call.id.as_str(),
                call.name.as_str(),
                call.input_json.as_str()
            ))
            .collect::<Vec<_>>(),
        vec![
            (CALLS[0].0, "read", CALLS[0].1),
            (CALLS[1].0, "read", CALLS[1].1)
        ]
    );
    let results: HashMap<_, _> = first
        .events
        .iter()
        .filter_map(|event| match event {
            RuntimeEvent::ToolCallFinished(result) => {
                Some((result.call_id.clone(), result.clone()))
            }
            _ => None,
        })
        .collect();
    for (id, expected) in [
        (CALLS[0].0, "alpha owned result"),
        (CALLS[1].0, "beta owned result"),
    ] {
        let result = results.get(id).unwrap();
        assert_eq!(result.status, RuntimeToolStatus::Success);
        assert!(!result.reused);
        assert!(result.output.contains(expected));
        assert_eq!(first.events.iter().filter(|event| matches!(event, RuntimeEvent::ToolCallRunning { call_id } if call_id == id)).count(), 1);
    }
    let usage: Vec<_> = first
        .events
        .iter()
        .filter_map(|event| match event {
            RuntimeEvent::UsageUpdated { usage, .. } => Some((
                usage.input,
                usage.output,
                usage.cache_read,
                usage.cache_write,
            )),
            _ => None,
        })
        .collect();
    assert_eq!(usage, vec![(100, 9, Some(70), None)]);
    let mut next = request(&model, dir.path());
    next.completed_tool_results = proposals
        .into_iter()
        .map(|call| {
            let result = results.get(&call.id).unwrap().clone();
            (
                call.id,
                CompletedToolCall {
                    tool: call.name,
                    input_json: call.input_json,
                    result,
                },
            )
        })
        .collect();
    next.history = first
        .messages
        .iter()
        .skip(1)
        .map(ChatMessage::without_live_tool_arguments)
        .collect();
    next.history.push(ChatMessage::new(
        ChatRole::User,
        "Continue using the completed reads.",
    ));
    let mut frame = crate::ReplayFrame::new(
        next.history.len(),
        next.reasoning.as_ref().unwrap().replay_policy,
    );
    frame.attach_route_fingerprint([7; 32]);
    next.replay_frame = Some(frame);
    let continued = tokio::time::timeout(
        Duration::from_secs(10),
        crate::run_agent(&provider, &tools, next, CancellationToken::new()),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(!continued.failed && !continued.interrupted);
    assert_eq!(continued.final_text, "Inspecting files. Again complete.");
    assert_eq!(continued.executed_tool_call_count, 0);
    assert_eq!(continued.events.iter().filter(|event| matches!(event, RuntimeEvent::ToolCallFinished(result) if result.reused && result.status == RuntimeToolStatus::Success)).count(), 2);
    assert!(
        !continued
            .events
            .iter()
            .any(|event| matches!(event, RuntimeEvent::ToolCallRunning { .. }))
    );
    {
        let requests = fixture.received.lock().unwrap();
        assert_eq!(requests.len(), 4);
        assert!(
            requests
                .iter()
                .all(|request| request.path == "/v1/chat/completions"
                    && request.body["model"] == "deepseek-v4.1-flash")
        );
        let messages = requests[1].body["messages"].as_array().unwrap();
        let assistant = messages
            .iter()
            .find(|message| message.get("tool_calls").is_some())
            .unwrap();
        for (index, (id, args)) in CALLS.iter().enumerate() {
            assert_eq!(assistant["tool_calls"][index]["id"], *id);
            assert_eq!(assistant["tool_calls"][index]["function"]["name"], "read");
            assert_eq!(
                assistant["tool_calls"][index]["function"]["arguments"],
                *args
            );
            let tool = messages
                .iter()
                .find(|message| message["role"] == "tool" && message["tool_call_id"] == *id)
                .unwrap();
            assert_eq!(tool["content"], results.get(*id).unwrap().output);
        }
    }
    fixture.finish().await;
    assert_eq!(
        std::fs::read_to_string(dir.path().join("alpha.txt")).unwrap(),
        "alpha owned result\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("beta.txt")).unwrap(),
        "beta owned result\n"
    );
}

#[tokio::test]
async fn malformed_usage_or_tools_never_execute_buffered_reads_or_finish_successfully() {
    let (dir, tools) = owned_files();
    for (label, chunks) in invalid_rounds() {
        let mut fixture = HttpFixture::new(vec![Reply::events(&chunks)]).await;
        let (provider, model) = bound_provider(&fixture.base);
        let outcome = tokio::time::timeout(
            Duration::from_secs(10),
            crate::run_agent(
                &provider,
                &tools,
                request(&model, dir.path()),
                CancellationToken::new(),
            ),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(outcome.failed && !outcome.interrupted, "{label}");
        assert_eq!(outcome.executed_tool_call_count, 0, "{label}");
        assert!(outcome.events.iter().any(|event| matches!(event, RuntimeEvent::Error(error) if matches!(error.as_ref(), VegaError::ProviderDiagnostic { kind: ProviderFailureKind::Protocol, .. }))), "{label}");
        assert!(
            !outcome.events.iter().any(|event| matches!(
                event,
                RuntimeEvent::ToolCallRunning { .. } | RuntimeEvent::ToolCallFinished(_)
            )),
            "{label}"
        );
        {
            let received = fixture.received.lock().unwrap();
            assert_eq!(received.len(), 1, "{label}");
            assert_eq!(received[0].path, "/v1/chat/completions", "{label}");
        }
        fixture.finish().await;
        assert_eq!(
            std::fs::read_to_string(dir.path().join("alpha.txt")).unwrap(),
            "alpha owned result\n",
            "{label}"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("beta.txt")).unwrap(),
            "beta owned result\n",
            "{label}"
        );
    }
}

#[tokio::test]
async fn cancellation_before_send_or_after_visible_delta_prevents_read_side_effects() {
    let (dir, tools) = owned_files();
    for before_send in [true, false] {
        let replies = if before_send {
            Vec::new()
        } else {
            vec![Reply::events(&two_reads(equal_usage()))]
        };
        let mut fixture = HttpFixture::new(replies).await;
        let (provider, model) = bound_provider(&fixture.base);
        let cancel = CancellationToken::new();
        let stop = cancel.clone();
        if before_send {
            cancel.cancel();
        }
        let result = tokio::time::timeout(
            Duration::from_secs(10),
            crate::run_agent_with_sink(
                &provider,
                &tools,
                request(&model, dir.path()),
                cancel,
                move |event| {
                    if matches!(event, RuntimeEvent::TextDelta(_)) {
                        stop.cancel();
                    }
                    async { Ok(()) }
                },
            ),
        )
        .await
        .unwrap();
        match result {
            Err(VegaError::Cancelled) if before_send => {}
            Ok(outcome) => {
                assert!(outcome.interrupted && !outcome.failed);
                assert_eq!(outcome.executed_tool_call_count, 0);
                assert!(
                    !outcome
                        .events
                        .iter()
                        .any(|event| matches!(event, RuntimeEvent::ToolCallRunning { .. }))
                );
            }
            other => panic!("unexpected cancellation outcome: {other:?}"),
        }
        fixture.finish().await;
    }
}
