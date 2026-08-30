//! Strict, permission-handoff-friendly, sandboxed bash execution.

use std::fmt;
use std::path::PathBuf;
use std::process::ExitStatus;
use std::time::{Duration, Instant};

use serde::Deserialize;
use tokio::io::AsyncReadExt;
use tokio::process::{Child, ChildStdout, Command};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::Tools;
use crate::error::{BashError, BashErrorCode};
use crate::output::{BASH_READ_CHUNK_BYTES, BashOutput, BashOutputCollector, CollectedBashOutput};
use crate::sandbox::{ExecutionHooks, SandboxConfig, TempRoot};

/// Default bash timeout required by the Phase 1 tool contract.
pub const DEFAULT_BASH_TIMEOUT_MS: u64 = 120_000;
const TERMINATION_GRACE: Duration = Duration::from_millis(300);
const OUTPUT_DRAIN_GRACE: Duration = Duration::from_millis(500);
const REAP_GRACE: Duration = Duration::from_secs(2);
const TEMP_PATH_PLACEHOLDER: &str = "[VEGA_TEMP]";

/// Strict parsed bash input bound to one project tool instance.
pub struct PreparedBash {
    instance_id: u64,
    project_root: PathBuf,
    command: String,
    timeout_ms: u64,
}

impl PreparedBash {
    /// Exact provider command used as the Phase 1 permission signature.
    pub fn command(&self) -> &str {
        &self.command
    }

    /// Effective timeout in milliseconds.
    pub const fn timeout_ms(&self) -> u64 {
        self.timeout_ms
    }
}

impl fmt::Debug for PreparedBash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedBash")
            .field("command", &"[REDACTED]")
            .field("timeout_ms", &self.timeout_ms)
            .finish()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BashInput {
    cmd: String,
    #[serde(default)]
    timeout_ms: OptionalTimeout,
}

#[derive(Default)]
struct OptionalTimeout(Option<u64>);

impl<'de> Deserialize<'de> for OptionalTimeout {
    fn deserialize<Deserializer>(deserializer: Deserializer) -> Result<Self, Deserializer::Error>
    where
        Deserializer: serde::Deserializer<'de>,
    {
        u64::deserialize(deserializer).map(|value| Self(Some(value)))
    }
}

impl Tools {
    /// Strictly parse provider JSON without executing it. T26 must complete
    /// permission handling before passing the prepared value to execution.
    pub fn prepare_bash_json(&self, raw_input: &str) -> Result<PreparedBash, BashError> {
        let (command, timeout_ms) = parse_bash_input(raw_input)?;
        Ok(PreparedBash {
            instance_id: self.instance_id,
            project_root: self.root.clone(),
            command,
            timeout_ms,
        })
    }

    /// Execute an already permission-approved bash call through Seatbelt.
    pub async fn execute_bash(
        &self,
        prepared: PreparedBash,
        cancel: CancellationToken,
    ) -> Result<BashOutput, BashError> {
        self.execute_bash_inner(prepared, cancel, &ExecutionHooks::default())
            .await
    }

    async fn execute_bash_inner(
        &self,
        prepared: PreparedBash,
        cancel: CancellationToken,
        hooks: &ExecutionHooks,
    ) -> Result<BashOutput, BashError> {
        if self.instance_id != prepared.instance_id || self.root != prepared.project_root {
            return Err(BashError::new(BashErrorCode::ScopeMismatch));
        }
        if cancel.is_cancelled() {
            return Err(BashError::new(BashErrorCode::Cancelled));
        }

        let sandbox = SandboxConfig::new(&self.root)?;
        let temp_root = TempRoot::create()?;
        hooks.note_temp_created(temp_root.path());
        let result = self
            .execute_bash_with_temp(prepared, cancel, hooks, &sandbox, &temp_root)
            .await;
        if result.cleanup_safe {
            hooks.note_before_cleanup(temp_root.path());
            match temp_root.cleanup() {
                Ok(()) => result.result,
                Err(error) => Err(error),
            }
        } else {
            result.result
        }
    }

    async fn execute_bash_with_temp(
        &self,
        prepared: PreparedBash,
        cancel: CancellationToken,
        hooks: &ExecutionHooks,
        sandbox: &SandboxConfig,
        temp_root: &TempRoot,
    ) -> BashAttempt {
        if let Err(error) = sandbox.preflight(temp_root, hooks) {
            return BashAttempt::safe(Err(error));
        }
        let self_test = sandbox.self_test(temp_root, hooks).await;
        if let Err(error) = self_test.result {
            return BashAttempt {
                result: Err(error),
                cleanup_safe: self_test.cleanup_safe,
            };
        }
        if cancel.is_cancelled() {
            return BashAttempt::safe(Err(BashError::new(BashErrorCode::Cancelled)));
        }

        let started = Instant::now();
        let mut child = match sandbox.spawn_shell(&prepared.command, temp_root, hooks) {
            Ok(child) => child,
            Err(error) => return BashAttempt::safe(Err(error)),
        };
        let Some(pgid) = child.id() else {
            let reaped = terminate_child_without_group(&mut child).await;
            return BashAttempt {
                result: Err(BashError::new(BashErrorCode::ProcessControlFailed)),
                cleanup_safe: reaped,
            };
        };
        let Some(stdout) = child.stdout.take() else {
            let termination = terminate_group(&mut child, pgid, hooks).await;
            return BashAttempt {
                result: Err(termination
                    .error
                    .unwrap_or_else(|| BashError::new(BashErrorCode::OutputFailed))),
                cleanup_safe: termination.reaped,
            };
        };
        let reader_stop = CancellationToken::new();
        let mut reader = tokio::spawn(read_output(stdout, reader_stop.clone()));
        let timeout = tokio::time::sleep(Duration::from_millis(prepared.timeout_ms));
        tokio::pin!(timeout);

        enum Completion {
            Exited(Result<ExitStatus, std::io::Error>),
            Cancelled,
            TimedOut,
        }
        let completion = tokio::select! {
            status = child.wait() => Completion::Exited(status),
            _ = cancel.cancelled() => Completion::Cancelled,
            _ = &mut timeout => Completion::TimedOut,
        };

        match completion {
            Completion::Exited(status) => {
                let status = match status {
                    Ok(status) => status,
                    Err(_) => {
                        let termination = terminate_group(&mut child, pgid, hooks).await;
                        reader_stop.cancel();
                        let reader_ok = finish_stopped_reader(reader).await.is_ok();
                        return BashAttempt {
                            result: Err(BashError::new(BashErrorCode::ProcessControlFailed)),
                            cleanup_safe: termination.reaped && reader_ok,
                        };
                    }
                };
                match finish_reader(&mut reader, &reader_stop, Some(pgid)).await {
                    Ok(mut collected) => {
                        redact_temp_path(&mut collected.text, temp_root.param());
                        BashAttempt::safe(Ok(build_output(status, started, collected)))
                    }
                    Err(error) => BashAttempt {
                        result: Err(error),
                        cleanup_safe: false,
                    },
                }
            }
            Completion::Cancelled => {
                let termination = terminate_group(&mut child, pgid, hooks).await;
                reader_stop.cancel();
                let reader_result = finish_stopped_reader(reader).await;
                let reader_ok = reader_result.is_ok();
                BashAttempt {
                    result: termination
                        .error
                        .or_else(|| reader_result.err())
                        .map_or_else(|| Err(BashError::new(BashErrorCode::Cancelled)), Err),
                    cleanup_safe: termination.reaped && reader_ok,
                }
            }
            Completion::TimedOut => {
                let termination = terminate_group(&mut child, pgid, hooks).await;
                reader_stop.cancel();
                let reader_result = finish_stopped_reader(reader).await;
                let reader_ok = reader_result.is_ok();
                BashAttempt {
                    result: termination
                        .error
                        .or_else(|| reader_result.err())
                        .map_or_else(|| Err(BashError::new(BashErrorCode::TimedOut)), Err),
                    cleanup_safe: termination.reaped && reader_ok,
                }
            }
        }
    }

    #[cfg(test)]
    pub(crate) async fn execute_bash_with_hooks(
        &self,
        prepared: PreparedBash,
        cancel: CancellationToken,
        hooks: &ExecutionHooks,
    ) -> Result<BashOutput, BashError> {
        self.execute_bash_inner(prepared, cancel, hooks).await
    }
}

/// Strictly extracts the exact permission signature without creating an
/// execution capability. Conversation uses this to validate persisted rules.
pub fn bash_permission_signature(raw_input: &str) -> Result<String, BashError> {
    parse_bash_input(raw_input).map(|(command, _)| command)
}

fn parse_bash_input(raw_input: &str) -> Result<(String, u64), BashError> {
    let input: BashInput =
        serde_json::from_str(raw_input).map_err(|_| BashError::new(BashErrorCode::InvalidInput))?;
    let timeout_ms = input.timeout_ms.0.unwrap_or(DEFAULT_BASH_TIMEOUT_MS);
    if input.cmd.is_empty() || timeout_ms == 0 {
        return Err(BashError::new(BashErrorCode::InvalidInput));
    }
    Ok((input.cmd, timeout_ms))
}

struct BashAttempt {
    result: Result<BashOutput, BashError>,
    cleanup_safe: bool,
}

impl BashAttempt {
    fn safe(result: Result<BashOutput, BashError>) -> Self {
        Self {
            result,
            cleanup_safe: true,
        }
    }
}

async fn read_output(
    mut stdout: ChildStdout,
    stop: CancellationToken,
) -> Result<CollectedBashOutput, BashError> {
    let mut collector = BashOutputCollector::new();
    let mut chunk = [0_u8; BASH_READ_CHUNK_BYTES];
    loop {
        let read = tokio::select! {
            result = stdout.read(&mut chunk) => result,
            _ = stop.cancelled() => break,
        }
        .map_err(|_| BashError::new(BashErrorCode::OutputFailed))?;
        if read == 0 {
            break;
        }
        collector.push(&chunk[..read]);
    }
    Ok(collector.finish())
}

async fn finish_reader(
    reader: &mut JoinHandle<Result<CollectedBashOutput, BashError>>,
    stop: &CancellationToken,
    pgid: Option<u32>,
) -> Result<CollectedBashOutput, BashError> {
    match tokio::time::timeout(OUTPUT_DRAIN_GRACE, &mut *reader).await {
        Ok(result) => join_output(result),
        Err(_) => {
            let mut signal_error = None;
            if let Some(group) = pgid {
                signal_error = signal_group(group, "-TERM").await.err();
                tokio::time::sleep(TERMINATION_GRACE).await;
                if let Err(error) = signal_group(group, "-KILL").await
                    && signal_error.is_none()
                {
                    signal_error = Some(error);
                }
            }
            stop.cancel();
            let result = (&mut *reader).await;
            let collected = join_output(result)?;
            if let Some(error) = signal_error {
                Err(error)
            } else {
                Ok(collected)
            }
        }
    }
}

async fn finish_stopped_reader(
    reader: JoinHandle<Result<CollectedBashOutput, BashError>>,
) -> Result<(), BashError> {
    join_output(reader.await).map(|_| ())
}

fn join_output(
    result: Result<Result<CollectedBashOutput, BashError>, tokio::task::JoinError>,
) -> Result<CollectedBashOutput, BashError> {
    result.map_err(|_| BashError::new(BashErrorCode::OutputFailed))?
}

struct Termination {
    reaped: bool,
    error: Option<BashError>,
}

async fn terminate_group(child: &mut Child, pgid: u32, hooks: &ExecutionHooks) -> Termination {
    let mut error = signal_group(pgid, "-TERM").await.err();
    tokio::time::sleep(TERMINATION_GRACE).await;
    if let Err(kill_error) = signal_group(pgid, "-KILL").await
        && error.is_none()
    {
        error = Some(kill_error);
    }
    let reaped = matches!(
        tokio::time::timeout(REAP_GRACE, child.wait()).await,
        Ok(Ok(_))
    ) && !hooks.force_unconfirmed_reap();
    if !reaped && error.is_none() {
        error = Some(BashError::new(BashErrorCode::ProcessControlFailed));
    }
    Termination { reaped, error }
}

async fn terminate_child_without_group(child: &mut Child) -> bool {
    let _ = child.start_kill();
    matches!(
        tokio::time::timeout(REAP_GRACE, child.wait()).await,
        Ok(Ok(_))
    )
}

async fn signal_group(pgid: u32, signal: &str) -> Result<(), BashError> {
    let target = format!("-{pgid}");
    let status = Command::new("/bin/kill")
        .args([signal, "--", &target])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map_err(|_| BashError::new(BashErrorCode::ProcessControlFailed))?;
    if status.success() || !group_exists(&target).await? {
        Ok(())
    } else {
        Err(BashError::new(BashErrorCode::ProcessControlFailed))
    }
}

async fn group_exists(target: &str) -> Result<bool, BashError> {
    let status = Command::new("/bin/kill")
        .args(["-0", "--", target])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map_err(|_| BashError::new(BashErrorCode::ProcessControlFailed))?;
    Ok(status.success())
}

fn build_output(
    status: ExitStatus,
    started: Instant,
    collected: CollectedBashOutput,
) -> BashOutput {
    let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    BashOutput {
        text: collected.text,
        exit_code: status.code().unwrap_or(-1),
        duration_ms,
        truncated: collected.truncated,
        #[cfg(test)]
        high_water_bytes: collected.high_water_bytes,
    }
}

fn redact_temp_path(text: &mut String, temp_path: &str) {
    let mut cursor = 0_usize;
    while let Some(relative) = text[cursor..].find(temp_path) {
        let start = cursor.saturating_add(relative);
        let end = start.saturating_add(temp_path.len());
        text.replace_range(start..end, TEMP_PATH_PLACEHOLDER);
        cursor = start.saturating_add(TEMP_PATH_PLACEHOLDER.len());
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::os::unix::process::CommandExt as _;
    use std::process::{Command as StdCommand, Stdio};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use tempfile::{TempDir, tempdir};
    use tokio_util::sync::CancellationToken;

    use crate::Tools;
    use crate::error::BashErrorCode;
    use crate::output::{
        BASH_LINE_MIDDLE_MARKER, BASH_MAX_BYTES_PER_SIDE, BASH_MAX_LINE_BYTES,
        BASH_OUTPUT_MIDDLE_MARKER, BASH_READ_CHUNK_BYTES,
    };
    use crate::sandbox::{ExecutionHooks, ScanFailure, TestPathHook};

    use super::{TEMP_PATH_PLACEHOLDER, redact_temp_path, signal_group};

    fn tools() -> (TempDir, Tools) {
        let project = tempdir().unwrap();
        let tools = Tools::new(project.path()).unwrap();
        (project, tools)
    }

    fn quote(path: &std::path::Path) -> String {
        format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
    }

    async fn run(tools: &Tools, raw: &str) -> Result<crate::BashOutput, crate::BashError> {
        let prepared = tools.prepare_bash_json(raw).unwrap();
        tools.execute_bash(prepared, CancellationToken::new()).await
    }

    fn capture_temp() -> (Arc<Mutex<Option<std::path::PathBuf>>>, TestPathHook) {
        let captured = Arc::new(Mutex::new(None));
        let hook_capture = captured.clone();
        let hook = Arc::new(move |path: &std::path::Path| {
            assert_eq!(
                fs::symlink_metadata(path).unwrap().permissions().mode() & 0o777,
                0o700
            );
            *hook_capture.lock().unwrap() = Some(path.to_path_buf());
        });
        (captured, hook)
    }

    fn captured_path(captured: &Arc<Mutex<Option<std::path::PathBuf>>>) -> std::path::PathBuf {
        captured.lock().unwrap().clone().unwrap()
    }

    #[test]
    fn bash_input_is_strict_and_defaults_timeout() {
        let (_project, tools) = tools();
        let default = tools.prepare_bash_json(r#"{"cmd":"pwd"}"#).unwrap();
        assert_eq!(default.command(), "pwd");
        assert_eq!(default.timeout_ms(), 120_000);
        assert!(!format!("{default:?}").contains("pwd"));

        let custom = tools
            .prepare_bash_json(r#"{"cmd":"pwd","timeout_ms":321}"#)
            .unwrap();
        assert_eq!(custom.timeout_ms(), 321);

        for raw in [
            "{}",
            r#"{"cmd":1}"#,
            r#"{"cmd":"pwd","cwd":"/"}"#,
            r#"{"cmd":"pwd","timeout_ms":0}"#,
            r#"{"cmd":"pwd","timeout_ms":null}"#,
            r#"{"cmd":"pwd","timeout_ms":-1}"#,
            r#"{"cmd":"pwd","timeout_ms":1.5}"#,
            r#"{"cmd":"pwd","timeout_ms":18446744073709551616}"#,
            r#"{"cmd":"a","cmd":"b"}"#,
        ] {
            assert_eq!(
                tools.prepare_bash_json(raw).unwrap_err().code(),
                BashErrorCode::InvalidInput,
                "{raw}"
            );
        }
    }

    #[test]
    fn bash_temp_redaction_replaces_exact_paths_after_stream_assembly_in_place() {
        let path = "/private/tmp/.vega-bash-0123456789abcdef";
        let mut output = format!("prefix {path}/a\n{path}\nsuffix");
        let capacity = output.capacity();
        redact_temp_path(&mut output, path);
        assert_eq!(
            output,
            format!("prefix {TEMP_PATH_PLACEHOLDER}/a\n{TEMP_PATH_PLACEHOLDER}\nsuffix")
        );
        assert!(!output.contains(path));
        assert_eq!(output.capacity(), capacity);
    }

    #[tokio::test]
    async fn bash_cwd_project_write_and_stdout_stderr_merge() {
        let (project, tools) = tools();
        let output = run(
            &tools,
            r#"{"cmd":"print out; print err >&2; pwd; print created > created.txt","timeout_ms":5000}"#,
        )
        .await
        .unwrap();
        assert_eq!(output.exit_code, 0);
        assert!(!output.truncated);
        let lines: Vec<_> = output.text.lines().collect();
        assert_eq!(lines[0..2], ["out", "err"]);
        assert_eq!(lines[2], tools.root().to_string_lossy());
        assert_eq!(
            fs::read_to_string(project.path().join("created.txt")).unwrap(),
            "created\n"
        );
    }

    #[tokio::test]
    async fn sandbox_temp_is_private_exact_allowed_exported_and_cleaned() {
        let (_project, tools) = tools();
        let prepared = tools
            .prepare_bash_json(
                r#"{"cmd":"test \"$TMPDIR\" = \"$TMP\" && test \"$TMPDIR\" = \"$TEMP\" && test \"$TMPDIR\" = \"$TEMPDIR\" && print allowed > \"$TMPDIR/file\" && print -- \"$TMPDIR\" \"$TMP\" \"$TEMP\" \"$TEMPDIR\" && print temp-ok","timeout_ms":5000}"#,
            )
            .unwrap();
        let (captured, hook) = capture_temp();
        let hooks = ExecutionHooks {
            after_temp_created: Some(hook),
            ..ExecutionHooks::default()
        };
        let output = tools
            .execute_bash_with_hooks(prepared, CancellationToken::new(), &hooks)
            .await
            .unwrap();
        let path = captured_path(&captured);
        assert_eq!(
            output.text,
            "[VEGA_TEMP] [VEGA_TEMP] [VEGA_TEMP] [VEGA_TEMP]\ntemp-ok"
        );
        assert!(!output.text.contains("/private/tmp"));
        assert_eq!(path.parent().unwrap(), std::path::Path::new("/private/tmp"));
        assert!(
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".vega-bash-")
        );
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn sandbox_denies_shared_private_tmp_but_allows_call_temp() {
        let (_project, tools) = tools();
        let shared = tempfile::tempdir_in("/private/tmp").unwrap();
        let target = shared.path().join("must-not-change");
        let command = format!(
            "print escaped > {} 2>/dev/null; print ok > \"$TMPDIR/allowed\"; print done",
            quote(&target)
        );
        let raw = serde_json::json!({"cmd": command, "timeout_ms": 5000}).to_string();
        let output = run(&tools, &raw).await.unwrap();
        assert_eq!(output.exit_code, 0);
        assert!(output.text.ends_with("done"));
        assert!(!target.exists());
    }

    #[tokio::test]
    async fn sandbox_temp_hardlink_is_rejected_before_any_child_spawn_and_cleaned() {
        let (_project, tools) = tools();
        let external = tempfile::tempdir_in("/private/tmp").unwrap();
        let external_file = external.path().join("external");
        fs::write(&external_file, "unchanged").unwrap();
        let prepared = tools
            .prepare_bash_json(r#"{"cmd":"print escaped > marker"}"#)
            .unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        let (captured, capture_hook) = capture_temp();
        let external_for_hook = external_file.clone();
        let inject_hook: TestPathHook = Arc::new(move |path| {
            capture_hook(path);
            fs::hard_link(&external_for_hook, path.join("linked")).unwrap();
        });
        let hooks = ExecutionHooks {
            spawn_count: Some(count.clone()),
            after_temp_created: Some(inject_hook),
            ..ExecutionHooks::default()
        };
        let error = tools
            .execute_bash_with_hooks(prepared, CancellationToken::new(), &hooks)
            .await
            .unwrap_err();
        assert_eq!(error.code(), BashErrorCode::HardlinkPreflight);
        assert_eq!(count.load(Ordering::SeqCst), 0);
        assert_eq!(fs::read_to_string(external_file).unwrap(), "unchanged");
        assert!(!captured_path(&captured).exists());
    }

    #[tokio::test]
    async fn sandbox_nested_temp_symlink_cleanup_never_touches_target() {
        let (_project, tools) = tools();
        let external = tempfile::tempdir_in("/private/tmp").unwrap();
        let sentinel = external.path().join("sentinel");
        fs::write(&sentinel, "untouched").unwrap();
        let command = format!(
            "ln -s {} \"$TMPDIR/nested-link\"; print done",
            quote(external.path())
        );
        let raw = serde_json::json!({"cmd": command, "timeout_ms": 5000}).to_string();
        let output = run(&tools, &raw).await.unwrap();
        assert_eq!(output.exit_code, 0);
        assert_eq!(fs::read_to_string(sentinel).unwrap(), "untouched");
    }

    #[tokio::test]
    async fn sandbox_cleanup_rejects_replaced_root_without_touching_attacker() {
        let (_project, tools) = tools();
        let attacker = tempfile::tempdir_in("/private/tmp").unwrap();
        let moved_holder = tempfile::tempdir_in("/private/tmp").unwrap();
        let sentinel = attacker.path().join("sentinel");
        fs::write(&sentinel, "untouched").unwrap();
        let moved = moved_holder.path().join("moved");
        let attacker_path = attacker.path().to_path_buf();
        let replaced = Arc::new(Mutex::new(None));
        let replaced_for_hook = replaced.clone();
        let hook: TestPathHook = Arc::new(move |path| {
            fs::rename(path, &moved).unwrap();
            symlink(&attacker_path, path).unwrap();
            *replaced_for_hook.lock().unwrap() = Some(path.to_path_buf());
        });
        let prepared = tools.prepare_bash_json(r#"{"cmd":"print done"}"#).unwrap();
        let hooks = ExecutionHooks {
            before_cleanup: Some(hook),
            ..ExecutionHooks::default()
        };
        let error = tools
            .execute_bash_with_hooks(prepared, CancellationToken::new(), &hooks)
            .await
            .unwrap_err();
        assert_eq!(error.code(), BashErrorCode::CleanupFailed);
        assert_eq!(fs::read_to_string(&sentinel).unwrap(), "untouched");
        let replaced_path = replaced.lock().unwrap().clone().unwrap();
        assert!(
            fs::symlink_metadata(&replaced_path)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        fs::remove_file(replaced_path).unwrap();
    }

    #[tokio::test]
    async fn sandbox_blocks_outside_git_entry_and_actual_gitdir() {
        let project = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let actual_gitdir = tempdir().unwrap();
        fs::write(
            project.path().join(".git"),
            format!("gitdir: {}\n", actual_gitdir.path().display()),
        )
        .unwrap();
        let tools = Tools::new(project.path()).unwrap();
        let outside_target = outside.path().join("blocked");
        let git_target = actual_gitdir.path().join("blocked");
        let command = format!(
            "print blocked > {} 2>/dev/null; print blocked > {} 2>/dev/null; print blocked > .git 2>/dev/null; print done",
            quote(&outside_target),
            quote(&git_target)
        );
        let raw = serde_json::json!({"cmd": command, "timeout_ms": 5000}).to_string();
        let output = run(&tools, &raw).await.unwrap();
        assert_eq!(output.exit_code, 0);
        assert!(output.text.ends_with("done"));
        assert!(!outside_target.exists());
        assert!(!git_target.exists());
        assert!(project.path().join(".git").is_file());
    }

    #[tokio::test]
    async fn sandbox_skips_but_keeps_in_project_actual_gitdir_read_only() {
        let project = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let outside_file = outside.path().join("outside");
        fs::write(&outside_file, "unchanged").unwrap();
        fs::create_dir(project.path().join("control")).unwrap();
        fs::hard_link(&outside_file, project.path().join("control/linked")).unwrap();
        fs::write(project.path().join(".git"), "gitdir: control\n").unwrap();
        let tools = Tools::new(project.path()).unwrap();
        let output = run(
            &tools,
            r#"{"cmd":"print changed > control/linked 2>/dev/null; print ok > marker","timeout_ms":5000}"#,
        )
        .await
        .unwrap();
        assert_eq!(output.exit_code, 0);
        assert_eq!(fs::read_to_string(&outside_file).unwrap(), "unchanged");
        assert_eq!(
            fs::read_to_string(project.path().join("marker")).unwrap(),
            "ok\n"
        );
    }

    #[tokio::test]
    async fn sandbox_profile_self_test_failure_never_runs_shell() {
        let (project, tools) = tools();
        let prepared = tools
            .prepare_bash_json(r#"{"cmd":"print escaped > marker","timeout_ms":5000}"#)
            .unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        let hooks = ExecutionHooks {
            spawn_count: Some(count.clone()),
            profile_override: Some("(this is not a valid profile)".to_string()),
            ..ExecutionHooks::default()
        };
        let error = tools
            .execute_bash_with_hooks(prepared, CancellationToken::new(), &hooks)
            .await
            .unwrap_err();
        assert_eq!(error.code(), BashErrorCode::SandboxUnavailable);
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert!(!project.path().join("marker").exists());
    }

    #[tokio::test]
    async fn sandbox_hardlink_preflight_rejects_visible_hidden_and_ignored_before_spawn() {
        for relative in ["linked", ".hidden-linked", "ignored/linked"] {
            let project = tempdir().unwrap();
            let outside = tempdir().unwrap();
            let outside_file = outside.path().join("outside");
            fs::write(&outside_file, "unchanged").unwrap();
            let linked = project.path().join(relative);
            fs::create_dir_all(linked.parent().unwrap()).unwrap();
            fs::hard_link(&outside_file, &linked).unwrap();
            fs::write(project.path().join(".gitignore"), "ignored/\n").unwrap();
            let tools = Tools::new(project.path()).unwrap();
            let prepared = tools
                .prepare_bash_json(r#"{"cmd":"print escaped > marker"}"#)
                .unwrap();
            let count = Arc::new(AtomicUsize::new(0));
            let hooks = ExecutionHooks {
                spawn_count: Some(count.clone()),
                ..ExecutionHooks::default()
            };
            let error = tools
                .execute_bash_with_hooks(prepared, CancellationToken::new(), &hooks)
                .await
                .unwrap_err();
            assert_eq!(error.code(), BashErrorCode::HardlinkPreflight, "{relative}");
            assert_eq!(count.load(Ordering::SeqCst), 0, "{relative}");
            assert!(!project.path().join("marker").exists());
            assert_eq!(fs::read_to_string(&outside_file).unwrap(), "unchanged");
        }
    }

    #[tokio::test]
    async fn sandbox_scan_failures_are_zero_spawn_and_single_link_is_allowed() {
        for failure in [ScanFailure::Traversal, ScanFailure::Metadata] {
            let (project, tools) = tools();
            fs::write(project.path().join("ordinary"), "one-link").unwrap();
            let prepared = tools
                .prepare_bash_json(r#"{"cmd":"print escaped > marker"}"#)
                .unwrap();
            let count = Arc::new(AtomicUsize::new(0));
            let hooks = ExecutionHooks {
                spawn_count: Some(count.clone()),
                scan_failure: Some(failure),
                ..ExecutionHooks::default()
            };
            let error = tools
                .execute_bash_with_hooks(prepared, CancellationToken::new(), &hooks)
                .await
                .unwrap_err();
            assert_eq!(error.code(), BashErrorCode::HardlinkPreflight);
            assert_eq!(count.load(Ordering::SeqCst), 0);
            assert!(!project.path().join("marker").exists());
        }

        let (project, tools) = tools();
        fs::write(project.path().join("ordinary"), "one-link").unwrap();
        let ignored_git = tempdir().unwrap();
        let ignored_git_file = ignored_git.path().join("outside");
        fs::write(&ignored_git_file, "git-data").unwrap();
        fs::create_dir(project.path().join(".git")).unwrap();
        fs::hard_link(&ignored_git_file, project.path().join(".git/linked")).unwrap();
        let prepared = tools
            .prepare_bash_json(r#"{"cmd":"print ok > marker"}"#)
            .unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        let hooks = ExecutionHooks {
            spawn_count: Some(count.clone()),
            ..ExecutionHooks::default()
        };
        let output = tools
            .execute_bash_with_hooks(prepared, CancellationToken::new(), &hooks)
            .await
            .unwrap();
        assert_eq!(output.exit_code, 0);
        assert_eq!(count.load(Ordering::SeqCst), 2);
        assert_eq!(
            fs::read_to_string(project.path().join("marker")).unwrap(),
            "ok\n"
        );
    }

    #[tokio::test]
    async fn sandbox_temp_is_cleaned_after_nonzero_and_pre_spawn_rejection() {
        let (_project, tools) = tools();
        let prepared = tools
            .prepare_bash_json(r#"{"cmd":"print data > \"$TMPDIR/file\"; exit 7"}"#)
            .unwrap();
        let (nonzero_temp, hook) = capture_temp();
        let hooks = ExecutionHooks {
            after_temp_created: Some(hook),
            ..ExecutionHooks::default()
        };
        let output = tools
            .execute_bash_with_hooks(prepared, CancellationToken::new(), &hooks)
            .await
            .unwrap();
        assert_eq!(output.exit_code, 7);
        assert!(!captured_path(&nonzero_temp).exists());

        let prepared = tools
            .prepare_bash_json(r#"{"cmd":"print escaped"}"#)
            .unwrap();
        let (rejected_temp, hook) = capture_temp();
        let hooks = ExecutionHooks {
            scan_failure: Some(ScanFailure::Traversal),
            after_temp_created: Some(hook),
            ..ExecutionHooks::default()
        };
        let error = tools
            .execute_bash_with_hooks(prepared, CancellationToken::new(), &hooks)
            .await
            .unwrap_err();
        assert_eq!(error.code(), BashErrorCode::HardlinkPreflight);
        assert!(!captured_path(&rejected_temp).exists());
    }

    #[tokio::test]
    async fn bash_output_keeps_4001_line_head_tail_with_byte_caps() {
        let (_project, tools) = tools();
        let output = run(
            &tools,
            r#"{"cmd":"i=1; while (( i <= 4001 )); do print line-$i; (( i++ )); done","timeout_ms":10000}"#,
        )
        .await
        .unwrap();
        assert!(output.truncated);
        assert!(output.text.starts_with("line-1\n"));
        assert!(output.text.contains(BASH_OUTPUT_MIDDLE_MARKER));
        assert!(output.text.ends_with("line-4001"));
        assert!(output.text.len() <= 2 * BASH_MAX_BYTES_PER_SIDE);
        assert!(
            output.high_water_bytes
                <= 2 * BASH_MAX_BYTES_PER_SIDE + BASH_MAX_LINE_BYTES + BASH_READ_CHUNK_BYTES + 8
        );
    }

    #[tokio::test]
    async fn bash_multimegabyte_no_newline_has_line_marker_and_bounded_high_water() {
        let (_project, tools) = tools();
        let output = run(
            &tools,
            r#"{"cmd":"/usr/bin/yes x | /usr/bin/head -c 5242880 | /usr/bin/tr -d '\\n'","timeout_ms":10000}"#,
        )
        .await
        .unwrap();
        assert_eq!(output.exit_code, 0);
        assert!(output.truncated);
        assert!(output.text.contains(BASH_LINE_MIDDLE_MARKER));
        assert!(output.text.len() <= BASH_MAX_LINE_BYTES);
        assert!(
            output.high_water_bytes
                <= 2 * BASH_MAX_BYTES_PER_SIDE + BASH_MAX_LINE_BYTES + BASH_READ_CHUNK_BYTES + 8
        );
    }

    async fn wait_for_pid(path: &std::path::Path) -> u32 {
        for _ in 0..100 {
            if let Ok(value) = fs::read_to_string(path)
                && let Ok(pid) = value.trim().parse()
            {
                return pid;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("pid file was not created: {}", path.display());
    }

    fn process_is_gone(pid: u32) -> bool {
        !StdCommand::new("/bin/kill")
            .args(["-0", &pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success()
    }

    #[tokio::test]
    async fn bash_parent_exit_with_inherited_stdout_reaps_group_before_cleanup() {
        let (project, tools) = tools();
        let prepared = tools
            .prepare_bash_json(
                r#"{"cmd":"/bin/sh -c 'sleep 30 & echo $! > descendant.pid'","timeout_ms":5000}"#,
            )
            .unwrap();
        let (captured, hook) = capture_temp();
        let hooks = ExecutionHooks {
            after_temp_created: Some(hook),
            ..ExecutionHooks::default()
        };
        let output = tools
            .execute_bash_with_hooks(prepared, CancellationToken::new(), &hooks)
            .await
            .unwrap();
        let descendant = wait_for_pid(&project.path().join("descendant.pid")).await;
        assert_eq!(output.exit_code, 0);
        assert!(process_is_gone(descendant));
        assert!(!captured_path(&captured).exists());
    }

    #[tokio::test]
    async fn bash_cancel_reaps_shell_and_inherited_process_group_descendant() {
        let (project, tools) = tools();
        let prepared = tools
            .prepare_bash_json(
                r#"{"cmd":"print $$ > shell.pid; sleep 30 & print $! > child.pid; wait","timeout_ms":10000}"#,
            )
            .unwrap();
        let cancel = CancellationToken::new();
        let task_tools = tools.clone();
        let task_cancel = cancel.clone();
        let (captured, hook) = capture_temp();
        let hooks = ExecutionHooks {
            after_temp_created: Some(hook),
            ..ExecutionHooks::default()
        };
        let task = tokio::spawn(async move {
            task_tools
                .execute_bash_with_hooks(prepared, task_cancel, &hooks)
                .await
        });
        let shell_pid = wait_for_pid(&project.path().join("shell.pid")).await;
        let child_pid = wait_for_pid(&project.path().join("child.pid")).await;
        cancel.cancel();
        let error = task.await.unwrap().unwrap_err();
        assert_eq!(error.code(), BashErrorCode::Cancelled);
        assert!(process_is_gone(shell_pid));
        assert!(process_is_gone(child_pid));
        assert!(!captured_path(&captured).exists());
    }

    #[tokio::test]
    async fn bash_custom_timeout_reaps_shell_and_descendant() {
        let (project, tools) = tools();
        let prepared = tools
            .prepare_bash_json(
                r#"{"cmd":"print $$ > timeout-shell.pid; sleep 30 & print $! > timeout-child.pid; wait","timeout_ms":300}"#,
            )
            .unwrap();
        let (captured, hook) = capture_temp();
        let hooks = ExecutionHooks {
            after_temp_created: Some(hook),
            ..ExecutionHooks::default()
        };
        let error = tools
            .execute_bash_with_hooks(prepared, CancellationToken::new(), &hooks)
            .await
            .unwrap_err();
        assert_eq!(error.code(), BashErrorCode::TimedOut);
        let shell_pid = wait_for_pid(&project.path().join("timeout-shell.pid")).await;
        let child_pid = wait_for_pid(&project.path().join("timeout-child.pid")).await;
        assert!(process_is_gone(shell_pid));
        assert!(process_is_gone(child_pid));
        assert!(!captured_path(&captured).exists());
    }

    #[tokio::test]
    async fn bash_unconfirmed_reap_retains_temp_for_safe_gc() {
        let (_project, tools) = tools();
        let prepared = tools
            .prepare_bash_json(r#"{"cmd":"sleep 30","timeout_ms":100}"#)
            .unwrap();
        let (captured, hook) = capture_temp();
        let hooks = ExecutionHooks {
            after_temp_created: Some(hook),
            force_unconfirmed_reap: true,
            ..ExecutionHooks::default()
        };
        let error = tools
            .execute_bash_with_hooks(prepared, CancellationToken::new(), &hooks)
            .await
            .unwrap_err();
        assert_eq!(error.code(), BashErrorCode::ProcessControlFailed);
        let retained = captured_path(&captured);
        assert!(retained.is_dir());
        fs::remove_dir_all(retained).unwrap();
    }

    #[tokio::test]
    async fn signal_group_treats_an_already_exited_child_as_gone() {
        let mut command = StdCommand::new("/usr/bin/true");
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0);
        let mut child = tokio::process::Command::from(command).spawn().unwrap();
        let pgid = child.id().unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        signal_group(pgid, "-TERM").await.unwrap();
        child.wait().await.unwrap();
    }
}
