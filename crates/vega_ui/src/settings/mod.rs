use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Div, Entity, EventEmitter, FocusHandle, Focusable, Global, MouseButton,
    MouseUpEvent, Window, actions, div, px, relative,
};
use vega_conversation::types::{
    PricingDraftReason, PricingEntryKind, PricingEntryProjection, PricingMutation, PricingNotice,
    PricingRateInputs, PricingSettingsErrorCode, PricingSettingsProjection, ReasoningChoice,
    ReasoningProfileProjection, ReasoningSupport,
};
use vega_store::config::{self, AppConfig, ProviderConfig};
use vega_store::context_compaction::ModelContextPolicy;
use vega_store::keystore;
use vega_theme::{Layout, Typography, theme};

use crate::text_input::TextInput;

#[cfg(test)]
mod reasoning_compatibility_tests;

actions!(
    vega_settings,
    [
        OpenSettings,
        CloseSettings,
        ActivateProviderAction,
        NextProviderAction,
        PreviousProviderAction,
        ActivatePricingAction,
        NextPricingAction,
        PreviousPricingAction,
        NextSkillsAction,
        PreviousSkillsAction,
        NextMcpAction,
        PreviousMcpAction
    ]
);

/// Typed pricing mutation emitted to the app-owned controller.
pub struct PricingMutationRequested {
    pub generation: u64,
    pub mutation: Result<PricingMutation, PricingSettingsErrorCode>,
}

/// Explicit recovery/reload request emitted to the app-owned controller.
pub struct PricingReloadRequested;

/// Retries the controller-owned exact dirty pricing plan.
pub struct PricingRetryRequested {
    pub generation: u64,
}

/// Discards the controller-owned dirty plan and keeps current authority.
pub struct PricingDiscardRequested {
    pub generation: u64,
}

/// Emitted after a Settings config mutation has been written successfully.
pub struct SettingsSaved {
    pub replay_identity_changed: bool,
}

/// Settings never opens SQLite itself. The app worker resolves this exact
/// provider/model policy and sends it back to the active editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelContextLoadRequested {
    pub request_id: u64,
    pub provider: String,
    pub model: String,
}

/// Typed mutation for the existing provider/model editor. Missing numeric
/// values together mean an explicitly unknown capacity, not assumed defaults.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelContextSaveRequested {
    pub request_id: u64,
    pub provider: String,
    pub model: String,
    pub input_limit: Option<u64>,
    pub output_limit: Option<u64>,
    pub automatic_compaction: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelContextLoaded {
    pub policy: Option<ModelContextPolicy>,
    pub legacy_present: bool,
}

/// Content-safe error vocabulary for the worker-owned reasoning settings
/// controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReasoningSettingsErrorCode {
    Io,
    Invalid,
    Conflict,
    Busy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReasoningSettingsProjection {
    Loading,
    Ready {
        generation: u64,
        profiles: Vec<ReasoningProfileProjection>,
        error: Option<ReasoningSettingsErrorCode>,
    },
    Saving {
        generation: u64,
        operation_id: u64,
        profile: ReasoningProfileProjection,
    },
}

/// Exact profile edit sent to the app controller. `base` lets the worker
/// compute a field patch and preserve disjoint external edits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReasoningProfileSaveRequested {
    pub generation: u64,
    pub operation_id: u64,
    pub base: ReasoningProfileProjection,
    pub profile: ReasoningProfileProjection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReasoningReloadRequested;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PricingEditorKind {
    AddCustom,
    UpdateCustom,
    UpdateBuiltinBase,
    UpdateDeepSeek,
}

#[derive(Clone)]
pub(crate) struct PricingEditor {
    kind: PricingEditorKind,
    model: Option<String>,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum PricingFocusTarget {
    Reload,
    Add,
    Edit(usize),
    Secondary(usize),
    Retry,
    Discard,
    Save,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReasoningFocusTarget {
    Reload,
    Preference {
        index: usize,
        choice: ReasoningChoice,
    },
}

/// Whether the settings view currently replaces the session placeholder.
///
/// Toggled by the app-level [`OpenSettings`]/[`CloseSettings`] handlers.
pub struct SettingsOpen(pub bool);

impl Global for SettingsOpen {}

/// One-shot route requested by a pricing preflight failure.
pub struct PricingSettingsRequested(pub bool);
impl Global for PricingSettingsRequested {}

/// Fixed permission-mode vocabulary (matches `vega_store::config::Defaults`).
const PERMISSION_MODES: [&str; 4] = ["readonly", "confirm", "auto", "full_access"];

/// Status placeholder shown only when the local store contains the reference;
/// the key value itself is never rendered (safety red line).
const KEY_STORED_PLACEHOLDER: &str = "•••••••已存储";

mod agents;
/// The settings view: a plain page with the provider list, the add-provider
/// form, and the default pickers. Holds its own form input buffers, so it
/// must be cached by the parent across re-renders (it is rebuilt — reloading
mod helpers;
mod mcp;
mod model_catalog;
mod model_configuration;
use model_configuration::SettingsDestination;
mod preferences;
mod provider_management;
mod reasoning_render;
mod reasoning_state;
mod render_impl;
mod skills;
mod state;
mod usage;
pub use usage::UsageReloadRequested;

pub use helpers::all_models;
pub(crate) use helpers::*;
pub use state::SettingsView;

mod updater;
