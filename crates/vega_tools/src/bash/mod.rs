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
mod tests;
