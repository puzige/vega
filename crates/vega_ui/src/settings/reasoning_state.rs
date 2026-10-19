use super::*;

fn reasoning_profile_for_edit(
    profiles: &[ReasoningProfileProjection],
    draft: Option<&ReasoningProfileProjection>,
    index: usize,
) -> Option<ReasoningProfileProjection> {
    let authority = profiles.get(index)?;
    draft
        .filter(|candidate| {
            candidate.provider == authority.provider && candidate.model == authority.model
        })
        .cloned()
        .or_else(|| Some(authority.clone()))
}

pub(crate) fn reasoning_choices(profile: &ReasoningProfileProjection) -> Vec<ReasoningChoice> {
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
    choices
}

impl SettingsView {
    pub(crate) fn select_reasoning_preference(
        &mut self,
        index: usize,
        choice: ReasoningChoice,
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
        if profile.preference == choice || !reasoning_choices(&profile).contains(&choice) {
            return;
        }
        profile.preference = choice;
        self.emit_reasoning_save(index, profile, cx);
    }

    pub(crate) fn rebuild_reasoning_focuses(&mut self, cx: &mut Context<Self>) {
        let mut targets = Vec::new();
        if let ReasoningSettingsProjection::Ready { profiles, .. } = &self.reasoning {
            targets.push(ReasoningFocusTarget::Reload);
            for (index, authority) in profiles.iter().enumerate() {
                if self
                    .model_settings
                    .as_ref()
                    .is_some_and(|(provider, model)| {
                        authority.provider != *provider || authority.model != *model
                    })
                {
                    continue;
                }
                let profile =
                    reasoning_profile_for_edit(profiles, self.reasoning_draft.as_ref(), index)
                        .unwrap_or_else(|| authority.clone());
                for choice in reasoning_choices(&profile) {
                    targets.push(ReasoningFocusTarget::Preference { index, choice });
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
