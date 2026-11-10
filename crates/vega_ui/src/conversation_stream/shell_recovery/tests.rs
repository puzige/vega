use super::*;

fn snapshot() -> Snapshot {
    Snapshot {
        id: ProjectionId::new(),
        observation: ProjectionId::new(),
        projection: ProjectionId::new(),
        confirmation: ProjectionId::new(),
        thread: "thread-a".into(),
        project: "project-a".into(),
        backend: TaskBackend::Native,
        draft: false,
        summary: false,
        sequence: 4,
        pending: None,
        error: None,
        opened: true,
        has_projection: true,
        has_confirmation: false,
        submitted: false,
        recovery: false,
        presentation_eligible: true,
        workspace: "/workspace".into(),
        rows: vec![Row {
            operation: "paused-operation".into(),
            thread: "thread-a".into(),
            command: "command".into(),
            exit_code: None,
            reason: "output_incomplete".into(),
            output_incomplete: true,
            can_retry: true,
            can_resume: true,
        }],
        active: Vec::new(),
        colors: vega_theme::LIGHT,
        appearance: vega_theme::Appearance::Light,
        language: "en",
        selected: None,
        selection: ProjectionId::new(),
        keyboard_active: false,
        reveal: false,
    }
}

#[test]
fn draft_presentation_is_hidden_unless_blocked_or_unavailable() {
    use super::super::{RecoveryPresentation, recovery_presentation};
    assert_eq!(
        recovery_presentation(true, false, false),
        RecoveryPresentation::Hidden
    );
    assert_eq!(
        recovery_presentation(true, true, false),
        RecoveryPresentation::Summary("ui.shell_recovery.paused")
    );
    for blocked in [false, true] {
        assert_eq!(
            recovery_presentation(true, blocked, true),
            RecoveryPresentation::Summary("ui.shell_recovery.status_unavailable")
        );
        for unavailable in [false, true] {
            assert_eq!(
                recovery_presentation(false, blocked, unavailable),
                RecoveryPresentation::Details
            );
        }
    }
    let mut state = snapshot();
    state.draft = true;
    state.summary = true;
    state.opened = false;
    state.rows.clear();
    state.active.push(Active {
        operation: "running".into(),
        state: "running".into(),
    });
    assert!(!state.visible());
    assert!(!state.accepts(&state, &Action::Review));
    assert!(!state.accepts(&state, &Action::Cancel("running".into())));
}

#[test]
fn draft_summary_only_offers_review_even_while_read_is_pending() {
    let mut state = snapshot();
    state.draft = true;
    state.summary = true;
    state.opened = false;
    state.pending = Some("ui.shell_recovery.checking");
    state.has_confirmation = true;
    assert!(state.visible());
    assert_eq!(state.controls().len(), 1);
    assert!(state.accepts(&state, &Action::Review));
    assert_eq!(state.initial_selection(), Some(Action::Review));
    for action in [
        Action::Check,
        Action::Retry,
        Action::Prepare,
        Action::Accept,
        Action::NotNow,
    ] {
        assert!(!state.accepts(&state, &action));
    }
    state.rows.clear();
    state.error = Some(ShellRecoveryError::Unavailable);
    assert!(state.visible());
    assert!(state.accepts(&state, &Action::Review));
    state.opened = true;
    assert!(!state.accepts(&state, &Action::Review));
}

#[test]
fn draft_details_keep_close_available_during_prepare_and_submitted_resume() {
    let mut state = snapshot();
    state.draft = true;
    for submitted in [false, true] {
        state.pending = Some("ui.shell_recovery.checking");
        state.submitted = submitted;
        state.has_confirmation = submitted;
        assert!(state.accepts(&state, &Action::NotNow));
        assert_eq!(state.initial_selection(), Some(Action::NotNow));
        assert!(!state.accepts(&state, &Action::Prepare));
        assert!(!state.accepts(&state, &Action::Accept));
    }
    state.recovery = true;
    assert!(state.accepts(&state, &Action::NotNow));
    assert!(state.accepts(&state, &Action::Reload));
    state.rows.clear();
    state.pending = None;
    state.submitted = false;
    state.has_confirmation = false;
    state.error = Some(ShellRecoveryError::Unavailable);
    state.recovery = false;
    assert!(state.accepts(&state, &Action::Read));
    assert!(state.accepts(&state, &Action::NotNow));
}

#[test]
fn draft_summary_overlay_and_persisted_route_cannot_replay_each_others_actions() {
    let mut state = snapshot();
    state.draft = true;
    state.has_confirmation = true;
    let details = state.clone();
    assert!(state.accepts(&details, &Action::Accept));
    state.summary = true;
    state.opened = false;
    assert!(!state.accepts(&details, &Action::Accept));
    let summary = state.clone();
    assert!(state.accepts(&summary, &Action::Review));
    state.summary = false;
    state.opened = true;
    assert!(!state.accepts(&summary, &Action::Review));
    state.draft = false;
    assert!(!state.accepts(&details, &Action::Accept));
}

fn recovery_thread() -> vega_conversation::types::Thread {
    use vega_conversation::types::{PermissionMode, Thread, ThreadMode, ThreadStatus};
    Thread {
        last_response_failed: false,
        reasoning_preferences: None,
        parent_thread_id: None,
        forked_from_thread_id: None,
        forked_from_message_id: None,
        id: "draft-a".into(),
        backend: TaskBackend::Native,
        project_id: "project-a".into(),
        title: String::new(),
        mode: ThreadMode::Execute,
        permission_mode: PermissionMode::Confirm,
        permission_revision: 1,
        draft_authority_epoch: Some("authority-a".into()),
        full_access_grant: None,
        default_permission: None,
        model: "model".into(),
        status: ThreadStatus::Active,
        pinned: false,
        unread: false,
        created_at: 0,
        updated_at: 0,
    }
}

#[test]
fn recovery_completion_requires_exact_route_and_every_upstream_authority_field() {
    use super::super::{ShellRecoveryRequested, same_recovery_authority};
    use vega_conversation::types::{DefaultPermissionSnapshot, PermissionMode, Thread, ThreadMode};
    let source = recovery_thread();
    let request = ShellRecoveryRequested {
        source: source.clone(),
        sequence: 7,
        draft: true,
        action: ShellRecoveryAction::Read,
    };
    assert!(request.matches_route(&source, true, 7));
    assert!(!request.matches_route(&source, false, 7));
    assert!(!request.matches_route(&source, true, 8));
    let mutations: [fn(&mut Thread); 7] = [
        |thread| thread.backend = TaskBackend::Codex,
        |thread| thread.mode = ThreadMode::Ask,
        |thread| thread.permission_mode = PermissionMode::ReadOnly,
        |thread| thread.permission_revision += 1,
        |thread| thread.draft_authority_epoch = Some("authority-b".into()),
        |thread| {
            thread.default_permission = Some(DefaultPermissionSnapshot {
                config_path: None,
                configuration_id: Some("config".into()),
                permission_mode: "confirm".into(),
                permission_revision: 2,
                full_access_grant: None,
            })
        },
        |thread| {
            thread.full_access_grant = Some(serde_json::from_value(serde_json::json!({
            "schema_version": 1, "contract_version": 1, "grant_id": "test-grant",
            "authority_epoch": "test-epoch", "scope": {"kind": "thread", "thread_id": "draft-a"},
            "origin": {"kind": "user_confirmation"}, "issued_at": 0
        })).unwrap())
        },
    ];
    for mutate in mutations {
        let mut current = source.clone();
        mutate(&mut current);
        assert!(!same_recovery_authority(&source, &current));
        assert!(!request.matches_route(&current, true, 7));
    }
    for mutate in [
        (|thread: &mut Thread| thread.id = "draft-b".into()) as fn(&mut Thread),
        |thread: &mut Thread| thread.project_id = "project-b".into(),
    ] {
        let mut current = source.clone();
        mutate(&mut current);
        assert!(!request.matches_route(&current, true, 7));
    }
    let mut cosmetic = source.clone();
    cosmetic.title = "new title".into();
    cosmetic.model = "other model".into();
    cosmetic.updated_at += 1;
    assert!(request.matches_route(&cosmetic, true, 7));
}

#[test]
fn closing_unsubmitted_prepare_invalidates_pending_and_late_confirmation() {
    use super::super::{RecoveryDecision, ShellRecoveryRequested};
    let source = recovery_thread();
    let request = ShellRecoveryRequested {
        source: source.clone(),
        sequence: 7,
        draft: true,
        action: ShellRecoveryAction::Read,
    };
    let mut decision = RecoveryDecision {
        confirmation: Some(Box::new("host-only")),
        submitted: false,
    };
    let mut sequence = 7;
    let mut pending = Some("ui.shell_recovery.checking");
    decision.dismiss(&mut sequence, &mut pending);
    assert!(decision.confirmation.is_none());
    assert!(pending.is_none());
    assert_eq!(sequence, 8);
    assert!(!request.matches_route(&source, true, sequence));
    pending = Some("ui.shell_recovery.checking");
    decision.dismiss(&mut sequence, &mut pending);
    assert_eq!(sequence, 9);
    assert!(pending.is_none());
}

#[test]
fn closing_submitted_resume_preserves_pending_decision_and_matching_completion() {
    use super::super::{RecoveryDecision, ShellRecoveryRequested};
    let source = recovery_thread();
    let request = ShellRecoveryRequested {
        source: source.clone(),
        sequence: 7,
        draft: true,
        action: ShellRecoveryAction::Read,
    };
    let mut decision = RecoveryDecision {
        confirmation: Some(Box::new("host-only")),
        submitted: true,
    };
    let mut sequence = 7;
    let mut pending = Some("ui.shell_recovery.resuming");
    decision.dismiss(&mut sequence, &mut pending);
    assert_eq!(decision.confirmation.as_deref(), Some(&"host-only"));
    assert!(decision.submitted);
    assert_eq!(pending, Some("ui.shell_recovery.resuming"));
    assert_eq!(sequence, 7);
    assert!(request.matches_route(&source, true, sequence));
}

#[test]
fn review_check_retry_prepare_and_confirmation_have_distinct_eligibility() {
    let mut state = snapshot();
    state.opened = false;
    assert!(state.accepts(&state, &Action::Review));
    assert!(!state.accepts(&state, &Action::Prepare));
    state.opened = true;
    for action in [
        Action::Check,
        Action::Retry,
        Action::Prepare,
        Action::NotNow,
    ] {
        assert!(state.accepts(&state, &action));
    }
    assert!(!state.accepts(&state, &Action::Accept));
    state.has_confirmation = true;
    assert!(state.accepts(&state, &Action::Accept));
    assert!(state.accepts(&state, &Action::KeepPaused));
    assert!(!state.accepts(&state, &Action::Prepare));
    assert_eq!(state.initial_selection(), Some(Action::KeepPaused));
}

#[test]
fn pending_and_submitted_confirmation_cannot_resume_again() {
    let mut state = snapshot();
    state.has_confirmation = true;
    let displayed = state.clone();
    assert!(state.accepts(&displayed, &Action::Accept));
    state.pending = Some("ui.shell_recovery.resuming");
    state.submitted = true;
    state.sequence += 1;
    assert!(!state.accepts(&displayed, &Action::Accept));
    assert!(!state.accepts(&state, &Action::Accept));
    state.pending = None;
    assert!(!state.accepts(&state, &Action::Accept));
    assert_eq!(state.initial_selection(), Some(Action::KeepPaused));
}

#[test]
fn stale_surface_source_sequence_projection_and_confirmation_are_rejected() {
    let mut state = snapshot();
    state.has_confirmation = true;
    let observed = state.clone();
    let mutations: [fn(&mut Snapshot); 10] = [
        |state| state.id = ProjectionId::new(),
        |state| state.observation = ProjectionId::new(),
        |state| state.projection = ProjectionId::new(),
        |state| state.confirmation = ProjectionId::new(),
        |state| state.thread = "thread-b".into(),
        |state| state.project = "project-b".into(),
        |state| state.backend = TaskBackend::Codex,
        |state| state.sequence += 1,
        |state| state.opened = false,
        |state| state.rows[0].operation = "other-operation".into(),
    ];
    for mutate in mutations {
        let mut current = state.clone();
        mutate(&mut current);
        assert!(!current.accepts(&observed, &Action::Accept));
    }
}

#[test]
fn row_constraints_disable_retry_and_prepare_without_hiding_check() {
    let mut state = snapshot();
    state.rows[0].can_retry = false;
    state.rows[0].can_resume = false;
    assert!(state.accepts(&state, &Action::Check));
    assert!(!state.accepts(&state, &Action::Retry));
    assert!(!state.accepts(&state, &Action::Prepare));
    state.rows[0].can_retry = true;
    assert!(state.accepts(&state, &Action::Retry));
    assert!(!state.accepts(&state, &Action::Prepare));
}

#[test]
fn cancel_targets_only_the_displayed_running_active_operation() {
    let mut state = snapshot();
    state.active = vec![
        Active {
            operation: "running".into(),
            state: "running".into(),
        },
        Active {
            operation: "stopping".into(),
            state: "stopping".into(),
        },
    ];
    assert!(state.accepts(&state, &Action::Cancel("running".into())));
    assert!(!state.accepts(&state, &Action::Cancel("stopping".into())));
    assert!(!state.accepts(&state, &Action::Cancel("paused-operation".into())));
    let observed = state.clone();
    state.active[0].state = "finishing".into();
    assert!(!state.accepts(&observed, &Action::Cancel("running".into())));
    state.pending = Some("ui.shell_recovery.checking");
    assert!(!state.accepts(&state, &Action::Check));
}

#[test]
fn tab_follows_displayed_order_skips_disabled_and_defaults_safe() {
    let mut state = snapshot();
    state.active.push(Active {
        operation: "stopping".into(),
        state: "stopping".into(),
    });
    state.rows[0].can_retry = false;
    assert_eq!(state.initial_selection(), Some(Action::NotNow));
    assert_eq!(state.next_selection(None, false), Some(Action::Check));
    assert_eq!(
        state.next_selection(Some(&Action::Check), false),
        Some(Action::Prepare)
    );
    assert_eq!(
        state.next_selection(Some(&Action::Prepare), true),
        Some(Action::Check)
    );
    state.has_confirmation = true;
    assert_eq!(state.initial_selection(), Some(Action::KeepPaused));
    assert_eq!(
        state.next_selection(Some(&Action::KeepPaused), true),
        Some(Action::Accept)
    );
    state.pending = Some("ui.shell_recovery.resuming");
    assert_eq!(state.initial_selection(), None);
    assert_eq!(state.next_selection(None, false), None);
}

#[test]
fn ui_recovery_never_exposes_prepare_or_accept_and_reload_has_no_business_intent() {
    let mut state = snapshot();
    state.recovery = true;
    state.has_confirmation = true;
    assert_eq!(state.initial_selection(), Some(Action::KeepPaused));
    assert!(state.accepts(&state, &Action::Reload));
    assert!(!state.accepts(&state, &Action::Accept));
    assert!(!state.accepts(&state, &Action::Prepare));
    let old = state.clone();
    state.id = ProjectionId::new();
    state.recovery = false;
    assert!(!state.accepts(&old, &Action::Accept));
    assert_eq!(state.initial_selection(), Some(Action::KeepPaused));
}

#[test]
fn empty_state_suppresses_controls_but_error_preserves_read_again() {
    let mut state = snapshot();
    state.rows.clear();
    assert!(!state.visible());
    assert!(!state.accepts(&state, &Action::Check));
    state.error = Some(ShellRecoveryError::Unavailable);
    assert!(state.visible());
    assert!(state.accepts(&state, &Action::Read));
    assert!(!state.accepts(&state, &Action::Accept));
}

#[test]
fn reveal_requires_current_native_focus_selection_and_exact_observation() {
    let mut state = snapshot();
    state.selected = Some(Action::NotNow);
    state.keyboard_active = true;
    state.reveal = true;
    let observed = state.clone();
    assert!(state.accepts_reveal(&observed, &Action::NotNow));
    state.selection = ProjectionId::new();
    assert!(!state.accepts_reveal(&observed, &Action::NotNow));
    state = observed.clone();
    state.keyboard_active = false;
    assert!(!state.accepts_reveal(&observed, &Action::NotNow));
    state = observed.clone();
    state.reveal = false;
    assert!(!state.accepts_reveal(&observed, &Action::NotNow));
    state = observed.clone();
    state.opened = false;
    assert!(!state.accepts_reveal(&observed, &Action::NotNow));
}

#[test]
fn reveal_preserves_visible_controls_and_minimally_scrolls_offscreen_controls() {
    let bounds = |top, height| Measurement {
        page: Point {
            x: Points(20.),
            y: Points(top),
        },
        frame: Bounds {
            origin: Point {
                x: Points(0.),
                y: Points(0.),
            },
            size: Size {
                width: Points(100.),
                height: Points(height),
            },
        },
    };
    let viewport = bounds(100., 200.);
    let scroll = ScrollEvent {
        offset: Point {
            x: Points(0.),
            y: Points(250.),
        },
        content_size: Size {
            width: Points(100.),
            height: Points(1000.),
        },
        size: viewport.frame.size,
        zoom_scale: 1.,
    };
    assert_eq!(
        reveal::destination(bounds(120., 30.), viewport, scroll),
        None
    );
    assert_eq!(
        reveal::destination(bounds(310., 30.), viewport, scroll),
        Some(Point {
            x: Points(0.),
            y: Points(290.)
        })
    );
    assert_eq!(
        reveal::destination(bounds(70., 30.), viewport, scroll),
        Some(Point {
            x: Points(0.),
            y: Points(220.)
        })
    );
}

#[test]
fn measured_selection_cannot_authorize_new_or_unseen_keyboard_selection() {
    let mut current = snapshot();
    current.has_confirmation = true;
    current.selected = Some(Action::KeepPaused);
    current.keyboard_active = true;
    let measured = current.clone();
    assert!(current.accepts_selection(&measured, &Action::KeepPaused));
    current.selected = Some(Action::Accept);
    current.selection = ProjectionId::new();
    assert!(!current.accepts_selection(&measured, &Action::Accept));
    let measured_accept = current.clone();
    assert!(current.accepts_selection(&measured_accept, &Action::Accept));
    current.pending = Some("ui.shell_recovery.resuming");
    current.submitted = true;
    assert!(!current.accepts_selection(&measured_accept, &Action::Accept));
}

#[test]
fn traversal_at_panel_boundaries_returns_to_native_focus_order() {
    let current = snapshot();
    assert_eq!(current.next_selection(Some(&Action::Check), true), None);
    assert_eq!(current.next_selection(Some(&Action::NotNow), false), None);
}

#[test]
fn hidden_confirmation_rejects_queued_activation_and_old_surface_replay() {
    let mut current = snapshot();
    current.has_confirmation = true;
    let queued = current.clone();
    assert!(current.accepts(&queued, &Action::Accept));
    current.presentation_eligible = false;
    assert!(!current.visible());
    for action in [Action::Accept, Action::KeepPaused, Action::NotNow] {
        assert!(!current.accepts(&queued, &action));
        assert!(!current.accepts(&current, &action));
    }
    assert!(current.has_confirmation);
    assert!(current.opened);
    assert_eq!(current.sequence, queued.sequence);
    assert_eq!(current.pending, queued.pending);
    current.presentation_eligible = true;
    current.id = ProjectionId::new();
    assert!(!current.accepts(&queued, &Action::Accept));
    assert!(current.accepts(&current, &Action::Accept));
    assert_eq!(current.initial_selection(), Some(Action::KeepPaused));
}

fn measured_selection() -> Snapshot {
    let mut current = snapshot();
    current.selected = Some(Action::NotNow);
    current.keyboard_active = true;
    current.reveal = true;
    current
}

fn scroll_observation(offset: f32, layout: u64) -> reveal::Observation {
    reveal::Observation {
        scroll: ScrollEvent {
            offset: Point {
                x: Points(0.),
                y: Points(offset),
            },
            content_size: Size {
                width: Points(200.),
                height: Points(1000.),
            },
            size: Size {
                width: Points(200.),
                height: Points(200.),
            },
            zoom_scale: 1.,
        },
        layout,
    }
}

#[test]
fn missing_measurement_allows_one_fresh_success_without_selection_change() {
    let current = measured_selection();
    let observation = scroll_observation(0., 1);
    let mut attempts = reveal::Attempts::default();
    let missing = attempts
        .begin(&current, &Action::NotNow, observation)
        .expect("first measure");
    assert!(!attempts.finish(
        &missing,
        &current,
        &Action::NotNow,
        Some(observation),
        reveal::Completion::Retry
    ));
    let fresh = attempts
        .begin(&current, &Action::NotNow, observation)
        .expect("fresh measure");
    assert!(attempts.finish(
        &fresh,
        &current,
        &Action::NotNow,
        Some(observation),
        reveal::Completion::Measured
    ));
    assert!(
        attempts
            .begin(&current, &Action::NotNow, observation)
            .is_none()
    );
    assert!(
        attempts
            .begin(&current, &Action::NotNow, scroll_observation(20., 2))
            .is_none()
    );
}

#[test]
fn scroll_invalidation_requires_fresh_exact_observation_before_acknowledgement() {
    let current = measured_selection();
    let before = scroll_observation(0., 1);
    let after = scroll_observation(120., 1);
    let mut attempts = reveal::Attempts::default();
    let stale = attempts
        .begin(&current, &Action::NotNow, before)
        .expect("first measure");
    assert!(!attempts.finish(
        &stale,
        &current,
        &Action::NotNow,
        Some(after),
        reveal::Completion::Measured
    ));
    let fresh = attempts
        .begin(&current, &Action::NotNow, after)
        .expect("fresh scroll measure");
    assert!(attempts.finish(
        &fresh,
        &current,
        &Action::NotNow,
        Some(after),
        reveal::Completion::Measured
    ));
}

#[test]
fn old_measurement_cannot_complete_or_release_a_new_selection_or_surface() {
    let original = measured_selection();
    let observation = scroll_observation(0., 1);
    for replace_surface in [false, true] {
        let mut attempts = reveal::Attempts::default();
        let old = attempts
            .begin(&original, &Action::NotNow, observation)
            .expect("old measure");
        let mut current = original.clone();
        if replace_surface {
            current.id = ProjectionId::new();
        } else {
            current.selection = ProjectionId::new();
        }
        let fresh = attempts
            .begin(&current, &Action::NotNow, observation)
            .expect("new measure");
        assert!(!attempts.finish(
            &old,
            &current,
            &Action::NotNow,
            Some(observation),
            reveal::Completion::Measured
        ));
        assert!(
            attempts
                .begin(&current, &Action::NotNow, observation)
                .is_none()
        );
        assert!(attempts.finish(
            &fresh,
            &current,
            &Action::NotNow,
            Some(observation),
            reveal::Completion::Measured
        ));
    }
}

#[test]
fn duplicate_notifications_do_not_measure_twice_and_unchanged_failure_stops() {
    let current = measured_selection();
    let observation = scroll_observation(0., 1);
    let mut attempts = reveal::Attempts::default();
    for _ in 0..2 {
        let attempt = attempts
            .begin(&current, &Action::NotNow, observation)
            .expect("bounded measure");
        for _ in 0..10 {
            assert!(
                attempts
                    .begin(&current, &Action::NotNow, observation)
                    .is_none()
            );
        }
        assert!(!attempts.finish(
            &attempt,
            &current,
            &Action::NotNow,
            Some(observation),
            reveal::Completion::Retry
        ));
    }
    for _ in 0..10 {
        assert!(
            attempts
                .begin(&current, &Action::NotNow, observation)
                .is_none()
        );
    }
    let changed = scroll_observation(0., 2);
    let fresh = attempts
        .begin(&current, &Action::NotNow, changed)
        .expect("new layout measure");
    assert!(attempts.finish(
        &fresh,
        &current,
        &Action::NotNow,
        Some(changed),
        reveal::Completion::Measured
    ));
}

#[test]
fn continuously_changing_layout_has_a_finite_per_selection_measurement_budget() {
    let mut current = measured_selection();
    let mut attempts = reveal::Attempts::default();
    for revision in 0..6 {
        let observation = scroll_observation(0., revision);
        let attempt = attempts
            .begin(&current, &Action::NotNow, observation)
            .expect("within total budget");
        assert!(!attempts.finish(
            &attempt,
            &current,
            &Action::NotNow,
            Some(observation),
            reveal::Completion::Retry
        ));
    }
    let changed = scroll_observation(0., 100);
    assert!(attempts.begin(&current, &Action::NotNow, changed).is_none());
    current.selection = ProjectionId::new();
    assert!(attempts.begin(&current, &Action::NotNow, changed).is_some());
}

#[test]
fn host_rejection_is_terminal_and_hidden_or_stale_completion_never_acknowledges() {
    let mut current = measured_selection();
    let before = scroll_observation(0., 1);
    let after = scroll_observation(100., 2);
    let mut attempts = reveal::Attempts::default();
    let rejected = attempts
        .begin(&current, &Action::NotNow, before)
        .expect("measure");
    assert!(!attempts.finish(
        &rejected,
        &current,
        &Action::NotNow,
        Some(before),
        reveal::Completion::Rejected
    ));
    assert!(attempts.begin(&current, &Action::NotNow, after).is_none());
    current.selection = ProjectionId::new();
    let hidden = attempts
        .begin(&current, &Action::NotNow, after)
        .expect("new selection");
    current.presentation_eligible = false;
    assert!(!attempts.finish(
        &hidden,
        &current,
        &Action::NotNow,
        Some(after),
        reveal::Completion::Measured
    ));
    assert!(attempts.begin(&current, &Action::NotNow, after).is_none());
}
