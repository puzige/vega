//! Durable context settings and append-only compaction checkpoints.
//!
//! This module is deliberately a SQLite-only repository.  It exposes the
//! chronological message/tool/image projection needed by the conversation
//! layer, while leaving provider messages, summaries, and UI state outside of
//! `vega_store`.

use std::convert::TryFrom;
use std::fmt;

use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use sha2::{Digest, Sha256};

use crate::image_attachments::ImageRow;
use crate::messages::MessageRow;

/// The largest setting accepted by the store.  This mirrors the runtime's
/// bounded `u32` provider conversion without making the store depend on the
/// runtime crate.
pub const MAX_CONTEXT_VALUE: u64 = u32::MAX as u64;

/// Product default for an unedited model: an editable input/output assumption,
/// not verified provider metadata. UI must label this distinction explicitly.
pub const DEFAULT_MODEL_INPUT_LIMIT: u64 = 300_000;
/// Product default response reserve for an unedited model.
pub const DEFAULT_MODEL_OUTPUT_RESERVE: u64 = 128_000;

const MAX_SOURCE_ROWS: i64 = 100_000;
const MAX_SOURCE_TEXT_BYTES: i64 = 64 * 1024 * 1024;
const MAX_SOURCE_IMAGE_BYTES: i64 = 32 * 1024 * 1024;

/// Context capacity and automatic-compaction policy for one configured
/// provider/model pair. A missing row uses the editable assumed default;
/// a saved row with two absent limits stays sendable without a budget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelContextPolicy {
    /// Exact configured provider identity.
    pub provider: String,
    /// Exact model identifier under that provider.
    pub model: String,
    /// Maximum estimated provider input budget, or `None` when unknown.
    pub input_limit: Option<u64>,
    /// Maximum response capability reserved from total context, not a
    /// per-request generation cap. Both numeric fields are configured together.
    pub output_reserve: Option<u64>,
    /// Whether automatic compaction is enabled once capacity is known.
    pub automatic_compaction: bool,
    /// Unix-millisecond update time supplied by the caller.
    pub updated_at: i64,
}

impl ModelContextPolicy {
    /// Editable assumption for an unedited provider/model. A missing database
    /// row keeps its provenance; opening Settings must not auto-save this.
    pub fn assumed_default(provider: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            model: model.into(),
            input_limit: Some(DEFAULT_MODEL_INPUT_LIMIT),
            output_reserve: Some(DEFAULT_MODEL_OUTPUT_RESERVE),
            automatic_compaction: true,
            updated_at: 0,
        }
    }

    /// Explicit unknown-capacity projection. Unlike an absent row, saving
    /// this intentionally opts out of budget-based compaction until edited.
    pub fn unconfigured(provider: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            model: model.into(),
            input_limit: None,
            output_reserve: None,
            automatic_compaction: true,
            updated_at: 0,
        }
    }

    fn validate(&self) -> Result<(), ContextSettingsError> {
        if self.provider.trim().is_empty() || self.model.trim().is_empty() {
            return Err(ContextSettingsError::Invalid);
        }
        match (self.input_limit, self.output_reserve) {
            (None, None) => Ok(()),
            (Some(input), Some(reserve))
                if input > 0
                    && input <= MAX_CONTEXT_VALUE
                    && reserve > 0
                    && input
                        .checked_add(reserve)
                        .is_some_and(|total| total <= MAX_CONTEXT_VALUE) =>
            {
                Ok(())
            }
            _ => Err(ContextSettingsError::Invalid),
        }
    }
}

/// Per-conversation/model context settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextSettings {
    /// Conversation identity.
    pub thread_id: String,
    /// Exact provider/model identity within the conversation.
    pub model: String,
    /// Configured total capacity; `None` means unknown/unconfigured.
    pub context_limit: Option<u64>,
    /// Positive output reserve.  It remains persisted when the total is
    /// unknown so a later limit edit has a complete draft.
    pub output_reserve: u64,
    /// Whether automatic compaction is enabled for this exact identity.
    pub automatic_compaction: bool,
    /// Unix-millisecond update time supplied by the conversation layer.
    pub updated_at: i64,
}

impl ContextSettings {
    fn validate(&self) -> Result<(), ContextSettingsError> {
        if self.thread_id.trim().is_empty() || self.model.trim().is_empty() {
            return Err(ContextSettingsError::Invalid);
        }
        if self
            .context_limit
            .is_some_and(|value| value == 0 || value > MAX_CONTEXT_VALUE)
            || self.output_reserve == 0
            || self.output_reserve > MAX_CONTEXT_VALUE
            || self
                .context_limit
                .is_some_and(|limit| self.output_reserve >= limit)
        {
            return Err(ContextSettingsError::Invalid);
        }
        Ok(())
    }
}

/// Settings validation or SQLite failure.
#[derive(Debug, thiserror::Error)]
pub enum ContextSettingsError {
    /// The exact identity or numeric contract is invalid.
    #[error("context settings are invalid")]
    Invalid,
    /// The context policy row could not be persisted.
    #[error("context settings persistence failed")]
    Store(#[from] rusqlite::Error),
}

/// Reads one exact provider/model policy. A missing row intentionally does
/// not import a legacy per-thread setting whose provider identity is unknown.
pub fn load_model_policy(
    conn: &Connection,
    provider: &str,
    model: &str,
) -> Result<Option<ModelContextPolicy>, ContextSettingsError> {
    if provider.trim().is_empty() || model.trim().is_empty() {
        return Err(ContextSettingsError::Invalid);
    }
    conn.query_row(
        "SELECT provider, model, input_limit, output_reserve, \
                automatic_compaction, updated_at \
         FROM model_context_policies WHERE provider = ?1 AND model = ?2",
        params![provider, model],
        model_policy_from_row,
    )
    .optional()
    .map_err(ContextSettingsError::Store)
}

/// Reports whether pre-correction thread-owned settings exist for this model.
/// Their provider identity is unknown, so this is only a Settings migration
/// notice and must never be used as a runtime policy fallback.
pub fn has_legacy_model_settings(
    conn: &Connection,
    model: &str,
) -> Result<bool, ContextSettingsError> {
    if model.trim().is_empty() {
        return Err(ContextSettingsError::Invalid);
    }
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM context_settings WHERE model = ?1)",
        [model],
        |row| row.get(0),
    )
    .map_err(ContextSettingsError::Store)
}

/// Saves a policy without modifying any conversation-owned legacy settings,
/// checkpoints, or transcript rows.
pub fn save_model_policy(
    conn: &Connection,
    policy: &ModelContextPolicy,
) -> Result<(), ContextSettingsError> {
    let _permit = crate::write_coordination::acquire(
        conn,
        crate::write_coordination::WriteCategory::ContextCompaction,
    )?;
    policy.validate()?;
    conn.execute(
        "INSERT INTO model_context_policies \
            (provider, model, input_limit, output_reserve, automatic_compaction, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
         ON CONFLICT(provider, model) DO UPDATE SET \
            input_limit = excluded.input_limit, \
            output_reserve = excluded.output_reserve, \
            automatic_compaction = excluded.automatic_compaction, \
            updated_at = excluded.updated_at",
        params![
            policy.provider,
            policy.model,
            policy.input_limit.map(|value| value as i64),
            policy.output_reserve.map(|value| value as i64),
            policy.automatic_compaction,
            policy.updated_at,
        ],
    )?;
    Ok(())
}

/// Content-free compaction lifecycle row.  Status/usage vocabulary is kept as
/// strings here so the store remains independent from runtime and UI crates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextCompactionStatus {
    /// Append order id.
    pub id: i64,
    /// Owning conversation.
    pub thread_id: String,
    /// Exact provider/model identity.
    pub model: String,
    /// Stable identity shared by all lifecycle rows for one attempt.
    pub operation_key: String,
    /// Runtime-local operation generation.
    pub generation: u64,
    /// `started|succeeded|failed|cancelled`.
    pub phase: String,
    /// `pending|known_priced|known_unpriced|unknown`.
    pub usage_state: String,
    /// Safe failure vocabulary, if any.
    pub failure: Option<String>,
    /// Durable source revision observed by the operation.
    pub source_version: u64,
    /// Bounded estimate that triggered the operation.
    pub estimated_tokens: u64,
    /// Configured input budget.
    pub input_budget: u64,
    /// Target estimate selected for compaction.
    pub target_tokens: u64,
    /// Unix-millisecond append timestamp.
    pub created_at: i64,
}

/// Insertable content-free compaction status.
#[derive(Clone, Copy)]
pub struct NewContextCompactionStatus<'a> {
    /// Owning conversation.
    pub thread_id: &'a str,
    /// Exact provider/model identity.
    pub model: &'a str,
    /// Stable identity shared by all lifecycle rows for one attempt.
    pub operation_key: &'a str,
    /// Runtime-local operation generation.
    pub generation: u64,
    /// `started|succeeded|failed|cancelled`.
    pub phase: &'a str,
    /// `pending|known_priced|known_unpriced|unknown`.
    pub usage_state: &'a str,
    /// Safe failure vocabulary, if any.
    pub failure: Option<&'a str>,
    /// Durable source revision observed by the operation.
    pub source_version: u64,
    /// Bounded estimate that triggered the operation.
    pub estimated_tokens: u64,
    /// Configured input budget.
    pub input_budget: u64,
    /// Target estimate selected for compaction.
    pub target_tokens: u64,
    /// Unix-millisecond append timestamp.
    pub created_at: i64,
}

const STATUS_PHASES: &[&str] = &["started", "succeeded", "failed", "cancelled"];
const STATUS_USAGE_STATES: &[&str] = &["pending", "known_priced", "known_unpriced", "unknown"];
const STATUS_FAILURES: &[&str] = &[
    "cancelled",
    "source_changed",
    "no_compactable_prefix",
    "too_large",
    "invalid_summary",
    "summary_timeout",
    "images_unsupported",
    "over_limit",
    "unavailable",
];

fn invalid_status_parameter(name: &str) -> rusqlite::Error {
    rusqlite::Error::InvalidParameterName(name.to_string())
}

fn status_i64(value: u64, name: &str) -> Result<i64, rusqlite::Error> {
    i64::try_from(value).map_err(|_| invalid_status_parameter(name))
}

/// Appends one lifecycle status row.  No summary/source/provider payload is
/// accepted by this API.
pub fn insert_status(
    conn: &Connection,
    status: NewContextCompactionStatus<'_>,
) -> Result<i64, rusqlite::Error> {
    let _permit = crate::write_coordination::acquire(
        conn,
        crate::write_coordination::WriteCategory::ContextCompaction,
    )?;
    if status.thread_id.trim().is_empty()
        || status.model.trim().is_empty()
        || status.operation_key.trim().is_empty()
        || status.operation_key.len() > 256
        || !STATUS_PHASES.contains(&status.phase)
        || !STATUS_USAGE_STATES.contains(&status.usage_state)
        || status
            .failure
            .is_some_and(|failure| !STATUS_FAILURES.contains(&failure))
    {
        return Err(invalid_status_parameter("context compaction status"));
    }
    let generation = status_i64(status.generation, "generation")?;
    let source_version = status_i64(status.source_version, "source_version")?;
    let estimated_tokens = status_i64(status.estimated_tokens, "estimated_tokens")?;
    let input_budget = status_i64(status.input_budget, "input_budget")?;
    let target_tokens = status_i64(status.target_tokens, "target_tokens")?;
    let (failure, failure_detail) = match status.failure {
        Some("summary_timeout") => (Some("invalid_summary"), Some("summary_timeout")),
        other => (other, None),
    };
    conn.execute(
        "INSERT INTO context_compaction_status \
         (thread_id, model, operation_key, generation, phase, usage_state, failure, \
          source_version, estimated_tokens, input_budget, target_tokens, created_at, failure_detail) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            status.thread_id,
            status.model,
            status.operation_key,
            generation,
            status.phase,
            status.usage_state,
            failure,
            source_version,
            estimated_tokens,
            input_budget,
            target_tokens,
            status.created_at,
            failure_detail,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Reads the newest status for one exact conversation/model identity.
pub fn latest_status(
    conn: &Connection,
    thread_id: &str,
    model: &str,
) -> Result<Option<ContextCompactionStatus>, rusqlite::Error> {
    if thread_id.trim().is_empty() || model.trim().is_empty() {
        return Err(invalid_status_parameter("context status identity"));
    }
    conn.query_row(
        "SELECT id, thread_id, model, operation_key, generation, phase, usage_state, \
                CASE WHEN failure_detail = 'summary_timeout' THEN failure_detail ELSE failure END, \
                source_version, estimated_tokens, input_budget, target_tokens, created_at \
         FROM context_compaction_status \
         WHERE thread_id = ?1 AND model = ?2 ORDER BY id DESC LIMIT 1",
        params![thread_id, model],
        status_from_row,
    )
    .optional()
}

/// Returns whether any completed/unfinished summary operation for this
/// identity has unknown accounting.  Unknown is sticky: a later priced
/// primary request must not make the restored aggregate look complete.
pub fn has_unknown_usage(
    conn: &Connection,
    thread_id: &str,
    model: &str,
) -> Result<bool, rusqlite::Error> {
    if thread_id.trim().is_empty() || model.trim().is_empty() {
        return Err(invalid_status_parameter("context status identity"));
    }
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM context_compaction_status pending \
         WHERE pending.thread_id = ?1 AND pending.model = ?2 \
           AND (pending.usage_state = 'unknown' \
                OR (pending.usage_state = 'pending' AND NOT EXISTS (\
                    SELECT 1 FROM context_compaction_status terminal \
                    WHERE terminal.thread_id = pending.thread_id \
                      AND terminal.model = pending.model \
                      AND terminal.operation_key = pending.operation_key \
                      AND terminal.phase IN ('succeeded', 'failed', 'cancelled') \
                      AND terminal.usage_state IN ('known_priced', 'known_unpriced', 'unknown')\
                ))))",
        params![thread_id, model],
        |row| row.get(0),
    )
}

/// Returns whether a conversation has any summary operation whose accounting
/// is unknown, regardless of the model used for that operation.  Token usage
/// is aggregated at thread scope, so switching models must not make a prior
/// unknown summary look priced after restart.
pub fn has_unknown_usage_for_thread(
    conn: &Connection,
    thread_id: &str,
) -> Result<bool, rusqlite::Error> {
    if thread_id.trim().is_empty() {
        return Err(invalid_status_parameter("context status thread"));
    }
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM context_compaction_status pending \
         WHERE pending.thread_id = ?1 \
           AND (pending.usage_state = 'unknown' \
                OR (pending.usage_state = 'pending' AND NOT EXISTS(\
                    SELECT 1 FROM context_compaction_status terminal \
                    WHERE terminal.thread_id = pending.thread_id \
                      AND terminal.operation_key = pending.operation_key \
                      AND terminal.phase IN ('succeeded', 'failed', 'cancelled')\
                      AND terminal.usage_state IN ('known_priced', 'known_unpriced', 'unknown')\
                ))))",
        [thread_id],
        |row| row.get(0),
    )
}

fn status_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ContextCompactionStatus> {
    let generation: i64 = row.get(4)?;
    let source_version: i64 = row.get(8)?;
    let estimated_tokens: i64 = row.get(9)?;
    let input_budget: i64 = row.get(10)?;
    let target_tokens: i64 = row.get(11)?;
    Ok(ContextCompactionStatus {
        id: row.get(0)?,
        thread_id: row.get(1)?,
        model: row.get(2)?,
        operation_key: row.get(3)?,
        generation: u64::try_from(generation).map_err(|_| {
            rusqlite::Error::FromSqlConversionFailure(
                4,
                rusqlite::types::Type::Integer,
                Box::new(std::io::Error::other("negative context status generation")),
            )
        })?,
        phase: row.get(5)?,
        usage_state: row.get(6)?,
        failure: row.get(7)?,
        source_version: u64::try_from(source_version).map_err(|_| {
            rusqlite::Error::FromSqlConversionFailure(
                8,
                rusqlite::types::Type::Integer,
                Box::new(std::io::Error::other("negative context status source")),
            )
        })?,
        estimated_tokens: u64::try_from(estimated_tokens).map_err(|_| {
            rusqlite::Error::FromSqlConversionFailure(
                9,
                rusqlite::types::Type::Integer,
                Box::new(std::io::Error::other("negative context status estimate")),
            )
        })?,
        input_budget: u64::try_from(input_budget).map_err(|_| {
            rusqlite::Error::FromSqlConversionFailure(
                10,
                rusqlite::types::Type::Integer,
                Box::new(std::io::Error::other("negative context status budget")),
            )
        })?,
        target_tokens: u64::try_from(target_tokens).map_err(|_| {
            rusqlite::Error::FromSqlConversionFailure(
                11,
                rusqlite::types::Type::Integer,
                Box::new(std::io::Error::other("negative context status target")),
            )
        })?,
        created_at: row.get(12)?,
    })
}

/// One ordered persisted tool-call projection used by summarization.
#[derive(Clone, PartialEq, Eq)]
pub struct ContextToolCallRow {
    /// Provider call id, independent from any pruning projection.
    pub id: String,
    /// Owning conversation.
    pub thread_id: String,
    /// Assistant message that proposed this call.
    pub message_id: String,
    /// Thread-order sequence of the call.
    pub seq: i64,
    /// Tool name.
    pub tool: String,
    /// Original JSON input.
    pub input_json: String,
    /// Bounded persisted output, when available.
    pub output_text: Option<String>,
    /// Lifecycle state.
    pub status: String,
    /// Approval audit JSON, when available.
    pub approval: Option<String>,
    /// Exact process exit code, when available.
    pub exit_code: Option<i32>,
    /// Exact execution duration, when available.
    pub duration_ms: Option<i64>,
    /// Creation time.
    pub created_at: i64,
    /// Terminal time, when available.
    pub finished_at: Option<i64>,
    /// UTF-8 byte offset within the owning assistant text, when known.
    pub text_offset_bytes: Option<i64>,
    pub response_id: Option<String>,
    /// Digest of the optional full-output path. The raw path is not exposed,
    /// but mutations still participate in the source fence.
    output_full_path_fingerprint: String,
}

impl fmt::Debug for ContextToolCallRow {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContextToolCallRow")
            .field("id_bytes", &self.id.len())
            .field("thread_id_bytes", &self.thread_id.len())
            .field("message_id_bytes", &self.message_id.len())
            .field("seq", &self.seq)
            .field("tool_bytes", &self.tool.len())
            .field("input_bytes", &self.input_json.len())
            .field("has_output", &self.output_text.is_some())
            .field("status", &self.status)
            .field("has_approval", &self.approval.is_some())
            .field("exit_code", &self.exit_code)
            .field("duration_ms", &self.duration_ms)
            .field("created_at", &self.created_at)
            .field("finished_at", &self.finished_at)
            .field("text_offset_bytes", &self.text_offset_bytes)
            .finish()
    }
}

/// Complete bounded source projection used to plan one compaction.
#[derive(Clone, PartialEq, Eq)]
pub struct ContextSource {
    /// Conversation identity.
    pub thread_id: String,
    /// Largest persisted sequence across message and tool rows.
    pub source_version: u64,
    /// Stable fingerprint of all source values, including same-seq content
    /// mutations and image bytes.
    pub fingerprint: String,
    /// Messages in chronological order.
    pub messages: Vec<MessageRow>,
    /// Tool calls in chronological order, independent from message pruning.
    pub tool_calls: Vec<ContextToolCallRow>,
    /// Images ordered by owning message sequence and ordinal.
    pub images: Vec<ImageRow>,
}

impl fmt::Debug for ContextSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContextSource")
            .field("thread_id_bytes", &self.thread_id.len())
            .field("source_version", &self.source_version)
            .field("fingerprint_bytes", &self.fingerprint.len())
            .field("message_count", &self.messages.len())
            .field("tool_call_count", &self.tool_calls.len())
            .field("image_count", &self.images.len())
            .finish()
    }
}

/// Read-only source watermark and fingerprint captured before provider work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextSourceVersion {
    /// Largest persisted sequence.
    pub source_version: u64,
    /// Fingerprint at capture time.
    pub fingerprint: String,
}

/// A checkpoint waiting to be atomically installed.
#[derive(Clone, PartialEq, Eq)]
pub struct NewContextCheckpoint {
    /// Conversation identity.
    pub thread_id: String,
    /// Exact provider/model identity.
    pub model: String,
    /// Source revision captured before asynchronous summarization.
    pub source_version: u64,
    /// Last message sequence covered by the summary prefix.
    pub covered_through_seq: u64,
    pub covered_assistant_id: Option<String>,
    pub covered_assistant_text_bytes: u64,
    pub covered_assistant_tool_seq: u64,
    /// Fingerprint captured with `source_version`.
    pub source_fingerprint: String,
    /// Labelled historical summary; raw transcript remains in messages.
    pub summary: String,
    /// Deterministic estimator version used for the projection.
    pub estimator_version: String,
    /// Expected latest checkpoint id, or `None` when no prior projection is
    /// expected.  This is the append-only compare-and-swap fence.
    pub expected_previous_id: Option<i64>,
    /// Unix-millisecond creation time.
    pub created_at: i64,
}

impl fmt::Debug for NewContextCheckpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NewContextCheckpoint")
            .field("thread_id_bytes", &self.thread_id.len())
            .field("model_bytes", &self.model.len())
            .field("source_version", &self.source_version)
            .field("covered_through_seq", &self.covered_through_seq)
            .field("fingerprint_bytes", &self.source_fingerprint.len())
            .field("summary_bytes", &self.summary.len())
            .field("estimator_version", &self.estimator_version)
            .field("expected_previous_id", &self.expected_previous_id)
            .field("created_at", &self.created_at)
            .finish()
    }
}

impl NewContextCheckpoint {
    fn validate(&self) -> Result<(), ContextCheckpointError> {
        if self.thread_id.trim().is_empty()
            || self.model.trim().is_empty()
            || self.source_version > i64::MAX as u64
            || self.covered_through_seq > i64::MAX as u64
            || self.source_fingerprint.trim().is_empty()
            || self.summary.trim().is_empty()
            || self.estimator_version.trim().is_empty()
        {
            return Err(ContextCheckpointError::Invalid);
        }
        if self.covered_through_seq > self.source_version {
            return Err(ContextCheckpointError::Invalid);
        }
        Ok(())
    }
}

/// Durable checkpoint row returned by reads and successful installs.
#[derive(Clone, PartialEq, Eq)]
pub struct ContextCheckpoint {
    /// SQLite row id; append order is durable.
    pub id: i64,
    /// Conversation identity.
    pub thread_id: String,
    /// Exact provider/model identity.
    pub model: String,
    /// Source watermark/version.
    pub source_version: u64,
    /// Last source message sequence covered by the summary.
    pub covered_through_seq: u64,
    pub covered_assistant_id: Option<String>,
    pub covered_assistant_text_bytes: u64,
    pub covered_assistant_tool_seq: u64,
    /// Source fingerprint captured with the checkpoint.
    pub source_fingerprint: String,
    /// Labelled historical summary.
    pub summary: String,
    /// Estimator metadata.
    pub estimator_version: String,
    /// Predecessor fence captured with this request for idempotent retries.
    pub expected_previous_id: Option<i64>,
    /// Unix-millisecond creation time.
    pub created_at: i64,
}

impl fmt::Debug for ContextCheckpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContextCheckpoint")
            .field("id", &self.id)
            .field("thread_id_bytes", &self.thread_id.len())
            .field("model_bytes", &self.model.len())
            .field("source_version", &self.source_version)
            .field("covered_through_seq", &self.covered_through_seq)
            .field("fingerprint_bytes", &self.source_fingerprint.len())
            .field("summary_bytes", &self.summary.len())
            .field("estimator_version", &self.estimator_version)
            .field("expected_previous_id", &self.expected_previous_id)
            .field("created_at", &self.created_at)
            .finish()
    }
}

/// Result of the compare-and-swap checkpoint install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextCheckpointInstall {
    /// New row committed atomically.
    Applied(ContextCheckpoint),
    /// The asynchronous result no longer matches the source or prior row.
    Stale,
    /// An identical append already committed; safe for an idempotent retry.
    AlreadyCommitted(ContextCheckpoint),
}

/// Checkpoint validation, stale fencing, or SQLite failure.
#[derive(Debug, thiserror::Error)]
pub enum ContextCheckpointError {
    /// Required identity, watermark, metadata, or summary is invalid.
    #[error("context checkpoint is invalid")]
    Invalid,
    /// Source changed or the expected prior append no longer matches.
    #[error("context checkpoint is stale")]
    Stale,
    /// The proposed covered prefix contains streaming or non-terminal tool
    /// activity and cannot be treated as an immutable summary source.
    #[error("context checkpoint coverage is incomplete")]
    IncompleteCoverage,
    /// SQLite transaction failed; rollback leaves the previous projection.
    #[error("context checkpoint persistence failed")]
    Store(#[from] rusqlite::Error),
    /// Cancellation won before the checkpoint transaction committed.
    #[error("context checkpoint installation was cancelled")]
    Cancelled,
}

/// Loads settings for exactly one `(thread_id, model)` pair.
pub fn load_settings(
    conn: &Connection,
    thread_id: &str,
    model: &str,
) -> Result<Option<ContextSettings>, ContextSettingsError> {
    if thread_id.trim().is_empty() || model.trim().is_empty() {
        return Err(ContextSettingsError::Invalid);
    }
    conn.query_row(
        "SELECT thread_id, model, context_limit, output_reserve, \
                automatic_compaction, updated_at \
         FROM context_settings WHERE thread_id = ?1 AND model = ?2",
        params![thread_id, model],
        context_settings_from_row,
    )
    .optional()
    .map_err(ContextSettingsError::Store)
}

/// Inserts or updates settings for exactly one `(thread_id, model)` pair.
pub fn save_settings(
    conn: &Connection,
    settings: &ContextSettings,
) -> Result<(), ContextSettingsError> {
    let _permit = crate::write_coordination::acquire(
        conn,
        crate::write_coordination::WriteCategory::ContextCompaction,
    )?;
    settings.validate()?;
    conn.execute(
        "INSERT INTO context_settings \
            (thread_id, model, context_limit, output_reserve, automatic_compaction, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
         ON CONFLICT(thread_id, model) DO UPDATE SET \
            context_limit = excluded.context_limit, \
            output_reserve = excluded.output_reserve, \
            automatic_compaction = excluded.automatic_compaction, \
            updated_at = excluded.updated_at",
        params![
            settings.thread_id,
            settings.model,
            settings.context_limit.map(|value| value as i64),
            settings.output_reserve as i64,
            settings.automatic_compaction,
            settings.updated_at,
        ],
    )?;
    Ok(())
}

/// Alias used by controller-facing conversation services.
pub fn upsert_settings(
    conn: &Connection,
    settings: &ContextSettings,
) -> Result<(), ContextSettingsError> {
    save_settings(conn, settings)
}

/// Reads the complete bounded source projection for one conversation.
pub fn load_source(conn: &Connection, thread_id: &str) -> Result<ContextSource, rusqlite::Error> {
    if thread_id.trim().is_empty() {
        return Err(rusqlite::Error::InvalidParameterName(
            "empty thread id".to_string(),
        ));
    }
    let transaction = Transaction::new_unchecked(conn, TransactionBehavior::Deferred)?;
    let source = load_source_inner(&transaction, thread_id, true)?;
    transaction.commit()?;
    Ok(source)
}

pub fn load_source_and_latest_checkpoint(
    conn: &Connection,
    thread_id: &str,
    model: &str,
) -> Result<(ContextSource, Option<ContextCheckpoint>), rusqlite::Error> {
    if thread_id.trim().is_empty() || model.trim().is_empty() {
        return Err(rusqlite::Error::InvalidParameterName(
            "empty replay source identity".to_string(),
        ));
    }
    let transaction = Transaction::new_unchecked(conn, TransactionBehavior::Deferred)?;
    let stored_model = crate::threads::find(&transaction, thread_id)?.map(|thread| thread.model);
    if stored_model.as_deref() != Some(model) {
        return Err(rusqlite::Error::InvalidParameterName(
            "replay model changed".to_string(),
        ));
    }
    let source = load_source_inner(&transaction, thread_id, true)?;
    let checkpoint = latest_checkpoint_in_transaction(&transaction, thread_id, model)?;
    transaction.commit()?;
    Ok((source, checkpoint))
}

pub fn redact_legacy_thread_content<F>(
    conn: &Connection,
    thread_id: &str,
    mut redact: F,
) -> Result<bool, rusqlite::Error>
where
    F: FnMut(&str, &[usize]) -> (String, Vec<usize>),
{
    let _permit = crate::write_coordination::acquire(
        conn,
        crate::write_coordination::WriteCategory::ContextCompaction,
    )?;
    let tool_rows = {
        let mut statement = conn.prepare(
            "SELECT id, output_text FROM tool_calls WHERE thread_id = ?1 AND output_text IS NOT NULL",
        )?;
        statement
            .query_map([thread_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?
    };
    let mut changed = false;
    for (id, text) in tool_rows {
        let (redacted, _) = redact(&text, &[]);
        if redacted != text {
            conn.execute(
                "UPDATE tool_calls SET output_text = ?1 WHERE id = ?2 AND thread_id = ?3",
                params![redacted, id, thread_id],
            )?;
            changed = true;
        }
    }

    let message_rows = {
        let mut statement = conn.prepare(
            "SELECT id, content FROM messages WHERE thread_id = ?1 AND role = 'assistant' AND status <> 'streaming'",
        )?;
        statement
            .query_map([thread_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?
    };
    for (id, text) in message_rows {
        let (redacted, _) = redact(&text, &[]);
        if redacted != text {
            let call_rows = {
                let mut statement = conn.prepare(
                    "SELECT id, text_offset_bytes FROM tool_calls WHERE thread_id = ?1 AND message_id = ?2",
                )?;
                statement
                    .query_map(params![thread_id, id], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, Option<i64>>(1)?))
                    })?
                    .collect::<Result<Vec<_>, _>>()?
            };
            let mut offsets = Vec::new();
            for (_, offset) in &call_rows {
                let Some(offset) = offset else {
                    continue;
                };
                let offset = usize::try_from(*offset).map_err(|_| {
                    rusqlite::Error::InvalidParameterName("negative tool text offset".into())
                })?;
                if offset > text.len() || !text.is_char_boundary(offset) {
                    return Err(rusqlite::Error::InvalidParameterName(
                        "invalid tool text offset".into(),
                    ));
                }
                offsets.push(offset);
            }
            let (redacted, redacted_offsets) = redact(&text, &offsets);
            if redacted_offsets.len() != offsets.len() {
                return Err(rusqlite::Error::InvalidParameterName(
                    "invalid redacted tool text offsets".into(),
                ));
            }
            let mut redacted_offsets = redacted_offsets.into_iter();
            for (call_id, offset) in call_rows {
                if offset.is_none() {
                    continue;
                }
                let redacted_offset = redacted_offsets.next().ok_or_else(|| {
                    rusqlite::Error::InvalidParameterName(
                        "missing redacted tool text offset".into(),
                    )
                })?;
                if redacted_offset > redacted.len() || !redacted.is_char_boundary(redacted_offset) {
                    return Err(rusqlite::Error::InvalidParameterName(
                        "invalid redacted tool text offset".into(),
                    ));
                }
                conn.execute(
                    "UPDATE tool_calls SET text_offset_bytes = ?1 WHERE id = ?2 AND thread_id = ?3",
                    params![redacted_offset as i64, call_id, thread_id],
                )?;
            }
            if redacted_offsets.next().is_some() {
                return Err(rusqlite::Error::InvalidParameterName(
                    "extra redacted tool text offsets".into(),
                ));
            }
            conn.execute(
                "UPDATE messages SET content = ?1 WHERE id = ?2 AND thread_id = ?3 AND role = 'assistant' AND status <> 'streaming'",
                params![redacted, id, thread_id],
            )?;
            changed = true;
        }
    }

    let checkpoint_rows = {
        let mut statement =
            conn.prepare("SELECT id, summary FROM context_checkpoints WHERE thread_id = ?1")?;
        statement
            .query_map([thread_id], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?
    };
    for (id, summary) in checkpoint_rows {
        let (redacted, _) = redact(&summary, &[]);
        if redacted != summary {
            conn.execute(
                "UPDATE context_checkpoints SET summary = ?1 WHERE id = ?2 AND thread_id = ?3",
                params![redacted, id, thread_id],
            )?;
            changed = true;
        }
    }
    Ok(changed)
}

/// Loads a complete source projection inside a caller-owned transaction.
///
/// Conversation preparation uses this before committing its user/streaming
/// rows so malformed images or an over-bound history roll back atomically with
/// the new turn.  The caller owns the transaction snapshot and commit.
pub fn load_source_in_transaction(
    transaction: &Transaction<'_>,
    thread_id: &str,
) -> Result<ContextSource, rusqlite::Error> {
    if thread_id.trim().is_empty() {
        return Err(rusqlite::Error::InvalidParameterName(
            "empty thread id".to_string(),
        ));
    }
    load_source_inner(transaction, thread_id, true)
}

/// Captures only the source fence when a caller does not need blobs.
pub fn load_source_version(
    conn: &Connection,
    thread_id: &str,
) -> Result<ContextSourceVersion, rusqlite::Error> {
    if thread_id.trim().is_empty() {
        return Err(rusqlite::Error::InvalidParameterName(
            "empty thread id".to_string(),
        ));
    }
    let transaction = Transaction::new_unchecked(conn, TransactionBehavior::Deferred)?;
    let source = load_source_inner(&transaction, thread_id, false)?;
    transaction.commit()?;
    Ok(ContextSourceVersion {
        source_version: source.source_version,
        fingerprint: source.fingerprint,
    })
}

/// Captures a metadata-only source fence in a caller-owned transaction.
pub fn load_source_version_in_transaction(
    transaction: &Transaction<'_>,
    thread_id: &str,
) -> Result<ContextSourceVersion, rusqlite::Error> {
    if thread_id.trim().is_empty() {
        return Err(rusqlite::Error::InvalidParameterName(
            "empty thread id".to_string(),
        ));
    }
    let source = load_source_inner(transaction, thread_id, false)?;
    Ok(ContextSourceVersion {
        source_version: source.source_version,
        fingerprint: source.fingerprint,
    })
}

pub fn latest_checkpoint(
    conn: &Connection,
    thread_id: &str,
    model: &str,
) -> Result<Option<ContextCheckpoint>, rusqlite::Error> {
    if thread_id.trim().is_empty() || model.trim().is_empty() {
        return Err(rusqlite::Error::InvalidParameterName(
            "empty checkpoint identity".to_string(),
        ));
    }
    conn.query_row(
        "SELECT id, thread_id, model, source_version, covered_through_seq, \
                source_fingerprint, summary, estimator_version, expected_previous_id, created_at, covered_assistant_id, covered_assistant_text_bytes, covered_assistant_tool_seq \
         FROM context_checkpoints \
         WHERE thread_id = ?1 ORDER BY id DESC LIMIT 1",
        params![thread_id],
        checkpoint_from_row,
    )
    .optional()
}

/// Loads the latest checkpoint using a caller-owned transaction snapshot.
pub fn latest_checkpoint_in_transaction(
    transaction: &Transaction<'_>,
    thread_id: &str,
    model: &str,
) -> Result<Option<ContextCheckpoint>, rusqlite::Error> {
    if thread_id.trim().is_empty() || model.trim().is_empty() {
        return Err(rusqlite::Error::InvalidParameterName(
            "empty checkpoint identity".to_string(),
        ));
    }
    transaction
        .query_row(
            "SELECT id, thread_id, model, source_version, covered_through_seq, \
                    source_fingerprint, summary, estimator_version, expected_previous_id, created_at, covered_assistant_id, covered_assistant_text_bytes, covered_assistant_tool_seq \
             FROM context_checkpoints \
             WHERE thread_id = ?1 ORDER BY id DESC LIMIT 1",
            params![thread_id],
            checkpoint_from_row,
        )
        .optional()
}

/// Alias used when restoring the durable projection after restart.
pub fn load_checkpoint(
    conn: &Connection,
    thread_id: &str,
    model: &str,
) -> Result<Option<ContextCheckpoint>, rusqlite::Error> {
    latest_checkpoint(conn, thread_id, model)
}

/// Installs a checkpoint only if both the source fence and expected prior
/// append still match.  The source is re-read inside `BEGIN IMMEDIATE`, so a
/// same-sequence content/tool mutation cannot race an asynchronous summary.
pub fn install_checkpoint(
    conn: &Connection,
    checkpoint: &NewContextCheckpoint,
) -> Result<ContextCheckpointInstall, ContextCheckpointError> {
    install_checkpoint_with_guard(conn, checkpoint, || false)
}

/// Installs a checkpoint with a caller-owned cancellation guard.
///
/// The guard is evaluated after `BEGIN IMMEDIATE` acquires the writer lock,
/// immediately before the insert, and immediately before commit.  A cancel
/// observed at any of those points rolls back the transaction, so a caller
/// waiting on SQLite cannot accidentally publish a checkpoint after it has
/// requested cancellation.  The original [`install_checkpoint`] facade uses
/// an always-false guard and retains the ordinary store API.
pub fn install_checkpoint_with_guard<F>(
    conn: &Connection,
    checkpoint: &NewContextCheckpoint,
    should_cancel: F,
) -> Result<ContextCheckpointInstall, ContextCheckpointError>
where
    F: Fn() -> bool,
{
    checkpoint.validate()?;
    let transaction = crate::write_coordination::WriteTransaction::new_with_cancel(
        conn,
        crate::write_coordination::WriteCategory::ContextCompaction,
        &should_cancel,
    )
    .map_err(|error| match error {
        rusqlite::Error::SqliteFailure(ref failure, _)
            if failure.extended_code == rusqlite::ffi::SQLITE_INTERRUPT =>
        {
            ContextCheckpointError::Cancelled
        }
        error => ContextCheckpointError::Store(error),
    })?;
    if should_cancel() {
        transaction.rollback()?;
        return Err(ContextCheckpointError::Cancelled);
    }

    let current = load_source_inner(&transaction, &checkpoint.thread_id, false)?;
    if current.source_version != checkpoint.source_version
        || current.fingerprint != checkpoint.source_fingerprint
    {
        transaction.rollback()?;
        return Ok(ContextCheckpointInstall::Stale);
    }

    if let Some(owner) = checkpoint.covered_assistant_id.as_deref() {
        let message = current
            .messages
            .iter()
            .find(|message| message.id == owner)
            .ok_or(ContextCheckpointError::IncompleteCoverage)?;
        let cursor = usize::try_from(checkpoint.covered_assistant_text_bytes)
            .map_err(|_| ContextCheckpointError::Invalid)?;
        if message.role != "assistant"
            || message.seq as u64 != checkpoint.covered_through_seq.saturating_add(1)
            || !message.content.is_char_boundary(cursor)
        {
            return Err(ContextCheckpointError::IncompleteCoverage);
        }
        let calls = current
            .tool_calls
            .iter()
            .filter(|call| {
                call.message_id == owner && call.seq as u64 <= checkpoint.covered_assistant_tool_seq
            })
            .collect::<Vec<_>>();
        if calls.is_empty()
            || calls.iter().any(|call| {
                !matches!(
                    call.status.as_str(),
                    "success" | "failed" | "cancelled" | "rejected"
                ) || call.output_text.is_none()
            })
        {
            return Err(ContextCheckpointError::IncompleteCoverage);
        }
        let last = calls
            .iter()
            .max_by_key(|call| call.seq)
            .ok_or(ContextCheckpointError::IncompleteCoverage)?;
        if last.seq as u64 != checkpoint.covered_assistant_tool_seq
            || last.text_offset_bytes != Some(cursor as i64)
            || current.tool_calls.iter().any(|call| {
                call.message_id == owner
                    && call.seq > last.seq
                    && match (&call.response_id, &last.response_id) {
                        (Some(left), Some(right)) => left == right,
                        _ => call.text_offset_bytes == last.text_offset_bytes,
                    }
            })
        {
            return Err(ContextCheckpointError::IncompleteCoverage);
        }
    } else if checkpoint.covered_assistant_text_bytes != 0
        || checkpoint.covered_assistant_tool_seq != 0
    {
        return Err(ContextCheckpointError::Invalid);
    }

    let thread_model: Option<String> = transaction
        .query_row(
            "SELECT model FROM threads WHERE id = ?1",
            [&checkpoint.thread_id],
            |row| row.get(0),
        )
        .optional()?;
    if thread_model.as_deref() != Some(checkpoint.model.as_str()) {
        transaction.rollback()?;
        return Ok(ContextCheckpointInstall::Stale);
    }
    let incomplete: i64 = transaction.query_row(
        "SELECT CASE WHEN EXISTS (\
            SELECT 1 FROM messages WHERE thread_id = ?1 AND seq <= ?2 \
              AND status NOT IN ('done', 'failed', 'interrupted')\
          ) OR EXISTS (\
            SELECT 1 FROM tool_calls c JOIN messages m ON m.id = c.message_id \
             WHERE c.thread_id = ?1 AND m.thread_id = ?1 AND m.seq <= ?2 \
               AND c.status NOT IN ('success', 'failed', 'cancelled', 'rejected')\
          ) THEN 1 ELSE 0 END",
        params![checkpoint.thread_id, checkpoint.covered_through_seq as i64],
        |row| row.get(0),
    )?;
    if incomplete != 0 {
        transaction.rollback()?;
        return Err(ContextCheckpointError::IncompleteCoverage);
    }

    // Request identity includes the captured predecessor but is independent
    // from whichever row is latest now. This makes a retry of a second append
    // idempotent even when its original predecessor is no longer latest.
    let committed_same_request = transaction
        .query_row(
            "SELECT id, thread_id, model, source_version, covered_through_seq, \
                    source_fingerprint, summary, estimator_version, expected_previous_id, created_at, covered_assistant_id, covered_assistant_text_bytes, covered_assistant_tool_seq \
             FROM context_checkpoints \
             WHERE thread_id = ?1 AND model = ?2 AND source_version = ?3 \
               AND covered_through_seq = ?4 AND source_fingerprint = ?5 \
               AND summary = ?6 AND estimator_version = ?7 \
               AND expected_previous_id IS ?8 AND created_at = ?9 AND covered_assistant_id IS ?10 AND covered_assistant_text_bytes = ?11 AND covered_assistant_tool_seq = ?12 \
             ORDER BY id DESC LIMIT 1",
            params![
                checkpoint.thread_id,
                checkpoint.model,
                checkpoint.source_version as i64,
                checkpoint.covered_through_seq as i64,
                checkpoint.source_fingerprint,
                checkpoint.summary,
                checkpoint.estimator_version,
                checkpoint.expected_previous_id,
                checkpoint.created_at,
            checkpoint.covered_assistant_id,
            checkpoint.covered_assistant_text_bytes as i64,
            checkpoint.covered_assistant_tool_seq as i64,
            ],
            checkpoint_from_row,
        )
        .optional()?;
    if let Some(existing) = committed_same_request {
        transaction.rollback()?;
        return Ok(ContextCheckpointInstall::AlreadyCommitted(existing));
    }

    let previous = transaction
        .query_row(
            "SELECT id, thread_id, model, source_version, covered_through_seq, \
                    source_fingerprint, summary, estimator_version, expected_previous_id, created_at, covered_assistant_id, covered_assistant_text_bytes, covered_assistant_tool_seq \
             FROM context_checkpoints WHERE thread_id = ?1 \
             ORDER BY id DESC LIMIT 1",
            params![checkpoint.thread_id],
            checkpoint_from_row,
        )
        .optional()?;
    if let Some(previous) = &previous {
        if previous.model == checkpoint.model
            && previous.source_version == checkpoint.source_version
            && previous.source_fingerprint == checkpoint.source_fingerprint
        {
            transaction.rollback()?;
            return Ok(ContextCheckpointInstall::Stale);
        }
        if checkpoint.expected_previous_id != Some(previous.id) {
            transaction.rollback()?;
            return Ok(ContextCheckpointInstall::Stale);
        }
        // The caller won the prior-row CAS and proposed a new append for a
        // newer source/result. Continue to the INSERT below.
    }
    if previous.is_none() && checkpoint.expected_previous_id.is_some() {
        transaction.rollback()?;
        return Ok(ContextCheckpointInstall::Stale);
    }

    if should_cancel() {
        transaction.rollback()?;
        return Err(ContextCheckpointError::Cancelled);
    }

    transaction.execute(
        "INSERT INTO context_checkpoints \
            (thread_id, model, source_version, covered_through_seq, source_fingerprint, \
             summary, estimator_version, expected_previous_id, created_at, covered_assistant_id, covered_assistant_text_bytes, covered_assistant_tool_seq) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            checkpoint.thread_id,
            checkpoint.model,
            checkpoint.source_version as i64,
            checkpoint.covered_through_seq as i64,
            checkpoint.source_fingerprint,
            checkpoint.summary,
            checkpoint.estimator_version,
            checkpoint.expected_previous_id,
            checkpoint.created_at,
            checkpoint.covered_assistant_id,
            checkpoint.covered_assistant_text_bytes as i64,
            checkpoint.covered_assistant_tool_seq as i64,
        ],
    )?;
    if should_cancel() {
        transaction.rollback()?;
        return Err(ContextCheckpointError::Cancelled);
    }
    let id = transaction.last_insert_rowid();
    let committed = ContextCheckpoint {
        id,
        thread_id: checkpoint.thread_id.clone(),
        model: checkpoint.model.clone(),
        source_version: checkpoint.source_version,
        covered_through_seq: checkpoint.covered_through_seq,
        covered_assistant_id: checkpoint.covered_assistant_id.clone(),
        covered_assistant_text_bytes: checkpoint.covered_assistant_text_bytes,
        covered_assistant_tool_seq: checkpoint.covered_assistant_tool_seq,
        source_fingerprint: checkpoint.source_fingerprint.clone(),
        summary: checkpoint.summary.clone(),
        estimator_version: checkpoint.estimator_version.clone(),
        expected_previous_id: checkpoint.expected_previous_id,
        created_at: checkpoint.created_at,
    };
    transaction.commit()?;
    Ok(ContextCheckpointInstall::Applied(committed))
}

fn context_settings_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ContextSettings> {
    let context_limit: Option<i64> = row.get(2)?;
    let output_reserve: i64 = row.get(3)?;
    let context_limit = context_limit
        .map(u64::try_from)
        .transpose()
        .map_err(|_| rusqlite::Error::InvalidParameterName("negative context limit".to_string()))?;
    let output_reserve = u64::try_from(output_reserve).map_err(|_| {
        rusqlite::Error::InvalidParameterName("negative output reserve".to_string())
    })?;
    Ok(ContextSettings {
        thread_id: row.get(0)?,
        model: row.get(1)?,
        context_limit,
        output_reserve,
        automatic_compaction: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

fn model_policy_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ModelContextPolicy> {
    let input_limit: Option<i64> = row.get(2)?;
    let output_reserve: Option<i64> = row.get(3)?;
    let input_limit = input_limit
        .map(u64::try_from)
        .transpose()
        .map_err(|_| rusqlite::Error::InvalidParameterName("negative input limit".into()))?;
    let output_reserve = output_reserve
        .map(u64::try_from)
        .transpose()
        .map_err(|_| rusqlite::Error::InvalidParameterName("negative output reserve".into()))?;
    Ok(ModelContextPolicy {
        provider: row.get(0)?,
        model: row.get(1)?,
        input_limit,
        output_reserve,
        automatic_compaction: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

fn load_source_inner(
    conn: &Connection,
    thread_id: &str,
    materialize_images: bool,
) -> rusqlite::Result<ContextSource> {
    let messages = load_messages(conn, thread_id)?;
    let tool_calls = load_tool_calls(conn, thread_id)?;
    let (fingerprint, images) =
        fingerprint_source(conn, &messages, &tool_calls, thread_id, materialize_images)?;
    let source_version = messages
        .iter()
        .map(|row| row.seq)
        .chain(tool_calls.iter().map(|row| row.seq))
        .fold(0_i64, i64::max);
    let source_version = u64::try_from(source_version).map_err(|_| {
        rusqlite::Error::InvalidParameterName("negative source sequence".to_string())
    })?;
    Ok(ContextSource {
        thread_id: thread_id.to_string(),
        source_version,
        fingerprint,
        messages,
        tool_calls,
        images,
    })
}

pub(crate) fn source_version_without_continuation(
    conn: &Connection,
    thread_id: &str,
    continuation_message_id: &str,
) -> rusqlite::Result<ContextSourceVersion> {
    let mut messages = load_messages(conn, thread_id)?;
    let mut tool_calls = load_tool_calls(conn, thread_id)?;
    messages.retain(|message| message.id != continuation_message_id);
    tool_calls.retain(|call| call.message_id != continuation_message_id);
    let (fingerprint, _) = fingerprint_source(conn, &messages, &tool_calls, thread_id, false)?;
    let source_version = messages
        .iter()
        .map(|row| row.seq)
        .chain(tool_calls.iter().map(|row| row.seq))
        .fold(0_i64, i64::max);
    Ok(ContextSourceVersion {
        source_version: u64::try_from(source_version).map_err(|_| {
            rusqlite::Error::InvalidParameterName("negative source sequence".into())
        })?,
        fingerprint,
    })
}

fn load_messages(conn: &Connection, thread_id: &str) -> rusqlite::Result<Vec<MessageRow>> {
    let (count, content_bytes): (i64, i64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(length(CAST(COALESCE((SELECT p.model_content FROM pending_messages p WHERE p.thread_id = messages.thread_id AND p.id = messages.id AND messages.role = 'user' AND p.state = 'delivered'), content) AS BLOB))), 0) \
         FROM messages WHERE thread_id = ?1",
        [thread_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if count > MAX_SOURCE_ROWS || content_bytes > MAX_SOURCE_TEXT_BYTES {
        return Err(source_too_large());
    }
    let mut statement = conn.prepare(
        "SELECT id, thread_id, seq, role, kind, COALESCE((SELECT p.model_content FROM pending_messages p WHERE p.thread_id = messages.thread_id AND p.id = messages.id AND messages.role = 'user' AND p.state = 'delivered'), content), status, created_at, \
                plan_status, plan_review_note, plan_reviewed_at \
         FROM messages WHERE thread_id = ?1 ORDER BY seq, id",
    )?;
    statement
        .query_map([thread_id], |row| {
            Ok(MessageRow {
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
            })
        })?
        .collect()
}

fn load_tool_calls(
    conn: &Connection,
    thread_id: &str,
) -> rusqlite::Result<Vec<ContextToolCallRow>> {
    let (count, input_bytes, output_bytes): (i64, i64, i64) = conn.query_row(
        "SELECT COUNT(*), \
                COALESCE(SUM(length(CAST(input_json AS BLOB))), 0), \
                COALESCE(SUM(length(CAST(COALESCE(output_text, '') AS BLOB))), 0) \
         FROM tool_calls WHERE thread_id = ?1",
        [thread_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    if count > MAX_SOURCE_ROWS
        || input_bytes > MAX_SOURCE_TEXT_BYTES
        || output_bytes > MAX_SOURCE_TEXT_BYTES
    {
        return Err(source_too_large());
    }
    let mut statement = conn.prepare(
        "SELECT id, thread_id, message_id, seq, tool, input_json, output_text, status, \
                approval, exit_code, duration_ms, created_at, finished_at, text_offset_bytes, \
                output_full_path, (SELECT response_id FROM tool_call_rounds r WHERE r.message_id = tool_calls.message_id AND r.call_id = tool_calls.id) \
         FROM tool_calls WHERE thread_id = ?1 ORDER BY seq, id",
    )?;
    statement
        .query_map([thread_id], |row| {
            Ok(ContextToolCallRow {
                id: row.get(0)?,
                thread_id: row.get(1)?,
                message_id: row.get(2)?,
                seq: row.get(3)?,
                tool: row.get(4)?,
                input_json: row.get(5)?,
                output_text: row.get(6)?,
                status: row.get(7)?,
                approval: row.get(8)?,
                exit_code: row.get(9)?,
                duration_ms: row.get(10)?,
                created_at: row.get(11)?,
                finished_at: row.get(12)?,
                text_offset_bytes: row.get(13)?,
                response_id: row.get(15)?,
                output_full_path_fingerprint: path_fingerprint(row.get(14)?),
            })
        })?
        .collect()
}

fn checkpoint_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ContextCheckpoint> {
    let source_version: i64 = row.get(3)?;
    let covered_through_seq: i64 = row.get(4)?;
    let source_version = u64::try_from(source_version).map_err(|_| {
        rusqlite::Error::InvalidParameterName("negative checkpoint source version".to_string())
    })?;
    let covered_through_seq = u64::try_from(covered_through_seq).map_err(|_| {
        rusqlite::Error::InvalidParameterName("negative covered sequence".to_string())
    })?;
    Ok(ContextCheckpoint {
        id: row.get(0)?,
        thread_id: row.get(1)?,
        model: row.get(2)?,
        source_version,
        covered_through_seq,
        covered_assistant_id: row.get(10)?,
        covered_assistant_text_bytes: u64::try_from(row.get::<_, i64>(11)?).map_err(|_| {
            rusqlite::Error::InvalidParameterName("negative checkpoint text cursor".into())
        })?,
        covered_assistant_tool_seq: u64::try_from(row.get::<_, i64>(12)?).map_err(|_| {
            rusqlite::Error::InvalidParameterName("negative checkpoint tool cursor".into())
        })?,
        source_fingerprint: row.get(5)?,
        summary: row.get(6)?,
        estimator_version: row.get(7)?,
        expected_previous_id: row.get(8)?,
        created_at: row.get(9)?,
    })
}

fn source_too_large() -> rusqlite::Error {
    rusqlite::Error::InvalidParameterName("context source exceeds bounded read".to_string())
}

fn fingerprint_source(
    conn: &Connection,
    messages: &[MessageRow],
    tool_calls: &[ContextToolCallRow],
    thread_id: &str,
    materialize_images: bool,
) -> rusqlite::Result<(String, Vec<ImageRow>)> {
    // SHA-256 is a stable source revision, not an authorization primitive.
    // Every field is length-delimited and Option discriminants are explicit,
    // so same-seq content/status/tool/image mutations cannot be mistaken for
    // an unchanged source.
    let mut hash = Sha256::new();
    for message in messages {
        feed(&mut hash, b"message", message.id.as_bytes());
        feed(&mut hash, b"thread", message.thread_id.as_bytes());
        feed(&mut hash, b"seq", &message.seq.to_le_bytes());
        feed(&mut hash, b"role", message.role.as_bytes());
        feed(&mut hash, b"kind", message.kind.as_bytes());
        feed(&mut hash, b"content", message.content.as_bytes());
        feed(&mut hash, b"status", message.status.as_bytes());
        feed(&mut hash, b"created", &message.created_at.to_le_bytes());
        feed_optional_str(&mut hash, b"plan-status", message.plan_status.as_deref());
        feed_optional_str(&mut hash, b"plan-note", message.plan_review_note.as_deref());
        feed_optional_i64(&mut hash, b"plan-reviewed", message.plan_reviewed_at);
    }
    for call in tool_calls {
        feed(&mut hash, b"call", call.id.as_bytes());
        feed(&mut hash, b"thread", call.thread_id.as_bytes());
        feed(&mut hash, b"message", call.message_id.as_bytes());
        feed(&mut hash, b"seq", &call.seq.to_le_bytes());
        feed(&mut hash, b"tool", call.tool.as_bytes());
        feed(&mut hash, b"input", call.input_json.as_bytes());
        feed_optional_str(&mut hash, b"output", call.output_text.as_deref());
        feed(&mut hash, b"status", call.status.as_bytes());
        feed_optional_str(&mut hash, b"approval", call.approval.as_deref());
        feed_optional_i32(&mut hash, b"exit", call.exit_code);
        feed_optional_i64(&mut hash, b"duration", call.duration_ms);
        feed(&mut hash, b"created", &call.created_at.to_le_bytes());
        feed_optional_i64(&mut hash, b"finished", call.finished_at);
        feed_optional_i64(&mut hash, b"text-offset-bytes", call.text_offset_bytes);
        feed_optional_str(&mut hash, b"response-id", call.response_id.as_deref());
        feed(
            &mut hash,
            b"output-full-path",
            call.output_full_path_fingerprint.as_bytes(),
        );
    }
    let (image_count, image_bytes): (i64, i64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(length(encoded)), 0) \
         FROM image_attachments a JOIN messages m ON m.id = a.message_id \
         WHERE m.thread_id = ?1 AND m.role = 'user'",
        [thread_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if image_count > MAX_SOURCE_ROWS || image_bytes > MAX_SOURCE_IMAGE_BYTES {
        return Err(source_too_large());
    }
    let image_capacity = if materialize_images {
        usize::try_from(image_count).map_err(|_| source_too_large())?
    } else {
        0
    };
    let mut images = Vec::with_capacity(image_capacity);
    let mut statement = conn.prepare(
        "SELECT a.message_id, a.ordinal, a.encoded \
         FROM image_attachments a JOIN messages m ON m.id = a.message_id \
         WHERE m.thread_id = ?1 AND m.role = 'user' ORDER BY m.seq, a.ordinal",
    )?;
    let mut rows = statement.query([thread_id])?;
    while let Some(row) = rows.next()? {
        let message_id: String = row.get(0)?;
        let ordinal: i64 = row.get(1)?;
        let encoded = row.get_ref(2)?.as_blob()?;
        feed(&mut hash, b"image-message", message_id.as_bytes());
        feed(&mut hash, b"image-ordinal", &ordinal.to_le_bytes());
        feed(&mut hash, b"image-bytes", encoded);
        if materialize_images {
            images.push(ImageRow {
                message_id,
                ordinal,
                artifact_call_id: None,
                encoded: encoded.to_vec(),
            });
        }
    }
    Ok((format!("{:x}", hash.finalize()), images))
}

fn feed(hash: &mut Sha256, tag: &[u8], value: &[u8]) {
    hash.update((tag.len() as u64).to_le_bytes());
    hash.update(tag);
    hash.update((value.len() as u64).to_le_bytes());
    hash.update(value);
}

fn feed_optional_str(hash: &mut Sha256, tag: &[u8], value: Option<&str>) {
    feed(hash, tag, &[u8::from(value.is_some())]);
    if let Some(value) = value {
        feed(hash, tag, value.as_bytes());
    }
}

fn feed_optional_i32(hash: &mut Sha256, tag: &[u8], value: Option<i32>) {
    feed(hash, tag, &[u8::from(value.is_some())]);
    if let Some(value) = value {
        feed(hash, tag, &value.to_le_bytes());
    }
}

fn feed_optional_i64(hash: &mut Sha256, tag: &[u8], value: Option<i64>) {
    feed(hash, tag, &[u8::from(value.is_some())]);
    if let Some(value) = value {
        feed(hash, tag, &value.to_le_bytes());
    }
}

fn path_fingerprint(path: Option<String>) -> String {
    let mut hash = Sha256::new();
    feed_optional_str(&mut hash, b"output-full-path", path.as_deref());
    format!("{:x}", hash.finalize())
}
