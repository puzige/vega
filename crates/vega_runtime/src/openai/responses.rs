//! Explicit Responses transport. All retained state belongs to one request/run.
use super::*;
use crate::{ChatRole, ProviderEvent};
use serde_json::{Value, json};
use std::collections::BTreeSet;

const ITEM_LIMIT: usize = 256;
const REPLAY_LIMIT: usize = 256 * 1024;
const WIRE_LIMIT: usize = 4 * 1024 * 1024;

fn invalid() -> VegaError {
    VegaError::Provider {
        status: None,
        message: "invalid, incomplete, or oversized Responses stream".into(),
        retryable: false,
    }
}

pub(crate) fn build_body(req: &ChatRequest) -> Result<Value, VegaError> {
    let mut reasoning = json!({"summary": "auto"});
    if let Some(profile) = &req.reasoning {
        // Responses cannot satisfy Chat-only original-text replay or Zhipu's off switch.
        if profile.preserve_reasoning_content
            || profile.protocol == ReasoningProtocol::ZhipuChatCompletions
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
        input.extend(message.response_reasoning.iter().cloned());
        if message.role == ChatRole::Tool {
            input.push(json!({"type":"function_call_output", "call_id":message.tool_call_id, "output":message.content}));
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
            // Responses defaults to strict when omitted, unlike Chat Completions.
            json!({"type":"function", "name":tool.name,"description":tool.description,"parameters":tool.input_schema,"strict":tool.strict})
        }).collect());
    }
    if let Some(limit) = req.max_tokens {
        body["max_output_tokens"] = json!(limit);
    }
    Ok(body)
}

#[derive(Default)]
pub(crate) struct Assembler {
    items: BTreeMap<u64, Value>,
    complete: BTreeSet<u64>,
    deltas: BTreeMap<(String, u64, String), String>,
    text_bytes: usize,
    last_summary: Option<(String, u64)>,
    pub(crate) terminal: bool,
    bytes: usize,
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
        if text.len() > if done { REPLAY_LIMIT } else { 64 * 1024 }
            || self.deltas.len() >= ITEM_LIMIT && !self.deltas.contains_key(&key)
        {
            return Err(invalid());
        }
        let prior = self.deltas.entry(key).or_default();
        let text = if done {
            text.strip_prefix(prior.as_str()).ok_or_else(invalid)?
        } else {
            text
        };
        if text.is_empty() {
            return Ok(out);
        }
        self.text_bytes = self.text_bytes.saturating_add(text.len());
        if self.text_bytes > 1024 * 1024 {
            return Err(invalid());
        }
        prior.push_str(text);
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
        self.bytes = self.bytes.saturating_add(data.len());
        if self.bytes > WIRE_LIMIT || self.terminal {
            return Err(invalid());
        }
        let event: Value = serde_json::from_str(data).map_err(|_| invalid())?;
        let kind = event["type"].as_str().ok_or_else(invalid)?;
        let mut out = Vec::new();
        match kind {
            "response.failed" | "response.incomplete" | "error" => return Err(invalid()),
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
                if self.items.len() >= ITEM_LIMIT && !self.items.contains_key(&index) {
                    return Err(invalid());
                }
                if event["item"].to_string().len() > REPLAY_LIMIT {
                    return Err(invalid());
                }
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
                if arguments.len().saturating_add(delta.len()) > REPLAY_LIMIT {
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
                    if items.len() > ITEM_LIMIT {
                        return Err(invalid());
                    }
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
                let mut replay_bytes = 0usize;
                let mut tools = Vec::new();
                for (index, item) in &self.items {
                    if !self.complete.contains(index) {
                        return Err(invalid());
                    }
                    match item["type"].as_str() {
                        Some("reasoning") => {
                            replay_bytes = replay_bytes.saturating_add(item.to_string().len());
                            if replay_bytes > REPLAY_LIMIT {
                                return Err(invalid());
                            }
                            replay.push(item.clone());
                        }
                        Some("function_call") => {
                            let arguments = item["arguments"].as_str().ok_or_else(invalid)?;
                            if arguments.len() > REPLAY_LIMIT
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
                            tools.push(ProviderEvent::ToolUse {
                                id: id.into(),
                                name: name.into(),
                                input_json: arguments.into(),
                            });
                        }
                        Some("message") => {}
                        _ => return Err(invalid()),
                    }
                }
                if !replay.is_empty() {
                    out.push(ProviderEvent::ReasoningReplay(replay));
                }
                let stop_reason = if tools.is_empty() {
                    StopReason::End
                } else {
                    StopReason::ToolUse
                };
                out.extend(tools);
                if let Some(usage) = event["response"].get("usage").filter(|v| !v.is_null()) {
                    out.push(ProviderEvent::Usage {
                        input: usage["input_tokens"].as_u64().unwrap_or(0),
                        output: usage["output_tokens"].as_u64().unwrap_or(0),
                        cache_read: usage["input_tokens_details"]["cached_tokens"]
                            .as_u64()
                            .unwrap_or(0),
                        cache_write: 0,
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
    // Limit bytes before Eventsource buffers an unterminated/malicious SSE frame.
    let bytes = resp.bytes_stream().scan(0usize, |total, chunk| {
        let next = match chunk {
            Ok(chunk) => {
                *total = total.saturating_add(chunk.len());
                if *total > WIRE_LIMIT {
                    Err(std::io::Error::other("Responses byte limit"))
                } else {
                    Ok(chunk)
                }
            }
            Err(_) => Err(std::io::Error::other("Responses stream transport failure")),
        };
        futures::future::ready(Some(next))
    });
    let inner = Box::pin(bytes.eventsource());
    Box::pin(futures::stream::unfold(
        (inner, Assembler::default(), VecDeque::new(), false, cancel),
        |(mut inner, mut parser, mut pending, mut ended, cancel)| async move {
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
                    _ => {
                        ended = true;
                        pending.push_back(Err(invalid()));
                    }
                }
            }
        },
    ))
}
