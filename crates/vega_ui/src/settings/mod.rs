//! Settings view (A1-10 UI skeleton): provider list, add-provider form, and
//! default model / permission mode pickers.
//!
//! The view is opened with Cmd+, ([`OpenSettings`]) and closed with Esc or
//! the back button ([`CloseSettings`]); whether it replaces the session
//! placeholder is tracked by the [`SettingsOpen`] global, following the
//! global pattern proven in T07. It loads the config from the config root
//! (`vega_store::paths`, tech-spec §6) when constructed and saves it back on
//! every mutation, so configuration survives a restart.
//!
//! Credentials never appear in the UI: the key form field is masked while
//! typing; cached actual local references decide the stored/re-entry badge.
//! Values persist only in the owner-only plaintext local credential store.

use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Div, Entity, EventEmitter, FocusHandle, Focusable, Global, MouseButton,
    MouseUpEvent, Window, actions, div, px, relative,
};
use vega_conversation::types::{
    PricingDraftReason, PricingEntryKind, PricingEntryProjection, PricingMutation, PricingNotice,
    PricingRateInputs, PricingSettingsErrorCode, PricingSettingsProjection, ReasoningChoice,
    ReasoningDisabledWire, ReasoningProfileProjection, ReasoningProtocol, ReasoningSupport,
};
use vega_store::config::{self, AppConfig, ProviderConfig};
use vega_store::context_compaction::ModelContextPolicy;
use vega_store::keystore;
use vega_theme::{Layout, Typography, theme};

use crate::text_input::TextInput;

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
        PreviousPricingAction
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
/// The app uses this as the boundary to refresh its model catalog; it never
/// carries a credential or requests an in-session model change.
pub struct SettingsSaved;

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

/// Typed projection of the independent reasoning.toml authority.
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

/// Requests a fresh worker-side reasoning.toml read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReasoningReloadRequested;

/// User-selectable capability templates. A template only supplies an
/// explicit declaration; it is never inferred from a provider URL or model
/// name. The resulting fields remain editable before the worker saves them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReasoningTemplate {
    OpenAi,
    Zhipu,
}

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

/// Keyboard focus targets for the thinking capability editor. The target is
/// rebuilt with the exact provider/model projection, so a stale failed draft
/// cannot be applied to a different profile by index alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReasoningFocusTarget {
    Reload,
    Template {
        index: usize,
        template: ReasoningTemplate,
    },
    Protocol(usize),
    Support(usize),
    Effort {
        index: usize,
        effort: &'static str,
    },
    Preference(usize),
    Disabled(usize),
    Replay(usize),
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

/// The settings view: a plain page with the provider list, the add-provider
/// form, and the default pickers. Holds its own form input buffers, so it
/// must be cached by the parent across re-renders (it is rebuilt — reloading
mod helpers;
mod mcp;
mod preferences;
mod provider_management;
mod reasoning_render;
mod reasoning_state;
mod render_impl;
mod skills;
mod state;
mod usage;
pub use usage::UsageReloadRequested;

#[cfg(test)]
mod tests;

pub use helpers::all_models;
pub(crate) use helpers::*;
pub use state::SettingsView;

mod updater;
