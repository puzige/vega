use vega_runtime::*;

fn decide(mode: RuntimePermissionMode, dangerous: bool) -> RuntimePermissionOutcome {
    let target = RuntimePermissionTarget {
        call_id: "call".into(),
        tool: RuntimeMutatingTool::Bash,
        exact_pattern: "command".into(),
        display_target: "command".into(),
    };
    let RuntimeCapabilityOutcome::ExecuteEligible(eligibility) = decide_capability(
        RuntimeRunMode::Execute,
        RuntimeToolClass::Mutating(target.clone()),
    ) else {
        panic!("Execute eligibility missing")
    };
    decide_execute_permission(
        eligibility,
        RuntimeExecutePermission {
            permission_mode: mode,
            target,
            danger: dangerous.then(|| RuntimeDangerFacts {
                rule_id: "danger".into(),
                reason: "reason".into(),
            }),
            exact_rule_matches: false,
            danger_response: None,
            ordinary_response: None,
        },
    )
    .unwrap()
}

#[test]
fn full_access_approves_without_claiming_user_danger_confirmation() {
    for dangerous in [false, true] {
        let RuntimePermissionOutcome::Approved {
            audit,
            remember_rule,
        } = decide(RuntimePermissionMode::FullAccess, dangerous)
        else {
            panic!("Full access prompted or rejected")
        };
        assert_eq!(audit.source, RuntimeApprovalSource::FullAccess);
        assert_eq!(audit.decision, RuntimeApprovalDecision::Once);
        assert!(audit.danger.is_none());
        assert!(!remember_rule);
    }
}

#[test]
fn restricted_modes_keep_their_decision_paths() {
    for mode in [
        RuntimePermissionMode::Confirm,
        RuntimePermissionMode::Auto,
        RuntimePermissionMode::ReadOnly,
    ] {
        assert!(matches!(
            decide(mode, true),
            RuntimePermissionOutcome::Prompt(_)
        ));
    }
    assert!(matches!(
        decide(RuntimePermissionMode::Confirm, false),
        RuntimePermissionOutcome::Prompt(_)
    ));
    assert!(matches!(
        decide(RuntimePermissionMode::Auto, false),
        RuntimePermissionOutcome::Approved { .. }
    ));
    assert!(matches!(
        decide(RuntimePermissionMode::ReadOnly, false),
        RuntimePermissionOutcome::Rejected { .. }
    ));
}

#[test]
fn ask_and_plan_cannot_enter_execute_authority() {
    for mode in [RuntimeRunMode::Ask, RuntimeRunMode::Plan] {
        assert!(matches!(
            decide_capability(
                mode,
                RuntimeToolClass::Mutating(RuntimePermissionTarget {
                    call_id: "call".into(),
                    tool: RuntimeMutatingTool::Bash,
                    exact_pattern: "command".into(),
                    display_target: "command".into(),
                })
            ),
            RuntimeCapabilityOutcome::Rejected(_)
        ));
    }
}
