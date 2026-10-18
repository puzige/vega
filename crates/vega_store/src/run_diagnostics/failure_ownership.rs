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
pub fn compaction_failure_message_ids(
    conn: &Connection,
    thread_id: &str,
) -> Result<Vec<(String, DiagnosticFailureCode)>, DiagnosticError> {
    validate_identity(thread_id, "thread_id")?;
    let mut statement = conn.prepare(
        "SELECT run_id, attempt_id, failure_code, id, failure_source, failure_kind, failure_scope,
                failure_observed, failure_limit, failure_units, failure_observed_items,
                failure_limit_items, failure_tool_index, http_status, request_id, retry_count
         FROM run_diagnostic_events
         WHERE thread_id = ?1 AND parent_attempt_id IS NULL
         AND phase = 'run' AND state = 'failed'
         AND failure_code IN (
            'summary_timeout', 'summary_truncated', 'summary_empty', 'summary_format_invalid',
            'summary_projection_invalid', 'summary_source_changed', 'summary_source_too_large',
            'summary_aggregate_too_large', 'summary_images_unsupported',
            'summary_no_compactable_prefix', 'summary_already_attempted', 'unknown_safe_failure'
         )
         ORDER BY id",
    )?;
    let roots = statement
        .query_map([thread_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<i64>>(7)?,
                row.get::<_, Option<i64>>(8)?,
                row.get::<_, Option<String>>(9)?,
                row.get::<_, Option<i64>>(10)?,
                row.get::<_, Option<i64>>(11)?,
                row.get::<_, Option<i64>>(12)?,
                row.get::<_, Option<u16>>(13)?,
                row.get::<_, Option<String>>(14)?,
                row.get::<_, Option<u32>>(15)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut failures = Vec::new();
    for (
        run_id,
        root_attempt_id,
        raw_code,
        root_event_id,
        failure_source,
        failure_kind,
        failure_scope,
        failure_observed,
        failure_limit,
        failure_units,
        failure_observed_items,
        failure_limit_items,
        failure_tool_index,
        http_status,
        request_id,
        retry_count,
    ) in roots
    {
        let direct_code = summary_diagnostic_code(&raw_code);
        let metrics = DiagnosticMetrics {
            failure_source,
            failure_kind,
            failure_scope,
            failure_observed: opt_u64(failure_observed, "failure_observed")?,
            failure_limit: opt_u64(failure_limit, "failure_limit")?,
            failure_units,
            failure_observed_items: opt_u64(failure_observed_items, "failure_observed_items")?,
            failure_limit_items: opt_u64(failure_limit_items, "failure_limit_items")?,
            failure_tool_index: opt_u64(failure_tool_index, "failure_tool_index")?,
            http_status,
            request_id,
            retry_count,
            ..DiagnosticMetrics::default()
        };
        let code = match direct_code {
            Some(code) => Some(code),
            None if raw_code == "unknown_safe_failure" && generic_failure_metrics(&metrics) => {
                let Some(message_id) =
                    failed_assistant_owner(conn, thread_id, &run_id, &root_attempt_id)?
                else {
                    continue;
                };
                legacy_compaction_failure_code(
                    conn,
                    thread_id,
                    &run_id,
                    &root_attempt_id,
                    root_event_id,
                    &message_id,
                )?
            }
            _ => None,
        };
        let Some(code) = code else {
            continue;
        };
        if let Some(message_id) =
            failed_assistant_owner(conn, thread_id, &run_id, &root_attempt_id)?
        {
            failures.push((message_id, code));
        }
    }
    Ok(failures)
}

fn generic_failure_metrics(metrics: &DiagnosticMetrics) -> bool {
    let no_details = metrics.failure_scope.is_none()
        && metrics.failure_observed.is_none()
        && metrics.failure_limit.is_none()
        && metrics.failure_units.is_none()
        && metrics.failure_observed_items.is_none()
        && metrics.failure_limit_items.is_none()
        && metrics.failure_tool_index.is_none()
        && metrics.http_status.is_none()
        && metrics.request_id.is_none()
        && metrics.retry_count.is_none();
    no_details
        && ((metrics.failure_source.is_none() && metrics.failure_kind.is_none())
            || (metrics.failure_source.as_deref() == Some("runtime")
                && metrics.failure_kind.as_deref() == Some("unknown")))
}

fn legacy_compaction_failure_code(
    conn: &Connection,
    thread_id: &str,
    run_id: &str,
    root_attempt_id: &str,
    root_event_id: i64,
    message_id: &str,
) -> Result<Option<DiagnosticFailureCode>, DiagnosticError> {
    let terminal = conn
        .query_row(
            "SELECT id, attempt_id, phase, state, failure_code
             FROM run_diagnostic_events
             WHERE thread_id = ?1 AND run_id = ?2 AND parent_attempt_id = ?3
             AND phase IN ('primary_model', 'tool', 'context_summary')
             AND state <> 'started' AND id < ?4
             ORDER BY id DESC LIMIT 1",
            params![thread_id, run_id, root_attempt_id, root_event_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            },
        )
        .optional()?;
    let Some((terminal_id, operation_key, phase, state, raw_code)) = terminal else {
        return Ok(None);
    };
    if phase != "context_summary" || state != "failed" {
        return Ok(None);
    }
    let Some(code) = raw_code.as_deref().and_then(summary_diagnostic_code) else {
        return Ok(None);
    };
    let Some(generation) = operation_generation(message_id, &operation_key) else {
        return Ok(None);
    };
    let root_started = {
        let mut statement = conn.prepare(
            "SELECT id FROM run_diagnostic_events
             WHERE thread_id = ?1 AND run_id = ?2 AND attempt_id = ?3
             AND parent_attempt_id IS NULL AND phase = 'run' AND state = 'started'",
        )?;
        statement
            .query_map(params![thread_id, run_id, root_attempt_id], |row| {
                row.get::<_, i64>(0)
            })?
            .collect::<Result<Vec<_>, _>>()?
    };
    let summary_started = {
        let mut statement = conn.prepare(
            "SELECT id FROM run_diagnostic_events
             WHERE thread_id = ?1 AND run_id = ?2 AND attempt_id = ?3
             AND parent_attempt_id = ?4 AND phase = 'context_summary' AND state = 'started'",
        )?;
        statement
            .query_map(
                params![thread_id, run_id, operation_key, root_attempt_id],
                |row| row.get::<_, i64>(0),
            )?
            .collect::<Result<Vec<_>, _>>()?
    };
    let summary_terminal_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM run_diagnostic_events
         WHERE thread_id = ?1 AND run_id = ?2 AND attempt_id = ?3
         AND parent_attempt_id = ?4 AND phase = 'context_summary' AND state <> 'started'",
        params![thread_id, run_id, operation_key, root_attempt_id],
        |row| row.get(0),
    )?;
    if root_started.len() != 1
        || summary_started.len() != 1
        || summary_terminal_count != 1
        || root_started[0] >= summary_started[0]
        || summary_started[0] >= terminal_id
        || terminal_id >= root_event_id
    {
        return Ok(None);
    }
    let incomplete_child: bool = conn.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM run_diagnostic_events started
            WHERE started.thread_id = ?1 AND started.run_id = ?2
              AND started.parent_attempt_id = ?3
              AND started.phase IN ('primary_model', 'tool', 'context_summary')
              AND started.state = 'started' AND started.id < ?4
              AND NOT EXISTS(
                  SELECT 1 FROM run_diagnostic_events terminal
                  WHERE terminal.thread_id = started.thread_id
                    AND terminal.run_id = started.run_id
                    AND terminal.attempt_id = started.attempt_id
                    AND terminal.parent_attempt_id = started.parent_attempt_id
                    AND terminal.phase = started.phase
                    AND terminal.state <> 'started' AND terminal.id < ?4
              )
        )",
        params![thread_id, run_id, root_attempt_id, root_event_id],
        |row| row.get(0),
    )?;
    if incomplete_child {
        return Ok(None);
    }
    let status_rows = {
        let mut statement = conn.prepare(
            "SELECT model, generation, phase,
                    CASE WHEN failure_detail = 'summary_timeout' THEN failure_detail ELSE failure END
             FROM context_compaction_status
             WHERE thread_id = ?1 AND operation_key = ?2
               AND phase IN ('failed', 'succeeded', 'cancelled')
             ORDER BY id",
        )?;
        statement
            .query_map(params![thread_id, operation_key], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?
    };
    if status_rows.len() != 1
        || status_rows[0].1 != generation
        || status_rows[0].2 != "failed"
        || status_rows[0].3.as_deref() != Some(compaction_status_failure(code))
    {
        return Ok(None);
    }
    let prefix = format!("{message_id}:context-generation-");
    let newer_successes = {
        let mut statement = conn.prepare(
            "SELECT operation_key, generation FROM context_compaction_status
             WHERE thread_id = ?1 AND model = ?2 AND phase = 'succeeded' AND generation > ?3
               AND substr(operation_key, 1, length(?4)) = ?4",
        )?;
        statement
            .query_map(
                params![thread_id, status_rows[0].0, generation, prefix],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )?
            .collect::<Result<Vec<_>, _>>()?
    };
    let newer_success = newer_successes
        .into_iter()
        .any(|(operation_key, stored_generation)| {
            operation_generation(message_id, &operation_key) == Some(stored_generation)
                && stored_generation > generation
        });
    if newer_success {
        return Ok(None);
    }
    Ok(Some(code))
}

fn operation_generation(message_id: &str, operation_key: &str) -> Option<i64> {
    let prefix = format!("{message_id}:context-generation-");
    let suffix = operation_key.strip_prefix(&prefix)?;
    let (generation, attempt) = suffix.split_once(":attempt-")?;
    if attempt.is_empty() {
        return None;
    }
    generation
        .parse::<i64>()
        .ok()
        .filter(|generation| *generation > 0)
}

fn summary_diagnostic_code(value: &str) -> Option<DiagnosticFailureCode> {
    Some(match value {
        "summary_timeout" => DiagnosticFailureCode::SummaryTimeout,
        "summary_truncated" => DiagnosticFailureCode::SummaryTruncated,
        "summary_empty" => DiagnosticFailureCode::SummaryEmpty,
        "summary_format_invalid" => DiagnosticFailureCode::SummaryFormatInvalid,
        "summary_projection_invalid" => DiagnosticFailureCode::SummaryProjectionInvalid,
        "summary_source_changed" => DiagnosticFailureCode::SummarySourceChanged,
        "summary_source_too_large" => DiagnosticFailureCode::SummarySourceTooLarge,
        "summary_aggregate_too_large" => DiagnosticFailureCode::SummaryAggregateTooLarge,
        "summary_images_unsupported" => DiagnosticFailureCode::SummaryImagesUnsupported,
        "summary_no_compactable_prefix" => DiagnosticFailureCode::SummaryNoCompactablePrefix,
        "summary_already_attempted" => DiagnosticFailureCode::SummaryAlreadyAttempted,
        _ => return None,
    })
}

fn compaction_status_failure(code: DiagnosticFailureCode) -> &'static str {
    match code {
        DiagnosticFailureCode::SummarySourceChanged => "source_changed",
        DiagnosticFailureCode::SummaryNoCompactablePrefix => "no_compactable_prefix",
        DiagnosticFailureCode::SummarySourceTooLarge
        | DiagnosticFailureCode::SummaryAggregateTooLarge => "too_large",
        DiagnosticFailureCode::SummaryImagesUnsupported => "images_unsupported",
        DiagnosticFailureCode::SummaryAlreadyAttempted => "unavailable",
        DiagnosticFailureCode::SummaryTimeout => "summary_timeout",
        DiagnosticFailureCode::SummaryTruncated
        | DiagnosticFailureCode::SummaryEmpty
        | DiagnosticFailureCode::SummaryFormatInvalid
        | DiagnosticFailureCode::SummaryProjectionInvalid => "invalid_summary",
        _ => "",
    }
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
