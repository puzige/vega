//! Per-task cost summary projection (S7-T40/A10-06, C4 contract).
//!
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
    let cache_usage = if aggregate.row_count == 0 {
        (None, None, false)
    } else {
        exact_primary_cache_usage(store, thread_id, message_id, aggregate)?
    };

    let combined = token_usage::aggregate_task_by_message(store.conn(), thread_id, message_id)
        .map_err(|error| ConversationError::Store(error.to_string()))?;
    let delegated_complete =
        vega_store::subagents::run_usage_complete(store.conn(), thread_id, message_id)
            .map_err(|error| ConversationError::Store(error.to_string()))?;
    let includes_children = combined.row_count > aggregate.row_count;
    let cache_usage = if !delegated_complete {
        (None, None, false)
    } else if includes_children {
        (
            cache_usage.0.map(|_| combined.cache_read_tokens),
            cache_usage.1.map(|_| combined.cache_write_tokens),
            cache_usage.2,
        )
    } else {
        cache_usage
    };
    let aggregate = combined;
    let usage = (aggregate.row_count > 0).then_some(TokenUsage {
        input: aggregate.input_tokens,
        output: aggregate.output_tokens,
        cache_read: cache_usage.0,
        cache_write: cache_usage.1,
    });
    let cost = match (cache_usage.2, aggregate.cost) {
        (true, token_usage::AggregateCost::Priced(total)) if aggregate.row_count > 0 => {
            SummaryCost::Priced(Microcents(total))
        }
        (_, token_usage::AggregateCost::Priced(_))
        | (_, token_usage::AggregateCost::Unavailable) => SummaryCost::Unavailable,
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

fn exact_primary_cache_usage(
    store: &Store,
    thread_id: &str,
    message_id: &str,
    aggregate: token_usage::UsageAggregate,
) -> Result<(Option<u64>, Option<u64>, bool), ConversationError> {
    let token_rows = token_usage_rows(store, thread_id, message_id)?;
    let events = vega_store::run_diagnostics::read_by_run(store.conn(), thread_id, message_id)
        .map_err(|error| ConversationError::Store(error.to_string()))?;
    let mut primary_rows = Vec::new();
    let mut attempt_ids = std::collections::HashSet::new();
    let mut cache_read_tokens = 0_u64;
    let mut cache_write_tokens = 0_u64;
    let mut cache_read_known = true;
    let mut cache_write_known = true;
    for event in events.into_iter().filter(|event| {
        event.event.phase == vega_store::run_diagnostics::DiagnosticPhase::PrimaryModel
            && event.event.state != vega_store::run_diagnostics::DiagnosticState::Started
            && event.event.parent_attempt_id.is_some()
    }) {
        let metrics = event.event.metrics;
        let (input, output) = match (metrics.input_tokens, metrics.output_tokens) {
            (Some(input), Some(output)) => (input, output),
            (None, None) => continue,
            _ => return Ok((None, None, false)),
        };
        if !attempt_ids.insert(event.event.attempt_id.clone()) {
            return Ok((None, None, false));
        }
        primary_rows.push((event.event.attempt_id, input, output));
        match metrics.cache_read_tokens {
            Some(cache_read) if cache_read <= input => {
                cache_read_tokens = cache_read_tokens.checked_add(cache_read).ok_or_else(|| {
                    ConversationError::CorruptRow("cache read overflow".to_string())
                })?;
            }
            Some(_) => return Ok((None, None, false)),
            None => cache_read_known = false,
        }
        if let Some(cache_write) = metrics.cache_write_tokens {
            cache_write_tokens = cache_write_tokens
                .checked_add(cache_write)
                .ok_or_else(|| ConversationError::CorruptRow("cache write overflow".to_string()))?;
        } else {
            cache_write_known = false;
        }
    }
    let primary_row_count = u64::try_from(primary_rows.len())
        .map_err(|_| ConversationError::CorruptRow("usage count overflow".to_string()))?;
    let token_row_count = u64::try_from(token_rows.len())
        .map_err(|_| ConversationError::CorruptRow("usage count overflow".to_string()))?;
    if primary_row_count != aggregate.row_count
        || token_row_count != aggregate.row_count
        || primary_rows.iter().zip(token_rows.iter()).any(
            |((_, diagnostic_input, diagnostic_output), token_row)| {
                *diagnostic_input != token_row.0 || *diagnostic_output != token_row.1
            },
        )
    {
        return Ok((None, None, false));
    }
    if (cache_read_known && cache_read_tokens != aggregate.cache_read_tokens)
        || (cache_write_known && cache_write_tokens != aggregate.cache_write_tokens)
    {
        return Ok((None, None, false));
    }
    let cache_read = cache_read_known.then_some(cache_read_tokens);
    let cache_write = cache_write_known.then_some(cache_write_tokens);
    Ok((cache_read, cache_write, true))
}

fn token_usage_rows(
    store: &Store,
    thread_id: &str,
    message_id: &str,
) -> Result<Vec<(u64, u64)>, ConversationError> {
    let mut statement = store
        .conn()
        .prepare(
            "SELECT input_tokens, output_tokens FROM token_usage \
             WHERE thread_id = ?1 AND message_id IS ?2 ORDER BY id",
        )
        .map_err(|error| ConversationError::Store(error.to_string()))?;
    let rows = statement
        .query_map([thread_id, message_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|error| ConversationError::Store(error.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| ConversationError::Store(error.to_string()))?;
    rows.into_iter()
        .map(|(input, output)| {
            let input = u64::try_from(input)
                .map_err(|_| ConversationError::CorruptRow("negative input usage".to_string()))?;
            let output = u64::try_from(output)
                .map_err(|_| ConversationError::CorruptRow("negative output usage".to_string()))?;
            Ok((input, output))
        })
        .collect()
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
    use vega_store::run_diagnostics::{
        DiagnosticFailureCode, DiagnosticMetrics, DiagnosticPhase, DiagnosticState,
        NewDiagnosticEvent,
    };
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
        diagnostic_usage_row(store, message_id, input, output, Some(cache), Some(0));
    }

    fn diagnostic_usage_row(
        store: &Store,
        message_id: &str,
        input: u64,
        output: u64,
        cache_read: Option<u64>,
        cache_write: Option<u64>,
    ) {
        diagnostic_usage_row_with_state(
            store,
            message_id,
            input,
            output,
            cache_read,
            cache_write,
            DiagnosticState::Succeeded,
        );
    }

    fn diagnostic_usage_row_with_state(
        store: &Store,
        message_id: &str,
        input: u64,
        output: u64,
        cache_read: Option<u64>,
        cache_write: Option<u64>,
        terminal_state: DiagnosticState,
    ) {
        let attempt_id = ulid::Ulid::generate().to_string();
        for (state, duration_ms, metrics) in [
            (DiagnosticState::Started, None, DiagnosticMetrics::default()),
            (
                terminal_state,
                Some(1),
                DiagnosticMetrics {
                    input_tokens: Some(input),
                    output_tokens: Some(output),
                    cache_read_tokens: cache_read,
                    cache_write_tokens: cache_write,
                    ..DiagnosticMetrics::default()
                },
            ),
        ] {
            vega_store::run_diagnostics::insert(
                store.conn(),
                &NewDiagnosticEvent {
                    thread_id: "summary-thread".to_string(),
                    run_id: message_id.to_string(),
                    attempt_id: attempt_id.clone(),
                    parent_attempt_id: Some("root_attempt".to_string()),
                    tool_call_id: None,
                    phase: DiagnosticPhase::PrimaryModel,
                    state,
                    failure_code: (state == DiagnosticState::Failed)
                        .then_some(DiagnosticFailureCode::ProviderProtocol),
                    occurred_at: 1,
                    duration_ms,
                    metrics,
                },
            )
            .unwrap();
        }
    }

    #[test]
    fn billable_failed_attempt_keeps_priced_cost_with_missing_cache_metrics() {
        let (store, _dir, _project) = store_with_thread();
        let message_id = assistant_message(&store, "failed");
        token_usage::insert(
            store.conn(),
            token_usage::NewTokenUsage {
                thread_id: "summary-thread",
                message_id: Some(&message_id),
                model: "priced-model",
                input_tokens: 100,
                output_tokens: 21,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                cost_microcents: 70,
                created_at: 1,
                pricing_version: Some(token_usage::PRICED_VERSION),
                pricing_profile: Some("base"),
                call_started_at: Some(1),
            },
        )
        .unwrap();
        diagnostic_usage_row_with_state(
            &store,
            &message_id,
            100,
            21,
            None,
            None,
            DiagnosticState::Failed,
        );
        let summary = task_cost_summary(&store, "summary-thread", &message_id, None).unwrap();
        assert_eq!(
            summary.usage,
            Some(TokenUsage {
                input: 100,
                output: 21,
                cache_read: None,
                cache_write: None,
            })
        );
        assert_eq!(summary.cost, SummaryCost::Priced(Microcents(70)));
    }

    #[test]
    fn cache_metrics_require_per_attempt_usage_alignment() {
        let (store, _dir, _project) = store_with_thread();
        let message_id = assistant_message(&store, "done");
        for (input, output, cache_read, cost) in [(100, 10, 20, 70), (200, 20, 40, 80)] {
            token_usage::insert(
                store.conn(),
                token_usage::NewTokenUsage {
                    thread_id: "summary-thread",
                    message_id: Some(&message_id),
                    model: "priced-model",
                    input_tokens: input,
                    output_tokens: output,
                    cache_read_tokens: cache_read,
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
        diagnostic_usage_row(&store, &message_id, 150, 15, Some(30), Some(0));
        diagnostic_usage_row(&store, &message_id, 150, 15, Some(30), Some(0));

        let summary = task_cost_summary(&store, "summary-thread", &message_id, None).unwrap();
        assert_eq!(
            summary.usage,
            Some(TokenUsage {
                input: 300,
                output: 30,
                cache_read: None,
                cache_write: None,
            })
        );
        assert_eq!(summary.cost, SummaryCost::Unavailable);
    }

    #[test]
    fn cache_metric_conflict_invalidates_priced_summary() {
        let (store, _dir, _project) = store_with_thread();
        let message_id = assistant_message(&store, "done");
        token_usage::insert(
            store.conn(),
            token_usage::NewTokenUsage {
                thread_id: "summary-thread",
                message_id: Some(&message_id),
                model: "priced-model",
                input_tokens: 100,
                output_tokens: 21,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                cost_microcents: 70,
                created_at: 1,
                pricing_version: Some(token_usage::PRICED_VERSION),
                pricing_profile: Some("base"),
                call_started_at: Some(1),
            },
        )
        .unwrap();
        diagnostic_usage_row(&store, &message_id, 100, 21, Some(5), Some(0));

        let summary = task_cost_summary(&store, "summary-thread", &message_id, None).unwrap();
        assert_eq!(summary.usage.map(|usage| usage.cache_read), Some(None));
        assert_eq!(summary.cost, SummaryCost::Unavailable);
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
                cache_read: Some(50),
                cache_write: Some(0),
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
                cache_read: None,
                cache_write: None,
            })
        );
        assert_eq!(summary.cost, SummaryCost::Unavailable);
    }

    #[test]
    fn missing_cache_metrics_preserve_known_usage_and_mark_cache_and_cost_unavailable() {
        let (store, _dir, _project) = store_with_thread();
        let message_id = assistant_message(&store, "done");
        token_usage::insert(
            store.conn(),
            token_usage::NewTokenUsage {
                thread_id: "summary-thread",
                message_id: Some(&message_id),
                model: "priced-model",
                input_tokens: 100,
                output_tokens: 21,
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
        diagnostic_usage_row(&store, &message_id, 100, 21, None, None);
        let summary = task_cost_summary(&store, "summary-thread", &message_id, None).unwrap();
        assert_eq!(
            summary.usage,
            Some(TokenUsage {
                input: 100,
                output: 21,
                cache_read: None,
                cache_write: None,
            })
        );
        assert_eq!(summary.cache_hit_percent, None);
        assert_eq!(summary.cost, SummaryCost::Unavailable);
    }

    #[test]
    fn unknown_cache_write_preserves_independently_reported_cache_read() {
        let (store, _dir, _project) = store_with_thread();
        let message_id = assistant_message(&store, "done");
        priced_row(&store, &message_id, 100, 20, 50, 120_000);
        token_usage::insert(
            store.conn(),
            token_usage::NewTokenUsage {
                thread_id: "summary-thread",
                message_id: Some(&message_id),
                model: "priced-model",
                input_tokens: 30,
                output_tokens: 5,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                cost_microcents: 0,
                created_at: 2,
                pricing_version: None,
                pricing_profile: None,
                call_started_at: None,
            },
        )
        .unwrap();
        diagnostic_usage_row(&store, &message_id, 30, 5, Some(0), None);
        let summary = task_cost_summary(&store, "summary-thread", &message_id, None).unwrap();
        assert_eq!(
            summary.usage,
            Some(TokenUsage {
                input: 130,
                output: 25,
                cache_read: Some(50),
                cache_write: None,
            })
        );
        assert_eq!(summary.cost, SummaryCost::Unavailable);
        assert_eq!(summary.cache_hit_percent, Some(38));
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
