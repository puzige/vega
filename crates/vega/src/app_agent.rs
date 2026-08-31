#[allow(unused_imports)]
use std::collections::{HashMap, VecDeque};
#[allow(unused_imports)]
use std::path::PathBuf;
#[allow(unused_imports)]
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
#[allow(unused_imports)]
use std::time::{Duration, Instant};

#[allow(unused_imports)]
use gpui::prelude::*;
#[allow(unused_imports)]
use gpui::{
    AnyElement, App, Bounds, Entity, Focusable, KeyBinding, TitlebarOptions, Window, WindowBounds,
    WindowOptions, actions, div, px, size,
};
#[allow(unused_imports)]
use gpui_platform::application;
#[allow(unused_imports)]
use vega_conversation::history::HistoryPage;
#[allow(unused_imports)]
use vega_conversation::types::{
    ArtifactCard as ArtifactProjection, ArtifactCardId, ArtifactPreviewProjection, BranchId,
    BranchSnapshot, BranchSwitchCompletion, BranchSwitchOutcome, CommitChecklist, CommitCompletion,
    CommitErrorCode, CommitOutcome, CommitPrepareCompletion, ConversationEvent, DiffTextProjection,
    GitWorkspaceErrorCode, OpenInOutcome, OpenInTarget, Plan, PlanReviewOutcome,
    PricingDraftReason, PricingNotice, PricingSettingsErrorCode, PricingSettingsProjection, Thread,
    ToolCall, WorkspaceFileId, WorkspaceSnapshot,
};
#[allow(unused_imports)]
use vega_conversation::{
    ArtifactCaptureCandidate, ArtifactService, BranchSwitchPermit, BranchWorkspaceService,
    GitWorkspaceService, PricingAuthority, PricingLoadOutcome, PricingSaveOutcome, PricingSavePlan,
    PricingSettingsService, TrustedGitService,
};
#[allow(unused_imports)]
use vega_store::Store;
#[allow(unused_imports)]
use vega_theme::{Theme, ThemeColors, Typography, theme};
#[allow(unused_imports)]
use vega_ui::artifact_card::{
    ArtifactCard, ArtifactCleared, ArtifactOpenRequested, ArtifactPreviewRequested,
};
#[allow(unused_imports)]
use vega_ui::branch_selector::{
    BranchListRequested, BranchOperationId, BranchSelector, BranchSelectorClosed,
    BranchSwitchRequested,
};
#[allow(unused_imports)]
use vega_ui::commit_panel::{
    CommitDraftRequested, CommitOperationId, CommitPanel, CommitPanelClosed,
    CommitPrepareRequested, CommitRequested,
};
#[allow(unused_imports)]
use vega_ui::conversation_stream::{
    ComposerDefaultsRequested, ComposerSubmitted, ConversationStream, HistoryPageRequested,
    OpenCommitPanelRequested, OpenWorkspaceDiffRequested, ThreadSettingsRequested,
    WorkspaceToolTerminal, bench as render_frame_bench,
};
#[allow(unused_imports)]
use vega_ui::diff_view::{
    DIFF_REFRESH_INTERVAL, DiffClosed, DiffProjectionRequested, DiffRetryRequested, DiffView,
};
#[allow(unused_imports)]
use vega_ui::plan_card::PlanReviewRequested;
#[allow(unused_imports)]
use vega_ui::settings::{
    CloseSettings, OpenSettings, PricingDiscardRequested, PricingMutationRequested,
    PricingReloadRequested, PricingRetryRequested, SettingsOpen, SettingsView, all_models,
};
#[allow(unused_imports)]
use vega_ui::sidebar::{
    AUTO_COLLAPSE_WIDTH, CONTENT_MAX_WIDTH, CONTENT_MIN_PADDING, NewThread, OpenedThread,
    PendingDeleteConfirm, Sidebar, SidebarCollapsed, ToggleSidebar, VegaStore, load_collapsed,
    render_delete_confirm_overlay, toggle_persisted,
};

#[allow(unused_imports)]
use crate::artifact_controller::*;
#[allow(unused_imports)]
use crate::branch_controller::*;
#[allow(unused_imports)]
use crate::commit_controller::*;
#[allow(unused_imports)]
use crate::diff_controller::*;
#[allow(unused_imports)]
use crate::pricing_controller::*;
#[allow(unused_imports)]
use crate::thread_reload::*;
#[allow(unused_imports)]
use crate::trusted_action::*;
#[allow(unused_imports)]
use crate::window::*;

pub(crate) const AGENT_EVENT_POLL: Duration = Duration::from_millis(4);
pub(crate) const AGENT_EVENT_CAPACITY: usize = 256;
pub(crate) const AGENT_EVENT_BATCH: usize = 128;

#[cfg(test)]
pub(crate) static AGENT_WORKER_STARTS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

pub(crate) const SYSTEM_PROMPT: &str =
    "You are Vega, a careful coding agent working inside the selected project.";

pub(crate) struct UnavailableProvider;

impl vega_runtime::Provider for UnavailableProvider {
    fn chat_stream(
        &self,
        _: vega_runtime::ChatRequest,
        _: tokio_util::sync::CancellationToken,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<vega_runtime::EventStream, vega_runtime::VegaError>,
                > + Send,
        >,
    > {
        Box::pin(async {
            Err(vega_runtime::VegaError::Provider {
                status: None,
                message: "provider unavailable".into(),
                retryable: false,
            })
        })
    }
}

pub(crate) enum PendingAgentRun {
    UserMessage(String),
    ApprovedPlan(String),
}

pub(crate) enum AgentUpdate {
    Event(vega_conversation::types::ConversationEvent),
    Finished(bool),
}

pub(crate) struct AgentBatch {
    pub(crate) events: Vec<vega_conversation::types::ConversationEvent>,
    pub(crate) finished: Option<bool>,
}

pub(crate) fn drain_agent_updates(receiver: &mpsc::Receiver<AgentUpdate>) -> AgentBatch {
    let mut events = Vec::new();
    let mut finished = None;
    for _ in 0..AGENT_EVENT_BATCH {
        match receiver.try_recv() {
            Ok(AgentUpdate::Event(event)) => events.push(event),
            Ok(AgentUpdate::Finished(success)) => {
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
    AgentBatch { events, finished }
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
}

pub(crate) enum AgentBatchIngress {
    Stale,
    Running,
    Finished { success: bool, run: ActiveAgentRun },
}

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

    pub(crate) fn select_exact(
        &self,
        model: &str,
    ) -> Result<PricingAuthority, PricingSettingsErrorCode> {
        let PricingControllerState::Ready { authority, .. } = &self.state else {
            return Err(match self.state {
                PricingControllerState::Invalid(code) => code,
                _ => PricingSettingsErrorCode::Busy,
            });
        };
        if authority.contains_exact_model(model) {
            Ok(authority.clone())
        } else {
            Err(PricingSettingsErrorCode::ModelNotPriced)
        }
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
    pub(crate) next_generation: u64,
    pub(crate) active: Option<ActiveAgentRun>,
    pub(crate) pending_review: Option<PendingPlanReview>,
}

impl AppAgentController {
    pub(crate) fn request_active_cancel(&self) {
        if let Some(active) = &self.active {
            active.cancel.cancel();
        }
    }

    pub(crate) fn queue_review(
        &mut self,
        stream: &Entity<ConversationStream>,
        request: &PlanReviewRequested,
    ) -> bool {
        // The caller already proved `stream` and `request.thread_id` own the
        // current cache. Any older active run may be cancelled first; the
        // review is persisted only after that worker reaches Finished.
        if self.active.is_none() || self.pending_review.is_some() {
            return false;
        }
        self.pending_review = Some(PendingPlanReview {
            stream: stream.clone(),
            request: request.clone(),
        });
        self.request_active_cancel();
        true
    }

    pub(crate) fn begin(
        &mut self,
        thread_id: String,
        stream: Entity<ConversationStream>,
        pending_user_content: Option<String>,
        pending_approved_instruction: Option<String>,
    ) -> (u64, tokio_util::sync::CancellationToken) {
        self.next_generation = self.next_generation.wrapping_add(1).max(1);
        let generation = self.next_generation;
        let cancel = tokio_util::sync::CancellationToken::new();
        self.active = Some(ActiveAgentRun {
            generation,
            thread_id,
            stream,
            cancel: cancel.clone(),
            pending_user_content,
            pending_approved_instruction,
            started: Instant::now(),
            terminal_message_id: None,
        });
        (generation, cancel)
    }

    pub(crate) fn matches(
        &self,
        generation: u64,
        thread_id: &str,
        stream: &Entity<ConversationStream>,
    ) -> bool {
        self.active.as_ref().is_some_and(|active| {
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
        let active = self.active.as_mut()?;
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
        let message_id = match event {
            ConversationEvent::MessageFinished { message_id, .. } => message_id,
            ConversationEvent::Interrupted { message_id } => message_id,
            ConversationEvent::Error {
                message_id: Some(message_id),
                ..
            } => message_id,
            _ => return,
        };
        if !self.matches(generation, thread_id, stream) {
            return;
        }
        if let Some(active) = self.active.as_mut() {
            active.terminal_message_id = Some(message_id.clone());
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
        self.active.take()
    }
}

pub(crate) fn unique_provider_for_model(
    config: &vega_store::config::AppConfig,
    model: &str,
) -> Option<vega_store::config::ProviderConfig> {
    let mut matches = config
        .providers
        .iter()
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

pub(crate) fn commit_provider(thread: &Thread) -> Arc<dyn vega_runtime::Provider> {
    vega_store::config::load()
        .ok()
        .and_then(|config| unique_provider_for_model(&config, &thread.model))
        .and_then(|provider| {
            vega_store::keystore::get_key(&provider.key_ref)
                .ok()
                .filter(|key| !key.is_empty())
                .and_then(|key| vega_runtime::OpenAiProvider::new(provider.base_url, key).ok())
        })
        .map(|provider| {
            Arc::new(provider.with_retry_policy(commit_retry_policy()))
                as Arc<dyn vega_runtime::Provider>
        })
        .unwrap_or_else(|| Arc::new(UnavailableProvider))
}

pub(crate) fn commit_retry_policy() -> vega_runtime::RetryPolicy {
    vega_runtime::RetryPolicy {
        max_retries: 0,
        ..vega_runtime::RetryPolicy::default()
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_agent_worker(
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
    #[cfg(test)] provider_override: Option<Arc<dyn vega_runtime::Provider>>,
) {
    #[cfg(test)]
    AGENT_WORKER_STARTS.fetch_add(1, Ordering::SeqCst);
    let success = (|| -> Result<(), ()> {
        // Config and Keychain are touched only after an explicit user submit
        // or committed Plan approval reaches this worker.
        let tools = vega_tools::Tools::new(&project_path).map_err(|_| ())?;
        let store = Store::open(database_path).map_err(|_| ())?;
        store.migrate().map_err(|_| ())?;
        #[cfg(test)]
        let provider = provider_override.unwrap_or_else(|| Arc::new(UnavailableProvider));
        #[cfg(not(test))]
        let provider: Arc<dyn vega_runtime::Provider> = vega_store::config::load()
            .ok()
            .and_then(|config| unique_provider_for_model(&config, &thread.model))
            .and_then(|provider| {
                vega_store::keystore::get_key(&provider.key_ref)
                    .ok()
                    .filter(|key| !key.is_empty())
                    .and_then(|key| vega_runtime::OpenAiProvider::new(provider.base_url, key).ok())
            })
            .map_or_else(
                || Arc::new(UnavailableProvider) as Arc<dyn vega_runtime::Provider>,
                |provider| Arc::new(provider) as Arc<dyn vega_runtime::Provider>,
            );
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
        let result = match run {
            PendingAgentRun::UserMessage(content) => {
                // A2-12: resolve `@path` tokens against the project root and
                // inject the referenced file contents ahead of the user text
                // (bounded: 8 files, 16 KiB each, 48 KiB total). Any
                // resolution failure degrades to the raw message — injection
                // never blocks or fails a run.
                let content = vega_tools::reference::resolve_bounded_references(
                    &project_path,
                    &content,
                    vega_tools::reference::REFERENCE_MAX_FILES,
                    vega_tools::reference::REFERENCE_MAX_FILE_BYTES,
                    vega_tools::reference::REFERENCE_MAX_TOTAL_BYTES,
                )
                .map(|refs| {
                    if refs.is_empty() {
                        content.clone()
                    } else {
                        format!(
                            "{}\n\n{}",
                            vega_tools::reference::render_reference_block(&refs),
                            content
                        )
                    }
                })
                .unwrap_or(content);
                runtime.block_on(vega_conversation::agent::run_thread_task_with_pricing(
                    &store,
                    provider.as_ref(),
                    &tools,
                    &thread.id,
                    &content,
                    SYSTEM_PROMPT,
                    cancel,
                    &permission_queue,
                    event_sink,
                    vega_conversation::agent::PersistenceActorConfig::default(),
                    None,
                    pricing_catalog,
                ))
            }
            PendingAgentRun::ApprovedPlan(instruction_message_id) => runtime.block_on(
                vega_conversation::agent::run_approved_plan_task_with_pricing(
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
                ),
            ),
        };
        result.map(|_| ()).map_err(|_| ())
    })()
    .is_ok();
    let _ = sender.send(AgentUpdate::Finished(success));
}
