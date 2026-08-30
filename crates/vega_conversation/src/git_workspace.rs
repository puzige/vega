//! Private bounded Git workspace service.
//!
//! Raw repository roots, paths, stderr, and patch bytes never leave this
//! module. Public callers receive only safe projections from `types`.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::Read;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::MetadataExt;
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use tokio_util::sync::CancellationToken;

use crate::types::{
    DiffHunk, DiffLanguage, DiffLayer, DiffRow, DiffRowKind, DiffSection, DiffTextProjection,
    GitWorkspaceError, GitWorkspaceErrorCode, WorkspaceChangeKind, WorkspaceFile, WorkspaceFileId,
    WorkspaceHead, WorkspaceLineCount, WorkspaceSnapshot, WorkspaceStats,
};

const GIT: &str = "/usr/bin/git";
const KILL: &str = "/bin/kill";
const IO_CHUNK: usize = 16 * 1024;
const READ_TIMEOUT: Duration = Duration::from_secs(10);
const TERM_GRACE: Duration = Duration::from_millis(300);
const DRAIN_GRACE: Duration = Duration::from_millis(500);
const STDOUT_LIMIT: usize = 8 * 1024 * 1024;
const STDERR_LIMIT: usize = 64 * 1024;
const SNAPSHOT_LIMIT: usize = 8 * 1024 * 1024;
const PATH_LIMIT: usize = 10_000;
const PATCH_LIMIT: usize = 4 * 1024 * 1024;
const PATCH_ROW_LIMIT: usize = 20_000;
const PATCH_LINE_LIMIT: usize = 64 * 1024;
static SERVICE_NONCE: AtomicU64 = AtomicU64::new(1);

const PREFIX: &[&str] = &[
    "--no-pager",
    "-c",
    "core.fsmonitor=false",
    "-c",
    "color.ui=false",
    "-c",
    "maintenance.auto=false",
    "-c",
    "maintenance.autoDetach=false",
    "-c",
    "gc.auto=0",
];

#[derive(Clone, Copy)]
struct RootIdentity {
    dev: u64,
    ino: u64,
}

#[derive(Clone)]
struct PrivateFile {
    id: WorkspaceFileId,
    path: OsString,
    previous_path: Option<OsString>,
    staged: WorkspaceChangeKind,
    unstaged: WorkspaceChangeKind,
    binary: bool,
    language: DiffLanguage,
    snapshot_identity: Arc<SnapshotIdentity>,
}

#[derive(PartialEq, Eq)]
struct SnapshotIdentity {
    status: Vec<u8>,
    staged_raw: Vec<u8>,
    unstaged_raw: Vec<u8>,
}

#[derive(Default)]
struct ServiceState {
    generation: u64,
    files: HashMap<WorkspaceFileId, PrivateFile>,
}

/// Headless, ephemeral Git snapshot and lazy-diff service.
pub struct GitWorkspaceService {
    root: PathBuf,
    identity: RootIdentity,
    instance_nonce: u64,
    next_generation: AtomicU64,
    state: Arc<Mutex<ServiceState>>,
    #[cfg(test)]
    executable: Option<PathBuf>,
}

impl std::fmt::Debug for GitWorkspaceService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GitWorkspaceService")
            .field("root", &"[redacted]")
            .field("generation", &self.next_generation.load(Ordering::Relaxed))
            .finish()
    }
}

impl GitWorkspaceService {
    /// Creates a service fenced to one canonical repository root.
    pub fn new(root: impl AsRef<Path>) -> Result<Self, GitWorkspaceError> {
        Self::new_inner(root.as_ref(), None)
    }

    fn new_inner(
        root: &Path,
        #[allow(unused_variables)] executable: Option<PathBuf>,
    ) -> Result<Self, GitWorkspaceError> {
        let root = fs::canonicalize(root).map_err(|_| error(GitWorkspaceErrorCode::InvalidRoot))?;
        let metadata =
            fs::metadata(&root).map_err(|_| error(GitWorkspaceErrorCode::InvalidRoot))?;
        if !metadata.is_dir() {
            return Err(error(GitWorkspaceErrorCode::InvalidRoot));
        }
        let instance_nonce = SERVICE_NONCE
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map_err(|_| error(GitWorkspaceErrorCode::OutputTooLarge))?;
        Ok(Self {
            root,
            identity: RootIdentity {
                dev: metadata.dev(),
                ino: metadata.ino(),
            },
            instance_nonce,
            next_generation: AtomicU64::new(0),
            state: Arc::new(Mutex::new(ServiceState::default())),
            #[cfg(test)]
            executable,
        })
    }

    /// Refreshes the complete metadata snapshot. A newer refresh invalidates
    /// an older in-flight result (latest generation wins).
    pub async fn refresh(
        &self,
        cancel: CancellationToken,
    ) -> Result<WorkspaceSnapshot, GitWorkspaceError> {
        {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            state.files.clear();
            state.generation = 0;
        }
        let generation = self
            .next_generation
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |value| {
                value.checked_add(1)
            })
            .map_err(|_| error(GitWorkspaceErrorCode::OutputTooLarge))?
            + 1;
        let root = self.root.clone();
        let identity = self.identity;
        let instance_nonce = self.instance_nonce;
        #[cfg(test)]
        let executable = self.executable.clone();
        let result = tokio::task::spawn_blocking(move || {
            let runner = Runner::new(
                root,
                identity,
                #[cfg(test)]
                executable,
            );
            build_snapshot(&runner, generation, instance_nonce, &cancel)
        })
        .await
        .map_err(|_| error(GitWorkspaceErrorCode::GitFailed))??;

        if self.next_generation.load(Ordering::SeqCst) != generation {
            return Err(error(GitWorkspaceErrorCode::StaleGeneration));
        }
        let (snapshot, files) = result;
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if self.next_generation.load(Ordering::SeqCst) != generation {
            return Err(error(GitWorkspaceErrorCode::StaleGeneration));
        }
        state.generation = generation;
        state.files = files.into_iter().map(|file| (file.id, file)).collect();
        Ok(snapshot)
    }

    /// Lazily loads one structured patch for the current snapshot.
    pub async fn diff(
        &self,
        file_id: WorkspaceFileId,
        cancel: CancellationToken,
    ) -> Result<DiffTextProjection, GitWorkspaceError> {
        let private = {
            let state = self
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            if state.generation != file_id.generation {
                return Err(error(GitWorkspaceErrorCode::StaleGeneration));
            }
            state
                .files
                .get(&file_id)
                .cloned()
                .ok_or_else(|| error(GitWorkspaceErrorCode::UnknownFile))?
        };
        let root = self.root.clone();
        let identity = self.identity;
        #[cfg(test)]
        let executable = self.executable.clone();
        let result = tokio::task::spawn_blocking(move || {
            let runner = Runner::new(
                root,
                identity,
                #[cfg(test)]
                executable,
            );
            build_projection(&runner, private, &cancel)
        })
        .await
        .map_err(|_| error(GitWorkspaceErrorCode::GitFailed))??;
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if state.generation != file_id.generation || !state.files.contains_key(&file_id) {
            return Err(error(GitWorkspaceErrorCode::StaleGeneration));
        }
        Ok(result)
    }

    #[cfg(test)]
    fn new_for_test(root: &Path, executable: PathBuf) -> Result<Self, GitWorkspaceError> {
        Self::new_inner(root, Some(executable))
    }
}

struct Runner {
    root: PathBuf,
    identity: RootIdentity,
    #[cfg(test)]
    executable: Option<PathBuf>,
}

struct Output {
    stdout: Vec<u8>,
}

impl Runner {
    fn new(
        root: PathBuf,
        identity: RootIdentity,
        #[cfg(test)] executable: Option<PathBuf>,
    ) -> Self {
        Self {
            root,
            identity,
            #[cfg(test)]
            executable,
        }
    }

    fn run(
        &self,
        verb: &'static str,
        args: &[OsString],
        stdout_limit: usize,
        cancel: &CancellationToken,
    ) -> Result<Output, GitWorkspaceError> {
        if !matches!(
            verb,
            "status"
                | "diff"
                | "rev-parse"
                | "for-each-ref"
                | "check-attr"
                | "ls-files"
                | "ls-tree"
                | "hash-object"
        ) {
            return Err(error(GitWorkspaceErrorCode::GitFailed));
        }
        self.verify_root()?;
        if cancel.is_cancelled() {
            return Err(error(GitWorkspaceErrorCode::Cancelled));
        }
        #[cfg(test)]
        let executable = self.executable.as_deref().unwrap_or_else(|| Path::new(GIT));
        #[cfg(not(test))]
        let executable = Path::new(GIT);
        let mut command = Command::new(executable);
        command.current_dir(&self.root);
        command.args(PREFIX).arg(verb).args(args);
        scrub_git_environment(&mut command);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        let mut child = command
            .spawn()
            .map_err(|_| error(GitWorkspaceErrorCode::SpawnFailed))?;
        collect_child(&mut child, stdout_limit, cancel)
    }

    fn verify_root(&self) -> Result<(), GitWorkspaceError> {
        let canonical = fs::canonicalize(&self.root)
            .map_err(|_| error(GitWorkspaceErrorCode::ChangedDuringRead))?;
        let metadata = fs::metadata(&canonical)
            .map_err(|_| error(GitWorkspaceErrorCode::ChangedDuringRead))?;
        if canonical != self.root
            || metadata.dev() != self.identity.dev
            || metadata.ino() != self.identity.ino
        {
            return Err(error(GitWorkspaceErrorCode::ChangedDuringRead));
        }
        Ok(())
    }
}

fn scrub_git_environment(command: &mut Command) {
    let explicit_git_keys: Vec<OsString> = command
        .get_envs()
        .filter(|(key, _)| key.as_bytes().starts_with(b"GIT_"))
        .map(|(key, _)| key.to_owned())
        .collect();
    for key in explicit_git_keys {
        command.env_remove(key);
    }
    for (key, _) in std::env::vars_os() {
        if key.as_os_str().as_bytes().starts_with(b"GIT_") {
            command.env_remove(key);
        }
    }
    command
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_PAGER", "cat")
        .env("GIT_LITERAL_PATHSPECS", "1")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("LC_ALL", "C");
}

struct ReaderResult {
    stream: Stream,
    bytes: Vec<u8>,
    overflow: bool,
    failed: bool,
}

#[derive(Clone, Copy)]
enum Stream {
    Stdout,
    Stderr,
}

fn collect_child(
    child: &mut Child,
    stdout_limit: usize,
    cancel: &CancellationToken,
) -> Result<Output, GitWorkspaceError> {
    let pgid = child.id();
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| error(GitWorkspaceErrorCode::ProcessControlFailed))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| error(GitWorkspaceErrorCode::ProcessControlFailed))?;
    let overflowed = Arc::new(AtomicBool::new(false));
    let (sender, receiver) = mpsc::channel();
    spawn_reader(
        stdout,
        Stream::Stdout,
        stdout_limit,
        overflowed.clone(),
        sender.clone(),
    );
    spawn_reader(
        stderr,
        Stream::Stderr,
        STDERR_LIMIT,
        overflowed.clone(),
        sender,
    );

    let started = Instant::now();
    let mut status = None;
    let mut stop_code = None;
    while status.is_none() {
        if cancel.is_cancelled() {
            stop_code = Some(GitWorkspaceErrorCode::Cancelled);
            break;
        }
        if overflowed.load(Ordering::SeqCst) {
            stop_code = Some(GitWorkspaceErrorCode::OutputTooLarge);
            break;
        }
        if started.elapsed() >= READ_TIMEOUT {
            stop_code = Some(GitWorkspaceErrorCode::TimedOut);
            break;
        }
        status = child
            .try_wait()
            .map_err(|_| error(GitWorkspaceErrorCode::ProcessControlFailed))?;
        if status.is_none() {
            thread::sleep(Duration::from_millis(5));
        }
    }
    if stop_code.is_some() {
        terminate_group(child, pgid)?;
    }

    let drain_started = Instant::now();
    let mut outputs = Vec::with_capacity(2);
    while outputs.len() < 2 && drain_started.elapsed() < DRAIN_GRACE {
        match receiver.recv_timeout(Duration::from_millis(10)) {
            Ok(output) => outputs.push(output),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    if outputs.len() < 2 {
        terminate_group(child, pgid)?;
        while outputs.len() < 2 {
            match receiver.recv_timeout(DRAIN_GRACE) {
                Ok(output) => outputs.push(output),
                Err(_) => return Err(error(GitWorkspaceErrorCode::ProcessControlFailed)),
            }
        }
    }
    if status.is_none() {
        status = Some(
            child
                .wait()
                .map_err(|_| error(GitWorkspaceErrorCode::ProcessControlFailed))?,
        );
    }
    if let Some(code) = stop_code {
        return Err(error(code));
    }
    if outputs.iter().any(|output| output.overflow) {
        return Err(error(GitWorkspaceErrorCode::OutputTooLarge));
    }
    if outputs.iter().any(|output| output.failed) {
        return Err(error(GitWorkspaceErrorCode::GitFailed));
    }
    let status = status.ok_or_else(|| error(GitWorkspaceErrorCode::ProcessControlFailed))?;
    if !status.success() {
        return Err(classify_git_failure(status, &outputs));
    }
    let stdout = outputs
        .into_iter()
        .find(|output| matches!(output.stream, Stream::Stdout))
        .map(|output| output.bytes)
        .ok_or_else(|| error(GitWorkspaceErrorCode::GitFailed))?;
    Ok(Output { stdout })
}

fn spawn_reader<R: Read + Send + 'static>(
    mut reader: R,
    stream: Stream,
    limit: usize,
    overflowed: Arc<AtomicBool>,
    sender: mpsc::Sender<ReaderResult>,
) {
    thread::spawn(move || {
        let mut retained = Vec::with_capacity(limit.min(IO_CHUNK));
        let mut chunk = [0_u8; IO_CHUNK];
        let mut overflow = false;
        let mut failed = false;
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(read) => {
                    if retained.len().saturating_add(read) <= limit {
                        retained.extend_from_slice(&chunk[..read]);
                    } else {
                        overflow = true;
                        overflowed.store(true, Ordering::SeqCst);
                    }
                }
                Err(_) => {
                    failed = true;
                    break;
                }
            }
        }
        let _ = sender.send(ReaderResult {
            stream,
            bytes: retained,
            overflow,
            failed,
        });
    });
}

fn terminate_group(child: &mut Child, pgid: u32) -> Result<(), GitWorkspaceError> {
    let _ = signal_group(pgid, "-TERM");
    thread::sleep(TERM_GRACE);
    let _ = signal_group(pgid, "-KILL");
    child
        .wait()
        .map_err(|_| error(GitWorkspaceErrorCode::ProcessControlFailed))?;
    Ok(())
}

fn signal_group(pgid: u32, signal: &str) -> std::io::Result<ExitStatus> {
    Command::new(KILL)
        .args([signal, "--", &format!("-{pgid}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
}

fn classify_git_failure(status: ExitStatus, outputs: &[ReaderResult]) -> GitWorkspaceError {
    let not_repository = status.code() == Some(128)
        && outputs.iter().any(|output| {
            matches!(output.stream, Stream::Stderr)
                && output
                    .bytes
                    .windows(b"not a git repository".len())
                    .any(|window| window == b"not a git repository")
        });
    error(if not_repository {
        GitWorkspaceErrorCode::NotRepository
    } else {
        GitWorkspaceErrorCode::GitFailed
    })
}

fn error(code: GitWorkspaceErrorCode) -> GitWorkspaceError {
    GitWorkspaceError::new(code)
}

#[derive(Clone)]
struct ParsedFile {
    path: Vec<u8>,
    previous_path: Option<Vec<u8>>,
    staged: WorkspaceChangeKind,
    unstaged: WorkspaceChangeKind,
    additions: WorkspaceLineCount,
    deletions: WorkspaceLineCount,
}

struct ParsedStatus {
    head: WorkspaceHead,
    files: BTreeMap<Vec<u8>, ParsedFile>,
}

fn build_snapshot(
    runner: &Runner,
    generation: u64,
    instance_nonce: u64,
    cancel: &CancellationToken,
) -> Result<(WorkspaceSnapshot, Vec<PrivateFile>), GitWorkspaceError> {
    let top = runner.run(
        "rev-parse",
        &[OsString::from("--show-toplevel")],
        STDOUT_LIMIT,
        cancel,
    )?;
    let top_bytes = trim_one_newline(&top.stdout);
    let top_path = fs::canonicalize(PathBuf::from(OsString::from_vec(top_bytes.to_vec())))
        .map_err(|_| error(GitWorkspaceErrorCode::NotRepository))?;
    if top_path != runner.root {
        return Err(error(GitWorkspaceErrorCode::InvalidRoot));
    }
    let status_output = runner.run("status", &status_args(), STDOUT_LIMIT, cancel)?;
    let mut parsed = parse_status(&status_output.stdout)?;
    let mut consumed = status_output.stdout.len();
    let mut raw_outputs = Vec::with_capacity(2);
    for cached in [true, false] {
        let raw_args = raw_args(cached);
        let raw = runner.run("diff", &raw_args, STDOUT_LIMIT, cancel)?;
        let raw_entries = validate_raw(&raw.stdout)?;
        cross_check_raw(&parsed.files, &raw_entries, cached)?;
        consumed = consumed.saturating_add(raw.stdout.len());
        raw_outputs.push(raw.stdout);

        let mut numstat_args = vec![
            OsString::from("--numstat"),
            OsString::from("-z"),
            OsString::from("--find-renames"),
            OsString::from("--no-ext-diff"),
            OsString::from("--no-textconv"),
        ];
        if cached {
            numstat_args.insert(0, OsString::from("--cached"));
        }
        let numstat = runner.run("diff", &numstat_args, STDOUT_LIMIT, cancel)?;
        let numstat_paths = merge_numstat(&mut parsed.files, &numstat.stdout, cached)?;
        let raw_paths: BTreeSet<&[u8]> = raw_entries
            .iter()
            .map(|entry| entry.path.as_slice())
            .collect();
        let numstat_paths: BTreeSet<&[u8]> = numstat_paths.iter().map(Vec::as_slice).collect();
        if raw_paths != numstat_paths {
            return Err(error(GitWorkspaceErrorCode::MalformedOutput));
        }
        consumed = consumed.saturating_add(numstat.stdout.len());
        if consumed > SNAPSHOT_LIMIT {
            return Err(error(GitWorkspaceErrorCode::OutputTooLarge));
        }
    }

    if parsed.files.len() > PATH_LIMIT {
        return Err(error(GitWorkspaceErrorCode::OutputTooLarge));
    }
    let snapshot_identity = Arc::new(SnapshotIdentity {
        status: status_output.stdout,
        staged_raw: raw_outputs.remove(0),
        unstaged_raw: raw_outputs.remove(0),
    });
    verify_snapshot_identity(runner, &snapshot_identity, cancel)?;
    let mut public_files = Vec::with_capacity(parsed.files.len());
    let mut private_files = Vec::with_capacity(parsed.files.len());
    let mut aggregate_add = Some(0_u64);
    let mut aggregate_delete = Some(0_u64);
    for (slot, parsed_file) in parsed.files.into_values().enumerate() {
        let slot = u32::try_from(slot).map_err(|_| error(GitWorkspaceErrorCode::OutputTooLarge))?;
        let seal = seal(
            runner.identity,
            instance_nonce,
            generation,
            slot,
            &parsed_file.path,
        );
        let id = WorkspaceFileId {
            generation,
            slot,
            seal,
        };
        let language = language_for(&parsed_file.path);
        let binary = matches!(parsed_file.additions, WorkspaceLineCount::Binary)
            || matches!(parsed_file.deletions, WorkspaceLineCount::Binary);
        fold_count(&mut aggregate_add, parsed_file.additions)?;
        fold_count(&mut aggregate_delete, parsed_file.deletions)?;
        public_files.push(WorkspaceFile {
            id,
            label: escape_path(&parsed_file.path),
            previous_label: parsed_file.previous_path.as_deref().map(escape_path),
            staged: parsed_file.staged,
            unstaged: parsed_file.unstaged,
            additions: parsed_file.additions,
            deletions: parsed_file.deletions,
            language,
        });
        private_files.push(PrivateFile {
            id,
            path: OsString::from_vec(parsed_file.path),
            previous_path: parsed_file.previous_path.map(OsString::from_vec),
            staged: parsed_file.staged,
            unstaged: parsed_file.unstaged,
            binary,
            language,
            snapshot_identity: snapshot_identity.clone(),
        });
    }
    let additions = aggregate_add.map_or(WorkspaceLineCount::Unknown, WorkspaceLineCount::Known);
    let deletions = aggregate_delete.map_or(WorkspaceLineCount::Unknown, WorkspaceLineCount::Known);
    let snapshot = WorkspaceSnapshot {
        generation,
        head: parsed.head,
        stats: WorkspaceStats {
            file_count: u32::try_from(public_files.len())
                .map_err(|_| error(GitWorkspaceErrorCode::OutputTooLarge))?,
            additions,
            deletions,
        },
        files: public_files,
    };
    let retained = estimate_snapshot_bytes(&snapshot);
    if consumed.saturating_add(retained) > SNAPSHOT_LIMIT {
        return Err(error(GitWorkspaceErrorCode::OutputTooLarge));
    }
    Ok((snapshot, private_files))
}

fn status_args() -> Vec<OsString> {
    vec![
        OsString::from("--porcelain=v2"),
        OsString::from("-z"),
        OsString::from("--branch"),
        OsString::from("--renames"),
        OsString::from("--untracked-files=all"),
    ]
}

fn raw_args(cached: bool) -> Vec<OsString> {
    let mut args = vec![
        OsString::from("--raw"),
        OsString::from("-z"),
        OsString::from("--abbrev=64"),
        OsString::from("--find-renames"),
        OsString::from("--no-ext-diff"),
        OsString::from("--no-textconv"),
    ];
    if cached {
        args.insert(0, OsString::from("--cached"));
    }
    args
}

fn verify_snapshot_identity(
    runner: &Runner,
    expected: &SnapshotIdentity,
    cancel: &CancellationToken,
) -> Result<(), GitWorkspaceError> {
    let status = runner.run("status", &status_args(), STDOUT_LIMIT, cancel)?;
    parse_status(&status.stdout)?;
    let staged = runner.run("diff", &raw_args(true), STDOUT_LIMIT, cancel)?;
    validate_raw(&staged.stdout)?;
    let unstaged = runner.run("diff", &raw_args(false), STDOUT_LIMIT, cancel)?;
    validate_raw(&unstaged.stdout)?;
    if status.stdout != expected.status
        || staged.stdout != expected.staged_raw
        || unstaged.stdout != expected.unstaged_raw
    {
        return Err(error(GitWorkspaceErrorCode::ChangedDuringRead));
    }
    Ok(())
}

fn parse_status(bytes: &[u8]) -> Result<ParsedStatus, GitWorkspaceError> {
    if !bytes.is_empty() && !bytes.ends_with(&[0]) {
        return Err(error(GitWorkspaceErrorCode::MalformedOutput));
    }
    let mut records = bytes.split(|byte| *byte == 0).peekable();
    let mut oid = None;
    let mut branch = None;
    let mut files = BTreeMap::new();
    while let Some(record) = records.next() {
        if record.is_empty() {
            if records.peek().is_none() {
                break;
            }
            return Err(error(GitWorkspaceErrorCode::MalformedOutput));
        }
        if let Some(value) = record.strip_prefix(b"# branch.oid ") {
            if oid.is_some()
                || (value != b"(initial)"
                    && !(matches!(value.len(), 40 | 64)
                        && value
                            .iter()
                            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))))
            {
                return Err(error(GitWorkspaceErrorCode::MalformedOutput));
            }
            oid = Some(value.to_vec());
            continue;
        }
        if let Some(value) = record.strip_prefix(b"# branch.head ") {
            if branch.is_some() || value.is_empty() {
                return Err(error(GitWorkspaceErrorCode::MalformedOutput));
            }
            branch = Some(value.to_vec());
            continue;
        }
        if record.starts_with(b"# branch.upstream ") || record.starts_with(b"# branch.ab ") {
            continue;
        }
        match record[0] {
            b'1' => {
                let fields = split_prefix_fields(record, 8)?;
                validate_ordinary_fields(&fields, false)?;
                let (staged, unstaged) = parse_xy(fields[1])?;
                insert_status(&mut files, fields[8], None, staged, unstaged)?;
            }
            b'2' => {
                let fields = split_prefix_fields(record, 9)?;
                validate_ordinary_fields(&fields, true)?;
                let old = records
                    .next()
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
                let (staged, unstaged) = parse_xy(fields[1])?;
                insert_status(&mut files, fields[9], Some(old), staged, unstaged)?;
            }
            b'u' => {
                let fields = split_prefix_fields(record, 10)?;
                if fields[0] != b"u"
                    || !valid_sub(fields[2])
                    || !fields[3..=6].iter().all(|field| valid_mode(field))
                    || !consistent_oids(&fields[7..=9])
                {
                    return Err(error(GitWorkspaceErrorCode::MalformedOutput));
                }
                insert_status(
                    &mut files,
                    fields[10],
                    None,
                    WorkspaceChangeKind::Unmerged,
                    WorkspaceChangeKind::Unmerged,
                )?;
            }
            b'?' => {
                let path = record
                    .strip_prefix(b"? ")
                    .filter(|path| !path.is_empty())
                    .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
                insert_status(
                    &mut files,
                    path,
                    None,
                    WorkspaceChangeKind::Unchanged,
                    WorkspaceChangeKind::Untracked,
                )?;
            }
            _ => return Err(error(GitWorkspaceErrorCode::MalformedOutput)),
        }
        if files.len() > PATH_LIMIT {
            return Err(error(GitWorkspaceErrorCode::OutputTooLarge));
        }
    }
    let oid = oid.ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
    let branch = branch.ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
    let branch_label = if branch == b"(detached)" {
        None
    } else {
        Some(escape_ref(&branch))
    };
    let head = if oid == b"(initial)" {
        WorkspaceHead::Unborn {
            label: branch_label,
        }
    } else if branch == b"(detached)" {
        WorkspaceHead::Detached
    } else {
        WorkspaceHead::Branch {
            label: branch_label.ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?,
        }
    };
    Ok(ParsedStatus { head, files })
}

fn split_prefix_fields(record: &[u8], spaces: usize) -> Result<Vec<&[u8]>, GitWorkspaceError> {
    let mut fields = Vec::with_capacity(spaces + 1);
    let mut start = 0;
    for _ in 0..spaces {
        let relative = record[start..]
            .iter()
            .position(|byte| *byte == b' ')
            .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
        let end = start + relative;
        fields.push(&record[start..end]);
        start = end + 1;
    }
    if start >= record.len() {
        return Err(error(GitWorkspaceErrorCode::MalformedOutput));
    }
    fields.push(&record[start..]);
    Ok(fields)
}

fn validate_ordinary_fields(fields: &[&[u8]], renamed: bool) -> Result<(), GitWorkspaceError> {
    if fields[0] != if renamed { b"2" } else { b"1" }
        || !valid_sub(fields[2])
        || !fields[3..=5].iter().all(|field| valid_mode(field))
        || !consistent_oids(&fields[6..=7])
    {
        return Err(error(GitWorkspaceErrorCode::MalformedOutput));
    }
    if renamed {
        let score = fields[8];
        if !matches!(score.first(), Some(b'R' | b'C'))
            || !(2..=4).contains(&score.len())
            || !score[1..].iter().all(u8::is_ascii_digit)
            || std::str::from_utf8(&score[1..])
                .ok()
                .and_then(|value| value.parse::<u16>().ok())
                .is_none_or(|value| value > 100)
            || !fields[1].contains(&score[0])
        {
            return Err(error(GitWorkspaceErrorCode::MalformedOutput));
        }
    }
    Ok(())
}

fn valid_sub(value: &[u8]) -> bool {
    value == b"N..."
        || (value.len() == 4
            && value[0] == b'S'
            && matches!(value[1], b'.' | b'C')
            && matches!(value[2], b'.' | b'M')
            && matches!(value[3], b'.' | b'U'))
}

fn consistent_oids(values: &[&[u8]]) -> bool {
    let Some(width) = values.first().map(|value| value.len()) else {
        return false;
    };
    matches!(width, 40 | 64)
        && values.iter().all(|value| {
            value.len() == width
                && value
                    .iter()
                    .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        })
}

fn parse_xy(value: &[u8]) -> Result<(WorkspaceChangeKind, WorkspaceChangeKind), GitWorkspaceError> {
    if value.len() != 2 {
        return Err(error(GitWorkspaceErrorCode::MalformedOutput));
    }
    Ok((parse_change(value[0])?, parse_change(value[1])?))
}

fn parse_change(value: u8) -> Result<WorkspaceChangeKind, GitWorkspaceError> {
    match value {
        b'.' => Ok(WorkspaceChangeKind::Unchanged),
        b'A' => Ok(WorkspaceChangeKind::Added),
        b'M' => Ok(WorkspaceChangeKind::Modified),
        b'D' => Ok(WorkspaceChangeKind::Deleted),
        b'R' => Ok(WorkspaceChangeKind::Renamed),
        b'C' => Ok(WorkspaceChangeKind::Copied),
        b'T' => Ok(WorkspaceChangeKind::TypeChanged),
        b'U' => Ok(WorkspaceChangeKind::Unmerged),
        _ => Err(error(GitWorkspaceErrorCode::MalformedOutput)),
    }
}

fn insert_status(
    files: &mut BTreeMap<Vec<u8>, ParsedFile>,
    path: &[u8],
    previous_path: Option<&[u8]>,
    staged: WorkspaceChangeKind,
    unstaged: WorkspaceChangeKind,
) -> Result<(), GitWorkspaceError> {
    validate_relative_path(path)?;
    if let Some(previous) = previous_path {
        validate_relative_path(previous)?;
    }
    if files.contains_key(path) {
        return Err(error(GitWorkspaceErrorCode::MalformedOutput));
    }
    files.insert(
        path.to_vec(),
        ParsedFile {
            path: path.to_vec(),
            previous_path: previous_path.map(<[u8]>::to_vec),
            staged,
            unstaged,
            additions: WorkspaceLineCount::Unknown,
            deletions: WorkspaceLineCount::Unknown,
        },
    );
    Ok(())
}

fn validate_relative_path(path: &[u8]) -> Result<(), GitWorkspaceError> {
    if path.is_empty()
        || path[0] == b'/'
        || path
            .split(|byte| *byte == b'/')
            .any(|part| part.is_empty() || part == b"." || part == b".." || part == b".git")
    {
        return Err(error(GitWorkspaceErrorCode::MalformedOutput));
    }
    Ok(())
}

struct RawEntry {
    path: Vec<u8>,
    previous_path: Option<Vec<u8>>,
    kind: WorkspaceChangeKind,
}

fn validate_raw(bytes: &[u8]) -> Result<Vec<RawEntry>, GitWorkspaceError> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    if !bytes.ends_with(&[0]) {
        return Err(error(GitWorkspaceErrorCode::MalformedOutput));
    }
    let mut records = bytes.split(|byte| *byte == 0).peekable();
    let mut entries = Vec::new();
    let mut oid_width = None;
    while let Some(header) = records.next() {
        if header.is_empty() && records.peek().is_none() {
            break;
        }
        let header = header
            .strip_prefix(b":")
            .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
        let mut pieces = header.splitn(5, |byte| *byte == b' ');
        let old_mode = pieces.next().ok_or_else(malformed)?;
        let new_mode = pieces.next().ok_or_else(malformed)?;
        let old_oid = pieces.next().ok_or_else(malformed)?;
        let new_oid = pieces.next().ok_or_else(malformed)?;
        if !valid_mode(old_mode)
            || !valid_mode(new_mode)
            || !valid_oid(old_oid)
            || !valid_oid(new_oid)
            || old_oid.len() != new_oid.len()
        {
            return Err(error(GitWorkspaceErrorCode::MalformedOutput));
        }
        if oid_width
            .replace(old_oid.len())
            .is_some_and(|width| width != old_oid.len())
        {
            return Err(error(GitWorkspaceErrorCode::MalformedOutput));
        }
        let status = pieces
            .next()
            .filter(|piece| !piece.is_empty())
            .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
        if !valid_raw_status(status) {
            return Err(error(GitWorkspaceErrorCode::MalformedOutput));
        }
        let path = records
            .next()
            .filter(|piece| !piece.is_empty())
            .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
        validate_relative_path(path)?;
        let mut previous_path = None;
        if matches!(status[0], b'R' | b'C') {
            let second = records
                .next()
                .filter(|piece| !piece.is_empty())
                .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
            validate_relative_path(second)?;
            previous_path = Some(path.to_vec());
            entries.push(RawEntry {
                path: second.to_vec(),
                previous_path,
                kind: if status[0] == b'R' {
                    WorkspaceChangeKind::Renamed
                } else {
                    WorkspaceChangeKind::Copied
                },
            });
        } else {
            entries.push(RawEntry {
                path: path.to_vec(),
                previous_path,
                kind: parse_change(status[0])?,
            });
        }
    }
    Ok(entries)
}

fn cross_check_raw(
    files: &BTreeMap<Vec<u8>, ParsedFile>,
    entries: &[RawEntry],
    staged: bool,
) -> Result<(), GitWorkspaceError> {
    let expected: BTreeMap<&[u8], (&ParsedFile, WorkspaceChangeKind)> = files
        .values()
        .filter_map(|file| {
            let kind = if staged { file.staged } else { file.unstaged };
            (kind != WorkspaceChangeKind::Unchanged && kind != WorkspaceChangeKind::Untracked)
                .then_some((file.path.as_slice(), (file, kind)))
        })
        .collect();
    if entries.len() != expected.len() {
        return Err(error(GitWorkspaceErrorCode::MalformedOutput));
    }
    for entry in entries {
        let (file, kind) = expected
            .get(entry.path.as_slice())
            .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
        let expected_previous = matches!(
            kind,
            WorkspaceChangeKind::Renamed | WorkspaceChangeKind::Copied
        )
        .then_some(file.previous_path.as_ref())
        .flatten();
        if *kind != entry.kind
            || expected_previous.map(Vec::as_slice) != entry.previous_path.as_deref()
        {
            return Err(error(GitWorkspaceErrorCode::MalformedOutput));
        }
    }
    Ok(())
}

fn malformed() -> GitWorkspaceError {
    error(GitWorkspaceErrorCode::MalformedOutput)
}

fn valid_mode(mode: &[u8]) -> bool {
    matches!(
        mode,
        b"000000" | b"100644" | b"100755" | b"120000" | b"160000"
    )
}

fn valid_oid(oid: &[u8]) -> bool {
    matches!(oid.len(), 40 | 64)
        && oid
            .iter()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn valid_raw_status(status: &[u8]) -> bool {
    match status.first() {
        Some(b'M' | b'A' | b'D' | b'T' | b'U') => status.len() == 1,
        Some(b'R' | b'C') => {
            (2..=4).contains(&status.len())
                && status[1..].iter().all(u8::is_ascii_digit)
                && std::str::from_utf8(&status[1..])
                    .ok()
                    .and_then(|value| value.parse::<u16>().ok())
                    .is_some_and(|score| score <= 100)
        }
        _ => false,
    }
}

fn merge_numstat(
    files: &mut BTreeMap<Vec<u8>, ParsedFile>,
    bytes: &[u8],
    staged: bool,
) -> Result<Vec<Vec<u8>>, GitWorkspaceError> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    if !bytes.ends_with(&[0]) {
        return Err(error(GitWorkspaceErrorCode::MalformedOutput));
    }
    let mut records = bytes.split(|byte| *byte == 0).peekable();
    let mut paths = Vec::new();
    while let Some(record) = records.next() {
        if record.is_empty() && records.peek().is_none() {
            break;
        }
        let first_tab = record
            .iter()
            .position(|byte| *byte == b'\t')
            .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
        let second_relative = record[first_tab + 1..]
            .iter()
            .position(|byte| *byte == b'\t')
            .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
        let second_tab = first_tab + 1 + second_relative;
        let additions = parse_count(&record[..first_tab])?;
        let deletions = parse_count(&record[first_tab + 1..second_tab])?;
        if matches!(additions, WorkspaceLineCount::Binary)
            != matches!(deletions, WorkspaceLineCount::Binary)
        {
            return Err(error(GitWorkspaceErrorCode::MalformedOutput));
        }
        let inline_path = &record[second_tab + 1..];
        let mut rename_old = None;
        let path = if inline_path.is_empty() {
            let old = records
                .next()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
            let new = records
                .next()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
            validate_relative_path(old)?;
            rename_old = Some(old);
            new
        } else {
            inline_path
        };
        validate_relative_path(path)?;
        let file = files
            .get_mut(path)
            .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
        let layer_kind = if staged { file.staged } else { file.unstaged };
        let expected_old = matches!(
            layer_kind,
            WorkspaceChangeKind::Renamed | WorkspaceChangeKind::Copied
        )
        .then_some(file.previous_path.as_deref())
        .flatten();
        if rename_old != expected_old {
            return Err(error(GitWorkspaceErrorCode::MalformedOutput));
        }
        file.additions = merge_count(file.additions, additions)?;
        file.deletions = merge_count(file.deletions, deletions)?;
        paths.push(path.to_vec());
    }
    Ok(paths)
}

fn parse_count(bytes: &[u8]) -> Result<WorkspaceLineCount, GitWorkspaceError> {
    if bytes == b"-" {
        return Ok(WorkspaceLineCount::Binary);
    }
    let string =
        std::str::from_utf8(bytes).map_err(|_| error(GitWorkspaceErrorCode::MalformedOutput))?;
    let value = string
        .parse::<u64>()
        .map_err(|_| error(GitWorkspaceErrorCode::MalformedOutput))?;
    Ok(WorkspaceLineCount::Known(value))
}

fn merge_count(
    a: WorkspaceLineCount,
    b: WorkspaceLineCount,
) -> Result<WorkspaceLineCount, GitWorkspaceError> {
    Ok(match (a, b) {
        (WorkspaceLineCount::Binary, _) | (_, WorkspaceLineCount::Binary) => {
            WorkspaceLineCount::Binary
        }
        (WorkspaceLineCount::Known(a), WorkspaceLineCount::Known(b)) => WorkspaceLineCount::Known(
            a.checked_add(b)
                .ok_or_else(|| error(GitWorkspaceErrorCode::OutputTooLarge))?,
        ),
        (WorkspaceLineCount::Unknown, value) | (value, WorkspaceLineCount::Unknown) => value,
    })
}

fn fold_count(total: &mut Option<u64>, value: WorkspaceLineCount) -> Result<(), GitWorkspaceError> {
    match value {
        WorkspaceLineCount::Known(value) => {
            if let Some(total) = total {
                *total = total
                    .checked_add(value)
                    .ok_or_else(|| error(GitWorkspaceErrorCode::OutputTooLarge))?;
            }
        }
        WorkspaceLineCount::Binary | WorkspaceLineCount::Unknown => *total = None,
    }
    Ok(())
}

fn build_projection(
    runner: &Runner,
    file: PrivateFile,
    cancel: &CancellationToken,
) -> Result<DiffTextProjection, GitWorkspaceError> {
    verify_snapshot_identity(runner, &file.snapshot_identity, cancel)?;
    if file.binary || file.staged == WorkspaceChangeKind::Unmerged {
        return Err(error(GitWorkspaceErrorCode::MetadataOnly));
    }
    let mut sections = Vec::new();
    if file.unstaged == WorkspaceChangeKind::Untracked {
        sections.push(project_untracked(runner, &file, cancel)?);
    } else {
        for (layer, changed) in [
            (
                DiffLayer::Staged,
                file.staged != WorkspaceChangeKind::Unchanged,
            ),
            (
                DiffLayer::Unstaged,
                file.unstaged != WorkspaceChangeKind::Unchanged,
            ),
        ] {
            if !changed {
                continue;
            }
            let mut args = vec![
                OsString::from("--patch"),
                OsString::from("--find-renames"),
                OsString::from("--no-ext-diff"),
                OsString::from("--no-textconv"),
                OsString::from("--unified=3"),
            ];
            if layer == DiffLayer::Staged {
                args.insert(0, OsString::from("--cached"));
            }
            args.push(OsString::from("--"));
            if let Some(previous) = &file.previous_path {
                args.push(previous.clone());
            }
            args.push(file.path.clone());
            let output = runner.run("diff", &args, PATCH_LIMIT, cancel)?;
            sections.push(parse_patch(layer, &output.stdout)?);
        }
    }
    let projection = DiffTextProjection {
        file_id: file.id,
        language: file.language,
        sections,
    };
    verify_snapshot_identity(runner, &file.snapshot_identity, cancel)?;
    Ok(projection)
}

fn project_untracked(
    runner: &Runner,
    file: &PrivateFile,
    cancel: &CancellationToken,
) -> Result<DiffSection, GitWorkspaceError> {
    if cancel.is_cancelled() {
        return Err(error(GitWorkspaceErrorCode::Cancelled));
    }
    let path = runner.root.join(&file.path);
    let canonical =
        fs::canonicalize(&path).map_err(|_| error(GitWorkspaceErrorCode::ChangedDuringRead))?;
    if canonical != path || !canonical.starts_with(&runner.root) {
        return Err(error(GitWorkspaceErrorCode::MetadataOnly));
    }
    let before =
        fs::symlink_metadata(&path).map_err(|_| error(GitWorkspaceErrorCode::ChangedDuringRead))?;
    if !before.file_type().is_file() || before.nlink() > 1 || before.size() > PATCH_LIMIT as u64 {
        return Err(error(GitWorkspaceErrorCode::MetadataOnly));
    }
    let mut reader = File::open(&path).map_err(|_| error(GitWorkspaceErrorCode::MetadataOnly))?;
    let mut bytes = Vec::with_capacity((before.size() as usize).min(PATCH_LIMIT));
    let mut chunk = [0_u8; IO_CHUNK];
    loop {
        let read = reader
            .read(&mut chunk)
            .map_err(|_| error(GitWorkspaceErrorCode::MetadataOnly))?;
        if read == 0 {
            break;
        }
        if bytes.len().saturating_add(read) > PATCH_LIMIT {
            return Err(error(GitWorkspaceErrorCode::OutputTooLarge));
        }
        bytes.extend_from_slice(&chunk[..read]);
        if cancel.is_cancelled() {
            return Err(error(GitWorkspaceErrorCode::Cancelled));
        }
    }
    let after =
        fs::symlink_metadata(&path).map_err(|_| error(GitWorkspaceErrorCode::ChangedDuringRead))?;
    let after_canonical =
        fs::canonicalize(&path).map_err(|_| error(GitWorkspaceErrorCode::ChangedDuringRead))?;
    if after_canonical != canonical || identity_tuple(&before) != identity_tuple(&after) {
        return Err(error(GitWorkspaceErrorCode::ChangedDuringRead));
    }
    let text =
        std::str::from_utf8(&bytes).map_err(|_| error(GitWorkspaceErrorCode::MetadataOnly))?;
    if text.as_bytes().contains(&0) {
        return Err(error(GitWorkspaceErrorCode::MetadataOnly));
    }
    let mut rows = Vec::new();
    for (index, line) in logical_lines(text).enumerate() {
        if line.len() > PATCH_LINE_LIMIT || rows.len() == PATCH_ROW_LIMIT {
            return Err(error(GitWorkspaceErrorCode::OutputTooLarge));
        }
        rows.push(DiffRow {
            kind: DiffRowKind::Addition,
            old_line: None,
            new_line: Some(
                u32::try_from(index + 1)
                    .map_err(|_| error(GitWorkspaceErrorCode::OutputTooLarge))?,
            ),
            text: line.to_owned(),
        });
    }
    Ok(DiffSection {
        layer: DiffLayer::Untracked,
        hunks: vec![DiffHunk {
            old_start: 0,
            old_count: 0,
            new_start: if rows.is_empty() { 0 } else { 1 },
            new_count: u32::try_from(rows.len())
                .map_err(|_| error(GitWorkspaceErrorCode::OutputTooLarge))?,
            heading_suffix: None,
            missing_trailing_newline: !bytes.ends_with(b"\n") && !bytes.is_empty(),
            rows,
        }],
    })
}

fn identity_tuple(metadata: &fs::Metadata) -> (u64, u64, u64, i64, i64) {
    (
        metadata.dev(),
        metadata.ino(),
        metadata.size(),
        metadata.mtime(),
        metadata.mtime_nsec(),
    )
}

fn parse_patch(layer: DiffLayer, bytes: &[u8]) -> Result<DiffSection, GitWorkspaceError> {
    if bytes.len() > PATCH_LIMIT {
        return Err(error(GitWorkspaceErrorCode::OutputTooLarge));
    }
    let text =
        std::str::from_utf8(bytes).map_err(|_| error(GitWorkspaceErrorCode::MetadataOnly))?;
    if text.contains("Binary files ") || text.contains("GIT binary patch") {
        return Err(error(GitWorkspaceErrorCode::MetadataOnly));
    }
    let mut hunks = Vec::new();
    let mut current: Option<DiffHunk> = None;
    let mut old_line = 0_u32;
    let mut new_line = 0_u32;
    let mut rows = 0_usize;
    for line in logical_lines(text) {
        if line.starts_with("@@ ") {
            if line.len() > PATCH_LINE_LIMIT {
                return Err(error(GitWorkspaceErrorCode::OutputTooLarge));
            }
            if let Some(hunk) = current.take() {
                validate_hunk(&hunk)?;
                hunks.push(hunk);
            }
            let (old_start, old_count, new_start, new_count, heading_suffix) =
                parse_hunk_header(line)?;
            old_line = old_start;
            new_line = new_start;
            current = Some(DiffHunk {
                old_start,
                old_count,
                new_start,
                new_count,
                heading_suffix,
                missing_trailing_newline: false,
                rows: Vec::new(),
            });
            continue;
        }
        let Some(hunk) = current.as_mut() else {
            continue;
        };
        let Some((&prefix, body)) = line.as_bytes().split_first() else {
            return Err(error(GitWorkspaceErrorCode::MalformedOutput));
        };
        if body.len() > PATCH_LINE_LIMIT {
            return Err(error(GitWorkspaceErrorCode::OutputTooLarge));
        }
        if prefix == b'\\' {
            hunk.missing_trailing_newline = true;
            continue;
        }
        rows += 1;
        if rows > PATCH_ROW_LIMIT {
            return Err(error(GitWorkspaceErrorCode::OutputTooLarge));
        }
        let body = std::str::from_utf8(body)
            .map_err(|_| error(GitWorkspaceErrorCode::MetadataOnly))?
            .to_owned();
        let row = match prefix {
            b' ' => {
                let row = DiffRow {
                    kind: DiffRowKind::Context,
                    old_line: Some(old_line),
                    new_line: Some(new_line),
                    text: body,
                };
                old_line = old_line.saturating_add(1);
                new_line = new_line.saturating_add(1);
                row
            }
            b'-' => {
                let row = DiffRow {
                    kind: DiffRowKind::Deletion,
                    old_line: Some(old_line),
                    new_line: None,
                    text: body,
                };
                old_line = old_line.saturating_add(1);
                row
            }
            b'+' => {
                let row = DiffRow {
                    kind: DiffRowKind::Addition,
                    old_line: None,
                    new_line: Some(new_line),
                    text: body,
                };
                new_line = new_line.saturating_add(1);
                row
            }
            _ => return Err(error(GitWorkspaceErrorCode::MalformedOutput)),
        };
        hunk.rows.push(row);
    }
    if let Some(hunk) = current {
        validate_hunk(&hunk)?;
        hunks.push(hunk);
    }
    Ok(DiffSection { layer, hunks })
}

fn parse_hunk_header(
    line: &str,
) -> Result<(u32, u32, u32, u32, Option<String>), GitWorkspaceError> {
    let end = line[3..]
        .find(" @@")
        .map(|index| index + 3)
        .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
    let mut ranges = line[3..end].split(' ');
    let old = ranges
        .next()
        .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
    let new = ranges
        .next()
        .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
    if ranges.next().is_some() {
        return Err(error(GitWorkspaceErrorCode::MalformedOutput));
    }
    let (old_start, old_count) = parse_range(old, '-')?;
    let (new_start, new_count) = parse_range(new, '+')?;
    let suffix = line[end + 3..]
        .strip_prefix(' ')
        .unwrap_or(&line[end + 3..]);
    let suffix = (!suffix.is_empty()).then(|| suffix.to_owned());
    Ok((old_start, old_count, new_start, new_count, suffix))
}

fn parse_range(value: &str, prefix: char) -> Result<(u32, u32), GitWorkspaceError> {
    let value = value
        .strip_prefix(prefix)
        .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
    let (start, count) = value.split_once(',').unwrap_or((value, "1"));
    let start = start
        .parse()
        .map_err(|_| error(GitWorkspaceErrorCode::MalformedOutput))?;
    let count = count
        .parse()
        .map_err(|_| error(GitWorkspaceErrorCode::MalformedOutput))?;
    Ok((start, count))
}

fn validate_hunk(hunk: &DiffHunk) -> Result<(), GitWorkspaceError> {
    let old = hunk
        .rows
        .iter()
        .filter(|row| row.kind != DiffRowKind::Addition)
        .count();
    let new = hunk
        .rows
        .iter()
        .filter(|row| row.kind != DiffRowKind::Deletion)
        .count();
    if old != hunk.old_count as usize || new != hunk.new_count as usize {
        return Err(error(GitWorkspaceErrorCode::MalformedOutput));
    }
    Ok(())
}

fn logical_lines(text: &str) -> impl Iterator<Item = &str> {
    text.split_terminator('\n')
}

fn language_for(path: &[u8]) -> DiffLanguage {
    let name = path.rsplit(|byte| *byte == b'/').next().unwrap_or(path);
    let extension = name
        .iter()
        .rposition(|byte| *byte == b'.')
        .map(|position| &name[position + 1..]);
    match extension {
        Some(b"rs") => DiffLanguage::Rust,
        Some(b"ts") => DiffLanguage::TypeScript,
        Some(b"tsx") => DiffLanguage::Tsx,
        Some(b"js" | b"jsx" | b"mjs" | b"cjs") => DiffLanguage::JavaScript,
        Some(b"py") => DiffLanguage::Python,
        _ => DiffLanguage::Plain,
    }
}

fn escape_path(path: &[u8]) -> String {
    escape_bytes(path)
}

fn escape_ref(reference: &[u8]) -> String {
    escape_bytes(reference)
}

fn escape_bytes(bytes: &[u8]) -> String {
    if let Ok(value) = std::str::from_utf8(bytes) {
        let mut escaped = String::new();
        for character in value.chars() {
            if character.is_control() || is_bidi_control(character) {
                for byte in character.to_string().bytes() {
                    escaped.push_str(&format!("\\x{byte:02x}"));
                }
            } else if character == '\\' {
                escaped.push_str("\\\\");
            } else {
                escaped.push(character);
            }
        }
        return escaped;
    }
    let mut escaped = String::new();
    for byte in bytes {
        if (0x20..=0x7e).contains(byte) && *byte != b'\\' {
            escaped.push(char::from(*byte));
        } else if *byte == b'\\' {
            escaped.push_str("\\\\");
        } else {
            escaped.push_str(&format!("\\x{byte:02x}"));
        }
    }
    escaped
}

fn is_bidi_control(character: char) -> bool {
    matches!(
        character,
        '\u{061c}'
            | '\u{200e}'
            | '\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2066}'..='\u{2069}'
    )
}

fn seal(
    identity: RootIdentity,
    instance_nonce: u64,
    generation: u64,
    slot: u32,
    path: &[u8],
) -> u64 {
    let mut value = identity.dev
        ^ identity.ino.rotate_left(17)
        ^ instance_nonce.rotate_left(23)
        ^ generation.rotate_left(31);
    value ^= u64::from(slot).rotate_left(7);
    for byte in path {
        value ^= u64::from(*byte);
        value = value.wrapping_mul(0x100_0000_01b3);
    }
    value
}

fn estimate_snapshot_bytes(snapshot: &WorkspaceSnapshot) -> usize {
    snapshot.files.iter().fold(0_usize, |total, file| {
        total
            .saturating_add(std::mem::size_of::<WorkspaceFile>())
            .saturating_add(file.label.len())
            .saturating_add(file.previous_label.as_ref().map_or(0, String::len))
    })
}

fn trim_one_newline(bytes: &[u8]) -> &[u8] {
    bytes.strip_suffix(b"\n").unwrap_or(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;
    use std::os::unix::fs::{PermissionsExt, symlink};
    use tempfile::{TempDir, tempdir};

    struct Repo {
        dir: TempDir,
    }

    impl Repo {
        fn new() -> Self {
            let dir = tempdir().unwrap();
            git(dir.path(), &["init", "-q"]);
            git(dir.path(), &["config", "user.name", "Vega Test"]);
            git(
                dir.path(),
                &["config", "user.email", "vega@example.invalid"],
            );
            Self { dir }
        }

        fn path(&self) -> &Path {
            self.dir.path()
        }

        fn write(&self, path: &str, body: &[u8]) {
            let path = self.path().join(path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(path, body).unwrap();
        }

        fn commit_all(&self) {
            git(self.path(), &["add", "-A"]);
            git(self.path(), &["commit", "-q", "-m", "fixture"]);
        }
    }

    fn git(root: &Path, args: &[&str]) {
        let status = Command::new(GIT)
            .current_dir(root)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }

    #[tokio::test]
    async fn git_workspace_clean_staged_unstaged_untracked_and_structured_projection() {
        let repo = Repo::new();
        repo.write("src/lib.rs", b"one\ntwo\n");
        repo.commit_all();
        let service = GitWorkspaceService::new(repo.path()).unwrap();
        let clean = service.refresh(CancellationToken::new()).await.unwrap();
        assert!(clean.files.is_empty());

        repo.write("src/lib.rs", b"ONE\ntwo\n");
        git(repo.path(), &["add", "src/lib.rs"]);
        repo.write("src/lib.rs", b"ONE\nTWO\n");
        repo.write("new.ts", b"export const value = 1;\n");
        let snapshot = service.refresh(CancellationToken::new()).await.unwrap();
        assert_eq!(snapshot.files.len(), 2);
        let tracked = snapshot
            .files
            .iter()
            .find(|file| file.label == "src/lib.rs")
            .unwrap();
        assert_eq!(tracked.staged, WorkspaceChangeKind::Modified);
        assert_eq!(tracked.unstaged, WorkspaceChangeKind::Modified);
        assert_eq!(tracked.language, DiffLanguage::Rust);
        let projection = service
            .diff(tracked.id, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(projection.sections.len(), 2);
        assert!(
            projection
                .sections
                .iter()
                .all(|section| !section.hunks.is_empty())
        );

        let untracked = snapshot
            .files
            .iter()
            .find(|file| file.label == "new.ts")
            .unwrap();
        assert_eq!(untracked.unstaged, WorkspaceChangeKind::Untracked);
        assert_eq!(untracked.additions, WorkspaceLineCount::Unknown);
        let projection = service
            .diff(untracked.id, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(projection.sections[0].layer, DiffLayer::Untracked);
        assert_eq!(projection.sections[0].hunks[0].rows.len(), 1);
    }

    #[tokio::test]
    async fn git_workspace_delete_rename_space_and_literal_magic_names() {
        let repo = Repo::new();
        repo.write("delete.txt", b"delete-only\n");
        for path in ["old name.txt", ":(glob)**", ":!safe"] {
            repo.write(path, b"body\n");
        }
        repo.commit_all();
        fs::remove_file(repo.path().join("delete.txt")).unwrap();
        fs::rename(
            repo.path().join("old name.txt"),
            repo.path().join("new name.txt"),
        )
        .unwrap();
        repo.write(":(glob)**", b"changed glob\n");
        repo.write(":!safe", b"changed exclude\n");
        git(repo.path(), &["add", "-A"]);
        repo.write("new name.txt", b"body\nafter-rename\n");
        let service = GitWorkspaceService::new(repo.path()).unwrap();
        let snapshot = service.refresh(CancellationToken::new()).await.unwrap();
        assert!(
            snapshot
                .files
                .iter()
                .any(|file| file.staged == WorkspaceChangeKind::Deleted)
        );
        let renamed = snapshot
            .files
            .iter()
            .find(|file| file.label == "new name.txt")
            .unwrap();
        assert_eq!(renamed.previous_label.as_deref(), Some("old name.txt"));
        assert_eq!(renamed.staged, WorkspaceChangeKind::Renamed);
        assert_eq!(renamed.unstaged, WorkspaceChangeKind::Modified);
        assert_eq!(
            service
                .diff(renamed.id, CancellationToken::new())
                .await
                .unwrap()
                .sections
                .len(),
            2
        );
        for name in [":(glob)**", ":!safe"] {
            let file = snapshot
                .files
                .iter()
                .find(|file| file.label == name)
                .unwrap();
            let projection = service
                .diff(file.id, CancellationToken::new())
                .await
                .unwrap();
            assert!(!projection.sections[0].hunks.is_empty());
        }
    }

    #[test]
    fn git_workspace_unstaged_type_two_record_is_strictly_parsed() {
        let oid = "a".repeat(40);
        let status = format!(
            "# branch.oid {oid}\0# branch.head main\0\
             2 .R N... 100644 100644 100644 {oid} {oid} R100 after.txt\0before.txt\0"
        );
        let parsed = parse_status(status.as_bytes()).unwrap();
        let renamed = parsed.files.get(b"after.txt".as_slice()).unwrap();
        assert_eq!(renamed.staged, WorkspaceChangeKind::Unchanged);
        assert_eq!(renamed.unstaged, WorkspaceChangeKind::Renamed);
        assert_eq!(
            renamed.previous_path.as_deref(),
            Some(b"before.txt".as_slice())
        );
    }

    #[tokio::test]
    async fn git_workspace_binary_symlink_and_special_are_metadata_only() {
        let repo = Repo::new();
        repo.write("binary.bin", b"a\0b");
        symlink("binary.bin", repo.path().join("link")).unwrap();
        let service = GitWorkspaceService::new(repo.path()).unwrap();
        let snapshot = service.refresh(CancellationToken::new()).await.unwrap();
        for label in ["binary.bin", "link"] {
            let file = snapshot
                .files
                .iter()
                .find(|file| file.label == label)
                .unwrap();
            assert_eq!(
                service
                    .diff(file.id, CancellationToken::new())
                    .await
                    .unwrap_err()
                    .code(),
                GitWorkspaceErrorCode::MetadataOnly
            );
        }
    }

    #[tokio::test]
    async fn git_workspace_unborn_detached_nonrepo_and_stale_ids_are_typed() {
        let repo = Repo::new();
        let service = GitWorkspaceService::new(repo.path()).unwrap();
        let unborn = service.refresh(CancellationToken::new()).await.unwrap();
        assert!(matches!(unborn.head, WorkspaceHead::Unborn { .. }));
        repo.write("a.py", b"print(1)\n");
        let first = service.refresh(CancellationToken::new()).await.unwrap();
        let stale = first.files[0].id;
        let other_service = GitWorkspaceService::new(repo.path()).unwrap();
        other_service
            .refresh(CancellationToken::new())
            .await
            .unwrap();
        other_service
            .refresh(CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(
            other_service
                .diff(stale, CancellationToken::new())
                .await
                .unwrap_err()
                .code(),
            GitWorkspaceErrorCode::UnknownFile
        );
        repo.write("b.txt", b"b\n");
        service.refresh(CancellationToken::new()).await.unwrap();
        assert_eq!(
            service
                .diff(stale, CancellationToken::new())
                .await
                .unwrap_err()
                .code(),
            GitWorkspaceErrorCode::StaleGeneration
        );
        repo.commit_all();
        git(repo.path(), &["checkout", "--detach", "-q"]);
        assert!(matches!(
            service
                .refresh(CancellationToken::new())
                .await
                .unwrap()
                .head,
            WorkspaceHead::Detached
        ));

        let nonrepo = tempdir().unwrap();
        let service = GitWorkspaceService::new(nonrepo.path()).unwrap();
        assert_eq!(
            service
                .refresh(CancellationToken::new())
                .await
                .unwrap_err()
                .code(),
            GitWorkspaceErrorCode::NotRepository
        );
    }

    #[tokio::test]
    async fn git_workspace_escapes_control_bidi_and_non_utf8_paths_without_round_trip() {
        let repo = Repo::new();
        for bytes in [
            b"tab\tname.txt".to_vec(),
            b"line\nname.txt".to_vec(),
            "bidi\u{202e}name.txt".as_bytes().to_vec(),
        ] {
            fs::write(repo.path().join(OsString::from_vec(bytes)), b"body\n").unwrap();
        }
        let service = GitWorkspaceService::new(repo.path()).unwrap();
        let snapshot = service.refresh(CancellationToken::new()).await.unwrap();
        assert_eq!(snapshot.files.len(), 3);
        let labels: Vec<&str> = snapshot
            .files
            .iter()
            .map(|file| file.label.as_str())
            .collect();
        assert!(labels.iter().any(|label| label.contains("\\x09")));
        assert!(labels.iter().any(|label| label.contains("\\x0a")));
        assert!(labels.iter().any(|label| label.contains("\\xe2\\x80\\xae")));
        assert_eq!(escape_path(b"invalid-\xff.rs"), "invalid-\\xff.rs");
        assert!(
            labels
                .iter()
                .all(|label| !label.contains('\n') && !label.contains('\t'))
        );
    }

    #[test]
    fn git_workspace_parsers_fail_closed_and_extension_map_is_frozen() {
        for malformed in [
            b"# branch.oid (initial)\0# branch.head main".as_slice(),
            b"# branch.oid (initial)\0# branch.head main\0x unknown\0".as_slice(),
            b"# branch.oid (initial)\0# branch.head main\0? ../escape\0".as_slice(),
        ] {
            let error = match parse_status(malformed) {
                Ok(_) => panic!("malformed status was accepted"),
                Err(error) => error,
            };
            assert_eq!(error.code(), GitWorkspaceErrorCode::MalformedOutput);
        }
        let error = match validate_raw(b":100644 100644 a b M file") {
            Ok(_) => panic!("malformed raw output was accepted"),
            Err(error) => error,
        };
        assert_eq!(error.code(), GitWorkspaceErrorCode::MalformedOutput);
        let mut files = BTreeMap::new();
        assert_eq!(
            merge_numstat(&mut files, b"1\t2\tfile", true)
                .unwrap_err()
                .code(),
            GitWorkspaceErrorCode::MalformedOutput
        );
        files.insert(
            b"file".to_vec(),
            ParsedFile {
                path: b"file".to_vec(),
                previous_path: None,
                staged: WorkspaceChangeKind::Modified,
                unstaged: WorkspaceChangeKind::Unchanged,
                additions: WorkspaceLineCount::Unknown,
                deletions: WorkspaceLineCount::Unknown,
            },
        );
        assert_eq!(
            merge_numstat(&mut files, b"-\t1\tfile\0", true)
                .unwrap_err()
                .code(),
            GitWorkspaceErrorCode::MalformedOutput
        );
        for (path, expected) in [
            (b"a.rs".as_slice(), DiffLanguage::Rust),
            (b"a.ts", DiffLanguage::TypeScript),
            (b"a.tsx", DiffLanguage::Tsx),
            (b"a.js", DiffLanguage::JavaScript),
            (b"a.jsx", DiffLanguage::JavaScript),
            (b"a.mjs", DiffLanguage::JavaScript),
            (b"a.cjs", DiffLanguage::JavaScript),
            (b"a.py", DiffLanguage::Python),
            (b"a.go", DiffLanguage::Plain),
        ] {
            assert_eq!(language_for(path), expected);
        }
    }

    #[test]
    fn git_workspace_projection_redaction_and_service_debug_are_safe() {
        let id = WorkspaceFileId {
            generation: 1,
            slot: 2,
            seal: 3,
        };
        let projection = DiffTextProjection {
            file_id: id,
            language: DiffLanguage::Plain,
            sections: vec![DiffSection {
                layer: DiffLayer::Untracked,
                hunks: vec![DiffHunk {
                    old_start: 0,
                    old_count: 0,
                    new_start: 1,
                    new_count: 1,
                    heading_suffix: None,
                    missing_trailing_newline: false,
                    rows: vec![DiffRow {
                        kind: DiffRowKind::Addition,
                        old_line: None,
                        new_line: Some(1),
                        text: "LEAK_SENTINEL".into(),
                    }],
                }],
            }],
        };
        let debug = format!("{projection:?}");
        assert!(!debug.contains("LEAK_SENTINEL"));
        assert!(debug.contains("redacted"));
        let repo = Repo::new();
        let service = GitWorkspaceService::new(repo.path()).unwrap();
        assert!(!format!("{service:?}").contains(&repo.path().to_string_lossy().to_string()));
    }

    #[test]
    fn git_workspace_hunk_suffix_no_newline_and_line_cap_are_preserved() {
        let exact_line = "x".repeat(PATCH_LINE_LIMIT);
        let patch =
            format!("@@ -0,0 +1,1 @@ fn name\n+{exact_line}\n\\ No newline at end of file\n");
        let section = parse_patch(DiffLayer::Unstaged, patch.as_bytes()).unwrap();
        let hunk = &section.hunks[0];
        assert_eq!(hunk.heading_suffix.as_deref(), Some("fn name"));
        assert!(hunk.missing_trailing_newline);
        assert_eq!(hunk.rows[0].text.len(), PATCH_LINE_LIMIT);
        let too_long = format!("@@ -0,0 +1,1 @@\n+{}\n", "x".repeat(PATCH_LINE_LIMIT + 1));
        let error = match parse_patch(DiffLayer::Unstaged, too_long.as_bytes()) {
            Ok(_) => panic!("oversized line was accepted"),
            Err(error) => error,
        };
        assert_eq!(error.code(), GitWorkspaceErrorCode::OutputTooLarge);
    }

    #[test]
    fn git_workspace_environment_scrub_is_exact() {
        let mut command = Command::new("/usr/bin/true");
        command
            .env("GIT_DIR", "/private/leak")
            .env("GIT_CONFIG_COUNT", "9")
            .env("VEGA_KEEP", "yes");
        scrub_git_environment(&mut command);
        let env: HashMap<_, _> = command
            .get_envs()
            .map(|(key, value)| (key.to_owned(), value.map(OsStr::to_owned)))
            .collect();
        assert_eq!(env.get(OsStr::new("GIT_DIR")), Some(&None));
        assert_eq!(env.get(OsStr::new("GIT_CONFIG_COUNT")), Some(&None));
        assert_eq!(
            env.get(OsStr::new("GIT_LITERAL_PATHSPECS"))
                .and_then(|value| value.as_deref()),
            Some(OsStr::new("1"))
        );
        assert_eq!(
            env.get(OsStr::new("GIT_NO_LAZY_FETCH"))
                .and_then(|value| value.as_deref()),
            Some(OsStr::new("1"))
        );
        assert_eq!(
            env.get(OsStr::new("VEGA_KEEP"))
                .and_then(|value| value.as_deref()),
            Some(OsStr::new("yes"))
        );
    }

    #[tokio::test]
    async fn git_workspace_runner_scrubs_git_environment_and_bounds_output() {
        let repo = Repo::new();
        let script = repo.path().join("fixture-git");
        fs::write(
            &script,
            "#!/bin/sh\nif env | grep '^GIT_CONFIG_COUNT=' >/dev/null; then exit 90; fi\nif [ \"$GIT_LITERAL_PATHSPECS\" != 1 ] || [ \"$GIT_NO_LAZY_FETCH\" != 1 ]; then exit 91; fi\npython3 -c 'import sys; sys.stdout.write(\"x\" * (8 * 1024 * 1024 + 1))'\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&script, permissions).unwrap();
        let service = GitWorkspaceService::new_for_test(repo.path(), script).unwrap();
        assert_eq!(
            service
                .refresh(CancellationToken::new())
                .await
                .unwrap_err()
                .code(),
            GitWorkspaceErrorCode::OutputTooLarge
        );
    }

    #[tokio::test]
    async fn git_workspace_cancel_is_typed_and_reaps_fixture_group() {
        let repo = Repo::new();
        let script = repo.path().join("fixture-git");
        let pid_file = repo.path().join("descendant.pid");
        fs::write(
            &script,
            format!(
                "#!/bin/sh\nsleep 30 &\nchild=$!\nprintf '%s' \"$child\" > '{}'\nwait\n",
                pid_file.display()
            ),
        )
        .unwrap();
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&script, permissions).unwrap();
        let service = Arc::new(GitWorkspaceService::new_for_test(repo.path(), script).unwrap());
        let cancel = CancellationToken::new();
        let task = tokio::spawn({
            let service = service.clone();
            let cancel = cancel.clone();
            async move { service.refresh(cancel).await }
        });
        for _ in 0..50 {
            if pid_file.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        cancel.cancel();
        assert_eq!(
            task.await.unwrap().unwrap_err().code(),
            GitWorkspaceErrorCode::Cancelled
        );
        let pid = fs::read_to_string(pid_file).unwrap();
        let mut gone = false;
        for _ in 0..50 {
            let status = Command::new(KILL)
                .args(["-0", &pid])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap();
            if !status.success() {
                gone = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(gone, "descendant process survived cancellation");
    }
}
