//! Sidebar shell (T09): the fixed 260px left column of the main window
//! layout ([vega-ui-spec.md §1](../../docs/vega-ui-spec.md)).
//!
//! This card only lands the shell: the project block is filled by T10, the
//! session block by T12, and the automation entry stays grayed out until
//! Phase 3 (A1-13). Collapse behavior:
//!
//! - Cmd+B ([`toggle_persisted`]) flips the [`SidebarCollapsed`] global and
//!   persists it as `ui.sidebar_collapsed` in `config.toml` (serde default
//!   `false`, so older configs keep loading).
//! - A viewport narrower than [`AUTO_COLLAPSE_WIDTH`] hides the sidebar
//!   regardless of the stored preference (auto-collapse, ui-spec §1).

use gpui::prelude::*;
use gpui::{AnyElement, App, Global, Window, actions, div, px};
use vega_store::config;
use vega_theme::{ThemeColors, Typography, theme};

actions!(vega_sidebar, [ToggleSidebar]);

/// Sidebar width in logical pixels (ui-spec §1).
pub const SIDEBAR_WIDTH: f32 = 260.0;

/// Viewport width below which the sidebar auto-collapses (ui-spec §1).
pub const AUTO_COLLAPSE_WIDTH: f32 = 960.0;

/// Content column max width in logical pixels (ui-spec §1).
pub const CONTENT_MAX_WIDTH: f32 = 820.0;

/// Content column minimum horizontal padding in logical pixels (ui-spec §1).
pub const CONTENT_MIN_PADDING: f32 = 24.0;

/// Whether the user collapsed the sidebar with Cmd+B.
///
/// Stored as a GPUI global (same pattern as [`crate::settings::SettingsOpen`])
/// and persisted in `config.toml` under `ui.sidebar_collapsed`. The effective
/// sidebar visibility is `!self.0 && viewport_width >= AUTO_COLLAPSE_WIDTH`
/// (the viewport rule is applied by the window root at render time).
pub struct SidebarCollapsed(pub bool);

impl Global for SidebarCollapsed {}

/// Loads the persisted collapsed preference; `false` when the config cannot
/// be read (error logged, sidebar stays visible — the safe default).
pub fn load_collapsed() -> bool {
    match config::load() {
        Ok(config) => config.ui.sidebar_collapsed,
        Err(error) => {
            tracing::error!(%error, "failed to read sidebar_collapsed from config.toml");
            false
        }
    }
}

/// Cmd+B handler: flips the preference, persists it to `config.toml`, and
/// refreshes windows. Persistence failures degrade to in-memory state
/// (ui-spec §4.6: no modals); the next successful toggle rewrites the file.
pub fn toggle_persisted(cx: &mut App) {
    let collapsed = !cx.global::<SidebarCollapsed>().0;
    cx.set_global(SidebarCollapsed(collapsed));
    match config::load() {
        Ok(mut config) => {
            config.ui.sidebar_collapsed = collapsed;
            if let Err(error) = config.save() {
                tracing::error!(%error, "failed to persist sidebar_collapsed to config.toml");
            }
        }
        Err(error) => {
            tracing::error!(%error, "failed to load config.toml for the sidebar toggle");
        }
    }
    cx.refresh_windows();
}

/// The sidebar shell: placeholder blocks only (project list → T10, session
/// list → T12, automation entry grayed out → Phase 3).
pub struct Sidebar;

impl Render for Sidebar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        div()
            .flex()
            .flex_col()
            .w(px(SIDEBAR_WIDTH))
            .h_full()
            .flex_shrink_0()
            .bg(colors.bg_sidebar)
            .px_4()
            .pt_4()
            .gap_4()
            .child(block("项目", Some("暂无项目"), &colors))
            .child(block("会话", Some("暂无会话"), &colors))
            .child(
                // Automation entry (A1-13): grayed out and inert until Phase 3.
                div()
                    .h(px(Typography::SIDEBAR_LINE_HEIGHT))
                    .flex()
                    .items_center()
                    .text_size(px(Typography::SIDEBAR))
                    .text_color(colors.text_tertiary)
                    .child("自动化"),
            )
            .into_any_element()
    }
}

/// One sidebar block: a block heading (ui-spec §3: 14px / 600) plus an
/// optional placeholder entry row (13px / 32px row height).
fn block(title: &'static str, hint: Option<&'static str>, colors: &ThemeColors) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_size(px(Typography::HEADING_BLOCK))
                .font_weight(Typography::HEADING_BLOCK_WEIGHT)
                .text_color(colors.text_primary)
                .child(title),
        )
        .children(hint.map(|hint| {
            div()
                .h(px(Typography::SIDEBAR_LINE_HEIGHT))
                .flex()
                .items_center()
                .text_size(px(Typography::SIDEBAR))
                .text_color(colors.text_secondary)
                .child(hint)
        }))
        .into_any_element()
}
