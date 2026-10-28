use super::*;
use crate::history::{HistoryEntry, latest_history_page};
use crate::types::{
    InterruptedAssistant, PermissionFence, ToolCallStatus, ToolCardResultProjection,
};
use vega_runtime::{
    RuntimeApprovalAudit, RuntimeApprovalDecision, RuntimeApprovalSource, RuntimeCapabilityOutcome,
    RuntimeDangerFacts, RuntimeExecutePermission, RuntimePermissionOutcome,
    RuntimePermissionTarget, RuntimeToolCall, RuntimeToolClass, RuntimeToolResult,
    decide_capability, decide_execute_permission,
};

const FORCE_PUSH: &str = "git push --force-with-lease=refs/heads/topic:0123456789012345678901234567890123456789 origin HEAD:topic";

fn target(command: &str) -> RuntimePermissionTarget {
    RuntimePermissionTarget {
        call_id: "call_permission".into(),
        tool: RuntimeMutatingTool::Bash,
        exact_pattern: command.into(),
        display_target: command.into(),
    }
}

fn input(command: &str) -> String {
    serde_json::json!({"command": command, "timeout": 120}).to_string()
}

fn decision(
    command: &str,
    mode: RuntimePermissionMode,
    exact_rule_matches: bool,
    response: Option<RuntimeUserDecision>,
) -> RuntimePermissionOutcome {
    let target = target(command);
    let RuntimeCapabilityOutcome::ExecuteEligible(eligibility) = decide_capability(
        RuntimeRunMode::Execute,
        RuntimeToolClass::Mutating(target.clone()),
    ) else {
        panic!("execute must admit the permission decision");
    };
    let danger = vega_tools::danger::detect_danger(command)
        .unwrap()
        .map(|danger| RuntimeDangerFacts {
            rule_id: danger.rule_id.into(),
            reason: danger.reason.into(),
        });
    decide_execute_permission(
        eligibility,
        RuntimeExecutePermission {
            permission_mode: mode,
            target,
            danger,
            exact_rule_matches,
            danger_response: response,
            ordinary_response: None,
        },
    )
    .unwrap()
}

fn full_access_audit(command: &str) -> RuntimeApprovalAudit {
    let RuntimePermissionOutcome::Approved {
        audit,
        remember_rule,
    } = decision(command, RuntimePermissionMode::FullAccess, false, None)
    else {
        panic!("full access must approve without another prompt");
    };
    assert!(!remember_rule);
    assert_eq!(audit.decision, RuntimeApprovalDecision::Once);
    assert_eq!(audit.source, RuntimeApprovalSource::FullAccess);
    assert_eq!(audit.note, None);
    assert_eq!(audit.danger, None);
    audit
}

fn pending(command: &str) -> tool_calls::ToolCallState {
    tool_calls::ToolCallState {
        thread_id: ulid::Ulid::generate().to_string(),
        tool: "bash".into(),
        input_json: input(command),
        status: "pending_approval".into(),
        approval: None,
        output_text: None,
        exit_code: None,
        duration_ms: None,
        output_full_path: None,
    }
}

struct Fixture {
    store: Store,
    project_id: String,
    thread_id: String,
    message_id: String,
    next_tool_seq: i64,
}

impl Fixture {
    fn new() -> Self {
        let store = Store::open(":memory:").unwrap();
        store.migrate().unwrap();
        let workspace = std::env::current_dir().unwrap().canonicalize().unwrap();
        let project = vega_store::projects::create(
            store.conn(),
            workspace.to_str().unwrap(),
            "permission regression",
            None,
        )
        .unwrap();
        let thread_id = ulid::Ulid::generate().to_string();
        vega_store::threads::create(
            store.conn(),
            vega_store::threads::NewThread {
                id: &thread_id,
                project_id: &project.id,
                title: "permission regression",
                mode: "execute",
                permission_mode: "full_access",
                permission_revision: 0,
                full_access_grant: None,
                model: "test-model",
                status: "active",
                pinned: false,
                unread: false,
                created_at: 1,
                updated_at: 1,
            },
        )
        .unwrap();
        let message_id = ulid::Ulid::generate().to_string();
        messages::insert(
            store.conn(),
            &messages::MessageRow {
                id: message_id.clone(),
                thread_id: thread_id.clone(),
                seq: 1,
                role: "assistant".into(),
                kind: "text".into(),
                content: String::new(),
                status: "streaming".into(),
                created_at: 1,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .unwrap();
        Self {
            store,
            project_id: project.id,
            thread_id,
            message_id,
            next_tool_seq: 1,
        }
    }

    fn persist(&mut self, event: RuntimeEvent) {
        persist_runtime_event(
            &self.store,
            &self.project_id,
            &self.thread_id,
            &self.message_id,
            "test-model",
            false,
            "",
            &mut self.next_tool_seq,
            &event,
            None,
        )
        .unwrap();
    }

    fn fence(&self) -> PermissionFence {
        let thread = vega_store::threads::find(self.store.conn(), &self.thread_id)
            .unwrap()
            .unwrap();
        crate::permission_contracts::capture_fence(&self.store, &thread).unwrap()
    }

    fn interrupt(&self, fence: Option<&PermissionFence>) -> InterruptedAssistant {
        if let Some(fence) = fence {
            let tx = self.store.immediate_transaction().unwrap();
            vega_store::permission_contracts::record_run_fence(
                &tx,
                &self.message_id,
                &self.thread_id,
                &serde_json::to_string(fence).unwrap(),
            )
            .unwrap();
            tx.commit().unwrap();
        }
        assert_eq!(
            messages::finish_streaming(
                self.store.conn(),
                &self.message_id,
                "",
                "interrupted",
                Some(5),
            )
            .unwrap(),
            1
        );
        InterruptedAssistant {
            message_id: self.message_id.clone(),
            seq: 1,
        }
    }
}

#[test]
fn full_access_accepts_all_danger_classes_without_a_second_approval() {
    for (command, rule_id) in [
        (FORCE_PUSH, Some("git-force-push")),
        ("rm -rf /", Some("rm-root-recursive-force")),
        ("dd if=input of=/dev/disk99", Some("raw-device-write")),
        ("mkfs.ext4 /dev/disk99", Some("filesystem-format")),
        (
            "diskutil eraseDisk APFS Test /dev/disk99",
            Some("diskutil-destructive"),
        ),
        ("pwd", None),
    ] {
        assert_eq!(
            vega_tools::danger::detect_danger(command)
                .unwrap()
                .map(|danger| danger.rule_id),
            rule_id
        );
        let audit = full_access_audit(command);
        let state = pending(command);
        validate_runtime_approval_event(&state, "call_permission", &audit, None).unwrap();
        let shared = approval_audit_from_runtime(&audit);
        assert_eq!(
            ApprovalAudit::from_json(&shared.to_json().unwrap()).unwrap(),
            shared
        );
        assert!(bash_danger_audit_matches(&state.input_json, &shared));
        assert_eq!(
            validate_recovered_projection(
                "",
                &state.thread_id,
                "call_permission",
                "bash",
                &state.input_json,
                "completed",
                RuntimeToolStatus::Success,
                &shared,
                Some(0),
                Some(1),
            )
            .unwrap(),
            state.input_json
        );
    }
}

#[test]
fn full_access_terminal_events_survive_storage_history_and_reuse() {
    for (runtime_status, status, output) in [
        (
            RuntimeToolStatus::Success,
            ToolCallStatus::Success,
            "completed".to_owned(),
        ),
        (
            RuntimeToolStatus::Failed,
            ToolCallStatus::Failed,
            vega_tools::BashErrorCode::SpawnFailed.tool_output(),
        ),
        (
            RuntimeToolStatus::Cancelled,
            ToolCallStatus::Cancelled,
            vega_runtime::CANCELLED_BEFORE_EXECUTION_OUTPUT.to_owned(),
        ),
    ] {
        let mut fixture = Fixture::new();
        let audit = full_access_audit(FORCE_PUSH);
        fixture.persist(RuntimeEvent::ToolCallProposed(RuntimeToolCall {
            id: "call_permission".into(),
            name: "bash".into(),
            input_json: input(FORCE_PUSH),
        }));
        fixture.persist(RuntimeEvent::ToolCallApproved {
            call_id: "call_permission".into(),
            audit: audit.clone(),
            remember_rule: None,
        });
        fixture.persist(RuntimeEvent::ToolCallRunning {
            call_id: "call_permission".into(),
        });
        let mut result = RuntimeToolResult {
            call_id: "call_permission".into(),
            output: output.clone(),
            status: runtime_status,
            reused: false,
            exit_code: (runtime_status == RuntimeToolStatus::Success).then_some(0),
            duration_ms: (runtime_status == RuntimeToolStatus::Success).then_some(1),
            truncated: (runtime_status == RuntimeToolStatus::Success).then_some(false),
            approval: Some(audit.clone()),
            remember_rule: None,
        };
        fixture.persist(RuntimeEvent::ToolCallFinished(result.clone()));
        let before =
            required_tool_state(&fixture.store, "call_permission", &fixture.thread_id).unwrap();
        assert_eq!(before.output_text.as_deref(), Some(output.as_str()));
        assert_eq!(ToolCallStatus::parse(&before.status), Some(status));
        assert_eq!(
            ApprovalAudit::from_json(before.approval.as_deref().unwrap()).unwrap(),
            approval_audit_from_runtime(&audit)
        );
        result.reused = true;
        result.truncated = None;
        fixture.persist(RuntimeEvent::ToolCallFinished(result.clone()));
        assert_eq!(
            required_tool_state(&fixture.store, "call_permission", &fixture.thread_id).unwrap(),
            before
        );
        messages::finish_streaming(
            fixture.store.conn(),
            &fixture.message_id,
            "",
            "done",
            Some(5),
        )
        .unwrap();
        let page = latest_history_page(&fixture.store, &fixture.thread_id, 20).unwrap();
        let tools: Vec<_> = page
            .entries
            .iter()
            .filter_map(|entry| match entry {
                HistoryEntry::Tool {
                    call_id,
                    approval,
                    result,
                    ..
                } => Some((call_id, approval, result)),
                _ => None,
            })
            .collect();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].0, "call_permission");
        assert_eq!(*tools[0].1, Some(Approval::Once));
        assert_eq!(
            *tools[0].2,
            Some(ToolCardResultProjection::Bash {
                status,
                output,
                exit_code: result.exit_code,
                duration_ms: result.duration_ms,
                truncated: None,
                reused: true,
            })
        );
    }
}

#[test]
fn malformed_full_access_audits_and_invalid_commands_remain_rejected() {
    let valid = full_access_audit(FORCE_PUSH);
    let state = pending(FORCE_PUSH);
    let mut invalid = Vec::new();
    for decision in [
        RuntimeApprovalDecision::Deny,
        RuntimeApprovalDecision::Always,
    ] {
        let mut audit = valid.clone();
        audit.decision = decision;
        invalid.push(audit);
    }
    let mut noted = valid.clone();
    noted.note = Some("not an approval".into());
    invalid.push(noted);
    let mut nested = valid.clone();
    nested.danger = Some(vega_runtime::RuntimeDangerAudit {
        rule_id: "git-force-push".into(),
        decision: RuntimeApprovalDecision::Once,
        note: None,
    });
    invalid.push(nested);
    for audit in invalid {
        let shared = approval_audit_from_runtime(&audit);
        assert!(shared.to_json().is_err());
        assert!(!bash_danger_audit_matches(&state.input_json, &shared));
        assert!(validate_runtime_approval_event(&state, "call_permission", &audit, None).is_err());
        assert!(
            validate_recovered_projection(
                "",
                &state.thread_id,
                "call_permission",
                "bash",
                &state.input_json,
                "completed",
                RuntimeToolStatus::Success,
                &shared,
                Some(0),
                Some(1),
            )
            .is_err()
        );
    }
    assert!(
        validate_runtime_approval_event(
            &state,
            "call_permission",
            &valid,
            Some(&target(FORCE_PUSH))
        )
        .is_err()
    );
    assert!(!bash_danger_audit_matches(
        r#"{"command":"","timeout":120}"#,
        &approval_audit_from_runtime(&valid)
    ));
}

#[test]
fn other_modes_still_require_danger_approval_even_with_an_exact_rule() {
    for mode in [
        RuntimePermissionMode::Confirm,
        RuntimePermissionMode::Auto,
        RuntimePermissionMode::ReadOnly,
    ] {
        for exact_rule in [false, true] {
            assert!(
                matches!(decision(FORCE_PUSH, mode, exact_rule, None), RuntimePermissionOutcome::Prompt(prompt) if prompt.danger.is_some())
            );
            for (response, expected_decision, expected_remember) in [
                (
                    RuntimeUserDecision::Once,
                    RuntimeApprovalDecision::Once,
                    false,
                ),
                (
                    RuntimeUserDecision::Always,
                    RuntimeApprovalDecision::Always,
                    true,
                ),
            ] {
                let outcome = decision(FORCE_PUSH, mode, exact_rule, Some(response));
                match outcome {
                    RuntimePermissionOutcome::Approved {
                        audit,
                        remember_rule,
                    } => {
                        assert_ne!(mode, RuntimePermissionMode::ReadOnly);
                        assert_eq!(audit.source, RuntimeApprovalSource::Danger);
                        assert_eq!(audit.decision, expected_decision);
                        assert_eq!(remember_rule, expected_remember);
                        let remembered = remember_rule.then(|| target(FORCE_PUSH));
                        validate_runtime_approval_event(
                            &pending(FORCE_PUSH),
                            "call_permission",
                            &audit,
                            remembered.as_ref(),
                        )
                        .unwrap();
                    }
                    RuntimePermissionOutcome::Rejected {
                        audit,
                        remember_rule,
                    } => {
                        assert_eq!(mode, RuntimePermissionMode::ReadOnly);
                        assert_eq!(audit.source, RuntimeApprovalSource::ReadOnly);
                        assert_eq!(audit.decision, RuntimeApprovalDecision::Deny);
                        assert_eq!(audit.danger.as_ref().unwrap().decision, expected_decision);
                        assert_eq!(remember_rule, expected_remember);
                        assert!(bash_danger_audit_matches(
                            &input(FORCE_PUSH),
                            &approval_audit_from_runtime(&audit)
                        ));
                    }
                    RuntimePermissionOutcome::Prompt(_) => {
                        panic!("a danger response must finish the decision")
                    }
                }
            }
            for (response, source) in [
                (
                    RuntimeUserDecision::Deny { note: None },
                    RuntimeApprovalSource::Danger,
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
                let RuntimePermissionOutcome::Rejected {
                    audit,
                    remember_rule,
                } = decision(FORCE_PUSH, mode, exact_rule, Some(response))
                else {
                    panic!("danger denial must not approve");
                };
                assert!(!remember_rule);
                assert_eq!(audit.decision, RuntimeApprovalDecision::Deny);
                assert_eq!(audit.source, source);
                assert!(bash_danger_audit_matches(
                    &input(FORCE_PUSH),
                    &approval_audit_from_runtime(&audit)
                ));
            }
        }
    }
    for (source, decision) in [
        (ApprovalSource::Auto, Approval::Once),
        (ApprovalSource::Rule, Approval::Always),
        (ApprovalSource::User, Approval::Once),
    ] {
        assert!(!bash_danger_audit_matches(
            &input(FORCE_PUSH),
            &ApprovalAudit {
                decision,
                source,
                note: None,
                danger: None
            }
        ));
    }
}

#[test]
fn historical_danger_approval_and_recovery_denial_remain_projectable() {
    let RuntimePermissionOutcome::Approved { audit, .. } = decision(
        FORCE_PUSH,
        RuntimePermissionMode::Confirm,
        false,
        Some(RuntimeUserDecision::Once),
    ) else {
        panic!("confirmed danger must be approved");
    };
    let shared = approval_audit_from_runtime(&audit);
    assert_eq!(shared.source, ApprovalSource::Danger);
    assert_eq!(
        validate_recovered_projection(
            "",
            "thread",
            "call_permission",
            "bash",
            &input(FORCE_PUSH),
            "completed",
            RuntimeToolStatus::Success,
            &shared,
            Some(0),
            Some(1),
        )
        .unwrap(),
        input(FORCE_PUSH)
    );
    let recovery =
        ApprovalAudit::from_json(vega_store::recovery::RECOVERY_DENIAL_APPROVAL_JSON).unwrap();
    assert_eq!(
        validate_recovered_projection(
            "",
            "thread",
            "call_permission",
            "bash",
            &input(FORCE_PUSH),
            vega_store::recovery::RECOVERY_REJECTED_OUTPUT,
            RuntimeToolStatus::Rejected,
            &recovery,
            None,
            None,
        )
        .unwrap(),
        input(FORCE_PUSH)
    );
}

#[test]
fn ask_and_plan_do_not_gain_mutating_capability() {
    for mode in [RuntimeRunMode::Ask, RuntimeRunMode::Plan] {
        let RuntimeCapabilityOutcome::Rejected(audit) =
            decide_capability(mode, RuntimeToolClass::Mutating(target(FORCE_PUSH)))
        else {
            panic!("non-execute mode must deny mutating capability");
        };
        assert_eq!(audit.source, RuntimeApprovalSource::RunMode);
        assert_eq!(audit.decision, RuntimeApprovalDecision::Deny);
        assert!(bash_danger_audit_matches(
            &input(FORCE_PUSH),
            &approval_audit_from_runtime(&audit)
        ));
    }
}

#[test]
fn full_access_resume_requires_matching_current_run_authority() {
    let fixture = Fixture::new();
    let fence = fixture.fence();
    assert!(fence.full_access_grant.is_none());
    let interrupted = fixture.interrupt(Some(&fence));
    let matches = |current: &PermissionFence| {
        crate::permission_contracts::interrupted_authority_matches(
            fixture.store.conn(),
            &interrupted,
            current,
        )
        .unwrap()
    };
    assert!(matches(&fence));
    for changed in [
        PermissionFence {
            thread_id: ulid::Ulid::generate().to_string(),
            ..fence.clone()
        },
        PermissionFence {
            database_identity: "other-database".into(),
            ..fence.clone()
        },
        PermissionFence {
            execution_policy_version: 1,
            ..fence.clone()
        },
        PermissionFence {
            permission_mode: "invalid".into(),
            ..fence.clone()
        },
        PermissionFence {
            permission_mode: "confirm".into(),
            ..fence.clone()
        },
        PermissionFence {
            run_mode: "ask".into(),
            ..fence.clone()
        },
    ] {
        assert!(!matches(&changed));
    }
    assert_eq!(
        vega_store::threads::set_permission_mode(
            fixture.store.conn(),
            &fixture.thread_id,
            "full_access",
            2
        )
        .unwrap(),
        1
    );
    assert!(!matches(&fixture.fence()));
}

#[test]
fn missing_or_reset_run_authority_cannot_resume_full_access() {
    let missing = Fixture::new();
    let fence = missing.fence();
    let interrupted = missing.interrupt(None);
    assert!(
        !crate::permission_contracts::interrupted_authority_matches(
            missing.store.conn(),
            &interrupted,
            &fence
        )
        .unwrap()
    );

    let fixture = Fixture::new();
    let fence = fixture.fence();
    let interrupted = fixture.interrupt(Some(&fence));
    fixture
        .store
        .conn()
        .execute(
            "UPDATE permission_contract_state SET authority_epoch = ?1 WHERE singleton = 1",
            [ulid::Ulid::generate().to_string()],
        )
        .unwrap();
    assert!(
        !crate::permission_contracts::interrupted_authority_matches(
            fixture.store.conn(),
            &interrupted,
            &fixture.fence()
        )
        .unwrap()
    );
    fixture
        .store
        .conn()
        .execute(
            "UPDATE permission_contract_state SET reset_state = 'pending' WHERE singleton = 1",
            [],
        )
        .unwrap();
    let thread = vega_store::threads::find(fixture.store.conn(), &fixture.thread_id)
        .unwrap()
        .unwrap();
    assert!(crate::permission_contracts::capture_fence(&fixture.store, &thread).is_err());
}
