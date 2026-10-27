//! macOS Seatbelt launch boundary and pre-spawn hardlink scan.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Read;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command as StdCommand, Stdio};

use std::time::Duration;

use tokio::process::{Child, Command};
use tokio_util::sync::CancellationToken;

use crate::bash::BashLaunchPolicy;

use crate::error::{BashError, BashErrorCode};
use crate::fence::discover_git_dir;

const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";
const SHELL: &str = "/bin/zsh";
const TEMP_BASE: &str = "/private/tmp";
const SELF_TEST_LIMIT: Duration = Duration::from_secs(2);
const BASE_PROFILE: &str = r#"(version 1)
(deny default)
(allow process-exec)
(allow process-fork)
(allow signal (target same-sandbox))
(allow file-read*)
(allow process-info-pidinfo (target self))
(allow process-info-setcontrol (target self))
(allow file-write-data (require-all (path "/dev/null") (vnode-type CHARACTER-DEVICE)))
(allow sysctl-read
  (sysctl-name "hw.activecpu")
  (sysctl-name "hw.byteorder")
  (sysctl-name "hw.cachelinesize_compat")
  (sysctl-name "hw.cpufamily")
  (sysctl-name "hw.cputype")
  (sysctl-name "hw.logicalcpu")
  (sysctl-name "hw.logicalcpu_max")
  (sysctl-name "hw.machine")
  (sysctl-name "hw.model")
  (sysctl-name "hw.memsize")
  (sysctl-name "hw.ncpu")
  (sysctl-name "hw.pagesize")
  (sysctl-name "hw.pagesize_compat")
  (sysctl-name "hw.physicalcpu")
  (sysctl-name "hw.physicalcpu_max")
  (sysctl-name "kern.argmax")
  (sysctl-name "kern.hostname")
  (sysctl-name "kern.maxfilesperproc")
  (sysctl-name "kern.osproductversion")
  (sysctl-name "kern.osrelease")
  (sysctl-name "kern.ostype")
  (sysctl-name "kern.osvariant_status")
  (sysctl-name "kern.osversion")
  (sysctl-name "kern.usrstack64")
  (sysctl-name "kern.version")
  (sysctl-name "sysctl.proc_cputype"))
(allow sysctl-write (sysctl-name "kern.grade_cputype"))
(allow mach-lookup (global-name "com.apple.system.opendirectoryd.libinfo"))"#;

#[derive(Clone, Copy)]
pub(crate) enum SandboxNetwork {
    Denied,
    Direct,
    Broker(u16),
}

fn restricted_profile(
    policy: BashLaunchPolicy,
    protected_root: Option<&Path>,
    network: SandboxNetwork,
    directories: &[crate::BashScopeDirectory],
) -> Result<String, BashError> {
    let mut profile = BASE_PROFILE.to_owned();
    match policy {
        BashLaunchPolicy::ReadOnly => {},
        BashLaunchPolicy::WorkspaceWrite => profile.push_str(
            "\n(allow file-write* (subpath (param \"PROJECT_ROOT\")))\n(allow file-write* (subpath (param \"TEMP_ROOT\")))",
        ),
        BashLaunchPolicy::FullAccess => return Err(BashError::new(BashErrorCode::SandboxUnavailable)),
    }
    for directory in directories {
        if policy != BashLaunchPolicy::WorkspaceWrite {
            return Err(BashError::new(BashErrorCode::ScopeMismatch));
        }
        directory.validate()?;
        let path = utf8_param(directory.path())?;
        profile.push_str(&format!("\n(allow file-write* (subpath \"{path}\"))"));
    }
    match network {
        SandboxNetwork::Denied => {},
        SandboxNetwork::Direct => profile.push_str(
            "\n(allow network-outbound (remote tcp \"*:*\") (remote udp \"*:*\"))\n(allow network-inbound (local tcp \"*:*\") (local udp \"*:*\"))\n(allow network-bind (local ip \"*:*\"))\n(allow mach-lookup (global-name \"com.apple.SecurityServer\") (global-name \"com.apple.networkd\") (global-name \"com.apple.ocspd\") (global-name \"com.apple.trustd.agent\") (global-name \"com.apple.SystemConfiguration.DNSConfiguration\") (global-name \"com.apple.SystemConfiguration.configd\"))\n(allow sysctl-read (sysctl-name-regex #\"^net.routetable\"))",
        ),
        SandboxNetwork::Broker(port) => profile.push_str(&format!(
            "\n(allow network-outbound (remote tcp \"127.0.0.1:{port}\"))"
        )),
    }
    profile.push_str(
        "\n(deny file-write* (subpath (param \"GIT_ENTRY\")))\n(deny file-write* (subpath (param \"GIT_DIR\")))\n(deny mach-lookup (global-name \"com.apple.taskgated\"))",
    );
    if let Some(root) = protected_root {
        let root = utf8_param(root)?;
        if root.contains(['"', '\\', '\n', '\r', '\0']) {
            return Err(BashError::new(BashErrorCode::SandboxUnavailable));
        }
        profile.push_str(&format!(
            "\n(deny file-read* file-write* (subpath \"{root}\"))"
        ));
    }
    Ok(profile)
}

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
    profile: String,
    temporary_path: PathBuf,
    environment: BTreeMap<String, String>,
    directories: Vec<crate::BashScopeDirectory>,
}

impl SandboxConfig {
    pub(crate) fn new(
        project_root: &Path,
        policy: BashLaunchPolicy,
        temp_root: &TempRoot,
        protected_root: Option<&Path>,
    ) -> Result<Self, BashError> {
        Self::scoped(
            project_root,
            policy,
            temp_root,
            protected_root,
            SandboxNetwork::Denied,
            Vec::new(),
            BTreeMap::new(),
        )
    }

    pub(crate) fn scoped(
        project_root: &Path,
        policy: BashLaunchPolicy,
        temp_root: &TempRoot,
        protected_root: Option<&Path>,
        network: SandboxNetwork,
        directories: Vec<crate::BashScopeDirectory>,
        additional_environment: BTreeMap<String, String>,
    ) -> Result<Self, BashError> {
        let profile = restricted_profile(policy, protected_root, network, &directories)?;
        temp_root.validate_identity()?;
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
        let mut environment = crate::credentials::controlled_environment(temp_root.path());
        environment.extend(additional_environment);
        Ok(Self {
            project_root: project_root.to_path_buf(),
            project_param,
            git_entry,
            git_entry_param,
            git_dir,
            git_dir_param,
            profile,
            temporary_path: temp_root.path().to_path_buf(),
            environment,
            directories,
        })
    }

    pub(crate) fn preflight(&self, temp_root: &TempRoot) -> Result<(), BashError> {
        if temp_root.path() != self.temporary_path {
            return Err(BashError::new(BashErrorCode::ScopeMismatch));
        }
        for directory in &self.directories {
            directory.validate()?;
            scan_hardlinks(directory.path(), None)?;
            directory.validate()?;
        }
        scan_hardlinks(&self.project_root, Some((&self.git_entry, &self.git_dir)))?;
        temp_root.validate_identity()?;
        scan_hardlinks(temp_root.path(), None)?;
        temp_root.validate_identity()
    }

    pub(crate) async fn self_test(
        &self,
        temp_root: &TempRoot,
        cancel: &CancellationToken,
        command_timeout: Duration,
    ) -> SandboxSelfTest {
        if cancel.is_cancelled() {
            return SandboxSelfTest {
                result: Err(BashError::new(BashErrorCode::Cancelled)),
                cleanup_safe: true,
            };
        }
        let command = match self.self_test_command(temp_root) {
            Ok(command) => command,
            Err(error) => {
                return SandboxSelfTest {
                    result: Err(error),
                    cleanup_safe: true,
                };
            }
        };
        if cancel.is_cancelled() {
            return SandboxSelfTest {
                result: Err(BashError::new(BashErrorCode::Cancelled)),
                cleanup_safe: true,
            };
        }
        let mut command = Command::from(command);
        command.kill_on_drop(true);
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(_) => {
                return SandboxSelfTest {
                    result: Err(BashError::new(BashErrorCode::SandboxUnavailable)),
                    cleanup_safe: true,
                };
            }
        };
        tokio::select! {
            status = child.wait() => match status {
                Ok(status) => SandboxSelfTest {
                    result: if status.success() { Ok(()) } else { Err(BashError::new(BashErrorCode::SandboxUnavailable)) },
                    cleanup_safe: true,
                },
                Err(_) => {
                    let _ = stop_self_test(&mut child).await;
                    SandboxSelfTest {
                        result: Err(BashError::new(BashErrorCode::ProcessControlFailed)),
                        cleanup_safe: false,
                    }
                },
            },
            _ = cancel.cancelled() => SandboxSelfTest {
                result: Err(BashError::new(BashErrorCode::Cancelled)),
                cleanup_safe: stop_self_test(&mut child).await,
            },
            _ = tokio::time::sleep(command_timeout.min(SELF_TEST_LIMIT)) => SandboxSelfTest {
                result: Err(BashError::new(BashErrorCode::SandboxUnavailable)),
                cleanup_safe: stop_self_test(&mut child).await,
            },
        }
    }

    fn self_test_command(&self, temp_root: &TempRoot) -> Result<StdCommand, BashError> {
        self.preflight(temp_root)?;
        let mut command = self.command(temp_root);
        command.arg("--").arg("/usr/bin/true");
        command.stdin(Stdio::null());
        command.stdout(Stdio::null());
        command.stderr(Stdio::null());
        command.current_dir(&self.project_root);
        command.process_group(0);
        Ok(command)
    }

    pub(crate) fn shell_command(
        &self,
        command_text: &str,
        temp_root: &TempRoot,
    ) -> Result<StdCommand, BashError> {
        self.preflight(temp_root)?;
        let mut command = self.command(temp_root);
        command.arg("--").arg(SHELL);
        shell_command(command, &self.project_root, command_text, temp_root, "-fc")
    }

    fn command(&self, temp_root: &TempRoot) -> StdCommand {
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
            .arg(&self.profile)
            .env_clear()
            .envs(&self.environment);
        command
    }
}

async fn stop_self_test(child: &mut Child) -> bool {
    let killed = child.start_kill().is_ok();
    let reaped = matches!(
        tokio::time::timeout(SELF_TEST_LIMIT, child.wait()).await,
        Ok(Ok(_))
    );
    killed && reaped
}

pub(crate) fn full_access_shell_command(
    project_root: &Path,
    command_text: &str,
    temp_root: &TempRoot,
) -> Result<StdCommand, BashError> {
    shell_command(
        StdCommand::new(SHELL),
        project_root,
        command_text,
        temp_root,
        "-lc",
    )
}

fn shell_command(
    mut command: StdCommand,
    project_root: &Path,
    command_text: &str,
    temp_root: &TempRoot,
    shell_flag: &str,
) -> Result<StdCommand, BashError> {
    temp_root.validate_identity()?;
    command
        .arg(shell_flag)
        .arg(format!("exec 2>&1\n{command_text}"))
        .current_dir(project_root)
        .env("TMPDIR", temp_root.param())
        .env("TMP", temp_root.param())
        .env("TEMP", temp_root.param())
        .env("TEMPDIR", temp_root.param())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);

    Ok(command)
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

    pub(crate) fn validate_identity(&self) -> Result<(), BashError> {
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

fn scan_hardlinks(root: &Path, project_git: Option<(&Path, &Path)>) -> Result<(), BashError> {
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
