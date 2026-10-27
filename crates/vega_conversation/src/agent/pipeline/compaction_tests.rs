use super::*;
use vega_store::context_compaction::ContextSource;
use vega_store::messages::MessageRow;

fn message(id: &str, seq: i64, role: &str, content: &str) -> MessageRow {
    MessageRow {
        id: id.into(),
        thread_id: "thread".into(),
        seq,
        role: role.into(),
        kind: "text".into(),
        content: content.into(),
        status: "done".into(),
        created_at: seq,
        plan_status: None,
        plan_review_note: None,
        plan_reviewed_at: None,
    }
}

fn source() -> ContextSource {
    ContextSource {
        thread_id: "thread".into(),
        source_version: 5,
        fingerprint: "synthetic-source".into(),
        messages: vec![
            message("old-user", 1, "user", "Earlier completed task"),
            message("old-answer", 2, "assistant", "Completed"),
            message(
                "task",
                3,
                "user",
                "Continue implementation; do not run local tests.",
            ),
            message("owner", 4, "assistant", "Working"),
            message("correction", 5, "user", "Stop now"),
        ],
        tool_calls: Vec::new(),
        images: Vec::new(),
    }
}

#[test]
fn partial_owner_restores_its_original_task_not_a_later_correction()
-> Result<(), Box<dyn std::error::Error>> {
    let source = source();
    let before = source.clone();
    let projection = historical_continuation_projection(&source, 3, Some("owner"))?;
    assert_eq!(projection.len(), 1);
    let (_, encoded) = projection[0]
        .content
        .rsplit_once('\n')
        .ok_or("missing reference")?;
    let reference: serde_json::Value = serde_json::from_str(encoded)?;
    assert_eq!(reference["source_message_id"], "task");
    assert_eq!(reference["source_sequence"], 3);
    assert_eq!(reference["content"], source.messages[2].content);
    assert_eq!(source, before);
    Ok(())
}

#[test]
fn retained_user_is_not_duplicated_and_full_prefix_does_not_invent_an_owner()
-> Result<(), Box<dyn std::error::Error>> {
    let source = source();
    assert!(historical_continuation_projection(&source, 2, Some("owner"))?.is_empty());
    assert!(historical_continuation_projection(&source, 3, None)?.is_empty());
    assert!(historical_continuation_projection(&source, 3, Some("absent"))?.is_empty());
    Ok(())
}

#[test]
fn reload_keeps_task_reference_before_later_stop_instruction()
-> Result<(), Box<dyn std::error::Error>> {
    let source = source();
    let checkpoint = vega_store::context_compaction::ContextCheckpoint {
        id: 1,
        thread_id: "thread".into(),
        model: "model".into(),
        source_version: 4,
        covered_through_seq: 3,
        covered_assistant_id: Some("owner".into()),
        covered_assistant_text_bytes: 0,
        covered_assistant_tool_seq: 0,
        source_fingerprint: "synthetic-source".into(),
        summary: "Implementation in progress".into(),
        estimator_version: "development".into(),
        expected_previous_id: None,
        projection_plan: None,
        referenced_summaries: Default::default(),
        created_at: 4,
    };
    let mut installed = vec![historical_summary_message(&checkpoint.summary)];
    installed.extend(historical_continuation_projection(
        &source,
        3,
        Some("owner"),
    )?);
    installed.extend(history_from_context_source(&source, "owner", Some(3))?);
    let reloaded =
        history_from_context_source_with_checkpoint(&source, Some(&checkpoint), "owner")?;
    assert_eq!(reloaded, installed);
    assert_eq!(reloaded.len(), 3);
    assert_eq!(reloaded[2].role, vega_runtime::ChatRole::User);
    assert_eq!(reloaded[2].content, "Stop now");
    Ok(())
}

#[test]
fn versioned_range_projection_keeps_both_directions_and_future_corrections()
-> Result<(), Box<dyn std::error::Error>> {
    use vega_store::context_projection::{
        ContinuationMode, ProjectionFragment, ProjectionPlan, ProjectionStrategy, SourceCursor,
        SourceRange,
    };
    let source = source();
    let before = source.clone();
    let boundary = SourceCursor {
        message_id: "old-answer".into(),
        message_seq: 2,
        assistant: None,
    };
    let tail = SourceCursor {
        message_id: "correction".into(),
        message_seq: 5,
        assistant: None,
    };
    let up_to = ProjectionPlan {
        schema: 1,
        strategy: ProjectionStrategy::PartialUpTo,
        continuation: ContinuationMode::NoContinuation,
        source_owner_id: None,
        task_message_id: Some("task".into()),
        covered: vec![SourceRange {
            after: None,
            through: Some(boundary.clone()),
        }],
        omitted: Vec::new(),
        fragments: vec![
            ProjectionFragment::Summary,
            ProjectionFragment::Source {
                range: SourceRange {
                    after: Some(boundary.clone()),
                    through: None,
                },
            },
        ],
    };
    let summary = "Earlier task completed.";
    let projected =
        interpret_projection(&source, &up_to, summary, &Default::default(), "__none__")?;
    assert_eq!(projected.len(), 4);
    assert_eq!(projected[0], historical_summary_message(summary));
    assert_eq!(projected[1].content, source.messages[2].content);
    assert_eq!(projected.last().ok_or("tail absent")?.content, "Stop now");
    let from = ProjectionPlan {
        strategy: ProjectionStrategy::PartialFrom,
        covered: vec![SourceRange {
            after: Some(boundary.clone()),
            through: Some(tail.clone()),
        }],
        fragments: vec![
            ProjectionFragment::Source {
                range: SourceRange {
                    after: None,
                    through: Some(boundary.clone()),
                },
            },
            ProjectionFragment::Summary,
            ProjectionFragment::Source {
                range: SourceRange {
                    after: Some(tail),
                    through: None,
                },
            },
        ],
        ..up_to.clone()
    };
    let summary = "Implementation stopped by the later user correction.";
    let projected = interpret_projection(&source, &from, summary, &Default::default(), "__none__")?;
    assert_eq!(projected.len(), 3);
    assert_eq!(projected[0].content, "Earlier completed task");
    assert_eq!(projected[1].content, "Completed");
    assert_eq!(projected[2], historical_summary_message(summary));
    let checkpoint = vega_store::context_compaction::ContextCheckpoint {
        id: 10,
        thread_id: "thread".into(),
        model: "model".into(),
        source_version: 5,
        covered_through_seq: 0,
        covered_assistant_id: None,
        covered_assistant_text_bytes: 0,
        covered_assistant_tool_seq: 0,
        source_fingerprint: source.fingerprint.clone(),
        summary: summary.into(),
        estimator_version: "development".into(),
        expected_previous_id: None,
        projection_plan: Some(from.clone()),
        referenced_summaries: Default::default(),
        created_at: 6,
    };
    assert_eq!(
        history_from_context_source_with_checkpoint(&source, Some(&checkpoint), "__none__")?,
        projected
    );
    let mut appended = source.clone();
    appended.messages.push(message(
        "resume",
        6,
        "user",
        "Continue only the pending fix",
    ));
    appended.source_version = 6;
    let reloaded =
        history_from_context_source_with_checkpoint(&appended, Some(&checkpoint), "__none__")?;
    assert_eq!(&reloaded[..projected.len()], projected.as_slice());
    assert_eq!(
        reloaded.last().ok_or("appended user absent")?.content,
        "Continue only the pending fix"
    );
    let mut invalid = from.clone();
    invalid.schema = 2;
    assert!(
        interpret_projection(&source, &invalid, summary, &Default::default(), "__none__").is_err()
    );
    let mut invalid = from.clone();
    invalid.covered[0].after = None;
    assert!(invalid.validate(&source).is_err());
    let mut referenced = from;
    referenced
        .fragments
        .insert(0, ProjectionFragment::PreviousSummary { checkpoint_id: 9 });
    assert!(
        interpret_projection(
            &source,
            &referenced,
            summary,
            &Default::default(),
            "__none__"
        )
        .is_err()
    );
    let prior = std::collections::BTreeMap::from([(9, "Prior decisions.".into())]);
    let projected = interpret_projection(&source, &referenced, summary, &prior, "__none__")?;
    assert_eq!(projected[0], historical_summary_message("Prior decisions."));
    assert_eq!(source, before);
    Ok(())
}
