use std::path::PathBuf;
use vega_store::{config, model_capabilities, paths::AppPaths, reasoning, yaml_config};

struct Installation(PathBuf);

impl Installation {
    fn new() -> Self {
        Self(
            std::env::temp_dir()
                .canonicalize()
                .expect("temporary root")
                .join(format!("vega-yaml-{}", ulid::Ulid::generate())),
        )
    }

    fn config(&self) -> PathBuf {
        self.0.join("config.yaml")
    }
}

impl Drop for Installation {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn explicit_root_isolated_and_invalid_override_never_falls_back() {
    let root = Installation::new();
    let paths = AppPaths::from_home(None, Some(root.0.as_os_str())).expect("absolute override");
    assert_eq!(paths.database_file(), root.0.join("data/vega.db"));
    assert_eq!(paths.config_file(), root.config());
    assert!(paths.is_explicit());
    assert!(!root.0.exists());
    assert!(AppPaths::from_home(Some(&root.0), Some(std::ffi::OsStr::new(""))).is_err());
    assert!(AppPaths::from_home(Some(&root.0), Some(std::ffi::OsStr::new("relative"))).is_err());
    assert!(AppPaths::from_home(None, None).is_err());
}

#[test]
fn unchanged_save_retains_exact_bytes_and_changed_field_preserves_comments() {
    let root = Installation::new();
    let path = root.config();
    config::load_from(&path).expect("initialize");
    std::fs::write(&path, "# owner note\nschema_version: 1\nextra: keep\nui:\n  theme: dark # theme note\n  language: en\n").expect("fixture");
    let before = std::fs::read(&path).expect("read");
    let mut edit = config::begin_edit(&path).expect("edit");
    edit.save().expect("no-op");
    assert_eq!(std::fs::read(&path).expect("read"), before);
    edit.config.ui.theme = "light".into();
    edit.save().expect("save");
    let after = std::fs::read_to_string(&path).expect("read");
    assert!(after.contains("# owner note"));
    assert!(after.contains("# theme note"));
    assert!(after.contains("extra: keep"));
    assert_eq!(config::read_from(&path).expect("read").ui.theme, "light");
    edit.config.ui.theme = "dark".into();
    edit.save().expect("second save");
    assert_eq!(config::read_from(&path).expect("read").ui.theme, "dark");
}

#[test]
fn invalid_documents_are_rejected_without_replacement() {
    let root = Installation::new();
    let path = root.config();
    config::load_from(&path).expect("initialize");
    for text in [
        "schema_version: 1\nschema_version: 1\n",
        "schema_version: 2\n",
        "schema_version: 1\nextra: &x [hello]\nother: *x\n",
        "schema_version: 1\nextra: !custom value\n",
        "schema_version: 1\n---\nschema_version: 1\n",
        "schema_version: 1\nmodel_capabilities: {version: 9}\n",
    ] {
        std::fs::write(&path, text).expect("fixture");
        assert!(config::load_from(&path).is_err(), "must reject {text}");
        assert_eq!(std::fs::read_to_string(&path).expect("read"), text);
    }
}

#[test]
fn external_edit_conflicts_and_does_not_lose_new_bytes() {
    let root = Installation::new();
    let path = root.config();
    config::load_from(&path).expect("initialize");
    let mut edit = config::begin_edit(&path).expect("edit");
    edit.config.ui.theme = "light".into();
    let external = "schema_version: 1\nextra: external\n";
    std::fs::write(&path, external).expect("external edit");
    assert!(edit.save().is_err());
    assert_eq!(std::fs::read_to_string(&path).expect("read"), external);
}

#[test]
fn model_sections_merge_and_permission_grant_can_be_cleared() {
    let root = Installation::new();
    let path = root.config();
    let mut app = config::load_from(&path).expect("initialize");
    app.defaults.full_access_grant = Some("synthetic-grant".into());
    app.save_to(&path).expect("save grant reference");
    let (base, snapshot) = reasoning::read_authority_from(&path).expect("reasoning");
    reasoning::save_profile_patch(
        &path,
        &snapshot,
        &base,
        &reasoning::ReasoningProfilePatch {
            provider: "example".into(),
            model: "example-model".into(),
            preference: Some("high".into()),
        },
    )
    .expect("preference");
    let (base, snapshot) = model_capabilities::read_authority_from(&path).expect("models");
    model_capabilities::save_model_patch(
        &path,
        &snapshot,
        &base,
        &model_capabilities::ModelCapabilityPatch::Upsert(
            model_capabilities::ModelCapabilityRecord {
                provider: "example".into(),
                model: "example-model".into(),
                allowed_efforts: Some(vec!["high".into()]),
                image_input: Some(true),
            },
        ),
    )
    .expect("capability");
    config::update_from(&path, |app| app.defaults.full_access_grant = None).expect("clear");
    assert_eq!(
        config::read_from(&path)
            .expect("app")
            .defaults
            .full_access_grant,
        None
    );
    assert_eq!(
        reasoning::read_from(&path)
            .expect("reasoning")
            .profile("example", "example-model")
            .expect("profile")
            .preference,
        "high"
    );
    assert_eq!(
        model_capabilities::read_from(&path)
            .expect("models")
            .record("example", "example-model")
            .expect("model")
            .image_input,
        Some(true)
    );
    let bytes = std::fs::read(&path).expect("read");
    let _: serde_json::Value = yaml_config::decode(&bytes).expect("valid shared YAML");
    assert!(!root.0.join("config.toml").exists());
    assert!(!root.0.join("reasoning-preferences.toml").exists());
}

#[test]
fn provider_reordering_preserves_identity_comments_and_unknown_fields() {
    let root = Installation::new();
    let path = root.config();
    config::load_from(&path).expect("initialize");
    std::fs::write(&path, "schema_version: 1\nproviders:\n  - name: first\n    base_url: https://first.invalid\n    models: []\n    key_ref: first\n    extra: retain # identity note\n  - name: second\n    base_url: https://second.invalid\n    models: []\n    key_ref: second\n").expect("fixture");
    config::update_from(&path, |app| app.providers.reverse()).expect("reorder");
    let text = std::fs::read_to_string(&path).expect("read");
    assert!(text.contains("extra: retain"));
    assert!(text.contains("# identity note"));
    assert_eq!(
        config::read_from(&path).expect("config").providers[0].name,
        "second"
    );
}

#[test]
fn active_configuration_lock_rejects_another_writer() {
    let root = Installation::new();
    let path = root.config();
    config::load_from(&path).expect("initialize");
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(root.0.join(".config.lock"))
        .expect("lock file");
    lock.try_lock().expect("external lock");
    assert!(config::begin_edit(&path).is_err());
    lock.unlock().expect("unlock");
    assert!(config::begin_edit(&path).is_ok());
}

#[test]
fn environment_reference_edit_retains_changed_and_untouched_notes() {
    let root = Installation::new();
    let path = root.config();
    config::load_from(&path).expect("initialize");
    std::fs::write(&path, "schema_version: 1\nmcp_servers:\n  - id: synthetic\n    transport:\n      environment:\n        - variable: FIRST_KEY\n          credential_ref: first\n          extra: keep-first # first-note\n        - variable: SECOND_KEY\n          credential_ref: second\n          extra: keep-second # second-note\n").expect("fixture");
    yaml_config::update_section(&path, "mcp_servers", &serde_json::json!([{"id":"synthetic", "transport":{"environment":[{"variable":"FIRST_KEY","credential_ref":"rotated"},{"variable":"SECOND_KEY","credential_ref":"second"}]}}]), None).expect("rotate reference");
    let text = std::fs::read_to_string(&path).expect("read");
    for retained in ["keep-first", "first-note", "keep-second", "second-note"] {
        assert!(text.contains(retained));
    }
    let value: serde_json::Value = yaml_config::decode(text.as_bytes()).expect("decode");
    assert_eq!(
        value["mcp_servers"][0]["transport"]["environment"][0]["credential_ref"],
        "rotated"
    );
}
