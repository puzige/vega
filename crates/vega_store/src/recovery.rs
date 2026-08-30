//! Startup recovery for incomplete rows left by a killed agent process.

use rusqlite::{Connection, params};

/// Canonical strict audit written for an incomplete approval at startup.
pub const RECOVERY_DENIAL_APPROVAL_JSON: &str =
    r#"{"decision":"deny","note":null,"source":"recovery","danger":null}"#;

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
        "UPDATE tool_calls SET status = 'rejected', approval = ?1, \
         output_text = 'Tool error: rejected during startup recovery because approval was incomplete.', \
         finished_at = ?2 \
         WHERE thread_id = ?3 AND status = 'pending_approval'",
        params![RECOVERY_DENIAL_APPROVAL_JSON, now_ms, thread_id],
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

#[cfg(test)]
mod tests {
    use super::{RECOVERY_DENIAL_APPROVAL_JSON, RecoveryCounts, recover_thread};
    use crate::Store;

    fn store() -> Store {
        let store = Store::open(":memory:").unwrap();
        store.migrate().unwrap();
        store
    }

    fn seed_thread(store: &Store, thread_id: &str) {
        let project_id = format!("project-{thread_id}");
        store
            .conn()
            .execute(
                "INSERT INTO projects (id, path, name, created_at, last_opened_at) \
                 VALUES (?1, ?2, ?1, 0, 0)",
                [project_id.as_str(), format!("/tmp/{project_id}").as_str()],
            )
            .unwrap();
        store
            .conn()
            .execute(
                "INSERT INTO threads (id, project_id, title, model, created_at, updated_at) \
                 VALUES (?1, ?2, '', '', 0, 0)",
                [thread_id, project_id.as_str()],
            )
            .unwrap();
        store
            .conn()
            .execute(
                "INSERT INTO messages (id, thread_id, seq, role, content, status, created_at) \
                 VALUES (?1, ?2, 1, 'assistant', '', 'streaming', 0)",
                [format!("message-{thread_id}").as_str(), thread_id],
            )
            .unwrap();
    }

    fn seed_call(store: &Store, id: &str, thread_id: &str, status: &str, approval: Option<&str>) {
        store
            .conn()
            .execute(
                "INSERT INTO tool_calls \
                 (id, thread_id, message_id, seq, tool, input_json, status, approval, created_at) \
                 VALUES (?1, ?2, ?3, 1, 'bash', '{}', ?4, ?5, 0)",
                rusqlite::params![
                    id,
                    thread_id,
                    format!("message-{thread_id}"),
                    status,
                    approval
                ],
            )
            .unwrap();
    }

    fn call_state(
        store: &Store,
        id: &str,
    ) -> (String, Option<String>, Option<String>, Option<i64>) {
        store
            .conn()
            .query_row(
                "SELECT status, approval, output_text, finished_at FROM tool_calls WHERE id = ?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap()
    }

    #[test]
    fn pending_rows_use_exact_strict_recovery_audit() {
        let store = store();
        seed_thread(&store, "thread-a");
        seed_call(&store, "pending-null", "thread-a", "pending_approval", None);
        seed_call(
            &store,
            "pending-corrupt",
            "thread-a",
            "pending_approval",
            Some("corrupt"),
        );

        assert_eq!(
            recover_thread(store.conn(), "thread-a", 42).unwrap(),
            RecoveryCounts {
                messages_interrupted: 1,
                tools_rejected: 2,
                tools_cancelled: 0,
            }
        );
        for id in ["pending-null", "pending-corrupt"] {
            let (status, approval, output, finished_at) = call_state(&store, id);
            assert_eq!(status, "rejected");
            assert_eq!(approval.as_deref(), Some(RECOVERY_DENIAL_APPROVAL_JSON));
            assert_eq!(
                output.as_deref(),
                Some(
                    "Tool error: rejected during startup recovery because approval was incomplete."
                )
            );
            assert_eq!(finished_at, Some(42));
        }
    }

    #[test]
    fn running_rows_cancel_without_fabricating_recovery_approval() {
        let store = store();
        seed_thread(&store, "thread-a");
        seed_call(&store, "approved", "thread-a", "approved", Some("once"));
        seed_call(&store, "running", "thread-a", "running", Some("always"));

        let counts = recover_thread(store.conn(), "thread-a", 77).unwrap();
        assert_eq!(counts.tools_cancelled, 2);
        for (id, original) in [("approved", "once"), ("running", "always")] {
            let (status, approval, output, finished_at) = call_state(&store, id);
            assert_eq!(status, "cancelled");
            assert_eq!(approval.as_deref(), Some(original));
            assert_eq!(
                output.as_deref(),
                Some("Tool cancelled during startup recovery because execution was incomplete.")
            );
            assert_eq!(finished_at, Some(77));
        }
    }

    #[test]
    fn terminal_rows_and_other_threads_are_unchanged() {
        let store = store();
        seed_thread(&store, "thread-a");
        seed_thread(&store, "thread-b");
        seed_call(&store, "terminal", "thread-a", "success", Some("once"));
        seed_call(&store, "other", "thread-b", "pending_approval", None);

        recover_thread(store.conn(), "thread-a", 5).unwrap();
        assert_eq!(
            call_state(&store, "terminal"),
            ("success".into(), Some("once".into()), None, None)
        );
        assert_eq!(
            call_state(&store, "other"),
            ("pending_approval".into(), None, None, None)
        );
        let other_message_status: String = store
            .conn()
            .query_row(
                "SELECT status FROM messages WHERE id = 'message-thread-b'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(other_message_status, "streaming");
    }

    #[test]
    fn message_and_tool_recovery_are_atomic() {
        let store = store();
        seed_thread(&store, "thread-a");
        seed_call(&store, "pending", "thread-a", "pending_approval", None);
        store
            .conn()
            .execute_batch(
                "CREATE TRIGGER reject_recovery BEFORE UPDATE ON tool_calls \
                 BEGIN SELECT RAISE(ABORT, 'blocked'); END;",
            )
            .unwrap();

        assert!(recover_thread(store.conn(), "thread-a", 9).is_err());
        let message_status: String = store
            .conn()
            .query_row(
                "SELECT status FROM messages WHERE id = 'message-thread-a'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(message_status, "streaming");
        assert_eq!(
            call_state(&store, "pending"),
            ("pending_approval".into(), None, None, None)
        );
    }
}
