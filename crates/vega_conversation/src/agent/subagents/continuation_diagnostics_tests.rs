use super::*;

#[test]
fn continuation_permission_matrix_is_unchanged() {
    for tool in ["followup_agent", "resume_agent"] {
        for state in [
            SubagentState::Running,
            SubagentState::Cancelling,
            SubagentState::Completed,
            SubagentState::Failed,
            SubagentState::Interrupted,
        ] {
            for occupied in [false, true] {
                for same_parent in [false, true] {
                    let current = if same_parent {
                        "original"
                    } else {
                        "new-execution"
                    };
                    let result = verify_continuation(tool, state, occupied, "original", current);
                    let allowed = !occupied
                        && match tool {
                            "followup_agent" => state == SubagentState::Completed && same_parent,
                            _ => state == SubagentState::Interrupted,
                        };
                    assert_eq!(
                        result.is_ok(),
                        allowed,
                        "{tool} {state:?} {occupied} {same_parent}"
                    );
                    if occupied
                        || matches!(state, SubagentState::Running | SubagentState::Cancelling)
                    {
                        assert!(result.unwrap_err().to_string().contains("agent_busy"));
                    }
                }
            }
        }
    }
}

#[test]
fn previous_parent_diagnostic_guides_explicit_handoff_without_conversion() {
    for state in [
        SubagentState::Completed,
        SubagentState::Failed,
        SubagentState::Interrupted,
    ] {
        let error = verify_continuation("followup_agent", state, false, "previous", "current")
            .unwrap_err()
            .to_string();
        for required in [
            "followup_parent_mismatch",
            "call spawn_agent",
            "task goal",
            "existing artifacts",
            "accessible links",
            "exact versions",
            "completed and remaining work",
            "necessary context",
            "constraints, evidence, next steps",
            "does not automatically inherit",
            "Do not retry",
            "resume_agent",
            "No new Agent has been created",
        ] {
            assert!(error.contains(required), "missing {required}: {error}");
        }
        assert!(!error.contains("followup_state_not_completed"));
    }
}

#[test]
fn same_parent_reports_actual_terminal_state_without_resume_authorization() {
    for state in [SubagentState::Failed, SubagentState::Interrupted] {
        let error = verify_continuation("followup_agent", state, false, "same", "same")
            .unwrap_err()
            .to_string();
        assert!(error.contains("followup_state_not_completed"));
        assert!(error.contains(state.as_str()));
        assert!(error.contains("normally completed"));
        assert!(
            error.contains("Do not retry the same followup or assume resume_agent is permitted")
        );
        assert!(!error.contains("followup_parent_mismatch"));
    }
    assert!(
        verify_continuation(
            "followup_agent",
            SubagentState::Completed,
            false,
            "stable-run",
            "stable-run"
        )
        .is_ok()
    );
}
