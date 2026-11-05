use super::*;

#[test]
fn explicit_skill_preparation_failures_keep_skill_rejection_and_recovery_contract() {
    for error in [
        vega_runtime::skills::SkillError::Stale,
        vega_runtime::skills::SkillError::Io,
    ] {
        let status = error.code();
        let mapped = skill_preparation_error(error);
        let ConversationError::Runtime(error) = mapped else {
            panic!("skill error must remain runtime rejection");
        };
        let VegaError::Tool { tool, message } = error.as_ref() else {
            panic!("skill error must retain tool category");
        };
        assert_eq!(tool, "skill");
        assert_eq!(
            message,
            &format!(
                "Skill activation rejected ({status}). Review the skill in Settings, then retry or remove the message."
            )
        );
    }
}
