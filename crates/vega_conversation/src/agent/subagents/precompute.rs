use super::*;
use crate::types::{ContextPrecomputeLease, ContextPrecomputeOwner, ContextPrecomputeRegistry};

#[cfg(test)]
#[path = "precompute_tests.rs"]
mod tests;

pub(super) struct ChildPrecompute {
    owner: ContextPrecomputeOwner,
    registry: ContextPrecomputeRegistry,
    identity: vega_store::thread_admission::DatabaseIdentity,
    thread: String,
    database: PathBuf,
}

impl Drop for ChildPrecompute {
    fn drop(&mut self) {
        if self.owner.close().is_err() {
            tracing::error!("child context precompute cancellation could not be confirmed");
        }
    }
}

impl ChildPrecompute {
    pub(super) fn begin(
        store: &Store,
        database: PathBuf,
        thread: &str,
        admission: &vega_store::thread_admission::ThreadAdmission,
        cancel: &CancellationToken,
    ) -> Result<(Self, Option<ContextPrecomputeLease>), VegaError> {
        if cancel.is_cancelled() {
            return Err(VegaError::Cancelled);
        }
        let registry = ContextPrecomputeRegistry::global();
        let owner = registry.owner_for_execution(cancel);
        let identity = store.database_identity();
        let lease = owner.bind(identity, thread, admission)?;
        Ok((
            Self {
                owner,
                registry,
                identity,
                thread: thread.into(),
                database,
            },
            lease,
        ))
    }

    pub(super) async fn finish(
        self,
        admission: &vega_store::thread_admission::ThreadAdmission,
    ) -> Result<(), VegaError> {
        self.owner.close()?;
        let pause = self.registry.pause_thread(self.identity, &self.thread)?;
        pause.drain().wait().await?;
        let database = self.database.clone();
        let thread = self.thread.clone();
        let identity = self.identity;
        let admission = admission.clone();
        tokio::task::spawn_blocking(move || {
            let _pause = pause;
            let store = Store::open(database).map_err(VegaError::Store)?;
            if store.database_identity() != identity || !admission.verifies(identity, &thread) {
                return Err(failure("child context cleanup authority changed"));
            }
            let transaction = vega_store::write_coordination::WriteTransaction::new(
                store.conn(),
                vega_store::write_coordination::WriteCategory::ContextCompaction,
            )
            .map_err(VegaError::Store)?;
            let generation =
                vega_store::context_precompute::ready::current_generation(&transaction, &thread)
                    .map_err(|_| failure("child context cleanup unavailable"))?;
            if let Some(generation) = generation {
                vega_store::context_precompute::ready::retire_in_transaction(
                    &transaction,
                    &thread,
                    &generation,
                )
                .map_err(|_| failure("child context cleanup unavailable"))?;
            }
            transaction.commit().map_err(VegaError::Store)
        })
        .await
        .map_err(|_| failure("child context cleanup unavailable"))?
    }
}
