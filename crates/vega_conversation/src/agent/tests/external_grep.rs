use super::*;
use crate::types::{ReadOnlyToolKind, ToolCardInputProjection, ToolCardResultProjection};
use vega_runtime::{
    RuntimeApprovalAudit, RuntimeApprovalDecision, RuntimeApprovalSource, RuntimePermissionTarget,
    RuntimeToolCall,
};
use vega_store::permissions as store_permissions;

struct GrepPermissionHook {
    requests: Arc<Mutex<Vec<PermissionRequest>>>,
    decision: PermissionDecision,
}

impl PermissionHook for GrepPermissionHook {
    fn request(
        &self,
        request: PermissionRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<PermissionDecision, VegaError>> {
        self.requests.lock().unwrap().push(request);
        let decision = self.decision.clone();
        async move { Ok(decision) }.boxed()
    }
}

fn grep_provider(id: &str, tool: &str, input: serde_json::Value) -> MockProvider {
    MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: id.into(),
                name: tool.into(),
                input_json: input.to_string(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ])
}

fn grep_terminal(run: &ConversationRun, id: &str) -> crate::types::ToolResult {
    run.events
        .iter()
        .find_map(|event| match event {
            ConversationEvent::ToolCallFinished { call_id, result } if call_id == id => {
                Some(result.clone())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing terminal for {id}: {:?}", run.events))
}

async fn reload_grep_history(store: &Store, tools: &vega_tools::Tools) {
    let provider = MockProvider::new(vec![ScriptStep::events(vec![ProviderEvent::Done {
        stop_reason: StopReason::End,
    }])]);
    let run = run_thread_task(
        store,
        &provider,
        tools,
        "thread-1",
        "continue",
        "system",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(!run.failed);
    assert_eq!(provider.requests().len(), 1);
}

struct CancelGrepPermissionHook;

impl PermissionHook for CancelGrepPermissionHook {
    fn request(
        &self,
        _request: PermissionRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<PermissionDecision, VegaError>> {
        cancel.cancel();
        async { Ok(PermissionDecision::Once) }.boxed()
    }
}

#[tokio::test]
async fn issue426_pending_grep_cancellation_persists_without_approval_or_rules() {
    let (store, project, data, project_id) = setup_external("confirm");
    let outside = tempdir().unwrap();
    let path = outside.path().canonicalize().unwrap().join("note.txt");
    fs::write(&path, "CANCEL_CANARY\n").unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let run = run_thread_task_with_permission_sink(
        &store,
        &grep_provider(
            "pending-cancel",
            "grep",
            serde_json::json!({"pattern":"CANCEL_CANARY", "path":path}),
        ),
        &tools,
        "thread-1",
        "search",
        "system",
        CancellationToken::new(),
        &CancelGrepPermissionHook,
        |_| Ok(()),
    )
    .await
    .unwrap();
    let result = grep_terminal(&run, "pending-cancel");
    assert_eq!(result.status, ToolCallStatus::Rejected);
    assert_eq!(result.output, "Tool error: permission denied");
    assert!(!result.output.contains("CANCEL_CANARY"));
    assert!(!run.events.iter().any(|event| matches!(
        event,
        ConversationEvent::ToolCallApproved { .. } | ConversationEvent::ToolCallRunning { .. }
    )));
    let state = tool_calls::find_state(store.conn(), "pending-cancel")
        .unwrap()
        .unwrap();
    assert_eq!(state.status, "rejected");
    let approval = ApprovalAudit::from_json(state.approval.as_deref().unwrap()).unwrap();
    assert_eq!(approval.decision, Approval::Deny);
    assert_eq!(approval.source, ApprovalSource::Timeout);
    assert!(
        store_permissions::list_exact(store.conn(), &project_id)
            .unwrap()
            .is_empty()
    );
    drop(store);
    let reopened = Store::open(data.path().join("vega.db")).unwrap();
    reload_grep_history(&reopened, &tools).await;
}

#[tokio::test]
async fn issue426_internal_grep_raw_receipt_reuses_after_reopen_and_rejects_changed_input() {
    let (store, project, data, _project_id) = setup_external("confirm");
    let file = project.path().join("local.txt");
    fs::write(&file, "needle RECEIPT_CANARY\n").unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let raw = r#"{ "path": "local.txt", "pattern": "needle" }"#;
    let first = run_thread_task(
        &store,
        &scripted_provider("internal-receipt", raw),
        &tools,
        "thread-1",
        "search",
        "system",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let result = grep_terminal(&first, "internal-receipt");
    assert_eq!(result.status, ToolCallStatus::Success);
    assert_eq!(result.output, "local.txt:1:needle RECEIPT_CANARY");
    assert!(!result.reused);
    let original = tool_calls::find_state(store.conn(), "internal-receipt")
        .unwrap()
        .unwrap();
    assert_eq!(original.input_json, raw);
    assert_eq!(original.status, "success");
    drop(store);
    let store = Store::open(data.path().join("vega.db")).unwrap();
    fs::remove_file(&file).unwrap();
    for (input, reuse) in [
        (raw.to_owned(), true),
        (raw.replace("needle", "changed"), false),
        (raw.replace("local.txt", "missing.txt"), false),
    ] {
        let run = run_thread_task(
            &store,
            &scripted_provider("internal-receipt", &input),
            &tools,
            "thread-1",
            "retry",
            "system",
            CancellationToken::new(),
        )
        .await
        .unwrap();
        let retry = grep_terminal(&run, "internal-receipt");
        assert_eq!(retry.reused, reuse);
        if reuse {
            assert_eq!(retry.status, ToolCallStatus::Success);
            assert_eq!(retry.output, result.output);
        } else {
            assert_eq!(retry.status, ToolCallStatus::Failed);
            assert_eq!(retry.output, vega_runtime::CALL_ID_CONFLICT_OUTPUT);
        }
        assert!(!run.events.iter().any(|event| matches!(
            event,
            ConversationEvent::ToolCallApproved { .. } | ConversationEvent::ToolCallRunning { .. }
        )));
        let saved = tool_calls::find_state(store.conn(), "internal-receipt")
            .unwrap()
            .unwrap();
        assert_eq!(saved.input_json, original.input_json);
        assert_eq!(saved.status, original.status);
        assert_eq!(saved.approval, original.approval);
        assert_eq!(saved.output_text, original.output_text);
    }
    reload_grep_history(&store, &tools).await;
}

#[test]
fn issue426_forged_pending_to_cancelled_is_always_rejected() {
    for tamper in 0..9 {
        let (store, _project, _data, project_id) = setup_external("confirm");
        seed_grep_message(&store);
        persist_grep_event(
            &store,
            &project_id,
            RuntimeEvent::ToolCallProposed(RuntimeToolCall {
                id: "pending-cancel".into(),
                name: if tamper == 1 { "glob" } else { "grep" }.into(),
                input_json: r#"{"pattern":"needle","path":"/external/search"}"#.into(),
            }),
        )
        .unwrap();
        let mut result = vega_runtime::RuntimeToolResult {
            call_id: "pending-cancel".into(),
            output: vega_runtime::CANCELLED_BEFORE_EXECUTION_OUTPUT.into(),
            status: RuntimeToolStatus::Cancelled,
            reused: false,
            exit_code: None,
            duration_ms: None,
            truncated: None,
            approval: Some(RuntimeApprovalAudit {
                decision: RuntimeApprovalDecision::Deny,
                note: None,
                source: RuntimeApprovalSource::Timeout,
                danger: None,
            }),
            remember_rule: None,
        };
        match tamper {
            2 => result.call_id = "wrong-call".into(),
            3 => result.output = "FORGED_CANCEL_CONTENT".into(),
            4 => result.exit_code = Some(0),
            5 => result.duration_ms = Some(1),
            6 => result.truncated = Some(false),
            7 => result.approval.as_mut().unwrap().source = RuntimeApprovalSource::User,
            8 => {
                result.remember_rule = Some(RuntimePermissionTarget {
                    call_id: "pending-cancel".into(),
                    tool: RuntimeMutatingTool::Read,
                    exact_pattern: "/external/search".into(),
                    display_target: "/external/search".into(),
                })
            }
            _ => {}
        }
        assert!(
            persist_grep_event(&store, &project_id, RuntimeEvent::ToolCallFinished(result))
                .is_err()
        );
        let state = tool_calls::find_state(store.conn(), "pending-cancel")
            .unwrap()
            .unwrap();
        assert_eq!(state.status, "pending_approval");
        assert!(state.approval.is_none());
        assert!(state.output_text.is_none());
        assert!(
            store_permissions::list_exact(store.conn(), &project_id)
                .unwrap()
                .is_empty()
        );
    }
}

#[tokio::test]
async fn issue426_external_grep_decisions_persist_and_reopen_with_safe_cards() {
    for directory in [false, true] {
        for (mode, decision, expected_status, expected_source) in [
            (
                "confirm",
                PermissionDecision::Once,
                ToolCallStatus::Success,
                ApprovalSource::User,
            ),
            (
                "auto",
                PermissionDecision::Always,
                ToolCallStatus::Success,
                ApprovalSource::User,
            ),
            (
                "readonly",
                PermissionDecision::Deny { note: None },
                ToolCallStatus::Rejected,
                ApprovalSource::User,
            ),
            (
                "confirm",
                PermissionDecision::Timeout,
                ToolCallStatus::Rejected,
                ApprovalSource::Timeout,
            ),
            (
                "full_access",
                PermissionDecision::Deny { note: None },
                ToolCallStatus::Success,
                ApprovalSource::FullAccess,
            ),
        ] {
            let (store, project, data, project_id) = setup_external(mode);
            let outside = tempdir().unwrap();
            let root = outside.path().canonicalize().unwrap();
            fs::create_dir(root.join("nested")).unwrap();
            fs::write(root.join("note.txt"), "MATCH_CANARY\n").unwrap();
            let path = if directory {
                root.clone()
            } else {
                root.join("note.txt")
            };
            let alias = if directory {
                root.join("nested/..")
            } else {
                root.join("nested/../note.txt")
            };
            let input = serde_json::json!({"pattern":"MATCH_CANARY", "path":alias});
            let provider = grep_provider("external-grep", "grep", input.clone());
            let requests = Arc::new(Mutex::new(Vec::new()));
            let always = decision == PermissionDecision::Always;
            let hook = GrepPermissionHook {
                requests: requests.clone(),
                decision,
            };
            let tools = vega_tools::Tools::new(project.path()).unwrap();
            let run = run_thread_task_with_permission_sink(
                &store,
                &provider,
                &tools,
                "thread-1",
                "search",
                "system",
                CancellationToken::new(),
                &hook,
                |_| Ok(()),
            )
            .await
            .unwrap();
            let result = grep_terminal(&run, "external-grep");
            assert_eq!(result.status, expected_status, "{}", result.output);
            if expected_status == ToolCallStatus::Success {
                assert!(result.output.contains("MATCH_CANARY"));
                assert!(
                    result
                        .output
                        .contains(root.join("note.txt").to_str().unwrap())
                );
                let wire = provider.requests();
                let live_call = wire[1]
                    .messages
                    .iter()
                    .flat_map(|message| &message.tool_calls)
                    .find(|call| call.id == "external-grep")
                    .unwrap();
                assert_eq!(
                    serde_json::from_str::<serde_json::Value>(&live_call.input_json).unwrap(),
                    input
                );
            } else {
                assert_eq!(result.output, "Tool error: permission denied");
                assert!(!result.output.contains("MATCH_CANARY"));
            }
            {
                let recorded = requests.lock().unwrap();
                assert_eq!(recorded.len(), usize::from(mode != "full_access"));
                if let Some(request) = recorded.first() {
                    assert_eq!(request.call_id, "external-grep");
                    assert_eq!(request.tool, "read");
                    assert_eq!(request.display_target, path.to_str().unwrap());
                    assert!(request.danger_rule_id.is_none());
                    assert!(request.external.is_none());
                }
            }
            let state = tool_calls::find_state(store.conn(), "external-grep")
                .unwrap()
                .unwrap();
            assert_eq!(state.tool, "grep");
            let audit = ApprovalAudit::from_json(state.approval.as_deref().unwrap()).unwrap();
            assert_eq!(audit.source, expected_source);
            assert_eq!(
                audit.decision,
                if always {
                    Approval::Always
                } else if expected_status == ToolCallStatus::Rejected {
                    Approval::Deny
                } else {
                    Approval::Once
                }
            );
            let saved: serde_json::Value = serde_json::from_str(&state.input_json).unwrap();
            assert_eq!(saved["path"], path.to_str().unwrap());
            assert_eq!(saved["pattern"], "MATCH_CANARY");
            let rules = store_permissions::list_exact(store.conn(), &project_id).unwrap();
            assert_eq!(rules.len(), usize::from(always));
            if always {
                assert_eq!(rules[0].tool, "read");
                assert_eq!(
                    rules[0].pattern,
                    if directory {
                        format!("grep-dir:v1:{}", path.display())
                    } else {
                        path.to_str().unwrap().to_string()
                    }
                );
            }
            drop(store);
            let reopened = Store::open(data.path().join("vega.db")).unwrap();
            let history = crate::history::restart_history_page(&reopened, "thread-1", 40).unwrap();
            assert!(history.entries.iter().any(|entry| matches!(entry,
                crate::history::HistoryEntry::Tool {
                    call_id, status,
                    input: Some(ToolCardInputProjection::ReadOnly { tool: ReadOnlyToolKind::Grep, permission_path: Some(permission_path), .. }),
                    result: Some(ToolCardResultProjection::ReadOnly { .. }), ..
                } if call_id == "external-grep" && *status == expected_status && permission_path == path.to_str().unwrap()
            )));
            reload_grep_history(&reopened, &tools).await;
        }
    }
}

#[tokio::test]
async fn issue426_directory_always_is_exact_and_does_not_inherit_or_grant_bare_read() {
    let (store, project, data, project_id) = setup_external("confirm");
    let outside = tempdir().unwrap();
    let root = outside.path().canonicalize().unwrap();
    let directory = root.join("search");
    let child = directory.join("child");
    let sibling = root.join("sibling");
    fs::create_dir_all(&child).unwrap();
    fs::create_dir(&sibling).unwrap();
    fs::write(directory.join("note.txt"), "needle\n").unwrap();
    fs::write(child.join("child.txt"), "needle\n").unwrap();
    fs::write(sibling.join("other.txt"), "needle\n").unwrap();
    store_permissions::insert_exact(
        store.conn(),
        store_permissions::InsertExactRule {
            project_id: &project_id,
            tool: "read",
            pattern: directory.to_str().unwrap(),
            created_at: 1,
        },
    )
    .unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let hook = GrepPermissionHook {
        requests: requests.clone(),
        decision: PermissionDecision::Always,
    };
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let run = run_thread_task_with_permission_sink(
        &store,
        &grep_provider(
            "grep-always",
            "grep",
            serde_json::json!({"pattern":"needle", "path":directory}),
        ),
        &tools,
        "thread-1",
        "search",
        "system",
        CancellationToken::new(),
        &hook,
        |_| Ok(()),
    )
    .await
    .unwrap();
    assert_eq!(
        grep_terminal(&run, "grep-always").status,
        ToolCallStatus::Success
    );
    assert_eq!(requests.lock().unwrap().len(), 1);
    let signature = format!("grep-dir:v1:{}", directory.display());
    assert!(
        store_permissions::matches_exact(store.conn(), &project_id, "read", &signature).unwrap()
    );
    drop(store);
    let reopened = Store::open(data.path().join("vega.db")).unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let hook = GrepPermissionHook {
        requests: requests.clone(),
        decision: PermissionDecision::Once,
    };
    for (id, tool, input, expected_source, expected_calls) in [
        (
            "grep-rule",
            "grep",
            serde_json::json!({"pattern":"needle", "path":directory}),
            ApprovalSource::Rule,
            1,
        ),
        (
            "grep-child",
            "grep",
            serde_json::json!({"pattern":"needle", "path":child}),
            ApprovalSource::User,
            2,
        ),
        (
            "grep-sibling",
            "grep",
            serde_json::json!({"pattern":"needle", "path":sibling}),
            ApprovalSource::User,
            3,
        ),
        (
            "grep-parent",
            "grep",
            serde_json::json!({"pattern":"needle", "path":root}),
            ApprovalSource::User,
            4,
        ),
        (
            "child-read",
            "read",
            serde_json::json!({"path":directory.join("note.txt")}),
            ApprovalSource::User,
            5,
        ),
    ] {
        let run = run_thread_task_with_permission_sink(
            &reopened,
            &grep_provider(id, tool, input),
            &tools,
            "thread-1",
            "search",
            "system",
            CancellationToken::new(),
            &hook,
            |_| Ok(()),
        )
        .await
        .unwrap();
        assert_eq!(grep_terminal(&run, id).status, ToolCallStatus::Success);
        assert_eq!(requests.lock().unwrap().len(), expected_calls);
        let state = tool_calls::find_state(reopened.conn(), id)
            .unwrap()
            .unwrap();
        assert_eq!(
            ApprovalAudit::from_json(state.approval.as_deref().unwrap())
                .unwrap()
                .source,
            expected_source
        );
    }
    assert_eq!(
        store_permissions::list_exact(reopened.conn(), &project_id)
            .unwrap()
            .len(),
        2
    );
    reload_grep_history(&reopened, &tools).await;
}

#[tokio::test]
async fn issue426_file_grep_reuses_existing_exact_read_rule() {
    let (store, project, _data, project_id) = setup_external("confirm");
    let outside = tempdir().unwrap();
    let path = outside.path().canonicalize().unwrap().join("note.txt");
    fs::write(&path, "needle\n").unwrap();
    store_permissions::insert_exact(
        store.conn(),
        store_permissions::InsertExactRule {
            project_id: &project_id,
            tool: "read",
            pattern: path.to_str().unwrap(),
            created_at: 1,
        },
    )
    .unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let hook = GrepPermissionHook {
        requests: requests.clone(),
        decision: PermissionDecision::Deny { note: None },
    };
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let run = run_thread_task_with_permission_sink(
        &store,
        &grep_provider(
            "file-rule",
            "grep",
            serde_json::json!({"pattern":"needle", "path":path}),
        ),
        &tools,
        "thread-1",
        "search",
        "system",
        CancellationToken::new(),
        &hook,
        |_| Ok(()),
    )
    .await
    .unwrap();
    assert_eq!(
        grep_terminal(&run, "file-rule").status,
        ToolCallStatus::Success
    );
    assert!(requests.lock().unwrap().is_empty());
    let state = tool_calls::find_state(store.conn(), "file-rule")
        .unwrap()
        .unwrap();
    assert_eq!(
        ApprovalAudit::from_json(state.approval.as_deref().unwrap())
            .unwrap()
            .source,
        ApprovalSource::Rule
    );
    reload_grep_history(&store, &tools).await;
}

#[tokio::test]
async fn issue426_cancel_after_approval_persists_cancelled_without_search_output() {
    let (store, project, data, _) = setup_external("confirm");
    let outside = tempdir().unwrap();
    let path = outside.path().canonicalize().unwrap().join("note.txt");
    fs::write(&path, "CANCEL_CANARY\n").unwrap();
    let cancel = CancellationToken::new();
    let hook = GrepPermissionHook {
        requests: Arc::new(Mutex::new(Vec::new())),
        decision: PermissionDecision::Once,
    };
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let run = run_thread_task_with_permission_sink(
        &store,
        &grep_provider(
            "cancel-grep",
            "grep",
            serde_json::json!({"pattern":"CANCEL_CANARY", "path":path}),
        ),
        &tools,
        "thread-1",
        "search",
        "system",
        cancel.clone(),
        &hook,
        |event| {
            if matches!(event, ConversationEvent::ToolCallApproved { .. }) {
                cancel.cancel();
            }
            Ok(())
        },
    )
    .await
    .unwrap();
    let result = grep_terminal(&run, "cancel-grep");
    assert_eq!(result.status, ToolCallStatus::Cancelled);
    assert!(!result.output.contains("CANCEL_CANARY"));
    let state = tool_calls::find_state(store.conn(), "cancel-grep")
        .unwrap()
        .unwrap();
    assert_eq!(state.status, "cancelled");
    drop(store);
    let reopened = Store::open(data.path().join("vega.db")).unwrap();
    reload_grep_history(&reopened, &tools).await;
}

#[tokio::test]
async fn issue426_invalid_grep_fails_then_corrects_and_survives_store_reopen() {
    use sha2::{Digest, Sha256};

    for input in [
        r#"{"pattern":"PRIVATE_PATTERN_CANARY(","path":null}"#,
        r#"{"pattern":7,"path":null}"#,
        r#"{"pattern":"PRIVATE_PATTERN_CANARY","path":7}"#,
        r#"{"pattern":"PRIVATE_PATTERN_CANARY","unsupported":true}"#,
        r#"{"pattern":"PRIVATE_PATTERN_CANARY","pattern":"needle"}"#,
        r#"{"pattern":"PRIVATE_PATTERN_CANARY""#,
    ] {
        let (store, project, data, project_id) = setup_external("confirm");
        fs::write(project.path().join("note.txt"), "CORRECTED_CANARY\n").unwrap();
        let tools = vega_tools::Tools::new(project.path()).unwrap();
        let hook = GrepPermissionHook {
            requests: Arc::new(Mutex::new(Vec::new())),
            decision: PermissionDecision::Once,
        };
        let provider = MockProvider::new_rounds(vec![
            vec![ScriptStep::events(vec![
                ProviderEvent::ToolUse {
                    id: "invalid-grep".into(),
                    name: "grep".into(),
                    input_json: input.into(),
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                },
            ])],
            vec![ScriptStep::events(vec![
                ProviderEvent::ToolUse {
                    id: "corrected-grep".into(),
                    name: "grep".into(),
                    input_json: r#"{"pattern":"CORRECTED_CANARY","path":null}"#.into(),
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
            "search",
            "system",
            CancellationToken::new(),
            &hook,
            |_| Ok(()),
        )
        .await
        .unwrap();
        assert!(!run.failed);
        let result = grep_terminal(&run, "invalid-grep");
        assert_eq!(result.status, ToolCallStatus::Failed);
        assert_eq!(result.output, vega_runtime::GREP_INVALID_INPUT_OUTPUT);
        assert!(!result.output.contains("PRIVATE_PATTERN_CANARY"));
        assert!(hook.requests.lock().unwrap().is_empty());
        let approved = run.events.iter().position(|event| matches!(event, ConversationEvent::ToolCallApproved { call_id, approval: Approval::Once } if call_id == "invalid-grep")).unwrap();
        let running = run.events.iter().position(|event| matches!(event, ConversationEvent::ToolCallRunning { call_id } if call_id == "invalid-grep")).unwrap();
        let finished = run.events.iter().position(|event| matches!(event, ConversationEvent::ToolCallFinished { call_id, .. } if call_id == "invalid-grep")).unwrap();
        assert!(approved < running && running < finished);
        let corrected = grep_terminal(&run, "corrected-grep");
        assert_eq!(corrected.status, ToolCallStatus::Success);
        assert!(corrected.output.contains("CORRECTED_CANARY"));
        let state = tool_calls::find_state(store.conn(), "invalid-grep")
            .unwrap()
            .unwrap();
        assert_eq!(state.status, "failed");
        assert!(crate::types::valid_grep_invalid_input(&state.input_json));
        assert!(!state.input_json.contains("PRIVATE_PATTERN_CANARY"));
        let audit = ApprovalAudit::from_json(state.approval.as_deref().unwrap()).unwrap();
        assert_eq!(audit.decision, Approval::Once);
        assert_eq!(audit.source, ApprovalSource::ReadonlyTool);
        let saved: serde_json::Value = serde_json::from_str(&state.input_json).unwrap();
        let mut digest = Sha256::new();
        digest.update(b"vega.grep.invalid-input.v1\0");
        digest.update(input.len().to_be_bytes());
        digest.update(input.as_bytes());
        assert_eq!(saved["raw_input_bytes"], input.len());
        assert_eq!(
            saved["raw_input_sha256"],
            format!("{:x}", digest.finalize())
        );
        assert_eq!(provider.requests().len(), 3);
        for request in provider.requests() {
            for call in request
                .messages
                .iter()
                .flat_map(|message| &message.tool_calls)
            {
                assert_ne!(call.id, "invalid-grep");
                assert!(!call.input_json.contains("raw_input_sha256"));
                assert!(!call.input_json.contains("PRIVATE_PATTERN_CANARY"));
            }
        }
        assert!(
            store_permissions::list_exact(store.conn(), &project_id)
                .unwrap()
                .is_empty()
        );
        drop(store);
        let reopened = Store::open(data.path().join("vega.db")).unwrap();
        let history = crate::history::restart_history_page(&reopened, "thread-1", 40).unwrap();
        assert!(history.entries.iter().any(|entry| matches!(entry,
            crate::history::HistoryEntry::Tool {
                call_id, status: ToolCallStatus::Failed,
                input: Some(ToolCardInputProjection::ReadOnly { tool: ReadOnlyToolKind::Grep, permission_path: None, display_target: None }),
                result: Some(ToolCardResultProjection::ReadOnly { status: ToolCallStatus::Failed, .. }), ..
            } if call_id == "invalid-grep"
        )));
        let provider = MockProvider::new(vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])]);
        let run = run_thread_task(
            &reopened,
            &provider,
            &tools,
            "thread-1",
            "continue",
            "system",
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert!(!run.failed);
        assert_eq!(provider.requests().len(), 1);
        for call in provider.requests()[0]
            .messages
            .iter()
            .flat_map(|message| &message.tool_calls)
        {
            assert_ne!(call.id, "invalid-grep");
            assert!(!call.input_json.contains("raw_input_sha256"));
        }
    }
}

fn seed_grep_message(store: &Store) {
    messages::insert(
        store.conn(),
        &messages::MessageRow {
            id: "grep-assistant".into(),
            thread_id: "thread-1".into(),
            seq: 1,
            role: "assistant".into(),
            kind: "text".into(),
            content: String::new(),
            status: "done".into(),
            created_at: 1,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .unwrap();
}

fn persist_grep_event(
    store: &Store,
    project_id: &str,
    event: RuntimeEvent,
) -> Result<(), VegaError> {
    persist_runtime_event(
        store,
        project_id,
        "thread-1",
        "grep-assistant",
        "mock-model",
        false,
        "",
        &mut 1,
        &event,
        None,
    )
}

#[test]
fn issue426_always_target_is_bound_before_store_approval_or_rule_insertion() {
    for (input, event_id, target_id, tool, exact, display, valid) in [
        (
            serde_json::json!({"pattern":"needle", "path":"/external/search"}),
            "grep-call",
            "grep-call",
            RuntimeMutatingTool::Read,
            "/external/search",
            "/external/search",
            true,
        ),
        (
            serde_json::json!({"pattern":"needle", "path":"/external/search"}),
            "grep-call",
            "grep-call",
            RuntimeMutatingTool::Read,
            "grep-dir:v1:/external/search",
            "/external/search",
            true,
        ),
        (
            serde_json::json!({"pattern":"needle", "path":"/external/search"}),
            "other-call",
            "other-call",
            RuntimeMutatingTool::Read,
            "/external/search",
            "/external/search",
            false,
        ),
        (
            serde_json::json!({"pattern":"needle", "path":"/external/search"}),
            "grep-call",
            "other-call",
            RuntimeMutatingTool::Read,
            "/external/search",
            "/external/search",
            false,
        ),
        (
            serde_json::json!({"pattern":"needle", "path":"/external/search"}),
            "grep-call",
            "grep-call",
            RuntimeMutatingTool::Bash,
            "/external/search",
            "/external/search",
            false,
        ),
        (
            serde_json::json!({"pattern":"needle", "path":"/external/search"}),
            "grep-call",
            "grep-call",
            RuntimeMutatingTool::Read,
            "/external/other",
            "/external/other",
            false,
        ),
        (
            serde_json::json!({"pattern":"needle", "path":"/external/search"}),
            "grep-call",
            "grep-call",
            RuntimeMutatingTool::Read,
            "grep-dir:v1:/external/other",
            "/external/search",
            false,
        ),
        (
            serde_json::json!({"pattern":"needle", "path":"/external/search"}),
            "grep-call",
            "grep-call",
            RuntimeMutatingTool::Read,
            "grep-dir:v1:/external/search/child",
            "/external/search",
            false,
        ),
        (
            serde_json::json!({"pattern":"needle", "path":"/external/search"}),
            "grep-call",
            "grep-call",
            RuntimeMutatingTool::Read,
            "grep-dir:v1:",
            "/external/search",
            false,
        ),
        (
            serde_json::json!({"pattern":"needle", "path":"/external/search"}),
            "grep-call",
            "grep-call",
            RuntimeMutatingTool::Read,
            "grep-dir:v2:/external/search",
            "/external/search",
            false,
        ),
        (
            serde_json::json!({"pattern":"needle", "path":"/external/search"}),
            "grep-call",
            "grep-call",
            RuntimeMutatingTool::Read,
            "grep-dir:v1:/external/search",
            "grep-dir:v1:/external/search",
            false,
        ),
        (
            serde_json::json!({"pattern":"needle", "path":"relative/search"}),
            "grep-call",
            "grep-call",
            RuntimeMutatingTool::Read,
            "relative/search",
            "relative/search",
            false,
        ),
        (
            serde_json::json!({"pattern":"needle", "path":"/external/../search"}),
            "grep-call",
            "grep-call",
            RuntimeMutatingTool::Read,
            "/external/../search",
            "/external/../search",
            false,
        ),
        (
            serde_json::json!({"path":"/external/search"}),
            "grep-call",
            "grep-call",
            RuntimeMutatingTool::Read,
            "/external/search",
            "/external/search",
            false,
        ),
        (
            serde_json::json!({"pattern":3, "path":"/external/search"}),
            "grep-call",
            "grep-call",
            RuntimeMutatingTool::Read,
            "/external/search",
            "/external/search",
            false,
        ),
    ] {
        let (store, _project, _data, project_id) = setup_external("confirm");
        seed_grep_message(&store);
        persist_grep_event(
            &store,
            &project_id,
            RuntimeEvent::ToolCallProposed(RuntimeToolCall {
                id: "grep-call".into(),
                name: "grep".into(),
                input_json: input.to_string(),
            }),
        )
        .unwrap();
        let outcome = persist_grep_event(
            &store,
            &project_id,
            RuntimeEvent::ToolCallApproved {
                call_id: event_id.into(),
                audit: RuntimeApprovalAudit {
                    decision: RuntimeApprovalDecision::Always,
                    note: None,
                    source: RuntimeApprovalSource::User,
                    danger: None,
                },
                remember_rule: Some(RuntimePermissionTarget {
                    call_id: target_id.into(),
                    tool,
                    exact_pattern: exact.into(),
                    display_target: display.into(),
                }),
            },
        );
        assert_eq!(
            outcome.is_ok(),
            valid,
            "{input} {event_id} {target_id} {tool:?} {exact} {display}"
        );
        let state = tool_calls::find_state(store.conn(), "grep-call")
            .unwrap()
            .unwrap();
        assert_eq!(
            state.status,
            if valid {
                "approved"
            } else {
                "pending_approval"
            }
        );
        let rules = store_permissions::list_exact(store.conn(), &project_id).unwrap();
        assert_eq!(rules.len(), usize::from(valid));
        if valid {
            assert_eq!(rules[0].pattern, exact);
            assert_eq!(rules[0].tool, "read");
        }
    }
}

#[test]
fn issue426_new_sources_require_valid_absolute_grep_and_do_not_authorize_glob() {
    for tool in ["grep", "glob"] {
        for input in [
            serde_json::json!({"pattern":"needle", "path":"/external/search"}),
            serde_json::json!({"pattern":"needle", "path":"relative/search"}),
            serde_json::json!({"pattern":"needle", "path":null}),
            serde_json::json!({"path":"/external/search"}),
            serde_json::json!({"pattern":7, "path":"/external/search"}),
        ] {
            for (source, decision) in [
                (ApprovalSource::User, Approval::Once),
                (ApprovalSource::Rule, Approval::Always),
                (ApprovalSource::FullAccess, Approval::Once),
                (ApprovalSource::Auto, Approval::Once),
            ] {
                let valid = tool == "grep"
                    && input["path"] == "/external/search"
                    && input["pattern"] == "needle"
                    && source != ApprovalSource::Auto;
                let audit = ApprovalAudit {
                    decision,
                    source,
                    note: None,
                    danger: None,
                };
                assert_eq!(
                    validate_recovered_projection(
                        "project",
                        "thread-1",
                        "grep-call",
                        tool,
                        &input.to_string(),
                        "result",
                        RuntimeToolStatus::Success,
                        &audit,
                        None,
                        None
                    )
                    .is_ok(),
                    valid
                );
                let (store, _project, _data, project_id) = setup_external("confirm");
                seed_grep_message(&store);
                persist_grep_event(
                    &store,
                    &project_id,
                    RuntimeEvent::ToolCallProposed(RuntimeToolCall {
                        id: "grep-call".into(),
                        name: tool.into(),
                        input_json: input.to_string(),
                    }),
                )
                .unwrap();
                let outcome = persist_grep_event(
                    &store,
                    &project_id,
                    RuntimeEvent::ToolCallApproved {
                        call_id: "grep-call".into(),
                        audit: approval_audit_to_runtime(&audit),
                        remember_rule: None,
                    },
                );
                assert_eq!(outcome.is_ok(), valid);
            }
        }
    }
}

fn invalid_grep_audit_input() -> String {
    serde_json::json!({
        "audit_version":"grep_invalid_v1", "raw_input_bytes":7,
        "raw_input_sha256":"a".repeat(64),
    })
    .to_string()
}

fn invalid_grep_readonly_approval() -> ApprovalAudit {
    ApprovalAudit {
        decision: Approval::Once,
        source: ApprovalSource::ReadonlyTool,
        note: None,
        danger: None,
    }
}

#[tokio::test]
async fn issue426_invalid_grep_cancellation_before_and_after_approval_reloads() {
    for after_approval in [false, true] {
        let (store, project, data, project_id) = setup_external("confirm");
        let tools = vega_tools::Tools::new(project.path()).unwrap();
        let cancel = CancellationToken::new();
        let hook = GrepPermissionHook {
            requests: Arc::new(Mutex::new(Vec::new())),
            decision: PermissionDecision::Once,
        };
        let run = run_thread_task_with_permission_sink(
            &store,
            &grep_provider("invalid-grep", "grep", serde_json::json!({"pattern":7})),
            &tools,
            "thread-1",
            "search",
            "system",
            cancel.clone(),
            &hook,
            |event| {
                let should_cancel = if after_approval {
                    matches!(event, ConversationEvent::ToolCallApproved { call_id, .. } if call_id == "invalid-grep")
                } else {
                    matches!(event, ConversationEvent::ToolCallProposed { call } if call.id == "invalid-grep")
                };
                if should_cancel {
                    cancel.cancel();
                }
                Ok(())
            },
        )
        .await
        .unwrap();
        let result = grep_terminal(&run, "invalid-grep");
        let (status, output, audit) = if after_approval {
            (
                ToolCallStatus::Cancelled,
                vega_runtime::CANCELLED_BEFORE_EXECUTION_OUTPUT,
                invalid_grep_readonly_approval(),
            )
        } else {
            (
                ToolCallStatus::Rejected,
                "Tool error: permission denied",
                ApprovalAudit {
                    decision: Approval::Deny,
                    source: ApprovalSource::Timeout,
                    note: None,
                    danger: None,
                },
            )
        };
        assert_eq!(result.status, status);
        assert_eq!(result.output, output);
        assert_eq!(run.events.iter().any(|event| matches!(event, ConversationEvent::ToolCallApproved { call_id, .. } if call_id == "invalid-grep")), after_approval);
        assert_eq!(run.events.iter().any(|event| matches!(event, ConversationEvent::ToolCallRunning { call_id } if call_id == "invalid-grep")), after_approval);
        let state = tool_calls::find_state(store.conn(), "invalid-grep")
            .unwrap()
            .unwrap();
        assert!(crate::types::valid_grep_invalid_input(&state.input_json));
        assert_eq!(
            ApprovalAudit::from_json(state.approval.as_deref().unwrap()).unwrap(),
            audit
        );
        assert!(hook.requests.lock().unwrap().is_empty());
        assert!(
            store_permissions::list_exact(store.conn(), &project_id)
                .unwrap()
                .is_empty()
        );
        drop(store);
        let reopened = Store::open(data.path().join("vega.db")).unwrap();
        reload_grep_history(&reopened, &tools).await;
    }
}

#[tokio::test]
async fn issue426_invalid_grep_incomplete_states_recover_and_reload() {
    for phase in ["pending_approval", "approved", "running"] {
        let (store, project, data, project_id) = setup_external("confirm");
        seed_grep_message(&store);
        persist_grep_event(
            &store,
            &project_id,
            RuntimeEvent::ToolCallProposed(RuntimeToolCall {
                id: "invalid-grep".into(),
                name: "grep".into(),
                input_json: invalid_grep_audit_input(),
            }),
        )
        .unwrap();
        if phase != "pending_approval" {
            persist_grep_event(
                &store,
                &project_id,
                RuntimeEvent::ToolCallApproved {
                    call_id: "invalid-grep".into(),
                    audit: approval_audit_to_runtime(&invalid_grep_readonly_approval()),
                    remember_rule: None,
                },
            )
            .unwrap();
        }
        if phase == "running" {
            persist_grep_event(
                &store,
                &project_id,
                RuntimeEvent::ToolCallRunning {
                    call_id: "invalid-grep".into(),
                },
            )
            .unwrap();
        }
        assert_eq!(
            tool_calls::find_state(store.conn(), "invalid-grep")
                .unwrap()
                .unwrap()
                .status,
            phase
        );
        drop(store);
        let reopened = Store::open(data.path().join("vega.db")).unwrap();
        let counts =
            vega_store::recovery::recover_thread(reopened.conn(), "thread-1", now_ms()).unwrap();
        let pending = phase == "pending_approval";
        assert_eq!(counts.tools_rejected, usize::from(pending));
        assert_eq!(counts.tools_cancelled, usize::from(!pending));
        let state = tool_calls::find_state(reopened.conn(), "invalid-grep")
            .unwrap()
            .unwrap();
        let (status, output, audit) = if pending {
            (
                "rejected",
                vega_store::recovery::RECOVERY_REJECTED_OUTPUT,
                ApprovalAudit {
                    decision: Approval::Deny,
                    source: ApprovalSource::Recovery,
                    note: None,
                    danger: None,
                },
            )
        } else {
            (
                "cancelled",
                vega_store::recovery::RECOVERY_CANCELLED_OUTPUT,
                invalid_grep_readonly_approval(),
            )
        };
        assert_eq!(state.status, status);
        assert_eq!(state.output_text.as_deref(), Some(output));
        assert_eq!(
            ApprovalAudit::from_json(state.approval.as_deref().unwrap()).unwrap(),
            audit
        );
        assert!(state.exit_code.is_none() && state.duration_ms.is_none());
        assert!(
            store_permissions::list_exact(reopened.conn(), &project_id)
                .unwrap()
                .is_empty()
        );
        let tools = vega_tools::Tools::new(project.path()).unwrap();
        reload_grep_history(&reopened, &tools).await;
    }
}

#[test]
fn issue426_invalid_grep_terminal_sources_and_fixed_outputs_are_exact() {
    let input = invalid_grep_audit_input();
    for (status, decision, source, output) in [
        (
            RuntimeToolStatus::Failed,
            Approval::Once,
            ApprovalSource::ReadonlyTool,
            vega_runtime::GREP_INVALID_INPUT_OUTPUT,
        ),
        (
            RuntimeToolStatus::Cancelled,
            Approval::Once,
            ApprovalSource::ReadonlyTool,
            vega_runtime::CANCELLED_BEFORE_EXECUTION_OUTPUT,
        ),
        (
            RuntimeToolStatus::Cancelled,
            Approval::Once,
            ApprovalSource::ReadonlyTool,
            vega_store::recovery::RECOVERY_CANCELLED_OUTPUT,
        ),
        (
            RuntimeToolStatus::Rejected,
            Approval::Deny,
            ApprovalSource::Recovery,
            vega_store::recovery::RECOVERY_REJECTED_OUTPUT,
        ),
        (
            RuntimeToolStatus::Rejected,
            Approval::Deny,
            ApprovalSource::Timeout,
            "Tool error: permission denied",
        ),
    ] {
        let audit = ApprovalAudit {
            decision,
            source,
            note: None,
            danger: None,
        };
        assert!(approval_source_matches(
            "grep", &input, status, source, false
        ));
        for tamper in 0..9 {
            let mut audit = audit.clone();
            let mut status = status;
            let mut output = output;
            let mut exit_code = None;
            let mut duration_ms = None;
            match tamper {
                1 => audit.source = ApprovalSource::User,
                2 => audit.decision = Approval::Always,
                3 => status = RuntimeToolStatus::Success,
                4 => output = "FORGED_PRIVATE_PATTERN_CANARY",
                5 => audit.note = Some("forged".into()),
                6 => {
                    audit.danger = Some(crate::types::DangerAudit {
                        rule_id: "forged".into(),
                        decision: Approval::Once,
                        note: None,
                    })
                }
                7 => exit_code = Some(0),
                8 => duration_ms = Some(1),
                _ => {}
            }
            assert_eq!(
                validate_recovered_projection(
                    "project",
                    "thread",
                    "invalid-grep",
                    "grep",
                    &input,
                    output,
                    status,
                    &audit,
                    exit_code,
                    duration_ms
                )
                .is_ok(),
                tamper == 0
            );
        }
        for other_source in [
            ApprovalSource::ReadonlyTool,
            ApprovalSource::Recovery,
            ApprovalSource::Timeout,
            ApprovalSource::User,
            ApprovalSource::Rule,
            ApprovalSource::FullAccess,
            ApprovalSource::Validation,
            ApprovalSource::Legacy,
        ] {
            assert_eq!(
                approval_source_matches("grep", &input, status, other_source, false),
                if status == RuntimeToolStatus::Rejected {
                    matches!(
                        other_source,
                        ApprovalSource::Recovery | ApprovalSource::Timeout
                    )
                } else {
                    other_source == ApprovalSource::ReadonlyTool
                }
            );
            let other_audit = ApprovalAudit {
                source: other_source,
                ..audit.clone()
            };
            assert_eq!(
                validate_recovered_projection(
                    "project",
                    "thread",
                    "invalid-grep",
                    "grep",
                    &input,
                    output,
                    status,
                    &other_audit,
                    None,
                    None
                )
                .is_ok(),
                other_source == source
            );
        }
        for other_output in [
            vega_runtime::GREP_INVALID_INPUT_OUTPUT,
            vega_runtime::CANCELLED_BEFORE_EXECUTION_OUTPUT,
            vega_store::recovery::RECOVERY_CANCELLED_OUTPUT,
            vega_store::recovery::RECOVERY_REJECTED_OUTPUT,
            "Tool error: permission denied",
        ] {
            let expected = other_output == output
                || (status == RuntimeToolStatus::Cancelled
                    && matches!(
                        other_output,
                        vega_runtime::CANCELLED_BEFORE_EXECUTION_OUTPUT
                            | vega_store::recovery::RECOVERY_CANCELLED_OUTPUT
                    ));
            assert_eq!(
                validate_recovered_projection(
                    "project",
                    "thread",
                    "invalid-grep",
                    "grep",
                    &input,
                    other_output,
                    status,
                    &audit,
                    None,
                    None
                )
                .is_ok(),
                expected
            );
        }
    }
    assert!(!approval_source_matches(
        "grep",
        &input,
        RuntimeToolStatus::Success,
        ApprovalSource::ReadonlyTool,
        false
    ));
    assert!(!approval_source_matches(
        "grep",
        &input,
        RuntimeToolStatus::Rejected,
        ApprovalSource::Validation,
        true
    ));
}

#[test]
fn issue426_invalid_grep_proposal_and_approval_reject_tampered_envelopes() {
    let valid = invalid_grep_audit_input();
    let hash = "a".repeat(64);
    let mut inputs = vec![valid.clone()];
    for (field, value) in [
        ("audit_version", serde_json::json!("grep_invalid_v2")),
        ("raw_input_bytes", serde_json::json!("7")),
        ("raw_input_sha256", serde_json::json!("A".repeat(64))),
        ("pattern", serde_json::json!("PRIVATE_PATTERN_CANARY")),
    ] {
        let mut input: serde_json::Value = serde_json::from_str(&valid).unwrap();
        input[field] = value;
        inputs.push(input.to_string());
    }
    inputs.push(format!(r#"{{"audit_version":"grep_invalid_v1","raw_input_bytes":7,"raw_input_bytes":7,"raw_input_sha256":"{hash}"}}"#));
    for (index, input_json) in inputs.into_iter().enumerate() {
        let (store, _project, _data, project_id) = setup_external("confirm");
        seed_grep_message(&store);
        let result = persist_grep_event(
            &store,
            &project_id,
            RuntimeEvent::ToolCallProposed(RuntimeToolCall {
                id: "invalid-grep".into(),
                name: "grep".into(),
                input_json,
            }),
        );
        assert_eq!(result.is_ok(), index == 0);
        assert_eq!(
            tool_calls::find_state(store.conn(), "invalid-grep")
                .unwrap()
                .is_some(),
            index == 0
        );
    }
    for (source, decision, valid) in [
        (ApprovalSource::ReadonlyTool, Approval::Once, true),
        (ApprovalSource::ReadonlyTool, Approval::Always, false),
        (ApprovalSource::User, Approval::Once, false),
        (ApprovalSource::Rule, Approval::Always, false),
        (ApprovalSource::FullAccess, Approval::Once, false),
        (ApprovalSource::Legacy, Approval::Once, false),
        (ApprovalSource::Recovery, Approval::Once, false),
        (ApprovalSource::Timeout, Approval::Once, false),
    ] {
        let (store, _project, _data, project_id) = setup_external("confirm");
        seed_grep_message(&store);
        persist_grep_event(
            &store,
            &project_id,
            RuntimeEvent::ToolCallProposed(RuntimeToolCall {
                id: "invalid-grep".into(),
                name: "grep".into(),
                input_json: invalid_grep_audit_input(),
            }),
        )
        .unwrap();
        let audit = ApprovalAudit {
            source,
            decision,
            ..invalid_grep_readonly_approval()
        };
        let result = persist_grep_event(
            &store,
            &project_id,
            RuntimeEvent::ToolCallApproved {
                call_id: "invalid-grep".into(),
                audit: approval_audit_to_runtime(&audit),
                remember_rule: None,
            },
        );
        assert_eq!(result.is_ok(), valid);
        assert_eq!(
            tool_calls::find_state(store.conn(), "invalid-grep")
                .unwrap()
                .unwrap()
                .status,
            if valid {
                "approved"
            } else {
                "pending_approval"
            }
        );
        assert!(
            store_permissions::list_exact(store.conn(), &project_id)
                .unwrap()
                .is_empty()
        );
    }
}

#[tokio::test]
async fn issue426_invalid_grep_reload_rejects_tampered_audit_feedback_source_and_status() {
    for tamper in 0..29 {
        let (store, project, data, project_id) = setup_external("confirm");
        seed_grep_message(&store);
        let mut input: serde_json::Value =
            serde_json::from_str(&invalid_grep_audit_input()).unwrap();
        let mut audit = invalid_grep_readonly_approval();
        let mut output = vega_runtime::GREP_INVALID_INPUT_OUTPUT;
        let mut status = "failed";
        let mut exit_code = None;
        let mut duration_ms = None;
        match tamper {
            1 => input["audit_version"] = "grep_invalid_v2".into(),
            2 => input["raw_input_bytes"] = "7".into(),
            3 => input["raw_input_sha256"] = "A".repeat(64).into(),
            4 => output = "Tool error: FORGED_PRIVATE_PATTERN_CANARY",
            5 => audit.source = ApprovalSource::Legacy,
            6 => audit.source = ApprovalSource::User,
            7 => audit.decision = Approval::Always,
            8 => status = "success",
            9 => status = "cancelled",
            10 => {
                status = "rejected";
                audit.decision = Approval::Deny;
                audit.source = ApprovalSource::Timeout;
                output = "Tool error: permission denied";
            }
            11 => input["pattern"] = "PRIVATE_PATTERN_CANARY".into(),
            12 => {
                input.as_object_mut().unwrap().remove("raw_input_sha256");
            }
            13 => exit_code = Some(0),
            14 => duration_ms = Some(1),
            15 => audit.note = Some("forged note".into()),
            18 | 19 | 21 | 25 => {
                status = "cancelled";
                output = if tamper == 18 {
                    vega_runtime::CANCELLED_BEFORE_EXECUTION_OUTPUT
                } else {
                    vega_store::recovery::RECOVERY_CANCELLED_OUTPUT
                };
                if tamper == 21 {
                    audit.source = ApprovalSource::Recovery;
                }
                if tamper == 25 {
                    audit.decision = Approval::Deny;
                }
            }
            20 | 22 | 23 | 24 | 26 | 27 | 28 => {
                status = "rejected";
                audit.decision = Approval::Deny;
                audit.source = match tamper {
                    22 => ApprovalSource::ReadonlyTool,
                    23 => ApprovalSource::Timeout,
                    _ => ApprovalSource::Recovery,
                };
                output = match tamper {
                    24 => "Tool error: permission denied",
                    26 => vega_runtime::GREP_INVALID_INPUT_OUTPUT,
                    _ => vega_store::recovery::RECOVERY_REJECTED_OUTPUT,
                };
                if tamper == 27 {
                    input["raw_input_bytes"] = "7".into();
                }
                if tamper == 28 {
                    input["pattern"] = "PRIVATE_PATTERN_CANARY".into();
                }
            }
            _ => {}
        }
        let input_json = match tamper {
            16 => format!(
                r#"{{"audit_version":"grep_invalid_v1","raw_input_bytes":7,"raw_input_bytes":7,"raw_input_sha256":"{}"}}"#,
                "a".repeat(64)
            ),
            17 => r#"{"pattern":"needle","path":"relative"}"#.into(),
            _ => input.to_string(),
        };
        tool_calls::insert_pending(
            store.conn(),
            tool_calls::NewToolCall {
                id: "invalid-grep",
                thread_id: "thread-1",
                message_id: "grep-assistant",
                seq: 1,
                tool: "grep",
                input_json: &input_json,
                status: "pending_approval",
                created_at: 1,
            },
        )
        .unwrap();
        let approval_json = if audit.source == ApprovalSource::Legacy {
            "once".to_string()
        } else {
            let source = match audit.source {
                ApprovalSource::ReadonlyTool => "readonly_tool",
                ApprovalSource::User => "user",
                ApprovalSource::Timeout => "timeout",
                ApprovalSource::Recovery => "recovery",
                source => panic!("unexpected approval source: {source:?}"),
            };
            serde_json::json!({
                "decision":audit.decision.as_str(), "source":source,
                "note":audit.note, "danger":null,
            })
            .to_string()
        };
        if status == "rejected" {
            tool_calls::reject(
                store.conn(),
                "invalid-grep",
                &approval_json,
                output,
                3,
                None,
            )
            .unwrap();
        } else {
            tool_calls::approve(store.conn(), "invalid-grep", &approval_json, None, 2).unwrap();
            tool_calls::mark_running(store.conn(), "invalid-grep").unwrap();
            tool_calls::finish(
                store.conn(),
                tool_calls::FinishToolCall {
                    id: "invalid-grep",
                    status,
                    output_text: output,
                    exit_code,
                    duration_ms,
                    finished_at: 3,
                },
            )
            .unwrap();
        }
        assert_eq!(
            validate_recovered_projection(
                &project_id,
                "thread-1",
                "invalid-grep",
                "grep",
                &input_json,
                output,
                match status {
                    "success" => RuntimeToolStatus::Success,
                    "cancelled" => RuntimeToolStatus::Cancelled,
                    "rejected" => RuntimeToolStatus::Rejected,
                    _ => RuntimeToolStatus::Failed,
                },
                &audit,
                exit_code,
                duration_ms
            )
            .is_ok(),
            matches!(tamper, 0 | 10 | 18 | 19 | 20)
        );
        drop(store);
        let reopened = Store::open(data.path().join("vega.db")).unwrap();
        let provider = MockProvider::new(vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])]);
        let tools = vega_tools::Tools::new(project.path()).unwrap();
        let run = run_thread_task(
            &reopened,
            &provider,
            &tools,
            "thread-1",
            "continue",
            "system",
            CancellationToken::new(),
        )
        .await;
        if matches!(tamper, 0 | 10 | 18 | 19 | 20) {
            assert!(!run.unwrap().failed);
            assert_eq!(provider.requests().len(), 1);
        } else {
            assert!(run.is_err(), "accepted tamper {tamper}");
            assert!(provider.requests().is_empty());
        }
    }
}

#[tokio::test]
async fn issue426_existing_relative_readonly_grep_and_glob_history_remains_valid() {
    for tool in ["grep", "glob"] {
        for legacy in [false, true] {
            let (store, project, data, project_id) = setup_external("confirm");
            seed_grep_message(&store);
            let input = serde_json::json!({"pattern":"needle", "path":"relative"}).to_string();
            tool_calls::insert_pending(
                store.conn(),
                tool_calls::NewToolCall {
                    id: "old-readonly",
                    thread_id: "thread-1",
                    message_id: "grep-assistant",
                    seq: 1,
                    tool,
                    input_json: &input,
                    status: "pending_approval",
                    created_at: 1,
                },
            )
            .unwrap();
            let audit = if legacy {
                "once".to_string()
            } else {
                ApprovalAudit {
                    decision: Approval::Once,
                    source: ApprovalSource::ReadonlyTool,
                    note: None,
                    danger: None,
                }
                .to_json()
                .unwrap()
            };
            tool_calls::approve(store.conn(), "old-readonly", &audit, None, 2).unwrap();
            tool_calls::mark_running(store.conn(), "old-readonly").unwrap();
            tool_calls::finish(
                store.conn(),
                tool_calls::FinishToolCall {
                    id: "old-readonly",
                    status: "success",
                    output_text: "relative/note.txt:1:needle",
                    exit_code: None,
                    duration_ms: None,
                    finished_at: 3,
                },
            )
            .unwrap();
            assert!(
                store_permissions::list_exact(store.conn(), &project_id)
                    .unwrap()
                    .is_empty()
            );
            drop(store);
            let reopened = Store::open(data.path().join("vega.db")).unwrap();
            let tools = vega_tools::Tools::new(project.path()).unwrap();
            reload_grep_history(&reopened, &tools).await;
        }
    }
}
