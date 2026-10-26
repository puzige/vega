//! Fixed, process-local Git executable selection.
//!
//! Production Git commands must never resolve an executable through `PATH`, a
//! repository, or user configuration. This module accepts only the platform
//! paths frozen by the R0a contract, validates the Homebrew layout, probes a
//! bounded `git --version`, and retains the selected file identity for the
//! lifetime of the owning service.

use super::*;
use std::ffi::OsStr;
use std::path::Component;
use std::sync::OnceLock;

const MIN_MAJOR: u32 = 2;
const MIN_MINOR: u32 = 40;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct ExecutableIdentity {
    dev: u64,
    ino: u64,
    size: u64,
    mtime: i64,
    mtime_ns: i64,
    ctime: i64,
    ctime_ns: i64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ExecutableKind {
    Homebrew { cellar: &'static str },
    System,
}

/// A canonical executable selected from the fixed production allowlist.
#[derive(Clone)]
pub(crate) struct GitExecutable {
    path: PathBuf,
    identity: ExecutableIdentity,
    kind: ExecutableKind,
}

impl std::fmt::Debug for GitExecutable {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GitExecutable")
            .field("kind", &self.kind)
            .field("identity", &"[redacted]")
            .finish()
    }
}

impl std::fmt::Debug for ExecutableKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Homebrew { .. } => "homebrew",
            Self::System => "system",
        })
    }
}

impl GitExecutable {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Verifies the fixed executable before every child process is spawned.
    pub(crate) fn verify(&self) -> Result<(), GitWorkspaceError> {
        let canonical = fs::canonicalize(&self.path)
            .map_err(|_| error(GitWorkspaceErrorCode::GitExecutableChanged))?;
        if canonical != self.path {
            return Err(error(GitWorkspaceErrorCode::GitExecutableChanged));
        }
        let metadata = fs::symlink_metadata(&self.path)
            .map_err(|_| error(GitWorkspaceErrorCode::GitExecutableChanged))?;
        if !is_safe_executable(&metadata) {
            return Err(error(GitWorkspaceErrorCode::GitExecutableChanged));
        }
        if executable_identity(&metadata) != self.identity {
            return Err(error(GitWorkspaceErrorCode::GitExecutableChanged));
        }
        Ok(())
    }
}

/// Resolves one accepted executable. Resolution failures are deliberately not
/// cached so installing Git while the app is running makes retry possible.
pub(crate) fn process_git_executable(
    cancel: &CancellationToken,
) -> Result<Arc<GitExecutable>, GitWorkspaceError> {
    static CACHE: OnceLock<Mutex<Option<Arc<GitExecutable>>>> = OnceLock::new();
    cached_git_executable(cancel, &CACHE, resolve_git_executable)
}

fn cached_git_executable(
    cancel: &CancellationToken,
    cache: &OnceLock<Mutex<Option<Arc<GitExecutable>>>>,
    resolve: impl FnOnce(&CancellationToken) -> Result<Arc<GitExecutable>, GitWorkspaceError>,
) -> Result<Arc<GitExecutable>, GitWorkspaceError> {
    if cancel.is_cancelled() {
        return Err(error(GitWorkspaceErrorCode::Cancelled));
    }
    let cache = cache.get_or_init(|| Mutex::new(None));
    if let Some(executable) = cache
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .clone()
    {
        return Ok(executable);
    }
    let selected = resolve(cancel)?;
    let mut guard = cache.lock().unwrap_or_else(|poison| poison.into_inner());
    Ok(guard.get_or_insert_with(|| selected.clone()).clone())
}

fn resolve_git_executable(
    cancel: &CancellationToken,
) -> Result<Arc<GitExecutable>, GitWorkspaceError> {
    resolve_git_executable_from(cancel, fixed_candidates())
}

fn resolve_git_executable_from(
    cancel: &CancellationToken,
    candidates: impl IntoIterator<Item = (PathBuf, ExecutableKind)>,
) -> Result<Arc<GitExecutable>, GitWorkspaceError> {
    let mut saw_candidate = false;
    let mut saw_unsupported = false;
    for (candidate, kind) in candidates {
        let Some(path) = canonical_candidate(&candidate, kind) else {
            continue;
        };
        saw_candidate = true;
        let metadata_before = fs::symlink_metadata(&path)
            .map_err(|_| error(GitWorkspaceErrorCode::GitExecutableChanged))?;
        if !is_safe_executable(&metadata_before)
            || fs::canonicalize(&path).ok().as_deref() != Some(path.as_path())
        {
            return Err(error(GitWorkspaceErrorCode::GitExecutableChanged));
        }
        let identity_before = executable_identity(&metadata_before);
        let selected = GitExecutable {
            path,
            identity: identity_before,
            kind,
        };
        selected.verify()?;
        match probe_version(selected.path(), cancel)? {
            VersionProbe::Supported(version) if version >= (MIN_MAJOR, MIN_MINOR, 0) => {
                selected.verify()?;
                return Ok(Arc::new(selected));
            }
            VersionProbe::Supported(_) | VersionProbe::Unsupported => {
                selected.verify()?;
                saw_unsupported = true;
            }
        }
    }
    Err(error(if saw_unsupported || saw_candidate {
        GitWorkspaceErrorCode::GitUnsupported
    } else {
        GitWorkspaceErrorCode::GitUnavailable
    }))
}

fn fixed_candidates() -> Vec<(PathBuf, ExecutableKind)> {
    if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        vec![
            (
                PathBuf::from("/opt/homebrew/opt/git/bin/git"),
                ExecutableKind::Homebrew {
                    cellar: "/opt/homebrew/Cellar",
                },
            ),
            (PathBuf::from("/usr/bin/git"), ExecutableKind::System),
        ]
    } else if cfg!(target_os = "macos") && cfg!(target_arch = "x86_64") {
        vec![
            (
                PathBuf::from("/usr/local/opt/git/bin/git"),
                ExecutableKind::Homebrew {
                    cellar: "/usr/local/Cellar",
                },
            ),
            (PathBuf::from("/usr/bin/git"), ExecutableKind::System),
        ]
    } else {
        vec![(PathBuf::from("/usr/bin/git"), ExecutableKind::System)]
    }
}

fn canonical_candidate(candidate: &Path, kind: ExecutableKind) -> Option<PathBuf> {
    let canonical = fs::canonicalize(candidate).ok()?;
    match kind {
        ExecutableKind::Homebrew { cellar } => {
            valid_homebrew_path(&canonical, cellar).then_some(canonical)
        }
        // The system slot is intentionally a single fixed file. A symlink or
        // an alternate system/toolchain path is not admitted as a substitute.
        ExecutableKind::System => (canonical == candidate).then_some(canonical),
    }
    .filter(|path| fs::symlink_metadata(path).is_ok_and(|metadata| is_safe_executable(&metadata)))
}

fn valid_homebrew_path(path: &Path, cellar: &str) -> bool {
    let Ok(relative) = path.strip_prefix(cellar) else {
        return false;
    };
    valid_homebrew_components(relative)
}

fn valid_homebrew_components(path: &Path) -> bool {
    let components: Vec<_> = path.components().collect();
    if components.len() != 4 {
        return false;
    }
    let [git, version, bin, executable] = components.as_slice() else {
        return false;
    };
    let Component::Normal(git) = git else {
        return false;
    };
    let Component::Normal(version) = version else {
        return false;
    };
    let Component::Normal(bin) = bin else {
        return false;
    };
    let Component::Normal(executable) = executable else {
        return false;
    };
    *git == OsStr::new("git")
        && *bin == OsStr::new("bin")
        && *executable == OsStr::new("git")
        && !version.is_empty()
        && version
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'.' | b'_' | b'+' | b'-'))
}

fn is_safe_executable(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_file() && metadata.mode() & 0o111 != 0 && metadata.mode() & 0o022 == 0
}

fn executable_identity(metadata: &std::fs::Metadata) -> ExecutableIdentity {
    ExecutableIdentity {
        dev: metadata.dev(),
        ino: metadata.ino(),
        size: metadata.len(),
        mtime: metadata.mtime(),
        mtime_ns: metadata.mtime_nsec(),
        ctime: metadata.ctime(),
        ctime_ns: metadata.ctime_nsec(),
    }
}

enum VersionProbe {
    Supported((u32, u32, u32)),
    Unsupported,
}

fn probe_version(
    path: &Path,
    cancel: &CancellationToken,
) -> Result<VersionProbe, GitWorkspaceError> {
    if cancel.is_cancelled() {
        return Err(error(GitWorkspaceErrorCode::Cancelled));
    }
    let mut command = Command::new(path);
    command
        .arg("--version")
        .current_dir("/")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    scrub_git_environment(&mut command);
    let mut child = command
        .spawn()
        .map_err(|_| error(GitWorkspaceErrorCode::SpawnFailed))?;
    let output = collect_child(
        &mut child,
        None,
        STDOUT_LIMIT.min(4096),
        STDERR_LIMIT,
        READ_TIMEOUT,
        cancel,
        OverflowPolicy::IMMEDIATE,
    )?;
    if output.overflow {
        return Err(error(GitWorkspaceErrorCode::OutputTooLarge));
    }
    Ok(
        parse_git_version(&output.stdout).map_or(VersionProbe::Unsupported, |version| {
            VersionProbe::Supported(version)
        }),
    )
}

/// Parses the stable macOS Git version line without accepting an arbitrary
/// executable banner or an unbounded numeric grammar.
pub(crate) fn parse_git_version(output: &[u8]) -> Option<(u32, u32, u32)> {
    let line = output.strip_suffix(b"\n")?;
    if line.is_empty() || line.contains(&0) || line.contains(&b'\n') {
        return None;
    }
    let text = std::str::from_utf8(line).ok()?;
    let banner = text.strip_prefix("git version ")?;
    let (version, suffix) = match banner.split_once(' ') {
        Some((version, suffix)) => (version, suffix),
        None => (banner, ""),
    };
    if !suffix.is_empty()
        && !(suffix.starts_with('(')
            && suffix.ends_with(')')
            && suffix[1..suffix.len() - 1].bytes().all(|byte| {
                byte.is_ascii_alphanumeric()
                    || matches!(byte, b'.' | b'_' | b'+' | b'-' | b'/' | b' ')
            }))
    {
        return None;
    }
    let mut components = version.split('.');
    let major = parse_version_component(components.next()?)?;
    let minor = parse_version_component(components.next()?)?;
    let patch = match components.next() {
        Some(component) => parse_version_component(component)?,
        None => 0,
    };
    components.next().is_none().then_some((major, minor, patch))
}

fn parse_version_component(component: &str) -> Option<u32> {
    (!component.is_empty() && component.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| component.parse().ok())?
}
