use rusqlite::{Connection, OptionalExtension, Transaction, params};
use sha2::{Digest, Sha256};

#[derive(Clone, PartialEq, Eq)]
pub struct CredentialContinuation {
    pub id: String,
    pub thread_id: String,
    pub user_message_id: String,
    pub source_version: u64,
    pub source_fingerprint: String,
    pub raw_user_fingerprint: String,
    pub state: String,
    pub reason: String,
    pub permission_fence: Option<String>,
}

impl std::fmt::Debug for CredentialContinuation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CredentialContinuation")
            .field("id_bytes", &self.id.len())
            .field("source_version", &self.source_version)
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

pub fn find(conn: &Connection, id: &str) -> rusqlite::Result<Option<CredentialContinuation>> {
    conn.query_row(
        "SELECT id, thread_id, user_message_id, source_version, source_fingerprint, state, reason, raw_user_fingerprint, permission_fence FROM credential_continuations WHERE id = ?1",
        [id],
        |row| Ok(CredentialContinuation {
            id: row.get(0)?, thread_id: row.get(1)?, user_message_id: row.get(2)?,
            source_version: u64::try_from(row.get::<_, i64>(3)?).map_err(|_| rusqlite::Error::InvalidParameterName("negative continuation source version".into()))?, source_fingerprint: row.get(4)?, state: row.get(5)?, reason: row.get(6)?, raw_user_fingerprint: row.get(7)?, permission_fence: row.get(8)?,
        }),
    ).optional()
}

pub fn suspend(
    tx: &Transaction<'_>,
    thread_id: &str,
    message_id: &str,
    reason: &str,
    now: i64,
) -> rusqlite::Result<()> {
    let _permit = crate::write_coordination::acquire(
        tx,
        crate::write_coordination::WriteCategory::Credentials,
    )?;
    let source = crate::context_compaction::load_source_in_transaction(tx, thread_id)?;
    let user_message_id: String = tx.query_row(
        "SELECT id FROM messages WHERE thread_id = ?1 AND role = 'user' AND seq < (SELECT seq FROM messages WHERE id = ?2) ORDER BY seq DESC LIMIT 1",
        params![thread_id, message_id], |row| row.get(0),
    )?;
    let raw_user_fingerprint = raw_user_fingerprint(tx, &user_message_id)?;
    let source_version = sql_source_version(source.source_version)?;
    let permission_fence: Option<String> = tx.query_row(
        "SELECT permission_fence FROM run_permission_fences WHERE message_id=?1 AND thread_id=?2",
        params![message_id, thread_id],
        |row| row.get(0),
    ).optional()?;
    let unknown =
        permission_fence.is_none() || has_unknown_effects(tx, thread_id, &user_message_id)?;
    tx.execute(
        "INSERT INTO credential_continuations (id, thread_id, user_message_id, source_version, source_fingerprint, state, reason, created_at, raw_user_fingerprint, permission_fence) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) ON CONFLICT(id) DO NOTHING",
        params![message_id, thread_id, user_message_id, source_version, source.fingerprint, if unknown { "unknown" } else { "ready" }, reason, now, raw_user_fingerprint, permission_fence],
    )?;
    Ok(())
}

fn sql_source_version(version: u64) -> rusqlite::Result<i64> {
    i64::try_from(version).map_err(|_| {
        rusqlite::Error::InvalidParameterName("continuation source version overflow".into())
    })
}

pub(crate) fn raw_user_fingerprint(
    conn: &Connection,
    user_message_id: &str,
) -> rusqlite::Result<String> {
    let content: String = conn.query_row(
        "SELECT content FROM messages WHERE id = ?1 AND role = 'user'",
        [user_message_id],
        |row| row.get(0),
    )?;
    Ok(format!("{:x}", Sha256::digest(content.as_bytes())))
}

pub fn has_unknown_effects(
    conn: &Connection,
    thread_id: &str,
    user_message_id: &str,
) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM tool_calls WHERE thread_id = ?1 AND message_id IN (SELECT id FROM messages WHERE thread_id = ?1 AND seq > (SELECT seq FROM messages WHERE id = ?2)) AND status IN ('approved', 'running', 'pending_approval', 'cancelled'))",
        params![thread_id, user_message_id], |row| row.get(0),
    )
}

pub fn claim(
    tx: &Transaction<'_>,
    receipt: &CredentialContinuation,
    operation_id: &str,
    permission_fence: &str,
) -> rusqlite::Result<bool> {
    let _permit = crate::write_coordination::acquire(
        tx,
        crate::write_coordination::WriteCategory::Credentials,
    )?;
    let current =
        crate::context_compaction::load_source_version_in_transaction(tx, &receipt.thread_id)?;
    if receipt.permission_fence.as_deref() != Some(permission_fence)
        || current.source_version != receipt.source_version
        || current.fingerprint != receipt.source_fingerprint
        || raw_user_fingerprint(tx, &receipt.user_message_id)? != receipt.raw_user_fingerprint
        || has_unknown_effects(tx, &receipt.thread_id, &receipt.user_message_id)?
    {
        return Ok(false);
    }
    Ok(tx.execute(
        "UPDATE credential_continuations SET state = 'claimed', operation_id = ?2 WHERE id = ?1 AND state = 'ready' AND source_version = ?3 AND source_fingerprint = ?4 AND permission_fence = ?5 AND thread_id = ?6 AND user_message_id = ?7 AND raw_user_fingerprint = ?8",
        params![receipt.id, operation_id, sql_source_version(receipt.source_version)?, receipt.source_fingerprint, permission_fence, receipt.thread_id, receipt.user_message_id, receipt.raw_user_fingerprint],
    )? == 1)
}

pub fn attach(tx: &Transaction<'_>, receipt_id: &str, message_id: &str) -> rusqlite::Result<()> {
    let _permit = crate::write_coordination::acquire(
        tx,
        crate::write_coordination::WriteCategory::Credentials,
    )?;
    tx.execute("UPDATE credential_continuations SET continuation_message_id = ?2 WHERE id = ?1 AND state = 'claimed'", params![receipt_id, message_id])?;
    Ok(())
}

pub fn complete(conn: &Connection, message_id: &str) -> rusqlite::Result<()> {
    let _permit = crate::write_coordination::acquire(
        conn,
        crate::write_coordination::WriteCategory::Credentials,
    )?;
    conn.execute("UPDATE credential_continuations SET state = 'completed' WHERE continuation_message_id = ?1 AND state = 'claimed'", [message_id])?;
    Ok(())
}
