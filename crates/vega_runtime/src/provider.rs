use std::fmt;
use std::pin::Pin;

use futures::Stream;
use futures::future::BoxFuture;
use tokio_util::sync::CancellationToken;

use crate::error::VegaError;

/// Protocol family used to encode a frozen thinking selection.
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
    #[default]
    Unknown,
}

impl ReasoningReplayPolicy {
    pub fn preserves_chat_response(self, tools_requested: bool) -> bool {
        match self {
            Self::DeepSeekTools | Self::StandardChat | Self::Unknown => tools_requested,
            Self::GlmPreserved | Self::GlmInterleaved | Self::ResponsesItems => true,
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
    Resolved,
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
    pub compatibility: Option<Box<crate::ChatCompatibility>>,
    /// Exact configured provider identifier.
    pub provider: String,
    /// Exact configured model identifier.
    pub model: String,
    /// Explicit protocol capability declaration.
    pub protocol: ReasoningProtocol,
    pub replay_policy: ReasoningReplayPolicy,
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
            .field(
                "compatibility",
                &self.compatibility.as_ref().map(|facts| &facts.profile_id),
            )
            .finish()
    }
}

impl FrozenReasoning {
    /// Safe profile for an unrecognised provider/model pair. The wire encoder
    /// omits all thinking controls for this profile.
    pub fn unknown(provider: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            compatibility: None,
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
        if let Some(facts) = &self.compatibility {
            if self.protocol != facts.protocol
                || self.replay_policy != facts.replay_policy
                || self.declared_efforts != facts.efforts
                || self.supports_disabled != facts.supports_disabled
                || self.disabled_wire != facts.disabled_wire
                || !facts.supports_choice(&self.choice)
            {
                return Err(VegaError::ReasoningSelectionInvalid {
                    message: "thinking choice does not match the resolved model capabilities"
                        .into(),
                });
            }
            return Ok(());
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
                || self.supports_disabled
                || self.disabled_wire.is_some()
                || !self.declared_efforts.is_empty())
        {
            return Err(VegaError::ReasoningSelectionInvalid {
                message:
                    "unknown reasoning protocol cannot declare controls or a specialized replay policy"
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
            ReasoningChoice::Enabled => Err(VegaError::ReasoningSelectionInvalid {
                message: "enabled is not declared by the provider/model profile".into(),
            }),
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReasoningChoice {
    Enabled,
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
    /// No HTTP response was received: DNS, TCP/TLS or redirect failure while
    /// establishing the request. Only this kind justifies network/address
    /// advice.
    Transport,
    /// An established response stream ended before its terminal event.
    StreamInterrupted,
    /// An established response stream or request exceeded its idle/request
    /// deadline.
    Timeout,
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
    pub chat_reasoning: Option<crate::ChatRawState>,
    live_wire_ids: bool,
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
    runtime_guidance: Option<RuntimeGuidance>,
    tool_context: Option<Box<crate::tool_context::ToolContext>>,
    model_projection: bool,
    replay_cold: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RuntimeGuidance {
    Progress,
    Recovery(RecoveryGuidance),
    ProgressAndRecovery(RecoveryGuidance),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RecoveryGuidance {
    RepeatedFailure,
    RepeatedResult,
    Review,
}

impl RecoveryGuidance {
    fn text(self) -> &'static str {
        match self {
            Self::RepeatedFailure => {
                "The same tool inputs produced the same unsuccessful evidence in three separate batches. Check the tool contract and current state, then make a substantive change to the inputs or approach. Respect permission decisions and verify uncertain side effects before any retry. Continue the authorized task when possible."
            }
            Self::RepeatedResult => {
                "The same tool inputs produced the same successful result in three separate batches. Check whether this is a legitimate wait that needs backoff, or whether a different query would advance the task. Repeated observations do not prove the task has failed. Continue useful work."
            }
            Self::Review => {
                "Review the confirmed facts, the key unknown, and how the next step will verify a success criterion. If the evidence is sufficient, proceed to implementation or conclusions; otherwise continue the necessary investigation. This is a strategy check, not a stopping condition."
            }
        }
    }
}

impl RuntimeGuidance {
    fn text(self) -> String {
        let mut text = "\n\n<vega_runtime_notice>".to_string();
        if matches!(self, Self::Progress | Self::ProgressAndRecovery(_)) {
            text.push_str("Give the user a brief factual progress update and the next step, then continue the authorized task. If waiting, identify the actual waiting condition. Do not invent progress, expose private reasoning, or end the task merely to report. ");
        }
        if let Self::Recovery(recovery) | Self::ProgressAndRecovery(recovery) = self {
            text.push_str(recovery.text());
            text.push(' ');
        }
        text.push_str("This reminder grants no additional permission.</vega_runtime_notice>");
        text
    }
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

struct PendingToolContext {
    source: Option<crate::tool_context::ToolIdentity>,
    wire: Option<crate::tool_context::ToolIdentity>,
    computer_use: bool,
}

impl PartialEq for ChatMessage {
    fn eq(&self, other: &Self) -> bool {
        self.images == other.images
            && self.role == other.role
            && self.content == other.content
            && self.tool_call_id == other.tool_call_id
            && self.tool_calls == other.tool_calls
            && self.reasoning_content == other.reasoning_content
            && self.chat_reasoning == other.chat_reasoning
            && self.live_wire_ids == other.live_wire_ids
            && self.response_reasoning == other.response_reasoning
            && self.response_replay_layout == other.response_replay_layout
            && self.replay_frame == other.replay_frame
            && self.live_tool_calls == other.live_tool_calls
            && self.runtime_guidance == other.runtime_guidance
            && self.tool_context == other.tool_context
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
            .field("chat_reasoning", &self.chat_reasoning)
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
            .saturating_add(self.tool_context.as_ref().map_or(0, |_| {
                std::mem::size_of::<crate::tool_context::ToolContext>()
            }))
            .saturating_add(self.tool_call_id.as_ref().map_or(0, string_size))
            .saturating_add(self.reasoning_content.as_ref().map_or(0, string_size))
            .saturating_add(self.chat_reasoning.as_ref().map_or(0, |raw| {
                raw.text
                    .as_ref()
                    .map_or(0, |(_, text)| text.capacity())
                    .saturating_add(raw.details.as_ref().map_or(0, |details| {
                        details
                            .capacity()
                            .saturating_mul(std::mem::size_of::<serde_json::Value>())
                            .saturating_add(details.iter().fold(0usize, |total, value| {
                                total.saturating_add(value_heap_size(value))
                            }))
                    }))
            }))
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
        self.live_wire_ids = true;
        self.historical_tool_statuses = None;
        self.model_projection = false;
        Ok(())
    }

    pub fn without_live_tool_arguments(&self) -> Self {
        let mut message = self.clone();
        message.live_tool_calls = None;
        message.live_wire_ids = false;
        message.model_projection = false;
        message
    }

    pub(crate) fn preserves_live_wire_ids(&self) -> bool {
        self.live_wire_ids
    }

    pub fn without_runtime_guidance(&self) -> Self {
        let mut message = self.clone();
        message.runtime_guidance = None;
        message
    }

    pub fn without_runtime_metadata(&self) -> Self {
        let mut message = self.without_runtime_guidance();
        message.tool_context = None;
        message
    }

    pub(crate) fn attach_progress_reminder(&mut self) -> bool {
        if self.role != ChatRole::Tool || self.model_projection {
            return false;
        }
        self.runtime_guidance = Some(match self.runtime_guidance {
            None => RuntimeGuidance::Progress,
            Some(RuntimeGuidance::Recovery(recovery)) => {
                RuntimeGuidance::ProgressAndRecovery(recovery)
            }
            Some(_) => return false,
        });
        true
    }

    pub(crate) fn attach_recovery_guidance(&mut self, recovery: RecoveryGuidance) -> bool {
        if self.role != ChatRole::Tool || self.model_projection {
            return false;
        }
        self.runtime_guidance = Some(match self.runtime_guidance {
            None => RuntimeGuidance::Recovery(recovery),
            Some(RuntimeGuidance::Progress) => RuntimeGuidance::ProgressAndRecovery(recovery),
            Some(_) => return false,
        });
        true
    }

    pub fn mark_stored_tool_calls(&mut self) {
        self.live_wire_ids = false;
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
        self.live_wire_ids = false;
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
            chat_reasoning: None,
            live_wire_ids: false,
            reasoning_content: None,
            response_reasoning: Vec::new(),
            response_replay_layout: None,
            replay_frame: None,
            live_tool_calls: None,
            historical_tool_statuses: None,
            runtime_guidance: None,
            tool_context: None,
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
            chat_reasoning: None,
            live_wire_ids: false,
            reasoning_content: None,
            response_reasoning: Vec::new(),
            response_replay_layout: None,
            replay_frame: None,
            live_tool_calls: None,
            historical_tool_statuses: None,
            runtime_guidance: None,
            tool_context: None,
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
            chat_reasoning: None,
            live_wire_ids: false,
            reasoning_content,
            response_reasoning: Vec::new(),
            response_replay_layout: None,
            replay_frame: None,
            live_tool_calls: None,
            historical_tool_statuses: None,
            runtime_guidance: None,
            tool_context: None,
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

    pub(crate) fn runtime_tool_result(
        call: &crate::RuntimeToolCall,
        result: &crate::RuntimeToolResult,
    ) -> Self {
        let mut message = Self::projected_tool_result(&call.id, &call.name, &result.output);
        message.tool_context =
            crate::tool_context::ToolContext::new(call, result, &message.content).map(Box::new);
        message
    }

    /// Builds a tool-result message associated with `call_id`.
    pub fn tool_result(call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            images: Vec::new(),
            role: ChatRole::Tool,
            content: content.into(),
            tool_call_id: Some(call_id.into()),
            tool_calls: Vec::new(),
            chat_reasoning: None,
            live_wire_ids: false,
            reasoning_content: None,
            response_reasoning: Vec::new(),
            response_replay_layout: None,
            replay_frame: None,
            live_tool_calls: None,
            historical_tool_statuses: None,
            runtime_guidance: None,
            tool_context: None,
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
                current.chat_reasoning = None;
            }
            if index >= frame.cold_prefix_len {
                cold_projected.push(current);
                continue;
            }
            let mut cold = current;
            cold.replay_cold = true;
            cold.live_wire_ids = false;
            cold.reasoning_content = None;
            cold.chat_reasoning = None;
            cold.response_reasoning.clear();
            cold.response_replay_layout = None;
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
                assistant.chat_reasoning = None;
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
    let mut tool_context = crate::tool_context::ToolContextProjector::default();
    let mut tool_identities =
        std::collections::HashMap::<String, std::collections::VecDeque<PendingToolContext>>::new();
    for (message_index, message) in cold_projected.iter().enumerate() {
        if message.role != ChatRole::Tool {
            tool_identities.clear();
            for (index, call) in message.tool_calls.iter().enumerate() {
                let source =
                    crate::tool_context::ToolIdentity::new(&call.id, &call.name, &call.input_json);
                let wire = if let Some(live) = &message.live_tool_calls {
                    live.get(index).and_then(Option::as_ref).and_then(|live| {
                        crate::tool_context::ToolIdentity::new(
                            &live.model.id,
                            &live.model.name,
                            &live.model.input_json,
                        )
                    })
                } else {
                    source
                };
                tool_identities
                    .entry(call.id.clone())
                    .or_default()
                    .push_back(PendingToolContext {
                        source,
                        wire,
                        computer_use: crate::computer_use::ComputerUseRun::tool_capability(
                            &call.name,
                        )
                        .is_some(),
                    });
            }
        }
        let historical_result = message.role == ChatRole::Tool
            && message.tool_call_id.as_ref().is_some_and(|id| {
                historical
                    .get(id)
                    .is_some_and(|pending| !pending.is_empty())
            });
        let tool_identity = if message.role == ChatRole::Tool {
            message
                .tool_call_id
                .as_ref()
                .and_then(|id| tool_identities.get_mut(id))
                .and_then(std::collections::VecDeque::pop_front)
        } else {
            None
        };
        if message.model_projection && !historical_result {
            if message.role == ChatRole::Tool
                && !message.replay_cold
                && message.images.is_empty()
                && let Some(id) = &message.tool_call_id
            {
                tool_context.observe_projected(
                    id,
                    &message.content,
                    tool_identity.as_ref().and_then(|identity| identity.wire),
                    message.tool_context.as_deref().copied(),
                );
            }
            projected.push(message.clone());
            continue;
        }
        if message.role != ChatRole::Tool && !message.model_projection {
            historical.clear();
        }
        let mut next = message.without_live_tool_arguments();
        next.model_projection = true;
        next.live_wire_ids = message.live_wire_ids;
        if message.role == ChatRole::Tool
            && message.images.is_empty()
            && let Some(id) = &message.tool_call_id
        {
            let (content, metadata) = tool_context.project(
                id,
                &message.content,
                tool_identity.as_ref().and_then(|identity| identity.source),
                message.tool_context.as_deref().copied(),
                message.replay_cold,
            );
            next.content = content;
            next.tool_context = metadata.map(Box::new);
        }
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
                    next.chat_reasoning = None;
                    next.response_reasoning.clear();
                    next.response_replay_layout = None;
                }
                if !next.content.is_empty()
                    || !next.images.is_empty()
                    || (!lost_live_binding
                        && ((next.reasoning_content.is_some() || next.chat_reasoning.is_some())
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
            let mut observation = serde_json::json!({
                "vega_history_observation": {
                    "version": 1,
                    "kind": "tool_result",
                    "authority": "none",
                    "tool": pending.tool,
                    "call_id": id,
                    "status": pending.status.map(StoredToolStatus::as_str),
                    "output": next.content,
                }
            });
            if let Some(guidance) = next.runtime_guidance.take() {
                observation["vega_history_observation"]["advisory"] =
                    serde_json::Value::String(guidance.text());
            }
            let content = serde_json::to_string(&observation).map_err(|_| invalid())?;
            next.role = ChatRole::User;
            next.tool_call_id = None;
            next.content = content;
            next.reasoning_content = None;
            next.chat_reasoning = None;
            next.response_reasoning.clear();
            next.response_replay_layout = None;
            next.historical_tool_statuses = None;
            next.tool_context = None;
        }
        if let Some(guidance) = next.runtime_guidance.take()
            && next.role == ChatRole::Tool
            && !tool_identity
                .as_ref()
                .is_some_and(|identity| identity.computer_use)
        {
            next.content.push_str(&guidance.text());
        }
        if next.role == ChatRole::Tool
            && let Some(context) = &mut next.tool_context
        {
            context.seal_projection(
                &next.content,
                tool_identity.and_then(|identity| identity.wire),
            );
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
            || (result.reasoning_content.is_some() || result.chat_reasoning.is_some())
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolChoice {
    Auto,
    None,
    Required,
    NamedFunction(String),
}

/// A chat completion request (tech-spec §4.1):
/// model / messages / tools / max_tokens.
#[derive(Clone, Default, PartialEq)]
pub struct ChatRequest {
    pub model_snapshot: Option<Box<crate::model_compat::ResolvedModel>>,
    pub tool_choice: Option<ToolChoice>,
    /// Model name as understood by the provider, e.g. `deepseek-chat`.
    pub model: String,
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
    ChatReasoningState(crate::ChatRawState),
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
            Self::ChatReasoningState(raw) => formatter
                .debug_tuple("ChatReasoningState")
                .field(raw)
                .finish(),
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
    fn model_snapshot(&self) -> Option<crate::model_compat::ResolvedModel> {
        None
    }

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
