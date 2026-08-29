//! Zero-dependency git repository detection (A1-03).
//!
//! [`detect_git`] inspects `<path>/.git` and resolves the currently checked
//! out branch from its `HEAD` file without shelling out to git or linking any
//! git crate:
//!
//! - `.git` **directory** (normal clone): read `<path>/.git/HEAD`.
//! - `.git` **file** (linked worktree): the file holds a `gitdir: <path>`
//!   pointer; that path (resolved relative to the worktree root when
//!   relative) names the worktree's git dir whose `HEAD` decides.
//! - `HEAD = "ref: refs/heads/X"` → `Some(X)`.
//! - detached `HEAD` (a raw object id) or no `.git` at all → `None`.
//!
//! A non-git directory is still registrable as a project (tech-spec §2:
//! `git_default_branch` is nullable); detection only decides what to store.

use std::fs;
use std::path::{Path, PathBuf};

/// Detects the git repository at `path` and returns the current branch name.
///
/// Returns `None` when `path` is not inside a git repository checkout, when
/// `HEAD` is detached, or when any file cannot be read (a broken `gitdir`
/// pointer is treated as "no branch" rather than an error).
pub fn detect_git(path: &Path) -> Option<String> {
    let dot_git = path.join(".git");
    if dot_git.is_dir() {
        // 普通 clone：.git 目录下直接读 HEAD。
        return head_branch(&dot_git);
    }
    if dot_git.is_file() {
        // linked worktree：.git 是文本指针文件（"gitdir: <path>"）。
        let pointer = fs::read_to_string(&dot_git).ok()?;
        let gitdir = pointer.strip_prefix("gitdir:")?.trim();
        let gitdir = PathBuf::from(gitdir);
        // 相对路径以 worktree 根目录（.git 文件所在目录）为基准解析。
        let gitdir = if gitdir.is_absolute() {
            gitdir
        } else {
            path.join(gitdir)
        };
        return head_branch(&gitdir);
    }
    None
}

/// Reads `<gitdir>/HEAD` and maps it to the checked out branch name.
///
/// `"ref: refs/heads/X"` → `Some(X)`; a detached `HEAD` (raw object id) or a
/// ref outside `refs/heads/` (e.g. a tag) is not a branch → `None`.
fn head_branch(gitdir: &Path) -> Option<String> {
    let head = fs::read_to_string(gitdir.join("HEAD")).ok()?;
    let target = head.trim().strip_prefix("ref: ")?.trim();
    target.strip_prefix("refs/heads/").map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::detect_git;
    use std::fs;
    use tempfile::tempdir;

    /// Writes `contents` to `dir/.git/HEAD`, creating the `.git` directory.
    fn write_head(repo: &std::path::Path, contents: &str) {
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::write(repo.join(".git/HEAD"), contents).unwrap();
    }

    #[test]
    fn repo_with_checked_out_branch_reports_the_branch() {
        let dir = tempdir().unwrap();
        write_head(dir.path(), "ref: refs/heads/main\n");
        assert_eq!(detect_git(dir.path()), Some("main".to_string()));

        // 分支名可以带斜杠（如 feature/xxx）。
        write_head(dir.path(), "ref: refs/heads/feature/detect\n");
        assert_eq!(detect_git(dir.path()), Some("feature/detect".to_string()));
    }

    #[test]
    fn worktree_pointer_file_is_followed_to_its_head() {
        let dir = tempdir().unwrap();
        let main_repo = dir.path().join("main");
        let worktree = dir.path().join("wt");
        fs::create_dir_all(main_repo.join(".git/worktrees/wt")).unwrap();
        fs::create_dir_all(&worktree).unwrap();

        // 绝对路径指针。
        fs::write(
            worktree.join(".git"),
            format!(
                "gitdir: {}\n",
                main_repo.join(".git/worktrees/wt").display()
            ),
        )
        .unwrap();
        fs::write(
            main_repo.join(".git/worktrees/wt/HEAD"),
            "ref: refs/heads/feature-x\n",
        )
        .unwrap();
        assert_eq!(
            detect_git(&worktree),
            Some("feature-x".to_string()),
            "absolute gitdir pointer"
        );

        // 相对路径指针（相对 worktree 根目录解析）。
        let worktree_rel = dir.path().join("wt-rel");
        fs::create_dir_all(&worktree_rel).unwrap();
        fs::write(
            worktree_rel.join(".git"),
            "gitdir: ../main/.git/worktrees/wt\n",
        )
        .unwrap();
        assert_eq!(
            detect_git(&worktree_rel),
            Some("feature-x".to_string()),
            "relative gitdir pointer"
        );
    }

    #[test]
    fn detached_head_reports_no_branch() {
        let dir = tempdir().unwrap();
        // detached：HEAD 直接写对象 id。
        write_head(dir.path(), "3f9c1b2a4d5e6f708192a3b4c5d6e7f8091a2b3c\n");
        assert_eq!(detect_git(dir.path()), None);

        // ref 指向 heads 之外（如 tag）也不算分支。
        write_head(dir.path(), "ref: refs/tags/v1.0\n");
        assert_eq!(detect_git(dir.path()), None);
    }

    #[test]
    fn plain_directory_reports_nothing() {
        let dir = tempdir().unwrap();
        assert_eq!(detect_git(dir.path()), None);

        // 完全不存在的路径同样返回 None（不报错、不 panic）。
        let missing = dir.path().join("missing");
        assert_eq!(detect_git(&missing), None);
    }
}
