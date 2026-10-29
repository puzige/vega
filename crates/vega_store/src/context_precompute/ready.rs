use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{ContextAppendFence, accounting, validate_in_transaction};
use crate::context_compaction::{
    ContextCheckpointError, ContextSource, NewContextCheckpoint, latest_checkpoint_in_transaction,
    validate_summary_references,
};
use crate::context_projection::{ProjectionFragment, ProjectionPlan, ProjectionStrategy};
use crate::write_coordination::{WriteCategory, WriteTransaction};

pub const MAX_READY_BYTES: usize = 8_000_000;
pub const MAX_READY_AGE_MS: i64 = 604_800_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadyCompaction {
    pub schema: u32,
    pub generation: String,
    pub checkpoint: NewContextCheckpoint,
    pub source: ContextAppendFence,
    pub summary_dependencies: String,
    pub pre_tokens: u64,
    pub preferences_digest: String,
    pub stage_count: u32,
    pub usage_complete: bool,
}

impl ReadyCompaction {
    pub fn encode(&self) -> Result<String, ContextCheckpointError> {
        self.validate_shape()?;
        let encoded = serde_json::to_string(self).map_err(|_| ContextCheckpointError::Invalid)?;
        if encoded.len() > MAX_READY_BYTES {
            return Err(ContextCheckpointError::Invalid);
        }
        Ok(encoded)
    }

    pub fn decode(encoded: &str) -> Result<Self, ContextCheckpointError> {
        if encoded.len() > MAX_READY_BYTES {
            return Err(ContextCheckpointError::Invalid);
        }
        let candidate: Self =
            serde_json::from_str(encoded).map_err(|_| ContextCheckpointError::Invalid)?;
        candidate.validate_shape()?;
        Ok(candidate)
    }

    fn validate_shape(&self) -> Result<(), ContextCheckpointError> {
        accounting::validate_identity(
            &self.generation,
            &self.checkpoint.thread_id,
            &self.checkpoint.model,
        )?;
        self.checkpoint.validate()?;
        self.source.validate_shape()?;
        let plan = self.plan()?;
        plan.encode()?;
        if self.schema != 1
            || self.source.thread_id != self.checkpoint.thread_id
            || self.checkpoint.source_version
                != self
                    .source
                    .through_message_seq
                    .max(self.source.through_tool_seq)
            || self.checkpoint.summary.len() > 128 * 1024
            || self.checkpoint.created_at < 0
            || self.pre_tokens == 0
            || self.pre_tokens > i64::MAX as u64
            || self.stage_count == 0
            || self.stage_count > 32
            || plan.strategy != ProjectionStrategy::Precompute
            || plan.covered.is_empty()
            || [
                &self.summary_dependencies,
                &self.preferences_digest,
                &self.checkpoint.source_fingerprint,
            ]
            .iter()
            .any(|value| value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        {
            return Err(ContextCheckpointError::Invalid);
        }
        Ok(())
    }

    fn plan(&self) -> Result<&ProjectionPlan, ContextCheckpointError> {
        self.checkpoint
            .projection_plan
            .as_ref()
            .ok_or(ContextCheckpointError::Invalid)
    }

    pub fn validate_current(
        &self,
        transaction: &Transaction<'_>,
        now_ms: i64,
    ) -> Result<ContextSource, ContextCheckpointError> {
        self.validate_shape()?;
        if now_ms < self.checkpoint.created_at
            || now_ms.saturating_sub(self.checkpoint.created_at) > MAX_READY_AGE_MS
        {
            return Err(ContextCheckpointError::Stale);
        }
        let current = validate_in_transaction(transaction, &self.source)?;
        let previous = latest_checkpoint_in_transaction(
            transaction,
            &self.checkpoint.thread_id,
            &self.checkpoint.model,
        )?;
        if previous.as_ref().map(|checkpoint| checkpoint.id) != self.checkpoint.expected_previous_id
            || summary_dependencies_in_transaction(
                transaction,
                &self.checkpoint.thread_id,
                &self.checkpoint.model,
            )? != self.summary_dependencies
        {
            return Err(ContextCheckpointError::Stale);
        }
        let previous_plan = previous
            .as_ref()
            .and_then(|checkpoint| checkpoint.projection_plan.as_ref());
        if self.plan()?.omitted.as_slice()
            != previous_plan.map_or(&[][..], |plan| plan.omitted.as_slice())
            || self.plan()?.fragments.iter().any(|fragment| {
                if let ProjectionFragment::PreviousSummary { checkpoint_id } = fragment {
                    previous.as_ref().is_none_or(|checkpoint| {
                        checkpoint.id != *checkpoint_id
                            && !checkpoint.referenced_summaries.contains_key(checkpoint_id)
                    })
                } else {
                    false
                }
            })
        {
            return Err(ContextCheckpointError::Invalid);
        }
        let mut captured = super::captured_prefix(&current, &self.source)?;
        captured.source_version = self.checkpoint.source_version;
        self.plan()?.validate_install(&captured)?;
        self.plan()?.validate_install(&current)?;
        validate_summary_references(transaction, &self.checkpoint.thread_id, self.plan()?)?;
        accounting::validate_finished(
            transaction,
            &self.generation,
            &self.checkpoint.thread_id,
            &self.checkpoint.model,
            self.stage_count,
            self.usage_complete,
        )?;
        Ok(current)
    }
}

pub fn summary_dependencies_in_transaction(
    transaction: &Transaction<'_>,
    thread_id: &str,
    model: &str,
) -> Result<String, ContextCheckpointError> {
    let mut digest = Sha256::new();
    if let Some(previous) = latest_checkpoint_in_transaction(transaction, thread_id, model)? {
        digest.update(previous.id.to_le_bytes());
        let encoded = previous
            .projection_plan
            .as_ref()
            .map(ProjectionPlan::encode)
            .transpose()?
            .unwrap_or_default();
        for value in [&previous.summary, &previous.source_fingerprint, &encoded] {
            digest.update((value.len() as u64).to_le_bytes());
            digest.update(value.as_bytes());
        }
        for (id, summary) in previous.referenced_summaries {
            digest.update(id.to_le_bytes());
            digest.update((summary.len() as u64).to_le_bytes());
            digest.update(summary.as_bytes());
        }
    }
    Ok(format!("{:x}", digest.finalize()))
}

pub fn current_generation(
    conn: &Connection,
    thread_id: &str,
) -> Result<Option<String>, ContextCheckpointError> {
    conn.query_row(
        "SELECT generation FROM context_precompute_slots WHERE thread_id=?1",
        [thread_id],
        |row| row.get(0),
    )
    .optional()
    .map_err(ContextCheckpointError::Store)
}

pub fn begin_generation(
    conn: &Connection,
    generation: &str,
    thread_id: &str,
    model: &str,
    expected_generation: Option<&str>,
) -> Result<(), ContextCheckpointError> {
    begin_generation_in_snapshot(
        conn,
        generation,
        thread_id,
        model,
        expected_generation,
        None,
    )
}

pub fn begin_generation_after_checkpoint(
    conn: &Connection,
    generation: &str,
    thread_id: &str,
    model: &str,
    expected_generation: Option<&str>,
    expected_checkpoint: Option<i64>,
) -> Result<(), ContextCheckpointError> {
    begin_generation_in_snapshot(
        conn,
        generation,
        thread_id,
        model,
        expected_generation,
        Some(expected_checkpoint),
    )
}

fn begin_generation_in_snapshot(
    conn: &Connection,
    generation: &str,
    thread_id: &str,
    model: &str,
    expected_generation: Option<&str>,
    expected_checkpoint: Option<Option<i64>>,
) -> Result<(), ContextCheckpointError> {
    accounting::validate_identity(generation, thread_id, model)?;
    let transaction = WriteTransaction::new(conn, WriteCategory::ContextCompaction)?;
    if let Some(expected) = expected_checkpoint {
        let current: Option<i64> = transaction.query_row(
            "SELECT MAX(id) FROM context_checkpoints WHERE thread_id=?1",
            [thread_id],
            |row| row.get(0),
        )?;
        if current != expected {
            return Err(ContextCheckpointError::Stale);
        }
    }
    let current: Option<String> = transaction
        .query_row(
            "SELECT generation FROM context_precompute_slots WHERE thread_id=?1",
            [thread_id],
            |row| row.get(0),
        )
        .optional()?;
    if current.as_deref() != expected_generation && current.as_deref() != Some(generation) {
        return Err(ContextCheckpointError::Stale);
    }
    let model_matches: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM threads WHERE id=?1 AND model=?2 AND pending_model IS NULL)",
        params![thread_id, model],
        |row| row.get(0),
    )?;
    if !model_matches {
        return Err(ContextCheckpointError::Stale);
    }
    let finished = accounting::ensure_identity(&transaction, generation, thread_id, model)?;
    if finished && current.as_deref() != Some(generation) {
        return Err(ContextCheckpointError::Stale);
    }
    let retired: bool = transaction.query_row(
        "SELECT retired FROM context_precompute_accounting WHERE generation=?1",
        [generation],
        |row| row.get(0),
    )?;
    if retired {
        return Err(ContextCheckpointError::Stale);
    }
    if current.as_deref() != Some(generation) {
        if let Some(previous) = current {
            transaction.execute(
                "UPDATE context_precompute_accounting SET retired=1 WHERE generation=?1",
                [&previous],
            )?;
            transaction.execute(
                "DELETE FROM context_precompute_ready WHERE thread_id=?1 AND generation=?2",
                params![thread_id, previous],
            )?;
        }
        transaction.execute(
            "INSERT INTO context_precompute_slots(thread_id,generation) VALUES(?1,?2) ON CONFLICT(thread_id) DO UPDATE SET generation=excluded.generation",
            params![thread_id, generation],
        )?;
    }
    transaction.commit()?;
    Ok(())
}

pub fn publish(
    conn: &Connection,
    candidate: &ReadyCompaction,
    now_ms: i64,
) -> Result<(), ContextCheckpointError> {
    publish_with_guard(conn, candidate, now_ms, || true)
}

pub fn publish_with_guard(
    conn: &Connection,
    candidate: &ReadyCompaction,
    now_ms: i64,
    mut permitted: impl FnMut() -> bool,
) -> Result<(), ContextCheckpointError> {
    if !permitted() {
        return Err(ContextCheckpointError::Cancelled);
    }
    let encoded = candidate.encode()?;
    let transaction = WriteTransaction::new(conn, WriteCategory::ContextCompaction)?;
    let current: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM context_precompute_slots s JOIN context_precompute_accounting a ON a.generation=s.generation WHERE s.thread_id=?1 AND s.generation=?2 AND a.retired=0)",
        params![candidate.checkpoint.thread_id, candidate.generation], |row| row.get(0),
    )?;
    if !current {
        return Err(ContextCheckpointError::Stale);
    }
    candidate.validate_current(&transaction, now_ms)?;
    let previous: Option<String> = transaction
        .query_row(
            "SELECT payload FROM context_precompute_ready WHERE thread_id=?1",
            [&candidate.checkpoint.thread_id],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(previous) = previous {
        if previous != encoded {
            return Err(ContextCheckpointError::Invalid);
        }
    } else {
        let model_matches: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM threads WHERE id=?1 AND model=?2 AND pending_model IS NULL)",
            params![candidate.checkpoint.thread_id, candidate.checkpoint.model], |row| row.get(0),
        )?;
        if !model_matches {
            return Err(ContextCheckpointError::Stale);
        }
        transaction.execute(
            "INSERT INTO context_precompute_ready(thread_id,generation,payload) VALUES(?1,?2,?3)",
            params![
                candidate.checkpoint.thread_id,
                candidate.generation,
                encoded
            ],
        )?;
    }
    if !permitted() {
        return Err(ContextCheckpointError::Cancelled);
    }
    transaction.commit()?;
    Ok(())
}

pub fn recover(
    conn: &Connection,
    thread_id: &str,
    now_ms: i64,
) -> Result<Option<ReadyCompaction>, ContextCheckpointError> {
    let transaction = conn.unchecked_transaction()?;
    let candidate = recover_in_transaction(&transaction, thread_id, now_ms)?;
    transaction.commit()?;
    Ok(candidate)
}

pub fn recover_in_transaction(
    transaction: &Transaction<'_>,
    thread_id: &str,
    now_ms: i64,
) -> Result<Option<ReadyCompaction>, ContextCheckpointError> {
    let row: Option<(String, String)> = transaction.query_row(
        "SELECT r.generation,r.payload FROM context_precompute_ready r JOIN context_precompute_slots s ON s.thread_id=r.thread_id AND s.generation=r.generation JOIN context_precompute_accounting a ON a.generation=r.generation WHERE r.thread_id=?1 AND a.retired=0 AND length(CAST(r.payload AS BLOB))<=8000000",
        [thread_id], |row| Ok((row.get(0)?, row.get(1)?)),
    ).optional()?;
    let Some((generation, payload)) = row else {
        return Ok(None);
    };
    let candidate = ReadyCompaction::decode(&payload)?;
    if candidate.checkpoint.thread_id != thread_id || candidate.generation != generation {
        return Err(ContextCheckpointError::Invalid);
    }
    candidate.validate_current(transaction, now_ms)?;
    Ok(Some(candidate))
}

pub fn retire(
    conn: &Connection,
    thread_id: &str,
    generation: &str,
) -> Result<bool, ContextCheckpointError> {
    let transaction = WriteTransaction::new(conn, WriteCategory::ContextCompaction)?;
    let changed = retire_in_transaction(&transaction, thread_id, generation)?;
    transaction.commit()?;
    Ok(changed)
}

pub fn retire_in_transaction(
    transaction: &Transaction<'_>,
    thread_id: &str,
    generation: &str,
) -> Result<bool, ContextCheckpointError> {
    let changed = transaction.execute("UPDATE context_precompute_accounting SET retired=1 WHERE generation=?1 AND thread_id=?2 AND retired=0", params![generation, thread_id])?;
    transaction.execute(
        "DELETE FROM context_precompute_ready WHERE thread_id=?1 AND generation=?2",
        params![thread_id, generation],
    )?;
    transaction.execute(
        "DELETE FROM context_precompute_slots WHERE thread_id=?1 AND generation=?2",
        params![thread_id, generation],
    )?;
    Ok(changed != 0)
}
