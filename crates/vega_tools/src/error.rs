//! Tool error taxonomy. Mutation failures deliberately expose only stable
//! codes through `Display` and `Debug`; optional edit context is available
//! through an explicit accessor and therefore cannot leak accidentally via
//! logging or ordinary error propagation.

use std::fmt;

/// Stable validation and mutation failure codes used by safe projections.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationErrorCode {
    MalformedJson,
    InputNotObject,
    UnexpectedField,
    MissingPath,
    WrongPathType,
    MissingContent,
    WrongContentType,
    MissingOldString,
    WrongOldStringType,
    MissingNewString,
    WrongNewStringType,
    PathAbsolute,
    PathParent,
    PathRoot,
    PathSymlink,
    PathHardlink,
    PathGit,
    PathNotFile,
    ParentNotFound,
    TargetNotFound,
    CheckpointIdInvalid,
    CheckpointUnavailable,
    CheckpointExists,
    CheckpointSymlink,
    CheckpointMetadataInvalid,
    EditEmptyOldString,
    EditNoMatch,
    EditMultipleMatches,
    TargetChanged,
    AtomicWriteFailed,
    FilesystemError,
    CodecInvalid,
    PreparedScopeMismatch,
}

impl MutationErrorCode {
    /// Wire value used by audit and tool-result projections.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MalformedJson => "malformed_json",
            Self::InputNotObject => "input_not_object",
            Self::UnexpectedField => "unexpected_field",
            Self::MissingPath => "missing_path",
            Self::WrongPathType => "wrong_path_type",
            Self::MissingContent => "missing_content",
            Self::WrongContentType => "wrong_content_type",
            Self::MissingOldString => "missing_old_string",
            Self::WrongOldStringType => "wrong_old_string_type",
            Self::MissingNewString => "missing_new_string",
            Self::WrongNewStringType => "wrong_new_string_type",
            Self::PathAbsolute => "path_absolute",
            Self::PathParent => "path_parent",
            Self::PathRoot => "path_root",
            Self::PathSymlink => "path_symlink",
            Self::PathHardlink => "path_hardlink",
            Self::PathGit => "path_git",
            Self::PathNotFile => "path_not_file",
            Self::ParentNotFound => "parent_not_found",
            Self::TargetNotFound => "target_not_found",
            Self::CheckpointIdInvalid => "checkpoint_id_invalid",
            Self::CheckpointUnavailable => "checkpoint_unavailable",
            Self::CheckpointExists => "checkpoint_exists",
            Self::CheckpointSymlink => "checkpoint_symlink",
            Self::CheckpointMetadataInvalid => "checkpoint_metadata_invalid",
            Self::EditEmptyOldString => "edit_empty_old_string",
            Self::EditNoMatch => "edit_no_match",
            Self::EditMultipleMatches => "edit_multiple_matches",
            Self::TargetChanged => "target_changed",
            Self::AtomicWriteFailed => "atomic_write_failed",
            Self::FilesystemError => "filesystem_error",
            Self::CodecInvalid => "codec_invalid",
            Self::PreparedScopeMismatch => "prepared_scope_mismatch",
        }
    }

    pub(crate) fn from_str(value: &str) -> Option<Self> {
        Some(match value {
            "malformed_json" => Self::MalformedJson,
            "input_not_object" => Self::InputNotObject,
            "unexpected_field" => Self::UnexpectedField,
            "missing_path" => Self::MissingPath,
            "wrong_path_type" => Self::WrongPathType,
            "missing_content" => Self::MissingContent,
            "wrong_content_type" => Self::WrongContentType,
            "missing_old_string" => Self::MissingOldString,
            "wrong_old_string_type" => Self::WrongOldStringType,
            "missing_new_string" => Self::MissingNewString,
            "wrong_new_string_type" => Self::WrongNewStringType,
            "path_absolute" => Self::PathAbsolute,
            "path_parent" => Self::PathParent,
            "path_root" => Self::PathRoot,
            "path_symlink" => Self::PathSymlink,
            "path_hardlink" => Self::PathHardlink,
            "path_git" => Self::PathGit,
            "path_not_file" => Self::PathNotFile,
            "parent_not_found" => Self::ParentNotFound,
            "target_not_found" => Self::TargetNotFound,
            "checkpoint_id_invalid" => Self::CheckpointIdInvalid,
            "checkpoint_unavailable" => Self::CheckpointUnavailable,
            "checkpoint_exists" => Self::CheckpointExists,
            "checkpoint_symlink" => Self::CheckpointSymlink,
            "checkpoint_metadata_invalid" => Self::CheckpointMetadataInvalid,
            "edit_empty_old_string" => Self::EditEmptyOldString,
            "edit_no_match" => Self::EditNoMatch,
            "edit_multiple_matches" => Self::EditMultipleMatches,
            "target_changed" => Self::TargetChanged,
            "atomic_write_failed" => Self::AtomicWriteFailed,
            "filesystem_error" => Self::FilesystemError,
            "codec_invalid" => Self::CodecInvalid,
            "prepared_scope_mismatch" => Self::PreparedScopeMismatch,
            _ => return None,
        })
    }

    pub(crate) const fn is_invalid_input_code(self) -> bool {
        matches!(
            self,
            Self::MalformedJson
                | Self::InputNotObject
                | Self::UnexpectedField
                | Self::MissingPath
                | Self::WrongPathType
                | Self::MissingContent
                | Self::WrongContentType
                | Self::MissingOldString
                | Self::WrongOldStringType
                | Self::MissingNewString
                | Self::WrongNewStringType
                | Self::PathAbsolute
                | Self::PathParent
                | Self::PathRoot
                | Self::PathSymlink
                | Self::PathHardlink
                | Self::PathGit
                | Self::PathNotFile
                | Self::ParentNotFound
                | Self::TargetNotFound
                | Self::CheckpointIdInvalid
                | Self::CheckpointUnavailable
                | Self::CheckpointSymlink
                | Self::EditEmptyOldString
                | Self::FilesystemError
        )
    }
}

/// Explicitly requested, bounded context for an edit match failure.
#[derive(Clone, PartialEq, Eq)]
pub struct EditFailureContext(String);

impl EditFailureContext {
    pub(crate) fn new(value: String) -> Self {
        Self(value)
    }

    /// Return the bounded context. Callers must opt in; it is never included
    /// in `Display` or `Debug` output.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for EditFailureContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("EditFailureContext([REDACTED])")
    }
}

/// A stable, redacted write/edit failure.
#[derive(Clone, PartialEq, Eq)]
pub struct MutationError {
    code: MutationErrorCode,
    context: Option<EditFailureContext>,
}

impl MutationError {
    pub(crate) fn new(code: MutationErrorCode) -> Self {
        Self {
            code,
            context: None,
        }
    }

    pub(crate) fn with_context(code: MutationErrorCode, context: String) -> Self {
        Self {
            code,
            context: Some(EditFailureContext::new(context)),
        }
    }

    /// Stable failure code.
    pub fn code(&self) -> MutationErrorCode {
        self.code
    }

    /// Bounded edit context, available only through explicit access.
    pub fn edit_context(&self) -> Option<&EditFailureContext> {
        self.context.as_ref()
    }
}

impl fmt::Debug for MutationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MutationError")
            .field("code", &self.code)
            .field("context", &self.context.as_ref().map(|_| "[REDACTED]"))
            .finish()
    }
}

impl fmt::Display for MutationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "write/edit failed ({})", self.code.as_str())
    }
}

impl std::error::Error for MutationError {}

/// Errors surfaced by the built-in tools.
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    /// A requested path resolved outside the project root: `..` traversal,
    /// absolute-path injection, or a symlink jumping out of the root.
    /// Path-fence red line (tech-spec §3, risks #4) — never softened.
    #[error("path escapes the project root: {0}")]
    PathEscape(String),

    /// The requested path does not exist.
    #[error("not found: {0}")]
    NotFound(String),

    /// The file was rejected by the NUL-byte binary probe (tech-spec §4.4).
    #[error("binary file: {0}")]
    BinaryFile(String),

    /// Caller input is malformed (bad regex, bad glob, 0 offset, …).
    #[error("invalid input: {0}")]
    InvalidInput(String),

    /// Result set exceeded a tool's hard limit. Reserved for callers that
    /// prefer a hard failure; T21's glob/grep truncate into
    /// [`ToolOutput::truncated`](crate::ToolOutput::truncated) instead.
    #[error("too many results (limit: {limit})")]
    TooManyResults { limit: usize },

    /// Underlying filesystem error.
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// Directory traversal failed before a file could be inspected.
    #[error("filesystem traversal failed: {0}")]
    Traversal(String),

    /// Stable, content-free write/edit failure.
    #[error(transparent)]
    Mutation(#[from] MutationError),
}
