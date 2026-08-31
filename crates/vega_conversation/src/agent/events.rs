use super::*;

pub(crate) async fn process_runtime_events<F>(
    mut receiver: mpsc::Receiver<RuntimeEnvelope>,
    actor: &PersistenceActor,
    message_id: &str,
    streamed_content: &mut String,
    events: &mut Vec<ConversationEvent>,
    event_sink: &mut F,
    cancel: CancellationToken,
) -> Result<(), VegaError>
where
    F: FnMut(&ConversationEvent) -> Result<(), VegaError>,
{
    let mut pending_text = Vec::new();
    let mut pending_text_bytes = 0usize;
    let mut batch_deadline = None;

    loop {
        let received = if let Some(deadline) = batch_deadline {
            match tokio::time::timeout_at(deadline, receiver.recv()).await {
                Ok(envelope) => envelope,
                Err(_) => {
                    if let Err(error) = flush_text_batch(
                        actor,
                        streamed_content,
                        &mut pending_text,
                        events,
                        event_sink,
                    )
                    .await
                    {
                        cancel.cancel();
                        return Err(error);
                    }
                    batch_deadline = None;
                    pending_text_bytes = 0;
                    continue;
                }
            }
        } else {
            receiver.recv().await
        };

        let Some(envelope) = received else {
            if let Err(error) = flush_text_batch(
                actor,
                streamed_content,
                &mut pending_text,
                events,
                event_sink,
            )
            .await
            {
                cancel.cancel();
                return Err(error);
            }
            return Ok(());
        };

        let RuntimeEnvelope { event, ack } = envelope;
        let result = if let RuntimeEvent::TextDelta(delta) = &event {
            streamed_content.push_str(delta);
            pending_text_bytes = pending_text_bytes.saturating_add(delta.len());
            if let Some(converted) = from_runtime_event(message_id, &event) {
                pending_text.push(converted);
            }
            if pending_text_bytes >= TEXT_BATCH_MAX_BYTES {
                let result = flush_text_batch(
                    actor,
                    streamed_content,
                    &mut pending_text,
                    events,
                    event_sink,
                )
                .await;
                pending_text_bytes = 0;
                batch_deadline = None;
                result
            } else {
                if batch_deadline.is_none() {
                    batch_deadline = Some(tokio::time::Instant::now() + TEXT_BATCH_MAX_DELAY);
                }
                Ok(())
            }
        } else {
            let flushed = flush_text_batch(
                actor,
                streamed_content,
                &mut pending_text,
                events,
                event_sink,
            )
            .await;
            batch_deadline = None;
            pending_text_bytes = 0;
            match flushed {
                Ok(()) if matches!(event, RuntimeEvent::ToolCallOutput { .. }) => Ok(()),
                Ok(()) => match actor.event(event.clone(), streamed_content.clone()).await {
                    Ok(()) => {
                        let terminal_output = match &event {
                            RuntimeEvent::ToolCallFinished(result)
                            | RuntimeEvent::ToolCallValidationRejected { result, .. }
                            | RuntimeEvent::ToolCallConflict { result, .. } => {
                                Some(ConversationEvent::ToolCallOutput {
                                    call_id: result.call_id.clone(),
                                    chunk: crate::types::ToolOutputChunk(result.output.clone()),
                                })
                            }
                            _ => None,
                        };
                        if let Some(output) = terminal_output {
                            event_sink(&output)?;
                            events.push(output);
                        }
                        if let Some(converted) = from_runtime_event(message_id, &event) {
                            event_sink(&converted)?;
                            events.push(converted);
                        }
                        Ok(())
                    }
                    Err(error) => Err(error),
                },
                Err(error) => Err(error),
            }
        };

        match result {
            Ok(()) => {
                if let Some(ack) = ack {
                    let _ = ack.send(Ok(()));
                }
            }
            Err(error) => {
                if let Some(ack) = ack {
                    let _ = ack.send(Err(persistence_actor_error(error.to_string())));
                }
                cancel.cancel();
                return Err(error);
            }
        }
    }
}

pub(crate) async fn flush_text_batch<F>(
    actor: &PersistenceActor,
    streamed_content: &str,
    pending_text: &mut Vec<ConversationEvent>,
    events: &mut Vec<ConversationEvent>,
    event_sink: &mut F,
) -> Result<(), VegaError>
where
    F: FnMut(&ConversationEvent) -> Result<(), VegaError>,
{
    if pending_text.is_empty() {
        return Ok(());
    }
    actor.snapshot(streamed_content.to_string()).await?;
    for event in pending_text.drain(..) {
        event_sink(&event)?;
        events.push(event);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn persist_runtime_event(
    store: &Store,
    project_id: &str,
    thread_id: &str,
    message_id: &str,
    model: &str,
    is_plan: bool,
    streamed_content: &str,
    next_tool_seq: &mut i64,
    event: &RuntimeEvent,
) -> Result<(), VegaError> {
    match event {
        RuntimeEvent::ToolCallProposed(call) => {
            validate_runtime_proposal(call)?;
            if let Some(existing) = tool_calls::find_identity(store.conn(), &call.id)? {
                if existing.thread_id != thread_id
                    || existing.tool != call.name
                    || !tool_inputs_semantically_equal(
                        &call.name,
                        &existing.input_json,
                        &call.input_json,
                    )
                {
                    return Err(VegaError::Tool {
                        tool: call.name.clone(),
                        message: format!(
                            "call id '{}' collides with persisted owner/tool/input",
                            call.id
                        ),
                    });
                }
            } else {
                tool_calls::insert_pending(
                    store.conn(),
                    tool_calls::NewToolCall {
                        id: &call.id,
                        thread_id,
                        message_id,
                        seq: *next_tool_seq,
                        tool: &call.name,
                        input_json: &call.input_json,
                        status: "pending_approval",
                        created_at: now_ms(),
                    },
                )
                .map_err(tool_transition_error)?;
                *next_tool_seq += 1;
            }
        }
        RuntimeEvent::ToolCallValidationRejected { call, result } => {
            validate_runtime_validation_event(call, result)?;
            if result.reused {
                let state = required_tool_state(store, &call.id, thread_id)?;
                validate_reused_terminal(project_id, thread_id, &state, result)?;
            }
            if !result.reused {
                let approval = result.approval.as_ref().ok_or_else(|| VegaError::Tool {
                    tool: call.name.clone(),
                    message: "validation rejection missing approval audit".to_string(),
                })?;
                let approval_json = approval_audit_from_runtime(approval)
                    .to_json()
                    .map_err(|_| safe_audit_error(&call.name))?;
                tool_calls::insert_validation_rejected(
                    store.conn(),
                    tool_calls::ValidationRejectedToolCall {
                        call: tool_calls::NewToolCall {
                            id: &call.id,
                            thread_id,
                            message_id,
                            seq: *next_tool_seq,
                            tool: &call.name,
                            input_json: &call.input_json,
                            status: "rejected",
                            created_at: now_ms(),
                        },
                        approval_json: &approval_json,
                        output_text: &result.output,
                        finished_at: now_ms(),
                    },
                )
                .map_err(tool_transition_error)?;
                *next_tool_seq += 1;
            }
        }
        RuntimeEvent::ToolCallConflict { call, result } => {
            validate_runtime_conflict_event(call, result)?;
            let existing = tool_calls::find_identity(store.conn(), &call.id)?.ok_or_else(|| {
                VegaError::Tool {
                    tool: "persistence".to_string(),
                    message: "call id conflict has no persisted identity".to_string(),
                }
            })?;
            let is_same_identity = existing.thread_id == thread_id
                && existing.tool == call.name
                && tool_inputs_semantically_equal(
                    &call.name,
                    &existing.input_json,
                    &call.input_json,
                );
            if is_same_identity {
                return Err(safe_audit_error(&call.name));
            }
        }
        RuntimeEvent::ToolCallApproved {
            call_id,
            audit,
            remember_rule,
        } => {
            let state = required_tool_state(store, call_id, thread_id)?;
            validate_runtime_approval_event(&state, call_id, audit, remember_rule.as_ref())?;
            let approval_json = approval_audit_from_runtime(audit)
                .to_json()
                .map_err(|_| safe_audit_error("permission"))?;
            let remember = remember_rule
                .as_ref()
                .map(|target| tool_calls::RememberExactRule {
                    project_id,
                    tool: target.tool.as_str(),
                    pattern: &target.exact_pattern,
                });
            tool_calls::approve(store.conn(), call_id, &approval_json, remember, now_ms())
                .map_err(tool_transition_error)?;
        }
        RuntimeEvent::ToolCallRunning { call_id } => {
            let state = required_tool_state(store, call_id, thread_id)?;
            if state.status != "approved" {
                return Err(safe_audit_error(&state.tool));
            }
            tool_calls::mark_running(store.conn(), call_id).map_err(tool_transition_error)?;
        }
        RuntimeEvent::ToolCallFinished(result) if result.reused => {
            let state = required_tool_state(store, &result.call_id, thread_id)?;
            validate_reused_terminal(project_id, thread_id, &state, result)?;
        }
        RuntimeEvent::ToolCallFinished(result) if !result.reused => {
            let state = required_tool_state(store, &result.call_id, thread_id)?;
            if result.status == RuntimeToolStatus::Rejected {
                let approval = result.approval.as_ref().ok_or_else(|| VegaError::Tool {
                    tool: "permission".to_string(),
                    message: "rejection missing approval audit".to_string(),
                })?;
                let approval_json = approval_audit_from_runtime(approval)
                    .to_json()
                    .map_err(|_| safe_audit_error("permission"))?;
                validate_runtime_terminal(
                    project_id,
                    thread_id,
                    &result.call_id,
                    &state,
                    result,
                    &approval_audit_from_runtime(approval),
                )?;
                validate_rejected_remember(&state, result, &approval_audit_from_runtime(approval))?;
                tool_calls::reject(
                    store.conn(),
                    &result.call_id,
                    &approval_json,
                    &result.output,
                    now_ms(),
                    result
                        .remember_rule
                        .as_ref()
                        .map(|target| tool_calls::RememberExactRule {
                            project_id,
                            tool: target.tool.as_str(),
                            pattern: &target.exact_pattern,
                        }),
                )
                .map_err(tool_transition_error)?;
            } else {
                let approval_json = state
                    .approval
                    .as_deref()
                    .ok_or_else(|| safe_audit_error(&state.tool))?;
                let approval = ApprovalAudit::from_json(approval_json)
                    .map_err(|_| safe_audit_error(&state.tool))?;
                validate_runtime_terminal(
                    project_id,
                    thread_id,
                    &result.call_id,
                    &state,
                    result,
                    &approval,
                )?;
                let status = match result.status {
                    RuntimeToolStatus::Success => "success",
                    RuntimeToolStatus::Failed => "failed",
                    RuntimeToolStatus::Cancelled => "cancelled",
                    RuntimeToolStatus::Rejected => return Err(safe_audit_error("permission")),
                };
                tool_calls::finish(
                    store.conn(),
                    tool_calls::FinishToolCall {
                        id: &result.call_id,
                        status,
                        output_text: &result.output,
                        exit_code: result.exit_code,
                        duration_ms: result.duration_ms,
                        finished_at: now_ms(),
                    },
                )
                .map_err(tool_transition_error)?;
            }
        }
        RuntimeEvent::UsageUpdated {
            usage,
            cost_microcents,
            pricing,
        } => {
            // C5: priced rows carry exact provenance; legacy/unpriced rows
            // keep NULL columns so zero-cost stays distinguishable.
            let (pricing_version, pricing_profile, call_started_at) = match pricing {
                Some(pricing) => (
                    Some(pricing.version.as_str()),
                    Some(pricing.profile.as_str()),
                    Some(pricing.call_started_at),
                ),
                None => (None, None, None),
            };
            token_usage::insert(
                store.conn(),
                token_usage::NewTokenUsage {
                    thread_id,
                    message_id: Some(message_id),
                    model,
                    input_tokens: usage.input,
                    output_tokens: usage.output,
                    cache_read_tokens: usage.cache_read,
                    cache_write_tokens: usage.cache_write,
                    cost_microcents: *cost_microcents,
                    created_at: now_ms(),
                    pricing_version,
                    pricing_profile,
                    call_started_at,
                },
            )?;
        }
        RuntimeEvent::Finished(_) => {
            if is_plan {
                messages::complete_plan(
                    store.conn(),
                    thread_id,
                    message_id,
                    streamed_content,
                    now_ms(),
                )
                .map_err(|error| VegaError::Tool {
                    tool: "plan".to_string(),
                    message: error.to_string(),
                })?;
            } else {
                ensure_message_updated(
                    messages::finish_streaming(store.conn(), message_id, streamed_content, "done")?,
                    message_id,
                )?;
                vega_store::threads::open_thread(store.conn(), thread_id, now_ms())?;
            }
        }
        RuntimeEvent::Interrupted => {
            ensure_message_updated(
                messages::finish_streaming(
                    store.conn(),
                    message_id,
                    streamed_content,
                    "interrupted",
                )?,
                message_id,
            )?;
            vega_store::threads::open_thread(store.conn(), thread_id, now_ms())?;
        }
        RuntimeEvent::Error(_) => {
            ensure_message_updated(
                messages::finish_streaming(store.conn(), message_id, streamed_content, "failed")?,
                message_id,
            )?;
            vega_store::threads::open_thread(store.conn(), thread_id, now_ms())?;
        }
        RuntimeEvent::TextDelta(_)
        | RuntimeEvent::ThinkingDelta(_)
        | RuntimeEvent::ToolCallOutput { .. }
        | RuntimeEvent::ToolCallFinished(_) => {}
    }
    Ok(())
}

pub(crate) fn tool_transition_error(error: tool_calls::ToolCallTransitionError) -> VegaError {
    VegaError::Tool {
        tool: "persistence".to_string(),
        message: error.to_string(),
    }
}

pub(crate) fn safe_audit_error(tool: &str) -> VegaError {
    VegaError::Tool {
        tool: tool.to_string(),
        message: "strict approval audit failed".to_string(),
    }
}

pub(crate) fn required_tool_state(
    store: &Store,
    call_id: &str,
    thread_id: &str,
) -> Result<tool_calls::ToolCallState, VegaError> {
    let state = tool_calls::find_state(store.conn(), call_id)?.ok_or_else(|| VegaError::Tool {
        tool: "persistence".to_string(),
        message: "tool call state is missing".to_string(),
    })?;
    if state.thread_id != thread_id {
        return Err(VegaError::Tool {
            tool: "persistence".to_string(),
            message: "tool call ownership mismatch".to_string(),
        });
    }
    Ok(state)
}

pub(crate) fn validate_runtime_proposal(
    call: &vega_runtime::RuntimeToolCall,
) -> Result<(), VegaError> {
    if matches!(call.name.as_str(), "write" | "edit") {
        let audit = vega_tools::WriteEditAudit::from_json(&call.input_json)
            .map_err(|_| safe_audit_error(&call.name))?;
        if audit.tool().as_str() != call.name {
            return Err(safe_audit_error(&call.name));
        }
    } else if !matches!(call.name.as_str(), "read" | "glob" | "grep" | "bash")
        && call.input_json != "{}"
    {
        return Err(safe_audit_error(&call.name));
    }
    Ok(())
}

pub(crate) fn tool_inputs_semantically_equal(tool: &str, left: &str, right: &str) -> bool {
    if !matches!(tool, "write" | "edit") {
        return left == right;
    }
    if let (Ok(left), Ok(right)) = (
        vega_tools::WriteEditAudit::from_json(left),
        vega_tools::WriteEditAudit::from_json(right),
    ) {
        return left.tool().as_str() == tool && right.tool().as_str() == tool && left == right;
    }
    if let (Ok(left), Ok(right)) = (
        vega_tools::InvalidWriteEditAudit::from_json(left),
        vega_tools::InvalidWriteEditAudit::from_json(right),
    ) {
        return left.tool().as_str() == tool && right.tool().as_str() == tool && left == right;
    }
    false
}

pub(crate) fn validate_runtime_validation_event(
    call: &vega_runtime::RuntimeToolCall,
    result: &vega_runtime::RuntimeToolResult,
) -> Result<(), VegaError> {
    let invalid = vega_tools::InvalidWriteEditAudit::from_json(&call.input_json)
        .map_err(|_| safe_audit_error(&call.name))?;
    let approval = result
        .approval
        .as_ref()
        .ok_or_else(|| safe_audit_error(&call.name))?;
    let expected = format!(
        "Tool error: invalid {} input ({})",
        call.name,
        invalid.validation_error_code().as_str()
    );
    if invalid.tool().as_str() != call.name
        || result.call_id != call.id
        || result.status != RuntimeToolStatus::Rejected
        || result.output != expected
        || result.exit_code.is_some()
        || result.duration_ms.is_some()
        || result.truncated.is_some()
        || result.remember_rule.is_some()
        || approval.decision != vega_runtime::RuntimeApprovalDecision::Deny
        || approval.source != vega_runtime::RuntimeApprovalSource::Validation
    {
        return Err(safe_audit_error(&call.name));
    }
    Ok(())
}

pub(crate) fn validate_runtime_conflict_event(
    call: &vega_runtime::RuntimeToolCall,
    result: &vega_runtime::RuntimeToolResult,
) -> Result<(), VegaError> {
    if matches!(call.name.as_str(), "write" | "edit") {
        let valid = vega_tools::WriteEditAudit::from_json(&call.input_json)
            .ok()
            .is_some_and(|audit| audit.tool().as_str() == call.name);
        let invalid = vega_tools::InvalidWriteEditAudit::from_json(&call.input_json)
            .ok()
            .is_some_and(|audit| audit.tool().as_str() == call.name);
        if !valid && !invalid {
            return Err(safe_audit_error(&call.name));
        }
    } else {
        validate_runtime_proposal(call)?;
    }
    if result.call_id != call.id
        || result.output != vega_runtime::CALL_ID_CONFLICT_OUTPUT
        || result.status != RuntimeToolStatus::Failed
        || result.reused
        || result.exit_code.is_some()
        || result.duration_ms.is_some()
        || result.truncated.is_some()
        || result.approval.is_some()
        || result.remember_rule.is_some()
    {
        return Err(safe_audit_error(&call.name));
    }
    Ok(())
}

pub(crate) fn validate_runtime_approval_event(
    state: &tool_calls::ToolCallState,
    call_id: &str,
    audit: &vega_runtime::RuntimeApprovalAudit,
    remember: Option<&vega_runtime::RuntimePermissionTarget>,
) -> Result<(), VegaError> {
    if state.status != "pending_approval"
        || audit.decision == vega_runtime::RuntimeApprovalDecision::Deny
    {
        return Err(safe_audit_error(&state.tool));
    }
    let shared = approval_audit_from_runtime(audit);
    shared
        .to_json()
        .map_err(|_| safe_audit_error(&state.tool))?;
    if !approval_source_matches(
        &state.tool,
        RuntimeToolStatus::Success,
        shared.source,
        false,
    ) {
        return Err(safe_audit_error(&state.tool));
    }
    if state.tool == "bash" && !bash_danger_audit_matches(&state.input_json, &shared) {
        return Err(safe_audit_error(&state.tool));
    }
    let expects_remember = audit.decision == vega_runtime::RuntimeApprovalDecision::Always
        && matches!(
            audit.source,
            vega_runtime::RuntimeApprovalSource::User | vega_runtime::RuntimeApprovalSource::Danger
        );
    if remember.is_some() != expects_remember {
        return Err(safe_audit_error(&state.tool));
    }
    if let Some(target) = remember
        && !target_matches_state(state, call_id, target)
    {
        return Err(safe_audit_error(&state.tool));
    }
    Ok(())
}

pub(crate) fn validate_rejected_remember(
    state: &tool_calls::ToolCallState,
    result: &vega_runtime::RuntimeToolResult,
    approval: &ApprovalAudit,
) -> Result<(), VegaError> {
    let expects_remember = approval.source == ApprovalSource::ReadOnly
        && approval
            .danger
            .as_ref()
            .is_some_and(|danger| danger.decision == Approval::Always);
    if result.remember_rule.is_some() != expects_remember {
        return Err(safe_audit_error(&state.tool));
    }
    if let Some(target) = &result.remember_rule
        && !target_matches_state(state, &result.call_id, target)
    {
        return Err(safe_audit_error(&state.tool));
    }
    Ok(())
}

pub(crate) fn target_matches_state(
    state: &tool_calls::ToolCallState,
    call_id: &str,
    target: &vega_runtime::RuntimePermissionTarget,
) -> bool {
    let exact_matches_input = match state.tool.as_str() {
        "write" | "edit" => vega_tools::WriteEditAudit::from_json(&state.input_json)
            .ok()
            .is_some_and(|audit| {
                audit.tool().as_str() == state.tool && audit.path() == target.exact_pattern
            }),
        "bash" => vega_tools::bash_permission_signature(&state.input_json)
            .ok()
            .is_some_and(|command| command == target.exact_pattern),
        _ => false,
    };
    target.call_id == call_id
        && target.tool.as_str() == state.tool
        && !target.exact_pattern.is_empty()
        && target.exact_pattern == target.display_target
        && exact_matches_input
}

pub(crate) fn validate_runtime_terminal(
    project_id: &str,
    thread_id: &str,
    call_id: &str,
    state: &tool_calls::ToolCallState,
    result: &vega_runtime::RuntimeToolResult,
    approval: &ApprovalAudit,
) -> Result<(), VegaError> {
    let result_approval = result.approval.as_ref().map(approval_audit_from_runtime);
    if result_approval.as_ref() != Some(approval)
        || result.call_id != call_id
        || (result.status == RuntimeToolStatus::Rejected && state.status != "pending_approval")
        || (result.status != RuntimeToolStatus::Rejected && state.status != "running")
        || (result.status != RuntimeToolStatus::Rejected && result.remember_rule.is_some())
        || matches!(
            result.status,
            RuntimeToolStatus::Rejected | RuntimeToolStatus::Failed
        ) && result.truncated.is_some()
        || result.status == RuntimeToolStatus::Success && result.truncated.is_none()
        || result.status == RuntimeToolStatus::Cancelled
            && state.tool == "bash"
            && result.truncated.is_some()
    {
        return Err(safe_audit_error(&state.tool));
    }
    let canonical = validate_recovered_projection(
        project_id,
        thread_id,
        call_id,
        &state.tool,
        &state.input_json,
        &result.output,
        result.status,
        approval,
        result.exit_code,
        result.duration_ms,
    )
    .map_err(|_| safe_audit_error(&state.tool))?;
    if !tool_inputs_semantically_equal(&state.tool, &canonical, &state.input_json) {
        return Err(safe_audit_error(&state.tool));
    }
    Ok(())
}

pub(crate) fn validate_reused_terminal(
    project_id: &str,
    thread_id: &str,
    state: &tool_calls::ToolCallState,
    result: &vega_runtime::RuntimeToolResult,
) -> Result<(), VegaError> {
    let expected_status = match result.status {
        RuntimeToolStatus::Rejected => "rejected",
        RuntimeToolStatus::Success => "success",
        RuntimeToolStatus::Failed => "failed",
        RuntimeToolStatus::Cancelled => "cancelled",
    };
    let approval_json = state
        .approval
        .as_deref()
        .ok_or_else(|| safe_audit_error(&state.tool))?;
    let approval =
        ApprovalAudit::from_json(approval_json).map_err(|_| safe_audit_error(&state.tool))?;
    let runtime_approval = result
        .approval
        .as_ref()
        .map(approval_audit_from_runtime)
        .ok_or_else(|| safe_audit_error(&state.tool))?;
    let persisted_duration = state
        .duration_ms
        .map(u64::try_from)
        .transpose()
        .map_err(|_| safe_audit_error(&state.tool))?;
    if state.status != expected_status
        || runtime_approval != approval
        || result.remember_rule.is_some()
        || result.truncated.is_some()
        || state.output_full_path.is_some()
        || state.output_text.as_deref() != Some(result.output.as_str())
        || state.exit_code != result.exit_code
        || persisted_duration != result.duration_ms
    {
        return Err(safe_audit_error(&state.tool));
    }
    let canonical = validate_recovered_projection(
        project_id,
        thread_id,
        &result.call_id,
        &state.tool,
        &state.input_json,
        &result.output,
        result.status,
        &approval,
        result.exit_code,
        result.duration_ms,
    )
    .map_err(|_| safe_audit_error(&state.tool))?;
    if !tool_inputs_semantically_equal(&state.tool, &canonical, &state.input_json) {
        return Err(safe_audit_error(&state.tool));
    }
    Ok(())
}

pub(crate) fn ensure_message_updated(updated: usize, message_id: &str) -> Result<(), VegaError> {
    if updated == 0 {
        Err(VegaError::Tool {
            tool: "runtime".to_string(),
            message: format!("streaming message row disappeared or became terminal: {message_id}"),
        })
    } else {
        Ok(())
    }
}

pub(crate) fn runtime_store_error(error: impl Into<VegaError>) -> ConversationError {
    ConversationError::Runtime(Arc::new(error.into()))
}

pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as i64)
}
