use super::*;
use std::os::unix::fs::symlink;
use tempfile::TempDir;
use vega_store::{Store, projects};

fn repo() -> TempDir {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir(temp.path().join(".git")).unwrap();
    std::fs::write(temp.path().join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    std::fs::write(temp.path().join(".git/config"), "").unwrap();
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
fn production_worker_db_branch_metadata_worktree_detached_and_missing() {
    let repo = repo();
    let owned = tempfile::tempdir().unwrap();
    let store = Store::open(owned.path().join("db.sqlite")).unwrap();
    store.migrate().unwrap();
    let primary = target(&store, repo.path());
    let service = ProjectBranchService::new().unwrap();
    service.request(1, vec![primary.clone()]);
    assert_eq!(completion(&service).rows[0].state.suffix(), Some("main"));
    std::fs::write(repo.path().join(".git/HEAD"), "ref: refs/heads/other\n").unwrap();
    service.request(2, vec![primary.clone()]);
    assert_eq!(completion(&service).rows[0].state.suffix(), Some("other"));
    let worktree = owned.path().join("linked");
    std::fs::create_dir(&worktree).unwrap();
    let metadata = repo
        .path()
        .canonicalize()
        .unwrap()
        .join(".git/worktrees/linked");
    std::fs::create_dir_all(&metadata).unwrap();
    std::fs::write(
        worktree.join(".git"),
        format!("gitdir: {}\n", metadata.display()),
    )
    .unwrap();
    std::fs::write(
        metadata.join("gitdir"),
        format!(
            "{}\n",
            worktree.canonicalize().unwrap().join(".git").display()
        ),
    )
    .unwrap();
    std::fs::write(metadata.join("HEAD"), "ref: refs/heads/linked\n").unwrap();
    let linked = target(&store, &worktree);
    // Owner registration is removed: reciprocal topology still authorizes linked HEAD.
    projects::remove(store.conn(), &primary.project_id).unwrap();
    service.request(3, vec![linked.clone()]);
    assert_eq!(completion(&service).rows[0].state.suffix(), Some("linked"));
    std::fs::write(metadata.join("HEAD"), format!("{}\n", "a".repeat(40))).unwrap();
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
