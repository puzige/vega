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
        Ok(Self { root: canonical })
    }

    /// The canonical project root all tool paths resolve against.
    pub fn root(&self) -> &Path {
        &self.root
    }
}
