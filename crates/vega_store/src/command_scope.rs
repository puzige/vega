use rusqlite::{Connection, OptionalExtension, Transaction, params};

pub struct ScopeAuditRecord<'a> {
    pub request_id: &'a str,
    pub thread_id: &'a str,
    pub run_id: &'a str,
    pub call_id: &'a str,
    pub request_json: &'a str,
}

pub fn approved_bash(
    conn: &Connection,
    thread: &str,
    run: &str,
    call: &str,
) -> rusqlite::Result<Option<(String, String)>> {
    conn.query_row(
        "SELECT input_json,approval FROM tool_calls WHERE id=?1 AND thread_id=?2 AND message_id=?3 AND tool='bash' AND status IN ('approved','running') AND approval IS NOT NULL AND output_text IS NULL AND finished_at IS NULL",
        params![call, thread, run],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ).optional()
}

pub fn insert_pending(
    tx: &Transaction<'_>,
    record: &ScopeAuditRecord<'_>,
    now: i64,
) -> rusqlite::Result<bool> {
    let _permit = crate::write_coordination::acquire(
        tx,
        crate::write_coordination::WriteCategory::Permissions,
    )?;
    Ok(tx.execute(
        "INSERT INTO command_scope_audits(request_id,thread_id,run_id,call_id,request,state,created_at,updated_at) SELECT ?1,?2,?3,?4,?5,'pending',?6,?6 WHERE EXISTS(SELECT 1 FROM messages WHERE id=?3 AND thread_id=?2 AND role='assistant' AND status='streaming') AND EXISTS(SELECT 1 FROM tool_calls WHERE id=?4 AND thread_id=?2 AND message_id=?3 AND tool='bash' AND status IN ('approved','running')) ON CONFLICT DO NOTHING",
        params![record.request_id, record.thread_id, record.run_id, record.call_id, record.request_json, now],
    )? == 1)
}

pub fn transition(
    tx: &Transaction<'_>,
    record: &ScopeAuditRecord<'_>,
    from: &str,
    to: &str,
    now: i64,
) -> rusqlite::Result<bool> {
    if !matches!(
        (from, to),
        (
            "pending",
            "approved" | "denied" | "timeout" | "cancelled" | "failed"
        ) | ("approved", "consumed" | "timeout" | "cancelled" | "failed")
    ) {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let _permit = crate::write_coordination::acquire(
        tx,
        crate::write_coordination::WriteCategory::Permissions,
    )?;
    Ok(tx.execute(
        "UPDATE command_scope_audits SET state=?7,updated_at=?8 WHERE request_id=?1 AND thread_id=?2 AND run_id=?3 AND call_id=?4 AND request=?5 AND state=?6",
        params![record.request_id, record.thread_id, record.run_id, record.call_id, record.request_json, from, to, now],
    )? == 1)
}
