//! Vega application entry point: boots the GPUI app and opens the main window.
//! The hidden `--vega-bench-render <out.json>` flag instead runs the S3-T17
//! render_frame self-measurement probe (see
//! [`vega_ui::conversation_stream::bench`]).

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Bounds, Entity, Focusable, KeyBinding, TitlebarOptions, Window, WindowBounds,
    WindowOptions, actions, div, px, size,
};
use gpui_platform::application;
use vega_conversation::types::{
    ArtifactCard as ArtifactProjection, ArtifactCardId, ArtifactPreviewProjection, BranchId,
    BranchSnapshot, BranchSwitchCompletion, BranchSwitchOutcome, CommitChecklist, CommitCompletion,
    CommitErrorCode, CommitOutcome, CommitPrepareCompletion, ConversationEvent, DiffTextProjection,
    GitWorkspaceErrorCode, OpenInOutcome, OpenInTarget, Plan, PlanReviewOutcome,
    PricingDraftReason, PricingNotice, PricingSettingsErrorCode, PricingSettingsProjection, Thread,
    ToolCall, WorkspaceFileId, WorkspaceSnapshot,
};
use vega_conversation::{
    ArtifactCaptureCandidate, ArtifactService, BranchSwitchPermit, BranchWorkspaceService,
    GitWorkspaceService, PricingAuthority, PricingLoadOutcome, PricingSaveOutcome, PricingSavePlan,
    PricingSettingsService, TrustedGitService,
};
use vega_store::Store;
use vega_theme::{Theme, ThemeColors, Typography, theme};
use vega_ui::artifact_card::{
    ArtifactCard, ArtifactCleared, ArtifactOpenRequested, ArtifactPreviewRequested,
};
use vega_ui::branch_selector::{
    BranchListRequested, BranchOperationId, BranchSelector, BranchSelectorClosed,
    BranchSwitchRequested,
};
use vega_ui::commit_panel::{
    CommitDraftRequested, CommitOperationId, CommitPanel, CommitPanelClosed,
    CommitPrepareRequested, CommitRequested,
};
use vega_ui::conversation_stream::{
    ComposerSubmitted, ConversationStream, OpenCommitPanelRequested, OpenWorkspaceDiffRequested,
    ThreadSettingsRequested, WorkspaceToolTerminal, bench as render_frame_bench,
};
use vega_ui::diff_view::{
    DIFF_REFRESH_INTERVAL, DiffClosed, DiffProjectionRequested, DiffRetryRequested, DiffView,
};
use vega_ui::plan_card::PlanReviewRequested;
use vega_ui::settings::{
    CloseSettings, OpenSettings, PricingDiscardRequested, PricingMutationRequested,
    PricingReloadRequested, PricingRetryRequested, SettingsOpen, SettingsView,
};
use vega_ui::sidebar::{
    AUTO_COLLAPSE_WIDTH, CONTENT_MAX_WIDTH, CONTENT_MIN_PADDING, NewThread, OpenedThread,
    PendingDeleteConfirm, Sidebar, SidebarCollapsed, ToggleSidebar, VegaStore, load_collapsed,
    render_delete_confirm_overlay, toggle_persisted,
};

actions!(vega, [Quit, ToggleTheme]);

/// Initial (and minimum) main window size in logical pixels (UI spec §1).
const WINDOW_MIN_WIDTH: f32 = 960.0;
const WINDOW_MIN_HEIGHT: f32 = 600.0;

/// Quick-template placeholder labels for the empty state (ui-spec §4.6);
/// intentionally inert until the template feature lands (A7-02).
const EMPTY_STATE_TEMPLATES: [&str; 3] = ["快捷模板 1", "快捷模板 2", "快捷模板 3"];
const AGENT_EVENT_POLL: Duration = Duration::from_millis(4);
const AGENT_EVENT_CAPACITY: usize = 256;
const AGENT_EVENT_BATCH: usize = 128;
const DIFF_RESULT_POLL: Duration = Duration::from_millis(4);
const PRICING_RESULT_POLL: Duration = Duration::from_millis(4);
#[cfg(test)]
static AGENT_WORKER_STARTS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
const SYSTEM_PROMPT: &str =
    "You are Vega, a careful coding agent working inside the selected project.";

struct UnavailableProvider;

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

enum PendingAgentRun {
    UserMessage(String),
    ApprovedPlan(String),
}

enum AgentUpdate {
    Event(vega_conversation::types::ConversationEvent),
    Finished(bool),
}

struct AgentBatch {
    events: Vec<vega_conversation::types::ConversationEvent>,
    finished: Option<bool>,
}

fn drain_agent_updates(receiver: &mpsc::Receiver<AgentUpdate>) -> AgentBatch {
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

struct ActiveAgentRun {
    generation: u64,
    thread_id: String,
    stream: Entity<ConversationStream>,
    cancel: tokio_util::sync::CancellationToken,
    pending_user_content: Option<String>,
    pending_approved_instruction: Option<String>,
    /// Live wall-clock measurement of this run (S7-T40). It exists only in
    /// run memory: the summary card shows it while the run is alive and `—`
    /// after a restart, because `messages` has no finished timestamp (C4).
    started: Instant,
    /// Assistant message id of the run's durable terminal event, if any
    /// (S7-T40 summary projection key; `None` when the run failed before a
    /// message ever started).
    terminal_message_id: Option<String>,
}

enum AgentBatchIngress {
    Stale,
    Running,
    Finished { success: bool, run: ActiveAgentRun },
}

struct PendingPlanReview {
    stream: Entity<ConversationStream>,
    request: PlanReviewRequested,
}

enum PricingControllerState {
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

fn pricing_retry_ready(
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

fn discard_pricing_draft(state: &mut PricingControllerState, generation: u64) -> bool {
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

struct PricingController {
    service: Option<Arc<PricingSettingsService>>,
    state: PricingControllerState,
    last_generation: u64,
    next_operation: u64,
    active_operation: Option<u64>,
}

impl PricingController {
    fn new(service: Option<Arc<PricingSettingsService>>) -> Self {
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

    fn begin_operation(&mut self) -> Option<u64> {
        if self.active_operation.is_some() {
            return None;
        }
        let operation = self.next_operation.checked_add(1)?;
        self.next_operation = operation;
        self.active_operation = Some(operation);
        Some(operation)
    }

    fn claim_completion(&mut self, operation: u64) -> bool {
        if self.active_operation != Some(operation) {
            return false;
        }
        self.active_operation = None;
        true
    }

    fn next_generation(&mut self) -> Option<u64> {
        let generation = self.last_generation.checked_add(1)?;
        self.last_generation = generation;
        Some(generation)
    }

    fn projection(&self) -> PricingSettingsProjection {
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

    fn select_exact(&self, model: &str) -> Result<PricingAuthority, PricingSettingsErrorCode> {
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

enum PricingWorkerResult {
    Authority(Result<PricingLoadOutcome, PricingSettingsErrorCode>),
    Save(PricingSaveOutcome),
}

#[derive(Clone, Copy)]
enum PricingWorkerKind {
    Authority,
    Save,
    Recovery,
}

#[derive(Default)]
struct AppAgentController {
    next_generation: u64,
    active: Option<ActiveAgentRun>,
    pending_review: Option<PendingPlanReview>,
}

impl AppAgentController {
    fn request_active_cancel(&self) {
        if let Some(active) = &self.active {
            active.cancel.cancel();
        }
    }

    fn queue_review(
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

    fn begin(
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

    fn matches(
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

    fn accept_durable_start(
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
    fn observe_terminal_message(
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

    fn finish(
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

struct PlanReviewRefresh {
    thread: Thread,
    plans: Vec<Plan>,
    approved_instruction_id: Option<String>,
}

struct ThreadStateRefresh {
    thread: Thread,
    plans: Vec<Plan>,
    history: Vec<String>,
    recoverable_approved_instruction: Option<String>,
}

const ARTIFACT_ROUTE_CAP: usize = 10_000;
#[cfg(test)]
static ARTIFACT_OPEN_WORKER_STARTS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);
#[cfg(test)]
static ARTIFACT_PREVIEW_WORKER_STARTS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

#[derive(Clone, PartialEq, Eq)]
struct ArtifactRouteIdentity {
    epoch: u64,
    thread_id: String,
    project_id: String,
    stream: Entity<ConversationStream>,
}

struct ArtifactTerminalJob {
    sequence: u64,
    work: ArtifactTerminalWork,
}

struct ArtifactTerminalDispatch {
    identity: ArtifactRouteIdentity,
    workspace: Arc<GitWorkspaceService>,
    service: Arc<ArtifactService>,
    job: ArtifactTerminalJob,
    cancel: tokio_util::sync::CancellationToken,
}

enum ArtifactTerminalWork {
    Refresh,
    Capture {
        call_id: String,
        candidate: ArtifactCaptureCandidate,
    },
}

struct ArtifactProposal {
    generation: u64,
    call: Option<ToolCall>,
}

struct ArtifactTerminalResult {
    captured: Option<(String, ArtifactProjection)>,
    cards: Vec<ArtifactProjection>,
}

#[derive(Clone, PartialEq, Eq)]
struct ArtifactPreviewFence {
    route: ArtifactRouteIdentity,
    sequence: u64,
    card_id: ArtifactCardId,
    file_id: WorkspaceFileId,
}

#[derive(Clone, PartialEq, Eq)]
struct ArtifactOpenFence {
    route: ArtifactRouteIdentity,
    sequence: u64,
    card_id: ArtifactCardId,
    file_id: WorkspaceFileId,
    target: OpenInTarget,
    lease: TrustedActionToken,
}

struct ActiveArtifactRoute {
    identity: ArtifactRouteIdentity,
    workspace: Arc<GitWorkspaceService>,
    service: Arc<ArtifactService>,
    cancel: tokio_util::sync::CancellationToken,
    agent_generation: Option<u64>,
    proposals: HashMap<String, ArtifactProposal>,
    terminal_sequence: u64,
    terminal_in_flight: Option<u64>,
    terminal_queue: VecDeque<ArtifactTerminalJob>,
    cards: HashMap<ArtifactCardId, Entity<ArtifactCard>>,
    preview_sequence: u64,
    preview_fence: Option<ArtifactPreviewFence>,
    preview_cancel: Option<tokio_util::sync::CancellationToken>,
    open_sequence: u64,
    open_fence: Option<ArtifactOpenFence>,
    open_cancel: Option<tokio_util::sync::CancellationToken>,
}

#[derive(Default)]
struct ArtifactController {
    next_route_epoch: u64,
    active: Option<ActiveArtifactRoute>,
}

impl ArtifactController {
    fn begin(
        &mut self,
        thread: &Thread,
        stream: Entity<ConversationStream>,
        root: PathBuf,
    ) -> Result<ArtifactRouteIdentity, GitWorkspaceErrorCode> {
        if self.active.is_some() {
            return Err(GitWorkspaceErrorCode::ArtifactConflict);
        }
        let epoch = self
            .next_route_epoch
            .checked_add(1)
            .ok_or(GitWorkspaceErrorCode::ArtifactLimit)?;
        let workspace = Arc::new(GitWorkspaceService::new(root).map_err(|failure| failure.code())?);
        let service = Arc::new(
            ArtifactService::new(
                workspace.clone(),
                thread.project_id.clone(),
                thread.id.clone(),
                epoch,
            )
            .map_err(|failure| failure.code())?,
        );
        self.next_route_epoch = epoch;
        let identity = ArtifactRouteIdentity {
            epoch,
            thread_id: thread.id.clone(),
            project_id: thread.project_id.clone(),
            stream,
        };
        self.active = Some(ActiveArtifactRoute {
            identity: identity.clone(),
            workspace,
            service,
            cancel: tokio_util::sync::CancellationToken::new(),
            agent_generation: None,
            proposals: HashMap::new(),
            terminal_sequence: 0,
            terminal_in_flight: None,
            terminal_queue: VecDeque::new(),
            cards: HashMap::new(),
            preview_sequence: 0,
            preview_fence: None,
            preview_cancel: None,
            open_sequence: 0,
            open_fence: None,
            open_cancel: None,
        });
        Ok(identity)
    }

    fn close(&mut self) -> Option<ActiveArtifactRoute> {
        let active = self.active.take();
        if let Some(active) = &active {
            active.cancel.cancel();
            if let Some(cancel) = &active.preview_cancel {
                cancel.cancel();
            }
            if let Some(cancel) = &active.open_cancel {
                cancel.cancel();
            }
        }
        active
    }

    fn matches(&self, identity: &ArtifactRouteIdentity) -> bool {
        self.active
            .as_ref()
            .is_some_and(|active| active.identity == *identity)
    }
}

fn run_artifact_terminal_worker(
    workspace: Arc<GitWorkspaceService>,
    service: Arc<ArtifactService>,
    job: ArtifactTerminalJob,
    cancel: tokio_util::sync::CancellationToken,
    sender: mpsc::SyncSender<Result<(u64, ArtifactTerminalResult), GitWorkspaceErrorCode>>,
) {
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| GitWorkspaceErrorCode::SpawnFailed)
        .and_then(|runtime| {
            runtime.block_on(async {
                let refreshed = workspace.refresh(cancel.child_token()).await;
                service
                    .reconcile(cancel.child_token())
                    .await
                    .map_err(|failure| failure.code())?;
                if let Err(failure) = refreshed {
                    if failure.code() == GitWorkspaceErrorCode::Cancelled {
                        return Err(failure.code());
                    }
                    return Ok((
                        job.sequence,
                        ArtifactTerminalResult {
                            captured: None,
                            cards: service.cards(),
                        },
                    ));
                }
                let captured = match job.work {
                    ArtifactTerminalWork::Capture { call_id, candidate } => service
                        .capture_candidate(candidate, cancel.child_token())
                        .await
                        .map_err(|failure| failure.code())?
                        .map(|card| (call_id, card)),
                    ArtifactTerminalWork::Refresh => None,
                };
                let cards = service
                    .reconcile(cancel)
                    .await
                    .map_err(|failure| failure.code())?;
                Ok((job.sequence, ArtifactTerminalResult { captured, cards }))
            })
        });
    let _ = sender.send(result);
}

fn run_artifact_preview_worker(
    service: Arc<ArtifactService>,
    fence: ArtifactPreviewFence,
    cancel: tokio_util::sync::CancellationToken,
    sender: mpsc::SyncSender<(
        ArtifactPreviewFence,
        Result<ArtifactPreviewProjection, GitWorkspaceErrorCode>,
    )>,
) {
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| GitWorkspaceErrorCode::SpawnFailed)
        .and_then(|runtime| {
            runtime
                .block_on(service.preview(fence.card_id, cancel))
                .map_err(|failure| failure.code())
        });
    let _ = sender.send((fence, result));
}

fn run_artifact_open_worker(
    service: Arc<ArtifactService>,
    fence: ArtifactOpenFence,
    cancel: tokio_util::sync::CancellationToken,
    sender: mpsc::SyncSender<(
        ArtifactOpenFence,
        Result<OpenInOutcome, GitWorkspaceErrorCode>,
    )>,
) {
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| GitWorkspaceErrorCode::SpawnFailed)
        .and_then(|runtime| {
            runtime
                .block_on(service.open_in(fence.card_id, fence.target, cancel))
                .map_err(|failure| failure.code())
        });
    let _ = sender.send((fence, result));
}

#[derive(Clone, PartialEq, Eq)]
struct DiffRouteIdentity {
    epoch: u64,
    thread_id: String,
    project_id: String,
}

#[derive(Clone, PartialEq, Eq)]
struct DiffProjectionFence {
    route: DiffRouteIdentity,
    refresh_request_seq: u64,
    snapshot_generation: u64,
    file_request_seq: u64,
    file_id: WorkspaceFileId,
}

struct PendingDiffProjection {
    fence: DiffProjectionFence,
    result: Result<DiffTextProjection, GitWorkspaceErrorCode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffProjectionDisposition {
    Apply,
    Defer,
    Drop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffRefreshDecision {
    Start(u64),
    Coalesced,
    Overflow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffRefreshCompletion {
    Latest,
    Superseded(Option<u64>),
}

struct ActiveDiffRoute {
    identity: DiffRouteIdentity,
    view: Entity<DiffView>,
    service: Option<Arc<GitWorkspaceService>>,
    cancel: tokio_util::sync::CancellationToken,
    refresh_request_seq: u64,
    refresh_in_flight: Option<u64>,
    queued_refresh_seq: Option<u64>,
    snapshot_generation: Option<u64>,
    file_request_seq: u64,
    requested_file: Option<WorkspaceFileId>,
    projection_cancel: Option<tokio_util::sync::CancellationToken>,
    pending_projection: Option<PendingDiffProjection>,
    focus_pending: bool,
}

impl ActiveDiffRoute {
    fn request_refresh(&mut self) -> DiffRefreshDecision {
        let Some(next) = self.refresh_request_seq.checked_add(1) else {
            return DiffRefreshDecision::Overflow;
        };
        self.refresh_request_seq = next;
        if self.refresh_in_flight.is_some() {
            self.queued_refresh_seq = Some(next);
            return DiffRefreshDecision::Coalesced;
        }
        self.refresh_in_flight = Some(next);
        DiffRefreshDecision::Start(next)
    }

    fn complete_refresh(&mut self, request_seq: u64) -> Option<DiffRefreshCompletion> {
        if self.refresh_in_flight != Some(request_seq) {
            return None;
        }
        self.refresh_in_flight = None;
        let queued = self.queued_refresh_seq.take();
        if request_seq == self.refresh_request_seq {
            Some(DiffRefreshCompletion::Latest)
        } else {
            if let Some(next) = queued {
                self.refresh_in_flight = Some(next);
            }
            Some(DiffRefreshCompletion::Superseded(queued))
        }
    }

    fn next_projection_fence(
        &mut self,
        generation: u64,
        file_id: WorkspaceFileId,
    ) -> Option<DiffProjectionFence> {
        if self.snapshot_generation != Some(generation) {
            return None;
        }
        let next = self.file_request_seq.checked_add(1)?;
        self.file_request_seq = next;
        self.requested_file = Some(file_id);
        Some(DiffProjectionFence {
            route: self.identity.clone(),
            refresh_request_seq: self.refresh_request_seq,
            snapshot_generation: generation,
            file_request_seq: next,
            file_id,
        })
    }

    fn projection_disposition(&self, fence: &DiffProjectionFence) -> DiffProjectionDisposition {
        let current = self.identity == fence.route
            && self.snapshot_generation == Some(fence.snapshot_generation)
            && self.file_request_seq == fence.file_request_seq
            && self.requested_file == Some(fence.file_id)
            && fence.refresh_request_seq <= self.refresh_request_seq;
        if !current {
            DiffProjectionDisposition::Drop
        } else if self.refresh_in_flight.is_some() {
            DiffProjectionDisposition::Defer
        } else {
            DiffProjectionDisposition::Apply
        }
    }
}

#[derive(Default)]
struct DiffController {
    next_route_epoch: u64,
    active: Option<ActiveDiffRoute>,
}

impl DiffController {
    fn begin(
        &mut self,
        thread_id: String,
        project_id: String,
        view: Entity<DiffView>,
    ) -> Option<DiffRouteIdentity> {
        self.close();
        let epoch = self.next_route_epoch.checked_add(1)?;
        self.next_route_epoch = epoch;
        let identity = DiffRouteIdentity {
            epoch,
            thread_id,
            project_id,
        };
        self.active = Some(ActiveDiffRoute {
            identity: identity.clone(),
            view,
            service: None,
            cancel: tokio_util::sync::CancellationToken::new(),
            refresh_request_seq: 0,
            refresh_in_flight: None,
            queued_refresh_seq: None,
            snapshot_generation: None,
            file_request_seq: 0,
            requested_file: None,
            projection_cancel: None,
            pending_projection: None,
            focus_pending: true,
        });
        Some(identity)
    }

    fn close(&mut self) {
        if let Some(active) = self.active.take() {
            active.cancel.cancel();
            if let Some(cancel) = active.projection_cancel {
                cancel.cancel();
            }
        }
    }

    fn matches(&self, identity: &DiffRouteIdentity) -> bool {
        self.active
            .as_ref()
            .is_some_and(|active| active.identity == *identity)
    }

    fn visible_view(&self, thread: &Thread) -> Option<Entity<DiffView>> {
        self.active.as_ref().and_then(|active| {
            (active.identity.thread_id == thread.id
                && active.identity.project_id == thread.project_id)
                .then(|| active.view.clone())
        })
    }
}

enum DiffRefreshWorkerResult {
    Ready {
        service: Arc<GitWorkspaceService>,
        snapshot: WorkspaceSnapshot,
    },
    Failed(GitWorkspaceErrorCode),
}

fn run_diff_refresh_worker(
    service: Option<Arc<GitWorkspaceService>>,
    root: Option<PathBuf>,
    cancel: tokio_util::sync::CancellationToken,
    sender: mpsc::SyncSender<DiffRefreshWorkerResult>,
) {
    let result = (|| {
        let service = match service {
            Some(service) => service,
            None => Arc::new(
                GitWorkspaceService::new(root.ok_or(GitWorkspaceErrorCode::InvalidRoot)?)
                    .map_err(|error| error.code())?,
            ),
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| GitWorkspaceErrorCode::SpawnFailed)?;
        let snapshot = runtime
            .block_on(service.refresh(cancel))
            .map_err(|error| error.code())?;
        Ok::<_, GitWorkspaceErrorCode>((service, snapshot))
    })();
    let output = match result {
        Ok((service, snapshot)) => DiffRefreshWorkerResult::Ready { service, snapshot },
        Err(code) => DiffRefreshWorkerResult::Failed(code),
    };
    let _ = sender.send(output);
}

fn run_diff_projection_worker(
    service: Arc<GitWorkspaceService>,
    file_id: WorkspaceFileId,
    cancel: tokio_util::sync::CancellationToken,
    sender: mpsc::SyncSender<Result<DiffTextProjection, GitWorkspaceErrorCode>>,
) {
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| GitWorkspaceErrorCode::SpawnFailed)
        .and_then(|runtime| {
            runtime
                .block_on(service.diff(file_id, cancel))
                .map_err(|error| error.code())
        });
    let _ = sender.send(result);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // Commit is the frozen T34 seam sharing this coordinator.
enum TrustedActionKind {
    BranchSwitch,
    ArtifactOpen,
    Commit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TrustedActionToken {
    generation: u64,
    kind: TrustedActionKind,
    owner_epoch: u64,
    request_sequence: u64,
}

#[derive(Default)]
struct TrustedActionState {
    next_generation: u64,
    active: Option<TrustedActionToken>,
}

#[derive(Clone, Default)]
struct TrustedActionCoordinator {
    state: Arc<Mutex<TrustedActionState>>,
}

impl TrustedActionCoordinator {
    fn acquire(
        &self,
        kind: TrustedActionKind,
        owner_epoch: u64,
        request_sequence: u64,
    ) -> Option<TrustedActionToken> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if state.active.is_some() {
            return None;
        }
        let generation = state.next_generation.checked_add(1)?;
        state.next_generation = generation;
        let token = TrustedActionToken {
            generation,
            kind,
            owner_epoch,
            request_sequence,
        };
        state.active = Some(token);
        Some(token)
    }

    fn release(&self, token: TrustedActionToken) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if state.active != Some(token) {
            return false;
        }
        state.active = None;
        true
    }

    fn is_busy(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .active
            .is_some()
    }

    #[cfg(test)]
    fn active_token(&self) -> Option<TrustedActionToken> {
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .active
    }
}

#[derive(Clone, PartialEq, Eq)]
struct CommitRouteIdentity {
    epoch: u64,
    thread_id: String,
    project_id: String,
    stream: Entity<ConversationStream>,
    panel: Entity<CommitPanel>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CommitPhase {
    Checklist,
    Preparing,
    CommitReady,
    Drafting,
    Committing,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CommitFenceAuthority {
    None,
    Snapshot(vega_conversation::types::IndexSnapshotId),
    Prepared(vega_conversation::types::PreparedCommitId),
}

#[derive(Clone, PartialEq, Eq)]
struct CommitFence {
    route: CommitRouteIdentity,
    sequence: u64,
    operation: Option<CommitOperationId>,
    phase: CommitPhase,
    authority: CommitFenceAuthority,
}

struct ActiveCommitRoute {
    identity: CommitRouteIdentity,
    service: Arc<TrustedGitService>,
    lease: TrustedActionToken,
    next_sequence: u64,
    phase: CommitPhase,
    snapshot: Option<vega_conversation::types::IndexSnapshotId>,
    prepared: Option<vega_conversation::types::PreparedCommitId>,
    focus_pending: bool,
    pending: Option<CommitFence>,
    cancel: Option<tokio_util::sync::CancellationToken>,
    terminal_done: Option<Arc<AtomicBool>>,
}

#[derive(Default)]
struct CommitController {
    next_epoch: u64,
    active: Option<ActiveCommitRoute>,
    retiring: Option<ActiveCommitRoute>,
}

impl CommitController {
    fn is_open(&self) -> bool {
        self.active.is_some() || self.retiring.is_some()
    }

    fn begin_fence(
        active: &mut ActiveCommitRoute,
        phase: CommitPhase,
        operation: Option<CommitOperationId>,
        authority: CommitFenceAuthority,
    ) -> Option<(
        CommitFence,
        tokio_util::sync::CancellationToken,
        Arc<AtomicBool>,
    )> {
        if active.pending.is_some() {
            return None;
        }
        let sequence = active.next_sequence.checked_add(1)?;
        active.next_sequence = sequence;
        active.phase = phase;
        let fence = CommitFence {
            route: active.identity.clone(),
            sequence,
            operation,
            phase,
            authority,
        };
        let cancel = tokio_util::sync::CancellationToken::new();
        let terminal_done = Arc::new(AtomicBool::new(false));
        active.pending = Some(fence.clone());
        active.cancel = Some(cancel.clone());
        active.terminal_done = Some(terminal_done.clone());
        Some((fence, cancel, terminal_done))
    }

    fn retire_or_close(&mut self) -> Option<(TrustedActionToken, Entity<ConversationStream>)> {
        let active = self.active.take()?;
        if active.pending.is_some() {
            if let Some(cancel) = &active.cancel {
                cancel.cancel();
            }
            if self.retiring.is_none() {
                self.retiring = Some(active);
                return None;
            }
        }
        Some((active.lease, active.identity.stream))
    }

    fn claim(&mut self, fence: &CommitFence) -> CommitClaim {
        if self.active.as_ref().is_some_and(|active| {
            active.pending.as_ref() == Some(fence)
                && Self::authority_is_current(active, fence.authority)
        }) {
            if let Some(active) = self.active.as_mut() {
                active.pending = None;
                active.cancel = None;
                active.terminal_done = None;
            }
            return CommitClaim::Active;
        }
        if self.retiring.as_ref().is_some_and(|active| {
            active.pending.as_ref() == Some(fence)
                && Self::authority_is_current(active, fence.authority)
        }) && let Some(active) = self.retiring.take()
        {
            return CommitClaim::Retiring(Box::new(active));
        }
        CommitClaim::Stale
    }

    fn authority_is_current(active: &ActiveCommitRoute, authority: CommitFenceAuthority) -> bool {
        match authority {
            CommitFenceAuthority::None => true,
            CommitFenceAuthority::Snapshot(id) => active.snapshot == Some(id),
            CommitFenceAuthority::Prepared(id) => active.prepared == Some(id),
        }
    }
}

enum CommitClaim {
    Active,
    Retiring(Box<ActiveCommitRoute>),
    Stale,
}

enum CommitWorkerResult {
    Checklist(Result<CommitChecklist, CommitErrorCode>),
    Prepare(CommitPrepareCompletion, CommitWorkspaceReconciliation),
    Draft(Result<vega_conversation::types::CommitDraft, CommitErrorCode>),
    Commit(CommitCompletion, CommitWorkspaceReconciliation),
    Recovered(CommitErrorCode, CommitWorkspaceReconciliation),
    RuntimeUnavailable(CommitErrorCode),
}

struct CommitWorkspaceReconciliation {
    workspace: Result<WorkspaceSnapshot, CommitErrorCode>,
    workspace_service: Arc<GitWorkspaceService>,
    branch: Option<CommitBranchReconciliation>,
    artifacts: Option<CommitArtifactReconciliation>,
}

#[cfg(test)]
#[derive(Default)]
struct CommitTestProbe {
    prepare_workers: std::sync::atomic::AtomicUsize,
    draft_workers: std::sync::atomic::AtomicUsize,
    commit_workers: std::sync::atomic::AtomicUsize,
    terminal_applications: std::sync::atomic::AtomicUsize,
    drop_commit_sender: AtomicBool,
    trace: Mutex<Vec<&'static str>>,
}

#[cfg(test)]
impl CommitTestProbe {
    fn record(&self, event: &'static str) {
        self.trace
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .push(event);
    }
}

type CommitBranchReconciliation = (
    Arc<BranchWorkspaceService>,
    Result<BranchSnapshot, GitWorkspaceErrorCode>,
);
type CommitArtifactReconciliation = (
    Arc<ArtifactService>,
    Result<Vec<ArtifactProjection>, GitWorkspaceErrorCode>,
);

fn reconcile_commit_consumers(
    runtime: &tokio::runtime::Runtime,
    service: &Arc<TrustedGitService>,
    workspace: Option<WorkspaceSnapshot>,
    branch: Option<Arc<BranchWorkspaceService>>,
    artifacts: Option<Arc<ArtifactService>>,
    #[cfg(test)] probe: Option<&Arc<CommitTestProbe>>,
) -> CommitWorkspaceReconciliation {
    // Mutation ownership is not terminal until one workspace snapshot is
    // authoritative both before and after every consumer read. Branch and
    // artifact failures remain typed projections, but a workspace failure or
    // observed C generation retains the trusted-action lease and retries.
    let mut candidate = workspace;
    let mut backoff = Duration::from_millis(25);
    loop {
        let authoritative = match candidate.take() {
            Some(snapshot) => snapshot,
            None => match runtime.block_on(service.recover_disconnected_mutation()) {
                Ok(snapshot) => snapshot,
                Err(_) => {
                    std::thread::sleep(backoff);
                    backoff = next_commit_recovery_backoff(backoff);
                    continue;
                }
            },
        };
        #[cfg(test)]
        if let Some(probe) = probe {
            probe.record("workspace_candidate");
        }
        let branch_result = branch.as_ref().map(|branch| {
            let snapshot = runtime
                .block_on(branch.refresh(tokio_util::sync::CancellationToken::new()))
                .map_err(|failure| failure.code());
            (branch.clone(), snapshot)
        });
        #[cfg(test)]
        if branch_result.is_some()
            && let Some(probe) = probe
        {
            probe.record("branch_result");
        }
        let artifact_result = artifacts.as_ref().map(|artifacts| {
            let cards = runtime
                .block_on(artifacts.reconcile(tokio_util::sync::CancellationToken::new()))
                .map_err(|failure| failure.code());
            (artifacts.clone(), cards)
        });
        #[cfg(test)]
        if artifact_result.is_some()
            && let Some(probe) = probe
        {
            probe.record("artifact_result");
        }
        match runtime.block_on(service.reconcile_workspace()) {
            Ok(final_workspace) if final_workspace == authoritative => {
                #[cfg(test)]
                if let Some(probe) = probe {
                    probe.record("workspace_final");
                }
                return CommitWorkspaceReconciliation {
                    workspace: Ok(final_workspace),
                    workspace_service: service.workspace_service(),
                    branch: branch_result,
                    artifacts: artifact_result,
                };
            }
            Ok(final_workspace) => {
                candidate = Some(final_workspace);
                std::thread::sleep(backoff);
                backoff = next_commit_recovery_backoff(backoff);
            }
            Err(_) => {
                std::thread::sleep(backoff);
                backoff = next_commit_recovery_backoff(backoff);
            }
        }
    }
}

fn commit_result_has_authoritative_workspace(
    phase: CommitPhase,
    result: &CommitWorkerResult,
) -> bool {
    match (phase, result) {
        (CommitPhase::Preparing, CommitWorkerResult::Prepare(completion, reconciled)) => {
            match &reconciled.workspace {
                Ok(workspace) => {
                    completion.workspace.as_ref() == Some(workspace)
                        && completion.prepared.as_ref().is_none_or(|prepared| {
                            prepared.workspace_generation == workspace.generation
                        })
                }
                Err(_) => completion.workspace.is_none() && completion.prepared.is_none(),
            }
        }
        (CommitPhase::Committing, CommitWorkerResult::Commit(completion, reconciled)) => {
            match &reconciled.workspace {
                Ok(workspace) => completion.workspace.as_ref() == Some(workspace),
                Err(_) => completion.workspace.is_none(),
            }
        }
        (CommitPhase::Checklist, CommitWorkerResult::Checklist(_))
        | (CommitPhase::Drafting, CommitWorkerResult::Draft(_)) => true,
        (
            CommitPhase::Preparing | CommitPhase::Committing,
            CommitWorkerResult::Recovered(_, reconciled),
        ) => reconciled.workspace.is_ok(),
        (
            CommitPhase::Preparing | CommitPhase::Committing,
            CommitWorkerResult::RuntimeUnavailable(_),
        ) => true,
        _ => false,
    }
}

fn commit_result_reconciliation(
    result: &CommitWorkerResult,
) -> Option<&CommitWorkspaceReconciliation> {
    match result {
        CommitWorkerResult::Prepare(_, reconciled)
        | CommitWorkerResult::Commit(_, reconciled)
        | CommitWorkerResult::Recovered(_, reconciled) => Some(reconciled),
        CommitWorkerResult::Checklist(_)
        | CommitWorkerResult::Draft(_)
        | CommitWorkerResult::RuntimeUnavailable(_) => None,
    }
}

fn map_commit_workspace_error(code: GitWorkspaceErrorCode) -> CommitErrorCode {
    match code {
        GitWorkspaceErrorCode::InvalidRoot => CommitErrorCode::InvalidRoot,
        GitWorkspaceErrorCode::NotRepository => CommitErrorCode::NotRepository,
        GitWorkspaceErrorCode::SpawnFailed => CommitErrorCode::SpawnFailed,
        GitWorkspaceErrorCode::TimedOut => CommitErrorCode::TimedOut,
        GitWorkspaceErrorCode::Cancelled => CommitErrorCode::Cancelled,
        GitWorkspaceErrorCode::OutputTooLarge => CommitErrorCode::OutputTooLarge,
        GitWorkspaceErrorCode::MalformedOutput => CommitErrorCode::MalformedOutput,
        GitWorkspaceErrorCode::ProcessControlFailed => CommitErrorCode::ProcessControlFailed,
        _ => CommitErrorCode::ChangedDuringRead,
    }
}

fn map_commit_reconcile_error(code: CommitErrorCode) -> GitWorkspaceErrorCode {
    match code {
        CommitErrorCode::InvalidRoot => GitWorkspaceErrorCode::InvalidRoot,
        CommitErrorCode::NotRepository => GitWorkspaceErrorCode::NotRepository,
        CommitErrorCode::SpawnFailed => GitWorkspaceErrorCode::SpawnFailed,
        CommitErrorCode::TimedOut => GitWorkspaceErrorCode::TimedOut,
        CommitErrorCode::Cancelled => GitWorkspaceErrorCode::Cancelled,
        CommitErrorCode::OutputTooLarge => GitWorkspaceErrorCode::OutputTooLarge,
        CommitErrorCode::MalformedOutput => GitWorkspaceErrorCode::MalformedOutput,
        CommitErrorCode::ProcessControlFailed => GitWorkspaceErrorCode::ProcessControlFailed,
        CommitErrorCode::GitFailed
        | CommitErrorCode::StaleAuthority
        | CommitErrorCode::UnsafeRepository
        | CommitErrorCode::UnsafeFilter
        | CommitErrorCode::IntentToAdd
        | CommitErrorCode::NoStagedChanges
        | CommitErrorCode::InvalidSelection
        | CommitErrorCode::ChangedDuringRead
        | CommitErrorCode::InvalidMessage
        | CommitErrorCode::DraftFailed => GitWorkspaceErrorCode::ChangedDuringRead,
    }
}

fn mark_commit_worker_terminal(
    done: Arc<AtomicBool>,
    window_alive: Arc<AtomicBool>,
    actions: TrustedActionCoordinator,
    lease: TrustedActionToken,
) {
    done.store(true, Ordering::SeqCst);
    if !window_alive.load(Ordering::SeqCst) {
        let _ = actions.release(lease);
    }
}

fn mark_commit_worker_terminal_if_authoritative(
    phase: CommitPhase,
    result: &CommitWorkerResult,
    done: Arc<AtomicBool>,
    window_alive: Arc<AtomicBool>,
    actions: TrustedActionCoordinator,
    lease: TrustedActionToken,
) {
    if commit_result_has_authoritative_workspace(phase, result) {
        mark_commit_worker_terminal(done, window_alive, actions, lease);
    }
}

fn build_commit_runtime() -> Result<tokio::runtime::Runtime, CommitErrorCode> {
    build_commit_runtime_with(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
    })
}

fn build_commit_runtime_with(
    factory: impl FnOnce() -> std::io::Result<tokio::runtime::Runtime>,
) -> Result<tokio::runtime::Runtime, CommitErrorCode> {
    factory().map_err(|_| CommitErrorCode::SpawnFailed)
}

fn next_commit_recovery_backoff(current: Duration) -> Duration {
    current.saturating_mul(2).min(Duration::from_secs(1))
}

fn build_commit_recovery_runtime_with(
    mut factory: impl FnMut() -> Result<tokio::runtime::Runtime, CommitErrorCode>,
    mut wait: impl FnMut(Duration),
) -> Result<tokio::runtime::Runtime, CommitErrorCode> {
    let mut backoff = Duration::from_millis(25);
    for attempt in 0..6 {
        if let Ok(runtime) = factory() {
            return Ok(runtime);
        }
        if attempt < 5 {
            wait(backoff);
            backoff = next_commit_recovery_backoff(backoff);
        }
    }
    Err(CommitErrorCode::SpawnFailed)
}

fn run_commit_checklist_worker(
    workspace: Arc<GitWorkspaceService>,
    service: Arc<TrustedGitService>,
    cancel: tokio_util::sync::CancellationToken,
    #[cfg(test)] probe: Option<Arc<CommitTestProbe>>,
) -> CommitWorkerResult {
    #[cfg(test)]
    let _ = probe;
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| CommitErrorCode::SpawnFailed)
        .and_then(|runtime| {
            runtime.block_on(async {
                workspace
                    .refresh(cancel.clone())
                    .await
                    .map_err(|error| map_commit_workspace_error(error.code()))?;
                service.open_checklist(cancel).await
            })
        });
    CommitWorkerResult::Checklist(result)
}

fn run_commit_prepare_worker(
    service: Arc<TrustedGitService>,
    snapshot_id: vega_conversation::types::IndexSnapshotId,
    selected: Vec<WorkspaceFileId>,
    cancel: tokio_util::sync::CancellationToken,
    branch: Option<Arc<BranchWorkspaceService>>,
    artifacts: Option<Arc<ArtifactService>>,
    #[cfg(test)] probe: Option<Arc<CommitTestProbe>>,
) -> CommitWorkerResult {
    #[cfg(test)]
    if let Some(probe) = &probe {
        probe.prepare_workers.fetch_add(1, Ordering::SeqCst);
    }
    let Ok(runtime) = build_commit_runtime() else {
        return CommitWorkerResult::RuntimeUnavailable(CommitErrorCode::SpawnFailed);
    };
    let mut completion = runtime.block_on(service.prepare(snapshot_id, selected, cancel));
    let reconciled = reconcile_commit_consumers(
        &runtime,
        &service,
        completion.workspace.take(),
        branch,
        artifacts,
        #[cfg(test)]
        probe.as_ref(),
    );
    match &reconciled.workspace {
        Ok(workspace) => {
            completion.workspace = Some(workspace.clone());
            if completion
                .prepared
                .as_ref()
                .is_some_and(|prepared| prepared.workspace_generation != workspace.generation)
            {
                completion.prepared = None;
                completion.error = Some(CommitErrorCode::ChangedDuringRead);
            }
        }
        Err(_) => {
            completion.prepared = None;
            completion.workspace = None;
            completion.error = Some(CommitErrorCode::ChangedDuringRead);
        }
    }
    CommitWorkerResult::Prepare(completion, reconciled)
}

fn run_commit_draft_worker(
    service: Arc<TrustedGitService>,
    prepared_id: vega_conversation::types::PreparedCommitId,
    thread: Thread,
    cancel: tokio_util::sync::CancellationToken,
    provider_override: Option<Arc<dyn vega_runtime::Provider>>,
    #[cfg(test)] probe: Option<Arc<CommitTestProbe>>,
) -> CommitWorkerResult {
    #[cfg(test)]
    if let Some(probe) = &probe {
        probe.draft_workers.fetch_add(1, Ordering::SeqCst);
    }
    let provider = provider_override.unwrap_or_else(|| commit_provider(&thread));
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| CommitErrorCode::DraftFailed)
        .and_then(|runtime| {
            runtime.block_on(service.draft(prepared_id, thread.model, provider, cancel))
        });
    CommitWorkerResult::Draft(result)
}

fn run_commit_execute_worker(
    service: Arc<TrustedGitService>,
    prepared_id: vega_conversation::types::PreparedCommitId,
    message: String,
    cancel: tokio_util::sync::CancellationToken,
    branch: Option<Arc<BranchWorkspaceService>>,
    artifacts: Option<Arc<ArtifactService>>,
    #[cfg(test)] probe: Option<Arc<CommitTestProbe>>,
) -> CommitWorkerResult {
    #[cfg(test)]
    if let Some(probe) = &probe {
        probe.commit_workers.fetch_add(1, Ordering::SeqCst);
    }
    let Ok(runtime) = build_commit_runtime() else {
        return CommitWorkerResult::RuntimeUnavailable(CommitErrorCode::SpawnFailed);
    };
    let mut completion = runtime.block_on(service.commit(prepared_id, message, cancel));
    let reconciled = reconcile_commit_consumers(
        &runtime,
        &service,
        completion.workspace.take(),
        branch,
        artifacts,
        #[cfg(test)]
        probe.as_ref(),
    );
    completion.workspace = reconciled.workspace.as_ref().ok().cloned();
    CommitWorkerResult::Commit(completion, reconciled)
}

fn run_commit_recovery_worker(
    service: Arc<TrustedGitService>,
    branch: Option<Arc<BranchWorkspaceService>>,
    artifacts: Option<Arc<ArtifactService>>,
    #[cfg(test)] probe: Option<Arc<CommitTestProbe>>,
) -> CommitWorkerResult {
    loop {
        let runtime = build_commit_recovery_runtime_with(build_commit_runtime, std::thread::sleep);
        if let Ok(runtime) = runtime {
            let reconciled = reconcile_commit_consumers(
                &runtime,
                &service,
                None,
                branch,
                artifacts,
                #[cfg(test)]
                probe.as_ref(),
            );
            return CommitWorkerResult::Recovered(CommitErrorCode::ChangedDuringRead, reconciled);
        }
        // Each construction batch is finite. Keep the exact owner live and
        // reschedule with a bounded pause rather than forge authority or spin.
        std::thread::sleep(Duration::from_secs(1));
    }
}

#[derive(Clone, PartialEq, Eq)]
struct BranchRouteIdentity {
    epoch: u64,
    thread_id: String,
    project_id: String,
    stream: Entity<ConversationStream>,
    selector: Entity<BranchSelector>,
}

#[derive(Clone, PartialEq, Eq)]
struct BranchListFence {
    route: BranchRouteIdentity,
    sequence: u64,
}

#[derive(Clone, PartialEq, Eq)]
struct BranchSwitchFence {
    route: BranchRouteIdentity,
    sequence: u64,
    snapshot_generation: u64,
    branch_id: BranchId,
    operation_id: BranchOperationId,
    lease: TrustedActionToken,
}

#[derive(Clone, PartialEq, Eq)]
struct BranchPrepareFence {
    route: BranchRouteIdentity,
    sequence: u64,
    snapshot_generation: u64,
    branch_id: BranchId,
    operation_id: BranchOperationId,
}

struct ActiveBranchRoute {
    identity: BranchRouteIdentity,
    service: Arc<BranchWorkspaceService>,
    cancel: tokio_util::sync::CancellationToken,
    list_sequence: u64,
    list_fence: Option<BranchListFence>,
    list_cancel: Option<tokio_util::sync::CancellationToken>,
    switch_sequence: u64,
    prepare_fence: Option<BranchPrepareFence>,
    switch_fence: Option<BranchSwitchFence>,
    switch_cancel: Option<tokio_util::sync::CancellationToken>,
}

#[derive(Default)]
struct BranchController {
    next_epoch: u64,
    active: Option<ActiveBranchRoute>,
    terminal_fence: Option<BranchSwitchFence>,
    cancelled_prepare: Option<BranchPrepareFence>,
}

impl BranchController {
    fn begin(
        &mut self,
        thread: &Thread,
        stream: Entity<ConversationStream>,
        selector: Entity<BranchSelector>,
        root: PathBuf,
    ) -> Result<BranchRouteIdentity, GitWorkspaceErrorCode> {
        self.close();
        let epoch = self
            .next_epoch
            .checked_add(1)
            .ok_or(GitWorkspaceErrorCode::OutputTooLarge)?;
        let service =
            Arc::new(BranchWorkspaceService::new(root).map_err(|failure| failure.code())?);
        self.next_epoch = epoch;
        let identity = BranchRouteIdentity {
            epoch,
            thread_id: thread.id.clone(),
            project_id: thread.project_id.clone(),
            stream,
            selector,
        };
        self.active = Some(ActiveBranchRoute {
            identity: identity.clone(),
            service,
            cancel: tokio_util::sync::CancellationToken::new(),
            list_sequence: 0,
            list_fence: None,
            list_cancel: None,
            switch_sequence: 0,
            prepare_fence: None,
            switch_fence: None,
            switch_cancel: None,
        });
        Ok(identity)
    }

    fn close(&mut self) -> Option<ActiveBranchRoute> {
        let mut active = self.active.take();
        if let Some(active) = &active {
            active.cancel.cancel();
            if let Some(cancel) = &active.list_cancel {
                cancel.cancel();
            }
            if let Some(cancel) = &active.switch_cancel {
                cancel.cancel();
            }
        }
        if let Some(fence) = active
            .as_mut()
            .and_then(|active| active.switch_fence.take())
            && self.terminal_fence.is_none()
        {
            self.terminal_fence = Some(fence);
        }
        if let Some(fence) = active
            .as_mut()
            .and_then(|active| active.prepare_fence.take())
            && self.cancelled_prepare.is_none()
        {
            self.cancelled_prepare = Some(fence);
        }
        active
    }

    fn claim_prepare(&mut self, fence: &BranchPrepareFence) -> bool {
        if let Some(active) = self.active.as_mut()
            && active.prepare_fence.as_ref() == Some(fence)
        {
            active.prepare_fence = None;
            return true;
        }
        if self.cancelled_prepare.as_ref() == Some(fence) {
            self.cancelled_prepare = None;
            return true;
        }
        false
    }

    fn claim_terminal(&mut self, fence: &BranchSwitchFence) -> bool {
        if let Some(active) = self.active.as_mut()
            && active.switch_fence.as_ref() == Some(fence)
        {
            active.switch_fence = None;
            active.switch_cancel = None;
            return true;
        }
        if self.terminal_fence.as_ref() == Some(fence) {
            self.terminal_fence = None;
            return true;
        }
        false
    }
}

fn run_branch_list_worker(
    service: Arc<BranchWorkspaceService>,
    fence: BranchListFence,
    cancel: tokio_util::sync::CancellationToken,
    sender: mpsc::SyncSender<(
        BranchListFence,
        Result<BranchSnapshot, GitWorkspaceErrorCode>,
    )>,
) {
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| GitWorkspaceErrorCode::SpawnFailed)
        .and_then(|runtime| {
            runtime
                .block_on(service.refresh(cancel))
                .map_err(|failure| failure.code())
        });
    let _ = sender.send((fence, result));
}

fn run_branch_prepare_worker(
    service: Arc<BranchWorkspaceService>,
    fence: BranchPrepareFence,
    cancel: tokio_util::sync::CancellationToken,
    sender: mpsc::SyncSender<(
        BranchPrepareFence,
        Result<BranchSwitchPermit, GitWorkspaceErrorCode>,
    )>,
) {
    let result = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime
            .block_on(service.prepare_switch(fence.branch_id, cancel))
            .map_err(|failure| failure.code()),
        Err(_) => Err(GitWorkspaceErrorCode::SpawnFailed),
    };
    let _ = sender.send((fence, result));
}

fn run_branch_switch_worker(
    service: Arc<BranchWorkspaceService>,
    permit: BranchSwitchPermit,
    fence: BranchSwitchFence,
    cancel: tokio_util::sync::CancellationToken,
    sender: mpsc::SyncSender<(BranchSwitchFence, BranchSwitchCompletion)>,
) {
    let completion = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime.block_on(service.execute_switch(permit, cancel)),
        Err(_) => BranchSwitchCompletion {
            outcome: BranchSwitchOutcome::Failed(GitWorkspaceErrorCode::SpawnFailed),
            snapshot: None,
        },
    };
    let _ = sender.send((fence, completion));
}

/// Persists the first-wins review. Only the committed approval winner returns
/// a durable instruction capability for the controller runner boundary.
fn persist_review(
    store: &Store,
    request: &PlanReviewRequested,
) -> Result<PlanReviewRefresh, String> {
    let outcome = vega_conversation::plans::review_plan(
        store,
        &request.thread_id,
        &request.plan_id,
        request.action.clone(),
    )
    .map_err(|error| error.to_string())?;
    let approved_instruction_id = match outcome {
        PlanReviewOutcome::Applied {
            instruction_message_id: Some(instruction_message_id),
        } => Some(instruction_message_id),
        PlanReviewOutcome::Applied {
            instruction_message_id: None,
        }
        | PlanReviewOutcome::Stale => None,
    };
    let thread = vega_conversation::threads::open_thread(store, &request.thread_id)
        .map_err(|error| error.to_string())?;
    let plans = vega_conversation::plans::list_plans(store, &request.thread_id)
        .map_err(|error| error.to_string())?;
    Ok(PlanReviewRefresh {
        thread,
        plans,
        approved_instruction_id,
    })
}

fn reload_thread_and_plans(store: &Store, thread_id: &str) -> Result<(Thread, Vec<Plan>), String> {
    let thread = vega_conversation::threads::open_thread(store, thread_id)
        .map_err(|error| error.to_string())?;
    let plans = vega_conversation::plans::list_plans(store, thread_id)
        .map_err(|error| error.to_string())?;
    Ok((thread, plans))
}

fn reload_thread_state(store: &Store, thread_id: &str) -> Result<ThreadStateRefresh, String> {
    let (thread, plans) = reload_thread_and_plans(store, thread_id)?;
    let history = vega_conversation::threads::composer_history(store, thread_id)
        .map_err(|error| error.to_string())?;
    let recoverable = vega_conversation::plans::recoverable_approved_instruction(store, thread_id)
        .map_err(|error| error.to_string())?;
    Ok(ThreadStateRefresh {
        thread,
        plans,
        history,
        recoverable_approved_instruction: recoverable,
    })
}

fn current_cache_matches(
    opened_thread_id: Option<&str>,
    cached_thread_id: Option<&str>,
    finished_thread_id: &str,
) -> bool {
    opened_thread_id == Some(finished_thread_id) && cached_thread_id == Some(finished_thread_id)
}

fn unique_provider_for_model(
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

fn commit_provider(thread: &Thread) -> Arc<dyn vega_runtime::Provider> {
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

fn commit_retry_policy() -> vega_runtime::RetryPolicy {
    vega_runtime::RetryPolicy {
        max_retries: 0,
        ..vega_runtime::RetryPolicy::default()
    }
}

#[allow(clippy::too_many_arguments)]
fn run_agent_worker(
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
        let tools = vega_tools::Tools::new(project_path).map_err(|_| ())?;
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

/// Root view of the main window: the A1 layout shell — a sidebar (260px,
/// collapsible) next to a content column (max 820px, centered) that hosts
/// either the settings view (Cmd+, / Esc), the opened session
/// ([`ConversationStream`], S3-T17), or the ui-spec §4.6 empty state.
struct VegaWindow {
    /// Sidebar with the [新建任务] button, projects block, and sessions block.
    sidebar: Entity<Sidebar>,
    /// Cached settings view entity. Kept while settings is open so re-renders
    /// (e.g. the theme toggle) never rebuild the form mid-typing; dropped when
    /// settings closes so the next open reloads the config from disk.
    settings_view: Option<Entity<SettingsView>>,
    pricing_controller: PricingController,
    /// Cached conversation stream for the open thread (id, view). S3-T17:
    /// built lazily on first render of an opened thread; rebuilt when another
    /// thread is opened. The stream itself is memory-only (no persistence).
    stream_view: Option<(String, Entity<ConversationStream>)>,
    agent_controller: AppAgentController,
    diff_controller: DiffController,
    artifact_controller: ArtifactController,
    branch_controller: BranchController,
    commit_controller: CommitController,
    trusted_actions: TrustedActionCoordinator,
    window_alive: Arc<AtomicBool>,
    #[cfg(test)]
    commit_provider_override: Option<Arc<dyn vega_runtime::Provider>>,
    #[cfg(test)]
    agent_provider_override: Option<Arc<dyn vega_runtime::Provider>>,
    #[cfg(test)]
    commit_test_probe: Option<Arc<CommitTestProbe>>,
    #[cfg(test)]
    pricing_drop_next_worker_result: bool,
    #[cfg(test)]
    pricing_next_worker_gate: Option<Arc<std::sync::Barrier>>,
}

impl VegaWindow {
    fn record_commit_probe(&self, event: &'static str) {
        #[cfg(not(test))]
        let _ = event;
        #[cfg(test)]
        if let Some(probe) = &self.commit_test_probe {
            probe.record(event);
        }
    }

    fn record_commit_terminal_application(&self, trace: bool) {
        #[cfg(not(test))]
        let _ = trace;
        #[cfg(test)]
        if let Some(probe) = &self.commit_test_probe {
            probe.terminal_applications.fetch_add(1, Ordering::SeqCst);
            if trace {
                probe.record("panel_terminal");
            }
        }
    }

    fn new(cx: &mut Context<Self>) -> Self {
        cx.observe_global::<OpenedThread>(|this, cx| {
            this.close_diff_if_route_stale(cx);
            this.close_artifact_if_route_stale(cx);
            this.close_branch_if_route_stale(cx);
            this.close_commit_if_route_stale(cx);
        })
        .detach();
        cx.observe_global::<SettingsOpen>(|this, cx| {
            this.close_diff_if_route_stale(cx);
            this.close_artifact_if_route_stale(cx);
            this.close_branch_if_route_stale(cx);
            this.close_commit_if_route_stale(cx);
        })
        .detach();
        cx.observe_global::<vega_ui::sidebar::SelectedProject>(|this, cx| {
            this.close_artifact_if_route_stale(cx);
            this.close_branch_if_route_stale(cx);
            this.close_commit_if_route_stale(cx);
        })
        .detach();
        let pricing_service = cx
            .global::<VegaStore>()
            .0
            .as_ref()
            .ok()
            .and_then(|store| store.database_path())
            .and_then(|path| path.parent())
            .map(|root| Arc::new(PricingSettingsService::new(root.join("pricing.json"))));
        let mut window = Self {
            sidebar: cx.new(Sidebar::new),
            settings_view: None,
            pricing_controller: PricingController::new(pricing_service),
            stream_view: None,
            agent_controller: AppAgentController::default(),
            diff_controller: DiffController::default(),
            artifact_controller: ArtifactController::default(),
            branch_controller: BranchController::default(),
            commit_controller: CommitController::default(),
            trusted_actions: TrustedActionCoordinator::default(),
            window_alive: Arc::new(AtomicBool::new(true)),
            #[cfg(test)]
            commit_provider_override: None,
            #[cfg(test)]
            agent_provider_override: None,
            #[cfg(test)]
            commit_test_probe: None,
            #[cfg(test)]
            pricing_drop_next_worker_result: false,
            #[cfg(test)]
            pricing_next_worker_gate: None,
        };
        window.start_pricing_load(cx);
        window
    }

    fn start_pricing_load(&mut self, cx: &mut Context<Self>) {
        let Some(service) = self.pricing_controller.service.clone() else {
            return;
        };
        let Some(operation) = self.pricing_controller.begin_operation() else {
            self.pricing_controller.state =
                PricingControllerState::Invalid(PricingSettingsErrorCode::LimitExceeded);
            return;
        };
        self.pricing_controller.state = PricingControllerState::Loading;
        self.spawn_pricing_worker(
            operation,
            PricingWorkerKind::Authority,
            move || PricingWorkerResult::Authority(service.load_or_seed()),
            cx,
        );
    }

    fn request_pricing_reload(&mut self, view: Entity<SettingsView>, cx: &mut Context<Self>) {
        if !cx.global::<SettingsOpen>().0
            || self.settings_view.as_ref() != Some(&view)
            || self.pricing_controller.active_operation.is_some()
        {
            return;
        }
        let Some(service) = self.pricing_controller.service.clone() else {
            self.pricing_controller.state =
                PricingControllerState::Invalid(PricingSettingsErrorCode::Io);
            self.push_pricing_projection(cx);
            return;
        };
        let Some(operation) = self.pricing_controller.begin_operation() else {
            self.pricing_controller.state =
                PricingControllerState::Invalid(PricingSettingsErrorCode::LimitExceeded);
            self.push_pricing_projection(cx);
            return;
        };
        self.pricing_controller.state = PricingControllerState::Reloading;
        self.push_pricing_projection(cx);
        self.spawn_pricing_worker(
            operation,
            PricingWorkerKind::Authority,
            move || PricingWorkerResult::Authority(service.reload()),
            cx,
        );
    }

    fn request_pricing_mutation(
        &mut self,
        view: Entity<SettingsView>,
        request: &PricingMutationRequested,
        cx: &mut Context<Self>,
    ) {
        if !cx.global::<SettingsOpen>().0
            || self.settings_view.as_ref() != Some(&view)
            || self.pricing_controller.active_operation.is_some()
        {
            return;
        }
        let (previous, generation, notice, draft, draft_reason) =
            match &self.pricing_controller.state {
                PricingControllerState::Ready {
                    authority,
                    generation,
                    notice,
                    draft,
                    draft_reason,
                    ..
                } => (
                    authority.clone(),
                    *generation,
                    *notice,
                    draft.clone(),
                    *draft_reason,
                ),
                _ => return,
            };
        if generation != request.generation {
            return;
        }
        if draft.is_some() {
            self.pricing_controller.state = PricingControllerState::Ready {
                authority: previous,
                generation,
                notice,
                draft,
                draft_reason,
                error: Some(PricingSettingsErrorCode::Busy),
            };
            self.push_pricing_projection(cx);
            return;
        }
        let mutation = match &request.mutation {
            Ok(mutation) => mutation.clone(),
            Err(code) => {
                self.pricing_controller.state = PricingControllerState::Ready {
                    authority: previous,
                    generation,
                    notice,
                    draft: None,
                    draft_reason: None,
                    error: Some(*code),
                };
                self.push_pricing_projection(cx);
                return;
            }
        };
        let Some(service) = self.pricing_controller.service.clone() else {
            return;
        };
        let plan = match service.prepare_save(&previous, mutation) {
            Ok(plan) => plan,
            Err(code) => {
                self.pricing_controller.state = PricingControllerState::Ready {
                    authority: previous,
                    generation,
                    notice,
                    draft: None,
                    draft_reason: None,
                    error: Some(code),
                };
                self.push_pricing_projection(cx);
                return;
            }
        };
        self.begin_pricing_save(previous, notice, generation, plan, service, cx);
    }

    fn begin_pricing_save(
        &mut self,
        previous: PricingAuthority,
        previous_notice: Option<PricingNotice>,
        generation: u64,
        plan: PricingSavePlan,
        service: Arc<PricingSettingsService>,
        cx: &mut Context<Self>,
    ) {
        let Some(operation) = self.pricing_controller.begin_operation() else {
            self.pricing_controller.state = pricing_retry_ready(
                previous,
                generation,
                previous_notice,
                plan,
                PricingSettingsErrorCode::LimitExceeded,
            );
            self.push_pricing_projection(cx);
            return;
        };
        self.pricing_controller.state = PricingControllerState::Saving {
            previous,
            previous_notice,
            generation,
            plan: plan.clone(),
        };
        self.push_pricing_projection(cx);
        self.spawn_pricing_worker(
            operation,
            PricingWorkerKind::Save,
            move || PricingWorkerResult::Save(service.save(&plan)),
            cx,
        );
    }

    fn request_pricing_retry(
        &mut self,
        view: Entity<SettingsView>,
        request: &PricingRetryRequested,
        cx: &mut Context<Self>,
    ) {
        if !cx.global::<SettingsOpen>().0
            || self.settings_view.as_ref() != Some(&view)
            || self.pricing_controller.active_operation.is_some()
        {
            return;
        }
        let (authority, notice, generation, plan) = match &self.pricing_controller.state {
            PricingControllerState::Ready {
                authority,
                notice,
                generation,
                draft: Some(plan),
                ..
            } if *generation == request.generation => {
                (authority.clone(), *notice, *generation, plan.clone())
            }
            _ => return,
        };
        let Some(service) = self.pricing_controller.service.clone() else {
            return;
        };
        self.begin_pricing_save(authority, notice, generation, plan, service, cx);
    }

    fn request_pricing_discard(
        &mut self,
        view: Entity<SettingsView>,
        request: &PricingDiscardRequested,
        cx: &mut Context<Self>,
    ) {
        if !cx.global::<SettingsOpen>().0
            || self.settings_view.as_ref() != Some(&view)
            || self.pricing_controller.active_operation.is_some()
        {
            return;
        }
        if !discard_pricing_draft(&mut self.pricing_controller.state, request.generation) {
            return;
        }
        self.push_pricing_projection(cx);
    }

    fn spawn_pricing_worker(
        &mut self,
        operation: u64,
        kind: PricingWorkerKind,
        worker: impl FnOnce() -> PricingWorkerResult + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        let (sender, receiver) = mpsc::sync_channel(1);
        #[cfg(test)]
        let drop_result = std::mem::replace(&mut self.pricing_drop_next_worker_result, false);
        #[cfg(test)]
        let worker_gate = self.pricing_next_worker_gate.take();
        #[cfg(not(test))]
        let drop_result = false;
        let spawned = std::thread::Builder::new()
            .name("vega-pricing".to_string())
            .spawn(move || {
                #[cfg(test)]
                if let Some(gate) = worker_gate {
                    gate.wait();
                }
                let result = worker();
                if !drop_result {
                    let _ = sender.send(result);
                }
            });
        if spawned.is_err() {
            self.apply_pricing_worker_not_started(operation, kind, cx);
            return;
        }
        cx.spawn(async move |this, cx| {
            loop {
                match receiver.try_recv() {
                    Ok(result) => {
                        let _ = this.update(cx, |this, cx| {
                            this.apply_pricing_worker_result(operation, result, cx);
                        });
                        break;
                    }
                    Err(mpsc::TryRecvError::Empty) => {
                        cx.background_executor().timer(PRICING_RESULT_POLL).await;
                    }
                    Err(mpsc::TryRecvError::Disconnected) => {
                        let _ = this.update(cx, |this, cx| {
                            this.apply_pricing_worker_disconnected(operation, kind, cx);
                        });
                        break;
                    }
                }
            }
        })
        .detach();
    }

    fn apply_pricing_worker_result(
        &mut self,
        operation: u64,
        result: PricingWorkerResult,
        cx: &mut Context<Self>,
    ) {
        if !self.pricing_controller.claim_completion(operation) {
            return;
        }
        match result {
            PricingWorkerResult::Authority(Ok(outcome)) => {
                let Some(generation) = self.pricing_controller.next_generation() else {
                    self.pricing_controller.state =
                        PricingControllerState::Invalid(PricingSettingsErrorCode::LimitExceeded);
                    self.push_pricing_projection(cx);
                    return;
                };
                self.pricing_controller.state = PricingControllerState::Ready {
                    authority: outcome.authority,
                    generation,
                    notice: outcome.notice,
                    draft: None,
                    draft_reason: None,
                    error: None,
                };
            }
            PricingWorkerResult::Authority(Err(code)) => {
                self.pricing_controller.state = PricingControllerState::Invalid(code);
            }
            PricingWorkerResult::Save(outcome) => {
                let old = std::mem::replace(
                    &mut self.pricing_controller.state,
                    PricingControllerState::Invalid(PricingSettingsErrorCode::Io),
                );
                let PricingControllerState::Saving {
                    previous,
                    previous_notice,
                    generation,
                    plan,
                } = old
                else {
                    return;
                };
                match outcome {
                    PricingSaveOutcome::Ready {
                        authority,
                        notice,
                        dirty_conflict,
                    } => {
                        let Some(new_generation) = self.pricing_controller.next_generation() else {
                            self.pricing_controller.state = PricingControllerState::Invalid(
                                PricingSettingsErrorCode::LimitExceeded,
                            );
                            self.push_pricing_projection(cx);
                            return;
                        };
                        self.pricing_controller.state = PricingControllerState::Ready {
                            authority,
                            generation: new_generation,
                            notice,
                            draft: dirty_conflict.then_some(plan),
                            draft_reason: dirty_conflict
                                .then_some(PricingDraftReason::ExternalConflict),
                            error: None,
                        };
                    }
                    PricingSaveOutcome::PreCommitFailure(code) => {
                        self.pricing_controller.state =
                            pricing_retry_ready(previous, generation, previous_notice, plan, code);
                    }
                    PricingSaveOutcome::RecoveryRequired => {
                        self.pricing_controller.state = PricingControllerState::Invalid(
                            PricingSettingsErrorCode::RecoveryRequired,
                        );
                    }
                }
            }
        }
        self.push_pricing_projection(cx);
    }

    fn apply_pricing_worker_not_started(
        &mut self,
        operation: u64,
        kind: PricingWorkerKind,
        cx: &mut Context<Self>,
    ) {
        if !self.pricing_controller.claim_completion(operation) {
            return;
        }
        let code = match kind {
            PricingWorkerKind::Recovery => PricingSettingsErrorCode::RecoveryRequired,
            PricingWorkerKind::Authority | PricingWorkerKind::Save => PricingSettingsErrorCode::Io,
        };
        let old = std::mem::replace(
            &mut self.pricing_controller.state,
            PricingControllerState::Invalid(code),
        );
        if matches!(kind, PricingWorkerKind::Save)
            && let PricingControllerState::Saving {
                previous,
                previous_notice,
                generation,
                plan,
            } = old
        {
            self.pricing_controller.state =
                pricing_retry_ready(previous, generation, previous_notice, plan, code);
        }
        self.push_pricing_projection(cx);
    }

    fn apply_pricing_worker_disconnected(
        &mut self,
        operation: u64,
        kind: PricingWorkerKind,
        cx: &mut Context<Self>,
    ) {
        if !self.pricing_controller.claim_completion(operation) {
            return;
        }
        if !matches!(kind, PricingWorkerKind::Save) {
            self.pricing_controller.state = PricingControllerState::Invalid(match kind {
                PricingWorkerKind::Recovery => PricingSettingsErrorCode::RecoveryRequired,
                PricingWorkerKind::Authority | PricingWorkerKind::Save => {
                    PricingSettingsErrorCode::Io
                }
            });
            self.push_pricing_projection(cx);
            return;
        }
        let PricingControllerState::Saving { plan, .. } = &self.pricing_controller.state else {
            self.pricing_controller.state =
                PricingControllerState::Invalid(PricingSettingsErrorCode::RecoveryRequired);
            self.push_pricing_projection(cx);
            return;
        };
        let plan = plan.clone();
        let Some(service) = self.pricing_controller.service.clone() else {
            self.pricing_controller.state =
                PricingControllerState::Invalid(PricingSettingsErrorCode::RecoveryRequired);
            self.push_pricing_projection(cx);
            return;
        };
        let Some(recovery_operation) = self.pricing_controller.begin_operation() else {
            self.pricing_controller.state =
                PricingControllerState::Invalid(PricingSettingsErrorCode::RecoveryRequired);
            self.push_pricing_projection(cx);
            return;
        };
        self.spawn_pricing_worker(
            recovery_operation,
            PricingWorkerKind::Recovery,
            move || PricingWorkerResult::Save(service.recover_started_save(&plan)),
            cx,
        );
    }

    fn push_pricing_projection(&mut self, cx: &mut Context<Self>) {
        if let Some(view) = &self.settings_view {
            let projection = self.pricing_controller.projection();
            view.update(cx, |view, cx| {
                view.apply_pricing_projection(projection, cx);
            });
        }
        cx.notify();
    }

    fn window_terminal_cleanup(&mut self) {
        self.window_alive.store(false, Ordering::SeqCst);
        if let Some(active) = self.agent_controller.active.take() {
            active.cancel.cancel();
        }
        self.diff_controller.close();
        let _ = self.artifact_controller.close();
        let _ = self.branch_controller.close();
        for route in [
            self.commit_controller.active.as_ref(),
            self.commit_controller.retiring.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            if let Some(cancel) = &route.cancel {
                cancel.cancel();
            }
            if route.pending.is_none()
                || route
                    .terminal_done
                    .as_ref()
                    .is_some_and(|done| done.load(Ordering::SeqCst))
            {
                let _ = self.trusted_actions.release(route.lease);
            }
        }
    }

    fn artifact_route_is_current(identity: &ArtifactRouteIdentity, cx: &App) -> bool {
        !cx.global::<SettingsOpen>().0
            && cx
                .global::<vega_ui::sidebar::SelectedProject>()
                .0
                .as_deref()
                == Some(identity.project_id.as_str())
            && cx
                .global::<OpenedThread>()
                .0
                .as_ref()
                .is_some_and(|thread| {
                    thread.id == identity.thread_id && thread.project_id == identity.project_id
                })
    }

    fn close_artifact_route(&mut self, code: GitWorkspaceErrorCode, cx: &mut Context<Self>) {
        if let Some(active) = self.artifact_controller.close() {
            for card in active.cards.into_values() {
                card.update(cx, |card, cx| card.invalidate(code, cx));
            }
            cx.notify();
        }
    }

    fn close_artifact_if_route_stale(&mut self, cx: &mut Context<Self>) {
        let stale = self
            .artifact_controller
            .active
            .as_ref()
            .is_some_and(|active| !Self::artifact_route_is_current(&active.identity, cx));
        if stale {
            self.close_artifact_route(GitWorkspaceErrorCode::StaleGeneration, cx);
        }
    }

    fn artifact_project_root(thread: &Thread, cx: &App) -> Result<PathBuf, GitWorkspaceErrorCode> {
        let store = cx
            .global::<VegaStore>()
            .0
            .as_ref()
            .map_err(|_| GitWorkspaceErrorCode::InvalidRoot)?;
        let project = vega_store::projects::find(store.conn(), &thread.project_id)
            .map_err(|_| GitWorkspaceErrorCode::InvalidRoot)?
            .ok_or(GitWorkspaceErrorCode::InvalidRoot)?;
        Ok(PathBuf::from(project.path))
    }

    fn branch_route_is_current(identity: &BranchRouteIdentity, cx: &App) -> bool {
        !cx.global::<SettingsOpen>().0
            && cx
                .global::<vega_ui::sidebar::SelectedProject>()
                .0
                .as_deref()
                == Some(identity.project_id.as_str())
            && cx
                .global::<OpenedThread>()
                .0
                .as_ref()
                .is_some_and(|thread| {
                    thread.id == identity.thread_id && thread.project_id == identity.project_id
                })
    }

    fn close_branch_route(&mut self, code: GitWorkspaceErrorCode, cx: &mut Context<Self>) {
        let pending = self
            .branch_controller
            .active
            .as_ref()
            .and_then(|active| active.identity.selector.read(cx).pending_key());
        if let Some(active) = self.branch_controller.close() {
            active.identity.selector.update(cx, |selector, cx| {
                if let Some((operation, generation, branch_id)) = pending {
                    let _ = selector.clear_pending(operation, generation, branch_id, cx);
                }
                selector.close_route(code, cx);
            });
            cx.notify();
        }
    }

    fn close_branch_if_route_stale(&mut self, cx: &mut Context<Self>) {
        let stale = self
            .branch_controller
            .active
            .as_ref()
            .is_some_and(|active| !Self::branch_route_is_current(&active.identity, cx));
        if stale {
            self.close_branch_route(GitWorkspaceErrorCode::StaleGeneration, cx);
        }
    }

    fn ensure_branch_route(
        &mut self,
        thread: &Thread,
        stream: Entity<ConversationStream>,
        cx: &mut Context<Self>,
    ) {
        let selector = stream.read(cx).branch_selector();
        let current = self
            .branch_controller
            .active
            .as_ref()
            .is_some_and(|active| {
                active.identity.thread_id == thread.id
                    && active.identity.project_id == thread.project_id
                    && active.identity.stream == stream
                    && active.identity.selector == selector
            });
        if current {
            return;
        }
        self.close_branch_route(GitWorkspaceErrorCode::StaleGeneration, cx);
        let result = Self::artifact_project_root(thread, cx).and_then(|root| {
            self.branch_controller
                .begin(thread, stream, selector, root)
                .map(|_| ())
        });
        if result.is_err() {
            self.close_branch_route(GitWorkspaceErrorCode::InvalidRoot, cx);
        }
    }

    fn request_branch_list(
        &mut self,
        selector: Entity<BranchSelector>,
        request: &BranchListRequested,
        cx: &mut Context<Self>,
    ) {
        let (fence, service, cancel) = {
            let Some(active) = self.branch_controller.active.as_mut() else {
                selector.update(cx, |selector, cx| {
                    selector.apply_error(GitWorkspaceErrorCode::StaleGeneration, cx)
                });
                return;
            };
            if !Self::branch_route_is_current(&active.identity, cx)
                || active.identity.selector != selector
                || active.identity.thread_id != request.thread_id
                || active.identity.project_id != request.project_id
                || active.prepare_fence.is_some()
                || active.switch_fence.is_some()
            {
                return;
            }
            let Some(sequence) = active.list_sequence.checked_add(1) else {
                self.close_branch_route(GitWorkspaceErrorCode::OutputTooLarge, cx);
                return;
            };
            active.list_sequence = sequence;
            if let Some(cancel) = active.list_cancel.take() {
                cancel.cancel();
            }
            let fence = BranchListFence {
                route: active.identity.clone(),
                sequence,
            };
            let cancel = active.cancel.child_token();
            active.list_fence = Some(fence.clone());
            active.list_cancel = Some(cancel.clone());
            (fence, active.service.clone(), cancel)
        };
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker_fence = fence.clone();
        let worker = std::thread::Builder::new()
            .name("vega-branch-list".into())
            .spawn(move || run_branch_list_worker(service, worker_fence, cancel, sender));
        if worker.is_err() {
            self.finish_branch_list(fence, Err(GitWorkspaceErrorCode::SpawnFailed), cx);
            return;
        }
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(DIFF_RESULT_POLL).await;
                let (fence, result) = match receiver.try_recv() {
                    Ok(output) => output,
                    Err(mpsc::TryRecvError::Empty) => continue,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        (fence, Err(GitWorkspaceErrorCode::SpawnFailed))
                    }
                };
                let _ = this.update(cx, |this, cx| this.finish_branch_list(fence, result, cx));
                break;
            }
        })
        .detach();
    }

    fn finish_branch_list(
        &mut self,
        fence: BranchListFence,
        result: Result<BranchSnapshot, GitWorkspaceErrorCode>,
        cx: &mut Context<Self>,
    ) {
        if !Self::branch_route_is_current(&fence.route, cx) {
            return;
        }
        let selector = {
            let Some(active) = self.branch_controller.active.as_mut() else {
                return;
            };
            if active.list_fence.as_ref() != Some(&fence) {
                return;
            }
            active.list_fence = None;
            active.list_cancel = None;
            active.identity.selector.clone()
        };
        match result {
            Ok(snapshot) => {
                selector.update(cx, |selector, cx| {
                    let _ = selector.apply_snapshot(snapshot, cx);
                });
            }
            Err(GitWorkspaceErrorCode::Cancelled | GitWorkspaceErrorCode::StaleGeneration) => {}
            Err(code) => selector.update(cx, |selector, cx| selector.apply_error(code, cx)),
        }
    }

    fn branch_guards_clear(&self, stream: &Entity<ConversationStream>, cx: &App) -> bool {
        !self.trusted_actions.is_busy()
            && !self.commit_controller.is_open()
            && self.agent_controller.active.is_none()
            && !stream.read(cx).has_active_agent()
            && !stream.read(cx).has_pending_permission()
            && !stream.read(cx).has_pending_plan_review(cx)
    }

    fn commit_route_is_current(&self, identity: &CommitRouteIdentity, cx: &App) -> bool {
        !cx.global::<SettingsOpen>().0
            && cx
                .global::<vega_ui::sidebar::SelectedProject>()
                .0
                .as_deref()
                == Some(identity.project_id.as_str())
            && cx
                .global::<OpenedThread>()
                .0
                .as_ref()
                .is_some_and(|thread| {
                    thread.id == identity.thread_id && thread.project_id == identity.project_id
                })
            && self
                .stream_view
                .as_ref()
                .is_some_and(|(thread_id, stream)| {
                    thread_id == &identity.thread_id
                        && stream == &identity.stream
                        && stream.read(cx).commit_panel() == identity.panel
                })
    }

    fn commit_guards_clear(&self, stream: &Entity<ConversationStream>, cx: &App) -> bool {
        !self.trusted_actions.is_busy()
            && self.agent_controller.active.is_none()
            && !stream.read(cx).has_active_agent()
            && !stream.read(cx).has_pending_permission()
            && !stream.read(cx).has_pending_plan_review(cx)
    }

    fn poll_commit_worker(
        &mut self,
        fence: CommitFence,
        receiver: mpsc::Receiver<CommitWorkerResult>,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(DIFF_RESULT_POLL).await;
                let result = match receiver.try_recv() {
                    Ok(result) => result,
                    Err(mpsc::TryRecvError::Empty) => continue,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        let _ = this.update(cx, |this, cx| {
                            this.recover_disconnected_commit_worker(fence, cx)
                        });
                        return;
                    }
                };
                let _ = this.update(cx, |this, cx| this.finish_commit_worker(fence, result, cx));
                break;
            }
        })
        .detach();
    }

    fn recover_disconnected_commit_worker(&mut self, fence: CommitFence, cx: &mut Context<Self>) {
        if matches!(fence.phase, CommitPhase::Checklist | CommitPhase::Drafting) {
            let result = if fence.phase == CommitPhase::Checklist {
                CommitWorkerResult::Checklist(Err(CommitErrorCode::SpawnFailed))
            } else {
                CommitWorkerResult::Draft(Err(CommitErrorCode::DraftFailed))
            };
            self.finish_commit_worker(fence, result, cx);
            return;
        }
        let route = self
            .commit_controller
            .active
            .as_mut()
            .filter(|active| active.pending.as_ref() == Some(&fence))
            .or_else(|| {
                self.commit_controller
                    .retiring
                    .as_mut()
                    .filter(|active| active.pending.as_ref() == Some(&fence))
            });
        let Some(active) = route else {
            return;
        };
        let service = active.service.clone();
        let lease = active.lease;
        let terminal_done = Arc::new(AtomicBool::new(false));
        active.terminal_done = Some(terminal_done.clone());
        let branch = self
            .branch_controller
            .active
            .as_ref()
            .filter(|branch| branch.identity.project_id == fence.route.project_id)
            .map(|branch| branch.service.clone());
        let artifacts = self
            .artifact_controller
            .active
            .as_ref()
            .filter(|artifacts| {
                artifacts.identity.project_id == fence.route.project_id
                    && Arc::ptr_eq(&artifacts.workspace, &service.workspace_service())
            })
            .map(|artifacts| artifacts.service.clone());
        let (sender, receiver) = mpsc::sync_channel(1);
        let window_alive = self.window_alive.clone();
        let actions = self.trusted_actions.clone();
        #[cfg(test)]
        let probe = self.commit_test_probe.clone();
        let recovery_phase = fence.phase;
        cx.background_executor()
            .spawn(async move {
                let result = run_commit_recovery_worker(
                    service,
                    branch,
                    artifacts,
                    #[cfg(test)]
                    probe,
                );
                mark_commit_worker_terminal_if_authoritative(
                    recovery_phase,
                    &result,
                    terminal_done,
                    window_alive,
                    actions,
                    lease,
                );
                let _ = sender.send(result);
            })
            .detach();
        self.poll_commit_worker(fence, receiver, cx);
    }

    fn finish_commit_worker(
        &mut self,
        fence: CommitFence,
        result: CommitWorkerResult,
        cx: &mut Context<Self>,
    ) {
        let has_authoritative_workspace =
            commit_result_has_authoritative_workspace(fence.phase, &result);
        let reconciliation = commit_result_reconciliation(&result);
        match self.commit_controller.claim(&fence) {
            CommitClaim::Stale => return,
            CommitClaim::Retiring(mut active) => {
                if !has_authoritative_workspace {
                    active.pending = Some(fence.clone());
                    self.commit_controller.retiring = Some(*active);
                    self.recover_disconnected_commit_worker(fence, cx);
                    return;
                }
                if let Some(reconciled) = reconciliation {
                    self.apply_commit_workspace_reconciliation(&fence.route, reconciled, cx);
                }
                let terminal_applied = if let Some(operation) = fence.operation {
                    active
                        .identity
                        .panel
                        .update(cx, |panel, cx| panel.clear_pending(operation, cx))
                } else {
                    false
                };
                if terminal_applied {
                    self.record_commit_terminal_application(true);
                }
                if self.trusted_actions.release(active.lease) {
                    active
                        .identity
                        .stream
                        .update(cx, |stream, cx| stream.set_trusted_action_busy(false, cx));
                    self.record_commit_probe("lease_release");
                }
                return;
            }
            CommitClaim::Active => {}
        }
        if !has_authoritative_workspace {
            if let Some(mut active) = self.commit_controller.active.take() {
                let _ = active
                    .identity
                    .panel
                    .update(cx, |panel, cx| panel.request_close(cx));
                active.pending = Some(fence.clone());
                self.commit_controller.retiring = Some(active);
                self.recover_disconnected_commit_worker(fence, cx);
            }
            return;
        }
        if let Some(reconciled) = reconciliation {
            self.apply_commit_workspace_reconciliation(&fence.route, reconciled, cx);
        }
        if !self.commit_route_is_current(&fence.route, cx) {
            self.close_commit_route(cx);
            return;
        }
        match (fence.phase, result) {
            (CommitPhase::Checklist, CommitWorkerResult::Checklist(result)) => {
                let accepted = match result {
                    Ok(checklist) => {
                        let snapshot_id = checklist.id;
                        let accepted = fence
                            .route
                            .panel
                            .update(cx, |panel, cx| panel.apply_checklist(checklist, cx));
                        if accepted && let Some(active) = self.commit_controller.active.as_mut() {
                            active.phase = CommitPhase::Checklist;
                            active.snapshot = Some(snapshot_id);
                            active.prepared = None;
                        }
                        accepted
                    }
                    Err(code) => fence.route.panel.update(cx, |panel, cx| {
                        panel.apply_error(
                            vega_ui::commit_panel::CommitPanelStage::Loading,
                            code,
                            cx,
                        )
                    }),
                };
                if !accepted {
                    let _ = fence.route.panel.update(cx, |panel, cx| {
                        panel.apply_error(
                            vega_ui::commit_panel::CommitPanelStage::Loading,
                            CommitErrorCode::MalformedOutput,
                            cx,
                        )
                    });
                    self.close_commit_route(cx);
                } else {
                    self.record_commit_terminal_application(false);
                }
            }
            (CommitPhase::Preparing, CommitWorkerResult::Prepare(completion, _)) => {
                let success = completion.prepared.is_some()
                    && completion.workspace.is_some()
                    && completion.error.is_none();
                let prepared_id = completion.prepared.as_ref().map(|prepared| prepared.id);
                if let Some(active) = self.commit_controller.active.as_mut() {
                    active.phase = if success {
                        CommitPhase::CommitReady
                    } else {
                        CommitPhase::Checklist
                    };
                    active.prepared = success.then_some(prepared_id).flatten();
                }
                if let Some(operation) = fence.operation {
                    let applied = fence.route.panel.update(cx, |panel, cx| {
                        let value = completion.prepared.ok_or(
                            completion
                                .error
                                .unwrap_or(CommitErrorCode::ChangedDuringRead),
                        );
                        panel.finish_prepare(operation, value, cx)
                    });
                    if applied {
                        self.record_commit_terminal_application(true);
                    }
                }
            }
            (CommitPhase::Drafting, CommitWorkerResult::Draft(result)) => {
                if let Some(active) = self.commit_controller.active.as_mut() {
                    active.phase = CommitPhase::CommitReady;
                }
                if let Some(operation) = fence.operation {
                    let applied = fence
                        .route
                        .panel
                        .update(cx, |panel, cx| panel.finish_draft(operation, result, cx));
                    if applied {
                        self.record_commit_terminal_application(false);
                    }
                }
            }
            (CommitPhase::Committing, CommitWorkerResult::Commit(completion, _)) => {
                let error = match completion.outcome {
                    CommitOutcome::Committed if completion.workspace.is_some() => None,
                    CommitOutcome::Committed => Some(CommitErrorCode::ChangedDuringRead),
                    CommitOutcome::Failed(code) => Some(code),
                };
                if let Some(operation) = fence.operation {
                    let applied = fence
                        .route
                        .panel
                        .update(cx, |panel, cx| panel.finish_commit(operation, error, cx));
                    if applied {
                        self.record_commit_terminal_application(true);
                    }
                }
                if let Some(active) = self.commit_controller.active.take()
                    && self.trusted_actions.release(active.lease)
                {
                    active
                        .identity
                        .stream
                        .update(cx, |stream, cx| stream.set_trusted_action_busy(false, cx));
                    self.record_commit_probe("lease_release");
                }
            }
            (CommitPhase::Preparing, CommitWorkerResult::Recovered(code, _)) => {
                if let Some(active) = self.commit_controller.active.as_mut() {
                    active.phase = CommitPhase::Checklist;
                    active.prepared = None;
                }
                if let Some(operation) = fence.operation {
                    let applied = fence.route.panel.update(cx, |panel, cx| {
                        panel.finish_prepare(operation, Err(code), cx)
                    });
                    if applied {
                        self.record_commit_terminal_application(false);
                    }
                }
            }
            (CommitPhase::Committing, CommitWorkerResult::Recovered(code, _)) => {
                if let Some(operation) = fence.operation {
                    let applied = fence.route.panel.update(cx, |panel, cx| {
                        panel.finish_commit(operation, Some(code), cx)
                    });
                    if applied {
                        self.record_commit_terminal_application(false);
                    }
                }
                if let Some(active) = self.commit_controller.active.take()
                    && self.trusted_actions.release(active.lease)
                {
                    active
                        .identity
                        .stream
                        .update(cx, |stream, cx| stream.set_trusted_action_busy(false, cx));
                    self.record_commit_probe("lease_release");
                }
            }
            (CommitPhase::Preparing, CommitWorkerResult::RuntimeUnavailable(code)) => {
                if let Some(operation) = fence.operation {
                    let applied = fence.route.panel.update(cx, |panel, cx| {
                        panel.finish_prepare(operation, Err(code), cx)
                    });
                    if applied {
                        self.record_commit_terminal_application(false);
                    }
                }
                if let Some(active) = self.commit_controller.active.take()
                    && self.trusted_actions.release(active.lease)
                {
                    active
                        .identity
                        .stream
                        .update(cx, |stream, cx| stream.set_trusted_action_busy(false, cx));
                }
            }
            (CommitPhase::Committing, CommitWorkerResult::RuntimeUnavailable(code)) => {
                if let Some(operation) = fence.operation {
                    let applied = fence.route.panel.update(cx, |panel, cx| {
                        panel.finish_commit(operation, Some(code), cx)
                    });
                    if applied {
                        self.record_commit_terminal_application(false);
                    }
                }
                if let Some(active) = self.commit_controller.active.take()
                    && self.trusted_actions.release(active.lease)
                {
                    active
                        .identity
                        .stream
                        .update(cx, |stream, cx| stream.set_trusted_action_busy(false, cx));
                }
            }
            _ => self.close_commit_route(cx),
        }
    }

    fn open_commit_panel(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &OpenCommitPanelRequested,
        cx: &mut Context<Self>,
    ) {
        let Some(thread) = cx.global::<OpenedThread>().0.clone().filter(|thread| {
            thread.id == request.thread_id && thread.project_id == request.project_id
        }) else {
            return;
        };
        if self
            .stream_view
            .as_ref()
            .is_none_or(|(thread_id, current)| thread_id != &thread.id || current != &stream)
        {
            return;
        }
        if !self.commit_guards_clear(&stream, cx) || self.commit_controller.is_open() {
            return;
        }
        let Ok(root) = Self::artifact_project_root(&thread, cx) else {
            return;
        };
        let workspace = self
            .artifact_controller
            .active
            .as_ref()
            .filter(|active| {
                active.identity.project_id == thread.project_id
                    && active.identity.thread_id == thread.id
                    && active.identity.stream == stream
            })
            .map(|active| active.workspace.clone())
            .or_else(|| GitWorkspaceService::new(&root).ok().map(Arc::new));
        let Some(workspace) = workspace else {
            return;
        };
        let Ok(service) = TrustedGitService::new(&root, workspace.clone()).map(Arc::new) else {
            return;
        };
        let Some(epoch) = self.commit_controller.next_epoch.checked_add(1) else {
            return;
        };
        let Some(lease) = self
            .trusted_actions
            .acquire(TrustedActionKind::Commit, epoch, 1)
        else {
            return;
        };
        let panel = stream.read(cx).commit_panel();
        if !panel.update(cx, |panel, cx| panel.request_open(cx)) {
            let _ = self.trusted_actions.release(lease);
            return;
        }
        let identity = CommitRouteIdentity {
            epoch,
            thread_id: thread.id,
            project_id: thread.project_id,
            stream: stream.clone(),
            panel: panel.clone(),
        };
        let mut active = ActiveCommitRoute {
            identity,
            service: service.clone(),
            lease,
            next_sequence: 0,
            phase: CommitPhase::Checklist,
            snapshot: None,
            prepared: None,
            focus_pending: true,
            pending: None,
            cancel: None,
            terminal_done: None,
        };
        let Some((fence, cancel, terminal_done)) = CommitController::begin_fence(
            &mut active,
            CommitPhase::Checklist,
            None,
            CommitFenceAuthority::None,
        ) else {
            let _ = panel.update(cx, |panel, cx| {
                panel.apply_error(
                    vega_ui::commit_panel::CommitPanelStage::Loading,
                    CommitErrorCode::OutputTooLarge,
                    cx,
                )
            });
            let _ = self.trusted_actions.release(lease);
            return;
        };
        self.commit_controller.next_epoch = epoch;
        self.commit_controller.active = Some(active);
        stream.update(cx, |stream, cx| stream.set_trusted_action_busy(true, cx));
        let (sender, receiver) = mpsc::sync_channel(1);
        let window_alive = self.window_alive.clone();
        let actions = self.trusted_actions.clone();
        #[cfg(test)]
        let probe = self.commit_test_probe.clone();
        cx.background_executor()
            .spawn(async move {
                let result = run_commit_checklist_worker(
                    workspace,
                    service,
                    cancel,
                    #[cfg(test)]
                    probe,
                );
                mark_commit_worker_terminal_if_authoritative(
                    CommitPhase::Checklist,
                    &result,
                    terminal_done,
                    window_alive,
                    actions,
                    lease,
                );
                let _ = sender.send(result);
            })
            .detach();
        self.poll_commit_worker(fence, receiver, cx);
    }

    fn close_commit_route(&mut self, cx: &mut Context<Self>) {
        if let Some((lease, stream)) = self.commit_controller.retire_or_close()
            && self.trusted_actions.release(lease)
        {
            stream.update(cx, |stream, cx| stream.set_trusted_action_busy(false, cx));
        }
    }

    fn fail_commit_request_before_worker(
        &mut self,
        panel: &Entity<CommitPanel>,
        operation: CommitOperationId,
        cx: &mut Context<Self>,
    ) {
        let _ = panel.update(cx, |panel, cx| {
            panel.fail_pending(operation, CommitErrorCode::OutputTooLarge, cx)
        });
        self.close_commit_route(cx);
    }

    fn close_commit_if_route_stale(&mut self, cx: &mut Context<Self>) {
        let stale = self
            .commit_controller
            .active
            .as_ref()
            .is_some_and(|active| !self.commit_route_is_current(&active.identity, cx));
        if stale {
            if let Some(active) = self.commit_controller.active.as_ref() {
                active.identity.panel.update(cx, |panel, cx| {
                    let _ = panel.request_close(cx);
                });
            }
            self.close_commit_route(cx);
        }
    }

    fn request_commit_prepare(
        &mut self,
        panel: Entity<CommitPanel>,
        request: &CommitPrepareRequested,
        cx: &mut Context<Self>,
    ) {
        let route_current = self
            .commit_controller
            .active
            .as_ref()
            .is_some_and(|active| self.commit_route_is_current(&active.identity, cx));
        let Some(active) = self.commit_controller.active.as_mut() else {
            return;
        };
        if active.identity.panel != panel
            || active.identity.thread_id != request.thread_id
            || active.identity.project_id != request.project_id
            || active.phase != CommitPhase::Checklist
            || active.snapshot != Some(request.snapshot_id)
            || active.pending.is_some()
            || !panel.read(cx).owns_pending(request.operation_id)
            || !route_current
        {
            return;
        }
        let Some((fence, cancel, terminal_done)) = CommitController::begin_fence(
            active,
            CommitPhase::Preparing,
            Some(request.operation_id),
            CommitFenceAuthority::Snapshot(request.snapshot_id),
        ) else {
            self.fail_commit_request_before_worker(&panel, request.operation_id, cx);
            return;
        };
        let service = active.service.clone();
        let lease = active.lease;
        let snapshot_id = request.snapshot_id;
        let selected = request.selected.clone();
        let branch = self
            .branch_controller
            .active
            .as_ref()
            .filter(|branch| branch.identity.project_id == request.project_id)
            .map(|branch| branch.service.clone());
        let artifacts = self
            .artifact_controller
            .active
            .as_ref()
            .filter(|artifacts| {
                artifacts.identity.project_id == request.project_id
                    && Arc::ptr_eq(&artifacts.workspace, &service.workspace_service())
            })
            .map(|artifacts| artifacts.service.clone());
        let (sender, receiver) = mpsc::sync_channel(1);
        let window_alive = self.window_alive.clone();
        let actions = self.trusted_actions.clone();
        #[cfg(test)]
        let probe = self.commit_test_probe.clone();
        cx.background_executor()
            .spawn(async move {
                let result = run_commit_prepare_worker(
                    service,
                    snapshot_id,
                    selected,
                    cancel,
                    branch,
                    artifacts,
                    #[cfg(test)]
                    probe,
                );
                mark_commit_worker_terminal_if_authoritative(
                    CommitPhase::Preparing,
                    &result,
                    terminal_done,
                    window_alive,
                    actions,
                    lease,
                );
                let _ = sender.send(result);
            })
            .detach();
        self.poll_commit_worker(fence, receiver, cx);
    }

    fn request_commit_draft(
        &mut self,
        panel: Entity<CommitPanel>,
        request: &CommitDraftRequested,
        cx: &mut Context<Self>,
    ) {
        let Some(thread) = cx.global::<OpenedThread>().0.clone() else {
            return;
        };
        let route_current = self
            .commit_controller
            .active
            .as_ref()
            .is_some_and(|active| self.commit_route_is_current(&active.identity, cx));
        let Some(active) = self.commit_controller.active.as_mut() else {
            return;
        };
        if active.identity.panel != panel
            || active.identity.thread_id != request.thread_id
            || active.identity.project_id != request.project_id
            || active.phase != CommitPhase::CommitReady
            || active.prepared != Some(request.prepared_id)
            || active.pending.is_some()
            || !panel.read(cx).owns_pending(request.operation_id)
            || !route_current
        {
            return;
        }
        let Some((fence, cancel, terminal_done)) = CommitController::begin_fence(
            active,
            CommitPhase::Drafting,
            Some(request.operation_id),
            CommitFenceAuthority::Prepared(request.prepared_id),
        ) else {
            self.fail_commit_request_before_worker(&panel, request.operation_id, cx);
            return;
        };
        let service = active.service.clone();
        let lease = active.lease;
        let prepared_id = request.prepared_id;
        #[cfg(test)]
        let provider_override = self.commit_provider_override.clone();
        #[cfg(not(test))]
        let provider_override = None;
        let (sender, receiver) = mpsc::sync_channel(1);
        let window_alive = self.window_alive.clone();
        let actions = self.trusted_actions.clone();
        #[cfg(test)]
        let probe = self.commit_test_probe.clone();
        cx.background_executor()
            .spawn(async move {
                let result = run_commit_draft_worker(
                    service,
                    prepared_id,
                    thread,
                    cancel,
                    provider_override,
                    #[cfg(test)]
                    probe,
                );
                mark_commit_worker_terminal_if_authoritative(
                    CommitPhase::Drafting,
                    &result,
                    terminal_done,
                    window_alive,
                    actions,
                    lease,
                );
                let _ = sender.send(result);
            })
            .detach();
        self.poll_commit_worker(fence, receiver, cx);
    }

    fn request_commit_execute(
        &mut self,
        panel: Entity<CommitPanel>,
        request: &CommitRequested,
        cx: &mut Context<Self>,
    ) {
        let route_current = self
            .commit_controller
            .active
            .as_ref()
            .is_some_and(|active| self.commit_route_is_current(&active.identity, cx));
        let Some(active) = self.commit_controller.active.as_mut() else {
            return;
        };
        if active.identity.panel != panel
            || active.identity.thread_id != request.thread_id
            || active.identity.project_id != request.project_id
            || active.phase != CommitPhase::CommitReady
            || active.prepared != Some(request.prepared_id)
            || active.pending.is_some()
            || !panel.read(cx).owns_pending(request.operation_id)
            || !route_current
        {
            return;
        }
        let Some((fence, cancel, terminal_done)) = CommitController::begin_fence(
            active,
            CommitPhase::Committing,
            Some(request.operation_id),
            CommitFenceAuthority::Prepared(request.prepared_id),
        ) else {
            self.fail_commit_request_before_worker(&panel, request.operation_id, cx);
            return;
        };
        let service = active.service.clone();
        let lease = active.lease;
        let prepared_id = request.prepared_id;
        let message = request.message.clone();
        let branch = self
            .branch_controller
            .active
            .as_ref()
            .filter(|branch| branch.identity.project_id == request.project_id)
            .map(|branch| branch.service.clone());
        let artifacts = self
            .artifact_controller
            .active
            .as_ref()
            .filter(|artifacts| {
                artifacts.identity.project_id == request.project_id
                    && Arc::ptr_eq(&artifacts.workspace, &service.workspace_service())
            })
            .map(|artifacts| artifacts.service.clone());
        let (sender, receiver) = mpsc::sync_channel(1);
        let window_alive = self.window_alive.clone();
        let actions = self.trusted_actions.clone();
        #[cfg(test)]
        let probe = self.commit_test_probe.clone();
        cx.background_executor()
            .spawn(async move {
                #[cfg(test)]
                let disconnect_probe = probe.clone();
                let result = run_commit_execute_worker(
                    service,
                    prepared_id,
                    message,
                    cancel,
                    branch,
                    artifacts,
                    #[cfg(test)]
                    probe,
                );
                #[cfg(test)]
                if disconnect_probe
                    .as_ref()
                    .is_some_and(|probe| probe.drop_commit_sender.swap(false, Ordering::SeqCst))
                {
                    return;
                }
                mark_commit_worker_terminal_if_authoritative(
                    CommitPhase::Committing,
                    &result,
                    terminal_done,
                    window_alive,
                    actions,
                    lease,
                );
                let _ = sender.send(result);
            })
            .detach();
        self.poll_commit_worker(fence, receiver, cx);
    }

    fn commit_panel_closed(
        &mut self,
        panel: Entity<CommitPanel>,
        request: &CommitPanelClosed,
        cx: &mut Context<Self>,
    ) {
        if self
            .commit_controller
            .active
            .as_ref()
            .is_some_and(|active| {
                active.identity.panel == panel
                    && active.identity.thread_id == request.thread_id
                    && active.identity.project_id == request.project_id
            })
        {
            self.close_commit_route(cx);
        }
    }

    fn request_branch_switch(
        &mut self,
        selector: Entity<BranchSelector>,
        request: &BranchSwitchRequested,
        cx: &mut Context<Self>,
    ) {
        let identity = self
            .branch_controller
            .active
            .as_ref()
            .filter(|active| {
                Self::branch_route_is_current(&active.identity, cx)
                    && active.identity.selector == selector
                    && active.identity.thread_id == request.thread_id
                    && active.identity.project_id == request.project_id
                    && active.prepare_fence.is_none()
                    && active.switch_fence.is_none()
                    && selector.read(cx).owns_pending(
                        request.operation_id,
                        request.snapshot_generation,
                        request.branch_id,
                    )
                    && selector
                        .read(cx)
                        .contains_switchable(request.snapshot_generation, request.branch_id)
            })
            .map(|active| active.identity.clone());
        let Some(identity) = identity else {
            selector.update(cx, |selector, cx| {
                let _ = selector.reject_switch(
                    request.operation_id,
                    request.snapshot_generation,
                    request.branch_id,
                    GitWorkspaceErrorCode::StaleGeneration,
                    cx,
                );
            });
            return;
        };
        if !self.branch_guards_clear(&identity.stream, cx) {
            selector.update(cx, |selector, cx| {
                let _ = selector.reject_switch(
                    request.operation_id,
                    request.snapshot_generation,
                    request.branch_id,
                    GitWorkspaceErrorCode::BranchOperationInProgress,
                    cx,
                );
            });
            return;
        }
        let Some(sequence) = self
            .branch_controller
            .active
            .as_ref()
            .and_then(|active| active.switch_sequence.checked_add(1))
        else {
            self.close_branch_route(GitWorkspaceErrorCode::OutputTooLarge, cx);
            return;
        };
        let (fence, service, cancel) = {
            let Some(active) = self.branch_controller.active.as_mut() else {
                return;
            };
            active.switch_sequence = sequence;
            let fence = BranchPrepareFence {
                route: identity,
                sequence,
                snapshot_generation: request.snapshot_generation,
                branch_id: request.branch_id,
                operation_id: request.operation_id,
            };
            let cancel = active.cancel.child_token();
            active.prepare_fence = Some(fence.clone());
            active.switch_cancel = Some(cancel.clone());
            (fence, active.service.clone(), cancel)
        };
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker_fence = fence.clone();
        let worker = std::thread::Builder::new()
            .name("vega-branch-preflight".into())
            .spawn(move || run_branch_prepare_worker(service, worker_fence, cancel, sender));
        if worker.is_err() {
            self.finish_branch_prepare(fence, Err(GitWorkspaceErrorCode::SpawnFailed), cx);
            return;
        }
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(DIFF_RESULT_POLL).await;
                let (fence, result) = match receiver.try_recv() {
                    Ok(output) => output,
                    Err(mpsc::TryRecvError::Empty) => continue,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        (fence, Err(GitWorkspaceErrorCode::SpawnFailed))
                    }
                };
                let _ = this.update(cx, |this, cx| this.finish_branch_prepare(fence, result, cx));
                break;
            }
        })
        .detach();
    }

    fn finish_branch_prepare(
        &mut self,
        fence: BranchPrepareFence,
        result: Result<BranchSwitchPermit, GitWorkspaceErrorCode>,
        cx: &mut Context<Self>,
    ) {
        if !self.branch_controller.claim_prepare(&fence) {
            return;
        }
        let current = Self::branch_route_is_current(&fence.route, cx)
            && fence.route.selector.read(cx).is_open()
            && fence.route.selector.read(cx).is_pending()
            && fence.route.selector.read(cx).owns_pending(
                fence.operation_id,
                fence.snapshot_generation,
                fence.branch_id,
            )
            && fence
                .route
                .selector
                .read(cx)
                .contains_switchable(fence.snapshot_generation, fence.branch_id);
        if !current {
            if let Some(active) = self.branch_controller.active.as_mut()
                && active.identity == fence.route
            {
                active.switch_cancel = None;
            }
            fence.route.selector.update(cx, |selector, cx| {
                let _ = selector.clear_pending(
                    fence.operation_id,
                    fence.snapshot_generation,
                    fence.branch_id,
                    cx,
                );
            });
            return;
        }
        let permit = match result {
            Ok(permit) => permit,
            Err(code) => {
                if let Some(active) = self.branch_controller.active.as_mut()
                    && active.identity == fence.route
                {
                    active.switch_cancel = None;
                }
                fence.route.selector.update(cx, |selector, cx| {
                    let _ = selector.finish_switch(
                        fence.operation_id,
                        fence.snapshot_generation,
                        fence.branch_id,
                        None,
                        Some(code),
                        cx,
                    );
                });
                return;
            }
        };
        if !self.branch_guards_clear(&fence.route.stream, cx) {
            if let Some(active) = self.branch_controller.active.as_mut()
                && active.identity == fence.route
            {
                active.switch_cancel = None;
            }
            fence.route.selector.update(cx, |selector, cx| {
                let _ = selector.reject_switch(
                    fence.operation_id,
                    fence.snapshot_generation,
                    fence.branch_id,
                    GitWorkspaceErrorCode::BranchOperationInProgress,
                    cx,
                );
            });
            return;
        }
        let Some(lease) = self.trusted_actions.acquire(
            TrustedActionKind::BranchSwitch,
            fence.route.epoch,
            fence.sequence,
        ) else {
            if let Some(active) = self.branch_controller.active.as_mut()
                && active.identity == fence.route
            {
                active.switch_cancel = None;
            }
            fence.route.selector.update(cx, |selector, cx| {
                let _ = selector.reject_switch(
                    fence.operation_id,
                    fence.snapshot_generation,
                    fence.branch_id,
                    GitWorkspaceErrorCode::BranchOperationInProgress,
                    cx,
                );
            });
            return;
        };
        fence
            .route
            .stream
            .update(cx, |stream, cx| stream.set_trusted_action_busy(true, cx));
        if let Some(active) = self.diff_controller.active.as_mut() {
            if let Some(cancel) = active.projection_cancel.take() {
                cancel.cancel();
            }
            active.pending_projection = None;
        }
        if let Some(active) = self.artifact_controller.active.as_mut() {
            Self::cancel_artifact_interactions(active, cx);
        }
        let execute_fence = BranchSwitchFence {
            route: fence.route,
            sequence: fence.sequence,
            snapshot_generation: fence.snapshot_generation,
            branch_id: fence.branch_id,
            operation_id: fence.operation_id,
            lease,
        };
        let (service, cancel) = {
            let Some(active) = self.branch_controller.active.as_mut() else {
                let _ = self.trusted_actions.release(lease);
                execute_fence
                    .route
                    .stream
                    .update(cx, |stream, cx| stream.set_trusted_action_busy(false, cx));
                return;
            };
            let cancel = active.cancel.child_token();
            active.switch_fence = Some(execute_fence.clone());
            active.switch_cancel = Some(cancel.clone());
            (active.service.clone(), cancel)
        };
        self.launch_branch_execute(service, permit, execute_fence, cancel, cx);
    }

    fn launch_branch_execute(
        &mut self,
        service: Arc<BranchWorkspaceService>,
        permit: BranchSwitchPermit,
        fence: BranchSwitchFence,
        cancel: tokio_util::sync::CancellationToken,
        cx: &mut Context<Self>,
    ) {
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker_fence = fence.clone();
        let worker = std::thread::Builder::new()
            .name("vega-branch-switch".into())
            .spawn(move || run_branch_switch_worker(service, permit, worker_fence, cancel, sender));
        if worker.is_err() {
            self.finish_branch_switch(
                fence,
                BranchSwitchCompletion {
                    outcome: BranchSwitchOutcome::Failed(GitWorkspaceErrorCode::SpawnFailed),
                    snapshot: None,
                },
                cx,
            );
            return;
        }
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(DIFF_RESULT_POLL).await;
                let (fence, completion) = match receiver.try_recv() {
                    Ok(output) => output,
                    Err(mpsc::TryRecvError::Empty) => continue,
                    Err(mpsc::TryRecvError::Disconnected) => (
                        fence,
                        BranchSwitchCompletion {
                            outcome: BranchSwitchOutcome::Failed(
                                GitWorkspaceErrorCode::SpawnFailed,
                            ),
                            snapshot: None,
                        },
                    ),
                };
                let _ = this.update(cx, |this, cx| {
                    this.finish_branch_switch(fence, completion, cx)
                });
                break;
            }
        })
        .detach();
    }

    fn branch_selector_closed(
        &mut self,
        selector: Entity<BranchSelector>,
        request: &BranchSelectorClosed,
        _cx: &mut Context<Self>,
    ) {
        let Some(active) = self.branch_controller.active.as_mut() else {
            return;
        };
        if active.identity.selector != selector
            || active.identity.thread_id != request.thread_id
            || active.identity.project_id != request.project_id
        {
            return;
        }
        if let Some(cancel) = active.list_cancel.take() {
            cancel.cancel();
        }
        active.list_fence = None;
        if let Some(cancel) = &active.switch_cancel {
            // The owner future stays alive and performs its authoritative cleanup.
            cancel.cancel();
        }
    }

    fn enqueue_artifact_workspace_reconcile(&mut self, project_id: &str, cx: &mut Context<Self>) {
        let Some(active) = self.artifact_controller.active.as_mut() else {
            return;
        };
        if active.identity.project_id != project_id
            || !Self::artifact_route_is_current(&active.identity, cx)
        {
            return;
        }
        Self::cancel_artifact_interactions(active, cx);
        let Some(sequence) = active.terminal_sequence.checked_add(1) else {
            self.close_artifact_route(GitWorkspaceErrorCode::ArtifactLimit, cx);
            return;
        };
        if active.terminal_queue.len() >= ARTIFACT_ROUTE_CAP {
            self.close_artifact_route(GitWorkspaceErrorCode::ArtifactLimit, cx);
            return;
        }
        active.terminal_sequence = sequence;
        active.terminal_queue.push_back(ArtifactTerminalJob {
            sequence,
            work: ArtifactTerminalWork::Refresh,
        });
        self.launch_next_artifact_terminal(cx);
    }

    fn workspace_action_finished(&mut self, project_id: &str, cx: &mut Context<Self>) {
        let diff_identity = self
            .diff_controller
            .active
            .as_ref()
            .filter(|active| active.identity.project_id == project_id)
            .map(|active| active.identity.clone());
        if let Some(identity) = diff_identity {
            self.schedule_diff_refresh(&identity, cx);
        }
        self.enqueue_artifact_workspace_reconcile(project_id, cx);
    }

    fn apply_commit_workspace_reconciliation(
        &mut self,
        route: &CommitRouteIdentity,
        reconciled: &CommitWorkspaceReconciliation,
        cx: &mut Context<Self>,
    ) {
        let exact_stream = self
            .stream_view
            .as_ref()
            .is_some_and(|(thread_id, stream)| {
                thread_id == &route.thread_id
                    && stream == &route.stream
                    && stream.read(cx).commit_panel() == route.panel
            });
        if !exact_stream {
            return;
        }
        if let Some(active) = self.diff_controller.active.as_mut()
            && active.identity.project_id == route.project_id
            && active.identity.thread_id == route.thread_id
        {
            active.cancel.cancel();
            if let Some(cancel) = active.projection_cancel.take() {
                cancel.cancel();
            }
            active.cancel = tokio_util::sync::CancellationToken::new();
            active.service = Some(reconciled.workspace_service.clone());
            active.refresh_in_flight = None;
            active.queued_refresh_seq = None;
            active.requested_file = None;
            active.pending_projection = None;
            active.snapshot_generation = reconciled
                .workspace
                .as_ref()
                .ok()
                .map(|workspace| workspace.generation);
            active
                .view
                .update(cx, |view, cx| match &reconciled.workspace {
                    Ok(workspace) => {
                        view.set_refreshing(false, cx);
                        view.apply_snapshot(workspace.clone(), cx);
                    }
                    Err(code) => {
                        view.set_refreshing(false, cx);
                        view.apply_refresh_error(map_commit_reconcile_error(*code), cx);
                    }
                });
            self.record_commit_probe("ui_diff");
        }
        if let Some((service, snapshot)) = &reconciled.branch
            && let Some(active) = self.branch_controller.active.as_mut()
            && active.identity.project_id == route.project_id
            && active.identity.thread_id == route.thread_id
            && active.identity.stream == route.stream
            && Arc::ptr_eq(&active.service, service)
        {
            if let Some(cancel) = active.list_cancel.take() {
                cancel.cancel();
            }
            active.list_fence = None;
            active
                .identity
                .selector
                .update(cx, |selector, cx| match snapshot {
                    Ok(snapshot) => {
                        let _ = selector.apply_snapshot(snapshot.clone(), cx);
                    }
                    Err(code) => selector.apply_error(*code, cx),
                });
            self.record_commit_probe("ui_branch");
        }
        let mut artifact_failure = None;
        if let Some((service, cards)) = &reconciled.artifacts
            && let Some(active) = self.artifact_controller.active.as_mut()
            && active.identity.project_id == route.project_id
            && active.identity.thread_id == route.thread_id
            && active.identity.stream == route.stream
            && Arc::ptr_eq(&active.service, service)
        {
            Self::cancel_artifact_interactions(active, cx);
            match cards {
                Ok(cards) => {
                    for projection in cards {
                        if let Some(card) = active.cards.get(&projection.id) {
                            card.update(cx, |card, cx| {
                                let _ = card.apply_metadata(projection.clone(), cx);
                            });
                        }
                    }
                }
                Err(code) => artifact_failure = Some(*code),
            }
            self.record_commit_probe("ui_artifact");
        }
        if let Some(code) = artifact_failure {
            self.close_artifact_route(code, cx);
        }
    }

    fn finish_branch_switch(
        &mut self,
        fence: BranchSwitchFence,
        completion: BranchSwitchCompletion,
        cx: &mut Context<Self>,
    ) {
        if !self.branch_controller.claim_terminal(&fence) {
            return;
        }
        let error = match completion.outcome {
            BranchSwitchOutcome::Switched => None,
            BranchSwitchOutcome::Failed(code) => Some(code),
        };
        fence.route.selector.update(cx, |selector, cx| {
            let _ = selector.finish_switch(
                fence.operation_id,
                fence.snapshot_generation,
                fence.branch_id,
                completion.snapshot,
                error,
                cx,
            );
        });
        // A worker may have attempted mutation even after its route became stale.
        // Queue all conservative workspace reconciliation before releasing authority.
        self.workspace_action_finished(&fence.route.project_id, cx);
        if self.trusted_actions.release(fence.lease) {
            fence
                .route
                .stream
                .update(cx, |stream, cx| stream.set_trusted_action_busy(false, cx));
        }
    }

    fn ensure_artifact_route(
        &mut self,
        thread: &Thread,
        stream: Entity<ConversationStream>,
        cx: &mut Context<Self>,
    ) {
        let current = self
            .artifact_controller
            .active
            .as_ref()
            .is_some_and(|active| {
                active.identity.thread_id == thread.id
                    && active.identity.project_id == thread.project_id
                    && active.identity.stream == stream
            });
        if current {
            return;
        }
        self.close_artifact_route(GitWorkspaceErrorCode::StaleGeneration, cx);
        let result = Self::artifact_project_root(thread, cx).and_then(|root| {
            self.artifact_controller
                .begin(thread, stream, root)
                .map(|_| ())
        });
        if result.is_err() {
            self.close_artifact_route(GitWorkspaceErrorCode::InvalidRoot, cx);
        }
    }

    fn begin_artifact_agent_generation(
        &mut self,
        generation: u64,
        stream: &Entity<ConversationStream>,
    ) {
        if let Some(active) = self.artifact_controller.active.as_mut()
            && active.identity.stream == *stream
        {
            active.proposals.clear();
            active.agent_generation = Some(generation);
        }
    }

    fn poison_artifact_agent_generation(
        &mut self,
        generation: u64,
        stream: &Entity<ConversationStream>,
    ) {
        if let Some(active) = self.artifact_controller.active.as_mut()
            && active.identity.stream == *stream
            && active.agent_generation == Some(generation)
        {
            active.proposals.clear();
            active.agent_generation = None;
        }
    }

    /// Applies one drained batch through the production ownership boundary.
    /// The caller remains responsible for finished-run reload and UI recovery.
    fn apply_agent_batch_ingress(
        &mut self,
        generation: u64,
        thread_id: &str,
        stream: &Entity<ConversationStream>,
        batch: AgentBatch,
        cx: &mut Context<Self>,
    ) -> AgentBatchIngress {
        if !self.agent_controller.matches(generation, thread_id, stream) {
            return AgentBatchIngress::Stale;
        }
        for event in batch.events {
            self.observe_artifact_event(generation, stream, &event, cx);
            if matches!(event, ConversationEvent::MessageStarted { .. })
                && let Some(content) = self
                    .agent_controller
                    .accept_durable_start(generation, thread_id, stream)
            {
                stream.update(cx, |stream, cx| {
                    stream.accept_composer_submission(&content, cx)
                });
            }
            self.agent_controller
                .observe_terminal_message(generation, thread_id, stream, &event);
            stream.update(cx, |stream, cx| stream.apply_event(event, cx));
        }
        let Some(success) = batch.finished else {
            return AgentBatchIngress::Running;
        };
        self.poison_artifact_agent_generation(generation, stream);
        let Some(run) = self.agent_controller.finish(generation, thread_id, stream) else {
            return AgentBatchIngress::Stale;
        };
        // S7-T40/C4: the run's durable terminal message becomes a read-only
        // per-task cost summary card. The projection reads only the persisted
        // audits (a non-terminal/corrupt row fails closed → no card); the
        // duration is the live in-memory wall-clock measurement and degrades
        // to `—` after a restart.
        if let Some(message_id) = &run.terminal_message_id {
            let duration_ms = u64::try_from(run.started.elapsed().as_millis()).ok();
            let projected = match &cx.global::<VegaStore>().0 {
                Ok(store) => vega_conversation::summary::task_cost_summary(
                    store,
                    thread_id,
                    message_id,
                    duration_ms,
                )
                .ok(),
                Err(_) => None,
            };
            if let Some(summary) = projected {
                stream.update(cx, |stream, cx| stream.apply_task_summary(summary, cx));
            }
        }
        AgentBatchIngress::Finished { success, run }
    }

    fn cancel_artifact_interactions(active: &mut ActiveArtifactRoute, cx: &mut App) {
        let preview_card = active.preview_fence.take().map(|fence| fence.card_id);
        let open_card = active.open_fence.take().map(|fence| fence.card_id);
        if let Some(cancel) = active.preview_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = active.open_cancel.take() {
            cancel.cancel();
        }
        for card_id in [preview_card, open_card].into_iter().flatten() {
            if let Some(card) = active.cards.get(&card_id) {
                card.update(cx, |card, cx| {
                    card.fail_request(GitWorkspaceErrorCode::StaleGeneration, cx)
                });
            }
        }
    }

    /// The only production artifact capture ingress: real AgentBatch events
    /// are observed here before ownership moves to ConversationStream.
    fn observe_artifact_event(
        &mut self,
        generation: u64,
        stream: &Entity<ConversationStream>,
        event: &ConversationEvent,
        cx: &mut Context<Self>,
    ) {
        let current = self
            .artifact_controller
            .active
            .as_ref()
            .is_some_and(|active| {
                active.identity.stream == *stream
                    && active.agent_generation == Some(generation)
                    && Self::artifact_route_is_current(&active.identity, cx)
            });
        if !current {
            return;
        }
        let Some(active) = self.artifact_controller.active.as_mut() else {
            return;
        };
        match event {
            ConversationEvent::ToolCallProposed { call }
                if matches!(call.tool.as_str(), "write" | "edit") =>
            {
                if let Err(failure) = ArtifactService::validate_proposal(call) {
                    self.close_artifact_route(failure.code(), cx);
                    return;
                }
                if let Some(existing) = active.proposals.get_mut(&call.id) {
                    if existing.generation != generation || existing.call.as_ref() != Some(call) {
                        existing.call = None;
                    }
                } else if active.proposals.len() < ARTIFACT_ROUTE_CAP {
                    active.proposals.insert(
                        call.id.clone(),
                        ArtifactProposal {
                            generation,
                            call: Some(call.clone()),
                        },
                    );
                } else {
                    self.close_artifact_route(GitWorkspaceErrorCode::ArtifactLimit, cx);
                }
            }
            ConversationEvent::ToolCallFinished { call_id, result } => {
                Self::cancel_artifact_interactions(active, cx);
                let call = active
                    .proposals
                    .remove(call_id)
                    .filter(|proposal| proposal.generation == generation)
                    .and_then(|proposal| proposal.call);
                let work = if let Some(call) = call {
                    match active.service.prepare_capture(&call, result) {
                        Ok(Some(candidate)) => ArtifactTerminalWork::Capture {
                            call_id: call_id.clone(),
                            candidate,
                        },
                        Ok(None) => ArtifactTerminalWork::Refresh,
                        Err(failure) => {
                            let code = failure.code();
                            self.close_artifact_route(code, cx);
                            return;
                        }
                    }
                } else {
                    ArtifactTerminalWork::Refresh
                };
                let Some(sequence) = active.terminal_sequence.checked_add(1) else {
                    self.close_artifact_route(GitWorkspaceErrorCode::ArtifactLimit, cx);
                    return;
                };
                if active.terminal_queue.len() >= ARTIFACT_ROUTE_CAP {
                    self.close_artifact_route(GitWorkspaceErrorCode::ArtifactLimit, cx);
                    return;
                }
                active.terminal_sequence = sequence;
                active
                    .terminal_queue
                    .push_back(ArtifactTerminalJob { sequence, work });
                self.launch_next_artifact_terminal(cx);
            }
            _ => {}
        }
    }

    fn launch_next_artifact_terminal(&mut self, cx: &mut Context<Self>) {
        let Some(dispatch) = self.take_next_artifact_terminal() else {
            return;
        };
        let ArtifactTerminalDispatch {
            identity,
            workspace,
            service,
            job,
            cancel,
        } = dispatch;
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker = std::thread::Builder::new()
            .name("vega-artifact-terminal".into())
            .spawn(move || run_artifact_terminal_worker(workspace, service, job, cancel, sender));
        if worker.is_err() {
            self.finish_artifact_terminal(&identity, Err(GitWorkspaceErrorCode::SpawnFailed), cx);
            return;
        }
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(DIFF_RESULT_POLL).await;
                let result = match receiver.try_recv() {
                    Ok(result) => Some(result),
                    Err(mpsc::TryRecvError::Empty) => None,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        Some(Err(GitWorkspaceErrorCode::SpawnFailed))
                    }
                };
                let Some(result) = result else {
                    continue;
                };
                let _ = this.update(cx, |this, cx| {
                    this.finish_artifact_terminal(&identity, result, cx)
                });
                break;
            }
        })
        .detach();
    }

    fn take_next_artifact_terminal(&mut self) -> Option<ArtifactTerminalDispatch> {
        let active = self.artifact_controller.active.as_mut()?;
        if active.terminal_in_flight.is_some() {
            return None;
        }
        let job = active.terminal_queue.pop_front()?;
        active.terminal_in_flight = Some(job.sequence);
        Some(ArtifactTerminalDispatch {
            identity: active.identity.clone(),
            workspace: active.workspace.clone(),
            service: active.service.clone(),
            job,
            cancel: active.cancel.child_token(),
        })
    }

    fn finish_artifact_terminal(
        &mut self,
        identity: &ArtifactRouteIdentity,
        result: Result<(u64, ArtifactTerminalResult), GitWorkspaceErrorCode>,
        cx: &mut Context<Self>,
    ) {
        if !Self::artifact_route_is_current(identity, cx) {
            if self.artifact_controller.matches(identity) {
                self.close_artifact_route(GitWorkspaceErrorCode::StaleGeneration, cx);
            }
            return;
        }
        let expected = self
            .artifact_controller
            .active
            .as_ref()
            .filter(|active| active.identity == *identity)
            .and_then(|active| active.terminal_in_flight);
        let sequence = result.as_ref().ok().map(|(sequence, _)| *sequence);
        if sequence.is_some() && sequence != expected {
            return;
        }
        if let Err(
            code @ (GitWorkspaceErrorCode::ArtifactConflict | GitWorkspaceErrorCode::ArtifactLimit),
        ) = &result
        {
            self.close_artifact_route(*code, cx);
            return;
        }
        let stream = {
            let Some(active) = self.artifact_controller.active.as_mut() else {
                return;
            };
            if active.identity != *identity {
                return;
            }
            active.terminal_in_flight = None;
            active.identity.stream.clone()
        };
        if let Ok((_, result)) = result {
            for projection in result.cards {
                let card = self
                    .artifact_controller
                    .active
                    .as_ref()
                    .and_then(|active| active.cards.get(&projection.id).cloned());
                if let Some(card) = card {
                    card.update(cx, |card, cx| {
                        let _ = card.apply_metadata(projection, cx);
                    });
                }
            }
            if let Some((call_id, projection)) = result.captured {
                let existing = self
                    .artifact_controller
                    .active
                    .as_ref()
                    .and_then(|active| active.cards.get(&projection.id).cloned());
                if let Some(card) = existing {
                    card.update(cx, |card, cx| {
                        let _ = card.apply_metadata(projection, cx);
                    });
                } else {
                    let card = cx.new(|cx| {
                        ArtifactCard::new(
                            identity.thread_id.clone(),
                            identity.project_id.clone(),
                            projection.clone(),
                            cx,
                        )
                    });
                    cx.subscribe(&card, |this, card, request, cx| {
                        this.request_artifact_preview(card.clone(), request, cx);
                    })
                    .detach();
                    cx.subscribe(&card, |this, card, request, cx| {
                        this.request_artifact_open(card.clone(), request, cx);
                    })
                    .detach();
                    cx.subscribe(&card, |this, card, request, cx| {
                        this.clear_artifact_requests(card.clone(), request, cx);
                    })
                    .detach();
                    if stream.update(cx, |stream, cx| {
                        stream.apply_artifact_card(&call_id, card.clone(), cx)
                    }) && let Some(active) = self.artifact_controller.active.as_mut()
                    {
                        active.cards.insert(projection.id, card);
                    }
                }
            }
        }
        self.launch_next_artifact_terminal(cx);
    }

    fn request_artifact_preview(
        &mut self,
        card: Entity<ArtifactCard>,
        request: &ArtifactPreviewRequested,
        cx: &mut Context<Self>,
    ) {
        let route_current = self
            .artifact_controller
            .active
            .as_ref()
            .is_some_and(|active| Self::artifact_route_is_current(&active.identity, cx));
        if !route_current {
            card.update(cx, |card, cx| {
                card.invalidate(GitWorkspaceErrorCode::StaleGeneration, cx)
            });
            self.close_artifact_route(GitWorkspaceErrorCode::StaleGeneration, cx);
            return;
        }
        if self.trusted_actions.is_busy() {
            card.update(cx, |card, cx| {
                card.fail_request(GitWorkspaceErrorCode::BranchOperationInProgress, cx)
            });
            return;
        }
        let (fence, service, cancel) = {
            let Some(active) = self.artifact_controller.active.as_mut() else {
                card.update(cx, |card, cx| {
                    card.invalidate(GitWorkspaceErrorCode::StaleGeneration, cx)
                });
                return;
            };
            let current = active.identity.thread_id == request.thread_id
                && active.identity.project_id == request.project_id
                && active.cards.get(&request.card_id) == Some(&card)
                && card.read(cx).projection().current_file_id == Some(request.file_id)
                && card.read(cx).projection().preview_available;
            if !current {
                card.update(cx, |card, cx| {
                    card.invalidate(GitWorkspaceErrorCode::StaleGeneration, cx)
                });
                return;
            }
            let Some(sequence) = active.preview_sequence.checked_add(1) else {
                self.close_artifact_route(GitWorkspaceErrorCode::ArtifactLimit, cx);
                return;
            };
            active.preview_sequence = sequence;
            if let Some(cancel) = active.preview_cancel.take() {
                cancel.cancel();
            }
            let fence = ArtifactPreviewFence {
                route: active.identity.clone(),
                sequence,
                card_id: request.card_id,
                file_id: request.file_id,
            };
            let cancel = active.cancel.child_token();
            active.preview_fence = Some(fence.clone());
            active.preview_cancel = Some(cancel.clone());
            (fence, active.service.clone(), cancel)
        };
        let (sender, receiver) = mpsc::sync_channel(1);
        #[cfg(test)]
        ARTIFACT_PREVIEW_WORKER_STARTS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let worker_fence = fence.clone();
        let worker = std::thread::Builder::new()
            .name("vega-artifact-preview".into())
            .spawn(move || run_artifact_preview_worker(service, worker_fence, cancel, sender));
        if worker.is_err() {
            self.finish_artifact_preview(fence, Err(GitWorkspaceErrorCode::SpawnFailed), cx);
            return;
        }
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(DIFF_RESULT_POLL).await;
                let (fence, result) = match receiver.try_recv() {
                    Ok(output) => output,
                    Err(mpsc::TryRecvError::Empty) => continue,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        (fence, Err(GitWorkspaceErrorCode::SpawnFailed))
                    }
                };
                let _ = this.update(cx, |this, cx| {
                    this.finish_artifact_preview(fence, result, cx)
                });
                break;
            }
        })
        .detach();
    }

    fn finish_artifact_preview(
        &mut self,
        fence: ArtifactPreviewFence,
        result: Result<ArtifactPreviewProjection, GitWorkspaceErrorCode>,
        cx: &mut Context<Self>,
    ) {
        if !Self::artifact_route_is_current(&fence.route, cx) {
            return;
        }
        let card = {
            let Some(active) = self.artifact_controller.active.as_mut() else {
                return;
            };
            if active.preview_fence.as_ref() != Some(&fence) {
                return;
            }
            active.preview_fence = None;
            active.preview_cancel = None;
            active.cards.get(&fence.card_id).cloned()
        };
        let Some(card) = card else {
            return;
        };
        if card.read(cx).projection().current_file_id != Some(fence.file_id) {
            card.update(cx, |card, cx| {
                card.invalidate(GitWorkspaceErrorCode::StaleGeneration, cx)
            });
            return;
        }
        match result {
            Ok(preview) => {
                card.update(cx, |card, cx| {
                    let _ = card.apply_preview(preview, cx);
                });
            }
            Err(GitWorkspaceErrorCode::Cancelled | GitWorkspaceErrorCode::StaleGeneration) => {}
            Err(code) => {
                card.update(cx, |card, cx| {
                    let _ = card.apply_preview_error(fence.card_id, fence.file_id, code, cx);
                });
            }
        }
    }

    fn request_artifact_open(
        &mut self,
        card: Entity<ArtifactCard>,
        request: &ArtifactOpenRequested,
        cx: &mut Context<Self>,
    ) {
        let route_current = self
            .artifact_controller
            .active
            .as_ref()
            .is_some_and(|active| Self::artifact_route_is_current(&active.identity, cx));
        if !route_current {
            card.update(cx, |card, cx| {
                card.invalidate(GitWorkspaceErrorCode::StaleGeneration, cx)
            });
            self.close_artifact_route(GitWorkspaceErrorCode::StaleGeneration, cx);
            return;
        }
        let sequence_overflow = self
            .artifact_controller
            .active
            .as_ref()
            .is_some_and(|active| {
                active.identity.thread_id == request.thread_id
                    && active.identity.project_id == request.project_id
                    && active.cards.get(&request.card_id) == Some(&card)
                    && card.read(cx).projection().current_file_id == Some(request.file_id)
                    && active.open_fence.is_none()
                    && active.open_sequence == u64::MAX
            });
        if sequence_overflow {
            card.update(cx, |card, cx| {
                let _ = card.apply_open_error(
                    request.card_id,
                    request.target,
                    GitWorkspaceErrorCode::ArtifactLimit,
                    cx,
                );
            });
            self.close_artifact_route(GitWorkspaceErrorCode::ArtifactLimit, cx);
            return;
        }
        let lease_input = self.artifact_controller.active.as_ref().and_then(|active| {
            let current = active.identity.thread_id == request.thread_id
                && active.identity.project_id == request.project_id
                && active.cards.get(&request.card_id) == Some(&card)
                && card.read(cx).projection().current_file_id == Some(request.file_id)
                && active.open_fence.is_none();
            current
                .then(|| {
                    active
                        .open_sequence
                        .checked_add(1)
                        .map(|sequence| (active.identity.clone(), sequence))
                })
                .flatten()
        });
        let Some((open_identity, open_sequence)) = lease_input else {
            let owned = self
                .artifact_controller
                .active
                .as_ref()
                .is_some_and(|active| {
                    active.identity.thread_id == request.thread_id
                        && active.identity.project_id == request.project_id
                        && active.cards.get(&request.card_id) == Some(&card)
                        && card.read(cx).projection().current_file_id == Some(request.file_id)
                });
            if owned {
                card.update(cx, |card, cx| {
                    card.fail_request(GitWorkspaceErrorCode::BranchOperationInProgress, cx)
                });
            } else {
                card.update(cx, |card, cx| {
                    card.invalidate(GitWorkspaceErrorCode::StaleGeneration, cx)
                });
            }
            return;
        };
        if !self.branch_guards_clear(&open_identity.stream, cx) {
            card.update(cx, |card, cx| {
                card.fail_request(GitWorkspaceErrorCode::BranchOperationInProgress, cx)
            });
            return;
        }
        let Some(lease) = self.trusted_actions.acquire(
            TrustedActionKind::ArtifactOpen,
            open_identity.epoch,
            open_sequence,
        ) else {
            card.update(cx, |card, cx| {
                card.fail_request(GitWorkspaceErrorCode::BranchOperationInProgress, cx)
            });
            return;
        };
        open_identity
            .stream
            .update(cx, |stream, cx| stream.set_trusted_action_busy(true, cx));
        let (fence, service, cancel) = {
            let Some(active) = self.artifact_controller.active.as_mut() else {
                let _ = self.trusted_actions.release(lease);
                open_identity
                    .stream
                    .update(cx, |stream, cx| stream.set_trusted_action_busy(false, cx));
                card.update(cx, |card, cx| {
                    card.invalidate(GitWorkspaceErrorCode::StaleGeneration, cx)
                });
                return;
            };
            let current = active.identity.thread_id == request.thread_id
                && active.identity.project_id == request.project_id
                && active.cards.get(&request.card_id) == Some(&card)
                && card.read(cx).projection().current_file_id == Some(request.file_id)
                && active.open_fence.is_none();
            if !current {
                let _ = self.trusted_actions.release(lease);
                open_identity
                    .stream
                    .update(cx, |stream, cx| stream.set_trusted_action_busy(false, cx));
                card.update(cx, |card, cx| {
                    card.invalidate(GitWorkspaceErrorCode::StaleGeneration, cx)
                });
                return;
            }
            let sequence = open_sequence;
            active.open_sequence = sequence;
            if let Some(cancel) = active.open_cancel.take() {
                cancel.cancel();
            }
            let fence = ArtifactOpenFence {
                route: active.identity.clone(),
                sequence,
                card_id: request.card_id,
                file_id: request.file_id,
                target: request.target,
                lease,
            };
            let cancel = active.cancel.child_token();
            active.open_fence = Some(fence.clone());
            active.open_cancel = Some(cancel.clone());
            (fence, active.service.clone(), cancel)
        };
        let (sender, receiver) = mpsc::sync_channel(1);
        #[cfg(test)]
        ARTIFACT_OPEN_WORKER_STARTS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let worker_fence = fence.clone();
        let worker = std::thread::Builder::new()
            .name("vega-artifact-open".into())
            .spawn(move || run_artifact_open_worker(service, worker_fence, cancel, sender));
        if worker.is_err() {
            self.finish_artifact_open(fence, Err(GitWorkspaceErrorCode::SpawnFailed), cx);
            return;
        }
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(DIFF_RESULT_POLL).await;
                let (fence, result) = match receiver.try_recv() {
                    Ok(output) => output,
                    Err(mpsc::TryRecvError::Empty) => continue,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        (fence, Err(GitWorkspaceErrorCode::SpawnFailed))
                    }
                };
                let _ = this.update(cx, |this, cx| this.finish_artifact_open(fence, result, cx));
                break;
            }
        })
        .detach();
    }

    fn finish_artifact_open(
        &mut self,
        fence: ArtifactOpenFence,
        result: Result<OpenInOutcome, GitWorkspaceErrorCode>,
        cx: &mut Context<Self>,
    ) {
        let released = self.trusted_actions.release(fence.lease);
        if released {
            fence
                .route
                .stream
                .update(cx, |stream, cx| stream.set_trusted_action_busy(false, cx));
        }
        if !Self::artifact_route_is_current(&fence.route, cx) {
            return;
        }
        let card = {
            let Some(active) = self.artifact_controller.active.as_mut() else {
                return;
            };
            if active.open_fence.as_ref() != Some(&fence) {
                return;
            }
            active.open_fence = None;
            active.open_cancel = None;
            active.cards.get(&fence.card_id).cloned()
        };
        let Some(card) = card else {
            return;
        };
        if card.read(cx).projection().current_file_id != Some(fence.file_id) {
            card.update(cx, |card, cx| {
                card.invalidate(GitWorkspaceErrorCode::StaleGeneration, cx)
            });
            return;
        }
        match result {
            Ok(outcome) => {
                card.update(cx, |card, cx| {
                    let _ = card.apply_open_outcome(outcome, cx);
                });
            }
            Err(GitWorkspaceErrorCode::Cancelled | GitWorkspaceErrorCode::StaleGeneration) => {
                card.update(cx, |card, cx| card.set_opening(None, cx));
            }
            Err(code) => {
                card.update(cx, |card, cx| {
                    let _ = card.apply_open_error(fence.card_id, fence.target, code, cx);
                });
            }
        }
    }

    fn clear_artifact_requests(
        &mut self,
        card: Entity<ArtifactCard>,
        request: &ArtifactCleared,
        _cx: &mut Context<Self>,
    ) {
        let Some(active) = self.artifact_controller.active.as_mut() else {
            return;
        };
        if active.identity.thread_id != request.thread_id
            || active.identity.project_id != request.project_id
            || active.cards.get(&request.card_id) != Some(&card)
        {
            return;
        }
        if active
            .preview_fence
            .as_ref()
            .is_some_and(|fence| fence.card_id == request.card_id)
        {
            if let Some(cancel) = active.preview_cancel.take() {
                cancel.cancel();
            }
            active.preview_fence = None;
        }
        if active
            .open_fence
            .as_ref()
            .is_some_and(|fence| fence.card_id == request.card_id)
        {
            if let Some(cancel) = active.open_cancel.take() {
                cancel.cancel();
            }
            active.open_fence = None;
        }
    }

    fn diff_route_is_current(identity: &DiffRouteIdentity, cx: &App) -> bool {
        !cx.global::<SettingsOpen>().0
            && cx
                .global::<OpenedThread>()
                .0
                .as_ref()
                .is_some_and(|thread| {
                    thread.id == identity.thread_id && thread.project_id == identity.project_id
                })
    }

    fn close_diff_if_route_stale(&mut self, cx: &mut Context<Self>) {
        let stale = self
            .diff_controller
            .active
            .as_ref()
            .is_some_and(|active| !Self::diff_route_is_current(&active.identity, cx));
        if stale {
            self.diff_controller.close();
            cx.notify();
        }
    }

    fn diff_project_root(
        &self,
        identity: &DiffRouteIdentity,
        cx: &App,
    ) -> Result<PathBuf, GitWorkspaceErrorCode> {
        let thread_matches = cx
            .global::<OpenedThread>()
            .0
            .as_ref()
            .is_some_and(|thread| {
                thread.id == identity.thread_id && thread.project_id == identity.project_id
            });
        if !thread_matches {
            return Err(GitWorkspaceErrorCode::Cancelled);
        }
        let store = cx
            .global::<VegaStore>()
            .0
            .as_ref()
            .map_err(|_| GitWorkspaceErrorCode::InvalidRoot)?;
        let project = vega_store::projects::find(store.conn(), &identity.project_id)
            .map_err(|_| GitWorkspaceErrorCode::InvalidRoot)?
            .ok_or(GitWorkspaceErrorCode::InvalidRoot)?;
        Ok(PathBuf::from(project.path))
    }

    fn open_workspace_diff(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &OpenWorkspaceDiffRequested,
        cx: &mut Context<Self>,
    ) {
        if !self.owns_stream_request(&stream, &request.thread_id, cx) {
            return;
        }
        let project_matches = cx
            .global::<OpenedThread>()
            .0
            .as_ref()
            .is_some_and(|thread| thread.project_id == request.project_id);
        if !project_matches {
            return;
        }
        let view =
            cx.new(|cx| DiffView::new(request.thread_id.clone(), request.project_id.clone(), cx));
        cx.subscribe(&view, |this, view, request, cx| {
            this.request_diff_projection(view.clone(), request, cx);
        })
        .detach();
        cx.subscribe(&view, |this, view, request, cx| {
            this.retry_workspace_diff(view.clone(), request, cx);
        })
        .detach();
        cx.subscribe(&view, |this, view, request, cx| {
            this.close_workspace_diff(view.clone(), request, cx);
        })
        .detach();
        let Some(identity) = self.diff_controller.begin(
            request.thread_id.clone(),
            request.project_id.clone(),
            view.clone(),
        ) else {
            view.update(cx, |view, cx| {
                view.apply_refresh_error(GitWorkspaceErrorCode::OutputTooLarge, cx)
            });
            return;
        };
        self.schedule_diff_refresh(&identity, cx);
        self.start_diff_poll(identity, view, cx);
        cx.notify();
    }

    fn start_diff_poll(
        &mut self,
        identity: DiffRouteIdentity,
        view: Entity<DiffView>,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(DIFF_REFRESH_INTERVAL).await;
                let keep_polling = this
                    .update(cx, |this, cx| {
                        let visible = this.diff_controller.active.as_ref().is_some_and(|active| {
                            active.identity == identity && active.view == view
                        }) && Self::diff_route_is_current(&identity, cx);
                        if visible {
                            this.schedule_diff_refresh(&identity, cx);
                        } else if this.diff_controller.matches(&identity) {
                            this.diff_controller.close();
                            cx.notify();
                        }
                        visible
                    })
                    .unwrap_or(false);
                if !keep_polling {
                    break;
                }
            }
        })
        .detach();
    }

    fn schedule_diff_refresh(&mut self, identity: &DiffRouteIdentity, cx: &mut Context<Self>) {
        if !Self::diff_route_is_current(identity, cx) {
            if self.diff_controller.matches(identity) {
                self.diff_controller.close();
                cx.notify();
            }
            return;
        }
        let request_seq = {
            let Some(active) = self.diff_controller.active.as_mut() else {
                return;
            };
            if active.identity != *identity {
                return;
            }
            let request_seq = match active.request_refresh() {
                DiffRefreshDecision::Start(request_seq) => request_seq,
                DiffRefreshDecision::Coalesced => return,
                DiffRefreshDecision::Overflow => {
                    self.diff_controller.close();
                    cx.notify();
                    return;
                }
            };
            active
                .view
                .update(cx, |view, cx| view.set_refreshing(true, cx));
            request_seq
        };
        self.launch_diff_refresh(identity, request_seq, cx);
    }

    fn launch_diff_refresh(
        &mut self,
        identity: &DiffRouteIdentity,
        request_seq: u64,
        cx: &mut Context<Self>,
    ) {
        if !Self::diff_route_is_current(identity, cx) {
            if self.diff_controller.matches(identity) {
                self.diff_controller.close();
                cx.notify();
            }
            return;
        }
        let (service, cancel) = {
            let Some(active) = self.diff_controller.active.as_ref() else {
                return;
            };
            if active.identity != *identity || active.refresh_in_flight != Some(request_seq) {
                return;
            }
            (active.service.clone(), active.cancel.child_token())
        };
        let root = if service.is_none() {
            match self.diff_project_root(identity, cx) {
                Ok(root) => Some(root),
                Err(code) => {
                    self.finish_diff_refresh(
                        identity,
                        request_seq,
                        DiffRefreshWorkerResult::Failed(code),
                        cx,
                    );
                    return;
                }
            }
        } else {
            None
        };
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker = std::thread::Builder::new()
            .name("vega-diff-refresh".into())
            .spawn(move || run_diff_refresh_worker(service, root, cancel, sender));
        if worker.is_err() {
            self.finish_diff_refresh(
                identity,
                request_seq,
                DiffRefreshWorkerResult::Failed(GitWorkspaceErrorCode::SpawnFailed),
                cx,
            );
            return;
        }
        let identity = identity.clone();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(DIFF_RESULT_POLL).await;
                let result = match receiver.try_recv() {
                    Ok(result) => Some(result),
                    Err(mpsc::TryRecvError::Empty) => None,
                    Err(mpsc::TryRecvError::Disconnected) => Some(DiffRefreshWorkerResult::Failed(
                        GitWorkspaceErrorCode::SpawnFailed,
                    )),
                };
                let Some(result) = result else {
                    continue;
                };
                let _ = this.update(cx, |this, cx| {
                    this.finish_diff_refresh(&identity, request_seq, result, cx)
                });
                break;
            }
        })
        .detach();
    }

    fn finish_diff_refresh(
        &mut self,
        identity: &DiffRouteIdentity,
        request_seq: u64,
        result: DiffRefreshWorkerResult,
        cx: &mut Context<Self>,
    ) {
        if !Self::diff_route_is_current(identity, cx) {
            if self.diff_controller.matches(identity) {
                self.diff_controller.close();
                cx.notify();
            }
            return;
        }
        enum RefreshUi {
            Snapshot(Entity<DiffView>, WorkspaceSnapshot),
            Error(Entity<DiffView>, GitWorkspaceErrorCode),
            Drop(Entity<DiffView>),
        }

        let completion = {
            let Some(active) = self.diff_controller.active.as_mut() else {
                return;
            };
            if active.identity != *identity {
                return;
            }
            let Some(completion) = active.complete_refresh(request_seq) else {
                return;
            };
            completion
        };
        if let DiffRefreshCompletion::Superseded(rerun_seq) = completion {
            if let DiffRefreshWorkerResult::Ready { service, .. } = result
                && let Some(active) = self.diff_controller.active.as_mut()
                && active.identity == *identity
            {
                active.service = Some(service);
            }
            if let Some(next) = rerun_seq {
                self.launch_diff_refresh(identity, next, cx);
            }
            return;
        }

        let (ui, pending) = {
            let Some(active) = self.diff_controller.active.as_mut() else {
                return;
            };
            if active.identity != *identity {
                return;
            }
            let view = active.view.clone();
            let mut pending = None;
            let ui = match result {
                DiffRefreshWorkerResult::Ready { service, snapshot } => {
                    let generation_changed =
                        active.snapshot_generation != Some(snapshot.generation);
                    active.service = Some(service);
                    active.snapshot_generation = Some(snapshot.generation);
                    if generation_changed {
                        if let Some(cancel) = active.projection_cancel.take() {
                            cancel.cancel();
                        }
                        active.requested_file = None;
                        active.pending_projection = None;
                    } else {
                        pending = active.pending_projection.take();
                    }
                    RefreshUi::Snapshot(view, snapshot)
                }
                DiffRefreshWorkerResult::Failed(
                    GitWorkspaceErrorCode::Cancelled | GitWorkspaceErrorCode::StaleGeneration,
                ) => RefreshUi::Drop(view),
                DiffRefreshWorkerResult::Failed(code) => {
                    active.snapshot_generation = None;
                    active.requested_file = None;
                    active.pending_projection = None;
                    if let Some(cancel) = active.projection_cancel.take() {
                        cancel.cancel();
                    }
                    RefreshUi::Error(view, code)
                }
            };
            (ui, pending)
        };
        match ui {
            RefreshUi::Snapshot(view, snapshot) => view.update(cx, |view, cx| {
                view.set_refreshing(false, cx);
                view.apply_snapshot(snapshot, cx);
            }),
            RefreshUi::Error(view, code) => view.update(cx, |view, cx| {
                view.set_refreshing(false, cx);
                view.apply_refresh_error(code, cx);
            }),
            RefreshUi::Drop(view) => {
                view.update(cx, |view, cx| view.set_refreshing(false, cx));
            }
        }
        if let Some(pending) = pending {
            self.apply_diff_projection_result(pending.fence, pending.result, cx);
        }
    }

    fn request_diff_projection(
        &mut self,
        view: Entity<DiffView>,
        request: &DiffProjectionRequested,
        cx: &mut Context<Self>,
    ) {
        let route_is_current = self.diff_controller.active.as_ref().is_some_and(|active| {
            active.view == view && Self::diff_route_is_current(&active.identity, cx)
        });
        if !route_is_current {
            self.close_diff_if_route_stale(cx);
            return;
        }
        let sequence_exhausted = self
            .diff_controller
            .active
            .as_ref()
            .is_some_and(|active| active.file_request_seq == u64::MAX);
        if sequence_exhausted {
            self.diff_controller.close();
            cx.notify();
            return;
        }
        let (fence, service, cancel) = {
            let Some(active) = self.diff_controller.active.as_mut() else {
                return;
            };
            if active.view != view
                || active.identity.thread_id != request.thread_id
                || active.identity.project_id != request.project_id
            {
                return;
            }
            let Some(service) = active.service.clone() else {
                return;
            };
            let Some(fence) = active.next_projection_fence(request.generation, request.file_id)
            else {
                return;
            };
            if let Some(cancel) = active.projection_cancel.take() {
                cancel.cancel();
            }
            active.pending_projection = None;
            let cancel = active.cancel.child_token();
            active.projection_cancel = Some(cancel.clone());
            (fence, service, cancel)
        };
        let (sender, receiver) = mpsc::sync_channel(1);
        let file_id = request.file_id;
        let worker = std::thread::Builder::new()
            .name("vega-diff-projection".into())
            .spawn(move || run_diff_projection_worker(service, file_id, cancel, sender));
        if worker.is_err() {
            self.apply_diff_projection_result(fence, Err(GitWorkspaceErrorCode::SpawnFailed), cx);
            return;
        }
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(DIFF_RESULT_POLL).await;
                let result = match receiver.try_recv() {
                    Ok(result) => Some(result),
                    Err(mpsc::TryRecvError::Empty) => None,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        Some(Err(GitWorkspaceErrorCode::SpawnFailed))
                    }
                };
                let Some(result) = result else {
                    continue;
                };
                let _ = this.update(cx, |this, cx| {
                    this.apply_diff_projection_result(fence, result, cx)
                });
                break;
            }
        })
        .detach();
    }

    fn apply_diff_projection_result(
        &mut self,
        fence: DiffProjectionFence,
        result: Result<DiffTextProjection, GitWorkspaceErrorCode>,
        cx: &mut Context<Self>,
    ) {
        if !Self::diff_route_is_current(&fence.route, cx) {
            if self.diff_controller.matches(&fence.route) {
                self.diff_controller.close();
                cx.notify();
            }
            return;
        }
        let view = {
            let Some(active) = self.diff_controller.active.as_mut() else {
                return;
            };
            let disposition = active.projection_disposition(&fence);
            if disposition == DiffProjectionDisposition::Drop {
                return;
            }
            active.projection_cancel = None;
            if disposition == DiffProjectionDisposition::Defer {
                active.pending_projection = Some(PendingDiffProjection { fence, result });
                return;
            }
            active.view.clone()
        };
        match result {
            Ok(projection) => {
                view.update(cx, |view, cx| {
                    let _ = view.apply_projection(projection, cx);
                });
            }
            Err(GitWorkspaceErrorCode::Cancelled | GitWorkspaceErrorCode::StaleGeneration) => {}
            Err(code) => {
                view.update(cx, |view, cx| {
                    view.apply_projection_error(fence.file_id, code, cx)
                });
            }
        }
    }

    fn retry_workspace_diff(
        &mut self,
        view: Entity<DiffView>,
        request: &DiffRetryRequested,
        cx: &mut Context<Self>,
    ) {
        let identity = self
            .diff_controller
            .active
            .as_ref()
            .filter(|active| {
                active.view == view
                    && active.identity.thread_id == request.thread_id
                    && active.identity.project_id == request.project_id
            })
            .map(|active| active.identity.clone());
        if let Some(identity) = identity {
            self.schedule_diff_refresh(&identity, cx);
        }
    }

    fn close_workspace_diff(
        &mut self,
        view: Entity<DiffView>,
        request: &DiffClosed,
        cx: &mut Context<Self>,
    ) {
        let matches = self.diff_controller.active.as_ref().is_some_and(|active| {
            active.view == view
                && active.identity.thread_id == request.thread_id
                && active.identity.project_id == request.project_id
        });
        if matches {
            self.diff_controller.close();
            cx.notify();
        }
    }

    fn workspace_tool_terminal(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &WorkspaceToolTerminal,
        cx: &mut Context<Self>,
    ) {
        if !self.owns_stream_request(&stream, &request.thread_id, cx) {
            return;
        }
        let identity = self
            .diff_controller
            .active
            .as_ref()
            .filter(|active| {
                active.identity.thread_id == request.thread_id
                    && active.identity.project_id == request.project_id
            })
            .map(|active| active.identity.clone());
        if let Some(identity) = identity {
            self.schedule_diff_refresh(&identity, cx);
        }
    }

    fn owns_stream_request(
        &self,
        stream: &Entity<ConversationStream>,
        thread_id: &str,
        cx: &App,
    ) -> bool {
        let current_matches = cx
            .global::<OpenedThread>()
            .0
            .as_ref()
            .is_some_and(|thread| thread.id == thread_id);
        current_matches
            && self
                .stream_view
                .as_ref()
                .is_some_and(|(cached_id, cached)| cached_id == thread_id && cached == stream)
    }

    fn apply_refresh(
        stream: &Entity<ConversationStream>,
        thread: Thread,
        plans: Vec<Plan>,
        cx: &mut Context<Self>,
    ) {
        cx.set_global(OpenedThread(Some(thread.clone())));
        stream.update(cx, |stream, cx| {
            stream.apply_thread(thread, cx);
            for plan in plans {
                stream.apply_plan(plan, cx);
            }
        });
    }

    fn apply_stream_state(
        stream: &Entity<ConversationStream>,
        thread: Thread,
        plans: Vec<Plan>,
        history: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        let thread_id = thread.id.clone();
        stream.update(cx, |stream, cx| {
            stream.apply_thread(thread, cx);
            for plan in plans {
                stream.apply_plan(plan, cx);
            }
            stream.apply_composer_history(&thread_id, history, cx);
        });
    }

    fn current_cached_stream_for_thread(
        &self,
        thread_id: &str,
        cx: &App,
    ) -> Option<Entity<ConversationStream>> {
        let opened_id = cx
            .global::<OpenedThread>()
            .0
            .as_ref()
            .map(|thread| thread.id.as_str());
        let cached_id = self
            .stream_view
            .as_ref()
            .map(|(cached_id, _)| cached_id.as_str());
        if !current_cache_matches(opened_id, cached_id, thread_id) {
            return None;
        }
        self.stream_view.as_ref().map(|(_, stream)| stream.clone())
    }

    fn cancel_active_agent(&mut self, cx: &mut Context<Self>) {
        let pending_review = self.agent_controller.pending_review.take();
        let artifact_run = self
            .agent_controller
            .active
            .as_ref()
            .map(|active| (active.generation, active.stream.clone()));
        if let Some(active) = &self.agent_controller.active {
            active.cancel.cancel();
            active
                .stream
                .update(cx, |stream, cx| stream.timeout_permission(cx));
        }
        if let Some((generation, stream)) = artifact_run {
            self.poison_artifact_agent_generation(generation, &stream);
        }
        if let Some(pending) = pending_review
            && self.owns_stream_request(&pending.stream, &pending.request.thread_id, cx)
        {
            let refresh = match &cx.global::<VegaStore>().0 {
                Ok(store) => reload_thread_and_plans(store, &pending.request.thread_id),
                Err(error) => Err(error.clone()),
            };
            if let Ok((thread, plans)) = refresh {
                Self::apply_refresh(&pending.stream, thread, plans, cx);
            } else {
                pending
                    .stream
                    .update(cx, ConversationStream::apply_controller_error);
            }
        }
    }

    fn start_agent_run(
        &mut self,
        stream: Entity<ConversationStream>,
        thread_id: &str,
        run: PendingAgentRun,
        cx: &mut Context<Self>,
    ) {
        if !self.owns_stream_request(&stream, thread_id, cx) {
            return;
        }
        let pending_user_content = match &run {
            PendingAgentRun::UserMessage(content) => Some(content.clone()),
            PendingAgentRun::ApprovedPlan(_) => None,
        };
        let pending_approved_instruction = match &run {
            PendingAgentRun::UserMessage(_) => None,
            PendingAgentRun::ApprovedPlan(instruction_id) => Some(instruction_id.clone()),
        };
        if self.agent_controller.active.is_some() || self.trusted_actions.is_busy() {
            match run {
                PendingAgentRun::UserMessage(_) => {
                    stream.update(cx, ConversationStream::reject_composer_submission);
                    stream.update(cx, ConversationStream::apply_agent_error);
                }
                PendingAgentRun::ApprovedPlan(_) => {
                    stream.update(cx, ConversationStream::apply_agent_error);
                }
            }
            return;
        }
        let prepared = match &cx.global::<VegaStore>().0 {
            Ok(store) => (|| {
                let database_path = store
                    .database_path()
                    .ok_or_else(|| "agent store is not file-backed".to_string())?
                    .to_path_buf();
                let thread = vega_conversation::threads::open_thread(store, thread_id)
                    .map_err(|error| error.to_string())?;
                let project = vega_store::projects::find(store.conn(), &thread.project_id)
                    .map_err(|error| error.to_string())?
                    .ok_or_else(|| "agent project is unavailable".to_string())?;
                Ok((
                    database_path,
                    std::path::PathBuf::from(project.path),
                    thread,
                ))
            })(),
            Err(error) => Err(error.clone()),
        };
        let Ok((database_path, project_path, thread)) = prepared else {
            if pending_user_content.is_some() {
                stream.update(cx, ConversationStream::reject_composer_submission);
            }
            if pending_approved_instruction.is_some() {
                stream.update(cx, ConversationStream::apply_approved_not_started);
            } else {
                stream.update(cx, ConversationStream::apply_agent_error);
            }
            return;
        };

        // T37 gate: durable Thread.model must resolve against the app-owned
        // Ready authority before begin, channel/worker spawn, config,
        // Keychain, or provider construction. T39 carries the returned
        // immutable capability into the runtime run (exact pricing for every
        // provider call) and into the Composer meter's provisional estimator.
        let pricing_catalog = match self.pricing_controller.select_exact(&thread.model) {
            Ok(selection) => selection.catalog(),
            Err(code) => {
                if let PricingControllerState::Ready {
                    authority,
                    generation,
                    notice,
                    draft,
                    draft_reason,
                    ..
                } = &self.pricing_controller.state
                {
                    self.pricing_controller.state = PricingControllerState::Ready {
                        authority: authority.clone(),
                        generation: *generation,
                        notice: *notice,
                        draft: draft.clone(),
                        draft_reason: *draft_reason,
                        error: Some(code),
                    };
                }
                if pending_user_content.is_some() {
                    stream.update(cx, ConversationStream::reject_composer_submission);
                }
                if pending_approved_instruction.is_some() {
                    stream.update(cx, ConversationStream::apply_approved_not_started);
                } else {
                    stream.update(cx, ConversationStream::apply_agent_error);
                }
                cx.set_global(SettingsOpen(true));
                self.push_pricing_projection(cx);
                return;
            }
        };

        let permission_queue = stream.read(cx).permission_queue();
        let (generation, cancel) = self.agent_controller.begin(
            thread_id.to_string(),
            stream.clone(),
            pending_user_content,
            pending_approved_instruction,
        );
        self.begin_artifact_agent_generation(generation, &stream);
        // S7-T39/C3: the provisional estimator freezes the run-start
        // selection; it never re-reads pricing files or the live authority.
        let meter_estimator = vega_conversation::types::RunUsageEstimator::new(
            &thread.model,
            pricing_catalog.clone(),
        );
        stream.update(cx, |stream, cx| {
            stream.install_meter_estimator(meter_estimator, cx)
        });
        let (sender, receiver) = mpsc::sync_channel(AGENT_EVENT_CAPACITY);
        let worker_sender = sender.clone();
        #[cfg(test)]
        let provider_override = self.agent_provider_override.clone();
        let worker = std::thread::Builder::new()
            .name("vega-agent".into())
            .spawn(move || {
                run_agent_worker(
                    database_path,
                    project_path,
                    thread,
                    run,
                    permission_queue,
                    cancel,
                    worker_sender,
                    Some(pricing_catalog),
                    #[cfg(test)]
                    provider_override,
                );
            });
        if worker.is_err() {
            self.poison_artifact_agent_generation(generation, &stream);
            let failed_run = self.agent_controller.active.take();
            if failed_run
                .as_ref()
                .is_some_and(|active| active.pending_user_content.is_some())
            {
                stream.update(cx, ConversationStream::reject_composer_submission);
            }
            stream.update(cx, |stream, cx| stream.timeout_permission(cx));
            if failed_run.is_some_and(|active| active.pending_approved_instruction.is_some()) {
                stream.update(cx, ConversationStream::apply_approved_not_started);
            } else {
                stream.update(cx, ConversationStream::apply_agent_error);
            }
            return;
        }
        drop(sender);

        let thread_id = thread_id.to_string();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(AGENT_EVENT_POLL).await;
                let batch = drain_agent_updates(&receiver);
                let keep_running = this
                    .update(cx, |this, cx| {
                        let (success, finished_run) = match this
                            .apply_agent_batch_ingress(generation, &thread_id, &stream, batch, cx)
                        {
                            AgentBatchIngress::Stale => return false,
                            AgentBatchIngress::Running => return true,
                            AgentBatchIngress::Finished { success, run } => (success, run),
                        };
                        let ActiveAgentRun {
                            pending_user_content: pending_user,
                            pending_approved_instruction,
                            ..
                        } = finished_run;
                        let approved_not_started = pending_approved_instruction.is_some();
                        if pending_user.is_some() {
                            stream.update(cx, ConversationStream::reject_composer_submission);
                        }
                        let pending_review = this.agent_controller.pending_review.take();
                        let refresh = match &cx.global::<VegaStore>().0 {
                            Ok(store) => reload_thread_state(store, &thread_id),
                            Err(error) => Err(error.clone()),
                        };
                        let mut recovery_projected = approved_not_started;
                        if let Ok(refresh) = refresh {
                            let display_stream = if let Some(current_stream) =
                                this.current_cached_stream_for_thread(&thread_id, cx)
                            {
                                cx.set_global(OpenedThread(Some(refresh.thread.clone())));
                                Self::apply_stream_state(
                                    &current_stream,
                                    refresh.thread,
                                    refresh.plans,
                                    refresh.history,
                                    cx,
                                );
                                current_stream
                            } else {
                                Self::apply_stream_state(
                                    &stream,
                                    refresh.thread,
                                    refresh.plans,
                                    refresh.history,
                                    cx,
                                );
                                stream.clone()
                            };
                            recovery_projected |=
                                refresh.recoverable_approved_instruction.is_some();
                            if recovery_projected {
                                display_stream
                                    .update(cx, ConversationStream::apply_approved_not_started);
                            }
                        } else if approved_not_started {
                            stream.update(cx, ConversationStream::apply_approved_not_started);
                        } else {
                            stream.update(cx, ConversationStream::apply_agent_error);
                        }
                        if !success && !recovery_projected {
                            stream.update(cx, ConversationStream::apply_agent_error);
                        }
                        if let Some(pending) = pending_review {
                            this.review_plan(pending.stream, &pending.request, cx);
                        }
                        false
                    })
                    .unwrap_or(false);
                if !keep_running {
                    break;
                }
            }
        })
        .detach();
    }

    fn submit_composer(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &ComposerSubmitted,
        cx: &mut Context<Self>,
    ) {
        if request.content.is_empty() || !self.owns_stream_request(&stream, &request.thread_id, cx)
        {
            return;
        }
        self.start_agent_run(
            stream,
            &request.thread_id,
            PendingAgentRun::UserMessage(request.content.clone()),
            cx,
        );
    }

    /// Whether the viewport is narrower than the auto-collapse threshold
    /// (ui-spec §1). Reads the live viewport size: every platform resize is
    /// delivered as an event (`Window::bounds_changed` → redraw), so each
    /// render sees the current size and no polling is involved.
    fn auto_collapsed(&self, window: &Window) -> bool {
        window.viewport_size().width < px(AUTO_COLLAPSE_WIDTH)
    }

    /// Cmd+N entry point: creates a thread in the selected project and opens
    /// it (the sidebar [新建任务] button shares this handler).
    fn open_new_thread(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.sidebar.update(cx, Sidebar::create_thread);
    }

    fn persist_thread_settings(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &ThreadSettingsRequested,
        cx: &mut Context<Self>,
    ) {
        if !self.owns_stream_request(&stream, &request.thread_id, cx) {
            return;
        }
        let thread_id = request.thread_id.clone();
        let result = match &cx.global::<VegaStore>().0 {
            Ok(store) => (|| {
                if let Some(mode) = request.mode {
                    vega_conversation::threads::set_thread_mode(store, &thread_id, mode)?;
                }
                if let Some(permission_mode) = request.permission_mode {
                    vega_conversation::threads::set_thread_permission_mode(
                        store,
                        &thread_id,
                        permission_mode,
                    )?;
                }
                vega_conversation::threads::open_thread(store, &thread_id)
            })()
            .map_err(|error| error.to_string()),
            Err(error) => Err(error.clone()),
        };
        match result {
            Ok(thread) => {
                cx.set_global(OpenedThread(Some(thread.clone())));
                stream.update(cx, |stream, cx| stream.apply_thread(thread, cx));
            }
            Err(_) => stream.update(cx, ConversationStream::apply_controller_error),
        }
    }

    fn review_plan(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &PlanReviewRequested,
        cx: &mut Context<Self>,
    ) {
        if !self.owns_stream_request(&stream, &request.thread_id, cx) {
            return;
        }
        if self.trusted_actions.is_busy() {
            stream.update(cx, ConversationStream::apply_controller_error);
            return;
        }
        if self.agent_controller.active.is_some() {
            if self.agent_controller.queue_review(&stream, request) {
                if let Some(active) = self.agent_controller.active.as_ref() {
                    self.poison_artifact_agent_generation(active.generation, &stream);
                }
                stream.update(cx, |stream, cx| stream.timeout_permission(cx));
            } else {
                stream.update(cx, ConversationStream::apply_controller_error);
            }
            return;
        }
        let result = match &cx.global::<VegaStore>().0 {
            Ok(store) => persist_review(store, request),
            Err(error) => Err(error.clone()),
        };
        match result {
            Ok(refresh) => {
                let approved_instruction_id = refresh.approved_instruction_id.clone();
                Self::apply_refresh(&stream, refresh.thread, refresh.plans, cx);
                if let Some(instruction_message_id) = approved_instruction_id {
                    self.start_agent_run(
                        stream,
                        &request.thread_id,
                        PendingAgentRun::ApprovedPlan(instruction_message_id),
                        cx,
                    );
                }
            }
            Err(_) => {
                // A SQLite error may be commit-ambiguous. Reload authoritative
                // state before deciding whether the card may be re-armed.
                let reload = match &cx.global::<VegaStore>().0 {
                    Ok(store) => reload_thread_and_plans(store, &request.thread_id),
                    Err(error) => Err(error.clone()),
                };
                if let Ok((thread, plans)) = reload {
                    Self::apply_refresh(&stream, thread, plans, cx);
                }
                stream.update(cx, ConversationStream::apply_controller_error);
            }
        }
    }
}

impl Drop for VegaWindow {
    fn drop(&mut self) {
        self.window_terminal_cleanup();
    }
}

impl Render for VegaWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Palette comes from the global theme so Cmd+Shift+L repaints instantly.
        let colors = theme(cx).colors;
        // Effective visibility: the user preference (Cmd+B, persisted) AND the
        // viewport auto-collapse rule (ui-spec §1).
        let sidebar_visible = !cx.global::<SidebarCollapsed>().0 && !self.auto_collapsed(window);
        // T13 delete confirmation overlay: rendered above everything (window
        // root, absolute) while a delete is pending (裁决②).
        let pending_delete = cx.global::<PendingDeleteConfirm>().0.clone();

        // Settings opens inside the content area (T09 layout change of the
        // T08 view switching): the sidebar stays visible unless collapsed.
        // 路由收敛（T12 + T17）：内容区 = 设置 or 会话流 or §4.6 空态。
        let settings_open = cx.global::<SettingsOpen>().0;
        if self.diff_controller.active.as_ref().is_some_and(|active| {
            settings_open || !Self::diff_route_is_current(&active.identity, cx)
        }) {
            self.diff_controller.close();
        }
        if self
            .artifact_controller
            .active
            .as_ref()
            .is_some_and(|active| {
                settings_open || !Self::artifact_route_is_current(&active.identity, cx)
            })
        {
            self.close_artifact_route(GitWorkspaceErrorCode::StaleGeneration, cx);
        }
        if self
            .branch_controller
            .active
            .as_ref()
            .is_some_and(|active| {
                settings_open || !Self::branch_route_is_current(&active.identity, cx)
            })
        {
            self.close_branch_route(GitWorkspaceErrorCode::StaleGeneration, cx);
        }
        let content: AnyElement = if settings_open {
            self.cancel_active_agent(cx);
            // 设置视图：缓存 Entity，避免主题刷新等重渲染时重建导致表单输入丢失。
            if self.settings_view.is_none() {
                let settings = cx.new(SettingsView::new);
                cx.subscribe(
                    &settings,
                    |this, view, request: &PricingMutationRequested, cx| {
                        this.request_pricing_mutation(view.clone(), request, cx);
                    },
                )
                .detach();
                cx.subscribe(&settings, |this, view, _: &PricingReloadRequested, cx| {
                    this.request_pricing_reload(view.clone(), cx);
                })
                .detach();
                cx.subscribe(
                    &settings,
                    |this, view, request: &PricingRetryRequested, cx| {
                        this.request_pricing_retry(view.clone(), request, cx);
                    },
                )
                .detach();
                cx.subscribe(
                    &settings,
                    |this, view, request: &PricingDiscardRequested, cx| {
                        this.request_pricing_discard(view.clone(), request, cx);
                    },
                )
                .detach();
                let projection = self.pricing_controller.projection();
                settings.update(cx, |settings, cx| {
                    settings.apply_pricing_projection(projection, cx);
                });
                self.settings_view = Some(settings);
            }
            match &self.settings_view {
                Some(settings) => settings.clone().into_any_element(),
                None => div().size_full().bg(colors.bg_base).into_any_element(),
            }
        } else {
            // 设置已关闭：丢弃缓存，下次打开时重新构造并载入最新配置。
            self.settings_view = None;
            match cx.global::<OpenedThread>().0.clone() {
                Some(thread) => {
                    if let Some(diff_view) = self.diff_controller.visible_view(&thread) {
                        let should_focus = self
                            .diff_controller
                            .active
                            .as_ref()
                            .is_some_and(|active| active.focus_pending);
                        if should_focus {
                            let focus = diff_view.read(cx).focus_handle(cx);
                            window.focus(&focus, cx);
                            if let Some(active) = self.diff_controller.active.as_mut() {
                                active.focus_pending = false;
                            }
                        }
                        return div()
                            .size_full()
                            .flex()
                            .flex_row()
                            .relative()
                            .bg(colors.bg_base)
                            .text_color(colors.text_primary)
                            .when(sidebar_visible, |row| row.child(self.sidebar.clone()))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .h_full()
                                    .overflow_hidden()
                                    .child(diff_view),
                            )
                            .children(pending_delete.map(|thread| {
                                render_delete_confirm_overlay(&thread, self.sidebar.clone(), colors)
                            }));
                    }
                    // S3-T17：会话流视图（每线程一个实体，切换会话时重建；
                    // MarkdownStream 内存态构造，不落库）。
                    let cached = match &self.stream_view {
                        Some((thread_id, view)) if *thread_id == thread.id => Some(view.clone()),
                        _ => None,
                    };
                    let stream = match cached {
                        Some(view) => view,
                        None => {
                            if let Some((_, previous)) = self.stream_view.take() {
                                self.cancel_active_agent(cx);
                                previous.update(cx, |stream, cx| stream.timeout_permission(cx));
                            }
                            let view = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
                            cx.subscribe(&view, |this, stream, request, cx| {
                                this.persist_thread_settings(stream.clone(), request, cx);
                            })
                            .detach();
                            cx.subscribe(&view, |this, stream, request, cx| {
                                this.review_plan(stream.clone(), request, cx);
                            })
                            .detach();
                            cx.subscribe(&view, |this, stream, request, cx| {
                                this.submit_composer(stream.clone(), request, cx);
                            })
                            .detach();
                            cx.subscribe(&view, |this, stream, request, cx| {
                                this.open_workspace_diff(stream.clone(), request, cx);
                            })
                            .detach();
                            cx.subscribe(&view, |this, stream, request, cx| {
                                this.open_commit_panel(stream.clone(), request, cx);
                            })
                            .detach();
                            cx.subscribe(&view, |this, stream, request, cx| {
                                this.workspace_tool_terminal(stream.clone(), request, cx);
                            })
                            .detach();
                            let branch_selector = view.read(cx).branch_selector();
                            cx.subscribe(&branch_selector, |this, selector, request, cx| {
                                this.request_branch_list(selector.clone(), request, cx);
                            })
                            .detach();
                            cx.subscribe(&branch_selector, |this, selector, request, cx| {
                                this.request_branch_switch(selector.clone(), request, cx);
                            })
                            .detach();
                            cx.subscribe(&branch_selector, |this, selector, request, cx| {
                                this.branch_selector_closed(selector.clone(), request, cx);
                            })
                            .detach();
                            let commit_panel = view.read(cx).commit_panel();
                            cx.subscribe(&commit_panel, |this, panel, request, cx| {
                                this.request_commit_prepare(panel.clone(), request, cx);
                            })
                            .detach();
                            cx.subscribe(&commit_panel, |this, panel, request, cx| {
                                this.request_commit_draft(panel.clone(), request, cx);
                            })
                            .detach();
                            cx.subscribe(&commit_panel, |this, panel, request, cx| {
                                this.request_commit_execute(panel.clone(), request, cx);
                            })
                            .detach();
                            cx.subscribe(&commit_panel, |this, panel, request, cx| {
                                this.commit_panel_closed(panel.clone(), request, cx);
                            })
                            .detach();
                            let initial = match &cx.global::<VegaStore>().0 {
                                Ok(store) => (|| {
                                    let plans =
                                        vega_conversation::plans::list_plans(store, &thread.id)?;
                                    let history = vega_conversation::threads::composer_history(
                                        store, &thread.id,
                                    )?;
                                    let recovery =
                                        vega_conversation::plans::recoverable_approved_instruction(
                                            store, &thread.id,
                                        )?;
                                    // S7-T39/C4: the calibrated counter baseline
                                    // comes from the conversation checked aggregate
                                    // query exactly once per route open; the meter
                                    // itself never touches SQLite afterwards.
                                    let usage = vega_conversation::threads::thread_usage_seed(
                                        store, &thread.id,
                                    )?;
                                    // S7-T40 restart recovery: token/cost/cache/
                                    // tool count re-project from the durable
                                    // audits; duration stays `—` (no finished
                                    // timestamp in `messages`, C4).
                                    let summary = vega_conversation::summary::latest_task_summary(
                                        store, &thread.id, None,
                                    )?;
                                    Ok((plans, history, recovery, usage, summary))
                                })(),
                                Err(error) => {
                                    Err(vega_conversation::types::ConversationError::Store(
                                        error.clone(),
                                    ))
                                }
                            };
                            view.update(cx, |stream, cx| match initial {
                                Ok((plans, history, recovery, usage, summary)) => {
                                    for plan in plans {
                                        stream.apply_plan(plan, cx);
                                    }
                                    stream.apply_composer_history(&thread.id, history, cx);
                                    if let Some(summary) = summary {
                                        stream.apply_task_summary(summary, cx);
                                    }
                                    if recovery.is_some() {
                                        stream.apply_approved_not_started(cx);
                                    }
                                    stream.restore_meter(usage, cx);
                                }
                                Err(_) => stream.apply_controller_error(cx),
                            });
                            self.stream_view = Some((thread.id.clone(), view.clone()));
                            view
                        }
                    };
                    self.ensure_artifact_route(&thread, stream.clone(), cx);
                    self.ensure_branch_route(&thread, stream.clone(), cx);
                    let commit_focus = self
                        .commit_controller
                        .active
                        .as_ref()
                        .filter(|active| active.focus_pending && active.identity.stream == stream)
                        .map(|active| active.identity.panel.read(cx).focus_handle(cx));
                    if let Some(focus) = commit_focus {
                        window.focus(&focus, cx);
                        if let Some(active) = self.commit_controller.active.as_mut() {
                            active.focus_pending = false;
                        }
                    }
                    stream.into_any_element()
                }
                None => {
                    if let Some((_, previous)) = self.stream_view.take() {
                        self.cancel_active_agent(cx);
                        previous.update(cx, |stream, cx| stream.timeout_permission(cx));
                    }
                    render_empty_state(colors)
                }
            }
        };

        div()
            .size_full()
            .flex()
            .flex_row()
            .relative()
            .bg(colors.bg_base)
            .text_color(colors.text_primary)
            .when(sidebar_visible, |row| row.child(self.sidebar.clone()))
            .child(
                // Content column host: settings brings its own 820px column,
                // the empty state is centered by its own layout.
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_hidden()
                    .child(content),
            )
            // T13 删除确认弹层：最后绘制以覆盖全窗口；遮罩点击 / Esc 取消。
            .children(
                pending_delete.map(|thread| {
                    render_delete_confirm_overlay(&thread, self.sidebar.clone(), colors)
                }),
            )
    }
}

/// The content-area empty state (ui-spec §4.6): centered guidance with inert
/// quick-template placeholder buttons, inside the 820px content column —
/// no large logo illustration. The temporary T10/T11 entry buttons were
/// retired in T12 (projects/sessions now live in the sidebar).
fn render_empty_state(colors: ThemeColors) -> AnyElement {
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .child(
            div()
                .w_full()
                .max_w(px(CONTENT_MAX_WIDTH))
                .px(px(CONTENT_MIN_PADDING))
                .flex()
                .flex_col()
                .items_center()
                .gap_3()
                .child(
                    div()
                        .text_size(px(Typography::HEADING_PAGE))
                        .font_weight(Typography::HEADING_PAGE_WEIGHT)
                        .child("✦ Vega"),
                )
                .child(
                    div()
                        .text_size(px(Typography::BODY))
                        .text_color(colors.text_secondary)
                        .child("开始一个新会话"),
                )
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .children(EMPTY_STATE_TEMPLATES.map(|label| {
                            div()
                                .px_3()
                                .py_1()
                                .rounded_md()
                                .border_1()
                                .border_color(colors.border_subtle)
                                .bg(colors.bg_elevated)
                                .text_size(px(Typography::SIDEBAR))
                                .text_color(colors.text_secondary)
                                .child(label)
                        })),
                ),
        )
        .into_any_element()
}

fn main() {
    // S3-T17 隐藏自测量模式：`vega --vega-bench-render <out.json>` 跑完写
    // JSON 后退出（xtask bench render_frame 的数据来源），不进入正常应用。
    if let Some(output) = render_frame_bench::output_path_from_args() {
        application().run(|cx: &mut App| render_frame_bench::start(output, cx));
        return;
    }

    application().run(|cx: &mut App| {
        // Seed the global theme from the macOS appearance; components read it
        // via `vega_theme::theme(cx)`.
        let theme = Theme::system(cx);
        cx.set_global(theme);

        // Sidebar collapse preference, restored from config.toml before the
        // window opens so the first frame already matches the stored state.
        cx.set_global(SidebarCollapsed(load_collapsed()));

        // Settings view starts closed; the window render reads this global.
        cx.set_global(SettingsOpen(false));

        // Key bindings for the vega_ui text input components.
        vega_ui::init(cx);

        // T12: open + migrate the store at the platform data root (tech-spec
        // §6) and seed the sidebar globals (selected project, block collapse
        // states, opened thread). On failure the app still boots and the
        // sidebar blocks degrade to inline error bars (ui-spec §4.6).
        vega_ui::sidebar::init(cx);

        let bounds = Bounds::centered(None, size(px(WINDOW_MIN_WIDTH), px(WINDOW_MIN_HEIGHT)), cx);
        let min_size = size(px(WINDOW_MIN_WIDTH), px(WINDOW_MIN_HEIGHT));

        let window = cx.open_window(
            WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some("Vega".into()),
                    ..Default::default()
                }),
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(min_size),
                ..Default::default()
            },
            |_, cx| cx.new(VegaWindow::new),
        );

        let window = match window {
            Ok(window) => window,
            Err(error) => {
                // Degrade path: without the main window there is nothing to run.
                tracing::error!(%error, "failed to open the main window");
                cx.quit();
                return;
            }
        };

        cx.activate(true);
        cx.bind_keys([
            KeyBinding::new("cmd-q", Quit, None),
            // Temporary verification binding for the theme token mechanism.
            KeyBinding::new("cmd-shift-l", ToggleTheme, None),
            // Settings view switching (T08).
            KeyBinding::new("cmd-,", OpenSettings, None),
            KeyBinding::new("escape", CloseSettings, None),
            // Sidebar collapse toggle (T09).
            KeyBinding::new("cmd-b", ToggleSidebar, None),
            // Thread creation (T11→T12): button and Cmd+N share one handler.
            KeyBinding::new("cmd-n", NewThread, None),
        ]);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.on_action(|_: &ToggleTheme, cx| {
            cx.global_mut::<Theme>().toggle();
            // Redraw all windows so the new palette is visible immediately.
            cx.refresh_windows();
        });
        cx.on_action(|_: &OpenSettings, cx| {
            cx.set_global(SettingsOpen(true));
            cx.refresh_windows();
        });
        cx.on_action(|_: &CloseSettings, cx| {
            // T13 裁决②：删除确认弹层存在时优先消费 Esc（弹层关闭后设置
            // 视图保持不变），行内重命名的 Esc 由其编辑器在更内层拦截。
            let overlay_open = cx
                .try_global::<PendingDeleteConfirm>()
                .is_some_and(|pending| pending.0.is_some());
            if overlay_open {
                cx.set_global(PendingDeleteConfirm(None));
            } else {
                cx.set_global(SettingsOpen(false));
            }
            cx.refresh_windows();
        });
        cx.on_action(move |_: &NewThread, cx| {
            if let Err(error) = window.update(cx, VegaWindow::open_new_thread) {
                tracing::error!(%error, "failed to handle Cmd+N in the main window");
            }
        });
        cx.on_action(|_: &ToggleSidebar, cx| toggle_persisted(cx));
        // Quit once the last window is closed so the process does not linger.
        cx.on_window_closed(|cx, _window_id| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::fs;
    use std::os::unix::ffi::OsStrExt;
    use std::process::Command;
    use tempfile::TempDir;
    use vega_conversation::types::{
        PermissionMode, PlanReviewAction, PlanStatus, ThreadMode, ThreadStatus, ToolResult,
    };
    use vega_store::messages::{MessageRow, complete_plan, insert};

    #[test]
    fn commit_provider_policy_disables_retries() {
        assert_eq!(commit_retry_policy().max_retries, 0);
    }

    #[test]
    fn pricing_precommit_failure_keeps_persistent_notice_and_exact_draft() {
        let data = tempfile::tempdir().expect("pricing state root");
        let service = PricingSettingsService::new(data.path().join("pricing.json"));
        let authority = service.load_or_seed().expect("pricing authority").authority;
        let plan = service
            .prepare_save(
                &authority,
                vega_conversation::types::PricingMutation::AddCustom {
                    model: "custom/retry".into(),
                    rates: vega_conversation::types::PricingRateInputs {
                        input_usd_per_million: "1".into(),
                        output_usd_per_million: "1".into(),
                        cache_read_usd_per_million: "1".into(),
                        cache_write_usd_per_million: "1".into(),
                    },
                },
            )
            .expect("pricing plan");
        let mut state = pricing_retry_ready(
            authority,
            9,
            Some(PricingNotice::DurabilityUnknownReconciled),
            plan,
            PricingSettingsErrorCode::Io,
        );
        assert!(matches!(
            &state,
            PricingControllerState::Ready {
                generation: 9,
                notice: Some(PricingNotice::DurabilityUnknownReconciled),
                draft: Some(_),
                draft_reason: Some(PricingDraftReason::RetryPending),
                error: Some(PricingSettingsErrorCode::Io),
                ..
            }
        ));
        assert!(discard_pricing_draft(&mut state, 9));
        assert!(matches!(
            state,
            PricingControllerState::Ready {
                draft: None,
                draft_reason: None,
                error: None,
                ..
            }
        ));
    }

    #[test]
    fn pricing_controller_operation_claim_is_single_flight_and_stale_safe() {
        let mut controller = PricingController::new(None);
        let first = controller
            .begin_operation()
            .expect("first pricing operation");
        assert!(controller.begin_operation().is_none());
        assert!(!controller.claim_completion(first + 1));
        assert_eq!(controller.active_operation, Some(first));
        assert!(controller.claim_completion(first));
        assert!(controller.active_operation.is_none());
    }

    struct CommitPanelHarness {
        panel: Entity<CommitPanel>,
    }

    impl Render for CommitPanelHarness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(self.panel.clone())
        }
    }

    struct PricingWindowHarness {
        root: Entity<VegaWindow>,
    }

    impl Render for PricingWindowHarness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.root.clone()
        }
    }

    #[gpui::test]
    async fn pricing_settings_and_agent_preflight_production_e2e(cx: &mut gpui::TestAppContext) {
        let repo = diff_controller_repo();
        let data = tempfile::tempdir().expect("pricing data root");
        let store = Store::open(data.path().join("vega.db")).expect("pricing file store");
        store.migrate().expect("pricing migrations");
        let project = vega_store::projects::create(
            store.conn(),
            repo.path().to_str().expect("UTF-8 pricing repo"),
            "pricing-e2e",
            None,
        )
        .expect("pricing project");
        let thread = vega_conversation::threads::create_thread(
            &store,
            &project.id,
            "custom/gated",
            PermissionMode::Confirm.as_str(),
        )
        .expect("pricing thread");
        cx.update(|cx| install_diff_window_globals(store, thread.clone(), cx));
        let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
        let provider = Arc::new(vega_runtime::MockProvider::new(vec![
            vega_runtime::ScriptStep::events(vec![
                vega_runtime::ProviderEvent::TextDelta("ok".into()),
                vega_runtime::ProviderEvent::Done {
                    stop_reason: vega_runtime::StopReason::End,
                },
            ]),
        ]));
        let root = cx.new(VegaWindow::new);
        root.update(cx, |root, _| {
            root.stream_view = Some((thread.id.clone(), stream.clone()));
            root.agent_provider_override = Some(provider.clone());
        });
        let window_root = root.clone();
        let _window: gpui::WindowHandle<PricingWindowHarness> = cx
            .update(|cx| {
                cx.open_window(Default::default(), move |_, cx| {
                    cx.new(|_| PricingWindowHarness { root: window_root })
                })
            })
            .expect("pricing window");
        pump_test_app(cx, |cx| {
            root.read_with(cx, |root, _| {
                matches!(
                    root.pricing_controller.state,
                    PricingControllerState::Ready { .. }
                )
            })
        });

        root.update(cx, |root, _| {
            let _ = root.artifact_controller.close();
        });

        let starts = AGENT_WORKER_STARTS.load(Ordering::SeqCst);
        let (agent_generation, artifact_epoch, artifact_active) = root.read_with(cx, |root, _| {
            (
                root.agent_controller.next_generation,
                root.artifact_controller.next_route_epoch,
                root.artifact_controller.active.is_some(),
            )
        });
        root.update(cx, |root, cx| {
            root.start_agent_run(
                stream.clone(),
                &thread.id,
                PendingAgentRun::UserMessage("blocked before pricing".into()),
                cx,
            );
        });
        assert_eq!(AGENT_WORKER_STARTS.load(Ordering::SeqCst), starts);
        assert!(provider.requests().is_empty());
        root.read_with(cx, |root, _| {
            assert!(root.agent_controller.active.is_none());
            assert_eq!(root.agent_controller.next_generation, agent_generation);
            assert_eq!(root.artifact_controller.next_route_epoch, artifact_epoch);
            assert_eq!(root.artifact_controller.active.is_some(), artifact_active);
        });
        assert!(cx.update(|cx| cx.global::<SettingsOpen>().0));
        root.update(cx, |root, cx| {
            root.start_agent_run(
                stream.clone(),
                &thread.id,
                PendingAgentRun::ApprovedPlan("not-started-without-pricing".into()),
                cx,
            );
        });
        assert_eq!(AGENT_WORKER_STARTS.load(Ordering::SeqCst), starts);
        assert!(provider.requests().is_empty());
        root.read_with(cx, |root, _| {
            assert!(root.agent_controller.active.is_none());
            assert_eq!(root.agent_controller.next_generation, agent_generation);
        });
        pump_test_app(cx, |cx| {
            root.read_with(cx, |root, _| root.settings_view.is_some())
        });
        let settings = root
            .read_with(cx, |root, _| root.settings_view.clone())
            .expect("production settings entity");
        let generation = root.read_with(cx, |root, _| match &root.pricing_controller.state {
            PricingControllerState::Ready { generation, .. } => *generation,
            _ => 0,
        });
        let gate = Arc::new(std::sync::Barrier::new(2));
        root.update(cx, |root, cx| {
            root.pricing_drop_next_worker_result = true;
            root.pricing_next_worker_gate = Some(gate.clone());
            root.request_pricing_mutation(
                settings.clone(),
                &PricingMutationRequested {
                    generation,
                    mutation: Ok(vega_conversation::types::PricingMutation::AddCustom {
                        model: "custom/gated".into(),
                        rates: vega_conversation::types::PricingRateInputs {
                            input_usd_per_million: "1".into(),
                            output_usd_per_million: "1".into(),
                            cache_read_usd_per_million: "1".into(),
                            cache_write_usd_per_million: "1".into(),
                        },
                    }),
                },
                cx,
            );
            assert!(matches!(
                root.pricing_controller.state,
                PricingControllerState::Saving { .. }
            ));
            cx.set_global(SettingsOpen(false));
            cx.refresh_windows();
        });
        cx.run_until_parked();
        root.read_with(cx, |root, _| {
            assert!(root.settings_view.is_none());
            assert!(matches!(
                root.pricing_controller.state,
                PricingControllerState::Saving { .. }
            ));
        });
        gate.wait();
        pump_test_app(cx, |cx| {
            root.read_with(cx, |root, _| {
                matches!(
                    &root.pricing_controller.state,
                    PricingControllerState::Ready {
                        authority,
                        draft: None,
                        notice: Some(PricingNotice::DurabilityUnknownReconciled),
                        ..
                    } if authority.contains_exact_model("custom/gated")
                )
            })
        });
        assert!(data.path().join("pricing.json").is_file());

        cx.update(|cx| {
            cx.set_global(SettingsOpen(true));
            cx.refresh_windows();
        });
        pump_test_app(cx, |cx| {
            root.read_with(cx, |root, _| {
                root.settings_view
                    .as_ref()
                    .is_some_and(|view| view != &settings)
            })
        });
        root.read_with(cx, |root, _| {
            assert!(matches!(
                &root.pricing_controller.state,
                PricingControllerState::Ready {
                    authority,
                    notice: Some(PricingNotice::DurabilityUnknownReconciled),
                    ..
                } if authority.contains_exact_model("custom/gated")
            ));
        });
        cx.update(|cx| {
            cx.set_global(SettingsOpen(false));
            cx.refresh_windows();
        });
        pump_test_app(cx, |cx| {
            root.read_with(cx, |root, _| root.settings_view.is_none())
        });

        root.update(cx, |root, cx| {
            root.start_agent_run(
                stream.clone(),
                &thread.id,
                PendingAgentRun::UserMessage("priced run".into()),
                cx,
            );
        });
        pump_test_app(cx, |cx| {
            root.read_with(cx, |root, _| root.agent_controller.active.is_none())
                && provider.requests().len() == 1
        });
        assert_eq!(AGENT_WORKER_STARTS.load(Ordering::SeqCst), starts + 1);
        assert_eq!(provider.requests().len(), 1);
    }

    #[derive(Clone)]
    enum CapturedCommitEvent {
        Prepare(CommitPrepareRequested),
        Draft(CommitDraftRequested),
        Commit(CommitRequested),
        Close,
    }

    #[track_caller]
    fn pump_test_app(
        cx: &mut gpui::TestAppContext,
        mut ready: impl FnMut(&mut gpui::TestAppContext) -> bool,
    ) {
        for _ in 0..400 {
            cx.executor().advance_clock(DIFF_RESULT_POLL);
            cx.run_until_parked();
            if ready(cx) {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("test app did not reach the expected terminal state");
    }

    #[test]
    fn branch_controller_shared_lease_is_first_wins_and_aba_safe() {
        let actions = TrustedActionCoordinator::default();
        let first = actions
            .acquire(TrustedActionKind::BranchSwitch, 7, 1)
            .expect("first owner");
        assert!(
            actions
                .acquire(TrustedActionKind::ArtifactOpen, 8, 1)
                .is_none(),
            "a second trusted action cannot overlap"
        );
        let mut forged = first;
        forged.generation += 1;
        assert!(!actions.release(forged));
        assert!(actions.is_busy());
        assert!(actions.release(first));
        let second = actions
            .acquire(TrustedActionKind::Commit, 7, 2)
            .expect("new generation");
        assert_ne!(first, second);
        assert!(!actions.release(first), "stale A cannot release B");
        assert!(actions.is_busy());
        assert!(actions.release(second));
    }

    #[test]
    fn commit_worker_terminal_releases_exact_owner_after_window_drop() {
        let actions = TrustedActionCoordinator::default();
        let lease = actions
            .acquire(TrustedActionKind::Commit, 7, 11)
            .expect("commit lease");
        let alive = Arc::new(AtomicBool::new(false));
        let done = Arc::new(AtomicBool::new(false));
        mark_commit_worker_terminal(done.clone(), alive, actions.clone(), lease);
        assert!(done.load(Ordering::Acquire));
        assert!(!actions.is_busy());

        let lease = actions
            .acquire(TrustedActionKind::Commit, 8, 12)
            .expect("fresh commit lease");
        let alive = Arc::new(AtomicBool::new(true));
        let done = Arc::new(AtomicBool::new(false));
        mark_commit_worker_terminal(done.clone(), alive.clone(), actions.clone(), lease);
        assert!(actions.is_busy(), "live window owns UI reconciliation");
        alive.store(false, Ordering::Release);
        assert!(done.load(Ordering::Acquire));
        assert!(actions.release(lease), "Drop exact terminal cleanup");
    }

    #[test]
    fn commit_terminal_and_window_cleanup_are_seqcst_race_safe() {
        for generation in 1..=1_000 {
            let actions = TrustedActionCoordinator::default();
            let lease = actions
                .acquire(TrustedActionKind::Commit, generation, 1)
                .expect("race lease");
            let alive = Arc::new(AtomicBool::new(true));
            let done = Arc::new(AtomicBool::new(false));
            let barrier = Arc::new(std::sync::Barrier::new(3));
            std::thread::scope(|scope| {
                scope.spawn({
                    let actions = actions.clone();
                    let alive = alive.clone();
                    let done = done.clone();
                    let barrier = barrier.clone();
                    move || {
                        barrier.wait();
                        mark_commit_worker_terminal(done, alive, actions, lease);
                    }
                });
                scope.spawn({
                    let actions = actions.clone();
                    let alive = alive.clone();
                    let done = done.clone();
                    let barrier = barrier.clone();
                    move || {
                        barrier.wait();
                        alive.store(false, Ordering::SeqCst);
                        if done.load(Ordering::SeqCst) {
                            let _ = actions.release(lease);
                        }
                    }
                });
                barrier.wait();
            });
            assert!(!actions.is_busy(), "iteration {generation} leaked lease");
            let fresh = actions
                .acquire(TrustedActionKind::Commit, generation, 2)
                .expect("fresh race lease");
            assert!(
                !actions.release(lease),
                "stale terminal released fresh lease"
            );
            assert!(actions.release(fresh));
        }
    }

    #[test]
    fn commit_runtime_failure_is_typed_and_recovery_backoff_is_bounded() {
        let failed = build_commit_runtime_with(|| Err(std::io::Error::other("fixture")));
        assert!(matches!(failed, Err(CommitErrorCode::SpawnFailed)));
        let terminal = CommitWorkerResult::RuntimeUnavailable(CommitErrorCode::SpawnFailed);
        assert!(commit_result_has_authoritative_workspace(
            CommitPhase::Preparing,
            &terminal
        ));
        assert!(commit_result_has_authoritative_workspace(
            CommitPhase::Committing,
            &terminal
        ));
        assert!(commit_result_reconciliation(&terminal).is_none());

        let mut delay = Duration::from_millis(25);
        for expected in [50, 100, 200, 400, 800, 1000, 1000] {
            delay = next_commit_recovery_backoff(delay);
            assert_eq!(delay, Duration::from_millis(expected));
        }

        let attempts = std::cell::Cell::new(0_u8);
        let mut waits = Vec::new();
        let recovered = build_commit_recovery_runtime_with(
            || {
                let attempt = attempts.get() + 1;
                attempts.set(attempt);
                if attempt < 3 {
                    Err(CommitErrorCode::SpawnFailed)
                } else {
                    build_commit_runtime()
                }
            },
            |delay| waits.push(delay),
        );
        assert!(recovered.is_ok());
        assert_eq!(attempts.get(), 3);
        assert_eq!(
            waits,
            [Duration::from_millis(25), Duration::from_millis(50)]
        );

        let attempts = std::cell::Cell::new(0_u8);
        let mut waits = Vec::new();
        assert!(matches!(
            build_commit_recovery_runtime_with(
                || {
                    attempts.set(attempts.get() + 1);
                    Err(CommitErrorCode::SpawnFailed)
                },
                |delay| waits.push(delay),
            ),
            Err(CommitErrorCode::SpawnFailed)
        ));
        assert_eq!(attempts.get(), 6);
        assert_eq!(waits.len(), 5, "the terminal failure is bounded");
    }

    #[gpui::test]
    async fn commit_controller_retiring_fence_is_first_wins_and_holds_owner(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(|cx| {
            cx.set_global(Theme::light());
            cx.set_global(SettingsOpen(false));
            vega_ui::init(cx);
        });
        let repo = diff_controller_repo();
        let thread = Thread {
            id: "commit-thread".into(),
            project_id: "commit-project".into(),
            title: String::new(),
            mode: ThreadMode::Execute,
            permission_mode: PermissionMode::Confirm,
            model: String::new(),
            status: ThreadStatus::Active,
            pinned: false,
            unread: false,
            created_at: 0,
            updated_at: 0,
        };
        let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
        let panel = stream.read_with(cx, |stream, _| stream.commit_panel());
        let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
        let service = Arc::new(
            TrustedGitService::new(repo.path(), workspace).expect("trusted commit service"),
        );
        let lease = TrustedActionToken {
            generation: 1,
            kind: TrustedActionKind::Commit,
            owner_epoch: 1,
            request_sequence: 1,
        };
        let identity = CommitRouteIdentity {
            epoch: 1,
            thread_id: thread.id,
            project_id: thread.project_id,
            stream: stream.clone(),
            panel,
        };
        let mut active = ActiveCommitRoute {
            identity,
            service,
            lease,
            next_sequence: 0,
            phase: CommitPhase::Checklist,
            snapshot: None,
            prepared: None,
            focus_pending: false,
            pending: None,
            cancel: None,
            terminal_done: None,
        };
        let (fence, cancel, _) = CommitController::begin_fence(
            &mut active,
            CommitPhase::Checklist,
            None,
            CommitFenceAuthority::None,
        )
        .expect("checklist owner fence");
        let mut controller = CommitController {
            next_epoch: 1,
            active: Some(active),
            retiring: None,
        };
        assert_eq!(controller.retire_or_close(), None);
        assert!(cancel.is_cancelled());
        assert!(controller.active.is_none());
        assert!(controller.retiring.is_some());
        assert!(matches!(
            controller.claim(&fence),
            CommitClaim::Retiring(active)
                if active.lease == lease && active.identity.stream == stream
        ));
        assert!(matches!(controller.claim(&fence), CommitClaim::Stale));
    }

    #[gpui::test]
    async fn commit_controller_binds_exact_snapshot_and_overflow_is_zero_work(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(|cx| {
            cx.set_global(Theme::light());
            cx.set_global(SettingsOpen(false));
            vega_ui::init(cx);
        });
        let repo = diff_controller_repo();
        let thread = Thread {
            id: "commit-capability-thread".into(),
            project_id: "commit-capability-project".into(),
            title: String::new(),
            mode: ThreadMode::Execute,
            permission_mode: PermissionMode::Confirm,
            model: String::new(),
            status: ThreadStatus::Active,
            pinned: false,
            unread: false,
            created_at: 0,
            updated_at: 0,
        };
        let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
        let panel = stream.read_with(cx, |stream, _| stream.commit_panel());
        let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
        let service = Arc::new(
            TrustedGitService::new(repo.path(), workspace.clone()).expect("trusted service"),
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let first = runtime.block_on(async {
            workspace
                .refresh(tokio_util::sync::CancellationToken::new())
                .await
                .expect("refresh");
            service
                .open_checklist(tokio_util::sync::CancellationToken::new())
                .await
                .expect("first checklist")
        });
        let second = runtime
            .block_on(service.open_checklist(tokio_util::sync::CancellationToken::new()))
            .expect("second checklist");
        assert_ne!(first.id, second.id);
        let identity = CommitRouteIdentity {
            epoch: 1,
            thread_id: thread.id,
            project_id: thread.project_id,
            stream,
            panel,
        };
        let lease = TrustedActionToken {
            generation: 1,
            kind: TrustedActionKind::Commit,
            owner_epoch: 1,
            request_sequence: 1,
        };
        let mut active = ActiveCommitRoute {
            identity,
            service,
            lease,
            next_sequence: 0,
            phase: CommitPhase::Checklist,
            snapshot: Some(first.id),
            prepared: None,
            focus_pending: false,
            pending: None,
            cancel: None,
            terminal_done: None,
        };
        let (wrong, _, _) = CommitController::begin_fence(
            &mut active,
            CommitPhase::Preparing,
            None,
            CommitFenceAuthority::Snapshot(second.id),
        )
        .expect("wrong capability fixture");
        let mut controller = CommitController {
            next_epoch: 1,
            active: Some(active),
            retiring: None,
        };
        assert!(matches!(controller.claim(&wrong), CommitClaim::Stale));
        let active = controller.active.as_mut().expect("active retained");
        active.pending = None;
        active.cancel = None;
        active.next_sequence = u64::MAX;
        let phase = active.phase;
        assert!(
            CommitController::begin_fence(
                active,
                CommitPhase::Preparing,
                None,
                CommitFenceAuthority::Snapshot(first.id),
            )
            .is_none()
        );
        assert!(active.phase == phase);
        assert!(active.pending.is_none());
        assert!(active.cancel.is_none());
    }

    #[gpui::test]
    async fn commit_controller_same_id_entity_aba_is_stale_and_worker_recovers_authority(
        cx: &mut gpui::TestAppContext,
    ) {
        let repo = artifact_controller_repo();
        let store = Store::open(":memory:").expect("commit window memory store");
        store.migrate().expect("commit window migrations");
        let project = vega_store::projects::create(
            store.conn(),
            repo.path().to_str().expect("UTF-8 commit root"),
            "commit",
            None,
        )
        .expect("commit project");
        let thread = vega_conversation::threads::create_thread(
            &store,
            &project.id,
            "mock",
            PermissionMode::Confirm.as_str(),
        )
        .expect("commit thread");
        cx.update(|cx| install_diff_window_globals(store, thread.clone(), cx));
        let old_stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
        let fresh_stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
        let old_panel = old_stream.read_with(cx, |stream, _| stream.commit_panel());
        let old_identity = CommitRouteIdentity {
            epoch: 1,
            thread_id: thread.id.clone(),
            project_id: thread.project_id.clone(),
            stream: old_stream,
            panel: old_panel,
        };
        let root = cx.new(VegaWindow::new);
        root.update(cx, |root, cx| {
            root.stream_view = Some((thread.id.clone(), fresh_stream));
            assert!(!root.commit_route_is_current(&old_identity, cx));
        });
        let fresh_diff =
            cx.new(|cx| DiffView::new(thread.id.clone(), thread.project_id.clone(), cx));
        root.update(cx, |root, _| {
            assert!(
                root.diff_controller
                    .begin(
                        thread.id.clone(),
                        thread.project_id.clone(),
                        fresh_diff.clone(),
                    )
                    .is_some()
            );
        });

        let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
        let service = Arc::new(
            TrustedGitService::new(repo.path(), workspace.clone()).expect("trusted service"),
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let stale = runtime.block_on(async {
            workspace
                .refresh(tokio_util::sync::CancellationToken::new())
                .await
                .expect("refresh");
            let stale = service
                .open_checklist(tokio_util::sync::CancellationToken::new())
                .await
                .expect("stale checklist");
            service
                .open_checklist(tokio_util::sync::CancellationToken::new())
                .await
                .expect("replacement checklist");
            stale
        });
        let result = run_commit_prepare_worker(
            service,
            stale.id,
            Vec::new(),
            tokio_util::sync::CancellationToken::new(),
            None,
            None,
            None,
        );
        let reconciliation = match result {
            CommitWorkerResult::Prepare(
                CommitPrepareCompletion {
                    prepared: None,
                    workspace: Some(_),
                    error: Some(CommitErrorCode::StaleAuthority),
                },
                reconciliation,
            ) => reconciliation,
            _ => panic!("stale capability must return typed prepare completion"),
        };
        root.update(cx, |root, cx| {
            root.apply_commit_workspace_reconciliation(&old_identity, &reconciliation, cx);
        });
        assert_eq!(
            fresh_diff.read_with(cx, |view, _| view.generation()),
            None,
            "old same-id stream completion cannot overwrite fresh Diff route"
        );
    }

    #[gpui::test]
    async fn commit_panel_accepts_canonical_mixed_staged_and_unstaged_identity(
        cx: &mut gpui::TestAppContext,
    ) {
        let repo = diff_controller_repo();
        run_fixture_git(repo.path(), &["add", "--", "tracked.rs"]);
        fs::write(
            repo.path().join("tracked.rs"),
            "fn base() {}\nfn changed() {}\nfn later() {}\n",
        )
        .expect("mixed worktree update");
        let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
        let service = TrustedGitService::new(repo.path(), workspace.clone()).expect("service");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let checklist = runtime.block_on(async {
            workspace
                .refresh(tokio_util::sync::CancellationToken::new())
                .await
                .expect("refresh");
            service
                .open_checklist(tokio_util::sync::CancellationToken::new())
                .await
                .expect("mixed checklist")
        });
        assert_eq!(checklist.staged.len(), 1);
        assert_eq!(checklist.optional.len(), 1);
        assert_eq!(checklist.staged[0].file_id, checklist.optional[0].file_id);
        let panel = cx.new(|cx| CommitPanel::new("thread".into(), "project".into(), cx));
        panel.update(cx, |panel, cx| {
            assert!(panel.request_open(cx));
            assert!(panel.apply_checklist(checklist, cx));
        });
    }

    #[gpui::test]
    async fn commit_panel_real_key_handlers_are_scoped_and_first_wins(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(|cx| {
            cx.set_global(Theme::light());
            vega_ui::init(cx);
        });
        let repo = diff_controller_repo();
        run_fixture_git(repo.path(), &["add", "--", "tracked.rs"]);
        fs::write(repo.path().join("optional.rs"), "fn optional() {}\n").expect("optional fixture");
        let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
        let service = Arc::new(
            TrustedGitService::new(repo.path(), workspace.clone()).expect("trusted service"),
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let checklist = runtime.block_on(async {
            workspace
                .refresh(tokio_util::sync::CancellationToken::new())
                .await
                .expect("refresh");
            service
                .open_checklist(tokio_util::sync::CancellationToken::new())
                .await
                .expect("checklist")
        });
        assert!(!checklist.staged.is_empty());
        assert_eq!(checklist.optional.len(), 1);

        let panel = cx.new(|cx| CommitPanel::new("thread".into(), "project".into(), cx));
        let events = Arc::new(Mutex::new(Vec::<CapturedCommitEvent>::new()));
        let window_events = events.clone();
        let root = panel.clone();
        let window = cx
            .update(|cx| {
                cx.open_window(Default::default(), move |_, cx| {
                    let events_prepare = window_events.clone();
                    let events_draft = window_events.clone();
                    let events_commit = window_events.clone();
                    let events_close = window_events.clone();
                    cx.new(|cx| {
                        cx.subscribe(&root, move |_, _, event: &CommitPrepareRequested, _| {
                            events_prepare
                                .lock()
                                .expect("events")
                                .push(CapturedCommitEvent::Prepare(event.clone()));
                        })
                        .detach();
                        cx.subscribe(&root, move |_, _, event: &CommitDraftRequested, _| {
                            events_draft
                                .lock()
                                .expect("events")
                                .push(CapturedCommitEvent::Draft(event.clone()));
                        })
                        .detach();
                        cx.subscribe(&root, move |_, _, event: &CommitRequested, _| {
                            events_commit
                                .lock()
                                .expect("events")
                                .push(CapturedCommitEvent::Commit(event.clone()));
                        })
                        .detach();
                        cx.subscribe(&root, move |_, _, _event: &CommitPanelClosed, _| {
                            events_close
                                .lock()
                                .expect("events")
                                .push(CapturedCommitEvent::Close);
                        })
                        .detach();
                        CommitPanelHarness { panel: root }
                    })
                })
            })
            .expect("commit key window");
        window
            .update(cx, |_, window, cx| {
                assert!(panel.update(cx, |panel, cx| panel.request_open(cx)));
                assert!(panel.update(cx, |panel, cx| {
                    panel.apply_checklist(checklist.clone(), cx)
                }));
                let focus = panel.read(cx).focus_handle(cx);
                focus.focus(window, cx);
            })
            .expect("open checklist");

        // Space at Cancel is inert; Tab skips the forced staged row and lands
        // on the sole optional worktree row.
        cx.simulate_keystrokes(window.into(), "space tab space cmd-enter cmd-enter");
        let prepare = events
            .lock()
            .expect("events")
            .iter()
            .filter_map(|event| match event {
                CapturedCommitEvent::Prepare(request) => Some(request.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(prepare.len(), 1, "prepare is exact first-wins");
        assert_eq!(prepare[0].selected.len(), 1, "optional Space toggles once");
        let completion = runtime.block_on(service.prepare(
            prepare[0].snapshot_id,
            prepare[0].selected.clone(),
            tokio_util::sync::CancellationToken::new(),
        ));
        let prepared = completion.prepared.expect("prepared authority");
        assert!(panel.update(cx, |panel, cx| {
            panel.finish_prepare(prepare[0].operation_id, Ok(prepared.clone()), cx)
        }));
        cx.run_until_parked();
        assert_eq!(
            panel.read_with(cx, |panel, _| panel.focused_control()),
            vega_ui::commit_panel::CommitPanelFocus::Cancel
        );

        // Editor Enter remains newline and emits no draft. Generate Enter and
        // Space each emit exactly once; repeating the same key while pending
        // cannot duplicate the operation.
        cx.simulate_keystrokes(window.into(), "tab enter");
        assert!(
            panel
                .read_with(cx, |panel, cx| panel.commit_message(cx))
                .contains('\n')
        );
        assert_eq!(
            events
                .lock()
                .expect("events")
                .iter()
                .filter(|event| matches!(event, CapturedCommitEvent::Draft(_)))
                .count(),
            0
        );
        cx.simulate_keystrokes(window.into(), "tab enter enter space");
        let first_draft = events
            .lock()
            .expect("events")
            .iter()
            .find_map(|event| match event {
                CapturedCommitEvent::Draft(request) => Some(request.clone()),
                _ => None,
            })
            .expect("Enter draft");
        assert_eq!(
            events
                .lock()
                .expect("events")
                .iter()
                .filter(|event| matches!(event, CapturedCommitEvent::Draft(_)))
                .count(),
            1
        );
        let provider = Arc::new(vega_runtime::MockProvider::new(vec![
            vega_runtime::ScriptStep::events(vec![
                vega_runtime::ProviderEvent::TextDelta("feat: generated".into()),
                vega_runtime::ProviderEvent::Done {
                    stop_reason: vega_runtime::StopReason::End,
                },
            ]),
        ]));
        let draft = runtime
            .block_on(service.draft(
                prepared.id,
                "mock".into(),
                provider,
                tokio_util::sync::CancellationToken::new(),
            ))
            .expect("mock draft");
        assert!(panel.update(cx, |panel, cx| {
            panel.finish_draft(first_draft.operation_id, Ok(draft), cx)
        }));
        cx.simulate_keystrokes(window.into(), "space space");
        assert_eq!(
            events
                .lock()
                .expect("events")
                .iter()
                .filter(|event| matches!(event, CapturedCommitEvent::Draft(_)))
                .count(),
            2,
            "Generate Space is first-wins"
        );
        let second_draft = events
            .lock()
            .expect("events")
            .iter()
            .filter_map(|event| match event {
                CapturedCommitEvent::Draft(request) => Some(request.clone()),
                _ => None,
            })
            .nth(1)
            .expect("Space draft");
        let provider = Arc::new(vega_runtime::MockProvider::new(vec![
            vega_runtime::ScriptStep::events(vec![
                vega_runtime::ProviderEvent::TextDelta("feat: generated".into()),
                vega_runtime::ProviderEvent::Done {
                    stop_reason: vega_runtime::StopReason::End,
                },
            ]),
        ]));
        let draft = runtime
            .block_on(service.draft(
                prepared.id,
                "mock".into(),
                provider,
                tokio_util::sync::CancellationToken::new(),
            ))
            .expect("second mock draft");
        assert!(panel.update(cx, |panel, cx| {
            panel.finish_draft(second_draft.operation_id, Ok(draft), cx)
        }));
        cx.simulate_keystrokes(window.into(), "tab cmd-enter cmd-enter escape escape");
        let events = events.lock().expect("events");
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, CapturedCommitEvent::Commit(_)))
                .count(),
            1,
            "commit is exact first-wins"
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, CapturedCommitEvent::Close))
                .count(),
            1,
            "Esc close is exact first-wins"
        );
        let commit = events.iter().find_map(|event| match event {
            CapturedCommitEvent::Commit(request) => Some(request),
            _ => None,
        });
        assert!(commit.is_some_and(|request| request.prepared_id == prepared.id));
    }

    #[gpui::test]
    async fn commit_app_production_handlers_reconcile_before_release_across_close_and_routes_s6_controller(
        cx: &mut gpui::TestAppContext,
    ) {
        let repo = diff_controller_repo();
        let store = Store::open(":memory:").expect("commit production store");
        store.migrate().expect("commit production migrations");
        let project = vega_store::projects::create(
            store.conn(),
            repo.path().to_str().expect("UTF-8 commit root"),
            "commit-production",
            None,
        )
        .expect("commit production project");
        let thread = vega_conversation::threads::create_thread(
            &store,
            &project.id,
            "mock",
            PermissionMode::Confirm.as_str(),
        )
        .expect("commit production thread");
        cx.update(|cx| install_diff_window_globals(store, thread.clone(), cx));
        let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
        let panel = stream.read_with(cx, |stream, _| stream.commit_panel());
        let panel_root = panel.clone();
        let panel_window = cx
            .update(|cx| {
                cx.open_window(Default::default(), move |_, cx| {
                    cx.new(|_| CommitPanelHarness { panel: panel_root })
                })
            })
            .expect("commit production panel window");
        let provider = Arc::new(vega_runtime::MockProvider::new(vec![
            vega_runtime::ScriptStep::events(vec![
                vega_runtime::ProviderEvent::TextDelta("feat: generated".into()),
                vega_runtime::ProviderEvent::Done {
                    stop_reason: vega_runtime::StopReason::End,
                },
            ]),
        ]));
        let probe = Arc::new(CommitTestProbe::default());
        let root = cx.new(VegaWindow::new);
        root.update(cx, |root, cx| {
            root.commit_provider_override = Some(provider.clone());
            root.commit_test_probe = Some(probe.clone());
            root.stream_view = Some((thread.id.clone(), stream.clone()));
            root.ensure_artifact_route(&thread, stream.clone(), cx);
            root.ensure_branch_route(&thread, stream.clone(), cx);
            root.open_workspace_diff(
                stream.clone(),
                &OpenWorkspaceDiffRequested {
                    thread_id: thread.id.clone(),
                    project_id: thread.project_id.clone(),
                },
                cx,
            );
            cx.subscribe(&panel, |this, panel, request, cx| {
                this.request_commit_prepare(panel.clone(), request, cx);
            })
            .detach();
            cx.subscribe(&panel, |this, panel, request, cx| {
                this.request_commit_draft(panel.clone(), request, cx);
            })
            .detach();
            cx.subscribe(&panel, |this, panel, request, cx| {
                this.request_commit_execute(panel.clone(), request, cx);
            })
            .detach();
            cx.subscribe(&panel, |this, panel, request, cx| {
                this.commit_panel_closed(panel.clone(), request, cx);
            })
            .detach();
        });
        let (branch_service, branch_selector, artifact_service) = root.read_with(cx, |root, _| {
            let branch = root
                .branch_controller
                .active
                .as_ref()
                .expect("initial branch route");
            let artifacts = root
                .artifact_controller
                .active
                .as_ref()
                .expect("initial artifact route");
            (
                branch.service.clone(),
                branch.identity.selector.clone(),
                artifacts.service.clone(),
            )
        });
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("commit production runtime");
        branch_selector.update(cx, |selector, cx| {
            assert!(selector.request_open(cx));
        });
        let initial_branch_error = runtime
            .block_on(branch_service.refresh(tokio_util::sync::CancellationToken::new()))
            .expect_err("dirty initial branch state");
        assert_eq!(
            initial_branch_error.code(),
            GitWorkspaceErrorCode::BranchDirty
        );
        runtime
            .block_on(artifact_service.reconcile(tokio_util::sync::CancellationToken::new()))
            .expect("initial artifact reconciliation");
        branch_selector.update(cx, |selector, cx| {
            selector.apply_error(initial_branch_error.code(), cx);
        });
        pump_test_app(cx, |cx| {
            root.read_with(cx, |root, cx| {
                root.diff_controller
                    .active
                    .as_ref()
                    .is_some_and(|active| active.view.read(cx).generation().is_some())
            })
        });
        root.update(cx, |root, cx| {
            root.open_commit_panel(
                stream.clone(),
                &OpenCommitPanelRequested {
                    thread_id: thread.id.clone(),
                    project_id: thread.project_id.clone(),
                },
                cx,
            );
        });
        for _ in 0..400 {
            cx.executor().advance_clock(DIFF_RESULT_POLL);
            cx.run_until_parked();
            if panel.read_with(cx, |panel, _| panel.stage())
                == vega_ui::commit_panel::CommitPanelStage::Checklist
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            panel.read_with(cx, |panel, _| panel.stage()),
            vega_ui::commit_panel::CommitPanelStage::Checklist,
            "second checklist controller_open={} lease_busy={}",
            root.read_with(cx, |root, _| root.commit_controller.is_open()),
            root.read_with(cx, |root, _| root.trusted_actions.is_busy())
        );
        panel_window
            .update(cx, |_, window, cx| {
                let focus = panel.read(cx).focus_handle(cx);
                focus.focus(window, cx);
            })
            .expect("focus first checklist");
        cx.simulate_keystrokes(panel_window.into(), "tab space cmd-enter cmd-enter");
        assert_eq!(
            panel.read_with(cx, |panel, _| panel.stage()),
            vega_ui::commit_panel::CommitPanelStage::Preparing
        );
        let cached_clean = fixture_git_command(repo.path(), &["diff", "--cached", "--quiet"])
            .status()
            .expect("inspect prepare mutation")
            .success();
        assert!(!cached_clean, "prepare worker established owned B");
        assert_eq!(probe.prepare_workers.load(Ordering::SeqCst), 1);
        assert_eq!(
            root.read_with(cx, |root, _| {
                root.commit_controller
                    .active
                    .as_ref()
                    .map(|active| active.next_sequence)
            }),
            Some(2),
            "repeated prepare ingress starts one production fence"
        );
        cx.simulate_keystrokes(panel_window.into(), "escape");
        pump_test_app(cx, |cx| {
            root.read_with(cx, |root, _| {
                !root.commit_controller.is_open() && !root.trusted_actions.is_busy()
            })
        });
        root.read_with(cx, |root, cx| {
            let diff = root
                .diff_controller
                .active
                .as_ref()
                .expect("diff survives prepare close");
            assert!(diff.view.read(cx).generation().is_some());
            let branch = root
                .branch_controller
                .active
                .as_ref()
                .expect("branch survives prepare close");
            assert_eq!(
                branch.identity.selector.read(cx).snapshot_generation(),
                None,
                "dirty prepare invalidates the clean-only branch snapshot"
            );
            let artifacts = root
                .artifact_controller
                .active
                .as_ref()
                .expect("artifact survives prepare close");
            assert!(artifacts.terminal_in_flight.is_none());
        });

        // Reopen against owned B, prepare without another add, enter a real
        // message through TextInput, and close while commit owns the lease.
        root.update(cx, |root, cx| {
            root.open_commit_panel(
                stream.clone(),
                &OpenCommitPanelRequested {
                    thread_id: thread.id.clone(),
                    project_id: thread.project_id.clone(),
                },
                cx,
            );
        });
        for _ in 0..400 {
            cx.executor().advance_clock(DIFF_RESULT_POLL);
            cx.run_until_parked();
            if panel.read_with(cx, |panel, _| panel.stage())
                == vega_ui::commit_panel::CommitPanelStage::Checklist
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            panel.read_with(cx, |panel, _| panel.stage()),
            vega_ui::commit_panel::CommitPanelStage::Checklist,
            "reopen state controller_open={} lease_busy={}",
            root.read_with(cx, |root, _| root.commit_controller.is_open()),
            root.read_with(cx, |root, _| root.trusted_actions.is_busy())
        );
        panel_window
            .update(cx, |_, window, cx| {
                let focus = panel.read(cx).focus_handle(cx);
                focus.focus(window, cx);
            })
            .expect("focus second checklist");
        probe
            .trace
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clear();
        cx.simulate_keystrokes(panel_window.into(), "tab cmd-enter cmd-enter");
        for _ in 0..400 {
            cx.executor().advance_clock(DIFF_RESULT_POLL);
            cx.run_until_parked();
            if panel.read_with(cx, |panel, _| panel.stage())
                == vega_ui::commit_panel::CommitPanelStage::CommitReady
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            panel.read_with(cx, |panel, _| panel.stage()),
            vega_ui::commit_panel::CommitPanelStage::CommitReady,
            "prepare ready controller_open={} lease_busy={}",
            root.read_with(cx, |root, _| root.commit_controller.is_open()),
            root.read_with(cx, |root, _| root.trusted_actions.is_busy())
        );
        assert_eq!(probe.prepare_workers.load(Ordering::SeqCst), 2);
        assert_eq!(
            probe
                .trace
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .as_slice(),
            [
                "workspace_candidate",
                "branch_result",
                "artifact_result",
                "workspace_final",
                "ui_diff",
                "ui_branch",
                "ui_artifact",
                "panel_terminal",
            ],
            "Prepare consumers must precede CommitReady and retain the lease"
        );
        cx.simulate_keystrokes(panel_window.into(), "tab tab enter enter");
        assert_eq!(
            panel.read_with(cx, |panel, _| panel.stage()),
            vega_ui::commit_panel::CommitPanelStage::Drafting
        );
        pump_test_app(cx, |cx| {
            panel.read_with(cx, |panel, cx| {
                panel.stage() == vega_ui::commit_panel::CommitPanelStage::CommitReady
                    && panel.commit_message(cx) == "feat: generated"
            })
        });
        assert_eq!(probe.draft_workers.load(Ordering::SeqCst), 1);
        assert_eq!(provider.requests().len(), 1, "draft provider is exact once");
        probe
            .trace
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clear();
        let terminal_before_commit = probe.terminal_applications.load(Ordering::SeqCst);
        probe.drop_commit_sender.store(true, Ordering::SeqCst);
        cx.simulate_keystrokes(panel_window.into(), "tab cmd-enter cmd-enter");
        assert_eq!(
            panel.read_with(cx, |panel, _| panel.stage()),
            vega_ui::commit_panel::CommitPanelStage::Committing
        );
        let commit_count = fixture_git_command(repo.path(), &["rev-list", "--count", "HEAD"])
            .output()
            .expect("inspect commit mutation");
        assert!(commit_count.status.success());
        assert_eq!(commit_count.stdout, b"2\n");
        cx.simulate_keystrokes(panel_window.into(), "escape");
        pump_test_app(cx, |cx| {
            root.read_with(cx, |root, _| {
                !root.commit_controller.is_open() && !root.trusted_actions.is_busy()
            })
        });
        assert_eq!(probe.commit_workers.load(Ordering::SeqCst), 1);
        assert_eq!(
            probe.terminal_applications.load(Ordering::SeqCst),
            terminal_before_commit + 1,
            "disconnected completion applies exactly one accepted terminal"
        );
        let trace = probe
            .trace
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone();
        assert_eq!(
            trace
                .iter()
                .filter(|event| **event == "workspace_final")
                .count(),
            2,
            "the dropped result is followed by one authoritative recovery"
        );
        for event in ["ui_diff", "ui_branch", "ui_artifact", "panel_terminal"] {
            assert_eq!(
                trace
                    .iter()
                    .filter(|candidate| **candidate == event)
                    .count(),
                1,
                "visible terminal event is exact once: {event}"
            );
        }
        let panel_terminal = trace
            .iter()
            .position(|event| *event == "panel_terminal")
            .expect("accepted panel terminal trace");
        assert!(
            ["ui_diff", "ui_branch", "ui_artifact"]
                .into_iter()
                .all(
                    |event| trace.iter().position(|candidate| *candidate == event)
                        < Some(panel_terminal)
                ),
            "visible consumers precede the panel terminal"
        );
        assert_eq!(
            trace.last(),
            Some(&"lease_release"),
            "exact shared lease release remains the final action"
        );
        let status = fixture_git_command(repo.path(), &["status", "--porcelain=v1"])
            .output()
            .expect("post-commit status");
        assert!(status.status.success());
        assert!(status.stdout.is_empty(), "commit leaves repository clean");
        let post_commit_branch = runtime
            .block_on(branch_service.refresh(tokio_util::sync::CancellationToken::new()))
            .expect("post-commit branch service refresh");
        root.read_with(cx, |root, cx| {
            assert!(
                root.diff_controller
                    .active
                    .as_ref()
                    .is_some_and(|active| active.view.read(cx).generation().is_some())
            );
            assert!(
                root.branch_controller
                    .active
                    .as_ref()
                    .is_some_and(
                        |active| active.identity.selector.read(cx).snapshot_generation()
                            == Some(post_commit_branch.generation)
                    )
            );
            assert!(root.artifact_controller.active.is_some());
        });

        let actions = root.read_with(cx, |root, _| root.trusted_actions.clone());
        root.update(cx, |root, _| root.window_terminal_cleanup());
        pump_test_app(cx, |_| !actions.is_busy());
        panel_window
            .update(cx, |_, window, _| window.remove_window())
            .expect("close commit production panel window");
        cx.run_until_parked();
    }

    #[gpui::test]
    async fn branch_controller_route_and_active_guards_fail_closed(cx: &mut gpui::TestAppContext) {
        let repo = artifact_controller_repo();
        let store = Store::open(":memory:").expect("branch window memory store");
        store.migrate().expect("branch window migrations");
        let project = vega_store::projects::create(
            store.conn(),
            repo.path().to_str().expect("UTF-8 branch root"),
            "branch",
            None,
        )
        .expect("branch project");
        let thread = vega_conversation::threads::create_thread(
            &store,
            &project.id,
            "mock",
            PermissionMode::Confirm.as_str(),
        )
        .expect("branch thread");
        cx.update(|cx| install_diff_window_globals(store, thread.clone(), cx));
        let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
        let root = cx.new(VegaWindow::new);
        root.update(cx, |root, cx| {
            root.stream_view = Some((thread.id.clone(), stream.clone()));
            root.ensure_branch_route(&thread, stream.clone(), cx);
            let active = root
                .branch_controller
                .active
                .as_ref()
                .expect("current branch route");
            assert!(VegaWindow::branch_route_is_current(&active.identity, cx));
            assert_eq!(active.identity.stream, stream);
            assert_eq!(active.identity.selector, stream.read(cx).branch_selector());
            assert!(root.branch_guards_clear(&stream, cx));

            let lease = root
                .trusted_actions
                .acquire(TrustedActionKind::Commit, 99, 1)
                .expect("future commit lease");
            assert!(!root.branch_guards_clear(&stream, cx));
            assert!(root.trusted_actions.release(lease));

            let (generation, _) =
                root.agent_controller
                    .begin(thread.id.clone(), stream.clone(), None, None);
            assert!(!root.branch_guards_clear(&stream, cx));
            let _ = root
                .agent_controller
                .finish(generation, &thread.id, &stream)
                .expect("finish guard run");

            stream.update(cx, |stream, cx| {
                stream.apply_plan(
                    Plan {
                        id: "pending-branch-plan".into(),
                        thread_id: thread.id.clone(),
                        content: "Inspect before switch".into(),
                        status: PlanStatus::Pending,
                        review_note: None,
                        reviewed_at: None,
                    },
                    cx,
                );
            });
            assert!(!root.branch_guards_clear(&stream, cx));

            cx.set_global(SettingsOpen(true));
            assert!(!VegaWindow::branch_route_is_current(
                &root
                    .branch_controller
                    .active
                    .as_ref()
                    .expect("route before settings close")
                    .identity,
                cx,
            ));
        });
    }

    #[gpui::test]
    async fn branch_controller_guard_change_after_preflight_starts_zero_execute(
        cx: &mut gpui::TestAppContext,
    ) {
        let repo = artifact_controller_repo();
        run_fixture_git(repo.path(), &["branch", "other"]);
        let store = Store::open(":memory:").expect("branch preflight store");
        store.migrate().expect("branch preflight migrations");
        let project = vega_store::projects::create(
            store.conn(),
            repo.path().to_str().expect("UTF-8 branch root"),
            "branch",
            None,
        )
        .expect("branch project");
        let thread = vega_conversation::threads::create_thread(
            &store,
            &project.id,
            "mock",
            PermissionMode::Confirm.as_str(),
        )
        .expect("branch thread");
        cx.update(|cx| install_diff_window_globals(store, thread.clone(), cx));
        let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
        let selector = stream.read_with(cx, |stream, _| stream.branch_selector());
        let root = cx.new(VegaWindow::new);
        let (identity, service) = root.update(cx, |root, cx| {
            root.stream_view = Some((thread.id.clone(), stream.clone()));
            root.ensure_branch_route(&thread, stream.clone(), cx);
            let active = root
                .branch_controller
                .active
                .as_ref()
                .expect("branch route");
            (active.identity.clone(), active.service.clone())
        });
        let list_fence = BranchListFence {
            route: identity.clone(),
            sequence: 1,
        };
        let (list_sender, list_receiver) = mpsc::sync_channel(1);
        run_branch_list_worker(
            service.clone(),
            list_fence,
            tokio_util::sync::CancellationToken::new(),
            list_sender,
        );
        let (_, snapshot) = list_receiver.recv().expect("branch snapshot output");
        let snapshot = snapshot.expect("branch snapshot");
        let target = snapshot
            .branches
            .iter()
            .find(|branch| !branch.current)
            .expect("switch target")
            .id;
        let operation = selector.update(cx, |selector, cx| {
            assert!(selector.request_open(cx));
            assert!(selector.apply_snapshot(snapshot.clone(), cx));
            selector
                .begin_switch(snapshot.generation, target, cx)
                .expect("switch operation")
        });
        let prepare_fence = BranchPrepareFence {
            route: identity,
            sequence: 1,
            snapshot_generation: snapshot.generation,
            branch_id: target,
            operation_id: operation,
        };
        root.update(cx, |root, _| {
            let active = root
                .branch_controller
                .active
                .as_mut()
                .expect("active preflight route");
            active.switch_sequence = 1;
            active.prepare_fence = Some(prepare_fence.clone());
            active.switch_cancel = Some(tokio_util::sync::CancellationToken::new());
        });
        let (prepare_sender, prepare_receiver) = mpsc::sync_channel(1);
        run_branch_prepare_worker(
            service,
            prepare_fence.clone(),
            tokio_util::sync::CancellationToken::new(),
            prepare_sender,
        );
        let (_, permit) = prepare_receiver.recv().expect("preflight output");
        let permit = permit.expect("valid preflight permit");
        root.update(cx, |root, cx| {
            let competing = root
                .trusted_actions
                .acquire(TrustedActionKind::Commit, 42, 1)
                .expect("guard changes after preflight");
            root.finish_branch_prepare(prepare_fence, Ok(permit), cx);
            assert!(
                root.branch_controller
                    .active
                    .as_ref()
                    .is_some_and(|active| active.switch_fence.is_none()),
                "guard change starts zero execute"
            );
            assert_eq!(root.trusted_actions.active_token(), Some(competing));
            assert!(root.trusted_actions.release(competing));
        });
        let output = fixture_git_command(repo.path(), &["symbolic-ref", "--short", "HEAD"])
            .output()
            .expect("read current branch");
        assert!(output.status.success());
        assert_ne!(output.stdout, b"other\n", "preflight alone never mutates");
        assert!(!selector.read_with(cx, |selector, _| selector.is_pending()));
    }

    #[gpui::test]
    async fn branch_controller_close_during_preflight_clears_exact_pending_then_reopens(
        cx: &mut gpui::TestAppContext,
    ) {
        let repo = artifact_controller_repo();
        run_fixture_git(repo.path(), &["branch", "preflight-close-target"]);
        let store = Store::open(":memory:").expect("branch close preflight store");
        store.migrate().expect("branch close preflight migrations");
        let project = vega_store::projects::create(
            store.conn(),
            repo.path().to_str().expect("UTF-8 branch root"),
            "branch",
            None,
        )
        .expect("branch project");
        let thread = vega_conversation::threads::create_thread(
            &store,
            &project.id,
            "mock",
            PermissionMode::Confirm.as_str(),
        )
        .expect("branch thread");
        cx.update(|cx| install_diff_window_globals(store, thread.clone(), cx));
        let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
        let selector = stream.read_with(cx, |stream, _| stream.branch_selector());
        let root = cx.new(VegaWindow::new);
        let (identity, service) = root.update(cx, |root, cx| {
            root.stream_view = Some((thread.id.clone(), stream.clone()));
            root.ensure_branch_route(&thread, stream.clone(), cx);
            let active = root
                .branch_controller
                .active
                .as_ref()
                .expect("branch close preflight route");
            (active.identity.clone(), active.service.clone())
        });
        let (list_sender, list_receiver) = mpsc::sync_channel(1);
        run_branch_list_worker(
            service.clone(),
            BranchListFence {
                route: identity.clone(),
                sequence: 1,
            },
            tokio_util::sync::CancellationToken::new(),
            list_sender,
        );
        let snapshot = list_receiver
            .recv()
            .expect("close preflight list output")
            .1
            .expect("close preflight snapshot");
        let target = snapshot
            .branches
            .iter()
            .find(|branch| !branch.current)
            .expect("close preflight target")
            .id;
        let operation = selector.update(cx, |selector, cx| {
            assert!(selector.request_open(cx));
            assert!(selector.apply_snapshot(snapshot.clone(), cx));
            selector
                .begin_switch(snapshot.generation, target, cx)
                .expect("close preflight operation")
        });
        let fence = BranchPrepareFence {
            route: identity,
            sequence: 1,
            snapshot_generation: snapshot.generation,
            branch_id: target,
            operation_id: operation,
        };
        let cancel = tokio_util::sync::CancellationToken::new();
        root.update(cx, |root, _| {
            let active = root
                .branch_controller
                .active
                .as_mut()
                .expect("active close preflight route");
            active.switch_sequence = 1;
            active.prepare_fence = Some(fence.clone());
            active.switch_cancel = Some(cancel.clone());
        });
        selector.update(cx, |selector, cx| {
            assert!(selector.request_close(cx));
        });
        root.update(cx, |root, cx| {
            root.branch_selector_closed(
                selector.clone(),
                &BranchSelectorClosed {
                    thread_id: thread.id.clone(),
                    project_id: thread.project_id.clone(),
                },
                cx,
            );
        });
        assert!(cancel.is_cancelled());
        assert_eq!(
            selector.read_with(cx, |selector, _| selector.pending_key()),
            Some((operation, snapshot.generation, target))
        );

        cx.update(|cx| cx.set_global(SettingsOpen(true)));
        cx.run_until_parked();
        assert!(
            !selector.read_with(cx, |selector, _| selector.is_pending()),
            "route close synchronously clears only its exact operation"
        );
        cx.update(|cx| cx.set_global(SettingsOpen(false)));
        let (fresh_identity, fresh_service) = root.update(cx, |root, cx| {
            root.ensure_branch_route(&thread, stream.clone(), cx);
            let active = root
                .branch_controller
                .active
                .as_ref()
                .expect("restored preflight route");
            (active.identity.clone(), active.service.clone())
        });

        let (prepare_sender, prepare_receiver) = mpsc::sync_channel(1);
        run_branch_prepare_worker(service, fence.clone(), cancel, prepare_sender);
        let (_, result) = prepare_receiver.recv().expect("close preflight terminal");
        root.update(cx, |root, cx| root.finish_branch_prepare(fence, result, cx));
        assert!(!selector.read_with(cx, |selector, _| selector.is_pending()));
        let (fresh_sender, fresh_receiver) = mpsc::sync_channel(1);
        run_branch_list_worker(
            fresh_service,
            BranchListFence {
                route: fresh_identity,
                sequence: 1,
            },
            tokio_util::sync::CancellationToken::new(),
            fresh_sender,
        );
        let fresh_snapshot = fresh_receiver
            .recv()
            .expect("fresh preflight list")
            .1
            .expect("fresh preflight snapshot");
        let fresh_target = fresh_snapshot
            .branches
            .iter()
            .find(|branch| !branch.current)
            .expect("fresh preflight target")
            .id;
        let fresh_operation = selector.update(cx, |selector, cx| {
            assert!(selector.request_open(cx), "fresh list request is reusable");
            assert!(selector.apply_snapshot(fresh_snapshot.clone(), cx));
            selector
                .begin_switch(fresh_snapshot.generation, fresh_target, cx)
                .expect("fresh preflight operation")
        });
        root.update(cx, |root, cx| {
            root.request_branch_switch(
                selector.clone(),
                &BranchSwitchRequested {
                    thread_id: thread.id.clone(),
                    project_id: thread.project_id.clone(),
                    snapshot_generation: fresh_snapshot.generation,
                    branch_id: fresh_target,
                    operation_id: fresh_operation,
                },
                cx,
            );
            assert!(
                root.branch_controller
                    .active
                    .as_ref()
                    .is_some_and(|active| active.prepare_fence.is_some())
            );
            root.close_branch_route(GitWorkspaceErrorCode::Cancelled, cx);
        });
    }

    #[gpui::test]
    async fn branch_controller_close_cancels_owner_but_releases_only_after_cleanup(
        cx: &mut gpui::TestAppContext,
    ) {
        let repo = artifact_controller_repo();
        run_fixture_git(repo.path(), &["branch", "cancel-target"]);
        let store = Store::open(":memory:").expect("branch cancel store");
        store.migrate().expect("branch cancel migrations");
        let project = vega_store::projects::create(
            store.conn(),
            repo.path().to_str().expect("UTF-8 branch root"),
            "branch",
            None,
        )
        .expect("branch project");
        let thread = vega_conversation::threads::create_thread(
            &store,
            &project.id,
            "mock",
            PermissionMode::Confirm.as_str(),
        )
        .expect("branch thread");
        cx.update(|cx| install_diff_window_globals(store, thread.clone(), cx));
        let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
        let selector = stream.read_with(cx, |stream, _| stream.branch_selector());
        let root = cx.new(VegaWindow::new);
        let (identity, service) = root.update(cx, |root, cx| {
            root.stream_view = Some((thread.id.clone(), stream.clone()));
            root.ensure_branch_route(&thread, stream.clone(), cx);
            let active = root
                .branch_controller
                .active
                .as_ref()
                .expect("branch cancel route");
            (active.identity.clone(), active.service.clone())
        });
        let list_fence = BranchListFence {
            route: identity.clone(),
            sequence: 1,
        };
        let (list_sender, list_receiver) = mpsc::sync_channel(1);
        run_branch_list_worker(
            service.clone(),
            list_fence,
            tokio_util::sync::CancellationToken::new(),
            list_sender,
        );
        let snapshot = list_receiver
            .recv()
            .expect("list output")
            .1
            .expect("list snapshot");
        let target = snapshot
            .branches
            .iter()
            .find(|branch| !branch.current)
            .expect("cancel target")
            .id;
        let operation = selector.update(cx, |selector, cx| {
            assert!(selector.request_open(cx));
            assert!(selector.apply_snapshot(snapshot.clone(), cx));
            selector
                .begin_switch(snapshot.generation, target, cx)
                .expect("cancel owner operation")
        });
        let prepare_fence = BranchPrepareFence {
            route: identity.clone(),
            sequence: 1,
            snapshot_generation: snapshot.generation,
            branch_id: target,
            operation_id: operation,
        };
        let (prepare_sender, prepare_receiver) = mpsc::sync_channel(1);
        run_branch_prepare_worker(
            service.clone(),
            prepare_fence,
            tokio_util::sync::CancellationToken::new(),
            prepare_sender,
        );
        let permit = prepare_receiver
            .recv()
            .expect("prepare output")
            .1
            .expect("prepare permit");
        let cancel = tokio_util::sync::CancellationToken::new();
        let fence = root.update(cx, |root, cx| {
            let lease = root
                .trusted_actions
                .acquire(TrustedActionKind::BranchSwitch, identity.epoch, 1)
                .expect("branch owner lease");
            stream.update(cx, |stream, cx| stream.set_trusted_action_busy(true, cx));
            let fence = BranchSwitchFence {
                route: identity,
                sequence: 1,
                snapshot_generation: snapshot.generation,
                branch_id: target,
                operation_id: operation,
                lease,
            };
            let active = root
                .branch_controller
                .active
                .as_mut()
                .expect("branch owner route");
            active.switch_fence = Some(fence.clone());
            active.switch_cancel = Some(cancel.clone());
            fence
        });
        selector.update(cx, |selector, cx| {
            assert!(selector.request_close(cx));
        });
        root.update(cx, |root, cx| {
            root.branch_selector_closed(
                selector.clone(),
                &BranchSelectorClosed {
                    thread_id: thread.id.clone(),
                    project_id: thread.project_id.clone(),
                },
                cx,
            );
            assert!(cancel.is_cancelled());
            assert!(root.trusted_actions.is_busy(), "close cannot release owner");
        });
        assert_eq!(
            selector.read_with(cx, |selector, _| selector.pending_key()),
            Some((operation, snapshot.generation, target))
        );
        cx.update(|cx| cx.set_global(SettingsOpen(true)));
        cx.run_until_parked();
        assert!(!selector.read_with(cx, |selector, _| selector.is_pending()));
        root.update(cx, |root, _| {
            assert!(
                root.trusted_actions.is_busy(),
                "settings cannot release owner"
            );
        });
        cx.update(|cx| cx.set_global(SettingsOpen(false)));
        let fresh_service = root.update(cx, |root, cx| {
            root.ensure_branch_route(&thread, stream.clone(), cx);
            root.branch_controller
                .active
                .as_ref()
                .expect("restored owner route")
                .service
                .clone()
        });
        let (sender, receiver) = mpsc::sync_channel(1);
        run_branch_switch_worker(service, permit, fence.clone(), cancel, sender);
        let (_, completion) = receiver.recv().expect("cancelled owner completion");
        assert!(matches!(
            completion.outcome,
            BranchSwitchOutcome::Failed(GitWorkspaceErrorCode::Cancelled)
        ));
        assert!(
            completion.snapshot.is_some(),
            "owner cancellation still returns authoritative refresh"
        );
        assert!(completion.snapshot.is_some());
        root.update(cx, |root, cx| {
            root.finish_branch_switch(fence, completion, cx);
            assert!(
                !root.trusted_actions.is_busy(),
                "cleanup completion releases exact owner"
            );
        });
        assert!(!selector.read_with(cx, |selector, _| selector.is_pending()));
        let (fresh_sender, fresh_receiver) = mpsc::sync_channel(1);
        run_branch_list_worker(
            fresh_service,
            BranchListFence {
                route: root.read_with(cx, |root, _| {
                    root.branch_controller
                        .active
                        .as_ref()
                        .expect("fresh owner identity")
                        .identity
                        .clone()
                }),
                sequence: 1,
            },
            tokio_util::sync::CancellationToken::new(),
            fresh_sender,
        );
        let refreshed = fresh_receiver
            .recv()
            .expect("fresh owner list")
            .1
            .expect("fresh owner snapshot");
        let fresh_generation = refreshed.generation;
        let fresh_target = refreshed
            .branches
            .iter()
            .find(|branch| !branch.current)
            .expect("fresh target after owner cleanup")
            .id;
        selector.update(cx, |selector, cx| {
            assert!(selector.request_open(cx), "selector reopens after cleanup");
            assert!(selector.apply_snapshot(refreshed, cx));
            assert!(
                selector
                    .begin_switch(fresh_generation, fresh_target, cx)
                    .is_some()
            );
        });
        assert!(!stream.read_with(cx, |stream, _| stream.has_active_agent()));
    }

    #[gpui::test]
    async fn branch_controller_s6_controller_owner_success_applies_authority_then_releases(
        cx: &mut gpui::TestAppContext,
    ) {
        let repo = artifact_controller_repo();
        run_fixture_git(repo.path(), &["branch", "success-target"]);
        let store = Store::open(":memory:").expect("branch success store");
        store.migrate().expect("branch success migrations");
        let project = vega_store::projects::create(
            store.conn(),
            repo.path().to_str().expect("UTF-8 branch root"),
            "branch",
            None,
        )
        .expect("branch project");
        let thread = vega_conversation::threads::create_thread(
            &store,
            &project.id,
            "mock",
            PermissionMode::Confirm.as_str(),
        )
        .expect("branch thread");
        cx.update(|cx| install_diff_window_globals(store, thread.clone(), cx));
        let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
        let selector = stream.read_with(cx, |stream, _| stream.branch_selector());
        let root = cx.new(VegaWindow::new);
        let (identity, service) = root.update(cx, |root, cx| {
            root.stream_view = Some((thread.id.clone(), stream.clone()));
            root.ensure_branch_route(&thread, stream.clone(), cx);
            let active = root
                .branch_controller
                .active
                .as_ref()
                .expect("branch success route");
            (active.identity.clone(), active.service.clone())
        });
        let (list_sender, list_receiver) = mpsc::sync_channel(1);
        run_branch_list_worker(
            service.clone(),
            BranchListFence {
                route: identity.clone(),
                sequence: 1,
            },
            tokio_util::sync::CancellationToken::new(),
            list_sender,
        );
        let snapshot = list_receiver
            .recv()
            .expect("success list output")
            .1
            .expect("success list snapshot");
        let target = snapshot
            .branches
            .iter()
            .find(|branch| branch.label == "success-target")
            .expect("success target")
            .id;
        let operation = selector.update(cx, |selector, cx| {
            assert!(selector.request_open(cx));
            assert!(selector.apply_snapshot(snapshot.clone(), cx));
            selector
                .begin_switch(snapshot.generation, target, cx)
                .expect("success owner operation")
        });
        let prepare_fence = BranchPrepareFence {
            route: identity.clone(),
            sequence: 1,
            snapshot_generation: snapshot.generation,
            branch_id: target,
            operation_id: operation,
        };
        let (prepare_sender, prepare_receiver) = mpsc::sync_channel(1);
        run_branch_prepare_worker(
            service.clone(),
            prepare_fence,
            tokio_util::sync::CancellationToken::new(),
            prepare_sender,
        );
        let permit = prepare_receiver
            .recv()
            .expect("success prepare output")
            .1
            .expect("success permit");
        let fence = root.update(cx, |root, cx| {
            let lease = root
                .trusted_actions
                .acquire(TrustedActionKind::BranchSwitch, identity.epoch, 1)
                .expect("success owner lease");
            stream.update(cx, |stream, cx| stream.set_trusted_action_busy(true, cx));
            let fence = BranchSwitchFence {
                route: identity,
                sequence: 1,
                snapshot_generation: snapshot.generation,
                branch_id: target,
                operation_id: operation,
                lease,
            };
            let active = root
                .branch_controller
                .active
                .as_mut()
                .expect("success owner route");
            active.switch_fence = Some(fence.clone());
            fence
        });
        let (sender, receiver) = mpsc::sync_channel(1);
        run_branch_switch_worker(
            service,
            permit,
            fence.clone(),
            tokio_util::sync::CancellationToken::new(),
            sender,
        );
        let (_, completion) = receiver.recv().expect("success owner completion");
        assert_eq!(completion.outcome, BranchSwitchOutcome::Switched);
        assert!(completion.snapshot.is_some());
        let authoritative = completion
            .snapshot
            .clone()
            .expect("success authoritative snapshot");
        let duplicate_fence = fence.clone();
        let duplicate_completion = completion.clone();
        root.update(cx, |root, cx| {
            root.finish_branch_switch(fence, completion, cx);
            assert!(!root.trusted_actions.is_busy());
        });
        assert!(!selector.read_with(cx, |selector, _| selector.is_open()));
        let output = fixture_git_command(repo.path(), &["symbolic-ref", "--short", "HEAD"])
            .output()
            .expect("read switched branch");
        assert!(output.status.success());
        assert_eq!(output.stdout, b"success-target\n");

        let fresh_target = authoritative
            .branches
            .iter()
            .find(|branch| !branch.current)
            .expect("fresh switch target")
            .id;
        let fresh_operation = selector.update(cx, |selector, cx| {
            assert!(selector.request_open(cx));
            assert!(selector.apply_snapshot(authoritative.clone(), cx));
            selector
                .begin_switch(authoritative.generation, fresh_target, cx)
                .expect("fresh owner operation")
        });
        let preview_cancel = tokio_util::sync::CancellationToken::new();
        let open_cancel = tokio_util::sync::CancellationToken::new();
        let fresh_fence = root.update(cx, |root, cx| {
            root.ensure_artifact_route(&thread, stream.clone(), cx);
            let active_artifact = root
                .artifact_controller
                .active
                .as_mut()
                .expect("fresh artifact route");
            active_artifact.preview_cancel = Some(preview_cancel.clone());
            active_artifact.open_cancel = Some(open_cancel.clone());

            let lease = root
                .trusted_actions
                .acquire(
                    TrustedActionKind::BranchSwitch,
                    duplicate_fence.route.epoch,
                    2,
                )
                .expect("fresh branch owner lease");
            let fresh = BranchSwitchFence {
                route: duplicate_fence.route.clone(),
                sequence: 2,
                snapshot_generation: authoritative.generation,
                branch_id: fresh_target,
                operation_id: fresh_operation,
                lease,
            };
            let active = root
                .branch_controller
                .active
                .as_mut()
                .expect("fresh branch route");
            active.switch_fence = Some(fresh.clone());
            active.switch_cancel = Some(tokio_util::sync::CancellationToken::new());

            root.finish_branch_switch(duplicate_fence, duplicate_completion, cx);
            assert!(
                root.branch_controller
                    .active
                    .as_ref()
                    .is_some_and(|active| active.switch_fence.as_ref() == Some(&fresh)),
                "old duplicate cannot claim the fresh branch fence"
            );
            assert!(root.trusted_actions.is_busy());
            assert!(!preview_cancel.is_cancelled());
            assert!(!open_cancel.is_cancelled());
            fresh
        });
        assert_eq!(
            selector.read_with(cx, |selector, _| selector.pending_key()),
            Some((fresh_operation, authoritative.generation, fresh_target,)),
            "old terminal cannot clear the fresh operation token"
        );
        root.update(cx, |root, cx| {
            root.finish_branch_switch(
                fresh_fence,
                BranchSwitchCompletion {
                    outcome: BranchSwitchOutcome::Failed(GitWorkspaceErrorCode::Cancelled),
                    snapshot: Some(authoritative),
                },
                cx,
            );
            assert!(!root.trusted_actions.is_busy());
        });
    }

    #[gpui::test]
    async fn branch_selector_real_projection_keyboard_first_wins_and_visible_range(
        cx: &mut gpui::TestAppContext,
    ) {
        let repo = artifact_controller_repo();
        run_fixture_git(repo.path(), &["branch", "aaa-selector"]);
        run_fixture_git(repo.path(), &["branch", "zzz-selector"]);
        let store = Store::open(":memory:").expect("branch selector interaction store");
        store
            .migrate()
            .expect("branch selector interaction migrations");
        let project = vega_store::projects::create(
            store.conn(),
            repo.path().to_str().expect("UTF-8 branch selector root"),
            "branch",
            None,
        )
        .expect("branch selector project");
        let thread = vega_conversation::threads::create_thread(
            &store,
            &project.id,
            "mock",
            PermissionMode::Confirm.as_str(),
        )
        .expect("branch selector thread");
        cx.update(|cx| install_diff_window_globals(store, thread.clone(), cx));
        let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
        let selector = stream.read_with(cx, |stream, _| stream.branch_selector());
        let root = cx.new(VegaWindow::new);
        let (identity, service) = root.update(cx, |root, cx| {
            root.stream_view = Some((thread.id.clone(), stream.clone()));
            root.ensure_branch_route(&thread, stream, cx);
            let active = root
                .branch_controller
                .active
                .as_ref()
                .expect("branch selector interaction route");
            (active.identity.clone(), active.service.clone())
        });
        let (sender, receiver) = mpsc::sync_channel(1);
        run_branch_list_worker(
            service,
            BranchListFence {
                route: identity,
                sequence: 1,
            },
            tokio_util::sync::CancellationToken::new(),
            sender,
        );
        let snapshot = receiver
            .recv()
            .expect("branch selector interaction list")
            .1
            .expect("branch selector interaction snapshot");
        let current = snapshot
            .branches
            .iter()
            .find(|branch| branch.current)
            .expect("current branch")
            .id;
        let switchable = snapshot
            .branches
            .iter()
            .filter(|branch| !branch.current)
            .map(|branch| branch.id)
            .collect::<Vec<_>>();
        assert_eq!(switchable.len(), 2);

        let window_selector = selector.clone();
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), move |_, _| window_selector)
                .expect("branch selector interaction window")
        });
        selector.update(cx, |selector, cx| {
            assert!(selector.request_open(cx));
            assert!(selector.apply_snapshot(snapshot.clone(), cx));
            assert!(
                selector
                    .begin_switch(snapshot.generation, current, cx)
                    .is_none(),
                "current branch is never activatable"
            );
        });
        window
            .update(cx, |selector, window, cx| {
                let focus = selector.focus_handle(cx);
                window.focus(&focus, cx);
            })
            .expect("focus branch selector");
        cx.run_until_parked();
        assert_eq!(
            selector.read_with(cx, |selector, _| selector.focused_branch()),
            Some(switchable[0])
        );
        cx.simulate_keystrokes(window.into(), "up");
        assert_eq!(
            selector.read_with(cx, |selector, _| selector.focused_branch()),
            Some(switchable[0]),
            "up does not wrap before first switchable row"
        );
        cx.simulate_keystrokes(window.into(), "down down");
        assert_eq!(
            selector.read_with(cx, |selector, _| selector.focused_branch()),
            Some(switchable[1]),
            "down skips current and does not wrap past the end"
        );
        cx.simulate_keystrokes(window.into(), "enter");
        let pending = selector
            .read_with(cx, |selector, _| selector.pending_key())
            .expect("Enter activates focused branch");
        cx.simulate_keystrokes(window.into(), "space");
        assert_eq!(
            selector.read_with(cx, |selector, _| selector.pending_key()),
            Some(pending),
            "Space cannot replace a pending first winner"
        );
        cx.simulate_keystrokes(window.into(), "escape");
        assert!(!selector.read_with(cx, |selector, _| selector.is_open()));
        assert_eq!(
            selector.read_with(cx, |selector, _| selector.pending_key()),
            Some(pending),
            "Esc closes visibility without forging terminal cleanup"
        );

        selector.update(cx, |selector, cx| {
            assert!(selector.clear_pending(pending.0, pending.1, pending.2, cx));
            assert!(selector.request_open(cx));
            let template = snapshot
                .branches
                .iter()
                .find(|branch| !branch.current)
                .expect("large-list template")
                .clone();
            let large = BranchSnapshot {
                generation: snapshot.generation,
                branches: vec![template.clone(); vega_ui::branch_selector::BRANCH_LIMIT],
            };
            assert!(selector.apply_snapshot(large, cx));
            let visible = selector.visible_rows(4_321..4_329);
            assert_eq!(visible.len(), 8);
            assert_eq!(visible.first().map(|row| row.0), Some(4_321));
            assert_eq!(visible.last().map(|row| row.0), Some(4_328));
            assert_eq!(vega_ui::branch_selector::BRANCH_ROW_HEIGHT, 24.0);
        });
        cx.simulate_keystrokes(window.into(), "space");
        let space_pending = selector
            .read_with(cx, |selector, _| selector.pending_key())
            .expect("Space activates focused branch");
        selector.update(cx, |selector, cx| {
            assert!(selector.clear_pending(space_pending.0, space_pending.1, space_pending.2, cx,));
            let template = snapshot
                .branches
                .iter()
                .find(|branch| !branch.current)
                .expect("over-limit template")
                .clone();
            let too_large = BranchSnapshot {
                generation: snapshot.generation,
                branches: vec![template; vega_ui::branch_selector::BRANCH_LIMIT + 1],
            };
            assert!(!selector.apply_snapshot(too_large, cx));
        });
    }

    fn pending_plan() -> (Store, String) {
        let store = Store::open(":memory:").expect("memory store");
        store.migrate().expect("migrations");
        let project = vega_store::projects::create(
            store.conn(),
            "/tmp/vega-controller-plan-test",
            "controller",
            None,
        )
        .expect("project");
        let thread = vega_conversation::threads::create_thread(
            &store,
            &project.id,
            "mock",
            PermissionMode::Confirm.as_str(),
        )
        .expect("thread");
        vega_conversation::threads::set_thread_mode(&store, &thread.id, ThreadMode::Plan)
            .expect("plan mode");
        insert(
            store.conn(),
            &MessageRow {
                id: "plan".into(),
                thread_id: thread.id.clone(),
                seq: 1,
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
        .expect("streaming plan");
        complete_plan(
            store.conn(),
            &thread.id,
            "plan",
            "1. inspect\n2. execute",
            2,
        )
        .expect("complete plan");
        (store, thread.id)
    }

    #[test]
    fn approval_commit_returns_one_durable_runner_capability() {
        let (store, thread_id) = pending_plan();
        let request = PlanReviewRequested {
            thread_id: thread_id.clone(),
            plan_id: "plan".into(),
            action: PlanReviewAction::Approve,
        };
        let refresh = persist_review(&store, &request).expect("approval refresh");
        assert_eq!(refresh.thread.mode, ThreadMode::Execute);
        assert_eq!(refresh.plans[0].status, PlanStatus::Approved);
        let instruction_id = refresh
            .approved_instruction_id
            .expect("approval runner capability");
        let instruction = vega_store::messages::find(store.conn(), &instruction_id)
            .expect("instruction query")
            .expect("durable instruction");
        assert_eq!(instruction.thread_id, thread_id);
        assert_eq!(instruction.role, "user");
        assert_eq!(instruction.kind, "text");
        assert_eq!(instruction.status, "done");

        let replay = persist_review(&store, &request).expect("stale review reload");
        assert_eq!(replay.approved_instruction_id, None);
    }

    #[test]
    fn change_and_abandon_never_schedule_execute_turn() {
        for action in [
            PlanReviewAction::RequestChanges { note: None },
            PlanReviewAction::Abandon { note: None },
        ] {
            let (store, thread_id) = pending_plan();
            let request = PlanReviewRequested {
                thread_id,
                plan_id: "plan".into(),
                action,
            };
            let refresh = persist_review(&store, &request).expect("non-approval refresh");
            assert_eq!(refresh.approved_instruction_id, None);
            assert_eq!(refresh.thread.mode, ThreadMode::Plan);
        }
    }

    #[test]
    fn provider_model_resolution_is_exact_and_unique() {
        let provider = |name: &str, models: &[&str]| vega_store::config::ProviderConfig {
            name: name.into(),
            base_url: "https://provider.invalid/v1".into(),
            models: models.iter().map(|model| (*model).to_string()).collect(),
            key_ref: name.into(),
        };
        let mut config = vega_store::config::AppConfig {
            providers: vec![provider("one", &["model"]), provider("two", &["other"])],
            ..Default::default()
        };
        assert_eq!(
            unique_provider_for_model(&config, "model").map(|provider| provider.name),
            Some("one".into())
        );
        assert!(unique_provider_for_model(&config, "missing").is_none());
        config.providers.push(provider("duplicate", &["model"]));
        assert!(unique_provider_for_model(&config, "model").is_none());
    }

    #[test]
    fn bounded_agent_channel_preserves_burst_order_and_terminal() {
        let (sender, receiver) = mpsc::sync_channel(AGENT_EVENT_CAPACITY);
        let producer = std::thread::spawn(move || {
            for index in 0..(AGENT_EVENT_CAPACITY + AGENT_EVENT_BATCH + 17) {
                sender
                    .send(AgentUpdate::Event(
                        vega_conversation::types::ConversationEvent::TextDelta {
                            message_id: "message".into(),
                            delta: index.to_string(),
                        },
                    ))
                    .expect("bounded event send");
            }
            sender
                .send(AgentUpdate::Finished(true))
                .expect("terminal send");
        });
        let mut seen = Vec::new();
        let finished = loop {
            let batch = drain_agent_updates(&receiver);
            assert!(batch.events.len() <= AGENT_EVENT_BATCH);
            for event in batch.events {
                if let vega_conversation::types::ConversationEvent::TextDelta { delta, .. } = event
                {
                    seen.push(delta.parse::<usize>().expect("ordered index"));
                }
            }
            if let Some(finished) = batch.finished {
                break finished;
            }
            std::thread::yield_now();
        };
        producer.join().expect("bounded producer");
        assert!(finished);
        assert_eq!(seen, (0..seen.len()).collect::<Vec<_>>());
        assert_eq!(seen.len(), AGENT_EVENT_CAPACITY + AGENT_EVENT_BATCH + 17);
        assert!(AGENT_EVENT_POLL < Duration::from_millis(16));
    }

    #[test]
    fn same_batch_applies_events_before_terminal() {
        let (sender, receiver) = mpsc::sync_channel(4);
        sender
            .send(AgentUpdate::Event(
                vega_conversation::types::ConversationEvent::MessageStarted {
                    message_id: "durable".into(),
                    seq: 2,
                },
            ))
            .expect("event send");
        sender
            .send(AgentUpdate::Finished(false))
            .expect("terminal send");
        let batch = drain_agent_updates(&receiver);
        assert_eq!(batch.events.len(), 1);
        assert_eq!(batch.finished, Some(false));
        assert!(matches!(
            &batch.events[0],
            vega_conversation::types::ConversationEvent::MessageStarted { message_id, .. }
                if message_id == "durable"
        ));
    }

    #[test]
    fn finished_refresh_routes_only_to_matching_current_thread_cache() {
        assert!(current_cache_matches(Some("a"), Some("a"), "a"));
        assert!(
            !current_cache_matches(Some("b"), Some("a"), "a"),
            "A→B must not overwrite B's OpenedThread"
        );
        assert!(
            !current_cache_matches(Some("a"), Some("b"), "a"),
            "a stale cache cannot receive A's authoritative refresh"
        );
        assert!(
            current_cache_matches(Some("a"), Some("a"), "a"),
            "A→B→A must refresh the rebuilt A entity"
        );
    }

    #[gpui::test]
    async fn cancellation_keeps_active_until_durable_handshake_finishes(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(|cx| {
            cx.set_global(Theme::light());
            cx.set_global(SettingsOpen(false));
            vega_ui::init(cx);
        });
        let (store, thread_id) = pending_plan();
        let thread =
            vega_conversation::threads::open_thread(&store, &thread_id).expect("thread projection");
        let stream = cx.new(|cx| ConversationStream::new(thread, cx));
        let mut controller = AppAgentController::default();
        let (generation, cancel) = controller.begin(
            thread_id.clone(),
            stream.clone(),
            Some("draft".into()),
            None,
        );
        controller.request_active_cancel();
        assert!(cancel.is_cancelled());
        assert!(controller.active.is_some());
        assert_eq!(
            controller.accept_durable_start(generation + 1, &thread_id, &stream),
            None
        );
        assert_eq!(
            controller.accept_durable_start(generation, &thread_id, &stream),
            Some("draft".into())
        );
        assert_eq!(
            controller.accept_durable_start(generation, &thread_id, &stream),
            None
        );
        assert!(
            controller
                .finish(generation + 1, &thread_id, &stream)
                .is_none()
        );
        assert!(controller.active.is_some());
        let finished = controller
            .finish(generation, &thread_id, &stream)
            .expect("exact terminal owns active run");
        assert!(finished.pending_user_content.is_none());
        assert!(controller.active.is_none());

        let (next_generation, next_cancel) = controller.begin(
            thread_id.clone(),
            stream.clone(),
            Some("second".into()),
            None,
        );
        assert_eq!(
            controller.accept_durable_start(next_generation, &thread_id, &stream),
            Some("second".into())
        );
        controller.request_active_cancel();
        assert!(next_cancel.is_cancelled());
        assert!(
            controller
                .finish(next_generation, &thread_id, &stream)
                .is_some()
        );

        let (prestart_generation, prestart_cancel) = controller.begin(
            thread_id.clone(),
            stream.clone(),
            Some("retryable".into()),
            None,
        );
        controller.request_active_cancel();
        assert!(prestart_cancel.is_cancelled());
        let prestart = controller
            .finish(prestart_generation, &thread_id, &stream)
            .expect("cancelled pre-start worker still reaches terminal");
        assert_eq!(prestart.pending_user_content, Some("retryable".into()));

        let (approved_generation, _) = controller.begin(
            thread_id.clone(),
            stream.clone(),
            None,
            Some("approved-instruction".into()),
        );
        let approved = controller
            .finish(approved_generation, &thread_id, &stream)
            .expect("approved pre-start failure reaches terminal");
        assert_eq!(
            approved.pending_approved_instruction.as_deref(),
            Some("approved-instruction")
        );
    }

    #[gpui::test]
    async fn active_plan_review_is_deferred_and_cancels_exactly_once(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(|cx| {
            cx.set_global(Theme::light());
            cx.set_global(SettingsOpen(false));
            vega_ui::init(cx);
        });
        let (store, thread_id) = pending_plan();
        let thread =
            vega_conversation::threads::open_thread(&store, &thread_id).expect("thread projection");
        let rebuilt_thread = thread.clone();
        let stream = cx.new(|cx| ConversationStream::new(thread, cx));
        let rebuilt_stream = cx.new(|cx| ConversationStream::new(rebuilt_thread, cx));
        let mut controller = AppAgentController::default();
        let (_, cancel) = controller.begin(thread_id.clone(), stream.clone(), None, None);
        let request = PlanReviewRequested {
            thread_id: thread_id.clone(),
            plan_id: "plan".into(),
            action: PlanReviewAction::Approve,
        };
        assert!(controller.queue_review(&rebuilt_stream, &request));
        assert!(cancel.is_cancelled());
        assert!(controller.active.is_some());
        assert_eq!(
            vega_conversation::plans::list_plans(&store, &thread_id)
                .expect("plans before terminal")[0]
                .status,
            PlanStatus::Pending
        );
        assert_eq!(
            controller
                .pending_review
                .as_ref()
                .map(|pending| (pending.stream.clone(), pending.request.clone())),
            Some((rebuilt_stream, request.clone()))
        );
        assert!(!controller.queue_review(&stream, &request));
        controller.active = None;
        let pending = controller.pending_review.take().expect("deferred review");
        assert_eq!(pending.request, request);
        assert!(controller.pending_review.is_none());
        let refresh = persist_review(&store, &pending.request).expect("deferred review commit");
        assert!(refresh.approved_instruction_id.is_some());
        let replay = persist_review(&store, &pending.request).expect("stale replay");
        assert!(replay.approved_instruction_id.is_none());
    }

    #[test]
    fn completion_first_makes_deferred_old_review_stale() {
        let (store, thread_id) = pending_plan();
        insert(
            store.conn(),
            &MessageRow {
                id: "new-plan".into(),
                thread_id: thread_id.clone(),
                seq: 2,
                role: "assistant".into(),
                kind: "text".into(),
                content: String::new(),
                status: "streaming".into(),
                created_at: 3,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .expect("new streaming plan");
        complete_plan(store.conn(), &thread_id, "new-plan", "new", 4).expect("new completion wins");
        let request = PlanReviewRequested {
            thread_id,
            plan_id: "plan".into(),
            action: PlanReviewAction::Approve,
        };
        let refresh = persist_review(&store, &request).expect("stale deferred review");
        assert!(refresh.approved_instruction_id.is_none());
        assert_eq!(refresh.plans[0].status, PlanStatus::Abandoned);
        assert_eq!(refresh.plans[1].status, PlanStatus::Pending);
    }

    fn scrub_fixture_git_environment(command: &mut Command) {
        let explicit_git_keys: Vec<OsString> = command
            .get_envs()
            .filter(|(key, _)| key.as_bytes().starts_with(b"GIT_"))
            .map(|(key, _)| key.to_owned())
            .collect();
        for key in explicit_git_keys {
            command.env_remove(key);
        }
        for (key, _) in std::env::vars_os() {
            if key.as_os_str().as_bytes().starts_with(b"GIT_") {
                command.env_remove(key);
            }
        }
    }

    fn configure_fixture_git_environment(command: &mut Command) {
        scrub_fixture_git_environment(command);
        command
            .env("LC_ALL", "C")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_PAGER", "cat")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null");
    }

    fn fixture_git_command(root: &std::path::Path, args: &[&str]) -> Command {
        let mut command = Command::new("/usr/bin/git");
        command.arg("-C").arg(root).args(args);
        configure_fixture_git_environment(&mut command);
        command
    }

    fn run_fixture_git(root: &std::path::Path, args: &[&str]) {
        let status = fixture_git_command(root, args)
            .status()
            .expect("fixture git spawn");
        assert!(status.success(), "fixture git failed: {args:?}");
    }

    #[test]
    fn diff_controller_fixture_scrubs_hook_git_environment() {
        let sentinel = tempfile::tempdir().expect("fresh sentinel repo");
        run_fixture_git(
            sentinel.path(),
            &["init", "-q", "--initial-branch=sentinel"],
        );
        run_fixture_git(
            sentinel.path(),
            &["config", "--local", "user.name", "Vega Sentinel"],
        );
        run_fixture_git(
            sentinel.path(),
            &[
                "config",
                "--local",
                "user.email",
                "sentinel@example.invalid",
            ],
        );
        fs::write(sentinel.path().join("sentinel.txt"), "sentinel\n").expect("sentinel body");
        run_fixture_git(sentinel.path(), &["add", "--", "sentinel.txt"]);
        run_fixture_git(sentinel.path(), &["commit", "-q", "-m", "sentinel"]);

        let sentinel_ref = sentinel.path().join(".git/refs/heads/sentinel");
        let sentinel_index = sentinel.path().join(".git/index");
        let ref_before = fs::read(&sentinel_ref).expect("sentinel ref before");
        let index_before = fs::read(&sentinel_index).expect("sentinel index before");

        let fixture = tempfile::tempdir().expect("fresh isolated fixture repo");
        let run_poisoned = |args: &[&str]| {
            let mut command = Command::new("/usr/bin/git");
            command
                .arg("-C")
                .arg(fixture.path())
                .args(args)
                .env("GIT_DIR", sentinel.path().join(".git"))
                .env("GIT_WORK_TREE", sentinel.path())
                .env("GIT_INDEX_FILE", &sentinel_index);
            configure_fixture_git_environment(&mut command);
            let status = command.status().expect("poisoned fixture git spawn");
            assert!(status.success(), "poisoned fixture git failed: {args:?}");
        };

        run_poisoned(&["init", "-q", "--initial-branch=fixture"]);
        run_poisoned(&["config", "--local", "user.name", "Vega Fixture"]);
        run_poisoned(&["config", "--local", "user.email", "fixture@example.invalid"]);
        fs::write(fixture.path().join("fixture.txt"), "fixture\n").expect("fixture body");
        run_poisoned(&["add", "--", "fixture.txt"]);
        run_poisoned(&["commit", "-q", "-m", "fixture"]);

        assert!(fixture.path().join(".git").is_dir());
        assert!(fixture.path().join("fixture.txt").is_file());
        assert_eq!(
            fs::read(&sentinel_ref).expect("sentinel ref after"),
            ref_before
        );
        assert_eq!(
            fs::read(&sentinel_index).expect("sentinel index after"),
            index_before
        );
        assert_eq!(
            fs::read(sentinel.path().join("sentinel.txt")).expect("sentinel body after"),
            b"sentinel\n"
        );
        assert!(!sentinel.path().join("fixture.txt").exists());
    }

    fn diff_controller_repo() -> TempDir {
        let repo = tempfile::tempdir().expect("fresh diff controller repo");
        run_fixture_git(repo.path(), &["init", "-q"]);
        run_fixture_git(
            repo.path(),
            &["config", "--local", "user.name", "Vega Test"],
        );
        run_fixture_git(
            repo.path(),
            &["config", "--local", "user.email", "vega@example.invalid"],
        );
        fs::write(repo.path().join("tracked.rs"), "fn base() {}\n").expect("fixture base");
        run_fixture_git(repo.path(), &["add", "--", "tracked.rs"]);
        run_fixture_git(repo.path(), &["commit", "-q", "-m", "base"]);
        fs::write(
            repo.path().join("tracked.rs"),
            "fn base() {}\nfn changed() {}\n",
        )
        .expect("fixture change");
        repo
    }

    fn receive_refresh(
        service: Option<Arc<GitWorkspaceService>>,
        root: Option<PathBuf>,
    ) -> (Arc<GitWorkspaceService>, WorkspaceSnapshot) {
        let (sender, receiver) = mpsc::sync_channel(1);
        run_diff_refresh_worker(
            service,
            root,
            tokio_util::sync::CancellationToken::new(),
            sender,
        );
        match receiver.recv().expect("refresh worker result") {
            DiffRefreshWorkerResult::Ready { service, snapshot } => (service, snapshot),
            DiffRefreshWorkerResult::Failed(code) => panic!("refresh failed: {}", code.as_str()),
        }
    }

    fn artifact_controller_repo() -> TempDir {
        let repo = tempfile::tempdir().expect("fresh artifact controller repo");
        run_fixture_git(repo.path(), &["init", "-q"]);
        run_fixture_git(
            repo.path(),
            &["config", "--local", "user.name", "Vega Test"],
        );
        run_fixture_git(
            repo.path(),
            &["config", "--local", "user.email", "vega@example.invalid"],
        );
        fs::write(repo.path().join("base.txt"), "base\n").expect("artifact fixture base");
        run_fixture_git(repo.path(), &["add", "--", "base.txt"]);
        run_fixture_git(repo.path(), &["commit", "-q", "-m", "base"]);
        repo
    }

    fn artifact_write_call(call_id: &str, path: &str, bytes: u64) -> ToolCall {
        ToolCall {
            id: call_id.to_owned(),
            tool: "write".to_owned(),
            input_json: format!(
                r#"{{"audit_version":"write_edit_v1","tool":"write","path":"{path}","content_bytes":{bytes},"fingerprint_v1":"{}"}}"#,
                "a".repeat(64)
            ),
        }
    }

    fn artifact_write_result(
        project_id: &str,
        thread_id: &str,
        call_id: &str,
        path: &str,
        bytes: u64,
        reused: bool,
    ) -> ToolResult {
        ToolResult {
            status: vega_conversation::types::ToolCallStatus::Success,
            output: vega_tools::WriteSuccessOutput {
                path: path.to_owned(),
                bytes_written: bytes,
                checkpoint_ref: vega_tools::CheckpointIds::new(project_id, thread_id, call_id)
                    .expect("artifact checkpoint ids")
                    .checkpoint_ref(),
            }
            .to_json()
            .expect("artifact result JSON"),
            reused,
            exit_code: None,
            duration_ms: None,
            truncated: (!reused).then_some(false),
            invalid: None,
        }
    }

    fn receive_artifact_terminal(
        workspace: Arc<GitWorkspaceService>,
        service: Arc<ArtifactService>,
        job: ArtifactTerminalJob,
    ) -> Result<(u64, ArtifactTerminalResult), GitWorkspaceErrorCode> {
        let (sender, receiver) = mpsc::sync_channel(1);
        run_artifact_terminal_worker(
            workspace,
            service,
            job,
            tokio_util::sync::CancellationToken::new(),
            sender,
        );
        receiver.recv().expect("artifact terminal result")
    }

    fn artifact_capture_work(
        service: &ArtifactService,
        call: ToolCall,
        result: ToolResult,
    ) -> ArtifactTerminalWork {
        let call_id = call.id.clone();
        let candidate = service
            .prepare_capture(&call, &result)
            .expect("strict artifact terminal")
            .expect("eligible artifact terminal");
        ArtifactTerminalWork::Capture { call_id, candidate }
    }

    #[test]
    fn artifact_controller_terminal_refresh_captures_and_bash_reconciles_downgrade() {
        const PROJECT: &str = "artifact-project";
        const THREAD: &str = "artifact-thread";
        let repo = artifact_controller_repo();
        fs::write(repo.path().join("artifact.txt"), "agent\n").expect("agent artifact body");
        let workspace =
            Arc::new(GitWorkspaceService::new(repo.path()).expect("artifact controller workspace"));
        let service = Arc::new(
            ArtifactService::new(workspace.clone(), PROJECT.into(), THREAD.into(), 1)
                .expect("artifact controller service"),
        );
        let first = receive_artifact_terminal(
            workspace.clone(),
            service.clone(),
            ArtifactTerminalJob {
                sequence: 1,
                work: artifact_capture_work(
                    &service,
                    artifact_write_call("write-1", "artifact.txt", 6),
                    artifact_write_result(PROJECT, THREAD, "write-1", "artifact.txt", 6, false),
                ),
            },
        )
        .expect("strict terminal capture");
        let (_, captured) = first.1.captured.expect("eligible terminal card");
        assert_eq!(
            captured.source,
            vega_conversation::types::ArtifactSource::AgentArtifact
        );
        assert!(captured.preview_available);

        fs::write(repo.path().join("artifact.txt"), "human\n").expect("later workspace mutation");
        let reconciled = receive_artifact_terminal(
            workspace,
            service,
            ArtifactTerminalJob {
                sequence: 2,
                work: ArtifactTerminalWork::Refresh,
            },
        )
        .expect("bash terminal reconciliation");
        assert!(reconciled.1.captured.is_none());
        assert_eq!(reconciled.1.cards.len(), 1);
        assert_eq!(
            reconciled.1.cards[0].source,
            vega_conversation::types::ArtifactSource::WorkspaceChange
        );
    }

    #[gpui::test]
    async fn artifact_controller_real_batch_pairing_conflict_overflow_and_route_cancel(
        cx: &mut gpui::TestAppContext,
    ) {
        let repo = artifact_controller_repo();
        let store = Store::open(":memory:").expect("artifact window memory store");
        store.migrate().expect("artifact window migrations");
        let project = vega_store::projects::create(
            store.conn(),
            repo.path().to_str().expect("UTF-8 artifact root"),
            "artifact",
            None,
        )
        .expect("artifact project");
        let thread = vega_conversation::threads::create_thread(
            &store,
            &project.id,
            "mock",
            PermissionMode::Confirm.as_str(),
        )
        .expect("artifact thread");
        cx.update(|cx| install_diff_window_globals(store, thread.clone(), cx));
        let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
        let root = cx.new(VegaWindow::new);
        let route_cancel = root.update(cx, |root, _| {
            root.artifact_controller
                .begin(&thread, stream.clone(), repo.path().to_path_buf())
                .expect("artifact route");
            root.artifact_controller
                .active
                .as_mut()
                .expect("active artifact route")
                .agent_generation = Some(1);
            root.artifact_controller
                .active
                .as_ref()
                .expect("active artifact route")
                .cancel
                .clone()
        });
        let original = artifact_write_call("reused-id", "artifact.txt", 6);
        let conflicting = artifact_write_call("reused-id", "other.txt", 1);
        root.update(cx, |root, cx| {
            // This is the same ordering as the real AgentBatch loop: observe
            // before ownership moves into ConversationStream.
            root.observe_artifact_event(
                1,
                &stream,
                &ConversationEvent::ToolCallProposed {
                    call: original.clone(),
                },
                cx,
            );
            root.observe_artifact_event(
                1,
                &stream,
                &ConversationEvent::ToolCallProposed { call: conflicting },
                cx,
            );
            assert!(
                root.artifact_controller
                    .active
                    .as_ref()
                    .and_then(|active| active.proposals.get("reused-id"))
                    .is_some_and(|proposal| proposal.call.is_none()),
                "a reused id with different safe audit data is corrupt"
            );
            root.artifact_controller
                .active
                .as_mut()
                .expect("active artifact route")
                .terminal_sequence = u64::MAX;
            root.observe_artifact_event(
                1,
                &stream,
                &ConversationEvent::ToolCallFinished {
                    call_id: "reused-id".into(),
                    result: artifact_write_result(
                        &thread.project_id,
                        &thread.id,
                        "reused-id",
                        "artifact.txt",
                        6,
                        false,
                    ),
                },
                cx,
            );
            assert!(root.artifact_controller.active.is_none());
        });
        assert!(route_cancel.is_cancelled(), "checked overflow closes route");

        fs::write(repo.path().join("artifact.txt"), "agent\n")
            .expect("first conflicting artifact body");
        fs::write(repo.path().join("other.txt"), "x").expect("second conflicting artifact body");
        let first_call = artifact_write_call("fifo-conflict", "artifact.txt", 6);
        let second_call = artifact_write_call("fifo-conflict", "other.txt", 1);
        stream.update(cx, |stream, cx| {
            stream.apply_event(
                ConversationEvent::ToolCallProposed {
                    call: first_call.clone(),
                },
                cx,
            )
        });
        let (identity, workspace, service, first_job, second_job, conflict_cancel) =
            root.update(cx, |root, _| {
                let identity = root
                    .artifact_controller
                    .begin(&thread, stream.clone(), repo.path().to_path_buf())
                    .expect("replacement artifact route");
                let active = root
                    .artifact_controller
                    .active
                    .as_mut()
                    .expect("conflict artifact route");
                let first_job = ArtifactTerminalJob {
                    sequence: 1,
                    work: artifact_capture_work(
                        &active.service,
                        first_call.clone(),
                        artifact_write_result(
                            &thread.project_id,
                            &thread.id,
                            "fifo-conflict",
                            "artifact.txt",
                            6,
                            false,
                        ),
                    ),
                };
                let second_job = ArtifactTerminalJob {
                    sequence: 2,
                    work: artifact_capture_work(
                        &active.service,
                        second_call,
                        artifact_write_result(
                            &thread.project_id,
                            &thread.id,
                            "fifo-conflict",
                            "other.txt",
                            1,
                            false,
                        ),
                    ),
                };
                active.terminal_in_flight = Some(1);
                (
                    identity,
                    active.workspace.clone(),
                    active.service.clone(),
                    first_job,
                    second_job,
                    active.cancel.clone(),
                )
            });
        let first_result = receive_artifact_terminal(workspace, service, first_job)
            .expect("first FIFO candidate capture");
        let card = root.update(cx, |root, cx| {
            root.finish_artifact_terminal(&identity, Ok(first_result), cx);
            let active = root
                .artifact_controller
                .active
                .as_mut()
                .expect("route after first FIFO capture");
            active.terminal_queue.push_back(second_job);
            active.terminal_queue.push_back(ArtifactTerminalJob {
                sequence: 3,
                work: ArtifactTerminalWork::Refresh,
            });
            active
                .cards
                .values()
                .next()
                .cloned()
                .expect("first FIFO card inserted adjacent to tool")
        });
        let ArtifactTerminalDispatch {
            identity: next_identity,
            workspace,
            service,
            job,
            cancel,
        } = root
            .update(cx, |root, _| root.take_next_artifact_terminal())
            .expect("production FIFO takes conflict before refresh");
        assert_eq!(job.sequence, 2);
        let (sender, receiver) = mpsc::sync_channel(1);
        run_artifact_terminal_worker(workspace, service, job, cancel, sender);
        let conflict = receiver.recv().expect("FIFO conflict worker result");
        assert!(matches!(
            conflict,
            Err(GitWorkspaceErrorCode::ArtifactConflict)
        ));
        root.update(cx, |root, cx| {
            assert_eq!(
                root.artifact_controller
                    .active
                    .as_ref()
                    .expect("route before conflict completion")
                    .terminal_queue
                    .len(),
                1,
                "later FIFO work remains queued until conflict closes the route"
            );
            root.finish_artifact_terminal(&next_identity, conflict, cx);
        });
        assert!(conflict_cancel.is_cancelled());
        assert!(root.read_with(cx, |root, _| root.artifact_controller.active.is_none()));
        assert!(card.read_with(cx, |card, _| {
            card.projection().current_file_id.is_none()
                && !card.projection().preview_available
                && card.inline_error_code() == Some(GitWorkspaceErrorCode::ArtifactConflict)
        }));
        let cap_cancel = root.update(cx, |root, cx| {
            root.artifact_controller
                .begin(&thread, stream.clone(), repo.path().to_path_buf())
                .expect("proposal cap route");
            root.artifact_controller
                .active
                .as_mut()
                .expect("proposal cap active route")
                .agent_generation = Some(1);
            let id = "i".repeat(120);
            let exact = ToolCall {
                input_json: "x".repeat(64 * 1024 - id.len() - "write".len()),
                id,
                tool: "write".into(),
            };
            root.observe_artifact_event(
                1,
                &stream,
                &ConversationEvent::ToolCallProposed {
                    call: exact.clone(),
                },
                cx,
            );
            assert!(
                root.artifact_controller
                    .active
                    .as_ref()
                    .is_some_and(|active| active.proposals.contains_key(&exact.id))
            );
            let mut plus_one = exact;
            plus_one.input_json.push('x');
            let cancel = root
                .artifact_controller
                .active
                .as_ref()
                .expect("proposal cap route before plus one")
                .cancel
                .clone();
            root.observe_artifact_event(
                1,
                &stream,
                &ConversationEvent::ToolCallProposed { call: plus_one },
                cx,
            );
            cancel
        });
        assert!(cap_cancel.is_cancelled());
        assert!(root.read_with(cx, |root, _| root.artifact_controller.active.is_none()));
        root.update(cx, |root, _| {
            root.artifact_controller
                .begin(&thread, stream, repo.path().to_path_buf())
                .expect("settings artifact route");
        });
        cx.update(|cx| cx.set_global(SettingsOpen(true)));
        cx.run_until_parked();
        assert!(root.read_with(cx, |root, _| root.artifact_controller.active.is_none()));
    }

    #[gpui::test]
    async fn artifact_controller_agent_batch_generation_orphans_are_content_free_refreshes(
        cx: &mut gpui::TestAppContext,
    ) {
        let repo = artifact_controller_repo();
        let store = Store::open(":memory:").expect("artifact generation store");
        store.migrate().expect("artifact generation migrations");
        let project = vega_store::projects::create(
            store.conn(),
            repo.path().to_str().expect("UTF-8 artifact root"),
            "artifact",
            None,
        )
        .expect("artifact generation project");
        let thread = vega_conversation::threads::create_thread(
            &store,
            &project.id,
            "mock",
            PermissionMode::Confirm.as_str(),
        )
        .expect("artifact generation thread");
        cx.update(|cx| install_diff_window_globals(store, thread.clone(), cx));
        let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
        let root = cx.new(VegaWindow::new);
        root.update(cx, |root, _| {
            root.artifact_controller
                .begin(&thread, stream.clone(), repo.path().to_path_buf())
                .expect("artifact generation route");
        });
        let generation_a = root.update(cx, |root, _| {
            let (generation, _) =
                root.agent_controller
                    .begin(thread.id.clone(), stream.clone(), None, None);
            root.begin_artifact_agent_generation(generation, &stream);
            generation
        });
        let (sender, receiver) = mpsc::sync_channel(4);
        sender
            .send(AgentUpdate::Event(ConversationEvent::ToolCallProposed {
                call: artifact_write_call("same-id", "artifact.txt", 6),
            }))
            .expect("orphan proposal");
        sender
            .send(AgentUpdate::Finished(false))
            .expect("orphan terminal");
        let batch = drain_agent_updates(&receiver);
        assert!(root.update(cx, |root, cx| matches!(
            root.apply_agent_batch_ingress(generation_a, &thread.id, &stream, batch, cx),
            AgentBatchIngress::Finished { success: false, .. }
        )));

        let generation_b = root.update(cx, |root, _| {
            let (generation, _) =
                root.agent_controller
                    .begin(thread.id.clone(), stream.clone(), None, None);
            root.begin_artifact_agent_generation(generation, &stream);
            root.artifact_controller
                .active
                .as_mut()
                .expect("active generation route")
                .terminal_in_flight = Some(999);
            generation
        });
        assert!(root.update(cx, |root, cx| matches!(
            root.apply_agent_batch_ingress(
                generation_a,
                &thread.id,
                &stream,
                AgentBatch {
                    events: vec![ConversationEvent::ToolCallProposed {
                        call: artifact_write_call("stale-generation", "artifact.txt", 6),
                    }],
                    finished: None,
                },
                cx,
            ),
            AgentBatchIngress::Stale
        )));
        assert!(root.read_with(cx, |root, _| {
            root.artifact_controller
                .active
                .as_ref()
                .is_some_and(|active| active.proposals.is_empty())
        }));
        let huge_output = "sensitive unrelated output".repeat(100_000);
        let (sender, receiver) = mpsc::sync_channel(4);
        sender
            .send(AgentUpdate::Event(ConversationEvent::ToolCallFinished {
                call_id: "same-id".into(),
                result: ToolResult {
                    status: vega_conversation::types::ToolCallStatus::Success,
                    output: huge_output,
                    reused: false,
                    exit_code: None,
                    duration_ms: None,
                    truncated: Some(false),
                    invalid: None,
                },
            }))
            .expect("later same-id terminal");
        let batch = drain_agent_updates(&receiver);
        root.update(cx, |root, cx| {
            assert!(matches!(
                root.apply_agent_batch_ingress(generation_b, &thread.id, &stream, batch, cx),
                AgentBatchIngress::Running
            ));
            let active = root
                .artifact_controller
                .active
                .as_ref()
                .expect("active artifact generation");
            assert!(matches!(
                active.terminal_queue.back().map(|job| &job.work),
                Some(ArtifactTerminalWork::Refresh)
            ));
            assert!(active.service.cards().is_empty());
        });

        assert!(root.update(cx, |root, cx| matches!(
            root.apply_agent_batch_ingress(
                generation_b,
                &thread.id,
                &stream,
                AgentBatch {
                    events: Vec::new(),
                    finished: Some(false),
                },
                cx,
            ),
            AgentBatchIngress::Finished { success: false, .. }
        )));
        let generation_c = root.update(cx, |root, _| {
            let (generation, _) =
                root.agent_controller
                    .begin(thread.id.clone(), stream.clone(), None, None);
            root.begin_artifact_agent_generation(generation, &stream);
            generation
        });
        root.update(cx, |root, cx| {
            assert!(matches!(
                root.apply_agent_batch_ingress(
                    generation_c,
                    &thread.id,
                    &stream,
                    AgentBatch {
                        events: vec![ConversationEvent::ToolCallProposed {
                            call: artifact_write_call("cancelled-id", "artifact.txt", 6),
                        }],
                        finished: None,
                    },
                    cx,
                ),
                AgentBatchIngress::Running
            ));
            root.cancel_active_agent(cx);
            assert!(
                root.artifact_controller
                    .active
                    .as_ref()
                    .is_some_and(|active| active.proposals.is_empty())
            );
            assert!(matches!(
                root.apply_agent_batch_ingress(
                    generation_c,
                    &thread.id,
                    &stream,
                    AgentBatch {
                        events: Vec::new(),
                        finished: Some(false),
                    },
                    cx,
                ),
                AgentBatchIngress::Finished { success: false, .. }
            ));
        });
        let generation_d = root.update(cx, |root, _| {
            let (generation, _) =
                root.agent_controller
                    .begin(thread.id.clone(), stream.clone(), None, None);
            root.begin_artifact_agent_generation(generation, &stream);
            generation
        });
        root.update(cx, |root, cx| {
            assert!(matches!(
                root.apply_agent_batch_ingress(
                    generation_d,
                    &thread.id,
                    &stream,
                    AgentBatch {
                        events: vec![ConversationEvent::ToolCallFinished {
                            call_id: "cancelled-id".into(),
                            result: artifact_write_result(
                                &thread.project_id,
                                &thread.id,
                                "cancelled-id",
                                "artifact.txt",
                                6,
                                false,
                            ),
                        }],
                        finished: None,
                    },
                    cx,
                ),
                AgentBatchIngress::Running
            ));
            let active = root
                .artifact_controller
                .active
                .as_ref()
                .expect("active cancelled replacement generation");
            assert!(matches!(
                active.terminal_queue.back().map(|job| &job.work),
                Some(ArtifactTerminalWork::Refresh)
            ));
            assert!(active.service.cards().is_empty());
        });
    }

    #[gpui::test]
    async fn artifact_controller_preview_open_latest_stale_and_max_fences(
        cx: &mut gpui::TestAppContext,
    ) {
        let repo = artifact_controller_repo();
        let late_branch_repo = artifact_controller_repo();
        run_fixture_git(
            late_branch_repo.path(),
            &["branch", "late-branch-callback-target"],
        );
        fs::write(repo.path().join("artifact.txt"), "agent\n").expect("preview artifact body");
        let store = Store::open(":memory:").expect("artifact fence memory store");
        store.migrate().expect("artifact fence migrations");
        let project = vega_store::projects::create(
            store.conn(),
            repo.path().to_str().expect("UTF-8 artifact root"),
            "artifact",
            None,
        )
        .expect("artifact project");
        let thread = vega_conversation::threads::create_thread(
            &store,
            &project.id,
            "mock",
            PermissionMode::Confirm.as_str(),
        )
        .expect("artifact thread");
        let workspace =
            Arc::new(GitWorkspaceService::new(repo.path()).expect("artifact fence workspace"));
        let service = Arc::new(
            ArtifactService::new(
                workspace.clone(),
                thread.project_id.clone(),
                thread.id.clone(),
                1,
            )
            .expect("artifact fence service"),
        );
        let terminal = receive_artifact_terminal(
            workspace.clone(),
            service.clone(),
            ArtifactTerminalJob {
                sequence: 1,
                work: artifact_capture_work(
                    &service,
                    artifact_write_call("write-1", "artifact.txt", 6),
                    artifact_write_result(
                        &thread.project_id,
                        &thread.id,
                        "write-1",
                        "artifact.txt",
                        6,
                        false,
                    ),
                ),
            },
        )
        .expect("artifact fence capture");
        let (_, projection) = terminal.1.captured.expect("artifact fence card");
        let file_id = projection.current_file_id.expect("current artifact file");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("artifact preview runtime");
        let preview = runtime
            .block_on(service.preview(projection.id, tokio_util::sync::CancellationToken::new()))
            .expect("artifact preview");

        cx.update(|cx| install_diff_window_globals(store, thread.clone(), cx));
        let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
        let root = cx.new(VegaWindow::new);
        let branch_identity = root.update(cx, |root, cx| {
            root.stream_view = Some((thread.id.clone(), stream.clone()));
            root.ensure_branch_route(&thread, stream.clone(), cx);
            let active = root
                .branch_controller
                .active
                .as_ref()
                .expect("artifact test branch route");
            active.identity.clone()
        });
        let branch_service = Arc::new(
            BranchWorkspaceService::new(late_branch_repo.path())
                .expect("artifact test clean branch service"),
        );
        let (branch_sender, branch_receiver) = mpsc::sync_channel(1);
        run_branch_list_worker(
            branch_service,
            BranchListFence {
                route: branch_identity.clone(),
                sequence: 1,
            },
            tokio_util::sync::CancellationToken::new(),
            branch_sender,
        );
        let branch_snapshot = branch_receiver
            .recv()
            .expect("artifact test branch list")
            .1
            .expect("artifact test branch snapshot");
        let late_branch_id = branch_snapshot
            .branches
            .iter()
            .find(|branch| !branch.current)
            .expect("artifact test branch target")
            .id;
        let late_operation = branch_identity.selector.update(cx, |selector, cx| {
            assert!(selector.request_open(cx));
            assert!(selector.apply_snapshot(branch_snapshot.clone(), cx));
            selector
                .begin_switch(branch_snapshot.generation, late_branch_id, cx)
                .expect("artifact test late operation")
        });
        let late_branch_fence = BranchSwitchFence {
            route: branch_identity,
            sequence: 1,
            snapshot_generation: branch_snapshot.generation,
            branch_id: late_branch_id,
            operation_id: late_operation,
            lease: TrustedActionToken {
                generation: 1,
                kind: TrustedActionKind::BranchSwitch,
                owner_epoch: 1,
                request_sequence: 1,
            },
        };
        root.update(cx, |root, _| {
            root.branch_controller
                .active
                .as_mut()
                .expect("artifact test active branch route")
                .switch_fence = Some(late_branch_fence.clone());
            assert!(root.branch_controller.claim_terminal(&late_branch_fence));
        });
        let card = cx.new(|cx| {
            ArtifactCard::new(
                thread.id.clone(),
                thread.project_id.clone(),
                projection.clone(),
                cx,
            )
        });
        assert!(!stream.update(cx, |stream, cx| {
            stream.apply_artifact_card("missing-call", card.clone(), cx)
        }));
        stream.update(cx, |stream, cx| {
            stream.apply_event(
                ConversationEvent::MessageStarted {
                    message_id: "assistant-before-artifact".into(),
                    seq: 1,
                },
                cx,
            );
            stream.apply_event(
                ConversationEvent::ToolCallProposed {
                    call: artifact_write_call("write-1", "artifact.txt", 6),
                },
                cx,
            );
            assert!(stream.apply_artifact_card("write-1", card.clone(), cx));
            assert!(
                stream.apply_artifact_card("write-1", card.clone(), cx),
                "an identical duplicate is idempotent"
            );
            stream.apply_event(
                ConversationEvent::ToolCallProposed {
                    call: artifact_write_call("later-tool", "later.txt", 1),
                },
                cx,
            );
            assert!(stream.artifact_card_is_adjacent("write-1"));
        });
        let route = root.update(cx, |root, _| {
            let route = root
                .artifact_controller
                .begin(&thread, stream.clone(), repo.path().to_path_buf())
                .expect("artifact fence route");
            let active = root
                .artifact_controller
                .active
                .as_mut()
                .expect("active artifact fence route");
            active.workspace = workspace;
            active.service = service;
            active.cards.insert(projection.id, card.clone());
            route
        });
        let older_preview = ArtifactPreviewFence {
            route: route.clone(),
            sequence: 1,
            card_id: projection.id,
            file_id,
        };
        let latest_preview = ArtifactPreviewFence {
            sequence: 2,
            ..older_preview.clone()
        };
        let rows_before_preview = stream.read_with(cx, |stream, cx| stream.virtual_row_count(cx));
        let expected_preview_rows = preview.text().split_inclusive('\n').count();
        root.update(cx, |root, cx| {
            let active = root
                .artifact_controller
                .active
                .as_mut()
                .expect("active artifact preview fence");
            active.preview_sequence = 2;
            active.preview_fence = Some(latest_preview.clone());
            root.finish_branch_switch(
                late_branch_fence.clone(),
                BranchSwitchCompletion {
                    outcome: BranchSwitchOutcome::Failed(GitWorkspaceErrorCode::Cancelled),
                    snapshot: None,
                },
                cx,
            );
            assert!(
                root.artifact_controller
                    .active
                    .as_ref()
                    .is_some_and(|active| active.preview_fence.as_ref() == Some(&latest_preview)),
                "old duplicate branch terminal cannot clear fresh preview fence"
            );
            root.finish_artifact_preview(older_preview, Ok(preview.clone()), cx);
            assert_eq!(card.read(cx).row_count(), 2, "stale preview is dropped");
            root.finish_artifact_preview(latest_preview, Ok(preview), cx);
            assert!(card.read(cx).row_count() > 2, "latest preview is applied");
            assert_eq!(
                stream.read(cx).virtual_row_count(cx),
                rows_before_preview + expected_preview_rows
            );
        });

        let older_open = ArtifactOpenFence {
            route: route.clone(),
            sequence: 1,
            card_id: projection.id,
            file_id,
            target: OpenInTarget::VisualStudioCode,
            lease: TrustedActionToken {
                generation: 1,
                kind: TrustedActionKind::ArtifactOpen,
                owner_epoch: route.epoch,
                request_sequence: 1,
            },
        };
        let latest_open = ArtifactOpenFence {
            sequence: 2,
            ..older_open.clone()
        };
        card.update(cx, |card, cx| {
            card.set_opening(Some(OpenInTarget::VisualStudioCode), cx)
        });
        root.update(cx, |root, cx| {
            let active = root
                .artifact_controller
                .active
                .as_mut()
                .expect("active artifact open fence");
            active.open_sequence = 2;
            active.open_fence = Some(latest_open.clone());
            root.finish_branch_switch(
                late_branch_fence,
                BranchSwitchCompletion {
                    outcome: BranchSwitchOutcome::Failed(GitWorkspaceErrorCode::Cancelled),
                    snapshot: None,
                },
                cx,
            );
            assert!(
                root.artifact_controller
                    .active
                    .as_ref()
                    .is_some_and(|active| active.open_fence.as_ref() == Some(&latest_open)),
                "old duplicate branch terminal cannot clear fresh open fence"
            );
            root.finish_artifact_open(
                older_open,
                Ok(OpenInOutcome {
                    card_id: projection.id,
                    target: OpenInTarget::VisualStudioCode,
                }),
                cx,
            );
            assert!(
                root.artifact_controller
                    .active
                    .as_ref()
                    .is_some_and(|active| active.open_fence.as_ref() == Some(&latest_open)),
                "stale open completion cannot release the latest fence"
            );
            root.finish_artifact_open(
                latest_open,
                Ok(OpenInOutcome {
                    card_id: projection.id,
                    target: OpenInTarget::VisualStudioCode,
                }),
                cx,
            );
            assert!(
                root.artifact_controller
                    .active
                    .as_ref()
                    .is_some_and(|active| active.open_fence.is_none()),
                "latest open completion is accepted"
            );
        });

        let terminal_cancel = tokio_util::sync::CancellationToken::new();
        card.update(cx, |card, cx| {
            card.set_opening(Some(OpenInTarget::Terminal), cx)
        });
        root.update(cx, |root, cx| {
            let active = root
                .artifact_controller
                .active
                .as_mut()
                .expect("active terminal cancellation route");
            active.agent_generation = Some(7);
            active.terminal_in_flight = Some(999);
            active.open_cancel = Some(terminal_cancel.clone());
            active.open_fence = Some(ArtifactOpenFence {
                route: route.clone(),
                sequence: 3,
                card_id: projection.id,
                file_id,
                target: OpenInTarget::Terminal,
                lease: TrustedActionToken {
                    generation: 3,
                    kind: TrustedActionKind::ArtifactOpen,
                    owner_epoch: route.epoch,
                    request_sequence: 3,
                },
            });
            root.observe_artifact_event(
                7,
                &route.stream,
                &ConversationEvent::ToolCallFinished {
                    call_id: "bash-terminal".into(),
                    result: ToolResult {
                        status: vega_conversation::types::ToolCallStatus::Success,
                        output: "unrelated raw output".repeat(100_000),
                        reused: false,
                        exit_code: Some(0),
                        duration_ms: Some(1),
                        truncated: Some(false),
                        invalid: None,
                    },
                },
                cx,
            );
            let active = root
                .artifact_controller
                .active
                .as_ref()
                .expect("terminal cancellation keeps route");
            assert!(active.open_fence.is_none());
            assert!(matches!(
                active.terminal_queue.back().map(|job| &job.work),
                Some(ArtifactTerminalWork::Refresh)
            ));
        });
        assert!(terminal_cancel.is_cancelled());
        assert_eq!(card.read_with(cx, |card, _| card.row_count()), 3);

        let open_starts = ARTIFACT_OPEN_WORKER_STARTS.load(std::sync::atomic::Ordering::SeqCst);
        card.update(cx, |card, cx| {
            card.set_opening(Some(OpenInTarget::Cursor), cx)
        });
        let cancel = root.update(cx, |root, cx| {
            let active = root
                .artifact_controller
                .active
                .as_mut()
                .expect("active artifact max fence");
            active.open_sequence = u64::MAX;
            let cancel = active.cancel.clone();
            root.request_artifact_open(
                card.clone(),
                &ArtifactOpenRequested {
                    thread_id: route.thread_id.clone(),
                    project_id: route.project_id.clone(),
                    card_id: projection.id,
                    file_id,
                    target: OpenInTarget::Cursor,
                },
                cx,
            );
            cancel
        });
        assert!(cancel.is_cancelled());
        assert!(root.read_with(cx, |root, _| root.artifact_controller.active.is_none()));
        assert_eq!(
            ARTIFACT_OPEN_WORKER_STARTS.load(std::sync::atomic::Ordering::SeqCst),
            open_starts,
            "checked overflow cannot start an Open worker"
        );
        assert_eq!(card.read_with(cx, |card, _| card.row_count()), 3);
        assert!(card.read_with(cx, |card, _| card.projection().current_file_id.is_none()));

        let removed_card = cx.new(|cx| {
            ArtifactCard::new(
                thread.id.clone(),
                thread.project_id.clone(),
                projection.clone(),
                cx,
            )
        });
        let removed_cancel = root.update(cx, |root, _| {
            root.artifact_controller
                .begin(&thread, stream.clone(), repo.path().to_path_buf())
                .expect("selected-project route");
            let active = root
                .artifact_controller
                .active
                .as_mut()
                .expect("selected-project active route");
            active.cards.insert(projection.id, removed_card.clone());
            active.cancel.clone()
        });
        cx.update(|cx| {
            cx.set_global(vega_ui::sidebar::SelectedProject(None));
        });
        cx.run_until_parked();
        assert!(removed_cancel.is_cancelled());
        assert!(removed_card.read_with(cx, |card, _| card.projection().current_file_id.is_none()));
        removed_card.update(cx, |card, cx| card.set_opening(Some(OpenInTarget::Zed), cx));
        root.update(cx, |root, cx| {
            root.request_artifact_open(
                removed_card.clone(),
                &ArtifactOpenRequested {
                    thread_id: thread.id.clone(),
                    project_id: thread.project_id.clone(),
                    card_id: projection.id,
                    file_id,
                    target: OpenInTarget::Zed,
                },
                cx,
            );
        });
        assert_eq!(
            ARTIFACT_OPEN_WORKER_STARTS.load(std::sync::atomic::Ordering::SeqCst),
            open_starts,
            "removed project cannot start an Open worker"
        );
        assert_eq!(removed_card.read_with(cx, |card, _| card.row_count()), 3);

        let active_none_card = cx.new(|cx| {
            ArtifactCard::new(
                thread.id.clone(),
                thread.project_id.clone(),
                projection.clone(),
                cx,
            )
        });
        let preview_starts =
            ARTIFACT_PREVIEW_WORKER_STARTS.load(std::sync::atomic::Ordering::SeqCst);
        root.update(cx, |root, cx| {
            root.request_artifact_preview(
                active_none_card.clone(),
                &ArtifactPreviewRequested {
                    thread_id: thread.id.clone(),
                    project_id: thread.project_id.clone(),
                    card_id: projection.id,
                    file_id,
                },
                cx,
            );
            root.request_artifact_open(
                active_none_card.clone(),
                &ArtifactOpenRequested {
                    thread_id: thread.id.clone(),
                    project_id: thread.project_id.clone(),
                    card_id: projection.id,
                    file_id,
                    target: OpenInTarget::DefaultApplication,
                },
                cx,
            );
            root.request_artifact_open(
                active_none_card.clone(),
                &ArtifactOpenRequested {
                    thread_id: thread.id.clone(),
                    project_id: thread.project_id.clone(),
                    card_id: projection.id,
                    file_id,
                    target: OpenInTarget::DefaultApplication,
                },
                cx,
            );
        });
        assert!(active_none_card.read_with(cx, |card, _| {
            card.projection().current_file_id.is_none()
                && !card.projection().preview_available
                && card.inline_error_code() == Some(GitWorkspaceErrorCode::StaleGeneration)
        }));
        assert_eq!(
            ARTIFACT_PREVIEW_WORKER_STARTS.load(std::sync::atomic::Ordering::SeqCst),
            preview_starts
        );
        assert_eq!(
            ARTIFACT_OPEN_WORKER_STARTS.load(std::sync::atomic::Ordering::SeqCst),
            open_starts
        );

        cx.update(|cx| {
            cx.set_global(vega_ui::sidebar::SelectedProject(Some(
                thread.project_id.clone(),
            )));
        });
        let owned_card = cx.new(|cx| {
            ArtifactCard::new(
                thread.id.clone(),
                thread.project_id.clone(),
                projection.clone(),
                cx,
            )
        });
        let foreign_card = cx.new(|cx| {
            ArtifactCard::new(
                thread.id.clone(),
                thread.project_id.clone(),
                projection.clone(),
                cx,
            )
        });
        root.update(cx, |root, _| {
            root.artifact_controller
                .begin(&thread, stream, repo.path().to_path_buf())
                .expect("ownership mismatch route");
            root.artifact_controller
                .active
                .as_mut()
                .expect("ownership mismatch active route")
                .cards
                .insert(projection.id, owned_card.clone());
        });
        root.update(cx, |root, cx| {
            root.request_artifact_open(
                foreign_card.clone(),
                &ArtifactOpenRequested {
                    thread_id: thread.id.clone(),
                    project_id: thread.project_id.clone(),
                    card_id: projection.id,
                    file_id,
                    target: OpenInTarget::RevealInFinder,
                },
                cx,
            );
        });
        assert!(foreign_card.read_with(cx, |card, _| {
            card.projection().current_file_id.is_none()
                && !card.projection().preview_available
                && card.inline_error_code() == Some(GitWorkspaceErrorCode::StaleGeneration)
        }));

        let (_, other_snapshot) = receive_refresh(None, Some(repo.path().to_path_buf()));
        let mismatched_file_id = other_snapshot.files[0].id;
        assert_ne!(mismatched_file_id, file_id);
        root.update(cx, |root, cx| {
            root.request_artifact_preview(
                owned_card.clone(),
                &ArtifactPreviewRequested {
                    thread_id: thread.id.clone(),
                    project_id: thread.project_id.clone(),
                    card_id: projection.id,
                    file_id: mismatched_file_id,
                },
                cx,
            );
        });
        assert!(owned_card.read_with(cx, |card, _| {
            card.projection().current_file_id.is_none()
                && !card.projection().preview_available
                && card.inline_error_code() == Some(GitWorkspaceErrorCode::StaleGeneration)
        }));
        assert_eq!(
            ARTIFACT_PREVIEW_WORKER_STARTS.load(std::sync::atomic::Ordering::SeqCst),
            preview_starts
        );
        assert_eq!(
            ARTIFACT_OPEN_WORKER_STARTS.load(std::sync::atomic::Ordering::SeqCst),
            open_starts
        );
    }

    fn install_diff_window_globals(store: Store, thread: Thread, cx: &mut App) {
        cx.set_global(Theme::light());
        cx.set_global(SettingsOpen(false));
        cx.set_global(SidebarCollapsed(false));
        cx.set_global(vega_ui::sidebar::SelectedProject(Some(
            thread.project_id.clone(),
        )));
        cx.set_global(OpenedThread(Some(thread)));
        cx.set_global(PendingDeleteConfirm(None));
        cx.set_global(vega_ui::sidebar::ProjectsCollapsed(false));
        cx.set_global(vega_ui::sidebar::SessionsCollapsed(false));
        cx.set_global(VegaStore(Ok(store)));
        vega_ui::init(cx);
    }

    #[test]
    fn diff_controller_worker_preserves_unchanged_generation_and_rejects_stale_file() {
        let repo = diff_controller_repo();
        let (service, first) = receive_refresh(None, Some(repo.path().to_path_buf()));
        assert_eq!(first.files.len(), 1);
        let old_file = first.files[0].id;

        let (service, unchanged) = receive_refresh(Some(service), None);
        assert_eq!(unchanged.generation, first.generation);
        assert_eq!(unchanged.files[0].id, old_file);

        fs::write(
            repo.path().join("tracked.rs"),
            "fn base() {}\nfn changed_again() {}\n",
        )
        .expect("second fixture change");
        let (service, changed) = receive_refresh(Some(service), None);
        assert_ne!(changed.generation, unchanged.generation);

        let (sender, receiver) = mpsc::sync_channel(1);
        run_diff_projection_worker(
            service,
            old_file,
            tokio_util::sync::CancellationToken::new(),
            sender,
        );
        assert_eq!(
            receiver
                .recv()
                .expect("stale projection result")
                .expect_err("old file capability must fail"),
            GitWorkspaceErrorCode::StaleGeneration
        );
    }

    #[gpui::test]
    async fn diff_controller_real_finish_drops_superseded_result_and_global_switch_closes_route(
        cx: &mut gpui::TestAppContext,
    ) {
        let repo = diff_controller_repo();
        let (service, snapshot) = receive_refresh(None, Some(repo.path().to_path_buf()));
        let snapshot_generation = snapshot.generation;
        let file_id = snapshot.files[0].id;
        let (projection_sender, projection_receiver) = mpsc::sync_channel(1);
        run_diff_projection_worker(
            service.clone(),
            file_id,
            tokio_util::sync::CancellationToken::new(),
            projection_sender,
        );
        let projection = projection_receiver
            .recv()
            .expect("pending projection worker")
            .expect("pending projection");
        let store = Store::open(":memory:").expect("diff window memory store");
        store.migrate().expect("diff window migrations");
        let project = vega_store::projects::create(
            store.conn(),
            repo.path().to_str().expect("UTF-8 fixture root"),
            "diff",
            None,
        )
        .expect("diff window project");
        let thread = vega_conversation::threads::create_thread(
            &store,
            &project.id,
            "mock",
            PermissionMode::Confirm.as_str(),
        )
        .expect("diff window thread");
        let thread_id = thread.id.clone();
        let project_id = thread.project_id.clone();
        cx.update(|cx| install_diff_window_globals(store, thread, cx));
        let root = cx.new(VegaWindow::new);
        let view = cx.new(|cx| DiffView::new(thread_id.clone(), project_id.clone(), cx));
        let identity = root.update(cx, |root, _| {
            root.diff_controller
                .begin(thread_id, project_id, view.clone())
                .expect("diff route")
        });
        root.update(cx, |root, cx| {
            let active = root
                .diff_controller
                .active
                .as_mut()
                .expect("active diff route");
            assert_eq!(active.request_refresh(), DiffRefreshDecision::Start(1));
            assert_eq!(active.request_refresh(), DiffRefreshDecision::Coalesced);
            active.snapshot_generation = Some(snapshot_generation);
            let pending_fence = active
                .next_projection_fence(snapshot_generation, file_id)
                .expect("pending projection fence");
            active.pending_projection = Some(PendingDiffProjection {
                fence: pending_fence,
                result: Ok(projection),
            });
            root.finish_diff_refresh(
                &identity,
                1,
                DiffRefreshWorkerResult::Ready { service, snapshot },
                cx,
            );
            assert_eq!(view.read(cx).generation(), None, "R1 must not reach the UI");
            assert!(
                root.diff_controller
                    .active
                    .as_ref()
                    .is_some_and(|active| active.pending_projection.is_some()),
                "R1 must not release a projection while R2 is outstanding"
            );
            assert_eq!(
                root.diff_controller
                    .active
                    .as_ref()
                    .and_then(|active| active.refresh_in_flight),
                Some(2),
                "only the latest queued refresh may remain active"
            );
        });
        let window_root = root.clone();
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), move |_, _| window_root)
                .expect("diff controller focus window")
        });
        cx.run_until_parked();
        assert!(root.read_with(cx, |root, _| {
            root.diff_controller
                .active
                .as_ref()
                .is_some_and(|active| !active.focus_pending)
        }));
        let focused = window
            .update(cx, |_, window, cx| {
                view.read(cx).focus_handle(cx).is_focused(window)
            })
            .expect("diff controller focus window");
        assert!(focused, "the visible DiffView must receive one-shot focus");
        cx.update(|cx| cx.set_global(SettingsOpen(true)));
        cx.run_until_parked();
        assert!(root.read_with(cx, |root, _| root.diff_controller.active.is_none()));

        cx.update(|cx| cx.set_global(SettingsOpen(false)));
        let exhausted_view = cx.new(|cx| DiffView::new("thread".into(), "project".into(), cx));
        let exhausted_cancel = root.update(cx, |root, cx| {
            let identity = root
                .diff_controller
                .begin(
                    cx.global::<OpenedThread>()
                        .0
                        .as_ref()
                        .expect("current thread")
                        .id
                        .clone(),
                    cx.global::<OpenedThread>()
                        .0
                        .as_ref()
                        .expect("current thread")
                        .project_id
                        .clone(),
                    exhausted_view.clone(),
                )
                .expect("exhausted route");
            let active = root
                .diff_controller
                .active
                .as_mut()
                .expect("exhausted active route");
            active.file_request_seq = u64::MAX;
            let cancel = active.cancel.clone();
            root.request_diff_projection(
                exhausted_view,
                &DiffProjectionRequested {
                    thread_id: identity.thread_id,
                    project_id: identity.project_id,
                    generation: snapshot_generation,
                    file_id,
                },
                cx,
            );
            assert!(root.diff_controller.active.is_none());
            cancel
        });
        assert!(exhausted_cancel.is_cancelled());
    }

    #[gpui::test]
    async fn diff_controller_route_latest_poll_tool_and_cross_project_fences(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(|cx| {
            cx.set_global(Theme::light());
            cx.set_global(SettingsOpen(false));
            cx.set_global(OpenedThread(Some(Thread {
                id: "thread-b".into(),
                project_id: "project-b".into(),
                title: String::new(),
                mode: ThreadMode::Execute,
                permission_mode: PermissionMode::Confirm,
                model: String::new(),
                status: ThreadStatus::Active,
                pinned: false,
                unread: false,
                created_at: 0,
                updated_at: 0,
            })));
            vega_ui::init(cx);
        });
        let repo = diff_controller_repo();
        let (_, snapshot) = receive_refresh(None, Some(repo.path().to_path_buf()));
        let file_id = snapshot.files[0].id;
        let first_view = cx.new(|cx| DiffView::new("thread-a".into(), "project-a".into(), cx));
        let second_view = cx.new(|cx| DiffView::new("thread-b".into(), "project-b".into(), cx));
        let mut controller = DiffController::default();
        let first_route = controller
            .begin("thread-a".into(), "project-a".into(), first_view)
            .expect("first route");
        let first_cancel = controller
            .active
            .as_ref()
            .expect("first active")
            .cancel
            .clone();
        let second_route = controller
            .begin("thread-b".into(), "project-b".into(), second_view)
            .expect("second route");
        assert!(first_cancel.is_cancelled());
        assert!(!controller.matches(&first_route));
        assert!(controller.matches(&second_route));
        assert!(
            controller
                .active
                .as_ref()
                .is_some_and(|active| active.focus_pending)
        );
        cx.update(|cx| {
            assert!(VegaWindow::diff_route_is_current(&second_route, cx));
            cx.set_global(SettingsOpen(true));
            assert!(!VegaWindow::diff_route_is_current(&second_route, cx));
            cx.set_global(SettingsOpen(false));
            let mut other = cx
                .global::<OpenedThread>()
                .0
                .clone()
                .expect("opened thread fixture");
            other.id = "thread-c".into();
            cx.set_global(OpenedThread(Some(other)));
            assert!(!VegaWindow::diff_route_is_current(&second_route, cx));
        });

        let active = controller.active.as_mut().expect("second active");
        active.snapshot_generation = Some(snapshot.generation);
        assert_eq!(
            active.request_refresh(),
            DiffRefreshDecision::Start(1),
            "initial/poll refresh starts one worker"
        );
        assert_eq!(
            active.request_refresh(),
            DiffRefreshDecision::Coalesced,
            "tool terminal coalesces while the poll refresh is active"
        );
        assert_eq!(active.refresh_request_seq, 2);
        assert_eq!(active.refresh_in_flight, Some(1));
        assert_eq!(active.queued_refresh_seq, Some(2));
        assert_eq!(
            active.complete_refresh(1),
            Some(DiffRefreshCompletion::Superseded(Some(2))),
            "the pre-terminal poll result is dropped and only queues R2"
        );
        assert_eq!(active.refresh_in_flight, Some(2));
        assert_eq!(
            active.complete_refresh(2),
            Some(DiffRefreshCompletion::Latest)
        );

        let older = active
            .next_projection_fence(snapshot.generation, file_id)
            .expect("older file request");
        let latest = active
            .next_projection_fence(snapshot.generation, file_id)
            .expect("latest file request");
        assert_eq!(
            active.projection_disposition(&older),
            DiffProjectionDisposition::Drop
        );
        assert_eq!(
            active.projection_disposition(&latest),
            DiffProjectionDisposition::Apply
        );
        assert_eq!(active.request_refresh(), DiffRefreshDecision::Start(3));
        assert_eq!(
            active.projection_disposition(&latest),
            DiffProjectionDisposition::Defer,
            "a projection waits for an in-flight refresh"
        );
        active.refresh_in_flight = None;
        assert_eq!(
            active.projection_disposition(&latest),
            DiffProjectionDisposition::Apply,
            "unchanged generation survives a newer completed refresh"
        );
        let mut wrong_project = latest.clone();
        wrong_project.route.project_id = "project-a".into();
        assert_eq!(
            active.projection_disposition(&wrong_project),
            DiffProjectionDisposition::Drop
        );
        active.snapshot_generation = Some(snapshot.generation + 1);
        assert_eq!(
            active.projection_disposition(&latest),
            DiffProjectionDisposition::Drop
        );
        assert_eq!(DIFF_REFRESH_INTERVAL, Duration::from_millis(750));
    }
}
