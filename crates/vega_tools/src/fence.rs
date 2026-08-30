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

use std::fs::{self, Metadata};
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};

use crate::error::{MutationErrorCode, ToolError};

const MAX_GIT_POINTER_BYTES: usize = 8 * 1024;

/// A mutation path after lexical, symlink, hardlink, file-type, and git
/// boundary checks.
pub(crate) struct MutationTarget {
    pub(crate) relative: PathBuf,
    pub(crate) display: String,
    pub(crate) absolute: PathBuf,
    pub(crate) metadata: Option<Metadata>,
}

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

/// Normalize a UTF-8 project-relative mutation path. `.` segments are
/// removed; roots, empty/root paths, parent traversal, and `.git` are denied.
pub(crate) fn normalize_mutation_path(input: &str) -> Result<(PathBuf, String), MutationErrorCode> {
    let path = Path::new(input);
    if path.is_absolute() || path.has_root() {
        return Err(MutationErrorCode::PathAbsolute);
    }

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => {
                if value == ".git" {
                    return Err(MutationErrorCode::PathGit);
                }
                normalized.push(value);
            }
            Component::CurDir => {}
            Component::ParentDir => return Err(MutationErrorCode::PathParent),
            Component::RootDir | Component::Prefix(_) => {
                return Err(MutationErrorCode::PathAbsolute);
            }
        }
    }
    if normalized.as_os_str().is_empty() {
        return Err(MutationErrorCode::PathRoot);
    }
    let Some(display) = normalized.to_str() else {
        return Err(MutationErrorCode::CodecInvalid);
    };
    Ok((normalized.clone(), display.to_string()))
}

/// Strict wire paths must already be in normalized form and may not address
/// any git control component.
pub(crate) fn validate_wire_path(input: &str) -> Result<(), MutationErrorCode> {
    let (_, normalized) = normalize_mutation_path(input)?;
    if normalized != input {
        return Err(MutationErrorCode::CodecInvalid);
    }
    Ok(())
}

/// Resolve a path for direct mutation. Existing symlink segments are denied,
/// a missing component is permitted only for the final target, and existing
/// targets must be single-linked regular files.
pub(crate) fn resolve_mutation_target(
    root: &Path,
    git_dir: Option<&Path>,
    input: &str,
    require_existing: bool,
) -> Result<MutationTarget, MutationErrorCode> {
    let (relative, display) = normalize_mutation_path(input)?;
    let components: Vec<_> = relative.components().collect();
    let mut current = root.to_path_buf();

    for (index, component) in components.iter().enumerate() {
        current.push(component.as_os_str());
        let final_component = index + 1 == components.len();
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(MutationErrorCode::PathSymlink);
                }
                if !final_component && !metadata.is_dir() {
                    return Err(MutationErrorCode::ParentNotFound);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && final_component => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(MutationErrorCode::ParentNotFound);
            }
            Err(_) => return Err(MutationErrorCode::FilesystemError),
        }
    }

    let absolute = root.join(&relative);
    let metadata = match fs::symlink_metadata(&absolute) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(MutationErrorCode::PathSymlink);
            }
            if !metadata.is_file() {
                return Err(MutationErrorCode::PathNotFile);
            }
            if metadata.nlink() > 1 {
                return Err(MutationErrorCode::PathHardlink);
            }
            let canonical = absolute
                .canonicalize()
                .map_err(|_| MutationErrorCode::FilesystemError)?;
            if !canonical.starts_with(root) {
                return Err(MutationErrorCode::PathSymlink);
            }
            if git_dir.is_some_and(|directory| canonical.starts_with(directory)) {
                return Err(MutationErrorCode::PathGit);
            }
            Some(metadata)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if require_existing {
                return Err(MutationErrorCode::TargetNotFound);
            }
            let Some(parent) = absolute.parent() else {
                return Err(MutationErrorCode::ParentNotFound);
            };
            let canonical_parent = parent
                .canonicalize()
                .map_err(|_| MutationErrorCode::ParentNotFound)?;
            if !canonical_parent.starts_with(root) {
                return Err(MutationErrorCode::PathSymlink);
            }
            if git_dir.is_some_and(|directory| canonical_parent.starts_with(directory)) {
                return Err(MutationErrorCode::PathGit);
            }
            None
        }
        Err(_) => return Err(MutationErrorCode::FilesystemError),
    };

    Ok(MutationTarget {
        relative,
        display,
        absolute,
        metadata,
    })
}

/// Discover the project's real git directory, including a worktree `.git`
/// pointer. Invalid pointers fail closed as a git boundary error.
pub(crate) fn discover_git_dir(root: &Path) -> Result<Option<PathBuf>, MutationErrorCode> {
    let dot_git = root.join(".git");
    let metadata = match fs::symlink_metadata(&dot_git) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(MutationErrorCode::PathGit),
    };
    if metadata.file_type().is_symlink() {
        return Err(MutationErrorCode::PathGit);
    }
    if metadata.is_dir() {
        return dot_git
            .canonicalize()
            .map(Some)
            .map_err(|_| MutationErrorCode::PathGit);
    }
    if !metadata.is_file() {
        return Err(MutationErrorCode::PathGit);
    }

    let file = fs::File::open(&dot_git).map_err(|_| MutationErrorCode::PathGit)?;
    let mut bytes = Vec::new();
    file.take((MAX_GIT_POINTER_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| MutationErrorCode::PathGit)?;
    if bytes.len() > MAX_GIT_POINTER_BYTES {
        return Err(MutationErrorCode::PathGit);
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| MutationErrorCode::PathGit)?;
    let Some(pointer) = text.trim().strip_prefix("gitdir:") else {
        return Err(MutationErrorCode::PathGit);
    };
    let pointer = pointer.trim();
    if pointer.is_empty() {
        return Err(MutationErrorCode::PathGit);
    }
    let path = Path::new(pointer);
    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    candidate
        .canonicalize()
        .map(Some)
        .map_err(|_| MutationErrorCode::PathGit)
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
