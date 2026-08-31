use super::*;

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
    let inner: RawSse = Box::pin(resp.bytes_stream().eventsource());
    let state = SsePipeline {
        inner,
        assembler: SseAssembler::default(),
        pending: VecDeque::new(),
        done: false,
        cancel,
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
                    st.pending.push_back(Err(map_sse_error(err)));
                }
                None => {
                    st.done = true;
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

pub(crate) fn map_sse_error(err: EventStreamError<reqwest::Error>) -> VegaError {
    VegaError::Provider {
        status: None,
        message: format!("SSE stream error: {err}"),
        retryable: false,
    }
}

/// One `delta.tool_calls` fragment slot, keyed by the wire `index` field.
#[derive(Default)]
pub(crate) struct ToolFragment {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) arguments: String,
}

impl fmt::Debug for ToolFragment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ToolFragment")
            .field("id_bytes", &self.id.len())
            .field("name_bytes", &self.name.len())
            .field("argument_bytes", &self.arguments.len())
            .finish()
    }
}

/// Incremental assembler from OpenAI chat-completion chunks to
/// [`ProviderEvent`]s (tech-spec §4.1):
///
/// - `choices[0].delta.content` → [`ProviderEvent::TextDelta`]
/// - `choices[0].delta.reasoning_content` → [`ProviderEvent::ThinkingDelta`]
/// - `choices[0].delta.tool_calls` → aggregated by index; [`ProviderEvent::
///   ToolUse`] only once fragments complete (flushed on `finish_reason` or
///   stream finalization)
/// - `usage` → [`ProviderEvent::Usage`]
/// - `choices[0].finish_reason` → the terminal [`StopReason`]
///
/// Unknown JSON fields are ignored.
#[derive(Default)]
pub(crate) struct SseAssembler {
    pub(crate) tools: BTreeMap<u64, ToolFragment>,
    pub(crate) finish_reason: Option<StopReason>,
}

impl fmt::Debug for SseAssembler {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SseAssembler")
            .field("tool_count", &self.tools.len())
            .field("finish_reason", &self.finish_reason)
            .finish()
    }
}

impl SseAssembler {
    /// Absorbs one SSE `data:` payload (one JSON chunk; the `[DONE]`
    /// sentinel is handled by the caller). Returns that chunk's events.
    pub(crate) fn absorb(&mut self, data: &str) -> Result<Vec<ProviderEvent>, VegaError> {
        let chunk: serde_json::Value =
            serde_json::from_str(data).map_err(|e| VegaError::Provider {
                status: None,
                message: format!("invalid SSE chunk JSON: {e}"),
                retryable: false,
            })?;
        let mut events = Vec::new();
        // usage：stream_options.include_usage 的最终 chunk（choices 为空数组）
        if let Some(usage) = chunk.get("usage").filter(|u| !u.is_null()) {
            events.push(usage_event(usage));
        }
        if let Some(choice) = chunk.get("choices").and_then(|c| c.get(0)) {
            if let Some(delta) = choice.get("delta") {
                if let Some(text) = str_field(delta, "content")
                    && !text.is_empty()
                {
                    events.push(ProviderEvent::TextDelta(text.to_string()));
                }
                if let Some(text) = str_field(delta, "reasoning_content")
                    && !text.is_empty()
                {
                    events.push(ProviderEvent::ThinkingDelta(text.to_string()));
                }
                if let Some(calls) = delta.get("tool_calls").and_then(|t| t.as_array()) {
                    for call in calls {
                        self.absorb_tool_call(call);
                    }
                }
            }
            if let Some(reason) = str_field(choice, "finish_reason") {
                self.finish_reason = Some(map_finish_reason(reason));
                // tool_calls 收敛：分片聚合完整后才发 ToolUse
                if self.finish_reason == Some(StopReason::ToolUse) {
                    events.extend(self.flush_tools());
                }
            }
        }
        Ok(events)
    }

    pub(crate) fn absorb_tool_call(&mut self, call: &serde_json::Value) {
        let index = call.get("index").and_then(|i| i.as_u64()).unwrap_or(0);
        let fragment = self.tools.entry(index).or_default();
        if let Some(id) = str_field(call, "id") {
            fragment.id = id.to_string();
        }
        let function = call.get("function");
        if let Some(name) = function
            .and_then(|f| f.get("name"))
            .and_then(|n| n.as_str())
        {
            fragment.name = name.to_string();
        }
        if let Some(args) = function
            .and_then(|f| f.get("arguments"))
            .and_then(|a| a.as_str())
        {
            // 分片拼装：只拼字符串，不做半截 JSON 解析
            fragment.arguments.push_str(args);
        }
    }

    /// Emits terminal events only after an explicit wire finish reason.
    pub(crate) fn finalize(&mut self) -> Result<Vec<ProviderEvent>, VegaError> {
        let finish_reason = self.finish_reason.ok_or_else(|| VegaError::Provider {
            status: None,
            message: String::from("SSE stream ended without finish_reason"),
            retryable: false,
        })?;
        let mut events = self.flush_tools();
        events.push(ProviderEvent::Done {
            stop_reason: finish_reason,
        });
        Ok(events)
    }

    pub(crate) fn flush_tools(&mut self) -> Vec<ProviderEvent> {
        let tools = std::mem::take(&mut self.tools);
        tools
            .into_values()
            .map(|fragment| ProviderEvent::ToolUse {
                id: fragment.id,
                name: fragment.name,
                input_json: fragment.arguments,
            })
            .collect()
    }
}

pub(crate) fn str_field<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(|v| v.as_str())
}

/// Maps the wire `finish_reason` onto the minimal [`StopReason`] set.
pub(crate) fn map_finish_reason(reason: &str) -> StopReason {
    match reason {
        "tool_calls" | "function_call" => StopReason::ToolUse,
        "length" | "max_tokens" => StopReason::Length,
        _ => StopReason::End,
    }
}

/// Maps the final usage chunk onto [`ProviderEvent::Usage`].
///
/// `prompt_tokens_details.cached_tokens` is the OpenAI-compatible cache-hit
/// counter; cache-write reporting has no cross-vendor wire field, so it
/// stays 0 until a provider documents one.
pub(crate) fn usage_event(usage: &serde_json::Value) -> ProviderEvent {
    let get = |key: &str| usage.get(key).and_then(|v| v.as_u64()).unwrap_or(0);
    let cache_read = usage
        .get("prompt_tokens_details")
        .and_then(|d| d.get("cached_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    ProviderEvent::Usage {
        input: get("prompt_tokens"),
        output: get("completion_tokens"),
        cache_read,
        cache_write: 0,
    }
}
