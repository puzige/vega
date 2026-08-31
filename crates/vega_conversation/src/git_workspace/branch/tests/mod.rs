use super::*;
use std::os::unix::fs::PermissionsExt;
mod codec_limits;
mod lease_cleanup;
mod mutation_runner;
mod snapshot_ids;
mod state_guards;
mod switch_e2e;

struct Repo(tempfile::TempDir);

impl Repo {
    fn new() -> Self {
        let directory = tempfile::Builder::new()
            .prefix("vega-branch-")
            .tempdir()
            .expect("create temp repo");
        git(directory.path(), &["init", "-q", "-b", "main"]);
        git(
            directory.path(),
            &["config", "user.email", "vega@example.invalid"],
        );
        git(directory.path(), &["config", "user.name", "Vega Test"]);
        fs::write(directory.path().join("README.md"), "main\n").expect("fixture write");
        git(directory.path(), &["add", "README.md"]);
        git(directory.path(), &["commit", "-q", "-m", "initial"]);
        Self(directory)
    }

    fn path(&self) -> &Path {
        self.0.path()
    }
}

fn git(root: &Path, args: &[&str]) {
    let mut command = Command::new(GIT);
    command.current_dir(root).args(args);
    scrub_git_environment(&mut command);
    command
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null");
    assert!(
        command.status().expect("git fixture").success(),
        "git {args:?}"
    );
}

fn git_output(root: &Path, args: &[&str]) -> Vec<u8> {
    let mut command = Command::new(GIT);
    command.current_dir(root).args(args);
    scrub_git_environment(&mut command);
    command
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null");
    let output = command.output().expect("git fixture output");
    assert!(output.status.success(), "git {args:?}");
    output.stdout
}

fn fake_runner(repo: &Repo, name: &str, body: &str) -> Runner {
    let script = repo.path().join(name);
    fs::write(&script, format!("#!/bin/sh\n{body}\n")).expect("fake git script");
    let mut permissions = fs::metadata(&script).expect("metadata").permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&script, permissions).expect("chmod");
    let root = fs::canonicalize(repo.path()).expect("canonical root");
    let metadata = fs::metadata(&root).expect("root metadata");
    Runner::new(
        root,
        RootIdentity {
            dev: metadata.dev(),
            ino: metadata.ino(),
        },
        Some(script),
    )
}

fn branch_id(snapshot: &BranchSnapshot, label: &str) -> BranchId {
    snapshot
        .branches
        .iter()
        .find(|branch| branch.label == label)
        .expect("fixture branch")
        .id
}

fn error_code<T>(result: Result<T, GitWorkspaceError>) -> GitWorkspaceErrorCode {
    match result {
        Ok(_) => panic!("expected failure"),
        Err(failure) => failure.code(),
    }
}
