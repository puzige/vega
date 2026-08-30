//! Pure permission decisions for mutating tools.
//!
//! This module deliberately contains no execution capability. An approved
//! decision authorizes a later prepared-tool call; bash still performs its
//! mandatory spawn-time sandbox and hardlink preflight in `vega_tools`.

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
}

/// Mutating tool vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RuntimeMutatingTool {
    /// Sandboxed shell command.
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
    /// Exact S4 bare compatibility value loaded from persistence.
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

/// Applies the fixed Execute order: danger → ReadOnly → rule → Auto → Confirm.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn target(tool: RuntimeMutatingTool) -> RuntimePermissionTarget {
        let exact_pattern = match tool {
            RuntimeMutatingTool::Bash => "cargo  test".to_string(),
            RuntimeMutatingTool::Write | RuntimeMutatingTool::Edit => "src/lib.rs".to_string(),
        };
        RuntimePermissionTarget {
            call_id: "call-1".into(),
            tool,
            display_target: exact_pattern.clone(),
            exact_pattern,
        }
    }

    fn facts(tool: RuntimeMutatingTool, mode: RuntimePermissionMode) -> RuntimeExecutePermission {
        RuntimeExecutePermission {
            permission_mode: mode,
            target: target(tool),
            danger: None,
            exact_rule_matches: false,
            danger_response: None,
            ordinary_response: None,
        }
    }

    fn decide(
        input: RuntimeExecutePermission,
    ) -> Result<RuntimePermissionOutcome, RuntimePermissionError> {
        let eligibility = match decide_capability(
            RuntimeRunMode::Execute,
            RuntimeToolClass::Mutating(input.target.clone()),
        ) {
            RuntimeCapabilityOutcome::ExecuteEligible(eligibility) => eligibility,
            _ => panic!("Execute mutating call must be eligible"),
        };
        decide_execute_permission(eligibility, input)
    }

    fn audit(outcome: &RuntimePermissionOutcome) -> Option<&RuntimeApprovalAudit> {
        match outcome {
            RuntimePermissionOutcome::Approved { audit, .. }
            | RuntimePermissionOutcome::Rejected { audit, .. } => Some(audit),
            RuntimePermissionOutcome::Prompt(_) => None,
        }
    }

    #[test]
    fn capability_matrix_precedes_danger() {
        for mode in [
            RuntimeRunMode::Ask,
            RuntimeRunMode::Plan,
            RuntimeRunMode::Execute,
        ] {
            let readonly = decide_capability(mode, RuntimeToolClass::Readonly);
            assert_eq!(
                audit(&match readonly {
                    RuntimeCapabilityOutcome::Approved(audit) =>
                        RuntimePermissionOutcome::Approved {
                            audit,
                            remember_rule: false,
                        },
                    _ => panic!("readonly must approve"),
                })
                .map(|value| value.source),
                Some(RuntimeApprovalSource::ReadonlyTool)
            );
            for tool in [
                RuntimeMutatingTool::Bash,
                RuntimeMutatingTool::Write,
                RuntimeMutatingTool::Edit,
            ] {
                let expected_target = target(tool);
                let outcome =
                    decide_capability(mode, RuntimeToolClass::Mutating(expected_target.clone()));
                if mode == RuntimeRunMode::Execute {
                    assert_eq!(
                        outcome,
                        RuntimeCapabilityOutcome::ExecuteEligible(RuntimeExecuteEligibility {
                            target: expected_target
                        })
                    );
                } else {
                    assert!(matches!(
                        outcome,
                        RuntimeCapabilityOutcome::Rejected(RuntimeApprovalAudit {
                            decision: RuntimeApprovalDecision::Deny,
                            source: RuntimeApprovalSource::RunMode,
                            danger: None,
                            ..
                        })
                    ));
                }
            }
        }
    }

    #[test]
    fn ask_and_plan_never_produce_execute_eligibility() {
        for mode in [RuntimeRunMode::Ask, RuntimeRunMode::Plan] {
            for tool in [
                RuntimeMutatingTool::Bash,
                RuntimeMutatingTool::Write,
                RuntimeMutatingTool::Edit,
            ] {
                assert!(matches!(
                    decide_capability(mode, RuntimeToolClass::Mutating(target(tool))),
                    RuntimeCapabilityOutcome::Rejected(_)
                ));
            }
        }
    }

    #[test]
    fn execute_eligibility_rejects_cross_call_and_cross_pattern_reuse() {
        let original = target(RuntimeMutatingTool::Bash);
        let eligibility = match decide_capability(
            RuntimeRunMode::Execute,
            RuntimeToolClass::Mutating(original.clone()),
        ) {
            RuntimeCapabilityOutcome::ExecuteEligible(eligibility) => eligibility,
            _ => panic!("Execute bash must be eligible"),
        };
        let mut cross_call = facts(RuntimeMutatingTool::Bash, RuntimePermissionMode::Auto);
        cross_call.target.call_id = "call-2".into();
        assert_eq!(
            decide_execute_permission(eligibility, cross_call),
            Err(RuntimePermissionError::InvalidTarget)
        );

        let eligibility = match decide_capability(
            RuntimeRunMode::Execute,
            RuntimeToolClass::Mutating(original),
        ) {
            RuntimeCapabilityOutcome::ExecuteEligible(eligibility) => eligibility,
            _ => panic!("Execute bash must be eligible"),
        };
        let mut cross_pattern = facts(RuntimeMutatingTool::Bash, RuntimePermissionMode::Auto);
        cross_pattern.target.exact_pattern = "cargo test --all".into();
        cross_pattern.target.display_target = "cargo test --all".into();
        assert_eq!(
            decide_execute_permission(eligibility, cross_pattern),
            Err(RuntimePermissionError::InvalidTarget)
        );
    }

    #[test]
    fn execute_modes_apply_readonly_rule_auto_then_confirm() {
        for tool in [
            RuntimeMutatingTool::Bash,
            RuntimeMutatingTool::Write,
            RuntimeMutatingTool::Edit,
        ] {
            let outcome = decide(facts(tool, RuntimePermissionMode::ReadOnly)).unwrap();
            assert_eq!(
                audit(&outcome).map(|a| a.source),
                Some(RuntimeApprovalSource::ReadOnly)
            );

            for mode in [RuntimePermissionMode::Confirm, RuntimePermissionMode::Auto] {
                let mut input = facts(tool, mode);
                input.exact_rule_matches = true;
                let outcome = decide(input).unwrap();
                assert!(matches!(
                    outcome,
                    RuntimePermissionOutcome::Approved {
                        audit: RuntimeApprovalAudit {
                            decision: RuntimeApprovalDecision::Always,
                            source: RuntimeApprovalSource::Rule,
                            ..
                        },
                        remember_rule: false
                    }
                ));
            }

            let outcome = decide(facts(tool, RuntimePermissionMode::Auto)).unwrap();
            assert_eq!(
                audit(&outcome).map(|a| a.source),
                Some(RuntimeApprovalSource::Auto)
            );
            assert!(matches!(
                decide(facts(tool, RuntimePermissionMode::Confirm)).unwrap(),
                RuntimePermissionOutcome::Prompt(RuntimePermissionPrompt { danger: None, .. })
            ));
        }
    }

    #[test]
    fn danger_precedes_readonly_rules_and_auto() {
        for mode in [
            RuntimePermissionMode::ReadOnly,
            RuntimePermissionMode::Confirm,
            RuntimePermissionMode::Auto,
        ] {
            let mut input = facts(RuntimeMutatingTool::Bash, mode);
            input.danger = Some(RuntimeDangerFacts {
                rule_id: "git-force-push".into(),
                reason: "rewrites history".into(),
            });
            input.exact_rule_matches = true;
            assert!(matches!(
                decide(input).unwrap(),
                RuntimePermissionOutcome::Prompt(RuntimePermissionPrompt {
                    danger: Some(_),
                    ..
                })
            ));
        }
    }

    #[test]
    fn dangerous_responses_record_nested_audit_and_never_second_prompt() {
        for tool in [
            RuntimeMutatingTool::Bash,
            RuntimeMutatingTool::Write,
            RuntimeMutatingTool::Edit,
        ] {
            for decision in [RuntimeUserDecision::Once, RuntimeUserDecision::Always] {
                let expected = if matches!(decision, RuntimeUserDecision::Always) {
                    RuntimeApprovalDecision::Always
                } else {
                    RuntimeApprovalDecision::Once
                };
                let mut input = facts(tool, RuntimePermissionMode::Confirm);
                input.danger = Some(RuntimeDangerFacts {
                    rule_id: "danger".into(),
                    reason: "reason".into(),
                });
                input.danger_response = Some(decision);
                let outcome = decide(input).unwrap();
                assert!(matches!(outcome, RuntimePermissionOutcome::Approved { .. }));
                let audit = audit(&outcome).unwrap();
                assert_eq!(audit.decision, expected);
                assert_eq!(audit.source, RuntimeApprovalSource::Danger);
                assert_eq!(audit.danger.as_ref().map(|d| d.decision), Some(expected));
            }
        }
    }

    #[test]
    fn danger_readonly_preserves_confirmation_and_always_intent() {
        let mut input = facts(RuntimeMutatingTool::Bash, RuntimePermissionMode::ReadOnly);
        input.danger = Some(RuntimeDangerFacts {
            rule_id: "danger".into(),
            reason: "reason".into(),
        });
        input.danger_response = Some(RuntimeUserDecision::Always);
        let outcome = decide(input).unwrap();
        assert!(matches!(
            outcome,
            RuntimePermissionOutcome::Rejected {
                audit: RuntimeApprovalAudit {
                    decision: RuntimeApprovalDecision::Deny,
                    source: RuntimeApprovalSource::ReadOnly,
                    danger: Some(RuntimeDangerAudit {
                        decision: RuntimeApprovalDecision::Always,
                        ..
                    }),
                    ..
                },
                remember_rule: true
            }
        ));
    }

    #[test]
    fn once_always_deny_note_and_timeout_are_audited() {
        for (decision, expected, source, remember) in [
            (
                RuntimeUserDecision::Once,
                RuntimeApprovalDecision::Once,
                RuntimeApprovalSource::User,
                false,
            ),
            (
                RuntimeUserDecision::Always,
                RuntimeApprovalDecision::Always,
                RuntimeApprovalSource::User,
                true,
            ),
            (
                RuntimeUserDecision::Deny {
                    note: Some("no".into()),
                },
                RuntimeApprovalDecision::Deny,
                RuntimeApprovalSource::User,
                false,
            ),
            (
                RuntimeUserDecision::Timeout,
                RuntimeApprovalDecision::Deny,
                RuntimeApprovalSource::Timeout,
                false,
            ),
        ] {
            let mut input = facts(RuntimeMutatingTool::Edit, RuntimePermissionMode::Confirm);
            input.ordinary_response = Some(decision);
            let outcome = decide(input).unwrap();
            let actual = audit(&outcome).unwrap();
            assert_eq!(actual.decision, expected);
            assert_eq!(actual.source, source);
            assert_eq!(
                matches!(
                    outcome,
                    RuntimePermissionOutcome::Approved {
                        remember_rule: true,
                        ..
                    }
                ),
                remember
            );
        }
    }

    #[test]
    fn exact_signatures_are_not_normalized_by_the_engine() {
        let mut first = facts(RuntimeMutatingTool::Bash, RuntimePermissionMode::Confirm);
        let mut second = first.clone();
        second.target.exact_pattern = "cargo test".into();
        assert_ne!(first.target.exact_pattern, second.target.exact_pattern);
        first.target.exact_pattern = "Cargo  test".into();
        assert_ne!(first.target.exact_pattern, second.target.exact_pattern);
        assert_eq!(
            target(RuntimeMutatingTool::Write).exact_pattern,
            "src/lib.rs"
        );
        assert_eq!(
            target(RuntimeMutatingTool::Edit).exact_pattern,
            "src/lib.rs"
        );
    }

    #[test]
    fn inconsistent_bundles_fail_closed() {
        let mut danger_without_facts =
            facts(RuntimeMutatingTool::Bash, RuntimePermissionMode::Auto);
        danger_without_facts.danger_response = Some(RuntimeUserDecision::Once);
        assert_eq!(
            decide(danger_without_facts),
            Err(RuntimePermissionError::InconsistentResponse)
        );

        let mut ordinary_for_danger =
            facts(RuntimeMutatingTool::Bash, RuntimePermissionMode::Confirm);
        ordinary_for_danger.danger = Some(RuntimeDangerFacts {
            rule_id: "d".into(),
            reason: "r".into(),
        });
        ordinary_for_danger.ordinary_response = Some(RuntimeUserDecision::Once);
        assert_eq!(
            decide(ordinary_for_danger),
            Err(RuntimePermissionError::InconsistentResponse)
        );

        let mut response_after_rule =
            facts(RuntimeMutatingTool::Write, RuntimePermissionMode::Confirm);
        response_after_rule.exact_rule_matches = true;
        response_after_rule.ordinary_response = Some(RuntimeUserDecision::Always);
        assert_eq!(
            decide(response_after_rule),
            Err(RuntimePermissionError::InconsistentResponse)
        );

        let mut invalid_target = facts(RuntimeMutatingTool::Edit, RuntimePermissionMode::Auto);
        invalid_target.target.exact_pattern.clear();
        assert_eq!(
            decide(invalid_target),
            Err(RuntimePermissionError::InvalidTarget)
        );

        let mut mismatched_target = facts(RuntimeMutatingTool::Bash, RuntimePermissionMode::Auto);
        mismatched_target.target.display_target = "echo harmless".into();
        assert_eq!(
            decide(mismatched_target),
            Err(RuntimePermissionError::InvalidTarget)
        );
    }
}
