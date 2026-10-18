use rusqlite::{Connection, params};
use serde::Serialize;

mod failure_ownership;
pub use failure_ownership::{
    budget_failure_message_ids, classified_failure_message_ids, compaction_failure_message_ids,
    projection_reasoning_failure,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticPhase {
    Run,
    PrimaryModel,
    ContextSummary,
    Tool,
}

impl DiagnosticPhase {
    fn as_str(self) -> &'static str {
        match self {
            Self::Run => "run",
            Self::PrimaryModel => "primary_model",
            Self::ContextSummary => "context_summary",
            Self::Tool => "tool",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticState {
    Started,
    Succeeded,
    Failed,
    Cancelled,
    Interrupted,
}

impl DiagnosticState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticFailureCode {
    Cancelled,
    Interrupted,
    ContextOverLimit,
    ReasoningLimit,
    SummaryTimeout,
    SummaryTruncated,
    SummaryEmpty,
    SummaryFormatInvalid,
    SummaryProjectionInvalid,
    SummarySourceChanged,
    SummarySourceTooLarge,
    SummaryAggregateTooLarge,
    SummaryImagesUnsupported,
    SummaryNoCompactablePrefix,
    SummaryAlreadyAttempted,
    ProviderHttp,
    ProviderTransportOrStream,
    ProviderProtocol,
    ProviderRejected,
    ToolFailed,
    ToolRejected,
    UnknownSafeFailure,
}

impl DiagnosticFailureCode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
            Self::ContextOverLimit => "context_over_limit",
            Self::ReasoningLimit => "reasoning_limit",
            Self::SummaryTimeout => "summary_timeout",
            Self::SummaryTruncated => "summary_truncated",
            Self::SummaryEmpty => "summary_empty",
            Self::SummaryFormatInvalid => "summary_format_invalid",
            Self::SummaryProjectionInvalid => "summary_projection_invalid",
            Self::SummarySourceChanged => "summary_source_changed",
            Self::SummarySourceTooLarge => "summary_source_too_large",
            Self::SummaryAggregateTooLarge => "summary_aggregate_too_large",
            Self::SummaryImagesUnsupported => "summary_images_unsupported",
            Self::SummaryNoCompactablePrefix => "summary_no_compactable_prefix",
            Self::SummaryAlreadyAttempted => "summary_already_attempted",
            Self::ProviderHttp => "provider_http",
            Self::ProviderTransportOrStream => "provider_transport_or_stream",
            Self::ProviderProtocol => "provider_protocol",
            Self::ProviderRejected => "provider_rejected",
            Self::ToolFailed => "tool_failed",
            Self::ToolRejected => "tool_rejected",
            Self::UnknownSafeFailure => "unknown_safe_failure",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticStopReason {
    End,
    ToolUse,
    Length,
}

impl DiagnosticStopReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::End => "end",
            Self::ToolUse => "tool_use",
            Self::Length => "length",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct DiagnosticMetrics {
    pub failure_tool_index: Option<u64>,
    pub failure_source: Option<String>,
    pub failure_kind: Option<String>,
    pub failure_scope: Option<String>,
    pub failure_observed: Option<u64>,
    pub failure_limit: Option<u64>,
    pub failure_units: Option<String>,
    pub failure_observed_items: Option<u64>,
    pub failure_limit_items: Option<u64>,

    pub stop_reason: Option<DiagnosticStopReason>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub visible_output_bytes: Option<u64>,
    pub tool_output_bytes: Option<u64>,
    pub tool_truncated: Option<bool>,
    pub http_status: Option<u16>,
    pub request_id: Option<String>,
    pub retry_count: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NewDiagnosticEvent {
    pub thread_id: String,
    pub run_id: String,
    pub attempt_id: String,
    pub parent_attempt_id: Option<String>,
    pub tool_call_id: Option<String>,
    pub phase: DiagnosticPhase,
    pub state: DiagnosticState,
    pub failure_code: Option<DiagnosticFailureCode>,
    pub occurred_at: i64,
    pub duration_ms: Option<u64>,
    pub metrics: DiagnosticMetrics,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DiagnosticEvent {
    pub id: i64,
    #[serde(flatten)]
    pub event: NewDiagnosticEvent,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DiagnosticExport {
    pub schema_version: u32,
    pub events: Vec<DiagnosticEvent>,
    pub incomplete_attempt_ids: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum DiagnosticError {
    #[error("invalid diagnostic event: {field}")]
    InvalidField { field: &'static str },
    #[error("diagnostic identifier was not found")]
    NotFound,
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub fn insert(conn: &Connection, event: &NewDiagnosticEvent) -> Result<i64, DiagnosticError> {
    validate(event)?;
    if event.state == DiagnosticState::Started {
        let duplicate: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM run_diagnostic_events WHERE thread_id=?1 AND run_id=?2 AND attempt_id=?3 AND state='started')",
            params![event.thread_id, event.run_id, event.attempt_id],
            |row| row.get(0),
        )?;
        if duplicate {
            return Err(DiagnosticError::InvalidField {
                field: "attempt_id",
            });
        }
    }
    if event.state != DiagnosticState::Started {
        let start_exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM run_diagnostic_events WHERE thread_id=?1 AND run_id=?2 AND attempt_id=?3 AND state='started')",
            params![event.thread_id, event.run_id, event.attempt_id],
            |row| row.get(0),
        )?;
        if !start_exists {
            return Err(DiagnosticError::InvalidField {
                field: "attempt_id",
            });
        }
        let terminal_exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM run_diagnostic_events WHERE thread_id=?1 AND run_id=?2 AND attempt_id=?3 AND state<>'started')",
            params![event.thread_id, event.run_id, event.attempt_id],
            |row| row.get(0),
        )?;
        if terminal_exists {
            return Err(DiagnosticError::InvalidField {
                field: "attempt_id",
            });
        }
    }
    conn.execute(
        "INSERT INTO run_diagnostic_events (
          thread_id, run_id, attempt_id, parent_attempt_id, tool_call_id, phase, state,
          failure_code, occurred_at, duration_ms, stop_reason, input_tokens, output_tokens,
          cache_read_tokens, cache_write_tokens, visible_output_bytes, tool_output_bytes,
          tool_truncated, http_status, request_id, retry_count, failure_source, failure_kind, failure_scope, failure_observed, failure_limit, failure_units, failure_observed_items, failure_limit_items, failure_tool_index
        ) VALUES (
          ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
          ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30
        )",
        params![
            event.thread_id,
            event.run_id,
            event.attempt_id,
            event.parent_attempt_id,
            event.tool_call_id,
            event.phase.as_str(),
            event.state.as_str(),
            event.failure_code.map(DiagnosticFailureCode::as_str),
            event.occurred_at,
            event.duration_ms.map(sql_i64).transpose().map_err(|_| {
                DiagnosticError::InvalidField {
                    field: "duration_ms",
                }
            })?,
            event.metrics.stop_reason.map(DiagnosticStopReason::as_str),
            event
                .metrics
                .input_tokens
                .map(sql_i64)
                .transpose()
                .map_err(|_| DiagnosticError::InvalidField {
                    field: "input_tokens"
                })?,
            event
                .metrics
                .output_tokens
                .map(sql_i64)
                .transpose()
                .map_err(|_| DiagnosticError::InvalidField {
                    field: "output_tokens"
                })?,
            event
                .metrics
                .cache_read_tokens
                .map(sql_i64)
                .transpose()
                .map_err(|_| DiagnosticError::InvalidField {
                    field: "cache_read_tokens"
                })?,
            event
                .metrics
                .cache_write_tokens
                .map(sql_i64)
                .transpose()
                .map_err(|_| DiagnosticError::InvalidField {
                    field: "cache_write_tokens"
                })?,
            event
                .metrics
                .visible_output_bytes
                .map(sql_i64)
                .transpose()
                .map_err(|_| DiagnosticError::InvalidField {
                    field: "visible_output_bytes"
                })?,
            event
                .metrics
                .tool_output_bytes
                .map(sql_i64)
                .transpose()
                .map_err(|_| DiagnosticError::InvalidField {
                    field: "tool_output_bytes"
                })?,
            event.metrics.tool_truncated.map(i64::from),
            event.metrics.http_status.map(i64::from),
            event.metrics.request_id,
            event.metrics.retry_count.map(i64::from),
            event.metrics.failure_source,
            event.metrics.failure_kind,
            event.metrics.failure_scope,
            event.metrics.failure_observed.map(sql_i64).transpose().map_err(|_| DiagnosticError::InvalidField { field: "failure_observed" })?,
            event.metrics.failure_limit.map(sql_i64).transpose().map_err(|_| DiagnosticError::InvalidField { field: "failure_limit" })?,
            event.metrics.failure_units,
            event.metrics.failure_observed_items.map(sql_i64).transpose().map_err(|_| DiagnosticError::InvalidField { field: "failure_observed_items" })?,
            event.metrics.failure_limit_items.map(sql_i64).transpose().map_err(|_| DiagnosticError::InvalidField { field: "failure_limit_items" })?,
            event.metrics.failure_tool_index.map(sql_i64).transpose().map_err(|_| DiagnosticError::InvalidField { field: "failure_tool_index" })?,

        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn read_by_run(
    conn: &Connection,
    thread_id: &str,
    run_id: &str,
) -> Result<Vec<DiagnosticEvent>, DiagnosticError> {
    validate_identity(thread_id, "thread_id")?;
    validate_identity(run_id, "run_id")?;
    let mut statement = conn.prepare("SELECT id, thread_id, run_id, attempt_id, parent_attempt_id, tool_call_id, phase, state, failure_code, occurred_at, duration_ms, stop_reason, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, visible_output_bytes, tool_output_bytes, tool_truncated, http_status, request_id, retry_count, failure_source, failure_kind, failure_scope, failure_observed, failure_limit, failure_units, failure_observed_items, failure_limit_items, failure_tool_index FROM run_diagnostic_events WHERE thread_id = ?1 AND run_id = ?2 ORDER BY id")?;
    let rows = statement.query_map(params![thread_id, run_id], read_row)?;
    rows.collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(TryInto::try_into)
        .collect()
}

pub fn read_by_thread(
    conn: &Connection,
    thread_id: &str,
) -> Result<Vec<DiagnosticEvent>, DiagnosticError> {
    validate_identity(thread_id, "thread_id")?;
    let mut statement = conn.prepare("SELECT id, thread_id, run_id, attempt_id, parent_attempt_id, tool_call_id, phase, state, failure_code, occurred_at, duration_ms, stop_reason, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, visible_output_bytes, tool_output_bytes, tool_truncated, http_status, request_id, retry_count, failure_source, failure_kind, failure_scope, failure_observed, failure_limit, failure_units, failure_observed_items, failure_limit_items, failure_tool_index FROM run_diagnostic_events WHERE thread_id = ?1 ORDER BY id")?;
    let rows = statement.query_map([thread_id], read_row)?;
    rows.collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(TryInto::try_into)
        .collect()
}

pub fn context_over_limit_run_ids(
    conn: &Connection,
    thread_id: &str,
) -> Result<Vec<String>, DiagnosticError> {
    validate_identity(thread_id, "thread_id")?;
    let mut statement = conn.prepare("SELECT DISTINCT run_id FROM run_diagnostic_events WHERE thread_id = ?1 AND phase = 'run' AND state = 'failed' AND failure_code = 'context_over_limit' ORDER BY run_id")?;
    Ok(statement
        .query_map([thread_id], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?)
}

pub fn export_run(
    conn: &Connection,
    thread_id: &str,
    run_id: &str,
) -> Result<String, DiagnosticError> {
    if thread_id.is_empty() || run_id.is_empty() {
        return Err(DiagnosticError::InvalidField { field: "identity" });
    }
    let events = read_by_run(conn, thread_id, run_id)?;
    if events.is_empty() {
        return Err(DiagnosticError::NotFound);
    }
    let incomplete_attempt_ids = incomplete_attempt_ids(&events);
    Ok(serde_json::to_string(&DiagnosticExport {
        schema_version: 1,
        events,
        incomplete_attempt_ids,
    })?)
}

pub fn incomplete_attempt_ids(events: &[DiagnosticEvent]) -> Vec<String> {
    let mut attempts = Vec::new();
    for event in events {
        if !attempts.iter().any(|(id, _)| id == &event.event.attempt_id) {
            attempts.push((event.event.attempt_id.clone(), false));
        }
        if event.event.state != DiagnosticState::Started
            && let Some((_, terminal)) = attempts
                .iter_mut()
                .find(|(id, _)| id == &event.event.attempt_id)
        {
            *terminal = true;
        }
    }
    attempts
        .into_iter()
        .filter_map(|(id, terminal)| (!terminal).then_some(id))
        .collect()
}

fn validate(event: &NewDiagnosticEvent) -> Result<(), DiagnosticError> {
    validate_identity(&event.thread_id, "thread_id")?;
    validate_identity(&event.run_id, "run_id")?;
    validate_identity(&event.attempt_id, "attempt_id")?;
    if event
        .parent_attempt_id
        .as_deref()
        .is_some_and(|id| validate_identity(id, "parent_attempt_id").is_err())
    {
        return Err(DiagnosticError::InvalidField {
            field: "parent_attempt_id",
        });
    }
    if event.tool_call_id.as_deref().is_some_and(|id| {
        !is_ascii_id(
            id,
            128,
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_-",
        )
    }) {
        return Err(DiagnosticError::InvalidField {
            field: "tool_call_id",
        });
    }
    if event.occurred_at < 0 {
        return Err(DiagnosticError::InvalidField {
            field: "occurred_at",
        });
    }
    if (event.state == DiagnosticState::Started) != event.duration_ms.is_none() {
        return Err(DiagnosticError::InvalidField {
            field: "duration_ms",
        });
    }
    let valid_failure_shape = matches!(
        (event.state, event.failure_code),
        (DiagnosticState::Started | DiagnosticState::Succeeded, None)
            | (DiagnosticState::Failed, Some(_))
            | (
                DiagnosticState::Cancelled,
                Some(DiagnosticFailureCode::Cancelled)
            )
            | (
                DiagnosticState::Interrupted,
                Some(DiagnosticFailureCode::Interrupted)
            )
    );
    if !valid_failure_shape {
        return Err(DiagnosticError::InvalidField {
            field: "failure_code",
        });
    }
    if event
        .metrics
        .failure_source
        .as_deref()
        .is_some_and(|value| !matches!(value, "runtime" | "provider"))
    {
        return Err(DiagnosticError::InvalidField {
            field: "failure_source",
        });
    }
    if event.metrics.failure_kind.as_deref().is_some_and(|value| {
        !matches!(
            value,
            "unknown"
                | "http"
                | "transport"
                | "protocol"
                | "rejected"
                | "output_limit"
                | "reasoning_limit"
                | "context_over_limit"
                | "cancelled"
                | "quota"
                | "authorization"
                | "rate_limited"
                | "credential_exposure"
                | "credential_check"
                | "credential_continuation"
                | "credential_projection"
        )
    }) {
        return Err(DiagnosticError::InvalidField {
            field: "failure_kind",
        });
    }
    if event.metrics.failure_scope.as_deref().is_some_and(|value| {
        !matches!(
            value,
            "delta"
                | "response"
                | "run"
                | "request_input"
                | "request_context"
                | "summary_input"
                | "compaction_result"
                | "sse_frame"
        )
    }) {
        return Err(DiagnosticError::InvalidField {
            field: "failure_scope",
        });
    }
    if event
        .metrics
        .failure_units
        .as_deref()
        .is_some_and(|value| !matches!(value, "bytes" | "tokens"))
    {
        return Err(DiagnosticError::InvalidField {
            field: "failure_units",
        });
    }
    if event
        .metrics
        .http_status
        .is_some_and(|status| !(100..=599).contains(&status))
    {
        return Err(DiagnosticError::InvalidField {
            field: "http_status",
        });
    }
    if event.metrics.request_id.as_deref().is_some_and(|id| {
        !is_ascii_id(
            id,
            128,
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789._:-",
        )
    }) {
        return Err(DiagnosticError::InvalidField {
            field: "request_id",
        });
    }
    Ok(())
}

fn validate_identity(value: &str, field: &'static str) -> Result<(), DiagnosticError> {
    if is_ascii_id(
        value,
        128,
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789._:-",
    ) {
        Ok(())
    } else {
        Err(DiagnosticError::InvalidField { field })
    }
}

fn is_ascii_id(value: &str, max: usize, allowed: &[u8]) -> bool {
    !value.is_empty() && value.len() <= max && value.bytes().all(|byte| allowed.contains(&byte))
}

fn sql_i64(value: u64) -> Result<i64, std::num::TryFromIntError> {
    i64::try_from(value)
}

struct RawEvent {
    failure_tool_index: Option<i64>,
    failure_source: Option<String>,
    failure_kind: Option<String>,
    failure_scope: Option<String>,
    failure_observed: Option<i64>,
    failure_limit: Option<i64>,
    failure_units: Option<String>,
    failure_observed_items: Option<i64>,
    failure_limit_items: Option<i64>,

    id: i64,
    thread_id: String,
    run_id: String,
    attempt_id: String,
    parent_attempt_id: Option<String>,
    tool_call_id: Option<String>,
    phase: String,
    state: String,
    failure_code: Option<String>,
    occurred_at: i64,
    duration_ms: Option<i64>,
    stop_reason: Option<String>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    cache_read_tokens: Option<i64>,
    cache_write_tokens: Option<i64>,
    visible_output_bytes: Option<i64>,
    tool_output_bytes: Option<i64>,
    tool_truncated: Option<bool>,
    http_status: Option<u16>,
    request_id: Option<String>,
    retry_count: Option<u32>,
}

fn read_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawEvent> {
    Ok(RawEvent {
        id: row.get(0)?,
        thread_id: row.get(1)?,
        run_id: row.get(2)?,
        attempt_id: row.get(3)?,
        parent_attempt_id: row.get(4)?,
        tool_call_id: row.get(5)?,
        phase: row.get(6)?,
        state: row.get(7)?,
        failure_code: row.get(8)?,
        occurred_at: row.get(9)?,
        duration_ms: row.get(10)?,
        stop_reason: row.get(11)?,
        input_tokens: row.get(12)?,
        output_tokens: row.get(13)?,
        cache_read_tokens: row.get(14)?,
        cache_write_tokens: row.get(15)?,
        visible_output_bytes: row.get(16)?,
        tool_output_bytes: row.get(17)?,
        tool_truncated: row.get(18)?,
        http_status: row.get(19)?,
        request_id: row.get(20)?,
        failure_tool_index: row.get(30)?,
        retry_count: row.get(21)?,
        failure_source: row.get(22)?,
        failure_kind: row.get(23)?,
        failure_scope: row.get(24)?,
        failure_observed: row.get(25)?,
        failure_limit: row.get(26)?,
        failure_units: row.get(27)?,
        failure_observed_items: row.get(28)?,
        failure_limit_items: row.get(29)?,
    })
}

impl TryFrom<RawEvent> for DiagnosticEvent {
    type Error = DiagnosticError;

    fn try_from(raw: RawEvent) -> Result<Self, Self::Error> {
        Ok(Self {
            id: raw.id,
            event: NewDiagnosticEvent {
                thread_id: raw.thread_id,
                run_id: raw.run_id,
                attempt_id: raw.attempt_id,
                parent_attempt_id: raw.parent_attempt_id,
                tool_call_id: raw.tool_call_id,
                phase: parse_phase(&raw.phase)?,
                state: parse_state(&raw.state)?,
                failure_code: raw.failure_code.as_deref().map(parse_failure).transpose()?,
                occurred_at: raw.occurred_at,
                duration_ms: raw
                    .duration_ms
                    .map(|value| {
                        u64::try_from(value).map_err(|_| DiagnosticError::InvalidField {
                            field: "duration_ms",
                        })
                    })
                    .transpose()?,
                metrics: DiagnosticMetrics {
                    stop_reason: raw
                        .stop_reason
                        .as_deref()
                        .map(parse_stop_reason)
                        .transpose()?,
                    input_tokens: opt_u64(raw.input_tokens, "input_tokens")?,
                    output_tokens: opt_u64(raw.output_tokens, "output_tokens")?,
                    cache_read_tokens: opt_u64(raw.cache_read_tokens, "cache_read_tokens")?,
                    cache_write_tokens: opt_u64(raw.cache_write_tokens, "cache_write_tokens")?,
                    visible_output_bytes: opt_u64(
                        raw.visible_output_bytes,
                        "visible_output_bytes",
                    )?,
                    tool_output_bytes: opt_u64(raw.tool_output_bytes, "tool_output_bytes")?,
                    tool_truncated: raw.tool_truncated,
                    http_status: raw.http_status,
                    request_id: raw.request_id,
                    failure_tool_index: opt_u64(raw.failure_tool_index, "failure_tool_index")?,
                    retry_count: raw.retry_count,
                    failure_source: raw.failure_source,
                    failure_kind: raw.failure_kind,
                    failure_scope: raw.failure_scope,
                    failure_observed: opt_u64(raw.failure_observed, "failure_observed")?,
                    failure_limit: opt_u64(raw.failure_limit, "failure_limit")?,
                    failure_units: raw.failure_units,
                    failure_observed_items: opt_u64(
                        raw.failure_observed_items,
                        "failure_observed_items",
                    )?,
                    failure_limit_items: opt_u64(raw.failure_limit_items, "failure_limit_items")?,
                },
            },
        })
    }
}

fn opt_u64(value: Option<i64>, field: &'static str) -> Result<Option<u64>, DiagnosticError> {
    value
        .map(|value| u64::try_from(value).map_err(|_| DiagnosticError::InvalidField { field }))
        .transpose()
}

fn parse_phase(value: &str) -> Result<DiagnosticPhase, DiagnosticError> {
    match value {
        "run" => Ok(DiagnosticPhase::Run),
        "primary_model" => Ok(DiagnosticPhase::PrimaryModel),
        "context_summary" => Ok(DiagnosticPhase::ContextSummary),
        "tool" => Ok(DiagnosticPhase::Tool),
        _ => Err(DiagnosticError::InvalidField { field: "phase" }),
    }
}

fn parse_state(value: &str) -> Result<DiagnosticState, DiagnosticError> {
    match value {
        "started" => Ok(DiagnosticState::Started),
        "succeeded" => Ok(DiagnosticState::Succeeded),
        "failed" => Ok(DiagnosticState::Failed),
        "cancelled" => Ok(DiagnosticState::Cancelled),
        "interrupted" => Ok(DiagnosticState::Interrupted),
        _ => Err(DiagnosticError::InvalidField { field: "state" }),
    }
}

fn parse_stop_reason(value: &str) -> Result<DiagnosticStopReason, DiagnosticError> {
    match value {
        "end" => Ok(DiagnosticStopReason::End),
        "tool_use" => Ok(DiagnosticStopReason::ToolUse),
        "length" => Ok(DiagnosticStopReason::Length),
        _ => Err(DiagnosticError::InvalidField {
            field: "stop_reason",
        }),
    }
}

fn parse_failure(value: &str) -> Result<DiagnosticFailureCode, DiagnosticError> {
    match value {
        "cancelled" => Ok(DiagnosticFailureCode::Cancelled),
        "interrupted" => Ok(DiagnosticFailureCode::Interrupted),
        "context_over_limit" => Ok(DiagnosticFailureCode::ContextOverLimit),
        "reasoning_limit" => Ok(DiagnosticFailureCode::ReasoningLimit),
        "summary_timeout" => Ok(DiagnosticFailureCode::SummaryTimeout),
        "summary_truncated" => Ok(DiagnosticFailureCode::SummaryTruncated),
        "summary_empty" => Ok(DiagnosticFailureCode::SummaryEmpty),
        "summary_format_invalid" => Ok(DiagnosticFailureCode::SummaryFormatInvalid),
        "summary_projection_invalid" => Ok(DiagnosticFailureCode::SummaryProjectionInvalid),
        "summary_source_changed" => Ok(DiagnosticFailureCode::SummarySourceChanged),
        "summary_source_too_large" => Ok(DiagnosticFailureCode::SummarySourceTooLarge),
        "summary_aggregate_too_large" => Ok(DiagnosticFailureCode::SummaryAggregateTooLarge),
        "summary_images_unsupported" => Ok(DiagnosticFailureCode::SummaryImagesUnsupported),
        "summary_no_compactable_prefix" => Ok(DiagnosticFailureCode::SummaryNoCompactablePrefix),
        "summary_already_attempted" => Ok(DiagnosticFailureCode::SummaryAlreadyAttempted),
        "provider_http" => Ok(DiagnosticFailureCode::ProviderHttp),
        "provider_transport_or_stream" => Ok(DiagnosticFailureCode::ProviderTransportOrStream),
        "provider_protocol" => Ok(DiagnosticFailureCode::ProviderProtocol),
        "provider_rejected" => Ok(DiagnosticFailureCode::ProviderRejected),
        "tool_failed" => Ok(DiagnosticFailureCode::ToolFailed),
        "tool_rejected" => Ok(DiagnosticFailureCode::ToolRejected),
        "unknown_safe_failure" => Ok(DiagnosticFailureCode::UnknownSafeFailure),
        _ => Err(DiagnosticError::InvalidField {
            field: "failure_code",
        }),
    }
}
