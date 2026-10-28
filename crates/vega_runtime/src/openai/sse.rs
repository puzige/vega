use super::*;
use crate::ProviderProtocolReason;

/// Cancel-aware sleep: cancellation aborts the backoff immediately.
pub(crate) async fn sleep_cancellable(
    delay: Duration,
    cancel: &CancellationToken,
) -> Result<(), VegaError> {
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(VegaError::Cancelled),
        _ = tokio::time::sleep(delay) => Ok(()),
    }
}

/// The raw SSE stream (byte stream wrapped by `eventsource-stream`), boxed so
/// the pipeline state below has a nameable type.
pub(crate) type RawSse =
    Pin<Box<dyn Stream<Item = Result<RawEvent, EventStreamError<reqwest::Error>>> + Send>>;

/// Pipeline state for the SSE → [`ProviderEvent`] stream.
pub(crate) struct SsePipeline {
    pub(crate) inner: RawSse,
    pub(crate) assembler: SseAssembler,
    pub(crate) pending: VecDeque<Result<ProviderEvent, VegaError>>,
    pub(crate) done: bool,
    pub(crate) cancel: CancellationToken,
    recoverable: bool,
}

/// Wraps a successful SSE response into the boxed event stream.
///
/// - chunks are parsed incrementally into events;
/// - `data: [DONE]` (or stream end) flushes buffered tool calls and emits
///   the terminal [`ProviderEvent::Done`];
/// - cancellation tears the stream down immediately with no further events;
/// - errors after stream establishment are terminal (retries only apply to
///   request setup — consumed increments cannot be replayed).
pub(crate) fn event_stream(resp: reqwest::Response, cancel: CancellationToken) -> EventStream {
    event_stream_with_recovery(resp, cancel, false)
}

pub(super) fn event_stream_with_recovery(
    resp: reqwest::Response,
    cancel: CancellationToken,
    recoverable: bool,
) -> EventStream {
    let inner: RawSse = Box::pin(resp.bytes_stream().eventsource());
    let state = SsePipeline {
        inner,
        assembler: SseAssembler::default(),
        pending: VecDeque::new(),
        done: false,
        cancel,
        recoverable,
    };
    let stream = futures::stream::unfold(state, |mut st| async move {
        loop {
            if let Some(item) = st.pending.pop_front() {
                return Some((item, st));
            }
            if st.done {
                return None;
            }
            // 取消与下一个 SSE 事件竞争：取消立即断流且不再产生事件
            let next = tokio::select! {
                biased;
                _ = st.cancel.cancelled() => return None,
                item = st.inner.next() => item,
            };
            match next {
                Some(Ok(event)) => {
                    if event.data.trim() == DONE_SENTINEL {
                        st.done = true;
                        match st.assembler.finalize() {
                            Ok(events) => st.pending.extend(events.into_iter().map(Ok)),
                            Err(error) => st.pending.push_back(Err(error)),
                        }
                    } else {
                        match st.assembler.absorb(&event.data) {
                            Ok(events) => st.pending.extend(events.into_iter().map(Ok)),
                            Err(err) => {
                                st.done = true;
                                st.pending.push_back(Err(err));
                            }
                        }
                    }
                }
                Some(Err(err)) => {
                    st.done = true;
                    st.pending
                        .push_back(Err(map_sse_error(err, st.recoverable)));
                }
                None => {
                    st.done = true;
                    if st.recoverable && st.assembler.finish_reason.is_none() {
                        st.pending
                            .push_back(Err(recovery::incomplete_stream_error()));
                        continue;
                    }
                    match st.assembler.finalize() {
                        Ok(events) => st.pending.extend(events.into_iter().map(Ok)),
                        Err(error) => st.pending.push_back(Err(error)),
                    }
                }
            }
        }
    });
    Box::pin(stream)
}

pub(crate) fn map_sse_error(err: EventStreamError<reqwest::Error>, recoverable: bool) -> VegaError {
    let transport = matches!(err, EventStreamError::Transport(_));
    VegaError::ProviderDiagnostic {
        // The response was already established here, so a byte-stream
        // failure is an interrupted body, not a failed connection.
        kind: if transport {
            ProviderFailureKind::StreamInterrupted
        } else {
            ProviderFailureKind::Protocol
        },
        status: None,
        message: "SSE stream failed".into(),
        retryable: transport && recoverable,
        protocol_reason: (!transport).then_some(ProviderProtocolReason::SseDecode),
        retry_count: None,
        request_id: None,
    }
}

#[derive(Default)]
pub(crate) struct ToolFragment {
    index: Option<u64>,
    id: String,
    name: String,
    arguments: String,
}

#[derive(Default)]
pub(crate) struct SseAssembler {
    tools: Vec<ToolFragment>,
    pub(crate) finish_reason: Option<StopReason>,
    source: Option<&'static str>,
    reasoning: String,
    last_delta: String,
    details: Option<Vec<serde_json::Value>>,
    usage: Option<UsageCounts>,
}

impl fmt::Debug for SseAssembler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SseAssembler")
            .field("tool_count", &self.tools.len())
            .field("finish_reason", &self.finish_reason)
            .field("reasoning_bytes", &self.reasoning.len())
            .field("detail_count", &self.details.as_ref().map(Vec::len))
            .finish()
    }
}

impl SseAssembler {
    pub(crate) fn absorb(&mut self, data: &str) -> Result<Vec<ProviderEvent>, VegaError> {
        let chunk: serde_json::Value = serde_json::from_str(data)
            .map_err(|_| invalid_protocol(ProviderProtocolReason::InvalidJson))?;
        if chunk.is_null() {
            return Ok(Vec::new());
        }
        if !chunk.is_object() {
            return Err(invalid_protocol(ProviderProtocolReason::InvalidFrame));
        }
        // Only a non-null `error` payload is an error frame; several compatible
        // gateways send `"error": null` on ordinary chunks. The frame is
        // classified instead of being reported as malformed data, so an
        // upstream capacity notice keeps its own failure kind.
        if let Some(error) = chunk.get("error").filter(|error| !error.is_null()) {
            if chunk.get("choices").is_none()
                && error.get("type").and_then(serde_json::Value::as_str) == Some("server_error")
                && error.get("code").and_then(serde_json::Value::as_str) == Some("request_timeout")
                && error
                    .get("message")
                    .is_some_and(serde_json::Value::is_string)
            {
                return Err(VegaError::ProviderDiagnostic {
                    kind: ProviderFailureKind::Timeout,
                    status: None,
                    message: "provider stream reported request_timeout".into(),
                    retryable: true,
                    protocol_reason: None,
                    retry_count: None,
                    request_id: None,
                });
            }

            return Err(crate::chat_compat::stream_error_frame(error));
        }
        let mut events = Vec::new();
        let choice = chunk.get("choices").and_then(|choices| choices.get(0));
        let mut counts = None;
        for usage in [
            chunk.get("usage"),
            choice.and_then(|choice| choice.get("usage")),
        ]
        .into_iter()
        .flatten()
        .filter(|value| !value.is_null())
        {
            // Accounting is metadata, not content: an imprecise report must not
            // discard an otherwise valid response. The last well-formed report
            // wins, and an unusable one is simply not recorded.
            if let Some(next) = usage_counts(usage) {
                counts = Some(next);
            }
        }
        if let Some(counts) = counts {
            self.usage = Some(counts);
        }
        if let Some(choice) = choice {
            if let Some(delta) = choice.get("delta").filter(|delta| !delta.is_null()) {
                let delta = delta
                    .as_object()
                    .ok_or_else(|| invalid_protocol(ProviderProtocolReason::InvalidDelta))?;
                if self.finish_reason.is_some() && !delta.is_empty() {
                    return Err(invalid_protocol(ProviderProtocolReason::AfterTerminal));
                }
                let mut aliases = Vec::new();
                let mut empty = None;
                for field in ["reasoning_content", "reasoning", "reasoning_text"] {
                    if let Some(value) = delta.get(field).filter(|value| !value.is_null()) {
                        let value = value.as_str().ok_or_else(|| {
                            invalid_protocol(ProviderProtocolReason::InvalidDelta)
                        })?;
                        if value.is_empty() {
                            if empty.is_none() {
                                empty = Some(field);
                            }
                        } else {
                            aliases.push((field, value));
                        }
                    }
                }
                if let Some((field, value)) = aliases.first().copied() {
                    if aliases.iter().any(|(_, other)| *other != value) {
                        return Err(invalid_protocol(
                            ProviderProtocolReason::ReasoningAliasConflict,
                        ));
                    }
                    if self.reasoning.is_empty() {
                        self.source = Some(field);
                    }
                    let active = aliases.iter().any(|(field, _)| Some(*field) == self.source);
                    if active {
                        self.reasoning.push_str(value);
                        self.last_delta = value.into();
                        events.push(ProviderEvent::ThinkingDelta(value.into()));
                    } else if self.last_delta != value {
                        return Err(invalid_protocol(
                            ProviderProtocolReason::ReasoningAliasConflict,
                        ));
                    }
                } else if self.source.is_none() {
                    self.source = empty;
                }
                if let Some(value) = delta
                    .get("reasoning_details")
                    .filter(|value| !value.is_null())
                {
                    let details = value
                        .as_array()
                        .ok_or_else(|| invalid_protocol(ProviderProtocolReason::InvalidDelta))?;
                    if self.details.is_none() {
                        self.details = Some(Vec::new());
                    }
                    for detail in details {
                        self.absorb_detail(detail)?;
                    }
                }
                if let Some(value) = delta.get("content").filter(|value| !value.is_null()) {
                    let value = value
                        .as_str()
                        .ok_or_else(|| invalid_protocol(ProviderProtocolReason::InvalidDelta))?;
                    if !value.is_empty() {
                        events.push(ProviderEvent::TextDelta(value.into()));
                    }
                }
                if let Some(calls) = delta.get("tool_calls").filter(|value| !value.is_null()) {
                    for call in calls
                        .as_array()
                        .ok_or_else(|| invalid_protocol(ProviderProtocolReason::InvalidDelta))?
                    {
                        self.absorb_tool_call(call)?;
                    }
                }
            }
            if let Some(reason) = choice
                .get("finish_reason")
                .filter(|value| !value.is_null() && value.as_str() != Some(""))
            {
                let mapped = map_finish_reason(
                    reason
                        .as_str()
                        .ok_or_else(|| invalid_protocol(ProviderProtocolReason::InvalidDelta))?,
                );
                // A stop reason this client does not know is not malformed
                // data: the stream is still well-formed. It degrades to the
                // closest known terminal state so the turn still completes and
                // its text is kept, instead of discarding the whole run. A
                // genuinely contradictory repeat is still rejected.
                let reason = if mapped == StopReason::Unknown {
                    if self.tools.is_empty() {
                        StopReason::End
                    } else {
                        StopReason::ToolUse
                    }
                } else {
                    mapped
                };
                if self
                    .finish_reason
                    .is_some_and(|previous| previous != reason)
                {
                    return Err(invalid_protocol(ProviderProtocolReason::TerminalConflict));
                }
                self.finish_reason = Some(reason);
                if reason == StopReason::ToolUse {
                    events.extend(self.flush_tools()?);
                } else if !self.tools.is_empty() {
                    return Err(invalid_protocol(ProviderProtocolReason::TerminalConflict));
                }
            }
        }
        Ok(events)
    }

    fn absorb_detail(&mut self, detail: &serde_json::Value) -> Result<(), VegaError> {
        let object = detail
            .as_object()
            .ok_or_else(|| invalid_protocol(ProviderProtocolReason::ReasoningDetail))?;
        let kind = object
            .get("type")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| invalid_protocol(ProviderProtocolReason::ReasoningDetail))?;
        let payload = match kind {
            "reasoning.text" => "text",
            "reasoning.summary" => "summary",
            "reasoning.encrypted" => "data",
            _ => return Err(invalid_protocol(ProviderProtocolReason::ReasoningDetail)),
        };
        if object
            .get(payload)
            .and_then(serde_json::Value::as_str)
            .is_none()
        {
            return Err(invalid_protocol(ProviderProtocolReason::ReasoningDetail));
        }
        for field in ["id", "format", "signature"] {
            if object
                .get(field)
                .filter(|value| !value.is_null())
                .is_some_and(|value| !value.is_string())
            {
                return Err(invalid_protocol(ProviderProtocolReason::ReasoningDetail));
            }
        }
        if object
            .get("index")
            .filter(|value| !value.is_null())
            .is_some_and(|value| value.as_u64().is_none())
        {
            return Err(invalid_protocol(ProviderProtocolReason::ReasoningDetail));
        }
        let details = self
            .details
            .as_mut()
            .ok_or_else(|| invalid_protocol(ProviderProtocolReason::ReasoningDetail))?;
        for existing in details.iter().filter(|existing| existing["type"] == kind) {
            let same_id = object
                .get("id")
                .and_then(serde_json::Value::as_str)
                .filter(|id| !id.is_empty())
                .is_some_and(|id| existing["id"] == id);
            if same_id
                && existing
                    .get("index")
                    .filter(|value| !value.is_null())
                    .zip(object.get("index").filter(|value| !value.is_null()))
                    .is_some_and(|(one, two)| one != two)
            {
                return Err(invalid_protocol(ProviderProtocolReason::ReasoningDetail));
            }
            if object
                .get("index")
                .and_then(serde_json::Value::as_u64)
                .zip(existing.get("index").and_then(serde_json::Value::as_u64))
                .is_some_and(|(one, two)| one == two)
                && object
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .filter(|id| !id.is_empty())
                    .zip(
                        existing
                            .get("id")
                            .and_then(serde_json::Value::as_str)
                            .filter(|id| !id.is_empty()),
                    )
                    .is_some_and(|(one, two)| one != two)
            {
                return Err(invalid_protocol(ProviderProtocolReason::ReasoningDetail));
            }
        }
        if kind != "reasoning.encrypted"
            && let Some(last) = details.last_mut()
        {
            let mut metadata = object.clone();
            metadata.remove(payload);
            let mut previous = last
                .as_object()
                .cloned()
                .ok_or_else(|| invalid_protocol(ProviderProtocolReason::ReasoningDetail))?;
            previous.remove(payload);
            if metadata == previous {
                let mut merged = last[payload]
                    .as_str()
                    .ok_or_else(|| invalid_protocol(ProviderProtocolReason::ReasoningDetail))?
                    .to_string();
                merged.push_str(
                    detail[payload]
                        .as_str()
                        .ok_or_else(|| invalid_protocol(ProviderProtocolReason::ReasoningDetail))?,
                );
                last[payload] = serde_json::json!(merged);
                return Ok(());
            }
        }
        details.push(detail.clone());
        Ok(())
    }

    fn absorb_tool_call(&mut self, call: &serde_json::Value) -> Result<(), VegaError> {
        if !call.is_object() {
            return Err(invalid_protocol(ProviderProtocolReason::ToolFragment));
        }
        let index = call
            .get("index")
            .filter(|value| !value.is_null())
            .map(|value| {
                value
                    .as_u64()
                    .ok_or_else(|| invalid_protocol(ProviderProtocolReason::ToolFragment))
            })
            .transpose()?;
        let id = call
            .get("id")
            .filter(|value| !value.is_null())
            .map(|value| {
                value
                    .as_str()
                    .ok_or_else(|| invalid_protocol(ProviderProtocolReason::ToolFragment))
            })
            .transpose()?
            .filter(|value| !value.is_empty());
        let by_index =
            index.and_then(|index| self.tools.iter().position(|tool| tool.index == Some(index)));
        let by_id = id.and_then(|id| self.tools.iter().position(|tool| tool.id == id));
        if by_index.zip(by_id).is_some_and(|(one, two)| one != two) {
            return Err(invalid_protocol(
                ProviderProtocolReason::ToolIdentityConflict,
            ));
        }
        let slot = if let Some(slot) = by_index.or(by_id) {
            slot
        } else {
            let candidates = self
                .tools
                .iter()
                .enumerate()
                .filter(|(_, tool)| {
                    index.is_none_or(|index| tool.index.is_none() || tool.index == Some(index))
                        && id.is_none_or(|id| tool.id.is_empty() || tool.id == id)
                        && (index.is_none() || tool.index.is_none())
                        && (id.is_none() || tool.id.is_empty())
                })
                .map(|(slot, _)| slot)
                .collect::<Vec<_>>();
            match candidates.as_slice() {
                [slot] => *slot,
                [] if index.is_some() || id.is_some() => {
                    self.tools.push(ToolFragment::default());
                    self.tools.len() - 1
                }
                _ => return Err(invalid_protocol(ProviderProtocolReason::ToolFragment)),
            }
        };
        let tool = &mut self.tools[slot];
        if index.zip(tool.index).is_some_and(|(one, two)| one != two)
            || (by_index.is_none() && id.is_some_and(|id| !tool.id.is_empty() && tool.id != id))
        {
            return Err(invalid_protocol(ProviderProtocolReason::ToolFragment));
        }
        if let Some(index) = index {
            tool.index = Some(index);
        }
        if tool.id.is_empty()
            && let Some(id) = id
        {
            tool.id = id.into();
        }
        if let Some(function) = call.get("function").filter(|value| !value.is_null()) {
            if !function.is_object() {
                return Err(invalid_protocol(ProviderProtocolReason::ToolFragment));
            }
            if let Some(name) = function.get("name").filter(|value| !value.is_null()) {
                let name = name
                    .as_str()
                    .ok_or_else(|| invalid_protocol(ProviderProtocolReason::ToolFragment))?;
                if !name.is_empty() {
                    if !tool.name.is_empty() && tool.name != name {
                        return Err(invalid_protocol(ProviderProtocolReason::ToolFragment));
                    }
                    tool.name = name.into();
                }
            }
            if let Some(arguments) = function.get("arguments").filter(|value| !value.is_null()) {
                tool.arguments.push_str(
                    arguments
                        .as_str()
                        .ok_or_else(|| invalid_protocol(ProviderProtocolReason::ToolFragment))?,
                );
            }
        }
        Ok(())
    }

    pub(crate) fn finalize(&mut self) -> Result<Vec<ProviderEvent>, VegaError> {
        let stop_reason = self
            .finish_reason
            .ok_or_else(|| invalid_protocol(ProviderProtocolReason::MissingTerminal))?;
        let mut events = self.flush_tools()?;
        let text = std::mem::take(&mut self.reasoning);
        let presence = if self.source == Some("reasoning_content") {
            if text.is_empty() {
                ChatReasoningPresence::Empty
            } else {
                ChatReasoningPresence::NonEmpty(text.clone())
            }
        } else {
            ChatReasoningPresence::Absent
        };
        events.push(ProviderEvent::ChatReasoningContent(presence));
        let source = match self.source {
            Some("reasoning") => Some(crate::ChatReasoningSource::Reasoning),
            Some("reasoning_text") => Some(crate::ChatReasoningSource::ReasoningText),
            _ => None,
        };
        let raw = crate::ChatRawState {
            text: source.map(|source| (source, text)),
            details: self.details.take(),
        };
        if raw.text.is_some() || raw.details.is_some() {
            events.push(ProviderEvent::ChatReasoningState(raw));
        }
        if let Some(counts) = self.usage.take() {
            events.push(counts.event());
        }
        events.push(ProviderEvent::Done { stop_reason });
        Ok(events)
    }

    fn flush_tools(&mut self) -> Result<Vec<ProviderEvent>, VegaError> {
        if self.tools.iter().any(|tool| {
            tool.id.is_empty()
                || tool.name.is_empty()
                || !serde_json::from_str::<serde_json::Value>(&tool.arguments)
                    .is_ok_and(|value| value.is_object())
        }) {
            return Err(invalid_protocol(ProviderProtocolReason::IncompleteTool));
        }
        let mut tools = std::mem::take(&mut self.tools);
        tools.sort_by_key(|tool| tool.index.unwrap_or(u64::MAX));
        Ok(tools
            .into_iter()
            .map(|tool| ProviderEvent::ToolUse {
                id: tool.id,
                name: tool.name,
                input_json: tool.arguments,
            })
            .collect())
    }
}

pub(crate) fn map_finish_reason(reason: &str) -> StopReason {
    match reason {
        "stop" => StopReason::End,
        "tool_calls" | "function_call" => StopReason::ToolUse,
        "length" | "max_tokens" => StopReason::Length,
        _ => StopReason::Unknown,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct UsageCounts {
    input: u64,
    output: u64,
    cache_read: Option<u64>,
    cache_write: Option<u64>,
}
impl UsageCounts {
    fn event(self) -> ProviderEvent {
        ProviderEvent::Usage {
            input: self.input,
            output: self.output,
            cache_read: self.cache_read,
            cache_write: self.cache_write,
        }
    }
}

fn usage_counts(usage: &serde_json::Value) -> Option<UsageCounts> {
    let number = |value: Option<&serde_json::Value>| {
        value
            .filter(|value| !value.is_null())
            .and_then(serde_json::Value::as_u64)
    };
    // Token totals are the only required figures; a report without them is
    // dropped rather than failing the response that carried it.
    let input = number(usage.get("prompt_tokens"))?;
    let output = number(usage.get("completion_tokens"))?;
    let cache_read = number(usage.pointer("/prompt_tokens_details/cached_tokens"))
        .or(number(usage.get("prompt_cache_hit_tokens")))
        .or(number(usage.get("cached_tokens")));
    let cache_write = number(usage.pointer("/prompt_tokens_details/cache_write_tokens"))
        .or(number(usage.get("cache_write_tokens")));
    // Cache figures that exceed the prompt are a misreported breakdown, not
    // unusable content: the token totals are still valid, so only the
    // inconsistent optional breakdown is dropped.
    let consistent = |count: Option<u64>| count.filter(|count| *count <= input);
    let cache_read = consistent(cache_read);
    let cache_write = consistent(cache_write);
    let cache_read = cache_read.filter(|read| {
        cache_write
            .and_then(|write| read.checked_add(write))
            .is_none_or(|total| total <= input)
    });
    let cache_write = cache_write.filter(|write| {
        cache_read
            .and_then(|read| read.checked_add(*write))
            .is_none_or(|total| total <= input)
    });
    Some(UsageCounts {
        input,
        output,
        cache_read,
        cache_write,
    })
}

fn invalid_protocol(reason: ProviderProtocolReason) -> VegaError {
    crate::chat_compat::protocol_error_with_reason(reason)
}

#[cfg(test)]
mod protocol_diagnostics_tests {
    use super::*;

    fn reason(error: VegaError, expected: &str) {
        assert_eq!(error.diagnostic_metrics().protocol_reason, Some(expected));
        assert!(!format!("{error:?} {error}").contains("private-sentinel"));
    }

    #[test]
    fn malformed_chat_frames_have_closed_reasons() {
        for (frame, expected) in [
            ("private-sentinel", "invalid_json"),
            ("[]", "invalid_frame"),
            (r#"{"choices":[{"delta":42}]}"#, "invalid_delta"),
            (
                r#"{"choices":[{"delta":{"reasoning":"private-sentinel","reasoning_content":"different"}}]}"#,
                "reasoning_alias_conflict",
            ),
            (
                r#"{"choices":[{"delta":{"reasoning_details":[{"type":"unknown"}]}}]}"#,
                "reasoning_detail",
            ),
            (
                r#"{"choices":[{"delta":{"tool_calls":[42]}}]}"#,
                "tool_fragment",
            ),
            (
                r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call","function":{"name":"read","arguments":"private-sentinel"}}]},"finish_reason":"tool_calls"}]}"#,
                "incomplete_tool",
            ),
        ] {
            reason(
                SseAssembler::default()
                    .absorb(frame)
                    .err()
                    .expect("must reject"),
                expected,
            );
        }
        reason(
            SseAssembler::default()
                .finalize()
                .err()
                .expect("missing terminal"),
            "missing_terminal",
        );
    }

    #[test]
    fn terminal_and_tool_identity_guards_keep_rejecting() {
        let mut parser = SseAssembler::default();
        parser
            .absorb(r#"{"choices":[{"finish_reason":"stop"}]}"#)
            .expect("terminal");
        reason(
            parser
                .absorb(r#"{"choices":[{"delta":{"content":"private-sentinel"}}]}"#)
                .err()
                .expect("late delta"),
            "after_terminal",
        );
        reason(
            parser
                .absorb(r#"{"choices":[{"finish_reason":"length"}]}"#)
                .err()
                .expect("conflict"),
            "terminal_conflict",
        );
        let mut parser = SseAssembler::default();
        parser.absorb(r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"a"},{"index":1,"id":"b"}]}}]}"#).expect("fragments");
        reason(
            parser
                .absorb(r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"b"}]}}]}"#)
                .err()
                .expect("identity"),
            "tool_identity_conflict",
        );
    }

    #[test]
    fn compatible_text_tools_and_upstream_errors_keep_their_behavior() {
        let mut parser = SseAssembler::default();
        let events = parser.absorb(r#"{"error":null,"choices":[{"delta":{"content":"ok"},"finish_reason":"future_reason"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"prompt_cache_miss_tokens":20}}"#).expect("compatible text");
        assert!(
            events
                .iter()
                .any(|event| matches!(event, ProviderEvent::TextDelta(text) if text == "ok"))
        );
        assert!(
            parser
                .finalize()
                .expect("done")
                .iter()
                .any(|event| matches!(
                    event,
                    ProviderEvent::Done {
                        stop_reason: StopReason::End
                    }
                ))
        );
        let mut parser = SseAssembler::default();
        let events = parser.absorb(r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call","function":{"name":"read","arguments":"{}"}}]},"finish_reason":"tool_calls"}]}"#).expect("valid tool");
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, ProviderEvent::ToolUse { .. }))
                .count(),
            1
        );
        assert!(
            parser
                .finalize()
                .expect("done")
                .iter()
                .any(|event| matches!(
                    event,
                    ProviderEvent::Done {
                        stop_reason: StopReason::ToolUse
                    }
                ))
        );
        for (frame, expected) in [
            (
                r#"{"error":{"code":"server_is_overloaded"}}"#,
                ProviderFailureKind::Overloaded,
            ),
            (
                r#"{"error":{"code":"denied","message":"private-sentinel"}}"#,
                ProviderFailureKind::Rejected,
            ),
            (
                r#"{"error":{"type":"server_error","code":"request_timeout","message":"private-sentinel"}}"#,
                ProviderFailureKind::Timeout,
            ),
        ] {
            let error = SseAssembler::default()
                .absorb(frame)
                .err()
                .expect("upstream error");
            assert!(
                matches!(error, VegaError::ProviderDiagnostic { kind, protocol_reason: None, .. } if kind == expected)
            );
            assert!(!format!("{error:?} {error}").contains("private-sentinel"));
        }
    }
}
