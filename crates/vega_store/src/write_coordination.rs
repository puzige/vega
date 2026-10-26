use std::collections::{HashMap, VecDeque};
use std::marker::PhantomData;
use std::ops::Deref;
use std::os::unix::fs::MetadataExt;
use std::sync::{Arc, Condvar, Mutex, OnceLock, Weak};
use std::thread::ThreadId;
use std::time::{Duration, Instant};

use rusqlite::{Connection, Transaction, TransactionBehavior};

const MAX_WAITERS: usize = 128;
const QUEUE_TIMEOUT: Duration = Duration::from_secs(10);
const CANCEL_INTERVAL: Duration = Duration::from_millis(25);

type WriterQueues = HashMap<DatabaseIdentity, Weak<WriterQueue>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WriteCategory {
    Initialization,
    Migration,
    Messages,
    ToolCalls,
    Usage,
    Diagnostics,
    Threads,
    Projects,
    Sidebar,
    PendingMessages,
    ContextCompaction,
    Credentials,
    Skills,
    Permissions,
    Delegation,
    Automation,
    CodexTasks,
    McpServers,
    Attachments,
    ComputerDelivery,
    RuntimeEvent,
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
enum DatabaseIdentity {
    File { device: u64, inode: u64 },
    Connection(usize),
}

#[derive(Default)]
struct WriterState {
    owner: Option<(ThreadId, usize, usize)>,
    waiters: VecDeque<u64>,
    next_ticket: u64,
}

#[derive(Default)]
struct WriterQueue {
    state: Mutex<WriterState>,
    changed: Condvar,
}

pub struct WritePermit<'a> {
    queue: Arc<WriterQueue>,
    connection: usize,
    category: WriteCategory,
    acquired: Instant,
    queue_wait: Duration,
    database: DatabaseIdentity,
    _connection: PhantomData<&'a Connection>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageOperation {
    PermitAcquired,
    PermitReleased,
    QueueFull,
    QueueTimeout,
    QueueCancelled,
    CrossConnectionReentry,
    Begin,
    Commit,
    Rollback,
    Persistence,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PersistenceCategory {
    Snapshot,
    Diagnostic,
    ResponseStarted,
    ReasoningCompleted,
    ResponseRetrying,
    Steering,
    BuiltinSkill,
    Screenshot,
    ToolRound,
    ResponseCompleted,
    DiagnosticAttempt,
    ReasoningReleased,
    TextDelta,
    ThinkingDelta,
    SummaryDelta,
    ToolProposal,
    ToolValidation,
    ToolConflict,
    ToolApproval,
    ToolRunning,
    ToolOutput,
    ToolTerminal,
    StageHandoff,
    SkillActivation,
    SkillSnapshot,
    Usage,
    CompactionUsage,
    CompactionStatus,
    ContextAccounting,
    Finished,
    Interrupted,
    Error,
}

pub fn record_persistence(
    conn: &Connection,
    category: PersistenceCategory,
    started: Instant,
    succeeded: bool,
    sqlite_extended_code: Option<i32>,
) {
    if let Ok(database) = identity(conn) {
        record(
            database,
            StorageObservation {
                category: WriteCategory::RuntimeEvent,
                operation: StorageOperation::Persistence,
                elapsed_ms: started.elapsed().as_millis() as u64,
                queue_wait_ms: None,
                succeeded,
                sqlite_primary_code: sqlite_extended_code.map(|code| code & 255),
                sqlite_extended_code,
                persistence: Some(category),
            },
        );
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct StorageObservation {
    persistence: Option<PersistenceCategory>,
    category: WriteCategory,
    operation: StorageOperation,
    elapsed_ms: u64,
    queue_wait_ms: Option<u64>,
    succeeded: bool,
    sqlite_primary_code: Option<i32>,
    sqlite_extended_code: Option<i32>,
}

fn observations() -> &'static Mutex<VecDeque<(DatabaseIdentity, StorageObservation)>> {
    static OBSERVATIONS: OnceLock<Mutex<VecDeque<(DatabaseIdentity, StorageObservation)>>> =
        OnceLock::new();
    OBSERVATIONS.get_or_init(Default::default)
}

fn record(database: DatabaseIdentity, observation: StorageObservation) {
    if let Ok(mut observations) = observations().lock() {
        while observations.len() >= 256 {
            observations.pop_front();
        }
        observations.push_back((database, observation));
    }
}

pub fn recent_observations(conn: &Connection) -> Vec<StorageObservation> {
    let Ok(database) = identity(conn) else {
        return Vec::new();
    };
    observations()
        .lock()
        .map(|observations| {
            observations
                .iter()
                .filter(|(key, _)| *key == database)
                .map(|(_, observation)| observation.clone())
                .collect()
        })
        .unwrap_or_default()
}

fn record_queue_failure(
    database: DatabaseIdentity,
    category: WriteCategory,
    operation: StorageOperation,
    started: Instant,
    code: i32,
) {
    record(
        database,
        StorageObservation {
            persistence: None,
            category,
            operation,
            elapsed_ms: started.elapsed().as_millis() as u64,
            queue_wait_ms: Some(started.elapsed().as_millis() as u64),
            succeeded: false,
            sqlite_primary_code: Some(code & 255),
            sqlite_extended_code: Some(code),
        },
    );
}

fn sqlite_error(code: i32) -> rusqlite::Error {
    rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(code), None)
}

fn unavailable() -> rusqlite::Error {
    sqlite_error(rusqlite::ffi::SQLITE_ABORT)
}

fn identity(conn: &Connection) -> rusqlite::Result<DatabaseIdentity> {
    match conn.path().filter(|path| !path.is_empty()) {
        Some(path) => {
            let metadata = std::fs::metadata(path).map_err(|_| unavailable())?;
            Ok(DatabaseIdentity::File {
                device: metadata.dev(),
                inode: metadata.ino(),
            })
        }
        None => Ok(DatabaseIdentity::Connection(
            conn as *const Connection as usize,
        )),
    }
}

fn queue(conn: &Connection) -> rusqlite::Result<Arc<WriterQueue>> {
    static QUEUES: OnceLock<Mutex<WriterQueues>> = OnceLock::new();
    let key = identity(conn)?;
    let mut queues = QUEUES
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| unavailable())?;
    queues.retain(|_, queue| queue.strong_count() > 0);
    if let Some(queue) = queues.get(&key).and_then(Weak::upgrade) {
        return Ok(queue);
    }
    let queue = Arc::new(WriterQueue::default());
    queues.insert(key, Arc::downgrade(&queue));
    Ok(queue)
}

pub fn acquire(conn: &Connection, category: WriteCategory) -> rusqlite::Result<WritePermit<'_>> {
    acquire_with_cancel(conn, category, || false)
}

pub fn acquire_with_cancel<F>(
    conn: &Connection,
    category: WriteCategory,
    should_cancel: F,
) -> rusqlite::Result<WritePermit<'_>>
where
    F: Fn() -> bool,
{
    acquire_with_deadline(conn, category, should_cancel, QUEUE_TIMEOUT)
}

fn acquire_with_deadline<F>(
    conn: &Connection,
    category: WriteCategory,
    should_cancel: F,
    timeout: Duration,
) -> rusqlite::Result<WritePermit<'_>>
where
    F: Fn() -> bool,
{
    let started = Instant::now();
    let database = identity(conn)?;
    let queue = queue(conn)?;
    let connection = conn as *const Connection as usize;
    let thread = std::thread::current().id();
    let mut state = queue.state.lock().map_err(|_| unavailable())?;
    if let Some((owner, owner_connection, depth)) = state.owner.as_mut()
        && *owner == thread
    {
        if *owner_connection != connection {
            record_queue_failure(
                database,
                category,
                StorageOperation::CrossConnectionReentry,
                started,
                rusqlite::ffi::SQLITE_LOCKED,
            );
            tracing::warn!(
                ?category,
                outcome = "cross_connection_reentry",
                sqlite_primary_code = 6,
                sqlite_extended_code = 6
            );
            return Err(sqlite_error(rusqlite::ffi::SQLITE_LOCKED));
        }
        *depth = depth.checked_add(1).ok_or_else(unavailable)?;
    } else {
        if state.waiters.len() >= MAX_WAITERS {
            record_queue_failure(
                database,
                category,
                StorageOperation::QueueFull,
                started,
                rusqlite::ffi::SQLITE_BUSY,
            );
            tracing::warn!(
                ?category,
                outcome = "queue_full",
                sqlite_primary_code = 5,
                sqlite_extended_code = 5
            );
            return Err(sqlite_error(rusqlite::ffi::SQLITE_BUSY));
        }
        let ticket = state.next_ticket;
        state.next_ticket = ticket.checked_add(1).ok_or_else(unavailable)?;
        state.waiters.push_back(ticket);
        loop {
            let cancelled = should_cancel();
            let elapsed = started.elapsed();
            if cancelled || elapsed >= timeout {
                state.waiters.retain(|waiting| *waiting != ticket);
                queue.changed.notify_all();
                let code = if cancelled {
                    rusqlite::ffi::SQLITE_INTERRUPT
                } else {
                    rusqlite::ffi::SQLITE_BUSY
                };
                record_queue_failure(
                    database,
                    category,
                    if cancelled {
                        StorageOperation::QueueCancelled
                    } else {
                        StorageOperation::QueueTimeout
                    },
                    started,
                    code,
                );
                tracing::warn!(
                    ?category,
                    outcome = if cancelled {
                        "queue_cancelled"
                    } else {
                        "queue_timeout"
                    },
                    queue_wait_ms = elapsed.as_millis() as u64,
                    sqlite_primary_code = code,
                    sqlite_extended_code = code
                );
                return Err(sqlite_error(code));
            }
            if state.owner.is_none() && state.waiters.front() == Some(&ticket) {
                state.waiters.pop_front();
                state.owner = Some((thread, connection, 1));
                break;
            }
            let remaining = timeout.saturating_sub(elapsed).min(CANCEL_INTERVAL);
            (state, _) = queue
                .changed
                .wait_timeout(state, remaining)
                .map_err(|_| unavailable())?;
        }
    }
    drop(state);
    let queue_wait = started.elapsed();
    record(
        database,
        StorageObservation {
            persistence: None,
            category,
            operation: StorageOperation::PermitAcquired,
            elapsed_ms: queue_wait.as_millis() as u64,
            queue_wait_ms: Some(queue_wait.as_millis() as u64),
            succeeded: true,
            sqlite_primary_code: None,
            sqlite_extended_code: None,
        },
    );
    Ok(WritePermit {
        queue,
        connection,
        category,
        acquired: Instant::now(),
        queue_wait,
        database,
        _connection: PhantomData,
    })
}

impl Drop for WritePermit<'_> {
    fn drop(&mut self) {
        let Ok(mut state) = self.queue.state.lock() else {
            return;
        };
        if let Some((owner, connection, depth)) = state.owner.as_mut()
            && *owner == std::thread::current().id()
            && *connection == self.connection
        {
            *depth -= 1;
            if *depth == 0 {
                state.owner = None;
                self.queue.changed.notify_all();
            }
        }
        drop(state);
        record(
            self.database,
            StorageObservation {
                persistence: None,
                category: self.category,
                operation: StorageOperation::PermitReleased,
                elapsed_ms: self.acquired.elapsed().as_millis() as u64,
                queue_wait_ms: Some(self.queue_wait.as_millis() as u64),
                succeeded: true,
                sqlite_primary_code: None,
                sqlite_extended_code: None,
            },
        );
        tracing::debug!(category = ?self.category, outcome = "permit_released", queue_wait_ms = self.queue_wait.as_millis() as u64, hold_ms = self.acquired.elapsed().as_millis() as u64);
    }
}

#[derive(Debug)]
pub enum CommitFailure {
    RolledBack(rusqlite::Error),
    Unknown(rusqlite::Error),
}

pub struct WriteTransaction<'a> {
    transaction: Transaction<'a>,
    permit: WritePermit<'a>,
}

impl<'a> WriteTransaction<'a> {
    pub fn new(conn: &'a Connection, category: WriteCategory) -> rusqlite::Result<Self> {
        Self::new_with_cancel(conn, category, || false)
    }

    pub fn new_with_cancel<F>(
        conn: &'a Connection,
        category: WriteCategory,
        should_cancel: F,
    ) -> rusqlite::Result<Self>
    where
        F: Fn() -> bool,
    {
        let permit = acquire_with_cancel(conn, category, &should_cancel)?;
        let started = Instant::now();
        let result = Transaction::new_unchecked(conn, TransactionBehavior::Immediate);
        log_transaction(&permit, StorageOperation::Begin, started, &result);
        let transaction = result?;
        if should_cancel() {
            transaction.rollback()?;
            return Err(sqlite_error(rusqlite::ffi::SQLITE_INTERRUPT));
        }
        Ok(Self {
            transaction,
            permit,
        })
    }

    pub fn commit(self) -> rusqlite::Result<()> {
        let Self {
            transaction,
            permit,
        } = self;
        let started = permit.acquired;
        let result = transaction.commit();
        log_transaction(&permit, StorageOperation::Commit, started, &result);
        result
    }

    pub fn commit_with_rollback(self) -> Result<(), CommitFailure> {
        let Self {
            transaction,
            permit,
        } = self;
        let result = transaction.execute_batch("COMMIT");
        log_transaction(&permit, StorageOperation::Commit, permit.acquired, &result);
        match result {
            Ok(()) => Ok(()),
            Err(error) if !transaction.is_autocommit() => {
                let rollback = transaction.execute_batch("ROLLBACK");
                log_transaction(&permit, StorageOperation::Rollback, permit.acquired, &rollback);
                if rollback.is_ok() && transaction.is_autocommit() {
                    Err(CommitFailure::RolledBack(error))
                } else {
                    Err(CommitFailure::Unknown(error))
                }
            }
            Err(error) => Err(CommitFailure::Unknown(error)),
        }
    }

    pub fn rollback(self) -> rusqlite::Result<()> {
        let Self {
            transaction,
            permit,
        } = self;
        let started = permit.acquired;
        let result = transaction.rollback();
        log_transaction(&permit, StorageOperation::Rollback, started, &result);
        result
    }
}

impl<'a> Deref for WriteTransaction<'a> {
    type Target = Transaction<'a>;

    fn deref(&self) -> &Self::Target {
        &self.transaction
    }
}

fn log_transaction<T>(
    permit: &WritePermit<'_>,
    action: StorageOperation,
    started: Instant,
    result: &rusqlite::Result<T>,
) {
    let (primary, extended) = match result {
        Err(rusqlite::Error::SqliteFailure(error, _)) => {
            (Some(error.extended_code & 0xff), Some(error.extended_code))
        }
        _ => (None, None),
    };
    record(
        permit.database,
        StorageObservation {
            persistence: None,
            category: permit.category,
            operation: action,
            elapsed_ms: started.elapsed().as_millis() as u64,
            queue_wait_ms: None,
            succeeded: result.is_ok(),
            sqlite_primary_code: primary,
            sqlite_extended_code: extended,
        },
    );
    tracing::debug!(
        category = ?permit.category,
        ?action,
        outcome = if result.is_ok() {
            "succeeded"
        } else {
            "failed"
        },
        transaction_ms = started.elapsed().as_millis() as u64,
        sqlite_primary_code = primary,
        sqlite_extended_code = extended
    );
}
