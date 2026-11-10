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
            interrupted: None,
            resume: false,
            pending_behavior: PendingMessageBehavior::Queue,
            appearance: vega_theme::Appearance::Light,
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
fn stable_presentation_preserves_identity_but_real_feedback_and_theme_changes_publish() {
    let original = snapshot();
    assert!(original.same_presentation(&original.clone()));
    let mut changed = original.clone();
    changed.source.appearance = vega_theme::Appearance::Dark;
    assert!(!original.same_presentation(&changed));
    let mut changed = original.clone();
    changed.selected = Some(0);
    assert!(!original.same_presentation(&changed));
    let mut changed = original.clone();
    changed.request_activation(0, Origin::Keyboard);
    assert!(!original.same_presentation(&changed));
    let mut changed = original.clone();
    changed.selection = ProjectionId::new();
    assert!(!original.same_presentation(&changed));
    let mut changed = original.clone();
    changed.rows[0].enabled = false;
    assert!(!original.same_presentation(&changed));
    let mut changed = original.clone();
    changed.geometry = Some(Geometry {
        bounds: gpui_kit::Bounds {
            origin: gpui_kit::point(px(0.), px(0.)),
            size: gpui_kit::size(px(32.), px(32.)),
        },
        viewport: gpui_kit::size(px(600.), px(400.)),
        trigger: None,
        foreground: px(400.),
    });
    assert!(!original.same_presentation(&changed));
    let mut changed = original.clone();
    changed.colors = vega_theme::Theme::dark().colors;
    assert!(!original.same_presentation(&changed));
    let mut changed = original.clone();
    changed.colors.brand_primary = vega_theme::Theme::dark().colors.brand_primary;
    assert!(!original.same_presentation(&changed));
}

#[test]
fn running_send_rejects_old_queue_steer_choice_and_input_revision() {
    let mut current = snapshot();
    current.kind = Kind::RunningSend;
    current.rows.truncate(1);
    current.rows[0].checked = None;
    current.rows[0].action = None;
    assert!(current.accepts_activation(&current, 0, Origin::Pointer));
    let queued = current.clone();
    current.source.pending_behavior = PendingMessageBehavior::Steer;
    assert!(!current.accepts_activation(&queued, 0, Origin::Pointer));
    let before_edit = current.clone();
    current.source.revision += 1;
    assert!(!current.accepts_activation(&before_edit, 0, Origin::Pointer));
    assert!(current.accepts_activation(&current, 0, Origin::Pointer));
}

#[test]
fn primary_action_rejects_changed_resume_identity_and_pending_state() {
    let mut current = snapshot();
    current.kind = Kind::Primary;
    current.rows.truncate(1);
    current.rows[0].checked = None;
    current.rows[0].action = None;
    assert!(current.accepts_activation(&current, 0, Origin::Pointer));
    let previous = current.clone();
    current.source.interrupted = Some(vega_conversation::types::InterruptedAssistant {
        message_id: "interrupted-message".into(),
        seq: 42,
    });
    current.source.resume = true;
    assert!(!current.accepts_activation(&previous, 0, Origin::Pointer));
    let previous = current.clone();
    current.source.interrupted.as_mut().expect("tail").seq = 43;
    assert!(!current.accepts_activation(&previous, 0, Origin::Pointer));
    current.kind = Kind::ResumePending;
    current.rows[0].enabled = false;
    for origin in [Origin::Pointer, Origin::Keyboard, Origin::Accessibility] {
        current.request_activation(0, origin);
        assert!(!current.accepts_activation(&current, 0, origin));
    }
    current.rows[0].enabled = true;
    assert!(!current.accepts_activation(&current, 0, Origin::Pointer));
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
