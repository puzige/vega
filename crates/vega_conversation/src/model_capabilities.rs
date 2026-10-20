use crate::types::{
    ModelCapabilityPatch, ModelCapabilityProjection, ModelCapabilitySettingsErrorCode,
};
use std::path::PathBuf;
use vega_runtime::model_compat::{
    ModelApi, ModelCapabilityOverride, ModelIdentity, ModelRoute, ResolvedModel,
    lookup_model_definition, resolve_model,
};
use vega_store::{
    config::{AppConfig, ProviderApi},
    model_capabilities::{
        self, ModelCapabilityConfig, ModelCapabilityRecord, ModelCapabilitySnapshot,
    },
};

#[derive(Debug, Clone)]
pub struct ModelCapabilityAuthority {
    pub models: Vec<ModelCapabilityProjection>,
    config: ModelCapabilityConfig,
    snapshot: ModelCapabilitySnapshot,
}

pub struct ModelCapabilitySettingsService {
    config_path: PathBuf,
}
impl ModelCapabilitySettingsService {
    pub fn new(config_path: PathBuf) -> Self {
        Self { config_path }
    }
    fn capability_path(&self) -> PathBuf {
        self.config_path
            .with_file_name(model_capabilities::MODEL_CAPABILITY_FILE_NAME)
    }
    pub fn load(&self) -> Result<ModelCapabilityAuthority, ModelCapabilitySettingsErrorCode> {
        let config = vega_store::config::read_from(&self.config_path)
            .map_err(|_| ModelCapabilitySettingsErrorCode::Io)?;
        let (caps, snapshot) = model_capabilities::read_authority_from(&self.capability_path())
            .map_err(store_error)?;
        let models = config
            .providers
            .iter()
            .flat_map(|provider| provider.models.iter().map(move |model| (provider, model)))
            .map(|(provider, model)| {
                let resolved = resolve_configured_model(&config, &caps, &provider.name, model)?;
                let user_override = caps.record(&provider.name, model).map(record_override);
                Ok(ModelCapabilityProjection {
                    resolved: Box::new(resolved),
                    user_override,
                })
            })
            .collect::<Result<Vec<_>, ModelCapabilitySettingsErrorCode>>()?;
        Ok(ModelCapabilityAuthority {
            models,
            config: caps,
            snapshot,
        })
    }
    pub fn save(
        &self,
        base: &ModelCapabilityAuthority,
        patch: &ModelCapabilityPatch,
    ) -> Result<ModelCapabilityAuthority, ModelCapabilitySettingsErrorCode> {
        let identity = match patch {
            ModelCapabilityPatch::Upsert(v) => &v.identity,
            ModelCapabilityPatch::Reset { identity } => identity,
        };
        let config = vega_store::config::read_from(&self.config_path)
            .map_err(|_| ModelCapabilitySettingsErrorCode::Io)?;
        let provider = config
            .providers
            .iter()
            .find(|p| p.name == identity.provider && p.models.contains(&identity.model))
            .ok_or(ModelCapabilitySettingsErrorCode::MissingModel)?;
        let definition = lookup_model_definition(
            &ModelRoute::new(api(provider.api), &provider.base_url)
                .map_err(|_| ModelCapabilitySettingsErrorCode::Invalid)?,
            identity,
        )
        .map_err(|_| ModelCapabilitySettingsErrorCode::Invalid)?;
        let store_patch = match patch {
            ModelCapabilityPatch::Upsert(value) => {
                resolve_model(&definition, Some(value))
                    .map_err(|_| ModelCapabilitySettingsErrorCode::Invalid)?;
                model_capabilities::ModelCapabilityPatch::Upsert(ModelCapabilityRecord {
                    provider: identity.provider.clone(),
                    model: identity.model.clone(),
                    allowed_efforts: value.allowed_efforts.clone(),
                    image_input: value.image_input,
                })
            }
            ModelCapabilityPatch::Reset { .. } => model_capabilities::ModelCapabilityPatch::Reset {
                provider: identity.provider.clone(),
                model: identity.model.clone(),
            },
        };
        model_capabilities::save_model_patch(
            &self.capability_path(),
            &base.snapshot,
            &base.config,
            &store_patch,
        )
        .map_err(store_error)?;
        self.load()
    }
}

pub fn api(api: ProviderApi) -> ModelApi {
    match api {
        ProviderApi::ChatCompletions => ModelApi::ChatCompletions,
        ProviderApi::Responses => ModelApi::Responses,
    }
}
fn record_override(record: &ModelCapabilityRecord) -> ModelCapabilityOverride {
    ModelCapabilityOverride {
        identity: ModelIdentity {
            provider: record.provider.clone(),
            model: record.model.clone(),
        },
        allowed_efforts: record.allowed_efforts.clone(),
        image_input: record.image_input,
    }
}
pub fn resolve_configured_model(
    config: &AppConfig,
    caps: &ModelCapabilityConfig,
    provider: &str,
    model: &str,
) -> Result<ResolvedModel, ModelCapabilitySettingsErrorCode> {
    let p = config
        .providers
        .iter()
        .find(|p| p.name == provider && p.models.iter().any(|v| v == model))
        .ok_or(ModelCapabilitySettingsErrorCode::MissingModel)?;
    let identity = ModelIdentity {
        provider: provider.into(),
        model: model.into(),
    };
    let route = ModelRoute::new(api(p.api), &p.base_url)
        .map_err(|_| ModelCapabilitySettingsErrorCode::Invalid)?;
    let definition = lookup_model_definition(&route, &identity)
        .map_err(|_| ModelCapabilitySettingsErrorCode::Invalid)?;
    resolve_model(
        &definition,
        caps.record(provider, model).map(record_override).as_ref(),
    )
    .map_err(|_| ModelCapabilitySettingsErrorCode::Invalid)
}
pub fn load_configured_model(
    config: &AppConfig,
    path: &std::path::Path,
    provider: &str,
    model: &str,
) -> Result<ResolvedModel, ModelCapabilitySettingsErrorCode> {
    let caps = model_capabilities::read_from(
        &path.with_file_name(model_capabilities::MODEL_CAPABILITY_FILE_NAME),
    )
    .map_err(store_error)?;
    resolve_configured_model(config, &caps, provider, model)
}
fn store_error(
    error: model_capabilities::ModelCapabilityError,
) -> ModelCapabilitySettingsErrorCode {
    match error {
        model_capabilities::ModelCapabilityError::Io(_) => ModelCapabilitySettingsErrorCode::Io,
        model_capabilities::ModelCapabilityError::ConcurrentModification => {
            ModelCapabilitySettingsErrorCode::Conflict
        }
        _ => ModelCapabilitySettingsErrorCode::Invalid,
    }
}
