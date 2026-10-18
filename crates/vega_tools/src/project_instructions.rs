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
