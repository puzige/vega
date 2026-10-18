pub use crate::automatic_titles::AutomaticTitleRequest;
use std::sync::Arc;
pub use vega_runtime::ImageAttachment;
pub use vega_runtime::computer_use::{
    Capability, ComputerUseControl, ComputerUseRun, ControlPhase, ControlSnapshot, DesktopTarget,
    ManagedComponents, Observation, SUPERSEDED_CONTROL_OUTPUT, Target,
};

use serde::{Deserialize, Deserializer, Serialize};

/// Stable, content-free pricing settings failure vocabulary.
mod artifact;
mod codex_task;
mod context;
mod events;
mod mcp;
mod message_location;
mod meter;
mod permission;
mod pricing;
mod reasoning;
mod response;
mod skills;
mod thread;
mod tool_calls;
mod usage;
mod usage_dashboard;
mod workspace;

pub use artifact::*;
pub use codex_task::*;
pub use context::*;
pub use events::*;
pub use mcp::*;
pub use message_location::*;
pub use meter::*;
pub use permission::*;
pub use pricing::*;
pub use reasoning::*;
pub use response::*;
pub use skills::*;
pub use thread::*;
pub use tool_calls::*;
pub use usage::*;
pub use usage_dashboard::*;
pub use workspace::*;

mod palette;
pub use palette::*;
mod terminal;
pub use terminal::*;

mod project_branch;
pub use project_branch::*;
mod navigation;
pub use navigation::*;

mod sidebar_organization;
pub use sidebar_organization::*;

mod provider_settings;
pub use provider_settings::*;

mod project_worker;
pub use project_worker::*;

mod running_threads;
pub use running_threads::*;

mod scroll;
pub use scroll::*;

mod updater;
pub use updater::*;

mod model_catalog;
pub use model_catalog::*;

mod pending_messages;
pub use pending_messages::*;

pub use vega_credentials::CredentialBinding;

pub use crate::privacy::{PrivacyFinding, PrivacyReview};

mod user_questions;
pub use user_questions::*;

mod automation;
pub use automation::*;

mod subagents;
pub use subagents::*;
