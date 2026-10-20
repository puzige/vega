use super::*;
use crate::ModelCapabilitySettingsService;
use crate::types::{ModelCapabilityPatch, ModelCapabilitySettingsErrorCode};
use vega_runtime::model_compat::{ModelCapabilityOverride, ModelIdentity};
use vega_store::model_capabilities::{ModelCapabilityConfig, ModelCapabilityRecord};

fn config(root: &std::path::Path, endpoint: &str, names: &[&str]) -> std::path::PathBuf {
    let path = root.join("config.toml");
    let providers = names
        .iter()
        .map(|provider| vega_store::config::ProviderConfig {
            name: (*provider).into(),
            base_url: endpoint.into(),
            models: vec!["model".into(), "second".into()],
            key_ref: "owned-key".into(),
            api: vega_store::config::ProviderApi::ChatCompletions,
            enabled: true,
        })
        .collect();
    vega_store::config::AppConfig {
        providers,
        ..Default::default()
    }
    .save_to(&path)
    .unwrap();
    path
}

fn patch(
    provider: &str,
    name: &str,
    efforts: Option<&[&str]>,
    image: Option<bool>,
) -> ModelCapabilityPatch {
    ModelCapabilityPatch::Upsert(ModelCapabilityOverride {
        identity: ModelIdentity {
            provider: provider.into(),
            model: name.into(),
        },
        allowed_efforts: efforts.map(|levels| levels.iter().map(|level| (*level).into()).collect()),
        image_input: image,
    })
}

#[test]
fn capability_service_load_save_reset_resolves_exact_models_and_keeps_preferences_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let path = config(
        dir.path(),
        "https://unknown.invalid/v1",
        &["first", "other"],
    );
    let pref = dir.path().join("reasoning-preferences.toml");
    let bytes = b"version=1\n[[preferences]]\nprovider='first'\nmodel='model'\npreference='high'\n";
    std::fs::write(&pref, bytes).unwrap();
    let service = ModelCapabilitySettingsService::new(path);
    let base = service.load().unwrap();
    assert_eq!(base.models.len(), 4);
    assert!(!dir.path().join("model-capabilities.toml").exists());
    let saved = service
        .save(
            &base,
            &patch("first", "model", Some(&["low", "high"]), Some(true)),
        )
        .unwrap();
    let selected = saved
        .models
        .iter()
        .find(|projection| {
            projection.resolved.identity
                == ModelIdentity {
                    provider: "first".into(),
                    model: "model".into(),
                }
        })
        .unwrap();
    assert_eq!(
        selected.resolved.capabilities.allowed_efforts,
        vec!["low", "high"]
    );
    assert!(selected.resolved.capabilities.image_input);
    assert!(
        saved
            .models
            .iter()
            .filter(|projection| projection.resolved.identity != selected.resolved.identity)
            .all(
                |projection| projection.resolved.capabilities.allowed_efforts.len() == 6
                    && !projection.resolved.capabilities.image_input
            )
    );
    assert_eq!(std::fs::read(&pref).unwrap(), bytes);
    let reset = service
        .save(
            &saved,
            &ModelCapabilityPatch::Reset {
                identity: selected.resolved.identity.clone(),
            },
        )
        .unwrap();
    assert!(reset.models.iter().all(|projection| {
        projection.resolved.capabilities.allowed_efforts.len() == 6
            && !projection.resolved.capabilities.image_input
    }));
    assert_eq!(std::fs::read(&pref).unwrap(), bytes);
}

#[test]
fn service_rejects_nonencodable_level_without_writing_or_altering_preferences() {
    let dir = tempfile::tempdir().unwrap();
    let path = config(dir.path(), "https://api.z.ai/api/paas/v4", &["fixture"]);
    let mut configured = vega_store::config::read_from(&path).unwrap();
    configured.providers[0].models = vec!["glm-4.7".into()];
    configured.save_to(&path).unwrap();
    let service = ModelCapabilitySettingsService::new(path);
    let base = service.load().unwrap();
    assert!(
        !base.models[0]
            .resolved
            .capabilities
            .encodable_efforts
            .contains(&"max".into())
    );
    assert_eq!(
        service
            .save(&base, &patch("fixture", "glm-4.7", Some(&["max"]), None))
            .unwrap_err(),
        ModelCapabilitySettingsErrorCode::Invalid
    );
    assert!(!dir.path().join("model-capabilities.toml").exists());
    assert!(!dir.path().join("reasoning-preferences.toml").exists());
}

#[test]
fn service_reloads_exact_provider_route_and_never_borrows_same_named_factory_model() {
    let dir = tempfile::tempdir().unwrap();
    let path = config(dir.path(), "https://unknown.invalid/v1", &["fixture"]);
    let mut configured = vega_store::config::read_from(&path).unwrap();
    configured.providers[0].models = vec!["glm-4.7".into()];
    configured.save_to(&path).unwrap();
    let service = ModelCapabilitySettingsService::new(path.clone());
    let unknown = service.load().unwrap();
    assert_eq!(
        unknown.models[0]
            .resolved
            .capabilities
            .allowed_efforts
            .len(),
        6
    );
    configured.providers[0].base_url = "https://api.z.ai/api/paas/v4".into();
    configured.save_to(&path).unwrap();
    let known = service.load().unwrap();
    assert_ne!(
        unknown.models[0].resolved.fingerprint,
        known.models[0].resolved.fingerprint
    );
    assert!(known.models[0].resolved.capabilities.allowed_efforts.len() < 6);
}

#[test]
fn orphan_records_do_not_create_models_or_vanish_when_other_models_are_saved() {
    let dir = tempfile::tempdir().unwrap();
    let path = config(dir.path(), "https://unknown.invalid/v1", &["fixture"]);
    let capability = dir.path().join("model-capabilities.toml");
    let orphan = ModelCapabilityRecord {
        provider: "gone".into(),
        model: "old".into(),
        allowed_efforts: Some(vec!["low".into()]),
        image_input: Some(true),
    };
    std::fs::write(
        &capability,
        vega_store::model_capabilities::encode(&ModelCapabilityConfig {
            version: 1,
            models: vec![orphan.clone()],
        })
        .unwrap(),
    )
    .unwrap();
    let service = ModelCapabilitySettingsService::new(path);
    let base = service.load().unwrap();
    assert_eq!(base.models.len(), 2);
    assert!(
        base.models
            .iter()
            .all(|model| model.resolved.identity.provider == "fixture")
    );
    assert_eq!(
        service
            .save(&base, &patch("gone", "old", Some(&["high"]), None))
            .unwrap_err(),
        ModelCapabilitySettingsErrorCode::MissingModel
    );
    service
        .save(&base, &patch("fixture", "model", None, Some(true)))
        .unwrap();
    assert!(
        vega_store::model_capabilities::read_from(&capability)
            .unwrap()
            .models
            .contains(&orphan)
    );
}

#[test]
fn service_rejects_removed_target_and_stale_same_target_but_merges_other_records() {
    let dir = tempfile::tempdir().unwrap();
    let path = config(dir.path(), "https://unknown.invalid/v1", &["fixture"]);
    let service = ModelCapabilitySettingsService::new(path.clone());
    let base = service.load().unwrap();
    let other = service
        .save(&base, &patch("fixture", "second", None, Some(true)))
        .unwrap();
    let saved = service
        .save(&base, &patch("fixture", "model", Some(&["high"]), None))
        .unwrap();
    assert!(
        saved
            .models
            .iter()
            .find(|projection| projection.resolved.identity.model == "second")
            .unwrap()
            .resolved
            .capabilities
            .image_input
    );
    assert_eq!(
        service
            .save(&other, &patch("fixture", "model", Some(&["low"]), None))
            .unwrap_err(),
        ModelCapabilitySettingsErrorCode::Conflict
    );
    let mut cfg = vega_store::config::read_from(&path).unwrap();
    cfg.providers[0].models = vec!["second".into()];
    cfg.save_to(&path).unwrap();
    let before = std::fs::read(dir.path().join("model-capabilities.toml")).unwrap();
    assert_eq!(
        service
            .save(&saved, &patch("fixture", "model", None, Some(true)))
            .unwrap_err(),
        ModelCapabilitySettingsErrorCode::MissingModel
    );
    assert_eq!(
        std::fs::read(dir.path().join("model-capabilities.toml")).unwrap(),
        before
    );
}

#[test]
fn corrupt_new_capability_authority_blocks_real_loading_while_old_file_never_participates() {
    let dir = tempfile::tempdir().unwrap();
    let path = config(dir.path(), "https://unknown.invalid/v1", &["fixture"]);
    let service = ModelCapabilitySettingsService::new(path);
    let old = dir.path().join("reasoning.toml");
    for bytes in [
        "version=1\n[[profiles]]\nprovider='fixture'\nmodel='model'\nprotocol='openai'",
        "version=2\n[[profiles]]\nprovider='fixture'\nmodel='model'\nreplay_profile='glm_preserved'",
        "broken [ TOML",
    ] {
        std::fs::write(&old, bytes).unwrap();
        assert_eq!(service.load().unwrap().models.len(), 2);
        assert_eq!(std::fs::read_to_string(&old).unwrap(), bytes);
        assert!(!dir.path().join("model-capabilities.toml").exists());
    }
    std::fs::write(dir.path().join("model-capabilities.toml"), "version=7\n").unwrap();
    assert!(service.load().is_err());
}

#[test]
fn loading_capabilities_ignores_preference_value_but_never_changes_its_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let path = config(dir.path(), "https://unknown.invalid/v1", &["fixture"]);
    let pref = dir.path().join("reasoning-preferences.toml");
    std::fs::write(&pref, "malformed preference [").unwrap();
    let before = std::fs::read(&pref).unwrap();
    let service = ModelCapabilitySettingsService::new(path);
    let base = service.load().unwrap();
    service
        .save(&base, &patch("fixture", "model", Some(&[]), None))
        .unwrap();
    assert_eq!(std::fs::read(&pref).unwrap(), before);
}

#[test]
fn real_provider_configuration_add_replace_remove_updates_exact_model_resolution() {
    let dir = tempfile::tempdir().unwrap();
    let path = config(dir.path(), "https://unknown.invalid/v1", &["original"]);
    let providers = crate::ProviderSettingsService::new(path.clone());
    let candidate = vega_store::config::ProviderConfig {
        name: "added".into(),
        base_url: "https://another.invalid/v1".into(),
        models: vec!["custom/model".into()],
        key_ref: String::new(),
        api: vega_store::config::ProviderApi::Responses,
        enabled: true,
    };
    providers
        .save_provider(None, candidate, Some("owned-local-credential".into()))
        .unwrap();
    let service = ModelCapabilitySettingsService::new(path.clone());
    let loaded = service.load().unwrap();
    let added = loaded
        .models
        .iter()
        .find(|projection| projection.resolved.identity.provider == "added")
        .unwrap();
    assert_eq!(added.resolved.identity.model, "custom/model");
    assert_eq!(added.resolved.capabilities.allowed_efforts.len(), 6);
    let old_fingerprint = added.resolved.fingerprint.clone();
    let base = providers
        .load()
        .unwrap()
        .providers
        .into_iter()
        .find(|provider| provider.name == "added")
        .unwrap();
    let mut changed = base.clone();
    changed.base_url = "https://different.invalid/v1".into();
    providers
        .save_provider(Some(base), changed.clone(), None)
        .unwrap();
    let refreshed = service.load().unwrap();
    let added = refreshed
        .models
        .iter()
        .find(|projection| projection.resolved.identity.provider == "added")
        .unwrap();
    assert_ne!(added.resolved.fingerprint, old_fingerprint);
    let saved = service
        .save(
            &refreshed,
            &patch("added", "custom/model", Some(&["high"]), None),
        )
        .unwrap();
    assert!(
        saved
            .models
            .iter()
            .any(|model| model.resolved.identity.provider == "original")
    );
    providers.delete_provider(changed).unwrap();
    let remaining = service.load().unwrap();
    assert!(
        remaining
            .models
            .iter()
            .all(|projection| projection.resolved.identity.provider == "original")
    );
    assert!(
        vega_store::model_capabilities::read_from(&dir.path().join("model-capabilities.toml"))
            .unwrap()
            .models
            .iter()
            .any(|record| record.provider == "added")
    );
    assert!(
        !std::fs::read_to_string(&path)
            .unwrap()
            .contains("owned-local-credential")
    );
}
