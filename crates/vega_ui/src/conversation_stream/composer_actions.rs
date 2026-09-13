//! Small local composer commands; all durable changes still use thread settings.
use super::*;
use gpui_kit::Focusable;

/// Thread-mode `+`-menu rows, in the order the slash parser also accepts.
/// Labels come from [`mode_command`], so the menu and the parser cannot drift.
const MODE_ORDER: [ThreadMode; 3] = [ThreadMode::Ask, ThreadMode::Plan, ThreadMode::Execute];

/// Permission-mode `+`-menu rows (R57 P1), in the same order as the
/// bottom-row control they replace. P2b removes that control; this group is
/// what keeps the permission mode reachable afterwards.
const PERMISSION_ORDER: [PermissionMode; 3] = [
    PermissionMode::ReadOnly,
    PermissionMode::Confirm,
    PermissionMode::Auto,
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

/// User-visible permission label, unchanged from the bottom-row control.
fn permission_label(mode: PermissionMode) -> &'static str {
    match mode {
        PermissionMode::ReadOnly => "只读",
        PermissionMode::Confirm => "确认",
        PermissionMode::Auto => "自动",
    }
}

/// One selectable `+`-menu row. Rendering, keyboard highlight indices, and
/// click dispatch all consume this one projection, so they cannot drift.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ComposerActionRow {
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
            Self::FileReference => "项目文件引用",
            Self::Mode(mode) => mode_command(mode),
            Self::Permission(mode) => permission_label(mode),
        }
    }

    /// Whether this row shows the thread's current authoritative value.
    fn is_selected(self, thread: &Thread) -> bool {
        match self {
            Self::FileReference => false,
            Self::Mode(mode) => thread.mode == mode,
            Self::Permission(mode) => thread.permission_mode == mode,
        }
    }

    /// Stable selector for the production click and keyboard tests.
    fn selector(self) -> &'static str {
        match self {
            Self::FileReference => "composer-action-file",
            Self::Mode(ThreadMode::Ask) => "composer-action-mode-ask",
            Self::Mode(ThreadMode::Plan) => "composer-action-mode-plan",
            Self::Mode(ThreadMode::Execute) => "composer-action-mode-execute",
            Self::Permission(PermissionMode::ReadOnly) => "composer-action-permission-readonly",
            Self::Permission(PermissionMode::Confirm) => "composer-action-permission-confirm",
            Self::Permission(PermissionMode::Auto) => "composer-action-permission-auto",
        }
    }

    /// Selector of the selection marker, which exists in the frame only while
    /// this row holds the thread's authoritative value.
    fn check_selector(self) -> &'static str {
        match self {
            Self::FileReference => "composer-action-file-check",
            Self::Mode(ThreadMode::Ask) => "composer-action-mode-ask-check",
            Self::Mode(ThreadMode::Plan) => "composer-action-mode-plan-check",
            Self::Mode(ThreadMode::Execute) => "composer-action-mode-execute-check",
            Self::Permission(PermissionMode::ReadOnly) => {
                "composer-action-permission-readonly-check"
            }
            Self::Permission(PermissionMode::Confirm) => "composer-action-permission-confirm-check",
            Self::Permission(PermissionMode::Auto) => "composer-action-permission-auto-check",
        }
    }
}

#[derive(Default)]
pub(crate) struct ComposerActions {
    menu: bool,
    slash: Option<String>,
    dismissed: Option<String>,
    highlight: usize,
    pub(crate) pending_mode: Option<(ThreadMode, String, String)>,
    pub(crate) running: bool,
    stopping: bool,
    stopped: bool,
    pub(crate) restore_focus: bool,
    terminal_cancelled: Option<bool>,
}

impl ComposerActions {
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
    fn rows(&self) -> Vec<ComposerActionRow> {
        let mut rows = Vec::new();
        if !self.visible() {
            return rows;
        }
        if self.menu {
            rows.push(ComposerActionRow::FileReference);
        }
        rows.extend(
            self.modes()
                .into_iter()
                .map(|(_, mode)| ComposerActionRow::Mode(mode)),
        );
        if self.menu {
            rows.extend(
                PERMISSION_ORDER
                    .into_iter()
                    .map(ComposerActionRow::Permission),
            );
        }
        rows
    }

    fn count(&self) -> usize {
        self.rows().len()
    }
}

impl ConversationStream {
    /// Closes every transient Composer surface before another one opens.
    /// Their controller state stays untouched; this only prevents overlapping
    /// action, file, mode, permission, and model menus.
    pub(crate) fn close_composer_popovers(&mut self, cx: &mut Context<Self>) {
        self.actions.menu = false;
        self.actions.slash = None;
        self.mode_menu_open = false;
        self.permission_menu_open = false;
        self.model_selector_open = false;
        self.close_file_selector_and_cancel(cx);
    }

    /// Projects worker ownership, including preparation before a durable message exists.
    pub fn begin_composer_run(&mut self, cx: &mut Context<Self>) {
        self.actions.running = true;
        self.actions.stopping = false;
        self.actions.stopped = false;
        self.actions.terminal_cancelled = None;
        self.actions.menu = false;
        self.actions.slash = None;
        self.controller_error = None;
        cx.notify();
    }

    /// Terminal-only release: never allow a replacement run while the old worker drains.
    pub fn finish_composer_run(&mut self, cancelled: bool, cx: &mut Context<Self>) -> bool {
        // A disconnected worker may have no final ConversationEvent. Release only
        // its presentation owner here; durable repair remains the restart path.
        if let Some((message_id, _)) = self.active_agent_message.clone() {
            self.finish_agent_message(&message_id, cx);
        }
        self.timeout_permission(cx);
        self.actions.running = false;
        self.actions.restore_focus = self.actions.stopping;
        self.actions.stopping = false;
        self.actions.stopped = self.actions.terminal_cancelled.unwrap_or(cancelled);
        cx.notify();
        self.actions.stopped
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
        if (!self.actions.running && !self.composer_submit_pending) || self.actions.stopping {
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
            self.actions.menu = false;
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
        self.actions.highlight = 0;
        self.focus_composer(window, cx);
        cx.notify();
    }

    pub(crate) fn close_composer_actions(
        &mut self,
        _: &CloseComposerActions,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.input.read(cx).is_composing() {
            cx.propagate();
            return;
        }
        self.actions.dismissed = Some(self.input.read(cx).text().to_owned());
        self.actions.menu = false;
        self.actions.slash = None;
        cx.stop_propagation();
        cx.notify();
    }

    fn move_composer_focus(&self, backwards: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.input.read(cx).is_composing() {
            cx.propagate();
            return;
        }
        let mut controls = vec![
            self.input.read(cx).focus_handle(cx),
            self.action_focus[0].clone(),
            self.compact_focus[0].clone(),
            self.compact_focus[1].clone(),
            self.model_focus.clone(),
            self.setting_focus[6].clone(),
        ];
        if self.actions.running || self.composer_submit_pending {
            controls.push(self.action_focus[1].clone());
        }
        let index = controls
            .iter()
            .position(|focus| focus.is_focused(window))
            .unwrap_or(0);
        let next = (index + if backwards { controls.len() - 1 } else { 1 }) % controls.len();
        controls[next].focus(window, cx);
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
            (self.actions.highlight + 1).min(self.actions.count().saturating_sub(1));
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
        let Some(row) = self.actions.rows().get(index).copied() else {
            return;
        };
        match row {
            ComposerActionRow::FileReference => self.insert_file_reference_token(window, cx),
            ComposerActionRow::Mode(mode) => self.select_composer_mode(mode, window, cx),
            ComposerActionRow::Permission(mode) => {
                self.actions.menu = false;
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
        self.actions.menu = false;
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
        self.actions.menu = false;
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

    pub(crate) fn render_composer_actions(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let rows = self.actions.rows();
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
            let selected = row.is_selected(&self.thread);
            let label = row.label();
            let row = *row;
            children.push(
                div()
                    .id(("composer-action", index))
                    .debug_selector(move || row.selector().to_string())
                    .h(px(Typography::SIDEBAR_LINE_HEIGHT))
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_1()
                    .rounded_md()
                    .text_size(px(Typography::SIDEBAR))
                    .text_color(if selected || highlighted {
                        colors.brand_primary
                    } else {
                        colors.text_primary
                    })
                    .when(highlighted, |row| row.bg(colors.bg_active))
                    .cursor_pointer()
                    .hover(move |row| row.bg(colors.bg_hover))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            this.select_composer_action(index, window, cx);
                        }),
                    )
                    // Fixed-width marker column: every label starts on the
                    // same x, with or without a check.
                    .child(
                        div()
                            .when(selected, |marker| {
                                marker
                                    .debug_selector(move || row.check_selector().to_string())
                                    .child("✓")
                            })
                            .w(px(Typography::SIDEBAR))
                            .flex_shrink_0(),
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
            .debug_selector(|| "composer-stop".into())
            .track_focus(&self.action_focus[1])
            .tab_stop(true)
            .aria_label(if self.actions.stopping {
                "正在停止"
            } else {
                "停止"
            })
            .key_context("ComposerStop")
            .on_action(cx.listener(Self::stop_composer_action))
            .size(px(Layout::COMPOSER_SEND_SIZE))
            .flex_shrink_0()
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(colors.bg_hover)
            .text_color(colors.text_primary)
            .when(!self.actions.stopping, |button| button.cursor_pointer())
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.request_composer_stop(cx)),
            )
            .child(crate::icons::icon(
                crate::icons::Icon::Close,
                colors.text_primary,
            ))
            .into_any_element()
    }

    pub(crate) fn render_composer_run_status(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        div()
            .when(self.actions.stopped, |status| {
                status
                    .debug_selector(|| "composer-stopped".into())
                    .text_size(px(Typography::METADATA))
                    .text_color(colors.text_secondary)
                    .child("已停止")
            })
            .into_any_element()
    }
}
