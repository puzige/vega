use super::*;
use crate::types::{SubagentAdmission, SubagentHost, SubagentSnapshot, SubagentState, TaskBackend};
use futures::future::LocalBoxFuture;
use futures::stream::{FuturesUnordered, StreamExt};
use std::collections::BTreeMap;
use std::future::Future;
use tokio::sync::Notify;

fn failure(message: &str) -> VegaError {
    VegaError::Tool {
        tool: "subagent".into(),
        message: message.into(),
    }
}

struct AgentRecord {
    snapshot: SubagentSnapshot,
    call_id: String,
    cancel: CancellationToken,
}

#[derive(Clone)]
struct Registry {
    agents: Arc<Mutex<BTreeMap<String, AgentRecord>>>,
    changed: Arc<Notify>,
}

impl Registry {
    fn discard(&self, id: &str) {
        if let Ok(mut agents) = self.agents.lock() {
            agents.remove(id);
        }
        self.changed.notify_waiters();
    }
    fn snapshots(&self, ids: Option<&[String]>) -> Result<Vec<SubagentSnapshot>, VegaError> {
        let agents = self
            .agents
            .lock()
            .map_err(|_| failure("state unavailable"))?;
        match ids {
            Some(ids) => ids
                .iter()
                .map(|id| {
                    agents
                        .get(id)
                        .map(AgentRecord::snapshot)
                        .ok_or_else(|| failure("unknown agent for this run"))
                })
                .collect(),
            None => Ok(agents.values().map(AgentRecord::snapshot).collect()),
        }
    }
    fn update(&self, snapshot: SubagentSnapshot) -> Result<(), VegaError> {
        if let Some(record) = self
            .agents
            .lock()
            .map_err(|_| failure("state unavailable"))?
            .get_mut(&snapshot.agent_id)
        {
            record.snapshot = snapshot;
        }
        self.changed.notify_waiters();
        Ok(())
    }
    fn cancel_all(&self) {
        if let Ok(agents) = self.agents.lock() {
            for record in agents
                .values()
                .filter(|record| !record.snapshot.state.is_terminal())
            {
                record.cancel.cancel();
            }
        }
    }
}

impl AgentRecord {
    fn snapshot(&self) -> SubagentSnapshot {
        let mut snapshot = self.snapshot.clone();
        if self.cancel.is_cancelled() && !snapshot.state.is_terminal() {
            snapshot.state = SubagentState::Cancelling;
        }
        snapshot
    }
}

enum Command {
    Spawn {
        call_id: String,
        name: String,
        task: String,
        reply: oneshot::Sender<Result<serde_json::Value, VegaError>>,
    },
    Cancel {
        id: String,
        reply: oneshot::Sender<Result<serde_json::Value, VegaError>>,
    },
}

struct Hook {
    sender: mpsc::Sender<Command>,
    registry: Registry,
}

fn receipt(mut agents: Vec<SubagentSnapshot>, timed_out: bool) -> serde_json::Value {
    let mut truncated = false;
    while serde_json::to_vec(&agents).map_or(true, |bytes| bytes.len() > 96 * 1024)
        && !agents.is_empty()
    {
        agents.pop();
        truncated = true;
    }
    serde_json::json!({"agents":agents,"timed_out":timed_out,"truncated":truncated})
}

impl vega_runtime::RuntimeSubagentHook for Hook {
    fn call(
        &self,
        call_id: String,
        tool: String,
        input: serde_json::Value,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<serde_json::Value, VegaError>> {
        let sender = self.sender.clone();
        let registry = self.registry.clone();
        async move {
            if !vega_runtime::valid_subagent_input(&tool,&input.to_string()) { return Err(failure("invalid input")); }
            if cancel.is_cancelled() { return Err(VegaError::Cancelled); }
            match tool.as_str() {
                "list_agents" => Ok(receipt(registry.snapshots(None)?,false)),
                "wait_agent" => {
                    let ids = input["agent_ids"].as_array().ok_or_else(|| failure("invalid input"))?.iter().filter_map(|id| id.as_str().map(str::to_owned)).collect::<Vec<_>>();
                    let deadline = tokio::time::Instant::now() + Duration::from_millis(input.get("timeout_ms").and_then(serde_json::Value::as_u64).unwrap_or(30_000));
                    loop {
                        let changed = registry.changed.notified(); tokio::pin!(changed); changed.as_mut().enable();
                        let snapshots = registry.snapshots(Some(&ids))?;
                        if snapshots.iter().any(|snapshot| snapshot.state.is_terminal()) { return Ok(receipt(snapshots,false)); }
                        if tokio::time::Instant::now() >= deadline { return Ok(receipt(snapshots,true)); }
                        tokio::select! { biased; _ = cancel.cancelled() => return Err(VegaError::Cancelled), _ = &mut changed => {}, _ = tokio::time::sleep_until(deadline) => return Ok(receipt(registry.snapshots(Some(&ids))?,true)) }
                    }
                }
                _ => {
                    let (reply,receiver) = oneshot::channel();
                    let command = if tool == "spawn_agent" { Command::Spawn {call_id,name:input["name"].as_str().ok_or_else(|| failure("invalid input"))?.trim().into(),task:input["task"].as_str().ok_or_else(|| failure("invalid input"))?.into(),reply} } else { Command::Cancel {id:input["agent_id"].as_str().ok_or_else(|| failure("invalid input"))?.into(),reply} };
                    tokio::select! { biased; _ = cancel.cancelled() => return Err(VegaError::Cancelled), result = sender.send(command) => result.map_err(|_| failure("parent run ended"))? }
                    tokio::select! { biased; _ = cancel.cancelled() => Err(VegaError::Cancelled), result = receiver => result.map_err(|_| failure("parent run ended"))? }
                }
            }
        }.boxed()
    }
}

#[derive(Clone)]
pub(super) struct ChildSettings {
    pub model: String,
    pub system_prompt: String,
    pub authority: DelegatedAuthority,
    pub pricing_catalog: Option<vega_token::PricingCatalog>,
    pub reasoning: Option<FrozenReasoning>,
    pub context_budget: Option<vega_runtime::ContextBudget>,
    pub automation_claim: Option<crate::types::AutomationRunClaim>,
    pub automation_hook: Option<Arc<dyn vega_runtime::RuntimeAutomationHook>>,
    pub owner_credential_reader: Option<vega_runtime::CredentialReader>,
    pub turn_limit: u32,
}

impl ChildSettings {
    pub(super) fn new(request: &AgentRequest, config: &PersistenceActorConfig) -> Self {
        Self {
            model: request.model.clone(),
            system_prompt: request.system_prompt.clone(),
            authority: DelegatedAuthority {
                run_mode: request.tool_config.run_mode,
                permission_mode: request.tool_config.permission_mode,
                exact_rules: request.tool_config.exact_rules.clone(),
            },
            pricing_catalog: request.pricing_catalog.clone(),
            reasoning: request.reasoning.clone(),
            context_budget: request.context_budget,
            automation_claim: config.automation_claim.clone(),
            automation_hook: config.automation_hook.clone(),
            owner_credential_reader: config.owner_credential_reader.clone(),
            turn_limit: config.turn_limit,
        }
    }
}

pub(super) struct ChildLaunch<'a> {
    pub provider: &'a dyn Provider,
    pub tools: &'a vega_tools::Tools,
    pub database: PathBuf,
    pub parent_thread: String,
    pub parent_message: String,
    pub settings: ChildSettings,
    pub mcp_servers: Vec<vega_runtime::McpReadyServer>,
    pub host: Arc<dyn SubagentHost>,
    pub cancel: CancellationToken,
}

pub(super) struct Supervisor {
    registry: Registry,
    sender: mpsc::Sender<Command>,
    receiver: mpsc::Receiver<Command>,
}

impl Supervisor {
    pub(super) fn new() -> Self {
        let (sender, receiver) = mpsc::channel(8);
        Self {
            registry: Registry {
                agents: Arc::new(Mutex::new(BTreeMap::new())),
                changed: Arc::new(Notify::new()),
            },
            sender,
            receiver,
        }
    }
    pub(super) fn hook(&self) -> Arc<dyn vega_runtime::RuntimeSubagentHook> {
        Arc::new(Hook {
            sender: self.sender.clone(),
            registry: self.registry.clone(),
        })
    }
    pub(super) async fn run<'a, F, T>(&mut self, parent: F, launch: ChildLaunch<'a>) -> T
    where
        F: Future<Output = T>,
    {
        tokio::pin!(parent);
        let mut children: FuturesUnordered<LocalBoxFuture<'a, ()>> = FuturesUnordered::new();
        let result = loop {
            tokio::select! {
                biased;
                result = &mut parent => break result,
                Some(()) = children.next(), if !children.is_empty() => {},
                command = self.receiver.recv() => {
                    match command {
                        Some(Command::Spawn {call_id,name,task,reply}) => {
                            let reserved = self.reserve(&launch,call_id,name,task);
                            match reserved {
                                Ok((snapshot,task,call_id,cancel)) => children.push(child_lifecycle(&launch,snapshot,task,call_id,cancel,reply,self.registry.clone())),
                                Err(error) => { let _ = reply.send(Err(error)); }
                            }
                        }
                        Some(Command::Cancel {id,reply}) => { let result = self.cancel(&launch,&id).await; let _ = reply.send(result); }
                        None => break parent.await,
                    }
                }
            }
        };
        self.registry.cancel_all();
        while children.next().await.is_some() {}
        result
    }
    fn reserve(
        &self,
        launch: &ChildLaunch<'_>,
        call_id: String,
        name: String,
        task: String,
    ) -> Result<(SubagentSnapshot, String, String, CancellationToken), VegaError> {
        if launch.cancel.is_cancelled() {
            return Err(VegaError::Cancelled);
        }
        let mut agents = self
            .registry
            .agents
            .lock()
            .map_err(|_| failure("state unavailable"))?;
        if agents.values().any(|record| record.call_id == call_id) {
            return Err(failure("spawn call already admitted"));
        }
        if agents.values().any(|record| record.snapshot.name == name) {
            return Err(failure("name already used in this run"));
        }
        if agents
            .values()
            .filter(|record| !record.snapshot.state.is_terminal())
            .count()
            >= 4
        {
            return Err(failure(
                "four agents are already active; wait for one to finish",
            ));
        }
        let snapshot = SubagentSnapshot {
            agent_id: ulid::Ulid::generate().to_string(),
            parent_thread_id: launch.parent_thread.clone(),
            parent_message_id: launch.parent_message.clone(),
            message_id: ulid::Ulid::generate().to_string(),
            name,
            state: SubagentState::Running,
            result: String::new(),
            truncated: false,
        };
        let cancel = launch.cancel.child_token();
        agents.insert(
            snapshot.agent_id.clone(),
            AgentRecord {
                snapshot: snapshot.clone(),
                call_id: call_id.clone(),
                cancel: cancel.clone(),
            },
        );
        Ok((snapshot, task, call_id, cancel))
    }
    async fn cancel(
        &self,
        launch: &ChildLaunch<'_>,
        id: &str,
    ) -> Result<serde_json::Value, VegaError> {
        let (mut snapshot, cancel) = {
            let agents = self
                .registry
                .agents
                .lock()
                .map_err(|_| failure("state unavailable"))?;
            let record = agents
                .get(id)
                .ok_or_else(|| failure("unknown agent for this run"))?;
            (record.snapshot.clone(), record.cancel.clone())
        };
        if !snapshot.state.is_terminal() {
            cancel.cancel();
            let database = launch.database.clone();
            let child = id.to_string();
            let run = snapshot.message_id.clone();
            tokio::task::spawn_blocking(move || {
                let store = Store::open(database).map_err(|_| failure("storage unavailable"))?;
                vega_store::subagents::request_cancel(store.conn(), &child, &run)
                    .map_err(|_| failure("storage unavailable"))
            })
            .await
            .map_err(|_| failure("storage unavailable"))??;
            snapshot = self.registry.snapshots(Some(&[id.to_string()]))?.remove(0);
            if !snapshot.state.is_terminal() {
                snapshot.state = SubagentState::Cancelling;
                self.registry.update(snapshot.clone())?;
                launch.host.updated(snapshot.clone())?;
            }
        }
        Ok(serde_json::json!({"agent":snapshot}))
    }
}

struct ChildProvider<'a> {
    inner: &'a dyn Provider,
    reader: Option<vega_runtime::CredentialReader>,
}
impl Provider for ChildProvider<'_> {
    fn chat_stream(
        &self,
        request: vega_runtime::ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<vega_runtime::EventStream, VegaError>> {
        if let Some(reader) = &self.reader
            && let Err(error) =
                super::credential_guard::reject_owner_secret(&request, reader.as_ref())
        {
            return async move { Err(error) }.boxed();
        }
        self.inner.chat_stream(request, cancel)
    }
    fn chat_stream_recovering(
        &self,
        request: vega_runtime::ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<vega_runtime::ProviderStream, VegaError>> {
        if let Some(reader) = &self.reader
            && let Err(error) =
                super::credential_guard::reject_owner_secret(&request, reader.as_ref())
        {
            return async move { Err(error) }.boxed();
        }
        self.inner.chat_stream_recovering(request, cancel)
    }
    fn chat_stream_once(
        &self,
        request: vega_runtime::ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<vega_runtime::EventStream, VegaError>> {
        if let Some(reader) = &self.reader
            && let Err(error) =
                super::credential_guard::reject_owner_secret(&request, reader.as_ref())
        {
            return async move { Err(error) }.boxed();
        }
        self.inner.chat_stream_once(request, cancel)
    }
}

struct ChildUsage {
    pending: std::sync::atomic::AtomicUsize,
    complete: std::sync::atomic::AtomicBool,
    parent: Option<Arc<dyn vega_runtime::RuntimeAutomationHook>>,
}

impl ChildUsage {
    fn new(parent: Option<Arc<dyn vega_runtime::RuntimeAutomationHook>>) -> Arc<Self> {
        Arc::new(Self {
            pending: std::sync::atomic::AtomicUsize::new(0),
            complete: std::sync::atomic::AtomicBool::new(true),
            parent,
        })
    }
    fn complete(&self) -> bool {
        self.complete.load(std::sync::atomic::Ordering::SeqCst)
            && self.pending.load(std::sync::atomic::Ordering::SeqCst) == 0
    }
}

impl vega_runtime::RuntimeAutomationHook for ChildUsage {
    fn context(&self) -> serde_json::Value {
        serde_json::json!({})
    }
    fn call(
        &self,
        _: String,
        _: serde_json::Value,
        _: CancellationToken,
    ) -> BoxFuture<'static, Result<serde_json::Value, VegaError>> {
        async { Err(failure("child cannot control parent goal")) }.boxed()
    }
    fn provider_started(&self) {
        self.pending
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if let Some(parent) = &self.parent {
            parent.provider_started();
        }
    }
    fn provider_usage(&self) {
        let _ = self.pending.fetch_update(
            std::sync::atomic::Ordering::SeqCst,
            std::sync::atomic::Ordering::SeqCst,
            |pending| pending.checked_sub(1),
        );
        if let Some(parent) = &self.parent {
            parent.provider_usage();
        }
    }
    fn missing_usage(&self) {
        self.complete
            .store(false, std::sync::atomic::Ordering::SeqCst);
        if let Some(parent) = &self.parent {
            parent.missing_usage();
        }
    }
}

fn bounded_result(text: &str) -> (String, bool) {
    let mut end = text.len().min(vega_runtime::SUBAGENT_RESULT_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_string(), end < text.len())
}

#[allow(clippy::too_many_arguments)]
fn child_lifecycle<'a>(
    launch: &ChildLaunch<'a>,
    mut snapshot: SubagentSnapshot,
    task: String,
    call_id: String,
    cancel: CancellationToken,
    reply: oneshot::Sender<Result<serde_json::Value, VegaError>>,
    registry: Registry,
) -> LocalBoxFuture<'a, ()> {
    let provider = launch.provider;
    let tools = launch.tools.isolated();
    let database = launch.database.clone();
    let host = launch.host.clone();
    let settings = launch.settings.clone();
    let mcp = launch.mcp_servers.clone();
    async move {
        let _live = match crate::subagents::LiveDelegation::new(&database, &snapshot.agent_id) {
            Ok(live) => live,
            Err(_) => {
                registry.discard(&snapshot.agent_id);
                let _ = reply.send(Err(failure("admission unavailable")));
                return;
            }
        };
        if cancel.is_cancelled() {
            registry.discard(&snapshot.agent_id);
            let _ = reply.send(Err(VegaError::Cancelled));
            return;
        }
        if let Some(reader) = &settings.owner_credential_reader {
            let safe = reader().ok().is_some_and(|known| {
                vega_runtime::redact_sensitive_credential_text_with_policy(&task, &known, None)
                    == task
                    && vega_runtime::redact_sensitive_credential_text_with_policy(
                        &snapshot.name,
                        &known,
                        None,
                    ) == snapshot.name
            });
            if !safe {
                registry.discard(&snapshot.agent_id);
                let _ = reply.send(Err(failure(
                    "delegated input failed credential safety checks",
                )));
                return;
            }
        }
        let created = create_child(
            database.clone(),
            snapshot.clone(),
            task.clone(),
            call_id,
            settings.clone(),
            cancel.clone(),
        )
        .await;
        let (thread, user_id) = match created {
            Ok(created) => created,
            Err(error) => {
                registry.discard(&snapshot.agent_id);
                let _ = reply.send(Err(error));
                return;
            }
        };
        let permissions = PermissionQueue::new();
        let questions = UserQuestionQueue::new();
        let queue_generation = ulid::Ulid::generate().to_string();
        let admission = host.admit(SubagentAdmission {
            thread,
            snapshot: snapshot.clone(),
            cancel: cancel.clone(),
            permission_queue: permissions.clone(),
            user_questions: questions.clone(),
            task: task.clone(),
            queue_generation: queue_generation.clone(),
        });
        let admitted = tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(VegaError::Cancelled),
            result = admission => result,
        };
        let admitted = admitted.and_then(|()| {
            if cancel.is_cancelled() {
                Err(VegaError::Cancelled)
            } else {
                host.updated(snapshot.clone())
            }
        });
        let usage_observer = ChildUsage::new(settings.automation_hook.clone());
        let result = match admitted {
            Err(error) => {
                if matches!(&error, VegaError::Cancelled) {
                    cancel.cancel();
                }
                let _ = reply.send(Err(error));
                Err(ConversationError::Runtime(Arc::new(failure(
                    "child admission failed",
                ))))
            }
            Ok(()) => {
                if reply
                    .send(Ok(serde_json::json!({"agent":snapshot})))
                    .is_err()
                {
                    cancel.cancel();
                }
                let mut config = PersistenceActorConfig {
                    delegated: true,
                    delegated_parent: Some((
                        snapshot.parent_thread_id.clone(),
                        snapshot.parent_message_id.clone(),
                    )),
                    delegated_message_id: Some(snapshot.message_id.clone()),
                    delegated_authority: Some(settings.authority.clone()),
                    delegated_context_budget: settings.context_budget,
                    automation_claim: settings.automation_claim.clone(),
                    automation_hook: Some(usage_observer.clone()),
                    owner_credential_reader: settings.owner_credential_reader.clone(),
                    turn_limit: settings.turn_limit,
                    user_questions: Some(Arc::new(questions.clone())),
                    ..Default::default()
                };
                if let Some(reader) = &config.owner_credential_reader {
                    config.steering_source =
                        Some(Arc::new(crate::pending_messages::QueueSteeringSource::new(
                            database.clone(),
                            tools.root().to_path_buf(),
                            snapshot.agent_id.clone(),
                            queue_generation,
                            reader.clone(),
                            cancel.clone(),
                        )));
                }
                let scoped_provider = ChildProvider {
                    inner: provider,
                    reader: config.owner_credential_reader.clone(),
                };
                match Store::open(&database) {
                    Ok(store) => {
                        let child_host = host.clone();
                        let id = snapshot.agent_id.clone();
                        let run = snapshot.message_id.clone();
                        super::entry::run_thread_task_with_automation_inner(
                            &store,
                            &scoped_provider,
                            &tools,
                            &snapshot.agent_id,
                            &task,
                            &settings.system_prompt,
                            cancel.clone(),
                            &permissions,
                            move |event| child_host.event(&id, &run, event),
                            config,
                            Some(user_id),
                            settings.pricing_catalog,
                            settings.reasoning,
                            Vec::new(),
                            mcp,
                        )
                        .await
                    }
                    Err(_) => Err(ConversationError::Store("child storage unavailable".into())),
                }
            }
        };
        questions.cancel_active();
        permissions.timeout_active();
        snapshot.state = match &result {
            Ok(run) if run.interrupted => SubagentState::Interrupted,
            Ok(run) if run.failed => SubagentState::Failed,
            Ok(run)
                if !run.content.trim().is_empty()
                    && run.events.iter().rev().find_map(|event| match event {
                        ConversationEvent::MessageFinished { stop_reason, .. } => {
                            Some(*stop_reason)
                        }
                        _ => None,
                    }) == Some(crate::types::ConversationStopReason::End) =>
            {
                SubagentState::Completed
            }
            Ok(_) => SubagentState::Failed,
            Err(_) if cancel.is_cancelled() => SubagentState::Interrupted,
            Err(_) => SubagentState::Failed,
        };
        let text = match &result {
            Ok(run) if snapshot.state == SubagentState::Completed => run.content.as_str(),
            Ok(run) if snapshot.state == SubagentState::Failed && !run.content.is_empty() => {
                &run.content
            }
            _ if snapshot.state == SubagentState::Interrupted => "Interrupted before completion",
            _ => "Subagent failed. Open the conversation for details.",
        };
        (snapshot.result, snapshot.truncated) = bounded_result(text);
        let complete = usage_observer.complete();
        let persisted = persist_child_terminal(database, snapshot.clone(), complete)
            .await
            .is_ok();
        if !persisted {
            snapshot.state = SubagentState::Failed;
            snapshot.result = "Subagent result could not be saved".into();
            snapshot.truncated = false;
        }
        let _ = registry.update(snapshot.clone());
        let _ = host.finished(snapshot, complete && persisted);
    }
    .boxed_local()
}

async fn create_child(
    database: PathBuf,
    snapshot: SubagentSnapshot,
    task: String,
    call_id: String,
    settings: ChildSettings,
    cancel: CancellationToken,
) -> Result<(crate::types::Thread, String), VegaError> {
    tokio::task::spawn_blocking(move || {
        if cancel.is_cancelled() {
            return Err(VegaError::Cancelled);
        }
        let store = Store::open(database).map_err(|_| failure("storage unavailable"))?;
        let tx = store
            .immediate_transaction()
            .map_err(|_| failure("storage unavailable"))?;
        if let Some(claim) = &settings.automation_claim {
            crate::automation::check_subagent_budget(&tx, claim)
                .map_err(|message| failure(&message))?;
        }
        let parent = vega_store::threads::find(&tx, &snapshot.parent_thread_id)
            .map_err(|_| failure("parent unavailable"))?
            .ok_or_else(|| failure("parent unavailable"))?;
        let parent =
            crate::threads::thread_from_row(&parent).map_err(|_| failure("parent unavailable"))?;
        if parent.backend != TaskBackend::Native || parent.parent_thread_id.is_some() {
            return Err(failure("nested delegation unavailable"));
        }
        let permission_mode = match settings.authority.permission_mode {
            RuntimePermissionMode::ReadOnly => "readonly",
            RuntimePermissionMode::Confirm => "confirm",
            RuntimePermissionMode::Auto => "auto",
            RuntimePermissionMode::FullAccess => "full_access",
        };
        let mut draft = crate::threads::draft_thread(
            parent.project_binding(),
            &settings.model,
            permission_mode,
        );
        draft.id = snapshot.agent_id.clone();
        let child = crate::threads::materialize_draft(&store, &draft)
            .map_err(|_| failure("child creation failed"))?;
        let changed = vega_store::threads::rename(&tx, &child.id, &snapshot.name, now_ms())
            .map_err(|_| failure("child creation failed"))?;
        if changed != 1 {
            return Err(failure("child creation failed"));
        }
        vega_store::subagents::insert(
            &tx,
            &vega_store::subagents::DelegationRow {
                child_thread_id: child.id.clone(),
                parent_thread_id: parent.id,
                parent_message_id: snapshot.parent_message_id,
                spawn_call_id: call_id,
                child_message_id: snapshot.message_id,
                name: snapshot.name,
                task: task.clone(),
                state: "running".into(),
                result: String::new(),
                truncated: false,
                created_at: now_ms(),
            },
        )
        .map_err(|_| failure("child creation failed"))?;
        let user_id = ulid::Ulid::generate().to_string();
        vega_store::messages::insert(
            &tx,
            &vega_store::messages::MessageRow {
                id: user_id.clone(),
                thread_id: child.id.clone(),
                seq: 1,
                role: "user".into(),
                kind: "text".into(),
                content: task,
                status: "done".into(),
                created_at: now_ms(),
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .map_err(|_| failure("child creation failed"))?;
        let row = vega_store::threads::find(&tx, &child.id)
            .map_err(|_| failure("child creation failed"))?
            .ok_or_else(|| failure("child creation failed"))?;
        let child =
            crate::threads::thread_from_row(&row).map_err(|_| failure("child creation failed"))?;
        if cancel.is_cancelled() {
            return Err(VegaError::Cancelled);
        }
        tx.commit().map_err(|_| failure("child creation failed"))?;
        Ok((child, user_id))
    })
    .await
    .map_err(|_| failure("child creation failed"))?
}

async fn persist_child_terminal(
    database: PathBuf,
    snapshot: SubagentSnapshot,
    usage_complete: bool,
) -> Result<(), VegaError> {
    tokio::task::spawn_blocking(move || {
        let store = Store::open(database).map_err(|_| failure("storage unavailable"))?;
        let tx = store
            .immediate_transaction()
            .map_err(|_| failure("storage unavailable"))?;
        if !vega_store::subagents::finish(
            &tx,
            &snapshot.agent_id,
            &snapshot.message_id,
            snapshot.state.as_str(),
            &snapshot.result,
            snapshot.truncated,
        )
        .map_err(|_| failure("storage unavailable"))?
        {
            return Err(failure("delegated generation changed"));
        }
        vega_store::subagents::mark_usage_complete(
            &tx,
            &snapshot.agent_id,
            &snapshot.message_id,
            usage_complete,
        )
        .map_err(|_| failure("storage unavailable"))?;
        if snapshot.state != SubagentState::Completed {
            vega_store::recovery::recover_thread_in_transaction(&tx, &snapshot.agent_id, now_ms())
                .map_err(|_| failure("storage unavailable"))?;
        }
        tx.commit().map_err(|_| failure("storage unavailable"))?;
        Ok(())
    })
    .await
    .map_err(|_| failure("storage unavailable"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use vega_runtime::RuntimeSubagentHook;

    struct Host;
    impl SubagentHost for Host {
        fn admit(&self, _: SubagentAdmission) -> BoxFuture<'static, Result<(), VegaError>> {
            async { Ok(()) }.boxed()
        }
        fn event(&self, _: &str, _: &str, _: &ConversationEvent) -> Result<(), VegaError> {
            Ok(())
        }
        fn updated(&self, _: SubagentSnapshot) -> Result<(), VegaError> {
            Ok(())
        }
        fn finished(&self, _: SubagentSnapshot, _: bool) -> Result<(), VegaError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn m06_m15_failed_and_interrupted_terminals_commit_with_recovery_or_rollback_together() {
        for state in [SubagentState::Failed, SubagentState::Interrupted] {
            for reject_recovery in [false, true] {
                let temp = tempfile::tempdir().unwrap();
                let path = temp.path().join("terminal.db");
                let store = Store::open(&path).unwrap();
                store.migrate().unwrap();
                store.conn().execute_batch("INSERT INTO threads(id,model,created_at,updated_at) VALUES('parent','mock',1,1),('child','mock',1,1); INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES('parent-run','parent',1,'assistant','','done',1),('child-run','child',1,'assistant','partial','streaming',1); INSERT INTO tool_calls(id,thread_id,message_id,seq,tool,input_json,status,approval,created_at) VALUES('child-tool','child','child-run',1,'bash','{}','running','{\"decision\":\"once\",\"note\":null,\"source\":\"auto\",\"danger\":null}',1);").unwrap();
                vega_store::subagents::insert(
                    store.conn(),
                    &vega_store::subagents::DelegationRow {
                        child_thread_id: "child".into(),
                        parent_thread_id: "parent".into(),
                        parent_message_id: "parent-run".into(),
                        spawn_call_id: "spawn".into(),
                        child_message_id: "child-run".into(),
                        name: "Child".into(),
                        task: "task".into(),
                        state: "running".into(),
                        result: String::new(),
                        truncated: false,
                        created_at: 1,
                    },
                )
                .unwrap();
                if reject_recovery {
                    store.conn().execute_batch("CREATE TRIGGER reject_child_recovery BEFORE UPDATE ON tool_calls BEGIN SELECT RAISE(ABORT,'synthetic recovery failure'); END;").unwrap();
                }
                let snapshot = SubagentSnapshot {
                    agent_id: "child".into(),
                    parent_thread_id: "parent".into(),
                    parent_message_id: "parent-run".into(),
                    message_id: "child-run".into(),
                    name: "Child".into(),
                    state,
                    result: format!("{} result preserved", state.as_str()),
                    truncated: false,
                };
                let result = persist_child_terminal(path.clone(), snapshot.clone(), true).await;
                assert_eq!(result.is_err(), reject_recovery);
                drop(store);
                let reopened = Store::open(path).unwrap();
                let saved = vega_store::subagents::find(reopened.conn(), "child")
                    .unwrap()
                    .unwrap();
                assert_eq!(
                    saved.state,
                    if reject_recovery {
                        "running"
                    } else {
                        state.as_str()
                    }
                );
                assert_eq!(
                    saved.result,
                    if reject_recovery {
                        ""
                    } else {
                        snapshot.result.as_str()
                    }
                );
                assert_eq!(
                    vega_store::messages::find(reopened.conn(), "child-run")
                        .unwrap()
                        .unwrap()
                        .status,
                    if reject_recovery {
                        "streaming"
                    } else {
                        "interrupted"
                    }
                );
                assert_eq!(
                    vega_store::tool_calls::find_state(reopened.conn(), "child-tool")
                        .unwrap()
                        .unwrap()
                        .status,
                    if reject_recovery {
                        "running"
                    } else {
                        "cancelled"
                    }
                );
                let complete: bool = reopened
                    .conn()
                    .query_row(
                        "SELECT usage_complete FROM delegated_runs WHERE child_thread_id='child'",
                        [],
                        |row| row.get(0),
                    )
                    .unwrap();
                assert_eq!(complete, !reject_recovery);
            }
        }
    }

    fn launch<'a>(provider: &'a dyn Provider, tools: &'a vega_tools::Tools) -> ChildLaunch<'a> {
        ChildLaunch {
            provider,
            tools,
            database: PathBuf::new(),
            parent_thread: "parent".into(),
            parent_message: "run".into(),
            settings: ChildSettings {
                model: "mock".into(),
                system_prompt: "System".into(),
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
                turn_limit: 0,
            },
            mcp_servers: Vec::new(),
            host: Arc::new(Host),
            cancel: CancellationToken::new(),
        }
    }

    #[tokio::test]
    async fn m03_m04_m05_capacity_names_scope_wait_timeout_and_wakeup() {
        let temp = tempfile::tempdir().unwrap();
        let tools = vega_tools::Tools::new(temp.path()).unwrap();
        let provider = vega_runtime::MockProvider::new(Vec::new());
        let launch = launch(&provider, &tools);
        let supervisor = Supervisor::new();
        let mut ids = Vec::new();
        for index in 0..4 {
            ids.push(
                supervisor
                    .reserve(
                        &launch,
                        format!("call-{index}"),
                        format!("Agent {index}"),
                        "task".into(),
                    )
                    .unwrap()
                    .0
                    .agent_id,
            );
        }
        assert!(
            supervisor
                .reserve(&launch, "fifth".into(), "Fifth".into(), "task".into())
                .is_err()
        );
        let hook = Hook {
            sender: supervisor.sender.clone(),
            registry: supervisor.registry.clone(),
        };
        assert!(
            hook.call(
                "x".into(),
                "wait_agent".into(),
                json!({"agent_ids":["foreign"]}),
                CancellationToken::new()
            )
            .await
            .is_err()
        );
        let timeout = hook
            .call(
                "timeout".into(),
                "wait_agent".into(),
                json!({"agent_ids":[ids[0]],"timeout_ms":0}),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(timeout["timed_out"], true);
        let wait = hook.call(
            "wait".into(),
            "wait_agent".into(),
            json!({"agent_ids":[ids[0]],"timeout_ms":60000}),
            CancellationToken::new(),
        );
        let complete = async {
            tokio::task::yield_now().await;
            let mut snapshot = supervisor
                .registry
                .snapshots(Some(&ids[..1]))
                .unwrap()
                .remove(0);
            snapshot.state = SubagentState::Completed;
            snapshot.result = "done".into();
            supervisor.registry.update(snapshot).unwrap();
        };
        let (result, ()) = tokio::join!(wait, complete);
        assert_eq!(result.unwrap()["timed_out"], false);
        assert!(
            supervisor
                .reserve(&launch, "duplicate".into(), "Agent 0".into(), "task".into())
                .is_err()
        );
        assert!(
            supervisor
                .reserve(
                    &launch,
                    "replacement".into(),
                    "Replacement".into(),
                    "task".into()
                )
                .is_ok()
        );
        let cancelled = CancellationToken::new();
        cancelled.cancel();
        assert!(matches!(
            hook.call(
                "cancel".into(),
                "wait_agent".into(),
                json!({"agent_ids":[ids[1]]}),
                cancelled
            )
            .await,
            Err(VegaError::Cancelled)
        ));
    }
}
