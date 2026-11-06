use std::collections::VecDeque;
use std::ffi::CString;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Component, Path};
use std::sync::Mutex;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::write_coordination::{PersistenceCategory, StorageObservation};

const CAPACITY: usize = 64;
const MAX_BYTES: u64 = 65536;
static FILE_ACCESS: Mutex<()> = Mutex::new(());
static SAVE_FAILURES: Mutex<VecDeque<(crate::thread_admission::DatabaseIdentity, SaveFailure)>> =
    Mutex::new(VecDeque::new());

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureStage {
    RecordValidation,
    ProcessLock,
    DatabasePath,
    ParentOpen,
    ParentValidation,
    DatabaseOpen,
    DatabaseValidation,
    DirectoryCreate,
    DirectoryOpen,
    DirectoryValidation,
    LockOpen,
    LockValidation,
    LockAcquire,
    Encoding,
    SlotSelection,
    TemporaryOpen,
    TemporaryValidation,
    TemporaryWrite,
    Publish,
    DirectorySync,
    SlotOpen,
    SlotValidation,
    SlotRead,
    SlotDecode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct DiagnosticFault {
    pub stage: FailureStage,
    pub io_kind: &'static str,
    pub os_code: Option<i32>,
}

impl std::fmt::Display for DiagnosticFault {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "diagnostic storage {:?}: {}",
            self.stage, self.io_kind
        )
    }
}

impl std::error::Error for DiagnosticFault {}

fn at<T>(stage: FailureStage, result: std::io::Result<T>) -> Result<T, DiagnosticFault> {
    result.map_err(|error| DiagnosticFault {
        stage,
        io_kind: match error.kind() {
            std::io::ErrorKind::NotFound => "not_found",
            std::io::ErrorKind::PermissionDenied => "permission_denied",
            std::io::ErrorKind::AlreadyExists => "already_exists",
            std::io::ErrorKind::WouldBlock => "would_block",
            std::io::ErrorKind::InvalidInput => "invalid_input",
            std::io::ErrorKind::InvalidData => "invalid_data",
            std::io::ErrorKind::TimedOut => "timed_out",
            std::io::ErrorKind::Interrupted => "interrupted",
            std::io::ErrorKind::Unsupported => "unsupported",
            std::io::ErrorKind::OutOfMemory => "out_of_memory",
            std::io::ErrorKind::StorageFull => "storage_full",
            _ => "other",
        },
        os_code: error.raw_os_error(),
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SaveFailure {
    pub failure: PersistenceFailure,
    pub fault: DiagnosticFault,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadState {
    Readable,
    Missing,
    Failed,
}

pub struct ReadReport {
    pub state: ReadState,
    pub fault: Option<DiagnosticFault>,
    pub failures: Vec<PersistenceFailure>,
}

pub fn process_save_failures(
    store: &crate::Store,
    thread_id: &str,
    run_id: &str,
) -> Vec<SaveFailure> {
    SAVE_FAILURES
        .lock()
        .map(|failures| {
            failures
                .iter()
                .filter(|(database, failure)| {
                    *database == store.database_identity()
                        && failure.failure.thread_id == thread_id
                        && failure.failure.run_id == run_id
                })
                .map(|(_, failure)| failure.clone())
                .collect()
        })
        .unwrap_or_default()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersistenceFailure {
    pub thread_id: String,
    pub run_id: String,
    pub message_id: String,
    pub occurred_at: u64,
    pub category: PersistenceCategory,
    pub elapsed_ms: u64,
    pub sqlite_extended_code: Option<i32>,
    pub observations: Vec<StorageObservation>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredFailure {
    version: u32,
    device: u64,
    inode: u64,
    failure: PersistenceFailure,
}

fn invalid() -> std::io::Error {
    std::io::Error::from(std::io::ErrorKind::InvalidData)
}

fn component(value: impl AsRef<[u8]>) -> std::io::Result<CString> {
    CString::new(value.as_ref()).map_err(|_| invalid())
}

fn open_at(directory: &File, name: &CString, flags: i32) -> std::io::Result<File> {
    // SAFETY: directory is live, name is NUL-terminated, flags include creation mode when needed.
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: successful openat returns a new descriptor owned by File.
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn owner() -> u32 {
    // SAFETY: geteuid reads the process effective identity without arguments.
    unsafe { libc::geteuid() }
}

fn private_file(file: &File) -> std::io::Result<()> {
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != owner()
        || metadata.nlink() != 1
        || metadata.mode() & 0o777 != 0o600
        || metadata.len() > MAX_BYTES
    {
        return Err(invalid());
    }
    Ok(())
}

fn directory(
    conn: &Connection,
    create: bool,
) -> Result<(File, std::fs::Metadata), DiagnosticFault> {
    let path = Path::new(at(
        FailureStage::DatabasePath,
        conn.path()
            .filter(|path| !path.is_empty())
            .ok_or_else(invalid),
    )?);
    if !path.is_absolute() {
        return at(FailureStage::DatabasePath, Err(invalid()));
    }
    let parent = at(
        FailureStage::DatabasePath,
        path.parent().ok_or_else(invalid),
    )?;
    let mut anchor = at(
        FailureStage::ParentOpen,
        OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open("/"),
    )?;
    for part in parent.components() {
        match part {
            Component::Normal(name) => {
                anchor = at(
                    FailureStage::ParentOpen,
                    open_at(
                        &anchor,
                        &at(FailureStage::DatabasePath, component(name.as_bytes()))?,
                        libc::O_RDONLY | libc::O_DIRECTORY,
                    ),
                )?;
            }
            Component::RootDir => {}
            _ => return at(FailureStage::DatabasePath, Err(invalid())),
        }
    }
    let metadata = at(FailureStage::ParentValidation, anchor.metadata())?;
    if metadata.uid() != owner() || metadata.mode() & 0o022 != 0 {
        return at(FailureStage::ParentValidation, Err(invalid()));
    }
    let name = at(
        FailureStage::DatabasePath,
        path.file_name().ok_or_else(invalid),
    )?;
    let database = at(
        FailureStage::DatabaseOpen,
        open_at(
            &anchor,
            &at(FailureStage::DatabasePath, component(name.as_bytes()))?,
            libc::O_RDONLY,
        ),
    )?;
    let database = at(FailureStage::DatabaseValidation, database.metadata())?;
    if !database.is_file() || database.uid() != owner() || database.nlink() != 1 {
        return at(FailureStage::DatabaseValidation, Err(invalid()));
    }
    let name = at(
        FailureStage::DirectoryOpen,
        component("persistence-failures"),
    )?;
    if create {
        // SAFETY: anchor is an owned directory and name is a fixed NUL-terminated component.
        let result = unsafe { libc::mkdirat(anchor.as_raw_fd(), name.as_ptr(), 0o700) };
        if result != 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::AlreadyExists {
                return at(FailureStage::DirectoryCreate, Err(error));
            }
        }
    }
    let directory = at(
        FailureStage::DirectoryOpen,
        open_at(&anchor, &name, libc::O_RDONLY | libc::O_DIRECTORY),
    )?;
    let metadata = at(FailureStage::DirectoryValidation, directory.metadata())?;
    if metadata.uid() != owner() || metadata.mode() & 0o777 != 0o700 {
        return at(FailureStage::DirectoryValidation, Err(invalid()));
    }
    Ok((directory, database))
}

fn validate(failure: &PersistenceFailure) -> std::io::Result<()> {
    for id in [&failure.thread_id, &failure.run_id, &failure.message_id] {
        if !crate::write_coordination::valid_diagnostic_id(id) {
            return Err(invalid());
        }
    }
    if failure.observations.len() > 64
        || failure
            .observations
            .iter()
            .any(|observation| !observation.valid())
        || failure.sqlite_extended_code.is_some_and(|code| code <= 0)
    {
        return Err(invalid());
    }
    Ok(())
}

fn lock_directory(directory: &File, create: bool) -> Result<File, DiagnosticFault> {
    let file = at(
        FailureStage::LockOpen,
        open_at(
            directory,
            &at(FailureStage::LockOpen, component(".lock"))?,
            libc::O_RDWR | if create { libc::O_CREAT } else { 0 },
        ),
    )?;
    at(FailureStage::LockValidation, private_file(&file))?;
    // SAFETY: file owns a live descriptor; nonblocking flock is released when that descriptor closes.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return at(
            FailureStage::LockAcquire,
            Err(std::io::Error::last_os_error()),
        );
    }
    Ok(file)
}

fn select_slot(directory: &File) -> Result<CString, DiagnosticFault> {
    let mut vacant = None;
    let mut oldest = None;
    for index in 0..CAPACITY {
        let name = at(
            FailureStage::SlotSelection,
            component(format!("{index:02}.json")),
        )?;
        match open_at(directory, &name, libc::O_RDONLY) {
            Ok(file) => {
                at(FailureStage::SlotSelection, private_file(&file))?;
                let metadata = at(FailureStage::SlotSelection, file.metadata())?;
                let age = (metadata.mtime(), metadata.mtime_nsec(), index);
                if oldest.as_ref().is_none_or(|(previous, _)| age < *previous) {
                    oldest = Some((age, name));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if vacant.is_none() {
                    vacant = Some(name);
                }
            }
            Err(error) => return at(FailureStage::SlotSelection, Err(error)),
        }
    }
    at(
        FailureStage::SlotSelection,
        vacant
            .or_else(|| oldest.map(|(_, name)| name))
            .ok_or_else(invalid),
    )
}

fn write_temporary(directory: &File, bytes: &[u8]) -> Result<(), DiagnosticFault> {
    let mut file = at(
        FailureStage::TemporaryOpen,
        open_at(
            directory,
            &at(FailureStage::TemporaryOpen, component(".pending"))?,
            libc::O_WRONLY | libc::O_CREAT,
        ),
    )?;
    at(FailureStage::TemporaryValidation, private_file(&file))?;
    at(FailureStage::TemporaryWrite, file.set_len(0))?;
    at(FailureStage::TemporaryWrite, file.write_all(bytes))?;
    at(FailureStage::TemporaryWrite, file.sync_all())
}

pub fn record(store: &crate::Store, failure: PersistenceFailure) -> Result<(), DiagnosticFault> {
    at(FailureStage::RecordValidation, validate(&failure))?;
    let result = record_inner(store.conn(), &failure);
    if let Err(fault) = result
        && let Ok(mut failures) = SAVE_FAILURES.lock()
    {
        if failures.len() == CAPACITY {
            failures.pop_front();
        }
        failures.push_back((store.database_identity(), SaveFailure { fault, failure }));
    }
    result
}

fn record_inner(conn: &Connection, failure: &PersistenceFailure) -> Result<(), DiagnosticFault> {
    let _guard = at(
        FailureStage::ProcessLock,
        FILE_ACCESS.lock().map_err(|_| invalid()),
    )?;
    let (directory, database) = directory(conn, true)?;
    let _lock = lock_directory(&directory, true)?;
    let stored = StoredFailure {
        version: 1,
        device: database.dev(),
        inode: database.ino(),
        failure: failure.clone(),
    };
    let bytes = at(
        FailureStage::Encoding,
        serde_json::to_vec(&stored).map_err(|_| invalid()),
    )?;
    if bytes.len() as u64 > MAX_BYTES {
        return at(FailureStage::Encoding, Err(invalid()));
    }
    let slot = select_slot(&directory)?;
    let temporary = at(FailureStage::TemporaryOpen, component(".pending"))?;
    write_temporary(&directory, &bytes)?;
    let result = (|| {
        // SAFETY: both names are fixed safe components relative to the same live directory descriptor.
        if unsafe {
            libc::renameat(
                directory.as_raw_fd(),
                temporary.as_ptr(),
                directory.as_raw_fd(),
                slot.as_ptr(),
            )
        } != 0
        {
            return at(FailureStage::Publish, Err(std::io::Error::last_os_error()));
        }
        at(FailureStage::DirectorySync, directory.sync_all())
    })();
    // SAFETY: temporary names the validated staging file within the exclusively locked directory.
    unsafe {
        libc::unlinkat(directory.as_raw_fd(), temporary.as_ptr(), 0);
    }
    result
}

pub fn read_report(conn: &Connection, thread_id: &str, run_id: &str) -> ReadReport {
    match read_by_run(conn, thread_id, run_id) {
        Ok(failures) => ReadReport {
            state: ReadState::Readable,
            fault: None,
            failures,
        },
        Err(fault) => ReadReport {
            state: if fault.stage == FailureStage::DirectoryOpen && fault.io_kind == "not_found" {
                ReadState::Missing
            } else {
                ReadState::Failed
            },
            fault: Some(fault),
            failures: Vec::new(),
        },
    }
}

pub fn read_by_run(
    conn: &Connection,
    thread_id: &str,
    run_id: &str,
) -> Result<Vec<PersistenceFailure>, DiagnosticFault> {
    let _guard = at(
        FailureStage::ProcessLock,
        FILE_ACCESS.lock().map_err(|_| invalid()),
    )?;
    let (directory, database) = directory(conn, false)?;
    let _lock = lock_directory(&directory, false)?;
    let mut failures = Vec::new();
    for slot in 0..CAPACITY {
        let name = at(FailureStage::SlotOpen, component(format!("{slot:02}.json")))?;
        let mut file = match open_at(&directory, &name, libc::O_RDONLY) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return at(FailureStage::SlotOpen, Err(error)),
        };
        at(FailureStage::SlotValidation, private_file(&file))?;
        let mut bytes = Vec::new();
        at(
            FailureStage::SlotRead,
            Read::by_ref(&mut file)
                .take(MAX_BYTES + 1)
                .read_to_end(&mut bytes),
        )?;
        if bytes.len() as u64 > MAX_BYTES {
            return at(FailureStage::SlotValidation, Err(invalid()));
        }
        let stored: StoredFailure = at(
            FailureStage::SlotDecode,
            serde_json::from_slice(&bytes).map_err(|_| invalid()),
        )?;
        at(FailureStage::SlotValidation, validate(&stored.failure))?;
        if stored.version != 1 {
            return at(FailureStage::SlotValidation, Err(invalid()));
        }
        if stored.device == database.dev()
            && stored.inode == database.ino()
            && stored.failure.thread_id == thread_id
            && stored.failure.run_id == run_id
        {
            failures.push(stored.failure);
        }
    }
    failures.sort_by_key(|failure| failure.occurred_at);
    Ok(failures)
}
