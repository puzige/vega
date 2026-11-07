use super::*;
use vega_conversation::types::ShellCommandProjection;

#[test]
fn persisted_command_terminal_updates_history_card_without_rewriting_receipt() {
    let receipt = ToolCardResultProjection::Bash {
        status: ToolCallStatus::Success,
        output: "original output".into(),
        exit_code: None,
        duration_ms: Some(15),
        truncated: Some(false),
        reused: false,
        session_state: Some("running".into()),
        operation_id: Some("operation".into()),
        output_incomplete: false,
    };
    let mut card = ToolCard::hydrated(
        Some(ToolCardInputProjection::Bash {
            command: "printf done".into(),
        }),
        ToolCallStatus::Success,
        Some(Approval::Once),
        Some(receipt.clone()),
        None,
    );
    assert_eq!(card.activity_state(), ToolActivityState::Active);
    let mut command = ShellCommandProjection {
        operation_id: "other".into(),
        call_id: "call".into(),
        thread_id: "thread".into(),
        command: "printf done".into(),
        state: "cancelled".into(),
        exit_code: None,
        duration_ms: Some(4200),
        output_incomplete: true,
    };
    assert!(!card.apply_shell_command(&command));
    assert_eq!(card.activity_state(), ToolActivityState::Active);
    command.operation_id = "operation".into();
    assert!(card.apply_shell_command(&command));
    assert_eq!(card.activity_state(), ToolActivityState::Cancelled);
    assert_eq!(card.shell_session_state(), Some("cancelled"));
    assert_eq!(card.bash_duration(), Some("4.2s".into()));
    assert_eq!(card.result.as_ref(), Some(&receipt));
    assert_eq!(card.output_rows, vec!["original output"]);
    assert!(
        card.bash_detail_title()
            .contains(vega_i18n::t("ui.shell_session.output_incomplete"))
    );
}
