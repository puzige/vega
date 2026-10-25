use super::*;
use crate::interaction::InteractionExt;
use crate::scroll_edge_fade::{ScrollFade, ScrollableFadeExt};
use gpui_kit::SharedString;
use vega_conversation::types::PrivacyReview;

#[derive(Clone)]
pub enum PrivacyAction {
    Review,
    Exception {
        snapshot: PrivacyReview,
        index: usize,
        allow: bool,
    },
    Clean {
        snapshot: PrivacyReview,
    },
}

#[derive(Clone)]
pub struct PrivacyRequested {
    pub thread_id: String,
    pub action: PrivacyAction,
}
impl EventEmitter<PrivacyRequested> for ConversationStream {}

#[derive(Default)]
pub(crate) struct PrivacyPanel {
    pub opened: bool,
    pub pending: bool,
    pub confirming: bool,
    pub review: Option<PrivacyReview>,
    pub error: Option<crate::localized_copy::LocalizedCopy>,
    pub details: HashSet<usize>,
}

impl ConversationStream {
    pub(super) fn request_privacy(&mut self, action: PrivacyAction, cx: &mut Context<Self>) {
        if self.privacy.pending {
            return;
        }
        self.privacy.opened = true;
        self.privacy.pending = true;
        self.privacy.confirming = false;
        self.privacy.error = None;
        self.privacy.details.clear();
        cx.emit(PrivacyRequested {
            thread_id: self.thread.id.clone(),
            action,
        });
        cx.notify();
    }

    pub fn apply_privacy_review(
        &mut self,
        result: Result<PrivacyReview, String>,
        cx: &mut Context<Self>,
    ) {
        self.apply_privacy_result(result.map_err(Into::into), cx);
    }

    pub fn apply_privacy_result(
        &mut self,
        result: Result<PrivacyReview, crate::localized_copy::LocalizedCopy>,
        cx: &mut Context<Self>,
    ) {
        self.privacy.pending = false;
        match result {
            Ok(review) => self.privacy.review = Some(review),
            Err(error) => self.privacy.error = Some(error),
        }
        cx.notify();
    }

    pub(crate) fn render_privacy_control(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let mut view = div()
            .px(px(Layout::CONVERSATION_PADDING))
            .flex()
            .flex_col()
            .gap_2();
        if self.privacy.pending {
            view = view.child(vega_i18n::t("ui.checking"));
        }
        if let Some(error) = &self.privacy.error {
            view = view.child(div().text_color(colors.danger).child(error.render()));
        }
        if let Some(review) = &self.privacy.review {
            let mut rows = div()
                .id("credential-review-results")
                .max_h(px(240.))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap_2()
                .scroll_edge_fade(colors.bg_base, ScrollFade::compact());
            if review.findings.is_empty() {
                rows = rows.child(vega_i18n::t("ui.no_credentials_need_attention"));
            }
            for (index, finding) in review.findings.iter().enumerate() {
                let snapshot = review.clone();
                let allow = !finding.excepted;
                let mut row = div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(finding.source_label())
                    .child(finding.preview_text())
                    .child(
                        PrivacyButton::new(format!("credential-details-{index}"), colors)
                            .label(if self.privacy.details.contains(&index) {
                                vega_i18n::t("ui.hide_details")
                            } else {
                                vega_i18n::t("ui.detection_details")
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !this.privacy.details.remove(&index) {
                                    this.privacy.details.insert(index);
                                }
                                cx.notify();
                            })),
                    );
                if self.privacy.details.contains(&index) {
                    row = row.child(format!(
                        "{} · {} · {} · {}–{}",
                        finding.source_id, finding.field, finding.rule, finding.start, finding.end
                    ));
                }
                if let Some(message) = &finding.message_id {
                    let message = message.clone();
                    row = row.child(
                        PrivacyButton::new(format!("credential-locate-{index}"), colors)
                            .label(vega_i18n::t("ui.locate"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.request_message_location(&message, cx)
                            })),
                    );
                }
                if !finding.exact {
                    row = row.child(
                        PrivacyButton::new(format!("credential-exception-{index}"), colors)
                            .label(if allow {
                                vega_i18n::t("ui.mark_as_test_value_false_positive")
                            } else {
                                vega_i18n::t("ui.remove_exception")
                            })
                            .disabled(self.privacy.pending)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.request_privacy(
                                    PrivacyAction::Exception {
                                        snapshot: snapshot.clone(),
                                        index,
                                        allow,
                                    },
                                    cx,
                                )
                            })),
                    );
                }
                rows = rows.child(row);
            }
            view = view.child(rows);
            if review.findings.iter().any(|finding| finding.cleanable) {
                if self.privacy.confirming {
                    let snapshot = review.clone();
                    view = view
                        .child(vega_i18n::t(
                            "ui.this_permanently_replaces_credentials_in_this_conversation_and",
                        ))
                        .child(
                            PrivacyButton::new("credential-clean-confirm", colors)
                                .label(vega_i18n::t("ui.confirm_permanent_cleanup"))
                                .disabled(self.privacy.pending)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.request_privacy(
                                        PrivacyAction::Clean {
                                            snapshot: snapshot.clone(),
                                        },
                                        cx,
                                    )
                                })),
                        );
                    view = view.child(
                        PrivacyButton::new("credential-clean-cancel", colors)
                            .label(vega_i18n::t("ui.cancel_cleanup"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.privacy.confirming = false;
                                cx.notify();
                            })),
                    );
                } else {
                    view = view.child(
                        PrivacyButton::new("credential-clean-preview", colors)
                            .label(vega_i18n::t(
                                "ui.permanently_remove_credentials_from_this_conversation",
                            ))
                            .disabled(self.privacy.pending)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.privacy.confirming = true;
                                cx.notify();
                            })),
                    );
                }
            }
        }
        view.child(
            PrivacyButton::new("credential-review-close", colors)
                .label(vega_i18n::t("ui.close"))
                .disabled(self.privacy.pending)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.privacy.opened = false;
                    this.privacy.confirming = false;
                    cx.notify();
                })),
        )
        .into_any_element()
    }
}

pub(super) struct PrivacyButton {
    id: SharedString,
    label: &'static str,
    disabled: bool,
    colors: vega_theme::ThemeColors,
}

impl PrivacyButton {
    pub(super) fn new(id: impl Into<SharedString>, colors: vega_theme::ThemeColors) -> Self {
        Self {
            id: id.into(),
            label: "",
            disabled: false,
            colors,
        }
    }
    pub(super) fn label(mut self, label: &'static str) -> Self {
        self.label = label;
        self
    }
    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
    pub(super) fn on_click(
        self,
        activate: impl Fn(&(), &mut Window, &mut App) + 'static,
    ) -> AnyElement {
        let enabled = !self.disabled;
        let colors = self.colors;
        let activate = std::rc::Rc::new(activate);
        let keyboard = activate.clone();
        div()
            .id(self.id)
            .aria_label(self.label)
            .focusable()
            .tab_stop(enabled)
            .px_2()
            .py_1()
            .rounded_md()
            .text_color(if enabled {
                colors.text_secondary
            } else {
                colors.text_tertiary
            })
            .interaction_surface(colors.bg_base, colors.bg_hover, colors.bg_active, enabled)
            .when(enabled, |button| {
                button
                    .cursor_pointer()
                    .focus_visible(move |style| style.border_1().border_color(colors.accent))
            })
            .on_click(move |_, window, cx| {
                if enabled {
                    cx.stop_propagation();
                    activate(&(), window, cx);
                }
            })
            .on_key_down(move |event, window, cx| {
                if enabled && matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    cx.stop_propagation();
                    keyboard(&(), window, cx);
                }
            })
            .child(self.label)
            .into_any_element()
    }
}
