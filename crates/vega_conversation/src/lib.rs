//! Thread and message models, streaming state machine, and context assembly.
//!
//! T11 (A1-02) scope: the `types` data-model subset and the thread
//! orchestration layer. The streaming state machine and context assembly
//! land in S3/S4.

pub mod agent;
mod git_workspace;
pub mod plans;
pub mod threads;
pub mod types;

pub use git_workspace::GitWorkspaceService;
