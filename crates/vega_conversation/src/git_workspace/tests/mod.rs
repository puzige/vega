use super::*;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::sync::atomic::AtomicUsize;
use tempfile::{TempDir, tempdir};

mod caps_runner;
mod lifecycle;
mod snapshot;

struct Repo {
    dir: TempDir,
}

impl Repo {
    fn new() -> Self {
        let dir = tempdir().unwrap();
        git(dir.path(), &["init", "-q", "--initial-branch=main"]);
        git(dir.path(), &["config", "user.name", "Vega Test"]);
        git(
            dir.path(),
            &["config", "user.email", "vega@example.invalid"],
        );
        Self { dir }
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn write(&self, path: &str, body: &[u8]) {
        let path = self.path().join(path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, body).unwrap();
    }

    fn commit_all(&self) {
        git(self.path(), &["add", "-A"]);
        git(self.path(), &["commit", "-q", "-m", "fixture"]);
    }
}

fn git(root: &Path, args: &[&str]) {
    let status = git_command(root, args).status().unwrap();
    assert!(status.success(), "git {args:?}");
}

fn git_command(root: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(GIT);
    command
        .current_dir(root)
        .args(args)
        // Exercise the same repository-targeting variables inherited from
        // Git hooks on every fixture command. The scrub below must win.
        .env("GIT_DIR", root.join(".vega-poison-git-dir"))
        .env("GIT_WORK_TREE", root.join(".vega-poison-work-tree"))
        .env("GIT_INDEX_FILE", root.join(".vega-poison-index"));
    scrub_git_environment(&mut command);
    command
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null");
    command
}
