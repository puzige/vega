//! The read-only tool surface: one [`Tools`] instance bound to a canonical
//! project root, shared by the agentic loop (T20).

use std::path::{Path, PathBuf};

use crate::error::ToolError;

/// Read-only tools (read / glob / grep) bound to one project root.
///
/// Every path argument is interpreted relative to the root and enforced by
/// the path fence ([`crate::fence`], tech-spec §3 red line). Cheap to clone
/// and share; the root is canonicalized once at construction.
#[derive(Debug, Clone)]
pub struct Tools {
    pub(crate) root: PathBuf,
}

impl Tools {
    /// Bind the tools to `root`, which must exist and is canonicalized.
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, ToolError> {
        let path = root.into();
        let canonical = path.canonicalize().map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => {
                ToolError::NotFound(path.to_string_lossy().into_owned())
            }
            _ => ToolError::Io(e),
        })?;
        if !canonical.is_dir() {
            return Err(ToolError::InvalidInput(
                "project root must be a directory".to_string(),
            ));
        }
        Ok(Self { root: canonical })
    }

    /// The canonical project root all tool paths resolve against.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// Shared deterministic directory walker. Standard filters remain enabled,
/// so `.gitignore`/`.ignore`/git excludes and hidden-file rules are honored.
/// `require_git(false)` makes project-local `.gitignore` work in tempdirs and
/// non-git projects too (tech-spec §4.4).
pub(crate) fn walker(start: &Path) -> ignore::Walk {
    let mut builder = ignore::WalkBuilder::new(start);
    builder
        .require_git(false)
        .follow_links(false)
        .sort_by_file_name(|left, right| left.cmp(right));
    builder.build()
}

/// Render a walker path relative to the canonical project root.
pub(crate) fn relative_display(root: &Path, path: &Path) -> Result<String, ToolError> {
    path.strip_prefix(root)
        .map(|relative| relative.to_string_lossy().into_owned())
        .map_err(|_| ToolError::PathEscape(path.to_string_lossy().into_owned()))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use crate::{ToolError, Tools};

    #[test]
    fn project_root_must_be_an_existing_directory() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("file.txt");
        fs::write(&file, "content").unwrap();

        assert!(matches!(Tools::new(&file), Err(ToolError::InvalidInput(_))));
        assert!(matches!(
            Tools::new(dir.path().join("missing")),
            Err(ToolError::NotFound(_))
        ));
    }
}
