use super::{
    Approval, ApprovalAudit, ApprovalCodecError, ApprovalSource, DangerAudit, PermissionDecision,
    PermissionMode, approval_audit_from_runtime, permission_decision_to_runtime,
    permission_request_from_runtime,
};
use vega_runtime::{
    RuntimeApprovalAudit, RuntimeApprovalDecision, RuntimeApprovalSource, RuntimeDangerAudit,
    RuntimeDangerFacts, RuntimeMutatingTool, RuntimePermissionPrompt, RuntimePermissionTarget,
    RuntimeUserDecision,
};

fn audit(decision: Approval, source: ApprovalSource, danger: Option<DangerAudit>) -> ApprovalAudit {
    ApprovalAudit {
        decision,
        note: None,
        source,
        danger,
    }
}

#[test]
fn permission_mode_round_trips_and_rejects_unknown_values() {
    for (raw, mode) in [
        ("readonly", PermissionMode::ReadOnly),
        ("confirm", PermissionMode::Confirm),
        ("auto", PermissionMode::Auto),
    ] {
        assert_eq!(PermissionMode::parse(raw), Some(mode));
        assert_eq!(mode.as_str(), raw);
    }
    for raw in ["", "Auto", "yolo", " confirm"] {
        assert_eq!(PermissionMode::parse(raw), None);
    }
}

#[test]
fn every_legal_structured_audit_round_trips() {
    let danger_once = DangerAudit {
        rule_id: "danger-rule".into(),
        decision: Approval::Once,
        note: None,
    };
    let danger_always = DangerAudit {
        rule_id: "danger-rule".into(),
        decision: Approval::Always,
        note: None,
    };
    let danger_deny = DangerAudit {
        rule_id: "danger-rule".into(),
        decision: Approval::Deny,
        note: None,
    };
    let cases = [
        audit(
            Approval::Once,
            ApprovalSource::Danger,
            Some(danger_once.clone()),
        ),
        audit(
            Approval::Always,
            ApprovalSource::Danger,
            Some(danger_always.clone()),
        ),
        audit(
            Approval::Deny,
            ApprovalSource::Danger,
            Some(danger_deny.clone()),
        ),
        audit(Approval::Deny, ApprovalSource::ReadOnly, None),
        audit(
            Approval::Deny,
            ApprovalSource::ReadOnly,
            Some(danger_always),
        ),
        audit(Approval::Deny, ApprovalSource::RunMode, None),
        audit(Approval::Always, ApprovalSource::Rule, None),
        audit(Approval::Once, ApprovalSource::Auto, None),
        audit(Approval::Once, ApprovalSource::User, None),
        audit(Approval::Always, ApprovalSource::User, None),
        ApprovalAudit {
            decision: Approval::Deny,
            note: Some("not now".into()),
            source: ApprovalSource::User,
            danger: None,
        },
        audit(Approval::Deny, ApprovalSource::Timeout, None),
        audit(Approval::Deny, ApprovalSource::Timeout, Some(danger_deny)),
        audit(Approval::Deny, ApprovalSource::Validation, None),
        audit(Approval::Once, ApprovalSource::ReadonlyTool, None),
        audit(Approval::Deny, ApprovalSource::Recovery, None),
    ];
    for expected in cases {
        let json = expected.to_json().unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value.as_object().map(|object| object.len()), Some(4));
        if let Some(danger) = value.get("danger").and_then(|value| value.as_object()) {
            assert_eq!(danger.len(), 3);
        }
        assert_eq!(ApprovalAudit::from_json(&json).unwrap(), expected);
    }
}

#[test]
fn recovery_encoding_is_the_canonical_exact_value() {
    let json = audit(Approval::Deny, ApprovalSource::Recovery, None)
        .to_json()
        .unwrap();
    assert_eq!(json, vega_store::recovery::RECOVERY_DENIAL_APPROVAL_JSON);
}

#[test]
fn exact_field_sets_and_scalar_types_fail_closed() {
    for raw in [
        r#"{"decision":"once","note":null,"source":"user"}"#,
        r#"{"decision":"once","note":null,"source":"user","danger":null,"extra":1}"#,
        r#"{"decision":"once","decision":"deny","note":null,"source":"user","danger":null}"#,
        r#"{"decision":"once","note":[],"source":"user","danger":null}"#,
        r#"{"decision":"once","note":null,"source":"danger","danger":{"rule_id":"d","decision":"once"}}"#,
        r#"{"decision":"once","note":null,"source":"danger","danger":{"rule_id":"d","decision":"once","note":null,"extra":1}}"#,
        r#"{"decision":"once","note":null,"source":"danger","danger":{"rule_id":"d","decision":"once","decision":"deny","note":null}}"#,
        r#"{"decision":"once","note":null,"source":"danger","danger":{"rule_id":"d","decision":"once","note":[]}}"#,
        r#""once""#,
        "[]",
        "1",
        "true",
        "{",
    ] {
        assert!(ApprovalAudit::from_json(raw).is_err(), "{raw}");
    }
}

#[test]
fn unknown_and_semantically_impossible_values_fail_closed() {
    for raw in [
        r#"{"decision":"later","note":null,"source":"user","danger":null}"#,
        r#"{"decision":"once","note":null,"source":"unknown","danger":null}"#,
        r#"{"decision":"once","note":null,"source":"legacy","danger":null}"#,
        r#"{"decision":"once","note":null,"source":"danger","danger":null}"#,
        r#"{"decision":"once","note":null,"source":"rule","danger":null}"#,
        r#"{"decision":"always","note":null,"source":"auto","danger":null}"#,
        r#"{"decision":"once","note":"not valid","source":"user","danger":null}"#,
        r#"{"decision":"deny","note":null,"source":"readonly_tool","danger":null}"#,
        r#"{"decision":"once","note":null,"source":"danger","danger":{"rule_id":"","decision":"once","note":null}}"#,
        r#"{"decision":"once","note":null,"source":"danger","danger":{"rule_id":"d","decision":"deny","note":null}}"#,
        r#"{"decision":"deny","note":null,"source":"readonly","danger":{"rule_id":"d","decision":"deny","note":null}}"#,
    ] {
        assert!(ApprovalAudit::from_json(raw).is_err(), "{raw}");
    }
}

#[test]
fn legacy_bare_values_are_read_only_and_exact() {
    for (raw, decision) in [
        ("once", Approval::Once),
        ("always", Approval::Always),
        ("deny", Approval::Deny),
    ] {
        let decoded = ApprovalAudit::from_json(raw).unwrap();
        assert_eq!(decoded.decision, decision);
        assert_eq!(decoded.source, ApprovalSource::Legacy);
        assert!(matches!(
            decoded.to_json(),
            Err(ApprovalCodecError::LegacyWrite)
        ));
    }
    for raw in [" once", "once ", "Once", "DENY", "\nonce"] {
        assert!(ApprovalAudit::from_json(raw).is_err(), "{raw:?}");
    }
}

#[test]
fn runtime_prompt_and_audit_mapping_are_field_exact_and_content_free() {
    let prompt = RuntimePermissionPrompt {
        target: RuntimePermissionTarget {
            call_id: "call-1".into(),
            tool: RuntimeMutatingTool::Write,
            exact_pattern: "src/lib.rs".into(),
            display_target: "src/lib.rs".into(),
        },
        danger: Some(RuntimeDangerFacts {
            rule_id: "rule-1".into(),
            reason: "reason".into(),
        }),
    };
    let request = permission_request_from_runtime(&prompt);
    assert_eq!(request.call_id, "call-1");
    assert_eq!(request.tool, "write");
    assert_eq!(request.display_target, "src/lib.rs");
    assert_eq!(request.danger_rule_id.as_deref(), Some("rule-1"));
    assert_eq!(request.danger_reason.as_deref(), Some("reason"));
    assert!(!request.display_target.contains("content"));

    let runtime = RuntimeApprovalAudit {
        decision: RuntimeApprovalDecision::Deny,
        note: None,
        source: RuntimeApprovalSource::ReadOnly,
        danger: Some(RuntimeDangerAudit {
            rule_id: "rule-1".into(),
            decision: RuntimeApprovalDecision::Always,
            note: None,
        }),
    };
    let shared = approval_audit_from_runtime(&runtime);
    assert_eq!(shared.decision, Approval::Deny);
    assert_eq!(shared.source, ApprovalSource::ReadOnly);
    assert_eq!(
        shared.danger.as_ref().map(|danger| danger.decision),
        Some(Approval::Always)
    );
    assert!(shared.to_json().is_ok());
}

#[test]
fn shared_ui_decisions_map_one_way_to_runtime() {
    assert_eq!(
        permission_decision_to_runtime(PermissionDecision::Once),
        RuntimeUserDecision::Once
    );
    assert_eq!(
        permission_decision_to_runtime(PermissionDecision::Always),
        RuntimeUserDecision::Always
    );
    assert_eq!(
        permission_decision_to_runtime(PermissionDecision::Deny {
            note: Some("no".into())
        }),
        RuntimeUserDecision::Deny {
            note: Some("no".into())
        }
    );
    assert_eq!(
        permission_decision_to_runtime(PermissionDecision::Timeout),
        RuntimeUserDecision::Timeout
    );
}
