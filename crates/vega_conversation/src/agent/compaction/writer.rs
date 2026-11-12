use super::*;
use crate::database_writer::{DatabaseCommand, Sealed};

pub(super) fn commit<C: DatabaseCommand<Output = Result<T, VegaError>>, T: Send + 'static>(
    path: &std::path::Path,
    command: C,
) -> Result<T, VegaError> {
    let store = Store::open_existing(path).map_err(VegaError::Store)?;
    crate::database_writer::commit(path, store.database_identity(), command)
        .map_err(|_| context_error(ContextRuntimeError::SourceChanged))?
}

pub(super) struct Install {
    pub checkpoint: vega_store::context_compaction::PreparedContextCheckpoint,
    pub admission: vega_store::thread_admission::ThreadAdmission,
    pub install_cancel: CancellationToken,
    pub deadline: tokio::time::Instant,
}
impl Sealed for Install {}
impl DatabaseCommand for Install {
    type Output = Result<(ContextCheckpointInstall, Option<String>), VegaError>;
    fn apply(self, store: &Store) -> Self::Output {
        let Self {
            checkpoint,
            admission,
            install_cancel,
            deadline,
        } = self;
        crate::thread_access::verify(store, &checkpoint.checkpoint().thread_id, &admission)
            .map_err(|_| context_error(ContextRuntimeError::SourceChanged))?;
        let (install, retired_generation) = checkpoint
            .install_and_retire(store.conn(), || {
                install_cancel.is_cancelled() || tokio::time::Instant::now() >= deadline
            })
            .map_err(|error| match error {
                vega_store::context_compaction::ContextCheckpointError::Stale => {
                    context_error(ContextRuntimeError::SourceChanged)
                }
                vega_store::context_compaction::ContextCheckpointError::IncompleteCoverage => {
                    context_error(ContextRuntimeError::SummaryProjectionInvalid)
                }
                vega_store::context_compaction::ContextCheckpointError::Invalid => {
                    context_error(ContextRuntimeError::SummaryProjectionInvalid)
                }
                vega_store::context_compaction::ContextCheckpointError::Cancelled => {
                    if install_cancel.is_cancelled() {
                        VegaError::Cancelled
                    } else {
                        context_error(ContextRuntimeError::SummaryTimedOut)
                    }
                }
                vega_store::context_compaction::ContextCheckpointError::Store(error) => {
                    VegaError::Store(error)
                }
            })?;
        Ok((install, retired_generation))
    }
}

pub(super) struct Consume {
    pub checkpoint: NewContextCheckpoint,
    pub admission: vega_store::thread_admission::ThreadAdmission,
    pub prepared: vega_store::context_precompute::consume::ConsumptionCommit,
    pub install: Option<vega_store::context_compaction::PreparedContextCheckpoint>,
    pub borrow: bool,
    pub install_cancel: CancellationToken,
    pub deadline: tokio::time::Instant,
}
impl Sealed for Consume {}
impl DatabaseCommand for Consume {
    type Output = Result<Option<ContextCheckpointInstall>, VegaError>;
    fn apply(self, store: &Store) -> Self::Output {
        use vega_store::context_compaction::ContextCheckpointError;
        use vega_store::context_precompute::consume;
        let Self {
            checkpoint,
            admission,
            prepared,
            install,
            borrow,
            install_cancel,
            deadline,
        } = self;
        crate::thread_access::verify(store, &checkpoint.thread_id, &admission)
            .map_err(|_| context_error(ContextRuntimeError::SourceChanged))?;
        if borrow {
            let transaction = store
                .conn()
                .unchecked_transaction()
                .map_err(VegaError::Store)?;
            if !consume::validate_for_install_in_transaction(
                &transaction,
                &prepared,
                &checkpoint.model,
                now_ms(),
            )
            .map_err(precompute::store_error)?
            {
                return Err(context_error(ContextRuntimeError::SourceChanged));
            }
            transaction.commit().map_err(VegaError::Store)?;
            if install_cancel.is_cancelled() {
                return Err(VegaError::Cancelled);
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(context_error(ContextRuntimeError::SourceChanged));
            }
            return Ok(None);
        }
        let install = install.ok_or_else(|| context_error(ContextRuntimeError::SourceChanged))?;
        match consume::install_prepared_with_guard(
            store.conn(),
            &prepared,
            &checkpoint,
            now_ms(),
            &install,
            || install_cancel.is_cancelled() || tokio::time::Instant::now() >= deadline,
        ) {
            Err(ContextCheckpointError::Cancelled) if !install_cancel.is_cancelled() => {
                let transaction = store
                    .conn()
                    .unchecked_transaction()
                    .map_err(|_| context_error(ContextRuntimeError::SourceChanged))?;
                if !matches!(
                    consume::validate_for_install_in_transaction(
                        &transaction,
                        &prepared,
                        &checkpoint.model,
                        now_ms(),
                    ),
                    Ok(true)
                ) {
                    return Err(context_error(ContextRuntimeError::SourceChanged));
                }
                transaction
                    .commit()
                    .map_err(|_| context_error(ContextRuntimeError::SourceChanged))?;
                if install_cancel.is_cancelled() {
                    return Err(VegaError::Cancelled);
                }
                Err(context_error(ContextRuntimeError::SummaryTimedOut))
            }
            result => result.map(Some).map_err(precompute::store_error),
        }
    }
}

pub(super) fn commit_micro(
    path: &std::path::Path,
    diagnostics: Option<&super::super::diagnostics::DiagnosticsContext>,
    command: Micro,
) -> Result<String, VegaError> {
    let store = Store::open_existing(path).map_err(VegaError::Store)?;
    micro_writer_result(
        crate::database_writer::commit(path, store.database_identity(), command),
        diagnostics,
    )
}

pub(super) fn micro_writer_result(
    result: Result<Result<String, VegaError>, String>,
    diagnostics: Option<&super::super::diagnostics::DiagnosticsContext>,
) -> Result<String, VegaError> {
    result.map_err(|_| Rejection::WriterAdmission.error(diagnostics))?
}

pub(super) struct Micro {
    pub prepared: vega_store::context_micro::PreparedMicro,
    pub cancel: CancellationToken,
    pub admission: vega_store::thread_admission::ThreadAdmission,
    pub diagnostics: Option<super::super::diagnostics::DiagnosticsContext>,
}
impl Sealed for Micro {}
impl DatabaseCommand for Micro {
    type Output = Result<String, VegaError>;
    fn apply(self, store: &Store) -> Self::Output {
        let Self {
            prepared,
            cancel,
            admission,
            diagnostics,
        } = self;
        crate::thread_access::verify(store, prepared.thread(), &admission)
            .map_err(|_| Rejection::Ownership.error(diagnostics.as_ref()))?;
        prepared
            .install(store.conn(), || cancel.is_cancelled())
            .map_err(|error| match error {
                vega_store::context_compaction::ContextCheckpointError::Cancelled => {
                    VegaError::Cancelled
                }
                vega_store::context_compaction::ContextCheckpointError::Store(error) => {
                    VegaError::Store(error)
                }
                _ => Rejection::MicroCommit.error(diagnostics.as_ref()),
            })
    }
}

pub(super) fn commit_conversation<
    C: DatabaseCommand<Output = Result<(), crate::types::ConversationError>>,
>(
    store: &Store,
    command: C,
) -> Result<(), crate::types::ConversationError> {
    let path = store
        .database_path()
        .ok_or(crate::types::ConversationError::InvalidTaskIdentity)?;
    crate::database_writer::commit(path, store.database_identity(), command)
        .map_err(crate::types::ConversationError::Store)?
}

pub(super) struct Settings {
    pub settings: crate::types::ContextSettings,
    pub admission: vega_store::thread_admission::ThreadAdmission,
}
impl Sealed for Settings {}
impl DatabaseCommand for Settings {
    type Output = Result<(), crate::types::ConversationError>;
    fn apply(self, store: &Store) -> Self::Output {
        let Self {
            settings,
            admission,
        } = self;
        crate::thread_access::verify(store, &settings.thread_id, &admission)?;
        save_settings(
            store.conn(),
            &StoreContextSettings {
                thread_id: settings.thread_id.clone(),
                model: settings.model.clone(),
                context_limit: settings.context_limit,
                output_reserve: settings.output_reserve,
                automatic_compaction: settings.automatic_compaction,
                updated_at: settings.updated_at,
            },
        )
        .map_err(|error| crate::types::ConversationError::Store(error.to_string()))
    }
}

pub(super) struct Usage {
    pub thread_id: String,
    pub model: String,
    pub usage: ContextCompactionUsage,
}
impl Sealed for Usage {}
impl DatabaseCommand for Usage {
    type Output = Result<(), crate::types::ConversationError>;
    fn apply(self, store: &Store) -> Self::Output {
        let Self {
            thread_id,
            model,
            usage,
        } = self;
        let thread_id = thread_id.as_str();
        let model = model.as_str();
        vega_store::token_usage::insert(
            store.conn(),
            vega_store::token_usage::NewTokenUsage {
                thread_id,
                message_id: None,
                model,
                input_tokens: usage.usage.input,
                output_tokens: usage.usage.output,
                cache_read_tokens: usage.usage.cache_read.unwrap_or(0),
                cache_write_tokens: usage.usage.cache_write.unwrap_or(0),
                cost_microcents: usage.cost_microcents,
                created_at: now_ms(),
                pricing_version: usage
                    .pricing
                    .as_ref()
                    .map(|pricing| pricing.version.as_str()),
                pricing_profile: usage
                    .pricing
                    .as_ref()
                    .map(|pricing| pricing.profile.as_str()),
                call_started_at: usage
                    .pricing
                    .as_ref()
                    .map(|pricing| pricing.call_started_at),
            },
        )
        .map(|_| ())
        .map_err(|error| crate::types::ConversationError::Store(error.to_string()))
    }
}

pub(super) struct Status {
    pub thread_id: String,
    pub model: String,
    pub operation_key: String,
    pub generation: u64,
    pub phase: String,
    pub record: crate::types::ContextCompactionStatusRecord,
}
impl Sealed for Status {}
impl DatabaseCommand for Status {
    type Output = Result<(), crate::types::ConversationError>;
    fn apply(self, store: &Store) -> Self::Output {
        let Self {
            thread_id,
            model,
            operation_key,
            generation,
            phase,
            record,
        } = self;
        let thread_id = thread_id.as_str();
        let model = model.as_str();
        let operation_key = operation_key.as_str();
        let usage_state = match record.usage {
            crate::types::ContextCompactionUsageState::Pending => "pending",
            crate::types::ContextCompactionUsageState::Known { priced: true } => "known_priced",
            crate::types::ContextCompactionUsageState::Known { priced: false } => "known_unpriced",
            crate::types::ContextCompactionUsageState::Unknown => "unknown",
        };
        let failure = record.failure.map(context_failure_name);
        let tx = store
            .immediate_transaction()
            .map_err(|error| crate::types::ConversationError::Store(error.to_string()))?;
        if phase == "started" {
            vega_store::messages::record_thread_tail_timestamp_boundary(
                &tx,
                thread_id,
                operation_key,
            )
            .map_err(|error| crate::types::ConversationError::Store(error.to_string()))?;
        }
        insert_status(
            &tx,
            NewContextCompactionStatus {
                thread_id,
                model,
                operation_key,
                generation,
                phase: &phase,
                usage_state,
                failure,
                source_version: record.source_version.unwrap_or_default(),
                estimated_tokens: record.estimated_tokens.unwrap_or_default(),
                input_budget: record.input_budget.unwrap_or_default(),
                target_tokens: record.target_tokens.unwrap_or_default(),
                created_at: record.updated_at,
            },
        )
        .map_err(|error| crate::types::ConversationError::Store(error.to_string()))?;
        tx.commit()
            .map_err(|error| crate::types::ConversationError::Store(error.to_string()))
    }
}
