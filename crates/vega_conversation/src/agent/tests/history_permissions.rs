use super::*;
use vega_store::permissions;

#[tokio::test]
async fn assembles_system_and_history_by_sequence_with_current_user_last() {
    let (store, dir, _project_id) = setup();
    for (id, seq, role, content, status) in [
        ("history-3", 3, "assistant", "failed answer", "failed"),
        ("history-1", 1, "user", "old question", "done"),
        ("history-2", 2, "assistant", "partial answer", "interrupted"),
    ] {
        messages::insert(
            store.conn(),
            &messages::MessageRow {
                id: id.into(),
                thread_id: "thread-1".into(),
                seq,
                role: role.into(),
                kind: "text".into(),
                content: content.into(),
                status: status.into(),
                created_at: seq,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .unwrap();
    }
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("answer".into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]);
    run_thread_task(
        &store,
        &provider,
        &tools,
        "thread-1",
        "current question",
        "system first",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let requests = provider.requests();
    let history: Vec<(vega_runtime::ChatRole, &str)> = requests[0]
        .messages
        .iter()
        .map(|message| (message.role, message.content.as_str()))
        .collect();
    assert_eq!(
        history,
        vec![
            (vega_runtime::ChatRole::System, "system first"),
            (vega_runtime::ChatRole::User, "old question"),
            (vega_runtime::ChatRole::Assistant, "partial answer"),
            (vega_runtime::ChatRole::Assistant, "failed answer"),
            (vega_runtime::ChatRole::User, "current question"),
        ]
    );
}

#[tokio::test]
async fn always_permission_and_rule_are_durable_before_second_write() {
    let (store, project_dir, _data_dir, project_id) = setup_external("confirm");
    let tools = vega_tools::Tools::new(project_dir.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "write-first".into(),
                name: "write".into(),
                input_json: r#"{"path":"same.txt","content":"first-secret"}"#.into(),
            },
            ProviderEvent::ToolUse {
                id: "write-second".into(),
                name: "write".into(),
                input_json: r#"{"path":"same.txt","content":"second-secret"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ]);
    let calls = Arc::new(AtomicUsize::new(0));
    let hook = FixedPermissionHook {
        calls: calls.clone(),
        decision: PermissionDecision::Always,
    };
    let run = run_thread_task_with_permission_sink(
        &store,
        &provider,
        &tools,
        "thread-1",
        "write twice",
        "system",
        CancellationToken::new(),
        &hook,
        |_| Ok(()),
    )
    .await
    .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        fs::read_to_string(project_dir.path().join("same.txt")).unwrap(),
        "second-secret"
    );
    let rules = permissions::list_exact(store.conn(), &project_id).unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].tool, "write");
    assert_eq!(rules[0].pattern, "same.txt");
    let approvals = ["write-first", "write-second"].map(|id| {
        let json: String = store
            .conn()
            .query_row(
                "SELECT approval FROM tool_calls WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .unwrap();
        ApprovalAudit::from_json(&json).unwrap()
    });
    assert_eq!(approvals[0].source, ApprovalSource::User);
    assert_eq!(approvals[0].decision, Approval::Always);
    assert_eq!(approvals[1].source, ApprovalSource::Rule);
    assert_eq!(approvals[1].decision, Approval::Always);
    assert!(run.events.iter().all(|event| {
        !format!("{event:?}").contains("first-secret")
            && !format!("{event:?}").contains("second-secret")
    }));
}

#[tokio::test]
async fn danger_readonly_always_rejects_and_persists_rule_atomically() {
    let (store, project_dir, data_dir, project_id) = setup_external("readonly");
    let tools = vega_tools::Tools::new(project_dir.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "danger-1".into(),
                name: "bash".into(),
                input_json: r#"{"cmd":"rm -rf /"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ]);
    let calls = Arc::new(AtomicUsize::new(0));
    let hook = FixedPermissionHook {
        calls: calls.clone(),
        decision: PermissionDecision::Always,
    };
    let run = run_thread_task_with_permission_sink(
        &store,
        &provider,
        &tools,
        "thread-1",
        "danger",
        "system",
        CancellationToken::new(),
        &hook,
        |_| Ok(()),
    )
    .await
    .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let (status, approval_json): (String, String) = store
        .conn()
        .query_row(
            "SELECT status, approval FROM tool_calls WHERE id = 'danger-1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(status, "rejected");
    let approval = ApprovalAudit::from_json(&approval_json).unwrap();
    assert_eq!(approval.source, ApprovalSource::ReadOnly);
    assert_eq!(approval.decision, Approval::Deny);
    assert_eq!(
        approval.danger.as_ref().map(|danger| danger.decision),
        Some(Approval::Always)
    );
    let rules = permissions::list_exact(store.conn(), &project_id).unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].tool, "bash");
    assert_eq!(rules[0].pattern, "rm -rf /");
    assert!(run.events.iter().any(|event| matches!(
        event,
        ConversationEvent::ToolCallFinished { result, .. }
            if result.status == ToolCallStatus::Rejected
    )));
    assert_eq!(
        fs::read_dir(data_dir.path().join("checkpoints"))
            .unwrap()
            .count(),
        0
    );
}

#[tokio::test]
async fn write_edit_and_bash_execute_serially_with_strict_db_results() {
    let (store, project_dir, data_dir, _project_id) = setup_external("auto");
    fs::write(project_dir.path().join("serial.txt"), "initial").unwrap();
    let tools = vega_tools::Tools::new(project_dir.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "write-1".into(),
                name: "write".into(),
                input_json: r#"{"path":"serial.txt","content":"hello"}"#.into(),
            },
            ProviderEvent::ToolUse {
                id: "edit-1".into(),
                name: "edit".into(),
                input_json: r#"{"path":"serial.txt","old_string":"hello","new_string":"world"}"#
                    .into(),
            },
            ProviderEvent::ToolUse {
                id: "bash-1".into(),
                name: "bash".into(),
                input_json: r#"{"cmd":"cat serial.txt"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ]);
    let run = run_thread_task_with_permission_sink(
        &store,
        &provider,
        &tools,
        "thread-1",
        "serial tools",
        "system",
        CancellationToken::new(),
        &FixedPermissionHook {
            calls: Arc::new(AtomicUsize::new(0)),
            decision: PermissionDecision::Deny { note: None },
        },
        |_| Ok(()),
    )
    .await
    .unwrap();
    assert_eq!(
        fs::read_to_string(project_dir.path().join("serial.txt")).unwrap(),
        "world"
    );
    let finished = run
        .events
        .iter()
        .filter_map(|event| match event {
            ConversationEvent::ToolCallFinished { call_id, result } => {
                Some((call_id.as_str(), result.status))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        finished,
        vec![
            ("write-1", ToolCallStatus::Success),
            ("edit-1", ToolCallStatus::Success),
            ("bash-1", ToolCallStatus::Success),
        ]
    );
    let rows = ["write-1", "edit-1", "bash-1"].map(|id| {
            store
                .conn()
                .query_row(
                    "SELECT status, output_text, exit_code, duration_ms, output_full_path FROM tool_calls WHERE id = ?1",
                    [id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Option<i32>>(2)?,
                            row.get::<_, Option<i64>>(3)?,
                            row.get::<_, Option<String>>(4)?,
                        ))
                    },
                )
                .unwrap()
        });
    assert!(rows.iter().all(|row| row.0 == "success" && row.4.is_none()));
    assert!(vega_tools::WriteSuccessOutput::from_json(&rows[0].1).is_ok());
    assert!(vega_tools::EditSuccessOutput::from_json(&rows[1].1).is_ok());
    assert!(rows[2].1.contains("world"));
    assert_eq!(rows[2].2, Some(0));
    assert!(rows[2].3.is_some());
    assert!(data_dir.path().join("checkpoints").exists());
}

#[test]
fn strict_projection_validation_is_semantic_and_binds_results_and_danger() {
    let project = tempdir().unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let audit = tools
        .audit_write_json(r#"{"path":"bound.txt","content":"body"}"#)
        .unwrap();
    let canonical = audit.to_json().unwrap();
    let value: serde_json::Value = serde_json::from_str(&canonical).unwrap();
    let reordered = format!(
        r#"{{"fingerprint_v1":{},"content_bytes":{},"path":{},"tool":{},"audit_version":{}}}"#,
        value["fingerprint_v1"],
        value["content_bytes"],
        value["path"],
        value["tool"],
        value["audit_version"]
    );
    assert!(tool_inputs_semantically_equal(
        "write", &canonical, &reordered
    ));

    let invalid = vega_tools::InvalidMutation::from_raw(
        vega_tools::MutationTool::Write,
        r#"{"path":"x","content":"secret","extra":true}"#,
        vega_tools::MutationErrorCode::UnexpectedField,
    )
    .unwrap();
    let invalid_json = invalid.audit().to_json().unwrap();
    let invalid_value: serde_json::Value = serde_json::from_str(&invalid_json).unwrap();
    let invalid_reordered = format!(
        r#"{{"validation_error_code":{},"raw_input_sha256":{},"raw_input_bytes":{},"tool":{},"audit_version":{}}}"#,
        invalid_value["validation_error_code"],
        invalid_value["raw_input_sha256"],
        invalid_value["raw_input_bytes"],
        invalid_value["tool"],
        invalid_value["audit_version"]
    );
    assert!(tool_inputs_semantically_equal(
        "write",
        &invalid_json,
        &invalid_reordered
    ));

    let ids = vega_tools::CheckpointIds::new("project", "thread", "call").unwrap();
    let success = vega_tools::WriteSuccessOutput {
        path: "bound.txt".to_string(),
        bytes_written: 4,
        checkpoint_ref: ids.checkpoint_ref(),
    };
    let success_json = success.to_json().unwrap();
    let auto = ApprovalAudit {
        decision: Approval::Once,
        note: None,
        source: ApprovalSource::Auto,
        danger: None,
    };
    assert!(
        validate_recovered_projection(
            "project",
            "thread",
            "call",
            "write",
            &reordered,
            &success_json,
            RuntimeToolStatus::Success,
            &auto,
            None,
            None,
        )
        .is_ok()
    );
    for corrupt_output in [
        vega_tools::WriteSuccessOutput {
            path: "other.txt".to_string(),
            ..success.clone()
        }
        .to_json()
        .unwrap(),
        vega_tools::WriteSuccessOutput {
            bytes_written: 5,
            ..success.clone()
        }
        .to_json()
        .unwrap(),
        vega_tools::WriteSuccessOutput {
            checkpoint_ref: vega_tools::CheckpointIds::new("project", "thread", "other")
                .unwrap()
                .checkpoint_ref(),
            ..success.clone()
        }
        .to_json()
        .unwrap(),
        "SECRET_RECOVERY_BODY".to_string(),
    ] {
        assert!(
            validate_recovered_projection(
                "project",
                "thread",
                "call",
                "write",
                &canonical,
                &corrupt_output,
                RuntimeToolStatus::Success,
                &auto,
                None,
                None,
            )
            .is_err()
        );
    }

    let validation = ApprovalAudit {
        decision: Approval::Deny,
        note: None,
        source: ApprovalSource::Validation,
        danger: None,
    };
    assert!(
        validate_recovered_projection(
            "project",
            "thread",
            "invalid",
            "write",
            &invalid_reordered,
            invalid.tool_result(),
            RuntimeToolStatus::Rejected,
            &validation,
            None,
            None,
        )
        .is_ok()
    );

    let dangerous = r#"{"cmd":"rm -rf /"}"#;
    let safe = r#"{"cmd":"printf safe"}"#;
    let wrong_danger = crate::types::DangerAudit {
        rule_id: "wrong".to_string(),
        decision: Approval::Once,
        note: None,
    };
    for (input, approval) in [
        (dangerous, auto.clone()),
        (
            dangerous,
            ApprovalAudit {
                decision: Approval::Once,
                note: None,
                source: ApprovalSource::Danger,
                danger: Some(wrong_danger.clone()),
            },
        ),
        (
            dangerous,
            ApprovalAudit {
                decision: Approval::Once,
                note: None,
                source: ApprovalSource::Legacy,
                danger: None,
            },
        ),
        (
            safe,
            ApprovalAudit {
                decision: Approval::Once,
                note: None,
                source: ApprovalSource::Danger,
                danger: Some(wrong_danger),
            },
        ),
    ] {
        assert!(
            validate_recovered_projection(
                "project",
                "thread",
                "bash-call",
                "bash",
                input,
                "output",
                RuntimeToolStatus::Success,
                &approval,
                Some(0),
                Some(1),
            )
            .is_err()
        );
    }

    let recovery = ApprovalAudit {
        decision: Approval::Deny,
        note: None,
        source: ApprovalSource::Recovery,
        danger: None,
    };
    assert!(
        validate_recovered_projection(
            "project",
            "thread",
            "unknown",
            "future_tool",
            "{}",
            vega_store::recovery::RECOVERY_REJECTED_OUTPUT,
            RuntimeToolStatus::Rejected,
            &recovery,
            None,
            None,
        )
        .is_ok()
    );
    assert!(
        validate_recovered_projection(
            "project",
            "thread",
            "unknown",
            "future_tool",
            "{\"secret\":true}",
            vega_store::recovery::RECOVERY_REJECTED_OUTPUT,
            RuntimeToolStatus::Rejected,
            &recovery,
            None,
            None,
        )
        .is_err()
    );

    let legacy_deny = ApprovalAudit {
        decision: Approval::Deny,
        note: None,
        source: ApprovalSource::Legacy,
        danger: None,
    };
    assert!(
        validate_recovered_projection(
            "project",
            "thread",
            "legacy",
            "write",
            &canonical,
            &legacy_unavailable_output("write"),
            RuntimeToolStatus::Rejected,
            &legacy_deny,
            None,
            None,
        )
        .is_ok()
    );
}

#[tokio::test]
async fn file_backed_recovery_reuses_write_edit_and_unknown_without_execution() {
    let (store, project_dir, data_dir, _project_id) = setup_external("auto");
    fs::write(project_dir.path().join("edit.txt"), "old").unwrap();
    messages::insert(
        store.conn(),
        &messages::MessageRow {
            id: "old-assistant".into(),
            thread_id: "thread-1".into(),
            seq: 1,
            role: "assistant".into(),
            kind: "text".into(),
            content: String::new(),
            status: "interrupted".into(),
            created_at: 1,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .unwrap();
    let tools = vega_tools::Tools::new(project_dir.path()).unwrap();
    let write_raw = r#"{"path":"new.txt","content":"recovery-secret"}"#;
    let edit_raw = r#"{"path":"edit.txt","old_string":"old","new_string":"recovery-new"}"#;
    let write_audit = tools
        .audit_write_json(write_raw)
        .unwrap()
        .to_json()
        .unwrap();
    let edit_audit = tools.audit_edit_json(edit_raw).unwrap().to_json().unwrap();
    for (seq, (id, tool, input, status)) in [
        (
            "recover-write",
            "write",
            write_audit.as_str(),
            "pending_approval",
        ),
        ("recover-edit", "edit", edit_audit.as_str(), "running"),
        ("recover-unknown", "future_tool", "{}", "pending_approval"),
    ]
    .into_iter()
    .enumerate()
    {
        tool_calls::insert(
            store.conn(),
            tool_calls::NewToolCall {
                id,
                thread_id: "thread-1",
                message_id: "old-assistant",
                seq: i64::try_from(seq + 1).unwrap(),
                tool,
                input_json: input,
                status,
                created_at: 1,
            },
        )
        .unwrap();
    }
    let auto_json = ApprovalAudit {
        decision: Approval::Once,
        note: None,
        source: ApprovalSource::Auto,
        danger: None,
    }
    .to_json()
    .unwrap();
    tool_calls::update(
        store.conn(),
        "recover-edit",
        "running",
        Some(&auto_json),
        None,
        None,
    )
    .unwrap();
    let database_path = data_dir.path().join("vega.db");
    drop(store);
    let reopened = Store::open(&database_path).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "recover-write".into(),
                name: "write".into(),
                input_json: write_raw.into(),
            },
            ProviderEvent::ToolUse {
                id: "recover-edit".into(),
                name: "edit".into(),
                input_json: edit_raw.into(),
            },
            ProviderEvent::ToolUse {
                id: "recover-unknown".into(),
                name: "future_tool".into(),
                input_json: r#"{"secret":"must-not-survive"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ]);
    let run = run_thread_task(
        &reopened,
        &provider,
        &tools,
        "thread-1",
        "resume",
        "system",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let reused = run
        .events
        .iter()
        .filter_map(|event| match event {
            ConversationEvent::ToolCallFinished { call_id, result } if result.reused => {
                Some((call_id.as_str(), result.status))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        reused,
        vec![
            ("recover-write", ToolCallStatus::Rejected),
            ("recover-edit", ToolCallStatus::Cancelled),
            ("recover-unknown", ToolCallStatus::Rejected),
        ]
    );
    assert!(!project_dir.path().join("new.txt").exists());
    assert_eq!(
        fs::read_to_string(project_dir.path().join("edit.txt")).unwrap(),
        "old"
    );
    let wire = format!("{:?}", provider.requests());
    assert!(!wire.contains("recovery-secret"));
    assert!(!wire.contains("recovery-new"));
    assert!(!wire.contains("must-not-survive"));
}
