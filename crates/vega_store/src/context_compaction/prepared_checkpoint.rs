use super::source_version::SourceFence;
use super::*;

pub struct PreparedContextCheckpoint {
    checkpoint: NewContextCheckpoint,
    fence: SourceFence,
}

impl PreparedContextCheckpoint {
    pub fn prepare(
        conn: &Connection,
        checkpoint: NewContextCheckpoint,
    ) -> Result<Self, ContextCheckpointError> {
        let tx = conn.unchecked_transaction()?;
        let prepared = Self::prepare_in_transaction(&tx, checkpoint)?;
        tx.commit()?;
        Ok(prepared)
    }

    pub fn prepare_in_transaction(
        tx: &Transaction<'_>,
        checkpoint: NewContextCheckpoint,
    ) -> Result<Self, ContextCheckpointError> {
        checkpoint.validate()?;
        let fence = SourceFence::capture(tx, &checkpoint.thread_id)?;
        let source = load_source_inner(tx, &checkpoint.thread_id, false)?;
        if source.source_version != checkpoint.source_version
            || source.fingerprint != checkpoint.source_fingerprint
        {
            return Err(ContextCheckpointError::Stale);
        }
        validate_prepared_coverage(tx, &checkpoint, &source)?;
        Ok(Self { checkpoint, fence })
    }

    pub fn checkpoint(&self) -> &NewContextCheckpoint {
        &self.checkpoint
    }

    pub fn validate(&self, tx: &Transaction<'_>) -> Result<bool, ContextCheckpointError> {
        Ok(self.fence.validate(tx)?)
    }

    pub fn install_in_transaction<F: Fn() -> bool>(
        &self,
        tx: &Transaction<'_>,
        cancel: &F,
    ) -> Result<ContextCheckpointInstall, ContextCheckpointError> {
        if cancel() {
            return Err(ContextCheckpointError::Cancelled);
        }
        if !self.validate(tx)? {
            return Ok(ContextCheckpointInstall::Stale);
        }
        install_validated_checkpoint_in_transaction(tx, &self.checkpoint, cancel)
    }

    pub fn install_and_retire<F: Fn() -> bool>(
        &self,
        conn: &Connection,
        cancel: F,
    ) -> Result<(ContextCheckpointInstall, Option<String>), ContextCheckpointError> {
        let tx = crate::write_coordination::WriteTransaction::new_with_cancel(
            conn,
            crate::write_coordination::WriteCategory::ContextCompaction,
            &cancel,
        )
        .map_err(|error| match error {
            rusqlite::Error::SqliteFailure(ref failure, _)
                if failure.extended_code == rusqlite::ffi::SQLITE_INTERRUPT =>
            {
                ContextCheckpointError::Cancelled
            }
            error => ContextCheckpointError::Store(error),
        })?;
        if cancel() {
            return Err(ContextCheckpointError::Cancelled);
        }
        let result = self.install_in_transaction(&tx, &cancel)?;
        let generation = if matches!(result, ContextCheckpointInstall::Applied(_)) {
            let generation = crate::context_precompute::ready::current_generation(
                &tx,
                &self.checkpoint.thread_id,
            )?;
            if let Some(generation) = &generation {
                crate::context_precompute::ready::retire_in_transaction(
                    &tx,
                    &self.checkpoint.thread_id,
                    generation,
                )?;
            }
            generation
        } else {
            None
        };
        if cancel() {
            return Err(ContextCheckpointError::Cancelled);
        }
        if matches!(result, ContextCheckpointInstall::Applied(_)) {
            tx.commit()?;
        } else {
            tx.rollback()?;
        }
        Ok((result, generation))
    }
}
