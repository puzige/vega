use super::*;
use crate::interaction::InteractionExt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelCapabilityFocusTarget {
    Reload,
    Effort { index: usize, effort: String },
    Image(usize),
    Save(usize),
    Cancel(usize),
    Reset(usize),
}

impl SettingsView {
    pub fn apply_model_capability_projection(
        &mut self,
        projection: ModelCapabilitySettingsProjection,
        cx: &mut Context<Self>,
    ) {
        if let ModelCapabilitySettingsProjection::Ready { models, error, .. } = &projection {
            self.model_capability_models = models.clone();
            if error.is_none() {
                self.model_capability_draft = None;
            }
        }
        self.model_capabilities = projection;
        self.rebuild_model_capability_focuses(cx);
        cx.notify();
    }

    pub fn model_capability_save_is_current(&self, generation: u64, operation_id: u64) -> bool {
        matches!(&self.model_capabilities, ModelCapabilitySettingsProjection::Saving { generation: current, operation_id: operation, .. } if *current == generation && *operation == operation_id)
    }

    pub fn model_capability_focus(
        &self,
        target: &ModelCapabilityFocusTarget,
    ) -> Option<FocusHandle> {
        self.model_capability_focuses
            .iter()
            .find(|(value, _)| value == target)
            .map(|(_, focus)| focus.clone())
    }

    pub(crate) fn rebuild_model_capability_focuses(&mut self, cx: &mut Context<Self>) {
        let mut targets = vec![ModelCapabilityFocusTarget::Reload];
        for (index, model) in self.model_capability_models.iter().enumerate() {
            for effort in &model.resolved.capabilities.encodable_efforts {
                targets.push(ModelCapabilityFocusTarget::Effort {
                    index,
                    effort: effort.clone(),
                });
            }
            targets.extend([
                ModelCapabilityFocusTarget::Image(index),
                ModelCapabilityFocusTarget::Save(index),
                ModelCapabilityFocusTarget::Cancel(index),
                ModelCapabilityFocusTarget::Reset(index),
            ]);
        }
        let previous = std::mem::take(&mut self.model_capability_focuses);
        self.model_capability_focuses = targets
            .into_iter()
            .map(|target| {
                let focus = previous
                    .iter()
                    .find(|(value, _)| value == &target)
                    .map(|(_, focus)| focus.clone())
                    .unwrap_or_else(|| cx.focus_handle().tab_stop(true));
                (target, focus)
            })
            .collect();
    }

    fn editable_capability(&self, index: usize) -> Option<ModelCapabilityOverride> {
        let authority = self.model_capability_models.get(index)?;
        self.model_capability_draft
            .as_ref()
            .filter(|draft| draft.identity == authority.resolved.identity)
            .cloned()
            .or_else(|| {
                Some(
                    authority
                        .user_override
                        .clone()
                        .unwrap_or_else(|| ModelCapabilityOverride {
                            identity: authority.resolved.identity.clone(),
                            allowed_efforts: None,
                            image_input: None,
                        }),
                )
            })
    }

    fn toggle_capability_effort(&mut self, index: usize, effort: &str, cx: &mut Context<Self>) {
        if !matches!(
            self.model_capabilities,
            ModelCapabilitySettingsProjection::Ready { .. }
        ) {
            return;
        }
        let Some(authority) = self.model_capability_models.get(index) else {
            return;
        };
        if !authority
            .resolved
            .capabilities
            .encodable_efforts
            .iter()
            .any(|value| value == effort)
        {
            return;
        }
        let Some(mut draft) = self.editable_capability(index) else {
            return;
        };
        let allowed = draft
            .allowed_efforts
            .get_or_insert_with(|| authority.resolved.capabilities.allowed_efforts.clone());
        if allowed.iter().any(|value| value == effort) {
            allowed.retain(|value| value != effort);
        } else {
            allowed.push(effort.into());
        }
        *allowed = REASONING_EFFORTS
            .iter()
            .filter(|value| allowed.iter().any(|effort| effort == **value))
            .map(|value| (*value).into())
            .collect();
        self.model_capability_draft = Some(draft);
        cx.notify();
    }

    fn toggle_capability_image(&mut self, index: usize, cx: &mut Context<Self>) {
        if !matches!(
            self.model_capabilities,
            ModelCapabilitySettingsProjection::Ready { .. }
        ) {
            return;
        }
        let Some(authority) = self.model_capability_models.get(index) else {
            return;
        };
        let Some(mut draft) = self.editable_capability(index) else {
            return;
        };
        draft.image_input = Some(
            !draft
                .image_input
                .unwrap_or(authority.resolved.capabilities.image_input),
        );
        self.model_capability_draft = Some(draft);
        cx.notify();
    }

    pub(crate) fn save_model_capability(
        &mut self,
        index: usize,
        reset: bool,
        cx: &mut Context<Self>,
    ) {
        let ModelCapabilitySettingsProjection::Ready { generation, .. } = self.model_capabilities
        else {
            return;
        };
        let Some(base) = self.model_capability_models.get(index).cloned() else {
            return;
        };
        let patch = if reset {
            ModelCapabilityPatch::Reset {
                identity: base.resolved.identity.clone(),
            }
        } else {
            let Some(draft) = self
                .model_capability_draft
                .as_ref()
                .filter(|value| value.identity == base.resolved.identity)
                .cloned()
            else {
                return;
            };
            ModelCapabilityPatch::Upsert(draft)
        };
        self.next_model_capability_operation = self.next_model_capability_operation.wrapping_add(1);
        let operation_id = self.next_model_capability_operation;
        self.model_capabilities = ModelCapabilitySettingsProjection::Saving {
            generation,
            operation_id,
            model: base.clone(),
        };
        cx.emit(ModelCapabilitySaveRequested {
            generation,
            operation_id,
            base,
            patch,
        });
        cx.notify();
    }

    pub(crate) fn cancel_model_capability(&mut self, cx: &mut Context<Self>) {
        if matches!(
            self.model_capabilities,
            ModelCapabilitySettingsProjection::Ready { .. }
        ) {
            self.model_capability_draft = None;
            cx.notify();
        }
    }

    fn capability_button(
        &self,
        id: String,
        label: &'static str,
        enabled: bool,
        target: ModelCapabilityFocusTarget,
        listener: impl Fn(&MouseUpEvent, &mut Window, &mut App) + 'static,
        cx: &App,
    ) -> AnyElement {
        capability_control(
            id,
            label.into(),
            None,
            enabled,
            self.model_capability_focus(&target),
            listener,
            theme(cx).colors,
        )
    }

    fn capability_actions(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let ready = matches!(
            self.model_capabilities,
            ModelCapabilitySettingsProjection::Ready { .. }
        );
        let dirty = self
            .model_capability_models
            .get(index)
            .is_some_and(|model| {
                self.model_capability_draft
                    .as_ref()
                    .is_some_and(|draft| draft.identity == model.resolved.identity)
            });
        settings_actions()
            .child(self.capability_button(
                format!("model-capability-save-{index}"),
                t("settings.action.save"),
                ready && dirty,
                ModelCapabilityFocusTarget::Save(index),
                cx.listener(move |this, _, _, cx| this.save_model_capability(index, false, cx)),
                cx,
            ))
            .child(self.capability_button(
                format!("model-capability-cancel-{index}"),
                t("settings.action.cancel"),
                ready && dirty,
                ModelCapabilityFocusTarget::Cancel(index),
                cx.listener(|this, _, _, cx| this.cancel_model_capability(cx)),
                cx,
            ))
            .child(self.capability_button(
                format!("model-capability-reset-{index}"),
                t("settings.capabilities.reset"),
                ready,
                ModelCapabilityFocusTarget::Reset(index),
                cx.listener(move |this, _, _, cx| this.save_model_capability(index, true, cx)),
                cx,
            ))
            .into_any_element()
    }

    fn capability_status(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let error = match &self.model_capabilities {
            ModelCapabilitySettingsProjection::Ready { error, .. } => *error,
            _ => None,
        };
        let status = match &self.model_capabilities {
            ModelCapabilitySettingsProjection::Loading => Some(t("settings.capabilities.loading")),
            ModelCapabilitySettingsProjection::Saving { .. } => Some(t("settings.saving")),
            _ => None,
        };
        settings_section()
            .child(self.capability_button(
                "model-capability-reload".into(),
                t("settings.action.reload"),
                !matches!(
                    self.model_capabilities,
                    ModelCapabilitySettingsProjection::Saving { .. }
                ),
                ModelCapabilityFocusTarget::Reload,
                cx.listener(|_, _, _, cx| cx.emit(ModelCapabilityReloadRequested)),
                cx,
            ))
            .children(status.map(|value| settings_status(value, colors.text_secondary, colors)))
            .children(error.map(|error| {
                settings_status(
                    match error {
                        ModelCapabilitySettingsErrorCode::Io => t("settings.capabilities.error.io"),
                        ModelCapabilitySettingsErrorCode::Invalid => {
                            t("settings.capabilities.error.invalid")
                        }
                        ModelCapabilitySettingsErrorCode::Conflict => {
                            t("settings.capabilities.error.conflict")
                        }
                        ModelCapabilitySettingsErrorCode::Busy => {
                            t("settings.capabilities.error.busy")
                        }
                        ModelCapabilitySettingsErrorCode::MissingModel => {
                            t("settings.capabilities.error.missing")
                        }
                    },
                    colors.danger,
                    colors,
                )
            }))
            .into_any_element()
    }

    pub(crate) fn render_model_capability_efforts(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let mut section = settings_section()
            .child(section_title(
                t("settings.capabilities.efforts"),
                colors.text_primary,
            ))
            .child(self.capability_status(cx));
        for (index, model) in self.model_capability_models.clone().into_iter().enumerate() {
            if self
                .model_settings
                .as_ref()
                .is_some_and(|(provider, name)| {
                    *provider != model.resolved.identity.provider
                        || *name != model.resolved.identity.model
                })
            {
                continue;
            }
            let draft = self.editable_capability(index);
            let allowed = draft
                .as_ref()
                .and_then(|draft| draft.allowed_efforts.as_ref())
                .unwrap_or(&model.resolved.capabilities.allowed_efforts);
            let mut options = settings_actions();
            for &effort in REASONING_EFFORTS {
                let supported = model
                    .resolved
                    .capabilities
                    .encodable_efforts
                    .iter()
                    .any(|value| value == effort);
                let enabled = supported
                    && matches!(
                        self.model_capabilities,
                        ModelCapabilitySettingsProjection::Ready { .. }
                    );
                let selected = allowed.iter().any(|value| value == effort);
                let label = if supported {
                    effort_label(effort).to_string()
                } else {
                    t_args(
                        "settings.capabilities.unsupported",
                        &[("label", effort_label(effort))],
                    )
                };
                options = options.child(capability_control(
                    format!("model-capability-effort-{index}-{effort}"),
                    label,
                    Some(selected),
                    enabled,
                    self.model_capability_focus(&ModelCapabilityFocusTarget::Effort {
                        index,
                        effort: effort.into(),
                    }),
                    cx.listener(move |this, _, _, cx| {
                        this.toggle_capability_effort(index, effort, cx)
                    }),
                    colors,
                ));
            }
            if self.model_settings.is_none() {
                section = section.child(div().child(format!(
                    "{} / {}",
                    model.resolved.identity.provider, model.resolved.identity.model
                )));
            }
            section = section
                .child(options)
                .child(self.capability_actions(index, cx));
        }
        section.into_any_element()
    }

    pub(crate) fn render_model_capability_image(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let mut section = settings_section();
        for (index, model) in self.model_capability_models.clone().into_iter().enumerate() {
            if self.model_settings.as_ref().is_none_or(|(provider, name)| {
                *provider != model.resolved.identity.provider
                    || *name != model.resolved.identity.model
            }) {
                continue;
            }
            let selected = self
                .editable_capability(index)
                .and_then(|draft| draft.image_input)
                .unwrap_or(model.resolved.capabilities.image_input);
            let enabled = matches!(
                self.model_capabilities,
                ModelCapabilitySettingsProjection::Ready { .. }
            );
            section = section
                .child(capability_control(
                    format!("model-capability-image-{index}"),
                    t("settings.capabilities.image_input").into(),
                    Some(selected),
                    enabled,
                    self.model_capability_focus(&ModelCapabilityFocusTarget::Image(index)),
                    cx.listener(move |this, _, _, cx| this.toggle_capability_image(index, cx)),
                    colors,
                ))
                .child(self.capability_actions(index, cx));
        }
        section.child(self.capability_status(cx)).into_any_element()
    }
}

fn effort_label(effort: &str) -> &'static str {
    match effort {
        "minimal" => t("settings.reasoning.effort.minimal"),
        "low" => t("settings.reasoning.effort.low"),
        "medium" => t("settings.reasoning.effort.medium"),
        "high" => t("settings.reasoning.effort.high"),
        "xhigh" => t("settings.reasoning.effort.xhigh"),
        "max" => t("settings.reasoning.effort.max"),
        _ => t("settings.reasoning.effort.unknown"),
    }
}

fn capability_control(
    id: String,
    label: String,
    checked: Option<bool>,
    enabled: bool,
    focus: Option<FocusHandle>,
    listener: impl Fn(&MouseUpEvent, &mut Window, &mut App) + 'static,
    colors: vega_theme::ThemeColors,
) -> AnyElement {
    let listener = std::rc::Rc::new(listener);
    let keyboard = listener.clone();
    let button = settings_action_chrome(colors)
        .id(id)
        .role(gpui_kit::Role::Button)
        .aria_label(label.clone())
        .when_some(checked, |button, checked| {
            button.aria_toggled(checked.into())
        })
        .interactive(colors, enabled, checked == Some(true));
    button
        .when(enabled, |button| button.focusable().tab_stop(true))
        .when_some(focus.filter(|_| enabled), |button, focus| {
            button.track_focus(&focus)
        })
        .focus_visible(move |style| style.border_color(colors.accent))
        .text_color(if !enabled {
            colors.text_tertiary
        } else if checked == Some(true) {
            colors.accent
        } else {
            colors.text_primary
        })
        .when(enabled, |button| {
            button
                .cursor_pointer()
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
        })
        .child(label)
        .into_any_element()
}
