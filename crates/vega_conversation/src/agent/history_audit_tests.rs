use super::*;
use sha2::{Digest, Sha256};
use std::cell::Cell;
use vega_runtime::{
    ChatMessage, ChatRequest, ChatRole, CompletedToolCall, EventStream, ProviderEvent,
    RuntimeApprovalAudit, RuntimeApprovalDecision, RuntimeApprovalSource, RuntimePermissionPrompt,
    RuntimeToolCall, RuntimeToolResult, StopReason, run_agent_with_permission_sink,
};
use vega_store::threads;

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Fixture {
    directory: PathBuf,
    path: PathBuf,
    store: Store,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

fn fixture() -> Result<Fixture, Box<dyn std::error::Error>> {
    let directory =
        std::env::temp_dir().join(format!("vega-history-audit-{}", ulid::Ulid::generate()));
    std::fs::create_dir(&directory)?;
    let path = directory.join("history.sqlite");
    let store = Store::open(&path)?;
    store.migrate()?;
    for id in ["current", "other", "child"] {
        threads::create_standalone(
            store.conn(),
            threads::NewThread {
                id,
                project_id: "",
                title: "Synthetic",
                mode: "execute",
                permission_mode: "readonly",
                permission_revision: 0,
                full_access_grant: None,
                reasoning_preferences: None,
                model: "synthetic",
                status: "active",
                pinned: false,
                unread: false,
                created_at: 1,
                updated_at: 1,
            },
        )?;
    }
    for (id, thread, seq, role, content) in [
        (
            "user",
            "current",
            1,
            "user",
            format!("甲乙SYNTHETIC-SECRET-0123456789{}", "界".repeat(1100)),
        ),
        ("old", "current", 2, "assistant", "older response".into()),
        ("run", "current", 3, "assistant", "current response".into()),
        (
            "foreign",
            "other",
            1,
            "user",
            "foreign private content".into(),
        ),
        (
            "child-result",
            "child",
            1,
            "assistant",
            "child response".into(),
        ),
    ] {
        messages::insert(
            store.conn(),
            &messages::MessageRow {
                id: id.into(),
                thread_id: thread.into(),
                seq,
                role: role.into(),
                kind: "text".into(),
                content,
                status: "done".into(),
                created_at: seq,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )?;
    }
    let path_text = "references/example.md";
    let body = "saved skill resource";
    let skill_input = serde_json::json!({
        "name":"demo", "path_bytes":path_text.len(),
        "path_sha256":format!("{:x}", Sha256::digest(path_text.as_bytes()))
    })
    .to_string();
    let skill_output = format!(
        "[Lower-trust Skill reference]\n{}",
        serde_json::json!({
            "name":"demo", "path":path_text, "text":body, "lower_trust":true,
            "content_sha256":format!("{:x}", Sha256::digest(body.as_bytes()))
        })
    );
    let spawn_input = r#"{"name":"child","task":"inspect"}"#;
    for (id, seq, tool, input, output) in [
        (
            "resource",
            1,
            "read_skill_resource",
            skill_input.as_str(),
            skill_output.as_str(),
        ),
        ("spawn", 2, "spawn_agent", spawn_input, "{}"),
    ] {
        tool_calls::insert(
            store.conn(),
            tool_calls::NewToolCall {
                id,
                thread_id: "current",
                message_id: "old",
                seq,
                tool,
                input_json: input,
                description: None,
                status: "success",
                created_at: 1,
            },
        )?;
        store.conn().execute(
            "UPDATE tool_calls SET output_text=?1, approval=?2 WHERE id=?3",
            (
                output,
                r#"{"decision":"once","note":null,"source":"readonly_tool","danger":null}"#,
                id,
            ),
        )?;
    }
    store.conn().execute("INSERT INTO delegated_executions(child_message_id,child_thread_id,parent_thread_id,parent_message_id,request_call_id,ordinal,kind,name,task,state,result,truncated,created_at) VALUES('child-result','child','current','old','spawn',1,'spawn','child','inspect','completed','delegated result',0,1)", [])?;
    Ok(Fixture {
        directory,
        path,
        store,
    })
}

fn input(kind: &str, source: Option<&str>, offset: usize) -> String {
    serde_json::json!({
        "kind":kind,"source_id":source,"after_sequence":0,"limit":1,"content_offset":offset
    })
    .to_string()
}

struct ScriptedProvider {
    calls: Vec<(String, String, String)>,
    requests: Mutex<Vec<ChatRequest>>,
}

impl Provider for ScriptedProvider {
    fn chat_stream(
        &self,
        request: ChatRequest,
        _: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        let mut requests = self.requests.lock().unwrap();
        let first = requests.is_empty();
        requests.push(request);
        let calls = self.calls.clone();
        Box::pin(async move {
            let events = if first {
                let mut events = calls
                    .into_iter()
                    .map(|(id, name, input_json)| {
                        Ok(ProviderEvent::ToolUse {
                            id,
                            name,
                            input_json,
                        })
                    })
                    .collect::<Vec<_>>();
                events.push(Ok(ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                }));
                events
            } else {
                vec![
                    Ok(ProviderEvent::TextDelta("continued".into())),
                    Ok(ProviderEvent::Done {
                        stop_reason: StopReason::End,
                    }),
                ]
            };
            Ok(Box::pin(futures::stream::iter(events)) as EventStream)
        })
    }
}

struct RejectPermission;
impl RuntimePermissionHook for RejectPermission {
    fn request(
        &self,
        _: RuntimePermissionPrompt,
        _: CancellationToken,
    ) -> BoxFuture<'static, Result<RuntimeUserDecision, VegaError>> {
        async { Ok(RuntimeUserDecision::Unavailable) }.boxed()
    }
}

fn request(
    config: RuntimeToolConfig,
    completed: std::collections::HashMap<String, CompletedToolCall>,
) -> AgentRequest {
    AgentRequest {
        model: "synthetic".into(),
        system_prompt: "system".into(),
        history: vec![ChatMessage::new(ChatRole::User, "continue")],
        max_tokens: None,
        completed_tool_results: completed,
        tool_config: config,
        pricing_catalog: None,
        reasoning: None,
        model_snapshot: None,
        replay_frame: None,
        context_budget: None,
        context_source_version: None,
        context_source_fingerprint: None,
        context_operation_id: None,
        context_compaction_hook: None,
    }
}

fn persist(store: &Store, seq: &Cell<i64>, event: &RuntimeEvent) -> Result<(), VegaError> {
    if !matches!(
        event,
        RuntimeEvent::ToolCallProposed(_)
            | RuntimeEvent::ToolCallApproved { .. }
            | RuntimeEvent::ToolCallRunning { .. }
            | RuntimeEvent::ToolCallFinished(_)
            | RuntimeEvent::ToolCallConflict { .. }
            | RuntimeEvent::ToolCallValidationRejected { .. }
    ) {
        return Ok(());
    }
    let mut next = seq.get();
    let result = events::persist_runtime_event(
        store,
        "",
        "current",
        "run",
        "synthetic",
        false,
        "",
        &mut next,
        event,
        None,
    );
    seq.set(next);
    result
}

fn scripted_provider(calls: Vec<(&str, &str, String)>) -> ScriptedProvider {
    ScriptedProvider {
        calls: calls
            .into_iter()
            .map(|(id, name, input)| (id.into(), name.into(), input))
            .collect(),
        requests: Mutex::new(Vec::new()),
    }
}

fn config(fixture: &Fixture) -> RuntimeToolConfig {
    RuntimeToolConfig::default()
        .with_conversation_history_reader(context_history::reader(
            fixture.path.clone(),
            "current".into(),
        ))
        .with_credential_reader(Arc::new(|| Ok(vec!["SYNTHETIC-SECRET-0123456789".into()])))
}

#[tokio::test]
async fn history_calls_survive_persistence_reopen_and_same_id_reuse() -> TestResult {
    let fixture = fixture()?;
    let tools = vega_tools::Tools::new(&fixture.directory)?;
    let message_input = input("messages", Some("user"), 2);
    let calls = vec![
        ("messages-call", "read_conversation", message_input.clone()),
        ("tools-call", "read_conversation", input("tools", Some("resource"), 0)),
        ("skill-call", "read_conversation", input("skill_resources", Some("resource"), 0)),
        ("delegated-call", "read_conversation", input("delegated_results", Some("spawn"), 0)),
        ("foreign-call", "read_conversation", input("messages", Some("foreign"), 0)),
        ("page-one", "read_conversation", input("messages", None, 0)),
        ("page-two", "read_conversation", serde_json::json!({"kind":"messages","source_id":null,"after_sequence":1,"limit":1,"content_offset":0}).to_string()),
        ("invalid-call", "read_conversation", r#"{"kind":"messages","source_id":null,"after_sequence":0,"limit":9,"content_offset":0,"thread_id":"other"}"#.into()),
        ("glob-call", "glob", r#"{"pattern":"*.sqlite"}"#.into()),
    ];
    let provider = scripted_provider(calls);
    let seq = Cell::new(tool_calls::next_seq(fixture.store.conn(), "current")?);
    let outcome = run_agent_with_permission_sink(
        &provider,
        &tools,
        request(config(&fixture), Default::default()),
        CancellationToken::new(),
        &RejectPermission,
        |event| futures::future::ready(persist(&fixture.store, &seq, &event)),
    )
    .await?;
    assert!(!outcome.failed);
    assert_eq!(outcome.final_text, "continued");
    {
        let requests = provider.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(
            requests[0]
                .tools
                .iter()
                .any(|tool| tool.name == "read_conversation")
        );
        let model_output = |id: &str| {
            requests[1]
                .messages
                .iter()
                .find(|message| message.tool_call_id.as_deref() == Some(id))
                .map(|message| message.content.as_str())
                .unwrap()
        };
        let result: serde_json::Value = serde_json::from_str(model_output("messages-call"))?;
        let observation = &result["vega_history_observation"];
        assert_eq!(observation["authority"], "none");
        assert_eq!(observation["page"]["records"][0]["source_id"], "user");
        assert_eq!(observation["page"]["records"][0]["content"]["offset"], 2);
        assert!(observation["page"]["records"][0]["content"]["next_offset"].is_number());
        assert!(!model_output("messages-call").contains("SYNTHETIC-SECRET-0123456789"));
        assert!(model_output("tools-call").contains("saved skill resource"));
        assert!(model_output("skill-call").contains("saved skill resource"));
        assert!(model_output("delegated-call").contains("delegated result"));
        let first_page: serde_json::Value = serde_json::from_str(model_output("page-one"))?;
        assert_eq!(
            first_page["vega_history_observation"]["page"]["next_after_sequence"],
            1
        );
        assert_eq!(
            first_page["vega_history_observation"]["page"]["has_more"],
            true
        );
        let second_page: serde_json::Value = serde_json::from_str(model_output("page-two"))?;
        assert_eq!(
            second_page["vega_history_observation"]["page"]["records"][0]["source_id"],
            "old"
        );
        let foreign: serde_json::Value = serde_json::from_str(model_output("foreign-call"))?;
        assert_eq!(
            foreign["vega_history_observation"]["page"]["records"],
            serde_json::json!([])
        );
        assert!(model_output("invalid-call").contains("unavailable tool"));
        assert!(!model_output("foreign-call").contains("foreign private content"));
    }

    let terminal = tool_calls::terminal_results(fixture.store.conn(), "current")?;
    for id in [
        "messages-call",
        "tools-call",
        "skill-call",
        "delegated-call",
        "foreign-call",
        "page-one",
        "page-two",
        "invalid-call",
    ] {
        let call = &terminal[id];
        let approval = ApprovalAudit::from_json(&call.approval)?;
        let status = if call.status == "rejected" {
            RuntimeToolStatus::Rejected
        } else {
            RuntimeToolStatus::Success
        };
        pipeline::validate_recovered_projection(
            "",
            "current",
            id,
            &call.tool,
            &call.input_json,
            &call.output,
            status,
            &approval,
            call.exit_code,
            call.duration_ms,
        )?;
    }
    assert_eq!(terminal["invalid-call"].input_json, "{}");
    let page = crate::history::latest_history_page(&fixture.store, "current", 100)?;
    for id in [
        "messages-call",
        "tools-call",
        "skill-call",
        "delegated-call",
        "foreign-call",
        "page-one",
        "page-two",
        "invalid-call",
    ] {
        assert!(page.entries.iter().any(|entry| matches!(entry,
            crate::history::HistoryEntry::Tool { call_id, input: Some(crate::types::ToolCardInputProjection::ReadOnly { tool: crate::types::ReadOnlyToolKind::ConversationHistory, .. }), result: Some(crate::types::ToolCardResultProjection::ReadOnly { .. }), .. } if call_id == id)));
    }
    let previous = &terminal["messages-call"];
    let approval = ApprovalAudit::from_json(&previous.approval)?;
    let completed = std::collections::HashMap::from([(
        "messages-call".into(),
        CompletedToolCall {
            tool: previous.tool.clone(),
            input_json: previous.input_json.clone(),
            result: RuntimeToolResult {
                call_id: "messages-call".into(),
                output: previous.output.clone(),
                status: RuntimeToolStatus::Success,
                reused: true,
                exit_code: None,
                duration_ms: None,
                truncated: None,
                approval: Some(approval_audit_to_runtime(&approval)),
                remember_rule: None,
            },
        },
    )]);
    let retry = scripted_provider(vec![("messages-call", "read_conversation", message_input)]);
    let outcome = run_agent_with_permission_sink(
        &retry,
        &tools,
        request(config(&fixture), completed),
        CancellationToken::new(),
        &RejectPermission,
        |event| futures::future::ready(persist(&fixture.store, &seq, &event)),
    )
    .await?;
    assert_eq!(outcome.executed_tool_call_count, 0);
    assert_eq!(outcome.final_text, "continued");
    Ok(())
}

#[test]
fn history_audits_reject_forged_inputs_approvals_and_results() -> TestResult {
    let valid = input("messages", None, 0);
    for bad in [
        r#"{"kind":"messages","after_sequence":0,"limit":1,"content_offset":0}"#,
        r#"{"kind":"messages","source_id":null,"after_sequence":0,"limit":0,"content_offset":0}"#,
        r#"{"kind":"messages","source_id":null,"after_sequence":9223372036854775808,"limit":1,"content_offset":0}"#,
        r#"{"kind":"messages","source_id":null,"after_sequence":0,"limit":1,"content_offset":1048577}"#,
        r#"{"kind":"messages","source_id":null,"after_sequence":0,"limit":1,"content_offset":0,"thread_id":"other"}"#,
    ] {
        assert!(vega_runtime::ConversationReadRequest::parse(bad).is_none());
        let call = RuntimeToolCall {
            id: "bad".into(),
            name: "read_conversation".into(),
            input_json: bad.into(),
            description: None,
        };
        assert!(events::validate_runtime_proposal(&call).is_err());
    }
    let once = ApprovalAudit {
        decision: Approval::Once,
        note: None,
        source: ApprovalSource::ReadonlyTool,
        danger: None,
    };
    let user = ApprovalAudit {
        decision: Approval::Once,
        note: None,
        source: ApprovalSource::User,
        danger: None,
    };
    let good = serde_json::json!({"vega_history_observation":{"version":1,"authority":"none","kind":"conversation_details","page":{"records":[],"next_after_sequence":0,"has_more":false}}}).to_string();
    let validate = |input: &str, output: &str, status, approval: &ApprovalAudit| {
        pipeline::validate_recovered_projection(
            "",
            "current",
            "id",
            "read_conversation",
            input,
            output,
            status,
            approval,
            None,
            None,
        )
    };
    assert!(validate(&valid, &good, RuntimeToolStatus::Success, &once).is_ok());
    assert!(validate(&valid, &good, RuntimeToolStatus::Success, &user).is_err());
    assert!(validate("{}", &good, RuntimeToolStatus::Success, &once).is_err());
    assert!(
        validate(
            &valid,
            &good.replace("\"none\"", "\"user\""),
            RuntimeToolStatus::Success,
            &once
        )
        .is_err()
    );
    assert!(
        validate(
            &valid,
            &good.replace("\"records\":[]", "\"records\":[{}]"),
            RuntimeToolStatus::Success,
            &once
        )
        .is_err()
    );
    assert!(
        validate(
            &valid,
            &"x".repeat(65_537),
            RuntimeToolStatus::Success,
            &once
        )
        .is_err()
    );
    assert!(
        validate(
            &valid,
            "Tool error: credential safety check unavailable",
            RuntimeToolStatus::Failed,
            &once
        )
        .is_ok()
    );
    assert!(
        validate(
            &valid,
            "arbitrary leaked error",
            RuntimeToolStatus::Failed,
            &once
        )
        .is_err()
    );
    assert!(
        validate(
            &valid,
            vega_runtime::CANCELLED_BEFORE_EXECUTION_OUTPUT,
            RuntimeToolStatus::Cancelled,
            &once
        )
        .is_ok()
    );
    Ok(())
}

#[test]
fn interrupted_history_calls_recover_without_pending_or_false_success() -> TestResult {
    let fixture = fixture()?;
    let seq = Cell::new(tool_calls::next_seq(fixture.store.conn(), "current")?);
    let audit = RuntimeApprovalAudit {
        decision: RuntimeApprovalDecision::Once,
        note: None,
        source: RuntimeApprovalSource::ReadonlyTool,
        danger: None,
    };
    for id in ["pending", "running"] {
        let call = RuntimeToolCall {
            id: id.into(),
            name: "read_conversation".into(),
            input_json: input("messages", None, 0),
            description: None,
        };
        persist(&fixture.store, &seq, &RuntimeEvent::ToolCallProposed(call))?;
        if id == "running" {
            persist(
                &fixture.store,
                &seq,
                &RuntimeEvent::ToolCallApproved {
                    call_id: id.into(),
                    audit: audit.clone(),
                    remember_rule: None,
                },
            )?;
            persist(
                &fixture.store,
                &seq,
                &RuntimeEvent::ToolCallRunning { call_id: id.into() },
            )?;
        }
    }
    vega_store::recovery::recover_thread(fixture.store.conn(), "current", 42)?;
    let terminal = tool_calls::terminal_results(fixture.store.conn(), "current")?;
    for (id, expected, status) in [
        ("pending", "rejected", RuntimeToolStatus::Rejected),
        ("running", "cancelled", RuntimeToolStatus::Cancelled),
    ] {
        let row = &terminal[id];
        assert_eq!(row.status, expected);
        let approval = ApprovalAudit::from_json(&row.approval)?;
        pipeline::validate_recovered_projection(
            "",
            "current",
            id,
            &row.tool,
            &row.input_json,
            &row.output,
            status,
            &approval,
            row.exit_code,
            row.duration_ms,
        )?;
    }
    let page = crate::history::latest_history_page(&fixture.store, "current", 100)?;
    for id in ["pending", "running"] {
        assert!(page.entries.iter().any(|entry| matches!(entry,
            crate::history::HistoryEntry::Tool { call_id, result: Some(crate::types::ToolCardResultProjection::ReadOnly { .. }), .. } if call_id == id)));
    }
    Ok(())
}

#[tokio::test]
async fn history_reader_failure_and_exclusive_batches_remain_recoverable() -> TestResult {
    let fixture = fixture()?;
    let tools = vega_tools::Tools::new(&fixture.directory)?;
    let seq = Cell::new(tool_calls::next_seq(fixture.store.conn(), "current")?);
    let failing = RuntimeToolConfig::default().with_conversation_history_reader(Arc::new(|_| {
        Err(VegaError::Tool {
            tool: "read_conversation".into(),
            message: "private details".into(),
        })
    }));
    let failed_provider = scripted_provider(vec![(
        "failure",
        "read_conversation",
        input("messages", None, 0),
    )]);
    let outcome = run_agent_with_permission_sink(
        &failed_provider,
        &tools,
        request(failing, Default::default()),
        CancellationToken::new(),
        &RejectPermission,
        |event| futures::future::ready(persist(&fixture.store, &seq, &event)),
    )
    .await?;
    assert_eq!(outcome.final_text, "continued");
    let failed =
        tool_calls::find_state(fixture.store.conn(), "failure")?.ok_or("missing failed call")?;
    assert_eq!(failed.status, "failed");
    assert_eq!(
        failed.output_text.as_deref(),
        Some("Tool error: conversation details unavailable; check the source or retry")
    );
    assert!(
        !failed
            .output_text
            .as_deref()
            .unwrap()
            .contains("private details")
    );
    let approval = ApprovalAudit::from_json(failed.approval.as_deref().ok_or("missing approval")?)?;
    pipeline::validate_recovered_projection(
        "",
        "current",
        "failure",
        &failed.tool,
        &failed.input_json,
        failed.output_text.as_deref().unwrap(),
        RuntimeToolStatus::Failed,
        &approval,
        None,
        None,
    )?;

    let question =
        serde_json::json!({"questions":[{"id":"choice","header":"Choice","question":"Choose?",
        "options":[{"label":"A","description":"first"},{"label":"B","description":"second"}]}]})
        .to_string();
    let exclusive = scripted_provider(vec![
        (
            "mixed-history",
            "read_conversation",
            input("messages", None, 0),
        ),
        ("question", "ask_user_question", question),
    ]);
    let outcome = run_agent_with_permission_sink(
        &exclusive,
        &tools,
        request(config(&fixture), Default::default()),
        CancellationToken::new(),
        &RejectPermission,
        |event| futures::future::ready(persist(&fixture.store, &seq, &event)),
    )
    .await?;
    assert_eq!(outcome.final_text, "continued");
    let rejected = tool_calls::find_state(fixture.store.conn(), "mixed-history")?
        .ok_or("missing rejected call")?;
    assert_eq!(rejected.status, "rejected");
    assert_eq!(
        rejected.output_text.as_deref(),
        Some(vega_runtime::USER_QUESTION_BATCH_REJECTED)
    );
    let approval =
        ApprovalAudit::from_json(rejected.approval.as_deref().ok_or("missing approval")?)?;
    pipeline::validate_recovered_projection(
        "",
        "current",
        "mixed-history",
        &rejected.tool,
        &rejected.input_json,
        rejected.output_text.as_deref().unwrap(),
        RuntimeToolStatus::Rejected,
        &approval,
        None,
        None,
    )?;
    Ok(())
}

#[tokio::test]
async fn cancellation_waits_for_reader_and_persists_no_history_content() -> TestResult {
    let fixture = fixture()?;
    let tools = vega_tools::Tools::new(&fixture.directory)?;
    let seq = Cell::new(tool_calls::next_seq(fixture.store.conn(), "current")?);
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let entered = Mutex::new(Some(entered_tx));
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let release = Mutex::new(release_rx);
    let config =
        RuntimeToolConfig::default().with_conversation_history_reader(Arc::new(move |_| {
            if let Some(entered) = entered.lock().unwrap().take() {
                let _ = entered.send(());
            }
            release
                .lock()
                .unwrap()
                .recv()
                .map_err(|_| VegaError::Cancelled)?;
            Ok(serde_json::json!({"records":[{"content":"never deliver this"}]}))
        }));
    let provider = scripted_provider(vec![(
        "cancelled",
        "read_conversation",
        input("messages", None, 0),
    )]);
    let cancel = CancellationToken::new();
    let run = run_agent_with_permission_sink(
        &provider,
        &tools,
        request(config, Default::default()),
        cancel.clone(),
        &RejectPermission,
        |event| futures::future::ready(persist(&fixture.store, &seq, &event)),
    );
    let stop = async {
        entered_rx.await?;
        cancel.cancel();
        release_tx.send(())?;
        Ok::<(), Box<dyn std::error::Error>>(())
    };
    let (outcome, stopped) = tokio::join!(run, stop);
    stopped?;
    assert!(outcome?.interrupted);
    let row = tool_calls::find_state(fixture.store.conn(), "cancelled")?
        .ok_or("missing cancelled call")?;
    assert_eq!(row.status, "cancelled");
    assert_eq!(
        row.output_text.as_deref(),
        Some(vega_runtime::CANCELLED_BEFORE_EXECUTION_OUTPUT)
    );
    assert!(
        !row.output_text
            .as_deref()
            .unwrap()
            .contains("never deliver")
    );
    let approval = ApprovalAudit::from_json(row.approval.as_deref().ok_or("missing approval")?)?;
    pipeline::validate_recovered_projection(
        "",
        "current",
        "cancelled",
        &row.tool,
        &row.input_json,
        row.output_text.as_deref().unwrap(),
        RuntimeToolStatus::Cancelled,
        &approval,
        None,
        None,
    )?;
    Ok(())
}
