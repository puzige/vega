//! Owner-only final provider boundary for previously persisted untrusted data.

use std::sync::Arc;

use futures::{StreamExt, future::BoxFuture};
use tokio_util::sync::CancellationToken;
use vega_runtime::{ChatRequest, EventStream, Provider, VegaError};

/// Re-reads owner-held credentials immediately before every model request.
/// The reader never enters a Debug value, request, event, audit, or log.
pub struct OwnerCredentialProvider {
    provider: Arc<dyn Provider>,
    known: Arc<dyn Fn() -> Result<Vec<String>, ()> + Send + Sync>,
    exceptions: Option<vega_runtime::CredentialExceptionFilter>,
}

impl OwnerCredentialProvider {
    /// Wrap a provider with the current owner-only credential authority.
    pub fn new(
        provider: Arc<dyn Provider>,
        known: Arc<dyn Fn() -> Result<Vec<String>, ()> + Send + Sync>,
    ) -> Self {
        Self {
            provider,
            known,
            exceptions: None,
        }
    }

    pub fn with_credential_exceptions(
        mut self,
        filter: vega_runtime::CredentialExceptionFilter,
    ) -> Self {
        self.exceptions = Some(filter);
        self
    }

    pub fn pre_attempt_guard_with_exceptions(
        known: vega_runtime::CredentialReader,
        filter: vega_runtime::CredentialExceptionFilter,
    ) -> impl Fn(&ChatRequest) -> Result<(), VegaError> + Send + Sync + 'static {
        move |request| reject_owner_secret_with_policy(request, known.as_ref(), Some(&filter))
    }

    /// The same final projection check for transports with internal retries.
    /// OpenAI calls it again immediately before every HTTP attempt, while the
    /// outer wrapper still protects other providers and auxiliary requests.
    pub fn pre_attempt_guard(
        known: Arc<dyn Fn() -> Result<Vec<String>, ()> + Send + Sync>,
    ) -> impl Fn(&ChatRequest) -> Result<(), VegaError> + Send + Sync + 'static {
        move |request| reject_owner_secret(request, known.as_ref())
    }
}

fn blocked() -> VegaError {
    VegaError::CredentialExposureBlocked
}

fn contains_json_secret(
    value: &serde_json::Value,
    credentials: &[String],
    exceptions: Option<&vega_runtime::CredentialExceptionFilter>,
) -> bool {
    match value {
        serde_json::Value::String(text) => contains_with_policy(text, credentials, exceptions),
        serde_json::Value::Array(items) => items
            .iter()
            .any(|item| contains_json_secret(item, credentials, exceptions)),
        serde_json::Value::Object(fields) => fields.iter().any(|(key, value)| {
            contains_with_policy(key, credentials, exceptions)
                || contains_json_secret(value, credentials, exceptions)
        }),
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {
            false
        }
    }
}

pub(crate) fn reject_owner_secret(
    request: &ChatRequest,
    reader: &(dyn Fn() -> Result<Vec<String>, ()> + Send + Sync),
) -> Result<(), VegaError> {
    reject_owner_secret_with_policy(request, reader, None)
}

fn contains_with_policy(
    text: &str,
    known: &[String],
    exceptions: Option<&vega_runtime::CredentialExceptionFilter>,
) -> bool {
    vega_runtime::redact_sensitive_credential_text_with_policy(text, known, exceptions) != text
}

fn first_json(content: &str) -> Option<serde_json::Value> {
    let mut stream = serde_json::Deserializer::from_str(content).into_iter::<serde_json::Value>();
    stream.next()?.ok()
}

fn history_observation(content: &str) -> Option<(serde_json::Value, bool)> {
    let value = first_json(content)?;
    let observation = &value["vega_history_observation"];
    if observation["kind"] == "conversation_details" {
        return Some((value, false));
    }
    if observation["kind"] == "tool_result" && observation["tool"] == "read_conversation" {
        let output = observation["output"].as_str()?;
        let nested = first_json(output)?;
        if nested["vega_history_observation"]["kind"] == "conversation_details" {
            return Some((value, true));
        }
    }
    None
}

pub(crate) fn history_fragment_sequences(
    messages: &[vega_runtime::ChatMessage],
) -> Result<Vec<String>, VegaError> {
    let mut groups =
        std::collections::BTreeMap::<(String, &'static str), Vec<(u64, String)>>::new();
    let mut bytes = 0usize;
    for message in messages {
        if !message.is_conversation_history_result() {
            continue;
        }
        let Some((observation, nested)) = history_observation(&message.content) else {
            if let Some(value) = first_json(&message.content) {
                let observation = &value["vega_history_observation"];
                if observation["kind"] == "tool_result"
                    && observation["tool"] == "read_conversation"
                    && observation["output"]
                        .as_str()
                        .is_some_and(|output| !output.trim_start().starts_with('{'))
                {
                    continue;
                }
                return Err(blocked());
            }
            if message.content.trim_start().starts_with('{') {
                return Err(blocked());
            }
            continue;
        };
        let value = if nested {
            let output = observation["vega_history_observation"]["output"]
                .as_str()
                .ok_or_else(blocked)?;
            first_json(output).ok_or_else(blocked)?
        } else {
            observation
        };
        let records = value["vega_history_observation"]["page"]["records"]
            .as_array()
            .ok_or_else(blocked)?;
        for record in records {
            let id = record["source_id"].as_str().ok_or_else(blocked)?;
            if id.contains(vega_runtime::PROVIDER_CREDENTIAL_REDACTION_MARKER) {
                return Err(blocked());
            }
            for field in ["content", "input", "output"] {
                let Some(text) = record[field]["text"].as_str() else {
                    continue;
                };
                let offset = record[field]["offset"].as_u64().ok_or_else(blocked)?;
                bytes = bytes.saturating_add(text.len());
                if bytes > 8 * 1024 * 1024 {
                    return Err(blocked());
                }
                groups
                    .entry((id.to_owned(), field))
                    .or_default()
                    .push((offset, text.to_owned()));
            }
        }
    }
    let mut sequences = Vec::new();
    for fragments in groups.values_mut() {
        fragments.sort_by_key(|(offset, _)| *offset);
        let mut start = 0u64;
        let mut run = Vec::<char>::new();
        for (offset, text) in fragments {
            let characters = text.chars().collect::<Vec<_>>();
            let end = start.saturating_add(run.len() as u64);
            if run.is_empty() || *offset > end {
                if !run.is_empty() {
                    sequences.push(run.iter().collect());
                }
                start = *offset;
                run = characters;
                continue;
            }
            let overlap_start = (*offset - start) as usize;
            let overlap = run
                .len()
                .saturating_sub(overlap_start)
                .min(characters.len());
            if run[overlap_start..overlap_start + overlap] != characters[..overlap] {
                return Err(blocked());
            }
            run.extend(characters.into_iter().skip(overlap));
        }
        if !run.is_empty() {
            sequences.push(run.iter().collect());
        }
    }
    Ok(sequences)
}

pub(crate) fn history_sequences_unsafe(
    sequences: &[String],
    credentials: &[String],
    exceptions: Option<&vega_runtime::CredentialExceptionFilter>,
) -> bool {
    sequences
        .iter()
        .any(|text| contains_with_policy(text, credentials, exceptions))
}

fn history_fragments_unsafe(
    request: &ChatRequest,
    credentials: &[String],
    exceptions: Option<&vega_runtime::CredentialExceptionFilter>,
) -> bool {
    history_fragment_sequences(&request.messages).map_or(true, |sequences| {
        history_sequences_unsafe(&sequences, credentials, exceptions)
    })
}

const UNAVAILABLE_SUMMARY: &str = "[Historical summary unavailable after a source or credential change; read original records if needed.]";

fn history_result_masked(message: &vega_runtime::ChatMessage) -> bool {
    if message.is_history_safety_masked() {
        return true;
    }
    if message.content == vega_runtime::CONVERSATION_HISTORY_RETRY_OUTPUT {
        return true;
    }
    first_json(&message.content).is_some_and(|value| {
        value["vega_history_observation"]["kind"] == "tool_result"
            && value["vega_history_observation"]["tool"] == "read_conversation"
            && value["vega_history_observation"]["output"]
                == vega_runtime::CONVERSATION_HISTORY_RETRY_OUTPUT
    })
}

fn history_result_unsafe(
    message: &vega_runtime::ChatMessage,
    report: &vega_runtime::HistorySafetyReport,
) -> bool {
    message.is_conversation_history_result()
        && !history_result_masked(message)
        && (report.invalidate_all
            || message
                .history_result_id()
                .is_none_or(|id| report.invalid_results.contains(id)))
}

fn summary_unsafe(
    message: &vega_runtime::ChatMessage,
    report: &vega_runtime::HistorySafetyReport,
) -> bool {
    message.is_historical_summary()
        && !message.is_history_safety_masked()
        && message.content != UNAVAILABLE_SUMMARY
        && (report.invalidate_all || report.invalidate_summary)
}

fn apply_history_report(request: &mut ChatRequest, report: &vega_runtime::HistorySafetyReport) {
    for message in &mut request.messages {
        if summary_unsafe(message, report) {
            message.content = UNAVAILABLE_SUMMARY.into();
            message.mark_history_safety_masked();
        } else if history_result_unsafe(message, report) {
            if let Some(mut observation) = first_json(&message.content)
                && observation["vega_history_observation"]["kind"] == "tool_result"
                && observation["vega_history_observation"]["tool"] == "read_conversation"
            {
                observation["vega_history_observation"]["output"] = serde_json::Value::String(
                    vega_runtime::CONVERSATION_HISTORY_RETRY_OUTPUT.into(),
                );
                message.content = observation.to_string();
            } else {
                message.content = vega_runtime::CONVERSATION_HISTORY_RETRY_OUTPUT.into();
            }
            message.mark_history_safety_masked();
        }
    }
}

fn inspect_history(
    request: &ChatRequest,
    check: &vega_runtime::HistorySafetyCheck,
    credentials: &[String],
    exceptions: Option<&vega_runtime::CredentialExceptionFilter>,
) -> vega_runtime::HistorySafetyReport {
    let mut active = request
        .messages
        .iter()
        .filter(|message| {
            message.is_conversation_history_result() && !history_result_masked(message)
        })
        .filter_map(|message| message.history_result_id().map(str::to_owned))
        .collect::<Vec<_>>();
    active.sort();
    active.dedup();
    let has_summary = request
        .messages
        .iter()
        .any(|message| message.is_historical_summary() && message.content != UNAVAILABLE_SUMMARY);
    if active.is_empty() && !has_summary {
        return vega_runtime::HistorySafetyReport::default();
    }
    check.inspect(&active, has_summary, credentials, exceptions)
}

fn history_request_unsafe(
    request: &ChatRequest,
    credentials: &[String],
    exceptions: Option<&vega_runtime::CredentialExceptionFilter>,
) -> bool {
    if let Some(check) = &request.history_safety {
        let report = inspect_history(request, check, credentials, exceptions);
        request.messages.iter().any(|message| {
            history_result_unsafe(message, &report) || summary_unsafe(message, &report)
        })
    } else {
        history_fragments_unsafe(request, credentials, exceptions)
    }
}

fn contains_chat_secret(
    raw: &vega_runtime::ChatRawState,
    known: &[String],
    exceptions: Option<&vega_runtime::CredentialExceptionFilter>,
) -> bool {
    raw.text
        .as_ref()
        .is_some_and(|(_, text)| contains_with_policy(text, known, exceptions))
        || raw.details.as_ref().is_some_and(|details| {
            details
                .iter()
                .any(|value| contains_json_secret(value, known, exceptions))
        })
}

fn contains_response_secret(
    message: &vega_runtime::ChatMessage,
    known: &[String],
    exceptions: Option<&vega_runtime::CredentialExceptionFilter>,
) -> bool {
    message
        .response_reasoning
        .iter()
        .any(|value| contains_json_secret(value, known, exceptions))
        || message
            .response_replay_layout
            .as_ref()
            .is_some_and(|layout| {
                layout
                    .response_id
                    .as_deref()
                    .is_some_and(|id| contains_with_policy(id, known, exceptions))
                    || layout.slots.iter().any(|slot| match slot {
                        vega_runtime::ResponseReplaySlot::Reasoning(value)
                        | vega_runtime::ResponseReplaySlot::Message(value) => {
                            contains_json_secret(value, known, exceptions)
                        }
                        vega_runtime::ResponseReplaySlot::FunctionCall {
                            item,
                            call_id,
                            name,
                            arguments,
                        } => {
                            contains_json_secret(item, known, exceptions)
                                || contains_with_policy(call_id, known, exceptions)
                                || contains_with_policy(name, known, exceptions)
                                || contains_with_policy(arguments, known, exceptions)
                        }
                    })
            })
}

pub(crate) fn reject_owner_secret_with_policy(
    request: &ChatRequest,
    reader: &(dyn Fn() -> Result<Vec<String>, ()> + Send + Sync),
    exceptions: Option<&vega_runtime::CredentialExceptionFilter>,
) -> Result<(), VegaError> {
    let known = reader().map_err(|_| VegaError::CredentialCheckUnavailable)?;
    if history_request_unsafe(request, &known, exceptions) {
        return Err(blocked());
    }
    if request.messages.iter().any(|message| {
        contains_with_policy(&message.content, &known, exceptions)
            || message
                .tool_call_id
                .as_deref()
                .is_some_and(|id| contains_with_policy(id, &known, exceptions))
            || message
                .reasoning_content
                .as_deref()
                .is_some_and(|reasoning| contains_with_policy(reasoning, &known, exceptions))
            || message
                .chat_reasoning
                .as_ref()
                .is_some_and(|raw| contains_chat_secret(raw, &known, exceptions))
            || contains_response_secret(message, &known, exceptions)
            || message.tool_calls.iter().any(|call| {
                contains_with_policy(&call.id, &known, exceptions)
                    || contains_with_policy(&call.name, &known, exceptions)
                    || contains_with_policy(&call.input_json, &known, exceptions)
            })
    }) || request.tools.iter().any(|tool| {
        contains_with_policy(&tool.name, &known, exceptions)
            || contains_with_policy(&tool.description, &known, exceptions)
            || contains_json_secret(&tool.input_schema, &known, exceptions)
            || serde_json::to_string(&tool.input_schema).map_or(true, |schema| {
                contains_with_policy(&schema, &known, exceptions)
            })
    }) {
        return Err(blocked());
    }
    Ok(())
}

fn project_json(
    value: &mut serde_json::Value,
    known: &[String],
    exceptions: Option<&vega_runtime::CredentialExceptionFilter>,
) -> Result<(), VegaError> {
    match value {
        serde_json::Value::String(text) => {
            *text =
                vega_runtime::redact_sensitive_credential_text_with_policy(text, known, exceptions)
        }
        serde_json::Value::Array(items) => {
            for item in items {
                project_json(item, known, exceptions)?;
            }
        }
        serde_json::Value::Object(fields) => {
            for (key, value) in fields {
                if contains_with_policy(key, known, exceptions) {
                    return Err(blocked());
                }
                project_json(value, known, exceptions)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub(crate) fn project_owner_secrets_with_policy(
    mut request: ChatRequest,
    reader: &(dyn Fn() -> Result<Vec<String>, ()> + Send + Sync),
    exceptions: Option<&vega_runtime::CredentialExceptionFilter>,
) -> Result<ChatRequest, VegaError> {
    let known = reader().map_err(|_| VegaError::CredentialCheckUnavailable)?;
    let report = if let Some(check) = &request.history_safety {
        inspect_history(&request, check, &known, exceptions)
    } else if history_fragments_unsafe(&request, &known, exceptions) {
        vega_runtime::HistorySafetyReport {
            invalidate_all: true,
            invalidate_summary: true,
            ..Default::default()
        }
    } else {
        vega_runtime::HistorySafetyReport::default()
    };
    apply_history_report(&mut request, &report);
    for message in &mut request.messages {
        if message
            .chat_reasoning
            .as_ref()
            .is_some_and(|raw| contains_chat_secret(raw, &known, exceptions))
            || contains_response_secret(message, &known, exceptions)
        {
            return Err(blocked());
        }
        message.content = vega_runtime::redact_sensitive_credential_text_with_policy(
            &message.content,
            &known,
            exceptions,
        );
        if let Some(reasoning) = &mut message.reasoning_content {
            *reasoning = vega_runtime::redact_sensitive_credential_text_with_policy(
                reasoning, &known, exceptions,
            );
        }
        for call in &mut message.tool_calls {
            match serde_json::from_str::<serde_json::Value>(&call.input_json) {
                Ok(mut value) => {
                    project_json(&mut value, &known, exceptions)?;
                    call.input_json = serde_json::to_string(&value).map_err(|_| blocked())?;
                }
                Err(_) if contains_with_policy(&call.input_json, &known, exceptions) => {
                    return Err(blocked());
                }
                Err(_) => {}
            }
        }
    }
    for (tool_index, tool) in request.tools.iter_mut().enumerate() {
        tool.description = vega_runtime::redact_sensitive_credential_text_with_policy(
            &tool.description,
            &known,
            exceptions,
        );
        if contains_json_secret(&tool.input_schema, &known, exceptions) {
            return Err(VegaError::CredentialProjectionInvalid { tool_index });
        }
    }
    Ok(request)
}

fn protected_stream(
    stream: EventStream,
    known: Vec<String>,
    exceptions: Option<vega_runtime::CredentialExceptionFilter>,
) -> EventStream {
    use vega_runtime::ProviderEvent;
    let known = Arc::new(known);
    let state = (
        stream,
        [String::new(), String::new(), String::new()],
        std::collections::VecDeque::new(),
        false,
    );
    Box::pin(futures::stream::unfold(
        state,
        move |(mut stream, mut pending, mut ready, mut ended)| {
            let known = known.clone();
            let exceptions = exceptions.clone();
            async move {
                loop {
                    if let Some(event) = ready.pop_front() {
                        return Some((event, (stream, pending, ready, ended)));
                    }
                    if ended {
                        return None;
                    }
                    let event = stream.next().await;
                    let part = match &event {
                        Some(Ok(ProviderEvent::TextDelta(text))) => Some((0, text)),
                        Some(Ok(ProviderEvent::ThinkingDelta(text))) => Some((1, text)),
                        Some(Ok(ProviderEvent::SummaryDelta(text))) => Some((2, text)),
                        _ => None,
                    };
                    if let Some((index, text)) = part {
                        pending[index].push_str(text);
                        let end = vega_runtime::credential_stream_safe_end(&pending[index], &known);
                        if end > 0 {
                            let raw: String = pending[index].drain(..end).collect();
                            let output = vega_runtime::redact_sensitive_credential_text_with_policy(
                                &raw,
                                &known,
                                exceptions.as_ref(),
                            );
                            ready.push_back(Ok(match index {
                                0 => ProviderEvent::TextDelta(output),
                                1 => ProviderEvent::ThinkingDelta(output),
                                _ => ProviderEvent::SummaryDelta(output),
                            }));
                        }
                        continue;
                    }
                    let flush = event.is_none()
                        || matches!(&event, Some(Err(_)) | Some(Ok(ProviderEvent::Done { .. })));
                    if flush {
                        for (index, text) in pending.iter_mut().enumerate() {
                            if !text.is_empty() {
                                let output =
                                    vega_runtime::redact_sensitive_credential_text_with_policy(
                                        text,
                                        &known,
                                        exceptions.as_ref(),
                                    );
                                text.clear();
                                ready.push_back(Ok(match index {
                                    0 => ProviderEvent::TextDelta(output),
                                    1 => ProviderEvent::ThinkingDelta(output),
                                    _ => ProviderEvent::SummaryDelta(output),
                                }));
                            }
                        }
                    }
                    match event {
                        Some(Ok(ProviderEvent::ResponseMetadata(mut metadata))) => {
                            if let Some(id) = &mut metadata.request_id {
                                *id = vega_runtime::redact_sensitive_credential_text_with_policy(
                                    id,
                                    &known,
                                    exceptions.as_ref(),
                                );
                            }
                            ready.push_back(Ok(ProviderEvent::ResponseMetadata(metadata)));
                        }
                        Some(Ok(ProviderEvent::ToolUse {
                            id,
                            name,
                            input_json,
                        })) => {
                            let projected = (|| {
                                if contains_with_policy(&id, &known, exceptions.as_ref())
                                    || contains_with_policy(&name, &known, exceptions.as_ref())
                                {
                                    return Err(blocked());
                                }
                                let input_json = if contains_with_policy(
                                    &input_json,
                                    &known,
                                    exceptions.as_ref(),
                                ) {
                                    let mut value: serde_json::Value =
                                        serde_json::from_str(&input_json).map_err(|_| blocked())?;
                                    project_json(&mut value, &known, exceptions.as_ref())?;
                                    serde_json::to_string(&value).map_err(|_| blocked())?
                                } else {
                                    input_json
                                };
                                Ok(ProviderEvent::ToolUse {
                                    id,
                                    name,
                                    input_json,
                                })
                            })();
                            if projected.is_err() {
                                ended = true;
                            }
                            ready.push_back(projected);
                        }
                        Some(Ok(ProviderEvent::ChatReasoningState(raw))) => {
                            if contains_chat_secret(&raw, &known, exceptions.as_ref()) {
                                ready.push_back(Err(blocked()));
                                ended = true;
                            } else {
                                ready.push_back(Ok(ProviderEvent::ChatReasoningState(raw)));
                            }
                        }
                        Some(Ok(ProviderEvent::ChatReasoningContent(presence))) => {
                            if matches!(&presence,vega_runtime::ChatReasoningPresence::NonEmpty(text) if contains_with_policy(text,&known,exceptions.as_ref()))
                            {
                                ready.push_back(Err(blocked()));
                                ended = true;
                            } else {
                                ready.push_back(Ok(ProviderEvent::ChatReasoningContent(presence)));
                            }
                        }
                        Some(Ok(ProviderEvent::ReasoningReplay(values))) => {
                            if values.iter().any(|value| {
                                contains_json_secret(value, &known, exceptions.as_ref())
                            }) {
                                ready.push_back(Err(blocked()));
                                ended = true;
                            } else {
                                ready.push_back(Ok(ProviderEvent::ReasoningReplay(values)));
                            }
                        }
                        Some(event) => {
                            if event.is_err() {
                                ended = true;
                            }
                            ready.push_back(event);
                        }
                        None => ended = true,
                    }
                }
            }
        },
    ))
}

impl Provider for OwnerCredentialProvider {
    fn chat_stream_with_submission(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
        submission: vega_runtime::ProviderRequestSubmission,
    ) -> BoxFuture<'static, Result<vega_runtime::ProviderStream, VegaError>> {
        let provider = self.provider.clone();
        let known = self.known.clone();
        let exceptions = self.exceptions.clone();
        Box::pin(async move {
            let request =
                project_owner_secrets_with_policy(request, known.as_ref(), exceptions.as_ref())?;
            reject_owner_secret_with_policy(&request, known.as_ref(), exceptions.as_ref())?;
            let credentials = known().map_err(|_| VegaError::CredentialCheckUnavailable)?;
            let mut response = provider
                .chat_stream_with_submission(request, cancel, submission)
                .await?;
            if let Some(id) = &mut response.metadata.request_id {
                *id = vega_runtime::redact_sensitive_credential_text_with_policy(
                    id,
                    &credentials,
                    exceptions.as_ref(),
                );
            }
            response.events = protected_stream(response.events, credentials, exceptions);
            Ok(response)
        })
    }

    fn compaction_provider_factory(&self) -> Option<vega_runtime::CompactionProviderFactory> {
        let factory = self.provider.compaction_provider_factory()?;
        let known = self.known.clone();
        let exceptions = self.exceptions.clone();
        Some(Box::new(move || {
            Ok(Arc::new(Self {
                provider: factory()?,
                known,
                exceptions,
            }))
        }))
    }

    fn model_snapshot(&self) -> Option<vega_runtime::model_compat::ResolvedModel> {
        self.provider.model_snapshot()
    }

    fn chat_stream_recovering(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<vega_runtime::ProviderStream, VegaError>> {
        let provider = self.provider.clone();
        let known = self.known.clone();
        let exceptions = self.exceptions.clone();
        Box::pin(async move {
            let request =
                project_owner_secrets_with_policy(request, known.as_ref(), exceptions.as_ref())?;
            reject_owner_secret_with_policy(&request, known.as_ref(), exceptions.as_ref())?;
            let credentials = known().map_err(|_| VegaError::CredentialCheckUnavailable)?;
            let mut response = provider.chat_stream_recovering(request, cancel).await?;
            if let Some(id) = &mut response.metadata.request_id {
                *id = vega_runtime::redact_sensitive_credential_text_with_policy(
                    id,
                    &credentials,
                    exceptions.as_ref(),
                );
            }
            response.events = protected_stream(response.events, credentials, exceptions);
            Ok(response)
        })
    }
    fn chat_stream_once(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        let provider = self.provider.clone();
        let known = self.known.clone();
        let exceptions = self.exceptions.clone();
        Box::pin(async move {
            let request =
                project_owner_secrets_with_policy(request, known.as_ref(), exceptions.as_ref())?;
            reject_owner_secret_with_policy(&request, known.as_ref(), exceptions.as_ref())?;
            let credentials = known().map_err(|_| VegaError::CredentialCheckUnavailable)?;
            let stream = provider.chat_stream_once(request, cancel).await?;
            Ok(protected_stream(stream, credentials, exceptions))
        })
    }

    fn chat_stream(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        let provider = self.provider.clone();
        let known = self.known.clone();
        let exceptions = self.exceptions.clone();
        Box::pin(async move {
            let request =
                project_owner_secrets_with_policy(request, known.as_ref(), exceptions.as_ref())?;
            reject_owner_secret_with_policy(&request, known.as_ref(), exceptions.as_ref())?;
            let credentials = known().map_err(|_| VegaError::CredentialCheckUnavailable)?;
            let stream = provider.chat_stream(request, cancel).await?;
            Ok(protected_stream(stream, credentials, exceptions))
        })
    }
}
