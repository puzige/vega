use super::context_accounting::{InputAnchor, InputDecision, PendingInputAnchor};
use crate::{
    ChatMessage, ChatRequest, ChatRole, ChatToolCall, ContextAccountingSource, ContextBudget,
    ContextCheck, ContextEstimateError, FrozenReasoning, ImageAttachment, ReasoningChoice,
    ReasoningProtocol, ReasoningReplayPolicy, ReplayFrame, RuntimeToolCall, RuntimeToolResult,
    RuntimeToolStatus, ToolDefinition, estimate_wire_context, project_model_messages,
};

const MODEL: &str = "gpt-4.1";

fn model_snapshot() -> crate::model_compat::ResolvedModel {
    use crate::model_compat::{
        ModelApi, ModelIdentity, ModelRoute, lookup_model_definition, resolve_model,
    };
    let route = ModelRoute::new(ModelApi::ChatCompletions, "https://api.openai.com/v1").unwrap();
    let identity = ModelIdentity {
        provider: "openai".into(),
        model: MODEL.into(),
    };
    resolve_model(&lookup_model_definition(&route, &identity).unwrap(), None).unwrap()
}
const OUTPUT_CAP: u32 = 4096;
const REVISION: u64 = 7;

fn tools() -> Vec<ToolDefinition> {
    vec![ToolDefinition {
        name: "read".into(),
        description: "Read a file".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {"path": {"type": "string"}},
            "required": ["path"]
        }),
        strict: false,
    }]
}

fn tool_call(id: &str, name: &str, input_json: &str) -> ChatToolCall {
    ChatToolCall {
        id: id.into(),
        name: name.into(),
        input_json: input_json.into(),
    }
}

fn cold_history(split_count: usize) -> Vec<ChatMessage> {
    let mut messages = vec![ChatMessage::new(ChatRole::System, "system instructions")];
    for index in 0..split_count {
        let id = format!("historical-{index}");
        let mut assistant = ChatMessage::assistant_with_tools(
            format!("historical assistant text {index}"),
            vec![tool_call(&id, "archive", r#"{"query":"previous"}"#)],
        );
        assistant
            .mark_stored_tool_calls_with_statuses(vec!["success".into()])
            .unwrap();
        messages.push(assistant);
        messages.push(ChatMessage::tool_result(
            id,
            format!("historical result {index}"),
        ));
    }
    let cold_prefix_len = messages.len();
    messages[0].attach_replay_frame(ReplayFrame::new(
        cold_prefix_len,
        ReasoningReplayPolicy::StandardChat,
    ));
    messages.push(ChatMessage::new(ChatRole::User, "current request"));
    messages
}

fn runtime_read_result(id: &str, output: &str) -> ChatMessage {
    let call = RuntimeToolCall {
        id: id.into(),
        name: "read".into(),
        input_json: r#"{"path":"source.txt"}"#.into(),
        description: None,
    };
    let result = RuntimeToolResult {
        call_id: id.into(),
        output: output.into(),
        status: RuntimeToolStatus::Success,
        reused: false,
        exit_code: None,
        duration_ms: None,
        truncated: Some(false),
        approval: None,
        remember_rule: None,
    };
    ChatMessage::runtime_tool_result(&call, &result)
}

fn append_tool_round(messages: &mut Vec<ChatMessage>, outputs: &[&str], reasoning: Option<&str>) {
    let calls = (0..outputs.len())
        .map(|index| tool_call(&format!("live-{index}"), "read", r#"{"path":"source.txt"}"#))
        .collect::<Vec<_>>();
    let mut assistant = ChatMessage::assistant_with_tools_and_reasoning(
        "checking the files",
        reasoning.map(str::to_owned),
        calls.clone(),
    );
    assistant
        .bind_live_tool_calls(calls.into_iter().map(Some).collect())
        .unwrap();
    messages.push(assistant);
    for (index, output) in outputs.iter().enumerate() {
        messages.push(runtime_read_result(&format!("live-{index}"), output));
    }
}

fn projected_request(messages: &[ChatMessage], tools: &[ToolDefinition]) -> ChatRequest {
    ChatRequest {
        model: MODEL.into(),
        model_snapshot: Some(Box::new(model_snapshot())),
        messages: project_model_messages(messages).unwrap(),
        tools: tools.to_vec(),
        max_tokens: Some(OUTPUT_CAP),
        reasoning: Some(FrozenReasoning::unknown("provider-a", MODEL)),
        ..Default::default()
    }
}

fn pending(request: &ChatRequest) -> PendingInputAnchor {
    PendingInputAnchor::from_request(request).unwrap().unwrap()
}

fn anchor(request: &ChatRequest, input: u64) -> InputAnchor {
    pending(request).complete(input).unwrap()
}

fn decide(
    anchor: Option<&InputAnchor>,
    messages: &[ChatMessage],
    request: &ChatRequest,
) -> InputDecision {
    let mut next = request.clone();
    next.messages = messages.to_vec();
    InputAnchor::decide_request(anchor, &next, REVISION).unwrap()
}

fn assert_anchored(
    decision: InputDecision,
    request: &ChatRequest,
    messages: &[ChatMessage],
    input: u64,
) {
    let prefix = estimate_wire_context(&request.messages, &request.tools)
        .unwrap()
        .input_tokens;
    let full = estimate_wire_context(messages, &request.tools)
        .unwrap()
        .input_tokens;
    let incremental = full.checked_sub(prefix).unwrap();
    assert_eq!(decision.source, ContextAccountingSource::UsageAnchored);
    assert_eq!(decision.baseline, Some(input));
    assert_eq!(decision.incremental, incremental);
    assert_eq!(decision.predicted, input + incremental);
    assert_eq!(decision.estimate.input_tokens, input + incremental);
    assert_eq!(decision.covered_messages, request.messages.len());
    assert_eq!(decision.revision, REVISION);
}

fn assert_estimated(decision: InputDecision, messages: &[ChatMessage], tools: &[ToolDefinition]) {
    let estimate = estimate_wire_context(messages, tools).unwrap();
    assert_eq!(decision.source, ContextAccountingSource::Estimated);
    assert_eq!(decision.baseline, None);
    assert_eq!(decision.incremental, estimate.input_tokens);
    assert_eq!(decision.predicted, estimate.input_tokens);
    assert_eq!(decision.estimate, estimate);
    assert_eq!(decision.covered_messages, 0);
    assert_eq!(decision.revision, REVISION);
    assert!(decision.predicted > 0);
}

fn observation_kind(message: &ChatMessage) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(&message.content).ok()?;
    value["vega_history_observation"]["kind"]
        .as_str()
        .map(str::to_owned)
}

#[test]
fn cold_split_prefixes_reuse_usage_after_a_complete_tool_round() {
    let definitions = tools();
    for split_count in [1, 2, 3, 5] {
        let mut messages = cold_history(split_count);
        let request = projected_request(&messages, &definitions);
        assert_eq!(request.messages.len(), messages.len() + split_count);
        for kind in ["assistant_text", "tool_activity", "tool_result"] {
            assert_eq!(
                request
                    .messages
                    .iter()
                    .filter(|message| observation_kind(message).as_deref() == Some(kind))
                    .count(),
                split_count
            );
        }
        let input = 85_032;
        let anchor = anchor(&request, input);
        append_tool_round(
            &mut messages,
            &["new tool result"],
            Some("retained reasoning"),
        );
        let projected = project_model_messages(&messages).unwrap();
        assert_eq!(
            &projected[..request.messages.len()],
            request.messages.as_slice()
        );
        assert_eq!(projected[request.messages.len()].role, ChatRole::Assistant);
        assert_eq!(projected[request.messages.len() + 1].role, ChatRole::Tool);
        assert_anchored(
            decide(Some(&anchor), &messages, &request),
            &request,
            &messages,
            input,
        );
    }
}

#[test]
fn unchanged_cold_and_warm_requests_reuse_exactly_the_provider_input() {
    let cold = cold_history(3);
    let warm = vec![
        ChatMessage::new(ChatRole::System, "system instructions"),
        ChatMessage::new(ChatRole::User, "current request"),
    ];
    for messages in [&cold, &warm] {
        let request = projected_request(messages, &tools());
        let anchor = anchor(&request, 85_032);
        let decision = decide(Some(&anchor), messages, &request);
        assert_anchored(decision, &request, messages, 85_032);
        assert_eq!(decision.incremental, 0);
        assert_eq!(decision.predicted, 85_032);
    }
}

#[test]
fn conservative_estimate_does_not_trigger_when_usage_and_tail_fit() {
    let mut messages = cold_history(1);
    messages.last_mut().unwrap().content = "H".repeat(870_000);
    let request = projected_request(&messages, &tools());
    let input = 132_000;
    let anchor = anchor(&request, input);
    append_tool_round(&mut messages, &["small result"], None);
    let budget = ContextBudget::new(320_000, 20_000, true).unwrap();
    assert_eq!(budget.input_budget(), 300_000);
    assert_eq!(budget.trigger_tokens().unwrap(), 287_000);
    assert_eq!(budget.blocking_tokens().unwrap(), 297_000);
    let raw = decide(None, &messages, &request);
    assert_estimated(raw, &messages, &request.tools);
    assert_eq!(budget.check(raw.estimate).unwrap(), ContextCheck::Triggered);
    let calibrated = decide(Some(&anchor), &messages, &request);
    assert_anchored(calibrated, &request, &messages, input);
    assert!(calibrated.predicted < budget.trigger_tokens().unwrap());
    assert_eq!(
        budget.check(calibrated.estimate).unwrap(),
        ContextCheck::Within
    );
}

#[test]
fn calibrated_trigger_and_input_limit_remain_enforced() {
    let mut messages = cold_history(2);
    let request = projected_request(&messages, &tools());
    append_tool_round(&mut messages, &["a real tool result"], None);
    let budget = ContextBudget::new(320_000, 20_000, true).unwrap();
    let trigger = budget.trigger_tokens().unwrap();
    let prefix = estimate_wire_context(&request.messages, &request.tools)
        .unwrap()
        .input_tokens;
    let full = estimate_wire_context(&messages, &request.tools)
        .unwrap()
        .input_tokens;
    let incremental = full.checked_sub(prefix).unwrap();
    assert!(incremental > 0 && incremental < trigger);
    let blocking = budget.blocking_tokens().unwrap();
    assert_eq!(trigger, 287_000);
    assert_eq!(blocking, 297_000);
    for (predicted, expected) in [
        (trigger - 1, ContextCheck::Within),
        (trigger, ContextCheck::Triggered),
        (blocking - 1, ContextCheck::Triggered),
        (blocking, ContextCheck::OverLimit),
        (budget.input_budget(), ContextCheck::OverLimit),
    ] {
        let input = predicted - incremental;
        let anchor = anchor(&request, input);
        let decision = decide(Some(&anchor), &messages, &request);
        assert_anchored(decision, &request, &messages, input);
        assert_eq!(decision.predicted, predicted);
        assert_eq!(budget.check(decision.estimate).unwrap(), expected);
    }
}

#[test]
fn appended_multi_tool_results_and_retained_reasoning_are_counted_once() {
    let mut messages = cold_history(3);
    let request = projected_request(&messages, &tools());
    let input = 121_896;
    let anchor = anchor(&request, input);
    let long_output = "distinct retained result ".repeat(32);
    let retained_reasoning = "R".repeat(1200);
    append_tool_round(
        &mut messages,
        &[&long_output, "second result"],
        Some(&retained_reasoning),
    );
    let projected = project_model_messages(&messages).unwrap();
    let tail = &projected[request.messages.len()..];
    assert_eq!(tail.len(), 3);
    assert_eq!(tail[0].tool_calls.len(), 2);
    assert_eq!(
        tail[0].reasoning_content.as_deref(),
        Some(retained_reasoning.as_str())
    );
    assert_eq!(tail[1].tool_call_id.as_deref(), Some("live-0"));
    assert_eq!(tail[1].content, long_output);
    assert_eq!(tail[2].tool_call_id.as_deref(), Some("live-1"));
    assert_eq!(tail[2].content, "second result");
    let full = decide(Some(&anchor), &messages, &request);
    assert_anchored(full, &request, &messages, input);

    let mut without_reasoning = messages.clone();
    let assistant_index = without_reasoning.len() - 3;
    without_reasoning[assistant_index].reasoning_content = None;
    let no_reasoning = decide(Some(&anchor), &without_reasoning, &request);
    assert_anchored(no_reasoning, &request, &without_reasoning, input);
    assert_eq!(full.incremental - no_reasoning.incremental, 400);

    let mut longer_first = messages.clone();
    let first_result = longer_first.len() - 2;
    longer_first[first_result]
        .content
        .push_str(&"A".repeat(1200));
    let first_changed = decide(Some(&anchor), &longer_first, &request);
    assert_anchored(first_changed, &request, &longer_first, input);
    assert_eq!(first_changed.incremental - full.incremental, 400);

    let mut longer_second = messages.clone();
    longer_second
        .last_mut()
        .unwrap()
        .content
        .push_str(&"B".repeat(1200));
    let second_changed = decide(Some(&anchor), &longer_second, &request);
    assert_anchored(second_changed, &request, &longer_second, input);
    assert_eq!(second_changed.incremental - full.incremental, 400);
}

#[test]
fn warm_tool_results_use_projected_previews_and_references() {
    let mut messages = vec![
        ChatMessage::new(ChatRole::System, "system instructions"),
        ChatMessage::new(ChatRole::User, "inspect the same file twice"),
    ];
    messages[0].attach_replay_frame(ReplayFrame::new(1, ReasoningReplayPolicy::StandardChat));
    let request = projected_request(&messages, &tools());
    let input = 40_000;
    let anchor = anchor(&request, input);
    let output = "A".repeat(30_000);
    append_tool_round(&mut messages, &[&output, &output], None);
    let projected = project_model_messages(&messages).unwrap();
    assert_eq!(projected.len(), messages.len());
    let first = &projected[request.messages.len() + 1];
    let second = &projected[request.messages.len() + 2];
    assert_eq!(first.role, ChatRole::Tool);
    assert!(first.content.contains("vega_tool_preview"));
    assert!(first.content.len() < output.len());
    let reference: serde_json::Value = serde_json::from_str(&second.content).unwrap();
    assert_eq!(reference["vega_tool_result_reference"]["call_id"], "live-0");
    assert_eq!(reference["vega_tool_result_reference"]["bytes"], 30_000);
    let decision = decide(Some(&anchor), &messages, &request);
    assert_anchored(decision, &request, &messages, input);

    let mut distinct = messages.clone();
    let second_index = distinct.len() - 1;
    distinct[second_index] = runtime_read_result("live-1", &"B".repeat(30_000));
    let distinct_projection = project_model_messages(&distinct).unwrap();
    assert!(
        distinct_projection
            .last()
            .unwrap()
            .content
            .contains("vega_tool_preview")
    );
    let distinct_decision = decide(Some(&anchor), &distinct, &request);
    assert_anchored(distinct_decision, &request, &distinct, input);
    assert!(distinct_decision.incremental > decision.incremental);
}

#[test]
fn changed_covered_history_and_request_identity_fall_back() {
    let history = cold_history(3);
    let mut request = projected_request(&history, &tools());
    let mut reasoning = FrozenReasoning::unknown("provider-a", MODEL);
    reasoning.protocol = ReasoningProtocol::OpenAiChatCompletions;
    reasoning.replay_policy = ReasoningReplayPolicy::StandardChat;
    reasoning.declared_efforts = vec!["low".into(), "high".into()];
    reasoning.choice = ReasoningChoice::Effort("low".into());
    reasoning.validate().unwrap();
    request.reasoning = Some(reasoning);
    let anchor = anchor(&request, 85_032);
    let mut messages = history.clone();
    append_tool_round(&mut messages, &["tail"], None);
    assert_anchored(
        decide(Some(&anchor), &messages, &request),
        &request,
        &messages,
        85_032,
    );

    let mut changed = messages.clone();
    changed[0].content.push_str(" changed system");
    assert_estimated(
        decide(Some(&anchor), &changed, &request),
        &changed,
        &request.tools,
    );
    let mut changed = messages.clone();
    changed[1].content.push_str(" changed historical text");
    assert_estimated(
        decide(Some(&anchor), &changed, &request),
        &changed,
        &request.tools,
    );
    let mut changed = messages.clone();
    changed[1].tool_calls[0].id = "different-id".into();
    changed[2].tool_call_id = Some("different-id".into());
    changed[1]
        .mark_stored_tool_calls_with_statuses(vec!["success".into()])
        .unwrap();
    assert_estimated(
        decide(Some(&anchor), &changed, &request),
        &changed,
        &request.tools,
    );
    let mut changed = messages.clone();
    changed[2].content.push_str(" changed historical result");
    assert_estimated(
        decide(Some(&anchor), &changed, &request),
        &changed,
        &request.tools,
    );
    let mut changed = messages.clone();
    changed.swap(1, 3);
    changed.swap(2, 4);
    assert_estimated(
        decide(Some(&anchor), &changed, &request),
        &changed,
        &request.tools,
    );
    let replacement = vec![
        ChatMessage::new(ChatRole::System, "system instructions"),
        ChatMessage::new(ChatRole::User, "compacted history replacement"),
    ];
    assert_estimated(
        decide(Some(&anchor), &replacement, &request),
        &replacement,
        &request.tools,
    );

    let mut changed_request = request.clone();
    changed_request.model = "model-b".into();
    assert_estimated(
        decide(Some(&anchor), &messages, &changed_request),
        &messages,
        &changed_request.tools,
    );
    let mut changed_request = request.clone();
    changed_request.reasoning = Some(FrozenReasoning::unknown("provider-b", MODEL));
    assert_estimated(
        decide(Some(&anchor), &messages, &changed_request),
        &messages,
        &changed_request.tools,
    );
    let mut changed_request = request.clone();
    let changed_reasoning = changed_request.reasoning.as_mut().unwrap();
    changed_reasoning.choice = ReasoningChoice::Effort("high".into());
    changed_reasoning.validate().unwrap();
    assert_estimated(
        decide(Some(&anchor), &messages, &changed_request),
        &messages,
        &changed_request.tools,
    );
    let mut changed_request = request.clone();
    changed_request.reasoning = None;
    assert_estimated(
        decide(Some(&anchor), &messages, &changed_request),
        &messages,
        &changed_request.tools,
    );
    let mut changed_request = request.clone();
    changed_request.max_tokens = Some(OUTPUT_CAP + 1);
    assert_estimated(
        decide(Some(&anchor), &messages, &changed_request),
        &messages,
        &changed_request.tools,
    );
    for mutation in 0..4 {
        let mut changed_request = request.clone();
        match mutation {
            0 => changed_request.tools[0].name = "different-tool".into(),
            1 => changed_request.tools[0].description = "different description".into(),
            2 => changed_request.tools[0].input_schema = serde_json::json!({"type": "array"}),
            3 => changed_request.tools[0].strict = true,
            _ => unreachable!(),
        }
        assert_estimated(
            decide(Some(&anchor), &messages, &changed_request),
            &messages,
            &changed_request.tools,
        );
    }
}

#[test]
fn append_can_change_the_previous_projected_prefix_and_must_invalidate_it() {
    let mut messages = vec![
        ChatMessage::new(ChatRole::System, "system instructions"),
        ChatMessage::new(ChatRole::User, "initial request"),
    ];
    messages[0].attach_replay_frame(ReplayFrame::new(2, ReasoningReplayPolicy::GlmInterleaved));
    append_tool_round(&mut messages, &["result"], Some("prior retained reasoning"));
    let request = projected_request(&messages, &tools());
    assert_eq!(
        request.messages[2].reasoning_content.as_deref(),
        Some("prior retained reasoning")
    );
    let anchor = anchor(&request, 40_000);
    let new_user_boundary = messages.len();
    messages[0].begin_interleaved_user_round(new_user_boundary);
    messages.push(ChatMessage::new(ChatRole::User, "follow-up"));
    let projected = project_model_messages(&messages).unwrap();
    assert_eq!(projected[2].reasoning_content, None);
    assert_ne!(projected[2], request.messages[2]);
    assert_estimated(
        decide(Some(&anchor), &messages, &request),
        &messages,
        &request.tools,
    );
}

#[test]
fn appending_a_live_id_collision_must_not_reuse_an_unsafe_prefix() {
    let id = "provider|shared";
    let call = tool_call(id, "read", r#"{"path":"source.txt"}"#);
    let mut messages = vec![
        ChatMessage::new(ChatRole::System, "system instructions"),
        ChatMessage::new(ChatRole::User, "request"),
        ChatMessage::assistant_with_tools("earlier call", vec![call.clone()]),
        ChatMessage::tool_result(id, "earlier result"),
    ];
    messages[0].attach_replay_frame(ReplayFrame::new(1, ReasoningReplayPolicy::StandardChat));
    let request = projected_request(&messages, &tools());
    assert!(!request.messages[2].preserves_live_wire_ids());
    let anchor = anchor(&request, 40_000);
    let mut assistant = ChatMessage::assistant_with_tools("repeated call", vec![call.clone()]);
    assistant.bind_live_tool_calls(vec![Some(call)]).unwrap();
    messages.push(assistant);
    messages.push(runtime_read_result(id, "repeated result"));
    let projected = project_model_messages(&messages).unwrap();
    assert_eq!(
        &projected[..request.messages.len()],
        request.messages.as_slice()
    );
    assert!(projected[request.messages.len()].preserves_live_wire_ids());
    assert_estimated(
        decide(Some(&anchor), &messages, &request),
        &messages,
        &request.tools,
    );
}

#[test]
fn preserved_live_ids_reuse_usage_across_later_tool_rounds() {
    for id in [
        "provider|live",
        "provider-with-a-live-tool-id-that-is-longer-than-forty",
    ] {
        let mut messages = vec![
            ChatMessage::new(ChatRole::System, "system instructions"),
            ChatMessage::new(ChatRole::User, "request"),
        ];
        let first = tool_call(id, "read", r#"{"path":"source.txt"}"#);
        let mut assistant = ChatMessage::assistant_with_tools("checking", vec![first.clone()]);
        assistant.bind_live_tool_calls(vec![Some(first)]).unwrap();
        messages.push(assistant);
        messages.push(runtime_read_result(id, "first result"));
        let request = projected_request(&messages, &tools());
        assert!(request.messages[2].preserves_live_wire_ids());
        let anchor = anchor(&request, 40_000);
        append_tool_round(&mut messages, &["next result"], None);
        assert_anchored(
            decide(Some(&anchor), &messages, &request),
            &request,
            &messages,
            40_000,
        );
    }
}

#[test]
fn changed_live_tool_wire_identity_falls_back_even_with_the_same_visible_call() {
    let mut messages = vec![
        ChatMessage::new(ChatRole::System, "system instructions"),
        ChatMessage::new(ChatRole::User, "request"),
    ];
    let call = tool_call("provider|live", "read", r#"{"path":"source.txt"}"#);
    let mut assistant = ChatMessage::assistant_with_tools("checking", vec![call.clone()]);
    assistant.bind_live_tool_calls(vec![Some(call)]).unwrap();
    messages.push(assistant);
    messages.push(runtime_read_result("provider|live", "result"));
    let request = projected_request(&messages, &tools());
    let anchor = anchor(&request, 40_000);
    let mut changed_arguments = messages.clone();
    let mut changed_call = changed_arguments[2].tool_calls[0].clone();
    changed_call.input_json = r#"{"path":"other.txt"}"#.into();
    changed_arguments[2].tool_calls[0] = changed_call.clone();
    changed_arguments[2]
        .bind_live_tool_calls(vec![Some(changed_call)])
        .unwrap();
    assert_estimated(
        decide(Some(&anchor), &changed_arguments, &request),
        &changed_arguments,
        &request.tools,
    );
    let mut changed = messages.clone();
    changed[2] = changed[2].without_live_tool_arguments();
    let original_projection = project_model_messages(&messages).unwrap();
    let changed_projection = project_model_messages(&changed).unwrap();
    assert_eq!(
        original_projection[2].tool_calls,
        changed_projection[2].tool_calls
    );
    assert!(original_projection[2].preserves_live_wire_ids());
    assert!(!changed_projection[2].preserves_live_wire_ids());
    assert_estimated(
        decide(Some(&anchor), &changed, &request),
        &changed,
        &request.tools,
    );
}

fn image(pixel: [u8; 3]) -> ImageAttachment {
    let buffer = image::RgbImage::from_pixel(1, 1, image::Rgb(pixel));
    let mut encoded = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(buffer)
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    ImageAttachment::from_bytes(encoded.into_inner()).unwrap()
}

#[test]
fn image_identity_is_covered_and_appended_images_are_charged() {
    let mut messages = cold_history(1);
    messages.last_mut().unwrap().images.push(image([255, 0, 0]));
    let request = projected_request(&messages, &tools());
    let input = 40_000;
    let anchor = anchor(&request, input);
    append_tool_round(&mut messages, &["result"], None);
    let original = decide(Some(&anchor), &messages, &request);
    assert_anchored(original, &request, &messages, input);
    assert_eq!(original.estimate.image_count, 1);

    let mut changed = messages.clone();
    let covered_user = changed.len() - 3;
    changed[covered_user].images[0] = image([0, 0, 255]);
    assert_estimated(
        decide(Some(&anchor), &changed, &request),
        &changed,
        &request.tools,
    );

    let mut appended = messages.clone();
    let mut user = ChatMessage::new(ChatRole::User, "another image");
    user.images.push(image([0, 255, 0]));
    appended.push(user);
    let with_image = decide(Some(&anchor), &appended, &request);
    assert_anchored(with_image, &request, &appended, input);
    assert_eq!(with_image.estimate.image_count, 2);
    let mut without_image = appended.clone();
    without_image.last_mut().unwrap().images.clear();
    let no_image = decide(Some(&anchor), &without_image, &request);
    assert_anchored(no_image, &request, &without_image, input);
    assert!(with_image.incremental > no_image.incremental);
}

#[test]
fn zero_usage_invalid_projection_and_arithmetic_overflow_do_not_bypass_safety() {
    let mut messages = cold_history(1);
    let request = projected_request(&messages, &tools());
    let zero_usage_anchor = pending(&request).complete(0);
    assert!(zero_usage_anchor.is_none());
    append_tool_round(&mut messages, &["result"], None);
    assert_estimated(
        decide(zero_usage_anchor.as_ref(), &messages, &request),
        &messages,
        &request.tools,
    );
    let overflow_anchor = anchor(&request, u64::MAX);
    assert!(matches!(
        InputAnchor::decide(
            Some(&overflow_anchor),
            &messages,
            &request.tools,
            &request.model,
            request.reasoning.as_ref(),
            request.max_tokens,
            REVISION,
        ),
        Err(ContextEstimateError::Overflow)
    ));
    let mut invalid = messages.clone();
    let invalid_boundary = invalid.len() + 1;
    invalid[0].update_replay_cold_prefix(invalid_boundary);
    assert!(project_model_messages(&invalid).is_err());
    let valid_anchor = anchor(&request, 40_000);
    assert!(
        InputAnchor::decide(
            Some(&valid_anchor),
            &invalid,
            &request.tools,
            &request.model,
            request.reasoning.as_ref(),
            request.max_tokens,
            REVISION,
        )
        .is_err()
    );
}

mod openai_wire_anchor_regressions {
    use super::*;
    use crate::model_compat::{
        ModelApi, ModelCapabilityOverride, ModelIdentity, ModelRoute, freeze_choice,
        lookup_model_definition, resolve_model,
    };
    use crate::{ResponseReplayLayout, ResponseReplaySlot};
    use serde_json::{Value, json};

    const BASE_URL: &str = "https://api.openai.com/v1";
    const WIRE_MODEL: &str = "gpt-4.1";
    const INPUT: u64 = 40_000;

    #[derive(Clone, Copy, Debug)]
    enum WireApi {
        Chat,
        Responses,
    }

    impl WireApi {
        fn model_api(self) -> ModelApi {
            match self {
                Self::Chat => ModelApi::ChatCompletions,
                Self::Responses => ModelApi::Responses,
            }
        }

        fn replay_policy(self) -> ReasoningReplayPolicy {
            match self {
                Self::Chat => ReasoningReplayPolicy::StandardChat,
                Self::Responses => ReasoningReplayPolicy::ResponsesItems,
            }
        }

        fn sequence_key(self) -> &'static str {
            match self {
                Self::Chat => "messages",
                Self::Responses => "input",
            }
        }

        fn body(self, request: &ChatRequest) -> Value {
            crate::openai::encode_request(BASE_URL, request, matches!(self, Self::Responses))
                .unwrap()
        }
    }

    fn initial_messages(api: WireApi) -> Vec<ChatMessage> {
        let mut system = ChatMessage::new(ChatRole::System, "system instructions");
        system.attach_replay_frame(ReplayFrame::new(2, api.replay_policy()));
        vec![system, ChatMessage::new(ChatRole::User, "request")]
    }

    fn request(api: WireApi, messages: &[ChatMessage]) -> ChatRequest {
        let route = ModelRoute::new(api.model_api(), BASE_URL).unwrap();
        let identity = ModelIdentity {
            provider: "openai".into(),
            model: WIRE_MODEL.into(),
        };
        let definition = lookup_model_definition(&route, &identity).unwrap();
        let capabilities = ModelCapabilityOverride {
            identity,
            allowed_efforts: None,
            image_input: Some(true),
        };
        let model = resolve_model(&definition, Some(&capabilities)).unwrap();
        let reasoning = freeze_choice(&model, ReasoningChoice::ProviderDefault).unwrap();
        assert_eq!(reasoning.replay_policy, api.replay_policy());
        let mut definitions = tools();
        definitions.push(ToolDefinition {
            name: "ComputerScreenshot".into(),
            description: "Capture a window screenshot".into(),
            input_schema: json!({
                "type": "object",
                "properties": {"target_id": {"type": "string"}},
                "required": ["target_id"],
                "additionalProperties": false,
            }),
            strict: true,
        });
        ChatRequest {
            model: WIRE_MODEL.into(),
            messages: project_model_messages(messages).unwrap(),
            tools: definitions,
            max_tokens: Some(OUTPUT_CAP),
            reasoning: Some(reasoning),
            model_snapshot: Some(Box::new(model)),
            ..Default::default()
        }
    }

    fn live_assistant(api: WireApi, ids: &[&str]) -> ChatMessage {
        let calls = ids
            .iter()
            .map(|id| tool_call(id, "read", r#"{"path":"source.txt"}"#))
            .collect();
        live_assistant_with_calls(api, calls)
    }

    fn screenshot_call(id: &str) -> ChatToolCall {
        tool_call(
            id,
            "ComputerScreenshot",
            &json!({"target_id": format!("window-{id}")}).to_string(),
        )
    }

    fn live_assistant_with_calls(api: WireApi, calls: Vec<ChatToolCall>) -> ChatMessage {
        let audit_calls = calls
            .iter()
            .map(|call| {
                let mut audit = call.clone();
                if call.name == "ComputerScreenshot" {
                    audit.input_json = crate::computer_use::ComputerUseAudit::for_tool(
                        &call.name,
                        &call.input_json,
                    );
                }
                audit
            })
            .collect();
        let mut assistant = ChatMessage::assistant_with_tools("", audit_calls);
        if matches!(api, WireApi::Responses) {
            let slots = calls
                .iter()
                .map(|call| ResponseReplaySlot::FunctionCall {
                    item: json!({
                        "type": "function_call",
                        "call_id": call.id,
                        "name": call.name,
                        "arguments": call.input_json,
                    }),
                    call_id: call.id.clone(),
                    name: call.name.clone(),
                    arguments: call.input_json.clone(),
                })
                .collect();
            assistant.response_replay_layout = Some(ResponseReplayLayout {
                response_id: Some("response-id".into()),
                slots,
            });
        }
        assistant
            .bind_live_tool_calls(calls.into_iter().map(Some).collect())
            .unwrap();
        assistant
    }

    fn append_live_round(api: WireApi, messages: &mut Vec<ChatMessage>, id: &str) {
        messages.push(live_assistant(api, &[id]));
        messages.push(runtime_read_result(id, "later result"));
    }

    fn sequence(api: WireApi, body: &Value) -> &[Value] {
        body[api.sequence_key()].as_array().unwrap()
    }

    fn call_ids(api: WireApi, body: &Value) -> Vec<&str> {
        match api {
            WireApi::Chat => sequence(api, body)
                .iter()
                .flat_map(|item| {
                    item.get("tool_calls")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                })
                .map(|call| call["id"].as_str().unwrap())
                .collect(),
            WireApi::Responses => sequence(api, body)
                .iter()
                .filter(|item| item["type"] == "function_call")
                .map(|call| call["call_id"].as_str().unwrap())
                .collect(),
        }
    }

    fn result_ids(api: WireApi, body: &Value) -> Vec<&str> {
        sequence(api, body)
            .iter()
            .filter_map(|item| match api {
                WireApi::Chat if item["role"] == "tool" => item["tool_call_id"].as_str(),
                WireApi::Responses if item["type"] == "function_call_output" => {
                    item["call_id"].as_str()
                }
                _ => None,
            })
            .collect()
    }

    fn assert_wire_prefix(api: WireApi, before: &Value, after: &Value) {
        let before_items = sequence(api, before);
        let after_items = sequence(api, after);
        assert!(after_items.len() > before_items.len(), "{api:?}");
        assert_eq!(before_items, &after_items[..before_items.len()], "{api:?}");
        let mut truncated = after.clone();
        truncated[api.sequence_key()]
            .as_array_mut()
            .unwrap()
            .truncate(before_items.len());
        assert_eq!(&truncated, before, "{api:?}");
    }

    #[test]
    fn live_wire_ids_keep_real_serialized_prefix_and_reuse_usage() {
        let long_id = format!("provider-{}", "long".repeat(20));
        let long_pipe_id = format!("provider|{}", "long".repeat(20));
        for api in [WireApi::Chat, WireApi::Responses] {
            for id in ["provider|live", long_id.as_str(), long_pipe_id.as_str()] {
                let mut messages = initial_messages(api);
                messages.push(live_assistant(api, &[id]));
                messages.push(runtime_read_result(id, "first result"));
                let before_request = request(api, &messages);
                assert!(before_request.messages[2].preserves_live_wire_ids());
                let before = api.body(&before_request);
                assert_eq!(call_ids(api, &before), vec![id]);
                assert_eq!(result_ids(api, &before), vec![id]);
                let usage_anchor = anchor(&before_request, INPUT);

                append_live_round(api, &mut messages, "later-live-call");
                let after_request = request(api, &messages);
                assert_eq!(
                    &after_request.messages[..before_request.messages.len()],
                    before_request.messages.as_slice()
                );
                let after = api.body(&after_request);
                assert_wire_prefix(api, &before, &after);
                assert_eq!(call_ids(api, &after), vec![id, "later-live-call"]);
                assert_eq!(result_ids(api, &after), vec![id, "later-live-call"]);
                assert_anchored(
                    decide(Some(&usage_anchor), &messages, &before_request),
                    &before_request,
                    &messages,
                    INPUT,
                );
            }
        }
    }

    #[test]
    fn colliding_with_real_mapped_id_changes_wire_prefix_and_rejects_usage() {
        let long_id = format!("non-live-{}", "long".repeat(20));
        for api in [WireApi::Chat, WireApi::Responses] {
            for id in ["provider|non-live", long_id.as_str()] {
                for rebased_cold_projection in [false, true] {
                    let mut messages = initial_messages(api);
                    let assistant = if rebased_cold_projection {
                        live_assistant(api, &[id])
                    } else {
                        ChatMessage::assistant_with_tools(
                            "",
                            vec![tool_call(id, "read", r#"{"path":"source.txt"}"#)],
                        )
                    };
                    messages.push(assistant);
                    messages.push(runtime_read_result(id, "first result"));
                    if rebased_cold_projection {
                        messages = project_model_messages(&messages).unwrap();
                        assert!(messages[2].preserves_live_wire_ids());
                        let cold_boundary = messages.len();
                        messages[0].update_replay_cold_prefix(cold_boundary);
                    }
                    let before_request = request(api, &messages);
                    assert!(!before_request.messages[2].preserves_live_wire_ids());
                    let before = api.body(&before_request);
                    let mapped = call_ids(api, &before)[0].to_owned();
                    assert_ne!(mapped, id, "{api:?}");
                    assert_eq!(result_ids(api, &before), vec![mapped.as_str()]);
                    let usage_anchor = anchor(&before_request, INPUT);
                    assert_anchored(
                        decide(Some(&usage_anchor), &messages, &before_request),
                        &before_request,
                        &messages,
                        INPUT,
                    );

                    append_live_round(api, &mut messages, &mapped);
                    let after_request = request(api, &messages);
                    assert_eq!(
                        &after_request.messages[..before_request.messages.len()],
                        before_request.messages.as_slice()
                    );
                    assert!(
                        after_request.messages[before_request.messages.len()]
                            .preserves_live_wire_ids()
                    );
                    let after = api.body(&after_request);
                    let after_calls = call_ids(api, &after);
                    assert_eq!(after_calls.len(), 2);
                    assert_ne!(after_calls[0], mapped, "{api:?}");
                    assert_ne!(after_calls[0], id, "{api:?}");
                    assert_eq!(after_calls[1], mapped);
                    assert_eq!(result_ids(api, &after), after_calls);
                    assert_ne!(
                        sequence(api, &before),
                        &sequence(api, &after)[..sequence(api, &before).len()],
                        "{api:?}"
                    );
                    let mut restored_prefix = after.clone();
                    let items = restored_prefix[api.sequence_key()].as_array_mut().unwrap();
                    items.truncate(sequence(api, &before).len());
                    for item in items {
                        match api {
                            WireApi::Chat if item["role"] == "assistant" => {
                                item["tool_calls"][0]["id"] = json!(mapped);
                            }
                            WireApi::Chat if item["role"] == "tool" => {
                                item["tool_call_id"] = json!(mapped);
                            }
                            WireApi::Responses if item["type"] == "function_call" => {
                                item["call_id"] = json!(mapped);
                            }
                            WireApi::Responses if item["type"] == "function_call_output" => {
                                item["call_id"] = json!(mapped);
                            }
                            _ => {}
                        }
                    }
                    assert_eq!(restored_prefix, before, "{api:?}");
                    assert_estimated(
                        decide(Some(&usage_anchor), &messages, &before_request),
                        &messages,
                        &before_request.tools,
                    );
                }
            }
        }
    }

    #[test]
    fn adjacent_tool_images_change_chat_grouping_and_force_conservative_fallback() {
        for api in [WireApi::Chat, WireApi::Responses] {
            for image_index in [0, 1] {
                for appended_image in [false, true] {
                    let mut messages = initial_messages(api);
                    let calls = ["first", "second", "third"]
                        .into_iter()
                        .map(screenshot_call)
                        .collect();
                    messages.push(live_assistant_with_calls(api, calls));
                    for (index, id) in ["first", "second"].into_iter().enumerate() {
                        let mut result = ChatMessage::tool_result(id, "covered result");
                        if index == image_index {
                            result.images.push(image([255, 0, 0]));
                        }
                        messages.push(result);
                    }
                    let before_request = request(api, &messages);
                    let before = api.body(&before_request);
                    let usage_anchor = anchor(&before_request, INPUT);
                    assert_anchored(
                        decide(Some(&usage_anchor), &messages, &before_request),
                        &before_request,
                        &messages,
                        INPUT,
                    );

                    let mut appended = ChatMessage::tool_result("third", "appended result");
                    if appended_image {
                        appended.images.push(image([0, 0, 255]));
                    }
                    messages.push(appended);
                    let after_request = request(api, &messages);
                    assert_eq!(
                        &after_request.messages[..before_request.messages.len()],
                        before_request.messages.as_slice()
                    );
                    let after = api.body(&after_request);
                    let before_items = sequence(api, &before);
                    let after_items = sequence(api, &after);
                    match api {
                        WireApi::Chat => {
                            let pending_index = before_items.len() - 1;
                            assert_eq!(
                                &before_items[..pending_index],
                                &after_items[..pending_index]
                            );
                            assert_eq!(after_items.len(), before_items.len() + 1);
                            assert_eq!(before_items[pending_index]["role"], "user");
                            assert_eq!(after_items[pending_index]["role"], "tool");
                            assert_eq!(after_items[pending_index]["tool_call_id"], "third");
                            assert_eq!(after_items[pending_index + 1]["role"], "user");
                            let old_parts =
                                before_items[pending_index]["content"].as_array().unwrap();
                            let new_parts = after_items[pending_index + 1]["content"]
                                .as_array()
                                .unwrap();
                            assert_eq!(old_parts.len(), 2);
                            assert_eq!(old_parts.as_slice(), &new_parts[..old_parts.len()]);
                            assert_eq!(old_parts[1]["type"], "image_url");
                            if appended_image {
                                assert_eq!(new_parts.len(), 4);
                                assert_eq!(new_parts[3]["type"], "image_url");
                                assert_ne!(old_parts[1], new_parts[3]);
                            } else {
                                assert_eq!(new_parts.len(), 2);
                            }
                            let mut restored_pending = after_items[pending_index + 1].clone();
                            restored_pending["content"]
                                .as_array_mut()
                                .unwrap()
                                .truncate(old_parts.len());
                            assert_eq!(restored_pending, before_items[pending_index]);
                        }
                        WireApi::Responses => {
                            assert_wire_prefix(api, &before, &after);
                            assert_eq!(after_items.len(), before_items.len() + 1);
                            let added = after_items.last().unwrap();
                            assert_eq!(added["type"], "function_call_output");
                            assert_eq!(added["call_id"], "third");
                            if appended_image {
                                assert_eq!(added["output"][0]["text"], "appended result");
                                assert_eq!(added["output"][1]["type"], "input_image");
                            } else {
                                assert_eq!(added["output"], "appended result");
                            }
                        }
                    }
                    assert_estimated(
                        decide(Some(&usage_anchor), &messages, &before_request),
                        &messages,
                        &before_request.tools,
                    );
                }
            }
        }
    }

    #[test]
    fn image_group_followed_by_a_new_tool_round_keeps_wire_prefix_and_usage() {
        for api in [WireApi::Chat, WireApi::Responses] {
            let mut messages = initial_messages(api);
            messages.push(live_assistant_with_calls(
                api,
                vec![screenshot_call("image-call")],
            ));
            let mut result = ChatMessage::tool_result("image-call", "image result");
            result.images.push(image([255, 0, 0]));
            messages.push(result);
            let before_request = request(api, &messages);
            let before = api.body(&before_request);
            let usage_anchor = anchor(&before_request, INPUT);

            append_live_round(api, &mut messages, "next-call");
            let after_request = request(api, &messages);
            let after = api.body(&after_request);
            assert_wire_prefix(api, &before, &after);
            assert_anchored(
                decide(Some(&usage_anchor), &messages, &before_request),
                &before_request,
                &messages,
                INPUT,
            );
        }
    }
}

#[test]
fn absent_or_changed_frozen_identity_does_not_reuse_usage() {
    let messages = cold_history(2);
    let request = projected_request(&messages, &tools());
    let usage = anchor(&request, 1234);
    let mut missing = request.clone();
    missing.model_snapshot = None;
    assert!(pending(&missing).complete(1234).is_none());
    assert_estimated(
        decide(Some(&usage), &messages, &missing),
        &messages,
        &missing.tools,
    );
    let mut changed = request.clone();
    use crate::model_compat::{
        ModelApi, ModelIdentity, ModelRoute, lookup_model_definition, resolve_model,
    };
    let route = ModelRoute::new(ModelApi::Responses, "https://api.openai.com/v1").unwrap();
    let identity = ModelIdentity {
        provider: "openai".into(),
        model: MODEL.into(),
    };
    changed.model_snapshot = Some(Box::new(
        resolve_model(&lookup_model_definition(&route, &identity).unwrap(), None).unwrap(),
    ));
    assert_estimated(
        decide(Some(&usage), &messages, &changed),
        &messages,
        &changed.tools,
    );
    assert_eq!(
        decide(Some(&usage), &messages, &request).source,
        ContextAccountingSource::UsageAnchored
    );
}
