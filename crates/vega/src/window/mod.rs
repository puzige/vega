use std::path::PathBuf;
use std::sync::atomic::*;
use std::sync::*;

use gpui_kit::prelude::*;
use gpui_kit::*;
use vega_conversation::types::*;
use vega_conversation::*;
use vega_theme::*;
use vega_ui::artifact_card::*;
use vega_ui::branch_selector::*;
use vega_ui::commit_panel::*;
use vega_ui::conversation_stream::*;
use vega_ui::diff_view::*;
use vega_ui::plan_card::PlanReviewRequested;
use vega_ui::settings::*;
use vega_ui::sidebar::*;

mod agent;
mod artifact;
mod branch;
mod commit;
mod commit_reconcile;
mod diff;
mod file_index;
pub(crate) mod navigation;
mod pricing;
mod reasoning;
mod render;
mod session;
mod workspace;

use self::file_index::*;
#[cfg(test)]
pub(crate) use self::file_index::{ActiveFileIndex, FileIndexOwner};
use crate::app_agent::*;
use crate::artifact_controller::*;
use crate::branch_controller::*;
use crate::commit_controller::*;
use crate::diff_controller::*;
use crate::pricing_controller::*;
use crate::thread_reload::*;
use crate::trusted_action::*;

/// The model-only portion of a completed in-session selection. Acknowledged
/// model saves must not carry a stale `Thread` snapshot into a later route or
/// Settings close, because sidebar edits may have changed the other fields in
/// the meantime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModelSelectionRefresh {
    pub(crate) thread_id: String,
    pub(crate) project_id: String,
    pub(crate) model: String,
}

impl ModelSelectionRefresh {
    pub(crate) fn from_thread(thread: &Thread) -> Self {
        Self {
            thread_id: thread.id.clone(),
            project_id: thread.project_id.clone(),
            model: thread.model.clone(),
        }
    }

    /// Merges the model field into the newest route projection. Every other
    /// field remains owned by that current projection.
    pub(crate) fn merge_into(&self, current: &Thread) -> Option<Thread> {
        if current.id != self.thread_id || current.project_id != self.project_id {
            return None;
        }
        let mut merged = current.clone();
        merged.model = self.model.clone();
        Some(merged)
    }
}

/// Root view of the main window: the A1 layout shell — a sidebar (260px,
/// collapsible) next to a content column (max 820px, centered) that hosts
/// either the settings view (Cmd+, / Esc), the opened session
/// ([`ConversationStream`], S3-T17), or the ui-spec §4.6 empty state.
pub(crate) struct VegaWindow {
    navigation: navigation::Navigation,
    pub(crate) palette: crate::app_palette::AppPalette,
    /// Sidebar with the [新建任务] button, projects block, and sessions block.
    pub(crate) sidebar: Entity<Sidebar>,
    workspace: workspace::Workspace,
    /// Explicit wide-rail choice. Width-driven hiding is derived separately
    /// from the live viewport so a resize never overwrites the user's choice.
    environment_collapsed: bool,
    /// Narrow windows show Environment as a temporary card over the shell.
    /// Route changes and opening a persistent right workspace close it.
    environment_overlay_open: bool,
    appearance_subscription: Option<Subscription>,
    /// Cached settings view entity. Kept while settings is open so re-renders
    /// (e.g. the theme toggle) never rebuild the form mid-typing; dropped when
    /// settings closes so the next open reloads the config from disk.
    pub(crate) settings_view: Option<Entity<SettingsView>>,
    pub(crate) pricing_controller: PricingController,
    /// Cached conversation stream for the open thread (id, view). S3-T17:
    /// built lazily on first render of an opened thread; rebuilt when another
    /// thread is opened. The stream itself is memory-only (no persistence).
    pub(crate) stream_view: Option<(String, Entity<ConversationStream>)>,
    pub(crate) agent_controller: AppAgentController,
    pub(crate) file_index_controller: FileIndexController,
    pub(crate) diff_controller: DiffController,
    pub(crate) artifact_controller: ArtifactController,
    pub(crate) branch_controller: BranchController,
    pub(crate) commit_controller: CommitController,
    pub(crate) trusted_actions: TrustedActionCoordinator,
    pub(crate) window_alive: Arc<AtomicBool>,
    /// Configured model ids projected from the worker-side config read. The
    /// selector intersects these with the in-memory Ready pricing authority.
    pub(crate) configured_models: Option<Vec<String>>,
    /// Exact provider/model reasoning projections loaded beside the model
    /// catalog. The stream receives a typed projection and performs no file
    /// IO of its own.
    pub(crate) configured_reasoning: Option<Vec<ReasoningProfileProjection>>,
    pub(crate) configured_reasoning_error: Option<ReasoningSettingsErrorCode>,
    pub(crate) configured_reasoning_authority: Option<(
        vega_store::reasoning::ReasoningConfig,
        vega_store::reasoning::ReasoningFileSnapshot,
    )>,
    /// App-level owner for the independent reasoning profile save. Settings
    /// can close while the worker is running, so this gate lives above the
    /// Settings entity and still blocks a new run until the exact ack lands.
    pub(crate) reasoning_save_pending: Option<(u64, u64)>,
    /// Provider Settings saves request a model catalog reload. If a reasoning
    /// save owns the catalog authority, defer that reload until its ack so the
    /// two workers cannot regress each other's projections.
    pub(crate) model_catalog_refresh_pending: bool,
    /// A failed reasoning save keeps its typed error visible through a
    /// coalesced provider catalog refresh. An explicit reload or a successful
    /// retry clears this hold.
    pub(crate) reasoning_error_hold: Option<ReasoningSettingsErrorCode>,
    pub(crate) model_catalog_loading: bool,
    /// Generation fence for worker-side config catalog reads. Closing
    /// Settings invalidates the prior result so an older worker cannot
    /// replace a freshly saved provider/model list.
    pub(crate) model_catalog_generation: u64,
    /// The model-only result of a save that completed while Settings was
    /// visible. It is merged into the newest route projection after close.
    pub(crate) deferred_model_refresh: Option<ModelSelectionRefresh>,
    /// E2E fixture seam (R1): owned temp config file for the model-selection
    /// worker's provider-uniqueness validation. Mirrors the other cfg(test)
    /// overrides; production always resolves the real config root.
    #[cfg(test)]
    pub(crate) model_selection_config_override: Option<std::path::PathBuf>,
    #[cfg(test)]
    pub(crate) commit_provider_override: Option<Arc<dyn vega_runtime::Provider>>,
    #[cfg(test)]
    pub(crate) agent_provider_override: Option<Arc<dyn vega_runtime::Provider>>,
    /// Per-window worker entry probe used by production-path tests. Keeping
    /// the counter with its owning controller prevents unrelated concurrent
    /// gpui tests from changing the zero/one spawn evidence.
    #[cfg(test)]
    pub(crate) agent_worker_start_probe: Arc<AgentWorkerStartProbe>,
    #[cfg(test)]
    pub(crate) commit_test_probe: Option<Arc<CommitTestProbe>>,
    #[cfg(test)]
    pub(crate) pricing_drop_next_worker_result: bool,
    #[cfg(test)]
    pub(crate) pricing_next_worker_gate: Option<Arc<std::sync::Barrier>>,
    #[cfg(test)]
    pub(crate) model_selection_worker_gate: Option<Arc<std::sync::Barrier>>,
    #[cfg(test)]
    pub(crate) reasoning_save_worker_gate: Option<Arc<std::sync::Barrier>>,
    #[cfg(test)]
    pub(crate) model_catalog_worker_gate: Option<Arc<std::sync::Barrier>>,
}

impl VegaWindow {
    pub(crate) fn record_commit_probe(&self, event: &'static str) {
        #[cfg(not(test))]
        let _ = event;
        #[cfg(test)]
        if let Some(probe) = &self.commit_test_probe {
            probe.record(event);
        }
    }

    pub(crate) fn record_commit_terminal_application(&self, trace: bool) {
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

    pub(crate) fn new(cx: &mut Context<Self>) -> Self {
        cx.observe_global::<OpenedThread>(|this, cx| {
            this.cancel_file_index_if_route_stale(cx);
            this.close_diff_if_route_stale(cx);
            this.close_artifact_if_route_stale(cx);
            this.close_branch_if_route_stale(cx);
            this.close_commit_if_route_stale(cx);
            let Some(thread) = cx.global::<OpenedThread>().0.clone() else {
                return;
            };
            let cached_stream = this
                .stream_view
                .as_ref()
                .filter(|(thread_id, _)| thread_id == &thread.id)
                .map(|(_, stream)| stream.clone());
            if let Some(stream) = cached_stream {
                // Sidebar rename owns only the title field. The stream method
                // fences id/project and leaves R1 model/pending state intact.
                stream.update(cx, |stream, cx| stream.apply_thread_title(&thread, cx));
            }
        })
        .detach();
        cx.observe_global::<SettingsOpen>(|this, cx| {
            this.cancel_file_index_if_route_stale(cx);
            this.close_diff_if_route_stale(cx);
            this.close_artifact_if_route_stale(cx);
            this.close_branch_if_route_stale(cx);
            this.close_commit_if_route_stale(cx);
            if cx.global::<SettingsOpen>().0 {
                return;
            }
            this.invalidate_model_catalog();
            this.apply_deferred_model_refresh(cx);
        })
        .detach();
        cx.observe_global::<vega_ui::sidebar::SelectedProject>(|this, cx| {
            this.cancel_file_index_if_route_stale(cx);
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
            navigation: navigation::Navigation::new(cx),
            palette: crate::app_palette::AppPalette::default(),
            sidebar: cx.new(Sidebar::new),
            settings_view: None,
            pricing_controller: PricingController::new(pricing_service),
            stream_view: None,
            agent_controller: AppAgentController::default(),
            file_index_controller: FileIndexController::default(),
            diff_controller: DiffController::default(),
            workspace: workspace::Workspace::default(),
            environment_collapsed: false,
            environment_overlay_open: false,
            appearance_subscription: None,
            artifact_controller: ArtifactController::default(),
            branch_controller: BranchController::default(),
            commit_controller: CommitController::default(),
            trusted_actions: TrustedActionCoordinator::default(),
            window_alive: Arc::new(AtomicBool::new(true)),
            configured_models: None,
            configured_reasoning: None,
            configured_reasoning_error: None,
            configured_reasoning_authority: None,
            reasoning_save_pending: None,
            model_catalog_refresh_pending: false,
            reasoning_error_hold: None,
            model_catalog_loading: false,
            model_catalog_generation: 0,
            deferred_model_refresh: None,
            #[cfg(test)]
            model_selection_config_override: None,
            #[cfg(test)]
            commit_provider_override: None,
            #[cfg(test)]
            agent_provider_override: None,
            #[cfg(test)]
            agent_worker_start_probe: Arc::new(AgentWorkerStartProbe::default()),
            #[cfg(test)]
            commit_test_probe: None,
            #[cfg(test)]
            pricing_drop_next_worker_result: false,
            #[cfg(test)]
            pricing_next_worker_gate: None,
            #[cfg(test)]
            model_selection_worker_gate: None,
            #[cfg(test)]
            reasoning_save_worker_gate: None,
            #[cfg(test)]
            model_catalog_worker_gate: None,
        };
        window.start_pricing_load(cx);
        window
    }

    pub(crate) fn window_terminal_cleanup(&mut self) {
        self.window_alive.store(false, Ordering::SeqCst);
        if let Some(active) = self.agent_controller.active.take() {
            active.cancel.cancel();
        }
        self.file_index_controller.cancel();
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
}

impl Drop for VegaWindow {
    fn drop(&mut self) {
        self.window_terminal_cleanup();
    }
}
