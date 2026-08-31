//! Tool-call audit CRUD over the existing `tool_calls` table.

use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension, params};

use crate::permissions::{self, InsertExactRule, PermissionsError};

/// Terminal persisted call used to validate call-id retry identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalToolCall {
    /// Original tool name.
    pub tool: String,
    /// Original JSON input.
    pub input_json: String,
    /// Persisted output.
    pub output: String,
    /// Persisted terminal DDL status.
    pub status: String,
    /// Strict approval JSON (or exact legacy value during compatibility reads).
    pub approval: String,
    /// Exact bash exit code when available.
    pub exit_code: Option<i32>,
    /// Exact bash duration when available.
    pub duration_ms: Option<u64>,
}

/// Identity and ownership fields for an existing provider call id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCallIdentity {
    /// Owning thread id.
    pub thread_id: String,
    /// Assistant message id that first proposed the call.
    pub message_id: String,
    /// Original tool name.
    pub tool: String,
    /// Original raw JSON input.
    pub input_json: String,
}

/// Immutable identity plus current lifecycle/audit state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCallState {
    /// Owning thread id.
    pub thread_id: String,
    /// Tool name.
    pub tool: String,
    /// Safe persisted input projection.
    pub input_json: String,
    /// Current lifecycle status.
    pub status: String,
    /// Strict approval JSON when decided.
    pub approval: Option<String>,
    /// Persisted bounded terminal output.
    pub output_text: Option<String>,
    /// Exact bash exit code when present.
    pub exit_code: Option<i32>,
    /// Stored signed duration; conversation validates the non-negative range.
    pub duration_ms: Option<i64>,
    /// Must remain absent in the S5 bounded-output contract.
    pub output_full_path: Option<String>,
}

/// Insertable tool-call fields.
pub struct NewToolCall<'a> {
    /// Provider call id.
    pub id: &'a str,
    /// Owning thread.
    pub thread_id: &'a str,
    /// Assistant message that proposed the call.
    pub message_id: &'a str,
    /// Monotonic call sequence.
    pub seq: i64,
    /// Tool name.
    pub tool: &'a str,
    /// Raw JSON arguments.
    pub input_json: &'a str,
    /// Initial DDL status.
    pub status: &'a str,
    /// Unix milliseconds.
    pub created_at: i64,
}

/// Exact rule to insert in the same transaction as an Always approval.
pub struct RememberExactRule<'a> {
    /// Project owning the rule.
    pub project_id: &'a str,
    /// Exact mutating tool name.
    pub tool: &'a str,
    /// Byte-exact command or normalized path.
    pub pattern: &'a str,
}

/// Terminal fields for the atomic invalid write/edit insertion.
pub struct ValidationRejectedToolCall<'a> {
    /// Immutable call identity and safe projection.
    pub call: NewToolCall<'a>,
    /// Strict deny/validation approval JSON.
    pub approval_json: &'a str,
    /// Content-free provider result.
    pub output_text: &'a str,
    /// Unix milliseconds.
    pub finished_at: i64,
}

/// Exact terminal update fields.
pub struct FinishToolCall<'a> {
    /// Provider call id.
    pub id: &'a str,
    /// Exact terminal status.
    pub status: &'a str,
    /// Bounded display/provider output.
    pub output_text: &'a str,
    /// Bash exit code when present.
    pub exit_code: Option<i32>,
    /// Bash duration when present.
    pub duration_ms: Option<u64>,
    /// Unix milliseconds.
    pub finished_at: i64,
}

/// Fail-closed lifecycle transition error.
#[derive(Debug, thiserror::Error)]
pub enum ToolCallTransitionError {
    /// SQLite operation failed.
    #[error("tool call persistence failed: {0}")]
    Store(#[from] rusqlite::Error),
    /// Permission-rule persistence failed in an Always transaction.
    #[error("permission persistence failed: {0}")]
    Permission(#[from] PermissionsError),
    /// Row is missing or not in the exact expected prior state.
    #[error("invalid tool call lifecycle transition")]
    InvalidTransition,
    /// Terminal status is outside the fixed vocabulary.
    #[error("invalid terminal tool call status")]
    InvalidTerminalStatus,
    /// Numeric metadata cannot be represented in SQLite.
    #[error("tool call metadata exceeds SQLite integer range")]
    MetadataRange,
}

/// Returns the next tool-call sequence for a thread.
pub fn next_seq(conn: &Connection, thread_id: &str) -> Result<i64, rusqlite::Error> {
    conn.query_row(
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM tool_calls WHERE thread_id = ?1",
        [thread_id],
        |row| row.get(0),
    )
}

/// Counts the persisted tool-call audit rows of one assistant message
/// (S7-T40 per-task summary; read-only aggregate over the existing audit).
pub fn count_by_message(
    conn: &Connection,
    thread_id: &str,
    message_id: &str,
) -> Result<u64, rusqlite::Error> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tool_calls WHERE thread_id = ?1 AND message_id = ?2",
        params![thread_id, message_id],
        |row| row.get(0),
    )?;
    Ok(u64::try_from(count).unwrap_or(0))
}

/// Inserts a proposed tool call.
pub fn insert(conn: &Connection, call: NewToolCall<'_>) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO tool_calls (id, thread_id, message_id, seq, tool, input_json, status, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![call.id, call.thread_id, call.message_id, call.seq, call.tool, call.input_json, call.status, call.created_at],
    )?;
    Ok(())
}

/// Inserts one ordinary proposal in `pending_approval`.
pub fn insert_pending(
    conn: &Connection,
    mut call: NewToolCall<'_>,
) -> Result<(), ToolCallTransitionError> {
    call.status = "pending_approval";
    insert(conn, call)?;
    Ok(())
}

/// Atomically inserts an invalid write/edit call directly as rejected.
pub fn insert_validation_rejected(
    conn: &Connection,
    rejected: ValidationRejectedToolCall<'_>,
) -> Result<(), ToolCallTransitionError> {
    conn.execute(
        "INSERT INTO tool_calls \
         (id, thread_id, message_id, seq, tool, input_json, output_text, status, approval, created_at, finished_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'rejected', ?8, ?9, ?10)",
        params![
            rejected.call.id,
            rejected.call.thread_id,
            rejected.call.message_id,
            rejected.call.seq,
            rejected.call.tool,
            rejected.call.input_json,
            rejected.output_text,
            rejected.approval_json,
            rejected.call.created_at,
            rejected.finished_at,
        ],
    )?;
    Ok(())
}

/// Transitions pending→approved and optionally remembers an exact rule in
/// the same SQLite transaction.
pub fn approve(
    conn: &Connection,
    id: &str,
    approval_json: &str,
    remember: Option<RememberExactRule<'_>>,
    now: i64,
) -> Result<(), ToolCallTransitionError> {
    let transaction = conn.unchecked_transaction()?;
    let updated = transaction.execute(
        "UPDATE tool_calls SET status = 'approved', approval = ?1 \
         WHERE id = ?2 AND status = 'pending_approval'",
        params![approval_json, id],
    )?;
    ensure_one(updated)?;
    if let Some(rule) = remember {
        permissions::insert_exact(
            &transaction,
            InsertExactRule {
                project_id: rule.project_id,
                tool: rule.tool,
                pattern: rule.pattern,
                created_at: now,
            },
        )?;
    }
    transaction.commit()?;
    Ok(())
}

/// Transitions pending→rejected with a strict terminal audit.
pub fn reject(
    conn: &Connection,
    id: &str,
    approval_json: &str,
    output_text: &str,
    finished_at: i64,
    remember: Option<RememberExactRule<'_>>,
) -> Result<(), ToolCallTransitionError> {
    let transaction = conn.unchecked_transaction()?;
    let updated = transaction.execute(
        "UPDATE tool_calls SET status = 'rejected', approval = ?1, output_text = ?2, finished_at = ?3 \
         WHERE id = ?4 AND status = 'pending_approval'",
        params![approval_json, output_text, finished_at, id],
    )?;
    ensure_one(updated)?;
    if let Some(rule) = remember {
        permissions::insert_exact(
            &transaction,
            InsertExactRule {
                project_id: rule.project_id,
                tool: rule.tool,
                pattern: rule.pattern,
                created_at: finished_at,
            },
        )?;
    }
    transaction.commit()?;
    Ok(())
}

/// Transitions approved→running.
pub fn mark_running(conn: &Connection, id: &str) -> Result<(), ToolCallTransitionError> {
    let updated = conn.execute(
        "UPDATE tool_calls SET status = 'running' WHERE id = ?1 AND status = 'approved'",
        [id],
    )?;
    ensure_one(updated)
}

/// Transitions running→one terminal execution state with bounded output and
/// exact bash metadata. `output_full_path` is deliberately untouched/NULL.
pub fn finish(
    conn: &Connection,
    finished: FinishToolCall<'_>,
) -> Result<(), ToolCallTransitionError> {
    if !matches!(finished.status, "success" | "failed" | "cancelled") {
        return Err(ToolCallTransitionError::InvalidTerminalStatus);
    }
    let duration_ms = finished
        .duration_ms
        .map(i64::try_from)
        .transpose()
        .map_err(|_| ToolCallTransitionError::MetadataRange)?;
    let updated = conn.execute(
        "UPDATE tool_calls SET status = ?1, output_text = ?2, exit_code = ?3, duration_ms = ?4, finished_at = ?5 \
         WHERE id = ?6 AND status = 'running'",
        params![
            finished.status,
            finished.output_text,
            finished.exit_code,
            duration_ms,
            finished.finished_at,
            finished.id,
        ],
    )?;
    ensure_one(updated)
}

fn ensure_one(updated: usize) -> Result<(), ToolCallTransitionError> {
    if updated == 1 {
        Ok(())
    } else {
        Err(ToolCallTransitionError::InvalidTransition)
    }
}

/// Whether a call id is already persisted.
pub fn exists(conn: &Connection, id: &str) -> Result<bool, rusqlite::Error> {
    let found: Option<i64> = conn
        .query_row("SELECT 1 FROM tool_calls WHERE id = ?1", [id], |row| {
            row.get(0)
        })
        .optional()?;
    Ok(found.is_some())
}

/// Loads ownership and immutable tool/input identity for one call id.
pub fn find_identity(
    conn: &Connection,
    id: &str,
) -> Result<Option<ToolCallIdentity>, rusqlite::Error> {
    conn.query_row(
        "SELECT thread_id, message_id, tool, input_json FROM tool_calls WHERE id = ?1",
        [id],
        |row| {
            Ok(ToolCallIdentity {
                thread_id: row.get(0)?,
                message_id: row.get(1)?,
                tool: row.get(2)?,
                input_json: row.get(3)?,
            })
        },
    )
    .optional()
}

/// Loads only globally occupied call ids owned by other threads. Their raw
/// inputs and results never cross into the current runtime.
pub fn foreign_call_ids(
    conn: &Connection,
    thread_id: &str,
) -> Result<Vec<String>, rusqlite::Error> {
    let mut stmt = conn.prepare("SELECT id FROM tool_calls WHERE thread_id != ?1 ORDER BY id")?;
    stmt.query_map([thread_id], |row| row.get(0))?.collect()
}

/// Loads the exact state required by conversation boundary validation.
pub fn find_state(conn: &Connection, id: &str) -> Result<Option<ToolCallState>, rusqlite::Error> {
    conn.query_row(
        "SELECT thread_id, tool, input_json, status, approval, output_text, exit_code, duration_ms, output_full_path FROM tool_calls WHERE id = ?1",
        [id],
        |row| {
            Ok(ToolCallState {
                thread_id: row.get(0)?,
                tool: row.get(1)?,
                input_json: row.get(2)?,
                status: row.get(3)?,
                approval: row.get(4)?,
                output_text: row.get(5)?,
                exit_code: row.get(6)?,
                duration_ms: row.get(7)?,
                output_full_path: row.get(8)?,
            })
        },
    )
    .optional()
}

/// Updates the lifecycle status, optional approval, and optional output.
pub fn update(
    conn: &Connection,
    id: &str,
    status: &str,
    approval: Option<&str>,
    output_text: Option<&str>,
    finished_at: Option<i64>,
) -> Result<usize, rusqlite::Error> {
    conn.execute(
        "UPDATE tool_calls SET status = ?1, approval = COALESCE(?2, approval), \
         output_text = COALESCE(?3, output_text), finished_at = COALESCE(?4, finished_at) \
         WHERE id = ?5",
        params![status, approval, output_text, finished_at, id],
    )
}

/// Loads every terminal result that has auditable output for restart/retry
/// call-id deduplication.
pub fn terminal_results(
    conn: &Connection,
    thread_id: &str,
) -> Result<HashMap<String, TerminalToolCall>, ToolCallTransitionError> {
    let corrupt_statuses: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tool_calls WHERE thread_id = ?1 AND status NOT IN \
         ('pending_approval', 'approved', 'running', 'success', 'failed', 'rejected', 'cancelled')",
        [thread_id],
        |row| row.get(0),
    )?;
    if corrupt_statuses != 0 {
        return Err(ToolCallTransitionError::InvalidTransition);
    }
    let mut stmt = conn.prepare(
        "SELECT id, tool, input_json, output_text, status, approval, exit_code, duration_ms, output_full_path FROM tool_calls \
         WHERE thread_id = ?1 AND status IN ('success', 'failed', 'rejected', 'cancelled')",
    )?;
    let rows = stmt.query_map([thread_id], |row| {
        Ok((
            row.get(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, Option<i32>>(6)?,
            row.get::<_, Option<i64>>(7)?,
            row.get::<_, Option<String>>(8)?,
        ))
    })?;
    let mut terminal = HashMap::new();
    for row in rows {
        let (id, tool, input_json, output, status, approval, exit_code, duration_ms, full_path) =
            row?;
        let (Some(output), Some(approval)) = (output, approval) else {
            return Err(ToolCallTransitionError::InvalidTransition);
        };
        if full_path.is_some() {
            return Err(ToolCallTransitionError::InvalidTransition);
        }
        let duration_ms = duration_ms
            .map(u64::try_from)
            .transpose()
            .map_err(|_| ToolCallTransitionError::MetadataRange)?;
        terminal.insert(
            id,
            TerminalToolCall {
                tool,
                input_json,
                output,
                status,
                approval,
                exit_code,
                duration_ms,
            },
        );
    }
    Ok(terminal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;

    fn store() -> Store {
        let store = Store::open(":memory:").unwrap();
        store.migrate().unwrap();
        store
            .conn()
            .execute(
                "INSERT INTO projects (id, path, name, created_at, last_opened_at) VALUES ('p', '/tmp/p', 'p', 0, 0)",
                [],
            )
            .unwrap();
        store
            .conn()
            .execute(
                "INSERT INTO threads (id, project_id, title, model, created_at, updated_at) VALUES ('t', 'p', '', '', 0, 0)",
                [],
            )
            .unwrap();
        store
            .conn()
            .execute(
                "INSERT INTO messages (id, thread_id, seq, role, content, status, created_at) VALUES ('m', 't', 1, 'assistant', '', 'streaming', 0)",
                [],
            )
            .unwrap();
        store
    }

    fn pending<'a>(id: &'a str, input: &'a str) -> NewToolCall<'a> {
        NewToolCall {
            id,
            thread_id: "t",
            message_id: "m",
            seq: 1,
            tool: "write",
            input_json: input,
            status: "ignored",
            created_at: 1,
        }
    }

    #[test]
    fn always_approval_and_rule_commit_atomically_and_duplicate_is_idempotent() {
        let store = store();
        insert_pending(store.conn(), pending("c", "safe")).unwrap();
        approve(
            store.conn(),
            "c",
            "strict",
            Some(RememberExactRule {
                project_id: "p",
                tool: "write",
                pattern: "src/lib.rs",
            }),
            2,
        )
        .unwrap();
        assert!(permissions::matches_exact(store.conn(), "p", "write", "src/lib.rs").unwrap());

        insert_pending(store.conn(), pending("c2", "safe2")).unwrap();
        approve(
            store.conn(),
            "c2",
            "strict",
            Some(RememberExactRule {
                project_id: "p",
                tool: "write",
                pattern: "src/lib.rs",
            }),
            3,
        )
        .unwrap();
        assert_eq!(permissions::list_exact(store.conn(), "p").unwrap().len(), 1);
    }

    #[test]
    fn invalid_rule_rolls_back_approval_and_rejection() {
        let store = store();
        insert_pending(store.conn(), pending("approve", "safe")).unwrap();
        assert!(
            approve(
                store.conn(),
                "approve",
                "strict",
                Some(RememberExactRule {
                    project_id: "p",
                    tool: "read",
                    pattern: "x",
                }),
                2,
            )
            .is_err()
        );
        assert_eq!(
            find_state(store.conn(), "approve").unwrap().unwrap().status,
            "pending_approval"
        );

        insert_pending(store.conn(), pending("reject", "safe")).unwrap();
        assert!(
            reject(
                store.conn(),
                "reject",
                "strict",
                "denied",
                2,
                Some(RememberExactRule {
                    project_id: "p",
                    tool: "read",
                    pattern: "x",
                }),
            )
            .is_err()
        );
        assert_eq!(
            find_state(store.conn(), "reject").unwrap().unwrap().status,
            "pending_approval"
        );
    }

    #[test]
    fn terminal_loader_fails_closed_on_nulls_full_path_and_negative_duration() {
        for mutation in [
            "UPDATE tool_calls SET output_text = NULL",
            "UPDATE tool_calls SET approval = NULL",
            "UPDATE tool_calls SET output_full_path = '/private/secret'",
            "UPDATE tool_calls SET duration_ms = -1",
            "UPDATE tool_calls SET status = 'corrupt'",
        ] {
            let store = store();
            store.conn().execute(
                "INSERT INTO tool_calls (id, thread_id, message_id, seq, tool, input_json, output_text, status, approval, created_at) VALUES ('c', 't', 'm', 1, 'bash', '{}', 'ok', 'success', 'once', 0)",
                [],
            ).unwrap();
            store.conn().execute(mutation, []).unwrap();
            assert!(terminal_results(store.conn(), "t").is_err(), "{mutation}");
        }
    }
}
