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

    fn legacy_steer(conn: &Connection) {
        message(conn, "goal", 1, "user", "done", "goal");
        message(conn, "original", 2, "assistant", "done", "prior");
        message(conn, "steer", 3, "user", "done", "correction");
        message(conn, "final", 4, "assistant", "failed", "");
        delivered(conn, "steer", "generation");
        let root = start(conn, "original", "root", "root", DiagnosticPhase::Run);
        let primary = start(
            conn,
            "original",
            "root",
            "primary-success",
            DiagnosticPhase::PrimaryModel,
        );
        finish(conn, primary, None);
        messages::record_assistant_response(
            conn,
            "original",
            "prior",
            &messages::AssistantResponseRow {
                message_id: "original".into(),
                response_id: "primary-success".into(),
                text_start_bytes: 0,
                text_end_bytes: 5,
                kind: "final".into(),
            },
        )
        .unwrap();
        let primary = start(
            conn,
            "original",
            "root",
            "primary-limit",
            DiagnosticPhase::PrimaryModel,
        );
        finish(conn, primary, Some(DiagnosticFailureCode::ReasoningLimit));
        finish(conn, root, Some(DiagnosticFailureCode::UnknownSafeFailure));
    }

    #[test]
    fn issue372_legacy_delivery_chain_uses_exact_durable_primary_and_pending_ownership() {
        let store = store();
        legacy_steer(store.conn());
        assert_eq!(
            budget_failure_message_ids(store.conn(), "thread").unwrap(),
            [("final".into(), DiagnosticFailureCode::ReasoningLimit)]
        );
        message(store.conn(), "other-user", 5, "user", "done", "independent");
        message(
            store.conn(),
            "other-assistant",
            6,
            "assistant",
            "failed",
            "",
        );
        let root = start(
            store.conn(),
            "other-assistant",
            "other-root",
            "other-root",
            DiagnosticPhase::Run,
        );
        let primary = start(
            store.conn(),
            "other-assistant",
            "other-root",
            "other-primary",
            DiagnosticPhase::PrimaryModel,
        );
        finish(
            store.conn(),
            primary,
            Some(DiagnosticFailureCode::ProviderHttp),
        );
        finish(
            store.conn(),
            root,
            Some(DiagnosticFailureCode::UnknownSafeFailure),
        );
        assert_eq!(
            budget_failure_message_ids(store.conn(), "thread").unwrap(),
            [("final".into(), DiagnosticFailureCode::ReasoningLimit)]
        );
    }

    #[test]
    fn issue372_delivery_ownership_rejects_missing_queued_gapped_and_independent_runs() {
        for mutation in [
            "DELETE FROM pending_messages WHERE id = 'steer'",
            "UPDATE pending_messages SET behavior = 'queue' WHERE id = 'steer'",
            "UPDATE pending_messages SET state = 'pending' WHERE id = 'steer'",
            "UPDATE pending_messages SET generation = '' WHERE id = 'steer'",
            "DELETE FROM messages WHERE id = 'steer'",
            "UPDATE messages SET seq = 5 WHERE id = 'final'",
            "UPDATE messages SET role = 'user' WHERE id = 'final'",
            "UPDATE messages SET status = 'done' WHERE id = 'final'",
        ] {
            let store = store();
            legacy_steer(store.conn());
            store.conn().execute(mutation, []).unwrap();
            assert!(
                budget_failure_message_ids(store.conn(), "thread")
                    .unwrap()
                    .is_empty(),
                "{mutation}"
            );
        }
        let store = store();
        legacy_steer(store.conn());
        start(
            store.conn(),
            "final",
            "independent-root",
            "independent-root",
            DiagnosticPhase::Run,
        );
        assert!(
            budget_failure_message_ids(store.conn(), "thread")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn issue372_multiple_deliveries_require_one_generation_and_preserve_correct_failed_tail() {
        for same_generation in [false, true] {
            let store = store();
            legacy_steer(store.conn());
            store
                .conn()
                .execute("UPDATE messages SET status = 'done' WHERE id = 'final'", [])
                .unwrap();
            message(
                store.conn(),
                "second-steer",
                5,
                "user",
                "done",
                "second correction",
            );
            message(store.conn(), "second-final", 6, "assistant", "failed", "");
            delivered(
                store.conn(),
                "second-steer",
                if same_generation {
                    "generation"
                } else {
                    "different-generation"
                },
            );
            let failures = budget_failure_message_ids(store.conn(), "thread").unwrap();
            if same_generation {
                assert_eq!(
                    failures,
                    [("second-final".into(), DiagnosticFailureCode::ReasoningLimit)]
                );
            } else {
                assert!(failures.is_empty());
            }
        }
    }

    #[test]
    fn issue372_unknown_roots_cannot_borrow_old_successful_or_other_parent_attempts() {
        for mutation in [
            "UPDATE run_diagnostic_events SET state = 'succeeded', failure_code = NULL WHERE attempt_id = 'primary-limit' AND state = 'failed'",
            "UPDATE run_diagnostic_events SET parent_attempt_id = 'another-root' WHERE attempt_id = 'primary-limit'",
            "UPDATE run_diagnostic_events SET failure_code = 'unknown_safe_failure' WHERE attempt_id = 'primary-limit' AND state = 'failed'",
        ] {
            let store = store();
            legacy_steer(store.conn());
            store.conn().execute(mutation, []).unwrap();
            assert!(
                budget_failure_message_ids(store.conn(), "thread")
                    .unwrap()
                    .is_empty()
            );
        }
        let store = store();
        legacy_steer(store.conn());
        let subsequent = start(
            store.conn(),
            "original",
            "root",
            "subsequent-primary",
            DiagnosticPhase::PrimaryModel,
        );
        finish(store.conn(), subsequent, None);
        store
            .conn()
            .execute(
                "DELETE FROM run_diagnostic_events WHERE attempt_id = 'root' AND state = 'failed'",
                [],
            )
            .unwrap();
        let root = NewDiagnosticEvent {
            thread_id: "thread".into(),
            run_id: "original".into(),
            attempt_id: "root".into(),
            parent_attempt_id: None,
            tool_call_id: None,
            phase: DiagnosticPhase::Run,
            state: DiagnosticState::Failed,
            failure_code: Some(DiagnosticFailureCode::UnknownSafeFailure),
            occurred_at: 3,
            duration_ms: Some(2),
            metrics: DiagnosticMetrics::default(),
        };
        insert(store.conn(), &root).unwrap();
        assert!(
            budget_failure_message_ids(store.conn(), "thread")
                .unwrap()
                .is_empty()
        );
        assert!(
            !projection_reasoning_failure(store.conn(), "thread", "unknown-operation", "final")
                .unwrap()
        );
    }

    #[test]
    fn issue372_projection_cause_requires_exact_operation_key_and_failed_owner() {
        let store = store();
        message(store.conn(), "owner", 1, "assistant", "failed", "");
        let root = start(store.conn(), "owner", "root", "root", DiagnosticPhase::Run);
        let operation = "owner:context-generation-1:attempt-one";
        let validation = start(
            store.conn(),
            "owner",
            "root",
            operation,
            DiagnosticPhase::ContextSummary,
        );
        finish(
            store.conn(),
            validation,
            Some(DiagnosticFailureCode::ReasoningLimit),
        );
        finish(
            store.conn(),
            root,
            Some(DiagnosticFailureCode::ReasoningLimit),
        );
        assert!(projection_reasoning_failure(store.conn(), "thread", operation, "owner").unwrap());
        assert!(
            !projection_reasoning_failure(
                store.conn(),
                "thread",
                "owner:context-generation-2:attempt-two",
                "owner"
            )
            .unwrap()
        );
        assert!(
            !projection_reasoning_failure(store.conn(), "thread", operation, "other-owner")
                .unwrap()
        );
        assert!(
            !projection_reasoning_failure(store.conn(), "other-thread", operation, "owner")
                .unwrap()
        );
        store.conn().execute("UPDATE run_diagnostic_events SET parent_attempt_id = 'wrong-root' WHERE attempt_id = ?1", [operation]).unwrap();
        assert!(!projection_reasoning_failure(store.conn(), "thread", operation, "owner").unwrap());
    }
}
