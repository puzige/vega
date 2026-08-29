//! Startup recovery for incomplete rows left by a killed agent process.

use rusqlite::{Connection, params};

/// Counts of stale rows normalized before a thread resumes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecoveryCounts {
    /// Streaming assistant rows changed to interrupted.
    pub messages_interrupted: usize,
    /// Pending approval rows changed to rejected.
    pub tools_rejected: usize,
    /// Approved/running tool rows changed to cancelled.
    pub tools_cancelled: usize,
}

/// Normalizes incomplete state for one thread in a single transaction.
pub fn recover_thread(
    conn: &Connection,
    thread_id: &str,
    now_ms: i64,
) -> Result<RecoveryCounts, rusqlite::Error> {
    let tx = conn.unchecked_transaction()?;
    let messages_interrupted = tx.execute(
        "UPDATE messages SET status = 'interrupted' \
         WHERE thread_id = ?1 AND status = 'streaming'",
        [thread_id],
    )?;
    let tools_rejected = tx.execute(
        "UPDATE tool_calls SET status = 'rejected', approval = 'deny', \
         output_text = 'Tool error: rejected during startup recovery because approval was incomplete.', \
         finished_at = ?1 \
         WHERE thread_id = ?2 AND status = 'pending_approval'",
        params![now_ms, thread_id],
    )?;
    let tools_cancelled = tx.execute(
        "UPDATE tool_calls SET status = 'cancelled', \
         output_text = 'Tool cancelled during startup recovery because execution was incomplete.', \
         finished_at = ?1 \
         WHERE thread_id = ?2 AND status IN ('approved', 'running')",
        params![now_ms, thread_id],
    )?;
    tx.commit()?;
    Ok(RecoveryCounts {
        messages_interrupted,
        tools_rejected,
        tools_cancelled,
    })
}
