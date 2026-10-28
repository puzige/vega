use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::context_compaction::ContextCheckpointError;
use crate::write_coordination::{WriteCategory, WriteTransaction};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReportedUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub cost_microcents: i64,
    pub created_at: i64,
    pub pricing_version: Option<String>,
    pub pricing_profile: Option<String>,
    pub call_started_at: Option<i64>,
}

impl ReportedUsage {
    fn validate(&self) -> Result<(), ContextCheckpointError> {
        if [
            Some(self.input_tokens),
            Some(self.output_tokens),
            self.cache_read_tokens,
            self.cache_write_tokens,
        ]
        .into_iter()
        .flatten()
        .any(|value| value > i64::MAX as u64)
            || self.cost_microcents < 0
            || self.created_at < 0
            || self
                .pricing_version
                .as_ref()
                .is_some_and(|value| value.len() > 4096)
            || self
                .pricing_profile
                .as_ref()
                .is_some_and(|value| value.len() > 4096)
        {
            return Err(ContextCheckpointError::Invalid);
        }
        Ok(())
    }
}

pub fn record_stage(
    conn: &Connection,
    generation: &str,
    thread_id: &str,
    model: &str,
    stage: u32,
    usage: Option<&ReportedUsage>,
) -> Result<Option<i64>, ContextCheckpointError> {
    validate_identity(generation, thread_id, model)?;
    if !(1..=32).contains(&stage) {
        return Err(ContextCheckpointError::Invalid);
    }
    if let Some(usage) = usage {
        usage.validate()?;
    }
    let payload = serde_json::to_string(&usage).map_err(|_| ContextCheckpointError::Invalid)?;
    if payload.len() > 16384 {
        return Err(ContextCheckpointError::Invalid);
    }
    let transaction = WriteTransaction::new(conn, WriteCategory::Usage)?;
    let finished = ensure_identity(&transaction, generation, thread_id, model)?;
    let previous: Option<(String, Option<i64>)> = transaction.query_row(
        "SELECT payload,usage_id FROM context_precompute_stages WHERE generation=?1 AND stage=?2",
        params![generation, stage], |row| Ok((row.get(0)?, row.get(1)?)),
    ).optional()?;
    if let Some((previous, id)) = previous {
        if previous != payload {
            return Err(ContextCheckpointError::Invalid);
        }
        transaction.commit()?;
        return Ok(id);
    }
    if finished {
        return Err(ContextCheckpointError::Invalid);
    }
    let usage_id = usage
        .map(|usage| {
            crate::token_usage::insert(
                &transaction,
                crate::token_usage::NewTokenUsage {
                    thread_id,
                    message_id: None,
                    model,
                    input_tokens: usage.input_tokens,
                    output_tokens: usage.output_tokens,
                    cache_read_tokens: usage.cache_read_tokens.unwrap_or(0),
                    cache_write_tokens: usage.cache_write_tokens.unwrap_or(0),
                    cost_microcents: usage.cost_microcents,
                    created_at: usage.created_at,
                    pricing_version: usage.pricing_version.as_deref(),
                    pricing_profile: usage.pricing_profile.as_deref(),
                    call_started_at: usage.call_started_at,
                },
            )
        })
        .transpose()?;
    transaction.execute(
        "INSERT INTO context_precompute_stages(generation,stage,payload,usage_id) VALUES(?1,?2,?3,?4)",
        params![generation, stage, payload, usage_id],
    )?;
    transaction.commit()?;
    Ok(usage_id)
}

pub fn finish(
    conn: &Connection,
    generation: &str,
    thread_id: &str,
    model: &str,
    stage_count: u32,
    usage_complete: bool,
) -> Result<(), ContextCheckpointError> {
    validate_identity(generation, thread_id, model)?;
    if stage_count > 32 {
        return Err(ContextCheckpointError::Invalid);
    }
    let transaction = WriteTransaction::new(conn, WriteCategory::Usage)?;
    let finished = ensure_identity(&transaction, generation, thread_id, model)?;
    let (count, min, max, unknown): (u32, u32, u32, u32) = transaction.query_row(
        "SELECT COUNT(*),COALESCE(MIN(stage),0),COALESCE(MAX(stage),0),COALESCE(SUM(usage_id IS NULL),0) FROM context_precompute_stages WHERE generation=?1",
        [generation], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )?;
    if count != stage_count
        || max != stage_count
        || min != u32::from(count != 0)
        || usage_complete != (unknown == 0)
    {
        return Err(ContextCheckpointError::Invalid);
    }
    if finished {
        validate_finished(
            &transaction,
            generation,
            thread_id,
            model,
            stage_count,
            usage_complete,
        )?;
    } else {
        transaction.execute(
            "UPDATE context_precompute_accounting SET stage_count=?2,usage_complete=?3 WHERE generation=?1",
            params![generation, stage_count, usage_complete],
        )?;
    }
    transaction.commit()?;
    Ok(())
}

pub(super) fn validate_finished(
    conn: &Connection,
    generation: &str,
    thread_id: &str,
    model: &str,
    stage_count: u32,
    usage_complete: bool,
) -> Result<(), ContextCheckpointError> {
    let matches: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM context_precompute_accounting WHERE generation=?1 AND thread_id=?2 AND model=?3 AND stage_count=?4 AND usage_complete=?5)",
        params![generation, thread_id, model, stage_count, usage_complete], |row| row.get(0),
    )?;
    if !matches {
        return Err(ContextCheckpointError::Invalid);
    }
    Ok(())
}

pub(super) fn ensure_identity(
    conn: &Connection,
    generation: &str,
    thread_id: &str,
    model: &str,
) -> Result<bool, ContextCheckpointError> {
    conn.execute(
        "INSERT INTO context_precompute_accounting(generation,thread_id,model) VALUES(?1,?2,?3) ON CONFLICT(generation) DO NOTHING",
        params![generation, thread_id, model],
    )?;
    let (stored_thread, stored_model, finished): (String, String, bool) = conn.query_row(
        "SELECT thread_id,model,stage_count IS NOT NULL FROM context_precompute_accounting WHERE generation=?1",
        [generation], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    if stored_thread != thread_id || stored_model != model {
        return Err(ContextCheckpointError::Invalid);
    }
    Ok(finished)
}

pub(super) fn validate_identity(
    generation: &str,
    thread_id: &str,
    model: &str,
) -> Result<(), ContextCheckpointError> {
    if [generation, thread_id, model]
        .iter()
        .any(|value| value.is_empty() || value.len() > 4096)
    {
        return Err(ContextCheckpointError::Invalid);
    }
    Ok(())
}
