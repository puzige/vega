use super::*;
use mcp_registry::FrozenMcpTool;
use sha2::{Digest, Sha256};

const MCP_ARGUMENT_LIMIT: usize = 256 * 1024;
const MCP_RESULT_LIMIT: usize = 256 * 1024;

pub(crate) enum PreparedRuntimeCall {
    Readonly(RuntimeToolCall),
    InvalidRead(RuntimeToolCall),
    PinnedRead {
        call: RuntimeToolCall,
        execution: RuntimeToolCall,
    },
    ExternalRead {
        call: RuntimeToolCall,
        target: RuntimePermissionTarget,
    },
    MixedRejected(RuntimeToolCall),
    Skill {
        call: RuntimeToolCall,
        action: SkillToolAction,
    },
    InvalidSkill(RuntimeToolCall),
    Mcp {
        call: RuntimeToolCall,
        arguments: Value,
        frozen: FrozenMcpTool,
        prompt: RuntimeMcpPermissionPrompt,
    },
    InvalidMcp {
        call: RuntimeToolCall,
    },
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
        full_access: bool,
    },
    InvalidWriteEdit {
        call: RuntimeToolCall,
        result: String,
    },
    RunModeMutation(RuntimeToolCall),
    InvalidBash {
        call: RuntimeToolCall,
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
            | Self::InvalidRead(call)
            | Self::PinnedRead { call, .. }
            | Self::MixedRejected(call)
            | Self::InvalidSkill(call)
            | Self::Unknown(call)
            | Self::RunModeMutation(call)
            | Self::InvalidWriteEdit { call, .. }
            | Self::InvalidBash { call, .. }
            | Self::InvalidMcp { call }
            | Self::ExternalRead { call, .. }
            | Self::Mcp { call, .. }
            | Self::Write { call, .. }
            | Self::Edit { call, .. }
            | Self::Bash { call, .. } => call,
            Self::Skill { call, .. } => call,
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

/// In a batch containing `load_skill`, operational calls must never reach
/// mutation preparation (which may inspect checkpoint state). Keep only the
/// same bounded audit projection needed for durable rejection.
pub(crate) fn prepare_mixed_rejection_call(
    base_tools: &vega_tools::Tools,
    config: &RuntimeToolConfig,
    capabilities: &RunCapabilitySnapshot,
    raw_call: RuntimeToolCall,
) -> Result<PreparedRuntimeCall, VegaError> {
    if matches!(raw_call.name.as_str(), "Write" | "Edit" | "write" | "edit") {
        let audited = if matches!(raw_call.name.as_str(), "Write" | "write") {
            base_tools.audit_write_json(&raw_call.input_json)
        } else {
            base_tools.audit_edit_json(&raw_call.input_json)
        };
        return match audited {
            Ok(audit) => Ok(PreparedRuntimeCall::MixedRejected(RuntimeToolCall {
                input_json: audit
                    .to_json()
                    .map_err(|_| safe_prepare_error(&raw_call.name))?,
                ..raw_call
            })),
            Err(vega_tools::PrepareMutationError::Invalid(invalid)) => {
                invalid_runtime_call(raw_call, invalid)
            }
            Err(vega_tools::PrepareMutationError::Internal(_)) => {
                Err(safe_prepare_error(&raw_call.name))
            }
        };
    }
    prepare_runtime_call(base_tools, config, capabilities, raw_call)
}

/// Replays use only the frozen safe identity and cannot yield an executable mutation.
pub(crate) fn prepare_completed_mutation(
    tools: &vega_tools::Tools,
    config: &RuntimeToolConfig,
    capabilities: &RunCapabilitySnapshot,
    call: RuntimeToolCall,
    prior: &CompletedToolCall,
) -> Result<PreparedRuntimeCall, VegaError> {
    if prior.tool == call.name
        && let Ok(audit) = vega_tools::WriteEditAudit::from_json(&prior.input_json)
    {
        match tools.audit_replay_json(audit.tool(), &call.input_json, &audit) {
            Ok(replayed) => {
                return Ok(PreparedRuntimeCall::MixedRejected(RuntimeToolCall {
                    input_json: replayed
                        .to_json()
                        .map_err(|_| safe_prepare_error(&call.name))?,
                    ..call
                }));
            }
            Err(vega_tools::PrepareMutationError::Invalid(invalid)) => {
                return invalid_runtime_call(call, invalid);
            }
            Err(vega_tools::PrepareMutationError::Internal(_)) => {
                return Err(safe_prepare_error(&call.name));
            }
        }
    }
    prepare_mixed_rejection_call(tools, config, capabilities, call)
}

#[derive(Clone)]
pub(crate) enum SkillToolAction {
    Load { name: String },
    Read { name: String, path: String },
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
    capabilities: &RunCapabilitySnapshot,
    raw_call: RuntimeToolCall,
) -> Result<PreparedRuntimeCall, VegaError> {
    if matches!(
        raw_call.name.as_str(),
        crate::skills::LOAD_SKILL_TOOL_NAME | crate::skills::READ_SKILL_RESOURCE_TOOL_NAME
    ) {
        return prepare_skill_call(raw_call, capabilities);
    }
    if let Some(frozen) = capabilities.mcp_tool(&raw_call.name) {
        return prepare_mcp_call(raw_call, frozen.clone());
    }
    if raw_call.name.starts_with("mcp_") {
        return Ok(PreparedRuntimeCall::Unknown(RuntimeToolCall {
            input_json: "{}".to_string(),
            ..raw_call
        }));
    }
    match raw_call.name.as_str() {
        "Read" | "read" => {
            // Resolve before approval and replace aliases with the exact target.
            // A failed resolution remains a normal failed Read, never a bypass.
            if let Ok(mut input) = parse_input(&raw_call.name, &raw_call.input_json)
                && validate_read_input(&input).is_ok()
                && let Ok(path) = file_input_path(&input)
                && let Ok(resolved) = base_tools.file_path(path)
            {
                let external = !resolved.starts_with(base_tools.root());
                let target_path = resolved.to_string_lossy().into_owned();
                if let Some(object) = input.as_object_mut() {
                    object.remove("path");
                    object.insert("file_path".into(), Value::String(target_path.clone()));
                }
                let call = RuntimeToolCall {
                    input_json: input.to_string(),
                    ..raw_call.clone()
                };
                if external {
                    let target = RuntimePermissionTarget {
                        call_id: call.id.clone(),
                        tool: RuntimeMutatingTool::Read,
                        exact_pattern: target_path.clone(),
                        display_target: target_path,
                    };
                    return Ok(PreparedRuntimeCall::ExternalRead { call, target });
                }
                return Ok(PreparedRuntimeCall::PinnedRead {
                    call: raw_call,
                    execution: call,
                });
            }
            Ok(PreparedRuntimeCall::InvalidRead(RuntimeToolCall {
                input_json: "{}".into(),
                ..raw_call
            }))
        }
        "glob" | "grep" => Ok(PreparedRuntimeCall::Readonly(raw_call)),
        "Write" | "Edit" | "write" | "edit" => {
            let tool = if matches!(raw_call.name.as_str(), "Write" | "write") {
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
                full_access: config.permission_mode == RuntimePermissionMode::FullAccess,
            }),
            Err(_) => {
                let safe_json = InvalidBashAudit::from_raw(&raw_call.input_json)
                    .and_then(|audit| audit.to_json())
                    .ok_or_else(|| safe_prepare_error("bash"))?;
                Ok(PreparedRuntimeCall::InvalidBash {
                    call: RuntimeToolCall {
                        input_json: safe_json,
                        ..raw_call
                    },
                })
            }
        },
        _ => Ok(PreparedRuntimeCall::Unknown(RuntimeToolCall {
            input_json: "{}".to_string(),
            ..raw_call
        })),
    }
}

fn prepare_skill_call(
    raw_call: RuntimeToolCall,
    capabilities: &RunCapabilitySnapshot,
) -> Result<PreparedRuntimeCall, VegaError> {
    if !capabilities
        .definitions()
        .iter()
        .any(|definition| definition.name == raw_call.name)
    {
        return Ok(PreparedRuntimeCall::Unknown(RuntimeToolCall {
            input_json: "{}".to_string(),
            ..raw_call
        }));
    }
    let parsed = (raw_call.input_json.len() <= 2048)
        .then(|| serde_json::from_str::<Value>(&raw_call.input_json).ok())
        .flatten();
    let Some(object) = parsed.as_ref().and_then(Value::as_object) else {
        return Ok(PreparedRuntimeCall::InvalidSkill(RuntimeToolCall {
            input_json: "{}".to_string(),
            ..raw_call
        }));
    };
    let Some(name) = object.get("name").and_then(Value::as_str).filter(|name| {
        !name.is_empty()
            && name.len() <= 64
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    }) else {
        return Ok(PreparedRuntimeCall::InvalidSkill(RuntimeToolCall {
            input_json: "{}".to_string(),
            ..raw_call
        }));
    };
    let (action, safe_json) = if raw_call.name == crate::skills::LOAD_SKILL_TOOL_NAME {
        if object.len() != 1 {
            return Ok(PreparedRuntimeCall::InvalidSkill(RuntimeToolCall {
                input_json: "{}".to_string(),
                ..raw_call
            }));
        }
        (
            SkillToolAction::Load {
                name: name.to_string(),
            },
            serde_json::json!({"name":name}).to_string(),
        )
    } else {
        let Some(path) = object
            .get("path")
            .and_then(Value::as_str)
            .filter(|path| !path.is_empty() && path.len() <= 1024 && !path.contains('\0'))
        else {
            return Ok(PreparedRuntimeCall::InvalidSkill(RuntimeToolCall {
                input_json: "{}".to_string(),
                ..raw_call
            }));
        };
        if object.len() != 2 {
            return Ok(PreparedRuntimeCall::InvalidSkill(RuntimeToolCall {
                input_json: "{}".to_string(),
                ..raw_call
            }));
        }
        let path_sha256 = format!("{:x}", Sha256::digest(path.as_bytes()));
        (
            SkillToolAction::Read {
                name: name.to_string(),
                path: path.to_string(),
            },
            serde_json::json!({"name":name,"path_bytes":path.len(),"path_sha256":path_sha256})
                .to_string(),
        )
    };
    Ok(PreparedRuntimeCall::Skill {
        call: RuntimeToolCall {
            input_json: safe_json,
            ..raw_call
        },
        action,
    })
}

fn prepare_mcp_call(
    raw_call: RuntimeToolCall,
    frozen: FrozenMcpTool,
) -> Result<PreparedRuntimeCall, VegaError> {
    let raw_bytes = raw_call.input_json.as_bytes();
    let digest = Sha256::digest(raw_bytes);
    let digest_hex = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let parsed = (raw_bytes.len() <= MCP_ARGUMENT_LIMIT)
        .then(|| serde_json::from_str::<Value>(&raw_call.input_json).ok())
        .flatten()
        .filter(Value::is_object);
    let field_preview = parsed.as_ref().and_then(Value::as_object).map(|object| {
        let mut fields = object
            .iter()
            .take(16)
            .map(|(key, value)| {
                let safe_key = if key.len() <= 64
                    && key
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
                {
                    key.as_str()
                } else {
                    "[redacted field]"
                };
                let kind = match value {
                    Value::Null => "null",
                    Value::Bool(_) => "boolean",
                    Value::Number(_) => "number",
                    Value::String(_) => "string",
                    Value::Array(_) => "array",
                    Value::Object(_) => "object",
                };
                format!("{safe_key}: {kind}")
            })
            .collect::<Vec<_>>();
        if object.len() > fields.len() {
            fields.push("…".to_string());
        }
        fields.join(", ")
    });
    let safe_json = serde_json::json!({
        "server_id": frozen.server_id(),
        "config_revision": frozen.config_revision(),
        "tool": frozen.exact_tool_name(),
        "arguments_bytes": raw_bytes.len(),
        "arguments_sha256": digest_hex,
        "argument_preview": field_preview.as_deref().unwrap_or("invalid arguments"),
    })
    .to_string();
    let call = RuntimeToolCall {
        input_json: safe_json,
        ..raw_call
    };
    let Some(arguments) = parsed else {
        return Ok(PreparedRuntimeCall::InvalidMcp { call });
    };
    let prompt = RuntimeMcpPermissionPrompt {
        call_id: call.id.clone(),
        server_id: frozen.server_id().to_string(),
        config_revision: frozen.config_revision(),
        exact_tool_name: frozen.exact_tool_name().to_string(),
        arguments_bytes: raw_bytes.len(),
        arguments_sha256: digest_hex,
        argument_preview: field_preview.unwrap_or_default(),
    };
    Ok(PreparedRuntimeCall::Mcp {
        call,
        arguments,
        frozen,
        prompt,
    })
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
        PreparedRuntimeCall::MixedRejected(call) => Ok(Authorization::Terminal(terminal_result(
            call,
            "Tool error: mixed Skill load batch rejected other tools".to_string(),
            RuntimeToolStatus::Rejected,
            Some(validation_audit()),
        ))),
        PreparedRuntimeCall::InvalidSkill(call) => Ok(Authorization::Terminal(terminal_result(
            call,
            "Tool error: invalid Skill input".to_string(),
            RuntimeToolStatus::Rejected,
            Some(validation_audit()),
        ))),
        PreparedRuntimeCall::Skill { .. } => {
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
        PreparedRuntimeCall::InvalidMcp { call } => Ok(Authorization::Terminal(terminal_result(
            call,
            "Tool error: invalid or oversized MCP arguments".to_string(),
            RuntimeToolStatus::Rejected,
            Some(validation_audit()),
        ))),
        PreparedRuntimeCall::Mcp {
            call,
            frozen,
            prompt,
            ..
        } => {
            if config.run_mode != RuntimeRunMode::Execute
                || config.permission_mode == RuntimePermissionMode::ReadOnly
            {
                return Ok(Authorization::Terminal(terminal_result(
                    call,
                    "Tool error: denied by run mode".to_string(),
                    RuntimeToolStatus::Rejected,
                    Some(run_mode_denial()),
                )));
            }
            if cancel.is_cancelled() || frozen.is_revoked() {
                return Ok(cancelled_permission(call));
            }
            let decision =
                wait_for_mcp_permission(hook, prompt.clone(), config.permission_timeout, cancel)
                    .await;
            let audit = match decision {
                RuntimeUserDecision::Once => RuntimeApprovalAudit {
                    decision: RuntimeApprovalDecision::Once,
                    note: None,
                    source: RuntimeApprovalSource::User,
                    danger: None,
                },
                RuntimeUserDecision::Deny { .. } | RuntimeUserDecision::Always => {
                    RuntimeApprovalAudit {
                        decision: RuntimeApprovalDecision::Deny,
                        note: None,
                        source: RuntimeApprovalSource::User,
                        danger: None,
                    }
                }
                RuntimeUserDecision::Timeout => RuntimeApprovalAudit {
                    decision: RuntimeApprovalDecision::Deny,
                    note: None,
                    source: RuntimeApprovalSource::Timeout,
                    danger: None,
                },
            };
            if audit.decision == RuntimeApprovalDecision::Once
                && !frozen.is_revoked()
                && !cancel.is_cancelled()
            {
                Ok(Authorization::Approved {
                    audit,
                    remember_rule: None,
                })
            } else {
                Ok(Authorization::Terminal(terminal_result(
                    call,
                    "Tool error: MCP call not approved".to_string(),
                    RuntimeToolStatus::Rejected,
                    Some(audit),
                )))
            }
        }
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
        PreparedRuntimeCall::InvalidBash { call } => Ok(Authorization::Terminal(terminal_result(
            call,
            BASH_INVALID_INPUT_OUTPUT.to_string(),
            RuntimeToolStatus::Rejected,
            Some(validation_audit()),
        ))),
        PreparedRuntimeCall::Unknown(call) => Ok(Authorization::Terminal(terminal_result(
            call,
            "Tool error: denied: unavailable tool".to_string(),
            RuntimeToolStatus::Rejected,
            Some(run_mode_denial()),
        ))),
        PreparedRuntimeCall::ExternalRead { call, target } => {
            if cancel.is_cancelled() {
                return Ok(cancelled_permission(call));
            }
            let known = exact_rules.contains(&RuntimeExactRule {
                tool: RuntimeMutatingTool::Read,
                pattern: target.exact_pattern.clone(),
            });
            let (decision, source) = if config.permission_mode == RuntimePermissionMode::FullAccess
            {
                (RuntimeUserDecision::Once, RuntimeApprovalSource::FullAccess)
            } else if known {
                (RuntimeUserDecision::Always, RuntimeApprovalSource::Rule)
            } else {
                (
                    wait_for_permission(
                        hook,
                        RuntimePermissionPrompt {
                            target: target.clone(),
                            danger: None,
                        },
                        config.permission_timeout,
                        cancel,
                    )
                    .await
                    .0,
                    RuntimeApprovalSource::User,
                )
            };
            let (decision, note, source, remember) = match decision {
                RuntimeUserDecision::Once => (RuntimeApprovalDecision::Once, None, source, false),
                RuntimeUserDecision::Always => (
                    RuntimeApprovalDecision::Always,
                    None,
                    source,
                    source == RuntimeApprovalSource::User,
                ),
                RuntimeUserDecision::Deny { note } => {
                    (RuntimeApprovalDecision::Deny, note, source, false)
                }
                RuntimeUserDecision::Timeout => (
                    RuntimeApprovalDecision::Deny,
                    None,
                    RuntimeApprovalSource::Timeout,
                    false,
                ),
            };
            let audit = RuntimeApprovalAudit {
                decision,
                note,
                source,
                danger: None,
            };
            if cancel.is_cancelled() {
                return Ok(cancelled_permission(call));
            }
            if decision == RuntimeApprovalDecision::Deny {
                Ok(Authorization::Terminal(terminal_result(
                    call,
                    "Tool error: permission denied".into(),
                    RuntimeToolStatus::Rejected,
                    Some(audit),
                )))
            } else {
                Ok(Authorization::Approved {
                    audit,
                    remember_rule: remember.then(|| target.clone()),
                })
            }
        }
        PreparedRuntimeCall::Readonly(_)
        | PreparedRuntimeCall::InvalidRead(_)
        | PreparedRuntimeCall::PinnedRead { .. } => {
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
                target.clone(),
                if config.permission_mode == RuntimePermissionMode::Auto
                    && target.tool != RuntimeMutatingTool::Bash
                    && std::path::Path::new(&target.exact_pattern).is_absolute()
                {
                    RuntimePermissionMode::Confirm
                } else {
                    config.permission_mode
                },
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

async fn wait_for_mcp_permission(
    hook: &dyn RuntimePermissionHook,
    prompt: RuntimeMcpPermissionPrompt,
    timeout: Duration,
    cancel: &CancellationToken,
) -> RuntimeUserDecision {
    let prompt_cancel = cancel.child_token();
    let future = hook.request_mcp(prompt, prompt_cancel.clone());
    let decision = tokio::select! {
        biased;
        _ = cancel.cancelled() => RuntimeUserDecision::Timeout,
        _ = tokio::time::sleep(timeout) => RuntimeUserDecision::Timeout,
        response = future => response.unwrap_or(RuntimeUserDecision::Timeout),
    };
    prompt_cancel.cancel();
    decision
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
    if tool == "bash"
        && let (Some(left), Some(right)) = (
            InvalidBashAudit::from_json(left),
            InvalidBashAudit::from_json(right),
        )
    {
        return left == right;
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

pub(crate) async fn execute_prepared_waiting(
    prepared: PreparedRuntimeCall,
    base_tools: &vega_tools::Tools,
    cancel: &CancellationToken,
) -> (RuntimeToolResult, bool) {
    match prepared {
        PreparedRuntimeCall::MixedRejected(call) => (
            terminal_result(
                &call,
                "Tool error: mixed Skill load batch rejected other tools".to_string(),
                RuntimeToolStatus::Rejected,
                Some(validation_audit()),
            ),
            false,
        ),
        PreparedRuntimeCall::Skill { call, .. } | PreparedRuntimeCall::InvalidSkill(call) => (
            terminal_result(
                &call,
                "Tool error: Skill dispatcher unavailable".to_string(),
                RuntimeToolStatus::Failed,
                None,
            ),
            false,
        ),
        PreparedRuntimeCall::InvalidMcp { call } => (
            terminal_result(
                &call,
                "Tool error: invalid or oversized MCP arguments".to_string(),
                RuntimeToolStatus::Rejected,
                Some(validation_audit()),
            ),
            false,
        ),
        PreparedRuntimeCall::Mcp {
            call,
            arguments,
            frozen,
            ..
        } => {
            if frozen.is_revoked() || cancel.is_cancelled() {
                return (
                    terminal_result(
                        &call,
                        CANCELLED_BEFORE_EXECUTION_OUTPUT.to_string(),
                        RuntimeToolStatus::Cancelled,
                        None,
                    ),
                    true,
                );
            }
            let call_cancel = cancel.child_token();
            let revoked = frozen.revoked_token();
            let dispatch = frozen.dispatch(arguments, call_cancel.clone());
            tokio::pin!(dispatch);
            let executed = tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    call_cancel.cancel();
                    dispatch.await
                },
                _ = revoked.cancelled() => {
                    call_cancel.cancel();
                    dispatch.await
                },
                result = &mut dispatch => result,
            };
            call_cancel.cancel();
            // A reply racing with revoke is not evidence that the configured
            // authority was still live when the remote side acted. Never
            // publish its content as a successful trusted completion.
            if cancel.is_cancelled() || revoked.is_cancelled() {
                return (
                    terminal_result(
                        &call,
                        "Tool error: MCP call outcome unknown after cancellation".to_string(),
                        RuntimeToolStatus::Cancelled,
                        None,
                    ),
                    true,
                );
            }
            match executed {
                Ok(output) => match frozen.safe_result_text(&output) {
                    Ok(text) if text.len() <= MCP_RESULT_LIMIT => {
                        let status = if output.is_error {
                            RuntimeToolStatus::Failed
                        } else {
                            RuntimeToolStatus::Success
                        };
                        let mut result = terminal_result(
                            &call,
                            format!("[Untrusted external MCP result]\n{text}"),
                            status,
                            None,
                        );
                        if status == RuntimeToolStatus::Success {
                            result.truncated = Some(false);
                        }
                        (result, false)
                    }
                    _ => (
                        terminal_result(
                            &call,
                            "Tool error: MCP result failed or exceeded limit".to_string(),
                            RuntimeToolStatus::Failed,
                            None,
                        ),
                        false,
                    ),
                },
                Err(error) => (
                    terminal_result(
                        &call,
                        error.safe_output().to_string(),
                        if matches!(error, mcp_registry::McpDispatchFailure::Cancelled) {
                            RuntimeToolStatus::Cancelled
                        } else {
                            RuntimeToolStatus::Failed
                        },
                        None,
                    ),
                    false,
                ),
            }
        }
        PreparedRuntimeCall::PinnedRead { execution, .. } => {
            execute_readonly_waiting(base_tools, &execution, cancel).await
        }
        PreparedRuntimeCall::InvalidRead(call) => (
            failed_tool_result(
                &call,
                "Read requires a valid file_path string identifying a regular text file".into(),
            ),
            false,
        ),
        PreparedRuntimeCall::Readonly(call) | PreparedRuntimeCall::ExternalRead { call, .. } => {
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
            full_access,
        } => {
            let result = if full_access {
                tools
                    .execute_bash_full_access(prepared, cancel.child_token())
                    .await
            } else {
                tools.execute_bash(prepared, cancel.child_token()).await
            };
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
        PreparedRuntimeCall::InvalidBash { call } => (
            terminal_result(
                &call,
                BASH_INVALID_INPUT_OUTPUT.to_string(),
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
        Err(error) => terminal_result(
            call,
            match error {
                vega_tools::ToolError::Mutation(error) => format!("Tool error: {error}"),
                _ => format!("Tool error: {} failed", call.name),
            },
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
            "Read" | "read" => {
                validate_read_input(&input)?;
                let path = file_input_path(&input)?;
                if std::path::Path::new(path).is_absolute()
                    && tools
                        .file_path(path)
                        .map_err(|_| "Read target is unavailable")?
                        != std::path::Path::new(path)
                {
                    return Err("Read target changed after approval; retry Read".to_string());
                }
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

fn validate_read_input(input: &Value) -> Result<(), String> {
    let Some(object) = input.as_object() else {
        return Err("Read input must be an object".into());
    };
    if object
        .keys()
        .any(|key| !matches!(key.as_str(), "file_path" | "path" | "offset" | "limit"))
    {
        return Err("Read accepts only file_path, offset and limit".into());
    }
    file_input_path(input)?;
    if optional_usize(input, "offset")? == Some(0) {
        return Err("Read offset must be one-based".into());
    }
    optional_usize(input, "limit")?;
    Ok(())
}

fn file_input_path(input: &Value) -> Result<&str, String> {
    if input.get("file_path").is_some() && input.get("path").is_some() {
        return Err("Use file_path only; do not supply both file_path and legacy path".to_string());
    }
    required_str(
        input,
        if input.get("path").is_some() {
            "path"
        } else {
            "file_path"
        },
    )
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

/// Returns the exact primary-wire tool schemas for one run mode.  Conversation
/// services use the same authority when estimating/manual-compacting a request
/// so controllers never reconstruct a drifted schema locally.
pub fn tool_definitions(run_mode: RuntimeRunMode) -> Vec<ToolDefinition> {
    let mut definitions = vec![
        ToolDefinition {
            name: "Read".to_string(),
            description: "Read a text file using an absolute file_path, including files outside the current project (subject to permission). Output has line numbers; omit those prefixes when editing. Read a file before Edit or overwriting with Write. Offset is one-based; null offset/limit uses the default range.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "file_path": { "type": "string", "description": "Absolute path to the file to read." },
                    "offset": { "type": ["integer", "null"], "minimum": 1 },
                    "limit": { "type": ["integer", "null"], "minimum": 0 }
                },
                "required": ["file_path", "offset", "limit"],
                "additionalProperties": false
            }),
            strict: true,
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
            strict: true,
        },
        ToolDefinition {
            name: "grep".to_string(),
            description: "Search project files with a regular expression.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string" },
                    "path": { "type": ["string", "null"] }
                },
                "required": ["pattern", "path"],
                "additionalProperties": false
            }),
            strict: true,
        },
    ];
    if run_mode == RuntimeRunMode::Execute {
        definitions.extend([
            ToolDefinition {
                name: "Write".to_string(),
                description: "Write content to an absolute file_path after permission approval. For an existing file, Read it first; changed files must be read again. Parent directories are created for new files. Prefer Edit for targeted changes.".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "file_path": { "type": "string", "description": "Absolute path to the file to write." },
                        "content": { "type": "string" }
                    },
                    "required": ["file_path", "content"],
                    "additionalProperties": false
                }),
                strict: true,
            },
            ToolDefinition {
                name: "Edit".to_string(),
                description: "Replace old_string with new_string in an absolute file_path after permission approval. Read the file first. Preserve exact indentation and omit Read line-number prefixes. old_string must uniquely identify the source; include more surrounding context if ambiguous, or set replace_all=true to replace every occurrence. new_string must differ from old_string. replace_all omitted or null defaults to false."
                    .to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "file_path": { "type": "string", "description": "Absolute path to the file to edit." },
                        "old_string": { "type": "string" },
                        "new_string": { "type": "string" },
                        "replace_all": { "type": ["boolean", "null"], "description": "Replace every occurrence; null means false." }
                    },
                    "required": ["file_path", "old_string", "new_string", "replace_all"],
                    "additionalProperties": false
                }),
                strict: true,
            },
            ToolDefinition {
                name: "bash".to_string(),
                description: "Run at project root after approval (sandboxed unless Full access). Use {\"cmd\":\"rg ...\"}; command is unsupported."
                    .to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "cmd": { "type": "string" },
                        "timeout_ms": { "type": ["integer", "null"], "minimum": 1 }
                    },
                    "required": ["cmd", "timeout_ms"],
                    "additionalProperties": false
                }),
                strict: true,
            },
        ]);
    }
    definitions
}
