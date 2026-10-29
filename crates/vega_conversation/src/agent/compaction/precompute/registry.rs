#![allow(missing_docs)]

use super::{ContextPrecomputeWorker, PreparedContextPrecompute, context_error};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use vega_runtime::{ContextRuntimeError, VegaError};
use vega_store::thread_admission::{DatabaseIdentity, ThreadAdmission};

#[derive(Clone, Default)]
pub struct ContextPrecomputeRegistry {
    inner: Arc<Mutex<RegistryState>>,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct Key {
    database: DatabaseIdentity,
    thread: String,
}

#[derive(Clone, PartialEq, Eq, Hash)]
enum Scope {
    Database(DatabaseIdentity),
    Thread(Key),
}

impl Scope {
    fn contains(&self, key: &Key) -> bool {
        match self {
            Self::Database(database) => *database == key.database,
            Self::Thread(thread) => thread == key,
        }
    }
}

#[derive(Default)]
struct RegistryState {
    closed: bool,
    entries: HashMap<Key, Entry>,
    pauses: HashMap<Scope, HashSet<ulid::Ulid>>,
}

impl Drop for RegistryState {
    fn drop(&mut self) {
        for entry in self.entries.values() {
            entry.epoch.cancel();
            if let Some(flight) = &entry.pending {
                flight.cancel.cancel();
            }
        }
    }
}

#[derive(Default)]
struct Entry {
    epoch: Arc<CancellationToken>,
    failures: u8,
    pending: Option<Arc<Flight>>,
}

struct Flight {
    key: Key,
    epoch: Arc<CancellationToken>,
    generation: String,
    cancel: CancellationToken,
    owner: Option<Arc<CancellationToken>>,
    state: Mutex<FlightState>,
}

#[derive(Default)]
struct FlightState {
    worker: Option<ContextPrecomputeWorker>,
    outcome: Option<Result<(), Arc<VegaError>>>,
}

impl Flight {
    fn poll(&self) -> Result<Option<Result<(), Arc<VegaError>>>, VegaError> {
        let mut state = self.state.lock().map_err(|_| unavailable())?;
        if state.outcome.is_none()
            && let Some(worker) = state.worker.as_mut()
            && let Some(outcome) = worker.take_finished()
        {
            state.outcome = Some(outcome.map_err(Arc::new));
            state.worker.take();
        }
        Ok(state.outcome.clone())
    }
}

#[derive(Clone)]
pub struct ContextPrecomputeLease {
    registry: ContextPrecomputeRegistry,
    key: Key,
    epoch: Arc<CancellationToken>,
    owner: Option<Arc<CancellationToken>>,
}

#[derive(Clone)]
pub struct ContextPrecomputeOwner {
    registry: ContextPrecomputeRegistry,
    cancel: Arc<CancellationToken>,
}

#[derive(Clone)]
pub struct ContextPrecomputeTask {
    registry: ContextPrecomputeRegistry,
    flight: Arc<Flight>,
}

#[derive(Clone)]
pub struct ContextPrecomputeDrain {
    registry: ContextPrecomputeRegistry,
    flights: Vec<Arc<Flight>>,
}

pub struct ContextPrecomputePause {
    registry: ContextPrecomputeRegistry,
    scope: Scope,
    token: ulid::Ulid,
    drain: ContextPrecomputeDrain,
}

fn unavailable() -> VegaError {
    context_error(ContextRuntimeError::SourceChanged)
}

fn counted_failure(error: &VegaError) -> bool {
    !matches!(
        error,
        VegaError::Cancelled
            | VegaError::Context(
                ContextRuntimeError::NoCompactablePrefix | ContextRuntimeError::SourceChanged
            )
    )
}

impl ContextPrecomputeRegistry {
    pub fn global() -> Self {
        static REGISTRY: OnceLock<ContextPrecomputeRegistry> = OnceLock::new();
        REGISTRY.get_or_init(Self::default).clone()
    }

    pub fn owner(&self) -> ContextPrecomputeOwner {
        ContextPrecomputeOwner {
            registry: self.clone(),
            cancel: Arc::new(CancellationToken::new()),
        }
    }

    pub(crate) fn owner_for_execution(&self, cancel: &CancellationToken) -> ContextPrecomputeOwner {
        ContextPrecomputeOwner {
            registry: self.clone(),
            cancel: Arc::new(cancel.child_token()),
        }
    }

    pub fn bind(
        &self,
        database: DatabaseIdentity,
        thread: &str,
        admission: &ThreadAdmission,
    ) -> Result<Option<ContextPrecomputeLease>, VegaError> {
        self.bind_owned(database, thread, admission, None)
    }

    fn bind_owned(
        &self,
        database: DatabaseIdentity,
        thread: &str,
        admission: &ThreadAdmission,
        owner: Option<Arc<CancellationToken>>,
    ) -> Result<Option<ContextPrecomputeLease>, VegaError> {
        if !admission.verifies(database, thread) {
            return Err(unavailable());
        }
        let key = Key {
            database,
            thread: thread.to_owned(),
        };
        let mut state = self.inner.lock().map_err(|_| unavailable())?;
        if state.closed
            || owner.as_ref().is_some_and(|owner| owner.is_cancelled())
            || state.pauses.keys().any(|scope| scope.contains(&key))
        {
            return Ok(None);
        }
        state.entries.retain(|_, entry| {
            entry.pending.is_some() || entry.failures > 0 || Arc::strong_count(&entry.epoch) > 1
        });
        let entry = state.entries.entry(key.clone()).or_default();
        Ok(Some(ContextPrecomputeLease {
            registry: self.clone(),
            key,
            epoch: entry.epoch.clone(),
            owner,
        }))
    }

    pub fn pending(
        &self,
        database: DatabaseIdentity,
        thread: &str,
        admission: &ThreadAdmission,
    ) -> Result<Option<ContextPrecomputeTask>, VegaError> {
        if !admission.verifies(database, thread) {
            return Err(unavailable());
        }
        let key = Key {
            database,
            thread: thread.to_owned(),
        };
        self.refresh(&key)?;
        let state = self.inner.lock().map_err(|_| unavailable())?;
        Ok(state
            .entries
            .get(&key)
            .and_then(|entry| entry.pending.clone())
            .map(|flight| ContextPrecomputeTask {
                registry: self.clone(),
                flight,
            }))
    }

    pub fn invalidate(
        &self,
        database: DatabaseIdentity,
        thread: &str,
    ) -> Result<ContextPrecomputeDrain, VegaError> {
        let scope = Scope::Thread(Key {
            database,
            thread: thread.to_owned(),
        });
        let mut state = self.inner.lock().map_err(|_| unavailable())?;
        Ok(self.invalidate_matching(&mut state, |key| scope.contains(key)))
    }

    pub fn pause_database(
        &self,
        database: DatabaseIdentity,
    ) -> Result<ContextPrecomputePause, VegaError> {
        self.pause(Scope::Database(database))
    }

    pub fn pause_thread(
        &self,
        database: DatabaseIdentity,
        thread: &str,
    ) -> Result<ContextPrecomputePause, VegaError> {
        self.pause(Scope::Thread(Key {
            database,
            thread: thread.to_owned(),
        }))
    }

    fn pause(&self, scope: Scope) -> Result<ContextPrecomputePause, VegaError> {
        let mut state = self.inner.lock().map_err(|_| unavailable())?;
        let token = ulid::Ulid::generate();
        state.pauses.entry(scope.clone()).or_default().insert(token);
        let drain = self.invalidate_matching(&mut state, |key| scope.contains(key));
        Ok(ContextPrecomputePause {
            registry: self.clone(),
            scope,
            token,
            drain,
        })
    }

    pub fn close(&self) -> Result<ContextPrecomputeDrain, VegaError> {
        let mut state = self.inner.lock().map_err(|_| unavailable())?;
        state.closed = true;
        Ok(self.invalidate_matching(&mut state, |_| true))
    }

    fn invalidate_matching(
        &self,
        state: &mut RegistryState,
        includes: impl Fn(&Key) -> bool,
    ) -> ContextPrecomputeDrain {
        let mut flights = Vec::new();
        for (key, entry) in &mut state.entries {
            if !includes(key) {
                continue;
            }
            entry.epoch.cancel();
            entry.epoch = Arc::new(CancellationToken::new());
            entry.failures = 0;
            if let Some(flight) = &entry.pending {
                flight.cancel.cancel();
                flights.push(flight.clone());
            }
        }
        ContextPrecomputeDrain {
            registry: self.clone(),
            flights,
        }
    }

    fn observe(
        &self,
        flight: &Arc<Flight>,
    ) -> Result<Option<Result<(), Arc<VegaError>>>, VegaError> {
        let outcome = flight.poll()?;
        if let Some(outcome) = &outcome {
            let mut state = self.inner.lock().map_err(|_| unavailable())?;
            if let Some(entry) = state.entries.get_mut(&flight.key)
                && entry
                    .pending
                    .as_ref()
                    .is_some_and(|pending| Arc::ptr_eq(pending, flight))
            {
                if Arc::ptr_eq(&entry.epoch, &flight.epoch) {
                    match outcome {
                        Ok(()) => entry.failures = 0,
                        Err(error) if counted_failure(error) => {
                            entry.failures = entry.failures.saturating_add(1);
                        }
                        Err(_) => {}
                    }
                }
                entry.pending = None;
            }
        }
        Ok(outcome)
    }

    fn refresh(&self, key: &Key) -> Result<(), VegaError> {
        let flight = self
            .inner
            .lock()
            .map_err(|_| unavailable())?
            .entries
            .get(key)
            .and_then(|entry| entry.pending.clone());
        if let Some(flight) = flight {
            self.observe(&flight)?;
        }
        Ok(())
    }
}

impl ContextPrecomputeLease {
    pub fn is_current(&self) -> Result<bool, VegaError> {
        let state = self.registry.inner.lock().map_err(|_| unavailable())?;
        Ok(!state.closed
            && !self.epoch.is_cancelled()
            && !self
                .owner
                .as_ref()
                .is_some_and(|owner| owner.is_cancelled())
            && !state.pauses.keys().any(|scope| scope.contains(&self.key))
            && state
                .entries
                .get(&self.key)
                .is_some_and(|entry| Arc::ptr_eq(&entry.epoch, &self.epoch)))
    }

    pub fn verifies(
        &self,
        database: DatabaseIdentity,
        thread: &str,
        admission: &ThreadAdmission,
    ) -> Result<bool, VegaError> {
        Ok(self.key.database == database
            && self.key.thread == thread
            && admission.verifies(database, thread)
            && self.is_current()?)
    }

    pub fn can_launch(&self) -> Result<bool, VegaError> {
        self.registry.refresh(&self.key)?;
        if !self.is_current()? {
            return Ok(false);
        }
        let state = self.registry.inner.lock().map_err(|_| unavailable())?;
        Ok(state.entries.get(&self.key).is_some_and(|entry| {
            Arc::ptr_eq(&entry.epoch, &self.epoch) && entry.pending.is_none() && entry.failures < 3
        }))
    }

    pub fn try_launch(
        &self,
        prepared: PreparedContextPrecompute,
    ) -> Result<Option<ContextPrecomputeTask>, VegaError> {
        if prepared.accounting.database_identity != self.key.database
            || prepared.accounting.thread_id != self.key.thread
        {
            return Err(unavailable());
        }
        self.registry.refresh(&self.key)?;
        let flight = {
            let mut state = self.registry.inner.lock().map_err(|_| unavailable())?;
            if state.closed
                || self.epoch.is_cancelled()
                || self
                    .owner
                    .as_ref()
                    .is_some_and(|owner| owner.is_cancelled())
                || state.pauses.keys().any(|scope| scope.contains(&self.key))
            {
                return Ok(None);
            }
            let Some(entry) = state.entries.get_mut(&self.key) else {
                return Ok(None);
            };
            if !Arc::ptr_eq(&entry.epoch, &self.epoch)
                || entry.pending.is_some()
                || entry.failures >= 3
            {
                return Ok(None);
            }
            let flight = Arc::new(Flight {
                key: self.key.clone(),
                epoch: self.epoch.clone(),
                generation: prepared.generation().to_owned(),
                cancel: self
                    .owner
                    .as_ref()
                    .map_or_else(|| self.epoch.child_token(), |owner| owner.child_token()),
                owner: self.owner.clone(),
                state: Mutex::new(FlightState::default()),
            });
            entry.pending = Some(flight.clone());
            flight
        };
        let launched = prepared.launch_with_cancel(flight.cancel.clone());
        {
            let mut state = flight.state.lock().map_err(|_| unavailable())?;
            match launched {
                Ok(worker) => state.worker = Some(worker),
                Err(error) => state.outcome = Some(Err(Arc::new(error))),
            }
        }
        Ok(Some(ContextPrecomputeTask {
            registry: self.registry.clone(),
            flight,
        }))
    }
}

impl ContextPrecomputeOwner {
    pub fn bind(
        &self,
        database: DatabaseIdentity,
        thread: &str,
        admission: &ThreadAdmission,
    ) -> Result<Option<ContextPrecomputeLease>, VegaError> {
        self.registry
            .bind_owned(database, thread, admission, Some(self.cancel.clone()))
    }

    pub fn close(&self) -> Result<ContextPrecomputeDrain, VegaError> {
        let state = self.registry.inner.lock().map_err(|_| unavailable())?;
        self.cancel.cancel();
        let flights = state
            .entries
            .values()
            .filter_map(|entry| {
                entry
                    .pending
                    .as_ref()
                    .filter(|flight| {
                        flight
                            .owner
                            .as_ref()
                            .is_some_and(|owner| Arc::ptr_eq(owner, &self.cancel))
                    })
                    .map(|flight| {
                        flight.cancel.cancel();
                        flight.clone()
                    })
            })
            .collect();
        Ok(ContextPrecomputeDrain {
            registry: self.registry.clone(),
            flights,
        })
    }
}

impl ContextPrecomputeTask {
    pub fn generation(&self) -> &str {
        &self.flight.generation
    }

    pub async fn wait_for_exit(&self) -> Result<(), VegaError> {
        while self.registry.observe(&self.flight)?.is_none() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        Ok(())
    }

    pub async fn wait(&self) -> Result<(), Arc<VegaError>> {
        loop {
            if let Some(outcome) = self.registry.observe(&self.flight).map_err(Arc::new)? {
                return outcome;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

impl ContextPrecomputeDrain {
    pub fn is_finished(&self) -> Result<bool, VegaError> {
        let mut finished = true;
        for flight in &self.flights {
            finished &= self.registry.observe(flight)?.is_some();
        }
        Ok(finished)
    }

    pub async fn wait(&self) -> Result<(), VegaError> {
        while !self.is_finished()? {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        Ok(())
    }

    pub fn wait_blocking(&self) -> Result<(), VegaError> {
        while !self.is_finished()? {
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    }
}

impl ContextPrecomputePause {
    pub fn drain(&self) -> &ContextPrecomputeDrain {
        &self.drain
    }
}

impl Drop for ContextPrecomputePause {
    fn drop(&mut self) {
        if let Ok(mut state) = self.registry.inner.lock()
            && let Some(tokens) = state.pauses.get_mut(&self.scope)
        {
            tokens.remove(&self.token);
            if tokens.is_empty() {
                state.pauses.remove(&self.scope);
            }
        }
    }
}
