use super::history_preparation::PreparedHistoryEntries;
use super::model::StreamCounters;
use crate::icons::Icon;
use crate::tool_card::{ToolActivityCategory, ToolActivityState, ToolCard};
use vega_conversation::history::{
    AssistantStatus, HistoryEntry, SkillHistoryActivation, SkillHistoryOrigin, SkillHistorySource,
    SkillHistoryStatus, SkillHistoryVerification,
};
use vega_conversation::types::{
    SkillCardOutcome, SkillToolKind, ToolCallStatus, ToolCardInputProjection,
    ToolCardResultProjection,
};

fn audit(
    seq: i64,
    verification: SkillHistoryVerification,
    status: SkillHistoryStatus,
) -> HistoryEntry {
    HistoryEntry::SkillActivation {
        seq,
        activation: SkillHistoryActivation {
            run_id: "assistant".into(),
            name: "example".into(),
            source_scope: SkillHistorySource::Project,
            content_sha256: "a".repeat(64),
            origin: SkillHistoryOrigin::Model,
            status,
            verification,
        },
    }
}

fn tool(seq: i64, status: ToolCallStatus, outcome: SkillCardOutcome) -> HistoryEntry {
    HistoryEntry::Tool {
        seq,
        message_id: "assistant".into(),
        call_id: "skill-call".into(),
        description: None,
        status,
        approval: None,
        input: Some(ToolCardInputProjection::Skill {
            kind: SkillToolKind::Load,
            name: Some("example".into()),
            path_bytes: None,
            path_sha256: None,
        }),
        result: Some(ToolCardResultProjection::Skill {
            status,
            outcome,
            reused: false,
        }),
    }
}

#[test]
fn audit_only_history_has_no_ui_entries_or_assistant_parts() {
    let original = vec![
        audit(
            1,
            SkillHistoryVerification::Verified,
            SkillHistoryStatus::Loaded,
        ),
        audit(
            2,
            SkillHistoryVerification::Unavailable,
            SkillHistoryStatus::Loaded,
        ),
        audit(
            3,
            SkillHistoryVerification::Unavailable,
            SkillHistoryStatus::Revoked,
        ),
    ];
    let page = PreparedHistoryEntries::new(original.clone(), &StreamCounters::default());
    assert!(page.entries.is_empty());
    assert!(page.assistants.is_empty());
    assert!(page.generated_notices.is_empty());
    assert_eq!(original.len(), 3);
    assert!(
        matches!(&original[2], HistoryEntry::SkillActivation { activation, .. } if activation.status == SkillHistoryStatus::Revoked)
    );
}

#[test]
fn audits_do_not_split_real_tools_or_change_answer_order() {
    let page = PreparedHistoryEntries::new(
        vec![
            tool(1, ToolCallStatus::Success, SkillCardOutcome::Loaded),
            audit(
                2,
                SkillHistoryVerification::Unavailable,
                SkillHistoryStatus::Loaded,
            ),
            HistoryEntry::AssistantText {
                seq: 3,
                message_id: "assistant".into(),
                content: "answer".into(),
                status: AssistantStatus::Done,
                execution_duration_ms: Some(1000),
            },
            audit(
                4,
                SkillHistoryVerification::Verified,
                SkillHistoryStatus::Loaded,
            ),
        ],
        &StreamCounters::default(),
    );
    assert_eq!(page.entries.len(), 2);
    assert!(
        matches!(&page.entries[0], HistoryEntry::Tool { call_id, .. } if call_id == "skill-call")
    );
    assert!(
        matches!(&page.entries[1], HistoryEntry::AssistantText { content, execution_duration_ms: Some(1000), .. } if content == "answer")
    );
    assert_eq!(page.assistants.len(), 1);
    let parts = &page.assistants[&1];
    assert_eq!(parts.len(), 1);
    assert!(!parts[0].starts_timestamp_boundary);
    assert_eq!(parts[0].key, "assistant-text-0");
}

#[test]
fn actual_skill_outcomes_survive_as_tool_activity_rows() {
    for (status, outcome, state) in [
        (
            ToolCallStatus::Success,
            SkillCardOutcome::Loaded,
            ToolActivityState::Success,
        ),
        (
            ToolCallStatus::Failed,
            SkillCardOutcome::Failed {
                code: "unavailable".into(),
            },
            ToolActivityState::Failed,
        ),
        (
            ToolCallStatus::Rejected,
            SkillCardOutcome::Rejected,
            ToolActivityState::Rejected,
        ),
        (
            ToolCallStatus::Cancelled,
            SkillCardOutcome::Cancelled,
            ToolActivityState::Cancelled,
        ),
    ] {
        let page = PreparedHistoryEntries::new(
            vec![
                tool(1, status, outcome.clone()),
                audit(
                    2,
                    SkillHistoryVerification::Unavailable,
                    SkillHistoryStatus::Loaded,
                ),
            ],
            &StreamCounters::default(),
        );
        assert_eq!(page.entries.len(), 1);
        let HistoryEntry::Tool {
            input,
            status: actual_status,
            result,
            description,
            ..
        } = &page.entries[0]
        else {
            panic!("actual tool call was removed");
        };
        assert_eq!(*actual_status, status);
        assert!(
            matches!(result, Some(ToolCardResultProjection::Skill { outcome: actual_outcome, .. }) if *actual_outcome == outcome)
        );
        let card = ToolCard::hydrated(
            input.clone(),
            *actual_status,
            None,
            result.clone(),
            description.clone(),
        );
        assert_eq!(card.activity_state(), state);
        assert_eq!(card.activity_category(), ToolActivityCategory::Skill);
        assert!(matches!(card.leading_icon(), Icon::Wrench));
        assert_eq!(card.row_count(), 1);
        let title = card.headline_title();
        assert!(title.contains("example"));
        assert!(
            card.description_headline()
                .is_some_and(|(headline, icon)| headline == title && matches!(icon, Icon::Wrench))
        );
    }
}

#[test]
fn resource_reads_remain_real_tool_rows_without_exposing_audit_details() {
    let mut entry = tool(
        1,
        ToolCallStatus::Success,
        SkillCardOutcome::ResourceRead {
            text_bytes: 123,
            sha256: "b".repeat(64),
        },
    );
    if let HistoryEntry::Tool { input, .. } = &mut entry {
        *input = Some(ToolCardInputProjection::Skill {
            kind: SkillToolKind::ReadResource,
            name: Some("example".into()),
            path_bytes: Some(16),
            path_sha256: Some("c".repeat(64)),
        });
    }
    let page = PreparedHistoryEntries::new(vec![entry], &StreamCounters::default());
    let HistoryEntry::Tool {
        input,
        status,
        result,
        ..
    } = &page.entries[0]
    else {
        panic!("resource tool call was removed");
    };
    let card = ToolCard::hydrated(input.clone(), *status, None, result.clone(), None);
    assert_eq!(card.activity_category(), ToolActivityCategory::Skill);
    assert_eq!(card.activity_state(), ToolActivityState::Success);
    assert!(matches!(card.leading_icon(), Icon::Wrench));
    assert_eq!(card.row_count(), 1);
    assert!(!card.headline_title().contains(&"c".repeat(64)));
}

#[test]
fn corrupt_skill_results_are_preserved_as_failed_tool_rows() {
    let mut entry = tool(1, ToolCallStatus::Failed, SkillCardOutcome::Loaded);
    if let HistoryEntry::Tool { result, .. } = &mut entry {
        *result = Some(ToolCardResultProjection::Corrupt);
    }
    let page = PreparedHistoryEntries::new(
        vec![
            entry,
            audit(
                2,
                SkillHistoryVerification::Unavailable,
                SkillHistoryStatus::Loaded,
            ),
        ],
        &StreamCounters::default(),
    );
    assert_eq!(page.entries.len(), 1);
    let HistoryEntry::Tool {
        input,
        status,
        result,
        ..
    } = &page.entries[0]
    else {
        panic!("corrupt tool call was removed");
    };
    assert!(matches!(result, Some(ToolCardResultProjection::Corrupt)));
    let card = ToolCard::hydrated(input.clone(), *status, None, result.clone(), None);
    assert_eq!(card.activity_state(), ToolActivityState::Failed);
    assert_eq!(
        card.headline_title(),
        vega_i18n::t("ui.tool_result_corrupted")
    );
    assert_eq!(card.row_count(), 1);
}
