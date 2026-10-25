use super::attachments::ImagePreview;
use super::*;
use crate::icons::{Icon, icon};
use crate::interaction::InteractionExt;
use crate::scroll_edge_fade::{ScrollEdgeFadeExt, ScrollFade};
use gpui_kit::{
    Anchor, Bounds, Div, Focusable, Global, Role, Stateful, anchored, canvas, deferred, point,
};
use std::cell::Cell;
use std::rc::Rc;
use vega_conversation::types::PendingMessage;

#[derive(Clone, Copy, Default)]
pub struct RunningMessageDefault(pub PendingMessageBehavior);

impl Global for RunningMessageDefault {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PendingQueueAction {
    Remove(String),
    Edit(String),
    Steer(String),
    ToggleDefault,
    Resume,
}

#[derive(Clone)]
pub struct PendingQueueRequested {
    pub thread_id: String,
    pub action: PendingQueueAction,
}

pub(super) struct PendingItemUi {
    pub(super) focus: [FocusHandle; 3],
    pub(super) images: Vec<ImagePreview>,
    bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
}

pub(crate) struct PendingQueueUi {
    pub(super) items: HashMap<String, PendingItemUi>,
    pub(super) menu: Option<String>,
    pub(super) recovered_edit: Option<PendingMessage>,
    error: Option<crate::localized_copy::LocalizedCopy>,
    pub(super) resume_focus: FocusHandle,
    pub(super) recover_focus: FocusHandle,
    wait: Option<(u64, tokio_util::sync::CancellationToken)>,
    menu_focus: [FocusHandle; 2],
    scroll: ScrollHandle,
    restore_focus: bool,
}

impl PendingQueueUi {
    pub(super) fn new(cx: &mut App) -> Self {
        Self {
            items: HashMap::new(),
            menu: None,
            recovered_edit: None,
            error: None,
            resume_focus: cx.focus_handle().tab_stop(true),
            recover_focus: cx.focus_handle().tab_stop(true),
            wait: None,
            menu_focus: std::array::from_fn(|_| cx.focus_handle().tab_stop(true)),
            scroll: ScrollHandle::new(),
            restore_focus: false,
        }
    }
}

#[derive(Clone)]
enum PendingControlAction {
    Queue(PendingQueueAction),
    Menu(String),
    RecoverEdit,
    Submit,
}

struct PendingButton {
    id: String,
    label: &'static str,
    description: Option<&'static str>,
    glyph: Icon,
    focus: FocusHandle,
    enabled: bool,
    selected: bool,
    action: PendingControlAction,
}

const ECHO_TEXT_KIND: &str = "user-text";
const ECHO_IMAGES_KIND: &str = "user-images";

fn echo_entry_identity(thread_id: &str, pending_id: &str, kind: &str) -> StreamEntryIdentity {
    StreamEntryIdentity {
        key: format!(
            "route:{}:{}:pending:{}:{}:kind:{}:{}",
            thread_id.len(),
            thread_id,
            pending_id.len(),
            pending_id,
            kind.len(),
            kind
        ),
        message_id: None,
        pending_id: Some(pending_id.to_string()),
        echo_kind: Some(kind.to_string()),
        sequence: None,
    }
}

impl EventEmitter<PendingQueueRequested> for ConversationStream {}

impl ConversationStream {
    pub fn set_pending_queue_wait(
        &mut self,
        generation: u64,
        cancel: tokio_util::sync::CancellationToken,
        cx: &mut Context<Self>,
    ) {
        self.pending_ui.wait = Some((generation, cancel));
        if !self.actions.running && !self.composer_submit_pending {
            self.actions.stopping = false;
        }
        cx.notify();
    }

    pub fn clear_pending_queue_wait(&mut self, generation: u64, cx: &mut Context<Self>) {
        if self
            .pending_ui
            .wait
            .as_ref()
            .is_some_and(|(current, _)| *current == generation)
        {
            self.pending_ui.wait = None;
            if !self.actions.running && !self.composer_submit_pending {
                self.actions.stopping = false;
            }
            cx.notify();
        }
    }

    pub(super) fn has_cancellable_queue_wait(&self) -> bool {
        self.pending_ui
            .wait
            .as_ref()
            .is_some_and(|(_, cancel)| !cancel.is_cancelled())
    }

    pub fn apply_pending_queue(
        &mut self,
        projection: PendingQueueProjection,
        cx: &mut Context<Self>,
    ) {
        let error = projection.error.as_ref().map(|error| error.clone().into());
        self.apply_pending_queue_localized(projection, error, cx);
    }

    pub fn apply_pending_queue_localized(
        &mut self,
        projection: PendingQueueProjection,
        error: Option<crate::localized_copy::LocalizedCopy>,
        cx: &mut Context<Self>,
    ) {
        self.pending_ui.error = error;
        let committed = projection.committed_message_ids();
        self.reconcile_pending_echoes(&projection, &committed, cx);
        let mut previous = std::mem::take(&mut self.pending_ui.items);
        for message in &projection.messages {
            if committed.contains(&message.id) {
                continue;
            }
            let item = previous
                .remove(&message.id)
                .unwrap_or_else(|| PendingItemUi {
                    focus: std::array::from_fn(|_| cx.focus_handle().tab_stop(true)),
                    images: self.history_image_previews(
                        message.images.iter().take(1).cloned().collect(),
                        cx,
                    ),
                    bounds: Rc::new(Cell::new(None)),
                });
            self.pending_ui.items.insert(message.id.clone(), item);
        }
        if self
            .pending_ui
            .menu
            .as_ref()
            .is_some_and(|id| !self.pending_ui.items.contains_key(id))
        {
            self.pending_ui.menu = None;
            self.pending_ui.restore_focus = true;
        }
        self.pending_queue = projection;
        cx.notify();
    }

    pub fn apply_pending_promotion(
        &mut self,
        pending_id: &str,
        generation: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(message) = self
            .pending_queue
            .messages
            .iter_mut()
            .find(|message| message.id == pending_id)
        else {
            return;
        };
        message.behavior = PendingMessageBehavior::Steer;
        message.generation = generation.to_string();
        let projection = self.pending_queue.clone();
        self.apply_pending_queue_localized(projection, self.pending_ui.error.clone(), cx);
    }

    pub(crate) fn visible_pending_messages(&self) -> Vec<&PendingMessage> {
        self.pending_queue
            .messages
            .iter()
            .filter(|message| self.pending_ui.items.contains_key(&message.id))
            .collect()
    }

    pub fn projected_pending_ids(&self) -> Vec<String> {
        self.pending_echoes
            .iter()
            .map(|echo| echo.pending_id.clone())
            .collect()
    }

    pub fn rail_pending_ids(&self) -> Vec<String> {
        self.visible_pending_messages()
            .iter()
            .map(|message| message.id.clone())
            .collect()
    }

    fn reconcile_pending_echoes(
        &mut self,
        projection: &PendingQueueProjection,
        committed: &[String],
        cx: &mut Context<Self>,
    ) {
        let stale = self
            .pending_echoes
            .iter()
            .map(|echo| echo.pending_id.clone())
            .filter(|pending_id| !committed.contains(pending_id))
            .collect::<Vec<_>>();
        for pending_id in stale {
            self.remove_pending_echo(&pending_id, cx);
        }
        for pending_id in committed {
            if self
                .pending_echoes
                .iter()
                .any(|echo| &echo.pending_id == pending_id)
            {
                continue;
            }
            let Some(message) = projection
                .messages
                .iter()
                .find(|message| &message.id == pending_id)
            else {
                continue;
            };
            self.append_pending_echo(message, cx);
        }
    }

    fn append_pending_echo(&mut self, message: &PendingMessage, cx: &mut Context<Self>) {
        self.ensure_entry_identities();
        let mut indices = Vec::new();
        let block_id = self.user_block_seq;
        self.user_block_seq += 1;
        let copy = MessageCopy::from_live_text(&message.content);
        let content = copy.original_text();
        let index = self.entries.len();
        self.entries.push(StreamEntry::User {
            copy,
            lines: user_message_lines(block_id, &content),
        });
        self.list_append(index);
        let identity = echo_entry_identity(&self.thread.id, &message.id, ECHO_TEXT_KIND);
        self.set_entry_identity(index, identity);
        indices.push(index);
        if !message.images.is_empty() {
            let images = self.history_image_previews(message.images.clone(), cx);
            let index = self.entries.len();
            self.entries.push(StreamEntry::UserImages { images });
            self.list_append(index);
            let identity = echo_entry_identity(&self.thread.id, &message.id, ECHO_IMAGES_KIND);
            self.set_entry_identity(index, identity);
            indices.push(index);
        }
        self.pending_echoes.push(PendingEcho {
            pending_id: message.id.clone(),
            indices,
        });
        cx.notify();
    }

    fn remove_pending_echo(&mut self, pending_id: &str, cx: &mut Context<Self>) {
        let Some(position) = self
            .pending_echoes
            .iter()
            .position(|echo| echo.pending_id == pending_id)
        else {
            return;
        };
        let mut indices = self.pending_echoes.remove(position).indices;
        indices.sort_unstable();
        for index in indices.into_iter().rev() {
            if index >= self.entries.len() {
                continue;
            }
            self.entries.remove(index);
            self.list_remove(index);
            self.shift_entry_indices(index, 1);
        }
        cx.notify();
    }

    fn promote_pending_echo(
        &mut self,
        pending_id: &str,
        message_id: &str,
        content: &str,
        seq: u64,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(position) = self
            .pending_echoes
            .iter()
            .position(|echo| echo.pending_id == pending_id)
        else {
            return false;
        };
        let echo = self.pending_echoes.remove(position);
        let sequence = i64::try_from(seq).ok();
        for index in echo.indices {
            let Some(kind) = self
                .entry_identities
                .get(index)
                .and_then(|identity| identity.echo_kind.clone())
            else {
                continue;
            };
            let identity = self.durable_entry_identity(message_id, &kind, sequence);
            self.set_entry_identity(index, identity);
        }
        if self.composer_history.last().map(String::as_str) != Some(content) {
            self.composer_history.push(content.to_string());
        }
        cx.notify();
        true
    }

    pub fn pending_queue(&self) -> &PendingQueueProjection {
        &self.pending_queue
    }

    pub fn accept_queued_submission(&mut self, content: &str, cx: &mut Context<Self>) {
        if !self.composer_submit_pending {
            return;
        }
        self.composer_submit_pending = false;
        self.pending_behavior_override = None;
        if self.pending_submission_revision.take() == Some(self.input.read(cx).content_revision())
            && self.input.read(cx).text() == content
        {
            self.input.update(cx, TextInput::clear);
        }
        let submitted = std::mem::take(&mut self.submitted_attachments);
        self.attachments
            .retain(|(id, _)| !submitted.iter().any(|(sent, _)| sent == id));
        cx.notify();
    }

    pub fn accept_pending_edit(&mut self, message: PendingMessage, cx: &mut Context<Self>) {
        if message.thread_id != self.thread.id {
            return;
        }
        if self.pending_draft_is_empty(cx) {
            self.restore_pending_draft(message, cx);
        } else {
            self.pending_ui.recovered_edit = Some(message);
        }
        cx.notify();
    }

    fn restore_pending_draft(&mut self, message: PendingMessage, cx: &mut Context<Self>) {
        self.input
            .update(cx, |input, cx| input.set_text(&message.content, cx));
        for image in self.history_image_previews(message.images, cx) {
            self.attachment_generation = self.attachment_generation.saturating_add(1);
            self.attachments.push((self.attachment_generation, image));
        }
        self.pending_behavior_override = Some(message.behavior);
    }

    pub(super) fn pending_draft_is_empty(&self, cx: &App) -> bool {
        self.input.read(cx).text().is_empty()
            && !self.input.read(cx).is_composing()
            && self.attachments.is_empty()
            && !self.attachment_import_pending
            && !self.composer_submit_pending
    }

    pub(crate) fn running_message_behavior(&self, cx: &App) -> PendingMessageBehavior {
        self.pending_behavior_override
            .unwrap_or_else(|| Self::default_running_message_behavior(cx))
    }

    fn default_running_message_behavior(cx: &App) -> PendingMessageBehavior {
        cx.try_global::<RunningMessageDefault>()
            .map(|value| value.0)
            .unwrap_or_default()
    }

    pub(super) fn pending_controls_enabled(&self) -> bool {
        self.thread.backend == TaskBackend::Native
            && !self.pending_queue.busy
            && !self.composer_submit_pending
    }

    fn queue_action(
        &mut self,
        action: PendingQueueAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.pending_controls_enabled() {
            return;
        }
        match &action {
            PendingQueueAction::Edit(id) => {
                if !self.pending_draft_is_empty(cx)
                    || self.pending_ui.recovered_edit.is_some()
                    || !self.pending_ui.items.contains_key(id)
                {
                    return;
                }
            }
            PendingQueueAction::Remove(id) => {
                if !self.pending_ui.items.contains_key(id) {
                    return;
                }
            }
            PendingQueueAction::Steer(id) => {
                if !self.actions.running
                    || !self.pending_queue.messages.iter().any(|message| {
                        &message.id == id && message.behavior == PendingMessageBehavior::Queue
                    })
                {
                    return;
                }
            }
            PendingQueueAction::Resume
                if self.actions.running || self.visible_pending_messages().is_empty() =>
            {
                return;
            }
            PendingQueueAction::Resume | PendingQueueAction::ToggleDefault => {}
        }
        self.close_pending_menu(window, cx);
        self.pending_queue.busy = true;
        self.pending_queue.error = None;
        self.pending_ui.error = None;
        if matches!(
            action,
            PendingQueueAction::Edit(_) | PendingQueueAction::Remove(_)
        ) {
            self.input.read(cx).focus_handle(cx).focus(window, cx);
        }
        cx.emit(PendingQueueRequested {
            thread_id: self.thread.id.clone(),
            action,
        });
        cx.notify();
    }

    fn activate_pending_control(
        &mut self,
        action: &PendingControlAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            PendingControlAction::Queue(action) => self.queue_action(action.clone(), window, cx),
            PendingControlAction::Submit => {
                if self.shows_running_send(cx) && !self.input.read(cx).is_composing() {
                    self.submit_message(cx);
                }
            }
            PendingControlAction::Menu(id) => {
                if self.pending_ui.menu.as_ref() == Some(id) {
                    self.close_pending_menu(window, cx);
                } else if self.pending_controls_enabled() && self.pending_ui.items.contains_key(id)
                {
                    self.close_composer_popovers(cx);
                    self.pending_ui.menu = Some(id.clone());
                    self.pending_ui.menu_focus[0].focus(window, cx);
                    cx.notify();
                }
            }
            PendingControlAction::RecoverEdit => {
                if self.pending_draft_is_empty(cx)
                    && let Some(message) = self.pending_ui.recovered_edit.take()
                {
                    self.restore_pending_draft(message, cx);
                    self.input.read(cx).focus_handle(cx).focus(window, cx);
                    cx.notify();
                }
            }
        }
    }

    fn pending_button(&self, button: PendingButton, cx: &mut Context<Self>) -> Stateful<Div> {
        let colors = theme(cx).colors;
        let PendingButton {
            id,
            label,
            description,
            glyph,
            focus,
            enabled,
            selected,
            action,
        } = button;
        let keyboard_action = action.clone();
        div()
            .id(id)
            .role(Role::Button)
            .aria_label(label)
            .when_some(description, |element, description| {
                element.aria_description(description)
            })
            .key_context("PendingMessageControl")
            .track_focus(&focus)
            .tab_stop(enabled)
            .size(px(Layout::TITLEBAR_CONTROL_SIZE))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .gap_1()
            .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
            .text_size(px(Typography::BODY))
            .text_color(if enabled {
                colors.text_secondary
            } else {
                colors.text_tertiary
            })
            .border_1()
            .border_color(gpui_kit::transparent_black())
            .focus_visible(move |style| style.border_color(colors.accent))
            .when(selected, |element| element.bg(colors.bg_active))
            .when(enabled, |element| {
                element
                    .cursor_pointer()
                    .hover(move |style| style.bg(colors.bg_hover))
            })
            .tooltip(move |_, cx| crate::icons::tooltip(description.unwrap_or(label), cx))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    if enabled {
                        this.activate_pending_control(&action, window, cx);
                    }
                }),
            )
            .on_action(
                cx.listener(move |this, _: &ActivatePendingMessageControl, window, cx| {
                    cx.stop_propagation();
                    if enabled {
                        this.activate_pending_control(&keyboard_action, window, cx);
                    }
                }),
            )
            .child(icon(
                glyph,
                if enabled {
                    colors.text_secondary
                } else {
                    colors.text_tertiary
                },
            ))
    }

    pub(crate) fn render_running_send(&self, can_send: bool, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let enabled = can_send && !self.input.read(cx).is_composing();
        let label = match self.running_message_behavior(cx) {
            PendingMessageBehavior::Queue => vega_i18n::t("ui.queue_message"),
            PendingMessageBehavior::Steer => vega_i18n::t("ui.steer"),
        };
        div()
            .id("composer-running-send")
            .role(Role::Button)
            .aria_label(label)
            .tooltip(move |_, cx| crate::icons::tooltip(label, cx))
            .key_context("PendingMessageControl")
            .track_focus(&self.action_focus[1])
            .tab_stop(true)
            .size(px(Layout::COMPOSER_SEND_SIZE))
            .flex_shrink_0()
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(if enabled {
                colors.accent
            } else {
                colors.bg_hover
            })
            .text_color(if enabled {
                colors.brand_on_accent
            } else {
                colors.text_tertiary
            })
            .when(enabled, |button| button.cursor_pointer())
            .interaction_surface(
                if enabled {
                    colors.accent
                } else {
                    colors.bg_hover
                },
                colors.brand_primary_strong,
                colors.accent,
                enabled,
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    cx.stop_propagation();
                    this.activate_pending_control(&PendingControlAction::Submit, window, cx);
                }),
            )
            .on_action(
                cx.listener(|this, _: &ActivatePendingMessageControl, window, cx| {
                    cx.stop_propagation();
                    this.activate_pending_control(&PendingControlAction::Submit, window, cx);
                }),
            )
            .child(icon(
                Icon::ArrowUp,
                if enabled {
                    colors.brand_on_accent
                } else {
                    colors.text_tertiary
                },
            ))
            .into_any_element()
    }

    pub(crate) fn render_pending_status(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let mut status = div()
            .w_full()
            .max_w(px(Layout::COMPOSER_MAX_WIDTH))
            .mx_auto()
            .flex()
            .flex_col()
            .key_context("Composer")
            .on_action(cx.listener(Self::next_composer_control))
            .on_action(cx.listener(Self::previous_composer_control))
            .gap_1();
        if self.pending_queue.paused && !self.visible_pending_messages().is_empty() {
            let resume = self
                .pending_button(
                    PendingButton {
                        id: "pending-resume".into(),
                        label: vega_i18n::t("ui.continue_queue"),
                        description: None,
                        glyph: Icon::Queue,
                        focus: self.pending_ui.resume_focus.clone(),
                        enabled: !self.actions.running && self.pending_controls_enabled(),
                        selected: false,
                        action: PendingControlAction::Queue(PendingQueueAction::Resume),
                    },
                    cx,
                )
                .w_auto()
                .px_2()
                .child(vega_i18n::t("ui.continue_queue"));
            status = status.child(div().flex().justify_end().mb_1().child(resume));
        }
        if let Some(message) = &self.pending_ui.recovered_edit {
            let can_restore = self.pending_draft_is_empty(cx);
            let recover = self
                .pending_button(
                    PendingButton {
                        id: "pending-recover-edit".into(),
                        label: vega_i18n::t("ui.continue_editing"),
                        description: (!can_restore)
                            .then_some(vega_i18n::t("ui.send_or_clear_the_current_draft_first")),
                        glyph: Icon::Pencil,
                        focus: self.pending_ui.recover_focus.clone(),
                        enabled: can_restore,
                        selected: false,
                        action: PendingControlAction::RecoverEdit,
                    },
                    cx,
                )
                .w_auto()
                .px_2()
                .child(vega_i18n::t("ui.continue_editing"));
            let preview = if message.content.is_empty() {
                vega_i18n::t("ui.image").to_string()
            } else {
                message.content.replace(['\r', '\n'], " ")
            };
            status = status.child(
                div()
                    .mb_1()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(Typography::BODY))
                                    .text_color(colors.text_secondary)
                                    .child(preview),
                            )
                            .child(recover),
                    )
                    .when(!can_restore, |element| {
                        element.child(
                            div()
                                .text_size(px(Typography::METADATA))
                                .text_color(colors.text_secondary)
                                .child(vega_i18n::t("ui.send_or_clear_the_current_draft_first")),
                        )
                    }),
            );
        }
        if let Some(error) = &self.pending_ui.error {
            status = status.child(div().mb_2().child(render_rows::status_notice(
                error.render(),
                colors.danger,
                &colors,
            )));
        }
        status.into_any_element()
    }

    pub(crate) fn render_pending_queue(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let visible = self
            .visible_pending_messages()
            .len()
            .min(Layout::PENDING_MESSAGE_VISIBLE_ROWS) as f32;
        let viewport_height = visible * Layout::PENDING_MESSAGE_ROW_HEIGHT;
        let rows = self
            .pending_queue
            .messages
            .iter()
            .filter_map(|message| {
                let item = self.pending_ui.items.get(&message.id)?;
                let id = message.id.clone();
                let more_bounds = item.bounds.clone();
                let enabled = self.pending_controls_enabled();
                let steer_enabled = enabled
                    && self.actions.running
                    && message.behavior == PendingMessageBehavior::Queue;
                let steer_description = if !self.actions.running {
                    Some(vega_i18n::t("ui.no_task_is_running_continue_the_queue"))
                } else if message.behavior == PendingMessageBehavior::Steer {
                    Some(vega_i18n::t(
                        "ui.this_message_is_already_scheduled_to_steer",
                    ))
                } else {
                    None
                };
                let steer = self
                    .pending_button(
                        PendingButton {
                            id: format!("pending-steer-{id}"),
                            label: vega_i18n::t("ui.steer"),
                            description: steer_description,
                            glyph: Icon::Steer,
                            focus: item.focus[0].clone(),
                            enabled: steer_enabled,
                            selected: false,
                            action: PendingControlAction::Queue(PendingQueueAction::Steer(
                                id.clone(),
                            )),
                        },
                        cx,
                    )
                    .w_auto()
                    .px_1()
                    .child(vega_i18n::t("ui.steer"));
                let remove = self.pending_button(
                    PendingButton {
                        id: format!("pending-remove-{id}"),
                        label: vega_i18n::t("ui.delete_message"),
                        description: None,
                        glyph: Icon::Trash,
                        focus: item.focus[1].clone(),
                        enabled,
                        selected: false,
                        action: PendingControlAction::Queue(PendingQueueAction::Remove(id.clone())),
                    },
                    cx,
                );
                let more = self
                    .pending_button(
                        PendingButton {
                            id: format!("pending-more-{id}"),
                            label: vega_i18n::t("ui.message_options"),
                            description: None,
                            glyph: Icon::More,
                            focus: item.focus[2].clone(),
                            enabled,
                            selected: self.pending_ui.menu.as_ref() == Some(&id),
                            action: PendingControlAction::Menu(id.clone()),
                        },
                        cx,
                    )
                    .relative()
                    .child(
                        canvas(
                            move |bounds, _, _| more_bounds.set(Some(bounds)),
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .inset_0(),
                    );
                let preview = if message.content.is_empty() {
                    vega_i18n::t("ui.image").into()
                } else {
                    message.content.replace(['\r', '\n'], " ")
                };
                Some(
                    div()
                        .h(px(Layout::PENDING_MESSAGE_ROW_HEIGHT))
                        .flex_shrink_0()
                        .px_2()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(icon(
                            if message.behavior == PendingMessageBehavior::Queue {
                                Icon::Queue
                            } else {
                                Icon::Steer
                            },
                            colors.text_tertiary,
                        ))
                        .children(item.images.iter().take(1).map(|image| {
                            div()
                                .size(px(Layout::PENDING_MESSAGE_THUMBNAIL))
                                .flex_shrink_0()
                                .relative()
                                .child(crate::image_preview::ImagePreview::thumbnail_sized(
                                    &image.view,
                                    Layout::PENDING_MESSAGE_THUMBNAIL,
                                    cx,
                                ))
                                .when(message.images.len() > 1, |element| {
                                    element.child(
                                        div()
                                            .absolute()
                                            .bottom_0()
                                            .right_0()
                                            .px_1()
                                            .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
                                            .bg(colors.bg_elevated)
                                            .text_color(colors.text_secondary)
                                            .text_size(px(Typography::METADATA))
                                            .child(format!("+{}", message.images.len() - 1)),
                                    )
                                })
                        }))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .text_size(px(Typography::BODY))
                                .text_color(colors.text_primary)
                                .child(preview),
                        )
                        .child(
                            div()
                                .flex_shrink_0()
                                .flex()
                                .items_center()
                                .gap_1()
                                .child(steer)
                                .child(remove)
                                .child(more),
                        ),
                )
            })
            .collect::<Vec<_>>();
        div()
            .w_full()
            .max_w(px(Layout::COMPOSER_MAX_WIDTH))
            .mx_auto()
            .px(px(Layout::PENDING_MESSAGE_INSET))
            .mb(px(-Layout::PENDING_MESSAGE_OVERLAP))
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(px(viewport_height + Layout::PENDING_MESSAGE_OVERLAP))
                    .rounded(px(Layout::PENDING_MESSAGE_RADIUS))
                    .bg(colors.bg_elevated)
                    .overflow_hidden()
                    .map(|mut rail| {
                        let fade = ScrollFade::compact()
                            .rounded_parent(rail.style())
                            .bottom_extension(px(Layout::PENDING_MESSAGE_OVERLAP));
                        rail.child(
                            div()
                                .id("pending-message-scroll")
                                .h(px(viewport_height))
                                .w_full()
                                .flex()
                                .flex_col()
                                .overflow_y_scroll()
                                .track_scroll(&self.pending_ui.scroll)
                                .key_context("Composer")
                                .on_action(cx.listener(Self::next_composer_control))
                                .on_action(cx.listener(Self::previous_composer_control))
                                .children(rows)
                                .tracked_scroll_edge_fade(
                                    self.pending_ui.scroll.clone(),
                                    colors.bg_elevated,
                                    fade,
                                ),
                        )
                    })
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .rounded(px(Layout::PENDING_MESSAGE_RADIUS))
                            .border_1()
                            .border_color(colors.border_subtle),
                    ),
            )
            .into_any_element()
    }

    fn close_pending_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.pending_ui.menu.take() else {
            return;
        };
        if let Some(item) = self.pending_ui.items.get(&id) {
            item.focus[2].focus(window, cx);
        } else {
            self.input.read(cx).focus_handle(cx).focus(window, cx);
        }
        cx.notify();
    }

    pub(crate) fn reconcile_pending_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.pending_ui.restore_focus)
            && self
                .pending_ui
                .menu_focus
                .iter()
                .any(|focus| focus.is_focused(window))
        {
            self.input.read(cx).focus_handle(cx).focus(window, cx);
        }
    }

    fn close_pending_menu_action(
        &mut self,
        _: &ClosePendingMessageMenu,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_pending_menu(window, cx);
        cx.stop_propagation();
    }

    fn next_pending_menu_item(
        &mut self,
        _: &NextPendingMessageMenuItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = self
            .pending_ui
            .menu_focus
            .iter()
            .position(|focus| focus.is_focused(window))
            .unwrap_or(0);
        self.pending_ui.menu_focus[1 - current].focus(window, cx);
        cx.stop_propagation();
    }

    pub(crate) fn render_pending_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let id = self.pending_ui.menu.as_ref()?;
        let item = self.pending_ui.items.get(id)?;
        let bounds = item.bounds.get()?;
        let colors = theme(cx).colors;
        let enabled = self.pending_controls_enabled();
        let editable = self.pending_draft_is_empty(cx) && self.pending_ui.recovered_edit.is_none();
        let edit_reason = if self.pending_ui.recovered_edit.is_some() {
            vega_i18n::t("ui.handle_the_retrieved_message_first")
        } else {
            vega_i18n::t("ui.send_or_clear_the_current_draft_first")
        };
        let edit = self
            .pending_button(
                PendingButton {
                    id: "pending-menu-edit".into(),
                    label: vega_i18n::t("ui.edit_message"),
                    description: (!editable).then_some(edit_reason),
                    glyph: Icon::Pencil,
                    focus: self.pending_ui.menu_focus[0].clone(),
                    enabled: enabled && editable,
                    selected: false,
                    action: PendingControlAction::Queue(PendingQueueAction::Edit(id.clone())),
                },
                cx,
            )
            .w_full()
            .h_auto()
            .min_h(px(Layout::PENDING_MESSAGE_ROW_HEIGHT))
            .px_2()
            .py_1()
            .justify_start()
            .tab_stop(true)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .child(vega_i18n::t("ui.edit_message"))
                    .when(!editable, |element| {
                        element.child(
                            div()
                                .text_size(px(Typography::METADATA))
                                .text_color(colors.text_tertiary)
                                .child(edit_reason),
                        )
                    }),
            );
        let toggle_label =
            if Self::default_running_message_behavior(cx) == PendingMessageBehavior::Queue {
                vega_i18n::t("ui.turn_queue_off")
            } else {
                vega_i18n::t("ui.turn_queue_on")
            };
        let toggle = self
            .pending_button(
                PendingButton {
                    id: "pending-menu-toggle".into(),
                    label: toggle_label,
                    description: None,
                    glyph: Icon::Queue,
                    focus: self.pending_ui.menu_focus[1].clone(),
                    enabled,
                    selected: false,
                    action: PendingControlAction::Queue(PendingQueueAction::ToggleDefault),
                },
                cx,
            )
            .w_full()
            .h(px(Layout::PENDING_MESSAGE_ROW_HEIGHT))
            .px_2()
            .justify_start()
            .child(toggle_label);
        let menu = div()
            .id("pending-message-menu")
            .key_context("PendingMessageMenu")
            .on_action(cx.listener(Self::close_pending_menu_action))
            .on_action(cx.listener(Self::next_pending_menu_item))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down_out(
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    if this
                        .pending_ui
                        .menu
                        .as_ref()
                        .and_then(|id| this.pending_ui.items.get(id))
                        .and_then(|item| item.bounds.get())
                        .is_some_and(|bounds| bounds.contains(&event.position))
                    {
                        return;
                    }
                    this.close_pending_menu(window, cx);
                }),
            )
            .occlude()
            .w(px(Layout::TASK_MENU_WIDTH))
            .p_1()
            .flex()
            .flex_col()
            .rounded(px(Layout::MENU_RADIUS))
            .border_1()
            .border_color(colors.border_subtle)
            .bg(colors.bg_elevated)
            .text_color(colors.text_primary)
            .shadow_sm()
            .child(edit)
            .child(toggle);
        Some(
            div()
                .absolute()
                .inset_0()
                .child(
                    anchored()
                        .anchor(Anchor::TopRight)
                        .position(point(bounds.right(), bounds.bottom()))
                        .snap_to_window_with_margin(px(Layout::PENDING_MESSAGE_OVERLAP))
                        .child(deferred(menu).with_priority(3)),
                )
                .into_any_element(),
        )
    }

    pub(crate) fn append_pending_user(
        &mut self,
        pending_id: &str,
        message_id: &str,
        seq: u64,
        content: &str,
        images: Vec<ImageAttachment>,
        cx: &mut Context<Self>,
    ) {
        self.pending_queue
            .messages
            .retain(|message| message.id != pending_id);
        self.pending_ui.items.remove(pending_id);
        if self.pending_ui.menu.as_deref() == Some(pending_id) {
            self.pending_ui.menu = None;
            self.pending_ui.restore_focus = true;
        }
        self.ensure_entry_identities();
        if self
            .entry_identities
            .iter()
            .any(|identity| identity.message_id.as_deref() == Some(message_id))
        {
            cx.notify();
            return;
        }
        if self.promote_pending_echo(pending_id, message_id, content, seq, cx) {
            return;
        }
        let sequence = i64::try_from(seq).ok();
        let block_id = self.user_block_seq;
        self.user_block_seq += 1;
        let index = self.entries.len();
        let copy = MessageCopy::from_live_text(content);
        let projected = copy.original_text();
        self.entries.push(StreamEntry::User {
            copy,
            lines: user_message_lines(block_id, &projected),
        });
        self.list_append(index);
        self.set_entry_identity(
            index,
            self.durable_entry_identity(message_id, "user-text", sequence),
        );
        if !images.is_empty() {
            let images = self.history_image_previews(images, cx);
            let index = self.entries.len();
            self.entries.push(StreamEntry::UserImages { images });
            self.list_append(index);
            self.set_entry_identity(
                index,
                self.durable_entry_identity(message_id, "user-images", sequence),
            );
        }
        if self.composer_history.last().map(String::as_str) != Some(content) {
            self.composer_history.push(content.to_string());
        }
        cx.notify();
    }
}
