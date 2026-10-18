use std::collections::{HashMap, HashSet};
#[cfg(test)]
use std::sync::atomic::*;
use std::sync::*;
use std::time::*;

use gpui_kit::*;
use sha2::{Digest, Sha256};
use vega_conversation::types::*;
use vega_conversation::*;
use vega_store::Store;
use vega_ui::conversation_stream::*;
use vega_ui::plan_card::PlanReviewRequested;

type CredentialReader = Arc<dyn Fn() -> Result<Vec<String>, ()> + Send + Sync>;

pub(crate) const AGENT_EVENT_POLL: Duration = Duration::from_millis(4);
pub(crate) const AGENT_EVENT_CAPACITY: usize = 256;
pub(crate) const AGENT_EVENT_BATCH: usize = 128;
static PROVIDER_CREDENTIAL_ROUTE_DOMAIN: OnceLock<[u8; 32]> = OnceLock::new();

/// One app-owned revocation domain shared by Settings and every run worker.
/// A Settings view must clone this service, never construct an independent
/// registry against the same database.
pub(crate) struct AppMcpSettings(pub(crate) Option<vega_conversation::McpServerSettingsService>);

impl Global for AppMcpSettings {}

#[derive(Clone, Default)]
pub(crate) struct AppReplaySessions(pub(crate) vega_conversation::agent::ReplaySessionRegistry);

impl Global for AppReplaySessions {}

#[cfg(test)]
type TestSseResponder = Arc<dyn Fn(&reqwest::Request) -> String + Send + Sync>;

#[cfg(test)]
#[derive(Default)]
pub(crate) struct AgentWorkerStartProbe {
    pub(crate) sse_responder: Option<TestSseResponder>,
    starts: AtomicUsize,
    tool_starts: Mutex<HashMap<String, usize>>,
    bash_executor: Option<vega_tools::BashTestExecutor>,
    /// Test-only handshake at the existing MockProvider construction boundary.
    /// The sender releases the worker explicitly; dropping it also unblocks cleanup.
    pub(crate) provider_construction_gate:
        Mutex<Option<(mpsc::SyncSender<()>, mpsc::Receiver<()>)>>,
    pub(crate) credential_stages: Mutex<std::collections::HashMap<String, &'static str>>,
}

#[cfg(test)]
impl AgentWorkerStartProbe {
    pub(crate) fn with_sse_responder(responder: TestSseResponder) -> Self {
        Self {
            sse_responder: Some(responder),
            ..Self::default()
        }
    }

    pub(crate) fn with_bash_executor(executor: vega_tools::BashTestExecutor) -> Self {
        Self {
            bash_executor: Some(executor),
            ..Self::default()
        }
    }

    pub(crate) fn load(&self) -> usize {
        self.starts.load(Ordering::SeqCst)
    }

    pub(crate) fn tool_start_counts(&self) -> HashMap<String, usize> {
        self.tool_starts
            .lock()
            .map_or_else(|_| HashMap::new(), |counts| counts.clone())
    }

    fn record_tool_start(&self, call_id: &str) {
        if let Ok(mut counts) = self.tool_starts.lock() {
            *counts.entry(call_id.to_owned()).or_default() += 1;
        }
    }

    fn record(&self) {
        self.starts.fetch_add(1, Ordering::SeqCst);
    }

    fn credential_stage(&self, thread: &str, stage: &'static str) {
        if let Ok(mut stages) = self.credential_stages.lock() {
            stages.insert(thread.to_owned(), stage);
        }
    }
}

#[cfg(test)]
pub(crate) use crate::agent_context::SYSTEM_PROMPT;

pub(crate) enum PendingAgentRun {
    Automation(Box<AutomationRunClaim>),
    UserMessage(UserSubmission),
    ApprovedPlan(String),
    CredentialContinuation(String),
}

/// Frozen explicit user payload, never reconstructed from a later UI draft.
pub(crate) struct UserSubmission {
    pub content: String,
    pub images: Vec<vega_conversation::types::ImageAttachment>,
}

impl From<String> for UserSubmission {
    fn from(content: String) -> Self {
        Self {
            content,
            images: Vec::new(),
        }
    }
}
impl From<&str> for UserSubmission {
    fn from(content: &str) -> Self {
        content.to_owned().into()
    }
}

pub(crate) enum AgentUpdate {
    Subagent(crate::native_subagents::NativeSubagentUpdate),
    Event(vega_conversation::types::ConversationEvent),
    CodexModeConfirmed,
    ComputerUseReady {
        control: ComputerUseControl,
        owner: String,
    },
    McpUnavailable(Vec<vega_conversation::types::McpServerDiagnostic>),
    /// The terminal carries a resolver rejection atomically with `success`.
    /// That prevents a poll between two channel messages from losing the
    /// typed reason before the pending draft is released.
    Finished {
        success: bool,
        reference_failure: Option<FileReferenceFailureCode>,
        credential_failure: bool,
    },
}

pub(crate) struct AgentBatch {
    pub(crate) subagents: Vec<crate::native_subagents::NativeSubagentUpdate>,
    pub(crate) events: Vec<vega_conversation::types::ConversationEvent>,
    pub(crate) codex_mode_confirmed: bool,
    pub(crate) computer_use_ready: Option<(ComputerUseControl, String)>,
    pub(crate) mcp_unavailable: Vec<vega_conversation::types::McpServerDiagnostic>,
    pub(crate) reference_failure: Option<FileReferenceFailureCode>,
    pub(crate) credential_failure: bool,
    pub(crate) finished: Option<bool>,
}

pub(crate) fn drain_agent_updates(receiver: &mpsc::Receiver<AgentUpdate>) -> AgentBatch {
    let mut events = Vec::new();
    let mut subagents = Vec::new();
    let mut codex_mode_confirmed = false;
    let mut computer_use_ready = None;
    let mut mcp_unavailable = Vec::new();
    let mut reference_failure = None;
    let mut credential_failure = false;
    let mut finished = None;
    for _ in 0..AGENT_EVENT_BATCH {
        match receiver.try_recv() {
            Ok(AgentUpdate::Subagent(update)) => subagents.push(update),
            Ok(AgentUpdate::Event(event)) => events.push(event),
            Ok(AgentUpdate::CodexModeConfirmed) => codex_mode_confirmed = true,
            Ok(AgentUpdate::ComputerUseReady { control, owner }) => {
                computer_use_ready = Some((control, owner))
            }
            Ok(AgentUpdate::McpUnavailable(diagnostics)) => {
                mcp_unavailable.extend(diagnostics);
            }
            Ok(AgentUpdate::Finished {
                success,
                reference_failure: terminal_failure,
                credential_failure: terminal_credential_failure,
            }) => {
                reference_failure = terminal_failure;
                credential_failure = terminal_credential_failure;
                finished = Some(success);
                break;
            }
            Err(mpsc::TryRecvError::Empty) => break,
            Err(mpsc::TryRecvError::Disconnected) => {
                finished = Some(false);
                break;
            }
        }
    }
    AgentBatch {
        subagents,
        events,
        codex_mode_confirmed,
        computer_use_ready,
        mcp_unavailable,
        reference_failure,
        credential_failure,
        finished,
    }
}

pub(crate) struct ActiveAgentRun {
    pub(crate) root_message_id: Option<String>,
    pub(crate) delegated_message_id: Option<String>,
    pub(crate) delegated_parent_thread: Option<String>,
    pub(crate) queue_generation: String,
    pub(crate) queue_delivery: Option<PendingMessage>,
    pub(crate) queue_ready: bool,
    pub(crate) generation: u64,
    pub(crate) thread_id: String,
    pub(crate) stream: Entity<ConversationStream>,
    pub(crate) cancel: tokio_util::sync::CancellationToken,
    pub(crate) computer_use: Option<ComputerUseControl>,
    pub(crate) computer_use_owner: String,
    pub(crate) pending_user_content: Option<String>,
    pub(crate) pending_approved_instruction: Option<String>,
    /// Live wall-clock measurement of this run (S7-T40). It exists only in
    /// run memory: the summary card shows it while the run is alive and `—`
    /// after a restart, because `messages` has no finished timestamp (C4).
    pub(crate) started: Instant,
    /// Assistant message id of the run's durable terminal event, if any
    /// (S7-T40 summary projection key; `None` when the run failed before a
    /// message ever started).
    pub(crate) terminal_message_id: Option<String>,
    /// Safe, run-owned terminal diagnosis. The raw provider body remains in
    /// the ephemeral event and is never copied to this controller state.
    pub(crate) terminal_failure: Option<RunFailureKind>,
    /// Value-free Settings connectivity diagnostics frozen at run start.
    pub(crate) mcp_unavailable: Vec<vega_conversation::types::McpServerDiagnostic>,
}

pub(crate) enum AgentBatchIngress {
    Stale,
    Running,
    Finished {
        success: bool,
        run: Box<ActiveAgentRun>,
        reference_failure: Option<FileReferenceFailureCode>,
        credential_failure: bool,
    },
}

#[derive(Clone)]
pub(crate) struct PendingPlanReview {
    pub(crate) stream: Entity<ConversationStream>,
    pub(crate) request: PlanReviewRequested,
}

pub(crate) enum PricingControllerState {
    Loading,
    Ready {
        authority: PricingAuthority,
        generation: u64,
        notice: Option<PricingNotice>,
        draft: Option<PricingSavePlan>,
        draft_reason: Option<PricingDraftReason>,
        error: Option<PricingSettingsErrorCode>,
    },
    Saving {
        previous: PricingAuthority,
        previous_notice: Option<PricingNotice>,
        generation: u64,
        plan: PricingSavePlan,
    },
    Reloading,
    Invalid(PricingSettingsErrorCode),
}

pub(crate) fn pricing_retry_ready(
    authority: PricingAuthority,
    generation: u64,
    notice: Option<PricingNotice>,
    plan: PricingSavePlan,
    code: PricingSettingsErrorCode,
) -> PricingControllerState {
    PricingControllerState::Ready {
        authority,
        generation,
        notice,
        draft: Some(plan),
        draft_reason: Some(PricingDraftReason::RetryPending),
        error: Some(code),
    }
}

pub(crate) fn discard_pricing_draft(state: &mut PricingControllerState, generation: u64) -> bool {
    let PricingControllerState::Ready {
        generation: current,
        draft,
        draft_reason,
        error,
        ..
    } = state
    else {
        return false;
    };
    if *current != generation || draft.is_none() {
        return false;
    }
    *draft = None;
    *draft_reason = None;
    *error = None;
    true
}

pub(crate) struct PricingController {
    pub(crate) service: Option<Arc<PricingSettingsService>>,
    pub(crate) state: PricingControllerState,
    pub(crate) last_generation: u64,
    pub(crate) next_operation: u64,
    pub(crate) active_operation: Option<u64>,
}

impl PricingController {
    pub(crate) fn new(service: Option<Arc<PricingSettingsService>>) -> Self {
        let state = if service.is_some() {
            PricingControllerState::Loading
        } else {
            PricingControllerState::Invalid(PricingSettingsErrorCode::Io)
        };
        Self {
            service,
            state,
            last_generation: 0,
            next_operation: 0,
            active_operation: None,
        }
    }

    pub(crate) fn begin_operation(&mut self) -> Option<u64> {
        if self.active_operation.is_some() {
            return None;
        }
        let operation = self.next_operation.checked_add(1)?;
        self.next_operation = operation;
        self.active_operation = Some(operation);
        Some(operation)
    }

    pub(crate) fn claim_completion(&mut self, operation: u64) -> bool {
        if self.active_operation != Some(operation) {
            return false;
        }
        self.active_operation = None;
        true
    }

    pub(crate) fn next_generation(&mut self) -> Option<u64> {
        let generation = self.last_generation.checked_add(1)?;
        self.last_generation = generation;
        Some(generation)
    }

    pub(crate) fn projection(&self) -> PricingSettingsProjection {
        match &self.state {
            PricingControllerState::Loading => PricingSettingsProjection::Loading,
            PricingControllerState::Ready {
                authority,
                generation,
                notice,
                draft,
                draft_reason,
                error,
            } => match draft {
                Some(plan) => PricingSettingsProjection::Ready {
                    generation: *generation,
                    entries: plan.entries(),
                    notice: *notice,
                    draft_reason: *draft_reason,
                    error: *error,
                },
                None => authority.project(*generation, *notice, None, *error),
            },
            PricingControllerState::Saving {
                generation, plan, ..
            } => PricingSettingsProjection::Saving {
                generation: *generation,
                entries: plan.entries(),
            },
            PricingControllerState::Reloading => PricingSettingsProjection::Reloading,
            PricingControllerState::Invalid(code) => PricingSettingsProjection::Invalid(*code),
        }
    }

    /// #60 R3: accounting is optional. Freeze only committed authority;
    /// an unsaved/retry draft must never affect a running call's estimate.
    pub(crate) fn catalog_for_run(&self, model: &str) -> Option<PricingCatalog> {
        let authority = match &self.state {
            PricingControllerState::Ready { authority, .. } => authority,
            PricingControllerState::Saving { previous, .. } => previous,
            PricingControllerState::Loading
            | PricingControllerState::Reloading
            | PricingControllerState::Invalid(_) => return None,
        };
        authority
            .contains_exact_model(model)
            .then(|| authority.catalog())
    }
}

pub(crate) enum PricingWorkerResult {
    Authority(Result<PricingLoadOutcome, PricingSettingsErrorCode>),
    Save(PricingSaveOutcome),
}

#[derive(Clone, Copy)]
pub(crate) enum PricingWorkerKind {
    Authority,
    Save,
    Recovery,
}

#[derive(Default)]
pub(crate) struct AppAgentController {
    pub(crate) delegated_unbound: HashSet<String>,
    pub(crate) queue_streams: HashMap<String, Entity<ConversationStream>>,
    pub(crate) queue_dispatches: HashMap<String, String>,
    pub(crate) queue_continue: HashMap<String, bool>,
    pub(crate) queue_delivered: HashSet<String>,
    pub(crate) queue_operations: HashMap<String, u64>,
    next_queue_operation: u64,
    pub(crate) running_message_default_generation: u64,
    pub(crate) file_read_states: HashMap<String, vega_tools::ReadState>,
    pub(crate) next_generation: u64,
    pub(crate) active: HashMap<String, ActiveAgentRun>,
    pub(crate) pending_review: HashMap<String, PendingPlanReview>,
    pub(crate) preparation_stream: Option<Entity<ConversationStream>>,
}

impl AppAgentController {
    pub(crate) fn begin_queue_operation(&mut self, thread_id: &str) -> Option<u64> {
        if self.queue_operations.contains_key(thread_id) {
            return None;
        }
        self.next_queue_operation = self.next_queue_operation.checked_add(1)?;
        let operation = self.next_queue_operation;
        self.queue_operations.insert(thread_id.into(), operation);
        Some(operation)
    }

    pub(crate) fn finish_queue_operation(&mut self, thread_id: &str, operation: u64) -> bool {
        if self.queue_operations.get(thread_id) != Some(&operation) {
            return false;
        }
        self.queue_operations.remove(thread_id);
        true
    }

    pub(crate) fn active_stream_for_thread(
        &self,
        thread_id: &str,
    ) -> Option<Entity<ConversationStream>> {
        self.active
            .get(thread_id)
            .map(|active| active.stream.clone())
            .or_else(|| self.queue_streams.get(thread_id).cloned())
    }

    pub(crate) fn request_active_cancel(&self, thread_id: &str) {
        if let Some(active) = self.active.get(thread_id) {
            if let Some(control) = &active.computer_use {
                control.stop();
            }
            active.cancel.cancel();
        }
    }

    pub(crate) fn queue_review(
        &mut self,
        stream: &Entity<ConversationStream>,
        request: &PlanReviewRequested,
    ) -> bool {
        if !self
            .active
            .get(&request.thread_id)
            .is_some_and(|run| run.stream == *stream)
            || self.pending_review.contains_key(&request.thread_id)
        {
            return false;
        }
        self.pending_review.insert(
            request.thread_id.clone(),
            PendingPlanReview {
                stream: stream.clone(),
                request: request.clone(),
            },
        );
        self.request_active_cancel(&request.thread_id);
        true
    }

    pub(crate) fn begin(
        &mut self,
        thread_id: String,
        stream: Entity<ConversationStream>,
        pending_user_content: Option<String>,
        pending_approved_instruction: Option<String>,
    ) -> Option<(u64, tokio_util::sync::CancellationToken)> {
        if self.active.contains_key(&thread_id) || self.queue_operations.contains_key(&thread_id) {
            return None;
        }
        self.next_generation = self.next_generation.checked_add(1)?;
        let generation = self.next_generation;
        let cancel = tokio_util::sync::CancellationToken::new();
        self.active.insert(
            thread_id.clone(),
            ActiveAgentRun {
                root_message_id: None,
                delegated_message_id: None,
                delegated_parent_thread: None,
                queue_generation: ulid::Ulid::generate().to_string(),
                queue_delivery: None,
                queue_ready: false,
                generation,
                thread_id,
                stream,
                cancel: cancel.clone(),
                computer_use: None,
                computer_use_owner: String::new(),
                pending_user_content,
                pending_approved_instruction,
                started: Instant::now(),
                terminal_message_id: None,
                terminal_failure: None,
                mcp_unavailable: Vec::new(),
            },
        );
        Some((generation, cancel))
    }

    pub(crate) fn matches(
        &self,
        generation: u64,
        thread_id: &str,
        stream: &Entity<ConversationStream>,
    ) -> bool {
        self.active.get(thread_id).is_some_and(|active| {
            active.generation == generation
                && active.thread_id == thread_id
                && active.stream == *stream
        })
    }

    pub(crate) fn accept_durable_start(
        &mut self,
        generation: u64,
        thread_id: &str,
        stream: &Entity<ConversationStream>,
    ) -> Option<String> {
        if !self.matches(generation, thread_id, stream) {
            return None;
        }
        let active = self.active.get_mut(thread_id)?;
        active.pending_approved_instruction = None;
        active.pending_user_content.take()
    }

    /// Records the run's terminal assistant message id from the durable
    /// terminal event (S7-T40 summary projection key). Duplicate or later
    /// events overwrite in place; the id is only consumed at run finish.
    pub(crate) fn observe_terminal_message(
        &mut self,
        generation: u64,
        thread_id: &str,
        stream: &Entity<ConversationStream>,
        event: &ConversationEvent,
    ) {
        if self.matches(generation, thread_id, stream)
            && let Some(active) = self.active.get_mut(thread_id)
        {
            if let ConversationEvent::MessageStarted { message_id, .. } = event {
                active
                    .root_message_id
                    .get_or_insert_with(|| message_id.clone());
            }
            match event {
                ConversationEvent::MessageStarted { .. }
                | ConversationEvent::Interrupted { .. }
                | ConversationEvent::Error { .. } => active.queue_ready = false,
                ConversationEvent::MessageFinished { stop_reason, .. } => {
                    active.queue_ready = *stop_reason == ConversationStopReason::End
                }
                _ => {}
            }
        }
        let (message_id, failure) = match event {
            ConversationEvent::MessageFinished { message_id, .. }
            | ConversationEvent::Interrupted { message_id, .. } => (Some(message_id), None),
            ConversationEvent::Error {
                message_id, error, ..
            } => (
                message_id.as_ref(),
                Some(RunFailureKind::from_runtime(error)),
            ),
            _ => return,
        };
        if !self.matches(generation, thread_id, stream) {
            return;
        }
        if let Some(active) = self.active.get_mut(thread_id) {
            if let Some(message_id) = message_id {
                active.terminal_message_id = Some(message_id.clone());
            }
            if let Some(failure) = failure {
                active.terminal_failure = Some(failure);
            }
        }
    }

    pub(crate) fn finish(
        &mut self,
        generation: u64,
        thread_id: &str,
        stream: &Entity<ConversationStream>,
    ) -> Option<ActiveAgentRun> {
        if !self.matches(generation, thread_id, stream) {
            return None;
        }
        let active = self.active.remove(thread_id)?;
        if let Some(control) = &active.computer_use {
            control.stop();
        }
        Some(active)
    }
}

pub(crate) fn unique_provider_for_model(
    config: &vega_store::config::AppConfig,
    model: &str,
) -> Option<vega_store::config::ProviderConfig> {
    let mut matches = config
        .providers
        .iter()
        .filter(|provider| provider.enabled)
        .filter(|provider| provider.models.iter().any(|candidate| candidate == model));
    let provider = matches.next()?.clone();
    if matches.next().is_some()
        || provider.base_url.trim().is_empty()
        || provider.key_ref.trim().is_empty()
    {
        return None;
    }
    Some(provider)
}

fn provider_replay_route_fingerprint(
    provider: &vega_store::config::ProviderConfig,
    credential_root: &std::path::Path,
    credential: &str,
    database_path: &std::path::Path,
    project_id: &str,
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    let api = format!("{:?}", provider.api);
    for (tag, value) in [
        (1, provider.name.as_bytes()),
        (2, api.as_bytes()),
        (3, provider.base_url.as_bytes()),
        (4, provider.key_ref.as_bytes()),
    ] {
        hasher.update([tag]);
        hasher.update((value.len() as u64).to_le_bytes());
        hasher.update(value);
    }
    hasher.update([5]);
    let process_domain = PROVIDER_CREDENTIAL_ROUTE_DOMAIN.get_or_init(|| {
        let mut domain = Sha256::new();
        domain.update(ulid::Ulid::generate().to_string().as_bytes());
        domain.update(ulid::Ulid::generate().to_string().as_bytes());
        domain.finalize().into()
    });
    hasher.update(process_domain);
    hasher.update([6]);
    hasher.update((credential.len() as u64).to_le_bytes());
    hasher.update(credential.as_bytes());
    hasher.update([7]);
    let root = credential_root.to_string_lossy();
    hasher.update((root.len() as u64).to_le_bytes());
    hasher.update(root.as_bytes());
    hasher.update([8]);
    let database = database_path.to_string_lossy();
    hasher.update((database.len() as u64).to_le_bytes());
    hasher.update(database.as_bytes());
    hasher.update([9]);
    hasher.update((project_id.len() as u64).to_le_bytes());
    hasher.update(project_id.as_bytes());
    hasher.finalize().into()
}

fn replay_persistence_config(
    registry: Option<&vega_conversation::agent::ReplaySessionRegistry>,
    route_fingerprint: Option<[u8; 32]>,
) -> vega_conversation::agent::PersistenceActorConfig {
    let config = vega_conversation::agent::PersistenceActorConfig::default();
    if let (Some(registry), Some(route_fingerprint)) = (registry, route_fingerprint) {
        config.with_replay_session_registry(registry.clone(), route_fingerprint)
    } else {
        config
    }
}

/// Performs the content-free provider and owner-only credential readiness
/// check used before a composer submission can materialize a draft.
///
/// All config/keystore IO is expected to run in the caller's bounded worker;
/// this function intentionally returns only typed provider/model identifiers
/// and never returns or logs a credential value.
pub(crate) fn preflight_provider(
    config_path: Option<&std::path::Path>,
    model: &str,
) -> Result<(), ProviderPreflightFailure> {
    let unavailable = |providers: Vec<String>| ProviderPreflightFailure::ProviderUnavailable {
        model: model.to_owned(),
        providers,
    };
    let Some(config_path) = config_path else {
        return Err(unavailable(Vec::new()));
    };
    let config = vega_store::config::read_from(config_path).map_err(|_| unavailable(Vec::new()))?;
    let mut providers = config
        .providers
        .iter()
        .filter(|provider| provider.models.iter().any(|candidate| candidate == model))
        .map(|provider| provider.name.clone())
        .collect::<Vec<_>>();
    providers.sort();
    providers.dedup();

    let enabled = config
        .providers
        .iter()
        .filter(|provider| provider.enabled)
        .filter(|provider| provider.models.iter().any(|candidate| candidate == model))
        .collect::<Vec<_>>();
    let Some(provider) = (enabled.len() == 1).then(|| enabled[0]) else {
        return Err(unavailable(providers));
    };
    if provider.base_url.trim().is_empty()
        || !vega_runtime::provider_check::valid_base_url(&provider.base_url)
        || provider.key_ref.trim().is_empty()
    {
        return Err(unavailable(vec![provider.name.clone()]));
    }
    let Some(root) = config_path.parent() else {
        return Err(unavailable(vec![provider.name.clone()]));
    };
    vega_store::keystore::get_key(root, &provider.key_ref).map_err(|_| {
        ProviderPreflightFailure::CredentialUnavailable {
            model: model.to_owned(),
            provider: provider.name.clone(),
        }
    })?;
    Ok(())
}

/// Resolves one immutable reasoning snapshot for the exact provider/model
/// pair selected for a run. Missing profiles intentionally become provider
/// default with no wire controls; malformed declared profiles fail closed so
/// an invalid Settings edit cannot silently become a different request.
pub(crate) fn reasoning_for_provider_model(
    provider: &str,
    api: vega_store::config::ProviderApi,
    base_url: &str,
    model: &str,
    reasoning_path: Option<&std::path::Path>,
) -> Result<vega_runtime::FrozenReasoning, ()> {
    let configured = match reasoning_path {
        Some(reasoning_path) => {
            let config = vega_store::reasoning::read_from(reasoning_path).map_err(|_| ())?;
            config
                .profile(provider, model)
                .map(ReasoningProfileProjection::from_store)
                .transpose()
                .map_err(|_| ())?
        }
        None => None,
    };
    let resolved = crate::reasoning_capabilities::resolve_reasoning_profile(
        provider, api, base_url, model, configured,
    )
    .map_err(|_| ())?;
    crate::reasoning_capabilities::freeze_reasoning_profile(resolved).map_err(|_| ())
}

fn reasoning_projection_from_frozen(
    frozen: &vega_runtime::FrozenReasoning,
) -> ReasoningProfileProjection {
    use vega_conversation::types::{ReasoningReplaySetting, ReasoningSupport};
    let replay_profile = match frozen.replay_policy {
        vega_runtime::ReasoningReplayPolicy::StandardChat => ReasoningReplaySetting::StandardChat,
        vega_runtime::ReasoningReplayPolicy::DeepSeekTools => ReasoningReplaySetting::DeepSeekTools,
        vega_runtime::ReasoningReplayPolicy::GlmPreserved => ReasoningReplaySetting::GlmPreserved,
        vega_runtime::ReasoningReplayPolicy::GlmInterleaved => {
            ReasoningReplaySetting::GlmInterleaved
        }
        vega_runtime::ReasoningReplayPolicy::ResponsesItems => {
            ReasoningReplaySetting::ResponsesItems
        }
        vega_runtime::ReasoningReplayPolicy::LegacyTextTools => {
            ReasoningReplaySetting::LegacyTextTools
        }
        vega_runtime::ReasoningReplayPolicy::Unknown => ReasoningReplaySetting::Auto,
    };
    ReasoningProfileProjection {
        provider: frozen.provider.clone(),
        model: frozen.model.clone(),
        protocol: frozen.protocol,
        support: if frozen.supports_disabled || !frozen.declared_efforts.is_empty() {
            ReasoningSupport::Optional
        } else if matches!(
            frozen.protocol,
            vega_runtime::ReasoningProtocol::ZhipuChatCompletions
        ) {
            ReasoningSupport::Required
        } else {
            ReasoningSupport::Unknown
        },
        efforts: frozen.declared_efforts.clone(),
        supports_disabled: frozen.supports_disabled,
        disabled_wire: frozen.disabled_wire,
        replay_profile,
        preference: frozen.choice.clone(),
    }
}

pub(crate) fn commit_provider(
    thread: &Thread,
    config_path: Option<&std::path::Path>,
) -> Result<Arc<dyn vega_runtime::Provider>, ()> {
    let path = config_path.ok_or(())?;
    let config = vega_store::config::read_from(path).map_err(|_| ())?;
    let provider = unique_provider_for_model(&config, &thread.model).ok_or(())?;
    let key = vega_store::keystore::get_key(path.parent().ok_or(())?, &provider.key_ref)
        .map_err(|_| ())?;
    let provider = vega_runtime::OpenAiProvider::new(provider.base_url, key)
        .map(|transport| {
            transport.with_responses_api(
                provider.api == vega_conversation::types::ProviderApi::Responses,
            )
        })
        .map_err(|_| ())?;
    Ok(Arc::new(provider.with_retry_policy(commit_retry_policy())))
}

pub(crate) fn commit_retry_policy() -> vega_runtime::RetryPolicy {
    vega_runtime::RetryPolicy {
        max_retries: 0,
        ..vega_runtime::RetryPolicy::default()
    }
}

/// Converts resolver diagnostics into the content-free shared failure
/// vocabulary before they cross the worker/UI boundary.
pub(crate) fn map_reference_failure(error: &vega_tools::ToolError) -> FileReferenceFailureCode {
    use vega_tools::ToolError;
    match error {
        ToolError::PathEscape(_) => FileReferenceFailureCode::OutsideProject,
        ToolError::NotFound(_) => FileReferenceFailureCode::Missing,
        ToolError::BinaryFile(_) => FileReferenceFailureCode::BinaryContent,
        ToolError::TooManyResults { limit }
            if *limit == vega_tools::reference::REFERENCE_MAX_FILES =>
        {
            FileReferenceFailureCode::TooManyReferences
        }
        ToolError::TooManyResults { limit }
            if *limit == vega_tools::reference::REFERENCE_MAX_FILE_BYTES as usize =>
        {
            FileReferenceFailureCode::FileTooLarge
        }
        ToolError::TooManyResults { .. } => FileReferenceFailureCode::TotalBytesExceeded,
        ToolError::InvalidInput(message) if message.starts_with("symlinked reference") => {
            FileReferenceFailureCode::SymlinkRejected
        }
        ToolError::InvalidInput(message) if message.ends_with(" is a directory") => {
            FileReferenceFailureCode::NotRegularFile
        }
        ToolError::InvalidInput(_) => FileReferenceFailureCode::InvalidReference,
        ToolError::Io(_) | ToolError::Traversal(_) => FileReferenceFailureCode::ReadFailed,
        ToolError::Mutation(_) => FileReferenceFailureCode::InvalidReference,
    }
}

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
pub(crate) fn run_agent_worker(
    database_path: std::path::PathBuf,
    project_path: std::path::PathBuf,
    thread: Thread,
    run: PendingAgentRun,
    permission_queue: vega_conversation::agent::PermissionQueue,
    cancel: tokio_util::sync::CancellationToken,
    sender: mpsc::SyncSender<AgentUpdate>,
    pricing_catalog: Option<vega_conversation::PricingCatalog>,
    config_path: Option<std::path::PathBuf>,
    reasoning: Option<vega_runtime::FrozenReasoning>,
    title_notifications: Option<mpsc::Sender<()>>,
    provider_override: Option<Arc<dyn vega_runtime::Provider>>,
    worker_start_probe: Arc<AgentWorkerStartProbe>,
) {
    run_agent_worker_with_mcp(
        database_path,
        project_path,
        thread,
        run,
        permission_queue,
        vega_conversation::agent::UserQuestionQueue::new(),
        cancel,
        sender,
        pricing_catalog,
        config_path,
        reasoning,
        title_notifications,
        None,
        vega_tools::ReadState::default(),
        None,
        Vec::new(),
        provider_override,
        Some(vega_conversation::agent::ReplaySessionRegistry::new()),
        worker_start_probe,
    );
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_agent_worker_with_mcp(
    database_path: std::path::PathBuf,
    project_path: std::path::PathBuf,
    thread: Thread,
    run: PendingAgentRun,
    permission_queue: vega_conversation::agent::PermissionQueue,
    user_questions: vega_conversation::agent::UserQuestionQueue,
    cancel: tokio_util::sync::CancellationToken,
    sender: mpsc::SyncSender<AgentUpdate>,
    // S7-T39/C3: frozen run-start pricing selection handed off with run
    // ownership; the worker never re-reads pricing files or the live
    // authority mid-run.
    pricing_catalog: Option<vega_conversation::PricingCatalog>,
    // Owned config path selected by the app/controller. The worker must never
    // fall back to process-global config while checking the frozen owner.
    config_path: Option<std::path::PathBuf>,
    // Optional UI-captured reasoning snapshot. When absent (legacy callers),
    // the worker resolves the exact profile once before the first request.
    reasoning: Option<vega_runtime::FrozenReasoning>,
    title_notifications: Option<mpsc::Sender<()>>,
    mcp_service: Option<vega_conversation::McpServerSettingsService>,
    file_read_state: vega_tools::ReadState,
    queue_context: Option<PendingRunContext>,
    automation_pauses: Vec<AutomationTask>,
    #[cfg(test)] provider_override: Option<Arc<dyn vega_runtime::Provider>>,
    replay_registry: Option<vega_conversation::agent::ReplaySessionRegistry>,
    #[cfg(test)] worker_start_probe: Arc<AgentWorkerStartProbe>,
) {
    #[cfg(test)]
    worker_start_probe.record();
    let automation_claim = match &run {
        PendingAgentRun::Automation(claim) => Some(claim.as_ref().clone()),
        _ => None,
    };
    let automation_path = database_path.clone();
    let automation_thread = thread.id.clone();
    let automation_snapshot = Store::open(&automation_path)
        .ok()
        .and_then(|store| vega_conversation::automation::load(&store, &automation_thread).ok())
        .unwrap_or_default();
    let run = match run {
        PendingAgentRun::Automation(claim) => {
            PendingAgentRun::UserMessage(automation_trigger(&claim).into())
        }
        other => other,
    };
    let mut reference_failure = None;
    let mut credential_failure = false;
    let mut computer_use: Option<ComputerUseRun> = None;
    let success = (|| -> Result<bool, ()> {
        let project_failure = |error| {
            let _ = sender.send(AgentUpdate::Event(ConversationEvent::Error {
                message_id: None,
                error: Arc::new(vega_runtime::VegaError::ProjectInstructions(error)),
                execution_duration_ms: None,
            }));
        };
        crate::agent_context::project_root_text(&project_path).map_err(project_failure)?;
        // Config and local credential storage are touched only after an explicit user submit
        // or committed Plan approval reaches this worker.
        let tools = vega_tools::Tools::new(&project_path)
            .map_err(|_| ())?
            .with_read_state(file_read_state);
        #[cfg(test)]
        let tools = match &worker_start_probe.bash_executor {
            Some(executor) => tools.with_bash_test_executor(executor.clone()),
            None => tools,
        };
        let store = Store::open(&database_path).map_err(|_| ())?;
        store.migrate().map_err(|_| ())?;
        vega_conversation::automation::pause_matching(&store, &thread.id, &automation_pauses, "已由你暂停").map_err(|_| ())?;
        // Resolve provider identity once, before constructing the provider and
        // before entering the runtime. The resulting FrozenReasoning is moved
        // into the selected entry point and is never re-read during retries
        // or tool rounds.
        let config = config_path
            .as_deref()
            .and_then(|path| vega_store::config::read_from(path).ok());
        let tools = crate::agent_context::protect_project_tools(tools, config_path.as_deref(), config.as_ref())?;
        let system_prompt = crate::agent_context::compose_agent_system_prompt(&tools).map_err(project_failure)?;
        let configured_provider = config
            .as_ref()
            .and_then(|config| unique_provider_for_model(config, &thread.model));
        let mut replay_route_fingerprint = None;
        // Issue #114: agentic turn guardrail from `[agent] turn_limit`
        // (0 = unlimited). Absent config (legacy callers/tests) stays unlimited.
        let turn_limit = config.as_ref().map_or(0, |config| config.agent.turn_limit);
        if let Some(config) = &config
            && (config.agent.desktop_use_enabled || config.agent.browser_use_enabled)
        {
            let run = ComputerUseRun::new(
                config.agent.desktop_use_enabled,
                config.agent.browser_use_enabled,
            );
            if cancel.is_cancelled() {
                run.control.stop();
                return Err(());
            }
            sender
                .send(AgentUpdate::ComputerUseReady {
                    control: run.control.clone(),
                    owner: if thread.title.is_empty() {
                        "未命名任务".into()
                    } else {
                        thread.title.clone()
                    },
                })
                .map_err(|_| ())?;
            computer_use = Some(run);
        }
        // Production must never turn an explicit config path with no unique
        // enabled owner into a synthetic `unknown` policy. Legacy tests may
        // inject a provider without a matching config, but an actual two-owner
        // ambiguity remains forbidden even through that test seam.
        #[cfg(test)]
        let allow_test_override = provider_override.is_some()
            && !config.as_ref().is_some_and(|config| {
                config
                    .providers
                    .iter()
                    .filter(|provider| {
                        provider.enabled
                            && provider.models.iter().any(|model| model == &thread.model)
                    })
                    .take(2)
                    .count()
                    > 1
            });
        #[cfg(not(test))]
        let allow_test_override = false;
        if config_path.is_some() && configured_provider.is_none() && !allow_test_override {
            return Err(());
        }
        let provider_name = configured_provider
            .as_ref()
            .map_or("unknown", |provider| provider.name.as_str());
        let reasoning_path = config_path
            .as_deref()
            .map(|path| path.with_file_name(vega_store::reasoning::REASONING_FILE_NAME));
        let reasoning = match (reasoning, configured_provider.as_ref()) {
            (Some(reasoning), Some(provider)) => {
                if reasoning.model != thread.model || reasoning.provider != provider.name {
                    return Err(());
                }
                let projection = reasoning_projection_from_frozen(&reasoning);
                let resolved = crate::reasoning_capabilities::resolve_reasoning_profile(
                    &provider.name,
                    provider.api,
                    &provider.base_url,
                    &thread.model,
                    Some(projection),
                )
                .map_err(|_| ())?;
                crate::reasoning_capabilities::freeze_reasoning_profile(resolved).map_err(|_| ())?
            }
            (Some(reasoning), None) => {
                reasoning.validate().map_err(|_| ())?;
                if reasoning.model != thread.model {
                    return Err(());
                }
                reasoning
            }
            (None, Some(provider)) => reasoning_for_provider_model(
                &provider.name,
                provider.api,
                &provider.base_url,
                &thread.model,
                reasoning_path.as_deref(),
            )?,
            (None, None) => vega_runtime::FrozenReasoning::unknown(provider_name, &thread.model),
        };
        // Provider construction stays below the reference resolver so an
        // unresolved @file can terminate with zero provider requests or
        // construction, preserving R5's fail-closed boundary.
        let mut provider_known_credential = None::<String>;
        let provider_credential_reader: Option<CredentialReader> = config_path
            .as_deref()
            .and_then(std::path::Path::parent)
            .zip(configured_provider.as_ref())
            .map(|(root, provider)| {
                let root = root.to_path_buf();
                let key_ref = provider.key_ref.clone();
                Arc::new(move || {
                    vega_store::keystore::get_key(&root, &key_ref)
                        .map(|key| vec![key])
                        .map_err(|_| ())
                }) as CredentialReader
            });
        let broker_credential_reader: Option<CredentialReader> = config_path
            .as_deref()
            .and_then(std::path::Path::parent)
            .map(|root| {
                let root = root.to_path_buf();
                let refs = config
                    .as_ref()
                    .map(|config| {
                        config
                            .credential_bindings
                            .iter()
                            .map(|binding| binding.key_ref.clone())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                Arc::new(move || {
                    refs.iter()
                        .map(|reference| {
                            vega_store::keystore::get_key(&root, reference).map_err(|_| ())
                        })
                        .collect()
                }) as CredentialReader
            });
        let credential_root = config_path.as_deref().and_then(std::path::Path::parent).map(std::path::Path::to_path_buf);
        let credential_exceptions: vega_runtime::CredentialExceptionFilter = credential_root.as_ref()
            .and_then(|root| vega_conversation::privacy::exception_filter(database_path.clone(), thread.id.clone(), root.clone()).ok())
            .unwrap_or_else(|| Arc::new(|_, _| false));
        let authority_database = database_path.clone();
        let authority_root = credential_root.clone();
        let owner = mcp_service.clone();
        let selected_provider_reader = provider_credential_reader.clone();
        #[cfg(test)]
        let authority_probe = worker_start_probe.clone();
        #[cfg(test)]
        let authority_thread = thread.id.clone();
        let owner_credential_reader: CredentialReader = Arc::new(move || {
            #[cfg(test)]
            authority_probe.credential_stage(&authority_thread, "authority_refresh");
            if let Some(root) = &authority_root {
                vega_conversation::privacy::refresh_authority(&authority_database, root).map_err(|_| ())?;
            }
            #[cfg(test)]
            authority_probe.credential_stage(&authority_thread, "owner_read");
            let mut known = match &owner {
                Some(service) => service.current_owner_credential_values().map_err(|_| ())?,
                None => Vec::new(),
            };
            #[cfg(test)]
            authority_probe.credential_stage(&authority_thread, "provider_read");
            if let Some(reader) = &selected_provider_reader {
                known.extend(reader()?);
            }
            #[cfg(test)]
            authority_probe.credential_stage(&authority_thread, "binding_read");
            if let Some(reader) = &broker_credential_reader {
                known.extend(reader()?);
            }
            #[cfg(test)]
            authority_probe.credential_stage(&authority_thread, "authority_ready");
            Ok(known)
        });
        let mut make_provider = || -> Result<Arc<dyn vega_runtime::Provider>, ()> {
            #[cfg(test)]
            {
                let gate = worker_start_probe
                    .provider_construction_gate
                    .lock()
                    .ok()
                    .and_then(|mut gate| gate.take());
                if let Some((entered, release)) = gate {
                    entered.send(()).map_err(|_| ())?;
                    // Elapsed time is not a release. Test panic drops the sender,
                    // which terminates this worker without constructing a provider.
                    release.recv().map_err(|_| ())?;
                }
                if let Some(provider) = provider_override.clone() {
                    worker_start_probe.credential_stage(&thread.id, "provider_constructed");
                    return Ok(provider);
                }
            }
            let provider = configured_provider.clone().ok_or(())?;
            let root = config_path
                .as_deref()
                .and_then(std::path::Path::parent)
                .ok_or(())?;
            let key = vega_store::keystore::get_key(root, &provider.key_ref).map_err(|_| {
                credential_failure = true;
            })?;
            replay_route_fingerprint = Some(provider_replay_route_fingerprint(
                &provider,
                root,
                &key,
                &database_path,
                &thread.project_id,
            ));
            provider_known_credential = Some(key.clone());
            let provider = vega_runtime::OpenAiProvider::new(provider.base_url, key)
                .map(|transport| {
                    transport.with_responses_api(
                        provider.api == vega_conversation::types::ProviderApi::Responses,
                    )
                })
                .map_err(|_| ())?
                .with_pre_attempt_guard(
                    vega_conversation::agent::OwnerCredentialProvider::pre_attempt_guard_with_exceptions(
                        owner_credential_reader.clone(),
                        credential_exceptions.clone(),
                    ),
                );
            #[cfg(test)]
            let provider = if let Some(responder) = worker_start_probe.sse_responder.clone() {
                provider.with_test_sse_responder(move |request| responder(request))
            } else {
                provider
            };
            Ok(Arc::new(provider) as Arc<dyn vega_runtime::Provider>)
        };
        let guard_provider = |provider: Arc<dyn vega_runtime::Provider>| {
            Arc::new(vega_conversation::agent::OwnerCredentialProvider::new(
                provider,
                owner_credential_reader.clone(),
            ).with_credential_exceptions(credential_exceptions.clone())) as Arc<dyn vega_runtime::Provider>
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| ())?;
        let event_sender = sender.clone();
        #[cfg(test)]
        let event_probe = worker_start_probe.clone();
        let event_sink = move |event: &vega_conversation::types::ConversationEvent| {
            #[cfg(test)]
            if let vega_conversation::types::ConversationEvent::ToolCallRunning { call_id } = event
            {
                event_probe.record_tool_start(call_id);
            }
            event_sender
                .send(AgentUpdate::Event(event.clone()))
                .map_err(|_| {
                    vega_runtime::VegaError::Io(std::io::Error::other(
                        "agent UI channel unavailable",
                    ))
                })
        };
        let steering_source = queue_context.as_ref().map(|context| {
            Arc::new(
                vega_conversation::pending_messages::QueueSteeringSource::new(
                    database_path.clone(),
                    project_path.clone(),
                    thread.id.clone(),
                    context.generation.clone(),
                    owner_credential_reader.clone(),
                    cancel.clone(),
                ).with_credential_exceptions(credential_exceptions.clone()),
            ) as Arc<dyn vega_runtime::RuntimeSteeringSource>
        });
        let pending_message_id = queue_context.and_then(|context| context.message_id);
        let subagent_host = crate::native_subagents::NativeSubagentHost::new(sender.clone());
        let result = match run {
            PendingAgentRun::Automation(_) => return Err(()),
            PendingAgentRun::UserMessage(submission) => {
                let content = submission.content;
                // #65 R2: freeze only composer text, before expanding referenced files.
                let title_source = content.clone();
                // A2-12: resolve `@path` tokens against the project root and
                // inject the referenced file contents ahead of the user text
                // (bounded: 8 files, 16 KiB each, 48 KiB total). A failure is
                // fail-closed: no provider is constructed and no request is
                // started with the unresolved user text.
                let refs = match vega_tools::reference::resolve_bounded_references(
                    &project_path,
                    &content,
                    vega_tools::reference::REFERENCE_MAX_FILES,
                    vega_tools::reference::REFERENCE_MAX_FILE_BYTES,
                    vega_tools::reference::REFERENCE_MAX_TOTAL_BYTES,
                ) {
                    Ok(refs) => refs,
                    Err(error) => {
                        reference_failure = Some(map_reference_failure(&error));
                        return Err(());
                    }
                };
                let content = if refs.is_empty() {
                    content
                } else {
                    format!(
                        "{}\n\n{}",
                        vega_tools::reference::render_reference_block(&refs),
                        content
                    )
                };
                let provider = guard_provider(make_provider()?);
                let automatic_title = title_notifications.map(|notifications| {
                    vega_conversation::types::AutomaticTitleRequest::new(
                        &title_source,
                        provider.clone(),
                        tokio_util::sync::CancellationToken::new(),
                        notifications,
                    )
                });
                // local credential storage access is synchronous. A route cancellation while
                // it was waiting must not start a late durable/network run.
                if cancel.is_cancelled() {
                    return Err(());
                }
                let mcp_servers = match &mcp_service {
                    Some(service) => runtime
                        .block_on(async {
                            tokio::select! {
                                biased;
                                _ = cancel.cancelled() => Err(()),
                                readiness = service.ready_for_run() => readiness.map_err(|_| ()),
                            }
                        })
                        .map(|readiness| {
                            if !readiness.unavailable.is_empty() {
                                let _ =
                                    sender.send(AgentUpdate::McpUnavailable(readiness.unavailable));
                            }
                            readiness.ready_servers
                        })?,
                    None => Vec::new(),
                };
                let mcp_servers = mcp_servers
                    .into_iter()
                    .map(|server| {
                        let server = server.with_known_credentials(
                            provider_known_credential.clone().into_iter().collect(),
                        );
                        match (
                            provider_known_credential.as_ref(),
                            &provider_credential_reader,
                        ) {
                            (Some(_), Some(reader)) => {
                                server.with_known_credentials_reader(reader.clone())
                            }
                            _ => server,
                        }
                    })
                    .collect();
                if cancel.is_cancelled() {
                    return Err(());
                }
                runtime.block_on(
                    vega_conversation::agent::run_thread_task_with_images_reasoning_and_mcp(
                        &store,
                        provider.as_ref(),
                        &tools,
                        &thread.id,
                        &content,
                        &system_prompt,
                        cancel,
                        &permission_queue,
                        event_sink,
                        replay_persistence_config(replay_registry.as_ref(), replay_route_fingerprint)
                            .with_subagent_host(subagent_host.clone())
                            .with_automation_claim(automation_claim.clone())
                            .with_automatic_title(automatic_title)
                            .with_turn_limit(turn_limit)
                            .with_computer_use(computer_use.clone())
                            .with_owner_credential_reader(owner_credential_reader.clone())
                            .with_credential_exceptions(Some(credential_exceptions.clone()))
                            .with_user_questions(Arc::new(user_questions.clone()))
                            .with_pending_input(
                                pending_message_id.clone(),
                                steering_source.clone(),
                            ),
                        None,
                        pricing_catalog,
                        Some(reasoning),
                        submission.images,
                        mcp_servers,
                    ),
                )
            }
            PendingAgentRun::ApprovedPlan(instruction_message_id) => {
                let provider = guard_provider(make_provider()?);
                // local credential storage access is synchronous. A route cancellation while
                // it was waiting must not start a late durable/network run.
                if cancel.is_cancelled() {
                    return Err(());
                }
                let mcp_servers = match &mcp_service {
                    Some(service) => runtime
                        .block_on(async {
                            tokio::select! {
                                biased;
                                _ = cancel.cancelled() => Err(()),
                                readiness = service.ready_for_run() => readiness.map_err(|_| ()),
                            }
                        })
                        .map(|readiness| {
                            if !readiness.unavailable.is_empty() {
                                let _ =
                                    sender.send(AgentUpdate::McpUnavailable(readiness.unavailable));
                            }
                            readiness.ready_servers
                        })?,
                    None => Vec::new(),
                };
                let mcp_servers = mcp_servers
                    .into_iter()
                    .map(|server| {
                        let server = server.with_known_credentials(
                            provider_known_credential.clone().into_iter().collect(),
                        );
                        match (
                            provider_known_credential.as_ref(),
                            &provider_credential_reader,
                        ) {
                            (Some(_), Some(reader)) => {
                                server.with_known_credentials_reader(reader.clone())
                            }
                            _ => server,
                        }
                    })
                    .collect();
                if cancel.is_cancelled() {
                    return Err(());
                }
                runtime.block_on(
                    vega_conversation::agent::run_approved_plan_task_with_pricing_reasoning_and_mcp(
                        &store,
                        provider.as_ref(),
                        &tools,
                        &thread.id,
                        &instruction_message_id,
                        &system_prompt,
                        cancel,
                        &permission_queue,
                        event_sink,
                        pricing_catalog,
                        Some(reasoning),
                        mcp_servers,
                        replay_persistence_config(replay_registry.as_ref(), replay_route_fingerprint)
                            .with_subagent_host(subagent_host.clone())
                            .with_turn_limit(turn_limit)
                            .with_computer_use(computer_use.clone())
                            .with_owner_credential_reader(owner_credential_reader.clone())
                            .with_credential_exceptions(Some(credential_exceptions.clone()))
                            .with_user_questions(Arc::new(user_questions.clone()))
                            .with_pending_input(
                                pending_message_id.clone(),
                                steering_source.clone(),
                            ),
                    ),
                )
            }
            PendingAgentRun::CredentialContinuation(receipt_id) => {
                let provider = guard_provider(make_provider()?);
                if cancel.is_cancelled() {
                    return Err(());
                }
                let mcp_servers = match &mcp_service {
                    Some(service) => runtime
                        .block_on(async {
                            tokio::select! {
                                biased;
                                _ = cancel.cancelled() => Err(()),
                                readiness = service.ready_for_run() => readiness.map_err(|_| ()),
                            }
                        })
                        .map(|readiness| {
                            if !readiness.unavailable.is_empty() {
                                let _ =
                                    sender.send(AgentUpdate::McpUnavailable(readiness.unavailable));
                            }
                            readiness.ready_servers
                        })?,
                    None => Vec::new(),
                };
                let mcp_servers = mcp_servers
                    .into_iter()
                    .map(|server| {
                        let server = server.with_known_credentials(
                            provider_known_credential.clone().into_iter().collect(),
                        );
                        match (
                            provider_known_credential.as_ref(),
                            &provider_credential_reader,
                        ) {
                            (Some(_), Some(reader)) => {
                                server.with_known_credentials_reader(reader.clone())
                            }
                            _ => server,
                        }
                    })
                    .collect();
                if cancel.is_cancelled() {
                    return Err(());
                }
                runtime.block_on(
                    vega_conversation::agent::resume_credential_continuation_with_mcp(
                        &store,
                        provider.as_ref(),
                        &tools,
                        &thread.id,
                        &receipt_id,
                        &system_prompt,
                        cancel,
                        &permission_queue,
                        event_sink,
                        replay_persistence_config(replay_registry.as_ref(), replay_route_fingerprint)
                            .with_subagent_host(subagent_host.clone())
                            .with_turn_limit(turn_limit)
                            .with_computer_use(computer_use.clone())
                            .with_owner_credential_reader(owner_credential_reader.clone())
                            .with_credential_exceptions(Some(credential_exceptions.clone()))
                            .with_user_questions(Arc::new(user_questions.clone()))
                            .with_pending_input(
                                pending_message_id.clone(),
                                steering_source.clone(),
                            ),
                        pricing_catalog,
                        Some(reasoning),
                        mcp_servers,
                    ),
                )
            }
        };
        if let Some(run) = &computer_use {
            runtime.block_on(run.shutdown());
        }
        Ok(result.is_ok_and(|run| !run.failed))
    })()
    .unwrap_or(false);
    if let Ok(store) = Store::open(automation_path) {
        if let Some(claim) = &automation_claim {
            let _ = vega_conversation::automation::finish_unsettled_claim(
                &store,
                claim,
                vega_conversation::automation::now_ms(),
            );
        }
        if !success {
            let _ = vega_conversation::automation::pause_matching(
                &store,
                &automation_thread,
                &automation_snapshot,
                "本轮未能完成，请检查会话后恢复",
            );
        }
    }
    if let Some(run) = &computer_use {
        run.control.stop();
    }
    let _ = sender.send(AgentUpdate::Finished {
        success,
        reference_failure,
        credential_failure,
    });
}

pub(crate) fn automation_trigger(claim: &AutomationRunClaim) -> &'static str {
    if claim.source == Some(AutomationKind::Loop) {
        "执行已设置的循环任务。"
    } else {
        "继续推进已设置的持续目标。"
    }
}

#[cfg(test)]
mod reasoning_replay_tests {
    use super::provider_replay_route_fingerprint;

    #[test]
    fn automatic_replay_provider_route_fence_hashes_exact_url_and_owner_key() {
        let root = tempfile::tempdir().unwrap();
        vega_store::keystore::set_key(root.path(), "owned", "credential-one").unwrap();
        let database = root.path().join("database.sqlite");
        let mut provider = vega_store::config::ProviderConfig {
            api: vega_store::config::ProviderApi::ChatCompletions,
            enabled: true,
            name: "owned".into(),
            base_url: "https://user:password@example.test/v1?token=query-secret".into(),
            models: vec!["fixture-model".into()],
            key_ref: "owned".into(),
        };
        let first_credential = vega_store::keystore::get_key(root.path(), "owned").unwrap();
        let first = provider_replay_route_fingerprint(
            &provider,
            root.path(),
            &first_credential,
            &database,
            "project-one",
        );
        provider.base_url = "https://user:password@example.test/v1?token=other".into();
        assert_ne!(
            first,
            provider_replay_route_fingerprint(
                &provider,
                root.path(),
                &first_credential,
                &database,
                "project-one",
            )
        );
        provider.base_url = "https://user:password@example.test/v1?token=query-secret".into();
        let revision = vega_store::keystore::revision(root.path());
        std::fs::write(
            root.path().join("credentials/credentials.toml"),
            "[keys]\nowned = 'credential-two'\n",
        )
        .unwrap();
        let second_credential = vega_store::keystore::get_key(root.path(), "owned").unwrap();
        assert_eq!(vega_store::keystore::revision(root.path()), revision);
        assert_ne!(
            first,
            provider_replay_route_fingerprint(
                &provider,
                root.path(),
                &second_credential,
                &database,
                "project-one",
            )
        );
        assert_ne!(
            first,
            provider_replay_route_fingerprint(
                &provider,
                root.path(),
                &first_credential,
                &root.path().join("other-database.sqlite"),
                "project-one",
            )
        );
        assert_ne!(
            first,
            provider_replay_route_fingerprint(
                &provider,
                root.path(),
                &first_credential,
                &database,
                "project-two",
            )
        );
        let rendered = format!("{first:?}");
        for secret in ["user", "password", "query-secret", "credential-one"] {
            assert!(!rendered.contains(secret));
        }
    }
}
