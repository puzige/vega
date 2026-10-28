use super::*;
use crate::model_compat::{ModelApi, request_model};
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn background_factory_survives_origin_runtime_and_preserves_frozen_request_guard()
-> Result<(), Box<dyn std::error::Error>> {
    for api in [ModelApi::ChatCompletions, ModelApi::Responses] {
        let origin = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let observed = Arc::new(AtomicUsize::new(0));
        let request = crate::ChatRequest {
            model: "synthetic-model".into(),
            messages: vec![crate::ChatMessage::new(
                crate::ChatRole::User,
                "Synthetic task",
            )],
            max_tokens: Some(100),
            ..Default::default()
        };
        let model = request_model(&request, api, "https://example.invalid/v1")?;
        let expected = model.clone();
        let guard_calls = observed.clone();
        let factory = origin.block_on(async {
            OpenAiProvider::new("https://example.invalid/v1", "synthetic-not-a-secret")?
                .with_responses_api(api == ModelApi::Responses)
                .with_model_snapshot(model)?
                .with_retry_policy(RetryPolicy {
                    max_retries: 7,
                    ..Default::default()
                })
                .with_pre_attempt_guard(move |request| {
                    assert_eq!(request.model_snapshot.as_deref(), Some(&expected));
                    guard_calls.fetch_add(1, Ordering::Relaxed);
                    Err(VegaError::CredentialCheckUnavailable)
                })
                .compaction_provider_factory()
                .ok_or(VegaError::CredentialCheckUnavailable)
        })?;
        drop(origin);
        let worker = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        worker.block_on(async {
            let provider = factory()?;
            assert_eq!(
                provider.model_snapshot(),
                request_model(&request, api, "https://example.invalid/v1").ok()
            );
            let submission = crate::ProviderRequestSubmission::default();
            assert!(matches!(
                provider
                    .chat_stream_with_submission(
                        request,
                        CancellationToken::new(),
                        submission.clone()
                    )
                    .await,
                Err(VegaError::CredentialCheckUnavailable)
            ));
            assert!(!submission.may_have_been_submitted());
            Ok::<_, VegaError>(())
        })?;
        assert_eq!(observed.load(Ordering::Relaxed), 1);
    }
    Ok(())
}
