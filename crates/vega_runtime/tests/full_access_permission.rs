use vega_runtime::{
    RuntimeApprovalAudit, RuntimeApprovalDecision, RuntimeApprovalSource, RuntimeCapabilityOutcome,
    RuntimeDangerFacts, RuntimeExecutePermission, RuntimeMutatingTool, RuntimePermissionMode,
    RuntimePermissionOutcome, RuntimePermissionPrompt, RuntimePermissionTarget, RuntimeRunMode,
    RuntimeToolClass, decide_capability, decide_execute_permission,
};

#[test]
fn full_access_approves_ordinary_mutations_but_prompts_for_danger() {
    let ordinary = RuntimePermissionTarget {
        call_id: "ordinary-call".into(),
        tool: RuntimeMutatingTool::Bash,
        exact_pattern: "mkdir capsule-output".into(),
        display_target: "mkdir capsule-output".into(),
    };
    let dangerous = RuntimePermissionTarget {
        call_id: "dangerous-call".into(),
        tool: RuntimeMutatingTool::Bash,
        exact_pattern: "rm -rf capsule-output".into(),
        display_target: "rm -rf capsule-output".into(),
    };
    let danger = RuntimeDangerFacts {
        rule_id: "recursive-delete".into(),
        reason: "Recursive deletion requires confirmation".into(),
    };
    for (target, danger) in [(ordinary, None), (dangerous, Some(danger))] {
        let RuntimeCapabilityOutcome::ExecuteEligible(eligibility) = decide_capability(
            RuntimeRunMode::Execute,
            RuntimeToolClass::Mutating(target.clone()),
        ) else {
            panic!("Execute mutating calls must reach the permission policy");
        };
        let outcome = decide_execute_permission(
            eligibility,
            RuntimeExecutePermission {
                permission_mode: RuntimePermissionMode::FullAccess,
                target: target.clone(),
                danger: danger.clone(),
                exact_rule_matches: false,
                danger_response: None,
                ordinary_response: None,
            },
        )
        .expect("valid Full Access permission facts");
        if let Some(danger) = danger {
            assert_eq!(
                outcome,
                RuntimePermissionOutcome::Prompt(RuntimePermissionPrompt {
                    target,
                    danger: Some(danger),
                })
            );
        } else {
            assert_eq!(
                outcome,
                RuntimePermissionOutcome::Approved {
                    audit: RuntimeApprovalAudit {
                        decision: RuntimeApprovalDecision::Once,
                        note: None,
                        source: RuntimeApprovalSource::FullAccess,
                        danger: None,
                    },
                    remember_rule: false,
                }
            );
        }
    }
}
