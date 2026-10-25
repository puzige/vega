//! Small local composer commands; all durable changes still use thread settings.
use super::*;
use crate::interaction::InteractionExt;
use gpui_kit::{Focusable, TestSupportExt};
use vega_conversation::types::{ComputerUseControl, ControlPhase};

/// Thread-mode `+`-menu rows, in the order the slash parser also accepts.
/// Labels come from [`mode_command`], so the menu and the parser cannot drift.
const MODE_ORDER: [ThreadMode; 3] = [ThreadMode::Ask, ThreadMode::Plan, ThreadMode::Execute];

pub(crate) const PERMISSION_ORDER: [PermissionMode; 4] = [
    PermissionMode::ReadOnly,
    PermissionMode::Confirm,
    PermissionMode::Auto,
    PermissionMode::FullAccess,
];

/// The exact slash command for one run mode. Single source for both the
/// accepted vocabulary and the `+`-menu label.
fn mode_command(mode: ThreadMode) -> &'static str {
    match mode {
        ThreadMode::Ask => "/ask",
        ThreadMode::Plan => "/plan",
        ThreadMode::Execute => "/execute",
    }
}

pub(crate) fn permission_label(mode: PermissionMode) -> &'static str {
    match mode {
        PermissionMode::ReadOnly => vega_i18n::t("ui.read_only"),
        PermissionMode::Confirm => vega_i18n::t("ui.confirm"),
        PermissionMode::Auto => vega_i18n::t("ui.auto"),
        PermissionMode::FullAccess => vega_i18n::t("ui.full_access"),
    }
}

pub(crate) fn permission_description(mode: PermissionMode) -> &'static str {
    match mode {
        PermissionMode::ReadOnly => vega_i18n::t("ui.ask_every_time"),
        PermissionMode::Confirm => vega_i18n::t("ui.ask_only_for_potentially_unsafe_actions"),
        PermissionMode::Auto => vega_i18n::t("ui.approve_automatically_using_the_sandbox"),
        PermissionMode::FullAccess => {
            vega_i18n::t("ui.no_sandbox_dangerous_actions_still_require_confirmation")
        }
    }
}

pub(crate) fn permission_icon(mode: PermissionMode) -> crate::icons::Icon {
    match mode {
        PermissionMode::ReadOnly => crate::icons::Icon::Hand,
        PermissionMode::Confirm => crate::icons::Icon::Shield,
        PermissionMode::Auto | PermissionMode::FullAccess => crate::icons::Icon::Warning,
    }
}

pub(crate) fn permission_is_warning(mode: PermissionMode) -> bool {
    matches!(mode, PermissionMode::Auto | PermissionMode::FullAccess)
}

pub(crate) const PERMISSION_PICKER_TITLE: &str = "ui.how_should_actions_be_approved";

pub(crate) const PERMISSION_PICKER_LEARN_MORE: &str = "ui.learn_more";

/// One selectable `+`-menu row. Rendering, keyboard highlight indices, and
/// click dispatch all consume this one projection, so they cannot drift.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ComposerActionRow {
    Goal,
    Loop,
    AddImages,
    CredentialReview,
    ComputerTakeover,
    /// `@` project-file reference (menu only).
    FileReference,
    /// One `/ask` `/plan` `/execute` thread-mode command.
    Mode(ThreadMode),
    /// One permission-mode choice (menu only).
    Permission(PermissionMode),
}

impl ComposerActionRow {
    fn label(self) -> &'static str {
        match self {
            Self::Goal => vega_i18n::t("ui.ongoing_goal"),
            Self::Loop => vega_i18n::t("ui.recurring_task"),
            Self::AddImages => vega_i18n::t("ui.add_images"),
            Self::CredentialReview => vega_i18n::t("ui.review_credentials"),
            Self::ComputerTakeover => vega_i18n::t("ui.take_over_computer_control"),
            Self::FileReference => vega_i18n::t("ui.reference_project_files"),
            Self::Mode(mode) => mode_command(mode),
            Self::Permission(mode) => permission_label(mode),
        }
    }

    /// Whether this row shows the thread's current authoritative value.
    fn is_selected(self, thread: &Thread) -> bool {
        match self {
            Self::Goal
            | Self::Loop
            | Self::AddImages
            | Self::ComputerTakeover
            | Self::CredentialReview => false,
            Self::FileReference => false,
            Self::Mode(mode) => thread.mode == mode,
            Self::Permission(mode) => thread.permission_mode == mode,
        }
    }
}

#[derive(Clone)]
struct ComposerTakeover {
    control: ComputerUseControl,
    epoch: u64,
    message_id: String,
    control_generation: u64,
}

#[derive(Default)]
pub(crate) struct ComposerActions {
    menu: bool,
    menu_rows: Vec<ComposerActionRow>,
    menu_generation: u64,
    control_generation: u64,
    takeover: Option<ComposerTakeover>,
    slash: Option<String>,
    dismissed: Option<String>,
    highlight: usize,
    pub(crate) pending_mode: Option<(ThreadMode, String, String)>,
    pub(crate) running: bool,
    stopping: bool,
    pub(crate) restore_focus: bool,
    terminal_cancelled: Option<bool>,
}

impl ComposerActions {
    fn close_menu(&mut self) {
        self.menu = false;
        self.menu_rows.clear();
        self.takeover = None;
        self.menu_generation = self.menu_generation.wrapping_add(1);
    }

    pub(super) fn invalidate_computer_takeover(&mut self) {
        self.control_generation = self.control_generation.wrapping_add(1);
    }

    pub(crate) fn visible(&self) -> bool {
        self.menu || self.slash.is_some()
    }

    /// Thread-mode commands matching the current slash query. The `+` menu
    /// itself always offers all three (empty query).
    fn modes(&self) -> Vec<(&'static str, ThreadMode)> {
        MODE_ORDER
            .into_iter()
            .map(|mode| (mode_command(mode), mode))
            .filter(|(command, _)| {
                self.slash
                    .as_ref()
                    .is_none_or(|query| command.starts_with(query))
            })
            .collect()
    }

    /// The exact visible rows, in render order. `highlight` indexes this
    /// vector, so navigation, rendering, and dispatch share one projection.
    ///
    /// The permission group is `+`-menu-only: a slash query keeps the legacy
    /// mode-command filtering and never offers permission rows.
    fn rows(&self, backend: TaskBackend) -> Vec<ComposerActionRow> {
        let mut rows = Vec::new();
        if !self.visible() {
            return rows;
        }
        if self.menu {
            rows.push(ComposerActionRow::FileReference);
        }
        if backend == TaskBackend::Native {
            rows.extend(
                self.modes()
                    .into_iter()
                    .map(|(_, mode)| ComposerActionRow::Mode(mode)),
            );
        }
        if self.menu && backend == TaskBackend::Native {
            rows.extend(
                PERMISSION_ORDER
                    .into_iter()
                    .map(ComposerActionRow::Permission),
            );
            rows.push(ComposerActionRow::AddImages);
        }
        rows
    }
}

impl ConversationStream {
    pub(crate) fn close_composer_popovers(&mut self, cx: &mut Context<Self>) {
        self.pending_ui.menu = None;
        self.context_control.open = false;
        self.context_usage_open = false;
        self.context_usage_dismissed = true;
        self.context_usage_trigger_hovered = false;
        self.context_usage_tooltip_hovered = false;
        self.actions.close_menu();
        self.actions.slash = None;
        self.model_picker_level = ModelPickerLevel::Closed;
        self.permission_picker_open = false;
        self.utility_projects_open = false;
        self.backend_selector_open = false;
        self.close_file_selector_and_cancel(cx);
    }

    /// Projects worker ownership, including preparation before a durable message exists.
    pub fn begin_composer_run(&mut self, cx: &mut Context<Self>) {
        self.invalidate_composer_prediction(cx);
        self.reset_live_context_usage();
        self.actions.running = true;
        self.actions.stopping = false;
        self.actions.terminal_cancelled = None;
        self.actions.close_menu();
        self.actions.slash = None;
        self.controller_runtime_error = None;
        self.controller_error = None;
        self.mcp_warning = None;
        cx.notify();
    }

    /// Terminal-only release: never allow a replacement run while the old worker drains.
    pub fn finish_composer_run(&mut self, cancelled: bool, cx: &mut Context<Self>) -> bool {
        self.clear_live_context_accounting();
        // A disconnected worker may have no final ConversationEvent. Release only
        // its presentation owner here; durable repair remains the restart path.
        if let Some((message_id, _)) = self.active_agent_message.clone() {
            self.finish_agent_message(&message_id, cx);
        }
        self.timeout_permission(cx);
        self.cancel_user_questions(cx);
        self.actions.running = false;
        self.actions.restore_focus = self.actions.stopping;
        self.actions.stopping = false;
        let stopped = self.actions.terminal_cancelled.unwrap_or(cancelled);
        cx.notify();
        stopped
    }

    pub(super) fn reset_composer_stopping_if_idle(&mut self) {
        if !self.actions.running && !self.composer_submit_pending {
            self.actions.stopping = false;
        }
    }

    pub(crate) fn record_composer_terminal(&mut self, message_id: &str, cancelled: bool) {
        if self
            .active_agent_message
            .as_ref()
            .is_some_and(|(id, _)| id == message_id)
        {
            self.actions.terminal_cancelled = Some(cancelled);
        }
    }

    /// Explicit user cancellation; the app validates the route and cancels its exact worker.
    pub fn request_composer_stop(&mut self, cx: &mut Context<Self>) {
        if (!self.actions.running
            && !self.composer_submit_pending
            && !self.has_cancellable_queue_wait())
            || self.actions.stopping
        {
            return;
        }
        self.actions.stopping = true;
        cx.emit(ComposerStopRequested {
            thread_id: self.thread.id.clone(),
        });
        cx.notify();
    }

    pub(crate) fn stop_composer_action(
        &mut self,
        _: &StopComposer,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.input.read(cx).is_composing() {
            cx.propagate();
            return;
        }
        self.request_composer_stop(cx);
        cx.stop_propagation();
    }

    pub(crate) fn sync_composer_actions(
        &mut self,
        input: &Entity<TextInput>,
        cx: &mut Context<Self>,
    ) {
        if input.read(cx).is_composing() {
            self.actions.close_menu();
            self.actions.slash = None;
            cx.notify();
            return;
        }

        let text = input.read(cx).text();
        let token = text.split_whitespace().next().unwrap_or("");
        let slash = if text.starts_with('/')
            && MODE_ORDER
                .into_iter()
                .any(|mode| mode_command(mode).starts_with(token))
            && self.actions.dismissed.as_deref() != Some(text)
            && !self.actions.menu
            && self.actions.pending_mode.is_none()
        {
            Some(token.to_owned())
        } else {
            None
        };
        if self.actions.slash != slash {
            self.actions.highlight = 0;
            self.actions.slash = slash;
        }
        cx.notify();
    }

    pub(crate) fn open_composer_actions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.input.read(cx).is_composing() {
            return;
        }
        let open = !self.actions.menu;
        self.close_composer_popovers(cx);
        self.actions.menu = open;
        if open {
            self.actions.menu_rows = self.actions.rows(self.thread.backend);
            self.actions.takeover = self.computer_use_control.as_ref().and_then(|control| {
                let status = control.status();
                let (message_id, _) = self.active_agent_message.as_ref()?;
                (status.phase == ControlPhase::Running).then(|| ComposerTakeover {
                    control: control.clone(),
                    epoch: status.epoch,
                    message_id: message_id.clone(),
                    control_generation: self.actions.control_generation,
                })
            });
            if self.actions.takeover.is_some() {
                self.actions
                    .menu_rows
                    .push(ComposerActionRow::ComputerTakeover);
            }
            if self.thread.backend == TaskBackend::Native {
                self.actions
                    .menu_rows
                    .push(ComposerActionRow::CredentialReview);
                self.actions
                    .menu_rows
                    .extend([ComposerActionRow::Goal, ComposerActionRow::Loop]);
            }
        }
        self.actions.highlight = 0;
        self.focus_composer(window, cx);
        cx.notify();
    }

    pub(crate) fn close_composer_actions(
        &mut self,
        _: &CloseComposerActions,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.input.read(cx).is_composing() {
            cx.propagate();
            return;
        }
        self.actions.dismissed = Some(self.input.read(cx).text().to_owned());
        self.actions.close_menu();
        self.actions.slash = None;
        cx.stop_propagation();
        cx.notify();
    }

    pub(super) fn shows_running_send(&self, cx: &App) -> bool {
        self.actions.running
            && self.thread.backend == TaskBackend::Native
            && !self.composer_submit_pending
            && !self.actions.stopping
            && (!self.input.read(cx).text().is_empty() || !self.attachments.is_empty())
    }

    fn move_composer_focus(&self, backwards: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.input.read(cx).is_composing() {
            return;
        }
        let mut controls = vec![
            self.input.read(cx).focus_handle(cx),
            self.action_focus[0].clone(),
        ];
        if self.thread.backend == TaskBackend::Native && !self.entries.is_empty() {
            controls.push(self.context_usage_focus.clone());
        }
        controls.push(self.model_focus.clone());
        if self.pending_queue.paused
            && !self.visible_pending_messages().is_empty()
            && !self.actions.running
            && self.pending_controls_enabled()
        {
            controls.push(self.pending_ui.resume_focus.clone());
        }
        if self.pending_ui.recovered_edit.is_some() && self.pending_draft_is_empty(cx) {
            controls.push(self.pending_ui.recover_focus.clone());
        }
        if self.pending_controls_enabled() {
            for message in self.visible_pending_messages() {
                if let Some(item) = self.pending_ui.items.get(&message.id) {
                    controls.extend(
                        item.images
                            .iter()
                            .take(1)
                            .map(|image| image.view.read(cx).thumbnail_focus()),
                    );
                    if self.actions.running && message.behavior == PendingMessageBehavior::Queue {
                        controls.push(item.focus[0].clone());
                    }
                    controls.extend(item.focus[1..].iter().cloned());
                }
            }
        }
        if self.actions.running || self.composer_submit_pending || self.has_cancellable_queue_wait()
        {
            controls.push(self.action_focus[1].clone());
        }
        controls.extend(self.automation_focuses());
        controls.extend(
            self.active_skill_focuses
                .iter()
                .flat_map(|focuses| focuses.iter().cloned()),
        );
        let next = controls
            .iter()
            .position(|focus| focus.is_focused(window))
            .and_then(|index| {
                if backwards {
                    index.checked_sub(1)
                } else {
                    index.checked_add(1)
                }
            })
            .and_then(|index| controls.get(index));
        if let Some(next) = next {
            next.focus(window, cx);
        } else if backwards {
            window.focus_prev(cx);
        } else {
            window.focus_next(cx);
        }
        cx.stop_propagation();
    }

    pub(crate) fn next_composer_control(
        &mut self,
        _: &NextComposerControl,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_composer_focus(false, window, cx);
    }

    pub(crate) fn previous_composer_control(
        &mut self,
        _: &PreviousComposerControl,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_composer_focus(true, window, cx);
    }

    pub(crate) fn previous_composer_action(
        &mut self,
        _: &PreviousComposerAction,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.input.read(cx).is_composing() {
            cx.propagate();
            return;
        }
        self.actions.highlight = self.actions.highlight.saturating_sub(1);
        cx.stop_propagation();
        cx.notify();
    }

    pub(crate) fn next_composer_action(
        &mut self,
        _: &NextComposerAction,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.input.read(cx).is_composing() {
            cx.propagate();
            return;
        }
        self.actions.highlight =
            (self.actions.highlight + 1).min(self.composer_action_rows().len().saturating_sub(1));
        cx.stop_propagation();
        cx.notify();
    }

    pub(crate) fn accept_composer_action(
        &mut self,
        _: &AcceptComposerAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.input.read(cx).is_composing() {
            cx.propagate();
            return;
        }
        self.select_composer_action(self.actions.highlight, window, cx);
        cx.stop_propagation();
    }

    fn select_composer_action(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.input.read(cx).is_composing() {
            return;
        }
        let Some(row) = self.composer_action_rows().get(index).copied() else {
            return;
        };
        self.select_composer_row(row, window, cx);
    }

    fn composer_takeover_enabled(&self) -> bool {
        self.actions.takeover.as_ref().is_some_and(|takeover| {
            let status = takeover.control.status();
            self.actions.menu
                && self.actions.control_generation == takeover.control_generation
                && self.computer_use_control.is_some()
                && self
                    .active_agent_message
                    .as_ref()
                    .is_some_and(|(id, _)| id == &takeover.message_id)
                && status.phase == ControlPhase::Running
                && status.epoch == takeover.epoch
        })
    }

    fn select_composer_row(
        &mut self,
        row: ComposerActionRow,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.input.read(cx).is_composing() {
            return;
        }
        match row {
            ComposerActionRow::Goal | ComposerActionRow::Loop => {
                let command = if row == ComposerActionRow::Goal {
                    "/goal"
                } else {
                    "/loop"
                };
                let original = self.input.read(cx).text().to_owned();
                self.actions.close_menu();
                self.actions.slash = None;
                self.input.update(cx, |input, cx| {
                    input.set_text(&format!("{command} {original}"), cx)
                });
                self.focus_composer(window, cx);
                cx.notify();
            }
            ComposerActionRow::ComputerTakeover => {
                if !self.composer_takeover_enabled() {
                    self.focus_composer(window, cx);
                    cx.notify();
                    return;
                }
                let Some(takeover) = self.actions.takeover.clone() else {
                    return;
                };
                if takeover.control.pause_at(takeover.epoch).is_ok() {
                    self.actions.close_menu();
                    self.focus_composer(window, cx);
                    cx.notify();
                }
            }
            ComposerActionRow::CredentialReview => {
                self.actions.close_menu();
                self.request_privacy(PrivacyAction::Review, cx);
                self.focus_composer(window, cx);
                cx.notify();
            }
            ComposerActionRow::AddImages => {
                self.actions.close_menu();
                self.pick_images(cx);
            }
            ComposerActionRow::FileReference => self.insert_file_reference_token(window, cx),
            ComposerActionRow::Mode(mode) => self.select_composer_mode(mode, window, cx),
            ComposerActionRow::Permission(mode) => {
                self.actions.close_menu();
                self.actions.slash = None;
                self.request_permission_mode(mode, cx);
                self.focus_composer(window, cx);
                cx.notify();
            }
        }
    }

    /// `@` reference row: close the menu and start the bounded file selector
    /// exactly as the previous positional implementation did.
    fn insert_file_reference_token(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.actions.close_menu();
        let draft = self.input.read(cx).text();
        let separator = if draft.is_empty() || draft.ends_with(char::is_whitespace) {
            ""
        } else {
            " "
        };
        let text = format!("{draft}{separator}@");
        self.input.update(cx, |input, cx| input.set_text(&text, cx));
        self.focus_composer(window, cx);
        let input = self.input.clone();
        self.sync_at_query(&input, cx);
        cx.notify();
    }

    /// Thread-mode row: same guarded request path as the slash command.
    fn select_composer_mode(
        &mut self,
        mode: ThreadMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.model_selection_blocked(cx)
            || self.has_pending_model_selection()
            || self.actions.pending_mode.is_some()
        {
            self.apply_controller_error(cx);
            return;
        }
        if let Some(prefix) = &self.actions.slash {
            let original = self.input.read(cx).text().to_owned();
            let remainder = original[prefix.len()..].to_owned();
            self.actions.pending_mode = Some((mode, original, remainder));
        }
        self.actions.close_menu();
        self.actions.slash = None;
        if self.thread.mode == mode {
            self.acknowledge_mode_action(cx);
        } else {
            self.request_mode(mode, cx);
        }
        self.focus_composer(window, cx);
        cx.notify();
    }

    pub(crate) fn acknowledge_mode_action(&mut self, cx: &mut Context<Self>) {
        if self
            .actions
            .pending_mode
            .as_ref()
            .is_none_or(|(mode, _, _)| *mode != self.thread.mode)
        {
            return;
        }
        if let Some((_, original, remainder)) = self.actions.pending_mode.take()
            && !self.input.read(cx).is_composing()
            && self.input.read(cx).text() == original
        {
            self.input
                .update(cx, |input, cx| input.set_text(&remainder, cx));
        }
    }

    fn composer_action_rows(&self) -> Vec<ComposerActionRow> {
        if self.actions.menu {
            self.actions.menu_rows.clone()
        } else {
            self.actions.rows(self.thread.backend)
        }
    }

    pub(crate) fn render_composer_actions(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let rows = self.composer_action_rows();
        let generation = self.actions.menu_generation;
        let menu = self.actions.menu;
        let mut children = Vec::with_capacity(rows.len() + 1);
        for (index, row) in rows.iter().enumerate() {
            // One separator between the thread-mode commands and the
            // permission group keeps the two vocabularies readable without
            // inventing a second row style.
            if matches!(row, ComposerActionRow::Permission(PermissionMode::ReadOnly)) && index > 0 {
                children.push(
                    div()
                        .h(px(1.))
                        .my_1()
                        .bg(colors.border_subtle)
                        .into_any_element(),
                );
            }
            let highlighted = self.actions.highlight == index;
            let enabled =
                *row != ComposerActionRow::ComputerTakeover || self.composer_takeover_enabled();
            let selected = row.is_selected(&self.thread);
            let label = row.label();
            let row = *row;
            children.push(
                div()
                    .id(format!("composer-action-{generation}-{index}"))
                    .aria_label(label)
                    .h(px(Typography::SIDEBAR_LINE_HEIGHT))
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_1()
                    .rounded_md()
                    .text_size(px(Typography::SIDEBAR))
                    .text_color(if !enabled {
                        colors.text_tertiary
                    } else if matches!(
                        row,
                        ComposerActionRow::Permission(PermissionMode::FullAccess)
                    ) {
                        colors.warning
                    } else if selected || highlighted {
                        colors.brand_primary
                    } else {
                        colors.text_primary
                    })
                    .when(highlighted, |row| row.bg(colors.bg_active))
                    .when(enabled, |row| {
                        row.cursor_pointer()
                            .hover(move |row| row.bg(colors.bg_hover))
                    })
                    .when(!enabled, |row| {
                        row.aria_label(vega_i18n::t("ui.take_over_computer_control_expired"))
                            .tooltip(|_, cx| {
                                crate::icons::tooltip(
                                    vega_i18n::t(
                                        "ui.this_takeover_has_expired_close_and_reopen_the",
                                    ),
                                    cx,
                                )
                            })
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if this.actions.menu_generation == generation
                            && this.actions.menu == menu
                            && this.composer_action_rows().contains(&row)
                        {
                            this.select_composer_row(row, window, cx);
                        }
                    }))
                    // Fixed-width marker column: every label starts on the
                    // same x, with or without a check.
                    .child(
                        div()
                            .when(selected, |marker| marker.child("✓"))
                            .w(px(Typography::SIDEBAR))
                            .flex_shrink_0(),
                    )
                    .when(
                        matches!(
                            row,
                            ComposerActionRow::Permission(PermissionMode::FullAccess)
                        ),
                        |row| {
                            row.child(crate::icons::icon(
                                crate::icons::Icon::Warning,
                                colors.warning,
                            ))
                        },
                    )
                    .child(label)
                    .into_any_element(),
            );
        }
        div()
            .id("composer-actions-menu")
            .w_full()
            .flex()
            .flex_col()
            .when(!children.is_empty(), |menu| {
                menu.p_2()
                    .mb_2()
                    .rounded(px(Layout::MENU_RADIUS))
                    .bg(colors.bg_elevated)
                    .border_1()
                    .border_color(colors.border_subtle)
                    .shadow_sm()
            })
            .children(children)
            .into_any_element()
    }

    pub(crate) fn render_composer_stop(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        div()
            .id("composer-stop")
            .tab_stop(true)
            .aria_label(if self.actions.stopping {
                vega_i18n::t("ui.stopping")
            } else {
                vega_i18n::t("ui.stop")
            })
            .key_context("ComposerStop")
            .on_action(cx.listener(Self::stop_composer_action))
            .size(px(Layout::COMPOSER_SEND_SIZE))
            .flex_shrink_0()
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(colors.accent)
            .text_color(colors.brand_on_accent)
            .when(!self.actions.stopping, |button| button.cursor_pointer())
            .interaction_surface(
                colors.accent,
                colors.brand_primary_strong,
                colors.accent,
                !self.actions.stopping,
            )
            .test_support()
            .track_focus(&self.action_focus[1])
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.request_composer_stop(cx)),
            )
            .child(crate::icons::icon(
                crate::icons::Icon::Close,
                colors.brand_on_accent,
            ))
            .into_any_element()
    }
}
