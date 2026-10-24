//! Startup recovery for incomplete rows left by a killed agent process.

use rusqlite::{Connection, params};

/// Canonical strict audit written for an incomplete approval at startup.
pub const RECOVERY_DENIAL_APPROVAL_JSON: &str =
    r#"{"decision":"deny","note":null,"source":"recovery","danger":null}"#;
/// Canonical provider-safe output for an incomplete startup approval.
pub const RECOVERY_REJECTED_OUTPUT: &str =
    "Tool error: rejected during startup recovery because approval was incomplete.";
/// Canonical provider-safe output for execution interrupted by startup.
pub const RECOVERY_CANCELLED_OUTPUT: &str =
    "Tool cancelled during startup recovery because execution was incomplete.";

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
    let _permit = crate::write_coordination::acquire(
        conn,
        crate::write_coordination::WriteCategory::Delegation,
    )?;
    let owner = crate::delegated_ownership::acquire_for_thread(conn, thread_id)?;
    let tx = crate::write_coordination::WriteTransaction::new(
        conn,
        crate::write_coordination::WriteCategory::Delegation,
    )?;
    let counts = recover_thread_with_owner(&tx, thread_id, now_ms, owner.as_deref())?;
    tx.commit()?;
    Ok(counts)
}

pub fn recover_thread_with_owner(
    tx: &rusqlite::Transaction<'_>,
    thread_id: &str,
    now_ms: i64,
    owner: Option<&crate::delegated_ownership::DelegatedOwnership>,
) -> Result<RecoveryCounts, rusqlite::Error> {
    let delegated = crate::subagents::find(tx, thread_id)?;
    if let Some(row) = &delegated
        && crate::delegated_stages::release_state(tx, thread_id, &row.child_message_id)?
            == "quarantined"
    {
        return Err(rusqlite::Error::InvalidParameterName(
            "delegated release remains unconfirmed".into(),
        ));
    }
    let abandoned = delegated.as_ref().filter(|row| {
        matches!(row.state.as_str(), "running" | "cancelling")
            || (row.state == "interrupted"
                && crate::delegated_stages::release_state(tx, thread_id, &row.child_message_id)
                    .is_ok_and(|state| state == "unknown"))
    });
    let effects = abandoned
        .map(|row| crate::delegated_stages::unknown_effects(tx, thread_id, &row.child_message_id))
        .transpose()?;
    let counts = recover_stopped_with_owner(tx, thread_id, now_ms, owner)?;
    if let Some(row) = abandoned {
        if crate::delegated_stages::release_state(tx, thread_id, &row.child_message_id)?
            == "unknown"
        {
            crate::delegated_stages::register(
                tx,
                thread_id,
                &row.child_message_id,
                None,
                &serde_json::json!([]),
            )?;
        }
        crate::delegated_stages::final_checkpoint(
            tx,
            thread_id,
            &row.child_message_id,
            true,
            effects.unwrap_or(serde_json::json!([])),
            now_ms,
        )?;
        crate::delegated_stages::set_release(
            tx,
            thread_id,
            &row.child_message_id,
            "recovered_abandoned",
        )?;
    }
    Ok(counts)
}

pub fn recover_stopped_with_owner(
    tx: &rusqlite::Transaction<'_>,
    thread_id: &str,
    now_ms: i64,
    owner: Option<&crate::delegated_ownership::DelegatedOwnership>,
) -> Result<RecoveryCounts, rusqlite::Error> {
    let _permit = crate::write_coordination::acquire(
        tx,
        crate::write_coordination::WriteCategory::Delegation,
    )?;
    if crate::threads::find(tx, thread_id)?.is_some_and(|row| row.parent_thread_id.is_some())
        && !owner.is_some_and(|owner| {
            crate::delegated_ownership::database_path(tx)
                .is_ok_and(|path| owner.verifies(&path, thread_id))
        })
    {
        return Err(rusqlite::Error::InvalidParameterName(
            "delegated writer proof required".into(),
        ));
    }
    let continuations = {
        let mut query = tx.prepare("SELECT c.continuation_message_id, c.reason, c.source_version, c.source_fingerprint, c.user_message_id, c.raw_user_fingerprint FROM credential_continuations c JOIN messages m ON m.id = c.continuation_message_id WHERE c.thread_id = ?1 AND c.state = 'claimed' AND m.status = 'streaming' AND c.user_message_id = (SELECT id FROM messages WHERE thread_id = ?1 AND role = 'user' ORDER BY seq DESC LIMIT 1)")?;
        query
            .query_map([thread_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?
    };
    let continuations = continuations
        .into_iter()
        .map(
            |(
                message_id,
                reason,
                source_version,
                fingerprint,
                user_message_id,
                raw_fingerprint,
            )| {
                let current = crate::context_compaction::source_version_without_continuation(
                    tx,
                    thread_id,
                    &message_id,
                )?;
                let source_version = u64::try_from(source_version).map_err(|_| {
                    rusqlite::Error::InvalidParameterName(
                        "negative continuation source version".into(),
                    )
                })?;
                let unchanged = current.source_version == source_version
                    && current.fingerprint == fingerprint
                    && crate::credential_continuation::raw_user_fingerprint(tx, &user_message_id)?
                        == raw_fingerprint;
                Ok((message_id, reason, unchanged))
            },
        )
        .collect::<rusqlite::Result<Vec<_>>>()?;
    crate::subagents::recover(tx, thread_id)?;
    let messages_interrupted = tx.execute(
        "UPDATE messages SET status = 'interrupted' \
         WHERE thread_id = ?1 AND status = 'streaming'",
        [thread_id],
    )?;
    let tools_rejected = tx.execute(
        "UPDATE tool_calls SET status = 'rejected', approval = ?1, output_text = ?2, \
         finished_at = ?3 \
         WHERE thread_id = ?4 AND status = 'pending_approval'",
        params![
            RECOVERY_DENIAL_APPROVAL_JSON,
            RECOVERY_REJECTED_OUTPUT,
            now_ms,
            thread_id
        ],
    )?;
    let tools_cancelled = tx.execute(
        "UPDATE tool_calls SET status = 'cancelled', output_text = ?1, finished_at = ?2 \
         WHERE thread_id = ?3 AND status IN ('approved', 'running')",
        params![RECOVERY_CANCELLED_OUTPUT, now_ms, thread_id],
    )?;
    for (message_id, reason, unchanged) in continuations {
        crate::credential_continuation::suspend(tx, thread_id, &message_id, &reason, now_ms)?;
        if !unchanged {
            tx.execute("UPDATE credential_continuations SET state = 'unknown', reason = 'source_changed' WHERE id = ?1", [&message_id])?;
        }
        crate::credential_continuation::complete(tx, &message_id)?;
    }
    Ok(RecoveryCounts {
        messages_interrupted,
        tools_rejected,
        tools_cancelled,
    })
}
