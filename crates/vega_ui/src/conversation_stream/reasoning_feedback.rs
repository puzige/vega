use super::*;
use gpui_kit::{Focusable, WeakEntity};

mod leaf;

#[derive(Clone)]
struct Binding {
    owner: WeakEntity<ConversationStream>,
    root: super::selection_menu::ReadingOwner,
    thread: Thread,
    failures: Vec<ReasoningDefaultFailure>,
    failure: ReasoningDefaultFailure,
    focus: Option<FocusHandle>,
}

impl Binding {
    fn capture(
        stream: &ConversationStream,
        failure: &ReasoningDefaultFailure,
        cx: &Context<ConversationStream>,
    ) -> Self {
        Self {
            owner: cx.weak_entity(),
            root: super::selection_menu::ReadingOwner::message(stream, cx.weak_entity(), None),
            thread: stream.thread.clone(),
            failures: stream.reasoning_default_failures.clone(),
            failure: failure.clone(),
            focus: stream
                .reasoning_default_retry_focuses
                .get(&failure.request.defaults.model)
                .cloned(),
        }
    }

    fn same(&self, other: &Self) -> bool {
        self.owner == other.owner
            && self.root.same_binding(&other.root)
            && self.thread == other.thread
            && self.failures == other.failures
            && self.failure == other.failure
            && self.focus == other.focus
    }

    fn source_current(&self, stream: &ConversationStream) -> bool {
        self.thread == stream.thread
            && self.failure.request.thread_id == stream.thread.id
            && self.failures == stream.reasoning_default_failures
            && stream.reasoning_default_failures.iter().find(|failure| {
                failure.request.defaults.model == self.failure.request.defaults.model
            }) == Some(&self.failure)
            && self.focus.as_ref()
                == stream
                    .reasoning_default_retry_focuses
                    .get(&self.failure.request.defaults.model)
    }

    fn current(&self, window: &Window, cx: &App) -> bool {
        self.root.root_current(window, cx)
            && self.owner.upgrade().is_some_and(|owner| {
                let stream = owner.read(cx);
                !stream.reading_preview
                    && !stream.permissions_revoked
                    && !stream.permission_change_pending()
                    && self.source_current(stream)
            })
    }

    fn composing(&self, cx: &App) -> bool {
        self.owner
            .upgrade()
            .is_none_or(|owner| owner.read(cx).input.read(cx).is_composing())
    }

    fn dispatch(&self, action: Action, window: &mut Window, cx: &mut App) {
        if !self.current(window, cx) || self.failure.retry_pending || self.focus.is_none() {
            return;
        }
        let _ = self.owner.update(cx, |stream, cx| {
            if !self.source_current(stream) {
                return;
            }
            match action {
                Action::Retry { keyboard } => {
                    stream
                        .request_reasoning_default_retry(&self.failure.request.defaults.model, cx);
                    if keyboard {
                        let focus = stream.input.read(cx).focus_handle(cx);
                        window.focus(&focus, cx);
                    }
                }
                Action::Neighbor { backwards } => {
                    if let Some(index) = stream
                        .reasoning_default_failures
                        .iter()
                        .position(|failure| failure == &self.failure)
                    {
                        stream.focus_reasoning_retry_neighbor(index, backwards, window, cx);
                    }
                }
            }
        });
    }
}

#[derive(Clone, Copy)]
enum Action {
    Retry { keyboard: bool },
    Neighbor { backwards: bool },
}

impl ConversationStream {
    pub(super) fn render_reasoning_feedback(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        div()
            .w_full()
            .children(self.reasoning_default_failures.iter().map(|failure| {
                let model = &failure.request.defaults.model;
                let binding = Binding::capture(self, failure, cx);
                let key = format!(
                    "reasoning-feedback-{}-{}-{model}",
                    self.thread.id, failure.request.revision
                );
                let mut row = div()
                    .w_full()
                    .max_w(px(Layout::COMPOSER_MAX_WIDTH))
                    .mx_auto()
                    .mt_1()
                    .mb_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().flex_1().min_w_0().child(leaf::Control {
                        id: format!("{key}-message").into(),
                        text: vega_i18n::t_args(
                            "ui.reasoning_default_not_saved",
                            &[("model", model)],
                        ),
                        label: vega_i18n::t_args(
                            "ui.reasoning_default_not_saved",
                            &[("model", model)],
                        ),
                        button: false,
                        colors,
                        foreground: colors.danger,
                        binding: binding.clone(),
                    }));
                if failure.retry_pending {
                    row = row.child(div().w(px(80.)).flex_shrink_0().child(leaf::Control {
                        id: format!("{key}-saving").into(),
                        text: vega_i18n::t("ui.saving").to_owned(),
                        label: vega_i18n::t("ui.saving").to_owned(),
                        button: false,
                        colors,
                        foreground: colors.text_secondary,
                        binding,
                    }));
                } else if binding.focus.is_some() {
                    row = row.child(div().w(px(80.)).flex_shrink_0().child(leaf::Control {
                        id: format!("{key}-retry").into(),
                        text: vega_i18n::t("ui.retry").to_owned(),
                        label: vega_i18n::t_args("ui.reasoning_default_retry", &[("model", model)]),
                        button: true,
                        colors,
                        foreground: colors.text_primary,
                        binding,
                    }));
                }
                row
            }))
            .into_any_element()
    }
}
