use super::*;

fn bounded_thinking() -> Vec<ProviderEvent> {
    (0..4)
        .map(|_| ProviderEvent::ThinkingDelta("r".repeat(REASONING_DELTA_MAX_BYTES)))
        .collect()
}

fn tool_round(index: usize, text: &str, replay: bool) -> Vec<ProviderEvent> {
    let mut events = bounded_thinking();
    if replay {
        events.push(ProviderEvent::ReasoningReplay(vec![serde_json::json!({
            "id": format!("reasoning-{index}"),
            "type": "reasoning",
            "encrypted_content": "q".repeat(240 * 1024)
        })]));
    }
    events.extend([
        ProviderEvent::TextDelta(text.into()),
        ProviderEvent::ToolUse {
            id: format!("call-{index}"),
            name: "read".into(),
            input_json: r#"{"path":"source.txt"}"#.into(),
        },
        ProviderEvent::Done {
            stop_reason: StopReason::ToolUse,
        },
    ]);
    events
}

struct RetainLastRound {
    calls: Arc<AtomicUsize>,
    seen: Arc<Mutex<Vec<ContextCompactionRequest>>>,
}

impl ContextCompactionHook for RetainLastRound {
    fn compact<'a>(
        &'a self,
        request: ContextCompactionRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'a, Result<ContextCompactionResult, ContextCompactionFailure>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.seen.lock().unwrap().push(request.clone());
        let start = request
            .messages
            .iter()
            .rposition(|message| !message.tool_calls.is_empty())
            .unwrap();
        let mut messages = vec![ChatMessage::new(
            ChatRole::User,
            "Task goal: finish the implementation. Progress: prior read rounds completed. Latest correction: preserve tool pairing.",
        )];
        messages.extend_from_slice(&request.messages[start..]);
        Box::pin(async move {
            Ok(ContextCompactionResult {
                messages,
                source_version: request.source_version + 1,
                source_fingerprint: request.source_fingerprint,
                usages: Vec::new(),
                usage_complete: false,
            })
        })
    }
}

#[derive(Clone, Copy)]
enum Boundary {
    Timeout,
    SourceChanged,
    InvalidSystem,
    OverTarget,
    Cancelled,
    ReasoningOverflow,
    ReplayOverflow,
}

struct BoundaryHook(Boundary);

impl ContextCompactionHook for BoundaryHook {
    fn compact<'a>(
        &'a self,
        request: ContextCompactionRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'a, Result<ContextCompactionResult, ContextCompactionFailure>> {
        let behavior = self.0;
        Box::pin(async move {
            let messages = match behavior {
                Boundary::Timeout => {
                    return Err(ContextCompactionFailure::new(
                        VegaError::Context(ContextRuntimeError::SummaryTimedOut),
                        None,
                    ));
                }
                Boundary::SourceChanged => {
                    return Err(ContextCompactionFailure::new(
                        VegaError::Context(ContextRuntimeError::SourceChanged),
                        None,
                    ));
                }
                Boundary::InvalidSystem => vec![ChatMessage::new(ChatRole::System, "invalid")],
                Boundary::OverTarget => request.messages,
                Boundary::Cancelled => {
                    cancel.cancel();
                    vec![ChatMessage::new(ChatRole::User, "summary")]
                }
                Boundary::ReasoningOverflow => {
                    let mut message = ChatMessage::new(ChatRole::Assistant, "");
                    message.reasoning_content = Some("r".repeat(REASONING_TURN_MAX_BYTES + 1));
                    vec![message]
                }
                Boundary::ReplayOverflow => {
                    let mut message = ChatMessage::new(ChatRole::Assistant, "");
                    message.response_reasoning = (0..257).map(|index| serde_json::json!({"type":"reasoning", "id": format!("reasoning-{index}")})).collect();
                    vec![message]
                }
            };
            Ok(ContextCompactionResult {
                messages,
                source_version: request.source_version + 1,
                source_fingerprint: request.source_fingerprint,
                usages: Vec::new(),
                usage_complete: false,
            })
        })
    }
}
