use super::reasoning_state::{REASONING_EFFORTS, is_legal_effort, reasoning_choices, standard_glm};
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
        if self.model_settings.is_none() {
            return self.render_reasoning_model_choices(cx);
        }
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
                    column = column.child(
                        settings_status(
                            reasoning_settings_error_label(*error),
                            colors.danger,
                            colors,
                        )
                        .debug_selector(|| "reasoning-save-error".into()),
                    );
                }
                let profiles = profiles.clone();
                let selected = self.model_settings.clone();
                column = column.children(
                    profiles
                        .into_iter()
                        .enumerate()
                        .filter(|(_, profile)| {
                            selected.as_ref().is_none_or(|(provider, model)| {
                                profile.provider == *provider && profile.model == *model
                            })
                        })
                        .map(|(index, profile)| self.render_reasoning_profile(index, profile, cx)),
                );
            }
        }
        column.into_any_element()
    }

    fn render_reasoning_model_choices(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let mut body = settings_section();
        match &self.reasoning {
            ReasoningSettingsProjection::Ready {
                profiles, error, ..
            } => {
                if let Some(error) = error {
                    body = body.child(settings_status(
                        reasoning_settings_error_label(*error),
                        colors.danger,
                        colors,
                    ));
                }
                if profiles.is_empty() {
                    body = body.child("尚无模型，请先添加提供方和模型");
                }
                for (index, profile) in profiles.iter().enumerate() {
                    let provider = profile.provider.clone();
                    let model = profile.model.clone();
                    body = body.child(
                        settings_actions()
                            .py_2()
                            .border_b_1()
                            .border_color(colors.border_subtle)
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .child(format!("{provider} / {model}")),
                            )
                            .child(reasoning_option(
                                format!("reasoning-model-select-{index}"),
                                "配置",
                                false,
                                colors,
                                cx.listener(move |this, _, _, cx| {
                                    this.open_model_settings(&provider, &model, 2, cx)
                                }),
                            )),
                    );
                }
            }
            ReasoningSettingsProjection::Loading => body = body.child("正在读取模型…"),
            ReasoningSettingsProjection::Saving { .. } => body = body.child("正在保存思考设置…"),
        }
        body.into_any_element()
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
        let mut card = settings_section()
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
            )
            .child(action_button(
                if self.reasoning_advanced {
                    "收起高级协议"
                } else {
                    "高级协议"
                },
                colors,
                self.reasoning_focus(&ReasoningFocusTarget::Advanced(index)),
                cx.listener(move |this, _: &MouseUpEvent, _, cx| {
                    this.toggle_reasoning_advanced(index, cx);
                }),
            ));

        if self.reasoning_advanced {
            let templates = if profile.protocol == ReasoningProtocol::ResponsesApi {
                vec![(ReasoningTemplate::Responses, "Responses 条目回传")]
            } else {
                vec![
                    (ReasoningTemplate::OpenAi, "OpenAI 兼容"),
                    (ReasoningTemplate::Zhipu, "智谱旧兼容"),
                    (ReasoningTemplate::DeepSeek, "DeepSeek 工具回传"),
                    (ReasoningTemplate::GlmPreserved, "GLM 保留思考"),
                    (ReasoningTemplate::GlmInterleaved, "GLM 当前轮思考"),
                ]
            };
            card = card.child(settings_field(
                format!("reasoning-advanced-templates-{index}"),
                "协议模板",
                None,
                settings_actions()
                    .flex_wrap()
                    .children(templates.into_iter().map(|(template, label)| {
                        let focus = self
                            .reasoning_focus(&ReasoningFocusTarget::Template { index, template });
                        let selected = profile.replay_profile == template_replay_setting(template);
                        reasoning_option_with_focus(
                            format!("reasoning-template-{index}-{}", template_id(template)),
                            label,
                            selected,
                            colors,
                            focus,
                            cx.listener(move |this, _, _, cx| {
                                this.apply_reasoning_template(index, template, cx);
                            }),
                        )
                    })),
                colors,
            ));
            if profile.protocol != ReasoningProtocol::ResponsesApi {
                card = card.child(settings_field(
                    format!("reasoning-protocol-{index}"),
                    "协议",
                    None,
                    settings_actions().children(
                        [
                            ReasoningProtocol::Unknown,
                            ReasoningProtocol::OpenAiChatCompletions,
                            ReasoningProtocol::ZhipuChatCompletions,
                            ReasoningProtocol::DeepSeekChatCompletions,
                        ]
                        .into_iter()
                        .enumerate()
                        .map(|(option, protocol)| {
                            reasoning_option(
                                format!("reasoning-protocol-{index}-option-{option}"),
                                reasoning_protocol_label(protocol),
                                profile.protocol == protocol,
                                colors,
                                cx.listener(move |this, _, _, cx| {
                                    this.select_reasoning_protocol(index, protocol, cx)
                                }),
                            )
                        }),
                    ),
                    colors,
                ));
                if profile.protocol != ReasoningProtocol::Unknown {
                    card = card.child(settings_field(
                        format!("reasoning-support-{index}"),
                        "支持范围",
                        None,
                        settings_actions().children(
                            [
                                ReasoningSupport::Required,
                                ReasoningSupport::Optional,
                                ReasoningSupport::Unsupported,
                            ]
                            .into_iter()
                            .enumerate()
                            .map(|(option, support)| {
                                reasoning_option(
                                    format!("reasoning-support-{index}-option-{option}"),
                                    reasoning_support_label(support),
                                    profile.support == support,
                                    colors,
                                    cx.listener(move |this, _, _, cx| {
                                        this.select_reasoning_support(index, support, cx)
                                    }),
                                )
                            }),
                        ),
                        colors,
                    ));
                }
                if profile.protocol != ReasoningProtocol::Unknown
                    && !standard_glm(&profile)
                    && !matches!(
                        profile.support,
                        ReasoningSupport::Unsupported | ReasoningSupport::Unknown
                    )
                {
                    card = card.child(settings_field(
                        format!("reasoning-disabled-{index}"),
                        "允许关闭思考",
                        None,
                        settings_switch(
                            format!("reasoning-disabled-toggle-{index}"),
                            "允许关闭思考",
                            profile.supports_disabled,
                            true,
                            colors,
                        )
                        .when_some(
                            self.reasoning_focus(&ReasoningFocusTarget::Disabled(index)),
                            |control, focus| control.track_focus(&focus),
                        )
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                this.toggle_reasoning_disabled(index, cx)
                            }),
                        )
                        .on_key_down(cx.listener(
                            move |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                    this.toggle_reasoning_disabled(index, cx);
                                    cx.stop_propagation();
                                }
                            },
                        )),
                        colors,
                    ));
                }
            }
        }

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
            None,
            effort_row,
            colors,
        ));
        card = card.child(settings_field(
            format!("reasoning-preference-{index}"),
            "当前偏好",
            None,
            settings_actions().children(reasoning_choices(&profile).into_iter().enumerate().map(
                |(option, choice)| {
                    let label = reasoning_choice_label(&choice);
                    let selected = profile.preference == choice;
                    reasoning_option(
                        format!("reasoning-preference-{index}-option-{option}"),
                        label,
                        selected,
                        colors,
                        cx.listener(move |this, _, _, cx| {
                            this.select_reasoning_preference(index, choice.clone(), cx)
                        }),
                    )
                },
            )),
            colors,
        ));
        card.into_any_element()
    }
}

fn template_id(template: ReasoningTemplate) -> &'static str {
    match template {
        ReasoningTemplate::OpenAi => "openai",
        ReasoningTemplate::Zhipu => "zhipu-legacy",
        ReasoningTemplate::DeepSeek => "deepseek",
        ReasoningTemplate::GlmPreserved => "glm-preserved",
        ReasoningTemplate::GlmInterleaved => "glm-interleaved",
        ReasoningTemplate::Responses => "responses",
    }
}

fn template_replay_setting(template: ReasoningTemplate) -> ReasoningReplaySetting {
    match template {
        ReasoningTemplate::OpenAi => ReasoningReplaySetting::StandardChat,
        ReasoningTemplate::Zhipu => ReasoningReplaySetting::LegacyTextTools,
        ReasoningTemplate::DeepSeek => ReasoningReplaySetting::DeepSeekTools,
        ReasoningTemplate::GlmPreserved => ReasoningReplaySetting::GlmPreserved,
        ReasoningTemplate::GlmInterleaved => ReasoningReplaySetting::GlmInterleaved,
        ReasoningTemplate::Responses => ReasoningReplaySetting::ResponsesItems,
    }
}

fn reasoning_option(
    id: String,
    label: impl Into<gpui_kit::SharedString>,
    selected: bool,
    colors: vega_theme::ThemeColors,
    listener: impl Fn(&MouseUpEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    reasoning_option_with_focus(id, label, selected, colors, None, listener)
}

fn reasoning_option_with_focus(
    id: String,
    label: impl Into<gpui_kit::SharedString>,
    selected: bool,
    colors: vega_theme::ThemeColors,
    focus: Option<FocusHandle>,
    listener: impl Fn(&MouseUpEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let selector = id.clone();
    let listener = std::rc::Rc::new(listener);
    let keyboard = listener.clone();
    settings_action_chrome(colors)
        .id(id)
        .debug_selector(move || selector.clone())
        .interactive(colors, true, selected)
        .focusable()
        .tab_stop(true)
        .when_some(focus, |button, focus| button.track_focus(&focus))
        .cursor_pointer()
        .focus_visible(move |style| style.border_color(colors.accent))
        .when(selected, |button| button.text_color(colors.accent))
        .on_mouse_up(MouseButton::Left, move |event, window, cx| {
            listener(event, window, cx)
        })
        .on_key_down(move |event, window, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                keyboard(
                    &MouseUpEvent {
                        button: MouseButton::Left,
                        position: gpui_kit::point(px(0.), px(0.)),
                        modifiers: event.keystroke.modifiers,
                        click_count: 1,
                    },
                    window,
                    cx,
                );
                cx.stop_propagation();
            }
        })
        .child(label.into())
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
        ReasoningProtocol::DeepSeekChatCompletions => "DeepSeek Chat",
        ReasoningProtocol::ResponsesApi => "Responses API",
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
