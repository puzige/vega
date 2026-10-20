use super::*;
use crate::model_compatibility_tests::http::{HttpFixture, Reply};
use crate::model_compatibility_tests::{
    assert_snapshot, complete, database, model, persist_message, provider,
};
use std::sync::Mutex;
use vega_runtime::model_compat::ModelApi;

#[tokio::test]
async fn manual_compaction_uses_frozen_model_and_installs_real_checkpoint_with_usage() {
    for api in [ModelApi::ChatCompletions, ModelApi::Responses] {
        let dir = tempfile::tempdir().unwrap();
        let store = database(dir.path(), "new-model");
        persist_message(
            &store,
            "user",
            1,
            "user",
            &"Previous user context ".repeat(200),
        );
        persist_message(
            &store,
            "assistant",
            2,
            "assistant",
            &"Previous assistant facts ".repeat(200),
        );
        persist_message(&store, "current-user", 3, "user", "Current continuation");
        let mut server = HttpFixture::new(vec![Reply::events(&complete(
            api,
            "Retained decisions and pending work.",
        ))])
        .await;
        let observed = Arc::new(Mutex::new(Vec::new()));
        let expected = model(api, &server.base);
        let provider = provider(api, &server.base, observed.clone());
        let result = compact_thread_manually(
            &store,
            &provider,
            "thread",
            "new-model",
            "System",
            Vec::new(),
            ContextBudget::new(100_000, 512, false).unwrap(),
            CancellationToken::new(),
            None,
            None,
        )
        .await
        .unwrap();
        assert!(
            result
                .messages
                .iter()
                .any(|message| message.content.contains("Retained decisions"))
        );
        assert!(
            !result
                .messages
                .iter()
                .any(|message| message.content.contains("private-reasoning"))
        );
        assert_eq!(result.usages.len(), 1);
        assert_eq!(
            (result.usages[0].usage.input, result.usages[0].usage.output),
            (17, 3)
        );
        assert_snapshot(&observed, &expected, 512);
        server.finish().await;
        let checkpoints: i64 = store
            .conn()
            .query_row(
                "SELECT count(*) FROM context_checkpoints WHERE thread_id='thread'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(checkpoints, 1);
    }
}

#[tokio::test]
async fn incomplete_summary_does_not_install_checkpoint_or_claim_compaction_success() {
    for chunks in [
        vec![
            serde_json::json!({"choices":[{"delta":{"content":"partial summary"},"finish_reason":""}]}),
        ],
        vec![
            serde_json::json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"forbidden-summary-call","function":{"name":"read","arguments":"{\"path\":\"README.md\"}"}}]},"finish_reason":"tool_calls"}]}),
        ],
        vec![
            serde_json::json!({"choices":[{"delta":{"content":"truncated summary"},"finish_reason":"length"}]}),
        ],
    ] {
        let dir = tempfile::tempdir().unwrap();
        let store = database(dir.path(), "new-model");
        persist_message(&store, "user", 1, "user", &"Previous history ".repeat(200));
        persist_message(&store, "assistant", 2, "assistant", "Answer");
        persist_message(&store, "current-user", 3, "user", "Current continuation");
        let mut server = HttpFixture::new(vec![Reply::events(&chunks)]).await;
        let provider = provider(
            ModelApi::ChatCompletions,
            &server.base,
            Arc::new(Mutex::new(Vec::new())),
        );
        let result = compact_thread_manually(
            &store,
            &provider,
            "thread",
            "new-model",
            "System",
            Vec::new(),
            ContextBudget::new(100_000, 512, false).unwrap(),
            CancellationToken::new(),
            None,
            None,
        )
        .await;
        assert!(result.is_err());
        server.finish().await;
        let checkpoints: i64 = store
            .conn()
            .query_row(
                "SELECT count(*) FROM context_checkpoints WHERE thread_id='thread'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(checkpoints, 0);
    }
}
