use super::*;
use crate::{ContextCheck, ContextCompactionRequest, ContextRuntimeError};

/// Runs the S4 headless agent loop with real fenced read/glob/grep tools.
pub async fn run_agent(
    provider: &dyn Provider,
    tools: &vega_tools::Tools,
    request: AgentRequest,
    cancel: CancellationToken,
) -> Result<AgentOutcome, VegaError> {
    run_agent_with_permission_sink(
        provider,
        tools,
        request,
        cancel,
        &RejectPermissionHook,
        |_| async { Ok(()) },
    )
    .await
}

pub(super) fn reasoning_budget_violation(
    delta_bytes: usize,
    turn_bytes: usize,
    run_bytes: usize,
) -> Option<(ReasoningBudgetScope, usize)> {
    if delta_bytes > REASONING_DELTA_MAX_BYTES {
        return Some((ReasoningBudgetScope::Delta, delta_bytes));
    }
    let next_turn = match turn_bytes.checked_add(delta_bytes) {
        Some(next) => next,
        None => return Some((ReasoningBudgetScope::Turn, usize::MAX)),
    };
    if next_turn > REASONING_TURN_MAX_BYTES {
        return Some((ReasoningBudgetScope::Turn, next_turn));
    }
    let next_run = match run_bytes.checked_add(delta_bytes) {
        Some(next) => next,
        None => return Some((ReasoningBudgetScope::Run, usize::MAX)),
    };
    if next_run > REASONING_RUN_MAX_BYTES {
        return Some((ReasoningBudgetScope::Run, next_run));
    }
    None
}

fn reasoning_budget_limit(scope: ReasoningBudgetScope) -> usize {
    match scope {
        ReasoningBudgetScope::Delta => REASONING_DELTA_MAX_BYTES,
        ReasoningBudgetScope::Turn => REASONING_TURN_MAX_BYTES,
        ReasoningBudgetScope::Run => REASONING_RUN_MAX_BYTES,
    }
}

/// Runs the agent and delivers each owned runtime event to an async sink at
/// its real lifecycle boundary before the loop may continue when required.
///
/// Returning an error from `sink` stops the task immediately. Conversation
/// uses awaited acknowledgements to persist critical state before side
/// effects while allowing text deltas to enter a bounded batching pipeline.
pub async fn run_agent_with_sink<F, Fut>(
    provider: &dyn Provider,
    tools: &vega_tools::Tools,
    request: AgentRequest,
    cancel: CancellationToken,
    sink: F,
) -> Result<AgentOutcome, VegaError>
where
    F: FnMut(RuntimeEvent) -> Fut,
    Fut: Future<Output = Result<(), VegaError>>,
{
    run_agent_with_permission_sink(
        provider,
        tools,
        request,
        cancel,
        &RejectPermissionHook,
        sink,
    )
    .await
}

/// Runs the full six-tool loop with an object-safe permission hook.
pub async fn run_agent_with_permission_sink<F, Fut>(
    provider: &dyn Provider,
    tools: &vega_tools::Tools,
    request: AgentRequest,
    cancel: CancellationToken,
    permission_hook: &dyn RuntimePermissionHook,
    sink: F,
) -> Result<AgentOutcome, VegaError>
where
    F: FnMut(RuntimeEvent) -> Fut,
    Fut: Future<Output = Result<(), VegaError>>,
{
    run_agent_with_permission_sink_and_context(
        provider,
        tools,
        request,
        cancel,
        permission_hook,
        None,
        sink,
    )
    .await
}

/// Permission-aware loop variant with a conversation-owned borrowed
/// compaction hook.  The borrowed form lets the conversation layer use the
/// provider/store lifetime it already owns without forcing runtime to depend
/// on that layer or to manufacture an `Arc` from a borrowed provider.
pub async fn run_agent_with_permission_sink_and_context<F, Fut>(
    provider: &dyn Provider,
    tools: &vega_tools::Tools,
    request: AgentRequest,
    cancel: CancellationToken,
    permission_hook: &dyn RuntimePermissionHook,
    external_context_hook: Option<&dyn crate::ContextCompactionHook>,
    mut sink: F,
) -> Result<AgentOutcome, VegaError>
where
    F: FnMut(RuntimeEvent) -> Fut,
    Fut: Future<Output = Result<(), VegaError>>,
{
    crate::images::validate_messages(&request.history)?;
    if let Some(reasoning) = &request.reasoning {
        reasoning.validate()?;
        if reasoning.model != request.model {
            return Err(VegaError::ReasoningSelectionInvalid {
                message: "reasoning profile model does not match request model".to_string(),
            });
        }
    }

    macro_rules! emit {
        ($events:ident, $sink:ident, $event:expr) => {{
            let event = $event;
            $sink(event.clone()).await?;
            $events.push(event);
        }};
    }

    let mut messages = Vec::with_capacity(request.history.len() + 1);
    let system_prompt = request.system_prompt.clone();
    messages.push(ChatMessage::new(ChatRole::System, system_prompt.clone()));
    messages.extend(request.history);
    let context_budget = request.context_budget;
    let request_context_hook = request.context_compaction_hook.clone();
    let context_operation_id = request
        .context_operation_id
        .clone()
        .unwrap_or_else(|| ulid::Ulid::generate().to_string());
    let mut context_source_version = request.context_source_version;
    let mut context_source_fingerprint = request.context_source_fingerprint.clone();
    // Durable source identity is paired with a local live-projection revision:
    // tool rounds can append content before SQLite's fingerprint catches up.
    let mut live_projection_revision = 0_u64;
    let mut attempted_context_source = None::<(u64, Option<String>, u64)>;
    let mut completed = request.completed_tool_results;
    let tool_config = request.tool_config;
    let capabilities = RunCapabilitySnapshot::freeze(
        tool_config.run_mode,
        tool_config.permission_mode,
        tool_config.mcp_candidates.clone(),
    )?;
    let mut exact_rules: HashSet<RuntimeExactRule> =
        tool_config.exact_rules.iter().cloned().collect();
    let mut events = Vec::new();
    let mut final_text = String::new();
    let mut tool_call_count = 0usize;
    let mut executed_tool_call_count = 0usize;
    let mut reasoning_run_bytes = 0usize;

    loop {
        if cancel.is_cancelled() {
            emit!(events, sink, RuntimeEvent::Interrupted);
            return Ok(outcome(
                events,
                messages,
                final_text,
                tool_call_count,
                executed_tool_call_count,
                true,
                false,
            ));
        }

        let tool_definitions = capabilities.definitions().to_vec();
        if let Some(budget) = context_budget {
            let estimate = crate::estimate_wire_context(&messages, &tool_definitions)
                .map_err(|error| VegaError::Context(error.into()))?;
            let check = budget
                .check(estimate)
                .map_err(|error| VegaError::Context(error.into()))?;
            let should_compact = matches!(check, ContextCheck::Triggered | ContextCheck::OverLimit)
                && budget.automatic_compaction();
            if should_compact {
                let source_version = context_source_version.unwrap_or_default();
                let source_fingerprint = context_source_fingerprint.clone();
                if attempted_context_source
                    == Some((
                        source_version,
                        source_fingerprint.clone(),
                        live_projection_revision,
                    ))
                {
                    return Err(VegaError::Context(ContextRuntimeError::AlreadyAttempted));
                }
                let hook = external_context_hook.or(request_context_hook.as_deref());
                let Some(hook) = hook else {
                    return Err(VegaError::Context(ContextRuntimeError::MissingHook));
                };
                if cancel.is_cancelled() {
                    emit!(events, sink, RuntimeEvent::Interrupted);
                    return Ok(outcome(
                        events,
                        messages,
                        final_text,
                        tool_call_count,
                        executed_tool_call_count,
                        true,
                        false,
                    ));
                }
                let target_tokens = budget
                    .target_tokens()
                    .map_err(|error| VegaError::Context(error.into()))?;
                let generation = live_projection_revision.saturating_add(1);
                // The source fence is deliberately not the operation
                // identity: a retry can observe the same source fingerprint
                // after a crash.  The conversation-owned run id (normally
                // the assistant message id) plus this run-local generation
                // keeps pending/terminal rows from different attempts
                // independent.  Source metadata remains in the status row.
                let operation_key = format!(
                    "{context_operation_id}:context-generation-{generation}:attempt-{}",
                    ulid::Ulid::generate()
                );
                emit!(
                    events,
                    sink,
                    RuntimeEvent::ContextCompactionStatusUpdated {
                        status: crate::ContextCompactionStatusUpdate {
                            operation_key: operation_key.clone(),
                            generation,
                            phase: crate::ContextCompactionPhase::Started,
                            source_version,
                            estimated_tokens: estimate.input_tokens,
                            input_budget: budget.input_budget(),
                            target_tokens,
                            usage: crate::ContextCompactionUsageState::Pending,
                            failure: None,
                        },
                    }
                );
                attempted_context_source = Some((
                    source_version,
                    source_fingerprint.clone(),
                    live_projection_revision,
                ));
                let compacted = hook
                    .compact(
                        ContextCompactionRequest {
                            system_prompt: system_prompt.clone(),
                            messages: messages.iter().skip(1).cloned().collect(),
                            tools: tool_definitions.clone(),
                            budget,
                            estimate,
                            target_tokens,
                            source_version,
                            source_fingerprint,
                            require_source_fence: false,
                            source_owner_id: Some(context_operation_id.clone()),
                        },
                        cancel.clone(),
                    )
                    .await;
                let compacted = match compacted {
                    Ok(compacted) => compacted,
                    Err(failure) => {
                        let usage_state = failure.usage.as_ref().map_or(
                            crate::ContextCompactionUsageState::Unknown,
                            |usage| crate::ContextCompactionUsageState::Known {
                                priced: usage.pricing.is_some(),
                            },
                        );
                        if let Some(usage) = failure.usage {
                            emit!(
                                events,
                                sink,
                                RuntimeEvent::ContextCompactionUsageUpdated { usage }
                            );
                        }
                        if cancel.is_cancelled() {
                            emit!(
                                events,
                                sink,
                                RuntimeEvent::ContextCompactionStatusUpdated {
                                    status: crate::ContextCompactionStatusUpdate {
                                        operation_key: operation_key.clone(),
                                        generation,
                                        phase: crate::ContextCompactionPhase::Cancelled,
                                        source_version,
                                        estimated_tokens: estimate.input_tokens,
                                        input_budget: budget.input_budget(),
                                        target_tokens,
                                        usage: usage_state,
                                        failure: Some(
                                            crate::ContextCompactionStatusFailure::Cancelled,
                                        ),
                                    },
                                }
                            );
                            emit!(events, sink, RuntimeEvent::Interrupted);
                            return Ok(outcome(
                                events,
                                messages,
                                final_text,
                                tool_call_count,
                                executed_tool_call_count,
                                true,
                                false,
                            ));
                        }
                        emit!(
                            events,
                            sink,
                            RuntimeEvent::ContextCompactionStatusUpdated {
                                status: crate::ContextCompactionStatusUpdate {
                                    operation_key: operation_key.clone(),
                                    generation,
                                    phase: crate::ContextCompactionPhase::Failed,
                                    source_version,
                                    estimated_tokens: estimate.input_tokens,
                                    input_budget: budget.input_budget(),
                                    target_tokens,
                                    usage: usage_state,
                                    failure: Some(
                                        crate::ContextCompactionStatusFailure::from_error(
                                            failure.error.as_ref(),
                                        ),
                                    ),
                                },
                            }
                        );
                        return Err(*failure.error);
                    }
                };
                let usage_state = compacted.usage.as_ref().map_or(
                    crate::ContextCompactionUsageState::Unknown,
                    |usage| crate::ContextCompactionUsageState::Known {
                        priced: usage.pricing.is_some(),
                    },
                );
                if let Some(usage) = compacted.usage.clone() {
                    emit!(
                        events,
                        sink,
                        RuntimeEvent::ContextCompactionUsageUpdated { usage }
                    );
                }
                if compacted
                    .messages
                    .iter()
                    .any(|message| message.role == ChatRole::System)
                {
                    emit!(
                        events,
                        sink,
                        RuntimeEvent::ContextCompactionStatusUpdated {
                            status: crate::ContextCompactionStatusUpdate {
                                operation_key: operation_key.clone(),
                                generation,
                                phase: crate::ContextCompactionPhase::Failed,
                                source_version,
                                estimated_tokens: estimate.input_tokens,
                                input_budget: budget.input_budget(),
                                target_tokens,
                                usage: usage_state,
                                failure: Some(
                                    crate::ContextCompactionStatusFailure::InvalidSummary,
                                ),
                            },
                        }
                    );
                    return Err(VegaError::Context(
                        ContextRuntimeError::SystemMessageInResult,
                    ));
                }
                let system = messages.first().cloned().ok_or(VegaError::Context(
                    ContextRuntimeError::SystemMessageInResult,
                ))?;
                let mut next_messages = Vec::with_capacity(compacted.messages.len() + 1);
                next_messages.push(system);
                next_messages.extend(compacted.messages);
                let compacted_estimate =
                    crate::estimate_wire_context(&next_messages, &tool_definitions)
                        .map_err(|error| VegaError::Context(error.into()))?;
                let target_tokens = budget
                    .target_tokens()
                    .map_err(|error| VegaError::Context(error.into()))?;
                if compacted_estimate.input_tokens > target_tokens
                    || compacted_estimate.input_tokens > budget.input_budget()
                {
                    emit!(
                        events,
                        sink,
                        RuntimeEvent::ContextCompactionStatusUpdated {
                            status: crate::ContextCompactionStatusUpdate {
                                operation_key: operation_key.clone(),
                                generation,
                                phase: crate::ContextCompactionPhase::Failed,
                                source_version,
                                estimated_tokens: estimate.input_tokens,
                                input_budget: budget.input_budget(),
                                target_tokens,
                                usage: usage_state,
                                failure: Some(crate::ContextCompactionStatusFailure::OverLimit,),
                            },
                        }
                    );
                    return Err(VegaError::Context(ContextRuntimeError::ResultOverLimit {
                        estimated_tokens: compacted_estimate.input_tokens,
                        target_tokens,
                    }));
                }
                if cancel.is_cancelled() {
                    // The hook has already installed a valid durable
                    // checkpoint, but the run must not replace its live
                    // request projection or issue another provider call after
                    // cancellation.  Report the durable result separately.
                    emit!(
                        events,
                        sink,
                        RuntimeEvent::ContextCompactionStatusUpdated {
                            status: crate::ContextCompactionStatusUpdate {
                                operation_key: operation_key.clone(),
                                generation,
                                phase: crate::ContextCompactionPhase::Succeeded,
                                source_version,
                                estimated_tokens: estimate.input_tokens,
                                input_budget: budget.input_budget(),
                                target_tokens,
                                usage: usage_state,
                                failure: None,
                            },
                        }
                    );
                    emit!(events, sink, RuntimeEvent::Interrupted);
                    return Ok(outcome(
                        events,
                        messages,
                        final_text,
                        tool_call_count,
                        executed_tool_call_count,
                        true,
                        false,
                    ));
                }
                messages = next_messages;
                live_projection_revision = generation;
                context_source_version = Some(compacted.source_version);
                context_source_fingerprint = compacted.source_fingerprint;
                emit!(
                    events,
                    sink,
                    RuntimeEvent::ContextCompactionStatusUpdated {
                        status: crate::ContextCompactionStatusUpdate {
                            operation_key,
                            generation,
                            phase: crate::ContextCompactionPhase::Succeeded,
                            source_version,
                            estimated_tokens: estimate.input_tokens,
                            input_budget: budget.input_budget(),
                            target_tokens,
                            usage: usage_state,
                            failure: None,
                        },
                    }
                );
            } else if matches!(check, ContextCheck::OverLimit) {
                return Err(VegaError::Context(ContextRuntimeError::OverLimit {
                    estimated_tokens: estimate.input_tokens,
                    input_budget: budget.input_budget(),
                }));
            }
        }

        // C3: the logical provider call start is frozen immediately before
        // the first `chat_stream`; provider-internal HTTP retries reuse this
        // exact timestamp, later rounds capture a fresh one.
        let call_started_utc_seconds = unix_utc_seconds();
        let mut usage_seen = false;
        let mut reasoning_turn_bytes = 0usize;
        let chat_request = ChatRequest {
            model: request.model.clone(),
            messages: messages.clone(),
            tools: tool_definitions,
            // The reserved output capacity protects the input budget; it is
            // not a request to generate that many tokens on every round.
            // Keep provider defaults when no explicit generation cap exists.
            max_tokens: request.max_tokens.map(|cap| {
                context_budget.map_or(cap, |budget| cap.min(budget.output_reserve() as u32))
            }),
            reasoning: request.reasoning.clone(),
        };
        let mut stream = match provider.chat_stream(chat_request, cancel.clone()).await {
            Ok(stream) => stream,
            Err(VegaError::Cancelled) => {
                emit!(events, sink, RuntimeEvent::Interrupted);
                return Ok(outcome(
                    events,
                    messages,
                    final_text,
                    tool_call_count,
                    executed_tool_call_count,
                    true,
                    false,
                ));
            }
            Err(error) => {
                emit!(events, sink, RuntimeEvent::Error(Arc::new(error)));
                return Ok(outcome(
                    events,
                    messages,
                    final_text,
                    tool_call_count,
                    executed_tool_call_count,
                    false,
                    true,
                ));
            }
        };

        let mut assistant_text = String::new();
        let preserve_reasoning_content = request
            .reasoning
            .as_ref()
            .is_some_and(|reasoning| reasoning.preserve_reasoning_content);
        let mut assistant_reasoning = String::new();
        let mut calls = Vec::new();
        let mut stop_reason = None;
        loop {
            let next = tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    emit!(events, sink, RuntimeEvent::Interrupted);
                    return Ok(outcome(
                        events,
                        messages,
                        final_text,
                        tool_call_count,
                        executed_tool_call_count,
                        true,
                        false,
                    ));
                }
                next = stream.next() => next,
            };
            let Some(item) = next else { break };
            match item {
                Ok(ProviderEvent::TextDelta(delta)) => {
                    assistant_text.push_str(&delta);
                    final_text.push_str(&delta);
                    emit!(events, sink, RuntimeEvent::TextDelta(delta));
                }
                Ok(ProviderEvent::ThinkingDelta(delta)) => {
                    if let Some((scope, observed_bytes)) = reasoning_budget_violation(
                        delta.len(),
                        reasoning_turn_bytes,
                        reasoning_run_bytes,
                    ) {
                        let error = Arc::new(VegaError::ReasoningBudgetExceeded {
                            scope,
                            limit_bytes: reasoning_budget_limit(scope),
                            observed_bytes,
                        });
                        cancel.cancel();
                        emit!(events, sink, RuntimeEvent::Error(error));
                        return Ok(outcome(
                            events,
                            messages,
                            final_text,
                            tool_call_count,
                            executed_tool_call_count,
                            false,
                            true,
                        ));
                    }
                    reasoning_turn_bytes += delta.len();
                    reasoning_run_bytes += delta.len();
                    if preserve_reasoning_content {
                        assistant_reasoning.push_str(&delta);
                    }
                    // Preserve the existing event contract. The conversation
                    // layer keeps this out of visible content, persistence,
                    // cost, and Debug projections.
                    emit!(events, sink, RuntimeEvent::ThinkingDelta(delta));
                }
                Ok(ProviderEvent::ToolUse {
                    id,
                    name,
                    input_json,
                }) => {
                    calls.push(RuntimeToolCall {
                        id,
                        name,
                        input_json,
                    });
                }
                Ok(ProviderEvent::Usage {
                    input,
                    output,
                    cache_read,
                    cache_write,
                }) => {
                    // C3: exactly one terminal usage per provider call;
                    // duplicates and usage-after-terminal fail closed.
                    if stop_reason.is_some() {
                        emit!(
                            events,
                            sink,
                            RuntimeEvent::Error(Arc::new(VegaError::Provider {
                                status: None,
                                message: "usage event after terminal done".to_string(),
                                retryable: false,
                            }))
                        );
                        return Ok(outcome(
                            events,
                            messages,
                            final_text,
                            tool_call_count,
                            executed_tool_call_count,
                            false,
                            true,
                        ));
                    }
                    if usage_seen {
                        emit!(
                            events,
                            sink,
                            RuntimeEvent::Error(Arc::new(VegaError::Provider {
                                status: None,
                                message: "duplicate usage event in one provider call".to_string(),
                                retryable: false,
                            }))
                        );
                        return Ok(outcome(
                            events,
                            messages,
                            final_text,
                            tool_call_count,
                            executed_tool_call_count,
                            false,
                            true,
                        ));
                    }
                    usage_seen = true;
                    let usage = RuntimeTokenUsage {
                        input,
                        output,
                        cache_read,
                        cache_write,
                    };
                    let (cost_microcents, pricing) = match request.pricing_catalog.as_ref() {
                        Some(catalog) => {
                            let quote = catalog.quote(
                                &request.model,
                                vega_token::UsageCounts {
                                    input: usage.input,
                                    output: usage.output,
                                    cache_read: usage.cache_read,
                                    cache_write: usage.cache_write,
                                },
                                call_started_utc_seconds,
                            );
                            match quote {
                                Ok(quote) => (
                                    quote.cost_microcents,
                                    Some(RuntimeUsagePricing {
                                        version: quote.pricing_version.to_string(),
                                        profile: match quote.profile {
                                            PricingProfile::Base => "base".to_string(),
                                            PricingProfile::PeakUtcWeekly => {
                                                "peak_utc_weekly".to_string()
                                            }
                                        },
                                        call_started_at: call_started_utc_seconds,
                                    }),
                                ),
                                Err(vega_token::PricingError::ModelNotFound { .. }) => {
                                    // C3 run preflight: an unpriced model keeps
                                    // legacy zero-cost semantics (guides the
                                    // user to Settings) instead of failing the
                                    // run.
                                    (0, None)
                                }
                                Err(error) => {
                                    // Invalid usage / overflow fails closed: no
                                    // zero or partial usage row may be written.
                                    emit!(
                                        events,
                                        sink,
                                        RuntimeEvent::Error(Arc::new(VegaError::Provider {
                                            status: None,
                                            message: format!("usage pricing failed: {error}"),
                                            retryable: false,
                                        }))
                                    );
                                    return Ok(outcome(
                                        events,
                                        messages,
                                        final_text,
                                        tool_call_count,
                                        executed_tool_call_count,
                                        false,
                                        true,
                                    ));
                                }
                            }
                        }
                        None => (0, None),
                    };
                    emit!(
                        events,
                        sink,
                        RuntimeEvent::UsageUpdated {
                            usage,
                            cost_microcents,
                            pricing,
                        }
                    );
                }
                Ok(ProviderEvent::Done {
                    stop_reason: reason,
                }) => stop_reason = Some(reason),
                Err(VegaError::Cancelled) => {
                    emit!(events, sink, RuntimeEvent::Interrupted);
                    return Ok(outcome(
                        events,
                        messages,
                        final_text,
                        tool_call_count,
                        executed_tool_call_count,
                        true,
                        false,
                    ));
                }
                Err(error) => {
                    emit!(events, sink, RuntimeEvent::Error(Arc::new(error)));
                    return Ok(outcome(
                        events,
                        messages,
                        final_text,
                        tool_call_count,
                        executed_tool_call_count,
                        false,
                        true,
                    ));
                }
            }
        }

        if calls.is_empty() {
            messages.push(ChatMessage::new(ChatRole::Assistant, assistant_text));
            let finish = match stop_reason.unwrap_or(StopReason::End) {
                StopReason::Length => RuntimeFinishReason::Length,
                StopReason::End | StopReason::ToolUse => RuntimeFinishReason::End,
            };
            emit!(events, sink, RuntimeEvent::Finished(finish));
            return Ok(outcome(
                events,
                messages,
                final_text,
                tool_call_count,
                executed_tool_call_count,
                false,
                false,
            ));
        }

        let mut prepared_calls = Vec::with_capacity(calls.len());
        for call in calls {
            match prepare_runtime_call(tools, &tool_config, &capabilities, call) {
                Ok(prepared) => prepared_calls.push(prepared),
                Err(error) => {
                    emit!(events, sink, RuntimeEvent::Error(Arc::new(error)));
                    return Ok(outcome(
                        events,
                        messages,
                        final_text,
                        tool_call_count,
                        executed_tool_call_count,
                        false,
                        true,
                    ));
                }
            }
        }
        let wire_calls = prepared_calls
            .iter()
            .map(|prepared| {
                let call = prepared.call();
                ChatToolCall {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    input_json: call.input_json.clone(),
                }
            })
            .collect();
        messages.push(ChatMessage::assistant_with_tools_and_reasoning(
            assistant_text,
            preserve_reasoning_content.then_some(assistant_reasoning),
            wire_calls,
        ));

        for prepared in prepared_calls {
            let call = prepared.call().clone();
            if tool_call_count >= TOOL_CALL_LIMIT {
                let notice = format!(
                    "Tool call limit ({TOOL_CALL_LIMIT}) reached; stopping without executing additional tools."
                );
                final_text.push_str(&notice);
                emit!(events, sink, RuntimeEvent::TextDelta(notice.clone()));
                messages.push(ChatMessage::new(ChatRole::Assistant, notice));
                emit!(
                    events,
                    sink,
                    RuntimeEvent::Finished(RuntimeFinishReason::ToolLimit)
                );
                return Ok(outcome(
                    events,
                    messages,
                    final_text,
                    tool_call_count,
                    executed_tool_call_count,
                    false,
                    false,
                ));
            }
            tool_call_count += 1;
            if tool_config.foreign_call_ids.contains(&call.id) {
                let conflict = conflict_result(&call);
                emit!(
                    events,
                    sink,
                    RuntimeEvent::ToolCallConflict {
                        call: call.clone(),
                        result: conflict.clone(),
                    }
                );
                messages.push(ChatMessage::tool_result(call.id, conflict.output));
                continue;
            }
            if let PreparedRuntimeCall::InvalidWriteEdit { result, .. } = &prepared {
                let mut terminal = terminal_result(
                    &call,
                    result.clone(),
                    RuntimeToolStatus::Rejected,
                    Some(validation_audit()),
                );
                if let Some(prior) = completed.get(&call.id).cloned() {
                    if prior.tool != call.name
                        || !runtime_inputs_semantically_equal(
                            &call.name,
                            &prior.input_json,
                            &call.input_json,
                        )
                    {
                        let conflict = conflict_result(&call);
                        emit!(
                            events,
                            sink,
                            RuntimeEvent::ToolCallConflict {
                                call: call.clone(),
                                result: conflict.clone(),
                            }
                        );
                        messages.push(ChatMessage::tool_result(&call.id, &conflict.output));
                        continue;
                    }
                    terminal = prior.result;
                    terminal.reused = true;
                    terminal.truncated = None;
                }
                emit!(
                    events,
                    sink,
                    RuntimeEvent::ToolCallValidationRejected {
                        call: call.clone(),
                        result: terminal.clone(),
                    }
                );
                messages.push(ChatMessage::tool_result(&call.id, &terminal.output));
                completed.insert(
                    call.id.clone(),
                    CompletedToolCall {
                        tool: call.name.clone(),
                        input_json: call.input_json.clone(),
                        result: terminal,
                    },
                );
                continue;
            }
            if let Some(prior) = completed.get(&call.id).cloned() {
                if prior.tool != call.name
                    || !runtime_inputs_semantically_equal(
                        &call.name,
                        &prior.input_json,
                        &call.input_json,
                    )
                {
                    let conflict = conflict_result(&call);
                    emit!(
                        events,
                        sink,
                        RuntimeEvent::ToolCallConflict {
                            call: call.clone(),
                            result: conflict.clone(),
                        }
                    );
                    messages.push(ChatMessage::tool_result(call.id, conflict.output));
                    continue;
                }
                emit!(events, sink, RuntimeEvent::ToolCallProposed(call.clone()));
                let mut result = prior.result;
                result.reused = true;
                result.truncated = None;
                emit!(
                    events,
                    sink,
                    RuntimeEvent::ToolCallOutput {
                        call_id: call.id.clone(),
                        chunk: result.output.clone(),
                    }
                );
                emit!(events, sink, RuntimeEvent::ToolCallFinished(result.clone()));
                messages.push(ChatMessage::tool_result(call.id, result.output));
                continue;
            }
            if cancel.is_cancelled() {
                emit!(events, sink, RuntimeEvent::Interrupted);
                return Ok(outcome(
                    events,
                    messages,
                    final_text,
                    tool_call_count,
                    executed_tool_call_count,
                    true,
                    false,
                ));
            }
            emit!(events, sink, RuntimeEvent::ToolCallProposed(call.clone()));

            let authorization = authorize_call(
                &prepared,
                &tool_config,
                &exact_rules,
                permission_hook,
                &cancel,
            )
            .await?;
            let (mut result, cancelled_while_running) = match authorization {
                Authorization::Terminal(result) => (result, false),
                Authorization::Approved {
                    audit,
                    remember_rule,
                } => {
                    emit!(
                        events,
                        sink,
                        RuntimeEvent::ToolCallApproved {
                            call_id: call.id.clone(),
                            audit: audit.clone(),
                            remember_rule: remember_rule.clone(),
                        }
                    );
                    if let Some(target) = remember_rule {
                        exact_rules.insert(RuntimeExactRule {
                            tool: target.tool,
                            pattern: target.exact_pattern,
                        });
                    }
                    emit!(
                        events,
                        sink,
                        RuntimeEvent::ToolCallRunning {
                            call_id: call.id.clone(),
                        }
                    );
                    if cancel.is_cancelled() {
                        (
                            terminal_result(
                                &call,
                                CANCELLED_BEFORE_EXECUTION_OUTPUT.to_string(),
                                RuntimeToolStatus::Cancelled,
                                Some(audit),
                            ),
                            true,
                        )
                    } else {
                        executed_tool_call_count += 1;
                        let (mut result, cancelled) =
                            execute_prepared_waiting(prepared, tools, &cancel).await;
                        result.approval = Some(audit);
                        (result, cancelled)
                    }
                }
            };
            if cancelled_while_running {
                result.status = RuntimeToolStatus::Cancelled;
            }
            emit!(
                events,
                sink,
                RuntimeEvent::ToolCallOutput {
                    call_id: call.id.clone(),
                    chunk: result.output.clone(),
                }
            );
            emit!(events, sink, RuntimeEvent::ToolCallFinished(result.clone()));
            if let Some(target) = &result.remember_rule {
                exact_rules.insert(RuntimeExactRule {
                    tool: target.tool,
                    pattern: target.exact_pattern.clone(),
                });
            }
            messages.push(ChatMessage::tool_result(&call.id, &result.output));
            completed.insert(
                call.id,
                CompletedToolCall {
                    tool: call.name,
                    input_json: call.input_json,
                    result,
                },
            );

            if cancelled_while_running || cancel.is_cancelled() {
                emit!(events, sink, RuntimeEvent::Interrupted);
                return Ok(outcome(
                    events,
                    messages,
                    final_text,
                    tool_call_count,
                    executed_tool_call_count,
                    true,
                    false,
                ));
            }
        }
        live_projection_revision = live_projection_revision.saturating_add(1);
    }
}
