use super::*;
use vega_runtime::{
    ChatRequest, EventStream, ProviderEvent, RuntimeSubagentHook, RuntimeToolResult, StopReason,
};

struct Model {
    calls: std::sync::atomic::AtomicUsize,
    block: bool,
}

impl Provider for Model {
    fn chat_stream(
        &self,
        _: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let block = self.block;
        async move {
            if block {
                cancel.cancelled().await;
                return Err(VegaError::Cancelled);
            }
            let stream: EventStream = Box::pin(futures::stream::iter([
                Ok(ProviderEvent::TextDelta("completed child".into())),
                Ok(ProviderEvent::Usage {
                    input: 10,
                    output: 3,
                    cache_read: Some(0),
                    cache_write: Some(0),
                }),
                Ok(ProviderEvent::Done {
                    stop_reason: StopReason::End,
                }),
            ]));
            Ok(stream)
        }
        .boxed()
    }
}

#[derive(Default)]
struct Host {
    finished: Mutex<Vec<SubagentSnapshot>>,
}

impl SubagentHost for Host {
    fn admit(&self, _: SubagentAdmission) -> BoxFuture<'static, Result<(), VegaError>> {
        async { Ok(()) }.boxed()
    }
    fn event(
        &self,
        _: &str,
        _: &str,
        _: &crate::types::ConversationEvent,
    ) -> Result<(), VegaError> {
        Ok(())
    }
    fn updated(&self, _: SubagentSnapshot) -> Result<(), VegaError> {
        Ok(())
    }
    fn finished(&self, snapshot: SubagentSnapshot, _: bool) -> Result<(), VegaError> {
        self.finished.lock().unwrap().push(snapshot);
        Ok(())
    }
}

async fn admitted(
    hook: &Arc<dyn RuntimeSubagentHook>,
    observer: &ReceiptObserver,
    tool: &str,
    input: serde_json::Value,
) -> SubagentSnapshot {
    let call = id();
    let value = hook
        .call(call.clone(), tool.into(), input, CancellationToken::new())
        .await
        .unwrap();
    observer.persisted(&RuntimeEvent::ToolCallFinished(RuntimeToolResult {
        call_id: call,
        output: value.to_string(),
        status: RuntimeToolStatus::Success,
        reused: false,
        exit_code: None,
        duration_ms: None,
        truncated: None,
        approval: None,
        remember_rule: None,
    }));
    serde_json::from_value(value["agent"].clone()).unwrap()
}

async fn released(
    hook: &Arc<dyn RuntimeSubagentHook>,
    snapshots: &[SubagentSnapshot],
) -> Vec<SubagentSnapshot> {
    let mut agents = Vec::new();
    for snapshot in snapshots {
        let value = hook
            .call(
                id(),
                "wait_agent".into(),
                serde_json::json!({"targets":[{"agent_id":snapshot.agent_id,"message_id":snapshot.message_id}]}),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(value["wake_reason"], "terminal");
        let mut released: Vec<SubagentSnapshot> =
            serde_json::from_value(value["agents"].clone()).unwrap();
        assert_eq!(released.len(), 1);
        agents.append(&mut released);
    }
    assert!(
        agents
            .iter()
            .all(|s| s.state == SubagentState::Completed && s.release_state == "released"),
        "terminal agents: {agents:?}"
    );
    agents
}

#[tokio::test]
async fn real_supervisor_spawn_followup_receipts_usage_and_release_survive_repeated_steering() {
    let mut fixture = Fixture::new();
    let model = Model {
        calls: std::sync::atomic::AtomicUsize::new(0),
        block: false,
    };
    let host = Arc::new(Host::default());
    let tools = vega_tools::Tools::new(fixture._directory.path()).unwrap();
    let mut supervisor = Supervisor::new();
    let hook = supervisor.hook();
    let observer = supervisor.receipt_observer();
    let launch = ChildLaunch {
        provider: &model,
        tools: &tools,
        database: fixture.database.clone(),
        parent_thread: fixture.parent.clone(),
        parent_message: fixture.root.clone(),
        settings: fixture.settings.clone(),
        mcp_servers: Vec::new(),
        host: host.clone(),
        cancel: CancellationToken::new(),
    };
    let parent = async {
        let first = admitted(&hook, &observer, "spawn_agent", serde_json::json!({"name":"first","task":"read"})).await;
        fixture.steer(); fixture.steer(); fixture.steer();
        let second = admitted(&hook, &observer, "spawn_agent", serde_json::json!({"name":"second","task":"read"})).await;
        let agents = released(&hook, &[first, second]).await;
        assert!(agents.iter().all(|s| s.result == "completed child"));
        let previous = &agents[0];
        for change in ["stale", "foreign", "unreleased"] {
            let input = match change {
                "stale" => serde_json::json!({"agent_id":previous.agent_id,"message_id":id(),"task":"continue"}),
                "foreign" => serde_json::json!({"agent_id":id(),"message_id":previous.message_id,"task":"continue"}),
                "unreleased" => {
                    let mut state = observer.0.state.lock().unwrap();
                    state.executions.get_mut(&(previous.agent_id.clone(),previous.message_id.clone())).unwrap().occupied = true;
                    serde_json::json!({"agent_id":previous.agent_id,"message_id":previous.message_id,"task":"continue"})
                }
                _ => unreachable!(),
            };
            assert!(hook.call(id(), "followup_agent".into(), input, CancellationToken::new()).await.is_err());
            if change == "unreleased" { observer.0.state.lock().unwrap().executions.get_mut(&(previous.agent_id.clone(),previous.message_id.clone())).unwrap().occupied = false; }
        }
        let mut previous = previous.clone();
        for _ in 0..3 {
            fixture.steer(); fixture.steer(); fixture.steer();
            let next = admitted(&hook, &observer, "followup_agent", serde_json::json!({"agent_id":previous.agent_id,"message_id":previous.message_id,"task":"continue"})).await;
            assert_eq!(next.agent_id, previous.agent_id);
            assert_ne!(next.message_id, previous.message_id);
            previous = released(&hook, &[next]).await.remove(0);
            assert_eq!(previous.result, "completed child");
        }
    }.boxed_local();
    tokio::time::timeout(Duration::from_secs(30), supervisor.run(parent, launch))
        .await
        .unwrap();
    assert_eq!(model.calls.load(std::sync::atomic::Ordering::SeqCst), 5);
    assert_eq!(host.finished.lock().unwrap().len(), 5);
    let usage: (i64,i64,i64) = fixture.store.conn().query_row(
        "SELECT COUNT(*),SUM(input_tokens),SUM(output_tokens) FROM token_usage WHERE delegated_parent_thread_id=?1 AND delegated_parent_message_id=?2",
        [&fixture.parent,&fixture.root], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))).unwrap();
    assert_eq!(usage, (5, 50, 15));
    assert!(
        supervisor
            .registry
            .state
            .lock()
            .unwrap()
            .executions
            .values()
            .all(|record| !record.occupied && record.writer.is_none())
    );
    let rows =
        vega_store::subagents::list(fixture.store.conn(), &fixture.parent, Some(&fixture.root))
            .unwrap();
    assert_eq!(rows.len(), 2);
    assert!(
        rows.iter()
            .all(|row| row.state == "completed" && row.result == "completed child")
    );
}

#[tokio::test]
async fn supervisor_stop_drains_children_and_rejects_later_spawn_or_followup() {
    let mut fixture = Fixture::new();
    fixture.steer();
    fixture.steer();
    fixture.steer();
    let model = Model {
        calls: std::sync::atomic::AtomicUsize::new(0),
        block: true,
    };
    let host = Arc::new(Host::default());
    let tools = vega_tools::Tools::new(fixture._directory.path()).unwrap();
    let mut supervisor = Supervisor::new();
    let hook = supervisor.hook();
    let observer = supervisor.receipt_observer();
    let cancel = CancellationToken::new();
    let launch = ChildLaunch {
        provider: &model,
        tools: &tools,
        database: fixture.database.clone(),
        parent_thread: fixture.parent.clone(),
        parent_message: fixture.root.clone(),
        settings: fixture.settings.clone(),
        mcp_servers: Vec::new(),
        host: host.clone(),
        cancel: cancel.clone(),
    };
    let parent = async {
        let child = admitted(&hook, &observer, "spawn_agent", serde_json::json!({"name":"blocking","task":"read"})).await;
        cancel.cancel();
        for (tool,input) in [("spawn_agent",serde_json::json!({"name":"later","task":"read"})),
            ("followup_agent",serde_json::json!({"agent_id":child.agent_id,"message_id":child.message_id,"task":"continue"}))] {
            assert!(hook.call(id(), tool.into(), input, CancellationToken::new()).await.is_err());
        }
        child
    }.boxed_local();
    let child = tokio::time::timeout(Duration::from_secs(30), supervisor.run(parent, launch))
        .await
        .unwrap();
    let row =
        vega_store::subagents::execution(fixture.store.conn(), &child.agent_id, &child.message_id)
            .unwrap()
            .unwrap()
            .record;
    assert_eq!(row.state, "interrupted");
    assert_eq!(
        vega_store::delegated_stages::release_state(
            fixture.store.conn(),
            &child.agent_id,
            &child.message_id
        )
        .unwrap(),
        "released"
    );
    assert!(
        supervisor
            .registry
            .state
            .lock()
            .unwrap()
            .executions
            .values()
            .all(|record| !record.occupied && record.writer.is_none())
    );
    let _owner = DelegatedOwnership::acquire(&fixture.database, &child.agent_id).unwrap();
    let _admission = crate::thread_access::acquire(&fixture.store, &child.agent_id).unwrap();
}

#[tokio::test]
async fn cancellation_while_admission_is_waiting_cannot_commit_spawn_or_followup() {
    for followup in [false, true] {
        let mut fixture = Fixture::new();
        let (snapshot, previous) = if followup {
            let first = fixture.snapshot(None);
            fixture
                .admit(&first, None, CancellationToken::new())
                .await
                .unwrap();
            let boundary = fixture.complete(&first);
            (fixture.snapshot(Some(&first.agent_id)), Some(boundary))
        } else {
            (fixture.snapshot(None), None)
        };
        fixture.steer();
        fixture.steer();
        fixture.steer();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let blocker = std::thread::spawn(move || {
            let _coordination = crate::permission_contracts::coordinate().unwrap();
            ready_tx.send(()).unwrap();
            release_rx.recv().unwrap();
        });
        ready_rx.recv().unwrap();
        let owner = DelegatedOwnership::acquire(&fixture.database, &snapshot.agent_id).unwrap();
        let admission = crate::thread_access::acquire(&fixture.store, &snapshot.agent_id).unwrap();
        let cancel = CancellationToken::new();
        let counts = fixture.counts();
        let pending = create_child(
            fixture.database.clone(),
            snapshot.clone(),
            "task".into(),
            id(),
            fixture.settings.clone(),
            cancel.clone(),
            previous,
            None,
            owner,
            admission,
        );
        tokio::pin!(pending);
        assert!(futures::poll!(&mut pending).is_pending());
        cancel.cancel();
        release_tx.send(()).unwrap();
        blocker.join().unwrap();
        assert!(matches!(pending.await, Err(VegaError::Cancelled)));
        assert_eq!(fixture.counts(), counts);
    }
}
