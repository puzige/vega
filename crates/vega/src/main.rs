//! Vega application entry point: boots the GPUI app and opens the main window.
//! The hidden `--vega-bench-render <out.json>` flag instead runs the S3-T17
//! render_frame self-measurement probe (see
//! [`vega_ui::conversation_stream::bench`]).

use std::path::PathBuf;
use std::sync::{Arc, mpsc};
use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Bounds, Entity, Focusable, KeyBinding, TitlebarOptions, Window, WindowBounds,
    WindowOptions, actions, div, px, size,
};
use gpui_platform::application;
use vega_conversation::GitWorkspaceService;
use vega_conversation::types::{
    DiffTextProjection, GitWorkspaceErrorCode, Plan, PlanReviewOutcome, Thread, WorkspaceFileId,
    WorkspaceSnapshot,
};
use vega_store::Store;
use vega_theme::{Theme, ThemeColors, Typography, theme};
use vega_ui::conversation_stream::{
    ComposerSubmitted, ConversationStream, OpenWorkspaceDiffRequested, ThreadSettingsRequested,
    WorkspaceToolTerminal, bench as render_frame_bench,
};
use vega_ui::diff_view::{
    DIFF_REFRESH_INTERVAL, DiffClosed, DiffProjectionRequested, DiffRetryRequested, DiffView,
};
use vega_ui::plan_card::PlanReviewRequested;
use vega_ui::settings::{CloseSettings, OpenSettings, SettingsOpen, SettingsView};
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
}

struct PendingPlanReview {
    stream: Entity<ConversationStream>,
    request: PlanReviewRequested,
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

#[allow(clippy::too_many_arguments)]
fn run_agent_worker(
    database_path: std::path::PathBuf,
    project_path: std::path::PathBuf,
    thread: Thread,
    run: PendingAgentRun,
    permission_queue: vega_conversation::agent::PermissionQueue,
    cancel: tokio_util::sync::CancellationToken,
    sender: mpsc::SyncSender<AgentUpdate>,
) {
    let success = (|| -> Result<(), ()> {
        // Config and Keychain are touched only after an explicit user submit
        // or committed Plan approval reaches this worker.
        let tools = vega_tools::Tools::new(project_path).map_err(|_| ())?;
        let store = Store::open(database_path).map_err(|_| ())?;
        store.migrate().map_err(|_| ())?;
        let provider: Box<dyn vega_runtime::Provider> = vega_store::config::load()
            .ok()
            .and_then(|config| unique_provider_for_model(&config, &thread.model))
            .and_then(|provider| {
                vega_store::keystore::get_key(&provider.key_ref)
                    .ok()
                    .filter(|key| !key.is_empty())
                    .and_then(|key| vega_runtime::OpenAiProvider::new(provider.base_url, key).ok())
            })
            .map_or_else(
                || Box::new(UnavailableProvider) as Box<dyn vega_runtime::Provider>,
                |provider| Box::new(provider) as Box<dyn vega_runtime::Provider>,
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
            PendingAgentRun::UserMessage(content) => runtime.block_on(
                vega_conversation::agent::run_thread_task_with_permission_sink(
                    &store,
                    provider.as_ref(),
                    &tools,
                    &thread.id,
                    &content,
                    SYSTEM_PROMPT,
                    cancel,
                    &permission_queue,
                    event_sink,
                ),
            ),
            PendingAgentRun::ApprovedPlan(instruction_message_id) => runtime.block_on(
                vega_conversation::agent::run_approved_plan_task_with_permission_sink(
                    &store,
                    provider.as_ref(),
                    &tools,
                    &thread.id,
                    &instruction_message_id,
                    SYSTEM_PROMPT,
                    cancel,
                    &permission_queue,
                    event_sink,
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
    /// Cached conversation stream for the open thread (id, view). S3-T17:
    /// built lazily on first render of an opened thread; rebuilt when another
    /// thread is opened. The stream itself is memory-only (no persistence).
    stream_view: Option<(String, Entity<ConversationStream>)>,
    agent_controller: AppAgentController,
    diff_controller: DiffController,
}

impl VegaWindow {
    fn new(cx: &mut Context<Self>) -> Self {
        cx.observe_global::<OpenedThread>(|this, cx| {
            this.close_diff_if_route_stale(cx);
        })
        .detach();
        cx.observe_global::<SettingsOpen>(|this, cx| {
            this.close_diff_if_route_stale(cx);
        })
        .detach();
        Self {
            sidebar: cx.new(Sidebar::new),
            settings_view: None,
            stream_view: None,
            agent_controller: AppAgentController::default(),
            diff_controller: DiffController::default(),
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
        if let Some(active) = &self.agent_controller.active {
            active.cancel.cancel();
            active
                .stream
                .update(cx, |stream, cx| stream.timeout_permission(cx));
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
        if self.agent_controller.active.is_some() {
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

        let permission_queue = stream.read(cx).permission_queue();
        let (generation, cancel) = self.agent_controller.begin(
            thread_id.to_string(),
            stream.clone(),
            pending_user_content,
            pending_approved_instruction,
        );
        let (sender, receiver) = mpsc::sync_channel(AGENT_EVENT_CAPACITY);
        let worker_sender = sender.clone();
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
                );
            });
        if worker.is_err() {
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
                let AgentBatch { events, finished } = drain_agent_updates(&receiver);
                let keep_running = this
                    .update(cx, |this, cx| {
                        if !this
                            .agent_controller
                            .matches(generation, &thread_id, &stream)
                        {
                            return false;
                        }
                        for event in events {
                            if matches!(
                                event,
                                vega_conversation::types::ConversationEvent::MessageStarted { .. }
                            ) && let Some(content) = this
                                .agent_controller
                                .accept_durable_start(generation, &thread_id, &stream)
                            {
                                stream.update(cx, |stream, cx| {
                                    stream.accept_composer_submission(&content, cx)
                                });
                            }
                            stream.update(cx, |stream, cx| stream.apply_event(event, cx));
                        }
                        let Some(success) = finished else {
                            return true;
                        };
                        let Some(finished_run) = this
                            .agent_controller
                            .finish(generation, &thread_id, &stream)
                        else {
                            return false;
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
        if self.agent_controller.active.is_some() {
            if self.agent_controller.queue_review(&stream, request) {
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
        if let Some(active) = self.agent_controller.active.take() {
            active.cancel.cancel();
        }
        self.diff_controller.close();
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
        let content: AnyElement = if settings_open {
            self.cancel_active_agent(cx);
            // 设置视图：缓存 Entity，避免主题刷新等重渲染时重建导致表单输入丢失。
            let settings = self
                .settings_view
                .get_or_insert_with(|| cx.new(SettingsView::new));
            settings.clone().into_any_element()
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
                                this.workspace_tool_terminal(stream.clone(), request, cx);
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
                                    Ok((plans, history, recovery))
                                })(),
                                Err(error) => {
                                    Err(vega_conversation::types::ConversationError::Store(
                                        error.clone(),
                                    ))
                                }
                            };
                            view.update(cx, |stream, cx| match initial {
                                Ok((plans, history, recovery)) => {
                                    for plan in plans {
                                        stream.apply_plan(plan, cx);
                                    }
                                    stream.apply_composer_history(&thread.id, history, cx);
                                    if recovery.is_some() {
                                        stream.apply_approved_not_started(cx);
                                    }
                                }
                                Err(_) => stream.apply_controller_error(cx),
                            });
                            self.stream_view = Some((thread.id.clone(), view.clone()));
                            view
                        }
                    };
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
    use std::fs;
    use std::process::Command;
    use tempfile::TempDir;
    use vega_conversation::types::{
        PermissionMode, PlanReviewAction, PlanStatus, ThreadMode, ThreadStatus,
    };
    use vega_store::messages::{MessageRow, complete_plan, insert};

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

    fn run_fixture_git(root: &std::path::Path, args: &[&str]) {
        let status = Command::new("/usr/bin/git")
            .arg("-C")
            .arg(root)
            .args(args)
            .env("LC_ALL", "C")
            .env("GIT_TERMINAL_PROMPT", "0")
            .status()
            .expect("fixture git spawn");
        assert!(status.success(), "fixture git failed: {args:?}");
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
