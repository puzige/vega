use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::sync::Semaphore;
use vega_runtime::{ChatMessage, ChatRequest, ChatRole, EventStream, ProviderEvent, StopReason};

struct Fixture {
    _directory: tempfile::TempDir,
    database: PathBuf,
    store: Store,
    tools: vega_tools::Tools,
    request: AgentRequest,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("temporary workspace");
        let database = directory.path().join("test.sqlite");
        let store = Store::open(&database).expect("test store");
        store.migrate().expect("temporary store schema");
        let project = vega_store::projects::create(
            store.conn(),
            directory.path().to_str().expect("workspace path"),
            "execution bounds",
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
        let tools = vega_tools::Tools::new(directory.path()).expect("tools");
        Self {
            _directory: directory,
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
        async move {
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

#[tokio::test]
async fn default_wait_suspends_across_former_timeout_without_releasing_active_child() {
    let fixture = Fixture::new();
    let provider = ScriptedProvider::default();
    let host = TestHost::new(true);
    let supervisor = Supervisor::new();
    let launch = fixture.launch(&provider, host);
    let reserved = supervisor
        .reserve(
            &launch,
            "spawn",
            "spawn_agent",
            &serde_json::json!({"name":"w","task":"work"}),
        )
        .expect("reserve");
    let hook = supervisor.hook();
    let pending = wait(&hook, &reserved.snapshot, None);
    tokio::pin!(pending);
    assert!(futures::poll!(&mut pending).is_pending());
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(61)).await;
    assert!(
        futures::poll!(&mut pending).is_pending(),
        "default wait must not wake the model after 30 or 60 seconds"
    );
    let mut terminal = reserved.snapshot.clone();
    terminal.state = SubagentState::Completed;
    supervisor
        .registry
        .update(terminal.clone())
        .expect("terminal");
    assert!(
        futures::poll!(&mut pending).is_pending(),
        "terminal persistence is not writer release"
    );
    supervisor.registry.finish(terminal, None);
    assert_eq!(pending.await["timed_out"], false);
}

fn reserve_wait_target(
    fixture: &Fixture,
    supervisor: &Supervisor,
    name: &str,
) -> ReservedExecution {
    supervisor
        .reserve(
            &fixture.launch(&ScriptedProvider::default(), TestHost::new(true)),
            name,
            "spawn_agent",
            &serde_json::json!({"name":name,"task":"bounded work"}),
        )
        .expect("wait target")
}

fn targets_input(targets: &[&SubagentSnapshot], timeout: u64) -> serde_json::Value {
    serde_json::json!({"targets":targets.iter().map(|target| serde_json::json!({"agent_id":target.agent_id,"message_id":target.message_id})).collect::<Vec<_>>(),"timeout_ms":timeout})
}

#[tokio::test]
async fn first_zero_and_finite_observations_preserve_each_new_execution_deadline() {
    let fixture = Fixture::new();
    let supervisor = Supervisor::new();
    let zero = reserve_wait_target(&fixture, &supervisor, "zero");
    let finite = reserve_wait_target(&fixture, &supervisor, "finite");
    let hook = supervisor.hook();
    let snapshot = bounded(wait(&hook, &zero.snapshot, Some(0))).await;
    assert_eq!(snapshot["timed_out"], true);
    assert_eq!(snapshot["timeout_coalesced"], false);
    assert_eq!(snapshot["wake_reason"], "timeout");
    tokio::time::pause();
    let pending = wait(&hook, &finite.snapshot, Some(500));
    tokio::pin!(pending);
    assert!(futures::poll!(&mut pending).is_pending());
    tokio::time::advance(Duration::from_millis(499)).await;
    assert!(futures::poll!(&mut pending).is_pending());
    tokio::time::advance(Duration::from_millis(1)).await;
    let receipt = bounded(pending).await;
    supervisor.registry.cancel_all();
    assert_eq!(receipt["timed_out"], true);
    assert_eq!(receipt["timeout_coalesced"], false);
    assert_eq!(receipt["wake_reason"], "timeout");
}

#[tokio::test]
async fn positive_and_zero_share_one_observation_despite_call_ids_deadlines_and_progress() {
    for (first_ms, repeated_ms) in [(0, 60_000), (500, 0), (500, 1_000), (0, 0)] {
        let fixture = Fixture::new();
        let supervisor = Supervisor::new();
        let reserved = reserve_wait_target(&fixture, &supervisor, "worker");
        let hook = supervisor.hook();
        tokio::time::pause();
        let first = hook.call(
            "first-observation".into(),
            "wait_agent".into(),
            wait_input(&reserved.snapshot, Some(first_ms)),
            CancellationToken::new(),
        );
        tokio::pin!(first);
        let first_receipt = if first_ms == 0 {
            bounded(first).await.expect("first snapshot")
        } else {
            assert!(futures::poll!(&mut first).is_pending());
            tokio::time::advance(Duration::from_millis(first_ms)).await;
            bounded(first).await.expect("first deadline")
        };
        assert_eq!(first_receipt["timed_out"], true);
        let listed = bounded(hook.call(
            "intervening-list".into(),
            "list_agents".into(),
            serde_json::json!({}),
            CancellationToken::new(),
        ))
        .await;
        assert!(listed.is_ok());
        let source = vega_store::context_compaction::load_source(fixture.store.conn(), "parent")
            .expect("read source");
        assert!(!source.messages.is_empty());
        let mut progress = reserved.snapshot.clone();
        progress.result = "New progress and tool activity are not a released result".into();
        supervisor
            .registry
            .update(progress)
            .expect("progress update");
        let repeated = hook.call(
            "different-call-id".into(),
            "wait_agent".into(),
            wait_input(&reserved.snapshot, Some(repeated_ms)),
            CancellationToken::new(),
        );
        tokio::pin!(repeated);
        assert!(
            futures::poll!(&mut repeated).is_pending(),
            "a second zero must also coalesce"
        );
        for _ in 0..4 {
            tokio::time::advance(Duration::from_secs(61)).await;
            assert!(
                futures::poll!(&mut repeated).is_pending(),
                "elapsed deadlines cannot renew the allowance"
            );
        }
        let mut terminal = reserved.snapshot.clone();
        terminal.state = SubagentState::Completed;
        supervisor
            .registry
            .update(terminal.clone())
            .expect("persisted terminal");
        assert!(
            futures::poll!(&mut repeated).is_pending(),
            "must still await host release"
        );
        supervisor.registry.finish(terminal, None);
        let receipt = bounded(repeated).await.expect("released terminal");
        supervisor.registry.cancel_all();
        tokio::time::resume();
        assert_eq!(receipt["timed_out"], false);
        assert_eq!(receipt["timeout_coalesced"], true);
        assert_eq!(receipt["wake_reason"], "terminal");
    }
}

#[tokio::test]
async fn allowance_validates_targets_before_claim_and_survives_reordering_subsets_and_new_targets()
{
    let fixture = Fixture::new();
    let supervisor = Supervisor::new();
    let a = reserve_wait_target(&fixture, &supervisor, "a");
    let b = reserve_wait_target(&fixture, &supervisor, "b");
    let c = reserve_wait_target(&fixture, &supervisor, "c");
    let hook = supervisor.hook();
    let mut unknown = b.snapshot.clone();
    unknown.message_id = "unknown-execution".into();
    assert!(
        bounded(hook.call(
            "invalid-target".into(),
            "wait_agent".into(),
            targets_input(&[&a.snapshot, &unknown], 0),
            CancellationToken::new()
        ))
        .await
        .is_err()
    );
    let valid_a = bounded(hook.call(
        "valid-a-after-invalid".into(),
        "wait_agent".into(),
        targets_input(&[&a.snapshot], 0),
        CancellationToken::new(),
    ))
    .await
    .expect("invalid peer did not consume valid target allowance");
    assert_eq!(valid_a["timeout_coalesced"], false);
    let first = bounded(hook.call(
        "valid-targets".into(),
        "wait_agent".into(),
        targets_input(&[&a.snapshot, &b.snapshot], 0),
        CancellationToken::new(),
    ))
    .await
    .expect("first snapshot");
    assert_eq!(first["timeout_coalesced"], false);
    tokio::time::pause();
    for targets in [
        vec![&b.snapshot, &a.snapshot],
        vec![&a.snapshot],
        vec![&b.snapshot],
    ] {
        let cancel = CancellationToken::new();
        let repeated = hook.call(
            "reordered-subset".into(),
            "wait_agent".into(),
            targets_input(&targets, 500),
            cancel.clone(),
        );
        tokio::pin!(repeated);
        assert!(futures::poll!(&mut repeated).is_pending());
        tokio::time::advance(Duration::from_millis(501)).await;
        assert!(
            futures::poll!(&mut repeated).is_pending(),
            "target arrays do not own allowances"
        );
        cancel.cancel();
        assert!(matches!(bounded(repeated).await, Err(VegaError::Cancelled)));
    }
    let mixed = hook.call(
        "new-target".into(),
        "wait_agent".into(),
        targets_input(&[&a.snapshot, &c.snapshot], 500),
        CancellationToken::new(),
    );
    tokio::pin!(mixed);
    assert!(futures::poll!(&mut mixed).is_pending());
    tokio::time::advance(Duration::from_millis(499)).await;
    assert!(futures::poll!(&mut mixed).is_pending());
    tokio::time::advance(Duration::from_millis(1)).await;
    let receipt = bounded(mixed)
        .await
        .expect("genuinely new execution deadline");
    assert_eq!(receipt["timed_out"], true);
    assert_eq!(receipt["timeout_coalesced"], false);
    let repeated = hook.call(
        "new-now-consumed".into(),
        "wait_agent".into(),
        targets_input(&[&c.snapshot], 0),
        CancellationToken::new(),
    );
    tokio::pin!(repeated);
    assert!(futures::poll!(&mut repeated).is_pending());
    supervisor.registry.cancel_all();
    assert!(bounded(repeated).await.is_err());
}

#[tokio::test]
async fn concurrent_waits_cannot_both_claim_the_same_execution_allowance() {
    let fixture = Fixture::new();
    let supervisor = Supervisor::new();
    let reserved = reserve_wait_target(&fixture, &supervisor, "worker");
    let hook = supervisor.hook();
    tokio::time::pause();
    let first = hook.call(
        "concurrent-a".into(),
        "wait_agent".into(),
        wait_input(&reserved.snapshot, Some(500)),
        CancellationToken::new(),
    );
    let second = hook.call(
        "concurrent-b".into(),
        "wait_agent".into(),
        wait_input(&reserved.snapshot, Some(500)),
        CancellationToken::new(),
    );
    let race = futures::future::select(first, second);
    tokio::pin!(race);
    assert!(futures::poll!(&mut race).is_pending());
    tokio::time::advance(Duration::from_millis(500)).await;
    let (receipt, mut remaining) = match bounded(race).await {
        futures::future::Either::Left(pair) | futures::future::Either::Right(pair) => pair,
    };
    assert_eq!(
        receipt.expect("one finite observer")["timeout_coalesced"],
        false
    );
    assert!(
        futures::poll!(&mut remaining).is_pending(),
        "the other concurrent observer must coalesce"
    );
    tokio::time::advance(Duration::from_secs(61)).await;
    assert!(futures::poll!(&mut remaining).is_pending());
    supervisor.registry.cancel_all();
    assert!(bounded(remaining).await.is_err());
}

#[tokio::test]
async fn delivered_terminal_does_not_rewake_mixed_pending_targets_but_new_release_does() {
    let fixture = Fixture::new();
    let supervisor = Supervisor::new();
    let a = reserve_wait_target(&fixture, &supervisor, "a");
    let b = reserve_wait_target(&fixture, &supervisor, "b");
    let hook = supervisor.hook();
    let initial = bounded(hook.call(
        "initial".into(),
        "wait_agent".into(),
        targets_input(&[&a.snapshot, &b.snapshot], 0),
        CancellationToken::new(),
    ))
    .await
    .expect("snapshot");
    assert_eq!(initial["timed_out"], true);
    let mut first_terminal = a.snapshot.clone();
    first_terminal.state = SubagentState::Completed;
    supervisor.registry.finish(first_terminal, None);
    assert_eq!(
        bounded(wait(&hook, &a.snapshot, None)).await["timed_out"],
        false
    );
    let repeated = hook.call(
        "mixed-old-terminal".into(),
        "wait_agent".into(),
        targets_input(&[&a.snapshot, &b.snapshot], 0),
        CancellationToken::new(),
    );
    tokio::pin!(repeated);
    assert!(
        futures::poll!(&mut repeated).is_pending(),
        "already delivered terminal must not wake the running target"
    );
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(121)).await;
    assert!(futures::poll!(&mut repeated).is_pending());
    let mut second_terminal = b.snapshot.clone();
    second_terminal.state = SubagentState::Completed;
    supervisor
        .registry
        .update(second_terminal.clone())
        .expect("terminal persistence");
    assert!(futures::poll!(&mut repeated).is_pending());
    supervisor.registry.finish(second_terminal, None);
    let receipt = bounded(repeated).await.expect("new released terminal");
    assert_eq!(receipt["timeout_coalesced"], true);
    assert_eq!(receipt["wake_reason"], "terminal");
    let cached = bounded(hook.call(
        "all-terminal".into(),
        "wait_agent".into(),
        targets_input(&[&b.snapshot, &a.snapshot], 0),
        CancellationToken::new(),
    ))
    .await
    .expect("all terminal cached results");
    supervisor.registry.cancel_all();
    assert_eq!(cached["timed_out"], false);
    assert_eq!(cached["agents"].as_array().expect("targets").len(), 2);
}

#[tokio::test]
async fn wait_cancellation_and_parent_end_release_waiters_without_success() {
    for timeout in [None, Some(0), Some(60_000)] {
        let fixture = Fixture::new();
        let supervisor = Supervisor::new();
        let reserved = reserve_wait_target(&fixture, &supervisor, "worker");
        let hook = supervisor.hook();
        assert_eq!(
            bounded(wait(&hook, &reserved.snapshot, Some(0))).await["timed_out"],
            true
        );
        let cancel = CancellationToken::new();
        let pending = hook.call(
            "wait".into(),
            "wait_agent".into(),
            wait_input(&reserved.snapshot, timeout),
            cancel.clone(),
        );
        tokio::pin!(pending);
        assert!(futures::poll!(&mut pending).is_pending());
        cancel.cancel();
        assert!(matches!(bounded(pending).await, Err(VegaError::Cancelled)));
        let ended = hook.call(
            "ended".into(),
            "wait_agent".into(),
            wait_input(&reserved.snapshot, timeout),
            CancellationToken::new(),
        );
        tokio::pin!(ended);
        assert!(
            futures::poll!(&mut ended).is_pending(),
            "cancelled observation must not refund allowance"
        );
        supervisor.registry.cancel_all();
        assert!(bounded(ended).await.is_err());
    }
    for repeated_timeout in [0, 500] {
        let fixture = Fixture::new();
        let supervisor = Supervisor::new();
        let reserved = reserve_wait_target(&fixture, &supervisor, "cancelled-first-observation");
        let hook = supervisor.hook();
        let cancel = CancellationToken::new();
        let first = hook.call(
            "first-finite-cancelled".into(),
            "wait_agent".into(),
            wait_input(&reserved.snapshot, Some(500)),
            cancel.clone(),
        );
        tokio::pin!(first);
        assert!(futures::poll!(&mut first).is_pending());
        cancel.cancel();
        assert!(matches!(bounded(first).await, Err(VegaError::Cancelled)));
        let repeated = hook.call(
            "after-first-cancellation".into(),
            "wait_agent".into(),
            wait_input(&reserved.snapshot, Some(repeated_timeout)),
            CancellationToken::new(),
        );
        tokio::pin!(repeated);
        let first_poll = futures::poll!(&mut repeated);
        let starts_pending = first_poll.is_pending();
        let mut remains_pending = false;
        if starts_pending {
            tokio::time::pause();
            tokio::time::advance(Duration::from_millis(501)).await;
            let after_deadline = futures::poll!(&mut repeated);
            remains_pending = after_deadline.is_pending();
            tokio::time::resume();
        }
        supervisor.registry.cancel_all();
        if remains_pending {
            assert!(bounded(repeated).await.is_err());
        }
        assert!(
            starts_pending && remains_pending,
            "cancelling the first finite wait cannot refund its allowance to zero or positive waits"
        );
    }
}

#[tokio::test]
async fn active_stage_budget_cancels_then_drains_host_release_before_wait_returns() {
    let fixture = Fixture::new();
    let provider = ScriptedProvider::default();
    let host = TestHost::new(false);
    let mut supervisor = Supervisor::new();
    let hook = supervisor.hook();
    let observer = supervisor.receipt_observer();
    let registry = supervisor.registry.clone();
    let parent = async {
        let snapshot = spawn(&hook, &observer, "Produce a reviewable patch").await;
        provider.request_count(1).await;
        let waiting = wait(&hook, &snapshot, None);
        tokio::pin!(waiting);
        assert!(futures::poll!(&mut waiting).is_pending());
        tokio::time::pause();
        tokio::time::advance(Duration::from_secs(20 * 60 + 1)).await;
        tokio::task::yield_now().await;
        let expired = host.cancel().is_cancelled();
        tokio::time::resume();
        if !expired {
            host.cancel().cancel();
        }
        host.until(|| host.release_started.load(Ordering::SeqCst))
            .await;
        let polled = futures::poll!(&mut waiting);
        let draining = polled.is_pending();
        let key = (snapshot.agent_id.clone(), snapshot.message_id.clone());
        let occupied = !registry
            .released_terminal(std::slice::from_ref(&key))
            .expect("release state");
        host.release_gate.add_permits(1);
        let receipt = match polled {
            std::task::Poll::Ready(value) => value,
            std::task::Poll::Pending => bounded(&mut waiting).await,
        };
        (expired, draining, occupied, receipt)
    };
    let (expired, draining, occupied, receipt) = supervisor
        .run(
            parent.boxed_local(),
            fixture.launch(&provider, host.clone()),
        )
        .await;
    assert!(expired, "active stage must be bounded");
    assert!(
        draining && occupied,
        "cancellation must drain the original host release future"
    );
    assert_eq!(receipt["agents"][0]["state"], "interrupted");
    assert!(
        receipt["agents"][0]["result"]
            .as_str()
            .expect("result")
            .contains("budget")
    );
    assert_eq!(receipt["agents"][0]["ready"], false);
    assert_eq!(provider.requests.lock().expect("requests").len(), 1);
}

#[tokio::test]
async fn failed_release_wakes_default_wait_but_blocks_followup_and_every_new_admission() {
    for persistence_failure in [false, true] {
        let fixture = Fixture::new();
        let provider = if persistence_failure {
            ScriptedProvider::default()
        } else {
            ScriptedProvider::with(vec![final_response("final patch receipt")])
        };
        let host = TestHost::new(true);
        host.release_failed
            .store(!persistence_failure, Ordering::SeqCst);
        let mut supervisor = Supervisor::new();
        let hook = supervisor.hook();
        let observer = supervisor.receipt_observer();
        let registry = supervisor.registry.clone();
        let parent = async {
            let snapshot = spawn(&hook, &observer, "Produce a patch").await;
            if persistence_failure {
                provider.request_count(1).await;
                fixture
                    .store
                    .conn()
                    .execute(
                        "UPDATE delegated_executions SET state='failed' WHERE child_message_id=?1",
                        [&snapshot.message_id],
                    )
                    .expect("inject failed terminal CAS");
                host.cancel().cancel();
            }
            let receipt = bounded(hook.call(
                "wait-failure".into(),
                "wait_agent".into(),
                wait_input(&snapshot, None),
                CancellationToken::new(),
            ))
            .await;
            let key = (snapshot.agent_id.clone(), snapshot.message_id.clone());
            let occupied = !registry
                .released_terminal(std::slice::from_ref(&key))
                .expect("release state");
            let followup = bounded(hook.call("followup".into(), "followup_agent".into(), serde_json::json!({"agent_id":snapshot.agent_id,"message_id":snapshot.message_id,"task":"continue"}), CancellationToken::new())).await;
            let replacement = bounded(hook.call(
                "replacement".into(),
                "spawn_agent".into(),
                serde_json::json!({"name":"another-writer","task":"new work"}),
                CancellationToken::new(),
            ))
            .await;
            (receipt, occupied, followup.is_err(), replacement.is_err())
        };
        let (receipt, occupied, followup_rejected, new_rejected) = supervisor
            .run(
                parent.boxed_local(),
                fixture.launch(&provider, host.clone()),
            )
            .await;
        assert!(
            occupied,
            "failed persistence/release must preserve occupation"
        );
        assert!(
            followup_rejected && new_rejected,
            "uncertain ownership blocks all new admissions"
        );
        assert!(
            receipt.is_err() || receipt.is_ok_and(|value| value["agents"][0]["state"] == "failed"),
            "default wait must report explicit failure"
        );
    }
}

#[tokio::test]
async fn cancelled_child_preserves_partial_evidence_and_never_claims_completion() {
    let fixture = Fixture::new();
    let provider = ScriptedProvider::with(vec![vec![
        ProviderEvent::TextDelta("partial artifact: revision abc; validation pending".into()),
        ProviderEvent::ToolUse {
            id: "read-status".into(),
            name: "read".into(),
            input_json: serde_json::json!({"path":"missing.txt"}).to_string(),
        },
        ProviderEvent::Done {
            stop_reason: StopReason::ToolUse,
        },
    ]]);
    let host = TestHost::new(true);
    let mut supervisor = Supervisor::new();
    let hook = supervisor.hook();
    let observer = supervisor.receipt_observer();
    let parent = async {
        let snapshot = spawn(&hook, &observer, "work").await;
        provider.request_count(2).await;
        host.cancel().cancel();
        let receipt = bounded(wait(&hook, &snapshot, None)).await;
        assert_eq!(receipt["agents"][0]["state"], "interrupted");
        assert!(
            receipt["agents"][0]["result"]
                .as_str()
                .expect("result")
                .contains("partial artifact")
        );
        assert_eq!(receipt["agents"][0]["ready"], false);
    };
    supervisor
        .run(
            parent.boxed_local(),
            fixture.launch(&provider, host.clone()),
        )
        .await;
}

#[tokio::test]
async fn child_provider_entry_methods_forward_legal_large_requests_without_new_byte_quotas() {
    let provider = ScriptedProvider::default();
    let scoped = ChildProvider {
        inner: &provider,
        reader: None,
    };
    let content = "界".repeat(90 * 1024);
    let request = ChatRequest {
        messages: vec![ChatMessage::new(ChatRole::User, content.clone())],
        ..Default::default()
    };
    assert!(
        scoped
            .chat_stream(request.clone(), CancellationToken::new())
            .await
            .is_ok()
    );
    assert!(
        scoped
            .chat_stream_once(request.clone(), CancellationToken::new())
            .await
            .is_ok()
    );
    assert!(
        scoped
            .chat_stream_recovering(request, CancellationToken::new())
            .await
            .is_ok()
    );
    let requests = provider.requests.lock().expect("requests");
    assert_eq!(requests.len(), 3);
    for request in requests.iter() {
        assert_eq!(request.messages.len(), 1);
        assert_eq!(request.messages[0].content, content);
    }
}

fn fixture_context_budget(input_limit: u64, automatic: bool) -> vega_runtime::ContextBudget {
    let policy = vega_store::context_compaction::ModelContextPolicy {
        provider: "fixture-provider".into(),
        model: "fixture-model".into(),
        input_limit: Some(input_limit),
        output_reserve: Some(4096),
        automatic_compaction: automatic,
        updated_at: 1,
    };
    let budget = super::super::compaction::model_context_budget(&policy)
        .expect("persisted input policy conversion")
        .expect("configured policy");
    assert_eq!(budget.input_budget(), input_limit);
    assert_eq!(budget.total_limit(), input_limit + 4096);
    assert_eq!(budget.output_reserve(), 4096);
    budget
}

async fn native_context_stage(
    system: &str,
    budget: Option<vega_runtime::ContextBudget>,
    scripts: Vec<Vec<ProviderEvent>>,
) -> (Vec<ChatRequest>, serde_json::Value) {
    let fixture = Fixture::new();
    fs::write(
        fixture._directory.path().join("context-evidence.txt"),
        "retained tool result ".repeat(3000),
    )
    .expect("owned read fixture");
    let provider = ScriptedProvider::with(scripts);
    let host = TestHost::new(true);
    let mut supervisor = Supervisor::new();
    let hook = supervisor.hook();
    let observer = supervisor.receipt_observer();
    let mut launch = fixture.launch(&provider, host);
    launch.settings.system_prompt = system.into();
    launch.settings.context_budget = budget;
    launch.settings.authority.permission_mode = RuntimePermissionMode::FullAccess;
    let parent = async {
        let snapshot = spawn(&hook, &observer, "Retain this exact user constraint").await;
        bounded(wait(&hook, &snapshot, None)).await
    };
    let receipt = supervisor.run(parent.boxed_local(), launch).await;
    let requests = provider.requests.lock().expect("requests").clone();
    (requests, receipt)
}

#[tokio::test]
async fn native_entry_token_admission_counts_complete_system_and_tool_schemas_and_preserves_none() {
    let system = format!("UNTRUNCATED_SYSTEM {}", "context ".repeat(40_000));
    let (large, accepted) = native_context_stage(
        &system,
        Some(fixture_context_budget(1_000_000, false)),
        vec![final_response("accepted intact")],
    )
    .await;
    assert_eq!(accepted["agents"][0]["state"], "completed");
    assert_eq!(large.len(), 1);
    let request = &large[0];
    assert!(
        request
            .messages
            .iter()
            .any(|message| message.role == ChatRole::System && message.content.contains(&system))
    );
    assert!(
        request
            .messages
            .iter()
            .any(|message| message.role == ChatRole::User
                && message.content == "Retain this exact user constraint")
    );
    assert!(!request.tools.is_empty());
    let estimate = vega_runtime::estimate_wire_context(&request.messages, &request.tools)
        .expect("assembled input");
    let without_tools = vega_runtime::estimate_wire_context(&request.messages, &[])
        .expect("schema-free comparison");
    assert!(without_tools.input_tokens < estimate.input_tokens);
    assert!(
        request
            .messages
            .iter()
            .map(|message| message.content.len())
            .sum::<usize>()
            > 256 * 1024
    );
    let (small, rejected) = native_context_stage(
        &system,
        Some(fixture_context_budget(estimate.input_tokens - 1, false)),
        vec![final_response("must not dispatch")],
    )
    .await;
    assert!(
        small.is_empty(),
        "the same assembled input exceeds its configured token allowance"
    );
    assert_ne!(rejected["agents"][0]["state"], "completed");
    let (exact, accepted) = native_context_stage(
        &system,
        Some(fixture_context_budget(estimate.input_tokens, false)),
        vec![final_response("exact input allowance")],
    )
    .await;
    assert_eq!(
        exact.len(),
        1,
        "input_limit is not reduced by the output reserve a second time"
    );
    assert_eq!(exact[0].max_tokens, request.max_tokens);
    assert_eq!(accepted["agents"][0]["state"], "completed");
    let (legacy, accepted) = native_context_stage(
        &system,
        None,
        vec![final_response("legacy capacity unknown")],
    )
    .await;
    assert_eq!(
        legacy.len(),
        1,
        "None retains legacy admission without an invented capacity"
    );
    assert_eq!(accepted["agents"][0]["state"], "completed");
    assert!(
        legacy[0]
            .messages
            .iter()
            .any(|message| message.content.contains(&system))
    );
}

#[tokio::test]
async fn native_current_stage_token_admission_counts_json_arguments_and_real_tool_results() {
    async fn run_case(input_limit: u64, content: &str) -> (Vec<ChatRequest>, serde_json::Value) {
        let mut fixture = Fixture::new();
        let workspace = fixture._directory.path().join("workspace");
        fs::create_dir(&workspace).expect("owned workspace");
        assert_eq!(
            fixture
                .store
                .conn()
                .execute(
                    "UPDATE projects SET path = ?1 WHERE id = (SELECT project_id FROM threads WHERE id = 'parent')",
                    [workspace.to_str().expect("workspace path")],
                )
                .expect("separate project from database and checkpoints"),
            1
        );
        fixture.tools = vega_tools::Tools::new(&workspace).expect("isolated tools");
        fs::write(
            workspace.join("context-evidence.txt"),
            "retained tool result ".repeat(3000),
        )
        .expect("owned read fixture");
        let write_input = serde_json::json!({"path":"context-created.txt","content":content});
        assert!(write_input.to_string().len() < 256 * 1024);
        let provider = ScriptedProvider::with(vec![
            tool_response(
                "Produce the scoped file",
                "context-write",
                "write",
                write_input,
            ),
            tool_response(
                "Read retained evidence",
                "context-read",
                "read",
                serde_json::json!({"path":"context-evidence.txt"}),
            ),
            final_response("tool context accepted"),
        ]);
        let mut supervisor = Supervisor::new();
        let hook = supervisor.hook();
        let observer = supervisor.receipt_observer();
        let mut launch = fixture.launch(&provider, TestHost::new(true));
        launch.settings.system_prompt = "Retain all arguments and tool evidence".into();
        launch.settings.context_budget = Some(fixture_context_budget(input_limit, false));
        launch.settings.authority.permission_mode = RuntimePermissionMode::FullAccess;
        let parent = async {
            let snapshot = spawn(&hook, &observer, "Retain this exact user constraint").await;
            bounded(wait(&hook, &snapshot, None)).await
        };
        let receipt = supervisor.run(parent.boxed_local(), launch).await;
        let requests = provider.requests.lock().expect("requests").clone();
        assert_eq!(
            fs::read_to_string(workspace.join("context-created.txt"))
                .unwrap_or_else(|error| panic!("write failed: {error}; receipt={receipt}")),
            content
        );
        let audit = vega_store::tool_calls::find_state(fixture.store.conn(), "context-write")
            .expect("stored write")
            .expect("write audit");
        assert_eq!(audit.status, "success", "receipt={receipt}");
        assert!(!audit.input_json.contains(content));
        assert!(
            serde_json::from_str::<serde_json::Value>(&audit.input_json)
                .expect("audit JSON")
                .get("content")
                .is_none()
        );
        (requests, receipt)
    }

    let content = "written content ".repeat(8000);
    let (large, accepted) = run_case(1_000_000, &content).await;
    assert_eq!(
        accepted["agents"][0]["state"], "completed",
        "receipt={accepted}"
    );
    assert_eq!(large.len(), 3);
    let second = &large[1];
    let third = &large[2];
    let second_wire = vega_runtime::project_model_messages(&second.messages)
        .expect("actual model tool projection");
    let write_call = second_wire
        .iter()
        .flat_map(|message| &message.tool_calls)
        .find(|call| call.id == "context-write")
        .expect("live write arguments");
    let write_input: serde_json::Value =
        serde_json::from_str(&write_call.input_json).expect("wire write JSON");
    assert_eq!(write_input["content"], content);
    assert!(write_call.input_json.len() < 256 * 1024);
    assert!(
        third
            .messages
            .iter()
            .any(|message| message.role == ChatRole::Tool
                && message.tool_call_id.as_deref() == Some("context-read")
                && message.content.contains("retained tool result"))
    );
    let first_tokens = vega_runtime::estimate_wire_context(&large[0].messages, &large[0].tools)
        .expect("first input")
        .input_tokens;
    let second_tokens = vega_runtime::estimate_wire_context(&second.messages, &second.tools)
        .expect("JSON input")
        .input_tokens;
    let third_tokens = vega_runtime::estimate_wire_context(&third.messages, &third.tools)
        .expect("tool result input")
        .input_tokens;
    assert!(first_tokens < second_tokens && second_tokens < third_tokens);
    for (input_limit, allowed_calls) in [(second_tokens - 1, 1), (third_tokens - 1, 2)] {
        let (requests, rejected) = run_case(input_limit, &content).await;
        assert_eq!(
            requests.len(),
            allowed_calls,
            "the over-budget next request never reaches the provider; receipt={rejected}"
        );
        assert_ne!(rejected["agents"][0]["state"], "completed");
    }
}

#[tokio::test]
async fn native_followup_excludes_old_progress_and_tools_but_preserves_users_final_and_storage() {
    let fixture = Fixture::new();
    let progress = "old-progress-only ".repeat(2200);
    fs::write(
        fixture._directory.path().join("evidence.txt"),
        "original read evidence",
    )
    .expect("read fixture");
    let provider = ScriptedProvider::with(vec![
        tool_response(
            &progress,
            "old-read",
            "read",
            serde_json::json!({"path":"evidence.txt"}),
        ),
        final_response("FINAL: revision abc is ready for independent review; CI pending"),
        tool_response(
            "Verify unchanged call identity",
            "old-read",
            "read",
            serde_json::json!({"path":"evidence.txt"}),
        ),
        final_response("followup finished"),
    ]);
    let host = TestHost::new(true);
    let mut supervisor = Supervisor::new();
    let hook = supervisor.hook();
    let observer = supervisor.receipt_observer();
    let parent = async {
        let initial = spawn(
            &hook,
            &observer,
            "Original constraint: preserve independent review and CI",
        )
        .await;
        let first_observation = bounded(wait(&hook, &initial, Some(0))).await;
        assert_eq!(first_observation["timeout_coalesced"], false);
        let first = bounded(wait(&hook, &initial, Some(60_000))).await;
        assert_eq!(first["timeout_coalesced"], true);
        let stored_before = messages::find(fixture.store.conn(), &initial.message_id)
            .expect("stored source")
            .expect("source exists");
        fs::write(
            fixture._directory.path().join("evidence.txt"),
            "changed file must not replace deduplicated evidence",
        )
        .expect("change fixture");
        let next = followup(
            &hook,
            &observer,
            &initial,
            "Current constraint: fix only the discovered defect",
        )
        .await;
        assert_eq!(initial.agent_id, next.agent_id);
        assert_ne!(initial.message_id, next.message_id);
        let next_observation = bounded(wait(&hook, &next, Some(0))).await;
        assert_eq!(
            next_observation["timeout_coalesced"], false,
            "actual followup gets a new execution allowance"
        );
        let second = bounded(wait(&hook, &next, Some(60_000))).await;
        assert_eq!(second["timeout_coalesced"], true);
        let requests = provider.requests.lock().expect("requests");
        let input = &requests.get(2).expect("followup entry request").messages;
        let after_reuse = &requests
            .get(3)
            .expect("followup after reused call")
            .messages;
        assert!(
            after_reuse
                .iter()
                .any(|message| message.role == ChatRole::Tool
                    && message.content.contains("original read evidence"))
        );
        assert!(
            !after_reuse
                .iter()
                .any(|message| message.content.contains("changed file must not"))
        );
        let users: Vec<_> = input
            .iter()
            .filter(|message| message.role == ChatRole::User)
            .map(|message| message.content.as_str())
            .collect();
        assert!(
            users
                .iter()
                .any(|text| text.contains("Original constraint"))
        );
        assert!(users.iter().any(|text| text.contains("Current constraint")));
        let original_index = users
            .iter()
            .position(|text| text.contains("Original constraint"))
            .expect("original");
        let current_index = users
            .iter()
            .position(|text| text.contains("Current constraint"))
            .expect("current");
        assert!(original_index < current_index);
        assert!(
            input
                .iter()
                .all(|message| !message.content.contains("old-progress-only")
                    && message.tool_calls.is_empty()
                    && message.role != ChatRole::Tool)
        );
        assert!(
            input
                .iter()
                .any(|message| message.role == ChatRole::Assistant
                    && message.content.contains("FINAL: revision abc"))
        );
        let joined = input
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains(&initial.agent_id) && joined.contains(&initial.message_id));
        assert_eq!(first["agents"][0]["state"], "completed");
        assert!(
            first["agents"][0]["result"]
                .as_str()
                .expect("final receipt")
                .contains("FINAL: revision abc")
        );
        assert_eq!(second["agents"][0]["state"], "completed");
        let stored_after = messages::find(fixture.store.conn(), &initial.message_id)
            .expect("source after")
            .expect("source exists");
        assert_eq!(stored_before.content, stored_after.content);
        assert!(stored_after.content.contains(&progress));
        let old_calls: i64 = fixture
            .store
            .conn()
            .query_row(
                "SELECT count(*) FROM tool_calls WHERE message_id = ?1",
                [&initial.message_id],
                |row| row.get(0),
            )
            .expect("retained tool calls");
        assert_eq!(old_calls, 1);
    };
    supervisor
        .run(parent.boxed_local(), fixture.launch(&provider, host))
        .await;
}

async fn native_followup_context_case(input_limit: u64) -> (Vec<ChatRequest>, serde_json::Value) {
    let fixture = Fixture::new();
    let provider = ScriptedProvider::with(vec![
        final_response(&"界".repeat(6000)),
        final_response("full user constraints accepted"),
    ]);
    let host = TestHost::new(true);
    let mut supervisor = Supervisor::new();
    let hook = supervisor.hook();
    let observer = supervisor.receipt_observer();
    let mut launch = fixture.launch(&provider, host);
    launch.settings.context_budget = Some(fixture_context_budget(input_limit, false));
    let parent = async {
        let initial = spawn(&hook, &observer, &"a".repeat(30 * 1024)).await;
        let first = bounded(wait(&hook, &initial, None)).await;
        assert_eq!(first["agents"][0]["truncated"], true);
        assert!(
            first["agents"][0]["result"]
                .as_str()
                .expect("UTF-8 truncated result")
                .len()
                <= 16 * 1024
        );
        assert_eq!(first["agents"][0]["message_id"], initial.message_id);
        let next = followup(&hook, &observer, &initial, &"b".repeat(30 * 1024)).await;
        bounded(wait(&hook, &next, None)).await
    };
    let receipt = supervisor.run(parent.boxed_local(), launch).await;
    let requests = provider.requests.lock().expect("requests").clone();
    (requests, receipt)
}

#[tokio::test]
async fn native_followup_allows_large_complete_constraints_within_tokens_and_rejects_over_budget() {
    let (large, accepted) = native_followup_context_case(1_000_000).await;
    assert_eq!(large.len(), 2);
    assert_eq!(accepted["agents"][0]["state"], "completed");
    let request = &large[1];
    for original in ["a".repeat(30 * 1024), "b".repeat(30 * 1024)] {
        assert!(
            request
                .messages
                .iter()
                .any(|message| message.role == ChatRole::User && message.content == original)
        );
    }
    assert!(
        request
            .messages
            .iter()
            .map(|message| message.content.len())
            .sum::<usize>()
            > 64 * 1024
    );
    let input_limit = vega_runtime::estimate_wire_context(&request.messages, &request.tools)
        .expect("complete followup estimate")
        .input_tokens
        - 1;
    let first_estimate = vega_runtime::estimate_wire_context(&large[0].messages, &large[0].tools)
        .expect("first stage estimate")
        .input_tokens;
    assert!(first_estimate < input_limit);
    let (small, rejected) = native_followup_context_case(input_limit).await;
    assert_eq!(
        small.len(),
        1,
        "over-budget followup never reaches the provider"
    );
    assert_ne!(rejected["agents"][0]["state"], "completed");
}

async fn native_image_followup(input_limit: u64) -> (Vec<ChatRequest>, serde_json::Value) {
    let fixture = Fixture::new();
    let mut supervisor = Supervisor::new();
    let source = seed_completed(&fixture, &supervisor, true).await;
    let provider = ScriptedProvider::with(vec![final_response("image constraint retained")]);
    let host = TestHost::new(true);
    let hook = supervisor.hook();
    let observer = supervisor.receipt_observer();
    let mut launch = fixture.launch(&provider, host);
    launch.settings.context_budget = Some(fixture_context_budget(input_limit, false));
    let parent = async {
        let next = followup(
            &hook,
            &observer,
            &source,
            "Retain the original image instruction",
        )
        .await;
        bounded(wait(&hook, &next, None)).await
    };
    let receipt = supervisor.run(parent.boxed_local(), launch).await;
    let requests = provider.requests.lock().expect("requests").clone();
    (requests, receipt)
}

#[tokio::test]
async fn native_followup_token_admission_charges_preserved_user_images_before_provider_dispatch() {
    let (large, accepted) = native_image_followup(1_000_000).await;
    assert_eq!(large.len(), 1);
    assert_eq!(accepted["agents"][0]["state"], "completed");
    let request = &large[0];
    let image_user = request
        .messages
        .iter()
        .find(|message| {
            message.role == ChatRole::User
                && message.content == "Intervening user constraint and resolved reference content"
        })
        .expect("original image instruction");
    assert_eq!(image_user.images.len(), 1);
    assert_eq!(
        (image_user.images[0].width(), image_user.images[0].height()),
        (1, 1)
    );
    let full = vega_runtime::estimate_wire_context(&request.messages, &request.tools)
        .expect("image request");
    let mut without_images = request.messages.clone();
    for message in &mut without_images {
        message.images.clear();
    }
    let text_only = vega_runtime::estimate_wire_context(&without_images, &request.tools)
        .expect("image-free comparison");
    assert_eq!(full.image_count, 1);
    assert!(text_only.input_tokens < full.input_tokens);
    let (small, rejected) = native_image_followup(full.input_tokens - 1).await;
    assert!(
        small.is_empty(),
        "the preserved image puts the complete input over its configured allowance"
    );
    assert_ne!(rejected["agents"][0]["state"], "completed");
}

fn question_response(id: &str) -> Vec<ProviderEvent> {
    tool_response(
        "Need an explicit choice",
        id,
        vega_runtime::USER_QUESTION_TOOL,
        serde_json::json!({"questions":[{"id":"choice","header":"Choice","question":"Choose the permitted option","options":[{"label":"A","description":"first"},{"label":"B","description":"second"}]}]}),
    )
}

fn answer_question(queue: &UserQuestionQueue) {
    let pending = queue.take_pending().expect("pending question");
    let answers = vega_runtime::UserQuestionAnswers {
        answers: BTreeMap::from([(
            "choice".into(),
            vega_runtime::UserQuestionAnswer {
                answers: vec!["A".into()],
            },
        )]),
    };
    assert!(pending.respond(answers).expect("valid answer"));
}

#[tokio::test]
async fn native_questions_wake_once_per_request_and_pause_only_actual_pending_time() {
    let fixture = Fixture::new();
    let provider = ScriptedProvider::with(vec![
        question_response("question-one"),
        question_response("question-two"),
        final_response("completed after both answers"),
    ]);
    let host = TestHost::new(true);
    let mut supervisor = Supervisor::new();
    let hook = supervisor.hook();
    let observer = supervisor.receipt_observer();
    let parent = async {
        let snapshot = spawn(&hook, &observer, "Ask the two necessary questions").await;
        host.until(|| host.questions().has_pending()).await;
        let first = tokio::time::timeout(
            Duration::from_millis(750),
            wait(&hook, &snapshot, Some(60_000)),
        )
        .await;
        if first.is_err() {
            host.cancel().cancel();
            bounded(wait(&hook, &snapshot, None)).await;
        }
        let first = first.expect("pending human question must wake parent");
        assert_eq!(first["timed_out"], false);
        assert_eq!(first["agents"][0]["state"], "running");
        let repeated = wait(&hook, &snapshot, Some(0));
        tokio::pin!(repeated);
        assert!(
            futures::poll!(&mut repeated).is_pending(),
            "same attention must not cause a hot loop"
        );
        tokio::time::pause();
        tokio::time::advance(Duration::from_secs(21 * 60)).await;
        tokio::task::yield_now().await;
        let cancelled_during_question = host.cancel().is_cancelled();
        let repeated_still_pending = futures::poll!(&mut repeated).is_pending();
        tokio::time::resume();
        answer_question(&host.questions());
        host.until(|| host.questions().has_pending()).await;
        let second = bounded(&mut repeated).await;
        assert_eq!(second["timed_out"], false);
        assert_eq!(second["timeout_coalesced"], true);
        assert_eq!(second["wake_reason"], "attention");
        answer_question(&host.questions());
        let terminal = bounded(wait(&hook, &snapshot, None)).await;
        assert!(
            !cancelled_during_question,
            "actual human pending time is excluded from active budget"
        );
        assert!(repeated_still_pending);
        assert_eq!(terminal["agents"][0]["state"], "completed");
        assert_eq!(provider.requests.lock().expect("requests").len(), 3);
    };
    supervisor
        .run(
            parent.boxed_local(),
            fixture.launch(&provider, host.clone()),
        )
        .await;
}

#[tokio::test]
async fn permission_attention_uses_the_existing_queue_without_stealing_its_listener() {
    let fixture = Fixture::new();
    let provider = ScriptedProvider::default();
    let host = TestHost::new(true);
    let mut supervisor = Supervisor::new();
    let hook = supervisor.hook();
    let observer = supervisor.receipt_observer();
    let parent = async {
        let snapshot = spawn(&hook, &observer, "work").await;
        provider.request_count(1).await;
        assert_eq!(
            bounded(wait(&hook, &snapshot, Some(0))).await["timed_out"],
            true
        );
        let queue = host.permissions();
        let decision = queue.request(
            PermissionRequest {
                call_id: "permission-one".into(),
                tool: "write".into(),
                display_target: "new.txt".into(),
                danger_rule_id: None,
                danger_reason: None,
                external: None,
                acp_options: None,
            },
            host.cancel(),
        );
        tokio::pin!(decision);
        assert!(futures::poll!(&mut decision).is_pending());
        let attention = tokio::time::timeout(
            Duration::from_millis(750),
            wait(&hook, &snapshot, Some(60_000)),
        )
        .await;
        let repeated = wait(&hook, &snapshot, Some(0));
        tokio::pin!(repeated);
        let repeated_poll = futures::poll!(&mut repeated);
        let repeated_pending = repeated_poll.is_pending();
        let visible = queue.take_pending();
        host.cancel().cancel();
        let _ = bounded(decision).await;
        if repeated_pending {
            bounded(&mut repeated).await;
        } else {
            bounded(wait(&hook, &snapshot, None)).await;
        }
        assert!(
            visible.is_some(),
            "UI must retain access to its original pending request"
        );
        let attention = attention.expect("permission attention");
        assert_eq!(attention["timed_out"], false);
        assert_eq!(attention["timeout_coalesced"], true);
        assert_eq!(attention["wake_reason"], "attention");
        assert!(
            repeated_pending,
            "same pending permission cannot renew or wake a coalesced wait"
        );
    };
    supervisor
        .run(
            parent.boxed_local(),
            fixture.launch(&provider, host.clone()),
        )
        .await;
}

async fn seed_completed(
    fixture: &Fixture,
    supervisor: &Supervisor,
    with_intervening_user_image: bool,
) -> SubagentSnapshot {
    let provider = ScriptedProvider::default();
    let launch = fixture.launch(&provider, TestHost::new(true));
    let reserved = supervisor
        .reserve(
            &launch,
            "seed-call",
            "spawn_agent",
            &serde_json::json!({"name":"seed","task":"Initial user constraint"}),
        )
        .expect("seed reserve");
    bounded(create_child(
        fixture.database.clone(),
        reserved.snapshot.clone(),
        reserved.task.clone(),
        reserved.call_id.clone(),
        fixture.settings(),
        reserved.cancel.clone(),
        None,
    ))
    .await
    .expect("seed creation");
    if with_intervening_user_image {
        insert_message(
            &fixture.store,
            &reserved.snapshot.agent_id,
            "prior-progress",
            "assistant",
            "earlier-stage-internal-progress",
            "done",
        );
        insert_message(
            &fixture.store,
            &reserved.snapshot.agent_id,
            "intervening-user",
            "user",
            "Intervening user constraint and resolved reference content",
            "done",
        );
        let mut encoded = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(1, 1)
            .write_to(&mut encoded, image::ImageFormat::Png)
            .expect("PNG");
        vega_store::image_attachments::insert(
            fixture.store.conn(),
            "intervening-user",
            0,
            &encoded.into_inner(),
        )
        .expect("source image");
    }
    insert_message(
        &fixture.store,
        &reserved.snapshot.agent_id,
        &reserved.snapshot.message_id,
        "assistant",
        "Final factual result",
        "streaming",
    );
    assert!(
        messages::record_assistant_response(
            fixture.store.conn(),
            &reserved.snapshot.message_id,
            "Final factual result",
            &messages::AssistantResponseRow {
                message_id: reserved.snapshot.message_id.clone(),
                response_id: "seed-final".into(),
                text_start_bytes: 0,
                text_end_bytes: "Final factual result".len(),
                kind: "final".into()
            }
        )
        .expect("final boundary")
    );
    messages::finish(
        fixture.store.conn(),
        &reserved.snapshot.message_id,
        "Final factual result",
        "done",
    )
    .expect("finish source");
    let mut terminal = reserved.snapshot;
    terminal.state = SubagentState::Completed;
    terminal.result = "Final factual result".into();
    let boundary = bounded(persist_child_terminal(
        fixture.database.clone(),
        terminal.clone(),
        true,
    ))
    .await
    .expect("persist terminal");
    assert!(boundary.is_some());
    supervisor.registry.finish(terminal.clone(), boundary);
    terminal
}

#[tokio::test]
async fn followup_projection_keeps_intervening_users_images_and_existing_authority() {
    let fixture = Fixture::new();
    let supervisor = Supervisor::new();
    let source = seed_completed(&fixture, &supervisor, true).await;
    let provider = ScriptedProvider::default();
    let launch = fixture.launch(&provider, TestHost::new(true));
    let next = supervisor.reserve(&launch, "next-call", "followup_agent", &serde_json::json!({"agent_id":source.agent_id,"message_id":source.message_id,"task":"Current user constraint"})).expect("reserve followup");
    let (_, instruction) = bounded(create_child(
        fixture.database.clone(),
        next.snapshot.clone(),
        next.task.clone(),
        next.call_id,
        fixture.settings(),
        next.cancel,
        next.previous,
    ))
    .await
    .expect("validated continuation");
    let prepared = super::super::pipeline::prepare_run_with_images_and_reasoning(
        fixture.database.clone(),
        source.agent_id.clone(),
        "Current user constraint".into(),
        "Frozen system authority".into(),
        instruction,
        next.snapshot.message_id.clone(),
        PersistenceActorConfig {
            delegated: true,
            delegated_parent: Some(("parent".into(), "parent-run".into())),
            delegated_message_id: Some(next.snapshot.message_id.clone()),
            ..Default::default()
        },
        true,
        None,
        None,
        Vec::new(),
    )
    .expect("prepared continuation");
    let history = &prepared.request.history;
    let users: Vec<_> = history
        .iter()
        .filter(|message| message.role == ChatRole::User)
        .collect();
    assert_eq!(users.len(), 3);
    assert_eq!(users[0].content, "Initial user constraint");
    assert_eq!(
        users[1].content,
        "Intervening user constraint and resolved reference content"
    );
    assert_eq!(users[1].images.len(), 1);
    assert_eq!(users[2].content, "Current user constraint");
    assert!(
        !history
            .iter()
            .any(|message| message.content.contains("earlier-stage-internal-progress"))
    );
    assert!(
        history
            .iter()
            .any(|message| message.role == ChatRole::Assistant
                && message.content.contains("Final factual result"))
    );
    assert_eq!(prepared.request.system_prompt, "Frozen system authority");
    assert_eq!(prepared.request.model, "fixture-model");
    assert_eq!(
        prepared.request.tool_config.permission_mode,
        RuntimePermissionMode::Confirm
    );
    assert!(
        messages::find(fixture.store.conn(), "prior-progress")
            .expect("retained source")
            .is_some()
    );
    supervisor.registry.cancel_all();
}

#[tokio::test]
async fn continuations_reject_same_sequence_edits_permission_changes_and_noncompleted_sources() {
    for mutation in [
        "source",
        "authority",
        "pending",
        "interrupted",
        "failed",
        "stale",
    ] {
        let fixture = Fixture::new();
        let supervisor = Supervisor::new();
        let source = seed_completed(&fixture, &supervisor, false).await;
        let provider = ScriptedProvider::default();
        let launch = fixture.launch(&provider, TestHost::new(true));
        if mutation == "source" {
            fixture.store.conn().execute("UPDATE messages SET content = 'externally edited at same sequence' WHERE id = ?1", [&source.message_id]).expect("external source edit");
        } else if mutation == "authority" {
            fixture
                .store
                .conn()
                .execute(
                    "UPDATE threads SET permission_mode = 'full_access' WHERE id = ?1",
                    [&source.agent_id],
                )
                .expect("external authority edit");
        } else if mutation == "pending" {
            crate::pending_messages::enqueue(
                &fixture.store,
                &crate::types::PendingMessage {
                    id: "pending-edit".into(),
                    thread_id: source.agent_id.clone(),
                    generation: "external-generation".into(),
                    behavior: crate::types::PendingMessageBehavior::Steer,
                    content: "External continuation".into(),
                    images: Vec::new(),
                },
            )
            .expect("external pending input");
        } else if matches!(mutation, "interrupted" | "failed") {
            let mut state = source.clone();
            state.state = if mutation == "failed" {
                SubagentState::Failed
            } else {
                SubagentState::Interrupted
            };
            supervisor
                .registry
                .update(state)
                .expect("noncompleted state");
        }
        let reserved = supervisor.reserve(&launch, "next-call", "followup_agent", &serde_json::json!({"agent_id":source.agent_id,"message_id":if mutation == "stale" {"other-execution"} else {&source.message_id},"task":"continue"}));
        match reserved {
            Err(_) => assert!(matches!(mutation, "interrupted" | "failed" | "stale")),
            Ok(reserved) => {
                let result = bounded(create_child(
                    fixture.database.clone(),
                    reserved.snapshot,
                    reserved.task,
                    reserved.call_id,
                    fixture.settings(),
                    reserved.cancel,
                    reserved.previous,
                ))
                .await;
                assert!(
                    result.is_err(),
                    "{mutation} must not authorize a new provider run"
                );
            }
        }
        assert!(provider.requests.lock().expect("requests").is_empty());
        supervisor.registry.cancel_all();
    }
}

#[tokio::test]
async fn bounded_native_stage_disables_compaction_but_ordinary_parent_retains_it() {
    let fixture = Fixture::new();
    let provider = ScriptedProvider::default();
    let policy = vega_store::context_compaction::ModelContextPolicy {
        provider: "fixture-provider".into(),
        model: "fixture-model".into(),
        input_limit: Some(100),
        output_reserve: Some(10),
        automatic_compaction: true,
        updated_at: 1,
    };
    vega_store::context_compaction::save_model_policy(fixture.store.conn(), &policy)
        .expect("model policy");
    let inherited = vega_runtime::ContextBudget::new(110, 10, true).expect("context budget");
    let host = TestHost::new(true);
    let mut supervisor = Supervisor::new();
    let source = seed_completed(&fixture, &supervisor, false).await;
    let hook = supervisor.hook();
    let observer = supervisor.receipt_observer();
    let mut launch = fixture.launch(&provider, host.clone());
    launch.settings.context_budget = Some(inherited);
    launch.settings.reasoning = Some(FrozenReasoning::unknown(
        "fixture-provider",
        "fixture-model",
    ));
    let parent = async {
        let snapshot = followup(&hook, &observer, &source, &"bounded task ".repeat(150)).await;
        let receipt = bounded(wait(&hook, &snapshot, None)).await;
        assert_ne!(receipt["agents"][0]["state"], "completed");
        assert!(provider.requests.lock().expect("requests").is_empty());
    };
    supervisor.run(parent.boxed_local(), launch).await;
    let child_compactions = host.compactions.load(Ordering::SeqCst);
    messages::finish(
        fixture.store.conn(),
        "parent-run",
        "Retained prior assistant evidence for the ordinary conversation",
        "done",
    )
    .expect("complete ordinary parent history");
    let mut ordinary_compactions = 0;
    let _ = bounded(super::super::entry::run_thread_task_with_automation_inner(
        &fixture.store,
        &provider,
        &fixture.tools,
        "parent",
        &"ordinary task ".repeat(150),
        "system",
        CancellationToken::new(),
        &super::super::RejectPermissionHook,
        |event| {
            if matches!(event, ConversationEvent::ContextCompactionStatus { .. }) {
                ordinary_compactions += 1;
            }
            Ok(())
        },
        PersistenceActorConfig::default(),
        None,
        None,
        Some(FrozenReasoning::unknown(
            "fixture-provider",
            "fixture-model",
        )),
        Vec::new(),
        Vec::new(),
    ))
    .await;
    assert_eq!(
        child_compactions, 0,
        "native bounded stages must fail context limits without automatic summary"
    );
    assert!(
        ordinary_compactions > 0,
        "ordinary main conversation still uses its configured automatic compaction"
    );
    assert_eq!(
        vega_store::context_compaction::load_model_policy(
            fixture.store.conn(),
            "fixture-provider",
            "fixture-model"
        )
        .expect("unchanged policy"),
        Some(policy)
    );
    assert!(inherited.automatic_compaction());
    assert_eq!(
        (inherited.total_limit(), inherited.output_reserve()),
        (110, 10)
    );
}

struct ParentWaitProvider {
    requests: Mutex<Vec<ChatRequest>>,
    explicit: bool,
}

impl Provider for ParentWaitProvider {
    fn chat_stream(
        &self,
        request: ChatRequest,
        _: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        let child = request
            .messages
            .iter()
            .any(|message| message.role == ChatRole::User && message.content == "CHILD_TASK_WAIT");
        let steered = request.messages.iter().any(|message| {
            message.role == ChatRole::User && message.content.contains("NEW_STEERING_CONSTRAINT")
        });
        let spawned = request
            .messages
            .iter()
            .filter(|message| message.role == ChatRole::Tool)
            .find_map(|message| {
                serde_json::from_str::<serde_json::Value>(&message.content)
                    .ok()
                    .and_then(|value| value.get("agent").cloned())
            });
        let observed = request
            .messages
            .iter()
            .filter(|message| message.role == ChatRole::Tool)
            .any(|message| {
                serde_json::from_str::<serde_json::Value>(&message.content)
                    .is_ok_and(|value| value.get("timed_out").is_some())
            });
        let events = if child {
            None
        } else if steered {
            Some(final_response("Applied steering once"))
        } else if let Some(agent) = spawned {
            let mut input = serde_json::json!({"targets":[{"agent_id":agent["agent_id"],"message_id":agent["message_id"]}]});
            if self.explicit {
                input["timeout_ms"] = if observed { 60_000 } else { 0 }.into();
            }
            Some(tool_response(
                "Waiting for the existing child",
                if self.explicit && !observed {
                    "parent-wait-initial"
                } else {
                    "parent-wait"
                },
                "wait_agent",
                input,
            ))
        } else {
            Some(tool_response(
                "Delegate bounded work",
                "parent-spawn",
                "spawn_agent",
                serde_json::json!({"name":"worker","task":"CHILD_TASK_WAIT"}),
            ))
        };
        self.requests.lock().expect("requests").push(request);
        async move {
            let stream: EventStream = match events {
                Some(events) => Box::pin(futures::stream::iter(events.into_iter().map(Ok))),
                None => Box::pin(futures::stream::pending()),
            };
            Ok(stream)
        }
        .boxed()
    }
}

fn enqueue_steering(fixture: &Fixture, generation: &str, id: &str, text: &str) {
    crate::pending_messages::enqueue(
        &fixture.store,
        &crate::types::PendingMessage {
            id: id.into(),
            thread_id: "parent".into(),
            generation: generation.into(),
            behavior: crate::types::PendingMessageBehavior::Steer,
            content: text.into(),
            images: Vec::new(),
        },
    )
    .expect("steering submission");
}

#[tokio::test]
async fn native_parent_wait_wakes_for_steering_before_or_during_wait_and_claims_once() {
    for (before_wait, explicit) in [(true, false), (false, false), (true, true), (false, true)] {
        let fixture = Fixture::new();
        let provider = ParentWaitProvider {
            requests: Mutex::new(Vec::new()),
            explicit,
        };
        let host = TestHost::new(true);
        let cancel = CancellationToken::new();
        let reader: vega_runtime::CredentialReader = Arc::new(|| Ok(Vec::new()));
        let source = Arc::new(crate::pending_messages::QueueSteeringSource::new(
            fixture.database.clone(),
            fixture._directory.path().to_path_buf(),
            "parent".into(),
            "parent-generation".into(),
            reader.clone(),
            cancel.clone(),
        ));
        let config = PersistenceActorConfig::default()
            .with_subagent_host(host.clone())
            .with_owner_credential_reader(reader)
            .with_pending_input(None, Some(source));
        let running = AtomicBool::new(false);
        let signal = Notify::new();
        let run = super::super::entry::run_thread_task_with_automation_inner(
            &fixture.store,
            &provider,
            &fixture.tools,
            "parent",
            "PARENT_TASK_WAIT",
            "Preserve all user constraints",
            cancel.clone(),
            &super::super::RejectPermissionHook,
            |event| {
                if matches!(event, ConversationEvent::ToolCallRunning { call_id } if call_id == "parent-wait")
                {
                    if before_wait {
                        enqueue_steering(
                            &fixture,
                            "parent-generation",
                            "steer-once",
                            "NEW_STEERING_CONSTRAINT",
                        );
                    }
                    running.store(true, Ordering::SeqCst);
                    signal.notify_waiters();
                }
                Ok(())
            },
            config,
            None,
            None,
            None,
            Vec::new(),
            Vec::new(),
        );
        let driver = async {
            bounded(async {
                loop {
                    let changed = signal.notified();
                    tokio::pin!(changed);
                    changed.as_mut().enable();
                    if running.load(Ordering::SeqCst) {
                        break;
                    }
                    changed.await;
                }
            })
            .await;
            if !before_wait {
                tokio::time::sleep(Duration::from_millis(30)).await;
                enqueue_steering(
                    &fixture,
                    "other-generation",
                    "wrong-generation",
                    "WRONG_GENERATION_MUST_REMAIN_QUEUED",
                );
                enqueue_steering(
                    &fixture,
                    "parent-generation",
                    "steer-once",
                    "NEW_STEERING_CONSTRAINT",
                );
            }
        };
        let guarded = async {
            tokio::pin!(run);
            tokio::select! {
                result = &mut run => (true, result),
                _ = tokio::time::sleep(Duration::from_secs(3)) => {
                    cancel.cancel();
                    (false, bounded(&mut run).await)
                }
            }
        };
        let ((finished_promptly, result), ()) = tokio::join!(guarded, driver);
        assert!(
            finished_promptly,
            "pending steering must wake the native wait without a 30-second timeout"
        );
        let result = result.expect("parent result");
        assert!(!result.interrupted && !result.failed);
        assert!(result.content.contains("Applied steering once"));
        let count: i64 = fixture.store.conn().query_row("SELECT count(*) FROM messages WHERE thread_id='parent' AND role='user' AND content='NEW_STEERING_CONSTRAINT'", [], |row| row.get(0)).expect("steering claim count");
        assert_eq!(count, 1);
        let pending = vega_store::pending_messages::count(fixture.store.conn(), "parent")
            .expect("pending count");
        assert_eq!(pending, if before_wait { 0 } else { 1 });
        let requests = provider.requests.lock().expect("requests");
        let roots: Vec<_> = requests
            .iter()
            .filter(|request| {
                !request.messages.iter().any(|message| {
                    message.role == ChatRole::User && message.content == "CHILD_TASK_WAIT"
                })
            })
            .collect();
        assert_eq!(
            roots.len(),
            if explicit { 4 } else { 3 },
            "one spawn, at most one initial snapshot, one suspended wait, one steered turn"
        );
        assert!(
            !roots
                .last()
                .expect("steered turn")
                .messages
                .iter()
                .any(|message| message
                    .content
                    .contains("WRONG_GENERATION_MUST_REMAIN_QUEUED"))
        );
    }
}

#[tokio::test]
async fn active_budget_accumulates_across_human_pause_and_does_not_pause_for_waiting_text() {
    let fixture = Fixture::new();
    let provider = ScriptedProvider::with(vec![tool_response(
        "I am waiting, but no human question is pending",
        "read-before-wait",
        "read",
        serde_json::json!({"path":"missing.txt"}),
    )]);
    let host = TestHost::new(true);
    let mut supervisor = Supervisor::new();
    let hook = supervisor.hook();
    let observer = supervisor.receipt_observer();
    let parent = async {
        let snapshot = spawn(&hook, &observer, "work").await;
        provider.request_count(2).await;
        tokio::time::pause();
        tokio::time::advance(Duration::from_secs(600)).await;
        tokio::task::yield_now().await;
        let input = vega_runtime::UserQuestionInput::from_json(&serde_json::json!({"questions":[{"id":"choice","header":"Choice","question":"Choose?","options":[{"label":"A","description":"first"},{"label":"B","description":"second"}]}]}).to_string()).expect("question input");
        let queue = host.questions();
        let answer = vega_runtime::RuntimeUserQuestionHook::request(
            &queue,
            vega_runtime::RuntimeUserQuestionRequest {
                call_id: "active-clock-question".into(),
                input,
            },
            host.cancel(),
        );
        tokio::pin!(answer);
        assert!(futures::poll!(&mut answer).is_pending());
        tokio::time::advance(Duration::from_secs(1)).await;
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_secs(1200)).await;
        tokio::task::yield_now().await;
        let paused_safely = !host.cancel().is_cancelled();
        answer_question(&queue);
        let _ = answer.await;
        tokio::time::advance(Duration::from_secs(1)).await;
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_secs(590)).await;
        tokio::task::yield_now().await;
        let before_deadline = !host.cancel().is_cancelled();
        tokio::time::advance(Duration::from_secs(20)).await;
        tokio::task::yield_now().await;
        let eventually_expired = host.cancel().is_cancelled();
        tokio::time::resume();
        if !eventually_expired {
            host.cancel().cancel();
        }
        let receipt = bounded(wait(&hook, &snapshot, None)).await;
        (paused_safely, before_deadline, eventually_expired, receipt)
    };
    let (paused, before, expired, receipt) = supervisor
        .run(
            parent.boxed_local(),
            fixture.launch(&provider, host.clone()),
        )
        .await;
    assert!(
        paused && before,
        "only actual pending intervals pause accumulated activity"
    );
    assert!(
        expired,
        "active minutes before and after human input share one stage budget"
    );
    assert_eq!(receipt["agents"][0]["state"], "interrupted");
    assert_eq!(receipt["agents"][0]["ready"], false);
}

#[tokio::test]
async fn completed_stage_is_not_retroactively_cancelled_while_host_release_is_pending() {
    let fixture = Fixture::new();
    let provider = ScriptedProvider::with(vec![final_response("Completed before stage deadline")]);
    let host = TestHost::new(false);
    let mut supervisor = Supervisor::new();
    let hook = supervisor.hook();
    let observer = supervisor.receipt_observer();
    let parent = async {
        let snapshot = spawn(&hook, &observer, "bounded work").await;
        host.until(|| host.release_started.load(Ordering::SeqCst))
            .await;
        tokio::time::pause();
        tokio::time::advance(Duration::from_secs(21 * 60)).await;
        tokio::task::yield_now().await;
        let was_cancelled = host.cancel().is_cancelled();
        tokio::time::resume();
        host.release_gate.add_permits(1);
        let receipt = bounded(wait(&hook, &snapshot, None)).await;
        (was_cancelled, receipt)
    };
    let (was_cancelled, receipt) = supervisor
        .run(
            parent.boxed_local(),
            fixture.launch(&provider, host.clone()),
        )
        .await;
    assert!(
        !was_cancelled,
        "completed execution is not subject to retrospective budget cancellation"
    );
    assert_eq!(receipt["agents"][0]["state"], "completed");
}

#[derive(Default)]
struct WaitPollState {
    count: usize,
    pending: bool,
    waker: Option<std::task::Waker>,
}

#[derive(Default)]
struct WaitPollObserver {
    calls: Mutex<BTreeMap<String, WaitPollState>>,
    changed: Notify,
}

impl WaitPollObserver {
    fn record(&self, call_id: &str, pending: bool, waker: &std::task::Waker) {
        let mut calls = self.calls.lock().expect("wait observations");
        let state = calls.entry(call_id.into()).or_default();
        state.count += 1;
        state.pending = pending;
        state.waker = pending.then(|| waker.clone());
        drop(calls);
        self.changed.notify_waiters();
    }

    async fn after(&self, call_id: &str, previous: usize) -> bool {
        bounded(async {
            loop {
                let changed = self.changed.notified();
                tokio::pin!(changed);
                changed.as_mut().enable();
                if let Some(state) = self.calls.lock().expect("wait observations").get(call_id)
                    && state.count > previous
                {
                    return state.pending;
                }
                changed.await;
            }
        })
        .await
    }

    fn count(&self, call_id: &str) -> usize {
        self.calls
            .lock()
            .expect("wait observations")
            .get(call_id)
            .expect("observed wait")
            .count
    }

    fn wake_pending(&self, call_id: &str) {
        let waker = self
            .calls
            .lock()
            .expect("wait observations")
            .get(call_id)
            .and_then(|state| state.waker.clone());
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

struct ObservedWaitHook {
    inner: Arc<dyn vega_runtime::RuntimeSubagentHook>,
    observer: Arc<WaitPollObserver>,
}

impl vega_runtime::RuntimeSubagentHook for ObservedWaitHook {
    fn call(
        &self,
        call_id: String,
        tool: String,
        input: serde_json::Value,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<serde_json::Value, VegaError>> {
        let observe = tool == "wait_agent";
        let observer = self.observer.clone();
        let mut inner = self.inner.call(call_id.clone(), tool, input, cancel);
        futures::future::poll_fn(move |context| {
            let result = inner.as_mut().poll(context);
            if observe {
                observer.record(&call_id, result.is_pending(), context.waker());
            }
            result
        })
        .boxed()
    }
}

struct ExplicitWaitReplayProvider {
    roots: AtomicUsize,
    child_requests: AtomicUsize,
    requests: Mutex<Vec<ChatRequest>>,
    called: Notify,
}

impl ExplicitWaitReplayProvider {
    async fn child_requested(&self) {
        bounded(async {
            loop {
                let called = self.called.notified();
                tokio::pin!(called);
                called.as_mut().enable();
                if self.child_requests.load(Ordering::SeqCst) > 0 {
                    return;
                }
                called.await;
            }
        })
        .await;
    }
}

impl Provider for ExplicitWaitReplayProvider {
    fn chat_stream(
        &self,
        request: ChatRequest,
        _: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        let child = request.messages.iter().any(|message| {
            message.role == ChatRole::User && message.content == "EXPLICIT_REPLAY_CHILD"
        });
        let events = if child {
            self.child_requests.fetch_add(1, Ordering::SeqCst);
            None
        } else {
            let turn = self.roots.fetch_add(1, Ordering::SeqCst);
            if turn == 0 {
                Some(tool_response(
                    "Delegate work",
                    "replay-spawn",
                    "spawn_agent",
                    serde_json::json!({"name":"replay-worker","task":"EXPLICIT_REPLAY_CHILD"}),
                ))
            } else {
                let agent = request
                    .messages
                    .iter()
                    .filter(|message| message.role == ChatRole::Tool)
                    .find_map(|message| {
                        serde_json::from_str::<serde_json::Value>(&message.content)
                            .ok()
                            .and_then(|value| value.get("agent").cloned())
                    })
                    .expect("real native spawn receipt");
                Some(tool_response(
                    "Await the same execution",
                    &format!("replay-wait-{turn}"),
                    "wait_agent",
                    serde_json::json!({"targets":[{"agent_id":agent["agent_id"],"message_id":agent["message_id"]}],"timeout_ms":60000}),
                ))
            }
        };
        self.requests.lock().expect("requests").push(request);
        self.called.notify_waiters();
        async move {
            let stream: EventStream = match events {
                Some(events) => Box::pin(futures::stream::iter(events.into_iter().map(Ok))),
                None => Box::pin(futures::stream::pending()),
            };
            Ok(stream)
        }
        .boxed()
    }
}

#[tokio::test]
async fn runtime_loop_and_supervisor_explicit_wait_replay_stops_provider_churn_after_first_timeout()
{
    let fixture = Fixture::new();
    let provider = ExplicitWaitReplayProvider {
        roots: AtomicUsize::new(0),
        child_requests: AtomicUsize::new(0),
        requests: Mutex::new(Vec::new()),
        called: Notify::new(),
    };
    let host = TestHost::new(true);
    let cancel = CancellationToken::new();
    let waits_started = AtomicUsize::new(0);
    let timed_out = AtomicUsize::new(0);
    let mut supervisor = Supervisor::new();
    let receipt_observer = supervisor.receipt_observer();
    let polls = Arc::new(WaitPollObserver::default());
    let wrapper = Arc::new(ObservedWaitHook {
        inner: supervisor.hook(),
        observer: polls.clone(),
    });
    let mut request = fixture.request.clone();
    request.tool_config = request.tool_config.with_subagents(Some(wrapper));
    let parent = vega_runtime::run_agent_with_sink(
        &provider,
        &fixture.tools,
        request,
        cancel.clone(),
        |event| {
            receipt_observer.persisted(&event);
            if matches!(&event, RuntimeEvent::ToolCallRunning { call_id } if call_id.starts_with("replay-wait-"))
            {
                waits_started.fetch_add(1, Ordering::SeqCst);
            }
            if let RuntimeEvent::ToolCallFinished(result) = &event
                && result.call_id.starts_with("replay-wait-")
                && serde_json::from_str::<serde_json::Value>(&result.output)
                    .is_ok_and(|value| value["timed_out"] == true)
            {
                timed_out.fetch_add(1, Ordering::SeqCst);
            }
            std::future::ready(Ok(()))
        },
    );
    let mut launch = fixture.launch(&provider, host);
    launch.cancel = cancel.clone();
    let run = supervisor.run(parent.boxed_local(), launch);
    let driver = async {
        assert!(
            polls.after("replay-wait-1", 0).await,
            "first actual wait registered its timer"
        );
        provider.child_requested().await;
        tokio::time::pause();
        tokio::time::advance(Duration::from_secs(61)).await;
        tokio::time::resume();
        assert!(
            polls.after("replay-wait-2", 0).await,
            "second actual wait is suspended"
        );
        assert_eq!(timed_out.load(Ordering::SeqCst), 1);
        for _ in 0..4 {
            let previous = polls.count("replay-wait-2");
            tokio::time::pause();
            tokio::time::advance(Duration::from_secs(61)).await;
            polls.wake_pending("replay-wait-2");
            let pending = polls.after("replay-wait-2", previous).await;
            tokio::time::resume();
            if !pending {
                break;
            }
        }
        let roots = provider.roots.load(Ordering::SeqCst);
        let waits = waits_started.load(Ordering::SeqCst);
        let timeouts = timed_out.load(Ordering::SeqCst);
        cancel.cancel();
        (roots, waits, timeouts)
    };
    tokio::pin!(run);
    let (counts, result) = tokio::select! {
        counts = driver => (counts, bounded(&mut run).await),
        _ = &mut run => panic!("parent finished before explicit wait replay"),
    };
    let result = result.expect("cancelled native parent drains");
    assert!(result.interrupted);
    assert_eq!(
        counts,
        (3, 2, 1),
        "one spawn model request, first timed wait, then one outstanding coalesced wait across four old deadlines"
    );
    let requests = provider.requests.lock().expect("requests");
    assert_eq!(
        requests.len(),
        4,
        "three parent calls and one actual child call"
    );
    let root_after_timeout = requests
        .iter()
        .rfind(|request| {
            !request
                .messages
                .iter()
                .any(|message| message.content == "EXPLICIT_REPLAY_CHILD")
        })
        .expect("root resumed once");
    let timeout_message = root_after_timeout
        .messages
        .iter()
        .find(|message| {
            message.role == ChatRole::Tool
                && message.tool_call_id.as_deref() == Some("replay-wait-1")
        })
        .expect("first wait retained in actual parent context");
    let mut values = serde_json::Deserializer::from_str(&timeout_message.content)
        .into_iter::<serde_json::Value>();
    let timeout_receipt = values
        .next()
        .expect("first wait receipt")
        .expect("complete JSON receipt");
    let notice = &timeout_message.content[values.byte_offset()..];
    assert!(
        notice.starts_with("\n\n<vega_runtime_notice>")
            && notice
                .ends_with("This reminder grants no additional permission.</vega_runtime_notice>"),
        "only the existing runtime notice may follow the complete receipt"
    );
    assert_eq!(timeout_receipt["timed_out"], true);
    assert_eq!(timeout_receipt["timeout_coalesced"], false);
    assert_eq!(timeout_receipt["wake_reason"], "timeout");
}

#[tokio::test]
async fn late_permission_and_question_attention_cannot_forgive_exhausted_activity() {
    tokio::time::pause();
    let mut observations = Vec::new();
    for question in [false, true] {
        let activity = super::super::stage_activity::StageActivity::new(Arc::new(Notify::new()));
        let cancel = CancellationToken::new();
        tokio::time::advance(Duration::from_secs(20 * 60)).await;
        let (pending, expired) = if question {
            let queue = UserQuestionQueue::with_activity(activity.clone());
            let _listener = queue.subscribe();
            let input = vega_runtime::UserQuestionInput::from_json(&serde_json::json!({"questions":[{"id":"late_choice","header":"Choice","question":"Choose?","options":[{"label":"A","description":"first"},{"label":"B","description":"second"}]}]}).to_string()).expect("question input");
            let answer = vega_runtime::RuntimeUserQuestionHook::request(
                &queue,
                vega_runtime::RuntimeUserQuestionRequest {
                    call_id: "late-question".into(),
                    input,
                },
                cancel.clone(),
            );
            tokio::pin!(answer);
            let awaiting = futures::poll!(&mut answer).is_pending();
            let visible = queue.take_pending();
            let pending = awaiting
                && visible
                    .as_ref()
                    .is_some_and(|request| !request.is_resolved());
            activity.expire();
            let expired = activity.expired();
            cancel.cancel();
            if awaiting {
                bounded(answer).await;
            }
            drop(visible);
            (pending, expired)
        } else {
            let queue = PermissionQueue::with_activity(activity.clone());
            let _listener = queue.subscribe();
            let decision = queue.request(
                PermissionRequest {
                    call_id: "late-permission".into(),
                    tool: "write".into(),
                    display_target: "late.txt".into(),
                    danger_rule_id: None,
                    danger_reason: None,
                    external: None,
                    acp_options: None,
                },
                cancel.clone(),
            );
            tokio::pin!(decision);
            let awaiting = futures::poll!(&mut decision).is_pending();
            let visible = queue.take_pending();
            let pending = awaiting
                && visible
                    .as_ref()
                    .is_some_and(|request| !request.is_resolved());
            activity.expire();
            let expired = activity.expired();
            cancel.cancel();
            if awaiting {
                let _ = bounded(decision).await;
            }
            drop(visible);
            (pending, expired)
        };
        activity.finish();
        observations.push((question, pending, expired));
    }
    tokio::time::resume();
    for (question, pending, expired) in observations {
        assert!(
            pending,
            "real human request must remain pending: question={question}"
        );
        assert!(
            expired,
            "late attention cannot forgive twenty elapsed active minutes: question={question}"
        );
    }
}

#[tokio::test]
async fn persistence_failure_is_visible_and_blocks_admission_before_host_release_drains() {
    let fixture = Fixture::new();
    let provider = ScriptedProvider::default();
    let host = TestHost::new(false);
    let mut supervisor = Supervisor::new();
    let hook = supervisor.hook();
    let observer = supervisor.receipt_observer();
    let registry = supervisor.registry.clone();
    let parent = async {
        let snapshot = spawn(&hook, &observer, "Produce a bounded artifact").await;
        provider.request_count(1).await;
        fixture
            .store
            .conn()
            .execute(
                "UPDATE delegated_executions SET state='failed' WHERE child_message_id=?1",
                [&snapshot.message_id],
            )
            .expect("inject failed terminal CAS");
        host.cancel().cancel();
        host.until(|| host.release_started.load(Ordering::SeqCst))
            .await;
        let waiting = hook.call(
            "persistence-failure-before-release".into(),
            "wait_agent".into(),
            wait_input(&snapshot, None),
            CancellationToken::new(),
        );
        tokio::pin!(waiting);
        let polled = futures::poll!(&mut waiting);
        let wait_failed_before_release = matches!(&polled, std::task::Poll::Ready(Err(_)));
        let key = (snapshot.agent_id.clone(), snapshot.message_id.clone());
        let occupied = !registry
            .released_terminal(std::slice::from_ref(&key))
            .expect("release state");
        let replacement = tokio::time::timeout(Duration::from_secs(5), hook.call(
            "new-writer-before-release".into(),
            "spawn_agent".into(),
            serde_json::json!({"name":"replacement","task":"new work after persistence failure"}),
            CancellationToken::new(),
        )).await;
        let admission_rejected_before_release = matches!(replacement, Ok(Err(_)));
        host.release_gate.add_permits(128);
        if polled.is_pending() {
            let _ = bounded(waiting).await;
        }
        (
            wait_failed_before_release,
            occupied,
            admission_rejected_before_release,
        )
    };
    let (wait_failed, occupied, rejected) = supervisor
        .run(
            parent.boxed_local(),
            fixture.launch(&provider, host.clone()),
        )
        .await;
    assert!(
        wait_failed,
        "terminal persistence failure must wake wait before host release"
    );
    assert!(occupied, "failed persistence must keep ownership occupied");
    assert!(
        rejected,
        "new admission must be rejected while failed persistence awaits host release"
    );
}

#[tokio::test]
async fn cancel_receipt_withholds_completed_state_until_host_release_without_recancelling() {
    let fixture = Fixture::new();
    let provider = ScriptedProvider::with(vec![final_response(
        "Completed and persisted before release",
    )]);
    let host = TestHost::new(false);
    let mut supervisor = Supervisor::new();
    let hook = supervisor.hook();
    let observer = supervisor.receipt_observer();
    let registry = supervisor.registry.clone();
    let parent = async {
        let snapshot = spawn(&hook, &observer, "Finish one reviewable stage").await;
        host.until(|| host.release_started.load(Ordering::SeqCst))
            .await;
        let durable = vega_store::subagents::execution(
            fixture.store.conn(),
            &snapshot.agent_id,
            &snapshot.message_id,
        )
        .expect("durable execution");
        let persisted_completed =
            durable.is_some_and(|execution| execution.record.state == "completed");
        let cancelled = bounded(hook.call(
            "cancel-completed-before-release".into(),
            "cancel_agent".into(),
            serde_json::json!({"agent_id":snapshot.agent_id,"message_id":snapshot.message_id}),
            CancellationToken::new(),
        ))
        .await;
        let nonterminal_receipt = cancelled
            .as_ref()
            .ok()
            .and_then(|value| value["agent"]["state"].as_str())
            .and_then(SubagentState::parse)
            .is_some_and(|state| !state.is_terminal());
        let not_recancelled = !host.cancel().is_cancelled();
        let key = (snapshot.agent_id.clone(), snapshot.message_id.clone());
        let occupied = !registry
            .released_terminal(std::slice::from_ref(&key))
            .expect("release state");
        host.release_gate.add_permits(1);
        let released = bounded(wait(&hook, &snapshot, None)).await;
        (
            persisted_completed,
            nonterminal_receipt,
            not_recancelled,
            occupied,
            released,
        )
    };
    let (persisted, nonterminal, not_recancelled, occupied, released) = supervisor
        .run(
            parent.boxed_local(),
            fixture.launch(&provider, host.clone()),
        )
        .await;
    assert!(
        persisted,
        "test boundary requires a completed persisted execution"
    );
    assert!(
        nonterminal && occupied,
        "cancel receipt cannot advertise terminal state before release"
    );
    assert!(
        not_recancelled,
        "completed work must not be retrospectively cancelled"
    );
    assert_eq!(released["agents"][0]["state"], "completed");
}
