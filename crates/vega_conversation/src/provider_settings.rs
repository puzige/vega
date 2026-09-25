//! Background provider configuration and explicit network operation authority.
use crate::types::*;
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;
use vega_runtime::provider_check;
use vega_store::config::{self, AppConfig};

/// Owns only a configuration path; never retains credentials or performs render-time IO.
#[derive(Clone, Debug)]
pub struct ProviderSettingsService {
    config_path: PathBuf,
    #[cfg(any(test, feature = "test-support"))]
    test_transport: Option<provider_check::mock::Transport>,
}

impl ProviderSettingsService {
    /// Create a service for an explicitly resolved configuration file.
    pub fn new(config_path: PathBuf) -> Self {
        Self {
            config_path,
            #[cfg(any(test, feature = "test-support"))]
            test_transport: None,
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn with_test_transport(mut self, transport: provider_check::mock::Transport) -> Self {
        self.test_transport = Some(transport);
        self
    }

    /// Read current configuration on a background worker.
    pub fn load(&self) -> Result<AppConfig, ProviderSettingsError> {
        config::read_from(&self.config_path).map_err(|_| ProviderSettingsError::Config)
    }

    /// Apply one field-level mutation against its exact current provider baseline.
    pub fn patch(&self, request: ProviderPatchRequest) -> Result<AppConfig, ProviderSettingsError> {
        let mut edit =
            config::begin_edit(&self.config_path).map_err(|_| ProviderSettingsError::Config)?;
        let config = &mut edit.config;
        let index = provider_index(config, &request.provider)?;
        match request.action {
            ProviderPatchAction::SetEnabled(enabled) => config.providers[index].enabled = enabled,
            ProviderPatchAction::Move {
                expected_order,
                ordered_names,
            } => {
                let current: Vec<_> = config.providers.iter().map(|p| p.name.clone()).collect();
                if current != expected_order {
                    return Err(ProviderSettingsError::Conflict);
                }
                let mut sorted = ordered_names.clone();
                sorted.sort();
                sorted.dedup();
                let mut expected = current.clone();
                expected.sort();
                if sorted != expected || ordered_names.len() != current.len() {
                    return Err(ProviderSettingsError::Invalid);
                }
                let mut ordered = Vec::with_capacity(current.len());
                for name in ordered_names {
                    let provider = config
                        .providers
                        .iter()
                        .find(|p| p.name == name)
                        .ok_or(ProviderSettingsError::Invalid)?;
                    ordered.push(provider.clone());
                }
                config.providers = ordered;
            }
            ProviderPatchAction::ImportModels(models) => {
                validate_models(&models)?;
                for model in models {
                    if !config.providers[index].models.contains(&model) {
                        config.providers[index].models.push(model);
                    }
                }
                validate_models(&config.providers[index].models)?;
            }
            ProviderPatchAction::EditModels(models) => {
                validate_models(&models)?;
                config.providers[index].models = models;
            }
        }
        edit.save().map_err(|_| ProviderSettingsError::Config)?;
        Ok(edit.config.clone())
    }

    /// Save an explicit form edit under the same authority lock as narrow patches.
    /// Credential values are transient arguments, never part of shared DTOs.
    pub fn save_provider(
        &self,
        base: Option<ProviderSnapshot>,
        mut provider: ProviderSnapshot,
        key: Option<String>,
    ) -> Result<AppConfig, ProviderSettingsError> {
        let mut edit =
            config::begin_edit(&self.config_path).map_err(|_| ProviderSettingsError::Config)?;
        let config = &mut edit.config;
        if provider.name.trim().is_empty()
            || provider.name.len() > 256
            || provider.name.chars().any(char::is_control)
            || !provider_check::valid_base_url(&provider.base_url)
        {
            return Err(ProviderSettingsError::Invalid);
        }
        validate_models(&provider.models)?;
        let index = base
            .as_ref()
            .map(|base| provider_index(config, base))
            .transpose()?;
        if config
            .providers
            .iter()
            .enumerate()
            .any(|(i, p)| p.name == provider.name && Some(i) != index)
        {
            return Err(ProviderSettingsError::Conflict);
        }
        if let Some(base) = base {
            provider.enabled = base.enabled;
            provider.key_ref = base.key_ref;
        } else {
            provider.key_ref = provider.name.clone();
            if key.as_ref().is_none_or(|key| key.is_empty()) {
                return Err(ProviderSettingsError::Credential);
            }
        }
        let root = self
            .config_path
            .parent()
            .ok_or(ProviderSettingsError::Config)?;
        let previous_key = if key.is_some() {
            match vega_store::keystore::get_key(root, &provider.key_ref) {
                Ok(key) => Some(key),
                Err(vega_store::keystore::Error::Missing) => None,
                Err(_) => return Err(ProviderSettingsError::Credential),
            }
        } else {
            None
        };
        if let Some(key) = key.as_ref() {
            vega_store::keystore::set_key(root, &provider.key_ref, key)
                .map_err(|_| ProviderSettingsError::Credential)?;
        }
        let key_ref = provider.key_ref.clone();
        if let Some(index) = index {
            config.providers[index] = provider;
        } else {
            config.providers.push(provider);
        }
        if edit.save().is_err() {
            if key.is_some() {
                let restored = if let Some(previous) = previous_key {
                    vega_store::keystore::set_key(root, &key_ref, &previous)
                } else {
                    vega_store::keystore::delete_key(root, &key_ref)
                };
                if restored.is_err() {
                    return Err(ProviderSettingsError::Credential);
                }
            }
            return Err(ProviderSettingsError::Config);
        }
        Ok(edit.config.clone())
    }

    /// Remove an exact provider baseline while retaining its local credential.
    pub fn delete_provider(
        &self,
        base: ProviderSnapshot,
    ) -> Result<AppConfig, ProviderSettingsError> {
        let mut edit =
            config::begin_edit(&self.config_path).map_err(|_| ProviderSettingsError::Config)?;
        let config = &mut edit.config;
        let index = provider_index(config, &base)?;
        config.providers.remove(index);
        edit.save().map_err(|_| ProviderSettingsError::Config)?;
        Ok(edit.config.clone())
    }

    /// Resolve local credentials only for explicit actions and echo operation identity.
    pub async fn network(
        &self,
        request: ProviderNetworkRequest,
        cancel: CancellationToken,
    ) -> ProviderNetworkResult {
        let operation = self.perform_network(&request, cancel.clone());
        #[cfg(any(test, feature = "test-support"))]
        let mut outcome = match &self.test_transport {
            Some(transport) => transport.scope(operation).await,
            None => operation.await,
        };
        #[cfg(not(any(test, feature = "test-support")))]
        let mut outcome = operation.await;
        if outcome.is_ok() {
            let service = self.clone();
            let provider = request.provider.clone();
            let recheck = tokio::task::spawn_blocking(move || {
                let config = service.load()?;
                provider_index(&config, &provider).map(|_| ())
            });
            let current = tokio::select! {
                biased;
                _ = cancel.cancelled() => Err(ProviderSettingsError::Cancelled),
                result = recheck => result.unwrap_or(Err(ProviderSettingsError::Config)),
            };
            if let Err(error) = current {
                outcome = Err(error);
            }
        }
        ProviderNetworkResult { request, outcome }
    }

    async fn perform_network(
        &self,
        request: &ProviderNetworkRequest,
        cancel: CancellationToken,
    ) -> Result<ProviderNetworkOutcome, ProviderSettingsError> {
        if cancel.is_cancelled() {
            return Err(ProviderSettingsError::Cancelled);
        }
        let service = self.clone();
        let provider = request.provider.clone();
        let prepared = tokio::task::spawn_blocking(move || {
            let config = service.load()?;
            provider_index(&config, &provider)?;
            if !provider.enabled {
                return Err(ProviderSettingsError::Disabled);
            }
            if !provider_check::valid_base_url(&provider.base_url) {
                return Err(ProviderSettingsError::Invalid);
            }
            let root = service
                .config_path
                .parent()
                .ok_or(ProviderSettingsError::Config)?;
            vega_store::keystore::get_key(root, &provider.key_ref)
                .map_err(|_| ProviderSettingsError::Credential)
        });
        let key = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(ProviderSettingsError::Cancelled),
            result = prepared => result.map_err(|_| ProviderSettingsError::Config)??,
        };
        let result = match &request.action {
            ProviderNetworkAction::DiscoverModels => {
                provider_check::discover(&request.provider.base_url, &key, cancel)
                    .await
                    .map(ProviderNetworkOutcome::Models)
            }
            ProviderNetworkAction::TestModel { model } => {
                if !request.provider.models.contains(model) {
                    return Err(ProviderSettingsError::Invalid);
                }
                provider_check::probe(&request.provider.base_url, &key, model, cancel)
                    .await
                    .map(|()| ProviderNetworkOutcome::ModelTestSucceeded)
            }
        };
        result.map_err(|error| match error {
            provider_check::CheckError::Invalid => ProviderSettingsError::Invalid,
            provider_check::CheckError::Cancelled => ProviderSettingsError::Cancelled,
            provider_check::CheckError::Timeout => ProviderSettingsError::Timeout,
            provider_check::CheckError::Network => ProviderSettingsError::Network,
            provider_check::CheckError::Http(status) => ProviderSettingsError::Http(status),
            provider_check::CheckError::Malformed => ProviderSettingsError::Malformed,
            provider_check::CheckError::Limit => ProviderSettingsError::Limit,
        })
    }
}

fn provider_index(
    config: &AppConfig,
    provider: &ProviderSnapshot,
) -> Result<usize, ProviderSettingsError> {
    let matches: Vec<_> = config
        .providers
        .iter()
        .enumerate()
        .filter(|(_, p)| p.name == provider.name)
        .collect();
    match matches.as_slice() {
        [(index, current)] if *current == provider => Ok(*index),
        _ => Err(ProviderSettingsError::Conflict),
    }
}

fn validate_models(models: &[String]) -> Result<(), ProviderSettingsError> {
    if models.len() > 1000 {
        return Err(ProviderSettingsError::Limit);
    }
    for (index, model) in models.iter().enumerate() {
        if !provider_check::valid_model_id(model) || models[..index].contains(model) {
            return Err(ProviderSettingsError::Invalid);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
