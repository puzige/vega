use super::ComposerActions;

#[test]
fn failed_run_releases_running_and_stopping_state() {
    let mut actions = ComposerActions {
        running: true,
        stopping: true,
        ..Default::default()
    };
    assert!(!actions.finish_run(false));
    assert!(!actions.running);
    assert!(!actions.stopping);
    assert!(actions.restore_focus);
    actions.running = true;
    assert!(!actions.finish_run(false));
    assert!(!actions.running);
    assert!(!actions.restore_focus);
}

#[test]
fn explicit_terminal_cancel_fact_overrides_later_request() {
    let mut actions = ComposerActions {
        running: true,
        terminal_cancelled: Some(false),
        ..Default::default()
    };
    assert!(!actions.finish_run(true));
    assert!(!actions.running);
    actions.running = true;
    actions.terminal_cancelled = Some(true);
    assert!(actions.finish_run(false));
    assert!(!actions.running);
}

#[test]
fn cancellation_without_terminal_event_releases_disconnected_worker() {
    let mut actions = ComposerActions {
        running: true,
        stopping: true,
        ..Default::default()
    };
    assert!(actions.finish_run(true));
    assert!(!actions.running);
    assert!(!actions.stopping);
}
