use super::reasoning_profile_patch;
use vega_conversation::types::{
    ReasoningChoice, ReasoningDisabledWire, ReasoningProfileProjection, ReasoningProtocol,
    ReasoningReplaySetting, ReasoningSupport,
};
use vega_store::reasoning::{
    ReasoningConfig, ReasoningConfigError, ReasoningFileSnapshot, ReasoningProfile,
    ReasoningProfilePatch, read_authority_from, read_from, save_if_unchanged, save_profile_patch,
};

fn normalized(preference: ReasoningChoice) -> ReasoningProfileProjection {
    ReasoningProfileProjection {
        provider: "proxy".into(),
        model: "step-5-preview".into(),
        protocol: ReasoningProtocol::OpenAiChatCompletions,
        support: ReasoningSupport::Optional,
        efforts: ["low", "medium", "high"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        supports_disabled: false,
        disabled_wire: None,
        replay_profile: ReasoningReplaySetting::StandardChat,
        preference,
    }
}

fn old_record() -> ReasoningProfile {
    ReasoningProfile {
        provider: "proxy".into(),
        model: "step-5-preview".into(),
        protocol: "openai_chat_completions".into(),
        support: "optional".into(),
        efforts: ["minimal", "low", "medium", "high", "xhigh", "max"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        supports_disabled: true,
        disabled_wire: Some("reasoning_effort_none".into()),
        replay_profile: "standard_chat".into(),
        preference: "high".into(),
    }
}

#[test]
fn existing_preference_save_keeps_imported_capabilities_and_disjoint_external_updates() {
    let root = tempfile::tempdir().expect("owned settings fixture");
    let path = root.path().join("reasoning.toml");
    let original = old_record();
    let authority = ReasoningConfig {
        version: 2,
        profiles: vec![original.clone()],
    };
    save_if_unchanged(&path, &ReasoningFileSnapshot::absent(), &authority).expect("seed authority");
    let (base, snapshot) = read_authority_from(&path).expect("base authority");
    let base_projection = normalized(ReasoningChoice::Effort("high".into()));
    let mut candidate = base_projection.clone();
    candidate.preference = ReasoningChoice::Effort("low".into());
    let patch =
        reasoning_profile_patch(&base, &base_projection, &candidate).expect("preference patch");
    assert_eq!(
        patch,
        ReasoningProfilePatch {
            provider: "proxy".into(),
            model: "step-5-preview".into(),
            preference: Some("low".into()),
            ..Default::default()
        }
    );
    let mut external = base.clone();
    external.profiles[0].efforts = vec!["low".into(), "high".into()];
    let mut unrelated = original;
    unrelated.model = "unrelated".into();
    external.profiles.push(unrelated.clone());
    save_if_unchanged(&path, &snapshot, &external).expect("external disjoint edit");
    save_profile_patch(&path, &snapshot, &base, &patch).expect("merge preference");
    let saved = read_from(&path).expect("saved authority");
    let expected = ReasoningProfile {
        preference: "low".into(),
        ..external.profiles[0].clone()
    };
    assert_eq!(saved.profiles, vec![expected, unrelated]);
}

#[test]
fn preference_save_rejects_same_field_conflict_and_capability_mutation() {
    let root = tempfile::tempdir().expect("owned conflict fixture");
    let path = root.path().join("reasoning.toml");
    let authority = ReasoningConfig {
        version: 2,
        profiles: vec![old_record()],
    };
    let snapshot =
        save_if_unchanged(&path, &ReasoningFileSnapshot::absent(), &authority).expect("seed");
    let base_projection = normalized(ReasoningChoice::Effort("high".into()));
    let mut candidate = base_projection.clone();
    candidate.preference = ReasoningChoice::Effort("low".into());
    let patch = reasoning_profile_patch(&authority, &base_projection, &candidate).expect("patch");
    let mut external = authority.clone();
    external.profiles[0].preference = "medium".into();
    save_if_unchanged(&path, &snapshot, &external).expect("concurrent preference");
    assert!(matches!(
        save_profile_patch(&path, &snapshot, &authority, &patch),
        Err(ReasoningConfigError::ConcurrentModification)
    ));
    assert_eq!(read_from(&path).expect("preserved external"), external);
    let mut altered = base_projection.clone();
    altered.preference = ReasoningChoice::Effort("low".into());
    altered.supports_disabled = true;
    altered.disabled_wire = Some(ReasoningDisabledWire::ReasoningEffortNone);
    assert!(reasoning_profile_patch(&authority, &base_projection, &altered).is_err());
}

#[test]
fn first_preference_save_materializes_known_facts_without_rewriting_on_load() {
    let root = tempfile::tempdir().expect("owned first-save fixture");
    let path = root.path().join("reasoning.toml");
    let (authority, snapshot) = read_authority_from(&path).expect("absent authority");
    assert!(!path.exists());
    let base = normalized(ReasoningChoice::ProviderDefault);
    let mut candidate = base.clone();
    candidate.preference = ReasoningChoice::Effort("medium".into());
    let patch =
        reasoning_profile_patch(&authority, &base, &candidate).expect("new preference patch");
    save_profile_patch(&path, &snapshot, &authority, &patch).expect("first preference save");
    let profile = read_from(&path).expect("new authority").profiles.remove(0);
    assert_eq!(profile.efforts, ["low", "medium", "high"]);
    assert_eq!(profile.preference, "medium");
    assert!(!profile.supports_disabled);
    assert_eq!(profile.disabled_wire, None);
    let before = std::fs::read(&path).expect("saved bytes");
    read_authority_from(&path).expect("read after save");
    assert_eq!(std::fs::read(&path).expect("read-only bytes"), before);
}

#[test]
fn legacy_v1_preference_can_be_repaired_without_replacing_imported_capabilities() {
    let root = tempfile::tempdir().expect("owned v1 fixture");
    let path = root.path().join("reasoning.toml");
    let source = "version = 1\n[[profiles]]\nprovider = \"proxy\"\nmodel = \"step-5-preview\"\nprotocol = \"openai_chat_completions\"\nsupport = \"optional\"\nefforts = [\"minimal\", \"low\", \"medium\", \"high\", \"xhigh\", \"max\"]\nsupports_disabled = true\ndisabled_wire = \"reasoning_effort_none\"\npreserve_reasoning_content = true\npreference = \"disabled\"\n";
    std::fs::write(&path, source).expect("v1 authority");
    let (authority, snapshot) = read_authority_from(&path).expect("v1 codec");
    assert_eq!(
        std::fs::read_to_string(&path).expect("unmodified v1"),
        source
    );
    let base = normalized(ReasoningChoice::Disabled);
    let mut candidate = base.clone();
    candidate.preference = ReasoningChoice::ProviderDefault;
    let patch = reasoning_profile_patch(&authority, &base, &candidate).expect("v1 repair patch");
    assert_eq!(
        patch,
        ReasoningProfilePatch {
            provider: "proxy".into(),
            model: "step-5-preview".into(),
            preference: Some("provider_default".into()),
            ..Default::default()
        }
    );
    save_profile_patch(&path, &snapshot, &authority, &patch).expect("repair v1");
    let saved = read_from(&path).expect("repaired v1");
    let mut expected = authority.profiles[0].clone();
    expected.preference = "provider_default".into();
    assert_eq!(saved.profiles, [expected]);
}
