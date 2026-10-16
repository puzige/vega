use super::*;

#[test]
fn issue300_bash_audit_accepts_only_its_reason_or_exact_legacy_feedback() {
    let audit = InvalidBashAudit::from_raw(r#"{"command":[]}"#).unwrap();
    assert!(audit.matches_feedback(&vega_tools::BashInputError::WrongCommandType.feedback()));
    assert!(!audit.matches_feedback(&vega_tools::BashInputError::MissingCommand.feedback()));
    assert!(!audit.matches_feedback(BASH_INVALID_INPUT_OUTPUT));
    assert!(!audit.matches_feedback(LEGACY_BASH_INVALID_INPUT_OUTPUT));
    let mut value: serde_json::Value = serde_json::from_str(&audit.to_json().unwrap()).unwrap();
    value["validation_error_code"] = serde_json::Value::Null;
    assert!(InvalidBashAudit::from_json(&value.to_string()).is_none());
    value
        .as_object_mut()
        .unwrap()
        .remove("validation_error_code");
    let legacy = InvalidBashAudit::from_json(&value.to_string()).unwrap();
    assert!(legacy.matches_feedback(BASH_INVALID_INPUT_OUTPUT));
    assert!(!legacy.matches_feedback(&audit.feedback()));
    assert!(!legacy.matches_feedback(&format!("{BASH_INVALID_INPUT_OUTPUT} altered")));
    assert!(legacy.semantically_equal(&audit));
    value["validation_error_code"] = serde_json::json!("missing_command");
    assert!(!audit.semantically_equal(&InvalidBashAudit::from_json(&value.to_string()).unwrap()));
}

#[test]
fn issue300_every_model_and_transport_uses_the_same_core_contract() {
    let definitions = tool_definitions(RuntimeRunMode::Execute);
    for model in ["deepseek-v4.1-flash", "glm-5.3", "unlisted-model"] {
        let request = ChatRequest {
            model: model.into(),
            messages: vec![ChatMessage::new(ChatRole::User, "fixture")],
            tools: definitions.clone(),
            ..Default::default()
        };
        let chat = crate::openai::build_request_body(&request);
        let responses = crate::openai::responses::build_body(&request).unwrap();
        for name in ["read", "write", "edit", "bash"] {
            let definition = definitions
                .iter()
                .find(|definition| definition.name == name)
                .unwrap();
            let chat_tool = chat["tools"]
                .as_array()
                .unwrap()
                .iter()
                .find(|tool| tool["function"]["name"] == name)
                .unwrap();
            let responses_tool = responses["tools"]
                .as_array()
                .unwrap()
                .iter()
                .find(|tool| tool["name"] == name)
                .unwrap();
            assert_eq!(chat_tool["function"]["parameters"], definition.input_schema);
            assert_eq!(responses_tool["parameters"], definition.input_schema);
            assert_eq!(chat_tool["function"]["strict"], true);
            assert_eq!(responses_tool["strict"], true);
        }
        assert!(!definitions.iter().any(|definition| matches!(
            definition.name.as_str(),
            "Read" | "Write" | "Edit" | "apply_patch"
        )));
    }
}

#[tokio::test]
async fn issue300_live_write_arguments_are_ephemeral_and_not_audit_protocol() {
    let project = tempdir().unwrap();
    let data = tempdir().unwrap();
    fs::create_dir(data.path().join("checkpoints")).unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "write-1".into(),
                name: "write".into(),
                input_json: r#"{"path":"new.txt","content":"fixture-body"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ]);
    let mut req = request(Vec::new());
    req.tool_config = tool_config(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::FullAccess,
        data.path().join("checkpoints"),
    );
    let outcome = run_agent(&provider, &tools, req, CancellationToken::new())
        .await
        .unwrap();
    let requests = provider.requests();
    let call = requests[1]
        .messages
        .iter()
        .flat_map(|message| &message.tool_calls)
        .find(|call| call.id == "write-1")
        .unwrap();
    let input: serde_json::Value = serde_json::from_str(&call.input_json).unwrap();
    assert_eq!(input["content"], "fixture-body");
    assert!(input.get("audit_version").is_none());
    assert!(!format!("{:?}", outcome.events).contains("fixture-body"));
    assert!(!format!("{:?}", outcome.messages).contains("fixture-body"));
    assert_eq!(
        crate::estimate_wire_context(&outcome.messages, &requests[1].tools).unwrap(),
        crate::estimate_wire_context(
            &crate::project_model_messages(&outcome.messages).unwrap(),
            &requests[1].tools
        )
        .unwrap()
    );
    let persisted = outcome
        .events
        .iter()
        .find_map(|event| match event {
            RuntimeEvent::ToolCallProposed(call) => Some(call),
            _ => None,
        })
        .unwrap();
    assert!(persisted.input_json.contains("write_edit_v1"));
    assert!(!persisted.input_json.contains("fixture-body"));
}

#[tokio::test]
async fn issue300_invalid_arguments_become_non_executable_feedback() {
    let project = tempdir().unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "bad".into(),
                name: "write".into(),
                input_json: r#"{"path":"new.txt","secret":"fixture-private-value"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ]);
    let outcome = run_agent(
        &provider,
        &tools,
        request(Vec::new()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(outcome.executed_tool_call_count, 0);
    let requests = provider.requests();
    assert!(
        requests[1]
            .messages
            .iter()
            .all(|message| message.tool_calls.is_empty() && message.role != ChatRole::Tool)
    );
    let wire = crate::openai::build_request_body(&requests[1]).to_string();
    assert!(!wire.contains("fixture-private-value"));
    assert!(!wire.contains("write_edit_invalid_v1"));
    assert!(wire.contains("missing_content"));
}

#[tokio::test]
async fn issue300_redacted_live_arguments_are_never_replayed_with_placeholder_content() {
    let project = tempdir().unwrap();
    let data = tempdir().unwrap();
    let secret = "private-credential-fixture-abcdefghijklmnopqrstuvwxyz";
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "secret-write".into(),
                name: "write".into(),
                input_json: serde_json::json!({"path":"new.txt","content":secret}).to_string(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ]);
    let mut req = request(vec![]);
    req.tool_config = tool_config(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::FullAccess,
        data.path().to_path_buf(),
    )
    .with_credential_reader(Arc::new(move || Ok(vec![secret.to_owned()])));
    let outcome = run_agent(&provider, &tools, req, CancellationToken::new())
        .await
        .unwrap();
    let requests = provider.requests();
    assert!(
        requests[1]
            .messages
            .iter()
            .all(|message| message.tool_calls.is_empty() && message.tool_call_id.is_none())
    );
    let wire = crate::openai::build_request_body(&requests[1]).to_string();
    assert!(wire.contains("Historical tool result"));
    assert!(!wire.contains(secret));
    assert!(!wire.contains("write_edit_v1"));
    assert!(!format!("{:?}", outcome.messages).contains(secret));
}
