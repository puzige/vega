use super::*;
use crate::database_writer::{DatabaseCommand, Sealed};

pub(super) fn commit<C: DatabaseCommand<Output = Result<(), VegaError>>>(
    path: &std::path::Path,
    identity: vega_store::DatabaseIdentity,
    command: C,
) -> Result<(), VegaError> {
    crate::database_writer::commit(path, identity, command)
        .map_err(|_| context_error(ContextRuntimeError::SourceChanged))?
}

pub(super) struct RecordDiagnostics {
    pub admission: vega_store::thread_admission::ThreadAdmission,
    pub thread_id: String,
    pub generation: String,
    pub events: Vec<vega_store::run_diagnostics::NewDiagnosticEvent>,
}
impl Sealed for RecordDiagnostics {}
impl DatabaseCommand for RecordDiagnostics {
    type Output = Result<(), VegaError>;

    fn apply(self, store: &Store) -> Self::Output {
        crate::thread_access::verify(store, &self.thread_id, &self.admission)
            .map_err(|_| context_error(ContextRuntimeError::SourceChanged))?;
        if self
            .events
            .iter()
            .any(|event| event.thread_id != self.thread_id || event.run_id != self.generation)
        {
            return Err(context_error(ContextRuntimeError::SourceChanged));
        }
        let transaction = vega_store::write_coordination::WriteTransaction::new(
            store.conn(),
            vega_store::write_coordination::WriteCategory::Diagnostics,
        )
        .map_err(VegaError::Store)?;
        for event in self.events {
            vega_store::run_diagnostics::insert(&transaction, &event)
                .map_err(|_| context_error(ContextRuntimeError::SourceChanged))?;
        }
        transaction.commit().map_err(VegaError::Store)
    }
}

pub(super) struct RecordStage {
    pub admission: vega_store::thread_admission::ThreadAdmission,
    pub generation: String,
    pub thread_id: String,
    pub model: String,
    pub stage: u32,
    pub reported: Option<accounting::ReportedUsage>,
    pub parent: Option<(String, String)>,
    pub claim: Option<crate::types::AutomationRunClaim>,
}
impl Sealed for RecordStage {}
impl DatabaseCommand for RecordStage {
    type Output = Result<(), VegaError>;
    fn apply(self, store: &Store) -> Self::Output {
        let Self {
            admission,
            generation,
            thread_id,
            model,
            stage,
            reported,
            parent,
            claim,
        } = self;
        crate::thread_access::verify(store, &thread_id, &admission)
            .map_err(|_| context_error(ContextRuntimeError::SourceChanged))?;
        let transaction = vega_store::write_coordination::WriteTransaction::new(
            store.conn(),
            vega_store::write_coordination::WriteCategory::Usage,
        )
        .map_err(VegaError::Store)?;
        let id = accounting::record_stage_in_transaction(
            &transaction,
            &generation,
            &thread_id,
            &model,
            stage,
            reported.as_ref(),
        )
        .map_err(store_error)?;
        if let (Some(id), Some(usage)) = (id, reported.as_ref()) {
            if let Some((parent_thread, parent_message)) = &parent {
                vega_store::subagents::attribute_usage(
                    &transaction,
                    id,
                    &thread_id,
                    parent_thread,
                    parent_message,
                    usage.cache_read_tokens.is_some() && usage.cache_write_tokens.is_some(),
                )
                .map_err(VegaError::Store)?;
            }
            if let Some(claim) = &claim {
                crate::automation::record_usage(
                    &transaction,
                    claim,
                    id,
                    usage.input_tokens,
                    usage.output_tokens,
                )
                .map_err(|_| context_error(ContextRuntimeError::SourceChanged))?;
            }
        }
        transaction.commit().map_err(VegaError::Store)?;
        Ok(())
    }
}

pub(super) struct Finish {
    pub admission: vega_store::thread_admission::ThreadAdmission,
    pub generation: String,
    pub thread_id: String,
    pub model: String,
    pub stages: u32,
    pub complete: bool,
}
impl Sealed for Finish {}
impl DatabaseCommand for Finish {
    type Output = Result<(), VegaError>;
    fn apply(self, store: &Store) -> Self::Output {
        crate::thread_access::verify(store, &self.thread_id, &self.admission)
            .map_err(|_| context_error(ContextRuntimeError::SourceChanged))?;
        accounting::finish(
            store.conn(),
            &self.generation,
            &self.thread_id,
            &self.model,
            self.stages,
            self.complete,
        )
        .map_err(store_error)
    }
}

pub(super) struct Retire {
    pub admission: vega_store::thread_admission::ThreadAdmission,
    pub thread_id: String,
    pub generation: String,
}
impl Sealed for Retire {}
impl DatabaseCommand for Retire {
    type Output = Result<(), VegaError>;
    fn apply(self, store: &Store) -> Self::Output {
        crate::thread_access::verify(store, &self.thread_id, &self.admission)
            .map_err(|_| context_error(ContextRuntimeError::SourceChanged))?;
        ready::retire(store.conn(), &self.thread_id, &self.generation)
            .map(|_| ())
            .map_err(store_error)
    }
}

pub(super) struct Begin {
    pub authority: crate::types::PermissionFence,
    pub admission: vega_store::thread_admission::ThreadAdmission,
    pub generation: String,
    pub thread_id: String,
    pub model: String,
    pub expected_generation: Option<String>,
    pub expected_checkpoint: Option<i64>,
}
impl Sealed for Begin {}
impl DatabaseCommand for Begin {
    type Output = Result<(), VegaError>;
    fn apply(self, store: &Store) -> Self::Output {
        let _coordination = crate::permission_contracts::coordinate()
            .map_err(|_| context_error(ContextRuntimeError::SourceChanged))?;
        verify_publication(store, &self.thread_id, &self.admission, &self.authority)?;
        ready::begin_generation_after_checkpoint(
            store.conn(),
            &self.generation,
            &self.thread_id,
            &self.model,
            self.expected_generation.as_deref(),
            self.expected_checkpoint,
        )
        .map_err(store_error)
    }
}

pub(super) struct Publish {
    pub authority: crate::types::PermissionFence,
    pub admission: vega_store::thread_admission::ThreadAdmission,
    pub candidate: ready::ReadyCompaction,
    pub prepared: ready::PreparedPublication,
    pub cancel: CancellationToken,
}
impl Sealed for Publish {}
impl DatabaseCommand for Publish {
    type Output = Result<(), VegaError>;
    fn apply(self, store: &Store) -> Self::Output {
        let _coordination = crate::permission_contracts::coordinate()
            .map_err(|_| context_error(ContextRuntimeError::SourceChanged))?;
        verify_publication(
            store,
            &self.candidate.checkpoint.thread_id,
            &self.admission,
            &self.authority,
        )?;
        if self.cancel.is_cancelled() {
            return Err(VegaError::Cancelled);
        }
        ready::publish_prepared_with_guard(
            store.conn(),
            &self.candidate,
            now_ms(),
            &self.prepared,
            || {
                !self.cancel.is_cancelled()
                    && verify_publication(
                        store,
                        &self.candidate.checkpoint.thread_id,
                        &self.admission,
                        &self.authority,
                    )
                    .is_ok()
            },
        )
        .map_err(store_error)
    }
}

fn verify_publication(
    store: &Store,
    thread: &str,
    admission: &vega_store::thread_admission::ThreadAdmission,
    authority: &crate::types::PermissionFence,
) -> Result<(), VegaError> {
    crate::thread_access::verify(store, thread, admission)
        .map_err(|_| context_error(ContextRuntimeError::SourceChanged))?;
    let row = vega_store::threads::find(store.conn(), thread)
        .map_err(VegaError::Store)?
        .ok_or_else(|| context_error(ContextRuntimeError::SourceChanged))?;
    if crate::permission_contracts::capture_fence_for_commit(store, &row)
        .map_err(|_| context_error(ContextRuntimeError::SourceChanged))?
        != *authority
    {
        return Err(context_error(ContextRuntimeError::SourceChanged));
    }
    Ok(())
}
