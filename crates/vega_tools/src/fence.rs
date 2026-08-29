//! Path fence: every tool path argument is resolved against the project
//! root and must canonicalize back inside it (tech-spec §3 red line,
//! risks #4). This is the agent-misoperation lifeline for all tools.
//!
//! Rejection order, cheapest and most deterministic first:
//!
//! 1. absolute-path injection — lexical check, no filesystem access;
//! 2. `..` traversal — lexical check, so `../missing` is still a fence
//!    rejection rather than a lookup miss;
//! 3. symlink escape — the canonicalized target must still sit under the
//!    canonicalized root.
//!
//! risks #4 acknowledges the user-space TOCTOU window that remains; the
//! OS-level containment layer (Seatbelt, S5+) sits on top of this fence and
//! is not this crate's job.

use std::path::{Component, Path, PathBuf};

use crate::error::ToolError;

/// Resolve `input` (relative to the canonicalized project `root`) to a
/// canonical path guaranteed to stay inside `root`.
///
/// Empty input resolves to the root itself. Non-existent targets surface
/// [`ToolError::NotFound`], but only after the lexical checks: an escape
/// attempt is rejected as [`ToolError::PathEscape`] even when the target
/// does not exist.
pub(crate) fn resolve_in_root(root: &Path, input: &str) -> Result<PathBuf, ToolError> {
    let relative = Path::new(input);

    // Red-line checks that never touch the filesystem.
    if relative.is_absolute() || relative.has_root() {
        return Err(ToolError::PathEscape(input.to_string()));
    }
    if relative
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        return Err(ToolError::PathEscape(input.to_string()));
    }

    let joined = root.join(relative);
    let canonical = joined.canonicalize().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => ToolError::NotFound(input.to_string()),
        _ => ToolError::Io(e),
    })?;

    // Symlink containment: after resolution the target must still be inside
    // the root. `starts_with` compares whole components, so `/root/x` does
    // not false-positive against `/root/xy`.
    if !canonical.starts_with(root) {
        return Err(ToolError::PathEscape(input.to_string()));
    }
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::tempdir;

    use super::super::Tools;
    use crate::error::ToolError;

    fn tools_at(root: &Path) -> Tools {
        Tools::new(root).unwrap()
    }

    #[test]
    fn dot_dot_traversal_is_rejected_lexically() {
        let dir = tempdir().unwrap();
        let tools = tools_at(dir.path());
        // 目标不存在也必须拒——词法检查先于文件系统访问
        for candidate in ["../outside.txt", "a/../../escape.txt", ".."] {
            assert!(matches!(
                tools.read(candidate, None, None).unwrap_err(),
                ToolError::PathEscape(_)
            ));
        }
    }

    #[test]
    fn absolute_path_injection_is_rejected() {
        let dir = tempdir().unwrap();
        let tools = tools_at(dir.path());
        for candidate in ["/etc/passwd", "/"] {
            assert!(matches!(
                tools.read(candidate, None, None).unwrap_err(),
                ToolError::PathEscape(_)
            ));
        }
    }

    #[test]
    fn symlink_pointing_outside_root_is_rejected() {
        let dir = tempdir().unwrap();
        let outside = tempdir().unwrap();
        fs::write(outside.path().join("secret.txt"), "secret").unwrap();
        fs::create_dir_all(dir.path().join("sub")).unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("secret.txt"),
            dir.path().join("sub/evil"),
        )
        .unwrap();

        let tools = tools_at(dir.path());
        assert!(matches!(
            tools.read("sub/evil", None, None).unwrap_err(),
            ToolError::PathEscape(_)
        ));
    }

    #[test]
    fn deep_relative_paths_inside_root_are_allowed() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("src/deep/lib.rs");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, "pub fn deep() {}\n").unwrap();

        let tools = tools_at(dir.path());
        let out = tools.read("src/deep/lib.rs", None, None).unwrap();
        assert_eq!(out.text, "1 | pub fn deep() {}");

        // 根内 symlink（指向根内另一文件）放行
        fs::write(dir.path().join("real.txt"), "inside\n").unwrap();
        std::os::unix::fs::symlink(dir.path().join("real.txt"), dir.path().join("alias.txt"))
            .unwrap();
        let out = tools.read("alias.txt", None, None).unwrap();
        assert_eq!(out.text, "1 | inside");
    }
}
