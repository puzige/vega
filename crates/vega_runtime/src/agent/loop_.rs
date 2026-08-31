use super::*;

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
    mut sink: F,
) -> Result<AgentOutcome, VegaError>
where
    F: FnMut(RuntimeEvent) -> Fut,
    Fut: Future<Output = Result<(), VegaError>>,
{
    macro_rules! emit {
        ($events:ident, $sink:ident, $event:expr) => {{
            let event = $event;
            $sink(event.clone()).await?;
            $events.push(event);
        }};
    }

    let mut messages = Vec::with_capacity(request.history.len() + 1);
    messages.push(ChatMessage::new(ChatRole::System, request.system_prompt));
    messages.extend(request.history);
    let mut completed = request.completed_tool_results;
    let tool_config = request.tool_config;
    let mut exact_rules: HashSet<RuntimeExactRule> =
        tool_config.exact_rules.iter().cloned().collect();
    let mut events = Vec::new();
    let mut final_text = String::new();
    let mut tool_call_count = 0usize;
    let mut executed_tool_call_count = 0usize;

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

        // C3: the logical provider call start is frozen immediately before
        // the first `chat_stream`; provider-internal HTTP retries reuse this
        // exact timestamp, later rounds capture a fresh one.
        let call_started_utc_seconds = unix_utc_seconds();
        let mut usage_seen = false;
        let chat_request = ChatRequest {
            model: request.model.clone(),
            messages: messages.clone(),
            tools: tool_definitions(tool_config.run_mode),
            max_tokens: request.max_tokens,
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
            match prepare_runtime_call(tools, &tool_config, call) {
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
        messages.push(ChatMessage::assistant_with_tools(
            assistant_text,
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
    }
}
