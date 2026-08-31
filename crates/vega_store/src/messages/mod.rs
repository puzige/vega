//! Message persistence and the add-only Plan review state machine.

use std::io;

use rusqlite::types::Type;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

const COLUMNS: &str = "id, thread_id, seq, role, kind, content, status, created_at, \
                       plan_status, plan_review_note, plan_reviewed_at";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageRow {
    pub id: String,
    pub thread_id: String,
    pub seq: i64,
    pub role: String,
    pub kind: String,
    pub content: String,
    pub status: String,
    pub created_at: i64,
    pub plan_status: Option<String>,
    pub plan_review_note: Option<String>,
    pub plan_reviewed_at: Option<i64>,
}

pub struct PlanReview<'a> {
    pub thread_id: &'a str,
    pub plan_id: &'a str,
    pub status: &'a str,
    pub note: Option<&'a str>,
    pub reviewed_at: i64,
    pub instruction: Option<PlanInstruction<'a>>,
}

pub struct PlanInstruction<'a> {
    pub id: &'a str,
    pub content: &'a str,
    pub created_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanReviewResult {
    Applied,
    Stale,
}

#[derive(Debug, thiserror::Error)]
pub enum PlanTransitionError {
    #[error("plan persistence failed")]
    Store(#[from] rusqlite::Error),
    #[error("plan transition found corrupt state")]
    CorruptState,
}

pub fn next_seq(conn: &Connection, thread_id: &str) -> Result<i64, rusqlite::Error> {
    conn.query_row(
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM messages WHERE thread_id = ?1",
        [thread_id],
        |row| row.get(0),
    )
}

pub fn insert(conn: &Connection, row: &MessageRow) -> Result<(), rusqlite::Error> {
    validate_message(row)?;
    conn.execute(
        "INSERT INTO messages (id, thread_id, seq, role, kind, content, status, created_at, \
         plan_status, plan_review_note, plan_reviewed_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            row.id,
            row.thread_id,
            row.seq,
            row.role,
            row.kind,
            row.content,
            row.status,
            row.created_at,
            row.plan_status,
            row.plan_review_note,
            row.plan_reviewed_at,
        ],
    )?;
    Ok(())
}

pub fn finish(
    conn: &Connection,
    id: &str,
    content: &str,
    status: &str,
) -> Result<usize, rusqlite::Error> {
    conn.execute(
        "UPDATE messages SET content = ?1, status = ?2 WHERE id = ?3",
        params![content, status, id],
    )
}

pub fn update_streaming_content(
    conn: &Connection,
    id: &str,
    content: &str,
) -> Result<usize, rusqlite::Error> {
    conn.execute(
        "UPDATE messages SET content = ?1 WHERE id = ?2 AND status = 'streaming'",
        params![content, id],
    )
}

pub fn finish_streaming(
    conn: &Connection,
    id: &str,
    content: &str,
    status: &str,
) -> Result<usize, rusqlite::Error> {
    conn.execute(
        "UPDATE messages SET content = ?1, status = ?2 WHERE id = ?3 AND status = 'streaming'",
        params![content, status, id],
    )
}

/// Completes a Plan and supersedes all older pending plans under one
/// `BEGIN IMMEDIATE` writer transaction.
pub fn complete_plan(
    conn: &Connection,
    thread_id: &str,
    message_id: &str,
    content: &str,
    now: i64,
) -> Result<(), PlanTransitionError> {
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    let mode: Option<String> = tx
        .query_row(
            "SELECT mode FROM threads WHERE id = ?1",
            [thread_id],
            |row| row.get(0),
        )
        .optional()?;
    if mode.as_deref() != Some("plan") {
        return Err(PlanTransitionError::CorruptState);
    }
    // Validate every plan or plan-metadata candidate before an UPDATE can
    // silently skip a semantically corrupt pending row.
    let _ = validated_plans(&tx, thread_id)?;
    tx.execute(
        "UPDATE messages SET plan_status = 'abandoned', \
         plan_review_note = 'superseded', plan_reviewed_at = ?1 \
         WHERE thread_id = ?2 AND id != ?3 AND role = 'assistant' \
         AND kind = 'plan' AND status = 'done' AND plan_status = 'pending' \
         AND plan_review_note IS NULL AND plan_reviewed_at IS NULL",
        params![now, thread_id, message_id],
    )?;
    let completed = tx.execute(
        "UPDATE messages SET content = ?1, kind = 'plan', status = 'done', \
         plan_status = 'pending' \
         WHERE id = ?2 AND thread_id = ?3 AND role = 'assistant' \
         AND kind = 'text' AND status = 'streaming' AND plan_status IS NULL \
         AND plan_review_note IS NULL AND plan_reviewed_at IS NULL",
        params![content, message_id, thread_id],
    )?;
    if completed != 1 {
        return Err(PlanTransitionError::CorruptState);
    }
    let touched = tx.execute(
        "UPDATE threads SET updated_at = ?1 WHERE id = ?2 AND mode = 'plan'",
        params![now, thread_id],
    )?;
    if touched != 1 {
        return Err(PlanTransitionError::CorruptState);
    }
    tx.commit()?;
    Ok(())
}

/// Applies one first-wins review. Approval changes Plan -> Execute and adds
/// its auditable user instruction before the same transaction commits.
pub fn review_plan(
    conn: &Connection,
    review: PlanReview<'_>,
) -> Result<PlanReviewResult, PlanTransitionError> {
    let approved = match review.status {
        "approved" => true,
        "changes_requested" | "abandoned" => false,
        _ => return Err(PlanTransitionError::CorruptState),
    };
    if approved != review.instruction.is_some() || (approved && review.note.is_some()) {
        return Err(PlanTransitionError::CorruptState);
    }
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    let _ = validated_plans(&tx, review.thread_id)?;
    let current = find(&tx, review.plan_id)?;
    let Some(current) = current else {
        tx.rollback()?;
        return Ok(PlanReviewResult::Stale);
    };
    if current.thread_id != review.thread_id
        || current.kind != "plan"
        || current.status != "done"
        || current.plan_status.as_deref() != Some("pending")
        || current.plan_review_note.is_some()
        || current.plan_reviewed_at.is_some()
    {
        tx.rollback()?;
        return Ok(PlanReviewResult::Stale);
    }
    let updated = tx.execute(
        "UPDATE messages SET plan_status = ?1, plan_review_note = ?2, \
         plan_reviewed_at = ?3 WHERE id = ?4 AND thread_id = ?5 \
         AND role = 'assistant' AND kind = 'plan' AND status = 'done' \
         AND plan_status = 'pending' AND plan_review_note IS NULL \
         AND plan_reviewed_at IS NULL",
        params![
            review.status,
            review.note,
            review.reviewed_at,
            review.plan_id,
            review.thread_id,
        ],
    )?;
    if updated == 0 {
        tx.rollback()?;
        return Ok(PlanReviewResult::Stale);
    }
    if updated != 1 {
        return Err(PlanTransitionError::CorruptState);
    }
    if let Some(instruction) = review.instruction {
        let mode_updated = tx.execute(
            "UPDATE threads SET mode = 'execute', updated_at = ?1 \
             WHERE id = ?2 AND mode = 'plan'",
            params![review.reviewed_at, review.thread_id],
        )?;
        if mode_updated != 1 {
            return Err(PlanTransitionError::CorruptState);
        }
        let seq = next_seq(&tx, review.thread_id)?;
        let inserted = tx.execute(
            "INSERT INTO messages (id, thread_id, seq, role, kind, content, status, created_at, \
             plan_status, plan_review_note, plan_reviewed_at) \
             VALUES (?1, ?2, ?3, 'user', 'text', ?4, 'done', ?5, NULL, NULL, NULL)",
            params![
                instruction.id,
                review.thread_id,
                seq,
                instruction.content,
                instruction.created_at,
            ],
        )?;
        if inserted != 1 {
            return Err(PlanTransitionError::CorruptState);
        }
    }
    tx.commit()?;
    Ok(PlanReviewResult::Applied)
}

pub fn recent(
    conn: &Connection,
    thread_id: &str,
    limit: usize,
) -> Result<Vec<MessageRow>, rusqlite::Error> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM (SELECT {COLUMNS} FROM messages \
         WHERE thread_id = ?1 AND status != 'streaming' ORDER BY seq DESC LIMIT ?2) \
         ORDER BY seq ASC"
    ))?;
    stmt.query_map(params![thread_id, limit as i64], row_from_query)?
        .collect()
}

/// Hard page cap (S8-T45/C7): one history page carries at most 200 messages.
pub const PAGE_LIMIT: usize = 200;

/// Cursor/keyset page bound: the highest eligible `seq`. The initial (newest)
/// page uses [`PageCursor::Head`]; older pages use `Before(oldest_loaded_seq)`.
/// A page never sees rows with `seq >= bound`, so pages cannot repeat or skip
/// rows while `seq` stays unique per thread (`UNIQUE(thread_id, seq)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageCursor {
    /// Newest end of the thread: no upper `seq` bound.
    Head,
    /// Rows with `seq` strictly below this value.
    Before(i64),
}

/// Typed page-request failure. `InvalidPageSize` is fail-closed: 0 is a
/// degenerate request and anything above [`PAGE_LIMIT`] is refused rather than
/// silently clamped (C7 200 上限 / +1 拒绝).
#[derive(Debug, thiserror::Error)]
pub enum PageRequestError {
    #[error("page size {0} is outside the 1..=200 page contract")]
    InvalidPageSize(usize),
    #[error("history page read failed")]
    Store(#[from] rusqlite::Error),
}

/// One bounded page of durable thread history.
///
/// `tool_calls` are the audit rows owned by this page's messages, batched in
/// one query (`WHERE message_id IN (page range subselect)`) and ordered by
/// `tool_calls.seq` — never per-message (C7 零 N+1). The page and its tool
/// rows come from a single deferred read transaction, so both observe the same
/// database snapshot (C7 一致快照). Raw persisted `input_json` stops at this
/// store boundary only; the conversation projection reduces it through the
/// owner redaction before any UI sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessagePage {
    /// Terminal rows in ascending `seq` order (streaming excluded, like
    /// [`recent`]; `interrupted`/`failed` rows are durable and included).
    pub rows: Vec<MessageRow>,
    /// `Some(oldest_seq)` when older rows may exist — pass
    /// [`PageCursor::Before`] to continue. `None` marks the oldest end of the
    /// thread, so another request cannot produce new rows.
    pub older_cursor: Option<i64>,
    /// Tool-call audit rows for this page's messages, ascending call seq.
    pub tool_calls: Vec<PageToolCall>,
}

/// Tool-call audit row as carried on a history page (bounded display fields;
/// `output_full_path` stays absent per the S5 bounded-output contract).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageToolCall {
    pub id: String,
    pub message_id: String,
    pub seq: i64,
    pub tool: String,
    pub input_json: String,
    pub output_text: Option<String>,
    pub status: String,
    pub approval: Option<String>,
    pub exit_code: Option<i32>,
    pub duration_ms: Option<i64>,
}

const PAGE_TOOL_COLUMNS: &str = "id, message_id, seq, tool, input_json, output_text, \
                                 status, approval, exit_code, duration_ms";

/// Reads one keyset page of durable history ending at `cursor`, oldest→newest.
///
/// Keyset shape: `WHERE thread_id = ? AND seq < bound ORDER BY seq DESC LIMIT N`
/// over `UNIQUE(thread_id, seq)` — pagination cost is O(page) regardless of
/// thread length. `limit` must be within `1..=[PAGE_LIMIT]`.
pub fn page_before(
    conn: &Connection,
    thread_id: &str,
    cursor: PageCursor,
    limit: usize,
) -> Result<MessagePage, PageRequestError> {
    if limit == 0 || limit > PAGE_LIMIT {
        return Err(PageRequestError::InvalidPageSize(limit));
    }
    let tx = conn.unchecked_transaction()?;
    let bound = match cursor {
        PageCursor::Head => i64::MAX,
        PageCursor::Before(seq) => seq,
    };
    let mut stmt = tx.prepare(&format!(
        "SELECT {COLUMNS} FROM (SELECT {COLUMNS} FROM messages \
         WHERE thread_id = ?1 AND status != 'streaming' AND seq < ?2 \
         ORDER BY seq DESC LIMIT ?3) ORDER BY seq ASC"
    ))?;
    let rows: Vec<MessageRow> = stmt
        .query_map(params![thread_id, bound, limit as i64], row_from_query)?
        .collect::<Result<_, _>>()?;
    let older_cursor = if rows.len() == limit {
        // Full page: more history may exist below the oldest row we have.
        rows.first().map(|row| row.seq)
    } else {
        None
    };
    let tool_calls = page_tool_calls(&tx, thread_id, &rows)?;
    drop(stmt);
    tx.commit()?;
    Ok(MessagePage {
        rows,
        older_cursor,
        tool_calls,
    })
}

/// Batch-reads the tool-call audit rows owned by `rows` in ONE query scoped to
/// the page's exact seq range (streaming rows excluded to mirror the page).
fn page_tool_calls(
    tx: &Transaction<'_>,
    thread_id: &str,
    rows: &[MessageRow],
) -> Result<Vec<PageToolCall>, rusqlite::Error> {
    let (Some(oldest), Some(newest)) = (rows.first(), rows.last()) else {
        return Ok(Vec::new());
    };
    let mut stmt = tx.prepare(&format!(
        "SELECT {PAGE_TOOL_COLUMNS} FROM tool_calls WHERE thread_id = ?1 AND message_id IN \
         (SELECT id FROM messages WHERE thread_id = ?1 AND seq >= ?2 AND seq <= ?3 \
         AND status != 'streaming') ORDER BY seq"
    ))?;
    let calls: Vec<PageToolCall> = stmt
        .query_map(params![thread_id, oldest.seq, newest.seq], |row| {
            Ok(PageToolCall {
                id: row.get(0)?,
                message_id: row.get(1)?,
                seq: row.get(2)?,
                tool: row.get(3)?,
                input_json: row.get(4)?,
                output_text: row.get(5)?,
                status: row.get(6)?,
                approval: row.get(7)?,
                exit_code: row.get(8)?,
                duration_ms: row.get(9)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(calls)
}

pub fn find(conn: &Connection, id: &str) -> Result<Option<MessageRow>, rusqlite::Error> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM messages WHERE id = ?1"),
        [id],
        row_from_query,
    )
    .optional()
}

/// Returns the latest terminal assistant message of a thread, if any
/// (S7-T40 per-task summary recovery scope; read-only projection).
pub fn last_terminal_assistant(
    conn: &Connection,
    thread_id: &str,
) -> Result<Option<MessageRow>, rusqlite::Error> {
    conn.query_row(
        &format!(
            "SELECT {COLUMNS} FROM messages WHERE thread_id = ?1 AND role = 'assistant' \
             AND status IN ('done', 'interrupted', 'failed') ORDER BY seq DESC LIMIT 1"
        ),
        [thread_id],
        row_from_query,
    )
    .optional()
}

pub fn plans_for_thread(
    conn: &Connection,
    thread_id: &str,
) -> Result<Vec<MessageRow>, rusqlite::Error> {
    validated_plans(conn, thread_id)
}

fn validated_plans(conn: &Connection, thread_id: &str) -> Result<Vec<MessageRow>, rusqlite::Error> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM messages WHERE thread_id = ?1 AND \
         (kind = 'plan' OR plan_status IS NOT NULL OR plan_review_note IS NOT NULL \
         OR plan_reviewed_at IS NOT NULL) ORDER BY seq"
    ))?;
    let plans: Vec<MessageRow> = stmt
        .query_map([thread_id], row_from_query)?
        .collect::<Result<_, _>>()?;
    if plans
        .iter()
        .filter(|plan| plan.plan_status.as_deref() == Some("pending"))
        .count()
        > 1
    {
        return Err(corrupt_row_error("multiple pending plans"));
    }
    Ok(plans)
}

fn row_from_query(row: &rusqlite::Row<'_>) -> Result<MessageRow, rusqlite::Error> {
    let message = MessageRow {
        id: row.get(0)?,
        thread_id: row.get(1)?,
        seq: row.get(2)?,
        role: row.get(3)?,
        kind: row.get(4)?,
        content: row.get(5)?,
        status: row.get(6)?,
        created_at: row.get(7)?,
        plan_status: row.get(8)?,
        plan_review_note: row.get(9)?,
        plan_reviewed_at: row.get(10)?,
    };
    validate_message(&message)?;
    Ok(message)
}

fn validate_message(message: &MessageRow) -> Result<(), rusqlite::Error> {
    let no_review = message.plan_status.is_none()
        && message.plan_review_note.is_none()
        && message.plan_reviewed_at.is_none();
    if message.kind != "plan" {
        return if no_review {
            Ok(())
        } else {
            Err(corrupt_row_error("non-plan review metadata"))
        };
    }
    if message.role != "assistant" {
        return Err(corrupt_row_error("plan role"));
    }
    if message.status != "done" {
        return Err(corrupt_row_error("plan lifecycle"));
    }
    match message.plan_status.as_deref() {
        Some("pending")
            if message.plan_review_note.is_none() && message.plan_reviewed_at.is_none() =>
        {
            Ok(())
        }
        Some("approved")
            if message.plan_review_note.is_none() && message.plan_reviewed_at.is_some() =>
        {
            Ok(())
        }
        Some("changes_requested" | "abandoned") if message.plan_reviewed_at.is_some() => Ok(()),
        _ => Err(corrupt_row_error("plan review state")),
    }
}

fn corrupt_row_error(reason: &'static str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        8,
        Type::Text,
        Box::new(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("corrupt message row: {reason}"),
        )),
    )
}

#[cfg(test)]
mod tests;
