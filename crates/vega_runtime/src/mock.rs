use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

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
#[derive(Clone, PartialEq, Eq)]
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
    /// Cancel-aware pause before the next step (test-only replay timing).
    Delay(Duration),
}

impl std::fmt::Debug for ScriptStep {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Events(events) => formatter
                .debug_struct("Events")
                .field("event_count", &events.len())
                .finish(),
            Self::Error {
                status,
                message,
                retryable,
            } => formatter
                .debug_struct("Error")
                .field("status", status)
                .field("message_bytes", &message.len())
                .field("retryable", retryable)
                .finish(),
            Self::Cancelled => formatter.write_str("Cancelled"),
            Self::Delay(duration) => formatter.debug_tuple("Delay").field(duration).finish(),
        }
    }
}

impl ScriptStep {
    /// Convenience constructor for an events step.
    pub fn events(events: impl Into<Vec<ProviderEvent>>) -> Self {
        Self::Events(events.into())
    }

    /// Convenience constructor for a cancel-aware replay delay.
    pub fn delay(duration: Duration) -> Self {
        Self::Delay(duration)
    }
}

/// A [`Provider`] that replays a scripted sequence of [`ScriptStep`]s.
///
/// Every `chat_stream` call gets a fresh replay of the same script, so one
/// instance drives any number of loop iterations in tests.
#[derive(Clone)]
pub struct MockProvider {
    scripts: Arc<Vec<Vec<ScriptStep>>>,
    repeat_first: bool,
    call_index: Arc<AtomicUsize>,
    requests: Arc<Mutex<Vec<ChatRequest>>>,
}

impl std::fmt::Debug for MockProvider {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MockProvider")
            .field("script_count", &self.scripts.len())
            .field("repeat_first", &self.repeat_first)
            .field("call_count", &self.call_index.load(Ordering::SeqCst))
            .field(
                "request_count",
                &self.requests.lock().map_or(0, |requests| requests.len()),
            )
            .finish()
    }
}

impl MockProvider {
    /// Builds a mock provider from a script; steps replay in order.
    pub fn new(script: Vec<ScriptStep>) -> Self {
        Self {
            scripts: Arc::new(vec![script]),
            repeat_first: true,
            call_index: Arc::new(AtomicUsize::new(0)),
            requests: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Builds a mock whose successive calls replay successive scripts.
    /// Calls after the final script replay an empty stream.
    pub fn new_rounds(scripts: Vec<Vec<ScriptStep>>) -> Self {
        Self {
            scripts: Arc::new(scripts),
            repeat_first: false,
            call_index: Arc::new(AtomicUsize::new(0)),
            requests: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Returns all chat requests observed so far, in call order.
    pub fn requests(&self) -> Vec<ChatRequest> {
        self.requests
            .lock()
            .map_or_else(|_| Vec::new(), |requests| requests.clone())
    }
}

impl Provider for MockProvider {
    fn chat_stream(
        &self,
        req: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        // 已取消的 token：不开流，直接快速失败（与真实 provider 语义一致）
        if cancel.is_cancelled() {
            return Box::pin(async { Err(VegaError::Cancelled) });
        }
        if let Ok(mut requests) = self.requests.lock() {
            requests.push(req);
        }
        let index = self.call_index.fetch_add(1, Ordering::SeqCst);
        let script = if self.repeat_first {
            self.scripts.first().cloned().unwrap_or_default()
        } else {
            self.scripts.get(index).cloned().unwrap_or_default()
        };
        let script = Arc::new(script);
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
                            Some(ScriptStep::Delay(duration)) => {
                                let completed = tokio::select! {
                                    biased;
                                    _ = cancel.cancelled() => false,
                                    _ = tokio::time::sleep(*duration) => true,
                                };
                                if !completed {
                                    return None;
                                }
                                step += 1;
                                event = 0;
                            }
                        }
                    }
                },
            );
            Ok(Box::pin(stream) as EventStream)
        })
    }
}
