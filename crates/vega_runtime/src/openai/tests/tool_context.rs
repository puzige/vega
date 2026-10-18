use super::*;

fn read_pair(id: &str) -> [ChatMessage; 2] {
    let call = crate::RuntimeToolCall {
        id: id.into(),
        name: "read".into(),
        input_json: r#"{"file_path":"/resolved/private/file"}"#.into(),
    };
    let wire = ChatToolCall {
        id: id.into(),
        name: "read".into(),
        input_json: r#"{"path":"file"}"#.into(),
    };
    let reasoning = serde_json::json!({"type":"reasoning","id":format!("reasoning-{id}"),"phase":"analysis","summary":[],"encrypted_content":format!("opaque-{id}")});
    let function = serde_json::json!({"type":"function_call","call_id":id,"name":"read","arguments":wire.input_json,"phase":"commentary"});
    let mut assistant = ChatMessage::assistant_with_tools(
        "",
        vec![ChatToolCall {
            id: call.id.clone(),
            name: call.name.clone(),
            input_json: call.input_json.clone(),
        }],
    );
    assistant.reasoning_content = Some(format!("private-{id}"));
    assistant.response_reasoning = vec![reasoning.clone()];
    assistant.response_replay_layout = Some(crate::ResponseReplayLayout {
        response_id: Some(format!("response-{id}")),
        slots: vec![
            crate::ResponseReplaySlot::Reasoning(reasoning),
            crate::ResponseReplaySlot::FunctionCall {
                item: function,
                call_id: id.into(),
                name: "read".into(),
                arguments: wire.input_json.clone(),
            },
        ],
    });
    assistant.bind_live_tool_calls(vec![Some(wire)]).unwrap();
    let result = crate::RuntimeToolResult {
        call_id: id.into(),
        output: "complete evidence ".repeat(1_000),
        status: crate::RuntimeToolStatus::Success,
        reused: false,
        exit_code: None,
        duration_ms: None,
        truncated: Some(false),
        approval: None,
        remember_rule: None,
    };
    let mut message = ChatMessage::runtime_tool_result(&call, &result);
    if id == "first" {
        message.attach_progress_reminder();
    }
    [assistant, message]
}

#[test]
fn tool_context_projection_preserves_warm_reasoning_and_identical_wire_prefixes() {
    for policy in [
        ReasoningReplayPolicy::DeepSeekTools,
        ReasoningReplayPolicy::ResponsesItems,
    ] {
        let mut system = ChatMessage::new(ChatRole::System, "system prefix");
        system.attach_replay_frame(crate::ReplayFrame::new(1, policy));
        let mut source = vec![system];
        source.extend(read_pair("first"));
        let first = crate::project_model_messages(&source).unwrap();
        source.extend(read_pair("second"));
        let second = crate::project_model_messages(&source).unwrap();
        let mut append = first.clone();
        append.extend_from_slice(&source[3..]);
        assert_eq!(crate::project_model_messages(&append).unwrap(), second);
        assert_eq!(crate::project_model_messages(&second).unwrap(), second);
        assert_eq!(&second[..first.len()], first.as_slice());
        for index in [1, 3] {
            assert_eq!(
                second[index].reasoning_content,
                source[index].reasoning_content
            );
            assert_eq!(
                second[index].response_reasoning,
                source[index].response_reasoning
            );
            assert_eq!(
                second[index].response_replay_layout,
                source[index].response_replay_layout
            );
            assert_eq!(second[index].tool_calls[0].input_json, r#"{"path":"file"}"#);
        }
        assert!(second[2].content.contains("vega_tool_preview"));
        assert_eq!(
            second[2].content.matches("<vega_runtime_notice>").count(),
            1
        );
        let reference: serde_json::Value = serde_json::from_str(&second[4].content).unwrap();
        assert_eq!(reference["vega_tool_result_reference"]["call_id"], "first");
        let profile = FrozenReasoning {
            provider: "provider".into(),
            model: MODEL.into(),
            protocol: if policy == ReasoningReplayPolicy::ResponsesItems {
                ReasoningProtocol::ResponsesApi
            } else {
                ReasoningProtocol::DeepSeekChatCompletions
            },
            replay_policy: policy,
            choice: ReasoningChoice::ProviderDefault,
            supports_disabled: false,
            disabled_wire: None,
            declared_efforts: Vec::new(),
        };
        let request = |messages| ChatRequest {
            model: MODEL.into(),
            messages,
            reasoning: Some(profile.clone()),
            ..Default::default()
        };
        if policy == ReasoningReplayPolicy::ResponsesItems {
            let before = responses::build_body(&request(first.clone())).unwrap();
            let after = responses::build_body(&request(second.clone())).unwrap();
            let before = before["input"].as_array().unwrap();
            let after = after["input"].as_array().unwrap();
            assert_eq!(before, &after[..before.len()]);
            assert_eq!(after[1], source[1].response_reasoning[0]);
            assert_eq!(after[2]["phase"], "commentary");
            assert_eq!(after[2]["arguments"], r#"{"path":"file"}"#);
            assert_eq!(after[3]["output"], second[2].content);
            assert_eq!(after[4], source[3].response_reasoning[0]);
            assert_eq!(after[6]["output"], second[4].content);
        } else {
            let before = build_request_body(&request(first.clone()));
            let after = build_request_body(&request(second.clone()));
            let before = before["messages"].as_array().unwrap();
            let after = after["messages"].as_array().unwrap();
            assert_eq!(before, &after[..before.len()]);
            assert_eq!(after[1]["reasoning_content"], "private-first");
            assert_eq!(after[3]["reasoning_content"], "private-second");
            assert_eq!(after[2]["content"], second[2].content);
            assert_eq!(after[4]["content"], second[4].content);
        }
        assert_eq!(
            crate::estimate_wire_context(&source, &[]).unwrap(),
            crate::estimate_wire_context(&second, &[]).unwrap()
        );
        assert_eq!(
            crate::estimate_chat_context("system prefix", &source[1..], &[]).unwrap(),
            crate::estimate_wire_context(&second, &[]).unwrap()
        );
        assert_eq!(source[2].content, "complete evidence ".repeat(1_000));
        assert_eq!(source[4].content, source[2].content);
        let length = source.len();
        source[0].attach_replay_frame(crate::ReplayFrame::new(
            length,
            ReasoningReplayPolicy::Unknown,
        ));
        let cold = crate::project_model_messages(&source).unwrap();
        assert!(cold.iter().all(|message| message.tool_calls.is_empty()
            && message.tool_call_id.is_none()
            && !message.content.contains("opaque-first")
            && !message.content.contains("private-first")
            && !message.content.contains("vega_tool_result_reference")));
        let cold_outputs = cold
            .iter()
            .filter_map(|message| serde_json::from_str::<serde_json::Value>(&message.content).ok())
            .filter(|value| value["vega_history_observation"]["kind"] == "tool_result")
            .collect::<Vec<_>>();
        assert_eq!(cold_outputs.len(), 2);
        assert!(cold_outputs.iter().all(|value| {
            value["vega_history_observation"]["output"]
                .as_str()
                .unwrap()
                .contains("vega_tool_preview")
        }));
        assert_eq!(crate::project_model_messages(&cold).unwrap(), cold);
    }
}
