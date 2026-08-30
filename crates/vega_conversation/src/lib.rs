//! Thread and message models, streaming state machine, and context assembly.
//!
//! T11 (A1-02) scope: the `types` data-model subset and the thread
//! orchestration layer. The streaming state machine and context assembly
//! land in S3/S4.

pub mod agent;
mod artifact;
mod git_workspace;
pub mod plans;
pub mod threads;
pub mod types;

pub use artifact::{ArtifactCaptureCandidate, ArtifactService};
pub use git_workspace::{
    BranchSwitchPermit, BranchWorkspaceService, GitWorkspaceService, TrustedGitService,
};
