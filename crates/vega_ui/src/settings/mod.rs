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
//! typing and every stored provider shows the constant "•••••••已存储"
//! placeholder; the key value itself only ever goes to the Keychain.

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Div, Entity, EventEmitter, FocusHandle, Global, MouseButton, MouseUpEvent,
    Window, actions, div, px, relative,
};
use vega_conversation::types::{
    PricingDraftReason, PricingEntryKind, PricingEntryProjection, PricingMutation, PricingNotice,
    PricingRateInputs, PricingSettingsErrorCode, PricingSettingsProjection,
};
use vega_store::config::{self, AppConfig, ProviderConfig};
use vega_store::keystore;
use vega_theme::{Typography, theme};

use crate::text_input::TextInput;

actions!(
    vega_settings,
    [
        OpenSettings,
        CloseSettings,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PricingEditorKind {
    AddCustom,
    UpdateCustom,
    UpdateBuiltinBase,
    UpdateDeepSeek,
}

#[derive(Clone)]
struct PricingEditor {
    kind: PricingEditorKind,
    model: Option<String>,
}

#[derive(Clone, PartialEq, Eq)]
enum PricingFocusTarget {
    Reload,
    Add,
    Edit(usize),
    Secondary(usize),
    Retry,
    Discard,
    Save,
    Cancel,
}

/// Whether the settings view currently replaces the session placeholder.
///
/// Toggled by the app-level [`OpenSettings`]/[`CloseSettings`] handlers.
pub struct SettingsOpen(pub bool);

impl Global for SettingsOpen {}

/// Fixed permission-mode vocabulary (matches `vega_store::config::Defaults`).
const PERMISSION_MODES: [&str; 3] = ["readonly", "confirm", "auto"];

/// Status placeholder shown for every provider with a non-empty `key_ref`;
/// the key value itself is never rendered (safety red line).
const KEY_STORED_PLACEHOLDER: &str = "•••••••已存储";

/// The settings view: a plain page with the provider list, the add-provider
/// form, and the default pickers. Holds its own form input buffers, so it
/// must be cached by the parent across re-renders (it is rebuilt — reloading
mod helpers;
mod render_impl;
mod state;

#[cfg(test)]
mod tests;

pub use helpers::all_models;
pub(crate) use helpers::*;
pub(crate) use render_impl::*;
pub use state::SettingsView;
pub(crate) use state::*;
