use super::*;

#[test]
fn trusted_switch_argv_is_exact_and_read_limits_remain_frozen() {
    let repo = Repo::new();
    let script = repo.path().join("fake-git.sh");
    fs::write(
        &script,
        "#!/bin/sh\nprintf '%s\\0' \"$@\" > switch-argv.bin\n",
    )
    .expect("script");
    let mut permissions = fs::metadata(&script).expect("metadata").permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&script, permissions).expect("chmod");
    let canonical_root = fs::canonicalize(repo.path()).expect("canonical root");
    let metadata = fs::metadata(&canonical_root).expect("root metadata");
    let runner = Runner::new(
        canonical_root,
        RootIdentity {
            dev: metadata.dev(),
            ino: metadata.ino(),
        },
        Some(script),
    );
    runner
        .run_trusted_switch(OsStr::new("topic"), &CancellationToken::new())
        .expect("fake switch");
    let bytes = fs::read(repo.path().join("switch-argv.bin")).expect("argv");
    let actual: Vec<&[u8]> = bytes
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty())
        .collect();
    let mut expected: Vec<&[u8]> = PREFIX.iter().map(|value| value.as_bytes()).collect();
    expected.extend(
        [
            "-c",
            "core.hooksPath=/dev/null",
            "switch",
            "--no-guess",
            "--no-overwrite-ignore",
            "--no-recurse-submodules",
            "topic",
        ]
        .iter()
        .map(|value| value.as_bytes()),
    );
    assert_eq!(actual, expected);
    assert_eq!(READ_TIMEOUT, Duration::from_secs(10));
    assert_eq!(MUTATION_TIMEOUT, Duration::from_secs(120));
    assert_eq!(MUTATION_STDOUT_LIMIT, 1024 * 1024);
    assert_eq!(STDERR_LIMIT, 64 * 1024);
}

#[test]
fn target_check_attr_uses_exact_source_argv_and_literal_nul_stdin() {
    let repo = Repo::new();
    let runner = fake_runner(
        &repo,
        "authority-recorder.sh",
        "printf 'CALL\\n' >> authority-argv\nprintf '<%s>\\n' \"$@\" >> authority-argv\ncase \" $* \" in *' --diff-filter=ACMRT '*) printf 'M\\000literal path\\000';; esac\ncase \" $* \" in *' check-attr '*) /bin/cat > authority-stdin;; esac",
    );
    let current = b"0000000000000000000000000000000000000000";
    let target = b"1111111111111111111111111111111111111111";
    validate_target_changes(
        &runner,
        current,
        target,
        1024,
        target.len() + b"topic".len(),
        &CancellationToken::new(),
    )
    .expect("authority validation");
    assert_eq!(
        fs::read(repo.path().join("authority-stdin")).expect("stdin record"),
        b"literal path\0"
    );
    let argv = fs::read_to_string(repo.path().join("authority-argv")).expect("argv record");
    assert!(argv.contains(
        "<--source=1111111111111111111111111111111111111111>\n<-z>\n<--stdin>\n<--all>\n"
    ));
    assert_eq!(argv.matches("<check-attr>").count(), 1);
    assert_eq!(argv.matches("<diff>").count(), 2);
}

#[test]
fn trusted_mutation_enforces_output_caps_nonzero_and_precancel_zero_spawn() {
    let repo = Repo::new();
    let exact_stdout = fake_runner(
        &repo,
        "stdout-exact.sh",
        "/usr/bin/yes x | /usr/bin/head -c 1048576",
    );
    assert!(
        exact_stdout
            .run_trusted_switch(OsStr::new("topic"), &CancellationToken::new())
            .is_ok()
    );
    let overflow_stdout = fake_runner(
        &repo,
        "stdout-overflow.sh",
        "/usr/bin/yes x | /usr/bin/head -c 1048577",
    );
    assert_eq!(
        error_code(
            overflow_stdout.run_trusted_switch(OsStr::new("topic"), &CancellationToken::new())
        ),
        GitWorkspaceErrorCode::OutputTooLarge
    );
    let exact_stderr = fake_runner(
        &repo,
        "stderr-exact.sh",
        "/usr/bin/yes x | /usr/bin/head -c 65536 >&2",
    );
    assert!(
        exact_stderr
            .run_trusted_switch(OsStr::new("topic"), &CancellationToken::new())
            .is_ok()
    );
    let overflow_stderr = fake_runner(
        &repo,
        "stderr-overflow.sh",
        "/usr/bin/yes x | /usr/bin/head -c 65537 >&2",
    );
    assert_eq!(
        error_code(
            overflow_stderr.run_trusted_switch(OsStr::new("topic"), &CancellationToken::new())
        ),
        GitWorkspaceErrorCode::OutputTooLarge
    );
    let nonzero = fake_runner(&repo, "nonzero.sh", "exit 17");
    assert_eq!(
        error_code(nonzero.run_trusted_switch(OsStr::new("topic"), &CancellationToken::new())),
        GitWorkspaceErrorCode::GitFailed
    );
    let no_spawn = repo.path().join("no-spawn");
    let precancel = fake_runner(&repo, "precancel.sh", "printf spawned > no-spawn");
    let token = CancellationToken::new();
    token.cancel();
    assert_eq!(
        error_code(precancel.run_trusted_switch(OsStr::new("topic"), &token)),
        GitWorkspaceErrorCode::Cancelled
    );
    assert!(!no_spawn.exists());
}

#[test]
fn trusted_mutation_cancellation_reaps_process_group_descendant() {
    let repo = Repo::new();
    let runner = fake_runner(
        &repo,
        "descendant.sh",
        "/bin/sleep 30 &\nprintf '%s' \"$!\" > descendant.pid\nwait",
    );
    let token = CancellationToken::new();
    let worker_token = token.clone();
    let worker =
        std::thread::spawn(move || runner.run_trusted_switch(OsStr::new("topic"), &worker_token));
    let pid_file = repo.path().join("descendant.pid");
    for _ in 0..500 {
        if fs::read_to_string(&pid_file).is_ok_and(|value| !value.is_empty()) {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let pid = fs::read_to_string(&pid_file)
        .expect("descendant pid")
        .parse::<u32>()
        .expect("numeric pid");
    token.cancel();
    assert_eq!(
        error_code(worker.join().expect("worker join")),
        GitWorkspaceErrorCode::Cancelled
    );
    let gone = (0..100).any(|_| {
        let status = Command::new(KILL)
            .args(["-0", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("kill probe");
        if status.success() {
            std::thread::sleep(Duration::from_millis(5));
            false
        } else {
            true
        }
    });
    assert!(gone, "mutation descendant survived cancellation");
}
