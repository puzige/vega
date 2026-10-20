use super::*;
use crate::model_compatibility_tests::http::{HttpFixture, Reply};
use crate::model_compatibility_tests::{
    assert_snapshot, complete, database, model, persist_message, provider,
};
use std::sync::Mutex;
use vega_runtime::model_compat::ModelApi;

#[tokio::test]
async fn prediction_collect_uses_bound_default_snapshot_and_parses_actual_json_text() {
    for api in [ModelApi::ChatCompletions, ModelApi::Responses] {
        let mut server = HttpFixture::new(vec![Reply::events(&complete(
            api,
            r#"{"suggestion":"Add a regression test"}"#,
        ))])
        .await;
        let observed = Arc::new(Mutex::new(Vec::new()));
        let expected = model(api, &server.base);
        let request = ComposerPredictionRequest::new(
            Arc::new(provider(api, &server.base, observed.clone())),
            "new-model".into(),
            ThreadMode::Execute,
        );
        let (suggestion, usage) = request
            .collect("[]".into(), &CancellationToken::new())
            .await;
        assert_eq!(suggestion.as_deref(), Some("Add a regression test"));
        let usage = usage.unwrap();
        assert_eq!((usage.input, usage.output), (17, 3));
        assert_snapshot(&observed, &expected, 512);
        server.finish().await;
    }
}

#[tokio::test]
async fn prediction_run_observes_real_owner_and_records_usage_without_rewriting_history() {
    let dir = tempfile::tempdir().unwrap();
    let store = database(dir.path(), "new-model");
    persist_message(&store, "user", 1, "user", "Original question");
    persist_message(&store, "assistant", 2, "assistant", "Visible response");
    let mut server = HttpFixture::new(vec![Reply::events(&complete(
        ModelApi::ChatCompletions,
        r#"{"suggestion":"Inspect the result"}"#,
    ))])
    .await;
    let observed = Arc::new(Mutex::new(Vec::new()));
    let expected = model(ModelApi::ChatCompletions, &server.base);
    let request = ComposerPredictionRequest::new(
        Arc::new(provider(
            ModelApi::ChatCompletions,
            &server.base,
            observed.clone(),
        )),
        "new-model".into(),
        ThreadMode::Execute,
    );
    let database = dir.path().join("store.db");
    let suggestion = tokio::task::spawn_blocking(move || {
        request.run(
            database,
            "thread".into(),
            "assistant".into(),
            CancellationToken::new(),
            None,
        )
    })
    .await
    .unwrap();
    assert_eq!(suggestion.as_deref(), Some("Inspect the result"));
    assert_snapshot(&observed, &expected, 512);
    server.finish().await;
    let count: i64 = store
        .conn()
        .query_row(
            "SELECT count(*) FROM messages WHERE thread_id='thread'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 2);
    let tokens: (u64, u64) = store
        .conn()
        .query_row(
            "SELECT input_tokens,output_tokens FROM token_usage WHERE thread_id='thread'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(tokens, (17, 3));
}

#[tokio::test]
async fn prediction_invalid_terminal_or_json_is_not_a_user_suggestion() {
    for chunks in [
        complete(ModelApi::ChatCompletions, r#"{"unexpected":"value"}"#),
        vec![
            serde_json::json!({"choices":[{"delta":{"content":"{\"suggestion\":\"partial\"}"},"finish_reason":""}]}),
        ],
    ] {
        let mut server = HttpFixture::new(vec![Reply::events(&chunks)]).await;
        let request = ComposerPredictionRequest::new(
            Arc::new(provider(
                ModelApi::ChatCompletions,
                &server.base,
                Arc::new(Mutex::new(Vec::new())),
            )),
            "new-model".into(),
            ThreadMode::Execute,
        );
        assert!(
            request
                .collect("[]".into(), &CancellationToken::new())
                .await
                .0
                .is_none()
        );
        server.finish().await;
    }
}
