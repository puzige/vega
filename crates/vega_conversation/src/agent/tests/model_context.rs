use super::*;
use vega_store::context_compaction::{
    ContextSettings as LegacyContextSettings, ModelContextPolicy, save_model_policy, save_settings,
};

fn policy(provider: &str, input: u64, reserve: u64) -> ModelContextPolicy {
    ModelContextPolicy {
        provider: provider.into(),
        model: "mock-model".into(),
        input_limit: Some(input),
        output_reserve: Some(reserve),
        automatic_compaction: true,
        updated_at: 1,
    }
}

fn prepare(
    store: &Store,
    thread_id: &str,
    provider: &str,
    suffix: &str,
) -> Result<PreparedRun, crate::types::ConversationError> {
    prepare_run_with_images_and_reasoning(
        store.database_path().unwrap().to_path_buf(),
        thread_id.into(),
        "hello".into(),
        "system".into(),
        format!("user-{suffix}"),
        format!("assistant-{suffix}"),
        PersistenceActorConfig::default(),
        false,
        None,
        Some(FrozenReasoning::unknown(provider, "mock-model")),
        Vec::new(),
    )
}

#[test]
fn model_owned_budget_is_shared_across_threads_and_frozen_before_tool_rounds() {
    let (store, _dir, project_id) = setup();
    vega_store::threads::create(
        store.conn(),
        vega_store::threads::NewThread {
            id: "thread-2",
            project_id: &project_id,
            title: "",
            mode: "execute",
            permission_mode: "confirm",
            model: "mock-model",
            status: "active",
            pinned: false,
            unread: false,
            created_at: 1,
            updated_at: 1,
        },
    )
    .unwrap();
    save_model_policy(store.conn(), &policy("provider-a", 8_000, 2_000)).unwrap();
    let first = prepare(&store, "thread-1", "provider-a", "first").unwrap();
    let second = prepare(&store, "thread-2", "provider-a", "second").unwrap();
    let expected = Some(vega_runtime::ContextBudget::new(10_000, 2_000, true).unwrap());
    assert_eq!(first.request.context_budget, expected);
    assert_eq!(second.request.context_budget, expected);

    save_model_policy(store.conn(), &policy("provider-a", 16_000, 4_000)).unwrap();
    assert_eq!(first.request.context_budget, expected);
    assert_eq!(second.request.context_budget, expected);
}

#[test]
fn same_model_id_different_provider_uses_only_exact_model_policy() {
    let (store, _dir, project_id) = setup();
    vega_store::threads::create(
        store.conn(),
        vega_store::threads::NewThread {
            id: "thread-2",
            project_id: &project_id,
            title: "",
            mode: "execute",
            permission_mode: "confirm",
            model: "mock-model",
            status: "active",
            pinned: false,
            unread: false,
            created_at: 1,
            updated_at: 1,
        },
    )
    .unwrap();
    save_model_policy(store.conn(), &policy("provider-a", 8_000, 2_000)).unwrap();
    save_model_policy(store.conn(), &policy("provider-b", 32_000, 8_000)).unwrap();
    let a = prepare(&store, "thread-1", "provider-a", "a").unwrap();
    let b = prepare(&store, "thread-2", "provider-b", "b").unwrap();
    assert_eq!(
        a.request.context_budget,
        Some(vega_runtime::ContextBudget::new(10_000, 2_000, true).unwrap())
    );
    assert_eq!(
        b.request.context_budget,
        Some(vega_runtime::ContextBudget::new(40_000, 8_000, true).unwrap())
    );
}

#[test]
fn missing_model_policy_uses_default_budget_not_legacy_thread_settings() {
    let (store, _dir, _) = setup();
    save_settings(
        store.conn(),
        &LegacyContextSettings {
            thread_id: "thread-1".into(),
            model: "mock-model".into(),
            context_limit: Some(10_000),
            output_reserve: 2_000,
            automatic_compaction: true,
            updated_at: 1,
        },
    )
    .unwrap();
    let prepared = prepare(&store, "thread-1", "provider-a", "legacy").unwrap();
    assert_eq!(
        prepared.request.context_budget,
        Some(vega_runtime::ContextBudget::new(428_000, 128_000, true).unwrap())
    );
    assert!(
        vega_store::context_compaction::load_settings(store.conn(), "thread-1", "mock-model")
            .unwrap()
            .is_some()
    );
}

#[test]
fn explicit_unknown_model_capacity_keeps_the_sendable_unbudgeted_path() {
    let (store, _dir, _) = setup();
    save_model_policy(
        store.conn(),
        &ModelContextPolicy::unconfigured("provider-a", "mock-model"),
    )
    .unwrap();
    let prepared = prepare(&store, "thread-1", "provider-a", "unknown").unwrap();
    assert_eq!(prepared.request.context_budget, None);
}

#[test]
fn missing_provider_identity_never_guesses_a_model_policy() {
    let (store, _dir, _) = setup();
    save_model_policy(store.conn(), &policy("provider-a", 8_000, 2_000)).unwrap();
    let prepared = prepare_run_with_images_and_reasoning(
        store.database_path().unwrap().to_path_buf(),
        "thread-1".into(),
        "hello".into(),
        "system".into(),
        "user-no-provider".into(),
        "assistant-no-provider".into(),
        PersistenceActorConfig::default(),
        false,
        None,
        None,
        Vec::new(),
    )
    .unwrap();
    assert_eq!(prepared.request.context_budget, None);
}

#[test]
fn independent_input_output_limits_convert_to_bounded_total() {
    let (store, _dir, _) = setup();
    save_model_policy(store.conn(), &policy("provider-a", u32::MAX as u64 - 1, 1)).unwrap();
    let prepared = prepare(&store, "thread-1", "provider-a", "limit").unwrap();
    let budget = prepared.request.context_budget.unwrap();
    assert_eq!(budget.total_limit(), u32::MAX as u64);
    assert_eq!(budget.input_budget(), u32::MAX as u64 - 1);
    assert_eq!(budget.output_reserve(), 1);
}

#[test]
fn model_policy_cannot_be_applied_to_a_different_thread_model() {
    let (store, _dir, _) = setup();
    save_model_policy(store.conn(), &policy("provider-a", 8_000, 2_000)).unwrap();
    let result = prepare_run_with_images_and_reasoning(
        store.database_path().unwrap().to_path_buf(),
        "thread-1".into(),
        "hello".into(),
        "system".into(),
        "user-mismatch".into(),
        "assistant-mismatch".into(),
        PersistenceActorConfig::default(),
        false,
        None,
        Some(FrozenReasoning::unknown("provider-a", "other-model")),
        Vec::new(),
    );
    assert!(matches!(
        result,
        Err(crate::types::ConversationError::CorruptRow(_))
    ));
    assert!(
        vega_store::messages::find(store.conn(), "user-mismatch")
            .unwrap()
            .is_none()
    );
}

#[test]
fn issue114_configured_turn_limit_reaches_runtime_tool_config() {
    let (store, _dir, _) = setup();
    let prepared = prepare_run_with_images_and_reasoning(
        store.database_path().unwrap().to_path_buf(),
        "thread-1".into(),
        "hello".into(),
        "system".into(),
        "user-turn-limit".into(),
        "assistant-turn-limit".into(),
        PersistenceActorConfig::default().with_turn_limit(7),
        false,
        None,
        Some(FrozenReasoning::unknown("provider-a", "mock-model")),
        Vec::new(),
    )
    .unwrap();
    assert_eq!(prepared.request.tool_config.turn_limit, 7);

    // The default stays unlimited (0) for legacy callers.
    let unlimited = prepare(&store, "thread-1", "provider-a", "unlimited").unwrap();
    assert_eq!(unlimited.request.tool_config.turn_limit, 0);
}
