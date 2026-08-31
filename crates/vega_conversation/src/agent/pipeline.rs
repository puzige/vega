use super::*;

pub(crate) fn prepare_run(
    database_path: PathBuf,
    thread_id: String,
    user_content: String,
    system_prompt: String,
    user_message_id: String,
    assistant_message_id: String,
    config: PersistenceActorConfig,
    uses_existing_user: bool,
    pricing_catalog: Option<vega_token::PricingCatalog>,
) -> Result<PreparedRun, ConversationError> {
    #[cfg(not(test))]
    let _ = &config;
    #[cfg(test)]
    if let Some(delay) = config.preparation_delay {
        std::thread::sleep(delay);
    }

    let store = Store::open(&database_path).map_err(runtime_store_error)?;
    #[cfg(test)]
    if config.preparation_query_only {
        store
            .conn()
            .execute_batch("PRAGMA query_only = ON")
            .map_err(runtime_store_error)?;
    }
    if !uses_existing_user {
        vega_store::recovery::recover_thread(store.conn(), &thread_id, now_ms())
            .map_err(runtime_store_error)?;
    }
    let transaction = store.immediate_transaction().map_err(runtime_store_error)?;
    let thread = vega_store::threads::find(&transaction, &thread_id)
        .map_err(runtime_store_error)?
        .ok_or_else(|| ConversationError::NotFound(thread_id.clone()))?;
    let run_mode = ThreadMode::parse(&thread.mode)
        .ok_or_else(|| ConversationError::CorruptRow(format!("run mode: {}", thread.mode)))?;
    let permission_mode =
        crate::types::PermissionMode::parse(&thread.permission_mode).ok_or_else(|| {
            ConversationError::CorruptRow(format!("permission_mode: {}", thread.permission_mode))
        })?;
    #[cfg(test)]
    let checkpoint_root = config.checkpoint_root.clone().unwrap_or_else(|| {
        database_path
            .parent()
            .map_or_else(PathBuf::new, |parent| parent.join("checkpoints"))
    });
    #[cfg(not(test))]
    let checkpoint_root = database_path
        .parent()
        .ok_or_else(|| ConversationError::CorruptRow("database path has no parent".to_string()))?
        .join("checkpoints");
    if checkpoint_root.as_os_str().is_empty() {
        return Err(ConversationError::CorruptRow(
            "database path has no parent".to_string(),
        ));
    }
    if run_mode == ThreadMode::Execute {
        fs::create_dir_all(&checkpoint_root).map_err(|_| {
            ConversationError::Runtime(Arc::new(VegaError::Io(std::io::Error::other(
                "checkpoint root unavailable",
            ))))
        })?;
    }
    let exact_rules = permissions::list_exact(&transaction, &thread.project_id)
        .map_err(|error| runtime_store_error(std::io::Error::other(error.to_string())))?
        .into_iter()
        .map(|rule| {
            if rule.pattern.is_empty() {
                return Err(ConversationError::CorruptRow(
                    "permission rule has empty exact pattern".to_string(),
                ));
            }
            let tool = match rule.tool.as_str() {
                "bash" => RuntimeMutatingTool::Bash,
                "write" => RuntimeMutatingTool::Write,
                "edit" => RuntimeMutatingTool::Edit,
                _ => {
                    return Err(ConversationError::CorruptRow(
                        "permission rule has unsupported tool".to_string(),
                    ));
                }
            };
            Ok(RuntimeExactRule {
                tool,
                pattern: rule.pattern,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let now = now_ms();
    if uses_existing_user {
        let existing = messages::find(&transaction, &user_message_id)
            .map_err(runtime_store_error)?
            .ok_or_else(|| ConversationError::NotFound(user_message_id.clone()))?;
        if existing.thread_id != thread_id
            || existing.role != "user"
            || existing.kind != "text"
            || existing.status != "done"
            || existing.content != crate::plans::APPROVAL_INSTRUCTION
            || user_content != crate::plans::APPROVAL_INSTRUCTION
            || run_mode != ThreadMode::Execute
        {
            return Err(ConversationError::CorruptRow(
                "approved instruction identity mismatch".to_string(),
            ));
        }
        let next = messages::next_seq(&transaction, &thread_id).map_err(runtime_store_error)?;
        if next != existing.seq + 1 {
            return Err(ConversationError::CorruptRow(
                "approved instruction was already consumed".to_string(),
            ));
        }
        let plans =
            messages::plans_for_thread(&transaction, &thread_id).map_err(runtime_store_error)?;
        let matching_approvals = plans
            .iter()
            .filter(|plan| plan.seq < existing.seq)
            .filter(|plan| {
                plan.plan_status.as_deref() == Some("approved")
                    && plan.plan_reviewed_at == Some(existing.created_at)
            })
            .count();
        if matching_approvals != 1 {
            return Err(ConversationError::CorruptRow(
                "approved instruction has no matching plan".to_string(),
            ));
        }
    } else {
        let user_seq = messages::next_seq(&transaction, &thread_id).map_err(runtime_store_error)?;
        messages::insert(
            &transaction,
            &messages::MessageRow {
                id: user_message_id.clone(),
                thread_id: thread_id.clone(),
                seq: user_seq,
                role: "user".to_string(),
                kind: "text".to_string(),
                content: user_content,
                status: "done".to_string(),
                created_at: now,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .map_err(runtime_store_error)?;
    }
    let assistant_seq =
        messages::next_seq(&transaction, &thread_id).map_err(runtime_store_error)?;
    messages::insert(
        &transaction,
        &messages::MessageRow {
            id: assistant_message_id.clone(),
            thread_id: thread_id.clone(),
            seq: assistant_seq,
            role: "assistant".to_string(),
            // A Plan is promoted atomically only on successful completion.
            // Interrupted/failed streams remain ordinary text history rows.
            kind: "text".to_string(),
            content: String::new(),
            status: "streaming".to_string(),
            created_at: now,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .map_err(runtime_store_error)?;

    let history = messages::recent(&transaction, &thread_id, HISTORY_WINDOW)
        .map_err(runtime_store_error)?
        .into_iter()
        .filter_map(|message| {
            let role = match message.role.as_str() {
                "user" => Some(vega_runtime::ChatRole::User),
                "assistant" => Some(vega_runtime::ChatRole::Assistant),
                _ => None,
            }?;
            Some(vega_runtime::ChatMessage::new(role, message.content))
        })
        .collect();
    let completed_tool_results = tool_calls::terminal_results(&transaction, &thread_id)
        .map_err(|error| runtime_store_error(std::io::Error::other(error.to_string())))?
        .into_iter()
        .map(|(call_id, call)| -> Result<_, ConversationError> {
            let approval = ApprovalAudit::from_json(&call.approval).map_err(|_| {
                ConversationError::CorruptRow(format!(
                    "terminal tool call {call_id} has invalid approval"
                ))
            })?;
            let status = match call.status.as_str() {
                "success" => RuntimeToolStatus::Success,
                "failed" => RuntimeToolStatus::Failed,
                "rejected" => RuntimeToolStatus::Rejected,
                "cancelled" => RuntimeToolStatus::Cancelled,
                other => {
                    return Err(ConversationError::CorruptRow(format!(
                        "terminal tool call {call_id} has status {other}"
                    )));
                }
            };
            let canonical_input = validate_recovered_projection(
                &thread.project_id,
                &thread_id,
                &call_id,
                &call.tool,
                &call.input_json,
                &call.output,
                status,
                &approval,
                call.exit_code,
                call.duration_ms,
            )?;
            let completed = vega_runtime::CompletedToolCall {
                tool: call.tool,
                input_json: canonical_input,
                result: vega_runtime::RuntimeToolResult {
                    call_id: call_id.clone(),
                    output: call.output,
                    status,
                    reused: true,
                    exit_code: call.exit_code,
                    duration_ms: call.duration_ms,
                    truncated: None,
                    approval: Some(approval_audit_to_runtime(&approval)),
                    remember_rule: None,
                },
            };
            Ok((call_id, completed))
        })
        .collect::<Result<_, _>>()?;
    let next_tool_seq =
        tool_calls::next_seq(&transaction, &thread_id).map_err(runtime_store_error)?;
    let foreign_call_ids =
        tool_calls::foreign_call_ids(&transaction, &thread_id).map_err(runtime_store_error)?;
    transaction.commit().map_err(runtime_store_error)?;

    Ok(PreparedRun {
        database_path,
        project_id: thread.project_id.clone(),
        model: thread.model.clone(),
        is_plan: run_mode == ThreadMode::Plan,
        user_message_id,
        assistant_message_id,
        assistant_seq,
        request: AgentRequest {
            model: thread.model,
            system_prompt,
            history,
            max_tokens: None,
            completed_tool_results,
            pricing_catalog,
            tool_config: RuntimeToolConfig::new(
                match run_mode {
                    ThreadMode::Ask => RuntimeRunMode::Ask,
                    ThreadMode::Plan => RuntimeRunMode::Plan,
                    ThreadMode::Execute => RuntimeRunMode::Execute,
                },
                match permission_mode {
                    crate::types::PermissionMode::ReadOnly => RuntimePermissionMode::ReadOnly,
                    crate::types::PermissionMode::Confirm => RuntimePermissionMode::Confirm,
                    crate::types::PermissionMode::Auto => RuntimePermissionMode::Auto,
                },
                thread.project_id,
                thread_id,
                checkpoint_root,
                exact_rules,
            )
            .with_foreign_call_ids(foreign_call_ids),
        },
        next_tool_seq,
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn validate_recovered_projection(
    project_id: &str,
    thread_id: &str,
    call_id: &str,
    tool: &str,
    input_json: &str,
    output: &str,
    status: RuntimeToolStatus,
    approval: &ApprovalAudit,
    exit_code: Option<i32>,
    duration_ms: Option<u64>,
) -> Result<String, ConversationError> {
    let corrupt = || {
        ConversationError::CorruptRow(format!(
            "terminal tool call {call_id} has invalid safe projection"
        ))
    };
    match tool {
        "write" | "edit" => {
            if exit_code.is_some() || duration_ms.is_some() {
                return Err(corrupt());
            }
            if let Ok(valid) = vega_tools::WriteEditAudit::from_json(input_json) {
                if valid.tool().as_str() != tool
                    || approval.source == ApprovalSource::Validation
                    || !approval_source_matches(tool, status, approval.source, false)
                {
                    return Err(corrupt());
                }
                let decision_valid = match status {
                    RuntimeToolStatus::Rejected => {
                        approval.decision == Approval::Deny
                            && (approval.source != ApprovalSource::Recovery
                                || output == vega_store::recovery::RECOVERY_REJECTED_OUTPUT)
                    }
                    RuntimeToolStatus::Success
                    | RuntimeToolStatus::Failed
                    | RuntimeToolStatus::Cancelled => approval.decision != Approval::Deny,
                };
                if !decision_valid {
                    return Err(corrupt());
                }
                let output_valid = match status {
                    RuntimeToolStatus::Success if tool == "write" => {
                        mutation_success_matches(&valid, project_id, thread_id, call_id, output)
                    }
                    RuntimeToolStatus::Success => {
                        mutation_success_matches(&valid, project_id, thread_id, call_id, output)
                    }
                    RuntimeToolStatus::Failed => {
                        output == format!("Tool error: {tool} failed")
                            || output == "Tool error: tool worker failed"
                            || output == "Tool error: invalid mutation result"
                    }
                    RuntimeToolStatus::Rejected if approval.source == ApprovalSource::RunMode => {
                        output == "Tool error: denied by run mode"
                    }
                    RuntimeToolStatus::Rejected if approval.source == ApprovalSource::Recovery => {
                        output == vega_store::recovery::RECOVERY_REJECTED_OUTPUT
                    }
                    RuntimeToolStatus::Rejected if approval.source == ApprovalSource::Legacy => {
                        output == legacy_unavailable_output(tool)
                            || output == "Tool error: permission denied"
                    }
                    RuntimeToolStatus::Rejected => output == "Tool error: permission denied",
                    RuntimeToolStatus::Cancelled
                        if output == vega_store::recovery::RECOVERY_CANCELLED_OUTPUT =>
                    {
                        true
                    }
                    RuntimeToolStatus::Cancelled
                        if output == vega_runtime::CANCELLED_BEFORE_EXECUTION_OUTPUT =>
                    {
                        true
                    }
                    RuntimeToolStatus::Cancelled if tool == "write" => {
                        mutation_success_matches(&valid, project_id, thread_id, call_id, output)
                            || output == "Tool error: write failed"
                            || output == "Tool error: tool worker failed"
                    }
                    RuntimeToolStatus::Cancelled => {
                        mutation_success_matches(&valid, project_id, thread_id, call_id, output)
                            || output == "Tool error: edit failed"
                            || output == "Tool error: tool worker failed"
                    }
                };
                if !output_valid {
                    return Err(corrupt());
                }
                return valid.to_json().map_err(|_| corrupt());
            }
            if let Ok(invalid) = vega_tools::InvalidWriteEditAudit::from_json(input_json) {
                let expected = format!(
                    "Tool error: invalid {tool} input ({})",
                    invalid.validation_error_code().as_str()
                );
                if invalid.tool().as_str() != tool
                    || status != RuntimeToolStatus::Rejected
                    || approval.decision != Approval::Deny
                    || approval.source != ApprovalSource::Validation
                    || output != expected
                {
                    return Err(corrupt());
                }
                return invalid.to_json().map_err(|_| corrupt());
            }
            Err(corrupt())
        }
        "read" | "glob" | "grep" | "bash" => {
            if !approval_source_matches(tool, status, approval.source, false) {
                return Err(corrupt());
            }
            if tool == "bash" && !bash_danger_audit_matches(input_json, approval) {
                return Err(corrupt());
            }
            let decision_valid = match status {
                RuntimeToolStatus::Rejected => approval.decision == Approval::Deny,
                RuntimeToolStatus::Success
                | RuntimeToolStatus::Failed
                | RuntimeToolStatus::Cancelled => approval.decision != Approval::Deny,
            };
            if !decision_valid || (tool != "bash" && (exit_code.is_some() || duration_ms.is_some()))
            {
                return Err(corrupt());
            }
            if tool != "bash"
                && status == RuntimeToolStatus::Rejected
                && !matches!(
                    (approval.source, output),
                    (
                        ApprovalSource::Recovery,
                        vega_store::recovery::RECOVERY_REJECTED_OUTPUT
                    ) | (ApprovalSource::Timeout, "Tool error: permission denied")
                )
            {
                return Err(corrupt());
            }
            if tool == "bash" {
                let metadata_valid = match status {
                    RuntimeToolStatus::Success => exit_code.is_some() && duration_ms.is_some(),
                    RuntimeToolStatus::Failed
                    | RuntimeToolStatus::Rejected
                    | RuntimeToolStatus::Cancelled => exit_code.is_none() && duration_ms.is_none(),
                };
                let output_valid = match status {
                    RuntimeToolStatus::Rejected if approval.source == ApprovalSource::RunMode => {
                        output == "Tool error: denied by run mode"
                    }
                    RuntimeToolStatus::Rejected
                        if approval.source == ApprovalSource::Validation =>
                    {
                        output == "Tool error: invalid bash input (invalid_input)"
                    }
                    RuntimeToolStatus::Rejected if approval.source == ApprovalSource::Recovery => {
                        output == vega_store::recovery::RECOVERY_REJECTED_OUTPUT
                    }
                    RuntimeToolStatus::Rejected if approval.source == ApprovalSource::Legacy => {
                        output == legacy_unavailable_output(tool)
                            || output == "Tool error: permission denied"
                    }
                    RuntimeToolStatus::Rejected => output == "Tool error: permission denied",
                    RuntimeToolStatus::Failed => is_safe_bash_failure(output),
                    RuntimeToolStatus::Cancelled => {
                        output == "Tool error: bash failed (cancelled)"
                            || output == vega_runtime::CANCELLED_BEFORE_EXECUTION_OUTPUT
                            || output == vega_store::recovery::RECOVERY_CANCELLED_OUTPUT
                    }
                    RuntimeToolStatus::Success => true,
                };
                if !metadata_valid || !output_valid {
                    return Err(corrupt());
                }
            }
            Ok(input_json.to_string())
        }
        _ if status == RuntimeToolStatus::Rejected
            && approval.decision == Approval::Deny
            && ((approval.source == ApprovalSource::RunMode
                && output == "Tool error: denied: unavailable tool")
                || (approval.source == ApprovalSource::Recovery
                    && output == vega_store::recovery::RECOVERY_REJECTED_OUTPUT)
                || (approval.source == ApprovalSource::Legacy
                    && output == legacy_unavailable_output(tool)))
            && input_json == "{}"
            && exit_code.is_none()
            && duration_ms.is_none() =>
        {
            Ok(input_json.to_string())
        }
        _ => Err(corrupt()),
    }
}

pub(crate) fn mutation_success_matches(
    audit: &vega_tools::WriteEditAudit,
    project_id: &str,
    thread_id: &str,
    call_id: &str,
    output: &str,
) -> bool {
    let Ok(ids) = vega_tools::CheckpointIds::new(project_id, thread_id, call_id) else {
        return false;
    };
    let expected_ref = ids.checkpoint_ref();
    match audit {
        vega_tools::WriteEditAudit::Write {
            path,
            content_bytes,
            ..
        } => vega_tools::WriteSuccessOutput::from_json(output)
            .ok()
            .is_some_and(|success| {
                success.path == *path
                    && success.bytes_written == *content_bytes
                    && success.checkpoint_ref == expected_ref
            }),
        vega_tools::WriteEditAudit::Edit { path, .. } => {
            vega_tools::EditSuccessOutput::from_json(output)
                .ok()
                .is_some_and(|success| {
                    success.path == *path && success.checkpoint_ref == expected_ref
                })
        }
    }
}

pub(crate) fn is_safe_bash_failure(output: &str) -> bool {
    const CODES: [&str; 9] = [
        "scope_mismatch",
        "hardlink_preflight",
        "sandbox_unavailable",
        "temp_unavailable",
        "cleanup_failed",
        "spawn_failed",
        "process_control_failed",
        "output_failed",
        "timed_out",
    ];
    CODES
        .iter()
        .any(|code| output == format!("Tool error: bash failed ({code})"))
}

pub(crate) fn legacy_unavailable_output(tool: &str) -> String {
    format!("Tool error: denied: tool '{tool}' is unavailable until the S5 permission gate")
}

pub(crate) fn bash_danger_audit_matches(input_json: &str, approval: &ApprovalAudit) -> bool {
    let Ok(command) = vega_tools::bash_permission_signature(input_json) else {
        return matches!(
            approval.source,
            ApprovalSource::Validation
                | ApprovalSource::RunMode
                | ApprovalSource::Recovery
                | ApprovalSource::Legacy
        ) && approval.danger.is_none();
    };
    let Ok(danger) = vega_tools::danger::detect_danger(&command) else {
        return false;
    };
    let Some(danger) = danger else {
        return approval.source != ApprovalSource::Danger && approval.danger.is_none();
    };
    if matches!(
        approval.source,
        ApprovalSource::RunMode | ApprovalSource::Recovery
    ) {
        return approval.danger.is_none();
    }
    if approval.source == ApprovalSource::Legacy {
        return approval.decision == Approval::Deny && approval.danger.is_none();
    }
    let Some(audit) = &approval.danger else {
        return false;
    };
    if audit.rule_id != danger.rule_id {
        return false;
    }
    match approval.source {
        ApprovalSource::Danger => {
            approval.decision == audit.decision && approval.note == audit.note
        }
        ApprovalSource::ReadOnly => {
            approval.decision == Approval::Deny
                && approval.note.is_none()
                && matches!(audit.decision, Approval::Once | Approval::Always)
        }
        ApprovalSource::Timeout => {
            approval.decision == Approval::Deny
                && approval.note.is_none()
                && audit.decision == Approval::Deny
                && audit.note.is_none()
        }
        ApprovalSource::RunMode
        | ApprovalSource::Rule
        | ApprovalSource::Auto
        | ApprovalSource::User
        | ApprovalSource::Validation
        | ApprovalSource::ReadonlyTool
        | ApprovalSource::Recovery
        | ApprovalSource::Legacy => false,
    }
}

pub(crate) fn approval_source_matches(
    tool: &str,
    status: RuntimeToolStatus,
    source: ApprovalSource,
    invalid_projection: bool,
) -> bool {
    if invalid_projection {
        return status == RuntimeToolStatus::Rejected && source == ApprovalSource::Validation;
    }
    match tool {
        "read" | "glob" | "grep" => {
            (status == RuntimeToolStatus::Rejected
                && matches!(source, ApprovalSource::Recovery | ApprovalSource::Timeout))
                || (status != RuntimeToolStatus::Rejected
                    && matches!(
                        source,
                        ApprovalSource::ReadonlyTool | ApprovalSource::Legacy
                    ))
        }
        "write" | "edit" => match status {
            RuntimeToolStatus::Rejected => matches!(
                source,
                ApprovalSource::RunMode
                    | ApprovalSource::ReadOnly
                    | ApprovalSource::User
                    | ApprovalSource::Timeout
                    | ApprovalSource::Legacy
                    | ApprovalSource::Recovery
            ),
            RuntimeToolStatus::Success
            | RuntimeToolStatus::Failed
            | RuntimeToolStatus::Cancelled => matches!(
                source,
                ApprovalSource::User
                    | ApprovalSource::Rule
                    | ApprovalSource::Auto
                    | ApprovalSource::Legacy
            ),
        },
        "bash" => match status {
            RuntimeToolStatus::Rejected => matches!(
                source,
                ApprovalSource::RunMode
                    | ApprovalSource::Validation
                    | ApprovalSource::Danger
                    | ApprovalSource::ReadOnly
                    | ApprovalSource::User
                    | ApprovalSource::Timeout
                    | ApprovalSource::Legacy
                    | ApprovalSource::Recovery
            ),
            RuntimeToolStatus::Success
            | RuntimeToolStatus::Failed
            | RuntimeToolStatus::Cancelled => matches!(
                source,
                ApprovalSource::User
                    | ApprovalSource::Rule
                    | ApprovalSource::Auto
                    | ApprovalSource::Danger
                    | ApprovalSource::Legacy
            ),
        },
        _ => status == RuntimeToolStatus::Rejected && source == ApprovalSource::RunMode,
    }
}

pub(crate) async fn finish_prepared_failure(
    database_path: PathBuf,
    assistant_message_id: String,
) -> Result<(), VegaError> {
    tokio::task::spawn_blocking(move || {
        let store = Store::open(database_path).map_err(VegaError::Store)?;
        messages::finish_streaming(store.conn(), &assistant_message_id, "", "failed")
            .map_err(VegaError::Store)
            .and_then(|updated| ensure_message_updated(updated, &assistant_message_id))
    })
    .await
    .map_err(|error| persistence_actor_error(format!("failure cleanup join failed: {error}")))?
}

pub(crate) fn forward_pipeline_error<F>(
    event_sink: &mut F,
    message_id: Option<String>,
    error: Arc<VegaError>,
) where
    F: FnMut(&ConversationEvent) -> Result<(), VegaError>,
{
    let _ = event_sink(&ConversationEvent::Error { message_id, error });
}
