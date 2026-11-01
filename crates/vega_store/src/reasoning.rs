use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

/// Current on-disk format version.
pub const REASONING_CONFIG_VERSION: u32 = 1;
/// File name under [`crate::paths::config_dir`].
pub const REASONING_FILE_NAME: &str = "config.yaml";

/// Error returned by the reasoning config codec and persistence boundary.
#[derive(Debug, thiserror::Error)]
pub enum ReasoningConfigError {
    /// Filesystem error.
    #[error("reasoning config io error: {0}")]
    Io(#[from] io::Error),
    /// Existing file is malformed TOML.
    #[error("reasoning config parse error: {0}")]
    Parse(#[from] toml::de::Error),
    /// Candidate could not be serialized.
    #[error("reasoning config serialize error: {0}")]
    Serialize(#[from] toml::ser::Error),
    /// Existing file is not valid UTF-8.
    #[error("reasoning config is not valid UTF-8")]
    Utf8(#[from] std::string::FromUtf8Error),
    /// A supported version was not available.
    #[error("unsupported reasoning config version: {0}")]
    UnsupportedVersion(u32),
    #[error("invalid reasoning profile: {0}")]
    InvalidProfile(String),
    /// The target changed after the worker read its expected snapshot.
    #[error("reasoning config changed concurrently")]
    ConcurrentModification,
}

/// Raw, versioned reasoning file. All fields are non-secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReasoningConfig {
    /// On-disk schema version.
    pub version: u32,
    #[serde(default, rename = "preferences")]
    pub profiles: Vec<ReasoningProfile>,
}

impl Default for ReasoningConfig {
    fn default() -> Self {
        Self {
            version: REASONING_CONFIG_VERSION,
            profiles: Vec::new(),
        }
    }
}

impl ReasoningConfig {
    /// Validates the whole document and rejects duplicate exact keys.
    pub fn validate(&self) -> Result<(), ReasoningConfigError> {
        if self.version != REASONING_CONFIG_VERSION {
            return Err(ReasoningConfigError::UnsupportedVersion(self.version));
        }
        for (index, profile) in self.profiles.iter().enumerate() {
            profile.validate().map_err(|message| {
                ReasoningConfigError::InvalidProfile(format!("profile {index}: {message}"))
            })?;
            if self.profiles[..index]
                .iter()
                .any(|prior| prior.provider == profile.provider && prior.model == profile.model)
            {
                return Err(ReasoningConfigError::InvalidProfile(format!(
                    "duplicate provider/model key {} / {}",
                    profile.provider, profile.model
                )));
            }
        }
        Ok(())
    }

    /// Returns the exact provider/model profile, if declared.
    pub fn profile(&self, provider: &str, model: &str) -> Option<&ReasoningProfile> {
        self.profiles
            .iter()
            .find(|profile| profile.provider == provider && profile.model == model)
    }

    /// Applies one exact profile patch without replacing unrelated records.
    pub fn upsert_profile(&mut self, profile: ReasoningProfile) {
        if let Some(existing) = self.profiles.iter_mut().find(|existing| {
            existing.provider == profile.provider && existing.model == profile.model
        }) {
            *existing = profile;
        } else {
            self.profiles.push(profile);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReasoningProfilePatch {
    /// Exact profile provider key.
    pub provider: String,
    /// Exact profile model key.
    pub model: String,
    /// New user preference, if edited.
    pub preference: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReasoningProfile {
    /// Exact provider identifier.
    pub provider: String,
    /// Exact model identifier.
    pub model: String,
    pub preference: String,
}

impl ReasoningProfile {
    pub fn validate(&self) -> Result<(), String> {
        if self.provider.trim().is_empty() {
            return Err("provider is empty".to_string());
        }
        if self.model.trim().is_empty() {
            return Err("model is empty".to_string());
        }
        if !matches!(
            self.preference.as_str(),
            "provider_default"
                | "enabled"
                | "disabled"
                | "minimal"
                | "low"
                | "medium"
                | "high"
                | "xhigh"
                | "max"
        ) {
            return Err("preference is not supported".to_string());
        }
        Ok(())
    }
}

/// Snapshot used for a worker-side compare-before-rename.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReasoningFileSnapshot {
    /// Whether the file existed when it was read.
    pub exists: bool,
    /// Exact bytes read from disk; empty when the file was absent.
    pub bytes: Vec<u8>,
}

impl ReasoningFileSnapshot {
    /// Builds the absent-file snapshot.
    pub fn absent() -> Self {
        Self {
            exists: false,
            bytes: Vec::new(),
        }
    }
}

/// Resolves the production reasoning config path.
pub fn reasoning_path() -> Result<PathBuf, ReasoningConfigError> {
    let dir = crate::paths::config_dir()
        .ok_or_else(|| io::Error::other("HOME environment variable is not set"))?;
    Ok(dir.join(REASONING_FILE_NAME))
}

/// Reads production reasoning config, returning an empty version-1 document
/// when the optional file does not exist.
pub fn load() -> Result<ReasoningConfig, ReasoningConfigError> {
    read_from(&reasoning_path()?)
}

/// Reads an owned path without creating or modifying it.
pub fn read_from(path: &Path) -> Result<ReasoningConfig, ReasoningConfigError> {
    read_authority_from(path).map(|(config, _)| config)
}

/// Reads and decodes one exact filesystem version, returning both the parsed
/// authority and the bytes used for compare-before-rename. Keeping these
/// values together avoids a second read producing a snapshot for a different
/// external write.
pub fn read_authority_from(
    path: &Path,
) -> Result<(ReasoningConfig, ReasoningFileSnapshot), ReasoningConfigError> {
    let snapshot = read_snapshot_from(path)?;
    let config = if snapshot.exists {
        decode(&snapshot.bytes)?
    } else {
        ReasoningConfig::default()
    };
    Ok((config, snapshot))
}

/// Reads the exact bytes used for a compare-before-rename.
pub fn read_snapshot_from(path: &Path) -> Result<ReasoningFileSnapshot, ReasoningConfigError> {
    match crate::yaml_config::read(path)? {
        Some(bytes) => Ok(ReasoningFileSnapshot {
            exists: true,
            bytes,
        }),
        None => Ok(ReasoningFileSnapshot::absent()),
    }
}

/// Serializes and atomically saves a candidate only if `expected` still
/// describes the target. The fingerprint check and rename are intentionally
/// documented as best-effort against non-cooperating writers; process-local
/// callers must serialize through one shared coordinator.
pub fn save_if_unchanged(
    path: &Path,
    expected: &ReasoningFileSnapshot,
    candidate: &ReasoningConfig,
) -> Result<ReasoningFileSnapshot, ReasoningConfigError> {
    let _guard = save_coordinator()
        .lock()
        .map_err(|_| coordinator_poisoned())?;
    save_if_unchanged_locked(path, expected, candidate)
}

/// Applies one exact profile patch through the process-wide save coordinator.
///
/// `base` is the authority and `expected` is the byte snapshot captured with
/// that authority. If another writer changed a disjoint field, the latest
/// file is merged with the patch. A same-field change is rejected with
/// [`ReasoningConfigError::ConcurrentModification`]. Unrelated profiles are
/// copied from the latest file, so a stale full-document snapshot can never
/// overwrite them.
pub fn save_profile_patch(
    path: &Path,
    expected: &ReasoningFileSnapshot,
    base: &ReasoningConfig,
    patch: &ReasoningProfilePatch,
) -> Result<ReasoningFileSnapshot, ReasoningConfigError> {
    base.validate()?;
    if patch.provider.trim().is_empty() || patch.model.trim().is_empty() {
        return Err(ReasoningConfigError::InvalidProfile(
            "profile patch provider/model is empty".to_string(),
        ));
    }
    let expected_config = if expected.exists {
        decode(&expected.bytes)?
    } else {
        ReasoningConfig::default()
    };
    if &expected_config != base {
        return Err(ReasoningConfigError::ConcurrentModification);
    }
    let _guard = save_coordinator()
        .lock()
        .map_err(|_| coordinator_poisoned())?;
    let current_snapshot = read_snapshot_from(path)?;
    if expected.exists && !current_snapshot.exists {
        return Err(ReasoningConfigError::ConcurrentModification);
    }
    let current = if current_snapshot.exists {
        decode(&current_snapshot.bytes)?
    } else {
        ReasoningConfig::default()
    };
    let candidate = merge_profile_patch(base, &current, patch)?;
    save_if_unchanged_locked(path, &current_snapshot, &candidate)
}

fn save_if_unchanged_locked(
    path: &Path,
    expected: &ReasoningFileSnapshot,
    candidate: &ReasoningConfig,
) -> Result<ReasoningFileSnapshot, ReasoningConfigError> {
    candidate.validate()?;
    let current = read_snapshot_from(path)?;
    if &current != expected {
        return Err(ReasoningConfigError::ConcurrentModification);
    }
    let document = crate::yaml_config::begin(path)?;
    if document.bytes().as_deref() != expected.exists.then_some(expected.bytes.as_slice()) {
        return Err(ReasoningConfigError::ConcurrentModification);
    }
    let bytes = document.save_fields(&serde_json::json!({"reasoning": candidate}))?;
    Ok(ReasoningFileSnapshot {
        exists: true,
        bytes,
    })
}

fn save_coordinator() -> &'static Mutex<()> {
    static COORDINATOR: OnceLock<Mutex<()>> = OnceLock::new();
    COORDINATOR.get_or_init(|| Mutex::new(()))
}

fn coordinator_poisoned() -> ReasoningConfigError {
    ReasoningConfigError::Io(io::Error::other("reasoning save coordinator is poisoned"))
}

fn decode(bytes: &[u8]) -> Result<ReasoningConfig, ReasoningConfigError> {
    let config: ReasoningConfig = crate::yaml_config::section(bytes, "reasoning")?;
    config.validate()?;
    Ok(config)
}

fn merge_profile_patch(
    base: &ReasoningConfig,
    current: &ReasoningConfig,
    patch: &ReasoningProfilePatch,
) -> Result<ReasoningConfig, ReasoningConfigError> {
    let base_profile = base.profile(&patch.provider, &patch.model);
    let current_profile = current.profile(&patch.provider, &patch.model);
    if base_profile.is_some() && current_profile.is_none() {
        return Err(ReasoningConfigError::ConcurrentModification);
    }
    let mut profile = current_profile
        .cloned()
        .unwrap_or_else(|| ReasoningProfile {
            provider: patch.provider.clone(),
            model: patch.model.clone(),
            preference: "provider_default".to_string(),
        });
    if let Some(desired) = &patch.preference {
        let base_preference = base_profile.map(|profile| &profile.preference);
        let current_preference = current_profile.map(|profile| &profile.preference);
        if current_preference != base_preference && current_preference != Some(desired) {
            return Err(ReasoningConfigError::ConcurrentModification);
        }
        profile.preference = desired.clone();
    }
    profile
        .validate()
        .map_err(ReasoningConfigError::InvalidProfile)?;
    let mut candidate = current.clone();
    candidate.upsert_profile(profile);
    candidate.validate()?;
    Ok(candidate)
}

pub fn encode(config: &ReasoningConfig) -> Result<Vec<u8>, ReasoningConfigError> {
    config.validate()?;
    serde_json::to_vec(&serde_json::json!({"schema_version": 1, "reasoning": config}))
        .map_err(|_| io::Error::other("reasoning serialization failed").into())
}
