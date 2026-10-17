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

use std::fmt;
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
    DeepSeekChatCompletions,
    ResponsesApi,
    /// No declared protocol or an endpoint whose capability is unknown.
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReasoningReplayPolicy {
    StandardChat,
    DeepSeekTools,
    GlmPreserved,
    GlmInterleaved,
    ResponsesItems,
    LegacyTextTools,
    #[default]
    Unknown,
}

impl ReasoningReplayPolicy {
    pub fn preserves_chat_response(self, tools_requested: bool) -> bool {
        match self {
            Self::DeepSeekTools | Self::LegacyTextTools => tools_requested,
            Self::GlmPreserved | Self::GlmInterleaved | Self::ResponsesItems => true,
            Self::StandardChat | Self::Unknown => false,
        }
    }

    pub fn requires_chat_replay_for_tools(self) -> bool {
        matches!(self, Self::DeepSeekTools | Self::GlmPreserved)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ReplayFrame {
    cold_prefix_len: usize,
    interleaved_user_boundary: Option<usize>,
    policy: ReasoningReplayPolicy,
    seal_eligible: bool,
    route_fingerprint: Option<[u8; 32]>,
    environment_fingerprint: Option<[u8; 32]>,
    candidate: Option<Box<ReplayCandidate>>,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ReplayCandidate {
    pub(crate) messages: Vec<ChatMessage>,
    pub(crate) environment_fingerprint: [u8; 32],
}

impl ReplayFrame {
    pub fn new(cold_prefix_len: usize, policy: ReasoningReplayPolicy) -> Self {
        Self {
            cold_prefix_len,
            interleaved_user_boundary: (policy == ReasoningReplayPolicy::GlmInterleaved)
                .then_some(cold_prefix_len),
            policy,
            seal_eligible: true,
            route_fingerprint: None,
            environment_fingerprint: None,
            candidate: None,
        }
    }

    pub fn cold_prefix_len(&self) -> usize {
        self.cold_prefix_len
    }

    #[cfg(test)]
    pub(crate) fn interleaved_user_boundary(&self) -> Option<usize> {
        self.interleaved_user_boundary
    }

    pub fn policy(&self) -> ReasoningReplayPolicy {
        self.policy
    }

    pub fn seal_eligible(&self) -> bool {
        self.seal_eligible
    }

    #[doc(hidden)]
    pub fn environment_fingerprint(&self) -> Option<[u8; 32]> {
        self.environment_fingerprint
    }

    #[doc(hidden)]
    pub fn attach_route_fingerprint(&mut self, fingerprint: [u8; 32]) {
        self.route_fingerprint = Some(fingerprint);
    }

    #[doc(hidden)]
    pub fn attach_candidate(
        &mut self,
        mut messages: Vec<ChatMessage>,
        environment_fingerprint: [u8; 32],
    ) {
        if let Some(system) = messages
            .first_mut()
            .filter(|message| message.role == ChatRole::System)
            && let Some(mut frame) = system.replay_frame().cloned()
        {
            frame.take_runtime_fence();
            system.attach_replay_frame(frame);
        }
        self.candidate = Some(Box::new(ReplayCandidate {
            messages,
            environment_fingerprint,
        }));
    }

    pub(crate) fn take_runtime_fence(&mut self) -> (Option<[u8; 32]>, Option<ReplayCandidate>) {
        (
            self.route_fingerprint.take(),
            self.candidate.take().map(|value| *value),
        )
    }

    #[doc(hidden)]
    pub fn bind_environment(&mut self, fingerprint: [u8; 32]) {
        self.environment_fingerprint = Some(fingerprint);
    }

    fn resident_size_bytes(&self) -> usize {
        let candidate_bytes = self.candidate.as_ref().map_or(0, |candidate| {
            candidate
                .messages
                .iter()
                .fold(std::mem::size_of::<ReplayCandidate>(), |total, message| {
                    total.saturating_add(message.replay_resident_size_bytes())
                })
                .saturating_add(
                    candidate
                        .messages
                        .capacity()
                        .saturating_sub(candidate.messages.len())
                        .saturating_mul(std::mem::size_of::<ChatMessage>()),
                )
        });
        std::mem::size_of::<Self>().saturating_add(candidate_bytes)
    }

    fn set_cold_prefix_len(&mut self, cold_prefix_len: usize) {
        self.cold_prefix_len = cold_prefix_len;
        if self.policy == ReasoningReplayPolicy::GlmInterleaved {
            self.interleaved_user_boundary = Some(cold_prefix_len);
        }
    }

    fn set_interleaved_user_boundary(&mut self, boundary: usize) {
        if self.policy == ReasoningReplayPolicy::GlmInterleaved {
            self.interleaved_user_boundary = Some(boundary);
        }
    }

    fn set_projected_boundaries(&mut self, cold_prefix_len: usize, user_boundary: Option<usize>) {
        self.cold_prefix_len = cold_prefix_len;
        if self.policy == ReasoningReplayPolicy::GlmInterleaved {
            self.interleaved_user_boundary = user_boundary;
        }
    }

    fn disable_seal(&mut self) {
        self.seal_eligible = false;
    }
}

impl std::fmt::Debug for ReplayFrame {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ReplayFrame")
            .field("cold_prefix_len", &self.cold_prefix_len)
            .field("interleaved_user_boundary", &self.interleaved_user_boundary)
            .field("policy", &self.policy)
            .field("seal_eligible", &self.seal_eligible)
            .field("has_route_fingerprint", &self.route_fingerprint.is_some())
            .field(
                "has_environment_fingerprint",
                &self.environment_fingerprint.is_some(),
            )
            .field("has_candidate", &self.candidate.is_some())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Default)]
pub enum ChatReasoningPresence {
    #[default]
    Absent,
    Empty,
    NonEmpty(String),
}

impl std::fmt::Debug for ChatReasoningPresence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Absent => formatter.write_str("Absent"),
            Self::Empty => formatter.write_str("Empty"),
            Self::NonEmpty(text) => formatter
                .debug_tuple("NonEmpty")
                .field(&format_args!("[redacted; {} bytes]", text.len()))
                .finish(),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum ResponseReplaySlot {
    Reasoning(serde_json::Value),
    Message(serde_json::Value),
    FunctionCall {
        item: serde_json::Value,
        call_id: String,
        name: String,
        arguments: String,
    },
}

#[derive(Clone, PartialEq, Eq, Default)]
pub struct ResponseReplayLayout {
    pub response_id: Option<String>,
    pub slots: Vec<ResponseReplaySlot>,
}

impl std::fmt::Debug for ResponseReplayLayout {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResponseReplayLayout")
            .field("has_response_id", &self.response_id.is_some())
            .field("slot_count", &self.slots.len())
            .finish()
    }
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
    pub replay_policy: ReasoningReplayPolicy,
    /// User choice: provider default, a true disabled operation, or a declared
    /// effort value.
    pub choice: ReasoningChoice,
    /// Whether the profile explicitly permits the disabled operation.
    pub supports_disabled: bool,
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
            .field("replay_policy", &self.replay_policy)
            .field("choice", &self.choice)
            .field("supports_disabled", &self.supports_disabled)
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
            replay_policy: ReasoningReplayPolicy::Unknown,
            choice: ReasoningChoice::ProviderDefault,
            supports_disabled: false,
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
            && (self.replay_policy != ReasoningReplayPolicy::Unknown
                && !(self.replay_policy == ReasoningReplayPolicy::LegacyTextTools
                    && !self.supports_disabled
                    && self.disabled_wire.is_none()
                    && self.declared_efforts.is_empty())
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
                ) | (
                    ReasoningProtocol::DeepSeekChatCompletions,
                    Some(ReasoningDisabledWire::ThinkingTypeDisabled)
                ) | (
                    ReasoningProtocol::ResponsesApi,
                    Some(ReasoningDisabledWire::ReasoningEffortNone)
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
        if matches!(self.protocol, ReasoningProtocol::DeepSeekChatCompletions)
            && self.declared_efforts.iter().any(|effort| {
                !matches!(
                    effort.as_str(),
                    "minimal" | "low" | "medium" | "high" | "xhigh" | "max"
                )
            })
        {
            return Err(VegaError::ReasoningSelectionInvalid {
                message: "DeepSeek profiles support only low/high/max".to_string(),
            });
        }
        let policy_protocol_matches = match self.replay_policy {
            ReasoningReplayPolicy::Unknown => true,
            ReasoningReplayPolicy::StandardChat => {
                !matches!(self.protocol, ReasoningProtocol::Unknown)
            }
            ReasoningReplayPolicy::LegacyTextTools => true,
            ReasoningReplayPolicy::DeepSeekTools => {
                matches!(self.protocol, ReasoningProtocol::DeepSeekChatCompletions)
            }
            ReasoningReplayPolicy::GlmPreserved | ReasoningReplayPolicy::GlmInterleaved => {
                matches!(self.protocol, ReasoningProtocol::ZhipuChatCompletions)
            }
            ReasoningReplayPolicy::ResponsesItems => {
                matches!(self.protocol, ReasoningProtocol::ResponsesApi)
            }
        };
        if !policy_protocol_matches {
            return Err(VegaError::ReasoningSelectionInvalid {
                message: "reasoning replay policy conflicts with the declared protocol".to_string(),
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
                        ) | (
                            ReasoningProtocol::DeepSeekChatCompletions,
                            Some(ReasoningDisabledWire::ThinkingTypeDisabled)
                        ) | (
                            ReasoningProtocol::ResponsesApi,
                            Some(ReasoningDisabledWire::ReasoningEffortNone)
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
    pub response_replay_layout: Option<ResponseReplayLayout>,
    replay_frame: Option<ReplayFrame>,
    live_tool_calls: Option<Vec<Option<LiveToolCall>>>,
    historical_tool_statuses: Option<Vec<HistoricalToolStatus>>,
    model_projection: bool,
    replay_cold: bool,
}

#[derive(Clone, PartialEq, Eq)]
struct HistoricalToolStatus {
    id: String,
    name: String,
    status: Option<StoredToolStatus>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum StoredToolStatus {
    Success,
    Failed,
    Rejected,
    Cancelled,
}

impl StoredToolStatus {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "success" => Some(Self::Success),
            "failed" => Some(Self::Failed),
            "rejected" => Some(Self::Rejected),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failed => "failed",
            Self::Rejected => "rejected",
            Self::Cancelled => "cancelled",
        }
    }
}

struct PendingHistoricalTool {
    tool: String,
    status: Option<StoredToolStatus>,
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
            && self.response_replay_layout == other.response_replay_layout
            && self.replay_frame == other.replay_frame
            && self.live_tool_calls == other.live_tool_calls
            && self.replay_cold == other.replay_cold
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
                "has_response_replay_layout",
                &self.response_replay_layout.is_some(),
            )
            .field("has_replay_frame", &self.replay_frame.is_some())
            .field("replay_cold", &self.replay_cold)
            .field(
                "reasoning_content_bytes",
                &self.reasoning_content.as_ref().map(String::len),
            )
            .finish()
    }
}

impl ChatMessage {
    #[doc(hidden)]
    pub fn replay_resident_size_bytes(&self) -> usize {
        fn string_size(value: &String) -> usize {
            value.capacity()
        }

        fn call_size(call: &ChatToolCall) -> usize {
            std::mem::size_of::<ChatToolCall>()
                .saturating_add(string_size(&call.id))
                .saturating_add(string_size(&call.name))
                .saturating_add(string_size(&call.input_json))
        }

        fn value_heap_size(value: &serde_json::Value) -> usize {
            const JSON_OBJECT_NODE_RESIDENT_OVERHEAD: usize = 1024;

            match value {
                serde_json::Value::Null
                | serde_json::Value::Bool(_)
                | serde_json::Value::Number(_) => 0,
                serde_json::Value::String(value) => value.capacity(),
                serde_json::Value::Array(values) => values
                    .capacity()
                    .saturating_mul(std::mem::size_of::<serde_json::Value>())
                    .saturating_add(values.iter().fold(0usize, |total, value| {
                        total.saturating_add(value_heap_size(value))
                    })),
                serde_json::Value::Object(values) => {
                    let entry_overhead = std::mem::size_of::<(String, serde_json::Value)>()
                        .saturating_mul(2)
                        .saturating_add(4usize.saturating_mul(std::mem::size_of::<usize>()));
                    let node_overhead = if values.is_empty() {
                        0
                    } else {
                        JSON_OBJECT_NODE_RESIDENT_OVERHEAD
                    };
                    node_overhead
                        .saturating_add(values.len().saturating_mul(entry_overhead))
                        .saturating_add(values.iter().fold(0usize, |total, (key, value)| {
                            total
                                .saturating_add(key.capacity())
                                .saturating_add(value_heap_size(value))
                        }))
                }
            }
        }

        fn value_size(value: &serde_json::Value) -> usize {
            std::mem::size_of::<serde_json::Value>().saturating_add(value_heap_size(value))
        }

        let mut size = std::mem::size_of::<Self>()
            .saturating_add(string_size(&self.content))
            .saturating_add(self.tool_call_id.as_ref().map_or(0, string_size))
            .saturating_add(self.reasoning_content.as_ref().map_or(0, string_size))
            .saturating_add(
                self.tool_calls
                    .capacity()
                    .saturating_mul(std::mem::size_of::<ChatToolCall>()),
            )
            .saturating_add(
                self.tool_calls
                    .iter()
                    .fold(0usize, |total, call| total.saturating_add(call_size(call))),
            )
            .saturating_add(
                self.response_reasoning
                    .capacity()
                    .saturating_mul(std::mem::size_of::<serde_json::Value>()),
            )
            .saturating_add(
                self.response_reasoning
                    .iter()
                    .fold(0usize, |total, item| total.saturating_add(value_size(item))),
            )
            .saturating_add(
                self.images
                    .capacity()
                    .saturating_mul(std::mem::size_of::<crate::ImageAttachment>()),
            );
        for image in &self.images {
            size = size
                .saturating_add(image.bytes().len())
                .saturating_add(image.mime_type().len())
                .saturating_add(std::mem::size_of::<(u32, u32)>());
        }
        if let Some(layout) = &self.response_replay_layout {
            size = size
                .saturating_add(std::mem::size_of::<ResponseReplayLayout>())
                .saturating_add(layout.response_id.as_ref().map_or(0, string_size))
                .saturating_add(
                    layout
                        .slots
                        .capacity()
                        .saturating_mul(std::mem::size_of::<ResponseReplaySlot>()),
                );
            for slot in &layout.slots {
                size = size.saturating_add(match slot {
                    ResponseReplaySlot::Reasoning(item) | ResponseReplaySlot::Message(item) => {
                        value_size(item)
                    }
                    ResponseReplaySlot::FunctionCall {
                        item,
                        call_id,
                        name,
                        arguments,
                    } => value_size(item)
                        .saturating_add(string_size(call_id))
                        .saturating_add(string_size(name))
                        .saturating_add(string_size(arguments)),
                });
            }
        }
        if let Some(calls) = &self.live_tool_calls {
            size = size.saturating_add(
                calls
                    .capacity()
                    .saturating_mul(std::mem::size_of::<Option<LiveToolCall>>()),
            );
            for call in calls.iter().flatten() {
                size = size
                    .saturating_add(call_size(&call.identity))
                    .saturating_add(call_size(&call.model));
            }
        }
        if let Some(statuses) = &self.historical_tool_statuses {
            size = size.saturating_add(
                statuses
                    .capacity()
                    .saturating_mul(std::mem::size_of::<HistoricalToolStatus>()),
            );
            for status in statuses {
                size = size
                    .saturating_add(string_size(&status.id))
                    .saturating_add(string_size(&status.name));
            }
        }
        size.saturating_add(
            self.replay_frame
                .as_ref()
                .map_or(0, ReplayFrame::resident_size_bytes),
        )
    }

    pub fn attach_replay_frame(&mut self, frame: ReplayFrame) {
        if self.role == ChatRole::System {
            self.replay_cold = frame.cold_prefix_len > 0;
            self.replay_frame = Some(frame);
        }
    }

    pub fn replay_frame(&self) -> Option<&ReplayFrame> {
        self.replay_frame.as_ref()
    }

    pub fn update_replay_cold_prefix(&mut self, cold_prefix_len: usize) {
        if self.role == ChatRole::System
            && let Some(frame) = &mut self.replay_frame
        {
            frame.set_cold_prefix_len(cold_prefix_len);
        }
    }

    pub(crate) fn begin_interleaved_user_round(&mut self, boundary: usize) {
        if self.role == ChatRole::System
            && let Some(frame) = &mut self.replay_frame
        {
            frame.set_interleaved_user_boundary(boundary);
        }
    }

    pub(crate) fn update_replay_projection_boundaries(
        &mut self,
        cold_prefix_len: usize,
        user_boundary: Option<usize>,
    ) {
        if self.role == ChatRole::System
            && let Some(frame) = &mut self.replay_frame
        {
            frame.set_projected_boundaries(cold_prefix_len, user_boundary);
        }
    }

    pub fn mark_replay_unsealable(&mut self) {
        if self.role == ChatRole::System
            && let Some(frame) = &mut self.replay_frame
        {
            frame.disable_seal();
        }
    }

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
        self.historical_tool_statuses = None;
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
        self.historical_tool_statuses = Some(
            self.tool_calls
                .iter()
                .map(|call| HistoricalToolStatus {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    status: None,
                })
                .collect(),
        );
        self.model_projection = false;
    }

    pub fn mark_stored_tool_calls_with_statuses(
        &mut self,
        statuses: Vec<String>,
    ) -> Result<(), VegaError> {
        let invalid = || VegaError::Context(crate::ContextRuntimeError::InvalidProjection);
        if statuses.len() != self.tool_calls.len() {
            return Err(invalid());
        }
        let statuses = statuses
            .iter()
            .map(|status| StoredToolStatus::parse(status).ok_or_else(invalid))
            .collect::<Result<Vec<_>, _>>()?;
        let historical_tool_statuses = self
            .tool_calls
            .iter()
            .zip(statuses)
            .map(|(call, status)| HistoricalToolStatus {
                id: call.id.clone(),
                name: call.name.clone(),
                status: Some(status),
            })
            .collect();
        self.live_tool_calls = Some(vec![None; self.tool_calls.len()]);
        self.historical_tool_statuses = Some(historical_tool_statuses);
        self.model_projection = false;
        Ok(())
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
            response_replay_layout: None,
            replay_frame: None,
            live_tool_calls: None,
            historical_tool_statuses: None,
            model_projection: false,
            replay_cold: false,
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
            response_replay_layout: None,
            replay_frame: None,
            live_tool_calls: None,
            historical_tool_statuses: None,
            model_projection: false,
            replay_cold: false,
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
            response_replay_layout: None,
            replay_frame: None,
            live_tool_calls: None,
            historical_tool_statuses: None,
            model_projection: false,
            replay_cold: false,
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
            response_replay_layout: None,
            replay_frame: None,
            live_tool_calls: None,
            historical_tool_statuses: None,
            model_projection: false,
            replay_cold: false,
        }
    }
}

pub fn project_model_messages(messages: &[ChatMessage]) -> Result<Vec<ChatMessage>, VegaError> {
    let invalid = || VegaError::Context(crate::ContextRuntimeError::InvalidProjection);
    let mut cold_projected = Vec::with_capacity(messages.len());
    let mut cold_boundary = None;
    let mut interleaved_user_boundary = None;
    if let Some(system) = messages.first()
        && system.role == ChatRole::System
        && let Some(frame) = &system.replay_frame
    {
        if frame.cold_prefix_len == 0 || frame.cold_prefix_len > messages.len() {
            return Err(invalid());
        }
        cold_boundary = Some(frame.cold_prefix_len);
        if frame.policy == ReasoningReplayPolicy::GlmInterleaved {
            let boundary = frame.interleaved_user_boundary.ok_or_else(invalid)?;
            if boundary < frame.cold_prefix_len || boundary > messages.len() {
                return Err(invalid());
            }
            interleaved_user_boundary = Some(boundary);
        }
        for (index, message) in messages.iter().enumerate() {
            let mut current = message.clone();
            if interleaved_user_boundary.is_some_and(|boundary| {
                index >= frame.cold_prefix_len
                    && index < boundary
                    && message.role == ChatRole::Assistant
            }) {
                current.reasoning_content = None;
            }
            if index >= frame.cold_prefix_len {
                cold_projected.push(current);
                continue;
            }
            let mut cold = current;
            cold.replay_cold = true;
            if message.role != ChatRole::Assistant || message.model_projection {
                cold_projected.push(cold);
                continue;
            }
            if !message.content.is_empty() {
                let content = serde_json::to_string(&serde_json::json!({
                    "vega_history_observation": {
                        "version": 1,
                        "authority": "none",
                        "kind": "assistant_text",
                        "text": message.content,
                    }
                }))
                .map_err(|_| invalid())?;
                let mut observation = ChatMessage::new(ChatRole::User, content);
                observation.model_projection = true;
                observation.replay_cold = true;
                cold_projected.push(observation);
            }
            if !message.tool_calls.is_empty() {
                let mut assistant = cold;
                assistant.content.clear();
                assistant.images.clear();
                assistant.reasoning_content = None;
                assistant.response_reasoning.clear();
                assistant.response_replay_layout = None;
                assistant.live_tool_calls = Some(vec![None; assistant.tool_calls.len()]);
                assistant.model_projection = false;
                cold_projected.push(assistant);
            }
        }
    } else {
        cold_projected.extend(messages.iter().cloned());
    }
    let mut historical = std::collections::HashMap::<
        String,
        std::collections::VecDeque<PendingHistoricalTool>,
    >::new();
    let mut projected = Vec::with_capacity(cold_projected.len());
    for (message_index, message) in cold_projected.iter().enumerate() {
        let historical_result = message.role == ChatRole::Tool
            && message.tool_call_id.as_ref().is_some_and(|id| {
                historical
                    .get(id)
                    .is_some_and(|pending| !pending.is_empty())
            });
        if message.model_projection && !historical_result {
            projected.push(message.clone());
            continue;
        }
        if message.role != ChatRole::Tool && !message.model_projection {
            historical.clear();
        }
        let mut next = message.without_live_tool_arguments();
        next.model_projection = true;
        if !message.tool_calls.is_empty() {
            if let Some(statuses) = &message.historical_tool_statuses
                && (statuses.len() != message.tool_calls.len()
                    || statuses
                        .iter()
                        .zip(&message.tool_calls)
                        .any(|(status, call)| status.id != call.id || status.name != call.name))
            {
                return Err(invalid());
            }
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
            if let Some(model_calls) = model_calls {
                next.tool_calls = model_calls;
                next.historical_tool_statuses = None;
                historical.clear();
            } else {
                let lost_live_binding =
                    message.live_tool_calls.is_some() && message.historical_tool_statuses.is_none();
                if lost_live_binding
                    && !completed_live_tool_group(&cold_projected, message_index, message)
                {
                    return Err(invalid());
                }
                next.tool_calls.clear();
                next.historical_tool_statuses = None;
                if lost_live_binding {
                    next.reasoning_content = None;
                    next.response_reasoning.clear();
                    next.response_replay_layout = None;
                }
                if !next.content.is_empty()
                    || !next.images.is_empty()
                    || (!lost_live_binding
                        && (next.reasoning_content.is_some()
                            || !next.response_reasoning.is_empty()
                            || next.response_replay_layout.is_some()))
                {
                    projected.push(next);
                }
                let calls = message
                    .tool_calls
                    .iter()
                    .enumerate()
                    .map(|(index, call)| {
                        let status = message
                            .historical_tool_statuses
                            .as_ref()
                            .and_then(|statuses| statuses[index].status)
                            .map(StoredToolStatus::as_str);
                        serde_json::json!({
                            "tool": call.name,
                            "call_id": call.id,
                            "arguments_available": false,
                            "status": status,
                        })
                    })
                    .collect::<Vec<_>>();
                let content = serde_json::to_string(&serde_json::json!({
                    "vega_history_observation": {
                        "version": 1,
                        "kind": "tool_activity",
                        "authority": "none",
                        "calls": calls,
                    }
                }))
                .map_err(|_| invalid())?;
                let mut activity = ChatMessage::new(ChatRole::User, content);
                activity.model_projection = true;
                activity.replay_cold = message.replay_cold;
                projected.push(activity);
                for (index, call) in message.tool_calls.iter().enumerate() {
                    let status = message
                        .historical_tool_statuses
                        .as_ref()
                        .and_then(|statuses| statuses[index].status);
                    historical.entry(call.id.clone()).or_default().push_back(
                        PendingHistoricalTool {
                            tool: call.name.clone(),
                            status,
                        },
                    );
                }
                continue;
            }
        } else if message.role == ChatRole::Tool
            && let Some(id) = &message.tool_call_id
            && let Some(pending) = historical
                .get_mut(id)
                .and_then(std::collections::VecDeque::pop_front)
        {
            let content = serde_json::to_string(&serde_json::json!({
                "vega_history_observation": {
                    "version": 1,
                    "kind": "tool_result",
                    "authority": "none",
                    "tool": pending.tool,
                    "call_id": id,
                    "status": pending.status.map(StoredToolStatus::as_str),
                    "output": message.content,
                }
            }))
            .map_err(|_| invalid())?;
            next.role = ChatRole::User;
            next.tool_call_id = None;
            next.content = content;
            next.reasoning_content = None;
            next.response_reasoning.clear();
            next.response_replay_layout = None;
            next.historical_tool_statuses = None;
        }
        projected.push(next);
    }
    if let Some(original_cold_boundary) = cold_boundary {
        let projected_boundary = projected
            .iter()
            .take_while(|message| message.replay_cold)
            .count();
        if projected_boundary == 0 {
            return Err(invalid());
        }
        if let Some(system) = projected.first_mut()
            && system.role == ChatRole::System
        {
            let rebased_user_boundary = interleaved_user_boundary
                .map(|boundary| {
                    boundary
                        .checked_sub(original_cold_boundary)
                        .and_then(|offset| projected_boundary.checked_add(offset))
                        .ok_or_else(invalid)
                })
                .transpose()?;
            system.update_replay_projection_boundaries(projected_boundary, rebased_user_boundary);
        }
    }
    Ok(projected)
}

fn completed_live_tool_group(
    messages: &[ChatMessage],
    assistant_index: usize,
    assistant: &ChatMessage,
) -> bool {
    if assistant.tool_calls.is_empty() {
        return false;
    }
    let mut pending = std::collections::HashSet::with_capacity(assistant.tool_calls.len());
    for call in &assistant.tool_calls {
        if !pending.insert(call.id.as_str()) {
            return false;
        }
    }
    for result in messages.iter().skip(assistant_index.saturating_add(1)) {
        if pending.is_empty() {
            return true;
        }
        if result.role != ChatRole::Tool
            || !result.tool_calls.is_empty()
            || result.reasoning_content.is_some()
            || !result.response_reasoning.is_empty()
            || result.response_replay_layout.is_some()
            || result.tool_call_id.is_none()
        {
            return false;
        }
        let Some(call_id) = result.tool_call_id.as_deref() else {
            return false;
        };
        if !pending.remove(call_id) {
            return false;
        }
    }
    pending.is_empty()
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
    ChatReasoningContent(ChatReasoningPresence),
    ResponseReplay(ResponseReplayLayout),
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
        cache_read: Option<u64>,
        /// Prompt tokens written to cache (when the API reports it).
        cache_write: Option<u64>,
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
            Self::ChatReasoningContent(presence) => formatter
                .debug_tuple("ChatReasoningContent")
                .field(presence)
                .finish(),
            Self::ResponseReplay(layout) => formatter
                .debug_tuple("ResponseReplay")
                .field(layout)
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
    fn automatic_replay_resident_size_includes_live_arguments_beyond_audit_projection() {
        let compact = bound_call("call", "write", "{}", "{}");
        let large_arguments = format!("{{\"content\":\"{}\"}}", "x".repeat(1_048_576));
        let large = bound_call("call", "write", "{}", &large_arguments);
        assert!(
            large.replay_resident_size_bytes() > compact.replay_resident_size_bytes() + 1_000_000
        );
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
    fn historical_observation_keeps_assistant_text_and_encodes_result_as_user_json() {
        let mut assistant = ChatMessage::assistant_with_tools(
            "I checked the file.",
            vec![ChatToolCall {
                id: "history-1".into(),
                name: "read".into(),
                input_json: "PRIVATE_PARAMETER".into(),
            }],
        );
        assistant
            .mark_stored_tool_calls_with_statuses(vec!["success".into()])
            .unwrap();
        let messages = vec![
            assistant,
            ChatMessage::tool_result("history-1", "first line\n\"quoted\" output"),
        ];
        let projected = project_model_messages(&messages).unwrap();
        assert_eq!(projected.len(), 3);
        assert_eq!(projected[0].role, ChatRole::Assistant);
        assert_eq!(projected[0].content, "I checked the file.");
        assert!(projected[0].tool_calls.is_empty());
        assert!(projected[0].reasoning_content.is_none());
        assert!(projected[0].response_reasoning.is_empty());
        assert_eq!(projected[1].role, ChatRole::User);
        assert!(projected[1].tool_calls.is_empty());
        assert!(projected[1].tool_call_id.is_none());
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&projected[1].content).unwrap(),
            serde_json::json!({
                "vega_history_observation": {
                    "version": 1,
                    "kind": "tool_activity",
                    "authority": "none",
                    "calls": [{
                        "tool": "read",
                        "call_id": "history-1",
                        "arguments_available": false,
                        "status": "success",
                    }],
                }
            })
        );
        assert_eq!(projected[2].role, ChatRole::User);
        assert!(projected[2].tool_calls.is_empty());
        assert!(projected[2].tool_call_id.is_none());
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&projected[2].content).unwrap(),
            serde_json::json!({
                "vega_history_observation": {
                    "version": 1,
                    "kind": "tool_result",
                    "authority": "none",
                    "tool": "read",
                    "call_id": "history-1",
                    "status": "success",
                    "output": "first line\n\"quoted\" output",
                }
            })
        );
        assert!(!projected.iter().any(|message| {
            message.content.contains("PRIVATE_PARAMETER")
                || message.content.contains("Historical tool activity")
                || message.content.contains("Historical tool result")
        }));
        assert_eq!(project_model_messages(&projected).unwrap(), projected);
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
        group.images.push(image.clone());
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
        assert_eq!(projected[0].content, "checking");
        assert_eq!(projected[0].images, vec![image.clone()]);
        assert_eq!(projected[1].role, ChatRole::User);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&projected[1].content).unwrap()["vega_history_observation"]
                ["calls"],
            serde_json::json!([
                {"tool":"read","call_id":"r1","arguments_available":false,"status":null},
                {"tool":"write","call_id":"bad","arguments_available":false,"status":null}
            ])
        );
        assert_eq!(projected[2].role, ChatRole::User);
        assert_eq!(projected[2].images, vec![image]);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&projected[2].content).unwrap()["vega_history_observation"]
                ["output"],
            "first result"
        );
        assert_eq!(projected[3].role, ChatRole::User);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&projected[3].content).unwrap()["vega_history_observation"]
                ["output"],
            "missing_content"
        );
        assert_eq!(projected[4].tool_calls[0].id, "r1");
        assert_eq!(
            projected[5],
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
            assert_eq!(projected[0].role, ChatRole::User);
            assert!(
                projected
                    .iter()
                    .all(|m| m.tool_calls.is_empty() && m.tool_call_id.is_none())
            );
            assert_eq!(projected[1].role, ChatRole::User);
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&projected[1].content).unwrap()["vega_history_observation"]
                    ["output"],
                "rejected"
            );
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
            assert_eq!(project_model_messages(&projected).unwrap(), projected);
        }
    }

    #[test]
    fn historical_observation_status_binding_is_atomic_and_identity_checked() {
        let mut message = ChatMessage::assistant_with_tools(
            "",
            vec![ChatToolCall {
                id: "stored".into(),
                name: "read".into(),
                input_json: "{}".into(),
            }],
        );
        assert!(matches!(
            message.mark_stored_tool_calls_with_statuses(vec!["running".into()]),
            Err(VegaError::Context(
                crate::ContextRuntimeError::InvalidProjection
            ))
        ));
        let unchanged = project_model_messages(std::slice::from_ref(&message)).unwrap();
        assert_eq!(unchanged[0].tool_calls.len(), 1);
        assert_eq!(unchanged[0].role, ChatRole::Assistant);
        message
            .mark_stored_tool_calls_with_statuses(vec!["cancelled".into()])
            .unwrap();
        let mut changed = message.clone();
        changed.tool_calls[0].name = "edit".into();
        assert!(matches!(
            project_model_messages(&[changed]),
            Err(VegaError::Context(
                crate::ContextRuntimeError::InvalidProjection
            ))
        ));
        let projected = project_model_messages(&[message]).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&projected[0].content).unwrap()["vega_history_observation"]
                ["calls"][0]["status"],
            "cancelled"
        );
        assert!(!format!("{:?}", projected[0]).contains("cancelled"));
    }

    #[test]
    fn replay_frame_marks_its_system_owner_as_cold() {
        let mut system = ChatMessage::new(ChatRole::System, "system");
        assert!(!system.replay_cold);
        system.attach_replay_frame(ReplayFrame::new(1, ReasoningReplayPolicy::Unknown));
        assert!(system.replay_cold);
        let projected = project_model_messages(&[system]).unwrap();
        assert_eq!(projected[0].replay_frame().unwrap().cold_prefix_len(), 1);
    }

    #[test]
    fn cold_replay_projection_rebases_private_boundary_after_opaque_turns() {
        let mut system = ChatMessage::new(ChatRole::System, "system");
        system.attach_replay_frame(ReplayFrame::new(6, ReasoningReplayPolicy::DeepSeekTools));
        let mut reasoning_only = ChatMessage::new(ChatRole::Assistant, "");
        reasoning_only.reasoning_content = Some("PRIVATE_CHAT_REASONING".into());
        let mut opaque_only = ChatMessage::new(ChatRole::Assistant, "");
        opaque_only
            .response_reasoning
            .push(serde_json::json!({"type":"reasoning","encrypted_content":"PRIVATE_OPAQUE"}));
        let mut live = ChatMessage::new(ChatRole::Assistant, "live final");
        live.reasoning_content = Some("LIVE_TOOL_TURN_REASONING".into());
        let messages = vec![
            system,
            ChatMessage::new(ChatRole::User, "previous user"),
            reasoning_only,
            opaque_only,
            ChatMessage::new(ChatRole::Assistant, "completed visible answer"),
            ChatMessage::new(ChatRole::User, "current user"),
            live,
        ];
        let projected = project_model_messages(&messages).unwrap();
        assert_eq!(projected.len(), 5);
        assert_eq!(projected[0].replay_frame().unwrap().cold_prefix_len(), 4);
        assert_eq!(projected[1].content, "previous user");
        assert_eq!(projected[2].role, ChatRole::User);
        assert_eq!(projected[3].content, "current user");
        assert_eq!(projected[4].content, "live final");
        assert_eq!(
            projected[4].reasoning_content.as_deref(),
            Some("LIVE_TOOL_TURN_REASONING")
        );
        assert!(projected.iter().all(|message| {
            !message.content.contains("PRIVATE_CHAT_REASONING")
                && !message.content.contains("PRIVATE_OPAQUE")
                && message.reasoning_content.as_deref() != Some("PRIVATE_CHAT_REASONING")
        }));
        assert_eq!(project_model_messages(&projected).unwrap(), projected);
    }

    #[test]
    fn interleaved_replay_rebases_user_range_after_cold_projection() {
        let mut system = ChatMessage::new(ChatRole::System, "system");
        let mut frame = ReplayFrame::new(6, ReasoningReplayPolicy::GlmInterleaved);
        frame.set_interleaved_user_boundary(8);
        system.attach_replay_frame(frame);
        let mut reasoning_only = ChatMessage::new(ChatRole::Assistant, "");
        reasoning_only.reasoning_content = Some("PRIVATE_COLD_REASONING".into());
        let mut opaque_only = ChatMessage::new(ChatRole::Assistant, "");
        opaque_only.response_reasoning.push(serde_json::json!({
            "type": "reasoning",
            "encrypted_content": "PRIVATE_COLD_OPAQUE"
        }));
        let mut warm_tool = ChatMessage::assistant_with_tools(
            "visible old-round work",
            vec![ChatToolCall {
                id: "warm-call".into(),
                name: "read".into(),
                input_json: r#"{"path":"source.txt"}"#.into(),
            }],
        );
        warm_tool.reasoning_content = Some("PRIVATE_OLD_ROUND_REASONING".into());
        let mut live = ChatMessage::new(ChatRole::Assistant, "new-round work");
        live.reasoning_content = Some("CURRENT_ROUND_REASONING".into());
        let projected = project_model_messages(&[
            system,
            ChatMessage::new(ChatRole::User, "cold user"),
            reasoning_only,
            opaque_only,
            ChatMessage::new(ChatRole::Assistant, "cold visible answer"),
            ChatMessage::new(ChatRole::User, "cold latest user"),
            warm_tool,
            ChatMessage::tool_result("warm-call", "visible tool result"),
            ChatMessage::new(ChatRole::User, "interleaved current user"),
            live,
        ])
        .unwrap();
        assert_eq!(projected[0].replay_frame().unwrap().cold_prefix_len(), 4);
        assert_eq!(
            projected[0]
                .replay_frame()
                .unwrap()
                .interleaved_user_boundary(),
            Some(6)
        );
        assert!(projected.iter().all(|message| {
            !message.content.contains("PRIVATE_COLD_REASONING")
                && !message.content.contains("PRIVATE_COLD_OPAQUE")
                && !message.content.contains("PRIVATE_OLD_ROUND_REASONING")
        }));
        assert_eq!(projected[4].content, "visible old-round work");
        assert_eq!(projected[4].reasoning_content, None);
        assert_eq!(projected[4].tool_calls[0].id, "warm-call");
        assert_eq!(projected[5].tool_call_id.as_deref(), Some("warm-call"));
        assert_eq!(projected[5].content, "visible tool result");
        assert_eq!(projected[6].content, "interleaved current user");
        assert_eq!(
            projected[7].reasoning_content.as_deref(),
            Some("CURRENT_ROUND_REASONING")
        );
        assert_eq!(project_model_messages(&projected).unwrap(), projected);
    }

    #[test]
    fn historical_observation_preserves_each_terminal_status() {
        let statuses = ["success", "failed", "rejected", "cancelled"];
        let mut assistant = ChatMessage::assistant_with_tools(
            "completed earlier work",
            statuses
                .iter()
                .enumerate()
                .map(|(index, _)| ChatToolCall {
                    id: format!("stored-{index}"),
                    name: "read".into(),
                    input_json: format!("PRIVATE_{index}"),
                })
                .collect(),
        );
        let mut equal_without_status = assistant.clone();
        equal_without_status.mark_stored_tool_calls();
        assistant
            .mark_stored_tool_calls_with_statuses(
                statuses
                    .iter()
                    .map(|status| (*status).to_string())
                    .collect(),
            )
            .unwrap();
        assert_eq!(assistant, equal_without_status);
        let messages = std::iter::once(assistant)
            .chain((0..statuses.len()).map(|index| {
                ChatMessage::tool_result(format!("stored-{index}"), format!("output-{index}"))
            }))
            .collect::<Vec<_>>();
        let projected = project_model_messages(&messages).unwrap();
        assert_eq!(projected.len(), statuses.len() + 2);
        let activity: serde_json::Value = serde_json::from_str(&projected[1].content).unwrap();
        assert_eq!(activity["vega_history_observation"]["authority"], "none");
        assert_eq!(
            activity["vega_history_observation"]["calls"]
                .as_array()
                .unwrap()
                .iter()
                .map(|call| call["status"].as_str().unwrap())
                .collect::<Vec<_>>(),
            statuses.to_vec()
        );
        for (index, status) in statuses.iter().enumerate() {
            let result: serde_json::Value =
                serde_json::from_str(&projected[index + 2].content).unwrap();
            assert_eq!(result["vega_history_observation"]["kind"], "tool_result");
            assert_eq!(result["vega_history_observation"]["authority"], "none");
            assert_eq!(
                result["vega_history_observation"]["call_id"],
                format!("stored-{index}")
            );
            assert_eq!(result["vega_history_observation"]["status"], *status);
            assert_eq!(
                result["vega_history_observation"]["output"],
                format!("output-{index}")
            );
        }
        assert!(
            projected
                .iter()
                .all(|message| { message.tool_calls.is_empty() && message.tool_call_id.is_none() })
        );
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
