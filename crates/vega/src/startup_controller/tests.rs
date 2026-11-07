use gpui_kit::{KeyDownEvent, Keystroke};
use vega_conversation::types::{PermissionResetError, StartupError};
use vega_ui::nsh::ProjectionId;

use super::{
    error_key, logo,
    model::{Action, Authority, Input, Key, RenderFence, Snapshot, key, traverse},
};

fn failure(recovery: bool) -> Authority {
    Authority {
        session: ProjectionId::new(),
        mount: ProjectionId::new(),
        projection: ProjectionId::new(),
        running: false,
        error: Some(StartupError::StorageUnavailable),
        recovery,
    }
}

#[test]
fn failure_actions_require_the_observed_session_mount_and_error_projection() {
    let observed = failure(false);
    assert!(observed.accepts(&observed, Action::Retry));
    assert!(observed.accepts(&observed, Action::Quit));
    for replacement in 0..5 {
        let mut current = observed.clone();
        match replacement {
            0 => current.session = ProjectionId::new(),
            1 => current.mount = ProjectionId::new(),
            2 => current.projection = ProjectionId::new(),
            3 => current.error = Some(StartupError::WorkerUnavailable),
            _ => current.running = true,
        }
        assert!(!current.accepts(&observed, Action::Retry));
        assert!(!current.accepts(&observed, Action::Quit));
    }
}

#[test]
fn recovery_reload_is_presentation_only_and_stale_business_actions_are_rejected() {
    let observed = failure(true);
    assert_eq!(
        observed.controls(),
        [Action::Retry, Action::Quit, Action::Reload]
    );
    assert!(observed.accepts(&observed, Action::Reload));
    let mut loading = observed.clone();
    loading.running = true;
    loading.error = None;
    loading.projection = ProjectionId::new();
    assert_eq!(loading.controls(), [Action::Reload]);
    for action in [Action::Retry, Action::Quit, Action::Reload] {
        assert!(!loading.accepts(&observed, action));
    }
    assert!(loading.accepts(&loading, Action::Reload));
    let mut replaced = observed.clone();
    replaced.mount = ProjectionId::new();
    assert!(!replaced.accepts(&observed, Action::Reload));
    let mut restored = observed.clone();
    restored.recovery = false;
    assert!(!restored.accepts(&observed, Action::Quit));
    assert!(!restored.accepts(&restored, Action::Reload));
}

#[test]
fn traversal_wraps_both_directions_and_loading_has_no_business_stops() {
    let controls = failure(false).controls();
    assert_eq!(traverse(None, &controls, false), Some(Action::Retry));
    assert_eq!(traverse(None, &controls, true), Some(Action::Quit));
    assert_eq!(
        traverse(Some(Action::Retry), &controls, true),
        Some(Action::Quit)
    );
    assert_eq!(
        traverse(Some(Action::Quit), &controls, false),
        Some(Action::Retry)
    );
    assert_eq!(traverse(None, &[], false), None);
    let mut loading = failure(false);
    loading.running = true;
    loading.error = None;
    assert!(loading.controls().is_empty());
}

fn event(source: &str) -> KeyDownEvent {
    KeyDownEvent {
        keystroke: Keystroke::parse(source).expect("valid development keystroke"),
        is_held: false,
        prefer_character_input: true,
    }
}

#[test]
fn native_keyboard_uses_keys_not_character_preference_and_ignores_repeats_and_modifiers() {
    assert_eq!(key(&event("tab")), Some(Key::Traverse { backwards: false }));
    assert_eq!(
        key(&event("shift-tab")),
        Some(Key::Traverse { backwards: true })
    );
    assert_eq!(key(&event("enter")), Some(Key::Activate));
    assert_eq!(key(&event("space")), Some(Key::Activate));
    for source in ["shift-enter", "ctrl-enter", "alt-space", "cmd-tab", "a"] {
        assert_eq!(key(&event(source)), None);
    }
    let mut held = event("enter");
    held.is_held = true;
    assert_eq!(key(&held), None);
    held = event("tab");
    held.is_held = true;
    assert_eq!(key(&held), None);
    let mut function = event("enter");
    function.keystroke.modifiers.function = true;
    assert_eq!(key(&function), None);
}

#[test]
fn shimmer_sweeps_off_canvas_and_peaks_at_the_selected_slice() {
    for index in 0..64 {
        assert_eq!(logo::blend(index, 0.), 0.);
        assert_eq!(logo::blend(index, 1.), 0.);
    }
    let phase = ((31.5 / 64.) + 0.2) / 1.4;
    assert!((logo::blend(31, phase) - 1.).abs() < 0.00001);
    assert_eq!(logo::blend(0, phase), 0.);
    assert_eq!(logo::blend(63, phase), 0.);
}

#[test]
fn startup_and_permission_reset_failures_keep_the_exact_mapping() {
    let cases = [
        (StartupError::PathsUnavailable, "app.startup.paths"),
        (StartupError::StorageUnavailable, "app.startup.storage"),
        (
            StartupError::ConfigurationUnavailable,
            "app.startup.configuration",
        ),
        (StartupError::AutomationRecovery, "app.startup.automation"),
        (StartupError::WorkerUnavailable, "app.startup.worker"),
        (
            StartupError::PermissionReset(PermissionResetError::Unavailable),
            "app.startup.reset_unavailable",
        ),
        (
            StartupError::PermissionReset(PermissionResetError::Busy),
            "app.startup.reset_busy",
        ),
        (
            StartupError::PermissionReset(PermissionResetError::UnsettledWork),
            "app.startup.reset_unsettled",
        ),
        (
            StartupError::PermissionReset(PermissionResetError::ConfigurationConflict),
            "app.startup.reset_conflict",
        ),
        (
            StartupError::PermissionReset(PermissionResetError::ConfigurationUnavailable),
            "app.startup.reset_configuration",
        ),
        (
            StartupError::PermissionReset(PermissionResetError::OutcomeUnknown),
            "app.startup.reset_unknown",
        ),
    ];
    for (error, expected) in cases {
        assert_eq!(error_key(error), expected);
    }
}

fn snapshot(authority: Authority, selected: Option<Action>) -> Snapshot {
    Snapshot {
        frame: ProjectionId::new(),
        authority,
        colors: vega_theme::LIGHT,
        selected,
        phase: None,
        language: "en",
        appearance: vega_theme::Appearance::Light,
    }
}

#[test]
fn installing_native_failure_host_does_not_authorize_any_input_until_exact_layout_ack() {
    for loading in [false, true] {
        for action in [Action::Retry, Action::Quit] {
            for input in [Input::Pointer, Input::Keyboard, Input::Accessibility] {
                let mut fence = RenderFence::default();
                let mut a = snapshot(failure(false), Some(action));
                if loading {
                    a.authority.running = true;
                    a.authority.error = None;
                    a.selected = None;
                }
                fence.publish(&mut a);
                let displayed_a = a.rendered_frame();
                assert!(fence.acknowledge(displayed_a.clone()));
                let mut b = a.clone();
                b.authority.projection = ProjectionId::new();
                b.authority.running = false;
                b.authority.error = Some(StartupError::WorkerUnavailable);
                b.selected = Some(action);
                fence.publish(&mut b);
                let installed_host_b = b.rendered_frame();
                let mut accepted = 0;
                for _ in 0..3 {
                    accepted += usize::from(fence.claim(&displayed_a, &b, action, input, false));
                    accepted +=
                        usize::from(fence.claim(&installed_host_b, &b, action, input, false));
                }
                assert_eq!(accepted, 0);
                assert!(!fence.acknowledge(displayed_a.clone()));
                assert!(fence.acknowledge(installed_host_b.clone()));
                assert!(!fence.claim(&displayed_a, &b, action, input, false));
                for _ in 0..3 {
                    accepted +=
                        usize::from(fence.claim(&installed_host_b, &b, action, input, false));
                    assert!(!fence.acknowledge(installed_host_b.clone()));
                    fence.publish(&mut b);
                }
                assert_eq!(accepted, 1);
            }
        }
    }
}

#[test]
fn selection_round_trip_needs_a_fresh_layout_ack_and_native_activation_matches_selection() {
    for input in [Input::Pointer, Input::Keyboard, Input::Accessibility] {
        let mut fence = RenderFence::default();
        let mut current = snapshot(failure(false), Some(Action::Retry));
        fence.publish(&mut current);
        let original_retry = current.rendered_frame();
        assert!(fence.acknowledge(original_retry.clone()));
        current.selected = Some(Action::Quit);
        assert!(!fence.claim(&original_retry, &current, Action::Retry, input, false));
        fence.publish(&mut current);
        let quit = current.rendered_frame();
        current.selected = Some(Action::Retry);
        fence.publish(&mut current);
        let returned_retry = current.rendered_frame();
        assert!(!fence.acknowledge(original_retry.clone()));
        assert!(!fence.acknowledge(quit.clone()));
        assert!(!fence.claim(&returned_retry, &current, Action::Retry, input, false));
        assert!(fence.acknowledge(returned_retry.clone()));
        for native in [Input::Keyboard, Input::Accessibility] {
            assert!(!fence.claim(&returned_retry, &current, Action::Quit, native, false));
        }
        assert!(fence.claim(&returned_retry, &current, Action::Retry, input, false));
    }
}

#[test]
fn replaced_recovery_mount_rejects_old_layout_acks_and_all_old_input_routes() {
    for action in [Action::Retry, Action::Quit, Action::Reload] {
        for input in [Input::Pointer, Input::Keyboard, Input::Accessibility] {
            let mut old_fence = RenderFence::default();
            let mut current = snapshot(failure(true), Some(action));
            old_fence.publish(&mut current);
            let old = current.rendered_frame();
            assert!(old_fence.acknowledge(old.clone()));
            current.authority.mount = ProjectionId::new();
            let mut replacement = RenderFence::default();
            replacement.publish(&mut current);
            let new = current.rendered_frame();
            assert!(!replacement.acknowledge(old.clone()));
            assert!(!replacement.claim(&old, &current, action, input, false));
            assert!(!replacement.claim(&new, &current, action, input, false));
            assert!(replacement.acknowledge(new.clone()));
            assert!(!replacement.acknowledge(old.clone()));
            assert!(!replacement.claim(&old, &current, action, input, false));
            assert!(replacement.claim(&new, &current, action, input, false));
            assert!(!replacement.claim(&new, &current, action, input, false));
        }
    }
}

#[test]
fn failed_recovery_only_allows_exact_ui_reload_without_claiming_a_render_ack() {
    for input in [Input::Pointer, Input::Keyboard, Input::Accessibility] {
        let mut fence = RenderFence::default();
        let mut current = snapshot(failure(true), Some(Action::Reload));
        fence.publish(&mut current);
        let installed = current.rendered_frame();
        for action in [Action::Retry, Action::Quit] {
            assert!(!fence.claim(&installed, &current, action, input, true));
        }
        assert!(!fence.claim(&installed, &current, Action::Reload, input, false));
        let mut stale = current.clone();
        stale.authority.mount = ProjectionId::new();
        assert!(!fence.claim(&installed, &stale, Action::Reload, input, true));
        assert!(fence.claim(&installed, &current, Action::Reload, input, true));
        assert!(!fence.claim(&installed, &current, Action::Reload, input, true));
        assert!(fence.acknowledge(installed.clone()));
        assert!(!fence.claim(&installed, &current, Action::Retry, input, true));
        assert!(!fence.claim(&installed, &current, Action::Reload, input, true));

        let mut primary = snapshot(failure(false), Some(Action::Reload));
        let mut primary_fence = RenderFence::default();
        primary_fence.publish(&mut primary);
        let observed = primary.rendered_frame();
        assert!(!primary_fence.claim(&observed, &primary, Action::Reload, input, true));
    }
}

#[test]
fn acknowledged_frames_still_require_current_state_and_cannot_replay_on_animation_updates() {
    for action in [Action::Retry, Action::Quit] {
        for input in [Input::Pointer, Input::Keyboard, Input::Accessibility] {
            let mut fence = RenderFence::default();
            let mut current = snapshot(failure(true), Some(action));
            fence.publish(&mut current);
            let observed = current.rendered_frame();
            assert!(fence.acknowledge(observed.clone()));
            assert!(!fence.claim(&observed, &current, action, input, true));
            for change in 0..9 {
                let mut changed = current.clone();
                match change {
                    0 => changed.authority.session = ProjectionId::new(),
                    1 => changed.authority.mount = ProjectionId::new(),
                    2 => changed.authority.projection = ProjectionId::new(),
                    3 => changed.authority.running = true,
                    4 => changed.authority.error = Some(StartupError::WorkerUnavailable),
                    5 => changed.authority.recovery = false,
                    6 => changed.selected = None,
                    7 => changed.language = "zh-CN",
                    _ => changed.appearance = vega_theme::Appearance::Dark,
                }
                assert!(!fence.claim(&observed, &changed, action, input, false));
            }
            current.phase = Some(0.5);
            fence.publish(&mut current);
            assert!(fence.claim(&observed, &current, action, input, false));
            current.phase = Some(0.75);
            fence.publish(&mut current);
            assert!(!fence.claim(&observed, &current, action, input, false));
            assert!(!fence.acknowledge(observed.clone()));
            assert!(!fence.claim(&observed, &current, action, input, false));
        }
    }
}
