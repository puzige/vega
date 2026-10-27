//! Strict, permission-handoff-friendly bash execution with a sandboxed default.

use std::fmt;
use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::{Command as StdCommand, ExitStatus};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::Tools;
use crate::error::{BashError, BashErrorCode};
use crate::output::{BASH_READ_CHUNK_BYTES, BashOutput, BashOutputCollector, CollectedBashOutput};
use crate::sandbox::{SandboxConfig, TempRoot, full_access_shell_command};

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BashLaunchPolicy {
    WorkspaceWrite,
    FullAccess,
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BashInputError {
    MalformedJson,
    InputNotObject,
    MissingCommand,
    WrongCommandType,
    EmptyCommand,
    UnexpectedField,
    ConflictingFields,
    InvalidTimeout,
}

impl BashInputError {
    pub const ALL: [Self; 8] = [
        Self::MalformedJson,
        Self::InputNotObject,
        Self::MissingCommand,
        Self::WrongCommandType,
        Self::EmptyCommand,
        Self::UnexpectedField,
        Self::ConflictingFields,
        Self::InvalidTimeout,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MalformedJson => "malformed_json",
            Self::InputNotObject => "input_not_object",
            Self::MissingCommand => "missing_command",
            Self::WrongCommandType => "wrong_command_type",
            Self::EmptyCommand => "empty_command",
            Self::UnexpectedField => "unexpected_field",
            Self::ConflictingFields => "conflicting_fields",
            Self::InvalidTimeout => "invalid_timeout",
        }
    }

    pub fn feedback(self) -> String {
        format!(
            "Tool error: invalid bash input ({}). Expected bash({{\"command\":\"pwd\",\"timeout\":null}}). command must be non-empty; timeout is a positive integer in seconds or null. Do not mix legacy cmd/timeout_ms with command/timeout.",
            self.as_str()
        )
    }
}

impl Tools {
    pub fn prepare_bash_json(&self, raw_input: &str) -> Result<PreparedBash, BashError> {
        let (command, timeout_ms) =
            parse_bash_input(raw_input).map_err(|_| BashError::new(BashErrorCode::InvalidInput))?;
        Ok(PreparedBash {
            instance_id: self.instance_id,
            project_root: self.root.clone(),
            command,
            timeout_ms,
        })
    }

    fn validate_bash_launch(
        &self,
        prepared: &PreparedBash,
        cancel: &CancellationToken,
    ) -> Result<(), BashError> {
        if self.instance_id != prepared.instance_id || self.root != prepared.project_root {
            return Err(BashError::new(BashErrorCode::ScopeMismatch));
        }
        if cancel.is_cancelled() {
            return Err(BashError::new(BashErrorCode::Cancelled));
        }

        Ok(())
    }

    pub async fn execute_bash(
        &self,
        prepared: PreparedBash,
        policy: BashLaunchPolicy,
        cancel: CancellationToken,
    ) -> Result<BashOutput, BashError> {
        self.validate_bash_launch(&prepared, &cancel)?;
        let temp_root = TempRoot::create()?;
        let result = self
            .execute_bash_with_temp(prepared, policy, cancel, &temp_root)
            .await;
        if result.cleanup_safe {
            match temp_root.cleanup() {
                Ok(()) => result.result,
                Err(error) => Err(error),
            }
        } else {
            result.result
        }
    }

    async fn prepare_bash_launch(
        &self,
        prepared: &PreparedBash,
        policy: BashLaunchPolicy,
        cancel: &CancellationToken,
        temp_root: &TempRoot,
    ) -> Result<BashLaunch, BashLaunchFailure> {
        self.validate_bash_launch(prepared, cancel)?;
        if policy == BashLaunchPolicy::FullAccess {
            return Ok(BashLaunch {
                command: full_access_shell_command(&self.root, &prepared.command, temp_root)?,
                credential_lease: None,
            });
        }
        let sandbox = SandboxConfig::new(&self.root)?;
        sandbox.preflight(temp_root)?;
        let self_test = sandbox.self_test(temp_root).await;
        if let Err(error) = self_test.result {
            return Err(BashLaunchFailure {
                error,
                cleanup_safe: self_test.cleanup_safe,
            });
        }
        self.validate_bash_launch(prepared, cancel)?;
        let credential_lease = match &self.credential_policy {
            Some(policy) => {
                policy
                    .start(&temp_root.path().join("credential-ca.pem"), cancel.clone())
                    .await?
            }
            None => None,
        };
        let credential_context = self.credential_policy.as_ref().map(|policy| {
            let mut environment = crate::credentials::controlled_environment(temp_root.path());
            if let Some((_, values)) = &credential_lease {
                environment.extend(values.clone());
            }
            crate::sandbox::ShellCredentialContext {
                protected_root: policy.protected_root.clone(),
                proxy_port: credential_lease.as_ref().map(|(lease, _)| lease.port),
                environment,
            }
        });
        self.validate_bash_launch(prepared, cancel)?;
        Ok(BashLaunch {
            command: sandbox.shell_command(
                &prepared.command,
                temp_root,
                credential_context.as_ref(),
            )?,
            credential_lease: credential_lease.map(|(lease, _)| lease),
        })
    }

    async fn execute_bash_with_temp(
        &self,
        prepared: PreparedBash,
        policy: BashLaunchPolicy,
        cancel: CancellationToken,
        temp_root: &TempRoot,
    ) -> BashAttempt {
        let launch = match self
            .prepare_bash_launch(&prepared, policy, &cancel, temp_root)
            .await
        {
            Ok(launch) => launch,
            Err(failure) => {
                return BashAttempt {
                    result: Err(failure.error),
                    cleanup_safe: failure.cleanup_safe,
                };
            }
        };
        if cancel.is_cancelled() {
            return BashAttempt::safe(Err(BashError::new(BashErrorCode::Cancelled)));
        }
        let _credential_lease = launch.credential_lease;
        let started = Instant::now();
        let mut command = Command::from(launch.command);
        command.kill_on_drop(true);
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(_) => return BashAttempt::safe(Err(BashError::new(BashErrorCode::SpawnFailed))),
        };
        if child.id().is_none() {
            let _ = terminate_child_without_group(&mut child).await;
            return BashAttempt {
                result: Err(BashError::new(BashErrorCode::ProcessControlFailed)),
                cleanup_safe: false,
            };
        }
        let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
            let termination = terminate_group(&mut child).await;
            return BashAttempt {
                result: Err(termination
                    .error
                    .unwrap_or_else(|| BashError::new(BashErrorCode::OutputFailed))),
                cleanup_safe: termination.reaped && termination.error.is_none(),
            };
        };
        let reader_stop = CancellationToken::new();
        let mut reader = tokio::spawn(read_output(stdout, stderr, reader_stop.clone()));
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
                        let _ = terminate_child_without_group(&mut child).await;
                        reader_stop.cancel();
                        let _ = finish_stopped_reader(reader).await;
                        return BashAttempt {
                            result: Err(BashError::new(BashErrorCode::ProcessControlFailed)),
                            cleanup_safe: false,
                        };
                    }
                };
                match finish_reader(&mut reader, &reader_stop).await {
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
                let termination = terminate_group(&mut child).await;
                reader_stop.cancel();
                let reader_result = finish_stopped_reader(reader).await;
                let reader_ok = reader_result.is_ok();
                BashAttempt {
                    result: termination
                        .error
                        .or_else(|| reader_result.err())
                        .map_or_else(|| Err(BashError::new(BashErrorCode::Cancelled)), Err),
                    cleanup_safe: termination.reaped && termination.error.is_none() && reader_ok,
                }
            }
            Completion::TimedOut => {
                let termination = terminate_group(&mut child).await;
                reader_stop.cancel();
                let reader_result = finish_stopped_reader(reader).await;
                let reader_ok = reader_result.is_ok();
                BashAttempt {
                    result: termination
                        .error
                        .or_else(|| reader_result.err())
                        .map_or_else(|| Err(BashError::new(BashErrorCode::TimedOut)), Err),
                    cleanup_safe: termination.reaped && termination.error.is_none() && reader_ok,
                }
            }
        }
    }
}

/// Strictly extracts the exact permission signature without creating an
/// execution capability. Conversation uses this to validate persisted rules.
pub fn bash_permission_signature(raw_input: &str) -> Result<String, BashError> {
    parse_bash_input(raw_input)
        .map(|(command, _)| command)
        .map_err(|_| BashError::new(BashErrorCode::InvalidInput))
}

pub fn legacy_bash_input_is_invalid(raw_input: &str) -> bool {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct LegacyInput {
        cmd: String,
        #[serde(default)]
        timeout_ms: Option<u64>,
    }
    serde_json::from_str::<LegacyInput>(raw_input).map_or(true, |input| {
        input.cmd.is_empty() || input.timeout_ms == Some(0)
    })
}

pub fn bash_input_error(raw_input: &str) -> Option<BashInputError> {
    parse_bash_input(raw_input).err()
}

pub fn bash_inputs_semantically_equal(left: &str, right: &str) -> bool {
    match (parse_bash_input(left), parse_bash_input(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn parse_bash_input(raw_input: &str) -> Result<(String, u64), BashInputError> {
    let value =
        crate::json::parse_unique_value(raw_input).map_err(|_| BashInputError::MalformedJson)?;
    let input = value.as_object().ok_or(BashInputError::InputNotObject)?;
    if (input.contains_key("command")
        && (input.contains_key("cmd") || input.contains_key("timeout_ms")))
        || (input.contains_key("cmd") && input.contains_key("timeout"))
        || (input.contains_key("timeout") && input.contains_key("timeout_ms"))
    {
        return Err(BashInputError::ConflictingFields);
    }
    if input
        .keys()
        .any(|key| !matches!(key.as_str(), "command" | "cmd" | "timeout" | "timeout_ms"))
    {
        return Err(BashInputError::UnexpectedField);
    }
    let command = input
        .get("command")
        .or_else(|| input.get("cmd"))
        .ok_or(BashInputError::MissingCommand)?
        .as_str()
        .ok_or(BashInputError::WrongCommandType)?;
    if command.trim().is_empty() {
        return Err(BashInputError::EmptyCommand);
    }
    let timeout_ms = match input.get("timeout").or_else(|| input.get("timeout_ms")) {
        None | Some(serde_json::Value::Null) => DEFAULT_BASH_TIMEOUT_MS,
        Some(value) => {
            let value = value
                .as_u64()
                .filter(|value| *value > 0)
                .ok_or(BashInputError::InvalidTimeout)?;
            if input.contains_key("timeout") {
                value
                    .checked_mul(1000)
                    .ok_or(BashInputError::InvalidTimeout)?
            } else {
                value
            }
        }
    };
    Ok((command.to_owned(), timeout_ms))
}

struct BashLaunch {
    command: StdCommand,
    credential_lease: Option<vega_credentials::ProxyLease>,
}

#[derive(Debug)]
struct BashLaunchFailure {
    error: BashError,
    cleanup_safe: bool,
}

impl From<BashError> for BashLaunchFailure {
    fn from(error: BashError) -> Self {
        Self {
            error,
            cleanup_safe: true,
        }
    }
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
    mut stdout: impl AsyncRead + Unpin,
    mut stderr: impl AsyncRead + Unpin,
    stop: CancellationToken,
) -> Result<CollectedBashOutput, BashError> {
    let mut collector = BashOutputCollector::new();
    let mut stdout_chunk = [0_u8; BASH_READ_CHUNK_BYTES];
    let mut stderr_chunk = [0_u8; BASH_READ_CHUNK_BYTES];
    let mut stdout_open = true;
    let mut stderr_open = true;
    while stdout_open || stderr_open {
        if stop.is_cancelled() {
            break;
        }
        tokio::select! {
            _ = stop.cancelled() => break,
            result = stdout.read(&mut stdout_chunk), if stdout_open => {
                let read = result.map_err(|_| BashError::new(BashErrorCode::OutputFailed))?;
                stdout_open = read != 0;
                collector.push(&stdout_chunk[..read]);
            }
            result = stderr.read(&mut stderr_chunk), if stderr_open => {
                let read = result.map_err(|_| BashError::new(BashErrorCode::OutputFailed))?;
                stderr_open = read != 0;
                collector.push(&stderr_chunk[..read]);
            }
        }
    }
    Ok(collector.finish())
}

async fn finish_reader(
    reader: &mut JoinHandle<Result<CollectedBashOutput, BashError>>,
    stop: &CancellationToken,
) -> Result<CollectedBashOutput, BashError> {
    match tokio::time::timeout(OUTPUT_DRAIN_GRACE, &mut *reader).await {
        Ok(result) => join_output(result),
        Err(_) => {
            stop.cancel();
            let _ = join_output((&mut *reader).await)?;
            Err(BashError::new(BashErrorCode::OutputIncomplete))
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

async fn terminate_group(child: &mut Child) -> Termination {
    let Some(pgid) = child.id() else {
        return Termination {
            reaped: false,
            error: Some(BashError::new(BashErrorCode::ProcessControlFailed)),
        };
    };
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
    );
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
    mut collected: CollectedBashOutput,
) -> BashOutput {
    if let Some(signal) = status.signal() {
        if !collected.text.is_empty() && !collected.text.ends_with('\n') {
            collected.text.push('\n');
        }
        collected
            .text
            .push_str(&format!("Process terminated by signal {signal}\n"));
    }
    let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    BashOutput {
        text: collected.text,
        exit_code: status.code().unwrap_or(-1),
        duration_ms,
        truncated: collected.truncated,
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
