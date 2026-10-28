use vega_runtime::*;

fn decide(
    mode: RuntimePermissionMode,
    danger: bool,
    exact_rule_matches: bool,
    response: Option<RuntimeUserDecision>,
) -> RuntimePermissionOutcome {
    let target = RuntimePermissionTarget {
        call_id: "call".into(),
        tool: RuntimeMutatingTool::Bash,
        exact_pattern: "git push --force-with-lease".into(),
        display_target: "git push --force-with-lease".into(),
    };
    let RuntimeCapabilityOutcome::ExecuteEligible(eligibility) = decide_capability(
        RuntimeRunMode::Execute,
        RuntimeToolClass::Mutating(target.clone()),
    ) else {
        panic!("execute capability rejected");
    };
    decide_execute_permission(
        eligibility,
        RuntimeExecutePermission {
            permission_mode: mode,
            target,
            danger: danger.then(|| RuntimeDangerFacts {
                rule_id: "git-force-push".into(),
                reason: "force push".into(),
            }),
            exact_rule_matches,
            danger_response: if danger { response.clone() } else { None },
            ordinary_response: if danger { None } else { response },
        },
    )
    .unwrap()
}

#[test]
fn full_access_ordinary_call_is_approved_without_a_prompt() {
    let RuntimePermissionOutcome::Approved {
        audit,
        remember_rule,
    } = decide(RuntimePermissionMode::FullAccess, false, false, None)
    else {
        panic!("ordinary Full access requested approval");
    };
    assert_eq!(audit.source, RuntimeApprovalSource::FullAccess);
    assert_eq!(audit.decision, RuntimeApprovalDecision::Once);
    assert!(audit.danger.is_none());
    assert!(!remember_rule);
}

#[test]
fn danger_requires_confirmation_even_with_automatic_policy_or_exact_rule() {
    for mode in [
        RuntimePermissionMode::FullAccess,
        RuntimePermissionMode::Auto,
        RuntimePermissionMode::Confirm,
    ] {
        for exact in [false, true] {
            let RuntimePermissionOutcome::Prompt(prompt) = decide(mode, true, exact, None) else {
                panic!("danger confirmation bypassed");
            };
            assert_eq!(prompt.danger.unwrap().rule_id, "git-force-push");
            let RuntimePermissionOutcome::Approved {
                audit,
                remember_rule,
            } = decide(mode, true, exact, Some(RuntimeUserDecision::Once))
            else {
                panic!("danger approval required a second confirmation");
            };
            assert_eq!(audit.source, RuntimeApprovalSource::Danger);
            assert_eq!(audit.decision, RuntimeApprovalDecision::Once);
            assert_eq!(
                audit.danger.unwrap().decision,
                RuntimeApprovalDecision::Once
            );
            assert!(!remember_rule);
        }
    }
}

#[test]
fn permission_failures_remain_distinct_and_never_authorize_or_remember() {
    for danger in [false, true] {
        for (response, source) in [
            (
                RuntimeUserDecision::Deny { note: None },
                if danger {
                    RuntimeApprovalSource::Danger
                } else {
                    RuntimeApprovalSource::User
                },
            ),
            (RuntimeUserDecision::Timeout, RuntimeApprovalSource::Timeout),
            (
                RuntimeUserDecision::Cancelled,
                RuntimeApprovalSource::Cancelled,
            ),
            (
                RuntimeUserDecision::Unavailable,
                RuntimeApprovalSource::Unavailable,
            ),
        ] {
            let mode = if danger {
                RuntimePermissionMode::FullAccess
            } else {
                RuntimePermissionMode::Confirm
            };
            let RuntimePermissionOutcome::Rejected {
                audit,
                remember_rule,
            } = decide(mode, danger, false, Some(response))
            else {
                panic!("failure authorized or reprompted");
            };
            assert_eq!(audit.source, source);
            assert_eq!(audit.decision, RuntimeApprovalDecision::Deny);
            assert!(audit.note.is_none());
            assert!(!remember_rule);
            assert_eq!(audit.danger.is_some(), danger);
            if let Some(nested) = audit.danger {
                assert_eq!(nested.rule_id, "git-force-push");
                assert_eq!(nested.decision, RuntimeApprovalDecision::Deny);
                assert!(nested.note.is_none());
            }
        }
    }
}
