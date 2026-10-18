//! Pure permission decisions for mutating tools.
//!
//! This module deliberately contains no execution capability. An approved
//! decision authorizes a later prepared-tool call; only FullAccess selects
//! direct shell execution instead of the default sandbox in `vega_tools`.

/// Thread run mode used by the headless permission boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeRunMode {
    /// Read-only question answering.
    Ask,
    /// Read-only plan generation.
    Plan,
    /// Tool execution subject to permissions.
    Execute,
}

/// Execute-mode permission policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimePermissionMode {
    /// Reject all mutating tools.
    ReadOnly,
    /// Ask before mutations.
    Confirm,
    /// Approve non-dangerous mutations automatically.
    Auto,
    /// Approve ordinary mutations and run bash without the OS sandbox.
    FullAccess,
}

/// Mutating tool vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RuntimeMutatingTool {
    /// External read requiring access approval, without mutation capability.
    Read,
    /// Shell command subject to the selected execution policy.
    Bash,
    /// Fenced file write.
    Write,
    /// Fenced unique-match edit.
    Edit,
}

impl RuntimeMutatingTool {
    /// Stable tool name used by permission persistence.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Bash => "bash",
            Self::Write => "write",
            Self::Edit => "edit",
        }
    }
}

/// Capability class checked before danger detection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeToolClass {
    /// `read|glob|grep`.
    Readonly,
    /// `bash|write|edit`.
    Mutating(RuntimePermissionTarget),
}

/// Content-free exact authorization target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimePermissionTarget {
    /// Provider call id.
    pub call_id: String,
    /// Mutating tool.
    pub tool: RuntimeMutatingTool,
    /// Exact command or normalized relative path persisted as a rule.
    pub exact_pattern: String,
    /// Safe target displayed to the user.
    pub display_target: String,
}

/// Stable danger classification supplied by `vega_tools::danger`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeDangerFacts {
    /// Stable centralized danger rule id.
    pub rule_id: String,
    /// Stable user-facing reason.
    pub reason: String,
}

/// User response to a permission prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeUserDecision {
    /// Allow once.
    Once,
    /// Allow and remember the exact signature.
    Always,
    /// Reject, optionally with a user note.
    Deny { note: Option<String> },
    /// Permission wait expired or disappeared.
    Timeout,
}

/// Persistable approval decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeApprovalDecision {
    /// Approved once.
    Once,
    /// Approved and remembered.
    Always,
    /// Denied.
    Deny,
}

/// Source of a runtime permission outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeApprovalSource {
    ComputerUse,
    /// Explicit dangerous-command decision.
    Danger,
    /// Read-only permission policy.
    ReadOnly,
    /// Ask/Plan capability gate.
    RunMode,
    /// Exact persisted rule.
    Rule,
    /// Auto policy.
    Auto,
    /// Explicit Full access policy.
    FullAccess,
    /// Ordinary user confirmation.
    User,
    /// Permission timeout.
    Timeout,
    /// Invalid tool input rejected without permission or execution.
    Validation,
    /// Read-only built-in tool.
    ReadonlyTool,
    /// Startup recovery.
    Recovery,
    Legacy,
}

/// Nested dangerous-command audit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeDangerAudit {
    /// Stable danger rule id.
    pub rule_id: String,
    /// Decision made on the danger prompt.
    pub decision: RuntimeApprovalDecision,
    /// Optional denial note.
    pub note: Option<String>,
}

/// Content-free runtime approval audit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeApprovalAudit {
    /// Final authorization decision.
    pub decision: RuntimeApprovalDecision,
    /// Optional denial note.
    pub note: Option<String>,
    /// Decision source.
    pub source: RuntimeApprovalSource,
    /// Nested danger decision when a danger prompt occurred.
    pub danger: Option<RuntimeDangerAudit>,
}

/// Prompt facts emitted by the pure engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimePermissionPrompt {
    /// Safe exact target.
    pub target: RuntimePermissionTarget,
    /// Danger facts for a danger prompt; `None` for ordinary confirmation.
    pub danger: Option<RuntimeDangerFacts>,
}

/// Content-safe one-call prompt for an external MCP capability. No server
/// annotation can turn this into a remembered built-in mutation rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeMcpPermissionPrompt {
    /// Provider call id.
    pub call_id: String,
    /// Vega-owned stable server id, not the server's display name.
    pub server_id: String,
    /// Exact configuration revision frozen for this run.
    pub config_revision: u64,
    /// Exact name advertised by this server.
    pub exact_tool_name: String,
    /// UTF-8 bytes in the model-proposed arguments.
    pub arguments_bytes: usize,
    /// SHA-256 digest of the exact argument bytes.
    pub arguments_sha256: String,
    /// Bounded field/type summary; argument values are not included.
    pub argument_preview: String,
}

/// Opaque proof that capability step -1 admitted an Execute mutating call.
///
/// Its fields are private, so callers can obtain it only from
/// [`decide_capability`] with `Execute + Mutating`.
#[derive(Debug, PartialEq, Eq)]
pub struct RuntimeExecuteEligibility {
    target: RuntimePermissionTarget,
}

/// Result of capability step -1.
#[derive(Debug, PartialEq, Eq)]
pub enum RuntimeCapabilityOutcome {
    /// Read-only tool is authorized.
    Approved(RuntimeApprovalAudit),
    /// Mutating tool is rejected in Ask/Plan.
    Rejected(RuntimeApprovalAudit),
    /// Mutating tool may enter the Execute permission engine.
    ExecuteEligible(RuntimeExecuteEligibility),
}

/// Facts for one Execute-mode mutating decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeExecutePermission {
    /// Thread permission policy.
    pub permission_mode: RuntimePermissionMode,
    /// Content-free authorization target.
    pub target: RuntimePermissionTarget,
    /// Centralized danger classification, if matched.
    pub danger: Option<RuntimeDangerFacts>,
    /// Whether the exact project/tool/pattern rule exists.
    pub exact_rule_matches: bool,
    /// Response to a danger prompt.
    pub danger_response: Option<RuntimeUserDecision>,
    /// Response to an ordinary Confirm prompt.
    pub ordinary_response: Option<RuntimeUserDecision>,
}

/// Pure authorization result. This type has no spawn or preflight bypass flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimePermissionOutcome {
    /// A user decision is required.
    Prompt(RuntimePermissionPrompt),
    /// Authorization completed.
    Approved {
        /// Persistable audit.
        audit: RuntimeApprovalAudit,
        /// Persist the exact rule after the audit is durably stored.
        remember_rule: bool,
    },
    /// Authorization rejected.
    Rejected {
        /// Persistable audit.
        audit: RuntimeApprovalAudit,
        /// Persist an explicitly selected `Always` rule even if ReadOnly rejects.
        remember_rule: bool,
    },
}

/// Invalid or inconsistent pure-engine facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RuntimePermissionError {
    /// Target does not contain a usable call id/signature/display value.
    #[error("invalid permission target")]
    InvalidTarget,
    /// Danger facts are incomplete.
    #[error("invalid danger facts")]
    InvalidDangerFacts,
    /// A response was supplied to the wrong prompt channel or decision step.
    #[error("inconsistent permission response")]
    InconsistentResponse,
}

/// Applies capability step -1 before any danger matching.
pub fn decide_capability(
    run_mode: RuntimeRunMode,
    tool_class: RuntimeToolClass,
) -> RuntimeCapabilityOutcome {
    match tool_class {
        RuntimeToolClass::Readonly => RuntimeCapabilityOutcome::Approved(RuntimeApprovalAudit {
            decision: RuntimeApprovalDecision::Once,
            note: None,
            source: RuntimeApprovalSource::ReadonlyTool,
            danger: None,
        }),
        RuntimeToolClass::Mutating(target) if run_mode == RuntimeRunMode::Execute => {
            RuntimeCapabilityOutcome::ExecuteEligible(RuntimeExecuteEligibility { target })
        }
        RuntimeToolClass::Mutating(_) => RuntimeCapabilityOutcome::Rejected(RuntimeApprovalAudit {
            decision: RuntimeApprovalDecision::Deny,
            note: None,
            source: RuntimeApprovalSource::RunMode,
            danger: None,
        }),
    }
}

/// Applies Execute order: danger → ReadOnly → rule → Auto/FullAccess → Confirm.
pub fn decide_execute_permission(
    eligibility: RuntimeExecuteEligibility,
    facts: RuntimeExecutePermission,
) -> Result<RuntimePermissionOutcome, RuntimePermissionError> {
    if eligibility.target != facts.target {
        return Err(RuntimePermissionError::InvalidTarget);
    }
    validate_facts(&facts)?;
    if let Some(danger) = &facts.danger {
        return decide_danger(
            facts.permission_mode,
            facts.target,
            danger,
            facts.danger_response,
        );
    }

    if facts.permission_mode == RuntimePermissionMode::ReadOnly {
        return Ok(rejected(RuntimeApprovalSource::ReadOnly, None, None, false));
    }
    if facts.exact_rule_matches {
        return Ok(approved(
            RuntimeApprovalDecision::Always,
            RuntimeApprovalSource::Rule,
            None,
            None,
            false,
        ));
    }
    match facts.permission_mode {
        RuntimePermissionMode::Auto => Ok(approved(
            RuntimeApprovalDecision::Once,
            RuntimeApprovalSource::Auto,
            None,
            None,
            false,
        )),
        RuntimePermissionMode::FullAccess => Ok(approved(
            RuntimeApprovalDecision::Once,
            RuntimeApprovalSource::FullAccess,
            None,
            None,
            false,
        )),
        RuntimePermissionMode::Confirm => match facts.ordinary_response {
            None => Ok(RuntimePermissionOutcome::Prompt(RuntimePermissionPrompt {
                target: facts.target,
                danger: None,
            })),
            Some(response) => Ok(outcome_from_user(
                response,
                RuntimeApprovalSource::User,
                None,
            )),
        },
        RuntimePermissionMode::ReadOnly => {
            Ok(rejected(RuntimeApprovalSource::ReadOnly, None, None, false))
        }
    }
}

fn validate_facts(facts: &RuntimeExecutePermission) -> Result<(), RuntimePermissionError> {
    if facts.target.call_id.is_empty()
        || facts.target.exact_pattern.is_empty()
        || facts.target.display_target.is_empty()
        || facts.target.exact_pattern != facts.target.display_target
    {
        return Err(RuntimePermissionError::InvalidTarget);
    }
    if let Some(danger) = &facts.danger
        && (danger.rule_id.is_empty() || danger.reason.is_empty())
    {
        return Err(RuntimePermissionError::InvalidDangerFacts);
    }
    if facts.danger.is_some() {
        if facts.ordinary_response.is_some() {
            return Err(RuntimePermissionError::InconsistentResponse);
        }
    } else if facts.danger_response.is_some() {
        return Err(RuntimePermissionError::InconsistentResponse);
    }
    if facts.danger.is_none()
        && facts.ordinary_response.is_some()
        && (facts.permission_mode != RuntimePermissionMode::Confirm || facts.exact_rule_matches)
    {
        return Err(RuntimePermissionError::InconsistentResponse);
    }
    Ok(())
}

fn decide_danger(
    mode: RuntimePermissionMode,
    target: RuntimePermissionTarget,
    danger: &RuntimeDangerFacts,
    response: Option<RuntimeUserDecision>,
) -> Result<RuntimePermissionOutcome, RuntimePermissionError> {
    let Some(response) = response else {
        return Ok(RuntimePermissionOutcome::Prompt(RuntimePermissionPrompt {
            target,
            danger: Some(danger.clone()),
        }));
    };
    let (decision, note, source, remember_rule) =
        user_parts(response, RuntimeApprovalSource::Danger);
    let danger_audit = RuntimeDangerAudit {
        rule_id: danger.rule_id.clone(),
        decision,
        note: note.clone(),
    };
    if decision == RuntimeApprovalDecision::Deny {
        return Ok(rejected(source, note, Some(danger_audit), false));
    }
    if mode == RuntimePermissionMode::ReadOnly {
        return Ok(rejected(
            RuntimeApprovalSource::ReadOnly,
            None,
            Some(danger_audit),
            remember_rule,
        ));
    }
    Ok(approved(
        decision,
        RuntimeApprovalSource::Danger,
        note,
        Some(danger_audit),
        remember_rule,
    ))
}

fn outcome_from_user(
    response: RuntimeUserDecision,
    default_source: RuntimeApprovalSource,
    danger: Option<RuntimeDangerAudit>,
) -> RuntimePermissionOutcome {
    let (decision, note, source, remember_rule) = user_parts(response, default_source);
    if decision == RuntimeApprovalDecision::Deny {
        rejected(source, note, danger, false)
    } else {
        approved(decision, source, note, danger, remember_rule)
    }
}

fn user_parts(
    response: RuntimeUserDecision,
    default_source: RuntimeApprovalSource,
) -> (
    RuntimeApprovalDecision,
    Option<String>,
    RuntimeApprovalSource,
    bool,
) {
    match response {
        RuntimeUserDecision::Once => (RuntimeApprovalDecision::Once, None, default_source, false),
        RuntimeUserDecision::Always => {
            (RuntimeApprovalDecision::Always, None, default_source, true)
        }
        RuntimeUserDecision::Deny { note } => {
            (RuntimeApprovalDecision::Deny, note, default_source, false)
        }
        RuntimeUserDecision::Timeout => (
            RuntimeApprovalDecision::Deny,
            None,
            RuntimeApprovalSource::Timeout,
            false,
        ),
    }
}

fn approved(
    decision: RuntimeApprovalDecision,
    source: RuntimeApprovalSource,
    note: Option<String>,
    danger: Option<RuntimeDangerAudit>,
    remember_rule: bool,
) -> RuntimePermissionOutcome {
    RuntimePermissionOutcome::Approved {
        audit: RuntimeApprovalAudit {
            decision,
            note,
            source,
            danger,
        },
        remember_rule,
    }
}

fn rejected(
    source: RuntimeApprovalSource,
    note: Option<String>,
    danger: Option<RuntimeDangerAudit>,
    remember_rule: bool,
) -> RuntimePermissionOutcome {
    RuntimePermissionOutcome::Rejected {
        audit: RuntimeApprovalAudit {
            decision: RuntimeApprovalDecision::Deny,
            note,
            source,
            danger,
        },
        remember_rule,
    }
}
