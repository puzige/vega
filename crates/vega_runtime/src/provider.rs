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

/// Protocol family used to encode a frozen thinking selection.
///
/// The runtime never infers this value from a provider URL or model name. The
/// app/store boundary supplies an explicit declaration; `Unknown` therefore
/// has the safe provider-default wire behavior only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReasoningProtocol {
    /// OpenAI-style Chat Completions using `reasoning_effort`.
    OpenAiChatCompletions,
    /// Zhipu/GLM Chat Completions using `thinking` and `reasoning_effort`.
    ZhipuChatCompletions,
    /// No declared protocol or an endpoint whose capability is unknown.
    #[default]
    Unknown,
}

/// Explicit wire operation used to disable thinking. `none` is only legal
/// when a profile declares this exact operation; it is never a universal
/// OpenAI compatibility assumption.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReasoningDisabledWire {
    /// `thinking: { type: "disabled" }`.
    ThinkingTypeDisabled,
    /// `reasoning_effort: "none"`.
    ReasoningEffortNone,
}

/// User selection frozen for one run and carried into every logical provider
/// request. The strings are identifiers/declared effort values, never raw
/// reasoning content.
#[derive(Clone, PartialEq, Eq)]
pub struct FrozenReasoning {
    /// Exact configured provider identifier.
    pub provider: String,
    /// Exact configured model identifier.
    pub model: String,
    /// Explicit protocol capability declaration.
    pub protocol: ReasoningProtocol,
    /// User choice: provider default, a true disabled operation, or a declared
    /// effort value.
    pub choice: ReasoningChoice,
    /// Whether the profile explicitly permits the disabled operation.
    pub supports_disabled: bool,
    /// Whether the provider requires original reasoning content to be kept for
    /// the next tool round.
    pub preserve_reasoning_content: bool,
    /// Exact disabled wire operation, when one is declared.
    pub disabled_wire: Option<ReasoningDisabledWire>,
    /// Declared effort values accepted by this exact provider/model profile.
    pub declared_efforts: Vec<String>,
}

impl std::fmt::Debug for FrozenReasoning {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FrozenReasoning")
            .field("provider_bytes", &self.provider.len())
            .field("model_bytes", &self.model.len())
            .field("protocol", &self.protocol)
            .field("choice", &self.choice)
            .field("supports_disabled", &self.supports_disabled)
            .field(
                "preserve_reasoning_content",
                &self.preserve_reasoning_content,
            )
            .field("disabled_wire", &self.disabled_wire)
            .field("declared_effort_count", &self.declared_efforts.len())
            .finish()
    }
}

impl FrozenReasoning {
    /// Safe profile for an unrecognised provider/model pair. The wire encoder
    /// omits all thinking controls for this profile.
    pub fn unknown(provider: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            model: model.into(),
            protocol: ReasoningProtocol::Unknown,
            choice: ReasoningChoice::ProviderDefault,
            supports_disabled: false,
            preserve_reasoning_content: false,
            disabled_wire: None,
            declared_efforts: Vec::new(),
        }
    }

    /// Validates that the frozen choice is permitted by its explicit profile.
    /// Invalid choices fail before a provider request is attempted.
    pub fn validate(&self) -> Result<(), VegaError> {
        if self.provider.trim().is_empty() || self.model.trim().is_empty() {
            return Err(VegaError::ReasoningSelectionInvalid {
                message: "reasoning profile provider/model is empty".to_string(),
            });
        }
        for effort in &self.declared_efforts {
            if !matches!(
                effort.as_str(),
                "minimal" | "low" | "medium" | "high" | "xhigh" | "max"
            ) {
                return Err(VegaError::ReasoningSelectionInvalid {
                    message: "reasoning profile declares an unsupported effort".to_string(),
                });
            }
        }
        if self
            .declared_efforts
            .iter()
            .enumerate()
            .any(|(index, effort)| self.declared_efforts[..index].contains(effort))
        {
            return Err(VegaError::ReasoningSelectionInvalid {
                message: "reasoning profile declares duplicate efforts".to_string(),
            });
        }
        if matches!(self.protocol, ReasoningProtocol::Unknown)
            && (self.preserve_reasoning_content
                || self.supports_disabled
                || self.disabled_wire.is_some()
                || !self.declared_efforts.is_empty())
        {
            return Err(VegaError::ReasoningSelectionInvalid {
                message:
                    "unknown reasoning protocol cannot declare controls or replay reasoning content"
                        .to_string(),
            });
        }
        if self.supports_disabled
            && !matches!(
                (self.protocol, self.disabled_wire),
                (
                    ReasoningProtocol::OpenAiChatCompletions,
                    Some(ReasoningDisabledWire::ReasoningEffortNone)
                ) | (
                    ReasoningProtocol::ZhipuChatCompletions,
                    Some(ReasoningDisabledWire::ThinkingTypeDisabled)
                )
            )
        {
            return Err(VegaError::ReasoningSelectionInvalid {
                message: "disabled wire does not match the declared protocol".to_string(),
            });
        }
        if !self.supports_disabled && self.disabled_wire.is_some() {
            return Err(VegaError::ReasoningSelectionInvalid {
                message: "disabled wire requires an enabled disabled capability".to_string(),
            });
        }
        if matches!(self.protocol, ReasoningProtocol::ZhipuChatCompletions)
            && matches!(self.model.as_str(), "glm-5.3" | "glm-5.3-flash")
            && (self
                .declared_efforts
                .iter()
                .any(|effort| !matches!(effort.as_str(), "low" | "high" | "max"))
                || self.supports_disabled
                || self.disabled_wire.is_some()
                || matches!(&self.choice, ReasoningChoice::Disabled))
        {
            return Err(VegaError::ReasoningSelectionInvalid {
                message:
                    "standard GLM 5.3 profiles support only low/high/max and cannot be disabled"
                        .to_string(),
            });
        }
        match &self.choice {
            ReasoningChoice::ProviderDefault => Ok(()),
            ReasoningChoice::Effort(effort)
                if !effort.trim().is_empty()
                    && self
                        .declared_efforts
                        .iter()
                        .any(|declared| declared == effort)
                    && !matches!(self.protocol, ReasoningProtocol::Unknown) =>
            {
                Ok(())
            }
            ReasoningChoice::Disabled
                if self.supports_disabled
                    && !matches!(self.protocol, ReasoningProtocol::Unknown)
                    && matches!(
                        (self.protocol, self.disabled_wire),
                        (
                            ReasoningProtocol::OpenAiChatCompletions,
                            Some(ReasoningDisabledWire::ReasoningEffortNone)
                        ) | (
                            ReasoningProtocol::ZhipuChatCompletions,
                            Some(ReasoningDisabledWire::ThinkingTypeDisabled)
                        )
                    ) =>
            {
                Ok(())
            }
            ReasoningChoice::Effort(_) => Err(VegaError::ReasoningSelectionInvalid {
                message: "effort is not declared by the provider/model profile".to_string(),
            }),
            ReasoningChoice::Disabled => Err(VegaError::ReasoningSelectionInvalid {
                message: "disabled is not declared by the provider/model profile".to_string(),
            }),
        }
    }
}

/// The three user-visible classes of thinking request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReasoningChoice {
    /// Omit all thinking controls and let the provider decide.
    ProviderDefault,
    /// Send the profile's explicit disabled operation.
    Disabled,
    /// Send one value from the profile's declared effort subset.
    Effort(String),
}

/// Scope of a bounded reasoning budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReasoningBudgetScope {
    /// One incoming SSE delta.
    Delta,
    /// One logical provider call/assistant turn.
    Turn,
    /// All logical calls in one run, including tool rounds.
    Run,
}

/// A stream of provider events, boxed for dyn-compatibility: mock and real
/// providers hand out the same type (tech-spec §4.1 `EventStream`).
pub type EventStream = Pin<Box<dyn Stream<Item = Result<ProviderEvent, VegaError>> + Send>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderFailureKind {
    OutputLimit,
    Http,
    Transport,
    Protocol,
    Rejected,
}

#[derive(Clone, Default, PartialEq, Eq)]
pub struct ProviderResponseMetadata {
    pub http_status: Option<u16>,
    pub request_id: Option<String>,
    pub retry_count: Option<u32>,
}

impl std::fmt::Debug for ProviderResponseMetadata {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderResponseMetadata")
            .field("http_status", &self.http_status)
            .field("has_request_id", &self.request_id.is_some())
            .field("retry_count", &self.retry_count)
            .finish()
    }
}

pub struct ProviderStream {
    pub events: EventStream,
    pub metadata: ProviderResponseMetadata,
}

impl std::fmt::Debug for ProviderStream {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderStream")
            .field("metadata", &self.metadata)
            .finish_non_exhaustive()
    }
}

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
#[derive(Clone, Eq)]
pub struct ChatMessage {
    /// Explicit validated user images; never attached to other roles.
    pub images: Vec<crate::ImageAttachment>,
    /// Sender role.
    pub role: ChatRole,
    /// Plain-text content (markdown lives here verbatim).
    pub content: String,
    /// Provider tool-call id for a `tool` result message.
    pub tool_call_id: Option<String>,
    /// Calls requested by an assistant message before tool results follow.
    pub tool_calls: Vec<ChatToolCall>,
    /// Original provider reasoning content for an assistant tool-call turn.
    ///
    /// This field is run-memory only. It is never persisted by
    /// `vega_conversation`, rendered as visible text, or printed by `Debug`.
    pub reasoning_content: Option<String>,
    pub response_reasoning: Vec<serde_json::Value>,
    live_tool_calls: Option<Vec<Option<LiveToolCall>>>,
    model_projection: bool,
}

impl PartialEq for ChatMessage {
    fn eq(&self, other: &Self) -> bool {
        self.images == other.images
            && self.role == other.role
            && self.content == other.content
            && self.tool_call_id == other.tool_call_id
            && self.tool_calls == other.tool_calls
            && self.reasoning_content == other.reasoning_content
            && self.response_reasoning == other.response_reasoning
            && self.live_tool_calls == other.live_tool_calls
    }
}

#[derive(Clone, PartialEq, Eq)]
struct LiveToolCall {
    identity: ChatToolCall,
    model: ChatToolCall,
}

impl std::fmt::Debug for ChatMessage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ChatMessage")
            .field("role", &self.role)
            .field("content_bytes", &self.content.len())
            .field("image_count", &self.images.len())
            .field(
                "tool_call_id_bytes",
                &self.tool_call_id.as_ref().map(String::len),
            )
            .field("tool_call_count", &self.tool_calls.len())
            .field(
                "reasoning_content_bytes",
                &self.reasoning_content.as_ref().map(String::len),
            )
            .finish()
    }
}

impl ChatMessage {
    pub(crate) fn bind_live_tool_calls(
        &mut self,
        calls: Vec<Option<ChatToolCall>>,
    ) -> Result<(), VegaError> {
        if calls.len() != self.tool_calls.len() {
            return Err(VegaError::Context(
                crate::ContextRuntimeError::InvalidProjection,
            ));
        }
        let mut bound = Vec::with_capacity(calls.len());
        for (identity, model) in self.tool_calls.iter().zip(calls) {
            if let Some(model) = model {
                if model.id != identity.id || model.name != identity.name {
                    return Err(VegaError::Context(
                        crate::ContextRuntimeError::InvalidProjection,
                    ));
                }
                bound.push(Some(LiveToolCall {
                    identity: identity.clone(),
                    model,
                }));
            } else {
                bound.push(None);
            }
        }
        self.live_tool_calls = Some(bound);
        self.model_projection = false;
        Ok(())
    }

    pub fn without_live_tool_arguments(&self) -> Self {
        let mut message = self.clone();
        message.live_tool_calls = None;
        message.model_projection = false;
        message
    }

    pub fn mark_stored_tool_calls(&mut self) {
        self.live_tool_calls = Some(vec![None; self.tool_calls.len()]);
        self.model_projection = false;
    }

    pub(crate) fn discard_computer_use_arguments(&mut self) {
        if self
            .tool_calls
            .iter()
            .any(|call| crate::computer_use::ComputerUseRun::tool_capability(&call.name).is_some())
        {
            self.model_projection = false;
        }
        if let Some(calls) = &mut self.live_tool_calls {
            for (identity, model) in self.tool_calls.iter().zip(calls) {
                if crate::computer_use::ComputerUseRun::tool_capability(&identity.name).is_some() {
                    *model = None;
                }
            }
        }
    }

    /// Builds a message from a role and any string-like content.
    pub fn new(role: ChatRole, content: impl Into<String>) -> Self {
        Self {
            images: Vec::new(),
            role,
            content: content.into(),
            tool_call_id: None,
            tool_calls: Vec::new(),
            reasoning_content: None,
            response_reasoning: Vec::new(),
            live_tool_calls: None,
            model_projection: false,
        }
    }

    /// Builds the assistant turn that requested `tool_calls`.
    pub fn assistant_with_tools(content: impl Into<String>, tool_calls: Vec<ChatToolCall>) -> Self {
        Self {
            images: Vec::new(),
            role: ChatRole::Assistant,
            content: content.into(),
            tool_call_id: None,
            tool_calls,
            reasoning_content: None,
            response_reasoning: Vec::new(),
            live_tool_calls: None,
            model_projection: false,
        }
    }

    /// Builds the assistant turn that requested tools and preserves the
    /// provider's original reasoning content for the next round.
    pub fn assistant_with_tools_and_reasoning(
        content: impl Into<String>,
        reasoning_content: Option<String>,
        tool_calls: Vec<ChatToolCall>,
    ) -> Self {
        Self {
            role: ChatRole::Assistant,
            content: content.into(),
            tool_call_id: None,
            tool_calls,
            reasoning_content,
            response_reasoning: Vec::new(),
            live_tool_calls: None,
            model_projection: false,
            images: Vec::new(),
        }
    }

    pub fn projected_tool_result(call_id: impl Into<String>, tool: &str, content: &str) -> Self {
        const SHELL_MODEL_OUTPUT_BYTES: usize = 65_536;
        const OMISSION: &str = "\n[Shell output shortened: middle omitted for model context. Narrow/filter or page the command to retrieve more detail.]\n";
        if tool != "bash" || content.len() <= SHELL_MODEL_OUTPUT_BYTES {
            return Self::tool_result(call_id, content);
        }
        let retained_bytes = SHELL_MODEL_OUTPUT_BYTES - OMISSION.len();
        let mut head_end = retained_bytes / 2;
        while !content.is_char_boundary(head_end) {
            head_end -= 1;
        }
        let mut tail_start = content.len() - (retained_bytes - retained_bytes / 2);
        while !content.is_char_boundary(tail_start) {
            tail_start += 1;
        }
        Self::tool_result(
            call_id,
            format!(
                "{}{}{}",
                &content[..head_end],
                OMISSION,
                &content[tail_start..]
            ),
        )
    }

    /// Builds a tool-result message associated with `call_id`.
    pub fn tool_result(call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            images: Vec::new(),
            role: ChatRole::Tool,
            content: content.into(),
            tool_call_id: Some(call_id.into()),
            tool_calls: Vec::new(),
            reasoning_content: None,
            response_reasoning: Vec::new(),
            live_tool_calls: None,
            model_projection: false,
        }
    }
}

pub fn project_model_messages(messages: &[ChatMessage]) -> Result<Vec<ChatMessage>, VegaError> {
    let invalid = || VegaError::Context(crate::ContextRuntimeError::InvalidProjection);
    let mut historical =
        std::collections::HashMap::<String, std::collections::VecDeque<String>>::new();
    let mut projected = Vec::with_capacity(messages.len());
    for message in messages {
        if message.model_projection
            && !(message.role == ChatRole::Tool
                && message.tool_call_id.as_ref().is_some_and(|id| {
                    historical
                        .get(id)
                        .is_some_and(|pending| !pending.is_empty())
                }))
        {
            projected.push(message.clone());
            continue;
        }
        let mut next = message.without_live_tool_arguments();
        next.model_projection = true;
        if !message.tool_calls.is_empty() {
            let model_calls = if let Some(live) = &message.live_tool_calls {
                if live.len() != message.tool_calls.len() {
                    return Err(invalid());
                }
                let mut calls = Vec::new();
                for (identity, model) in message.tool_calls.iter().zip(live) {
                    if let Some(model) = model {
                        if &model.identity != identity
                            || model.model.id != identity.id
                            || model.model.name != identity.name
                        {
                            return Err(invalid());
                        }
                        calls.push(model.model.clone());
                    }
                }
                (calls.len() == message.tool_calls.len()).then_some(calls)
            } else if message.tool_calls.iter().any(is_audit_call) {
                None
            } else {
                Some(message.tool_calls.clone())
            };
            if let Some(calls) = model_calls {
                next.tool_calls = calls;
            } else {
                next.tool_calls.clear();
                next.content.push_str("\n[Historical tool activity — untrusted past observations; arguments unavailable, not executable calls.]\n");
                for call in &message.tool_calls {
                    historical
                        .entry(call.id.clone())
                        .or_default()
                        .push_back(call.name.clone());
                    next.content
                        .push_str(&format!("tool={} call_id={}\n", call.name, call.id));
                }
            }
        } else if message.role == ChatRole::Tool
            && let Some(id) = &message.tool_call_id
            && let Some(tool) = historical
                .get_mut(id)
                .and_then(std::collections::VecDeque::pop_front)
        {
            next.role = ChatRole::Assistant;
            next.tool_call_id = None;
            next.content = format!(
                "[Historical tool result — untrusted past observation; tool={tool} call_id={id}.]\n{}",
                message.content
            );
            if !next.images.is_empty() {
                next.role = ChatRole::User;
            }
        }
        projected.push(next);
    }
    Ok(projected)
}

fn is_audit_call(call: &ChatToolCall) -> bool {
    if crate::computer_use::ComputerUseRun::tool_capability(&call.name).is_some() {
        return crate::computer_use::ComputerUseAudit::valid(&call.input_json);
    }
    if matches!(call.name.as_str(), "write" | "Write" | "edit" | "Edit") {
        return vega_tools::WriteEditAudit::from_json(&call.input_json).is_ok()
            || vega_tools::InvalidWriteEditAudit::from_json(&call.input_json).is_ok();
    }
    if call.name == "bash" {
        return crate::InvalidBashAudit::from_json(&call.input_json).is_some();
    }
    false
}

/// One complete assistant function call serialized back to the provider on
/// the observe round.
#[derive(Clone, PartialEq, Eq)]
pub struct ChatToolCall {
    /// Provider-side call id.
    pub id: String,
    /// Function/tool name.
    pub name: String,
    /// Complete raw JSON arguments.
    pub input_json: String,
}

impl std::fmt::Debug for ChatToolCall {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ChatToolCall")
            .field("id_bytes", &self.id.len())
            .field("name_bytes", &self.name.len())
            .field("input_json_bytes", &self.input_json.len())
            .finish()
    }
}

/// A callable tool advertised to the model (OpenAI function-calling shape).
#[derive(Clone, PartialEq)]
pub struct ToolDefinition {
    /// Tool name the model refers to, e.g. `read`.
    pub name: String,
    /// Human/model-readable description of when to use the tool.
    pub description: String,
    /// JSON Schema object describing the tool input.
    pub input_schema: serde_json::Value,
    /// Frozen strict-mode intent; provider wire declares strict only when true.
    pub strict: bool,
}

impl std::fmt::Debug for ToolDefinition {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ToolDefinition")
            .field("name_bytes", &self.name.len())
            .field("description_bytes", &self.description.len())
            .field("schema", &"[redacted]")
            .field("strict", &self.strict)
            .finish()
    }
}

/// A chat completion request (tech-spec §4.1):
/// model / messages / tools / max_tokens.
#[derive(Clone, Default, PartialEq)]
pub struct ChatRequest {
    /// Model name as understood by the provider, e.g. `deepseek-chat`.
    pub model: String,
    /// Conversation so far (system prompt + history window; T20 assembles).
    pub messages: Vec<ChatMessage>,
    /// Tools advertised to the model; empty when none.
    pub tools: Vec<ToolDefinition>,
    /// Generation cap in tokens; `None` lets the provider default apply.
    pub max_tokens: Option<u32>,
    /// Optional frozen thinking choice. `None` is retained for legacy callers
    /// and means provider default with no extra wire fields.
    pub reasoning: Option<FrozenReasoning>,
}

impl std::fmt::Debug for ChatRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ChatRequest")
            .field("model_is_empty", &self.model.is_empty())
            .field("model_bytes", &self.model.len())
            .field("message_count", &self.messages.len())
            .field("tool_count", &self.tools.len())
            .field("max_tokens", &self.max_tokens)
            .field("reasoning", &self.reasoning)
            .finish()
    }
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
    Unknown,
}

/// Incremental provider event (tech-spec §4.1). [`ProviderEvent::ToolUse`]
/// is emitted only after the model's argument fragments are fully aggregated.
#[derive(Clone, PartialEq, Eq)]
pub enum ProviderEvent {
    ResponseMetadata(ProviderResponseMetadata),
    RetryScheduled {
        attempt: u32,
        max_retries: u32,
    },
    /// Incremental visible text.
    TextDelta(String),
    /// Incremental reasoning text (OpenAI-compatible `reasoning_content`).
    ThinkingDelta(String),
    SummaryDelta(String),
    ReasoningReplay(Vec<serde_json::Value>),
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

impl std::fmt::Debug for ProviderEvent {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ResponseMetadata(metadata) => formatter
                .debug_tuple("ResponseMetadata")
                .field(metadata)
                .finish(),
            Self::RetryScheduled {
                attempt,
                max_retries,
            } => formatter
                .debug_struct("RetryScheduled")
                .field("attempt", attempt)
                .field("max_retries", max_retries)
                .finish(),
            Self::TextDelta(text) => formatter
                .debug_tuple("TextDelta")
                .field(&format_args!("[redacted; {} bytes]", text.len()))
                .finish(),
            Self::ReasoningReplay(items) => formatter
                .debug_tuple("ReasoningReplay")
                .field(&items.len())
                .finish(),
            Self::SummaryDelta(text) => formatter
                .debug_tuple("SummaryDelta")
                .field(&format_args!("[redacted; {} bytes]", text.len()))
                .finish(),
            Self::ThinkingDelta(text) => formatter
                .debug_tuple("ThinkingDelta")
                .field(&format_args!("[redacted; {} bytes]", text.len()))
                .finish(),
            Self::ToolUse {
                id,
                name,
                input_json,
            } => formatter
                .debug_struct("ToolUse")
                .field("id_bytes", &id.len())
                .field("name_bytes", &name.len())
                .field("input_json_bytes", &input_json.len())
                .finish(),
            Self::Usage {
                input,
                output,
                cache_read,
                cache_write,
            } => formatter
                .debug_struct("Usage")
                .field("input", input)
                .field("output", output)
                .field("cache_read", cache_read)
                .field("cache_write", cache_write)
                .finish(),
            Self::Done { stop_reason } => formatter
                .debug_struct("Done")
                .field("stop_reason", stop_reason)
                .finish(),
        }
    }
}

/// LLM provider abstraction (tech-spec §4.1).
///
/// `chat_stream` returns a `BoxFuture` resolving to an [`EventStream`] so
/// that connection setup, retries, and cancellation all happen inside the
/// future while the loop only polls events afterwards.
pub trait Provider: Send + Sync {
    fn chat_stream_recovering(
        &self,
        req: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<ProviderStream, VegaError>> {
        self.chat_stream_with_metadata(req, cancel)
    }
    /// A single auxiliary request, without transport retries. Implementations
    /// with retrying transports must override this entry point.
    fn chat_stream_once(
        &self,
        req: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        self.chat_stream(req, cancel)
    }
    fn chat_stream_once_with_metadata(
        &self,
        req: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<ProviderStream, VegaError>> {
        let future = self.chat_stream_once(req, cancel);
        Box::pin(async move {
            future.await.map(|events| ProviderStream {
                events,
                metadata: ProviderResponseMetadata::default(),
            })
        })
    }
    /// Starts a streaming chat completion for `req`; `cancel` aborts the
    /// attempt (and later the stream) as soon as it fires.
    fn chat_stream(
        &self,
        req: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>>;

    fn chat_stream_with_metadata(
        &self,
        req: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<ProviderStream, VegaError>> {
        let future = self.chat_stream(req, cancel);
        Box::pin(async move {
            future.await.map(|events| ProviderStream {
                events,
                metadata: ProviderResponseMetadata::default(),
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bound_call(id: &str, name: &str, audit: &str, input: &str) -> ChatMessage {
        let mut message = ChatMessage::assistant_with_tools(
            "",
            vec![ChatToolCall {
                id: id.into(),
                name: name.into(),
                input_json: audit.into(),
            }],
        );
        message
            .bind_live_tool_calls(vec![Some(ChatToolCall {
                id: id.into(),
                name: name.into(),
                input_json: input.into(),
            })])
            .unwrap();
        message
    }

    #[test]
    fn issue300_projection_is_idempotent_and_never_infers_mcp_audits_from_business_fields() {
        let input = r#"{"server_id":"business","config_revision":1,"arguments_bytes":12,"arguments_sha256":"business-value"}"#;
        let live = bound_call("mcp-1", "mcp__fixture", "{\"audit\":true}", input);
        for call in [
            live,
            ChatMessage::assistant_with_tools(
                "",
                vec![ChatToolCall {
                    id: "mcp-1".into(),
                    name: "mcp__fixture".into(),
                    input_json: input.into(),
                }],
            ),
        ] {
            let projected = project_model_messages(&[
                call,
                ChatMessage::tool_result("mcp-1", "server-says-ok"),
            ])
            .unwrap();
            assert_eq!(projected[0].tool_calls[0].input_json, input);
            assert_eq!(projected[1].role, ChatRole::Tool);
            assert_eq!(project_model_messages(&projected).unwrap(), projected);
        }
    }

    #[test]
    fn issue300_mixed_group_preserves_images_and_consumes_reused_call_ids() {
        let mut group = ChatMessage::assistant_with_tools(
            "checking",
            vec![
                ChatToolCall {
                    id: "r1".into(),
                    name: "read".into(),
                    input_json: "{}".into(),
                },
                ChatToolCall {
                    id: "bad".into(),
                    name: "write".into(),
                    input_json: "{}".into(),
                },
            ],
        );
        group
            .bind_live_tool_calls(vec![
                Some(ChatToolCall {
                    id: "r1".into(),
                    name: "read".into(),
                    input_json: r#"{"path":"a"}"#.into(),
                }),
                None,
            ])
            .unwrap();
        let mut image_bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(2, 2)
            .write_to(&mut image_bytes, image::ImageFormat::Png)
            .unwrap();
        let image = crate::ImageAttachment::from_bytes(image_bytes.into_inner()).unwrap();
        let mut first_result = ChatMessage::tool_result("r1", "first result");
        first_result.images.push(image.clone());
        let replay = bound_call("r1", "read", "{}", r#"{"path":"a"}"#);
        let projected = project_model_messages(&[
            group,
            first_result,
            ChatMessage::tool_result("bad", "missing_content"),
            replay,
            ChatMessage::tool_result("r1", "reused result"),
        ])
        .unwrap();
        assert!(projected[0].tool_calls.is_empty());
        assert!(projected[0].content.contains("Historical tool activity"));
        assert_eq!(projected[1].role, ChatRole::User);
        assert_eq!(projected[1].images, vec![image]);
        assert!(projected[1].content.ends_with("first result"));
        assert!(projected[2].content.ends_with("missing_content"));
        assert_eq!(projected[3].tool_calls[0].id, "r1");
        assert_eq!(
            projected[4],
            ChatMessage::tool_result("r1", "reused result")
        );
        assert_eq!(project_model_messages(&projected).unwrap(), projected);
    }

    #[test]
    fn issue300_sidecar_binds_full_audit_identity_and_is_debug_redacted() {
        let root = tempfile::tempdir().unwrap();
        let audit = vega_tools::Tools::new(root.path())
            .unwrap()
            .audit_write_json(r#"{"path":"a","content":"PRIVATE_BODY"}"#)
            .unwrap()
            .to_json()
            .unwrap();
        let bound = bound_call(
            "w1",
            "write",
            &audit,
            r#"{"path":"a","content":"PRIVATE_BODY"}"#,
        );
        assert!(!format!("{bound:?}").contains("PRIVATE_BODY"));
        assert!(!bound.tool_calls[0].input_json.contains("PRIVATE_BODY"));
        assert_ne!(bound, bound.without_live_tool_arguments());
        for field in ["id", "name", "audit"] {
            let mut changed = bound.clone();
            match field {
                "id" => changed.tool_calls[0].id = "another".into(),
                "name" => changed.tool_calls[0].name = "edit".into(),
                _ => changed.tool_calls[0].input_json = "{}".into(),
            }
            assert!(matches!(
                project_model_messages(&[changed]),
                Err(VegaError::Context(
                    crate::ContextRuntimeError::InvalidProjection
                ))
            ));
        }
    }

    #[test]
    fn issue300_stored_empty_audits_are_history_and_cua_redaction_cannot_revive_arguments() {
        for tool in ["read", "load_skill", "unknown"] {
            let mut message = ChatMessage::assistant_with_tools(
                "",
                vec![ChatToolCall {
                    id: "old".into(),
                    name: tool.into(),
                    input_json: "{}".into(),
                }],
            );
            message.mark_stored_tool_calls();
            let projected =
                project_model_messages(&[message, ChatMessage::tool_result("old", "rejected")])
                    .unwrap();
            assert!(
                projected
                    .iter()
                    .all(|m| m.tool_calls.is_empty() && m.tool_call_id.is_none())
            );
            assert!(projected[1].content.ends_with("rejected"));
        }
        let message = bound_call(
            "cua",
            "ComputerScreenshot",
            "{}",
            r#"{"target_id":"PRIVATE_TARGET"}"#,
        );
        let internal = vec![message, ChatMessage::tool_result("cua", "PRIVATE_RESULT")];
        for mut messages in [internal.clone(), project_model_messages(&internal).unwrap()] {
            crate::computer_use::redact_ephemeral_messages(&mut messages);
            let projected = project_model_messages(&messages).unwrap();
            assert!(
                projected
                    .iter()
                    .all(|m| m.tool_calls.is_empty() && m.tool_call_id.is_none())
            );
            assert!(
                !projected
                    .iter()
                    .any(|m| m.content.contains("PRIVATE_TARGET"))
            );
        }
    }

    #[test]
    fn issue302_shell_projection_bounds_utf8_and_preserves_other_tools() {
        for output in [String::new(), "x".repeat(65_535), "x".repeat(65_536)] {
            assert_eq!(
                ChatMessage::projected_tool_result("call", "bash", &output),
                ChatMessage::tool_result("call", &output)
            );
        }
        for output in [
            "x".repeat(65_537),
            "x".repeat(2_622_078),
            "界🦀".repeat(30_000),
        ] {
            let projected = ChatMessage::projected_tool_result("call", "bash", &output);
            assert!(projected.content.len() <= 65_536);
            let (head, rest) = projected
                .content
                .split_once("\n[Shell output shortened:")
                .unwrap();
            let (_, tail) = rest.split_once("]\n").unwrap();
            assert!(!head.is_empty() && !tail.is_empty());
            assert!(output.starts_with(head));
            assert!(output.ends_with(tail));
            assert!(projected.content.contains("middle omitted"));
            assert!(projected.content.contains("Narrow/filter or page"));
            assert_eq!(
                ChatMessage::projected_tool_result("call", "bash", &projected.content),
                projected
            );
            for tool in ["read", "edit", "write", "mcp__fixture"] {
                assert_eq!(
                    ChatMessage::projected_tool_result("call", tool, &output),
                    ChatMessage::tool_result("call", &output)
                );
            }
        }
    }

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

    #[test]
    fn response_metadata_debug_hides_request_id_value() {
        const REQUEST_ID_SENTINEL: &str = "VEGA_REQUEST_ID_CANARY";
        let metadata = ProviderResponseMetadata {
            http_status: Some(200),
            request_id: Some(REQUEST_ID_SENTINEL.to_string()),
            retry_count: Some(1),
        };
        let rendered = format!("{metadata:?}");
        assert!(rendered.contains("200"));
        assert!(rendered.contains("retry_count"));
        assert!(!rendered.contains(REQUEST_ID_SENTINEL));
    }

    #[test]
    fn provider_debug_carriers_redact_distinct_payloads() {
        let sentinels = [
            "VEGA_MODEL_SENTINEL",
            "VEGA_MESSAGE_SENTINEL",
            "VEGA_CALL_ID_SENTINEL",
            "VEGA_TOOL_NAME_SENTINEL",
            "VEGA_TOOL_INPUT_SENTINEL",
            "VEGA_SCHEMA_SENTINEL",
            "VEGA_THINKING_SENTINEL",
        ];
        let call = ChatToolCall {
            id: sentinels[2].into(),
            name: sentinels[3].into(),
            input_json: sentinels[4].into(),
        };
        let request = ChatRequest {
            model: sentinels[0].into(),
            messages: vec![ChatMessage::assistant_with_tools(sentinels[1], vec![call])],
            tools: vec![ToolDefinition {
                name: sentinels[3].into(),
                description: sentinels[1].into(),
                input_schema: serde_json::json!({"value": sentinels[5]}),
                strict: false,
            }],
            max_tokens: Some(256),
            reasoning: None,
        };
        let values = [
            format!("{request:?}"),
            format!("{:?}", request.messages[0]),
            format!("{:?}", request.messages[0].tool_calls[0]),
            format!("{:?}", request.tools[0]),
            format!("{:?}", ProviderEvent::ThinkingDelta(sentinels[6].into())),
            format!("{:?}", ProviderEvent::TextDelta(sentinels[1].into())),
            format!(
                "{:?}",
                ProviderEvent::ToolUse {
                    id: sentinels[2].into(),
                    name: sentinels[3].into(),
                    input_json: sentinels[4].into(),
                }
            ),
        ];
        for rendered in values {
            for sentinel in sentinels {
                assert!(
                    !rendered.contains(sentinel),
                    "provider Debug leaked payload"
                );
            }
        }
    }
}
