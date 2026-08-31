//! Thread and message models, streaming state machine, and context assembly.
//!
//! T11 (A1-02) scope: the `types` data-model subset and the thread
//! orchestration layer. The streaming state machine and context assembly
//! land in S3/S4.

pub mod agent;
mod artifact;
mod git_workspace;
pub mod history;
pub mod plans;
mod pricing;
pub mod summary;
pub mod threads;
pub mod types;

pub use artifact::{ArtifactCaptureCandidate, ArtifactService};
pub use git_workspace::{
    BranchSwitchPermit, BranchWorkspaceService, GitWorkspaceService, TrustedGitService,
};
// S7-T39: downstream crates (app/UI) receive the frozen pricing capability
// through `vega_conversation` only — no direct pricing-engine dependency.
pub use pricing::{
    PricingAuthority, PricingLoadOutcome, PricingSaveOutcome, PricingSavePlan,
    PricingSettingsService,
};
pub use vega_token::{ModelPricingSpec, PricingCatalog, RateSpec, UsageCounts};
