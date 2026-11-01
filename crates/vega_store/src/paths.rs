use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AppPathsError {
    #[error("VEGA_HOME must be a nonempty absolute path without parent traversal")]
    InvalidOverride,
    #[error("an absolute user home directory is required")]
    HomeUnavailable,
    #[error("unsafe Vega path: {0}")]
    UnsafePath(PathBuf),
    #[error("Vega path is inaccessible: {0}")]
    InaccessiblePath(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    root: PathBuf,
    explicit: bool,
}

impl AppPaths {
    pub fn resolve() -> Result<Self, AppPathsError> {
        let vega_home = std::env::var_os("VEGA_HOME");
        if vega_home.is_some() {
            return Self::from_home(None, vega_home.as_deref());
        }
        Self::from_home(std::env::home_dir().as_deref(), None)
    }

    pub fn from_home(
        user_home: Option<&Path>,
        vega_home: Option<&OsStr>,
    ) -> Result<Self, AppPathsError> {
        if let Some(value) = vega_home {
            let path = Path::new(value);
            if !valid_home_path(path) {
                return Err(AppPathsError::InvalidOverride);
            }
            return Ok(Self {
                root: path.to_path_buf(),
                explicit: true,
            });
        }
        let home = user_home
            .filter(|path| valid_home_path(path))
            .ok_or(AppPathsError::HomeUnavailable)?;
        Ok(Self {
            root: home.join(".vega"),
            explicit: false,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn is_explicit(&self) -> bool {
        self.explicit
    }

    pub fn config_file(&self) -> PathBuf {
        self.root.join("config.yaml")
    }

    pub fn credentials_dir(&self) -> PathBuf {
        self.root.join("credentials")
    }

    pub fn data_dir(&self) -> PathBuf {
        self.root.join("data")
    }

    pub fn database_file(&self) -> PathBuf {
        self.data_dir().join("vega.db")
    }

    pub fn scratch_dir(&self) -> PathBuf {
        self.data_dir().join("scratch")
    }

    pub fn skills_dir(&self) -> PathBuf {
        self.root.join("skills")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.root.join("cache")
    }

    pub fn run_dir(&self) -> PathBuf {
        self.root.join("run")
    }

    pub fn backups_dir(&self) -> PathBuf {
        self.root.join("backups")
    }

    pub fn validate_existing(&self) -> Result<(), AppPathsError> {
        for directory in [
            self.root.clone(),
            self.data_dir(),
            self.credentials_dir(),
            self.scratch_dir(),
            self.skills_dir(),
            self.logs_dir(),
            self.cache_dir(),
            self.run_dir(),
            self.backups_dir(),
        ] {
            validate_directory_path(&directory)?;
        }
        for file in [self.config_file(), self.database_file()] {
            match std::fs::symlink_metadata(&file) {
                Ok(metadata) if metadata.is_file() && !is_link(&metadata) => {}
                Ok(_) => return Err(AppPathsError::UnsafePath(file)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(AppPathsError::InaccessiblePath(file)),
            }
        }
        Ok(())
    }

    pub fn initialize_data_dir(&self) -> Result<(), AppPathsError> {
        self.validate_existing()?;
        create_private_directory(self.root())?;
        create_private_directory(&self.data_dir())?;
        self.validate_existing()
    }
}

fn valid_home_path(path: &Path) -> bool {
    path.is_absolute()
        && !path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
}

fn is_link(metadata: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    metadata.file_type().is_symlink()
}

fn validate_directory_path(path: &Path) -> Result<(), AppPathsError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        if matches!(component, Component::Prefix(_)) {
            continue;
        }
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.is_dir() && !is_link(&metadata) => {}
            Ok(_) => return Err(AppPathsError::UnsafePath(current)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(_) => return Err(AppPathsError::InaccessiblePath(current)),
        }
    }
    std::fs::read_dir(path).map_err(|_| AppPathsError::InaccessiblePath(path.to_path_buf()))?;
    Ok(())
}

#[cfg(unix)]
fn create_private_directory(path: &Path) -> Result<(), AppPathsError> {
    use std::ffi::CString;
    use std::fs::{File, OpenOptions};
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

    let inaccessible = || AppPathsError::InaccessiblePath(path.to_path_buf());
    let mut directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open("/")
        .map_err(|_| inaccessible())?;
    for component in path.components() {
        let Component::Normal(name) = component else {
            continue;
        };
        let name = CString::new(name.as_bytes()).map_err(|_| inaccessible())?;
        // SAFETY: directory is live and name is a valid NUL-terminated path component.
        let mut fd = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT) {
            // SAFETY: directory is live and name is a valid NUL-terminated path component.
            let result = unsafe { libc::mkdirat(directory.as_raw_fd(), name.as_ptr(), 0o700) };
            if result != 0 && std::io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST) {
                return Err(inaccessible());
            }
            // SAFETY: directory is live and name is a valid NUL-terminated path component.
            fd = unsafe {
                libc::openat(
                    directory.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
        }
        if fd < 0 {
            return Err(inaccessible());
        }
        // SAFETY: a successful openat returned a new descriptor owned by this File.
        directory = unsafe { File::from_raw_fd(fd) };
    }
    let metadata = directory.metadata().map_err(|_| inaccessible())?;
    // SAFETY: geteuid takes no arguments and reads the effective user identity.
    let owner = unsafe { libc::geteuid() };
    if metadata.uid() != owner || metadata.mode() & 0o022 != 0 {
        return Err(AppPathsError::UnsafePath(path.to_path_buf()));
    }
    if metadata.mode() & 0o700 != 0o700 {
        return Err(inaccessible());
    }
    Ok(())
}

#[cfg(not(unix))]
fn create_private_directory(path: &Path) -> Result<(), AppPathsError> {
    validate_directory_path(path)?;
    std::fs::create_dir_all(path)
        .map_err(|_| AppPathsError::InaccessiblePath(path.to_path_buf()))?;
    validate_directory_path(path)
}

pub fn config_dir() -> Option<PathBuf> {
    AppPaths::resolve().ok().map(|paths| paths.root)
}

pub fn data_dir() -> Option<PathBuf> {
    AppPaths::resolve().ok().map(|paths| paths.data_dir())
}
