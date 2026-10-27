use vega_runtime::{ChatMessage, ChatRole, validate_compaction_replay};

#[test]
fn all_cold_history_uses_end_boundary_without_inventing_a_user_turn() {
    let history = vec![
        ChatMessage::new(ChatRole::User, "Historical summary"),
        ChatMessage::new(ChatRole::User, "Historical task reference"),
        ChatMessage::new(ChatRole::Assistant, "Retained assistant work"),
    ];
    assert!(validate_compaction_replay(&history, 3, Some(3), true));
    assert!(!validate_compaction_replay(&history, 2, Some(2), true));
    assert!(!validate_compaction_replay(&history, 2, Some(3), true));
    assert!(!validate_compaction_replay(&history, 3, None, true));
    assert!(!validate_compaction_replay(&history, 3, Some(4), true));
}

#[test]
fn retained_user_boundary_stays_after_cold_prefix() {
    let history = vec![
        ChatMessage::new(ChatRole::User, "Historical summary"),
        ChatMessage::new(ChatRole::Assistant, "Earlier work"),
        ChatMessage::new(ChatRole::User, "Stop now"),
    ];
    assert!(validate_compaction_replay(&history, 1, Some(2), true));
    assert!(!validate_compaction_replay(&history, 1, Some(1), true));
    assert!(!validate_compaction_replay(&history, 3, Some(2), true));
}

#[test]
fn malformed_tool_results_cannot_be_hidden_by_an_end_boundary() {
    let mut result = ChatMessage::new(ChatRole::Tool, "Unpaired result");
    result.tool_call_id = Some("missing-call".into());
    let history = vec![
        ChatMessage::new(ChatRole::User, "Historical summary"),
        result,
    ];
    assert!(!validate_compaction_replay(&history, 2, Some(2), true));
    assert!(!validate_compaction_replay(&[], 0, Some(0), true));
}
