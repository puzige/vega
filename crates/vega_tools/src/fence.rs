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
                if value.as_encoded_bytes().eq_ignore_ascii_case(b".git") {
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
    if Path::new(input).is_absolute() {
        let path = Path::new(input);
        if input.contains('\0')
            || path.parent().is_none()
            || path.components().any(|c| {
                matches!(c, Component::ParentDir)
                    || c.as_os_str()
                        .as_encoded_bytes()
                        .eq_ignore_ascii_case(b".git")
            })
            || path.components().collect::<PathBuf>().to_string_lossy() != input
        {
            return Err(MutationErrorCode::CodecInvalid);
        }
    } else {
        let (_, normalized) = normalize_mutation_path(input)?;
        if normalized != input {
            return Err(MutationErrorCode::CodecInvalid);
        }
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
    let absolute = resolve_file_path(root, input)?;
    if git_dir.is_some_and(|directory| absolute.starts_with(directory)) {
        return Err(MutationErrorCode::PathGit);
    }
    // Protect worktree control directories even for an external repository.
    for ancestor in absolute.ancestors().skip(1) {
        if let Some(directory) = discover_git_dir(ancestor)?
            && absolute.starts_with(directory)
        {
            return Err(MutationErrorCode::PathGit);
        }
    }
    let metadata = match fs::symlink_metadata(&absolute) {
        Ok(metadata) => {
            if !metadata.is_file() {
                return Err(MutationErrorCode::PathNotFile);
            }
            if metadata.nlink() != 1 {
                return Err(MutationErrorCode::PathHardlink);
            }
            Some(metadata)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if require_existing {
                return Err(MutationErrorCode::TargetNotFound);
            }
            None
        }
        Err(_) => return Err(MutationErrorCode::FilesystemError),
    };
    let relative = absolute
        .strip_prefix(root)
        .unwrap_or(&absolute)
        .to_path_buf();
    let display = relative
        .to_str()
        .ok_or(MutationErrorCode::CodecInvalid)?
        .to_string();
    Ok(MutationTarget {
        relative,
        display,
        absolute,
        metadata,
    })
}

/// Canonicalize existing prefixes, retaining absent suffixes without creating them.
/// This is the common identity used for reading, permission and mutation.
pub(crate) fn resolve_file_path(root: &Path, input: &str) -> Result<PathBuf, MutationErrorCode> {
    if input.is_empty() || input.contains('\0') {
        return Err(MutationErrorCode::PathRoot);
    }
    let candidate = root.join(input);
    let mut current = PathBuf::new();
    for component in candidate.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                current.pop();
            }
            Component::Normal(value) => {
                if value.as_encoded_bytes().eq_ignore_ascii_case(b".git") {
                    return Err(MutationErrorCode::PathGit);
                }
                current.push(value);
            }
            other => current.push(other.as_os_str()),
        }
        match fs::symlink_metadata(&current) {
            Ok(_) => {
                current = current
                    .canonicalize()
                    .map_err(|_| MutationErrorCode::PathSymlink)?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(MutationErrorCode::FilesystemError),
        }
        if current.components().any(|part| {
            part.as_os_str()
                .as_encoded_bytes()
                .eq_ignore_ascii_case(b".git")
        }) {
            return Err(MutationErrorCode::PathGit);
        }
    }
    if current.parent().is_none() {
        return Err(MutationErrorCode::PathRoot);
    }
    Ok(current)
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
    fn file_reads_resolve_absolute_parent_and_external_symlinks() {
        let owned = tempdir().unwrap();
        let root = owned.path().join("project");
        fs::create_dir(&root).unwrap();
        let outside = owned.path().join("note.txt");
        fs::write(&outside, "outside").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("alias")).unwrap();
        let tools = tools_at(&root);
        for input in [outside.to_str().unwrap(), "../note.txt", "alias"] {
            assert_eq!(tools.read(input, None, None).unwrap().text, "1 | outside");
        }
        // Search/walk tools retain their project-only fence.
        assert!(matches!(
            super::resolve_in_root(tools.root(), "../note.txt"),
            Err(ToolError::PathEscape(_))
        ));
        assert!(matches!(
            super::resolve_in_root(tools.root(), "alias"),
            Err(ToolError::PathEscape(_))
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
