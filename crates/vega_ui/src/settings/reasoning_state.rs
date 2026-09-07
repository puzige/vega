use super::*;

/// The effort identifiers accepted by the explicit Settings editor. They are
/// wire values inside the model, but render helpers translate them to
/// user-facing labels.
pub(crate) const REASONING_EFFORTS: [&str; 6] =
    ["minimal", "low", "medium", "high", "xhigh", "max"];

fn default_efforts(protocol: ReasoningProtocol, model: &str) -> Vec<String> {
    if matches!(protocol, ReasoningProtocol::ZhipuChatCompletions)
        && matches!(model, "glm-5.3" | "glm-5.3-flash")
    {
        return ["low", "high", "max"]
            .into_iter()
            .map(str::to_string)
            .collect();
    }
    match protocol {
        ReasoningProtocol::OpenAiChatCompletions => {
            REASONING_EFFORTS.into_iter().map(str::to_string).collect()
        }
        ReasoningProtocol::ZhipuChatCompletions => ["low", "high", "max"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        ReasoningProtocol::Unknown => Vec::new(),
    }
}

pub(crate) fn is_legal_effort(profile: &ReasoningProfileProjection, effort: &str) -> bool {
    profile.protocol != ReasoningProtocol::Unknown
        && !matches!(
            profile.support,
            ReasoningSupport::Unsupported | ReasoningSupport::Unknown
        )
        && REASONING_EFFORTS.contains(&effort)
        && !(matches!(profile.protocol, ReasoningProtocol::ZhipuChatCompletions)
            && matches!(profile.model.as_str(), "glm-5.3" | "glm-5.3-flash")
            && !matches!(effort, "low" | "high" | "max"))
}

pub(crate) fn standard_glm(profile: &ReasoningProfileProjection) -> bool {
    matches!(profile.protocol, ReasoningProtocol::ZhipuChatCompletions)
        && matches!(profile.model.as_str(), "glm-5.3" | "glm-5.3-flash")
}

fn reasoning_profile_for_edit(
    profiles: &[ReasoningProfileProjection],
    draft: Option<&ReasoningProfileProjection>,
    index: usize,
) -> Option<ReasoningProfileProjection> {
    let authority = profiles.get(index)?;
    // A failed edit belongs to one exact provider/model pair. Never let a
    // draft from row A become the base for row B merely because the row index
    // happens to be the same.
    draft
        .filter(|candidate| {
            candidate.provider == authority.provider && candidate.model == authority.model
        })
        .cloned()
        .or_else(|| Some(authority.clone()))
}

fn apply_protocol_template(
    base: &ReasoningProfileProjection,
    template: ReasoningTemplate,
) -> ReasoningProfileProjection {
    let mut profile = base.clone();
    match template {
        ReasoningTemplate::OpenAi => {
            profile.protocol = ReasoningProtocol::OpenAiChatCompletions;
            profile.support = ReasoningSupport::Optional;
            profile.efforts = default_efforts(profile.protocol, &profile.model);
            profile.supports_disabled = true;
            profile.disabled_wire = Some(ReasoningDisabledWire::ReasoningEffortNone);
            profile.preserve_reasoning_content = false;
            profile.preference = ReasoningChoice::ProviderDefault;
        }
        ReasoningTemplate::Zhipu => {
            profile.protocol = ReasoningProtocol::ZhipuChatCompletions;
            profile.support = ReasoningSupport::Required;
            profile.efforts = default_efforts(profile.protocol, &profile.model);
            // Zhipu's standard thinking request has an explicit enabled
            // operation. Disabled is deliberately not assumed here.
            profile.supports_disabled = false;
            profile.disabled_wire = None;
            profile.preserve_reasoning_content = true;
            profile.preference = ReasoningChoice::Effort("high".to_string());
            if !profile.efforts.iter().any(|effort| effort == "high") {
                profile.preference = profile
                    .efforts
                    .first()
                    .cloned()
                    .map(ReasoningChoice::Effort)
                    .unwrap_or(ReasoningChoice::ProviderDefault);
            }
        }
    }
    profile
}

fn reasoning_choices(profile: &ReasoningProfileProjection) -> Vec<ReasoningChoice> {
    // Required means the provider enables thinking, not that the user must
    // pick a named effort. Omitting the effort lets the provider choose its
    // own legal default and is distinct from an explicit Disabled choice.
    let mut choices = vec![ReasoningChoice::ProviderDefault];
    if profile.supports_disabled {
        choices.push(ReasoningChoice::Disabled);
    }
    if !matches!(
        profile.support,
        ReasoningSupport::Unsupported | ReasoningSupport::Unknown
    ) {
        choices.extend(profile.efforts.iter().cloned().map(ReasoningChoice::Effort));
    }
    if choices.is_empty() {
        choices.push(ReasoningChoice::ProviderDefault);
    }
    choices
}

impl SettingsView {
    pub(crate) fn cycle_reasoning_preference(&mut self, index: usize, cx: &mut Context<Self>) {
        let ReasoningSettingsProjection::Ready { profiles, .. } = &self.reasoning else {
            return;
        };
        let Some(mut profile) =
            reasoning_profile_for_edit(profiles, self.reasoning_draft.as_ref(), index)
        else {
            return;
        };
        let choices = reasoning_choices(&profile);
        if choices.len() < 2 {
            return;
        }
        let current = profile.preference.clone();
        let current_index = choices
            .iter()
            .position(|choice| *choice == current)
            .unwrap_or(0);
        profile.preference = choices[(current_index + 1) % choices.len()].clone();
        self.emit_reasoning_save(index, profile, cx);
    }

    /// Applies a user-selected capability template to one exact
    /// provider/model row. The resulting profile remains a normal precise
    /// save request, so the controller still owns validation, merge, and
    /// durable acknowledgement.
    pub fn apply_reasoning_template(
        &mut self,
        index: usize,
        template: ReasoningTemplate,
        cx: &mut Context<Self>,
    ) {
        let ReasoningSettingsProjection::Ready { profiles, .. } = &self.reasoning else {
            return;
        };
        let Some(base) = reasoning_profile_for_edit(profiles, self.reasoning_draft.as_ref(), index)
        else {
            return;
        };
        self.emit_reasoning_save(index, apply_protocol_template(&base, template), cx);
    }

    /// Cycles the declared protocol through the explicit choices. Selecting
    /// Unknown clears all controls and therefore returns to provider default;
    /// it is never inferred from an endpoint or model name.
    pub(crate) fn cycle_reasoning_protocol(&mut self, index: usize, cx: &mut Context<Self>) {
        let ReasoningSettingsProjection::Ready { profiles, .. } = &self.reasoning else {
            return;
        };
        let Some(mut profile) =
            reasoning_profile_for_edit(profiles, self.reasoning_draft.as_ref(), index)
        else {
            return;
        };
        let next = match profile.protocol {
            ReasoningProtocol::Unknown => ReasoningProtocol::OpenAiChatCompletions,
            ReasoningProtocol::OpenAiChatCompletions => ReasoningProtocol::ZhipuChatCompletions,
            ReasoningProtocol::ZhipuChatCompletions => ReasoningProtocol::Unknown,
        };
        profile = if next == ReasoningProtocol::Unknown {
            ReasoningProfileProjection::unknown(profile.provider, profile.model)
        } else {
            apply_protocol_template(
                &profile,
                if next == ReasoningProtocol::OpenAiChatCompletions {
                    ReasoningTemplate::OpenAi
                } else {
                    ReasoningTemplate::Zhipu
                },
            )
        };
        self.emit_reasoning_save(index, profile, cx);
    }

    /// Cycles the support declaration. A disabled/unknown declaration has no
    /// editable controls until the user explicitly chooses a supported state.
    pub(crate) fn cycle_reasoning_support(&mut self, index: usize, cx: &mut Context<Self>) {
        let ReasoningSettingsProjection::Ready { profiles, .. } = &self.reasoning else {
            return;
        };
        let Some(mut profile) =
            reasoning_profile_for_edit(profiles, self.reasoning_draft.as_ref(), index)
        else {
            return;
        };
        profile.support = match profile.support {
            ReasoningSupport::Unknown => ReasoningSupport::Required,
            ReasoningSupport::Required => ReasoningSupport::Optional,
            ReasoningSupport::Optional => ReasoningSupport::Unsupported,
            ReasoningSupport::Unsupported => ReasoningSupport::Required,
        };
        if matches!(
            profile.support,
            ReasoningSupport::Required | ReasoningSupport::Optional
        ) && profile.efforts.is_empty()
        {
            profile.efforts = default_efforts(profile.protocol, &profile.model);
        }
        if matches!(
            profile.support,
            ReasoningSupport::Unsupported | ReasoningSupport::Unknown
        ) {
            profile.efforts.clear();
            profile.supports_disabled = false;
            profile.disabled_wire = None;
            profile.preserve_reasoning_content = false;
            profile.preference = ReasoningChoice::ProviderDefault;
        }
        self.emit_reasoning_save(index, profile, cx);
    }

    /// Toggles one explicitly declared effort value. Standard GLM profiles
    /// are restricted to low/high/max by the same model-aware rule as the
    /// store/runtime validators.
    pub(crate) fn toggle_reasoning_effort(
        &mut self,
        index: usize,
        effort: &'static str,
        cx: &mut Context<Self>,
    ) {
        let ReasoningSettingsProjection::Ready { profiles, .. } = &self.reasoning else {
            return;
        };
        let Some(mut profile) =
            reasoning_profile_for_edit(profiles, self.reasoning_draft.as_ref(), index)
        else {
            return;
        };
        if !is_legal_effort(&profile, effort) {
            return;
        }
        if let Some(position) = profile
            .efforts
            .iter()
            .position(|candidate| candidate == effort)
        {
            profile.efforts.remove(position);
            if profile.preference == ReasoningChoice::Effort(effort.to_string()) {
                profile.preference = ReasoningChoice::ProviderDefault;
            }
        } else {
            profile.efforts.push(effort.to_string());
            profile.efforts.sort_by_key(|candidate| {
                REASONING_EFFORTS
                    .iter()
                    .position(|known| known == candidate)
                    .unwrap_or(REASONING_EFFORTS.len())
            });
        }
        self.emit_reasoning_save(index, profile, cx);
    }

    /// Toggles a declared disabled wire operation. Zhipu standard GLM
    /// profiles intentionally have no disabled action.
    pub(crate) fn toggle_reasoning_disabled(&mut self, index: usize, cx: &mut Context<Self>) {
        let ReasoningSettingsProjection::Ready { profiles, .. } = &self.reasoning else {
            return;
        };
        let Some(mut profile) =
            reasoning_profile_for_edit(profiles, self.reasoning_draft.as_ref(), index)
        else {
            return;
        };
        if profile.protocol == ReasoningProtocol::Unknown
            || matches!(
                profile.support,
                ReasoningSupport::Unsupported | ReasoningSupport::Unknown
            )
            || standard_glm(&profile)
        {
            return;
        }
        profile.supports_disabled = !profile.supports_disabled;
        profile.disabled_wire = if profile.supports_disabled {
            Some(match profile.protocol {
                ReasoningProtocol::OpenAiChatCompletions => {
                    ReasoningDisabledWire::ReasoningEffortNone
                }
                ReasoningProtocol::ZhipuChatCompletions => {
                    ReasoningDisabledWire::ThinkingTypeDisabled
                }
                ReasoningProtocol::Unknown => return,
            })
        } else {
            None
        };
        if !profile.supports_disabled && profile.preference == ReasoningChoice::Disabled {
            profile.preference = ReasoningChoice::ProviderDefault;
        }
        self.emit_reasoning_save(index, profile, cx);
    }

    pub(crate) fn toggle_reasoning_replay(&mut self, index: usize, cx: &mut Context<Self>) {
        let ReasoningSettingsProjection::Ready { profiles, .. } = &self.reasoning else {
            return;
        };
        let Some(mut profile) =
            reasoning_profile_for_edit(profiles, self.reasoning_draft.as_ref(), index)
        else {
            return;
        };
        if profile.protocol == ReasoningProtocol::Unknown
            || matches!(
                profile.support,
                ReasoningSupport::Unsupported | ReasoningSupport::Unknown
            )
        {
            return;
        }
        profile.preserve_reasoning_content = !profile.preserve_reasoning_content;
        self.emit_reasoning_save(index, profile, cx);
    }

    pub(crate) fn rebuild_reasoning_focuses(&mut self, cx: &mut Context<Self>) {
        let mut targets = Vec::new();
        if let ReasoningSettingsProjection::Ready { profiles, .. } = &self.reasoning {
            targets.push(ReasoningFocusTarget::Reload);
            for (index, authority) in profiles.iter().enumerate() {
                let profile =
                    reasoning_profile_for_edit(profiles, self.reasoning_draft.as_ref(), index)
                        .unwrap_or_else(|| authority.clone());
                if profile.protocol == ReasoningProtocol::Unknown
                    || matches!(profile.support, ReasoningSupport::Unknown)
                {
                    targets.push(ReasoningFocusTarget::Template {
                        index,
                        template: ReasoningTemplate::OpenAi,
                    });
                    targets.push(ReasoningFocusTarget::Template {
                        index,
                        template: ReasoningTemplate::Zhipu,
                    });
                    continue;
                }
                targets.push(ReasoningFocusTarget::Protocol(index));
                targets.push(ReasoningFocusTarget::Support(index));
                for effort in REASONING_EFFORTS {
                    if is_legal_effort(&profile, effort) {
                        targets.push(ReasoningFocusTarget::Effort { index, effort });
                    }
                }
                targets.push(ReasoningFocusTarget::Preference(index));
                if !standard_glm(&profile)
                    && profile.protocol != ReasoningProtocol::Unknown
                    && !matches!(
                        profile.support,
                        ReasoningSupport::Unsupported | ReasoningSupport::Unknown
                    )
                {
                    targets.push(ReasoningFocusTarget::Disabled(index));
                }
                if profile.protocol != ReasoningProtocol::Unknown
                    && !matches!(
                        profile.support,
                        ReasoningSupport::Unsupported | ReasoningSupport::Unknown
                    )
                {
                    targets.push(ReasoningFocusTarget::Replay(index));
                }
            }
        }
        self.reasoning_focuses = targets
            .into_iter()
            .map(|target| (target, cx.focus_handle().tab_stop(true)))
            .collect();
    }

    pub(crate) fn reasoning_focus(&self, target: &ReasoningFocusTarget) -> Option<FocusHandle> {
        self.reasoning_focuses
            .iter()
            .find(|(candidate, _)| candidate == target)
            .map(|(_, focus)| focus.clone())
    }
}
