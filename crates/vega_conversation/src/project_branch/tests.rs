use super::*;
use std::os::unix::fs::symlink;
use tempfile::TempDir;
use vega_store::{Store, projects};

fn git(root: &Path, args: &[&str]) {
    let output = std::process::Command::new("/usr/bin/git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "owned Git command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
fn repo() -> TempDir {
    let temp = tempfile::tempdir().unwrap();
    git(temp.path(), &["init", "-b", "main"]);
    git(
        temp.path(),
        &[
            "-c",
            "user.name=Vega Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "initial",
        ],
    );
    temp
}
fn target(store: &Store, path: &Path) -> ProjectBranchTarget {
    let path = path.canonicalize().unwrap().to_str().unwrap().to_string();
    let project = projects::create(store.conn(), &path, "owned", Some("cached-wrong")).unwrap();
    ProjectBranchTarget {
        project_id: project.id,
        registered_path: project.path,
    }
}
fn completion(service: &ProjectBranchService) -> ProjectBranchCompletion {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(result) = service.take_completion() {
            return result;
        }
        assert!(Instant::now() < deadline, "bounded worker completion");
        std::thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn production_worker_real_db_checkout_worktree_detached_and_missing() {
    let repo = repo();
    let owned = tempfile::tempdir().unwrap();
    let store = Store::open(owned.path().join("db.sqlite")).unwrap();
    store.migrate().unwrap();
    let primary = target(&store, repo.path());
    let service = ProjectBranchService::new().unwrap();
    service.request(1, vec![primary.clone()]);
    assert_eq!(completion(&service).rows[0].state.suffix(), Some("main"));
    git(repo.path(), &["checkout", "-b", "other"]);
    service.request(2, vec![primary.clone()]);
    assert_eq!(completion(&service).rows[0].state.suffix(), Some("other"));
    let worktree = owned.path().join("linked");
    git(
        repo.path(),
        &[
            "worktree",
            "add",
            "-b",
            "linked",
            worktree.to_str().unwrap(),
        ],
    );
    let linked = target(&store, &worktree);
    // Owner registration is removed: reciprocal topology still authorizes linked HEAD.
    projects::remove(store.conn(), &primary.project_id).unwrap();
    service.request(3, vec![linked.clone()]);
    assert_eq!(completion(&service).rows[0].state.suffix(), Some("linked"));
    git(&worktree, &["checkout", "--detach"]);
    service.request(4, vec![linked.clone()]);
    assert_eq!(
        completion(&service).rows[0].state,
        ProjectBranchState::Detached
    );
    let pointer = std::fs::read_to_string(worktree.join(".git")).unwrap();
    let metadata = Path::new(pointer.trim().strip_prefix("gitdir: ").unwrap());
    let backlink = std::fs::read(metadata.join("gitdir")).unwrap();
    std::fs::write(metadata.join("gitdir"), "/unrelated/.git\n").unwrap();
    service.request(5, vec![linked.clone()]);
    assert_eq!(
        completion(&service).rows[0].state,
        ProjectBranchState::Unknown
    );
    std::fs::write(metadata.join("gitdir"), backlink).unwrap();
    std::fs::remove_file(worktree.join(".git")).unwrap();
    service.request(6, vec![linked.clone()]);
    assert_eq!(
        completion(&service).rows[0].state,
        ProjectBranchState::NonGit
    );
    std::fs::remove_dir(&worktree).unwrap();
    service.request(7, vec![linked]);
    assert_eq!(
        completion(&service).rows[0].state,
        ProjectBranchState::Unknown
    );
}
#[test]
fn metadata_fences_reject_external_pointer_symlink_fifo_oversize_and_bad_head() {
    let repo = repo();
    let root = repo.path().canonicalize().unwrap();
    let gitdir = root.join(".git");
    let original = std::fs::read(gitdir.join("HEAD")).unwrap();
    std::fs::remove_file(gitdir.join("HEAD")).unwrap();
    symlink("config", gitdir.join("HEAD")).unwrap();
    assert!(read_branch(&root, &|| true).is_err());
    std::fs::remove_file(gitdir.join("HEAD")).unwrap();
    let fifo = CString::new(gitdir.join("HEAD").as_os_str().as_bytes()).unwrap();
    // SAFETY: valid owned path, no aliases, bounded test-only FIFO construction.
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    assert!(read_branch(&root, &|| true).is_err());
    std::fs::remove_file(gitdir.join("HEAD")).unwrap();
    std::fs::write(gitdir.join("HEAD"), vec![b'a'; 4097]).unwrap();
    assert!(read_branch(&root, &|| true).is_err());
    std::fs::write(gitdir.join("HEAD"), &original).unwrap();
    let other = tempfile::tempdir().unwrap();
    let other_root = other.path().canonicalize().unwrap();
    std::fs::write(
        other_root.join(".git"),
        format!("gitdir: {}\n", gitdir.display()),
    )
    .unwrap();
    assert!(read_branch(&other_root, &|| true).is_err());
    for raw in [
        "ref: refs/heads/a\nb\n",
        "ref: refs/heads/a.lock\n",
        "ref: refs/heads/a..b\n",
    ] {
        assert!(parse_head(raw).is_err());
    }
    assert!(read_branch(&root, &|| false).is_err());
}
#[test]
fn cancelled_and_superseded_batches_never_publish_stale_completion() {
    let repo = repo();
    let target = ProjectBranchTarget {
        project_id: "owned".into(),
        registered_path: repo.path().canonicalize().unwrap().to_str().unwrap().into(),
    };
    let service = ProjectBranchService::new().unwrap();
    service.request(1, vec![target.clone(); 128]);
    service.invalidate(2);
    service.request(3, vec![target]);
    let result = completion(&service);
    assert_eq!(result.generation, 3);
    assert_eq!(result.rows.len(), 1);
    let shared = service.shared.clone();
    drop(service);
    assert!(shared.cancel.is_cancelled());
    std::thread::sleep(Duration::from_millis(10));
    assert!(shared.mailbox.lock().unwrap().completion.is_none());
}
