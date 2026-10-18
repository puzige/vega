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
