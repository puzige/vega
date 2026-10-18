use super::*;

pub(crate) fn install_diff_window_globals(store: Store, thread: Thread, cx: &mut App) {
    gpui_kit::init(cx);
    cx.set_global(Theme::light());
    cx.set_global(SettingsOpen(false));
    cx.set_global(SidebarCollapsed(false));
    cx.set_global(SidebarWidth(Layout::SIDEBAR_WIDTH));
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
