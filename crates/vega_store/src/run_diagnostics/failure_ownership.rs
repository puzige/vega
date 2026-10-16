use super::*;
use rusqlite::OptionalExtension;

#[allow(missing_docs)]
pub fn budget_failure_message_ids(
    conn: &Connection,
    thread_id: &str,
) -> Result<Vec<(String, DiagnosticFailureCode)>, DiagnosticError> {
    validate_identity(thread_id, "thread_id")?;
    let mut statement = conn.prepare(
        "SELECT run_id, attempt_id, failure_code, id FROM run_diagnostic_events
         WHERE thread_id = ?1 AND phase = 'run' AND state = 'failed'
         AND failure_code IN ('context_over_limit', 'reasoning_limit', 'unknown_safe_failure')
         ORDER BY id",
    )?;
    let roots = statement
        .query_map([thread_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut failures = Vec::new();
    for (run_id, root_attempt_id, code, root_event_id) in roots {
        let code = match code.as_str() {
            "context_over_limit" => DiagnosticFailureCode::ContextOverLimit,
            "reasoning_limit" => DiagnosticFailureCode::ReasoningLimit,
            "unknown_safe_failure" => {
                let terminal = conn
                    .query_row(
                        "SELECT state, failure_code FROM run_diagnostic_events
                     WHERE thread_id = ?1 AND run_id = ?2 AND parent_attempt_id = ?3
                     AND phase = 'primary_model' AND id < ?4 ORDER BY id DESC LIMIT 1",
                        params![thread_id, run_id, root_attempt_id, root_event_id],
                        |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
                    )
                    .optional()?;
                if terminal != Some(("failed".into(), Some("reasoning_limit".into()))) {
                    continue;
                }
                DiagnosticFailureCode::ReasoningLimit
            }
            _ => continue,
        };
        if let Some(message_id) =
            failed_assistant_owner(conn, thread_id, &run_id, &root_attempt_id)?
        {
            failures.push((message_id, code));
        }
    }
    Ok(failures)
}

#[allow(missing_docs)]
pub fn projection_reasoning_failure(
    conn: &Connection,
    thread_id: &str,
    operation_key: &str,
    message_id: &str,
) -> Result<bool, DiagnosticError> {
    validate_identity(thread_id, "thread_id")?;
    let mut statement = conn.prepare(
        "SELECT diagnostic.run_id, diagnostic.parent_attempt_id
         FROM run_diagnostic_events diagnostic JOIN run_diagnostic_events root
         ON root.thread_id = diagnostic.thread_id AND root.run_id = diagnostic.run_id
         AND root.attempt_id = diagnostic.parent_attempt_id
         AND root.phase = 'run' AND root.state = 'failed' AND root.failure_code = 'reasoning_limit'
         WHERE diagnostic.thread_id = ?1 AND diagnostic.attempt_id = ?2
         AND diagnostic.phase = 'context_summary' AND diagnostic.state = 'failed'
         AND diagnostic.failure_code = 'reasoning_limit'
         AND diagnostic.id < root.id
         AND EXISTS(SELECT 1 FROM run_diagnostic_events started
            WHERE started.thread_id = diagnostic.thread_id AND started.run_id = diagnostic.run_id
            AND started.attempt_id = diagnostic.attempt_id AND started.state = 'started'
            AND started.phase = 'context_summary' AND started.parent_attempt_id = diagnostic.parent_attempt_id
            AND started.id < diagnostic.id)",
    )?;
    let candidates = statement
        .query_map(params![thread_id, operation_key], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if candidates.len() != 1 {
        return Ok(false);
    }
    let (run_id, root_attempt_id) = &candidates[0];
    Ok(
        failed_assistant_owner(conn, thread_id, run_id, root_attempt_id)?.as_deref()
            == Some(message_id),
    )
}

struct AssistantOwner {
    id: String,
    seq: i64,
    status: String,
}

fn failed_assistant_owner(
    conn: &Connection,
    thread_id: &str,
    run_id: &str,
    root_attempt_id: &str,
) -> Result<Option<String>, DiagnosticError> {
    let mut owner = conn.query_row(
        "SELECT id, seq, status FROM messages WHERE thread_id = ?1 AND id = ?2 AND role = 'assistant'",
        params![thread_id, run_id],
        |row| Ok(AssistantOwner { id: row.get(0)?, seq: row.get(1)?, status: row.get(2)? }),
    ).optional()?;
    let mut generation = None::<String>;
    while let Some(current) = owner {
        let conflicting_response: bool = conn.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM assistant_responses response JOIN run_diagnostic_events diagnostic
                ON diagnostic.thread_id = ?1 AND diagnostic.attempt_id = response.response_id
                AND diagnostic.phase = 'primary_model' AND diagnostic.state = 'succeeded'
                WHERE response.message_id = ?2
                AND (diagnostic.run_id <> ?3 OR diagnostic.parent_attempt_id <> ?4)
            )",
            params![thread_id, current.id, run_id, root_attempt_id],
            |row| row.get(0),
        )?;
        if conflicting_response {
            return Ok(None);
        }
        if current.status == "failed" {
            return Ok(Some(current.id));
        }
        if current.status != "done" {
            return Ok(None);
        }
        let Some(user_seq) = current.seq.checked_add(1) else {
            return Ok(None);
        };
        let delivery = conn
            .query_row(
                "SELECT pending.generation FROM messages user JOIN pending_messages pending
             ON pending.thread_id = user.thread_id AND pending.id = user.id
             WHERE user.thread_id = ?1 AND user.seq = ?2 AND user.role = 'user'
             AND user.status = 'done' AND pending.state = 'delivered'
             AND pending.behavior = 'steer' AND pending.generation <> ''",
                params![thread_id, user_seq],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let Some(delivery_generation) = delivery else {
            return Ok(None);
        };
        if generation
            .as_ref()
            .is_some_and(|generation| *generation != delivery_generation)
        {
            return Ok(None);
        }
        generation = Some(delivery_generation);
        let Some(assistant_seq) = user_seq.checked_add(1) else {
            return Ok(None);
        };
        owner = conn.query_row(
            "SELECT assistant.id, assistant.seq, assistant.status FROM messages assistant
             WHERE assistant.thread_id = ?1 AND assistant.seq = ?2 AND assistant.role = 'assistant'
             AND NOT EXISTS(SELECT 1 FROM run_diagnostic_events diagnostic
                 WHERE diagnostic.thread_id = assistant.thread_id AND diagnostic.run_id = assistant.id
                 AND diagnostic.phase = 'run')",
            params![thread_id, assistant_seq],
            |row| Ok(AssistantOwner { id: row.get(0)?, seq: row.get(1)?, status: row.get(2)? }),
        ).optional()?;
    }
    Ok(None)
}

pub fn classified_failure_message_ids(
    conn: &Connection,
    thread_id: &str,
) -> Result<Vec<(String, DiagnosticMetrics)>, DiagnosticError> {
    validate_identity(thread_id, "thread_id")?;
    let mut statement = conn.prepare("SELECT id, thread_id, run_id, attempt_id, parent_attempt_id, tool_call_id, phase, state, failure_code, occurred_at, duration_ms, stop_reason, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, visible_output_bytes, tool_output_bytes, tool_truncated, http_status, request_id, retry_count, failure_source, failure_kind, failure_scope, failure_observed, failure_limit, failure_units, failure_observed_items, failure_limit_items, failure_tool_index FROM run_diagnostic_events WHERE thread_id = ?1 AND phase = 'run' AND state = 'failed' AND failure_kind IS NOT NULL ORDER BY id")?;
    let rows = statement.query_map([thread_id], read_row)?;
    let mut failures = Vec::new();
    for row in rows {
        let event = DiagnosticEvent::try_from(row?)?.event;
        if let Some(owner) =
            failed_assistant_owner(conn, thread_id, &event.run_id, &event.attempt_id)?
        {
            failures.push((owner, event.metrics));
        }
    }
    Ok(failures)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Store, messages, pending_messages, threads};

    fn store() -> Store {
        let store = Store::open(":memory:").unwrap();
        store.migrate().unwrap();
        threads::create_standalone(
            store.conn(),
            threads::NewThread {
                id: "thread",
                project_id: "",
                title: "fixture",
                mode: "execute",
                permission_mode: "readonly",
                model: "mock",
                status: "active",
                pinned: false,
                unread: false,
                created_at: 1,
                updated_at: 1,
            },
        )
        .unwrap();
        store
    }

    fn start(
        conn: &Connection,
        run: &str,
        root: &str,
        attempt: &str,
        phase: DiagnosticPhase,
    ) -> NewDiagnosticEvent {
        let event = NewDiagnosticEvent {
            thread_id: "thread".into(),
            run_id: run.into(),
            attempt_id: attempt.into(),
            parent_attempt_id: (phase != DiagnosticPhase::Run).then(|| root.into()),
            tool_call_id: None,
            phase,
            state: DiagnosticState::Started,
            failure_code: None,
            occurred_at: 1,
            duration_ms: None,
            metrics: DiagnosticMetrics::default(),
        };
        insert(conn, &event).unwrap();
        event
    }

    fn finish(
        conn: &Connection,
        mut event: NewDiagnosticEvent,
        code: Option<DiagnosticFailureCode>,
    ) {
        event.state = if code.is_some() {
            DiagnosticState::Failed
        } else {
            DiagnosticState::Succeeded
        };
        event.failure_code = code;
        event.duration_ms = Some(1);
        event.occurred_at = 2;
        insert(conn, &event).unwrap();
    }

    fn message(conn: &Connection, id: &str, seq: i64, role: &str, status: &str, content: &str) {
        messages::insert(
            conn,
            &messages::MessageRow {
                id: id.into(),
                thread_id: "thread".into(),
                seq,
                role: role.into(),
                kind: "text".into(),
                content: content.into(),
                status: status.into(),
                created_at: 1,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .unwrap();
    }

    fn delivered(conn: &Connection, id: &str, generation: &str) {
        pending_messages::insert(
            conn,
            &pending_messages::PendingMessageRow {
                id: id.into(),
                thread_id: "thread".into(),
                ordinal: 0,
                behavior: "steer".into(),
                generation: generation.into(),
                payload: String::new(),
                created_at: 1,
            },
        )
        .unwrap();
        pending_messages::mark_delivered(conn, "thread", id).unwrap();
    }

    #[test]
    fn classified_failure_details_only_restore_the_failed_owner() {
        let store = store();
        message(store.conn(), "run", 1, "assistant", "failed", "");
        let mut root = start(store.conn(), "run", "root", "root", DiagnosticPhase::Run);
        root.metrics.failure_source = Some("provider".into());
        root.metrics.failure_kind = Some("output_limit".into());
        root.metrics.failure_scope = Some("response".into());
        finish(
            store.conn(),
            root,
            Some(DiagnosticFailureCode::ProviderRejected),
        );
        start(
            store.conn(),
            "run",
            "root",
            "unrelated-tool",
            DiagnosticPhase::Tool,
        );
        store.conn().execute_batch("PRAGMA ignore_check_constraints = ON; UPDATE run_diagnostic_events SET input_tokens = -1 WHERE attempt_id = 'unrelated-tool'; PRAGMA ignore_check_constraints = OFF;").unwrap();
        assert!(read_by_thread(store.conn(), "thread").is_err());
        let failures = classified_failure_message_ids(store.conn(), "thread").unwrap();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].0, "run");
        assert_eq!(failures[0].1.failure_kind.as_deref(), Some("output_limit"));
        store
            .conn()
            .execute("UPDATE messages SET status = 'done' WHERE id = 'run'", [])
            .unwrap();
        assert!(
            classified_failure_message_ids(store.conn(), "thread")
                .unwrap()
                .is_empty()
        );
    }
}
