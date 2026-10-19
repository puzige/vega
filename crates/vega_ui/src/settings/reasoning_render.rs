use super::reasoning_state::reasoning_choices;
use super::*;
use crate::interaction::InteractionExt;

impl SettingsView {
    pub(crate) fn render_reasoning(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        if self.model_settings.is_none() {
            return self.render_reasoning_model_choices(cx);
        }
        let reload_enabled = !matches!(&self.reasoning, ReasoningSettingsProjection::Saving { .. });
        let header = settings_actions()
            .items_center()
            .justify_between()
            .child(section_title("思考偏好", colors.text_primary))
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
                        "尚无模型，请先添加提供方和模型",
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
        let mut card = settings_section().child(
            div()
                .text_size(px(Typography::HEADING_BLOCK))
                .font_weight(Typography::HEADING_BLOCK_WEIGHT)
                .child(owner),
        );
        if !profile.supports_preference() {
            card = card.child(settings_status(
                format!(
                    "已保存的思考偏好“{}”不受此模型支持，请重新选择",
                    reasoning_choice_label(&profile.preference)
                ),
                colors.danger,
                colors,
            ));
        }
        card.child(settings_field(
            "当前偏好",
            None,
            settings_actions().children(reasoning_choices(&profile).into_iter().enumerate().map(
                |(option, choice)| {
                    let label = reasoning_choice_label(&choice);
                    let selected = profile.preference == choice;
                    let focus = self.reasoning_focus(&ReasoningFocusTarget::Preference {
                        index,
                        choice: choice.clone(),
                    });
                    reasoning_option_with_focus(
                        format!("reasoning-preference-{index}-option-{option}"),
                        label,
                        selected,
                        colors,
                        focus,
                        cx.listener(move |this, _, _, cx| {
                            this.select_reasoning_preference(index, choice.clone(), cx)
                        }),
                    )
                },
            )),
            colors,
        ))
        .into_any_element()
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
    let listener = std::rc::Rc::new(listener);
    let keyboard = listener.clone();
    settings_action_chrome(colors)
        .id(id)
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
        ReasoningChoice::Enabled => "开启思考".into(),
        ReasoningChoice::ProviderDefault => "提供方默认".to_string(),
        ReasoningChoice::Disabled => "关闭思考".to_string(),
        ReasoningChoice::Effort(effort) => reasoning_effort_label(effort).to_string(),
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
        ReasoningSettingsErrorCode::Invalid => "无法使用当前思考设置，请检查偏好文件后重新加载",
        ReasoningSettingsErrorCode::Conflict => "思考设置发生并发修改，已保留外部版本",
        ReasoningSettingsErrorCode::Busy => "思考设置正在保存",
    }
}
