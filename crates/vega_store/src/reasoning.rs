//! Versioned application-level thinking capability and preference storage.
//!
//! The file is deliberately independent from `config.toml`: a reasoning
//! worker never rewrites provider/default/permission settings from a stale
//! `AppConfig` snapshot. The store crate owns only the persistence-shaped
//! strings; conversion to runtime enums happens across the existing
//! crate boundary.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Current on-disk format version.
pub const REASONING_CONFIG_VERSION: u32 = 1;
/// File name under [`crate::paths::config_dir`].
pub const REASONING_FILE_NAME: &str = "reasoning.toml";

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
    /// A profile violates the explicit capability grammar.
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
    #[serde(default = "default_version")]
    pub version: u32,
    /// Explicit provider/model capability records and preferences.
    #[serde(default)]
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

/// Exact field patch for one provider/model profile.
///
/// `None` means the field is untouched. `disabled_wire` is therefore an
/// `Option<Option<String>>`: `None` leaves it alone while `Some(None)` clears
/// an existing declaration. Provider and model identify the target and are
/// never changed by a patch.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReasoningProfilePatch {
    /// Exact profile provider key.
    pub provider: String,
    /// Exact profile model key.
    pub model: String,
    /// New protocol, if edited.
    pub protocol: Option<String>,
    /// New support state, if edited.
    pub support: Option<String>,
    /// New effort list, if edited.
    pub efforts: Option<Vec<String>>,
    /// New disabled capability flag, if edited.
    pub supports_disabled: Option<bool>,
    /// New explicit disabled operation, if edited.
    pub disabled_wire: Option<Option<String>>,
    /// New replay requirement, if edited.
    pub preserve_reasoning_content: Option<bool>,
    /// New user preference, if edited.
    pub preference: Option<String>,
}

fn default_version() -> u32 {
    REASONING_CONFIG_VERSION
}

/// One explicit capability and preference for one exact provider/model pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReasoningProfile {
    /// Exact provider identifier.
    pub provider: String,
    /// Exact model identifier.
    pub model: String,
    /// `openai_chat_completions`, `zhipu_chat_completions`, or `unknown`.
    #[serde(default = "default_protocol")]
    pub protocol: String,
    /// `required`, `optional`, `unsupported`, or `unknown`.
    #[serde(default = "default_support")]
    pub support: String,
    /// Explicit legal effort values for this pair.
    #[serde(default)]
    pub efforts: Vec<String>,
    /// Whether a real disabled wire operation is declared.
    #[serde(default)]
    pub supports_disabled: bool,
    /// `thinking_type_disabled` or `reasoning_effort_none` when enabled.
    #[serde(default)]
    pub disabled_wire: Option<String>,
    /// Keep original GLM reasoning content for tool-round replay.
    #[serde(default)]
    pub preserve_reasoning_content: bool,
    /// `provider_default`, `disabled`, or one of `efforts`.
    #[serde(default = "default_preference")]
    pub preference: String,
}

impl ReasoningProfile {
    /// Validates explicit capability data before it can become runtime state.
    pub fn validate(&self) -> Result<(), String> {
        if self.provider.trim().is_empty() {
            return Err("provider is empty".to_string());
        }
        if self.model.trim().is_empty() {
            return Err("model is empty".to_string());
        }
        if !matches!(
            self.protocol.as_str(),
            "openai_chat_completions" | "zhipu_chat_completions" | "unknown"
        ) {
            return Err("protocol is not declared".to_string());
        }
        if !matches!(
            self.support.as_str(),
            "required" | "optional" | "unsupported" | "unknown"
        ) {
            return Err("support is not declared".to_string());
        }
        for effort in &self.efforts {
            if effort.trim().is_empty() {
                return Err("effort is empty".to_string());
            }
            if !matches!(
                effort.as_str(),
                "minimal" | "low" | "medium" | "high" | "xhigh" | "max"
            ) {
                return Err(format!("effort {effort} is not supported"));
            }
        }
        if self
            .efforts
            .iter()
            .enumerate()
            .any(|(index, effort)| self.efforts[..index].iter().any(|prior| prior == effort))
        {
            return Err("efforts contain duplicates".to_string());
        }
        if self.supports_disabled
            && !matches!(
                (self.protocol.as_str(), self.disabled_wire.as_deref()),
                ("openai_chat_completions", Some("reasoning_effort_none"))
                    | ("zhipu_chat_completions", Some("thinking_type_disabled"))
            )
        {
            return Err(
                "supports_disabled requires a disabled_wire matching the declared protocol"
                    .to_string(),
            );
        }
        if !self.supports_disabled && self.disabled_wire.is_some() {
            return Err("disabled_wire requires supports_disabled".to_string());
        }
        if self.preference != "provider_default"
            && self.preference != "disabled"
            && !self.efforts.iter().any(|effort| effort == &self.preference)
        {
            return Err("preference is not declared by efforts".to_string());
        }
        if self.preference == "disabled" && !self.supports_disabled {
            return Err("disabled preference requires supports_disabled".to_string());
        }
        if matches!(self.support.as_str(), "unsupported" | "unknown")
            && self.preference != "provider_default"
        {
            return Err("unsupported/unknown profile must use provider_default".to_string());
        }
        if self.protocol == "unknown"
            && (self.supports_disabled
                || !self.efforts.is_empty()
                || self.preserve_reasoning_content)
        {
            return Err("unknown protocol cannot declare wire controls or replay".to_string());
        }
        if self.protocol == "zhipu_chat_completions"
            && matches!(self.model.as_str(), "glm-5.3" | "glm-5.3-flash")
            && (self
                .efforts
                .iter()
                .any(|effort| !matches!(effort.as_str(), "low" | "high" | "max"))
                || self.supports_disabled
                || self.disabled_wire.is_some()
                || self.preference == "disabled")
        {
            return Err(
                "standard GLM 5.3 profiles support only low/high/max and cannot be disabled"
                    .to_string(),
            );
        }
        Ok(())
    }
}

fn default_protocol() -> String {
    "unknown".to_string()
}

fn default_support() -> String {
    "unknown".to_string()
}

fn default_preference() -> String {
    "provider_default".to_string()
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
    let snapshot = match fs::read(path) {
        Ok(bytes) => ReasoningFileSnapshot {
            exists: true,
            bytes,
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => ReasoningFileSnapshot::absent(),
        Err(error) => return Err(ReasoningConfigError::Io(error)),
    };
    let config = if snapshot.exists {
        decode(&snapshot.bytes)?
    } else {
        ReasoningConfig::default()
    };
    Ok((config, snapshot))
}

/// Reads the exact bytes used for a compare-before-rename.
pub fn read_snapshot_from(path: &Path) -> Result<ReasoningFileSnapshot, ReasoningConfigError> {
    match fs::read(path) {
        Ok(bytes) => Ok(ReasoningFileSnapshot {
            exists: true,
            bytes,
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Ok(ReasoningFileSnapshot::absent())
        }
        Err(error) => Err(ReasoningConfigError::Io(error)),
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
    let bytes = encode(candidate)?;
    write_atomic(path, &bytes, expected)?;
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
    let text = String::from_utf8(bytes.to_vec())?;
    let config: ReasoningConfig = toml::from_str(&text)?;
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
    let mut candidate = current.clone();
    let mut profile = current_profile.cloned().or_else(|| {
        base_profile.cloned().or_else(|| {
            Some(ReasoningProfile {
                provider: patch.provider.clone(),
                model: patch.model.clone(),
                protocol: default_protocol(),
                support: default_support(),
                efforts: Vec::new(),
                supports_disabled: false,
                disabled_wire: None,
                preserve_reasoning_content: false,
                preference: default_preference(),
            })
        })
    });
    let Some(ref mut profile) = profile else {
        return Err(ReasoningConfigError::InvalidProfile(
            "profile patch target is unavailable".to_string(),
        ));
    };

    merge_field(
        "protocol",
        base_profile.map(|profile| &profile.protocol),
        current_profile.map(|profile| &profile.protocol),
        patch.protocol.as_ref(),
        &mut profile.protocol,
    )?;
    merge_field(
        "support",
        base_profile.map(|profile| &profile.support),
        current_profile.map(|profile| &profile.support),
        patch.support.as_ref(),
        &mut profile.support,
    )?;
    merge_field(
        "efforts",
        base_profile.map(|profile| &profile.efforts),
        current_profile.map(|profile| &profile.efforts),
        patch.efforts.as_ref(),
        &mut profile.efforts,
    )?;
    merge_field(
        "supports_disabled",
        base_profile.map(|profile| &profile.supports_disabled),
        current_profile.map(|profile| &profile.supports_disabled),
        patch.supports_disabled.as_ref(),
        &mut profile.supports_disabled,
    )?;
    merge_field(
        "disabled_wire",
        base_profile.map(|profile| &profile.disabled_wire),
        current_profile.map(|profile| &profile.disabled_wire),
        patch.disabled_wire.as_ref(),
        &mut profile.disabled_wire,
    )?;
    merge_field(
        "preserve_reasoning_content",
        base_profile.map(|profile| &profile.preserve_reasoning_content),
        current_profile.map(|profile| &profile.preserve_reasoning_content),
        patch.preserve_reasoning_content.as_ref(),
        &mut profile.preserve_reasoning_content,
    )?;
    merge_field(
        "preference",
        base_profile.map(|profile| &profile.preference),
        current_profile.map(|profile| &profile.preference),
        patch.preference.as_ref(),
        &mut profile.preference,
    )?;

    // A profile that was removed externally is not recreated from stale
    // authority. This preserves the external version and makes the conflict
    // visible to the caller.
    if base_profile.is_some() && current_profile.is_none() {
        return Err(ReasoningConfigError::ConcurrentModification);
    }
    profile
        .validate()
        .map_err(ReasoningConfigError::InvalidProfile)?;
    candidate.upsert_profile(profile.clone());
    candidate.validate()?;
    Ok(candidate)
}

fn merge_field<T: Clone + PartialEq>(
    _name: &str,
    base: Option<&T>,
    current: Option<&T>,
    desired: Option<&T>,
    destination: &mut T,
) -> Result<(), ReasoningConfigError> {
    let Some(desired) = desired else {
        return Ok(());
    };
    if current != base && current != Some(desired) {
        return Err(ReasoningConfigError::ConcurrentModification);
    }
    *destination = desired.clone();
    Ok(())
}

/// Encodes a validated config with a stable header.
pub fn encode(config: &ReasoningConfig) -> Result<Vec<u8>, ReasoningConfigError> {
    config.validate()?;
    let body = toml::to_string_pretty(config)?;
    Ok(format!("# Vega reasoning capability configuration.\n{body}").into_bytes())
}

fn write_atomic(
    path: &Path,
    bytes: &[u8],
    expected: &ReasoningFileSnapshot,
) -> Result<(), ReasoningConfigError> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("reasoning config path has no parent"))?;
    fs::create_dir_all(parent)?;
    let tmp = unique_temp_path(path);
    let result = (|| -> Result<(), ReasoningConfigError> {
        let mut file = OpenOptions::new().create_new(true).write(true).open(&tmp)?;
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
        // This second read deliberately happens after the temporary file is
        // durable and immediately before rename. It narrows the race window
        // for non-cooperating writers but cannot provide an inter-process CAS.
        let current = read_snapshot_from(path)?;
        if &current != expected {
            return Err(ReasoningConfigError::ConcurrentModification);
        }
        fs::rename(&tmp, path)?;
        // Directory fsync closes the durability gap after rename on platforms
        // that support opening a directory as a File (macOS/Linux).
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn unique_temp_path(path: &Path) -> PathBuf {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos() as u64);
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("reasoning.toml");
    path.with_file_name(format!(
        ".{file_name}.tmp-{}-{sequence}-{nanos}",
        std::process::id()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn glm() -> ReasoningProfile {
        ReasoningProfile {
            provider: "zhipu".into(),
            model: "glm-5.3".into(),
            protocol: "zhipu_chat_completions".into(),
            support: "required".into(),
            efforts: vec!["low".into(), "high".into(), "max".into()],
            supports_disabled: false,
            disabled_wire: None,
            preserve_reasoning_content: true,
            preference: "max".into(),
        }
    }

    #[test]
    fn unknown_file_loads_provider_default_without_creating_it() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(REASONING_FILE_NAME);
        assert_eq!(read_from(&path).unwrap(), ReasoningConfig::default());
        assert!(!path.exists());
    }

    #[test]
    fn glm_standard_profile_rejects_disabled_and_preserves_declared_efforts() {
        let mut config = ReasoningConfig {
            version: 1,
            profiles: vec![glm()],
        };
        config.validate().unwrap();
        let mut disabled = glm();
        disabled.preference = "disabled".into();
        config.upsert_profile(disabled);
        assert!(config.validate().is_err());
        let mut disabled_capability = glm();
        disabled_capability.supports_disabled = true;
        disabled_capability.disabled_wire = Some("thinking_type_disabled".into());
        assert!(
            ReasoningConfig {
                version: 1,
                profiles: vec![disabled_capability],
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn disabled_wire_must_match_declared_protocol() {
        let mut openai = ReasoningProfile {
            provider: "openai".into(),
            model: "o4-mini".into(),
            protocol: "openai_chat_completions".into(),
            support: "optional".into(),
            efforts: vec!["low".into()],
            supports_disabled: true,
            disabled_wire: Some("thinking_type_disabled".into()),
            preserve_reasoning_content: false,
            preference: "provider_default".into(),
        };
        assert!(openai.validate().is_err());

        openai.disabled_wire = Some("reasoning_effort_none".into());
        openai.validate().unwrap();

        openai.protocol = "zhipu_chat_completions".into();
        assert!(openai.validate().is_err());
    }

    #[test]
    fn openai_profile_accepts_xhigh_but_standard_glm_rejects_it() {
        let mut openai = glm();
        openai.provider = "openai".into();
        openai.model = "o4-mini".into();
        openai.protocol = "openai_chat_completions".into();
        openai.support = "optional".into();
        openai.efforts = vec!["low".into(), "xhigh".into(), "max".into()];
        openai.preference = "xhigh".into();
        openai.preserve_reasoning_content = false;
        ReasoningConfig {
            version: 1,
            profiles: vec![openai],
        }
        .validate()
        .unwrap();

        let mut invalid_glm = glm();
        invalid_glm.efforts.push("xhigh".into());
        assert!(
            ReasoningConfig {
                version: 1,
                profiles: vec![invalid_glm],
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn unknown_protocol_cannot_claim_reasoning_replay() {
        let mut unknown = glm();
        unknown.protocol = "unknown".into();
        unknown.support = "unknown".into();
        unknown.efforts.clear();
        unknown.preference = "provider_default".into();
        assert!(
            ReasoningConfig {
                version: 1,
                profiles: vec![unknown],
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn compare_before_rename_preserves_unrelated_profiles_and_rejects_stale_writer() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(REASONING_FILE_NAME);
        let original = ReasoningConfig {
            version: 1,
            profiles: vec![glm()],
        };
        let first = ReasoningFileSnapshot::absent();
        let after_first = save_if_unchanged(&path, &first, &original).unwrap();
        let mut second_config = original.clone();
        let mut openai = glm();
        openai.provider = "openai".into();
        openai.model = "o4-mini".into();
        openai.protocol = "openai_chat_completions".into();
        openai.support = "optional".into();
        openai.preference = "provider_default".into();
        openai.preserve_reasoning_content = false;
        second_config.upsert_profile(openai);
        save_if_unchanged(&path, &after_first, &second_config).unwrap();
        let stale = save_if_unchanged(&path, &first, &original);
        assert!(matches!(
            stale,
            Err(ReasoningConfigError::ConcurrentModification)
        ));
        assert!(
            read_from(&path)
                .unwrap()
                .profile("zhipu", "glm-5.3")
                .is_some()
        );
        assert!(
            read_from(&path)
                .unwrap()
                .profile("openai", "o4-mini")
                .is_some()
        );
    }

    #[test]
    fn profile_patch_merges_disjoint_external_field_and_rejects_same_field() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(REASONING_FILE_NAME);
        let base = ReasoningConfig {
            version: 1,
            profiles: vec![glm()],
        };
        let expected = save_if_unchanged(&path, &ReasoningFileSnapshot::absent(), &base).unwrap();

        // A cooperative external writer changes preserve_reasoning_content;
        // the preference patch can merge on top without replacing it.
        let mut external = base.clone();
        external.profiles[0].preserve_reasoning_content = false;
        let external_snapshot = save_if_unchanged(&path, &expected, &external).unwrap();
        let merged = save_profile_patch(
            &path,
            &expected,
            &base,
            &ReasoningProfilePatch {
                provider: "zhipu".into(),
                model: "glm-5.3".into(),
                preference: Some("low".into()),
                ..ReasoningProfilePatch::default()
            },
        )
        .unwrap();
        let merged_config = read_from(&path).unwrap();
        assert_eq!(merged_config.profiles[0].preference, "low");
        assert!(!merged_config.profiles[0].preserve_reasoning_content);
        assert_ne!(merged, external_snapshot);

        // A same-field external edit cannot be overwritten by a stale draft.
        let mut same_field = merged_config.clone();
        same_field.profiles[0].preference = "high".into();
        let current = read_snapshot_from(&path).unwrap();
        save_if_unchanged(&path, &current, &same_field).unwrap();
        let conflict = save_profile_patch(
            &path,
            &merged,
            &merged_config,
            &ReasoningProfilePatch {
                provider: "zhipu".into(),
                model: "glm-5.3".into(),
                preference: Some("max".into()),
                ..ReasoningProfilePatch::default()
            },
        );
        assert!(matches!(
            conflict,
            Err(ReasoningConfigError::ConcurrentModification)
        ));
    }
}
