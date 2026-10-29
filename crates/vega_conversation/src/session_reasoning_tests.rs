use super::*;
use crate::types::{ReasoningChoice, ReasoningProtocol, ReasoningReplaySetting, ReasoningSupport};

fn profile(provider: &str, model: &str, choice: &str) -> ReasoningProfileProjection {
    let mut profile = ReasoningProfileProjection::unknown(provider, model);
    profile.protocol = ReasoningProtocol::OpenAiChatCompletions;
    profile.replay_profile = ReasoningReplaySetting::StandardChat;
    profile.support = ReasoningSupport::Optional;
    profile.efforts = vec!["low".into(), "high".into()];
    profile.preference = ReasoningChoice::Effort(choice.into());
    profile
}

fn defaults(choice: &str) -> Vec<SessionReasoningPreference> {
    vec![SessionReasoningPreference {
        provider: "provider".into(),
        model: "model".into(),
        preference: choice.into(),
    }]
}

fn thread(store: &Store) -> crate::types::Thread {
    crate::threads::create_standalone_thread(store, "model", "confirm").unwrap()
}

fn choice(store: &Store, id: &str) -> ReasoningChoice {
    let preferences = crate::threads::get_thread(store, id)
        .unwrap()
        .reasoning_preferences
        .unwrap();
    project(profile("provider", "model", "high"), &preferences)
        .unwrap()
        .freeze()
        .unwrap()
        .choice
}

#[test]
fn edits_are_isolated_in_storage_projection_and_frozen_requests() {
    let store = Store::open(":memory:").unwrap();
    store.migrate().unwrap();
    let a = thread(&store);
    let b = thread(&store);
    let global = defaults("low");
    initialize(&store, &a.id, &global).unwrap();
    initialize(&store, &b.id, &global).unwrap();
    let before = project(profile("provider", "model", "high"), &global)
        .unwrap()
        .freeze()
        .unwrap();
    for tier in ["high", "low", "high"] {
        save(&store, &a.id, &profile("provider", "model", tier)).unwrap();
        assert_eq!(choice(&store, &b.id), ReasoningChoice::Effort("low".into()));
    }
    assert_eq!(
        choice(&store, &a.id),
        ReasoningChoice::Effort("high".into())
    );
    save(&store, &b.id, &profile("provider", "model", "high")).unwrap();
    save(&store, &b.id, &profile("provider", "model", "low")).unwrap();
    assert_eq!(
        choice(&store, &a.id),
        ReasoningChoice::Effort("high".into())
    );
    assert_eq!(global, defaults("low"));
    assert_eq!(before.choice, ReasoningChoice::Effort("low".into()));
}

#[test]
fn defaults_are_initialization_only_including_empty_snapshot() {
    let store = Store::open(":memory:").unwrap();
    store.migrate().unwrap();
    let a = thread(&store);
    let b = thread(&store);
    initialize(&store, &a.id, &defaults("low")).unwrap();
    initialize(&store, &b.id, &[]).unwrap();
    initialize(&store, &a.id, &defaults("high")).unwrap();
    initialize(&store, &b.id, &defaults("high")).unwrap();
    let c = thread(&store);
    initialize(&store, &c.id, &defaults("high")).unwrap();
    assert_eq!(choice(&store, &a.id), ReasoningChoice::Effort("low".into()));
    assert_eq!(choice(&store, &b.id), ReasoningChoice::ProviderDefault);
    assert_eq!(
        choice(&store, &c.id),
        ReasoningChoice::Effort("high".into())
    );
}

#[test]
fn drafts_materialize_exact_snapshot_and_reload_after_reopen() {
    let path = std::env::temp_dir().join(format!(
        "vega-session-reasoning-{}.sqlite",
        ulid::Ulid::generate()
    ));
    let store = Store::open(&path).unwrap();
    store.migrate().unwrap();
    let mut draft = crate::threads::draft_thread(None, "model", "confirm");
    let mut preferences = defaults("low");
    set_preference(&mut preferences, &profile("provider", "model", "high")).unwrap();
    draft.reasoning_preferences = Some(preferences.clone());
    let durable = crate::threads::materialize_draft(&store, &draft).unwrap();
    assert_eq!(durable.reasoning_preferences, Some(preferences.clone()));
    drop(store);
    let reopened = Store::open_existing(&path).unwrap();
    assert_eq!(
        crate::threads::get_thread(&reopened, &durable.id)
            .unwrap()
            .reasoning_preferences,
        Some(preferences)
    );
    assert_eq!(
        choice(&reopened, &durable.id),
        ReasoningChoice::Effort("high".into())
    );
    drop(reopened);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn exact_provider_model_binding_survives_switching_and_staging() {
    let store = Store::open(":memory:").unwrap();
    store.migrate().unwrap();
    let a = thread(&store);
    initialize(&store, &a.id, &defaults("low")).unwrap();
    let current = project(profile("provider", "model", "high"), &defaults("low"))
        .unwrap()
        .freeze()
        .unwrap();
    crate::threads::stage_thread_model(&store, &a.id, "next").unwrap();
    let next = save(&store, &a.id, &profile("provider", "next", "high")).unwrap();
    assert_eq!(
        crate::threads::get_thread(&store, &a.id).unwrap().model,
        "model"
    );
    assert_eq!(choice(&store, &a.id), ReasoningChoice::Effort("low".into()));
    assert_eq!(current.choice, ReasoningChoice::Effort("low".into()));
    assert_eq!(
        project(profile("provider", "next", "low"), &next)
            .unwrap()
            .freeze()
            .unwrap()
            .choice,
        ReasoningChoice::Effort("high".into())
    );
    assert_eq!(
        project(profile("other", "model", "high"), &next)
            .unwrap()
            .preference,
        ReasoningChoice::ProviderDefault
    );
    assert_eq!(
        project(profile("provider", "absent", "high"), &next)
            .unwrap()
            .preference,
        ReasoningChoice::ProviderDefault
    );
    assert_eq!(
        vega_store::threads::pending_model(store.conn(), &a.id)
            .unwrap()
            .as_deref(),
        Some("next")
    );
}

#[test]
fn invalid_choice_or_failed_write_keeps_confirmed_value() {
    let store = Store::open(":memory:").unwrap();
    store.migrate().unwrap();
    let a = thread(&store);
    initialize(&store, &a.id, &defaults("low")).unwrap();
    assert!(save(&store, &a.id, &profile("provider", "model", "max")).is_err());
    assert_eq!(choice(&store, &a.id), ReasoningChoice::Effort("low".into()));
    store.conn().execute_batch("CREATE TRIGGER refuse_reasoning BEFORE UPDATE OF reasoning_preferences ON threads BEGIN SELECT RAISE(ABORT, 'failure'); END;").unwrap();
    assert!(save(&store, &a.id, &profile("provider", "model", "high")).is_err());
    assert_eq!(choice(&store, &a.id), ReasoningChoice::Effort("low".into()));
    store
        .conn()
        .execute_batch("DROP TRIGGER refuse_reasoning;")
        .unwrap();
    save(&store, &a.id, &profile("provider", "model", "high")).unwrap();
    assert_eq!(
        choice(&store, &a.id),
        ReasoningChoice::Effort("high".into())
    );
    assert!(save(&store, "missing", &profile("provider", "model", "high")).is_err());
}

#[test]
fn codex_draft_preserves_snapshot_including_initialized_empty_value() {
    use crate::types::*;
    let root =
        std::env::temp_dir().join(format!("vega-codex-reasoning-{}", ulid::Ulid::generate()));
    std::fs::create_dir(&root).unwrap();
    let root = std::fs::canonicalize(root).unwrap();
    let store = Store::open(":memory:").unwrap();
    store.migrate().unwrap();
    let snapshot = CodexExecutionSnapshot {
        profile: CodexProfileReference {
            id: "profile".into(),
            display_name: "Profile".into(),
        },
        adapter: CodexAdapterKind::CodexAcp,
        executable: "/usr/bin/true".into(),
        arguments: Vec::new(),
        adapter_version: "1.0.0".into(),
        codex_version: "1.0.0".into(),
        settings: CodexRunSettings {
            model: None,
            model_provider: None,
            reasoning_effort: None,
            sandbox_mode: CodexSandboxMode::ReadOnly,
            approval_policy: CodexApprovalPolicy::OnRequest,
        },
        workspace: CodexWorkspaceSnapshot {
            project_id: None,
            worktree_id: Some("worktree".into()),
            canonical_working_directory: root.to_str().unwrap().into(),
            additional_directories: Vec::new(),
        },
    };
    for preferences in [defaults("high"), Vec::new()] {
        let mut draft = crate::threads::draft_thread(None, "model", "confirm");
        draft.backend = TaskBackend::Codex;
        draft.reasoning_preferences = Some(preferences.clone());
        let materialized =
            crate::codex_tasks::materialize_codex_draft(&store, &draft, &snapshot).unwrap();
        assert_eq!(
            crate::threads::get_thread(&store, &materialized.id)
                .unwrap()
                .reasoning_preferences,
            Some(preferences)
        );
    }
    std::fs::remove_dir(root).unwrap();
}
