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
mod tests {
    use std::sync::{Arc, Barrier};

    use super::*;
    use crate::Store;

    fn setup() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("vega.db")).unwrap();
        store.migrate().unwrap();
        store
            .conn()
            .execute_batch(
                "INSERT INTO projects VALUES ('p','/tmp/p','p',NULL,0,0); \
                 INSERT INTO threads (id,project_id,mode,model,created_at,updated_at) \
                 VALUES ('t','p','plan','mock',0,0);",
            )
            .unwrap();
        (store, dir)
    }

    fn streaming(id: &str, seq: i64) -> MessageRow {
        MessageRow {
            id: id.into(),
            thread_id: "t".into(),
            seq,
            role: "assistant".into(),
            kind: "text".into(),
            content: String::new(),
            status: "streaming".into(),
            created_at: seq,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        }
    }

    #[test]
    fn completion_promotes_text_and_supersedes_exact_old_pending() {
        let (store, _dir) = setup();
        insert(store.conn(), &streaming("one", 1)).unwrap();
        complete_plan(store.conn(), "t", "one", "first", 10).unwrap();
        insert(store.conn(), &streaming("two", 2)).unwrap();
        complete_plan(store.conn(), "t", "two", "second", 20).unwrap();
        let plans = plans_for_thread(store.conn(), "t").unwrap();
        assert_eq!(plans.len(), 2);
        assert_eq!(plans[0].plan_status.as_deref(), Some("abandoned"));
        assert_eq!(plans[0].plan_review_note.as_deref(), Some("superseded"));
        assert_eq!(plans[0].plan_reviewed_at, Some(20));
        assert_eq!(plans[1].kind, "plan");
        assert_eq!(plans[1].plan_status.as_deref(), Some("pending"));
    }

    #[test]
    fn failed_current_promotion_rolls_back_supersede() {
        let (store, _dir) = setup();
        insert(store.conn(), &streaming("old", 1)).unwrap();
        complete_plan(store.conn(), "t", "old", "first", 10).unwrap();
        let error = complete_plan(store.conn(), "t", "missing", "second", 20).unwrap_err();
        assert!(matches!(error, PlanTransitionError::CorruptState));
        let old = find(store.conn(), "old").unwrap().unwrap();
        assert_eq!(old.plan_status.as_deref(), Some("pending"));
        assert_eq!(old.plan_reviewed_at, None);
    }

    #[test]
    fn corrupt_metadata_fails_every_read_and_blocks_completion() {
        let (store, _dir) = setup();
        store
            .conn()
            .execute_batch(
                "INSERT INTO messages (id,thread_id,seq,role,kind,content,status,created_at,plan_status,plan_review_note) \
                 VALUES ('bad','t',1,'assistant','plan','secret','done',0,'pending','illegal');",
            )
            .unwrap();
        insert(store.conn(), &streaming("new", 2)).unwrap();
        assert!(find(store.conn(), "bad").is_err());
        assert!(recent(store.conn(), "t", 10).is_err());
        assert!(plans_for_thread(store.conn(), "t").is_err());
        assert!(complete_plan(store.conn(), "t", "new", "new", 2).is_err());
        let new = find(store.conn(), "new").unwrap().unwrap();
        assert_eq!(new.status, "streaming");
    }

    #[test]
    fn non_plan_metadata_is_not_hidden_from_plan_validation() {
        let (store, _dir) = setup();
        store
            .conn()
            .execute(
                "INSERT INTO messages (id,thread_id,seq,role,kind,content,status,created_at,plan_status) \
                 VALUES ('bad','t',1,'assistant','text','x','done',0,'pending')",
                [],
            )
            .unwrap();
        assert!(plans_for_thread(store.conn(), "t").is_err());
    }

    #[test]
    fn approved_plan_with_review_note_fails_all_reads() {
        let (store, _dir) = setup();
        store
            .conn()
            .execute(
                "INSERT INTO messages \
                 (id,thread_id,seq,role,kind,content,status,created_at,plan_status,plan_review_note,plan_reviewed_at) \
                 VALUES ('bad','t',1,'assistant','plan','steps','done',0,'approved','secret',1)",
                [],
            )
            .unwrap();
        assert!(find(store.conn(), "bad").is_err());
        assert!(recent(store.conn(), "t", 10).is_err());
        assert!(plans_for_thread(store.conn(), "t").is_err());
    }

    #[test]
    fn approved_transition_with_note_is_rejected_without_mutation() {
        let (store, _dir) = setup();
        insert(store.conn(), &streaming("plan", 1)).unwrap();
        complete_plan(store.conn(), "t", "plan", "steps", 1).unwrap();
        let result = review_plan(
            store.conn(),
            PlanReview {
                thread_id: "t",
                plan_id: "plan",
                status: "approved",
                note: Some("illegal"),
                reviewed_at: 2,
                instruction: Some(PlanInstruction {
                    id: "instruction",
                    content: "approved",
                    created_at: 2,
                }),
            },
        );
        assert!(matches!(result, Err(PlanTransitionError::CorruptState)));
        let plan = find(store.conn(), "plan").unwrap().unwrap();
        assert_eq!(plan.plan_status.as_deref(), Some("pending"));
        assert!(find(store.conn(), "instruction").unwrap().is_none());
    }

    #[test]
    fn obsolete_streaming_plan_shape_is_corrupt_everywhere() {
        let (store, _dir) = setup();
        store
            .conn()
            .execute(
                "INSERT INTO messages (id,thread_id,seq,role,kind,content,status,created_at) \
                 VALUES ('bad','t',1,'assistant','plan','partial','streaming',0)",
                [],
            )
            .unwrap();
        insert(store.conn(), &streaming("new", 2)).unwrap();
        assert!(find(store.conn(), "bad").is_err());
        assert!(plans_for_thread(store.conn(), "t").is_err());
        assert!(complete_plan(store.conn(), "t", "new", "new", 3).is_err());
    }

    #[test]
    fn review_distinguishes_terminal_stale_from_corrupt_pending() {
        let (store, _dir) = setup();
        insert(store.conn(), &streaming("plan", 1)).unwrap();
        complete_plan(store.conn(), "t", "plan", "steps", 10).unwrap();
        let applied = review_plan(
            store.conn(),
            PlanReview {
                thread_id: "t",
                plan_id: "plan",
                status: "abandoned",
                note: None,
                reviewed_at: 11,
                instruction: None,
            },
        )
        .unwrap();
        assert_eq!(applied, PlanReviewResult::Applied);
        let stale = review_plan(
            store.conn(),
            PlanReview {
                thread_id: "t",
                plan_id: "plan",
                status: "abandoned",
                note: None,
                reviewed_at: 12,
                instruction: None,
            },
        )
        .unwrap();
        assert_eq!(stale, PlanReviewResult::Stale);

        store
            .conn()
            .execute_batch(
                "UPDATE messages SET plan_status='pending',plan_review_note='bad',plan_reviewed_at=NULL WHERE id='plan';",
            )
            .unwrap();
        assert!(
            review_plan(
                store.conn(),
                PlanReview {
                    thread_id: "t",
                    plan_id: "plan",
                    status: "abandoned",
                    note: None,
                    reviewed_at: 13,
                    instruction: None,
                }
            )
            .is_err()
        );
    }

    #[test]
    fn separate_connections_serialize_review_to_one_winner() {
        let (store, dir) = setup();
        insert(store.conn(), &streaming("plan", 1)).unwrap();
        complete_plan(store.conn(), "t", "plan", "steps", 10).unwrap();
        drop(store);
        let barrier = Arc::new(Barrier::new(3));
        let mut workers = Vec::new();
        for status in ["approved", "abandoned"] {
            let path = dir.path().join("vega.db");
            let barrier = barrier.clone();
            workers.push(std::thread::spawn(move || {
                let connection = Connection::open(path).unwrap();
                connection
                    .busy_timeout(std::time::Duration::from_secs(5))
                    .unwrap();
                barrier.wait();
                let instruction = (status == "approved").then_some(PlanInstruction {
                    id: "approval-instruction",
                    content: "approved",
                    created_at: 20,
                });
                review_plan(
                    &connection,
                    PlanReview {
                        thread_id: "t",
                        plan_id: "plan",
                        status,
                        note: None,
                        reviewed_at: 20,
                        instruction,
                    },
                )
                .unwrap()
            }));
        }
        barrier.wait();
        let results: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect();
        assert_eq!(
            results
                .iter()
                .filter(|result| **result == PlanReviewResult::Applied)
                .count(),
            1
        );
        assert_eq!(
            results
                .iter()
                .filter(|result| **result == PlanReviewResult::Stale)
                .count(),
            1
        );
    }

    #[test]
    fn completion_and_old_approval_obey_both_commit_orders() {
        let (store, _dir) = setup();
        insert(store.conn(), &streaming("old", 1)).unwrap();
        complete_plan(store.conn(), "t", "old", "old", 10).unwrap();
        insert(store.conn(), &streaming("new", 2)).unwrap();
        complete_plan(store.conn(), "t", "new", "new", 20).unwrap();
        let stale = review_plan(
            store.conn(),
            PlanReview {
                thread_id: "t",
                plan_id: "old",
                status: "approved",
                note: None,
                reviewed_at: 21,
                instruction: Some(PlanInstruction {
                    id: "late",
                    content: "late",
                    created_at: 21,
                }),
            },
        )
        .unwrap();
        assert_eq!(stale, PlanReviewResult::Stale);

        let (store, _dir) = setup();
        insert(store.conn(), &streaming("old", 1)).unwrap();
        complete_plan(store.conn(), "t", "old", "old", 10).unwrap();
        insert(store.conn(), &streaming("new", 2)).unwrap();
        assert_eq!(
            review_plan(
                store.conn(),
                PlanReview {
                    thread_id: "t",
                    plan_id: "old",
                    status: "approved",
                    note: None,
                    reviewed_at: 20,
                    instruction: Some(PlanInstruction {
                        id: "winner",
                        content: "winner",
                        created_at: 20,
                    }),
                },
            )
            .unwrap(),
            PlanReviewResult::Applied
        );
        assert!(complete_plan(store.conn(), "t", "new", "new", 21).is_err());
        let current = find(store.conn(), "new").unwrap().unwrap();
        assert_eq!(current.status, "streaming");
        assert_eq!(current.kind, "text");
    }

    #[test]
    fn separate_connections_serialize_completions_to_one_pending() {
        let (store, dir) = setup();
        insert(store.conn(), &streaming("a", 1)).unwrap();
        insert(store.conn(), &streaming("b", 2)).unwrap();
        drop(store);
        let barrier = Arc::new(Barrier::new(3));
        let mut workers = Vec::new();
        for (id, now) in [("a", 10), ("b", 20)] {
            let path = dir.path().join("vega.db");
            let barrier = barrier.clone();
            workers.push(std::thread::spawn(move || {
                let connection = Connection::open(path).unwrap();
                connection
                    .busy_timeout(std::time::Duration::from_secs(5))
                    .unwrap();
                barrier.wait();
                complete_plan(&connection, "t", id, id, now).unwrap();
            }));
        }
        barrier.wait();
        for worker in workers {
            worker.join().unwrap();
        }
        let reopened = Store::open(dir.path().join("vega.db")).unwrap();
        let plans = plans_for_thread(reopened.conn(), "t").unwrap();
        assert_eq!(plans.len(), 2);
        assert_eq!(
            plans
                .iter()
                .filter(|plan| plan.plan_status.as_deref() == Some("pending"))
                .count(),
            1
        );
        assert_eq!(
            plans
                .iter()
                .filter(|plan| plan.plan_status.as_deref() == Some("abandoned")
                    && plan.plan_review_note.as_deref() == Some("superseded"))
                .count(),
            1
        );
    }

    // ─── keyset page contract (S8-T45/C7) ────────────────────────────────────

    /// Seeds `count` terminal assistant rows plus one user row per message
    /// (2 rows per step, seqs 1..=2*count) and returns the store.
    fn seed_page_thread(count: usize) -> (Store, tempfile::TempDir) {
        let (store, dir) = setup();
        for step in 0..count {
            insert(
                store.conn(),
                &MessageRow {
                    id: format!("user-{step}"),
                    thread_id: "t".into(),
                    seq: (2 * step + 1) as i64,
                    role: "user".into(),
                    kind: "text".into(),
                    content: format!("问 {step}"),
                    status: "done".into(),
                    created_at: 1,
                    plan_status: None,
                    plan_review_note: None,
                    plan_reviewed_at: None,
                },
            )
            .unwrap();
            insert(
                store.conn(),
                &MessageRow {
                    id: format!("assistant-{step}"),
                    thread_id: "t".into(),
                    seq: (2 * step + 2) as i64,
                    role: "assistant".into(),
                    kind: "text".into(),
                    content: format!("答 {step}"),
                    status: "done".into(),
                    created_at: 1,
                    plan_status: None,
                    plan_review_note: None,
                    plan_reviewed_at: None,
                },
            )
            .unwrap();
        }
        (store, dir)
    }

    fn tool_call(id: &str, message_id: &str, seq: i64) -> String {
        format!(
            "INSERT INTO tool_calls \
             (id, thread_id, message_id, seq, tool, input_json, output_text, status, created_at) \
             VALUES ('{id}', 't', '{message_id}', {seq}, 'bash', '{{}}', 'out', 'success', 1)"
        )
    }

    #[test]
    fn page_size_boundaries_reject_zero_and_one_over_the_cap() {
        let (store, _dir) = seed_page_thread(3);
        let conn = store.conn();
        for rejected in [0usize, PAGE_LIMIT + 1, 5000] {
            let error = page_before(conn, "t", PageCursor::Head, rejected).unwrap_err();
            assert!(
                matches!(error, PageRequestError::InvalidPageSize(size) if size == rejected),
                "page size {rejected} must be refused, got {error:?}"
            );
        }
        // 1 / 199 / 200 stay inside the contract (200 = full thread here).
        assert_eq!(
            page_before(conn, "t", PageCursor::Head, 1)
                .unwrap()
                .rows
                .len(),
            1
        );
        assert_eq!(
            page_before(conn, "t", PageCursor::Head, PAGE_LIMIT)
                .unwrap()
                .rows
                .len(),
            6
        );
    }

    #[test]
    fn empty_thread_pages_to_exhaustion_without_cursor() {
        let (store, _dir) = setup();
        let page = page_before(store.conn(), "t", PageCursor::Head, PAGE_LIMIT).unwrap();
        assert!(page.rows.is_empty());
        assert_eq!(page.older_cursor, None);
        assert!(page.tool_calls.is_empty());
    }

    #[test]
    fn exactly_one_page_has_no_older_cursor() {
        let (store, _dir) = seed_page_thread(100); // 200 rows exactly
        let page = page_before(store.conn(), "t", PageCursor::Head, PAGE_LIMIT).unwrap();
        assert_eq!(page.rows.len(), 200);
        // The page is full yet there is nothing older: one extra read proves
        // exhaustion (a full page alone must not imply more history).
        assert_eq!(page.older_cursor, Some(1));
        let older = page_before(
            store.conn(),
            "t",
            PageCursor::Before(page.older_cursor.unwrap()),
            PAGE_LIMIT,
        )
        .unwrap();
        assert!(older.rows.is_empty());
        assert_eq!(older.older_cursor, None);
    }

    #[test]
    fn cursor_walk_covers_every_seq_exactly_once() {
        let (store, _dir) = seed_page_thread(250); // 500 rows
        let conn = store.conn();
        let mut seen: Vec<i64> = Vec::new();
        let mut cursor = PageCursor::Head;
        let mut rounds = 0;
        loop {
            let page = page_before(conn, "t", cursor, 199).unwrap();
            for row in &page.rows {
                seen.push(row.seq);
            }
            rounds += 1;
            match page.older_cursor {
                Some(seq) => cursor = PageCursor::Before(seq),
                None => break,
            }
        }
        assert_eq!(seen.len(), 500, "no duplicates and no gaps");
        // Pages arrive newest→oldest; sorted, the union must be exactly the
        // thread's seq range with nothing missing and nothing repeated.
        seen.sort_unstable();
        assert!(
            seen.iter().copied().eq(1..=500),
            "keyset walk must be strictly continuous"
        );
        assert_eq!(rounds, 3, "500 rows over 199-sized pages: 3 reads");
    }

    #[test]
    fn interrupted_and_failed_rows_are_durable_streaming_is_not() {
        let (store, _dir) = setup();
        for (id, seq, status) in [
            ("done", 1, "done"),
            ("interrupted", 2, "interrupted"),
            ("failed", 3, "failed"),
            ("streaming", 4, "streaming"),
        ] {
            insert(
                store.conn(),
                &MessageRow {
                    id: id.into(),
                    thread_id: "t".into(),
                    seq,
                    role: "assistant".into(),
                    kind: "text".into(),
                    content: id.into(),
                    status: status.into(),
                    created_at: 1,
                    plan_status: None,
                    plan_review_note: None,
                    plan_reviewed_at: None,
                },
            )
            .unwrap();
        }
        let page = page_before(store.conn(), "t", PageCursor::Head, PAGE_LIMIT).unwrap();
        let statuses: Vec<&str> = page
            .rows
            .iter()
            .map(|row| (row.id.as_str(), row.status.as_str()).0)
            .collect();
        assert_eq!(statuses, vec!["done", "interrupted", "failed"]);
    }

    #[test]
    fn mid_run_insert_never_reuses_or_steals_a_cursor_page() {
        let (store, _dir) = seed_page_thread(250); // 500 rows
        let conn = store.conn();
        let head = page_before(conn, "t", PageCursor::Head, 200).unwrap();
        assert_eq!(head.rows.first().unwrap().seq, 301);
        assert_eq!(head.rows.last().unwrap().seq, 500);
        // New work lands strictly above every loaded seq (next_seq = MAX+1).
        insert(
            conn,
            &MessageRow {
                id: "late".into(),
                thread_id: "t".into(),
                seq: next_seq(conn, "t").unwrap(),
                role: "user".into(),
                kind: "text".into(),
                content: "mid-run".into(),
                status: "done".into(),
                created_at: 2,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .unwrap();
        let older = page_before(
            conn,
            "t",
            PageCursor::Before(head.older_cursor.unwrap()),
            200,
        )
        .unwrap();
        assert_eq!(older.rows.len(), 200);
        assert_eq!(older.rows.first().unwrap().seq, 101);
        assert_eq!(older.rows.last().unwrap().seq, 300);
        assert_eq!(older.older_cursor, Some(101));
    }

    #[test]
    fn reopening_the_thread_resets_cursor_to_the_newest_page() {
        let (store, dir) = seed_page_thread(250);
        drop(store);
        let reopened = Store::open(dir.path().join("vega.db")).unwrap();
        let first = page_before(reopened.conn(), "t", PageCursor::Head, 200).unwrap();
        assert_eq!(first.rows.first().unwrap().seq, 301);
        assert_eq!(first.rows.last().unwrap().seq, 500);
        // A fresh open always starts from Head again; the old in-memory cursor
        // is view state, not durable state.
        let again = page_before(reopened.conn(), "t", PageCursor::Head, 200).unwrap();
        assert_eq!(again, first);
    }

    #[test]
    fn page_batches_tool_calls_of_exactly_its_messages() {
        let (store, _dir) = seed_page_thread(4); // 8 rows
        let conn = store.conn();
        conn.execute_batch(&tool_call("tc-old-1", "assistant-0", 1))
            .unwrap();
        conn.execute_batch(&tool_call("tc-old-2", "assistant-0", 2))
            .unwrap();
        conn.execute_batch(&tool_call("tc-new", "assistant-3", 3))
            .unwrap();
        let head = page_before(conn, "t", PageCursor::Head, 4).unwrap();
        // Newest 4 rows = user-2, assistant-2, user-3, assistant-3: only the
        // newest tool call belongs to this page.
        let ids: Vec<&str> = head
            .tool_calls
            .iter()
            .map(|call| call.id.as_str())
            .collect();
        assert_eq!(ids, vec!["tc-new"]);
        assert_eq!(head.tool_calls[0].message_id, "assistant-3");
        let older =
            page_before(conn, "t", PageCursor::Before(head.older_cursor.unwrap()), 4).unwrap();
        let ids: Vec<&str> = older
            .tool_calls
            .iter()
            .map(|call| call.id.as_str())
            .collect();
        assert_eq!(ids, vec!["tc-old-1", "tc-old-2"], "ordered by call seq");
        assert_eq!(older.tool_calls[0].message_id, "assistant-0");
    }

    #[test]
    fn page_read_issues_constant_statements_regardless_of_row_count() {
        use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

        static STATEMENTS: AtomicUsize = AtomicUsize::new(0);
        unsafe extern "C" fn trace_statements(
            code: u32,
            context: *mut std::ffi::c_void,
            _first: *mut std::ffi::c_void,
            _second: *mut std::ffi::c_void,
        ) -> i32 {
            // SQLITE_TRACE_STMT == 1: one compiled application statement.
            if code == 1 {
                let counter = unsafe { &*(context as *const AtomicUsize) };
                counter.fetch_add(1, AtomicOrdering::SeqCst);
            }
            0
        }
        let install_trace = |conn: &Connection| {
            let db = unsafe { conn.handle() };
            let registered = unsafe {
                rusqlite::ffi::sqlite3_trace_v2(
                    db,
                    1,
                    Some(trace_statements),
                    &STATEMENTS as *const AtomicUsize as *mut std::ffi::c_void,
                )
            };
            assert_eq!(registered, 0, "trace hook registration");
        };

        let (small, _small_dir) = seed_page_thread(1);
        let (large, _large_dir) = seed_page_thread(100);
        install_trace(small.conn());
        install_trace(large.conn());

        STATEMENTS.store(0, AtomicOrdering::SeqCst);
        page_before(small.conn(), "t", PageCursor::Head, PAGE_LIMIT).unwrap();
        let small_statements = STATEMENTS.load(AtomicOrdering::SeqCst);

        STATEMENTS.store(0, AtomicOrdering::SeqCst);
        page_before(large.conn(), "t", PageCursor::Head, PAGE_LIMIT).unwrap();
        let large_statements = STATEMENTS.load(AtomicOrdering::SeqCst);

        assert_eq!(
            small_statements, large_statements,
            "per-page statement count must not scale with rows (C7 零 N+1)"
        );
        assert!(
            large_statements <= 8,
            "one page stays a bounded constant statement count, got {large_statements}"
        );
    }

    #[test]
    fn page_and_tool_batch_observe_one_snapshot() {
        // A second connection writes between the page read and its tool batch;
        // the page API must never surface the mixed state, so the newest page
        // cannot gain tool rows that its own snapshot did not contain.
        let (store, dir) = seed_page_thread(2); // 4 rows
        let writer = Connection::open(dir.path().join("vega.db")).unwrap();
        writer
            .busy_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        let page = page_before(store.conn(), "t", PageCursor::Head, PAGE_LIMIT).unwrap();
        assert!(page.tool_calls.is_empty());
        writer
            .execute_batch(&tool_call("tc-after", "assistant-1", 9))
            .unwrap();
        let page_again = page_before(store.conn(), "t", PageCursor::Head, PAGE_LIMIT).unwrap();
        assert_eq!(page_again.tool_calls.len(), 1, "new snapshot sees the row");
        let ids: Vec<&str> = page_again
            .tool_calls
            .iter()
            .map(|call| call.id.as_str())
            .collect();
        assert_eq!(ids, vec!["tc-after"]);
    }
}
