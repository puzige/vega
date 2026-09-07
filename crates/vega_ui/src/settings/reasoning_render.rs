use super::reasoning_state::{REASONING_EFFORTS, is_legal_effort, standard_glm};
use super::*;

impl SettingsView {
    /// Independent thinking capability and preference editor. The page lets
    /// a user explicitly declare a provider/model protocol from a readable
    /// template, then edit support, effort, disabled, preference, and
    /// tool-round replay. Every control emits a typed save request; this view
    /// never reads or writes the filesystem itself.
    pub(crate) fn render_reasoning(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let reload_enabled = !matches!(&self.reasoning, ReasoningSettingsProjection::Saving { .. });
        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .child(section_title("思考能力与偏好", colors.text_primary))
            .child(action_button(
                "重新加载",
                colors,
                reload_enabled
                    .then(|| self.reasoning_focus(&ReasoningFocusTarget::Reload))
                    .flatten(),
                cx.listener(|_, _: &MouseUpEvent, _, cx| {
                    cx.emit(ReasoningReloadRequested);
                }),
            ));
        let mut column = div().flex().flex_col().gap_2().child(header);
        match &self.reasoning {
            ReasoningSettingsProjection::Loading => {
                column = column.child(reasoning_status("正在读取思考设置…", colors.text_secondary));
            }
            ReasoningSettingsProjection::Saving { profile, .. } => {
                column = column.child(reasoning_status(
                    format!("正在保存 {} / {}…", profile.provider, profile.model),
                    colors.text_secondary,
                ));
            }
            ReasoningSettingsProjection::Ready {
                profiles, error, ..
            } => {
                if profiles.is_empty() {
                    column = column.child(reasoning_status(
                        "当前还没有已声明的模型能力；未知模型会使用提供方默认",
                        colors.text_tertiary,
                    ));
                }
                if let Some(error) = error {
                    column = column.child(reasoning_status(
                        reasoning_settings_error_label(*error),
                        colors.danger,
                    ));
                }
                let profiles = profiles.clone();
                column = column.children(
                    profiles
                        .into_iter()
                        .enumerate()
                        .map(|(index, profile)| self.render_reasoning_profile(index, profile, cx)),
                );
            }
        }
        column.into_any_element()
    }

    fn render_reasoning_profile(
        &mut self,
        index: usize,
        authority: ReasoningProfileProjection,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let profile = self
            .reasoning_draft
            .as_ref()
            .filter(|draft| draft.provider == authority.provider && draft.model == authority.model)
            .cloned()
            .unwrap_or(authority);
        let owner = format!("{} / {}", profile.provider, profile.model);
        let mut card = div()
            .flex()
            .flex_col()
            .gap_1()
            .px_3()
            .py_2()
            .rounded(px(Layout::PANEL_RADIUS))
            .border_1()
            .border_color(colors.border_subtle)
            .bg(colors.bg_elevated)
            .child(owner);

        if profile.protocol == ReasoningProtocol::Unknown
            || matches!(profile.support, ReasoningSupport::Unknown)
        {
            card = card.child(
                div()
                    .text_color(colors.text_secondary)
                    .child("尚未声明此模型的协议与能力；请选择一个明确的起始模板"),
            );
            let openai_focus = self.reasoning_focus(&ReasoningFocusTarget::Template {
                index,
                template: ReasoningTemplate::OpenAi,
            });
            let zhipu_focus = self.reasoning_focus(&ReasoningFocusTarget::Template {
                index,
                template: ReasoningTemplate::Zhipu,
            });
            card = card.child(
                div()
                    .flex()
                    .gap_2()
                    .child(action_button(
                        "使用 OpenAI 兼容模板",
                        colors,
                        openai_focus,
                        cx.listener(move |this, _: &MouseUpEvent, _, cx| {
                            this.apply_reasoning_template(index, ReasoningTemplate::OpenAi, cx);
                        }),
                    ))
                    .child(action_button(
                        "使用智谱 GLM 模板",
                        colors,
                        zhipu_focus,
                        cx.listener(move |this, _: &MouseUpEvent, _, cx| {
                            this.apply_reasoning_template(index, ReasoningTemplate::Zhipu, cx);
                        }),
                    )),
            );
            return card.into_any_element();
        }

        card = card.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(format!(
                    "协议：{}",
                    reasoning_protocol_label(profile.protocol)
                ))
                .child(action_button_owned(
                    "切换协议".to_string(),
                    colors,
                    self.reasoning_focus(&ReasoningFocusTarget::Protocol(index)),
                    cx.listener(move |this, _: &MouseUpEvent, _, cx| {
                        this.cycle_reasoning_protocol(index, cx);
                    }),
                )),
        );
        card = card.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(format!(
                    "支持范围：{}",
                    reasoning_support_label(profile.support)
                ))
                .child(action_button_owned(
                    "切换支持范围".to_string(),
                    colors,
                    self.reasoning_focus(&ReasoningFocusTarget::Support(index)),
                    cx.listener(move |this, _: &MouseUpEvent, _, cx| {
                        this.cycle_reasoning_support(index, cx);
                    }),
                )),
        );

        let efforts = if profile.efforts.is_empty() {
            "提供方默认".to_string()
        } else {
            profile
                .efforts
                .iter()
                .map(|effort| reasoning_effort_label(effort))
                .collect::<Vec<_>>()
                .join("、")
        };
        card = card.child(
            div()
                .text_color(colors.text_secondary)
                .child(format!("已允许档位：{efforts}")),
        );

        let mut effort_row = div().flex().flex_wrap().gap_2();
        let effort_enabled = !matches!(
            profile.support,
            ReasoningSupport::Unsupported | ReasoningSupport::Unknown
        );
        for effort in REASONING_EFFORTS {
            if !is_legal_effort(&profile, effort) {
                continue;
            }
            let selected = profile.efforts.iter().any(|candidate| candidate == effort);
            let label = format!(
                "{} {}",
                reasoning_effort_label(effort),
                if selected { "✓" } else { "" }
            );
            let focus = effort_enabled
                .then(|| self.reasoning_focus(&ReasoningFocusTarget::Effort { index, effort }));
            effort_row = effort_row.child(action_button_owned(
                label,
                colors,
                focus.flatten(),
                cx.listener(move |this, _: &MouseUpEvent, _, cx| {
                    this.toggle_reasoning_effort(index, effort, cx);
                }),
            ));
        }
        card = card.child(effort_row);

        let preference = reasoning_choice_label(&profile.preference);
        card = card.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(format!("当前偏好：{preference}"))
                .child(action_button_owned(
                    "切换偏好".to_string(),
                    colors,
                    self.reasoning_focus(&ReasoningFocusTarget::Preference(index)),
                    cx.listener(move |this, _: &MouseUpEvent, _, cx| {
                        this.cycle_reasoning_preference(index, cx);
                    }),
                )),
        );

        if standard_glm(&profile) {
            card = card.child(
                div()
                    .text_color(colors.text_secondary)
                    .child("关闭思考：标准 GLM 模型固定开启"),
            );
        } else {
            let disabled = if profile.supports_disabled {
                "是"
            } else {
                "否"
            };
            card = card.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(format!("允许关闭思考：{disabled}"))
                    .child(action_button_owned(
                        "切换关闭能力".to_string(),
                        colors,
                        self.reasoning_focus(&ReasoningFocusTarget::Disabled(index)),
                        cx.listener(move |this, _: &MouseUpEvent, _, cx| {
                            this.toggle_reasoning_disabled(index, cx);
                        }),
                    )),
            );
        }

        let replay = if profile.preserve_reasoning_content {
            "工具轮回传：开"
        } else {
            "工具轮回传：关"
        };
        let replay_editable = profile.protocol != ReasoningProtocol::Unknown
            && !matches!(
                profile.support,
                ReasoningSupport::Unsupported | ReasoningSupport::Unknown
            );
        card = card.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(replay)
                .child(action_button_owned(
                    if replay_editable {
                        "切换工具轮回传".to_string()
                    } else {
                        "工具轮回传未声明".to_string()
                    },
                    colors,
                    replay_editable
                        .then(|| self.reasoning_focus(&ReasoningFocusTarget::Replay(index)))
                        .flatten(),
                    cx.listener(move |this, _: &MouseUpEvent, _, cx| {
                        this.toggle_reasoning_replay(index, cx);
                    }),
                )),
        );
        card.into_any_element()
    }
}

fn reasoning_status(label: impl Into<String>, color: gpui::Rgba) -> Div {
    div()
        .text_color(color)
        .text_size(px(Typography::BODY))
        .child(label.into())
}

fn reasoning_choice_label(choice: &ReasoningChoice) -> String {
    match choice {
        ReasoningChoice::ProviderDefault => "提供方默认".to_string(),
        ReasoningChoice::Disabled => "关闭思考".to_string(),
        ReasoningChoice::Effort(effort) => reasoning_effort_label(effort).to_string(),
    }
}

fn reasoning_protocol_label(protocol: ReasoningProtocol) -> &'static str {
    match protocol {
        ReasoningProtocol::OpenAiChatCompletions => "OpenAI 兼容",
        ReasoningProtocol::ZhipuChatCompletions => "智谱 GLM",
        ReasoningProtocol::Unknown => "未声明",
    }
}

fn reasoning_support_label(support: ReasoningSupport) -> &'static str {
    match support {
        ReasoningSupport::Required => "始终开启",
        ReasoningSupport::Optional => "可选",
        ReasoningSupport::Unsupported => "不支持思考控制",
        ReasoningSupport::Unknown => "未声明",
    }
}

fn reasoning_effort_label(effort: &str) -> &'static str {
    match effort {
        "minimal" => "最少",
        "low" => "低",
        "medium" => "中",
        "high" => "高",
        "xhigh" => "很高",
        "max" => "最高",
        _ => "未识别档位",
    }
}

fn reasoning_settings_error_label(error: ReasoningSettingsErrorCode) -> &'static str {
    match error {
        ReasoningSettingsErrorCode::Io => "思考设置读写或保存复验失败，请重新加载",
        ReasoningSettingsErrorCode::Invalid => "思考能力声明无效，请重新选择模板或修正设置",
        ReasoningSettingsErrorCode::Conflict => "思考设置发生并发修改，已保留外部版本",
        ReasoningSettingsErrorCode::Busy => "思考设置正在保存",
    }
}
