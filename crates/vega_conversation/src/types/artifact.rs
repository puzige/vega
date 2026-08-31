use super::*;

/// Opaque route-owned identifier for one ephemeral artifact card.
///
/// It is deliberately not serializable and cannot be constructed outside the
/// conversation crate. A card id is meaningful only to the service that

/// Opaque route-owned identifier for one ephemeral artifact card.
///
/// It is deliberately not serializable and cannot be constructed outside the
/// conversation crate. A card id is meaningful only to the service that

/// issued it for the current route epoch.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ArtifactCardId {
    pub(crate) route_epoch: u64,
    pub(crate) slot: u32,
    pub(crate) seal: u64,
}

impl std::fmt::Debug for ArtifactCardId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ArtifactCardId([opaque])")
    }
}

/// Monotonic provenance label for an ephemeral artifact card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactSource {
    /// A strict, non-reused write/edit success whose current file identity and
    /// unfiltered Git object digest were proven.
    AgentArtifact,
    /// A workspace change that is not, or is no longer, provably agent-owned.
    WorkspaceChange,
}

/// Safe metadata for one route-owned artifact card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactCard {
    pub id: ArtifactCardId,
    pub label: String,
    pub source: ArtifactSource,
    /// Present only while the artifact maps to a current, regular workspace
    /// file. Preview and Open in are disabled when this is absent.
    pub current_file_id: Option<WorkspaceFileId>,
    /// Whether the private raw path is eligible for a bounded text preview.
    /// Content validation still happens lazily when Preview is requested.
    pub preview_available: bool,
}

/// Bounded text projection for an artifact preview. It intentionally does not
/// implement serde and its `Debug` output never contains file content.
#[derive(Clone, PartialEq, Eq)]
pub struct ArtifactPreviewProjection {
    pub(crate) card_id: ArtifactCardId,
    pub(crate) file_id: WorkspaceFileId,
    pub(crate) text: String,
}

impl ArtifactPreviewProjection {
    pub const fn card_id(&self) -> ArtifactCardId {
        self.card_id
    }

    pub const fn file_id(&self) -> WorkspaceFileId {
        self.file_id
    }

    pub fn text(&self) -> &str {
        &self.text
    }
}

impl std::fmt::Debug for ArtifactPreviewProjection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ArtifactPreviewProjection")
            .field("card_id", &self.card_id)
            .field("file_id", &self.file_id)
            .field("text", &"[redacted]")
            .finish()
    }
}

/// Fixed Phase 1 external handoff allowlist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenInTarget {
    VisualStudioCode,
    Cursor,
    Zed,
    Terminal,
    DefaultApplication,
    RevealInFinder,
}

/// Content-free confirmation of exactly one successful Open in launch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenInOutcome {
    pub card_id: ArtifactCardId,
    pub target: OpenInTarget,
}

/// Stable, content-free Git workspace error vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitWorkspaceErrorCode {
    InvalidRoot,
    NotRepository,
    SpawnFailed,
    GitFailed,
    TimedOut,
    Cancelled,
    OutputTooLarge,
    MalformedOutput,
    StaleGeneration,
    UnknownFile,
    MetadataOnly,
    ChangedDuringRead,
    ProcessControlFailed,
    ArtifactConflict,
    ArtifactLimit,
    BranchDirty,
    BranchOperationInProgress,
    BranchDetached,
    BranchUnborn,
    BranchUnsafeFilter,
    BranchAlreadyCurrent,
}

impl GitWorkspaceErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidRoot => "invalid_root",
            Self::NotRepository => "not_repository",
            Self::SpawnFailed => "spawn_failed",
            Self::GitFailed => "git_failed",
            Self::TimedOut => "timed_out",
            Self::Cancelled => "cancelled",
            Self::OutputTooLarge => "output_too_large",
            Self::MalformedOutput => "malformed_output",
            Self::StaleGeneration => "stale_generation",
            Self::UnknownFile => "unknown_file",
            Self::MetadataOnly => "metadata_only",
            Self::ChangedDuringRead => "changed_during_read",
            Self::ProcessControlFailed => "process_control_failed",
            Self::ArtifactConflict => "artifact_conflict",
            Self::ArtifactLimit => "artifact_limit",
            Self::BranchDirty => "branch_dirty",
            Self::BranchOperationInProgress => "branch_operation_in_progress",
            Self::BranchDetached => "branch_detached",
            Self::BranchUnborn => "branch_unborn",
            Self::BranchUnsafeFilter => "branch_unsafe_filter",
            Self::BranchAlreadyCurrent => "branch_already_current",
        }
    }
}

/// Public error containing no root, path, stderr, or patch content.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct GitWorkspaceError {
    code: GitWorkspaceErrorCode,
}

impl GitWorkspaceError {
    pub(crate) const fn new(code: GitWorkspaceErrorCode) -> Self {
        Self { code }
    }

    pub const fn code(self) -> GitWorkspaceErrorCode {
        self.code
    }
}

impl std::fmt::Debug for GitWorkspaceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("GitWorkspaceError")
            .field(&self.code.as_str())
            .finish()
    }
}

impl std::fmt::Display for GitWorkspaceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.code.as_str())
    }
}

impl std::error::Error for GitWorkspaceError {}
