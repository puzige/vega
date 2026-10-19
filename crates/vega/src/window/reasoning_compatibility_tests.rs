use super::*;
use vega_store::config::ProviderApi;
use vega_store::reasoning::{
    self, ReasoningConfig, ReasoningConfigError, ReasoningProfile, ReasoningProfilePatch,
};

const ENDPOINT: &str = "https://api.together.ai/v1";
const MODEL: &str = "Qwen/Qwen3.5-9B";

fn resolved(preference: &str) -> ReasoningProfileProjection {
    let stored = ReasoningProfile {
        provider: "fixture".into(),
        model: MODEL.into(),
        preference: preference.into(),
    };
    let projection =
        ReasoningProfileProjection::from_store(&stored).expect("new preference projection");
    crate::reasoning_capabilities::resolve_reasoning_profile(
        "fixture",
        ProviderApi::ChatCompletions,
        ENDPOINT,
        MODEL,
        Some(projection),
    )
    .expect("actual facts")
}

#[test]
fn controller_saves_enabled_as_three_fields_and_reloads_through_actual_provider_facts() {
    let root = tempfile::tempdir().expect("owned controller fixture");
    let path = root.path().join(reasoning::REASONING_FILE_NAME);
    let (authority, snapshot) = read_reasoning_authority(&path).expect("absent authority");
    assert!(!snapshot.exists);
    assert!(!path.exists());
    let base = resolved("provider_default");
    let candidate = resolved("enabled");
    let patch = reasoning_profile_patch(&authority, &base, &candidate)
        .expect("preference-only controller patch");
    assert_eq!(patch.preference.as_deref(), Some("enabled"));
    reasoning::save_profile_patch(&path, &snapshot, &authority, &patch).expect("controller save");
    let (saved, _) = read_reasoning_authority(&path).expect("saved authority");
    assert_eq!(
        saved.profiles,
        vec![ReasoningProfile {
            provider: "fixture".into(),
            model: MODEL.into(),
            preference: "enabled".into()
        }]
    );
    let encoded = String::from_utf8(reasoning::encode(&saved).expect("encode")).expect("utf8");
    assert_eq!(encoded.lines().filter(|line| line.contains('=')).count(), 4);
    for forbidden in [
        "protocol",
        "replay_profile",
        "efforts",
        "supports_disabled",
        "reasoning_details",
        "reasoning_content",
    ] {
        assert!(!encoded.contains(forbidden));
    }
    let config = vega_store::config::AppConfig {
        providers: vec![vega_store::config::ProviderConfig {
            api: ProviderApi::ChatCompletions,
            enabled: true,
            name: "fixture".into(),
            base_url: ENDPOINT.into(),
            models: vec![MODEL.into()],
            key_ref: "fixture-key".into(),
        }],
        ..Default::default()
    };
    let reconciled = reconcile_reasoning_profiles(&saved, vec![base], Some(&config))
        .expect("actual reconciliation");
    assert_eq!(format!("{:?}", reconciled[0].preference), "Enabled");
    assert!(reconciled[0].efforts.is_empty());
    let frozen = crate::app_agent::reasoning_for_provider_model(
        "fixture",
        ProviderApi::ChatCompletions,
        ENDPOINT,
        MODEL,
        Some(&path),
    )
    .expect("real loader freeze");
    assert_eq!(format!("{:?}", frozen.choice), "Enabled");
}

#[test]
fn old_reasoning_files_never_affect_actual_load_or_new_explicit_enabled_save() {
    for old in [
        "version=1\n[[profiles]]\nprovider='fixture'\nmodel='Qwen/Qwen3.5-9B'\nprotocol='openai_chat_completions'\nsupport='optional'\nefforts=['high']\npreference='high'",
        "version=2\n[[profiles]]\nprovider='fixture'\nmodel='Qwen/Qwen3.5-9B'\nprotocol='openai_chat_completions'\nsupport='optional'\nefforts=['low']\nreplay_profile='standard_chat'\npreference='low'",
        "{broken TOML",
        "version=1\n[[preferences]]\nprovider='fixture'\nmodel='Qwen/Qwen3.5-9B'\npreference='enabled'\nprotocol='injected'",
    ] {
        let root = tempfile::tempdir().expect("owned legacy boundary fixture");
        let old_path = root.path().join("reasoning.toml");
        let path = root.path().join(reasoning::REASONING_FILE_NAME);
        std::fs::write(&old_path, old).expect("old file");
        let frozen = crate::app_agent::reasoning_for_provider_model(
            "fixture",
            ProviderApi::ChatCompletions,
            ENDPOINT,
            MODEL,
            Some(&path),
        )
        .expect("absent new authority");
        assert_eq!(frozen.choice, ReasoningChoice::ProviderDefault);
        assert!(!path.exists());
        assert_eq!(std::fs::read_to_string(&old_path).expect("old bytes"), old);
        let (authority, snapshot) = read_reasoning_authority(&path).expect("new authority");
        let patch = reasoning_profile_patch(
            &authority,
            &resolved("provider_default"),
            &resolved("enabled"),
        )
        .expect("explicit save");
        reasoning::save_profile_patch(&path, &snapshot, &authority, &patch).expect("new save");
        assert_eq!(
            std::fs::read_to_string(&old_path).expect("old unchanged"),
            old
        );
        assert!(
            reasoning::read_from(&path)
                .expect("new file")
                .profiles
                .iter()
                .any(|profile| profile.preference == "enabled")
        );
    }
}

#[test]
fn preference_compare_and_swap_preserves_other_models_and_rejects_same_identity_changes_or_deletion()
 {
    let root = tempfile::tempdir().expect("owned CAS fixture");
    let path = root.path().join(reasoning::REASONING_FILE_NAME);
    let base = ReasoningConfig {
        version: 1,
        profiles: vec![ReasoningProfile {
            provider: "fixture".into(),
            model: MODEL.into(),
            preference: "provider_default".into(),
        }],
    };
    std::fs::write(&path, reasoning::encode(&base).expect("base encode")).expect("base write");
    let (_, snapshot) = read_reasoning_authority(&path).expect("snapshot");
    let patch = reasoning_profile_patch(&base, &resolved("provider_default"), &resolved("enabled"))
        .expect("enabled patch");
    let mut external = base.clone();
    external.upsert_profile(ReasoningProfile {
        provider: "other".into(),
        model: "unrelated".into(),
        preference: "max".into(),
    });
    std::fs::write(
        &path,
        reasoning::encode(&external).expect("external encode"),
    )
    .expect("external write");
    reasoning::save_profile_patch(&path, &snapshot, &base, &patch).expect("disjoint merge");
    let loaded = reasoning::read_from(&path).expect("merged authority");
    assert_eq!(
        loaded
            .profile("other", "unrelated")
            .expect("other")
            .preference,
        "max"
    );
    assert_eq!(
        loaded.profile("fixture", MODEL).expect("target").preference,
        "enabled"
    );
    let (base, snapshot) = read_reasoning_authority(&path).expect("fresh authority");
    let mut external = base.clone();
    external.upsert_profile(ReasoningProfile {
        provider: "fixture".into(),
        model: MODEL.into(),
        preference: "disabled".into(),
    });
    std::fs::write(&path, reasoning::encode(&external).expect("changed target"))
        .expect("external change");
    let next = ReasoningProfilePatch {
        provider: "fixture".into(),
        model: MODEL.into(),
        preference: Some("provider_default".into()),
    };
    assert!(matches!(
        reasoning::save_profile_patch(&path, &snapshot, &base, &next),
        Err(ReasoningConfigError::ConcurrentModification)
    ));
    std::fs::remove_file(&path).expect("external delete");
    assert!(matches!(
        reasoning::save_profile_patch(&path, &snapshot, &base, &next),
        Err(ReasoningConfigError::ConcurrentModification)
    ));
    assert!(!path.exists());
}

#[test]
fn invalid_model_preference_stays_visible_and_can_be_repaired_without_accepting_capability_mutation()
 {
    let authority = ReasoningConfig {
        version: 1,
        profiles: vec![ReasoningProfile {
            provider: "fixture".into(),
            model: MODEL.into(),
            preference: "high".into(),
        }],
    };
    let invalid = resolved("high");
    assert_eq!(invalid.preference, ReasoningChoice::Effort("high".into()));
    assert!(!invalid.supports_preference());
    assert!(invalid.freeze().is_err());
    let candidate = resolved("enabled");
    let patch = reasoning_profile_patch(&authority, &invalid, &candidate).expect("repair patch");
    assert_eq!(patch.preference.as_deref(), Some("enabled"));
    let mut changed = candidate.clone();
    changed.efforts.push("high".into());
    assert_eq!(
        reasoning_profile_patch(&authority, &invalid, &changed),
        Err(ReasoningSettingsErrorCode::Invalid)
    );
    for contents in [
        "version=2\n",
        "version=1\n[[preferences]]\nprovider='fixture'\nmodel='sample'\npreference='enabled'\nprotocol='openai_chat_completions'",
        "version=1\n[[preferences]]\nprovider='fixture'\nmodel='sample'\npreference='enabled'\n[[preferences]]\nprovider='fixture'\nmodel='sample'\npreference='high'",
    ] {
        let root = tempfile::tempdir().expect("invalid new fixture");
        let path = root.path().join(reasoning::REASONING_FILE_NAME);
        std::fs::write(&path, contents).expect("invalid file");
        assert!(read_reasoning_authority(&path).is_err());
        assert_eq!(
            std::fs::read_to_string(&path).expect("invalid unchanged"),
            contents
        );
    }
}
