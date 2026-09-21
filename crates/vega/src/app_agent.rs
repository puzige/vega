use std::collections::HashMap;
#[cfg(test)]
use std::sync::atomic::*;
use std::sync::*;
use std::time::*;

use gpui_kit::*;
use vega_conversation::types::*;
use vega_conversation::*;
use vega_store::Store;
use vega_ui::conversation_stream::*;
use vega_ui::plan_card::PlanReviewRequested;

type CredentialReader = Arc<dyn Fn() -> Result<Vec<String>, ()> + Send + Sync>;

pub(crate) const AGENT_EVENT_POLL: Duration = Duration::from_millis(4);
pub(crate) const AGENT_EVENT_CAPACITY: usize = 256;
pub(crate) const AGENT_EVENT_BATCH: usize = 128;

/// One app-owned revocation domain shared by Settings and every run worker.
/// A Settings view must clone this service, never construct an independent
/// registry against the same database.
pub(crate) struct AppMcpSettings(pub(crate) Option<vega_conversation::McpServerSettingsService>);

impl Global for AppMcpSettings {}

#[cfg(test)]
#[derive(Default)]
pub(crate) struct AgentWorkerStartProbe {
    starts: AtomicUsize,
    /// One bounded test-only delay at the existing MockProvider construction boundary.
    pub(crate) provider_construction_gate:
        Mutex<Option<(mpsc::SyncSender<()>, mpsc::Receiver<()>)>>,
}

#[cfg(test)]
impl AgentWorkerStartProbe {
    pub(crate) fn load(&self) -> usize {
        self.starts.load(Ordering::SeqCst)
    }

    fn record(&self) {
        self.starts.fetch_add(1, Ordering::SeqCst);
    }
}

pub(crate) const SYSTEM_PROMPT: &str =
    "You are Vega, a careful coding agent working inside the selected project.";

pub(crate) enum PendingAgentRun {
    UserMessage(UserSubmission),
    ApprovedPlan(String),
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
    Event(vega_conversation::types::ConversationEvent),
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
    pub(crate) events: Vec<vega_conversation::types::ConversationEvent>,
    pub(crate) mcp_unavailable: Vec<vega_conversation::types::McpServerDiagnostic>,
    pub(crate) reference_failure: Option<FileReferenceFailureCode>,
    pub(crate) credential_failure: bool,
    pub(crate) finished: Option<bool>,
}

pub(crate) fn drain_agent_updates(receiver: &mpsc::Receiver<AgentUpdate>) -> AgentBatch {
    let mut events = Vec::new();
    let mut mcp_unavailable = Vec::new();
    let mut reference_failure = None;
    let mut credential_failure = false;
    let mut finished = None;
    for _ in 0..AGENT_EVENT_BATCH {
        match receiver.try_recv() {
            Ok(AgentUpdate::Event(event)) => events.push(event),
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
        events,
        mcp_unavailable,
        reference_failure,
        credential_failure,
        finished,
    }
}

pub(crate) struct ActiveAgentRun {
    pub(crate) generation: u64,
    pub(crate) thread_id: String,
    pub(crate) stream: Entity<ConversationStream>,
    pub(crate) cancel: tokio_util::sync::CancellationToken,
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
    pub(crate) file_read_states: HashMap<String, vega_tools::ReadState>,
    pub(crate) next_generation: u64,
    pub(crate) active: HashMap<String, ActiveAgentRun>,
    pub(crate) pending_review: HashMap<String, PendingPlanReview>,
    pub(crate) preparation_stream: Option<Entity<ConversationStream>>,
}

impl AppAgentController {
    pub(crate) fn active_stream_for_thread(
        &self,
        thread_id: &str,
    ) -> Option<Entity<ConversationStream>> {
        self.active
            .get(thread_id)
            .map(|active| active.stream.clone())
    }

    pub(crate) fn request_active_cancel(&self, thread_id: &str) {
        if let Some(active) = self.active.get(thread_id) {
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
        if self.active.contains_key(&thread_id) {
            return None;
        }
        self.next_generation = self.next_generation.checked_add(1)?;
        let generation = self.next_generation;
        let cancel = tokio_util::sync::CancellationToken::new();
        self.active.insert(
            thread_id.clone(),
            ActiveAgentRun {
                generation,
                thread_id,
                stream,
                cancel: cancel.clone(),
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
        let (message_id, failure) = match event {
            ConversationEvent::MessageFinished { message_id, .. }
            | ConversationEvent::Interrupted { message_id } => (Some(message_id), None),
            ConversationEvent::Error { message_id, error } => (
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
        self.active.remove(thread_id)
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
    model: &str,
    reasoning_path: Option<&std::path::Path>,
) -> Result<vega_runtime::FrozenReasoning, ()> {
    let Some(reasoning_path) = reasoning_path else {
        return Ok(vega_runtime::FrozenReasoning::unknown(provider, model));
    };
    let config = vega_store::reasoning::read_from(reasoning_path).map_err(|_| ())?;
    let Some(profile) = config.profile(provider, model) else {
        return Ok(vega_runtime::FrozenReasoning::unknown(provider, model));
    };
    let projection = ReasoningProfileProjection::from_store(profile).map_err(|_| ())?;
    projection.freeze().map_err(|_| ())
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
    let provider = vega_runtime::OpenAiProvider::new(provider.base_url, key).map_err(|_| ())?;
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
        cancel,
        sender,
        pricing_catalog,
        config_path,
        reasoning,
        title_notifications,
        None,
        vega_tools::ReadState::default(),
        provider_override,
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
    #[cfg(test)] provider_override: Option<Arc<dyn vega_runtime::Provider>>,
    #[cfg(test)] worker_start_probe: Arc<AgentWorkerStartProbe>,
) {
    #[cfg(test)]
    worker_start_probe.record();
    let mut reference_failure = None;
    let mut credential_failure = false;
    let success = (|| -> Result<bool, ()> {
        // Config and local credential storage are touched only after an explicit user submit
        // or committed Plan approval reaches this worker.
        let tools = vega_tools::Tools::new(&project_path)
            .map_err(|_| ())?
            .with_read_state(file_read_state);
        let store = Store::open(database_path).map_err(|_| ())?;
        store.migrate().map_err(|_| ())?;
        // Resolve provider identity once, before constructing the provider and
        // before entering the runtime. The resulting FrozenReasoning is moved
        // into the selected entry point and is never re-read during retries
        // or tool rounds.
        let config = config_path
            .as_deref()
            .and_then(|path| vega_store::config::read_from(path).ok());
        let configured_provider = config
            .as_ref()
            .and_then(|config| unique_provider_for_model(config, &thread.model));
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
        let reasoning = match reasoning {
            Some(reasoning) => {
                reasoning.validate().map_err(|_| ())?;
                if reasoning.model != thread.model {
                    return Err(());
                }
                if configured_provider
                    .as_ref()
                    .is_none_or(|provider| reasoning.provider != provider.name)
                {
                    return Err(());
                }
                reasoning
            }
            None => {
                let reasoning_path = config_path
                    .as_deref()
                    .map(|path| path.with_file_name(vega_store::reasoning::REASONING_FILE_NAME));
                reasoning_for_provider_model(
                    provider_name,
                    &thread.model,
                    reasoning_path.as_deref(),
                )?
            }
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
        let owner = mcp_service.clone();
        let selected_provider_reader = provider_credential_reader.clone();
        let owner_credential_reader: CredentialReader = Arc::new(move || {
            let mut known = match &owner {
                Some(service) => service.current_owner_credential_values().map_err(|_| ())?,
                None => Vec::new(),
            };
            if let Some(reader) = &selected_provider_reader {
                known.extend(reader()?);
            }
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
                    let _ = entered.send(());
                    let _ = release.recv_timeout(Duration::from_secs(5));
                }
                if let Some(provider) = provider_override.clone() {
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
            provider_known_credential = Some(key.clone());
            let provider = vega_runtime::OpenAiProvider::new(provider.base_url, key)
                .map_err(|_| ())?
                .with_pre_attempt_guard(
                    vega_conversation::agent::OwnerCredentialProvider::pre_attempt_guard(
                        owner_credential_reader.clone(),
                    ),
                );
            Ok(Arc::new(provider) as Arc<dyn vega_runtime::Provider>)
        };
        let guard_provider = |provider: Arc<dyn vega_runtime::Provider>| {
            Arc::new(vega_conversation::agent::OwnerCredentialProvider::new(
                provider,
                owner_credential_reader.clone(),
            )) as Arc<dyn vega_runtime::Provider>
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
        let result =
            match run {
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
                    Some(service) => runtime.block_on(async {
                        tokio::select! {
                            biased;
                            _ = cancel.cancelled() => Err(()),
                            readiness = service.ready_for_run() => readiness.map_err(|_| ()),
                        }
                    }).map(|readiness| {
                        if !readiness.unavailable.is_empty() {
                            let _ = sender.send(AgentUpdate::McpUnavailable(readiness.unavailable));
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
                            SYSTEM_PROMPT,
                            cancel,
                            &permission_queue,
                            event_sink,
                            vega_conversation::agent::PersistenceActorConfig::default()
                                .with_automatic_title(automatic_title),
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
                    Some(service) => runtime.block_on(async {
                        tokio::select! {
                            biased;
                            _ = cancel.cancelled() => Err(()),
                            readiness = service.ready_for_run() => readiness.map_err(|_| ()),
                        }
                    }).map(|readiness| {
                        if !readiness.unavailable.is_empty() {
                            let _ = sender.send(AgentUpdate::McpUnavailable(readiness.unavailable));
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
                        SYSTEM_PROMPT,
                        cancel,
                        &permission_queue,
                        event_sink,
                        pricing_catalog,
                        Some(reasoning),
                        mcp_servers,
                    ),
                )
                }
            };
        Ok(result.is_ok_and(|run| !run.failed))
    })()
    .unwrap_or(false);
    let _ = sender.send(AgentUpdate::Finished {
        success,
        reference_failure,
        credential_failure,
    });
}
