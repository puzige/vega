use super::*;

#[test]
fn git_workspace_hunk_suffix_no_newline_and_line_cap_are_preserved() {
    let exact_line = "x".repeat(PATCH_LINE_LIMIT);
    let patch = format!("@@ -0,0 +1,1 @@ fn name\n+{exact_line}\n\\ No newline at end of file\n");
    let mut rows = PATCH_ROW_LIMIT;
    let section = parse_patch(DiffLayer::Unstaged, patch.as_bytes(), &mut rows).unwrap();
    let hunk = &section.hunks[0];
    assert_eq!(hunk.heading_suffix.as_deref(), Some("fn name"));
    assert!(hunk.missing_trailing_newline);
    assert_eq!(hunk.rows[0].text.len(), PATCH_LINE_LIMIT);
    let too_long = format!("@@ -0,0 +1,1 @@\n+{}\n", "x".repeat(PATCH_LINE_LIMIT + 1));
    let error = match parse_patch(DiffLayer::Unstaged, too_long.as_bytes(), &mut rows) {
        Ok(_) => panic!("oversized line was accepted"),
        Err(error) => error,
    };
    assert_eq!(error.code(), GitWorkspaceErrorCode::OutputTooLarge);
    let mut rows = PATCH_ROW_LIMIT;
    let bad_marker = b"@@ -1,1 +1,1 @@\n same\n\\ unexpected marker\n";
    let error = match parse_patch(DiffLayer::Unstaged, bad_marker, &mut rows) {
        Ok(_) => panic!("unknown backslash marker was accepted"),
        Err(error) => error,
    };
    assert_eq!(error.code(), GitWorkspaceErrorCode::MalformedOutput);
    let mut rows = PATCH_ROW_LIMIT;
    let overflow = b"@@ -4294967295,1 +1,1 @@\n same\n";
    let error = match parse_patch(DiffLayer::Unstaged, overflow, &mut rows) {
        Ok(_) => panic!("overflowing line coordinate was accepted"),
        Err(error) => error,
    };
    assert_eq!(error.code(), GitWorkspaceErrorCode::MalformedOutput);
}

#[test]
fn git_workspace_combined_patch_byte_and_row_caps_are_inclusive() {
    let mut bytes = PATCH_LIMIT;
    consume_projection_bytes(&mut bytes, PATCH_LIMIT / 2).unwrap();
    consume_projection_bytes(&mut bytes, PATCH_LIMIT - PATCH_LIMIT / 2).unwrap();
    assert_eq!(bytes, 0);
    assert_eq!(
        consume_projection_bytes(&mut bytes, 1).unwrap_err().code(),
        GitWorkspaceErrorCode::OutputTooLarge
    );

    let patch = |rows: usize| {
        let mut body = format!("@@ -0,0 +1,{rows} @@\n");
        for _ in 0..rows {
            body.push_str("+x\n");
        }
        body
    };
    let mut rows = PATCH_ROW_LIMIT;
    let staged = parse_patch(
        DiffLayer::Staged,
        patch(PATCH_ROW_LIMIT / 2).as_bytes(),
        &mut rows,
    )
    .unwrap();
    let unstaged = parse_patch(
        DiffLayer::Unstaged,
        patch(PATCH_ROW_LIMIT / 2).as_bytes(),
        &mut rows,
    )
    .unwrap();
    assert_eq!(rows, 0);
    assert_eq!(staged.hunks[0].rows.len(), PATCH_ROW_LIMIT / 2);
    assert_eq!(unstaged.hunks[0].rows.len(), PATCH_ROW_LIMIT / 2);
    let mut rows = PATCH_ROW_LIMIT;
    parse_patch(
        DiffLayer::Staged,
        patch(PATCH_ROW_LIMIT / 2).as_bytes(),
        &mut rows,
    )
    .unwrap();
    let row_error = match parse_patch(
        DiffLayer::Unstaged,
        patch(PATCH_ROW_LIMIT / 2 + 1).as_bytes(),
        &mut rows,
    ) {
        Ok(_) => panic!("combined row cap +1 was accepted"),
        Err(error) => error,
    };
    assert_eq!(row_error.code(), GitWorkspaceErrorCode::OutputTooLarge);
}

#[test]
fn git_workspace_environment_scrub_is_exact() {
    let mut command = Command::new("/usr/bin/true");
    command
        .env("GIT_DIR", "/private/leak")
        .env("GIT_CONFIG_COUNT", "9")
        .env("VEGA_KEEP", "yes");
    scrub_git_environment(&mut command);
    let env: HashMap<_, _> = command
        .get_envs()
        .map(|(key, value)| (key.to_owned(), value.map(OsStr::to_owned)))
        .collect();
    assert_eq!(env.get(OsStr::new("GIT_DIR")), Some(&None));
    assert_eq!(env.get(OsStr::new("GIT_CONFIG_COUNT")), Some(&None));
    assert_eq!(
        env.get(OsStr::new("GIT_LITERAL_PATHSPECS"))
            .and_then(|value| value.as_deref()),
        Some(OsStr::new("1"))
    );
    assert_eq!(
        env.get(OsStr::new("GIT_NO_LAZY_FETCH"))
            .and_then(|value| value.as_deref()),
        Some(OsStr::new("1"))
    );
    assert_eq!(
        env.get(OsStr::new("VEGA_KEEP"))
            .and_then(|value| value.as_deref()),
        Some(OsStr::new("yes"))
    );
}

#[tokio::test]
async fn git_workspace_runner_scrubs_git_environment_and_bounds_output() {
    let repo = Repo::new();
    let script = repo.path().join("fixture-git");
    fs::write(
            &script,
            "#!/bin/sh\nif env | grep '^GIT_CONFIG_COUNT=' >/dev/null; then exit 90; fi\nif [ \"$GIT_LITERAL_PATHSPECS\" != 1 ] || [ \"$GIT_NO_LAZY_FETCH\" != 1 ]; then exit 91; fi\npython3 -c 'import sys; sys.stdout.write(\"x\" * (8 * 1024 * 1024 + 1))'\n",
        )
        .unwrap();
    let mut permissions = fs::metadata(&script).unwrap().permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&script, permissions).unwrap();
    let service = GitWorkspaceService::new_for_test(repo.path(), script).unwrap();
    assert_eq!(
        service
            .refresh(CancellationToken::new())
            .await
            .unwrap_err()
            .code(),
        GitWorkspaceErrorCode::OutputTooLarge
    );
}

#[tokio::test]
async fn git_workspace_explicit_filter_attribute_rejects_before_driver_execution() {
    let repo = Repo::new();
    repo.write("victim.txt", b"base\n");
    repo.commit_all();
    let marker = repo.path().join("filter-ran");
    let driver = repo.path().join("filter-driver");
    fs::write(
        &driver,
        format!("#!/bin/sh\nprintf ran > '{}'\ncat\n", marker.display()),
    )
    .unwrap();
    let mut permissions = fs::metadata(&driver).unwrap().permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&driver, permissions).unwrap();
    git(
        repo.path(),
        &["config", "filter.evil.clean", &driver.to_string_lossy()],
    );
    repo.write(".gitattributes", b"*.txt filter=evil\n");
    repo.write("victim.txt", b"changed\n");

    let service = GitWorkspaceService::new(repo.path()).unwrap();
    assert_eq!(
        service
            .refresh(CancellationToken::new())
            .await
            .unwrap_err()
            .code(),
        GitWorkspaceErrorCode::GitFailed
    );
    assert!(!marker.exists(), "filter driver executed during preflight");
    assert_eq!(
        validate_filter_attrs(&[b"victim.txt".to_vec()], b"victim.txt\0filter\0unset\0")
            .unwrap_err()
            .code(),
        GitWorkspaceErrorCode::GitFailed
    );
}

#[test]
fn git_workspace_bounded_stdin_stdout_stderr_progress_concurrently() {
    let repo = Repo::new();
    let script = repo.path().join("fixture-git");
    fs::write(
            &script,
            "#!/bin/sh\nif [ \"${13}\" != check-attr ] || [ \"${14}\" != -z ] || [ \"${15}\" != --stdin ] || [ \"${16}\" != --all ]; then exit 91; fi\npython3 -c 'import sys; sys.stdout.write(\"o\" * 65536); sys.stdout.flush(); data=sys.stdin.buffer.read(); sys.stderr.write(\"e\" * 32768); sys.stderr.flush(); sys.stdout.write(str(len(data)))'\n",
        )
        .unwrap();
    let mut permissions = fs::metadata(&script).unwrap().permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&script, permissions).unwrap();
    let service = GitWorkspaceService::new_for_test(repo.path(), script.clone()).unwrap();
    let runner = Runner::new(service.root.clone(), service.identity, Some(script));
    let input = vec![b'i'; 128 * 1024];
    let output = runner
        .run_with_input(
            "check-attr",
            &[
                OsString::from("-z"),
                OsString::from("--stdin"),
                OsString::from("--all"),
            ],
            Arc::from(input),
            128 * 1024,
            &CancellationToken::new(),
        )
        .unwrap();
    assert_eq!(&output.stdout[..65_536], vec![b'o'; 65_536]);
    assert!(output.stdout.ends_with(b"131072"));
}

#[test]
fn git_workspace_stderr_cap_is_inclusive_and_plus_one_fails() {
    let repo = Repo::new();
    let script = repo.path().join("fixture-git");
    let write_fixture = |size: usize| {
        fs::write(
            &script,
            format!("#!/bin/sh\npython3 -c 'import sys; sys.stderr.write(\"e\" * {size})'\n"),
        )
        .unwrap();
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&script, permissions).unwrap();
    };
    let service = GitWorkspaceService::new_for_test(repo.path(), script.clone()).unwrap();
    let runner = Runner::new(service.root.clone(), service.identity, Some(script.clone()));
    write_fixture(STDERR_LIMIT);
    runner
        .run(
            "rev-parse",
            &[OsString::from("--show-toplevel")],
            1,
            &CancellationToken::new(),
        )
        .unwrap();
    write_fixture(STDERR_LIMIT + 1);
    let stderr_error = match runner.run(
        "rev-parse",
        &[OsString::from("--show-toplevel")],
        1,
        &CancellationToken::new(),
    ) {
        Ok(_) => panic!("stderr cap +1 was accepted"),
        Err(error) => error,
    };
    assert_eq!(stderr_error.code(), GitWorkspaceErrorCode::OutputTooLarge);
}

#[test]
fn commit_summary_exact_argv_null_stdin_and_full_dual_drain() {
    let repo = Repo::new();
    let fixture = tempdir().unwrap();
    let script = fixture.path().join("summary-git");
    let argv = fixture.path().join("argv");
    let stdin = fixture.path().join("stdin");
    let cwd = fixture.path().join("cwd");
    let env = fixture.path().join("env");
    let tail = fixture.path().join("tail");
    fs::write(
            &script,
            format!(
                "#!/bin/sh\nset -eu\n: > '{argv}'\nfor arg in \"$@\"; do printf '%s\\0' \"$arg\" >> '{argv}'; done\nwc -c > '{stdin}'\npwd > '{cwd}'\nenv | grep '^GIT_' > '{env}'\npython3 -c 'import os,sys; sys.stdout.buffer.write(b\"o\" * 131072); sys.stdout.flush(); sys.stderr.buffer.write(b\"e\" * 65536); sys.stderr.flush(); open(sys.argv[1], \"wb\").close()' '{tail}'\n",
                argv = argv.display(),
                stdin = stdin.display(),
                cwd = cwd.display(),
                env = env.display(),
                tail = tail.display(),
            ),
        )
        .unwrap();
    let mut permissions = fs::metadata(&script).unwrap().permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&script, permissions).unwrap();
    let service = GitWorkspaceService::new_for_test(repo.path(), script.clone()).unwrap();
    let runner = Runner::new(service.root.clone(), service.identity, Some(script));
    let output = runner
        .run_commit_summary(256 * 1024, &CancellationToken::new())
        .unwrap();
    assert_eq!(output.stdout, vec![b'o'; 131_072]);
    assert!(!output.overflow);
    assert!(tail.exists());
    assert_eq!(fs::read_to_string(stdin).unwrap().trim(), "0");
    assert_eq!(
        fs::read_to_string(cwd).unwrap().trim(),
        service.root.to_str().unwrap()
    );
    let mut git_environment: Vec<_> = fs::read_to_string(env)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect();
    git_environment.sort();
    assert_eq!(
        git_environment,
        [
            "GIT_LITERAL_PATHSPECS=1",
            "GIT_NO_LAZY_FETCH=1",
            "GIT_PAGER=cat",
            "GIT_TERMINAL_PROMPT=0",
        ]
    );
    let mut expected = Vec::new();
    for argument in PREFIX.iter().copied().chain([
        "-c",
        "core.quotePath=true",
        "--no-optional-locks",
        "diff",
        "--cached",
        "--patch",
        "--find-renames",
        "--no-ext-diff",
        "--no-textconv",
        "--full-index",
        "--",
    ]) {
        expected.extend_from_slice(argument.as_bytes());
        expected.push(0);
    }
    assert_eq!(fs::read(argv).unwrap(), expected);
}

#[test]
fn commit_summary_stderr_overflow_is_fully_drained_then_rejected() {
    let repo = Repo::new();
    let fixture = tempdir().unwrap();
    let script = fixture.path().join("summary-git");
    let tail = fixture.path().join("stderr-tail");
    fs::write(
            &script,
            format!(
                "#!/bin/sh\nexec python3 -c 'import sys; sys.stderr.buffer.write(b\"e\" * 196608); sys.stderr.flush(); open(sys.argv[1], \"wb\").close()' '{}'\n",
                tail.display()
            ),
        )
        .unwrap();
    let mut permissions = fs::metadata(&script).unwrap().permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&script, permissions).unwrap();
    let service = GitWorkspaceService::new_for_test(repo.path(), script.clone()).unwrap();
    let runner = Runner::new(service.root.clone(), service.identity, Some(script));
    let started = Instant::now();
    let error = match runner.run_commit_summary(256 * 1024, &CancellationToken::new()) {
        Ok(_) => panic!("stderr overflow was accepted"),
        Err(error) => error,
    };
    assert_eq!(error.code(), GitWorkspaceErrorCode::OutputTooLarge);
    assert!(
        tail.exists(),
        "producer did not reach its post-overflow tail"
    );
    assert!(started.elapsed() < READ_TIMEOUT);
}

#[test]
fn commit_summary_raw_cap_is_inclusive_and_overflow_tail_is_drained() {
    let repo = Repo::new();
    let fixture = tempdir().unwrap();
    let script = fixture.path().join("summary-git");
    let tail = fixture.path().join("stdout-tail");
    let service = GitWorkspaceService::new_for_test(repo.path(), script.clone()).unwrap();
    let runner = Runner::new(service.root.clone(), service.identity, Some(script.clone()));
    const CAP: usize = 256 * 1024;
    for (size, overflow) in [(CAP, false), (CAP + 1, true), (CAP * 3, true)] {
        let _ = fs::remove_file(&tail);
        fs::write(
                &script,
                format!(
                    "#!/bin/sh\nexec python3 -c 'import sys; sys.stdout.buffer.write(b\"x\" * {size}); sys.stdout.flush(); open(sys.argv[1], \"wb\").close()' '{}'\n",
                    tail.display()
                ),
            )
            .unwrap();
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&script, permissions).unwrap();
        let output = runner
            .run_commit_summary(CAP, &CancellationToken::new())
            .unwrap();
        assert_eq!(output.stdout, vec![b'x'; CAP.min(size)]);
        assert_eq!(output.overflow, overflow, "size {size}");
        assert!(tail.exists(), "size {size} tail was not drained");
    }
}

#[test]
fn summary_reader_chunk_partition_is_irrelevant() {
    struct Chunked {
        bytes: Vec<u8>,
        offset: usize,
        chunk: usize,
    }
    impl Read for Chunked {
        fn read(&mut self, target: &mut [u8]) -> std::io::Result<usize> {
            let available = self.bytes.len().saturating_sub(self.offset);
            let read = available.min(self.chunk).min(target.len());
            target[..read].copy_from_slice(&self.bytes[self.offset..self.offset + read]);
            self.offset += read;
            Ok(read)
        }
    }
    let bytes: Vec<u8> = (0..65_567).map(|index| (index % 251) as u8).collect();
    for chunk in [1, 4 * 1024, IO_CHUNK] {
        let (sender, receiver) = mpsc::channel();
        spawn_reader(
            Chunked {
                bytes: bytes.clone(),
                offset: 0,
                chunk,
            },
            Stream::Stdout,
            65_536,
            Arc::new(AtomicBool::new(false)),
            false,
            sender,
        );
        let output = receiver.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(output.bytes, bytes[..65_536]);
        assert!(output.overflow);
        assert!(!output.failed);
    }
}

#[test]
fn summary_reader_first_read_crosses_cap_and_still_drains_tail() {
    struct OneRead {
        bytes: Vec<u8>,
        consumed: Arc<AtomicUsize>,
    }
    impl Read for OneRead {
        fn read(&mut self, target: &mut [u8]) -> std::io::Result<usize> {
            if self.bytes.is_empty() {
                return Ok(0);
            }
            let read = self.bytes.len().min(target.len());
            target[..read].copy_from_slice(&self.bytes[..read]);
            self.bytes.drain(..read);
            self.consumed.fetch_add(read, Ordering::SeqCst);
            Ok(read)
        }
    }
    const CAP: usize = 37;
    let bytes: Vec<u8> = (0..(CAP + 211)).map(|index| (index % 251) as u8).collect();
    assert!(bytes.len() < IO_CHUNK, "fixture must cross cap in one read");
    let consumed = Arc::new(AtomicUsize::new(0));
    let (sender, receiver) = mpsc::channel();
    spawn_reader(
        OneRead {
            bytes: bytes.clone(),
            consumed: consumed.clone(),
        },
        Stream::Stdout,
        CAP,
        Arc::new(AtomicBool::new(false)),
        false,
        sender,
    );
    let output = receiver.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(output.bytes, bytes[..CAP]);
    assert!(output.overflow);
    assert_eq!(consumed.load(Ordering::SeqCst), bytes.len());
}

#[test]
fn commit_summary_deferred_overflow_never_eof_uses_bounded_timeout() {
    let repo = Repo::new();
    let fixture = tempdir().unwrap();
    let script = fixture.path().join("summary-git");
    let overflow_reached = fixture.path().join("overflow-reached");
    let descendant_pid = fixture.path().join("descendant-pid");
    fs::write(
            &script,
            format!(
                "#!/bin/sh\nset -eu\npython3 -c 'import sys; sys.stdout.buffer.write(b\"x\" * 262145); sys.stdout.flush()'\n: > '{}'\nsleep 30 &\nprintf '%s' \"$!\" > '{}'\nwait\n",
                overflow_reached.display(),
                descendant_pid.display(),
            ),
        )
        .unwrap();
    let mut permissions = fs::metadata(&script).unwrap().permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&script, permissions).unwrap();
    let service = GitWorkspaceService::new_for_test(repo.path(), script.clone()).unwrap();
    let runner = Runner::new(service.root.clone(), service.identity, Some(script));
    let started = Instant::now();
    let error = match runner.run_commit_summary_with_timeout(
        256 * 1024,
        &CancellationToken::new(),
        Duration::from_millis(750),
    ) {
        Ok(_) => panic!("never-EOF summary was accepted"),
        Err(error) => error,
    };
    assert_eq!(error.code(), GitWorkspaceErrorCode::TimedOut);
    assert!(overflow_reached.exists());
    assert!(started.elapsed() >= Duration::from_millis(700));
    assert!(started.elapsed() < Duration::from_secs(3));
    let pid = fs::read_to_string(descendant_pid).unwrap();
    assert!(
        !Command::new(KILL)
            .args(["-0", &pid])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success(),
        "summary timeout descendant survived cleanup"
    );
}
