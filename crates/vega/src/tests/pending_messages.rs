use super::*;
use vega_runtime::{ChatRole, MockProvider, Provider, ProviderEvent, ScriptStep, StopReason};
use vega_ui::conversation_stream::{PendingQueueAction, PendingQueueRequested};

struct FirstRequestGate {
    provider: Arc<MockProvider>,
    release: Arc<tokio::sync::Notify>,
    first: AtomicBool,
}

impl Provider for FirstRequestGate {
    fn chat_stream(
        &self,
        request: vega_runtime::ChatRequest,
        cancel: tokio_util::sync::CancellationToken,
    ) -> futures::future::BoxFuture<
        'static,
        Result<vega_runtime::EventStream, vega_runtime::VegaError>,
    > {
        let future = self.provider.chat_stream(request, cancel.clone());
        let wait = self.first.swap(false, Ordering::SeqCst);
        let release = self.release.clone();
        Box::pin(async move {
            if wait {
                tokio::select! {
                    _ = cancel.cancelled() => return Err(vega_runtime::VegaError::Cancelled),
                    _ = release.notified() => {}
                }
            }
            future.await
        })
    }
}

fn done(text: &str) -> Vec<ScriptStep> {
    vec![
        ScriptStep::text(text),
        ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }]),
    ]
}
