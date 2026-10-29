use super::*;
use crate::context_compaction::install_checkpoint_and_retire_precompute_with_guard;

#[test]
fn foreground_install_rolls_back_candidate_retirement_on_failure_or_cancellation() -> TestResult {
    for mode in ["stale", "cancel", "cancel_after_retire", "retire_failure"] {
        let store = fixture(false)?;
        let original_source = crate::context_compaction::load_source(store.conn(), "thread")?;
        let old = candidate(&store, "old")?;
        start_and_settle(&store, "old", None)?;
        ready::publish(store.conn(), &old, 1000)?;
        let mut checkpoint = old.checkpoint.clone();
        checkpoint.summary = "FOREGROUND_SUMMARY".into();
        if mode == "stale" {
            checkpoint.source_fingerprint = "f".repeat(64);
        }
        if mode == "retire_failure" {
            store.conn().execute_batch(
                "CREATE TRIGGER reject_candidate_retire BEFORE DELETE ON context_precompute_slots BEGIN SELECT RAISE(FAIL, 'synthetic retirement failure'); END;",
            )?;
        }
        let result =
            install_checkpoint_and_retire_precompute_with_guard(store.conn(), &checkpoint, || {
                mode == "cancel"
                    || (mode == "cancel_after_retire"
                        && ready::current_generation(store.conn(), "thread")
                            .unwrap()
                            .is_none())
            });
        match mode {
            "stale" => assert!(matches!(
                result,
                Ok((ContextCheckpointInstall::Stale, None))
            )),
            "retire_failure" => assert!(matches!(result, Err(ContextCheckpointError::Store(_)))),
            _ => assert!(matches!(result, Err(ContextCheckpointError::Cancelled))),
        }
        assert_eq!(
            ready::recover(store.conn(), "thread", 1000)?,
            Some(old),
            "{mode}"
        );
        assert_eq!(bills(&store)?, 1, "{mode}");
        let checkpoints: i64 =
            store
                .conn()
                .query_row("SELECT COUNT(*) FROM context_checkpoints", [], |row| {
                    row.get(0)
                })?;
        assert_eq!(checkpoints, 0, "{mode}");
        assert_eq!(
            crate::context_compaction::load_source(store.conn(), "thread")?,
            original_source
        );
    }
    Ok(())
}

#[test]
fn foreground_install_retires_only_its_snapshot_and_reobservation_preserves_new_candidate()
-> TestResult {
    for published in [false, true] {
        let store = fixture(false)?;
        let original_source = crate::context_compaction::load_source(store.conn(), "thread")?;
        let old = candidate(&store, "old")?;
        start_and_settle(&store, "old", None)?;
        if published {
            ready::publish(store.conn(), &old, 1000)?;
        }
        let mut checkpoint = old.checkpoint.clone();
        checkpoint.summary = "FOREGROUND_SUMMARY".into();
        let (install, retired) =
            install_checkpoint_and_retire_precompute_with_guard(store.conn(), &checkpoint, || {
                false
            })?;
        let ContextCheckpointInstall::Applied(installed) = install else {
            panic!("not installed")
        };
        assert_eq!(retired.as_deref(), Some("old"));
        assert!(ready::current_generation(store.conn(), "thread")?.is_none());
        assert!(ready::recover(store.conn(), "thread", 1000)?.is_none());
        let retired: bool = store.conn().query_row(
            "SELECT retired FROM context_precompute_accounting WHERE generation='old'",
            [],
            |row| row.get(0),
        )?;
        assert!(retired);
        assert!(ready::publish(store.conn(), &old, 1000).is_err());
        assert!(matches!(
            ready::begin_generation_after_checkpoint(
                store.conn(),
                "late",
                "thread",
                "model",
                None,
                None,
            ),
            Err(ContextCheckpointError::Stale)
        ));
        assert!(ready::current_generation(store.conn(), "thread")?.is_none());
        let next = candidate(&store, "next")?;
        ready::begin_generation_after_checkpoint(
            store.conn(),
            "next",
            "thread",
            "model",
            None,
            Some(installed.id),
        )?;
        accounting::record_stage(store.conn(), "next", "thread", "model", 1, Some(&usage()))?;
        accounting::finish(store.conn(), "next", "thread", "model", 1, true)?;
        ready::publish(store.conn(), &next, 1000)?;
        assert!(!ready::retire(store.conn(), "thread", "old")?);
        let (reobserved, retired) =
            install_checkpoint_and_retire_precompute_with_guard(store.conn(), &checkpoint, || {
                false
            })?;
        assert!(
            matches!(reobserved, ContextCheckpointInstall::AlreadyCommitted(existing) if existing == installed)
        );
        assert!(retired.is_none());
        assert_eq!(ready::recover(store.conn(), "thread", 1000)?, Some(next));
        assert_eq!(bills(&store)?, 2);
        let checkpoints: i64 =
            store
                .conn()
                .query_row("SELECT COUNT(*) FROM context_checkpoints", [], |row| {
                    row.get(0)
                })?;
        assert_eq!(checkpoints, 1);
        assert_eq!(
            crate::context_compaction::load_source(store.conn(), "thread")?,
            original_source
        );
    }
    Ok(())
}
