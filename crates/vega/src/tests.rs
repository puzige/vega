use std::fs;
use std::sync::{Arc, mpsc};
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

mod composer_predictions;
mod diff;
mod model_selection;
mod title_fixture;
use diff::install_diff_window_globals;

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
