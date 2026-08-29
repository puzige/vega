//! Mock provider: scripted replay of provider events (tech-spec §8, S4-T19).
//!
//! This is the shared test infrastructure for the S4-S8 agentic-loop tests:
//! callers hand in a script ([`ScriptStep`] sequence) and
//! [`MockProvider::chat_stream`] replays it step by step, honouring the
//! [`CancellationToken`] like a real provider would (fail fast when already
//! cancelled, stop mid-stream without further events otherwise).

use std::sync::Arc;

use futures::future::BoxFuture;
use tokio_util::sync::CancellationToken;

use crate::error::VegaError;
use crate::provider::{ChatRequest, EventStream, Provider, ProviderEvent};

/// One step of a mock script: a burst of events or a terminal error.
///
/// The error variants mirror the [`VegaError`] shapes a provider can emit
/// (provider failure / cancellation) while staying `Clone` — [`VegaError`]
/// itself wraps non-cloneable std errors and therefore cannot be stored in a
/// script directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptStep {
    /// Events replayed in order; the stream continues with the next step.
    Events(Vec<ProviderEvent>),
    /// Terminal provider failure: surfaces as a stream error, then ends the
    /// stream (mirrors `VegaError::Provider`).
    Error {
        /// HTTP status to report, if any.
        status: Option<u16>,
        /// Error text.
        message: String,
        /// Whether the failure reads as retryable.
        retryable: bool,
    },
    /// Terminal cancellation (mirrors `VegaError::Cancelled`).
    Cancelled,
}

impl ScriptStep {
    /// Convenience constructor for an events step.
    pub fn events(events: impl Into<Vec<ProviderEvent>>) -> Self {
        Self::Events(events.into())
    }

    /// Convenience constructor for a single text-delta step.
    pub fn text(delta: impl Into<String>) -> Self {
        Self::Events(vec![ProviderEvent::TextDelta(delta.into())])
    }
}

/// A [`Provider`] that replays a scripted sequence of [`ScriptStep`]s.
///
/// Every `chat_stream` call gets a fresh replay of the same script, so one
/// instance drives any number of loop iterations in tests.
#[derive(Debug, Clone)]
pub struct MockProvider {
    script: Arc<Vec<ScriptStep>>,
}

impl MockProvider {
    /// Builds a mock provider from a script; steps replay in order.
    pub fn new(script: Vec<ScriptStep>) -> Self {
        Self {
            script: Arc::new(script),
        }
    }
}

impl Provider for MockProvider {
    fn chat_stream(
        &self,
        _req: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        // 已取消的 token：不开流，直接快速失败（与真实 provider 语义一致）
        if cancel.is_cancelled() {
            return Box::pin(async { Err(VegaError::Cancelled) });
        }
        let script = Arc::clone(&self.script);
        Box::pin(async move {
            // unfold 状态：当前步 / 步内事件下标 / 脚本 / 取消令牌
            let stream = futures::stream::unfold(
                (0usize, 0usize, script, cancel),
                |(mut step, mut event, script, cancel)| async move {
                    loop {
                        // 每个事件发出前检查取消：立即断流且不再产生事件
                        if cancel.is_cancelled() {
                            return None;
                        }
                        match script.get(step) {
                            None => return None, // 脚本回放完毕
                            Some(ScriptStep::Events(events)) => match events.get(event) {
                                Some(ev) => {
                                    let item = Ok(ev.clone());
                                    event += 1;
                                    return Some((item, (step, event, script, cancel)));
                                }
                                None => {
                                    // 当前步耗尽，推进到下一步
                                    step += 1;
                                    event = 0;
                                }
                            },
                            Some(ScriptStep::Error {
                                status,
                                message,
                                retryable,
                            }) => {
                                let item = Err(VegaError::Provider {
                                    status: *status,
                                    message: message.clone(),
                                    retryable: *retryable,
                                });
                                step = script.len(); // 终态：报错后流结束
                                return Some((item, (step, event, script, cancel)));
                            }
                            Some(ScriptStep::Cancelled) => {
                                let item = Err(VegaError::Cancelled);
                                step = script.len(); // 终态
                                return Some((item, (step, event, script, cancel)));
                            }
                        }
                    }
                },
            );
            Ok(Box::pin(stream) as EventStream)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ChatMessage, ChatRole, StopReason};
    use futures::Stream;

    fn request() -> ChatRequest {
        ChatRequest {
            model: "mock-model".to_string(),
            messages: vec![ChatMessage::new(ChatRole::User, "hi")],
            ..Default::default()
        }
    }

    /// Drains up to `limit` items from an already-pinned boxed stream.
    async fn collect(
        stream: &mut (impl Stream<Item = Result<ProviderEvent, VegaError>> + Unpin),
        limit: usize,
    ) -> Vec<Result<ProviderEvent, VegaError>> {
        use futures::StreamExt;
        let mut out = Vec::new();
        for _ in 0..limit {
            match stream.next().await {
                Some(item) => out.push(item),
                None => break,
            }
        }
        out
    }

    /// Element-wise comparison: `Ok` items via `ProviderEvent: PartialEq`,
    /// `Err` items via `VegaError`'s `Display` (it wraps non-`PartialEq`
    /// std errors, so direct equality is unavailable).
    fn assert_items_eq(
        actual: &[Result<ProviderEvent, VegaError>],
        expected: &[Result<ProviderEvent, VegaError>],
    ) {
        assert_eq!(
            actual.len(),
            expected.len(),
            "item count mismatch: {actual:?} vs {expected:?}"
        );
        for (i, (a, e)) in actual.iter().zip(expected.iter()).enumerate() {
            match (a, e) {
                (Ok(a), Ok(e)) => assert_eq!(a, e, "item #{i}"),
                (Err(a), Err(e)) => assert_eq!(a.to_string(), e.to_string(), "item #{i}"),
                _ => panic!("item #{i} mismatch: {a:?} vs {e:?}"),
            }
        }
    }

    #[tokio::test]
    async fn replays_plain_text_deltas_then_usage_and_done() {
        let provider = MockProvider::new(vec![
            ScriptStep::text("Hel"),
            ScriptStep::text("lo "),
            ScriptStep::text("world"),
            ScriptStep::events(vec![
                ProviderEvent::Usage {
                    input: 12,
                    output: 3,
                    cache_read: 4,
                    cache_write: 0,
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::End,
                },
            ]),
        ]);
        let mut stream = provider
            .chat_stream(request(), CancellationToken::new())
            .await
            .unwrap();
        let items = collect(&mut stream, 8).await;
        assert_items_eq(
            &items,
            &[
                Ok(ProviderEvent::TextDelta("Hel".into())),
                Ok(ProviderEvent::TextDelta("lo ".into())),
                Ok(ProviderEvent::TextDelta("world".into())),
                Ok(ProviderEvent::Usage {
                    input: 12,
                    output: 3,
                    cache_read: 4,
                    cache_write: 0,
                }),
                Ok(ProviderEvent::Done {
                    stop_reason: StopReason::End,
                }),
            ],
        );
    }

    #[tokio::test]
    async fn replays_tool_call_script() {
        let provider = MockProvider::new(vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "call_1".into(),
                name: "grep".into(),
                input_json: r#"{"pattern":"TODO"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])]);
        let mut stream = provider
            .chat_stream(request(), CancellationToken::new())
            .await
            .unwrap();
        let items = collect(&mut stream, 4).await;
        assert_items_eq(
            &items,
            &[
                Ok(ProviderEvent::ToolUse {
                    id: "call_1".into(),
                    name: "grep".into(),
                    input_json: r#"{"pattern":"TODO"}"#.into(),
                }),
                Ok(ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                }),
            ],
        );
    }

    #[tokio::test]
    async fn error_step_is_terminal_and_truncates_the_script() {
        let provider = MockProvider::new(vec![
            ScriptStep::text("partial"),
            ScriptStep::Error {
                status: Some(503),
                message: "overloaded".into(),
                retryable: true,
            },
            ScriptStep::text("never replayed"),
        ]);
        let mut stream = provider
            .chat_stream(request(), CancellationToken::new())
            .await
            .unwrap();
        let items = collect(&mut stream, 8).await;
        assert_eq!(items.len(), 2, "stream must end right after the error");
        assert_items_eq(
            &items,
            &[
                Ok(ProviderEvent::TextDelta("partial".into())),
                Err(VegaError::Provider {
                    status: Some(503),
                    message: "overloaded".into(),
                    retryable: true,
                }),
            ],
        );
    }

    #[tokio::test]
    async fn cancelled_step_maps_to_vega_error_cancelled() {
        let provider = MockProvider::new(vec![ScriptStep::Cancelled]);
        let mut stream = provider
            .chat_stream(request(), CancellationToken::new())
            .await
            .unwrap();
        let items = collect(&mut stream, 4).await;
        assert_eq!(items.len(), 1);
        assert!(matches!(items[0], Err(VegaError::Cancelled)));
    }

    #[tokio::test]
    async fn already_cancelled_token_fails_fast_without_a_stream() {
        let provider = MockProvider::new(vec![ScriptStep::text("never")]);
        let cancel = CancellationToken::new();
        cancel.cancel();
        let result = provider.chat_stream(request(), cancel).await;
        assert!(matches!(result, Err(VegaError::Cancelled)));
    }

    #[tokio::test]
    async fn mid_stream_cancel_stops_replay_without_further_events() {
        let provider = MockProvider::new(vec![
            ScriptStep::text("first"),
            ScriptStep::text("second"),
            ScriptStep::text("third"),
        ]);
        let cancel = CancellationToken::new();
        let mut stream = provider
            .chat_stream(request(), cancel.clone())
            .await
            .unwrap();
        let first = collect(&mut stream, 1).await;
        assert_items_eq(&first, &[Ok(ProviderEvent::TextDelta("first".into()))]);
        // 取消后不再产生任何事件（立即断流）
        cancel.cancel();
        let rest = collect(&mut stream, 8).await;
        assert!(
            rest.is_empty(),
            "no events after cancellation, got {rest:?}"
        );
    }

    #[tokio::test]
    async fn replays_freshly_on_every_call() {
        let provider = MockProvider::new(vec![ScriptStep::text("again")]);
        for _ in 0..2 {
            let mut stream = provider
                .chat_stream(request(), CancellationToken::new())
                .await
                .unwrap();
            let items = collect(&mut stream, 2).await;
            assert_items_eq(&items, &[Ok(ProviderEvent::TextDelta("again".into()))]);
        }
    }
}
