use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};

use futures::{future::BoxFuture, stream};
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ExtendedColorType, ImageEncoder};
use tokio_util::sync::CancellationToken;
use vega_runtime::{
    AgentOutcome, AgentRequest, CONTEXT_ESTIMATOR_VERSION, ChatMessage, ChatRequest, ChatRole,
    ChatToolCall, ContextAccountingStage, ContextBudget, ContextCheck, ContextCompactionFailure,
    ContextCompactionHook, ContextCompactionPhase, ContextCompactionRequest,
    ContextCompactionResult, ContextCompactionStatusFailure, ContextCompactionStatusUpdate,
    ContextEstimate, ContextRuntimeError, EventStream, ImageAttachment, Provider, ProviderEvent,
    ReasoningReplayPolicy, ReplayFrame, RuntimeEvent, RuntimeFinishReason, RuntimePermissionMode,
    RuntimeRunMode, RuntimeToolConfig, StopReason, VegaError, estimate_chat_context,
    estimate_wire_context, project_model_messages, run_agent_with_sink, tool_definitions,
};
use vega_tools::Tools;

const SYSTEM: &str = "Describe the supplied synthetic input.";
const ANSWER: &str = "The synthetic input was received.";
const PARTIAL_ANSWER: &str = "Partial synthetic response.";
const OVERFLOW: &str = r#"{"error":{"code":"context_length_exceeded","message":"prompt is too long","requested_tokens":400001,"max_context_tokens":400000}}"#;

#[derive(Clone, Copy)]
enum Reply {
    Text,
    SetupOverflow,
    StreamOverflow,
    PartialTextStreamOverflow,
}

struct ScriptedProvider {
    replies: Mutex<VecDeque<Reply>>,
    requests: Mutex<Vec<ChatRequest>>,
}

impl ScriptedProvider {
    fn new(replies: impl IntoIterator<Item = Reply>) -> Self {
        Self {
            replies: Mutex::new(replies.into_iter().collect()),
            requests: Mutex::new(Vec::new()),
        }
    }

    fn requests(&self) -> Vec<ChatRequest> {
        self.requests.lock().unwrap().clone()
    }
}

fn provider_overflow() -> VegaError {
    VegaError::Provider {
        status: Some(400),
        message: OVERFLOW.into(),
        retryable: false,
    }
}

impl Provider for ScriptedProvider {
    fn chat_stream(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        assert!(!cancel.is_cancelled(), "provider called after cancellation");
        self.requests.lock().unwrap().push(request);
        let reply = self
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected additional provider request");
        Box::pin(async move {
            let events = match reply {
                Reply::SetupOverflow => return Err(provider_overflow()),
                Reply::StreamOverflow => vec![Err(provider_overflow())],
                Reply::PartialTextStreamOverflow => vec![
                    Ok(ProviderEvent::TextDelta(PARTIAL_ANSWER.into())),
                    Err(provider_overflow()),
                ],
                Reply::Text => vec![
                    Ok(ProviderEvent::TextDelta(ANSWER.into())),
                    Ok(ProviderEvent::Done {
                        stop_reason: StopReason::End,
                    }),
                ],
            };
            Ok(Box::pin(stream::iter(events)) as EventStream)
        })
    }
}

#[derive(Clone)]
enum SummaryReply {
    Projection(Vec<ChatMessage>),
    Failure(ContextRuntimeError),
    Cancelled,
    CancelAfterProjection(Vec<ChatMessage>),
}

struct ScriptedCompaction {
    reply: SummaryReply,
    requests: Mutex<Vec<ContextCompactionRequest>>,
}

impl ScriptedCompaction {
    fn new(reply: SummaryReply) -> Arc<Self> {
        Arc::new(Self {
            reply,
            requests: Mutex::new(Vec::new()),
        })
    }

    fn requests(&self) -> Vec<ContextCompactionRequest> {
        self.requests.lock().unwrap().clone()
    }
}

impl ContextCompactionHook for ScriptedCompaction {
    fn compact<'a>(
        &'a self,
        request: ContextCompactionRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'a, Result<ContextCompactionResult, ContextCompactionFailure>> {
        self.requests.lock().unwrap().push(request.clone());
        let reply = self.reply.clone();
        Box::pin(async move {
            let messages = match reply {
                SummaryReply::Projection(messages) => messages,
                SummaryReply::Failure(error) => {
                    return Err(ContextCompactionFailure::new(
                        VegaError::Context(error),
                        None,
                    ));
                }
                SummaryReply::Cancelled => {
                    cancel.cancel();
                    return Err(ContextCompactionFailure::new(VegaError::Cancelled, None));
                }
                SummaryReply::CancelAfterProjection(messages) => {
                    cancel.cancel();
                    messages
                }
            };
            Ok(ContextCompactionResult {
                messages,
                source_version: request.source_version + 1,
                source_fingerprint: Some("synthetic-compacted-source".into()),
                replay_cold_prefix_messages: Some(1),
                replay_interleaved_user_boundary: None,
                usages: Vec::new(),
                usage_complete: false,
            })
        })
    }
}

fn request(history: Vec<ChatMessage>) -> AgentRequest {
    AgentRequest {
        model: "synthetic-vision-model".into(),
        system_prompt: SYSTEM.into(),
        history,
        max_tokens: Some(128),
        completed_tool_results: HashMap::new(),
        tool_config: RuntimeToolConfig::new(
            RuntimeRunMode::Ask,
            RuntimePermissionMode::ReadOnly,
            "synthetic-project".into(),
            "synthetic-thread".into(),
            env!("CARGO_MANIFEST_DIR").into(),
            Vec::new(),
        ),
        pricing_catalog: None,
        reasoning: None,
        model_snapshot: None,
        replay_frame: None,
        context_budget: None,
        context_source_version: Some(17),
        context_source_fingerprint: Some("synthetic-source".into()),
        context_operation_id: Some("synthetic-run".into()),
        context_compaction_hook: None,
    }
}

fn assembled_messages(request: &AgentRequest) -> Vec<ChatMessage> {
    let mut system = ChatMessage::new(ChatRole::System, request.system_prompt.clone());
    system.attach_replay_frame(request.replay_frame.clone().unwrap_or_else(|| {
        ReplayFrame::new(request.history.len() + 1, ReasoningReplayPolicy::Unknown)
    }));
    let mut messages = vec![system];
    messages.extend(request.history.clone());
    messages
}

fn estimate(request: &AgentRequest) -> ContextEstimate {
    estimate_wire_context(
        &assembled_messages(request),
        &tool_definitions(RuntimeRunMode::Ask),
    )
    .unwrap()
}

fn budget(input: u64, automatic: bool) -> ContextBudget {
    ContextBudget::new(input + 128, 128, automatic).unwrap()
}

fn threshold_budget(request: &AgentRequest) -> ContextBudget {
    let estimate = estimate(request);
    let budget = budget(estimate.input_tokens * 5 / 4, true);
    assert_eq!(budget.trigger_tokens().unwrap(), estimate.input_tokens);
    assert_eq!(budget.check(estimate).unwrap(), ContextCheck::Triggered);
    budget
}

async fn run(
    provider: &ScriptedProvider,
    request: AgentRequest,
) -> (Result<AgentOutcome, VegaError>, Vec<RuntimeEvent>) {
    let tools = Tools::new(env!("CARGO_MANIFEST_DIR")).unwrap();
    let mut events = Vec::new();
    let result = run_agent_with_sink(
        provider,
        &tools,
        request,
        CancellationToken::new(),
        |event| {
            events.push(event);
            std::future::ready(Ok(()))
        },
    )
    .await;
    (result, events)
}

fn summary_statuses(events: &[RuntimeEvent]) -> Vec<&ContextCompactionStatusUpdate> {
    events
        .iter()
        .filter_map(|event| match event {
            RuntimeEvent::ContextCompactionStatusUpdated { status } => Some(status),
            _ => None,
        })
        .collect()
}

fn assert_no_summary(events: &[RuntimeEvent]) {
    assert!(
        summary_statuses(events).is_empty(),
        "unexpected summary lifecycle"
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, RuntimeEvent::ContextCompactionUsageUpdated { .. }))
    );
}

fn assert_success(outcome: &AgentOutcome) {
    assert!(!outcome.failed);
    assert!(!outcome.interrupted);
    assert_eq!(outcome.final_text, ANSWER);
    assert_eq!(outcome.executed_tool_call_count, 0);
    assert_eq!(
        outcome
            .events
            .iter()
            .filter_map(|event| match event {
                RuntimeEvent::TextDelta(text) => Some(text.as_str()),
                _ => None,
            })
            .collect::<String>(),
        ANSWER
    );
    assert!(
        outcome
            .events
            .iter()
            .any(|event| matches!(event, RuntimeEvent::Finished(RuntimeFinishReason::End)))
    );
    assert!(
        !outcome
            .events
            .iter()
            .any(|event| matches!(event, RuntimeEvent::Error(_)))
    );
}

fn assert_history_sent(sent: &ChatRequest, history: &[ChatMessage]) {
    let mut expected = vec![sent.messages[0].clone()];
    expected.extend_from_slice(history);
    assert_eq!(sent.messages, project_model_messages(&expected).unwrap());
}

fn assert_summary_lifecycle(events: &[RuntimeEvent], terminal: ContextCompactionPhase) {
    let statuses = summary_statuses(events);
    assert_eq!(
        statuses
            .iter()
            .map(|status| status.phase)
            .collect::<Vec<_>>(),
        vec![ContextCompactionPhase::Started, terminal]
    );
    assert_eq!(statuses[0].operation_key, statuses[1].operation_key);
    assert_eq!(statuses[0].source_version, 17);
    assert_eq!(statuses[0].input_budget, statuses[1].input_budget);
}

fn assert_overflow_outcome(outcome: &AgentOutcome) {
    assert_overflow_outcome_with_text(outcome, "");
}

fn assert_overflow_outcome_with_text(outcome: &AgentOutcome, expected_text: &str) {
    assert!(outcome.failed);
    assert!(!outcome.interrupted);
    assert_eq!(outcome.final_text, expected_text);
    assert_eq!(
        outcome
            .events
            .iter()
            .filter_map(|event| match event {
                RuntimeEvent::TextDelta(text) => Some(text.as_str()),
                _ => None,
            })
            .collect::<String>(),
        expected_text
    );
    assert_eq!(outcome.executed_tool_call_count, 0);
    let errors = outcome
        .events
        .iter()
        .filter_map(|event| match event {
            RuntimeEvent::Error(error) => Some(error.as_ref()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(errors.len(), 1);
    assert!(matches!(
        errors[0],
        VegaError::Provider { status: Some(400), message, retryable: false }
            if message == OVERFLOW
    ));
    assert!(
        !outcome
            .events
            .iter()
            .any(|event| matches!(event, RuntimeEvent::Finished(_)))
    );
}

fn png_bytes(
    width: u32,
    height: u32,
    pixels: &[u8],
    compression: CompressionType,
    filter: FilterType,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    PngEncoder::new_with_quality(&mut bytes, compression, filter)
        .write_image(pixels, width, height, ExtendedColorType::L8)
        .unwrap();
    bytes
}

fn solid_image(width: u32, height: u32) -> ImageAttachment {
    ImageAttachment::from_bytes(png_bytes(
        width,
        height,
        &vec![192; (width * height) as usize],
        CompressionType::Fast,
        FilterType::NoFilter,
    ))
    .unwrap()
}

fn noise_pixels(width: u32, height: u32) -> Vec<u8> {
    let mut state = 0x4f1b_92d3_u32;
    (0..width * height)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        })
        .collect()
}

fn noise_png(width: u32, height: u32) -> Vec<u8> {
    png_bytes(
        width,
        height,
        &noise_pixels(width, height),
        CompressionType::Fast,
        FilterType::NoFilter,
    )
}

fn large_valid_image() -> ImageAttachment {
    static IMAGE: OnceLock<ImageAttachment> = OnceLock::new();
    IMAGE
        .get_or_init(|| {
            let image = ImageAttachment::from_bytes(noise_png(2304, 2304)).unwrap();
            assert!(image.bytes().len() > 5 * 1024 * 1024);
            assert!(image.bytes().len() < 16 * 1024 * 1024 / 3);
            image
        })
        .clone()
}

fn user_with_images(images: Vec<ImageAttachment>) -> ChatMessage {
    let mut message = ChatMessage::new(ChatRole::User, "Describe these synthetic images.");
    message.images = images;
    message
}

fn completed_group(id: &str, output: &str, images: Vec<ImageAttachment>) -> Vec<ChatMessage> {
    let assistant = ChatMessage::assistant_with_tools(
        "",
        vec![ChatToolCall {
            id: id.into(),
            name: "inspect_synthetic_input".into(),
            input_json: r#"{"region":"synthetic"}"#.into(),
        }],
    );
    let mut tool = ChatMessage::tool_result(id, output);
    tool.images = images;
    vec![assistant, tool]
}

fn image_increments(history: &[ChatMessage]) -> [u64; 2] {
    let mut text_only = history.to_vec();
    for message in &mut text_only {
        message.images.clear();
    }
    let mut wire = vec![ChatMessage::new(ChatRole::System, SYSTEM)];
    wire.extend_from_slice(history);
    let mut wire_text_only = vec![ChatMessage::new(ChatRole::System, SYSTEM)];
    wire_text_only.extend(text_only.clone());
    let chat = estimate_chat_context(SYSTEM, history, &[]).unwrap();
    let wire_estimate = estimate_wire_context(&wire, &[]).unwrap();
    assert_eq!(chat, wire_estimate);
    assert_eq!(
        chat.image_count,
        history
            .iter()
            .map(|message| message.images.len())
            .sum::<usize>()
    );
    [
        chat.input_tokens
            - estimate_chat_context(SYSTEM, &text_only, &[])
                .unwrap()
                .input_tokens,
        wire_estimate.input_tokens
            - estimate_wire_context(&wire_text_only, &[])
                .unwrap()
                .input_tokens,
    ]
}

fn differently_compressed_images() -> [ImageAttachment; 2] {
    let pixels = (0..256 * 256)
        .map(|index| ((index % 256 / 8 % 2) * 128 + (index / 256 / 16 % 2) * 64) as u8)
        .collect::<Vec<_>>();
    let images = [
        ImageAttachment::from_bytes(png_bytes(
            256,
            256,
            &pixels,
            CompressionType::Fast,
            FilterType::NoFilter,
        ))
        .unwrap(),
        ImageAttachment::from_bytes(png_bytes(
            256,
            256,
            &pixels,
            CompressionType::Best,
            FilterType::Adaptive,
        ))
        .unwrap(),
    ];
    assert_ne!(images[0].bytes().len(), images[1].bytes().len());
    for image in &images {
        let decoded = image::load_from_memory(image.bytes()).unwrap().into_luma8();
        assert_eq!(decoded.dimensions(), (256, 256));
        assert_eq!(decoded.as_raw(), &pixels);
    }
    images
}

#[test]
fn user_image_estimates_ignore_lossless_compression_in_chat_and_wire() {
    let images = differently_compressed_images();
    let original = images.clone();
    let first = image_increments(&[user_with_images(vec![images[0].clone()])]);
    let second = image_increments(&[user_with_images(vec![images[1].clone()])]);
    assert_eq!(
        first, second,
        "encoded byte size must not change the visual estimate"
    );
    assert_eq!(first, [1280, 1280]);
    assert_eq!(images, original);
}

#[test]
fn versioned_user_image_cost_is_positive_and_grows_with_geometry() {
    let small = solid_image(1, 1);
    let larger = solid_image(513, 257);
    assert_eq!(
        image_increments(&[user_with_images(vec![small])]),
        [1025, 1025]
    );
    assert_eq!(
        image_increments(&[user_with_images(vec![larger])]),
        [1540, 1540]
    );
    assert_eq!(CONTEXT_ESTIMATOR_VERSION, "vega-context-estimator-v3");
}

#[test]
fn tool_image_estimates_keep_their_geometry_allowance_and_ignore_compression() {
    let images = differently_compressed_images();
    let original = images.clone();
    for image in &images {
        assert_eq!(
            image_increments(&completed_group(
                "observation",
                "image",
                vec![image.clone()]
            )),
            [1536, 1536]
        );
    }
    assert_eq!(
        image_increments(&completed_group("small", "image", vec![solid_image(1, 1)])),
        [1281, 1281]
    );
    assert_eq!(
        image_increments(&completed_group(
            "large",
            "image",
            vec![solid_image(513, 257)]
        )),
        [1796, 1796]
    );
    assert_eq!(images, original);
}

#[tokio::test]
async fn first_send_with_two_synthetic_screenshots_preserves_images_without_summary() {
    let images = [(2808, 1720), (2936, 1878)]
        .into_iter()
        .map(|(width, height)| {
            let mut pixels = noise_pixels(width, height);
            pixels[..(width * height / 2) as usize].fill(224);
            ImageAttachment::from_bytes(png_bytes(
                width,
                height,
                &pixels,
                CompressionType::Fast,
                FilterType::NoFilter,
            ))
            .unwrap()
        })
        .collect::<Vec<_>>();
    assert!(
        images
            .iter()
            .map(|image| image.bytes().len())
            .sum::<usize>()
            > 1_200_000
    );
    vega_runtime::images::validate_images(&images).unwrap();
    let history = vec![user_with_images(images.clone())];
    let mut request = request(history.clone());
    request.context_budget = Some(ContextBudget::new(428_000, 128_000, true).unwrap());
    let hook = ScriptedCompaction::new(SummaryReply::Failure(
        ContextRuntimeError::NoCompactablePrefix,
    ));
    request.context_compaction_hook = Some(hook.clone());
    let provider = ScriptedProvider::new([Reply::Text]);
    let (result, events) = run(&provider, request).await;
    let outcome = result.expect("ordinary first-turn screenshots must reach the provider");
    assert_success(&outcome);
    let sent = provider.requests();
    assert_eq!(sent.len(), 1);
    assert_history_sent(&sent[0], &history);
    assert_eq!(sent[0].messages[1].images, images);
    assert_eq!(sent[0].max_tokens, Some(128));
    assert_eq!(outcome.messages[1].images, images);
    assert!(hook.requests().is_empty());
    assert_no_summary(&events);
    assert_eq!(image_increments(&history), [42_454, 42_454]);
}

async fn assert_initial_send(history: Vec<ChatMessage>, at_hard_limit: bool, with_hook: bool) {
    let mut request = request(history.clone());
    let estimate = estimate(&request);
    let budget = if at_hard_limit {
        budget(estimate.input_tokens, true)
    } else {
        threshold_budget(&request)
    };
    assert_eq!(budget.check(estimate).unwrap(), ContextCheck::Triggered);
    request.context_budget = Some(budget);
    let hook = ScriptedCompaction::new(SummaryReply::Failure(
        ContextRuntimeError::NoCompactablePrefix,
    ));
    if with_hook {
        request.context_compaction_hook = Some(hook.clone());
    }
    let provider = ScriptedProvider::new([Reply::Text]);
    let (result, events) = run(&provider, request).await;
    let outcome = result.expect("initial input within the hard limit must skip compaction");
    assert!(hook.requests().is_empty());
    assert_success(&outcome);
    assert_eq!(provider.requests().len(), 1);
    assert_history_sent(&provider.requests()[0], &history);
    assert_no_summary(&events);
    assert!(events.iter().any(
        |event| matches!(event, RuntimeEvent::ContextAccountingUpdated(decision)
        if decision.stage == ContextAccountingStage::PrimaryPreflight
            && decision.predicted_input == estimate.input_tokens
            && decision.input_budget == budget.input_budget())
    ));
}

#[tokio::test]
async fn single_user_at_trigger_sends_without_a_compaction_hook() {
    assert_initial_send(
        vec![ChatMessage::new(ChatRole::User, "Current input")],
        false,
        false,
    )
    .await;
}

#[tokio::test]
async fn single_user_at_exact_hard_limit_sends_without_a_compaction_hook() {
    assert_initial_send(
        vec![ChatMessage::new(ChatRole::User, "Current input")],
        true,
        false,
    )
    .await;
}

#[tokio::test]
async fn single_user_at_trigger_and_exact_hard_limit_skips_a_present_compaction_hook() {
    for at_hard_limit in [false, true] {
        assert_initial_send(
            vec![ChatMessage::new(ChatRole::User, "Current input")],
            at_hard_limit,
            true,
        )
        .await;
    }
}

#[tokio::test]
async fn user_tool_metadata_preserves_terminal_compaction_errors_at_trigger() {
    let mut with_call = ChatMessage::new(ChatRole::User, "Current input");
    with_call.tool_calls.push(ChatToolCall {
        id: "synthetic-call".into(),
        name: "inspect_synthetic_input".into(),
        input_json: "{}".into(),
    });
    let mut with_result_id = ChatMessage::new(ChatRole::User, "Current input");
    with_result_id.tool_call_id = Some("synthetic-call".into());
    let mut with_empty_result_id = ChatMessage::new(ChatRole::User, "Current input");
    with_empty_result_id.tool_call_id = Some(String::new());
    for message in [with_call, with_result_id, with_empty_result_id] {
        let history = vec![message];
        let mut request = request(history.clone());
        let projected = project_model_messages(&assembled_messages(&request)).unwrap();
        assert_eq!(projected.len(), 2);
        assert_eq!(projected[1].role, ChatRole::User);
        assert_eq!(projected[1].tool_calls, history[0].tool_calls);
        assert_eq!(projected[1].tool_call_id, history[0].tool_call_id);
        let budget = threshold_budget(&request);
        request.context_budget = Some(budget);
        let hook =
            ScriptedCompaction::new(SummaryReply::Failure(ContextRuntimeError::SourceChanged));
        request.context_compaction_hook = Some(hook.clone());
        let provider = ScriptedProvider::new([]);
        let (result, events) = run(&provider, request).await;
        assert!(matches!(
            result,
            Err(VegaError::Context(ContextRuntimeError::SourceChanged))
        ));
        assert!(provider.requests().is_empty());
        assert_hook_received(&hook, &history, budget);
        assert_summary_lifecycle(&events, ContextCompactionPhase::Failed);
        assert_eq!(
            summary_statuses(&events)[1].failure,
            Some(ContextCompactionStatusFailure::SourceChanged)
        );
    }
}

async fn assert_initial_over_limit(automatic: bool, with_hook: bool) {
    let mut request = request(vec![ChatMessage::new(ChatRole::User, "Current input")]);
    let estimate = estimate(&request);
    let budget = budget(estimate.input_tokens - 1, automatic);
    request.context_budget = Some(budget);
    let hook = ScriptedCompaction::new(SummaryReply::Failure(
        ContextRuntimeError::NoCompactablePrefix,
    ));
    if with_hook {
        request.context_compaction_hook = Some(hook.clone());
    }
    let provider = ScriptedProvider::new([]);
    let (result, events) = run(&provider, request).await;
    let error = result.expect_err("initial input above the hard limit must not be sent");
    assert!(
        matches!(error, VegaError::Context(ContextRuntimeError::OverLimit { estimated_tokens, input_budget })
        if estimated_tokens == estimate.input_tokens && input_budget == budget.input_budget()),
        "unexpected error: {error:?}"
    );
    assert!(provider.requests().is_empty());
    assert!(hook.requests().is_empty());
    assert_no_summary(&events);
}

#[tokio::test]
async fn initial_over_limit_with_automatic_compaction_returns_typed_limit_without_summary() {
    assert_initial_over_limit(true, true).await;
}

#[tokio::test]
async fn initial_over_limit_without_automatic_compaction_returns_typed_limit() {
    assert_initial_over_limit(false, true).await;
}

#[tokio::test]
async fn initial_over_limit_without_a_hook_is_not_a_missing_hook_error() {
    assert_initial_over_limit(true, false).await;
}

#[tokio::test]
async fn cancellation_while_awaiting_primary_preflight_precedes_initial_limit_errors() {
    for automatic in [true, false] {
        let history = vec![ChatMessage::new(ChatRole::User, "Current input")];
        let mut request = request(history.clone());
        let estimate = estimate(&request);
        let budget = budget(estimate.input_tokens - 1, automatic);
        assert_eq!(budget.check(estimate).unwrap(), ContextCheck::OverLimit);
        request.context_budget = Some(budget);
        let provider = ScriptedProvider::new([]);
        let tools = Tools::new(env!("CARGO_MANIFEST_DIR")).unwrap();
        let cancel = CancellationToken::new();
        let mut events = Vec::new();
        let result = run_agent_with_sink(&provider, &tools, request, cancel.clone(), |event| {
            let cancel = cancel.clone();
            let at_preflight = matches!(&event, RuntimeEvent::ContextAccountingUpdated(decision)
                if decision.stage == ContextAccountingStage::PrimaryPreflight);
            events.push(event);
            async move {
                if at_preflight {
                    tokio::task::yield_now().await;
                    cancel.cancel();
                }
                Ok(())
            }
        })
        .await;
        assert!(cancel.is_cancelled());
        assert_eq!(
            events
                .iter()
                .filter(
                    |event| matches!(event, RuntimeEvent::ContextAccountingUpdated(decision)
            if decision.stage == ContextAccountingStage::PrimaryPreflight
                && decision.predicted_input == estimate.input_tokens
                && decision.input_budget == budget.input_budget())
                )
                .count(),
            1
        );
        let outcome =
            result.expect("preflight cancellation must precede over-limit and missing-hook errors");
        assert!(outcome.interrupted);
        assert!(!outcome.failed);
        assert!(outcome.final_text.is_empty());
        assert_eq!(&outcome.messages[1..], history);
        assert_eq!(outcome.executed_tool_call_count, 0);
        assert!(
            events
                .iter()
                .any(|event| matches!(event, RuntimeEvent::Interrupted))
        );
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, RuntimeEvent::Error(_) | RuntimeEvent::Finished(_)))
        );
        assert!(provider.requests().is_empty());
        assert_no_summary(&events);
    }
}

fn old_history() -> Vec<ChatMessage> {
    vec![
        ChatMessage::new(
            ChatRole::User,
            "Earlier synthetic information. ".repeat(4000),
        ),
        ChatMessage::new(ChatRole::Assistant, "Earlier answer."),
        ChatMessage::new(
            ChatRole::User,
            "Use the earlier result for the current task.",
        ),
    ]
}

fn summarized_history(history: &[ChatMessage]) -> Vec<ChatMessage> {
    vec![
        ChatMessage::new(
            ChatRole::User,
            "Earlier exchange summarized: synthetic information retained.",
        ),
        history.last().unwrap().clone(),
    ]
}

fn assert_hook_received(hook: &ScriptedCompaction, history: &[ChatMessage], budget: ContextBudget) {
    let requests = hook.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].messages, history);
    assert_eq!(requests[0].system_prompt, SYSTEM);
    assert_eq!(requests[0].budget, budget);
    assert_eq!(requests[0].source_version, 17);
    assert_eq!(
        requests[0].source_fingerprint.as_deref(),
        Some("synthetic-source")
    );
    assert_eq!(
        requests[0].source_owner_id.as_deref(),
        Some("synthetic-run")
    );
    assert!(!requests[0].require_source_fence);
}

#[tokio::test]
async fn old_history_at_trigger_uses_summary_projection_and_retains_current_user() {
    let history = old_history();
    let projection = summarized_history(&history);
    let mut request = request(history.clone());
    let budget = threshold_budget(&request);
    request.context_budget = Some(budget);
    let hook = ScriptedCompaction::new(SummaryReply::Projection(projection.clone()));
    request.context_compaction_hook = Some(hook.clone());
    let provider = ScriptedProvider::new([Reply::Text]);
    let (result, events) = run(&provider, request).await;
    assert_success(&result.unwrap());
    assert_hook_received(&hook, &history, budget);
    assert_eq!(provider.requests().len(), 1);
    assert_history_sent(&provider.requests()[0], &projection);
    assert_summary_lifecycle(&events, ContextCompactionPhase::Succeeded);
}

#[tokio::test]
async fn first_user_round_with_multiple_completed_tool_groups_can_still_compact() {
    let current = ChatMessage::new(ChatRole::User, "Inspect the synthetic input in stages.");
    let retained = completed_group("retained", "Retained completed observation", Vec::new());
    let mut history = vec![current.clone()];
    history.extend(completed_group(
        "first",
        &"First observation. ".repeat(4000),
        Vec::new(),
    ));
    history.extend(completed_group(
        "second",
        &"Second observation. ".repeat(4000),
        Vec::new(),
    ));
    history.extend(retained.clone());
    assert_eq!(
        history
            .iter()
            .filter(|message| message.role == ChatRole::User)
            .count(),
        1
    );
    let mut projection = vec![
        ChatMessage::new(ChatRole::User, "Earlier completed observations summarized."),
        current,
    ];
    projection.extend(retained);
    let mut request = request(history.clone());
    request.replay_frame = Some(ReplayFrame::new(1, ReasoningReplayPolicy::Unknown));
    let budget = threshold_budget(&request);
    request.context_budget = Some(budget);
    let hook = ScriptedCompaction::new(SummaryReply::Projection(projection.clone()));
    request.context_compaction_hook = Some(hook.clone());
    let provider = ScriptedProvider::new([Reply::Text]);
    let (result, events) = run(&provider, request).await;
    let outcome = result.unwrap();
    assert_success(&outcome);
    assert_hook_received(&hook, &history, budget);
    assert_eq!(provider.requests().len(), 1);
    assert_history_sent(&provider.requests()[0], &projection);
    assert!(
        provider.requests()[0]
            .messages
            .iter()
            .any(
                |message| message.tool_call_id.as_deref() == Some("retained")
                    && message.content.contains("Retained completed observation")
            )
    );
    assert_summary_lifecycle(&events, ContextCompactionPhase::Succeeded);
}

async fn assert_initial_provider_overflow(reply: Reply) {
    let history = vec![ChatMessage::new(ChatRole::User, "Initial input")];
    let mut request = request(history.clone());
    let estimate = estimate(&request);
    let budget = budget(estimate.input_tokens * 3, true);
    assert_eq!(budget.check(estimate).unwrap(), ContextCheck::Within);
    request.context_budget = Some(budget);
    let hook = ScriptedCompaction::new(SummaryReply::Failure(
        ContextRuntimeError::NoCompactablePrefix,
    ));
    request.context_compaction_hook = Some(hook.clone());
    let provider = ScriptedProvider::new([reply]);
    let (result, events) = run(&provider, request).await;
    let outcome = result
        .expect("provider overflow must remain the visible failure, not an empty-summary error");
    assert_overflow_outcome(&outcome);
    assert_eq!(provider.requests().len(), 1);
    assert_history_sent(&provider.requests()[0], &history);
    assert!(hook.requests().is_empty());
    assert_no_summary(&events);
}

#[tokio::test]
async fn initial_provider_setup_overflow_preserves_failure_without_summary_or_retry() {
    assert_initial_provider_overflow(Reply::SetupOverflow).await;
}

#[tokio::test]
async fn initial_provider_stream_overflow_preserves_failure_without_summary_or_retry() {
    assert_initial_provider_overflow(Reply::StreamOverflow).await;
}

async fn assert_old_history_overflow_recovery(reply: Reply, second_reply: Reply) {
    let history = old_history();
    let projection = summarized_history(&history);
    let mut request = request(history.clone());
    let estimate = estimate(&request);
    let budget = budget(estimate.input_tokens * 3, true);
    assert_eq!(budget.check(estimate).unwrap(), ContextCheck::Within);
    request.context_budget = Some(budget);
    let hook = ScriptedCompaction::new(SummaryReply::Projection(projection.clone()));
    request.context_compaction_hook = Some(hook.clone());
    let provider = ScriptedProvider::new([reply, second_reply]);
    let (result, events) = run(&provider, request).await;
    let outcome = result.unwrap();
    match second_reply {
        Reply::Text => assert_success(&outcome),
        Reply::SetupOverflow | Reply::StreamOverflow => assert_overflow_outcome(&outcome),
        Reply::PartialTextStreamOverflow => {
            assert_overflow_outcome_with_text(&outcome, PARTIAL_ANSWER)
        }
    }
    assert_hook_received(&hook, &history, budget);
    let sent = provider.requests();
    assert_eq!(sent.len(), 2);
    assert_history_sent(&sent[0], &history);
    assert_history_sent(&sent[1], &projection);
    assert_ne!(sent[0].messages, sent[1].messages);
    assert_summary_lifecycle(&events, ContextCompactionPhase::Succeeded);
}

#[tokio::test]
async fn old_history_provider_setup_overflow_recovers_once_with_a_smaller_projection() {
    assert_old_history_overflow_recovery(Reply::SetupOverflow, Reply::Text).await;
}

#[tokio::test]
async fn old_history_provider_stream_overflow_recovers_once_with_a_smaller_projection() {
    assert_old_history_overflow_recovery(Reply::StreamOverflow, Reply::Text).await;
}

#[tokio::test]
async fn repeated_provider_overflow_does_not_start_another_recovery() {
    assert_old_history_overflow_recovery(Reply::SetupOverflow, Reply::SetupOverflow).await;
    assert_old_history_overflow_recovery(Reply::StreamOverflow, Reply::StreamOverflow).await;
}

#[tokio::test]
async fn forced_recovery_timeout_is_terminal_even_when_local_input_is_within_budget() {
    for reply in [Reply::SetupOverflow, Reply::StreamOverflow] {
        let history = old_history();
        let mut request = request(history.clone());
        let estimate = estimate(&request);
        let budget = budget(estimate.input_tokens * 3, true);
        assert_eq!(budget.check(estimate).unwrap(), ContextCheck::Within);
        request.context_budget = Some(budget);
        let hook =
            ScriptedCompaction::new(SummaryReply::Failure(ContextRuntimeError::SummaryTimedOut));
        request.context_compaction_hook = Some(hook.clone());
        let provider = ScriptedProvider::new([reply]);
        let (result, events) = run(&provider, request).await;
        assert!(matches!(
            result,
            Err(VegaError::Context(ContextRuntimeError::SummaryTimedOut))
        ));
        assert_eq!(provider.requests().len(), 1);
        assert_history_sent(&provider.requests()[0], &history);
        assert_hook_received(&hook, &history, budget);
        assert_summary_lifecycle(&events, ContextCompactionPhase::Failed);
        assert_eq!(
            summary_statuses(&events)[1].failure,
            Some(ContextCompactionStatusFailure::TimedOut)
        );
    }
}

async fn assert_forced_projection_rejected(
    history: Vec<ChatMessage>,
    projection: Vec<ChatMessage>,
    budget: ContextBudget,
) {
    for reply in [Reply::SetupOverflow, Reply::StreamOverflow] {
        let mut initial = request(history.clone());
        initial.replay_frame = Some(ReplayFrame::new(2, ReasoningReplayPolicy::Unknown));
        let original_estimate = estimate(&initial);
        assert_eq!(
            budget.check(original_estimate).unwrap(),
            ContextCheck::Within
        );
        initial.context_budget = Some(budget);
        let mut projected = request(projection.clone());
        projected.replay_frame = initial.replay_frame.clone();
        let compacted_estimate = estimate(&projected);
        let hook = ScriptedCompaction::new(SummaryReply::Projection(projection.clone()));
        initial.context_compaction_hook = Some(hook.clone());
        let provider = ScriptedProvider::new([reply]);
        let (result, events) = run(&provider, initial).await;
        assert!(
            matches!(result, Err(VegaError::Context(ContextRuntimeError::ResultOverLimit { estimated_tokens, target_tokens }))
            if estimated_tokens == compacted_estimate.input_tokens && target_tokens == budget.target_tokens().unwrap()),
            "unexpected forced recovery result: {result:?}"
        );
        assert_eq!(provider.requests().len(), 1);
        assert_history_sent(&provider.requests()[0], &history);
        assert_hook_received(&hook, &history, budget);
        assert_eq!(hook.requests()[0].estimate, original_estimate);
        assert_summary_lifecycle(&events, ContextCompactionPhase::Failed);
        assert_eq!(
            summary_statuses(&events)[1].failure,
            Some(ContextCompactionStatusFailure::OverLimit)
        );
        assert!(events.iter().any(
            |event| matches!(event, RuntimeEvent::ContextAccountingUpdated(decision)
            if decision.stage == ContextAccountingStage::PostSummaryTarget
                && decision.predicted_input == compacted_estimate.input_tokens
                && decision.input_budget == budget.input_budget())
        ));
    }
}

#[tokio::test]
async fn forced_recovery_rejects_unchanged_and_larger_projections_below_target() {
    let history = old_history();
    let mut initial = request(history.clone());
    initial.replay_frame = Some(ReplayFrame::new(2, ReasoningReplayPolicy::Unknown));
    let original_estimate = estimate(&initial);
    let budget = budget(original_estimate.input_tokens * 3, true);
    let mut larger = history.clone();
    larger[0]
        .content
        .push_str(&"Additional synthetic information. ".repeat(100));
    for projection in [history.clone(), larger] {
        let mut projected = request(projection.clone());
        projected.replay_frame = initial.replay_frame.clone();
        let compacted_estimate = estimate(&projected);
        if projection == history {
            assert_eq!(
                project_model_messages(&assembled_messages(&projected)).unwrap(),
                project_model_messages(&assembled_messages(&initial)).unwrap()
            );
            assert_eq!(compacted_estimate, original_estimate);
        } else {
            assert!(compacted_estimate.input_tokens > original_estimate.input_tokens);
        }
        assert!(compacted_estimate.input_tokens < budget.target_tokens().unwrap());
        assert_forced_projection_rejected(history.clone(), projection, budget).await;
    }
}

#[tokio::test]
async fn forced_recovery_rejects_reduced_projection_still_above_target() {
    let history = old_history();
    let mut initial = request(history.clone());
    initial.replay_frame = Some(ReplayFrame::new(2, ReasoningReplayPolicy::Unknown));
    let original_estimate = estimate(&initial);
    let budget = budget(original_estimate.input_tokens * 4 / 3, true);
    let mut projection = history.clone();
    projection[0].content = "Earlier synthetic information. ".repeat(3800);
    let mut projected = request(projection.clone());
    projected.replay_frame = initial.replay_frame.clone();
    let compacted_estimate = estimate(&projected);
    assert!(compacted_estimate.input_tokens < original_estimate.input_tokens);
    assert!(compacted_estimate.input_tokens > budget.target_tokens().unwrap());
    assert!(compacted_estimate.input_tokens < budget.input_budget());
    assert_forced_projection_rejected(history, projection, budget).await;
}

#[tokio::test]
async fn old_history_provider_overflow_without_a_hook_preserves_failure_without_retry() {
    for reply in [Reply::SetupOverflow, Reply::StreamOverflow] {
        let history = old_history();
        let mut request = request(history.clone());
        let estimate = estimate(&request);
        let budget = budget(estimate.input_tokens * 3, true);
        assert_eq!(budget.check(estimate).unwrap(), ContextCheck::Within);
        request.context_budget = Some(budget);
        let provider = ScriptedProvider::new([reply]);
        let (result, events) = run(&provider, request).await;
        assert_overflow_outcome(&result.unwrap());
        assert_eq!(provider.requests().len(), 1);
        assert_history_sent(&provider.requests()[0], &history);
        assert_no_summary(&events);
    }
}

#[tokio::test]
async fn old_history_partial_stream_overflow_preserves_text_and_error_without_recovery() {
    let history = old_history();
    let mut request = request(history.clone());
    let estimate = estimate(&request);
    let budget = budget(estimate.input_tokens * 3, true);
    assert_eq!(budget.check(estimate).unwrap(), ContextCheck::Within);
    request.context_budget = Some(budget);
    let hook = ScriptedCompaction::new(SummaryReply::Failure(ContextRuntimeError::SourceChanged));
    request.context_compaction_hook = Some(hook.clone());
    let provider = ScriptedProvider::new([Reply::PartialTextStreamOverflow]);
    let (result, events) = run(&provider, request).await;
    assert_overflow_outcome_with_text(&result.unwrap(), PARTIAL_ANSWER);
    assert_eq!(provider.requests().len(), 1);
    assert_history_sent(&provider.requests()[0], &history);
    assert!(hook.requests().is_empty());
    assert_no_summary(&events);
}

async fn summary_failure_run(
    error: ContextRuntimeError,
    over_limit: bool,
) -> (
    Result<AgentOutcome, VegaError>,
    Vec<RuntimeEvent>,
    ScriptedProvider,
) {
    let history = old_history();
    let mut request = request(history.clone());
    let budget = if over_limit {
        budget(estimate(&request).input_tokens - 1, true)
    } else {
        threshold_budget(&request)
    };
    request.context_budget = Some(budget);
    let hook = ScriptedCompaction::new(SummaryReply::Failure(error));
    request.context_compaction_hook = Some(hook.clone());
    let provider = ScriptedProvider::new([Reply::Text]);
    let (result, events) = run(&provider, request).await;
    assert_hook_received(&hook, &history, budget);
    assert_summary_lifecycle(&events, ContextCompactionPhase::Failed);
    (result, events, provider)
}

#[tokio::test]
async fn recoverable_summary_failure_at_trigger_sends_unchanged_history() {
    let (result, events, provider) =
        summary_failure_run(ContextRuntimeError::SummaryTimedOut, false).await;
    assert_success(&result.unwrap());
    assert_eq!(provider.requests().len(), 1);
    assert_history_sent(&provider.requests()[0], &old_history());
    assert_eq!(
        summary_statuses(&events)[1].failure,
        Some(ContextCompactionStatusFailure::TimedOut)
    );
}

#[tokio::test]
async fn recoverable_summary_failure_above_hard_limit_never_sends() {
    let (result, events, provider) =
        summary_failure_run(ContextRuntimeError::SummaryTimedOut, true).await;
    assert!(matches!(
        result,
        Err(VegaError::Context(ContextRuntimeError::SummaryTimedOut))
    ));
    assert!(provider.requests().is_empty());
    assert_eq!(
        summary_statuses(&events)[1].failure,
        Some(ContextCompactionStatusFailure::TimedOut)
    );
}

#[tokio::test]
async fn source_changed_summary_failure_is_not_swallowed_below_hard_limit() {
    let (result, events, provider) =
        summary_failure_run(ContextRuntimeError::SourceChanged, false).await;
    assert!(matches!(
        result,
        Err(VegaError::Context(ContextRuntimeError::SourceChanged))
    ));
    assert!(provider.requests().is_empty());
    assert_eq!(
        summary_statuses(&events)[1].failure,
        Some(ContextCompactionStatusFailure::SourceChanged)
    );
}

async fn assert_cancelled_summary(reply: SummaryReply, terminal: ContextCompactionPhase) {
    let history = old_history();
    let mut request = request(history.clone());
    let budget = threshold_budget(&request);
    request.context_budget = Some(budget);
    let hook = ScriptedCompaction::new(reply);
    request.context_compaction_hook = Some(hook.clone());
    let provider = ScriptedProvider::new([]);
    let (result, events) = run(&provider, request).await;
    let outcome = result.unwrap();
    assert!(outcome.interrupted);
    assert!(!outcome.failed);
    assert!(outcome.final_text.is_empty());
    assert_eq!(&outcome.messages[1..], history);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, RuntimeEvent::Interrupted))
    );
    assert!(provider.requests().is_empty());
    assert_hook_received(&hook, &history, budget);
    assert_summary_lifecycle(&events, terminal);
}

#[tokio::test]
async fn cancellation_during_summary_does_not_send_a_provider_request() {
    assert_cancelled_summary(SummaryReply::Cancelled, ContextCompactionPhase::Cancelled).await;
}

#[tokio::test]
async fn cancellation_after_summary_install_keeps_live_history_and_does_not_send() {
    assert_cancelled_summary(
        SummaryReply::CancelAfterProjection(summarized_history(&old_history())),
        ContextCompactionPhase::Succeeded,
    )
    .await;
}

#[tokio::test]
async fn summary_projection_cannot_replace_system_authority() {
    let history = old_history();
    let mut request = request(history.clone());
    request.context_budget = Some(threshold_budget(&request));
    let mut projection = summarized_history(&history);
    projection.insert(
        0,
        ChatMessage::new(ChatRole::System, "Untrusted summary instruction"),
    );
    request.context_compaction_hook = Some(ScriptedCompaction::new(SummaryReply::Projection(
        projection,
    )));
    let provider = ScriptedProvider::new([]);
    let (result, events) = run(&provider, request).await;
    assert!(matches!(
        result,
        Err(VegaError::Context(
            ContextRuntimeError::SystemMessageInResult
        ))
    ));
    assert!(provider.requests().is_empty());
    assert_summary_lifecycle(&events, ContextCompactionPhase::Failed);
    assert_eq!(
        summary_statuses(&events)[1].failure,
        Some(ContextCompactionStatusFailure::InvalidSummary)
    );
}

#[test]
fn image_decoder_keeps_format_decode_dimension_and_single_image_byte_limits() {
    let small = solid_image(2, 2);
    assert_eq!(
        (small.width(), small.height(), small.mime_type()),
        (2, 2, "image/png")
    );
    assert!(ImageAttachment::from_bytes(Vec::new()).is_err());
    assert!(ImageAttachment::from_bytes(b"not an image".to_vec()).is_err());
    assert!(ImageAttachment::from_bytes(b"GIF89a".to_vec()).is_err());
    assert!(
        ImageAttachment::from_bytes(small.bytes()[..small.bytes().len() / 2].to_vec()).is_err()
    );
    let oversized_geometry = png_bytes(
        4001,
        4000,
        &vec![0; 4001 * 4000],
        CompressionType::Fast,
        FilterType::NoFilter,
    );
    assert!(ImageAttachment::from_bytes(oversized_geometry).is_err());
    let oversized_bytes = noise_png(3000, 3000);
    assert!(oversized_bytes.len() > 8 * 1024 * 1024);
    assert_eq!(
        ImageAttachment::from_bytes(oversized_bytes).unwrap_err().0,
        "image must be 1 byte to 8 MiB"
    );
    assert!(large_valid_image().bytes().len() < 8 * 1024 * 1024);
    let rgb = image::RgbImage::from_pixel(2, 2, image::Rgb([32, 64, 96]));
    for (format, mime) in [
        (image::ImageFormat::Jpeg, "image/jpeg"),
        (image::ImageFormat::WebP, "image/webp"),
    ] {
        let mut encoded = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(rgb.clone())
            .write_to(&mut encoded, format)
            .unwrap();
        let attachment = ImageAttachment::from_bytes(encoded.into_inner()).unwrap();
        assert_eq!(attachment.mime_type(), mime);
        assert_eq!((attachment.width(), attachment.height()), (2, 2));
    }
}

async fn assert_valid_images_reach_provider(history: Vec<ChatMessage>) {
    let mut request = request(history.clone());
    request.replay_frame = Some(ReplayFrame::new(1, ReasoningReplayPolicy::Unknown));
    let provider = ScriptedProvider::new([Reply::Text]);
    let (result, events) = run(&provider, request).await;
    assert_success(&result.unwrap());
    assert_eq!(provider.requests().len(), 1);
    assert_history_sent(&provider.requests()[0], &history);
    assert_no_summary(&events);
}

async fn assert_invalid_images_rejected(history: Vec<ChatMessage>) {
    let provider = ScriptedProvider::new([]);
    let (result, events) = run(&provider, request(history)).await;
    assert!(
        matches!(result, Err(VegaError::Io(ref error)) if error.kind() == std::io::ErrorKind::InvalidInput),
        "unexpected validation result: {result:?}"
    );
    assert!(provider.requests().is_empty());
    assert_no_summary(&events);
}

#[tokio::test]
async fn four_images_per_user_message_are_accepted_but_five_are_rejected() {
    let image = solid_image(1, 1);
    assert_valid_images_reach_provider(vec![user_with_images(vec![image.clone(); 4])]).await;
    assert_invalid_images_rejected(vec![user_with_images(vec![image; 5])]).await;
}

#[tokio::test]
async fn user_turn_image_bytes_remain_limited_to_sixteen_mebibytes() {
    let image = large_valid_image();
    assert!(image.bytes().len() * 3 < 16 * 1024 * 1024);
    assert!(image.bytes().len() * 4 > 16 * 1024 * 1024);
    assert_valid_images_reach_provider(vec![user_with_images(vec![image.clone(); 3])]).await;
    assert_invalid_images_rejected(vec![user_with_images(vec![image; 4])]).await;
}

#[tokio::test]
async fn history_image_bytes_remain_limited_to_thirty_two_mebibytes() {
    let image = large_valid_image();
    assert!(image.bytes().len() * 6 < 32 * 1024 * 1024);
    assert!(image.bytes().len() * 7 > 32 * 1024 * 1024);
    let history = vec![user_with_images(vec![image.clone()]); 6];
    assert_valid_images_reach_provider(history.clone()).await;
    let mut excessive = history;
    excessive.push(user_with_images(vec![image]));
    assert_invalid_images_rejected(excessive).await;
}

#[tokio::test]
async fn tool_observation_images_keep_two_image_and_eight_mebibyte_limits() {
    let small = solid_image(1, 1);
    assert_valid_images_reach_provider(completed_group(
        "valid",
        "observations",
        vec![small.clone(); 2],
    ))
    .await;
    assert_invalid_images_rejected(completed_group("count", "observations", vec![small; 3])).await;
    let large = large_valid_image();
    assert_valid_images_reach_provider(completed_group(
        "bytes-valid",
        "observation",
        vec![large.clone()],
    ))
    .await;
    assert_invalid_images_rejected(completed_group("bytes", "observations", vec![large; 2])).await;
}

#[tokio::test]
async fn images_on_system_assistant_or_unbound_tool_messages_are_rejected() {
    let image = solid_image(1, 1);
    for role in [ChatRole::System, ChatRole::Assistant, ChatRole::Tool] {
        let mut message = ChatMessage::new(role, "Invalid image owner");
        message.images.push(image.clone());
        assert_invalid_images_rejected(vec![message]).await;
    }
    let mut empty_id = ChatMessage::tool_result("", "Invalid image owner");
    empty_id.images.push(image);
    assert_invalid_images_rejected(vec![empty_id]).await;
}
