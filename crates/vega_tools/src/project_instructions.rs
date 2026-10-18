use crate::{ToolError, Tools};
use std::io::Read;

pub const PROJECT_INSTRUCTIONS_MAX_BYTES: usize = 65_536;

pub struct ProjectInstructions {
    file_name: &'static str,
    content: String,
}

impl ProjectInstructions {
    pub fn file_name(&self) -> &'static str {
        self.file_name
    }

    pub fn content(&self) -> &str {
        &self.content
    }
}

impl std::fmt::Debug for ProjectInstructions {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProjectInstructions")
            .field("file_name", &self.file_name)
            .field("content_bytes", &self.content.len())
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectInstructionsFailure {
    InvalidRootPath,
    Unavailable,
    TooLarge,
    InvalidText,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("project instructions {file_name} could not be loaded ({failure:?})")]
pub struct ProjectInstructionsError {
    file_name: &'static str,
    failure: ProjectInstructionsFailure,
}

impl ProjectInstructionsError {
    pub fn invalid_root_path() -> Self {
        Self {
            file_name: "project root",
            failure: ProjectInstructionsFailure::InvalidRootPath,
        }
    }

    pub fn file_name(&self) -> &'static str {
        self.file_name
    }

    pub fn failure(&self) -> ProjectInstructionsFailure {
        self.failure
    }
}

impl Tools {
    pub fn project_instructions(
        &self,
    ) -> Result<Option<ProjectInstructions>, ProjectInstructionsError> {
        for file_name in ["AGENTS.md", "CLAUDE.md"] {
            let path = self.root().join(file_name);
            let error = |failure| ProjectInstructionsError { file_name, failure };
            let file = match crate::text_file::open_regular(&path, true) {
                Ok(file) => file,
                Err(ToolError::Io(io_error)) if io_error.kind() == std::io::ErrorKind::NotFound => {
                    crate::text_file::open_directory(self.root())
                        .map_err(|_| error(ProjectInstructionsFailure::Unavailable))?;
                    continue;
                }
                Err(_) => return Err(error(ProjectInstructionsFailure::Unavailable)),
            };
            self.check_credential_path(&path)
                .map_err(|_| error(ProjectInstructionsFailure::Unavailable))?;
            let metadata = file
                .metadata()
                .map_err(|_| error(ProjectInstructionsFailure::Unavailable))?;
            if metadata.len() > PROJECT_INSTRUCTIONS_MAX_BYTES as u64 {
                return Err(error(ProjectInstructionsFailure::TooLarge));
            }
            let mut bytes = Vec::new();
            file.take(PROJECT_INSTRUCTIONS_MAX_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| error(ProjectInstructionsFailure::Unavailable))?;
            if bytes.len() > PROJECT_INSTRUCTIONS_MAX_BYTES {
                return Err(error(ProjectInstructionsFailure::TooLarge));
            }
            let content = String::from_utf8(bytes)
                .map_err(|_| error(ProjectInstructionsFailure::InvalidText))?;
            if content.contains('\0') {
                return Err(error(ProjectInstructionsFailure::InvalidText));
            }
            return Ok(Some(ProjectInstructions { file_name, content }));
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Arc;

    #[test]
    fn root_instructions_prefer_agents_and_freeze_without_scanning_other_scopes() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("project");
        fs::create_dir_all(root.join("nested")).unwrap();
        fs::write(parent.path().join("AGENTS.md"), "outside instructions").unwrap();
        fs::write(root.join("nested/AGENTS.md"), "nested instructions").unwrap();
        let tools = crate::Tools::new(&root).unwrap();
        assert!(tools.project_instructions().unwrap().is_none());
        fs::write(root.join("CLAUDE.md"), "fallback instructions").unwrap();
        let fallback = tools.project_instructions().unwrap().unwrap();
        assert_eq!(fallback.file_name(), "CLAUDE.md");
        assert_eq!(fallback.content(), "fallback instructions");
        fs::write(root.join("AGENTS.md"), "private root instructions").unwrap();
        let snapshot = tools.project_instructions().unwrap().unwrap();
        assert_eq!(snapshot.file_name(), "AGENTS.md");
        assert_eq!(snapshot.content(), "private root instructions");
        assert!(!format!("{snapshot:?}").contains("private root"));
        fs::write(root.join("AGENTS.md"), "updated instructions").unwrap();
        assert_eq!(snapshot.content(), "private root instructions");
        assert_eq!(
            tools.project_instructions().unwrap().unwrap().content(),
            "updated instructions"
        );
    }

    #[test]
    fn unsafe_root_instructions_never_fall_back_to_claude() {
        for kind in [
            "symlink",
            "dangling",
            "hardlink",
            "directory",
            "socket",
            "fifo",
            "oversize",
            "invalid",
            "nul",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let candidate = dir.path().join("AGENTS.md");
            fs::write(dir.path().join("CLAUDE.md"), "must not load").unwrap();
            let source = dir.path().join("source");
            fs::write(&source, "private instructions").unwrap();
            let mut socket = None;
            match kind {
                "symlink" => std::os::unix::fs::symlink(&source, &candidate).unwrap(),
                "dangling" => {
                    std::os::unix::fs::symlink(dir.path().join("missing"), &candidate).unwrap()
                }
                "hardlink" => fs::hard_link(&source, &candidate).unwrap(),
                "directory" => fs::create_dir(&candidate).unwrap(),
                "socket" => {
                    socket = Some(std::os::unix::net::UnixListener::bind(&candidate).unwrap())
                }
                "fifo" => {
                    use std::os::unix::ffi::OsStrExt;
                    let path = std::ffi::CString::new(candidate.as_os_str().as_bytes()).unwrap();
                    // SAFETY: path is a live NUL-terminated path and mode contains valid permission bits.
                    let result = unsafe { libc::mkfifo(path.as_ptr(), 0o600) };
                    assert_eq!(result, 0);
                }
                "oversize" => {
                    fs::write(&candidate, vec![b'a'; PROJECT_INSTRUCTIONS_MAX_BYTES + 1]).unwrap()
                }
                "invalid" => fs::write(&candidate, [0xff]).unwrap(),
                "nul" => fs::write(&candidate, b"text\0instructions").unwrap(),
                _ => unreachable!(),
            }
            let error = crate::Tools::new(dir.path())
                .unwrap()
                .project_instructions()
                .unwrap_err();
            assert_eq!(error.file_name(), "AGENTS.md", "{kind}");
            assert!(!format!("{error:?} {error}").contains("private instructions"));
            drop(socket);
        }
    }

    #[test]
    fn root_disappearance_is_not_missing_instructions() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("project");
        fs::create_dir(&root).unwrap();
        let tools = crate::Tools::new(&root).unwrap();
        fs::remove_dir(&root).unwrap();
        assert!(tools.project_instructions().is_err());
    }

    #[test]
    fn unreadable_primary_instructions_fail_without_loading_fallback() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("AGENTS.md");
        fs::write(&path, "private unreadable instructions").unwrap();
        fs::write(dir.path().join("CLAUDE.md"), "fallback must not load").unwrap();
        let permissions = fs::metadata(&path).unwrap().permissions();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o0)).unwrap();
        let result = crate::Tools::new(dir.path())
            .unwrap()
            .project_instructions();
        fs::set_permissions(&path, permissions).unwrap();
        assert_eq!(result.unwrap_err().file_name(), "AGENTS.md");
    }

    #[test]
    fn root_instruction_limit_is_complete_and_credential_paths_remain_denied() {
        let dir = tempfile::tempdir().unwrap();
        let tools = crate::Tools::new(dir.path())
            .unwrap()
            .with_credential_policy(
                crate::ToolCredentialPolicy::new(
                    dir.path().to_path_buf(),
                    Vec::new(),
                    Arc::new(|_| unreachable!()),
                )
                .unwrap(),
            );
        assert!(tools.project_instructions().unwrap().is_none());
        fs::write(dir.path().join("AGENTS.md"), "private config").unwrap();
        assert!(tools.project_instructions().is_err());
        let tools = crate::Tools::new(dir.path()).unwrap();
        let content = "x".repeat(PROJECT_INSTRUCTIONS_MAX_BYTES);
        fs::write(dir.path().join("AGENTS.md"), &content).unwrap();
        assert_eq!(
            tools.project_instructions().unwrap().unwrap().content(),
            content
        );
    }
}
