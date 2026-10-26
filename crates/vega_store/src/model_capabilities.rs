use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

pub const MODEL_CAPABILITY_FILE_NAME: &str = "model-capabilities.toml";
pub type ModelCapabilitySnapshot = crate::reasoning::ReasoningFileSnapshot;

#[derive(Debug, thiserror::Error)]
pub enum ModelCapabilityError {
    #[error("model capabilities io error")]
    Io(#[from] io::Error),
    #[error("model capabilities parse error")]
    Parse(#[from] toml::de::Error),
    #[error("model capabilities serialization error")]
    Serialize(#[from] toml::ser::Error),
    #[error("model capabilities are invalid")]
    Invalid,
    #[error("model capabilities changed concurrently")]
    ConcurrentModification,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelCapabilityConfig {
    pub version: u32,
    #[serde(default)]
    pub models: Vec<ModelCapabilityRecord>,
}
impl Default for ModelCapabilityConfig {
    fn default() -> Self {
        Self {
            version: 1,
            models: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelCapabilityRecord {
    pub provider: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_efforts: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_input: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelCapabilityPatch {
    Upsert(ModelCapabilityRecord),
    Reset { provider: String, model: String },
}
impl ModelCapabilityPatch {
    fn identity(&self) -> (&str, &str) {
        match self {
            Self::Upsert(r) => (&r.provider, &r.model),
            Self::Reset { provider, model } => (provider, model),
        }
    }
}

impl ModelCapabilityConfig {
    pub fn record(&self, provider: &str, model: &str) -> Option<&ModelCapabilityRecord> {
        self.models
            .iter()
            .find(|r| r.provider == provider && r.model == model)
    }
    fn validate(&self) -> Result<(), ModelCapabilityError> {
        if self.version != 1 {
            return Err(ModelCapabilityError::Invalid);
        }
        for (index, record) in self.models.iter().enumerate() {
            if record.provider.trim().is_empty()
                || record.model.trim().is_empty()
                || self.models[..index]
                    .iter()
                    .any(|r| r.provider == record.provider && r.model == record.model)
                || record.allowed_efforts.as_ref().is_some_and(|values| {
                    values.iter().enumerate().any(|(i, v)| {
                        !matches!(
                            v.as_str(),
                            "minimal" | "low" | "medium" | "high" | "xhigh" | "max"
                        ) || values[..i].contains(v)
                    })
                })
            {
                return Err(ModelCapabilityError::Invalid);
            }
        }
        Ok(())
    }
}

fn snapshot(path: &Path) -> Result<ModelCapabilitySnapshot, ModelCapabilityError> {
    match fs::read(path) {
        Ok(bytes) => Ok(ModelCapabilitySnapshot {
            exists: true,
            bytes,
        }),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(ModelCapabilitySnapshot::absent()),
        Err(e) => Err(e.into()),
    }
}
fn decode(
    snapshot: &ModelCapabilitySnapshot,
) -> Result<ModelCapabilityConfig, ModelCapabilityError> {
    if !snapshot.exists {
        return Ok(ModelCapabilityConfig::default());
    }
    let text = std::str::from_utf8(&snapshot.bytes).map_err(|_| ModelCapabilityError::Invalid)?;
    let config: ModelCapabilityConfig = toml::from_str(text)?;
    config.validate()?;
    Ok(config)
}
pub fn read_from(path: &Path) -> Result<ModelCapabilityConfig, ModelCapabilityError> {
    read_authority_from(path).map(|(config, _)| config)
}
pub fn read_authority_from(
    path: &Path,
) -> Result<(ModelCapabilityConfig, ModelCapabilitySnapshot), ModelCapabilityError> {
    let snapshot = snapshot(path)?;
    let config = decode(&snapshot)?;
    Ok((config, snapshot))
}
pub fn encode(config: &ModelCapabilityConfig) -> Result<Vec<u8>, ModelCapabilityError> {
    config.validate()?;
    Ok(toml::to_string_pretty(config)?.into_bytes())
}

pub fn save_model_patch(
    path: &Path,
    expected: &ModelCapabilitySnapshot,
    base: &ModelCapabilityConfig,
    patch: &ModelCapabilityPatch,
) -> Result<ModelCapabilitySnapshot, ModelCapabilityError> {
    static COORDINATOR: OnceLock<Mutex<()>> = OnceLock::new();
    let _guard = COORDINATOR
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| {
            ModelCapabilityError::Io(io::Error::other("model capability save unavailable"))
        })?;
    base.validate()?;
    if &decode(expected)? != base {
        return Err(ModelCapabilityError::ConcurrentModification);
    }
    let (provider, model) = patch.identity();
    if provider.trim().is_empty() || model.trim().is_empty() {
        return Err(ModelCapabilityError::Invalid);
    }
    let current = snapshot(path)?;
    if expected.exists && !current.exists {
        return Err(ModelCapabilityError::ConcurrentModification);
    }
    let mut config = decode(&current)?;
    if config.record(provider, model) != base.record(provider, model) {
        return Err(ModelCapabilityError::ConcurrentModification);
    }
    config
        .models
        .retain(|r| r.provider != provider || r.model != model);
    if let ModelCapabilityPatch::Upsert(record) = patch {
        config.models.push(record.clone());
    }
    let bytes = encode(&config)?;
    write_atomic(path, &current, &bytes)?;
    Ok(ModelCapabilitySnapshot {
        exists: true,
        bytes,
    })
}

fn write_atomic(
    path: &Path,
    expected: &ModelCapabilitySnapshot,
    bytes: &[u8],
) -> Result<(), ModelCapabilityError> {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("model capability parent unavailable"))?;
    fs::create_dir_all(parent)?;
    let name = path
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or(MODEL_CAPABILITY_FILE_NAME);
    let temp: PathBuf = path.with_file_name(format!(
        ".{name}.tmp-{}-{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)?;
        if expected.exists {
            file.set_permissions(fs::metadata(path)?.permissions())?;
        }
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
        if &snapshot(path)? != expected {
            return Err(ModelCapabilityError::ConcurrentModification);
        }
        fs::rename(&temp, path)?;
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}
