//! Strict, permission-handoff-friendly bash execution with a sandboxed default.

use std::fmt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
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
use crate::sandbox::{SandboxConfig, SandboxNetwork, TempRoot, full_access_shell_command};

mod scope;
pub use scope::{BashScopeDirectory, BashScopeRequest, PreparedBashScope};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BashLaunchPolicy {
    ReadOnly,
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
    requested_scope: Option<BashScopeRequest>,
    description: Option<String>,
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

    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    pub const fn tool_instance_id(&self) -> u64 {
        self.instance_id
    }

    pub fn requested_scope(&self) -> Option<&BashScopeRequest> {
        self.requested_scope.as_ref()
    }

    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    pub fn input_json(&self) -> String {
        let mut input = if self.timeout_ms.is_multiple_of(1000) {
            serde_json::json!({"command": self.command, "timeout": self.timeout_ms / 1000})
        } else {
            serde_json::json!({"cmd": self.command, "timeout_ms": self.timeout_ms})
        };
        if let Some(scope) = &self.requested_scope {
            input["requested_scope"] = serde_json::json!(scope);
        }
        input.to_string()
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
    InvalidDescription,
    EmptyCommand,
    UnexpectedField,
    ConflictingFields,
    InvalidTimeout,
    InvalidScope,
}

impl fmt::Display for BashInputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl BashInputError {
    pub const ALL: [Self; 10] = [
        Self::MalformedJson,
        Self::InputNotObject,
        Self::MissingCommand,
        Self::WrongCommandType,
        Self::InvalidDescription,
        Self::EmptyCommand,
        Self::UnexpectedField,
        Self::ConflictingFields,
        Self::InvalidTimeout,
        Self::InvalidScope,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MalformedJson => "malformed_json",
            Self::InputNotObject => "input_not_object",
            Self::MissingCommand => "missing_command",
            Self::WrongCommandType => "wrong_command_type",
            Self::InvalidDescription => "invalid_description",
            Self::EmptyCommand => "empty_command",
            Self::UnexpectedField => "unexpected_field",
            Self::ConflictingFields => "conflicting_fields",
            Self::InvalidTimeout => "invalid_timeout",
            Self::InvalidScope => "invalid_scope",
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
        let input =
            parse_bash_input(raw_input).map_err(|_| BashError::new(BashErrorCode::InvalidInput))?;
        Ok(PreparedBash {
            instance_id: self.instance_id,
            project_root: self.root.clone(),
            command: input.command,
            timeout_ms: input.timeout_ms,
            requested_scope: input.requested_scope,
            description: input.description,
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
        self.execute_bash_scoped(prepared, policy, None, cancel)
            .await
    }

    pub async fn execute_bash_scoped(
        &self,
        prepared: PreparedBash,
        policy: BashLaunchPolicy,
        scope: Option<PreparedBashScope>,
        cancel: CancellationToken,
    ) -> Result<BashOutput, BashError> {
        self.validate_bash_launch(&prepared, &cancel)?;
        if let Some(scope) = &scope {
            scope.validate(&prepared, policy)?;
        } else if prepared.requested_scope.is_some() && policy != BashLaunchPolicy::FullAccess {
            return Err(BashError::new(BashErrorCode::ScopeMismatch));
        }
        let temp_root = TempRoot::create()?;
        let result = self
            .execute_bash_with_temp(prepared, policy, scope, cancel, &temp_root)
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
        self.prepare_scoped_bash_launch(prepared, policy, None, cancel, temp_root)
            .await
    }

    async fn prepare_scoped_bash_launch(
        &self,
        prepared: &PreparedBash,
        policy: BashLaunchPolicy,
        scope: Option<&PreparedBashScope>,
        cancel: &CancellationToken,
        temp_root: &TempRoot,
    ) -> Result<BashLaunch, BashLaunchFailure> {
        let mut lease = None;
        match self
            .build_scoped_bash_launch(prepared, policy, scope, cancel, temp_root, &mut lease)
            .await
        {
            Ok(command) => Ok(BashLaunch {
                command,
                credential_lease: lease,
            }),
            Err(mut failure) => {
                if let Some(lease) = lease
                    && lease.shutdown().await.is_err()
                {
                    failure.error = BashError::new(BashErrorCode::CleanupFailed);
                    failure.cleanup_safe = false;
                }
                Err(failure)
            }
        }
    }

    async fn build_scoped_bash_launch(
        &self,
        prepared: &PreparedBash,
        policy: BashLaunchPolicy,
        scope: Option<&PreparedBashScope>,
        cancel: &CancellationToken,
        temp_root: &TempRoot,
        lease: &mut Option<vega_credentials::ProxyLease>,
    ) -> Result<StdCommand, BashLaunchFailure> {
        self.validate_bash_launch(prepared, cancel)?;
        if policy == BashLaunchPolicy::FullAccess {
            return full_access_shell_command(&self.root, &prepared.command, temp_root)
                .map_err(Into::into);
        }
        if let Some(credentials) = &self.credential_policy {
            credentials.validate_bindings()?;
        }
        let sandbox = if let Some(scope) = scope {
            scope.validate(prepared, policy)?;
            if self.bash_binding_snapshot()? != scope.bindings {
                return Err(BashError::new(BashErrorCode::ScopeMismatch).into());
            }
            let mut environment = std::collections::BTreeMap::new();
            let network = if !scope.network() {
                SandboxNetwork::Denied
            } else if scope.bindings.is_empty() {
                SandboxNetwork::Direct
            } else {
                let credentials = self
                    .credential_policy
                    .as_ref()
                    .ok_or_else(|| BashError::new(BashErrorCode::ScopeMismatch))?;
                let broker = vega_credentials::CredentialBroker::new(
                    &scope.bindings,
                    credentials.reader.clone(),
                )
                .map_err(|_| BashError::new(BashErrorCode::SandboxUnavailable))?;
                self.validate_bash_launch(prepared, cancel)?;
                *lease = Some(
                    vega_credentials::ProxyLease::start(broker, cancel.clone())
                        .await
                        .map_err(|_| BashError::new(BashErrorCode::SandboxUnavailable))?,
                );
                let proxy = lease
                    .as_ref()
                    .ok_or_else(|| BashError::new(BashErrorCode::SandboxUnavailable))?;
                temp_root.validate_identity()?;
                let ca_path = temp_root.path().join("ca.pem");
                std::fs::write(&ca_path, &proxy.ca_pem)
                    .map_err(|_| BashError::new(BashErrorCode::TempUnavailable))?;
                environment = proxy
                    .environment(
                        ca_path
                            .to_str()
                            .ok_or_else(|| BashError::new(BashErrorCode::TempUnavailable))?,
                    )
                    .map_err(|_| BashError::new(BashErrorCode::SandboxUnavailable))?;
                SandboxNetwork::Broker(proxy.port)
            };
            SandboxConfig::scoped(
                &self.root,
                policy,
                temp_root,
                self.credential_policy
                    .as_ref()
                    .map(|policy| policy.protected_root.as_path()),
                network,
                scope.directories.clone(),
                environment,
            )?
        } else {
            if prepared.requested_scope.is_some() {
                return Err(BashError::new(BashErrorCode::ScopeMismatch).into());
            }
            SandboxConfig::new(
                &self.root,
                policy,
                temp_root,
                self.credential_policy
                    .as_ref()
                    .map(|policy| policy.protected_root.as_path()),
            )?
        };
        self.validate_bash_launch(prepared, cancel)?;
        let self_test = sandbox
            .self_test(
                temp_root,
                cancel,
                Duration::from_millis(prepared.timeout_ms),
            )
            .await;
        if let Err(error) = self_test.result {
            return Err(BashLaunchFailure {
                error,
                cleanup_safe: self_test.cleanup_safe,
            });
        }
        self.validate_bash_launch(prepared, cancel)?;
        if let Some(scope) = scope {
            scope.validate(prepared, policy)?;
            if self.bash_binding_snapshot()? != scope.bindings {
                return Err(BashError::new(BashErrorCode::ScopeMismatch).into());
            }
        }
        sandbox
            .shell_command(&prepared.command, temp_root)
            .map_err(Into::into)
    }

    async fn execute_bash_with_temp(
        &self,
        prepared: PreparedBash,
        policy: BashLaunchPolicy,
        scope: Option<PreparedBashScope>,
        cancel: CancellationToken,
        temp_root: &TempRoot,
    ) -> BashAttempt {
        let launch_result = if let Some(scope) = &scope {
            self.prepare_scoped_bash_launch(&prepared, policy, Some(scope), &cancel, temp_root)
                .await
        } else {
            self.prepare_bash_launch(&prepared, policy, &cancel, temp_root)
                .await
        };
        let launch = match launch_result {
            Ok(launch) => launch,
            Err(failure) => {
                return BashAttempt {
                    result: Err(failure.error),
                    cleanup_safe: failure.cleanup_safe,
                };
            }
        };
        let mut attempt = self
            .execute_bash_command(prepared, launch.command, cancel, temp_root)
            .await;
        if let Some(lease) = launch.credential_lease
            && lease.shutdown().await.is_err()
        {
            attempt.result = Err(BashError::new(BashErrorCode::CleanupFailed));
            attempt.cleanup_safe = false;
        }
        attempt
    }

    async fn execute_bash_command(
        &self,
        prepared: PreparedBash,
        command: StdCommand,
        cancel: CancellationToken,
        temp_root: &TempRoot,
    ) -> BashAttempt {
        if cancel.is_cancelled() {
            return BashAttempt::safe(Err(BashError::new(BashErrorCode::Cancelled)));
        }
        let started = Instant::now();
        let mut command = Command::from(command);
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
        .map(|input| input.command)
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
        (Ok(left), Ok(right)) => {
            left.command == right.command
                && left.timeout_ms == right.timeout_ms
                && left.requested_scope == right.requested_scope
        }
        _ => false,
    }
}

struct ParsedBashInput {
    command: String,
    timeout_ms: u64,
    requested_scope: Option<BashScopeRequest>,
    description: Option<String>,
}

fn parse_bash_input(raw_input: &str) -> Result<ParsedBashInput, BashInputError> {
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
    if input.keys().any(|key| {
        !matches!(
            key.as_str(),
            "command" | "cmd" | "timeout" | "timeout_ms" | "requested_scope" | "description"
        )
    }) {
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
    let requested_scope = input
        .get("requested_scope")
        .map(|value| {
            serde_json::from_value(value.clone()).map_err(|_| BashInputError::InvalidScope)
        })
        .transpose()?;
    let description = input
        .get("description")
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or(BashInputError::InvalidDescription)
        })
        .transpose()?;
    Ok(ParsedBashInput {
        command: command.to_owned(),
        timeout_ms,
        requested_scope,
        description,
    })
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
