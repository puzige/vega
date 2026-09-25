use super::context_accounting::{InputAnchor, InputDecision, PendingInputAnchor};
use super::tools_exec::SkillToolAction;
use super::*;
use crate::skills::{BatchPolicy, classify_tool_batch};
use crate::{ContextCheck, ContextCompactionRequest, ContextRuntimeError};

fn request_output_cap(
    max_tokens: Option<u32>,
    budget: Option<crate::ContextBudget>,
) -> Option<u32> {
    max_tokens.map(|cap| budget.map_or(cap, |budget| cap.min(budget.output_reserve() as u32)))
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
    match error {
        VegaError::ProviderDiagnostic { kind, .. } => match kind {
            crate::ProviderFailureKind::Http => RuntimeDiagnosticFailure::ProviderHttp,
            crate::ProviderFailureKind::Transport => {
                RuntimeDiagnosticFailure::ProviderTransportOrStream
            }
            crate::ProviderFailureKind::Protocol => RuntimeDiagnosticFailure::ProviderProtocol,
            crate::ProviderFailureKind::Rejected => RuntimeDiagnosticFailure::ProviderRejected,
        },
        VegaError::Provider {
            status: Some(_), ..
        } => RuntimeDiagnosticFailure::ProviderHttp,
        VegaError::ReasoningBudgetExceeded { .. } => RuntimeDiagnosticFailure::ReasoningLimit,
        VegaError::Context(ContextRuntimeError::OverLimit { .. }) => {
            RuntimeDiagnosticFailure::ContextOverLimit
        }
        VegaError::Cancelled => RuntimeDiagnosticFailure::UnknownSafeFailure,
        _ => RuntimeDiagnosticFailure::UnknownSafeFailure,
    }
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
        *first = ChatMessage::new(ChatRole::System, system);
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
    projected[0] = ChatMessage::new(ChatRole::System, system.clone());
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
                    phase: if cancel.is_cancelled() {
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
            if cancel.is_cancelled() {
                return Ok(false);
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
    let previous_system = messages.remove(0);
    messages.clear();
    messages.push(previous_system);
    messages.extend(compacted.messages);
    if let Some(result) = pending_result
        && messages.last() == Some(result)
    {
        messages.pop();
    }
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
            let read = run.read_reference(&name, &path, |prospective| {
                // Reserve the complete content-bearing result in the next
                // primary request, including its tool-result wrapper.
                let mut projected = messages.to_vec();
                projected.push(ChatMessage::tool_result(
                    &call.id,
                    format!("[Lower-trust Skill reference]\n{prospective}"),
                ));
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
                Ok(reference) => (
                    format!(
                        "[Lower-trust Skill reference]\n{}",
                        reference.to_json().map_err(|_| skill_runtime_error())?
                    ),
                    true,
                ),
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
    let base_system_prompt = request.system_prompt.clone();
    messages.push(ChatMessage::new(
        ChatRole::System,
        base_system_prompt.clone(),
    ));
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
    let mut automatic_compaction_failures = 0_u8;
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
    .with_skills(!skill_catalog.is_empty(), skill_config.is_some())?;
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
    let mut reasoning_run_bytes = 0usize;
    let mut direct_user_skill_round = true;
    let mut input_anchor: Option<InputAnchor> = None;
    let mut primary_revision = 0_u64;

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
                        input_anchor = None;
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

    loop {
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
        messages[0] = ChatMessage::new(ChatRole::System, system_prompt.clone());
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
            let should_compact = matches!(check, ContextCheck::Triggered | ContextCheck::OverLimit)
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
                    if matches!(check, ContextCheck::OverLimit) {
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
                let mut next_messages = Vec::with_capacity(compacted.messages.len() + 1);
                next_messages.push(system);
                next_messages.extend(compacted.messages);
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
                automatic_compaction_failures = 0;
                messages = next_messages;
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
        let chat_request = ChatRequest {
            model: request.model.clone(),
            messages: messages.clone(),
            tools: tool_definitions,
            // The reserved output capacity protects the input budget; it is
            // not a request to generate that many tokens on every round.
            // Keep provider defaults when no explicit generation cap exists.
            max_tokens: request_output_cap(request.max_tokens, context_budget),
            reasoning: request.reasoning.clone(),
        };
        primary_revision = primary_revision.checked_add(1).ok_or(VegaError::Context(
            ContextRuntimeError::Estimate(crate::ContextEstimateError::Overflow),
        ))?;
        let pending_anchor = if context_budget.is_some() {
            PendingInputAnchor::from_request(&chat_request)
                .map_err(|error| VegaError::Context(error.into()))?
        } else {
            None
        };
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
        let call = match provider
            .chat_stream_with_metadata(chat_request, cancel.clone())
            .await
        {
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
            match item {
                Ok(ProviderEvent::TextDelta(delta)) => {
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
                        emit!(
                            events,
                            sink,
                            primary_attempt_event(
                                &attempt_id,
                                RuntimeDiagnosticState::Failed,
                                Some(RuntimeDiagnosticFailure::ReasoningLimit),
                                attempt_started_at,
                                &diagnostic_metrics,
                                Some(&error),
                            )
                        );
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
                    diagnostic_metrics.input_tokens = Some(input);
                    diagnostic_metrics.output_tokens = Some(output);
                    diagnostic_metrics.cache_read_tokens = Some(cache_read);
                    diagnostic_metrics.cache_write_tokens = Some(cache_write);
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
                    usage_seen = true;
                    valid_primary_input =
                        (input > 0 && cache_read <= input && cache_write <= input).then_some(input);
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
        let normal_terminal = matches!(
            (stop_reason, calls.is_empty()),
            (Some(StopReason::End), true) | (Some(StopReason::ToolUse), false)
        );
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

        let batch_policy = classify_tool_batch(
            &calls
                .iter()
                .map(|call| call.name.as_str())
                .collect::<Vec<_>>(),
        );
        let mut prepared_calls = Vec::with_capacity(calls.len());
        for call in calls {
            // Compare completed mutations without re-reading today's target.
            let prior_mutation = completed
                .get(&call.id)
                .filter(|_| matches!(call.name.as_str(), "Write" | "Edit" | "write" | "edit"));
            let prepared = if let Some(prior) = prior_mutation {
                prepare_completed_mutation(tools, &tool_config, &capabilities, call, prior)
            } else if batch_policy == BatchPolicy::RejectOtherTools
                && call.name != crate::skills::LOAD_SKILL_TOOL_NAME
            {
                prepare_mixed_rejection_call(tools, &tool_config, &capabilities, call)
            } else {
                prepare_runtime_call(tools, &tool_config, &capabilities, call)
            };
            match prepared {
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

        if batch_policy == BatchPolicy::RejectOtherTools {
            // A provider can emit operational calls before a load in the
            // same response. Resolve all loads first, then deny every other
            // member without entering its dispatcher or permission prompt.
            prepared_calls.sort_by_key(|prepared| {
                prepared.call().name != crate::skills::LOAD_SKILL_TOOL_NAME
            });
        }

        for prepared in prepared_calls {
            let call = prepared.call().clone();
            // `tool_call_count` is a telemetry/result field only. The run is
            // bounded by `tool_config.turn_limit` (provider round-trips), not
            // by a per-tool-call count, matching Claude Code / Codex.
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
            let validation_output = match &prepared {
                PreparedRuntimeCall::InvalidWriteEdit { result, .. } => Some(result.as_str()),
                PreparedRuntimeCall::InvalidBash { .. } => Some(BASH_INVALID_INPUT_OUTPUT),
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
                emit!(events, sink, RuntimeEvent::ToolCallFinished(result.clone()));
                messages.push(ChatMessage::tool_result(&call.id, &result.output));
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
        if batch_policy != BatchPolicy::SkillOnly {
            direct_user_skill_round = false;
        }
        live_projection_revision = live_projection_revision.saturating_add(1);
    }
}
