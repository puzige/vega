use super::*;
use crate::types::{RunFailureKind, Thread, approval_audit_to_runtime};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use vega_runtime::{ChatRequest, ChatRole, EventStream, ProviderEvent, StopReason};
use vega_store::run_diagnostics::{self, DiagnosticPhase, DiagnosticState};

const MODEL: &str = "startup-model";
const SPAWN_CALL: &str = "spawn_receipt_call";
const CHILD_TASK: &str = "return the child result";
const PARENT_TASK: &str = "delegate the scoped work and collect its result";
const PRIVATE_SENTINEL: &str = "/private/owner-secret-sentinel";

struct Fixture {
    directory: PathBuf,
    database: PathBuf,
    store: Store,
    thread: Thread,
    tools: vega_tools::Tools,
}

impl Fixture {
    fn new() -> Self {
        let directory =
            std::env::temp_dir().join(format!("vega-startup-receipts-{}", ulid::Ulid::generate()));
        fs::create_dir(&directory).expect("owned fixture directory");
        let database = directory.join("conversation.sqlite");
        let store = Store::open(&database).expect("fixture store");
        store.migrate().expect("fixture migrations");
        let thread = crate::threads::create_standalone_thread(&store, MODEL, "confirm")
            .expect("parent thread");
        let tools = vega_tools::Tools::new(&directory).expect("owned workspace");
        Self {
            directory,
            database,
            store,
            thread,
            tools,
        }
    }

    fn provider(&self, wait: bool) -> ScriptProvider {
        ScriptProvider {
            database: self.database.clone(),
            parent_id: self.thread.id.clone(),
            wait,
            parent_calls: AtomicUsize::new(0),
            child_calls: AtomicUsize::new(0),
            child_receipts: Mutex::new(Vec::new()),
        }
    }

    async fn run(
        &self,
        provider: &ScriptProvider,
        host: Arc<RecordingHost>,
        cancel: CancellationToken,
        reader: Option<vega_runtime::CredentialReader>,
        events: &mut Vec<ConversationEvent>,
    ) -> Result<super::super::ConversationRun, ConversationError> {
        let mut config = PersistenceActorConfig::default().with_subagent_host(host);
        if let Some(reader) = reader {
            config = config.with_owner_credential_reader(reader);
        }
        tokio::time::timeout(
            Duration::from_secs(20),
            super::super::run_thread_task_with_images_reasoning_and_mcp(
                &self.store,
                provider,
                &self.tools,
                &self.thread.id,
                PARENT_TASK,
                "Complete the delegated task.",
                cancel,
                &PermissionQueue::new(),
                |event| {
                    events.push(event.clone());
                    Ok(())
                },
                config,
                None,
                None,
                None,
                Vec::new(),
                Vec::new(),
            ),
        )
        .await
        .expect("conversation must drain")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

struct ScriptProvider {
    database: PathBuf,
    parent_id: String,
    wait: bool,
    parent_calls: AtomicUsize,
    child_calls: AtomicUsize,
    child_receipts: Mutex<Vec<(String, bool)>>,
}

fn response(mut events: Vec<ProviderEvent>, reason: StopReason) -> Vec<ProviderEvent> {
    events.push(ProviderEvent::Usage {
        input: 5,
        output: 3,
        cache_read: Some(0),
        cache_write: Some(0),
    });
    events.push(ProviderEvent::Done {
        stop_reason: reason,
    });
    events
}

impl Provider for ScriptProvider {
    fn chat_stream(
        &self,
        request: ChatRequest,
        _: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        let child = request
            .messages
            .iter()
            .any(|message| message.role == ChatRole::User && message.content == CHILD_TASK);
        let events = if child {
            self.child_calls.fetch_add(1, Ordering::SeqCst);
            let store = Store::open_read_only(&self.database).expect("provider receipt read");
            let state = tool_calls::find_state(store.conn(), SPAWN_CALL)
                .expect("spawn state")
                .expect("spawn owner");
            let children = vega_store::subagents::list(store.conn(), &self.parent_id, None)
                .expect("delegated identity");
            let receipt = state
                .output_text
                .as_deref()
                .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok());
            let same_identity = children.len() == 1
                && state.thread_id == self.parent_id
                && receipt.as_ref().is_some_and(|value| {
                    value["agent"]["agent_id"] == children[0].child_thread_id
                        && value["agent"]["parent_thread_id"] == self.parent_id
                        && value["agent"]["parent_message_id"] == children[0].parent_message_id
                        && value["agent"]["message_id"] == children[0].child_message_id
                });
            self.child_receipts
                .lock()
                .expect("receipt observations")
                .push((state.status, same_identity));
            response(
                vec![ProviderEvent::TextDelta("child complete".into())],
                StopReason::End,
            )
        } else {
            let call = self.parent_calls.fetch_add(1, Ordering::SeqCst);
            if call == 0 {
                response(
                    vec![ProviderEvent::ToolUse {
                        id: SPAWN_CALL.into(),
                        name: "spawn_agent".into(),
                        input_json: serde_json::json!({"name":"worker","task":CHILD_TASK})
                            .to_string(),
                    }],
                    StopReason::ToolUse,
                )
            } else if call == 1 && self.wait {
                let store = Store::open_read_only(&self.database).expect("wait receipt read");
                let child = vega_store::subagents::list(store.conn(), &self.parent_id, None)
                    .expect("wait child")
                    .remove(0);
                response(
                    vec![ProviderEvent::ToolUse {
                        id: "wait_receipt_call".into(),
                        name: "wait_agent".into(),
                        input_json: serde_json::json!({
                            "agent_ids":[child.child_thread_id],"timeout_ms":60000
                        })
                        .to_string(),
                    }],
                    StopReason::ToolUse,
                )
            } else {
                response(
                    vec![ProviderEvent::TextDelta("parent complete".into())],
                    StopReason::End,
                )
            }
        };
        async move {
            Ok(Box::pin(futures::stream::iter(events.into_iter().map(Ok))) as EventStream)
        }
        .boxed()
    }
}

#[derive(Clone, Copy)]
enum AdmissionMode {
    Accept,
    Reject,
    Stop,
}

struct RecordingHost {
    mode: AdmissionMode,
    parent_cancel: CancellationToken,
    admitted: AtomicBool,
    events: Mutex<Vec<ConversationEvent>>,
    finished: Mutex<Vec<SubagentSnapshot>>,
}

impl RecordingHost {
    fn new(mode: AdmissionMode, parent_cancel: CancellationToken) -> Arc<Self> {
        Arc::new(Self {
            mode,
            parent_cancel,
            admitted: AtomicBool::new(false),
            events: Mutex::new(Vec::new()),
            finished: Mutex::new(Vec::new()),
        })
    }
}

impl SubagentHost for RecordingHost {
    fn admit(&self, _: SubagentAdmission) -> BoxFuture<'static, Result<(), VegaError>> {
        self.admitted.store(true, Ordering::SeqCst);
        let result = match self.mode {
            AdmissionMode::Accept => Ok(()),
            AdmissionMode::Reject => Err(failure("conversation owner unavailable")),
            AdmissionMode::Stop => {
                self.parent_cancel.cancel();
                Ok(())
            }
        };
        async move { result }.boxed()
    }

    fn event(&self, _: &str, _: &str, event: &ConversationEvent) -> Result<(), VegaError> {
        self.events.lock().expect("host events").push(event.clone());
        Ok(())
    }

    fn updated(&self, _: SubagentSnapshot) -> Result<(), VegaError> {
        Ok(())
    }

    fn finished(&self, snapshot: SubagentSnapshot, _: bool) -> Result<(), VegaError> {
        self.finished.lock().expect("host terminals").push(snapshot);
        Ok(())
    }
}

fn preparation_reader(
    database: &Path,
    host: Arc<RecordingHost>,
) -> (vega_runtime::CredentialReader, Arc<Mutex<Vec<String>>>) {
    let database = database.to_path_buf();
    let executor = std::thread::current().id();
    let observed = Arc::new(Mutex::new(Vec::new()));
    let recorded = observed.clone();
    let reader = Arc::new(move || {
        if host.admitted.load(Ordering::SeqCst) && std::thread::current().id() != executor {
            let store = Store::open_read_only(&database).map_err(|_| ())?;
            let state = tool_calls::find_state(store.conn(), SPAWN_CALL)
                .map_err(|_| ())?
                .ok_or(())?;
            recorded.lock().map_err(|_| ())?.push(state.status);
        }
        Ok(Vec::new())
    });
    (reader, observed)
}

fn parent_run_id(events: &[ConversationEvent]) -> String {
    events
        .iter()
        .find_map(|event| match event {
            ConversationEvent::MessageStarted { message_id, .. } => Some(message_id.clone()),
            _ => None,
        })
        .expect("parent assistant")
}

fn assert_drained(fixture: &Fixture, host: &RecordingHost, state: SubagentState) {
    let children = vega_store::subagents::list(fixture.store.conn(), &fixture.thread.id, None)
        .expect("durable children");
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].state, state.as_str());
    assert!(!crate::subagents::is_live(
        &fixture.store,
        &children[0].child_thread_id
    ));
    assert_eq!(host.finished.lock().expect("host terminals").len(), 1);
    assert_eq!(
        vega_store::subagents::active_count(
            fixture.store.conn(),
            &fixture.thread.id,
            &children[0].parent_message_id
        )
        .expect("active delegated generation count"),
        0
    );
}

fn assert_safe_failure(
    fixture: &Fixture,
    events: &[ConversationEvent],
    reason: &str,
    sqlite: Option<(i64, i64)>,
) {
    let run = parent_run_id(events);
    let error = events
        .iter()
        .rev()
        .find_map(|event| match event {
            ConversationEvent::Error { error, .. } => Some(error),
            _ => None,
        })
        .expect("live failure");
    let kind = RunFailureKind::from_runtime(error);
    assert_ne!(kind, RunFailureKind::Runtime);
    let message = kind.message();
    assert!(message.to_ascii_lowercase().contains("retry"));
    for text in [message, error.to_string(), format!("{error:?}")] {
        assert!(!text.contains(PRIVATE_SENTINEL));
        assert!(!text.contains(CHILD_TASK));
        assert!(!text.contains("UPDATE tool_calls"));
    }
    let export = run_diagnostics::export_run(fixture.store.conn(), &fixture.thread.id, &run)
        .expect("failure export");
    assert!(!export.contains(PRIVATE_SENTINEL));
    assert!(!export.contains(CHILD_TASK));
    assert!(!export.contains(PARENT_TASK));
    let json: serde_json::Value = serde_json::from_str(&export).expect("diagnostic JSON");
    let records = json["events"].as_array().expect("diagnostic events");
    for phase in ["run", "tool"] {
        let failed = records
            .iter()
            .find(|event| event["phase"] == phase && event["state"] == "failed")
            .expect("failed diagnostic closes its attempt");
        assert_eq!(failed["metrics"]["runtime_stage"], "tool_terminal");
        assert_eq!(failed["metrics"]["runtime_reason"], reason);
        if phase == "tool" {
            assert_eq!(failed["tool_call_id"], SPAWN_CALL);
        }
        match sqlite {
            Some((primary, extended)) => {
                assert_eq!(failed["metrics"]["sqlite_code"], primary);
                assert_eq!(failed["metrics"]["sqlite_extended_code"], extended);
            }
            None => {
                assert!(failed["metrics"]["sqlite_code"].is_null());
                assert!(failed["metrics"]["sqlite_extended_code"].is_null());
            }
        }
        assert!(failed["metrics"]["io_kind"].is_null());
        let started = records.iter().find(|event| {
            event["attempt_id"] == failed["attempt_id"] && event["state"] == "started"
        });
        assert!(started.is_some());
    }
    let reopened = Store::open(&fixture.database).expect("reopen store");
    crate::history::restart_history_page(&reopened, &fixture.thread.id, 100)
        .expect("reopen history");
    let restored =
        run_diagnostics::classified_failure_message_ids(reopened.conn(), &fixture.thread.id)
            .expect("restored failure owners");
    let metrics = &restored
        .iter()
        .find(|(owner, _)| owner == &run)
        .expect("original failed assistant ownership")
        .1;
    assert_eq!(RunFailureKind::from_diagnostic(metrics), kind);
    let restored_json = serde_json::to_value(metrics).expect("restored metadata");
    assert_eq!(restored_json["runtime_stage"], "tool_terminal");
    assert_eq!(restored_json["runtime_reason"], reason);
    let reopened_export = run_diagnostics::export_run(reopened.conn(), &fixture.thread.id, &run)
        .expect("reopened export");
    assert_eq!(reopened_export, export);
}

fn readonly_audit() -> ApprovalAudit {
    ApprovalAudit {
        decision: Approval::Once,
        note: None,
        source: ApprovalSource::ReadonlyTool,
        danger: None,
    }
}

#[tokio::test(flavor = "current_thread")]
async fn credential_loading_does_not_lock_parent_receipt_persistence() {
    let fixture = Fixture::new();
    let parent_message = ulid::Ulid::generate().to_string();
    messages::insert(
        fixture.store.conn(),
        &messages::MessageRow {
            id: parent_message.clone(),
            thread_id: fixture.thread.id.clone(),
            seq: 1,
            role: "assistant".into(),
            kind: "text".into(),
            content: String::new(),
            status: "streaming".into(),
            created_at: 1,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .expect("parent assistant");
    let snapshot = SubagentSnapshot {
        agent_id: ulid::Ulid::generate().to_string(),
        parent_thread_id: fixture.thread.id.clone(),
        parent_message_id: parent_message.clone(),
        message_id: ulid::Ulid::generate().to_string(),
        name: "worker".into(),
        state: SubagentState::Running,
        result: String::new(),
        truncated: false,
    };
    let settings = ChildSettings {
        model: MODEL.into(),
        system_prompt: "Complete the delegated task.".into(),
        authority: DelegatedAuthority {
            run_mode: RuntimeRunMode::Execute,
            permission_mode: RuntimePermissionMode::Confirm,
            exact_rules: Vec::new(),
        },
        pricing_catalog: None,
        reasoning: None,
        context_budget: None,
        automation_claim: None,
        automation_hook: None,
        owner_credential_reader: None,
        turn_limit: 10,
    };
    let (_, user_id) = create_child(
        fixture.database.clone(),
        snapshot.clone(),
        CHILD_TASK.into(),
        SPAWN_CALL.into(),
        settings,
        CancellationToken::new(),
    )
    .await
    .expect("real child creation");
    let call = vega_runtime::RuntimeToolCall {
        id: SPAWN_CALL.into(),
        name: "spawn_agent".into(),
        input_json: serde_json::json!({"name":"worker","task":CHILD_TASK}).to_string(),
    };
    let mut sequence = 1;
    for event in [
        RuntimeEvent::ToolCallProposed(call),
        RuntimeEvent::ToolCallApproved {
            call_id: SPAWN_CALL.into(),
            audit: approval_audit_to_runtime(&readonly_audit()),
            remember_rule: None,
        },
        RuntimeEvent::ToolCallRunning {
            call_id: SPAWN_CALL.into(),
        },
    ] {
        persist_runtime_event(
            &fixture.store,
            "",
            &fixture.thread.id,
            &parent_message,
            MODEL,
            false,
            "",
            &mut sequence,
            &event,
            None,
        )
        .expect("real parent lifecycle");
    }
    let (entered, waiting) = oneshot::channel();
    let entered = Mutex::new(Some(entered));
    let (release, released) = std::sync::mpsc::channel();
    let released = Mutex::new(released);
    let reader: vega_runtime::CredentialReader = Arc::new(move || {
        if let Some(entered) = entered.lock().map_err(|_| ())?.take() {
            entered.send(()).map_err(|_| ())?;
        }
        released
            .lock()
            .map_err(|_| ())?
            .recv_timeout(Duration::from_secs(20))
            .map_err(|_| ())?;
        Ok(Vec::new())
    });
    let config = PersistenceActorConfig {
        delegated: true,
        delegated_parent: Some((fixture.thread.id.clone(), parent_message.clone())),
        delegated_message_id: Some(snapshot.message_id.clone()),
        owner_credential_reader: Some(reader),
        ..Default::default()
    };
    let database = fixture.database.clone();
    let child_id = snapshot.agent_id.clone();
    let assistant_id = snapshot.message_id.clone();
    let preparation = tokio::task::spawn_blocking(move || {
        prepare_run_with_images_and_reasoning(
            database,
            child_id,
            CHILD_TASK.into(),
            "Complete the delegated task.".into(),
            user_id,
            assistant_id,
            config,
            true,
            None,
            None,
            Vec::new(),
        )
        .map(|_| ())
    });
    tokio::time::timeout(Duration::from_secs(10), waiting)
        .await
        .expect("preparation reached its credential boundary")
        .expect("credential reader entered");
    let receipt = serde_json::json!({"agent":snapshot}).to_string();
    let terminal = RuntimeEvent::ToolCallFinished(vega_runtime::RuntimeToolResult {
        call_id: SPAWN_CALL.into(),
        output: receipt.clone(),
        status: RuntimeToolStatus::Success,
        reused: false,
        exit_code: None,
        duration_ms: None,
        truncated: Some(false),
        approval: Some(approval_audit_to_runtime(&readonly_audit())),
        remember_rule: None,
    });
    let parent_result = persist_runtime_event(
        &fixture.store,
        "",
        &fixture.thread.id,
        &parent_message,
        MODEL,
        false,
        "",
        &mut sequence,
        &terminal,
        None,
    );
    let sqlite_codes = if parent_result.is_err() {
        let typed_result = tool_calls::finish(
            fixture.store.conn(),
            tool_calls::FinishToolCall {
                id: SPAWN_CALL,
                status: "success",
                output_text: &receipt,
                exit_code: None,
                duration_ms: None,
                finished_at: 2,
            },
        );
        match typed_result {
            Err(tool_calls::ToolCallTransitionError::Store(error)) => error
                .sqlite_error()
                .map(|error| (error.extended_code & 255, error.extended_code)),
            _ => None,
        }
    } else {
        None
    };
    let before_release = tool_calls::find_state(fixture.store.conn(), SPAWN_CALL)
        .expect("parent state before release")
        .expect("parent call exists");
    release.send(()).expect("release credential read");
    preparation
        .await
        .expect("preparation worker")
        .expect("child preparation commits after release");
    assert!(
        parent_result.is_ok(),
        "parent receipt must persist during credential loading; runtime_stage=tool_terminal sqlite_codes={sqlite_codes:?}"
    );
    assert_eq!(before_release.status, "success");
    assert_eq!(
        before_release.output_text.as_deref(),
        Some(receipt.as_str())
    );
    assert!(
        messages::find(fixture.store.conn(), &snapshot.message_id)
            .expect("child assistant read")
            .is_some()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn child_starts_only_after_its_spawn_receipt_is_durable() {
    let fixture = Fixture::new();
    let provider = fixture.provider(true);
    let cancel = CancellationToken::new();
    let host = RecordingHost::new(AdmissionMode::Accept, cancel.clone());
    let (reader, preparation_receipts) = preparation_reader(&fixture.database, host.clone());
    let mut events = Vec::new();
    let run = fixture
        .run(&provider, host.clone(), cancel, Some(reader), &mut events)
        .await
        .expect("parent run");
    assert!(!run.failed && !run.interrupted);
    assert_eq!(run.content, "parent complete");
    assert_eq!(provider.child_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        *preparation_receipts.lock().expect("preparation receipts"),
        vec!["success".to_string()]
    );
    assert_eq!(
        *provider.child_receipts.lock().expect("provider receipts"),
        vec![("success".to_string(), true)]
    );
    assert_drained(&fixture, &host, SubagentState::Completed);
    let reopened = Store::open(&fixture.database).expect("reopen successful run");
    let history = crate::history::restart_history_page(&reopened, &fixture.thread.id, 100)
        .expect("parent history");
    assert_eq!(history.subagents.len(), 1);
    assert_eq!(history.subagents[0].result, "child complete");
    assert_eq!(history.subagents[0].state, SubagentState::Completed);
}

#[tokio::test(flavor = "current_thread")]
async fn rejected_spawn_storage_closes_diagnostics_without_starting_child() {
    let fixture = Fixture::new();
    fixture
        .store
        .conn()
        .execute_batch(&format!(
            "CREATE TRIGGER reject_spawn_receipt BEFORE UPDATE OF status ON tool_calls
             WHEN OLD.id='{SPAWN_CALL}' AND NEW.status='success'
             BEGIN SELECT RAISE(ABORT,'{PRIVATE_SENTINEL}'); END;"
        ))
        .expect("real terminal storage fault");
    let provider = fixture.provider(false);
    let cancel = CancellationToken::new();
    let host = RecordingHost::new(AdmissionMode::Accept, cancel.clone());
    let (reader, preparation) = preparation_reader(&fixture.database, host.clone());
    let mut events = Vec::new();
    let result = fixture
        .run(&provider, host.clone(), cancel, Some(reader), &mut events)
        .await;
    assert!(result.is_err());
    assert_safe_failure(&fixture, &events, "database", Some((19, 1811)));
    assert_eq!(provider.child_calls.load(Ordering::SeqCst), 0);
    assert!(
        preparation
            .lock()
            .expect("preparation observations")
            .is_empty()
    );
    assert!(host.events.lock().expect("child events").is_empty());
    assert_drained(&fixture, &host, SubagentState::Interrupted);
    let children = vega_store::subagents::list(fixture.store.conn(), &fixture.thread.id, None)
        .expect("interrupted child");
    let assistant_count: i64 = fixture
        .store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE thread_id=?1 AND role='assistant'",
            [&children[0].child_thread_id],
            |row| row.get(0),
        )
        .expect("child preparation side effects");
    assert_eq!(assistant_count, 0);
}

#[tokio::test(flavor = "current_thread")]
async fn pre_receipt_stop_and_admission_failure_leave_no_owned_workers() {
    for mode in [AdmissionMode::Stop, AdmissionMode::Reject] {
        let fixture = Fixture::new();
        let provider = fixture.provider(false);
        let cancel = CancellationToken::new();
        let host = RecordingHost::new(mode, cancel.clone());
        let mut events = Vec::new();
        let run = fixture
            .run(&provider, host.clone(), cancel, None, &mut events)
            .await
            .expect("cancellation or handled business failure");
        assert_eq!(provider.child_calls.load(Ordering::SeqCst), 0);
        assert!(host.events.lock().expect("child events").is_empty());
        match mode {
            AdmissionMode::Stop => {
                assert!(run.interrupted && !run.failed);
                assert_drained(&fixture, &host, SubagentState::Interrupted);
            }
            AdmissionMode::Reject => {
                assert!(!run.interrupted && !run.failed);
                assert_eq!(run.content, "parent complete");
                let state = tool_calls::find_state(fixture.store.conn(), SPAWN_CALL)
                    .expect("business failure state")
                    .expect("spawn call");
                assert_eq!(state.status, "failed");
                assert!(
                    state
                        .output_text
                        .expect("business failure result")
                        .starts_with("Tool error: tool 'subagent' failed:")
                );
                assert_drained(&fixture, &host, SubagentState::Failed);
            }
            AdmissionMode::Accept => unreachable!(),
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn strict_spawn_audit_failure_is_safe_and_durable() {
    let fixture = Fixture::new();
    fixture
        .store
        .conn()
        .execute_batch(&format!(
            "CREATE TRIGGER corrupt_spawn_audit AFTER UPDATE OF status ON tool_calls
             WHEN OLD.id='{SPAWN_CALL}' AND NEW.status='running'
             BEGIN UPDATE tool_calls
             SET approval='{{\"decision\":\"once\",\"note\":null,\"source\":\"{PRIVATE_SENTINEL}\",\"danger\":null}}'
             WHERE id=NEW.id; END;"
        ))
        .expect("real strict audit fault");
    let provider = fixture.provider(false);
    let cancel = CancellationToken::new();
    let host = RecordingHost::new(AdmissionMode::Accept, cancel.clone());
    let mut events = Vec::new();
    let result = fixture
        .run(&provider, host.clone(), cancel, None, &mut events)
        .await;
    assert!(result.is_err());
    assert_safe_failure(&fixture, &events, "approval_audit", None);
    assert_eq!(provider.child_calls.load(Ordering::SeqCst), 0);
    assert!(host.events.lock().expect("child events").is_empty());
    assert_drained(&fixture, &host, SubagentState::Interrupted);
    let arbitrary = VegaError::Tool {
        tool: "Read".into(),
        message: format!("strict approval audit failed database is locked {PRIVATE_SENTINEL}"),
    };
    assert_eq!(
        RunFailureKind::from_runtime(&arbitrary),
        RunFailureKind::Runtime
    );
    let metrics = serde_json::to_value(crate::agent::diagnostics::failure_metrics(&arbitrary))
        .expect("unclassified ordinary tool metadata");
    assert!(metrics["runtime_reason"].is_null());
    assert!(metrics["sqlite_code"].is_null());
    assert!(!metrics.to_string().contains(PRIVATE_SENTINEL));
    let roots = run_diagnostics::read_by_thread(fixture.store.conn(), &fixture.thread.id)
        .expect("all diagnostics");
    assert!(roots.iter().any(|event| {
        event.event.phase == DiagnosticPhase::Run && event.event.state == DiagnosticState::Failed
    }));
}
