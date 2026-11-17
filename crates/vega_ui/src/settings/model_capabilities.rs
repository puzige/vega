use super::*;
mod nsh_capabilities;
pub(super) use nsh_capabilities::Presentations;

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
        cx.notify();
    }

    pub fn model_capability_save_is_current(&self, generation: u64, operation_id: u64) -> bool {
        matches!(&self.model_capabilities, ModelCapabilitySettingsProjection::Saving { generation: current, operation_id: operation, .. } if *current == generation && *operation == operation_id)
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
