//! Owner-only plaintext local credentials, separate from config.toml.
//! The caller supplies the configuration root; OS credential stores are never accessed.
//! Unix descriptor-relative IO rejects links and insecure existing storage.

use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path, sync::Mutex};

const MAX_BYTES: u64 = 1024 * 1024;
const MAX_ENTRIES: usize = 256;
static LOCK: Mutex<()> = Mutex::new(());

/// Content-free credential errors: never contain file contents or secret values.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("本地凭据不存在，请在设置中重新输入 API Key")]
    Missing,
    #[error("本地凭据无法读取或保存，请检查访问权限")]
    Io,
    #[error("本地凭据文件格式损坏，原文件已保留")]
    Malformed,
    #[error("本地凭据超过大小限制")]
    Limit,
    #[error("本地凭据路径、类型或权限不安全")]
    Unsafe,
    #[error("当前平台尚不支持安全的本地凭据存储")]
    Unsupported,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    keys: BTreeMap<String, String>,
}

fn validate_ref(name: &str) -> Result<(), Error> {
    if name.is_empty()
        || name.len() > 256
        || name.contains(['/', '\\', '\0'])
        || matches!(name, "." | "..")
    {
        return Err(Error::Unsafe);
    }
    Ok(())
}

fn validate(document: &Document) -> Result<(), Error> {
    if document.keys.len() > MAX_ENTRIES {
        return Err(Error::Limit);
    }
    for (name, key) in &document.keys {
        validate_ref(name)?;
        if key.is_empty() || key.len() > 64 * 1024 {
            return Err(Error::Limit);
        }
    }
    Ok(())
}

/// Store a key under the explicitly resolved configuration root.
pub fn set_key(root: &Path, name: &str, key: &str) -> Result<(), Error> {
    validate_ref(name)?;
    if key.is_empty() || key.len() > 64 * 1024 {
        return Err(Error::Limit);
    }
    let _guard = LOCK.lock().map_err(|_| Error::Io)?;
    let directory = platform::directory(root, true)?;
    let mut document = platform::read(&directory)?;
    document.keys.insert(name.into(), key.into());
    validate(&document)?;
    platform::write(&directory, &document)
}

/// Read a key from the same explicit root used by Settings.
pub fn get_key(root: &Path, name: &str) -> Result<String, Error> {
    validate_ref(name)?;
    let _guard = LOCK.lock().map_err(|_| Error::Io)?;
    let directory = platform::directory(root, false)?;
    platform::read(&directory)?
        .keys
        .remove(name)
        .ok_or(Error::Missing)
}

/// Remove a key atomically; missing keys remain a typed error.
pub fn delete_key(root: &Path, name: &str) -> Result<(), Error> {
    validate_ref(name)?;
    let _guard = LOCK.lock().map_err(|_| Error::Io)?;
    let directory = platform::directory(root, false)?;
    let mut document = platform::read(&directory)?;
    document.keys.remove(name).ok_or(Error::Missing)?;
    platform::write(&directory, &document)
}

/// Return only available reference names; callers never retain secret values for UI badges.
pub fn available_refs(root: &Path) -> Result<Vec<String>, Error> {
    let _guard = LOCK.lock().map_err(|_| Error::Io)?;
    let directory = platform::directory(root, false)?;
    Ok(platform::read(&directory)?.keys.into_keys().collect())
}

#[cfg(unix)]
mod platform {
    use super::*;
    use std::{
        ffi::CStr,
        fs::File,
        io::{Read, Write},
        os::fd::{AsRawFd, FromRawFd},
        os::unix::fs::{MetadataExt, OpenOptionsExt},
        path::Component,
    };
    const NAME: &CStr = c"credentials.toml";

    fn io_error() -> Error {
        match std::io::Error::last_os_error().raw_os_error() {
            Some(libc::ENOENT) => Error::Missing,
            Some(libc::ELOOP | libc::ENOTDIR) => Error::Unsafe,
            _ => Error::Io,
        }
    }

    fn open_at(dir: &File, name: &CStr, flags: i32) -> Result<File, Error> {
        // SAFETY: directory is live, name is NUL terminated, returned fd becomes uniquely owned.
        let fd = unsafe {
            libc::openat(
                dir.as_raw_fd(),
                name.as_ptr(),
                flags | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
                0o600,
            )
        };
        if fd < 0 {
            return Err(io_error());
        }
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    fn check(file: &File, directory: bool) -> Result<(), Error> {
        let metadata = file.metadata().map_err(|_| Error::Io)?;
        // SAFETY: geteuid has no arguments or memory safety requirements.
        let owner = unsafe { libc::geteuid() };
        if metadata.uid() != owner
            || metadata.mode() & 0o7777 != if directory { 0o700 } else { 0o600 }
            || if directory {
                !metadata.is_dir()
            } else {
                !metadata.is_file() || metadata.nlink() != 1
            }
        {
            return Err(Error::Unsafe);
        }
        Ok(())
    }

    pub(super) fn directory(root: &Path, create: bool) -> Result<File, Error> {
        if !root.is_absolute() || root.components().any(|c| matches!(c, Component::ParentDir)) {
            return Err(Error::Unsafe);
        }
        if create {
            std::fs::create_dir_all(root).map_err(|_| Error::Io)?;
        }
        // Config root is a trusted caller-selected anchor, including macOS /tmp alias.
        let root = root.canonicalize().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::Missing
            } else {
                Error::Io
            }
        })?;
        let anchor = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(root)
            .map_err(|_| Error::Unsafe)?;
        if create {
            // SAFETY: live directory fd and constant NUL terminated child name.
            let result =
                unsafe { libc::mkdirat(anchor.as_raw_fd(), c"credentials".as_ptr(), 0o700) };
            if result != 0 && std::io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST) {
                return Err(Error::Io);
            }
        }
        let directory = open_at(&anchor, c"credentials", libc::O_RDONLY | libc::O_DIRECTORY)?;
        check(&directory, true)?;
        Ok(directory)
    }

    pub(super) fn read(directory: &File) -> Result<Document, Error> {
        let file = match open_at(directory, NAME, libc::O_RDONLY) {
            Ok(file) => file,
            Err(Error::Missing) => return Ok(Document::default()),
            Err(error) => return Err(error),
        };
        check(&file, false)?;
        if file.metadata().map_err(|_| Error::Io)?.len() > MAX_BYTES {
            return Err(Error::Limit);
        }
        let mut bytes = Vec::new();
        file.take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| Error::Io)?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(Error::Limit);
        }
        let text = std::str::from_utf8(&bytes).map_err(|_| Error::Malformed)?;
        let document: Document = toml::from_str(text).map_err(|_| Error::Malformed)?;
        validate(&document)?;
        Ok(document)
    }

    pub(super) fn write(directory: &File, document: &Document) -> Result<(), Error> {
        let body = toml::to_string(document).map_err(|_| Error::Malformed)?;
        if body.len() as u64 > MAX_BYTES {
            return Err(Error::Limit);
        }
        let name = std::ffi::CString::new(format!(".credentials-{}.tmp", ulid::Ulid::generate()))
            .map_err(|_| Error::Io)?;
        let mut file = open_at(
            directory,
            &name,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
        )?;
        let result = (|| {
            check(&file, false)?;
            file.write_all(body.as_bytes()).map_err(|_| Error::Io)?;
            file.sync_all().map_err(|_| Error::Io)?;
            // Revalidate target before replacement; rename never follows target symlinks.
            read(directory)?;
            // SAFETY: live directory and NUL terminated names, no borrowed fd is closed.
            if unsafe {
                libc::renameat(
                    directory.as_raw_fd(),
                    name.as_ptr(),
                    directory.as_raw_fd(),
                    NAME.as_ptr(),
                )
            } != 0
            {
                return Err(Error::Io);
            }
            directory.sync_all().map_err(|_| Error::Io)
        })();
        // SAFETY: removes only the exclusively-created, uniquely-named temporary entry.
        unsafe {
            libc::unlinkat(directory.as_raw_fd(), name.as_ptr(), 0);
        }
        result
    }
}

#[cfg(not(unix))]
mod platform {
    use super::*;
    pub(super) fn directory(_: &Path, _: bool) -> Result<(), Error> {
        Err(Error::Unsupported)
    }
    pub(super) fn read(_: &()) -> Result<Document, Error> {
        Err(Error::Unsupported)
    }
    pub(super) fn write(_: &(), _: &Document) -> Result<(), Error> {
        Err(Error::Unsupported)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::{PermissionsExt, symlink},
    };

    #[test]
    fn owned_crud_and_concurrent_updates() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(get_key(root.path(), "one"), Err(Error::Missing));
        set_key(root.path(), "one", "test-value").unwrap();
        set_key(root.path(), "one", "replacement").unwrap();
        assert_eq!(get_key(root.path(), "one").unwrap(), "replacement");
        std::thread::scope(|scope| {
            for i in 0..16 {
                let root = root.path();
                scope
                    .spawn(move || set_key(root, &format!("ref-{i}"), "owned-test-value").unwrap());
            }
        });
        assert_eq!(available_refs(root.path()).unwrap().len(), 17);
        delete_key(root.path(), "one").unwrap();
        assert_eq!(delete_key(root.path(), "one"), Err(Error::Missing));
        assert_eq!(
            fs::metadata(root.path().join("credentials"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(root.path().join("credentials/credentials.toml"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    #[test]
    fn malformed_and_oversized_preserved_without_secret_errors() {
        let root = tempfile::tempdir().unwrap();
        set_key(root.path(), "a", "test-value").unwrap();
        let path = root.path().join("credentials/credentials.toml");
        let body = "not TOML private-sentinel";
        fs::write(&path, body).unwrap();
        let error = set_key(root.path(), "b", "new-sentinel").unwrap_err();
        assert_eq!(error, Error::Malformed);
        assert!(!format!("{error:?} {error}").contains("sentinel"));
        assert_eq!(fs::read_to_string(&path).unwrap(), body);
        fs::write(&path, vec![b'x'; MAX_BYTES as usize + 1]).unwrap();
        assert_eq!(set_key(root.path(), "b", "value"), Err(Error::Limit));
        assert_eq!(fs::metadata(&path).unwrap().len(), MAX_BYTES + 1);
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
    }

    #[test]
    fn rejects_links_traversal_and_insecure_modes() {
        let root = tempfile::tempdir().unwrap();
        set_key(root.path(), "a", "test-value").unwrap();
        let path = root.path().join("credentials/credentials.toml");
        for name in ["../escape", "a/b", "..", ""] {
            assert_eq!(set_key(root.path(), name, "value"), Err(Error::Unsafe));
        }
        assert_eq!(
            get_key(&root.path().join("../root"), "a"),
            Err(Error::Unsafe)
        );
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(get_key(root.path(), "a"), Err(Error::Unsafe));
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let other = root.path().join("other");
        fs::rename(&path, &other).unwrap();
        symlink(&other, &path).unwrap();
        assert_eq!(set_key(root.path(), "a", "value"), Err(Error::Unsafe));
        fs::remove_file(&path).unwrap();
        fs::hard_link(&other, &path).unwrap();
        assert_eq!(get_key(root.path(), "a"), Err(Error::Unsafe));
        fs::remove_file(&path).unwrap();
        fs::remove_dir(root.path().join("credentials")).unwrap();
        symlink(root.path(), root.path().join("credentials")).unwrap();
        assert_eq!(get_key(root.path(), "a"), Err(Error::Unsafe));
    }
}
