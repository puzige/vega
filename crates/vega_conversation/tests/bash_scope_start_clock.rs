use std::fs::{self, DirBuilder};
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::FutureExt;
use futures::future::BoxFuture;
use futures::stream;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use vega_conversation::agent::{
    PendingPermission, PendingScopePermission, PermissionQueue, PermissionQueueListener,
    run_thread_task_with_permission_sink,
};
use vega_conversation::threads;
use vega_conversation::types::{
    Approval, ApprovalAudit, ApprovalSource, BashStartClock, CommandScopeDecision,
    ConversationEvent, PermissionDecision, ToolCallStatus, ToolResult,
};
use vega_runtime::{ChatRequest, EventStream, Provider, ProviderEvent, StopReason, VegaError};
use vega_store::{Store, permissions, projects, tool_calls};
use vega_tools::{BashErrorCode, ToolCredentialPolicy, Tools};

const CALL_ID: &str = "scope-clock-call";
const TEST_TIMEOUT: Duration = Duration::from_secs(40);
const WAIT_FOR_PROCESS: Duration = Duration::from_secs(15);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let base = std::env::temp_dir().canonicalize().expect("temporary base");
        let path = base.join(format!("vega-scope-clock-{}", ulid::Ulid::generate()));
        DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .expect("create private test directory");
        Self(path.canonicalize().expect("canonical test directory"))
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct OfflineProvider {
    input: String,
    rounds: std::sync::atomic::AtomicUsize,
}

impl Provider for OfflineProvider {
    fn chat_stream(
        &self,
        _request: ChatRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        let first = self
            .rounds
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            == 0;
        let events = if first {
            vec![
                Ok(ProviderEvent::ToolUse {
                    id: CALL_ID.to_string(),
                    name: "bash".to_string(),
                    input_json: self.input.clone(),
                }),
                Ok(ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                }),
            ]
        } else {
            vec![
                Ok(ProviderEvent::TextDelta("Complete".to_string())),
                Ok(ProviderEvent::Done {
                    stop_reason: StopReason::End,
                }),
            ]
        };
        Box::pin(async move { Ok(Box::pin(stream::iter(events)) as EventStream) })
    }
}

enum Observation {
    Running(BashStartClock),
    Finished(ToolResult, Instant),
}

fn fail_early_observation(phase: &str, observation: Option<Observation>) -> ! {
    match observation {
        Some(Observation::Running(_)) => panic!("tool started running before {phase}"),
        Some(Observation::Finished(result, _)) => {
            let bash_error = [
                BashErrorCode::InvalidInput,
                BashErrorCode::ScopeMismatch,
                BashErrorCode::ScopeDenied,
                BashErrorCode::HardlinkPreflight,
                BashErrorCode::SandboxUnavailable,
                BashErrorCode::TempUnavailable,
                BashErrorCode::CleanupFailed,
                BashErrorCode::SpawnFailed,
                BashErrorCode::ProcessControlFailed,
                BashErrorCode::OutputFailed,
                BashErrorCode::OutputIncomplete,
                BashErrorCode::Cancelled,
                BashErrorCode::TimedOut,
            ]
            .into_iter()
            .find(|code| code.tool_output() == result.output)
            .map(BashErrorCode::as_str);
            panic!("tool finished before {phase}: result={result:?}, bash_error={bash_error:?}");
        }
        None => panic!("observation channel closed before {phase}"),
    }
}

#[derive(Clone, Copy)]
enum ScopeAction {
    Allow,
    Deny,
    TimeoutDecision,
    Cancel,
    ChangeDirectoryIdentity,
    HardlinkPreflight,
}

async fn ordinary_request(
    queue: &PermissionQueue,
    listener: &mut PermissionQueueListener,
) -> PendingPermission {
    loop {
        if let Some(pending) = queue.take_pending() {
            return pending;
        }
        assert!(listener.changed().await, "permission listener closed");
    }
}

async fn scope_request(
    queue: &PermissionQueue,
    listener: &mut PermissionQueueListener,
) -> PendingScopePermission {
    loop {
        if let Some(pending) = queue.take_pending_scope() {
            return pending;
        }
        assert!(listener.changed().await, "scope listener closed");
    }
}

async fn wait_for_file(path: &Path) {
    tokio::time::timeout(WAIT_FOR_PROCESS, async {
        while !fs::read(path).is_ok_and(|contents| contents == b"started") {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("shell did not produce its start marker");
}

async fn wait_for_clock(clock: &BashStartClock) -> Instant {
    tokio::time::timeout(WAIT_FOR_PROCESS, async {
        loop {
            if let Some(started) = clock.started_at() {
                break started;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("shell start clock was not set")
}

fn scope_state(store: &Store, request_id: &str) -> String {
    store
        .conn()
        .query_row(
            "SELECT state FROM command_scope_audits WHERE request_id = ?1",
            [request_id],
            |row| row.get(0),
        )
        .expect("scope audit")
}

fn shell_quote(path: &Path) -> String {
    let text = path.to_str().expect("UTF-8 test path");
    format!("'{}'", text.replace('\'', "'\\''"))
}

async fn exercise_scope(action: ScopeAction) {
    let temp = TestDirectory::new();
    let workspace = temp.0.join("workspace");
    let scope = temp.0.join("scope");
    let protected = temp.0.join("protected");
    fs::create_dir(&workspace).expect("workspace directory");
    fs::create_dir(&scope).expect("scope directory");
    fs::create_dir(&protected).expect("protected directory");
    let workspace = workspace.canonicalize().expect("canonical workspace");
    let scope = scope.canonicalize().expect("canonical scope");
    let store = Store::open(temp.0.join("vega.db")).expect("open store");
    store.migrate().expect("migrate store");
    let project = projects::create(
        store.conn(),
        workspace.to_str().expect("UTF-8 workspace"),
        "scope-clock",
        None,
    )
    .expect("create project");
    let thread = threads::create_thread(&store, &project.id, "offline-model", "confirm")
        .expect("create thread");
    let original_rules = permissions::list_exact(store.conn(), &project.id).expect("exact rules");
    let policy = ToolCredentialPolicy::new(
        protected,
        vec![],
        Arc::new(|_: &str| panic!("empty bindings must not read credentials")),
    )
    .expect("configure empty credential policy")
    .with_bindings_reader(Arc::new(|| Ok(vec![])));
    let tools = Tools::new(&workspace)
        .expect("workspace tools")
        .with_credential_policy(policy);
    let started_file = scope.join("started");
    let release_file = scope.join("release");
    let command = format!(
        "printf started > {} || exit $?; while [ ! -f {} ]; do /bin/sleep 0.05; done; printf finished",
        shell_quote(&started_file),
        shell_quote(&release_file),
    );
    let input = serde_json::json!({
        "command": command,
        "timeout": 20,
        "requested_scope": {
            "network": false,
            "write_roots": [scope.to_str().expect("UTF-8 scope")],
            "reason": "Write only within the isolated test directory"
        }
    })
    .to_string();
    let provider = OfflineProvider {
        input,
        rounds: std::sync::atomic::AtomicUsize::new(0),
    };
    let queue = PermissionQueue::new();
    let mut listener = queue.subscribe();
    let cancel = CancellationToken::new();
    let (observed_tx, mut observed_rx) = mpsc::unbounded_channel();

    let run = run_thread_task_with_permission_sink(
        &store,
        &provider,
        &tools,
        &thread.id,
        "Run the isolated command",
        "",
        cancel.clone(),
        &queue,
        |event| {
            match event {
                ConversationEvent::ToolCallRunning {
                    call_id,
                    bash_started_at,
                } if call_id == CALL_ID => {
                    observed_tx
                        .send(Observation::Running(
                            bash_started_at.as_ref().expect("bash start handle").clone(),
                        ))
                        .expect("running observation receiver");
                }
                ConversationEvent::ToolCallFinished { call_id, result } if call_id == CALL_ID => {
                    observed_tx
                        .send(Observation::Finished(result.clone(), Instant::now()))
                        .expect("terminal observation receiver");
                }
                _ => {}
            }
            Ok(())
        },
    );

    let drive = async {
        let pending = tokio::select! {
            biased;
            pending = ordinary_request(&queue, &mut listener) => pending,
            observation = observed_rx.recv() => {
                fail_early_observation("ordinary permission request", observation)
            }
        };
        let (request, mut lease) = pending.into_parts().expect("ordinary permission lease");
        assert_eq!(request.call_id, CALL_ID);
        assert_eq!(request.tool, "bash");
        assert_eq!(request.display_target, command);
        assert!(request.danger_rule_id.is_none());
        assert!(lease.respond(PermissionDecision::Once));

        let mut early_clock = None;
        let pending = loop {
            tokio::select! {
                biased;
                pending = scope_request(&queue, &mut listener) => break pending,
                observation = observed_rx.recv() => match observation {
                    Some(Observation::Running(clock)) => {
                        assert!(early_clock.replace(clock).is_none(), "duplicate running event");
                    }
                    other => fail_early_observation("scope permission request", other),
                }
            }
        };
        let (request, mut lease) = pending.into_parts();
        assert_eq!(request.call_id(), CALL_ID);
        assert_eq!(request.command(), command);
        assert_eq!(request.project_root(), workspace.to_str().unwrap());
        assert!(!request.scope().network());
        assert_eq!(
            request.scope().write_roots(),
            &[scope.to_str().unwrap().to_string()]
        );
        assert_eq!(request.directories().len(), 1);
        assert_eq!(request.directories()[0].path(), scope);
        assert!(request.bindings().is_empty());
        let request_id = request.request_id().to_string();

        let clock = match early_clock {
            Some(clock) => clock,
            None => match observed_rx.recv().await {
                Some(Observation::Running(clock)) => clock,
                other => fail_early_observation("scope approval", other),
            },
        };
        assert!(clock.started_at().is_none());
        assert!(!started_file.exists());
        assert_eq!(scope_state(&store, &request_id), "pending");
        let running = tool_calls::find_state(store.conn(), CALL_ID)
            .expect("running tool state")
            .expect("tool call");
        assert_eq!(
            ToolCallStatus::parse(&running.status),
            Some(ToolCallStatus::Running)
        );
        let approval = running.approval.expect("ordinary approval");
        let decoded = ApprovalAudit::from_json(&approval).expect("ordinary approval audit");
        assert_eq!(decoded.decision, Approval::Once);
        assert_eq!(decoded.source, ApprovalSource::User);

        tokio::time::sleep(Duration::from_millis(350)).await;
        assert!(
            clock.started_at().is_none(),
            "approval wait must not start the clock"
        );
        assert!(!started_file.exists());
        assert_eq!(scope_state(&store, &request_id), "pending");

        let release_time = match action {
            ScopeAction::Allow => {
                let approved_at = Instant::now();
                assert!(lease.respond(CommandScopeDecision::AllowOnce));
                wait_for_file(&started_file).await;
                let started = wait_for_clock(&clock).await;
                assert!(started >= approved_at);
                assert_eq!(fs::read(&started_file).expect("start marker"), b"started");
                assert!(!release_file.exists());
                assert_eq!(scope_state(&store, &request_id), "consumed");
                tokio::time::sleep(Duration::from_millis(200)).await;
                let elapsed = started.elapsed();
                fs::write(&release_file, b"release").expect("release shell");
                Some((started, elapsed))
            }
            ScopeAction::Deny => {
                assert!(lease.respond(CommandScopeDecision::Deny));
                None
            }
            ScopeAction::TimeoutDecision => {
                assert!(lease.respond(CommandScopeDecision::Timeout));
                None
            }
            ScopeAction::Cancel => {
                cancel.cancel();
                None
            }
            ScopeAction::ChangeDirectoryIdentity => {
                fs::rename(&scope, temp.0.join("original-scope"))
                    .expect("move original scope directory");
                fs::create_dir(&scope).expect("replace scope directory");
                assert!(lease.respond(CommandScopeDecision::AllowOnce));
                None
            }
            ScopeAction::HardlinkPreflight => {
                let source = workspace.join("preflight-source");
                fs::write(&source, b"isolated fixture").expect("create preflight source");
                fs::hard_link(&source, workspace.join("preflight-link"))
                    .expect("create preflight hardlink");
                assert!(lease.respond(CommandScopeDecision::AllowOnce));
                None
            }
        };

        let (result, finished_at) = match observed_rx.recv().await.expect("terminal event") {
            Observation::Finished(result, observed_at) => (result, observed_at),
            Observation::Running(_) => panic!("duplicate running event"),
        };
        let terminal = tool_calls::find_state(store.conn(), CALL_ID)
            .expect("terminal tool state")
            .expect("tool call");
        assert_eq!(terminal.approval.as_deref(), Some(approval.as_str()));
        assert_eq!(ToolCallStatus::parse(&terminal.status), Some(result.status));
        assert_eq!(terminal.exit_code, result.exit_code);
        assert_eq!(
            permissions::list_exact(store.conn(), &project.id).expect("exact rules"),
            original_rules
        );
        let saved_thread = threads::get_thread(&store, &thread.id).expect("saved thread");
        assert_eq!(saved_thread.permission_mode, thread.permission_mode);
        assert_eq!(saved_thread.permission_revision, thread.permission_revision);

        match action {
            ScopeAction::Allow => {
                let (started, elapsed_at_release) = release_time.expect("release timing");
                assert_eq!(result.status, ToolCallStatus::Success);
                assert_eq!(result.exit_code, Some(0));
                assert_eq!(result.output, "finished");
                let duration = result.duration_ms.expect("terminal shell duration");
                assert!(u128::from(duration) >= elapsed_at_release.as_millis());
                let observed_elapsed = finished_at.duration_since(started).as_millis();
                assert!(u128::from(duration).abs_diff(observed_elapsed) < 5_000);
                assert_eq!(terminal.duration_ms, Some(i64::try_from(duration).unwrap()));
                assert_eq!(scope_state(&store, &request_id), "consumed");
                assert_eq!(clock.started_at(), Some(started));
            }
            ScopeAction::Deny
            | ScopeAction::TimeoutDecision
            | ScopeAction::Cancel
            | ScopeAction::ChangeDirectoryIdentity
            | ScopeAction::HardlinkPreflight => {
                assert!(clock.started_at().is_none());
                assert!(!started_file.exists());
                assert!(result.duration_ms.is_none());
                if matches!(action, ScopeAction::HardlinkPreflight) {
                    assert_eq!(
                        result.output,
                        BashErrorCode::HardlinkPreflight.tool_output()
                    );
                }
                assert_eq!(
                    result.status,
                    if matches!(action, ScopeAction::Cancel) {
                        ToolCallStatus::Cancelled
                    } else {
                        ToolCallStatus::Failed
                    }
                );
                let expected_scope_state = match action {
                    ScopeAction::Deny => "denied",
                    ScopeAction::TimeoutDecision => "timeout",
                    ScopeAction::Cancel => "cancelled",
                    ScopeAction::ChangeDirectoryIdentity => "pending",
                    ScopeAction::HardlinkPreflight => "consumed",
                    ScopeAction::Allow => unreachable!(),
                };
                assert_eq!(scope_state(&store, &request_id), expected_scope_state);
            }
        }
    };

    tokio::pin!(run);
    tokio::pin!(drive);
    let deadline = tokio::time::sleep(TEST_TIMEOUT);
    tokio::pin!(deadline);
    let mut run_result = None;
    let driven = loop {
        tokio::select! {
            biased;
            result = std::panic::AssertUnwindSafe(drive.as_mut()).catch_unwind() => break result,
            _ = &mut deadline => {
                cancel.cancel();
                if run_result.is_none() {
                    let _ = tokio::time::timeout(Duration::from_secs(5), run.as_mut()).await;
                }
                if matches!(action, ScopeAction::Allow) {
                    let _ = fs::write(&release_file, b"release");
                }
                panic!("permission driver timed out; conversation result: {run_result:?}");
            }
            result = run.as_mut(), if run_result.is_none() => {
                run_result = Some(result);
            }
        }
    };
    if let Err(payload) = driven {
        cancel.cancel();
        if run_result.is_none() {
            let _ = tokio::time::timeout(Duration::from_secs(5), run.as_mut()).await;
        }
        if matches!(action, ScopeAction::Allow) {
            let _ = fs::write(&release_file, b"release");
        }
        std::panic::resume_unwind(payload);
    }
    let run_result = match run_result {
        Some(result) => result,
        None => match tokio::time::timeout_at(deadline.deadline(), run.as_mut()).await {
            Ok(result) => result,
            Err(_) => {
                cancel.cancel();
                let _ = tokio::time::timeout(Duration::from_secs(5), run.as_mut()).await;
                if matches!(action, ScopeAction::Allow) {
                    let _ = fs::write(&release_file, b"release");
                }
                panic!("conversation did not finish before the deadline");
            }
        },
    };
    let outcome = run_result.expect("conversation run");
    assert!(!outcome.failed);
    assert_eq!(outcome.interrupted, matches!(action, ScopeAction::Cancel));
    let retained_running = outcome.events.iter().find(|event| {
        matches!(event, ConversationEvent::ToolCallRunning { call_id, .. } if call_id == CALL_ID)
    });
    assert!(matches!(
        retained_running,
        Some(ConversationEvent::ToolCallRunning {
            bash_started_at: None,
            ..
        })
    ));
}

#[tokio::test]
async fn approved_scope_starts_the_clock_only_when_the_shell_spawns() {
    exercise_scope(ScopeAction::Allow).await;
}

#[tokio::test]
async fn denied_scope_never_starts_the_shell_clock() {
    exercise_scope(ScopeAction::Deny).await;
}

#[tokio::test]
async fn scope_timeout_decision_never_starts_the_shell_clock() {
    exercise_scope(ScopeAction::TimeoutDecision).await;
}

#[tokio::test]
async fn cancelled_scope_never_starts_the_shell_clock() {
    exercise_scope(ScopeAction::Cancel).await;
}

#[tokio::test]
async fn changed_scope_directory_never_starts_the_shell_clock() {
    exercise_scope(ScopeAction::ChangeDirectoryIdentity).await;
}

#[tokio::test]
async fn approved_scope_with_failed_preflight_never_starts_the_shell_clock() {
    exercise_scope(ScopeAction::HardlinkPreflight).await;
}
