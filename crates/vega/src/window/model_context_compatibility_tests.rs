use super::*;
use vega_store::model_capabilities::{ModelCapabilityConfig, ModelCapabilityRecord};

use crate::compatibility_http::{HttpFixture, Reply};

#[tokio::test]
async fn manual_context_worker_factory_consumes_native_snapshot_and_keeps_capacity_authority() {
    for api in [
        vega_store::config::ProviderApi::ChatCompletions,
        vega_store::config::ProviderApi::Responses,
    ] {
        let dir = tempfile::tempdir().unwrap();
        let chunks = if api == vega_store::config::ProviderApi::Responses {
            vec![
                serde_json::json!({"type":"response.completed","response":{"id":"r","status":"completed","output":[{"type":"message","id":"m","role":"assistant","content":[{"type":"output_text","text":"Retained decisions"}]}],"usage":{"input_tokens":17,"output_tokens":3}}}),
            ]
        } else {
            vec![
                serde_json::json!({"choices":[{"delta":{"content":"Retained decisions"},"finish_reason":""}]}),
                serde_json::json!({"choices":[{"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":17,"completion_tokens":3}}),
            ]
        };
        let mut server = HttpFixture::new(vec![Reply::events(&chunks)]).await;
        let config = dir.path().join("config.toml");
        vega_store::config::AppConfig {
            providers: vec![vega_store::config::ProviderConfig {
                name: "fixture".into(),
                base_url: server.base.clone(),
                models: vec!["future".into()],
                key_ref: "key".into(),
                api,
                enabled: true,
            }],
            ..Default::default()
        }
        .save_to(&config)
        .unwrap();
        vega_store::keystore::set_key(dir.path(), "key", "owned-fixture-key").unwrap();
        let db = dir.path().join("store.db");
        let store = Store::open(&db).unwrap();
        store.migrate().unwrap();
        vega_store::threads::create_standalone(
            store.conn(),
            vega_store::threads::NewThread {
                id: "thread",
                project_id: "",
                title: "Fixture",
                mode: "execute",
                permission_mode: "readonly",
                model: "future",
                status: "active",
                pinned: false,
                unread: false,
                created_at: 1,
                updated_at: 1,
            },
        )
        .unwrap();
        for (id, seq, role, text) in [
            ("u", 1, "user", "Previous request ".repeat(200)),
            ("a", 2, "assistant", "Previous answer ".repeat(200)),
            ("u2", 3, "user", "Current continuation".into()),
        ] {
            vega_store::messages::insert(
                store.conn(),
                &vega_store::messages::MessageRow {
                    id: id.into(),
                    thread_id: "thread".into(),
                    seq,
                    role: role.into(),
                    kind: "text".into(),
                    content: text,
                    status: "done".into(),
                    created_at: seq,
                    plan_status: None,
                    plan_review_note: None,
                    plan_reviewed_at: None,
                },
            )
            .unwrap();
        }
        let policy = vega_store::context_compaction::ModelContextPolicy {
            provider: "fixture".into(),
            model: "future".into(),
            input_limit: Some(100_000),
            output_reserve: Some(512),
            automatic_compaction: false,
            updated_at: 1,
        };
        vega_store::context_compaction::save_model_policy(store.conn(), &policy).unwrap();
        let cap = dir.path().join("model-capabilities.toml");
        std::fs::write(
            &cap,
            vega_store::model_capabilities::encode(&ModelCapabilityConfig {
                version: 1,
                models: vec![ModelCapabilityRecord {
                    provider: "fixture".into(),
                    model: "future".into(),
                    allowed_efforts: Some(Vec::new()),
                    image_input: None,
                }],
            })
            .unwrap(),
        )
        .unwrap();
        let before = std::fs::read(&cap).unwrap();
        let result = tokio::task::spawn_blocking(move || {
            run_manual_context_worker(
                db,
                Some(config),
                "thread".into(),
                "future".into(),
                1,
                CancellationToken::new(),
                None,
                None,
            )
        })
        .await
        .unwrap();
        assert!(result.is_ok());
        server.finish().await;
        assert_eq!(
            server.received.lock().unwrap()[0].path,
            if api == vega_store::config::ProviderApi::Responses {
                "/v1/responses"
            } else {
                "/v1/chat/completions"
            }
        );
        let body = &server.received.lock().unwrap()[0].body;
        assert_eq!(
            body[if api == vega_store::config::ProviderApi::Responses {
                "max_output_tokens"
            } else {
                "max_tokens"
            }],
            512
        );
        assert!(body.get("reasoning").is_none());
        assert!(body.get("reasoning_effort").is_none());
        assert_eq!(
            vega_store::context_compaction::load_model_policy(store.conn(), "fixture", "future")
                .unwrap(),
            Some(policy)
        );
        assert_eq!(std::fs::read(&cap).unwrap(), before);
    }
}
