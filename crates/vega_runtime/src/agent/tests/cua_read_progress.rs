use super::*;
use crate::computer_use::{ComputerUseRun, DesktopTarget};
use serde_json::{Value, json};

#[derive(Clone, Copy)]
enum Step {
    Directory,
    Page,
    Observe(u32),
    Type,
    Screenshot(u32),
    Finish,
}

struct ReadProvider {
    run: ComputerUseRun,
    windows: Vec<DesktopTarget>,
    steps: Vec<Step>,
    requests: Arc<Mutex<Vec<ChatRequest>>>,
    targets: Mutex<std::collections::HashMap<u32, String>>,
}

fn values(message: &ChatMessage) -> impl Iterator<Item = Value> + '_ {
    message.content.lines().filter_map(|line| {
        serde_json::from_str(line.strip_prefix("AX elements: ").unwrap_or(line)).ok()
    })
}

impl crate::Provider for ReadProvider {
    fn chat_stream(
        &self,
        req: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<crate::EventStream, VegaError>> {
        let mut requests = self.requests.lock().unwrap();
        let index = requests.len();
        requests.push(req.clone());
        drop(requests);
        let last = req
            .messages
            .iter()
            .rev()
            .find(|message| message.role == ChatRole::Tool);
        let mut targets = self.targets.lock().unwrap();
        if let Some(last) = last {
            for value in values(last) {
                if let Some(windows) = value["windows"].as_array() {
                    for window in windows {
                        targets.insert(
                            window["window_id"].as_u64().unwrap() as u32,
                            window["target_id"].as_str().unwrap().into(),
                        );
                    }
                }
            }
        }
        let step = self.steps.get(index).copied().unwrap_or(Step::Finish);
        let script = if matches!(step, Step::Finish) {
            vec![
                ScriptStep::text("Already delivered."),
                ScriptStep::events(vec![ProviderEvent::Done {
                    stop_reason: StopReason::End,
                }]),
            ]
        } else {
            let (name, arguments) = match step {
                Step::Directory => ("ComputerObserve", json!({"target_id":null,"cursor":null})),
                Step::Page => {
                    let cursor = last
                        .and_then(|message| {
                            values(message)
                                .find_map(|value| value["next_cursor"].as_str().map(str::to_owned))
                        })
                        .unwrap();
                    ("ComputerObserve", json!({"target_id":null,"cursor":cursor}))
                }
                Step::Observe(window) => (
                    "ComputerObserve",
                    json!({"target_id":targets[&window],"cursor":null}),
                ),
                Step::Screenshot(window) => {
                    let current = last.and_then(|message| {
                        message.content.lines().find_map(|line| {
                            line.strip_prefix("Screenshot target: ")
                                .and_then(|value| serde_json::from_str::<Value>(value).ok())
                                .and_then(|value| value["target_id"].as_str().map(str::to_owned))
                        })
                    });
                    (
                        "ComputerScreenshot",
                        json!({"target_id":current.filter(|id|targets.get(&window)==Some(id)).unwrap_or_else(||targets[&window].clone())}),
                    )
                }
                Step::Type => {
                    let last = last.unwrap();
                    let id = last
                        .content
                        .lines()
                        .find_map(|line| line.strip_prefix("observation_id: "))
                        .unwrap();
                    let token = values(last)
                        .find_map(|value| value["element_token"].as_str().map(str::to_owned))
                        .unwrap();
                    (
                        "ComputerAct",
                        json!({"observation_id":id,"action":"type_text","target":token,"text":" APPENDED"}),
                    )
                }
                Step::Finish => unreachable!(),
            };
            vec![ScriptStep::events(vec![
                ProviderEvent::ToolUse {
                    id: format!("read-{index}"),
                    name: name.into(),
                    input_json: arguments.to_string(),
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                },
            ])]
        };
        let run = self.run.clone();
        let windows = self
            .windows
            .iter()
            .cloned()
            .map(|mut window| {
                window.selection_id = format!("target-{index}-{}", window.window_id);
                window
            })
            .collect();
        Box::pin(async move {
            if matches!(step, Step::Directory) {
                run.set_desktop_targets_fixture(windows).await;
            }
            MockProvider::new(script).chat_stream(req, cancel).await
        })
    }
}

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

fn frame(window: u32, token: usize, value: &str, caret: Option<u32>) -> vega_mcp::ManagedResult {
    let mut pixels = image::RgbImage::from_pixel(32, 20, image::Rgb([255, 255, 255]));
    if let Some(column) = caret {
        for y in 4..16 {
            pixels.put_pixel(column, y, image::Rgb([0, 0, 0]));
        }
    }
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(pixels)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    vega_mcp::ManagedResult {
        text: Vec::new(),
        images: vec![vega_mcp::ManagedImage {
            bytes: bytes.into_inner(),
            mime_type: "image/png".into(),
        }],
        structured: Some(
            json!({"pid":1,"window_id":window,"screenshot_frame_valid":true,"snapshot_id":format!("snapshot-{token}"),"capture_id":format!("capture-{token}"),"degraded":false,"elements":[{"element_token":format!("edit-{token}"),"role":"AXTextArea","label":"Editor","value":value}],"tree_markdown":value}),
        ),
        is_error: false,
    }
}

fn action_ack() -> vega_mcp::ManagedResult {
    vega_mcp::ManagedResult {
        text: Vec::new(),
        images: Vec::new(),
        structured: None,
        is_error: false,
    }
}

fn provider(run: ComputerUseRun, steps: Vec<Step>) -> ReadProvider {
    ReadProvider {
        run,
        windows: vec![window(2)],
        steps,
        requests: Arc::new(Mutex::new(Vec::new())),
        targets: Mutex::new(std::collections::HashMap::new()),
    }
}

fn configured_request(run: ComputerUseRun, root: &std::path::Path) -> AgentRequest {
    let mut req = request(vec![ChatMessage::new(
        ChatRole::User,
        "Append text in the owned window and show its screenshot.",
    )]);
    req.tool_config = tool_config(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::FullAccess,
        root.to_owned(),
    )
    .with_computer_use(Some(run));
    req
}

#[tokio::test]
async fn issue75_successful_reads_action_returns_current_target_and_delivers_without_rediscovery() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = ComputerUseRun::new(true, false);
    let actions = run
        .install_desktop_fixture(
            vec![window(2)],
            vec![
                frame(2, 0, "BEFORE", None),
                action_ack(),
                frame(2, 1, "BEFORE APPENDED", Some(3)),
                frame(2, 2, "BEFORE APPENDED", None),
            ],
        )
        .await;
    let provider = provider(
        run.clone(),
        vec![
            Step::Directory,
            Step::Observe(2),
            Step::Type,
            Step::Screenshot(2),
            Step::Finish,
        ],
    );
    let outcome = run_agent(
        &provider,
        &tools,
        configured_request(run.clone(), dir.path()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let requests = provider.requests.lock().unwrap();
    assert_eq!(requests.len(), 5);
    let action_reply = requests[3]
        .messages
        .iter()
        .find(|message| message.tool_call_id.as_deref() == Some("read-2"))
        .unwrap();
    assert!(action_reply.content.contains("BEFORE APPENDED"));
    assert_eq!(action_reply.images.len(), 1);
    assert_eq!(actions.load(Ordering::SeqCst), 1);
    assert_eq!(outcome.tool_call_count, 4);
    assert!(outcome.events.iter().any(|event|matches!(event,RuntimeEvent::ToolCallFinished(result) if result.call_id=="read-3" && result.status==RuntimeToolStatus::Success)));
    assert_eq!(
        outcome
            .events
            .iter()
            .filter(|event| matches!(event, RuntimeEvent::ScreenshotDelivered { .. }))
            .count(),
        1
    );
    assert!(action_reply.content.contains("Screenshot target: "));
    assert!(action_reply.content.contains("target-0-2"));
}

#[tokio::test]
async fn issue75_successful_reads_directory_window_cycle_stops_despite_changed_references() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = ComputerUseRun::new(true, false);
    let mut results = vec![
        frame(2, 0, "BEFORE", None),
        action_ack(),
        frame(2, 1, "BEFORE APPENDED", None),
    ];
    results.extend((2..5).map(|index| frame(2, index, "BEFORE APPENDED", Some(index as u32))));
    let actions = run.install_desktop_fixture(vec![window(2)], results).await;
    let provider = provider(
        run.clone(),
        vec![
            Step::Directory,
            Step::Observe(2),
            Step::Type,
            Step::Directory,
            Step::Observe(2),
            Step::Directory,
            Step::Observe(2),
            Step::Directory,
            Step::Observe(2),
            Step::Finish,
        ],
    );
    let outcome = run_agent(
        &provider,
        &tools,
        configured_request(run.clone(), dir.path()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let requests = provider.requests.lock().unwrap();
    assert_eq!(requests.len(), 8);
    assert_eq!(actions.load(Ordering::SeqCst), 1);
    assert_eq!(outcome.tool_call_count, 8);
    assert!(outcome.events.iter().any(|event|matches!(event,RuntimeEvent::ToolCallFinished(result) if result.output.contains("no_progress"))));
    assert!(outcome.final_text.contains("未取得进展"));
    assert!(!outcome.final_text.contains("Already delivered"));
    let catalogs = requests
        .iter()
        .flat_map(|request| request.messages.iter())
        .flat_map(values)
        .filter_map(|value| value["windows"][0]["target_id"].as_str().map(str::to_owned))
        .collect::<std::collections::HashSet<_>>();
    assert!(catalogs.len() >= 3);
}

#[tokio::test]
async fn issue75_successful_reads_caret_blink_does_not_reset_same_window() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = ComputerUseRun::new(true, false);
    run.install_desktop_fixture(
        vec![window(2)],
        vec![
            frame(2, 0, "UNCHANGED", Some(2)),
            frame(2, 1, "UNCHANGED", None),
            frame(2, 2, "UNCHANGED", Some(3)),
        ],
    )
    .await;
    let provider = provider(
        run.clone(),
        vec![
            Step::Directory,
            Step::Observe(2),
            Step::Observe(2),
            Step::Observe(2),
            Step::Finish,
        ],
    );
    let outcome = run_agent(
        &provider,
        &tools,
        configured_request(run, dir.path()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(provider.requests.lock().unwrap().len(), 4);
    assert!(outcome.events.iter().any(|event|matches!(event,RuntimeEvent::ToolCallFinished(result) if result.output.contains("no_progress"))));
}

#[tokio::test]
async fn issue75_successful_reads_pause_preserves_delivery_ack_without_stale_pixels() {
    for pause_in_final in [false, true] {
        let dir = tempdir().unwrap();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let run = ComputerUseRun::new(true, false);
        let actions = run
            .install_desktop_fixture(
                vec![window(2), window(3)],
                vec![frame(2, 0, "A", None), frame(3, 1, "B", None)],
            )
            .await;
        let mut provider = provider(
            run.clone(),
            vec![
                Step::Directory,
                Step::Screenshot(2),
                Step::Screenshot(3),
                Step::Finish,
            ],
        );
        provider.windows.push(window(3));
        let control = run.control.clone();
        let observed = control.clone();
        let (paused_tx, paused_rx) = tokio::sync::oneshot::channel();
        let mut paused_tx = Some(paused_tx);
        let operation = run_agent_with_sink(
            &provider,
            &tools,
            configured_request(run.clone(), dir.path()),
            CancellationToken::new(),
            move |event| {
                let should_pause = if pause_in_final {
                    matches!(event, RuntimeEvent::TextDelta(_))
                } else {
                    matches!(&event,RuntimeEvent::ToolCallFinished(result) if result.call_id=="read-2")
                };
                if should_pause && let Some(sender) = paused_tx.take() {
                    observed.pause();
                    let _ = sender.send(());
                }
                async { Ok(()) }
            },
        );
        tokio::pin!(operation);
        tokio::select! {result=&mut operation=>panic!("ended before pause: {result:?}"),result=paused_rx=>result.unwrap()}
        assert!(matches!(
            futures::poll!(&mut operation),
            std::task::Poll::Pending
        ));
        assert_eq!(
            control.status().phase,
            crate::computer_use::ControlPhase::Paused
        );
        control.resume(control.status().epoch).unwrap();
        let outcome = operation.await.unwrap();
        assert!(!outcome.interrupted);
        assert_eq!(actions.load(Ordering::SeqCst), 0);
        assert_eq!(outcome.tool_call_count, 3);
        assert_eq!(
            outcome
                .events
                .iter()
                .filter(|event| matches!(event, RuntimeEvent::ScreenshotDelivered { .. }))
                .count(),
            2
        );
        let requests = provider.requests.lock().unwrap();
        assert_eq!(requests.len(), 4);
        let messages = if pause_in_final {
            &outcome.messages
        } else {
            &requests[3].messages
        };
        for call_id in ["read-1", "read-2"] {
            let message = messages
                .iter()
                .find(|message| message.tool_call_id.as_deref() == Some(call_id))
                .unwrap();
            assert!(message.content.contains("conversation_image_attachment"));
            assert!(!message.content.contains("[External tool observation;"));
            assert!(message.images.is_empty());
        }
    }
}

#[tokio::test]
async fn issue75_successful_reads_distinct_windows_and_valid_pages_keep_advancing() {
    for pages in [false, true] {
        let dir = tempdir().unwrap();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let run = ComputerUseRun::new(true, false);
        let windows = (2..if pages { 702 } else { 7 })
            .map(window)
            .collect::<Vec<_>>();
        let results = if pages {
            Vec::new()
        } else {
            (2..7)
                .map(|id| frame(id, id as usize, "UNCHANGED", None))
                .collect()
        };
        run.install_desktop_fixture(windows.clone(), results).await;
        let mut steps = vec![Step::Directory];
        if pages {
            steps.extend([Step::Page, Step::Page, Step::Page]);
        } else {
            steps.extend((2..7).map(Step::Observe));
        }
        steps.push(Step::Finish);
        let expected = steps.len();
        let mut provider = provider(run.clone(), steps);
        provider.windows = windows;
        let outcome = run_agent(
            &provider,
            &tools,
            configured_request(run, dir.path()),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        let requests = provider.requests.lock().unwrap();
        assert_eq!(requests.len(), expected);
        assert_eq!(outcome.tool_call_count, expected - 1);
        assert!(
            outcome
                .events
                .iter()
                .filter_map(|event| match event {
                    RuntimeEvent::ToolCallFinished(result) => Some(result),
                    _ => None,
                })
                .all(|result| result.status == RuntimeToolStatus::Success)
        );
        assert!(!outcome.final_text.contains("未取得进展"));
        if pages {
            let ids = requests
                .iter()
                .flat_map(|request| request.messages.iter())
                .flat_map(values)
                .filter_map(|value| value["windows"].as_array().cloned())
                .flatten()
                .map(|value| value["window_id"].as_u64().unwrap())
                .collect::<std::collections::HashSet<_>>();
            assert_eq!(ids.len(), 700);
        }
    }
}

#[tokio::test]
async fn issue75_successful_reads_ax_viewport_and_canvas_changes_are_real_progress() {
    for mode in 0..3 {
        let dir = tempdir().unwrap();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let run = ComputerUseRun::new(true, false);
        let results = (0..5)
            .map(|index| {
                let value = if mode == 0 {
                    format!("VALUE-{index}")
                } else {
                    "UNCHANGED".into()
                };
                let mut result = frame(2, index, &value, Some(index as u32 + 1));
                if mode == 1 {
                    let mut bytes = std::io::Cursor::new(Vec::new());
                    image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
                        32,
                        20,
                        image::Rgb([index as u8 * 30, 0, 0]),
                    ))
                    .write_to(&mut bytes, image::ImageFormat::Png)
                    .unwrap();
                    result.images[0].bytes = bytes.into_inner();
                } else if mode == 2 {
                    result.structured.as_mut().unwrap()["elements"] = json!([]);
                }
                result
            })
            .collect();
        run.install_desktop_fixture(vec![window(2)], results).await;
        let provider = provider(
            run.clone(),
            vec![
                Step::Directory,
                Step::Observe(2),
                Step::Observe(2),
                Step::Observe(2),
                Step::Observe(2),
                Step::Observe(2),
                Step::Finish,
            ],
        );
        let outcome = run_agent(
            &provider,
            &tools,
            configured_request(run, dir.path()),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(provider.requests.lock().unwrap().len(), 7, "mode={mode}");
        assert_eq!(outcome.tool_call_count, 6);
        assert!(
            outcome
                .events
                .iter()
                .filter_map(|event| match event {
                    RuntimeEvent::ToolCallFinished(result) => Some(result),
                    _ => None,
                })
                .all(|result| result.status == RuntimeToolStatus::Success)
        );
    }
}

#[tokio::test]
async fn issue75_successful_reads_two_batches_after_pause_pass_real_image_and_wire_validation() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = ComputerUseRun::new(true, false);
    run.install_desktop_fixture(
        vec![window(2), window(3)],
        vec![
            frame(2, 0, "A", None),
            frame(3, 1, "B", None),
            colored_frame(2, 2, 90),
            colored_frame(3, 3, 120),
        ],
    )
    .await;
    let mut provider = provider(
        run.clone(),
        vec![
            Step::Directory,
            Step::Screenshot(2),
            Step::Screenshot(3),
            Step::Directory,
            Step::Screenshot(2),
            Step::Screenshot(3),
            Step::Finish,
        ],
    );
    provider.windows.push(window(3));
    let control = run.control.clone();
    let observed = control.clone();
    let (paused_tx, paused_rx) = tokio::sync::oneshot::channel();
    let mut paused_tx = Some(paused_tx);
    let operation = run_agent_with_sink(
        &provider,
        &tools,
        configured_request(run.clone(), dir.path()),
        CancellationToken::new(),
        move |event| {
            if matches!(&event,RuntimeEvent::ToolCallFinished(result) if result.call_id=="read-2")
                && let Some(sender) = paused_tx.take()
            {
                observed.pause();
                let _ = sender.send(());
            }
            async { Ok(()) }
        },
    );
    tokio::pin!(operation);
    tokio::select! {result=&mut operation=>panic!("ended before pause: {result:?}"),result=paused_rx=>result.unwrap()}
    assert!(matches!(
        futures::poll!(&mut operation),
        std::task::Poll::Pending
    ));
    assert_eq!(
        control.status().phase,
        crate::computer_use::ControlPhase::Paused
    );
    control.resume(control.status().epoch).unwrap();
    let outcome = operation.await.unwrap();
    assert!(!outcome.interrupted);
    assert_eq!(outcome.tool_call_count, 6);
    assert_eq!(
        outcome
            .events
            .iter()
            .filter(|event| matches!(event, RuntimeEvent::ScreenshotDelivered { .. }))
            .count(),
        4
    );
    let requests = provider.requests.lock().unwrap();
    assert_eq!(requests.len(), 7);
    for request in requests.iter() {
        crate::images::validate_messages(&request.messages).unwrap();
    }
    let request = requests.last().unwrap();
    let tool_ids = request
        .messages
        .iter()
        .filter_map(|message| message.tool_call_id.clone())
        .collect::<std::collections::HashSet<_>>();
    let call_ids = request
        .messages
        .iter()
        .flat_map(|message| message.tool_calls.iter().map(|call| call.id.clone()))
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(tool_ids, call_ids);
    assert_eq!(tool_ids.len(), 6);
    for call_id in ["read-1", "read-2", "read-4", "read-5"] {
        let message = request
            .messages
            .iter()
            .find(|message| message.tool_call_id.as_deref() == Some(call_id))
            .unwrap();
        assert!(message.content.contains("conversation_image_attachment"));
        assert!(!message.content.contains("Screenshot target: "));
        if ["read-1", "read-2"].contains(&call_id) {
            assert!(message.images.is_empty());
            assert!(!message.content.contains("[External tool observation;"));
        }
    }
    assert_eq!(
        request
            .messages
            .iter()
            .map(|message| message.images.len())
            .sum::<usize>(),
        1
    );
    let responses = crate::openai::responses::build_body(request).unwrap();
    let completions = crate::openai::build_request_body(request);
    assert_eq!(
        responses["input"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|item| item["type"] == "function_call_output")
            .count(),
        6
    );
    assert_eq!(
        completions["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|item| item["role"] == "tool")
            .count(),
        6
    );
    let mut stale = request.messages.clone();
    for call_id in ["read-1", "read-2", "read-4", "read-5"] {
        let message = stale
            .iter_mut()
            .find(|message| message.tool_call_id.as_deref() == Some(call_id))
            .unwrap();
        message.images = vec![run.screenshot(call_id).unwrap().0];
    }
    assert!(
        matches!(crate::images::validate_messages(&stale),Err(VegaError::Io(error)) if error.kind()==std::io::ErrorKind::InvalidInput && error.to_string()=="tool observation image budget exceeded")
    );
}

fn assert_private_calls_remain_audited(events: &[RuntimeEvent], count: usize) {
    let calls = events
        .iter()
        .filter_map(|event| match event {
            RuntimeEvent::ToolCallProposed(call) => Some(call),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), count);
    for call in calls {
        assert!(crate::computer_use::ComputerUseAudit::valid(
            &call.input_json
        ));
        assert!(!call.input_json.contains("target-"));
        assert!(!call.input_json.contains("APPENDED"));
        assert!(!call.input_json.contains("Owned"));
    }
}

fn assert_wire_calls_match(request: &ChatRequest) {
    crate::images::validate_messages(&request.messages).unwrap();
    let expected = request
        .messages
        .iter()
        .flat_map(|message| &message.tool_calls)
        .map(|call| {
            (
                call.id.clone(),
                serde_json::from_str::<Value>(&call.input_json).unwrap(),
            )
        })
        .collect::<std::collections::HashMap<_, _>>();
    let completions = crate::openai::build_request_body(request);
    let actual = completions["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|message| message["tool_calls"].as_array())
        .flatten()
        .map(|call| {
            (
                call["id"].as_str().unwrap().to_owned(),
                serde_json::from_str::<Value>(call["function"]["arguments"].as_str().unwrap())
                    .unwrap(),
            )
        })
        .collect::<std::collections::HashMap<_, _>>();
    assert_eq!(actual, expected);
    let responses = crate::openai::responses::build_body(request).unwrap();
    let actual = responses["input"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|call| call["type"] == "function_call")
        .map(|call| {
            (
                call["call_id"].as_str().unwrap().to_owned(),
                serde_json::from_str::<Value>(call["arguments"].as_str().unwrap()).unwrap(),
            )
        })
        .collect::<std::collections::HashMap<_, _>>();
    assert_eq!(actual, expected);
    let results = request
        .messages
        .iter()
        .filter_map(|message| message.tool_call_id.as_ref())
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(results, expected.into_keys().collect());
}

#[tokio::test]
async fn issue75_delivery_progress_two_images_keep_model_targets_and_completion_receipts() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = ComputerUseRun::new(true, false);
    run.install_desktop_fixture(
        vec![window(2), window(3)],
        vec![frame(2, 0, "A", None), frame(3, 1, "B", None)],
    )
    .await;
    let mut provider = provider(
        run.clone(),
        vec![
            Step::Directory,
            Step::Screenshot(2),
            Step::Screenshot(3),
            Step::Finish,
        ],
    );
    provider.windows.push(window(3));
    let outcome = run_agent(
        &provider,
        &tools,
        configured_request(run, dir.path()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(!outcome.interrupted);
    assert_eq!(outcome.final_text, "Already delivered.");
    assert_eq!(outcome.tool_call_count, 3);
    assert_eq!(
        outcome
            .events
            .iter()
            .filter(|event| matches!(event, RuntimeEvent::ScreenshotDelivered { .. }))
            .count(),
        2
    );
    assert_private_calls_remain_audited(&outcome.events, 3);
    for message in &outcome.messages {
        for call in &message.tool_calls {
            assert!(crate::computer_use::ComputerUseAudit::valid(
                &call.input_json
            ));
        }
        if message.role == ChatRole::Tool {
            assert!(!message.content.contains("Owned"));
            assert!(!message.content.contains("window_id"));
            assert!(message.images.is_empty());
        }
    }
    let requests = provider.requests.lock().unwrap();
    assert_eq!(requests.len(), 4);
    let final_request = requests.last().unwrap();
    for (call_id, target) in [("read-1", "target-0-2"), ("read-2", "target-0-3")] {
        let call = final_request
            .messages
            .iter()
            .flat_map(|message| &message.tool_calls)
            .find(|call| call.id == call_id)
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&call.input_json).unwrap(),
            json!({"target_id":target})
        );
        let message = final_request
            .messages
            .iter()
            .find(|message| message.tool_call_id.as_deref() == Some(call_id))
            .unwrap();
        let receipt = values(message)
            .find(|value| value["status"] == "delivered")
            .unwrap();
        assert_eq!(
            receipt["next_step"],
            "continue_remaining_requested_work_or_finish"
        );
        assert_eq!(
            receipt["window"]["title"],
            if call_id == "read-1" {
                "Owned 2"
            } else {
                "Owned 3"
            }
        );
        assert_eq!(receipt["unchanged"], false);
    }
    assert_wire_calls_match(final_request);
}

#[tokio::test]
async fn issue75_delivery_progress_action_model_parameters_are_ephemeral() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = ComputerUseRun::new(true, false);
    let actions = run
        .install_desktop_fixture(
            vec![window(2)],
            vec![
                frame(2, 0, "BEFORE", None),
                action_ack(),
                frame(2, 1, "BEFORE APPENDED", Some(3)),
                frame(2, 2, "BEFORE APPENDED", None),
            ],
        )
        .await;
    let provider = provider(
        run.clone(),
        vec![
            Step::Directory,
            Step::Observe(2),
            Step::Type,
            Step::Screenshot(2),
            Step::Finish,
        ],
    );
    let outcome = run_agent(
        &provider,
        &tools,
        configured_request(run, dir.path()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(actions.load(Ordering::SeqCst), 1);
    assert_private_calls_remain_audited(&outcome.events, 4);
    let requests = provider.requests.lock().unwrap();
    let call = requests[3]
        .messages
        .iter()
        .flat_map(|message| &message.tool_calls)
        .find(|call| call.id == "read-2")
        .unwrap();
    let args = serde_json::from_str::<Value>(&call.input_json).unwrap();
    assert_eq!(args["text"], " APPENDED");
    assert_eq!(args["target"], "edit-0");
    assert_eq!(args["action"], "type_text");
    assert!(
        args["observation_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
    );
    assert_wire_calls_match(requests.last().unwrap());
}

#[tokio::test]
async fn issue75_delivery_progress_repeated_screenshots_do_not_consume_attachment_budget() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = ComputerUseRun::new(true, false);
    let mut steps = Vec::new();
    let mut frames = Vec::new();
    for batch in 0..5 {
        steps.extend([Step::Directory, Step::Screenshot(2), Step::Screenshot(3)]);
        frames.extend([
            frame(2, batch * 2, "A", None),
            frame(3, batch * 2 + 1, "B", None),
        ]);
    }
    steps.push(Step::Finish);
    run.install_desktop_fixture(vec![window(2), window(3)], frames)
        .await;
    let mut provider = provider(run.clone(), steps);
    provider.windows.push(window(3));
    let outcome = run_agent(
        &provider,
        &tools,
        configured_request(run, dir.path()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(
        outcome
            .events
            .iter()
            .filter(|event| matches!(event, RuntimeEvent::ScreenshotDelivered { .. }))
            .count(),
        2
    );
    assert!(!outcome.interrupted);
    assert!(outcome.final_text.contains("重复"));
    assert!(outcome.tool_call_count <= 10);
    assert!(!outcome.events.iter().any(|event|matches!(event,RuntimeEvent::ToolCallFinished(result) if result.output.contains("screenshot_limit"))));
    let requests = provider.requests.lock().unwrap();
    for request in requests.iter() {
        assert_wire_calls_match(request);
    }
    for (call_id, original) in [("read-4", "read-1"), ("read-5", "read-2")] {
        let message = requests
            .last()
            .unwrap()
            .messages
            .iter()
            .find(|message| message.tool_call_id.as_deref() == Some(call_id))
            .unwrap();
        let receipt = values(message)
            .find(|value| value["status"] == "delivered")
            .unwrap();
        assert_eq!(receipt["artifact_id"], format!("screenshot:{original}"));
        assert_eq!(receipt["unchanged"], true);
        assert_eq!(
            receipt["next_step"],
            "continue_remaining_requested_work_or_finish"
        );
    }
}

fn colored_frame(window: u32, token: usize, color: u8) -> vega_mcp::ManagedResult {
    let mut result = frame(window, token, "CHANGED", None);
    let mut encoded = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        32,
        20,
        image::Rgb([color, 0, 0]),
    ))
    .write_to(&mut encoded, image::ImageFormat::Png)
    .unwrap();
    result.images[0].bytes = encoded.into_inner();
    result
}

fn reencoded_frame(window: u32, token: usize) -> vega_mcp::ManagedResult {
    use image::ImageEncoder;
    let mut result = frame(window, token, "UNCHANGED", None);
    let original = result.images[0].bytes.clone();
    let pixels = image::load_from_memory(&original).unwrap().to_rgb8();
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new_with_quality(
        &mut bytes,
        image::codecs::png::CompressionType::Best,
        image::codecs::png::FilterType::NoFilter,
    )
    .write_image(
        pixels.as_raw(),
        pixels.width(),
        pixels.height(),
        image::ExtendedColorType::Rgb8,
    )
    .unwrap();
    assert_ne!(bytes, original);
    assert_eq!(
        image::load_from_memory(&bytes).unwrap().to_rgba8(),
        image::load_from_memory(&original).unwrap().to_rgba8()
    );
    result.images[0].bytes = bytes;
    result
}

#[tokio::test]
async fn issue75_delivery_progress_reencoding_reuses_but_real_changes_and_other_windows_deliver() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = ComputerUseRun::new(true, false);
    run.install_desktop_fixture(
        vec![window(2), window(3)],
        vec![
            frame(2, 0, "A", None),
            reencoded_frame(2, 1),
            frame(3, 2, "B", None),
            colored_frame(2, 3, 60),
            frame(2, 4, "BACK TO A", None),
        ],
    )
    .await;
    let mut provider = provider(
        run.clone(),
        vec![
            Step::Directory,
            Step::Screenshot(2),
            Step::Screenshot(2),
            Step::Screenshot(3),
            Step::Screenshot(2),
            Step::Screenshot(2),
            Step::Finish,
        ],
    );
    provider.windows.push(window(3));
    let outcome = run_agent(
        &provider,
        &tools,
        configured_request(run, dir.path()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(outcome.final_text, "Already delivered.");
    let delivered = outcome
        .events
        .iter()
        .filter_map(|event| match event {
            RuntimeEvent::ScreenshotDelivered { call_id, .. } => Some(call_id.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(delivered, ["read-1", "read-3", "read-4", "read-5"]);
    let requests = provider.requests.lock().unwrap();
    let reused = requests
        .last()
        .unwrap()
        .messages
        .iter()
        .find(|message| message.tool_call_id.as_deref() == Some("read-2"))
        .unwrap();
    let receipt = values(reused)
        .find(|value| value["status"] == "delivered")
        .unwrap();
    assert_eq!(receipt["artifact_id"], "screenshot:read-1");
    assert_eq!(receipt["unchanged"], true);
    for request in requests.iter() {
        assert_wire_calls_match(request);
    }
}

#[tokio::test]
async fn issue75_delivery_progress_pause_rechecks_scope_then_reuses_existing_attachments() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = ComputerUseRun::new(true, false);
    run.install_desktop_fixture(
        vec![window(2), window(3)],
        vec![
            frame(2, 0, "A", None),
            frame(3, 1, "B", None),
            reencoded_frame(2, 2),
            reencoded_frame(3, 3),
        ],
    )
    .await;
    let mut provider = provider(
        run.clone(),
        vec![
            Step::Directory,
            Step::Screenshot(2),
            Step::Screenshot(3),
            Step::Directory,
            Step::Screenshot(2),
            Step::Screenshot(3),
            Step::Finish,
        ],
    );
    provider.windows.push(window(3));
    let control = run.control.clone();
    let observed = control.clone();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut tx = Some(tx);
    let operation = run_agent_with_sink(
        &provider,
        &tools,
        configured_request(run.clone(), dir.path()),
        CancellationToken::new(),
        move |event| {
            if matches!(&event,RuntimeEvent::ToolCallFinished(result) if result.call_id=="read-2")
                && let Some(tx) = tx.take()
            {
                observed.pause();
                let _ = tx.send(());
            }
            async { Ok(()) }
        },
    );
    tokio::pin!(operation);
    tokio::select! {result=&mut operation=>panic!("ended before pause {result:?}"), result=rx=>result.unwrap()}
    assert!(matches!(
        futures::poll!(&mut operation),
        std::task::Poll::Pending
    ));
    assert_eq!(
        control.status().phase,
        crate::computer_use::ControlPhase::Paused
    );
    let calls = run.fixture_calls().await.len();
    assert!(matches!(
        futures::poll!(&mut operation),
        std::task::Poll::Pending
    ));
    assert_eq!(run.fixture_calls().await.len(), calls);
    control.resume(control.status().epoch).unwrap();
    let outcome = operation.await.unwrap();
    assert_eq!(outcome.tool_call_count, 6);
    assert_eq!(outcome.final_text, "Already delivered.");
    assert_eq!(
        outcome
            .events
            .iter()
            .filter(|event| matches!(event, RuntimeEvent::ScreenshotDelivered { .. }))
            .count(),
        2
    );
    assert_eq!(run.fixture_calls().await.len(), 4);
    let requests = provider.requests.lock().unwrap();
    for request in requests.iter() {
        assert_wire_calls_match(request);
    }
    for (call, original) in [("read-4", "read-1"), ("read-5", "read-2")] {
        let message = requests
            .last()
            .unwrap()
            .messages
            .iter()
            .find(|message| message.tool_call_id.as_deref() == Some(call))
            .unwrap();
        let receipt = values(message)
            .find(|value| value["status"] == "delivered")
            .unwrap();
        assert_eq!(receipt["artifact_id"], format!("screenshot:{original}"));
        assert_eq!(receipt["unchanged"], true);
    }
    assert!(
        requests
            .last()
            .unwrap()
            .messages
            .iter()
            .filter(|message| matches!(message.tool_call_id.as_deref(), Some("read-1" | "read-2")))
            .all(|message| message.images.is_empty())
    );
}

#[tokio::test]
async fn issue75_delivery_progress_user_cancel_wins_over_repeated_capture_guard() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = ComputerUseRun::new(true, false);
    run.install_desktop_fixture(
        vec![window(2)],
        (0..4).map(|index| frame(2, index, "A", None)).collect(),
    )
    .await;
    let provider = provider(
        run.clone(),
        vec![
            Step::Directory,
            Step::Screenshot(2),
            Step::Screenshot(2),
            Step::Screenshot(2),
            Step::Screenshot(2),
            Step::Finish,
        ],
    );
    let cancel = CancellationToken::new();
    let stop = cancel.clone();
    let outcome = run_agent_with_sink(
        &provider,
        &tools,
        configured_request(run, dir.path()),
        cancel,
        move |event| {
            if matches!(&event,RuntimeEvent::ToolCallFinished(result) if result.call_id=="read-4") {
                stop.cancel();
            }
            async { Ok(()) }
        },
    )
    .await
    .unwrap();
    assert!(outcome.interrupted);
    assert!(
        outcome
            .events
            .iter()
            .any(|event| matches!(event, RuntimeEvent::Interrupted))
    );
    assert!(
        !outcome
            .events
            .iter()
            .any(|event| matches!(event, RuntimeEvent::Finished(_)))
    );
    assert_eq!(
        outcome
            .events
            .iter()
            .filter(|event| matches!(event, RuntimeEvent::ScreenshotDelivered { .. }))
            .count(),
        1
    );
    assert_eq!(provider.requests.lock().unwrap().len(), 5);
}

struct DeliveryCompaction {
    observed: Arc<Mutex<Vec<ChatMessage>>>,
}

impl ContextCompactionHook for DeliveryCompaction {
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
                    "Screenshots already delivered.",
                )],
                source_version: request.source_version + 1,
                source_fingerprint: request.source_fingerprint,
                replay_cold_prefix_messages: None,
                replay_interleaved_user_boundary: None,
                usages: Vec::new(),
                usage_complete: false,
            })
        }
        .boxed()
    }
}

#[tokio::test]
async fn issue75_delivery_progress_compaction_only_receives_safe_calls_and_delivery_receipts() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = ComputerUseRun::new(true, false);
    run.install_desktop_fixture(vec![window(2)], vec![frame(2, 0, "A", None)])
        .await;
    let capture = provider(
        run.clone(),
        vec![Step::Directory, Step::Screenshot(2), Step::Finish],
    );
    run_agent(
        &capture,
        &tools,
        configured_request(run, dir.path()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let messages = capture
        .requests
        .lock()
        .unwrap()
        .last()
        .unwrap()
        .messages
        .iter()
        .skip(1)
        .cloned()
        .collect::<Vec<_>>();
    assert!(
        messages
            .iter()
            .any(|message| message.content.contains("Owned 2"))
    );
    assert!(
        messages
            .iter()
            .flat_map(|message| &message.tool_calls)
            .any(|call| call.input_json.contains("target-0-2"))
    );
    let mut req = configured_request(ComputerUseRun::new(true, false), dir.path());
    req.history = messages;
    req.history.push(ChatMessage::new(
        ChatRole::Assistant,
        "padding ".repeat(16_000),
    ));
    let observed = Arc::new(Mutex::new(Vec::new()));
    req.context_compaction_hook = Some(Arc::new(DeliveryCompaction {
        observed: observed.clone(),
    }));
    req.context_source_version = Some(1);
    req.context_budget = Some(ContextBudget::new(20_000, 1_000, true).unwrap());
    let finish = MockProvider::new(vec![
        ScriptStep::text("Attached."),
        ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }]),
    ]);
    let outcome = run_agent(&finish, &tools, req, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(outcome.final_text, "Attached.");
    let compacted = observed.lock().unwrap();
    assert!(!compacted.is_empty());
    assert!(
        compacted
            .iter()
            .any(|message| message.content.contains("conversation_image_attachment"))
    );
    for message in compacted.iter() {
        assert!(message.images.is_empty());
        assert!(!message.content.contains("Owned 2"));
        assert!(!message.content.contains("window_id"));
        for call in &message.tool_calls {
            assert!(crate::computer_use::ComputerUseAudit::valid(
                &call.input_json
            ));
            assert!(!call.input_json.contains("target-0-2"));
        }
    }
}

#[tokio::test]
async fn issue75_delivery_progress_other_window_delivery_does_not_reset_duplicate_capture_count() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = ComputerUseRun::new(true, false);
    run.install_desktop_fixture(
        vec![window(2), window(3)],
        vec![
            frame(2, 0, "A", None),
            frame(2, 1, "A", None),
            frame(3, 2, "B", None),
            frame(2, 3, "A", None),
            colored_frame(3, 4, 90),
            frame(2, 5, "A", None),
        ],
    )
    .await;
    let mut provider = provider(
        run.clone(),
        vec![
            Step::Directory,
            Step::Screenshot(2),
            Step::Screenshot(2),
            Step::Screenshot(3),
            Step::Screenshot(2),
            Step::Observe(3),
            Step::Screenshot(2),
            Step::Finish,
        ],
    );
    provider.windows.push(window(3));
    let outcome = run_agent(
        &provider,
        &tools,
        configured_request(run, dir.path()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(outcome.tool_call_count, 7);
    assert_eq!(provider.requests.lock().unwrap().len(), 7);
    assert!(!outcome.interrupted);
    assert!(outcome.final_text.contains("已有截图保留"));
    assert!(outcome.events.iter().any(|event|matches!(event,RuntimeEvent::ToolCallFinished(result) if result.call_id=="read-6" && result.output.contains("no_progress"))));
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
async fn issue75_delivery_progress_changed_actions_in_another_window_preserve_duplicate_count() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = ComputerUseRun::new(true, false);
    let actions = run
        .install_desktop_fixture(
            vec![window(2), window(3)],
            vec![
                frame(2, 0, "A", None),
                frame(2, 1, "A", None),
                frame(3, 2, "B", None),
                action_ack(),
                frame(3, 3, "B APPENDED", Some(5)),
                frame(2, 4, "A", None),
                frame(3, 5, "B APPENDED", None),
                action_ack(),
                frame(3, 6, "B APPENDED APPENDED", Some(6)),
                frame(2, 7, "A", None),
                frame(3, 8, "B APPENDED APPENDED", None),
                action_ack(),
                frame(3, 9, "B APPENDED APPENDED APPENDED", Some(7)),
            ],
        )
        .await;
    let mut provider = provider(
        run.clone(),
        vec![
            Step::Directory,
            Step::Screenshot(2),
            Step::Screenshot(2),
            Step::Observe(3),
            Step::Type,
            Step::Screenshot(2),
            Step::Observe(3),
            Step::Type,
            Step::Screenshot(2),
            Step::Observe(3),
            Step::Type,
            Step::Finish,
        ],
    );
    provider.windows.push(window(3));
    let outcome = run_agent(
        &provider,
        &tools,
        configured_request(run.clone(), dir.path()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(actions.load(Ordering::SeqCst), 2);
    assert_eq!(outcome.tool_call_count, 9);
    assert_eq!(provider.requests.lock().unwrap().len(), 9);
    assert!(outcome.final_text.contains("已有截图保留"));
    assert!(!outcome.interrupted);
    assert!(outcome.events.iter().any(|event|matches!(event,RuntimeEvent::ToolCallFinished(result) if result.call_id=="read-8" && result.output.contains("no_progress"))));
    assert_eq!(
        outcome
            .events
            .iter()
            .filter(|event| matches!(event, RuntimeEvent::ScreenshotDelivered { .. }))
            .count(),
        1
    );
    for call in ["read-4", "read-7"] {
        assert!(outcome.events.iter().any(|event|matches!(event,RuntimeEvent::ToolCallFinished(result) if result.call_id==call && result.status==RuntimeToolStatus::Success)));
    }
    let requests = provider.requests.lock().unwrap();
    for (index, text) in [(5, "B APPENDED"), (8, "B APPENDED APPENDED")] {
        let action = requests[index]
            .messages
            .iter()
            .rev()
            .find(|message| message.role == ChatRole::Tool)
            .unwrap();
        assert!(action.content.contains(text));
        assert_eq!(action.images.len(), 1);
    }
    assert_wire_calls_match(requests.last().unwrap());
}

#[tokio::test]
async fn issue75_delivery_progress_changed_action_in_same_window_resets_its_duplicate_count() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = ComputerUseRun::new(true, false);
    let actions = run
        .install_desktop_fixture(
            vec![window(2)],
            vec![
                frame(2, 0, "A", None),
                frame(2, 1, "A", None),
                frame(2, 2, "A", None),
                frame(2, 3, "A", None),
                action_ack(),
                frame(2, 4, "A APPENDED", None),
                frame(2, 5, "A APPENDED", None),
            ],
        )
        .await;
    let provider = provider(
        run.clone(),
        vec![
            Step::Directory,
            Step::Screenshot(2),
            Step::Screenshot(2),
            Step::Screenshot(2),
            Step::Observe(2),
            Step::Type,
            Step::Screenshot(2),
            Step::Finish,
        ],
    );
    let outcome = run_agent(
        &provider,
        &tools,
        configured_request(run, dir.path()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(actions.load(Ordering::SeqCst), 1);
    assert_eq!(outcome.tool_call_count, 7);
    assert_eq!(outcome.final_text, "Already delivered.");
    assert!(!outcome.interrupted);
    assert_eq!(
        outcome
            .events
            .iter()
            .filter(|event| matches!(event, RuntimeEvent::ScreenshotDelivered { .. }))
            .count(),
        1
    );
    let requests = provider.requests.lock().unwrap();
    assert_eq!(requests.len(), 8);
    assert!(
        requests[6]
            .messages
            .iter()
            .rev()
            .find(|message| message.role == ChatRole::Tool)
            .unwrap()
            .content
            .contains("A APPENDED")
    );
    let receipt = requests
        .last()
        .unwrap()
        .messages
        .iter()
        .find(|message| message.tool_call_id.as_deref() == Some("read-6"))
        .unwrap();
    assert!(
        values(receipt)
            .any(|value| value["unchanged"] == true && value["artifact_id"] == "screenshot:read-1")
    );
    assert_wire_calls_match(requests.last().unwrap());
}
