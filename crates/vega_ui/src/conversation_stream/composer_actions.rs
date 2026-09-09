//! Small local composer commands; all durable changes still use thread settings.
use super::*;
use gpui_kit::Focusable;

const MODES: [(&str, ThreadMode); 3] = [
    ("/ask", ThreadMode::Ask),
    ("/plan", ThreadMode::Plan),
    ("/execute", ThreadMode::Execute),
];

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

    fn modes(&self) -> Vec<(&'static str, ThreadMode)> {
        MODES
            .into_iter()
            .filter(|(command, _)| {
                self.slash
                    .as_ref()
                    .is_none_or(|query| command.starts_with(query))
            })
            .collect()
    }

    fn count(&self) -> usize {
        self.modes().len() + usize::from(self.menu)
    }
}

impl ConversationStream {
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
            && MODES.iter().any(|(command, _)| command.starts_with(token))
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
        self.actions.menu = !self.actions.menu;
        self.actions.slash = None;
        self.actions.highlight = 0;
        self.mode_menu_open = false;
        self.permission_menu_open = false;
        self.model_selector_open = false;
        self.close_file_selector_and_cancel(cx);
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
        if self.actions.menu && index == 0 {
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
            return;
        }
        let offset = usize::from(self.actions.menu);
        let Some((_, mode)) = self
            .actions
            .modes()
            .get(index.saturating_sub(offset))
            .copied()
        else {
            return;
        };
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
        let mut labels = Vec::new();
        if self.actions.menu {
            labels.push("项目文件引用");
        }
        if self.actions.visible() {
            labels.extend(self.actions.modes().iter().map(|(label, _)| *label));
        }
        div()
            .id("composer-actions-menu")
            .w_full()
            .flex()
            .flex_col()
            .when(!labels.is_empty(), |menu| {
                menu.p_2()
                    .mb_2()
                    .rounded(px(Layout::MENU_RADIUS))
                    .bg(colors.bg_elevated)
                    .border_1()
                    .border_color(colors.border_subtle)
                    .shadow_sm()
            })
            .children(labels.into_iter().enumerate().map(|(index, label)| {
                let highlighted = self.actions.highlight == index;
                div()
                    .id(("composer-action", index))
                    .h(px(Typography::SIDEBAR_LINE_HEIGHT))
                    .px_2()
                    .flex()
                    .items_center()
                    .rounded_md()
                    .text_size(px(Typography::SIDEBAR))
                    .text_color(if highlighted {
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
                    .child(label)
            }))
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
