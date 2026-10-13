use super::*;

struct ResumeProvider {
    inner: MockProvider,
    run: ComputerUseRun,
    pause_round: usize,
    paused: Option<Arc<tokio::sync::Notify>>,
    release: Option<Arc<tokio::sync::Notify>>,
}

fn fresh_windows() -> Vec<DesktopTarget> {
    [2, 3]
        .into_iter()
        .map(|id| {
            let mut target = window(id);
            target.selection_id = format!("fresh-{id}");
            target
        })
        .collect()
}

fn refresh() -> Vec<ScriptStep> {
    batch(&[(
        "refresh",
        "ComputerObserve",
        json!({"target_id":null,"cursor":null}),
    )])
}

fn fresh_shots() -> Vec<ScriptStep> {
    batch(&[
        (
            "fresh-a",
            "ComputerScreenshot",
            json!({"target_id":"fresh-2"}),
        ),
        (
            "fresh-b",
            "ComputerScreenshot",
            json!({"target_id":"fresh-3"}),
        ),
    ])
}

impl Provider for ResumeProvider {
    fn chat_stream(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<crate::EventStream, VegaError>> {
        let pause = self.inner.requests().len() == self.pause_round;
        let future = self.inner.chat_stream(request, cancel);
        let run = self.run.clone();
        let paused = self.paused.clone();
        let release = self.release.clone();
        Box::pin(async move {
            if pause {
                run.control.pause();
                run.control.finish_cleanup();
                run.set_desktop_targets_fixture(fresh_windows()).await;
                if let (Some(paused), Some(release)) = (paused, release) {
                    paused.notify_one();
                    release.notified().await;
                } else {
                    run.control.resume(run.control.status().epoch).unwrap();
                }
            }
            future.await
        })
    }
}

fn assert_superseded(request: &ChatRequest, id: &str) {
    let message = result(request, id);
    assert_eq!(
        message.content,
        crate::computer_use::SUPERSEDED_CONTROL_OUTPUT
    );
    assert!(message.images.is_empty());
    let receipt: Value = serde_json::from_str(&message.content).unwrap();
    assert_eq!(receipt["executed"], false);
    assert_eq!(receipt["status"], "superseded");
}

#[tokio::test]
async fn issue75_resume_refresh_discards_model_epoch_calls_without_losing_mixed_tools() {
    for held_pause in [false, true] {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("note.txt"), "ordinary work").unwrap();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let run = ComputerUseRun::new(true, false);
        let actions = run
            .install_desktop_fixture(
                vec![window(2), window(3)],
                vec![frame(2), frame(3), frame(2), frame(3)],
            )
            .await;
        let paused = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let provider = ResumeProvider {
            inner: MockProvider::new_rounds(vec![
                directory(),
                batch(&[
                    (
                        "read-a",
                        "ComputerObserve",
                        json!({"target_id":"fixture-2","cursor":null}),
                    ),
                    (
                        "read-b",
                        "ComputerObserve",
                        json!({"target_id":"fixture-3","cursor":null}),
                    ),
                ]),
                batch(&[
                    (
                        "old-a",
                        "ComputerScreenshot",
                        json!({"target_id":"fixture-2"}),
                    ),
                    ("ordinary", "read", json!({"path":"note.txt"})),
                    (
                        "old-b",
                        "ComputerScreenshot",
                        json!({"target_id":"fixture-3"}),
                    ),
                ]),
                refresh(),
                fresh_shots(),
                finish(),
            ]),
            run: run.clone(),
            pause_round: 2,
            paused: held_pause.then_some(paused.clone()),
            release: held_pause.then_some(release.clone()),
        };
        let operation = run_agent(
            &provider,
            &tools,
            configured(run.clone(), dir.path()),
            CancellationToken::new(),
        );
        tokio::pin!(operation);
        if held_pause {
            tokio::select! { result=&mut operation=>panic!("ended while paused: {result:?}"), _=paused.notified()=>{} }
            assert_eq!(run.control.status().phase, ControlPhase::Paused);
            assert_eq!(run.fixture_calls().await.len(), 2);
            run.control.resume(run.control.status().epoch).unwrap();
            release.notify_one();
        }
        let outcome = operation.await.unwrap();
        assert!(!outcome.interrupted && !outcome.failed);
        assert_eq!(outcome.executed_tool_call_count, 7);
        assert_eq!(run.fixture_calls().await.len(), 4);
        assert_eq!(actions.load(Ordering::SeqCst), 0);
        assert_eq!(
            outcome
                .events
                .iter()
                .filter(|event| matches!(event, RuntimeEvent::ScreenshotDelivered { .. }))
                .count(),
            2
        );
        assert!(!outcome.events.iter().any(|event|matches!(event,RuntimeEvent::ToolCallProposed(call) if call.id.starts_with("old-"))));
        assert_eq!(outcome.events.iter().filter(|event|matches!(event,RuntimeEvent::ToolCallFinished(result) if result.call_id=="ordinary" && result.status==RuntimeToolStatus::Success)).count(),1);
        assert!(!outcome.events.iter().any(|event|matches!(event,RuntimeEvent::ToolCallFinished(result) if result.status!=RuntimeToolStatus::Success)));
        let requests = provider.inner.requests();
        assert_eq!(requests.len(), 6);
        assert_superseded(&requests[3], "old-a");
        assert_superseded(&requests[3], "old-b");
        for message in &requests[3].messages {
            assert!(message.images.is_empty());
            assert!(!message.content.contains("BODY-2") && !message.content.contains("BODY-3"));
        }
        assert!(result(&requests[5], "fresh-a").content.contains("Owned 2"));
        assert!(result(&requests[5], "fresh-b").content.contains("Owned 3"));
        for request in &requests {
            assert_provider_contract(request);
        }
        for message in &outcome.messages {
            assert!(!message.content.contains("BODY-"));
            assert!(message.images.is_empty());
            for call in &message.tool_calls {
                if ComputerUseRun::tool_capability(&call.name).is_some() {
                    assert!(crate::computer_use::ComputerUseAudit::valid(
                        &call.input_json
                    ));
                }
            }
        }
        assert!(!run.no_progress());
    }
}

#[tokio::test]
async fn issue75_resume_refresh_between_tools_keeps_ack_and_only_captures_remaining_window() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = ComputerUseRun::new(true, false);
    run.install_desktop_fixture(vec![window(2), window(3)], vec![frame(2), frame(3)])
        .await;
    let provider = MockProvider::new_rounds(vec![
        directory(),
        batch(&[
            (
                "delivered",
                "ComputerScreenshot",
                json!({"target_id":"fixture-2"}),
            ),
            (
                "old-b",
                "ComputerScreenshot",
                json!({"target_id":"fixture-3"}),
            ),
        ]),
        refresh(),
        batch(&[(
            "remaining",
            "ComputerScreenshot",
            json!({"target_id":"fresh-3"}),
        )]),
        finish(),
    ]);
    let observed = run.clone();
    let outcome=run_agent_with_sink(&provider,&tools,configured(run.clone(),dir.path()),CancellationToken::new(),move |event| {
        let pause = matches!(event,RuntimeEvent::ToolCallFinished(ref result) if result.call_id=="delivered");
        let run = observed.clone();
        async move {
            if pause {
                run.control.pause();
                run.control.finish_cleanup();
                run.set_desktop_targets_fixture(fresh_windows()).await;
                run.control.resume(run.control.status().epoch).unwrap();
            }
            Ok(())
        }
    }).await.unwrap();
    assert!(!outcome.interrupted && !outcome.failed);
    assert_eq!(outcome.executed_tool_call_count, 4);
    assert_eq!(run.fixture_calls().await.len(), 2);
    assert_eq!(
        outcome
            .events
            .iter()
            .filter(|event| matches!(event, RuntimeEvent::ScreenshotDelivered { .. }))
            .count(),
        2
    );
    let requests = provider.requests();
    assert_superseded(&requests[2], "old-b");
    let delivered = result(&requests[2], "delivered");
    assert!(delivered.content.contains("conversation_image_attachment"));
    assert!(delivered.images.is_empty());
    assert!(!delivered.content.contains("observation_id:"));
    assert_eq!(
        requests[4]
            .messages
            .iter()
            .map(|message| message.images.len())
            .sum::<usize>(),
        1
    );
    for request in &requests {
        assert_provider_contract(request);
    }
}

#[tokio::test]
async fn issue75_resume_refresh_after_approval_is_cancelled_without_dispatch_or_run_interruption() {
    for boundary in ["proposed", "approved", "running"] {
        let dir = tempdir().unwrap();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let run = ComputerUseRun::new(true, false);
        run.install_desktop_fixture(vec![window(2), window(3)], vec![frame(2), frame(3)])
            .await;
        let provider = MockProvider::new_rounds(vec![
            directory(),
            batch(&[
                (
                    "old-a",
                    "ComputerScreenshot",
                    json!({"target_id":"fixture-2"}),
                ),
                (
                    "old-b",
                    "ComputerScreenshot",
                    json!({"target_id":"fixture-3"}),
                ),
            ]),
            refresh(),
            fresh_shots(),
            finish(),
        ]);
        let observed = run.clone();
        let outcome = run_agent_with_sink(
            &provider,
            &tools,
            configured(run.clone(), dir.path()),
            CancellationToken::new(),
            move |event| {
                let pause = match &event {
                    RuntimeEvent::ToolCallProposed(call) => {
                        boundary == "proposed" && call.id == "old-a"
                    }
                    RuntimeEvent::ToolCallApproved { call_id, .. } => {
                        boundary == "approved" && call_id == "old-a"
                    }
                    RuntimeEvent::ToolCallRunning { call_id } => {
                        boundary == "running" && call_id == "old-a"
                    }
                    _ => false,
                };
                let run = observed.clone();
                async move {
                    if pause {
                        run.control.pause();
                        run.control.finish_cleanup();
                        run.set_desktop_targets_fixture(fresh_windows()).await;
                        run.control.resume(run.control.status().epoch).unwrap();
                    }
                    Ok(())
                }
            },
        )
        .await
        .unwrap();
        assert!(!outcome.interrupted && !outcome.failed);
        assert_eq!(outcome.executed_tool_call_count, 4);
        assert_eq!(run.fixture_calls().await.len(), 2);
        let cancelled = outcome
            .events
            .iter()
            .find_map(|event| match event {
                RuntimeEvent::ToolCallFinished(result) if result.call_id == "old-a" => Some(result),
                _ => None,
            })
            .unwrap();
        assert_eq!(cancelled.status, RuntimeToolStatus::Cancelled);
        assert_eq!(
            cancelled.output,
            crate::computer_use::SUPERSEDED_CONTROL_OUTPUT
        );
        assert_eq!(
            cancelled.approval.as_ref().unwrap().source,
            RuntimeApprovalSource::ComputerUse
        );
        assert_eq!(
            cancelled.approval.as_ref().unwrap().decision,
            RuntimeApprovalDecision::Once
        );
        assert!(
            !outcome.events.iter().any(
                |event| matches!(event,RuntimeEvent::ToolCallProposed(call) if call.id=="old-b")
            )
        );
        let requests = provider.requests();
        assert_superseded(&requests[2], "old-a");
        assert_superseded(&requests[2], "old-b");
        for request in &requests {
            assert_provider_contract(request);
        }
    }
}

#[tokio::test]
async fn issue75_resume_refresh_root_cancel_wins_over_superseded_model_batch() {
    for stop_control in [false, true] {
        let dir = tempdir().unwrap();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let run = ComputerUseRun::new(true, false);
        run.install_desktop_fixture(vec![window(2)], Vec::new())
            .await;
        let paused = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let provider = ResumeProvider {
            inner: MockProvider::new_rounds(vec![
                directory(),
                batch(&[(
                    "old",
                    "ComputerScreenshot",
                    json!({"target_id":"fixture-2"}),
                )]),
                finish(),
            ]),
            run: run.clone(),
            pause_round: 1,
            paused: Some(paused.clone()),
            release: Some(release.clone()),
        };
        let cancel = CancellationToken::new();
        let operation = run_agent(
            &provider,
            &tools,
            configured(run.clone(), dir.path()),
            cancel.clone(),
        );
        tokio::pin!(operation);
        tokio::select! { result=&mut operation=>panic!("ended before cancellation {result:?}"), _=paused.notified()=>{} }
        if stop_control {
            run.control.stop();
        } else {
            cancel.cancel();
        }
        release.notify_one();
        let outcome = operation.await.unwrap();
        assert!(outcome.interrupted);
        assert!(
            outcome
                .events
                .iter()
                .any(|event| matches!(event, RuntimeEvent::Interrupted))
        );
        assert_eq!(outcome.executed_tool_call_count, 1);
        assert!(run.fixture_calls().await.is_empty());
        assert_eq!(provider.inner.requests().len(), 2);
        assert!(
            !outcome
                .events
                .iter()
                .any(|event| matches!(event, RuntimeEvent::ScreenshotDelivered { .. }))
        );
    }
}

struct ActResumeProvider {
    run: ComputerUseRun,
    mode: &'static str,
    requests: Mutex<Vec<ChatRequest>>,
}

impl Provider for ActResumeProvider {
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
            1 => batch(&[(
                "observe",
                "ComputerObserve",
                json!({"target_id":"fixture-2","cursor":null}),
            )]),
            2 => {
                let observation = result(&request, "observe")
                    .content
                    .lines()
                    .find_map(|line| line.strip_prefix("observation_id: "))
                    .unwrap();
                let mut calls = vec![(
                    "action",
                    "ComputerAct",
                    json!({"observation_id":observation,"action":"type_text","target":"edit-2","text":"ONCE"}),
                )];
                if self.mode != "unknown" {
                    calls.push((
                        "old-shot",
                        "ComputerScreenshot",
                        json!({"target_id":"fixture-2"}),
                    ));
                }
                if self.mode == "prepared" {
                    self.run.control.pause();
                    self.run.control.finish_cleanup();
                    self.run
                        .control
                        .resume(self.run.control.status().epoch)
                        .unwrap();
                }
                batch(&calls)
            }
            3 => refresh(),
            4 => batch(&[(
                "fresh",
                "ComputerScreenshot",
                json!({"target_id":"fixture-2"}),
            )]),
            _ => finish(),
        };
        MockProvider::new(script).chat_stream(request, cancel)
    }
}

#[tokio::test]
async fn issue75_resume_refresh_never_replays_dispatched_or_prepared_input() {
    for mode in ["prepared", "completed", "unknown"] {
        let dir = tempdir().unwrap();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let run = ComputerUseRun::new(true, false);
        let mut frames = vec![frame(2)];
        if mode != "prepared" {
            frames.push(vega_mcp::ManagedResult {
                text: Vec::new(),
                images: Vec::new(),
                structured: None,
                is_error: false,
            });
            frames.push(if mode == "unknown" {
                vega_mcp::ManagedResult {
                    text: vec!["PRIVATE_POSTCHECK".into()],
                    images: Vec::new(),
                    structured: Some(json!({"code":"px_capture_unavailable"})),
                    is_error: true,
                }
            } else {
                let mut changed = frame(2);
                changed.structured.as_mut().unwrap()["tree_markdown"] = json!("BODY-2 ONCE");
                changed
            });
        }
        if mode != "unknown" {
            frames.push(frame(2));
        }
        let actions = run.install_desktop_fixture(vec![window(2)], frames).await;
        let provider = ActResumeProvider {
            run: run.clone(),
            mode,
            requests: Mutex::new(Vec::new()),
        };
        let control = run.control.clone();
        let outcome=run_agent_with_sink(&provider,&tools,configured(run.clone(),dir.path()),CancellationToken::new(),move |event| {
            if mode=="completed" && matches!(event,RuntimeEvent::ToolCallFinished(ref result) if result.call_id=="action") {
                control.pause();
                control.finish_cleanup();
                control.resume(control.status().epoch).unwrap();
            }
            async {Ok(())}
        }).await.unwrap();
        assert_eq!(
            actions.load(Ordering::SeqCst),
            usize::from(mode != "prepared")
        );
        if mode == "unknown" {
            let result = outcome
                .events
                .iter()
                .find_map(|event| match event {
                    RuntimeEvent::ToolCallFinished(result) if result.call_id == "action" => {
                        Some(result)
                    }
                    _ => None,
                })
                .unwrap();
            assert_eq!(result.status, RuntimeToolStatus::Failed);
            assert!(result.output.starts_with("outcome_unknown:"));
            assert!(
                !result.output.contains("superseded")
                    && !result.output.contains("PRIVATE_POSTCHECK")
            );
            assert!(
                !outcome
                    .events
                    .iter()
                    .any(|event| matches!(event, RuntimeEvent::ScreenshotDelivered { .. }))
            );
        } else {
            assert!(!outcome.interrupted && !outcome.failed);
            assert_eq!(
                outcome
                    .events
                    .iter()
                    .filter(|event| matches!(event, RuntimeEvent::ScreenshotDelivered { .. }))
                    .count(),
                1
            );
            let requests = provider.requests.lock().unwrap();
            assert_superseded(&requests[3], "old-shot");
            if mode == "prepared" {
                assert_superseded(&requests[3], "action");
            }
            assert!(!requests[3].messages.iter().any(|message|!message.images.is_empty() || message.content.contains("BODY-2")));
            for request in requests.iter() {
                assert_provider_contract(request);
            }
        }
    }
}

#[tokio::test]
async fn issue75_resume_refresh_model_epoch_requires_ready_phase_before_and_after_trim() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = ComputerUseRun::new(true, false);
    let cancel = CancellationToken::new();
    run.wait_ready(&cancel).await.unwrap();
    let initial = run.ready_model_epoch().unwrap();
    run.control.pause();
    assert_eq!(run.ready_model_epoch(), None);
    run.control.finish_cleanup();
    assert_eq!(run.ready_model_epoch(), None);
    let provider = MockProvider::new_rounds(vec![finish()]);
    let operation = run_agent(
        &provider,
        &tools,
        configured(run.clone(), dir.path()),
        cancel,
    );
    tokio::pin!(operation);
    std::future::poll_fn(|cx| {
        assert!(std::future::Future::poll(operation.as_mut(), cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    assert!(provider.requests().is_empty());
    run.control.resume(run.control.status().epoch).unwrap();
    assert_ne!(run.ready_model_epoch(), Some(initial));
    run.wait_ready(&CancellationToken::new()).await.unwrap();
    let before_trim = run.ready_model_epoch().unwrap();
    run.control.pause();
    run.trim_messages(&mut []);
    assert_ne!(run.ready_model_epoch(), Some(before_trim));
    run.control.finish_cleanup();
    run.control.resume(run.control.status().epoch).unwrap();
    let outcome = operation.await.unwrap();
    assert!(!outcome.interrupted && !outcome.failed);
    assert_eq!(provider.requests().len(), 1);
}

#[tokio::test]
async fn issue75_resume_refresh_execution_preflight_is_undispatched_and_cancel_has_priority() {
    for stop in [false, true] {
        let dir = tempdir().unwrap();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let run = ComputerUseRun::new(true, false);
        let actions = run
            .install_desktop_fixture(vec![window(2)], Vec::new())
            .await;
        let (entered, release) = run.gate_desktop_permission_check(4).await;
        let provider = MockProvider::new_rounds(vec![
            directory(),
            batch(&[(
                "preflight",
                "ComputerScreenshot",
                json!({"target_id":"fixture-2"}),
            )]),
            finish(),
        ]);
        let operation = run_agent(
            &provider,
            &tools,
            configured(run.clone(), dir.path()),
            CancellationToken::new(),
        );
        tokio::pin!(operation);
        tokio::select! {
            result = &mut operation => panic!("ended before execution preflight: {result:?}"),
            _ = entered.notified() => {}
        }
        if stop {
            run.control.stop();
        } else {
            run.control.pause();
            run.control.finish_cleanup();
            run.control.resume(run.control.status().epoch).unwrap();
        }
        release.notify_one();
        let outcome = operation.await.unwrap();
        assert_eq!(actions.load(Ordering::SeqCst), 0);
        assert!(run.fixture_calls().await.is_empty());
        assert!(
            !outcome
                .events
                .iter()
                .any(|event| matches!(event, RuntimeEvent::ScreenshotDelivered { .. }))
        );
        if stop {
            assert!(outcome.interrupted);
            assert!(!outcome.events.iter().any(|event|matches!(event,RuntimeEvent::ToolCallFinished(result) if result.output == crate::computer_use::SUPERSEDED_CONTROL_OUTPUT)));
        } else {
            assert!(!outcome.interrupted && !outcome.failed);
            assert_eq!(outcome.executed_tool_call_count, 1);
            assert!(outcome.events.iter().any(|event|matches!(event,RuntimeEvent::ToolCallFinished(result)
                if result.call_id == "preflight" && result.status == RuntimeToolStatus::Cancelled
                    && result.output == crate::computer_use::SUPERSEDED_CONTROL_OUTPUT
                    && result.approval.as_ref().is_some_and(|audit| audit.decision == RuntimeApprovalDecision::Once && audit.source == RuntimeApprovalSource::ComputerUse)
            )));
            assert_superseded(&provider.requests()[2], "preflight");
        }
        for request in provider.requests() {
            assert_provider_contract(&request);
        }
    }
}
