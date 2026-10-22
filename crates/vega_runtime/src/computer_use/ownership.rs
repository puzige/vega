use super::{ComputerUseError as Error, probe::PreparationTransport};
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Default)]
pub(crate) struct OwnershipAuthority {
    pub(super) owner: Arc<Mutex<Option<String>>>,
    pending: Mutex<Vec<Arc<CleanupEntry>>>,
    drain: tokio::sync::Mutex<()>,
}

struct CleanupEntry {
    owner: Arc<Mutex<Option<String>>>,
    id: String,
    state: tokio::sync::Mutex<CleanupState>,
}

struct CleanupState {
    transport: Option<Box<dyn PreparationTransport + Send>>,
    outcome: Option<Result<(), Error>>,
    reported: bool,
}

impl CleanupEntry {
    async fn attempt(&self, state: &mut CleanupState) -> Result<(), Error> {
        state.outcome = None;
        state.reported = false;
        let result = match state.transport.as_mut() {
            Some(transport) => transport.cleanup().await,
            None => Ok(()),
        };
        if result.is_ok() {
            state.transport = None;
            let mut owner = self.owner.lock().unwrap_or_else(|error| error.into_inner());
            if owner.as_ref() == Some(&self.id) {
                *owner = None;
            }
        }
        state.outcome = Some(result);
        result
    }
}

impl OwnershipAuthority {
    pub(super) fn shared() -> Arc<Self> {
        static AUTHORITY: OnceLock<Arc<OwnershipAuthority>> = OnceLock::new();
        AUTHORITY.get_or_init(|| Arc::new(Self::default())).clone()
    }

    pub(super) fn acquire(&self, id: &str) -> Result<(), Error> {
        let mut owner = self.owner.lock().map_err(|_| Error::Unavailable)?;
        if owner.is_some() {
            return Err(Error::Busy);
        }
        *owner = Some(id.to_owned());
        Ok(())
    }

    pub(super) fn release(&self, id: &str) {
        let mut owner = self.owner.lock().unwrap_or_else(|error| error.into_inner());
        if owner.as_deref() == Some(id) {
            *owner = None;
        }
    }

    pub(super) fn retain<T: PreparationTransport + Send + 'static>(
        &self,
        id: String,
        transport: T,
    ) {
        let entry = Arc::new(CleanupEntry {
            owner: self.owner.clone(),
            id,
            state: tokio::sync::Mutex::new(CleanupState {
                transport: Some(Box::new(transport)),
                outcome: None,
                reported: false,
            }),
        });
        self.pending
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(entry.clone());
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let mut state = entry.state.lock().await;
                if state.outcome.is_none() {
                    let _ = entry.attempt(&mut state).await;
                }
            });
        }
    }

    pub(crate) async fn cleanup_pending(&self) -> Result<(), Error> {
        let _drain = self.drain.lock().await;
        let entries = self
            .pending
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        for entry in entries {
            let mut state = entry.state.lock().await;
            let result = match state.outcome {
                Some(Ok(())) => Ok(()),
                Some(Err(error)) if !state.reported => Err(error),
                _ => entry.attempt(&mut state).await,
            };
            state.reported = true;
            result?;
            self.pending
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .retain(|pending| !Arc::ptr_eq(pending, &entry));
        }
        Ok(())
    }
}

impl Drop for OwnershipAuthority {
    fn drop(&mut self) {
        let entries = std::mem::take(
            self.pending
                .get_mut()
                .unwrap_or_else(|error| error.into_inner()),
        );
        if !entries.is_empty() {
            Self::shared()
                .pending
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .extend(entries);
        }
    }
}
