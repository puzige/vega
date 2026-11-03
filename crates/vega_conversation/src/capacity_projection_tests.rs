use crate::agent::read_context_projection_with_provider;
use crate::types::{ContextProjection, ContextSettings, RunFailureKind};
use vega_store::Store;
use vega_store::context_compaction::{ModelContextPolicy, save_model_policy};
use vega_store::messages::{MessageRow, insert as insert_message};
use vega_store::run_diagnostics::{
    DiagnosticFailureCode, DiagnosticMetrics, DiagnosticPhase, DiagnosticState, NewDiagnosticEvent,
    classified_failure_message_ids, insert as insert_diagnostic,
};
use vega_store::threads::{NewThread, create_standalone};

const THREAD: &str = "capacity-thread";
const MODEL: &str = "gpt-6.1-sol";
const PROVIDER: &str = "cpa";
const POLICY_UPDATED_AT: i64 = 42;

fn configured_policy(provider: &str) -> ModelContextPolicy {
    ModelContextPolicy {
        provider: provider.into(),
        model: MODEL.into(),
        input_limit: Some(300_000),
        output_reserve: Some(128_000),
        automatic_compaction: true,
        updated_at: POLICY_UPDATED_AT,
    }
}

fn store_with_capacity() -> Store {
    let store = Store::open(":memory:").expect("open in-memory store");
    store.migrate().expect("migrate in-memory store");
    create_standalone(
        store.conn(),
        NewThread {
            id: THREAD,
            project_id: "",
            title: "Capacity projection",
            mode: "ask",
            permission_mode: "readonly",
            permission_revision: 0,
            full_access_grant: None,
            model: MODEL,
            status: "active",
            pinned: false,
            unread: false,
            created_at: 1,
            updated_at: 1,
            reasoning_preferences: None,
        },
    )
    .expect("create native thread");
    save_model_policy(store.conn(), &configured_policy(PROVIDER))
        .expect("save configured capacity");
    store
}

fn append_failed_run(
    store: &Store,
    owner: &str,
    code: DiagnosticFailureCode,
    metrics: DiagnosticMetrics,
) {
    let first_seq = vega_store::messages::next_seq(store.conn(), THREAD).expect("next message seq");
    for (offset, id, role, content, status) in [
        (
            0,
            format!("{owner}-user"),
            "user",
            "Continue the task",
            "done",
        ),
        (1, owner.into(), "assistant", "Partial response", "failed"),
    ] {
        insert_message(
            store.conn(),
            &MessageRow {
                id,
                thread_id: THREAD.into(),
                seq: first_seq + offset,
                role: role.into(),
                kind: "text".into(),
                content: content.into(),
                status: status.into(),
                created_at: first_seq + offset,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .expect("persist conversation message");
    }
    let started = NewDiagnosticEvent {
        thread_id: THREAD.into(),
        run_id: owner.into(),
        attempt_id: format!("{owner}-attempt"),
        parent_attempt_id: None,
        tool_call_id: None,
        phase: DiagnosticPhase::Run,
        state: DiagnosticState::Started,
        failure_code: None,
        occurred_at: first_seq,
        duration_ms: None,
        metrics: DiagnosticMetrics::default(),
    };
    insert_diagnostic(store.conn(), &started).expect("persist run start");
    insert_diagnostic(
        store.conn(),
        &NewDiagnosticEvent {
            state: DiagnosticState::Failed,
            failure_code: Some(code),
            occurred_at: first_seq + 1,
            duration_ms: Some(1),
            metrics,
            ..started
        },
    )
    .expect("persist classified run failure");
}

fn read_projection(store: &Store) -> ContextProjection {
    read_context_projection_with_provider(
        store,
        THREAD,
        MODEL,
        Some(PROVIDER),
        "You are a helpful assistant.",
    )
    .expect("project configured capacity alongside historical diagnostics")
}

fn assert_configured_capacity(projection: &ContextProjection) {
    assert_eq!(projection.model_input_limit, Some(300_000));
    assert_eq!(
        projection.settings,
        Some(ContextSettings {
            thread_id: THREAD.into(),
            model: MODEL.into(),
            context_limit: Some(428_000),
            output_reserve: 128_000,
            automatic_compaction: true,
            updated_at: POLICY_UPDATED_AT,
        })
    );
}

#[test]
fn capacity_projection_preserves_failure_with_null_trailing_metrics() {
    let store = store_with_capacity();
    let metrics = DiagnosticMetrics {
        failure_source: Some("provider".into()),
        failure_kind: Some("protocol".into()),
        ..DiagnosticMetrics::default()
    };
    append_failed_run(
        &store,
        "failed-with-null-details",
        DiagnosticFailureCode::ProviderProtocol,
        metrics.clone(),
    );

    let projection = read_projection(&store);

    assert_configured_capacity(&projection);
    assert_eq!(
        projection.run_failures,
        vec![(
            "failed-with-null-details".into(),
            RunFailureKind::ProviderProtocol
        )]
    );
    assert_eq!(
        classified_failure_message_ids(store.conn(), THREAD)
            .expect("read nullable diagnostic fields"),
        vec![("failed-with-null-details".into(), metrics)]
    );
}

#[test]
fn capacity_projection_preserves_distinct_protocol_and_credential_failure_owners() {
    let store = store_with_capacity();
    let protocol_metrics = DiagnosticMetrics {
        failure_source: Some("provider".into()),
        failure_kind: Some("protocol".into()),
        protocol_reason: Some("missing_terminal".into()),
        ..DiagnosticMetrics::default()
    };
    let credential_metrics = DiagnosticMetrics {
        failure_source: Some("runtime".into()),
        failure_kind: Some("credential_check".into()),
        credential_stage: Some("provider".into()),
        credential_reason: Some("missing".into()),
        ..DiagnosticMetrics::default()
    };
    append_failed_run(
        &store,
        "protocol-owner",
        DiagnosticFailureCode::ProviderProtocol,
        protocol_metrics.clone(),
    );
    append_failed_run(
        &store,
        "credential-owner",
        DiagnosticFailureCode::RuntimeFailure,
        credential_metrics.clone(),
    );

    let projection = read_projection(&store);

    assert_configured_capacity(&projection);
    assert_eq!(
        projection.run_failures,
        vec![
            ("protocol-owner".into(), RunFailureKind::ProviderProtocol),
            (
                "credential-owner".into(),
                RunFailureKind::CredentialStorageUnavailable
            ),
        ]
    );
    assert_eq!(
        classified_failure_message_ids(store.conn(), THREAD)
            .expect("read populated diagnostic fields"),
        vec![
            ("protocol-owner".into(), protocol_metrics),
            ("credential-owner".into(), credential_metrics),
        ]
    );
}

#[test]
fn explicitly_unknown_capacity_stays_unknown_despite_other_provider_capacity_and_failure() {
    let store = store_with_capacity();
    save_model_policy(
        store.conn(),
        &ModelContextPolicy::unconfigured(PROVIDER, MODEL),
    )
    .expect("save explicitly unknown capacity");
    save_model_policy(store.conn(), &configured_policy("other-provider"))
        .expect("save unrelated provider capacity");
    append_failed_run(
        &store,
        "unknown-capacity-owner",
        DiagnosticFailureCode::ProviderProtocol,
        DiagnosticMetrics {
            failure_source: Some("provider".into()),
            failure_kind: Some("protocol".into()),
            protocol_reason: Some("missing_terminal".into()),
            ..DiagnosticMetrics::default()
        },
    );

    let projection = read_projection(&store);

    assert_eq!(projection.model_input_limit, None);
    assert_eq!(projection.settings, None);
    assert!(!projection.compactable);
    assert_eq!(
        projection.run_failures,
        vec![(
            "unknown-capacity-owner".into(),
            RunFailureKind::ProviderProtocol
        )]
    );
}
