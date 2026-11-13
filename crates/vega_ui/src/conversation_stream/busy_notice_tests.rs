use super::ComposerActions;
use crate::localized_copy::LocalizedCopy;

#[test]
fn busy_notice_survives_until_terminal_release() {
    let mut actions = ComposerActions {
        running: true,
        ..Default::default()
    };
    let mut error = None;
    ComposerActions::apply_busy(&mut error);
    ComposerActions::apply_busy(&mut error);
    assert!(actions.running);
    assert!(matches!(error, Some(LocalizedCopy::AgentBusy)));
    assert!(!actions.finish_run(false, &mut error));
    assert!(!actions.running);
    assert!(error.is_none());
}

#[test]
fn cancellation_clears_busy_without_changing_focus_or_cancel_precedence() {
    let mut actions = ComposerActions {
        running: true,
        stopping: true,
        terminal_cancelled: Some(true),
        ..Default::default()
    };
    let mut error = Some(LocalizedCopy::AgentBusy);
    assert!(actions.finish_run(false, &mut error));
    assert!(error.is_none());
    assert!(!actions.running);
    assert!(!actions.stopping);
    assert!(actions.restore_focus);
}

#[test]
fn real_errors_survive_busy_and_terminal_release() {
    let errors = [
        LocalizedCopy::storage("storage sentinel"),
        LocalizedCopy::new("ui.local_credentials_are_missing_or_unreadable_re_enter"),
        LocalizedCopy::Text("permission denied".into()),
        LocalizedCopy::Text("runtime failure".into()),
        LocalizedCopy::new("ui.this_conversation_is_running_wait_for_it_to"),
        LocalizedCopy::Text(
            "This conversation is running. Wait for it to finish or stop the current task.".into(),
        ),
    ];
    for original in errors {
        let expected = format!("{original:?}");
        for cancelled in [false, true] {
            let mut actions = ComposerActions {
                running: true,
                ..Default::default()
            };
            let mut error = Some(original.clone());
            ComposerActions::apply_busy(&mut error);
            assert_eq!(format!("{:?}", error.as_ref().unwrap()), expected);
            assert_eq!(actions.finish_run(cancelled, &mut error), cancelled);
            assert_eq!(format!("{:?}", error.as_ref().unwrap()), expected);
        }
    }
}

#[test]
fn real_failure_replacing_busy_is_preserved_at_completion() {
    let mut actions = ComposerActions {
        running: true,
        ..Default::default()
    };
    let mut error = None;
    ComposerActions::apply_busy(&mut error);
    assert!(matches!(error, Some(LocalizedCopy::AgentBusy)));
    error = Some(LocalizedCopy::Text("save failure".into()));
    actions.finish_run(false, &mut error);
    assert!(matches!(error, Some(LocalizedCopy::Text(ref text)) if text == "save failure"));
}

#[test]
fn completion_without_notice_does_not_create_an_error() {
    let mut actions = ComposerActions {
        running: true,
        ..Default::default()
    };
    let mut error = None;
    actions.finish_run(false, &mut error);
    assert!(error.is_none());
    assert!(!actions.running);
}
