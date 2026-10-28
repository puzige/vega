use super::*;
use std::collections::VecDeque;
use std::sync::Mutex;
use vega_store::context_compaction::{
    ContextCheckpoint, ContextSource, latest_checkpoint, load_source,
};
use vega_store::context_projection::{ProjectionFragment, ProjectionStrategy};

type TestResult = Result<(), Box<dyn std::error::Error>>;

pub(super) enum SummaryReply {
    Text(String),
    Error(VegaError),
    Cancel,
}

pub(super) struct ScriptedSummaryProvider {
    replies: Mutex<VecDeque<SummaryReply>>,
    requests: Mutex<Vec<ChatRequest>>,
}

impl ScriptedSummaryProvider {
    pub(super) fn new(replies: impl IntoIterator<Item = SummaryReply>) -> Self {
        Self {
            replies: Mutex::new(replies.into_iter().collect()),
            requests: Mutex::new(Vec::new()),
        }
    }

    pub(super) fn requests(&self) -> Vec<ChatRequest> {
        assert!(self.replies.lock().unwrap().is_empty());
        self.requests.lock().unwrap().clone()
    }
}

impl Provider for ScriptedSummaryProvider {
    fn chat_stream(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> futures::future::BoxFuture<'static, Result<vega_runtime::EventStream, VegaError>> {
        assert!(request.tools.is_empty());
        self.requests.lock().unwrap().push(request);
        let reply = self
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected summary request");
        Box::pin(async move {
            let usage = Ok(ProviderEvent::Usage {
                input: 100,
                output: 10,
                cache_read: Some(0),
                cache_write: Some(0),
            });
            let events = match reply {
                SummaryReply::Text(summary) => vec![
                    usage,
                    Ok(ProviderEvent::TextDelta(summary)),
                    Ok(ProviderEvent::Done {
                        stop_reason: StopReason::End,
                    }),
                ],
                SummaryReply::Error(error) => vec![usage, Err(error)],
                SummaryReply::Cancel => {
                    let stream: vega_runtime::EventStream = Box::pin(
                        futures::stream::iter([usage]).chain(futures::stream::poll_fn(move |_| {
                            cancel.cancel();
                            std::task::Poll::Ready(Some(Err(VegaError::Cancelled)))
                        })),
                    );
                    return Ok(stream);
                }
            };
            let stream: vega_runtime::EventStream = Box::pin(futures::stream::iter(events));
            Ok(stream)
        })
    }
}

fn overflow(gap: Option<u64>) -> VegaError {
    let mut detail = serde_json::json!({"code": "prompt_too_long"});
    if let Some(gap) = gap {
        detail["requested_tokens"] = (100_000 + gap).into();
        detail["max_context_tokens"] = 100_000.into();
    }
    VegaError::Provider {
        status: Some(400),
        message: serde_json::json!({"error": detail}).to_string(),
        retryable: false,
    }
}

fn prepare_rounds(store: &Store, count: i64) -> TestResult {
    for round in 3..=count {
        insert_round(store, round)?;
    }
    for round in 1..=count {
        store.conn().execute(
            "UPDATE tool_calls SET output_text=?1 WHERE id=?2",
            (
                format!(
                    "RESULT_{round} {}",
                    "synthetic historical detail ".repeat(300)
                ),
                format!("call-{round}"),
            ),
        )?;
    }
    Ok(())
}

fn visible_history(
    source: &ContextSource,
    checkpoint: Option<&ContextCheckpoint>,
) -> Result<Vec<ChatMessage>, Box<dyn std::error::Error>> {
    let mut visible = source.clone();
    for message in &mut visible.messages {
        if message.status == "streaming" {
            message.status = "done".into();
        }
    }
    Ok(
        crate::agent::pipeline::history_from_context_source_with_checkpoint(
            &visible, checkpoint, "__none__",
        )?,
    )
}

fn request(
    source: &ContextSource,
    messages: Vec<ChatMessage>,
    trigger: ContextCompactionTrigger,
    owner: Option<&str>,
) -> Result<ContextCompactionRequest, Box<dyn std::error::Error>> {
    let budget = ContextBudget::new(200_000, 32_000, true)?;
    Ok(ContextCompactionRequest {
        trigger,
        system_prompt: "system".into(),
        estimate: estimate_chat_context("system", &messages, &[])?,
        messages,
        tools: Vec::new(),
        budget,
        target_tokens: budget.target_tokens()?,
        source_version: source.source_version,
        source_fingerprint: Some(source.fingerprint.clone()),
        require_source_fence: true,
        source_owner_id: owner.map(str::to_owned),
    })
}

fn input_history(request: &ChatRequest) -> &[ChatMessage] {
    &request.messages[1..request.messages.len() - 1]
}

fn calls(messages: &[ChatMessage]) -> Vec<&str> {
    messages
        .iter()
        .flat_map(|message| &message.tool_calls)
        .map(|call| call.id.as_str())
        .collect()
}

fn text(messages: &[ChatMessage]) -> String {
    messages
        .iter()
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn ordered(messages: &[ChatMessage], markers: &[&str]) {
    let text = text(messages);
    let positions = markers
        .iter()
        .map(|marker| text.find(marker).expect("ordered marker absent"))
        .collect::<Vec<_>>();
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "unexpected order: {text}"
    );
}

fn reload_matches(
    store: &Store,
    source: &ContextSource,
    result: &ContextCompactionResult,
) -> TestResult {
    let checkpoint =
        latest_checkpoint(store.conn(), "thread", "model")?.ok_or("checkpoint missing")?;
    let reloaded = visible_history(source, Some(&checkpoint))?;
    assert!(durable_messages_equal(
        &result.messages.iter().collect::<Vec<_>>(),
        &reloaded.iter().collect::<Vec<_>>()
    ));
    assert_eq!(load_source(store.conn(), "thread")?, *source);
    Ok(())
}

#[tokio::test]
async fn summary_overflow_moves_complete_response_boundary_with_and_without_gap() -> TestResult {
    for known_gap in [false, true] {
        let (_directory, path, store) = source_fixture()?;
        prepare_rounds(&store, 5)?;
        store.conn().execute(
            "UPDATE tool_call_rounds SET response_id='response-1' WHERE call_id='call-2'",
            [],
        )?;
        let source = load_source(store.conn(), "thread")?;
        let history = visible_history(&source, None)?;
        let fourth = history
            .iter()
            .position(|message| message.tool_calls.iter().any(|call| call.id == "call-4"))
            .ok_or("fourth response missing")?;
        let fifth = history
            .iter()
            .position(|message| message.tool_calls.iter().any(|call| call.id == "call-5"))
            .ok_or("fifth response missing")?;
        let gap = known_gap.then(|| {
            estimate_chat_context("", &history[fourth..fifth], &[])
                .unwrap()
                .input_tokens
                + 1
        });
        let provider = ScriptedSummaryProvider::new([
            SummaryReply::Error(overflow(gap)),
            SummaryReply::Text("RECOVERED".into()),
        ]);
        let result =
            ConversationCompactionHook::new(&provider, path, "thread", "model", None, None)
                .compact(
                    request(
                        &source,
                        history.clone(),
                        ContextCompactionTrigger::InputOverflow {
                            token_gap: None,
                            deeper: false,
                        },
                        Some("owner"),
                    )?,
                    CancellationToken::new(),
                )
                .await
                .map_err(|failure| *failure.error)?;
        let requests = provider.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            calls(input_history(&requests[0])),
            ["call-1", "call-2", "call-3", "call-4"]
        );
        assert_eq!(
            calls(input_history(&requests[1])),
            if known_gap {
                vec!["call-1", "call-2"]
            } else {
                vec!["call-1", "call-2", "call-3"]
            }
        );
        assert_eq!(
            calls(&result.messages),
            if known_gap {
                vec!["call-3", "call-4", "call-5"]
            } else {
                vec!["call-4", "call-5"]
            }
        );
        assert!(
            estimate_wire_context(&requests[1].messages, &[])?.input_tokens
                < estimate_wire_context(&requests[0].messages, &[])?.input_tokens
        );
        assert!(
            estimate_chat_context("system", &result.messages, &[])?.input_tokens
                < estimate_chat_context("system", &history, &[])?.input_tokens
        );
        assert_eq!(
            result.reactive_retained_groups,
            Some(if known_gap { 3 } else { 2 })
        );
        assert_eq!(result.usages.len(), 2);
        assert!(result.usage_complete);
        let checkpoint =
            latest_checkpoint(store.conn(), "thread", "model")?.ok_or("checkpoint absent")?;
        assert!(
            checkpoint
                .projection_plan
                .as_ref()
                .unwrap()
                .omitted
                .is_empty()
        );
        reload_matches(&store, &source, &result)?;
    }
    Ok(())
}

#[tokio::test]
async fn initial_gap_keeps_multiple_groups_and_deeper_reduces_the_retained_tail() -> TestResult {
    let (_directory, path, store) = source_fixture()?;
    prepare_rounds(&store, 5)?;
    let source = load_source(store.conn(), "thread")?;
    let history = visible_history(&source, None)?;
    let fifth = history
        .iter()
        .position(|message| message.tool_calls.iter().any(|call| call.id == "call-5"))
        .ok_or("fifth response missing")?;
    let gap = estimate_chat_context("", &history[fifth..], &[])?.input_tokens + 1;
    let provider = ScriptedSummaryProvider::new([
        SummaryReply::Text("FIRST_SUMMARY".into()),
        SummaryReply::Text("DEEPER_SUMMARY".into()),
    ]);
    let hook = ConversationCompactionHook::new(&provider, path, "thread", "model", None, None);
    let first = hook
        .compact(
            request(
                &source,
                history,
                ContextCompactionTrigger::InputOverflow {
                    token_gap: Some(gap),
                    deeper: false,
                },
                Some("owner"),
            )?,
            CancellationToken::new(),
        )
        .await
        .map_err(|failure| *failure.error)?;
    assert_eq!(first.reactive_retained_groups, Some(2));
    assert_eq!(calls(&first.messages), ["call-4", "call-5"]);
    let second = hook
        .compact(
            request(
                &source,
                first.messages.clone(),
                ContextCompactionTrigger::InputOverflow {
                    token_gap: Some(u64::MAX - 100_000),
                    deeper: true,
                },
                Some("owner"),
            )?,
            CancellationToken::new(),
        )
        .await
        .map_err(|failure| *failure.error)?;
    assert_eq!(second.reactive_retained_groups, Some(1));
    assert_eq!(calls(&second.messages), ["call-5"]);
    assert!(
        estimate_chat_context("system", &second.messages, &[])?.input_tokens
            < estimate_chat_context("system", &first.messages, &[])?.input_tokens
    );
    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(calls(input_history(&requests[1])), ["call-4"]);
    assert!(!text(input_history(&requests[1])).contains("RESULT_1"));
    ordered(input_history(&requests[1]), &["FIRST_SUMMARY", "RESULT_4"]);
    reload_matches(&store, &source, &second)?;
    Ok(())
}

#[tokio::test]
async fn partial_then_reactive_retry_preserves_summary_and_retained_source_order() -> TestResult {
    use crate::types::{PartialCompactionDirection as Direction, PartialCompactionSelection};
    let (_directory, path, store) = source_fixture()?;
    prepare_rounds(&store, 2)?;
    store
        .conn()
        .execute("UPDATE messages SET status='done' WHERE id='owner'", [])?;
    let mut row = messages::find(store.conn(), "task")?.ok_or("task missing")?;
    for (id, seq, marker) in [("middle", 5, "SOURCE_MIDDLE"), ("tail", 6, "SOURCE_TAIL")] {
        row.id = id.into();
        row.seq = seq;
        row.role = "assistant".into();
        row.content = format!("{marker} {}", "synthetic history ".repeat(400));
        messages::insert(store.conn(), &row)?;
    }
    let source = load_source(store.conn(), "thread")?;
    let provider = ScriptedSummaryProvider::new([
        SummaryReply::Text("SUMMARY_TAIL".into()),
        SummaryReply::Text("SUMMARY_TASK".into()),
        SummaryReply::Error(overflow(None)),
        SummaryReply::Text("REACTIVE_SUMMARY".into()),
    ]);
    let mut history = visible_history(&source, None)?;
    for (id, direction) in [("tail", Direction::From), ("owner", Direction::UpTo)] {
        let result =
            ConversationCompactionHook::new(&provider, path.clone(), "thread", "model", None, None)
                .with_partial_selection(PartialCompactionSelection {
                    message_id: id.into(),
                    source_fingerprint: source.fingerprint.clone(),
                    direction,
                })
                .compact(
                    request(&source, history, ContextCompactionTrigger::Manual, None)?,
                    CancellationToken::new(),
                )
                .await
                .map_err(|failure| *failure.error)?;
        history = result.messages;
    }
    let previous =
        latest_checkpoint(store.conn(), "thread", "model")?.ok_or("partial checkpoint missing")?;
    let result = ConversationCompactionHook::new(&provider, path, "thread", "model", None, None)
        .compact(
            request(
                &source,
                history,
                ContextCompactionTrigger::InputOverflow {
                    token_gap: None,
                    deeper: false,
                },
                None,
            )?,
            CancellationToken::new(),
        )
        .await
        .map_err(|failure| *failure.error)?;
    let requests = provider.requests();
    assert_eq!(requests.len(), 4);
    ordered(
        input_history(&requests[2]),
        &["SUMMARY_TASK", "RESULT_1", "RESULT_2", "SOURCE_MIDDLE"],
    );
    ordered(
        input_history(&requests[3]),
        &["SUMMARY_TASK", "RESULT_1", "RESULT_2"],
    );
    assert!(!text(input_history(&requests[3])).contains("SOURCE_MIDDLE"));
    for input in &requests[2..] {
        assert!(!text(input_history(input)).contains("SOURCE_TAIL"));
        assert!(!text(input_history(input)).contains("Continue implementation; no local tests."));
    }
    ordered(
        &result.messages,
        &["REACTIVE_SUMMARY", "SOURCE_MIDDLE", "SUMMARY_TAIL"],
    );
    assert!(!text(&result.messages).contains("RESULT_1"));
    let checkpoint =
        latest_checkpoint(store.conn(), "thread", "model")?.ok_or("reactive checkpoint missing")?;
    let plan = checkpoint.projection_plan.as_ref().unwrap();
    assert_eq!(plan.strategy, ProjectionStrategy::Reactive);
    assert_eq!(
        plan.omitted,
        previous.projection_plan.as_ref().unwrap().omitted
    );
    assert!(
        plan.fragments
            .iter()
            .any(|fragment| matches!(fragment, ProjectionFragment::PreviousSummary { .. }))
    );
    reload_matches(&store, &source, &result)?;
    Ok(())
}

#[tokio::test]
async fn last_resort_omits_only_oldest_complete_groups_and_protects_pending_user() -> TestResult {
    let (_directory, path, store) = source_fixture()?;
    prepare_rounds(&store, 6)?;
    store
        .conn()
        .execute("UPDATE messages SET status='done' WHERE id='owner'", [])?;
    let mut row = messages::find(store.conn(), "task")?.ok_or("task missing")?;
    row.id = "pending".into();
    row.seq = 20;
    row.content = "PENDING_REQUEST: keep this exact user request".into();
    messages::insert(store.conn(), &row)?;
    let source = load_source(store.conn(), "thread")?;
    let provider = ScriptedSummaryProvider::new([
        SummaryReply::Error(overflow(None)),
        SummaryReply::Error(overflow(None)),
        SummaryReply::Text("RECOVERED_WITH_OMISSION".into()),
    ]);
    let result = ConversationCompactionHook::new(&provider, path, "thread", "model", None, None)
        .with_full_on_threshold(true)
        .compact(
            request(
                &source,
                visible_history(&source, None)?,
                ContextCompactionTrigger::Threshold,
                None,
            )?,
            CancellationToken::new(),
        )
        .await
        .map_err(|failure| *failure.error)?;
    let requests = provider.requests();
    assert_eq!(requests.len(), 3);
    assert!(!text(input_history(&requests[1])).contains("PENDING_REQUEST"));
    assert!(!text(input_history(&requests[2])).contains("PENDING_REQUEST"));
    assert_eq!(result.messages.last().unwrap().content, row.content);
    let checkpoint =
        latest_checkpoint(store.conn(), "thread", "model")?.ok_or("checkpoint missing")?;
    let plan = checkpoint.projection_plan.as_ref().unwrap();
    assert_eq!(plan.omitted.len(), 1);
    let omitted = plan.omitted[0].project(&source)?;
    assert_eq!(
        omitted
            .messages
            .iter()
            .map(|message| message.id.as_str())
            .collect::<Vec<_>>(),
        ["task", "owner"]
    );
    assert_eq!(
        omitted
            .tool_calls
            .iter()
            .map(|call| call.id.as_str())
            .collect::<Vec<_>>(),
        ["call-1"]
    );
    assert_eq!(
        calls(input_history(&requests[2])),
        ["call-2", "call-3", "call-4", "call-5", "call-6"]
    );
    assert!(text(&result.messages).contains("NOT covered by the following summary"));
    plan.validate_install(&source)?;
    reload_matches(&store, &source, &result)?;
    Ok(())
}

#[tokio::test]
async fn last_resort_stops_after_three_head_truncations_without_installing() -> TestResult {
    let (_directory, path, store) = source_fixture()?;
    prepare_rounds(&store, 10)?;
    let source = load_source(store.conn(), "thread")?;
    let provider =
        ScriptedSummaryProvider::new((0..4).map(|_| SummaryReply::Error(overflow(None))));
    let failure = ConversationCompactionHook::new(&provider, path, "thread", "model", None, None)
        .with_full_on_threshold(true)
        .compact(
            request(
                &source,
                visible_history(&source, None)?,
                ContextCompactionTrigger::Threshold,
                Some("owner"),
            )?,
            CancellationToken::new(),
        )
        .await
        .unwrap_err();
    assert!(failure.error.context_input_overflow().is_some());
    let requests = provider.requests();
    assert_eq!(requests.len(), 4);
    let selected = requests
        .iter()
        .map(|request| calls(input_history(request)))
        .collect::<Vec<_>>();
    assert_eq!(
        selected.iter().map(Vec::len).collect::<Vec<_>>(),
        [10, 8, 6, 4]
    );
    assert_eq!(selected[3], ["call-7", "call-8", "call-9", "call-10"]);
    assert_eq!(failure.usages.len(), 4);
    assert!(failure.usage_complete);
    assert!(latest_checkpoint(store.conn(), "thread", "model")?.is_none());
    assert_eq!(load_source(store.conn(), "thread")?, source);
    Ok(())
}

#[tokio::test]
async fn generic_summary_error_and_cancellation_never_enter_omission_recovery() -> TestResult {
    for cancelled in [false, true] {
        let (_directory, path, store) = source_fixture()?;
        prepare_rounds(&store, 5)?;
        let source = load_source(store.conn(), "thread")?;
        let provider = ScriptedSummaryProvider::new([if cancelled {
            SummaryReply::Cancel
        } else {
            SummaryReply::Error(VegaError::Provider {
                status: Some(503),
                message: "unavailable".into(),
                retryable: false,
            })
        }]);
        let failure =
            ConversationCompactionHook::new(&provider, path, "thread", "model", None, None)
                .compact(
                    request(
                        &source,
                        visible_history(&source, None)?,
                        ContextCompactionTrigger::InputOverflow {
                            token_gap: None,
                            deeper: false,
                        },
                        Some("owner"),
                    )?,
                    CancellationToken::new(),
                )
                .await
                .unwrap_err();
        assert_eq!(matches!(*failure.error, VegaError::Cancelled), cancelled);
        assert_eq!(provider.requests().len(), 1);
        assert_eq!(failure.usages.len(), 1);
        assert!(failure.usage_complete);
        assert!(latest_checkpoint(store.conn(), "thread", "model")?.is_none());
        assert_eq!(load_source(store.conn(), "thread")?, source);
    }
    Ok(())
}

#[tokio::test]
async fn reactive_and_deeper_restore_historical_images_without_duplicate_references() -> TestResult
{
    let (_directory, path, store) = source_fixture()?;
    prepare_rounds(&store, 5)?;
    let mut encoded = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(1, 1).write_to(&mut encoded, image::ImageFormat::Png)?;
    let encoded = encoded.into_inner();
    vega_store::image_attachments::insert(store.conn(), "task", 0, &encoded)?;
    let source = load_source(store.conn(), "thread")?;
    let history = visible_history(&source, None)?;
    let fifth = history
        .iter()
        .position(|message| message.tool_calls.iter().any(|call| call.id == "call-5"))
        .ok_or("fifth response missing")?;
    let gap = estimate_chat_context("", &history[fifth..], &[])?.input_tokens + 1;
    let provider = ScriptedSummaryProvider::new([
        SummaryReply::Text("IMAGE_SUMMARY".into()),
        SummaryReply::Text("DEEPER_IMAGE_SUMMARY".into()),
    ]);
    let hook = ConversationCompactionHook::new(&provider, path, "thread", "model", None, None);
    let mut messages = history;
    for deeper in [false, true] {
        let result = hook
            .compact(
                request(
                    &source,
                    messages,
                    ContextCompactionTrigger::InputOverflow {
                        token_gap: Some(gap),
                        deeper,
                    },
                    Some("owner"),
                )?,
                CancellationToken::new(),
            )
            .await
            .map_err(|failure| *failure.error)?;
        let images = result
            .messages
            .iter()
            .flat_map(|message| &message.images)
            .collect::<Vec<_>>();
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].bytes(), encoded.as_slice());
        assert_eq!(
            result.reactive_retained_groups,
            Some(if deeper { 1 } else { 2 })
        );
        let checkpoint =
            latest_checkpoint(store.conn(), "thread", "model")?.ok_or("checkpoint missing")?;
        assert_eq!(
            checkpoint
                .projection_plan
                .as_ref()
                .unwrap()
                .fragments
                .iter()
                .filter(|fragment| matches!(fragment, ProjectionFragment::HistoricalImages { .. }))
                .count(),
            1
        );
        reload_matches(&store, &source, &result)?;
        messages = result.messages;
    }
    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    for request in &requests {
        assert!(
            request
                .messages
                .iter()
                .all(|message| message.images.is_empty())
        );
        assert!(text(input_history(request)).contains("visual data omitted from summarizer input"));
    }
    Ok(())
}

#[tokio::test]
async fn reactive_nonshrinking_summary_cannot_install_a_checkpoint() -> TestResult {
    let (_directory, path, store) = source_fixture()?;
    let source = load_source(store.conn(), "thread")?;
    let provider = ScriptedSummaryProvider::new(
        (0..4).map(|_| SummaryReply::Text("NOT_SMALLER ".repeat(500))),
    );
    let failure = ConversationCompactionHook::new(&provider, path, "thread", "model", None, None)
        .compact(
            request(
                &source,
                visible_history(&source, None)?,
                ContextCompactionTrigger::InputOverflow {
                    token_gap: None,
                    deeper: false,
                },
                Some("owner"),
            )?,
            CancellationToken::new(),
        )
        .await
        .unwrap_err();
    assert!(matches!(
        *failure.error,
        VegaError::Context(ContextRuntimeError::SummaryProjectionInvalid)
    ));
    assert_eq!(provider.requests().len(), 4);
    assert!(latest_checkpoint(store.conn(), "thread", "model")?.is_none());
    assert_eq!(load_source(store.conn(), "thread")?, source);
    Ok(())
}
