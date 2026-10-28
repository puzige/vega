use super::*;
use crate::context_precompute::consume;

fn prepared(store: &Store) -> Result<consume::PreparedConsumption, Box<dyn std::error::Error>> {
    let transaction = store.conn().unchecked_transaction()?;
    let prepared = consume::prepare_in_transaction(&transaction, "thread", None, 1000)?
        .ok_or("candidate absent")?;
    transaction.commit()?;
    Ok(prepared)
}

fn publish(store: &Store) -> TestResult {
    let candidate = candidate(store, "generation")?;
    start_and_settle(store, "generation", None)?;
    ready::publish(store.conn(), &candidate, 1000)?;
    Ok(())
}

fn checkpoint_count(store: &Store) -> Result<i64, rusqlite::Error> {
    store
        .conn()
        .query_row("SELECT COUNT(*) FROM context_checkpoints", [], |row| {
            row.get(0)
        })
}

#[test]
fn ready_consumption_installs_current_source_and_retires_without_rebilling() -> TestResult {
    let store = fixture(false)?;
    publish(&store)?;
    append_turn(&store)?;
    store.conn().execute(
        "UPDATE threads SET model='current-model' WHERE id='thread'",
        [],
    )?;
    let prepared = prepared(&store)?;
    let before = crate::context_compaction::load_source(store.conn(), "thread")?;
    assert!(prepared.usage_priced());
    let checkpoint = prepared.checkpoint("current-model", None, "current-estimator", 1000)?;
    assert_eq!(checkpoint.source_version, before.source_version);
    assert_eq!(
        checkpoint
            .projection_plan
            .as_ref()
            .unwrap()
            .task_message_id
            .as_deref(),
        Some("next-task")
    );
    assert_eq!(
        checkpoint.projection_plan.as_ref().unwrap().covered,
        prepared
            .candidate()
            .checkpoint
            .projection_plan
            .as_ref()
            .unwrap()
            .covered
    );
    assert!(matches!(
        consume::install_with_guard(store.conn(), &prepared, &checkpoint, 1000, || false)?,
        ContextCheckpointInstall::Applied(_)
    ));
    assert_eq!(checkpoint_count(&store)?, 1);
    assert_eq!(bills(&store)?, 1);
    assert_eq!(
        crate::context_compaction::load_source(store.conn(), "thread")?,
        before
    );
    assert!(ready::recover(store.conn(), "thread", 1000)?.is_none());
    assert!(matches!(
        consume::install_with_guard(store.conn(), &prepared, &checkpoint, 1000, || false)?,
        ContextCheckpointInstall::Stale
    ));
    assert!(matches!(
        install_checkpoint(store.conn(), &checkpoint)?,
        ContextCheckpointInstall::AlreadyCommitted(_)
    ));
    assert_eq!(checkpoint_count(&store)?, 1);
    assert_eq!(bills(&store)?, 1);
    Ok(())
}

#[test]
fn ready_consumption_refences_appended_hidden_bodies_and_screenshots() -> TestResult {
    for mutation in [
        "UPDATE messages SET content='CHANGED_RAW' WHERE id='next-task'",
        "UPDATE image_attachments SET encoded=X'0908' WHERE message_id='next-owner'",
    ] {
        let store = fixture(false)?;
        publish(&store)?;
        append_turn(&store)?;
        store.conn().execute_batch(
            "INSERT INTO image_attachments(message_id,ordinal,encoded) VALUES('next-owner',0,X'0708');
             INSERT INTO computer_screenshot_artifacts(call_id,message_id,ordinal) VALUES('next-call','next-owner',0);"
        )?;
        let prepared = prepared(&store)?;
        let checkpoint = prepared.checkpoint("model", None, "estimator", 1000)?;
        store.conn().execute(mutation, [])?;
        let current = crate::context_compaction::load_source(store.conn(), "thread")?;
        assert_eq!(
            current.fingerprint,
            prepared.source().fingerprint,
            "legacy fence: {mutation}"
        );
        assert_eq!(current.source_version, prepared.source().source_version);
        assert!(ready::recover(store.conn(), "thread", 1000)?.is_some());
        assert!(
            matches!(
                consume::install_with_guard(store.conn(), &prepared, &checkpoint, 1000, || false),
                Err(ContextCheckpointError::Stale)
            ),
            "{mutation}"
        );
        assert_eq!(checkpoint_count(&store)?, 0);
        assert_eq!(bills(&store)?, 1);
        assert!(ready::recover(store.conn(), "thread", 1000)?.is_some());
    }
    Ok(())
}

#[test]
fn ready_consumption_cancel_after_insert_or_retirement_rolls_back_both() -> TestResult {
    for after_retirement in [false, true] {
        let store = fixture(false)?;
        publish(&store)?;
        let prepared = prepared(&store)?;
        let checkpoint = prepared.checkpoint("model", None, "estimator", 1000)?;
        let observed = std::cell::Cell::new(false);
        let result = consume::install_with_guard(
            store.conn(),
            &prepared,
            &checkpoint,
            1000,
            || {
                let installed = checkpoint_count(&store).unwrap() == 1;
                let retired: bool = store.conn().query_row(
                "SELECT retired FROM context_precompute_accounting WHERE generation='generation'", [], |row| row.get(0)
            ).unwrap();
                let cancel = installed && (!after_retirement || retired);
                observed.set(observed.get() || cancel);
                cancel
            },
        );
        assert!(observed.get());
        assert!(matches!(result, Err(ContextCheckpointError::Cancelled)));
        assert_eq!(checkpoint_count(&store)?, 0);
        assert!(ready::recover(store.conn(), "thread", 1000)?.is_some());
        assert_eq!(bills(&store)?, 1);
    }
    Ok(())
}

#[test]
fn ready_consumption_rejects_changed_identity_source_authority_and_payload() -> TestResult {
    for mutation in [
        "generation",
        "append",
        "model",
        "pending",
        "predecessor",
        "payload",
        "checkpoint",
        "owner",
        "expiry",
    ] {
        let store = fixture(false)?;
        publish(&store)?;
        let prepared = prepared(&store)?;
        let mut checkpoint = prepared.checkpoint("model", None, "estimator", 1000)?;
        match mutation {
            "generation" => {
                let next = candidate(&store, "next-generation")?;
                start_and_settle(&store, "next-generation", Some("generation"))?;
                ready::publish(store.conn(), &next, 1000)?;
            }
            "append" => append_turn(&store)?,
            "model" => {
                store
                    .conn()
                    .execute("UPDATE threads SET model='different' WHERE id='thread'", [])?;
            }
            "pending" => {
                store.conn().execute(
                    "UPDATE threads SET pending_model='different' WHERE id='thread'",
                    [],
                )?;
            }
            "predecessor" => {
                install_checkpoint(store.conn(), &checkpoint)?;
            }
            "payload" => {
                let mut changed = prepared.candidate().clone();
                changed.checkpoint.summary = "CHANGED_SUMMARY".into();
                store.conn().execute(
                    "UPDATE context_precompute_ready SET payload=?1",
                    [changed.encode()?],
                )?;
            }
            "checkpoint" => checkpoint.summary = "UNTRUSTED_SUMMARY".into(),
            "owner" => assert!(
                prepared
                    .checkpoint("model", Some("task"), "estimator", 1000)
                    .is_err()
            ),
            _ => {}
        }
        if mutation == "owner" {
            continue;
        }
        let now = if mutation == "expiry" {
            1001 + ready::MAX_READY_AGE_MS
        } else {
            1000
        };
        let result =
            consume::install_with_guard(store.conn(), &prepared, &checkpoint, now, || false);
        assert!(
            matches!(
                result,
                Ok(ContextCheckpointInstall::Stale)
                    | Err(ContextCheckpointError::Stale | ContextCheckpointError::Invalid)
            ),
            "{mutation}: {result:?}"
        );
        assert_eq!(
            checkpoint_count(&store)?,
            i64::from(mutation == "predecessor")
        );
        let generation: String = store.conn().query_row(
            "SELECT generation FROM context_precompute_slots WHERE thread_id='thread'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(
            generation,
            if mutation == "generation" {
                "next-generation"
            } else {
                "generation"
            }
        );
        assert_eq!(bills(&store)?, if mutation == "generation" { 2 } else { 1 });
    }
    Ok(())
}

#[test]
fn ready_consumption_keeps_unknown_and_unpriced_accounting_distinct() -> TestResult {
    for mode in ["known", "unpriced", "unknown"] {
        let store = fixture(false)?;
        let mut candidate = candidate(&store, "generation")?;
        candidate.usage_complete = mode != "unknown";
        ready::begin_generation(store.conn(), "generation", "thread", "model", None)?;
        let mut reported = usage();
        if mode == "unpriced" {
            reported.pricing_version = None;
            reported.pricing_profile = None;
            reported.call_started_at = None;
        }
        accounting::record_stage(
            store.conn(),
            "generation",
            "thread",
            "model",
            1,
            (mode != "unknown").then_some(&reported),
        )?;
        accounting::finish(
            store.conn(),
            "generation",
            "thread",
            "model",
            1,
            mode != "unknown",
        )?;
        ready::publish(store.conn(), &candidate, 1000)?;
        let prepared = prepared(&store)?;
        assert_eq!(prepared.usage_priced(), mode == "known");
        assert_eq!(prepared.candidate().usage_complete, mode != "unknown");
        let checkpoint = prepared.checkpoint("model", None, "estimator", 1000)?;
        consume::install_with_guard(store.conn(), &prepared, &checkpoint, 1000, || false)?;
        assert_eq!(bills(&store)?, i64::from(mode != "unknown"));
    }
    Ok(())
}
