use super::*;
use std::ffi::{OsStr, OsString};

fn arguments(command: &StdCommand) -> Vec<OsString> {
    command.get_args().map(OsStr::to_owned).collect()
}

fn profile(command: &StdCommand) -> String {
    let args = arguments(command);
    let index = args
        .iter()
        .position(|arg| arg == "-p")
        .expect("profile argument");
    args[index + 1].to_str().expect("utf8 profile").into()
}

fn environment(command: &StdCommand) -> BTreeMap<OsString, Option<OsString>> {
    command
        .get_envs()
        .map(|(key, value)| (key.to_owned(), value.map(OsStr::to_owned)))
        .collect()
}

#[test]
fn restricted_probe_and_shell_share_final_policy_and_controlled_environment() {
    let project = TempRoot::create().expect("project fixture");
    let temporary = TempRoot::create().expect("command temp fixture");
    let protected = project.path().join("protected-credentials");
    std::fs::create_dir(&protected).expect("protected fixture root");
    for policy in [BashLaunchPolicy::ReadOnly, BashLaunchPolicy::WorkspaceWrite] {
        let sandbox = SandboxConfig::new(project.path(), policy, &temporary, Some(&protected))
            .expect("restricted config");
        let probe = sandbox.self_test_command(&temporary).expect("probe plan");
        let shell = sandbox
            .shell_command("printf fixture", &temporary)
            .expect("shell plan");
        assert_eq!(probe.get_program(), OsStr::new("/usr/bin/sandbox-exec"));
        assert_eq!(shell.get_program(), probe.get_program());
        let final_profile = profile(&shell);
        assert_eq!(profile(&probe), final_profile);
        assert!(final_profile.starts_with("(version 1)\n(deny default)"));
        assert!(final_profile.contains("(allow file-read*)"));
        assert!(final_profile.contains("(allow process-info-pidinfo (target self))"));
        assert!(final_profile.contains("(allow process-info-setcontrol (target self))"));
        assert!(final_profile.contains("(allow signal (target same-sandbox))"));
        assert!(final_profile.contains("(vnode-type CHARACTER-DEVICE)"));
        for denied in [
            "(allow default)",
            "(allow network",
            "(allow process-info*",
            "(allow ipc-",
            "(allow iokit-",
            "(allow pseudo-tty",
            "net.routetable",
            "kern.proc.",
        ] {
            assert!(
                !final_profile.contains(denied),
                "unexpected capability: {denied}"
            );
        }
        let workspace_write = "(allow file-write* (subpath (param \"PROJECT_ROOT\")))";
        let temporary_write = "(allow file-write* (subpath (param \"TEMP_ROOT\")))";
        let writable = policy == BashLaunchPolicy::WorkspaceWrite;
        assert_eq!(final_profile.contains(workspace_write), writable);
        assert_eq!(final_profile.contains(temporary_write), writable);
        let git_deny = final_profile
            .find("(deny file-write* (subpath (param \"GIT_ENTRY\")))")
            .expect("Git entry forced deny");
        assert!(final_profile.contains("(deny file-write* (subpath (param \"GIT_DIR\")))"));
        if writable {
            assert!(
                git_deny
                    > final_profile
                        .find(workspace_write)
                        .expect("workspace allow")
            );
            assert!(git_deny > final_profile.find(temporary_write).expect("temp allow"));
        }
        assert!(final_profile.ends_with(&format!(
            "(deny file-read* file-write* (subpath \"{}\"))",
            protected.display()
        )));
        let allowed_mach = final_profile
            .lines()
            .filter(|line| line.starts_with("(allow mach-lookup"))
            .collect::<Vec<_>>();
        assert_eq!(
            allowed_mach,
            ["(allow mach-lookup (global-name \"com.apple.system.opendirectoryd.libinfo\"))"]
        );
        assert!(final_profile.contains("(deny mach-lookup (global-name \"com.apple.taskgated\"))"));
        let expected_environment = [
            (
                "PATH",
                "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin",
            ),
            ("HOME", temporary.param()),
            ("ZDOTDIR", temporary.param()),
            ("LANG", "en_US.UTF-8"),
            ("TMPDIR", temporary.param()),
            ("TMP", temporary.param()),
            ("TEMP", temporary.param()),
            ("TEMPDIR", temporary.param()),
        ]
        .into_iter()
        .map(|(key, value)| (OsString::from(key), Some(OsString::from(value))))
        .collect::<BTreeMap<_, _>>();
        assert_eq!(environment(&probe), expected_environment);
        assert_eq!(environment(&shell), expected_environment);
        assert_eq!(probe.get_current_dir(), Some(project.path()));
        assert_eq!(shell.get_current_dir(), Some(project.path()));
        let args = arguments(&shell);
        assert_eq!(
            &args[args.len() - 4..],
            ["--", "/bin/zsh", "-fc", "exec 2>&1\nprintf fixture"].map(OsString::from)
        );
        assert_eq!(
            arguments(&probe).last(),
            Some(&OsString::from("/usr/bin/true"))
        );
        assert_eq!(
            std::fs::read_dir(temporary.path())
                .expect("temp entries")
                .count(),
            0
        );
    }
    temporary.cleanup().expect("temp cleanup");
    project.cleanup().expect("project cleanup");
}

#[tokio::test]
async fn restricted_launch_rejects_unrepresentable_paths_and_foreign_temp_before_spawn() {
    let project = TempRoot::create().expect("project fixture");
    let temporary = TempRoot::create().expect("command temp fixture");
    let other_temp = TempRoot::create().expect("other temp fixture");
    let bad_root = project.path().join("quoted-\"root");
    assert!(
        SandboxConfig::new(
            project.path(),
            BashLaunchPolicy::ReadOnly,
            &temporary,
            Some(&bad_root)
        )
        .is_err()
    );
    assert!(
        SandboxConfig::new(
            project.path(),
            BashLaunchPolicy::FullAccess,
            &temporary,
            None
        )
        .is_err()
    );
    let sandbox = SandboxConfig::new(project.path(), BashLaunchPolicy::ReadOnly, &temporary, None)
        .expect("sandbox");
    assert!(sandbox.self_test_command(&other_temp).is_err());
    assert!(sandbox.shell_command("pwd", &other_temp).is_err());
    let stop = CancellationToken::new();
    stop.cancel();
    let probe = sandbox
        .self_test(&temporary, &stop, Duration::from_secs(1))
        .await;
    assert_eq!(
        probe.result.expect_err("cancelled before process").code(),
        BashErrorCode::Cancelled
    );
    assert!(probe.cleanup_safe);
    other_temp.cleanup().expect("other cleanup");
    temporary.cleanup().expect("temp cleanup");
    project.cleanup().expect("project cleanup");
}
