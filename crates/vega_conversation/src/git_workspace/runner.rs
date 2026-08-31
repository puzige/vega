use super::*;

pub(crate) struct Runner {
    pub(crate) root: PathBuf,
    pub(crate) identity: RootIdentity,
    #[cfg(test)]
    pub(crate) executable: Option<PathBuf>,
}

pub(crate) struct Output {
    pub(crate) stdout: Vec<u8>,
    pub(crate) overflow: bool,
}

impl Runner {
    pub(crate) fn new(
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

    pub(crate) fn run(
        &self,
        verb: &'static str,
        args: &[OsString],
        stdout_limit: usize,
        cancel: &CancellationToken,
    ) -> Result<Output, GitWorkspaceError> {
        self.run_inner(verb, args, None, stdout_limit, cancel)
    }

    pub(crate) fn run_with_input(
        &self,
        verb: &'static str,
        args: &[OsString],
        input: Arc<[u8]>,
        stdout_limit: usize,
        cancel: &CancellationToken,
    ) -> Result<Output, GitWorkspaceError> {
        self.run_inner(verb, args, Some(input), stdout_limit, cancel)
    }

    pub(crate) fn run_inner(
        &self,
        verb: &'static str,
        args: &[OsString],
        input: Option<Arc<[u8]>>,
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
        command
            .args(PREFIX)
            .arg("--no-optional-locks")
            .arg(verb)
            .args(args);
        scrub_git_environment(&mut command);
        command
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        let mut child = command
            .spawn()
            .map_err(|_| error(GitWorkspaceErrorCode::SpawnFailed))?;
        collect_child(
            &mut child,
            input,
            stdout_limit,
            STDERR_LIMIT,
            READ_TIMEOUT,
            cancel,
            OverflowPolicy::IMMEDIATE,
        )
    }

    pub(crate) fn run_trusted_switch(
        &self,
        branch: &OsStr,
        cancel: &CancellationToken,
    ) -> Result<Output, GitWorkspaceError> {
        #[cfg(test)]
        let executable = self.executable.as_deref().unwrap_or_else(|| Path::new(GIT));
        #[cfg(not(test))]
        let executable = Path::new(GIT);
        self.run_trusted_switch_with_executable(branch, cancel, executable)
    }

    pub(crate) fn run_trusted_mutation(
        &self,
        verb: &'static str,
        args: &[OsString],
        input: Arc<[u8]>,
        cancel: &CancellationToken,
    ) -> Result<Output, GitWorkspaceError> {
        #[cfg(test)]
        let executable = self.executable.as_deref().unwrap_or_else(|| Path::new(GIT));
        #[cfg(not(test))]
        let executable = Path::new(GIT);
        self.run_trusted_mutation_with_executable(verb, args, input, cancel, executable)
    }

    pub(crate) fn run_trusted_mutation_with_executable(
        &self,
        verb: &'static str,
        args: &[OsString],
        input: Arc<[u8]>,
        cancel: &CancellationToken,
        executable: &Path,
    ) -> Result<Output, GitWorkspaceError> {
        self.run_trusted_mutation_with_executable_and_timeout(
            verb,
            args,
            input,
            cancel,
            executable,
            MUTATION_TIMEOUT,
        )
    }

    pub(crate) fn run_trusted_mutation_with_executable_and_timeout(
        &self,
        verb: &'static str,
        args: &[OsString],
        input: Arc<[u8]>,
        cancel: &CancellationToken,
        executable: &Path,
        timeout: Duration,
    ) -> Result<Output, GitWorkspaceError> {
        if !matches!(verb, "add" | "commit") {
            return Err(error(GitWorkspaceErrorCode::GitFailed));
        }
        self.verify_root()?;
        if cancel.is_cancelled() {
            return Err(error(GitWorkspaceErrorCode::Cancelled));
        }
        let mut command = Command::new(executable);
        command.current_dir(&self.root);
        command
            .args(PREFIX)
            .args(["-c", "core.hooksPath=/dev/null"])
            .arg(verb)
            .args(args);
        scrub_git_environment(&mut command);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        let mut child = command
            .spawn()
            .map_err(|_| error(GitWorkspaceErrorCode::SpawnFailed))?;
        collect_child(
            &mut child,
            Some(input),
            MUTATION_STDOUT_LIMIT,
            STDERR_LIMIT,
            timeout,
            cancel,
            OverflowPolicy::IMMEDIATE,
        )
    }

    pub(crate) fn run_commit_summary(
        &self,
        stdout_limit: usize,
        cancel: &CancellationToken,
    ) -> Result<Output, GitWorkspaceError> {
        self.run_commit_summary_with_timeout(stdout_limit, cancel, READ_TIMEOUT)
    }

    pub(crate) fn run_commit_summary_with_timeout(
        &self,
        stdout_limit: usize,
        cancel: &CancellationToken,
        timeout: Duration,
    ) -> Result<Output, GitWorkspaceError> {
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
        command
            .args(PREFIX)
            .args(["-c", "core.quotePath=true"])
            .arg("--no-optional-locks")
            .args([
                "diff",
                "--cached",
                "--patch",
                "--find-renames",
                "--no-ext-diff",
                "--no-textconv",
                "--full-index",
                "--",
            ]);
        scrub_git_environment(&mut command);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        let mut child = command
            .spawn()
            .map_err(|_| error(GitWorkspaceErrorCode::SpawnFailed))?;
        collect_child(
            &mut child,
            None,
            stdout_limit,
            STDERR_LIMIT,
            timeout,
            cancel,
            OverflowPolicy::DEFERRED,
        )
    }

    pub(crate) fn run_trusted_switch_with_executable(
        &self,
        branch: &OsStr,
        cancel: &CancellationToken,
        executable: &Path,
    ) -> Result<Output, GitWorkspaceError> {
        self.verify_root()?;
        if cancel.is_cancelled() {
            return Err(error(GitWorkspaceErrorCode::Cancelled));
        }
        let mut command = Command::new(executable);
        command.current_dir(&self.root);
        command
            .args(PREFIX)
            .args(["-c", "core.hooksPath=/dev/null", "switch"])
            .args([
                OsStr::new("--no-guess"),
                OsStr::new("--no-overwrite-ignore"),
                OsStr::new("--no-recurse-submodules"),
            ])
            .arg(branch);
        scrub_git_environment(&mut command);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        let mut child = command
            .spawn()
            .map_err(|_| error(GitWorkspaceErrorCode::SpawnFailed))?;
        collect_child(
            &mut child,
            None,
            MUTATION_STDOUT_LIMIT,
            STDERR_LIMIT,
            MUTATION_TIMEOUT,
            cancel,
            OverflowPolicy::IMMEDIATE,
        )
    }

    pub(crate) fn verify_root(&self) -> Result<(), GitWorkspaceError> {
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

pub(crate) fn scrub_git_environment(command: &mut Command) {
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

pub(crate) struct ReaderResult {
    pub(crate) stream: Stream,
    pub(crate) bytes: Vec<u8>,
    pub(crate) overflow: bool,
    pub(crate) failed: bool,
}

#[derive(Clone, Copy)]
pub(crate) enum Stream {
    Stdout,
    Stderr,
}

#[derive(Clone, Copy)]
pub(crate) struct OverflowPolicy {
    pub(crate) stdout_immediate: bool,
    pub(crate) stderr_immediate: bool,
}

impl OverflowPolicy {
    const IMMEDIATE: Self = Self {
        stdout_immediate: true,
        stderr_immediate: true,
    };
    const DEFERRED: Self = Self {
        stdout_immediate: false,
        stderr_immediate: false,
    };
}

pub(crate) fn collect_child(
    child: &mut Child,
    input: Option<Arc<[u8]>>,
    stdout_limit: usize,
    stderr_limit: usize,
    timeout: Duration,
    cancel: &CancellationToken,
    overflow_policy: OverflowPolicy,
) -> Result<Output, GitWorkspaceError> {
    let pgid = child.id();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdin = input.as_ref().and_then(|_| child.stdin.take());
    let (stdout, stderr, stdin) = match (stdout, stderr, stdin, input.is_some()) {
        (Some(stdout), Some(stderr), Some(stdin), true) => (stdout, stderr, Some(stdin)),
        (Some(stdout), Some(stderr), None, false) => (stdout, stderr, None),
        (stdout, stderr, stdin, _) => {
            cleanup_partial_child(child, pgid, stdout, stderr, stdin);
            return Err(error(GitWorkspaceErrorCode::ProcessControlFailed));
        }
    };
    let overflowed = Arc::new(AtomicBool::new(false));
    let writer_done = Arc::new(AtomicBool::new(input.is_none()));
    let writer_failed = Arc::new(AtomicBool::new(false));
    if let Some(input) = input {
        let Some(mut stdin) = stdin else {
            cleanup_partial_child(child, pgid, None, None, None);
            return Err(error(GitWorkspaceErrorCode::ProcessControlFailed));
        };
        let done = writer_done.clone();
        let failed = writer_failed.clone();
        thread::spawn(move || {
            for chunk in input.chunks(IO_CHUNK) {
                if stdin.write_all(chunk).is_err() {
                    failed.store(true, Ordering::SeqCst);
                    break;
                }
            }
            drop(stdin);
            done.store(true, Ordering::SeqCst);
        });
    }
    let (sender, receiver) = mpsc::channel();
    spawn_reader(
        stdout,
        Stream::Stdout,
        stdout_limit,
        overflowed.clone(),
        overflow_policy.stdout_immediate,
        sender.clone(),
    );
    spawn_reader(
        stderr,
        Stream::Stderr,
        stderr_limit,
        overflowed.clone(),
        overflow_policy.stderr_immediate,
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
        if writer_failed.load(Ordering::SeqCst) {
            stop_code = Some(GitWorkspaceErrorCode::GitFailed);
            break;
        }
        if started.elapsed() >= timeout {
            stop_code = Some(GitWorkspaceErrorCode::TimedOut);
            break;
        }
        match child.try_wait() {
            Ok(current) => status = current,
            Err(_) => {
                stop_code = Some(GitWorkspaceErrorCode::ProcessControlFailed);
                break;
            }
        }
        if status.is_none() {
            thread::sleep(Duration::from_millis(5));
        }
    }
    let mut cleanup_failed = false;
    if stop_code.is_some() && terminate_group(child, pgid).is_err() {
        cleanup_failed = true;
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
        stop_code.get_or_insert(GitWorkspaceErrorCode::ProcessControlFailed);
        if terminate_group(child, pgid).is_err() {
            cleanup_failed = true;
        }
        while outputs.len() < 2 {
            match receiver.recv_timeout(DRAIN_GRACE) {
                Ok(output) => outputs.push(output),
                Err(_) => {
                    cleanup_failed = true;
                    break;
                }
            }
        }
    }
    if status.is_none() {
        let deadline = Instant::now();
        while status.is_none() && deadline.elapsed() < DRAIN_GRACE {
            status = match child.try_wait() {
                Ok(status) => status,
                Err(_) => {
                    cleanup_failed = true;
                    let _ = terminate_group(child, pgid);
                    break;
                }
            };
            if status.is_none() {
                thread::sleep(Duration::from_millis(5));
            }
        }
        if status.is_none() {
            cleanup_failed = true;
            let _ = terminate_group(child, pgid);
        }
    }
    let writer_started = Instant::now();
    while !writer_done.load(Ordering::SeqCst) && writer_started.elapsed() < DRAIN_GRACE {
        thread::sleep(Duration::from_millis(5));
    }
    if !writer_done.load(Ordering::SeqCst) || writer_failed.load(Ordering::SeqCst) {
        stop_code.get_or_insert(GitWorkspaceErrorCode::GitFailed);
        if terminate_group(child, pgid).is_err() {
            cleanup_failed = true;
        }
    }
    if cleanup_failed {
        return Err(error(GitWorkspaceErrorCode::ProcessControlFailed));
    }
    if let Some(code) = stop_code {
        return Err(error(code));
    }
    if outputs.iter().any(|output| {
        output.overflow
            && (matches!(output.stream, Stream::Stderr) || overflow_policy.stdout_immediate)
    }) {
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
        .ok_or_else(|| error(GitWorkspaceErrorCode::GitFailed))?;
    Ok(Output {
        stdout: stdout.bytes,
        overflow: stdout.overflow,
    })
}

pub(crate) fn cleanup_partial_child(
    child: &mut Child,
    pgid: u32,
    stdout: Option<ChildStdout>,
    stderr: Option<ChildStderr>,
    stdin: Option<ChildStdin>,
) {
    drop(stdin);
    let overflowed = Arc::new(AtomicBool::new(false));
    let (sender, receiver) = mpsc::channel();
    let mut readers = 0;
    if let Some(stdout) = stdout {
        readers += 1;
        spawn_reader(
            stdout,
            Stream::Stdout,
            0,
            overflowed.clone(),
            true,
            sender.clone(),
        );
    }
    if let Some(stderr) = stderr {
        readers += 1;
        spawn_reader(stderr, Stream::Stderr, 0, overflowed, true, sender.clone());
    }
    drop(sender);
    let _ = terminate_group(child, pgid);
    for _ in 0..readers {
        if receiver.recv_timeout(DRAIN_GRACE).is_err() {
            break;
        }
    }
}

pub(crate) fn spawn_reader<R: Read + Send + 'static>(
    mut reader: R,
    stream: Stream,
    limit: usize,
    overflowed: Arc<AtomicBool>,
    overflow_is_fatal: bool,
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
                    let remaining = limit.saturating_sub(retained.len());
                    let retain = remaining.min(read);
                    retained.extend_from_slice(&chunk[..retain]);
                    if retain < read {
                        overflow = true;
                        if overflow_is_fatal {
                            overflowed.store(true, Ordering::SeqCst);
                        }
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

pub(crate) fn terminate_group(child: &mut Child, pgid: u32) -> Result<(), GitWorkspaceError> {
    let mut control_failed = !signal_group_checked(pgid, "-TERM");
    thread::sleep(TERM_GRACE);
    if !signal_group_checked(pgid, "-KILL") {
        control_failed = true;
        if child.kill().is_err() {
            control_failed = true;
        }
    }
    let mut reaped = bounded_reap(child, DRAIN_GRACE);
    if !reaped {
        control_failed = true;
        let _ = child.kill();
        reaped = bounded_reap(child, DRAIN_GRACE);
    }
    if control_failed || !reaped {
        Err(error(GitWorkspaceErrorCode::ProcessControlFailed))
    } else {
        Ok(())
    }
}

pub(crate) fn signal_group(pgid: u32, signal: &str) -> std::io::Result<ExitStatus> {
    Command::new(KILL)
        .args([signal, "--", &format!("-{pgid}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
}

pub(crate) fn signal_group_checked(pgid: u32, signal: &str) -> bool {
    match signal_group(pgid, signal) {
        Ok(status) if status.success() => true,
        Ok(_) => !group_exists(pgid),
        Err(_) => false,
    }
}

pub(crate) fn group_exists(pgid: u32) -> bool {
    Command::new(KILL)
        .args(["-0", "--", &format!("-{pgid}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

pub(crate) fn bounded_reap(child: &mut Child, timeout: Duration) -> bool {
    let started = Instant::now();
    while started.elapsed() < timeout {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) => thread::sleep(Duration::from_millis(5)),
            Err(_) => return false,
        }
    }
    false
}

pub(crate) fn classify_git_failure(
    status: ExitStatus,
    outputs: &[ReaderResult],
) -> GitWorkspaceError {
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

pub(crate) fn error(code: GitWorkspaceErrorCode) -> GitWorkspaceError {
    GitWorkspaceError::new(code)
}
