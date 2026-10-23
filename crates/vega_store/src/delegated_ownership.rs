use std::ffi::CString;
use std::fs::{File, OpenOptions};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

pub struct DelegatedOwnership {
    database: PathBuf,
    child: String,
    anchor: File,
    directory: File,
    file: File,
}

fn unavailable() -> rusqlite::Error {
    rusqlite::Error::InvalidParameterName("delegated ownership unavailable".into())
}

fn checked(file: &File, directory: bool) -> rusqlite::Result<()> {
    let meta = file.metadata().map_err(|_| unavailable())?;
    // SAFETY: geteuid takes no pointers and has no memory preconditions.
    let uid = unsafe { libc::geteuid() };
    if meta.uid() != uid
        || meta.mode() & 0o7777 != if directory { 0o700 } else { 0o600 }
        || if directory {
            !meta.is_dir()
        } else {
            !meta.is_file() || meta.nlink() != 1
        }
    {
        return Err(unavailable());
    }
    Ok(())
}

impl DelegatedOwnership {
    pub fn acquire(database: &Path, child: &str) -> rusqlite::Result<Arc<Self>> {
        if child.is_empty()
            || child.len() > 128
            || !child
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(unavailable());
        }
        let database = database.canonicalize().map_err(|_| unavailable())?;
        let parent = database.parent().ok_or_else(unavailable)?;
        let anchor = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(parent)
            .map_err(|_| unavailable())?;
        // SAFETY: anchor owns a live descriptor and the directory name is NUL terminated.
        let made =
            unsafe { libc::mkdirat(anchor.as_raw_fd(), c"delegated-owner-locks".as_ptr(), 0o700) };
        if made != 0 && std::io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST) {
            return Err(unavailable());
        }
        // SAFETY: anchor is live and the static child name is NUL terminated.
        let fd = unsafe {
            libc::openat(
                anchor.as_raw_fd(),
                c"delegated-owner-locks".as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(unavailable());
        }
        // SAFETY: a successful openat returned a newly owned descriptor.
        let directory = unsafe { File::from_raw_fd(fd) };
        checked(&directory, true)?;
        let name = CString::new(format!("{child}.lock")).map_err(|_| unavailable())?;
        // SAFETY: directory is live, name is NUL terminated, and mode accompanies O_CREAT.
        let fd = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDWR
                    | libc::O_CREAT
                    | libc::O_NOFOLLOW
                    | libc::O_CLOEXEC
                    | libc::O_NONBLOCK,
                0o600,
            )
        };
        if fd < 0 {
            return Err(unavailable());
        }
        // SAFETY: a successful openat returned a newly owned descriptor.
        let file = unsafe { File::from_raw_fd(fd) };
        checked(&file, false)?;
        // SAFETY: file retains its descriptor for the entire lock lifetime.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(unavailable());
        }
        checked(&file, false)?;
        let proof = Arc::new(Self {
            database,
            child: child.into(),
            anchor,
            directory,
            file,
        });
        if !proof.identities_match() {
            return Err(unavailable());
        }
        Ok(proof)
    }

    pub fn verifies(&self, database: &Path, child: &str) -> bool {
        self.child == child
            && database
                .canonicalize()
                .is_ok_and(|path| path == self.database)
            && self.identities_match()
    }

    fn identities_match(&self) -> bool {
        let Some(parent) = self.database.parent() else {
            return false;
        };
        let Ok(anchor) = std::fs::symlink_metadata(parent) else {
            return false;
        };
        let Ok(owned_anchor) = self.anchor.metadata() else {
            return false;
        };
        if anchor.dev() != owned_anchor.dev()
            || anchor.ino() != owned_anchor.ino()
            || !anchor.is_dir()
        {
            return false;
        }
        let Ok(name) = CString::new(format!("{}.lock", self.child)) else {
            return false;
        };
        checked(&self.directory, true).is_ok()
            && checked(&self.file, false).is_ok()
            && named_identity(&self.anchor, c"delegated-owner-locks", &self.directory)
            && named_identity(&self.directory, &name, &self.file)
    }

    pub fn quarantine(self: Arc<Self>) {
        static QUARANTINED: OnceLock<Mutex<Vec<Arc<DelegatedOwnership>>>> = OnceLock::new();
        match QUARANTINED.get_or_init(Default::default).lock() {
            Ok(mut owners) => owners.push(self),
            Err(_) => std::mem::forget(self),
        }
    }
}

pub fn database_path(conn: &rusqlite::Connection) -> rusqlite::Result<PathBuf> {
    let path: String = conn.query_row(
        "SELECT file FROM pragma_database_list WHERE name='main'",
        [],
        |row| row.get(0),
    )?;
    if path.is_empty() {
        return Err(unavailable());
    }
    Ok(path.into())
}

pub fn acquire_for_thread(
    conn: &rusqlite::Connection,
    thread: &str,
) -> rusqlite::Result<Option<Arc<DelegatedOwnership>>> {
    if crate::threads::find(conn, thread)?.is_some_and(|row| row.parent_thread_id.is_some()) {
        Ok(Some(DelegatedOwnership::acquire(
            &database_path(conn)?,
            thread,
        )?))
    } else {
        Ok(None)
    }
}

fn named_identity(parent: &File, name: &std::ffi::CStr, owned: &File) -> bool {
    let Ok(metadata) = owned.metadata() else {
        return false;
    };
    // SAFETY: parent is live and name is a NUL-terminated single component.
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NONBLOCK | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return false;
    }
    // SAFETY: successful openat returned a newly owned descriptor.
    let named = unsafe { File::from_raw_fd(fd) };
    named
        .metadata()
        .is_ok_and(|named| named.dev() == metadata.dev() && named.ino() == metadata.ino())
}
