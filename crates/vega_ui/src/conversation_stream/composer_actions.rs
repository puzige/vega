//! Small local composer commands; all durable changes still use thread settings.
use super::*;
use crate::interaction::InteractionExt;
use gpui_kit::{Focusable, Role};

#[cfg(test)]
#[path = "busy_notice_tests.rs"]
mod busy_notice_tests;

#[derive(Clone, Copy, PartialEq, Eq)]
enum CommandId {
    Ask,
    Plan,
    Execute,
    Compact,
    Goal,
    Loop,
    Fork,
}

#[derive(Clone, Copy)]
enum CommandExecution {
    Mode(ThreadMode),
    Complete,
    Fork,
}

struct ComposerCommand {
    id: CommandId,
    token: &'static str,
    label_key: &'static str,
    aliases: &'static [&'static str],
    default_visible: bool,
    requires_empty_draft: bool,
    execution: CommandExecution,
}

const COMMANDS: [ComposerCommand; 7] = [
    ComposerCommand {
        id: CommandId::Ask,
        token: "/ask",
        label_key: "ui.command_ask",
        aliases: &["询问", "问答"],
        default_visible: true,
        requires_empty_draft: false,
        execution: CommandExecution::Mode(ThreadMode::Ask),
    },
    ComposerCommand {
        id: CommandId::Plan,
        token: "/plan",
        label_key: "ui.command_plan",
        aliases: &["计划"],
        default_visible: true,
        requires_empty_draft: false,
        execution: CommandExecution::Mode(ThreadMode::Plan),
    },
    ComposerCommand {
        id: CommandId::Execute,
        token: "/execute",
        label_key: "ui.command_execute",
        aliases: &["执行"],
        default_visible: true,
        requires_empty_draft: false,
        execution: CommandExecution::Mode(ThreadMode::Execute),
    },
    ComposerCommand {
        id: CommandId::Compact,
        token: "/compact",
        label_key: "ui.command_compact",
        aliases: &["压缩"],
        default_visible: true,
        requires_empty_draft: true,
        execution: CommandExecution::Complete,
    },
    ComposerCommand {
        id: CommandId::Goal,
        token: "/goal",
        label_key: "ui.ongoing_goal",
        aliases: &["目标"],
        default_visible: false,
        requires_empty_draft: false,
        execution: CommandExecution::Complete,
    },
    ComposerCommand {
        id: CommandId::Loop,
        token: "/loop",
        label_key: "ui.recurring_task",
        aliases: &["定时", "周期"],
        default_visible: false,
        requires_empty_draft: false,
        execution: CommandExecution::Complete,
    },
    ComposerCommand {
        id: CommandId::Fork,
        token: "/fork",
        label_key: "ui.command_fork",
        aliases: &["分叉"],
        default_visible: false,
        requires_empty_draft: true,
        execution: CommandExecution::Fork,
    },
];

impl CommandId {
    fn command(self) -> &'static ComposerCommand {
        &COMMANDS[self as usize]
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CommandAvailability {
    Available,
    Disabled(&'static str),
    Hidden,
}

impl ComposerCommand {
    fn matches(&self, query: &str) -> bool {
        let query = query.strip_prefix('/').unwrap_or(query).to_lowercase();
        if query.is_empty() {
            return self.default_visible;
        }
        self.token[1..].starts_with(&query)
            || self.aliases.iter().any(|alias| alias.starts_with(&query))
            || vega_i18n::t(self.label_key)
                .to_lowercase()
                .starts_with(&query)
    }
}

pub(crate) const PERMISSION_ORDER: [PermissionMode; 4] = [
    PermissionMode::ReadOnly,
    PermissionMode::Confirm,
    PermissionMode::Auto,
    PermissionMode::FullAccess,
];

pub(crate) fn permission_label(mode: PermissionMode) -> &'static str {
    match mode {
        PermissionMode::ReadOnly => vega_i18n::t("ui.read_only"),
        PermissionMode::Confirm => vega_i18n::t("ui.permission_ask_for_approval"),
        PermissionMode::Auto => vega_i18n::t("ui.permission_approve_for_me"),
        PermissionMode::FullAccess => vega_i18n::t("ui.full_access"),
    }
}

pub(crate) fn permission_description(mode: PermissionMode) -> &'static str {
    match mode {
        PermissionMode::ReadOnly => vega_i18n::t("ui.permission_description_readonly"),
        PermissionMode::Confirm => vega_i18n::t("ui.permission_description_confirm"),
        PermissionMode::Auto => vega_i18n::t("ui.permission_description_auto"),
        PermissionMode::FullAccess => vega_i18n::t("ui.permission_description_full_access"),
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum ComposerActionRow {
    AddImages,
    FileReference,
    Command(CommandId),
    Skill(usize),
}

impl ComposerActionRow {
    fn label(self) -> &'static str {
        match self {
            Self::AddImages => vega_i18n::t("ui.add_images"),
            Self::FileReference => vega_i18n::t("ui.reference_project_files"),
            Self::Command(id) => vega_i18n::t(id.command().label_key),
            Self::Skill(_) => "",
        }
    }

    fn is_selected(self, thread: &Thread) -> bool {
        matches!(self, Self::Command(id)
            if matches!(id.command().execution, CommandExecution::Mode(mode) if thread.mode == mode))
    }
}

#[derive(Default)]
pub(crate) struct ComposerActions {
    menu: bool,
    menu_rows: Vec<ComposerActionRow>,
    pub(super) menu_generation: u64,
    pub(super) control_generation: u64,
    slash: Option<String>,
    skill_query: Option<(std::ops::Range<usize>, String)>,
    pub(super) skill_projection_requested: bool,
    dismissed: Option<String>,
    highlight: usize,
    scroll: ScrollHandle,
    pub(super) trigger_bounds: std::rc::Rc<std::cell::Cell<Option<gpui_kit::Bounds<Pixels>>>>,
    pub(crate) pending_mode: Option<(ThreadMode, String, String)>,
    pending_mode_revision: u64,
    pub(crate) running: bool,
    stopping: bool,
    pub(crate) restore_focus: bool,
    terminal_cancelled: Option<bool>,
}

impl ComposerActions {
    pub(super) fn apply_busy(controller_error: &mut Option<crate::localized_copy::LocalizedCopy>) {
        controller_error.get_or_insert(crate::localized_copy::LocalizedCopy::AgentBusy);
    }

    fn finish_run(
        &mut self,
        cancelled: bool,
        controller_error: &mut Option<crate::localized_copy::LocalizedCopy>,
    ) -> bool {
        if matches!(
            controller_error,
            Some(crate::localized_copy::LocalizedCopy::AgentBusy)
        ) {
            *controller_error = None;
        }
        self.running = false;
        self.restore_focus = self.stopping;
        self.stopping = false;
        self.terminal_cancelled.unwrap_or(cancelled)
    }

    fn close_menu(&mut self) {
        self.menu = false;
        self.menu_rows.clear();
        self.skill_query = None;
        self.menu_generation = self.menu_generation.wrapping_add(1);
    }

    pub(super) fn invalidate_computer_takeover(&mut self) {
        self.control_generation = self.control_generation.wrapping_add(1);
    }

    pub(crate) fn visible(&self) -> bool {
        self.menu || self.slash.is_some() || self.skill_query.is_some()
    }
}

impl ConversationStream {
    pub fn apply_interrupted_tail(
        &mut self,
        tail: Option<vega_conversation::types::InterruptedAssistant>,
        cx: &mut Context<Self>,
    ) {
        self.interrupted_tail = tail;
        cx.notify();
    }

    pub fn finish_composer_resume_request(&mut self, cx: &mut Context<Self>) {
        self.composer_resume_pending = false;
        cx.notify();
    }

    pub fn can_resume_composer(&self, cx: &App) -> bool {
        !self.composer_resume_pending && self.composer_resume_available(cx)
    }

    pub fn owns_composer_resume_request(
        &self,
        request: &ComposerResumeRequested,
        cx: &App,
    ) -> bool {
        self.composer_resume_pending
            && request.thread_id == self.thread.id
            && self.interrupted_tail.as_ref() == Some(&request.expected)
            && self.composer_resume_available(cx)
    }

    fn composer_resume_available(&self, cx: &App) -> bool {
        self.thread.backend == TaskBackend::Native
            && !self.permission_change_pending()
            && self.interrupted_tail.is_some()
            && !self.reading_preview
            && !self.actions.running
            && !self.actions.stopping
            && !self.composer_submit_pending
            && !self.ordinary_pending_before_run()
            && !self.has_cancellable_queue_wait()
            && self.input.read(cx).text().is_empty()
            && self.attachments.is_empty()
            && !self.attachment_import_pending
            && !self.pending_queue.busy
            && self.pending_queue.messages.is_empty()
            && !self.context_operation_busy()
            && self.actions.pending_mode.is_none()
            && !self.approved_not_started
            && !self.trusted_action_busy
            && !self.skill_mutation_pending
            && self.model_selection_pending.is_none()
            && !self.permission_queue().has_pending()
            && !self.has_pending_user_question()
    }

    pub(crate) fn shows_composer_resume(&self, cx: &App) -> bool {
        self.thread.backend == TaskBackend::Native
            && self.interrupted_tail.is_some()
            && self.input.read(cx).text().is_empty()
            && self.attachments.is_empty()
    }

    pub(crate) fn request_composer_resume(&mut self, cx: &mut Context<Self>) {
        if !self.can_resume_composer(cx) || self.input.read(cx).is_composing() {
            return;
        }
        let Some(expected) = self.interrupted_tail.clone() else {
            return;
        };
        self.close_composer_popovers(cx);
        self.composer_resume_pending = true;
        cx.emit(ComposerResumeRequested {
            thread_id: self.thread.id.clone(),
            expected,
        });
        cx.notify();
    }

    pub(crate) fn activate_composer_primary(&mut self, cx: &mut Context<Self>) {
        if self.shows_composer_resume(cx) {
            self.request_composer_resume(cx);
        } else {
            self.submit_message(cx);
        }
    }

    pub(crate) fn close_composer_popovers(&mut self, cx: &mut Context<Self>) {
        self.pending_ui.menu = None;
        self.context_control.open = false;
        self.context_usage_trigger_hovered = false;
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
        self.bash_clock_cancel_requested = false;
        self.permissions_revoked = false;
        self.permission_editor.active_mode = Some(if self.thread.mode != ThreadMode::Execute {
            PermissionMode::ReadOnly
        } else {
            self.thread.permission_mode
        });
        self.invalidate_composer_prediction(cx);
        self.reset_live_context_usage();
        self.actions.running = true;
        self.actions.stopping = false;
        self.actions.terminal_cancelled = None;
        self.actions.close_menu();
        self.actions.slash = None;
        self.controller_runtime_error = None;
        self.controller_error = None;
        self.sync_retained_thinking_actions(cx);
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
        let finished_entries = self
            .entry_identities
            .iter()
            .enumerate()
            .filter_map(|(index, identity)| {
                identity
                    .message_id
                    .as_ref()
                    .is_some_and(|owner| self.timestamps.running_assistants.contains(owner))
                    .then_some(index)
            })
            .collect::<Vec<_>>();
        self.timestamps.finish_run();
        for index in finished_entries {
            self.invalidate_item(Some(index));
        }
        self.permission_editor.active_mode = None;
        self.composer_resume_pending = false;
        let stopped = self
            .actions
            .finish_run(cancelled, &mut self.controller_error);
        self.sync_retained_thinking_actions(cx);
        cx.notify();
        stopped
    }

    pub(super) fn reset_composer_stopping_if_idle(&mut self) {
        if !self.actions.running
            && !self.composer_submit_pending
            && !self.ordinary_pending_before_run()
        {
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
            && !self.ordinary_pending_before_run()
            && !self.has_cancellable_queue_wait())
            || self.actions.stopping
        {
            return;
        }
        self.actions.stopping = true;
        self.cancel_live_bash_clocks(cx);
        if !self.cancel_ordinary_pending(cx) {
            cx.emit(ComposerStopRequested {
                thread_id: self.thread.id.clone(),
            });
        }
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

        let text = input.read(cx).text().to_owned();
        let skill_query = (self.thread.backend == TaskBackend::Native
            && self.actions.dismissed.as_deref() != Some(text.as_str())
            && !self.actions.menu)
            .then(|| input.read(cx).trailing_skill_query())
            .flatten();
        let token = text.split_whitespace().next().unwrap_or("");
        let slash = if self.thread.backend == TaskBackend::Native
            && text.starts_with('/')
            && skill_query.is_none()
            && !token[1..].contains('/')
            && (text == token
                || COMMANDS.iter().any(|command| {
                    command.matches(token) && matches!(command.execution, CommandExecution::Mode(_))
                }))
            && self.actions.dismissed.as_deref() != Some(text.as_str())
            && !self.actions.menu
            && self.actions.pending_mode.is_none()
        {
            Some(token.to_owned())
        } else {
            None
        };
        if self.actions.slash != slash || self.actions.skill_query != skill_query {
            self.actions.highlight = 0;
            self.actions.scroll.scroll_to_item(0);
            self.actions.menu_generation = self.actions.menu_generation.wrapping_add(1);
            self.actions.slash = slash;
            self.actions.skill_query = skill_query;
        }
        if (self.actions.skill_query.is_some() || self.actions.slash.is_some())
            && self.skill_projection.is_none()
            && !self.actions.skill_projection_requested
        {
            self.actions.skill_projection_requested = true;
            self.request_skill_projection(cx);
        }
        cx.notify();
    }

    pub(crate) fn open_composer_actions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.input.read(cx).is_composing() {
            return;
        }
        if !self.context_menu_available() {
            return;
        }
        let open = !self.actions.menu;
        self.close_composer_popovers(cx);
        self.actions.menu = open;
        if open {
            self.actions.menu_rows.clear();
            if !self.thread.project_id.is_empty() {
                self.actions
                    .menu_rows
                    .push(ComposerActionRow::FileReference);
            }
            if self.thread.backend == TaskBackend::Native {
                self.actions.menu_rows.push(ComposerActionRow::AddImages);
            }
        }
        self.actions.highlight = 0;
        self.actions.scroll.scroll_to_item(0);
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
        let mut controls = vec![self.input.read(cx).focus_handle(cx)];
        if self.context_menu_available() {
            controls.push(self.action_focus[0].clone());
        }
        if self.thread.backend == TaskBackend::Native && !self.permission_change_pending() {
            controls.push(self.permission_editor.trigger.clone());
        }
        if self.thread.backend == TaskBackend::Native && self.context_usage_visible() {
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
        if !self.pending_ui.recovered_edit.is_empty() && self.pending_draft_is_empty(cx) {
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
        controls.push(self.action_focus[1].clone());
        controls.extend(self.automation_focuses());
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
        if !self.composer_actions_visible(cx) {
            cx.propagate();
            return;
        }
        self.actions.highlight = self.actions.highlight.saturating_sub(1);
        self.actions.scroll.scroll_to_item(self.actions.highlight);
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
        if !self.composer_actions_visible(cx) {
            cx.propagate();
            return;
        }
        self.actions.highlight =
            (self.actions.highlight + 1).min(self.composer_action_rows(cx).len().saturating_sub(1));
        self.actions.scroll.scroll_to_item(self.actions.highlight);
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
        if !self.composer_actions_visible(cx) {
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
        let rows = self.composer_action_rows(cx);
        let Some(row) = rows.get(index.min(rows.len().saturating_sub(1))).copied() else {
            return;
        };
        self.select_composer_row(row, window, cx);
    }

    pub(crate) fn context_menu_available(&self) -> bool {
        self.thread.backend == TaskBackend::Native || !self.thread.project_id.is_empty()
    }

    fn composer_fork_request(&self) -> Option<ForkRequested> {
        (0..self.entries.len())
            .rev()
            .find_map(|index| self.fork_request_for_entry(index))
    }

    fn command_availability(&self, command: &ComposerCommand, cx: &App) -> CommandAvailability {
        if self.thread.backend != TaskBackend::Native {
            return CommandAvailability::Hidden;
        }
        if command.id == CommandId::Compact && self.draft_route
            || command.id == CommandId::Fork
                && (self.draft_route || self.composer_fork_request().is_none())
        {
            return CommandAvailability::Hidden;
        }
        if self.model_selection_blocked(cx)
            || self.has_pending_model_selection()
            || self.actions.pending_mode.is_some()
            || self.composer_resume_pending
            || self.permissions_revoked
        {
            return CommandAvailability::Disabled("ui.command_busy");
        }
        if command.requires_empty_draft && !self.attachments.is_empty() {
            return CommandAvailability::Disabled("ui.command_remove_attachments");
        }
        if command.id == CommandId::Compact {
            return self.compact_command_availability(cx).map_or(
                CommandAvailability::Available,
                CommandAvailability::Disabled,
            );
        }
        if command.id == CommandId::Fork && !self.fork_is_idle(cx) {
            return CommandAvailability::Disabled("ui.command_busy");
        }
        CommandAvailability::Available
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
            ComposerActionRow::Command(id) => {
                let command = id.command();
                match self.command_availability(command, cx) {
                    CommandAvailability::Hidden => return,
                    CommandAvailability::Disabled(reason) => {
                        self.controller_error =
                            Some(crate::localized_copy::LocalizedCopy::new(reason));
                        cx.notify();
                        return;
                    }
                    CommandAvailability::Available => {}
                }
                let Some(query) = self.actions.slash.as_deref() else {
                    return;
                };
                let original = self.input.read(cx).text().to_owned();
                if !original.starts_with(query) || !command.matches(query) {
                    return;
                }
                match command.execution {
                    CommandExecution::Mode(mode) => self.select_composer_mode(mode, window, cx),
                    CommandExecution::Complete => {
                        let range = 0..query.len();
                        let completed = format!("{} ", command.token);
                        self.close_composer_popovers(cx);
                        self.input.update(cx, |input, cx| {
                            input.replace_composer_token(range, &completed, cx)
                        });
                        self.focus_composer(window, cx);
                        cx.notify();
                    }
                    CommandExecution::Fork => {
                        if let Some(request) = self.composer_fork_request() {
                            self.close_composer_popovers(cx);
                            self.request_fork(&request, cx);
                            self.focus_composer(window, cx);
                        }
                    }
                }
            }
            ComposerActionRow::Skill(index) => {
                let candidate = self
                    .skill_projection
                    .as_ref()
                    .and_then(|projection| projection.candidates.get(index))
                    .cloned();
                if let Some(candidate) = candidate {
                    self.choose_skill_candidate(&candidate, cx);
                    self.focus_composer(window, cx);
                }
            }
            ComposerActionRow::AddImages => {
                self.actions.close_menu();
                self.pick_images(cx);
            }
            ComposerActionRow::FileReference => self.insert_file_reference_token(window, cx),
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
        let separator = separator.to_owned();
        let end = draft.len();
        let insertion = format!("{separator}@");
        self.input.update(cx, |input, cx| {
            input.replace_composer_token(end..end, &insertion, cx)
        });
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
            self.actions.pending_mode_revision = self.input.read(cx).content_revision();
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
            && self.input.read(cx).content_revision() == self.actions.pending_mode_revision
        {
            let consumed = original.len() - remainder.len();
            self.input.update(cx, |input, cx| {
                input.replace_composer_token(0..consumed, "", cx)
            });
        }
    }

    fn composer_action_rows(&self, cx: &App) -> Vec<ComposerActionRow> {
        if self.actions.menu {
            return self.actions.menu_rows.clone();
        }
        let text = self.input.read(cx).text();
        let mut rows = Vec::new();
        if let Some(query) = self.actions.slash.as_deref() {
            rows.extend(
                COMMANDS
                    .iter()
                    .filter(|command| {
                        command.matches(query)
                            && (text == query
                                || matches!(command.execution, CommandExecution::Mode(_)))
                            && self.command_availability(command, cx) != CommandAvailability::Hidden
                    })
                    .map(|command| ComposerActionRow::Command(command.id)),
            );
        }
        if let Some((_, query)) = self.skill_mention_query(cx) {
            rows.extend(
                self.matching_skill_candidates(&query)
                    .into_iter()
                    .map(ComposerActionRow::Skill),
            );
        }
        rows
    }

    pub(crate) fn composer_actions_visible(&self, cx: &App) -> bool {
        self.actions.visible() && !self.composer_action_rows(cx).is_empty()
    }

    pub(super) fn skill_mention_query(&self, cx: &App) -> Option<(std::ops::Range<usize>, String)> {
        if let Some(query) = &self.actions.skill_query {
            return (self.input.read(cx).trailing_skill_query().as_ref() == Some(query))
                .then(|| query.clone());
        }
        let query = self.actions.slash.as_ref()?;
        (self.input.read(cx).text() == query).then(|| (0..query.len(), query[1..].to_owned()))
    }

    fn matching_skill_candidates(&self, query: &str) -> Vec<usize> {
        let Some(projection) = &self.skill_projection else {
            return Vec::new();
        };
        let query = query.to_lowercase();
        let mut matches: Vec<_> = projection
            .candidates
            .iter()
            .enumerate()
            .filter_map(|(index, candidate)| {
                let name = candidate.name.to_lowercase();
                let source = candidate.source_label.to_lowercase();
                (name.contains(&query) || source.contains(&query)).then_some((
                    !name.starts_with(&query) && !source.starts_with(&query),
                    index,
                ))
            })
            .collect();
        matches.sort_by_key(|item| item.0);
        matches.into_iter().map(|(_, index)| index).collect()
    }

    pub(crate) fn render_composer_actions(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let rows = self.composer_action_rows(cx);
        if !self.actions.visible() || rows.is_empty() {
            return div().into_any_element();
        }
        let generation = self.actions.menu_generation;
        let menu = self.actions.menu;
        let revision = self.input.read(cx).content_revision();
        let projection_generation = self.skill_projection_generation;
        let has_skills = rows
            .iter()
            .any(|row| matches!(row, ComposerActionRow::Skill(_)));
        let mut children = Vec::with_capacity(rows.len() + 1);
        for (index, row) in rows.iter().enumerate() {
            let highlighted = self.actions.highlight.min(rows.len().saturating_sub(1)) == index;
            let reason = match row {
                ComposerActionRow::Command(id) => match self.command_availability(id.command(), cx)
                {
                    CommandAvailability::Disabled(reason) => Some(reason),
                    _ => None,
                },
                _ => None,
            };
            let enabled = reason.is_none();
            let selected = row.is_selected(&self.thread);
            let candidate = match row {
                ComposerActionRow::Skill(index) => self
                    .skill_projection
                    .as_ref()
                    .and_then(|projection| projection.candidates.get(*index)),
                _ => None,
            };
            let label = candidate.map_or_else(|| row.label().to_owned(), |item| item.name.clone());
            let source = candidate.map(|item| {
                let label = vega_i18n::t(match item.scope {
                    vega_conversation::types::SkillUiScope::Project => "ui.skill_source_project",
                    vega_conversation::types::SkillUiScope::VegaGlobal => "ui.skill_source_local",
                    vega_conversation::types::SkillUiScope::Imported => "ui.skill_source_imported",
                });
                let peers: Vec<_> = self
                    .skill_projection
                    .as_ref()
                    .into_iter()
                    .flat_map(|projection| &projection.candidates)
                    .filter(|peer| peer.name == item.name && peer.scope == item.scope)
                    .collect();
                if peers.len() > 1 {
                    let ordinal = peers
                        .iter()
                        .position(|peer| peer.source_id == item.source_id)
                        .map_or(1, |index| index + 1);
                    format!("{label} {ordinal}")
                } else {
                    label.to_owned()
                }
            });
            let accessible_label = candidate.map_or_else(
                || label.clone(),
                |item| {
                    format!(
                        "{}: {} ({})",
                        item.name,
                        item.description,
                        source.as_deref().unwrap_or_default()
                    )
                },
            );
            let row = *row;
            children.push(
                div()
                    .id(format!("composer-action-{generation}-{index}"))
                    .aria_label(reason.map_or_else(
                        || accessible_label.clone(),
                        |reason| format!("{accessible_label}: {}", vega_i18n::t(reason)),
                    ))
                    .when(candidate.is_some(), |row| {
                        row.tooltip(move |_, cx| {
                            crate::icons::tooltip(accessible_label.clone(), cx)
                        })
                    })
                    .min_h(px(Typography::SIDEBAR_LINE_HEIGHT))
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded(px(Layout::MENU_RADIUS))
                    .text_size(px(Typography::SIDEBAR))
                    .text_color(if !enabled {
                        colors.text_tertiary
                    } else if selected && candidate.is_none() {
                        colors.brand_primary
                    } else {
                        colors.text_primary
                    })
                    .when(highlighted, |row| row.bg(colors.bg_active))
                    .when(enabled, |row| {
                        row.cursor_pointer()
                            .hover(move |row| row.bg(colors.bg_hover))
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if this.actions.menu_generation == generation
                            && this.actions.menu == menu
                            && this.input.read(cx).content_revision() == revision
                            && this.skill_projection_generation == projection_generation
                            && this.composer_action_rows(cx).contains(&row)
                        {
                            this.select_composer_row(row, window, cx);
                        }
                    }))
                    .child(
                        div()
                            .when(candidate.is_some(), |marker| {
                                marker.child(crate::icons::icon(
                                    crate::icons::Icon::Blocks,
                                    colors.text_secondary,
                                ))
                            })
                            .when(candidate.is_none() && selected, |marker| marker.child("✓"))
                            .w(px(Typography::SIDEBAR))
                            .flex_shrink_0(),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .when(candidate.is_some(), |item| {
                                item.flex().flex_1().items_center().gap_2()
                            })
                            .when(candidate.is_none(), |item| item.flex().flex_col())
                            .child(
                                div()
                                    .min_w_0()
                                    .max_w_full()
                                    .flex_shrink(1.)
                                    .truncate()
                                    .child(label),
                            )
                            .when_some(candidate, |item, candidate| {
                                item.child(
                                    div()
                                        .min_w_0()
                                        .flex_1()
                                        .truncate()
                                        .text_color(colors.text_secondary)
                                        .child(candidate.description.clone()),
                                )
                            })
                            .when_some(reason, |item, reason| {
                                item.child(
                                    div()
                                        .text_color(colors.text_secondary)
                                        .child(vega_i18n::t(reason)),
                                )
                            }),
                    )
                    .when_some(source, |item, source| {
                        item.child(
                            div()
                                .flex_shrink_0()
                                .text_size(px(Typography::METADATA))
                                .text_color(colors.text_secondary)
                                .truncate()
                                .child(source),
                        )
                    })
                    .when_some(
                        match row {
                            ComposerActionRow::Command(id) => Some(id.command().token),
                            _ => None,
                        },
                        |item, token| {
                            item.child(
                                div()
                                    .ml_auto()
                                    .text_color(colors.text_secondary)
                                    .child(token),
                            )
                        },
                    )
                    .into_any_element(),
            );
        }
        gpui_kit::deferred(
            div()
                .id("composer-actions-menu")
                .absolute()
                .bottom(gpui_kit::relative(1.0))
                .mb_2()
                .left_0()
                .when(has_skills, |menu| menu.w_full())
                .when(!has_skills, |menu| menu.w(px(Layout::MENU_MAX_WIDTH)))
                .max_w_full()
                .occlude()
                .on_mouse_down_out(cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    let on_trigger = this.context_menu_available()
                        && this
                            .actions
                            .trigger_bounds
                            .get()
                            .is_some_and(|bounds| bounds.contains(&event.position));
                    if !on_trigger && this.actions.visible() && !this.input.read(cx).is_composing()
                    {
                        this.actions.dismissed = Some(this.input.read(cx).text().to_owned());
                        this.actions.close_menu();
                        this.actions.slash = None;
                        cx.notify();
                    }
                }))
                .p_2()
                .rounded(px(Layout::MENU_RADIUS))
                .bg(colors.bg_elevated)
                .border_1()
                .border_color(colors.border_subtle)
                .shadow_sm()
                .child(
                    div()
                        .id("composer-actions-scroll")
                        .max_h(px(Layout::COMPOSER_PICKER_MAX_HEIGHT))
                        .overflow_y_scroll()
                        .track_scroll(&self.actions.scroll)
                        .flex()
                        .flex_col()
                        .children(children),
                ),
        )
        .with_priority(2)
        .into_any_element()
    }

    pub(crate) fn render_composer_stop(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        div()
            .id("composer-stop")
            .role(Role::Button)
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
            .border_1()
            .border_color(gpui_kit::transparent_black())
            .focus_visible(move |style| style.border_color(colors.brand_on_accent))
            .when(!self.actions.stopping, |button| button.cursor_pointer())
            .interaction_surface(
                colors.accent,
                colors.brand_primary_strong,
                colors.accent,
                !self.actions.stopping,
            )
            .track_focus(&self.action_focus[1])
            .on_click(cx.listener(|this, _, _, cx| this.request_composer_stop(cx)))
            .child(crate::icons::icon(
                crate::icons::Icon::Close,
                colors.brand_on_accent,
            ))
            .into_any_element()
    }
}
