use super::*;

#[derive(Clone)]
pub(in crate::settings) struct NavigationDraft {
    descriptor: PricingEditor,
    draft: Draft,
    observed: Values,
}

impl PartialEq for NavigationDraft {
    fn eq(&self, other: &Self) -> bool {
        self.descriptor == other.descriptor
            && self.draft.session.matches(&other.draft.session)
            && self.observed == other.observed
    }
}

impl NavigationDraft {
    pub(in crate::settings) fn refresh(&mut self) {
        self.observed = self.draft.read();
    }

    pub(in crate::settings) fn can_claim(&self) -> bool {
        !self.observed.frozen && !self.observed.composing.contains(&true)
    }
}

impl SettingsView {
    pub(in crate::settings) fn navigation_pricing_draft(&self) -> Option<NavigationDraft> {
        self.pricing_editor
            .clone()
            .zip(self.pricing_nsh_draft.clone())
            .map(|(descriptor, draft)| NavigationDraft {
                descriptor,
                observed: draft.read(),
                draft,
            })
    }

    pub(in crate::settings) fn claim_navigation_pricing(
        &mut self,
        observed: &NavigationDraft,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(current) = self.navigation_pricing_draft() else {
            return false;
        };
        current == *observed && self.claim_pricing_draft(Some(&observed.observed), cx)
    }

    pub(in crate::settings) fn invalidate_navigation_pricing(
        &self,
        observed: Option<&NavigationDraft>,
    ) -> bool {
        match (self.navigation_pricing_draft(), observed) {
            (None, None) => true,
            (Some(current), Some(observed)) if current == *observed => {
                let mut values = current
                    .draft
                    .values
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                if *values != observed.observed || values.frozen {
                    return false;
                }
                values.frozen = true;
                true
            }
            _ => false,
        }
    }
}
