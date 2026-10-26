use rusqlite::{Connection, params};

pub fn record(
    conn: &Connection,
    thread_id: &str,
    completed_message_id: &str,
    event: &str,
    attempt: u8,
    occurred_at: i64,
) {
    if !matches!(
        event,
        "prepared"
            | "preparation_blocked"
            | "stream_ineligible"
            | "catalog_deferred"
            | "catalog_resumed"
            | "started"
            | "cancelled"
            | "stale"
            | "local_failure"
            | "attempt_started"
            | "suggestion"
            | "abstained"
            | "invalid_output"
            | "stream_interrupted"
            | "timeout"
            | "provider_retryable"
            | "provider_stopped"
            | "safety_stopped"
            | "unexpected_event"
            | "exhausted"
            | "applied"
            | "apply_rejected"
    ) {
        tracing::warn!("composer prediction diagnostic category rejected");
        return;
    }
    let result = (|| -> rusqlite::Result<()> {
        let _permit = crate::write_coordination::acquire(
            conn,
            crate::write_coordination::WriteCategory::Diagnostics,
        )?;
        conn.execute(
            "INSERT INTO composer_prediction_events (thread_id, completed_message_id, event, attempt, occurred_at)
             SELECT ?1, ?2, ?3, ?4, ?5 WHERE EXISTS (
                 SELECT 1 FROM messages WHERE id = ?2 AND thread_id = ?1
             )",
            params![thread_id, completed_message_id, event, attempt, occurred_at],
        )?;
        Ok(())
    })();
    if result.is_err() {
        tracing::warn!("composer prediction diagnostic persistence failed");
    }
}
