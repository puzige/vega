//! Compact, safe artifact cards embedded in the virtual conversation stream.

use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, MouseButton,
    Subscription, Window, actions, div, px,
};
use std::collections::{HashMap, HashSet};
use vega_conversation::types::{
    ArtifactCard as ArtifactProjection, ArtifactCardId, ArtifactPreviewProjection,
    GitWorkspaceErrorCode, OpenInOutcome, OpenInTarget, WorkspaceFileId,
};
use vega_theme::{Layout, ThemeColors, Typography, theme};

use crate::conversation_stream::{MONOFONT, OpenWorkspaceDiffRequested, ROW_HEIGHT};
use crate::icons::{Icon, icon};
use crate::interaction::InteractionExt;
actions!(vega_artifact_card, [ArtifactActivate, ArtifactClear]);

const OPEN_TARGETS: [(OpenInTarget, &str); 6] = [
    (OpenInTarget::VisualStudioCode, "VS Code"),
    (OpenInTarget::Cursor, "Cursor"),
    (OpenInTarget::Zed, "Zed"),
    (OpenInTarget::Terminal, "Terminal"),
    (OpenInTarget::DefaultApplication, "Default"),
    (OpenInTarget::RevealInFinder, "Finder"),
];

/// Explicit request for the sole bounded artifact body channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactPreviewRequested {
    pub thread_id: String,
    pub project_id: String,
    pub card_id: ArtifactCardId,
    pub file_id: WorkspaceFileId,
}

/// Explicit request for one fixed external handoff target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactOpenRequested {
    pub thread_id: String,
    pub project_id: String,
    pub card_id: ArtifactCardId,
    pub file_id: WorkspaceFileId,
    pub target: OpenInTarget,
}

/// Clears preview/inline state and cancels requests without deleting history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactCleared {
    pub thread_id: String,
    pub project_id: String,
    pub card_id: ArtifactCardId,
}

/// Route-owned presentation state; it never reads a path or filesystem.
pub struct ArtifactCard {
    reading_preview: bool,
    thread_id: String,
    project_id: String,
    projection: ArtifactProjection,
    preview_lines: Vec<String>,
    inline_error: Option<GitWorkspaceErrorCode>,
    opening: Option<OpenInTarget>,
    focus: [FocusHandle; 7],
    compact_focus: [FocusHandle; 2],
    focus_subscriptions: Vec<Subscription>,
    hovered: bool,
    menu_open: bool,
}

impl EventEmitter<ArtifactPreviewRequested> for ArtifactCard {}
impl EventEmitter<ArtifactOpenRequested> for ArtifactCard {}
impl EventEmitter<ArtifactCleared> for ArtifactCard {}

impl Focusable for ArtifactCard {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.compact_focus[0].clone()
    }
}

impl Render for ArtifactCard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_compact(true, window, cx)
    }
}

impl ArtifactCard {
    pub(crate) fn set_reading_preview(&mut self, reading: bool, cx: &mut Context<Self>) {
        if self.reading_preview != reading {
            self.reading_preview = reading;
            self.menu_open = false;
            cx.notify();
        }
    }

    pub fn new(
        thread_id: String,
        project_id: String,
        projection: ArtifactProjection,
        cx: &mut Context<Self>,
    ) -> Self {
        let preview_enabled = projection.preview_available && projection.current_file_id.is_some();
        let compact_focus = std::array::from_fn(|_| cx.focus_handle().tab_stop(true));
        Self {
            reading_preview: false,
            thread_id,
            project_id,
            projection,
            preview_lines: Vec::new(),
            inline_error: None,
            opening: None,
            compact_focus,
            focus_subscriptions: Vec::new(),
            hovered: false,
            menu_open: false,
            focus: std::array::from_fn(|index| {
                cx.focus_handle()
                    .tab_index(index as isize + 1)
                    .tab_stop(index != 0 || preview_enabled)
            }),
        }
    }

    pub fn id(&self) -> ArtifactCardId {
        self.projection.id
    }

    pub fn projection(&self) -> &ArtifactProjection {
        &self.projection
    }

    pub fn row_count(&self) -> usize {
        2 + self.preview_lines.len() + usize::from(self.inline_error.is_some())
    }

    pub fn apply_metadata(
        &mut self,
        projection: ArtifactProjection,
        cx: &mut Context<Self>,
    ) -> bool {
        if projection.id != self.projection.id {
            return false;
        }
        let capability_changed = projection.current_file_id != self.projection.current_file_id
            || !projection.preview_available;
        self.projection = projection;
        if capability_changed {
            self.preview_lines.clear();
            self.inline_error = None;
            self.opening = None;
        }
        cx.notify();
        true
    }

    pub fn apply_preview(
        &mut self,
        preview: ArtifactPreviewProjection,
        cx: &mut Context<Self>,
    ) -> bool {
        if preview.card_id() != self.projection.id
            || self.projection.current_file_id != Some(preview.file_id())
            || !self.projection.preview_available
        {
            return false;
        }
        self.preview_lines = preview_rows(preview.text());
        self.inline_error = None;
        cx.notify();
        true
    }

    pub fn apply_preview_error(
        &mut self,
        card_id: ArtifactCardId,
        file_id: WorkspaceFileId,
        code: GitWorkspaceErrorCode,
        cx: &mut Context<Self>,
    ) -> bool {
        if card_id != self.projection.id || self.projection.current_file_id != Some(file_id) {
            return false;
        }
        self.preview_lines.clear();
        self.inline_error = Some(code);
        cx.notify();
        true
    }

    pub fn set_opening(&mut self, target: Option<OpenInTarget>, cx: &mut Context<Self>) {
        self.opening = target;
        if target.is_some() {
            self.inline_error = None;
        }
        cx.notify();
    }

    pub fn apply_open_outcome(&mut self, outcome: OpenInOutcome, cx: &mut Context<Self>) -> bool {
        if outcome.card_id != self.projection.id || self.opening != Some(outcome.target) {
            return false;
        }
        self.opening = None;
        self.inline_error = None;
        cx.notify();
        true
    }

    pub fn apply_open_error(
        &mut self,
        card_id: ArtifactCardId,
        target: OpenInTarget,
        code: GitWorkspaceErrorCode,
        cx: &mut Context<Self>,
    ) -> bool {
        if card_id != self.projection.id || self.opening != Some(target) {
            return false;
        }
        self.opening = None;
        self.inline_error = Some(code);
        cx.notify();
        true
    }

    pub fn fail_corrupt(&mut self, cx: &mut Context<Self>) {
        self.preview_lines.clear();
        self.opening = None;
        self.inline_error = Some(GitWorkspaceErrorCode::ArtifactConflict);
        cx.notify();
    }

    /// Cancels one UI-local operation while retaining the current capability.
    pub fn fail_request(&mut self, code: GitWorkspaceErrorCode, cx: &mut Context<Self>) {
        self.preview_lines.clear();
        self.opening = None;
        self.inline_error = Some(code);
        cx.notify();
    }

    /// Permanently disables a historical card after its owning route closes.
    pub fn invalidate(&mut self, code: GitWorkspaceErrorCode, cx: &mut Context<Self>) {
        self.projection.current_file_id = None;
        self.projection.preview_available = false;
        self.fail_request(code, cx);
    }

    /// Requests the production bounded preview channel.
    pub fn preview(&mut self, cx: &mut Context<Self>) {
        if !self.projection.preview_available || self.opening.is_some() {
            return;
        }
        let Some(file_id) = self.projection.current_file_id else {
            return;
        };
        self.inline_error = None;
        cx.emit(ArtifactPreviewRequested {
            thread_id: self.thread_id.clone(),
            project_id: self.project_id.clone(),
            card_id: self.projection.id,
            file_id,
        });
        cx.notify();
    }

    fn open(&mut self, target: OpenInTarget, cx: &mut Context<Self>) {
        if self.reading_preview || self.opening.is_some() {
            return;
        }
        let Some(file_id) = self.projection.current_file_id else {
            return;
        };
        self.menu_open = false;
        self.opening = Some(target);
        self.inline_error = None;
        cx.emit(ArtifactOpenRequested {
            thread_id: self.thread_id.clone(),
            project_id: self.project_id.clone(),
            card_id: self.projection.id,
            file_id,
            target,
        });
        cx.notify();
    }

    fn clear(&mut self, cx: &mut Context<Self>) {
        self.preview_lines.clear();
        self.inline_error = None;
        self.opening = None;
        cx.emit(ArtifactCleared {
            thread_id: self.thread_id.clone(),
            project_id: self.project_id.clone(),
            card_id: self.projection.id,
        });
        cx.notify();
    }

    fn activate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focused = self
            .focus
            .iter()
            .position(|handle| handle.is_focused(window));
        match focused {
            Some(0) => self.preview(cx),
            Some(index) => self.open(OPEN_TARGETS[index - 1].0, cx),
            None if self.projection.preview_available => self.preview(cx),
            None => self.open(OPEN_TARGETS[0].0, cx),
        }
    }

    fn close_menu(&mut self, cx: &mut Context<Self>) {
        if self.menu_open {
            self.menu_open = false;
            cx.notify();
        }
    }

    fn toggle_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.reading_preview
            || self.opening.is_some()
            || self.projection.current_file_id.is_none()
        {
            return;
        }
        self.menu_open = !self.menu_open;
        self.compact_focus[1].focus(window, cx);
        cx.notify();
    }

    fn observe_compact_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.focus_subscriptions.is_empty() {
            return;
        }
        for focus in self.compact_focus.iter().chain(self.focus.iter()) {
            self.focus_subscriptions
                .push(cx.on_focus_in(focus, window, |_, _, cx| cx.notify()));
            self.focus_subscriptions
                .push(cx.on_focus_out(focus, window, |this, _, window, cx| {
                    if !this
                        .compact_focus
                        .iter()
                        .chain(this.focus.iter())
                        .any(|focus| focus.contains_focused(window, cx))
                    {
                        this.menu_open = false;
                    }
                    cx.notify();
                }));
        }
    }

    fn render_compact(
        &mut self,
        last: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.observe_compact_focus(window, cx);
        let card = cx.entity();
        let colors = theme(cx).colors;
        let projection = self.projection.clone();
        let focus = self.compact_focus.clone();
        let reading = self.reading_preview;
        let opening = self.opening.is_some();
        let menu_open =
            self.menu_open && !reading && !opening && projection.current_file_id.is_some();
        let preview_enabled =
            projection.current_file_id.is_some() && projection.preview_available && !opening;
        let open_enabled = projection.current_file_id.is_some() && !opening;
        if (!menu_open
            && self
                .focus
                .iter()
                .skip(1)
                .any(|focus| focus.is_focused(window)))
            || (reading && focus[1].is_focused(window))
        {
            focus[usize::from(!reading && open_enabled)].focus(window, cx);
        }
        let show_actions = self.hovered
            || menu_open
            || focus.iter().any(|focus| focus.contains_focused(window, cx));
        let hint = if opening {
            vega_i18n::t("ui.opening_file_in_progress")
        } else {
            vega_i18n::t("ui.file_unavailable")
        };
        let details = match projection.size_bytes {
            Some(size) => format!("{} · {}", projection.label, format_size(size)),
            None => projection.label.clone(),
        };
        let details = if preview_enabled {
            details
        } else {
            format!("{details} · {hint}")
        };
        let (directory, filename) = artifact_path(&projection.label);
        let hover_card = card.clone();
        let key_card = card.clone();
        let preview_card = card.clone();
        let mut row = div()
            .id(("artifact-file", card.entity_id()))
            .track_focus(
                &focus[0]
                    .clone()
                    .tab_stop(preview_enabled || (!reading && open_enabled)),
            )
            .tab_stop(preview_enabled || (!reading && open_enabled))
            .role(if preview_enabled {
                gpui_kit::Role::Button
            } else {
                gpui_kit::Role::Group
            })
            .aria_label(if preview_enabled {
                vega_i18n::t_args("ui.preview_name", &[("name", &projection.label)])
            } else {
                projection.label.clone()
            })
            .aria_description(if preview_enabled {
                vega_i18n::t("ui.preview_file")
            } else {
                hint
            })
            .tooltip(move |_, cx| crate::icons::tooltip(details.clone(), cx))
            .relative()
            .h(px(Layout::ARTIFACT_ROW_HEIGHT))
            .flex_shrink_0()
            .w_full()
            .min_w_0()
            .flex()
            .items_center()
            .px_3()
            .gap_2()
            .border_t(px(Layout::ARTIFACT_BORDER))
            .border_color(colors.border_subtle)
            .when(last, |row| {
                row.rounded_bl(px(Layout::TOOL_DETAIL_RADIUS))
                    .rounded_br(px(Layout::TOOL_DETAIL_RADIUS))
            })
            .when(preview_enabled, |row| {
                row.cursor_pointer()
                    .hover(move |style| style.bg(colors.bg_hover))
            })
            .focus_visible(move |style| style.bg(colors.bg_hover))
            .on_mouse_up(MouseButton::Left, move |_, window, cx| {
                cx.stop_propagation();
                if preview_enabled {
                    preview_card.update(cx, |card, cx| {
                        card.compact_focus[0].focus(window, cx);
                        card.preview(cx);
                    });
                }
            })
            .on_hover(move |hovered, _, cx| {
                hover_card.update(cx, |card, cx| {
                    card.hovered = *hovered;
                    cx.notify();
                });
            })
            .on_key_down(move |event, window, cx| {
                if event.keystroke.key == "tab" {
                    if event.keystroke.modifiers.shift {
                        window.focus_prev(cx);
                    } else {
                        window.focus_next(cx);
                    }
                    cx.stop_propagation();
                } else if event.keystroke.key == "escape" && key_card.read(cx).menu_open {
                    key_card.update(cx, |card, cx| {
                        card.close_menu(cx);
                        card.compact_focus[1].focus(window, cx);
                    });
                    cx.stop_propagation();
                } else if matches!(event.keystroke.key.as_str(), "enter" | "space")
                    && key_card.read(cx).compact_focus[0].is_focused(window)
                {
                    cx.stop_propagation();
                    if preview_enabled {
                        key_card.update(cx, ArtifactCard::preview);
                    }
                }
            })
            .child(icon(Icon::Document, colors.text_secondary))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .overflow_hidden()
                    .flex()
                    .items_center()
                    .text_size(px(Typography::BODY))
                    .when(!directory.is_empty(), |path| {
                        path.child(
                            div()
                                .min_w_0()
                                .flex_shrink_1()
                                .truncate()
                                .text_color(colors.text_secondary)
                                .child(directory.to_owned()),
                        )
                    })
                    .child(
                        div()
                            .min_w_0()
                            .max_w_full()
                            .flex_shrink_0()
                            .truncate()
                            .text_color(colors.text_primary)
                            .child(filename.to_owned()),
                    ),
            );
        if !reading && show_actions {
            let menu_card = card.clone();
            let menu_key = card.clone();
            let trigger = compact_button(
                "ui.open_file_in",
                Icon::More,
                &focus[1],
                open_enabled,
                hint,
                colors,
            )
            .capture_any_mouse_down(|_, _, cx| cx.stop_propagation())
            .on_mouse_up(MouseButton::Left, move |_, window, cx| {
                cx.stop_propagation();
                if open_enabled {
                    menu_card.update(cx, |card, cx| card.toggle_menu(window, cx));
                }
            })
            .on_key_down(move |event, window, cx| {
                if event.keystroke.key == "tab"
                    && !event.keystroke.modifiers.shift
                    && menu_key.read(cx).menu_open
                {
                    cx.stop_propagation();
                    let menu_focus = menu_key.read(cx).focus[1].clone();
                    menu_focus.focus(window, cx);
                } else if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    cx.stop_propagation();
                    menu_key.update(cx, |card, cx| card.toggle_menu(window, cx));
                } else if event.keystroke.key == "down" && open_enabled {
                    cx.stop_propagation();
                    menu_key.update(cx, |card, cx| {
                        card.menu_open = true;
                        card.focus[1].focus(window, cx);
                        cx.notify();
                    });
                }
            });
            row = row.child(trigger);
        } else if !reading {
            row = row.child(div().size(px(Layout::ARTIFACT_ACTION_SIZE)).flex_shrink_0());
        }
        if menu_open {
            let outside = card.clone();
            let mut popup = div()
                .id(("artifact-open-menu", card.entity_id()))
                .occlude()
                .w(px(Layout::TASK_MENU_WIDTH))
                .py_1()
                .flex()
                .flex_col()
                .bg(colors.bg_elevated)
                .border_1()
                .border_color(colors.border_subtle)
                .rounded_lg()
                .shadow_md()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_mouse_down_out(move |_, _, cx| outside.update(cx, ArtifactCard::close_menu));
            for (index, (target, label)) in OPEN_TARGETS.into_iter().enumerate() {
                let label = match target {
                    OpenInTarget::Terminal => vega_i18n::t("ui.terminal"),
                    OpenInTarget::DefaultApplication => vega_i18n::t("ui.default"),
                    _ => label,
                };
                let open_card = card.clone();
                let key_card = card.clone();
                let item_focus = self.focus[index + 1].clone();
                let item = crate::menu_list::row_container(
                    ("artifact-open-target", index),
                    false,
                    true,
                    colors,
                )
                .track_focus(&item_focus)
                .tab_stop(true)
                .role(gpui_kit::Role::MenuItem)
                .aria_label(label)
                .text_color(colors.text_primary)
                .child(label)
                .on_mouse_up(MouseButton::Left, move |_, window, cx| {
                    cx.stop_propagation();
                    open_card.update(cx, |card, cx| {
                        card.close_menu(cx);
                        card.compact_focus[1].focus(window, cx);
                        card.open(target, cx);
                    });
                })
                .on_key_down(move |event, window, cx| {
                    let key = event.keystroke.key.as_str();
                    if key == "tab" {
                        cx.stop_propagation();
                        key_card.update(cx, |card, cx| {
                            if event.keystroke.modifiers.shift {
                                if index == 0 {
                                    card.compact_focus[1].focus(window, cx);
                                } else {
                                    card.focus[index].focus(window, cx);
                                }
                            } else if index + 1 < OPEN_TARGETS.len() {
                                card.focus[index + 2].focus(window, cx);
                            } else {
                                card.close_menu(cx);
                                card.compact_focus[1].focus(window, cx);
                                window.focus_next(cx);
                            }
                        });
                    } else if matches!(key, "enter" | "space" | "up" | "down") {
                        cx.stop_propagation();
                        key_card.update(cx, |card, cx| {
                            if matches!(key, "enter" | "space") {
                                card.close_menu(cx);
                                card.compact_focus[1].focus(window, cx);
                                card.open(target, cx);
                            } else {
                                let next = if key == "down" {
                                    (index + 1) % 6
                                } else {
                                    (index + 5) % 6
                                };
                                card.focus[next + 1].focus(window, cx);
                            }
                        });
                    }
                });
                popup = popup.child(item);
            }
            row = row.child(
                div().absolute().top_0().right_0().child(
                    gpui_kit::anchored()
                        .anchor(gpui_kit::Anchor::TopRight)
                        .position_mode(gpui_kit::AnchoredPositionMode::Local)
                        .position(gpui_kit::point(px(0.), px(Layout::ARTIFACT_ROW_HEIGHT)))
                        .snap_to_window_with_margin(px(Layout::TOOLBAR_TRAILING_INSET))
                        .child(gpui_kit::deferred(popup).with_priority(2)),
                ),
            );
        }
        row.into_any_element()
    }

    pub fn render_row(
        card: Entity<Self>,
        row: usize,
        _window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        if row == 1 && card.read(cx).reading_preview {
            return div().h_0().into_any_element();
        }
        let colors = theme(cx).colors;
        let (projection, preview_lines, inline_error, opening, focus) = {
            let card = card.read(cx);
            (
                card.projection.clone(),
                card.preview_lines.clone(),
                card.inline_error,
                card.opening,
                card.focus.clone(),
            )
        };
        let base = div()
            .h(px(ROW_HEIGHT))
            .w_full()
            .min_w_0()
            .flex()
            .items_center()
            .overflow_hidden()
            .px_3()
            .bg(colors.bg_elevated)
            .border_color(colors.border_subtle);
        if row == 0 {
            return base
                .border_t_1()
                .rounded_tl_lg()
                .rounded_tr_lg()
                .gap_2()
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .truncate()
                        .font_family(MONOFONT.to_string())
                        .text_size(px(Typography::CODE))
                        .child(projection.label),
                )
                .into_any_element();
        }
        if row == 1 {
            let preview_enabled = projection.preview_available
                && projection.current_file_id.is_some()
                && opening.is_none();
            let open_enabled = projection.current_file_id.is_some() && opening.is_none();
            let preview_card = card.clone();
            let activate_card = card.clone();
            let clear_card = card.clone();
            let mut actions = div()
                .w_full()
                .min_w_0()
                .flex()
                .gap_1()
                .key_context("ArtifactCard")
                .on_action(move |_: &ArtifactActivate, window, cx| {
                    activate_card.update(cx, |card, cx| card.activate(window, cx));
                })
                .on_action(move |_: &ArtifactClear, _, cx| {
                    clear_card.update(cx, ArtifactCard::clear);
                })
                .child(
                    artifact_button(
                        vega_i18n::t("ui.preview"),
                        preview_enabled,
                        focus[0].clone(),
                        colors,
                    )
                    .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                        preview_card.update(cx, ArtifactCard::preview);
                    }),
                );
            for (index, (target, label)) in OPEN_TARGETS.iter().copied().enumerate() {
                let label = match target {
                    OpenInTarget::Terminal => vega_i18n::t("ui.terminal"),
                    OpenInTarget::DefaultApplication => vega_i18n::t("ui.default"),
                    _ => label,
                };
                let open_card = card.clone();
                actions = actions.child(
                    artifact_button(label, open_enabled, focus[index + 1].clone(), colors)
                        .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                            open_card.update(cx, |card, cx| card.open(target, cx));
                        }),
                );
            }
            return base
                .when(preview_lines.is_empty() && inline_error.is_none(), |row| {
                    row.border_b_1().rounded_bl_lg().rounded_br_lg()
                })
                .child(actions)
                .into_any_element();
        }
        let preview_index = row - 2;
        if let Some(line) = preview_lines.get(preview_index) {
            return base
                .bg(colors.code_bg)
                .font_family(MONOFONT.to_string())
                .text_size(px(Typography::CODE))
                .text_color(colors.text_primary)
                .child(div().min_w_0().w_full().truncate().child(line.clone()))
                .into_any_element();
        }
        base.border_b_1()
            .rounded_bl_lg()
            .rounded_br_lg()
            .text_size(px(Typography::SIDEBAR))
            .text_color(colors.danger)
            .child(error_label(inline_error))
            .into_any_element()
    }
}

struct ArtifactGroupFile {
    card: Entity<ArtifactCard>,
    capture_order: u64,
    _subscription: Subscription,
}

pub struct ArtifactGroup {
    message_id: String,
    files: Vec<ArtifactGroupFile>,
    capture_order: u64,
    expanded: bool,
    capture_errors: Vec<(String, GitWorkspaceErrorCode)>,
    file_errors: Vec<(Entity<ArtifactCard>, GitWorkspaceErrorCode)>,
    review_request: OpenWorkspaceDiffRequested,
    review_disabled_reason_key: Option<&'static str>,
}

impl EventEmitter<OpenWorkspaceDiffRequested> for ArtifactGroup {}

impl ArtifactGroup {
    pub(crate) fn new(
        message_id: String,
        review_request: OpenWorkspaceDiffRequested,
        reading_preview: bool,
        standalone: bool,
    ) -> Self {
        Self {
            message_id,
            files: Vec::new(),
            capture_order: 0,
            expanded: false,
            capture_errors: Vec::new(),
            file_errors: Vec::new(),
            review_request,
            review_disabled_reason_key: review_disabled_reason_key(reading_preview, standalone),
        }
    }

    pub fn message_id(&self) -> &str {
        &self.message_id
    }

    fn visible_files(&self) -> usize {
        if self.expanded {
            self.files.len()
        } else {
            self.files.len().min(Layout::ARTIFACT_VISIBLE_FILES)
        }
    }

    pub fn row_count(&self) -> usize {
        self.visible_files()
            + usize::from(self.files.len() > Layout::ARTIFACT_VISIBLE_FILES)
            + self.capture_errors.len()
            + self.file_errors.len()
    }

    pub(crate) fn estimated_height(&self) -> f32 {
        let rows = self.row_count();
        if rows == 0 {
            0.
        } else {
            Layout::ARTIFACT_HEADER_HEIGHT
                + rows as f32 * Layout::ARTIFACT_ROW_HEIGHT
                + 2. * Layout::ARTIFACT_BORDER
        }
    }

    pub(crate) fn set_review_availability(
        &mut self,
        reading_preview: bool,
        standalone: bool,
        cx: &mut Context<Self>,
    ) {
        let reason = review_disabled_reason_key(reading_preview, standalone);
        if self.review_disabled_reason_key != reason {
            self.review_disabled_reason_key = reason;
            cx.notify();
        }
    }

    fn open_review(&mut self, cx: &mut Context<Self>) {
        if self.review_disabled_reason_key.is_none() && !self.files.is_empty() {
            cx.emit(self.review_request.clone());
        }
    }

    fn toggle_expanded(&mut self, cx: &mut Context<Self>) {
        self.expanded = !self.expanded;
        cx.notify();
    }

    fn sync_file(&mut self, card: Entity<ArtifactCard>, cx: &mut Context<Self>) {
        let mut indices = HashMap::<String, usize>::with_capacity(self.files.len());
        let mut files = Vec::<ArtifactGroupFile>::with_capacity(self.files.len());
        for file in self.files.drain(..) {
            let label = file.card.read(cx).projection.label.clone();
            if let Some(&index) = indices.get(&label) {
                if file.capture_order > files[index].capture_order {
                    files[index] = file;
                }
            } else {
                indices.insert(label, files.len());
                files.push(file);
            }
        }
        self.files = files;
        let live = self
            .files
            .iter()
            .map(|file| file.card.entity_id())
            .collect::<HashSet<_>>();
        self.file_errors
            .retain(|(card, _)| live.contains(&card.entity_id()));
        if !live.contains(&card.entity_id()) {
            cx.notify();
            return;
        }
        let error = card.read(cx).inline_error;
        if let Some(index) = self
            .file_errors
            .iter()
            .position(|(owned, _)| owned == &card)
        {
            if let Some(error) = error {
                self.file_errors[index].1 = error;
            } else {
                self.file_errors.remove(index);
            }
        } else if let Some(error) = error {
            self.file_errors.push((card, error));
        }
        cx.notify();
    }

    pub(crate) fn capture(
        &mut self,
        call_id: &str,
        card: Entity<ArtifactCard>,
        cx: &mut Context<Self>,
    ) {
        self.capture_order += 1;
        let subscription = cx.observe(&card, |this, card, cx| this.sync_file(card, cx));
        self.files.push(ArtifactGroupFile {
            card: card.clone(),
            capture_order: self.capture_order,
            _subscription: subscription,
        });
        self.capture_errors.retain(|(owner, _)| owner != call_id);
        self.sync_file(card, cx);
    }

    pub(crate) fn resolve_capture_error(&mut self, call_id: &str, cx: &mut Context<Self>) {
        let before = self.capture_errors.len();
        self.capture_errors.retain(|(owner, _)| owner != call_id);
        if before != self.capture_errors.len() {
            cx.notify();
        }
    }

    pub(crate) fn capture_error(
        &mut self,
        call_id: &str,
        code: GitWorkspaceErrorCode,
        cx: &mut Context<Self>,
    ) {
        if let Some((_, error)) = self
            .capture_errors
            .iter_mut()
            .find(|(owner, _)| owner == call_id)
        {
            *error = code;
        } else {
            self.capture_errors.push((call_id.to_owned(), code));
        }
        cx.notify();
    }

    pub(crate) fn render(group: Entity<Self>, window: &mut Window, cx: &mut App) -> AnyElement {
        let state = group.read(cx);
        if state.row_count() == 0 {
            return div().h_0().into_any_element();
        }
        let visible = state.visible_files();
        let total = state.files.len();
        let expanded = state.expanded;
        let has_toggle = total > Layout::ARTIFACT_VISIBLE_FILES;
        let heading = if total == 1 {
            let card = state.files[0].card.read(cx);
            vega_i18n::t_args(
                "ui.artifact.edited_file",
                &[("name", artifact_path(&card.projection.label).1)],
            )
        } else {
            vega_i18n::t_args("ui.artifact.edited_files", &[("count", &total.to_string())])
        };
        let review_reason = state
            .review_disabled_reason_key
            .or((total == 0).then_some("ui.artifact.review_empty"))
            .map(vega_i18n::t);
        let files = state
            .files
            .iter()
            .take(visible)
            .map(|file| file.card.clone())
            .collect::<Vec<_>>();
        let errors = state
            .capture_errors
            .iter()
            .map(|(_, code)| *code)
            .chain(state.file_errors.iter().map(|(_, code)| *code))
            .collect::<Vec<_>>();
        let colors = theme(cx).colors;
        let review_focus = crate::disclosure::focus(
            format!("artifact-review-{}", group.entity_id()).into(),
            window,
            cx,
        );
        let review_click = group.clone();
        let review_key = group.clone();
        let review = div()
            .id(("artifact-review", group.entity_id()))
            .interactive(colors, review_reason.is_none(), false)
            .track_focus(&review_focus)
            .tab_stop(true)
            .role(gpui_kit::Role::Button)
            .aria_label(vega_i18n::t("ui.open_review"))
            .aria_description(
                review_reason.unwrap_or(vega_i18n::t("ui.artifact.open_workspace_changes")),
            )
            .tooltip(move |_, cx| {
                crate::icons::tooltip(review_reason.unwrap_or(vega_i18n::t("ui.open_review")), cx)
            })
            .focus_visible(move |style| style.bg(colors.bg_hover))
            .h(px(Layout::ARTIFACT_ACTION_SIZE))
            .px_2()
            .flex_shrink_0()
            .flex()
            .items_center()
            .rounded_md()
            .text_size(px(Typography::BODY))
            .text_color(if review_reason.is_none() {
                colors.text_primary
            } else {
                colors.text_tertiary
            })
            .when(review_reason.is_none(), |button| button.cursor_pointer())
            .child(vega_i18n::t("ui.open_review"))
            .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                cx.stop_propagation();
                review_click.update(cx, ArtifactGroup::open_review);
            })
            .on_key_down(move |event, window, cx| {
                if event.keystroke.key == "tab" {
                    if event.keystroke.modifiers.shift {
                        window.focus_prev(cx);
                    } else {
                        window.focus_next(cx);
                    }
                    cx.stop_propagation();
                } else if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    cx.stop_propagation();
                    review_key.update(cx, ArtifactGroup::open_review);
                }
            });
        let header = div()
            .h(px(Layout::ARTIFACT_HEADER_HEIGHT))
            .flex_shrink_0()
            .min_w_0()
            .px_3()
            .flex()
            .items_center()
            .gap_2()
            .child(icon(Icon::Document, colors.text_secondary))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .text_size(px(Typography::HEADING_CARD))
                    .font_weight(Typography::HEADING_CARD_WEIGHT)
                    .text_color(colors.text_primary)
                    .child(heading),
            )
            .child(review);
        let mut container = div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .bg(colors.bg_elevated)
            .border(px(Layout::ARTIFACT_BORDER))
            .border_color(colors.border_subtle)
            .rounded(px(Layout::TOOL_DETAIL_RADIUS))
            .child(header);
        for (index, card) in files.into_iter().enumerate() {
            let last = index + 1 == visible && !has_toggle && errors.is_empty();
            container =
                container.child(card.update(cx, |card, cx| card.render_compact(last, window, cx)));
        }
        if has_toggle {
            let label = if expanded {
                vega_i18n::t("ui.artifact.collapse_files").to_owned()
            } else {
                vega_i18n::t_args(
                    "ui.artifact.show_more_files",
                    &[("count", &(total - visible).to_string())],
                )
            };
            let disclosure_id: gpui_kit::SharedString =
                format!("artifact-disclosure-{}", group.entity_id()).into();
            let focus = crate::disclosure::focus(disclosure_id.clone(), window, cx);
            let click_focus = focus.clone();
            let click_group = group.clone();
            let key_group = group.clone();
            let toggle = div()
                .id(("artifact-show-more", group.entity_id()))
                .group(disclosure_id.clone())
                .interactive(colors, true, false)
                .track_focus(&focus)
                .tab_stop(true)
                .role(gpui_kit::Role::Button)
                .aria_label(label.clone())
                .aria_expanded(expanded)
                .focus_visible(move |style| style.bg(colors.bg_hover))
                .h(px(Layout::ARTIFACT_ROW_HEIGHT))
                .flex_shrink_0()
                .border_t(px(Layout::ARTIFACT_BORDER))
                .border_color(colors.border_subtle)
                .when(errors.is_empty(), |row| {
                    row.rounded_bl(px(Layout::TOOL_DETAIL_RADIUS))
                        .rounded_br(px(Layout::TOOL_DETAIL_RADIUS))
                })
                .px_3()
                .flex()
                .items_center()
                .gap_2()
                .cursor_pointer()
                .text_size(px(Typography::BODY))
                .text_color(colors.text_secondary)
                .child(label)
                .child(crate::disclosure::inline_chevron(
                    disclosure_id,
                    &focus,
                    expanded,
                    true,
                    colors,
                    window,
                ))
                .on_mouse_up(MouseButton::Left, move |_, window, cx| {
                    cx.stop_propagation();
                    click_focus.focus(window, cx);
                    click_group.update(cx, ArtifactGroup::toggle_expanded);
                })
                .on_key_down(move |event, window, cx| {
                    if event.keystroke.key == "tab" {
                        if event.keystroke.modifiers.shift {
                            window.focus_prev(cx);
                        } else {
                            window.focus_next(cx);
                        }
                        cx.stop_propagation();
                    } else if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        cx.stop_propagation();
                        key_group.update(cx, ArtifactGroup::toggle_expanded);
                    }
                });
            container = container.child(toggle);
        }
        let error_count = errors.len();
        for (index, error) in errors.into_iter().enumerate() {
            container = container.child(
                div()
                    .h(px(Layout::ARTIFACT_ROW_HEIGHT))
                    .flex_shrink_0()
                    .border_t(px(Layout::ARTIFACT_BORDER))
                    .border_color(colors.border_subtle)
                    .when(index + 1 == error_count, |row| {
                        row.rounded_bl(px(Layout::TOOL_DETAIL_RADIUS))
                            .rounded_br(px(Layout::TOOL_DETAIL_RADIUS))
                    })
                    .px_3()
                    .flex()
                    .items_center()
                    .text_size(px(Typography::BODY))
                    .text_color(colors.danger)
                    .child(error_label(Some(error))),
            );
        }
        container.into_any_element()
    }
}

fn review_disabled_reason_key(reading_preview: bool, standalone: bool) -> Option<&'static str> {
    if reading_preview {
        Some("ui.artifact.review_reading_preview")
    } else if standalone {
        Some("ui.artifact.review_project_required")
    } else {
        None
    }
}

fn artifact_path(label: &str) -> (&str, &str) {
    label
        .rfind('/')
        .map_or(("", label), |index| label.split_at(index + 1))
}

fn format_size(size: u64) -> String {
    if size < 1024 {
        return format!("{size} B");
    }
    let units = ["KB", "MB", "GB", "TB", "PB", "EB"];
    let mut value = size as f64 / 1024.;
    let mut unit = 0;
    while value >= 1024. && unit + 1 < units.len() {
        value /= 1024.;
        unit += 1;
    }
    format!("{value:.1} {}", units[unit])
}

fn compact_button(
    label_key: &'static str,
    glyph: Icon,
    focus: &FocusHandle,
    enabled: bool,
    disabled_hint: &'static str,
    colors: ThemeColors,
) -> crate::interaction::Interaction {
    let label = vega_i18n::t(label_key);
    div()
        .id(label_key)
        .interactive(colors, enabled, false)
        .track_focus(&focus.clone().tab_stop(enabled))
        .tab_stop(enabled)
        .role(gpui_kit::Role::Button)
        .aria_label(label)
        .size(px(Layout::ARTIFACT_ACTION_SIZE))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .focus_visible(move |style| style.bg(colors.bg_hover))
        .tooltip(move |_, cx| {
            crate::icons::tooltip(if enabled { label } else { disabled_hint }, cx)
        })
        .child(icon(
            glyph,
            if enabled {
                colors.text_secondary
            } else {
                colors.text_tertiary
            },
        ))
}

fn artifact_button(
    label: &'static str,
    enabled: bool,
    focus: FocusHandle,
    colors: ThemeColors,
) -> gpui_kit::Div {
    div()
        .track_focus(&focus)
        .flex_shrink_0()
        .px_2()
        .rounded_md()
        .border_1()
        .border_color(colors.border_subtle)
        .text_size(px(Typography::SIDEBAR))
        .when(enabled, |button| {
            button
                .cursor_pointer()
                .text_color(colors.text_primary)
                .hover(move |style| style.bg(colors.bg_hover))
        })
        .when(!enabled, |button| button.text_color(colors.text_tertiary))
        .child(label)
}

fn error_label(code: Option<GitWorkspaceErrorCode>) -> String {
    code.map_or_else(String::new, |code| match code {
        GitWorkspaceErrorCode::GitUnavailable => {
            vega_i18n::t("ui.git_2_40_was_not_found_install_homebrew").to_owned()
        }
        GitWorkspaceErrorCode::GitUnsupported => {
            vega_i18n::t("ui.git_2_40_is_required_upgrade_git_and").to_owned()
        }
        GitWorkspaceErrorCode::GitExecutableChanged => {
            vega_i18n::t("ui.git_changed_while_vega_was_running_restart_vega").to_owned()
        }
        code => vega_i18n::t_args("ui.artifact_unavailable_code", &[("code", code.as_str())]),
    })
}

fn preview_rows(text: &str) -> Vec<String> {
    text.split_inclusive('\n')
        .map(|line| line.strip_suffix('\n').unwrap_or(line).to_owned())
        .collect()
}
