use super::*;
use vega_runtime::{
    MICRO_KEEP_RECENT, MICRO_MIN_SAVINGS, MicroCompactionResult, MicroCompactionTarget,
};

impl ConversationCompactionHook<'_> {
    pub(super) async fn micro_compact_impl(
        &self,
        request: vega_runtime::MicroCompactionRequest,
        cancel: CancellationToken,
    ) -> Result<Option<MicroCompactionResult>, VegaError> {
        if cancel.is_cancelled() {
            return Err(VegaError::Cancelled);
        }
        let mut wire = request.wire_messages();
        let request = request.context;
        if self.partial_selection.is_some() || !request.budget.automatic_compaction() {
            return Ok(None);
        }
        let original = estimate_wire_context(&wire, &request.tools)
            .map_err(|error| VegaError::Context(error.into()))?
            .input_tokens;
        if request.estimate.input_tokens
            < request
                .budget
                .precompute_tokens()
                .map_err(|error| VegaError::Context(error.into()))?
        {
            return Ok(None);
        }
        let path = self.database_path.clone();
        let thread_id = self.thread_id.clone();
        let model = self.model.clone();
        let admission = self.admission.clone();
        let read_cancel = cancel.clone();
        let (source, checkpoint, admission) = tokio::task::spawn_blocking(move || {
            if read_cancel.is_cancelled() {
                return Err(VegaError::Cancelled);
            }
            let store = Store::open(path).map_err(VegaError::Store)?;
            let admission =
                super::super::ordinary::verified_admission(&store, &thread_id, admission.as_ref())
                    .map_err(|_| context_error(ContextRuntimeError::SourceChanged))?;
            let transaction = store
                .conn()
                .unchecked_transaction()
                .map_err(VegaError::Store)?;
            let source =
                load_source_in_transaction(&transaction, &thread_id).map_err(VegaError::Store)?;
            let checkpoint = latest_checkpoint_in_transaction(&transaction, &thread_id, &model)
                .map_err(VegaError::Store)?;
            transaction.commit().map_err(VegaError::Store)?;
            Ok::<_, VegaError>((source, checkpoint, admission))
        })
        .await
        .map_err(|_| context_error(ContextRuntimeError::SourceChanged))??;
        let mut validation = request.clone();
        vega_runtime::computer_use::redact_ephemeral_messages(&mut validation.messages);
        validate_request_source(&source, checkpoint.as_ref(), &validation)
            .map_err(|failure| *failure.error)?;
        let active = super::super::pipeline::source_after_checkpoint(&source, checkpoint.as_ref())
            .map_err(|_| context_error(ContextRuntimeError::InvalidProjection))?;
        let candidates = active
            .tool_calls
            .iter()
            .filter(|call| {
                vega_store::context_micro::eligible(call)
                    && request.messages.iter().any(|message| {
                        message.role == ChatRole::Tool
                            && message.tool_call_id.as_deref() == Some(call.id.as_str())
                    })
            })
            .collect::<Vec<_>>();
        let clear_count = candidates.len().saturating_sub(MICRO_KEEP_RECENT);
        let targets = candidates[..clear_count]
            .iter()
            .filter(|call| !call.micro_compacted)
            .map(|call| MicroCompactionTarget {
                call_id: call.id.clone(),
                status: call.status.clone(),
            })
            .collect::<Vec<_>>();
        if targets.is_empty() {
            return Ok(None);
        }
        vega_runtime::apply_micro_compaction(&mut wire[1..], &targets)?;
        let after = estimate_wire_context(&wire, &request.tools)
            .map_err(|error| VegaError::Context(error.into()))?
            .input_tokens;
        let estimated_savings = original.saturating_sub(after);
        if estimated_savings < MICRO_MIN_SAVINGS {
            return Ok(None);
        }
        if cancel.is_cancelled() {
            return Err(VegaError::Cancelled);
        }
        let path = self.database_path.clone();
        let model = self.model.clone();
        let source_version = source.source_version;
        let call_ids = targets
            .iter()
            .map(|target| target.call_id.clone())
            .collect::<Vec<_>>();
        let source_fingerprint = tokio::task::spawn_blocking(move || {
            let _admission = admission;
            let store = Store::open(path).map_err(VegaError::Store)?;
            vega_store::context_micro::install(
                store.conn(),
                &source,
                &model,
                checkpoint.map(|checkpoint| checkpoint.id),
                &call_ids,
                now_ms(),
                || cancel.is_cancelled(),
            )
            .map_err(|error| match error {
                vega_store::context_compaction::ContextCheckpointError::Cancelled => {
                    VegaError::Cancelled
                }
                vega_store::context_compaction::ContextCheckpointError::Store(error) => {
                    VegaError::Store(error)
                }
                _ => context_error(ContextRuntimeError::SourceChanged),
            })
        })
        .await
        .map_err(|_| context_error(ContextRuntimeError::SourceChanged))??;
        Ok(Some(MicroCompactionResult {
            source_version,
            source_fingerprint,
            targets,
            estimated_savings,
        }))
    }
}
