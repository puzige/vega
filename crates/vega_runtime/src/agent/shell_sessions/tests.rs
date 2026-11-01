use super::*;
use std::error::Error;
use std::os::unix::fs::symlink;

fn serial() -> &'static tokio::sync::Mutex<()> {
    static SERIAL: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    SERIAL.get_or_init(|| tokio::sync::Mutex::new(()))
}

struct Workspace(PathBuf);

impl Workspace {
    fn new() -> Result<Self, std::io::Error> {
        let path = std::env::temp_dir().join(format!("vega-shell-runtime-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&path)?;
        Ok(Self(path.canonicalize()?))
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn workspace_mutations_exclude_sessions_through_aliases_and_nested_roots() -> Result<(), Box<dyn Error>> {
    let _serial = serial().lock().await;
    let workspace = Workspace::new()?;
    let nested = workspace.0.join("nested");
    std::fs::create_dir(&nested)?;
    let alias = workspace.0.with_extension("alias");
    symlink(&workspace.0, &alias)?;
    let mutation = RuntimeWorkspaceMutationGuard::acquire(&alias)?;
    assert!(Reservation::acquire(&workspace.0).is_err());
    assert!(Reservation::acquire(&nested).is_err());
    drop(mutation);
    let reservation = Reservation::acquire(&workspace.0)?;
    assert!(RuntimeWorkspaceMutationGuard::is_blocked(&alias)?);
    assert!(RuntimeWorkspaceMutationGuard::acquire(&nested).is_err());
    assert!(RuntimeWorkspaceMutationGuard::acquire(&alias).is_err());
    reservation.finish(true);
    assert!(!RuntimeWorkspaceMutationGuard::is_blocked(&alias)?);
    drop(RuntimeWorkspaceMutationGuard::acquire(&alias)?);
    std::fs::remove_file(alias)?;
    Ok(())
}

#[tokio::test]
async fn application_capacity_rejects_before_launch_and_retains_completed_operations() -> Result<(), Box<dyn Error>> {
    let _serial = serial().lock().await;
    let workspace = Workspace::new()?;
    let mut active = Vec::new();
    for index in 0..4 {
        let path = workspace.0.join(index.to_string());
        std::fs::create_dir(&path)?;
        active.push(Reservation::acquire(&path)?);
    }
    let extra = workspace.0.join("extra");
    std::fs::create_dir(&extra)?;
    assert!(Reservation::acquire(&extra).is_err());
    for reservation in &active {
        reservation.finish(true);
    }
    let mut retained = active;
    for _ in 4..64 {
        let reservation = Reservation::acquire(&extra)?;
        reservation.finish(true);
        retained.push(reservation);
    }
    assert!(Reservation::acquire(&extra).is_err());
    retained.pop();
    let reservation = Reservation::acquire(&extra)?;
    reservation.finish(true);
    drop(reservation);
    drop(retained);
    let reservation = Reservation::acquire(&extra)?;
    reservation.finish(true);
    Ok(())
}

#[tokio::test]
async fn unknown_cleanup_keeps_workspace_quarantined_after_owner_release() -> Result<(), Box<dyn Error>> {
    let _serial = serial().lock().await;
    let workspace = Workspace::new()?;
    let reservation = Reservation::acquire(&workspace.0)?;
    reservation.finish(false);
    drop(reservation);
    assert!(RuntimeWorkspaceMutationGuard::is_blocked(&workspace.0)?);
    assert!(RuntimeWorkspaceMutationGuard::acquire(&workspace.0).is_err());
    assert!(Reservation::acquire(&workspace.0).is_err());
    {
        let mut registry = registry().lock().map_err(|_| "registry unavailable")?;
        registry.workspaces.remove(&workspace.0);
        registry.active = registry.active.saturating_sub(1);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
struct PersistenceFailure {
    records: Arc<Mutex<Vec<RuntimeShellOperationRecord>>>,
}

#[cfg(target_os = "macos")]
impl RuntimeShellOperationHook for PersistenceFailure {
    fn record(&self, record: RuntimeShellOperationRecord) -> BoxFuture<'static, Result<(), VegaError>> {
        let records = self.records.clone();
        async move {
            records.lock().map_err(|_| failure("test_record_unavailable"))?.push(record);
            Err(failure("injected_persistence_failure"))
        }.boxed()
    }
}

#[tokio::test]
async fn absent_persistence_or_cancelled_run_rejects_session_authority() -> Result<(), Box<dyn Error>> {
    let _serial = serial().lock().await;
    let workspace = Workspace::new()?;
    let tools = Tools::new(&workspace.0)?;
    let mut config = RuntimeToolConfig::default();
    config.thread_id = "thread".into();
    let cancel = CancellationToken::new();
    let owner = ShellRun::new(&tools, &config, Some("run".into()), cancel.clone());
    let prepared = tools.prepare_bash_json("{\"cmd\":\"true\",\"wait_ms\":0}")?;
    assert!(owner.validate_launch(&prepared, BashLaunchPolicy::WorkspaceWrite).is_err());
    assert!(!owner.live_receipt("{\"handle\":\"historical\",\"operation_id\":\"historical\"}"));
    let input = SessionInput::parse(&serde_json::json!({"handle":ulid::Ulid::generate().to_string()}).to_string(), false).ok_or("session input rejected")?;
    assert!(owner.observe(&input, false, &tools).await.is_err());
    cancel.cancel();
    assert!(owner.open().is_err());
    assert!(owner.observe(&input, true, &tools).await.is_err());
    owner.shutdown().await?;
    assert!(!RuntimeWorkspaceMutationGuard::is_blocked(&workspace.0)?);
    Ok(())
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn failed_intent_persistence_never_spawns_or_leaves_a_live_handle() -> Result<(), Box<dyn Error>> {
    let _serial = serial().lock().await;
    let workspace = Workspace::new()?;
    let tools = Tools::new(&workspace.0)?;
    let records = Arc::new(Mutex::new(Vec::new()));
    let mut config = RuntimeToolConfig::default().with_shell_operations(Arc::new(PersistenceFailure { records: records.clone() }));
    config.thread_id = "thread".into();
    let owner = ShellRun::new(&tools, &config, Some("run".into()), CancellationToken::new());
    let prepared = tools.prepare_bash_json("{\"cmd\":\"touch must-not-exist\",\"wait_ms\":0}")?;
    let call = RuntimeToolCall { id: "call".into(), name: "bash".into(), input_json: prepared.input_json(), description: None };
    let result = owner.launch(&call, tools, prepared, BashLaunchPolicy::WorkspaceWrite, &RejectPermissionHook, &config).await;
    assert!(result.is_err_and(|error| error.to_string().contains("operation_persistence_failed")));
    {
        let records = records.lock().map_err(|_| "record lock poisoned")?;
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].state, "prepared");
    }
    assert!(!workspace.0.join("must-not-exist").exists());
    assert!(!owner.active()?);
    assert!(!RuntimeWorkspaceMutationGuard::is_blocked(&workspace.0)?);
    owner.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn legacy_bash_without_wait_keeps_original_readonly_authorization() -> Result<(), Box<dyn Error>> {
    use super::super::tools_exec::{Authorization, PreparedRuntimeCall, authorize_call};
    let _serial = serial().lock().await;
    let workspace = Workspace::new()?;
    let tools = Tools::new(&workspace.0)?;
    let config = RuntimeToolConfig::default();
    for raw in ["{\"cmd\":\"pwd\"}", "{\"cmd\":\"pwd\",\"wait_ms\":null}"] {
        let prepared = tools.prepare_bash_json(raw)?;
        assert_eq!(prepared.wait_ms(), None);
        let call = RuntimeToolCall { id: ulid::Ulid::generate().to_string(), name: "bash".into(), input_json: raw.into(), description: None };
        let prepared = PreparedRuntimeCall::Bash { call, tools: tools.clone(), prepared, policy: BashLaunchPolicy::ReadOnly };
        let authorization = authorize_call(&prepared, &config, &HashSet::new(), &RejectPermissionHook, &CancellationToken::new()).await?;
        assert!(matches!(authorization, Authorization::Approved { .. }));
    }
    Ok(())
}

#[test]
fn session_observation_parameters_reject_scope_expansion_and_out_of_range_waits() {
    let handle = ulid::Ulid::generate().to_string();
    assert!(SessionInput::parse(&serde_json::json!({"handle":handle,"wait_ms":300000}).to_string(), false).is_some());
    for input in [
        serde_json::json!({"handle":handle,"wait_ms":300001}),
        serde_json::json!({"handle":handle,"cursor":-1}),
        serde_json::json!({"handle":handle,"command":"true"}),
        serde_json::json!({"handle":handle,"requested_scope":{"network":true}}),
    ] {
        assert!(SessionInput::parse(&input.to_string(), false).is_none());
    }
    assert!(SessionInput::parse(&serde_json::json!({"handle":handle,"wait_ms":0}).to_string(), true).is_none());
    assert!(SessionInput::parse(&serde_json::json!({"handle":handle}).to_string(), true).is_some());
}
