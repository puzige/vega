//! Deterministic context-budget accounting and the headless compaction seam.
//!
//! This module deliberately owns no persistence or UI code. Conversation
//! orchestration supplies a [`ContextCompactionHook`] when a configured
//! conversation can safely build and install a durable projection.

use std::fmt;
use std::sync::Arc;

use futures::future::BoxFuture;
use tokio_util::sync::CancellationToken;

use crate::{ChatMessage, ToolDefinition, VegaError};

/// Version stamped on every budget estimate and durable compaction checkpoint.
pub const CONTEXT_ESTIMATOR_VERSION: &str = "vega-context-estimator-v3";

/// A fixed protocol framing charge applied to every request.
pub const CONTEXT_PROTOCOL_OVERHEAD_TOKENS: u64 = 32;

/// A fixed safety allowance for provider framing and tokenizer variance.
pub const CONTEXT_SAFETY_ALLOWANCE_TOKENS: u64 = 256;

/// Maximum user-configurable total context limit. The bound keeps arithmetic
/// and provider `max_tokens` conversions finite while allowing large models.
pub const MAX_CONTEXT_LIMIT: u64 = u32::MAX as u64;

/// Maximum bytes accepted by the deterministic text approximation per value.
const MAX_ESTIMATED_BYTES: usize = 64 * 1024 * 1024;

/// A persisted total context limit and output reserve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextBudget {
    /// Total model context capacity, in estimated tokens.
    total_limit: u64,
    /// Output capacity reserved for the next provider response.
    output_reserve: u64,
    /// Whether automatic compaction may run before a primary request.
    automatic_compaction: bool,
    policy_input_limit: u64,
}

impl ContextBudget {
    /// Builds a budget after validating the user-editable integer fields.
    pub fn new(
        total_limit: u64,
        output_reserve: u64,
        automatic_compaction: bool,
    ) -> Result<Self, ContextEstimateError> {
        if total_limit == 0 || total_limit > MAX_CONTEXT_LIMIT {
            return Err(ContextEstimateError::InvalidBudget);
        }
        if output_reserve == 0 || output_reserve >= total_limit || output_reserve > u32::MAX as u64
        {
            return Err(ContextEstimateError::InvalidBudget);
        }
        Ok(Self {
            total_limit,
            output_reserve,
            automatic_compaction,
            policy_input_limit: total_limit - output_reserve,
        })
    }

    pub fn with_model_limits(
        mut self,
        context_window: Option<u64>,
        max_output_tokens: Option<u32>,
    ) -> Result<Self, ContextEstimateError> {
        if let Some(max_output) = max_output_tokens.filter(|value| *value > 0) {
            let input_limit = self.input_budget();
            self.output_reserve = self.output_reserve.min(u64::from(max_output));
            self.total_limit = input_limit
                .checked_add(self.output_reserve)
                .ok_or(ContextEstimateError::Overflow)?;
        }
        if let Some(window) = context_window.filter(|value| *value > 0) {
            self.total_limit = self.total_limit.min(window);
            if self.output_reserve >= self.total_limit {
                return Err(ContextEstimateError::InvalidBudget);
            }
        }
        self.policy_input_limit = self.policy_input_limit.min(self.input_budget());
        Ok(self)
    }

    pub fn with_policy_input_limit(
        mut self,
        input_limit: u64,
    ) -> Result<Self, ContextEstimateError> {
        if input_limit == 0 || input_limit > self.input_budget() {
            return Err(ContextEstimateError::InvalidBudget);
        }
        self.policy_input_limit = input_limit;
        Ok(self)
    }

    pub fn policy_input_limit(self) -> u64 {
        self.policy_input_limit
    }

    pub fn summary_output_reserve(self) -> u64 {
        self.output_reserve.min(20_000)
    }

    /// Total configured context capacity.
    pub fn total_limit(self) -> u64 {
        self.total_limit
    }

    /// Output capacity reserved for the next provider response.
    pub fn output_reserve(self) -> u64 {
        self.output_reserve
    }

    /// Whether automatic compaction is enabled.
    pub fn automatic_compaction(self) -> bool {
        self.automatic_compaction
    }

    pub fn without_automatic_compaction(mut self) -> Self {
        self.automatic_compaction = false;
        self
    }

    /// Input budget after reserving response capacity.
    pub fn input_budget(self) -> u64 {
        self.total_limit - self.output_reserve
    }

    pub fn trigger_tokens(self) -> Result<u64, ContextEstimateError> {
        let buffer = floor_ratio(self.policy_input_limit, 1, 5)?.min(13_000);
        Ok(self.policy_input_limit - buffer)
    }

    pub fn precompute_tokens(self) -> Result<u64, ContextEstimateError> {
        Ok(floor_ratio(self.policy_input_limit, 4, 5)?.min(self.trigger_tokens()?))
    }

    pub fn warning_tokens(self) -> Result<u64, ContextEstimateError> {
        let threshold = if self.automatic_compaction {
            self.trigger_tokens()?
        } else {
            self.policy_input_limit
        };
        Ok(threshold.saturating_sub(20_000))
    }

    pub fn blocking_tokens(self) -> Result<u64, ContextEstimateError> {
        let buffer = floor_ratio(self.input_budget(), 1, 20)?.min(3_000);
        Ok(self.input_budget() - buffer)
    }

    pub fn target_tokens(self) -> Result<u64, ContextEstimateError> {
        Ok(self
            .trigger_tokens()?
            .min(self.blocking_tokens()?)
            .saturating_sub(1))
    }

    /// Classifies one estimate against this budget before a primary request.
    pub fn check(self, estimate: ContextEstimate) -> Result<ContextCheck, ContextEstimateError> {
        if estimate.input_tokens >= self.blocking_tokens()? {
            return Ok(ContextCheck::OverLimit);
        }
        if self.automatic_compaction && estimate.input_tokens >= self.trigger_tokens()? {
            return Ok(ContextCheck::Triggered);
        }
        Ok(ContextCheck::Within)
    }
}

/// The result of a deterministic request estimate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextEstimate {
    /// Approximate input tokens. This is not provider-exact usage.
    pub input_tokens: u64,
    /// Number of message values included in the estimate.
    pub message_count: usize,
    /// Number of tool schemas included in the estimate.
    pub tool_count: usize,
    /// Number of image contributions included in the estimate.
    pub image_count: usize,
}

/// Budget state before a primary provider request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextCheck {
    /// No configured total limit; callers must retain legacy send behavior.
    Unconfigured,
    /// Estimate is below the automatic trigger.
    Within,
    /// Estimate reached the automatic trigger but still fits the input cap.
    Triggered,
    /// Estimate cannot be sent under the configured input budget.
    OverLimit,
}

/// Content-free deterministic-estimator failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ContextEstimateError {
    /// User fields did not satisfy the positive/bounded integer contract.
    #[error("context budget is invalid")]
    InvalidBudget,
    /// Estimator arithmetic or provider conversion overflowed.
    #[error("context estimate exceeded safe arithmetic bounds")]
    Overflow,
    /// A value exceeded the bounded estimator input contract.
    #[error("context estimate input is too large")]
    InputTooLarge,
}

/// Content-free failures raised while enforcing a configured budget in the
/// live agent loop.  These metadata-only variants are safe to surface to the
/// conversation layer and never contain prompt or provider payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ContextRuntimeError {
    /// Deterministic request accounting failed before any provider call.
    #[error(transparent)]
    Estimate(#[from] ContextEstimateError),
    /// The assembled request cannot fit the configured input budget.
    #[error("context estimate {estimated_tokens} exceeds input budget {input_budget}")]
    OverLimit {
        /// Deterministic estimated input size.
        estimated_tokens: u64,
        /// Total limit after reserving output capacity.
        input_budget: u64,
    },
    /// A configured budget requested compaction but no conversation hook was
    /// supplied by the caller.
    #[error("context compaction is configured but unavailable")]
    MissingHook,
    /// A bounded summary stage cannot fit its own provider input budget.
    #[error(
        "context summary input estimate {estimated_tokens} exceeds input budget {input_budget}"
    )]
    SummaryInputOverLimit {
        /// Deterministic estimate of the summary request input.
        estimated_tokens: u64,
        /// Total capacity after reserving output for that summary response.
        input_budget: u64,
    },
    /// The hook returned a projection that still cannot fit.
    #[error("context compaction result {estimated_tokens} exceeds target/input budget")]
    ResultOverLimit {
        /// Deterministic estimate of the returned projection.
        estimated_tokens: u64,
        /// Target selected by the budget.
        target_tokens: u64,
    },
    /// The hook returned a projection containing a system message.  System
    /// authority remains owned by the loop and is never accepted from a
    /// historical summary.
    #[error("context compaction result contains a system message")]
    SystemMessageInResult,
    /// A second attempt for the same durable source was requested.
    #[error("context compaction already attempted for this source")]
    AlreadyAttempted,
    /// No complete older group can be summarized while retaining the newest
    /// user turn.
    #[error("context has no compactable complete prefix")]
    NoCompactablePrefix,
    /// The summary response was empty, truncated, malformed, timed out, or
    /// otherwise unusable.
    #[error("context summary response was invalid")]
    InvalidSummary,
    #[error("context summary response was empty")]
    SummaryEmpty,
    #[error("context summary framing or format was invalid")]
    SummaryFormatInvalid,
    #[error("generated context summary projection was invalid")]
    SummaryProjectionInvalid,
    /// Historical source or reconstructed projection failed structural validation.
    #[error("context source or projection was invalid")]
    InvalidProjection,
    /// The summary provider exhausted its output allowance. Only byte/token
    /// counts are retained; neither visible nor reasoning content is exposed.
    #[error(
        "context summary output truncated (visible {visible_bytes} bytes, thinking {thinking_bytes} bytes)"
    )]
    SummaryOutputTruncated {
        /// Visible text bytes received before the terminal Length event.
        visible_bytes: usize,
        /// Thinking bytes received before the terminal Length event.
        thinking_bytes: usize,
        /// Provider-reported output tokens, when supplied.
        output_tokens: Option<u64>,
    },
    /// The source fence changed before the asynchronous result could commit.
    #[error("context source changed while compaction was running")]
    SourceChanged,
    /// The bounded source could not be safely represented to the summarizer.
    #[error("context summary source exceeds the bounded compaction plan")]
    SourceTooLarge,
    /// Ordered segment summaries plus any predecessor exceed the aggregate byte ceiling.
    #[error("context summary aggregate exceeds the bounded plan")]
    AggregateTooLarge,
    /// The compactable prefix contains images whose provider-safe multimodal
    /// summary representation is unavailable.
    #[error("context summary cannot safely represent historical images")]
    ImagesUnsupported,
    /// The summary provider did not finish within the fixed operation bound.
    #[error("context summary timed out")]
    SummaryTimedOut,
}

impl fmt::Display for ContextEstimate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} estimated input tokens", self.input_tokens)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextCompactionTrigger {
    Threshold,
    Manual,
    HardLimit,
    InputOverflow {
        token_gap: Option<u64>,
        deeper: bool,
    },
}

/// Request handed to a conversation-owned compaction implementation.
#[derive(Clone)]
pub struct ContextCompactionRequest {
    pub trigger: ContextCompactionTrigger,
    /// System prompt is supplied for bounded summary planning; the hook must
    /// never install it as historical data.
    pub system_prompt: String,
    /// Current provider projection, oldest to newest.
    pub messages: Vec<ChatMessage>,
    /// Exact tool schemas on the primary wire request. Summary calls never
    /// receive these tools, but the hook uses them when checking the final
    /// primary projection against the same budget.
    pub tools: Vec<ToolDefinition>,
    /// Frozen total/reserve policy used to validate both the summary request
    /// and the returned primary projection.
    pub budget: ContextBudget,
    /// Estimate that caused this request.
    pub estimate: ContextEstimate,
    /// Desired post-compaction estimate.
    pub target_tokens: u64,
    /// Monotonic source version owned by the caller.
    pub source_version: u64,
    /// Optional durable source fingerprint. A revision number alone cannot
    /// detect same-sequence streaming-content mutations.
    pub source_fingerprint: Option<String>,
    /// Manual callers require the hook's initial source snapshot to match
    /// this fence exactly.  The live tool-loop path leaves this false because
    /// persisted tool output may legitimately advance after the run-start
    /// projection; the final checkpoint CAS still rejects a stale install.
    pub require_source_fence: bool,
    /// Durable assistant row owned by the live run, when known. Automatic
    /// compaction may ignore mutations attached to this still-streaming row
    /// (text/tool persistence), but unrelated history changes must fail the
    /// source fence before another provider request.
    pub source_owner_id: Option<String>,
}

impl fmt::Debug for ContextCompactionRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContextCompactionRequest")
            .field("trigger", &self.trigger)
            .field("message_count", &self.messages.len())
            .field("estimate", &self.estimate)
            .field("budget", &self.budget)
            .field("target_tokens", &self.target_tokens)
            .field("source_version", &self.source_version)
            .field(
                "source_fingerprint_bytes",
                &self.source_fingerprint.as_ref().map(String::len),
            )
            .field("require_source_fence", &self.require_source_fence)
            .field(
                "source_owner_id_bytes",
                &self.source_owner_id.as_ref().map(String::len),
            )
            .finish()
    }
}

pub fn validate_compaction_replay(
    messages: &[ChatMessage],
    cold_prefix: usize,
    user_boundary: Option<usize>,
    interleaved: bool,
) -> bool {
    if cold_prefix == 0 || cold_prefix > messages.len() {
        return false;
    }
    if !interleaved {
        return true;
    }
    let Some(boundary) = user_boundary else {
        return false;
    };
    if boundary < cold_prefix
        || boundary > messages.len()
        || (boundary == messages.len() && cold_prefix != messages.len())
        || messages
            .get(boundary)
            .is_some_and(|message| message.role != crate::ChatRole::User)
    {
        return false;
    }
    let mut pending = std::collections::HashSet::new();
    for message in &messages[..boundary] {
        match message.role {
            crate::ChatRole::Assistant => {
                if !pending.is_empty() {
                    return false;
                }
                for call in &message.tool_calls {
                    if !pending.insert(call.id.as_str()) {
                        return false;
                    }
                }
            }
            crate::ChatRole::Tool => {
                let Some(call_id) = message.tool_call_id.as_deref() else {
                    return false;
                };
                if !pending.remove(call_id) {
                    return false;
                }
            }
            crate::ChatRole::User if !pending.is_empty() => return false,
            crate::ChatRole::System => return false,
            crate::ChatRole::User => {}
        }
    }
    pending.is_empty()
}

/// A validated provider projection returned by a compaction hook.
#[derive(Clone)]
pub struct ContextCompactionResult {
    /// Summary-injected provider history, still below system authority.
    pub messages: Vec<ChatMessage>,
    /// Durable source watermark corresponding to the summary.
    pub source_version: u64,
    /// Fingerprint of the source projection used for this result.
    pub source_fingerprint: Option<String>,
    pub replay_cold_prefix_messages: Option<usize>,
    pub replay_interleaved_user_boundary: Option<usize>,
    /// Actual usage from each completed summary stage, in request order.
    pub usages: Vec<ContextCompactionUsage>,
    /// False when any stage omitted Usage; known stages still remain billable.
    pub usage_complete: bool,
}

impl fmt::Debug for ContextCompactionResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContextCompactionResult")
            .field("message_count", &self.messages.len())
            .field("source_version", &self.source_version)
            .field(
                "source_fingerprint_bytes",
                &self.source_fingerprint.as_ref().map(String::len),
            )
            .field(
                "replay_cold_prefix_messages",
                &self.replay_cold_prefix_messages,
            )
            .field(
                "replay_interleaved_user_boundary",
                &self.replay_interleaved_user_boundary,
            )
            .field("usage_count", &self.usages.len())
            .field("usage_complete", &self.usage_complete)
            .finish()
    }
}

/// Conversation's bounded, cancellable compaction boundary.
pub trait ContextCompactionHook: Send + Sync {
    /// Builds a durable projection without mutating runtime messages itself.
    fn compact<'a>(
        &'a self,
        request: ContextCompactionRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'a, Result<ContextCompactionResult, ContextCompactionFailure>>;
}

/// Actual usage from a summary provider call, with the same frozen pricing
/// provenance used by primary requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextCompactionUsage {
    /// Provider-reported token counts.
    pub usage: crate::RuntimeTokenUsage,
    /// Checked cost from the frozen catalog, or the legacy zero placeholder
    /// when no matching profile exists.
    pub cost_microcents: i64,
    /// Pricing provenance; `None` means the usage was unpriced/unknown.
    pub pricing: Option<crate::RuntimeUsagePricing>,
}

/// Runtime-local lifecycle phase for a compaction status event.  Conversation
/// converts this closed vocabulary into its UI-safe status record; no runtime
/// event carries transcript or summary text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextCompactionPhase {
    /// The bounded summary operation started.
    Started,
    /// A checkpoint was installed and the projection passed validation.
    Succeeded,
    /// The operation failed without installing a replacement projection.
    Failed,
    /// Cancellation was observed at the compaction boundary.
    Cancelled,
}

/// Accounting state for the summary provider call.  `Unknown` is distinct
/// from a priced zero: providers are allowed to omit Usage, and consumers
/// must then keep the run's cost display unknown instead of fabricating a
/// zero-cost row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextCompactionUsageState {
    /// The summary request has started but has not reached its terminal state.
    Pending,
    /// A Usage event was received.  `priced` records whether the frozen
    /// catalog supplied provenance for it.
    Known { priced: bool },
    /// The summary request ended without a Usage event.
    Unknown,
}

/// Closed failure vocabulary for a compaction status event.  Provider/store
/// diagnostics stay in the typed error path and never enter UI status text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextCompactionStatusFailure {
    /// Cancellation was observed.
    Cancelled,
    /// Source or predecessor fence no longer matched.
    SourceChanged,
    /// No complete prefix was available while retaining the newest user.
    NoCompactablePrefix,
    /// Source/result exceeded a bounded safety limit.
    TooLarge,
    /// Summary stream or reconstructed projection was malformed.
    InvalidSummary,
    TimedOut,
    /// Historical images are not currently representable by the summarizer.
    ImagesUnsupported,
    /// The configured budget could not fit the request/result.
    OverLimit,
    #[allow(missing_docs)]
    ReasoningLimit,
    #[allow(missing_docs)]
    Unavailable,
}

impl ContextCompactionStatusFailure {
    /// Reduces a runtime error to the content-free status vocabulary.
    pub fn from_error(error: &VegaError) -> Self {
        match error {
            VegaError::Cancelled => Self::Cancelled,
            VegaError::ReasoningBudgetExceeded { .. }
            | VegaError::ReasoningReplayBudgetExceeded { .. } => Self::ReasoningLimit,
            VegaError::Context(ContextRuntimeError::SourceChanged) => Self::SourceChanged,
            VegaError::Context(ContextRuntimeError::NoCompactablePrefix) => {
                Self::NoCompactablePrefix
            }
            VegaError::Context(ContextRuntimeError::SourceTooLarge) => Self::TooLarge,
            VegaError::Context(ContextRuntimeError::AggregateTooLarge) => Self::TooLarge,
            VegaError::Context(ContextRuntimeError::SummaryInputOverLimit { .. }) => {
                Self::OverLimit
            }
            VegaError::Context(ContextRuntimeError::ResultOverLimit { .. }) => Self::OverLimit,
            VegaError::Context(ContextRuntimeError::ImagesUnsupported) => Self::ImagesUnsupported,
            VegaError::Context(ContextRuntimeError::OverLimit { .. })
            | VegaError::Context(ContextRuntimeError::Estimate(_)) => Self::OverLimit,
            VegaError::Context(ContextRuntimeError::SummaryTimedOut) => Self::TimedOut,
            VegaError::Context(
                ContextRuntimeError::InvalidSummary
                | ContextRuntimeError::SummaryEmpty
                | ContextRuntimeError::SummaryFormatInvalid
                | ContextRuntimeError::SummaryProjectionInvalid
                | ContextRuntimeError::InvalidProjection
                | ContextRuntimeError::SummaryOutputTruncated { .. },
            ) => Self::InvalidSummary,
            _ => Self::Unavailable,
        }
    }
}

/// Metadata accompanying one compaction lifecycle phase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextCompactionStatusUpdate {
    /// Stable source/attempt identity shared by all lifecycle phases.  It is
    /// intentionally separate from `generation`, which resets with a new
    /// runtime invocation.
    pub operation_key: String,
    /// Runtime-local operation generation.  It changes for each attempt so a
    /// late status cannot overwrite a newer operation in the conversation.
    pub generation: u64,
    /// Lifecycle phase.
    pub phase: ContextCompactionPhase,
    /// Durable source revision observed by the runtime.
    pub source_version: u64,
    /// Estimate that caused compaction, when known.
    pub estimated_tokens: u64,
    /// Configured input budget.
    pub input_budget: u64,
    /// Target estimate after compaction.
    pub target_tokens: u64,
    /// Whether summary accounting is pending, known, or unavailable.
    pub usage: ContextCompactionUsageState,
    /// Safe failure code for Failed/Cancelled phases.
    pub failure: Option<ContextCompactionStatusFailure>,
}

/// A failed summary operation that may still carry actual provider usage.
///
/// Usage events can arrive before a summary is rejected for truncation,
/// cancellation, timeout, or persistence failure.  Keeping that usage on the
/// error prevents accounting from disappearing with the failed checkpoint.
pub struct ContextCompactionFailure {
    /// Safe typed failure to return to the runtime/conversation boundary.
    pub error: Box<VegaError>,
    /// Usage already observed before failure, if any.
    pub usages: Vec<ContextCompactionUsage>,
    /// False when any attempted stage omitted Usage.
    pub usage_complete: bool,
}

impl ContextCompactionFailure {
    /// Creates a compact error carrier for the provider/compaction boundary.
    ///
    /// `VegaError` contains provider and context details that are useful to
    /// callers, but it is large enough that embedding it directly in a
    /// `Result` makes every async error path expensive. Keep the public error
    /// semantics while boxing only the uncommon failure value.
    pub fn new(error: VegaError, usage: Option<ContextCompactionUsage>) -> Self {
        Self {
            error: Box::new(error),
            usage_complete: usage.is_some(),
            usages: usage.into_iter().collect(),
        }
    }

    /// Preserves all completed-stage usage when a later stage fails.
    pub fn with_usages(
        error: VegaError,
        usages: Vec<ContextCompactionUsage>,
        usage_complete: bool,
    ) -> Self {
        Self {
            error: Box::new(error),
            usages,
            usage_complete,
        }
    }
}

impl fmt::Debug for ContextCompactionFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContextCompactionFailure")
            .field("error", self.error.as_ref())
            .field("usage_count", &self.usages.len())
            .field("usage_complete", &self.usage_complete)
            .finish()
    }
}

/// Shared hook carrier used by [`crate::AgentRequest`].
pub type SharedContextCompactionHook = Arc<dyn ContextCompactionHook>;

/// Classifies an estimate while preserving the legacy unconfigured path.
pub fn classify_context(
    budget: Option<&ContextBudget>,
    estimate: ContextEstimate,
) -> Result<ContextCheck, ContextEstimateError> {
    budget.map_or(Ok(ContextCheck::Unconfigured), |budget| {
        budget.check(estimate)
    })
}

/// Estimates a complete primary request from protocol values, message text,
/// tool schemas and validated image contributions. The result is explicitly
/// approximate; provider usage events remain authoritative when available.
///
/// Text and typed JSON are charged with separate UTF-8 byte approximations,
/// followed by one conservative factor on non-image accounting. This deterministic
/// approximation is stable across model changes, but remains approximate.
/// Image bytes and geometry receive a fixed nonzero contribution because
/// provider image tokenization is model-specific; this policy does not claim
/// provider-exact image accounting.
pub fn estimate_chat_context(
    system_prompt: &str,
    messages: &[ChatMessage],
    tools: &[ToolDefinition],
) -> Result<ContextEstimate, ContextEstimateError> {
    let mut input_tokens = CONTEXT_PROTOCOL_OVERHEAD_TOKENS;
    input_tokens = input_tokens
        .checked_add(estimate_text(system_prompt)?)
        .and_then(|value| value.checked_add(CONTEXT_SAFETY_ALLOWANCE_TOKENS))
        .ok_or(ContextEstimateError::Overflow)?;
    let mut image_count = 0usize;
    let mut image_tokens = 0u64;
    let projected =
        crate::project_model_messages(messages).map_err(|_| ContextEstimateError::InputTooLarge)?;
    for message in &projected {
        input_tokens = checked_add(input_tokens, CONTEXT_PROTOCOL_OVERHEAD_TOKENS)?;
        if message.response_replay_layout.is_some() {
            input_tokens = checked_add(input_tokens, estimate_response_replay_layout(message)?)?;
            continue;
        }
        input_tokens = checked_add(input_tokens, estimate_text(&message.content)?)?;
        if let Some(reasoning) = &message.reasoning_content {
            input_tokens = checked_add(input_tokens, estimate_text(reasoning)?)?;
        }
        if let Some(raw) = &message.chat_reasoning {
            if let Some((source, text)) = &raw.text {
                input_tokens = checked_add(input_tokens, estimate_text(source.field())?)?;
                input_tokens = checked_add(input_tokens, estimate_text(text)?)?;
            }
            if let Some(details) = &raw.details {
                input_tokens = checked_add(
                    input_tokens,
                    estimate_json(&serde_json::json!(details).to_string())?,
                )?;
            }
        }
        for item in &message.response_reasoning {
            input_tokens = checked_add(input_tokens, estimate_json(&item.to_string())?)?;
        }
        for call in &message.tool_calls {
            input_tokens = checked_add(input_tokens, estimate_text(&call.id)?)?;
            input_tokens = checked_add(input_tokens, estimate_text(&call.name)?)?;
            input_tokens = checked_add(input_tokens, estimate_json(&call.input_json)?)?;
        }
        if let Some(call_id) = &message.tool_call_id {
            input_tokens = checked_add(input_tokens, estimate_text(call_id)?)?;
        }
        for image in &message.images {
            image_count = image_count
                .checked_add(1)
                .ok_or(ContextEstimateError::Overflow)?;
            image_tokens = checked_add(image_tokens, estimate_message_image(message, image)?)?;
        }
    }
    let mut tool_count = 0usize;
    for tool in tools {
        tool_count = tool_count
            .checked_add(1)
            .ok_or(ContextEstimateError::Overflow)?;
        let schema = serde_json::to_string(&tool.input_schema)
            .map_err(|_| ContextEstimateError::InputTooLarge)?;
        for value in [&tool.name, &tool.description] {
            input_tokens = checked_add(input_tokens, estimate_text(value)?)?;
        }
        input_tokens = checked_add(input_tokens, estimate_json(&schema)?)?;
        if tool.strict {
            input_tokens = checked_add(input_tokens, estimate_json(r#""strict":true"#)?)?;
        }
        input_tokens = checked_add(input_tokens, CONTEXT_PROTOCOL_OVERHEAD_TOKENS)?;
    }
    input_tokens = checked_add(ceil_ratio(input_tokens, 4, 3)?, image_tokens)?;
    Ok(ContextEstimate {
        input_tokens,
        message_count: projected.len() + 1,
        tool_count,
        image_count,
    })
}

/// Estimates the request exactly as the runtime wire assembly sees it.
///
/// [`estimate_chat_context`] is intentionally convenient for callers that
/// still hold a separate system prompt.  The agent loop, however, has
/// already prepended the system message to its `messages` vector.  Calling
/// this helper there avoids charging that prompt a second time.
pub fn estimate_wire_context(
    messages: &[ChatMessage],
    tools: &[ToolDefinition],
) -> Result<ContextEstimate, ContextEstimateError> {
    let mut input_tokens = CONTEXT_PROTOCOL_OVERHEAD_TOKENS
        .checked_add(CONTEXT_SAFETY_ALLOWANCE_TOKENS)
        .ok_or(ContextEstimateError::Overflow)?;
    let mut image_count = 0usize;
    let mut image_tokens = 0u64;
    let projected =
        crate::project_model_messages(messages).map_err(|_| ContextEstimateError::InputTooLarge)?;
    for (index, message) in projected.iter().enumerate() {
        // The standalone estimator charges one framing unit for the implicit
        // system slot.  When the loop hands us the already assembled wire
        // vector, that slot is the first explicit System value, so do not
        // charge a second framing unit for it.
        if !(index == 0 && message.role == crate::ChatRole::System) {
            input_tokens = checked_add(input_tokens, CONTEXT_PROTOCOL_OVERHEAD_TOKENS)?;
        }
        if message.response_replay_layout.is_some() {
            input_tokens = checked_add(input_tokens, estimate_response_replay_layout(message)?)?;
            continue;
        }
        input_tokens = checked_add(input_tokens, estimate_text(&message.content)?)?;
        if let Some(reasoning) = &message.reasoning_content {
            input_tokens = checked_add(input_tokens, estimate_text(reasoning)?)?;
        }
        if let Some(raw) = &message.chat_reasoning {
            if let Some((source, text)) = &raw.text {
                input_tokens = checked_add(input_tokens, estimate_text(source.field())?)?;
                input_tokens = checked_add(input_tokens, estimate_text(text)?)?;
            }
            if let Some(details) = &raw.details {
                input_tokens = checked_add(
                    input_tokens,
                    estimate_json(&serde_json::json!(details).to_string())?,
                )?;
            }
        }
        for item in &message.response_reasoning {
            input_tokens = checked_add(input_tokens, estimate_json(&item.to_string())?)?;
        }
        for call in &message.tool_calls {
            input_tokens = checked_add(input_tokens, estimate_text(&call.id)?)?;
            input_tokens = checked_add(input_tokens, estimate_text(&call.name)?)?;
            input_tokens = checked_add(input_tokens, estimate_json(&call.input_json)?)?;
        }
        if let Some(call_id) = &message.tool_call_id {
            input_tokens = checked_add(input_tokens, estimate_text(call_id)?)?;
        }
        for image in &message.images {
            image_count = image_count
                .checked_add(1)
                .ok_or(ContextEstimateError::Overflow)?;
            image_tokens = checked_add(image_tokens, estimate_message_image(message, image)?)?;
        }
    }
    let mut tool_count = 0usize;
    for tool in tools {
        tool_count = tool_count
            .checked_add(1)
            .ok_or(ContextEstimateError::Overflow)?;
        let schema = serde_json::to_string(&tool.input_schema)
            .map_err(|_| ContextEstimateError::InputTooLarge)?;
        for value in [&tool.name, &tool.description] {
            input_tokens = checked_add(input_tokens, estimate_text(value)?)?;
        }
        input_tokens = checked_add(input_tokens, estimate_json(&schema)?)?;
        if tool.strict {
            input_tokens = checked_add(input_tokens, estimate_json(r#""strict":true"#)?)?;
        }
        input_tokens = checked_add(input_tokens, CONTEXT_PROTOCOL_OVERHEAD_TOKENS)?;
    }
    input_tokens = checked_add(ceil_ratio(input_tokens, 4, 3)?, image_tokens)?;
    Ok(ContextEstimate {
        input_tokens,
        message_count: if projected
            .first()
            .is_some_and(|message| message.role == crate::ChatRole::System)
        {
            projected.len()
        } else {
            projected.len() + 1
        },
        tool_count,
        image_count,
    })
}

fn checked_add(left: u64, right: u64) -> Result<u64, ContextEstimateError> {
    left.checked_add(right)
        .ok_or(ContextEstimateError::Overflow)
}

fn estimate_text(value: &str) -> Result<u64, ContextEstimateError> {
    estimate_bytes(value, 4)
}

fn estimate_json(value: &str) -> Result<u64, ContextEstimateError> {
    estimate_bytes(value, 2)
}

fn estimate_response_replay_layout(message: &ChatMessage) -> Result<u64, ContextEstimateError> {
    let items = crate::openai::responses::response_layout_items(message)
        .map_err(|_| ContextEstimateError::InputTooLarge)?
        .ok_or(ContextEstimateError::InputTooLarge)?;
    items.into_iter().try_fold(0u64, |total, item| {
        let serialized =
            serde_json::to_string(&item).map_err(|_| ContextEstimateError::InputTooLarge)?;
        checked_add(
            checked_add(total, CONTEXT_PROTOCOL_OVERHEAD_TOKENS)?,
            estimate_json(&serialized)?,
        )
    })
}

fn estimate_bytes(value: &str, divisor: u64) -> Result<u64, ContextEstimateError> {
    if value.len() > MAX_ESTIMATED_BYTES {
        return Err(ContextEstimateError::InputTooLarge);
    }
    let bytes = u64::try_from(value.len()).map_err(|_| ContextEstimateError::Overflow)?;
    bytes
        .checked_add(divisor - 1)
        .map(|bytes| bytes / divisor)
        .ok_or(ContextEstimateError::Overflow)
}

fn estimate_message_image(
    message: &ChatMessage,
    image: &crate::ImageAttachment,
) -> Result<u64, ContextEstimateError> {
    if message.role == crate::ChatRole::Tool {
        let pixels = u64::from(image.width())
            .checked_mul(u64::from(image.height()))
            .ok_or(ContextEstimateError::Overflow)?;
        checked_add(ceil_ratio(pixels, 1, 256)?, 1280)
    } else {
        estimate_image(image)
    }
}

fn estimate_image(image: &crate::ImageAttachment) -> Result<u64, ContextEstimateError> {
    let pixels = u64::from(image.width())
        .checked_mul(u64::from(image.height()))
        .ok_or(ContextEstimateError::Overflow)?;
    estimate_image_contribution(pixels)
}

fn estimate_image_contribution(pixels: u64) -> Result<u64, ContextEstimateError> {
    let geometry = pixels
        .checked_add(255)
        .ok_or(ContextEstimateError::Overflow)?
        / 256;
    1024u64
        .checked_add(geometry)
        .ok_or(ContextEstimateError::Overflow)
}

fn ceil_ratio(value: u64, numerator: u64, denominator: u64) -> Result<u64, ContextEstimateError> {
    value
        .checked_mul(numerator)
        .and_then(|value| value.checked_add(denominator - 1))
        .map(|value| value / denominator)
        .ok_or(ContextEstimateError::Overflow)
}

fn floor_ratio(value: u64, numerator: u64, denominator: u64) -> Result<u64, ContextEstimateError> {
    value
        .checked_mul(numerator)
        .map(|value| value / denominator)
        .ok_or(ContextEstimateError::Overflow)
}
