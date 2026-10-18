use std::sync::Arc;
use std::time::Duration;

use gpui_kit::prelude::*;
use gpui_kit::{App, Entity};
use tempfile::TempDir;
use vega_conversation::types::*;
use vega_store::Store;
use vega_theme::*;
use vega_ui::conversation_stream::ConversationStream;
use vega_ui::settings::SettingsOpen;
use vega_ui::sidebar::{
    OpenedThread, PendingDeleteConfirm, SidebarCollapsed, SidebarWidth, VegaStore,
};

use crate::app_agent::*;
use crate::window::*;

mod session_completion_notifications;

mod model_selection {
    pub(super) fn model_selection_config(path: &std::path::Path) {
        std::fs::write(
            path,
            r#"[[providers]]
name = "owned"
base_url = "https://provider.invalid/v1"
models = ["gpt-5.6-terra", "gpt-5.6-luna"]
key_ref = "owned"

[defaults]
model = "gpt-5.6-terra"
permission_mode = "confirm"

[ui]
theme = "dark"
"#,
        )
        .unwrap();
    }
}

fn install_diff_window_globals(store: Store, thread: Thread, cx: &mut App) {
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

#[track_caller]
fn pump_test_app(
    cx: &mut gpui_kit::TestAppContext,
    mut ready: impl FnMut(&mut gpui_kit::TestAppContext) -> bool,
) {
    for _ in 0..400 {
        cx.executor()
            .advance_clock(crate::diff_controller::DIFF_RESULT_POLL);
        cx.run_until_parked();
        if ready(cx) {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("test app did not reach the expected terminal state");
}
