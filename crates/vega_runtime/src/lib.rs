//! Headless agent core: provider abstraction and mock replay (tech-spec
//! §4.1 §7 §8, A3-01 / S4-T19).
//!
//! The crate is UI-free (no gpui, headless red line): it only knows how to
//! talk to LLM providers and how to replay scripted event streams. The
//! agentic loop, tool execution, and permission gating arrive with S4-T20 /
//! S4-T21 on top of the [`Provider`] boundary defined here.
//!
//! - [`provider`]: [`ChatRequest`] / [`ProviderEvent`] / the [`Provider`]
//!   trait — trait methods hand-box their future into a `BoxFuture` (the
//!   exact shape `async-trait` would expand to), keeping the crate free of
//!   proc-macro dependencies while staying `dyn`-compatible (architect's
//!   pre-ruling, S4-T19).
//! - [`mock`]: scripted replay provider ([`MockProvider`]) — the shared
//!   test infrastructure for the S4-S8 agentic-loop tests (tech-spec §8).
//! - [`retry`]: the [`RetryPolicy`] schedule (1s / 2s / 4s, 3 retries).
//! - [`error`]: the unified [`VegaError`] (tech-spec §7, `Send + Sync`).

mod error;
mod mock;
mod provider;
mod retry;

pub use error::VegaError;
pub use mock::{MockProvider, ScriptStep};
pub use provider::{
    ChatMessage, ChatRequest, ChatRole, EventStream, Provider, ProviderEvent, StopReason,
    ToolDefinition,
};
pub use retry::RetryPolicy;
