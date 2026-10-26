use std::collections::HashMap;
use std::os::unix::fs::MetadataExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};

use rusqlite::Connection;
use ulid::Ulid;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct DatabaseIdentity(Identity);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum Identity {
    File { device: u64, inode: u64 },
    Memory(Ulid),
}

impl DatabaseIdentity {
    pub fn permission_fence_key(self) -> String {
        match self.0 {
            Identity::File { device, inode } => format!("file:{device}:{inode}"),
            Identity::Memory(id) => format!("memory:{id}"),
        }
    }

    pub(crate) fn for_connection(conn: &Connection) -> rusqlite::Result<Self> {
        let identity = match conn.path().filter(|path| !path.is_empty()) {
            Some(path) => {
                let metadata = std::fs::metadata(path).map_err(|_| {
                    rusqlite::Error::SqliteFailure(
                        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_ABORT),
                        None,
                    )
                })?;
                Identity::File {
                    device: metadata.dev(),
                    inode: metadata.ino(),
                }
            }
            None => Identity::Memory(Ulid::generate()),
        };
        Ok(Self(identity))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum AdmissionError {
    #[error("thread admission state is unavailable")]
    Unavailable,
    #[error("thread admission requires a thread identity")]
    InvalidThread,
}

#[derive(Clone, Eq, Hash, PartialEq)]
struct AdmissionKey {
    database: DatabaseIdentity,
    thread: String,
}

struct Admission {
    key: AdmissionKey,
    token: Ulid,
    title_started: AtomicBool,
}

#[derive(Clone)]
pub struct AdmissionDrain(Weak<Admission>);

impl AdmissionDrain {
    pub fn is_released(&self) -> bool {
        self.0.strong_count() == 0
    }

    pub fn is_title_draining(&self, database: DatabaseIdentity, thread: &str) -> bool {
        self.0.upgrade().is_some_and(|owner| {
            owner.key.database == database
                && owner.key.thread == thread
                && owner.title_started.load(Ordering::Acquire)
        })
    }
}

pub struct TitleAdmission(ThreadAdmission);

impl TitleAdmission {
    pub fn admission(&self) -> &ThreadAdmission {
        &self.0
    }
}

#[derive(Clone)]
pub struct ThreadAdmission(Arc<Admission>);

impl std::fmt::Debug for ThreadAdmission {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ThreadAdmission")
            .field("token", &self.0.token)
            .finish_non_exhaustive()
    }
}

impl ThreadAdmission {
    pub fn drain(&self) -> AdmissionDrain {
        AdmissionDrain(Arc::downgrade(&self.0))
    }

    pub fn into_title_drain(self) -> TitleAdmission {
        self.0.title_started.store(true, Ordering::Release);
        TitleAdmission(self)
    }

    pub fn verifies(&self, database: DatabaseIdentity, thread: &str) -> bool {
        self.0.key.database == database && self.0.key.thread == thread
    }

    pub fn same_owner(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) && self.0.token == other.0.token
    }
}

#[derive(Default)]
struct AdmissionRegistry {
    owners: HashMap<AdmissionKey, Weak<Admission>>,
    pauses: HashMap<DatabaseIdentity, Ulid>,
}

type Admissions = Mutex<AdmissionRegistry>;

static ADMISSIONS: OnceLock<Admissions> = OnceLock::new();

pub struct DatabaseAdmissionPause {
    database: DatabaseIdentity,
    token: Ulid,
}

impl DatabaseAdmissionPause {
    pub fn owners_released(&self) -> Result<bool, AdmissionError> {
        let registry = ADMISSIONS
            .get_or_init(Default::default)
            .lock()
            .map_err(|_| AdmissionError::Unavailable)?;
        if registry.pauses.get(&self.database) != Some(&self.token) {
            return Err(AdmissionError::Unavailable);
        }
        Ok(!registry
            .owners
            .iter()
            .any(|(key, owner)| key.database == self.database && owner.strong_count() > 0))
    }
}

impl Drop for DatabaseAdmissionPause {
    fn drop(&mut self) {
        if let Ok(mut registry) = ADMISSIONS.get_or_init(Default::default).lock()
            && registry.pauses.get(&self.database) == Some(&self.token)
        {
            registry.pauses.remove(&self.database);
        }
    }
}

pub fn try_pause_database(
    database: DatabaseIdentity,
) -> Result<Option<DatabaseAdmissionPause>, AdmissionError> {
    let mut registry = ADMISSIONS
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| AdmissionError::Unavailable)?;
    if registry.pauses.contains_key(&database) {
        return Ok(None);
    }
    let token = Ulid::generate();
    registry.pauses.insert(database, token);
    Ok(Some(DatabaseAdmissionPause { database, token }))
}

pub fn try_acquire(
    database: DatabaseIdentity,
    thread: &str,
) -> Result<Option<ThreadAdmission>, AdmissionError> {
    try_acquire_in(ADMISSIONS.get_or_init(Default::default), database, thread)
}

fn try_acquire_in(
    admissions: &Admissions,
    database: DatabaseIdentity,
    thread: &str,
) -> Result<Option<ThreadAdmission>, AdmissionError> {
    if thread.is_empty() {
        return Err(AdmissionError::InvalidThread);
    }
    let key = AdmissionKey {
        database,
        thread: thread.to_owned(),
    };
    let mut admissions = admissions.lock().map_err(|_| AdmissionError::Unavailable)?;
    admissions
        .owners
        .retain(|_, admission| admission.strong_count() > 0);
    if admissions.pauses.contains_key(&database)
        || admissions
            .owners
            .get(&key)
            .and_then(Weak::upgrade)
            .is_some()
    {
        return Ok(None);
    }
    let admission = Arc::new(Admission {
        key: key.clone(),
        token: Ulid::generate(),
        title_started: AtomicBool::new(false),
    });
    admissions.owners.insert(key, Arc::downgrade(&admission));
    Ok(Some(ThreadAdmission(admission)))
}
