use super::*;
use crate::interaction::InteractionExt;
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
    pub error: Option<String>,
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
            view = view.child("正在检查…");
        }
        if let Some(error) = &self.privacy.error {
            view = view.child(div().text_color(colors.danger).child(error.clone()));
        }
        if let Some(review) = &self.privacy.review {
            let mut rows = div()
                .id("credential-review-results")
                .max_h(px(240.))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap_2();
            if review.findings.is_empty() {
                rows = rows.child("未发现需要处理的凭据");
            }
            for (index, finding) in review.findings.iter().enumerate() {
                let snapshot = review.clone();
                let allow = !finding.excepted;
                let mut row = div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(finding.source_label.clone())
                    .child(finding.preview.clone())
                    .child(
                        PrivacyButton::new(format!("credential-details-{index}"), colors)
                            .label(if self.privacy.details.contains(&index) {
                                "收起详情"
                            } else {
                                "检测详情"
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
                            .label("定位")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.request_message_location(&message, cx)
                            })),
                    );
                }
                if !finding.exact {
                    row = row.child(
                        PrivacyButton::new(format!("credential-exception-{index}"), colors)
                            .label(if allow {
                                "标记为测试值 / 误报"
                            } else {
                                "撤销例外"
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
                    view = view.child("将永久替换本对话记录中的凭据，无法撤销。备份、WAL 和外部文件可能仍保留原值。")
                        .child(PrivacyButton::new("credential-clean-confirm", colors).label("确认永久清理").disabled(self.privacy.pending)
                            .on_click(cx.listener(move |this, _, _, cx| this.request_privacy(PrivacyAction::Clean { snapshot:snapshot.clone() }, cx))));
                    view = view.child(
                        PrivacyButton::new("credential-clean-cancel", colors)
                            .label("取消清理")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.privacy.confirming = false;
                                cx.notify();
                            })),
                    );
                } else {
                    view = view.child(
                        PrivacyButton::new("credential-clean-preview", colors)
                            .label("永久清理对话中的凭据")
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
                .label("关闭")
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
        let selector = self.id.clone();
        div()
            .id(self.id)
            .debug_selector(move || selector.to_string())
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
