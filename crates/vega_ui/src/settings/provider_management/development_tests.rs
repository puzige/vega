use super::*;

fn snapshot() -> Snapshot {
    Snapshot {
        id: ProjectionId::new(),
        version: ProjectionId::new(),
        visible: true,
        authority: Arc::new(Authority {
            rows: vec![
                ProviderRow {
                    name: "first".into(),
                    url: "https://first.invalid/v1".into(),
                    api: ProviderApi::ChatCompletions,
                    enabled: true,
                    credential: true,
                    models: vec![("existing".into(), String::new())],
                },
                ProviderRow {
                    name: "second".into(),
                    url: "https://second.invalid/v1".into(),
                    api: ProviderApi::Responses,
                    enabled: true,
                    credential: true,
                    models: vec![],
                },
            ],
            selected: Some("first".into()),
            route: None,
            generation: 7,
            operation: 3,
            section: 0,
            form: false,
            existing: true,
            retained_key: false,
            api: ProviderApi::ChatCompletions,
            detail: true,
            saving: false,
            network: false,
            path: true,
            candidates: Some(vec!["existing".into(), "candidate".into()]),
            checked: vec!["candidate".into()],
            statuses: vec![],
            message: None,
            delete: None,
            model: None,
            unavailable: false,
            save_intent: 0,
            language: "en",
        }),
        editor: None,
        colors: vega_theme::Theme::light().colors,
        narrow: false,
        focus: None,
    }
}

#[test]
fn closed_provider_route_rejects_even_a_new_current_snapshot() {
    assert!(provider_visible(0, true));
    assert!(!provider_visible(1, true));
    let mut hidden = snapshot();
    hidden.visible = provider_visible(0, false);
    assert!(!accepts(
        &hidden,
        &hidden,
        &Action::Command(Command::Import)
    ));
}

#[test]
fn returning_to_list_exits_clean_form_and_retires_its_draft() {
    let draft = Draft::new(["first".into(), "url".into(), "model".into()]);
    let mut provider = super::super::ProviderManagement::default();
    let state = &mut provider;
    state.form = true;
    state.detail_open = true;
    state.nsh_draft = Some(draft.clone());
    state.intent_stamp = Some(draft.stamp());
    provider.return_to_list();
    assert!(!provider.form);
    assert!(!provider.detail_open);
    assert!(provider.nsh_draft.is_none());
    assert!(provider.intent_stamp.is_none());
    assert!(!draft.edit(0, Some("late edit"), None));
}

#[test]
fn provider_actions_reject_replaced_authority_hidden_routes_and_old_candidate_identity() {
    let shown = snapshot();
    let action = Action::Command(Command::Import);
    assert!(accepts(&shown, &shown, &action));
    let mut current = shown.clone();
    current.visible = false;
    assert!(!accepts(&current, &shown, &action));
    current = shown.clone();
    current.version = ProjectionId::new();
    assert!(!accepts(&current, &shown, &action));
    current = shown.clone();
    Arc::make_mut(&mut current.authority).selected = Some("second".into());
    assert!(!accepts(&current, &shown, &action));
    current = shown.clone();
    Arc::make_mut(&mut current.authority).operation += 1;
    assert!(!accepts(&current, &shown, &action));
    current = shown.clone();
    Arc::make_mut(&mut current.authority).candidates = Some(vec!["replacement".into()]);
    assert!(!accepts(
        &current,
        &shown,
        &Action::Command(Command::Candidate("candidate".into()))
    ));
    assert!(!allowed(
        &shown,
        &Action::Command(Command::Candidate("existing".into()))
    ));
}

#[test]
fn provider_reordering_is_by_displayed_identity_and_rejects_new_order_or_busy_state() {
    let shown = snapshot();
    let action = Action::Move("first".into(), "second".into());
    assert!(accepts(&shown, &shown, &action));
    let mut current = shown.clone();
    Arc::make_mut(&mut current.authority).rows.swap(0, 1);
    assert!(!accepts(&current, &shown, &action));
    current = shown.clone();
    Arc::make_mut(&mut current.authority).saving = true;
    assert!(!allowed(&current, &action));
    current = shown.clone();
    current.narrow = true;
    assert!(!allowed(&current, &action));
    assert!(!allowed(
        &shown,
        &Action::Move("first".into(), "removed".into())
    ));
}

#[test]
fn form_save_and_navigation_submit_require_exact_session_revision_and_no_ime() {
    let mut shown = snapshot();
    Arc::make_mut(&mut shown.authority).form = true;
    let draft = Draft::new([
        "first".into(),
        "https://first.invalid/v1".into(),
        "existing".into(),
    ]);
    shown.editor = Some(Editor {
        values: draft.read(),
        draft: draft.clone(),
    });
    assert!(accepts(&shown, &shown, &Action::Submit(0)));
    assert!(draft.edit(0, Some("newer"), None));
    let mut current = shown.clone();
    current.editor.as_mut().unwrap().values = draft.read();
    assert!(!accepts(&current, &shown, &Action::Submit(0)));
    assert!(draft.edit(0, None, Some(true)));
    current.editor.as_mut().unwrap().values = draft.read();
    assert!(!allowed(&current, &Action::Submit(0)));
    assert!(draft.edit(0, None, Some(false)));
    current.editor.as_mut().unwrap().values = draft.read();
    Arc::make_mut(&mut current.authority).save_intent = 2;
    assert!(allowed(&current, &Action::Submit(2)));
    assert!(!allowed(&current, &Action::Submit(1)));
    let mut replacement = current.clone();
    let replacement_draft = Draft::new(draft.read().text);
    replacement.editor = Some(Editor {
        values: replacement_draft.read(),
        draft: replacement_draft,
    });
    assert!(!accepts(&replacement, &current, &Action::Submit(2)));
}

#[test]
fn disabled_provider_cannot_discover_or_test_and_delete_requires_current_confirmation() {
    let mut shown = snapshot();
    Arc::make_mut(&mut shown.authority).rows[0].enabled = false;
    assert!(!allowed(&shown, &Action::Command(Command::Discover)));
    assert!(!allowed(
        &shown,
        &Action::Command(Command::Test("existing".into()))
    ));
    let delete = Action::Command(Command::DeleteModel("first".into(), "existing".into()));
    assert!(!allowed(&shown, &delete));
    Arc::make_mut(&mut shown.authority).delete = Some(("first".into(), "existing".into()));
    assert!(allowed(&shown, &delete));
    let old = shown.clone();
    Arc::make_mut(&mut shown.authority).generation += 1;
    assert!(!accepts(&shown, &old, &delete));
}
