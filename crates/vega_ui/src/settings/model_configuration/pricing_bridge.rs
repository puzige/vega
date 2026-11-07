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

#[cfg(test)]
mod tests {
    use super::*;

    fn draft() -> NavigationDraft {
        let draft = Draft::new(std::array::from_fn(|_| "0.00000001".to_owned()));
        NavigationDraft {
            descriptor: PricingEditor {
                kind: PricingEditorKind::AddCustom,
                model: None,
            },
            observed: draft.read(),
            draft,
        }
    }

    #[test]
    fn newer_price_text_and_sessions_invalidate_guard_authority() {
        let old = draft();
        let mut current = old.clone();
        assert!(current.draft.edit(1, Some("0.00000002"), None));
        current.refresh();
        assert!(old != current);
        assert!(old != draft());
        assert!(current.draft.claim(Some(&old.observed)).is_none());
        assert_eq!(
            current.draft.claim(Some(&current.observed)).unwrap()[1],
            "0.00000002"
        );
    }

    #[test]
    fn composing_and_frozen_prices_cannot_be_claimed() {
        let mut current = draft();
        assert!(current.draft.edit(1, None, Some(true)));
        current.refresh();
        assert!(!current.can_claim());
        assert!(current.draft.claim(Some(&current.observed)).is_none());
        assert!(current.draft.edit(1, None, Some(false)));
        current.refresh();
        assert!(current.can_claim());
        assert!(current.draft.claim(Some(&current.observed)).is_some());
        current.refresh();
        assert!(!current.can_claim());
        assert!(!current.draft.edit(1, Some("late"), None));
    }
}
