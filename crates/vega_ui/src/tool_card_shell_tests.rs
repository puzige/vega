use super::*;

#[test]
fn shell_poll_preserves_pending_control_and_unresolved_failure() {
    let mut card = ToolCard::corrupt();
    card.input = Some(ToolCardInputProjection::Bash {
        command: "true".into(),
    });
    card.result = Some(ToolCardResultProjection::Bash {
        status: ToolCallStatus::Success,
        output: String::new(),
        exit_code: None,
        duration_ms: None,
        truncated: None,
        reused: false,
        session_state: Some("running".into()),
        operation_id: Some("operation".into()),
        output_incomplete: false,
    });
    let mut command = vega_conversation::types::ShellCommandProjection {
        concurrent: true,
        cleanup_unresolved: false,
        can_cancel: true,
        can_retry: false,
        operation_id: "operation".into(),
        call_id: "call".into(),
        thread_id: "thread".into(),
        command: "true".into(),
        state: "running".into(),
        exit_code: None,
        duration_ms: None,
        output_incomplete: false,
    };
    assert!(card.apply_shell_command(&command));
    card.shell_control_pending = true;
    assert!(card.apply_shell_command(&command));
    assert!(card.shell_control_pending);
    assert!(!card.finish_shell_control("other", true));
    assert!(card.shell_control_pending);
    assert!(card.finish_shell_control("operation", true));
    assert!(card.apply_shell_command(&command));
    assert!(card.shell_control_failed);
    command.state = "outcome_unknown".into();
    command.cleanup_unresolved = true;
    command.can_cancel = false;
    command.can_retry = true;
    assert!(card.apply_shell_command(&command));
    assert!(card.shell_control_failed);
    assert_eq!(card.shell_session_state(), Some("outcome_unknown"));
    command.state = "cancelled".into();
    command.cleanup_unresolved = false;
    command.can_retry = false;
    assert!(card.apply_shell_command(&command));
    assert!(!card.shell_control_failed);
    assert_eq!(card.shell_session_state(), Some("cancelled"));
}
