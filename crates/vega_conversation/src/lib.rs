pub mod agent;
mod artifact;
pub mod attachments;
mod automatic_titles;
pub mod codex_tasks;
pub mod command_scope;
mod composer_predictions;
pub mod display_history;
mod git_workspace;
pub mod history;
pub mod link_preview;
pub mod permission_contracts;
pub mod permission_reset;
pub mod plans;
mod pricing;
pub mod session_reasoning;
pub mod startup;
pub mod summary;
mod thread_access;
pub mod thread_permissions;
pub mod threads;
pub mod types;
mod usage_dashboard;
pub use usage_dashboard::UsageDashboardController;

pub use artifact::{ArtifactCaptureCandidate, ArtifactService};
pub use git_workspace::{
    BranchSwitchPermit, BranchWorkspaceService, GitWorkspaceService, TrustedGitService,
};

pub use pricing::{
    PricingAuthority, PricingLoadOutcome, PricingSaveOutcome, PricingSavePlan,
    PricingSettingsService,
};
pub use vega_token::{ModelPricingSpec, PricingCatalog, RateSpec, UsageCounts};

pub mod navigation;
pub mod palette;
pub mod terminal;

mod project_branch;
pub use project_branch::ProjectBranchService;

pub mod sidebar_organization;

mod provider_settings;
pub use provider_settings::ProviderSettingsService;

mod mcp_settings;
pub use mcp_settings::{McpRunReadiness, McpServerSettingsService, McpSettingsError};

mod skill_settings;
pub use skill_settings::{SkillSettingsError, SkillSettingsService};

pub mod ordinary_submissions;
pub mod pending_messages;
pub mod submission_diagnostics;

pub mod privacy;
mod run_credentials;

pub mod automation;

pub mod subagents;

pub mod model_capabilities;
pub use model_capabilities::{ModelCapabilityAuthority, ModelCapabilitySettingsService};
