use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

use vega_store::{DatabaseIdentity, Store};

pub fn shutdown_database_writers() -> Result<(), String> {
    crate::agent::persistence::database_writer::DatabaseWriter::shutdown_all()
}

mod sealed {
    pub(crate) trait Sealed {}
}

pub(crate) use sealed::Sealed;

pub(crate) trait DatabaseCommand: Sealed + Send + Sized + 'static {
    type Output: Send + 'static;
    fn apply(self, store: &Store) -> Self::Output;

    fn apply_with_admission(self, store: &Store, _remaining: Duration) -> Self::Output {
        self.apply(store)
    }
}

#[derive(Clone, Default)]
pub(crate) struct CommitCancellation {
    pub token: Option<CancellationToken>,
    pub submission: Option<crate::types::OrdinarySubmission>,
}

impl CommitCancellation {
    fn is_cancelled(&self) -> bool {
        self.token
            .as_ref()
            .is_some_and(CancellationToken::is_cancelled)
            || self.submission.as_ref().is_some_and(|submission| {
                let snapshot = submission.snapshot();
                snapshot.unavailable || snapshot.retired
            })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CommitError {
    CancelledBeforeEnqueue,
    CancelledBeforeStart,
    BudgetBeforeEnqueue,
    BudgetBeforeStart,
    Unavailable,
    IdentityChanged,
    OutcomeUnknown,
}

impl std::fmt::Display for CommitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::CancelledBeforeEnqueue => "database commit cancelled; not enqueued",
            Self::CancelledBeforeStart => {
                "database commit cancelled; queued command will not execute"
            }
            Self::BudgetBeforeEnqueue => "database admission budget exhausted; not enqueued",
            Self::BudgetBeforeStart => {
                "database admission budget exhausted; queued command will not execute"
            }
            Self::Unavailable => "database writer unavailable before enqueue",
            Self::IdentityChanged => "database identity changed before commit",
            Self::OutcomeUnknown => "database commit outcome unknown; reconcile before retry",
        })
    }
}

impl std::error::Error for CommitError {}

#[derive(Clone, Copy)]
enum AdmissionPhase {
    Waiting,
    Started,
    Rejected(CommitError),
}

pub(crate) struct CommitAdmission {
    cancellation: CommitCancellation,
    budget: Duration,
    started: OnceLock<Instant>,
    phase: Mutex<AdmissionPhase>,
}

impl CommitAdmission {
    fn new(cancellation: CommitCancellation, budget: Duration) -> Self {
        Self {
            cancellation,
            budget,
            started: OnceLock::new(),
            phase: Mutex::new(AdmissionPhase::Waiting),
        }
    }

    pub(crate) fn start_wait(&self) {
        self.started.get_or_init(Instant::now);
    }

    fn elapsed(&self) -> Duration {
        self.started.get().map(Instant::elapsed).unwrap_or_default()
    }

    pub(crate) fn rejection(&self, enqueued: bool) -> Option<CommitError> {
        let mut phase = self.phase.lock().unwrap_or_else(|error| error.into_inner());
        match *phase {
            AdmissionPhase::Started => None,
            AdmissionPhase::Rejected(error) => Some(error),
            AdmissionPhase::Waiting => {
                let error = if self.cancellation.is_cancelled() {
                    Some(if enqueued {
                        CommitError::CancelledBeforeStart
                    } else {
                        CommitError::CancelledBeforeEnqueue
                    })
                } else if self.elapsed() >= self.budget {
                    Some(if enqueued {
                        CommitError::BudgetBeforeStart
                    } else {
                        CommitError::BudgetBeforeEnqueue
                    })
                } else {
                    None
                };
                if let Some(error) = error {
                    *phase = AdmissionPhase::Rejected(error);
                }
                error
            }
        }
    }

    fn begin(&self) -> Result<Duration, CommitError> {
        let mut phase = self.phase.lock().unwrap_or_else(|error| error.into_inner());
        if let AdmissionPhase::Rejected(error) = *phase {
            return Err(error);
        }
        let remaining = self.budget.saturating_sub(self.elapsed());
        let error = if self.cancellation.is_cancelled() {
            Some(CommitError::CancelledBeforeStart)
        } else if remaining.is_zero() {
            Some(CommitError::BudgetBeforeStart)
        } else {
            None
        };
        if let Some(error) = error {
            *phase = AdmissionPhase::Rejected(error);
            return Err(error);
        }
        *phase = AdmissionPhase::Started;
        Ok(remaining)
    }
}

pub(crate) trait WriteRequest: Send {
    fn execute(self: Box<Self>, store: &Store);
    fn execute_terminal(self: Box<Self>, store: &Store) -> bool {
        self.execute(store);
        true
    }
}

struct Request<C: DatabaseCommand> {
    identity: DatabaseIdentity,
    command: C,
    reply: std::sync::mpsc::SyncSender<Result<C::Output, CommitError>>,
    admission: Option<Arc<CommitAdmission>>,
}

impl<C: DatabaseCommand> WriteRequest for Request<C> {
    fn execute(self: Box<Self>, store: &Store) {
        let Self { identity, command, reply, admission } = *self;
        let remaining = admission
            .as_ref()
            .map(|admission| admission.begin())
            .transpose();
        let result = remaining.and_then(|remaining| {
            store
                .verify_database_identity(identity)
                .map_err(|_| CommitError::IdentityChanged)?;
            Ok(match remaining {
                Some(remaining) => command.apply_with_admission(store, remaining),
                None => command.apply(store),
            })
        });
        let _ = reply.send(result);
    }
}

pub(crate) struct NecessaryLease {
    writer: std::sync::Arc<crate::agent::persistence::database_writer::DatabaseWriter>,
    sender: Option<std::sync::mpsc::SyncSender<Box<dyn WriteRequest>>>,
    capacity: Option<tokio::sync::OwnedSemaphorePermit>,
}

impl NecessaryLease {
    pub(crate) fn new(
        writer: std::sync::Arc<crate::agent::persistence::database_writer::DatabaseWriter>,
        sender: std::sync::mpsc::SyncSender<Box<dyn WriteRequest>>,
        capacity: tokio::sync::OwnedSemaphorePermit,
    ) -> Self {
        Self {
            writer,
            sender: Some(sender),
            capacity: Some(capacity),
        }
    }

    pub(crate) fn reserve(path: &Path) -> Result<Self, String> {
        crate::agent::persistence::database_writer::DatabaseWriter::open_blocking(
            path.to_path_buf(),
        )
        .map_err(|_| "database writer unavailable".to_string())?
        .reserve_necessary()
    }

    pub(crate) fn submit<C: DatabaseCommand<Output = Result<(), String>>>(
        mut self,
        identity: DatabaseIdentity,
        command: C,
    ) -> Result<std::sync::mpsc::Receiver<Result<(), String>>, String> {
        let (reply, receipt) = std::sync::mpsc::sync_channel(1);
        let request = NecessaryRequest {
            identity,
            command,
            reply,
            _capacity: self.capacity.take(),
        };
        let sender = self
            .sender
            .take()
            .ok_or("necessary terminal already consumed")?;
        sender
            .try_send(Box::new(request))
            .map_err(|_| "necessary terminal channel unavailable".to_string())?;
        self.writer.wake();
        Ok(receipt)
    }
}

impl Drop for NecessaryLease {
    fn drop(&mut self) {
        drop(self.sender.take());
        self.writer.wake();
    }
}

struct NecessaryRequest<C: DatabaseCommand<Output = Result<(), String>>> {
    identity: DatabaseIdentity,
    command: C,
    reply: std::sync::mpsc::SyncSender<Result<(), String>>,
    _capacity: Option<tokio::sync::OwnedSemaphorePermit>,
}

impl<C: DatabaseCommand<Output = Result<(), String>>> WriteRequest for NecessaryRequest<C> {
    fn execute(self: Box<Self>, store: &Store) {
        let _ = self.execute_terminal(store);
    }

    fn execute_terminal(self: Box<Self>, store: &Store) -> bool {
        let result = if store.verify_database_identity(self.identity).is_ok() {
            self.command.apply(store)
        } else {
            Err("database identity changed before necessary terminal".into())
        };
        if result.is_err() {
            tracing::error!(outcome = "necessary_terminal_not_confirmed");
        }
        let succeeded = result.is_ok();
        let _ = self.reply.send(result);
        succeeded
    }
}

pub(crate) fn commit<C: DatabaseCommand>(
    path: &Path,
    identity: DatabaseIdentity,
    command: C,
) -> Result<C::Output, String> {
    commit_inner(path, identity, command, None).map_err(|error| error.to_string())
}

pub(crate) fn commit_with_admission<C: DatabaseCommand>(
    path: &Path,
    identity: DatabaseIdentity,
    command: C,
    cancellation: CommitCancellation,
    budget: Duration,
) -> Result<C::Output, CommitError> {
    if cancellation.is_cancelled() {
        return Err(CommitError::CancelledBeforeEnqueue);
    }
    if budget.is_zero() {
        return Err(CommitError::BudgetBeforeEnqueue);
    }
    commit_inner(path, identity, command, Some((cancellation, budget)))
}

fn commit_inner<C: DatabaseCommand>(
    path: &Path,
    identity: DatabaseIdentity,
    command: C,
    admission: Option<(CommitCancellation, Duration)>,
) -> Result<C::Output, CommitError> {
    let writer = crate::agent::persistence::database_writer::DatabaseWriter::open_blocking(
        path.to_path_buf(),
    )
    .map_err(|_| CommitError::Unavailable)?;
    let admission = admission
        .map(|(cancellation, budget)| Arc::new(CommitAdmission::new(cancellation, budget)));
    let (reply, receipt) = std::sync::mpsc::sync_channel(1);
    let request = Box::new(Request {
        identity,
        command,
        reply,
        admission: admission.clone(),
    });
    if let Some(admission) = &admission {
        writer.submit_with_admission(request, Some(admission))?;
    } else {
        writer
            .submit(request)
            .map_err(|_| CommitError::Unavailable)?;
        return receipt.recv().map_err(|_| CommitError::OutcomeUnknown)?;
    }
    loop {
        if let Some(error) = admission
            .as_ref()
            .and_then(|admission| admission.rejection(true))
        {
            return Err(error);
        }
        match receipt.recv_timeout(Duration::from_millis(25)) {
            Ok(result) => return result,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err(CommitError::OutcomeUnknown);
            }
        }
    }
}
