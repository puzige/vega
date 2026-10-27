use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Decisions {
    prompts: AtomicUsize,
}

impl RuntimePermissionHook for Decisions {
    fn request(
        &self,
        _prompt: RuntimePermissionPrompt,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<RuntimeUserDecision, VegaError>> {
        self.prompts.fetch_add(1, Ordering::SeqCst);
        async { Ok(RuntimeUserDecision::Always) }.boxed()
    }
}

#[tokio::test]
async fn bash_preparation_and_authorization_preserve_readonly_across_all_run_modes_and_rules() {
    let tools = vega_tools::Tools::new(std::env::temp_dir()).expect("existing temporary root");
    for run_mode in [
        RuntimeRunMode::Ask,
        RuntimeRunMode::Plan,
        RuntimeRunMode::Execute,
    ] {
        for permission_mode in [
            RuntimePermissionMode::ReadOnly,
            RuntimePermissionMode::Confirm,
            RuntimePermissionMode::Auto,
            RuntimePermissionMode::FullAccess,
        ] {
            let expected = match (run_mode, permission_mode) {
                (RuntimeRunMode::Execute, RuntimePermissionMode::FullAccess) => {
                    vega_tools::BashLaunchPolicy::FullAccess
                }
                (
                    RuntimeRunMode::Execute,
                    RuntimePermissionMode::Confirm | RuntimePermissionMode::Auto,
                ) => vega_tools::BashLaunchPolicy::WorkspaceWrite,
                _ => vega_tools::BashLaunchPolicy::ReadOnly,
            };
            for command in ["printf fixture", "rm -rf /"] {
                let dangerous = command == "rm -rf /";
                for remembered in [false, true] {
                    let rule = RuntimeExactRule {
                        tool: RuntimeMutatingTool::Bash,
                        pattern: command.into(),
                    };
                    let rules = if remembered { vec![rule] } else { Vec::new() };
                    let config = RuntimeToolConfig::new(
                        run_mode,
                        permission_mode,
                        "fixture-project".into(),
                        "fixture-thread".into(),
                        std::env::temp_dir(),
                        rules.clone(),
                    );
                    let capabilities =
                        RunCapabilitySnapshot::freeze(run_mode, permission_mode, Vec::new())
                            .expect("frozen capabilities");
                    let names = capabilities
                        .definitions()
                        .iter()
                        .map(|value| value.name.as_str())
                        .collect::<Vec<_>>();
                    assert_eq!(names.iter().filter(|name| **name == "bash").count(), 1);
                    assert_eq!(
                        names.contains(&"write"),
                        run_mode == RuntimeRunMode::Execute
                    );
                    assert_eq!(names.contains(&"edit"), run_mode == RuntimeRunMode::Execute);
                    let prepared = prepare_runtime_call(
                        &tools,
                        &config,
                        &capabilities,
                        RuntimeToolCall {
                            id: "fixture-call".into(),
                            name: "bash".into(),
                            input_json: serde_json::json!({"command": command, "timeout": 3})
                                .to_string(),
                        },
                    )
                    .expect("prepared Bash");
                    let PreparedRuntimeCall::Bash {
                        policy,
                        prepared: bash,
                        ..
                    } = &prepared
                    else {
                        panic!("Bash must be prepared in every run mode");
                    };
                    assert_eq!(*policy, expected);
                    assert_eq!(bash.command(), command);
                    assert_eq!(bash.timeout_ms(), 3_000);
                    let hook = Decisions {
                        prompts: AtomicUsize::new(0),
                    };
                    let result = authorize_call(
                        &prepared,
                        &config,
                        &rules.into_iter().collect(),
                        &hook,
                        &CancellationToken::new(),
                    )
                    .await
                    .expect("authorization");
                    if expected == vega_tools::BashLaunchPolicy::ReadOnly {
                        assert_eq!(hook.prompts.load(Ordering::SeqCst), 0);
                        if dangerous {
                            let Authorization::Terminal(result) = result else {
                                panic!("danger must be rejected")
                            };
                            assert_eq!(result.status, RuntimeToolStatus::Rejected);
                            assert!(result.remember_rule.is_none());
                            let audit = result.approval.expect("audit");
                            assert_eq!(audit.source, RuntimeApprovalSource::SandboxReadOnly);
                            assert_eq!(audit.decision, RuntimeApprovalDecision::Deny);
                            assert_eq!(
                                audit.danger.expect("danger fact").decision,
                                RuntimeApprovalDecision::Deny
                            );
                        } else {
                            let Authorization::Approved {
                                audit,
                                remember_rule,
                            } = result
                            else {
                                panic!("safe readonly Bash must be allowed")
                            };
                            assert_eq!(audit.source, RuntimeApprovalSource::SandboxReadOnly);
                            assert_eq!(audit.decision, RuntimeApprovalDecision::Once);
                            assert!(audit.danger.is_none());
                            assert!(remember_rule.is_none());
                        }
                    } else {
                        let Authorization::Approved { audit, .. } = result else {
                            panic!("approved writable Bash")
                        };
                        if dangerous {
                            assert_eq!(audit.source, RuntimeApprovalSource::Danger);
                            assert_eq!(hook.prompts.load(Ordering::SeqCst), 1);
                        } else if remembered {
                            assert_eq!(audit.source, RuntimeApprovalSource::Rule);
                            assert_eq!(hook.prompts.load(Ordering::SeqCst), 0);
                        } else {
                            let source = match permission_mode {
                                RuntimePermissionMode::Confirm => RuntimeApprovalSource::User,
                                RuntimePermissionMode::Auto => RuntimeApprovalSource::Auto,
                                RuntimePermissionMode::FullAccess => {
                                    RuntimeApprovalSource::FullAccess
                                }
                                RuntimePermissionMode::ReadOnly => unreachable!(),
                            };
                            assert_eq!(audit.source, source);
                            assert_eq!(
                                hook.prompts.load(Ordering::SeqCst),
                                usize::from(permission_mode == RuntimePermissionMode::Confirm)
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn readonly_bash_decisions_do_not_authorize_file_mutations_or_accept_invalid_targets() {
    for tool in [RuntimeMutatingTool::Write, RuntimeMutatingTool::Edit] {
        let target = RuntimePermissionTarget {
            call_id: "fixture".into(),
            tool,
            exact_pattern: "file.txt".into(),
            display_target: "file.txt".into(),
        };
        assert!(crate::permission::decide_readonly_bash(target.clone(), None).is_err());
        for mode in [RuntimeRunMode::Ask, RuntimeRunMode::Plan] {
            assert!(matches!(
                decide_capability(mode, RuntimeToolClass::Mutating(target.clone())),
                RuntimeCapabilityOutcome::Rejected(_)
            ));
        }
        let RuntimeCapabilityOutcome::ExecuteEligible(eligibility) = decide_capability(
            RuntimeRunMode::Execute,
            RuntimeToolClass::Mutating(target.clone()),
        ) else {
            panic!("execute eligibility")
        };
        let outcome = decide_execute_permission(
            eligibility,
            RuntimeExecutePermission {
                permission_mode: RuntimePermissionMode::ReadOnly,
                target,
                danger: None,
                exact_rule_matches: true,
                danger_response: None,
                ordinary_response: None,
            },
        )
        .expect("readonly file decision");
        assert!(matches!(
            outcome,
            RuntimePermissionOutcome::Rejected {
                remember_rule: false,
                ..
            }
        ));
    }
    let target = RuntimePermissionTarget {
        call_id: String::new(),
        tool: RuntimeMutatingTool::Bash,
        exact_pattern: "pwd".into(),
        display_target: "pwd".into(),
    };
    assert!(crate::permission::decide_readonly_bash(target, None).is_err());
}
