use super::*;
use serde_json::{Value, json};
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use vega_runtime::{ChatRequest, EventStream, ProviderEvent, RuntimeToolResult, StopReason};

#[derive(Clone)]
enum Response {
    Finish(&'static str, bool),
    Hold(CancellationToken, &'static str),
    Fail,
    Events(Vec<ProviderEvent>),
}

#[derive(Default)]
struct ScriptedProvider {
    requests: Mutex<Vec<ChatRequest>>,
    responses: Mutex<VecDeque<Response>>,
    changed: Notify,
}

impl ScriptedProvider {
    fn push(&self, response: Response) {
        self.responses.lock().unwrap().push_back(response);
    }

    fn count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }

    async fn reached(&self, count: usize) {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self.count() >= count {
                return;
            }
            changed.await;
        }
    }
}

impl Provider for ScriptedProvider {
    fn chat_stream(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        self.requests.lock().unwrap().push(request);
        self.changed.notify_waiters();
        let response = self.responses.lock().unwrap().pop_front().unwrap();
        async move {
            let (text, usage) = match response {
                Response::Finish(text, usage) => (text, usage),
                Response::Hold(gate, text) => {
                    tokio::select! {
                        biased;
                        _ = cancel.cancelled() => return Err(VegaError::Cancelled),
                        _ = gate.cancelled() => {}
                    }
                    (text, true)
                }
                Response::Fail => return Err(failure("synthetic provider failure")),
                Response::Events(events) => {
                    return Ok(
                        Box::pin(futures::stream::iter(events.into_iter().map(Ok))) as EventStream
                    );
                }
            };
            let mut events = vec![Ok(ProviderEvent::TextDelta(text.into()))];
            if usage {
                events.push(Ok(ProviderEvent::Usage {
                    input: 11,
                    output: 7,
                    cache_read: Some(0),
                    cache_write: Some(0),
                }));
            }
            events.push(Ok(ProviderEvent::Done {
                stop_reason: StopReason::End,
            }));
            Ok(Box::pin(futures::stream::iter(events)) as EventStream)
        }
        .boxed()
    }
}

#[derive(Default)]
struct RecordingHost {
    admitted: Mutex<Vec<SubagentSnapshot>>,
    terminal: Mutex<Vec<SubagentSnapshot>>,
    cancel_tokens: Mutex<Vec<CancellationToken>>,
    reject_next: AtomicBool,
    admission_gate: Mutex<Option<CancellationToken>>,
    cancelling_write_gate: Mutex<Option<Arc<CancellingWriteGate>>>,
    changed: Notify,
}

struct CancellingWriteGate {
    database: PathBuf,
    held: Mutex<Option<Store>>,
}

impl CancellingWriteGate {
    fn hold(&self) -> Result<(), VegaError> {
        let store =
            Store::open(&self.database).map_err(|_| failure("test database unavailable"))?;
        store
            .conn()
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(|_| failure("test write gate unavailable"))?;
        *self.held.lock().unwrap() = Some(store);
        Ok(())
    }

    fn release(&self) {
        let store = self.held.lock().unwrap().take().unwrap();
        store.conn().execute_batch("ROLLBACK").unwrap();
    }
}

impl RecordingHost {
    async fn admitted_count(&self, count: usize) {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self.admitted.lock().unwrap().len() >= count {
                return;
            }
            changed.await;
        }
    }

    async fn ended(&self, message_id: &str) -> SubagentSnapshot {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if let Some(snapshot) = self
                .terminal
                .lock()
                .unwrap()
                .iter()
                .find(|snapshot| snapshot.message_id == message_id)
                .cloned()
            {
                return snapshot;
            }
            changed.await;
        }
    }
}

impl SubagentHost for RecordingHost {
    fn admit(&self, admission: SubagentAdmission) -> BoxFuture<'static, Result<(), VegaError>> {
        self.admitted.lock().unwrap().push(admission.snapshot);
        self.cancel_tokens.lock().unwrap().push(admission.cancel);
        self.changed.notify_waiters();
        let reject = self.reject_next.swap(false, Ordering::SeqCst);
        let gate = self.admission_gate.lock().unwrap().clone();
        async move {
            if let Some(gate) = gate {
                gate.cancelled().await;
            }
            if reject {
                Err(failure("synthetic host rejection"))
            } else {
                Ok(())
            }
        }
        .boxed()
    }

    fn event(&self, _: &str, _: &str, _: &ConversationEvent) -> Result<(), VegaError> {
        Ok(())
    }

    fn updated(&self, snapshot: SubagentSnapshot) -> Result<(), VegaError> {
        if snapshot.state == SubagentState::Cancelling
            && let Some(gate) = self.cancelling_write_gate.lock().unwrap().clone()
        {
            gate.hold()?;
        }
        Ok(())
    }

    fn finished(&self, snapshot: SubagentSnapshot, _: bool) -> Result<(), VegaError> {
        self.terminal.lock().unwrap().push(snapshot);
        self.changed.notify_waiters();
        Ok(())
    }
}

struct Fixture {
    _directory: tempfile::TempDir,
    database: PathBuf,
    store: Store,
    tools: vega_tools::Tools,
    parent: crate::types::Thread,
    provider: Arc<ScriptedProvider>,
    host: Arc<RecordingHost>,
    settings: ChildSettings,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("state.sqlite");
        let store = Store::open(&database).unwrap();
        store.migrate().unwrap();
        let project = vega_store::projects::create(
            store.conn(),
            directory.path().to_str().unwrap(),
            "Workspace",
            None,
        )
        .unwrap();
        let parent =
            crate::threads::create_thread(&store, &project.id, "fixture", "confirm").unwrap();
        let tools = vega_tools::Tools::new(directory.path()).unwrap();
        Self {
            _directory: directory,
            database,
            store,
            tools,
            parent,
            provider: Arc::new(ScriptedProvider::default()),
            host: Arc::new(RecordingHost::default()),
            settings: ChildSettings {
                model: "fixture".into(),
                system_prompt: "Complete the assigned task.".into(),
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
                owner_credential_reader: Some(Arc::new(|| Ok(Vec::new()))),
                turn_limit: 4,
            },
        }
    }

    async fn run<F, Fut, T>(&self, scenario: F) -> T
    where
        F: FnOnce(Run) -> Fut,
        Fut: Future<Output = T>,
    {
        self.run_named("parent-run", scenario).await
    }

    async fn run_named<F, Fut, T>(&self, parent_message: &str, scenario: F) -> T
    where
        F: FnOnce(Run) -> Fut,
        Fut: Future<Output = T>,
    {
        if let Some(parent) = messages::find(self.store.conn(), parent_message).unwrap() {
            assert_eq!(parent.thread_id, self.parent.id);
            assert_eq!(parent.role, "assistant");
        } else {
            messages::insert(
                self.store.conn(),
                &messages::MessageRow {
                    id: parent_message.into(),
                    thread_id: self.parent.id.clone(),
                    seq: messages::next_seq(self.store.conn(), &self.parent.id).unwrap(),
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
            .unwrap();
        }
        let mut supervisor = Supervisor::new();
        let cancel = CancellationToken::new();
        let run = Run {
            hook: supervisor.hook(),
            receipts: supervisor.receipt_observer(),
            database: self.database.clone(),
            parent: self.parent.id.clone(),
            cancel: cancel.clone(),
            provider: self.provider.clone(),
            host: self.host.clone(),
        };
        let launch = ChildLaunch {
            provider: self.provider.as_ref(),
            tools: &self.tools,
            database: self.database.clone(),
            parent_thread: self.parent.id.clone(),
            parent_message: parent_message.into(),
            settings: self.settings.clone(),
            mcp_servers: Vec::new(),
            host: self.host.clone(),
            cancel,
        };
        tokio::time::timeout(
            Duration::from_secs(10),
            supervisor.run(scenario(run).boxed_local(), launch),
        )
        .await
        .expect("subagent scenario must settle without leaking a runner")
    }
}

#[derive(Clone)]
struct Run {
    hook: Arc<dyn vega_runtime::RuntimeSubagentHook>,
    receipts: ReceiptObserver,
    database: PathBuf,
    parent: String,
    cancel: CancellationToken,
    provider: Arc<ScriptedProvider>,
    host: Arc<RecordingHost>,
}

impl Run {
    async fn call(&self, call: &str, tool: &str, input: Value) -> Result<Value, VegaError> {
        self.hook
            .call(call.into(), tool.into(), input, self.cancel.clone())
            .await
    }

    async fn spawn(&self, call: &str, name: &str, task: &str) -> SubagentSnapshot {
        snapshot(
            self.call(call, "spawn_agent", json!({"name": name, "task": task}))
                .await
                .expect("spawn admission must succeed"),
        )
    }

    async fn followup(
        &self,
        call: &str,
        previous: &SubagentSnapshot,
        task: &str,
    ) -> SubagentSnapshot {
        snapshot(
            self.call(call, "followup_agent", followup_input(previous, task))
                .await
                .expect("a completed owned child must accept a related follow-up"),
        )
    }

    fn persisted(&self, call: &str, receipt: &SubagentSnapshot, reused: bool) {
        self.receipts
            .persisted(&RuntimeEvent::ToolCallFinished(RuntimeToolResult {
                call_id: call.into(),
                output: json!({"agent": receipt}).to_string(),
                status: RuntimeToolStatus::Success,
                reused,
                exit_code: None,
                duration_ms: None,
                truncated: None,
                approval: None,
                remember_rule: None,
            }));
    }

    async fn completed(&self, call: &str, name: &str, task: &str) -> SubagentSnapshot {
        let admitted = self.spawn(call, name, task).await;
        self.persisted(call, &admitted, false);
        let completed = self.host.ended(&admitted.message_id).await;
        assert_eq!(completed.state, SubagentState::Completed);
        completed
    }

    fn store(&self) -> Store {
        Store::open(&self.database).unwrap()
    }

    fn messages(&self, child: &str) -> Vec<(String, String, String)> {
        let store = self.store();
        let mut query = store
            .conn()
            .prepare("SELECT id,role,content FROM messages WHERE thread_id=?1 ORDER BY seq")
            .unwrap();
        query
            .query_map([child], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    fn executions(&self, child: &str) -> Vec<(String, String, String)> {
        let store = self.store();
        let mut query = store
            .conn()
            .prepare("SELECT child_message_id,state,result FROM delegated_executions WHERE child_thread_id=?1 ORDER BY ordinal")
            .expect("the additive execution history must exist");
        query
            .query_map([child], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    async fn reject(&self, call: &str, previous: &SubagentSnapshot, task: &str, reason: &str) {
        let messages = self.messages(&previous.agent_id);
        let executions = self.executions(&previous.agent_id);
        let providers = self.provider.count();
        let error = self
            .call(call, "followup_agent", followup_input(previous, task))
            .await
            .expect_err("ineligible follow-up must fail before admission");
        assert!(error.to_string().contains(reason), "{error}");
        assert_eq!(self.messages(&previous.agent_id), messages);
        assert_eq!(self.executions(&previous.agent_id), executions);
        assert_eq!(self.provider.count(), providers);
    }
}

fn snapshot(receipt: Value) -> SubagentSnapshot {
    serde_json::from_value(receipt["agent"].clone()).unwrap()
}

fn followup_input(previous: &SubagentSnapshot, task: &str) -> Value {
    json!({"agent_id": previous.agent_id, "message_id": previous.message_id, "task": task})
}

fn target(snapshot: &SubagentSnapshot) -> Value {
    json!({"agent_id": snapshot.agent_id, "message_id": snapshot.message_id})
}

#[tokio::test]
async fn completed_child_continues_with_history_and_keeps_both_results() {
    let fixture = Fixture::new();
    fixture
        .provider
        .push(Response::Finish("First finding", true));
    fixture
        .provider
        .push(Response::Finish("Verified finding", true));
    fixture
        .run(|run| async move {
            let first = run
                .completed("spawn", "Inspector", "Inspect the parser")
                .await;
            let second = run.followup("continue", &first, "Verify the finding").await;
            assert_eq!(second.agent_id, first.agent_id);
            assert_ne!(second.message_id, first.message_id);
            assert_eq!(second.state, SubagentState::Running);
            assert_eq!(run.provider.count(), 1);
            run.persisted("continue", &second, false);
            let completed = run.host.ended(&second.message_id).await;
            assert_eq!(completed.state, SubagentState::Completed);
            assert_eq!(completed.result, "Verified finding");
            {
                let requests = run.provider.requests.lock().unwrap();
                let history = &requests[1].messages;
                for (role, text) in [
                    (vega_runtime::ChatRole::User, "Inspect the parser"),
                    (vega_runtime::ChatRole::Assistant, "First finding"),
                    (vega_runtime::ChatRole::User, "Verify the finding"),
                ] {
                    assert!(
                        history
                            .iter()
                            .any(|message| message.role == role && message.content == text)
                    );
                }
                assert!(
                    !requests[1]
                        .tools
                        .iter()
                        .any(|tool| matches!(tool.name.as_str(), "spawn_agent" | "followup_agent"))
                );
            }
            assert_eq!(
                run.executions(&first.agent_id),
                vec![
                    (
                        first.message_id.clone(),
                        "completed".into(),
                        "First finding".into()
                    ),
                    (
                        second.message_id.clone(),
                        "completed".into(),
                        "Verified finding".into()
                    ),
                ]
            );
            let store = run.store();
            let old: (String, String) = store
                .conn()
                .query_row(
                    "SELECT child_message_id,result FROM delegated_runs WHERE child_thread_id=?1",
                    [&first.agent_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            assert_eq!(old, (first.message_id, "First finding".into()));
            let children = crate::subagents::children(&store, &run.parent).unwrap();
            assert_eq!(children.len(), 1);
            assert_eq!(children[0].message_id, second.message_id);
            let listed = run.call("list", "list_agents", json!({})).await.unwrap();
            assert_eq!(listed["agents"].as_array().unwrap().len(), 1);
            assert_eq!(listed["agents"][0]["message_id"], second.message_id);
        })
        .await;
}

#[tokio::test]
async fn busy_failed_interrupted_and_foreign_children_reject_without_mutation() {
    let fixture = Fixture::new();
    fixture.provider.push(Response::Fail);
    fixture
        .run(|run| async move {
            let busy = run.spawn("busy", "Busy", "Wait for input").await;
            run.reject("busy-followup", &busy, "More work", "agent_busy")
                .await;
            let cancellation = run
                .call("cancel", "cancel_agent", target(&busy))
                .await
                .unwrap();
            assert!(matches!(
                snapshot(cancellation).state,
                SubagentState::Cancelling | SubagentState::Interrupted
            ));
            let interrupted = run.host.ended(&busy.message_id).await;
            run.reject(
                "interrupted-followup",
                &interrupted,
                "More work",
                "agent_not_completed",
            )
            .await;
            let failed = run.spawn("failed", "Failed", "Try a provider").await;
            run.persisted("failed", &failed, false);
            let failed = run.host.ended(&failed.message_id).await;
            assert_eq!(failed.state, SubagentState::Failed);
            run.reject("failed-followup", &failed, "Retry", "agent_not_completed")
                .await;
            let mut foreign = failed.clone();
            foreign.agent_id = "unowned-child".into();
            run.reject("foreign", &foreign, "Take over", "unknown_agent")
                .await;
        })
        .await;
}

#[tokio::test]
async fn cancelling_child_rejects_followup_before_its_terminal_write_can_finish() {
    let fixture = Fixture::new();
    let gate = Arc::new(CancellingWriteGate {
        database: fixture.database.clone(),
        held: Mutex::new(None),
    });
    *fixture.host.cancelling_write_gate.lock().unwrap() = Some(gate.clone());
    fixture
        .run(|run| async move {
            let busy = run.spawn("spawn", "Worker", "Wait for receipt").await;
            let cancelling = snapshot(
                run.call("cancel", "cancel_agent", json!({"agent_id": busy.agent_id}))
                    .await
                    .unwrap(),
            );
            assert_eq!(cancelling.state, SubagentState::Cancelling);
            assert!(gate.held.lock().unwrap().is_some());
            assert_eq!(run.executions(&busy.agent_id)[0].1, "cancelling");
            run.reject(
                "followup-while-cancelling",
                &busy,
                "More work",
                "agent_busy",
            )
            .await;
            assert_eq!(run.executions(&busy.agent_id).len(), 1);
            assert_eq!(run.executions(&busy.agent_id)[0].1, "cancelling");
            assert!(run.host.terminal.lock().unwrap().is_empty());
            assert_eq!(run.provider.count(), 0);
            assert_eq!(
                vega_store::subagents::active_count(run.store().conn(), &run.parent, "parent-run")
                    .unwrap(),
                1
            );
            gate.release();
            assert_eq!(
                run.host.ended(&busy.message_id).await.state,
                SubagentState::Interrupted
            );
            assert_eq!(
                vega_store::subagents::active_count(run.store().conn(), &run.parent, "parent-run")
                    .unwrap(),
                0
            );
        })
        .await;
}

#[tokio::test]
async fn concurrent_followups_use_compare_and_exchange_and_exact_wait_cancel_targets() {
    let fixture = Fixture::new();
    fixture
        .provider
        .push(Response::Finish("First result", true));
    let gate = CancellationToken::new();
    fixture.provider.push(Response::Hold(gate, "Second result"));
    fixture
        .run(|run| async move {
            let first = run.completed("spawn", "Worker", "Initial task").await;
            let (left, right) = futures::join!(
                run.call(
                    "left",
                    "followup_agent",
                    followup_input(&first, "Left task")
                ),
                run.call(
                    "right",
                    "followup_agent",
                    followup_input(&first, "Right task")
                )
            );
            let (second, loser) = match (left, right) {
                (Ok(value), Err(error)) => (snapshot(value), error),
                (Err(error), Ok(value)) => (snapshot(value), error),
                _ => panic!("exactly one request may acquire the completed generation"),
            };
            assert!(
                loser.to_string().contains("stale_execution")
                    || loser.to_string().contains("agent_busy")
            );
            assert_eq!(run.executions(&first.agent_id).len(), 2);
            let first_wait = run
                .call(
                    "wait-old",
                    "wait_agent",
                    json!({"targets": [target(&first)], "timeout_ms": 0}),
                )
                .await
                .unwrap();
            assert_eq!(first_wait["timed_out"], false);
            assert_eq!(first_wait["agents"][0]["result"], "First result");
            let second_wait = run
                .call(
                    "wait-current",
                    "wait_agent",
                    json!({"targets": [target(&second)], "timeout_ms": 0}),
                )
                .await
                .unwrap();
            assert_eq!(second_wait["timed_out"], true);
            assert_eq!(second_wait["agents"][0]["message_id"], second.message_id);
            let unchanged = snapshot(
                run.call("cancel-old", "cancel_agent", target(&first))
                    .await
                    .unwrap(),
            );
            assert_eq!(unchanged.state, SubagentState::Completed);
            assert!(!run.host.cancel_tokens.lock().unwrap()[1].is_cancelled());
            for (tool, input) in [
                (
                    "wait_agent",
                    json!({"agent_ids": [first.agent_id], "timeout_ms": 0}),
                ),
                ("cancel_agent", json!({"agent_id": first.agent_id})),
            ] {
                let error = run
                    .call("legacy-after-reuse", tool, input)
                    .await
                    .unwrap_err();
                assert!(error.to_string().contains("execution_id_required"));
            }
            run.call("cancel-current", "cancel_agent", target(&second))
                .await
                .unwrap();
            let terminal = run.host.ended(&second.message_id).await;
            assert_eq!(terminal.state, SubagentState::Interrupted);
            assert_eq!(run.executions(&first.agent_id)[0].2, "First result");
            assert_eq!(run.provider.count(), 1);
        })
        .await;
}

#[tokio::test]
async fn repeated_call_returns_its_original_admission_and_rejects_changed_input() {
    let fixture = Fixture::new();
    fixture.provider.push(Response::Finish("One result", true));
    fixture
        .run(|run| async move {
            let first = run.spawn("spawn", "Worker", "Initial task").await;
            let duplicate = run.spawn("spawn", "Worker", "Initial task").await;
            assert_eq!(duplicate, first);
            let error = run
                .call(
                    "spawn",
                    "spawn_agent",
                    json!({"name": "Worker", "task": "Different task"}),
                )
                .await
                .unwrap_err();
            assert!(!error.to_string().is_empty());
            assert_eq!(run.messages(&first.agent_id).len(), 1);
            assert_eq!(run.executions(&first.agent_id).len(), 1);
            assert_eq!(run.host.admitted.lock().unwrap().len(), 1);
            run.persisted("spawn", &first, false);
            run.host.ended(&first.message_id).await;
            let original = run.spawn("spawn", "Worker", "Initial task").await;
            assert_eq!(original, first);
            assert_eq!(run.provider.count(), 1);
        })
        .await;
}

#[tokio::test]
async fn reused_child_waits_for_its_own_new_durable_receipt() {
    let fixture = Fixture::new();
    fixture
        .provider
        .push(Response::Finish("First result", true));
    fixture
        .provider
        .push(Response::Finish("Second result", true));
    fixture
        .run(|run| async move {
            let first = run.completed("spawn", "Worker", "Initial task").await;
            let second = run.followup("continue", &first, "Follow-up task").await;
            run.persisted("spawn", &first, false);
            run.persisted("continue", &second, true);
            assert!(
                tokio::time::timeout(Duration::from_millis(40), run.provider.reached(2))
                    .await
                    .is_err()
            );
            assert_eq!(run.executions(&first.agent_id)[0].1, "completed");
            let replay = run.followup("continue", &first, "Follow-up task").await;
            assert_eq!(replay, second);
            let error = run
                .call(
                    "continue",
                    "followup_agent",
                    followup_input(&first, "Changed task"),
                )
                .await
                .unwrap_err();
            assert!(!error.to_string().is_empty());
            assert_eq!(
                run.messages(&first.agent_id)
                    .iter()
                    .filter(|(_, role, _)| role == "user")
                    .count(),
                2
            );
            run.persisted("continue", &second, false);
            assert_eq!(
                run.host.ended(&second.message_id).await.state,
                SubagentState::Completed
            );
            assert_eq!(run.provider.count(), 2);
        })
        .await;
}

#[tokio::test]
async fn capacity_counts_pending_admissions_and_completed_agents_release_slots() {
    let fixture = Fixture::new();
    fixture
        .provider
        .push(Response::Finish("Ready for reuse", true));
    fixture
        .run(|run| async move {
            let first = run.completed("first", "Reusable", "Initial task").await;
            let admission_gate = CancellationToken::new();
            *run.host.admission_gate.lock().unwrap() = Some(admission_gate.clone());
            let attempts = (0..5).map(|index| {
                let run = run.clone();
                async move {
                    run.call(&format!("spawn-{index}"), "spawn_agent", json!({"name": format!("Worker {index}"), "task": "Do not run before receipt"})).await
                }
            });
            let admissions = futures::future::join_all(attempts);
            let reservation_check = async {
                run.host.admitted_count(5).await;
                run.reject("pending-over-capacity", &first, "Continue", "capacity_exceeded").await;
                assert_eq!(vega_store::subagents::active_count(run.store().conn(), &run.parent, "parent-run").unwrap(), 4);
                assert_eq!(run.provider.count(), 1);
                admission_gate.cancel();
            };
            let (admitted, ()) = futures::join!(admissions, reservation_check);
            let good = admitted.into_iter().filter_map(Result::ok).map(snapshot).collect::<Vec<_>>();
            assert_eq!(good.len(), 4);
            run.reject("over-capacity", &first, "Continue", "capacity_exceeded").await;
            assert_eq!(vega_store::subagents::active_count(run.store().conn(), &run.parent, "parent-run").unwrap(), 4);
            run.call("release-slot", "cancel_agent", target(&good[0])).await.unwrap();
            run.host.ended(&good[0].message_id).await;
            let second = run.followup("reuse-slot", &first, "Continue").await;
            assert_eq!(second.agent_id, first.agent_id);
            assert_eq!(vega_store::subagents::active_count(run.store().conn(), &run.parent, "parent-run").unwrap(), 4);
            assert_eq!(run.provider.count(), 1);
        })
        .await;
    assert_eq!(
        vega_store::subagents::active_count(fixture.store.conn(), &fixture.parent.id, "parent-run")
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn externally_appended_edited_and_pending_messages_invalidate_continuation() {
    for modification in ["append", "edit", "pending"] {
        let fixture = Fixture::new();
        fixture
            .provider
            .push(Response::Finish("Original result", true));
        fixture
            .run(|run| async move {
                let first = run.completed("spawn", "Worker", "Initial task").await;
                let store = run.store();
                match modification {
                    "append" => {
                        store.conn().execute("INSERT INTO messages(id,thread_id,seq,role,kind,content,status,created_at) SELECT 'manual',?1,MAX(seq)+1,'user','text','Manual continuation','done',50 FROM messages WHERE thread_id=?1", [&first.agent_id]).unwrap();
                    }
                    "edit" => {
                        store.conn().execute("UPDATE messages SET content='Edited instruction' WHERE thread_id=?1 AND role='user'", [&first.agent_id]).unwrap();
                    }
                    _ => {
                        vega_store::pending_messages::insert(store.conn(), &vega_store::pending_messages::PendingMessageRow {
                            id: "queued-manual".into(), thread_id: first.agent_id.clone(), ordinal: 0,
                            behavior: "queue".into(), generation: "manual".into(),
                            payload: json!({"content": "Pending manual input"}).to_string(), created_at: 50,
                        }).unwrap();
                    }
                }
                run.reject("continue", &first, "Automatic continuation", "externally_continued").await;
                assert_eq!(run.executions(&first.agent_id)[0].2, "Original result");
            })
            .await;
    }
}

#[tokio::test]
async fn rejected_input_rolls_back_reservation_and_host_rejection_preserves_failed_execution() {
    let mut fixture = Fixture::new();
    fixture.settings.owner_credential_reader = Some(Arc::new(|| {
        Ok(vec!["synthetic-private-provider-token".into()])
    }));
    fixture
        .provider
        .push(Response::Finish("Original result", true));
    fixture
        .run(|run| async move {
            let first = run.completed("spawn", "Worker", "Initial task").await;
            let messages = run.messages(&first.agent_id);
            let denied = run
                .call(
                    "unsafe",
                    "followup_agent",
                    followup_input(&first, "synthetic-private-provider-token"),
                )
                .await;
            assert!(denied.is_err());
            assert_eq!(run.messages(&first.agent_id), messages);
            assert_eq!(run.executions(&first.agent_id).len(), 1);
            run.host.reject_next.store(true, Ordering::SeqCst);
            assert!(
                run.call(
                    "host-rejected",
                    "followup_agent",
                    followup_input(&first, "Related work")
                )
                .await
                .is_err()
            );
            let admitted = run.host.admitted.lock().unwrap().last().unwrap().clone();
            let terminal = run.host.ended(&admitted.message_id).await;
            assert_eq!(terminal.state, SubagentState::Failed);
            assert_eq!(run.executions(&first.agent_id).len(), 2);
            assert_eq!(run.executions(&first.agent_id)[0].2, "Original result");
            assert_eq!(run.executions(&first.agent_id)[1].1, "failed");
            assert_eq!(
                vega_store::subagents::active_count(run.store().conn(), &run.parent, "parent-run")
                    .unwrap(),
                0
            );
            assert_eq!(run.provider.count(), 1);
        })
        .await;
}

#[tokio::test]
async fn changed_mode_permissions_or_workspace_cannot_expand_reuse_authority() {
    for change in [
        "UPDATE threads SET mode='ask' WHERE id=?1",
        "UPDATE threads SET permission_mode='full_access' WHERE id=?1",
        "UPDATE threads SET project_id=NULL,workspace_owner_thread_id=NULL WHERE id=?1",
    ] {
        let fixture = Fixture::new();
        fixture
            .provider
            .push(Response::Finish("Original result", true));
        fixture
            .run(|run| async move {
                let first = run.completed("spawn", "Worker", "Initial task").await;
                run.store()
                    .conn()
                    .execute(change, [&first.agent_id])
                    .unwrap();
                let messages = run.messages(&first.agent_id);
                assert!(
                    run.call(
                        "changed-authority",
                        "followup_agent",
                        followup_input(&first, "Continue")
                    )
                    .await
                    .is_err()
                );
                assert_eq!(run.messages(&first.agent_id), messages);
                assert_eq!(run.executions(&first.agent_id).len(), 1);
                assert_eq!(run.executions(&first.agent_id)[0].2, "Original result");
                assert_eq!(run.provider.count(), 1);
                assert_eq!(
                    vega_store::subagents::active_count(
                        run.store().conn(),
                        &run.parent,
                        "parent-run"
                    )
                    .unwrap(),
                    0
                );
            })
            .await;
    }
}

#[tokio::test]
async fn parent_exit_interrupts_new_execution_without_rewriting_completed_history() {
    let fixture = Fixture::new();
    fixture
        .provider
        .push(Response::Finish("Original result", true));
    let (first, second, hook) = fixture
        .run(|run| async move {
            let first = run.completed("spawn", "Worker", "Initial task").await;
            let second = run.followup("continue", &first, "Pending receipt").await;
            assert!(crate::threads::delete_thread(&run.store(), &first.agent_id).is_err());
            assert!(crate::threads::delete_thread(&run.store(), &run.parent).is_err());
            (first, second, run.hook.clone())
        })
        .await;
    let records: Vec<(String, String)> = fixture.store.conn().prepare(
        "SELECT state,result FROM delegated_executions WHERE child_thread_id=?1 ORDER BY ordinal"
    ).unwrap().query_map([&first.agent_id], |row| Ok((row.get(0)?,row.get(1)?))).unwrap().map(Result::unwrap).collect();
    assert_eq!(records[0], ("completed".into(), "Original result".into()));
    assert_eq!(records[1].0, "interrupted");
    assert_eq!(fixture.provider.count(), 1);
    let ended = hook
        .call(
            "after-end".into(),
            "followup_agent".into(),
            followup_input(&second, "New work"),
            CancellationToken::new(),
        )
        .await
        .unwrap_err();
    assert!(ended.to_string().contains("parent_run_ended"));
    crate::threads::delete_thread(&fixture.store, &first.agent_id).unwrap();
    let remaining: i64 = fixture
        .store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM delegated_executions WHERE child_thread_id=?1",
            [&first.agent_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(remaining, 0);
}

#[tokio::test]
async fn transaction_failure_rolls_back_input_and_releases_the_reservation() {
    let fixture = Fixture::new();
    fixture
        .provider
        .push(Response::Finish("Original result", true));
    fixture.run(|run| async move {
        let first = run.completed("spawn", "Worker", "Initial task").await;
        let messages = run.messages(&first.agent_id);
        let store = run.store();
        store.conn().execute_batch("CREATE TRIGGER reject_followup BEFORE INSERT ON delegated_executions WHEN NEW.kind='followup' BEGIN SELECT RAISE(FAIL,'synthetic write failure'); END;").unwrap();
        assert!(run.call("write-failure", "followup_agent", followup_input(&first, "Related work")).await.is_err());
        assert_eq!(run.messages(&first.agent_id), messages);
        assert_eq!(run.executions(&first.agent_id).len(), 1);
        assert_eq!(run.provider.count(), 1);
        assert_eq!(vega_store::subagents::active_count(store.conn(), &run.parent, "parent-run").unwrap(), 0);
        store.conn().execute_batch("DROP TRIGGER reject_followup;").unwrap();
        let second = run.followup("retry-admission", &first, "Related work").await;
        assert_eq!(second.agent_id, first.agent_id);
        assert_eq!(run.executions(&first.agent_id).len(), 2);
    }).await;
}

#[tokio::test]
async fn failed_followup_preserves_original_result_and_releases_its_slot() {
    let fixture = Fixture::new();
    fixture
        .provider
        .push(Response::Finish("Original result", true));
    fixture.provider.push(Response::Fail);
    fixture
        .run(|run| async move {
            let first = run.completed("spawn", "Worker", "Initial task").await;
            let second = run.followup("continue", &first, "Related work").await;
            run.persisted("continue", &second, false);
            let terminal = run.host.ended(&second.message_id).await;
            assert_eq!(terminal.state, SubagentState::Failed);
            assert_eq!(run.executions(&first.agent_id)[0].2, "Original result");
            assert_eq!(run.executions(&first.agent_id)[1].1, "failed");
            assert_eq!(
                vega_store::subagents::active_count(run.store().conn(), &run.parent, "parent-run")
                    .unwrap(),
                0
            );
            run.reject("retry-failed", &terminal, "Retry", "agent_not_completed")
                .await;
        })
        .await;
}

#[tokio::test]
async fn legacy_single_execution_wait_and_cancel_remain_readable() {
    let fixture = Fixture::new();
    fixture
        .provider
        .push(Response::Finish("Legacy result", true));
    fixture
        .run(|run| async move {
            let first = run.completed("spawn", "Worker", "Initial task").await;
            let waited = run
                .call(
                    "old-wait",
                    "wait_agent",
                    json!({"agent_ids": [first.agent_id], "timeout_ms": 0}),
                )
                .await
                .unwrap();
            assert_eq!(waited["agents"][0]["message_id"], first.message_id);
            assert_eq!(waited["agents"][0]["result"], "Legacy result");
            let cancelled = snapshot(
                run.call(
                    "old-cancel",
                    "cancel_agent",
                    json!({"agent_id": first.agent_id}),
                )
                .await
                .unwrap(),
            );
            assert_eq!(cancelled.state, SubagentState::Completed);
            assert_eq!(cancelled.result, "Legacy result");
            assert_eq!(run.provider.count(), 1);
        })
        .await;
}

#[tokio::test]
async fn persisted_children_from_an_earlier_parent_run_are_not_reusable() {
    let fixture = Fixture::new();
    fixture
        .provider
        .push(Response::Finish("Original result", true));
    let first = fixture
        .run(|run| async move { run.completed("spawn", "Worker", "Initial task").await })
        .await;
    fixture
        .run_named("later-parent-run", |run| async move {
            run.reject("foreign-run", &first, "Continue", "unknown_agent")
                .await;
            let error = run
                .call(
                    "foreign-wait",
                    "wait_agent",
                    json!({"targets": [target(&first)], "timeout_ms": 0}),
                )
                .await
                .unwrap_err();
            assert!(error.to_string().contains("unknown_agent"));
            let listed = run.call("list", "list_agents", json!({})).await.unwrap();
            assert!(listed["agents"].as_array().unwrap().is_empty());
        })
        .await;
}

#[tokio::test]
async fn derived_summary_does_not_invalidate_the_source_history_for_reuse() {
    let fixture = Fixture::new();
    fixture
        .provider
        .push(Response::Finish("Original result", true));
    fixture
        .run(|run| async move {
            let first = run.completed("spawn", "Worker", "Initial task").await;
            let store = run.store();
            let source =
                vega_store::context_compaction::load_source_version(store.conn(), &first.agent_id)
                    .unwrap();
            let installed = vega_store::context_compaction::install_checkpoint(
                store.conn(),
                &vega_store::context_compaction::NewContextCheckpoint {
                    thread_id: first.agent_id.clone(),
                    model: "fixture".into(),
                    source_version: source.source_version,
                    covered_through_seq: 1,
                    covered_assistant_id: None,
                    covered_assistant_text_bytes: 0,
                    covered_assistant_tool_seq: 0,
                    source_fingerprint: source.fingerprint,
                    summary: "Summary of the initial task".into(),
                    estimator_version: "fixture".into(),
                    expected_previous_id: None,
                    created_at: 100,
                },
            )
            .unwrap();
            assert!(matches!(
                installed,
                vega_store::context_compaction::ContextCheckpointInstall::Applied(_)
            ));
            let second = run.followup("continue", &first, "Follow-up task").await;
            assert_eq!(second.agent_id, first.agent_id);
            assert_eq!(run.executions(&first.agent_id).len(), 2);
        })
        .await;
}

#[path = "subagent_pipeline_tests.rs"]
mod pipeline_tests;
