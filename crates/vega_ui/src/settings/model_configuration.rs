use super::*;

pub(super) mod nsh_navigation;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsDestination {
    Section(usize),
    ModelSection(usize),
    ProviderCommand,
}

impl SettingsView {
    pub(crate) fn guard_settings_close(
        &mut self,
        _: &CloseSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.guard_default_permission_close(window, cx) {
            cx.stop_propagation();
        } else if self.settings_help_open.take().is_some() {
            window.focus(&self.settings_help_focus, cx);
            cx.stop_propagation();
            cx.notify();
        } else if self.settings_have_draft(cx) {
            self.invalidate_settings_navigation_intent();
            self.pending_settings_close = true;
            self.pending_section = Some(SettingsDestination::ModelSection(self.section));
            cx.stop_propagation();
            cx.notify();
        } else {
            cx.propagate();
        }
    }

    pub(crate) fn navigation_section(&self) -> usize {
        if matches!(self.section, 2 | 3) {
            0
        } else {
            self.section
        }
    }

    pub(crate) fn request_settings_section(&mut self, destination: usize, cx: &mut Context<Self>) {
        self.request_settings_destination(SettingsDestination::Section(destination), cx);
    }

    fn request_settings_destination(
        &mut self,
        destination: SettingsDestination,
        cx: &mut Context<Self>,
    ) {
        if self.default_permission.saving() {
            return;
        }
        self.discard_default_permission_intent();
        if self.settings_have_draft(cx) {
            self.invalidate_settings_navigation_intent();
            self.pending_settings_close = false;
            self.pending_section = Some(destination);
            cx.notify();
            return;
        }
        self.finish_settings_navigation(destination, cx);
    }

    fn finish_settings_navigation(
        &mut self,
        destination: SettingsDestination,
        cx: &mut Context<Self>,
    ) {
        self.invalidate_settings_navigation_intent();
        let before = self.settings_motion_destination();
        if matches!(destination, SettingsDestination::Section(0)) {
            self.remember_model_return_focus();
        }
        let section = match destination {
            SettingsDestination::ProviderCommand => {
                self.pending_section = None;
                self.finish_provider_navigation(cx);
                return;
            }
            SettingsDestination::Section(section) => {
                self.close_model_configuration(cx);
                section
            }
            SettingsDestination::ModelSection(section) => section,
        };
        self.skills_section_changed(section, cx);
        self.section = section;
        self.pending_section = None;
        self.settings_help_open = None;
        self.commit_settings_motion(before);
        cx.notify();
    }

    pub(crate) fn resume_settings_navigation(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(destination) = self.pending_section
            && self.error.is_none()
            && !self.settings_have_draft(cx)
            && !self.settings_navigation_saving()
        {
            self.finish_settings_navigation(destination, cx);
            if self.pending_settings_close {
                self.pending_settings_close = false;
                window.dispatch_action(Box::new(CloseSettings), cx);
            }
        }
    }

    pub(crate) fn settings_have_draft(&self, cx: &App) -> bool {
        self.provider_has_draft(cx)
            || self.pricing_editor.is_some()
            || matches!(
                self.pricing,
                PricingSettingsProjection::Ready {
                    draft_reason: Some(_),
                    ..
                }
            )
            || self.reasoning_draft.is_some()
            || self.model_capability_draft.is_some()
    }

    pub(super) fn settings_navigation_saving(&self) -> bool {
        self.provider_is_saving()
            || matches!(self.reasoning, ReasoningSettingsProjection::Saving { .. })
            || matches!(self.pricing, PricingSettingsProjection::Saving { .. })
            || matches!(
                self.model_capabilities,
                ModelCapabilitySettingsProjection::Saving { .. }
            )
    }

    fn save_settings_before_navigation(
        &mut self,
        pricing: Option<&super::nsh_pricing::navigation_bridge::NavigationDraft>,
        provider: Option<&super::provider_management::ProviderDraftStamp>,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.settings_navigation_saving() || self.pending_section.is_none() {
            return false;
        }
        if self.section == 3 {
            if let PricingSettingsProjection::Ready {
                generation,
                draft_reason: Some(_),
                ..
            } = self.pricing
            {
                cx.emit(PricingRetryRequested { generation });
            } else {
                let Some(pricing) = pricing else {
                    return false;
                };
                if !self.claim_navigation_pricing(pricing, cx) {
                    return false;
                }
                self.submit_pricing_editor_values(cx);
            }
        } else if self.model_capability_draft.is_some() {
            if let Some(index) = self.model_capability_models.iter().position(|model| {
                self.model_capability_draft
                    .as_ref()
                    .is_some_and(|draft| draft.identity == model.resolved.identity)
            }) {
                self.save_model_capability(index, false, cx);
            } else {
                return false;
            }
        } else if self.section == 2 {
            if let (Some(draft), ReasoningSettingsProjection::Ready { profiles, .. }) =
                (self.reasoning_draft.clone(), &self.reasoning)
                && let Some(index) = profiles.iter().position(|profile| {
                    profile.provider == draft.provider && profile.model == draft.model
                })
            {
                self.emit_reasoning_save(index, draft, cx);
            } else {
                return false;
            }
        } else {
            let Some(provider) = provider else {
                return false;
            };
            return self.request_provider_nsh_save_observed(provider, cx);
        }
        true
    }

    fn discard_settings_before_navigation(
        &mut self,
        pricing: Option<&super::nsh_pricing::navigation_bridge::NavigationDraft>,
        provider: Option<&super::provider_management::ProviderDraftStamp>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.settings_navigation_saving() || self.pending_section.is_none() {
            return false;
        }
        if !self.invalidate_navigation_pricing(pricing) {
            return false;
        }
        if let Some(provider) = provider
            && !self.discard_provider_nsh_observed(provider, cx)
        {
            return false;
        }
        if let PricingSettingsProjection::Ready {
            generation,
            draft_reason: Some(_),
            ..
        } = self.pricing
        {
            cx.emit(PricingDiscardRequested { generation });
        }
        self.pricing_editor = None;
        self.pricing_nsh_draft = None;
        self.reasoning_draft = None;
        self.model_capability_draft = None;
        if let Some(destination) = self.pending_section {
            if provider.is_none() {
                self.discard_provider_draft(cx);
            }
            if self.settings_have_draft(cx) {
                cx.notify();
                return true;
            }
            self.finish_settings_navigation(destination, cx);
            if self.pending_settings_close {
                self.pending_settings_close = false;
                window.dispatch_action(Box::new(CloseSettings), cx);
            }
        }
        true
    }

    pub(super) fn invalidate_settings_navigation_intent(&mut self) {
        self.settings_navigation_presentation = None;
    }

    fn stay_in_settings(&mut self, cx: &mut Context<Self>) {
        self.invalidate_settings_navigation_intent();
        self.pending_section = None;
        self.pending_settings_close = false;
        cx.notify();
    }
}
