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
use crate::app_agent::*;
#[allow(unused_imports)]
use crate::artifact_controller::*;
#[allow(unused_imports)]
use crate::branch_controller::*;
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

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct CommitRouteIdentity {
    pub(crate) epoch: u64,
    pub(crate) thread_id: String,
    pub(crate) project_id: String,
    pub(crate) stream: Entity<ConversationStream>,
    pub(crate) panel: Entity<CommitPanel>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommitPhase {
    Checklist,
    Preparing,
    CommitReady,
    Drafting,
    Committing,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommitFenceAuthority {
    None,
    Snapshot(vega_conversation::types::IndexSnapshotId),
    Prepared(vega_conversation::types::PreparedCommitId),
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct CommitFence {
    pub(crate) route: CommitRouteIdentity,
    pub(crate) sequence: u64,
    pub(crate) operation: Option<CommitOperationId>,
    pub(crate) phase: CommitPhase,
    pub(crate) authority: CommitFenceAuthority,
}

pub(crate) struct ActiveCommitRoute {
    pub(crate) identity: CommitRouteIdentity,
    pub(crate) service: Arc<TrustedGitService>,
    pub(crate) lease: TrustedActionToken,
    pub(crate) next_sequence: u64,
    pub(crate) phase: CommitPhase,
    pub(crate) snapshot: Option<vega_conversation::types::IndexSnapshotId>,
    pub(crate) prepared: Option<vega_conversation::types::PreparedCommitId>,
    pub(crate) focus_pending: bool,
    pub(crate) pending: Option<CommitFence>,
    pub(crate) cancel: Option<tokio_util::sync::CancellationToken>,
    pub(crate) terminal_done: Option<Arc<AtomicBool>>,
}

#[derive(Default)]
pub(crate) struct CommitController {
    pub(crate) next_epoch: u64,
    pub(crate) active: Option<ActiveCommitRoute>,
    pub(crate) retiring: Option<ActiveCommitRoute>,
}

impl CommitController {
    pub(crate) fn is_open(&self) -> bool {
        self.active.is_some() || self.retiring.is_some()
    }

    pub(crate) fn begin_fence(
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

    pub(crate) fn retire_or_close(
        &mut self,
    ) -> Option<(TrustedActionToken, Entity<ConversationStream>)> {
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

    pub(crate) fn claim(&mut self, fence: &CommitFence) -> CommitClaim {
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

    pub(crate) fn authority_is_current(
        active: &ActiveCommitRoute,
        authority: CommitFenceAuthority,
    ) -> bool {
        match authority {
            CommitFenceAuthority::None => true,
            CommitFenceAuthority::Snapshot(id) => active.snapshot == Some(id),
            CommitFenceAuthority::Prepared(id) => active.prepared == Some(id),
        }
    }
}

pub(crate) enum CommitClaim {
    Active,
    Retiring(Box<ActiveCommitRoute>),
    Stale,
}

pub(crate) enum CommitWorkerResult {
    Checklist(Result<CommitChecklist, CommitErrorCode>),
    Prepare(CommitPrepareCompletion, CommitWorkspaceReconciliation),
    Draft(Result<vega_conversation::types::CommitDraft, CommitErrorCode>),
    Commit(CommitCompletion, CommitWorkspaceReconciliation),
    Recovered(CommitErrorCode, CommitWorkspaceReconciliation),
    RuntimeUnavailable(CommitErrorCode),
}

pub(crate) struct CommitWorkspaceReconciliation {
    pub(crate) workspace: Result<WorkspaceSnapshot, CommitErrorCode>,
    pub(crate) workspace_service: Arc<GitWorkspaceService>,
    pub(crate) branch: Option<CommitBranchReconciliation>,
    pub(crate) artifacts: Option<CommitArtifactReconciliation>,
}

#[cfg(test)]
#[derive(Default)]
pub(crate) struct CommitTestProbe {
    pub(crate) prepare_workers: std::sync::atomic::AtomicUsize,
    pub(crate) draft_workers: std::sync::atomic::AtomicUsize,
    pub(crate) commit_workers: std::sync::atomic::AtomicUsize,
    pub(crate) terminal_applications: std::sync::atomic::AtomicUsize,
    pub(crate) drop_commit_sender: AtomicBool,
    pub(crate) trace: Mutex<Vec<&'static str>>,
}

#[cfg(test)]
impl CommitTestProbe {
    pub(crate) fn record(&self, event: &'static str) {
        self.trace
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .push(event);
    }
}

pub(crate) type CommitBranchReconciliation = (
    Arc<BranchWorkspaceService>,
    Result<BranchSnapshot, GitWorkspaceErrorCode>,
);
pub(crate) type CommitArtifactReconciliation = (
    Arc<ArtifactService>,
    Result<Vec<ArtifactProjection>, GitWorkspaceErrorCode>,
);

pub(crate) fn reconcile_commit_consumers(
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

pub(crate) fn commit_result_has_authoritative_workspace(
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

pub(crate) fn commit_result_reconciliation(
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

pub(crate) fn map_commit_workspace_error(code: GitWorkspaceErrorCode) -> CommitErrorCode {
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

pub(crate) fn map_commit_reconcile_error(code: CommitErrorCode) -> GitWorkspaceErrorCode {
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

pub(crate) fn mark_commit_worker_terminal(
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

pub(crate) fn mark_commit_worker_terminal_if_authoritative(
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

pub(crate) fn build_commit_runtime() -> Result<tokio::runtime::Runtime, CommitErrorCode> {
    build_commit_runtime_with(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
    })
}

pub(crate) fn build_commit_runtime_with(
    factory: impl FnOnce() -> std::io::Result<tokio::runtime::Runtime>,
) -> Result<tokio::runtime::Runtime, CommitErrorCode> {
    factory().map_err(|_| CommitErrorCode::SpawnFailed)
}

pub(crate) fn next_commit_recovery_backoff(current: Duration) -> Duration {
    current.saturating_mul(2).min(Duration::from_secs(1))
}

pub(crate) fn build_commit_recovery_runtime_with(
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

pub(crate) fn run_commit_checklist_worker(
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

pub(crate) fn run_commit_prepare_worker(
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

pub(crate) fn run_commit_draft_worker(
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

pub(crate) fn run_commit_execute_worker(
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

pub(crate) fn run_commit_recovery_worker(
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
