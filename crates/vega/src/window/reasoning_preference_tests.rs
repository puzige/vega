use super::{read_reasoning_authority, reasoning_profile_patch};
use vega_conversation::types::{
    ReasoningChoice, ReasoningDisabledWire, ReasoningProfileProjection,
};
use vega_store::config::ProviderApi;
use vega_store::reasoning::{
    ReasoningConfig, ReasoningConfigError, encode, read_authority_from, read_from,
    read_snapshot_from, save_profile_patch,
};

fn document(preference: &str) -> String {
    format!(
        "version = 1\n[[preferences]]\nprovider = \"proxy\"\nmodel = \"step-5-preview\"\npreference = \"{preference}\"\n"
    )
}

fn other_record(preference: &str) -> String {
    format!(
        "\n[[preferences]]\nprovider = \"other\"\nmodel = \"other-model\"\npreference = \"{preference}\"\n"
    )
}

fn resolved_profile(authority: &ReasoningConfig) -> ReasoningProfileProjection {
    let configured = authority
        .profile("proxy", "step-5-preview")
        .map(ReasoningProfileProjection::from_store)
        .transpose()
        .expect("preference projection");
    crate::reasoning_capabilities::resolve_reasoning_profile(
        "proxy",
        ProviderApi::ChatCompletions,
        "https://gateway.example/v1",
        "step-5-preview",
        configured,
    )
    .expect("internal Step capabilities")
}

#[test]
fn preference_codec_round_trips_only_exact_identity_and_preference_records() {
    let root = tempfile::tempdir().expect("owned codec fixture");
    let path = root.path().join("reasoning-preferences.toml");
    let source = document("high") + &other_record("provider_default");
    std::fs::write(&path, &source).expect("new document");
    let config = read_from(&path).expect("new preference grammar");
    assert_eq!(
        serde_json::to_value(&config).expect("stored shape"),
        serde_json::json!({
            "version": 1,
            "preferences": [
                {"provider":"proxy","model":"step-5-preview","preference":"high"},
                {"provider":"other","model":"other-model","preference":"provider_default"}
            ]
        })
    );
    let encoded = encode(&config).expect("preference encoding");
    let copy = root.path().join("encoded.toml");
    std::fs::write(&copy, encoded).expect("encoded copy");
    assert_eq!(read_from(&copy).expect("encoded round trip"), config);
    assert_eq!(
        std::fs::read_to_string(&path).expect("original document"),
        source
    );
}

#[test]
fn new_preference_grammar_rejects_legacy_fields_duplicates_and_invalid_versions() {
    let root = tempfile::tempdir().expect("owned rejected grammar");
    let path = root.path().join("reasoning-preferences.toml");
    let mut rejected = vec![
        document("high").replacen("version = 1", "version = 2", 1),
        document("high").replacen("version = 1", "version = 0", 1),
        document("high") + "\n[[preferences]]\nprovider = \"proxy\"\nmodel = \"step-5-preview\"\npreference = \"low\"\n",
        "version = 1\n[[profiles]]\nprovider = \"proxy\"\nmodel = \"step-5-preview\"\npreference = \"provider_default\"\n".into(),
        document("high").replace("preference = \"high\"", "preference = [\"high\"]"),
    ];
    for field in [
        "protocol = \"openai_chat_completions\"",
        "support = \"optional\"",
        "efforts = [\"high\"]",
        "supports_disabled = true",
        "disabled_wire = \"reasoning_effort_none\"",
        "replay_profile = \"standard_chat\"",
        "preserve_reasoning_content = true",
    ] {
        rejected.push(format!("{}{field}\n", document("high")));
    }
    for source in rejected {
        std::fs::write(&path, &source).expect("invalid input");
        assert!(
            read_from(&path).is_err(),
            "accepted unsupported preference document: {source}"
        );
        assert_eq!(
            std::fs::read_to_string(&path).expect("rejected bytes preserved"),
            source
        );
    }
}

#[test]
fn loading_missing_preferences_is_read_only_and_first_save_writes_no_capabilities() {
    let root = tempfile::tempdir().expect("owned first-save fixture");
    let old_path = root.path().join("reasoning.toml");
    let new_path = root.path().join("reasoning-preferences.toml");
    let old = "old configuration remains untouched";
    std::fs::write(&old_path, old).expect("old file");
    let (authority, snapshot) = read_reasoning_authority(&new_path).expect("absent authority");
    let base = resolved_profile(&authority);
    assert_eq!(base.preference, ReasoningChoice::ProviderDefault);
    assert_eq!(base.efforts, ["low", "medium", "high"]);
    assert!(!new_path.exists());
    assert_eq!(
        std::fs::read_dir(root.path()).expect("before save").count(),
        1
    );
    let mut candidate = base.clone();
    candidate.preference = ReasoningChoice::Effort("medium".into());
    let patch = reasoning_profile_patch(&authority, &base, &candidate).expect("preference patch");
    save_profile_patch(&new_path, &snapshot, &authority, &patch).expect("explicit first save");
    let saved = read_from(&new_path).expect("new authority");
    assert_eq!(
        serde_json::to_value(saved).expect("saved shape"),
        serde_json::json!({
            "version":1,
            "preferences":[{"provider":"proxy","model":"step-5-preview","preference":"medium"}]
        })
    );
    assert_eq!(
        std::fs::read_to_string(&old_path).expect("old unchanged"),
        old
    );
}

#[test]
fn preference_patch_preserves_external_updates_to_another_model() {
    let root = tempfile::tempdir().expect("owned disjoint edit fixture");
    let path = root.path().join("reasoning-preferences.toml");
    std::fs::write(&path, document("high") + &other_record("provider_default")).expect("base file");
    let (authority, snapshot) = read_authority_from(&path).expect("base authority");
    let base = resolved_profile(&authority);
    let mut candidate = base.clone();
    candidate.preference = ReasoningChoice::Effort("medium".into());
    let patch =
        reasoning_profile_patch(&authority, &base, &candidate).expect("owned preference patch");
    std::fs::write(&path, document("high") + &other_record("low"))
        .expect("external other model edit");
    save_profile_patch(&path, &snapshot, &authority, &patch).expect("disjoint merge");
    let saved = read_from(&path).expect("merged authority");
    assert_eq!(
        serde_json::to_value(saved).expect("merged shape"),
        serde_json::json!({
            "version":1,
            "preferences":[
                {"provider":"proxy","model":"step-5-preview","preference":"medium"},
                {"provider":"other","model":"other-model","preference":"low"}
            ]
        })
    );
}

#[test]
fn stale_preference_patch_cannot_overwrite_a_conflict_or_resurrect_external_deletion() {
    for external_change in ["preference", "record deletion", "file deletion"] {
        let root = tempfile::tempdir().expect("owned CAS fixture");
        let path = root.path().join("reasoning-preferences.toml");
        std::fs::write(&path, document("high")).expect("base file");
        let (authority, snapshot) = read_authority_from(&path).expect("base authority");
        let base = resolved_profile(&authority);
        let mut candidate = base.clone();
        candidate.preference = ReasoningChoice::Effort("medium".into());
        let patch = reasoning_profile_patch(&authority, &base, &candidate).expect("pending patch");
        match external_change {
            "preference" => std::fs::write(&path, document("low")).expect("external preference"),
            "record deletion" => std::fs::write(&path, "version = 1\npreferences = []\n")
                .expect("external record deletion"),
            _ => std::fs::remove_file(&path).expect("external file deletion"),
        }
        let external_snapshot = read_snapshot_from(&path).expect("external snapshot");
        assert!(matches!(
            save_profile_patch(&path, &snapshot, &authority, &patch),
            Err(ReasoningConfigError::ConcurrentModification)
        ));
        assert_eq!(
            read_snapshot_from(&path).expect("external preserved"),
            external_snapshot
        );
    }
}

#[test]
fn unsupported_new_preference_stays_repairable_and_only_a_legal_selection_is_saved() {
    for (invalid, repaired) in [
        ("max", ReasoningChoice::Effort("medium".into())),
        ("disabled", ReasoningChoice::ProviderDefault),
    ] {
        let root = tempfile::tempdir().expect("owned repair fixture");
        let path = root.path().join("reasoning-preferences.toml");
        let source = document(invalid);
        std::fs::write(&path, &source).expect("syntactically valid preference");
        let (authority, snapshot) = read_reasoning_authority(&path).expect("repairable authority");
        let base = resolved_profile(&authority);
        let expected_choice = if invalid == "disabled" {
            ReasoningChoice::Disabled
        } else {
            ReasoningChoice::Effort(invalid.into())
        };
        assert_eq!(base.preference, expected_choice);
        assert_eq!(base.efforts, ["low", "medium", "high"]);
        assert!(base.freeze().is_err());
        assert_eq!(
            std::fs::read_to_string(&path).expect("no silent default"),
            source
        );
        let mut candidate = base.clone();
        candidate.preference = repaired.clone();
        let patch =
            reasoning_profile_patch(&authority, &base, &candidate).expect("legal repair patch");
        save_profile_patch(&path, &snapshot, &authority, &patch).expect("explicit repair");
        let frozen = crate::app_agent::reasoning_for_provider_model(
            "proxy",
            ProviderApi::ChatCompletions,
            "https://gateway.example/v1",
            "step-5-preview",
            Some(&path),
        )
        .expect("repaired runtime preference");
        assert_eq!(frozen.choice, repaired);
        assert_eq!(frozen.declared_efforts, ["low", "medium", "high"]);
    }
}

#[test]
fn preference_controller_rejects_attempts_to_edit_capabilities() {
    let authority = ReasoningConfig::default();
    let base = resolved_profile(&authority);
    let mut candidate = base.clone();
    candidate.supports_disabled = true;
    candidate.disabled_wire = Some(ReasoningDisabledWire::ReasoningEffortNone);
    assert!(reasoning_profile_patch(&authority, &base, &candidate).is_err());
}
