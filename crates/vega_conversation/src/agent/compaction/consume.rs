use super::*;
use vega_store::context_compaction::ContextCheckpointError;
use vega_store::context_precompute::consume;

impl ConversationCompactionHook<'_> {
    pub(super) async fn consume_precompute(
        &self,
        request: &ContextCompactionRequest,
        replay_frame: Option<&vega_runtime::ReplayFrame>,
        cancel: &CancellationToken,
    ) -> Result<Option<ContextCompactionResult>, VegaError> {
        if cancel.is_cancelled() {
            return Err(VegaError::Cancelled);
        }
        if self.partial_selection.is_some()
            || (request.source_owner_id.is_some() && replay_frame.is_none())
            || matches!(
                request.trigger,
                ContextCompactionTrigger::InputOverflow { deeper: true, .. }
            )
        {
            return Ok(None);
        }
        let deadline = tokio::time::Instant::now() + SUMMARY_OPERATION_TIMEOUT;
        let path = self.database_path.clone();
        let thread_id = self.thread_id.clone();
        let model = self.model.clone();
        let admission = self.admission.clone();
        let owner = request.source_owner_id.clone();
        let read_cancel = cancel.clone();
        let captured = tokio::task::spawn_blocking(move || {
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
            let (model_matches, pending): (bool, bool) = transaction
                .query_row(
                    "SELECT model=?2,pending_model IS NOT NULL FROM threads WHERE id=?1",
                    (&thread_id, &model),
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(VegaError::Store)?;
            if !model_matches {
                return Err(context_error(ContextRuntimeError::SourceChanged));
            }
            if pending {
                return Ok(None);
            }
            let prepared = match consume::prepare_in_transaction(
                &transaction,
                &thread_id,
                owner.as_deref(),
                now_ms(),
            ) {
                Ok(Some(prepared)) => prepared,
                Ok(None)
                | Err(
                    ContextCheckpointError::Stale
                    | ContextCheckpointError::Invalid
                    | ContextCheckpointError::IncompleteCoverage,
                ) => return Ok(None),
                Err(error) => return Err(precompute::store_error(error)),
            };
            let previous = latest_checkpoint_in_transaction(&transaction, &thread_id, &model)
                .map_err(VegaError::Store)?;
            transaction.commit().map_err(VegaError::Store)?;
            Ok::<_, VegaError>(Some((prepared, previous, admission)))
        })
        .await
        .map_err(|_| context_error(ContextRuntimeError::SourceChanged))??;
        let Some((prepared, previous, admission)) = captured else {
            return Ok(None);
        };
        if cancel.is_cancelled() {
            return Err(VegaError::Cancelled);
        }
        validate_request_source(prepared.source(), previous.as_ref(), request)
            .map_err(|failure| *failure.error)?;
        if prepared.candidate().preferences_digest != self.precompute_preferences_digest() {
            return Ok(None);
        }
        reject_unprojected_shell_credentials(
            prepared.source(),
            previous.as_ref(),
            request.source_owner_id.as_deref(),
            self.credential_reader.as_ref(),
            self.credential_exceptions.as_ref(),
        )?;
        let checkpoint = prepared
            .checkpoint(
                &self.model,
                request.source_owner_id.as_deref(),
                CONTEXT_ESTIMATOR_VERSION,
                now_ms(),
            )
            .map_err(precompute::store_error)?;
        let plan = checkpoint
            .projection_plan
            .as_ref()
            .ok_or_else(|| context_error(ContextRuntimeError::InvalidProjection))?;
        let live = if request.source_owner_id.is_some() {
            match super::super::pipeline::match_live_ranges(
                prepared.source(),
                plan,
                &request.messages,
            ) {
                Ok(live) => live,
                Err(_) => return Ok(None),
            }
        } else {
            Vec::new()
        };
        let mut references = previous
            .as_ref()
            .map(|previous| previous.referenced_summaries.clone())
            .unwrap_or_default();
        if let Some(previous) = &previous {
            references.insert(previous.id, previous.summary.clone());
        }
        let messages = super::super::pipeline::interpret_projection_with_live(
            prepared.source(),
            plan,
            &checkpoint.summary,
            &references,
            "__precompute_consumption__",
            &live,
        )
        .map_err(|_| context_error(ContextRuntimeError::InvalidProjection))?;
        let mut cold = messages.len();
        if let Some(first) = live.first() {
            cold = (0..messages.len())
                .find(|start| {
                    messages.get(*start..start.saturating_add(first.messages.len()))
                        == Some(first.messages)
                })
                .ok_or_else(|| context_error(ContextRuntimeError::InvalidProjection))?;
        }
        let interleaved = replay_frame.is_some_and(|frame| {
            frame.policy() == vega_runtime::ReasoningReplayPolicy::GlmInterleaved
        });
        let user = messages[cold..]
            .iter()
            .rposition(|message| message.role == ChatRole::User)
            .map(|index| cold + index);
        if interleaved && user.is_none() {
            cold = messages.len();
        }
        let result = ContextCompactionResult {
            messages,
            source_version: prepared.source().source_version,
            source_fingerprint: Some(prepared.source().fingerprint.clone()),
            replay_cold_prefix_messages: Some(cold),
            replay_interleaved_user_boundary: interleaved.then_some(user.unwrap_or(cold)),
            reactive_retained_groups: None,
            usages: Vec::new(),
            usage_complete: prepared.candidate().usage_complete,
            settled_usage: Some(if prepared.candidate().usage_complete {
                vega_runtime::ContextCompactionUsageState::Known {
                    priced: prepared.usage_priced(),
                }
            } else {
                vega_runtime::ContextCompactionUsageState::Unknown
            }),
        };
        let wire = result.wire_messages(request, replay_frame)?;
        let original_wire = vega_runtime::MicroCompactionRequest {
            context: request.clone(),
            replay_frame: replay_frame.cloned(),
        }
        .wire_messages();
        let original = estimate_wire_context(&original_wire, &request.tools)
            .map_err(|error| VegaError::Context(error.into()))?
            .input_tokens;
        let after = estimate_wire_context(&wire, &request.tools)
            .map_err(|error| VegaError::Context(error.into()))?
            .input_tokens;
        let threshold = request
            .budget
            .trigger_tokens()
            .map_err(|error| VegaError::Context(error.into()))?;
        if after >= original
            || after >= threshold
            || after > request.target_tokens
            || after > request.budget.input_budget()
        {
            return Ok(None);
        }
        let credential_request = ChatRequest {
            model: self.model.clone(),
            messages: wire,
            tools: request.tools.clone(),
            reasoning: self.reasoning.clone(),
            ..Default::default()
        };
        let empty_reader = || Ok(Vec::new());
        let reader = self.credential_reader.as_deref().unwrap_or(&empty_reader);
        super::super::credential_guard::reject_owner_secret_with_policy(
            &credential_request,
            reader,
            self.credential_exceptions.as_ref(),
        )?;
        if cancel.is_cancelled() {
            return Err(VegaError::Cancelled);
        }
        let path = self.database_path.clone();
        let install_cancel = cancel.clone();
        let install = tokio::task::spawn_blocking(move || {
            let store = Store::open(path).map_err(VegaError::Store)?;
            crate::thread_access::verify(&store, &checkpoint.thread_id, &admission)
                .map_err(|_| context_error(ContextRuntimeError::SourceChanged))?;
            consume::install_with_guard(store.conn(), &prepared, &checkpoint, now_ms(), || {
                install_cancel.is_cancelled() || tokio::time::Instant::now() >= deadline
            })
            .map_err(|error| match error {
                ContextCheckpointError::Cancelled if !install_cancel.is_cancelled() => {
                    context_error(ContextRuntimeError::SummaryTimedOut)
                }
                error => precompute::store_error(error),
            })
        })
        .await
        .map_err(|_| context_error(ContextRuntimeError::SourceChanged))??;
        if matches!(install, ContextCheckpointInstall::Stale) {
            return Err(context_error(ContextRuntimeError::SourceChanged));
        }
        Ok(Some(result))
    }
}
