//! Headless agent core: provider abstraction, OpenAI-compatible streaming,
//! and mock replay (tech-spec §4.1 §7 §8, A3-01/A3-02 / S4-T19).
//!
//! The crate is UI-free (no gpui, headless red line): it only knows how to
//! talk to LLM providers and how to replay scripted event streams. The
//! agentic loop executes the fenced S4 read-only tools on top of the
//! [`Provider`] boundary defined here.
//!
//! - [`provider`]: [`ChatRequest`] / [`ProviderEvent`] / the [`Provider`]
//!   trait — trait methods hand-box their future into a `BoxFuture` (the
//!   exact shape `async-trait` would expand to), keeping the crate free of
//!   proc-macro dependencies while staying `dyn`-compatible (architect's
//!   pre-ruling, S4-T19).
//! - [`openai`]: OpenAI-compatible SSE implementation — `stream: true` +
//!   `stream_options.include_usage`, `delta.tool_calls` aggregation by
//!   index, retry with exponential backoff / `Retry-After`, and
//!   cancellation. The API key only ever enters the Authorization header.
//! - [`mock`]: scripted replay provider ([`MockProvider`]) — the shared
//!   test infrastructure for the S4-S8 agentic-loop tests (tech-spec §8).
//! - [`agent`]: system-plus-history context assembly, serialized tool loop,
//!   cancellation, call-id deduplication, limits, and runtime-local events.
//! - [`retry`]: the [`RetryPolicy`] schedule (1s / 2s / 4s, 3 retries).
//! - [`error`]: the unified [`VegaError`] (tech-spec §7, `Send + Sync`).
//!
//! # Example
//!
//! ```
//! use futures::StreamExt;
//! use tokio_util::sync::CancellationToken;
//! use vega_runtime::{
//!     ChatMessage, ChatRequest, ChatRole, MockProvider, Provider, ProviderEvent,
//!     ScriptStep, StopReason,
//! };
//!
//! let provider = MockProvider::new(vec![
//!     ScriptStep::text("Hel"),
//!     ScriptStep::events(vec![
//!         ProviderEvent::Usage { input: 10, output: 2, cache_read: 0, cache_write: 0 },
//!         ProviderEvent::Done { stop_reason: StopReason::End },
//!     ]),
//! ]);
//!
//! let req = ChatRequest {
//!     model: "mock-model".into(),
//!     messages: vec![ChatMessage::new(ChatRole::User, "hi")],
//!     ..Default::default()
//! };
//! let stream = futures::executor::block_on(provider.chat_stream(req, CancellationToken::new()));
//! assert!(stream.is_ok());
//! let events: Vec<_> = match stream {
//!     Ok(stream) => futures::executor::block_on(stream.collect()),
//!     Err(_) => Vec::new(),
//! };
//! assert!(
//!     matches!(&events[0], Ok(ProviderEvent::TextDelta(text)) if text == "Hel")
//! );
//! assert!(matches!(
//!     &events[2],
//!     Ok(ProviderEvent::Done { stop_reason: StopReason::End })
//! ));
//! ```

mod agent;
mod error;
mod mock;
mod openai;
mod provider;
mod retry;

pub use agent::{
    AgentOutcome, AgentRequest, CompletedToolCall, RuntimeEvent, RuntimeFinishReason,
    RuntimeTokenUsage, RuntimeToolCall, RuntimeToolResult, RuntimeToolStatus, TOOL_CALL_LIMIT,
    run_agent, run_agent_with_sink,
};
pub use error::VegaError;
pub use mock::{MockProvider, ScriptStep};
pub use openai::OpenAiProvider;
pub use provider::{
    ChatMessage, ChatRequest, ChatRole, ChatToolCall, EventStream, Provider, ProviderEvent,
    StopReason, ToolDefinition,
};
pub use retry::RetryPolicy;
