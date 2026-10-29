use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use vega_runtime::{
    ChatMessage, ChatRequest, ChatRole, ProviderEvent, ProviderRequestSubmission,
    ProviderResponseMetadata, ProviderStream, StopReason,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const SECRET: &str = "synthetic-owner-secret-abcdefghijklmnopqrstuvwxyz";

#[derive(Default)]
struct ProviderState {
    factories: AtomicUsize,
    requests: AtomicUsize,
    reject: AtomicBool,
    echo_secret: AtomicBool,
}

struct IndependentProvider(Arc<ProviderState>);

impl Provider for IndependentProvider {
    fn compaction_provider_factory(&self) -> Option<vega_runtime::CompactionProviderFactory> {
        let state = self.0.clone();
        Some(Box::new(move || {
            state.factories.fetch_add(1, Ordering::AcqRel);
            Ok(Arc::new(Self(state)))
        }))
    }

    fn chat_stream_with_submission(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
        submission: ProviderRequestSubmission,
    ) -> BoxFuture<'static, Result<ProviderStream, VegaError>> {
        if cancel.is_cancelled() {
            return Box::pin(async { Err(VegaError::Cancelled) });
        }
        if self.0.reject.load(Ordering::Acquire) {
            return Box::pin(async { Err(VegaError::CredentialCheckUnavailable) });
        }
        submission.mark_possible_submission();
        self.chat_stream_with_metadata(request, cancel)
    }

    fn chat_stream_with_metadata(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<ProviderStream, VegaError>> {
        let response = self.chat_stream(request, cancel);
        let secret = self.0.echo_secret.load(Ordering::Acquire);
        Box::pin(async move {
            Ok(ProviderStream {
                events: response.await?,
                metadata: ProviderResponseMetadata {
                    http_status: Some(200),
                    retry_count: Some(2),
                    request_id: Some(if secret { SECRET } else { "request-safe" }.into()),
                },
            })
        })
    }

    fn chat_stream(
        &self,
        _: ChatRequest,
        _: CancellationToken,
    ) -> BoxFuture<'static, Result<vega_runtime::EventStream, VegaError>> {
        self.0.requests.fetch_add(1, Ordering::AcqRel);
        let text = if self.0.echo_secret.load(Ordering::Acquire) {
            SECRET
        } else {
            "Summary retained."
        };
        Box::pin(async move {
            let events: vega_runtime::EventStream = Box::pin(futures::stream::iter([
                Ok(ProviderEvent::TextDelta(text.into())),
                Ok(ProviderEvent::Done {
                    stop_reason: StopReason::End,
                }),
            ]));
            Ok(events)
        })
    }
}

fn request(text: &str) -> ChatRequest {
    ChatRequest {
        model_snapshot: None,
        tool_choice: None,
        model: "model".into(),
        messages: vec![ChatMessage::new(ChatRole::User, text)],
        tools: Vec::new(),
        max_tokens: Some(100),
        reasoning: None,
    }
}

#[tokio::test]
async fn owned_child_rechecks_credentials_before_submission_and_preserves_inner_guards()
-> TestResult {
    let state = Arc::new(ProviderState::default());
    let available = Arc::new(AtomicBool::new(true));
    let reader: vega_runtime::CredentialReader = {
        let available = available.clone();
        Arc::new(move || {
            if available.load(Ordering::Acquire) {
                Ok(vec![SECRET.into()])
            } else {
                Err(())
            }
        })
    };
    let factory = {
        let borrowed = IndependentProvider(state.clone());
        let child = ChildProvider {
            inner: ChildTransport::Borrowed(&borrowed),
            reader: Some(reader),
        };
        child.compaction_provider_factory().ok_or("owned factory")?
    };
    let owned = factory()?;
    assert_eq!(state.factories.load(Ordering::Acquire), 1);
    for mode in ["unavailable", "secret", "inner", "cancel", "success"] {
        available.store(mode != "unavailable", Ordering::Release);
        state.reject.store(mode == "inner", Ordering::Release);
        let submission = ProviderRequestSubmission::default();
        let cancel = CancellationToken::new();
        if mode == "cancel" {
            cancel.cancel();
        }
        let result = owned
            .chat_stream_with_submission(
                request(if mode == "secret" {
                    SECRET
                } else {
                    "Continue the task."
                }),
                cancel,
                submission.clone(),
            )
            .await;
        assert_eq!(
            submission.may_have_been_submitted(),
            mode == "success",
            "{mode}"
        );
        if mode == "success" {
            let mut response = result?;
            assert_eq!(response.metadata.http_status, Some(200));
            assert_eq!(response.metadata.retry_count, Some(2));
            assert_eq!(
                response.metadata.request_id.as_deref(),
                Some("request-safe")
            );
            assert_eq!(
                response.events.next().await.transpose()?,
                Some(ProviderEvent::TextDelta("Summary retained.".into()))
            );
            assert_eq!(
                response.events.next().await.transpose()?,
                Some(ProviderEvent::Done {
                    stop_reason: StopReason::End
                })
            );
            assert!(response.events.next().await.is_none());
        } else {
            assert!(result.is_err(), "{mode}");
            if matches!(mode, "unavailable" | "inner") {
                assert!(matches!(result, Err(VegaError::CredentialCheckUnavailable)));
            }
            assert_eq!(state.requests.load(Ordering::Acquire), 0, "{mode}");
        }
    }
    assert_eq!(state.requests.load(Ordering::Acquire), 1);
    Ok(())
}

#[tokio::test]
async fn owned_child_keeps_owner_output_filter_and_metadata_redaction() -> TestResult {
    let state = Arc::new(ProviderState::default());
    state.echo_secret.store(true, Ordering::Release);
    let reader: vega_runtime::CredentialReader = Arc::new(|| Ok(vec![SECRET.into()]));
    let owned = {
        let protected = crate::agent::credential_guard::OwnerCredentialProvider::new(
            Arc::new(IndependentProvider(state.clone())),
            reader.clone(),
        );
        let child = ChildProvider {
            inner: ChildTransport::Borrowed(&protected),
            reader: Some(reader),
        };
        child.compaction_provider_factory().ok_or("owned factory")?()?
    };
    let submission = ProviderRequestSubmission::default();
    let mut response = owned
        .chat_stream_with_submission(
            request("Continue."),
            CancellationToken::new(),
            submission.clone(),
        )
        .await?;
    assert!(submission.may_have_been_submitted());
    assert_eq!(state.factories.load(Ordering::Acquire), 1);
    assert_eq!(state.requests.load(Ordering::Acquire), 1);
    assert_eq!(response.metadata.http_status, Some(200));
    let request_id = response.metadata.request_id.ok_or("request metadata")?;
    assert!(!request_id.contains(SECRET));
    let mut text = String::new();
    let mut done = false;
    while let Some(event) = response.events.next().await {
        match event? {
            ProviderEvent::TextDelta(delta) => text.push_str(&delta),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            } => done = true,
            _ => {}
        }
    }
    assert!(!text.is_empty());
    assert!(!text.contains(SECRET));
    assert!(done);
    Ok(())
}

#[test]
fn child_without_an_owned_transport_does_not_advertise_background_capability() {
    struct BorrowedOnly;
    impl Provider for BorrowedOnly {
        fn chat_stream(
            &self,
            _: ChatRequest,
            _: CancellationToken,
        ) -> BoxFuture<'static, Result<vega_runtime::EventStream, VegaError>> {
            Box::pin(async { Err(VegaError::Cancelled) })
        }
    }
    let child = ChildProvider {
        inner: ChildTransport::Borrowed(&BorrowedOnly),
        reader: None,
    };
    assert!(child.compaction_provider_factory().is_none());
}
