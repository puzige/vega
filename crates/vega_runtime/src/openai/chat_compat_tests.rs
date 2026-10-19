use super::*;
use crate::{
    AgentOutcome, AgentRequest, ChatMessage, ChatRole, FrozenReasoning, ReasoningReplayPolicy,
    ReplayFrame, RuntimeEvent, RuntimeToolConfig, RuntimeToolStatus,
};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Mutex;
use tempfile::TempDir;

struct Script {
    chunks: Vec<Value>,
    extra: Vec<ProviderEvent>,
    finalize: bool,
}

impl Script {
    fn read(id: &str, reasoning: Option<&str>) -> Self {
        let mut delta = json!({"role":"assistant"});
        if let Some(reasoning) = reasoning {
            delta["reasoning_content"] = json!(reasoning);
        }
        Self {
            chunks: vec![
                json!({"choices":[{"delta":delta}]}),
                json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":id,"type":"function","function":{"name":"read","arguments":"{\"path\":\"sample.txt\",\"offset\":null,\"limit\":null}"}}]}}]}),
                json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]}),
            ],
            extra: Vec::new(),
            finalize: true,
        }
    }

    fn final_reply(text: &str, reasoning: Option<&str>) -> Self {
        let mut delta = json!({"content":text});
        if let Some(reasoning) = reasoning {
            delta["reasoning_content"] = json!(reasoning);
        }
        Self {
            chunks: vec![json!({"choices":[{"delta":delta,"finish_reason":"stop"}]})],
            extra: Vec::new(),
            finalize: true,
        }
    }
}

struct ScriptedProvider {
    scripts: Mutex<VecDeque<Script>>,
    bodies: Arc<Mutex<Vec<Value>>>,
}

impl ScriptedProvider {
    fn new(scripts: Vec<Script>) -> Self {
        Self {
            scripts: Mutex::new(scripts.into()),
            bodies: Default::default(),
        }
    }

    fn bodies(&self) -> Vec<Value> {
        self.bodies.lock().expect("captured requests").clone()
    }
}

impl Provider for ScriptedProvider {
    fn chat_stream(
        &self,
        request: ChatRequest,
        _: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        self.bodies
            .lock()
            .expect("request capture")
            .push(build_request_body(&request));
        let script = self.scripts.lock().expect("script queue").pop_front();
        Box::pin(async move {
            let script = script.expect("unexpected provider attempt");
            let mut assembler = sse::SseAssembler::default();
            let mut events = Vec::new();
            let mut complete = true;
            for chunk in script.chunks {
                match assembler.absorb(&chunk.to_string()) {
                    Ok(chunk_events) => events.extend(chunk_events.into_iter().map(Ok)),
                    Err(error) => {
                        events.push(Err(error));
                        complete = false;
                        break;
                    }
                }
            }
            if complete {
                events.extend(script.extra.into_iter().map(Ok));
                if script.finalize {
                    match assembler.finalize() {
                        Ok(terminal) => events.extend(terminal.into_iter().map(Ok)),
                        Err(error) => events.push(Err(error)),
                    }
                }
            }
            Ok(Box::pin(futures::stream::iter(events)) as EventStream)
        })
    }
}

fn fixture() -> (TempDir, vega_tools::Tools) {
    let root = tempfile::tempdir().expect("owned tool fixture");
    std::fs::write(root.path().join("sample.txt"), "fixture result\n").expect("fixture file");
    let tools = vega_tools::Tools::new(root.path()).expect("owned tools");
    (root, tools)
}

fn legacy_step() -> FrozenReasoning {
    FrozenReasoning {
        provider: "cpa".into(),
        model: "step-5-preview".into(),
        protocol: ReasoningProtocol::OpenAiChatCompletions,
        replay_policy: ReasoningReplayPolicy::StandardChat,
        choice: ReasoningChoice::Effort("high".into()),
        supports_disabled: true,
        disabled_wire: Some(ReasoningDisabledWire::ReasoningEffortNone),
        declared_efforts: ["minimal", "low", "medium", "high", "xhigh", "max"]
            .into_iter()
            .map(str::to_string)
            .collect(),
    }
}

fn request(reasoning: Option<FrozenReasoning>) -> AgentRequest {
    AgentRequest {
        model: reasoning
            .as_ref()
            .map_or("step-5-preview", |profile| profile.model.as_str())
            .to_string(),
        system_prompt: "Use the file and answer.".into(),
        history: vec![ChatMessage::new(ChatRole::User, "Read sample.txt")],
        max_tokens: Some(256),
        completed_tool_results: HashMap::new(),
        tool_config: RuntimeToolConfig::default().with_turn_limit(6),
        pricing_catalog: None,
        reasoning,
        replay_frame: None,
        context_budget: None,
        context_source_version: None,
        context_source_fingerprint: None,
        context_operation_id: None,
        context_compaction_hook: None,
    }
}

fn tool_messages(body: &Value) -> Vec<&Value> {
    body["messages"]
        .as_array()
        .expect("wire messages")
        .iter()
        .filter(|message| message["role"] == "assistant" && message.get("tool_calls").is_some())
        .collect()
}

fn assert_no_tool_side_effects(outcome: &AgentOutcome) {
    assert!(outcome.failed);
    assert_eq!(outcome.executed_tool_call_count, 0);
    assert!(!outcome.events.iter().any(|event| matches!(
        event,
        RuntimeEvent::ToolCallApproved { .. }
            | RuntimeEvent::ToolCallRunning { .. }
            | RuntimeEvent::ToolCallOutput { .. }
            | RuntimeEvent::ToolCallFinished(_)
    )));
    assert!(
        outcome.messages.iter().all(|message| {
            message.reasoning_content.is_none() && message.tool_calls.is_empty()
        })
    );
}

#[tokio::test]
async fn standard_chat_completes_successive_tools_with_exact_reasoning_and_results() {
    let (_root, tools) = fixture();
    let provider = ScriptedProvider::new(vec![
        Script::read("first", Some("first raw\n思考")),
        Script::read("second", Some("second raw")),
        Script::final_reply("complete", Some("final raw")),
    ]);
    let outcome = crate::run_agent(
        &provider,
        &tools,
        request(Some(legacy_step())),
        CancellationToken::new(),
    )
    .await
    .expect("agent outcome");
    assert!(!outcome.failed);
    assert_eq!(outcome.final_text, "complete");
    assert_eq!(outcome.executed_tool_call_count, 2);
    let finished = outcome
        .events
        .iter()
        .filter_map(|event| match event {
            RuntimeEvent::ToolCallFinished(result) => Some(result),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(finished.len(), 2);
    assert_eq!(finished[0].call_id, "first");
    assert_eq!(finished[1].call_id, "second");
    assert!(finished.iter().all(|result| {
        result.status == RuntimeToolStatus::Success && result.output.contains("fixture result")
    }));
    let bodies = provider.bodies();
    assert_eq!(bodies.len(), 3);
    for body in &bodies {
        assert_eq!(body["reasoning_effort"], "high");
        assert!(body.get("thinking").is_none());
    }
    let calls = tool_messages(&bodies[2]);
    assert_eq!(calls.len(), 2);
    for (index, (id, raw)) in [("first", "first raw\n思考"), ("second", "second raw")]
        .into_iter()
        .enumerate()
    {
        assert_eq!(calls[index]["reasoning_content"], raw);
        assert_eq!(calls[index]["tool_calls"][0]["id"], id);
        assert_eq!(calls[index]["tool_calls"][0]["function"]["name"], "read");
        assert_eq!(
            calls[index]["tool_calls"][0]["function"]["arguments"],
            "{\"path\":\"sample.txt\",\"offset\":null,\"limit\":null}"
        );
        let result = bodies[2]["messages"]
            .as_array()
            .expect("messages")
            .iter()
            .find(|message| message["tool_call_id"] == id)
            .expect("wire result");
        assert_eq!(result["role"], "tool");
        assert!(
            result["content"]
                .as_str()
                .expect("result text")
                .contains("fixture result")
        );
    }
}

#[tokio::test]
async fn generic_chat_preserves_absence_empty_and_original_text_without_control_injection() {
    let (_root, tools) = fixture();
    for declaration in [
        None,
        Some(FrozenReasoning::unknown("custom", "step-5-preview")),
        {
            let mut standard = legacy_step();
            standard.choice = ReasoningChoice::ProviderDefault;
            Some(standard)
        },
    ] {
        for raw in [None, Some(""), Some("raw ∑\nreasoning")] {
            let provider = ScriptedProvider::new(vec![
                Script::read("one", raw),
                Script::final_reply("done", None),
            ]);
            let outcome = crate::run_agent(
                &provider,
                &tools,
                request(declaration.clone()),
                CancellationToken::new(),
            )
            .await
            .expect("generic outcome");
            assert!(
                !outcome.failed,
                "declaration {declaration:?}, presence {raw:?}"
            );
            assert_eq!(outcome.final_text, "done");
            assert_eq!(outcome.executed_tool_call_count, 1);
            let bodies = provider.bodies();
            let calls = tool_messages(&bodies[1]);
            assert_eq!(calls.len(), 1);
            assert_eq!(
                calls[0].get("reasoning_content"),
                raw.map(|text| json!(text)).as_ref()
            );
            assert!(bodies.iter().all(|body| {
                body.get("thinking").is_none() && body.get("reasoning_effort").is_none()
            }));
        }
    }
}

#[tokio::test]
async fn raw_chat_state_survives_valid_warm_continuation_but_not_a_changed_route() {
    let (_root, tools) = fixture();
    let provider = ScriptedProvider::new(vec![
        Script::read("warm-read", Some("tool original")),
        Script::final_reply("first final", Some("final original")),
    ]);
    let mut initial = request(None);
    let mut frame = ReplayFrame::new(2, ReasoningReplayPolicy::Unknown);
    frame.attach_route_fingerprint([7; 32]);
    initial.replay_frame = Some(frame);
    let outcome = crate::run_agent(&provider, &tools, initial, CancellationToken::new())
        .await
        .expect("initial outcome");
    assert!(!outcome.failed);
    let initial_frame = outcome.messages[0].replay_frame().expect("frame");
    assert!(initial_frame.seal_eligible());
    let environment = initial_frame
        .environment_fingerprint()
        .expect("environment");
    assert_eq!(
        outcome
            .messages
            .last()
            .expect("final")
            .reasoning_content
            .as_deref(),
        Some("final original")
    );
    for (route, warm) in [([7; 32], true), ([8; 32], false)] {
        let provider = ScriptedProvider::new(vec![Script::final_reply("continued", None)]);
        let mut next = request(None);
        next.history = outcome.messages[1..]
            .iter()
            .map(ChatMessage::without_runtime_metadata)
            .collect();
        next.history
            .push(ChatMessage::new(ChatRole::User, "Use the earlier result"));
        let mut next_frame = ReplayFrame::new(next.history.len(), ReasoningReplayPolicy::Unknown);
        next_frame.attach_route_fingerprint(route);
        next_frame.attach_candidate(outcome.messages.clone(), environment);
        next.replay_frame = Some(next_frame);
        let continued = crate::run_agent(&provider, &tools, next, CancellationToken::new())
            .await
            .expect("continued outcome");
        assert!(!continued.failed);
        assert_eq!(continued.executed_tool_call_count, 0);
        let bodies = provider.bodies();
        let messages = bodies[0]["messages"]
            .as_array()
            .expect("continued messages");
        if warm {
            assert!(
                messages
                    .iter()
                    .any(|message| message["reasoning_content"] == "tool original")
            );
            assert!(
                messages
                    .iter()
                    .any(|message| message["reasoning_content"] == "final original")
            );
        } else {
            assert!(
                messages
                    .iter()
                    .all(|message| message.get("reasoning_content").is_none())
            );
        }
    }
}

#[tokio::test]
async fn required_provider_state_and_foreign_responses_fail_before_tool_side_effects() {
    let (_root, tools) = fixture();
    for (protocol, replay_policy, model) in [
        (
            ReasoningProtocol::DeepSeekChatCompletions,
            ReasoningReplayPolicy::DeepSeekTools,
            "deepseek-v4-pro",
        ),
        (
            ReasoningProtocol::ZhipuChatCompletions,
            ReasoningReplayPolicy::GlmPreserved,
            "glm-5.3",
        ),
        (
            ReasoningProtocol::ZhipuChatCompletions,
            ReasoningReplayPolicy::GlmInterleaved,
            "glm-4.7",
        ),
        (
            ReasoningProtocol::ResponsesApi,
            ReasoningReplayPolicy::ResponsesItems,
            "responses-model",
        ),
    ] {
        let mut profile = FrozenReasoning::unknown("official", model);
        profile.protocol = protocol;
        profile.replay_policy = replay_policy;
        let provider = ScriptedProvider::new(vec![Script::read("must-not-run", None)]);
        let outcome = crate::run_agent(
            &provider,
            &tools,
            request(Some(profile)),
            CancellationToken::new(),
        )
        .await
        .expect("strict outcome");
        assert_no_tool_side_effects(&outcome);
        assert_eq!(provider.bodies().len(), 1);
    }
    for declaration in [None, Some(legacy_step())] {
        for foreign_layout in [false, true] {
            let mut script = Script::read("foreign", Some("chat original"));
            script.extra = if foreign_layout {
                vec![ProviderEvent::ResponseReplay(crate::ResponseReplayLayout {
                    response_id: Some("foreign-response".into()),
                    slots: Vec::new(),
                })]
            } else {
                vec![ProviderEvent::ReasoningReplay(vec![
                    json!({"type":"reasoning","encrypted_content":"opaque sentinel"}),
                ])]
            };
            let provider = ScriptedProvider::new(vec![script]);
            let outcome = crate::run_agent(
                &provider,
                &tools,
                request(declaration.clone()),
                CancellationToken::new(),
            )
            .await
            .expect("foreign outcome");
            assert_no_tool_side_effects(&outcome);
            assert_eq!(provider.bodies().len(), 1);
        }
    }
}

#[tokio::test]
async fn unsupported_saved_step_choice_is_rejected_before_any_provider_request() {
    let (_root, tools) = fixture();
    for choice in [
        ReasoningChoice::Disabled,
        ReasoningChoice::Effort("max".into()),
    ] {
        let mut profile = legacy_step();
        profile.supports_disabled = false;
        profile.disabled_wire = None;
        profile.declared_efforts = ["low", "medium", "high"]
            .into_iter()
            .map(str::to_string)
            .collect();
        profile.choice = choice;
        let provider = ScriptedProvider::new(Vec::new());
        let result = crate::run_agent(
            &provider,
            &tools,
            request(Some(profile)),
            CancellationToken::new(),
        )
        .await;
        assert!(matches!(
            result,
            Err(VegaError::ReasoningSelectionInvalid { .. })
        ));
        assert!(provider.bodies().is_empty());
    }
}

#[tokio::test]
async fn malformed_reasoning_and_incomplete_streams_never_execute_partial_tools() {
    let (_root, tools) = fixture();
    for malformed in [json!(17), json!({"text":"not a string"})] {
        let mut script = Script::read("partial", None);
        script.chunks.insert(
            2,
            json!({"choices":[{"delta":{"reasoning_content":malformed}}]}),
        );
        let provider = ScriptedProvider::new(vec![script]);
        let outcome = crate::run_agent(&provider, &tools, request(None), CancellationToken::new())
            .await
            .expect("malformed outcome");
        assert_no_tool_side_effects(&outcome);
    }
    for finalize in [false, true] {
        let mut script = Script::read("partial", Some("partial state"));
        script.chunks.pop();
        script.finalize = finalize;
        let provider = ScriptedProvider::new(vec![script]);
        let outcome = crate::run_agent(&provider, &tools, request(None), CancellationToken::new())
            .await
            .expect("incomplete outcome");
        assert_no_tool_side_effects(&outcome);
    }
}

#[test]
fn optional_reasoning_without_tools_keeps_default_wire_and_has_no_replay_contract() {
    let body = build_request_body(&ChatRequest {
        model: "custom-model".into(),
        messages: vec![ChatMessage::new(ChatRole::User, "answer")],
        tools: Vec::new(),
        max_tokens: None,
        reasoning: None,
    });
    assert!(body.get("tools").is_none());
    assert!(body.get("thinking").is_none());
    assert!(body.get("reasoning_effort").is_none());
    assert!(body["messages"][0].get("reasoning_content").is_none());
    for policy in [
        ReasoningReplayPolicy::Unknown,
        ReasoningReplayPolicy::StandardChat,
        ReasoningReplayPolicy::LegacyTextTools,
    ] {
        assert!(!policy.preserves_chat_response(false));
    }
}

#[test]
fn reasoning_debug_output_never_contains_original_chat_or_opaque_state() {
    let message = ChatMessage::assistant_with_tools_and_reasoning(
        "visible",
        Some("private chat sentinel".into()),
        Vec::new(),
    );
    assert!(!format!("{message:?}").contains("private chat sentinel"));
    let event = ProviderEvent::ReasoningReplay(vec![
        json!({"type":"reasoning","encrypted_content":"private opaque sentinel"}),
    ]);
    assert!(!format!("{event:?}").contains("private opaque sentinel"));
}
