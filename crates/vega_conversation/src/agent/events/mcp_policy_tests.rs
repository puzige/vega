use super::*;

fn identity() -> crate::types::McpCallIdentity {
    crate::types::McpCallIdentity {
        server_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
        config_revision: 2,
        exact_tool_name: "qa_receipt".into(),
        arguments_bytes: 2,
        arguments_sha256: "a".repeat(64),
        argument_preview: String::new(),
    }
}

fn input(identity: &crate::types::McpCallIdentity) -> String {
    serde_json::json!({
        "server_id":identity.server_id,
        "config_revision":identity.config_revision,
        "tool":identity.exact_tool_name,
        "arguments_bytes":identity.arguments_bytes,
        "arguments_sha256":identity.arguments_sha256,
        "argument_preview":identity.argument_preview,
    })
    .to_string()
}

fn state() -> tool_calls::ToolCallState {
    let identity = identity();
    tool_calls::ToolCallState {
        thread_id: "01ARZ3NDEKTSV4RRFFQ69G5FAW".into(),
        tool: identity.alias(),
        input_json: input(&identity),
        status: "pending_approval".into(),
        approval: None,
        output_text: None,
        exit_code: None,
        duration_ms: None,
        output_full_path: None,
    }
}

fn audit(source: vega_runtime::RuntimeApprovalSource) -> vega_runtime::RuntimeApprovalAudit {
    vega_runtime::RuntimeApprovalAudit {
        decision: vega_runtime::RuntimeApprovalDecision::Once,
        source,
        note: None,
        danger: None,
    }
}

#[test]
fn approval_accepts_only_one_call_user_or_full_access_with_matching_identity() {
    let state = state();
    for source in [
        vega_runtime::RuntimeApprovalSource::User,
        vega_runtime::RuntimeApprovalSource::FullAccess,
    ] {
        assert!(validate_runtime_approval_event(&state, "call", &audit(source), None).is_ok());
    }
    for source in [
        vega_runtime::RuntimeApprovalSource::Auto,
        vega_runtime::RuntimeApprovalSource::Rule,
        vega_runtime::RuntimeApprovalSource::ReadonlyTool,
    ] {
        assert!(validate_runtime_approval_event(&state, "call", &audit(source), None).is_err());
    }
    for decision in [
        vega_runtime::RuntimeApprovalDecision::Always,
        vega_runtime::RuntimeApprovalDecision::Deny,
    ] {
        let mut audit = audit(vega_runtime::RuntimeApprovalSource::FullAccess);
        audit.decision = decision;
        assert!(validate_runtime_approval_event(&state, "call", &audit, None).is_err());
    }
    let mut forged = state.clone();
    forged.tool.push('x');
    assert!(
        validate_runtime_approval_event(
            &forged,
            "call",
            &audit(vega_runtime::RuntimeApprovalSource::FullAccess),
            None
        )
        .is_err()
    );
    forged = state.clone();
    forged.input_json = "{\"source\":\"full_access\"}".into();
    assert!(
        validate_runtime_approval_event(
            &forged,
            "call",
            &audit(vega_runtime::RuntimeApprovalSource::FullAccess),
            None
        )
        .is_err()
    );
    let mut noted = audit(vega_runtime::RuntimeApprovalSource::FullAccess);
    noted.note = Some("user clicked".into());
    assert!(validate_runtime_approval_event(&state, "call", &noted, None).is_err());
    let target = vega_runtime::RuntimePermissionTarget {
        call_id: "call".into(),
        tool: vega_runtime::RuntimeMutatingTool::Read,
        exact_pattern: "anything".into(),
        display_target: "anything".into(),
    };
    assert!(
        validate_runtime_approval_event(
            &state,
            "call",
            &audit(vega_runtime::RuntimeApprovalSource::FullAccess),
            Some(&target)
        )
        .is_err()
    );
}

#[test]
fn both_audit_sources_restore_truthful_success_failure_and_cancellation() {
    let state = state();
    for source in [ApprovalSource::User, ApprovalSource::FullAccess] {
        let audit = ApprovalAudit {
            decision: Approval::Once,
            source,
            note: None,
            danger: None,
        };
        let encoded = audit.to_json().unwrap();
        let decoded = ApprovalAudit::from_json(&encoded).unwrap();
        assert_eq!(decoded, audit);
        for (status, output) in [
            (
                RuntimeToolStatus::Success,
                "[Untrusted external MCP result]\nreceipt",
            ),
            (
                RuntimeToolStatus::Failed,
                "[Untrusted external MCP result]\nQA_EXPECTED_TOOL_ERROR",
            ),
            (
                RuntimeToolStatus::Failed,
                "Tool error: MCP invalid protocol response",
            ),
            (
                RuntimeToolStatus::Failed,
                "Tool error: MCP response limit exceeded",
            ),
            (
                RuntimeToolStatus::Failed,
                "Tool error: MCP unsupported result content",
            ),
            (
                RuntimeToolStatus::Failed,
                "Tool error: MCP authorization required or invalid",
            ),
            (
                RuntimeToolStatus::Cancelled,
                "Tool error: MCP call outcome unknown after cancellation",
            ),
            (
                RuntimeToolStatus::Cancelled,
                vega_store::recovery::RECOVERY_CANCELLED_OUTPUT,
            ),
        ] {
            assert!(approval_source_matches(
                &state.tool,
                &state.input_json,
                status,
                source,
                false
            ));
            assert!(
                validate_recovered_projection(
                    "",
                    &state.thread_id,
                    "call",
                    &state.tool,
                    &state.input_json,
                    output,
                    status,
                    &decoded,
                    None,
                    None
                )
                .is_ok()
            );
        }
        assert!(
            validate_recovered_projection(
                "",
                &state.thread_id,
                "call",
                &state.tool,
                &state.input_json,
                "fabricated success",
                RuntimeToolStatus::Success,
                &decoded,
                None,
                None
            )
            .is_err()
        );
    }
    for decision in [Approval::Always, Approval::Deny] {
        let audit = ApprovalAudit {
            decision,
            source: ApprovalSource::FullAccess,
            note: None,
            danger: None,
        };
        assert!(
            validate_recovered_projection(
                "",
                &state.thread_id,
                "call",
                &state.tool,
                &state.input_json,
                "[Untrusted external MCP result]\nreceipt",
                RuntimeToolStatus::Success,
                &audit,
                None,
                None
            )
            .is_err()
        );
    }
}

struct Database {
    root: std::path::PathBuf,
    path: std::path::PathBuf,
    store: Store,
    thread: String,
    message: String,
}

impl Database {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("vega-mcp-policy-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("test.sqlite");
        let store = Store::open(&path).unwrap();
        store.migrate().unwrap();
        let project =
            vega_store::projects::create(store.conn(), root.to_str().unwrap(), "policy", None)
                .unwrap();
        let thread = ulid::Ulid::generate().to_string();
        vega_store::threads::create(
            store.conn(),
            vega_store::threads::NewThread {
                reasoning_preferences: None,
                id: &thread,
                project_id: &project.id,
                title: "policy",
                mode: "execute",
                permission_mode: "full_access",
                permission_revision: 0,
                full_access_grant: None,
                model: "test",
                status: "active",
                pinned: false,
                unread: false,
                created_at: 1,
                updated_at: 1,
            },
        )
        .unwrap();
        let message = ulid::Ulid::generate().to_string();
        vega_store::messages::insert(
            store.conn(),
            &vega_store::messages::MessageRow {
                id: message.clone(),
                thread_id: thread.clone(),
                seq: 0,
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
        let fence = crate::permission_contracts::capture_fence(
            &store,
            &vega_store::threads::find(store.conn(), &thread)
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        store.conn().execute("INSERT INTO run_permission_fences(message_id,thread_id,permission_fence) VALUES (?1,?2,?3)", (&message, &thread, serde_json::to_string(&fence).unwrap())).unwrap();
        Self {
            root,
            path,
            store,
            thread,
            message,
        }
    }
}

impl Drop for Database {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn full_access_approval_requires_current_database_thread_mode_and_epoch() {
    let db = Database::new();
    assert!(validate_mcp_full_access_authority(&db.store, &db.thread, &db.message).is_ok());
    assert!(validate_mcp_full_access_authority(&db.store, "wrong-thread", &db.message).is_err());
    assert!(validate_mcp_full_access_authority(&db.store, &db.thread, "wrong-message").is_err());
    db.store
        .conn()
        .execute(
            "UPDATE threads SET permission_mode='confirm' WHERE id=?1",
            [&db.thread],
        )
        .unwrap();
    assert!(validate_mcp_full_access_authority(&db.store, &db.thread, &db.message).is_err());
    db.store
        .conn()
        .execute(
            "UPDATE threads SET permission_mode='full_access' WHERE id=?1",
            [&db.thread],
        )
        .unwrap();
    db.store
        .conn()
        .execute(
            "UPDATE permission_contract_state SET reset_state='pending' WHERE singleton=1",
            [],
        )
        .unwrap();
    assert!(validate_mcp_full_access_authority(&db.store, &db.thread, &db.message).is_err());
    db.store.conn().execute("UPDATE permission_contract_state SET reset_state='ready',authority_epoch=?1 WHERE singleton=1", [ulid::Ulid::generate().to_string()]).unwrap();
    assert!(validate_mcp_full_access_authority(&db.store, &db.thread, &db.message).is_err());
}

#[test]
fn accepted_steering_retains_full_access_authority_but_never_changed_authority() {
    let db = Database::new();
    let input_id = ulid::Ulid::generate().to_string();
    let next_message = ulid::Ulid::generate().to_string();
    vega_store::pending_messages::insert(
        db.store.conn(),
        &vega_store::pending_messages::PendingMessageRow {
            id: input_id.clone(),
            thread_id: db.thread.clone(),
            ordinal: 0,
            behavior: "steer".into(),
            generation: "generation".into(),
            payload: "{}".into(),
            created_at: 2,
        },
    )
    .unwrap();
    let steering = vega_runtime::RuntimeSteeringInput {
        original_content: "continue".into(),
        user_message_seq: 1,
        id: input_id.clone(),
        assistant_message_id: next_message.clone(),
        message: vega_runtime::ChatMessage::new(vega_runtime::ChatRole::User, "continue"),
    };
    let mut seq = 0;
    persist_runtime_event(
        &db.store,
        "",
        &db.thread,
        &db.message,
        "test",
        false,
        "",
        &mut seq,
        &RuntimeEvent::SteeringAccepted(steering),
        None,
        None,
    )
    .unwrap();
    assert!(validate_mcp_full_access_authority(&db.store, &db.thread, &next_message).is_ok());
    let call = vega_runtime::RuntimeToolCall {
        id: "after-steering".into(),
        name: identity().alias(),
        input_json: input(&identity()),
        description: None,
    };
    persist_runtime_event(
        &db.store,
        "",
        &db.thread,
        &next_message,
        "test",
        false,
        "",
        &mut seq,
        &RuntimeEvent::ToolCallProposed(call.clone()),
        None,
        None,
    )
    .unwrap();
    persist_runtime_event(
        &db.store,
        "",
        &db.thread,
        &next_message,
        "test",
        false,
        "",
        &mut seq,
        &RuntimeEvent::ToolCallApproved {
            call_id: call.id,
            audit: audit(vega_runtime::RuntimeApprovalSource::FullAccess),
            remember_rule: None,
        },
        None,
        None,
    )
    .unwrap();
    db.store
        .conn()
        .execute(
            "UPDATE pending_messages SET permission_invalidated=1 WHERE id=?1",
            [&input_id],
        )
        .unwrap();
    assert!(validate_mcp_full_access_authority(&db.store, &db.thread, &next_message).is_err());
    db.store
        .conn()
        .execute(
            "UPDATE pending_messages SET permission_invalidated=0 WHERE id=?1",
            [&input_id],
        )
        .unwrap();
    db.store
        .conn()
        .execute(
            "UPDATE threads SET permission_mode='confirm' WHERE id=?1",
            [&db.thread],
        )
        .unwrap();
    assert!(validate_mcp_full_access_authority(&db.store, &db.thread, &next_message).is_err());
}

#[test]
fn approval_persists_before_running_and_failure_leaves_call_unapproved() {
    let db = Database::new();
    let identity = identity();
    let call = vega_runtime::RuntimeToolCall {
        id: "durable".into(),
        name: identity.alias(),
        input_json: input(&identity),
        description: None,
    };
    let mut seq = 0;
    let apply = |event: &RuntimeEvent, seq: &mut i64| {
        persist_runtime_event(
            &db.store,
            "",
            &db.thread,
            &db.message,
            "test",
            false,
            "",
            seq,
            event,
            None,
            None,
        )
    };
    apply(&RuntimeEvent::ToolCallProposed(call.clone()), &mut seq).unwrap();
    db.store.conn().execute_batch("CREATE TRIGGER reject_approval BEFORE UPDATE ON tool_calls WHEN NEW.status='approved' BEGIN SELECT RAISE(ABORT,'audit unavailable'); END;").unwrap();
    let event = RuntimeEvent::ToolCallApproved {
        call_id: call.id.clone(),
        audit: audit(vega_runtime::RuntimeApprovalSource::FullAccess),
        remember_rule: None,
    };
    assert!(apply(&event, &mut seq).is_err());
    let status: String = db
        .store
        .conn()
        .query_row(
            "SELECT status FROM tool_calls WHERE id='durable'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(status, "pending_approval");
    assert!(
        apply(
            &RuntimeEvent::ToolCallRunning {
                call_id: call.id.clone(),
                bash_started_at: None
            },
            &mut seq
        )
        .is_err()
    );
    db.store
        .conn()
        .execute_batch("DROP TRIGGER reject_approval;")
        .unwrap();
    apply(&event, &mut seq).unwrap();
    apply(
        &RuntimeEvent::ToolCallRunning {
            call_id: call.id.clone(),
            bash_started_at: None,
        },
        &mut seq,
    )
    .unwrap();
    let reopened = Store::open(&db.path).unwrap();
    let encoded: String = reopened
        .conn()
        .query_row(
            "SELECT approval FROM tool_calls WHERE id='durable'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let saved = ApprovalAudit::from_json(&encoded).unwrap();
    assert_eq!(saved.source, ApprovalSource::FullAccess);
    assert_eq!(saved.decision, Approval::Once);
    let legacy = vega_runtime::RuntimeToolCall {
        id: "legacy-user".into(),
        ..call.clone()
    };
    apply(&RuntimeEvent::ToolCallProposed(legacy.clone()), &mut seq).unwrap();
    apply(
        &RuntimeEvent::ToolCallApproved {
            call_id: legacy.id.clone(),
            audit: audit(vega_runtime::RuntimeApprovalSource::User),
            remember_rule: None,
        },
        &mut seq,
    )
    .unwrap();
    apply(
        &RuntimeEvent::ToolCallRunning {
            call_id: legacy.id,
            bash_started_at: None,
        },
        &mut seq,
    )
    .unwrap();
    let recovered = vega_store::recovery::recover_thread(reopened.conn(), &db.thread, 3).unwrap();
    assert_eq!(recovered.tools_cancelled, 2);
    for id in ["durable", "legacy-user"] {
        let (status, approval, output): (String, String, String) = reopened
            .conn()
            .query_row(
                "SELECT status,approval,output_text FROM tool_calls WHERE id=?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(status, "cancelled");
        let approval = ApprovalAudit::from_json(&approval).unwrap();
        assert_eq!(
            approval.source,
            if id == "durable" {
                ApprovalSource::FullAccess
            } else {
                ApprovalSource::User
            }
        );
        assert!(
            validate_recovered_projection(
                "",
                &db.thread,
                id,
                &call.name,
                &call.input_json,
                &output,
                RuntimeToolStatus::Cancelled,
                &approval,
                None,
                None
            )
            .is_ok()
        );
    }
    assert_eq!(
        vega_store::recovery::recover_thread(reopened.conn(), &db.thread, 4)
            .unwrap()
            .tools_cancelled,
        0
    );
    assert_eq!(
        reopened
            .conn()
            .query_row("SELECT count(*) FROM permissions", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}
