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

fn directory(conn: &Connection, create: bool) -> std::io::Result<(File, std::fs::Metadata)> {
    let path = Path::new(
        conn.path()
            .filter(|path| !path.is_empty())
            .ok_or_else(invalid)?,
    );
    if !path.is_absolute() {
        return Err(invalid());
    }
    let parent = path.parent().ok_or_else(invalid)?;
    let mut anchor = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open("/")?;
    for part in parent.components() {
        match part {
            Component::Normal(name) => {
                anchor = open_at(
                    &anchor,
                    &component(name.as_bytes())?,
                    libc::O_RDONLY | libc::O_DIRECTORY,
                )?;
            }
            Component::RootDir => {}
            _ => return Err(invalid()),
        }
    }
    let metadata = anchor.metadata()?;
    if metadata.uid() != owner() || metadata.mode() & 0o022 != 0 {
        return Err(invalid());
    }
    let database = open_at(
        &anchor,
        &component(path.file_name().ok_or_else(invalid)?.as_bytes())?,
        libc::O_RDONLY,
    )?;
    let database = database.metadata()?;
    if !database.is_file() || database.uid() != owner() || database.nlink() != 1 {
        return Err(invalid());
    }
    let name = component("persistence-failures")?;
    if create {
        // SAFETY: anchor is an owned directory and name is a fixed NUL-terminated component.
        let result = unsafe { libc::mkdirat(anchor.as_raw_fd(), name.as_ptr(), 0o700) };
        if result != 0
            && std::io::Error::last_os_error().kind() != std::io::ErrorKind::AlreadyExists
        {
            return Err(std::io::Error::last_os_error());
        }
    }
    let directory = open_at(&anchor, &name, libc::O_RDONLY | libc::O_DIRECTORY)?;
    let metadata = directory.metadata()?;
    if metadata.uid() != owner() || metadata.mode() & 0o777 != 0o700 {
        return Err(invalid());
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

fn lock_directory(directory: &File) -> std::io::Result<File> {
    let file = open_at(
        directory,
        &component(".lock")?,
        libc::O_RDWR | libc::O_CREAT,
    )?;
    private_file(&file)?;
    // SAFETY: file owns a live descriptor; nonblocking flock is released when that descriptor closes.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(file)
}

fn select_slot(directory: &File) -> std::io::Result<CString> {
    let mut vacant = None;
    let mut oldest = None;
    for index in 0..CAPACITY {
        let name = component(format!("{index:02}.json"))?;
        match open_at(directory, &name, libc::O_RDONLY) {
            Ok(file) => {
                private_file(&file)?;
                let metadata = file.metadata()?;
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
            Err(error) => return Err(error),
        }
    }
    vacant
        .or_else(|| oldest.map(|(_, name)| name))
        .ok_or_else(invalid)
}

fn write_temporary(directory: &File, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = open_at(
        directory,
        &component(".pending")?,
        libc::O_WRONLY | libc::O_CREAT,
    )?;
    private_file(&file)?;
    file.set_len(0)?;
    file.write_all(bytes)?;
    file.sync_all()
}

pub fn record(conn: &Connection, failure: PersistenceFailure) -> std::io::Result<()> {
    validate(&failure)?;
    let _guard = FILE_ACCESS.lock().map_err(|_| invalid())?;
    let (directory, database) = directory(conn, true)?;
    let _lock = lock_directory(&directory)?;
    let stored = StoredFailure {
        version: 1,
        device: database.dev(),
        inode: database.ino(),
        failure,
    };
    let bytes = serde_json::to_vec(&stored).map_err(|_| invalid())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(invalid());
    }
    let slot = select_slot(&directory)?;
    let temporary = component(".pending")?;
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
            return Err(std::io::Error::last_os_error());
        }
        directory.sync_all()
    })();
    // SAFETY: temporary names the validated staging file within the exclusively locked directory.
    unsafe {
        libc::unlinkat(directory.as_raw_fd(), temporary.as_ptr(), 0);
    }
    result
}

pub fn read_by_run(
    conn: &Connection,
    thread_id: &str,
    run_id: &str,
) -> std::io::Result<Vec<PersistenceFailure>> {
    let _guard = FILE_ACCESS.lock().map_err(|_| invalid())?;
    let (directory, database) = match directory(conn, false) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let _lock = lock_directory(&directory)?;
    let mut failures = Vec::new();
    for slot in 0..CAPACITY {
        let name = component(format!("{slot:02}.json"))?;
        let mut file = match open_at(&directory, &name, libc::O_RDONLY) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        private_file(&file)?;
        let mut bytes = Vec::new();
        Read::by_ref(&mut file)
            .take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(invalid());
        }
        let stored: StoredFailure = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        validate(&stored.failure)?;
        if stored.version != 1 {
            return Err(invalid());
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
