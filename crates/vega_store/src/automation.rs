use rusqlite::{Connection, OptionalExtension, params};

pub fn tasks(conn: &Connection, thread: &str) -> rusqlite::Result<Vec<String>> {
    conn.prepare("SELECT data FROM automation_tasks WHERE thread_id=?1 ORDER BY kind")?
        .query_map([thread], |row| row.get(0))?
        .collect()
}

pub fn all_tasks(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    conn.prepare("SELECT data FROM automation_tasks ORDER BY thread_id,kind")?
        .query_map([], |row| row.get(0))?
        .collect()
}

pub fn save_task(
    conn: &Connection,
    thread: &str,
    kind: &str,
    identity: &str,
    revision: u64,
    data: &str,
) -> rusqlite::Result<()> {
    let revision = i64::try_from(revision)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    conn.execute(
        "INSERT INTO automation_tasks(thread_id,kind,identity,revision,data) VALUES (?1,?2,?3,?4,?5) \
         ON CONFLICT(thread_id,kind) DO UPDATE SET identity=excluded.identity,revision=excluded.revision,data=excluded.data",
        params![thread, kind, identity, revision, data],
    )?;
    Ok(())
}

pub fn delete_task(conn: &Connection, thread: &str, kind: &str) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM automation_tasks WHERE thread_id=?1 AND kind=?2",
        params![thread, kind],
    )?;
    Ok(())
}

pub fn active_run(conn: &Connection, thread: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT data FROM automation_runs WHERE thread_id=?1 AND finished=0",
        [thread],
        |row| row.get(0),
    )
    .optional()
}

pub fn run(conn: &Connection, thread: &str, id: &str) -> rusqlite::Result<Option<(bool, String)>> {
    conn.query_row(
        "SELECT finished,data FROM automation_runs WHERE thread_id=?1 AND id=?2",
        params![thread, id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .optional()
}

pub fn insert_run(conn: &Connection, thread: &str, id: &str, data: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO automation_runs(id,thread_id,data) VALUES (?1,?2,?3)",
        params![id, thread, data],
    )?;
    Ok(())
}

pub fn save_run(
    conn: &Connection,
    thread: &str,
    id: &str,
    finished: bool,
    data: &str,
) -> rusqlite::Result<()> {
    conn.execute("UPDATE automation_runs SET data=?3,finished=?4 WHERE thread_id=?1 AND id=?2 AND finished=0", params![thread,id,data,finished])?;
    Ok(())
}

pub fn recover_claims(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute("UPDATE automation_runs SET finished=1 WHERE finished=0", [])?;
    Ok(())
}

pub fn active_runs(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    conn.prepare("SELECT data FROM automation_runs WHERE finished=0")?
        .query_map([], |row| row.get(0))?
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revision_preserves_sqlite_maximum_and_rejects_invalid_values_without_writes() {
        let store = crate::Store::open(":memory:").unwrap();
        store.migrate().unwrap();
        store.conn().execute_batch("INSERT INTO threads(id,model,created_at,updated_at) VALUES ('thread','model',1,1);").unwrap();
        let maximum = u64::try_from(i64::MAX).unwrap();
        save_task(
            store.conn(),
            "thread",
            "goal",
            "kept",
            maximum,
            r#"{"value":"kept"}"#,
        )
        .unwrap();
        let snapshot: (String, i64, String) = store.conn().query_row(
            "SELECT identity,revision,data FROM automation_tasks WHERE thread_id='thread' AND kind='goal'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).unwrap();
        assert_eq!(
            snapshot,
            ("kept".into(), i64::MAX, r#"{"value":"kept"}"#.into())
        );
        for revision in [0, maximum + 1, u64::MAX] {
            for kind in ["goal", "loop"] {
                let error = save_task(store.conn(), "thread", kind, "rejected", revision, "{}")
                    .unwrap_err();
                if revision == 0 {
                    assert!(matches!(error, rusqlite::Error::SqliteFailure(_, _)));
                } else {
                    assert!(matches!(error, rusqlite::Error::ToSqlConversionFailure(_)));
                }
                let actual: (String, i64, String) = store.conn().query_row(
                    "SELECT identity,revision,data FROM automation_tasks WHERE thread_id='thread' AND kind='goal'",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                ).unwrap();
                assert_eq!(actual, snapshot);
                assert_eq!(tasks(store.conn(), "thread").unwrap().len(), 1);
            }
        }
    }
}
