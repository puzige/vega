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
