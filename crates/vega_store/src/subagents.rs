use rusqlite::{Connection, OptionalExtension, params};

#[derive(Clone, PartialEq, Eq)]
pub struct DelegationRow {
    pub child_thread_id: String,
    pub parent_thread_id: String,
    pub parent_message_id: String,
    pub spawn_call_id: String,
    pub child_message_id: String,
    pub name: String,
    pub task: String,
    pub state: String,
    pub result: String,
    pub truncated: bool,
    pub created_at: i64,
}

const COLUMNS: &str = "child_thread_id,parent_thread_id,parent_message_id,request_call_id,child_message_id,name,task,state,result,truncated,created_at";

pub struct ExecutionRow {
    pub record: DelegationRow,
    pub ordinal: i64,
    pub instruction_message_id: Option<String>,
}

fn row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DelegationRow> {
    Ok(DelegationRow {
        child_thread_id: row.get(0)?,
        parent_thread_id: row.get(1)?,
        parent_message_id: row.get(2)?,
        spawn_call_id: row.get(3)?,
        child_message_id: row.get(4)?,
        name: row.get(5)?,
        task: row.get(6)?,
        state: row.get(7)?,
        result: row.get(8)?,
        truncated: row.get(9)?,
        created_at: row.get(10)?,
    })
}

pub fn list(
    conn: &Connection,
    parent: &str,
    run: Option<&str>,
) -> rusqlite::Result<Vec<DelegationRow>> {
    let mut query = conn.prepare(&format!("SELECT {COLUMNS} FROM delegated_executions e WHERE parent_thread_id=?1 AND (?2 IS NULL OR parent_message_id=?2) AND NOT EXISTS(SELECT 1 FROM delegated_executions newer WHERE newer.child_thread_id=e.child_thread_id AND newer.ordinal>e.ordinal) ORDER BY (SELECT MIN(created_at) FROM delegated_executions first WHERE first.child_thread_id=e.child_thread_id),child_thread_id"))?;
    query.query_map(params![parent, run], row)?.collect()
}

pub fn find(conn: &Connection, child: &str) -> rusqlite::Result<Option<DelegationRow>> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM delegated_executions WHERE child_thread_id=?1 ORDER BY ordinal DESC LIMIT 1"),
        [child],
        row,
    )
    .optional()
}

pub fn execution(
    conn: &Connection,
    child: &str,
    message: &str,
) -> rusqlite::Result<Option<ExecutionRow>> {
    conn.query_row(
        &format!("SELECT {COLUMNS},ordinal,instruction_message_id FROM delegated_executions WHERE child_thread_id=?1 AND child_message_id=?2"),
        params![child, message],
        |value| Ok(ExecutionRow { record: row(value)?, ordinal: value.get(11)?, instruction_message_id: value.get(12)? }),
    ).optional()
}

pub fn history(conn: &Connection, parent: &str) -> rusqlite::Result<Vec<DelegationRow>> {
    let mut query = conn.prepare(&format!("SELECT {COLUMNS} FROM delegated_executions WHERE parent_thread_id=?1 ORDER BY created_at,child_thread_id,ordinal"))?;
    query.query_map([parent], row)?.collect()
}

pub(crate) fn privacy_scope(
    conn: &Connection,
    thread: &str,
) -> rusqlite::Result<Vec<DelegationRow>> {
    let mut query = conn.prepare(&format!("SELECT {COLUMNS} FROM delegated_executions WHERE parent_thread_id=?1 OR child_thread_id=?1 ORDER BY child_thread_id,ordinal"))?;
    query.query_map([thread], row)?.collect()
}

pub fn active_count(conn: &Connection, parent: &str, run: &str) -> rusqlite::Result<usize> {
    let count:i64 = conn.query_row("SELECT COUNT(*) FROM delegated_executions WHERE parent_thread_id=?1 AND parent_message_id=?2 AND state IN ('running','cancelling')", params![parent,run], |row| row.get(0))?;
    usize::try_from(count).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, count))
}

pub fn insert(conn: &Connection, record: &DelegationRow) -> rusqlite::Result<()> {
    let _permit = crate::write_coordination::acquire(
        conn,
        crate::write_coordination::WriteCategory::Delegation,
    )?;
    let instruction: String = conn.query_row(
        "SELECT id FROM messages WHERE thread_id=?1 AND role='user' ORDER BY seq LIMIT 1",
        [&record.child_thread_id],
        |row| row.get(0),
    )?;
    conn.execute("INSERT INTO delegated_runs(child_thread_id,parent_thread_id,parent_message_id,spawn_call_id,child_message_id,name,task,state,result,truncated,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)", params![record.child_thread_id,record.parent_thread_id,record.parent_message_id,record.spawn_call_id,record.child_message_id,record.name,record.task,record.state,record.result,record.truncated,record.created_at])?;
    conn.execute(
        "UPDATE threads SET parent_thread_id=?2,workspace_owner_thread_id=(SELECT COALESCE(workspace_owner_thread_id,id) FROM threads WHERE id=?2 AND project_id IS NULL) WHERE id=?1 AND backend='native' AND id<>?2",
        params![record.child_thread_id, record.parent_thread_id],
    )?;
    insert_execution(conn, record, 1, None, &instruction)?;
    Ok(())
}

fn insert_execution(
    conn: &Connection,
    record: &DelegationRow,
    ordinal: i64,
    previous: Option<&str>,
    instruction: &str,
) -> rusqlite::Result<()> {
    let _permit = crate::write_coordination::acquire(
        conn,
        crate::write_coordination::WriteCategory::Delegation,
    )?;
    conn.execute("INSERT INTO delegated_executions(child_thread_id,parent_thread_id,parent_message_id,request_call_id,child_message_id,name,task,state,result,truncated,created_at,ordinal,previous_child_message_id,kind,instruction_message_id) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)", params![record.child_thread_id,record.parent_thread_id,record.parent_message_id,record.spawn_call_id,record.child_message_id,record.name,record.task,record.state,record.result,record.truncated,record.created_at,ordinal,previous,if previous.is_some() { "followup" } else { "spawn" },instruction])?;
    Ok(())
}

pub fn append(
    conn: &Connection,
    record: &DelegationRow,
    previous: &str,
    instruction: &str,
) -> rusqlite::Result<()> {
    let old =
        execution(conn, &record.child_thread_id, previous)?.ok_or(rusqlite::Error::InvalidQuery)?;
    let latest = find(conn, &record.child_thread_id)?.ok_or(rusqlite::Error::InvalidQuery)?;
    let message = crate::messages::find(conn, instruction)?.ok_or(rusqlite::Error::InvalidQuery)?;
    if latest.child_message_id != previous
        || old.record.state != "completed"
        || old.record.parent_thread_id != record.parent_thread_id
        || old.record.parent_message_id != record.parent_message_id
        || old.instruction_message_id.is_none()
        || message.thread_id != record.child_thread_id
        || message.role != "user"
        || message.kind != "text"
        || message.status != "done"
        || message.content != record.task
    {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let ordinal = old
        .ordinal
        .checked_add(1)
        .ok_or(rusqlite::Error::InvalidQuery)?;
    insert_execution(conn, record, ordinal, Some(previous), instruction)
}

pub fn append_resume(
    conn: &Connection,
    record: &DelegationRow,
    previous: &str,
    instruction: &str,
) -> rusqlite::Result<()> {
    let old =
        execution(conn, &record.child_thread_id, previous)?.ok_or(rusqlite::Error::InvalidQuery)?;
    let latest = find(conn, &record.child_thread_id)?.ok_or(rusqlite::Error::InvalidQuery)?;
    let message = crate::messages::find(conn, instruction)?.ok_or(rusqlite::Error::InvalidQuery)?;
    if latest.child_message_id != previous
        || old.record.state != "interrupted"
        || old.record.parent_thread_id != record.parent_thread_id
        || old.instruction_message_id.is_none()
        || message.thread_id != record.child_thread_id
        || message.role != "user"
        || message.kind != "text"
        || message.status != "done"
        || message.content != record.task
    {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let ordinal = old
        .ordinal
        .checked_add(1)
        .ok_or(rusqlite::Error::InvalidQuery)?;
    insert_execution(conn, record, ordinal, Some(previous), instruction)
}

pub fn request_cancel(conn: &Connection, child: &str, run: &str) -> rusqlite::Result<bool> {
    let _permit = crate::write_coordination::acquire(
        conn,
        crate::write_coordination::WriteCategory::Delegation,
    )?;
    let changed = conn.execute("UPDATE delegated_executions SET state='cancelling' WHERE child_thread_id=?1 AND child_message_id=?2 AND state='running'", params![child,run])? == 1;
    if changed {
        conn.execute("UPDATE delegated_runs SET state='cancelling' WHERE child_thread_id=?1 AND child_message_id=?2 AND state='running'", params![child,run])?;
    }
    Ok(changed)
}

pub fn finish(
    conn: &Connection,
    child: &str,
    run: &str,
    state: &str,
    result: &str,
    truncated: bool,
) -> rusqlite::Result<bool> {
    let _permit = crate::write_coordination::acquire(
        conn,
        crate::write_coordination::WriteCategory::Delegation,
    )?;
    let changed = conn.execute("UPDATE delegated_executions SET state=?3,result=?4,truncated=?5 WHERE child_thread_id=?1 AND child_message_id=?2 AND state IN ('running','cancelling')", params![child,run,state,result,truncated])? == 1;
    if changed {
        conn.execute("UPDATE delegated_runs SET state=?3,result=?4,truncated=?5 WHERE child_thread_id=?1 AND child_message_id=?2 AND state IN ('running','cancelling')", params![child,run,state,result,truncated])?;
    }
    Ok(changed)
}

pub fn recover(conn: &Connection, child: &str) -> rusqlite::Result<()> {
    let _permit = crate::write_coordination::acquire(
        conn,
        crate::write_coordination::WriteCategory::Delegation,
    )?;
    conn.execute("UPDATE delegated_executions SET state='interrupted',result='Interrupted before completion',truncated=0 WHERE child_thread_id=?1 AND state IN ('running','cancelling')", [child])?;
    conn.execute("UPDATE delegated_runs SET state='interrupted',result='Interrupted before completion',truncated=0 WHERE child_thread_id=?1 AND state IN ('running','cancelling')", [child])?;
    Ok(())
}

pub fn attribute_usage(
    conn: &Connection,
    id: i64,
    child: &str,
    parent: &str,
    run: &str,
    cache_known: bool,
) -> rusqlite::Result<()> {
    let _permit = crate::write_coordination::acquire(
        conn,
        crate::write_coordination::WriteCategory::Delegation,
    )?;
    conn.execute("UPDATE token_usage SET delegated_parent_thread_id=?3,delegated_parent_message_id=?4,delegated_cache_known=?5 WHERE id=?1 AND thread_id=?2 AND delegated_parent_thread_id IS NULL",params![id,child,parent,run,cache_known])?;
    Ok(())
}

pub fn mark_usage_complete(
    conn: &Connection,
    child: &str,
    run: &str,
    complete: bool,
) -> rusqlite::Result<()> {
    let _permit = crate::write_coordination::acquire(
        conn,
        crate::write_coordination::WriteCategory::Delegation,
    )?;
    conn.execute("UPDATE delegated_executions SET usage_complete=?3 WHERE child_thread_id=?1 AND child_message_id=?2",params![child,run,complete])?;
    conn.execute("UPDATE delegated_runs SET usage_complete=?3 WHERE child_thread_id=?1 AND child_message_id=?2",params![child,run,complete])?;
    Ok(())
}

pub fn has_unknown_usage(conn: &Connection, parent: &str) -> rusqlite::Result<bool> {
    conn.query_row("SELECT EXISTS(SELECT 1 FROM delegated_executions WHERE parent_thread_id=?1 AND usage_complete=0)",[parent],|row| row.get(0))
}

pub fn run_usage_complete(conn: &Connection, parent: &str, run: &str) -> rusqlite::Result<bool> {
    conn.query_row("SELECT NOT EXISTS(SELECT 1 FROM delegated_executions WHERE parent_thread_id=?1 AND parent_message_id=?2 AND usage_complete=0) AND NOT EXISTS(SELECT 1 FROM token_usage WHERE delegated_parent_thread_id=?1 AND delegated_parent_message_id=?2 AND delegated_cache_known IS NOT 1)",params![parent,run],|row| row.get(0))
}
