//! Read-only sidebar HEAD projection shared by conversation and UI.

/// Registered project identity and its exact persisted root path.
#[derive(Clone, PartialEq, Eq)]
pub struct ProjectBranchTarget {
    pub project_id: String,
    pub registered_path: String,
}

/// Content-free failure states never retain a previous branch label.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProjectBranchState {
    Branch(String),
    Detached,
    NonGit,
    Unknown,
}

impl ProjectBranchState {
    /// Returns the suffix displayed by the project row.
    pub fn suffix(&self) -> Option<&str> {
        match self {
            Self::Branch(label) => Some(label),
            Self::Detached => Some("detached"),
            Self::NonGit | Self::Unknown => None,
        }
    }
}

/// One identity-bound result from the bounded background batch.
#[derive(Clone)]
pub struct ProjectBranchRow {
    pub target: ProjectBranchTarget,
    pub state: ProjectBranchState,
}

/// Generation-bound batch completion; raw metadata never crosses this boundary.
pub struct ProjectBranchCompletion {
    pub generation: u64,
    pub rows: Vec<ProjectBranchRow>,
}
