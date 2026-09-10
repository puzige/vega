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

/// Persisted Sidebar geometry. These storage-layer bounds mirror the R21
/// visual tokens without introducing a UI dependency into `vega_store`.
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

/// One OpenAI-compatible provider entry.
///
/// `key_ref` is only a reference name into the local credential store; the credential
/// value itself never appears in this file (see [`crate::keystore`]).
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderConfig {
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
            enabled: true,
            name: String::new(),
            base_url: String::new(),
            models: Vec::new(),
            key_ref: String::new(),
        }
    }
}

/// Default choices for new conversations.
///
/// Fields are plain `String` on purpose: the shared enums
/// (`PermissionMode`, `RunMode`) live in `vega_conversation::types`
/// (tech-spec §3) and a `vega_store` → `vega_conversation` dependency would
/// create a cycle; bridging is deferred to S4+.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Defaults {
    /// Default model ID for new conversations.
    pub model: String,
    /// Default permission mode: `"readonly"` | `"confirm"` | `"auto"`.
    /// Defaults to `"confirm"` (phase1-plan E5 safe default).
    pub permission_mode: String,
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            model: String::new(),
            permission_mode: "confirm".to_string(),
        }
    }
}

/// UI preferences.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiPrefs {
    /// UI theme. Defaults to `"dark"`.
    pub theme: String,
    /// Whether the sidebar is collapsed (Cmd+B). Defaults to `false`; the
    /// serde default keeps configs written before this field loadable.
    #[serde(default)]
    pub sidebar_collapsed: bool,
    /// User-selected Sidebar width. Legacy configs resolve to the R21
    /// default; both deserialization and serialization clamp invalid values.
    #[serde(
        default = "sidebar_width_default",
        deserialize_with = "deserialize_sidebar_width",
        serialize_with = "serialize_sidebar_width"
    )]
    pub sidebar_width: f32,
    /// Whether the sidebar 「项目」 block is collapsed (T12). Defaults to
    /// `false`; serde default keeps older configs loadable.
    #[serde(default)]
    pub projects_collapsed: bool,
    /// Whether the sidebar 「会话」 block is collapsed (T12). Defaults to
    /// `false`; serde default keeps older configs loadable.
    #[serde(default)]
    pub sessions_collapsed: bool,
}

impl Default for UiPrefs {
    fn default() -> Self {
        Self {
            theme: "dark".to_string(),
            sidebar_collapsed: false,
            sidebar_width: SIDEBAR_WIDTH_DEFAULT,
            projects_collapsed: false,
            sessions_collapsed: false,
        }
    }
}

/// Top-level configuration at the config root (tech-spec §6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AppConfig {
    /// Configured providers.
    pub providers: Vec<ProviderConfig>,
    /// Default model / permission mode.
    pub defaults: Defaults,
    /// UI preferences.
    pub ui: UiPrefs,
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

/// Path-parameterized variant of [`load`]; also used by tests and the R1
/// model-selection worker (owned config file path, no env dependency).
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Unique temp directory per call so parallel tests never collide.
    fn temp_dir(tag: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "vega-store-t06-{tag}-{}-{nanos}-{seq}",
            std::process::id()
        ))
    }

    fn sample_config() -> AppConfig {
        AppConfig {
            providers: vec![ProviderConfig {
                enabled: true,
                name: "deepseek".to_string(),
                base_url: "https://api.deepseek.com".to_string(),
                models: vec!["deepseek-chat".to_string(), "deepseek-reasoner".to_string()],
                key_ref: "deepseek".to_string(),
            }],
            defaults: Defaults {
                model: "deepseek-chat".to_string(),
                permission_mode: "confirm".to_string(),
            },
            ui: UiPrefs {
                theme: "dark".to_string(),
                sidebar_collapsed: false,
                sidebar_width: SIDEBAR_WIDTH_DEFAULT,
                projects_collapsed: false,
                sessions_collapsed: false,
            },
        }
    }

    #[test]
    fn round_trip_preserves_all_fields() {
        let config = sample_config();
        let dir = temp_dir("roundtrip");
        let path = dir.join("config.toml");
        config.save_to(&path).unwrap();
        let loaded = load_from(&path).unwrap();
        assert_eq!(config, loaded);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn sidebar_collapsed_round_trips_and_defaults_false() {
        let dir = temp_dir("sidebar-collapsed");
        let path = dir.join("config.toml");
        // Round-trip preserves a non-default sidebar_collapsed value.
        let mut config = sample_config();
        config.ui.sidebar_collapsed = true;
        config.save_to(&path).unwrap();
        let loaded = load_from(&path).unwrap();
        assert!(loaded.ui.sidebar_collapsed);
        // Backward compatibility: a config written before the field existed
        // loads with the serde default (`false`) and keeps the theme. The
        // legacy file carries every pre-T09 field (T06 template shape).
        let legacy_path = dir.join("legacy.toml");
        fs::write(
            &legacy_path,
            concat!(
                "[[providers]]\n",
                "name = \"deepseek\"\n",
                "base_url = \"https://api.deepseek.com\"\n",
                "models = [\"deepseek-chat\"]\n",
                "key_ref = \"deepseek\"\n",
                "\n",
                "[defaults]\n",
                "model = \"deepseek-chat\"\n",
                "permission_mode = \"confirm\"\n",
                "\n",
                "[ui]\n",
                "theme = \"dark\"\n",
            ),
        )
        .unwrap();
        let legacy = load_from(&legacy_path).unwrap();
        assert!(!legacy.ui.sidebar_collapsed);
        assert_eq!(legacy.ui.sidebar_width, SIDEBAR_WIDTH_DEFAULT);
        assert_eq!(legacy.ui.theme, "dark");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn sidebar_width_round_trips_clamps_and_defaults_for_legacy_config() {
        let dir = temp_dir("sidebar-width");
        let path = dir.join("config.toml");
        let mut config = sample_config();
        config.ui.sidebar_width = 348.5;
        config.save_to(&path).unwrap();
        assert_eq!(load_from(&path).unwrap().ui.sidebar_width, 348.5);

        config.ui.sidebar_width = f32::INFINITY;
        config.save_to(&path).unwrap();
        assert_eq!(
            load_from(&path).unwrap().ui.sidebar_width,
            SIDEBAR_WIDTH_DEFAULT
        );

        fs::write(
            &path,
            "providers = []\n\n[defaults]\nmodel = \"\"\npermission_mode = \"confirm\"\n\n[ui]\ntheme = \"dark\"\nsidebar_width = 999.0\n",
        )
        .unwrap();
        assert_eq!(
            load_from(&path).unwrap().ui.sidebar_width,
            SIDEBAR_WIDTH_MAX
        );

        fs::write(
            &path,
            "providers = []\n\n[defaults]\nmodel = \"\"\npermission_mode = \"confirm\"\n\n[ui]\ntheme = \"dark\"\n",
        )
        .unwrap();
        assert_eq!(
            load_from(&path).unwrap().ui.sidebar_width,
            SIDEBAR_WIDTH_DEFAULT
        );

        for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(clamp_sidebar_width(invalid), SIDEBAR_WIDTH_DEFAULT);
        }
        assert_eq!(clamp_sidebar_width(-1.0), SIDEBAR_WIDTH_MIN);
        assert_eq!(clamp_sidebar_width(10_000.0), SIDEBAR_WIDTH_MAX);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn block_collapse_fields_round_trip_and_default_false() {
        let dir = temp_dir("block-collapsed");
        let path = dir.join("config.toml");
        // Round-trip preserves non-default block collapse values (T12).
        let mut config = sample_config();
        config.ui.projects_collapsed = true;
        config.ui.sessions_collapsed = true;
        config.save_to(&path).unwrap();
        let loaded = load_from(&path).unwrap();
        assert!(loaded.ui.projects_collapsed);
        assert!(loaded.ui.sessions_collapsed);
        // Backward compatibility: a config written before the fields existed
        // loads with the serde default (`false` for both).
        let legacy_path = dir.join("legacy.toml");
        fs::write(
            &legacy_path,
            concat!(
                "[[providers]]\n",
                "name = \"deepseek\"\n",
                "base_url = \"https://api.deepseek.com\"\n",
                "models = [\"deepseek-chat\"]\n",
                "key_ref = \"deepseek\"\n",
                "\n",
                "[defaults]\n",
                "model = \"deepseek-chat\"\n",
                "permission_mode = \"confirm\"\n",
                "\n",
                "[ui]\n",
                "theme = \"dark\"\n",
                "sidebar_collapsed = true\n",
            ),
        )
        .unwrap();
        let legacy = load_from(&legacy_path).unwrap();
        assert!(!legacy.ui.projects_collapsed);
        assert!(!legacy.ui.sessions_collapsed);
        // 既有字段不受影响。
        assert!(legacy.ui.sidebar_collapsed);
        assert_eq!(legacy.ui.theme, "dark");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn missing_file_creates_default_template() {
        let dir = temp_dir("template");
        let path = dir.join("nested").join("config.toml");
        let loaded = load_from(&path).unwrap();
        assert_eq!(loaded, AppConfig::default());
        assert_eq!(loaded.defaults.permission_mode, "confirm");
        assert_eq!(loaded.ui.theme, "dark");
        // The template must carry explanatory comments and stay parseable.
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.starts_with('#'));
        assert!(text.contains("key_ref"));
        let reparsed: AppConfig = toml::from_str(&text).unwrap();
        assert_eq!(reparsed, AppConfig::default());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn invalid_toml_yields_parse_error() {
        let dir = temp_dir("parse-error");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        fs::write(&path, "not [valid toml").unwrap();
        let err = load_from(&path).unwrap_err();
        assert!(matches!(err, ConfigError::Parse(_)));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn key_ref_is_a_reference_name_only() {
        let config = sample_config();
        let provider = &config.providers[0];
        // key_ref is a reference into the local credential store, by convention the
        // provider name; it carries no credential value.
        assert_eq!(provider.key_ref, provider.name);
        let body = toml::to_string_pretty(&config).unwrap();
        assert!(body.contains("key_ref = \"deepseek\""));
    }
}
