use super::*;

struct Persistence;

impl RuntimeShellOperationHook for Persistence {
    fn record(&self, _: RuntimeShellOperationRecord) -> BoxFuture<'static, Result<(), VegaError>> {
        async { Ok(()) }.boxed()
    }
}

#[test]
fn authorized_background_preflight_remains_run_bound_without_enforcing_exclusion() {
    let root = std::env::temp_dir().join(format!("vega-preflight-{}", ulid::Ulid::generate()));
    std::fs::create_dir(&root).unwrap();
    let tools = Tools::new(&root).unwrap();
    let mut config = RuntimeToolConfig {
        thread_id: "thread".into(),
        ..RuntimeToolConfig::default()
    };
    config.shell_operations = Some(Arc::new(Persistence));
    let owner = ShellRun::new(
        &tools,
        &config,
        Some("run".into()),
        CancellationToken::new(),
    );
    let prepared = tools
        .prepare_bash_json(r#"{"command":"true","timeout":120,"wait_ms":0}"#)
        .unwrap();
    assert_eq!(prepared.wait_ms(), Some(0));
    assert!(
        owner
            .validate_launch(&prepared, BashLaunchPolicy::FullAccess)
            .is_ok()
    );
    assert!(
        owner
            .validate_launch(&prepared, BashLaunchPolicy::ReadOnly)
            .is_err()
    );
    let other = Tools::new(&root).unwrap();
    let other = other
        .prepare_bash_json(r#"{"command":"true","timeout":120,"wait_ms":0}"#)
        .unwrap();
    assert!(
        owner
            .validate_launch(&other, BashLaunchPolicy::FullAccess)
            .is_err()
    );
    owner.stop_admission();
    assert!(
        owner
            .validate_launch(&prepared, BashLaunchPolicy::FullAccess)
            .is_err()
    );
    std::fs::remove_dir(root).unwrap();
}

#[test]
fn concurrent_reservations_never_acquire_workspace_exclusion() {
    let root = std::env::temp_dir().join(format!("vega-concurrency-{}", ulid::Ulid::generate()));
    std::fs::create_dir(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let sandbox = Reservation::acquire(&root, false).unwrap();
    let first = Reservation::acquire(&root, true).unwrap();
    let second = Reservation::acquire(&root, true).unwrap();
    assert!(RuntimeWorkspaceMutationGuard::acquire(&root).is_err());
    assert!(Reservation::acquire(&root, false).is_err());
    sandbox.finish(true);
    let write = RuntimeWorkspaceMutationGuard::acquire(&root).unwrap();
    first.finish(false);
    second.finish(false);
    drop(write);
    assert!(RuntimeWorkspaceMutationGuard::acquire(&root).is_ok());
    let concurrent = Reservation::acquire(&root, true).unwrap();
    let sandbox = Reservation::acquire(&root, false).unwrap();
    concurrent.finish(false);
    assert!(RuntimeWorkspaceMutationGuard::acquire(&root).is_err());
    sandbox.finish(false);
    let bypass = Reservation::acquire(&root, true).unwrap();
    bypass.finish(false);
    assert!(RuntimeWorkspaceMutationGuard::acquire(&root).is_err());
    sandbox.release_blocker().unwrap();
    assert!(RuntimeWorkspaceMutationGuard::acquire(&root).is_ok());
    let slots: Vec<_> = (0..4)
        .map(|_| Reservation::acquire(&root, true).unwrap())
        .collect();
    assert!(Reservation::acquire(&root, true).is_err());
    assert!(RuntimeWorkspaceMutationGuard::acquire(&root).is_ok());
    slots[0].settle(false, false);
    assert!(Reservation::acquire(&root, true).is_err());
    slots[0].settle(true, false);
    let replacement = Reservation::acquire(&root, true).unwrap();
    assert!(RuntimeWorkspaceMutationGuard::acquire(&root).is_ok());
    replacement.finish(true);
    drop(slots);
    std::fs::remove_dir(root).unwrap();
}
