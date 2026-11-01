use rusqlite::{Connection, OptionalExtension, params};

#[cfg(test)]
#[path = "shell_operations_tests.rs"]
mod tests;

pub struct ShellOperationRecord<'a> {
    pub operation_id: &'a str,
    pub thread_id: &'a str,
    pub run_id: &'a str,
    pub call_id: &'a str,
    pub generation: &'a str,
    pub workspace: &'a str,
    pub state: &'a str,
    pub exit_code: Option<i32>,
    pub error: Option<&'a str>,
    pub duration_ms: Option<u64>,
}

fn terminal(state: &str) -> bool {
    matches!(state, "succeeded" | "failed" | "timed_out" | "cancelled" | "outcome_unknown")
}

pub fn record(conn: &Connection, record: &ShellOperationRecord<'_>, now: i64) -> rusqlite::Result<()> {
    let tx = crate::write_coordination::WriteTransaction::new(conn, crate::write_coordination::WriteCategory::ToolCalls)?;
    let prior: Option<(String, String, String, String, String, String, Option<i32>, Option<String>, Option<i64>)> = tx.query_row(
        "SELECT thread_id,run_id,call_id,generation,workspace,state,exit_code,error,duration_ms FROM shell_operations WHERE operation_id=?1",
        [record.operation_id],
        |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,row.get(6)?,row.get(7)?,row.get(8)?)),
    ).optional()?;
    let duration = record.duration_ms.map(i64::try_from).transpose().map_err(|_| rusqlite::Error::InvalidQuery)?;
    if let Some((thread, run, call, generation, workspace, state, exit_code, error, prior_duration)) = prior {
        if thread != record.thread_id || run != record.run_id || call != record.call_id || generation != record.generation || workspace != record.workspace {
            return Err(rusqlite::Error::InvalidQuery);
        }
        if terminal(&state) {
            if state != record.state || exit_code != record.exit_code || error.as_deref() != record.error || prior_duration != duration {
                return Err(rusqlite::Error::InvalidQuery);
            }
        } else {
            let valid = match state.as_str() {
                "prepared" => matches!(record.state, "running" | "failed" | "cancelled" | "outcome_unknown"),
                "running" => matches!(record.state, "running" | "finishing" | "stopping") || terminal(record.state),
                "finishing" | "stopping" => record.state == state || terminal(record.state),
                _ => false,
            };
            if !valid { return Err(rusqlite::Error::InvalidQuery); }
            tx.execute("UPDATE shell_operations SET state=?2,exit_code=?3,error=?4,duration_ms=?5,updated_at=?6 WHERE operation_id=?1", params![record.operation_id,record.state,record.exit_code,record.error,duration,now])?;
        }
    } else {
        if record.state != "prepared" || record.exit_code.is_some() || record.error.is_some() || duration.is_some() {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let inserted = tx.execute(
            "INSERT INTO shell_operations(operation_id,thread_id,run_id,call_id,generation,workspace,state,created_at,updated_at) SELECT ?1,?2,?3,?4,?5,?6,'prepared',?7,?7 WHERE EXISTS(SELECT 1 FROM messages WHERE id=?3 AND thread_id=?2 AND role='assistant' AND status='streaming') AND EXISTS(SELECT 1 FROM tool_calls WHERE id=?4 AND thread_id=?2 AND message_id=?3 AND tool='bash' AND status='running')",
            params![record.operation_id,record.thread_id,record.run_id,record.call_id,record.generation,record.workspace,now],
        )?;
        if inserted != 1 { return Err(rusqlite::Error::InvalidQuery); }
    }
    tx.commit()
}

pub fn has_unknown_workspace(conn: &Connection, workspace: &str) -> rusqlite::Result<bool> {
    conn.query_row("SELECT EXISTS(SELECT 1 FROM shell_operations WHERE workspace=?1 AND state='outcome_unknown')", [workspace], |row| row.get(0))
}

pub fn recover_thread(conn: &Connection, thread: &str, now: i64) -> rusqlite::Result<usize> {
    conn.execute("UPDATE shell_operations SET state='outcome_unknown',error='session_expired',updated_at=?2 WHERE thread_id=?1 AND state IN ('prepared','running','finishing','stopping')", params![thread,now])
}
