use super::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use vega_tools::{BashLaunchPolicy, BashSession, BashSessionSnapshot, PreparedBash, Tools};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeShellOperationRecord {
    pub operation_id: String,
    pub run_id: String,
    pub thread_id: String,
    pub call_id: String,
    pub generation: String,
    pub workspace: String,
    pub state: String,
    pub exit_code: Option<i32>,
    pub error: Option<String>,
    pub duration_ms: Option<u64>,
}

pub trait RuntimeShellOperationHook: Send + Sync {
    fn record(&self, record: RuntimeShellOperationRecord) -> BoxFuture<'static, Result<(), VegaError>>;
}

#[derive(Default)]
struct WorkspaceState {
    mutations: usize,
    session: bool,
    quarantined: bool,
}

#[derive(Default)]
struct Registry {
    retained: usize,
    active: usize,
    workspaces: HashMap<PathBuf, WorkspaceState>,
}

fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(Mutex::default)
}

fn generation() -> &'static str {
    static GENERATION: OnceLock<String> = OnceLock::new();
    GENERATION.get_or_init(|| ulid::Ulid::generate().to_string())
}

fn workspace_identity(workspace: &Path) -> Result<PathBuf, VegaError> {
    std::fs::canonicalize(workspace).map_err(|_| failure("workspace_identity_unavailable"))
}

async fn process_prerequisite() -> Result<(), VegaError> {
    use std::os::unix::fs::PermissionsExt;
    for path in ["/bin/kill", "/usr/bin/sandbox-exec"] {
        let metadata = std::fs::metadata(path).map_err(|_| failure("session_process_control_unavailable"))?;
        if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
            return Err(failure("session_process_control_unavailable"));
        }
    }
    let mut probe = tokio::process::Command::new("/bin/kill");
    probe.args(["-0", "--", &std::process::id().to_string()])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    match tokio::time::timeout(Duration::from_secs(2), probe.status()).await {
        Ok(Ok(status)) if status.success() => Ok(()),
        _ => Err(failure("session_process_control_unavailable")),
    }
}

fn failure(code: &str) -> VegaError {
    VegaError::Tool {
        tool: "bash".into(),
        message: format!("shell session: {code}"),
    }
}

pub struct RuntimeWorkspaceMutationGuard {
    workspace: PathBuf,
    additional: Vec<RuntimeWorkspaceMutationGuard>,
}

impl RuntimeWorkspaceMutationGuard {
    pub fn acquire(workspace: &Path) -> Result<Self, VegaError> {
        let workspace = workspace_identity(workspace)?;
        let mut registry = registry().lock().map_err(|_| failure("registry_unavailable"))?;
        let state = registry.workspaces.entry(workspace.clone()).or_default();
        if state.quarantined {
            return Err(failure("workspace_quarantined"));
        }
        if state.session {
            return Err(failure("wait_or_cancel_active_shell"));
        }
        if registry.workspaces.iter().any(|(path, state)| {
            (workspace.starts_with(path) || path.starts_with(&workspace)) && (state.session || state.quarantined)
        }) {
            return Err(failure("wait_or_cancel_active_shell"));
        }
        registry.workspaces.entry(workspace.clone()).or_default().mutations += 1;
        Ok(Self { workspace, additional: Vec::new() })
    }

    pub fn is_blocked(workspace: &Path) -> Result<bool, VegaError> {
        let workspace = workspace_identity(workspace)?;
        let registry = registry().lock().map_err(|_| failure("registry_unavailable"))?;
        Ok(registry.workspaces.iter().any(|(path, state)| {
            (workspace.starts_with(path) || path.starts_with(&workspace)) && (state.session || state.quarantined)
        }))
    }
}

impl Drop for RuntimeWorkspaceMutationGuard {
    fn drop(&mut self) {
        if let Ok(mut registry) = registry().lock()
            && let Some(state) = registry.workspaces.get_mut(&self.workspace)
        {
            state.mutations = state.mutations.saturating_sub(1);
            if state.mutations == 0 && !state.session && !state.quarantined {
                registry.workspaces.remove(&self.workspace);
            }
        }
    }
}

struct Reservation {
    workspace: PathBuf,
    active: AtomicBool,
}

impl Reservation {
    fn acquire(workspace: &Path) -> Result<Self, VegaError> {
        let mut registry = registry().lock().map_err(|_| failure("registry_unavailable"))?;
        if registry.retained >= 64 || registry.active >= 4 {
            return Err(failure("application_capacity_reached"));
        }
        if registry.workspaces.iter().any(|(path, state)| {
            (workspace.starts_with(path) || path.starts_with(workspace)) && (state.session || state.quarantined || state.mutations != 0)
        }) {
            return Err(failure("workspace_busy"));
        }
        let state = registry.workspaces.entry(workspace.to_path_buf()).or_default();
        if state.quarantined {
            return Err(failure("workspace_quarantined"));
        }
        if state.session || state.mutations != 0 {
            return Err(failure("workspace_busy"));
        }
        state.session = true;
        registry.retained += 1;
        registry.active += 1;
        Ok(Self { workspace: workspace.to_path_buf(), active: AtomicBool::new(true) })
    }

    fn finish(&self, cleanup_safe: bool) {
        if !self.active.swap(false, Ordering::AcqRel) {
            return;
        }
        if let Ok(mut registry) = registry().lock() {
            if cleanup_safe {
                registry.active = registry.active.saturating_sub(1);
            }
            if let Some(state) = registry.workspaces.get_mut(&self.workspace) {
                state.session = false;
                state.quarantined |= !cleanup_safe;
                if state.mutations == 0 && !state.quarantined {
                    registry.workspaces.remove(&self.workspace);
                }
            }
        }
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        self.finish(false);
        if let Ok(mut registry) = registry().lock() {
            registry.retained = registry.retained.saturating_sub(1);
        }
    }
}

struct Operation {
    handle: String,
    record: RuntimeShellOperationRecord,
    session: BashSession,
    reservation: Reservation,
    terminal: tokio::sync::Mutex<bool>,
    persistence_failed: AtomicBool,
}

pub(super) struct ShellRun {
    workspace: PathBuf,
    run_id: Option<String>,
    thread_id: String,
    hook: Option<Arc<dyn RuntimeShellOperationHook>>,
    cancel: CancellationToken,
    closed: AtomicBool,
    lifecycle: tokio::sync::Mutex<()>,
    operations: Mutex<Vec<Arc<Operation>>>,
    instance: Mutex<Option<u64>>,
    failed: AtomicBool,
    monitors: Mutex<Vec<tokio::task::JoinHandle<()>>>,
}

impl ShellRun {
    pub(super) fn new(tools: &Tools, config: &RuntimeToolConfig, run_id: Option<String>, cancel: CancellationToken) -> Arc<Self> {
        Arc::new(Self {
            workspace: tools.root().to_path_buf(),
            run_id: run_id.filter(|id| !id.trim().is_empty()),
            thread_id: config.thread_id.clone(),
            hook: config.shell_operations.clone(),
            cancel,
            closed: AtomicBool::new(false),
            lifecycle: tokio::sync::Mutex::new(()),
            operations: Mutex::new(Vec::new()),
            instance: Mutex::new(None),
            failed: AtomicBool::new(false),
            monitors: Mutex::new(Vec::new()),
        })
    }

    fn operations(&self) -> Result<Vec<Arc<Operation>>, VegaError> {
        self.operations.lock().map(|operations| operations.clone()).map_err(|_| failure("registry_unavailable"))
    }

    fn open(&self) -> Result<(), VegaError> {
        if self.closed.load(Ordering::Acquire) || self.cancel.is_cancelled() {
            return Err(failure("run_closed"));
        }
        Ok(())
    }

    pub(super) fn stop_admission(&self) {
        self.closed.store(true, Ordering::Release);
    }

    pub(super) fn active(&self) -> Result<bool, VegaError> {
        Ok(self.operations()?.iter().any(|operation| operation.reservation.active.load(Ordering::Acquire)))
    }

    pub(super) fn validate_launch(&self, prepared: &PreparedBash, policy: BashLaunchPolicy) -> Result<(), VegaError> {
        self.open()?;
        if self.hook.is_none() || self.run_id.is_none() || self.thread_id.is_empty() {
            return Err(failure("durable_run_identity_required"));
        }
        if !cfg!(target_os = "macos") {
            return Err(failure("session_process_control_unsupported"));
        }
        if policy != BashLaunchPolicy::WorkspaceWrite || prepared.requested_scope().is_some_and(|scope| !scope.write_roots().is_empty()) {
            return Err(failure("workspace_sandbox_required"));
        }
        if workspace_identity(prepared.project_root())? != self.workspace {
            return Err(failure("workspace_identity_mismatch"));
        }
        let mut instance = self.instance.lock().map_err(|_| failure("registry_unavailable"))?;
        if instance.is_some_and(|instance| instance != prepared.tool_instance_id()) {
            return Err(failure("tool_identity_mismatch"));
        }
        *instance = Some(prepared.tool_instance_id());
        let operations = self.operations()?;
        if operations.len() >= 4 {
            return Err(failure("run_retained_capacity_reached"));
        }
        if operations.iter().any(|operation| operation.reservation.active.load(Ordering::Acquire)) {
            return Err(failure("wait_or_cancel_active_shell"));
        }
        Ok(())
    }

    async fn record(&self, record: RuntimeShellOperationRecord) -> Result<(), VegaError> {
        self.hook.as_ref().ok_or_else(|| failure("persistence_unavailable"))?.record(record).await.map_err(|_| failure("operation_persistence_failed"))
    }

    async fn finish(&self, operation: &Operation, snapshot: &BashSessionSnapshot) -> Result<(), VegaError> {
        if !terminal_state(&snapshot.state) {
            return Ok(());
        }
        let mut terminal = operation.terminal.lock().await;
        if *terminal {
            return Ok(());
        }
        let mut record = operation.record.clone();
        record.state = if snapshot.cleanup_safe { snapshot.state.clone() } else { "outcome_unknown".into() };
        record.exit_code = snapshot.exit_code;
        record.error = if operation.persistence_failed.load(Ordering::Acquire) {
            record.state = if snapshot.cleanup_safe { "failed".into() } else { "outcome_unknown".into() };
            Some("operation_persistence_failed".into())
        } else {
            snapshot.error.clone()
        };
        record.duration_ms = Some(snapshot.duration_ms);
        let persisted = self.record(record).await;
        if persisted.is_err() || !snapshot.cleanup_safe {
            self.failed.store(true, Ordering::Release);
            self.closed.store(true, Ordering::Release);
            self.cancel.cancel();
        }
        if persisted.is_err()
            && let Ok(mut registry) = registry().lock()
        {
            registry.workspaces.entry(self.workspace.clone()).or_default().quarantined = true;
        }
        operation.reservation.finish(snapshot.cleanup_safe);
        if persisted.is_ok() {
            *terminal = true;
        }
        persisted
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn launch(self: &Arc<Self>, call: &RuntimeToolCall, tools: Tools, prepared: PreparedBash, policy: BashLaunchPolicy, hook: &dyn RuntimePermissionHook, config: &RuntimeToolConfig) -> Result<String, VegaError> {
        let lifecycle = self.lifecycle.lock().await;
        self.validate_launch(&prepared, policy)?;
        let reservation = Reservation::acquire(&self.workspace)?;
        let mut record = RuntimeShellOperationRecord {
            operation_id: ulid::Ulid::generate().to_string(),
            run_id: self.run_id.clone().ok_or_else(|| failure("durable_run_identity_required"))?,
            thread_id: self.thread_id.clone(),
            call_id: call.id.clone(),
            generation: generation().into(),
            workspace: self.workspace.to_string_lossy().into_owned(),
            state: "prepared".into(),
            exit_code: None,
            error: None,
            duration_ms: None,
        };
        if self.record(record.clone()).await.is_err() {
            reservation.finish(true);
            return Err(failure("operation_persistence_failed"));
        }
        if let Err(error) = process_prerequisite().await {
            record.state = "failed".into();
            record.error = Some("session_process_control_unavailable".into());
            let persisted = self.record(record).await;
            reservation.finish(true);
            persisted?;
            return Err(error);
        }
        if self.open().is_err() {
            record.state = "cancelled".into();
            let result = self.record(record).await;
            reservation.finish(true);
            result?;
            return Err(failure("run_closed"));
        }
        let wait_ms = prepared.wait_ms().ok_or_else(|| failure("explicit_wait_required"))?;
        let mut redaction = config.clone();
        redaction.shell_run = None;
        redaction.shell_operations = None;
        let reader = redaction.credential_reader.clone();
        let redact = Arc::new(move |text: &str| redaction.redact_tool_output(text).map_err(|_| "redaction_unavailable".to_string()));
        let started = if prepared.requested_scope().is_some() {
            hook.start_scoped_bash_session(call.id.clone(), tools, prepared, policy, self.cancel.child_token(), redact).await
        } else {
            let safe_end = Arc::new(move |text: &str| {
                let known = match &reader {
                    Some(reader) => reader().map_err(|_| "redaction_unavailable".to_string())?,
                    None => Vec::new(),
                };
                Ok(credential_stream_safe_end(text, &known))
            });
            match vega_tools::BashSessionRedactor::new(redact, safe_end, 64 * 1024) {
                Ok(redactor) => tools.start_bash_session_with_redactor(prepared, policy, None, self.cancel.child_token(), redactor).await,
                Err(error) => Err(error),
            }
        };
        let session = match started {
            Ok(session) => session,
            Err(error) => {
                record.state = "failed".into();
                record.error = Some(error.code().tool_output());
                let result = self.record(record).await;
                reservation.finish(true);
                result?;
                return Err(failure("session_launch_failed"));
            }
        };
        record.state = "running".into();
        let operation = Arc::new(Operation {
            handle: ulid::Ulid::generate().to_string(),
            record: record.clone(),
            session,
            reservation,
            terminal: tokio::sync::Mutex::new(false),
            persistence_failed: AtomicBool::new(false),
        });
        let registered = self.operations.lock().map(|mut operations| operations.push(operation.clone())).is_ok();
        if !registered {
            self.closed.store(true, Ordering::Release);
            self.failed.store(true, Ordering::Release);
            self.cancel.cancel();
            let snapshot = operation.session.cancel_and_wait().await;
            let _ = self.finish(&operation, &snapshot).await;
            return Err(failure("registry_unavailable"));
        }
        if self.record(record).await.is_err() {
            operation.persistence_failed.store(true, Ordering::Release);
            self.failed.store(true, Ordering::Release);
            self.closed.store(true, Ordering::Release);
            self.cancel.cancel();
            let snapshot = operation.session.cancel_and_wait().await;
            let _ = self.finish(&operation, &snapshot).await;
            return Err(failure("operation_persistence_failed"));
        }
        let owner = self.clone();
        let watched = operation.clone();
        let monitor = tokio::spawn(async move {
            loop {
                match watched.session.observe(None, 300_000).await {
                    Ok(snapshot) if terminal_state(&snapshot.state) => {
                        let _ = owner.finish(&watched, &snapshot).await;
                        break;
                    }
                    Ok(_) => {}
                    Err(_) => {
                        owner.closed.store(true, Ordering::Release);
                        owner.failed.store(true, Ordering::Release);
                        owner.cancel.cancel();
                        let snapshot = watched.session.cancel_and_wait().await;
                        let _ = owner.finish(&watched, &snapshot).await;
                        break;
                    }
                }
            }
        });
        let registered = self.monitors.lock().map(|mut monitors| monitors.push(monitor)).is_ok();
        if !registered {
            self.closed.store(true, Ordering::Release);
            self.failed.store(true, Ordering::Release);
            self.cancel.cancel();
            let snapshot = operation.session.cancel_and_wait().await;
            let _ = self.finish(&operation, &snapshot).await;
            return Err(failure("registry_unavailable"));
        }
        drop(lifecycle);
        self.observe_operation(&operation, None, wait_ms, false).await
    }

    async fn observe_operation(&self, operation: &Operation, cursor: Option<u64>, wait_ms: u64, cancel: bool) -> Result<String, VegaError> {
        let snapshot = if cancel {
            operation.session.cancel_and_wait().await
        } else {
            let observe = operation.session.observe(cursor, wait_ms);
            tokio::pin!(observe);
            tokio::select! {
                biased;
                _ = self.cancel.cancelled() => operation.session.cancel_and_wait().await,
                snapshot = &mut observe => snapshot.map_err(|_| failure("session_observation_failed"))?,
            }
        };
        self.finish(operation, &snapshot).await?;
        Ok(serde_json::json!({
            "handle": operation.handle,
            "operation_id": operation.record.operation_id,
            "state": snapshot.state,
            "exit_code": snapshot.exit_code,
            "output": snapshot.output,
            "next_cursor": snapshot.next_cursor,
            "retained_from": snapshot.retained_from,
            "output_gap": snapshot.output_gap,
            "truncated": snapshot.truncated,
            "duration_ms": snapshot.duration_ms,
            "error": snapshot.error,
            "cleanup_safe": snapshot.cleanup_safe,
        }).to_string())
    }

    pub(super) async fn observe(&self, input: &SessionInput, cancel: bool, tools: &Tools) -> Result<String, VegaError> {
        self.open()?;
        if workspace_identity(tools.root())? != self.workspace {
            return Err(failure("workspace_identity_mismatch"));
        }
        let identity = tools.prepare_bash_json("{\"cmd\":\"true\"}").map_err(|_| failure("tool_identity_unavailable"))?;
        if *self.instance.lock().map_err(|_| failure("registry_unavailable"))? != Some(identity.tool_instance_id()) {
            return Err(failure("tool_identity_mismatch"));
        }
        let operation = self.operations()?.into_iter().find(|operation| operation.handle == input.handle).ok_or_else(|| failure("unknown_or_expired_handle"))?;
        self.observe_operation(&operation, input.cursor, input.wait_ms.unwrap_or(30_000), cancel).await
    }

    pub(super) fn live_receipt(&self, output: &str) -> bool {
        let Ok(value) = serde_json::from_str::<Value>(output) else { return false; };
        self.open().is_ok() && self.operations().is_ok_and(|operations| operations.iter().any(|operation| {
            value["handle"].as_str() == Some(operation.handle.as_str()) && value["operation_id"].as_str() == Some(operation.record.operation_id.as_str())
        }))
    }

    pub(super) async fn shutdown(&self) -> Result<(), VegaError> {
        self.closed.store(true, Ordering::Release);
        let _lifecycle = self.lifecycle.lock().await;
        let operations = self.operations()?;
        let mut failed = false;
        for operation in &operations {
            let snapshot = operation.session.cancel_and_wait().await;
            failed |= !snapshot.cleanup_safe;
            failed |= self.finish(operation, &snapshot).await.is_err();
        }
        let monitors = self.monitors.lock().map(|mut monitors| std::mem::take(&mut *monitors)).map_err(|_| failure("registry_unavailable"))?;
        for monitor in monitors {
            failed |= monitor.await.is_err();
        }
        self.operations.lock().map_err(|_| failure("registry_unavailable"))?.clear();
        drop(operations);
        if failed || self.failed.load(Ordering::Acquire) {
            Err(failure("shell_shutdown_failed_or_outcome_unknown"))
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SessionInput {
    handle: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cursor: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    wait_ms: Option<u64>,
}

impl SessionInput {
    pub(super) fn semantically_equal(&self, other: &Self) -> bool {
        self.handle == other.handle
            && self.cursor.unwrap_or(0) == other.cursor.unwrap_or(0)
            && self.wait_ms.unwrap_or(30_000) == other.wait_ms.unwrap_or(30_000)
    }

    pub(super) fn parse(raw: &str, cancel: bool) -> Option<Self> {
        if raw.len() > 2048 {
            return None;
        }
        let value: Value = serde_json::from_str(raw).ok()?;
        let object = value.as_object()?;
        if cancel && (object.contains_key("cursor") || object.contains_key("wait_ms")) {
            return None;
        }
        let input: Self = serde_json::from_value(value).ok()?;
        if input.handle.len() != 26 || ulid::Ulid::from_string(&input.handle).is_err() || input.wait_ms.is_some_and(|wait| wait > 300_000) {
            return None;
        }
        Some(input)
    }
}

#[cfg(test)]
mod tests;

fn terminal_state(state: &str) -> bool {
    matches!(state, "succeeded" | "failed" | "timed_out" | "cancelled" | "outcome_unknown")
}

pub(super) fn admit_prepared(call: &super::tools_exec::PreparedRuntimeCall, config: &RuntimeToolConfig) -> Result<Option<RuntimeWorkspaceMutationGuard>, VegaError> {
    let Some(owner) = &config.shell_run else { return Ok(None); };
    owner.open()?;
    if owner.active()? && !independent_read(call) {
        return Err(failure("wait_or_cancel_active_shell"));
    }
    if independent_read(call) {
        return Ok(None);
    }
    RuntimeWorkspaceMutationGuard::acquire(&owner.workspace).map(Some)
}

pub(super) fn execution_admission(call: &super::tools_exec::PreparedRuntimeCall, config: &RuntimeToolConfig, tools: &Tools) -> Result<Option<RuntimeWorkspaceMutationGuard>, VegaError> {
    if matches!(call, super::tools_exec::PreparedRuntimeCall::Bash { prepared, .. } if prepared.wait_ms().is_some()) {
        return Ok(None);
    }
    admit(call, config, tools)
}

pub(super) fn independent_read(call: &super::tools_exec::PreparedRuntimeCall) -> bool {
    use super::tools_exec::PreparedRuntimeCall;
    matches!(call, PreparedRuntimeCall::PinnedRead { .. } | PreparedRuntimeCall::Grep { target: None, .. }) || matches!(call, PreparedRuntimeCall::Readonly(call) if call.name == "glob") || matches!(call, PreparedRuntimeCall::ShellControl { .. })
}

pub(super) fn admit(call: &super::tools_exec::PreparedRuntimeCall, config: &RuntimeToolConfig, tools: &Tools) -> Result<Option<RuntimeWorkspaceMutationGuard>, VegaError> {
    if let Some(owner) = &config.shell_run {
        owner.open()?;
        if owner.active()? && !independent_read(call) {
            return Err(failure("wait_or_cancel_active_shell"));
        }
    }
    if independent_read(call) {
        return Ok(None);
    }
    let mut guard = RuntimeWorkspaceMutationGuard::acquire(tools.root())?;
    use super::tools_exec::PreparedRuntimeCall;
    let external_path = match call {
        PreparedRuntimeCall::Write { prepared, .. } => Some(prepared.normalized_path()),
        PreparedRuntimeCall::Edit { prepared, .. } => Some(prepared.normalized_path()),
        _ => None,
    };
    if let Some(path) = external_path {
        let path = tools.file_path(path).map_err(|_| failure("mutation_target_unavailable"))?;
        if !path.starts_with(tools.root()) {
            let mut parent = path.parent().ok_or_else(|| failure("mutation_target_unavailable"))?;
            while !parent.exists() {
                parent = parent.parent().ok_or_else(|| failure("mutation_target_unavailable"))?;
            }
            guard.additional.push(RuntimeWorkspaceMutationGuard::acquire(parent)?);
        }
    }
    if let PreparedRuntimeCall::Bash { prepared, policy, .. } = call {
        if *policy == BashLaunchPolicy::FullAccess {
            guard.additional.push(RuntimeWorkspaceMutationGuard::acquire(Path::new("/"))?);
        } else if let Some(scope) = prepared.requested_scope() {
            for root in scope.write_roots() {
                guard.additional.push(RuntimeWorkspaceMutationGuard::acquire(Path::new(root))?);
            }
        }
    }
    Ok(Some(guard))
}
