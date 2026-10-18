use super::context_accounting::{InputAnchor, InputDecision, PendingInputAnchor};
use super::tools_exec::SkillToolAction;
use super::*;
use crate::skills::{BatchPolicy, classify_tool_batch};
use crate::{ContextCheck, ContextCompactionRequest, ContextRuntimeError};

fn redact_runtime_tool_result(config: &RuntimeToolConfig, result: &mut RuntimeToolResult) {
    match config.redact_tool_output(&result.output) {
        Ok(output) => result.output = output,
        Err(_) => {
            result.output = "Tool error: credential safety check unavailable".to_string();
            result.status = RuntimeToolStatus::Failed;
            result.truncated = None;
            result.exit_code = None;
        }
    }
}

fn request_output_cap(
    max_tokens: Option<u32>,
    budget: Option<crate::ContextBudget>,
) -> Option<u32> {
    max_tokens.map(|cap| budget.map_or(cap, |budget| cap.min(budget.output_reserve() as u32)))
}

fn replay_user_boundary_follows_complete_tool_groups(
    messages: &[ChatMessage],
    boundary: usize,
) -> bool {
    if boundary >= messages.len() || messages[boundary].role != ChatRole::User {
        return false;
    }
    let mut pending = std::collections::HashSet::new();
    for message in &messages[..boundary] {
        match message.role {
            ChatRole::Assistant => {
                if !pending.is_empty() {
                    return false;
                }
                for call in &message.tool_calls {
                    if !pending.insert(call.id.as_str()) {
                        return false;
                    }
                }
            }
            ChatRole::Tool => {
                let Some(call_id) = message.tool_call_id.as_deref() else {
                    return false;
                };
                if !pending.remove(call_id) {
                    return false;
                }
            }
            ChatRole::User if !pending.is_empty() => return false,
            ChatRole::System => return false,
            ChatRole::User => {}
        }
    }
    pending.is_empty()
}

fn input_decision(
    anchor: Option<&InputAnchor>,
    messages: &[ChatMessage],
    tools: &[ToolDefinition],
    model: &str,
    reasoning: Option<&FrozenReasoning>,
    max_tokens: Option<u32>,
    revision: u64,
) -> Result<InputDecision, VegaError> {
    InputAnchor::decide(
        anchor, messages, tools, model, reasoning, max_tokens, revision,
    )
    .map_err(|error| VegaError::Context(error.into()))
}

fn accounting_event(
    decision: InputDecision,
    budget: crate::ContextBudget,
    stage: ContextAccountingStage,
) -> Result<RuntimeEvent, VegaError> {
    Ok(RuntimeEvent::ContextAccountingUpdated(
        ContextAccountingDecision {
            source: decision.source,
            stage,
            provider_input_baseline: decision.baseline,
            incremental_estimate: decision.incremental,
            predicted_input: decision.predicted,
            input_budget: budget.input_budget(),
            trigger_tokens: budget
                .trigger_tokens()
                .map_err(|error| VegaError::Context(error.into()))?,
            target_tokens: budget
                .target_tokens()
                .map_err(|error| VegaError::Context(error.into()))?,
            revision: decision.revision,
            covered_messages: decision.covered_messages,
        },
    ))
}

fn skill_runtime_error() -> VegaError {
    VegaError::Tool {
        tool: "skill".to_string(),
        message: "Skill run state unavailable or invalid".to_string(),
    }
}

fn compaction_usage_state(
    usages: &[crate::ContextCompactionUsage],
    complete: bool,
) -> crate::ContextCompactionUsageState {
    if !complete || usages.is_empty() {
        crate::ContextCompactionUsageState::Unknown
    } else {
        crate::ContextCompactionUsageState::Known {
            priced: usages.iter().all(|usage| usage.pricing.is_some()),
        }
    }
}

fn diagnostic_failure(error: &VegaError) -> RuntimeDiagnosticFailure {
    if error.context_input_overflow().is_some() {
        return RuntimeDiagnosticFailure::ContextOverLimit;
    }
    match error {
        VegaError::ProviderPayloadTooLarge { .. } => RuntimeDiagnosticFailure::ProviderProtocol,
        VegaError::ProviderDiagnostic { kind, .. } => match kind {
            crate::ProviderFailureKind::Http => RuntimeDiagnosticFailure::ProviderHttp,
            crate::ProviderFailureKind::Transport => {
                RuntimeDiagnosticFailure::ProviderTransportOrStream
            }
            crate::ProviderFailureKind::Protocol => RuntimeDiagnosticFailure::ProviderProtocol,
            crate::ProviderFailureKind::Rejected | crate::ProviderFailureKind::OutputLimit => {
                RuntimeDiagnosticFailure::ProviderRejected
            }
        },
        VegaError::Provider {
            status: Some(_), ..
        } => RuntimeDiagnosticFailure::ProviderHttp,
        VegaError::ReasoningBudgetExceeded { .. }
        | VegaError::ReasoningReplayBudgetExceeded { .. } => {
            RuntimeDiagnosticFailure::ReasoningLimit
        }
        VegaError::ReasoningReplayUnsupported | VegaError::ReasoningReplayIncomplete => {
            RuntimeDiagnosticFailure::ReasoningReplay
        }
        VegaError::Context(
            ContextRuntimeError::OverLimit { .. }
            | ContextRuntimeError::SummaryInputOverLimit { .. }
            | ContextRuntimeError::ResultOverLimit { .. },
        ) => RuntimeDiagnosticFailure::ContextOverLimit,
        VegaError::Cancelled => RuntimeDiagnosticFailure::UnknownSafeFailure,
        _ => RuntimeDiagnosticFailure::UnknownSafeFailure,
    }
}

fn install_compacted_replay_frame(
    messages: &mut [ChatMessage],
    cold_prefix: Option<usize>,
    user_boundary: Option<usize>,
) -> Result<(), VegaError> {
    let invalid = || VegaError::Context(ContextRuntimeError::InvalidProjection);
    let history_len = messages.len().checked_sub(1).ok_or_else(invalid)?;
    let cold_prefix = cold_prefix.unwrap_or(history_len);
    if cold_prefix == 0 || cold_prefix > history_len {
        return Err(invalid());
    }
    if messages[0]
        .replay_frame()
        .is_some_and(|frame| frame.policy() == crate::ReasoningReplayPolicy::GlmInterleaved)
        && !user_boundary.is_some_and(|boundary| {
            boundary >= cold_prefix
                && replay_user_boundary_follows_complete_tool_groups(&messages[1..], boundary)
        })
    {
        return Err(invalid());
    }
    messages[0].update_replay_cold_prefix(cold_prefix + 1);
    if let Some(boundary) = user_boundary {
        messages[0].begin_interleaved_user_round(boundary + 1);
    }
    Ok(())
}

fn validate_replay_response(
    reasoning: Option<&FrozenReasoning>,
    tools_requested: bool,
    calls: &[RuntimeToolCall],
    presence: &crate::ChatReasoningPresence,
    response_reasoning: &[serde_json::Value],
    response_layout: Option<&crate::ResponseReplayLayout>,
    assistant_text: &str,
) -> Result<(), VegaError> {
    let raw_chat_state = !matches!(presence, crate::ChatReasoningPresence::Absent);
    let raw_response_state = !response_reasoning.is_empty() || response_layout.is_some();
    let policy = reasoning
        .map(|profile| profile.replay_policy)
        .unwrap_or(crate::ReasoningReplayPolicy::Unknown);
    if !calls.is_empty()
        && matches!(
            policy,
            crate::ReasoningReplayPolicy::Unknown | crate::ReasoningReplayPolicy::StandardChat
        )
        && (raw_chat_state || raw_response_state)
    {
        return Err(VegaError::ReasoningReplayUnsupported);
    }
    if tools_requested
        && !reasoning.is_some_and(|profile| profile.choice == crate::ReasoningChoice::Disabled)
    {
        if matches!(
            policy,
            crate::ReasoningReplayPolicy::DeepSeekTools
                | crate::ReasoningReplayPolicy::GlmPreserved
                | crate::ReasoningReplayPolicy::GlmInterleaved
        ) && matches!(presence, crate::ChatReasoningPresence::Absent)
        {
            return Err(VegaError::ReasoningReplayIncomplete);
        }
        if policy == crate::ReasoningReplayPolicy::ResponsesItems && response_layout.is_none() {
            return Err(VegaError::ReasoningReplayIncomplete);
        }
    }
    if policy == crate::ReasoningReplayPolicy::ResponsesItems
        && !calls.is_empty()
        && response_layout.is_none()
    {
        return Err(VegaError::ReasoningReplayIncomplete);
    }
    if let Some(layout) = response_layout {
        let candidate = ChatMessage::assistant_with_tools(
            assistant_text,
            calls
                .iter()
                .map(|call| crate::ChatToolCall {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    input_json: call.input_json.clone(),
                })
                .collect(),
        );
        let mut candidate = candidate;
        candidate.response_reasoning = response_reasoning.to_vec();
        candidate.response_replay_layout = Some(layout.clone());
        crate::openai::responses::response_layout_items(&candidate)?;
    }
    Ok(())
}

fn primary_attempt_event(
    attempt_id: &str,
    state: RuntimeDiagnosticState,
    failure: Option<RuntimeDiagnosticFailure>,
    started_at: std::time::Instant,
    metrics: &RuntimeDiagnosticMetrics,
    error: Option<&VegaError>,
) -> RuntimeEvent {
    let mut metrics = metrics.clone();
    if let Some(error) = error {
        let detail = error.diagnostic_metrics();
        metrics.failure_tool_index = detail.failure_tool_index;
        metrics.failure_source = detail.failure_source;
        metrics.failure_kind = detail.failure_kind;
        metrics.failure_scope = detail.failure_scope;
        metrics.failure_observed = detail.failure_observed;
        metrics.failure_limit = detail.failure_limit;
        metrics.failure_units = detail.failure_units;
        metrics.failure_observed_items = detail.failure_observed_items;
        metrics.failure_limit_items = detail.failure_limit_items;
    }
    match error {
        Some(VegaError::ProviderDiagnostic {
            status,
            retry_count,
            request_id,
            ..
        }) => {
            if status.is_some() {
                metrics.provider.http_status = *status;
            }
            if retry_count.is_some() {
                metrics.provider.retry_count = *retry_count;
            }
            if request_id.is_some() {
                metrics.provider.request_id = request_id.clone();
            }
        }
        Some(VegaError::Provider {
            status: Some(status),
            ..
        }) => {
            metrics.provider.http_status = Some(*status);
        }
        _ => {}
    }
    RuntimeEvent::DiagnosticAttempt(RuntimeDiagnosticAttempt {
        attempt_id: attempt_id.to_string(),
        phase: RuntimeDiagnosticPhase::PrimaryModel,
        state,
        failure,
        duration_ms: (state != RuntimeDiagnosticState::Started)
            .then(|| u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX)),
        metrics,
    })
}

#[cfg(test)]
mod diagnostic_tests {
    use super::*;

    #[test]
    fn stream_failure_preserves_success_response_metadata() {
        let metrics = RuntimeDiagnosticMetrics {
            provider: crate::ProviderResponseMetadata {
                http_status: Some(200),
                request_id: Some("resp-opaque-1".into()),
                retry_count: Some(2),
            },
            ..RuntimeDiagnosticMetrics::default()
        };
        let error = VegaError::ProviderDiagnostic {
            kind: crate::ProviderFailureKind::Protocol,
            status: None,
            message: "parser detail canary".into(),
            retryable: false,
            retry_count: None,
            request_id: None,
        };
        let event = primary_attempt_event(
            "attempt",
            RuntimeDiagnosticState::Failed,
            Some(RuntimeDiagnosticFailure::ProviderProtocol),
            std::time::Instant::now(),
            &metrics,
            Some(&error),
        );
        let RuntimeEvent::DiagnosticAttempt(event) = event else {
            unreachable!()
        };
        assert_eq!(event.metrics.provider.http_status, Some(200));
        assert_eq!(
            event.metrics.provider.request_id.as_deref(),
            Some("resp-opaque-1")
        );
        assert_eq!(event.metrics.provider.retry_count, Some(2));
    }
}

// Deliberately closed: status-less transport/projection guards and authorization
// failures must never be converted into permission to issue a primary request.
fn recoverable_compaction_failure(error: &VegaError) -> bool {
    matches!(
        error,
        VegaError::Provider {
            status: Some(400 | 408 | 413 | 429 | 500..=599),
            ..
        } | VegaError::Context(
            ContextRuntimeError::NoCompactablePrefix
                | ContextRuntimeError::SummaryInputOverLimit { .. }
                | ContextRuntimeError::ResultOverLimit { .. }
                | ContextRuntimeError::InvalidSummary
                | ContextRuntimeError::SummaryTimedOut
                | ContextRuntimeError::SummaryOutputTruncated { .. }
        ) | VegaError::ProviderDiagnostic {
            status: Some(400 | 408 | 413 | 429 | 500..=599),
            ..
        }
    )
}

fn skill_authority_current(config: Option<&RuntimeSkillConfig>) -> bool {
    config
        .and_then(|skills| skills.authority_probe.as_ref())
        .is_none_or(|probe| probe())
}

fn skill_system_prompt(base: &str, run: &SkillRun) -> Result<String, VegaError> {
    let mut result = base.to_string();
    let catalog = run.model_catalog();
    if !catalog.is_empty() {
        result.push_str("\n\n");
        result.push_str(catalog);
    }
    let envelope = run
        .render_skill_envelope()
        .map_err(|_| skill_runtime_error())?;
    if !envelope.is_empty() {
        result.push_str("\n\n");
        result.push_str(&envelope);
    }
    Ok(result)
}

#[derive(Clone, Copy)]
struct InputAccounting<'a> {
    anchor: Option<&'a InputAnchor>,
    model: &'a str,
    reasoning: Option<&'a FrozenReasoning>,
    max_tokens: Option<u32>,
    revision: u64,
}

fn skill_fits(
    base: &str,
    catalog: &str,
    prospective: &str,
    messages: &[ChatMessage],
    tools: &[ToolDefinition],
    budget: Option<crate::ContextBudget>,
    accounting: InputAccounting<'_>,
) -> bool {
    let Some(budget) = budget else {
        return true;
    };
    let mut projected = messages.to_vec();
    let mut system = base.to_string();
    if !catalog.is_empty() {
        system.push_str("\n\n");
        system.push_str(catalog);
    }
    if !prospective.is_empty() {
        system.push_str("\n\n");
        system.push_str(prospective);
    }
    if let Some(first) = projected.first_mut() {
        let mut system_message = ChatMessage::new(ChatRole::System, system);
        if let Some(frame) = first.replay_frame().cloned() {
            system_message.attach_replay_frame(frame);
        }
        *first = system_message;
    }
    input_decision(
        accounting.anchor,
        &projected,
        tools,
        accounting.model,
        accounting.reasoning,
        accounting.max_tokens,
        accounting.revision,
    )
    .is_ok_and(|decision| decision.predicted <= budget.input_budget())
}

#[allow(clippy::too_many_arguments)]
async fn compact_before_skill_rejection<F, Fut>(
    computer_use: Option<&crate::computer_use::ComputerUseRun>,
    prospective: &str,
    base: &str,
    catalog: &str,
    messages: &mut Vec<ChatMessage>,
    pending_result: Option<&ChatMessage>,
    tools: &[ToolDefinition],
    budget: Option<crate::ContextBudget>,
    anchor: Option<&InputAnchor>,
    model: &str,
    reasoning: Option<&FrozenReasoning>,
    max_tokens: Option<u32>,
    revision: u64,
    hook: Option<&dyn crate::ContextCompactionHook>,
    operation_id: &str,
    source_version: &mut Option<u64>,
    source_fingerprint: &mut Option<String>,
    projection_revision: &mut u64,
    attempted_source: &mut Option<(u64, Option<String>, u64)>,
    automatic_failures: &mut u8,
    cancel: &CancellationToken,
    events: &mut Vec<RuntimeEvent>,
    sink: &mut F,
) -> Result<bool, VegaError>
where
    F: FnMut(RuntimeEvent) -> Fut,
    Fut: Future<Output = Result<(), VegaError>>,
{
    let Some(budget) = budget.filter(|budget| budget.automatic_compaction()) else {
        return Ok(false);
    };
    if *automatic_failures >= 3 {
        return Ok(false);
    }
    let Some(hook) = hook else {
        return Ok(false);
    };
    // Only older complete turns may be compacted. The current user task and
    // any current tool results cannot be reinterpreted as activation intent.
    let latest_user = messages
        .iter()
        .rposition(|message| message.role == ChatRole::User);
    if !latest_user.is_some_and(|index| {
        index > 1
            && messages[1..index]
                .iter()
                .any(|message| message.role == ChatRole::Assistant)
    }) {
        return Ok(false);
    }
    let source = source_version.unwrap_or_default();
    let fingerprint = source_fingerprint.clone();
    if *attempted_source == Some((source, fingerprint.clone(), *projection_revision)) {
        return Ok(false);
    }
    if cancel.is_cancelled() {
        return Ok(false);
    }
    let mut system = base.to_string();
    if !catalog.is_empty() {
        system.push_str("\n\n");
        system.push_str(catalog);
    }
    if !prospective.is_empty() {
        system.push_str("\n\n");
        system.push_str(prospective);
    }
    let mut projected = messages.clone();
    if let Some(result) = pending_result {
        projected.push(result.clone());
    }
    let mut system_message = ChatMessage::new(ChatRole::System, system.clone());
    if let Some(frame) = projected[0].replay_frame().cloned() {
        system_message.attach_replay_frame(frame);
    }
    projected[0] = system_message;
    let decision = input_decision(
        anchor, &projected, tools, model, reasoning, max_tokens, revision,
    )?;
    let estimate = decision.estimate;
    let event = accounting_event(decision, budget, ContextAccountingStage::SkillProspect)?;
    sink(event.clone()).await?;
    events.push(event);
    let target_tokens = budget
        .target_tokens()
        .map_err(|error| VegaError::Context(error.into()))?;
    let generation = projection_revision.saturating_add(1);
    let operation_key = format!(
        "{operation_id}:context-generation-{generation}:attempt-{}",
        ulid::Ulid::generate()
    );
    let started = RuntimeEvent::ContextCompactionStatusUpdated {
        status: crate::ContextCompactionStatusUpdate {
            operation_key: operation_key.clone(),
            generation,
            phase: crate::ContextCompactionPhase::Started,
            source_version: source,
            estimated_tokens: estimate.input_tokens,
            input_budget: budget.input_budget(),
            target_tokens,
            usage: crate::ContextCompactionUsageState::Pending,
            failure: None,
        },
    };
    sink(started.clone()).await?;
    events.push(started);
    *attempted_source = Some((source, fingerprint.clone(), *projection_revision));
    if let Some(run) = computer_use {
        run.invalidate_observations();
        run.trim_messages(messages);
        run.trim_messages(&mut projected);
        crate::computer_use::redact_ephemeral_messages(messages);
        crate::computer_use::redact_ephemeral_messages(&mut projected);
    }
    let compacted = hook
        .compact(
            ContextCompactionRequest {
                system_prompt: system,
                messages: projected.iter().skip(1).cloned().collect(),
                tools: tools.to_vec(),
                budget,
                estimate,
                target_tokens,
                source_version: source,
                source_fingerprint: fingerprint,
                require_source_fence: false,
                source_owner_id: Some(operation_id.to_string()),
            },
            cancel.clone(),
        )
        .await;
    let (compacted, usage_state) = match compacted {
        Ok(compacted) => {
            let usage_state = compaction_usage_state(&compacted.usages, compacted.usage_complete);
            for usage in compacted.usages.iter().cloned() {
                let event = RuntimeEvent::ContextCompactionUsageUpdated { usage };
                sink(event.clone()).await?;
                events.push(event);
            }
            (compacted, usage_state)
        }
        Err(failure) => {
            let usage_state = compaction_usage_state(&failure.usages, failure.usage_complete);
            for usage in failure.usages {
                let event = RuntimeEvent::ContextCompactionUsageUpdated { usage };
                sink(event.clone()).await?;
                events.push(event);
            }
            let event = RuntimeEvent::ContextCompactionStatusUpdated {
                status: crate::ContextCompactionStatusUpdate {
                    operation_key,
                    generation,
                    phase: if matches!(failure.error.as_ref(), VegaError::Cancelled) {
                        crate::ContextCompactionPhase::Cancelled
                    } else {
                        crate::ContextCompactionPhase::Failed
                    },
                    source_version: source,
                    estimated_tokens: estimate.input_tokens,
                    input_budget: budget.input_budget(),
                    target_tokens,
                    usage: usage_state,
                    failure: Some(crate::ContextCompactionStatusFailure::from_error(
                        failure.error.as_ref(),
                    )),
                },
            };
            sink(event.clone()).await?;
            events.push(event);
            if matches!(failure.error.as_ref(), VegaError::Cancelled) {
                return Err(*failure.error);
            }
            if recoverable_compaction_failure(failure.error.as_ref()) {
                *automatic_failures = automatic_failures.saturating_add(1);
                // The prospective envelope remains rejected. The ordinary loop
                // independently gates the unchanged, actually active request.
                return Ok(false);
            }
            return Err(*failure.error);
        }
    };
    if compacted
        .messages
        .iter()
        .any(|message| message.role == ChatRole::System)
    {
        return Err(VegaError::Context(
            ContextRuntimeError::SystemMessageInResult,
        ));
    }
    let mut next_projected = Vec::with_capacity(compacted.messages.len() + 2);
    next_projected.push(projected.remove(0));
    next_projected.extend(compacted.messages.iter().cloned());
    if let Some(result) = pending_result
        && next_projected.last() != Some(result)
    {
        next_projected.push(result.clone());
    }
    install_compacted_replay_frame(
        &mut next_projected,
        compacted.replay_cold_prefix_messages,
        compacted.replay_interleaved_user_boundary,
    )?;
    let compacted_estimate = crate::estimate_wire_context(&next_projected, tools)
        .map_err(|error| VegaError::Context(error.into()))?;
    let fits = compacted_estimate.input_tokens <= target_tokens
        && compacted_estimate.input_tokens <= budget.input_budget();
    let event = RuntimeEvent::ContextCompactionStatusUpdated {
        status: crate::ContextCompactionStatusUpdate {
            operation_key,
            generation,
            phase: if fits {
                crate::ContextCompactionPhase::Succeeded
            } else {
                crate::ContextCompactionPhase::Failed
            },
            source_version: source,
            estimated_tokens: estimate.input_tokens,
            input_budget: budget.input_budget(),
            target_tokens,
            usage: usage_state,
            failure: (!fits).then_some(crate::ContextCompactionStatusFailure::OverLimit),
        },
    };
    sink(event.clone()).await?;
    events.push(event);
    if !fits || cancel.is_cancelled() {
        if !fits {
            *automatic_failures = automatic_failures.saturating_add(1);
        }
        return Ok(false);
    }
    *automatic_failures = 0;
    // The candidate is not active yet; preserve the old system authority
    // until the same hash is rechecked and activation succeeds below.
    next_projected[0].content.clone_from(&messages[0].content);
    if let Some(result) = pending_result
        && next_projected.last() == Some(result)
    {
        next_projected.pop();
    }
    *messages = next_projected;
    discard_reasoning_events(events, 0);
    sink(RuntimeEvent::ReasoningRetentionReleased).await?;
    events.push(RuntimeEvent::ReasoningRetentionReleased);
    *projection_revision = generation;
    *source_version = Some(compacted.source_version);
    *source_fingerprint = compacted.source_fingerprint;
    Ok(true)
}

struct SkillExecution {
    result: RuntimeToolResult,
    state_event: Option<RuntimeEvent>,
    over_budget_envelope: Option<String>,
}

#[allow(clippy::too_many_arguments)]
fn execute_skill_call(
    config: &RuntimeSkillConfig,
    call: &RuntimeToolCall,
    action: SkillToolAction,
    base_prompt: &str,
    messages: &[ChatMessage],
    definitions: &[ToolDefinition],
    budget: Option<crate::ContextBudget>,
    anchor: Option<&InputAnchor>,
    model: &str,
    reasoning: Option<&FrozenReasoning>,
    max_tokens: Option<u32>,
    revision: u64,
    direct_user_round: bool,
) -> Result<SkillExecution, VegaError> {
    let mut run = config.run.lock().map_err(|_| skill_runtime_error())?;
    let catalog = run.model_catalog().to_string();
    match action {
        SkillToolAction::Load { name } => {
            let mut over_budget_envelope = None;
            let outcome = run.load_model_for_round(&name, direct_user_round, |prospective| {
                let mut projected = messages.to_vec();
                projected.push(ChatMessage::tool_result(
                    &call.id,
                    serde_json::json!({"name": name, "status": "loaded"}).to_string(),
                ));
                let fits = skill_fits(
                    base_prompt,
                    &catalog,
                    prospective,
                    &projected,
                    definitions,
                    budget,
                    InputAccounting {
                        anchor,
                        model,
                        reasoning,
                        max_tokens,
                        revision,
                    },
                );
                if !fits {
                    over_budget_envelope = Some(prospective.to_string());
                }
                fits
            });
            let success = matches!(outcome.receipt.status, "loaded" | "already_loaded");
            let output = outcome
                .receipt
                .to_json()
                .map_err(|_| skill_runtime_error())?;
            let mut result = terminal_result(
                call,
                output,
                if success {
                    RuntimeToolStatus::Success
                } else {
                    RuntimeToolStatus::Failed
                },
                None,
            );
            if success {
                result.truncated = Some(false);
            }
            let binding = run.binding().cloned();
            let snapshot = if success && outcome.receipt.status == "loaded" && binding.is_some() {
                Some(run.export_snapshot().map_err(|_| skill_runtime_error())?)
            } else {
                None
            };
            Ok(SkillExecution {
                result,
                state_event: Some(RuntimeEvent::SkillActivation {
                    audit: outcome.audit,
                    binding,
                    snapshot,
                }),
                over_budget_envelope: (outcome.receipt.status == "over_budget")
                    .then_some(over_budget_envelope)
                    .flatten(),
            })
        }
        SkillToolAction::Read { name, path } => {
            let envelope = run
                .render_skill_envelope()
                .map_err(|_| skill_runtime_error())?;
            let read = run.read_resource(&name, &path, |prospective| {
                let mut projected = messages.to_vec();
                projected.push(ChatMessage::tool_result(&call.id, prospective.to_string()));
                skill_fits(
                    base_prompt,
                    &catalog,
                    &envelope,
                    &projected,
                    definitions,
                    budget,
                    InputAccounting {
                        anchor,
                        model,
                        reasoning,
                        max_tokens,
                        revision,
                    },
                )
            });
            let (output, success) = match read {
                Ok(output) => (output, true),
                Err(error) => (
                    serde_json::json!({"name":name,"status":error.code()}).to_string(),
                    false,
                ),
            };
            let mut result = terminal_result(
                call,
                output,
                if success {
                    RuntimeToolStatus::Success
                } else {
                    RuntimeToolStatus::Failed
                },
                None,
            );
            if success {
                result.truncated = Some(false);
            }
            let state_event = if success {
                match run.binding().cloned() {
                    Some(binding) => Some(RuntimeEvent::SkillSnapshot {
                        binding,
                        snapshot: run.export_snapshot().map_err(|_| skill_runtime_error())?,
                    }),
                    None => None,
                }
            } else {
                None
            };
            Ok(SkillExecution {
                result,
                state_event,
                over_budget_envelope: None,
            })
        }
    }
}

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

fn retain_reasoning_event(
    events: &mut Vec<RuntimeEvent>,
    mut event: RuntimeEvent,
    retained: &mut usize,
) {
    let text = match &mut event {
        RuntimeEvent::ThinkingDelta(text) | RuntimeEvent::SummaryDelta(text) => text,
        _ => return,
    };
    let mut end = text
        .len()
        .min(REASONING_DISPLAY_MAX_BYTES.saturating_sub(*retained));
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    if end == 0 {
        return;
    }
    text.truncate(end);
    text.shrink_to_fit();
    *retained += text.len();
    events.push(event);
}

fn discard_reasoning_events(events: &mut Vec<RuntimeEvent>, start: usize) {
    let mut index = 0usize;
    events.retain(|event| {
        let keep = index < start
            || !matches!(
                event,
                RuntimeEvent::ThinkingDelta(_) | RuntimeEvent::SummaryDelta(_)
            );
        index += 1;
        keep
    });
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
    sink: F,
) -> Result<AgentOutcome, VegaError>
where
    F: FnMut(RuntimeEvent) -> Fut,
    Fut: Future<Output = Result<(), VegaError>>,
{
    let computer_use = request.tool_config.computer_use.clone();
    let lifetime = CancellationToken::new();
    let monitor = computer_use.clone().map(|run| {
        let lifetime = lifetime.clone();
        let root_cancel = cancel.clone();
        tokio::spawn(async move {
            run.monitor(lifetime, root_cancel).await;
        })
    });
    let result = run_agent_with_permission_sink_and_context_inner(
        provider,
        tools,
        request,
        cancel,
        permission_hook,
        external_context_hook,
        sink,
    )
    .await;
    lifetime.cancel();
    if let Some(monitor) = monitor {
        let _ = monitor.await;
    }
    if let Some(run) = computer_use {
        run.shutdown().await;
    }
    result
}

async fn run_agent_with_permission_sink_and_context_inner<F, Fut>(
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
    let mut replay_frame = request.replay_frame.clone().unwrap_or_else(|| {
        crate::ReplayFrame::new(
            request.history.len().saturating_add(1),
            request
                .reasoning
                .as_ref()
                .map(|profile| profile.replay_policy)
                .unwrap_or(crate::ReasoningReplayPolicy::Unknown),
        )
    });
    let (replay_route_fingerprint, mut replay_candidate) = replay_frame.take_runtime_fence();
    let mut cold_current_user = replay_frame
        .cold_prefix_len()
        .checked_sub(1)
        .and_then(|index| request.history.get(index))
        .filter(|message| message.role == ChatRole::User)
        .cloned();
    let mut replay_environment = None::<[u8; 32]>;
    let builtin_cua = request
        .tool_config
        .computer_use
        .as_ref()
        .is_some_and(|run| run.enabled(crate::computer_use::Capability::Desktop));
    let base_system_prompt = if builtin_cua {
        crate::computer_use::guide::system_prompt(&request.system_prompt)
    } else {
        request.system_prompt.clone()
    };
    let base_system_prompt =
        automation::system_context(base_system_prompt, request.tool_config.automation.as_ref());
    let mut system_message = ChatMessage::new(ChatRole::System, base_system_prompt.clone());
    system_message.attach_replay_frame(replay_frame);
    messages.push(system_message);
    messages.extend(request.history);
    let context_budget = request.context_budget;
    let request_context_hook = request.context_compaction_hook.clone();
    let mut context_operation_id = request
        .context_operation_id
        .clone()
        .unwrap_or_else(|| ulid::Ulid::generate().to_string());
    let mut context_source_version = request.context_source_version;
    let mut context_source_fingerprint = request.context_source_fingerprint.clone();
    // Durable source identity is paired with a local live-projection revision:
    // tool rounds can append content before SQLite's fingerprint catches up.
    let mut live_projection_revision = 0_u64;
    let mut attempted_context_source = None::<(u64, Option<String>, u64)>;
    let mut automatic_compaction_failures = 0_u8;
    let mut provider_overflow_recovered = false;
    let mut force_provider_compaction = false;
    let mut completed = request.completed_tool_results;
    let tool_config = request.tool_config;
    let skill_config = tool_config.skills.clone();
    let skill_catalog = if let Some(config) = &skill_config {
        config
            .run
            .lock()
            .map_err(|_| skill_runtime_error())?
            .model_catalog()
            .to_string()
    } else {
        String::new()
    };
    let capabilities = RunCapabilitySnapshot::freeze(
        tool_config.run_mode,
        tool_config.permission_mode,
        tool_config.mcp_candidates.clone(),
    )?
    .with_skills(!skill_catalog.is_empty(), skill_config.is_some())?
    .with_computer_use(tool_config.computer_use.as_ref())
    .with_automation(tool_config.automation.as_ref());
    let mut exact_rules: HashSet<RuntimeExactRule> =
        tool_config.exact_rules.iter().cloned().collect();
    let mut events = Vec::new();
    let mut final_text = String::new();
    let mut tool_call_count = 0usize;
    let mut executed_tool_call_count = 0usize;
    // Agentic turn counter: the outer `loop` below issues exactly one provider
    // request per iteration. `turn_limit == 0` means unlimited (Claude Code's
    // `--max-turns` default); a positive value soft-stops before issuing the
    // (limit + 1)th request.
    let mut turn_index = 0usize;
    let mut direct_user_skill_round = true;
    let mut input_anchor: Option<InputAnchor> = None;
    let mut primary_revision = 0_u64;
    let mut progress = progress::ProgressReporter::new();
    let mut recovery = progress_recovery::ProgressRecovery::default();

    if !skill_authority_current(skill_config.as_ref()) {
        cancel.cancel();
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

    if builtin_cua {
        emit!(
            events,
            sink,
            RuntimeEvent::BuiltinSkillActivated(crate::computer_use::guide::activation())
        );
    }
    if let Some(config) = &skill_config {
        let initial_snapshot = {
            let run = config.run.lock().map_err(|_| skill_runtime_error())?;
            run.binding()
                .cloned()
                .map(|binding| {
                    run.export_snapshot()
                        .map(|snapshot| (binding, snapshot))
                        .map_err(|_| skill_runtime_error())
                })
                .transpose()?
        };
        if let Some((binding, snapshot)) = initial_snapshot {
            emit!(
                events,
                sink,
                RuntimeEvent::SkillSnapshot { binding, snapshot }
            );
        }
        for selection in &config.explicit {
            let mut compacted_for_selection = false;
            let (activation, binding, snapshot) = loop {
                let (activation, binding, snapshot, over_budget_envelope) = {
                    let mut run = config.run.lock().map_err(|_| skill_runtime_error())?;
                    let mut over_budget_envelope = None;
                    let activation = run.load_explicit(selection, |prospective| {
                        let fits = skill_fits(
                            &base_system_prompt,
                            &skill_catalog,
                            prospective,
                            &messages,
                            capabilities.definitions(),
                            context_budget,
                            InputAccounting {
                                anchor: input_anchor.as_ref(),
                                model: &request.model,
                                reasoning: request.reasoning.as_ref(),
                                max_tokens: request_output_cap(request.max_tokens, context_budget),
                                revision: primary_revision,
                            },
                        );
                        if !fits {
                            over_budget_envelope = Some(prospective.to_string());
                        }
                        fits
                    });
                    let binding = run.binding().cloned();
                    let snapshot = if activation.receipt.status == "loaded" && binding.is_some() {
                        Some(run.export_snapshot().map_err(|_| skill_runtime_error())?)
                    } else {
                        None
                    };
                    (activation, binding, snapshot, over_budget_envelope)
                };
                if !compacted_for_selection
                    && activation.receipt.status == "over_budget"
                    && let Some(prospective) = over_budget_envelope
                {
                    compacted_for_selection = true;
                    let hook = external_context_hook.or(request_context_hook.as_deref());
                    if compact_before_skill_rejection(
                        tool_config.computer_use.as_ref(),
                        &prospective,
                        &base_system_prompt,
                        &skill_catalog,
                        &mut messages,
                        None,
                        capabilities.definitions(),
                        context_budget,
                        input_anchor.as_ref(),
                        &request.model,
                        request.reasoning.as_ref(),
                        request_output_cap(request.max_tokens, context_budget),
                        primary_revision,
                        hook,
                        &context_operation_id,
                        &mut context_source_version,
                        &mut context_source_fingerprint,
                        &mut live_projection_revision,
                        &mut attempted_context_source,
                        &mut automatic_compaction_failures,
                        &cancel,
                        &mut events,
                        &mut sink,
                    )
                    .await?
                    {
                        replay_candidate = None;
                        cold_current_user = None;
                        input_anchor = None;
                        recovery = progress_recovery::ProgressRecovery::default();
                        continue;
                    }
                }
                break (activation, binding, snapshot);
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
            let must_pause = activation.must_pause;
            emit!(
                events,
                sink,
                RuntimeEvent::SkillActivation {
                    audit: activation.audit,
                    binding,
                    snapshot,
                }
            );
            if must_pause {
                emit!(
                    events,
                    sink,
                    RuntimeEvent::Error(Arc::new(skill_runtime_error()))
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

    'primary: loop {
        turn_index += 1;
        if tool_config.turn_limit > 0 && turn_index > tool_config.turn_limit {
            let notice = format!(
                "Agent turn limit ({}) reached; stopping without issuing another request.",
                tool_config.turn_limit
            );
            final_text.push_str(&notice);
            emit!(events, sink, RuntimeEvent::TextDelta(notice.clone()));
            messages.push(ChatMessage::new(ChatRole::Assistant, notice));
            emit!(
                events,
                sink,
                RuntimeEvent::Finished(RuntimeFinishReason::TurnLimit)
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
        if !skill_authority_current(skill_config.as_ref()) {
            cancel.cancel();
        }
        let system_prompt = if let Some(config) = &skill_config {
            let run = config.run.lock().map_err(|_| skill_runtime_error())?;
            skill_system_prompt(&base_system_prompt, &run)?
        } else {
            base_system_prompt.clone()
        };
        let mut system_message = ChatMessage::new(ChatRole::System, system_prompt.clone());
        if let Some(frame) = messages[0].replay_frame().cloned() {
            system_message.attach_replay_frame(frame);
        }
        messages[0] = system_message;
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

        if let Some(run) = &tool_config.computer_use {
            if run.wait_ready(&cancel).await.is_err() {
                cancel.cancel();
            }
            run.trim_messages(&mut messages);
        }
        let tool_definitions = capabilities.definitions().to_vec();
        if let Some(route_fingerprint) = replay_route_fingerprint {
            let current_environment = crate::agent::replay_environment_fingerprint(
                (
                    &request.model,
                    request.reasoning.as_ref(),
                    request.max_tokens,
                ),
                &tool_config,
                context_budget,
                &route_fingerprint,
                &system_prompt,
                &tool_definitions,
            )?;
            match replay_environment {
                Some(previous) if previous != current_environment => {
                    messages[0].mark_replay_unsealable();
                    replay_candidate = None;
                }
                Some(_) => {}
                None => {
                    replay_environment = Some(current_environment);
                    let mut frame = messages[0]
                        .replay_frame()
                        .cloned()
                        .ok_or(VegaError::Context(ContextRuntimeError::InvalidProjection))?;
                    frame.bind_environment(current_environment);
                    messages[0].attach_replay_frame(frame);
                    if let Some(candidate) = replay_candidate.take()
                        && candidate.environment_fingerprint == current_environment
                        && let Some(current_user) = cold_current_user.clone()
                        && let Some(candidate_system) = candidate.messages.first()
                        && candidate_system.role == ChatRole::System
                    {
                        let candidate_frame = candidate_system.replay_frame().cloned();
                        if let Some(candidate_frame) = candidate_frame
                            && candidate_frame.seal_eligible()
                            && candidate_frame.cold_prefix_len() <= candidate.messages.len()
                        {
                            let mut warm = candidate.messages;
                            let mut warm_system =
                                ChatMessage::new(ChatRole::System, system_prompt.clone());
                            warm_system.attach_replay_frame(candidate_frame);
                            warm[0] = warm_system;
                            let interleaved_user_boundary = warm.len();
                            warm[0].begin_interleaved_user_round(interleaved_user_boundary);
                            warm.push(current_user);
                            messages = warm;
                        }
                    }
                }
            }
            if let Some(run) = &tool_config.computer_use {
                run.trim_messages(&mut messages);
            }
        }
        'compaction: {
            let Some(budget) = context_budget else {
                break 'compaction;
            };
            let decision = input_decision(
                input_anchor.as_ref(),
                &messages,
                &tool_definitions,
                &request.model,
                request.reasoning.as_ref(),
                request_output_cap(request.max_tokens, context_budget),
                primary_revision,
            )?;
            let estimate = decision.estimate;
            emit!(
                events,
                sink,
                accounting_event(decision, budget, ContextAccountingStage::PrimaryPreflight)?
            );
            let check = budget
                .check(estimate)
                .map_err(|error| VegaError::Context(error.into()))?;
            let should_compact = (force_provider_compaction
                || matches!(check, ContextCheck::Triggered | ContextCheck::OverLimit))
                && budget.automatic_compaction();
            if should_compact {
                let source_version = context_source_version.unwrap_or_default();
                let source_fingerprint = context_source_fingerprint.clone();
                if automatic_compaction_failures >= 3
                    || attempted_context_source
                        == Some((
                            source_version,
                            source_fingerprint.clone(),
                            live_projection_revision,
                        ))
                {
                    if force_provider_compaction || matches!(check, ContextCheck::OverLimit) {
                        return Err(VegaError::Context(ContextRuntimeError::OverLimit {
                            estimated_tokens: estimate.input_tokens,
                            input_budget: budget.input_budget(),
                        }));
                    }
                    break 'compaction;
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
                if let Some(run) = &tool_config.computer_use {
                    run.invalidate_observations();
                    run.trim_messages(&mut messages);
                    crate::computer_use::redact_ephemeral_messages(&mut messages);
                }
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
                        let usage_state =
                            compaction_usage_state(&failure.usages, failure.usage_complete);
                        for usage in failure.usages {
                            emit!(
                                events,
                                sink,
                                RuntimeEvent::ContextCompactionUsageUpdated { usage }
                            );
                        }
                        if matches!(failure.error.as_ref(), VegaError::Cancelled) {
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
                        if force_provider_compaction {
                            return Err(*failure.error);
                        }
                        if recoverable_compaction_failure(failure.error.as_ref()) {
                            automatic_compaction_failures =
                                automatic_compaction_failures.saturating_add(1);
                            if !matches!(check, ContextCheck::OverLimit) {
                                break 'compaction;
                            }
                        }
                        return Err(*failure.error);
                    }
                };
                let usage_state =
                    compaction_usage_state(&compacted.usages, compacted.usage_complete);
                for usage in compacted.usages.iter().cloned() {
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
                let replay_cold_prefix_messages = compacted.replay_cold_prefix_messages;
                let replay_interleaved_user_boundary = compacted.replay_interleaved_user_boundary;
                let interleaved_policy = system.replay_frame().is_some_and(|frame| {
                    frame.policy() == crate::ReasoningReplayPolicy::GlmInterleaved
                });
                let replay_boundaries_valid = match replay_cold_prefix_messages {
                    Some(cold_prefix) => cold_prefix > 0 && cold_prefix <= compacted.messages.len(),
                    None => !interleaved_policy,
                } && (!interleaved_policy
                    || match (
                        replay_cold_prefix_messages,
                        replay_interleaved_user_boundary,
                    ) {
                        (Some(cold_prefix), Some(user_boundary)) => {
                            user_boundary >= cold_prefix
                                && replay_user_boundary_follows_complete_tool_groups(
                                    &compacted.messages,
                                    user_boundary,
                                )
                        }
                        _ => false,
                    });
                if !replay_boundaries_valid {
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
                    return Err(VegaError::Context(ContextRuntimeError::InvalidProjection));
                }
                let mut next_messages = Vec::with_capacity(compacted.messages.len() + 1);
                next_messages.push(system);
                next_messages.extend(compacted.messages);
                install_compacted_replay_frame(
                    &mut next_messages,
                    replay_cold_prefix_messages,
                    replay_interleaved_user_boundary,
                )?;
                let compacted_estimate =
                    crate::estimate_wire_context(&next_messages, &tool_definitions)
                        .map_err(|error| VegaError::Context(error.into()))?;
                let post_decision = InputDecision {
                    estimate: compacted_estimate,
                    source: ContextAccountingSource::Estimated,
                    baseline: None,
                    incremental: compacted_estimate.input_tokens,
                    predicted: compacted_estimate.input_tokens,
                    revision: primary_revision,
                    covered_messages: 0,
                };
                emit!(
                    events,
                    sink,
                    accounting_event(
                        post_decision,
                        budget,
                        ContextAccountingStage::PostSummaryTarget
                    )?
                );
                let target_tokens = budget
                    .target_tokens()
                    .map_err(|error| VegaError::Context(error.into()))?;
                if (force_provider_compaction
                    && compacted_estimate.input_tokens >= estimate.input_tokens)
                    || compacted_estimate.input_tokens > target_tokens
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
                if force_provider_compaction && next_messages == messages {
                    return Err(VegaError::Context(ContextRuntimeError::NoCompactablePrefix));
                }
                force_provider_compaction = false;
                automatic_compaction_failures = 0;
                messages = next_messages;
                recovery = progress_recovery::ProgressRecovery::default();
                discard_reasoning_events(&mut events, 0);
                emit!(events, sink, RuntimeEvent::ReasoningRetentionReleased);
                if let Some(run) = &tool_config.computer_use {
                    run.invalidate_observations();
                    run.trim_messages(&mut messages);
                }
                drop(input_anchor.take());
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
        if !skill_authority_current(skill_config.as_ref()) {
            cancel.cancel();
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
        let call_started_utc_seconds = unix_utc_seconds();
        let mut usage_seen = false;
        let mut valid_primary_input = None;
        let mut anchor_protocol_valid = true;
        let mut reasoning_turn_bytes = 0usize;
        let mut retained_reasoning_bytes = 0usize;
        if let Some(run) = &tool_config.computer_use
            && run.wait_ready(&cancel).await.is_err()
        {
            cancel.cancel();
        }
        let response_text_start = final_text.len();
        let mut response_last_visible = None;
        discard_reasoning_events(&mut events, 0);
        let reasoning_event_start = events.len();
        emit!(
            events,
            sink,
            RuntimeEvent::AssistantResponseStarted {
                text_start_bytes: response_text_start
            }
        );
        let attempt_id = ulid::Ulid::generate().to_string();
        let attempt_started_at = std::time::Instant::now();
        let mut diagnostic_metrics = RuntimeDiagnosticMetrics::default();
        emit!(
            events,
            sink,
            primary_attempt_event(
                &attempt_id,
                RuntimeDiagnosticState::Started,
                None,
                attempt_started_at,
                &diagnostic_metrics,
                None,
            )
        );
        let computer_use_epoch = if let Some(run) = &tool_config.computer_use {
            loop {
                if run.wait_ready(&cancel).await.is_err() {
                    cancel.cancel();
                }
                if cancel.is_cancelled() {
                    run.trim_messages(&mut messages);
                    break None;
                }
                let Some(epoch) = run.ready_model_epoch() else {
                    continue;
                };
                run.trim_messages(&mut messages);
                if run.ready_model_epoch() == Some(epoch) {
                    break Some(epoch);
                }
            }
        } else {
            None
        };
        let chat_request = ChatRequest {
            model: request.model.clone(),
            messages: crate::project_model_messages(&messages)?,
            tools: tool_definitions,
            max_tokens: request_output_cap(request.max_tokens, context_budget),
            reasoning: request.reasoning.clone(),
        };
        let tools_requested = !chat_request.tools.is_empty();
        primary_revision = primary_revision.checked_add(1).ok_or(VegaError::Context(
            ContextRuntimeError::Estimate(crate::ContextEstimateError::Overflow),
        ))?;
        let pending_anchor = if context_budget.is_some() {
            PendingInputAnchor::from_request(&chat_request)
                .map_err(|error| VegaError::Context(error.into()))?
        } else {
            None
        };
        if let Some(run) = &tool_config.computer_use {
            run.consume_model_batch(&mut messages);
        }
        let call = match if cancel.is_cancelled() {
            Err(VegaError::Cancelled)
        } else {
            if let Some(hook) = &tool_config.automation {
                hook.provider_started();
            }
            provider
                .chat_stream_recovering(chat_request, cancel.clone())
                .await
        } {
            Ok(call) => call,
            Err(VegaError::Cancelled) => {
                emit!(
                    events,
                    sink,
                    primary_attempt_event(
                        &attempt_id,
                        RuntimeDiagnosticState::Cancelled,
                        None,
                        attempt_started_at,
                        &diagnostic_metrics,
                        None,
                    )
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
            Err(error) => {
                emit!(
                    events,
                    sink,
                    primary_attempt_event(
                        &attempt_id,
                        RuntimeDiagnosticState::Failed,
                        Some(diagnostic_failure(&error)),
                        attempt_started_at,
                        &diagnostic_metrics,
                        Some(&error),
                    )
                );
                if !provider_overflow_recovered
                    && error.context_input_overflow().is_some()
                    && context_budget.is_some_and(|budget| budget.automatic_compaction())
                    && external_context_hook
                        .or(request_context_hook.as_deref())
                        .is_some()
                {
                    provider_overflow_recovered = true;
                    force_provider_compaction = true;
                    attempted_context_source = None;
                    drop(input_anchor.take());
                    turn_index = turn_index.saturating_sub(1);
                    continue 'primary;
                }
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
        diagnostic_metrics.provider = call.metadata;
        let mut stream = call.events;

        let mut assistant_text = String::new();
        let mut response_reasoning = Vec::new();
        let mut reasoning_presence = None;
        let mut response_layout = None;
        let mut calls = Vec::new();
        let mut stop_reason = None;
        loop {
            let next = tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    emit!(
                        events,
                        sink,
                        primary_attempt_event(
                            &attempt_id,
                            RuntimeDiagnosticState::Cancelled,
                            None,
                            attempt_started_at,
                            &diagnostic_metrics,
                            None,
                        )
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
                next = stream.next() => next,
            };
            let Some(item) = next else { break };
            if stop_reason.is_some() {
                anchor_protocol_valid = false;
            }
            let is_summary = matches!(&item, Ok(ProviderEvent::SummaryDelta(_)));
            match item {
                Ok(ProviderEvent::ResponseMetadata(metadata)) => {
                    diagnostic_metrics.provider = metadata;
                }
                Ok(ProviderEvent::RetryScheduled {
                    attempt,
                    max_retries,
                }) => {
                    if let Some(hook) = &tool_config.automation {
                        if !usage_seen {
                            hook.missing_usage();
                        }
                        hook.provider_started();
                    }
                    discard_reasoning_events(&mut events, reasoning_event_start);
                    final_text.truncate(response_text_start);
                    assistant_text.clear();
                    response_last_visible = None;
                    response_reasoning.clear();
                    reasoning_presence = None;
                    response_layout = None;
                    calls.clear();
                    stop_reason = None;
                    usage_seen = false;
                    valid_primary_input = None;
                    anchor_protocol_valid = true;
                    reasoning_turn_bytes = 0;
                    retained_reasoning_bytes = 0;
                    diagnostic_metrics = RuntimeDiagnosticMetrics::default();
                    diagnostic_metrics.provider.retry_count = Some(attempt);
                    emit!(
                        events,
                        sink,
                        RuntimeEvent::ResponseRetrying {
                            text_start_bytes: response_text_start,
                            attempt,
                            max_retries,
                        }
                    );
                }
                Ok(ProviderEvent::TextDelta(delta)) => {
                    if delta.chars().any(|character| !character.is_whitespace()) {
                        response_last_visible = Some(tokio::time::Instant::now());
                    }
                    diagnostic_metrics.visible_output_bytes = Some(
                        diagnostic_metrics
                            .visible_output_bytes
                            .unwrap_or_default()
                            .saturating_add(delta.len() as u64),
                    );
                    assistant_text.push_str(&delta);
                    final_text.push_str(&delta);
                    emit!(events, sink, RuntimeEvent::TextDelta(delta));
                }
                Ok(ProviderEvent::ReasoningReplay(items)) => {
                    let error = if !response_reasoning.is_empty() {
                        Some(VegaError::ProviderDiagnostic {
                            kind: crate::ProviderFailureKind::Protocol,
                            status: None,
                            message: "duplicate reasoning replay in one provider call".into(),
                            retryable: false,
                            retry_count: None,
                            request_id: None,
                        })
                    } else {
                        None
                    };
                    if let Some(error) = error {
                        emit!(
                            events,
                            sink,
                            primary_attempt_event(
                                &attempt_id,
                                RuntimeDiagnosticState::Failed,
                                Some(diagnostic_failure(&error)),
                                attempt_started_at,
                                &diagnostic_metrics,
                                Some(&error)
                            )
                        );
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
                    response_reasoning = items;
                }
                Ok(ProviderEvent::ChatReasoningContent(presence)) => {
                    if reasoning_presence.replace(presence).is_some() {
                        let error = VegaError::ProviderDiagnostic {
                            kind: crate::ProviderFailureKind::Protocol,
                            status: None,
                            message: "duplicate chat reasoning state in one provider call".into(),
                            retryable: false,
                            retry_count: None,
                            request_id: None,
                        };
                        emit!(
                            events,
                            sink,
                            primary_attempt_event(
                                &attempt_id,
                                RuntimeDiagnosticState::Failed,
                                Some(diagnostic_failure(&error)),
                                attempt_started_at,
                                &diagnostic_metrics,
                                Some(&error)
                            )
                        );
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
                Ok(ProviderEvent::ResponseReplay(layout)) => {
                    if response_layout.replace(layout).is_some() {
                        let error = VegaError::ProviderDiagnostic {
                            kind: crate::ProviderFailureKind::Protocol,
                            status: None,
                            message: "duplicate response replay layout in one provider call".into(),
                            retryable: false,
                            retry_count: None,
                            request_id: None,
                        };
                        emit!(
                            events,
                            sink,
                            primary_attempt_event(
                                &attempt_id,
                                RuntimeDiagnosticState::Failed,
                                Some(diagnostic_failure(&error)),
                                attempt_started_at,
                                &diagnostic_metrics,
                                Some(&error)
                            )
                        );
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
                Ok(ProviderEvent::ThinkingDelta(delta) | ProviderEvent::SummaryDelta(delta)) => {
                    reasoning_turn_bytes = reasoning_turn_bytes.saturating_add(delta.len());
                    let event = if is_summary {
                        RuntimeEvent::SummaryDelta(delta)
                    } else {
                        RuntimeEvent::ThinkingDelta(delta)
                    };
                    sink(event.clone()).await?;
                    retain_reasoning_event(&mut events, event, &mut retained_reasoning_bytes);
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
                        let error = VegaError::Provider {
                            status: None,
                            message: "usage event after terminal done".to_string(),
                            retryable: false,
                        };
                        emit!(
                            events,
                            sink,
                            primary_attempt_event(
                                &attempt_id,
                                RuntimeDiagnosticState::Failed,
                                Some(RuntimeDiagnosticFailure::UnknownSafeFailure),
                                attempt_started_at,
                                &diagnostic_metrics,
                                Some(&error),
                            )
                        );
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
                    if usage_seen {
                        let error = VegaError::Provider {
                            status: None,
                            message: "duplicate usage event in one provider call".to_string(),
                            retryable: false,
                        };
                        emit!(
                            events,
                            sink,
                            primary_attempt_event(
                                &attempt_id,
                                RuntimeDiagnosticState::Failed,
                                Some(RuntimeDiagnosticFailure::UnknownSafeFailure),
                                attempt_started_at,
                                &diagnostic_metrics,
                                Some(&error),
                            )
                        );
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
                    diagnostic_metrics.input_tokens = Some(input);
                    diagnostic_metrics.output_tokens = Some(output);
                    diagnostic_metrics.cache_read_tokens = cache_read;
                    diagnostic_metrics.cache_write_tokens = cache_write;
                    usage_seen = true;
                    valid_primary_input = (input > 0
                        && cache_read.is_none_or(|tokens| tokens <= input)
                        && cache_write.is_none_or(|tokens| tokens <= input))
                    .then_some(input);
                    let usage = RuntimeTokenUsage {
                        input,
                        output,
                        cache_read,
                        cache_write,
                    };
                    let quote = request.pricing_catalog.as_ref().map(|catalog| {
                        catalog.quote_with_optional_caches(
                            &request.model,
                            usage.input,
                            usage.output,
                            usage.cache_read,
                            usage.cache_write,
                            call_started_utc_seconds,
                        )
                    });
                    let (cost_microcents, pricing) = match quote {
                        Some(Ok(Some(quote))) => (
                            quote.cost_microcents,
                            Some(RuntimeUsagePricing {
                                version: quote.pricing_version.to_string(),
                                profile: match quote.profile {
                                    PricingProfile::Base => "base".to_string(),
                                    PricingProfile::PeakUtcWeekly => "peak_utc_weekly".to_string(),
                                },
                                call_started_at: call_started_utc_seconds,
                            }),
                        ),
                        Some(Ok(None))
                        | Some(Err(vega_token::PricingError::ModelNotFound { .. }))
                        | None => (0, None),
                        Some(Err(error)) => {
                            let error = VegaError::Provider {
                                status: None,
                                message: format!("usage pricing failed: {error}"),
                                retryable: false,
                            };
                            emit!(
                                events,
                                sink,
                                primary_attempt_event(
                                    &attempt_id,
                                    RuntimeDiagnosticState::Failed,
                                    Some(RuntimeDiagnosticFailure::UnknownSafeFailure),
                                    attempt_started_at,
                                    &diagnostic_metrics,
                                    Some(&error),
                                )
                            );
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
                    emit!(
                        events,
                        sink,
                        RuntimeEvent::UsageUpdated {
                            usage,
                            cost_microcents,
                            pricing,
                        }
                    );
                    if let Some(hook) = &tool_config.automation {
                        hook.provider_usage();
                    }
                }
                Ok(ProviderEvent::Done {
                    stop_reason: reason,
                }) => {
                    diagnostic_metrics.stop_reason = Some(reason);
                    stop_reason = Some(reason);
                }
                Err(VegaError::Cancelled) => {
                    emit!(
                        events,
                        sink,
                        primary_attempt_event(
                            &attempt_id,
                            RuntimeDiagnosticState::Cancelled,
                            None,
                            attempt_started_at,
                            &diagnostic_metrics,
                            None,
                        )
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
                Err(error) => {
                    emit!(
                        events,
                        sink,
                        primary_attempt_event(
                            &attempt_id,
                            RuntimeDiagnosticState::Failed,
                            Some(diagnostic_failure(&error)),
                            attempt_started_at,
                            &diagnostic_metrics,
                            Some(&error),
                        )
                    );
                    if !provider_overflow_recovered
                        && error.context_input_overflow().is_some()
                        && context_budget.is_some_and(|budget| budget.automatic_compaction())
                        && external_context_hook
                            .or(request_context_hook.as_deref())
                            .is_some()
                        && assistant_text.is_empty()
                        && response_reasoning.is_empty()
                        && calls.is_empty()
                        && final_text.len() == response_text_start
                        && reasoning_turn_bytes == 0
                    {
                        provider_overflow_recovered = true;
                        force_provider_compaction = true;
                        attempted_context_source = None;
                        drop(input_anchor.take());
                        turn_index = turn_index.saturating_sub(1);
                        continue 'primary;
                    }
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

        if !skill_authority_current(skill_config.as_ref()) {
            cancel.cancel();
        }
        if cancel.is_cancelled() {
            emit!(
                events,
                sink,
                primary_attempt_event(
                    &attempt_id,
                    RuntimeDiagnosticState::Cancelled,
                    None,
                    attempt_started_at,
                    &diagnostic_metrics,
                    None,
                )
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
        if let Some(run) = &tool_config.computer_use {
            if run.wait_ready(&cancel).await.is_err() {
                cancel.cancel();
            }
            run.trim_messages(&mut messages);
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
        let reasoning_presence = reasoning_presence.unwrap_or_default();
        if stop_reason.is_some()
            && let Err(error) = validate_replay_response(
                request.reasoning.as_ref(),
                tools_requested,
                &calls,
                &reasoning_presence,
                &response_reasoning,
                response_layout.as_ref(),
                &assistant_text,
            )
        {
            emit!(
                events,
                sink,
                primary_attempt_event(
                    &attempt_id,
                    RuntimeDiagnosticState::Failed,
                    Some(diagnostic_failure(&error)),
                    attempt_started_at,
                    &diagnostic_metrics,
                    Some(&error)
                )
            );
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
        provider_overflow_recovered = false;
        let normal_terminal = matches!(
            (stop_reason, calls.is_empty()),
            (Some(StopReason::End), true) | (Some(StopReason::ToolUse), false)
        );
        if !normal_terminal {
            messages[0].mark_replay_unsealable();
        }
        emit!(
            events,
            sink,
            primary_attempt_event(
                &attempt_id,
                RuntimeDiagnosticState::Succeeded,
                None,
                attempt_started_at,
                &diagnostic_metrics,
                None,
            )
        );
        input_anchor = if anchor_protocol_valid && normal_terminal {
            pending_anchor
                .and_then(|pending| valid_primary_input.and_then(|input| pending.complete(input)))
        } else {
            None
        };
        progress.observe_visible(response_last_visible);
        if !calls.is_empty() {
            emit!(
                events,
                sink,
                RuntimeEvent::ToolRoundCompleted {
                    response_id: attempt_id.clone(),
                    call_ids: calls.iter().map(|call| call.id.clone()).collect(),
                }
            );
        }
        if !usage_seen && let Some(hook) = &tool_config.automation {
            hook.missing_usage();
        }
        let response_kind = if !calls.is_empty() {
            Some(RuntimeAssistantResponseKind::Commentary)
        } else if stop_reason == Some(StopReason::End)
            && anchor_protocol_valid
            && !assistant_text.trim().is_empty()
        {
            Some(RuntimeAssistantResponseKind::Final)
        } else {
            None
        };
        if final_text.len() > response_text_start
            && let Some(kind) = response_kind
        {
            emit!(
                events,
                sink,
                RuntimeEvent::AssistantResponseCompleted {
                    response_id: attempt_id.clone(),
                    text_start_bytes: response_text_start,
                    text_end_bytes: final_text.len(),
                    kind,
                }
            );
        }
        if calls.is_empty() {
            let replay_policy = request
                .reasoning
                .as_ref()
                .map(|profile| profile.replay_policy)
                .unwrap_or(crate::ReasoningReplayPolicy::Unknown);
            if !replay_policy.preserves_chat_response(tools_requested)
                && (!matches!(&reasoning_presence, crate::ChatReasoningPresence::Absent)
                    || !response_reasoning.is_empty()
                    || response_layout.is_some())
            {
                messages[0].mark_replay_unsealable();
            }
            let mut assistant = ChatMessage::new(ChatRole::Assistant, assistant_text);
            if request.reasoning.as_ref().is_some_and(|profile| {
                profile
                    .replay_policy
                    .preserves_chat_response(tools_requested)
            }) {
                assistant.reasoning_content = match reasoning_presence {
                    crate::ChatReasoningPresence::Absent => None,
                    crate::ChatReasoningPresence::Empty => Some(String::new()),
                    crate::ChatReasoningPresence::NonEmpty(text) => Some(text),
                };
            }
            if request.reasoning.as_ref().is_some_and(|profile| {
                profile.replay_policy == crate::ReasoningReplayPolicy::ResponsesItems
            }) {
                assistant.response_reasoning = response_reasoning;
                assistant.response_replay_layout = response_layout;
            }
            messages.push(assistant);
            let finish = match stop_reason.unwrap_or(StopReason::End) {
                StopReason::Length => RuntimeFinishReason::Length,
                StopReason::End | StopReason::ToolUse | StopReason::Unknown => {
                    RuntimeFinishReason::End
                }
            };
            if finish == RuntimeFinishReason::End
                && (tool_config.turn_limit == 0 || turn_index < tool_config.turn_limit)
                && let Some(source) = &tool_config.steering_source
                && let Some(input) = source.take().await?
            {
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
                crate::images::validate_messages(std::slice::from_ref(&input.message))?;
                emit!(events, sink, RuntimeEvent::SteeringAccepted(input.clone()));
                progress = progress::ProgressReporter::new();
                recovery = progress_recovery::ProgressRecovery::default();
                context_operation_id = input.assistant_message_id.clone();
                let interleaved_user_boundary = messages.len();
                messages[0].begin_interleaved_user_round(interleaved_user_boundary);
                messages.push(input.message);
                final_text.clear();
                input_anchor = None;
                live_projection_revision = live_projection_revision.saturating_add(1);
                continue;
            }
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

        let mixed_question_batch =
            calls.len() != 1 && calls.iter().any(|call| call.name == USER_QUESTION_TOOL);
        let batch_policy = classify_tool_batch(
            &calls
                .iter()
                .map(|call| call.name.as_str())
                .collect::<Vec<_>>(),
        );
        let original_calls = calls
            .iter()
            .map(|call| crate::ChatToolCall {
                id: call.id.clone(),
                name: call.name.clone(),
                input_json: call.input_json.clone(),
            })
            .collect::<Vec<_>>();
        let mut prepared_calls = Vec::with_capacity(calls.len());
        for call in calls {
            // Compare completed mutations without re-reading today's target.
            let prior_mutation = completed
                .get(&call.id)
                .filter(|_| matches!(call.name.as_str(), "Write" | "Edit" | "write" | "edit"));
            let prepared = if mixed_question_batch {
                prepare_mixed_rejection_call(tools, &tool_config, &capabilities, call)
            } else if let Some(prior) = prior_mutation {
                prepare_completed_mutation(tools, &tool_config, &capabilities, call, prior)
            } else if batch_policy == BatchPolicy::RejectOtherTools
                && call.name != crate::skills::LOAD_SKILL_TOOL_NAME
            {
                prepare_mixed_rejection_call(tools, &tool_config, &capabilities, call)
            } else {
                prepare_runtime_call(tools, &tool_config, &capabilities, call)
            };
            match prepared {
                Ok(mut prepared) => {
                    prepared.bind_computer_use_epoch(computer_use_epoch);
                    prepared_calls.push(prepared);
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
        let wire_calls = prepared_calls
            .iter()
            .map(PreparedRuntimeCall::audit_call)
            .collect();
        let mut assistant_message = ChatMessage::assistant_with_tools(assistant_text, wire_calls);
        if request.reasoning.as_ref().is_some_and(|profile| {
            profile
                .replay_policy
                .preserves_chat_response(tools_requested)
        }) {
            assistant_message.reasoning_content = match reasoning_presence {
                crate::ChatReasoningPresence::Absent => None,
                crate::ChatReasoningPresence::Empty => Some(String::new()),
                crate::ChatReasoningPresence::NonEmpty(text) => Some(text),
            };
        }
        if request.reasoning.as_ref().is_some_and(|profile| {
            profile.replay_policy == crate::ReasoningReplayPolicy::ResponsesItems
        }) {
            assistant_message.response_reasoning = response_reasoning;
            assistant_message.response_replay_layout = response_layout;
        }
        let model_calls = prepared_calls
            .iter()
            .zip(original_calls)
            .map(|(prepared, original)| {
                prepared.model_call().and_then(|call| {
                    tool_config
                        .redact_tool_output(&call.input_json)
                        .is_ok_and(|redacted| redacted == call.input_json)
                        .then_some(original)
                        .filter(|original| {
                            tool_config
                                .redact_tool_output(&original.input_json)
                                .is_ok_and(|redacted| redacted == original.input_json)
                        })
                })
            })
            .collect();
        assistant_message.bind_live_tool_calls(model_calls)?;
        messages.push(assistant_message);

        if batch_policy == BatchPolicy::RejectOtherTools {
            // A provider can emit operational calls before a load in the
            // same response. Resolve all loads first, then deny every other
            // member without entering its dispatcher or permission prompt.
            prepared_calls.sort_by_key(|prepared| {
                prepared.call().name != crate::skills::LOAD_SKILL_TOOL_NAME
            });
        }

        if let Some(run) = &tool_config.computer_use {
            run.begin_model_batch(
                &attempt_id,
                prepared_calls
                    .iter()
                    .all(|prepared| prepared.call().name == "ComputerScreenshot"),
            );
        }
        for prepared in prepared_calls {
            let call = prepared.call().clone();
            // `tool_call_count` is a telemetry/result field only. The run is
            // bounded by `tool_config.turn_limit` (provider round-trips), not
            // by a per-tool-call count, matching Claude Code / Codex.
            tool_call_count += 1;
            if tool_config.foreign_call_ids.contains(&call.id) {
                let conflict = conflict_result(&call);
                recovery.observe(&call, &conflict);
                emit!(
                    events,
                    sink,
                    RuntimeEvent::ToolCallConflict {
                        call: call.clone(),
                        result: conflict.clone(),
                    }
                );
                messages.push(ChatMessage::runtime_tool_result(&call, &conflict));
                continue;
            }
            let validation_output = match &prepared {
                PreparedRuntimeCall::InvalidWriteEdit { result, .. } => Some(result.clone()),
                PreparedRuntimeCall::InvalidBash { call } => {
                    InvalidBashAudit::from_json(&call.input_json).map(|audit| audit.feedback())
                }
                _ => None,
            };
            if let Some(validation_output) = validation_output {
                let mut terminal = terminal_result(
                    &call,
                    validation_output.to_string(),
                    RuntimeToolStatus::Rejected,
                    Some(validation_audit()),
                );
                if let Some(prior) = completed.get(&call.id).cloned() {
                    if !tool_names_equivalent(&prior.tool, &call.name)
                        || !runtime_inputs_semantically_equal(
                            &call.name,
                            &prior.input_json,
                            &call.input_json,
                        )
                    {
                        let conflict = conflict_result(&call);
                        recovery.observe(&call, &conflict);
                        emit!(
                            events,
                            sink,
                            RuntimeEvent::ToolCallConflict {
                                call: call.clone(),
                                result: conflict.clone(),
                            }
                        );
                        messages.push(ChatMessage::runtime_tool_result(&call, &conflict));
                        continue;
                    }
                    terminal = prior.result;
                    terminal.reused = true;
                    terminal.truncated = None;
                }
                redact_runtime_tool_result(&tool_config, &mut terminal);
                recovery.observe(&call, &terminal);
                emit!(
                    events,
                    sink,
                    RuntimeEvent::ToolCallValidationRejected {
                        call: call.clone(),
                        result: terminal.clone(),
                    }
                );
                messages.push(ChatMessage::runtime_tool_result(&call, &terminal));
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
            if mixed_question_batch {
                let result = if completed.contains_key(&call.id) {
                    let result = conflict_result(&call);
                    emit!(
                        events,
                        sink,
                        RuntimeEvent::ToolCallConflict {
                            call: call.clone(),
                            result: result.clone()
                        }
                    );
                    result
                } else {
                    emit!(events, sink, RuntimeEvent::ToolCallProposed(call.clone()));
                    let result = terminal_result(
                        &call,
                        USER_QUESTION_BATCH_REJECTED.into(),
                        RuntimeToolStatus::Rejected,
                        Some(validation_audit()),
                    );
                    emit!(events, sink, RuntimeEvent::ToolCallFinished(result.clone()));
                    completed.insert(
                        call.id.clone(),
                        CompletedToolCall {
                            tool: call.name.clone(),
                            input_json: call.input_json.clone(),
                            result: result.clone(),
                        },
                    );
                    result
                };
                recovery.observe(&call, &result);
                messages.push(ChatMessage::runtime_tool_result(&call, &result));
                continue;
            }
            if let Some(prior) = completed.get(&call.id).cloned() {
                if matches!(
                    call.name.as_str(),
                    crate::skills::LOAD_SKILL_TOOL_NAME
                        | crate::skills::READ_SKILL_RESOURCE_TOOL_NAME
                ) {
                    // A durable receipt alone is insufficient to replay a
                    // Skill call: the frozen body/reference must be restored
                    // under its separate trusted run binding first.
                    return Err(skill_runtime_error());
                }
                if !tool_names_equivalent(&prior.tool, &call.name)
                    || !runtime_inputs_semantically_equal(
                        &call.name,
                        &prior.input_json,
                        &call.input_json,
                    )
                {
                    let conflict = conflict_result(&call);
                    recovery.observe(&call, &conflict);
                    emit!(
                        events,
                        sink,
                        RuntimeEvent::ToolCallConflict {
                            call: call.clone(),
                            result: conflict.clone(),
                        }
                    );
                    messages.push(ChatMessage::runtime_tool_result(&call, &conflict));
                    continue;
                }
                emit!(events, sink, RuntimeEvent::ToolCallProposed(call.clone()));
                let mut result = prior.result;
                result.reused = true;
                result.truncated = None;
                redact_runtime_tool_result(&tool_config, &mut result);
                user_questions::validate_redacted_result(&call, &mut result);
                recovery.observe(&call, &result);
                emit!(
                    events,
                    sink,
                    RuntimeEvent::ToolCallOutput {
                        call_id: call.id.clone(),
                        chunk: result.output.clone(),
                    }
                );
                emit!(events, sink, RuntimeEvent::ToolCallFinished(result.clone()));
                messages.push(ChatMessage::runtime_tool_result(&call, &result));
                continue;
            }
            if batch_policy == BatchPolicy::RejectOtherTools
                && call.name != crate::skills::LOAD_SKILL_TOOL_NAME
            {
                emit!(events, sink, RuntimeEvent::ToolCallProposed(call.clone()));
                let result = terminal_result(
                    &call,
                    "Tool error: mixed Skill load batch rejected other tools".to_string(),
                    RuntimeToolStatus::Rejected,
                    Some(validation_audit()),
                );
                let mut result = result;
                redact_runtime_tool_result(&tool_config, &mut result);
                user_questions::validate_redacted_result(&call, &mut result);
                recovery.observe(&call, &result);
                emit!(events, sink, RuntimeEvent::ToolCallFinished(result.clone()));
                messages.push(ChatMessage::runtime_tool_result(&call, &result));
                completed.insert(
                    call.id.clone(),
                    CompletedToolCall {
                        tool: call.name,
                        input_json: call.input_json,
                        result,
                    },
                );
                continue;
            }
            if !skill_authority_current(skill_config.as_ref()) {
                cancel.cancel();
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
            let cua_authorization = if matches!(&prepared, PreparedRuntimeCall::ComputerUse { .. })
            {
                let authorization = authorize_call(
                    &prepared,
                    &tool_config,
                    &exact_rules,
                    permission_hook,
                    &cancel,
                )
                .await?;
                if matches!(&authorization, Authorization::Terminal(result) if result.status == RuntimeToolStatus::Cancelled && result.output == crate::computer_use::SUPERSEDED_CONTROL_OUTPUT)
                {
                    if let Authorization::Terminal(result) = &authorization {
                        recovery.observe(&call, result);
                    }
                    tool_call_count -= 1;
                    messages.push(ChatMessage::tool_result(
                        &call.id,
                        crate::computer_use::SUPERSEDED_CONTROL_OUTPUT,
                    ));
                    if let Some(run) = &tool_config.computer_use {
                        run.trim_messages(&mut messages);
                    }
                    continue;
                }
                Some(authorization)
            } else {
                None
            };
            emit!(events, sink, RuntimeEvent::ToolCallProposed(call.clone()));

            let authorization = if let Some(authorization) = cua_authorization {
                authorization
            } else {
                authorize_call(
                    &prepared,
                    &tool_config,
                    &exact_rules,
                    permission_hook,
                    &cancel,
                )
                .await?
            };
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
                    } else if prepared.superseded_after_wait(&cancel).await {
                        (
                            terminal_result(
                                &call,
                                crate::computer_use::SUPERSEDED_CONTROL_OUTPUT.into(),
                                RuntimeToolStatus::Cancelled,
                                Some(audit),
                            ),
                            false,
                        )
                    } else if tool_config.computer_use.as_ref().is_some_and(|run| {
                        prepared.model_call().is_some_and(|model_call| {
                            run.defer_model_read(
                                &model_call.name,
                                &model_call.input_json,
                                &mut messages,
                            )
                        })
                    }) {
                        (
                            terminal_result(
                                &call,
                                crate::computer_use::ComputerUseError::ObservationBudget {
                                    executed: false,
                                }
                                .feedback(),
                                RuntimeToolStatus::Failed,
                                Some(audit),
                            ),
                            false,
                        )
                    } else {
                        executed_tool_call_count += 1;
                        let (mut result, cancelled) = match prepared {
                            PreparedRuntimeCall::Skill { action, .. } => {
                                let config =
                                    skill_config.as_ref().ok_or_else(skill_runtime_error)?;
                                let mut executed = execute_skill_call(
                                    config,
                                    &call,
                                    action.clone(),
                                    &base_system_prompt,
                                    &messages,
                                    capabilities.definitions(),
                                    context_budget,
                                    input_anchor.as_ref(),
                                    &request.model,
                                    request.reasoning.as_ref(),
                                    request_output_cap(request.max_tokens, context_budget),
                                    primary_revision,
                                    direct_user_skill_round,
                                )?;
                                if let Some(prospective) = executed.over_budget_envelope.as_deref()
                                {
                                    let pending_result = match &action {
                                        SkillToolAction::Load { name } => Some(ChatMessage::tool_result(
                                            &call.id,
                                            serde_json::json!({"name": name, "status": "loaded"}).to_string(),
                                        )),
                                        SkillToolAction::Read { .. } => None,
                                    };
                                    let hook =
                                        external_context_hook.or(request_context_hook.as_deref());
                                    if compact_before_skill_rejection(
                                        tool_config.computer_use.as_ref(),
                                        prospective,
                                        &base_system_prompt,
                                        &skill_catalog,
                                        &mut messages,
                                        pending_result.as_ref(),
                                        capabilities.definitions(),
                                        context_budget,
                                        input_anchor.as_ref(),
                                        &request.model,
                                        request.reasoning.as_ref(),
                                        request_output_cap(request.max_tokens, context_budget),
                                        primary_revision,
                                        hook,
                                        &context_operation_id,
                                        &mut context_source_version,
                                        &mut context_source_fingerprint,
                                        &mut live_projection_revision,
                                        &mut attempted_context_source,
                                        &mut automatic_compaction_failures,
                                        &cancel,
                                        &mut events,
                                        &mut sink,
                                    )
                                    .await?
                                    {
                                        input_anchor = None;
                                        recovery = progress_recovery::ProgressRecovery::default();
                                        executed = execute_skill_call(
                                            config,
                                            &call,
                                            action,
                                            &base_system_prompt,
                                            &messages,
                                            capabilities.definitions(),
                                            context_budget,
                                            input_anchor.as_ref(),
                                            &request.model,
                                            request.reasoning.as_ref(),
                                            request_output_cap(request.max_tokens, context_budget),
                                            primary_revision,
                                            direct_user_skill_round,
                                        )?;
                                    }
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
                                if let Some(event) = executed.state_event {
                                    emit!(events, sink, event);
                                }
                                (executed.result, false)
                            }
                            other => execute_prepared_waiting(other, tools, &cancel).await,
                        };
                        if result.status == RuntimeToolStatus::Cancelled
                            && result.output == crate::computer_use::SUPERSEDED_CONTROL_OUTPUT
                        {
                            executed_tool_call_count -= 1;
                        }
                        result.approval = Some(audit);
                        (result, cancelled)
                    }
                }
            };
            if cancelled_while_running
                || (call.name == "ComputerScreenshot"
                    && result.status == RuntimeToolStatus::Success
                    && cancel.is_cancelled())
            {
                result.status = RuntimeToolStatus::Cancelled;
            }
            let mut project_observation = result.status == RuntimeToolStatus::Success;
            if project_observation
                && !cancel.is_cancelled()
                && let Some(run) = &tool_config.computer_use
                && let Err(error) =
                    run.stage_model_observation(&attempt_id, &call.id, &mut messages)
            {
                project_observation = false;
                if let Some(receipt) = run.limited_screenshot_receipt(&call.id, error) {
                    result.output = receipt;
                } else {
                    result.status = RuntimeToolStatus::Failed;
                    result.output = error.feedback();
                    run.discard_pending_screenshot(&call.id);
                }
            }
            redact_runtime_tool_result(&tool_config, &mut result);
            user_questions::validate_redacted_result(&call, &mut result);
            recovery.observe(&call, &result);
            if call.name == "ComputerScreenshot"
                && result.status == RuntimeToolStatus::Success
                && !cancel.is_cancelled()
                && let Some(run) = &tool_config.computer_use
                && let Some((image, epoch)) = run.screenshot(&call.id)
            {
                emit!(
                    events,
                    sink,
                    RuntimeEvent::ScreenshotDelivered {
                        call_id: call.id.clone(),
                        image,
                        control: run.control.clone(),
                        epoch,
                        cancel: cancel.clone(),
                    }
                );
                run.acknowledge_screenshot(&call.id, epoch)
                    .map_err(|_| VegaError::Tool {
                        tool: "ComputerScreenshot".into(),
                        message: "screenshot delivery acknowledgement failed".into(),
                    })?;
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
            let mut model_result = ChatMessage::runtime_tool_result(&call, &result);
            if project_observation
                && result.status == RuntimeToolStatus::Success
                && let Some(run) = &tool_config.computer_use
            {
                run.apply_observation(&call.id, &mut model_result);
            }
            messages.push(model_result);
            if let Some(run) = &tool_config.computer_use {
                run.trim_messages(&mut messages);
            }
            completed.insert(
                call.id,
                CompletedToolCall {
                    tool: call.name,
                    input_json: call.input_json,
                    result,
                },
            );

            if !cancelled_while_running
                && !cancel.is_cancelled()
                && tool_config
                    .computer_use
                    .as_ref()
                    .is_some_and(|run| run.no_progress())
            {
                let message = if tool_config
                    .computer_use
                    .as_ref()
                    .is_some_and(|run| run.has_delivered_screenshots())
                {
                    "已有截图保留在对话中。后续电脑操作连续未取得进展，已停止重复操作。请根据最后一个工具结果处理尚未完成的工作。"
                } else {
                    "电脑操作连续未取得进展，已停止本轮自动操作。请根据对话中的错误修正目标或参数后重新发起；不会继续重复点击或换用脚本。"
                };
                final_text.push_str(message);
                emit!(events, sink, RuntimeEvent::TextDelta(message.into()));
                emit!(
                    events,
                    sink,
                    RuntimeEvent::Finished(RuntimeFinishReason::End)
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
        let mut steered = false;
        if !cancel.is_cancelled()
            && (tool_config.turn_limit == 0 || turn_index < tool_config.turn_limit)
            && let Some(source) = &tool_config.steering_source
            && let Some(input) = source.take().await?
        {
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
            crate::images::validate_messages(std::slice::from_ref(&input.message))?;
            emit!(events, sink, RuntimeEvent::SteeringAccepted(input.clone()));
            progress = progress::ProgressReporter::new();
            recovery = progress_recovery::ProgressRecovery::default();
            steered = true;
            context_operation_id = input.assistant_message_id.clone();
            let interleaved_user_boundary = messages.len();
            messages[0].begin_interleaved_user_round(interleaved_user_boundary);
            messages.push(input.message);
            final_text.clear();
            input_anchor = None;
        }
        if !steered && !cancel.is_cancelled() {
            progress.after_tool_batch(messages.last_mut());
            if let Some(guidance) = recovery.finish_batch()
                && let Some(last) = messages.last_mut()
            {
                last.attach_recovery_guidance(guidance);
            }
        }
        if batch_policy != BatchPolicy::SkillOnly {
            direct_user_skill_round = false;
        }
        live_projection_revision = live_projection_revision.saturating_add(1);
    }
}

#[cfg(test)]
mod compaction_cancellation_tests {
    use super::*;

    struct CancelAtCompletion(bool);

    impl crate::ContextCompactionHook for CancelAtCompletion {
        fn compact<'a>(
            &'a self,
            _request: ContextCompactionRequest,
            cancel: CancellationToken,
        ) -> futures::future::BoxFuture<
            'a,
            Result<crate::ContextCompactionResult, crate::ContextCompactionFailure>,
        > {
            Box::pin(async move {
                cancel.cancel();
                Err(crate::ContextCompactionFailure::new(
                    if self.0 {
                        VegaError::Cancelled
                    } else {
                        VegaError::CredentialCheckUnavailable
                    },
                    None,
                ))
            })
        }
    }

    #[tokio::test]
    async fn issue397_skill_prospect_compaction_preserves_typed_failure_and_cancellation() {
        for cancelled_error in [true, false] {
            let mut messages = vec![
                ChatMessage::new(ChatRole::System, "system"),
                ChatMessage::new(ChatRole::User, "previous request"),
                ChatMessage::new(ChatRole::Assistant, "previous answer"),
                ChatMessage::new(ChatRole::User, "current request"),
            ];
            let mut events = Vec::new();
            let cancel = CancellationToken::new();
            let result = compact_before_skill_rejection(
                None,
                "skill body",
                "system",
                "catalog",
                &mut messages,
                None,
                &[],
                Some(crate::ContextBudget::new(50_000, 1000, true).unwrap()),
                None,
                "mock-model",
                None,
                None,
                0,
                Some(&CancelAtCompletion(cancelled_error)),
                "operation",
                &mut None,
                &mut None,
                &mut 0,
                &mut None,
                &mut 0,
                &cancel,
                &mut events,
                &mut |_| std::future::ready(Ok(())),
            )
            .await;
            assert!(cancel.is_cancelled());
            if cancelled_error {
                assert!(matches!(result, Err(VegaError::Cancelled)));
            } else {
                assert!(matches!(result, Err(VegaError::CredentialCheckUnavailable)));
            }
            assert!(
                matches!(events.last(), Some(RuntimeEvent::ContextCompactionStatusUpdated { status })
                if status.phase == if cancelled_error { crate::ContextCompactionPhase::Cancelled }
                else { crate::ContextCompactionPhase::Failed })
            );
        }
    }
}
