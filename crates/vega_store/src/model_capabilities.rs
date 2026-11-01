use serde::{Deserialize, Serialize};
use std::{
    io,
    path::Path,
    sync::{Mutex, OnceLock},
};

pub const MODEL_CAPABILITY_FILE_NAME: &str = "config.yaml";
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
pub struct ModelCapabilityRecord {
    pub provider: String,
    pub model: String,
    pub allowed_efforts: Option<Vec<String>>,
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
    pub(crate) fn validate(&self) -> Result<(), ModelCapabilityError> {
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
    match crate::yaml_config::read(path)? {
        Some(bytes) => Ok(ModelCapabilitySnapshot {
            exists: true,
            bytes,
        }),
        None => Ok(ModelCapabilitySnapshot::absent()),
    }
}
fn decode(
    snapshot: &ModelCapabilitySnapshot,
) -> Result<ModelCapabilityConfig, ModelCapabilityError> {
    if !snapshot.exists {
        return Ok(ModelCapabilityConfig::default());
    }
    let config: ModelCapabilityConfig =
        crate::yaml_config::section(&snapshot.bytes, "model_capabilities")?;
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
    serde_json::to_vec(&serde_json::json!({"schema_version": 1, "model_capabilities": config}))
        .map_err(|_| ModelCapabilityError::Invalid)
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
    config.validate()?;
    let document = crate::yaml_config::begin(path)?;
    if document.bytes().as_deref() != current.exists.then_some(current.bytes.as_slice()) {
        return Err(ModelCapabilityError::ConcurrentModification);
    }
    let bytes = document.save_fields(&serde_json::json!({"model_capabilities": config}))?;
    Ok(ModelCapabilitySnapshot {
        exists: true,
        bytes,
    })
}
