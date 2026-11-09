use super::*;

struct NeverPersist;

impl RuntimeShellOperationHook for NeverPersist {
    fn record(&self, _: RuntimeShellOperationRecord) -> BoxFuture<'static, Result<(), VegaError>> {
        panic!("launch validation must not persist or start commands")
    }
}

fn fixture() -> Result<(Tools, Arc<ShellRun>), Box<dyn std::error::Error>> {
    let tools = Tools::new(std::env::current_dir()?)?;
    let config = RuntimeToolConfig {
        thread_id: "thread".into(),
        ..RuntimeToolConfig::default().with_shell_operations(Arc::new(NeverPersist))
    };
    let owner = ShellRun::new(
        &tools,
        &config,
        Some("run".into()),
        CancellationToken::new(),
    );
    Ok((tools, owner))
}

#[test]
fn blocking_selection_does_not_confuse_zero_yield_with_timeout()
-> Result<(), Box<dyn std::error::Error>> {
    let (tools, _) = fixture()?;
    for raw in [
        r#"{"command":"true","timeout":240}"#,
        r#"{"command":"true","timeout":240,"wait_ms":null}"#,
    ] {
        assert_eq!(tools.prepare_bash_json(raw)?.wait_ms(), None);
    }
    let asynchronous =
        tools.prepare_bash_json(r#"{"command":"true","timeout":240,"wait_ms":0}"#)?;
    assert_eq!(asynchronous.wait_ms(), Some(0));
    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
fn incompatible_policies_explain_unstarted_command_without_changing_permissions()
-> Result<(), Box<dyn std::error::Error>> {
    let (tools, owner) = fixture()?;
    let prepared = tools.prepare_bash_json(r#"{"command":"true","wait_ms":0}"#)?;
    for (policy, reason) in [
        (BashLaunchPolicy::FullAccess, "Full Access policy"),
        (BashLaunchPolicy::ReadOnly, "read-only policy"),
    ] {
        let error = owner
            .validate_launch(&prepared, policy)
            .unwrap_err()
            .to_string();
        for expected in [
            "workspace_sandbox_required",
            "command NOT started",
            reason,
            "including 0",
            "wait_ms omitted or null",
            "Keep the current permissions",
            "No automatic retry or fallback",
        ] {
            assert!(error.contains(expected), "missing {expected}: {error}");
        }
        assert!(owner.operations()?.is_empty());
    }
    assert!(
        owner
            .validate_launch(&prepared, BashLaunchPolicy::WorkspaceWrite)
            .is_ok()
    );
    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
fn external_roots_are_distinguished_from_network_only_scope()
-> Result<(), Box<dyn std::error::Error>> {
    let (tools, owner) = fixture()?;
    let external = tools.prepare_bash_json(
        r#"{"command":"true","wait_ms":0,"requested_scope":{"write_roots":["/tmp"],"reason":"test"}}"#,
    )?;
    let error = owner
        .validate_launch(&external, BashLaunchPolicy::WorkspaceWrite)
        .unwrap_err()
        .to_string();
    assert!(error.contains("requested_scope.write_roots contains extra writable directories"));
    assert!(error.contains("command NOT started"));
    assert!(error.contains("any necessary scope"));
    let combined = owner
        .validate_launch(&external, BashLaunchPolicy::FullAccess)
        .unwrap_err()
        .to_string();
    assert!(combined.contains("Full Access policy"));
    assert!(combined.contains("extra writable directories"));
    let network = tools.prepare_bash_json(
        r#"{"command":"true","wait_ms":0,"requested_scope":{"network":true,"reason":"test"}}"#,
    )?;
    assert!(
        owner
            .validate_launch(&network, BashLaunchPolicy::WorkspaceWrite)
            .is_ok()
    );
    assert!(owner.operations()?.is_empty());
    Ok(())
}
