use super::*;
use gpui_kit::WeakEntity;

#[path = "native_leaf.rs"]
mod native_leaf;

#[derive(Clone)]
struct Binding {
    owner: WeakEntity<ConversationStream>,
    root: super::super::selection_menu::ReadingOwner,
    thread: Thread,
    operation: u64,
    source: Option<Thread>,
    phase: Phase,
    focus: Option<FocusHandle>,
}

impl Binding {
    fn capture(stream: &ConversationStream, cx: &Context<ConversationStream>) -> Self {
        Self {
            owner: cx.weak_entity(),
            root: super::super::selection_menu::ReadingOwner::message(
                stream,
                cx.weak_entity(),
                None,
            ),
            thread: stream.thread.clone(),
            operation: stream.permission_editor.operation,
            source: stream.permission_editor.source.clone(),
            phase: stream.permission_editor.phase.clone(),
            focus: Some(stream.permission_editor.reload.clone()),
        }
    }

    fn same(&self, other: &Self) -> bool {
        self.owner == other.owner
            && self.root.same_binding(&other.root)
            && self.thread == other.thread
            && self.operation == other.operation
            && self.source == other.source
            && self.phase == other.phase
            && self.focus == other.focus
    }

    fn source_current(&self, stream: &ConversationStream) -> bool {
        self.thread == stream.thread
            && self.operation == stream.permission_editor.operation
            && self.source == stream.permission_editor.source
            && self.phase == stream.permission_editor.phase
            && self.focus.as_ref() == Some(&stream.permission_editor.reload)
    }

    fn can_reload(&self) -> bool {
        matches!(self.phase, Phase::Failed(_)) && self.focus.is_some()
    }

    fn current(&self, window: &Window, cx: &App) -> bool {
        self.root.root_current(window, cx)
            && self.owner.upgrade().is_some_and(|owner| {
                let stream = owner.read(cx);
                !stream.reading_preview
                    && !stream.ui_presentations_suspended
                    && self.source_current(stream)
            })
    }

    fn composing(&self, cx: &App) -> bool {
        self.owner
            .upgrade()
            .is_none_or(|owner| owner.read(cx).input.read(cx).is_composing())
    }

    fn reload(&self, window: &mut Window, cx: &mut App) {
        if !self.current(window, cx) || !self.can_reload() {
            return;
        }
        let _ = self.owner.update(cx, |stream, cx| {
            if self.source_current(stream) {
                stream.reload_thread_permissions(window, cx);
            }
        });
    }
}

pub(super) fn render(
    stream: &ConversationStream,
    cx: &mut Context<ConversationStream>,
) -> AnyElement {
    let Phase::Failed(error) = stream.permission_editor.phase else {
        return div().into_any_element();
    };
    let key = match error {
        ThreadPermissionError::Conflict => "ui.permission_error_conflict",
        ThreadPermissionError::ResetPending => "ui.permission_error_reset",
        ThreadPermissionError::OutcomeUnknown => "ui.permission_error_unknown",
        ThreadPermissionError::ConfirmationRequired => "ui.permission_error_confirmation",
        ThreadPermissionError::Unavailable => "ui.permission_error_unavailable",
    };
    let colors = theme(cx).colors;
    let binding = Binding::capture(stream, cx);
    let identity = format!("thread-permission-feedback-{}", stream.thread.id);
    let message = vega_i18n::t(key).to_owned();
    let reload = vega_i18n::t("ui.permission_reload").to_owned();
    div()
        .id("thread-permission-feedback")
        .w_full()
        .max_w(px(Layout::COMPOSER_MAX_WIDTH))
        .mx_auto()
        .mb_2()
        .px_2()
        .flex()
        .items_center()
        .gap_2()
        .child(div().flex_1().min_w_0().child(native_leaf::Control {
            id: format!("{identity}-message").into(),
            text: message.clone(),
            label: message,
            button: false,
            colors,
            foreground: colors.danger,
            binding: binding.clone(),
        }))
        .child(
            div()
                .w(px(112.))
                .flex_shrink_0()
                .child(native_leaf::Control {
                    id: format!("{identity}-reload").into(),
                    text: reload.clone(),
                    label: reload,
                    button: true,
                    colors,
                    foreground: colors.text_primary,
                    binding,
                }),
        )
        .into_any_element()
}
