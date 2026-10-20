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
mod automation;
mod automation_editor;
mod branch;
mod commit;
mod commit_reconcile;
mod composer_predictions;
mod context;
mod diff;
mod file_index;
mod image_preview;
mod model_capabilities;
mod model_catalog;
mod model_context;
mod native_subagents;
pub(crate) mod navigation;
mod notifications;
mod pending_messages;
mod pricing;
mod privacy;
mod reasoning;
mod render;
mod scroll_state;
mod session;
mod skills;
mod subagent_panel;
mod updater;
mod workspace;

use self::file_index::*;
use crate::app_agent::*;
use crate::artifact_controller::*;
use crate::branch_controller::*;
use crate::commit_controller::*;
use crate::diff_controller::*;
use crate::pricing_controller::*;
use crate::thread_reload::*;
use crate::trusted_action::*;

#[derive(Clone)]
pub(crate) struct DeferredMessageLocation {
    pub(crate) stream: Entity<ConversationStream>,
    pub(crate) request: MessageLocationWorkerRequest,
}

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

pub(crate) struct VegaWindow {
    #[cfg(test)]
    composer_config_path_override: Option<PathBuf>,
    pub(crate) composer_prediction_config_generation: u64,
    pub(crate) composer_prediction: Option<composer_predictions::ComposerPredictionOwner>,
    pub(crate) updater: crate::updater::Updater,
    update_notice_dismissed: bool,
    navigation: navigation::Navigation,
    completion_attention: notifications::CompletionAttention,
    pub(crate) palette: crate::app_palette::AppPalette,
    /// Sidebar with the [新建任务] button, projects block, and sessions block.
    pub(crate) sidebar: Entity<Sidebar>,
    workspace: workspace::Workspace,
    subagent_panel: subagent_panel::SubagentPanel,
    pub(crate) automation_editors: std::collections::HashMap<
        vega_ui::automation_editor::AutomationEditorKey,
        Entity<vega_ui::automation_editor::AutomationEditor>,
    >,
    image_preview: image_preview::ImagePreviewController,
    /// Explicit wide-rail choice. Width-driven hiding is derived separately
    /// from the live viewport so a resize never overwrites the user's choice.
    environment_collapsed: bool,
    /// Narrow windows show Environment as a temporary card over the shell.
    /// Route changes and opening a persistent right workspace close it.
    environment_overlay_open: bool,
    sidebar_resize_dragging: bool,
    appearance_subscription: Option<Subscription>,
    /// Cached settings view entity. Kept while settings is open so re-renders
    /// (e.g. the theme toggle) never rebuild the form mid-typing; dropped when
    /// settings closes so the next open reloads the config from disk.
    pub(crate) settings_view: Option<Entity<SettingsView>>,
    /// Clone of the one app-owned MCP revocation domain. Settings and run
    /// workers must never instantiate independent registries for this DB.
    pub(crate) mcp_settings: Option<vega_conversation::McpServerSettingsService>,
    pub(crate) pricing_controller: PricingController,
    pub(crate) stream_view: Option<(String, Entity<ConversationStream>)>,
    thread_scroll_states: scroll_state::ThreadScrollStateLru,
    pub(crate) message_location_route_thread_id: Option<String>,
    pub(crate) message_location_route_generation: u64,
    pub(crate) message_location_request_generation: u64,
    pub(crate) deferred_message_location: Option<DeferredMessageLocation>,
    pub(crate) draft: Option<Thread>,
    pub(crate) agent_controller: AppAgentController,
    pub(crate) automation_controller: crate::automation_controller::AutomationController,
    pub(crate) context_controller: context::ContextController,
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
    model_capability_authority: Option<ModelCapabilityAuthority>,
    model_capability_projection: ModelCapabilitySettingsProjection,
    model_capability_generation: u64,
    model_capability_pending: Option<(u64, u64)>,
    /// Exact provider/model reasoning projections loaded beside the model
    /// catalog. The stream receives a typed projection and performs no file
    /// IO of its own.
    pub(crate) configured_reasoning: Option<Vec<ReasoningProfileProjection>>,
    pub(crate) configured_reasoning_error: Option<ReasoningSettingsErrorCode>,
    pub(crate) configured_reasoning_authority: Option<(
        vega_store::reasoning::ReasoningConfig,
        vega_store::reasoning::ReasoningFileSnapshot,
    )>,
    loaded_replay_config: Option<vega_store::config::AppConfig>,
    /// App-level owner for the independent reasoning profile save. Settings
    /// can close while the worker is running, so this gate lives above the
    /// Settings entity and still blocks a new run until the exact ack lands.
    pub(crate) reasoning_save_pending: Option<(u64, u64)>,
    pub(crate) composer_reasoning_operation: u64,
    pub(crate) pending_composer_thinking: Option<(String, String)>,
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
}

impl VegaWindow {
    pub(crate) fn new(cx: &mut Context<Self>) -> Self {
        Self::new_inner(
            #[cfg(test)]
            None,
            cx,
        )
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(config_path: PathBuf, cx: &mut Context<Self>) -> Self {
        Self::new_inner(Some(config_path), cx)
    }

    fn new_inner(#[cfg(test)] config_path: Option<PathBuf>, cx: &mut Context<Self>) -> Self {
        cx.on_release(|this, cx| {
            if cx
                .try_global::<crate::automation_controller::AutomationGlobal>()
                .is_some()
            {
                cx.update_global::<crate::automation_controller::AutomationGlobal, _>(
                    |global, _| global.close_window(&this.window_alive),
                );
            }
            let thread_ids: Vec<String> = this.agent_controller.active.keys().cloned().collect();
            for thread_id in thread_ids {
                vega_ui::sidebar::set_thread_running(&thread_id, false, cx);
            }
        })
        .detach();
        cx.observe_global::<OpenedThread>(|this, cx| {
            let route_thread_id = cx
                .global::<OpenedThread>()
                .0
                .as_ref()
                .map(|thread| thread.id.clone());
            if this
                .composer_prediction
                .as_ref()
                .is_some_and(|owner| !this.composer_prediction_route_matches(owner, cx))
            {
                this.cancel_composer_prediction(cx);
            }
            this.update_message_location_route(route_thread_id.as_deref());
            this.cancel_context_if_route_stale(cx);
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
                stream.update(cx, |stream, cx| stream.apply_thread_title(&thread, cx));
            }
            if let Some(stream) = this.automation_controller.streams.get(&thread.id) {
                stream.update(cx, |stream, cx| {
                    stream.apply_automation_thread_status(&thread, cx)
                });
            }
        })
        .detach();
        cx.observe_global::<SettingsOpen>(|this, cx| {
            this.cancel_composer_prediction(cx);
            this.cancel_context_if_route_stale(cx);
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
        let sidebar = cx.new(Sidebar::new);
        cx.subscribe(&sidebar, |this, _, event: &SidebarEvent, cx| match event {
            SidebarEvent::ThreadDeleted(thread_id) => {
                this.cancel_composer_prediction_for_thread(thread_id, cx);
                this.cancel_agent_for_thread(thread_id, cx);
            }
        })
        .detach();
        let mut window = Self {
            #[cfg(test)]
            composer_config_path_override: config_path,
            composer_prediction_config_generation: 0,
            composer_prediction: None,
            updater: crate::updater::Updater::default(),
            update_notice_dismissed: false,
            navigation: navigation::Navigation::new(cx, None),
            completion_attention: notifications::CompletionAttention::default(),
            palette: crate::app_palette::AppPalette::default(),
            sidebar,
            settings_view: None,
            mcp_settings: cx
                .try_global::<crate::app_agent::AppMcpSettings>()
                .and_then(|settings| settings.0.clone()),
            pricing_controller: PricingController::new(pricing_service),
            stream_view: None,
            thread_scroll_states: scroll_state::ThreadScrollStateLru::default(),
            message_location_route_thread_id: None,
            message_location_route_generation: 0,
            message_location_request_generation: 0,
            deferred_message_location: None,
            draft: None,
            agent_controller: AppAgentController::default(),
            automation_controller: crate::automation_controller::AutomationController::default(),
            context_controller: context::ContextController::default(),
            file_index_controller: FileIndexController::default(),
            diff_controller: DiffController::default(),
            workspace: workspace::Workspace::default(),
            subagent_panel: subagent_panel::SubagentPanel::new(cx),
            automation_editors: Default::default(),
            image_preview: image_preview::ImagePreviewController::new(cx),
            environment_collapsed: true,
            environment_overlay_open: false,
            sidebar_resize_dragging: false,
            appearance_subscription: None,
            artifact_controller: ArtifactController::default(),
            branch_controller: BranchController::default(),
            commit_controller: CommitController::default(),
            trusted_actions: TrustedActionCoordinator::default(),
            window_alive: Arc::new(AtomicBool::new(true)),
            configured_models: None,
            model_capability_authority: None,
            model_capability_projection: ModelCapabilitySettingsProjection::Loading,
            model_capability_generation: 0,
            model_capability_pending: None,
            configured_reasoning: None,
            configured_reasoning_error: None,
            configured_reasoning_authority: None,
            loaded_replay_config: None,
            reasoning_save_pending: None,
            composer_reasoning_operation: 0,
            pending_composer_thinking: None,
            model_catalog_refresh_pending: false,
            reasoning_error_hold: None,
            model_catalog_loading: false,
            model_catalog_generation: 0,
            deferred_model_refresh: None,
        };
        window.start_pricing_load(cx);
        #[cfg(not(test))]
        window.start_updater(cx);
        #[cfg(test)]
        if window.composer_config_path_override.is_none() {
            window.start_updater(cx);
        }
        window
    }

    pub(crate) fn window_terminal_cleanup(&mut self) {
        self.composer_prediction = None;
        self.image_preview.retire();
        self.context_controller.cancel();
        self.window_alive.store(false, Ordering::SeqCst);
        for (_, active) in self.agent_controller.active.drain() {
            if let Some(control) = &active.computer_use {
                control.stop();
            }
            active.cancel.cancel();
        }
        self.file_index_controller.cancel();
        self.diff_controller.close();
        let _ = self.artifact_controller.close();
        for (_, route) in self.artifact_controller.retained.drain() {
            route.cancel.cancel();
        }
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

#[cfg(test)]
mod model_capability_tests;
