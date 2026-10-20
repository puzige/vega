use futures::StreamExt;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use vega_runtime::model_compat::{
    ModelApi, ModelIdentity, ModelRoute, ResolvedModel, lookup_model_definition, resolve_model,
};
use vega_runtime::{
    ChatMessage, ChatRequest, ChatRole, OpenAiProvider, Provider, ProviderEvent, RetryPolicy,
};
use vega_store::Store;

#[path = "../../../vega_runtime/src/openai/compatibility_tests/http.rs"]
pub(crate) mod http;
use http::{HttpFixture, Reply};

mod lifecycle;

pub(crate) fn database(root: &std::path::Path, model: &str) -> Store {
    let store = Store::open(root.join("store.db")).unwrap();
    store.migrate().unwrap();
    let project =
        vega_store::projects::create(store.conn(), root.to_str().unwrap(), "Owned fixture", None)
            .unwrap();
    vega_store::threads::create(
        store.conn(),
        vega_store::threads::NewThread {
            id: "thread",
            project_id: &project.id,
            title: "Fixture",
            mode: "execute",
            permission_mode: "readonly",
            model,
            status: "active",
            pinned: false,
            unread: false,
            created_at: 1,
            updated_at: 1,
        },
    )
    .unwrap();
    store
}

pub(crate) fn model(api: ModelApi, endpoint: &str) -> ResolvedModel {
    let route = ModelRoute::new(api, endpoint).unwrap();
    let source = lookup_model_definition(
        &route,
        &ModelIdentity {
            provider: "owned".into(),
            model: "new-model".into(),
        },
    )
    .unwrap();
    resolve_model(&source, None).unwrap()
}

pub(crate) fn provider(
    api: ModelApi,
    endpoint: &str,
    observed: Arc<Mutex<Vec<ChatRequest>>>,
) -> OpenAiProvider {
    OpenAiProvider::new(endpoint, "owned-fixture-key")
        .unwrap()
        .with_responses_api(api == ModelApi::Responses)
        .with_retry_policy(RetryPolicy {
            max_retries: 0,
            base_delay: Duration::ZERO,
            max_delay: Duration::ZERO,
        })
        .with_model_snapshot(model(api, endpoint))
        .unwrap()
        .with_pre_attempt_guard(move |request| {
            observed.lock().unwrap().push(request.clone());
            Ok(())
        })
}

pub(crate) fn complete(api: ModelApi, text: &str) -> Vec<Value> {
    if api == ModelApi::Responses {
        vec![
            json!({"type":"response.completed","response":{"id":"r","status":"completed","output":[{"type":"message","id":"m","role":"assistant","content":[{"type":"output_text","text":text}]}],"usage":{"input_tokens":17,"output_tokens":3,"input_tokens_details":{"cached_tokens":5}}}}),
        ]
    } else {
        vec![
            json!({"choices":[{"delta":{},"finish_reason":""}]}),
            json!({"choices":[{"delta":{"content":text},"finish_reason":null}]}),
            json!({"choices":[{"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":17,"completion_tokens":3,"prompt_tokens_details":{"cached_tokens":5}}}),
        ]
    }
}

pub(crate) fn assert_snapshot(
    observed: &Arc<Mutex<Vec<ChatRequest>>>,
    expected: &ResolvedModel,
    cap: u32,
) {
    let requests = observed.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].model_snapshot.as_deref(), Some(expected));
    assert_eq!(requests[0].max_tokens, Some(cap));
    assert!(requests[0].tools.is_empty());
    assert!(
        requests[0].reasoning.as_ref().is_none_or(
            |reasoning| reasoning.choice == vega_runtime::ReasoningChoice::ProviderDefault
        )
    );
}

pub(crate) fn persist_message(store: &Store, id: &str, seq: i64, role: &str, content: &str) {
    vega_store::messages::insert(
        store.conn(),
        &vega_store::messages::MessageRow {
            id: id.into(),
            thread_id: "thread".into(),
            seq,
            role: role.into(),
            kind: "text".into(),
            content: content.into(),
            status: "done".into(),
            created_at: seq,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .unwrap();
}

mod settings_service;
