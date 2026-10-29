use super::*;

#[tokio::test]
async fn delegated_arming_requires_a_persisted_nonfinal_response_and_keeps_foreground_suppression()
-> TestResult {
    for case in [
        "tools",
        "initial",
        "estimate",
        "limit",
        "steering",
        "automatic",
        "auto_off",
        "ack",
    ] {
        let trace = Trace::default();
        let hook = RecordingHook::new(trace.clone(), false, false);
        let mut request = request(hook.clone());
        request.tool_config = request.tool_config.with_delegated_compaction(true);
        if matches!(case, "estimate" | "automatic") {
            request.history.extend([
                ChatMessage::new(ChatRole::Assistant, "a".repeat(350_000)),
                ChatMessage::new(ChatRole::User, "Continue the task"),
            ]);
            let input = estimate_wire_context(
                &fixture_wire(&request),
                &vega_runtime::tool_definitions(request.tool_config.run_mode),
            )?;
            let reserve = if case == "estimate" { 25_000 } else { 16_000 };
            let budget = ContextBudget::new(input.input_tokens + reserve, 10_000, true)?;
            assert!(input.input_tokens >= budget.precompute_tokens()?);
            assert_eq!(
                matches!(budget.check(input)?, vega_runtime::ContextCheck::Triggered),
                case == "automatic"
            );
            request.context_budget = Some(budget);
        }
        if case == "limit" {
            request.tool_config = request.tool_config.with_turn_limit(1);
        }
        if case == "steering" {
            request.tool_config = request
                .tool_config
                .with_steering_source(Arc::new(OneSteering(Mutex::new(Some(
                    RuntimeSteeringInput {
                        original_content: "New task".into(),
                        user_message_seq: 4,
                        id: "new-user".into(),
                        assistant_message_id: "new-owner".into(),
                        message: ChatMessage::new(ChatRole::User, "New task"),
                    },
                )))));
        }
        if case == "auto_off" {
            request.context_budget = request
                .context_budget
                .map(ContextBudget::without_automatic_compaction);
        }
        let input = request.context_budget.unwrap().precompute_tokens()? + 1000;
        let mut first = response(input, StopReason::ToolUse);
        first.insert(
            2,
            Ok(ProviderEvent::ToolUse {
                id: "call".into(),
                name: "unavailable_tool".into(),
                input_json: "{}".into(),
            }),
        );
        let replies = if matches!(case, "initial" | "estimate") {
            vec![response(input, StopReason::End)]
        } else if matches!(case, "limit" | "ack") {
            vec![first]
        } else {
            vec![first, response(1000, StopReason::End)]
        };
        let provider = ScriptedProvider::new(trace.clone(), replies);
        let tools = vega_tools::Tools::new(std::env::current_dir()?)?;
        let outcome = run_agent_with_sink(
            &provider,
            &tools,
            request,
            CancellationToken::new(),
            |event| {
                sink_event(&trace, &event);
                std::future::ready(
                    if case == "ack" && matches!(event, RuntimeEvent::ToolCallFinished(_)) {
                        Err(VegaError::CredentialCheckUnavailable)
                    } else {
                        Ok(())
                    },
                )
            },
        )
        .await;
        if case == "ack" {
            assert!(outcome.is_err());
        } else {
            assert!(!outcome?.failed, "{case}");
        }
        let armed = hook.armed.lock().unwrap();
        assert_eq!(armed.len(), usize::from(case == "tools"), "{case}");
        assert_eq!(
            hook.compacted.lock().unwrap().len(),
            usize::from(case == "automatic"),
            "{case}"
        );
        assert!(provider.replies.lock().unwrap().is_empty(), "{case}");
        if case == "tools" {
            assert!(
                armed[0]
                    .messages
                    .iter()
                    .any(|message| message.tool_call_id.as_deref() == Some("call"))
            );
            let trace = trace.lock().unwrap();
            let arm = trace.iter().position(|value| *value == "arm").unwrap();
            assert!(trace.iter().position(|value| *value == "tool").unwrap() < arm);
            assert!(arm < trace.iter().position(|value| *value == "finished").unwrap());
        }
    }
    Ok(())
}
