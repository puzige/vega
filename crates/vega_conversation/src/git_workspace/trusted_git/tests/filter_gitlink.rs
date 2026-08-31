use super::*;

#[test]
fn head_and_ref_oid_codecs_reject_mixed_width_uppercase_and_zero() {
    let valid_40 = vec![b'a'; 40];
    let valid_64 = vec![b'b'; 64];
    assert!(valid_nonzero_oid(&valid_40, 40));
    assert!(valid_nonzero_oid(&valid_64, 64));
    for (value, width) in [
        (vec![b'a'; 39], 40),
        (vec![b'a'; 41], 40),
        (vec![b'a'; 40], 64),
        (vec![b'a'; 64], 40),
        (vec![b'A'; 40], 40),
        (vec![b'0'; 40], 40),
        (vec![b'0'; 64], 64),
    ] {
        assert!(!valid_nonzero_oid(&value, width));
        let mut refs = value;
        refs.extend_from_slice(b"\0refs/heads/master\0\n");
        assert_eq!(
            parse_ref_target(&refs, b"refs/heads/master", width),
            Err(CommitErrorCode::MalformedOutput)
        );
    }
}

#[tokio::test]
async fn capture_head_service_rejects_bad_born_oids_before_any_mutation() {
    for bad_oid in [
        "0".repeat(40),
        "0".repeat(64),
        "A".repeat(40),
        "a".repeat(39),
        "a".repeat(64),
    ] {
        let repo = Repo::new();
        fs::write(repo.path().join("tracked.txt"), "candidate\n").expect("candidate");
        let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
        let terminal = workspace
            .refresh(CancellationToken::new())
            .await
            .expect("baseline workspace");
        let read_dir = tempfile::tempdir().expect("bad head read fixture");
        let read = read_dir.path().join("git-read.sh");
        fs::write(
                &read,
                format!(
                    "#!/bin/sh\nset -eu\nfor arg in \"$@\"; do if [ \"$arg\" = status ]; then printf '# branch.oid {bad_oid}\\0# branch.head master\\0'; exit 0; fi; done\nexec /usr/bin/git \"$@\"\n"
                ),
            )
            .expect("bad head read script");
        let mut permissions = fs::metadata(&read)
            .expect("bad head read metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&read, permissions).expect("bad head read executable");
        let (mutation_dir, mutation, _argv, _input) = mutation_recorder();
        let trusted = TrustedGitService::new_with_executables_for_test(
            repo.path(),
            workspace,
            mutation,
            read,
        )
        .expect("bad head trusted service");
        assert!(
            matches!(
                trusted.open_checklist(CancellationToken::new()).await,
                Err(CommitErrorCode::MalformedOutput)
            ),
            "bad born oid length={} prefix={:?}",
            bad_oid.len(),
            bad_oid.as_bytes().first()
        );
        assert!(!mutation_dir.path().join("mutation-attempts").exists());
        assert_terminal_workspace(&trusted, &terminal);
    }
}

#[test]
fn mode_codecs_and_gitlink_union_are_closed() {
    let oid = vec![b'1'; 40];
    for mode in [b"100644", b"100755", b"120000", b"160000"] {
        assert_eq!(
            parse_stages(&stage_record(mode, &oid, b"path"), 40).map(|v| v.len()),
            Ok(1)
        );
    }
    for (mode, kind) in [
        (b"100644".as_slice(), b"blob".as_slice()),
        (b"100755".as_slice(), b"blob".as_slice()),
        (b"120000".as_slice(), b"blob".as_slice()),
        (b"160000".as_slice(), b"commit".as_slice()),
    ] {
        assert_eq!(
            parse_tree(&tree_record(mode, kind, &oid, b"path"), 40).map(|v| v.len()),
            Ok(1)
        );
    }
    for (mode, kind) in [
        (b"040000".as_slice(), b"tree".as_slice()),
        (b"100600".as_slice(), b"blob".as_slice()),
        (b"160000".as_slice(), b"blob".as_slice()),
        (b"100644".as_slice(), b"commit".as_slice()),
    ] {
        assert!(matches!(
            parse_tree(&tree_record(mode, kind, &oid, b"path"), 40),
            Err(CommitErrorCode::MalformedOutput)
        ));
    }
    for stage in [b"1", b"2", b"3"] {
        let mut record = b"100644 ".to_vec();
        record.extend_from_slice(&oid);
        record.push(b' ');
        record.extend_from_slice(stage);
        record.extend_from_slice(b"\tpath\0");
        assert!(matches!(
            parse_stages(&record, 40),
            Err(CommitErrorCode::MalformedOutput)
        ));
    }
    assert!(matches!(
        parse_tree(&tree_record(b"100644", b"blob", &[b'0'; 40], b"path"), 40),
        Err(CommitErrorCode::MalformedOutput)
    ));

    let stage = StageEntry {
        mode: b"160000".to_vec(),
        oid: oid.clone(),
        path: b"module".to_vec(),
    };
    let tree = TreeEntry {
        mode: b"160000".to_vec(),
        object_type: b"commit".to_vec(),
        oid: oid.clone(),
        path: b"module".to_vec(),
    };
    assert_eq!(
        cross_check_authority(
            &[],
            std::slice::from_ref(&stage),
            std::slice::from_ref(&tree),
        ),
        Ok(())
    );
    assert_eq!(
        cross_check_authority(&[], std::slice::from_ref(&stage), &[]),
        Err(CommitErrorCode::UnsafeRepository)
    );
    assert_eq!(
        cross_check_authority(&[], &[], std::slice::from_ref(&tree)),
        Err(CommitErrorCode::UnsafeRepository)
    );
    let mut changed_stage = stage.clone();
    changed_stage.oid = vec![b'2'; 40];
    assert_eq!(
        cross_check_authority(&[], &[changed_stage], std::slice::from_ref(&tree)),
        Err(CommitErrorCode::UnsafeRepository)
    );
    let record = StatusRecord {
        shape: StatusShape::Ordinary,
        x: b'.',
        y: b'M',
        sub: b"N...".to_vec(),
        head_mode: b"160000".to_vec(),
        index_mode: b"160000".to_vec(),
        worktree_mode: b"160000".to_vec(),
        head_oid: oid.clone(),
        index_oid: oid,
        path: b"module".to_vec(),
        previous: None,
    };
    assert_eq!(
        cross_check_authority(&[record], &[stage], &[tree]),
        Err(CommitErrorCode::UnsafeRepository)
    );
}

#[test]
fn raw_rename_copy_topology_is_exact_and_fail_closed() {
    let head = test_head(false, 40);
    let source_oid = vec![b'1'; 40];
    let other_oid = vec![b'2'; 40];
    let source_tree = tree_record(b"100644", b"blob", &source_oid, b"source.txt");
    let source_stage = stage_record(b"100644", &source_oid, b"source.txt");
    let destination = |path: &[u8]| stage_record(b"100644", &source_oid, path);
    let authority = |status: Vec<u8>, stage: Vec<u8>, tree: Vec<u8>| {
        finalize_authority(head.clone(), status, stage, tree, 1)
    };

    let mut shared_copy = status_prefix(&head);
    shared_copy.extend_from_slice(&status_rc_record(
        b'C',
        &source_oid,
        &source_oid,
        b"copy-a.txt",
        b"source.txt",
    ));
    shared_copy.extend_from_slice(&status_rc_record(
        b'C',
        &source_oid,
        &source_oid,
        b"copy-b.txt",
        b"source.txt",
    ));
    let mut shared_copy_stage = source_stage.clone();
    shared_copy_stage.extend_from_slice(&destination(b"copy-a.txt"));
    shared_copy_stage.extend_from_slice(&destination(b"copy-b.txt"));
    assert!(authority(shared_copy, shared_copy_stage, source_tree.clone()).is_ok());

    let mut rename = status_prefix(&head);
    rename.extend_from_slice(&status_rc_record(
        b'R',
        &source_oid,
        &source_oid,
        b"renamed.txt",
        b"source.txt",
    ));
    let mut retained_source = source_stage.clone();
    retained_source.extend_from_slice(&destination(b"renamed.txt"));
    assert!(matches!(
        authority(rename.clone(), retained_source, source_tree.clone()),
        Err(CommitErrorCode::MalformedOutput)
    ));

    let mut copy = status_prefix(&head);
    copy.extend_from_slice(&status_rc_record(
        b'C',
        &source_oid,
        &source_oid,
        b"copied.txt",
        b"source.txt",
    ));
    assert!(matches!(
        authority(copy, destination(b"copied.txt"), source_tree.clone()),
        Err(CommitErrorCode::MalformedOutput)
    ));

    let mut destination_exists_tree = source_tree.clone();
    destination_exists_tree.extend_from_slice(&tree_record(
        b"100644",
        b"blob",
        &other_oid,
        b"renamed.txt",
    ));
    assert!(matches!(
        authority(
            rename.clone(),
            destination(b"renamed.txt"),
            destination_exists_tree,
        ),
        Err(CommitErrorCode::MalformedOutput)
    ));

    let mut duplicate_rename = rename;
    duplicate_rename.extend_from_slice(&status_rc_record(
        b'R',
        &source_oid,
        &source_oid,
        b"renamed-again.txt",
        b"source.txt",
    ));
    let mut duplicate_destinations = destination(b"renamed.txt");
    duplicate_destinations.extend_from_slice(&destination(b"renamed-again.txt"));
    assert!(matches!(
        authority(
            duplicate_rename,
            duplicate_destinations,
            source_tree.clone(),
        ),
        Err(CommitErrorCode::MalformedOutput)
    ));

    let mut same_path = status_prefix(&head);
    same_path.extend_from_slice(&status_rc_record(
        b'R',
        &source_oid,
        &source_oid,
        b"source.txt",
        b"source.txt",
    ));
    assert!(matches!(
        authority(same_path, source_stage, source_tree),
        Err(CommitErrorCode::MalformedOutput)
    ));
}

#[test]
fn authority_combined_bytes_and_logical_paths_are_exactly_bounded() {
    let head = test_head(true, 40);
    let prefix = status_prefix(&head);
    let path_len = SNAPSHOT_LIMIT - prefix.len() - b"? \0".len();
    let mut exact = prefix.clone();
    exact.extend_from_slice(b"? ");
    exact.extend(std::iter::repeat_n(b'p', path_len));
    exact.push(0);
    let authority = finalize_authority(head.clone(), exact, Vec::new(), Vec::new(), 1)
        .expect("exact retained authority");
    assert_eq!(authority.status_raw.len(), SNAPSHOT_LIMIT);

    let mut plus_one = prefix.clone();
    plus_one.extend_from_slice(b"? ");
    plus_one.extend(std::iter::repeat_n(b'p', path_len + 1));
    plus_one.push(0);
    assert!(matches!(
        finalize_authority(head.clone(), plus_one, Vec::new(), Vec::new(), 1),
        Err(CommitErrorCode::OutputTooLarge)
    ));

    let build_paths = |count: usize| {
        let mut status = status_prefix(&head);
        for index in 0..count {
            status.extend_from_slice(format!("? path-{index:05}").as_bytes());
            status.push(0);
        }
        status
    };
    let exact_paths = finalize_authority(
        head.clone(),
        build_paths(PATH_LIMIT),
        Vec::new(),
        Vec::new(),
        1,
    )
    .expect("exact logical path authority");
    assert_eq!(
        logical_path_count(&exact_paths.records, &exact_paths.stages, &exact_paths.tree),
        Ok(PATH_LIMIT)
    );
    assert!(matches!(
        finalize_authority(
            head.clone(),
            build_paths(PATH_LIMIT + 1),
            Vec::new(),
            Vec::new(),
            1,
        ),
        Err(CommitErrorCode::OutputTooLarge)
    ));
}

#[test]
fn explicit_filter_values_are_typed_unsafe_filter() {
    let paths = vec![b"tracked.txt".to_vec()];
    for value in [b"set".as_slice(), b"unset", b"unspecified", b"driver"] {
        let mut attrs = b"tracked.txt\0filter\0".to_vec();
        attrs.extend_from_slice(value);
        attrs.push(0);
        let error = validate_filter_attrs(&paths, &attrs).expect_err("explicit filter");
        assert_eq!(error.code(), GitWorkspaceErrorCode::GitFailed);
        let mapped = if error.code() == GitWorkspaceErrorCode::GitFailed {
            CommitErrorCode::UnsafeFilter
        } else {
            map_workspace_error(error)
        };
        assert_eq!(mapped, CommitErrorCode::UnsafeFilter);
    }
}

#[tokio::test]
async fn prepare_maps_every_explicit_filter_value_to_unsafe_filter_before_add() {
    for value in ["set", "unset", "unspecified", "driver"] {
        let repo = Repo::new();
        fs::write(repo.path().join("tracked.txt"), "filter candidate\n").expect("filter candidate");
        let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
        workspace
            .refresh(CancellationToken::new())
            .await
            .expect("filter baseline workspace");
        let read_dir = tempfile::tempdir().expect("filter read fixture");
        let read = read_dir.path().join("git-read.sh");
        fs::write(
                &read,
                format!(
                    "#!/bin/sh\nset -eu\nfor arg in \"$@\"; do if [ \"$arg\" = check-attr ]; then printf 'tracked.txt\\0filter\\0{value}\\0'; exit 0; fi; done\nexec /usr/bin/git \"$@\"\n"
                ),
            )
            .expect("filter read script");
        let mut permissions = fs::metadata(&read)
            .expect("filter read metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&read, permissions).expect("filter read executable");
        let (mutation_dir, mutation, _argv, _input) = mutation_recorder();
        let attempts = mutation_dir.path().join("mutation-attempts");
        let trusted = TrustedGitService::new_with_executables_for_test(
            repo.path(),
            workspace,
            mutation,
            read,
        )
        .expect("filter trusted service");
        let checklist = trusted
            .open_checklist(CancellationToken::new())
            .await
            .expect("filter checklist");
        let completion = trusted
            .prepare(
                checklist.id,
                vec![checklist.optional[0].file_id],
                CancellationToken::new(),
            )
            .await;
        assert_eq!(completion.error, Some(CommitErrorCode::UnsafeFilter));
        assert!(completion.prepared.is_none());
        assert_terminal_workspace(
            &trusted,
            completion
                .workspace
                .as_ref()
                .expect("filter terminal workspace"),
        );
        assert!(!attempts.exists(), "explicit filter spawned add: {value}");
    }
}

#[tokio::test]
async fn selected_current_or_rename_old_gitattributes_is_zero_add_unsafe_filter() {
    let repo = Repo::new();
    fs::write(repo.path().join(".gitattributes"), "# candidate\n")
        .expect("current attributes candidate");
    let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
    workspace
        .refresh(CancellationToken::new())
        .await
        .expect("current attributes workspace");
    let (mutation_dir, mutation, _argv, _input) = mutation_recorder();
    let attempts = mutation_dir.path().join("mutation-attempts");
    let trusted = TrustedGitService::new_with_mutation_for_test(repo.path(), workspace, mutation)
        .expect("current attributes service");
    let checklist = trusted
        .open_checklist(CancellationToken::new())
        .await
        .expect("current attributes checklist");
    let selected = checklist
        .optional
        .iter()
        .find(|row| row.label == ".gitattributes")
        .expect("current attributes row")
        .file_id;
    let completion = trusted
        .prepare(checklist.id, vec![selected], CancellationToken::new())
        .await;
    assert_eq!(completion.error, Some(CommitErrorCode::UnsafeFilter));
    assert_terminal_workspace(
        &trusted,
        completion
            .workspace
            .as_ref()
            .expect("current attributes terminal"),
    );
    assert!(!attempts.exists());

    let repo = Repo::new();
    fs::write(repo.path().join(".gitattributes"), "# base\n").expect("old attributes base");
    run_git(repo.path(), &["add", ".gitattributes"]);
    run_git(repo.path(), &["commit", "-qm", "attributes base"]);
    run_git(repo.path(), &["mv", ".gitattributes", "attributes.txt"]);
    fs::write(repo.path().join("attributes.txt"), "# base\n# worktree\n")
        .expect("rename destination edit");
    let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
    workspace
        .refresh(CancellationToken::new())
        .await
        .expect("old attributes workspace");
    let (mutation_dir, mutation, _argv, _input) = mutation_recorder();
    let attempts = mutation_dir.path().join("mutation-attempts");
    let trusted = TrustedGitService::new_with_mutation_for_test(repo.path(), workspace, mutation)
        .expect("old attributes service");
    let checklist = trusted
        .open_checklist(CancellationToken::new())
        .await
        .expect("old attributes checklist");
    let selected = checklist
        .optional
        .iter()
        .find(|row| row.previous_label.as_deref() == Some(".gitattributes"))
        .expect("rename old attributes row")
        .file_id;
    let completion = trusted
        .prepare(checklist.id, vec![selected], CancellationToken::new())
        .await;
    assert_eq!(completion.error, Some(CommitErrorCode::UnsafeFilter));
    assert_terminal_workspace(
        &trusted,
        completion
            .workspace
            .as_ref()
            .expect("old attributes terminal"),
    );
    assert!(!attempts.exists());
}

#[tokio::test]
async fn attrs_drift_at_immediate_final_and_post_add_barriers_has_zero_zero_one_add() {
    for drift_call in [2_u8, 3, 4] {
        let repo = Repo::new();
        fs::write(repo.path().join("tracked.txt"), "attrs candidate\n").expect("attrs candidate");
        let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
        workspace
            .refresh(CancellationToken::new())
            .await
            .expect("attrs A workspace");
        let read_dir = tempfile::tempdir().expect("attrs read fixture");
        let read = read_dir.path().join("git-read.sh");
        let count = read_dir.path().join("attr-count");
        let quote = |path: &Path| path.to_string_lossy().replace('\'', "'\\''");
        fs::write(
                &read,
                format!(
                    "#!/bin/sh\nset -eu\nfor arg in \"$@\"; do if [ \"$arg\" = check-attr ]; then count=0; [ -e '{count}' ] && count=$(/bin/cat '{count}'); count=$((count + 1)); printf '%s' \"$count\" > '{count}'; if [ \"$count\" -eq {drift_call} ]; then printf 'tracked.txt\\0text\\0set\\0'; fi; exit 0; fi; done\nexec /usr/bin/git \"$@\"\n",
                    count = quote(&count),
                ),
            )
            .expect("attrs read script");
        let mut permissions = fs::metadata(&read)
            .expect("attrs read metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&read, permissions).expect("attrs read executable");
        let (mutation_dir, mutation, _argv, _input) = mutation_recorder();
        let attempts = mutation_dir.path().join("mutation-attempts");
        let trusted = TrustedGitService::new_with_executables_for_test(
            repo.path(),
            workspace,
            mutation,
            read,
        )
        .expect("attrs trusted service");
        let checklist = trusted
            .open_checklist(CancellationToken::new())
            .await
            .expect("attrs checklist");
        let completion = trusted
            .prepare(
                checklist.id,
                vec![checklist.optional[0].file_id],
                CancellationToken::new(),
            )
            .await;
        assert_eq!(
            completion.error,
            Some(CommitErrorCode::ChangedDuringRead),
            "attrs barrier {drift_call}"
        );
        assert!(completion.prepared.is_none());
        let terminal = completion
            .workspace
            .as_ref()
            .expect("attrs terminal workspace");
        assert_terminal_workspace(&trusted, terminal);
        if drift_call == 4 {
            assert_eq!(fs::read(&attempts).expect("post-add attempt"), b"x");
        } else {
            assert!(!attempts.exists(), "pre-add attrs drift spawned add");
        }
    }
}

#[tokio::test]
async fn real_gitlink_is_allowed_only_as_exact_clean_unchanged_union_entry() {
    fn install_gitlink(repo: &Repo) -> Vec<u8> {
        let target = run_git_output(repo.path(), &["rev-parse", "HEAD"])
            .strip_suffix(b"\n")
            .expect("gitlink target newline")
            .to_vec();
        let target_text = std::str::from_utf8(&target).expect("fixture oid");
        let cache = format!("160000,{target_text},module");
        run_git(
            repo.path(),
            &["update-index", "--add", "--cacheinfo", &cache],
        );
        run_git(repo.path(), &["commit", "-qm", "add gitlink"]);
        let module = repo.path().join("module");
        let mut clone = Command::new(GIT);
        clone
            .current_dir(repo.path())
            .args(["clone", "-q"])
            .arg(repo.path())
            .arg(&module);
        scrub_git_environment(&mut clone);
        assert!(clone.status().expect("clone gitlink worktree").success());
        run_git(&module, &["checkout", "-q", target_text]);
        target
    }

    let unchanged = Repo::new();
    install_gitlink(&unchanged);
    fs::write(unchanged.path().join("tracked.txt"), "ordinary change\n")
        .expect("ordinary alongside gitlink");
    let (_workspace, trusted) = unchanged.services().await;
    let checklist = trusted
        .open_checklist(CancellationToken::new())
        .await
        .expect("unchanged gitlink checklist");
    assert_eq!(checklist.optional.len(), 1);
    assert!(!checklist.optional[0].label.contains("module"));
    let prepared = trusted
        .prepare(
            checklist.id,
            vec![checklist.optional[0].file_id],
            CancellationToken::new(),
        )
        .await
        .prepared
        .expect("unchanged gitlink prepared");
    assert_eq!(prepared.staged_file_count, 2);

    let deleted = Repo::new();
    install_gitlink(&deleted);
    let (workspace, _clean_service) = deleted.services().await;
    run_git(
        deleted.path(),
        &["update-index", "--force-remove", "module"],
    );
    fs::remove_dir_all(deleted.path().join("module")).expect("remove deleted gitlink worktree");
    let trusted = TrustedGitService::new(deleted.path(), workspace).expect("deleted service");
    assert_eq!(
        trusted.open_checklist(CancellationToken::new()).await,
        Err(CommitErrorCode::UnsafeRepository)
    );

    let updated = Repo::new();
    install_gitlink(&updated);
    let (workspace, _clean_service) = updated.services().await;
    let other = run_git_output(updated.path(), &["rev-parse", "HEAD"]);
    let other = std::str::from_utf8(other.strip_suffix(b"\n").expect("updated oid newline"))
        .expect("updated oid");
    let cache = format!("160000,{other},module");
    run_git(
        updated.path(),
        &["update-index", "--add", "--cacheinfo", &cache],
    );
    let trusted = TrustedGitService::new(updated.path(), workspace).expect("updated service");
    assert_eq!(
        trusted.open_checklist(CancellationToken::new()).await,
        Err(CommitErrorCode::UnsafeRepository)
    );

    for mode in ["100644", "120000"] {
        let changed = Repo::new();
        install_gitlink(&changed);
        let (workspace, _clean_service) = changed.services().await;
        let blob = run_git_output(changed.path(), &["rev-parse", "HEAD:tracked.txt"]);
        let blob =
            std::str::from_utf8(blob.strip_suffix(b"\n").expect("blob newline")).expect("blob oid");
        let cache = format!("{mode},{blob},module");
        fs::remove_dir_all(changed.path().join("module"))
            .expect("remove type-changed gitlink worktree");
        run_git(
            changed.path(),
            &["update-index", "--add", "--cacheinfo", &cache],
        );
        let trusted =
            TrustedGitService::new(changed.path(), workspace).expect("type change service");
        assert_eq!(
            trusted.open_checklist(CancellationToken::new()).await,
            Err(CommitErrorCode::UnsafeRepository),
            "gitlink type change mode {mode}"
        );
    }

    let unborn = Repo::new();
    let target = install_gitlink(&unborn);
    let (workspace, _clean_service) = unborn.services().await;
    let target = std::str::from_utf8(&target).expect("unborn target oid");
    run_git(
        unborn.path(),
        &["symbolic-ref", "HEAD", "refs/heads/unborn"],
    );
    run_git(unborn.path(), &["read-tree", "--empty"]);
    fs::remove_dir_all(unborn.path().join("module")).expect("remove unborn gitlink worktree");
    let cache = format!("160000,{target},module");
    run_git(
        unborn.path(),
        &["update-index", "--add", "--cacheinfo", &cache],
    );
    let trusted = TrustedGitService::new(unborn.path(), workspace).expect("unborn service");
    assert_eq!(
        trusted.open_checklist(CancellationToken::new()).await,
        Err(CommitErrorCode::UnsafeRepository)
    );
}

#[tokio::test]
async fn empty_blob_add_worktree_delete_and_staged_empty_delete_remain_distinct() {
    let added = Repo::new();
    fs::write(added.path().join("empty.txt"), b"").expect("empty add");
    run_git(added.path(), &["add", "empty.txt"]);
    let (_workspace, trusted) = added.services().await;
    let checklist = trusted
        .open_checklist(CancellationToken::new())
        .await
        .expect("empty add checklist");
    assert_eq!(checklist.staged.len(), 1);
    assert!(checklist.optional.is_empty());
    let prepared = trusted
        .prepare(checklist.id, Vec::new(), CancellationToken::new())
        .await
        .prepared
        .expect("empty add prepared");
    assert_eq!(
        trusted
            .commit(
                prepared.id,
                "test: add empty blob".into(),
                CancellationToken::new(),
            )
            .await
            .outcome,
        CommitOutcome::Committed
    );

    let deleted = Repo::new();
    fs::write(deleted.path().join("empty.txt"), b"").expect("tracked empty");
    run_git(deleted.path(), &["add", "empty.txt"]);
    run_git(deleted.path(), &["commit", "-qm", "empty base"]);
    fs::remove_file(deleted.path().join("empty.txt")).expect("worktree empty delete");
    let (_workspace, trusted) = deleted.services().await;
    let checklist = trusted
        .open_checklist(CancellationToken::new())
        .await
        .expect("worktree empty delete checklist");
    assert!(checklist.staged.is_empty());
    assert_eq!(checklist.optional.len(), 1);
    assert_eq!(checklist.optional[0].kind, CommitSelectionKind::Deleted);

    for select_delete in [false, true] {
        let repo = Repo::new();
        fs::write(repo.path().join("tracked.txt"), b"").expect("stage empty blob");
        run_git(repo.path(), &["add", "tracked.txt"]);
        fs::remove_file(repo.path().join("tracked.txt")).expect("delete after staged empty");
        let (_workspace, trusted) = repo.services().await;
        let checklist = trusted
            .open_checklist(CancellationToken::new())
            .await
            .expect("staged empty plus delete checklist");
        assert_eq!(checklist.staged.len(), 1);
        assert_eq!(checklist.staged[0].kind, CommitSelectionKind::Modified);
        assert_eq!(checklist.optional.len(), 1);
        assert_eq!(checklist.optional[0].kind, CommitSelectionKind::Deleted);
        let selected = if select_delete {
            vec![checklist.optional[0].file_id]
        } else {
            Vec::new()
        };
        let completion = trusted
            .prepare(checklist.id, selected, CancellationToken::new())
            .await;
        let prepared = completion.prepared.expect("staged-empty prepared");
        let committed = trusted
            .commit(
                prepared.id,
                format!("test: staged empty delete selected={select_delete}"),
                CancellationToken::new(),
            )
            .await;
        assert_eq!(committed.outcome, CommitOutcome::Committed);
        let indexed = run_git_output(repo.path(), &["ls-files", "--stage", "tracked.txt"]);
        assert_eq!(indexed.is_empty(), select_delete);
    }
}

#[test]
fn logical_rename_sides_and_raw_path_codec_are_closed() {
    let synthetic = |index: usize| StatusRecord {
        shape: StatusShape::Rename,
        x: b'R',
        y: b'.',
        sub: b"N...".to_vec(),
        head_mode: b"100644".to_vec(),
        index_mode: b"100644".to_vec(),
        worktree_mode: b"100644".to_vec(),
        head_oid: vec![b'1'; 40],
        index_oid: vec![b'2'; 40],
        path: format!("new-{index:05}").into_bytes(),
        previous: Some(format!("old-{index:05}").into_bytes()),
    };
    let exact: Vec<_> = (0..PATH_LIMIT / 2).map(synthetic).collect();
    assert_eq!(logical_path_count(&exact, &[], &[]), Ok(PATH_LIMIT));
    let plus_one: Vec<_> = (0..=PATH_LIMIT / 2).map(synthetic).collect();
    assert!(logical_path_count(&plus_one, &[], &[]).expect("logical count") > PATH_LIMIT);

    for unsafe_path in [
        b"".as_slice(),
        b"/absolute",
        b"a//b",
        b"a/./b",
        b"a/../b",
        b".git/config",
        b"a/.git/config",
    ] {
        assert!(validate_relative_path(unsafe_path).is_err());
    }
    let head = test_head(true, 40);
    let mut status = status_prefix(&head);
    for path in [
        b"space name".as_slice(),
        b"tab\tname",
        b"line\nname",
        b"-leading",
        b"\xff-nonutf8",
    ] {
        status.extend_from_slice(b"? ");
        status.extend_from_slice(path);
        status.push(0);
    }
    let authority =
        finalize_authority(head, status, Vec::new(), Vec::new(), 1).expect("awkward raw authority");
    assert!(
        authority
            .records
            .windows(2)
            .all(|pair| pair[0].path < pair[1].path)
    );
    assert!(
        authority
            .records
            .iter()
            .any(|record| record.path == b"\xff-nonutf8")
    );
}

#[test]
fn unborn_three_source_add_and_conflict_table_is_fail_closed() {
    let head = test_head(true, 40);
    let blob = vec![b'1'; 40];
    let mut status = status_prefix(&head);
    status.extend_from_slice(b"1 A. N... 000000 100644 100644 ");
    status.extend(std::iter::repeat_n(b'0', 40));
    status.push(b' ');
    status.extend_from_slice(&blob);
    status.extend_from_slice(b" added.txt\0");
    let stage = stage_record(b"100644", &blob, b"added.txt");
    let authority = finalize_authority(head.clone(), status.clone(), stage.clone(), Vec::new(), 1)
        .expect("canonical unborn add");
    assert_eq!(authority.records.len(), 1);
    assert_eq!(authority.stages.len(), 1);
    assert!(authority.tree.is_empty());

    assert!(matches!(
        finalize_authority(head.clone(), status.clone(), Vec::new(), Vec::new(), 1),
        Err(CommitErrorCode::MalformedOutput)
    ));
    assert!(matches!(
        finalize_authority(head.clone(), status_prefix(&head), stage, Vec::new(), 1,),
        Err(CommitErrorCode::MalformedOutput)
    ));
    assert!(matches!(
        finalize_authority(
            head,
            status,
            stage_record(b"100755", &blob, b"added.txt"),
            tree_record(b"100644", b"blob", &blob, b"fabricated.txt"),
            1,
        ),
        Err(CommitErrorCode::MalformedOutput)
    ));
}
