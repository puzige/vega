//! Tool surface: one [`Tools`] instance bound to a canonical project root.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::checkpoint::MutationContext;
use crate::codec::CheckpointIds;
use crate::error::{MutationError, ToolError};
use crate::fence::discover_git_dir;

static TOOL_INSTANCE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// Read and explicitly configured mutation tools bound to one project root.
///
/// Every path argument is interpreted relative to the root and enforced by
/// the path fence ([`crate::fence`], tech-spec §3 red line). Cheap to clone
/// and share; the root is canonicalized once at construction.
#[derive(Clone)]
pub struct Tools {
    pub(crate) root: PathBuf,
    pub(crate) mutation: Option<MutationContext>,
    pub(crate) instance_id: u64,
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
        Ok(Self {
            root: canonical,
            mutation: None,
            instance_id: TOOL_INSTANCE_SEQUENCE.fetch_add(1, Ordering::Relaxed),
        })
    }

    /// Explicitly bind this tool instance to one checkpoint call. `Tools::new`
    /// remains read-only; write/edit fail closed until this method succeeds.
    pub fn with_mutation_context(
        mut self,
        checkpoint_root: impl Into<PathBuf>,
        project_id: &str,
        thread_id: &str,
        call_id: &str,
    ) -> Result<Self, ToolError> {
        let git_dir = discover_git_dir(&self.root)
            .map_err(|code| ToolError::from(MutationError::new(code)))?;
        let ids = CheckpointIds::new(project_id, thread_id, call_id)?;
        self.mutation = Some(MutationContext::new(
            checkpoint_root.into(),
            &self.root,
            ids,
            git_dir.as_deref(),
        )?);
        Ok(self)
    }

    /// The canonical project root all tool paths resolve against.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl fmt::Debug for Tools {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Tools")
            .field("root", &self.root)
            .field("mutation", &self.mutation.as_ref().map(|_| "configured"))
            .finish()
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
