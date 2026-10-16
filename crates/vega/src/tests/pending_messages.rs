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

fn gate(
    f: &Fixture,
    provider: Arc<MockProvider>,
    cx: &mut gpui_kit::TestAppContext,
) -> Arc<tokio::sync::Notify> {
    let release = Arc::new(tokio::sync::Notify::new());
    f.root.update(cx, |root, _| {
        root.agent_provider_override =
            Some(with_auxiliary_title_fixture(Arc::new(FirstRequestGate {
                provider,
                release: release.clone(),
                first: AtomicBool::new(true),
            })));
    });
    release
}

fn send(f: &Fixture, text: &str, cx: &mut gpui_kit::TestAppContext) {
    edit(f, text, cx);
    cx.simulate_keystrokes(f.window.into(), "cmd-enter");
}

fn queued(f: &Fixture, id: &str, behavior: PendingMessageBehavior) -> PendingMessage {
    PendingMessage {
        id: id.into(),
        thread_id: f.thread.id.clone(),
        generation: "prior-process".into(),
        behavior,
        content: id.into(),
        images: Vec::new(),
    }
}

fn queue_action(f: &Fixture, action: PendingQueueAction, cx: &mut gpui_kit::TestAppContext) {
    f.root.update(cx, |root, cx| {
        root.pending_queue_requested(
            f.stream.clone(),
            &PendingQueueRequested {
                thread_id: f.thread.id.clone(),
                action,
            },
            cx,
        )
    });
}
