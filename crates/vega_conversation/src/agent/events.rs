use super::*;

pub(super) struct RuntimeEventContext<'a, F> {
    pub(super) message_id: &'a str,
    pub(super) streamed_content: &'a mut String,
    pub(super) events: &'a mut Vec<ConversationEvent>,
    pub(super) event_sink: &'a mut F,
    pub(super) cancel: CancellationToken,
    pub(super) run_started_at: std::time::Instant,
    pub(super) diagnostics: &'a super::diagnostics::DiagnosticsWriter,
    pub(super) thread_id: &'a str,
    pub(super) run_id: &'a str,
    pub(super) root_attempt_id: &'a str,
    pub(super) subagent_receipts: Option<super::subagents::ReceiptObserver>,
}

pub(super) async fn process_runtime_events<F>(
    mut receiver: mpsc::Receiver<RuntimeEnvelope>,
    actor: &PersistenceActor,
    context: RuntimeEventContext<'_, F>,
) -> Result<(), VegaError>
where
    F: FnMut(&ConversationEvent) -> Result<(), VegaError>,
{
    let RuntimeEventContext {
        message_id,
        streamed_content,
        events,
        event_sink,
        cancel,
        run_started_at,
        diagnostics,
        thread_id,
        run_id,
        root_attempt_id,
        subagent_receipts,
    } = context;
    let mut message_id = message_id.to_owned();
    let mut pending_text = Vec::new();
    let mut pending_text_bytes = 0usize;
    let mut batch_deadline = None;
    let mut response_start = None;
    let mut reasoning_event_start = 0usize;
    let mut tool_attempts =
        std::collections::HashMap::<String, (String, std::time::Instant)>::new();

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
                        let error = error
                            .at_runtime_stage(vega_runtime::RuntimeFailureStage::EventPipeline);
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
                let error =
                    error.at_runtime_stage(vega_runtime::RuntimeFailureStage::EventPipeline);
                cancel.cancel();
                return Err(error);
            }
            return Ok(());
        };

        let RuntimeEnvelope { event, ack } = envelope;
        let invalid_boundary = match &event {
            RuntimeEvent::AssistantResponseStarted {
                text_start_bytes, ..
            } => {
                if *text_start_bytes == streamed_content.len() {
                    response_start = Some(*text_start_bytes);
                    false
                } else {
                    true
                }
            }
            RuntimeEvent::ResponseRetrying {
                text_start_bytes, ..
            } => {
                response_start != Some(*text_start_bytes)
                    || !streamed_content.is_char_boundary(*text_start_bytes)
            }
            _ => false,
        };
        let result = if invalid_boundary {
            Err(persistence_actor_error("invalid response retry boundary"))
        } else if let RuntimeEvent::DiagnosticAttempt(attempt) = &event {
            diagnostics.runtime_attempt(thread_id, run_id, root_attempt_id, attempt);
            Ok(())
        } else if let RuntimeEvent::TextDelta(delta) = &event {
            streamed_content.push_str(delta);
            pending_text_bytes = pending_text_bytes.saturating_add(delta.len());
            if let Some(converted) = from_runtime_event(&message_id, &event) {
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
                Ok(()) => {
                    if let RuntimeEvent::ResponseRetrying {
                        text_start_bytes, ..
                    } = &event
                    {
                        streamed_content.truncate(*text_start_bytes);
                    }
                    let execution_duration_ms = terminal_execution_duration(&event, run_started_at);
                    match actor
                        .event(
                            event.clone(),
                            streamed_content.clone(),
                            execution_duration_ms
                                .map(|duration| i64::try_from(duration).unwrap_or(i64::MAX)),
                        )
                        .await
                    {
                        Ok(()) => (|| {
                            if matches!(event, RuntimeEvent::AssistantResponseStarted { .. }) {
                                events.retain(|event| {
                                    !matches!(
                                        event,
                                        ConversationEvent::ThinkingDelta { .. }
                                            | ConversationEvent::SummaryDelta { .. }
                                    )
                                });
                                reasoning_event_start = events.len();
                            }
                            let release_start = match &event {
                                RuntimeEvent::ReasoningRetentionReleased => Some(0),
                                RuntimeEvent::ResponseRetrying { .. } => {
                                    Some(reasoning_event_start)
                                }
                                _ => None,
                            };
                            if let Some(start) = release_start {
                                let mut index = 0usize;
                                events.retain(|event| {
                                    let keep = index < start
                                        || !matches!(
                                            event,
                                            ConversationEvent::ThinkingDelta { .. }
                                                | ConversationEvent::SummaryDelta { .. }
                                        );
                                    index += 1;
                                    keep
                                });
                            }
                            if let RuntimeEvent::SteeringAccepted(input) = &event {
                                let transitioned = [
                                    ConversationEvent::MessageFinished {
                                        message_id: message_id.clone(),
                                        stop_reason: crate::types::ConversationStopReason::End,
                                        execution_duration_ms: None,
                                    },
                                    ConversationEvent::PendingMessageDelivered {
                                        pending_id: input.id.clone(),
                                        message_id: input.id.clone(),
                                        seq: input.user_message_seq as u64,
                                        content: input.original_content.clone(),
                                        images: input.message.images.clone(),
                                    },
                                    ConversationEvent::MessageStarted {
                                        message_id: input.assistant_message_id.clone(),
                                        seq: input.user_message_seq.saturating_add(1) as u64,
                                    },
                                    ConversationEvent::RunStarted {
                                        message_id: input.assistant_message_id.clone(),
                                        started_at: run_started_at,
                                    },
                                ];
                                message_id = input.assistant_message_id.clone();
                                streamed_content.clear();
                                response_start = None;
                                for converted in transitioned {
                                    event_sink(&converted)?;
                                    events.push(converted);
                                }
                            }
                            record_tool_lifecycle(
                                diagnostics,
                                thread_id,
                                run_id,
                                root_attempt_id,
                                &event,
                                &mut tool_attempts,
                            );
                            let terminal_output = match &event {
                                RuntimeEvent::ToolCallFinished(result)
                                | RuntimeEvent::ToolReadFinished { result, .. }
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
                            if let Some(converted) = from_runtime_event(&message_id, &event) {
                                let converted = with_terminal_execution_duration(
                                    converted,
                                    execution_duration_ms,
                                );
                                event_sink(&converted)?;
                                events.push(converted);
                            }
                            Ok(())
                        })(),
                        Err(error) => {
                            record_tool_persistence_failure(
                                diagnostics,
                                thread_id,
                                run_id,
                                root_attempt_id,
                                &event,
                                &error,
                                &mut tool_attempts,
                            );
                            Err(error)
                        }
                    }
                }
                Err(error) => {
                    let error =
                        error.at_runtime_stage(vega_runtime::RuntimeFailureStage::EventPipeline);
                    record_tool_persistence_failure(
                        diagnostics,
                        thread_id,
                        run_id,
                        root_attempt_id,
                        &event,
                        &error,
                        &mut tool_attempts,
                    );
                    Err(error)
                }
            }
        };

        match result {
            Ok(()) => {
                if let Some(observer) = &subagent_receipts {
                    observer.persisted(&event);
                }
                if let Some(ack) = ack {
                    let _ = ack.send(Ok(()));
                }
            }
            Err(error) => {
                let error =
                    error.at_runtime_stage(vega_runtime::RuntimeFailureStage::EventPipeline);
                if let Some(ack) = ack {
                    let acknowledgement_error = error.acknowledgement_error();
                    let _ = ack.send(Err(acknowledgement_error));
                }
                cancel.cancel();
                return Err(error);
            }
        }
    }
}

fn record_tool_lifecycle(
    diagnostics: &super::diagnostics::DiagnosticsWriter,
    thread_id: &str,
    run_id: &str,
    root_attempt_id: &str,
    event: &RuntimeEvent,
    attempts: &mut std::collections::HashMap<String, (String, std::time::Instant)>,
) {
    match event {
        RuntimeEvent::ToolCallProposed(call) => {
            let attempt_id = ulid::Ulid::generate().to_string();
            let started_at = std::time::Instant::now();
            attempts.insert(call.id.clone(), (attempt_id.clone(), started_at));
            diagnostics.tool_event(super::diagnostics::ToolDiagnosticEvent {
                thread_id,
                run_id,
                root_attempt_id,
                attempt_id: &attempt_id,
                tool_call_id: &call.id,
                state: vega_store::run_diagnostics::DiagnosticState::Started,
                failure_code: None,
                failure: None,
                started_at,
                output_bytes: None,
                truncated: None,
                duration_ms: None,
            });
        }
        RuntimeEvent::ToolCallValidationRejected { result, .. }
        | RuntimeEvent::ToolCallConflict { result, .. }
        | RuntimeEvent::ToolCallFinished(result)
        | RuntimeEvent::ToolReadFinished { result, .. } => {
            let (attempt_id, started_at) = if let Some(attempt) = attempts.remove(&result.call_id) {
                attempt
            } else {
                let attempt_id = ulid::Ulid::generate().to_string();
                let started_at = std::time::Instant::now();
                diagnostics.tool_event(super::diagnostics::ToolDiagnosticEvent {
                    thread_id,
                    run_id,
                    root_attempt_id,
                    attempt_id: &attempt_id,
                    tool_call_id: &result.call_id,
                    state: vega_store::run_diagnostics::DiagnosticState::Started,
                    failure_code: None,
                    failure: None,
                    started_at,
                    output_bytes: None,
                    truncated: None,
                    duration_ms: None,
                });
                (attempt_id, started_at)
            };
            let (state, failure) = match result.status {
                vega_runtime::RuntimeToolStatus::Success => (
                    vega_store::run_diagnostics::DiagnosticState::Succeeded,
                    None,
                ),
                vega_runtime::RuntimeToolStatus::Rejected => (
                    vega_store::run_diagnostics::DiagnosticState::Failed,
                    Some(vega_store::run_diagnostics::DiagnosticFailureCode::ToolRejected),
                ),
                vega_runtime::RuntimeToolStatus::Failed => (
                    vega_store::run_diagnostics::DiagnosticState::Failed,
                    Some(vega_store::run_diagnostics::DiagnosticFailureCode::ToolFailed),
                ),
                vega_runtime::RuntimeToolStatus::Cancelled => (
                    vega_store::run_diagnostics::DiagnosticState::Cancelled,
                    None,
                ),
            };
            diagnostics.tool_event(super::diagnostics::ToolDiagnosticEvent {
                thread_id,
                run_id,
                root_attempt_id,
                attempt_id: &attempt_id,
                tool_call_id: &result.call_id,
                state,
                failure_code: failure,
                failure: None,
                started_at,
                output_bytes: Some(result.output.len() as u64),
                truncated: result.truncated,
                duration_ms: result.duration_ms,
            });
        }
        _ => {}
    }
}

fn record_tool_persistence_failure(
    diagnostics: &super::diagnostics::DiagnosticsWriter,
    thread_id: &str,
    run_id: &str,
    root_attempt_id: &str,
    event: &RuntimeEvent,
    error: &VegaError,
    attempts: &mut std::collections::HashMap<String, (String, std::time::Instant)>,
) {
    let call_id = match event {
        RuntimeEvent::ToolCallProposed(call) => &call.id,
        RuntimeEvent::ToolCallApproved { call_id, .. }
        | RuntimeEvent::ToolCallRunning { call_id } => call_id,
        RuntimeEvent::ToolCallFinished(result)
        | RuntimeEvent::ToolReadFinished { result, .. }
        | RuntimeEvent::ToolCallValidationRejected { result, .. }
        | RuntimeEvent::ToolCallConflict { result, .. } => &result.call_id,
        _ => return,
    };
    let (attempt_id, started_at) = attempts.remove(call_id).unwrap_or_else(|| {
        let attempt_id = ulid::Ulid::generate().to_string();
        let started_at = std::time::Instant::now();
        diagnostics.tool_event(super::diagnostics::ToolDiagnosticEvent {
            thread_id,
            run_id,
            root_attempt_id,
            attempt_id: &attempt_id,
            tool_call_id: call_id,
            state: vega_store::run_diagnostics::DiagnosticState::Started,
            failure_code: None,
            failure: None,
            started_at,
            output_bytes: None,
            truncated: None,
            duration_ms: None,
        });
        (attempt_id, started_at)
    });
    diagnostics.tool_event(super::diagnostics::ToolDiagnosticEvent {
        thread_id,
        run_id,
        root_attempt_id,
        attempt_id: &attempt_id,
        tool_call_id: call_id,
        state: vega_store::run_diagnostics::DiagnosticState::Failed,
        failure_code: Some(super::diagnostics::run_failure(error)),
        failure: Some(error),
        started_at,
        output_bytes: None,
        truncated: None,
        duration_ms: None,
    });
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

fn terminal_execution_duration(
    event: &RuntimeEvent,
    run_started_at: std::time::Instant,
) -> Option<u64> {
    matches!(
        event,
        RuntimeEvent::Finished(_)
            | RuntimeEvent::StageHandoff { .. }
            | RuntimeEvent::Interrupted
            | RuntimeEvent::Error(_)
    )
    .then(|| u64::try_from(run_started_at.elapsed().as_millis()).unwrap_or(u64::MAX))
}

fn with_terminal_execution_duration(
    event: ConversationEvent,
    execution_duration_ms: Option<u64>,
) -> ConversationEvent {
    match event {
        ConversationEvent::MessageFinished {
            message_id,
            stop_reason,
            ..
        } => ConversationEvent::MessageFinished {
            message_id,
            stop_reason,
            execution_duration_ms,
        },
        ConversationEvent::Interrupted { message_id, .. } => ConversationEvent::Interrupted {
            message_id,
            execution_duration_ms,
        },
        ConversationEvent::Error {
            message_id, error, ..
        } => ConversationEvent::Error {
            message_id,
            error,
            execution_duration_ms,
        },
        other => other,
    }
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
    execution_duration_ms: Option<i64>,
) -> Result<(), VegaError> {
    match event {
        RuntimeEvent::ResponseRetrying { .. } => {
            vega_store::messages::update_streaming_content(
                store.conn(),
                message_id,
                streamed_content,
            )
            .map_err(VegaError::from)?;
        }
        RuntimeEvent::AssistantResponseStarted { .. } => {}
        RuntimeEvent::ReasoningCompleted { .. } => {}
        RuntimeEvent::SteeringAccepted(input) => {
            let tx = store.immediate_transaction()?;
            if !vega_store::pending_messages::mark_delivered(&tx, thread_id, &input.id)? {
                return Err(runtime_state_error());
            }
            ensure_message_updated(
                messages::finish_streaming_in_transaction(
                    &tx,
                    message_id,
                    streamed_content,
                    "done",
                    execution_duration_ms,
                )?,
                message_id,
            )?;
            vega_store::pending_messages::preserve_model_input(
                &tx,
                thread_id,
                &input.id,
                &input.message.content,
            )?;
            let seq = messages::next_seq(&tx, thread_id)?;
            if seq != input.user_message_seq {
                return Err(runtime_state_error());
            }
            for (id, role, content, status, seq) in [
                (
                    &input.id,
                    "user",
                    input.original_content.as_str(),
                    "done",
                    seq,
                ),
                (
                    &input.assistant_message_id,
                    "assistant",
                    "",
                    "streaming",
                    seq.saturating_add(1),
                ),
            ] {
                messages::insert(
                    &tx,
                    &messages::MessageRow {
                        id: id.clone(),
                        thread_id: thread_id.into(),
                        seq,
                        role: role.into(),
                        kind: "text".into(),
                        content: content.into(),
                        status: status.into(),
                        created_at: now_ms(),
                        plan_status: None,
                        plan_review_note: None,
                        plan_reviewed_at: None,
                    },
                )?;
            }
            vega_store::delegated_stages::inherit_execution(
                &tx,
                thread_id,
                message_id,
                &input.assistant_message_id,
            )?;
            for (ordinal, image) in input.message.images.iter().enumerate() {
                vega_store::image_attachments::insert(&tx, &input.id, ordinal, image.bytes())?;
            }
            tx.commit()?;
        }
        RuntimeEvent::BuiltinSkillActivated(activation) => {
            vega_store::computer_use_delivery::activate_builtin(
                store.conn(),
                thread_id,
                message_id,
                &activation.name,
                activation.version,
                &activation.content_sha256,
            )
            .map_err(|error| storage_or_audit_error(error, "builtin_skill"))?;
        }
        RuntimeEvent::ScreenshotDelivered {
            call_id,
            image,
            control,
            epoch,
            cancel,
        } => {
            crate::attachments::validate_images(std::slice::from_ref(image))
                .map_err(|_| safe_audit_error("ComputerScreenshot"))?;
            control
                .commit_delivery(*epoch, || {
                    if cancel.is_cancelled() {
                        return Err(safe_audit_error("ComputerScreenshot"));
                    }
                    vega_store::computer_use_delivery::insert_screenshot(
                        store.conn(),
                        thread_id,
                        message_id,
                        call_id,
                        image.bytes(),
                    )
                    .map_err(|error| storage_or_audit_error(error, "ComputerScreenshot"))
                })
                .map_err(|_| safe_audit_error("ComputerScreenshot"))??;
        }
        RuntimeEvent::ToolRoundCompleted {
            response_id,
            call_ids,
        } => {
            let transaction = store.conn().unchecked_transaction()?;
            for call_id in call_ids {
                transaction.execute(
                    "INSERT OR IGNORE INTO tool_call_rounds (message_id, call_id, response_id) VALUES (?1, ?2, ?3)",
                    (message_id, &call_id, &response_id),
                )?;
            }
            transaction.commit()?;
        }
        RuntimeEvent::AssistantResponseCompleted {
            response_id,
            text_start_bytes,
            text_end_bytes,
            kind,
        } => {
            messages::record_assistant_response(
                store.conn(),
                message_id,
                streamed_content,
                &messages::AssistantResponseRow {
                    message_id: message_id.to_string(),
                    response_id: response_id.clone(),
                    text_start_bytes: *text_start_bytes,
                    text_end_bytes: *text_end_bytes,
                    kind: match kind {
                        vega_runtime::RuntimeAssistantResponseKind::Commentary => "commentary",
                        vega_runtime::RuntimeAssistantResponseKind::Final => "final",
                    }
                    .to_string(),
                },
            )?;
        }
        RuntimeEvent::SkillSnapshot { binding, snapshot } => {
            persist_skill_snapshot(store, thread_id, message_id, binding, snapshot)?;
        }
        RuntimeEvent::SkillActivation {
            audit,
            binding,
            snapshot,
        } => {
            if let Some(snapshot) = snapshot {
                let binding = binding.as_ref().ok_or_else(|| safe_audit_error("skill"))?;
                persist_skill_snapshot(store, thread_id, message_id, binding, snapshot)?;
            }
            let scope = audit.source_scope.map(|scope| match scope {
                vega_runtime::skills::SourceScope::Project => "project",
                vega_runtime::skills::SourceScope::VegaGlobal => "vega_global",
                vega_runtime::skills::SourceScope::Imported => "imported",
            });
            let origin = match audit.origin {
                vega_runtime::skills::ActivationOrigin::ExplicitUser => "explicit_user",
                vega_runtime::skills::ActivationOrigin::Model => "model",
            };
            vega_store::skills::append_activation_audit(
                store.conn(),
                vega_store::skills::NewSkillActivationAudit {
                    run_id: message_id,
                    thread_id,
                    name: &audit.name,
                    source_scope: scope,
                    content_sha256: audit.content_sha256.as_deref(),
                    origin,
                    status: audit.status,
                    created_at: now_ms(),
                },
            )
            .map_err(skill_store_error)?;
        }
        RuntimeEvent::ToolCallProposed(call) => {
            validate_runtime_proposal(call)?;
            if let Some(existing) = tool_calls::find_identity(store.conn(), &call.id)? {
                if existing.thread_id != thread_id
                    || !vega_runtime::tool_names_equivalent(&existing.tool, &call.name)
                    || !tool_inputs_semantically_equal(
                        &call.name,
                        &existing.input_json,
                        &call.input_json,
                    )
                {
                    return Err(runtime_state_error());
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
                let approval = result
                    .approval
                    .as_ref()
                    .ok_or_else(|| safe_audit_error(&call.name))?;
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
            let existing = tool_calls::find_identity(store.conn(), &call.id)?
                .ok_or_else(runtime_state_error)?;
            let is_same_identity = existing.thread_id == thread_id
                && vega_runtime::tool_names_equivalent(&existing.tool, &call.name)
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
            let remember = (!project_id.is_empty())
                .then(|| {
                    remember_rule
                        .as_ref()
                        .map(|target| tool_calls::RememberExactRule {
                            project_id,
                            tool: target.tool.as_str(),
                            pattern: &target.exact_pattern,
                        })
                })
                .flatten();
            tool_calls::approve(store.conn(), call_id, &approval_json, remember, now_ms())
                .map_err(tool_transition_error)?;
        }
        RuntimeEvent::ToolCallRunning { call_id } => {
            let state = required_tool_state(store, call_id, thread_id)?;
            if state.status != "approved" {
                return Err(runtime_state_error());
            }
            tool_calls::mark_running(store.conn(), call_id).map_err(tool_transition_error)?;
        }
        RuntimeEvent::ToolCallFinished(result) | RuntimeEvent::ToolReadFinished { result, .. }
            if result.reused =>
        {
            let state = required_tool_state(store, &result.call_id, thread_id)?;
            validate_reused_terminal(project_id, thread_id, &state, result)?;
        }
        RuntimeEvent::ToolCallFinished(result) | RuntimeEvent::ToolReadFinished { result, .. }
            if !result.reused =>
        {
            let state = required_tool_state(store, &result.call_id, thread_id)?;
            if result.status == RuntimeToolStatus::Rejected {
                let approval = result
                    .approval
                    .as_ref()
                    .ok_or_else(|| safe_audit_error("permission"))?;
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
                        .filter(|_| !project_id.is_empty())
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
                let tx = store.immediate_transaction()?;
                tool_calls::finish(
                    &tx,
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
                if let RuntimeEvent::ToolReadFinished { identity, .. } = event {
                    if result.status != RuntimeToolStatus::Success
                        || result.truncated != Some(false)
                        || !identity.complete
                    {
                        return Err(runtime_state_error());
                    }
                    vega_store::delegated_stages::record_read(
                        &tx,
                        thread_id,
                        message_id,
                        &result.call_id,
                        &identity.canonical_path,
                        &identity.sha256,
                    )?;
                }
                tx.commit()?;
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
                    cache_read_tokens: usage.cache_read.unwrap_or(0),
                    cache_write_tokens: usage.cache_write.unwrap_or(0),
                    cost_microcents: *cost_microcents,
                    created_at: now_ms(),
                    pricing_version,
                    pricing_profile,
                    call_started_at,
                },
            )?;
        }
        RuntimeEvent::ContextCompactionUsageUpdated { usage } => {
            // A summary request is not an assistant prose turn.  Persist its
            // actual token event as thread-level usage with no fabricated
            // message id; unpriced rows retain the established NULL pricing
            // provenance and zero-cost placeholder semantics.
            token_usage::insert(
                store.conn(),
                token_usage::NewTokenUsage {
                    thread_id,
                    message_id: None,
                    model,
                    input_tokens: usage.usage.input,
                    output_tokens: usage.usage.output,
                    cache_read_tokens: usage.usage.cache_read.unwrap_or(0),
                    cache_write_tokens: usage.usage.cache_write.unwrap_or(0),
                    cost_microcents: usage.cost_microcents,
                    created_at: now_ms(),
                    pricing_version: usage
                        .pricing
                        .as_ref()
                        .map(|pricing| pricing.version.as_str()),
                    pricing_profile: usage
                        .pricing
                        .as_ref()
                        .map(|pricing| pricing.profile.as_str()),
                    call_started_at: usage
                        .pricing
                        .as_ref()
                        .map(|pricing| pricing.call_started_at),
                },
            )?;
        }
        RuntimeEvent::ContextCompactionStatusUpdated { .. } => {
            let RuntimeEvent::ContextCompactionStatusUpdated { status } = event else {
                unreachable!()
            };
            let phase = match status.phase {
                vega_runtime::ContextCompactionPhase::Started => "started",
                vega_runtime::ContextCompactionPhase::Succeeded => "succeeded",
                vega_runtime::ContextCompactionPhase::Failed => "failed",
                vega_runtime::ContextCompactionPhase::Cancelled => "cancelled",
            };
            let usage_state = match status.usage {
                vega_runtime::ContextCompactionUsageState::Pending => "pending",
                vega_runtime::ContextCompactionUsageState::Known { priced: true } => "known_priced",
                vega_runtime::ContextCompactionUsageState::Known { priced: false } => {
                    "known_unpriced"
                }
                vega_runtime::ContextCompactionUsageState::Unknown => "unknown",
            };
            let failure = status.failure.map(|failure| match failure {
                vega_runtime::ContextCompactionStatusFailure::Cancelled => "cancelled",
                vega_runtime::ContextCompactionStatusFailure::SourceChanged => "source_changed",
                vega_runtime::ContextCompactionStatusFailure::NoCompactablePrefix => {
                    "no_compactable_prefix"
                }
                vega_runtime::ContextCompactionStatusFailure::TooLarge => "too_large",
                vega_runtime::ContextCompactionStatusFailure::ReasoningLimit => "too_large",
                vega_runtime::ContextCompactionStatusFailure::InvalidSummary => "invalid_summary",
                vega_runtime::ContextCompactionStatusFailure::TimedOut => "summary_timeout",
                vega_runtime::ContextCompactionStatusFailure::ImagesUnsupported => {
                    "images_unsupported"
                }
                vega_runtime::ContextCompactionStatusFailure::OverLimit => "over_limit",
                vega_runtime::ContextCompactionStatusFailure::Unavailable => "unavailable",
            });
            vega_store::context_compaction::insert_status(
                store.conn(),
                vega_store::context_compaction::NewContextCompactionStatus {
                    thread_id,
                    model,
                    operation_key: &status.operation_key,
                    generation: status.generation,
                    phase,
                    usage_state,
                    failure,
                    source_version: status.source_version,
                    estimated_tokens: status.estimated_tokens,
                    input_budget: status.input_budget,
                    target_tokens: status.target_tokens,
                    created_at: now_ms(),
                },
            )?;
        }
        RuntimeEvent::StageHandoff { checkpoint_id } => {
            let tx = store.immediate_transaction()?;
            vega_store::delegated_stages::mark_handoff(&tx, thread_id, message_id, checkpoint_id)?;
            ensure_message_updated(
                messages::finish_streaming_in_transaction(
                    &tx,
                    message_id,
                    streamed_content,
                    "done",
                    execution_duration_ms,
                )?,
                message_id,
            )?;
            vega_store::threads::open_thread_in_transaction(&tx, thread_id, now_ms())?;
            tx.commit()?;
        }
        RuntimeEvent::Finished(_) => {
            if is_plan {
                messages::complete_plan(
                    store.conn(),
                    thread_id,
                    message_id,
                    streamed_content,
                    now_ms(),
                    execution_duration_ms,
                )
                .map_err(|error| match error {
                    messages::PlanTransitionError::Store(error) => VegaError::Store(error),
                    messages::PlanTransitionError::CorruptState => runtime_state_error(),
                })?;
            } else {
                ensure_message_updated(
                    messages::finish_streaming(
                        store.conn(),
                        message_id,
                        streamed_content,
                        "done",
                        execution_duration_ms,
                    )?,
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
                    execution_duration_ms,
                )?,
                message_id,
            )?;
            vega_store::threads::open_thread(store.conn(), thread_id, now_ms())?;
        }
        RuntimeEvent::Error(error) => {
            let transaction = store.immediate_transaction()?;
            ensure_message_updated(
                messages::finish_streaming_in_transaction(
                    &transaction,
                    message_id,
                    streamed_content,
                    "failed",
                    execution_duration_ms,
                )?,
                message_id,
            )?;
            let reason = match error.as_ref() {
                VegaError::CredentialExposureBlocked => Some("exposure".to_owned()),
                VegaError::CredentialCheckUnavailable => Some("unavailable".to_owned()),
                VegaError::CredentialProjectionInvalid { tool_index } => {
                    Some(format!("projection:{tool_index}"))
                }
                _ => None,
            };
            if let Some(reason) = reason {
                vega_store::credential_continuation::suspend(
                    &transaction,
                    thread_id,
                    message_id,
                    &reason,
                    now_ms(),
                )?;
            }
            vega_store::credential_continuation::complete(&transaction, message_id)?;
            vega_store::threads::open_thread_in_transaction(&transaction, thread_id, now_ms())?;
            transaction.commit()?;
        }
        RuntimeEvent::TextDelta(_)
        | RuntimeEvent::ThinkingDelta(_)
        | RuntimeEvent::SummaryDelta(_)
        | RuntimeEvent::ReasoningRetentionReleased
        | RuntimeEvent::DiagnosticAttempt(_)
        | RuntimeEvent::ContextAccountingUpdated(_)
        | RuntimeEvent::ToolCallOutput { .. }
        | RuntimeEvent::ToolCallFinished(_)
        | RuntimeEvent::ToolReadFinished { .. } => {}
    }
    Ok(())
}

fn persist_skill_snapshot(
    store: &Store,
    thread_id: &str,
    message_id: &str,
    binding: &vega_runtime::skills::RunBinding,
    snapshot: &vega_runtime::skills::SkillRunSnapshot,
) -> Result<(), VegaError> {
    if binding.run_id() != message_id || binding.thread_id() != thread_id {
        return Err(safe_audit_error("skill"));
    }
    vega_store::skills::save_snapshot(
        store.conn(),
        vega_store::skills::NewSkillSnapshot {
            run_id: binding.run_id(),
            thread_id: binding.thread_id(),
            consent_generation: binding.consent_generation(),
            revocation_generation: binding.revocation_generation(),
            catalog_sha256: binding.catalog_sha256(),
            snapshot_sha256: snapshot.sha256(),
            bytes: snapshot.bytes(),
            updated_at: now_ms(),
        },
    )
    .map_err(skill_store_error)
}

fn skill_store_error(error: vega_store::skills::SkillStoreError) -> VegaError {
    match error {
        vega_store::skills::SkillStoreError::Sql(error) => VegaError::Store(error),
        _ => safe_audit_error("skill"),
    }
}

fn storage_or_audit_error(error: impl Into<VegaError>, tool: &str) -> VegaError {
    match error.into() {
        VegaError::Store(error) if error.sqlite_error().is_some() => VegaError::Store(error),
        _ => safe_audit_error(tool),
    }
}

pub(crate) fn tool_transition_error(error: tool_calls::ToolCallTransitionError) -> VegaError {
    match error {
        tool_calls::ToolCallTransitionError::Store(error)
        | tool_calls::ToolCallTransitionError::Permission(
            vega_store::permissions::PermissionsError::Store(error),
        ) => VegaError::Store(error),
        _ => runtime_state_error(),
    }
}

pub(crate) fn safe_audit_error(_tool: &str) -> VegaError {
    VegaError::RuntimeFailure(vega_runtime::RuntimeFailureDetail::new(
        vega_runtime::RuntimeFailureReason::ApprovalAudit,
    ))
}

fn runtime_state_error() -> VegaError {
    VegaError::RuntimeFailure(vega_runtime::RuntimeFailureDetail::new(
        vega_runtime::RuntimeFailureReason::State,
    ))
}

pub(crate) fn required_tool_state(
    store: &Store,
    call_id: &str,
    thread_id: &str,
) -> Result<tool_calls::ToolCallState, VegaError> {
    let state = tool_calls::find_state(store.conn(), call_id)?.ok_or_else(runtime_state_error)?;
    if state.thread_id != thread_id {
        return Err(runtime_state_error());
    }
    Ok(state)
}

pub(crate) fn validate_runtime_proposal(
    call: &vega_runtime::RuntimeToolCall,
) -> Result<(), VegaError> {
    if vega_runtime::is_subagent_tool(&call.name) {
        return (call.input_json == "{}"
            || vega_runtime::valid_subagent_input(&call.name, &call.input_json))
        .then_some(())
        .ok_or_else(|| safe_audit_error(&call.name));
    }
    if matches!(
        call.name.as_str(),
        "get_goal" | "update_goal" | "schedule_wakeup"
    ) {
        return valid_automation_call_projection(&call.name, &call.input_json)
            .then_some(())
            .ok_or_else(|| safe_audit_error(&call.name));
    }
    if vega_runtime::computer_use::ComputerUseRun::tool_capability(&call.name).is_some() {
        return vega_runtime::computer_use::ComputerUseAudit::valid(&call.input_json)
            .then_some(())
            .ok_or_else(|| safe_audit_error(&call.name));
    }
    if call.name == vega_runtime::USER_QUESTION_TOOL {
        let valid = vega_runtime::UserQuestionInput::from_json(&call.input_json).is_ok()
            || vega_runtime::user_question_validation_error(&call.input_json).is_some();
        return valid
            .then_some(())
            .ok_or_else(|| safe_audit_error(&call.name));
    }
    if call.name == "grep" && crate::types::grep_invalid_input_claim(&call.input_json) {
        return crate::types::valid_grep_invalid_input(&call.input_json)
            .then_some(())
            .ok_or_else(|| safe_audit_error(&call.name));
    }
    if matches!(call.name.as_str(), "load_skill" | "read_skill_resource") {
        return super::pipeline::valid_skill_call_projection(&call.name, &call.input_json)
            .then_some(())
            .ok_or_else(|| safe_audit_error(&call.name));
    }
    if call.name.starts_with("mcp_") {
        let projected = crate::types::ToolCall {
            id: call.id.clone(),
            tool: call.name.clone(),
            input_json: call.input_json.clone(),
        };
        return crate::types::McpCallIdentity::from_tool_call(&projected)
            .map(|_| ())
            .ok_or_else(|| safe_audit_error(&call.name));
    }
    if matches!(call.name.as_str(), "Write" | "Edit" | "write" | "edit") {
        let audit = vega_tools::WriteEditAudit::from_json(&call.input_json)
            .map_err(|_| safe_audit_error(&call.name))?;
        if !audit.tool().as_str().eq_ignore_ascii_case(&call.name) {
            return Err(safe_audit_error(&call.name));
        }
    } else if !matches!(
        call.name.as_str(),
        "Read" | "read" | "glob" | "grep" | "bash"
    ) && call.input_json != "{}"
    {
        return Err(safe_audit_error(&call.name));
    }
    Ok(())
}

pub(crate) fn tool_inputs_semantically_equal(tool: &str, left: &str, right: &str) -> bool {
    if tool == "bash"
        && let (Some(left), Some(right)) = (
            vega_runtime::InvalidBashAudit::from_json(left),
            vega_runtime::InvalidBashAudit::from_json(right),
        )
    {
        return left.semantically_equal(&right);
    }
    if tool == "bash" {
        return vega_tools::bash_inputs_semantically_equal(left, right);
    }
    if !matches!(tool, "Write" | "Edit" | "write" | "edit") {
        return left == right;
    }
    if let (Ok(left), Ok(right)) = (
        vega_tools::WriteEditAudit::from_json(left),
        vega_tools::WriteEditAudit::from_json(right),
    ) {
        return left.tool().as_str().eq_ignore_ascii_case(tool)
            && right.tool().as_str().eq_ignore_ascii_case(tool)
            && left == right;
    }
    if let (Ok(left), Ok(right)) = (
        vega_tools::InvalidWriteEditAudit::from_json(left),
        vega_tools::InvalidWriteEditAudit::from_json(right),
    ) {
        return left.tool().as_str().eq_ignore_ascii_case(tool)
            && right.tool().as_str().eq_ignore_ascii_case(tool)
            && left == right;
    }
    false
}

pub(crate) fn validate_runtime_validation_event(
    call: &vega_runtime::RuntimeToolCall,
    result: &vega_runtime::RuntimeToolResult,
) -> Result<(), VegaError> {
    if call.name == "bash" {
        return crate::types::validate_runtime_validation_rejection(call, result)
            .map(|_| ())
            .ok_or_else(|| safe_audit_error(&call.name));
    }
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
    if !invalid.tool().as_str().eq_ignore_ascii_case(&call.name)
        || result.call_id != call.id
        || result.status != RuntimeToolStatus::Rejected
        || (result.output != expected
            && !invalid
                .validation_error_code()
                .matches_validation_result(invalid.tool().as_str(), &result.output))
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
    if matches!(call.name.as_str(), "Write" | "Edit" | "write" | "edit") {
        let valid = vega_tools::WriteEditAudit::from_json(&call.input_json)
            .ok()
            .is_some_and(|audit| audit.tool().as_str().eq_ignore_ascii_case(&call.name));
        let invalid = vega_tools::InvalidWriteEditAudit::from_json(&call.input_json)
            .ok()
            .is_some_and(|audit| audit.tool().as_str().eq_ignore_ascii_case(&call.name));
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
    if state.status != "pending_approval" {
        return Err(runtime_state_error());
    }
    if audit.decision == vega_runtime::RuntimeApprovalDecision::Deny {
        return Err(safe_audit_error(&state.tool));
    }
    let shared = approval_audit_from_runtime(audit);
    shared
        .to_json()
        .map_err(|_| safe_audit_error(&state.tool))?;
    if vega_runtime::is_subagent_tool(&state.tool)
        && (!vega_runtime::valid_subagent_input(&state.tool, &state.input_json)
            || shared.decision != Approval::Once
            || shared.source != ApprovalSource::ReadonlyTool
            || shared.note.is_some()
            || shared.danger.is_some()
            || remember.is_some())
    {
        return Err(safe_audit_error(&state.tool));
    }
    if matches!(
        state.tool.as_str(),
        "get_goal" | "update_goal" | "schedule_wakeup"
    ) && (shared.decision != Approval::Once
        || shared.source != ApprovalSource::ReadonlyTool
        || shared.note.is_some()
        || shared.danger.is_some()
        || remember.is_some())
    {
        return Err(safe_audit_error(&state.tool));
    }
    if state.tool.starts_with("mcp_")
        && (shared.decision != Approval::Once
            || shared.source != ApprovalSource::User
            || shared.danger.is_some()
            || remember.is_some())
    {
        return Err(safe_audit_error(&state.tool));
    }
    if state.tool == "grep" && crate::types::grep_invalid_input_claim(&state.input_json) {
        return (crate::types::valid_grep_invalid_input(&state.input_json)
            && shared.decision == Approval::Once
            && shared.source == ApprovalSource::ReadonlyTool
            && shared.note.is_none()
            && shared.danger.is_none()
            && remember.is_none())
        .then_some(())
        .ok_or_else(|| safe_audit_error(&state.tool));
    }
    if !approval_source_matches(
        &state.tool,
        &state.input_json,
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
    if state.tool == "grep" {
        return target.call_id == call_id
            && target.tool == RuntimeMutatingTool::Read
            && crate::types::grep_permission_target_matches(
                &state.input_json,
                &target.exact_pattern,
                &target.display_target,
            );
    }
    let exact_matches_input = match state.tool.as_str() {
        "Read" | "read" => serde_json::from_str::<serde_json::Value>(&state.input_json)
            .ok()
            .is_some_and(|input| {
                input.get("file_path").and_then(serde_json::Value::as_str)
                    == Some(target.exact_pattern.as_str())
                    && std::path::Path::new(&target.exact_pattern).is_absolute()
            }),
        "Write" | "Edit" | "write" | "edit" => {
            vega_tools::WriteEditAudit::from_json(&state.input_json)
                .ok()
                .is_some_and(|audit| {
                    audit.tool().as_str().eq_ignore_ascii_case(&state.tool)
                        && audit.path() == target.exact_pattern
                })
        }
        "bash" => vega_tools::bash_permission_signature(&state.input_json)
            .ok()
            .is_some_and(|command| command == target.exact_pattern),
        _ => false,
    };
    target.call_id == call_id
        && target.tool.as_str().eq_ignore_ascii_case(&state.tool)
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
    if result.call_id != call_id
        || (result.status == RuntimeToolStatus::Rejected && state.status != "pending_approval")
        || (result.status != RuntimeToolStatus::Rejected && state.status != "running")
    {
        return Err(runtime_state_error());
    }
    if result_approval.as_ref() != Some(approval)
        || (result.status != RuntimeToolStatus::Rejected && result.remember_rule.is_some())
        || matches!(
            result.status,
            RuntimeToolStatus::Rejected | RuntimeToolStatus::Failed
        ) && result.truncated.is_some()
        || result.status == RuntimeToolStatus::Success && result.truncated.is_none()
        || vega_runtime::computer_use::ComputerUseRun::tool_capability(&state.tool).is_some()
            && result.output == vega_runtime::computer_use::SUPERSEDED_CONTROL_OUTPUT
            && result.truncated.is_some()
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

pub(crate) fn ensure_message_updated(updated: usize, _message_id: &str) -> Result<(), VegaError> {
    if updated == 0 {
        Err(runtime_state_error())
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
