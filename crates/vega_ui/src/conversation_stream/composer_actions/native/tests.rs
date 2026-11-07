use super::*;

pub(super) fn snapshot() -> Snapshot {
    Snapshot {
        id: ProjectionId::new(),
        epoch: ProjectionId::new(),
        source: Source {
            thread: Thread {
                last_response_failed: false,
                reasoning_preferences: None,
                parent_thread_id: None,
                forked_from_thread_id: None,
                forked_from_message_id: None,
                id: "synthetic-thread".into(),
                backend: TaskBackend::Native,
                project_id: "synthetic-project".into(),
                title: "Synthetic task".into(),
                mode: ThreadMode::Ask,
                permission_mode: PermissionMode::ReadOnly,
                permission_revision: 1,
                draft_authority_epoch: None,
                full_access_grant: None,
                default_permission: None,
                model: "synthetic-model".into(),
                status: vega_conversation::types::ThreadStatus::Active,
                pinned: false,
                unread: false,
                created_at: 0,
                updated_at: 0,
            },
            input: gpui_kit::EntityId::from(1_u64),
            revision: 3,
            generation: 7,
            control_generation: 2,
            menu: true,
            slash: None,
            skill_query: None,
            projection_generation: 5,
            projection: None,
            models: Vec::new(),
            defaults: ComposerDefaults::default(),
            pending_model: None,
            model_owner: None,
            next_model: None,
            active_agent: None,
            active_sequence: None,
            active_response: None,
            pending_mode: None,
            pending_mode_revision: 0,
            state: vec![false; 21],
            attachments: 0,
        },
        kind: Kind::Menu,
        rows: (0..12)
            .map(|index| Row {
                action: Some(ComposerActionRow::Command(CommandId::Ask)),
                label: format!("Action {index}"),
                accessible: format!("Action {index}"),
                description: None,
                source: None,
                token: Some("/ask"),
                reason: None,
                checked: Some(index == 0),
                enabled: true,
            })
            .collect(),
        query: None,
        status: None,
        highlight: 0,
        navigation: 0,
        selected: None,
        selection: ProjectionId::new(),
        colors: vega_theme::Theme::light().colors,
        geometry: None,
        activation: None,
    }
}

#[test]
fn input_highlight_reveals_without_host_selection_and_requires_current_display() {
    let first = snapshot();
    let mut next = first.clone();
    next.highlight = 11;
    next.navigation += 1;
    assert_eq!(next.selected, None);
    assert_eq!(next.reveal_target(), Some(11));
    assert!(!next.accepts_input_display(&first, 0, true));
    assert!(!next.accepts_input_display(&next, 11, false));
    assert!(next.accepts_input_display(&next, 11, true));
    let before_round_trip = next.clone();
    next.highlight = 10;
    next.navigation += 1;
    next.highlight = 11;
    next.navigation += 1;
    assert!(!next.accepts_input_display(&before_round_trip, 11, true));
    next.rows[11].enabled = false;
    assert!(!next.accepts_input_display(&next, 11, true));
}

#[test]
fn focus_departure_cancels_intent_in_both_old_and_republished_observations() {
    let mut current = snapshot();
    current.request_activation(2, Origin::Keyboard);
    let queued = current.clone();
    assert!(current.accepts_activation(&queued, 2, Origin::Keyboard));
    current.cancel_intent();
    let republished = current.clone();
    assert_eq!(republished.activation, None);
    assert!(!current.accepts_activation(&queued, 2, Origin::Keyboard));
    assert!(!current.accepts_activation(&republished, 2, Origin::Keyboard));
    assert!(!current.accepts_activation(&queued, 2, Origin::Pointer));
}

#[test]
fn explicit_pointer_and_ax_origins_cannot_borrow_keyboard_intent() {
    let mut current = snapshot();
    assert!(current.accepts_activation(&current, 3, Origin::Pointer));
    assert!(!current.accepts_activation(&current, 3, Origin::Keyboard));
    current.request_activation(3, Origin::Accessibility);
    assert!(current.accepts_activation(&current, 3, Origin::Accessibility));
    assert!(!current.accepts_activation(&current, 3, Origin::Keyboard));
    let displayed_ax = current.clone();
    current.request_activation(4, Origin::Keyboard);
    assert!(!current.accepts_activation(&displayed_ax, 3, Origin::Accessibility));
    current.rows[4].enabled = false;
    assert!(!current.accepts_activation(&current, 4, Origin::Pointer));
    assert!(!current.accepts_activation(&current, 4, Origin::Keyboard));
}

#[test]
fn display_decision_rejects_revision_route_epoch_and_mount_replacement() {
    let observed = snapshot();
    let changes: &[fn(&mut Snapshot)] = &[
        |value| value.source.revision += 1,
        |value| value.source.generation += 1,
        |value| value.source.projection_generation += 1,
        |value| value.source.thread.id = "different-thread".into(),
        |value| value.source.thread.model = "different-model".into(),
        |value| value.source.state[6] = true,
        |value| value.epoch = ProjectionId::new(),
        |value| value.id = ProjectionId::new(),
        |value| value.selection = ProjectionId::new(),
    ];
    for change in changes {
        let mut current = observed.clone();
        change(&mut current);
        assert!(!current.accepts_input_display(&observed, 0, true));
        assert!(!current.accepts_activation(&observed, 0, Origin::Pointer));
    }
}
