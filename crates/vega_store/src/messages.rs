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

pub fn find(conn: &Connection, id: &str) -> Result<Option<MessageRow>, rusqlite::Error> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM messages WHERE id = ?1"),
        [id],
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
}
