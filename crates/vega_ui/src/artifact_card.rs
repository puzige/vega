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
                    .child(projection.label),
            );
        if let Some(size) = projection.size_bytes {
            row = row.child(
                div()
                    .flex_shrink_0()
                    .text_size(px(Typography::METADATA))
                    .text_color(colors.text_tertiary)
                    .child(format_size(size)),
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
                .track_focus(&item_focus)
                .tab_stop(true)
                .role(gpui_kit::Role::MenuItem)
                .aria_label(label)
                .text_color(colors.text_primary)
                .child(label)
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
                .child(label)
                .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                    click_group.update(cx, |group, cx| {
                        group.expanded = true;
                        cx.notify();
                    })
                })
                .on_key_down(move |event, window, cx| {
                    if event.keystroke.key == "tab" {
                        if event.keystroke.modifiers.shift {
                            let group = key_group.read(cx);
                            let focus = group
                                .files
                                .iter()
                                .take(group.visible_files())
                                .rev()
                                .find_map(|file| {
                                    let card = file.card.read(cx);
                                    (!card.reading_preview
                                        && card.opening.is_none()
                                        && card.projection.current_file_id.is_some())
                                    .then(|| card.compact_focus[1].clone())
                                });
                            if let Some(focus) = focus {
                                focus.focus(window, cx);
                            } else {
                                window.focus_prev(cx);
                            }
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
                    .child(error_label(Some(error))),
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
) -> crate::interaction::Interaction {
    div()
        .id(label)
        .interactive(colors, enabled, false)
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
        .child(label)
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
