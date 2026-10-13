use super::*;

fn desktop_run() -> crate::computer_use::ComputerUseRun {
    crate::computer_use::ComputerUseRun::new(true, false)
}

#[test]
fn issue75_usability_screenshot_has_minimal_target_only_contract() {
    let run = desktop_run();
    let definitions = run.definitions();
    let screenshot = definitions
        .iter()
        .find(|tool| tool.name == "ComputerScreenshot")
        .expect("explicit screenshot delivery tool");
    assert_eq!(
        screenshot.input_schema["required"],
        serde_json::json!(["target_id"])
    );
    assert_eq!(screenshot.input_schema["additionalProperties"], false);
    assert_eq!(
        screenshot.input_schema["properties"]
            .as_object()
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn issue75_usability_builtin_guide_is_automatic_and_capability_scoped() {
    for enabled in [true, false] {
        let dir = tempdir().unwrap();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let provider = MockProvider::new(vec![
            ScriptStep::text("ready"),
            ScriptStep::events(vec![ProviderEvent::Done {
                stop_reason: StopReason::End,
            }]),
        ]);
        let mut req = request(vec![ChatMessage::new(ChatRole::User, "desktop screenshot")]);
        req.tool_config = tool_config(
            RuntimeRunMode::Execute,
            RuntimePermissionMode::FullAccess,
            dir.path().to_owned(),
        )
        .with_computer_use(enabled.then(desktop_run));
        run_agent(&provider, &tools, req, CancellationToken::new())
            .await
            .unwrap();
        let requests = provider.requests();
        let system = requests[0]
            .messages
            .iter()
            .filter(|message| message.role == ChatRole::System)
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(system.contains("vega-cua"), enabled);
        assert_eq!(system.contains("ComputerScreenshot"), enabled);
    }
}

#[tokio::test]
async fn issue75_usability_repeated_invalid_calls_stop_before_next_provider_round() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = desktop_run();
    run.install_desktop_fixture(Vec::new(), Vec::new()).await;
    let mut rounds = (0..3).map(|index| vec![ScriptStep::events(vec![ProviderEvent::ToolUse { id: format!("invalid-{index}"), name: "ComputerObserve".into(), input_json: serde_json::json!({"window_id": 44 + index, "text":format!("unused-{index}")}).to_string() }, ProviderEvent::Done { stop_reason: StopReason::ToolUse }])]).collect::<Vec<_>>();
    rounds.push(vec![
        ScriptStep::text("must not request this round"),
        ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }]),
    ]);
    let provider = MockProvider::new_rounds(rounds);
    let mut req = request(vec![ChatMessage::new(ChatRole::User, "desktop screenshot")]);
    req.tool_config = tool_config(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::FullAccess,
        dir.path().to_owned(),
    )
    .with_computer_use(Some(run));
    let outcome = run_agent(&provider, &tools, req, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(provider.requests().len(), 3);
    assert!(outcome.events.iter().any(|event| matches!(event, RuntimeEvent::ToolCallFinished(result) if result.output.contains("no_progress"))));
    assert!(!outcome.final_text.contains("must not request"));
    let requests = provider.requests();
    let feedback = requests[1]
        .messages
        .iter()
        .find(|message| message.role == ChatRole::Tool)
        .unwrap();
    assert!(
        feedback.content.contains("\"code\":\"invalid_arguments\""),
        "{}",
        feedback.content
    );
    assert!(feedback.content.contains("\"field\":\"target_id\""));
    assert!(feedback.content.contains("\"retryable\":true"));
    assert!(!feedback.content.contains("44"));
}

fn fixture_window() -> crate::computer_use::DesktopTarget {
    crate::computer_use::DesktopTarget {
        selection_id: "fixture".into(),
        application: "Fixture".into(),
        bundle_id: "test.fixture".into(),
        pid: 1,
        window_id: 2,
        title: "Owned test".into(),
    }
}

fn frame_result() -> vega_mcp::ManagedResult {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(4, 3)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    vega_mcp::ManagedResult {
        text: Vec::new(),
        images: vec![vega_mcp::ManagedImage {
            bytes: bytes.into_inner(),
            mime_type: "image/png".into(),
        }],
        structured: Some(
            serde_json::json!({"pid":1,"window_id":2,"screenshot_frame_valid":true,"snapshot_id":"snapshot","capture_id":"capture","elements":[],"degraded":false}),
        ),
        is_error: false,
    }
}

fn call_round(id: &str, name: &str, args: serde_json::Value) -> Vec<ScriptStep> {
    vec![ScriptStep::events(vec![
        ProviderEvent::ToolUse {
            id: id.into(),
            name: name.into(),
            input_json: args.to_string(),
        },
        ProviderEvent::Done {
            stop_reason: StopReason::ToolUse,
        },
    ])]
}

fn screenshot_rounds() -> Vec<Vec<ScriptStep>> {
    vec![
        call_round(
            "directory",
            "ComputerObserve",
            serde_json::json!({"target_id":null,"cursor":null}),
        ),
        call_round(
            "capture",
            "ComputerScreenshot",
            serde_json::json!({"target_id":"fixture"}),
        ),
        vec![
            ScriptStep::text("Screenshot attached."),
            ScriptStep::events(vec![ProviderEvent::Done {
                stop_reason: StopReason::End,
            }]),
        ],
    ]
}

#[tokio::test]
async fn issue75_usability_screenshot_delivers_in_two_calls_and_three_rounds() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = desktop_run();
    let actions = run
        .install_desktop_fixture(vec![fixture_window()], vec![frame_result()])
        .await;
    let provider = MockProvider::new_rounds(screenshot_rounds());
    let mut req = request(vec![ChatMessage::new(ChatRole::User, "Screenshot Fixture")]);
    req.tool_config = tool_config(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::FullAccess,
        dir.path().to_owned(),
    )
    .with_computer_use(Some(run.clone()));
    let outcome = run_agent(&provider, &tools, req, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(provider.requests().len(), 3);
    assert_eq!(outcome.tool_call_count, 2);
    let calls = run.fixture_calls().await;
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "get_window_state");
    assert_eq!(calls[0].1["include_accessibility_tree"], false);
    assert_eq!(calls[0].1["include_screenshot"], true);
    assert_eq!(calls[0].1["pid"], 1);
    assert_eq!(calls[0].1["window_id"], 2);
    assert_eq!(actions.load(Ordering::SeqCst), 0);
    assert_eq!(
        outcome
            .events
            .iter()
            .filter(|event| matches!(event, RuntimeEvent::ScreenshotDelivered { .. }))
            .count(),
        1
    );
    let delivered = outcome
        .events
        .iter()
        .position(|event| matches!(event, RuntimeEvent::ScreenshotDelivered { .. }))
        .unwrap();
    let success = outcome.events.iter().position(|event| matches!(event, RuntimeEvent::ToolCallFinished(result) if result.call_id == "capture")).unwrap();
    assert!(delivered < success);
    assert!(
        provider.requests()[2]
            .messages
            .iter()
            .any(|message| message.role == ChatRole::Tool
                && message.content.contains("conversation_image_attachment"))
    );
    assert!(outcome.events.iter().any(|event| matches!(event, RuntimeEvent::BuiltinSkillActivated(activation) if activation.name == "vega-cua" && activation.content_sha256.len() == 64)));
}

#[tokio::test]
async fn issue75_usability_delivery_ack_failure_stops_before_success_or_next_provider() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = desktop_run();
    run.install_desktop_fixture(vec![fixture_window()], vec![frame_result()])
        .await;
    let provider = MockProvider::new_rounds(screenshot_rounds());
    let mut req = request(vec![ChatMessage::new(ChatRole::User, "Screenshot Fixture")]);
    req.tool_config = tool_config(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::FullAccess,
        dir.path().to_owned(),
    )
    .with_computer_use(Some(run));
    let events = Arc::new(Mutex::new(Vec::new()));
    let captured = events.clone();
    let result = run_agent_with_sink(
        &provider,
        &tools,
        req,
        CancellationToken::new(),
        move |event| {
            let failed = matches!(event, RuntimeEvent::ScreenshotDelivered { .. });
            captured.lock().unwrap().push(event);
            async move {
                if failed {
                    Err(VegaError::Cancelled)
                } else {
                    Ok(())
                }
            }
        },
    )
    .await;
    assert!(result.is_err());
    assert_eq!(provider.requests().len(), 2);
    assert!(!events.lock().unwrap().iter().any(|event| matches!(event, RuntimeEvent::ToolCallFinished(result) if result.call_id == "capture" && result.status == RuntimeToolStatus::Success)));
}

#[tokio::test]
async fn issue75_usability_full_run_directory_success_does_not_reset_ax_failure_count() {
    for stop_at_limit in [false, true] {
        let dir = tempdir().unwrap();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let run = desktop_run();
        let failures = (0..3).map(|_| {
            let mut result = frame_result();
            result.structured.as_mut().unwrap()["degraded"] = serde_json::json!(true);
            result.structured.as_mut().unwrap()["degraded_reason"] = serde_json::json!("ax_window_unresolved: window_id 2 exists and is owned by pid 1, but none of the 2 AXWindow element(s) under that pid reports this CGWindowID. The tree is returned EMPTY on purpose: the accessibility elements reachable under this pid belong to other surfaces (the menu bar, other windows), not to the requested window, so presenting them would misground the next action.");
            result
        }).collect();
        let actions = run
            .install_desktop_fixture(vec![fixture_window()], failures)
            .await;
        let mut rounds = Vec::new();
        for index in 0..3 {
            rounds.push(call_round(
                &format!("directory-{index}"),
                "ComputerObserve",
                serde_json::json!({"target_id":null,"cursor":null}),
            ));
            rounds.push(call_round(
                &format!("observe-{index}"),
                "ComputerObserve",
                serde_json::json!({"target_id":"fixture","cursor":null}),
            ));
        }
        rounds.push(call_round(
            "forbidden-fallback",
            "Bash",
            serde_json::json!({"command":"false"}),
        ));
        let provider = MockProvider::new_rounds(rounds);
        let mut req = request(vec![ChatMessage::new(ChatRole::User, "Screenshot Fixture")]);
        req.tool_config = tool_config(
            RuntimeRunMode::Execute,
            RuntimePermissionMode::FullAccess,
            dir.path().to_owned(),
        )
        .with_computer_use(Some(run));
        let cancel = CancellationToken::new();
        let stop = cancel.clone();
        let outcome = run_agent_with_sink(&provider, &tools, req, cancel, move |event| {
            if stop_at_limit && matches!(event, RuntimeEvent::ToolCallFinished(ref result) if result.output.contains("no_progress")) { stop.cancel(); }
            async { Ok(()) }
        }).await.unwrap();
        assert_eq!(provider.requests().len(), 6);
        assert_eq!(actions.load(Ordering::SeqCst), 0);
        assert_eq!(
            outcome
                .events
                .iter()
                .any(|event| matches!(event, RuntimeEvent::Interrupted)),
            stop_at_limit
        );
        assert!(!outcome.events.iter().any(
            |event| matches!(event, RuntimeEvent::ToolCallProposed(call) if call.name == "Bash")
        ));
        if !stop_at_limit {
            assert!(outcome.final_text.contains("未取得进展"));
        }
    }
}

struct RepeatedClickProvider {
    run: crate::computer_use::ComputerUseRun,
    rounds: AtomicUsize,
}

impl crate::Provider for RepeatedClickProvider {
    fn chat_stream(
        &self,
        req: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<crate::EventStream, VegaError>> {
        let index = self.rounds.fetch_add(1, Ordering::SeqCst);
        let (name, args) = match index {
            0 => (
                "ComputerObserve",
                serde_json::json!({"target_id":null,"cursor":null}),
            ),
            1 => (
                "ComputerObserve",
                serde_json::json!({"target_id":"fixture","cursor":null}),
            ),
            _ => {
                let snapshot = self.run.control.snapshot();
                let id = &snapshot.observations.last().unwrap().id;
                (
                    "ComputerAct",
                    serde_json::json!({"observation_id":id,"action":"click","target":null,"text":format!("unused-{index}"),"key":null,"url":null,"x":1,"y":1,"direction":null,"amount":null}),
                )
            }
        };
        MockProvider::new(call_round(&format!("call-{index}"), name, args)).chat_stream(req, cancel)
    }
}

#[tokio::test]
async fn issue75_usability_full_run_unchanged_click_ignores_unused_arguments_and_stops_after_third_dispatch()
 {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = desktop_run();
    let mut results = vec![frame_result()];
    for _ in 0..3 {
        results.push(vega_mcp::ManagedResult {
            text: Vec::new(),
            images: Vec::new(),
            structured: None,
            is_error: false,
        });
        results.push(frame_result());
    }
    let actions = run
        .install_desktop_fixture(vec![fixture_window()], results)
        .await;
    let provider = RepeatedClickProvider {
        run: run.clone(),
        rounds: AtomicUsize::new(0),
    };
    let mut req = request(vec![ChatMessage::new(
        ChatRole::User,
        "Click fixture until it changes",
    )]);
    req.tool_config = tool_config(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::FullAccess,
        dir.path().to_owned(),
    )
    .with_computer_use(Some(run));
    let outcome = run_agent(&provider, &tools, req, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(provider.rounds.load(Ordering::SeqCst), 5);
    assert_eq!(actions.load(Ordering::SeqCst), 3);
    let final_result = outcome
        .events
        .iter()
        .find_map(|event| match event {
            RuntimeEvent::ToolCallFinished(result) if result.output.contains("no_progress") => {
                Some(result)
            }
            _ => None,
        })
        .unwrap();
    assert!(!final_result.output.contains("not_dispatched"));
    assert!(!final_result.output.contains("unused-"));
    assert!(outcome.final_text.contains("未取得进展"));
    assert!(
        !outcome
            .events
            .iter()
            .any(|event| matches!(event, RuntimeEvent::ScreenshotDelivered { .. }))
    );
}

#[tokio::test]
async fn issue75_usability_two_windows_deliver_three_calls_without_ambiguous_selection() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = desktop_run();
    let mut second = fixture_window();
    second.selection_id = "fixture-two".into();
    second.window_id = 3;
    let mut second_frame = frame_result();
    second_frame.structured.as_mut().unwrap()["window_id"] = serde_json::json!(3);
    let actions = run
        .install_desktop_fixture(
            vec![fixture_window(), second],
            vec![frame_result(), second_frame],
        )
        .await;
    let mut rounds = screenshot_rounds();
    rounds[1] = vec![ScriptStep::events(vec![
        ProviderEvent::ToolUse {
            id: "capture-one".into(),
            name: "ComputerScreenshot".into(),
            input_json: serde_json::json!({"target_id":"fixture"}).to_string(),
        },
        ProviderEvent::ToolUse {
            id: "capture-two".into(),
            name: "ComputerScreenshot".into(),
            input_json: serde_json::json!({"target_id":"fixture-two"}).to_string(),
        },
        ProviderEvent::Done {
            stop_reason: StopReason::ToolUse,
        },
    ])];
    let provider = MockProvider::new_rounds(rounds);
    let mut req = request(vec![ChatMessage::new(
        ChatRole::User,
        "Screenshot both fixture windows",
    )]);
    req.tool_config = tool_config(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::FullAccess,
        dir.path().to_owned(),
    )
    .with_computer_use(Some(run));
    let outcome = run_agent(&provider, &tools, req, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(outcome.tool_call_count, 3);
    assert_eq!(provider.requests().len(), 3);
    assert_eq!(actions.load(Ordering::SeqCst), 0);
    assert_eq!(
        outcome
            .events
            .iter()
            .filter(|event| matches!(event, RuntimeEvent::ScreenshotDelivered { .. }))
            .count(),
        2
    );
}

#[tokio::test]
async fn issue75_usability_cursor_error_names_only_the_bad_schema_field() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = desktop_run();
    run.install_desktop_fixture(Vec::new(), Vec::new()).await;
    let provider = MockProvider::new_rounds(vec![
        call_round(
            "invalid-cursor",
            "ComputerObserve",
            serde_json::json!({"target_id":null,"cursor":{"secret":"PRIVATE_VALUE"}}),
        ),
        vec![
            ScriptStep::text("correct cursor"),
            ScriptStep::events(vec![ProviderEvent::Done {
                stop_reason: StopReason::End,
            }]),
        ],
    ]);
    let mut req = request(vec![ChatMessage::new(ChatRole::User, "inspect")]);
    req.tool_config = tool_config(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::FullAccess,
        dir.path().to_owned(),
    )
    .with_computer_use(Some(run));
    run_agent(&provider, &tools, req, CancellationToken::new())
        .await
        .unwrap();
    let requests = provider.requests();
    let result = requests[1]
        .messages
        .iter()
        .find(|message| message.role == ChatRole::Tool)
        .unwrap();
    assert!(result.content.contains("\"field\":\"cursor\""));
    assert!(!result.content.contains("PRIVATE_VALUE"));
}

#[tokio::test]
async fn issue75_readonly_permissions_missing_grant_is_cua_rejected_not_run_mode() {
    for (mode, permission, expected_source, expected_code) in [
        (
            RuntimeRunMode::Execute,
            RuntimePermissionMode::FullAccess,
            RuntimeApprovalSource::ComputerUse,
            "screen_recording_permission_missing",
        ),
        (
            RuntimeRunMode::Plan,
            RuntimePermissionMode::FullAccess,
            RuntimeApprovalSource::RunMode,
            "denied by run mode",
        ),
        (
            RuntimeRunMode::Execute,
            RuntimePermissionMode::ReadOnly,
            RuntimeApprovalSource::RunMode,
            "denied by run mode",
        ),
    ] {
        let dir = tempdir().unwrap();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let run = desktop_run();
        run.install_desktop_fixture(vec![fixture_window()], vec![frame_result()])
            .await;
        run.set_desktop_permissions_fixture(vec![
            serde_json::json!({"accessibility":false,"screen_recording":false}),
        ])
        .await;
        let provider = MockProvider::new_rounds(screenshot_rounds());
        let mut req = request(vec![ChatMessage::new(ChatRole::User, "Screenshot Fixture")]);
        req.tool_config = tool_config(mode, permission, dir.path().to_owned())
            .with_computer_use(Some(run.clone()));
        let outcome = run_agent(&provider, &tools, req, CancellationToken::new())
            .await
            .unwrap();
        let result = outcome
            .events
            .iter()
            .find_map(|event| match event {
                RuntimeEvent::ToolCallFinished(result) => Some(result),
                _ => None,
            })
            .unwrap();
        assert_eq!(result.status, RuntimeToolStatus::Rejected);
        assert_eq!(result.approval.as_ref().unwrap().source, expected_source);
        assert_eq!(
            result.approval.as_ref().unwrap().decision,
            RuntimeApprovalDecision::Deny
        );
        assert!(result.output.contains(expected_code), "{}", result.output);
        assert!(
            !outcome
                .events
                .iter()
                .any(|event| matches!(event, RuntimeEvent::ScreenshotDelivered { .. }))
        );
        assert!(run.fixture_calls().await.is_empty());
    }
}

#[tokio::test]
async fn issue75_readonly_permissions_cancel_at_approval_wins_before_capture() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = desktop_run();
    run.install_desktop_fixture(vec![fixture_window()], vec![frame_result()])
        .await;
    run.set_desktop_permissions_fixture(vec![
        serde_json::json!({"accessibility":false,"screen_recording":true}),
    ])
    .await;
    let provider = MockProvider::new_rounds(screenshot_rounds());
    let mut req = request(vec![ChatMessage::new(ChatRole::User, "Screenshot Fixture")]);
    req.tool_config = tool_config(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::FullAccess,
        dir.path().to_owned(),
    )
    .with_computer_use(Some(run.clone()));
    let cancel = CancellationToken::new();
    let owned = cancel.clone();
    let outcome = run_agent_with_sink(&provider, &tools, req, cancel, move |event| {
        if matches!(&event,RuntimeEvent::ToolCallApproved{call_id,..} if call_id=="capture") {
            owned.cancel();
        }
        async { Ok(()) }
    })
    .await
    .unwrap();
    assert!(
        outcome
            .events
            .iter()
            .any(|event| matches!(event, RuntimeEvent::Interrupted))
    );
    assert_eq!(provider.requests().len(), 2);
    assert!(run.fixture_calls().await.is_empty());
    assert!(
        !outcome
            .events
            .iter()
            .any(|event| matches!(event, RuntimeEvent::ScreenshotDelivered { .. }))
    );
}
