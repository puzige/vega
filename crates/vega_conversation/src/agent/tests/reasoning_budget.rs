use super::*;
use vega_runtime::{ChatRequest, EventStream, REASONING_DELTA_MAX_BYTES, ReasoningBudgetScope};

struct LongThinkingTask {
    primary_calls: Arc<AtomicUsize>,
    summary_calls: Arc<AtomicUsize>,
    requests: Arc<Mutex<Vec<ChatRequest>>>,
}

impl Provider for LongThinkingTask {
    fn chat_stream(
        &self,
        request: ChatRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        self.requests.lock().unwrap().push(request.clone());
        let events = if request.tools.is_empty() {
            self.summary_calls.fetch_add(1, Ordering::SeqCst);
            vec![ProviderEvent::TextDelta("Task goal: complete the task. Progress: completed prior read rounds. Latest correction: preserve paired results.".into()), ProviderEvent::Done { stop_reason: StopReason::End }]
        } else {
            let index = self.primary_calls.fetch_add(1, Ordering::SeqCst);
            if index == 10 {
                vec![
                    ProviderEvent::TextDelta("task finished".into()),
                    ProviderEvent::Done {
                        stop_reason: StopReason::End,
                    },
                ]
            } else {
                let mut events = (0..4)
                    .map(|_| ProviderEvent::ThinkingDelta("r".repeat(REASONING_DELTA_MAX_BYTES)))
                    .collect::<Vec<_>>();
                events.extend([
                    ProviderEvent::TextDelta("progress ".repeat(4_000)),
                    ProviderEvent::ToolUse {
                        id: format!("read-372-{index}"),
                        name: "read".into(),
                        input_json: r#"{"path":"lib.rs"}"#.into(),
                    },
                    ProviderEvent::Done {
                        stop_reason: StopReason::ToolUse,
                    },
                ]);
                events
            }
        };
        Box::pin(async move {
            Ok(Box::pin(futures::stream::iter(events.into_iter().map(Ok))) as EventStream)
        })
    }
}
