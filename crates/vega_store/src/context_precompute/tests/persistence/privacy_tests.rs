use super::*;

#[test]
fn privacy_cleanup_retires_related_candidates_atomically_without_erasing_usage() -> TestResult {
    let store = fixture(false)?;
    start_and_settle(&store, "generation", None)?;
    let candidate = candidate(&store, "generation")?;
    ready::publish(store.conn(), &candidate, 1000)?;
    assert!(ready::recover(store.conn(), "thread", 1000)?.is_some());
    for (thread, parent) in [("child", Some("thread")), ("unrelated", None)] {
        store.conn().execute("INSERT INTO threads(id,model,parent_thread_id,created_at,updated_at) VALUES(?1,'model',?2,1,1)",params![thread,parent])?;
        store.conn().execute("INSERT INTO context_precompute_accounting(generation,thread_id,model) VALUES(?1,?1,'model')",[thread])?;
        store.conn().execute(
            "INSERT INTO context_precompute_slots(thread_id,generation) VALUES(?1,?1)",
            [thread],
        )?;
        store.conn().execute("INSERT INTO context_precompute_ready(thread_id,generation,payload) VALUES(?1,?1,'old-summary')",[thread])?;
    }
    let source = crate::credential_privacy::source_fingerprint(store.conn(), "thread")?;
    store.conn().execute_batch("CREATE TRIGGER reject_privacy_retirement BEFORE DELETE ON context_precompute_ready WHEN OLD.thread_id='thread' BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;")?;
    assert!(
        crate::credential_privacy::clean(store.conn(), "thread", &source, |text, offsets| (
            text.replace("RAW_TASK", "SAFE_MSG"),
            offsets.to_vec()
        ))
        .is_err()
    );
    assert_eq!(
        crate::messages::find(store.conn(), "task")?
            .ok_or("task")?
            .content,
        "RAW_TASK"
    );
    assert!(ready::recover(store.conn(), "thread", 1000)?.is_some());
    assert_eq!(bills(&store)?, 1);
    store
        .conn()
        .execute_batch("DROP TRIGGER reject_privacy_retirement;")?;
    crate::credential_privacy::clean(store.conn(), "thread", &source, |text, offsets| {
        (text.replace("RAW_TASK", "SAFE_MSG"), offsets.to_vec())
    })?;
    assert_eq!(
        crate::messages::find(store.conn(), "task")?
            .ok_or("task")?
            .content,
        "SAFE_MSG"
    );
    assert!(ready::recover(store.conn(), "thread", 1000)?.is_none());
    for table in ["context_precompute_ready", "context_precompute_slots"] {
        let affected: i64 = store.conn().query_row(
            &format!("SELECT COUNT(*) FROM {table} WHERE thread_id IN ('thread','child')"),
            [],
            |row| row.get(0),
        )?;
        assert_eq!(affected, 0);
        let unrelated: i64 = store.conn().query_row(
            &format!("SELECT COUNT(*) FROM {table} WHERE thread_id='unrelated'"),
            [],
            |row| row.get(0),
        )?;
        assert_eq!(unrelated, 1);
    }
    let retired: i64=store.conn().query_row("SELECT SUM(retired) FROM context_precompute_accounting WHERE thread_id IN ('thread','child')",[],|row|row.get(0))?;
    assert_eq!(retired, 2);
    assert_eq!(bills(&store)?, 1);
    let stages: i64 = store.conn().query_row(
        "SELECT COUNT(*) FROM context_precompute_stages WHERE generation='generation'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(stages, 1);
    assert!(ready::publish(store.conn(), &candidate, 1000).is_err());
    Ok(())
}
