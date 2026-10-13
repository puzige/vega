use super::attachments::ImagePreview;
use super::*;
use crate::icons::{Icon, icon};
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
    pub(super) send_focus: FocusHandle,
    pub(super) resume_focus: FocusHandle,
    pub(super) recover_focus: FocusHandle,
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
            send_focus: cx.focus_handle().tab_stop(true),
            resume_focus: cx.focus_handle().tab_stop(true),
            recover_focus: cx.focus_handle().tab_stop(true),
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

impl EventEmitter<PendingQueueRequested> for ConversationStream {}

impl ConversationStream {
    pub fn apply_pending_queue(
        &mut self,
        projection: PendingQueueProjection,
        cx: &mut Context<Self>,
    ) {
        let mut previous = std::mem::take(&mut self.pending_ui.items);
        for message in &projection.messages {
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
                if self.actions.running || self.pending_queue.messages.is_empty() =>
            {
                return;
            }
            PendingQueueAction::Resume | PendingQueueAction::ToggleDefault => {}
        }
        self.close_pending_menu(window, cx);
        self.pending_queue.busy = true;
        self.pending_queue.error = None;
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
            PendingControlAction::Submit => self.submit_message(cx),
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
        let selector = id.clone();
        let keyboard_action = action.clone();
        div()
            .id(id)
            .debug_selector(move || selector.clone())
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
        self.pending_button(
            PendingButton {
                id: "composer-running-send".into(),
                label: match self.running_message_behavior(cx) {
                    PendingMessageBehavior::Queue => "排队发送",
                    PendingMessageBehavior::Steer => "插话",
                },
                description: None,
                glyph: Icon::ArrowUp,
                focus: self.pending_ui.send_focus.clone(),
                enabled: can_send,
                selected: false,
                action: PendingControlAction::Submit,
            },
            cx,
        )
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
        if self.pending_queue.paused && !self.pending_queue.messages.is_empty() {
            let resume = self
                .pending_button(
                    PendingButton {
                        id: "pending-resume".into(),
                        label: "继续队列",
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
                .child("继续队列");
            status = status.child(div().flex().justify_end().mb_1().child(resume));
        }
        if let Some(message) = &self.pending_ui.recovered_edit {
            let can_restore = self.pending_draft_is_empty(cx);
            let recover = self
                .pending_button(
                    PendingButton {
                        id: "pending-recover-edit".into(),
                        label: "继续编辑",
                        description: (!can_restore).then_some("请先发送或清空当前草稿"),
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
                .child("继续编辑");
            let preview = if message.content.is_empty() {
                "图片".to_string()
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
                                .child("请先发送或清空当前草稿"),
                        )
                    }),
            );
        }
        if let Some(error) = &self.pending_queue.error {
            status = status.child(div().mb_2().child(render_rows::status_notice(
                error.clone(),
                colors.danger,
                &colors,
            )));
        }
        status.into_any_element()
    }

    pub(crate) fn render_pending_queue(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let visible = self
            .pending_queue
            .messages
            .len()
            .min(Layout::PENDING_MESSAGE_VISIBLE_ROWS) as f32;
        let viewport_height = visible * Layout::PENDING_MESSAGE_ROW_HEIGHT;
        let rows =
            self.pending_queue
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
                        Some("当前没有运行中的任务，请继续队列")
                    } else if message.behavior == PendingMessageBehavior::Steer {
                        Some("该消息已安排插话")
                    } else {
                        None
                    };
                    let steer = self
                        .pending_button(
                            PendingButton {
                                id: format!("pending-steer-{id}"),
                                label: "插话",
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
                        .child("插话");
                    let remove = self.pending_button(
                        PendingButton {
                            id: format!("pending-remove-{id}"),
                            label: "删除消息",
                            description: None,
                            glyph: Icon::Trash,
                            focus: item.focus[1].clone(),
                            enabled,
                            selected: false,
                            action: PendingControlAction::Queue(PendingQueueAction::Remove(
                                id.clone(),
                            )),
                        },
                        cx,
                    );
                    let more = self
                        .pending_button(
                            PendingButton {
                                id: format!("pending-more-{id}"),
                                label: "消息选项",
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
                        "图片".into()
                    } else {
                        message.content.replace(['\r', '\n'], " ")
                    };
                    let content_id = id.clone();
                    let row_id = id.clone();
                    Some(
                        div()
                            .debug_selector(move || format!("pending-row-{row_id}"))
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
                            .children(item.images.iter().take(1).enumerate().map(
                                |(index, image)| {
                                    let image_id = id.clone();
                                    div()
                                        .debug_selector(move || {
                                            format!("pending-thumbnail-{image_id}-{index}")
                                        })
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
                                                    .child(format!(
                                                        "+{}",
                                                        message.images.len() - 1
                                                    )),
                                            )
                                        })
                                },
                            ))
                            .child(
                                div()
                                    .debug_selector(move || format!("pending-content-{content_id}"))
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
                    .debug_selector(|| "pending-message-rail".into())
                    .relative()
                    .w_full()
                    .h(px(viewport_height + Layout::PENDING_MESSAGE_OVERLAP))
                    .rounded(px(Layout::PENDING_MESSAGE_RADIUS))
                    .bg(colors.bg_elevated)
                    .overflow_hidden()
                    .child(
                        div()
                            .id("pending-message-scroll")
                            .debug_selector(|| "pending-message-scroll".into())
                            .h(px(viewport_height))
                            .w_full()
                            .flex()
                            .flex_col()
                            .overflow_y_scroll()
                            .track_scroll(&self.pending_ui.scroll)
                            .key_context("Composer")
                            .on_action(cx.listener(Self::next_composer_control))
                            .on_action(cx.listener(Self::previous_composer_control))
                            .children(rows),
                    )
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
            "请先处理已取回的消息"
        } else {
            "请先发送或清空当前草稿"
        };
        let edit = self
            .pending_button(
                PendingButton {
                    id: "pending-menu-edit".into(),
                    label: "编辑消息",
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
            .child(div().flex().flex_col().min_w_0().child("编辑消息").when(
                !editable,
                |element| {
                    element.child(
                        div()
                            .text_size(px(Typography::METADATA))
                            .text_color(colors.text_tertiary)
                            .child(edit_reason),
                    )
                },
            ));
        let toggle_label =
            if Self::default_running_message_behavior(cx) == PendingMessageBehavior::Queue {
                "关闭排队"
            } else {
                "开启排队"
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
            .debug_selector(|| "pending-message-menu".into())
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
        let sequence = i64::try_from(seq).ok();
        let block_id = self.user_block_seq;
        self.user_block_seq += 1;
        let index = self.entries.len();
        self.entries.push(StreamEntry::User {
            copy: MessageCopy::new(content),
            lines: user_message_lines(block_id, content),
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
