use super::*;

fn snapshot(pane: Pane) -> Snapshot {
    Snapshot {
        id: ProjectionId::new(),
        pane,
        visible: true,
        submitted: false,
        authority: Authority {
            intent: Intent {
                section: 2,
                model: Some(("provider".into(), "model".into())),
                pending: Some(SettingsDestination::ModelSection(3)),
                closing: false,
            },
            available: true,
            saving: false,
            permission_pending: false,
            has_draft: true,
            provider: NavigationAuthority::development_fixture(),
            pricing: PricingSettingsProjection::Loading,
            pricing_draft: None,
            reasoning: ReasoningSettingsProjection::Ready {
                generation: 4,
                profiles: vec![],
                error: None,
            },
            reasoning_draft: None,
            reasoning_operation: 7,
            capabilities: ModelCapabilitySettingsProjection::Loading,
            capability_models: vec![],
            capability_draft: None,
            capability_operation: 8,
            error: None,
        },
        colors: vega_theme::LIGHT,
    }
}

#[test]
fn guard_actions_reject_new_route_pending_destination_or_close_intent() {
    let old = snapshot(Pane::Guard);
    assert!(accepts(&old, &old, Action::Discard));
    let mut current = old.clone();
    current.authority.intent.model = Some(("provider".into(), "another".into()));
    assert!(!accepts(&current, &old, Action::Save));
    assert!(!accepts(&current, &old, Action::Discard));
    current = old.clone();
    current.authority.intent.pending = Some(SettingsDestination::Section(0));
    assert!(!accepts(&current, &old, Action::Discard));
    current = old.clone();
    current.authority.intent.closing = true;
    assert!(!accepts(&current, &old, Action::Stay));
}

#[test]
fn old_authority_cannot_save_or_discard_new_controller_operations() {
    let old = snapshot(Pane::Guard);
    let mut current = old.clone();
    current.authority.reasoning_operation += 1;
    assert!(!accepts(&current, &old, Action::Save));
    current = old.clone();
    current.authority.capability_operation += 1;
    assert!(!accepts(&current, &old, Action::Discard));
    current = old.clone();
    current.authority.pricing = PricingSettingsProjection::Ready {
        generation: 9,
        entries: vec![],
        notice: None,
        draft_reason: Some(vega_conversation::types::PricingDraftReason::RetryPending),
        error: None,
    };
    assert!(!accepts(&current, &old, Action::Save));
    current = old.clone();
    current.authority.provider.development_edit_key();
    assert!(!accepts(&current, &old, Action::Discard));
}

#[test]
fn provider_guard_rejects_newer_native_draft_and_composition() {
    let old = snapshot(Pane::Guard);
    assert!(accepts(&old, &old, Action::Save));
    assert!(accepts(&old, &old, Action::Discard));
    let mut current = old.clone();
    current.authority.provider.development_edit_draft(false);
    assert!(!accepts(&current, &old, Action::Save));
    assert!(!accepts(&current, &old, Action::Discard));
    current.authority.provider.development_edit_draft(true);
    assert!(!allowed(&current, Action::Save));
    assert!(allowed(&current, Action::Stay));
}

#[test]
fn saving_and_duplicate_submissions_block_destructive_actions_but_not_stay() {
    let mut current = snapshot(Pane::Guard);
    current.authority.saving = true;
    assert!(!allowed(&current, Action::Save));
    assert!(!allowed(&current, Action::Discard));
    assert!(allowed(&current, Action::Stay));
    current.authority.saving = false;
    current.submitted = true;
    assert!(!allowed(&current, Action::Save));
    assert!(!allowed(&current, Action::Discard));
    assert!(allowed(&current, Action::Stay));
}

#[test]
fn hidden_or_reopened_surfaces_reject_queued_actions() {
    let old = snapshot(Pane::Guard);
    let mut current = old.clone();
    current.visible = false;
    assert!(!accepts(&current, &old, Action::Discard));
    current = old.clone();
    current.id = ProjectionId::new();
    assert!(!accepts(&current, &old, Action::Save));
}

#[test]
fn model_tabs_preserve_selected_unavailable_and_library_routes() {
    let mut current = snapshot(Pane::Model);
    assert!(!allowed(&current, Action::Tab(2)));
    assert!(selected(&current, Action::Tab(2)));
    assert!(allowed(&current, Action::Tab(3)));
    assert!(allowed(&current, Action::Back));
    current.authority.available = false;
    assert!(!allowed(&current, Action::Tab(0)));
    assert!(allowed(&current, Action::Tab(3)));
    current.authority.intent.model = None;
    assert!(!allowed(&current, Action::Tab(3)));
    assert!(allowed(&current, Action::Back));
    assert!(!allowed(&current, Action::Discard));
}
