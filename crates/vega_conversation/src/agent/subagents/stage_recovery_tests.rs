use super::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::sync::Semaphore;
use vega_runtime::{ChatRequest, ChatRole, EventStream, ProviderEvent, StopReason};

struct Fixture {
    _directory: tempfile::TempDir,
    workspace: PathBuf,
    database: PathBuf,
    store: Store,
    tools: vega_tools::Tools,
    request: AgentRequest,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("temporary workspace");
        let workspace = directory.path().join("workspace");
        let storage = directory.path().join("storage");
        std::fs::create_dir(&workspace).expect("workspace directory");
        std::fs::create_dir(&storage).expect("storage directory");
        let database = storage.join("test.sqlite");
        let store = Store::open(&database).expect("test store");
        store.migrate().expect("temporary store schema");
        let project = vega_store::projects::create(
            store.conn(),
            workspace.to_str().expect("workspace path"),
            "stage recovery",
            None,
        )
        .expect("project");
        vega_store::threads::create(
            store.conn(),
            vega_store::threads::NewThread {
                id: "parent",
                project_id: &project.id,
                title: "parent",
                mode: "execute",
                permission_mode: "confirm",
                model: "fixture-model",
                status: "active",
                pinned: false,
                unread: false,
                created_at: 1,
                updated_at: 1,
            },
        )
        .expect("parent thread");
        let prepared = super::super::pipeline::prepare_run_with_images_and_reasoning(
            database.clone(),
            "parent".into(),
            "root goal".into(),
            "Follow the explicit task and retain its constraints.".into(),
            "parent-user".into(),
            "parent-run".into(),
            PersistenceActorConfig::default(),
            false,
            None,
            None,
            Vec::new(),
        )
        .expect("native parent fixture");
        let tools = vega_tools::Tools::new(&workspace).expect("tools");
        Self {
            _directory: directory,
            workspace,
            database,
            store,
            tools,
            request: prepared.request,
        }
    }

    fn settings(&self) -> ChildSettings {
        ChildSettings::new(&self.request, &PersistenceActorConfig::default())
    }

    fn launch<'a>(&'a self, provider: &'a dyn Provider, host: Arc<TestHost>) -> ChildLaunch<'a> {
        ChildLaunch {
            provider,
            tools: &self.tools,
            database: self.database.clone(),
            parent_thread: "parent".into(),
            parent_message: "parent-run".into(),
            settings: self.settings(),
            mcp_servers: Vec::new(),
            host,
            cancel: CancellationToken::new(),
        }
    }
}

fn insert_message(store: &Store, thread: &str, id: &str, role: &str, content: &str, status: &str) {
    messages::insert(
        store.conn(),
        &messages::MessageRow {
            id: id.into(),
            thread_id: thread.into(),
            seq: messages::next_seq(store.conn(), thread).expect("message sequence"),
            role: role.into(),
            kind: "text".into(),
            content: content.into(),
            status: status.into(),
            created_at: 1,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .expect("message");
}

#[derive(Default)]
struct ScriptedProvider {
    scripts: Mutex<VecDeque<Vec<ProviderEvent>>>,
    requests: Mutex<Vec<ChatRequest>>,
    called: Notify,
    gates: Mutex<VecDeque<Option<Arc<Semaphore>>>>,
}

impl ScriptedProvider {
    fn with(scripts: Vec<Vec<ProviderEvent>>) -> Self {
        Self {
            scripts: Mutex::new(scripts.into()),
            ..Self::default()
        }
    }

    async fn request_count(&self, count: usize) {
        bounded(async {
            loop {
                let called = self.called.notified();
                tokio::pin!(called);
                called.as_mut().enable();
                if self.requests.lock().expect("requests").len() >= count {
                    return;
                }
                called.await;
            }
        })
        .await;
    }
}

impl Provider for ScriptedProvider {
    fn chat_stream(
        &self,
        request: ChatRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        self.requests.lock().expect("requests").push(request);
        self.called.notify_waiters();
        let script = self.scripts.lock().expect("scripts").pop_front();
        let gate = self.gates.lock().expect("gates").pop_front().flatten();
        async move {
            if let Some(gate) = gate {
                gate.acquire().await.expect("provider gate").forget();
            }
            let stream: EventStream = match script {
                Some(script) => Box::pin(futures::stream::iter(script.into_iter().map(Ok))),
                None => Box::pin(futures::stream::pending()),
            };
            Ok(stream)
        }
        .boxed()
    }
}

struct HostAdmission {
    cancel: CancellationToken,
    permissions: PermissionQueue,
    questions: UserQuestionQueue,
    _permission_listener: PermissionQueueListener,
    _question_listener: UserQuestionListener,
}

struct TestHost {
    admission: Mutex<Option<HostAdmission>>,
    changed: Notify,
    release_gate: Arc<Semaphore>,
    release_started: AtomicBool,
    release_failed: AtomicBool,
    released: Mutex<Vec<SubagentSnapshot>>,
    compactions: AtomicUsize,
    finished_calls: Mutex<Vec<String>>,
}

impl TestHost {
    fn new(release: bool) -> Arc<Self> {
        Arc::new(Self {
            admission: Mutex::new(None),
            changed: Notify::new(),
            release_gate: Arc::new(Semaphore::new(if release { 128 } else { 0 })),
            release_started: AtomicBool::new(false),
            release_failed: AtomicBool::new(false),
            released: Mutex::new(Vec::new()),
            compactions: AtomicUsize::new(0),
            finished_calls: Mutex::new(Vec::new()),
        })
    }

    async fn until(&self, predicate: impl Fn() -> bool) {
        bounded(async {
            loop {
                let changed = self.changed.notified();
                tokio::pin!(changed);
                changed.as_mut().enable();
                if predicate() {
                    return;
                }
                tokio::select! {
                    _ = changed => {},
                    _ = tokio::time::sleep(Duration::from_millis(10)) => {},
                }
            }
        })
        .await;
    }

    async fn await_release(&self) {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self.release_started.load(Ordering::SeqCst) {
                return;
            }
            changed.await;
        }
    }

    async fn pending_permission(&self) {
        let (permissions, mut changed) = {
            let admission = self.admission.lock().expect("admission");
            let admission = admission.as_ref().expect("admitted");
            (
                admission.permissions.clone(),
                admission._permission_listener.receiver.clone(),
            )
        };
        loop {
            if permissions.has_pending() {
                return;
            }
            changed.changed().await.expect("permission listener alive");
        }
    }

    fn cancel(&self) -> CancellationToken {
        self.admission
            .lock()
            .expect("admission")
            .as_ref()
            .expect("admitted")
            .cancel
            .clone()
    }

    fn questions(&self) -> UserQuestionQueue {
        self.admission
            .lock()
            .expect("admission")
            .as_ref()
            .expect("admitted")
            .questions
            .clone()
    }

    fn permissions(&self) -> PermissionQueue {
        self.admission
            .lock()
            .expect("admission")
            .as_ref()
            .expect("admitted")
            .permissions
            .clone()
    }
}

impl SubagentHost for TestHost {
    fn admit(&self, admission: SubagentAdmission) -> BoxFuture<'static, Result<(), VegaError>> {
        let permission_listener = admission.permission_queue.subscribe();
        let question_listener = admission.user_questions.subscribe();
        *self.admission.lock().expect("admission") = Some(HostAdmission {
            cancel: admission.cancel,
            permissions: admission.permission_queue,
            questions: admission.user_questions,
            _permission_listener: permission_listener,
            _question_listener: question_listener,
        });
        self.changed.notify_waiters();
        async { Ok(()) }.boxed()
    }

    fn event(&self, _: &str, _: &str, event: &ConversationEvent) -> Result<(), VegaError> {
        if let ConversationEvent::ToolCallFinished { call_id, .. } = event {
            self.finished_calls
                .lock()
                .expect("finished calls")
                .push(call_id.to_string());
        }
        if matches!(event, ConversationEvent::ContextCompactionStatus { .. }) {
            self.compactions.fetch_add(1, Ordering::SeqCst);
        }
        self.changed.notify_waiters();
        Ok(())
    }

    fn updated(&self, _: SubagentSnapshot) -> Result<(), VegaError> {
        self.changed.notify_waiters();
        Ok(())
    }

    fn finished(&self, snapshot: SubagentSnapshot, _: bool) -> Result<(), VegaError> {
        self.released.lock().expect("released").push(snapshot);
        Ok(())
    }

    fn finish_and_release(
        &self,
        _: SubagentSnapshot,
        _: bool,
    ) -> BoxFuture<'static, Result<(), VegaError>> {
        self.release_started.store(true, Ordering::SeqCst);
        self.changed.notify_waiters();
        let gate = self.release_gate.clone();
        let fail = self.release_failed.load(Ordering::SeqCst);
        async move {
            let permit = gate.acquire().await.expect("release gate");
            permit.forget();
            if fail {
                Err(failure("fixture release failure"))
            } else {
                Ok(())
            }
        }
        .boxed()
    }
}

async fn bounded<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(5), future)
        .await
        .expect("native operation must finish within five seconds")
}

async fn followup(
    hook: &Arc<dyn vega_runtime::RuntimeSubagentHook>,
    observer: &ReceiptObserver,
    prior: &SubagentSnapshot,
    task: &str,
) -> SubagentSnapshot {
    let value = bounded(hook.call(
        "followup-call".into(),
        "followup_agent".into(),
        serde_json::json!({"agent_id":prior.agent_id,"message_id":prior.message_id,"task":task}),
        CancellationToken::new(),
    ))
    .await
    .expect("followup admission");
    acknowledge(observer, "followup-call", &value);
    serde_json::from_value(value["agent"].clone()).expect("followup snapshot")
}

fn tool_response(text: &str, id: &str, name: &str, input: serde_json::Value) -> Vec<ProviderEvent> {
    vec![
        ProviderEvent::TextDelta(text.into()),
        ProviderEvent::ToolUse {
            id: id.into(),
            name: name.into(),
            input_json: input.to_string(),
        },
        ProviderEvent::Done {
            stop_reason: StopReason::ToolUse,
        },
    ]
}

fn final_response(text: &str) -> Vec<ProviderEvent> {
    vec![
        ProviderEvent::TextDelta(text.into()),
        ProviderEvent::Usage {
            input: 1,
            output: 1,
            cache_read: Some(0),
            cache_write: Some(0),
        },
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ]
}

async fn spawn(
    hook: &Arc<dyn vega_runtime::RuntimeSubagentHook>,
    observer: &ReceiptObserver,
    task: &str,
) -> SubagentSnapshot {
    let value = bounded(hook.call(
        "spawn-call".into(),
        "spawn_agent".into(),
        serde_json::json!({"name":"worker","task":task}),
        CancellationToken::new(),
    ))
    .await
    .expect("spawn");
    acknowledge(observer, "spawn-call", &value);
    serde_json::from_value(value["agent"].clone()).expect("spawn snapshot")
}

fn acknowledge(observer: &ReceiptObserver, call_id: &str, value: &serde_json::Value) {
    observer.persisted(&RuntimeEvent::ToolCallFinished(
        vega_runtime::RuntimeToolResult {
            call_id: call_id.into(),
            output: value.to_string(),
            status: RuntimeToolStatus::Success,
            reused: false,
            exit_code: None,
            duration_ms: None,
            truncated: None,
            approval: None,
            remember_rule: None,
        },
    ));
}

fn wait_input(snapshot: &SubagentSnapshot, timeout: Option<u64>) -> serde_json::Value {
    let mut input = serde_json::json!({"targets":[{"agent_id":snapshot.agent_id,"message_id":snapshot.message_id}]});
    if let Some(timeout) = timeout {
        input["timeout_ms"] = timeout.into();
    }
    input
}

async fn wait(
    hook: &Arc<dyn vega_runtime::RuntimeSubagentHook>,
    snapshot: &SubagentSnapshot,
    timeout: Option<u64>,
) -> serde_json::Value {
    hook.call(
        "wait-call".into(),
        "wait_agent".into(),
        wait_input(snapshot, timeout),
        CancellationToken::new(),
    )
    .await
    .expect("wait")
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn checkpoint(ready: bool, path: &str, read_call: &str) -> Value {
    json!({
        "stage":"implementation",
        "summary":"Prepared the scoped artifact; independent review and CI remain pending.",
        "artifacts":[{"path":path,"read_call_id":read_call}],
        "evidence_call_ids":[read_call],
        "unresolved":["Independent review", "Final exact-version CI"],
        "next_step":"Review this artifact without repeating completed checks.",
        "ready_for_review":ready
    })
}

fn read_response(id: &str, path: &str) -> Vec<ProviderEvent> {
    tool_response(
        "Read the actual artifact.",
        id,
        "Read",
        json!({"path":path}),
    )
}

fn checkpoint_response(id: &str, input: Value) -> Vec<ProviderEvent> {
    tool_response("Record this stage.", id, "checkpoint_stage", input)
}

fn first_json(text: &str) -> Option<Value> {
    serde_json::Deserializer::from_str(text)
        .into_iter::<Value>()
        .next()
        .and_then(Result::ok)
}

fn audit(fixture: &Fixture, id: &str) -> vega_store::tool_calls::ToolCallState {
    vega_store::tool_calls::find_state(fixture.store.conn(), id)
        .expect("tool audit")
        .expect("persisted call")
}

fn checkpoint_output(fixture: &Fixture, id: &str) -> Option<Value> {
    let call = audit(fixture, id);
    (call.status == "success")
        .then(|| call.output_text.as_deref().and_then(first_json))
        .flatten()
}

impl ScriptedProvider {
    fn gate_request(&self, index: usize) -> Arc<Semaphore> {
        let gate = Arc::new(Semaphore::new(0));
        let mut gates = self.gates.lock().expect("gates");
        while gates.len() <= index {
            gates.push_back(None);
        }
        gates[index] = Some(gate.clone());
        gate
    }

    fn count(&self) -> usize {
        self.requests.lock().expect("requests").len()
    }
}

impl TestHost {
    async fn call_finished(&self, id: &str) {
        self.until(|| {
            self.finished_calls
                .lock()
                .expect("finished calls")
                .iter()
                .any(|call| call == id)
        })
        .await;
    }
}

async fn list(hook: &Arc<dyn vega_runtime::RuntimeSubagentHook>) -> Value {
    bounded(hook.call(
        "list".into(),
        "list_agents".into(),
        json!({}),
        CancellationToken::new(),
    ))
    .await
    .expect("list")
}

fn listed_agent(value: &Value, id: &str) -> Value {
    value["agents"]
        .as_array()
        .and_then(|agents| agents.iter().find(|agent| agent["agent_id"] == id))
        .cloned()
        .unwrap_or(Value::Null)
}

async fn finish_observation(
    hook: &Arc<dyn vega_runtime::RuntimeSubagentHook>,
    host: &TestHost,
    child: &SubagentSnapshot,
) -> Result<Value, VegaError> {
    host.cancel().cancel();
    host.release_gate.add_permits(32);
    bounded(hook.call(
        "drain".into(),
        "wait_agent".into(),
        wait_input(child, None),
        CancellationToken::new(),
    ))
    .await
}

fn resume_input(child: &SubagentSnapshot, checkpoint: &Value) -> Value {
    json!({
        "agent_id":child.agent_id,"message_id":child.message_id,
        "checkpoint_id":checkpoint["id"],"source_version":checkpoint["source_version"],
        "source_fingerprint":checkpoint["source_fingerprint"],
        "task":"Continue the same scoped task. Preserve all constraints and inspect existing evidence before any new action.",
        "reconciliations":[]
    })
}

async fn call_resume(
    hook: &Arc<dyn vega_runtime::RuntimeSubagentHook>,
    observer: &ReceiptObserver,
    call_id: &str,
    input: Value,
) -> Result<Value, VegaError> {
    let result = bounded(hook.call(
        call_id.into(),
        "resume_agent".into(),
        input,
        CancellationToken::new(),
    ))
    .await;
    if let Ok(value) = &result {
        acknowledge(observer, call_id, value);
    }
    result
}

fn new_parent_run(fixture: &Fixture, run: &str) {
    fixture
        .store
        .conn()
        .execute(
            "UPDATE messages SET status='done' WHERE thread_id='parent' AND status='streaming'",
            [],
        )
        .expect("finish fixture parent");
    insert_message(
        &fixture.store,
        "parent",
        &format!("{run}-user"),
        "user",
        "Continue the existing child, keeping the original acceptance.",
        "done",
    );
    insert_message(&fixture.store, "parent", run, "assistant", "", "streaming");
}

async fn interrupt_with_checkpoint(fixture: &Fixture) -> (SubagentSnapshot, Value) {
    std::fs::write(
        fixture.workspace.join("artifact.txt"),
        b"retained version\r\n",
    )
    .expect("artifact");
    let provider = ScriptedProvider::with(vec![
        read_response("read-artifact", "artifact.txt"),
        checkpoint_response(
            "save-stage",
            checkpoint(false, "artifact.txt", "read-artifact"),
        ),
    ]);
    let host = TestHost::new(true);
    let mut supervisor = Supervisor::new();
    let hook = supervisor.hook();
    let observer = supervisor.receipt_observer();
    supervisor
        .run(
            async {
                let child = spawn(
                    &hook,
                    &observer,
                    "Preserve the raw CRLF artifact and do not repeat existing CI.",
                )
                .await;
                provider.request_count(3).await;
                let terminal = finish_observation(&hook, &host, &child)
                    .await
                    .expect("interrupted terminal");
                (child, terminal["agents"][0].clone())
            }
            .boxed_local(),
            fixture.launch(&provider, host.clone()),
        )
        .await
}

#[tokio::test]
async fn elapsed_active_time_never_cancels_native_child_or_repeats_provider_work() {
    for human_pending in [false, true] {
        let fixture = Fixture::new();
        let scripts = if human_pending {
            vec![tool_response(
                "Need a real decision.",
                "human-choice",
                "ask_user_question",
                json!({"questions":[{"id":"continue_work","header":"Continue","question":"Which scoped action?","options":[{"label":"Keep scope","description":"Preserve original acceptance."},{"label":"Inspect only","description":"Inspect the saved work."}]}]}),
            )]
        } else {
            Vec::new()
        };
        let provider = ScriptedProvider::with(scripts);
        let host = TestHost::new(true);
        let mut supervisor = Supervisor::new();
        let hook = supervisor.hook();
        let observer = supervisor.receipt_observer();
        let observed = supervisor
            .run(
                async {
                    let child = spawn(
                        &hook,
                        &observer,
                        "Work until the explicit task is finished.",
                    )
                    .await;
                    provider.request_count(1).await;
                    let mut stayed_live = true;
                    if human_pending {
                        host.until(|| host.questions().has_pending()).await;
                        let pending = host.questions().take_pending().expect("real question");
                        tokio::time::pause();
                        tokio::time::advance(Duration::from_secs(40 * 60)).await;
                        stayed_live &= !host.cancel().is_cancelled() && !pending.is_resolved();
                        tokio::time::resume();
                        pending
                            .respond(
                                serde_json::from_value(
                                    json!({"answers":{"continue_work":{"answers":["Keep scope"]}}}),
                                )
                                .expect("answer"),
                            )
                            .expect("respond");
                        provider.request_count(2).await;
                    }
                    let count = provider.count();
                    tokio::time::pause();
                    for _ in 0..2 {
                        tokio::time::advance(Duration::from_secs(21 * 60)).await;
                        tokio::task::yield_now().await;
                        stayed_live &= !host.cancel().is_cancelled();
                    }
                    tokio::time::resume();
                    let unchanged_requests = provider.count() == count;
                    let terminal = finish_observation(&hook, &host, &child).await;
                    (stayed_live, unchanged_requests, terminal)
                }
                .boxed_local(),
                fixture.launch(&provider, host.clone()),
            )
            .await;
        assert!(
            observed.0,
            "elapsed active time must not cancel a native child"
        );
        assert!(
            observed.1,
            "elapsed time must not restart or repeat provider work"
        );
        assert_eq!(
            observed.2.expect("explicit cancellation drains")["agents"][0]["state"],
            "interrupted"
        );
    }
}

#[tokio::test]
async fn nonready_checkpoint_is_durable_before_ack_and_stage_continues() {
    let fixture = Fixture::new();
    let bytes = b"untracked raw artifact\r\n";
    std::fs::write(fixture.workspace.join("artifact.txt"), bytes).expect("artifact");
    let provider = ScriptedProvider::with(vec![
        read_response("read-version", "artifact.txt"),
        checkpoint_response(
            "stage-save",
            checkpoint(false, "artifact.txt", "read-version"),
        ),
    ]);
    let host = TestHost::new(true);
    let mut supervisor = Supervisor::new();
    let hook = supervisor.hook();
    let observer = supervisor.receipt_observer();
    let observed = supervisor
        .run(
            async {
                let child = spawn(
                    &hook,
                    &observer,
                    "Retain a nonready checkpoint and continue.",
                )
                .await;
                provider.request_count(3).await;
                let receipt = checkpoint_output(&fixture, "stage-save");
                let current = listed_agent(&list(&hook).await, &child.agent_id);
                let alive =
                    !host.cancel().is_cancelled() && !host.release_started.load(Ordering::SeqCst);
                let persisted = receipt
                    .as_ref()
                    .and_then(|value| value["checkpoint"]["id"].as_str())
                    .map(|id| {
                        let reopened =
                            Store::open_read_only(&fixture.database).expect("reopen durable store");
                        reopened
                            .conn()
                            .query_row(
                                "SELECT COUNT(*) FROM delegated_stage_checkpoints WHERE id=?1",
                                [id],
                                |row| row.get::<_, i64>(0),
                            )
                            .expect("durable checkpoint query")
                    });
                let _ = finish_observation(&hook, &host, &child).await;
                (receipt, current, alive, persisted)
            }
            .boxed_local(),
            fixture.launch(&provider, host.clone()),
        )
        .await;
    let receipt = observed
        .0
        .expect("checkpoint tool must persist and acknowledge a real checkpoint");
    assert_eq!(receipt["checkpoint"]["origin"], "declared");
    assert_eq!(
        receipt["checkpoint"]["manifest"]["artifacts"][0]["sha256"],
        sha256(bytes)
    );
    assert_eq!(
        receipt["checkpoint"]["manifest"]["unresolved"],
        json!(["Independent review", "Final exact-version CI"])
    );
    assert_eq!(receipt["ready_for_review"], false);
    assert_eq!(observed.1["ready_for_review"], false);
    assert!(
        observed.2,
        "saving recovery information must not stop the writer"
    );
    assert_eq!(
        observed.3,
        Some(1),
        "acknowledgement requires durable append-only checkpoint"
    );
}

#[tokio::test]
async fn ready_handoff_stops_stage_but_waits_for_terminal_persistence_and_host_release() {
    let fixture = Fixture::new();
    let bytes = b"review this exact file\n";
    std::fs::write(fixture.workspace.join("artifact.txt"), bytes).expect("artifact");
    let provider = ScriptedProvider::with(vec![
        read_response("read-ready", "artifact.txt"),
        checkpoint_response(
            "stage-ready",
            checkpoint(true, "artifact.txt", "read-ready"),
        ),
    ]);
    let host = TestHost::new(false);
    let mut supervisor = Supervisor::new();
    let hook = supervisor.hook();
    let observer = supervisor.receipt_observer();
    let observed = supervisor
        .run(
            async {
                let child =
                    spawn(&hook, &observer, "Publish the stopped artifact for review.").await;
                host.call_finished("stage-ready").await;
                let receipt = checkpoint_output(&fixture, "stage-ready");
                let reached_release = bounded(async {
                    tokio::select! {
                        biased;
                        _ = provider.request_count(3) => false,
                        _ = host.await_release() => true,
                    }
                })
                .await;
                let held = listed_agent(&list(&hook).await, &child.agent_id);
                let calls_before_release = provider.count();
                host.release_gate.add_permits(32);
                if !reached_release {
                    host.cancel().cancel();
                }
                let terminal = bounded(wait(&hook, &child, None)).await;
                (
                    receipt,
                    reached_release,
                    held,
                    calls_before_release,
                    terminal,
                )
            }
            .boxed_local(),
            fixture.launch(&provider, host.clone()),
        )
        .await;
    assert!(
        observed.0.is_some(),
        "ready declaration must be validated by runtime"
    );
    assert!(
        observed.1,
        "ready stage must end without an extra provider request"
    );
    assert_eq!(observed.2["ready_for_review"], false);
    assert_ne!(observed.2["state"], "completed");
    assert_eq!(
        observed.3, 2,
        "no later provider or tool dispatch after ready declaration"
    );
    assert_eq!(observed.4["agents"][0]["ready_for_review"], true);
    assert_eq!(observed.4["agents"][0]["release_state"], "released");
}

#[tokio::test]
async fn runtime_handoff_can_followup_without_fabricating_model_final_metadata() {
    let fixture = Fixture::new();
    std::fs::write(fixture.workspace.join("artifact.txt"), b"scoped work\n").expect("artifact");
    let provider = ScriptedProvider::with(vec![
        read_response("handoff-read", "artifact.txt"),
        checkpoint_response("handoff", checkpoint(true, "artifact.txt", "handoff-read")),
    ]);
    let host = TestHost::new(true);
    let mut supervisor = Supervisor::new();
    let hook = supervisor.hook();
    let observer = supervisor.receipt_observer();
    let observed = supervisor
        .run(
            async {
                let child = spawn(
                    &hook,
                    &observer,
                    "Keep acceptance; prepare the stage for independent review.",
                )
                .await;
                host.call_finished("handoff").await;
                let saved = checkpoint_output(&fixture, "handoff");
                if saved.is_none() {
                    let _ = finish_observation(&hook, &host, &child).await;
                    return None;
                }
                let terminal = bounded(wait(&hook, &child, None)).await;
                let handoff_finals: i64 = fixture
                    .store
                    .conn()
                    .query_row(
                        "SELECT COUNT(*) FROM assistant_responses WHERE message_id=?1 AND kind='final'",
                        [&child.message_id],
                        |row| row.get(0),
                    )
                    .expect("handoff response metadata");
                let handoff_final_text =
                    messages::completed_response_text(fixture.store.conn(), &child.message_id)
                        .expect("handoff completed response");
                let before = provider.count();
                provider
                    .scripts
                    .lock()
                    .expect("scripts")
                    .push_back(final_response(
                        "Reviewed the saved scope; CI remains pending.",
                    ));
                let next = followup(
                    &hook,
                    &observer,
                    &child,
                    "Inspect the saved artifact and retain independent CI.",
                )
                .await;
                let followed = bounded(wait(&hook, &next, None)).await;
                let followup_finals: i64 = fixture
                    .store
                    .conn()
                    .query_row(
                        "SELECT COUNT(*) FROM assistant_responses WHERE message_id=?1 AND kind='final'",
                        [&next.message_id],
                        |row| row.get(0),
                    )
                    .expect("followup response metadata");
                let followup_final_text =
                    messages::completed_response_text(fixture.store.conn(), &next.message_id)
                        .expect("followup completed response");
                let request = provider
                    .requests
                    .lock()
                    .expect("requests")
                    .last()
                    .cloned()
                    .expect("followup request");
                Some((
                    child,
                    next,
                    terminal,
                    followed,
                    before,
                    request,
                    handoff_finals,
                    handoff_final_text,
                    followup_finals,
                    followup_final_text,
                ))
            }
            .boxed_local(),
            fixture.launch(&provider, host.clone()),
        )
        .await;
    let (
        child,
        next,
        terminal,
        followed,
        before,
        request,
        handoff_finals,
        handoff_final_text,
        followup_finals,
        followup_final_text,
    ) = observed.expect("runtime-origin handoff must exist before continuation");
    assert_eq!(handoff_finals, 0, "runtime handoff is not a model final");
    assert!(handoff_final_text.is_none());
    assert_eq!(followup_finals, 1);
    assert_eq!(
        followup_final_text.as_deref(),
        Some("Reviewed the saved scope; CI remains pending.")
    );
    assert_eq!(terminal["agents"][0]["ready_for_review"], true);
    assert_eq!(before, 2);
    assert_eq!(next.agent_id, child.agent_id);
    assert_ne!(next.message_id, child.message_id);
    assert_eq!(followed["agents"][0]["state"], "completed");
    assert_eq!(
        followed["agents"][0]["ready_for_review"], false,
        "ordinary completion cannot inherit an earlier READY"
    );
    assert!(
        request
            .messages
            .iter()
            .any(|message| message.role != ChatRole::System
                && message.content.contains("Independent review")
                && message.content.contains(&child.message_id)),
        "handoff manifest and provenance must be lower-trust task data"
    );
    assert!(
        !request
            .messages
            .iter()
            .filter(|message| message.role == ChatRole::System)
            .any(|message| message.content.contains("Prepared the scoped artifact"))
    );
}

#[tokio::test]
async fn checkpoint_requires_complete_call_bound_read_and_unchanged_raw_file() {
    for case in [
        "partial",
        "long_line",
        "result_truncation",
        "write_preimage",
        "changed",
        "missing",
        "wrong_call",
        "foreign_execution",
        "utf16",
    ] {
        let fixture = Fixture::new();
        if case == "foreign_execution" {
            let _ = interrupt_with_checkpoint(&fixture).await;
            new_parent_run(&fixture, "parent-read-binding");
        }
        let bytes = match case {
            "foreign_execution" => b"retained version\r\n".to_vec(),
            "long_line" => vec![b'x'; 3000],
            "result_truncation" => b"ordinary line\n".repeat(20_000),
            "utf16" => [
                vec![0xff, 0xfe],
                "raw UTF16 version\r\n"
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes)
                    .collect(),
            ]
            .concat(),
            _ => b"first\r\nsecond\r\n".to_vec(),
        };
        let path = fixture.workspace.join("artifact.txt");
        std::fs::write(&path, &bytes).expect("artifact");
        let read = if case == "partial" {
            tool_response(
                "Inspect one line.",
                "artifact-read",
                "Read",
                json!({"path":"artifact.txt","limit":1}),
            )
        } else if case == "write_preimage" {
            tool_response(
                "Write a new artifact.",
                "artifact-read",
                "Write",
                json!({"path":"artifact.txt","content":"new version\n"}),
            )
        } else {
            read_response("artifact-read", "artifact.txt")
        };
        let call = match case {
            "wrong_call" => "unrelated-read",
            "foreign_execution" => "read-artifact",
            _ => "artifact-read",
        };
        let mut scripts = vec![
            read,
            checkpoint_response("check-file", checkpoint(false, "artifact.txt", call)),
        ];
        let checkpoint_index = if case == "write_preimage" {
            scripts.insert(0, read_response("preimage-read", "artifact.txt"));
            2
        } else {
            1
        };
        let provider = ScriptedProvider::with(scripts);
        let gate = provider.gate_request(checkpoint_index);
        let host = TestHost::new(true);
        let mut supervisor = Supervisor::new();
        let hook = supervisor.hook();
        let observer = supervisor.receipt_observer();
        let mut launch = fixture.launch(&provider, host.clone());
        if case == "foreign_execution" {
            launch.parent_message = "parent-read-binding".into();
        }
        let observed = supervisor
            .run(
                async {
                    let child = spawn(&hook, &observer, "Keep file identity exact.").await;
                    if case == "write_preimage" {
                        host.until(|| host.permissions().has_pending()).await;
                        let (_, mut lease) = host
                            .permissions()
                            .take_pending()
                            .expect("write approval")
                            .into_parts()
                            .expect("approval lease");
                        lease.respond(crate::types::PermissionDecision::Once);
                    }
                    provider.request_count(checkpoint_index + 1).await;
                    if case == "changed" {
                        std::fs::write(&path, b"a later writer changed this\n")
                            .expect("change artifact");
                    }
                    if case == "missing" {
                        std::fs::remove_file(&path).expect("remove artifact");
                    }
                    gate.add_permits(1);
                    provider.request_count(checkpoint_index + 2).await;
                    let read_audit = audit(&fixture, "artifact-read");
                    let receipt = checkpoint_output(&fixture, "check-file");
                    let _ = finish_observation(&hook, &host, &child).await;
                    (read_audit, receipt)
                }
                .boxed_local(),
                launch,
            )
            .await;
        assert_eq!(
            observed.0.status, "success",
            "{case}: fixture must exercise a real successful source tool"
        );
        if case == "utf16" {
            let receipt = observed
                .1
                .expect("complete UTF16 Read must create a checkpoint");
            assert_eq!(
                receipt["checkpoint"]["manifest"]["artifacts"][0]["sha256"],
                sha256(&bytes)
            );
        } else {
            assert!(
                observed.1.is_none(),
                "{case}: incomplete, unrelated or changed evidence cannot establish an artifact"
            );
        }
    }
}

#[tokio::test]
async fn external_read_denial_and_mixed_ready_batch_cannot_bypass_tool_authority() {
    for mixed in [false, true] {
        let fixture = Fixture::new();
        let outside = tempfile::tempdir().expect("external folder");
        let external = outside.path().join("external.txt");
        std::fs::write(&external, b"external private content\n").expect("external artifact");
        std::fs::write(fixture.workspace.join("artifact.txt"), b"original\n")
            .expect("local artifact");
        let path = if mixed {
            "artifact.txt".into()
        } else {
            external.to_str().expect("external path").to_string()
        };
        let mut checkpoint_call =
            checkpoint_response("unsafe-handoff", checkpoint(true, &path, "authority-read"));
        if mixed {
            checkpoint_call.insert(
                2,
                ProviderEvent::ToolUse {
                    id: "later-write".into(),
                    name: "Write".into(),
                    input_json: json!({"path":"artifact.txt","content":"must not run\n"})
                        .to_string(),
                },
            );
        }
        let provider = ScriptedProvider::with(vec![
            read_response("authority-read", &path),
            checkpoint_call,
        ]);
        let host = TestHost::new(true);
        let mut supervisor = Supervisor::new();
        let hook = supervisor.hook();
        let observer = supervisor.receipt_observer();
        let observed = supervisor
            .run(
                async {
                    let child = spawn(&hook, &observer, "Never bypass file permission.").await;
                    if !mixed {
                        host.until(|| host.permissions().has_pending()).await;
                        let (_, mut lease) = host
                            .permissions()
                            .take_pending()
                            .expect("external read permission")
                            .into_parts()
                            .expect("lease");
                        lease.respond(crate::types::PermissionDecision::Deny { note: None });
                    }
                    host.call_finished("unsafe-handoff").await;
                    let saved = checkpoint_output(&fixture, "unsafe-handoff");
                    let continued = bounded(async {
                        tokio::select! {
                            biased;
                            _ = host.pending_permission() => false,
                            _ = host.await_release() => false,
                            _ = provider.request_count(3) => true,
                        }
                    })
                    .await;
                    let pending_write = host.permissions().has_pending();
                    let released = host.release_started.load(Ordering::SeqCst);
                    let _ = finish_observation(&hook, &host, &child).await;
                    (saved, pending_write, continued, released)
                }
                .boxed_local(),
                fixture.launch(&provider, host.clone()),
            )
            .await;
        assert!(observed.0.is_none());
        assert!(
            observed.2 && !observed.1 && !observed.3,
            "checkpoint rejection must reach the next provider before a later permission or ready release"
        );
        assert_eq!(
            std::fs::read(fixture.workspace.join("artifact.txt")).expect("artifact bytes"),
            b"original\n"
        );
    }
}

#[tokio::test]
async fn checkpoint_persistence_failure_cannot_acknowledge_or_publish_ready() {
    let fixture = Fixture::new();
    let exists: bool = fixture.store.conn().query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='delegated_stage_checkpoints')", [], |row| row.get(0)).expect("checkpoint schema inventory");
    if exists {
        fixture.store.conn().execute_batch("CREATE TRIGGER reject_stage_checkpoint BEFORE INSERT ON delegated_stage_checkpoints BEGIN SELECT RAISE(ABORT,'fixture checkpoint persistence failure'); END;").expect("checkpoint fault injection");
    }
    std::fs::write(
        fixture.workspace.join("artifact.txt"),
        b"durability first\n",
    )
    .expect("artifact");
    let provider = ScriptedProvider::with(vec![
        read_response("persist-read", "artifact.txt"),
        checkpoint_response(
            "persist-stage",
            checkpoint(false, "artifact.txt", "persist-read"),
        ),
    ]);
    let host = TestHost::new(true);
    let mut supervisor = Supervisor::new();
    let hook = supervisor.hook();
    let observer = supervisor.receipt_observer();
    let observed = supervisor
        .run(
            async {
                let child = spawn(&hook, &observer, "Require a durable checkpoint.").await;
                host.call_finished("persist-stage").await;
                let call = audit(&fixture, "persist-stage");
                let visible = listed_agent(&list(&hook).await, &child.agent_id);
                let _ = finish_observation(&hook, &host, &child).await;
                (call, visible)
            }
            .boxed_local(),
            fixture.launch(&provider, host.clone()),
        )
        .await;
    assert!(exists, "native checkpoint storage must exist");
    assert_ne!(observed.0.status, "success");
    assert!(
        observed
            .0
            .output_text
            .unwrap_or_default()
            .contains("persist"),
        "failed durable save must report a persistence condition"
    );
    assert_eq!(observed.1["ready_for_review"], false);
    assert!(observed.1["checkpoint"].is_null());
}

#[tokio::test]
async fn handoff_terminal_persistence_or_release_failure_retains_occupancy_without_ready() {
    for persist_failure in [false, true] {
        let fixture = Fixture::new();
        std::fs::write(fixture.workspace.join("artifact.txt"), b"held version\n")
            .expect("artifact");
        let provider = ScriptedProvider::with(vec![
            read_response("held-read", "artifact.txt"),
            checkpoint_response("held-stage", checkpoint(true, "artifact.txt", "held-read")),
        ]);
        let gate = provider.gate_request(1);
        let host = TestHost::new(false);
        host.release_failed
            .store(!persist_failure, Ordering::SeqCst);
        let mut supervisor = Supervisor::new();
        let hook = supervisor.hook();
        let observer = supervisor.receipt_observer();
        let observed = supervisor.run(async {
            let child = spawn(&hook, &observer, "Stop only after durable handoff.").await;
            provider.request_count(2).await;
            if persist_failure {
                fixture.store.conn().execute_batch("CREATE TRIGGER reject_terminal_state BEFORE UPDATE OF state ON delegated_executions WHEN NEW.state IN ('completed','failed','interrupted') BEGIN SELECT RAISE(ABORT,'fixture terminal persistence failure'); END;").expect("terminal persistence fault");
            }
            gate.add_permits(1);
            host.call_finished("held-stage").await;
            let saved = checkpoint_output(&fixture, "held-stage");
            if saved.is_none() {
                let _ = finish_observation(&hook, &host, &child).await;
                return None;
            }
            host.until(|| host.release_started.load(Ordering::SeqCst)).await;
            let visible = listed_agent(&list(&hook).await, &child.agent_id);
            host.release_gate.add_permits(32);
            let result = bounded(hook.call("failed-handoff-wait".into(),"wait_agent".into(),wait_input(&child,None),CancellationToken::new())).await;
            let replacement = bounded(hook.call("replacement".into(), "spawn_agent".into(), json!({"name":"replacement","task":"Do not replace an unconfirmed writer."}), CancellationToken::new())).await;
            Some((visible, replacement.is_err(), result.is_err(), child, saved))
        }.boxed_local(), fixture.launch(&provider, host.clone())).await;
        let (visible, rejected, explicit_failure, child, saved) =
            observed.expect("runtime must create a real handoff before failure injection");
        let before =
            vega_store::context_compaction::load_source(fixture.store.conn(), &child.agent_id)
                .expect("quarantined source");
        new_parent_run(&fixture, "parent-after-release-failure");
        let _ = crate::history::restart_history_page(&fixture.store, "parent", 20);
        let prepare_rejected = super::super::pipeline::prepare_run_with_images_and_reasoning(
            fixture.database.clone(),
            child.agent_id.clone(),
            "Do not replace an unconfirmed writer".into(),
            "system".into(),
            "quarantine-user".into(),
            "quarantine-run".into(),
            PersistenceActorConfig::default(),
            false,
            None,
            None,
            Vec::new(),
        )
        .is_err();
        let next_provider = ScriptedProvider::default();
        let next_host = TestHost::new(true);
        let mut next_supervisor = Supervisor::new();
        let next_hook = next_supervisor.hook();
        let next_observer = next_supervisor.receipt_observer();
        let mut next_launch = fixture.launch(&next_provider, next_host);
        next_launch.parent_message = "parent-after-release-failure".into();
        let resumed = next_supervisor
            .run(
                async {
                    call_resume(
                        &next_hook,
                        &next_observer,
                        "quarantined-resume",
                        resume_input(&child, &saved.expect("saved checkpoint")["checkpoint"]),
                    )
                    .await
                }
                .boxed_local(),
                next_launch,
            )
            .await;
        let after =
            vega_store::context_compaction::load_source(fixture.store.conn(), &child.agent_id)
                .expect("source after recreation");
        assert!(
            prepare_rejected && resumed.is_err(),
            "failed release is not proof of an abandoned process"
        );
        assert_eq!(next_provider.count(), 0);
        assert_eq!(
            before.fingerprint, after.fingerprint,
            "reload cannot normalize a quarantined writer"
        );
        assert_eq!(visible["ready_for_review"], false);
        assert_ne!(visible["release_state"], "released");
        assert!(
            rejected,
            "unconfirmed persistence/release must retain admission occupancy"
        );
        assert!(explicit_failure, "failure must wake wait explicitly");
    }
}

fn request_text(request: &ChatRequest) -> String {
    request
        .messages
        .iter()
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn original_history(
    fixture: &Fixture,
    child: &SubagentSnapshot,
) -> Vec<(String, String, String, i64)> {
    let mut query = fixture
        .store
        .conn()
        .prepare("SELECT id,role,content,created_at FROM messages WHERE thread_id=?1 ORDER BY seq")
        .expect("history query");
    query
        .query_map([&child.agent_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .expect("history")
        .collect::<Result<_, _>>()
        .expect("history rows")
}

#[tokio::test]
async fn interrupted_stage_resumes_same_child_after_parent_run_and_supervisor_recreation() {
    let fixture = Fixture::new();
    let (child, previous) = interrupt_with_checkpoint(&fixture).await;
    let old_history = original_history(&fixture, &child);
    let old_execution =
        vega_store::subagents::execution(fixture.store.conn(), &child.agent_id, &child.message_id)
            .expect("old execution")
            .expect("execution");
    new_parent_run(&fixture, "parent-resumed");
    let reopened = Store::open(&fixture.database).expect("new process store");
    let _ = crate::history::restart_history_page(&reopened, "parent", 20)
        .expect("restart parent history");
    let provider = ScriptedProvider::with(vec![final_response(
        "Continued saved work without replaying its Read or CI.",
    )]);
    let host = TestHost::new(true);
    let mut supervisor = Supervisor::new();
    let hook = supervisor.hook();
    let observer = supervisor.receipt_observer();
    let mut launch = fixture.launch(&provider, host.clone());
    launch.parent_message = "parent-resumed".into();
    let observed = supervisor
        .run(
            async {
                let visible = listed_agent(&list(&hook).await, &child.agent_id);
                let input = resume_input(&child, &visible["checkpoint"]);
                let result = call_resume(&hook, &observer, "resume-stage", input.clone()).await;
                let Some(next) = result.as_ref().ok().and_then(|value| {
                    serde_json::from_value::<SubagentSnapshot>(value["agent"].clone()).ok()
                }) else {
                    return (visible, result, None, None);
                };
                let duplicate = call_resume(&hook, &observer, "resume-stage", input)
                    .await
                    .ok();
                let terminal = bounded(wait(&hook, &next, None)).await;
                (visible, result, duplicate, Some(terminal))
            }
            .boxed_local(),
            launch,
        )
        .await;
    assert_eq!(previous["state"], "interrupted");
    assert!(
        !observed.0["checkpoint"].is_null(),
        "new parent run must expose durable child recovery identity"
    );
    let resumed = observed
        .1
        .expect("explicit cross-parent-run recovery must be admitted");
    assert_eq!(resumed["agent"]["agent_id"], child.agent_id);
    assert_ne!(resumed["agent"]["message_id"], child.message_id);
    assert_eq!(resumed["agent"]["parent_message_id"], "parent-resumed");
    assert_eq!(
        observed.2.expect("idempotent resume")["agent"]["message_id"],
        resumed["agent"]["message_id"]
    );
    assert_eq!(
        observed.3.expect("resumed terminal")["agents"][0]["state"],
        "completed"
    );
    assert_eq!(
        provider.count(),
        1,
        "resume must not replay completed tools or duplicate the admission"
    );
    let requests = provider.requests.lock().expect("requests");
    let text = request_text(&requests[0]);
    for required in [
        "Preserve the raw CRLF artifact",
        "do not repeat existing CI",
        "Independent review",
        "Final exact-version CI",
        &child.message_id,
        "read-artifact",
    ] {
        assert!(text.contains(required), "recovery packet lost {required}");
    }
    assert!(original_history(&fixture, &child).starts_with(&old_history));
    let old_after =
        vega_store::subagents::execution(fixture.store.conn(), &child.agent_id, &child.message_id)
            .expect("old after")
            .expect("old retained");
    assert_eq!(old_after.ordinal, old_execution.ordinal);
    assert_eq!(old_after.record.created_at, old_execution.record.created_at);
    assert_eq!(
        old_after.record.parent_message_id,
        old_execution.record.parent_message_id
    );
    let latest = vega_store::subagents::execution(
        fixture.store.conn(),
        &child.agent_id,
        resumed["agent"]["message_id"]
            .as_str()
            .expect("new message"),
    )
    .expect("new execution")
    .expect("new retained");
    assert_eq!(latest.ordinal, old_execution.ordinal + 1);
}

#[tokio::test]
async fn resume_rejects_stale_source_authority_artifact_and_pending_steering_before_dispatch() {
    for case in [
        "message",
        "checkpoint",
        "version",
        "fingerprint",
        "source_changed",
        "model",
        "permission",
        "workspace",
        "artifact_changed",
        "pending_steering",
        "parent_cancel",
        "malformed",
        "unrelated_parent",
    ] {
        let fixture = Fixture::new();
        let (child, previous) = interrupt_with_checkpoint(&fixture).await;
        let saved = previous["checkpoint"].clone();
        let mut input = resume_input(&child, &saved);
        match case {
            "message" => input["message_id"] = "not-the-current-execution".into(),
            "checkpoint" => input["checkpoint_id"] = "not-the-latest-checkpoint".into(),
            "version" => input["source_version"] = u64::MAX.into(),
            "fingerprint" => input["source_fingerprint"] = "0".repeat(64).into(),
            "malformed" => input["reconciliations"] = "not-an-array".into(),
            "source_changed" => insert_message(
                &fixture.store,
                &child.agent_id,
                "late-constraint",
                "user",
                "New user constraint must not be omitted.",
                "done",
            ),
            "artifact_changed" => std::fs::write(
                fixture.workspace.join("artifact.txt"),
                b"new unrelated version\n",
            )
            .expect("external edit"),
            "model" => {
                fixture
                    .store
                    .conn()
                    .execute(
                        "UPDATE threads SET model='other-model' WHERE id=?1",
                        [&child.agent_id],
                    )
                    .expect("authority mutation");
            }
            "permission" => {
                fixture
                    .store
                    .conn()
                    .execute(
                        "UPDATE threads SET permission_mode='auto' WHERE id=?1",
                        [&child.agent_id],
                    )
                    .expect("permission mutation");
            }
            "workspace" => {
                fixture
                    .store
                    .conn()
                    .execute(
                        "UPDATE threads SET project_id=NULL WHERE id=?1",
                        [&child.agent_id],
                    )
                    .expect("workspace mutation");
            }
            "pending_steering" => vega_store::pending_messages::insert(
                fixture.store.conn(),
                &vega_store::pending_messages::PendingMessageRow {
                    id: "pending-instruction".into(),
                    thread_id: child.agent_id.clone(),
                    ordinal: 0,
                    behavior: "steer".into(),
                    generation: child.message_id.clone(),
                    payload: json!({"content":"Retain the queued constraint.","images":[]})
                        .to_string(),
                    created_at: 2,
                },
            )
            .expect("pending user instruction"),
            _ => {}
        }
        let preserved = original_history(&fixture, &child);
        new_parent_run(&fixture, "parent-guarded");
        let provider = ScriptedProvider::default();
        let host = TestHost::new(true);
        let mut supervisor = Supervisor::new();
        let hook = supervisor.hook();
        let observer = supervisor.receipt_observer();
        let mut launch = fixture.launch(&provider, host.clone());
        launch.parent_message = "parent-guarded".into();
        if case == "unrelated_parent" {
            let parent = vega_store::threads::find(fixture.store.conn(), "parent")
                .expect("parent")
                .expect("exists");
            vega_store::threads::create(
                fixture.store.conn(),
                vega_store::threads::NewThread {
                    id: "unrelated-parent",
                    project_id: &parent.project_id,
                    title: "unrelated",
                    mode: "execute",
                    permission_mode: "confirm",
                    model: "fixture-model",
                    status: "active",
                    pinned: false,
                    unread: false,
                    created_at: 2,
                    updated_at: 2,
                },
            )
            .expect("unrelated parent");
            insert_message(
                &fixture.store,
                "unrelated-parent",
                "unrelated-user",
                "user",
                "An unrelated task.",
                "done",
            );
            insert_message(
                &fixture.store,
                "unrelated-parent",
                "unrelated-run",
                "assistant",
                "",
                "streaming",
            );
            launch.parent_thread = "unrelated-parent".into();
            launch.parent_message = "unrelated-run".into();
        }
        if case == "parent_cancel" {
            launch.cancel.cancel();
        }
        let result = supervisor
            .run(
                async { call_resume(&hook, &observer, "guarded-resume", input).await }
                    .boxed_local(),
                launch,
            )
            .await;
        assert!(
            !saved.is_null(),
            "{case}: precondition requires runtime interruption checkpoint"
        );
        assert!(result.is_err(), "{case}: invalid recovery must be rejected");
        assert_eq!(
            provider.count(),
            0,
            "{case}: rejected recovery must not dispatch child work"
        );
        assert_eq!(
            original_history(&fixture, &child),
            preserved,
            "{case}: failed admission must preserve source"
        );
    }
}

struct HeldOwnerLock(std::fs::File);

impl HeldOwnerLock {
    fn acquire(fixture: &Fixture, child: &SubagentSnapshot) -> Self {
        let directory = fixture
            .database
            .parent()
            .expect("database directory")
            .join("delegated-owner-locks");
        std::fs::create_dir_all(&directory).expect("private ownership directory");
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))
            .expect("private directory mode");
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(directory.join(format!("{}.lock", child.agent_id)))
            .expect("native ownership file");
        // SAFETY: file owns a live descriptor for the entire nonblocking flock call.
        assert_eq!(
            unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
            0,
            "separate OS descriptor must acquire inactive child lock"
        );
        Self(file)
    }
}

impl Drop for HeldOwnerLock {
    fn drop(&mut self) {
        // SAFETY: self retains the open descriptor until after unlocking it.
        assert_eq!(unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN) }, 0);
    }
}

#[tokio::test]
async fn actual_os_owner_lock_blocks_resume_then_atomic_admission_allows_only_one_writer() {
    let fixture = Fixture::new();
    let (child, previous) = interrupt_with_checkpoint(&fixture).await;
    let lock = HeldOwnerLock::acquire(&fixture, &child);
    new_parent_run(&fixture, "parent-lock");
    let provider = ScriptedProvider::default();
    let host = TestHost::new(true);
    let mut supervisor = Supervisor::new();
    let hook = supervisor.hook();
    let observer = supervisor.receipt_observer();
    let mut launch = fixture.launch(&provider, host.clone());
    launch.parent_message = "parent-lock".into();
    let observed = supervisor
        .run(
            async {
                let input = resume_input(&child, &previous["checkpoint"]);
                let blocked = call_resume(&hook, &observer, "blocked-owner", input.clone()).await;
                let blocked_count = provider.count();
                drop(lock);
                let (left, right) = futures::join!(
                    call_resume(&hook, &observer, "left-owner", input.clone()),
                    call_resume(&hook, &observer, "right-owner", input)
                );
                let admitted: Vec<_> = [&left, &right]
                    .into_iter()
                    .filter_map(|result| result.as_ref().ok())
                    .filter_map(|value| {
                        serde_json::from_value::<SubagentSnapshot>(value["agent"].clone()).ok()
                    })
                    .collect();
                if !admitted.is_empty() {
                    provider.request_count(1).await;
                    for (index, next) in admitted.iter().enumerate() {
                        let _ = bounded(hook.call(
                            format!("cancel-racing-{index}"),
                            "cancel_agent".into(),
                            json!({"agent_id":next.agent_id,"message_id":next.message_id}),
                            CancellationToken::new(),
                        ))
                        .await;
                    }
                    for next in &admitted {
                        let _ = bounded(wait(&hook, next, None)).await;
                    }
                }
                (blocked.is_err(), blocked_count, admitted.len())
            }
            .boxed_local(),
            launch,
        )
        .await;
    assert!(observed.0);
    assert_eq!(observed.1, 0, "ownership conflict must precede dispatch");
    assert_eq!(
        observed.2, 1,
        "exactly one explicit resume may win after OS ownership is released"
    );
    assert_eq!(provider.count(), 1);
}

fn inject_unknown_effect(fixture: &Fixture, child: &SubagentSnapshot, id: &str) {
    vega_store::tool_calls::insert_pending(
        fixture.store.conn(),
        vega_store::tool_calls::NewToolCall {
            id,
            thread_id: &child.agent_id,
            message_id: &child.message_id,
            seq: 9_999,
            tool: "bash",
            input_json: "{\"command\":\"printf previous-validation-run-reference\"}",
            status: "pending_approval",
            created_at: 2,
        },
    )
    .expect("crash-boundary external effect audit");
    let approval = crate::types::ApprovalAudit::from_json(
        "{\"decision\":\"once\",\"note\":null,\"source\":\"user\",\"danger\":null}",
    )
    .expect("valid prior approval")
    .to_json()
    .expect("approval encoding");
    vega_store::tool_calls::approve(fixture.store.conn(), id, &approval, None, 2)
        .expect("prior effect approval");
    vega_store::tool_calls::mark_running(fixture.store.conn(), id)
        .expect("crash at dispatched effect boundary");
}

#[tokio::test]
async fn ownership_proof_guards_history_restart_and_ordinary_pipeline_normalization() {
    for operation in ["parent_history", "child_history", "ordinary_prepare"] {
        let fixture = Fixture::new();
        let (child, _) = interrupt_with_checkpoint(&fixture).await;
        fixture
            .store
            .conn()
            .execute(
                "UPDATE delegated_executions SET state='running' WHERE child_thread_id=?1",
                [&child.agent_id],
            )
            .expect("recreated active owner record");
        fixture
            .store
            .conn()
            .execute(
                "UPDATE messages SET status='streaming' WHERE id=?1",
                [&child.message_id],
            )
            .expect("recreated active message");
        inject_unknown_effect(&fixture, &child, "still-running-effect");
        let lock = HeldOwnerLock::acquire(&fixture, &child);
        let source =
            vega_store::context_compaction::load_source(fixture.store.conn(), &child.agent_id)
                .expect("source before");
        let prepare_rejected = match operation {
            "parent_history" => {
                let _ = crate::history::restart_history_page(&fixture.store, "parent", 20);
                true
            }
            "child_history" => {
                let _ = crate::history::restart_history_page(&fixture.store, &child.agent_id, 20);
                true
            }
            _ => super::super::pipeline::prepare_run_with_images_and_reasoning(
                fixture.database.clone(),
                child.agent_id.clone(),
                "Unauthorized concurrent continuation".into(),
                "system".into(),
                "concurrent-user".into(),
                "concurrent-run".into(),
                PersistenceActorConfig::default(),
                false,
                None,
                None,
                Vec::new(),
            )
            .is_err(),
        };
        let after =
            vega_store::context_compaction::load_source(fixture.store.conn(), &child.agent_id)
                .expect("source after");
        let effect = audit(&fixture, "still-running-effect");
        let execution = vega_store::subagents::find(fixture.store.conn(), &child.agent_id)
            .expect("execution")
            .expect("child");
        drop(lock);
        assert!(
            prepare_rejected,
            "ordinary preparation must reject a different live owner"
        );
        assert_eq!(
            source.source_version, after.source_version,
            "{operation}: locked history cannot be normalized"
        );
        assert_eq!(
            source.fingerprint, after.fingerprint,
            "{operation}: locked source must be byte-identical"
        );
        assert_eq!(effect.status, "running");
        assert_eq!(execution.state, "running");
    }
}

#[tokio::test]
async fn unknown_external_effect_is_preserved_and_requires_explicit_evidence_without_replay() {
    let fixture = Fixture::new();
    std::fs::write(
        fixture.workspace.join("artifact.txt"),
        b"existing validation reference: previous-run\n",
    )
    .expect("existing evidence");
    let first = ScriptedProvider::with(vec![
        read_response("effect-evidence", "artifact.txt"),
        checkpoint_response(
            "effect-checkpoint",
            checkpoint(false, "artifact.txt", "effect-evidence"),
        ),
    ]);
    let first_host = TestHost::new(true);
    let mut original = Supervisor::new();
    let first_hook = original.hook();
    let first_observer = original.receipt_observer();
    let (child, terminal) = original
        .run(
            async {
                let child = spawn(
                    &first_hook,
                    &first_observer,
                    "Inspect the existing validation; never relaunch it.",
                )
                .await;
                first.request_count(3).await;
                inject_unknown_effect(&fixture, &child, "uncertain-external-call");
                let terminal = finish_observation(&first_hook, &first_host, &child)
                    .await
                    .expect("interrupted writer");
                (child, terminal["agents"][0].clone())
            }
            .boxed_local(),
            fixture.launch(&first, first_host.clone()),
        )
        .await;
    new_parent_run(&fixture, "parent-reconcile");
    let provider = ScriptedProvider::with(vec![final_response(
        "Used existing evidence; no external action was replayed.",
    )]);
    let host = TestHost::new(true);
    let mut supervisor = Supervisor::new();
    let hook = supervisor.hook();
    let observer = supervisor.receipt_observer();
    let mut launch = fixture.launch(&provider, host.clone());
    launch.parent_message = "parent-reconcile".into();
    let observed = supervisor.run(async {
        let input = resume_input(&child, &terminal["checkpoint"]);
        let missing = call_resume(&hook, &observer, "unreconciled", input.clone()).await;
        let mut fabricated = input.clone();
        fabricated["reconciliations"] = json!([{"call_id":"uncertain-external-call","evidence_call_ids":["nonexistent-evidence"],"resolution":"Claimed success without evidence."}]);
        let invalid = call_resume(&hook, &observer, "invalid-reconciliation", fabricated).await;
        let count_before = provider.count();
        let mut resolved = input;
        resolved["reconciliations"] = json!([{"call_id":"uncertain-external-call","evidence_call_ids":["effect-evidence"],"resolution":"Retain previous-run as an unresolved external validation reference. Do not replay; continue only independent inspection."}]);
        let result = call_resume(&hook, &observer, "reconciled", resolved).await;
        let final_state = if let Ok(value) = &result {
            let next: SubagentSnapshot = serde_json::from_value(value["agent"].clone()).expect("resumed child");
            Some(bounded(wait(&hook, &next, None)).await)
        } else { None };
        (missing.is_err(), invalid.is_err(), count_before, result, final_state)
    }.boxed_local(), launch).await;
    let effects = terminal["checkpoint"]["unknown_effects"]
        .as_array()
        .expect("interruption must preserve unknown effects before normalization");
    assert!(
        effects
            .iter()
            .any(|effect| effect["call_id"] == "uncertain-external-call"
                && effect["status"] == "running")
    );
    assert!(
        observed.0 && observed.1,
        "every unresolved effect needs real accessible evidence"
    );
    assert_eq!(observed.2, 0);
    assert!(
        observed.3.is_ok(),
        "explicit evidence-backed disposition permits independent continuation"
    );
    assert_eq!(
        observed.4.expect("continued task")["agents"][0]["state"],
        "completed"
    );
    assert_eq!(provider.count(), 1);
    let requests = provider.requests.lock().expect("requests");
    let text = request_text(&requests[0]);
    assert!(
        text.contains("uncertain-external-call")
            && text.contains("Do not replay")
            && text.contains("effect-evidence")
    );
    let count: i64 = fixture
        .store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM tool_calls WHERE thread_id=?1 AND tool='bash'",
            [&child.agent_id],
            |row| row.get(0),
        )
        .expect("external call count");
    assert_eq!(
        count, 1,
        "resume must retain original audit rather than redispatching its effect"
    );
}

#[tokio::test]
async fn interruption_without_declaration_preserves_all_users_images_partial_output_and_token_admission()
 {
    for small_budget in [false, true] {
        let fixture = Fixture::new();
        std::fs::write(
            fixture.workspace.join("artifact.txt"),
            b"unfinished real work\n",
        )
        .expect("artifact");
        let first = ScriptedProvider::with(vec![read_response("partial-read", "artifact.txt")]);
        let first_host = TestHost::new(true);
        let mut original = Supervisor::new();
        let first_hook = original.hook();
        let first_observer = original.receipt_observer();
        let mut encoded = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image::RgbImage::new(1, 1))
            .write_to(&mut encoded, image::ImageFormat::Png)
            .expect("explicit user image fixture");
        let image = vega_runtime::ImageAttachment::from_bytes(encoded.into_inner())
            .expect("valid user image");
        let constraint = "Preserve every user constraint and original image. ".repeat(1800);
        let (child, terminal) = original
            .run(
                async {
                    let child = spawn(
                        &first_hook,
                        &first_observer,
                        "Never treat interrupted work as ready.",
                    )
                    .await;
                    first.request_count(2).await;
                    insert_message(
                        &fixture.store,
                        &child.agent_id,
                        "retained-user",
                        "user",
                        &constraint,
                        "done",
                    );
                    vega_store::image_attachments::insert(
                        fixture.store.conn(),
                        "retained-user",
                        0,
                        image.bytes(),
                    )
                    .expect("persist user attachment");
                    let terminal = finish_observation(&first_hook, &first_host, &child)
                        .await
                        .expect("explicit interrupted terminal");
                    (child, terminal["agents"][0].clone())
                }
                .boxed_local(),
                fixture.launch(&first, first_host.clone()),
            )
            .await;
        let before = original_history(&fixture, &child);
        new_parent_run(&fixture, "parent-reconstruct");
        let provider = ScriptedProvider::with(vec![final_response(
            "Reconstructed the partial work from its original evidence.",
        )]);
        let host = TestHost::new(true);
        let mut supervisor = Supervisor::new();
        let hook = supervisor.hook();
        let observer = supervisor.receipt_observer();
        let mut launch = fixture.launch(&provider, host.clone());
        launch.parent_message = "parent-reconstruct".into();
        launch.settings.context_budget = if small_budget {
            Some(vega_runtime::ContextBudget::new(128, 1, true).expect("small inherited budget"))
        } else {
            None
        };
        let observed = supervisor
            .run(
                async {
                    let result = call_resume(
                        &hook,
                        &observer,
                        "reconstruct",
                        resume_input(&child, &terminal["checkpoint"]),
                    )
                    .await;
                    let final_state = if let Ok(value) = &result {
                        let next: SubagentSnapshot =
                            serde_json::from_value(value["agent"].clone()).expect("resumed child");
                        Some(bounded(wait(&hook, &next, None)).await)
                    } else {
                        None
                    };
                    (result, final_state)
                }
                .boxed_local(),
                launch,
            )
            .await;
        assert_eq!(terminal["ready_for_review"], false);
        assert_eq!(terminal["checkpoint"]["origin"], "interrupted");
        assert!(
            terminal["checkpoint"]["manifest"]["next_step"].is_null(),
            "system checkpoint cannot invent an intended next action"
        );
        assert!(original_history(&fixture, &child).starts_with(&before));
        assert_eq!(host.compactions.load(Ordering::SeqCst), 0);
        if small_budget {
            assert_eq!(
                provider.count(),
                0,
                "required recovery context must fail model admission without silent omission"
            );
            assert!(
                observed.0.is_err()
                    || observed
                        .1
                        .is_some_and(|value| value["agents"][0]["state"] == "failed")
            );
        } else {
            assert!(
                observed.0.is_ok(),
                "legacy unknown budget must permit explicit incomplete reconstruction"
            );
            assert_eq!(provider.count(), 1);
            let requests = provider.requests.lock().expect("requests");
            assert!(
                requests[0]
                    .messages
                    .iter()
                    .any(|message| message.role == ChatRole::User
                        && message.content == constraint
                        && message.images == vec![image.clone()])
            );
            let text = request_text(&requests[0]);
            assert!(
                text.contains("partial-read")
                    && text.contains("Read the actual artifact.")
                    && text.contains(&child.message_id)
            );
        }
    }
}

#[tokio::test]
async fn credential_protected_files_cannot_become_artifacts_even_with_a_claimed_read_reference() {
    let mut fixture = Fixture::new();
    let protected = fixture.workspace.join("protected");
    std::fs::create_dir(&protected).expect("credential root");
    std::fs::write(
        protected.join("secret.txt"),
        b"fixture protected material\n",
    )
    .expect("protected file");
    let secret_reads = Arc::new(AtomicUsize::new(0));
    let observed_reads = secret_reads.clone();
    let policy = vega_tools::ToolCredentialPolicy::new(
        protected,
        Vec::new(),
        Arc::new(move |_| {
            observed_reads.fetch_add(1, Ordering::SeqCst);
            Ok("unused".into())
        }),
    )
    .expect("real credential boundary");
    fixture.tools = vega_tools::Tools::new(&fixture.workspace)
        .expect("tools")
        .with_credential_policy(policy);
    let provider = ScriptedProvider::with(vec![
        read_response("credential-read", "protected/secret.txt"),
        checkpoint_response(
            "credential-stage",
            checkpoint(false, "protected/secret.txt", "credential-read"),
        ),
    ]);
    let host = TestHost::new(true);
    let mut supervisor = Supervisor::new();
    let hook = supervisor.hook();
    let observer = supervisor.receipt_observer();
    let observed = supervisor
        .run(
            async {
                let child = spawn(&hook, &observer, "Respect the credential boundary.").await;
                provider.request_count(3).await;
                let read = audit(&fixture, "credential-read");
                let checkpoint = checkpoint_output(&fixture, "credential-stage");
                let _ = finish_observation(&hook, &host, &child).await;
                (read, checkpoint)
            }
            .boxed_local(),
            fixture.launch(&provider, host.clone()),
        )
        .await;
    assert_ne!(observed.0.status, "success");
    assert!(observed.1.is_none());
    assert_eq!(secret_reads.load(Ordering::SeqCst), 0);
    assert!(
        !provider
            .requests
            .lock()
            .expect("requests")
            .iter()
            .any(|request| request_text(request).contains("fixture protected material"))
    );
}

async fn answer_permissions_until(
    host: &TestHost,
    finished: CancellationToken,
    decision: crate::types::PermissionDecision,
    answered: &AtomicUsize,
) {
    loop {
        tokio::select! {
            _ = finished.cancelled() => return,
            _ = host.until(|| host.admission.lock().expect("admission").as_ref().is_some_and(|admission| admission.permissions.has_pending())) => {},
        }
        if let Some(pending) = host.permissions().take_pending()
            && let Some((_, mut lease)) = pending.into_parts()
            && lease.respond(decision.clone())
        {
            answered.fetch_add(1, Ordering::SeqCst);
        }
    }
}

#[tokio::test]
async fn resumed_external_artifact_requires_current_read_permission_before_provider_dispatch() {
    let fixture = Fixture::new();
    let outside = tempfile::tempdir().expect("external workspace");
    let path = outside.path().join("external-artifact.txt");
    std::fs::write(&path, b"previous authorized file\n").expect("external artifact");
    let path = path.to_str().expect("external path");
    let first = ScriptedProvider::with(vec![
        read_response("external-evidence", path),
        checkpoint_response(
            "external-checkpoint",
            checkpoint(false, path, "external-evidence"),
        ),
    ]);
    let first_host = TestHost::new(true);
    let approved = AtomicUsize::new(0);
    let mut original = Supervisor::new();
    let first_hook = original.hook();
    let first_observer = original.receipt_observer();
    let done = CancellationToken::new();
    let (child, terminal) = original
        .run(
            async {
                let workflow = async {
                    let child = spawn(
                        &first_hook,
                        &first_observer,
                        "Preserve this explicitly authorized external artifact.",
                    )
                    .await;
                    first.request_count(3).await;
                    let terminal = finish_observation(&first_hook, &first_host, &child)
                        .await
                        .expect("interrupted stage");
                    done.cancel();
                    (child, terminal["agents"][0].clone())
                };
                let (result, ()) = futures::join!(
                    workflow,
                    answer_permissions_until(
                        &first_host,
                        done.clone(),
                        crate::types::PermissionDecision::Once,
                        &approved
                    )
                );
                result
            }
            .boxed_local(),
            fixture.launch(&first, first_host.clone()),
        )
        .await;
    new_parent_run(&fixture, "parent-external");
    let provider = ScriptedProvider::default();
    let host = TestHost::new(true);
    let denied = AtomicUsize::new(0);
    let done = CancellationToken::new();
    let mut supervisor = Supervisor::new();
    let hook = supervisor.hook();
    let observer = supervisor.receipt_observer();
    let mut launch = fixture.launch(&provider, host.clone());
    launch.parent_message = "parent-external".into();
    let observed = supervisor
        .run(
            async {
                let workflow = async {
                    let result = call_resume(
                        &hook,
                        &observer,
                        "external-resume",
                        resume_input(&child, &terminal["checkpoint"]),
                    )
                    .await;
                    let terminal = if let Ok(value) = &result {
                        let next: SubagentSnapshot = serde_json::from_value(value["agent"].clone())
                            .expect("resumed execution");
                        Some(
                            bounded(hook.call(
                                "external-wait".into(),
                                "wait_agent".into(),
                                wait_input(&next, None),
                                CancellationToken::new(),
                            ))
                            .await,
                        )
                    } else {
                        None
                    };
                    done.cancel();
                    (result, terminal)
                };
                let (result, ()) = futures::join!(
                    workflow,
                    answer_permissions_until(
                        &host,
                        done.clone(),
                        crate::types::PermissionDecision::Deny { note: None },
                        &denied
                    )
                );
                result
            }
            .boxed_local(),
            launch,
        )
        .await;
    assert!(
        approved.load(Ordering::SeqCst) >= 1,
        "initial file read must cross real permission queue"
    );
    assert!(!terminal["checkpoint"].is_null());
    assert!(
        denied.load(Ordering::SeqCst) >= 1,
        "prior one-time Read approval cannot authorize resume revalidation"
    );
    assert_eq!(provider.count(), 0);
    assert!(
        observed.0.is_err()
            || observed.1.is_some_and(|result| result.is_err()
                || result.is_ok_and(|value| value["agents"][0]["state"] == "failed"))
    );
}

#[tokio::test]
async fn unsafe_ownership_paths_fail_closed_without_changing_source_or_dispatching() {
    for case in [
        "symlink",
        "hardlink",
        "world_readable_directory",
        "world_readable_file",
    ] {
        let fixture = Fixture::new();
        let (child, terminal) = interrupt_with_checkpoint(&fixture).await;
        let lock = HeldOwnerLock::acquire(&fixture, &child);
        drop(lock);
        let directory = fixture
            .database
            .parent()
            .expect("database directory")
            .join("delegated-owner-locks");
        let path = directory.join(format!("{}.lock", child.agent_id));
        match case {
            "symlink" | "hardlink" => {
                std::fs::remove_file(&path).expect("replace inactive lock fixture");
                let target = fixture._directory.path().join("unrelated-lock-target");
                std::fs::write(&target, b"do not modify\n").expect("unrelated fixture");
                if case == "symlink" {
                    std::os::unix::fs::symlink(&target, &path).expect("unsafe symlink");
                } else {
                    std::fs::hard_link(&target, &path).expect("unsafe hardlink");
                }
            }
            "world_readable_directory" => {
                std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755))
                    .expect("unsafe directory mode")
            }
            _ => std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
                .expect("unsafe file mode"),
        }
        let before = original_history(&fixture, &child);
        new_parent_run(&fixture, "parent-unsafe-lock");
        let provider = ScriptedProvider::default();
        let host = TestHost::new(true);
        let mut supervisor = Supervisor::new();
        let hook = supervisor.hook();
        let observer = supervisor.receipt_observer();
        let mut launch = fixture.launch(&provider, host.clone());
        launch.parent_message = "parent-unsafe-lock".into();
        let result = supervisor
            .run(
                async {
                    call_resume(
                        &hook,
                        &observer,
                        "unsafe-lock-resume",
                        resume_input(&child, &terminal["checkpoint"]),
                    )
                    .await
                }
                .boxed_local(),
                launch,
            )
            .await;
        assert!(
            !terminal["checkpoint"].is_null(),
            "{case}: actual recovery precondition"
        );
        assert!(
            result.is_err(),
            "{case}: unavailable ownership proof must fail closed"
        );
        assert_eq!(provider.count(), 0);
        assert_eq!(original_history(&fixture, &child), before);
    }
}

#[path = "stage_steering_tests.rs"]
mod stage_steering_tests;

#[path = "lifecycle_ownership_tests.rs"]
mod lifecycle_ownership_tests;
