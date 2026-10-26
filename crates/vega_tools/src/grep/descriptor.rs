use std::ffi::{CStr, CString, OsStr, OsString};
use std::fs::{File, Metadata};
use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::ptr::NonNull;

use crate::{ToolError, Tools};

use super::{check_cancelled, invalid};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Identity {
    device: u64,
    inode: u64,
    kind: libc::mode_t,
}

impl Identity {
    fn from_metadata(metadata: &Metadata) -> Self {
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            kind: metadata.mode() as libc::mode_t & libc::S_IFMT,
        }
    }

    fn from_stat(metadata: &libc::stat) -> Self {
        Self {
            device: metadata.st_dev as u64,
            inode: metadata.st_ino,
            kind: metadata.st_mode & libc::S_IFMT,
        }
    }

    pub(super) fn is_directory(self) -> bool {
        self.kind == libc::S_IFDIR
    }

    pub(super) fn is_file(self) -> bool {
        self.kind == libc::S_IFREG
    }
}

#[derive(Clone)]
pub(super) struct CredentialGuard {
    roots: Vec<PathBuf>,
    identities: Vec<(u64, u64)>,
}

impl CredentialGuard {
    pub(super) fn new(tools: &Tools) -> Self {
        let Some(policy) = &tools.credential_policy else {
            return Self {
                roots: Vec::new(),
                identities: Vec::new(),
            };
        };
        let mut roots = vec![policy.protected_root.clone()];
        let mut identities = vec![policy.protected_identity];
        if let Ok(current) = policy.protected_root.canonicalize() {
            if let Ok(metadata) = current.metadata() {
                identities.push((metadata.dev(), metadata.ino()));
            }
            roots.push(current);
        }
        Self { roots, identities }
    }

    pub(super) fn merged(&self, current: Self) -> Self {
        let mut merged = self.clone();
        merged.roots.extend(current.roots);
        merged.identities.extend(current.identities);
        merged
    }

    pub(super) fn blocks_path(&self, path: &Path) -> bool {
        self.roots.iter().any(|root| path.starts_with(root))
    }

    pub(super) fn overlaps(&self, path: &Path) -> bool {
        self.roots
            .iter()
            .any(|root| path.starts_with(root) || root.starts_with(path))
    }

    pub(super) fn blocks_identity(&self, identity: Identity) -> bool {
        self.identities.contains(&(identity.device, identity.inode))
    }

    pub(super) fn blocks_chain(&self, chain: &[Identity]) -> bool {
        chain.iter().any(|identity| self.blocks_identity(*identity))
    }

    fn blocks_metadata(&self, metadata: &Metadata) -> bool {
        self.blocks_identity(Identity::from_metadata(metadata))
            || (!self.roots.is_empty() && metadata.is_file() && metadata.nlink() != 1)
    }

    pub(super) fn blocks_file(&self, file: &File) -> Result<bool, ToolError> {
        Ok(self.blocks_metadata(&file.metadata()?))
    }

    pub(super) fn allows_project_entry(&self, path: &Path) -> bool {
        if self.blocks_path(path) || super::contains_git_component(path) {
            return false;
        }
        path.symlink_metadata().is_ok_and(|metadata| {
            !metadata.file_type().is_symlink() && !self.blocks_metadata(&metadata)
        })
    }
}

pub(super) struct Target {
    pub(super) file: File,
    pub(super) chain: Vec<Identity>,
}

pub(super) fn capture(path: &Path, guard: &CredentialGuard) -> Result<Target, ToolError> {
    if !path.is_absolute() || guard.blocks_path(path) {
        return Err(invalid("grep target is protected"));
    }
    let mut file = File::open("/")?;
    let mut chain = vec![Identity::from_metadata(&file.metadata()?)];
    for component in path.components() {
        let Component::Normal(name) = component else {
            if component == Component::RootDir {
                continue;
            }
            return Err(invalid("grep target is not canonical"));
        };
        let entry = snapshot(&file, name)?;
        if guard.blocks_identity(entry.identity) {
            return Err(invalid("grep target is protected"));
        }
        file = open_child(&file, &entry)?;
        chain.push(entry.identity);
    }
    if guard.blocks_chain(&chain) || guard.blocks_file(&file)? {
        return Err(invalid("grep target is protected"));
    }
    Ok(Target { file, chain })
}

pub(super) fn verify(
    path: &Path,
    target: &File,
    chain: &[Identity],
    guard: &CredentialGuard,
) -> Result<(), ToolError> {
    let current =
        capture(path, guard).map_err(|_| invalid("grep target changed before execution"))?;
    if current.chain != chain
        || chain.last() != Some(&Identity::from_metadata(&target.metadata()?))
        || guard.blocks_file(target)?
    {
        return Err(invalid("grep target changed before execution"));
    }
    Ok(())
}

pub(super) struct Entry {
    pub(super) name: OsString,
    pub(super) identity: Identity,
    links: u64,
}

impl Entry {
    pub(super) fn is_protected(&self, guard: &CredentialGuard) -> bool {
        guard.blocks_identity(self.identity)
            || (!guard.roots.is_empty() && self.identity.is_file() && self.links != 1)
    }
}

fn component_name(name: &OsStr) -> Result<CString, ToolError> {
    if name.as_bytes().contains(&b'/') || matches!(name.as_bytes(), b"." | b"..") {
        return Err(invalid("invalid grep path component"));
    }
    CString::new(name.as_bytes()).map_err(|_| invalid("invalid grep path component"))
}

fn snapshot(directory: &File, name: &OsStr) -> Result<Entry, ToolError> {
    let component = component_name(name)?;
    let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: directory is live, component is a single NUL-terminated name, and metadata is writable.
    let result = unsafe {
        libc::fstatat(
            directory.as_raw_fd(),
            component.as_ptr(),
            metadata.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: fstatat succeeded and initialized the entire stat value.
    let metadata = unsafe { metadata.assume_init() };
    Ok(Entry {
        name: name.to_os_string(),
        identity: Identity::from_stat(&metadata),
        links: metadata.st_nlink as u64,
    })
}

pub(super) fn open_child(directory: &File, entry: &Entry) -> Result<File, ToolError> {
    if !entry.identity.is_file() && !entry.identity.is_directory() {
        return Err(invalid("grep target must be a regular file or directory"));
    }
    let name = component_name(&entry.name)?;
    let flags = libc::O_RDONLY
        | libc::O_NOFOLLOW
        | libc::O_NONBLOCK
        | libc::O_CLOEXEC
        | if entry.identity.is_directory() {
            libc::O_DIRECTORY
        } else {
            0
        };
    // SAFETY: directory is live and name is a NUL-terminated single component opened without following links.
    let descriptor = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags) };
    if descriptor < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: successful openat returned a new descriptor owned only by this File.
    let file = unsafe { File::from_raw_fd(descriptor) };
    if Identity::from_metadata(&file.metadata()?) != entry.identity {
        return Err(invalid("grep entry changed during traversal"));
    }
    Ok(file)
}

pub(super) fn open_relative(
    directory: &File,
    path: &Path,
    guard: &CredentialGuard,
) -> Result<Option<File>, ToolError> {
    let mut file = directory.try_clone()?;
    for component in path.components() {
        let Component::Normal(name) = component else {
            return Err(invalid("invalid grep relative path"));
        };
        let entry = snapshot(&file, name)?;
        if entry.is_protected(guard) {
            return Ok(None);
        }
        file = open_child(&file, &entry)?;
    }
    if guard.blocks_file(&file)? {
        return Ok(None);
    }
    if !file.metadata()?.is_file() {
        return Err(invalid("grep target must be a regular file"));
    }
    Ok(Some(file))
}

struct DirectoryStream(NonNull<libc::DIR>);

impl Drop for DirectoryStream {
    fn drop(&mut self) {
        // SAFETY: this stream owns the live DIR pointer returned by fdopendir and closes it exactly once.
        unsafe { libc::closedir(self.0.as_ptr()) };
    }
}

#[cfg(target_vendor = "apple")]
fn directory_errno() -> *mut libc::c_int {
    // SAFETY: __error returns the current thread's valid errno storage.
    unsafe { libc::__error() }
}

#[cfg(not(target_vendor = "apple"))]
fn directory_errno() -> *mut libc::c_int {
    // SAFETY: __errno_location returns the current thread's valid errno storage.
    unsafe { libc::__errno_location() }
}

pub(super) fn entries(
    directory: &File,
    cancel: &CancellationToken,
) -> Result<Vec<Entry>, ToolError> {
    let descriptor = directory.try_clone()?.into_raw_fd();
    // SAFETY: descriptor is an owned duplicate of a live directory descriptor transferred on success.
    let pointer = unsafe { libc::fdopendir(descriptor) };
    let Some(pointer) = NonNull::new(pointer) else {
        let error = std::io::Error::last_os_error();
        // SAFETY: fdopendir failed without taking ownership of descriptor.
        drop(unsafe { File::from_raw_fd(descriptor) });
        return Err(error.into());
    };
    let stream = DirectoryStream(pointer);
    let mut entries = Vec::new();
    loop {
        check_cancelled(cancel)?;
        // SAFETY: directory_errno returns this thread's writable errno storage.
        unsafe { *directory_errno() = 0 };
        // SAFETY: stream owns a live DIR pointer and no other caller accesses it.
        let entry = unsafe { libc::readdir(stream.0.as_ptr()) };
        if entry.is_null() {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error().is_some_and(|code| code != 0) {
                return Err(error.into());
            }
            break;
        }
        // SAFETY: readdir returned a live entry with a NUL-terminated d_name, consumed before the next call.
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
        if matches!(name.to_bytes(), b"." | b"..") {
            continue;
        }
        let name = OsString::from_vec(name.to_bytes().to_vec());
        match snapshot(directory, &name) {
            Ok(entry) => entries.push(entry),
            Err(ToolError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(entries)
}
