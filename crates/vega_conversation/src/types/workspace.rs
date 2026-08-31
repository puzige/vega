use super::*;

/// Opaque identifier for one file in one workspace snapshot.
///
/// The identifier is deliberately not serializable and cannot be constructed
/// outside this crate. It is only meaningful while its snapshot generation is

/// Opaque identifier for one file in one workspace snapshot.
///
/// The identifier is deliberately not serializable and cannot be constructed
/// outside this crate. It is only meaningful while its snapshot generation is

/// current.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorkspaceFileId {
    pub(crate) generation: u64,
    pub(crate) slot: u32,
    pub(crate) seal: u64,
}

impl std::fmt::Debug for WorkspaceFileId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("WorkspaceFileId([opaque])")
    }
}

/// Opaque identifier for one local branch in one branch-list generation.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct BranchId {
    pub(crate) generation: u64,
    pub(crate) slot: u32,
    pub(crate) seal: u64,
}

impl std::fmt::Debug for BranchId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("BranchId([opaque])")
    }
}

/// Safe projection of one local branch. Raw ref bytes and object ids remain
/// private to the headless Git service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchItem {
    pub id: BranchId,
    pub label: String,
    pub current: bool,
}

/// Bounded, ephemeral local-branch snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchSnapshot {
    pub generation: u64,
    pub branches: Vec<BranchItem>,
}

/// Bounded `@file` candidate projection (A2-12, S8-T47): project-relative
/// file paths for the composer reference selector. Produced by the app
/// layer from a gitignore-aware bounded walk (deterministic lexicographic
/// order, hard entry cap), handed to the IO-free selector as a typed
/// snapshot. No filesystem access happens behind this type.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileIndexSnapshot {
    pub entries: Vec<String>,
}

/// Provider/model/thinking selection state for the composer selector
/// (A2-14, S8-T47). Persisted at the app-level config seam; the run-start
/// model snapshot semantics of `threads.model` are untouched.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ComposerDefaults {
    /// Selected model id (empty = no explicit selection yet).
    pub model: String,
    /// Thinking level (`off|low|medium|high`).
    pub thinking: String,
}

/// Content-free outcome of an attempted branch switch. The accompanying
/// snapshot, when present, is authoritative for every exit path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchSwitchOutcome {
    Switched,
    Failed(GitWorkspaceErrorCode),
}

/// Authoritative post-switch refresh plus the content-free switch outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchSwitchCompletion {
    pub outcome: BranchSwitchOutcome,
    pub snapshot: Option<BranchSnapshot>,
}

/// Opaque identifier for one canonical three-source index snapshot.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct IndexSnapshotId {
    pub(crate) generation: u64,
    pub(crate) slot: u64,
    pub(crate) seal: u64,
}

impl std::fmt::Debug for IndexSnapshotId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("IndexSnapshotId([opaque])")
    }
}

/// Safe classification for one row in the commit checklist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitSelectionKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    TypeChanged,
}

/// Safe checklist row. Raw paths, object ids and Git status bytes remain
/// private to the trusted service.
#[derive(Clone, PartialEq, Eq)]
pub struct CommitSelection {
    pub file_id: WorkspaceFileId,
    pub label: String,
    pub previous_label: Option<String>,
    pub kind: CommitSelectionKind,
    pub forced: bool,
}

impl std::fmt::Debug for CommitSelection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CommitSelection")
            .field("file_id", &self.file_id)
            .field("kind", &self.kind)
            .field("forced", &self.forced)
            .field("label_bytes", &self.label.len())
            .field(
                "has_previous_label",
                &self.previous_label.as_ref().map(|label| label.len()),
            )
            .finish()
    }
}

/// Canonical displayed A authority for the first commit confirmation.
#[derive(Clone, PartialEq, Eq)]
pub struct CommitChecklist {
    pub id: IndexSnapshotId,
    pub workspace_generation: u64,
    pub staged: Vec<CommitSelection>,
    pub optional: Vec<CommitSelection>,
}

impl std::fmt::Debug for CommitChecklist {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CommitChecklist")
            .field("id", &self.id)
            .field("workspace_generation", &self.workspace_generation)
            .field("staged_count", &self.staged.len())
            .field("optional_count", &self.optional.len())
            .finish()
    }
}

/// Opaque, single-use B authority displayed before the final commit.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct PreparedCommitId {
    pub(crate) generation: u64,
    pub(crate) slot: u64,
    pub(crate) seal: u64,
}

impl std::fmt::Debug for PreparedCommitId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PreparedCommitId([opaque])")
    }
}

/// Content-free safe projection of an accepted B authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedCommit {
    pub id: PreparedCommitId,
    pub workspace_generation: u64,
    pub staged_file_count: u32,
    pub summary_truncated: bool,
}

/// Editable provider draft. Debug output deliberately excludes content.
#[derive(Clone, PartialEq, Eq)]
pub struct CommitDraft {
    text: String,
}

impl CommitDraft {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub(crate) fn new(text: String) -> Self {
        Self { text }
    }
}

impl std::fmt::Debug for CommitDraft {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CommitDraft")
            .field("text_bytes", &self.text.len())
            .finish()
    }
}

/// Stable content-free failure vocabulary for T34.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitErrorCode {
    InvalidRoot,
    NotRepository,
    SpawnFailed,
    GitFailed,
    TimedOut,
    Cancelled,
    OutputTooLarge,
    MalformedOutput,
    StaleAuthority,
    UnsafeRepository,
    UnsafeFilter,
    IntentToAdd,
    NoStagedChanges,
    InvalidSelection,
    ChangedDuringRead,
    InvalidMessage,
    DraftFailed,
    ProcessControlFailed,
}

impl CommitErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidRoot => "invalid_root",
            Self::NotRepository => "not_repository",
            Self::SpawnFailed => "spawn_failed",
            Self::GitFailed => "git_failed",
            Self::TimedOut => "timed_out",
            Self::Cancelled => "cancelled",
            Self::OutputTooLarge => "output_too_large",
            Self::MalformedOutput => "malformed_output",
            Self::StaleAuthority => "stale_authority",
            Self::UnsafeRepository => "unsafe_repository",
            Self::UnsafeFilter => "unsafe_filter",
            Self::IntentToAdd => "intent_to_add",
            Self::NoStagedChanges => "no_staged_changes",
            Self::InvalidSelection => "invalid_selection",
            Self::ChangedDuringRead => "changed_during_read",
            Self::InvalidMessage => "invalid_message",
            Self::DraftFailed => "draft_failed",
            Self::ProcessControlFailed => "process_control_failed",
        }
    }
}

/// Prepare result always carries the authoritative post-attempt workspace
/// snapshot when its owner refresh succeeds.
#[derive(Clone, PartialEq, Eq)]
pub struct CommitPrepareCompletion {
    pub prepared: Option<PreparedCommit>,
    pub workspace: Option<WorkspaceSnapshot>,
    pub error: Option<CommitErrorCode>,
}

impl std::fmt::Debug for CommitPrepareCompletion {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CommitPrepareCompletion")
            .field("prepared", &self.prepared)
            .field(
                "workspace_generation",
                &self
                    .workspace
                    .as_ref()
                    .map(|workspace| workspace.generation),
            )
            .field("error", &self.error)
            .finish()
    }
}

/// Content-free terminal outcome of the trusted commit mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitOutcome {
    Committed,
    Failed(CommitErrorCode),
}

/// Commit result plus the authoritative post-attempt workspace snapshot.
#[derive(Clone, PartialEq, Eq)]
pub struct CommitCompletion {
    pub outcome: CommitOutcome,
    pub workspace: Option<WorkspaceSnapshot>,
}

impl std::fmt::Debug for CommitCompletion {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CommitCompletion")
            .field("outcome", &self.outcome)
            .field(
                "workspace_generation",
                &self
                    .workspace
                    .as_ref()
                    .map(|workspace| workspace.generation),
            )
            .finish()
    }
}

/// Current repository head projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceHead {
    Branch { label: String },
    Detached,
    Unborn { label: Option<String> },
}

/// One side of a tracked workspace change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceChangeKind {
    Unchanged,
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    TypeChanged,
    Unmerged,
    Untracked,
}

/// Line statistics are never guessed for binary or untracked files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceLineCount {
    Known(u64),
    Binary,
    Unknown,
}

/// Safe file metadata projected from a private raw Git path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceFile {
    pub id: WorkspaceFileId,
    pub label: String,
    pub previous_label: Option<String>,
    pub staged: WorkspaceChangeKind,
    pub unstaged: WorkspaceChangeKind,
    pub additions: WorkspaceLineCount,
    pub deletions: WorkspaceLineCount,
    pub language: DiffLanguage,
}

/// Bounded aggregate snapshot statistics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceStats {
    pub file_count: u32,
    pub additions: WorkspaceLineCount,
    pub deletions: WorkspaceLineCount,
}

/// Safe, ephemeral projection of the latest workspace snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceSnapshot {
    pub generation: u64,
    pub head: WorkspaceHead,
    pub files: Vec<WorkspaceFile>,
    pub stats: WorkspaceStats,
}

/// Frozen syntax-highlight language vocabulary for Sprint 6.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLanguage {
    Rust,
    TypeScript,
    Tsx,
    JavaScript,
    Python,
    Plain,
}

/// Layer that produced a structured patch section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLayer {
    Staged,
    Unstaged,
    Untracked,
}

/// Structured diff row kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffRowKind {
    Context,
    Addition,
    Deletion,
}

/// One source row. The body intentionally implements no `Debug` or serde
/// traits so it cannot accidentally enter events, logs, persistence, or wire
/// payloads.
#[derive(Clone, PartialEq, Eq)]
pub struct DiffRow {
    pub(crate) kind: DiffRowKind,
    pub(crate) old_line: Option<u32>,
    pub(crate) new_line: Option<u32>,
    pub(crate) text: String,
}

impl DiffRow {
    pub const fn kind(&self) -> DiffRowKind {
        self.kind
    }

    pub const fn old_line(&self) -> Option<u32> {
        self.old_line
    }

    pub const fn new_line(&self) -> Option<u32> {
        self.new_line
    }

    pub fn text(&self) -> &str {
        &self.text
    }
}

/// Structured hunk with parsed coordinates rather than raw patch headers.
#[derive(Clone, PartialEq, Eq)]
pub struct DiffHunk {
    pub(crate) old_start: u32,
    pub(crate) old_count: u32,
    pub(crate) new_start: u32,
    pub(crate) new_count: u32,
    pub(crate) heading_suffix: Option<String>,
    pub(crate) missing_trailing_newline: bool,
    pub(crate) rows: Vec<DiffRow>,
}

impl DiffHunk {
    pub const fn old_start(&self) -> u32 {
        self.old_start
    }

    pub const fn old_count(&self) -> u32 {
        self.old_count
    }

    pub const fn new_start(&self) -> u32 {
        self.new_start
    }

    pub const fn new_count(&self) -> u32 {
        self.new_count
    }

    pub fn heading_suffix(&self) -> Option<&str> {
        self.heading_suffix.as_deref()
    }

    pub const fn missing_trailing_newline(&self) -> bool {
        self.missing_trailing_newline
    }

    pub fn rows(&self) -> &[DiffRow] {
        &self.rows
    }
}

/// A staged, unstaged, or untracked structured diff section.
#[derive(Clone, PartialEq, Eq)]
pub struct DiffSection {
    pub(crate) layer: DiffLayer,
    pub(crate) hunks: Vec<DiffHunk>,
}

impl DiffSection {
    pub const fn layer(&self) -> DiffLayer {
        self.layer
    }

    pub fn hunks(&self) -> &[DiffHunk] {
        &self.hunks
    }
}

/// Bounded source projection for Diff UI. Its debug representation is always
/// redacted and it is deliberately not serializable.
#[derive(Clone, PartialEq, Eq)]
pub struct DiffTextProjection {
    pub(crate) file_id: WorkspaceFileId,
    pub(crate) language: DiffLanguage,
    pub(crate) sections: Vec<DiffSection>,
}

impl DiffTextProjection {
    pub const fn file_id(&self) -> WorkspaceFileId {
        self.file_id
    }

    pub const fn language(&self) -> DiffLanguage {
        self.language
    }

    pub fn sections(&self) -> &[DiffSection] {
        &self.sections
    }
}

impl std::fmt::Debug for DiffTextProjection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DiffTextProjection")
            .field("file_id", &self.file_id)
            .field("language", &self.language)
            .field("sections", &"[redacted]")
            .finish()
    }
}
