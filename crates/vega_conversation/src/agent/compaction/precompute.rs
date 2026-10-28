use super::*;
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use vega_store::context_precompute::{accounting, ready};

pub(super) struct CapturedPrecompute {
    pub fence: vega_store::context_precompute::ContextAppendFence,
    pub dependencies: String,
    pub expected_generation: Option<String>,
    pub database_identity: vega_store::thread_admission::DatabaseIdentity,
}

#[derive(Clone)]
pub(super) struct StageAccounting {
    database_path: PathBuf,
    database_identity: vega_store::thread_admission::DatabaseIdentity,
    thread_id: String,
    model: String,
    generation: String,
    stages: Arc<AtomicU32>,
    complete: Arc<AtomicBool>,
}

impl StageAccounting {
    fn open(&self) -> Result<Store, VegaError> {
        let store = Store::open(&self.database_path).map_err(VegaError::Store)?;
        if store.database_identity() != self.database_identity {
            return Err(context_error(ContextRuntimeError::SourceChanged));
        }
        Ok(store)
    }

    pub(super) async fn record(
        &self,
        stage: usize,
        metrics: &SummaryDiagnosticMetrics,
    ) -> Result<(), VegaError> {
        if !metrics.request_may_have_been_submitted {
            return Ok(());
        }
        let stage = u32::try_from(stage)
            .map_err(|_| context_error(ContextRuntimeError::InvalidProjection))?;
        let reported = metrics
            .usage
            .as_ref()
            .map(|usage| accounting::ReportedUsage {
                input_tokens: usage.usage.input,
                output_tokens: usage.usage.output,
                cache_read_tokens: usage.usage.cache_read,
                cache_write_tokens: usage.usage.cache_write,
                cost_microcents: usage.cost_microcents,
                created_at: now_ms(),
                pricing_version: usage
                    .pricing
                    .as_ref()
                    .map(|pricing| pricing.version.clone()),
                pricing_profile: usage
                    .pricing
                    .as_ref()
                    .map(|pricing| pricing.profile.clone()),
                call_started_at: usage
                    .pricing
                    .as_ref()
                    .map(|pricing| pricing.call_started_at),
            });
        let this = self.clone();
        tokio::task::spawn_blocking(move || {
            let store = this.open()?;
            accounting::record_stage(
                store.conn(),
                &this.generation,
                &this.thread_id,
                &this.model,
                stage,
                reported.as_ref(),
            )
            .map_err(store_error)?;
            if reported.is_none() {
                this.complete.store(false, Ordering::Release);
            }
            this.stages.store(stage, Ordering::Release);
            Ok(())
        })
        .await
        .map_err(|_| context_error(ContextRuntimeError::SourceChanged))?
    }

    async fn finish(&self) -> Result<(), VegaError> {
        let this = self.clone();
        tokio::task::spawn_blocking(move || {
            let store = this.open()?;
            accounting::finish(
                store.conn(),
                &this.generation,
                &this.thread_id,
                &this.model,
                this.stages.load(Ordering::Acquire),
                this.complete.load(Ordering::Acquire),
            )
            .map_err(store_error)
        })
        .await
        .map_err(|_| context_error(ContextRuntimeError::SourceChanged))?
    }

    async fn retire(&self) -> Result<(), VegaError> {
        let this = self.clone();
        tokio::task::spawn_blocking(move || {
            let store = this.open()?;
            ready::retire(store.conn(), &this.thread_id, &this.generation)
                .map(|_| ())
                .map_err(store_error)
        })
        .await
        .map_err(|_| context_error(ContextRuntimeError::SourceChanged))?
    }
}

#[allow(missing_docs)]
pub struct PreparedContextPrecompute {
    provider: vega_runtime::CompactionProviderFactory,
    accounting: StageAccounting,
    request: ContextCompactionRequest,
    captured: CapturedCompaction,
    reasoning: Option<FrozenReasoning>,
    pricing_catalog: Option<vega_token::PricingCatalog>,
    credential_reader: Option<vega_runtime::CredentialReader>,
    credential_exceptions: Option<vega_runtime::CredentialExceptionFilter>,
    summary_variant: SummaryVariant,
    summary_requirements: Option<String>,
    preferences_digest: String,
}

#[allow(missing_docs)]
pub struct ContextPrecomputeWorker {
    generation: String,
    cancel: CancellationToken,
    completion: tokio::sync::oneshot::Receiver<Result<(), VegaError>>,
}

impl Drop for ContextPrecomputeWorker {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

#[allow(missing_docs)]
impl ContextPrecomputeWorker {
    pub fn generation(&self) -> &str {
        &self.generation
    }

    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    pub async fn wait(&mut self) -> Result<(), VegaError> {
        (&mut self.completion)
            .await
            .map_err(|_| context_error(ContextRuntimeError::SourceChanged))?
    }
}

#[allow(missing_docs)]
impl PreparedContextPrecompute {
    pub fn generation(&self) -> &str {
        &self.accounting.generation
    }

    pub fn launch(self) -> Result<ContextPrecomputeWorker, VegaError> {
        let generation = self.accounting.generation.clone();
        let cancel = CancellationToken::new();
        let worker_cancel = cancel.clone();
        let (sender, completion) = tokio::sync::oneshot::channel();
        std::thread::Builder::new()
            .name("vega-context-precompute".into())
            .spawn(move || {
                let result = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => {
                        let result = runtime.block_on(self.generate(worker_cancel));
                        drop(runtime);
                        result
                    }
                    Err(_) => Err(context_error(ContextRuntimeError::SourceChanged)),
                };
                let _ = sender.send(result);
            })
            .map_err(|_| context_error(ContextRuntimeError::SourceChanged))?;
        Ok(ContextPrecomputeWorker {
            generation,
            cancel,
            completion,
        })
    }

    pub(super) async fn generate(mut self, cancel: CancellationToken) -> Result<(), VegaError> {
        let captured = self
            .captured
            .precompute
            .take()
            .ok_or_else(|| context_error(ContextRuntimeError::InvalidProjection))?;
        let accounting = self.accounting.clone();
        let expected_generation = captured.expected_generation.clone();
        let begin = accounting.clone();
        if cancel.is_cancelled() {
            return Err(VegaError::Cancelled);
        }
        tokio::task::spawn_blocking(move || {
            let store = begin.open()?;
            ready::begin_generation(
                store.conn(),
                &begin.generation,
                &begin.thread_id,
                &begin.model,
                expected_generation.as_deref(),
            )
            .map_err(store_error)
        })
        .await
        .map_err(|_| context_error(ContextRuntimeError::SourceChanged))??;
        let result = async {
            if cancel.is_cancelled() {
                return Err(VegaError::Cancelled);
            }
            let provider = (self.provider)()?;
            let mut hook = ConversationCompactionHook::new(
                provider.as_ref(),
                accounting.database_path.clone(),
                &accounting.thread_id,
                &accounting.model,
                self.reasoning,
                self.pricing_catalog,
            );
            hook.credential_reader = self.credential_reader;
            hook.credential_exceptions = self.credential_exceptions;
            hook.summary_variant = self.summary_variant;
            hook.summary_requirements = self.summary_requirements;
            hook.stage_accounting = Some(accounting.clone());
            let pre_tokens = self.request.estimate.input_tokens;
            let generated = hook
                .generate_captured(
                    self.request,
                    cancel.clone(),
                    self.captured,
                    tokio::time::Instant::now() + SUMMARY_OPERATION_TIMEOUT,
                    true,
                )
                .await
                .map_err(|failure| *failure.error)?;
            Ok(ready::ReadyCompaction {
                schema: 1,
                generation: accounting.generation.clone(),
                checkpoint: generated.checkpoint,
                source: captured.fence,
                summary_dependencies: captured.dependencies,
                pre_tokens,
                preferences_digest: self.preferences_digest,
                stage_count: accounting.stages.load(Ordering::Acquire),
                usage_complete: accounting.complete.load(Ordering::Acquire),
            })
        }
        .await;
        let settled = accounting.finish().await;
        let result = match (result, settled) {
            (_, Err(error)) | (Err(error), Ok(())) => Err(error),
            (Ok(candidate), Ok(())) => {
                let publish = accounting.clone();
                tokio::task::spawn_blocking(move || {
                    let store = publish.open()?;
                    if cancel.is_cancelled() {
                        return Err(VegaError::Cancelled);
                    }
                    ready::publish_with_guard(store.conn(), &candidate, now_ms(), || {
                        !cancel.is_cancelled()
                    })
                    .map_err(store_error)
                })
                .await
                .map_err(|_| context_error(ContextRuntimeError::SourceChanged))?
            }
        };
        if result.is_err() {
            accounting.retire().await?;
        }
        result
    }
}

impl ConversationCompactionHook<'_> {
    pub(super) fn precompute_preferences_digest(&self) -> String {
        let mut preferences = Sha256::new();
        preferences.update(b"vega-precompute-preferences-v1");
        preferences.update(self.summary_variant.instruction().as_bytes());
        preferences.update([0]);
        preferences.update(
            self.summary_requirements
                .as_deref()
                .unwrap_or_default()
                .as_bytes(),
        );
        format!("{:x}", preferences.finalize())
    }

    #[allow(missing_docs)]
    pub async fn prepare_precompute(
        &self,
        request: ContextCompactionRequest,
        cancel: CancellationToken,
    ) -> Result<Option<crate::types::PreparedContextPrecompute>, VegaError> {
        if self.partial_selection.is_some()
            || request.trigger != ContextCompactionTrigger::Threshold
        {
            return Ok(None);
        }
        let Some(provider) = self.provider.compaction_provider_factory() else {
            return Ok(None);
        };
        let (captured, guard) = self
            .capture_compaction(&request, &cancel, true)
            .await
            .map_err(|failure| *failure.error)?;
        validate_request_source(&captured.source, captured.checkpoint.as_ref(), &request)
            .map_err(|failure| *failure.error)?;
        reject_unprojected_shell_credentials(
            &captured.source,
            captured.checkpoint.as_ref(),
            request.source_owner_id.as_deref(),
            self.credential_reader.as_ref(),
            self.credential_exceptions.as_ref(),
        )?;
        let mut planner = reactive::ResponsePlanner::new(
            &captured.source,
            captured.checkpoint.as_ref(),
            request.source_owner_id.as_deref(),
        )
        .map_err(|failure| *failure.error)?;
        planner
            .start_reactive(ContextCompactionTrigger::Threshold)
            .map_err(|failure| *failure.error)?;
        let database_identity = captured
            .precompute
            .as_ref()
            .ok_or_else(|| context_error(ContextRuntimeError::InvalidProjection))?
            .database_identity;
        drop(guard);
        if cancel.is_cancelled() {
            return Err(VegaError::Cancelled);
        }
        Ok(Some(PreparedContextPrecompute {
            provider,
            accounting: StageAccounting {
                database_path: self.database_path.clone(),
                database_identity,
                thread_id: self.thread_id.clone(),
                model: self.model.clone(),
                generation: ulid::Ulid::generate().to_string(),
                stages: Arc::new(AtomicU32::new(0)),
                complete: Arc::new(AtomicBool::new(true)),
            },
            request,
            captured,
            reasoning: self.reasoning.clone(),
            pricing_catalog: self.pricing_catalog.clone(),
            credential_reader: self.credential_reader.clone(),
            credential_exceptions: self.credential_exceptions.clone(),
            summary_variant: self.summary_variant,
            summary_requirements: self.summary_requirements.clone(),
            preferences_digest: self.precompute_preferences_digest(),
        }))
    }
}

pub(super) fn store_error(
    error: vega_store::context_compaction::ContextCheckpointError,
) -> VegaError {
    use vega_store::context_compaction::ContextCheckpointError;
    match error {
        ContextCheckpointError::Store(error) => VegaError::Store(error),
        ContextCheckpointError::Cancelled => VegaError::Cancelled,
        ContextCheckpointError::Stale => context_error(ContextRuntimeError::SourceChanged),
        ContextCheckpointError::Invalid | ContextCheckpointError::IncompleteCoverage => {
            context_error(ContextRuntimeError::InvalidProjection)
        }
    }
}
