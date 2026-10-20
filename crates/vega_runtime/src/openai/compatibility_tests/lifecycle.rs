use super::*;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use crate::{RuntimeEvent, RuntimePermissionMode};

#[derive(Clone)]
struct ScriptedProvider {
    endpoint: String,
    responses: bool,
    scripts: Arc<Mutex<VecDeque<Vec<Value>>>>,
    requests: Arc<Mutex<Vec<ChatRequest>>>,
    bodies: Arc<Mutex<Vec<Value>>>,
}

impl ScriptedProvider {
    fn new(model: &ResolvedModel, scripts: Vec<Vec<Value>>, responses: bool) -> Self {
        Self {
            endpoint: if responses {
                "https://response.invalid/v1"
            } else {
                "https://fixture.invalid/v1"
            }
            .into(),
            responses,
            scripts: Arc::new(Mutex::new(scripts.into())),
            requests: Arc::new(Mutex::new(Vec::new())),
            bodies: Arc::new(Mutex::new(Vec::new())),
        }
        .checked_model(model)
    }

    fn checked_model(self, model: &ResolvedModel) -> Self {
        assert!(!model.identity.model.is_empty());
        self
    }
}

impl Provider for ScriptedProvider {
    fn chat_stream(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        let state = self.clone();
        Box::pin(async move {
            if cancel.is_cancelled() {
                return Err(VegaError::Cancelled);
            }
            let body = super::super::encode_request(&state.endpoint, &request, state.responses)?;
            state.requests.lock().unwrap().push(request);
            state.bodies.lock().unwrap().push(body);
            let script = state
                .scripts
                .lock()
                .unwrap()
                .pop_front()
                .expect("bounded provider script");
            let events = if state.responses {
                decode_responses(&script)
            } else {
                decode_chat(&script, true)
            };
            let items: Vec<Result<ProviderEvent, VegaError>> = match events {
                Ok(events) => events.into_iter().map(Ok).collect(),
                Err(error) => vec![Err(error)],
            };
            Ok(Box::pin(futures::stream::iter(items)) as EventStream)
        })
    }
}

fn agent_request(model: &ResolvedModel, root: &std::path::Path) -> AgentRequest {
    let mut frame = crate::ReplayFrame::new(
        1,
        freeze_choice(model, ReasoningChoice::ProviderDefault)
            .unwrap()
            .replay_policy,
    );
    frame.attach_route_fingerprint([7; 32]);
    AgentRequest {
        model: model.identity.model.clone(),
        system_prompt: "Use the owned test workspace.".into(),
        history: vec![ChatMessage::new(
            ChatRole::User,
            "Read README.md and report the result.",
        )],
        max_tokens: Some(256),
        completed_tool_results: HashMap::new(),
        tool_config: RuntimeToolConfig::new(
            RuntimeRunMode::Execute,
            RuntimePermissionMode::ReadOnly,
            "project".into(),
            "thread".into(),
            root.join("checkpoints"),
            Vec::new(),
        ),
        pricing_catalog: None,
        reasoning: Some(freeze_choice(model, ReasoningChoice::ProviderDefault).unwrap()),
        model_snapshot: Some(Box::new(model.clone())),
        replay_frame: Some(frame),
        context_budget: None,
        context_source_version: None,
        context_source_fingerprint: None,
        context_operation_id: None,
        context_compaction_hook: None,
    }
}

fn read_round(raw: Value, call_id: &str, arguments: &str) -> Vec<Value> {
    let mut delta = raw;
    delta["tool_calls"] = json!([{"index":0,"id":call_id,"type":"function","function":{"name":"read","arguments":arguments}}]);
    vec![
        chat_chunk(json!({}), json!("")),
        chat_chunk(delta, json!("tool_calls")),
    ]
}

fn final_round() -> Vec<Value> {
    vec![
        chat_chunk(json!({"content":"Complete"}), Value::Null),
        chat_chunk(json!({}), json!("stop")),
    ]
}

#[tokio::test]
async fn actual_tool_loop_preserves_first_id_original_arguments_raw_and_single_execution() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("README.md"), "owned contents").unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    for name in ["step-5-preview", "deepseek-v4.1-flash"] {
        let model = fixed(
            ModelApi::ChatCompletions,
            "https://fixture.invalid/v1",
            "custom-proxy",
            name,
        );
        let mut chunks = read_round(
            json!({"reasoning_content":"private plan"}),
            "functions.read:0",
            "",
        );
        chunks.pop();
        chunks.push(chat_chunk(json!({"reasoning_content":"private plan","tool_calls":[{"index":0,"id":"functions.read:0","function":{"name":"read","arguments":""}}]}), Value::Null));
        chunks.push(chat_chunk(json!({"tool_calls":[{"index":0,"id":"wire-alias","function":{"name":null,"arguments":"{\"path\":\"README.md\"}"}}]}),json!("tool_calls")));
        let provider = ScriptedProvider::new(&model, vec![chunks, final_round()], false);
        let observed = Arc::new(Mutex::new(Vec::new()));
        let sink = observed.clone();
        let outcome = crate::run_agent_with_sink(
            &provider,
            &tools,
            agent_request(&model, dir.path()),
            CancellationToken::new(),
            move |event| {
                sink.lock().unwrap().push(event);
                async { Ok(()) }
            },
        )
        .await
        .unwrap();
        assert!(!outcome.failed);
        assert_eq!(outcome.final_text, "Complete");
        assert_eq!(outcome.executed_tool_call_count, 1);
        assert_eq!(outcome.tool_call_count, 1);
        let requests = provider.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(
            requests
                .iter()
                .all(|request| request.model_snapshot.as_deref() == Some(&model))
        );
        let tool_call = requests[1]
            .messages
            .iter()
            .flat_map(|message| &message.tool_calls)
            .find(|call| call.id == "functions.read:0")
            .unwrap();
        assert_eq!(tool_call.name, "read");
        assert_eq!(tool_call.input_json, r#"{"path":"README.md"}"#);
        let bodies = provider.bodies.lock().unwrap();
        assert_eq!(
            bodies[1]["messages"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|message| message["tool_calls"].as_array().into_iter().flatten())
                .next()
                .unwrap()["id"],
            "functions.read:0"
        );
        assert!(bodies[1].to_string().contains("private plan"));
        assert_eq!(
            bodies[1]["messages"]
                .as_array()
                .unwrap()
                .iter()
                .find(|message| message["role"] == "tool")
                .unwrap()["tool_call_id"],
            "functions.read:0"
        );
        assert_eq!(
            bodies[1]["messages"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|message| message["tool_calls"].as_array().into_iter().flatten())
                .next()
                .unwrap()["function"]["arguments"],
            r#"{"path":"README.md"}"#
        );
        assert!(bodies[1].to_string().contains("owned contents"));
        assert!(!outcome.final_text.contains("private plan"));
        assert!(!format!("{outcome:?}").contains("private plan"));
        assert_eq!(
            observed
                .lock()
                .unwrap()
                .iter()
                .filter_map(|event| match event {
                    RuntimeEvent::ThinkingDelta(text) => Some(text.as_str()),
                    _ => None,
                })
                .collect::<String>(),
            "private plan"
        );
    }
}

#[tokio::test]
async fn malformed_or_incomplete_proposals_never_start_tools_or_succeed() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("README.md"), "unchanged").unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let model = resolved(json!({}));
    for script in [
        vec![chat_chunk(
            json!({"tool_calls":[{"index":0,"id":"a","function":{"name":"read","arguments":"{\"path\":\"README.md\""}}]}),
            json!("tool_calls"),
        )],
        vec![chat_chunk(
            json!({"tool_calls":[{"index":0,"id":"a","function":{"name":"read","arguments":"{\"path\":\"README.md\"}"}}]}),
            json!(""),
        )],
        vec![chat_chunk(
            json!({"tool_calls":[{"index":0,"id":"same","function":{"name":"read","arguments":"{}"}},{"index":1,"id":"same","function":{"name":"read","arguments":"{}"}}]}),
            json!("tool_calls"),
        )],
        vec![chat_chunk(
            json!({"reasoning":"bad","reasoning_content":"different","tool_calls":[{"index":0,"id":"a","function":{"name":"read","arguments":"{}"}}]}),
            json!("tool_calls"),
        )],
    ] {
        let provider = ScriptedProvider::new(&model, vec![script], false);
        let outcome = crate::run_agent(
            &provider,
            &tools,
            agent_request(&model, dir.path()),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert!(outcome.failed);
        assert_eq!(outcome.executed_tool_call_count, 0);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("README.md")).unwrap(),
            "unchanged"
        );
    }
}

#[tokio::test]
async fn unmodified_json_validation_and_permission_rejection_keep_tools_fenced() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("README.md");
    std::fs::write(&target, "unchanged").unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let model = resolved(json!({}));
    for (name, args) in [
        ("read", r#"{"path":5}"#),
        ("read", r#"{"path":"README.md","offset":null}"#),
        ("read", r#"{"path":"README.md","offset":"1"}"#),
        ("write", r#"{"path":"README.md","content":"changed"}"#),
    ] {
        let proposal = vec![chat_chunk(
            json!({"tool_calls":[{"index":0,"id":"invalid","function":{"name":name,"arguments":args}}]}),
            json!("tool_calls"),
        )];
        let provider = ScriptedProvider::new(&model, vec![proposal, final_round()], false);
        let outcome = crate::run_agent(
            &provider,
            &tools,
            agent_request(&model, dir.path()),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(outcome.executed_tool_call_count, 0, "{name} {args}");
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "unchanged");
        assert!(provider.bodies.lock().unwrap().len() <= 2);
    }
}

#[tokio::test]
async fn completed_call_identity_is_not_executed_twice_in_one_run() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("README.md"), "read once").unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let model = resolved(json!({}));
    let round = read_round(json!({}), "same-call", r#"{"path":"README.md"}"#);
    let provider = ScriptedProvider::new(&model, vec![round.clone(), round, final_round()], false);
    let outcome = crate::run_agent(
        &provider,
        &tools,
        agent_request(&model, dir.path()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(!outcome.failed);
    assert_eq!(outcome.executed_tool_call_count, 1);
    assert_eq!(outcome.final_text, "Complete");
}

#[tokio::test]
async fn warm_candidate_replays_only_under_same_model_route_api_and_effective_capabilities() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("README.md"), "read once").unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let original = resolved(json!({}));
    let first_provider = ScriptedProvider::new(
        &original,
        vec![
            read_round(
                json!({"reasoning":"private warm source","reasoning_details":[{"type":"reasoning.encrypted","id":"opaque","data":"signed-secret"}]}),
                "call-warm",
                r#"{"path":"README.md"}"#,
            ),
            final_round(),
        ],
        false,
    );
    let first = crate::run_agent(
        &first_provider,
        &tools,
        agent_request(&original, dir.path()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(!first.failed);
    let environment = first.messages[0]
        .replay_frame()
        .unwrap()
        .environment_fingerprint()
        .unwrap();
    for change in [
        "none", "image", "efforts", "provider", "model", "route", "api",
    ] {
        let mut model = original.clone();
        if change == "image" {
            model = resolve_model(
                &original.definition,
                Some(&override_for(&original, None, Some(true))),
            )
            .unwrap();
        }
        if change == "efforts" {
            model = resolve_model(
                &original.definition,
                Some(&override_for(&original, Some(&["low"]), None)),
            )
            .unwrap();
        }
        if change == "provider" {
            model = resolved(json!({"provider":"other"}));
        }
        if change == "model" {
            model = resolved(json!({"id":"other-model"}));
        }
        if change == "route" {
            model = resolved(json!({"baseUrl":"https://other.invalid/v1"}));
        }
        if change == "api" {
            model =
                resolved(json!({"api":"openai-responses","baseUrl":"https://response.invalid/v1"}));
        }
        let response = if change == "api" {
            vec![
                json!({"type":"response.completed","response":{"id":"response-complete","status":"completed","output":[{"type":"message","id":"message-done","role":"assistant","content":[{"type":"output_text","text":"Complete"}]}]}}),
            ]
        } else {
            final_round()
        };
        let mut provider = ScriptedProvider::new(&model, vec![response], change == "api");
        if change == "route" {
            provider.endpoint = "https://other.invalid/v1".into();
        }
        let mut next = agent_request(&model, dir.path());
        next.history = first
            .messages
            .iter()
            .skip(1)
            .map(ChatMessage::without_live_tool_arguments)
            .collect();
        next.history.push(ChatMessage::new(
            ChatRole::User,
            "Continue without reading again.",
        ));
        let policy = freeze_choice(&model, ReasoningChoice::ProviderDefault)
            .unwrap()
            .replay_policy;
        let mut frame = crate::ReplayFrame::new(next.history.len(), policy);
        frame.attach_route_fingerprint(if change == "route" || change == "api" {
            [8; 32]
        } else {
            [7; 32]
        });
        frame.attach_candidate(first.messages.clone(), environment);
        next.replay_frame = Some(frame);
        let continued = crate::run_agent(&provider, &tools, next, CancellationToken::new())
            .await
            .unwrap();
        assert!(!continued.failed, "{change}");
        assert_eq!(continued.executed_tool_call_count, 0, "{change}");
        let body = provider.bodies.lock().unwrap()[0].to_string();
        assert_eq!(
            body.contains("private warm source"),
            change == "none",
            "{change}"
        );
        assert_eq!(body.contains("signed-secret"), change == "none", "{change}");
        assert!(!continued.final_text.contains("signed-secret"));
    }
}

#[tokio::test]
async fn user_and_history_images_block_before_send_while_legal_tool_images_block_next_send() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("red.png"), pixels().bytes()).unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let text_model = resolved(json!({"input":["text"]}));
    for history_image in [false, true] {
        let provider = ScriptedProvider::new(&text_model, vec![final_round()], false);
        let mut request = agent_request(&text_model, dir.path());
        let mut image = ChatMessage::new(ChatRole::User, "Describe the image");
        image.images = vec![pixels()];
        request.history = if history_image {
            vec![
                image,
                ChatMessage::new(ChatRole::Assistant, "Previous response"),
                ChatMessage::new(ChatRole::User, "Continue"),
            ]
        } else {
            vec![image]
        };
        let result = crate::run_agent(&provider, &tools, request, CancellationToken::new()).await;
        assert!(result.is_err() || result.is_ok_and(|outcome| outcome.failed));
        assert!(provider.bodies.lock().unwrap().is_empty());
    }
    let provider = ScriptedProvider::new(
        &text_model,
        vec![
            read_round(json!({}), "read-image", r#"{"path":"red.png"}"#),
            final_round(),
        ],
        false,
    );
    let outcome = crate::run_agent(
        &provider,
        &tools,
        agent_request(&text_model, dir.path()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(outcome.failed);
    assert_eq!(outcome.executed_tool_call_count, 1);
    assert_eq!(provider.bodies.lock().unwrap().len(), 1);
    assert_eq!(
        std::fs::read(dir.path().join("red.png")).unwrap(),
        pixels().bytes()
    );
    let vision = resolve_model(
        &text_model.definition,
        Some(&override_for(&text_model, None, Some(true))),
    )
    .unwrap();
    let provider = ScriptedProvider::new(
        &vision,
        vec![
            read_round(json!({}), "read-image", r#"{"path":"red.png"}"#),
            final_round(),
        ],
        false,
    );
    let outcome = crate::run_agent(
        &provider,
        &tools,
        agent_request(&vision, dir.path()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(!outcome.failed);
    assert_eq!(outcome.executed_tool_call_count, 1);
    assert_eq!(provider.bodies.lock().unwrap().len(), 2);
    assert!(
        provider.bodies.lock().unwrap()[1]
            .to_string()
            .contains("data:image/png;base64,")
    );
}

#[tokio::test]
async fn cancelled_run_never_contacts_provider_or_starts_tools() {
    let dir = tempfile::tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let model = resolved(json!({}));
    let provider = ScriptedProvider::new(&model, vec![final_round()], false);
    let cancel = CancellationToken::new();
    cancel.cancel();
    let result =
        crate::run_agent(&provider, &tools, agent_request(&model, dir.path()), cancel).await;
    assert!(
        matches!(result, Err(VegaError::Cancelled))
            || result.is_ok_and(|outcome| outcome.interrupted
                && !outcome.failed
                && outcome.executed_tool_call_count == 0)
    );
    assert!(provider.bodies.lock().unwrap().is_empty());
}

#[tokio::test]
async fn frozen_capacity_output_reserve_remains_authoritative_over_source_model_limits() {
    let dir = tempfile::tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let model = resolved(json!({"contextWindow":200,"maxTokens":100}));
    let provider = ScriptedProvider::new(&model, vec![final_round()], false);
    let mut request = agent_request(&model, dir.path());
    request.context_budget = Some(crate::ContextBudget::new(100_000, 800, false).unwrap());
    let outcome = crate::run_agent(&provider, &tools, request, CancellationToken::new())
        .await
        .unwrap();
    assert!(!outcome.failed);
    assert_eq!(provider.requests.lock().unwrap()[0].max_tokens, Some(800));
    assert_eq!(provider.bodies.lock().unwrap()[0]["max_tokens"], 800);
    let provider = ScriptedProvider::new(&model, vec![final_round()], false);
    let mut request = agent_request(&model, dir.path());
    request.context_budget = Some(crate::ContextBudget::new(500, 100, false).unwrap());
    request.history[0].content = "large untrusted history ".repeat(5000);
    let outcome = crate::run_agent(&provider, &tools, request, CancellationToken::new())
        .await
        .unwrap();
    assert!(outcome.failed);
    assert!(provider.bodies.lock().unwrap().is_empty());
}

#[tokio::test]
async fn declared_thinking_as_text_is_wire_only_and_cold_projection_never_restores_it() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("README.md"), "owned").unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    for visible in ["Visible preface", ""] {
        let model = resolved(json!({"compat":{"requiresThinkingAsText":true}}));
        let provider = ScriptedProvider::new(
            &model,
            vec![
                read_round(
                    json!({"reasoning_content":"private thinking","content":visible}),
                    "thinking-call",
                    r#"{"path":"README.md"}"#,
                ),
                final_round(),
            ],
            false,
        );
        let outcome = crate::run_agent(
            &provider,
            &tools,
            agent_request(&model, dir.path()),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert!(!outcome.failed);
        let bodies = provider.bodies.lock().unwrap();
        let assistant = bodies[1]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|message| message["role"] == "assistant" && message.get("tool_calls").is_some())
            .unwrap();
        assert_eq!(
            assistant["content"][0],
            json!({"type":"text","text":"private thinking"})
        );
        if !visible.is_empty() {
            assert_eq!(
                assistant["content"][1],
                json!({"type":"text","text":visible})
            );
        }
        assert!(assistant.get("reasoning_content").is_none());
        assert!(!outcome.final_text.contains("private thinking"));
        let mut cold = bound_request(&model, ReasoningChoice::ProviderDefault);
        cold.messages = outcome.messages.clone();
        let mut system = ChatMessage::new(ChatRole::System, "New run");
        system.attach_replay_frame(crate::ReplayFrame::new(
            cold.messages.len() + 1,
            crate::ReasoningReplayPolicy::Unknown,
        ));
        cold.messages.insert(0, system);
        cold.messages
            .push(ChatMessage::new(ChatRole::User, "Continue"));
        let body =
            super::super::encode_request("https://fixture.invalid/v1", &cold, false).unwrap();
        assert!(!body.to_string().contains("private thinking"));
    }
}

#[tokio::test]
async fn routed_response_model_never_replaces_configured_request_or_replay_identity() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("README.md"), "owned").unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let model = resolved(json!({"id":"openrouter/auto"}));
    let mut round = read_round(json!({}), "routed-call", r#"{"path":"README.md"}"#);
    for chunk in &mut round {
        chunk["model"] = json!("other/actual-routed");
    }
    let provider = ScriptedProvider::new(&model, vec![round, final_round()], false);
    let outcome = crate::run_agent(
        &provider,
        &tools,
        agent_request(&model, dir.path()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(!outcome.failed);
    assert!(
        provider
            .requests
            .lock()
            .unwrap()
            .iter()
            .all(|request| request.model == "openrouter/auto"
                && request.model_snapshot.as_deref() == Some(&model))
    );
    assert!(
        provider
            .bodies
            .lock()
            .unwrap()
            .iter()
            .all(|body| body["model"] == "openrouter/auto")
    );
}

#[tokio::test]
async fn cancellation_after_visible_delta_fences_queued_tools_before_any_execution() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("README.md"), "unchanged").unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let model = resolved(json!({}));
    let provider = ScriptedProvider::new(
        &model,
        vec![read_round(
            json!({"content":"Partial visible text"}),
            "cancelled-call",
            r#"{"path":"README.md"}"#,
        )],
        false,
    );
    let cancel = CancellationToken::new();
    let stop = cancel.clone();
    let outcome = crate::run_agent_with_sink(
        &provider,
        &tools,
        agent_request(&model, dir.path()),
        cancel,
        move |event| {
            if matches!(event, RuntimeEvent::TextDelta(_)) {
                stop.cancel();
            }
            async { Ok(()) }
        },
    )
    .await
    .unwrap();
    assert!(outcome.interrupted);
    assert_eq!(outcome.executed_tool_call_count, 0);
    assert_eq!(provider.bodies.lock().unwrap().len(), 1);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("README.md")).unwrap(),
        "unchanged"
    );
}

#[test]
fn private_text_and_opaque_detail_bytes_remain_in_real_context_and_resident_accounting() {
    let mut assistant = ChatMessage::assistant_with_tools(
        "visible",
        vec![ChatToolCall {
            id: "call".into(),
            name: "read".into(),
            input_json: r#"{"path":"README.md"}"#.into(),
        }],
    );
    let baseline = assistant.replay_resident_size_bytes();
    let events=decode_chat(&[chat_chunk(json!({"reasoning_text":"private ".repeat(1024),"reasoning_details":[{"type":"reasoning.encrypted","id":"opaque","data":"sealed".repeat(2048)}]}),json!("stop"))],true).unwrap();
    for event in events {
        if let ProviderEvent::ChatReasoningState(raw) = event {
            assistant.chat_reasoning = Some(raw);
        }
    }
    assert!(assistant.replay_resident_size_bytes() > baseline + 15_000);
    let without = ChatMessage::assistant_with_tools("visible", assistant.tool_calls.clone());
    let before = crate::estimate_wire_context(&[without], &[]).unwrap();
    let after = crate::estimate_wire_context(&[assistant], &[]).unwrap();
    assert!(after.input_tokens > before.input_tokens + 1000);
}

#[tokio::test]
async fn known_required_replay_fields_remain_strict_while_generic_missing_reasoning_is_legal() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("README.md"), "owned").unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    for (endpoint, name, required) in [
        ("https://api.deepseek.com", "deepseek-reasoner", true),
        ("https://api.z.ai/api/paas/v4", "glm-4.7", true),
        ("https://fixture.invalid/v1", "step-5-preview", false),
        ("https://fixture.invalid/v1", "deepseek-v4-flash", false),
    ] {
        let model = fixed(ModelApi::ChatCompletions, endpoint, "fixture", name);
        for raw in [None, Some("")] {
            let fields = raw.map_or(json!({}), |text| json!({"reasoning_content":text}));
            let mut provider = ScriptedProvider::new(
                &model,
                vec![
                    read_round(fields, "required-call", r#"{"path":"README.md"}"#),
                    final_round(),
                ],
                false,
            );
            provider.endpoint = endpoint.into();
            let outcome = crate::run_agent(
                &provider,
                &tools,
                agent_request(&model, dir.path()),
                CancellationToken::new(),
            )
            .await
            .unwrap();
            assert_eq!(outcome.failed, required && raw.is_none(), "{name} {raw:?}");
            assert_eq!(
                outcome.executed_tool_call_count,
                usize::from(!required || raw.is_some()),
                "{name}"
            );
            if !outcome.failed {
                let bodies = provider.bodies.lock().unwrap();
                let assistant = bodies[1]["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|message| {
                        message["role"] == "assistant" && message.get("tool_calls").is_some()
                    })
                    .unwrap();
                assert_eq!(
                    assistant.get("reasoning_content").and_then(Value::as_str),
                    raw,
                    "{name}"
                );
            }
        }
    }
}
