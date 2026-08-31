use super::*;

#[tokio::test]
async fn artifact_preview_is_bounded_utf8_no_nul_and_path_classified() {
    let repo = Repo::new();
    repo.write("artifact.txt", b"safe preview\n");
    let (workspace, service, card) = captured_text_artifact(&repo, 10).await;
    let preview = service
        .preview(card.id, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(preview.text(), "safe preview\n");
    assert!(!format!("{preview:?}").contains("safe preview"));

    repo.write("artifact.txt", b"secret\0tail\n");
    workspace.refresh(CancellationToken::new()).await.unwrap();
    let cards = service.reconcile(CancellationToken::new()).await.unwrap();
    assert_eq!(
        service
            .preview(cards[0].id, CancellationToken::new())
            .await
            .unwrap_err()
            .code(),
        GitWorkspaceErrorCode::MetadataOnly
    );

    let oversized = vec![b'x'; PREVIEW_BYTES + 1];
    repo.write("large.txt", &oversized);
    workspace.refresh(CancellationToken::new()).await.unwrap();
    let large = service
        .capture(
            &write_call("large", "large.txt", (PREVIEW_BYTES + 1) as u64),
            &write_result("large", "large.txt", (PREVIEW_BYTES + 1) as u64, false),
            CancellationToken::new(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(large.source, ArtifactSource::WorkspaceChange);
    assert!(large.current_file_id.is_some());
    assert_eq!(
        service
            .preview(large.id, CancellationToken::new())
            .await
            .unwrap_err()
            .code(),
        GitWorkspaceErrorCode::OutputTooLarge
    );

    repo.write("unknown.svg", b"<svg>secret</svg>\n");
    workspace.refresh(CancellationToken::new()).await.unwrap();
    let svg = service
        .capture(
            &write_call("svg", "unknown.svg", 18),
            &write_result("svg", "unknown.svg", 18, false),
            CancellationToken::new(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        service
            .preview(svg.id, CancellationToken::new())
            .await
            .unwrap_err()
            .code(),
        GitWorkspaceErrorCode::MetadataOnly
    );
}

#[tokio::test]
async fn artifact_preview_public_api_exact_and_plus_one_boundaries() {
    let repo = Repo::new();
    let exact_bytes = format!("{}\n", "x".repeat(127)).repeat(8192);
    assert_eq!(exact_bytes.len(), PREVIEW_BYTES);
    let too_many_bytes = format!("{exact_bytes}x");
    let exact_lines = "line\n".repeat(PREVIEW_LINES);
    let too_many_lines = format!("{exact_lines}line");
    let exact_line = "z".repeat(PREVIEW_LINE_BYTES);
    let too_long_line = "z".repeat(PREVIEW_LINE_BYTES + 1);
    for (path, bytes) in [
        ("exact-bytes.txt", exact_bytes.as_bytes()),
        ("too-many-bytes.txt", too_many_bytes.as_bytes()),
        ("exact-lines.txt", exact_lines.as_bytes()),
        ("too-many-lines.txt", too_many_lines.as_bytes()),
        ("exact-line.txt", exact_line.as_bytes()),
        ("too-long-line.txt", too_long_line.as_bytes()),
        ("invalid-utf8.txt", b"sentinel-\xff"),
    ] {
        repo.write(path, bytes);
    }
    let workspace = refreshed_workspace(&repo).await;
    let service =
        ArtifactService::new(workspace, PROJECT_ID.to_owned(), THREAD_ID.to_owned(), 101).unwrap();
    for (index, (path, expected)) in [
        ("exact-bytes.txt", None),
        (
            "too-many-bytes.txt",
            Some(GitWorkspaceErrorCode::OutputTooLarge),
        ),
        ("exact-lines.txt", None),
        (
            "too-many-lines.txt",
            Some(GitWorkspaceErrorCode::OutputTooLarge),
        ),
        ("exact-line.txt", None),
        (
            "too-long-line.txt",
            Some(GitWorkspaceErrorCode::OutputTooLarge),
        ),
        (
            "invalid-utf8.txt",
            Some(GitWorkspaceErrorCode::MetadataOnly),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let call_id = format!("preview-{index}");
        let bytes = fs::metadata(repo.path().join(path)).unwrap().len();
        let card = service
            .capture(
                &write_call(&call_id, path, bytes),
                &write_result(&call_id, path, bytes, false),
                CancellationToken::new(),
            )
            .await
            .unwrap()
            .unwrap();
        match expected {
            None => {
                let projection = service
                    .preview(card.id, CancellationToken::new())
                    .await
                    .unwrap();
                assert_eq!(projection.text().len() as u64, bytes);
                assert!(!format!("{projection:?}").contains("sentinel"));
            }
            Some(code) => assert_eq!(
                service
                    .preview(card.id, CancellationToken::new())
                    .await
                    .unwrap_err()
                    .code(),
                code
            ),
        }
    }
}

#[tokio::test]
async fn open_in_uses_six_exact_raw_argv_forms() {
    let repo = Repo::new();
    repo.write(
        "artifact.txt",
        b"shared-1\nshared-2\nshared-3\nshared-4\nshared-5\nshared-6\nshared-7\nshared-8\nbase\n",
    );
    repo.commit_all();
    repo.write(
        "artifact.txt",
        b"shared-1\nshared-2\nshared-3\nshared-4\nshared-5\nshared-6\nshared-7\nshared-8\nagent\n",
    );
    let (workspace, base_service, card) = captured_text_artifact(&repo, 12).await;

    let raw_name = OsString::from("-awkward name\tline\n.txt");
    fs::rename(
        repo.path().join("artifact.txt"),
        repo.path().join(&raw_name),
    )
    .unwrap();
    git(repo.path(), &["add", "-A"]);
    workspace.refresh(CancellationToken::new()).await.unwrap();
    let launcher_dir = tempfile::tempdir().unwrap();
    let recording = launcher_dir.path().join("argv.bin");
    let script_body = format!(
        ": > '{}'; for arg in \"$@\"; do printf '%s\\0' \"$arg\" >> '{}'; done; exit 0",
        recording.display(),
        recording.display()
    );
    let launcher = launcher_script(launcher_dir.path(), &script_body);
    let service = ArtifactService::new_for_test(
        workspace,
        PROJECT_ID.to_owned(),
        THREAD_ID.to_owned(),
        12,
        launcher,
        Duration::from_secs(1),
    )
    .unwrap();
    {
        let mut target = service.state.lock().unwrap();
        let source = base_service.state.lock().unwrap();
        target.by_call_id.insert("call-1".to_owned(), 0);
        let original = &source.cards[0];
        target.cards.push(ArtifactRecord {
            id: ArtifactCardId {
                route_epoch: 12,
                slot: 0,
                seal: card_seal(service.instance_nonce, 12, 0),
            },
            fingerprint: original.fingerprint.clone(),
            path: original.path.clone(),
            label: original.label.clone(),
            source: original.source,
            evidence: original.evidence.clone(),
            current_file_id: original.current_file_id,
            stale_disabled: original.stale_disabled,
        });
    }
    let card_id = service.reconcile(CancellationToken::new()).await.unwrap()[0].id;
    let canonical_root = fs::canonicalize(repo.path()).unwrap();
    let absolute_target = canonical_root.join(&raw_name);
    let cases = [
        (
            OpenInTarget::VisualStudioCode,
            vec![
                b"-a".to_vec(),
                b"Visual Studio Code".to_vec(),
                absolute_target.as_os_str().as_bytes().to_vec(),
            ],
        ),
        (
            OpenInTarget::Cursor,
            vec![
                b"-a".to_vec(),
                b"Cursor".to_vec(),
                absolute_target.as_os_str().as_bytes().to_vec(),
            ],
        ),
        (
            OpenInTarget::Zed,
            vec![
                b"-a".to_vec(),
                b"Zed".to_vec(),
                absolute_target.as_os_str().as_bytes().to_vec(),
            ],
        ),
        (
            OpenInTarget::Terminal,
            vec![
                b"-a".to_vec(),
                b"Terminal".to_vec(),
                canonical_root.as_os_str().as_bytes().to_vec(),
            ],
        ),
        (
            OpenInTarget::DefaultApplication,
            vec![absolute_target.as_os_str().as_bytes().to_vec()],
        ),
        (
            OpenInTarget::RevealInFinder,
            vec![
                b"-R".to_vec(),
                absolute_target.as_os_str().as_bytes().to_vec(),
            ],
        ),
    ];
    for (target, expected) in cases {
        service
            .open_in(card_id, target, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(raw_argv(&recording), expected);
    }
    assert_eq!(service.launch_attempts(), 6);

    let non_utf8_target = canonical_root.join(OsString::from_vec(b"raw-\xff.txt".to_vec()));
    let status = Command::new(&service.launcher)
        .args(open_arguments(
            &canonical_root,
            &non_utf8_target,
            OpenInTarget::DefaultApplication,
        ))
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(
        raw_argv(&recording),
        vec![non_utf8_target.as_os_str().as_bytes().to_vec()]
    );
    assert_eq!(card.id.route_epoch, 12);
}

#[tokio::test]
async fn open_in_preflight_is_zero_attempt_and_failures_are_one_attempt() {
    let repo = Repo::new();
    repo.write("artifact.txt", b"agent\n");
    let (workspace, _base, card) = captured_text_artifact(&repo, 13).await;
    let launcher_dir = tempfile::tempdir().unwrap();
    let success_launcher = launcher_script(launcher_dir.path(), "exit 0");
    let service = ArtifactService::new_for_test(
        workspace.clone(),
        PROJECT_ID.to_owned(),
        THREAD_ID.to_owned(),
        13,
        success_launcher,
        Duration::from_secs(1),
    )
    .unwrap();
    let imported = service
        .capture(
            &write_call("call-1", "artifact.txt", 6),
            &write_result("call-1", "artifact.txt", 6, false),
            CancellationToken::new(),
        )
        .await
        .unwrap()
        .unwrap();
    repo.write("artifact.txt", b"changed\n");
    workspace.refresh(CancellationToken::new()).await.unwrap();
    assert!(
        service
            .open_in(
                imported.id,
                OpenInTarget::DefaultApplication,
                CancellationToken::new(),
            )
            .await
            .is_err()
    );
    assert_eq!(service.launch_attempts(), 0);

    let current = service.reconcile(CancellationToken::new()).await.unwrap()[0].id;
    let missing = ArtifactService::new_for_test(
        workspace.clone(),
        PROJECT_ID.to_owned(),
        THREAD_ID.to_owned(),
        14,
        launcher_dir.path().join("missing-open"),
        Duration::from_secs(1),
    )
    .unwrap();
    let missing_card = missing
        .capture(
            &write_call("missing-call", "artifact.txt", 8),
            &write_result("missing-call", "artifact.txt", 8, false),
            CancellationToken::new(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        missing
            .open_in(
                missing_card.id,
                OpenInTarget::DefaultApplication,
                CancellationToken::new(),
            )
            .await
            .unwrap_err()
            .code(),
        GitWorkspaceErrorCode::SpawnFailed
    );
    assert_eq!(missing.launch_attempts(), 1);

    let nonzero_launcher = launcher_script(launcher_dir.path(), "exit 7");
    let nonzero = ArtifactService::new_for_test(
        workspace.clone(),
        PROJECT_ID.to_owned(),
        THREAD_ID.to_owned(),
        15,
        nonzero_launcher,
        Duration::from_secs(1),
    )
    .unwrap();
    let nonzero_card = nonzero
        .capture(
            &write_call("nonzero-call", "artifact.txt", 8),
            &write_result("nonzero-call", "artifact.txt", 8, false),
            CancellationToken::new(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        nonzero
            .open_in(
                nonzero_card.id,
                OpenInTarget::DefaultApplication,
                CancellationToken::new(),
            )
            .await
            .unwrap_err()
            .code(),
        GitWorkspaceErrorCode::GitFailed
    );
    assert_eq!(nonzero.launch_attempts(), 1);

    let timeout_pids = launcher_dir.path().join("timeout-pids");
    let timeout_launcher = launcher_script(
        launcher_dir.path(),
        &format!(
            "sleep 5 & child=$!; printf '%s\\n%s\\n' \"$$\" \"$child\" > '{}'; wait \"$child\"",
            timeout_pids.display()
        ),
    );
    let timeout = ArtifactService::new_for_test(
        workspace,
        PROJECT_ID.to_owned(),
        THREAD_ID.to_owned(),
        16,
        timeout_launcher,
        Duration::from_millis(20),
    )
    .unwrap();
    let timeout_card = timeout
        .capture(
            &write_call("timeout-call", "artifact.txt", 8),
            &write_result("timeout-call", "artifact.txt", 8, false),
            CancellationToken::new(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        timeout
            .open_in(
                timeout_card.id,
                OpenInTarget::DefaultApplication,
                CancellationToken::new(),
            )
            .await
            .unwrap_err()
            .code(),
        GitWorkspaceErrorCode::TimedOut
    );
    assert_eq!(timeout.launch_attempts(), 1);
    let timeout_processes = fs::read_to_string(&timeout_pids)
        .unwrap()
        .lines()
        .map(|line| line.parse::<u32>().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(timeout_processes.len(), 2);
    for pid in timeout_processes {
        assert!(
            !pid_is_alive(pid),
            "timed-out launcher descendant {pid} survived"
        );
    }
    assert_eq!(current.route_epoch, 13);
    assert_eq!(card.id.route_epoch, 13);
}

#[tokio::test]
async fn open_in_symlink_segment_hardlink_special_and_root_swap_are_zero_attempt() {
    use std::os::unix::fs::symlink;

    let launcher_dir = tempfile::tempdir().unwrap();
    let launcher = launcher_script(launcher_dir.path(), "exit 0");

    let symlink_repo = Repo::new();
    symlink_repo.write("nested/artifact.txt", b"agent\n");
    let (_workspace, mut symlink_service, symlink_card) =
        captured_artifact_at(&symlink_repo, "nested/artifact.txt", 21).await;
    symlink_service.launcher = launcher.clone();
    let external = tempfile::tempdir().unwrap();
    fs::rename(
        symlink_repo.path().join("nested"),
        external.path().join("nested"),
    )
    .unwrap();
    symlink(
        external.path().join("nested"),
        symlink_repo.path().join("nested"),
    )
    .unwrap();
    assert!(
        symlink_service
            .open_in(
                symlink_card.id,
                OpenInTarget::DefaultApplication,
                CancellationToken::new(),
            )
            .await
            .is_err()
    );
    assert_eq!(symlink_service.launch_attempts(), 0);

    let hardlink_repo = Repo::new();
    hardlink_repo.write("artifact.txt", b"agent\n");
    let (_workspace, mut hardlink_service, hardlink_card) =
        captured_text_artifact(&hardlink_repo, 22).await;
    hardlink_service.launcher = launcher.clone();
    let hardlink_dir = tempfile::tempdir().unwrap();
    fs::hard_link(
        hardlink_repo.path().join("artifact.txt"),
        hardlink_dir.path().join("alias.txt"),
    )
    .unwrap();
    assert!(
        hardlink_service
            .open_in(
                hardlink_card.id,
                OpenInTarget::DefaultApplication,
                CancellationToken::new(),
            )
            .await
            .is_err()
    );
    assert_eq!(hardlink_service.launch_attempts(), 0);

    let special_repo = Repo::new();
    special_repo.write("artifact.txt", b"agent\n");
    let (_workspace, mut special_service, special_card) =
        captured_text_artifact(&special_repo, 23).await;
    special_service.launcher = launcher.clone();
    fs::remove_file(special_repo.path().join("artifact.txt")).unwrap();
    let status = Command::new("/usr/bin/mkfifo")
        .arg(special_repo.path().join("artifact.txt"))
        .status()
        .unwrap();
    assert!(status.success());
    assert!(
        special_service
            .open_in(
                special_card.id,
                OpenInTarget::DefaultApplication,
                CancellationToken::new(),
            )
            .await
            .is_err()
    );
    assert_eq!(special_service.launch_attempts(), 0);

    let root_repo = Repo::new();
    root_repo.write("artifact.txt", b"agent\n");
    let (_workspace, mut root_service, root_card) = captured_text_artifact(&root_repo, 24).await;
    root_service.launcher = launcher;
    let original_root = root_repo.path().to_path_buf();
    let moved_root = original_root.with_extension("moved-root");
    fs::rename(&original_root, &moved_root).unwrap();
    fs::create_dir(&original_root).unwrap();
    assert!(
        root_service
            .open_in(
                root_card.id,
                OpenInTarget::DefaultApplication,
                CancellationToken::new(),
            )
            .await
            .is_err()
    );
    assert_eq!(root_service.launch_attempts(), 0);
    fs::remove_dir(&original_root).unwrap();
    fs::rename(&moved_root, &original_root).unwrap();
}
