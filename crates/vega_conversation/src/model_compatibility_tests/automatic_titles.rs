use super::*;
use crate::model_compatibility_tests::http::{HttpFixture, Reply};
use crate::model_compatibility_tests::{
    assert_snapshot, complete, database, model, persist_message, provider,
};
use std::sync::Mutex;
use vega_runtime::model_compat::ModelApi;

#[tokio::test]
async fn automatic_title_collect_uses_bound_default_snapshot_and_authentic_terminal() {
    for api in [ModelApi::ChatCompletions, ModelApi::Responses] {
        let mut server =
            HttpFixture::new(vec![Reply::events(&complete(api, "A useful title"))]).await;
        let observed = Arc::new(Mutex::new(Vec::new()));
        let expected = model(api, &server.base);
        let provider = Arc::new(provider(api, &server.base, observed.clone()));
        let (notifications, _) = mpsc::channel();
        let request = AutomaticTitleRequest::new(
            "User source",
            provider,
            CancellationToken::new(),
            notifications,
        );
        let (title, usage) = request.collect("new-model").await;
        assert_eq!(title.as_deref(), Some("A useful title"));
        assert_eq!(usage, Some((17, 3, Some(5), None)));
        assert_snapshot(&observed, &expected, 512);
        server.finish().await;
        let body = &server.received.lock().unwrap()[0].body;
        assert!(body.get("reasoning").is_none());
        assert!(body.get("reasoning_effort").is_none());
    }
}

#[tokio::test]
async fn title_launch_updates_only_claimed_title_and_records_real_total_usage() {
    let dir = tempfile::tempdir().unwrap();
    let store = database(dir.path(), "new-model");
    persist_message(&store, "user", 1, "user", "Original question");
    store
        .conn()
        .execute(
            "UPDATE threads SET title='',auto_title_state='eligible' WHERE id='thread'",
            [],
        )
        .unwrap();
    assert!(
        vega_store::threads::claim_auto_title(store.conn(), "thread", "user", "Fallback").unwrap()
    );
    let mut server = HttpFixture::new(vec![Reply::events(&complete(
        ModelApi::ChatCompletions,
        "Generated title",
    ))])
    .await;
    let observed = Arc::new(Mutex::new(Vec::new()));
    let expected = model(ModelApi::ChatCompletions, &server.base);
    let (notifications, _) = mpsc::channel();
    AutomaticTitleRequest::new(
        "Original question",
        Arc::new(provider(
            ModelApi::ChatCompletions,
            &server.base,
            observed.clone(),
        )),
        CancellationToken::new(),
        notifications,
    )
    .launch(
        dir.path().join("store.db"),
        "thread".into(),
        "user".into(),
        "new-model".into(),
        None,
    );
    let deadline = tokio::time::Instant::now() + Duration::from_secs(6);
    loop {
        let title = vega_store::threads::find(store.conn(), "thread")
            .unwrap()
            .unwrap()
            .title;
        if title == "Generated title" {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "title worker did not complete"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_snapshot(&observed, &expected, 512);
    server.finish().await;
    let tokens: (i64, i64) = store
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
async fn title_incomplete_and_tool_outputs_never_become_a_generated_title() {
    for chunks in [
        vec![serde_json::json!({"choices":[{"delta":{"content":"partial"},"finish_reason":""}]})],
        vec![
            serde_json::json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call","function":{"name":"read","arguments":"{}"}}]},"finish_reason":"tool_calls"}]}),
        ],
    ] {
        let mut server = HttpFixture::new(vec![Reply::events(&chunks)]).await;
        let (notifications, _) = mpsc::channel();
        let observed = Arc::new(Mutex::new(Vec::new()));
        let request = AutomaticTitleRequest::new(
            "Question",
            Arc::new(provider(ModelApi::ChatCompletions, &server.base, observed)),
            CancellationToken::new(),
            notifications,
        );
        let (title, _) = request.collect("new-model").await;
        assert!(title.is_none());
        server.finish().await;
    }
}
