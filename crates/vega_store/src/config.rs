//! Application configuration stored at `$HOME/.vega/config.toml`.
//!
//! The config file never contains credential values: each provider carries
//! a `key_ref`, which is the reference name of the credential kept in the
//! system Keychain (see [`crate::keystore`]).

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

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
/// `key_ref` is only a reference name into the Keychain; the credential
/// value itself never appears in this file (see [`crate::keystore`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ProviderConfig {
    /// Provider identifier, e.g. `"deepseek"`.
    pub name: String,
    /// OpenAI-compatible endpoint base URL.
    pub base_url: String,
    /// Model IDs offered by this provider.
    pub models: Vec<String>,
    /// Keychain reference name for this provider's credential; by
    /// convention the provider name.
    pub key_ref: String,
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
}

impl Default for UiPrefs {
    fn default() -> Self {
        Self {
            theme: "dark".to_string(),
        }
    }
}

/// Top-level configuration at `$HOME/.vega/config.toml`.
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
#   name     - provider identifier, also used as the Keychain reference
#   base_url - OpenAI-compatible endpoint base URL
#   models   - model IDs offered by this provider
#   key_ref  - reference name under which the provider's credential is
#              stored in the system Keychain (vega_store::keystore);
#              the credential value itself is never written to this file
#
# [defaults]
#   model           - default model ID for new conversations
#   permission_mode - \"readonly\" | \"confirm\" | \"auto\" (default: \"confirm\")
#
# [ui]
#   theme - UI theme (default: \"dark\")
";

/// Path of the config file: `$HOME/.vega/config.toml` (macOS-first; `HOME`
/// must be set).
fn config_path() -> Result<PathBuf, ConfigError> {
    let home = std::env::var("HOME").map_err(io::Error::other)?;
    Ok(PathBuf::from(home).join(".vega").join("config.toml"))
}

/// Load the config from `$HOME/.vega/config.toml`.
///
/// If the file does not exist, a default template (with explanatory
/// comments) is written first and the default config is returned.
pub fn load() -> Result<AppConfig, ConfigError> {
    load_from(&config_path()?)
}

/// Path-parameterized variant of [`load`]; also used by tests to stay off
/// `$HOME`.
fn load_from(path: &Path) -> Result<AppConfig, ConfigError> {
    match std::fs::read_to_string(path) {
        Ok(text) => toml::from_str(&text).map_err(ConfigError::Parse),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            let config = AppConfig::default();
            config.save_to(path)?;
            Ok(config)
        }
        Err(err) => Err(ConfigError::Io(err)),
    }
}

impl AppConfig {
    /// Save the config to `$HOME/.vega/config.toml` (atomic write).
    pub fn save(&self) -> Result<(), ConfigError> {
        self.save_to(&config_path()?)
    }

    /// Path-parameterized variant of [`AppConfig::save`]; also used by
    /// tests.
    ///
    /// Writes a sibling `.tmp` file first, then renames it over `path`, so
    /// readers never observe a partially written file.
    fn save_to(&self, path: &Path) -> Result<(), ConfigError> {
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
        // key_ref is a reference into the Keychain, by convention the
        // provider name; it carries no credential value.
        assert_eq!(provider.key_ref, provider.name);
        let body = toml::to_string_pretty(&config).unwrap();
        assert!(body.contains("key_ref = \"deepseek\""));
    }
}
