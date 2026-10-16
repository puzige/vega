use super::*;
use crate::EventStream;

fn overflow() -> ScriptStep {
    ScriptStep::Error {
        status: Some(400),
        message:
            r#"responses request failed (HTTP 400): {"error":{"code":"context_length_exceeded"}}"#
                .into(),
        retryable: false,
    }
}

struct OpenOverflowProvider {
    opens: Arc<AtomicUsize>,
    inner: MockProvider,
}

impl Provider for OpenOverflowProvider {
    fn chat_stream(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        if self.opens.fetch_add(1, Ordering::SeqCst) == 0 {
            return Box::pin(async {
                Err(VegaError::Provider {
                    status: Some(413),
                    message: r#"{"error":{"code":"context_length_exceeded"}}"#.into(),
                    retryable: false,
                })
            });
        }
        self.inner.chat_stream(request, cancel)
    }
}
