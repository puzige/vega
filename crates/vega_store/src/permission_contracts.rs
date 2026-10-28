use rusqlite::{Connection, OptionalExtension, params};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContractState {
    pub authority_epoch: String,
    pub reset_pending: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThreadPermissionRecord {
    pub mode: String,
    pub revision: i64,
    pub grant: Option<String>,
}

pub(crate) fn initialize(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute(
        "INSERT INTO permission_contract_state(singleton, authority_epoch, reset_state) \
         VALUES (1, ?1, 'ready') ON CONFLICT(singleton) DO NOTHING",
        [ulid::Ulid::generate().to_string()],
    )?;
    load_state(transaction)?;
    Ok(())
}

pub fn record_run_fence(
    transaction: &rusqlite::Transaction<'_>,
    message_id: &str,
    thread_id: &str,
    fence: &str,
) -> rusqlite::Result<()> {
    let changed = transaction.execute(
        "INSERT INTO run_permission_fences(message_id, thread_id, permission_fence) \
         SELECT id, thread_id, ?3 FROM messages \
         WHERE id=?1 AND thread_id=?2 AND role='assistant' AND status='streaming'",
        params![message_id, thread_id, fence],
    )?;
    if changed != 1 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(())
}

pub fn load_run_fence(
    conn: &Connection,
    message_id: &str,
    thread_id: &str,
) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT f.permission_fence FROM run_permission_fences f JOIN messages m ON m.id=f.message_id WHERE f.message_id=?1 AND f.thread_id=?2 AND m.thread_id=?2 AND m.role='assistant' AND m.status='streaming'",
        params![message_id, thread_id],
        |row| row.get(0),
    ).optional()
}

pub fn load_interrupted_run_fence(
    conn: &Connection,
    message_id: &str,
    thread_id: &str,
    seq: i64,
) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT f.permission_fence FROM run_permission_fences f JOIN messages m ON m.id=f.message_id WHERE f.message_id=?1 AND f.thread_id=?2 AND m.thread_id=?2 AND m.seq=?3 AND m.role='assistant' AND m.status='interrupted'",
        params![message_id, thread_id, seq],
        |row| row.get(0),
    ).optional()
}

pub fn record_delegated_fence(
    conn: &rusqlite::Transaction<'_>,
    child: &str,
    message: &str,
    fence: &str,
) -> rusqlite::Result<()> {
    let changed = conn.execute(
        "INSERT INTO delegated_permission_fences(child_thread_id,child_message_id,permission_fence) SELECT child_thread_id,child_message_id,?3 FROM delegated_executions WHERE child_thread_id=?1 AND child_message_id=?2 AND state='running'",
        params![child, message, fence],
    )?;
    if changed != 1 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(())
}

pub fn load_delegated_fence(
    conn: &Connection,
    child: &str,
    message: &str,
) -> rusqlite::Result<String> {
    conn.query_row(
        "SELECT permission_fence FROM delegated_permission_fences WHERE child_thread_id=?1 AND child_message_id=?2",
        params![child, message],
        |row| row.get(0),
    )
}

pub fn interrupted_delegated_fence(
    conn: &Connection,
    child: &str,
    message: &str,
    parent: &str,
) -> rusqlite::Result<String> {
    conn.query_row(
        "SELECT f.permission_fence FROM delegated_executions e JOIN delegated_permission_fences f ON f.child_message_id=e.child_message_id AND f.child_thread_id=e.child_thread_id WHERE e.child_thread_id=?1 AND e.child_message_id=?2 AND e.parent_thread_id=?3 AND e.state='interrupted'",
        params![child, message, parent],
        |row| row.get(0),
    )
}

pub fn previous_delegated_fence(
    conn: &Connection,
    child: &str,
    message: &str,
    parent_thread: &str,
    parent_message: &str,
) -> rusqlite::Result<Option<(String, Option<String>)>> {
    let (previous, fence): (Option<String>, Option<String>) = conn.query_row(
        "SELECT e.previous_child_message_id,f.permission_fence FROM delegated_executions e LEFT JOIN delegated_permission_fences f ON f.child_message_id=e.previous_child_message_id AND f.child_thread_id=e.child_thread_id WHERE e.child_thread_id=?1 AND e.child_message_id=?2 AND e.parent_thread_id=?3 AND e.parent_message_id=?4 AND e.state='running'",
        params![child, message, parent_thread, parent_message],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    Ok(previous.map(|message| (message, fence)))
}

pub fn load_state(conn: &Connection) -> rusqlite::Result<ContractState> {
    conn.query_row(
        "SELECT authority_epoch, reset_state FROM permission_contract_state WHERE singleton=1",
        [],
        |row| {
            let authority_epoch: String = row.get(0)?;
            let state: String = row.get(1)?;
            if ulid::Ulid::from_string(&authority_epoch).is_err()
                || !matches!(state.as_str(), "ready" | "pending")
            {
                return Err(rusqlite::Error::InvalidQuery);
            }
            Ok(ContractState {
                authority_epoch,
                reset_pending: state == "pending",
            })
        },
    )
}

pub fn load_thread(
    conn: &Connection,
    thread_id: &str,
) -> rusqlite::Result<Option<ThreadPermissionRecord>> {
    conn.query_row(
        "SELECT permission_mode, permission_revision, full_access_grant FROM threads WHERE id=?1",
        [thread_id],
        |row| {
            let revision = row.get(1)?;
            if revision < 0 {
                return Err(rusqlite::Error::InvalidQuery);
            }
            Ok(ThreadPermissionRecord {
                mode: row.get(0)?,
                revision,
                grant: row.get(2)?,
            })
        },
    )
    .optional()
}

pub fn compare_exchange_thread(
    transaction: &rusqlite::Transaction<'_>,
    thread_id: &str,
    expected_revision: i64,
    expected_epoch: &str,
    mode: &str,
    grant: Option<&str>,
    updated_at: i64,
) -> rusqlite::Result<bool> {
    if expected_revision < 0
        || expected_revision == i64::MAX
        || !matches!(mode, "readonly" | "confirm" | "auto" | "full_access")
        || grant.is_some()
    {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let changed = transaction.execute(
        "UPDATE threads SET permission_mode=?1, permission_revision=permission_revision+1, \
         full_access_grant=?2, updated_at=?3 \
         WHERE id=?4 AND permission_revision=?5 \
         AND EXISTS (SELECT 1 FROM permission_contract_state \
         WHERE singleton=1 AND authority_epoch=?6 AND reset_state='ready')",
        params![
            mode,
            grant,
            updated_at,
            thread_id,
            expected_revision,
            expected_epoch
        ],
    )?;
    if changed == 1 {
        transaction.execute(
            "UPDATE credential_continuations SET state='unknown' \
             WHERE thread_id=?1 AND state='ready'",
            [thread_id],
        )?;
    }
    Ok(changed == 1)
}
