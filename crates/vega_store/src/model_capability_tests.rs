use crate::model_capabilities::{
    ModelCapabilityConfig, ModelCapabilityPatch, ModelCapabilityRecord, encode,
    read_authority_from, read_from, save_model_patch,
};

fn record(
    provider: &str,
    model: &str,
    efforts: Option<&[&str]>,
    image: Option<bool>,
) -> ModelCapabilityRecord {
    ModelCapabilityRecord {
        provider: provider.into(),
        model: model.into(),
        allowed_efforts: efforts.map(|values| values.iter().map(|value| (*value).into()).collect()),
        image_input: image,
    }
}

fn write(path: &std::path::Path, records: Vec<ModelCapabilityRecord>) {
    std::fs::write(
        path,
        encode(&ModelCapabilityConfig {
            version: 1,
            models: records,
        })
        .unwrap(),
    )
    .unwrap();
}

#[test]
fn missing_capability_file_returns_inheritance_without_creating_any_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("model-capabilities.toml");
    let (config, snapshot) = read_authority_from(&path).unwrap();
    assert_eq!(config.version, 1);
    assert!(config.models.is_empty());
    assert!(!snapshot.exists);
    assert!(!path.exists());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn capability_codec_preserves_missing_empty_and_explicit_false_without_preferences() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("model-capabilities.toml");
    let config = ModelCapabilityConfig {
        version: 1,
        models: vec![
            record("p", "inherited", None, None),
            record("p", "empty", Some(&[]), Some(false)),
            record("p", "visual", Some(&["low", "high"]), Some(true)),
        ],
    };
    let bytes = encode(&config).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    assert_eq!(read_from(&path).unwrap(), config);
    let text = String::from_utf8(bytes).unwrap();
    let value: toml::Value = toml::from_str(&text).unwrap();
    assert_eq!(
        value
            .as_table()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>(),
        ["models", "version"].into_iter().collect()
    );
    for model in value["models"].as_array().unwrap() {
        assert!(model.as_table().unwrap().keys().all(|key| matches!(
            key.as_str(),
            "provider" | "model" | "allowed_efforts" | "image_input"
        )));
    }
    assert!(value["models"][0].get("allowed_efforts").is_none());
    assert!(value["models"][0].get("image_input").is_none());
    assert_eq!(
        value["models"][1]["allowed_efforts"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(value["models"][1]["image_input"].as_bool(), Some(false));
    assert!(!text.contains("preference"));
    assert!(!text.contains("protocol"));
    assert!(!text.contains("thinkingLevelMap"));
}

#[test]
fn capability_schema_rejects_invalid_documents_and_never_rewrites_them() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("model-capabilities.toml");
    let cases: Vec<Vec<u8>> = vec![
        b"version=2\n".to_vec(),
        b"version=1\nextra=true\n".to_vec(),
        b"version=1\n[[models]]\nprovider=''\nmodel='m'\n".to_vec(),
        b"version=1\n[[models]]\nprovider='p'\nmodel='  '\n".to_vec(),
        b"version=1\n[[models]]\nprovider='p'\nmodel='m'\nallowed_efforts=['high','high']\n"
            .to_vec(),
        b"version=1\n[[models]]\nprovider='p'\nmodel='m'\nallowed_efforts=['off']\n".to_vec(),
        b"version=1\n[[models]]\nprovider='p'\nmodel='m'\nallowed_efforts=['enabled']\n".to_vec(),
        b"version=1\n[[models]]\nprovider='p'\nmodel='m'\nallowed_efforts=['ultra']\n".to_vec(),
        b"version=1\n[[models]]\nprovider='p'\nmodel='m'\nimage_input='true'\n".to_vec(),
        b"version=1\n[[models]]\nprovider='p'\nmodel='m'\npreference='high'\n".to_vec(),
        b"version=1\n[[models]]\nprovider='p'\nmodel='m'\nprotocol='openai'\n".to_vec(),
        b"version=1\n[[models]]\nprovider='p'\nmodel='m'\nthinking_level_map={high='max'}\n"
            .to_vec(),
        b"version=1\n[[models]]\nprovider='p'\nmodel='m'\n[[models]]\nprovider='p'\nmodel='m'\n"
            .to_vec(),
        b"version=1\n[[models]\n".to_vec(),
        vec![0xff, 0xfe],
    ];
    for bytes in cases {
        std::fs::write(&path, &bytes).unwrap();
        assert!(
            read_authority_from(&path).is_err(),
            "{}",
            String::from_utf8_lossy(&bytes)
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn first_explicit_save_is_atomic_and_round_trips_only_the_requested_target() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("model-capabilities.toml");
    let (base, snapshot) = read_authority_from(&path).unwrap();
    let candidate = record("p", "m", Some(&["low", "high"]), None);
    let next = save_model_patch(
        &path,
        &snapshot,
        &base,
        &ModelCapabilityPatch::Upsert(candidate.clone()),
    )
    .unwrap();
    assert!(next.exists);
    assert_eq!(std::fs::read(&path).unwrap(), next.bytes);
    assert_eq!(read_from(&path).unwrap().models, vec![candidate]);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    assert!(!dir.path().join("reasoning-preferences.toml").exists());
}

#[test]
fn target_record_cas_merges_disjoint_external_models_and_preserves_orphans() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("model-capabilities.toml");
    let original = record("p", "m", Some(&["high"]), None);
    let orphan = record("removed", "old", None, Some(true));
    write(&path, vec![original.clone(), orphan.clone()]);
    let (base, snapshot) = read_authority_from(&path).unwrap();
    let other = record("p", "other", None, Some(false));
    let same_name = record("another", "m", Some(&["low"]), None);
    write(
        &path,
        vec![original, orphan.clone(), other.clone(), same_name.clone()],
    );
    let changed = record("p", "m", Some(&["low", "high"]), None);
    save_model_patch(
        &path,
        &snapshot,
        &base,
        &ModelCapabilityPatch::Upsert(changed.clone()),
    )
    .unwrap();
    let after = read_from(&path).unwrap();
    for expected in [changed, orphan, other, same_name] {
        assert!(after.models.contains(&expected));
    }
    assert_eq!(after.models.len(), 4);
}

#[test]
fn same_target_external_change_or_deletion_rejects_save_without_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("model-capabilities.toml");
    for external in [vec![record("p", "m", Some(&["low"]), None)], Vec::new()] {
        write(&path, vec![record("p", "m", Some(&["high"]), None)]);
        let (base, snapshot) = read_authority_from(&path).unwrap();
        write(&path, external);
        let before = std::fs::read(&path).unwrap();
        let result = save_model_patch(
            &path,
            &snapshot,
            &base,
            &ModelCapabilityPatch::Upsert(record("p", "m", Some(&[]), Some(true))),
        );
        assert!(result.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
    write(&path, vec![record("p", "m", Some(&["high"]), None)]);
    let (base, snapshot) = read_authority_from(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert!(
        save_model_patch(
            &path,
            &snapshot,
            &base,
            &ModelCapabilityPatch::Upsert(record("p", "m", Some(&[]), None))
        )
        .is_err()
    );
    assert!(!path.exists());
}

#[test]
fn reset_removes_exact_record_and_keeps_other_overrides_and_preference_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("model-capabilities.toml");
    let preferences = dir.path().join("reasoning-preferences.toml");
    let preference = b"version=1\n[[preferences]]\nprovider='p'\nmodel='m'\npreference='high'\n";
    std::fs::write(&preferences, preference).unwrap();
    let other = record("p", "other", None, Some(true));
    write(
        &path,
        vec![record("p", "m", Some(&[]), Some(false)), other.clone()],
    );
    let (base, snapshot) = read_authority_from(&path).unwrap();
    save_model_patch(
        &path,
        &snapshot,
        &base,
        &ModelCapabilityPatch::Reset {
            provider: "p".into(),
            model: "m".into(),
        },
    )
    .unwrap();
    assert_eq!(read_from(&path).unwrap().models, vec![other]);
    assert_eq!(std::fs::read(&preferences).unwrap(), preference);
}

#[test]
fn failed_save_keeps_existing_bytes_and_never_claims_readback_authority() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("model-capabilities.toml");
    write(&path, vec![record("p", "m", None, Some(true))]);
    let (base, snapshot) = read_authority_from(&path).unwrap();
    let before = std::fs::read(&path).unwrap();
    assert!(
        save_model_patch(
            &path,
            &snapshot,
            &base,
            &ModelCapabilityPatch::Upsert(record("p", "m", Some(&["invalid"]), None))
        )
        .is_err()
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    std::fs::write(&path, "invalid TOML [").unwrap();
    assert!(
        save_model_patch(
            &path,
            &snapshot,
            &base,
            &ModelCapabilityPatch::Reset {
                provider: "p".into(),
                model: "m".into()
            }
        )
        .is_err()
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "invalid TOML [");
}

#[test]
fn old_reasoning_documents_never_participate_in_capability_or_preference_loading() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("model-capabilities.toml");
    let old = dir.path().join("reasoning.toml");
    let preference = dir.path().join("reasoning-preferences.toml");
    for bytes in [
        b"version=1\n[[profiles]]\nprovider='p'\nmodel='m'\nprotocol='zhipu_glm'\n".as_slice(),
        b"version=2\n[[profiles]]\nprovider='p'\nmodel='m'\nreplay_profile='glm_preserved'\n"
            .as_slice(),
        b"broken [ TOML".as_slice(),
        b"version=1\nheaders={Authorization='do-not-use'}".as_slice(),
    ] {
        std::fs::write(&old, bytes).unwrap();
        assert!(read_from(&path).unwrap().models.is_empty());
        assert!(
            crate::reasoning::read_from(&preference)
                .unwrap()
                .profiles
                .is_empty()
        );
        assert!(!path.exists());
        assert!(!preference.exists());
        assert_eq!(std::fs::read(&old).unwrap(), bytes);
    }
}

#[test]
fn preference_and_capability_round_trips_are_independent_documents() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("model-capabilities.toml");
    let preference = dir.path().join("reasoning-preferences.toml");
    write(
        &path,
        vec![record("p", "m", Some(&["low", "high"]), Some(false))],
    );
    let capability_bytes = std::fs::read(&path).unwrap();
    let (base, snapshot) = crate::reasoning::read_authority_from(&preference).unwrap();
    crate::reasoning::save_profile_patch(
        &preference,
        &snapshot,
        &base,
        &crate::reasoning::ReasoningProfilePatch {
            provider: "p".into(),
            model: "m".into(),
            preference: Some("provider_default".into()),
        },
    )
    .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), capability_bytes);
    let value: toml::Value = toml::from_str(&std::fs::read_to_string(preference).unwrap()).unwrap();
    assert_eq!(
        value["preferences"][0]
            .as_table()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>(),
        ["provider", "model", "preference"].into_iter().collect()
    );
}

#[test]
fn capability_atomic_save_preserves_existing_file_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("model-capabilities.toml");
    write(&path, vec![record("p", "m", None, None)]);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o660)).unwrap();
    let (base, snapshot) = read_authority_from(&path).unwrap();
    save_model_patch(
        &path,
        &snapshot,
        &base,
        &ModelCapabilityPatch::Upsert(record("p", "m", Some(&["high"]), None)),
    )
    .unwrap();
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o660
    );
}
