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

use crate::conversation_stream::{MONOFONT, ROW_HEIGHT};
use crate::icons::{Icon, icon};
use crate::interaction::InteractionExt;
use gpui_kit::TestSupportExt;
#[cfg(test)]
use vega_conversation::types::ArtifactSource;

#[cfg(any(test, feature = "artifact-test-observation"))]
use observation::observe_text as artifact_text;
#[cfg(not(any(test, feature = "artifact-test-observation")))]
use std::convert::identity as artifact_text;

#[cfg(any(test, feature = "artifact-test-observation"))]
#[doc(hidden)]
pub mod observation;

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
        self.focus[0].clone()
    }
}

impl Render for ArtifactCard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_compact(window, cx)
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

    fn render_compact(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        self.observe_compact_focus(window, cx);
        let card = cx.entity();
        let colors = theme(cx).colors;
        let projection = self.projection.clone();
        let focus = self.compact_focus.clone();
        let show_actions = self.hovered
            || self.menu_open
            || focus.iter().any(|focus| focus.contains_focused(window, cx));
        let reading = self.reading_preview;
        let opening = self.opening.is_some();
        let menu_open =
            self.menu_open && !reading && !opening && projection.current_file_id.is_some();
        let preview_enabled =
            projection.current_file_id.is_some() && projection.preview_available && !opening;
        let open_enabled = projection.current_file_id.is_some() && !opening;
        let hint = if opening {
            "Opening file in progress"
        } else {
            "File unavailable"
        };
        let hover_card = card.clone();
        let key_card = card.clone();
        let mut row = div()
            .id(("artifact-file", card.entity_id()))
            .when(!reading && !show_actions, |row| {
                row.track_focus(
                    &focus[usize::from(!preview_enabled)]
                        .clone()
                        .tab_stop(preview_enabled || open_enabled),
                )
            })
            .relative()
            .h(px(ROW_HEIGHT))
            .flex_shrink_0()
            .w_full()
            .min_w_0()
            .flex()
            .items_center()
            .px_3()
            .gap_2()
            .hover(move |style| style.bg(colors.bg_hover))
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
                } else if event.keystroke.key == "escape" {
                    key_card.update(cx, |card, cx| {
                        card.close_menu(cx);
                        card.compact_focus[1].focus(window, cx);
                    });
                    cx.stop_propagation();
                }
            })
            .child(icon(Icon::Document, colors.text_secondary))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .font_family(MONOFONT.to_string())
                    .text_size(px(Typography::CODE))
                    .text_color(colors.text_primary)
                    .child(artifact_text(projection.label)),
            );
        if let Some(size) = projection.size_bytes {
            row = row.child(
                div()
                    .flex_shrink_0()
                    .text_size(px(Typography::METADATA))
                    .text_color(colors.text_tertiary)
                    .child(artifact_text(format_size(size))),
            );
        }
        if !reading && show_actions {
            let preview_card = card.clone();
            let preview_key = card.clone();
            let preview = compact_button(
                "Preview file",
                Icon::Document,
                &focus[0],
                preview_enabled,
                hint,
                colors,
            )
            .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                cx.stop_propagation();
                if preview_enabled {
                    preview_card.update(cx, ArtifactCard::preview);
                }
            })
            .on_key_down(move |event, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    cx.stop_propagation();
                    if preview_enabled {
                        preview_key.update(cx, ArtifactCard::preview);
                    }
                }
            });
            let menu_card = card.clone();
            let menu_key = card.clone();
            let trigger = compact_button(
                "Open file in",
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
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
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
            row = row.child(preview).child(trigger);
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
                .on_mouse_down_out(move |_, _, cx| outside.update(cx, ArtifactCard::close_menu));
            for (index, (target, label)) in OPEN_TARGETS.into_iter().enumerate() {
                let open_card = card.clone();
                let key_card = card.clone();
                let item_focus = self.focus[index + 1].clone();
                let item = crate::menu_list::row_container(
                    ("artifact-open-target", index),
                    false,
                    true,
                    colors,
                )
                .test_support()
                .track_focus(&item_focus)
                .tab_stop(true)
                .role(gpui_kit::Role::MenuItem)
                .aria_label(label)
                .text_color(colors.text_primary)
                .child(artifact_text(label))
                .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                    cx.stop_propagation();
                    open_card.update(cx, |card, cx| {
                        card.close_menu(cx);
                        card.open(target, cx);
                    });
                })
                .on_key_down(move |event, window, cx| {
                    let key = event.keystroke.key.as_str();
                    if matches!(key, "enter" | "space" | "up" | "down") {
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
                        .position(gpui_kit::point(px(0.), px(ROW_HEIGHT)))
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
                        .child(artifact_text(projection.label)),
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
                    artifact_button("Preview", preview_enabled, focus[0].clone(), colors)
                        .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                            preview_card.update(cx, ArtifactCard::preview);
                        }),
                );
            for (index, (target, label)) in OPEN_TARGETS.iter().copied().enumerate() {
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
                .child(
                    div()
                        .min_w_0()
                        .w_full()
                        .truncate()
                        .child(artifact_text(line.clone())),
                )
                .into_any_element();
        }
        base.border_b_1()
            .rounded_bl_lg()
            .rounded_br_lg()
            .text_size(px(Typography::SIDEBAR))
            .text_color(colors.danger)
            .child(artifact_text(error_label(inline_error)))
            .into_any_element()
    }
}

const MAX_VISIBLE_ARTIFACTS: usize = 6;

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
}

impl ArtifactGroup {
    pub(crate) fn new(message_id: String) -> Self {
        Self {
            message_id,
            files: Vec::new(),
            capture_order: 0,
            expanded: false,
            capture_errors: Vec::new(),
            file_errors: Vec::new(),
        }
    }

    pub fn message_id(&self) -> &str {
        &self.message_id
    }

    fn visible_files(&self) -> usize {
        if self.expanded {
            self.files.len()
        } else {
            self.files.len().min(MAX_VISIBLE_ARTIFACTS)
        }
    }

    pub fn row_count(&self) -> usize {
        let visible = self.visible_files();
        visible
            + usize::from(visible < self.files.len())
            + self.capture_errors.len()
            + self.file_errors.len()
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

    pub(crate) fn render(group: Entity<Self>, _window: &mut Window, cx: &mut App) -> AnyElement {
        let state = group.read(cx);
        let visible = state.visible_files();
        let overflow = state.files.len() - visible;
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
        let mut container = div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .bg(colors.bg_elevated)
            .border_1()
            .border_color(colors.border_subtle)
            .rounded(px(Layout::TOOL_DETAIL_RADIUS));
        for card in files {
            container = container.child(card);
        }
        if overflow > 0 {
            let label = format!("Show {overflow} more");
            let click_group = group.clone();
            let key_group = group.clone();
            let expand = div()
                .id("artifact-show-more")
                .interactive(colors, true, false)
                .test_support()
                .focusable()
                .tab_stop(true)
                .role(gpui_kit::Role::Button)
                .aria_label(label.clone())
                .h(px(ROW_HEIGHT))
                .px_3()
                .flex()
                .items_center()
                .text_size(px(Typography::SIDEBAR))
                .text_color(colors.text_secondary)
                .child(artifact_text(label))
                .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                    click_group.update(cx, |group, cx| {
                        group.expanded = true;
                        cx.notify();
                    })
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
                        key_group.update(cx, |group, cx| {
                            let visible = group.visible_files();
                            let focus = group
                                .files
                                .iter()
                                .skip(visible)
                                .chain(group.files.iter().take(visible))
                                .find_map(|file| {
                                    let card = file.card.read(cx);
                                    (!card.reading_preview
                                        && card.opening.is_none()
                                        && card.projection.current_file_id.is_some())
                                    .then(|| {
                                        card.compact_focus
                                            [usize::from(!card.projection.preview_available)]
                                        .clone()
                                    })
                                });
                            group.expanded = true;
                            if let Some(focus) = focus {
                                focus.focus(window, cx);
                            } else {
                                window.focus_next(cx);
                            }
                            cx.notify();
                        });
                    }
                });
            container = container.child(expand);
        }
        for error in errors {
            container = container.child(
                div()
                    .h(px(ROW_HEIGHT))
                    .flex_shrink_0()
                    .px_3()
                    .flex()
                    .items_center()
                    .text_size(px(Typography::SIDEBAR))
                    .text_color(colors.danger)
                    .child(artifact_text(error_label(Some(error)))),
            );
        }
        container.into_any_element()
    }
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
    label: &'static str,
    glyph: Icon,
    focus: &FocusHandle,
    enabled: bool,
    disabled_hint: &'static str,
    colors: ThemeColors,
) -> gpui_kit::base::ObservedElement<crate::interaction::Interaction> {
    div()
        .id(label)
        .interactive(colors, enabled, false)
        .test_support()
        .track_focus(&focus.clone().tab_stop(enabled))
        .tab_stop(enabled)
        .role(gpui_kit::Role::Button)
        .aria_label(label)
        .size(px(ROW_HEIGHT))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
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
        .child(artifact_text(label))
}

fn error_label(code: Option<GitWorkspaceErrorCode>) -> String {
    code.map_or_else(String::new, |code| match code {
        GitWorkspaceErrorCode::GitUnavailable => {
            "Git 2.40+ was not found. Install Homebrew Git and retry.".to_owned()
        }
        GitWorkspaceErrorCode::GitUnsupported => {
            "Git 2.40+ is required. Upgrade Git and retry.".to_owned()
        }
        GitWorkspaceErrorCode::GitExecutableChanged => {
            "Git changed while Vega was running. Restart Vega and retry.".to_owned()
        }
        code => format!("Artifact unavailable ({})", code.as_str()),
    })
}

fn preview_rows(text: &str) -> Vec<String> {
    text.split_inclusive('\n')
        .map(|line| line.strip_suffix('\n').unwrap_or(line).to_owned())
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::observation::*;
    use gpui_kit::test::{ElementSnapshot, TestSupportExt, TestWindowExt};
    use gpui_kit::{
        Bounds, Hsla, InputEvent, Pixels, Render, TestAppContext, WindowBounds, WindowHandle,
        WindowOptions, size,
    };
    use vega_conversation::history::{AssistantStatus, HistoryEntry, HistoryPage};
    use vega_conversation::types::{
        ConversationEvent, ConversationStopReason, PermissionMode, SummaryCost, TaskBackend,
        TaskCostSummary, TaskSummaryOutcome, Thread, ThreadMode, ThreadStatus, ToolCall,
        ToolCallStatus, ToolResult, artifact_test_error_event,
    };
    use vega_theme::{DARK, LIGHT, Layout, Theme};

    use crate::conversation_stream::ConversationStream;

    const TARGETS: [(OpenInTarget, &str); 6] = [
        (OpenInTarget::VisualStudioCode, "VS Code"),
        (OpenInTarget::Cursor, "Cursor"),
        (OpenInTarget::Zed, "Zed"),
        (OpenInTarget::Terminal, "Terminal"),
        (OpenInTarget::DefaultApplication, "Default"),
        (OpenInTarget::RevealInFinder, "Finder"),
    ];

    fn control(window: &Window, label: &str) -> ElementSnapshot {
        let matches: Vec<_> = gpui_kit::base::test_support::snapshots(window)
            .into_iter()
            .filter(|snapshot| snapshot.label() == Some(label))
            .collect();
        assert_eq!(matches.len(), 1, "one AX control named {label}");
        matches[0].clone()
    }

    fn hover_point(window: &mut Window, position: gpui_kit::Point<Pixels>, cx: &mut App) {
        window.dispatch_event(
            gpui_kit::MouseMoveEvent {
                position,
                pressed_button: None,
                modifiers: Default::default(),
            }
            .to_platform_input(),
            cx,
        );
        frame(window, cx);
    }

    fn hover_file(window: &mut Window, filename: &str, cx: &mut App) {
        frame(window, cx);
        let name = painted()
            .into_iter()
            .find(|text| text.text == filename)
            .expect("painted filename");
        hover_point(window, name.bounds.center(), cx);
    }

    fn click_control(window: &mut Window, label: &str, cx: &mut App) {
        let target = control(window, label);
        assert!(target.visible());
        let position = target.bounds().center();
        hover_point(window, position, cx);
        window.dispatch_event(
            gpui_kit::MouseDownEvent {
                button: MouseButton::Left,
                position,
                modifiers: Default::default(),
                click_count: 1,
                first_mouse: false,
            }
            .to_platform_input(),
            cx,
        );
        frame(window, cx);
        window.dispatch_event(
            gpui_kit::MouseUpEvent {
                button: MouseButton::Left,
                position,
                modifiers: Default::default(),
                click_count: 1,
            }
            .to_platform_input(),
            cx,
        );
        frame(window, cx);
    }

    fn no_controls(window: &Window) {
        assert!(
            gpui_kit::base::test_support::snapshots(window)
                .iter()
                .all(|snapshot| {
                    !matches!(snapshot.label(), Some("Preview file" | "Open file in"))
                        || !snapshot.visible()
                })
        );
    }

    fn thread() -> Thread {
        Thread {
            parent_thread_id: None,
            id: "thread".into(),
            backend: TaskBackend::Native,
            project_id: "project".into(),
            title: "Artifact contract".into(),
            mode: ThreadMode::Execute,
            permission_mode: PermissionMode::Confirm,
            model: "synthetic".into(),
            status: ThreadStatus::Active,
            pinned: false,
            unread: false,
            created_at: 0,
            updated_at: 0,
        }
    }

    fn projection(slot: u32, generation: u64, size_bytes: Option<u64>) -> ArtifactProjection {
        ArtifactProjection {
            id: ArtifactCardId::for_test(1, slot + (generation as u32 - 1) * 1000),
            label: format!("file-{slot}.txt"),
            source: ArtifactSource::AgentArtifact,
            current_file_id: Some(WorkspaceFileId::for_test(generation, slot)),
            preview_available: true,
            size_bytes,
        }
    }

    fn card(projection: ArtifactProjection, cx: &mut App) -> Entity<ArtifactCard> {
        cx.new(|cx| ArtifactCard::new("thread".into(), "project".into(), projection, cx))
    }

    #[derive(Clone)]
    struct Harness {
        stream: Entity<ConversationStream>,
        workspace: Rc<RefCell<Option<Entity<ArtifactCard>>>>,
        previews: Rc<RefCell<Vec<ArtifactPreviewRequested>>>,
        opens: Rc<RefCell<Vec<ArtifactOpenRequested>>>,
        subscriptions: Rc<RefCell<Vec<gpui_kit::Subscription>>>,
    }

    impl Render for Harness {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let body = if let Some(card) = self.workspace.borrow().as_ref() {
                gpui_kit::uniform_list("workspace-artifact", card.read(cx).row_count(), {
                    let card = card.clone();
                    cx.processor(move |_, range: std::ops::Range<usize>, window, cx| {
                        range
                            .map(|row| ArtifactCard::render_row(card.clone(), row, window, cx))
                            .collect::<Vec<_>>()
                    })
                })
                .h_full()
                .into_any_element()
            } else {
                self.stream.clone().into_any_element()
            };
            let _ = window;
            div()
                .size_full()
                .flex()
                .flex_col()
                .child(div().id("outside").test_support().h(px(30.)).w_full())
                .child(div().flex_1().min_h_0().w_full().child(body))
        }
    }

    fn setup(cx: &mut TestAppContext, dark: bool) -> WindowHandle<Harness> {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::init(cx);
            cx.set_global(if dark { Theme::dark() } else { Theme::light() });
            cx.set_reduce_motion(true);
        });
        cx.update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        Default::default(),
                        size(px(960.), px(2400.)),
                    ))),
                    ..WindowOptions::default()
                },
                |_, cx| {
                    cx.new(|cx| Harness {
                        stream: cx.new(|cx| ConversationStream::new(thread(), cx)),
                        workspace: Rc::default(),
                        previews: Rc::default(),
                        opens: Rc::default(),
                        subscriptions: Rc::default(),
                    })
                },
            )
        })
        .unwrap()
    }

    fn in_window(
        cx: &mut TestAppContext,
        handle: WindowHandle<Harness>,
        f: impl FnOnce(&mut Harness, &mut Window, &mut App),
    ) {
        cx.update_window(handle.into(), |root, window, cx| {
            let root = root.downcast::<Harness>().unwrap();
            let mut harness = root.read(cx).clone();
            f(&mut harness, window, cx);
        })
        .unwrap();
    }

    fn event(stream: &Entity<ConversationStream>, event: ConversationEvent, cx: &mut App) {
        stream.update(cx, |stream, cx| stream.apply_event(event, cx));
    }

    fn start(stream: &Entity<ConversationStream>, owner: &str, cx: &mut App) {
        event(
            stream,
            ConversationEvent::MessageStarted {
                message_id: owner.into(),
                seq: 1,
            },
            cx,
        );
        event(
            stream,
            ConversationEvent::TextDelta {
                message_id: owner.into(),
                delta: format!("Answer for {owner}.\n"),
            },
            cx,
        );
    }

    fn propose(stream: &Entity<ConversationStream>, call_id: &str, cx: &mut App) {
        event(stream, ConversationEvent::ToolCallProposed {
            call: ToolCall {
                id: call_id.into(),
                tool: "Write".into(),
                input_json: serde_json::json!({
                    "audit_version": "write_edit_v1", "tool": "write", "path": "tool-target.txt", "content_bytes": 1,
                    "fingerprint_v1": "0".repeat(64)
                }).to_string(),
            },
        }, cx);
        event(
            stream,
            ConversationEvent::ToolCallFinished {
                call_id: call_id.into(),
                result: ToolResult {
                    status: ToolCallStatus::Success,
                    output: serde_json::json!({
                        "path": "tool-target.txt", "bytes_written": 1,
                        "checkpoint_ref": "preimage-v1/id-61/id-62/id-63"
                    })
                    .to_string(),
                    reused: false,
                    exit_code: None,
                    duration_ms: None,
                    truncated: None,
                    invalid: None,
                },
            },
            cx,
        );
    }

    fn capture(
        harness: &mut Harness,
        call_id: &str,
        projection: ArtifactProjection,
        cx: &mut App,
    ) -> Entity<ArtifactCard> {
        propose(&harness.stream, call_id, cx);
        let card = card(projection, cx);
        subscribe(harness, &card, cx);
        assert!(
            harness
                .stream
                .update(cx, |stream, cx| stream.apply_artifact_card(
                    call_id,
                    card.clone(),
                    cx
                ))
        );
        card
    }

    fn subscribe(harness: &mut Harness, card: &Entity<ArtifactCard>, cx: &mut App) {
        let previews = harness.previews.clone();
        harness.subscriptions.borrow_mut().push(
            cx.subscribe(card, move |_, event: &ArtifactPreviewRequested, _| {
                previews.borrow_mut().push(event.clone())
            }),
        );
        let opens = harness.opens.clone();
        harness.subscriptions.borrow_mut().push(
            cx.subscribe(card, move |_, event: &ArtifactOpenRequested, _| {
                opens.borrow_mut().push(event.clone())
            }),
        );
    }

    fn finish(
        stream: &Entity<ConversationStream>,
        owner: &str,
        outcome: TaskSummaryOutcome,
        cx: &mut App,
    ) {
        let terminal = match outcome {
            TaskSummaryOutcome::Completed => ConversationEvent::MessageFinished {
                message_id: owner.into(),
                stop_reason: ConversationStopReason::End,
                execution_duration_ms: Some(1),
            },
            TaskSummaryOutcome::Interrupted => ConversationEvent::Interrupted {
                message_id: owner.into(),
                execution_duration_ms: Some(1),
            },
            TaskSummaryOutcome::Failed => artifact_test_error_event(Some(owner.into())),
        };
        event(stream, terminal, cx);
    }

    fn summary(
        stream: &Entity<ConversationStream>,
        owner: &str,
        outcome: TaskSummaryOutcome,
        cx: &mut App,
    ) {
        stream.update(cx, |stream, cx| {
            stream.apply_task_summary(
                TaskCostSummary {
                    message_id: owner.into(),
                    outcome,
                    usage: None,
                    cost: SummaryCost::Unavailable,
                    duration_ms: Some(1),
                    tool_count: 1,
                    cache_hit_percent: None,
                },
                cx,
            )
        });
    }

    fn finish_with_summary(stream: &Entity<ConversationStream>, owner: &str, cx: &mut App) {
        finish(stream, owner, TaskSummaryOutcome::Completed, cx);
        summary(stream, owner, TaskSummaryOutcome::Completed, cx);
    }

    #[gpui_kit::test]
    fn production_text_observation_covers_card_content_actions_and_danger(cx: &mut TestAppContext) {
        for dark in [false, true] {
            let handle = setup(cx, dark);
            in_window(cx, handle, |harness, window, cx| {
                let projection = projection(1, 1, None);
                let card_id = projection.id;
                let file_id = projection.current_file_id.unwrap();
                let card = card(projection, cx);
                card.update(cx, |card, cx| {
                    assert!(card.apply_preview(
                        ArtifactPreviewProjection::for_test(
                            card_id,
                            file_id,
                            "agent artifact\nworkspace change\n".into()
                        ),
                        cx
                    ));
                    card.set_opening(Some(OpenInTarget::Zed), cx);
                    assert!(card.apply_open_error(
                        card_id,
                        OpenInTarget::Zed,
                        GitWorkspaceErrorCode::SpawnFailed,
                        cx
                    ));
                });
                *harness.workspace.borrow_mut() = Some(card);
                frame(window, cx);
                let rendered = painted();
                let body_style = |label: &str| {
                    rendered.iter().any(|text| {
                        text.text == label
                            && text.style.as_ref().is_some_and(|style| {
                                style.font_family.as_ref() == MONOFONT
                                    && style.color == Hsla::from(theme(cx).colors.text_primary)
                            })
                    })
                };
                assert!(
                    body_style("agent artifact") && body_style("workspace change"),
                    "real preview content containing legacy-label strings must be observed independently of source metadata"
                );
                assert!(texts().contains(&"file-1.txt".into()));
                for label in [
                    "Preview", "VS Code", "Cursor", "Zed", "Terminal", "Default", "Finder",
                ] {
                    assert!(
                        texts().contains(&label.into()),
                        "real production action text must be observed: {label}"
                    );
                }
                let error = rendered
                    .iter()
                    .find(|text| text.text == "Artifact unavailable (spawn_failed)")
                    .expect("real production error text");
                assert_eq!(
                    error.style.as_ref().unwrap().color,
                    Hsla::from(theme(cx).colors.danger)
                );
                *harness.workspace.borrow_mut() = None;
                start(&harness.stream, "observation", cx);
                frame(window, cx);
                let assistant = painted()
                    .into_iter()
                    .find(|text| {
                        text.scope == "assistant" && text.text.contains("Answer for observation.")
                    })
                    .expect("real selectable assistant shaping and paint must be observed");
                assert!(
                    items("assistant")
                        .iter()
                        .any(|item| item.painted && assistant.ancestors.contains(&item.token))
                );
            });
        }
    }

    #[gpui_kit::test]
    fn artifact_cards_stay_out_of_the_stream_while_the_run_is_active(cx: &mut TestAppContext) {
        let handle = setup(cx, false);
        in_window(cx, handle, |harness, window, cx| {
            start(&harness.stream, "active", cx);
            let captured = capture(harness, "write", projection(1, 1, Some(914)), cx);
            frame(window, cx);
            assert!(items("artifact-group").is_empty());
            assert!(!texts().iter().any(|text| text == "file-1.txt"));
            assert_eq!(captured.read(cx).row_count(), 2);
            finish_with_summary(&harness.stream, "active", cx);
            frame(window, cx);
            ordered("active", 1);
            assert!(group_texts("active", 1).contains(&"file-1.txt".into()));
        });
    }

    #[gpui_kit::test]
    fn finished_turn_groups_captured_artifacts_after_the_assistant_entry(cx: &mut TestAppContext) {
        for outcome in [
            TaskSummaryOutcome::Completed,
            TaskSummaryOutcome::Interrupted,
            TaskSummaryOutcome::Failed,
        ] {
            let handle = setup(cx, false);
            in_window(cx, handle, |harness, window, cx| {
                start(&harness.stream, "terminal", cx);
                capture(harness, "write", projection(1, 1, None), cx);
                event(
                    &harness.stream,
                    ConversationEvent::TextDelta {
                        message_id: "terminal".into(),
                        delta: "Final answer after tools.\n".into(),
                    },
                    cx,
                );
                finish(&harness.stream, "terminal", outcome, cx);
                summary(&harness.stream, "terminal", outcome, cx);
                finish(&harness.stream, "terminal", outcome, cx);
                summary(&harness.stream, "terminal", outcome, cx);
                frame(window, cx);
                assert_eq!(items("artifact-group").len(), 1);
                ordered("terminal", 1);
                let final_text = painted()
                    .into_iter()
                    .find(|text| {
                        text.scope == "assistant" && text.text.contains("Final answer after tools.")
                    })
                    .expect("painted final assistant");
                assert!(final_text.bounds.bottom() <= group("terminal", 1).bounds.top());
            });
        }
    }

    #[gpui_kit::test]
    fn artifact_group_collapses_repeated_edits_of_one_file(cx: &mut TestAppContext) {
        let handle = setup(cx, false);
        in_window(cx, handle, |harness, window, cx| {
            start(&harness.stream, "dedup", cx);
            let mut capture_ids = Vec::new();
            for generation in 1..=3 {
                let captured = capture(
                    harness,
                    &format!("edit-{generation}"),
                    projection(1, generation, Some(generation * 100)),
                    cx,
                );
                assert!(
                    !capture_ids.contains(&captured.read(cx).id()),
                    "each capture has a distinct card capability"
                );
                capture_ids.push(captured.read(cx).id());
            }
            finish_with_summary(&harness.stream, "dedup", cx);
            frame(window, cx);
            let rendered = group_texts("dedup", 1);
            assert_eq!(
                rendered
                    .iter()
                    .filter(|text| text.as_str() == "file-1.txt")
                    .count(),
                1
            );
            assert!(rendered.contains(&"300 B".into()));
            assert!(!rendered.contains(&"100 B".into()));
            assert!(!rendered.contains(&"200 B".into()));
            hover_file(window, "file-1.txt", cx);
            click_control(window, "Preview file", cx);
        });
        in_window(cx, handle, |harness, window, cx| {
            assert_eq!(
                harness.previews.borrow().as_slice(),
                &[ArtifactPreviewRequested {
                    thread_id: "thread".into(),
                    project_id: "project".into(),
                    card_id: ArtifactCardId::for_test(1, 2001),
                    file_id: WorkspaceFileId::for_test(3, 1),
                }]
            );
            start(&harness.stream, "distinct", cx);
            for slot in 2..=4 {
                capture(
                    harness,
                    &format!("write-{slot}"),
                    projection(slot, 1, None),
                    cx,
                );
            }
            finish_with_summary(&harness.stream, "distinct", cx);
            frame(window, cx);
            group("dedup", 1);
            assert_eq!(
                group_texts("distinct", 3),
                vec!["file-2.txt", "file-3.txt", "file-4.txt"]
            );
            assert_eq!(group_texts("dedup", 1), vec!["file-1.txt", "300 B"]);
        });
    }

    #[gpui_kit::test]
    fn artifact_group_merges_capture_after_reconciled_rename(cx: &mut TestAppContext) {
        let handle = setup(cx, false);
        in_window(cx, handle, |harness, _, cx| {
            start(&harness.stream, "reconciled-rename", cx);
            let mut original = projection(1, 1, Some(100));
            original.label = "reports/a.txt".into();
            let captured = capture(harness, "write-original", original, cx);
            let mut renamed = captured.read(cx).projection().clone();
            renamed.label = "reports/b.txt".into();
            captured.update(cx, |card, cx| {
                assert!(card.apply_metadata(renamed, cx));
            });
        });
        in_window(cx, handle, |harness, window, cx| {
            let mut latest = projection(1, 2, Some(200));
            latest.label = "reports/b.txt".into();
            capture(harness, "edit-renamed", latest, cx);
            finish_with_summary(&harness.stream, "reconciled-rename", cx);
            frame(window, cx);
            assert!(painted().iter().any(|text| {
                text.scope == "assistant" && text.text.contains("Answer for reconciled-rename.")
            }));
            let rendered = group_texts("reconciled-rename", 1);
            assert_eq!(
                rendered
                    .iter()
                    .filter(|text| text.as_str() == "reports/b.txt")
                    .count(),
                1
            );
            assert!(!rendered.contains(&"reports/a.txt".into()));
            assert!(rendered.contains(&"200 B".into()));
            assert!(!rendered.contains(&"100 B".into()));
            hover_file(window, "reports/b.txt", cx);
            click_control(window, "Preview file", cx);
        });
        in_window(cx, handle, |harness, _, _| {
            assert_eq!(
                harness.previews.borrow().as_slice(),
                &[ArtifactPreviewRequested {
                    thread_id: "thread".into(),
                    project_id: "project".into(),
                    card_id: ArtifactCardId::for_test(1, 1001),
                    file_id: WorkspaceFileId::for_test(2, 1),
                }]
            );
        });
    }

    #[gpui_kit::test]
    fn artifact_group_offers_show_more_beyond_the_visible_cap(cx: &mut TestAppContext) {
        let handle = setup(cx, false);
        in_window(cx, handle, |harness, window, cx| {
            start(&harness.stream, "overflow", cx);
            for slot in 1..=7 {
                capture(
                    harness,
                    &format!("write-{slot}"),
                    projection(slot, 1, None),
                    cx,
                );
            }
            finish_with_summary(&harness.stream, "overflow", cx);
            frame(window, cx);
            let rendered = group_texts("overflow", 7);
            for slot in 1..=6 {
                assert!(rendered.contains(&format!("file-{slot}.txt")));
            }
            assert!(!rendered.contains(&"file-7.txt".into()));
            assert!(rendered.contains(&"Show 1 more".into()));
            click_control(window, "Show 1 more", cx);
            frame(window, cx);
            let rendered = group_texts("overflow", 7);
            for slot in 1..=7 {
                assert!(rendered.contains(&format!("file-{slot}.txt")));
            }
            assert!(!rendered.contains(&"Show 1 more".into()));
        });
    }

    #[gpui_kit::test]
    fn artifact_group_omits_source_labels_and_editor_row(cx: &mut TestAppContext) {
        for dark in [false, true] {
            let handle = setup(cx, dark);
            in_window(cx, handle, |harness, window, cx| {
                let colors = if dark { DARK } else { LIGHT };
                start(&harness.stream, "appearance", cx);
                capture(harness, "write", projection(1, 1, Some(914)), cx);
                let mut unknown = projection(2, 1, None);
                unknown.source = ArtifactSource::WorkspaceChange;
                let workspace = capture(harness, "edit", unknown, cx);
                finish_with_summary(&harness.stream, "appearance", cx);
                frame(window, cx);
                let group_item = group("appearance", 2);
                let rendered = group_texts("appearance", 2);
                assert!(
                    rendered.contains(&"file-1.txt".into())
                        && rendered.contains(&"file-2.txt".into()),
                    "production filename observation must work before absence assertions: {rendered:?}"
                );
                for absent in [
                    "agent artifact",
                    "workspace change",
                    "VS Code",
                    "Cursor",
                    "Zed",
                    "Terminal",
                    "Default",
                    "Finder",
                    "0 B",
                    "Unknown size",
                ] {
                    assert!(
                        !rendered.iter().any(|text| text.contains(absent)),
                        "unexpected painted text {absent}: {rendered:?}"
                    );
                }
                assert_eq!(
                    rendered
                        .iter()
                        .filter(|text| text.ends_with(" B"))
                        .map(String::as_str)
                        .collect::<Vec<_>>(),
                    vec!["914 B"]
                );
                let name = group_painted("appearance", 2)
                    .into_iter()
                    .find(|text| text.text == "file-1.txt")
                    .unwrap();
                assert_eq!(name.style.as_ref().unwrap().font_family.as_ref(), MONOFONT);
                assert_eq!(
                    name.style.as_ref().unwrap().font_size,
                    gpui_kit::AbsoluteLength::from(px(Typography::CODE))
                );
                let second = group_painted("appearance", 2)
                    .into_iter()
                    .find(|text| text.text == "file-2.txt")
                    .unwrap();
                assert_eq!(
                    second.bounds.origin.y - name.bounds.origin.y,
                    px(ROW_HEIGHT)
                );
                let metadata = group_painted("appearance", 2)
                    .into_iter()
                    .find(|text| text.text == "914 B")
                    .unwrap();
                assert_eq!(
                    metadata.style.as_ref().unwrap().color,
                    Hsla::from(colors.text_tertiary)
                );
                assert_eq!(
                    metadata.style.as_ref().unwrap().font_size,
                    gpui_kit::AbsoluteLength::from(px(Typography::METADATA))
                );
                no_controls(window);
                let quads = window.painted_quads();
                let scale = window.scale_factor();
                assert!(group_item.bounds.size.width >= px(24.));
                assert!(group_item.bounds.size.height >= px(24.));
                let artifact_bounds = Bounds::from_corners(
                    gpui_kit::point(
                        window.pixel_snap(group_item.bounds.left()),
                        window.pixel_snap(group_item.bounds.top()),
                    ),
                    gpui_kit::point(
                        window.pixel_snap(group_item.bounds.right()),
                        window.pixel_snap(group_item.bounds.bottom()),
                    ),
                )
                .scale(scale);
                let radius = px(Layout::TOOL_DETAIL_RADIUS).scale(scale);
                let has_container_geometry = |quad: &gpui_kit::Quad| {
                    quad.bounds == artifact_bounds
                        && quad.corner_radii.top_left == radius
                        && quad.corner_radii.top_right == radius
                        && quad.corner_radii.bottom_left == radius
                        && quad.corner_radii.bottom_right == radius
                };
                assert!(quads.iter().any(|quad| {
                    has_container_geometry(quad)
                        && quad.background.as_solid() == Some(Hsla::from(colors.bg_elevated))
                }));
                let zero = px(0.).scale(scale);
                assert!(quads.iter().any(|quad| {
                    has_container_geometry(quad)
                        && quad.border_color == Hsla::from(colors.border_subtle)
                        && quad.border_widths.top > zero
                        && quad.border_widths.right > zero
                        && quad.border_widths.bottom > zero
                        && quad.border_widths.left > zero
                }));
                hover_file(window, "file-1.txt", cx);
                assert!(control(window, "Preview file").visible());
                assert!(control(window, "Open file in").visible());
                assert!(window.painted_quads().iter().any(|quad| {
                    quad.bounds.contains(&name.bounds.center().scale(scale))
                        && quad.background.as_solid() == Some(Hsla::from(colors.bg_hover))
                }));
                window.hover("outside", cx);
                *harness.workspace.borrow_mut() = Some(workspace);
                frame(window, cx);
                let rendered = texts();
                assert!(
                    rendered.contains(&"file-2.txt".into()),
                    "workspace text observation must work before absence assertions: {rendered:?}"
                );
                assert!(
                    !rendered
                        .iter()
                        .any(|text| text.contains("workspace change")
                            || text.contains("agent artifact")),
                    "unexpected workspace source label: {rendered:?}"
                );
                for (_, target) in TARGETS {
                    assert!(rendered.contains(&target.into()));
                }
                assert_eq!(
                    harness
                        .workspace
                        .borrow()
                        .as_ref()
                        .unwrap()
                        .read(cx)
                        .row_count(),
                    2
                );
            });
        }
    }

    #[gpui_kit::test]
    fn artifact_group_keeps_failures_visible_after_the_turn_ends(cx: &mut TestAppContext) {
        for dark in [false, true] {
            let handle = setup(cx, dark);
            let expected = Hsla::from(if dark { DARK.danger } else { LIGHT.danger });
            let mut cards = Vec::new();
            in_window(cx, handle, |harness, window, cx| {
                start(&harness.stream, "failure-only", cx);
                propose(&harness.stream, "failed-capture", cx);
                assert!(harness.stream.update(cx, |stream, cx| {
                    stream.apply_artifact_capture_error(
                        "failed-capture",
                        GitWorkspaceErrorCode::GitUnavailable,
                        cx,
                    )
                }));
                frame(window, cx);
                assert!(items("artifact-group").is_empty());
                finish_with_summary(&harness.stream, "failure-only", cx);
                frame(window, cx);
                assert_eq!(
                    group_texts("failure-only", 1),
                    vec!["Git 2.40+ was not found. Install Homebrew Git and retry."]
                );
                assert!(
                    group_painted("failure-only", 1).iter().all(|text| text
                        .style
                        .as_ref()
                        .unwrap()
                        .color
                        == expected)
                );
                no_controls(window);
                start(&harness.stream, "failures", cx);
                for slot in 1..=7 {
                    cards.push(capture(
                        harness,
                        &format!("write-{slot}"),
                        projection(slot, 1, None),
                        cx,
                    ));
                }
                propose(&harness.stream, "capture-error", cx);
                assert!(harness.stream.update(cx, |stream, cx| {
                    stream.apply_artifact_capture_error(
                        "capture-error",
                        GitWorkspaceErrorCode::GitUnavailable,
                        cx,
                    )
                }));
                finish_with_summary(&harness.stream, "failures", cx);
                frame(window, cx);
                group("failures", 8);
                assert_eq!(
                    group_texts("failure-only", 1),
                    vec!["Git 2.40+ was not found. Install Homebrew Git and retry."]
                );
                assert!(group_painted("failures", 8).iter().any(|text| text.text
                    == "Git 2.40+ was not found. Install Homebrew Git and retry."
                    && text.style.as_ref().unwrap().color == expected));
                assert!(!group_texts("failures", 8).contains(&"file-7.txt".into()));
            });
            cx.update(|cx| {
                cards[6].update(cx, |card, cx| {
                    assert!(card.apply_preview_error(
                        ArtifactCardId::for_test(1, 7),
                        WorkspaceFileId::for_test(1, 7),
                        GitWorkspaceErrorCode::ChangedDuringRead,
                        cx
                    ));
                });
            });
            in_window(cx, handle, |_, window, cx| {
                frame(window, cx);
                group("failures", 9);
                assert!(!group_texts("failures", 9).contains(&"file-7.txt".into()));
                assert!(group_painted("failures", 9).iter().any(|text| text.text
                    == "Artifact unavailable (changed_during_read)"
                    && text.style.as_ref().unwrap().color == expected));
            });
            cx.update(|cx| {
                cards[6].update(cx, |card, cx| {
                    card.set_opening(Some(OpenInTarget::Zed), cx);
                    assert!(card.apply_open_error(
                        ArtifactCardId::for_test(1, 7),
                        OpenInTarget::Zed,
                        GitWorkspaceErrorCode::SpawnFailed,
                        cx
                    ));
                });
            });
            in_window(cx, handle, |_, window, cx| {
                frame(window, cx);
                group("failures", 9);
                assert!(!group_texts("failures", 9).contains(&"file-7.txt".into()));
                assert!(
                    group_painted("failures", 9)
                        .iter()
                        .any(|text| text.text == "Artifact unavailable (spawn_failed)"
                            && text.style.as_ref().unwrap().color == expected)
                );
            });
            cx.update(|cx| {
                cards[6].update(cx, |card, cx| {
                    card.set_opening(Some(OpenInTarget::Zed), cx);
                    assert!(card.apply_open_outcome(
                        OpenInOutcome {
                            card_id: ArtifactCardId::for_test(1, 7),
                            target: OpenInTarget::Zed,
                        },
                        cx
                    ));
                });
            });
            in_window(cx, handle, |_, window, cx| {
                frame(window, cx);
                group("failures", 8);
                click_control(window, "Show 1 more", cx);
                frame(window, cx);
                group("failures", 8);
                assert!(group_texts("failures", 8).contains(&"file-7.txt".into()));
                assert!(group_painted("failures", 8).iter().any(|text| text.text
                    == "Git 2.40+ was not found. Install Homebrew Git and retry."
                    && text.style.as_ref().unwrap().color == expected));
            });
        }
    }

    #[gpui_kit::test]
    fn artifact_captured_after_the_turn_finishes_joins_that_turn_group(cx: &mut TestAppContext) {
        let handle = setup(cx, false);
        in_window(cx, handle, |harness, window, cx| {
            start(&harness.stream, "old", cx);
            for call in ["first-late", "second-late", "after-reset"] {
                propose(&harness.stream, call, cx);
            }
            finish_with_summary(&harness.stream, "old", cx);
            frame(window, cx);
            assert!(items("artifact-group").is_empty());
            start(&harness.stream, "new", cx);
            let first = card(projection(1, 1, None), cx);
            assert!(
                harness
                    .stream
                    .update(cx, |stream, cx| stream.apply_artifact_card(
                        "first-late",
                        first,
                        cx
                    ))
            );
            frame(window, cx);
            ordered("old", 1);
            assert_eq!(items("artifact-group").len(), 1);
            let second = card(projection(2, 1, None), cx);
            assert!(
                harness
                    .stream
                    .update(cx, |stream, cx| stream.apply_artifact_card(
                        "second-late",
                        second.clone(),
                        cx
                    ))
            );
            assert!(
                harness
                    .stream
                    .update(cx, |stream, cx| stream.apply_artifact_card(
                        "second-late",
                        second,
                        cx
                    ))
            );
            let mut distinct_metadata = projection(3, 1, None);
            distinct_metadata.label = "file-1.txt".into();
            capture(harness, "new-turn-file", distinct_metadata, cx);
            frame(window, cx);
            assert_eq!(group_texts("old", 2), vec!["file-1.txt", "file-2.txt"]);
            assert_eq!(items("artifact-group").len(), 1);
            let unknown = card(projection(99, 1, None), cx);
            assert!(
                !harness
                    .stream
                    .update(cx, |stream, cx| stream.apply_artifact_card(
                        "never-proposed",
                        unknown,
                        cx
                    ))
            );
            assert!(
                !harness
                    .stream
                    .update(cx, |stream, cx| stream.apply_artifact_capture_error(
                        "never-proposed",
                        GitWorkspaceErrorCode::GitUnavailable,
                        cx
                    ))
            );
            finish_with_summary(&harness.stream, "new", cx);
            frame(window, cx);
            assert_eq!(group_texts("old", 2), vec!["file-1.txt", "file-2.txt"]);
            assert_eq!(group_texts("new", 1), vec!["file-1.txt"]);
            ordered("old", 2);
            ordered("new", 1);
            assert_eq!(items("artifact-group").len(), 2);
            let page = HistoryPage {
                subagents: Vec::new(),
                entries: vec![HistoryEntry::AssistantText {
                    seq: 20,
                    message_id: "history".into(),
                    content: "Historical answer".into(),
                    status: AssistantStatus::Done,
                    execution_duration_ms: None,
                }],
                older_cursor: None,
                newer_cursor: None,
                newest_seq: Some(20),
            };
            assert!(harness.stream.update(cx, |stream, cx| {
                stream.replace_history_window(page, "history", cx)
            }));
            start(&harness.stream, "after-history", cx);
            let stale = card(projection(3, 1, None), cx);
            assert!(
                !harness
                    .stream
                    .update(cx, |stream, cx| stream.apply_artifact_card(
                        "after-reset",
                        stale,
                        cx
                    ))
            );
            assert!(
                !harness
                    .stream
                    .update(cx, |stream, cx| stream.apply_artifact_capture_error(
                        "after-reset",
                        GitWorkspaceErrorCode::GitUnavailable,
                        cx
                    ))
            );
            event(&harness.stream, artifact_test_error_event(None), cx);
            frame(window, cx);
            assert!(items("artifact-group").is_empty());
            assert!(!texts().iter().any(|text| text.starts_with("file-")));
        });
    }

    #[gpui_kit::test]
    fn artifact_menu_dispatches_allowlisted_targets_and_closes(cx: &mut TestAppContext) {
        let handle = setup(cx, false);
        in_window(cx, handle, |harness, window, cx| {
            start(&harness.stream, "menu", cx);
            capture(harness, "write", projection(1, 1, None), cx);
            finish_with_summary(&harness.stream, "menu", cx);
            frame(window, cx);
            hover_file(window, "file-1.txt", cx);
            click_control(window, "Open file in", cx);
            frame(window, cx);
            for (_, target) in TARGETS {
                assert!(texts().contains(&target.into()));
            }
            click_control(window, "Open file in", cx);
            frame(window, cx);
            assert!(!texts().contains(&"VS Code".into()));
            click_control(window, "Open file in", cx);
            window.click("outside", cx);
            frame(window, cx);
            assert!(!texts().contains(&"VS Code".into()));
            hover_file(window, "file-1.txt", cx);
            click_control(window, "Open file in", cx);
            window.press("escape", cx);
            frame(window, cx);
            assert!(!texts().contains(&"VS Code".into()));
        });
        for (target, label) in TARGETS {
            let target_handle = setup(cx, false);
            in_window(cx, target_handle, |harness, window, cx| {
                start(&harness.stream, "target", cx);
                capture(harness, "write", projection(1, 1, None), cx);
                finish_with_summary(&harness.stream, "target", cx);
                frame(window, cx);
                hover_file(window, "file-1.txt", cx);
                click_control(window, "Open file in", cx);
                frame(window, cx);
                for (_, text) in TARGETS {
                    assert_eq!(
                        texts()
                            .iter()
                            .filter(|value| value.as_str() == text)
                            .count(),
                        1
                    );
                }
                click_control(window, label, cx);
                frame(window, cx);
                assert!(!texts().contains(&"VS Code".into()));
            });
            in_window(cx, target_handle, |harness, _, _| {
                assert_eq!(
                    harness.opens.borrow().as_slice(),
                    &[ArtifactOpenRequested {
                        thread_id: "thread".into(),
                        project_id: "project".into(),
                        card_id: ArtifactCardId::for_test(1, 1),
                        file_id: WorkspaceFileId::for_test(1, 1),
                        target,
                    }]
                );
                assert!(harness.previews.borrow().is_empty());
            });
        }
    }

    #[gpui_kit::test]
    fn artifact_preview_preserves_workspace_rows_and_request_fences(cx: &mut TestAppContext) {
        let handle = setup(cx, false);
        in_window(cx, handle, |harness, window, cx| {
            start(&harness.stream, "preview", cx);
            capture(harness, "write", projection(1, 1, None), cx);
            finish_with_summary(&harness.stream, "preview", cx);
            frame(window, cx);
            hover_file(window, "file-1.txt", cx);
            click_control(window, "Preview file", cx);
        });
        in_window(cx, handle, |harness, window, cx| {
            assert_eq!(
                harness.previews.borrow().as_slice(),
                &[ArtifactPreviewRequested {
                    thread_id: "thread".into(),
                    project_id: "project".into(),
                    card_id: ArtifactCardId::for_test(1, 1),
                    file_id: WorkspaceFileId::for_test(1, 1),
                }]
            );
            let card = card(projection(1, 1, None), cx);
            assert_eq!(card.read(cx).row_count(), 2);
            card.update(cx, |card, cx| {
                assert!(card.apply_preview(
                    ArtifactPreviewProjection::for_test(
                        ArtifactCardId::for_test(1, 1),
                        WorkspaceFileId::for_test(1, 1),
                        "first line\nsecond line\n".into(),
                    ),
                    cx
                ));
                assert!(!card.apply_preview(
                    ArtifactPreviewProjection::for_test(
                        ArtifactCardId::for_test(1, 1),
                        WorkspaceFileId::for_test(2, 1),
                        "stale body".into(),
                    ),
                    cx
                ));
                assert!(!card.apply_preview_error(
                    ArtifactCardId::for_test(2, 1),
                    WorkspaceFileId::for_test(1, 1),
                    GitWorkspaceErrorCode::UnknownFile,
                    cx
                ));
            });
            assert_eq!(card.read(cx).row_count(), 4);
            *harness.workspace.borrow_mut() = Some(card.clone());
            frame(window, cx);
            assert!(texts().contains(&"first line".into()));
            assert!(texts().contains(&"second line".into()));
            assert!(!texts().contains(&"stale body".into()));
            for (_, target) in TARGETS {
                assert!(texts().contains(&target.into()));
            }
            card.update(cx, |card, cx| {
                card.set_opening(Some(OpenInTarget::Zed), cx);
                assert!(card.apply_open_error(
                    ArtifactCardId::for_test(1, 1),
                    OpenInTarget::Zed,
                    GitWorkspaceErrorCode::SpawnFailed,
                    cx
                ));
            });
            assert_eq!(card.read(cx).row_count(), 5);
            frame(window, cx);
            assert!(texts().contains(&"Artifact unavailable (spawn_failed)".into()));
            card.update(cx, |card, cx| card.set_reading_preview(true, cx));
            assert_eq!(card.read(cx).row_count(), 5);
            frame(window, cx);
            for (_, target) in TARGETS {
                assert!(!texts().contains(&target.into()));
            }
            assert!(texts().contains(&"first line".into()));
        });
    }

    #[gpui_kit::test]
    fn artifact_controls_do_not_request_unavailable_or_busy_files(cx: &mut TestAppContext) {
        for state in ["missing", "opening", "reading"] {
            let handle = setup(cx, false);
            in_window(cx, handle, |harness, window, cx| {
                start(&harness.stream, "blocked", cx);
                let mut metadata = projection(1, 1, None);
                if state == "missing" {
                    metadata.current_file_id = None;
                }
                let card = capture(harness, "write", metadata, cx);
                if state == "opening" {
                    card.update(cx, |card, cx| card.set_opening(Some(OpenInTarget::Zed), cx));
                }
                if state == "reading" {
                    harness
                        .stream
                        .update(cx, |stream, cx| stream.set_reading_preview(true, cx));
                }
                finish_with_summary(&harness.stream, "blocked", cx);
                frame(window, cx);
                hover_file(window, "file-1.txt", cx);
                if state == "reading" {
                    no_controls(window);
                } else {
                    assert_eq!(control(window, "Preview file").disabled(), Some(true));
                    assert_eq!(control(window, "Open file in").disabled(), Some(true));
                    click_control(window, "Preview file", cx);
                    click_control(window, "Open file in", cx);
                }
                window.press("tab", cx);
                window.press("enter", cx);
            });
            if state != "reading" {
                for label in ["Preview file", "Open file in"] {
                    in_window(cx, handle, |_, window, cx| {
                        hover_file(window, "file-1.txt", cx);
                        let target = control(window, label);
                        hover_point(window, target.bounds().center(), cx);
                    });
                    cx.run_until_parked();
                    cx.executor()
                        .advance_clock(std::time::Duration::from_secs(1));
                    cx.run_until_parked();
                    in_window(cx, handle, |_, window, cx| {
                        frame(window, cx);
                        let reasons: Vec<_> = painted()
                            .into_iter()
                            .filter(|text| text.scope == "tooltip")
                            .map(|text| text.text)
                            .collect();
                        assert!(
                            reasons.iter().any(|reason| {
                                let reason = reason.to_lowercase();
                                if state == "missing" {
                                    reason.contains("unavailable")
                                        || reason.contains("no current file")
                                } else {
                                    reason.contains("opening") || reason.contains("in progress")
                                }
                            }),
                            "disabled reason must be painted by the real tooltip: {state}, {label}, {reasons:?}"
                        );
                    });
                }
            }
            in_window(cx, handle, |harness, _, _| {
                assert!(
                    harness.previews.borrow().is_empty(),
                    "preview emitted in {state}"
                );
                assert!(harness.opens.borrow().is_empty(), "open emitted in {state}");
            });
        }
    }

    #[gpui_kit::test]
    fn artifact_actions_are_keyboard_reachable(cx: &mut TestAppContext) {
        let handle = setup(cx, false);
        in_window(cx, handle, |harness, window, cx| {
            start(&harness.stream, "keyboard", cx);
            capture(harness, "write", projection(1, 1, None), cx);
            finish_with_summary(&harness.stream, "keyboard", cx);
            frame(window, cx);
            window.hover("outside", cx);
            no_controls(window);
            harness
                .stream
                .update(cx, |stream, cx| stream.focus_composer(window, cx));
        });
        let mut reached = false;
        for _ in 0..24 {
            in_window(cx, handle, |_, window, cx| window.press("tab", cx));
            in_window(cx, handle, |_, window, cx| {
                frame(window, cx);
                reached = gpui_kit::base::test_support::snapshots(window)
                    .iter()
                    .any(|snapshot| {
                        snapshot.label() == Some("Preview file") && snapshot.focused() == Some(true)
                    });
            });
            if reached {
                break;
            }
        }
        assert!(reached, "Tab must reach preview without pointer hover");
        in_window(cx, handle, |_, window, cx| {
            assert!(control(window, "Preview file").visible());
            assert!(control(window, "Open file in").visible());
            window.press("enter", cx);
        });
        in_window(cx, handle, |harness, window, cx| {
            assert_eq!(harness.previews.borrow().len(), 1);
            window.press("tab", cx);
        });
        in_window(cx, handle, |_, window, cx| {
            frame(window, cx);
            assert_eq!(control(window, "Open file in").focused(), Some(true));
            window.press("enter", cx);
        });
        in_window(cx, handle, |harness, window, cx| {
            frame(window, cx);
            for (_, target) in TARGETS {
                assert!(texts().contains(&target.into()));
            }
            assert!(harness.opens.borrow().is_empty());
        });
    }

    #[gpui_kit::test]
    fn artifact_overflow_keyboard_traverses_and_expands_to_an_available_file_action(
        cx: &mut TestAppContext,
    ) {
        fn focused_action(window: &Window, filename: &str, label: &str) -> Option<ElementSnapshot> {
            let filename = painted()
                .into_iter()
                .find(|text| text.text == filename)
                .expect("painted filename");
            gpui_kit::base::test_support::snapshots(window)
                .into_iter()
                .find(|snapshot| {
                    snapshot.label() == Some(label)
                        && snapshot.focused() == Some(true)
                        && snapshot.visible()
                        && snapshot.bounds().top() <= filename.bounds.center().y
                        && snapshot.bounds().bottom() >= filename.bounds.center().y
                })
        }

        for activation in ["enter", "space"] {
            let handle = setup(cx, false);
            let mut composer_focus = None;
            in_window(cx, handle, |harness, window, cx| {
                start(&harness.stream, "overflow-keyboard", cx);
                for slot in 1..=7 {
                    capture(
                        harness,
                        &format!("write-{slot}"),
                        projection(slot, 1, None),
                        cx,
                    );
                }
                finish_with_summary(&harness.stream, "overflow-keyboard", cx);
                frame(window, cx);
                window.hover("outside", cx);
                no_controls(window);
                harness
                    .stream
                    .update(cx, |stream, cx| stream.focus_composer(window, cx));
                composer_focus = Some(window.focused(cx).expect("composer focus"));
            });
            in_window(cx, handle, |_, window, cx| window.press("tab", cx));
            let mut composer_control = None;
            in_window(cx, handle, |_, window, cx| {
                frame(window, cx);
                composer_control = Some(window.focused(cx).expect("real composer control"));
                assert_ne!(composer_control, composer_focus);
                no_controls(window);
            });
            let mut preceding_focus = None;
            for _ in 0..32 {
                in_window(cx, handle, |_, window, cx| window.press("tab", cx));
                in_window(cx, handle, |_, window, cx| {
                    frame(window, cx);
                    if let Some(action) = focused_action(window, "file-6.txt", "Open file in") {
                        assert_ne!(action.disabled(), Some(true));
                        preceding_focus = Some(window.focused(cx).expect("sixth file action"));
                    }
                });
                if preceding_focus.is_some() {
                    break;
                }
            }
            assert!(
                preceding_focus.is_some(),
                "Tab must reach the sixth file action"
            );
            in_window(cx, handle, |_, window, cx| window.press("tab", cx));
            let mut overflow_focus = None;
            in_window(cx, handle, |_, window, cx| {
                frame(window, cx);
                let rendered = group_texts("overflow-keyboard", 7);
                for slot in 1..=6 {
                    assert!(rendered.contains(&format!("file-{slot}.txt")));
                }
                assert!(!rendered.contains(&"file-7.txt".into()));
                assert!(control(window, "Show 1 more").visible());
                no_controls(window);
                overflow_focus = Some(window.focused(cx).expect("overflow focus after real Tab"));
                assert_ne!(overflow_focus, preceding_focus);
                assert_ne!(overflow_focus, composer_control);
                assert_ne!(overflow_focus, composer_focus);
            });
            in_window(cx, handle, |_, window, cx| window.press("shift-tab", cx));
            in_window(cx, handle, |_, window, cx| {
                frame(window, cx);
                let action = focused_action(window, "file-6.txt", "Open file in")
                    .expect("Shift-Tab returns to the preceding visible file action");
                assert_ne!(action.disabled(), Some(true));
                assert_eq!(window.focused(cx), preceding_focus);
            });
            in_window(cx, handle, |_, window, cx| window.press("tab", cx));
            in_window(cx, handle, |_, window, cx| {
                frame(window, cx);
                assert_eq!(window.focused(cx), overflow_focus);
                no_controls(window);
            });
            in_window(cx, handle, |_, window, cx| window.press("tab", cx));
            in_window(cx, handle, |_, window, cx| {
                frame(window, cx);
                assert_eq!(window.focused(cx), composer_control);
                assert_ne!(window.focused(cx), overflow_focus);
                assert!(control(window, "Show 1 more").visible());
            });
            let mut returned = false;
            for _ in 0..32 {
                in_window(cx, handle, |_, window, cx| window.press("tab", cx));
                in_window(cx, handle, |_, window, cx| {
                    frame(window, cx);
                    returned = window.focused(cx) == overflow_focus;
                });
                if returned {
                    break;
                }
            }
            assert!(
                returned,
                "Tab must return to the still-collapsed overflow control"
            );
            in_window(cx, handle, |_, window, cx| window.press(activation, cx));
            in_window(cx, handle, |harness, window, cx| {
                frame(window, cx);
                let rendered = group_texts("overflow-keyboard", 7);
                for slot in 1..=7 {
                    assert!(
                        rendered.contains(&format!("file-{slot}.txt")),
                        "{activation}"
                    );
                }
                assert!(!rendered.contains(&"Show 1 more".into()));
                assert!(
                    gpui_kit::base::test_support::snapshots(window)
                        .iter()
                        .all(|snapshot| snapshot.label() != Some("Show 1 more"))
                );
                let action = focused_action(window, "file-7.txt", "Preview file")
                    .expect("keyboard expansion focuses the newly exposed available file action");
                assert_ne!(action.disabled(), Some(true));
                assert!(harness.previews.borrow().is_empty());
                assert!(harness.opens.borrow().is_empty());
            });
            in_window(cx, handle, |_, window, cx| window.press("enter", cx));
            in_window(cx, handle, |harness, _, _| {
                assert_eq!(
                    harness.previews.borrow().as_slice(),
                    &[ArtifactPreviewRequested {
                        thread_id: "thread".into(),
                        project_id: "project".into(),
                        card_id: ArtifactCardId::for_test(1, 7),
                        file_id: WorkspaceFileId::for_test(1, 7),
                    }]
                );
                assert!(harness.opens.borrow().is_empty());
            });
            in_window(cx, handle, |_, window, cx| window.press("tab", cx));
            in_window(cx, handle, |_, window, cx| {
                frame(window, cx);
                let action = focused_action(window, "file-7.txt", "Open file in")
                    .expect("Tab continues to the seventh file's other available action");
                assert_ne!(action.disabled(), Some(true));
            });
        }
    }
}
