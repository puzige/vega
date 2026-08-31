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
use super::*;
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
use crate::commit_controller::*;
#[allow(unused_imports)]
use crate::diff_controller::*;
#[allow(unused_imports)]
use crate::pricing_controller::*;
#[allow(unused_imports)]
use crate::thread_reload::*;
#[allow(unused_imports)]
use crate::trusted_action::*;

impl VegaWindow {
    pub(crate) fn workspace_tool_terminal(
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

    pub(crate) fn owns_stream_request(
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

    /// Scroll-up hydration (S8-T45/C7): one page read per request on a
    /// worker thread; the store global itself stays on the main thread. The
    /// route fence is checked before spawning so a request from a stale
    /// view never reaches the store, and again on completion so a page that
    /// finished after a route switch is dropped (A→B→A 晚到页丢弃).
    pub(crate) fn request_history_page(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &HistoryPageRequested,
        cx: &mut Context<Self>,
    ) {
        if !self.owns_stream_request(&stream, &request.thread_id, cx) {
            return;
        }
        let database_path = match &cx.global::<VegaStore>().0 {
            Ok(store) => match store.database_path() {
                Some(path) => path.to_path_buf(),
                None => return,
            },
            Err(_) => return,
        };
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker_request = request.clone();
        let worker = std::thread::Builder::new()
            .name("vega-history-page".into())
            .spawn(move || run_history_page_worker(database_path, worker_request, sender));
        if worker.is_err() {
            stream.update(cx, |stream, cx| stream.apply_history_load_failed(cx));
            return;
        }
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(DIFF_RESULT_POLL).await;
                let outcome = match receiver.try_recv() {
                    Ok((_, outcome)) => outcome,
                    Err(mpsc::TryRecvError::Empty) => continue,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        Err(HistoryPageFailure::Store("history page worker lost".into()))
                    }
                };
                let _ = this.update(cx, |this, cx| {
                    this.finish_history_page(stream.clone(), outcome, cx)
                });
                break;
            }
        })
        .detach();
    }

    /// Applies a finished hydration page to its requesting stream, gated by
    /// the same route fence as the request: only the currently open thread's
    /// cached stream may take a page.
    pub(crate) fn finish_history_page(
        &mut self,
        stream: Entity<ConversationStream>,
        outcome: HistoryPageOutcome,
        cx: &mut Context<Self>,
    ) {
        let Some(opened) = cx.global::<OpenedThread>().0.clone() else {
            return;
        };
        if !self.owns_stream_request(&stream, &opened.id, cx) {
            return;
        }
        stream.update(cx, |stream, cx| match outcome {
            Ok(page) => stream.apply_history_page(page, cx),
            Err(_) => stream.apply_history_load_failed(cx),
        });
    }

    pub(crate) fn apply_refresh(
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

    pub(crate) fn apply_stream_state(
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

    pub(crate) fn current_cached_stream_for_thread(
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

    pub(crate) fn cancel_active_agent(&mut self, cx: &mut Context<Self>) {
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

    pub(crate) fn start_agent_run(
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

    pub(crate) fn submit_composer(
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
}
