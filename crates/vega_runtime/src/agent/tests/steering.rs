use super::*;

struct SteeringFixture(Mutex<Option<RuntimeSteeringInput>>);

impl RuntimeSteeringSource for SteeringFixture {
    fn take(&self) -> BoxFuture<'_, Result<Option<RuntimeSteeringInput>, VegaError>> {
        async move { Ok(self.0.lock().unwrap().take()) }.boxed()
    }
}

fn steering() -> Arc<SteeringFixture> {
    Arc::new(SteeringFixture(Mutex::new(Some(RuntimeSteeringInput {
        original_content: "follow-up".into(),
        user_message_seq: 2,
        id: "pending-b".into(),
        assistant_message_id: "assistant-b".into(),
        message: ChatMessage::new(ChatRole::User, "follow-up"),
    }))))
}

#[tokio::test]
async fn issue299_final_response_boundary_consumes_steer_and_continues() {
    let project = tempdir().unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![
            ScriptStep::text("first"),
            ScriptStep::events(vec![ProviderEvent::Done {
                stop_reason: StopReason::End,
            }]),
        ],
        vec![
            ScriptStep::text("second"),
            ScriptStep::events(vec![ProviderEvent::Done {
                stop_reason: StopReason::End,
            }]),
        ],
    ]);
    let mut req = request(vec![ChatMessage::new(ChatRole::User, "original")]);
    req.tool_config = req.tool_config.with_steering_source(steering());
    let outcome = run_agent(&provider, &tools, req, CancellationToken::new())
        .await
        .unwrap();
    let roles = outcome
        .messages
        .iter()
        .map(|message| message.role)
        .collect::<Vec<_>>();
    assert_eq!(
        roles,
        [
            ChatRole::System,
            ChatRole::User,
            ChatRole::Assistant,
            ChatRole::User,
            ChatRole::Assistant
        ]
    );
    assert_eq!(outcome.final_text, "second");
    assert_eq!(
        outcome
            .events
            .iter()
            .filter(|event| matches!(event, RuntimeEvent::SteeringAccepted(_)))
            .count(),
        1
    );
}

#[tokio::test]
async fn issue299_cancel_and_turn_limit_preserve_unconsumed_steer() {
    for cancel_run in [false, true] {
        let project = tempdir().unwrap();
        let tools = vega_tools::Tools::new(project.path()).unwrap();
        let provider = MockProvider::new(vec![
            ScriptStep::text("first"),
            ScriptStep::events(vec![ProviderEvent::Done {
                stop_reason: StopReason::End,
            }]),
        ]);
        let source = steering();
        let mut req = request(Vec::new());
        req.tool_config = req
            .tool_config
            .with_steering_source(source.clone())
            .with_turn_limit(1);
        let cancel = CancellationToken::new();
        if cancel_run {
            cancel.cancel();
        }
        let _ = run_agent(&provider, &tools, req, cancel).await.unwrap();
        assert!(source.0.lock().unwrap().is_some());
    }
}

#[tokio::test]
async fn issue299_steer_waits_for_complete_tool_batch_and_preserves_pairing() {
    let project = tempdir().unwrap();
    let data = tempdir().unwrap();
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let worker_entered = entered.clone();
    let worker_release = release.clone();
    let tools = vega_tools::Tools::new(project.path())
        .unwrap()
        .with_bash_test_executor(Arc::new(move |_, _, cancel| {
            let entered = worker_entered.clone();
            let release = worker_release.clone();
            Box::pin(async move {
                entered.notify_one();
                release.notified().await;
                assert!(!cancel.is_cancelled());
                Ok(vega_tools::BashOutput {
                    text: "tool-result".into(),
                    exit_code: 0,
                    duration_ms: 1,
                    truncated: false,
                })
            })
        }));
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "call-a".into(),
                name: "bash".into(),
                input_json: r#"{"cmd":"printf first"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![
            ScriptStep::text("after-steer"),
            ScriptStep::events(vec![ProviderEvent::Done {
                stop_reason: StopReason::End,
            }]),
        ],
    ]);
    let source = Arc::new(SteeringFixture(Mutex::new(None)));
    let mut req = request(vec![ChatMessage::new(ChatRole::User, "original")]);
    req.tool_config = tool_config(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::FullAccess,
        data.path().join("checkpoints"),
    )
    .with_steering_source(source.clone());
    let run = run_agent(&provider, &tools, req, CancellationToken::new());
    let inject = async {
        entered.notified().await;
        assert_eq!(provider.requests().len(), 1);
        *source.0.lock().unwrap() = steering().0.lock().unwrap().take();
        release.notify_one();
    };
    let (outcome, ()) = tokio::join!(run, inject);
    assert!(!outcome.unwrap().interrupted);
    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    assert!(
        !requests[0]
            .messages
            .iter()
            .any(|message| message.content == "follow-up")
    );
    let messages = &requests[1].messages;
    let tool_index = messages
        .iter()
        .position(|message| message.tool_call_id.as_deref() == Some("call-a"))
        .unwrap();
    assert_eq!(messages[tool_index - 1].tool_calls[0].id, "call-a");
    assert_eq!(messages[tool_index].content, "tool-result");
    assert_eq!(messages[tool_index + 1].role, ChatRole::User);
    assert_eq!(messages[tool_index + 1].content, "follow-up");
}
