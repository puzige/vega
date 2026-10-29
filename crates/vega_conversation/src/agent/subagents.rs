use super::*;
use crate::types::{
    SubagentAdmission, SubagentAdmissionOutcome, SubagentHost, SubagentSnapshot, SubagentState,
    TaskBackend,
};
use futures::future::LocalBoxFuture;
use futures::stream::{FuturesUnordered, StreamExt};
use std::collections::BTreeMap;
use tokio::sync::Notify;
#[path = "subagents/lifecycle_ownership.rs"]
mod lifecycle_ownership;
#[path = "subagents/stage_recovery.rs"]
mod stage_recovery;
use lifecycle_ownership::LifecycleOwnership;
use vega_store::delegated_ownership::DelegatedOwnership;

fn failure(message: &str) -> VegaError {
    VegaError::Tool {
        tool: "subagent".into(),
        message: message.into(),
    }
}

type ExecutionKey = (String, String);
type AdmissionReply = oneshot::Sender<Result<serde_json::Value, VegaError>>;

#[derive(Clone)]
struct ContinuationBoundary {
    message_id: String,
    source_version: u64,
    fingerprint: String,
    thread: vega_store::threads::ThreadRow,
}

struct AgentRecord {
    snapshot: SubagentSnapshot,
    call_id: String,
    cancel: CancellationToken,
    receipt_persisted: CancellationToken,
    writer: Option<(
        Arc<DelegatedOwnership>,
        vega_store::thread_admission::ThreadAdmission,
    )>,
    occupied: bool,
    admitted: bool,
    committed: bool,
    boundary: Option<ContinuationBoundary>,
    activity: super::stage_activity::StageActivity,
    delivered_attention: u64,
    bounded_observed: bool,
    terminal_delivered: bool,
}

struct AdmissionCall {
    tool: String,
    input: serde_json::Value,
    waiters: Vec<AdmissionReply>,
    outcome: Option<Result<serde_json::Value, String>>,
}

#[derive(Default)]
struct RegistryState {
    executions: BTreeMap<ExecutionKey, AgentRecord>,
    latest: BTreeMap<String, String>,
    calls: BTreeMap<String, AdmissionCall>,
    ended: bool,
    admission_failure: Option<String>,
    steering: Option<Arc<dyn vega_runtime::RuntimeSteeringSource>>,
    delivered_steering: Option<String>,
}

#[derive(Clone)]
pub(super) struct ReceiptObserver(Registry);

impl ReceiptObserver {
    pub(super) fn persisted(&self, event: &RuntimeEvent) {
        if let RuntimeEvent::ToolCallFinished(result) = event
            && result.status == RuntimeToolStatus::Success
            && !result.reused
            && let Ok(value) = serde_json::from_str::<serde_json::Value>(&result.output)
            && let Some(agent) = value.get("agent")
            && let (Some(id), Some(message)) =
                (agent["agent_id"].as_str(), agent["message_id"].as_str())
            && let Ok(state) = self.0.state.lock()
            && let Some(record) = state.executions.get(&(id.into(), message.into()))
            && record.call_id == result.call_id
            && record.admitted
            && !record.cancel.is_cancelled()
            && !record.snapshot.state.is_terminal()
        {
            record.receipt_persisted.cancel();
        }
    }
}

#[derive(Clone)]
struct Registry {
    state: Arc<Mutex<RegistryState>>,
    changed: Arc<Notify>,
}

impl Registry {
    fn active(&self) -> Result<(), VegaError> {
        let state = self
            .state
            .lock()
            .map_err(|_| failure("state unavailable"))?;
        if state.ended {
            Err(failure("parent_run_ended"))
        } else {
            Ok(())
        }
    }

    fn waitable(&self) -> Result<(), VegaError> {
        self.active()?;
        let state = self
            .state
            .lock()
            .map_err(|_| failure("state unavailable"))?;
        match &state.admission_failure {
            Some(error) => Err(failure(error)),
            None => Ok(()),
        }
    }

    fn attention(&self, targets: &[ExecutionKey]) -> Result<bool, VegaError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| failure("state unavailable"))?;
        if let Some(error) = &state.admission_failure {
            return Err(failure(error));
        }
        let mut changed = false;
        for key in targets {
            if let Some(record) = state.executions.get_mut(key)
                && let Some(generation) = record.activity.attention()
                && generation != record.delivered_attention
            {
                record.delivered_attention = generation;
                changed = true;
            }
        }
        Ok(changed)
    }

    fn quarantine(&self, mut snapshot: SubagentSnapshot, message: &str) {
        snapshot.state = SubagentState::Failed;
        snapshot.result = message.into();
        snapshot.truncated = false;
        if let Ok(mut state) = self.state.lock() {
            state.admission_failure = Some(message.into());
            if let Some(record) = state
                .executions
                .get_mut(&(snapshot.agent_id.clone(), snapshot.message_id.clone()))
            {
                record.snapshot = snapshot;
                record.boundary = None;
            }
        }
        self.changed.notify_waiters();
    }

    fn begin_call(
        &self,
        call_id: &str,
        tool: &str,
        input: &serde_json::Value,
        reply: AdmissionReply,
    ) -> Result<bool, VegaError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| failure("state unavailable"))?;
        if let Some(call) = state.calls.get_mut(call_id) {
            if call.tool != tool || call.input != *input {
                let _ = reply.send(Err(failure("call_id_conflict")));
            } else if let Some(outcome) = &call.outcome {
                let _ = reply.send(outcome.clone().map_err(|message| failure(&message)));
            } else {
                call.waiters.push(reply);
            }
            return Ok(false);
        }
        state.calls.insert(
            call_id.into(),
            AdmissionCall {
                tool: tool.into(),
                input: input.clone(),
                waiters: vec![reply],
                outcome: None,
            },
        );
        Ok(true)
    }

    fn resolve_call(&self, call_id: &str, outcome: Result<serde_json::Value, VegaError>) -> bool {
        let outcome = outcome.map_err(|error| error.to_string());
        let mut delivered = false;
        if let Ok(mut state) = self.state.lock()
            && let Some(call) = state.calls.get_mut(call_id)
        {
            if call.outcome.is_some() {
                return false;
            }
            call.outcome = Some(outcome.clone());
            for waiter in std::mem::take(&mut call.waiters) {
                delivered |= waiter
                    .send(outcome.clone().map_err(|message| failure(&message)))
                    .is_ok();
            }
        }
        delivered
    }

    fn discard(&self, snapshot: &SubagentSnapshot, previous: Option<&ContinuationBoundary>) {
        if let Ok(mut state) = self.state.lock() {
            state
                .executions
                .remove(&(snapshot.agent_id.clone(), snapshot.message_id.clone()));
            if state.latest.get(&snapshot.agent_id) == Some(&snapshot.message_id) {
                if let Some(previous) = previous {
                    state
                        .latest
                        .insert(snapshot.agent_id.clone(), previous.message_id.clone());
                } else {
                    state.latest.remove(&snapshot.agent_id);
                }
            }
        }
        self.changed.notify_waiters();
    }

    fn target(&self, id: &str, message: Option<&str>) -> Result<ExecutionKey, VegaError> {
        let state = self
            .state
            .lock()
            .map_err(|_| failure("state unavailable"))?;
        let latest = state
            .latest
            .get(id)
            .ok_or_else(|| failure("unknown_agent"))?;
        let message = if let Some(message) = message {
            message
        } else {
            if state
                .executions
                .keys()
                .filter(|(agent, _)| agent == id)
                .count()
                != 1
            {
                return Err(failure("execution_id_required"));
            }
            latest
        };
        let key = (id.into(), message.into());
        if !state.executions.contains_key(&key) {
            return Err(failure("unknown_agent"));
        }
        Ok(key)
    }

    fn snapshots(
        &self,
        targets: Option<&[ExecutionKey]>,
    ) -> Result<Vec<SubagentSnapshot>, VegaError> {
        let state = self
            .state
            .lock()
            .map_err(|_| failure("state unavailable"))?;
        match targets {
            Some(targets) => targets
                .iter()
                .map(|key| {
                    state
                        .executions
                        .get(key)
                        .map(AgentRecord::snapshot)
                        .ok_or_else(|| failure("unknown_agent"))
                })
                .collect(),
            None => Ok(state
                .latest
                .iter()
                .filter_map(|(id, message)| state.executions.get(&(id.clone(), message.clone())))
                .filter(|record| record.committed)
                .map(AgentRecord::snapshot)
                .collect()),
        }
    }

    fn claim_observation(&self, targets: &[ExecutionKey]) -> Result<bool, VegaError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| failure("state unavailable"))?;
        for key in targets {
            if !state.executions.contains_key(key) {
                return Err(failure("unknown_agent"));
            }
        }
        let fresh = targets.iter().any(|key| {
            let record = &state.executions[key];
            record.occupied && !record.bounded_observed
        });
        for key in targets {
            if let Some(record) = state.executions.get_mut(key)
                && record.occupied
            {
                record.bounded_observed = true;
            }
        }
        Ok(fresh)
    }

    fn released_terminal(&self, targets: &[ExecutionKey]) -> Result<bool, VegaError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| failure("state unavailable"))?;
        let all = targets.iter().all(|key| {
            state
                .executions
                .get(key)
                .is_some_and(|record| record.snapshot.state.is_terminal() && !record.occupied)
        });
        let mut fresh = false;
        for key in targets {
            if let Some(record) = state.executions.get_mut(key)
                && record.snapshot.state.is_terminal()
                && !record.occupied
                && !record.terminal_delivered
            {
                record.terminal_delivered = true;
                fresh = true;
            }
        }
        Ok(all || fresh)
    }

    fn steering_ready(&self, id: String) -> Result<bool, VegaError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| failure("state unavailable"))?;
        if state.delivered_steering.as_ref() == Some(&id) {
            return Ok(false);
        }
        state.delivered_steering = Some(id);
        Ok(true)
    }

    fn update(&self, snapshot: SubagentSnapshot) -> Result<(), VegaError> {
        if let Some(record) = self
            .state
            .lock()
            .map_err(|_| failure("state unavailable"))?
            .executions
            .get_mut(&(snapshot.agent_id.clone(), snapshot.message_id.clone()))
        {
            record.snapshot = snapshot;
        }
        self.changed.notify_waiters();
        Ok(())
    }

    fn finish(&self, snapshot: SubagentSnapshot, boundary: Option<ContinuationBoundary>) {
        if let Ok(mut state) = self.state.lock()
            && let Some(record) = state
                .executions
                .get_mut(&(snapshot.agent_id.clone(), snapshot.message_id.clone()))
        {
            record.snapshot = snapshot;
            record.writer = None;
            record.occupied = false;
            record.boundary = boundary;
        }
        self.changed.notify_waiters();
    }

    fn cancel_all(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.ended = true;
            for record in state.executions.values().filter(|record| record.occupied) {
                record.cancel.cancel();
            }
        }
        self.changed.notify_waiters();
    }
}

impl AgentRecord {
    fn snapshot(&self) -> SubagentSnapshot {
        let mut snapshot = self.snapshot.clone();
        if self.occupied {
            snapshot.ready_for_review = false;
            if !(snapshot.state.is_terminal() && snapshot.release_state == "unknown") {
                if self.cancel.is_cancelled() {
                    snapshot.state = SubagentState::Cancelling;
                } else if snapshot.state.is_terminal() {
                    snapshot.state = SubagentState::Running;
                }
            }
        }
        snapshot
    }
}

enum Command {
    Admit {
        call_id: String,
        tool: String,
        input: serde_json::Value,
        reply: AdmissionReply,
    },
    Cancel {
        target: ExecutionKey,
        reply: AdmissionReply,
    },
}

struct Hook {
    sender: mpsc::Sender<Command>,
    registry: Registry,
}

fn receipt(agents: Vec<SubagentSnapshot>, timed_out: bool) -> Result<serde_json::Value, VegaError> {
    if serde_json::to_vec(&agents).map_or(true, |bytes| bytes.len() > 96 * 1024) {
        return Err(failure(
            "subagent receipt too large; narrow the requested agents",
        ));
    }
    let truncated = false;
    let mut value =
        serde_json::json!({"agents":agents,"timed_out":timed_out,"truncated":truncated});
    if let Some(agents) = value["agents"].as_array_mut() {
        for agent in agents {
            agent["ready"] = false.into();
        }
    }
    Ok(value)
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
            registry.active()?;
            if !vega_runtime::valid_subagent_input(&tool, &input.to_string()) { return Err(failure("invalid input")); }
            if cancel.is_cancelled() { return Err(VegaError::Cancelled); }
            match tool.as_str() {
                "list_agents" => receipt(registry.snapshots(None)?, false),
                "wait_agent" => {
                    let targets = if let Some(targets) = input["targets"].as_array() {
                        targets.iter().map(|target| registry.target(target["agent_id"].as_str().ok_or_else(|| failure("invalid input"))?, target["message_id"].as_str())).collect::<Result<Vec<_>, _>>()?
                    } else {
                        input["agent_ids"].as_array().ok_or_else(|| failure("invalid input"))?.iter().map(|id| registry.target(id.as_str().ok_or_else(|| failure("invalid input"))?, None)).collect::<Result<Vec<_>, _>>()?
                    };
                    let explicit = input.get("timeout_ms").and_then(serde_json::Value::as_u64);
                    let fresh = if explicit.is_some() { registry.claim_observation(&targets)? } else { false };
                    let coalesced = explicit.is_some() && !fresh;
                    let deadline = explicit.filter(|_| fresh).map(|millis| tokio::time::Instant::now() + Duration::from_millis(millis));
                    let answer = |reason: &str| -> Result<serde_json::Value, VegaError> {
                        let mut value = receipt(registry.snapshots(Some(&targets))?, reason == "timeout")?;
                        value["timeout_coalesced"] = coalesced.into();
                        value["wake_reason"] = reason.into();
                        Ok(value)
                    };
                    loop {
                        let changed = registry.changed.notified(); tokio::pin!(changed); changed.as_mut().enable();
                        registry.waitable()?;
                        if cancel.is_cancelled() { return Err(VegaError::Cancelled); }
                        if registry.released_terminal(&targets)? { return answer("terminal"); }
                        if registry.attention(&targets)? { return answer("attention"); }
                        let steering = registry.state.lock().map_err(|_| failure("state unavailable"))?.steering.clone();
                        if let Some(source) = &steering {
                            let ready = tokio::select! { biased; _ = cancel.cancelled() => return Err(VegaError::Cancelled), ready = source.ready() => ready? };
                            if let Some(id) = ready && registry.steering_ready(id)? { return answer("steering"); }
                        }
                        if deadline.is_some_and(|deadline| tokio::time::Instant::now() >= deadline) { return answer("timeout"); }
                        let timeout = async { match deadline { Some(deadline) => tokio::time::sleep_until(deadline).await, None => futures::future::pending().await } };
                        let readiness = async { if steering.is_some() { tokio::time::sleep(Duration::from_millis(250)).await } else { futures::future::pending().await } };
                        tokio::select! { biased; _ = cancel.cancelled() => return Err(VegaError::Cancelled), _ = &mut changed => {}, _ = timeout => {}, _ = readiness => {} }
                    }
                }
                _ => {
                    let (reply, receiver) = oneshot::channel();
                    let command = if tool == "cancel_agent" {
                        Command::Cancel { target: registry.target(input["agent_id"].as_str().ok_or_else(|| failure("invalid input"))?, input["message_id"].as_str())?, reply }
                    } else {
                        Command::Admit { call_id, tool, input, reply }
                    };
                    tokio::select! { biased; _ = cancel.cancelled() => return Err(VegaError::Cancelled), result = sender.send(command) => result.map_err(|_| failure("parent_run_ended"))? }
                    tokio::select! { biased; _ = cancel.cancelled() => Err(VegaError::Cancelled), result = receiver => result.map_err(|_| failure("parent_run_ended"))? }
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
    pub run_credential_reader: Option<crate::types::RunCredentialReader>,
    pub turn_limit: u32,
    pub steering_source: Option<Arc<dyn vega_runtime::RuntimeSteeringSource>>,
}

impl ChildSettings {
    pub(super) fn new(
        request: &AgentRequest,
        config: &PersistenceActorConfig,
        permission_fence: &crate::types::PermissionFence,
    ) -> Self {
        Self {
            model: request.model.clone(),
            system_prompt: request.system_prompt.clone(),
            authority: DelegatedAuthority {
                permission_fence: permission_fence.clone(),
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
            run_credential_reader: config.run_credential_reader.clone(),
            turn_limit: config.turn_limit,
            steering_source: config.steering_source.clone(),
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

struct ReservedExecution {
    snapshot: SubagentSnapshot,
    task: String,
    call_id: String,
    cancel: CancellationToken,
    previous: Option<ContinuationBoundary>,
    activity: super::stage_activity::StageActivity,
    owner: Arc<DelegatedOwnership>,
    admission: vega_store::thread_admission::ThreadAdmission,
    resume: Option<serde_json::Value>,
}

impl Supervisor {
    pub(super) fn new() -> Self {
        let (sender, receiver) = mpsc::channel(8);
        Self {
            registry: Registry {
                state: Arc::new(Mutex::new(RegistryState::default())),
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
    pub(super) fn receipt_observer(&self) -> ReceiptObserver {
        ReceiptObserver(self.registry.clone())
    }
    pub(super) async fn run<'a, T>(
        &mut self,
        mut parent: LocalBoxFuture<'a, T>,
        launch: ChildLaunch<'a>,
    ) -> T {
        if let Ok(mut state) = self.registry.state.lock() {
            state.steering = launch.settings.steering_source.clone();
        }
        if let Err(error) =
            stage_recovery::load(&self.registry, &launch.database, &launch.parent_thread)
            && let Ok(mut state) = self.registry.state.lock()
        {
            state.admission_failure = Some(error.to_string());
        }
        let mut children: FuturesUnordered<LocalBoxFuture<'a, ()>> = FuturesUnordered::new();
        let result = loop {
            tokio::select! {
                biased;
                result = &mut parent => break result,
                Some(()) = children.next(), if !children.is_empty() => {},
                command = self.receiver.recv() => {
                    match command {
                        Some(Command::Admit { call_id, tool, input, reply }) => {
                            if let Ok(true) = self.registry.begin_call(&call_id, &tool, &input, reply) {
                                match self.reserve(&launch, &call_id, &tool, &input) {
                                    Ok(reserved) => children.push(child_lifecycle(&launch, reserved, self.registry.clone())),
                                    Err(error) => { self.registry.resolve_call(&call_id, Err(error)); }
                                }
                            }
                        }
                        Some(Command::Cancel { target, reply }) => { let result = self.cancel(&launch, &target).await; let _ = reply.send(result); }
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
        call_id: &str,
        tool: &str,
        input: &serde_json::Value,
    ) -> Result<ReservedExecution, VegaError> {
        if launch.cancel.is_cancelled() {
            return Err(failure("parent_run_ended"));
        }
        let mut state = self
            .registry
            .state
            .lock()
            .map_err(|_| failure("state unavailable"))?;
        if state.ended {
            return Err(failure("parent_run_ended"));
        }
        if let Some(error) = &state.admission_failure {
            return Err(failure(error));
        }
        let task = input["task"]
            .as_str()
            .ok_or_else(|| failure("invalid input"))?
            .to_owned();
        let (id, name, previous) = if matches!(tool, "followup_agent" | "resume_agent") {
            let id = input["agent_id"]
                .as_str()
                .ok_or_else(|| failure("invalid input"))?;
            let message = input["message_id"]
                .as_str()
                .ok_or_else(|| failure("invalid input"))?;
            let latest = state
                .latest
                .get(id)
                .ok_or_else(|| failure("unknown_agent"))?;
            if latest != message {
                return Err(failure("stale_execution"));
            }
            let old = state
                .executions
                .get(&(id.into(), message.into()))
                .ok_or_else(|| failure("unknown_agent"))?;
            if old.occupied
                || matches!(
                    old.snapshot.state,
                    SubagentState::Running | SubagentState::Cancelling
                )
            {
                return Err(failure("agent_busy"));
            }
            if (tool == "followup_agent"
                && (old.snapshot.state != SubagentState::Completed
                    || old.snapshot.parent_message_id != launch.parent_message))
                || (tool == "resume_agent" && old.snapshot.state != SubagentState::Interrupted)
            {
                return Err(failure("agent state does not permit this continuation"));
            }
            let previous = old
                .boundary
                .clone()
                .ok_or_else(|| failure("externally_continued"))?;
            (id.to_owned(), old.snapshot.name.clone(), Some(previous))
        } else {
            let name = input["name"]
                .as_str()
                .ok_or_else(|| failure("invalid input"))?
                .trim()
                .to_owned();
            if state.executions.values().any(|record| {
                record.snapshot.parent_message_id == launch.parent_message
                    && record.snapshot.name == name
            }) {
                return Err(failure("name already used in this run"));
            }
            (ulid::Ulid::generate().to_string(), name, None)
        };
        if state
            .executions
            .values()
            .filter(|record| record.occupied)
            .count()
            >= 4
        {
            return Err(failure("capacity_exceeded"));
        }
        let snapshot = SubagentSnapshot {
            agent_id: id.clone(),
            parent_thread_id: launch.parent_thread.clone(),
            parent_message_id: launch.parent_message.clone(),
            message_id: ulid::Ulid::generate().to_string(),
            name,
            state: SubagentState::Running,
            result: String::new(),
            truncated: false,
            checkpoint: None,
            release_state: "held".into(),
            ready_for_review: false,
        };
        let owner = DelegatedOwnership::acquire(&launch.database, &snapshot.agent_id)
            .map_err(|_| failure("native writer ownership unavailable"))?;
        let store = Store::open(&launch.database)?;
        let admission = crate::thread_access::acquire(&store, &snapshot.agent_id)
            .map_err(|_| failure("child admission unavailable"))?;
        let resume = (tool == "resume_agent").then(|| input.clone());
        if let (Some(input), Some(previous)) = (&resume, &previous) {
            stage_recovery::verify_resume(
                &store,
                &snapshot,
                previous,
                input,
                &launch.settings.authority,
            )?;
        }
        let cancel = launch.cancel.child_token();
        let activity = super::stage_activity::StageActivity::new(self.registry.changed.clone());
        state.latest.insert(id.clone(), snapshot.message_id.clone());
        state.executions.insert(
            (id, snapshot.message_id.clone()),
            AgentRecord {
                snapshot: snapshot.clone(),
                call_id: call_id.into(),
                cancel: cancel.clone(),
                receipt_persisted: CancellationToken::new(),
                writer: Some((owner.clone(), admission.clone())),
                occupied: true,
                admitted: false,
                committed: false,
                boundary: None,
                activity: activity.clone(),
                delivered_attention: 0,
                bounded_observed: false,
                terminal_delivered: false,
            },
        );
        Ok(ReservedExecution {
            snapshot,
            task,
            call_id: call_id.into(),
            cancel,
            previous,
            activity,
            owner,
            admission,
            resume,
        })
    }

    async fn cancel(
        &self,
        launch: &ChildLaunch<'_>,
        target: &ExecutionKey,
    ) -> Result<serde_json::Value, VegaError> {
        let (mut snapshot, cancel, writer) = {
            let state = self
                .registry
                .state
                .lock()
                .map_err(|_| failure("state unavailable"))?;
            let record = state
                .executions
                .get(target)
                .ok_or_else(|| failure("unknown_agent"))?;
            (
                record.snapshot.clone(),
                record.cancel.clone(),
                record.writer.clone(),
            )
        };
        if !snapshot.state.is_terminal() {
            cancel.cancel();
            let database = launch.database.clone();
            let (child, run) = target.clone();
            let durable = tokio::task::spawn_blocking(move || {
                let (owner, admission) =
                    writer.ok_or_else(|| failure("child writer unavailable"))?;
                if !owner.verifies(&database, &child) {
                    return Err(failure("native writer identity changed"));
                }
                let store = Store::open(database).map_err(|_| failure("storage unavailable"))?;
                crate::thread_access::verify(&store, &child, &admission)
                    .map_err(|_| failure("child admission identity changed"))?;
                let tx = store
                    .immediate_transaction()
                    .map_err(|_| failure("storage unavailable"))?;
                vega_store::subagents::request_cancel(&tx, &child, &run)
                    .map_err(|_| failure("storage unavailable"))?;
                let execution = vega_store::subagents::execution(&tx, &child, &run)
                    .map_err(|_| failure("storage unavailable"))?;
                tx.commit().map_err(|_| failure("storage unavailable"))?;
                Ok::<_, VegaError>(execution)
            })
            .await
            .map_err(|_| failure("storage unavailable"))??;
            snapshot = self
                .registry
                .snapshots(Some(std::slice::from_ref(target)))?
                .into_iter()
                .next()
                .ok_or_else(|| failure("unknown_agent"))?;
            if let Some(execution) = durable
                && let Some(state) = SubagentState::parse(&execution.record.state)
                && state.is_terminal()
            {
                snapshot.state = state;
                snapshot.result = execution.record.result;
                snapshot.truncated = execution.record.truncated;
                self.registry.update(snapshot.clone())?;
            }
            if !snapshot.state.is_terminal() {
                snapshot.state = SubagentState::Cancelling;
                self.registry.update(snapshot.clone())?;
                launch.host.updated(snapshot.clone())?;
            }
        }
        let snapshot = self
            .registry
            .snapshots(Some(std::slice::from_ref(target)))?
            .into_iter()
            .next()
            .ok_or_else(|| failure("unknown_agent"))?;
        Ok(serde_json::json!({"agent":snapshot}))
    }
}

struct ChildProvider<'a> {
    inner: &'a dyn Provider,
    reader: Option<vega_runtime::CredentialReader>,
}
impl Provider for ChildProvider<'_> {
    fn model_snapshot(&self) -> Option<vega_runtime::model_compat::ResolvedModel> {
        self.inner.model_snapshot()
    }

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

fn child_lifecycle<'a>(
    launch: &ChildLaunch<'a>,
    reserved: ReservedExecution,
    registry: Registry,
) -> LocalBoxFuture<'a, ()> {
    child_lifecycle_inner(launch, reserved, registry)
}

fn child_lifecycle_inner<'a>(
    launch: &ChildLaunch<'a>,
    reserved: ReservedExecution,
    registry: Registry,
) -> LocalBoxFuture<'a, ()> {
    let mut ownership =
        LifecycleOwnership::new(launch.database.clone(), &reserved, registry.clone());
    let ReservedExecution {
        mut snapshot,
        task,
        call_id,
        cancel,
        previous,
        activity,
        owner,
        admission,
        resume,
    } = reserved;
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
                registry.discard(&snapshot, previous.as_ref());
                registry.resolve_call(&call_id, Err(failure("admission_failed")));
                return;
            }
        };
        if cancel.is_cancelled() {
            registry.discard(&snapshot, previous.as_ref());
            registry.resolve_call(&call_id, Err(VegaError::Cancelled));
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
                    && resume.as_ref().is_none_or(|input| {
                        let input_text = input.to_string();
                        vega_runtime::redact_sensitive_credential_text_with_policy(&input_text,&known,None) == input_text
                    })
            });
            if !safe {
                registry.discard(&snapshot, previous.as_ref());
                registry.resolve_call(
                    &call_id,
                    Err(failure("delegated input failed credential safety checks")),
                );
                return;
            }
        }
        let permissions = PermissionQueue::with_activity(activity.clone());
        let questions = UserQuestionQueue::with_activity(activity.clone());
        let queue_generation = ulid::Ulid::generate().to_string();
        let mut resumed_admission = false;
        if let (Some(input), Some(previous)) = (&resume,&previous) {
            let validation = async {
                let store = Store::open(&database)?;
                let checkpoint = stage_recovery::verify_resume(&store,&snapshot,previous,input,&settings.authority)?;
                let row = vega_store::threads::find(store.conn(),&snapshot.agent_id)?.ok_or_else(|| failure("unknown_agent"))?;
                let config = stage_recovery::tool_config(&database,&snapshot,&settings,row.project_id.clone());
                let thread = crate::threads::thread_from_row(&row).map_err(|_| failure("child identity unavailable"))?;
                ownership.arm();
                resumed_admission = true;
                match host.admit_with_ownership(SubagentAdmission { thread,snapshot:snapshot.clone(),cancel:cancel.clone(),permission_queue:permissions.clone(),user_questions:questions.clone(),task:task.clone(),queue_generation:queue_generation.clone() }).await {
                    SubagentAdmissionOutcome::Accepted => {},
                    SubagentAdmissionOutcome::Rejected(error) => { resumed_admission=false; return Err(error); },
                    SubagentAdmissionOutcome::Unknown(error) => return Err(error),
                }
                if cancel.is_cancelled() { return Err(VegaError::Cancelled); }
                stage_recovery::validate_artifacts(&tools,&config,&permissions,&checkpoint,&cancel).await?;
                stage_recovery::verify_resume(&store,&snapshot,previous,input,&settings.authority)?;
                if cancel.is_cancelled() { return Err(VegaError::Cancelled); }
                Ok::<(),VegaError>(())
            }.await;
            if let Err(error) = validation {
                questions.cancel_active(); permissions.finish_active(&cancel);
                if resumed_admission && host.finish_and_release(lifecycle_ownership::unstarted_terminal(&snapshot,&cancel),false).await.is_err() {
                    ownership.quarantine("Recovery validation host release failed; ownership remains occupied");
                } else if ownership.disarm_unstarted() { registry.discard(&snapshot,Some(previous)); }
                registry.resolve_call(&call_id,Err(error));
                return;
            }
        }
        ownership.arm();
        let created = create_child(
            database.clone(),
            snapshot.clone(),
            task.clone(),
            call_id.clone(),
            settings.clone(),
            cancel.clone(),
            previous.clone(),
            resume.clone(),
            owner.clone(),
            admission.clone(),
        )
        .await;
        let (thread, user_id) = match created {
            Ok(created) => created,
            Err(error) => {
                if resumed_admission && host.finish_and_release(lifecycle_ownership::unstarted_terminal(&snapshot,&cancel),false).await.is_err() {
                    ownership.quarantine("Recovery admission host release failed; ownership remains occupied");
                } else if ownership.disarm_unstarted() { registry.discard(&snapshot,previous.as_ref()); }
                registry.resolve_call(&call_id, Err(error));
                return;
            }
        };
        if let Ok(mut state) = registry.state.lock()
            && let Some(record) = state
                .executions
                .get_mut(&(snapshot.agent_id.clone(), snapshot.message_id.clone()))
        {
            record.committed = true;
        }
        let stage_config = stage_recovery::tool_config(&database,&snapshot,&settings,thread.project_id.clone());
        let mut host_may_own = resumed_admission;
        let admitted = if resumed_admission { Ok(()) } else {
            host_may_own = true;
            match host.admit_with_ownership(SubagentAdmission {
            thread,
            snapshot: snapshot.clone(),
            cancel: cancel.clone(),
            permission_queue: permissions.clone(),
            user_questions: questions.clone(),
            task: task.clone(),
            queue_generation: queue_generation.clone(),
        }).await {
            SubagentAdmissionOutcome::Accepted => Ok(()),
            SubagentAdmissionOutcome::Rejected(error) => { host_may_own=false; Err(error) },
            SubagentAdmissionOutcome::Unknown(error) => Err(error),
        }};
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
                registry.resolve_call(&call_id, Err(error));
                Err(ConversationError::Runtime(Arc::new(failure(
                    "child admission failed",
                ))))
            }
            Ok(()) => {
                let receipt_persisted = registry.state.lock().ok().and_then(|mut state| {
                    state
                        .executions
                        .get_mut(&(snapshot.agent_id.clone(), snapshot.message_id.clone()))
                        .map(|record| {
                            record.admitted = true;
                            record.receipt_persisted.clone()
                        })
                });
                if !registry.resolve_call(&call_id, Ok(serde_json::json!({"agent":snapshot}))) {
                    cancel.cancel();
                }
                let ready = match receipt_persisted {
                    Some(receipt_persisted) => tokio::select! {
                        biased;
                        _ = cancel.cancelled() => false,
                        _ = receipt_persisted.cancelled() => true,
                    },
                    None => false,
                };
                if !ready {
                    cancel.cancel();
                    Err(ConversationError::Runtime(Arc::new(VegaError::Cancelled)))
                } else {
                    let mut config = PersistenceActorConfig {
                        delegated: true,
                        delegated_owner: Some(owner.clone()),
                        stage_hook: Some(Arc::new(stage_recovery::StageHook { database:database.clone(),snapshot:snapshot.clone(),owner:owner.clone(),admission:admission.clone(),tools:tools.clone(),config:stage_config.clone(),permissions:permissions.clone(),registry:registry.clone() })),
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
                        run_credential_reader: settings.run_credential_reader.clone(),
                        turn_limit: settings.turn_limit,
                        user_questions: Some(Arc::new(questions.clone())),
                        ..Default::default()
                    };
                    let scoped_provider = ChildProvider {
                        inner: provider,
                        reader: config.owner_credential_reader.clone(),
                    };
                    match Store::open(&database) {
                        Ok(store) => async {
                            crate::thread_access::verify(&store, &snapshot.agent_id, &admission)?;
                            config.admission = Some(admission.clone());
                            if let Some(reader) = config.owner_credential_reader.clone() {
                                config = config.with_pending_input(None, Some(Arc::new(crate::pending_messages::QueueSteeringSource::new(
                                    database.clone(), tools.root().to_path_buf(), snapshot.agent_id.clone(), queue_generation,
                                    reader.clone(), cancel.clone(),
                                ).with_thread_admission(admission.clone()))));
                            }
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
                        }.await,
                        Err(_) => Err(ConversationError::Store("child storage unavailable".into())),
                    }
                }
            }
        };
        activity.finish();
        questions.cancel_active();
        permissions.finish_active(&cancel);
        let handoff = Store::open(&database).and_then(|store| vega_store::delegated_stages::handoff(store.conn(),&snapshot.agent_id,&snapshot.message_id)).ok().flatten().is_some();
        snapshot.state = match &result {
            Ok(_) if handoff => SubagentState::Completed,
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
        let retained = match &result {
            Ok(_) if snapshot.state == SubagentState::Completed => {
                let final_text = Store::open(&database).ok().and_then(|store| {
                    let source = vega_store::context_compaction::load_source(store.conn(), &snapshot.agent_id).ok()?;
                    super::pipeline::completed_stage_text(store.conn(), &source, &snapshot.message_id).ok()
                });
                match final_text {
                    Some(text) => text,
                    None => {
                        snapshot.state = SubagentState::Failed;
                        "Completed response could not be safely identified; inspect the original conversation.".into()
                    }
                }
            }
            Ok(run) if !run.content.is_empty() => run.content.clone(),
            _ if snapshot.state == SubagentState::Interrupted => Store::open_read_only(&database)
                .ok()
                .and_then(|store| messages::find(store.conn(), &snapshot.message_id).ok().flatten())
                .map(|message| message.content)
                .filter(|content| !content.is_empty())
                .unwrap_or_else(|| "Interrupted before completion".into()),
            _ => "Subagent failed. Open the conversation for details.".into(),
        };
        (snapshot.result, snapshot.truncated) = bounded_result(&retained);
        let complete = usage_observer.complete();
        let terminal = persist_child_terminal(database.clone(), snapshot.clone(), complete, owner.clone(), admission.clone()).await;
        let mut persisted = terminal.is_ok();
        let boundary = terminal.ok().flatten();
        if persisted { persisted = match Store::open(&database) { Ok(store) => stage_recovery::refresh(&store,&mut snapshot).is_ok(), Err(_) => false }; }
        if !persisted {
            snapshot.state = SubagentState::Failed;
            snapshot.result = "Subagent result could not be saved".into();
            snapshot.truncated = false;
            registry.quarantine(snapshot.clone(), "Subagent terminal persistence failed; ownership remains occupied");
        } else {
            let _ = registry.update(snapshot.clone());
        }
        let released = !host_may_own || host.finish_and_release(snapshot.clone(),complete && persisted).await.is_ok();
        let release_recorded = if released && persisted {
            Store::open(&database).and_then(|store| vega_store::delegated_stages::set_release(store.conn(),&snapshot.agent_id,&snapshot.message_id,"released")).is_ok()
        } else { false };
        if !release_recorded {
            ownership.quarantine(if !persisted { "Subagent terminal persistence failed; ownership remains occupied" } else { "Subagent host release failed; ownership remains occupied" });
        } else {
            let refreshed = match Store::open(&database) { Ok(store) => stage_recovery::refresh(&store,&mut snapshot).is_ok(), Err(_) => false };
            if refreshed { registry.finish(snapshot,boundary); ownership.disarm(); }
            else { ownership.quarantine("Subagent release receipt unavailable; ownership remains occupied"); }
        }
    }
    .boxed_local()
}

fn same_context(
    left: &vega_store::threads::ThreadRow,
    right: &vega_store::threads::ThreadRow,
) -> bool {
    left.id == right.id
        && left.parent_thread_id == right.parent_thread_id
        && left.workspace_owner_thread_id == right.workspace_owner_thread_id
        && left.forked_from_thread_id == right.forked_from_thread_id
        && left.forked_from_message_id == right.forked_from_message_id
        && left.project_id == right.project_id
        && left.mode == right.mode
        && left.permission_mode == right.permission_mode
        && left.model == right.model
        && left.backend == right.backend
        && left.status == right.status
}

#[allow(clippy::too_many_arguments)]
async fn create_child(
    database: PathBuf,
    snapshot: SubagentSnapshot,
    task: String,
    call_id: String,
    settings: ChildSettings,
    cancel: CancellationToken,
    previous: Option<ContinuationBoundary>,
    resume: Option<serde_json::Value>,
    owner: Arc<DelegatedOwnership>,
    admission: vega_store::thread_admission::ThreadAdmission,
) -> Result<(crate::types::Thread, String), VegaError> {
    tokio::task::spawn_blocking(move || {
        if cancel.is_cancelled() {
            return Err(VegaError::Cancelled);
        }
        if !owner.verifies(&database, &snapshot.agent_id) {
            return Err(failure("native writer identity changed"));
        }
        let store = Store::open(database).map_err(|_| failure("storage unavailable"))?;
        let _permission_coordination = crate::permission_contracts::coordinate()
            .map_err(|_| failure("permission coordination unavailable"))?;
        crate::permission_contracts::require_admission_open(&store)
            .map_err(|_| failure("permission admission unavailable"))?;
        if !settings.authority.matches_fence() {
            return Err(failure("delegated permission source changed"));
        }
        crate::thread_access::verify(&store, &snapshot.agent_id, &admission)
            .map_err(|_| failure("child admission identity changed"))?;
        let tx = store
            .immediate_transaction()
            .map_err(|_| failure("storage unavailable"))?;
        #[cfg(test)]
        steering_tests::steering_lifecycle_tests::admission_checkpoint(&snapshot.message_id);
        if cancel.is_cancelled() {
            return Err(VegaError::Cancelled);
        }
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
        if let Some(previous) = &previous {
            let row = vega_store::threads::find(&tx, &snapshot.agent_id)
                .map_err(|_| failure("storage unavailable"))?
                .ok_or_else(|| failure("unknown_agent"))?;
            let source = vega_store::context_compaction::load_source_version_in_transaction(
                &tx,
                &snapshot.agent_id,
            )
            .map_err(|_| failure("storage unavailable"))?;
            if !same_context(&row, &previous.thread)
                || row.backend != "native"
                || row.mode != "execute"
                || row.permission_mode != permission_mode
                || row.model != settings.model
                || row.parent_thread_id.as_deref() != Some(&snapshot.parent_thread_id)
                || source.source_version != previous.source_version
                || source.fingerprint != previous.fingerprint
                || vega_store::pending_messages::count(&tx, &snapshot.agent_id)
                    .map_err(|_| failure("storage unavailable"))?
                    != 0
            {
                return Err(failure("externally_continued"));
            }
            let latest = vega_store::subagents::find(&tx, &snapshot.agent_id)
                .map_err(|_| failure("storage unavailable"))?
                .ok_or_else(|| failure("unknown_agent"))?;
            if latest.child_message_id != previous.message_id {
                return Err(failure("stale_execution"));
            }
            if let Some(input) = &resume {
                stage_recovery::verify_resume_in_transaction(&tx, &snapshot, previous, input)?;
            }
            if latest.state
                != if resume.is_some() {
                    "interrupted"
                } else {
                    "completed"
                }
            {
                return Err(failure("agent state changed"));
            }
        } else {
            let mut draft = crate::threads::draft_thread(
                parent.project_binding(),
                &settings.model,
                permission_mode,
            );
            draft.reasoning_preferences = parent.reasoning_preferences.clone();
            draft.id = snapshot.agent_id.clone();
            crate::threads::materialize_draft_in_transaction(&tx, &draft)
                .map_err(|_| failure("admission_failed"))?;
            if vega_store::threads::rename(&tx, &snapshot.agent_id, &snapshot.name, now_ms())
                .map_err(|_| failure("admission_failed"))?
                != 1
            {
                return Err(failure("admission_failed"));
            }
        }
        let user_id = ulid::Ulid::generate().to_string();
        let seq = vega_store::messages::next_seq(&tx, &snapshot.agent_id)
            .map_err(|_| failure("admission_failed"))?;
        vega_store::messages::insert(
            &tx,
            &vega_store::messages::MessageRow {
                id: user_id.clone(),
                thread_id: snapshot.agent_id.clone(),
                seq,
                role: "user".into(),
                kind: "text".into(),
                content: task.clone(),
                status: "done".into(),
                created_at: now_ms(),
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .map_err(|_| failure("admission_failed"))?;
        let record = vega_store::subagents::DelegationRow {
            child_thread_id: snapshot.agent_id.clone(),
            parent_thread_id: parent.id,
            parent_message_id: snapshot.parent_message_id,
            spawn_call_id: call_id,
            child_message_id: snapshot.message_id,
            name: snapshot.name,
            task,
            state: "running".into(),
            result: String::new(),
            truncated: false,
            created_at: now_ms(),
        };
        if let Some(previous) = previous {
            if resume.is_some() {
                vega_store::subagents::append_resume(&tx, &record, &previous.message_id, &user_id)
            } else {
                vega_store::subagents::append(&tx, &record, &previous.message_id, &user_id)
            }
            .map_err(|_| failure("admission_failed"))?;
        } else {
            vega_store::subagents::insert(&tx, &record).map_err(|_| failure("admission_failed"))?;
        }
        vega_store::delegated_stages::register(
            &tx,
            &record.child_thread_id,
            &record.child_message_id,
            resume
                .as_ref()
                .and_then(|input| input["checkpoint_id"].as_str()),
            &resume
                .as_ref()
                .map(|input| input["reconciliations"].clone())
                .unwrap_or_else(|| serde_json::json!([])),
        )?;
        let row = vega_store::threads::find(&tx, &snapshot.agent_id)
            .map_err(|_| failure("admission_failed"))?
            .ok_or_else(|| failure("admission_failed"))?;
        let permission_fence = crate::permission_contracts::capture_delegated_fence(
            &store,
            &row,
            &record.parent_thread_id,
            &record.parent_message_id,
            &record.child_message_id,
            &settings.authority.permission_fence,
        )
        .map_err(|_| failure("delegated permission source changed"))?;
        let encoded = serde_json::to_string(&permission_fence)
            .map_err(|_| failure("delegated permission snapshot unavailable"))?;
        vega_store::permission_contracts::record_delegated_fence(
            &tx,
            &record.child_thread_id,
            &record.child_message_id,
            &encoded,
        )?;
        let child =
            crate::threads::thread_from_row(&row).map_err(|_| failure("admission_failed"))?;
        if cancel.is_cancelled() {
            return Err(VegaError::Cancelled);
        }
        tx.commit().map_err(|_| failure("admission_failed"))?;
        Ok((child, user_id))
    })
    .await
    .map_err(|_| failure("admission_failed"))?
}

async fn persist_child_terminal(
    database: PathBuf,
    snapshot: SubagentSnapshot,
    usage_complete: bool,
    owner: Arc<DelegatedOwnership>,
    admission: vega_store::thread_admission::ThreadAdmission,
) -> Result<Option<ContinuationBoundary>, VegaError> {
    tokio::task::spawn_blocking(move || {
        if !owner.verifies(&database, &snapshot.agent_id) {
            return Err(failure("native writer identity changed"));
        }
        let store = Store::open(database).map_err(|_| failure("storage unavailable"))?;
        crate::thread_access::verify(&store, &snapshot.agent_id, &admission)
            .map_err(|_| failure("child admission identity changed"))?;
        let tx = store
            .immediate_transaction()
            .map_err(|_| failure("storage unavailable"))?;
        let effects = vega_store::delegated_stages::unknown_effects(
            &tx,
            &snapshot.agent_id,
            &snapshot.message_id,
        )?;
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
            vega_store::recovery::recover_stopped_with_owner(
                &tx,
                &snapshot.agent_id,
                now_ms(),
                Some(&owner),
            )
            .map_err(|_| failure("storage unavailable"))?;
        }
        if snapshot.state != SubagentState::Completed
            || vega_store::delegated_stages::latest(&tx, &snapshot.agent_id, &snapshot.message_id)?
                .is_some()
        {
            vega_store::delegated_stages::final_checkpoint(
                &tx,
                &snapshot.agent_id,
                &snapshot.message_id,
                snapshot.state != SubagentState::Completed,
                effects,
                now_ms(),
            )?;
        }
        let boundary = if matches!(
            snapshot.state,
            SubagentState::Completed | SubagentState::Interrupted
        ) && vega_store::pending_messages::count(&tx, &snapshot.agent_id)
            .map_err(|_| failure("storage unavailable"))?
            == 0
        {
            let thread = vega_store::threads::find(&tx, &snapshot.agent_id)
                .map_err(|_| failure("storage unavailable"))?
                .ok_or_else(|| failure("unknown_agent"))?;
            let source = vega_store::context_compaction::load_source_version_in_transaction(
                &tx,
                &snapshot.agent_id,
            )
            .map_err(|_| failure("storage unavailable"))?;
            Some(ContinuationBoundary {
                message_id: snapshot.message_id,
                source_version: source.source_version,
                fingerprint: source.fingerprint,
                thread,
            })
        } else {
            None
        };
        tx.commit().map_err(|_| failure("storage unavailable"))?;
        Ok(boundary)
    })
    .await
    .map_err(|_| failure("storage unavailable"))?
}

#[cfg(test)]
#[path = "subagents/steering_tests.rs"]
mod steering_tests;
