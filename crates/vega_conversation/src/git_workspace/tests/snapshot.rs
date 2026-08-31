use super::*;

#[test]
fn git_workspace_fixture_git_scrubs_repository_targeting_environment() {
    let repo = Repo::new();
    repo.write("isolated.txt", b"isolated\n");
    repo.commit_all();

    assert!(repo.path().join(".git").is_dir());
    for path in [
        ".vega-poison-git-dir",
        ".vega-poison-work-tree",
        ".vega-poison-index",
    ] {
        assert!(!repo.path().join(path).exists(), "poison target {path}");
    }
}

#[tokio::test]
async fn git_workspace_clean_staged_unstaged_untracked_and_structured_projection() {
    let repo = Repo::new();
    repo.write("src/lib.rs", b"one\ntwo\n");
    repo.commit_all();
    let service = GitWorkspaceService::new(repo.path()).unwrap();
    let clean = service.refresh(CancellationToken::new()).await.unwrap();
    assert!(clean.files.is_empty());

    repo.write("src/lib.rs", b"ONE\ntwo\n");
    git(repo.path(), &["add", "src/lib.rs"]);
    repo.write("src/lib.rs", b"ONE\nTWO\n");
    repo.write("new.ts", b"export const value = 1;\n");
    let snapshot = service.refresh(CancellationToken::new()).await.unwrap();
    assert_eq!(snapshot.files.len(), 2);
    let tracked = snapshot
        .files
        .iter()
        .find(|file| file.label == "src/lib.rs")
        .unwrap();
    assert_eq!(tracked.staged, WorkspaceChangeKind::Modified);
    assert_eq!(tracked.unstaged, WorkspaceChangeKind::Modified);
    assert_eq!(tracked.language, DiffLanguage::Rust);
    let projection = service
        .diff(tracked.id, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(projection.sections.len(), 2);
    assert!(
        projection
            .sections
            .iter()
            .all(|section| !section.hunks.is_empty())
    );

    let untracked = snapshot
        .files
        .iter()
        .find(|file| file.label == "new.ts")
        .unwrap();
    assert_eq!(untracked.unstaged, WorkspaceChangeKind::Untracked);
    assert_eq!(untracked.additions, WorkspaceLineCount::Unknown);
    let projection = service
        .diff(untracked.id, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(projection.sections[0].layer, DiffLayer::Untracked);
    assert_eq!(projection.sections[0].hunks[0].rows.len(), 1);
}

#[tokio::test]
async fn git_workspace_staged_and_unstaged_sections_share_row_budget() {
    let repo = Repo::new();
    repo.write("large.txt", "a\n".repeat(6_000).as_bytes());
    repo.commit_all();
    repo.write("large.txt", "b\n".repeat(6_000).as_bytes());
    git(repo.path(), &["add", "large.txt"]);
    repo.write("large.txt", "c\n".repeat(6_000).as_bytes());
    let service = GitWorkspaceService::new(repo.path()).unwrap();
    let snapshot = service.refresh(CancellationToken::new()).await.unwrap();
    assert_eq!(
        service
            .diff(snapshot.files[0].id, CancellationToken::new())
            .await
            .unwrap_err()
            .code(),
        GitWorkspaceErrorCode::OutputTooLarge
    );
}

#[tokio::test]
async fn git_workspace_delete_rename_space_and_literal_magic_names() {
    let repo = Repo::new();
    repo.write("delete.txt", b"delete-only\n");
    for path in ["old name.txt", ":(glob)**", ":!safe"] {
        repo.write(path, b"body\n");
    }
    repo.commit_all();
    fs::remove_file(repo.path().join("delete.txt")).unwrap();
    fs::rename(
        repo.path().join("old name.txt"),
        repo.path().join("new name.txt"),
    )
    .unwrap();
    repo.write(":(glob)**", b"changed glob\n");
    repo.write(":!safe", b"changed exclude\n");
    git(repo.path(), &["add", "-A"]);
    repo.write("new name.txt", b"body\nafter-rename\n");
    let service = GitWorkspaceService::new(repo.path()).unwrap();
    let snapshot = service.refresh(CancellationToken::new()).await.unwrap();
    assert!(
        snapshot
            .files
            .iter()
            .any(|file| file.staged == WorkspaceChangeKind::Deleted)
    );
    let renamed = snapshot
        .files
        .iter()
        .find(|file| file.label == "new name.txt")
        .unwrap();
    assert_eq!(renamed.previous_label.as_deref(), Some("old name.txt"));
    assert_eq!(renamed.staged, WorkspaceChangeKind::Renamed);
    assert_eq!(renamed.unstaged, WorkspaceChangeKind::Modified);
    assert_eq!(
        service
            .diff(renamed.id, CancellationToken::new())
            .await
            .unwrap()
            .sections
            .len(),
        2
    );
    for name in [":(glob)**", ":!safe"] {
        let file = snapshot
            .files
            .iter()
            .find(|file| file.label == name)
            .unwrap();
        let projection = service
            .diff(file.id, CancellationToken::new())
            .await
            .unwrap();
        assert!(!projection.sections[0].hunks.is_empty());
    }
}

#[test]
fn git_workspace_unstaged_type_two_record_is_strictly_parsed() {
    let oid = "a".repeat(40);
    let status = format!(
        "# branch.oid {oid}\0# branch.head main\0\
             2 .R N... 100644 100644 100644 {oid} {oid} R100 after.txt\0before.txt\0"
    );
    let parsed = parse_status(status.as_bytes()).unwrap();
    let renamed = parsed.files.get(b"after.txt".as_slice()).unwrap();
    assert_eq!(renamed.staged, WorkspaceChangeKind::Unchanged);
    assert_eq!(renamed.unstaged, WorkspaceChangeKind::Renamed);
    assert_eq!(
        renamed.previous_path.as_deref(),
        Some(b"before.txt".as_slice())
    );
}

#[tokio::test]
async fn git_workspace_binary_symlink_and_special_are_metadata_only() {
    let repo = Repo::new();
    repo.write("binary.bin", b"a\0b");
    symlink("binary.bin", repo.path().join("link")).unwrap();
    let service = GitWorkspaceService::new(repo.path()).unwrap();
    let snapshot = service.refresh(CancellationToken::new()).await.unwrap();
    for label in ["binary.bin", "link"] {
        let file = snapshot
            .files
            .iter()
            .find(|file| file.label == label)
            .unwrap();
        assert_eq!(
            service
                .diff(file.id, CancellationToken::new())
                .await
                .unwrap_err()
                .code(),
            GitWorkspaceErrorCode::MetadataOnly
        );
    }
}

#[tokio::test]
async fn git_workspace_tracked_staged_and_unstaged_symlinks_are_metadata_only() {
    let repo = Repo::new();
    repo.write("target.txt", b"target\n");
    repo.write("staged-link", b"regular staged\n");
    repo.write("unstaged-link", b"regular unstaged\n");
    repo.commit_all();
    fs::remove_file(repo.path().join("staged-link")).unwrap();
    fs::remove_file(repo.path().join("unstaged-link")).unwrap();
    symlink("target.txt", repo.path().join("staged-link")).unwrap();
    symlink("target.txt", repo.path().join("unstaged-link")).unwrap();
    git(repo.path(), &["add", "staged-link"]);

    let service = GitWorkspaceService::new(repo.path()).unwrap();
    let snapshot = service.refresh(CancellationToken::new()).await.unwrap();
    for label in ["staged-link", "unstaged-link"] {
        let file = snapshot
            .files
            .iter()
            .find(|file| file.label == label)
            .unwrap();
        assert_eq!(
            service
                .diff(file.id, CancellationToken::new())
                .await
                .unwrap_err()
                .code(),
            GitWorkspaceErrorCode::MetadataOnly
        );
    }
}

#[tokio::test]
async fn git_workspace_real_conflict_is_unmerged_metadata_only_without_filter_execution() {
    let repo = Repo::new();
    repo.write("conflict.txt", b"base\n");
    repo.commit_all();
    git(repo.path(), &["branch", "side"]);
    git(repo.path(), &["checkout", "-q", "side"]);
    repo.write("conflict.txt", b"side\n");
    repo.commit_all();
    git(repo.path(), &["checkout", "-q", "main"]);
    repo.write("conflict.txt", b"main\n");
    repo.commit_all();
    let merge = git_command(repo.path(), &["merge", "--no-edit", "side"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(!merge.success());
    let marker = repo.path().join("filter-ran");
    git(
        repo.path(),
        &[
            "config",
            "filter.unused.clean",
            &format!("printf ran > '{}'; cat", marker.display()),
        ],
    );
    let service = GitWorkspaceService::new(repo.path()).unwrap();
    let snapshot = service.refresh(CancellationToken::new()).await.unwrap();
    let conflicted = snapshot
        .files
        .iter()
        .find(|file| file.label == "conflict.txt")
        .unwrap();
    assert_eq!(conflicted.staged, WorkspaceChangeKind::Unmerged);
    assert_eq!(conflicted.unstaged, WorkspaceChangeKind::Unmerged);
    assert_eq!(conflicted.additions, WorkspaceLineCount::Unknown);
    assert_eq!(conflicted.deletions, WorkspaceLineCount::Unknown);
    assert_eq!(
        service
            .diff(conflicted.id, CancellationToken::new())
            .await
            .unwrap_err()
            .code(),
        GitWorkspaceErrorCode::MetadataOnly
    );
    assert!(!marker.exists());
}

#[tokio::test]
async fn git_workspace_unborn_detached_nonrepo_and_stale_ids_are_typed() {
    let repo = Repo::new();
    let service = GitWorkspaceService::new(repo.path()).unwrap();
    let unborn = service.refresh(CancellationToken::new()).await.unwrap();
    assert!(matches!(unborn.head, WorkspaceHead::Unborn { .. }));
    repo.write("a.py", b"print(1)\n");
    let first = service.refresh(CancellationToken::new()).await.unwrap();
    let stale = first.files[0].id;
    let other_service = GitWorkspaceService::new(repo.path()).unwrap();
    other_service
        .refresh(CancellationToken::new())
        .await
        .unwrap();
    repo.write("other.txt", b"other\n");
    other_service
        .refresh(CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(
        other_service
            .diff(stale, CancellationToken::new())
            .await
            .unwrap_err()
            .code(),
        GitWorkspaceErrorCode::UnknownFile
    );
    fs::remove_file(repo.path().join("other.txt")).unwrap();
    repo.write("b.txt", b"b\n");
    service.refresh(CancellationToken::new()).await.unwrap();
    assert_eq!(
        service
            .diff(stale, CancellationToken::new())
            .await
            .unwrap_err()
            .code(),
        GitWorkspaceErrorCode::StaleGeneration
    );
    repo.commit_all();
    git(repo.path(), &["checkout", "--detach", "-q"]);
    assert!(matches!(
        service
            .refresh(CancellationToken::new())
            .await
            .unwrap()
            .head,
        WorkspaceHead::Detached
    ));

    let nonrepo = tempdir().unwrap();
    let service = GitWorkspaceService::new(nonrepo.path()).unwrap();
    assert_eq!(
        service
            .refresh(CancellationToken::new())
            .await
            .unwrap_err()
            .code(),
        GitWorkspaceErrorCode::NotRepository
    );
}

#[tokio::test]
async fn git_workspace_identical_refresh_retains_generation_and_opaque_ids() {
    let repo = Repo::new();
    repo.write("stable.rs", b"fn stable() {}\n");
    let service = GitWorkspaceService::new(repo.path()).unwrap();
    let first = service.refresh(CancellationToken::new()).await.unwrap();
    let first_id = first.files[0].id;

    let second = service.refresh(CancellationToken::new()).await.unwrap();
    assert_eq!(second.generation, first.generation);
    assert_eq!(second.files[0].id, first_id);
    assert_eq!(second, first);
    assert_eq!(
        service
            .diff(first_id, CancellationToken::new())
            .await
            .unwrap()
            .file_id(),
        first_id
    );

    repo.write("stable.rs", b"fn changed() {}\n");
    let changed = service.refresh(CancellationToken::new()).await.unwrap();
    assert_ne!(changed.generation, first.generation);
    assert_ne!(changed.files[0].id, first_id);
    assert_eq!(
        service
            .diff(first_id, CancellationToken::new())
            .await
            .unwrap_err()
            .code(),
        GitWorkspaceErrorCode::StaleGeneration
    );
}

#[tokio::test]
async fn git_workspace_canonical_vec_slot_and_seal_are_lookup_authority() {
    let repo = Repo::new();
    repo.write("z-last.txt", b"z\n");
    repo.write("a-first.txt", b"a\n");
    let service = GitWorkspaceService::new(repo.path()).unwrap();
    let snapshot = service.refresh(CancellationToken::new()).await.unwrap();
    assert_eq!(snapshot.files.len(), 2);
    {
        let state = service
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        assert_eq!(state.files.len(), snapshot.files.len());
        for (slot, (public, private)) in snapshot.files.iter().zip(&state.files).enumerate() {
            assert_eq!(usize::try_from(public.id.slot).unwrap(), slot);
            assert_eq!(private.id, public.id);
            assert_eq!(escape_path(private.path.as_bytes()), public.label);
        }
    }
    let valid = snapshot.files[0].id;
    assert_eq!(
        service
            .diff(valid, CancellationToken::new())
            .await
            .unwrap()
            .file_id(),
        valid
    );
    let forged = WorkspaceFileId {
        generation: valid.generation,
        slot: valid.slot,
        seal: valid.seal ^ 1,
    };
    assert_eq!(
        service
            .diff(forged, CancellationToken::new())
            .await
            .unwrap_err()
            .code(),
        GitWorkspaceErrorCode::UnknownFile
    );
}

#[tokio::test]
async fn git_workspace_clean_unchanged_refresh_retains_generation() {
    let repo = Repo::new();
    repo.write("clean.txt", b"clean\n");
    repo.commit_all();
    let service = GitWorkspaceService::new(repo.path()).unwrap();

    let first = service.refresh(CancellationToken::new()).await.unwrap();
    let second = service.refresh(CancellationToken::new()).await.unwrap();

    assert!(first.files.is_empty());
    assert_eq!(second, first);
}

#[tokio::test]
async fn git_workspace_clean_head_only_change_rotates_generation() {
    let repo = Repo::new();
    repo.write("clean.txt", b"clean\n");
    repo.commit_all();
    let service = GitWorkspaceService::new(repo.path()).unwrap();
    let before = service.refresh(CancellationToken::new()).await.unwrap();

    git(
        repo.path(),
        &["commit", "-q", "--allow-empty", "-m", "head"],
    );
    let after = service.refresh(CancellationToken::new()).await.unwrap();

    assert!(before.files.is_empty());
    assert!(after.files.is_empty());
    assert_eq!(after.head, before.head);
    assert_eq!(after.stats, before.stats);
    assert_ne!(after.generation, before.generation);
}

#[tokio::test]
async fn git_workspace_clean_info_attributes_change_rotates_generation() {
    let repo = Repo::new();
    repo.write("clean.txt", b"clean\n");
    repo.commit_all();
    let service = GitWorkspaceService::new(repo.path()).unwrap();
    let before = service.refresh(CancellationToken::new()).await.unwrap();

    fs::write(
        repo.path().join(".git/info/attributes"),
        b"clean.txt linguist-language=Rust\n",
    )
    .unwrap();
    let after = service.refresh(CancellationToken::new()).await.unwrap();

    assert!(before.files.is_empty());
    assert!(after.files.is_empty());
    assert_eq!(after.head, before.head);
    assert_eq!(after.stats, before.stats);
    assert_ne!(after.generation, before.generation);
}

#[tokio::test]
async fn git_workspace_private_content_head_and_raw_rename_rotate_ids() {
    let repo = Repo::new();
    repo.write("tracked.txt", b"base\n");
    repo.commit_all();
    repo.write("tracked.txt", b"aaaa\n");
    let service = GitWorkspaceService::new(repo.path()).unwrap();
    let content_a = service.refresh(CancellationToken::new()).await.unwrap();
    let content_a_id = content_a.files[0].id;

    // Same path, size, classification and line statistics: ctime/private
    // file identity is still part of the equality authority.
    repo.write("tracked.txt", b"bbbb\n");
    let content_b = service.refresh(CancellationToken::new()).await.unwrap();
    assert_ne!(content_b.generation, content_a.generation);
    assert_eq!(
        service
            .diff(content_a_id, CancellationToken::new())
            .await
            .unwrap_err()
            .code(),
        GitWorkspaceErrorCode::StaleGeneration
    );

    // An empty commit changes only the captured HEAD while the safe file
    // projection remains equal.
    let before_head = content_b;
    git(
        repo.path(),
        &["commit", "-q", "--allow-empty", "-m", "head"],
    );
    let after_head = service.refresh(CancellationToken::new()).await.unwrap();
    assert_ne!(after_head.generation, before_head.generation);
    assert_eq!(after_head.files[0].label, before_head.files[0].label);
    assert_eq!(after_head.files[0].staged, before_head.files[0].staged);
    assert_eq!(after_head.files[0].unstaged, before_head.files[0].unstaged);
    assert_eq!(after_head.stats, before_head.stats);

    let old_path_id = after_head.files[0].id;
    fs::rename(
        repo.path().join("tracked.txt"),
        repo.path().join("renamed.txt"),
    )
    .unwrap();
    let renamed = service.refresh(CancellationToken::new()).await.unwrap();
    assert_ne!(renamed.generation, after_head.generation);
    assert!(renamed.files.iter().any(|file| file.label == "renamed.txt"));
    assert_eq!(
        service
            .diff(old_path_id, CancellationToken::new())
            .await
            .unwrap_err()
            .code(),
        GitWorkspaceErrorCode::StaleGeneration
    );
}

#[tokio::test]
async fn git_workspace_aba_allocates_fresh_generation_without_id_revival() {
    let repo = Repo::new();
    repo.write("aba.txt", b"state-a\n");
    let service = GitWorkspaceService::new(repo.path()).unwrap();
    let first_a = service.refresh(CancellationToken::new()).await.unwrap();
    let first_id = first_a.files[0].id;

    repo.write("aba.txt", b"state-b\n");
    let state_b = service.refresh(CancellationToken::new()).await.unwrap();
    repo.write("aba.txt", b"state-a\n");
    let second_a = service.refresh(CancellationToken::new()).await.unwrap();

    assert_ne!(state_b.generation, first_a.generation);
    assert_ne!(second_a.generation, state_b.generation);
    assert_ne!(second_a.generation, first_a.generation);
    assert_ne!(second_a.files[0].id, first_id);
    assert_eq!(
        service
            .diff(first_id, CancellationToken::new())
            .await
            .unwrap_err()
            .code(),
        GitWorkspaceErrorCode::StaleGeneration
    );
}

#[tokio::test]
async fn git_workspace_latest_failure_invalidates_ids_and_next_success_reseals() {
    let repo = Repo::new();
    repo.write("failure.txt", b"stable\n");
    let service = GitWorkspaceService::new(repo.path()).unwrap();
    let before = service.refresh(CancellationToken::new()).await.unwrap();
    let old_id = before.files[0].id;

    let cancelled = CancellationToken::new();
    cancelled.cancel();
    assert_eq!(
        service.refresh(cancelled).await.unwrap_err().code(),
        GitWorkspaceErrorCode::Cancelled
    );
    assert_eq!(
        service
            .diff(old_id, CancellationToken::new())
            .await
            .unwrap_err()
            .code(),
        GitWorkspaceErrorCode::StaleGeneration
    );

    let after = service.refresh(CancellationToken::new()).await.unwrap();
    assert_ne!(after.generation, before.generation);
    assert_ne!(after.files[0].id, old_id);
}

#[tokio::test]
async fn git_workspace_generation_allocation_failure_invalidates_current() {
    let repo = Repo::new();
    repo.write("overflow.txt", b"before\n");
    let service = GitWorkspaceService::new(repo.path()).unwrap();
    let before = service.refresh(CancellationToken::new()).await.unwrap();
    let old_id = before.files[0].id;
    service
        .state
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .next_generation = u64::MAX;
    repo.write("overflow.txt", b"after!\n");

    assert_eq!(
        service
            .refresh(CancellationToken::new())
            .await
            .unwrap_err()
            .code(),
        GitWorkspaceErrorCode::OutputTooLarge
    );
    assert_eq!(
        service
            .diff(old_id, CancellationToken::new())
            .await
            .unwrap_err()
            .code(),
        GitWorkspaceErrorCode::StaleGeneration
    );
}

#[tokio::test]
async fn git_workspace_escapes_control_bidi_and_non_utf8_paths_without_round_trip() {
    let repo = Repo::new();
    for bytes in [
        b"tab\tname.txt".to_vec(),
        b"line\nname.txt".to_vec(),
        "bidi\u{202e}name.txt".as_bytes().to_vec(),
    ] {
        fs::write(repo.path().join(OsString::from_vec(bytes)), b"body\n").unwrap();
    }
    let service = GitWorkspaceService::new(repo.path()).unwrap();
    let snapshot = service.refresh(CancellationToken::new()).await.unwrap();
    assert_eq!(snapshot.files.len(), 3);
    let labels: Vec<&str> = snapshot
        .files
        .iter()
        .map(|file| file.label.as_str())
        .collect();
    assert!(labels.iter().any(|label| label.contains("\\x09")));
    assert!(labels.iter().any(|label| label.contains("\\x0a")));
    assert!(labels.iter().any(|label| label.contains("\\xe2\\x80\\xae")));
    assert_eq!(escape_path(b"invalid-\xff.rs"), "invalid-\\xff.rs");
    assert!(
        labels
            .iter()
            .all(|label| !label.contains('\n') && !label.contains('\t'))
    );
}

#[test]
fn git_workspace_parsers_fail_closed_and_extension_map_is_frozen() {
    for malformed in [
        b"# branch.oid (initial)\0# branch.head main".as_slice(),
        b"# branch.oid (initial)\0# branch.head main\0x unknown\0".as_slice(),
        b"# branch.oid (initial)\0# branch.head main\0? ../escape\0".as_slice(),
    ] {
        let error = match parse_status(malformed) {
            Ok(_) => panic!("malformed status was accepted"),
            Err(error) => error,
        };
        assert_eq!(error.code(), GitWorkspaceErrorCode::MalformedOutput);
    }
    let error = match validate_raw(b":100644 100644 a b M file") {
        Ok(_) => panic!("malformed raw output was accepted"),
        Err(error) => error,
    };
    assert_eq!(error.code(), GitWorkspaceErrorCode::MalformedOutput);
    let mut files = BTreeMap::new();
    assert_eq!(
        merge_numstat(&mut files, b"1\t2\tfile", true)
            .unwrap_err()
            .code(),
        GitWorkspaceErrorCode::MalformedOutput
    );
    files.insert(
        b"file".to_vec(),
        ParsedFile {
            path: b"file".to_vec(),
            previous_path: None,
            staged: WorkspaceChangeKind::Modified,
            unstaged: WorkspaceChangeKind::Unchanged,
            additions: WorkspaceLineCount::Unknown,
            deletions: WorkspaceLineCount::Unknown,
            metadata_only: false,
        },
    );
    assert_eq!(
        merge_numstat(&mut files, b"-\t1\tfile\0", true)
            .unwrap_err()
            .code(),
        GitWorkspaceErrorCode::MalformedOutput
    );
    assert_eq!(
        merge_numstat(&mut files, b"1\t1\tfile\x001\t1\tfile\0", true)
            .unwrap_err()
            .code(),
        GitWorkspaceErrorCode::MalformedOutput
    );
    let oid = "a".repeat(40);
    let raw = format!(
        ":100644 100644 {oid} {oid} M\0file\0\
             :100644 100644 {oid} {oid} M\0file\0"
    );
    let duplicate_error = match validate_raw(raw.as_bytes()) {
        Ok(_) => panic!("duplicate raw path was accepted"),
        Err(error) => error,
    };
    assert_eq!(
        duplicate_error.code(),
        GitWorkspaceErrorCode::MalformedOutput
    );

    let mut unmerged_files = BTreeMap::from([(
        b"conflict".to_vec(),
        ParsedFile {
            path: b"conflict".to_vec(),
            previous_path: None,
            staged: WorkspaceChangeKind::Unchanged,
            unstaged: WorkspaceChangeKind::Unmerged,
            additions: WorkspaceLineCount::Unknown,
            deletions: WorkspaceLineCount::Unknown,
            metadata_only: false,
        },
    )]);
    let conflict_paths = merge_numstat(
        &mut unmerged_files,
        b"0\t0\tconflict\x004\t0\tconflict\0",
        false,
    )
    .unwrap();
    assert_eq!(conflict_paths, [b"conflict".to_vec(), b"conflict".to_vec()]);
    assert_eq!(
        unmerged_files[b"conflict".as_slice()].additions,
        WorkspaceLineCount::Unknown
    );
    assert_eq!(
        merge_numstat(
            &mut unmerged_files,
            b"0\t0\tconflict\x004\t0\tconflict\x001\t0\tconflict\0",
            false,
        )
        .unwrap_err()
        .code(),
        GitWorkspaceErrorCode::MalformedOutput
    );

    let conflict_raw = format!(
        ":100644 100644 {oid} {oid} U\0conflict\0\
             :100644 100644 {oid} {oid} M\0conflict\0"
    );
    assert_eq!(validate_raw(conflict_raw.as_bytes()).unwrap().len(), 2);
    let third_raw = format!("{conflict_raw}:100644 100644 {oid} {oid} M\0conflict\0");
    let third_error = match validate_raw(third_raw.as_bytes()) {
        Ok(_) => panic!("third unmerged raw record was accepted"),
        Err(error) => error,
    };
    assert_eq!(third_error.code(), GitWorkspaceErrorCode::MalformedOutput);
    for (path, expected) in [
        (b"a.rs".as_slice(), DiffLanguage::Rust),
        (b"a.ts", DiffLanguage::TypeScript),
        (b"a.tsx", DiffLanguage::Tsx),
        (b"a.js", DiffLanguage::JavaScript),
        (b"a.jsx", DiffLanguage::JavaScript),
        (b"a.mjs", DiffLanguage::JavaScript),
        (b"a.cjs", DiffLanguage::JavaScript),
        (b"a.py", DiffLanguage::Python),
        (b"a.go", DiffLanguage::Plain),
    ] {
        assert_eq!(language_for(path), expected);
    }
}

#[test]
fn git_workspace_retained_budget_and_path_caps_are_inclusive() {
    let mut budget = RetainedBudget::new(10);
    budget.charge(3).unwrap();
    assert_eq!(budget.remaining(), 7);
    budget.charge(7).unwrap();
    assert_eq!(budget.remaining(), 0);
    assert_eq!(budget.retained(), 10);
    assert_eq!(
        budget.charge(1).unwrap_err().code(),
        GitWorkspaceErrorCode::OutputTooLarge
    );

    assert_eq!(
        parse_nul_paths(b"one\0\0two\0").unwrap_err().code(),
        GitWorkspaceErrorCode::MalformedOutput
    );
    let mut paths = Vec::new();
    for index in 0..=PATH_LIMIT {
        paths.extend_from_slice(format!("path-{index}\0").as_bytes());
    }
    assert_eq!(
        parse_nul_paths(&paths).unwrap_err().code(),
        GitWorkspaceErrorCode::OutputTooLarge
    );
}

#[test]
fn git_workspace_candidate_logical_retained_private_paths_are_exactly_bounded() {
    let identity = Arc::new(SnapshotIdentity {
        filter_paths: Arc::from([]),
        filter_attrs: Vec::new(),
        status: Vec::new(),
        staged_raw: Vec::new(),
        unstaged_raw: Vec::new(),
        staged_numstat: Vec::new(),
        unstaged_numstat: Vec::new(),
    });
    let id = WorkspaceFileId {
        generation: 0,
        slot: 0,
        seal: 0,
    };
    let snapshot = WorkspaceSnapshot {
        generation: 0,
        head: WorkspaceHead::Detached,
        files: vec![WorkspaceFile {
            id,
            label: String::new(),
            previous_label: None,
            staged: WorkspaceChangeKind::Modified,
            unstaged: WorkspaceChangeKind::Unchanged,
            additions: WorkspaceLineCount::Unknown,
            deletions: WorkspaceLineCount::Unknown,
            language: DiffLanguage::Plain,
        }],
        stats: WorkspaceStats {
            file_count: 1,
            additions: WorkspaceLineCount::Unknown,
            deletions: WorkspaceLineCount::Unknown,
        },
    };
    let make_private = |current_len: usize, previous_len: usize| PrivateFile {
        id,
        path: OsString::from_vec(vec![b'p'; current_len]),
        previous_path: Some(OsString::from_vec(vec![b'o'; previous_len])),
        staged: WorkspaceChangeKind::Modified,
        unstaged: WorkspaceChangeKind::Unchanged,
        binary: false,
        metadata_only: false,
        language: DiffLanguage::Plain,
        snapshot_identity: identity.clone(),
        worktree_identity: None,
    };
    let base_private = [make_private(0, 1)];
    let base = ensure_candidate_retained(&identity, &snapshot, &base_private, usize::MAX).unwrap();
    let current_len = SNAPSHOT_LIMIT.checked_sub(base).unwrap();
    let exact_private = [make_private(current_len, 1)];
    assert_eq!(
        ensure_candidate_retained(&identity, &snapshot, &exact_private, SNAPSHOT_LIMIT).unwrap(),
        SNAPSHOT_LIMIT
    );
    let plus_one_private = [make_private(current_len, 2)];
    assert_eq!(
        ensure_candidate_retained(&identity, &snapshot, &plus_one_private, SNAPSHOT_LIMIT)
            .unwrap_err()
            .code(),
        GitWorkspaceErrorCode::OutputTooLarge
    );
}

#[test]
fn git_workspace_projection_redaction_and_service_debug_are_safe() {
    let id = WorkspaceFileId {
        generation: 1,
        slot: 2,
        seal: 3,
    };
    let projection = DiffTextProjection {
        file_id: id,
        language: DiffLanguage::Plain,
        sections: vec![DiffSection {
            layer: DiffLayer::Untracked,
            hunks: vec![DiffHunk {
                old_start: 0,
                old_count: 0,
                new_start: 1,
                new_count: 1,
                heading_suffix: None,
                missing_trailing_newline: false,
                rows: vec![DiffRow {
                    kind: DiffRowKind::Addition,
                    old_line: None,
                    new_line: Some(1),
                    text: "LEAK_SENTINEL".into(),
                }],
            }],
        }],
    };
    let debug = format!("{projection:?}");
    assert!(!debug.contains("LEAK_SENTINEL"));
    assert!(debug.contains("redacted"));
    let repo = Repo::new();
    let service = GitWorkspaceService::new(repo.path()).unwrap();
    assert!(!format!("{service:?}").contains(&repo.path().to_string_lossy().to_string()));
}
