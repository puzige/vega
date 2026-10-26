//! Application configuration stored at
//! `${XDG_CONFIG_HOME:-$HOME/.config}/vega/config.toml`
//! ([`crate::paths`], tech-spec §6).
//!
//! The config file never contains credential values: each provider carries
//! a `key_ref`, which is the reference name of the credential kept in the
//! local credential file (see [`crate::keystore`]).

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

static EDIT_LOCK: Mutex<()> = Mutex::new(());

use serde::{Deserialize, Serialize};

pub const SIDEBAR_WIDTH_DEFAULT: f32 = 304.0;
pub const SIDEBAR_WIDTH_MIN: f32 = 240.0;
pub const SIDEBAR_WIDTH_MAX: f32 = 365.0;

/// Normalizes untrusted persisted or caller-provided Sidebar widths.
pub fn clamp_sidebar_width(width: f32) -> f32 {
    if width.is_finite() {
        width.clamp(SIDEBAR_WIDTH_MIN, SIDEBAR_WIDTH_MAX)
    } else {
        SIDEBAR_WIDTH_DEFAULT
    }
}

fn sidebar_width_default() -> f32 {
    SIDEBAR_WIDTH_DEFAULT
}

fn deserialize_sidebar_width<'de, D>(deserializer: D) -> Result<f32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    f32::deserialize(deserializer).map(clamp_sidebar_width)
}

fn serialize_sidebar_width<S>(width: &f32, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_f32(clamp_sidebar_width(*width))
}

/// Error raised while loading or saving [`AppConfig`].
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// Filesystem error (including a missing `HOME` environment variable).
    #[error("config io error: {0}")]
    Io(#[from] io::Error),
    /// The config file exists but is not valid TOML.
    #[error("config parse error: {0}")]
    Parse(#[from] toml::de::Error),
    /// The config could not be serialized.
    #[error("config serialize error: {0}")]
    Serialize(#[from] toml::ser::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderApi {
    #[default]
    ChatCompletions,
    Responses,
}

/// One OpenAI-compatible provider entry.
///
/// `key_ref` is only a reference name into the local credential store; the credential
/// value itself never appears in this file (see [`crate::keystore`]).
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderConfig {
    #[serde(default)]
    pub api: ProviderApi,
    /// Whether this provider is available for new runs.
    #[serde(default = "provider_enabled_default")]
    pub enabled: bool,
    /// Provider identifier, e.g. `"deepseek"`.
    pub name: String,
    /// OpenAI-compatible endpoint base URL.
    pub base_url: String,
    /// Model IDs offered by this provider.
    pub models: Vec<String>,
    /// local credential store reference name for this provider's credential; by
    /// convention the provider name.
    pub key_ref: String,
}

impl std::fmt::Debug for ProviderConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderConfig")
            .field("api", &self.api)
            .field("enabled", &self.enabled)
            .field("name", &self.name)
            .field("base_url", &"<redacted>")
            .field("models", &self.models)
            .field("key_ref", &self.key_ref)
            .finish()
    }
}

fn provider_enabled_default() -> bool {
    true
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            api: ProviderApi::default(),
            enabled: true,
            name: String::new(),
            base_url: String::new(),
            models: Vec::new(),
            key_ref: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Defaults {
    /// Default model ID for new conversations.
    pub model: String,
    /// Default permission mode: `"readonly"` | `"confirm"` | `"auto"` | `"full_access"`.
    /// Defaults to `"confirm"` (phase1-plan E5 safe default).
    pub permission_mode: String,
    #[serde(default)]
    pub permission_revision: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_access_grant: Option<String>,
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            model: String::new(),
            permission_mode: "confirm".to_string(),
            permission_revision: 0,
            full_access_grant: None,
        }
    }
}

/// UI preferences.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiPrefs {
    /// UI theme. Defaults to `"dark"`.
    pub theme: String,
    #[serde(default = "default_ui_language")]
    pub language: String,
    /// Whether the sidebar is collapsed (Cmd+B). Defaults to `false`; the
    /// serde default keeps configs written before this field loadable.
    #[serde(default)]
    pub sidebar_collapsed: bool,
    #[serde(
        default = "sidebar_width_default",
        deserialize_with = "deserialize_sidebar_width",
        serialize_with = "serialize_sidebar_width"
    )]
    pub sidebar_width: f32,
    #[serde(default)]
    pub projects_collapsed: bool,
    #[serde(default)]
    pub sessions_collapsed: bool,
}

fn default_ui_language() -> String {
    "en".to_string()
}

impl Default for UiPrefs {
    fn default() -> Self {
        Self {
            theme: "dark".to_string(),
            language: default_ui_language(),
            sidebar_collapsed: false,
            sidebar_width: SIDEBAR_WIDTH_DEFAULT,
            projects_collapsed: false,
            sessions_collapsed: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentConfig {
    #[serde(default = "default_composer_predictions_enabled")]
    pub composer_predictions_enabled: bool,
    #[serde(default = "default_running_message_behavior")]
    pub running_message_behavior: String,
    #[serde(default)]
    pub desktop_use_enabled: bool,
    #[serde(default)]
    pub browser_use_enabled: bool,
    /// Maximum number of agentic turns before a run stops. `0` = unlimited.
    #[serde(default)]
    pub turn_limit: u32,
    #[serde(default)]
    pub codex_acp_profile: Option<CodexAcpProfileConfig>,
}

fn default_composer_predictions_enabled() -> bool {
    true
}

fn default_running_message_behavior() -> String {
    "queue".into()
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            composer_predictions_enabled: true,
            running_message_behavior: default_running_message_behavior(),
            desktop_use_enabled: false,
            browser_use_enabled: false,
            turn_limit: 0,
            codex_acp_profile: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodexAcpProfileConfig {
    pub display_name: String,
    pub executable: String,
    #[serde(default)]
    pub args: Vec<String>,
}

impl CodexAcpProfileConfig {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.display_name.trim().is_empty()
            || self.display_name.len() > 128
            || self.display_name.chars().any(char::is_control)
        {
            return Err("invalid Codex profile name");
        }
        let executable = Path::new(&self.executable);
        if self.executable.trim().is_empty()
            || self.executable.len() > 4096
            || self.executable.chars().any(char::is_control)
            || !executable.is_absolute()
            || executable.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::CurDir | std::path::Component::ParentDir
                )
            })
        {
            return Err("Codex executable must be an absolute path");
        }
        if self.args.len() > 32 || self.args.iter().any(|arg| !codex_profile_arg_is_safe(arg)) {
            return Err("Codex profile arguments are invalid or may contain a secret");
        }
        Ok(())
    }
}

fn codex_profile_arg_is_safe(value: &str) -> bool {
    if value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
        return false;
    }
    let lowered = value.to_ascii_lowercase();
    let sensitive_markers = [
        "api-key",
        "api_key",
        "apikey",
        "token",
        "secret",
        "password",
        "passwd",
        "credential",
        "auth",
        "oauth",
        "private-key",
        "private_key",
        "ssh-key",
        "bearer",
        "env-file",
        "openai_api_key=",
    ];
    let suspicious_prefixes = [
        "sk-",
        "sk_",
        "ghp_",
        "gho_",
        "ghu_",
        "ghs_",
        "github_pat_",
        "xoxb-",
        "xoxp-",
        "glpat-",
        "akia",
        "asia",
        "aiza",
        "eyj",
    ];
    !sensitive_markers
        .iter()
        .any(|marker| lowered.contains(marker))
        && !suspicious_prefixes
            .iter()
            .any(|prefix| lowered.contains(prefix))
        && !value
            .split(|character: char| !character.is_ascii_hexdigit())
            .any(|component| component.len() >= 32)
        && !value
            .split_once("://")
            .map(|(_, rest)| rest.split('/').next().unwrap_or_default())
            .is_some_and(|authority| authority.contains('@') && authority.contains(':'))
}

/// Top-level configuration at the config root (tech-spec §6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub credential_bindings: Vec<vega_credentials::CredentialBinding>,
    /// Configured providers.
    pub providers: Vec<ProviderConfig>,
    /// Default model / permission mode.
    pub defaults: Defaults,
    /// UI preferences.
    pub ui: UiPrefs,
    /// Agent loop guardrails.
    #[serde(default)]
    pub agent: AgentConfig,
}

/// Comment header written at the top of the config file so every field is
/// self-documented.
const FILE_HEADER: &str = "\
# Vega configuration file.
#
# [[providers]]: one block per OpenAI-compatible provider.
#   name     - provider identifier, also used as the local credential store reference
#   base_url - OpenAI-compatible endpoint base URL
#   models   - model IDs offered by this provider
#   key_ref  - reference name under which the provider's credential is
#              stored in the local credential file (vega_store::keystore);
#              the credential value itself is never written to this file
#
# [defaults]
#   model           - default model ID for new conversations
#   permission_mode - \"readonly\" | \"confirm\" | \"auto\" (default: \"confirm\")
#
# [ui]
#   theme              - UI theme (default: \"dark\")
#   sidebar_collapsed  - whether the sidebar starts collapsed (default: false)
#   sidebar_width      - Sidebar width in logical pixels (240..365; default: 304)
#   projects_collapsed - whether the sidebar 「项目」 block starts collapsed
#                        (default: false)
#   sessions_collapsed - whether the sidebar 「会话」 block starts collapsed
#                        (default: false)
#
# [agent]
#   turn_limit - maximum number of agentic turns (provider round-trips) in one
#                run before it stops; 0 means unlimited (default: 0). This caps
#                turns, not tool calls, mirroring Claude Code's --max-turns.
";

/// Path of the config file: `${XDG_CONFIG_HOME:-$HOME/.config}/vega/config.toml`
/// (resolved by [`crate::paths`]; `HOME` must be set).
fn config_path() -> Result<PathBuf, ConfigError> {
    let dir = crate::paths::config_dir()
        .ok_or_else(|| io::Error::other("HOME environment variable is not set"))?;
    Ok(dir.join("config.toml"))
}

/// Load the config from the config root (tech-spec §6).
///
/// If the file does not exist, a default template (with explanatory
/// comments) is written first and the default config is returned.
pub fn load() -> Result<AppConfig, ConfigError> {
    load_from(&config_path()?)
}

pub fn load_from(path: &Path) -> Result<AppConfig, ConfigError> {
    let edit = begin_edit(path)?;
    if !path.exists() {
        edit.save()?;
    }
    Ok(edit.config.clone())
}

/// Reads an existing config file without creating or modifying anything.
/// Worker-side validation paths use this variant so a selection request is
/// strictly read-only until its durable thread update begins.
pub fn read_from(path: &Path) -> Result<AppConfig, ConfigError> {
    let text = std::fs::read_to_string(path)?;
    toml::from_str(&text).map_err(ConfigError::Parse)
}

/// A serialized read/edit/save transaction for the single application config file.
/// Keep this guard alive across any credential rollback. Use `save`, never
/// `AppConfig::save_to`, while holding the guard (the latter acquires the same lock).
pub struct ConfigEdit {
    /// Latest disk state; mutate only fields owned by the requested operation.
    pub config: AppConfig,
    path: PathBuf,
    _guard: MutexGuard<'static, ()>,
}

impl ConfigEdit {
    /// Atomically publish the edited state while retaining the transaction lock.
    pub fn save(&self) -> Result<(), ConfigError> {
        self.config.save_to_unlocked(&self.path)
    }
}

/// Read the current config under the common writer lock; a missing file starts
/// with defaults and is not created until the caller explicitly saves.
pub fn begin_edit(path: &Path) -> Result<ConfigEdit, ConfigError> {
    let guard = EDIT_LOCK
        .lock()
        .map_err(|_| io::Error::other("config edit lock unavailable"))?;
    let config = match std::fs::read_to_string(path) {
        Ok(text) => toml::from_str(&text).map_err(ConfigError::Parse)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => AppConfig::default(),
        Err(error) => return Err(ConfigError::Io(error)),
    };
    Ok(ConfigEdit {
        config,
        path: path.to_path_buf(),
        _guard: guard,
    })
}

/// Merge a narrow field mutation onto current disk state and atomically save it.
pub fn update_from(
    path: &Path,
    mutate: impl FnOnce(&mut AppConfig),
) -> Result<AppConfig, ConfigError> {
    let mut edit = begin_edit(path)?;
    mutate(&mut edit.config);
    edit.save()?;
    Ok(edit.config.clone())
}

/// Merge a narrow field mutation at the resolved global config path.
pub fn update(mutate: impl FnOnce(&mut AppConfig)) -> Result<AppConfig, ConfigError> {
    update_from(&config_path()?, mutate)
}

impl AppConfig {
    /// Save the config to the config root (atomic write).
    pub fn save(&self) -> Result<(), ConfigError> {
        self.save_to(&config_path()?)
    }

    /// Path-parameterized variant of [`AppConfig::save`]; also used by
    /// tests.
    ///
    /// Writes a sibling `.tmp` file first, then renames it over `path`, so
    /// readers never observe a partially written file.
    pub fn save_to(&self, path: &Path) -> Result<(), ConfigError> {
        let _guard = EDIT_LOCK
            .lock()
            .map_err(|_| io::Error::other("config edit lock unavailable"))?;
        self.save_to_unlocked(path)
    }

    fn save_to_unlocked(&self, path: &Path) -> Result<(), ConfigError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let body = toml::to_string_pretty(self)?;
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, format!("{FILE_HEADER}\n{body}"))?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }
}
