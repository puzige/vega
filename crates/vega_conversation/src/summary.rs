//! Per-task cost summary projection (S7-T40/A10-06, C4 contract).
//!
//! The projection aggregates only the finished assistant task's own
//! provider-call rows (`token_usage` by `message_id`) and its tool-call audit
//! rows (`tool_calls` by `message_id`), both via checked `vega_store`
//! queries. Unavailable facts stay typed (no usage rows → `None` tokens,
//! legacy/unknown pricing version → `SummaryCost::Unavailable`) and must be
//! rendered as `—`, never fabricated to zero. The wall-clock duration is
//! supplied by the live run's memory only; restart recovery passes `None`
//! because `messages` has no finished timestamp (C4).

use vega_store::{Store, messages, token_usage, tool_calls};

use crate::types::{
    ConversationError, Microcents, SummaryCost, TaskCostSummary, TaskSummaryOutcome, TokenUsage,
};
/// Projects the typed cost summary of one assistant task.
///
/// The message must be a durable terminal assistant row of `thread_id`
/// (`done | interrupted | failed`); a still-streaming or unknown status fails
/// closed so a running task can never be presented as finished. Callers pass
/// `duration_ms = Some` only while the live run's wall-clock measurement is
/// in memory; restart recovery passes `None` (renders as `—`).
pub fn task_cost_summary(
    store: &Store,
    thread_id: &str,
    message_id: &str,
    duration_ms: Option<u64>,
) -> Result<TaskCostSummary, ConversationError> {
    let row = messages::find(store.conn(), message_id)
        .map_err(|error| ConversationError::Store(error.to_string()))?
        .ok_or_else(|| ConversationError::NotFound(message_id.to_string()))?;
    if row.thread_id != thread_id || row.role != "assistant" {
        return Err(ConversationError::CorruptRow(
            "summary message id does not own the thread/role".to_string(),
        ));
    }
    let outcome = match row.status.as_str() {
        "done" => TaskSummaryOutcome::Completed,
        "interrupted" => TaskSummaryOutcome::Interrupted,
        "failed" => TaskSummaryOutcome::Failed,
        other => {
            return Err(ConversationError::CorruptRow(format!(
                "summary message is not terminal: {other}"
            )));
        }
    };

    let aggregate = token_usage::aggregate_by_message(store.conn(), thread_id, message_id)
        .map_err(|error| ConversationError::Store(error.to_string()))?;
    let tool_count = tool_calls::count_by_message(store.conn(), thread_id, message_id)
        .map_err(|error| ConversationError::Store(error.to_string()))?;

    let usage = (aggregate.row_count > 0).then_some(TokenUsage {
        input: aggregate.input_tokens,
        output: aggregate.output_tokens,
        cache_read: aggregate.cache_read_tokens,
        cache_write: aggregate.cache_write_tokens,
    });
    let cost = match aggregate.cost {
        token_usage::AggregateCost::Priced(total) if aggregate.row_count > 0 => {
            SummaryCost::Priced(Microcents(total))
        }
        token_usage::AggregateCost::Priced(_) | token_usage::AggregateCost::Unavailable => {
            SummaryCost::Unavailable
        }
    };
    let cache_hit_percent = usage.and_then(TaskCostSummary::cache_hit_percent);

    Ok(TaskCostSummary {
        message_id: message_id.to_string(),
        outcome,
        usage,
        cost,
        duration_ms,
        tool_count,
        cache_hit_percent,
    })
}

/// Projects the summary of the thread's latest terminal assistant task, or
/// `None` when the thread has none (S7-T40 restart recovery scope).
///
/// Durable `token`/`cost`/`cache`/`tool-count` fields recover from the audit
/// rows; the caller must pass `duration_ms = None` on restart because the
/// wall-clock measurement only exists in the live run's memory (C4).
pub fn latest_task_summary(
    store: &Store,
    thread_id: &str,
    duration_ms: Option<u64>,
) -> Result<Option<TaskCostSummary>, ConversationError> {
    let Some(row) = messages::last_terminal_assistant(store.conn(), thread_id)
        .map_err(|error| ConversationError::Store(error.to_string()))?
    else {
        return Ok(None);
    };
    task_cost_summary(store, thread_id, &row.id, duration_ms).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TaskSummaryOutcome;
    use vega_store::projects;
    use vega_store::threads;

    fn store_with_thread() -> (Store, tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("vega.db")).unwrap();
        store.migrate().unwrap();
        let project = projects::create(store.conn(), "/tmp/summary-spec", "summary", None)
            .expect("project fixture");
        threads::create(
            store.conn(),
            threads::NewThread {
                id: "summary-thread",
                project_id: &project.id,
                title: "summary",
                mode: "execute",
                permission_mode: "auto",
                model: "priced-model",
                status: "active",
                pinned: false,
                unread: false,
                created_at: 1,
                updated_at: 1,
            },
        )
        .expect("thread fixture");
        (store, dir, project.id)
    }

    fn assistant_message(store: &Store, status: &str) -> String {
        let message_id = ulid::Ulid::generate().to_string();
        messages::insert(
            store.conn(),
            &messages::MessageRow {
                id: message_id.clone(),
                thread_id: "summary-thread".to_string(),
                seq: messages::next_seq(store.conn(), "summary-thread").unwrap(),
                role: "assistant".to_string(),
                kind: String::new(),
                content: String::new(),
                status: status.to_string(),
                created_at: 1,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .expect("message fixture");
        message_id
    }

    fn priced_row(store: &Store, message_id: &str, input: u64, output: u64, cache: u64, cost: i64) {
        token_usage::insert(
            store.conn(),
            token_usage::NewTokenUsage {
                thread_id: "summary-thread",
                message_id: Some(message_id),
                model: "priced-model",
                input_tokens: input,
                output_tokens: output,
                cache_read_tokens: cache,
                cache_write_tokens: 0,
                cost_microcents: cost,
                created_at: 1,
                pricing_version: Some(token_usage::PRICED_VERSION),
                pricing_profile: Some("base"),
                call_started_at: Some(1),
            },
        )
        .unwrap();
    }

    #[test]
    fn completed_summary_projects_exact_fields() {
        let (store, _dir, _project) = store_with_thread();
        let message_id = assistant_message(&store, "done");
        priced_row(&store, &message_id, 100, 20, 50, 120_000);
        priced_row(&store, &message_id, 30, 5, 0, 30_000);
        tool_calls::insert(
            store.conn(),
            tool_calls::NewToolCall {
                id: "call-1",
                thread_id: "summary-thread",
                message_id: &message_id,
                seq: 1,
                tool: "read",
                input_json: "{}",
                status: "success",
                created_at: 1,
            },
        )
        .unwrap();

        let summary =
            task_cost_summary(&store, "summary-thread", &message_id, Some(1_500)).unwrap();
        assert_eq!(summary.message_id, message_id);
        assert_eq!(summary.outcome, TaskSummaryOutcome::Completed);
        assert_eq!(
            summary.usage,
            Some(TokenUsage {
                input: 130,
                output: 25,
                cache_read: 50,
                cache_write: 0,
            })
        );
        assert_eq!(summary.cost, SummaryCost::Priced(Microcents(150_000)));
        assert_eq!(summary.duration_ms, Some(1_500));
        assert_eq!(summary.tool_count, 1);
        // 50/130 = 38.46% → half-up 38%.
        assert_eq!(summary.cache_hit_percent, Some(38));
    }

    #[test]
    fn zero_input_usage_keeps_defined_zero_cache_hit() {
        let (store, _dir, _project) = store_with_thread();
        let message_id = assistant_message(&store, "done");
        priced_row(&store, &message_id, 0, 10, 0, 20_000);
        let summary = task_cost_summary(&store, "summary-thread", &message_id, None).unwrap();
        assert_eq!(summary.cache_hit_percent, Some(0));
        assert_eq!(
            summary.duration_ms, None,
            "restart recovery has no duration"
        );
    }

    #[test]
    fn missing_usage_stays_typed_unavailable_not_zero() {
        let (store, _dir, _project) = store_with_thread();
        let message_id = assistant_message(&store, "interrupted");
        let summary = task_cost_summary(&store, "summary-thread", &message_id, None).unwrap();
        assert_eq!(summary.outcome, TaskSummaryOutcome::Interrupted);
        assert_eq!(summary.usage, None);
        assert_eq!(summary.cost, SummaryCost::Unavailable);
        assert_eq!(summary.cache_hit_percent, None);
        assert_eq!(summary.tool_count, 0);
    }

    #[test]
    fn unpriced_rows_keep_tokens_but_cost_unavailable() {
        let (store, _dir, _project) = store_with_thread();
        let message_id = assistant_message(&store, "failed");
        token_usage::insert(
            store.conn(),
            token_usage::NewTokenUsage {
                thread_id: "summary-thread",
                message_id: Some(&message_id),
                model: "priced-model",
                input_tokens: 7,
                output_tokens: 3,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                cost_microcents: 0,
                created_at: 1,
                pricing_version: None,
                pricing_profile: None,
                call_started_at: None,
            },
        )
        .unwrap();
        let summary = task_cost_summary(&store, "summary-thread", &message_id, None).unwrap();
        assert_eq!(summary.outcome, TaskSummaryOutcome::Failed);
        assert_eq!(
            summary.usage,
            Some(TokenUsage {
                input: 7,
                output: 3,
                cache_read: 0,
                cache_write: 0,
            })
        );
        assert_eq!(summary.cost, SummaryCost::Unavailable);
    }

    #[test]
    fn non_terminal_or_foreign_messages_fail_closed() {
        let (store, _dir, _project) = store_with_thread();
        let streaming = assistant_message(&store, "streaming");
        assert!(matches!(
            task_cost_summary(&store, "summary-thread", &streaming, None),
            Err(ConversationError::CorruptRow(_))
        ));

        let finished = assistant_message(&store, "done");
        assert!(matches!(
            task_cost_summary(&store, "other-thread", &finished, None),
            Err(ConversationError::CorruptRow(_))
        ));
        assert!(matches!(
            task_cost_summary(&store, "summary-thread", "missing", None),
            Err(ConversationError::NotFound(_))
        ));
    }

    #[test]
    fn usage_audit_survives_thread_deletion_by_message_scope() {
        let (store, _dir, _project) = store_with_thread();
        let message_id = assistant_message(&store, "done");
        priced_row(&store, &message_id, 100, 20, 50, 120_000);
        threads::delete_thread(store.conn(), "summary-thread").unwrap();
        // token_usage has no thread foreign key: the audit rows survive the
        // deletion contract (messages/tool_calls rows do not, by design).
        let aggregate =
            token_usage::aggregate_by_message(store.conn(), "summary-thread", &message_id).unwrap();
        assert_eq!(aggregate.input_tokens, 100);
        assert_eq!(aggregate.cost, token_usage::AggregateCost::Priced(120_000));
        assert!(matches!(
            task_cost_summary(&store, "summary-thread", &message_id, None),
            Err(ConversationError::NotFound(_))
        ));
    }
}
