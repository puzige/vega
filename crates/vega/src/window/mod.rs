use std::path::PathBuf;
use std::sync::atomic::*;
use std::sync::*;

use gpui::prelude::*;
use gpui::*;
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
mod pricing;
mod render;
mod session;

use crate::app_agent::*;
use crate::artifact_controller::*;
use crate::branch_controller::*;
use crate::commit_controller::*;
use crate::diff_controller::*;
use crate::pricing_controller::*;
use crate::thread_reload::*;
use crate::trusted_action::*;

/// Root view of the main window: the A1 layout shell — a sidebar (260px,
/// collapsible) next to a content column (max 820px, centered) that hosts
/// either the settings view (Cmd+, / Esc), the opened session
/// ([`ConversationStream`], S3-T17), or the ui-spec §4.6 empty state.
pub(crate) struct VegaWindow {
    /// Sidebar with the [新建任务] button, projects block, and sessions block.
    pub(crate) sidebar: Entity<Sidebar>,
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
    pub(crate) diff_controller: DiffController,
    pub(crate) artifact_controller: ArtifactController,
    pub(crate) branch_controller: BranchController,
    pub(crate) commit_controller: CommitController,
    pub(crate) trusted_actions: TrustedActionCoordinator,
    pub(crate) window_alive: Arc<AtomicBool>,
    #[cfg(test)]
    pub(crate) commit_provider_override: Option<Arc<dyn vega_runtime::Provider>>,
    #[cfg(test)]
    pub(crate) agent_provider_override: Option<Arc<dyn vega_runtime::Provider>>,
    #[cfg(test)]
    pub(crate) commit_test_probe: Option<Arc<CommitTestProbe>>,
    #[cfg(test)]
    pub(crate) pricing_drop_next_worker_result: bool,
    #[cfg(test)]
    pub(crate) pricing_next_worker_gate: Option<Arc<std::sync::Barrier>>,
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

    pub(crate) fn window_terminal_cleanup(&mut self) {
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
}

impl Drop for VegaWindow {
    fn drop(&mut self) {
        self.window_terminal_cleanup();
    }
}
