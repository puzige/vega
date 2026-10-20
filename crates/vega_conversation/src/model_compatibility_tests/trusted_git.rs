use super::*;
use crate::model_compatibility_tests::http::{HttpFixture, Reply};
use crate::model_compatibility_tests::{assert_snapshot, complete, model, provider};
use vega_runtime::model_compat::ModelApi;

#[tokio::test]
async fn commit_draft_request_and_collector_use_real_bound_model_and_native_wire() {
    for api in [ModelApi::ChatCompletions, ModelApi::Responses] {
        let mut server = HttpFixture::new(vec![Reply::events(&complete(
            api,
            "fix: preserve model capability selection",
        ))])
        .await;
        let observed = Arc::new(Mutex::new(Vec::new()));
        let expected = model(api, &server.base);
        let request = draft_request("new-model".into(), "owned staged diff", false);
        assert!(request.messages[1].content.contains("truncated=false"));
        let draft = collect_draft_with_deadline(
            Arc::new(provider(api, &server.base, observed.clone())),
            request,
            CancellationToken::new(),
            Duration::from_secs(6),
        )
        .await
        .unwrap();
        assert_eq!(draft, "fix: preserve model capability selection");
        assert_snapshot(&observed, &expected, 256);
        server.finish().await;
        let received = server.received.lock().unwrap();
        assert_eq!(received.len(), 1);
        assert!(received[0].body.get("reasoning").is_none());
        assert!(received[0].body.get("reasoning_effort").is_none());
    }
}

#[tokio::test]
async fn commit_draft_rejects_tool_output_incomplete_terminal_and_oversized_text() {
    for chunks in [
        vec![
            serde_json::json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call","function":{"name":"read","arguments":"{}"}}]},"finish_reason":"tool_calls"}]}),
        ],
        vec![serde_json::json!({"choices":[{"delta":{"content":"partial"},"finish_reason":""}]})],
        complete(ModelApi::ChatCompletions, &"x".repeat(32 * 1024 + 1)),
    ] {
        let mut server = HttpFixture::new(vec![Reply::events(&chunks)]).await;
        let request = draft_request("new-model".into(), "owned staged diff", true);
        assert!(request.messages[1].content.contains("truncated=true"));
        let result = collect_draft_with_deadline(
            Arc::new(provider(
                ModelApi::ChatCompletions,
                &server.base,
                Arc::new(Mutex::new(Vec::new())),
            )),
            request,
            CancellationToken::new(),
            Duration::from_secs(6),
        )
        .await;
        assert!(matches!(result, Err(CommitErrorCode::DraftFailed)));
        server.finish().await;
    }
}
