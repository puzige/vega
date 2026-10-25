use super::*;
use crate::{
    AgentRequest, ChatRequest, ChatRole, EventStream, Provider, ProviderEvent, RuntimeEvent,
    RuntimePermissionMode, RuntimeRunMode, RuntimeToolConfig, RuntimeToolStatus, StopReason,
    VegaError,
};
use futures::future::BoxFuture;
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::Duration;

#[derive(Default)]
struct DriverState {
    permissions: [bool; 2],
    connections: usize,
    calls: Vec<String>,
    windows: usize,
    unknown_action: bool,
    pause_on_action: Option<ComputerUseControl>,
}

#[derive(Clone)]
pub(super) struct Driver(Arc<Mutex<DriverState>>);

impl Driver {
    fn new(permissions: [bool; 2], unknown_action: bool) -> Self {
        Self(Arc::new(Mutex::new(DriverState {
            permissions,
            unknown_action,
            ..DriverState::default()
        })))
    }

    pub(super) fn connected(&self) {
        self.0.lock().unwrap().connections += 1;
    }

    pub(super) fn windows(&self) -> Result<Vec<DesktopTarget>, ComputerUseError> {
        self.0.lock().unwrap().windows += 1;
        Ok(vec![DesktopTarget {
            selection_id: "driver-reference".into(),
            application: "Example".into(),
            bundle_id: "example.test".into(),
            pid: 7,
            window_id: 11,
            title: "Private window text".into(),
        }])
    }

    pub(super) fn call(
        &self,
        name: &str,
        _: Value,
    ) -> Result<vega_mcp::ManagedResult, vega_mcp::McpError> {
        let mut state = self.0.lock().unwrap();
        state.calls.push(name.into());
        if name == "check_permissions" {
            return Ok(vega_mcp::ManagedResult {
                text: Vec::new(),
                images: Vec::new(),
                is_error: false,
                structured: Some(
                    json!({"accessibility":state.permissions[0],"screen_recording":state.permissions[1]}),
                ),
            });
        }
        if state.unknown_action && name == "click" {
            if let Some(control) = &state.pause_on_action {
                control.pause();
            }
            return Err(vega_mcp::McpError::Timeout);
        }
        panic!("unexpected managed call: {name}");
    }
}

async fn configured_run(driver: &Driver) -> ComputerUseRun {
    let mut run = ComputerUseRun::new(true, false);
    run.control =
        ComputerUseControl::with_authority(Arc::new(ownership::OwnershipAuthority::default()));
    run.backend.lock().await.test_driver = Some(driver.clone());
    run
}

fn request(run: ComputerUseRun) -> AgentRequest {
    AgentRequest {
        model: "scripted".into(),
        system_prompt: "Perform the requested task safely.".into(),
        history: vec![crate::ChatMessage::new(
            ChatRole::User,
            "Inspect the window and explain what remains.",
        )],
        max_tokens: None,
        completed_tool_results: HashMap::new(),
        tool_config: RuntimeToolConfig::new(
            RuntimeRunMode::Execute,
            RuntimePermissionMode::FullAccess,
            "test-project".into(),
            "test-thread".into(),
            std::path::PathBuf::new(),
            Vec::new(),
        )
        .with_computer_use(Some(run))
        .with_turn_limit(12),
        pricing_catalog: None,
        reasoning: None,
        model_snapshot: None,
        replay_frame: None,
        context_budget: None,
        context_source_version: None,
        context_source_fingerprint: None,
        context_operation_id: None,
        context_compaction_hook: None,
    }
}

fn tool(id: &str, name: &str, input: Value) -> ProviderEvent {
    ProviderEvent::ToolUse {
        id: id.into(),
        name: name.into(),
        input_json: input.to_string(),
    }
}

fn directory(id: &str) -> ProviderEvent {
    tool(
        id,
        "ComputerObserve",
        json!({"target_id":null,"cursor":null}),
    )
}

fn result<'a>(request: &'a ChatRequest, id: &str) -> &'a crate::ChatMessage {
    let messages = request
        .messages
        .iter()
        .filter(|message| {
            message.role == ChatRole::Tool && message.tool_call_id.as_deref() == Some(id)
        })
        .collect::<Vec<_>>();
    assert_eq!(messages.len(), 1, "tool result must be paired exactly once");
    assert!(
        request
            .messages
            .iter()
            .flat_map(|message| &message.tool_calls)
            .any(|call| call.id == id)
    );
    messages[0]
}

fn error_result(request: &ChatRequest, id: &str) -> Value {
    serde_json::from_str(&result(request, id).content).unwrap()
}

struct ScriptedProvider {
    steps: Mutex<VecDeque<Vec<ProviderEvent>>>,
    requests: Mutex<Vec<ChatRequest>>,
    expected_error: Option<(&'static str, &'static str)>,
}

impl ScriptedProvider {
    fn new(steps: Vec<Vec<ProviderEvent>>) -> Self {
        Self {
            steps: Mutex::new(steps.into()),
            requests: Mutex::new(Vec::new()),
            expected_error: None,
        }
    }
}

impl Provider for ScriptedProvider {
    fn chat_stream(
        &self,
        req: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        assert!(!cancel.is_cancelled());
        let step = self
            .steps
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected provider request");
        if let Some((id, code)) = self.expected_error
            && (req
                .messages
                .iter()
                .any(|message| message.tool_call_id.as_deref() == Some(id))
                || step
                    .iter()
                    .any(|event| matches!(event, ProviderEvent::TextDelta(_))))
        {
            assert_eq!(error_result(&req, id)["code"], code);
        }
        self.requests.lock().unwrap().push(req);
        Box::pin(async move {
            Ok(Box::pin(futures::stream::iter(step.into_iter().map(Ok))) as EventStream)
        })
    }
}

fn batch(mut calls: Vec<ProviderEvent>) -> Vec<ProviderEvent> {
    calls.push(ProviderEvent::Done {
        stop_reason: StopReason::ToolUse,
    });
    calls
}

fn explanation(text: &str) -> Vec<ProviderEvent> {
    vec![
        ProviderEvent::TextDelta(text.into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ]
}

#[tokio::test]
async fn permission_failures_reach_the_next_provider_request_and_freeze_driver_calls() {
    for (permissions, code) in [
        ([true, false], "screen_recording_permission_missing"),
        ([false, true], "accessibility_permission_missing"),
        ([false, false], "permissions_missing"),
    ] {
        let driver = Driver::new(permissions, false);
        let run = configured_run(&driver).await;
        let mut provider = ScriptedProvider::new(vec![
            batch(vec![
                tool(
                    "permission",
                    "ComputerObserve",
                    json!({"target_id":"not-yet-resolved","cursor":null}),
                ),
                tool("independent", "glob", json!({"pattern":"no-match-*"})),
                directory("same-batch-frozen"),
            ]),
            batch(vec![directory("frozen")]),
            explanation(
                "Computer access is unavailable; I can explain the remaining work without controlling it.",
            ),
        ]);
        provider.expected_error = Some(("permission", code));
        let root = CancellationToken::new();
        let tools = vega_tools::Tools::new(std::env::current_dir().unwrap()).unwrap();
        let outcome = tokio::time::timeout(
            Duration::from_secs(3),
            crate::run_agent(&provider, &tools, request(run), root.clone()),
        )
        .await
        .unwrap()
        .unwrap();
        let requests = provider.requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        for (index, id) in [(1, "permission"), (1, "same-batch-frozen"), (2, "frozen")] {
            let error = error_result(&requests[index], id);
            assert_eq!(error["code"], code);
            assert_eq!(error["capability"], "desktop");
            assert_eq!(error["stage"], "permissions");
            assert_eq!(error["execution_state"], "not_dispatched");
            assert_eq!(error["retryable"], false);
            assert_eq!(error["next_step"], "open_system_settings");
        }
        assert!(
            !result(&requests[1], "independent")
                .content
                .starts_with("Tool error:")
        );
        let state = driver.0.lock().unwrap();
        assert_eq!(state.connections, 1);
        assert_eq!(state.calls, ["check_permissions"]);
        assert_eq!(state.windows, 0);
        assert!(!root.is_cancelled());
        assert!(!outcome.interrupted && !outcome.failed);
        assert_eq!(
            outcome.final_text,
            "Computer access is unavailable; I can explain the remaining work without controlling it."
        );
        assert!(outcome.events.iter().any(|event| matches!(event,
            RuntimeEvent::ToolCallFinished(result) if result.call_id == "permission"
                && result.status == RuntimeToolStatus::Rejected)));
    }
}

#[tokio::test]
async fn repeated_directory_guard_returns_truthful_diagnostics_and_the_model_continues() {
    let driver = Driver::new([true, true], false);
    let run = configured_run(&driver).await;
    let mut provider = ScriptedProvider::new(vec![
        batch(vec![directory("first")]),
        batch(vec![directory("second")]),
        batch(vec![
            directory("third"),
            directory("blocked"),
            tool("other", "glob", json!({"pattern":"no-match-*"})),
        ]),
        explanation(
            "The directory read succeeded, but repeated reads triggered the progress guard. I will explain what remains.",
        ),
    ]);
    provider.expected_error = Some(("third", "no_progress"));
    let root = CancellationToken::new();
    let tools = vega_tools::Tools::new(std::env::current_dir().unwrap()).unwrap();
    let outcome = tokio::time::timeout(
        Duration::from_secs(3),
        crate::run_agent(&provider, &tools, request(run), root.clone()),
    )
    .await
    .unwrap()
    .unwrap();
    let requests = provider.requests.lock().unwrap();
    assert_eq!(requests.len(), 4);
    let error = error_result(&requests[3], "third");
    assert_eq!(error["code"], "no_progress");
    assert_eq!(error["execution_state"], "failed");
    assert_eq!(
        error["guard"],
        json!({"trigger":"repeated_directory","count":3,"threshold":3,
        "underlying_result":"succeeded","dispatch_started":true})
    );
    let blocked = error_result(&requests[3], "blocked");
    assert_eq!(blocked["execution_state"], "not_dispatched");
    assert_eq!(blocked["guard"], error["guard"]);
    assert!(
        !result(&requests[3], "other")
            .content
            .starts_with("Tool error:")
    );
    assert!(!error.to_string().contains("Private window text"));
    assert!(!error.to_string().contains("window_id"));
    assert_eq!(driver.0.lock().unwrap().windows, 3);
    assert!(!root.is_cancelled() && !outcome.interrupted && !outcome.failed);
    assert_eq!(
        outcome.final_text,
        "The directory read succeeded, but repeated reads triggered the progress guard. I will explain what remains."
    );
}

fn observation(run: &ComputerUseRun, id: &str, catalog: bool) -> Observation {
    Observation {
        id: id.into(),
        run_id: run.control.run_id().into(),
        call_id: id.into(),
        epoch: run.control.status().epoch,
        capability: Capability::Desktop,
        source: ToolObservationSource::CuaWindow,
        model_image: false,
        read_only: false,
        target: if catalog {
            Target::DesktopCatalog
        } else {
            Target::Desktop(DesktopTarget {
                selection_id: "reference".into(),
                application: "Example".into(),
                bundle_id: "example.test".into(),
                pid: 7,
                window_id: 11,
                title: "Private window text".into(),
            })
        },
        image: None,
        original_width: 100,
        original_height: 100,
        text: if catalog {
            json!({"windows":[{"pid":7,"window_id":11,"title":"Private window text","target_id":id}]}).to_string()
        } else {
            "visible".into()
        },
        snapshot_id: Some(id.into()),
        capture_id: Some(id.into()),
        elements: json!([{"role":"AXButton","element_token":"button"}]),
        background_input: background::BackgroundInput::default(),
    }
}

#[test]
fn interleaved_window_reads_do_not_reset_the_third_identical_directory() {
    let run = ComputerUseRun::new(true, false);
    let mut guard = progress::ProgressGuard::default();
    for index in 0..5 {
        let catalog = index % 2 == 0;
        let after = observation(&run, &format!("fresh-{index}"), catalog);
        let attempt = progress::Attempt::new(
            "ComputerObserve",
            &json!({"target_id":index}),
            Some(&after.target),
            None,
        );
        let stopped = guard.finish(attempt, &Ok("completed".into()), Some(&after), false, true);
        if index < 4 {
            assert!(stopped.is_none());
        } else {
            let stopped = stopped.unwrap();
            assert_eq!(stopped.trigger, progress::GuardTrigger::RepeatedDirectory);
            assert_eq!(stopped.count, 3);
            assert_eq!(stopped.threshold, 3);
            assert_eq!(
                stopped.underlying_result,
                progress::UnderlyingResult::Succeeded
            );
        }
    }
}

#[tokio::test]
async fn dispatched_unknown_action_is_never_replayed_and_later_calls_are_not_dispatched() {
    let driver = Driver::new([true, true], true);
    let run = configured_run(&driver).await;
    let epoch = run.control.begin(Capability::Desktop).unwrap();
    run.control
        .connected(epoch, Capability::Desktop, Target::DesktopCatalog)
        .unwrap();
    let mut previous = observation(&run, "observed", false);
    run.control
        .set_windows(epoch, driver.windows().unwrap())
        .unwrap();
    let target = run.control.catalog_page(epoch, None).unwrap()["windows"][0]["target_id"]
        .as_str()
        .unwrap()
        .to_string();
    run.control.select_window(epoch, &target).unwrap();
    previous.target = run.control.target(Capability::Desktop).unwrap().unwrap();
    run.control.publish(previous).unwrap();
    driver.0.lock().unwrap().pause_on_action = Some(run.control.clone());
    let input =
        json!({"observation_id":"observed","action":"click","target":"button","x":null,"y":null});
    let root = CancellationToken::new();
    let unknown = run
        .execute_at("ComputerAct", "action", input.clone(), epoch, &root)
        .await
        .unwrap_err();
    let value: Value = serde_json::from_str(&unknown.feedback()).unwrap();
    assert_eq!(value["code"], "timeout");
    assert_eq!(value["execution_state"], "outcome_unknown");
    assert_eq!(value["next_step"], "ask_user_to_verify_without_replay");
    assert!(valid_error_output(
        "ComputerAct",
        &unknown.feedback(),
        false
    ));
    assert_eq!(run.control.status().phase, ControlPhase::Failed);
    assert!(run.control.resume(run.control.status().epoch).is_err());
    let calls = driver.0.lock().unwrap().calls.clone();
    let later = run
        .execute_at(
            "ComputerAct",
            "later",
            input,
            run.control.status().epoch,
            &root,
        )
        .await
        .unwrap_err();
    let later: Value = serde_json::from_str(&later.feedback()).unwrap();
    assert_eq!(later["code"], "timeout");
    assert_eq!(later["execution_state"], "not_dispatched");
    assert!(valid_error_output("ComputerAct", &later.to_string(), false));
    assert_eq!(driver.0.lock().unwrap().calls, calls);
    assert_eq!(
        calls.iter().filter(|call| call.as_str() == "click").count(),
        1
    );
    assert!(!root.is_cancelled());
}

#[tokio::test]
async fn failed_control_still_honors_a_real_stop_and_pause_cannot_restart_it() {
    let driver = Driver::new([true, false], false);
    let run = configured_run(&driver).await;
    let epoch = run.control.status().epoch;
    let error = diagnostics::failure(
        Capability::Desktop,
        diagnostics::Stage::Permissions,
        diagnostics::Class::ScreenRecordingPermissionMissing,
    );
    run.control.fail(epoch, error);
    run.control.pause();
    assert_eq!(run.control.status().phase, ControlPhase::Failed);
    assert!(run.control.resume(run.control.status().epoch).is_err());
    let root = CancellationToken::new();
    assert_eq!(
        run.wait_ready(&root).await,
        Ok(Readiness::Unavailable(error))
    );
    let mut cleanup = run.control.subscribe();
    let lifetime = CancellationToken::new();
    let monitor_run = run.clone();
    let monitor_root = root.clone();
    let monitor = tokio::spawn(async move { monitor_run.monitor(lifetime, monitor_root).await });
    tokio::time::timeout(Duration::from_secs(1), cleanup.changed())
        .await
        .unwrap()
        .unwrap();
    assert!(!root.is_cancelled());
    run.control.stop();
    tokio::time::timeout(Duration::from_secs(1), root.cancelled())
        .await
        .unwrap();
    monitor.await.unwrap();
    assert_eq!(
        run.wait_ready(&root).await,
        Err(ComputerUseError::Cancelled)
    );
    let fresh = configured_run(&driver).await;
    assert_eq!(
        fresh.wait_ready(&CancellationToken::new()).await,
        Ok(Readiness::Ready)
    );
}

#[tokio::test]
async fn resumed_control_supersedes_old_requests_without_backend_work() {
    let driver = Driver::new([true, true], false);
    let run = configured_run(&driver).await;
    let epoch = run.control.begin(Capability::Desktop).unwrap();
    run.control
        .connected(epoch, Capability::Desktop, Target::DesktopCatalog)
        .unwrap();
    let old_observation = observation(&run, "old-observation", true);
    run.control.pause();
    assert!(run.cleanup_revoked().await);
    run.control.resume(run.control.status().epoch).unwrap();
    let root = CancellationToken::new();
    let failure = run
        .execute_at("ComputerObserve", "old", json!({}), epoch, &root)
        .await
        .unwrap_err();
    assert_eq!(failure.feedback(), SUPERSEDED_CONTROL_OUTPUT);
    assert!(driver.0.lock().unwrap().calls.is_empty());
    assert!(run.control.publish(old_observation).is_err());
    assert!(!root.is_cancelled());
}

#[test]
fn screenshot_capacity_diagnostics_keep_existing_limits_and_success_fact() {
    let mut guard = progress::ProgressGuard::default();
    guard.screenshot_limit(5, 1024);
    let diagnostic = guard.stopped().unwrap();
    assert_eq!(diagnostic.trigger, progress::GuardTrigger::ScreenshotCount);
    assert_eq!(diagnostic.count, 5);
    assert_eq!(diagnostic.threshold, 4);
    assert_eq!(
        diagnostic.underlying_result,
        progress::UnderlyingResult::Succeeded
    );
    guard.screenshot_limit(4, 16 * 1024 * 1024 + 1);
    assert_eq!(guard.stopped().unwrap().threshold, 16 * 1024 * 1024);
}

#[tokio::test]
async fn reused_screenshot_error_preserves_the_error_and_earlier_delivery_receipt() {
    let run = ComputerUseRun::new(true, false);
    let mut encoded = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgba8(1, 1)
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    let image = crate::ImageAttachment::from_bytes(encoded.into_inner()).unwrap();
    let Target::Desktop(target) = observation(&run, "window", false).target else {
        panic!()
    };
    run.screenshots.lock().unwrap().push(ScreenshotCapture {
        call_id: "delivered".into(),
        image,
        epoch: 1,
        delivered: true,
        target,
        pixels: [0; 32],
        aliases: vec!["reused".into()],
    });
    let mut failure = ToolFailure::new(
        ComputerUseError::NoProgress,
        Capability::Desktop,
        diagnostics::Stage::Progress,
        ExecutionState::Failed,
    );
    failure.guard = Some(progress::GuardDiagnostic {
        trigger: progress::GuardTrigger::RepeatedWindow,
        count: 3,
        threshold: 3,
        underlying_result: progress::UnderlyingResult::Succeeded,
        dispatch_started: true,
    });
    let output = failure.feedback();
    let mut messages = vec![
        crate::ChatMessage::tool_result("delivered", screenshot_receipt("delivered")),
        crate::ChatMessage::tool_result("reused", &output),
    ];
    run.trim_messages(&mut messages);
    assert_eq!(messages[1].content, output);
    assert_eq!(
        serde_json::from_str::<Value>(&messages[0].content).unwrap()["status"],
        "delivered"
    );
    assert!(valid_error_output("ComputerScreenshot", &output, false));
    let budget = run
        .limited_screenshot_receipt(
            "reused",
            ComputerUseError::ObservationBudget { executed: true },
        )
        .unwrap();
    let budget: Value = serde_json::from_str(&budget).unwrap();
    assert_eq!(budget["status"], "delivered");
    assert_eq!(budget["model_projection"]["execution_state"], "failed");
}
