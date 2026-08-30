//! macOS Seatbelt launch boundary and pre-spawn hardlink scan.

use std::fs::{self, File};
use std::io::Read;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command as StdCommand, Stdio};

use tokio::process::{Child, Command};

use crate::error::{BashError, BashErrorCode};
use crate::fence::discover_git_dir;

const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";
const SHELL: &str = "/bin/zsh";
const TEMP_BASE: &str = "/private/tmp";
const PROFILE: &str = r#"(version 1)
(allow default)
(deny file-write*)
(allow file-write* (subpath (param "PROJECT_ROOT")))
(allow file-write* (subpath (param "TEMP_ROOT")))
(deny file-write* (subpath (param "GIT_ENTRY")))
(deny file-write* (subpath (param "GIT_DIR")))
(allow network*)"#;

pub(crate) struct SandboxSelfTest {
    pub(crate) result: Result<(), BashError>,
    pub(crate) cleanup_safe: bool,
}

pub(crate) struct SandboxConfig {
    project_root: PathBuf,
    project_param: String,
    git_entry: PathBuf,
    git_entry_param: String,
    git_dir: PathBuf,
    git_dir_param: String,
}

impl SandboxConfig {
    pub(crate) fn new(project_root: &Path) -> Result<Self, BashError> {
        let metadata = fs::symlink_metadata(SANDBOX_EXEC)
            .map_err(|_| BashError::new(BashErrorCode::SandboxUnavailable))?;
        if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
            return Err(BashError::new(BashErrorCode::SandboxUnavailable));
        }
        let discovered = discover_git_dir(project_root)
            .map_err(|_| BashError::new(BashErrorCode::SandboxUnavailable))?;
        let git_entry = project_root.join(".git");
        let git_dir = discovered.unwrap_or_else(|| git_entry.clone());
        let project_param = utf8_param(project_root)?;
        let git_entry_param = utf8_param(&git_entry)?;
        let git_dir_param = utf8_param(&git_dir)?;
        Ok(Self {
            project_root: project_root.to_path_buf(),
            project_param,
            git_entry,
            git_entry_param,
            git_dir,
            git_dir_param,
        })
    }

    pub(crate) fn preflight(
        &self,
        temp_root: &TempRoot,
        hooks: &ExecutionHooks,
    ) -> Result<(), BashError> {
        hooks.maybe_fail_traversal()?;
        let mut metadata_seen = 0_usize;
        scan_hardlinks(
            &self.project_root,
            Some((&self.git_entry, &self.git_dir)),
            hooks,
            &mut metadata_seen,
        )?;
        temp_root.validate_identity()?;
        scan_hardlinks(temp_root.path(), None, hooks, &mut metadata_seen)?;
        temp_root.validate_identity()
    }

    pub(crate) async fn self_test(
        &self,
        temp_root: &TempRoot,
        hooks: &ExecutionHooks,
    ) -> SandboxSelfTest {
        let mut command = self.command(hooks.profile(), temp_root);
        command.arg("--").arg("/usr/bin/true");
        command.stdin(Stdio::null());
        command.stdout(Stdio::null());
        command.stderr(Stdio::null());
        command.current_dir(&self.project_root);
        command.process_group(0);
        hooks.note_spawn();
        let mut command = Command::from(command);
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(_) => {
                return SandboxSelfTest {
                    result: Err(BashError::new(BashErrorCode::SandboxUnavailable)),
                    cleanup_safe: true,
                };
            }
        };
        match child.wait().await {
            Ok(status) if status.success() => SandboxSelfTest {
                result: Ok(()),
                cleanup_safe: true,
            },
            Ok(_) => SandboxSelfTest {
                result: Err(BashError::new(BashErrorCode::SandboxUnavailable)),
                cleanup_safe: true,
            },
            Err(_) => SandboxSelfTest {
                result: Err(BashError::new(BashErrorCode::SandboxUnavailable)),
                cleanup_safe: false,
            },
        }
    }

    pub(crate) fn spawn_shell(
        &self,
        command_text: &str,
        temp_root: &TempRoot,
        hooks: &ExecutionHooks,
    ) -> Result<Child, BashError> {
        let mut command = self.command(hooks.profile(), temp_root);
        command
            .arg("--")
            .arg(SHELL)
            .arg("-lc")
            .arg(format!("exec 2>&1\n{command_text}"));
        command.current_dir(&self.project_root);
        command.stdin(Stdio::null());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::null());
        command.process_group(0);
        hooks.note_spawn();
        let mut command = Command::from(command);
        command.kill_on_drop(true);
        command
            .spawn()
            .map_err(|_| BashError::new(BashErrorCode::SpawnFailed))
    }

    fn command(&self, profile: &str, temp_root: &TempRoot) -> StdCommand {
        let mut command = StdCommand::new(SANDBOX_EXEC);
        command
            .arg("-D")
            .arg(format!("PROJECT_ROOT={}", self.project_param))
            .arg("-D")
            .arg(format!("TEMP_ROOT={}", temp_root.param()))
            .arg("-D")
            .arg(format!("GIT_ENTRY={}", self.git_entry_param))
            .arg("-D")
            .arg(format!("GIT_DIR={}", self.git_dir_param))
            .arg("-p")
            .arg(profile)
            .env("TMPDIR", temp_root.param())
            .env("TMP", temp_root.param())
            .env("TEMP", temp_root.param())
            .env("TEMPDIR", temp_root.param());
        command
    }
}

pub(crate) struct TempRoot {
    path: PathBuf,
    base: PathBuf,
    param: String,
    dev: u64,
    ino: u64,
}

impl TempRoot {
    pub(crate) fn create() -> Result<Self, BashError> {
        let base = fs::canonicalize(TEMP_BASE)
            .map_err(|_| BashError::new(BashErrorCode::TempUnavailable))?;
        let base_metadata = fs::symlink_metadata(&base)
            .map_err(|_| BashError::new(BashErrorCode::TempUnavailable))?;
        if base_metadata.file_type().is_symlink() || !base_metadata.is_dir() {
            return Err(BashError::new(BashErrorCode::TempUnavailable));
        }

        for _ in 0..16 {
            let path = base.join(random_temp_name()?);
            let mut builder = fs::DirBuilder::new();
            builder.mode(0o700);
            match builder.create(&path) {
                Ok(()) => return Self::validate_created(path, base),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => return Err(BashError::new(BashErrorCode::TempUnavailable)),
            }
        }
        Err(BashError::new(BashErrorCode::TempUnavailable))
    }

    fn validate_created(path: PathBuf, base: PathBuf) -> Result<Self, BashError> {
        let result = (|| {
            let link_metadata = fs::symlink_metadata(&path)
                .map_err(|_| BashError::new(BashErrorCode::TempUnavailable))?;
            if link_metadata.file_type().is_symlink()
                || !link_metadata.is_dir()
                || link_metadata.permissions().mode() & 0o777 != 0o700
            {
                return Err(BashError::new(BashErrorCode::TempUnavailable));
            }
            let canonical = fs::canonicalize(&path)
                .map_err(|_| BashError::new(BashErrorCode::TempUnavailable))?;
            if canonical.parent() != Some(base.as_path()) || canonical != path {
                return Err(BashError::new(BashErrorCode::TempUnavailable));
            }
            let metadata = fs::metadata(&canonical)
                .map_err(|_| BashError::new(BashErrorCode::TempUnavailable))?;
            if link_metadata.dev() != metadata.dev() || link_metadata.ino() != metadata.ino() {
                return Err(BashError::new(BashErrorCode::TempUnavailable));
            }
            let param = utf8_param(&canonical)
                .map_err(|_| BashError::new(BashErrorCode::TempUnavailable))?;
            Ok(Self {
                path: canonical,
                base,
                param,
                dev: metadata.dev(),
                ino: metadata.ino(),
            })
        })();
        if result.is_err() {
            let _ = fs::remove_dir(&path);
        }
        result
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn param(&self) -> &str {
        &self.param
    }

    fn validate_identity(&self) -> Result<(), BashError> {
        let link_metadata = fs::symlink_metadata(&self.path)
            .map_err(|_| BashError::new(BashErrorCode::TempUnavailable))?;
        if link_metadata.file_type().is_symlink()
            || !link_metadata.is_dir()
            || link_metadata.dev() != self.dev
            || link_metadata.ino() != self.ino
        {
            return Err(BashError::new(BashErrorCode::TempUnavailable));
        }
        let canonical = fs::canonicalize(&self.path)
            .map_err(|_| BashError::new(BashErrorCode::TempUnavailable))?;
        if canonical != self.path || canonical.parent() != Some(self.base.as_path()) {
            return Err(BashError::new(BashErrorCode::TempUnavailable));
        }
        Ok(())
    }

    pub(crate) fn cleanup(&self) -> Result<(), BashError> {
        // Move only the originally recorded inode into a new private holder.
        // A replacement symlink or directory fails the payload identity check,
        // so cleanup never traverses an attacker-selected root.
        self.validate_identity()
            .map_err(|_| BashError::new(BashErrorCode::CleanupFailed))?;
        let holder = CleanupHolder::create(&self.base)?;
        let payload = holder.path.join("payload");
        fs::rename(&self.path, &payload)
            .map_err(|_| BashError::new(BashErrorCode::CleanupFailed))?;
        let payload_metadata = fs::symlink_metadata(&payload)
            .map_err(|_| BashError::new(BashErrorCode::CleanupFailed))?;
        if payload_metadata.file_type().is_symlink()
            || !payload_metadata.is_dir()
            || payload_metadata.dev() != self.dev
            || payload_metadata.ino() != self.ino
        {
            return Err(BashError::new(BashErrorCode::CleanupFailed));
        }
        holder.validate_identity()?;
        // On macOS std implements remove_dir_all with openat(O_NOFOLLOW) and
        // unlinkat, including protection against nested symlink-swap races.
        fs::remove_dir_all(&holder.path).map_err(|_| BashError::new(BashErrorCode::CleanupFailed))
    }
}

struct CleanupHolder {
    path: PathBuf,
    base: PathBuf,
    dev: u64,
    ino: u64,
}

impl CleanupHolder {
    fn create(base: &Path) -> Result<Self, BashError> {
        for _ in 0..16 {
            let name = random_name(".vega-cleanup-")
                .map_err(|_| BashError::new(BashErrorCode::CleanupFailed))?;
            let path = base.join(name);
            let mut builder = fs::DirBuilder::new();
            builder.mode(0o700);
            match builder.create(&path) {
                Ok(()) => {
                    let metadata = fs::symlink_metadata(&path)
                        .map_err(|_| BashError::new(BashErrorCode::CleanupFailed))?;
                    if metadata.file_type().is_symlink()
                        || !metadata.is_dir()
                        || metadata.permissions().mode() & 0o777 != 0o700
                    {
                        return Err(BashError::new(BashErrorCode::CleanupFailed));
                    }
                    return Ok(Self {
                        path,
                        base: base.to_path_buf(),
                        dev: metadata.dev(),
                        ino: metadata.ino(),
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => return Err(BashError::new(BashErrorCode::CleanupFailed)),
            }
        }
        Err(BashError::new(BashErrorCode::CleanupFailed))
    }

    fn validate_identity(&self) -> Result<(), BashError> {
        let metadata = fs::symlink_metadata(&self.path)
            .map_err(|_| BashError::new(BashErrorCode::CleanupFailed))?;
        if metadata.file_type().is_symlink()
            || !metadata.is_dir()
            || metadata.dev() != self.dev
            || metadata.ino() != self.ino
        {
            return Err(BashError::new(BashErrorCode::CleanupFailed));
        }
        let canonical = fs::canonicalize(&self.path)
            .map_err(|_| BashError::new(BashErrorCode::CleanupFailed))?;
        if canonical != self.path || canonical.parent() != Some(self.base.as_path()) {
            return Err(BashError::new(BashErrorCode::CleanupFailed));
        }
        Ok(())
    }
}

fn utf8_param(path: &Path) -> Result<String, BashError> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| BashError::new(BashErrorCode::SandboxUnavailable))
}

fn random_temp_name() -> Result<String, BashError> {
    random_name(".vega-bash-")
}

fn random_name(prefix: &str) -> Result<String, BashError> {
    let mut random = [0_u8; 16];
    File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut random))
        .map_err(|_| BashError::new(BashErrorCode::TempUnavailable))?;
    let mut name = String::with_capacity(prefix.len() + random.len() * 2);
    name.push_str(prefix);
    for byte in random {
        use std::fmt::Write as _;
        write!(&mut name, "{byte:02x}")
            .map_err(|_| BashError::new(BashErrorCode::TempUnavailable))?;
    }
    Ok(name)
}

fn scan_hardlinks(
    root: &Path,
    project_git: Option<(&Path, &Path)>,
    hooks: &ExecutionHooks,
    metadata_seen: &mut usize,
) -> Result<(), BashError> {
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let directory_metadata = fs::symlink_metadata(&directory)
            .map_err(|_| BashError::new(BashErrorCode::HardlinkPreflight))?;
        if directory_metadata.file_type().is_symlink() || !directory_metadata.is_dir() {
            return Err(BashError::new(BashErrorCode::HardlinkPreflight));
        }
        let entries = fs::read_dir(&directory)
            .map_err(|_| BashError::new(BashErrorCode::HardlinkPreflight))?;
        for entry in entries {
            let entry = entry.map_err(|_| BashError::new(BashErrorCode::HardlinkPreflight))?;
            let path = entry.path();
            if let Some((git_entry, git_dir)) = project_git
                && (path == git_entry || path == git_dir || path.starts_with(git_dir))
            {
                continue;
            }
            hooks.maybe_fail_metadata(*metadata_seen)?;
            *metadata_seen = metadata_seen.saturating_add(1);
            let link_metadata = fs::symlink_metadata(&path)
                .map_err(|_| BashError::new(BashErrorCode::HardlinkPreflight))?;
            if link_metadata.file_type().is_symlink() {
                continue;
            }
            let metadata = fs::metadata(&path)
                .map_err(|_| BashError::new(BashErrorCode::HardlinkPreflight))?;
            if link_metadata.dev() != metadata.dev() || link_metadata.ino() != metadata.ino() {
                return Err(BashError::new(BashErrorCode::HardlinkPreflight));
            }
            if metadata.is_file() && metadata.nlink() > 1 {
                return Err(BashError::new(BashErrorCode::HardlinkPreflight));
            }
            if metadata.is_dir() {
                pending.push(path);
            }
        }
    }
    Ok(())
}

#[derive(Default)]
pub(crate) struct ExecutionHooks {
    #[cfg(test)]
    pub(crate) spawn_count: Option<std::sync::Arc<std::sync::atomic::AtomicUsize>>,
    #[cfg(test)]
    pub(crate) scan_failure: Option<ScanFailure>,
    #[cfg(test)]
    pub(crate) profile_override: Option<String>,
    #[cfg(test)]
    pub(crate) after_temp_created: Option<TestPathHook>,
    #[cfg(test)]
    pub(crate) before_cleanup: Option<TestPathHook>,
    #[cfg(test)]
    pub(crate) force_unconfirmed_reap: bool,
}

#[cfg(test)]
pub(crate) type TestPathHook = std::sync::Arc<dyn Fn(&Path) + Send + Sync>;

impl ExecutionHooks {
    fn profile(&self) -> &str {
        #[cfg(test)]
        if let Some(profile) = self.profile_override.as_deref() {
            return profile;
        }
        PROFILE
    }

    fn note_spawn(&self) {
        #[cfg(test)]
        if let Some(count) = &self.spawn_count {
            count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }

    pub(crate) fn note_temp_created(&self, _path: &Path) {
        #[cfg(test)]
        if let Some(hook) = &self.after_temp_created {
            hook(_path);
        }
    }

    pub(crate) fn note_before_cleanup(&self, _path: &Path) {
        #[cfg(test)]
        if let Some(hook) = &self.before_cleanup {
            hook(_path);
        }
    }

    #[cfg(test)]
    pub(crate) fn force_unconfirmed_reap(&self) -> bool {
        self.force_unconfirmed_reap
    }

    #[cfg(not(test))]
    pub(crate) const fn force_unconfirmed_reap(&self) -> bool {
        false
    }

    fn maybe_fail_traversal(&self) -> Result<(), BashError> {
        #[cfg(test)]
        if self.scan_failure == Some(ScanFailure::Traversal) {
            return Err(BashError::new(BashErrorCode::HardlinkPreflight));
        }
        Ok(())
    }

    fn maybe_fail_metadata(&self, _seen: usize) -> Result<(), BashError> {
        #[cfg(test)]
        if self.scan_failure == Some(ScanFailure::Metadata) && _seen == 0 {
            return Err(BashError::new(BashErrorCode::HardlinkPreflight));
        }
        Ok(())
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScanFailure {
    Traversal,
    Metadata,
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    use std::path::PathBuf;

    use crate::error::BashErrorCode;

    use super::utf8_param;

    #[test]
    fn sandbox_profile_parameter_rejects_non_utf8_without_lossy_aliasing() {
        let path = PathBuf::from(OsString::from_vec(vec![b'/', b't', b'm', b'p', b'/', 0xff]));
        let error = utf8_param(&path).unwrap_err();
        assert_eq!(error.code(), BashErrorCode::SandboxUnavailable);
        assert!(!format!("{error:?}").contains("tmp"));
    }
}
