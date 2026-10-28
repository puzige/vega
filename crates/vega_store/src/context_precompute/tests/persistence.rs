use super::*;

mod consume_tests;
use crate::context_compaction::{
    ContextCheckpointInstall, NewContextCheckpoint, install_checkpoint,
};
use crate::context_precompute::{accounting, ready};
use crate::context_projection::{
    ContinuationMode, ProjectionFragment, ProjectionPlan, ProjectionStrategy, SourceCursor,
    SourceRange,
};

fn usage() -> accounting::ReportedUsage {
    accounting::ReportedUsage {
        input_tokens: 100,
        output_tokens: 10,
        cache_read_tokens: Some(20),
        cache_write_tokens: None,
        cost_microcents: 7,
        created_at: 1000,
        pricing_version: Some("pricing_v1".into()),
        pricing_profile: Some("base".into()),
        call_started_at: Some(1),
    }
}

fn bills(store: &Store) -> Result<i64, rusqlite::Error> {
    store
        .conn()
        .query_row("SELECT COUNT(*) FROM token_usage", [], |row| row.get(0))
}

fn candidate(
    store: &Store,
    generation: &str,
) -> Result<ready::ReadyCompaction, Box<dyn std::error::Error>> {
    let transaction = store.conn().unchecked_transaction()?;
    let (source, fence) = capture_in_transaction(&transaction, "thread", None)?;
    let previous = crate::context_compaction::latest_checkpoint_in_transaction(
        &transaction,
        "thread",
        "model",
    )?;
    let summary_dependencies =
        ready::summary_dependencies_in_transaction(&transaction, "thread", "model")?;
    transaction.commit()?;
    let boundary = SourceCursor {
        message_id: "task".into(),
        message_seq: 1,
        assistant: None,
    };
    Ok(ready::ReadyCompaction {
        schema: 1,
        generation: generation.into(),
        checkpoint: NewContextCheckpoint {
            thread_id: "thread".into(),
            model: "model".into(),
            source_version: source.source_version,
            covered_through_seq: 1,
            covered_assistant_id: None,
            covered_assistant_text_bytes: 0,
            covered_assistant_tool_seq: 0,
            source_fingerprint: source.fingerprint,
            summary: "SYNTHETIC_SUMMARY".into(),
            estimator_version: "synthetic_estimator".into(),
            expected_previous_id: previous.map(|checkpoint| checkpoint.id),
            projection_plan: Some(ProjectionPlan {
                schema: 1,
                strategy: ProjectionStrategy::Precompute,
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
                            after: Some(boundary),
                            through: None,
                        },
                    },
                ],
            }),
            created_at: 1000,
        },
        source: fence,
        summary_dependencies,
        pre_tokens: 100_000,
        preferences_digest: "0".repeat(64),
        stage_count: 1,
        usage_complete: true,
    })
}

fn start_and_settle(store: &Store, generation: &str, previous: Option<&str>) -> TestResult {
    ready::begin_generation(store.conn(), generation, "thread", "model", previous)?;
    accounting::record_stage(
        store.conn(),
        generation,
        "thread",
        "model",
        1,
        Some(&usage()),
    )?;
    accounting::finish(store.conn(), generation, "thread", "model", 1, true)?;
    Ok(())
}

#[test]
fn precompute_accounting_is_idempotent_and_rejects_conflicts_without_partial_bills() -> TestResult {
    let store = fixture(false)?;
    let reported = usage();
    let id = accounting::record_stage(
        store.conn(),
        "generation",
        "thread",
        "model",
        1,
        Some(&reported),
    )?;
    assert!(id.is_some());
    assert_eq!(
        accounting::record_stage(
            store.conn(),
            "generation",
            "thread",
            "model",
            1,
            Some(&reported)
        )?,
        id
    );
    assert_eq!(bills(&store)?, 1);
    let mut conflicting = reported.clone();
    conflicting.output_tokens += 1;
    assert!(
        accounting::record_stage(
            store.conn(),
            "generation",
            "thread",
            "model",
            1,
            Some(&conflicting)
        )
        .is_err()
    );
    assert!(
        accounting::record_stage(
            store.conn(),
            "generation",
            "other-thread",
            "model",
            2,
            Some(&reported)
        )
        .is_err()
    );
    assert!(
        accounting::record_stage(
            store.conn(),
            "generation",
            "thread",
            "other-model",
            2,
            Some(&reported)
        )
        .is_err()
    );
    assert_eq!(bills(&store)?, 1);
    assert_eq!(
        accounting::record_stage(store.conn(), "generation", "thread", "model", 2, None)?,
        None
    );
    assert_eq!(bills(&store)?, 1);
    assert!(accounting::finish(store.conn(), "generation", "thread", "model", 2, true).is_err());
    accounting::finish(store.conn(), "generation", "thread", "model", 2, false)?;
    accounting::finish(store.conn(), "generation", "thread", "model", 2, false)?;
    assert!(
        accounting::record_stage(
            store.conn(),
            "generation",
            "thread",
            "model",
            3,
            Some(&reported)
        )
        .is_err()
    );
    assert_eq!(
        accounting::record_stage(
            store.conn(),
            "generation",
            "thread",
            "model",
            1,
            Some(&reported)
        )?,
        id
    );
    let total = crate::token_usage::aggregate_by_thread(store.conn(), "thread")?;
    assert_eq!(total.input_tokens, 100);
    assert_eq!(total.output_tokens, 10);
    assert_eq!(total.cache_read_tokens, 20);
    Ok(())
}

#[test]
fn precompute_accounting_requires_contiguous_stages_and_survives_retirement_and_thread_deletion()
-> TestResult {
    let store = fixture(false)?;
    ready::begin_generation(store.conn(), "generation", "thread", "model", None)?;
    accounting::record_stage(
        store.conn(),
        "generation",
        "thread",
        "model",
        2,
        Some(&usage()),
    )?;
    assert!(accounting::finish(store.conn(), "generation", "thread", "model", 1, true).is_err());
    assert!(accounting::finish(store.conn(), "generation", "thread", "model", 2, true).is_err());
    assert!(ready::retire(store.conn(), "thread", "generation")?);
    accounting::record_stage(store.conn(), "generation", "thread", "model", 1, None)?;
    accounting::finish(store.conn(), "generation", "thread", "model", 2, false)?;
    assert_eq!(bills(&store)?, 1);
    assert!(ready::begin_generation(store.conn(), "generation", "thread", "model", None).is_err());
    store.conn().execute_batch(
        "DELETE FROM tool_calls; DELETE FROM messages; DELETE FROM threads WHERE id='thread';",
    )?;
    assert_eq!(bills(&store)?, 1);
    assert_eq!(
        accounting::record_stage(
            store.conn(),
            "generation",
            "thread",
            "model",
            2,
            Some(&usage())
        )?,
        Some(1)
    );
    assert_eq!(bills(&store)?, 1);
    Ok(())
}

#[test]
fn ready_publication_is_separate_from_install_and_recovery_retains_safe_appends() -> TestResult {
    let store = fixture(false)?;
    let ready = candidate(&store, "generation")?;
    assert!(ready::publish(store.conn(), &ready, 1000).is_err());
    ready::begin_generation(store.conn(), "generation", "thread", "model", None)?;
    assert!(ready::publish(store.conn(), &ready, 1000).is_err());
    assert!(ready::recover(store.conn(), "thread", 1000)?.is_none());
    accounting::record_stage(
        store.conn(),
        "generation",
        "thread",
        "model",
        1,
        Some(&usage()),
    )?;
    accounting::finish(store.conn(), "generation", "thread", "model", 1, true)?;
    ready::publish(store.conn(), &ready, 1000)?;
    ready::publish(store.conn(), &ready, 1000)?;
    assert_eq!(
        ready::recover(store.conn(), "thread", 1000)?,
        Some(ready.clone())
    );
    assert_eq!(
        ready::recover(store.conn(), "thread", 1000)?,
        Some(ready.clone())
    );
    assert!(ready::recover(store.conn(), "other", 1000)?.is_none());
    append_turn(&store)?;
    assert_eq!(
        ready::recover(store.conn(), "thread", 1000)?,
        Some(ready.clone())
    );
    store.conn().execute(
        "UPDATE threads SET model='other-model' WHERE id='thread'",
        [],
    )?;
    assert_eq!(ready::recover(store.conn(), "thread", 1000)?, Some(ready));
    assert_eq!(bills(&store)?, 1);
    assert!(
        crate::context_compaction::latest_checkpoint(store.conn(), "thread", "model")?.is_none()
    );
    assert!(ready::retire(store.conn(), "thread", "generation")?);
    assert!(ready::recover(store.conn(), "thread", 1000)?.is_none());
    assert_eq!(bills(&store)?, 1);
    Ok(())
}

#[test]
fn ready_publication_cancellation_rolls_back_without_losing_accounting() -> TestResult {
    let store = fixture(false)?;
    let candidate = candidate(&store, "cancelled")?;
    start_and_settle(&store, "cancelled", None)?;
    let mut guards = 0;
    let result = ready::publish_with_guard(store.conn(), &candidate, 1000, || {
        guards += 1;
        guards == 1
    });
    assert!(matches!(result, Err(ContextCheckpointError::Cancelled)));
    assert_eq!(guards, 2);
    assert!(ready::recover(store.conn(), "thread", 1000)?.is_none());
    assert_eq!(bills(&store)?, 1);
    ready::publish_with_guard(store.conn(), &candidate, 1000, || true)?;
    assert_eq!(
        ready::recover(store.conn(), "thread", 1000)?,
        Some(candidate)
    );
    assert_eq!(bills(&store)?, 1);
    Ok(())
}

#[test]
fn ready_generation_cas_prevents_late_publication_removal_and_resurrection() -> TestResult {
    let store = fixture(false)?;
    let old = candidate(&store, "old")?;
    start_and_settle(&store, "old", None)?;
    ready::publish(store.conn(), &old, 1000)?;
    let new = candidate(&store, "new")?;
    assert!(ready::begin_generation(store.conn(), "new", "thread", "model", None).is_err());
    start_and_settle(&store, "new", Some("old"))?;
    assert!(ready::publish(store.conn(), &old, 1000).is_err());
    ready::publish(store.conn(), &new, 1000)?;
    assert!(!ready::retire(store.conn(), "thread", "old")?);
    assert_eq!(
        ready::recover(store.conn(), "thread", 1000)?,
        Some(new.clone())
    );
    assert!(ready::retire(store.conn(), "thread", "new")?);
    assert!(ready::publish(store.conn(), &new, 1000).is_err());
    assert!(ready::begin_generation(store.conn(), "new", "thread", "model", None).is_err());
    assert_eq!(bills(&store)?, 2);
    Ok(())
}

#[test]
fn ready_recovery_rejects_expiry_malformed_payload_and_changed_source() -> TestResult {
    let store = fixture(false)?;
    let candidate = candidate(&store, "generation")?;
    start_and_settle(&store, "generation", None)?;
    ready::publish(store.conn(), &candidate, 1000)?;
    assert!(ready::recover(store.conn(), "thread", 999).is_err());
    assert!(ready::recover(store.conn(), "thread", 1000 + ready::MAX_READY_AGE_MS)?.is_some());
    assert!(ready::recover(store.conn(), "thread", 1001 + ready::MAX_READY_AGE_MS).is_err());
    let value = serde_json::to_value(&candidate)?;
    for (field, replacement) in [
        ("schema", serde_json::json!(2)),
        ("generation", serde_json::json!("other")),
        ("stage_count", serde_json::json!(2)),
        ("summary_dependencies", serde_json::json!("f".repeat(64))),
        ("unknown", serde_json::json!(true)),
    ] {
        let mut changed = value.clone();
        changed[field] = replacement;
        store.conn().execute(
            "UPDATE context_precompute_ready SET payload=?1 WHERE thread_id='thread'",
            [serde_json::to_string(&changed)?],
        )?;
        assert!(ready::recover(store.conn(), "thread", 1000).is_err());
    }
    store.conn().execute(
        "UPDATE context_precompute_ready SET payload=?1 WHERE thread_id='thread'",
        [candidate.encode()?],
    )?;
    store
        .conn()
        .execute("UPDATE messages SET content='edited' WHERE id='owner'", [])?;
    assert!(ready::recover(store.conn(), "thread", 1000).is_err());
    assert!(ready::ReadyCompaction::decode(&" ".repeat(ready::MAX_READY_BYTES + 1)).is_err());
    assert_eq!(bills(&store)?, 1);
    Ok(())
}

#[test]
fn ready_recovery_detects_redacted_predecessor_and_disallows_new_omission() -> TestResult {
    let store = fixture(false)?;
    let first = candidate(&store, "first")?;
    assert!(matches!(
        install_checkpoint(store.conn(), &first.checkpoint)?,
        ContextCheckpointInstall::Applied(_)
    ));
    let mut next = candidate(&store, "next")?;
    start_and_settle(&store, "next", None)?;
    let plan = next
        .checkpoint
        .projection_plan
        .as_mut()
        .ok_or("plan missing")?;
    let task = plan.covered.pop().ok_or("coverage missing")?;
    let owner = SourceCursor {
        message_id: "owner".into(),
        message_seq: 2,
        assistant: None,
    };
    plan.omitted.push(task.clone());
    plan.covered.push(SourceRange {
        after: task.through,
        through: Some(owner.clone()),
    });
    plan.fragments = vec![
        ProjectionFragment::Summary,
        ProjectionFragment::Source {
            range: SourceRange {
                after: Some(owner),
                through: None,
            },
        },
    ];
    assert!(ready::publish(store.conn(), &next, 1000).is_err());
    let next = candidate(&store, "next")?;
    ready::publish(store.conn(), &next, 1000)?;
    store.conn().execute(
        "UPDATE context_checkpoints SET summary='REDACTED' WHERE thread_id='thread'",
        [],
    )?;
    assert!(ready::recover(store.conn(), "thread", 1000).is_err());
    assert_eq!(bills(&store)?, 1);
    Ok(())
}

#[test]
fn pending_model_change_blocks_publication_but_unknown_usage_can_be_ready() -> TestResult {
    let store = fixture(false)?;
    let mut candidate = candidate(&store, "generation")?;
    candidate.usage_complete = false;
    ready::begin_generation(store.conn(), "generation", "thread", "model", None)?;
    assert_eq!(
        ready::current_generation(store.conn(), "thread")?.as_deref(),
        Some("generation")
    );
    accounting::record_stage(store.conn(), "generation", "thread", "model", 1, None)?;
    accounting::finish(store.conn(), "generation", "thread", "model", 1, false)?;
    store.conn().execute(
        "UPDATE threads SET pending_model='other-model' WHERE id='thread'",
        [],
    )?;
    assert!(ready::publish(store.conn(), &candidate, 1000).is_err());
    assert!(
        ready::begin_generation(
            store.conn(),
            "replacement",
            "thread",
            "model",
            Some("generation")
        )
        .is_err()
    );
    assert!(ready::recover(store.conn(), "thread", 1000)?.is_none());
    store.conn().execute(
        "UPDATE threads SET pending_model=NULL WHERE id='thread'",
        [],
    )?;
    ready::publish(store.conn(), &candidate, 1000)?;
    assert_eq!(
        ready::recover(store.conn(), "thread", 1000)?,
        Some(candidate)
    );
    assert_eq!(bills(&store)?, 0);
    Ok(())
}

#[test]
fn ready_plan_cannot_move_its_captured_boundary_into_a_later_append() -> TestResult {
    let store = fixture(false)?;
    let mut candidate = candidate(&store, "generation")?;
    start_and_settle(&store, "generation", None)?;
    append_turn(&store)?;
    let plan = candidate
        .checkpoint
        .projection_plan
        .as_mut()
        .ok_or("plan missing")?;
    let future = SourceCursor {
        message_id: "next-task".into(),
        message_seq: 3,
        assistant: None,
    };
    plan.covered[0].through = Some(future.clone());
    plan.fragments = vec![
        ProjectionFragment::Summary,
        ProjectionFragment::Source {
            range: SourceRange {
                after: Some(future),
                through: None,
            },
        },
    ];
    assert!(ready::publish(store.conn(), &candidate, 1000).is_err());
    assert!(ready::recover(store.conn(), "thread", 1000)?.is_none());
    assert_eq!(bills(&store)?, 1);
    Ok(())
}

#[test]
fn ready_and_accounting_survive_database_reopen_without_rebilling() -> TestResult {
    let directory =
        std::env::temp_dir().join(format!("vega-precompute-{}", ulid::Ulid::generate()));
    std::fs::create_dir(&directory)?;
    let path = directory.join("database.sqlite");
    let store = fixture(false)?;
    let candidate = candidate(&store, "generation")?;
    start_and_settle(&store, "generation", None)?;
    ready::publish(store.conn(), &candidate, 1000)?;
    store
        .conn()
        .execute("VACUUM INTO ?1", [path.to_str().ok_or("invalid path")?])?;
    drop(store);
    let reopened = Store::open(&path)?;
    reopened.migrate()?;
    assert_eq!(
        ready::recover(reopened.conn(), "thread", 1000)?,
        Some(candidate)
    );
    accounting::record_stage(
        reopened.conn(),
        "generation",
        "thread",
        "model",
        1,
        Some(&usage()),
    )?;
    assert_eq!(bills(&reopened)?, 1);
    assert!(ready::retire(reopened.conn(), "thread", "generation")?);
    drop(reopened);
    let reopened = Store::open(&path)?;
    assert!(ready::recover(reopened.conn(), "thread", 1000)?.is_none());
    assert_eq!(bills(&reopened)?, 1);
    drop(reopened);
    std::fs::remove_dir_all(directory)?;
    Ok(())
}
