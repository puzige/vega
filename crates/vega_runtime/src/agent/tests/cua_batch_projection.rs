mod text_delivery;

use super::*;
use crate::computer_use::{ComputerUseRun, ControlPhase, DesktopTarget};
use serde_json::{Value, json};

fn window(id: u32) -> DesktopTarget {
    DesktopTarget {
        selection_id: format!("fixture-{id}"),
        application: "Fixture".into(),
        bundle_id: "test.fixture".into(),
        pid: 1,
        window_id: id,
        title: format!("Owned {id}"),
    }
}

fn frame(id: u32) -> vega_mcp::ManagedResult {
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
        structured: Some(json!({
            "pid":1,"window_id":id,"screenshot_frame_valid":true,
            "snapshot_id":format!("snapshot-{id}"),"capture_id":format!("capture-{id}"),
            "degraded":false,"elements":[{"element_token":format!("edit-{id}"),"role":"AXTextArea","label":"Editor","value":format!("BODY-{id}")}],
            "tree_markdown":format!("BODY-{id}")
        })),
        is_error: false,
    }
}

fn batch(calls: &[(&str, &str, Value)]) -> Vec<ScriptStep> {
    let mut events = calls
        .iter()
        .map(|(id, name, args)| ProviderEvent::ToolUse {
            id: (*id).into(),
            name: (*name).into(),
            input_json: args.to_string(),
        })
        .collect::<Vec<_>>();
    events.push(ProviderEvent::Done {
        stop_reason: StopReason::ToolUse,
    });
    vec![ScriptStep::events(events)]
}

fn directory() -> Vec<ScriptStep> {
    batch(&[(
        "directory",
        "ComputerObserve",
        json!({"target_id":null,"cursor":null}),
    )])
}

fn finish() -> Vec<ScriptStep> {
    vec![
        ScriptStep::text("Read both windows."),
        ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }]),
    ]
}

fn configured(run: ComputerUseRun, path: &std::path::Path) -> AgentRequest {
    let mut req = request(vec![ChatMessage::new(
        ChatRole::User,
        "Read both owned windows and deliver the requested screenshots.",
    )]);
    req.tool_config = tool_config(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::FullAccess,
        path.to_owned(),
    )
    .with_computer_use(Some(run));
    req
}

fn result<'a>(request: &'a ChatRequest, id: &str) -> &'a ChatMessage {
    request
        .messages
        .iter()
        .find(|message| message.tool_call_id.as_deref() == Some(id))
        .unwrap()
}

#[tokio::test]
async fn issue75_batch_projection_two_observations_reach_exactly_one_provider_request() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("note.txt"), "ordinary work").unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = ComputerUseRun::new(true, false);
    run.install_desktop_fixture(vec![window(2), window(3)], vec![frame(2), frame(3)])
        .await;
    let provider = MockProvider::new_rounds(vec![
        directory(),
        batch(&[
            (
                "a",
                "ComputerObserve",
                json!({"target_id":"fixture-2","cursor":null}),
            ),
            (
                "b",
                "ComputerObserve",
                json!({"target_id":"fixture-3","cursor":null}),
            ),
        ]),
        batch(&[("read", "read", json!({"path":"note.txt"}))]),
        finish(),
    ]);
    let outcome = run_agent(
        &provider,
        &tools,
        configured(run, dir.path()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let requests = provider.requests();
    let a = result(&requests[2], "a");
    let b = result(&requests[2], "b");
    assert!(
        a.content.contains("BODY-2"),
        "earlier successful read was lost: {}",
        a.content
    );
    assert!(a.content.contains("read-only"));
    assert!(a.content.contains("Owned 2"));
    assert!(!a.content.contains("observation_id:"));
    assert!(!a.content.contains("element_token"));
    assert!(!a.content.contains("edit-2"));
    assert!(a.content.contains("\"target_id\":\"fixture-2\""));
    assert_eq!(a.images.len(), 1);
    assert!(b.content.contains("BODY-3") && b.content.contains("observation_id:"));
    assert_eq!(b.images.len(), 1);
    assert_eq!(
        result(&requests[3], "a").content,
        crate::computer_use::INVALIDATED_OBSERVATION
    );
    assert!(result(&requests[3], "a").images.is_empty());
    for request in requests {
        assert_provider_contract(&request);
    }
    assert!(
        !outcome
            .messages
            .iter()
            .any(|message| message.content.contains("BODY-"))
    );
}

#[tokio::test]
async fn issue75_batch_projection_observe_then_screenshot_keeps_text_and_delivery() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = ComputerUseRun::new(true, false);
    run.install_desktop_fixture(vec![window(2), window(3)], vec![frame(2), frame(3)])
        .await;
    let provider = MockProvider::new_rounds(vec![
        directory(),
        batch(&[
            (
                "a",
                "ComputerObserve",
                json!({"target_id":"fixture-2","cursor":null}),
            ),
            ("b", "ComputerScreenshot", json!({"target_id":"fixture-3"})),
        ]),
        finish(),
    ]);
    let outcome = run_agent(
        &provider,
        &tools,
        configured(run, dir.path()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let requests = provider.requests();
    assert!(result(&requests[2], "a").content.contains("BODY-2"));
    assert_eq!(result(&requests[2], "a").images.len(), 1);
    assert!(
        result(&requests[2], "b")
            .content
            .contains("conversation_image_attachment")
    );
    assert_eq!(result(&requests[2], "b").images.len(), 1);
    assert_eq!(
        outcome
            .events
            .iter()
            .filter(|event| matches!(event, RuntimeEvent::ScreenshotDelivered { .. }))
            .count(),
        1
    );
    crate::images::validate_messages(&requests[2].messages).unwrap();
}

#[tokio::test]
async fn issue75_batch_projection_third_capture_is_deferred_before_dispatch_and_ack() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = ComputerUseRun::new(true, false);
    run.install_desktop_fixture(
        vec![window(2), window(3), window(4)],
        vec![frame(2), frame(3), frame(4)],
    )
    .await;
    let provider = MockProvider::new_rounds(vec![
        directory(),
        batch(&[
            (
                "a",
                "ComputerObserve",
                json!({"target_id":"fixture-2","cursor":null}),
            ),
            (
                "b",
                "ComputerObserve",
                json!({"target_id":"fixture-3","cursor":null}),
            ),
            ("c", "ComputerScreenshot", json!({"target_id":"fixture-4"})),
        ]),
        finish(),
    ]);
    let outcome = run_agent(
        &provider,
        &tools,
        configured(run.clone(), dir.path()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(
        run.fixture_calls().await.len(),
        2,
        "third read must not dispatch"
    );
    assert_eq!(outcome.executed_tool_call_count, 3);
    assert!(
        !outcome
            .events
            .iter()
            .any(|event| matches!(event, RuntimeEvent::ScreenshotDelivered { .. }))
    );
    let requests = provider.requests();
    let denied = result(&requests[2], "c");
    assert!(denied.content.contains("observation_budget_exceeded"));
    assert!(denied.content.contains("\"executed\":false"));
    assert!(denied.content.contains("\"deferred\":true"));
    assert!(outcome.events.iter().any(|event|matches!(event,RuntimeEvent::ToolCallFinished(result) if result.call_id=="c" && result.status==RuntimeToolStatus::Failed)));
    assert!(result(&requests[2], "a").content.contains("BODY-2"));
    assert!(result(&requests[2], "b").content.contains("BODY-3"));
    crate::images::validate_messages(&requests[2].messages).unwrap();
}

#[tokio::test]
async fn issue75_batch_projection_pause_at_actual_send_clears_stale_payload_but_keeps_ack() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = ComputerUseRun::new(true, false);
    run.install_desktop_fixture(vec![window(2), window(3)], vec![frame(2), frame(3)])
        .await;
    let provider = MockProvider::new_rounds(vec![
        directory(),
        batch(&[
            (
                "a",
                "ComputerObserve",
                json!({"target_id":"fixture-2","cursor":null}),
            ),
            ("b", "ComputerScreenshot", json!({"target_id":"fixture-3"})),
        ]),
        finish(),
    ]);
    let control = run.control.clone();
    let attempts = Arc::new(AtomicUsize::new(0));
    let count = attempts.clone();
    let outcome=run_agent_with_sink(&provider,&tools,configured(run,dir.path()),CancellationToken::new(),move |event| {
        if matches!(event,RuntimeEvent::DiagnosticAttempt(ref value) if value.phase==RuntimeDiagnosticPhase::PrimaryModel && value.state==RuntimeDiagnosticState::Started)
            && count.fetch_add(1,Ordering::SeqCst)==2 {
            control.pause();
            control.finish_cleanup();
            assert_eq!(control.status().phase,ControlPhase::Paused);
            control.resume(control.status().epoch).unwrap();
        }
        async {Ok(())}
    }).await.unwrap();
    assert!(!outcome.interrupted);
    let requests = provider.requests();
    let a = result(&requests[2], "a");
    let b = result(&requests[2], "b");
    assert!(!a.content.contains("BODY-2") && a.images.is_empty());
    assert!(b.content.contains("conversation_image_attachment"));
    assert!(
        b.images.is_empty(),
        "pixels must not cross a pause at the actual send boundary"
    );
    assert!(!b.content.contains("observation_id:"));
}

fn existing_tool_images(images: Vec<crate::ImageAttachment>) -> Vec<ChatMessage> {
    let mut result = ChatMessage::tool_result("existing", "Earlier owned image");
    result.images = images;
    vec![
        ChatMessage::assistant_with_tools(
            "",
            vec![crate::ChatToolCall {
                id: "existing".into(),
                name: "read".into(),
                input_json: json!({"path":"prior.png"}).to_string(),
            }],
        ),
        result,
    ]
}

fn padded_image(bytes: usize) -> crate::ImageAttachment {
    let mut png = frame(2).images.remove(0).bytes;
    png.resize(bytes, 0);
    crate::ImageAttachment::from_bytes(png).unwrap()
}

#[tokio::test]
async fn issue75_batch_projection_byte_budget_marks_execution_and_does_not_ack_failed_capture() {
    for (images, executed) in [
        (vec![padded_image(8 * 1024 * 1024)], false),
        (
            vec![padded_image(4 * 1024 * 1024), padded_image(4 * 1024 * 1024)],
            false,
        ),
        (vec![padded_image(8 * 1024 * 1024 - 20)], true),
    ] {
        let dir = tempdir().unwrap();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let run = ComputerUseRun::new(true, false);
        run.install_desktop_fixture(vec![window(2)], vec![frame(2)])
            .await;
        let provider = MockProvider::new_rounds(vec![
            directory(),
            batch(&[(
                "capture",
                "ComputerScreenshot",
                json!({"target_id":"fixture-2"}),
            )]),
            finish(),
        ]);
        let mut req = configured(run.clone(), dir.path());
        req.history.splice(0..0, existing_tool_images(images));
        let outcome = run_agent(&provider, &tools, req, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(run.fixture_calls().await.len(), usize::from(executed));
        assert_eq!(outcome.executed_tool_call_count, 1 + usize::from(executed));
        assert!(
            !outcome
                .events
                .iter()
                .any(|event| matches!(event, RuntimeEvent::ScreenshotDelivered { .. }))
        );
        let requests = provider.requests();
        let failure = result(&requests[2], "capture");
        let feedback: Value = serde_json::from_str(&failure.content).unwrap();
        assert_eq!(feedback["code"], "observation_budget_exceeded");
        assert_eq!(feedback["executed"], executed);
        assert!(failure.images.is_empty());
        assert!(
            run.screenshot("capture").is_none(),
            "failed pre-ACK capture must not occupy the new-delivery budget"
        );
        for request in &requests {
            crate::images::validate_messages(&request.messages).unwrap();
        }
    }
}

#[tokio::test]
async fn issue75_batch_projection_budget_does_not_rewrite_prior_delivered_ack_as_failure() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = ComputerUseRun::new(true, false);
    let mut same_pixels = frame(2);
    let original_len = same_pixels.images[0].bytes.len();
    same_pixels.images[0].bytes.extend([0; 512]);
    run.install_desktop_fixture(vec![window(2)], vec![frame(2), same_pixels])
        .await;
    let provider = MockProvider::new_rounds(vec![
        directory(),
        batch(&[(
            "first",
            "ComputerScreenshot",
            json!({"target_id":"fixture-2"}),
        )]),
        batch(&[(
            "duplicate",
            "ComputerScreenshot",
            json!({"target_id":"fixture-2"}),
        )]),
        finish(),
    ]);
    let mut req = configured(run, dir.path());
    req.history.splice(
        0..0,
        existing_tool_images(vec![padded_image(8 * 1024 * 1024 - original_len)]),
    );
    let outcome = run_agent(&provider, &tools, req, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(
        outcome
            .events
            .iter()
            .filter(|event| matches!(event, RuntimeEvent::ScreenshotDelivered { .. }))
            .count(),
        1
    );
    let duplicate = outcome
        .events
        .iter()
        .find_map(|event| match event {
            RuntimeEvent::ToolCallFinished(result) if result.call_id == "duplicate" => Some(result),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        duplicate.status,
        RuntimeToolStatus::Success,
        "the original image remains delivered"
    );
    let requests = provider.requests();
    let projected = result(&requests[3], "duplicate");
    let feedback: Value = serde_json::from_str(&projected.content).unwrap();
    assert_eq!(feedback["status"], "delivered");
    assert_eq!(feedback["unchanged"], true);
    assert_eq!(
        feedback["model_projection"]["code"],
        "observation_budget_exceeded"
    );
    assert_eq!(feedback["model_projection"]["executed"], true);
    assert!(projected.images.is_empty());
    crate::images::validate_messages(&requests[3].messages).unwrap();
}

fn assert_provider_contract(request: &ChatRequest) {
    crate::images::validate_messages(&request.messages).unwrap();
    let expected_calls = request
        .messages
        .iter()
        .flat_map(|message| &message.tool_calls)
        .map(|call| call.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    let results = request
        .messages
        .iter()
        .filter_map(|message| message.tool_call_id.as_deref())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(expected_calls, results);
    let cc = crate::openai::build_request_body(request);
    let responses = crate::openai::responses::build_body(request).unwrap();
    assert_eq!(
        cc["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|message| message["role"] == "tool")
            .count(),
        results.len()
    );
    assert_eq!(
        responses["input"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|item| item["type"] == "function_call_output")
            .count(),
        results.len()
    );
}

struct ActionProvider {
    run: ComputerUseRun,
    old: Arc<Mutex<Option<String>>>,
    requests: Mutex<Vec<ChatRequest>>,
}

impl Provider for ActionProvider {
    fn chat_stream(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<crate::EventStream, VegaError>> {
        let mut requests = self.requests.lock().unwrap();
        let index = requests.len();
        requests.push(request.clone());
        drop(requests);
        let script = match index {
            0 => directory(),
            1 => batch(&[
                (
                    "a",
                    "ComputerObserve",
                    json!({"target_id":"fixture-2","cursor":null}),
                ),
                (
                    "b",
                    "ComputerObserve",
                    json!({"target_id":"fixture-3","cursor":null}),
                ),
            ]),
            2 => {
                let old = self.old.lock().unwrap().clone().unwrap();
                let b = result(&request, "b");
                let current = b
                    .content
                    .lines()
                    .find_map(|line| line.strip_prefix("observation_id: "))
                    .unwrap();
                assert!(
                    self.run
                        .control
                        .observation(crate::computer_use::Capability::Desktop, &old)
                        .is_err()
                );
                assert!(
                    self.run
                        .control
                        .observation(crate::computer_use::Capability::Desktop, current)
                        .is_ok()
                );
                batch(&[
                    (
                        "old-action",
                        "ComputerAct",
                        json!({"observation_id":old,"action":"type_text","target":"edit-2","text":"NEVER"}),
                    ),
                    (
                        "current-action",
                        "ComputerAct",
                        json!({"observation_id":current,"action":"type_text","target":"edit-3","text":"CHANGED"}),
                    ),
                ])
            }
            _ => finish(),
        };
        MockProvider::new(script).chat_stream(request, cancel)
    }
}

#[tokio::test]
async fn issue75_batch_projection_old_action_stays_invalid_and_current_action_consumes_reads() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = ComputerUseRun::new(true, false);
    let mut changed = frame(3);
    changed.structured.as_mut().unwrap()["tree_markdown"] = json!("BODY-3 CHANGED");
    let actions = run
        .install_desktop_fixture(
            vec![window(2), window(3)],
            vec![
                frame(2),
                frame(3),
                vega_mcp::ManagedResult {
                    text: Vec::new(),
                    images: Vec::new(),
                    structured: None,
                    is_error: false,
                },
                changed,
            ],
        )
        .await;
    let old = Arc::new(Mutex::new(None));
    let recorded = old.clone();
    let control = run.control.clone();
    let provider = ActionProvider {
        run: run.clone(),
        old,
        requests: Mutex::new(Vec::new()),
    };
    let outcome = run_agent_with_sink(
        &provider,
        &tools,
        configured(run, dir.path()),
        CancellationToken::new(),
        move |event| {
            if matches!(event,RuntimeEvent::ToolCallFinished(ref result) if result.call_id=="a") {
                *recorded.lock().unwrap() = control
                    .snapshot()
                    .observations
                    .iter()
                    .find(|item| item.call_id == "a")
                    .map(|item| item.id.clone());
            }
            async { Ok(()) }
        },
    )
    .await
    .unwrap();
    assert_eq!(actions.load(Ordering::SeqCst), 1);
    let requests = provider.requests.lock().unwrap();
    assert!(result(&requests[2], "a").content.contains("BODY-2"));
    assert!(
        result(&requests[3], "old-action")
            .content
            .contains("stale_observation")
    );
    assert!(
        result(&requests[3], "current-action")
            .content
            .contains("BODY-3 CHANGED")
    );
    assert!(!result(&requests[3], "a").content.contains("BODY-2"));
    assert!(result(&requests[3], "a").images.is_empty());
    for request in requests.iter() {
        assert_provider_contract(request);
    }
    assert!(!outcome.interrupted);
    for message in outcome.messages {
        assert!(!message.content.contains("BODY-"));
        for call in message.tool_calls {
            assert!(crate::computer_use::ComputerUseAudit::valid(
                &call.input_json
            ));
        }
    }
}

#[tokio::test]
async fn issue75_batch_projection_pause_within_batch_keeps_ack_and_discards_old_read_scope() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = ComputerUseRun::new(true, false);
    run.install_desktop_fixture(vec![window(2), window(3)], vec![frame(2), frame(3)])
        .await;
    let provider = MockProvider::new_rounds(vec![
        directory(),
        batch(&[
            ("a", "ComputerScreenshot", json!({"target_id":"fixture-2"})),
            (
                "b",
                "ComputerObserve",
                json!({"target_id":"fixture-3","cursor":null}),
            ),
        ]),
        finish(),
    ]);
    let control = run.control.clone();
    let outcome = run_agent_with_sink(
        &provider,
        &tools,
        configured(run.clone(), dir.path()),
        CancellationToken::new(),
        move |event| {
            if matches!(event,RuntimeEvent::ToolCallFinished(ref result) if result.call_id=="a") {
                control.pause();
                control.finish_cleanup();
                control.resume(control.status().epoch).unwrap();
            }
            async { Ok(()) }
        },
    )
    .await
    .unwrap();
    assert!(!outcome.interrupted);
    assert_eq!(run.fixture_calls().await.len(), 1);
    let requests = provider.requests();
    assert!(
        result(&requests[2], "a")
            .content
            .contains("conversation_image_attachment")
    );
    assert!(result(&requests[2], "a").images.is_empty());
    assert!(
        !result(&requests[2], "a")
            .content
            .contains("observation_id:")
    );
    assert!(
        result(&requests[2], "b")
            .content
            .contains("control_unavailable"),
        "{}",
        result(&requests[2], "b").content
    );
    assert_provider_contract(&requests[2]);
}

#[tokio::test]
async fn issue75_batch_projection_cancel_within_batch_stops_and_redacts_unconsumed_content() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = ComputerUseRun::new(true, false);
    run.install_desktop_fixture(vec![window(2), window(3)], vec![frame(2), frame(3)])
        .await;
    let provider = MockProvider::new_rounds(vec![
        directory(),
        batch(&[
            (
                "a",
                "ComputerObserve",
                json!({"target_id":"fixture-2","cursor":null}),
            ),
            (
                "b",
                "ComputerObserve",
                json!({"target_id":"fixture-3","cursor":null}),
            ),
        ]),
        finish(),
    ]);
    let cancel = CancellationToken::new();
    let stop = cancel.clone();
    let outcome = run_agent_with_sink(
        &provider,
        &tools,
        configured(run.clone(), dir.path()),
        cancel,
        move |event| {
            if matches!(event,RuntimeEvent::ToolCallFinished(ref result) if result.call_id=="a") {
                stop.cancel();
            }
            async { Ok(()) }
        },
    )
    .await
    .unwrap();
    assert!(outcome.interrupted);
    assert_eq!(provider.requests().len(), 2);
    assert_eq!(run.fixture_calls().await.len(), 1);
    assert_eq!(run.control.status().phase, ControlPhase::Stopped);
    for message in outcome.messages {
        assert!(message.images.is_empty());
        assert!(!message.content.contains("BODY-"));
    }
}

#[tokio::test]
async fn issue75_batch_projection_deferred_window_can_be_read_in_next_batch() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = ComputerUseRun::new(true, false);
    run.install_desktop_fixture(
        vec![window(2), window(3), window(4)],
        vec![frame(2), frame(3), frame(4)],
    )
    .await;
    let provider = MockProvider::new_rounds(vec![
        directory(),
        batch(&[
            (
                "a",
                "ComputerObserve",
                json!({"target_id":"fixture-2","cursor":null}),
            ),
            (
                "b",
                "ComputerObserve",
                json!({"target_id":"fixture-3","cursor":null}),
            ),
            (
                "deferred",
                "ComputerScreenshot",
                json!({"target_id":"fixture-4"}),
            ),
        ]),
        batch(&[(
            "remaining",
            "ComputerScreenshot",
            json!({"target_id":"fixture-4"}),
        )]),
        finish(),
    ]);
    let recorded = Arc::new(Mutex::new(Vec::<String>::new()));
    let stale_checked = Arc::new(AtomicUsize::new(0));
    let checked = stale_checked.clone();
    let control = run.control.clone();
    let outcome = run_agent_with_sink(
        &provider,
        &tools,
        configured(run.clone(), dir.path()),
        CancellationToken::new(),
        move |event| {
            if let RuntimeEvent::ToolCallFinished(ref finished) = event {
                if matches!(finished.call_id.as_str(), "a" | "b") {
                    let id = control
                        .snapshot()
                        .observations
                        .iter()
                        .find(|item| item.call_id == finished.call_id)
                        .unwrap()
                        .id
                        .clone();
                    recorded.lock().unwrap().push(id);
                } else if finished.call_id == "remaining" {
                    assert_eq!(control.status().phase, ControlPhase::Running);
                    let old = recorded.lock().unwrap();
                    assert_eq!(old.len(), 2);
                    for id in old.iter() {
                        assert_eq!(
                            control
                                .observation(crate::computer_use::Capability::Desktop, id)
                                .err(),
                            Some(crate::computer_use::ComputerUseError::StaleObservation)
                        );
                    }
                    checked.fetch_add(1, Ordering::SeqCst);
                }
            }
            async { Ok(()) }
        },
    )
    .await
    .unwrap();
    assert_eq!(run.fixture_calls().await.len(), 3);
    assert_eq!(outcome.executed_tool_call_count, 4);
    let requests = provider.requests();
    assert!(
        result(&requests[2], "deferred")
            .content
            .contains("\"executed\":false")
    );
    assert!(result(&requests[2], "a").content.contains("BODY-2"));
    assert!(
        result(&requests[3], "remaining")
            .content
            .contains("conversation_image_attachment")
    );
    assert_eq!(stale_checked.load(Ordering::SeqCst), 1);
    for (call, id, other) in [("a", 2, 3), ("b", 3, 2)] {
        let prior = result(&requests[3], call);
        assert!(prior.content.contains(&format!("BODY-{id}")));
        assert!(prior.content.contains(&format!("Owned {id}")));
        assert!(!prior.content.contains(&format!("Owned {other}")));
        assert!(prior.content.contains("Prior read-only evidence"));
        assert!(prior.content.contains("observed_at_unix_ms"));
        assert!(prior.images.is_empty());
        for reference in [
            "observation_id:",
            "element_token",
            "edit-",
            "snapshot-",
            "capture-",
        ] {
            assert!(!prior.content.contains(reference));
        }
    }
    assert_eq!(result(&requests[3], "remaining").images.len(), 1);
    assert_eq!(
        requests[3]
            .messages
            .iter()
            .map(|message| message.images.len())
            .sum::<usize>(),
        1
    );
    assert_eq!(
        outcome
            .events
            .iter()
            .filter(|event| matches!(event, RuntimeEvent::ScreenshotDelivered { .. }))
            .count(),
        1
    );
    for request in &requests {
        assert_provider_contract(request);
    }
}

#[tokio::test]
async fn issue75_batch_projection_failed_capture_releases_slot_for_later_valid_delivery() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = ComputerUseRun::new(true, false);
    let original_len = frame(2).images[0].bytes.len();
    let mut frames = (2..6)
        .map(|id| {
            let mut large = frame(id);
            large.images[0].bytes.extend([0; 512]);
            large
        })
        .collect::<Vec<_>>();
    frames.push(frame(6));
    run.install_desktop_fixture((2..7).map(window).collect(), frames)
        .await;
    let mut rounds = vec![directory()];
    for index in 0..5 {
        rounds.push(batch(&[(
            &format!("capture-{index}"),
            "ComputerScreenshot",
            json!({"target_id":format!("fixture-{}",index+2)}),
        )]));
    }
    rounds.push(finish());
    let provider = MockProvider::new_rounds(rounds);
    let mut req = configured(run.clone(), dir.path());
    req.history.splice(
        0..0,
        existing_tool_images(vec![padded_image(8 * 1024 * 1024 - original_len)]),
    );
    let outcome = run_agent(&provider, &tools, req, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(run.fixture_calls().await.len(), 5);
    assert_eq!(
        outcome
            .events
            .iter()
            .filter(|event| matches!(event, RuntimeEvent::ScreenshotDelivered { .. }))
            .count(),
        1
    );
    for index in 0..4 {
        assert!(run.screenshot(&format!("capture-{index}")).is_none());
    }
    assert!(run.screenshot("capture-4").is_some());
    let requests = provider.requests();
    assert!(
        result(requests.last().unwrap(), "capture-4")
            .content
            .contains("conversation_image_attachment")
    );
    for request in &requests {
        assert_provider_contract(request);
    }
}

struct BatchCompaction {
    observed: Arc<Mutex<Vec<ChatMessage>>>,
}

impl ContextCompactionHook for BatchCompaction {
    fn compact<'a>(
        &'a self,
        request: ContextCompactionRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'a, Result<ContextCompactionResult, ContextCompactionFailure>> {
        *self.observed.lock().unwrap() = request.messages;
        async move {
            Ok(ContextCompactionResult {
                messages: vec![ChatMessage::new(
                    ChatRole::User,
                    "Continue the requested text task.",
                )],
                source_version: request.source_version + 1,
                source_fingerprint: request.source_fingerprint,
                usages: Vec::new(),
                usage_complete: false,
            })
        }
        .boxed()
    }
}

#[tokio::test]
async fn issue75_batch_projection_compaction_clears_pending_reads_before_persistent_hook() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = ComputerUseRun::new(true, false);
    let mut a = frame(2);
    let mut b = frame(3);
    for frame in [&mut a, &mut b] {
        frame.structured.as_mut().unwrap()["tree_markdown"] = json!("BATCHBODY ".repeat(8000));
    }
    run.install_desktop_fixture(vec![window(2), window(3)], vec![a, b])
        .await;
    let provider = MockProvider::new_rounds(vec![
        directory(),
        batch(&[
            (
                "a",
                "ComputerObserve",
                json!({"target_id":"fixture-2","cursor":null}),
            ),
            (
                "b",
                "ComputerObserve",
                json!({"target_id":"fixture-3","cursor":null}),
            ),
        ]),
        finish(),
    ]);
    let observed = Arc::new(Mutex::new(Vec::new()));
    let mut req = configured(run, dir.path());
    req.context_compaction_hook = Some(Arc::new(BatchCompaction {
        observed: observed.clone(),
    }));
    req.context_source_version = Some(1);
    req.context_budget = Some(ContextBudget::new(20000, 1000, true).unwrap());
    let outcome = run_agent(&provider, &tools, req, CancellationToken::new())
        .await
        .unwrap();
    let compacted = observed.lock().unwrap();
    assert!(!compacted.is_empty());
    for message in compacted.iter().chain(outcome.messages.iter()) {
        assert!(message.images.is_empty());
        assert!(!message.content.contains("BATCHBODY"));
        assert!(!message.content.contains("Owned 2"));
        for call in &message.tool_calls {
            if ComputerUseRun::tool_capability(&call.name).is_some() {
                assert!(crate::computer_use::ComputerUseAudit::valid(
                    &call.input_json
                ));
            }
        }
    }
    assert_eq!(provider.requests().len(), 3);
    assert!(
        !provider.requests()[2]
            .messages
            .iter()
            .any(|message| message.content.contains("BATCHBODY"))
    );
}

#[tokio::test]
async fn issue75_batch_projection_sink_error_discards_unconsumed_results_and_stops_provider() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = ComputerUseRun::new(true, false);
    run.install_desktop_fixture(vec![window(2), window(3)], vec![frame(2), frame(3)])
        .await;
    let provider = MockProvider::new_rounds(vec![
        directory(),
        batch(&[
            (
                "a",
                "ComputerObserve",
                json!({"target_id":"fixture-2","cursor":null}),
            ),
            (
                "b",
                "ComputerObserve",
                json!({"target_id":"fixture-3","cursor":null}),
            ),
        ]),
        finish(),
    ]);
    let held = Arc::new(Mutex::new(Vec::new()));
    let saved = held.clone();
    let observed = run.clone();
    let outcome = run_agent_with_sink(
        &provider,
        &tools,
        configured(run.clone(), dir.path()),
        CancellationToken::new(),
        move |event| {
            let failed =
                matches!(event,RuntimeEvent::ToolCallFinished(ref result) if result.call_id=="b");
            if let RuntimeEvent::ToolCallFinished(ref finished) = event {
                let mut message = ChatMessage::tool_result(&finished.call_id, "");
                observed.apply_observation(&finished.call_id, &mut message);
                saved.lock().unwrap().push(message);
            }
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
    assert!(outcome.is_err());
    assert_eq!(provider.requests().len(), 2);
    let mut held = held.lock().unwrap().clone();
    assert!(
        held.iter()
            .any(|message| message.content.contains("BODY-2"))
    );
    run.trim_messages(&mut held);
    assert!(
        held.iter()
            .all(|message| message.images.is_empty() && !message.content.contains("BODY-"))
    );
    assert_eq!(run.control.status().phase, ControlPhase::Stopped);
}

#[tokio::test]
async fn issue75_batch_projection_cancel_at_actual_send_does_not_call_provider() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = ComputerUseRun::new(true, false);
    run.install_desktop_fixture(vec![window(2), window(3)], vec![frame(2), frame(3)])
        .await;
    let provider = MockProvider::new_rounds(vec![
        directory(),
        batch(&[
            (
                "a",
                "ComputerObserve",
                json!({"target_id":"fixture-2","cursor":null}),
            ),
            ("b", "ComputerScreenshot", json!({"target_id":"fixture-3"})),
        ]),
        finish(),
    ]);
    let cancel = CancellationToken::new();
    let stopped = cancel.clone();
    let mut attempts = 0;
    let outcome=run_agent_with_sink(&provider,&tools,configured(run,dir.path()),cancel,move |event|{
        if matches!(event,RuntimeEvent::DiagnosticAttempt(ref value) if value.phase==RuntimeDiagnosticPhase::PrimaryModel && value.state==RuntimeDiagnosticState::Started) {
            attempts+=1;if attempts==3{stopped.cancel();}
        }
        async{Ok(())}
    }).await.unwrap();
    assert!(outcome.interrupted);
    assert_eq!(provider.requests().len(), 2);
    assert_eq!(
        outcome
            .events
            .iter()
            .filter(|event| matches!(event, RuntimeEvent::ScreenshotDelivered { .. }))
            .count(),
        1
    );
    for message in outcome.messages {
        assert!(message.images.is_empty());
        assert!(!message.content.contains("BODY-"));
    }
}

#[tokio::test]
async fn issue75_batch_projection_directory_without_image_does_not_consume_image_slots() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = ComputerUseRun::new(true, false);
    run.install_desktop_fixture(vec![window(2), window(3)], vec![frame(2), frame(3)])
        .await;
    let provider = MockProvider::new_rounds(vec![
        directory(),
        batch(&[
            (
                "a",
                "ComputerObserve",
                json!({"target_id":"fixture-2","cursor":null}),
            ),
            (
                "b",
                "ComputerObserve",
                json!({"target_id":"fixture-3","cursor":null}),
            ),
            (
                "directory-again",
                "ComputerObserve",
                json!({"target_id":null,"cursor":null}),
            ),
        ]),
        finish(),
    ]);
    let outcome = run_agent(
        &provider,
        &tools,
        configured(run, dir.path()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(outcome.executed_tool_call_count, 4);
    let requests = provider.requests();
    assert!(result(&requests[2], "a").content.contains("BODY-2"));
    assert!(result(&requests[2], "b").content.contains("BODY-3"));
    let directory = result(&requests[2], "directory-again");
    assert!(directory.content.contains("fixture-2"));
    assert!(directory.images.is_empty());
    assert!(!directory.content.contains("observation_budget_exceeded"));
    assert_provider_contract(&requests[2]);
}
