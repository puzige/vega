use super::*;

use futures::future::BoxFuture;
use std::sync::Mutex;
use vega_runtime::{
    EventStream, MockProvider, ProviderResponseMetadata, ProviderStream, ScriptStep,
};

struct TimedSummaryProvider {
    inner: MockProvider,
    acquisition_delay: Duration,
    tokens: Mutex<Vec<CancellationToken>>,
}

impl TimedSummaryProvider {
    fn new(scripts: Vec<Vec<ScriptStep>>, acquisition_seconds: u64) -> Self {
        Self {
            inner: MockProvider::new_rounds(scripts),
            acquisition_delay: Duration::from_secs(acquisition_seconds),
            tokens: Mutex::new(Vec::new()),
        }
    }
}

impl Provider for TimedSummaryProvider {
    fn chat_stream(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        self.tokens.lock().unwrap().push(cancel.clone());
        let inner = self.inner.clone();
        let delay = self.acquisition_delay;
        Box::pin(async move {
            tokio::time::sleep(delay).await;
            inner.chat_stream(request, cancel).await
        })
    }

    fn chat_stream_with_metadata(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<ProviderStream, VegaError>> {
        let stream = self.chat_stream(request, cancel);
        Box::pin(async move {
            Ok(ProviderStream {
                events: stream.await?,
                metadata: ProviderResponseMetadata {
                    http_status: Some(200),
                    request_id: None,
                    retry_count: Some(0),
                },
            })
        })
    }
}

fn usage(output: u64) -> ScriptStep {
    ScriptStep::events(vec![ProviderEvent::Usage {
        input: 42,
        output,
        cache_read: Some(0),
        cache_write: Some(0),
    }])
}

fn done() -> ScriptStep {
    ScriptStep::events(vec![ProviderEvent::Done {
        stop_reason: StopReason::End,
    }])
}

fn delay(seconds: u64) -> ScriptStep {
    ScriptStep::delay(Duration::from_secs(seconds))
}

fn fixture() -> (Store, tempfile::TempDir) {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(directory.path().join("vega.db")).unwrap();
    store.migrate().unwrap();
    vega_store::threads::create_standalone(
        store.conn(),
        vega_store::threads::NewThread {
            id: "timeout-thread",
            project_id: "",
            title: "fixture",
            mode: "execute",
            permission_mode: "readonly",
            model: "mock-model",
            status: "active",
            pinned: false,
            unread: false,
            created_at: 1,
            updated_at: 1,
        },
    )
    .unwrap();
    for (index, role, content) in [
        (1, "user", "old request"),
        (2, "assistant", "old answer"),
        (3, "user", "later request"),
        (4, "assistant", "later answer"),
        (5, "user", "continue"),
    ] {
        vega_store::messages::insert(
            store.conn(),
            &vega_store::messages::MessageRow {
                id: format!("message-{index}"),
                thread_id: "timeout-thread".into(),
                seq: index,
                role: role.into(),
                kind: String::new(),
                content: content.into(),
                status: "done".into(),
                created_at: index,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .unwrap();
    }
    save_settings(
        store.conn(),
        &StoreContextSettings {
            thread_id: "timeout-thread".into(),
            model: "mock-model".into(),
            context_limit: Some(50_000),
            output_reserve: 1_000,
            automatic_compaction: false,
            updated_at: 1,
        },
    )
    .unwrap();
    (store, directory)
}
