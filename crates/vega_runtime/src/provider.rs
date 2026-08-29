//! Provider abstraction (tech-spec §4.1, A3-01 / S4-T19).
//!
//! The [`Provider`] trait is the seam between the agentic loop (S4-T20) and
//! concrete LLM backends: the OpenAI-compatible implementation ([`crate::
//! openai::OpenAiProvider`]) and the scripted [`crate::mock::MockProvider`]
//! are interchangeable behind `dyn Provider`.
//!
//! Architect's pre-ruling (S4-T19): the trait method returns a manually
//! boxed `BoxFuture` — exactly the shape the `async_trait` macro would
//! expand to — so the crate gains dyn-compatibility with zero proc-macro
//! dependencies.

use std::pin::Pin;

use futures::Stream;
use futures::future::BoxFuture;
use tokio_util::sync::CancellationToken;

use crate::error::VegaError;

/// A stream of provider events, boxed for dyn-compatibility: mock and real
/// providers hand out the same type (tech-spec §4.1 `EventStream`).
pub type EventStream = Pin<Box<dyn Stream<Item = Result<ProviderEvent, VegaError>> + Send>>;

/// Chat role of a message, using the OpenAI-compatible wire vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatRole {
    /// System prompt.
    System,
    /// User turn.
    User,
    /// Assistant turn.
    Assistant,
    /// Tool result turn (OpenAI-compatible `tool` role).
    Tool,
}

impl ChatRole {
    /// Wire name used by OpenAI-compatible chat APIs.
    pub fn as_str(self) -> &'static str {
        match self {
            ChatRole::System => "system",
            ChatRole::User => "user",
            ChatRole::Assistant => "assistant",
            ChatRole::Tool => "tool",
        }
    }
}

/// One chat message (`role` + `content`, tech-spec §4.1 ChatRequest).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    /// Sender role.
    pub role: ChatRole,
    /// Plain-text content (markdown lives here verbatim).
    pub content: String,
    /// Provider tool-call id for a `tool` result message.
    pub tool_call_id: Option<String>,
    /// Calls requested by an assistant message before tool results follow.
    pub tool_calls: Vec<ChatToolCall>,
}

impl ChatMessage {
    /// Builds a message from a role and any string-like content.
    pub fn new(role: ChatRole, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            tool_call_id: None,
            tool_calls: Vec::new(),
        }
    }

    /// Builds the assistant turn that requested `tool_calls`.
    pub fn assistant_with_tools(content: impl Into<String>, tool_calls: Vec<ChatToolCall>) -> Self {
        Self {
            role: ChatRole::Assistant,
            content: content.into(),
            tool_call_id: None,
            tool_calls,
        }
    }

    /// Builds a tool-result message associated with `call_id`.
    pub fn tool_result(call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::Tool,
            content: content.into(),
            tool_call_id: Some(call_id.into()),
            tool_calls: Vec::new(),
        }
    }
}

/// One complete assistant function call serialized back to the provider on
/// the observe round.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatToolCall {
    /// Provider-side call id.
    pub id: String,
    /// Function/tool name.
    pub name: String,
    /// Complete raw JSON arguments.
    pub input_json: String,
}

/// A callable tool advertised to the model (OpenAI function-calling shape).
#[derive(Debug, Clone, PartialEq)]
pub struct ToolDefinition {
    /// Tool name the model refers to, e.g. `read`.
    pub name: String,
    /// Human/model-readable description of when to use the tool.
    pub description: String,
    /// JSON Schema object describing the tool input.
    pub input_schema: serde_json::Value,
}

/// A chat completion request (tech-spec §4.1):
/// model / messages / tools / max_tokens.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChatRequest {
    /// Model name as understood by the provider, e.g. `deepseek-chat`.
    pub model: String,
    /// Conversation so far (system prompt + history window; T20 assembles).
    pub messages: Vec<ChatMessage>,
    /// Tools advertised to the model; empty when none.
    pub tools: Vec<ToolDefinition>,
    /// Generation cap in tokens; `None` lets the provider default apply.
    pub max_tokens: Option<u32>,
}

/// Why the provider stopped generating (tech-spec §4.1 minimal set).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// Natural end of the assistant turn.
    End,
    /// The model requested tool calls.
    ToolUse,
    /// Token budget exhausted.
    Length,
}

/// Incremental provider event (tech-spec §4.1). [`ProviderEvent::ToolUse`]
/// is emitted only after the model's argument fragments are fully aggregated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderEvent {
    /// Incremental visible text.
    TextDelta(String),
    /// Incremental reasoning text (OpenAI-compatible `reasoning_content`).
    ThinkingDelta(String),
    /// A complete tool call: id / name / raw JSON input string.
    ToolUse {
        /// Provider-side tool call id (aligns with `tool_calls.id`, §2).
        id: String,
        /// Tool name.
        name: String,
        /// Aggregated JSON arguments (still a raw string at this layer).
        input_json: String,
    },
    /// Token accounting recovered from the final usage chunk.
    Usage {
        /// Prompt tokens.
        input: u64,
        /// Completion tokens.
        output: u64,
        /// Prompt tokens served from cache.
        cache_read: u64,
        /// Prompt tokens written to cache (when the API reports it).
        cache_write: u64,
    },
    /// Terminal event carrying the mapped stop reason.
    Done {
        /// Merged from the wire `finish_reason`.
        stop_reason: StopReason,
    },
}

/// LLM provider abstraction (tech-spec §4.1).
///
/// `chat_stream` returns a `BoxFuture` resolving to an [`EventStream`] so
/// that connection setup, retries, and cancellation all happen inside the
/// future while the loop only polls events afterwards.
pub trait Provider: Send + Sync {
    /// Starts a streaming chat completion for `req`; `cancel` aborts the
    /// attempt (and later the stream) as soon as it fires.
    fn chat_stream(
        &self,
        req: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile-time proof of the architect's pre-ruling: the trait is
    /// dyn-compatible and its objects are `Send + Sync`.
    #[test]
    fn provider_is_object_safe_and_send_sync() {
        fn assert_object<T: Send + Sync + ?Sized>() {}
        assert_object::<dyn Provider>();
    }

    #[test]
    fn chat_role_wire_names_match_openai_vocabulary() {
        assert_eq!(ChatRole::System.as_str(), "system");
        assert_eq!(ChatRole::User.as_str(), "user");
        assert_eq!(ChatRole::Assistant.as_str(), "assistant");
        assert_eq!(ChatRole::Tool.as_str(), "tool");
    }

    #[test]
    fn chat_request_default_is_empty() {
        let req = ChatRequest::default();
        assert!(req.model.is_empty());
        assert!(req.messages.is_empty());
        assert!(req.tools.is_empty());
        assert_eq!(req.max_tokens, None);
    }
}
