use rusqlite::{Connection, OptionalExtension, Transaction};

use super::{capture_in_transaction, ready};
use crate::context_compaction::{
    ContextCheckpointError, ContextCheckpointInstall, ContextSource, NewContextCheckpoint,
    install_checkpoint_in_transaction,
};
use crate::context_projection::ContinuationMode;
use crate::write_coordination::{WriteCategory, WriteTransaction};

pub struct ConsumptionCommit {
    candidate: ready::ReadyCompaction,
    encoded: String,
    fence: crate::context_compaction::source_version::SourceFence,
}

pub struct PreparedConsumption {
    candidate: ready::ReadyCompaction,
    source: ContextSource,
    usage_priced: bool,
    fence: crate::context_compaction::source_version::SourceFence,
}

impl PreparedConsumption {
    pub fn commit_projection(&self) -> Result<ConsumptionCommit, ContextCheckpointError> {
        Ok(ConsumptionCommit {
            candidate: self.candidate.clone(),
            encoded: self.candidate.encode()?,
            fence: self.fence.clone(),
        })
    }

    pub fn candidate(&self) -> &ready::ReadyCompaction {
        &self.candidate
    }

    pub fn source(&self) -> &ContextSource {
        &self.source
    }

    pub fn usage_priced(&self) -> bool {
        self.usage_priced
    }

    pub fn prepare_install(
        &self,
        conn: &Connection,
        checkpoint: NewContextCheckpoint,
    ) -> Result<crate::context_compaction::PreparedContextCheckpoint, ContextCheckpointError> {
        let tx = conn.unchecked_transaction()?;
        if !self.fence.validate(&tx)? {
            return Err(ContextCheckpointError::Stale);
        }
        let owner = checkpoint
            .projection_plan
            .as_ref()
            .and_then(|plan| plan.source_owner_id.as_deref());
        if checkpoint
            != self.checkpoint(
                &checkpoint.model,
                owner,
                &checkpoint.estimator_version,
                checkpoint.created_at,
            )?
        {
            return Err(ContextCheckpointError::Invalid);
        }
        let install = crate::context_compaction::PreparedContextCheckpoint::prepare_in_transaction(
            &tx, checkpoint,
        )?;
        tx.commit()?;
        Ok(install)
    }

    pub fn checkpoint(
        &self,
        model: &str,
        source_owner_id: Option<&str>,
        estimator_version: &str,
        now_ms: i64,
    ) -> Result<NewContextCheckpoint, ContextCheckpointError> {
        let mut checkpoint = self.candidate.checkpoint.clone();
        checkpoint.model = model.to_owned();
        checkpoint.source_version = self.source.source_version;
        checkpoint.source_fingerprint = self.source.fingerprint.clone();
        checkpoint.estimator_version = estimator_version.to_owned();
        checkpoint.created_at = now_ms;
        let plan = checkpoint
            .projection_plan
            .as_mut()
            .ok_or(ContextCheckpointError::Invalid)?;
        let owner = source_owner_id
            .map(|id| {
                self.source
                    .messages
                    .last()
                    .filter(|message| message.id == id && message.role == "assistant")
                    .ok_or(ContextCheckpointError::Stale)
            })
            .transpose()?;
        plan.source_owner_id = source_owner_id.map(str::to_owned);
        plan.continuation = if owner.is_some() {
            ContinuationMode::ContinueExistingRun
        } else {
            ContinuationMode::NoContinuation
        };
        plan.task_message_id = self
            .source
            .messages
            .iter()
            .rev()
            .find(|message| {
                message.role == "user"
                    && message.status == "done"
                    && owner.is_none_or(|owner| message.seq < owner.seq)
            })
            .map(|message| message.id.clone());
        plan.validate_install(&self.source)?;
        checkpoint.validate()?;
        Ok(checkpoint)
    }
}

pub fn prepare_in_transaction(
    transaction: &Transaction<'_>,
    thread_id: &str,
    source_owner_id: Option<&str>,
    now_ms: i64,
) -> Result<Option<PreparedConsumption>, ContextCheckpointError> {
    let Some(candidate) = ready::recover_in_transaction(transaction, thread_id, now_ms)? else {
        return Ok(None);
    };
    let (source, _) = capture_in_transaction(transaction, thread_id, source_owner_id)?;
    let (stages, unpriced): (u32, u32) = transaction.query_row(
        "SELECT COUNT(*),COALESCE(SUM(u.id IS NULL OR u.pricing_version IS NULL OR u.pricing_profile IS NULL OR u.call_started_at IS NULL),0) FROM context_precompute_stages s LEFT JOIN token_usage u ON u.id=s.usage_id AND u.thread_id=?2 AND u.model=?3 WHERE s.generation=?1",
        rusqlite::params![candidate.generation, thread_id, candidate.checkpoint.model],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if stages != candidate.stage_count {
        return Err(ContextCheckpointError::Invalid);
    }
    Ok(Some(PreparedConsumption {
        candidate,
        source,
        usage_priced: unpriced == 0,
        fence: crate::context_compaction::source_version::SourceFence::capture(
            transaction,
            thread_id,
        )?,
    }))
}

pub fn validate_for_install_in_transaction(
    transaction: &Transaction<'_>,
    prepared: &ConsumptionCommit,
    model: &str,
    now_ms: i64,
) -> Result<bool, ContextCheckpointError> {
    let candidate = &prepared.candidate;
    let thread_id = &candidate.checkpoint.thread_id;
    if now_ms < candidate.checkpoint.created_at
        || now_ms.saturating_sub(candidate.checkpoint.created_at) > ready::MAX_READY_AGE_MS
        || !prepared.fence.validate(transaction)?
    {
        return Ok(false);
    }
    let current: Option<String> = transaction.query_row("SELECT r.payload FROM context_precompute_ready r JOIN context_precompute_slots s ON s.thread_id=r.thread_id AND s.generation=r.generation JOIN context_precompute_accounting a ON a.generation=r.generation WHERE r.thread_id=?1 AND a.retired=0", [thread_id], |row| row.get(0)).optional()?;
    if current.as_deref() != Some(prepared.encoded.as_str()) {
        return Ok(false);
    }
    super::accounting::validate_finished(
        transaction,
        &candidate.generation,
        thread_id,
        &candidate.checkpoint.model,
        candidate.stage_count,
        candidate.usage_complete,
    )?;
    Ok(transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM threads WHERE id=?1 AND model=?2 AND pending_model IS NULL)",
        rusqlite::params![thread_id, model],
        |row| row.get(0),
    )?)
}

pub fn install_with_guard<F>(
    conn: &Connection,
    prepared: &PreparedConsumption,
    checkpoint: &NewContextCheckpoint,
    now_ms: i64,
    should_cancel: F,
) -> Result<ContextCheckpointInstall, ContextCheckpointError>
where
    F: Fn() -> bool,
{
    let owner = checkpoint
        .projection_plan
        .as_ref()
        .and_then(|plan| plan.source_owner_id.as_deref());
    if checkpoint
        != &prepared.checkpoint(
            &checkpoint.model,
            owner,
            &checkpoint.estimator_version,
            checkpoint.created_at,
        )?
    {
        return Err(ContextCheckpointError::Invalid);
    }
    let transaction =
        WriteTransaction::new_with_cancel(conn, WriteCategory::ContextCompaction, &should_cancel)
            .map_err(|error| match error {
            rusqlite::Error::SqliteFailure(ref failure, _)
                if failure.extended_code == rusqlite::ffi::SQLITE_INTERRUPT =>
            {
                ContextCheckpointError::Cancelled
            }
            error => ContextCheckpointError::Store(error),
        })?;
    let candidate = &prepared.candidate;
    if !validate_for_install_in_transaction(
        &transaction,
        &prepared.commit_projection()?,
        &checkpoint.model,
        now_ms,
    )? {
        return Ok(ContextCheckpointInstall::Stale);
    }
    let result = install_checkpoint_in_transaction(&transaction, checkpoint, &should_cancel)?;
    if matches!(result, ContextCheckpointInstall::Stale) {
        return Ok(result);
    }
    if !ready::retire_in_transaction(&transaction, &checkpoint.thread_id, &candidate.generation)? {
        return Err(ContextCheckpointError::Stale);
    }
    if should_cancel() {
        return Err(ContextCheckpointError::Cancelled);
    }
    transaction.commit()?;
    Ok(result)
}

pub fn install_prepared_with_guard<F>(
    conn: &Connection,
    prepared: &ConsumptionCommit,
    checkpoint: &NewContextCheckpoint,
    now_ms: i64,
    install: &crate::context_compaction::PreparedContextCheckpoint,
    should_cancel: F,
) -> Result<ContextCheckpointInstall, ContextCheckpointError>
where
    F: Fn() -> bool,
{
    if install.checkpoint() != checkpoint {
        return Err(ContextCheckpointError::Invalid);
    }
    let transaction =
        WriteTransaction::new_with_cancel(conn, WriteCategory::ContextCompaction, &should_cancel)
            .map_err(|error| match error {
            rusqlite::Error::SqliteFailure(ref failure, _)
                if failure.extended_code == rusqlite::ffi::SQLITE_INTERRUPT =>
            {
                ContextCheckpointError::Cancelled
            }
            error => ContextCheckpointError::Store(error),
        })?;
    let candidate = &prepared.candidate;
    if !validate_for_install_in_transaction(&transaction, prepared, &checkpoint.model, now_ms)? {
        return Ok(ContextCheckpointInstall::Stale);
    }
    let result = install.install_in_transaction(&transaction, &should_cancel)?;
    if matches!(result, ContextCheckpointInstall::Stale) {
        return Ok(result);
    }
    if !ready::retire_in_transaction(&transaction, &checkpoint.thread_id, &candidate.generation)? {
        return Err(ContextCheckpointError::Stale);
    }
    if should_cancel() {
        return Err(ContextCheckpointError::Cancelled);
    }
    transaction.commit()?;
    Ok(result)
}
