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
    conn.execute(
        "INSERT INTO context_compaction_status \
         (thread_id, model, operation_key, generation, phase, usage_state, failure, \
          source_version, estimated_tokens, input_budget, target_tokens, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            status.thread_id,
            status.model,
            status.operation_key,
            generation,
            status.phase,
            status.usage_state,
            status.failure,
            source_version,
            estimated_tokens,
            input_budget,
            target_tokens,
            status.created_at,
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
        "SELECT id, thread_id, model, operation_key, generation, phase, usage_state, failure, \
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

/// Loads the latest committed checkpoint for one exact identity.
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
                source_fingerprint, summary, estimator_version, expected_previous_id, created_at \
         FROM context_checkpoints \
         WHERE thread_id = ?1 AND model = ?2 ORDER BY id DESC LIMIT 1",
        params![thread_id, model],
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
                    source_fingerprint, summary, estimator_version, expected_previous_id, created_at \
             FROM context_checkpoints \
             WHERE thread_id = ?1 AND model = ?2 ORDER BY id DESC LIMIT 1",
            params![thread_id, model],
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
    let transaction = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
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
                    source_fingerprint, summary, estimator_version, expected_previous_id, created_at \
             FROM context_checkpoints \
             WHERE thread_id = ?1 AND model = ?2 AND source_version = ?3 \
               AND covered_through_seq = ?4 AND source_fingerprint = ?5 \
               AND summary = ?6 AND estimator_version = ?7 \
               AND expected_previous_id IS ?8 AND created_at = ?9 \
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
                    source_fingerprint, summary, estimator_version, expected_previous_id, created_at \
             FROM context_checkpoints WHERE thread_id = ?1 AND model = ?2 \
             ORDER BY id DESC LIMIT 1",
            params![checkpoint.thread_id, checkpoint.model],
            checkpoint_from_row,
        )
        .optional()?;
    if let Some(previous) = &previous {
        if previous.source_version == checkpoint.source_version
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
             summary, estimator_version, expected_previous_id, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
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

fn load_messages(conn: &Connection, thread_id: &str) -> rusqlite::Result<Vec<MessageRow>> {
    let (count, content_bytes): (i64, i64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(length(CAST(content AS BLOB))), 0) \
         FROM messages WHERE thread_id = ?1",
        [thread_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if count > MAX_SOURCE_ROWS || content_bytes > MAX_SOURCE_TEXT_BYTES {
        return Err(source_too_large());
    }
    let mut statement = conn.prepare(
        "SELECT id, thread_id, seq, role, kind, content, status, created_at, \
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
                output_full_path \
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
        feed(
            &mut hash,
            b"output-full-path",
            call.output_full_path_fingerprint.as_bytes(),
        );
    }
    let (image_count, image_bytes): (i64, i64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(length(encoded)), 0) \
         FROM image_attachments a JOIN messages m ON m.id = a.message_id \
         WHERE m.thread_id = ?1",
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
         WHERE m.thread_id = ?1 ORDER BY m.seq, a.ordinal",
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Store, messages, threads};
    use tempfile::tempdir;

    fn store() -> (Store, tempfile::TempDir) {
        let directory = tempdir().unwrap();
        let store = Store::open(directory.path().join("vega.db")).unwrap();
        store.migrate().unwrap();
        store
            .conn()
            .execute(
                "INSERT INTO projects (id, path, name, created_at, last_opened_at) \
                 VALUES ('p', '/tmp/context-compaction', 'p', 0, 0)",
                [],
            )
            .unwrap();
        store
            .conn()
            .execute(
                "INSERT INTO threads (id, project_id, model, created_at, updated_at) \
                 VALUES ('t', 'p', 'model-a', 0, 0)",
                [],
            )
            .unwrap();
        (store, directory)
    }

    fn add_message(store: &Store, id: &str, seq: i64, role: &str, content: &str) {
        messages::insert(
            store.conn(),
            &messages::MessageRow {
                id: id.into(),
                thread_id: "t".into(),
                seq,
                role: role.into(),
                kind: "text".into(),
                content: content.into(),
                status: "done".into(),
                created_at: seq,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .unwrap();
    }

    fn checkpoint(store: &Store, previous: Option<i64>) -> NewContextCheckpoint {
        let source = load_source(store.conn(), "t").unwrap();
        NewContextCheckpoint {
            thread_id: "t".into(),
            model: "model-a".into(),
            source_version: source.source_version,
            covered_through_seq: source.source_version,
            source_fingerprint: source.fingerprint,
            summary: "[historical context] keep the user's goal".into(),
            estimator_version: "test-estimator-v1".into(),
            expected_previous_id: previous,
            created_at: 1,
        }
    }

    #[test]
    fn missing_model_policy_has_editable_assumed_defaults_without_writing_a_row() {
        let (store, _directory) = store();
        assert!(
            load_model_policy(store.conn(), "provider-a", "model-a")
                .unwrap()
                .is_none()
        );
        let policy = load_model_policy(store.conn(), "provider-a", "model-a")
            .unwrap()
            .unwrap_or_else(|| ModelContextPolicy::assumed_default("provider-a", "model-a"));
        assert!(policy.automatic_compaction);
        assert_eq!(policy.input_limit, Some(300_000));
        assert_eq!(policy.output_reserve, Some(128_000));
        assert!(
            load_model_policy(store.conn(), "provider-a", "model-a")
                .unwrap()
                .is_none(),
            "opening the model must not silently persist assumed defaults"
        );
    }

    #[test]
    fn explicit_unknown_capacity_is_distinct_from_missing_policy() {
        let (store, _directory) = store();
        let unknown = ModelContextPolicy::unconfigured("provider-a", "model-a");
        save_model_policy(store.conn(), &unknown).unwrap();
        assert_eq!(
            load_model_policy(store.conn(), "provider-a", "model-a").unwrap(),
            Some(unknown)
        );
    }

    #[test]
    fn model_policy_is_shared_across_threads_but_isolated_by_provider_and_model() {
        let (store, directory) = store();
        store
            .conn()
            .execute(
                "INSERT INTO threads (id, project_id, model, created_at, updated_at) \
                 VALUES ('other-thread', 'p', 'model-a', 0, 0)",
                [],
            )
            .unwrap();
        let policy = ModelContextPolicy {
            provider: "provider-a".into(),
            model: "model-a".into(),
            input_limit: Some(8_000),
            output_reserve: Some(2_000),
            automatic_compaction: false,
            updated_at: 10,
        };
        save_model_policy(store.conn(), &policy).unwrap();
        assert_eq!(
            load_model_policy(store.conn(), "provider-a", "model-a").unwrap(),
            Some(policy.clone())
        );
        assert_eq!(
            load_model_policy(store.conn(), "provider-b", "model-a").unwrap(),
            None
        );
        assert_eq!(
            load_model_policy(store.conn(), "provider-a", "model-b").unwrap(),
            None
        );
        drop(store);
        let reopened = Store::open(directory.path().join("vega.db")).unwrap();
        reopened.migrate().unwrap();
        assert_eq!(
            load_model_policy(reopened.conn(), "provider-a", "model-a").unwrap(),
            Some(policy)
        );
        let count: i64 = reopened
            .conn()
            .query_row("SELECT COUNT(*) FROM model_context_policies", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 1, "policy does not duplicate per thread");
    }

    #[test]
    fn model_policy_rejects_partial_or_inconsistent_numeric_values() {
        let (store, _directory) = store();
        let base = ModelContextPolicy {
            provider: "provider-a".into(),
            model: "model-a".into(),
            input_limit: Some(8),
            output_reserve: Some(2),
            automatic_compaction: true,
            updated_at: 1,
        };
        for invalid in [
            ModelContextPolicy {
                provider: String::new(),
                ..base.clone()
            },
            ModelContextPolicy {
                model: String::new(),
                ..base.clone()
            },
            ModelContextPolicy {
                input_limit: Some(0),
                ..base.clone()
            },
            ModelContextPolicy {
                output_reserve: None,
                ..base.clone()
            },
            ModelContextPolicy {
                output_reserve: Some(MAX_CONTEXT_VALUE),
                ..base.clone()
            },
            ModelContextPolicy {
                input_limit: None,
                ..base.clone()
            },
            ModelContextPolicy {
                input_limit: Some(MAX_CONTEXT_VALUE),
                ..base.clone()
            },
        ] {
            assert!(matches!(
                save_model_policy(store.conn(), &invalid),
                Err(ContextSettingsError::Invalid)
            ));
        }
        assert!(
            load_model_policy(store.conn(), "provider-a", "model-a")
                .unwrap()
                .is_none()
        );
        let largest_valid = ModelContextPolicy {
            input_limit: Some(MAX_CONTEXT_VALUE - 1),
            output_reserve: Some(1),
            ..base
        };
        save_model_policy(store.conn(), &largest_valid).unwrap();
        assert_eq!(
            load_model_policy(store.conn(), "provider-a", "model-a").unwrap(),
            Some(largest_valid)
        );
    }

    #[test]
    fn model_policy_does_not_delete_or_import_conflicting_legacy_thread_settings() {
        let (store, directory) = store();
        store
            .conn()
            .execute(
                "INSERT INTO threads (id, project_id, model, created_at, updated_at) \
                 VALUES ('other-thread', 'p', 'model-a', 0, 0)",
                [],
            )
            .unwrap();
        for (thread_id, limit) in [("t", 10_000), ("other-thread", 20_000)] {
            save_settings(
                store.conn(),
                &ContextSettings {
                    thread_id: thread_id.into(),
                    model: "model-a".into(),
                    context_limit: Some(limit),
                    output_reserve: 1_000,
                    automatic_compaction: true,
                    updated_at: 5,
                },
            )
            .unwrap();
        }
        assert_eq!(
            load_model_policy(store.conn(), "provider-a", "model-a").unwrap(),
            None,
            "legacy rows cannot identify a provider or choose between conflicting limits"
        );
        assert!(has_legacy_model_settings(store.conn(), "model-a").unwrap());
        assert!(!has_legacy_model_settings(store.conn(), "model-b").unwrap());
        let policy = ModelContextPolicy {
            provider: "provider-a".into(),
            model: "model-a".into(),
            input_limit: Some(28_000),
            output_reserve: Some(2_000),
            automatic_compaction: true,
            updated_at: 6,
        };
        save_model_policy(store.conn(), &policy).unwrap();
        drop(store);
        let reopened = Store::open(directory.path().join("vega.db")).unwrap();
        reopened.migrate().unwrap();
        assert_eq!(
            load_model_policy(reopened.conn(), "provider-a", "model-a").unwrap(),
            Some(policy)
        );
        assert!(has_legacy_model_settings(reopened.conn(), "model-a").unwrap());
        assert_eq!(
            load_settings(reopened.conn(), "t", "model-a")
                .unwrap()
                .unwrap()
                .context_limit,
            Some(10_000)
        );
        assert_eq!(
            load_settings(reopened.conn(), "other-thread", "model-a")
                .unwrap()
                .unwrap()
                .context_limit,
            Some(20_000)
        );
    }

    #[test]
    fn settings_are_isolated_by_exact_thread_and_model_and_reopen() {
        let (store, directory) = store();
        save_settings(
            store.conn(),
            &ContextSettings {
                thread_id: "t".into(),
                model: "model-a".into(),
                context_limit: Some(10_000),
                output_reserve: 2_000,
                automatic_compaction: true,
                updated_at: 5,
            },
        )
        .unwrap();
        save_settings(
            store.conn(),
            &ContextSettings {
                thread_id: "t".into(),
                model: "model-b".into(),
                context_limit: None,
                output_reserve: 1,
                automatic_compaction: false,
                updated_at: 6,
            },
        )
        .unwrap();
        assert_eq!(
            load_settings(store.conn(), "t", "model-a")
                .unwrap()
                .unwrap()
                .context_limit,
            Some(10_000)
        );
        assert_eq!(
            load_settings(store.conn(), "t", "model-b")
                .unwrap()
                .unwrap()
                .context_limit,
            None
        );
        drop(store);
        let reopened = Store::open(directory.path().join("vega.db")).unwrap();
        assert_eq!(
            load_settings(reopened.conn(), "t", "model-a")
                .unwrap()
                .unwrap()
                .output_reserve,
            2_000
        );
    }

    #[test]
    fn settings_reject_zero_or_inconsistent_numeric_values() {
        let (store, _directory) = store();
        let base = ContextSettings {
            thread_id: "t".into(),
            model: "model-a".into(),
            context_limit: Some(10),
            output_reserve: 2,
            automatic_compaction: true,
            updated_at: 1,
        };
        for invalid in [
            ContextSettings {
                output_reserve: 0,
                ..base.clone()
            },
            ContextSettings {
                context_limit: Some(0),
                ..base.clone()
            },
            ContextSettings {
                context_limit: Some(2),
                output_reserve: 2,
                ..base.clone()
            },
            ContextSettings {
                context_limit: Some(1),
                output_reserve: 2,
                ..base.clone()
            },
            ContextSettings {
                thread_id: String::new(),
                ..base.clone()
            },
        ] {
            assert!(matches!(
                save_settings(store.conn(), &invalid),
                Err(ContextSettingsError::Invalid)
            ));
        }
        assert!(
            load_settings(store.conn(), "t", "model-a")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn status_usage_unknown_is_operation_scoped_and_survives_reopen() {
        let (store, directory) = store();
        let started = NewContextCompactionStatus {
            thread_id: "t",
            model: "model-a",
            operation_key: "source-a",
            generation: 1,
            phase: "started",
            usage_state: "pending",
            failure: None,
            source_version: 2,
            estimated_tokens: 8_000,
            input_budget: 8_000,
            target_tokens: 6_000,
            created_at: 1,
        };
        insert_status(store.conn(), started).unwrap();
        // Until a terminal status arrives, accounting is conservatively
        // unknown (this is also the safe restart projection).
        assert!(has_unknown_usage(store.conn(), "t", "model-a").unwrap());
        insert_status(
            store.conn(),
            NewContextCompactionStatus {
                phase: "succeeded",
                usage_state: "known_priced",
                ..started
            },
        )
        .unwrap();
        assert!(!has_unknown_usage(store.conn(), "t", "model-a").unwrap());
        insert_status(
            store.conn(),
            NewContextCompactionStatus {
                operation_key: "source-b",
                generation: 1,
                phase: "started",
                usage_state: "pending",
                ..started
            },
        )
        .unwrap();
        assert!(has_unknown_usage(store.conn(), "t", "model-a").unwrap());
        drop(store);
        let reopened = Store::open(directory.path().join("vega.db")).unwrap();
        assert!(has_unknown_usage(reopened.conn(), "t", "model-a").unwrap());
        assert_eq!(
            latest_status(reopened.conn(), "t", "model-a")
                .unwrap()
                .unwrap()
                .operation_key,
            "source-b"
        );
        insert_status(
            reopened.conn(),
            NewContextCompactionStatus {
                operation_key: "source-b",
                phase: "succeeded",
                usage_state: "known_priced",
                ..started
            },
        )
        .unwrap();
        assert!(!has_unknown_usage(reopened.conn(), "t", "model-a").unwrap());
        insert_status(
            reopened.conn(),
            NewContextCompactionStatus {
                operation_key: "source-c",
                phase: "succeeded",
                usage_state: "unknown",
                ..started
            },
        )
        .unwrap();
        assert!(has_unknown_usage(reopened.conn(), "t", "model-a").unwrap());
    }

    #[test]
    fn unknown_summary_usage_is_thread_scoped_across_models_and_reopen() {
        let (store, directory) = store();
        let unknown_model_a = NewContextCompactionStatus {
            thread_id: "t",
            model: "model-a",
            operation_key: "manual-a",
            generation: 1,
            phase: "failed",
            usage_state: "unknown",
            failure: Some("unavailable"),
            source_version: 2,
            estimated_tokens: 8_000,
            input_budget: 8_000,
            target_tokens: 6_000,
            created_at: 1,
        };
        insert_status(store.conn(), unknown_model_a).unwrap();
        insert_status(
            store.conn(),
            NewContextCompactionStatus {
                model: "model-b",
                operation_key: "manual-b",
                phase: "succeeded",
                usage_state: "known_priced",
                failure: None,
                ..unknown_model_a
            },
        )
        .unwrap();

        assert!(has_unknown_usage(store.conn(), "t", "model-a").unwrap());
        assert!(!has_unknown_usage(store.conn(), "t", "model-b").unwrap());
        assert!(has_unknown_usage_for_thread(store.conn(), "t").unwrap());

        drop(store);
        let reopened = Store::open(directory.path().join("vega.db")).unwrap();
        assert!(!has_unknown_usage(reopened.conn(), "t", "model-b").unwrap());
        assert!(has_unknown_usage_for_thread(reopened.conn(), "t").unwrap());
    }

    #[test]
    fn cancelled_checkpoint_guard_after_writer_wait_rolls_back_before_commit() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::thread;
        use std::time::Duration;

        let (store, directory) = store();
        add_message(&store, "u", 1, "user", "goal");
        let proposed = checkpoint(&store, None);
        let database_path = directory.path().join("vega.db");
        let lock_store = Store::open(&database_path).unwrap();
        let writer_lock = lock_store.immediate_transaction().unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let entered = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        let worker_entered = Arc::clone(&entered);
        let worker_path = database_path.clone();
        let worker = thread::spawn(move || {
            let worker_store = Store::open(worker_path).unwrap();
            worker_entered.store(true, Ordering::Release);
            install_checkpoint_with_guard(worker_store.conn(), &proposed, || {
                worker_cancel.load(Ordering::Acquire)
            })
        });
        while !entered.load(Ordering::Acquire) {
            thread::yield_now();
        }
        thread::sleep(Duration::from_millis(50));
        cancel.store(true, Ordering::Release);
        drop(writer_lock);
        let result = worker.join().unwrap();
        assert!(matches!(result, Err(ContextCheckpointError::Cancelled)));
        assert!(
            latest_checkpoint(store.conn(), "t", "model-a")
                .unwrap()
                .is_none()
        );
        let raw_messages: i64 = store
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE thread_id = 't'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(raw_messages, 1);
    }

    #[test]
    fn source_is_chronological_and_same_seq_mutation_changes_fingerprint() {
        let (store, _directory) = store();
        add_message(&store, "u", 1, "user", "goal");
        add_message(&store, "a", 2, "assistant", "answer");
        crate::image_attachments::insert(store.conn(), "u", 0, &[1, 2, 3]).unwrap();
        store
            .conn()
            .execute(
                "INSERT INTO tool_calls (id, thread_id, message_id, seq, tool, input_json, output_text, status, created_at) \
                 VALUES ('call', 't', 'a', 3, 'read', '{\"path\":\"src\"}', 'result', 'success', 3)",
                [],
            )
            .unwrap();
        let before = load_source(store.conn(), "t").unwrap();
        assert_eq!(
            before
                .messages
                .iter()
                .map(|row| row.seq)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(before.tool_calls[0].id, "call");
        assert_eq!(before.images.len(), 1);
        let fence_only = load_source_version(store.conn(), "t").unwrap();
        assert_eq!(fence_only.source_version, before.source_version);
        assert_eq!(fence_only.fingerprint, before.fingerprint);
        store
            .conn()
            .execute(
                "UPDATE tool_calls SET text_offset_bytes = 7 WHERE id = 'call'",
                [],
            )
            .unwrap();
        let offset_changed = load_source(store.conn(), "t").unwrap();
        assert_ne!(before.fingerprint, offset_changed.fingerprint);
        store
            .conn()
            .execute("UPDATE messages SET content = 'changed' WHERE id = 'a'", [])
            .unwrap();
        let after = load_source(store.conn(), "t").unwrap();
        assert_eq!(before.source_version, after.source_version);
        assert_ne!(offset_changed.fingerprint, after.fingerprint);
    }

    #[test]
    fn context_debug_redacts_transcript_tool_payload_and_summary() {
        const SECRET: &str = "CONTEXT_COMPACTION_DEBUG_SENTINEL";
        let (store, _directory) = store();
        add_message(&store, "u", 1, "user", SECRET);
        add_message(&store, "a", 2, "assistant", "answer");
        store
            .conn()
            .execute(
                "INSERT INTO tool_calls (id, thread_id, message_id, seq, tool, input_json, output_text, status, created_at) \
                 VALUES ('call', 't', 'a', 3, 'read', ?1, ?1, 'success', 3)",
                [SECRET],
            )
            .unwrap();
        let source = load_source(store.conn(), "t").unwrap();
        let new_checkpoint = NewContextCheckpoint {
            thread_id: "t".into(),
            model: "model-a".into(),
            source_version: source.source_version,
            covered_through_seq: source.source_version,
            source_fingerprint: source.fingerprint.clone(),
            summary: SECRET.into(),
            estimator_version: "test-estimator-v1".into(),
            expected_previous_id: None,
            created_at: 1,
        };
        for rendered in [
            format!("{source:?}"),
            format!("{:?}", source.tool_calls[0]),
            format!("{new_checkpoint:?}"),
        ] {
            assert!(!rendered.contains(SECRET), "debug leaked source payload");
        }
        let installed = install_checkpoint(store.conn(), &new_checkpoint).unwrap();
        let row = match installed {
            ContextCheckpointInstall::Applied(row) => row,
            other => panic!("unexpected install result: {other:?}"),
        };
        assert!(!format!("{row:?}").contains(SECRET));
    }

    #[test]
    fn stale_same_seq_checkpoint_is_rejected_and_raw_rows_remain() {
        let (store, _directory) = store();
        add_message(&store, "u", 1, "user", "goal");
        add_message(&store, "a", 2, "assistant", "answer");
        let first = checkpoint(&store, None);
        store
            .conn()
            .execute("UPDATE messages SET content = 'changed' WHERE id = 'a'", [])
            .unwrap();
        assert_eq!(
            install_checkpoint(store.conn(), &first).unwrap(),
            ContextCheckpointInstall::Stale
        );
        assert_eq!(
            store
                .conn()
                .query_row::<String, _, _>(
                    "SELECT content FROM messages WHERE id = 'a'",
                    [],
                    |row| row.get(0)
                )
                .unwrap(),
            "changed"
        );
        assert!(
            latest_checkpoint(store.conn(), "t", "model-a")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn checkpoint_coverage_uses_owner_message_order_not_independent_tool_seq() {
        let (store, directory) = store();
        add_message(&store, "u", 1, "user", "goal");
        add_message(&store, "a", 2, "assistant", "read result");
        store
            .conn()
            .execute(
                "INSERT INTO tool_calls \
                 (id, thread_id, message_id, seq, tool, input_json, output_text, status, created_at) \
                 VALUES ('early-call', 't', 'a', 100, 'read', '{}', 'ok', 'success', 1)",
                [],
            )
            .unwrap();
        let first_source = load_source(store.conn(), "t").unwrap();
        let first = NewContextCheckpoint {
            thread_id: "t".into(),
            model: "model-a".into(),
            source_version: first_source.source_version,
            covered_through_seq: 2,
            source_fingerprint: first_source.fingerprint,
            summary: "complete early group".into(),
            estimator_version: "test-estimator-v1".into(),
            expected_previous_id: None,
            created_at: 1,
        };
        let first_id = match install_checkpoint(store.conn(), &first).unwrap() {
            ContextCheckpointInstall::Applied(row) => row.id,
            other => panic!("unexpected first checkpoint result: {other:?}"),
        };

        add_message(&store, "late", 100, "assistant", "pending late work");
        store
            .conn()
            .execute(
                "INSERT INTO tool_calls \
                 (id, thread_id, message_id, seq, tool, input_json, status, created_at) \
                 VALUES ('late-call', 't', 'late', 2, 'read', '{}', 'pending_approval', 2)",
                [],
            )
            .unwrap();
        let second_source = load_source(store.conn(), "t").unwrap();
        let second = NewContextCheckpoint {
            thread_id: "t".into(),
            model: "model-a".into(),
            source_version: second_source.source_version,
            covered_through_seq: 2,
            source_fingerprint: second_source.fingerprint,
            summary: "early group remains complete".into(),
            estimator_version: "test-estimator-v1".into(),
            expected_previous_id: Some(first_id),
            created_at: 2,
        };
        assert_eq!(first.source_version, second.source_version);
        assert_ne!(first.source_fingerprint, second.source_fingerprint);
        assert_eq!(second.expected_previous_id, Some(first_id));
        let current_fence = load_source_version(store.conn(), "t").unwrap();
        assert_eq!(current_fence.source_version, second.source_version);
        assert_eq!(current_fence.fingerprint, second.source_fingerprint);
        let second_id = match install_checkpoint(store.conn(), &second).unwrap() {
            ContextCheckpointInstall::Applied(row) => row.id,
            other => panic!("unexpected second checkpoint result: {other:?}"),
        };
        drop(store);
        let reopened = Store::open(directory.path().join("vega.db")).unwrap();
        assert_eq!(
            latest_checkpoint(reopened.conn(), "t", "model-a")
                .unwrap()
                .unwrap()
                .id,
            second_id
        );
        // The request's predecessor remains the first row; exact request
        // identity makes this retry idempotent after the second append.
        assert!(matches!(
            install_checkpoint(reopened.conn(), &second).unwrap(),
            ContextCheckpointInstall::AlreadyCommitted(row) if row.id == second_id
        ));
    }

    #[test]
    fn checkpoint_install_reloads_and_preserves_previous_on_insert_failure() {
        let (store, directory) = store();
        add_message(&store, "u", 1, "user", "goal");
        let first = checkpoint(&store, None);
        let installed = install_checkpoint(store.conn(), &first).unwrap();
        let first_row = match installed {
            ContextCheckpointInstall::Applied(row) => row,
            other => panic!("unexpected install result: {other:?}"),
        };
        assert!(matches!(
            install_checkpoint(store.conn(), &first).unwrap(),
            ContextCheckpointInstall::AlreadyCommitted(row) if row.id == first_row.id
        ));
        let different_summary = NewContextCheckpoint {
            summary: "different projection".into(),
            ..first.clone()
        };
        assert_eq!(
            install_checkpoint(store.conn(), &different_summary).unwrap(),
            ContextCheckpointInstall::Stale
        );
        let wrong_prior = NewContextCheckpoint {
            expected_previous_id: Some(first_row.id + 1),
            ..first.clone()
        };
        assert_eq!(
            install_checkpoint(store.conn(), &wrong_prior).unwrap(),
            ContextCheckpointInstall::Stale
        );
        let duplicate_request = NewContextCheckpoint {
            expected_previous_id: Some(first_row.id),
            ..first.clone()
        };
        let duplicate = install_checkpoint(store.conn(), &duplicate_request).unwrap();
        assert_eq!(duplicate, ContextCheckpointInstall::Stale);
        add_message(&store, "next", 2, "user", "new source turn");
        let source = load_source(store.conn(), "t").unwrap();
        store
            .conn()
            .execute_batch(
                "CREATE TRIGGER force_context_checkpoint_failure BEFORE INSERT ON context_checkpoints BEGIN SELECT RAISE(ABORT, 'forced'); END;",
            )
            .unwrap();
        let failed = NewContextCheckpoint {
            source_version: source.source_version,
            source_fingerprint: source.fingerprint,
            expected_previous_id: Some(first_row.id),
            summary: "new projection".into(),
            ..first.clone()
        };
        assert!(matches!(
            install_checkpoint(store.conn(), &failed),
            Err(ContextCheckpointError::Store(_))
        ));
        let current = latest_checkpoint(store.conn(), "t", "model-a")
            .unwrap()
            .unwrap();
        assert_eq!(current.id, first_row.id);
        assert_eq!(current.summary, first_row.summary);
        drop(store);
        let reopened = Store::open(directory.path().join("vega.db")).unwrap();
        assert_eq!(
            latest_checkpoint(reopened.conn(), "t", "model-a")
                .unwrap()
                .unwrap()
                .id,
            first_row.id
        );
    }

    #[test]
    fn thread_delete_cascades_settings_and_checkpoints_without_deleting_other_raw_tables() {
        let (store, _directory) = store();
        add_message(&store, "u", 1, "user", "goal");
        save_settings(
            store.conn(),
            &ContextSettings {
                thread_id: "t".into(),
                model: "model-a".into(),
                context_limit: Some(1_000),
                output_reserve: 1,
                automatic_compaction: true,
                updated_at: 1,
            },
        )
        .unwrap();
        let row = checkpoint(&store, None);
        assert!(matches!(
            install_checkpoint(store.conn(), &row).unwrap(),
            ContextCheckpointInstall::Applied(_)
        ));
        threads::delete_thread(store.conn(), "t").unwrap();
        assert!(
            latest_checkpoint(store.conn(), "t", "model-a")
                .unwrap()
                .is_none()
        );
        assert!(
            load_settings(store.conn(), "t", "model-a")
                .unwrap()
                .is_none()
        );
        assert_eq!(
            store
                .conn()
                .query_row::<i64, _, _>(
                    "SELECT COUNT(*) FROM messages WHERE thread_id = 't'",
                    [],
                    |row| row.get(0)
                )
                .unwrap(),
            0
        );
    }
}
