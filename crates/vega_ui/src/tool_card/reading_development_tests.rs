use super::*;
use vega_conversation::types::SkillToolKind;

fn reading_card(status: ToolCallStatus, outcome: Option<SkillCardOutcome>) -> ToolCard {
    ToolCard::hydrated(
        Some(ToolCardInputProjection::Skill {
            kind: SkillToolKind::Load,
            name: Some("review".into()),
            path_bytes: None,
            path_sha256: None,
        }),
        status,
        None,
        outcome.map(|outcome| ToolCardResultProjection::Skill {
            status,
            outcome,
            reused: false,
        }),
        Some("This lower-trust description must not replace the reading status".into()),
    )
}

#[test]
fn skill_reading_preserves_lifecycle_in_compact_and_group_headlines() {
    for status in [
        ToolCallStatus::PendingApproval,
        ToolCallStatus::Approved,
        ToolCallStatus::Running,
        ToolCallStatus::Failed,
        ToolCallStatus::Rejected,
        ToolCallStatus::Cancelled,
    ] {
        let card = reading_card(status, None);
        let expected = vega_i18n::t_args(
            "ui.action_skill",
            &[
                (
                    "action",
                    &generic_verb("read", card.activity_state(), status),
                ),
                ("name", "review"),
            ],
        );
        assert_eq!(card.compact_title(), expected);
        assert_eq!(
            card.description_headline(),
            Some((expected, Icon::BookOpen))
        );
        assert_eq!(card.leading_icon(), Icon::BookOpen);
    }
}

#[test]
fn skill_success_is_compact_but_resource_and_reuse_details_remain_available() {
    for outcome in [
        SkillCardOutcome::Loaded,
        SkillCardOutcome::AlreadyLoaded,
        SkillCardOutcome::ResourceRead {
            text_bytes: 2048,
            sha256: "a".repeat(64),
        },
        SkillCardOutcome::AssetMetadata { size_bytes: 4096 },
    ] {
        let mut card = reading_card(ToolCallStatus::Success, Some(outcome.clone()));
        card.description = None;
        assert_eq!(
            card.compact_title(),
            vega_i18n::t_args("ui.loaded_skill_name", &[("name", "review")])
        );
        assert!(card.has_detail());
        let detail = card
            .detail()
            .unwrap_or_else(|| panic!("Skill detail is required"));
        assert_eq!(detail.command.as_deref(), Some("review"));
        assert_eq!(detail.output, vec![skill_summary(Some("review"), &outcome)]);
        assert_eq!(card.detail_row_count(), detail.logical_rows());
    }
}

#[test]
fn skill_failure_rejection_and_cancellation_keep_terminal_evidence() {
    for (status, outcome) in [
        (
            ToolCallStatus::Failed,
            SkillCardOutcome::Failed {
                code: "unavailable".into(),
            },
        ),
        (ToolCallStatus::Rejected, SkillCardOutcome::Rejected),
        (ToolCallStatus::Cancelled, SkillCardOutcome::Cancelled),
    ] {
        let card = reading_card(status, Some(outcome.clone()));
        assert_eq!(
            card.compact_title(),
            skill_summary(Some("review"), &outcome)
        );
        assert!(card.has_detail());
        assert_ne!(card.activity_state(), ToolActivityState::Success);
    }
}

#[test]
fn activation_receipt_is_read_only_display_without_permission_or_call_identity() {
    let card = ToolCard::activated_skill("review".into());
    assert!(card.reads_skill("review"));
    assert!(!card.reads_skill("other"));
    assert_eq!(card.activity_state(), ToolActivityState::Success);
    assert_eq!(card.permission_identity(), None);
    assert_eq!(card.approval, None);
    assert_eq!(
        card.compact_title(),
        vega_i18n::t_args("ui.loaded_skill_name", &[("name", "review")])
    );
}

#[test]
fn corrupt_skill_result_cannot_be_hidden_by_a_description() {
    let mut card = reading_card(ToolCallStatus::Success, None);
    card.result = Some(ToolCardResultProjection::Corrupt);
    card.refresh_summary();
    assert_eq!(card.compact_title(), vega_i18n::t(CORRUPT_LABEL));
    assert_eq!(card.activity_state(), ToolActivityState::Failed);
}
