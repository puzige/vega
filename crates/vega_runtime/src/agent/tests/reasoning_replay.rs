use super::*;
use crate::{
    MockProvider, ProviderEvent, ReasoningReplayPolicy, ReplayFrame, ScriptStep, StopReason,
};

struct ReplaySteeringSource(Mutex<std::collections::VecDeque<Option<crate::RuntimeSteeringInput>>>);

impl crate::RuntimeSteeringSource for ReplaySteeringSource {
    fn take(
        &self,
    ) -> futures::future::BoxFuture<'_, Result<Option<crate::RuntimeSteeringInput>, crate::VegaError>>
    {
        async move { Ok(self.0.lock().unwrap().pop_front().flatten()) }.boxed()
    }
}

fn replay_seed(outcome: &AgentOutcome, route: [u8; 32]) -> ReplayFrame {
    let fingerprint = outcome.messages[0]
        .replay_frame()
        .and_then(ReplayFrame::environment_fingerprint)
        .unwrap();
    let mut frame = ReplayFrame::new(1, ReasoningReplayPolicy::DeepSeekTools);
    frame.attach_route_fingerprint(route);
    frame.attach_candidate(outcome.messages.clone(), fingerprint);
    frame
}

async fn run_seed_with_route() -> AgentOutcome {
    let project = tempdir().unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("candidate-only-answer".into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]);
    let route = [21; 32];
    let mut req = request(vec![ChatMessage::new(ChatRole::User, "seed user")]);
    let mut frame = ReplayFrame::new(1, ReasoningReplayPolicy::DeepSeekTools);
    frame.attach_route_fingerprint(route);
    req.replay_frame = Some(frame);
    run_agent(&provider, &tools, req, CancellationToken::new())
        .await
        .unwrap()
}

async fn assert_candidate_environment_change_cools_history(
    mut configure: impl FnMut(&mut AgentRequest),
) {
    let seed = run_seed_with_route().await;
    let route = [21; 32];
    let project = tempdir().unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(vec![ProviderEvent::Done {
        stop_reason: StopReason::End,
    }])]);
    let mut req = request(vec![ChatMessage::new(ChatRole::User, "cold fallback user")]);
    configure(&mut req);
    req.replay_frame = Some(replay_seed(&seed, route));
    let outcome = run_agent(&provider, &tools, req, CancellationToken::new())
        .await
        .unwrap();
    assert!(!outcome.failed);
    let request = &provider.requests()[0];
    assert!(
        request
            .messages
            .iter()
            .any(|message| message.content == "cold fallback user")
    );
    assert!(
        request
            .messages
            .iter()
            .all(|message| { !message.content.contains("candidate-only-answer") })
    );
}

#[tokio::test]
async fn omitted_replay_frame_enters_runtime_as_cold_history() {
    let project = tempdir().unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(vec![ProviderEvent::Done {
        stop_reason: StopReason::End,
    }])]);
    let req = request(vec![
        ChatMessage::new(ChatRole::User, "previous user"),
        ChatMessage::new(ChatRole::Assistant, "previous assistant answer"),
        ChatMessage::new(ChatRole::User, "current user"),
    ]);
    let outcome = run_agent(&provider, &tools, req, CancellationToken::new())
        .await
        .unwrap();
    let requests = provider.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].messages[0].role, ChatRole::System);
    assert_eq!(requests[0].messages[1].content, "previous user");
    assert_eq!(requests[0].messages[2].role, ChatRole::User);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&requests[0].messages[2].content).unwrap()["vega_history_observation"]
            ["text"],
        "previous assistant answer"
    );
    assert_eq!(requests[0].messages[3].content, "current user");
    assert!(
        outcome.messages[0]
            .replay_frame()
            .is_some_and(|frame| frame.cold_prefix_len() == 4)
    );
}

#[tokio::test]
async fn automatic_replay_final_runtime_system_change_keeps_cold_fallback() {
    assert_candidate_environment_change_cools_history(|request| {
        request.system_prompt = "Changed system instruction".into();
    })
    .await;
}

#[tokio::test]
async fn automatic_replay_actual_tool_definition_change_keeps_cold_fallback() {
    assert_candidate_environment_change_cools_history(|request| {
        request.tool_config = RuntimeToolConfig::new(
            RuntimeRunMode::Execute,
            RuntimePermissionMode::Confirm,
            "project-one".into(),
            "thread-one".into(),
            std::path::PathBuf::from("/tmp/project-one"),
            Vec::new(),
        );
    })
    .await;
}

#[tokio::test]
async fn automatic_replay_skill_catalog_change_keeps_cold_fallback() {
    assert_candidate_environment_change_cools_history(|request| {
        let catalog = crate::skills::SkillCatalog::freeze(Vec::new(), &[], false).unwrap();
        request.tool_config = RuntimeToolConfig::default()
            .with_skill_run(crate::skills::SkillRun::new(catalog, true), Vec::new());
    })
    .await;
}

#[tokio::test]
async fn automatic_replay_computer_use_snapshot_change_keeps_cold_fallback() {
    assert_candidate_environment_change_cools_history(|request| {
        let run = crate::computer_use::ComputerUseRun::new(true, false);
        request.tool_config.computer_use = Some(run);
    })
    .await;
}

#[tokio::test]
async fn automatic_replay_context_budget_threshold_change_keeps_cold_fallback() {
    let seed = run_seed_with_route().await;
    let route = [21; 32];
    let project = tempdir().unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(vec![ProviderEvent::Done {
        stop_reason: StopReason::End,
    }])]);
    let mut req = request(vec![ChatMessage::new(ChatRole::User, "cold fallback user")]);
    req.context_budget = Some(crate::ContextBudget::new(12_000, 2_000, true).unwrap());
    req.replay_frame = Some(replay_seed(&seed, route));
    let outcome = run_agent(&provider, &tools, req, CancellationToken::new())
        .await
        .unwrap();
    assert!(!outcome.failed);
    let request = &provider.requests()[0];
    assert!(
        request
            .messages
            .iter()
            .any(|message| message.content == "cold fallback user")
    );
    assert!(
        request
            .messages
            .iter()
            .all(|message| !message.content.contains("candidate-only-answer"))
    );
}

async fn run_glm_two_user_rounds(replay_policy: ReasoningReplayPolicy) -> Vec<crate::ChatRequest> {
    let project = tempdir().unwrap();
    let data = tempdir().unwrap();
    std::fs::write(project.path().join("source.txt"), "source output").unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ThinkingDelta("first chain".into()),
            ProviderEvent::ChatReasoningContent(crate::ChatReasoningPresence::NonEmpty(
                "first chain".into(),
            )),
            ProviderEvent::ToolUse {
                id: "read-first".into(),
                name: "read".into(),
                input_json: r#"{"path":"source.txt"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("first final answer".into()),
            ProviderEvent::ChatReasoningContent(crate::ChatReasoningPresence::Empty),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::ThinkingDelta("second chain".into()),
            ProviderEvent::ChatReasoningContent(crate::ChatReasoningPresence::NonEmpty(
                "second chain".into(),
            )),
            ProviderEvent::ToolUse {
                id: "read-second".into(),
                name: "read".into(),
                input_json: r#"{"path":"source.txt"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("second final answer".into()),
            ProviderEvent::ChatReasoningContent(crate::ChatReasoningPresence::Empty),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
    ]);
    let steering = Arc::new(ReplaySteeringSource(Mutex::new(
        std::collections::VecDeque::from([
            None,
            Some(crate::RuntimeSteeringInput {
                original_content: "second user request".into(),
                user_message_seq: 2,
                id: "user-second".into(),
                assistant_message_id: "assistant-second".into(),
                message: ChatMessage::new(ChatRole::User, "second user request"),
            }),
        ]),
    )));
    let reasoning = FrozenReasoning {
        provider: "zhipu".into(),
        model: "glm-5.3".into(),
        protocol: ReasoningProtocol::ZhipuChatCompletions,
        replay_policy,
        choice: ReasoningChoice::Effort("high".into()),
        supports_disabled: false,
        disabled_wire: None,
        declared_efforts: vec!["high".into()],
    };
    let mut req = request(vec![ChatMessage::new(ChatRole::User, "first user request")]);
    req.model = reasoning.model.clone();
    req.reasoning = Some(reasoning);
    req.replay_frame = Some(ReplayFrame::new(2, replay_policy));
    req.tool_config = RuntimeToolConfig::new(
        RuntimeRunMode::Ask,
        RuntimePermissionMode::ReadOnly,
        "project-one".into(),
        "thread-one".into(),
        data.path().join("checkpoints"),
        Vec::new(),
    )
    .with_steering_source(steering);
    let outcome = run_agent(&provider, &tools, req, CancellationToken::new())
        .await
        .unwrap();
    assert!(!outcome.failed);
    assert_eq!(outcome.final_text, "second final answer");
    assert_eq!(outcome.executed_tool_call_count, 2);
    assert_eq!(
        outcome
            .events
            .iter()
            .filter(|event| matches!(event, RuntimeEvent::SteeringAccepted(_)))
            .count(),
        1
    );
    provider.requests()
}

#[tokio::test]
async fn automatic_replay_glm_preserved_and_interleaved_keep_distinct_user_ranges() {
    let preserved = run_glm_two_user_rounds(ReasoningReplayPolicy::GlmPreserved).await;
    let interleaved = run_glm_two_user_rounds(ReasoningReplayPolicy::GlmInterleaved).await;
    assert_eq!(preserved.len(), 4);
    assert_eq!(interleaved.len(), 4);
    for requests in [&preserved, &interleaved] {
        assert!(requests[2].messages.iter().any(|message| {
            message.role == ChatRole::User && message.content == "second user request"
        }));
        let latest = &requests[3].messages;
        assert!(
            latest
                .iter()
                .any(|message| message.content == "first final answer")
        );
        assert!(latest.iter().any(|message| {
            message.role == ChatRole::Tool
                && message.tool_call_id.as_deref() == Some("read-first")
                && message.content.contains("source output")
        }));
        assert!(latest.iter().any(|message| {
            message
                .tool_calls
                .iter()
                .any(|call| call.id == "read-first")
        }));
        assert!(latest.iter().any(|message| {
            message.role == ChatRole::Tool
                && message.tool_call_id.as_deref() == Some("read-second")
                && message.content.contains("source output")
        }));
        assert!(latest.iter().any(|message| {
            message
                .tool_calls
                .iter()
                .any(|call| call.id == "read-second")
        }));
    }
    let first_round_preserved = preserved[3]
        .messages
        .iter()
        .find(|message| {
            message
                .tool_calls
                .iter()
                .any(|call| call.id == "read-first")
        })
        .and_then(|message| message.reasoning_content.as_deref());
    let first_round_interleaved = interleaved[3]
        .messages
        .iter()
        .find(|message| {
            message
                .tool_calls
                .iter()
                .any(|call| call.id == "read-first")
        })
        .and_then(|message| message.reasoning_content.as_deref());
    let second_round_interleaved = interleaved[3]
        .messages
        .iter()
        .find(|message| {
            message
                .tool_calls
                .iter()
                .any(|call| call.id == "read-second")
        })
        .and_then(|message| message.reasoning_content.as_deref());
    let second_round_preserved = preserved[3]
        .messages
        .iter()
        .find(|message| {
            message
                .tool_calls
                .iter()
                .any(|call| call.id == "read-second")
        })
        .and_then(|message| message.reasoning_content.as_deref());
    assert_eq!(first_round_preserved, Some("first chain"));
    assert_eq!(first_round_interleaved, None);
    assert_eq!(second_round_interleaved, Some("second chain"));
    assert_eq!(second_round_preserved, Some("second chain"));
}
