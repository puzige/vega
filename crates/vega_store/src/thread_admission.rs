use std::collections::HashMap;
use std::os::unix::fs::MetadataExt;
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
    pub fn verifies(&self, database: DatabaseIdentity, thread: &str) -> bool {
        self.0.key.database == database && self.0.key.thread == thread
    }

    pub fn same_owner(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) && self.0.token == other.0.token
    }
}

type Admissions = Mutex<HashMap<AdmissionKey, Weak<Admission>>>;

pub fn try_acquire(
    database: DatabaseIdentity,
    thread: &str,
) -> Result<Option<ThreadAdmission>, AdmissionError> {
    static ADMISSIONS: OnceLock<Admissions> = OnceLock::new();
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
    admissions.retain(|_, admission| admission.strong_count() > 0);
    if admissions.get(&key).and_then(Weak::upgrade).is_some() {
        return Ok(None);
    }
    let admission = Arc::new(Admission {
        key: key.clone(),
        token: Ulid::generate(),
    });
    admissions.insert(key, Arc::downgrade(&admission));
    Ok(Some(ThreadAdmission(admission)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use std::sync::Barrier;

    #[test]
    fn last_clone_holds_exact_database_and_thread_until_release() {
        let first = Store::open(":memory:").unwrap();
        let second = Store::open(":memory:").unwrap();
        let database = first.database_identity();
        let owner = try_acquire(database, "a").unwrap().unwrap();
        let clone = owner.clone();
        let token = owner.0.token;
        assert!(owner.same_owner(&clone));
        assert!(owner.verifies(database, "a"));
        assert!(!owner.verifies(database, "b"));
        assert!(!owner.verifies(second.database_identity(), "a"));
        assert!(try_acquire(database, "b").unwrap().is_some());
        assert!(
            try_acquire(second.database_identity(), "a")
                .unwrap()
                .is_some()
        );
        drop(owner);
        assert!(try_acquire(database, "a").unwrap().is_none());
        drop(clone);
        let next = try_acquire(database, "a").unwrap().unwrap();
        assert_ne!(token, next.0.token);
        assert!(try_acquire(database, "a").unwrap().is_none());
    }

    #[test]
    fn file_aliases_share_admission_and_cached_identity_needs_no_path() {
        let directory = std::env::temp_dir().join(format!("vega-admission-{}", Ulid::generate()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("database.sqlite");
        let alias = directory.join("alias.sqlite");
        let first = Store::open(&path).unwrap();
        std::os::unix::fs::symlink(&path, &alias).unwrap();
        let second = Store::open_read_only(&alias).unwrap();
        assert_eq!(first.database_identity(), second.database_identity());
        let owner = try_acquire(first.database_identity(), "a")
            .unwrap()
            .unwrap();
        assert!(
            try_acquire(second.database_identity(), "a")
                .unwrap()
                .is_none()
        );
        std::fs::remove_file(&alias).unwrap();
        assert!(owner.verifies(second.database_identity(), "a"));
        drop(second);
        drop(first);
        drop(owner);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn concurrent_contenders_cannot_both_own_a_thread() {
        let store = Store::open(":memory:").unwrap();
        let database = store.database_identity();
        let barrier = Arc::new(Barrier::new(2));
        let contenders: Vec<_> = (0..2)
            .map(|_| {
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    let admission = try_acquire(database, "a").unwrap();
                    barrier.wait();
                    admission
                })
            })
            .collect();
        let owners: Vec<_> = contenders
            .into_iter()
            .filter_map(|contender| contender.join().unwrap())
            .collect();
        assert_eq!(owners.len(), 1);
        assert!(try_acquire(database, "a").unwrap().is_none());
        drop(owners);
        assert!(try_acquire(database, "a").unwrap().is_some());
    }

    #[test]
    fn poisoned_registry_refuses_new_owners() {
        let admissions = Arc::new(Admissions::default());
        let poisoned = admissions.clone();
        assert!(
            std::thread::spawn(move || {
                let _guard = poisoned.lock().unwrap();
                panic!("poison admission registry");
            })
            .join()
            .is_err()
        );
        let store = Store::open(":memory:").unwrap();
        assert!(matches!(
            try_acquire_in(&admissions, store.database_identity(), "a"),
            Err(AdmissionError::Unavailable)
        ));
    }
}
