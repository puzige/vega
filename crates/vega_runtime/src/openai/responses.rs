use super::*;
use crate::{ChatRole, ProviderEvent, ReasoningReplayPolicy};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

const TOOL_ARGUMENT_LIMIT: usize = 256 * 1024;
const WIRE_LIMIT: usize = 4 * 1024 * 1024;

#[derive(Debug)]
enum ResponseBytesError {
    Limit { observed_bytes: usize },
    Transport,
}

#[derive(Default)]
struct ResponseFrameBudget {
    bytes: usize,
    line_has_content: bool,
    after_cr: bool,
}

impl ResponseFrameBudget {
    fn consume(&mut self, chunk: &[u8]) -> Result<(), ResponseBytesError> {
        for byte in chunk {
            if self.after_cr && *byte == b'\n' {
                self.after_cr = false;
                if self.bytes > 0 {
                    self.bytes = self.bytes.saturating_add(1);
                    self.check()?;
                }
                continue;
            }
            self.bytes = self.bytes.saturating_add(1);
            self.check()?;
            self.after_cr = *byte == b'\r';
            if matches!(*byte, b'\n' | b'\r') {
                if !self.line_has_content {
                    self.bytes = 0;
                }
                self.line_has_content = false;
            } else {
                self.line_has_content = true;
            }
        }
        Ok(())
    }

    fn check(&self) -> Result<(), ResponseBytesError> {
        if self.bytes > WIRE_LIMIT {
            Err(ResponseBytesError::Limit {
                observed_bytes: self.bytes,
            })
        } else {
            Ok(())
        }
    }
}

impl fmt::Display for ResponseBytesError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Limit { .. } => "Responses byte limit",
            Self::Transport => "Responses transport failure",
        })
    }
}

fn invalid() -> VegaError {
    response_error(crate::ProviderFailureKind::Protocol)
}

fn response_error(kind: crate::ProviderFailureKind) -> VegaError {
    VegaError::ProviderDiagnostic {
        kind,
        status: None,
        message: String::new(),
        retryable: false,
        retry_count: None,
        request_id: None,
    }
}

pub(crate) fn build_body(req: &ChatRequest) -> Result<Value, VegaError> {
    let mut reasoning = json!({"summary": "auto"});
    if let Some(profile) = &req.reasoning {
        if profile.replay_policy != ReasoningReplayPolicy::ResponsesItems
            || profile.protocol == ReasoningProtocol::ZhipuChatCompletions
            || profile.protocol == ReasoningProtocol::DeepSeekChatCompletions
            || (profile.choice == ReasoningChoice::Disabled
                && profile.disabled_wire != Some(ReasoningDisabledWire::ReasoningEffortNone))
        {
            return Err(VegaError::ReasoningSelectionInvalid { message: "Responses does not support this reasoning replay/off profile; choose Chat Completions or a compatible profile".into() });
        }
        match &profile.choice {
            ReasoningChoice::Effort(effort) => reasoning["effort"] = json!(effort),
            ReasoningChoice::Disabled => reasoning["effort"] = json!("none"),
            ReasoningChoice::ProviderDefault => {}
        }
    }
    let mut input = Vec::new();
    for message in &req.messages {
        if let Some(layout) = response_layout_items(message)? {
            input.extend(layout);
            continue;
        }
        input.extend(message.response_reasoning.iter().cloned());
        if message.role == ChatRole::Tool {
            let output = if message.images.is_empty() {
                json!(message.content)
            } else {
                use base64::Engine;
                let mut parts = vec![json!({"type":"input_text","text":message.content})];
                parts.extend(message.images.iter().map(|image| json!({"type":"input_image","image_url":format!("data:{};base64,{}", image.mime_type(), base64::engine::general_purpose::STANDARD.encode(image.bytes())),"detail":"auto"})));
                Value::Array(parts)
            };
            input.push(json!({"type":"function_call_output", "call_id":message.tool_call_id, "output":output}));
            continue;
        }
        if !message.content.is_empty() || !message.images.is_empty() {
            let mut content = Vec::new();
            if !message.content.is_empty() {
                content.push(json!({"type": if message.role == ChatRole::Assistant { "output_text" } else { "input_text" }, "text": message.content}));
            }
            for image in &message.images {
                use base64::Engine;
                content.push(json!({"type":"input_image", "image_url":format!("data:{};base64,{}", image.mime_type(), base64::engine::general_purpose::STANDARD.encode(image.bytes()))}));
            }
            input.push(json!({"role":message.role.as_str(),"content":content}));
        }
        for call in &message.tool_calls {
            input.push(json!({"type":"function_call", "call_id":call.id, "name":call.name, "arguments":call.input_json}));
        }
    }
    let mut body = json!({"model":req.model,"input":input,"stream":true,"store":false,"reasoning":reasoning,"include":["reasoning.encrypted_content"]});
    if !req.tools.is_empty() {
        body["tools"] = Value::Array(req.tools.iter().map(|tool| {
            json!({"type":"function", "name":tool.name,"description":tool.description,"parameters":tool.input_schema,"strict":tool.strict})
        }).collect());
    }
    if let Some(limit) = req.max_tokens {
        body["max_output_tokens"] = json!(limit);
    }
    Ok(body)
}

pub(crate) fn response_layout_items(
    message: &crate::ChatMessage,
) -> Result<Option<Vec<Value>>, VegaError> {
    let Some(layout) = &message.response_replay_layout else {
        return Ok(None);
    };
    let invalid = || VegaError::Context(crate::ContextRuntimeError::InvalidProjection);
    if message.role != ChatRole::Assistant {
        return Err(invalid());
    }
    let mut items = Vec::with_capacity(layout.slots.len());
    let mut replay_reasoning = Vec::new();
    let mut call_slots = BTreeSet::new();
    let mut message_text = String::new();
    for slot in &layout.slots {
        match slot {
            crate::ResponseReplaySlot::Reasoning(item) => {
                if item["type"] != "reasoning" || !item.is_object() {
                    return Err(invalid());
                }
                replay_reasoning.push(item.clone());
                items.push(item.clone());
            }
            crate::ResponseReplaySlot::Message(item) => {
                if item["type"] != "message"
                    || item["role"].as_str() != Some("assistant")
                    || !item.is_object()
                {
                    return Err(invalid());
                }
                let content = item["content"].as_array().ok_or_else(invalid)?;
                for part in content {
                    match part["type"].as_str() {
                        Some("output_text") => {
                            message_text.push_str(part["text"].as_str().ok_or_else(invalid)?);
                        }
                        Some("refusal") => {
                            message_text.push_str(part["refusal"].as_str().ok_or_else(invalid)?);
                        }
                        _ => return Err(invalid()),
                    }
                }
                items.push(item.clone());
            }
            crate::ResponseReplaySlot::FunctionCall {
                item,
                call_id,
                name,
                arguments,
            } => {
                if item["type"] != "function_call"
                    || item["call_id"].as_str() != Some(call_id)
                    || item["name"].as_str() != Some(name)
                    || item["arguments"].as_str() != Some(arguments)
                    || !call_slots.insert(call_id.clone())
                {
                    return Err(invalid());
                }
                let call = message
                    .tool_calls
                    .iter()
                    .find(|call| call.id == *call_id && call.name == *name)
                    .ok_or_else(invalid)?;
                if call.input_json != *arguments
                    || !serde_json::from_str::<Value>(arguments)
                        .is_ok_and(|value| value.is_object())
                {
                    return Err(invalid());
                }
                items.push(item.clone());
            }
        }
    }
    if message_text != message.content
        || call_slots.len() != message.tool_calls.len()
        || replay_reasoning != message.response_reasoning
    {
        return Err(invalid());
    }
    Ok(Some(items))
}

#[derive(Default)]
struct TextProgress {
    bytes: usize,
    digest: Sha256,
}

impl TextProgress {
    fn absorb<'a>(&mut self, text: &'a str, done: bool) -> Result<&'a str, VegaError> {
        let suffix = if done {
            let prefix = text.get(..self.bytes).ok_or_else(invalid)?;
            if Sha256::digest(prefix.as_bytes()) != self.digest.clone().finalize() {
                return Err(invalid());
            }
            &text[self.bytes..]
        } else {
            text
        };
        self.bytes = self.bytes.saturating_add(suffix.len());
        self.digest.update(suffix.as_bytes());
        Ok(suffix)
    }
}

#[derive(Default)]
pub(crate) struct Assembler {
    items: BTreeMap<u64, Value>,
    complete: BTreeSet<u64>,
    deltas: BTreeMap<(String, u64, String), TextProgress>,
    last_summary: Option<(String, u64)>,
    pub(crate) terminal: bool,
}
impl Assembler {
    fn text_event(&mut self, event: &Value, kind: &str) -> Result<Vec<ProviderEvent>, VegaError> {
        let mut out = Vec::new();
        let item = event["item_id"].as_str().ok_or_else(invalid)?.to_owned();
        let index = event["summary_index"]
            .as_u64()
            .or_else(|| event["content_index"].as_u64())
            .unwrap_or(0);
        let family = kind
            .trim_end_matches(".delta")
            .trim_end_matches(".done")
            .to_owned();
        let key = (item.clone(), index, family);
        let done = kind.ends_with(".done");
        let text = event[if done && kind == "response.refusal.done" {
            "refusal"
        } else if done {
            "text"
        } else {
            "delta"
        }]
        .as_str()
        .ok_or_else(invalid)?;
        let text = self.deltas.entry(key).or_default().absorb(text, done)?;
        if text.is_empty() {
            return Ok(out);
        }
        if kind.starts_with("response.reasoning_summary_text.") {
            let key = (item, index);
            if self.last_summary.as_ref().is_some_and(|last| last != &key) {
                out.push(ProviderEvent::SummaryDelta("\n\n".into()));
            }
            self.last_summary = Some(key);
            push_text(&mut out, text, ProviderEvent::SummaryDelta);
        } else if kind.starts_with("response.reasoning_text.") {
            push_text(&mut out, text, ProviderEvent::ThinkingDelta);
        } else {
            push_text(&mut out, text, ProviderEvent::TextDelta);
        }
        Ok(out)
    }
    fn snapshot(&mut self, item: &Value) -> Result<Vec<ProviderEvent>, VegaError> {
        let mut out = Vec::new();
        for (field, family) in [
            ("summary", "response.reasoning_summary_text.done"),
            ("content", "response.output_text.done"),
        ] {
            if let Some(parts) = item[field].as_array() {
                for (index, part) in parts.iter().enumerate() {
                    let (kind, text_field) = match part["type"].as_str() {
                        Some("summary_text" | "output_text") => (family, "text"),
                        Some("refusal") => ("response.refusal.done", "refusal"),
                        Some("reasoning_text") => ("response.reasoning_text.done", "text"),
                        _ => continue,
                    };
                    let mut event =
                        json!({"item_id":item["id"],"summary_index":index,"content_index":index});
                    event[text_field] = part[text_field].clone();
                    out.extend(self.text_event(&event, kind)?);
                }
            }
        }
        Ok(out)
    }
    pub(crate) fn absorb(&mut self, data: &str) -> Result<Vec<ProviderEvent>, VegaError> {
        if data.len() > WIRE_LIMIT {
            return Err(VegaError::ProviderPayloadTooLarge {
                observed_bytes: data.len(),
                limit_bytes: WIRE_LIMIT,
            });
        }
        if self.terminal {
            return Err(invalid());
        }
        let event: Value = serde_json::from_str(data).map_err(|_| invalid())?;
        let kind = event["type"].as_str().ok_or_else(invalid)?;
        let mut out = Vec::new();
        match kind {
            "response.incomplete" => {
                let kind = if event
                    .pointer("/response/incomplete_details/reason")
                    .and_then(Value::as_str)
                    == Some("max_output_tokens")
                {
                    crate::ProviderFailureKind::OutputLimit
                } else {
                    crate::ProviderFailureKind::Rejected
                };
                return Err(response_error(kind));
            }
            "response.failed" | "error" => {
                return Err(response_error(crate::ProviderFailureKind::Rejected));
            }
            "response.reasoning_summary_text.delta"
            | "response.reasoning_text.delta"
            | "response.output_text.delta"
            | "response.reasoning_summary_text.done"
            | "response.reasoning_text.done"
            | "response.output_text.done"
            | "response.refusal.delta"
            | "response.refusal.done" => {
                out.extend(self.text_event(&event, kind)?);
            }
            "response.output_item.added" | "response.output_item.done" => {
                let index = event["output_index"].as_u64().ok_or_else(invalid)?;
                self.items.insert(index, event["item"].clone());
                if kind.ends_with(".done") {
                    self.complete.insert(index);
                    out.extend(self.snapshot(&event["item"])?);
                }
            }
            "response.function_call_arguments.delta" => {
                let index = event["output_index"].as_u64().ok_or_else(invalid)?;
                let item = self.items.get_mut(&index).ok_or_else(invalid)?;
                let delta = event["delta"].as_str().ok_or_else(invalid)?;
                let mut arguments = item["arguments"].as_str().unwrap_or_default().to_owned();
                if arguments.len().saturating_add(delta.len()) > TOOL_ARGUMENT_LIMIT {
                    return Err(invalid());
                }
                arguments.push_str(delta);
                item["arguments"] = json!(arguments);
            }
            "response.completed" => {
                if event["response"]["status"]
                    .as_str()
                    .is_some_and(|status| status != "completed")
                {
                    return Err(invalid());
                }
                if let Some(items) = event["response"]["output"].as_array() {
                    for item in self
                        .items
                        .values()
                        .filter(|item| item["type"] == "function_call")
                    {
                        if !items.iter().any(|candidate| {
                            candidate["type"] == "function_call"
                                && candidate["call_id"] == item["call_id"]
                        }) {
                            return Err(invalid());
                        }
                    }
                    self.items = items
                        .iter()
                        .cloned()
                        .enumerate()
                        .map(|(i, item)| (i as u64, item))
                        .collect();
                    self.complete = self.items.keys().copied().collect();
                }
                let snapshots: Vec<_> = self.items.values().cloned().collect();
                for item in &snapshots {
                    out.extend(self.snapshot(item)?);
                }
                let mut replay = Vec::new();
                let mut tools = Vec::new();
                let mut slots = Vec::with_capacity(self.items.len());
                for (index, item) in &self.items {
                    if !self.complete.contains(index) {
                        return Err(invalid());
                    }
                    match item["type"].as_str() {
                        Some("reasoning") => {
                            replay.push(item.clone());
                            slots.push(crate::ResponseReplaySlot::Reasoning(item.clone()));
                        }
                        Some("function_call") => {
                            let arguments = item["arguments"].as_str().ok_or_else(invalid)?;
                            if arguments.len() > TOOL_ARGUMENT_LIMIT
                                || !serde_json::from_str::<Value>(arguments)
                                    .is_ok_and(|v| v.is_object())
                            {
                                return Err(invalid());
                            }
                            let id = item["call_id"]
                                .as_str()
                                .filter(|id| !id.is_empty())
                                .ok_or_else(invalid)?;
                            let name = item["name"]
                                .as_str()
                                .filter(|name| !name.is_empty())
                                .ok_or_else(invalid)?;
                            slots.push(crate::ResponseReplaySlot::FunctionCall {
                                item: item.clone(),
                                call_id: id.into(),
                                name: name.into(),
                                arguments: arguments.into(),
                            });
                            tools.push(ProviderEvent::ToolUse {
                                id: id.into(),
                                name: name.into(),
                                input_json: arguments.into(),
                            });
                        }
                        Some("message") => {
                            if item["role"].as_str() != Some("assistant") {
                                return Err(invalid());
                            }
                            slots.push(crate::ResponseReplaySlot::Message(item.clone()))
                        }
                        _ => return Err(invalid()),
                    }
                }
                if !replay.is_empty() {
                    out.push(ProviderEvent::ReasoningReplay(replay));
                }
                out.push(ProviderEvent::ResponseReplay(crate::ResponseReplayLayout {
                    response_id: event["response"]["id"].as_str().map(str::to_owned),
                    slots,
                }));
                let stop_reason = if tools.is_empty() {
                    StopReason::End
                } else {
                    StopReason::ToolUse
                };
                out.extend(tools);
                if let Some(usage) = event["response"].get("usage").filter(|v| !v.is_null()) {
                    let input = usage["input_tokens"].as_u64().ok_or_else(invalid)?;
                    let output = usage["output_tokens"].as_u64().ok_or_else(invalid)?;
                    let cache_read = usage["input_tokens_details"]["cached_tokens"].as_u64();
                    if cache_read.is_some_and(|tokens| tokens > input) {
                        return Err(invalid());
                    }
                    out.push(ProviderEvent::Usage {
                        input,
                        output,
                        cache_read,
                        cache_write: None,
                    });
                }
                out.push(ProviderEvent::Done { stop_reason });
                self.terminal = true;
            }
            _ => {}
        }
        Ok(out)
    }
}

fn push_text(events: &mut Vec<ProviderEvent>, mut text: &str, make: fn(String) -> ProviderEvent) {
    while !text.is_empty() {
        let mut end = text.len().min(64 * 1024);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        events.push(make(text[..end].to_owned()));
        text = &text[end..];
    }
}

pub(crate) fn event_stream(resp: reqwest::Response, cancel: CancellationToken) -> EventStream {
    event_stream_with_recovery(resp, cancel, false)
}

pub(super) fn event_stream_with_recovery(
    resp: reqwest::Response,
    cancel: CancellationToken,
    recoverable: bool,
) -> EventStream {
    let bytes = resp
        .bytes_stream()
        .scan(ResponseFrameBudget::default(), |budget, chunk| {
            let next = match chunk {
                Ok(chunk) => budget.consume(&chunk).map(|()| chunk),
                Err(_) => Err(ResponseBytesError::Transport),
            };
            futures::future::ready(Some(next))
        });
    let inner = Box::pin(bytes.eventsource());
    Box::pin(futures::stream::unfold(
        (inner, Assembler::default(), VecDeque::new(), false, cancel),
        move |(mut inner, mut parser, mut pending, mut ended, cancel)| async move {
            loop {
                if cancel.is_cancelled() {
                    return None;
                }
                if let Some(event) = pending.pop_front() {
                    return Some((event, (inner, parser, pending, ended, cancel)));
                }
                if ended || parser.terminal {
                    return None;
                }
                let next = tokio::select! { biased; _ = cancel.cancelled() => return None, next = inner.next() => next };
                match next {
                    Some(Ok(event)) => match parser.absorb(&event.data) {
                        Ok(events) => pending.extend(events.into_iter().map(Ok)),
                        Err(error) => {
                            ended = true;
                            pending.push_back(Err(error));
                        }
                    },
                    Some(Err(EventStreamError::Transport(ResponseBytesError::Transport)))
                    | None => {
                        ended = true;
                        pending.push_back(Err(if recoverable {
                            recovery::incomplete_stream_error()
                        } else {
                            response_error(crate::ProviderFailureKind::Transport)
                        }));
                    }
                    Some(Err(EventStreamError::Transport(ResponseBytesError::Limit {
                        observed_bytes,
                    }))) => {
                        ended = true;
                        pending.push_back(Err(VegaError::ProviderPayloadTooLarge {
                            observed_bytes,
                            limit_bytes: WIRE_LIMIT,
                        }));
                    }
                    Some(Err(_)) => {
                        ended = true;
                        pending.push_back(Err(invalid()));
                    }
                }
            }
        },
    ))
}

#[cfg(test)]
mod frame_budget_tests {
    use super::*;

    #[test]
    fn valid_frame_boundaries_release_budget_across_split_line_endings() {
        for ending in ["\n", "\r", "\r\n"] {
            let mut budget = ResponseFrameBudget::default();
            let frame = format!("data: {}{ending}{ending}", "x".repeat(WIRE_LIMIT / 2));
            for _ in 0..4 {
                for chunk in frame.as_bytes().chunks(1024) {
                    budget.consume(chunk).unwrap();
                }
            }
            assert_eq!(budget.bytes, 0);
        }
        let mut budget = ResponseFrameBudget::default();
        for chunk in [b"data: x\r".as_slice(), b"\n\r", b"\n", b"data: y\r", b"\r"] {
            budget.consume(chunk).unwrap();
        }
        assert_eq!(budget.bytes, 0);
    }

    #[test]
    fn multiline_sse_frame_is_bounded_before_json_assembly() {
        let mut budget = ResponseFrameBudget::default();
        let line = format!("data: {}\n", "x".repeat(WIRE_LIMIT / 2));
        budget.consume(line.as_bytes()).unwrap();
        assert!(
            matches!(budget.consume(line.as_bytes()),Err(ResponseBytesError::Limit {observed_bytes}) if observed_bytes == WIRE_LIMIT+1)
        );
    }
}

#[cfg(test)]
mod failure_tests {
    use super::*;

    #[test]
    fn incomplete_output_and_malformed_payload_have_distinct_safe_diagnostics() {
        let mut parser = Assembler::default();
        let error = parser.absorb(r#"{"type":"response.incomplete","response":{"incomplete_details":{"reason":"max_output_tokens"}}}"#).unwrap_err();
        assert!(matches!(
            error,
            VegaError::ProviderDiagnostic {
                kind: crate::ProviderFailureKind::OutputLimit,
                ..
            }
        ));
        let detail = error.diagnostic_metrics();
        assert_eq!(detail.failure_source, Some("provider"));
        assert_eq!(detail.failure_kind, Some("output_limit"));
        assert_eq!(detail.failure_scope, Some("response"));
        assert_eq!(detail.failure_observed, None);
        assert_eq!(detail.failure_limit, None);
        let error = Assembler::default()
            .absorb("PRIVATE_INVALID_BODY")
            .unwrap_err();
        assert!(matches!(
            error,
            VegaError::ProviderDiagnostic {
                kind: crate::ProviderFailureKind::Protocol,
                ..
            }
        ));
        assert!(!format!("{error:?}").contains("PRIVATE_INVALID_BODY"));
    }
}
