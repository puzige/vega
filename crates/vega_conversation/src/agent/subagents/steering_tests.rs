use super::*;
use std::collections::HashMap;
use vega_runtime::{
    ChatMessage, ChatRequest, ChatRole, EventStream, ProviderEvent, RuntimeSteeringInput,
    RuntimeSteeringSource, StopReason,
};

const PARENT: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

struct ObservedQueue {
    inner: crate::pending_messages::QueueSteeringSource,
    empty: mpsc::UnboundedSender<()>,
}

impl RuntimeSteeringSource for ObservedQueue {
    fn ready(&self) -> BoxFuture<'_, Result<Option<String>, VegaError>> {
        async move {
            let ready = self.inner.ready().await?;
            if ready.is_none() {
                let _ = self.empty.send(());
            }
            Ok(ready)
        }
        .boxed()
    }

    fn take(
        &self,
        frozen: Vec<vega_runtime::skills::SkillSelection>,
    ) -> BoxFuture<'_, Result<Option<RuntimeSteeringInput>, VegaError>> {
        self.inner.take(frozen)
    }

    fn acknowledge<'a>(
        &'a self,
        input: &'a RuntimeSteeringInput,
    ) -> BoxFuture<'a, Result<(), VegaError>> {
        self.inner.acknowledge(input)
    }
}

struct Fixture {
    root: PathBuf,
    database: PathBuf,
    project: String,
    source: Arc<ObservedQueue>,
    empty: mpsc::UnboundedReceiver<()>,
    registry: Registry,
    hook: Arc<Hook>,
    child_cancel: CancellationToken,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("vega-wait-{}", ulid::Ulid::generate()));
        fs::create_dir_all(&root).unwrap();
        let database = root.join("queue.sqlite");
        let store = Store::open(&database).unwrap();
        store.migrate().unwrap();
        let project = vega_store::projects::create(
            store.conn(),
            root.to_str().unwrap(),
            "wait regression",
            None,
        )
        .unwrap()
        .id;
        vega_store::threads::create(
            store.conn(),
            vega_store::threads::NewThread {
                id: PARENT,
                project_id: &project,
                title: "wait regression",
                mode: "execute",
                permission_mode: "readonly",
                permission_revision: 0,
                full_access_grant: None,
                model: "scripted",
                status: "active",
                pinned: false,
                unread: false,
                created_at: 1,
                updated_at: 1,
                reasoning_preferences: None,
            },
        )
        .unwrap();
        let row = vega_store::threads::find(store.conn(), PARENT)
            .unwrap()
            .unwrap();
        let fence = crate::permission_contracts::capture_fence(&store, &row).unwrap();
        let queue = crate::pending_messages::QueueSteeringSource::new(
            database.clone(),
            root.clone(),
            PARENT.into(),
            "generation".into(),
            Arc::new(|| Ok(Vec::new())),
            CancellationToken::new(),
        );
        assert!(queue.authority_slot().set(fence).is_ok());
        let (empty_tx, empty) = mpsc::unbounded_channel();
        let source = Arc::new(ObservedQueue {
            inner: queue,
            empty: empty_tx,
        });
        let changed = Arc::new(Notify::new());
        let child_cancel = CancellationToken::new();
        let snapshot = SubagentSnapshot {
            agent_id: "child".into(),
            message_id: "execution".into(),
            parent_thread_id: PARENT.into(),
            parent_message_id: "assistant".into(),
            name: "held child".into(),
            state: SubagentState::Running,
            result: String::new(),
            truncated: false,
            checkpoint: None,
            release_state: "held".into(),
            ready_for_review: false,
        };
        let record = AgentRecord {
            snapshot,
            call_id: "spawn".into(),
            cancel: child_cancel.clone(),
            receipt_persisted: CancellationToken::new(),
            writer: None,
            occupied: true,
            admitted: true,
            committed: true,
            boundary: None,
            activity: super::super::stage_activity::StageActivity::new(changed.clone()),
            delivered_attention: 0,
            bounded_observed: false,
            terminal_delivered: false,
        };
        let mut state = RegistryState {
            steering: Some(source.clone()),
            ..Default::default()
        };
        state.latest.insert("child".into(), "execution".into());
        state
            .executions
            .insert(("child".into(), "execution".into()), record);
        let registry = Registry {
            state: Arc::new(Mutex::new(state)),
            changed,
        };
        let (sender, _) = mpsc::channel(1);
        let hook = Arc::new(Hook {
            sender,
            registry: registry.clone(),
        });
        Self {
            root,
            database,
            project,
            source,
            empty,
            registry,
            hook,
            child_cancel,
        }
    }

    fn enqueue(&self, id: &str, behavior: crate::types::PendingMessageBehavior, generation: &str) {
        let store = Store::open(&self.database).unwrap();
        let row = vega_store::threads::find(store.conn(), PARENT)
            .unwrap()
            .unwrap();
        let fence = crate::permission_contracts::capture_fence(&store, &row).unwrap();
        crate::pending_messages::enqueue(
            &store,
            &crate::types::PendingMessage {
                id: id.into(),
                thread_id: PARENT.into(),
                generation: generation.into(),
                behavior,
                content: format!("instruction {id}"),
                images: Vec::new(),
                references: Vec::new(),
                reference_mentions: Vec::new(),
                skill_mentions: Vec::new(),
                permission_fence: Some(fence),
            },
        )
        .unwrap();
    }

    fn assert_held(&self) {
        let agents = self.registry.snapshots(None).unwrap();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].state, SubagentState::Running);
        assert_eq!(agents[0].release_state, "held");
        assert!(
            self.registry
                .state
                .lock()
                .unwrap()
                .executions
                .values()
                .all(|r| r.occupied)
        );
        assert!(!self.child_cancel.is_cancelled());
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn arguments(timeout: Option<u64>) -> serde_json::Value {
    let mut input = serde_json::json!({"targets":[{"agent_id":"child","message_id":"execution"}]});
    if let Some(timeout) = timeout {
        input["timeout_ms"] = timeout.into();
    }
    input
}

struct ScriptedProvider {
    waits: usize,
    requests: Arc<Mutex<Vec<ChatRequest>>>,
}

impl Provider for ScriptedProvider {
    fn chat_stream(
        &self,
        req: ChatRequest,
        _: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        let mut requests = self.requests.lock().unwrap();
        let first = requests.is_empty();
        requests.push(req);
        let events = if first {
            let mut events = (0..self.waits)
                .map(|index| {
                    Ok(ProviderEvent::ToolUse {
                        id: format!("wait-{index}"),
                        name: "wait_agent".into(),
                        input_json: arguments(Some(60_000)).to_string(),
                    })
                })
                .collect::<Vec<_>>();
            events.push(Ok(ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            }));
            events
        } else {
            vec![
                Ok(ProviderEvent::TextDelta("accepted".into())),
                Ok(ProviderEvent::Done {
                    stop_reason: StopReason::End,
                }),
            ]
        };
        Box::pin(async move { Ok(Box::pin(futures::stream::iter(events)) as EventStream) })
    }
}

async fn batch_accepts_steering(waits: usize) {
    let mut fixture = Fixture::new();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let provider = ScriptedProvider {
        waits,
        requests: requests.clone(),
    };
    let tools = vega_tools::Tools::new(&fixture.root).unwrap();
    let config = RuntimeToolConfig::new(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::ReadOnly,
        fixture.project.clone(),
        PARENT.into(),
        fixture.root.join("checkpoints"),
        Vec::new(),
    )
    .with_subagents(Some(fixture.hook.clone()))
    .with_steering_source(fixture.source.clone());
    let request = AgentRequest {
        model: "scripted".into(),
        system_prompt: "Follow the user".into(),
        history: vec![ChatMessage::new(ChatRole::User, "wait")],
        max_tokens: None,
        completed_tool_results: HashMap::new(),
        tool_config: config,
        history_safety: None,
        pricing_catalog: None,
        reasoning: None,
        model_snapshot: None,
        replay_frame: None,
        context_budget: None,
        context_source_version: None,
        context_source_fingerprint: None,
        context_operation_id: None,
        context_compaction_hook: None,
    };
    let source = fixture.source.clone();
    let hook = fixture.hook.clone();
    let database = fixture.database.clone();
    let outcome = {
        let run = vega_runtime::run_agent(&provider, &tools, request, CancellationToken::new());
        let inject = async {
            fixture.empty.recv().await.unwrap();
            fixture.enqueue(
                "first",
                crate::types::PendingMessageBehavior::Steer,
                "generation",
            );
        };
        let (outcome, ()) =
            tokio::time::timeout(Duration::from_secs(10), async { tokio::join!(run, inject) })
                .await
                .expect("batch must reach steering acceptance while child is held");
        outcome.unwrap()
    };
    assert!(!outcome.interrupted);
    assert!(!outcome.failed);
    fixture.assert_held();
    {
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        let history = &requests[1].messages;
        let results = history
            .iter()
            .filter(|m| m.role == ChatRole::Tool)
            .collect::<Vec<_>>();
        assert_eq!(results.len(), waits);
        for (index, result) in results.iter().enumerate() {
            assert_eq!(
                result.tool_call_id.as_deref(),
                Some(format!("wait-{index}").as_str())
            );
            let receipt: serde_json::Value = serde_json::from_str(&result.content).unwrap();
            assert_eq!(receipt["wake_reason"], "steering");
            assert_eq!(receipt["timeout_coalesced"], index > 0);
            assert_eq!(receipt["timed_out"], false);
            assert_eq!(receipt["agents"][0]["state"], "running");
        }
        let instructions = history
            .iter()
            .enumerate()
            .filter(|(_, m)| m.role == ChatRole::User && m.content == "instruction first")
            .collect::<Vec<_>>();
        assert_eq!(instructions.len(), 1);
        let last_tool = history
            .iter()
            .rposition(|m| m.role == ChatRole::Tool)
            .unwrap();
        assert!(instructions[0].0 > last_tool);
    }
    let store = Store::open(&database).unwrap();
    let state: String = store
        .conn()
        .query_row(
            "SELECT state FROM pending_messages WHERE id='first'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(state, "claimed");
    assert!(source.ready().await.unwrap().is_none());
    assert!(source.take(Vec::new()).await.unwrap().is_none());
    while fixture.empty.try_recv().is_ok() {}
    let wait = vega_runtime::RuntimeSubagentHook::call(
        hook.as_ref(),
        "after".into(),
        "wait_agent".into(),
        arguments(Some(1)),
        CancellationToken::new(),
    );
    tokio::pin!(wait);
    tokio::select! {
        result = &mut wait => panic!("accepted message must not wake a later wait: {result:?}"),
        signal = fixture.empty.recv() => { signal.unwrap(); }
    }
    assert!(futures::poll!(&mut wait).is_pending());
    fixture.enqueue(
        "second",
        crate::types::PendingMessageBehavior::Steer,
        "generation",
    );
    let receipt = tokio::time::timeout(Duration::from_secs(10), wait)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(receipt["wake_reason"], "steering");
    assert_eq!(receipt["timeout_coalesced"], true);
    let next = source.take(Vec::new()).await.unwrap().unwrap();
    assert_eq!(next.message.content, "instruction second");
    source.acknowledge(&next).await.unwrap();
    assert!(source.take(Vec::new()).await.unwrap().is_none());
    fixture.assert_held();
}

#[tokio::test]
async fn two_waits_accept_pending_steering_once_before_child_completion() {
    batch_accepts_steering(2).await;
}

#[tokio::test]
async fn three_repeated_waits_accept_steering_with_coalesced_deadlines() {
    batch_accepts_steering(3).await;
}

#[tokio::test]
async fn ineligible_queue_entries_do_not_wake_and_cancellation_still_interrupts() {
    let mut fixture = Fixture::new();
    fixture.enqueue(
        "queued",
        crate::types::PendingMessageBehavior::Queue,
        "generation",
    );
    fixture.enqueue(
        "old",
        crate::types::PendingMessageBehavior::Steer,
        "old-generation",
    );
    fixture.enqueue(
        "invalid",
        crate::types::PendingMessageBehavior::Steer,
        "generation",
    );
    let store = Store::open(&fixture.database).unwrap();
    store
        .conn()
        .execute(
            "UPDATE pending_messages SET permission_invalidated=1 WHERE id='invalid'",
            [],
        )
        .unwrap();
    let cancel = CancellationToken::new();
    let hook = fixture.hook.clone();
    let wait = vega_runtime::RuntimeSubagentHook::call(
        hook.as_ref(),
        "cancel".into(),
        "wait_agent".into(),
        arguments(None),
        cancel.clone(),
    );
    tokio::pin!(wait);
    tokio::select! {
        result = &mut wait => panic!("ineligible messages woke wait: {result:?}"),
        signal = fixture.empty.recv() => { signal.unwrap(); }
    }
    assert!(futures::poll!(&mut wait).is_pending());
    cancel.cancel();
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(10), wait)
            .await
            .unwrap(),
        Err(VegaError::Cancelled)
    ));
    fixture.assert_held();
}

#[tokio::test]
async fn no_message_preserves_timeout_coalescing_and_released_terminal_results() {
    let mut fixture = Fixture::new();
    let hook = fixture.hook.clone();
    let timed = vega_runtime::RuntimeSubagentHook::call(
        hook.as_ref(),
        "snapshot".into(),
        "wait_agent".into(),
        arguments(Some(0)),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(timed["wake_reason"], "timeout");
    assert_eq!(timed["timed_out"], true);
    assert_eq!(timed["timeout_coalesced"], false);
    while fixture.empty.try_recv().is_ok() {}
    let wait = vega_runtime::RuntimeSubagentHook::call(
        hook.as_ref(),
        "terminal".into(),
        "wait_agent".into(),
        arguments(Some(0)),
        CancellationToken::new(),
    );
    tokio::pin!(wait);
    tokio::select! {
        result = &mut wait => panic!("second observation renewed deadline: {result:?}"),
        signal = fixture.empty.recv() => { signal.unwrap(); }
    }
    assert!(futures::poll!(&mut wait).is_pending());
    let mut snapshot = fixture.registry.snapshots(None).unwrap().remove(0);
    snapshot.state = SubagentState::Completed;
    snapshot.release_state = "released".into();
    snapshot.result = "finished".into();
    fixture.registry.finish(snapshot, None);
    let receipt = tokio::time::timeout(Duration::from_secs(10), wait)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(receipt["wake_reason"], "terminal");
    assert_eq!(receipt["timeout_coalesced"], true);
    assert_eq!(receipt["agents"][0]["state"], "completed");
    assert_eq!(receipt["agents"][0]["result"], "finished");
    assert_eq!(receipt["agents"][0]["release_state"], "released");
}
