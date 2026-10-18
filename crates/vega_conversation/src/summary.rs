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
