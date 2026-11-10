use super::*;
use std::process::Command;

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args([
            "-c",
            "user.name=Vega Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[tokio::test]
async fn prepared_file_count_follows_index_changes() {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("vega-907-staged-{}-{stamp}", std::process::id()));
    fs::create_dir(&root).unwrap();
    git(&root, &["init", "-q"]);
    for name in [
        "changed",
        "untouched",
        "removed",
        "old-name",
        "other-worktree",
    ] {
        fs::write(root.join(name), format!("{name} baseline\n")).unwrap();
    }
    git(&root, &["add", "."]);
    git(&root, &["commit", "-qm", "fixture"]);
    fs::write(root.join("changed"), "changed once\n").unwrap();
    let workspace = Arc::new(GitWorkspaceService::new(&root).unwrap());
    let service = TrustedGitService::new(&root, workspace.clone()).unwrap();
    workspace.refresh(CancellationToken::new()).await.unwrap();
    let first = service
        .open_checklist(CancellationToken::new())
        .await
        .unwrap();
    assert!(first.staged.is_empty());
    assert_eq!(first.optional.len(), 1);
    let result = service
        .prepare(
            first.id,
            vec![first.optional[0].file_id],
            CancellationToken::new(),
        )
        .await;
    assert_eq!(
        result
            .prepared
            .expect("one selected worktree file")
            .staged_file_count,
        1
    );
    assert_eq!(
        git(&root, &["diff", "--cached", "--name-only"]),
        "changed\n"
    );

    fs::write(root.join("changed"), "extra unstaged edit\n").unwrap();
    fs::write(root.join("other-worktree"), "unstaged only\n").unwrap();
    fs::write(root.join("untracked"), "ignored by prepare\n").unwrap();
    git(&root, &["rm", "-q", "removed"]);
    git(&root, &["mv", "old-name", "new-name"]);
    fs::write(root.join("added"), "new staged file\n").unwrap();
    git(&root, &["add", "--", "added"]);
    workspace.refresh(CancellationToken::new()).await.unwrap();
    let next = service
        .open_checklist(CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(next.staged.len(), 4);
    assert_eq!(next.optional.len(), 3);
    let result = service
        .prepare(next.id, vec![], CancellationToken::new())
        .await;
    assert_eq!(
        result
            .prepared
            .expect("mixed staged changes")
            .staged_file_count,
        4
    );
    assert_eq!(
        git(
            &root,
            &["diff", "--cached", "--name-only", "--find-renames"]
        ),
        "added\nchanged\nnew-name\nremoved\n"
    );
    fs::remove_dir_all(root).unwrap();
}
