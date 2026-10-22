#[cfg(test)]
mod continuation_tests;

use std::collections::{HashMap, HashSet};
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
    ComposerPredictionReady(ComposerPredictionRequest),
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
    pub(crate) composer_prediction: Option<ComposerPredictionRequest>,
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
    let mut composer_prediction = None;
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
            Ok(AgentUpdate::ComposerPredictionReady(request)) => {
                composer_prediction = Some(request)
            }
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
        composer_prediction,
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

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RunCancellationReason {
    User,
    ReviewHandoff,
}

#[derive(Clone, Copy)]
pub(crate) enum RunCompletionOutcome {
    Completed,
    Failed,
    Interrupted,
}

pub(crate) struct ActiveAgentRun {
    pub(crate) composer_prediction_config_generation: u64,
    pub(crate) composer_prediction: Option<ComposerPredictionRequest>,
    pub(crate) root_message_id: Option<String>,
    pub(crate) message_ids: Vec<String>,
    pub(crate) cancellation_reason: Option<RunCancellationReason>,
    pub(crate) completion_outcome: Option<RunCompletionOutcome>,
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
    pub(crate) started: Instant,
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

type SubagentReleaseReply = tokio::sync::oneshot::Sender<Result<(), vega_runtime::VegaError>>;

#[derive(Default)]
pub(crate) struct AppAgentController {
    pub(crate) delegated_unbound: HashSet<String>,
    pub(crate) queue_streams: HashMap<String, Entity<ConversationStream>>,
    pub(crate) queue_dispatches: HashMap<String, String>,
    pub(crate) queue_continue: HashMap<String, bool>,
    pub(crate) queue_delivered: HashSet<String>,
    pub(crate) queue_operations: HashMap<String, u64>,
    pub(crate) subagent_releases: HashMap<String, Vec<SubagentReleaseReply>>,
    pub(crate) delegated_latest: HashMap<String, String>,
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
        if let Some(active) = self.active.get_mut(&request.thread_id) {
            active.cancellation_reason = Some(RunCancellationReason::ReviewHandoff);
        }
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
                composer_prediction_config_generation: 0,
                composer_prediction: None,
                root_message_id: None,
                message_ids: Vec::new(),
                cancellation_reason: None,
                completion_outcome: None,
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
                if !active.message_ids.contains(message_id) {
                    active.message_ids.push(message_id.clone());
                }
                active.completion_outcome = None;
            }
            match event {
                ConversationEvent::MessageStarted { .. } => active.queue_ready = false,
                ConversationEvent::Interrupted { .. } => {
                    active.queue_ready = false;
                    active.completion_outcome = Some(RunCompletionOutcome::Interrupted);
                }
                ConversationEvent::Error { .. } => {
                    active.queue_ready = false;
                    active.completion_outcome = Some(RunCompletionOutcome::Failed);
                }
                ConversationEvent::MessageFinished { stop_reason, .. } => {
                    active.queue_ready = *stop_reason == ConversationStopReason::End;
                    active.completion_outcome = Some(match stop_reason {
                        ConversationStopReason::End => RunCompletionOutcome::Completed,
                        ConversationStopReason::Length | ConversationStopReason::TurnLimit => {
                            RunCompletionOutcome::Interrupted
                        }
                    });
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

fn composer_prediction_authority(
    path: Option<std::path::PathBuf>,
    provider: Option<vega_store::config::ProviderConfig>,
    model: String,
) -> Arc<dyn Fn() -> bool + Send + Sync> {
    let frozen_key = path
        .as_deref()
        .and_then(std::path::Path::parent)
        .zip(provider.as_ref())
        .and_then(|(root, provider)| vega_store::keystore::get_key(root, &provider.key_ref).ok());
    Arc::new(move || {
        let (Some(path), Some(provider), Some(key)) = (&path, &provider, &frozen_key) else {
            return path.is_none() && provider.is_none();
        };
        vega_store::config::read_from(path).is_ok_and(|config| {
            config.agent.composer_predictions_enabled
                && unique_provider_for_model(&config, &model).as_ref() == Some(provider)
                && path.parent().is_some_and(|root| {
                    vega_store::keystore::get_key(root, &provider.key_ref)
                        .is_ok_and(|current| current == *key)
                })
        })
    })
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
    let identity = vega_runtime::model_compat::ModelIdentity {
        provider: provider.into(),
        model: model.into(),
    };
    let definition = vega_runtime::model_compat::lookup_model_definition(
        &vega_runtime::model_compat::ModelRoute::new(
            vega_conversation::model_capabilities::api(api),
            base_url,
        )
        .map_err(|_| ())?,
        &identity,
    )
    .map_err(|_| ())?;
    let capabilities = match reasoning_path {
        Some(path) => vega_store::model_capabilities::read_from(
            &path.with_file_name(vega_store::model_capabilities::MODEL_CAPABILITY_FILE_NAME),
        )
        .map_err(|_| ())?,
        None => Default::default(),
    };
    let user_override = capabilities.record(provider, model).map(|record| {
        vega_runtime::model_compat::ModelCapabilityOverride {
            identity,
            allowed_efforts: record.allowed_efforts.clone(),
            image_input: record.image_input,
        }
    });
    let model = vega_runtime::model_compat::resolve_model(&definition, user_override.as_ref())
        .map_err(|_| ())?;
    let resolved = ReasoningProfileProjection::from_resolved_model(
        &model,
        configured.map_or(vega_runtime::ReasoningChoice::ProviderDefault, |profile| {
            profile.preference
        }),
    );
    crate::reasoning_capabilities::freeze_reasoning_profile(resolved).map_err(|_| ())
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
    let model_snapshot = vega_conversation::model_capabilities::load_configured_model(
        &config,
        path,
        &provider.name,
        &thread.model,
    )
    .map_err(|_| ())?;
    let provider = vega_runtime::OpenAiProvider::new(provider.base_url, key)
        .map(|transport| {
            transport.with_responses_api(
                provider.api == vega_conversation::types::ProviderApi::Responses,
            )
        })
        .map_err(|_| ())?;
    let provider = provider
        .with_model_snapshot(model_snapshot)
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
pub(crate) fn run_agent_worker_with_mcp(
    database_path: std::path::PathBuf,
    project_path: std::path::PathBuf,
    thread: Thread,
    run: PendingAgentRun,
    permission_queue: vega_conversation::agent::PermissionQueue,
    user_questions: vega_conversation::agent::UserQuestionQueue,
    cancel: tokio_util::sync::CancellationToken,
    sender: mpsc::SyncSender<AgentUpdate>,

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
    replay_registry: Option<vega_conversation::agent::ReplaySessionRegistry>,
) {
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
        if config_path.is_some() && configured_provider.is_none() {
            return Err(());
        }
        let reasoning_path = config_path
            .as_deref()
            .map(|path| path.with_file_name(vega_store::reasoning::REASONING_FILE_NAME));
        let reasoning = match (reasoning, configured_provider.as_ref()) {
            (Some(reasoning), Some(provider)) => {
                if reasoning.model != thread.model || reasoning.provider != provider.name {
                    return Err(());
                }
                let path = config_path.as_deref().ok_or(())?;
                let config = vega_store::config::read_from(path).map_err(|_| ())?;
                let model = vega_conversation::model_capabilities::load_configured_model(
                    &config,
                    path,
                    &provider.name,
                    &thread.model,
                )
                .map_err(|_| ())?;
                vega_runtime::model_compat::freeze_choice(&model, reasoning.choice)
                    .map_err(|_| ())?
            }
            (None, Some(provider)) => reasoning_for_provider_model(
                &provider.name,
                provider.api,
                &provider.base_url,
                &thread.model,
                reasoning_path.as_deref(),
            )?,
            (_, None) => return Err(()),
        };

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
        let owner_credential_reader: CredentialReader = Arc::new(move || {
            if let Some(root) = &authority_root {
                vega_conversation::privacy::refresh_authority(&authority_database, root).map_err(|_| ())?;
            }
            let mut known = match &owner {
                Some(service) => service.current_owner_credential_values().map_err(|_| ())?,
                None => Vec::new(),
            };
            if let Some(reader) = &selected_provider_reader {
                known.extend(reader()?);
            }
            if let Some(reader) = &broker_credential_reader {
                known.extend(reader()?);
            }
            Ok(known)
        });
        let mut make_provider = || -> Result<Arc<dyn vega_runtime::Provider>, ()> {
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
            let model_snapshot = vega_conversation::model_capabilities::load_configured_model(
                &vega_store::config::read_from(config_path.as_deref().ok_or(())?).map_err(|_| ())?,
                config_path.as_deref().ok_or(())?,
                &provider.name,
                &thread.model,
            )
            .map_err(|_| ())?;
            let provider = vega_runtime::OpenAiProvider::new(provider.base_url, key)
                .map(|transport| {
                    transport.with_responses_api(
                        provider.api == vega_conversation::types::ProviderApi::Responses,
                    )
                })
                .map_err(|_| ())?
                .with_model_snapshot(model_snapshot)
                .map_err(|_| ())?
                .with_pre_attempt_guard(
                    vega_conversation::agent::OwnerCredentialProvider::pre_attempt_guard_with_exceptions(
                        owner_credential_reader.clone(),
                        credential_exceptions.clone(),
                    ),
                );
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
        let event_sink = move |event: &vega_conversation::types::ConversationEvent| {
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
        let prediction_parent_cancel = cancel.clone();
        let mut composer_prediction = None;
        let result = match run {
            PendingAgentRun::Automation(_) => return Err(()),
            PendingAgentRun::UserMessage(submission) => {
                let content = submission.content;

                let title_source = content.clone();

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
                if automation_claim.is_none()
                    && thread.parent_thread_id.is_none()
                    && thread.backend == TaskBackend::Native
                    && thread.mode != ThreadMode::Plan
                    && config.as_ref().is_none_or(|config| config.agent.composer_predictions_enabled)
                {
                    composer_prediction = Some(
                        ComposerPredictionRequest::new(provider.clone(), thread.model.clone(), thread.mode)
                            .with_authority_guard(composer_prediction_authority(
                                config_path.clone(), configured_provider.clone(), thread.model.clone(),
                            )),
                    );
                }
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
        let success = result.is_ok_and(|run| !run.failed);
        if success && !prediction_parent_cancel.is_cancelled()
            && let Some(request) = composer_prediction
        {
            let _ = sender.send(AgentUpdate::ComposerPredictionReady(request));
        }
        Ok(success)
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
