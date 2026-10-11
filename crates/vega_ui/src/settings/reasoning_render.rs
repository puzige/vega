use super::reasoning_state::{REASONING_EFFORTS, is_legal_effort, standard_glm};
use super::*;
use crate::interaction::InteractionExt;

impl SettingsView {
    /// Independent thinking capability and preference editor. The page lets
    /// a user explicitly declare a provider/model protocol from a readable
    /// template, then edit support, effort, disabled, preference, and
    /// tool-round replay. Every control emits a typed save request; this view
    /// never reads or writes the filesystem itself.
    pub(crate) fn render_reasoning(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let reload_enabled = !matches!(&self.reasoning, ReasoningSettingsProjection::Saving { .. });
        let header = settings_actions()
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
        let mut column = settings_section().child(header);
        match &self.reasoning {
            ReasoningSettingsProjection::Loading => {
                column = column.child(settings_status(
                    "正在读取思考设置…",
                    colors.text_secondary,
                    colors,
                ));
            }
            ReasoningSettingsProjection::Saving { profile, .. } => {
                column = column.child(settings_status(
                    format!("正在保存 {} / {}…", profile.provider, profile.model),
                    colors.text_secondary,
                    colors,
                ));
            }
            ReasoningSettingsProjection::Ready {
                profiles, error, ..
            } => {
                if profiles.is_empty() {
                    column = column.child(settings_status(
                        "当前还没有已声明的模型能力；未知模型会使用提供方默认",
                        colors.text_secondary,
                        colors,
                    ));
                }
                if let Some(error) = error {
                    column = column.child(settings_status(
                        reasoning_settings_error_label(*error),
                        colors.danger,
                        colors,
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
        let mut card = settings_group(colors)
            .child(
                div()
                    .text_size(px(Typography::HEADING_BLOCK))
                    .font_weight(Typography::HEADING_BLOCK_WEIGHT)
                    .child(owner),
            )
            .child(
                div()
                    .text_size(px(Typography::METADATA))
                    .text_color(colors.text_secondary)
                    .child(reasoning_protocol_label(profile.protocol)),
            );

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
                    .flex_wrap()
                    .items_center()
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

        card = card
            .child(settings_field(
                format!("reasoning-protocol-{index}"),
                "协议",
                None,
                settings_actions()
                    .child(reasoning_protocol_label(profile.protocol))
                    .child(reasoning_change_button(
                        "更改协议",
                        colors,
                        self.reasoning_focus(&ReasoningFocusTarget::Protocol(index)),
                        cx.listener(move |this, _: &MouseUpEvent, _, cx| {
                            this.cycle_reasoning_protocol(index, cx)
                        }),
                    )),
                colors,
            ))
            .child(settings_field(
                format!("reasoning-support-{index}"),
                "支持范围",
                None,
                settings_actions()
                    .child(reasoning_support_label(profile.support))
                    .child(reasoning_change_button(
                        "更改支持范围",
                        colors,
                        self.reasoning_focus(&ReasoningFocusTarget::Support(index)),
                        cx.listener(move |this, _: &MouseUpEvent, _, cx| {
                            this.cycle_reasoning_support(index, cx)
                        }),
                    )),
                colors,
            ));
        let effort_enabled = !matches!(
            profile.support,
            ReasoningSupport::Unsupported | ReasoningSupport::Unknown
        );
        let mut effort_row = settings_actions().gap_1();
        for effort in REASONING_EFFORTS {
            if !is_legal_effort(&profile, effort) {
                continue;
            }
            let selected = profile.efforts.iter().any(|candidate| candidate == effort);
            let focus = effort_enabled
                .then(|| self.reasoning_focus(&ReasoningFocusTarget::Effort { index, effort }))
                .flatten();
            let enabled = focus.is_some();
            effort_row = effort_row.child(
                settings_action_chrome(colors)
                    .id(format!("reasoning-effort-{index}-{effort}"))
                    .interactive(colors, enabled, selected)
                    .debug_selector(move || format!("reasoning-effort-{index}-{effort}"))
                    .border_color(gpui_kit::transparent_black())
                    .when_some(focus, |button, focus| button.track_focus(&focus))
                    .focus_visible(move |style| style.border_color(colors.accent))
                    .aria_label(format!(
                        "{}：{}",
                        reasoning_effort_label(effort),
                        if selected { "已允许" } else { "未允许" }
                    ))
                    .when(selected, |button| button.text_color(colors.brand_primary))
                    .when(!enabled, |button| button.text_color(colors.text_tertiary))
                    .when(enabled, |button| {
                        button.cursor_pointer().on_mouse_up(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                this.toggle_reasoning_effort(index, effort, cx)
                            }),
                        )
                    })
                    .child(reasoning_effort_label(effort))
                    .when(selected, |button| {
                        button.child(crate::icons::icon(
                            crate::icons::Icon::Check,
                            colors.brand_primary,
                        ))
                    }),
            );
        }
        card = card.child(settings_field(
            format!("reasoning-efforts-{index}"),
            "允许档位",
            Some(if profile.efforts.is_empty() {
                "提供方默认".into()
            } else {
                "可选择多个档位".into()
            }),
            effort_row,
            colors,
        ));
        card = card.child(settings_field(
            format!("reasoning-preference-{index}"),
            "当前偏好",
            None,
            settings_actions()
                .child(reasoning_choice_label(&profile.preference))
                .child(reasoning_change_button(
                    "更改偏好",
                    colors,
                    self.reasoning_focus(&ReasoningFocusTarget::Preference(index)),
                    cx.listener(move |this, _: &MouseUpEvent, _, cx| {
                        this.cycle_reasoning_preference(index, cx)
                    }),
                )),
            colors,
        ));
        if standard_glm(&profile) {
            card = card.child(settings_field(
                format!("reasoning-disabled-{index}"),
                "关闭思考",
                None,
                div()
                    .text_color(colors.text_secondary)
                    .child("标准 GLM 模型固定开启"),
                colors,
            ));
        } else {
            card = card.child(settings_field(
                format!("reasoning-disabled-{index}"),
                "允许关闭思考",
                None,
                settings_actions()
                    .child(if profile.supports_disabled {
                        "是"
                    } else {
                        "否"
                    })
                    .child(reasoning_change_button(
                        "更改关闭能力",
                        colors,
                        self.reasoning_focus(&ReasoningFocusTarget::Disabled(index)),
                        cx.listener(move |this, _: &MouseUpEvent, _, cx| {
                            this.toggle_reasoning_disabled(index, cx)
                        }),
                    )),
                colors,
            ));
        }
        let replay_editable = profile.protocol != ReasoningProtocol::Unknown
            && !matches!(
                profile.support,
                ReasoningSupport::Unsupported | ReasoningSupport::Unknown
            );
        card = card.child(settings_field(
            format!("reasoning-replay-{index}"),
            "工具轮回传",
            None,
            settings_actions()
                .child(if profile.preserve_reasoning_content {
                    "开"
                } else {
                    "关"
                })
                .child(reasoning_change_button(
                    if replay_editable {
                        "更改工具轮回传"
                    } else {
                        "工具轮回传未声明"
                    },
                    colors,
                    replay_editable
                        .then(|| self.reasoning_focus(&ReasoningFocusTarget::Replay(index)))
                        .flatten(),
                    cx.listener(move |this, _: &MouseUpEvent, _, cx| {
                        this.toggle_reasoning_replay(index, cx)
                    }),
                )),
            colors,
        ));
        card.into_any_element()
    }
}

fn reasoning_change_button(
    label: &'static str,
    colors: vega_theme::ThemeColors,
    focus: Option<FocusHandle>,
    listener: impl Fn(&MouseUpEvent, &mut Window, &mut gpui_kit::App) + 'static,
) -> AnyElement {
    let enabled = focus.is_some();
    let id = focus
        .as_ref()
        .map(gpui_kit::ElementId::from)
        .unwrap_or_else(|| label.into());
    settings_action_chrome(colors)
        .id(id)
        .interactive(colors, enabled, false)
        .border_color(gpui_kit::transparent_black())
        .aria_label(label)
        .when_some(focus, |button, focus| button.track_focus(&focus))
        .focus_visible(move |style| style.border_color(colors.accent))
        .text_color(if enabled {
            colors.text_secondary
        } else {
            colors.text_tertiary
        })
        .when(enabled, |button| {
            button
                .cursor_pointer()
                .on_mouse_up(MouseButton::Left, listener)
        })
        .child("更改")
        .into_any_element()
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
