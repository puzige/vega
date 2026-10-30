use super::{
    ReasoningDefaultsSaveJob, ReasoningProfileProjection, ReasoningSettingsErrorCode,
    SessionReasoningSaveJob, apply_draft_reasoning_result, confirmed_session_defaults,
    prepare_composer_defaults_save, read_reasoning_authority, reasoning_intent_pending,
    reasoning_profile_patch, save_session_reasoning, session_defaults,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use vega_conversation::types::{ComposerDefaults, SessionReasoningPreference};
use vega_runtime::ReasoningChoice;
use vega_runtime::{ReasoningDisabledWire, ReasoningProtocol, ReasoningSupport};
use vega_store::reasoning::{ReasoningConfig, ReasoningProfile, ReasoningProfilePatch};
use vega_ui::conversation_stream::ComposerDefaultsRequested;

struct Fixture {
    directory: PathBuf,
    database: PathBuf,
    config: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "vega-767-{}-{}",
            std::process::id(),
            vega_conversation::threads::new_thread_id()
        ));
        std::fs::create_dir(&directory).unwrap();
        let database = directory.join("store.sqlite");
        let config = directory.join("reasoning-preferences.toml");
        let store = vega_store::Store::open(&database).unwrap();
        store.migrate().unwrap();
        drop(store);
        Self {
            directory,
            database,
            config,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.directory).unwrap();
    }
}

fn profile(provider: &str, model: &str, choice: ReasoningChoice) -> ReasoningProfileProjection {
    let mut profile = ReasoningProfileProjection::unknown(provider, model);
    profile.protocol = ReasoningProtocol::OpenAiChatCompletions;
    profile.support = ReasoningSupport::Optional;
    profile.efforts = vec!["low".into(), "medium".into(), "high".into()];
    profile.supports_disabled = true;
    profile.disabled_wire = Some(ReasoningDisabledWire::ReasoningEffortNone);
    profile.preference = choice;
    profile
}

fn default_job(path: &Path, candidate: &ReasoningProfileProjection) -> ReasoningDefaultsSaveJob {
    let (base, snapshot) = read_reasoning_authority(path).unwrap();
    let mut old = candidate.clone();
    old.preference = ReasoningChoice::ProviderDefault;
    let mut patch = reasoning_profile_patch(&base, &old, candidate).unwrap();
    patch.preference = Some(match &candidate.preference {
        ReasoningChoice::ProviderDefault => "provider_default".into(),
        ReasoningChoice::Disabled => "disabled".into(),
        ReasoningChoice::Enabled => "enabled".into(),
        ReasoningChoice::Effort(value) => value.clone(),
    });
    ReasoningDefaultsSaveJob {
        path: path.into(),
        base,
        snapshot,
        patch,
    }
}

fn choice(preferences: &[SessionReasoningPreference], provider: &str, model: &str) -> String {
    preferences
        .iter()
        .find(|entry| entry.provider == provider && entry.model == model)
        .unwrap()
        .preference
        .clone()
}

fn write_defaults(path: &Path, records: &[(&str, &str, &str)]) {
    let config = ReasoningConfig {
        version: 1,
        profiles: records
            .iter()
            .map(|(provider, model, choice)| ReasoningProfile {
                provider: (*provider).into(),
                model: (*model).into(),
                preference: (*choice).into(),
            })
            .collect(),
    };
    std::fs::write(path, vega_store::reasoning::encode(&config).unwrap()).unwrap();
}

fn reload_defaults(path: &Path) -> Vec<SessionReasoningPreference> {
    let (config, _) = read_reasoning_authority(path).unwrap();
    let profiles = config
        .profiles
        .iter()
        .map(|value| ReasoningProfileProjection::from_store(value).unwrap())
        .collect::<Vec<_>>();
    session_defaults(&profiles)
}

#[test]
fn confirmed_authority_survives_catalog_invalidation_for_new_drafts() {
    let config = ReasoningConfig {
        version: 1,
        profiles: vec![ReasoningProfile {
            provider: "p".into(),
            model: "m".into(),
            preference: "high".into(),
        }],
    };
    let confirmed = confirmed_session_defaults(None, Some(&config), None).unwrap();
    let mut draft = vega_conversation::threads::draft_thread(None, "m", "confirm");
    draft.reasoning_preferences = Some(confirmed);
    assert_eq!(
        choice(draft.reasoning_preferences.as_ref().unwrap(), "p", "m"),
        "high"
    );
    assert!(confirmed_session_defaults(None, None, None).is_none());
    for error in [
        ReasoningSettingsErrorCode::Io,
        ReasoningSettingsErrorCode::Invalid,
        ReasoningSettingsErrorCode::Conflict,
        ReasoningSettingsErrorCode::Busy,
    ] {
        assert!(confirmed_session_defaults(None, Some(&config), Some(error)).is_none());
    }
}

#[test]
fn pending_reasoning_materialization_fence_is_scoped_to_the_draft() {
    let mut latest = HashMap::new();
    let request = ComposerDefaultsRequested {
        revision: 1,
        thread_id: "draft-a".into(),
        defaults: ComposerDefaults {
            model: "m".into(),
            ..ComposerDefaults::default()
        },
    };
    latest.insert(("draft-a".into(), "m".into()), request);
    assert!(reasoning_intent_pending(&latest, "draft-a"));
    assert!(!reasoning_intent_pending(&latest, "draft-b"));
    latest.clear();
    assert!(!reasoning_intent_pending(&latest, "draft-a"));
}

#[test]
fn delayed_draft_reasoning_completion_changes_only_the_matching_identity() {
    let mut a = vega_conversation::threads::draft_thread(None, "m", "confirm");
    let mut b = vega_conversation::threads::draft_thread(None, "m", "confirm");
    let original_a = vec![
        SessionReasoningPreference {
            provider: "p".into(),
            model: "m".into(),
            preference: "low".into(),
        },
        SessionReasoningPreference {
            provider: "p".into(),
            model: "other".into(),
            preference: "medium".into(),
        },
    ];
    let original_b = vec![SessionReasoningPreference {
        provider: "p".into(),
        model: "m".into(),
        preference: "medium".into(),
    }];
    let saved_a = profile("p", "m", ReasoningChoice::Effort("high".into()));
    a.reasoning_preferences = Some(original_a);
    b.reasoning_preferences = Some(original_b.clone());
    let a_id = a.id.clone();
    assert!(!apply_draft_reasoning_result(&mut b, &a_id, &saved_a));
    assert_eq!(b.reasoning_preferences, Some(original_b));
    assert!(apply_draft_reasoning_result(&mut a, &a_id, &saved_a));
    let preferences = a.reasoning_preferences.as_ref().unwrap();
    assert_eq!(choice(preferences, "p", "m"), "high");
    assert_eq!(choice(preferences, "p", "other"), "medium");
    a.reasoning_preferences = None;
    assert!(!apply_draft_reasoning_result(&mut a, &a_id, &saved_a));
    assert!(a.reasoning_preferences.is_none());
}

#[test]
fn unavailable_or_conflicting_catalog_cannot_prepare_a_session_or_default_write() {
    let fixture = Fixture::new();
    write_defaults(&fixture.config, &[("p", "m", "low"), ("q", "m", "medium")]);
    let store = vega_store::Store::open_existing(&fixture.database).unwrap();
    let thread =
        vega_conversation::threads::create_standalone_thread(&store, "m", "confirm").unwrap();
    let preferences = reload_defaults(&fixture.config);
    vega_conversation::session_reasoning::initialize(&store, &thread.id, &preferences).unwrap();
    let source = profile("p", "m", ReasoningChoice::Effort("low".into()));
    let candidate = profile("p", "m", ReasoningChoice::Effort("high".into()));
    let authority = read_reasoning_authority(&fixture.config).unwrap();
    let session_before = vega_conversation::threads::get_thread(&store, &thread.id)
        .unwrap()
        .reasoning_preferences;
    let defaults_before = std::fs::read(&fixture.config).unwrap();

    for error in [
        ReasoningSettingsErrorCode::Io,
        ReasoningSettingsErrorCode::Invalid,
        ReasoningSettingsErrorCode::Conflict,
        ReasoningSettingsErrorCode::Busy,
    ] {
        assert_eq!(
            prepare_composer_defaults_save(
                Some(fixture.config.clone()),
                Some(authority.clone()),
                Some(error),
                &source,
                &candidate,
            )
            .err(),
            Some(error)
        );
    }
    assert_eq!(
        prepare_composer_defaults_save(
            Some(fixture.config.clone()),
            None,
            None,
            &source,
            &candidate,
        )
        .err(),
        Some(ReasoningSettingsErrorCode::Io)
    );
    let mut incompatible = candidate.clone();
    incompatible.efforts.push("unexpected".into());
    assert_eq!(
        prepare_composer_defaults_save(
            Some(fixture.config.clone()),
            Some(authority.clone()),
            None,
            &source,
            &incompatible,
        )
        .err(),
        Some(ReasoningSettingsErrorCode::Invalid)
    );
    assert!(
        prepare_composer_defaults_save(
            Some(fixture.config.clone()),
            Some(authority),
            None,
            &source,
            &candidate,
        )
        .is_ok()
    );
    assert_eq!(std::fs::read(&fixture.config).unwrap(), defaults_before);
    assert_eq!(
        vega_conversation::threads::get_thread(&store, &thread.id)
            .unwrap()
            .reasoning_preferences,
        session_before
    );
}

#[test]
fn saved_conversation_updates_new_defaults_without_changing_other_sessions_or_models() {
    let fixture = Fixture::new();
    write_defaults(
        &fixture.config,
        &[
            ("p", "m", "low"),
            ("p", "other", "medium"),
            ("q", "m", "low"),
        ],
    );
    let store = vega_store::Store::open_existing(&fixture.database).unwrap();
    let a = vega_conversation::threads::create_standalone_thread(&store, "m", "confirm").unwrap();
    let b = vega_conversation::threads::create_standalone_thread(&store, "m", "confirm").unwrap();
    let defaults = reload_defaults(&fixture.config);
    vega_conversation::session_reasoning::initialize(&store, &a.id, &defaults).unwrap();
    vega_conversation::session_reasoning::initialize(&store, &b.id, &defaults).unwrap();
    let frozen = profile("p", "m", ReasoningChoice::Effort("low".into()))
        .freeze()
        .unwrap();
    let candidate = profile("p", "m", ReasoningChoice::Effort("high".into()));
    let (saved, authority) = save_session_reasoning(SessionReasoningSaveJob {
        database: Some(fixture.database.clone()),
        thread_id: a.id.clone(),
        draft: false,
        preferences: defaults,
        defaults: Some(default_job(&fixture.config, &candidate)),
        candidate,
    })
    .unwrap();
    assert_eq!(authority.unwrap().error, None);
    assert_eq!(choice(&saved, "p", "m"), "high");
    assert_eq!(
        choice(
            &vega_conversation::threads::get_thread(&store, &b.id)
                .unwrap()
                .reasoning_preferences
                .unwrap(),
            "p",
            "m"
        ),
        "low"
    );
    assert_eq!(frozen.choice, ReasoningChoice::Effort("low".into()));
    drop(store);
    let defaults_after_restart = reload_defaults(&fixture.config);
    let mut draft = vega_conversation::threads::draft_thread(None, "m", "confirm");
    draft.reasoning_preferences = Some(defaults_after_restart.clone());
    assert_eq!(
        choice(draft.reasoning_preferences.as_ref().unwrap(), "p", "m"),
        "high"
    );
    assert_eq!(choice(&defaults_after_restart, "p", "other"), "medium");
    assert_eq!(choice(&defaults_after_restart, "q", "m"), "low");
}

#[test]
fn draft_choices_survive_materialization_and_each_supported_value_updates_defaults() {
    let fixture = Fixture::new();
    let store = vega_store::Store::open_existing(&fixture.database).unwrap();
    let mut draft = vega_conversation::threads::draft_thread(None, "m", "confirm");
    let mut preferences = Vec::new();
    for (selection, expected) in [
        (ReasoningChoice::Effort("high".into()), "high"),
        (ReasoningChoice::Disabled, "disabled"),
        (ReasoningChoice::ProviderDefault, "provider_default"),
    ] {
        let candidate = profile("p", "m", selection);
        let (saved, authority) = save_session_reasoning(SessionReasoningSaveJob {
            database: None,
            thread_id: draft.id.clone(),
            draft: true,
            preferences,
            defaults: Some(default_job(&fixture.config, &candidate)),
            candidate,
        })
        .unwrap();
        assert_eq!(authority.unwrap().error, None);
        assert_eq!(
            choice(&reload_defaults(&fixture.config), "p", "m"),
            expected
        );
        preferences = saved;
    }
    draft.reasoning_preferences = Some(preferences);
    let materialized = vega_conversation::threads::materialize_draft(&store, &draft).unwrap();
    assert_eq!(
        choice(&materialized.reasoning_preferences.unwrap(), "p", "m"),
        "provider_default"
    );
    assert_eq!(
        choice(
            &vega_conversation::threads::get_thread(&store, &draft.id)
                .unwrap()
                .reasoning_preferences
                .unwrap(),
            "p",
            "m"
        ),
        "provider_default"
    );
}

#[test]
fn session_failure_does_not_write_defaults_and_default_conflicts_report_partial_success() {
    let fixture = Fixture::new();
    write_defaults(&fixture.config, &[("p", "m", "low"), ("q", "m", "medium")]);
    let candidate = profile("p", "m", ReasoningChoice::Effort("high".into()));
    let job = default_job(&fixture.config, &candidate);
    assert!(
        save_session_reasoning(SessionReasoningSaveJob {
            database: Some(fixture.database.clone()),
            thread_id: "missing".into(),
            draft: false,
            preferences: Vec::new(),
            defaults: Some(job),
            candidate: candidate.clone(),
        })
        .is_err()
    );
    assert_eq!(choice(&reload_defaults(&fixture.config), "p", "m"), "low");
    let stale = default_job(&fixture.config, &candidate);
    let concurrent = ReasoningProfilePatch {
        provider: "p".into(),
        model: "m".into(),
        preference: Some("medium".into()),
    };
    vega_store::reasoning::save_profile_patch(
        &fixture.config,
        &stale.snapshot,
        &stale.base,
        &concurrent,
    )
    .unwrap();
    let (saved, authority) = save_session_reasoning(SessionReasoningSaveJob {
        database: None,
        thread_id: "draft".into(),
        draft: true,
        preferences: Vec::new(),
        defaults: Some(stale),
        candidate,
    })
    .unwrap();
    assert_eq!(choice(&saved, "p", "m"), "high");
    assert_eq!(
        authority.unwrap().error,
        Some(ReasoningSettingsErrorCode::Conflict)
    );
    let current = reload_defaults(&fixture.config);
    assert_eq!(choice(&current, "p", "m"), "medium");
    assert_eq!(choice(&current, "q", "m"), "medium");
}
