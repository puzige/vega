use super::*;

pub(crate) enum PreparedRuntimeCall {
    Readonly(RuntimeToolCall),
    Write {
        call: RuntimeToolCall,
        tools: vega_tools::Tools,
        prepared: vega_tools::PreparedWrite,
    },
    Edit {
        call: RuntimeToolCall,
        tools: vega_tools::Tools,
        prepared: vega_tools::PreparedEdit,
    },
    Bash {
        call: RuntimeToolCall,
        tools: vega_tools::Tools,
        prepared: vega_tools::PreparedBash,
    },
    InvalidWriteEdit {
        call: RuntimeToolCall,
        result: String,
    },
    RunModeMutation(RuntimeToolCall),
    InvalidBash {
        call: RuntimeToolCall,
        code: vega_tools::BashErrorCode,
    },
    Unknown(RuntimeToolCall),
}

impl fmt::Debug for PreparedRuntimeCall {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedRuntimeCall")
            .field("call", self.call())
            .field("private_input", &"[REDACTED]")
            .finish()
    }
}

impl PreparedRuntimeCall {
    pub(crate) fn call(&self) -> &RuntimeToolCall {
        match self {
            Self::Readonly(call)
            | Self::Unknown(call)
            | Self::RunModeMutation(call)
            | Self::InvalidWriteEdit { call, .. }
            | Self::InvalidBash { call, .. }
            | Self::Write { call, .. }
            | Self::Edit { call, .. }
            | Self::Bash { call, .. } => call,
        }
    }

    fn permission_target(&self) -> Option<RuntimePermissionTarget> {
        match self {
            Self::Write { call, prepared, .. } => Some(RuntimePermissionTarget {
                call_id: call.id.clone(),
                tool: RuntimeMutatingTool::Write,
                exact_pattern: prepared.normalized_path().to_string(),
                display_target: prepared.normalized_path().to_string(),
            }),
            Self::Edit { call, prepared, .. } => Some(RuntimePermissionTarget {
                call_id: call.id.clone(),
                tool: RuntimeMutatingTool::Edit,
                exact_pattern: prepared.normalized_path().to_string(),
                display_target: prepared.normalized_path().to_string(),
            }),
            Self::Bash { call, prepared, .. } => Some(RuntimePermissionTarget {
                call_id: call.id.clone(),
                tool: RuntimeMutatingTool::Bash,
                exact_pattern: prepared.command().to_string(),
                display_target: prepared.command().to_string(),
            }),
            _ => None,
        }
    }
}

pub(crate) enum Authorization {
    Approved {
        audit: RuntimeApprovalAudit,
        remember_rule: Option<RuntimePermissionTarget>,
    },
    Terminal(RuntimeToolResult),
}

pub(crate) fn prepare_runtime_call(
    base_tools: &vega_tools::Tools,
    config: &RuntimeToolConfig,
    raw_call: RuntimeToolCall,
) -> Result<PreparedRuntimeCall, VegaError> {
    match raw_call.name.as_str() {
        "read" | "glob" | "grep" => Ok(PreparedRuntimeCall::Readonly(raw_call)),
        "write" | "edit" => {
            let tool = if raw_call.name == "write" {
                vega_tools::MutationTool::Write
            } else {
                vega_tools::MutationTool::Edit
            };
            let audit = if tool == vega_tools::MutationTool::Write {
                base_tools.audit_write_json(&raw_call.input_json)
            } else {
                base_tools.audit_edit_json(&raw_call.input_json)
            };
            let audit = match audit {
                Ok(audit) => audit,
                Err(vega_tools::PrepareMutationError::Invalid(invalid)) => {
                    return invalid_runtime_call(raw_call, invalid);
                }
                Err(vega_tools::PrepareMutationError::Internal(_)) => {
                    return Err(safe_prepare_error(&raw_call.name));
                }
            };
            if vega_tools::CheckpointIds::new(&config.project_id, &config.thread_id, &raw_call.id)
                .is_err()
            {
                let invalid = vega_tools::InvalidMutation::from_raw(
                    tool,
                    &raw_call.input_json,
                    vega_tools::MutationErrorCode::CheckpointIdInvalid,
                )
                .map_err(|_| safe_prepare_error(&raw_call.name))?;
                return invalid_runtime_call(raw_call, invalid);
            }
            if config.run_mode != RuntimeRunMode::Execute {
                let safe_json = audit
                    .to_json()
                    .map_err(|_| safe_prepare_error(&raw_call.name))?;
                return Ok(PreparedRuntimeCall::RunModeMutation(RuntimeToolCall {
                    input_json: safe_json,
                    ..raw_call
                }));
            }
            let scoped = match base_tools.clone().with_mutation_context(
                &config.checkpoint_root,
                &config.project_id,
                &config.thread_id,
                &raw_call.id,
            ) {
                Ok(scoped) => scoped,
                Err(vega_tools::ToolError::Mutation(error))
                    if error.code() == vega_tools::MutationErrorCode::CheckpointIdInvalid =>
                {
                    let invalid = vega_tools::InvalidMutation::from_raw(
                        tool,
                        &raw_call.input_json,
                        vega_tools::MutationErrorCode::CheckpointIdInvalid,
                    )
                    .map_err(|_| safe_prepare_error(&raw_call.name))?;
                    return invalid_runtime_call(raw_call, invalid);
                }
                Err(_) => return Err(safe_prepare_error(&raw_call.name)),
            };
            if tool == vega_tools::MutationTool::Write {
                match scoped.prepare_write_json(&raw_call.input_json) {
                    Ok(prepared) => {
                        let safe_json = prepared
                            .audit()
                            .to_json()
                            .map_err(|_| safe_prepare_error("write"))?;
                        Ok(PreparedRuntimeCall::Write {
                            call: RuntimeToolCall {
                                input_json: safe_json,
                                ..raw_call
                            },
                            tools: scoped,
                            prepared,
                        })
                    }
                    Err(vega_tools::PrepareMutationError::Invalid(invalid)) => {
                        invalid_runtime_call(raw_call, invalid)
                    }
                    Err(vega_tools::PrepareMutationError::Internal(_)) => {
                        Err(safe_prepare_error("write"))
                    }
                }
            } else {
                match scoped.prepare_edit_json(&raw_call.input_json) {
                    Ok(prepared) => {
                        let safe_json = prepared
                            .audit()
                            .to_json()
                            .map_err(|_| safe_prepare_error("edit"))?;
                        Ok(PreparedRuntimeCall::Edit {
                            call: RuntimeToolCall {
                                input_json: safe_json,
                                ..raw_call
                            },
                            tools: scoped,
                            prepared,
                        })
                    }
                    Err(vega_tools::PrepareMutationError::Invalid(invalid)) => {
                        invalid_runtime_call(raw_call, invalid)
                    }
                    Err(vega_tools::PrepareMutationError::Internal(_)) => {
                        Err(safe_prepare_error("edit"))
                    }
                }
            }
        }
        "bash" if config.run_mode != RuntimeRunMode::Execute => {
            Ok(PreparedRuntimeCall::RunModeMutation(raw_call))
        }
        "bash" => match base_tools.prepare_bash_json(&raw_call.input_json) {
            Ok(prepared) => Ok(PreparedRuntimeCall::Bash {
                call: raw_call,
                tools: base_tools.clone(),
                prepared,
            }),
            Err(error) => Ok(PreparedRuntimeCall::InvalidBash {
                call: raw_call,
                code: error.code(),
            }),
        },
        _ => Ok(PreparedRuntimeCall::Unknown(RuntimeToolCall {
            input_json: "{}".to_string(),
            ..raw_call
        })),
    }
}

pub(crate) fn invalid_runtime_call(
    raw_call: RuntimeToolCall,
    invalid: vega_tools::InvalidMutation,
) -> Result<PreparedRuntimeCall, VegaError> {
    let safe_json = invalid
        .audit()
        .to_json()
        .map_err(|_| safe_prepare_error(&raw_call.name))?;
    Ok(PreparedRuntimeCall::InvalidWriteEdit {
        call: RuntimeToolCall {
            input_json: safe_json,
            ..raw_call
        },
        result: invalid.tool_result().to_string(),
    })
}

pub(crate) fn safe_prepare_error(tool: &str) -> VegaError {
    VegaError::Tool {
        tool: tool.to_string(),
        message: "safe input preparation failed".to_string(),
    }
}

pub(crate) async fn authorize_call(
    prepared: &PreparedRuntimeCall,
    config: &RuntimeToolConfig,
    exact_rules: &HashSet<RuntimeExactRule>,
    hook: &dyn RuntimePermissionHook,
    cancel: &CancellationToken,
) -> Result<Authorization, VegaError> {
    match prepared {
        PreparedRuntimeCall::InvalidWriteEdit { call, result } => {
            Ok(Authorization::Terminal(terminal_result(
                call,
                result.clone(),
                RuntimeToolStatus::Rejected,
                Some(validation_audit()),
            )))
        }
        PreparedRuntimeCall::RunModeMutation(call) => Ok(Authorization::Terminal(terminal_result(
            call,
            "Tool error: denied by run mode".to_string(),
            RuntimeToolStatus::Rejected,
            Some(run_mode_denial()),
        ))),
        PreparedRuntimeCall::InvalidBash { call, code } => {
            Ok(Authorization::Terminal(terminal_result(
                call,
                format!("Tool error: invalid bash input ({})", code.as_str()),
                RuntimeToolStatus::Rejected,
                Some(validation_audit()),
            )))
        }
        PreparedRuntimeCall::Unknown(call) => Ok(Authorization::Terminal(terminal_result(
            call,
            "Tool error: denied: unavailable tool".to_string(),
            RuntimeToolStatus::Rejected,
            Some(run_mode_denial()),
        ))),
        PreparedRuntimeCall::Readonly(_) => {
            if cancel.is_cancelled() {
                return Ok(cancelled_permission(prepared.call()));
            }
            match decide_capability(config.run_mode, RuntimeToolClass::Readonly) {
                RuntimeCapabilityOutcome::Approved(audit) => Ok(Authorization::Approved {
                    audit,
                    remember_rule: None,
                }),
                RuntimeCapabilityOutcome::Rejected(audit) => {
                    Ok(Authorization::Terminal(terminal_result(
                        prepared.call(),
                        "Tool error: denied".to_string(),
                        RuntimeToolStatus::Rejected,
                        Some(audit),
                    )))
                }
                RuntimeCapabilityOutcome::ExecuteEligible(_) => Err(safe_permission_error()),
            }
        }
        PreparedRuntimeCall::Write { .. }
        | PreparedRuntimeCall::Edit { .. }
        | PreparedRuntimeCall::Bash { .. } => {
            let target = prepared
                .permission_target()
                .ok_or_else(safe_permission_error)?;
            let capability =
                decide_capability(config.run_mode, RuntimeToolClass::Mutating(target.clone()));
            let RuntimeCapabilityOutcome::ExecuteEligible(eligibility) = capability else {
                return match capability {
                    RuntimeCapabilityOutcome::Rejected(audit) => {
                        Ok(Authorization::Terminal(terminal_result(
                            prepared.call(),
                            "Tool error: denied by run mode".to_string(),
                            RuntimeToolStatus::Rejected,
                            Some(audit),
                        )))
                    }
                    _ => Err(safe_permission_error()),
                };
            };
            let danger = if let PreparedRuntimeCall::Bash { prepared, .. } = prepared {
                vega_tools::danger::detect_danger(prepared.command())
                    .map_err(|_| safe_permission_error())?
                    .map(|danger| RuntimeDangerFacts {
                        rule_id: danger.rule_id.to_string(),
                        reason: danger.reason.to_string(),
                    })
            } else {
                None
            };
            if cancel.is_cancelled() && danger.is_none() {
                return Ok(cancelled_permission(prepared.call()));
            }
            let exact_rule_matches = exact_rules.contains(&RuntimeExactRule {
                tool: target.tool,
                pattern: target.exact_pattern.clone(),
            });
            decide_mutating_permission(
                eligibility,
                target,
                config.permission_mode,
                danger,
                exact_rule_matches,
                config.permission_timeout,
                hook,
                cancel,
            )
            .await
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn decide_mutating_permission(
    eligibility: crate::RuntimeExecuteEligibility,
    target: RuntimePermissionTarget,
    permission_mode: RuntimePermissionMode,
    danger: Option<RuntimeDangerFacts>,
    exact_rule_matches: bool,
    timeout: Duration,
    hook: &dyn RuntimePermissionHook,
    cancel: &CancellationToken,
) -> Result<Authorization, VegaError> {
    let facts = RuntimeExecutePermission {
        permission_mode,
        target: target.clone(),
        danger: danger.clone(),
        exact_rule_matches,
        danger_response: None,
        ordinary_response: None,
    };
    let initial =
        decide_execute_permission(eligibility, facts).map_err(|_| safe_permission_error())?;
    let RuntimePermissionOutcome::Prompt(prompt) = initial else {
        return permission_outcome(target, initial);
    };
    let (decision, _) = wait_for_permission(hook, prompt.clone(), timeout, cancel).await;
    let next_eligibility = match decide_capability(
        RuntimeRunMode::Execute,
        RuntimeToolClass::Mutating(target.clone()),
    ) {
        RuntimeCapabilityOutcome::ExecuteEligible(eligibility) => eligibility,
        _ => return Err(safe_permission_error()),
    };
    let response_facts = RuntimeExecutePermission {
        permission_mode,
        target: target.clone(),
        danger,
        exact_rule_matches,
        danger_response: prompt.danger.as_ref().map(|_| decision.clone()),
        ordinary_response: prompt.danger.is_none().then_some(decision),
    };
    let outcome = decide_execute_permission(next_eligibility, response_facts)
        .map_err(|_| safe_permission_error())?;
    permission_outcome(target, outcome)
}

pub(crate) fn permission_outcome(
    target: RuntimePermissionTarget,
    outcome: RuntimePermissionOutcome,
) -> Result<Authorization, VegaError> {
    match outcome {
        RuntimePermissionOutcome::Approved {
            audit,
            remember_rule,
        } => Ok(Authorization::Approved {
            audit,
            remember_rule: remember_rule.then_some(target),
        }),
        RuntimePermissionOutcome::Rejected {
            audit,
            remember_rule,
        } => {
            let mut result = terminal_result(
                &RuntimeToolCall {
                    id: target.call_id.clone(),
                    name: target.tool.as_str().to_string(),
                    input_json: String::new(),
                },
                "Tool error: permission denied".to_string(),
                RuntimeToolStatus::Rejected,
                Some(audit),
            );
            result.remember_rule = remember_rule.then_some(target);
            Ok(Authorization::Terminal(result))
        }
        RuntimePermissionOutcome::Prompt(_) => Err(safe_permission_error()),
    }
}

pub(crate) async fn wait_for_permission(
    hook: &dyn RuntimePermissionHook,
    prompt: RuntimePermissionPrompt,
    timeout: Duration,
    cancel: &CancellationToken,
) -> (RuntimeUserDecision, bool) {
    let prompt_cancel = cancel.child_token();
    let future = hook.request(prompt, prompt_cancel.clone());
    let waited = tokio::select! {
        biased;
        _ = cancel.cancelled() => (RuntimeUserDecision::Timeout, true),
        _ = tokio::time::sleep(timeout) => (RuntimeUserDecision::Timeout, false),
        decision = future => (decision.unwrap_or(RuntimeUserDecision::Timeout), false),
    };
    prompt_cancel.cancel();
    waited
}

pub(crate) fn validation_audit() -> RuntimeApprovalAudit {
    RuntimeApprovalAudit {
        decision: RuntimeApprovalDecision::Deny,
        note: None,
        source: RuntimeApprovalSource::Validation,
        danger: None,
    }
}

pub(crate) fn run_mode_denial() -> RuntimeApprovalAudit {
    RuntimeApprovalAudit {
        decision: RuntimeApprovalDecision::Deny,
        note: None,
        source: RuntimeApprovalSource::RunMode,
        danger: None,
    }
}

pub(crate) fn cancelled_permission(call: &RuntimeToolCall) -> Authorization {
    Authorization::Terminal(terminal_result(
        call,
        "Tool error: permission denied".to_string(),
        RuntimeToolStatus::Rejected,
        Some(RuntimeApprovalAudit {
            decision: RuntimeApprovalDecision::Deny,
            note: None,
            source: RuntimeApprovalSource::Timeout,
            danger: None,
        }),
    ))
}

pub(crate) fn safe_permission_error() -> VegaError {
    VegaError::Tool {
        tool: "permission".to_string(),
        message: "permission decision failed closed".to_string(),
    }
}

pub(crate) fn terminal_result(
    call: &RuntimeToolCall,
    output: String,
    status: RuntimeToolStatus,
    approval: Option<RuntimeApprovalAudit>,
) -> RuntimeToolResult {
    RuntimeToolResult {
        call_id: call.id.clone(),
        output,
        status,
        reused: false,
        exit_code: None,
        duration_ms: None,
        truncated: None,
        approval,
        remember_rule: None,
    }
}

pub(crate) fn conflict_result(call: &RuntimeToolCall) -> RuntimeToolResult {
    terminal_result(
        call,
        CALL_ID_CONFLICT_OUTPUT.to_string(),
        RuntimeToolStatus::Failed,
        None,
    )
}

pub(crate) fn runtime_inputs_semantically_equal(tool: &str, left: &str, right: &str) -> bool {
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

pub(crate) async fn execute_prepared_waiting(
    prepared: PreparedRuntimeCall,
    base_tools: &vega_tools::Tools,
    cancel: &CancellationToken,
) -> (RuntimeToolResult, bool) {
    match prepared {
        PreparedRuntimeCall::Readonly(call) => {
            execute_readonly_waiting(base_tools, &call, cancel).await
        }
        PreparedRuntimeCall::Write {
            call,
            tools,
            prepared,
        } => {
            let call_for_worker = call.clone();
            let worker_cancel = cancel.child_token();
            let mut task = tokio::task::spawn_blocking(move || {
                if worker_cancel.is_cancelled() {
                    return terminal_result(
                        &call_for_worker,
                        CANCELLED_BEFORE_EXECUTION_OUTPUT.to_string(),
                        RuntimeToolStatus::Cancelled,
                        None,
                    );
                }
                let result = tools.execute_write(prepared);
                mutation_result(&call_for_worker, result, true)
            });
            wait_blocking_result(&call, &mut task, cancel).await
        }
        PreparedRuntimeCall::Edit {
            call,
            tools,
            prepared,
        } => {
            let call_for_worker = call.clone();
            let worker_cancel = cancel.child_token();
            let mut task = tokio::task::spawn_blocking(move || {
                if worker_cancel.is_cancelled() {
                    return terminal_result(
                        &call_for_worker,
                        CANCELLED_BEFORE_EXECUTION_OUTPUT.to_string(),
                        RuntimeToolStatus::Cancelled,
                        None,
                    );
                }
                let result = tools.execute_edit(prepared);
                mutation_result(&call_for_worker, result, false)
            });
            wait_blocking_result(&call, &mut task, cancel).await
        }
        PreparedRuntimeCall::Bash {
            call,
            tools,
            prepared,
        } => {
            let result = tools.execute_bash(prepared, cancel.child_token()).await;
            match result {
                Ok(output) => (
                    RuntimeToolResult {
                        call_id: call.id,
                        output: output.text,
                        status: RuntimeToolStatus::Success,
                        reused: false,
                        exit_code: Some(output.exit_code),
                        duration_ms: Some(output.duration_ms),
                        truncated: Some(output.truncated),
                        approval: None,
                        remember_rule: None,
                    },
                    false,
                ),
                Err(error) => {
                    let cancelled = error.code() == vega_tools::BashErrorCode::Cancelled;
                    (
                        terminal_result(
                            &call,
                            format!("Tool error: bash failed ({})", error.code().as_str()),
                            if cancelled {
                                RuntimeToolStatus::Cancelled
                            } else {
                                RuntimeToolStatus::Failed
                            },
                            None,
                        ),
                        cancelled,
                    )
                }
            }
        }
        PreparedRuntimeCall::InvalidWriteEdit { call, result } => (
            terminal_result(
                &call,
                result,
                RuntimeToolStatus::Rejected,
                Some(validation_audit()),
            ),
            false,
        ),
        PreparedRuntimeCall::RunModeMutation(call) => (
            terminal_result(
                &call,
                "Tool error: denied by run mode".to_string(),
                RuntimeToolStatus::Rejected,
                Some(run_mode_denial()),
            ),
            false,
        ),
        PreparedRuntimeCall::InvalidBash { call, code } => (
            terminal_result(
                &call,
                format!("Tool error: invalid bash input ({})", code.as_str()),
                RuntimeToolStatus::Rejected,
                Some(validation_audit()),
            ),
            false,
        ),
        PreparedRuntimeCall::Unknown(call) => (
            terminal_result(
                &call,
                "Tool error: denied: unavailable tool".to_string(),
                RuntimeToolStatus::Rejected,
                Some(run_mode_denial()),
            ),
            false,
        ),
    }
}

pub(crate) async fn wait_blocking_result(
    call: &RuntimeToolCall,
    task: &mut tokio::task::JoinHandle<RuntimeToolResult>,
    cancel: &CancellationToken,
) -> (RuntimeToolResult, bool) {
    tokio::select! {
        biased;
        _ = cancel.cancelled() => {
            let result = match (&mut *task).await {
                Ok(result) => result,
                Err(_) => terminal_result(call, "Tool error: tool worker failed".to_string(), RuntimeToolStatus::Failed, None),
            };
            (result, true)
        }
        joined = &mut *task => {
            let result = joined.unwrap_or_else(|_| terminal_result(call, "Tool error: tool worker failed".to_string(), RuntimeToolStatus::Failed, None));
            (result, false)
        }
    }
}

pub(crate) fn mutation_result(
    call: &RuntimeToolCall,
    result: Result<vega_tools::ToolOutput, vega_tools::ToolError>,
    write: bool,
) -> RuntimeToolResult {
    match result {
        Ok(output) => {
            let strict = if write {
                vega_tools::WriteSuccessOutput::from_json(&output.text).is_ok()
            } else {
                vega_tools::EditSuccessOutput::from_json(&output.text).is_ok()
            };
            if strict {
                RuntimeToolResult {
                    call_id: call.id.clone(),
                    output: output.text,
                    status: RuntimeToolStatus::Success,
                    reused: false,
                    exit_code: None,
                    duration_ms: None,
                    truncated: Some(output.truncated),
                    approval: None,
                    remember_rule: None,
                }
            } else {
                terminal_result(
                    call,
                    "Tool error: invalid mutation result".to_string(),
                    RuntimeToolStatus::Failed,
                    None,
                )
            }
        }
        Err(_) => terminal_result(
            call,
            format!("Tool error: {} failed", call.name),
            RuntimeToolStatus::Failed,
            None,
        ),
    }
}

pub(crate) fn outcome(
    events: Vec<RuntimeEvent>,
    messages: Vec<ChatMessage>,
    final_text: String,
    tool_call_count: usize,
    executed_tool_call_count: usize,
    interrupted: bool,
    failed: bool,
) -> AgentOutcome {
    AgentOutcome {
        events,
        messages,
        final_text,
        tool_call_count,
        executed_tool_call_count,
        interrupted,
        failed,
    }
}

pub(crate) async fn execute_readonly_waiting(
    tools: &vega_tools::Tools,
    call: &RuntimeToolCall,
    cancel: &CancellationToken,
) -> (RuntimeToolResult, bool) {
    let owned_tools = tools.clone();
    let owned_call = call.clone();
    let worker_cancel = cancel.child_token();
    let mut task = tokio::task::spawn_blocking(move || {
        if worker_cancel.is_cancelled() {
            terminal_result(
                &owned_call,
                CANCELLED_BEFORE_EXECUTION_OUTPUT.to_string(),
                RuntimeToolStatus::Cancelled,
                None,
            )
        } else {
            execute_readonly(&owned_tools, &owned_call)
        }
    });
    tokio::select! {
        biased;
        _ = cancel.cancelled() => {
            let result = match task.await {
                Ok(result) => result,
                Err(error) => failed_tool_result(call, format!("tool worker failed: {error}")),
            };
            (result, true)
        }
        result = &mut task => {
            let result = match result {
                Ok(result) => result,
                Err(error) => failed_tool_result(call, format!("tool worker failed: {error}")),
            };
            (result, false)
        }
    }
}

pub(crate) fn execute_readonly(
    tools: &vega_tools::Tools,
    call: &RuntimeToolCall,
) -> RuntimeToolResult {
    let result =
        parse_input(&call.name, &call.input_json).and_then(|input| match call.name.as_str() {
            "read" => {
                let path = required_str(&input, "path")?;
                let offset = optional_usize(&input, "offset")?;
                let limit = optional_usize(&input, "limit")?;
                tools
                    .read(path, offset, limit)
                    .map_err(|error| error.to_string())
            }
            "glob" => {
                let pattern = required_str(&input, "pattern")?;
                tools.glob(pattern).map_err(|error| error.to_string())
            }
            "grep" => {
                let pattern = required_str(&input, "pattern")?;
                let path = optional_str(&input, "path")?;
                tools.grep(pattern, path).map_err(|error| error.to_string())
            }
            _ => Err("permission gate rejected a non-readonly tool".to_string()),
        });
    match result {
        Ok(output) => RuntimeToolResult {
            call_id: call.id.clone(),
            output: truncate_output_lines(&output.text),
            status: RuntimeToolStatus::Success,
            reused: false,
            exit_code: None,
            duration_ms: None,
            truncated: Some(output.truncated),
            approval: None,
            remember_rule: None,
        },
        Err(message) => RuntimeToolResult {
            call_id: call.id.clone(),
            output: format!("Tool error: {message}"),
            status: RuntimeToolStatus::Failed,
            reused: false,
            exit_code: None,
            duration_ms: None,
            truncated: None,
            approval: None,
            remember_rule: None,
        },
    }
}

pub(crate) fn failed_tool_result(call: &RuntimeToolCall, message: String) -> RuntimeToolResult {
    RuntimeToolResult {
        call_id: call.id.clone(),
        output: format!("Tool error: {message}"),
        status: RuntimeToolStatus::Failed,
        reused: false,
        exit_code: None,
        duration_ms: None,
        truncated: None,
        approval: None,
        remember_rule: None,
    }
}

pub(crate) fn parse_input(tool: &str, input_json: &str) -> Result<Value, String> {
    let input: Value = serde_json::from_str(input_json)
        .map_err(|error| format!("invalid {tool} input JSON: {error}"))?;
    if input.is_object() {
        Ok(input)
    } else {
        Err(format!("{tool} input must be a JSON object"))
    }
}

pub(crate) fn required_str<'a>(input: &'a Value, key: &str) -> Result<&'a str, String> {
    input
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing or non-string '{key}'"))
}

pub(crate) fn optional_str<'a>(input: &'a Value, key: &str) -> Result<Option<&'a str>, String> {
    match input.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_str()
            .map(Some)
            .ok_or_else(|| format!("'{key}' must be a string")),
    }
}

pub(crate) fn optional_usize(input: &Value, key: &str) -> Result<Option<usize>, String> {
    match input.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            let raw = value
                .as_u64()
                .ok_or_else(|| format!("'{key}' must be a non-negative integer"))?;
            usize::try_from(raw)
                .map(Some)
                .map_err(|_| format!("'{key}' exceeds this platform's integer range"))
        }
    }
}

pub(crate) fn truncate_output_lines(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= OUTPUT_HALF_LINES * 2 {
        return text.to_string();
    }
    let mut kept = Vec::with_capacity(OUTPUT_HALF_LINES * 2 + 1);
    kept.extend_from_slice(&lines[..OUTPUT_HALF_LINES]);
    kept.push(OUTPUT_TRUNCATION_MARKER);
    kept.extend_from_slice(&lines[lines.len() - OUTPUT_HALF_LINES..]);
    kept.join("\n")
}

pub(crate) fn tool_definitions(run_mode: RuntimeRunMode) -> Vec<ToolDefinition> {
    let mut definitions = vec![
        ToolDefinition {
            name: "read".to_string(),
            description: "Read a project-relative text file with line numbers.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "offset": { "type": "integer", "minimum": 1 },
                    "limit": { "type": "integer", "minimum": 0 }
                },
                "required": ["path"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "glob".to_string(),
            description: "List project files matching a gitignore-style glob.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "pattern": { "type": "string" } },
                "required": ["pattern"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "grep".to_string(),
            description: "Search project files with a regular expression.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string" },
                    "path": { "type": "string" }
                },
                "required": ["pattern"],
                "additionalProperties": false
            }),
        },
    ];
    if run_mode == RuntimeRunMode::Execute {
        definitions.extend([
            ToolDefinition {
                name: "write".to_string(),
                description: "Write a project-relative file after permission approval.".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "content": { "type": "string" }
                    },
                    "required": ["path", "content"],
                    "additionalProperties": false
                }),
            },
            ToolDefinition {
                name: "edit".to_string(),
                description: "Replace one unique string in a project-relative file after approval."
                    .to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "old_string": { "type": "string" },
                        "new_string": { "type": "string" }
                    },
                    "required": ["path", "old_string", "new_string"],
                    "additionalProperties": false
                }),
            },
            ToolDefinition {
                name: "bash".to_string(),
                description: "Run a sandboxed command at the project root after approval."
                    .to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "cmd": { "type": "string" },
                        "timeout_ms": { "type": "integer", "minimum": 1 }
                    },
                    "required": ["cmd"],
                    "additionalProperties": false
                }),
            },
        ]);
    }
    definitions
}
