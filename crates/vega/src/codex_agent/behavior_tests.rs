use super::*;

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Fixture {
    root: PathBuf,
    store: Store,
    writes: CodexWrites,
    thread: String,
}
impl Fixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let root =
            std::env::temp_dir().join(format!("vega-codex-behavior-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&root)?;
        let root = std::fs::canonicalize(root)?;
        let path = root.join("database.db");
        let store = Store::open(&path)?;
        store.migrate()?;
        let mut draft = vega_conversation::threads::draft_thread(None, "model", "confirm");
        draft.backend = TaskBackend::Codex;
        let snapshot = CodexExecutionSnapshot {
            profile: CodexProfileReference {
                id: "profile".into(),
                display_name: "Profile".into(),
            },
            adapter: CodexAdapterKind::CodexAcp,
            executable: "/mock-codex".into(),
            arguments: Vec::new(),
            adapter_version: "1".into(),
            codex_version: "1".into(),
            settings: CodexRunSettings {
                model: None,
                model_provider: None,
                reasoning_effort: None,
                sandbox_mode: CodexSandboxMode::WorkspaceWrite,
                approval_policy: CodexApprovalPolicy::OnRequest,
            },
            workspace: CodexWorkspaceSnapshot {
                project_id: None,
                worktree_id: Some("worktree".into()),
                canonical_working_directory: root.to_string_lossy().into_owned(),
                additional_directories: Vec::new(),
            },
        };
        let startup =
            vega_conversation::codex_tasks::prepare_codex_startup(&path, draft, snapshot)?;
        let thread = startup.thread.id;
        startup.persistence.session_outcome(
            CodexSessionOutcome::Confirmed {
                session_id: "session".into(),
            },
            unix_millis().unwrap(),
        )?;
        startup.persistence.create_messages(
            "question".into(),
            "user".into(),
            "assistant".into(),
            unix_millis().unwrap(),
        )?;
        let writes = CodexWrites::from_startup(startup.persistence);
        Ok(Self {
            root,
            store,
            writes,
            thread,
        })
    }
    fn activities(
        &self,
        sender: &std_mpsc::SyncSender<AgentUpdate>,
    ) -> HashMap<String, CodexToolActivity> {
        let mut activities = HashMap::new();
        let external = ensure_codex_tool_activity(
            &self.writes,
            &self.thread,
            "session",
            "assistant",
            &serde_json::json!({"toolCallId":"external", "kind":"read", "rawInput":{}}),
            &mut activities,
            sender,
        )
        .unwrap();
        activities.get_mut(&external).unwrap().permission_pending = true;
        activities
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn permission(activity: &CodexToolActivity) -> PermissionRequest {
    PermissionRequest {
        call_id: activity.call_id.clone(),
        tool: "codex_acp".into(),
        display_target: activity.identity.permission_target(),
        danger_rule_id: None,
        danger_reason: None,
        external: None,
        acp_options: Some(vec![PermissionOptionChoice {
            option_id: "allow".into(),
            name: "Allow".into(),
            kind: Some("allow_once".into()),
        }]),
    }
}

#[tokio::test]
async fn prompt_finish_waits_for_selected_permission_before_installing_user_audit() -> TestResult {
    let fixture = Fixture::new()?;
    let (sender, _events) = std_mpsc::sync_channel(32);
    let mut activities = fixture.activities(&sender);
    let external = activities.keys().next().unwrap().clone();
    let call = activities[&external].call_id.clone();
    let queue = PermissionQueue::new();
    let _listener = queue.subscribe();
    let cancel = CancellationToken::new();
    let selection =
        select_codex_permission(&queue, permission(&activities[&external]), cancel.clone());
    tokio::pin!(selection);
    std::future::poll_fn(|cx| {
        assert!(std::future::Future::poll(selection.as_mut(), cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    let pending = queue.take_pending().unwrap();
    let (_, mut lease) = pending.into_parts().unwrap();
    assert!(lease.respond(PermissionDecision::AcpOption {
        option_id: "allow".into()
    }));
    let (option, kind) = selection.await.unwrap();
    assert_eq!(option, "allow");
    apply_codex_tool_status(
        &fixture.writes,
        &external,
        Some("completed"),
        &mut activities,
        &sender,
    );
    assert_eq!(
        vega_store::tool_calls::find_state(fixture.store.conn(), &call)?
            .unwrap()
            .status,
        "pending_approval"
    );
    let (resolved, mut receiver) = tokio_mpsc::unbounded_channel();
    let mut tasks = tokio::task::JoinSet::new();
    tasks.spawn(async move {
        tokio::task::yield_now().await;
        resolved
            .send(CodexPermissionResolution::Selected {
                external_id: external,
                kind,
            })
            .unwrap();
    });
    settle_codex_permissions(
        &fixture.writes,
        &cancel,
        &queue,
        &mut tasks,
        &mut receiver,
        &mut activities,
        &sender,
    )
    .await;
    finalize_codex_tool_activities(
        &fixture.writes,
        &fixture.thread,
        "assistant",
        &mut activities,
        ToolCallStatus::Failed,
        &sender,
    );
    let state = vega_store::tool_calls::find_state(fixture.store.conn(), &call)?.unwrap();
    assert_eq!(state.status, "success");
    let audit: Value = serde_json::from_str(state.approval.as_deref().unwrap())?;
    assert_eq!(audit["source"], "user");
    assert!(fixture.writes.healthy());
    Ok(())
}

#[tokio::test]
async fn prompt_finish_without_selection_never_fabricates_user_approval() -> TestResult {
    let fixture = Fixture::new()?;
    let (sender, _events) = std_mpsc::sync_channel(32);
    let mut activities = fixture.activities(&sender);
    let external = activities.keys().next().unwrap().clone();
    let call = activities[&external].call_id.clone();
    let queue = PermissionQueue::new();
    let _listener = queue.subscribe();
    let cancel = CancellationToken::new();
    let request = permission(&activities[&external]);
    let (resolved, mut receiver) = tokio_mpsc::unbounded_channel();
    let mut tasks = tokio::task::JoinSet::new();
    let worker_queue = queue.clone();
    let worker_cancel = cancel.clone();
    tasks.spawn(async move {
        let selected = select_codex_permission(&worker_queue, request, worker_cancel).await;
        assert!(selected.is_none());
        resolved
            .send(CodexPermissionResolution::TimedOut {
                external_id: external,
            })
            .unwrap();
    });
    tokio::task::yield_now().await;
    settle_codex_permissions(
        &fixture.writes,
        &cancel,
        &queue,
        &mut tasks,
        &mut receiver,
        &mut activities,
        &sender,
    )
    .await;
    let state = vega_store::tool_calls::find_state(fixture.store.conn(), &call)?.unwrap();
    assert_eq!(state.status, "rejected");
    let audit: Value = serde_json::from_str(state.approval.as_deref().unwrap())?;
    assert_eq!(audit["source"], "timeout");
    Ok(())
}

#[tokio::test]
async fn unoffered_permission_option_is_rejected_by_real_selection_path() -> TestResult {
    let fixture = Fixture::new()?;
    let (sender, _events) = std_mpsc::sync_channel(32);
    let activities = fixture.activities(&sender);
    let activity = activities.values().next().unwrap();
    let queue = PermissionQueue::new();
    let _listener = queue.subscribe();
    let selection = select_codex_permission(&queue, permission(activity), CancellationToken::new());
    tokio::pin!(selection);
    std::future::poll_fn(|cx| {
        assert!(std::future::Future::poll(selection.as_mut(), cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    let (_, mut lease) = queue.take_pending().unwrap().into_parts().unwrap();
    assert!(lease.respond(PermissionDecision::AcpOption {
        option_id: "invented".into()
    }));
    assert!(selection.await.is_none());
    Ok(())
}

#[test]
fn user_denial_does_not_become_approval_when_actual_terminal_arrives_first() -> TestResult {
    let fixture = Fixture::new()?;
    let (sender, _events) = std_mpsc::sync_channel(32);
    let mut activities = fixture.activities(&sender);
    let external = activities.keys().next().unwrap().clone();
    let call = activities[&external].call_id.clone();
    apply_codex_tool_status(
        &fixture.writes,
        &external,
        Some("completed"),
        &mut activities,
        &sender,
    );
    apply_codex_permission_resolution(
        &fixture.writes,
        CodexPermissionResolution::Selected {
            external_id: external,
            kind: Some("reject_once".into()),
        },
        &mut activities,
        &sender,
    );
    let state = vega_store::tool_calls::find_state(fixture.store.conn(), &call)?.unwrap();
    assert_eq!(state.status, "success");
    let audit: Value = serde_json::from_str(state.approval.as_deref().unwrap())?;
    assert_eq!(audit["source"], "user");
    assert_eq!(audit["decision"], "deny");
    assert!(fixture.writes.healthy());
    Ok(())
}

#[test]
fn failed_tool_sql_is_not_replayed_by_prompt_finalization() -> TestResult {
    let fixture = Fixture::new()?;
    let (sender, _events) = std_mpsc::sync_channel(32);
    let mut activities = fixture.activities(&sender);
    let external = activities.keys().next().unwrap().clone();
    let activity = activities.get_mut(&external).unwrap();
    activity.permission_pending = false;
    fixture.store.conn().execute_batch("CREATE TRIGGER fail_codex_terminal BEFORE UPDATE ON tool_calls WHEN NEW.status='success' BEGIN SELECT RAISE(ABORT,'injected tool failure'); END;")?;
    finish_codex_tool(
        &fixture.writes,
        activity,
        ToolCallStatus::Success,
        CodexTerminalReason::ToolCompleted,
        &sender,
    );
    assert!(activity.persistence_uncertain);
    let call = activity.call_id.clone();
    fixture
        .store
        .conn()
        .execute_batch("DROP TRIGGER fail_codex_terminal;")?;
    finalize_codex_tool_activities(
        &fixture.writes,
        &fixture.thread,
        "assistant",
        &mut activities,
        ToolCallStatus::Failed,
        &sender,
    );
    assert_eq!(
        vega_store::tool_calls::find_state(fixture.store.conn(), &call)?
            .unwrap()
            .status,
        "pending_approval"
    );
    assert!(!fixture.writes.healthy());
    Ok(())
}

fn pending_startup(
    fixture: &Fixture,
) -> Result<vega_conversation::codex_tasks::CodexStartup, String> {
    let snapshot =
        vega_conversation::codex_tasks::codex_task_identity(&fixture.store, &fixture.thread)
            .map_err(|error| error.to_string())?
            .ok_or("Codex fixture missing")?
            .snapshot;
    let mut draft = vega_conversation::threads::draft_thread(None, "model", "confirm");
    draft.backend = TaskBackend::Codex;
    vega_conversation::codex_tasks::prepare_codex_startup(
        &fixture.root.join("database.db"),
        draft,
        snapshot,
    )
}

#[test]
fn cancelled_request_and_started_writes_drop_do_not_wait_for_sql_writer() -> TestResult {
    for external_started in [false, true] {
        let fixture = Fixture::new()?;
        let startup = pending_startup(&fixture)?;
        let thread = startup.thread.id.clone();
        fixture.store.conn().execute_batch("BEGIN IMMEDIATE;")?;
        let (finished, returned) = std_mpsc::sync_channel(1);
        let dropping = std::thread::spawn(move || {
            if external_started {
                let writes = CodexWrites::from_startup(startup.persistence);
                writes.mark_external_started();
                drop(writes);
            } else {
                let cancel = CancellationToken::new();
                cancel.cancel();
                let (sender, _events) = std_mpsc::sync_channel(1);
                drop(CodexWorkerRequest {
                    persistence: startup.persistence,
                    thread: startup.thread,
                    snapshot: startup.snapshot,
                    prompt: "question".into(),
                    permission_queue: PermissionQueue::new(),
                    cancel,
                    sender,
                });
            }
            finished.send(()).unwrap();
        });
        let before_release = returned.recv_timeout(std::time::Duration::from_millis(100));
        fixture.store.conn().execute_batch("COMMIT;")?;
        dropping.join().unwrap();
        before_release?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let state =
                vega_conversation::codex_tasks::codex_task_identity(&fixture.store, &thread)?
                    .unwrap()
                    .session_creation;
            if !matches!(state, CodexSessionCreationState::Intent { .. }) {
                assert!(matches!(
                    (external_started, state),
                    (false, CodexSessionCreationState::DefinitivelyFailed { .. })
                        | (true, CodexSessionCreationState::Uncertain { .. })
                ));
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    Ok(())
}

#[test]
fn cancelled_worker_returns_and_publishes_finished_only_after_settlement_ack() -> TestResult {
    let fixture = Fixture::new()?;
    let startup = pending_startup(&fixture)?;
    let thread = startup.thread.id.clone();
    let cancel = CancellationToken::new();
    cancel.cancel();
    let (sender, events) = std_mpsc::sync_channel(8);
    let request = CodexWorkerRequest {
        persistence: startup.persistence,
        thread: startup.thread,
        snapshot: startup.snapshot,
        prompt: "question".into(),
        permission_queue: PermissionQueue::new(),
        cancel,
        sender: sender.clone(),
    };
    fixture.store.conn().execute_batch("BEGIN IMMEDIATE;")?;
    let (begun, began) = std_mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        begun.send(()).unwrap();
        let success = run_codex_agent_worker(request);
        sender
            .send(AgentUpdate::Finished {
                success,
                reference_failure: None,
                credential_failure: false,
            })
            .unwrap();
    });
    began.recv_timeout(std::time::Duration::from_secs(5))?;
    let before_release = events.recv_timeout(std::time::Duration::from_millis(100));
    fixture.store.conn().execute_batch("COMMIT;")?;
    assert!(matches!(
        before_release,
        Err(std_mpsc::RecvTimeoutError::Timeout)
    ));
    assert!(matches!(
        events.recv_timeout(std::time::Duration::from_secs(5))?,
        AgentUpdate::Finished { success: false, .. }
    ));
    assert!(matches!(
        vega_conversation::codex_tasks::codex_task_identity(&fixture.store, &thread)?
            .unwrap()
            .session_creation,
        CodexSessionCreationState::DefinitivelyFailed {
            code: CodexSessionFailureCode::ProcessStartFailed,
            ..
        }
    ));
    worker.join().unwrap();
    Ok(())
}

#[test]
fn explicit_worker_finish_preserves_business_failure_and_reports_settlement_failure() -> TestResult
{
    for (success, unhealthy, sql_failure) in [
        (true, false, false),
        (false, false, false),
        (true, true, false),
        (true, false, true),
    ] {
        let fixture = Fixture::new()?;
        let startup = pending_startup(&fixture)?;
        let thread = startup.thread.id.clone();
        let writes = CodexWrites::from_startup(startup.persistence);
        writes.mark_external_started();
        if unhealthy {
            writes.fail("unknown prior receipt; no replay");
        }
        if sql_failure {
            fixture.store.conn().execute_batch("CREATE TRIGGER fail_worker_settlement BEFORE UPDATE ON codex_session_creations BEGIN SELECT RAISE(ABORT,'injected worker settlement failure'); END;")?;
        }
        assert_eq!(
            writes.finish_worker(success),
            success && !unhealthy && !sql_failure
        );
        let state = vega_conversation::codex_tasks::codex_task_identity(&fixture.store, &thread)?
            .unwrap()
            .session_creation;
        assert!(matches!(
            (sql_failure, state),
            (true, CodexSessionCreationState::Intent { .. })
                | (
                    false,
                    CodexSessionCreationState::Uncertain {
                        code: CodexSessionUncertaintyCode::OutcomeUnknown,
                        ..
                    }
                )
        ));
    }
    Ok(())
}
