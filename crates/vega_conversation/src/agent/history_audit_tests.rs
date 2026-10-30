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
const THREAD: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

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
    for id in [THREAD, "other", "child"] {
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
            THREAD,
            1,
            "user",
            format!("甲乙SYNTHETIC-SECRET-0123456789{}", "界".repeat(1100)),
        ),
        ("old", THREAD, 2, "assistant", "older response".into()),
        ("run", THREAD, 3, "assistant", "current response".into()),
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
                thread_id: THREAD,
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
    store.conn().execute("INSERT INTO delegated_executions(child_message_id,child_thread_id,parent_thread_id,parent_message_id,request_call_id,ordinal,kind,name,task,state,result,truncated,created_at) VALUES('child-result','child','01ARZ3NDEKTSV4RRFFQ69G5FAV','old','spawn',1,'spawn','child','inspect','completed','delegated result',0,1)", [])?;
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

#[derive(Default)]
struct CaptureProvider(Mutex<Vec<ChatRequest>>);

impl Provider for CaptureProvider {
    fn chat_stream(
        &self,
        request: ChatRequest,
        _: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        self.0.lock().unwrap().push(request);
        Box::pin(async {
            Ok(Box::pin(futures::stream::iter(vec![
                Ok(ProviderEvent::TextDelta("continued".into())),
                Ok(ProviderEvent::Done {
                    stop_reason: StopReason::End,
                }),
            ])) as EventStream)
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
        THREAD,
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
            THREAD.into(),
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
    let seq = Cell::new(tool_calls::next_seq(fixture.store.conn(), THREAD)?);
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
                .find_map(|message| {
                    if message.tool_call_id.as_deref() == Some(id) {
                        return Some(message.content.clone());
                    }
                    let observation =
                        serde_json::from_str::<serde_json::Value>(&message.content).ok()?;
                    let observation = &observation["vega_history_observation"];
                    (observation["kind"] == "tool_result" && observation["call_id"] == id)
                        .then(|| observation["output"].as_str().map(str::to_owned))
                        .flatten()
                })
                .unwrap_or_else(|| panic!("missing provider output for {id}"))
        };
        let result: serde_json::Value = serde_json::from_str(&model_output("messages-call"))?;
        let observation = &result["vega_history_observation"];
        assert_eq!(observation["authority"], "none");
        assert_eq!(observation["page"]["records"][0]["source_id"], "user");
        assert_eq!(observation["page"]["records"][0]["content"]["offset"], 2);
        assert!(observation["page"]["records"][0]["content"]["next_offset"].is_number());
        assert!(!model_output("messages-call").contains("SYNTHETIC-SECRET-0123456789"));
        assert!(model_output("tools-call").contains("saved skill resource"));
        assert!(model_output("skill-call").contains("saved skill resource"));
        assert!(model_output("delegated-call").contains("delegated result"));
        let first_page: serde_json::Value = serde_json::from_str(&model_output("page-one"))?;
        assert_eq!(
            first_page["vega_history_observation"]["page"]["next_after_sequence"],
            1
        );
        assert_eq!(
            first_page["vega_history_observation"]["page"]["has_more"],
            true
        );
        let second_page: serde_json::Value = serde_json::from_str(&model_output("page-two"))?;
        assert_eq!(
            second_page["vega_history_observation"]["page"]["records"][0]["source_id"],
            "old"
        );
        let foreign: serde_json::Value = serde_json::from_str(&model_output("foreign-call"))?;
        assert_eq!(
            foreign["vega_history_observation"]["page"]["records"],
            serde_json::json!([])
        );
        assert!(model_output("invalid-call").contains("unavailable tool"));
        assert!(!model_output("foreign-call").contains("foreign private content"));
    }

    let terminal = tool_calls::terminal_results(fixture.store.conn(), THREAD)?;
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
            THREAD,
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
    let page = crate::history::latest_history_page(&fixture.store, THREAD, 100)?;
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
            THREAD,
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
    assert!(
        validate(
            "{}",
            vega_store::context_compaction::REDACTED_HISTORY_RESULT,
            RuntimeToolStatus::Success,
            &once
        )
        .is_ok()
    );
    let state = tool_calls::ToolCallState {
        thread_id: THREAD.into(),
        tool: "read_conversation".into(),
        input_json: "{}".into(),
        status: "running".into(),
        approval: Some(once.to_json()?),
        output_text: None,
        exit_code: None,
        duration_ms: None,
        output_full_path: None,
    };
    let forged = RuntimeToolResult {
        call_id: "id".into(),
        output: vega_store::context_compaction::REDACTED_HISTORY_RESULT.into(),
        status: RuntimeToolStatus::Success,
        reused: false,
        exit_code: None,
        duration_ms: None,
        truncated: Some(false),
        approval: Some(approval_audit_to_runtime(&once)),
        remember_rule: None,
    };
    assert!(events::validate_runtime_terminal("", THREAD, "id", &state, &forged, &once).is_err());
    Ok(())
}

#[test]
fn interrupted_history_calls_recover_without_pending_or_false_success() -> TestResult {
    let fixture = fixture()?;
    let seq = Cell::new(tool_calls::next_seq(fixture.store.conn(), THREAD)?);
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
    vega_store::recovery::recover_thread(fixture.store.conn(), THREAD, 42)?;
    let terminal = tool_calls::terminal_results(fixture.store.conn(), THREAD)?;
    for (id, expected, status) in [
        ("pending", "rejected", RuntimeToolStatus::Rejected),
        ("running", "cancelled", RuntimeToolStatus::Cancelled),
    ] {
        let row = &terminal[id];
        assert_eq!(row.status, expected);
        let approval = ApprovalAudit::from_json(&row.approval)?;
        pipeline::validate_recovered_projection(
            "",
            THREAD,
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
    let page = crate::history::latest_history_page(&fixture.store, THREAD, 100)?;
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
    let seq = Cell::new(tool_calls::next_seq(fixture.store.conn(), THREAD)?);
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
        THREAD,
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
        THREAD,
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
    let seq = Cell::new(tool_calls::next_seq(fixture.store.conn(), THREAD)?);
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
        THREAD,
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

#[tokio::test]
async fn history_metadata_can_be_redacted_without_losing_the_audit() -> TestResult {
    let fixture = fixture()?;
    let tools = vega_tools::Tools::new(&fixture.directory)?;
    let config = RuntimeToolConfig::default()
        .with_conversation_history_reader(context_history::reader(
            fixture.path.clone(),
            THREAD.into(),
        ))
        .with_credential_reader(Arc::new(|| {
            Ok(vec!["read_skill_resource".into(), "unknown".into()])
        }));
    let provider = scripted_provider(vec![
        (
            "resource-redacted",
            "read_conversation",
            input("skill_resources", Some("resource"), 0),
        ),
        (
            "delegated-redacted",
            "read_conversation",
            input("delegated_results", Some("spawn"), 0),
        ),
    ]);
    let seq = Cell::new(tool_calls::next_seq(fixture.store.conn(), THREAD)?);
    let outcome = run_agent_with_permission_sink(
        &provider,
        &tools,
        request(config, Default::default()),
        CancellationToken::new(),
        &RejectPermission,
        |event| futures::future::ready(persist(&fixture.store, &seq, &event)),
    )
    .await?;
    assert_eq!(outcome.final_text, "continued");
    for id in ["resource-redacted", "delegated-redacted"] {
        let row = tool_calls::find_state(fixture.store.conn(), id)?.ok_or("missing call")?;
        assert_eq!(row.status, "success");
        let output = row.output_text.ok_or("missing output")?;
        assert!(!output.contains("read_skill_resource"));
        assert!(!output.contains("unknown"));
        assert!(output.contains(vega_runtime::PROVIDER_CREDENTIAL_REDACTION_MARKER));
    }
    Ok(())
}

#[tokio::test]
async fn credential_change_during_history_read_returns_safe_recoverable_failure() -> TestResult {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    let fixture = fixture()?;
    let tools = vega_tools::Tools::new(&fixture.directory)?;
    let started = Arc::new(AtomicBool::new(false));
    let reads = Arc::new(AtomicUsize::new(0));
    let reader_started = started.clone();
    let credential_started = started.clone();
    let config = RuntimeToolConfig::default()
        .with_conversation_history_reader(Arc::new(move |_| {
            reader_started.store(true, Ordering::SeqCst);
            Ok(serde_json::json!({"records":[{"source_id":"user","sequence":1,"role":"user",
                "message_kind":"text","status":"done","content":"late synthetic secret",
                "attachments":{"count":0,"encoded_bytes":0}}],"next_after_sequence":1,"has_more":false}))
        }))
        .with_credential_reader(Arc::new(move || {
            if credential_started.load(Ordering::SeqCst) && reads.fetch_add(1, Ordering::SeqCst) > 0 {
                Ok(vec!["late synthetic secret".into()])
            } else {
                Ok(Vec::new())
            }
        }));
    let provider = scripted_provider(vec![(
        "changing",
        "read_conversation",
        input("messages", Some("user"), 0),
    )]);
    let seq = Cell::new(tool_calls::next_seq(fixture.store.conn(), THREAD)?);
    let outcome = run_agent_with_permission_sink(
        &provider,
        &tools,
        request(config, Default::default()),
        CancellationToken::new(),
        &RejectPermission,
        |event| futures::future::ready(persist(&fixture.store, &seq, &event)),
    )
    .await?;
    assert_eq!(outcome.final_text, "continued");
    let row = tool_calls::find_state(fixture.store.conn(), "changing")?.ok_or("missing call")?;
    assert_eq!(row.status, "failed");
    assert_eq!(
        row.output_text.as_deref(),
        Some(vega_runtime::CONVERSATION_HISTORY_RETRY_OUTPUT)
    );
    let requests = provider.requests.lock().unwrap();
    assert!(
        requests[1]
            .messages
            .iter()
            .all(|message| !message.content.contains("late synthetic secret"))
    );
    Ok(())
}

#[tokio::test]
async fn changed_credentials_invalidate_old_pages_and_same_id_reuse() -> TestResult {
    let fixture = fixture()?;
    let tools = vega_tools::Tools::new(&fixture.directory)?;
    let seq = Cell::new(tool_calls::next_seq(fixture.store.conn(), THREAD)?);
    let message_input = input("messages", Some("user"), 2);
    let initial = scripted_provider(vec![(
        "old-page",
        "read_conversation",
        message_input.clone(),
    )]);
    let initial_config = RuntimeToolConfig::default().with_conversation_history_reader(
        context_history::reader(fixture.path.clone(), THREAD.into()),
    );
    let outcome = run_agent_with_permission_sink(
        &initial,
        &tools,
        request(initial_config, Default::default()),
        CancellationToken::new(),
        &RejectPermission,
        |event| futures::future::ready(persist(&fixture.store, &seq, &event)),
    )
    .await?;
    assert_eq!(outcome.final_text, "continued");
    let terminal = tool_calls::terminal_results(fixture.store.conn(), THREAD)?;
    let old = &terminal["old-page"];
    assert!(old.output.contains("SYNTHETIC-SECRET-0123456789"));
    assert!(old.output.contains("next_offset"));
    let approval = ApprovalAudit::from_json(&old.approval)?;
    let completed = std::collections::HashMap::from([(
        "old-page".into(),
        CompletedToolCall {
            tool: old.tool.clone(),
            input_json: old.input_json.clone(),
            result: RuntimeToolResult {
                call_id: "old-page".into(),
                output: old.output.clone(),
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
    let retry = scripted_provider(vec![("old-page", "read_conversation", message_input)]);
    let outcome = run_agent_with_permission_sink(
        &retry,
        &tools,
        request(config(&fixture), completed),
        CancellationToken::new(),
        &RejectPermission,
        |event| futures::future::ready(persist(&fixture.store, &seq, &event)),
    )
    .await?;
    assert_eq!(outcome.final_text, "continued");
    assert_eq!(outcome.executed_tool_call_count, 0);
    assert!(outcome.events.iter().any(|event| matches!(event,
        RuntimeEvent::ToolCallFinished(result) if result.call_id == "old-page"
            && result.status == RuntimeToolStatus::Failed
            && result.output == vega_runtime::CONVERSATION_HISTORY_RETRY_OUTPUT)));
    {
        let requests = retry.requests.lock().unwrap();
        assert!(
            requests[1]
                .messages
                .iter()
                .all(|message| !message.content.contains("SYNTHETIC-SECRET-0123456789"))
        );
    }
    let state = tool_calls::find_state(fixture.store.conn(), "old-page")?.ok_or("missing call")?;
    let forged = RuntimeToolResult {
        call_id: "old-page".into(),
        output: "forged success".into(),
        status: RuntimeToolStatus::Success,
        reused: true,
        exit_code: None,
        duration_ms: None,
        truncated: None,
        approval: Some(approval_audit_to_runtime(&approval)),
        remember_rule: None,
    };
    assert!(events::validate_reused_terminal("", THREAD, &state, &forged).is_err());

    let prepared = pipeline::prepare_run_with_images_and_reasoning(
        fixture.path.clone(),
        THREAD.into(),
        "continue".into(),
        "system".into(),
        ulid::Ulid::generate().to_string(),
        ulid::Ulid::generate().to_string(),
        PersistenceActorConfig::default().with_owner_credential_reader(Arc::new(|| {
            Ok(vec!["SYNTHETIC-SECRET-0123456789".into()])
        })),
        false,
        None,
        None,
        Vec::new(),
        None,
    )?;
    let next = &prepared.request.completed_tool_results["old-page"]
        .result
        .output;
    assert_eq!(
        next,
        vega_store::context_compaction::REDACTED_HISTORY_RESULT
    );
    assert!(!next.contains("next_offset"));
    assert!(!next.contains("SYNTHETIC-SECRET-0123456789"));
    assert!(prepared.request.history.iter().all(|message| {
        message.role != ChatRole::Tool || !message.content.contains("SYNTHETIC-SECRET-0123456789")
    }));
    let assistant_id = prepared.assistant_message_id.clone();
    drop(prepared);
    messages::finish(fixture.store.conn(), &assistant_id, "", "done")?;
    let fingerprint =
        vega_store::credential_privacy::source_fingerprint(fixture.store.conn(), THREAD)?;
    let known = vec![
        "messages".to_string(),
        "SYNTHETIC-SECRET-0123456789".to_string(),
    ];
    vega_store::credential_privacy::clean(
        fixture.store.conn(),
        THREAD,
        &fingerprint,
        |text, offsets| {
            vega_runtime::redact_sensitive_credential_text_with_offsets(text, &known, offsets)
        },
    )?;
    let cleaned =
        tool_calls::find_state(fixture.store.conn(), "old-page")?.ok_or("missing call")?;
    assert_eq!(cleaned.input_json, "{}");
    assert_eq!(
        cleaned.output_text.as_deref(),
        Some(vega_store::context_compaction::REDACTED_HISTORY_RESULT)
    );
    let prepared_again = pipeline::prepare_run_with_images_and_reasoning(
        fixture.path.clone(),
        THREAD.into(),
        "continue again".into(),
        "system".into(),
        ulid::Ulid::generate().to_string(),
        ulid::Ulid::generate().to_string(),
        PersistenceActorConfig::default()
            .with_owner_credential_reader(Arc::new(move || Ok(known.clone()))),
        false,
        None,
        None,
        Vec::new(),
        None,
    )?;
    let recovered = &prepared_again.request.completed_tool_results["old-page"];
    assert_eq!(recovered.input_json, "{}");
    assert_eq!(
        recovered.result.output,
        vega_store::context_compaction::REDACTED_HISTORY_RESULT
    );
    Ok(())
}

#[tokio::test]
async fn source_identity_redacted_during_read_remains_a_safe_observation() -> TestResult {
    use std::sync::atomic::{AtomicBool, Ordering};
    let fixture = fixture()?;
    let tools = vega_tools::Tools::new(&fixture.directory)?;
    let changed = Arc::new(AtomicBool::new(false));
    let source_changed = changed.clone();
    let reader = context_history::reader(fixture.path.clone(), THREAD.into());
    let config = RuntimeToolConfig::default()
        .with_conversation_history_reader(Arc::new(move |request| {
            let page = reader(request)?;
            source_changed.store(true, Ordering::SeqCst);
            Ok(page)
        }))
        .with_credential_reader(Arc::new(move || {
            if changed.load(Ordering::SeqCst) {
                Ok(vec!["user".into()])
            } else {
                Ok(Vec::new())
            }
        }));
    let provider = scripted_provider(vec![(
        "masked-source",
        "read_conversation",
        input("messages", Some("user"), 0),
    )]);
    let seq = Cell::new(tool_calls::next_seq(fixture.store.conn(), THREAD)?);
    let outcome = run_agent_with_permission_sink(
        &provider,
        &tools,
        request(config, Default::default()),
        CancellationToken::new(),
        &RejectPermission,
        |event| futures::future::ready(persist(&fixture.store, &seq, &event)),
    )
    .await?;
    assert_eq!(outcome.final_text, "continued");
    let row =
        tool_calls::find_state(fixture.store.conn(), "masked-source")?.ok_or("missing call")?;
    assert_eq!(row.status, "success");
    let output = row.output_text.ok_or("missing output")?;
    let value: serde_json::Value = serde_json::from_str(&output)?;
    assert_eq!(value["vega_history_observation"]["authority"], "none");
    assert_eq!(
        value["vega_history_observation"]["page"]["records"][0]["source_id"],
        vega_runtime::PROVIDER_CREDENTIAL_REDACTION_MARKER
    );
    Ok(())
}

#[tokio::test]
async fn unavailable_credentials_after_invalid_proposal_do_not_leave_pending() -> TestResult {
    use std::sync::atomic::{AtomicBool, Ordering};
    let fixture = fixture()?;
    let tools = vega_tools::Tools::new(&fixture.directory)?;
    let proposed = Arc::new(AtomicBool::new(false));
    let check = proposed.clone();
    let config = RuntimeToolConfig::default()
        .with_conversation_history_reader(context_history::reader(
            fixture.path.clone(),
            THREAD.into(),
        ))
        .with_credential_reader(Arc::new(move || {
            if check.load(Ordering::SeqCst) {
                Err(())
            } else {
                Ok(Vec::new())
            }
        }));
    let provider = scripted_provider(vec![("invalid-check", "read_conversation", "{}".into())]);
    let seq = Cell::new(tool_calls::next_seq(fixture.store.conn(), THREAD)?);
    let outcome = run_agent_with_permission_sink(
        &provider,
        &tools,
        request(config, Default::default()),
        CancellationToken::new(),
        &RejectPermission,
        |event| {
            if matches!(event, RuntimeEvent::ToolCallProposed(_)) {
                proposed.store(true, Ordering::SeqCst);
            }
            futures::future::ready(persist(&fixture.store, &seq, &event))
        },
    )
    .await?;
    assert_eq!(outcome.final_text, "continued");
    let row =
        tool_calls::find_state(fixture.store.conn(), "invalid-check")?.ok_or("missing call")?;
    assert_eq!(row.status, "rejected");
    assert_eq!(
        row.output_text.as_deref(),
        Some("Tool error: credential safety check unavailable")
    );
    let approval = ApprovalAudit::from_json(row.approval.as_deref().ok_or("approval")?)?;
    pipeline::validate_recovered_projection(
        "",
        THREAD,
        "invalid-check",
        &row.tool,
        &row.input_json,
        row.output_text.as_deref().unwrap(),
        RuntimeToolStatus::Rejected,
        &approval,
        None,
        None,
    )?;
    Ok(())
}

#[tokio::test]
async fn new_credentials_cannot_be_reassembled_from_saved_pages() -> TestResult {
    let fixture = fixture()?;
    let tools = vega_tools::Tools::new(&fixture.directory)?;
    let secret = "separate-sensitive-value-123456789";
    let content = format!("{}{}end", "界".repeat(1018), secret);
    fixture
        .store
        .conn()
        .execute("UPDATE messages SET content=?1 WHERE id='user'", [&content])?;
    let provider = scripted_provider(vec![
        (
            "page-a",
            "read_conversation",
            input("messages", Some("user"), 0),
        ),
        (
            "page-b",
            "read_conversation",
            input("messages", Some("user"), 1024),
        ),
    ]);
    let seq = Cell::new(tool_calls::next_seq(fixture.store.conn(), THREAD)?);
    let initial = RuntimeToolConfig::default().with_conversation_history_reader(
        context_history::reader(fixture.path.clone(), THREAD.into()),
    );
    let mut initial_request = request(initial, Default::default());
    let mut frame = vega_runtime::ReplayFrame::new(1, vega_runtime::ReasoningReplayPolicy::Unknown);
    frame.attach_route_fingerprint([7; 32]);
    initial_request.replay_frame = Some(frame);
    let outcome = run_agent_with_permission_sink(
        &provider,
        &tools,
        initial_request,
        CancellationToken::new(),
        &RejectPermission,
        |event| futures::future::ready(persist(&fixture.store, &seq, &event)),
    )
    .await?;
    assert_eq!(outcome.final_text, "continued");
    let before = tool_calls::terminal_results(fixture.store.conn(), THREAD)?;
    let first = &before["page-a"].output;
    let second = &before["page-b"].output;
    assert!(!first.contains(secret));
    assert!(!second.contains(secret));
    assert!(first.contains(&secret[..6]));
    assert!(second.contains(&secret[6..]));
    let mut raw_request = provider.requests.lock().unwrap()[1].clone();
    let history_results = raw_request
        .messages
        .iter()
        .filter(|message| message.is_conversation_history_result())
        .collect::<Vec<_>>();
    assert_eq!(history_results.len(), 2);
    assert!(
        history_results
            .iter()
            .any(|message| message.content.contains(&secret[..6]))
    );
    assert!(
        history_results
            .iter()
            .any(|message| message.content.contains(&secret[6..]))
    );
    let ordinary_user_json = history_results[0].content.clone();
    let sequences = credential_guard::history_fragment_sequences(&raw_request.messages)?;
    assert!(sequences.iter().any(|sequence| sequence.contains(secret)));
    let mut one_stage = raw_request.clone();
    let mut seen = false;
    one_stage.messages.retain(|message| {
        if !message.is_conversation_history_result() {
            return true;
        }
        if seen {
            return false;
        }
        seen = true;
        true
    });
    one_stage.credential_scan_history = Some(sequences.into());
    let no_credentials = || Ok(Vec::new());
    let safe_stage = credential_guard::project_owner_secrets_with_policy(
        one_stage.clone(),
        &no_credentials,
        None,
    )?;
    assert!(
        safe_stage
            .messages
            .iter()
            .any(|message| message.is_conversation_history_result()
                && message.content.contains(&secret[..6]))
    );
    let credentials = || Ok(vec![secret.into()]);
    assert!(
        credential_guard::project_owner_secrets_with_policy(one_stage, &credentials, None).is_err()
    );
    raw_request
        .messages
        .push(ChatMessage::new(ChatRole::User, &ordinary_user_json));
    assert!(credential_guard::reject_owner_secret(&raw_request, &credentials).is_err());
    let projected =
        credential_guard::project_owner_secrets_with_policy(raw_request, &credentials, None)?;
    credential_guard::reject_owner_secret(&projected, &credentials)?;
    assert_eq!(
        projected.messages.last().ok_or("missing user")?.content,
        ordinary_user_json
    );
    let protected = projected
        .messages
        .iter()
        .filter(|message| message.is_conversation_history_result())
        .collect::<Vec<_>>();
    assert_eq!(protected.len(), 2);
    assert!(
        protected
            .iter()
            .all(|message| !message.content.contains(&secret[..6])
                && !message.content.contains(&secret[6..]))
    );

    let source = vega_store::context_compaction::load_source(fixture.store.conn(), THREAD)?;
    let registry = ReplaySessionRegistry::new();
    let fence = ReplayCheckpointFence::from_checkpoint(None);
    let lease = registry.begin(THREAD, source.source_version, &source.fingerprint, fence);
    assert!(registry.seal(
        lease,
        source.source_version,
        source.fingerprint.clone(),
        fence,
        "run".into(),
        outcome.messages.clone()
    ));
    let probe = registry.begin(THREAD, source.source_version, &source.fingerprint, fence);
    assert!(probe.candidate().is_some());
    assert!(registry.seal(
        probe,
        source.source_version,
        source.fingerprint,
        fence,
        "run".into(),
        outcome.messages.clone()
    ));

    let prepared = pipeline::prepare_run_with_images_and_reasoning(
        fixture.path.clone(),
        THREAD.into(),
        "continue".into(),
        "system".into(),
        ulid::Ulid::generate().to_string(),
        ulid::Ulid::generate().to_string(),
        PersistenceActorConfig::default()
            .with_owner_credential_reader(Arc::new(move || Ok(vec![secret.into()])))
            .with_replay_session_registry(registry, [7; 32]),
        false,
        None,
        None,
        Vec::new(),
        None,
    )?;
    assert!(
        prepared
            .replay_lease
            .as_ref()
            .is_some_and(|lease| lease.candidate().is_some())
    );
    for id in ["page-a", "page-b"] {
        let output = &prepared.request.completed_tool_results[id].result.output;
        assert_eq!(output, &before[id].output);
        assert!(output.contains("next_offset"));
    }
    let mut forwarded = prepared.request.clone();
    if let Some(frame) = forwarded.replay_frame.as_mut() {
        frame.attach_route_fingerprint([7; 32]);
        if let Some(session) = prepared
            .replay_lease
            .as_ref()
            .and_then(|lease| lease.candidate())
        {
            frame.attach_candidate(
                session.messages().to_vec(),
                session.environment_fingerprint(),
            );
        }
    }
    let capture = Arc::new(CaptureProvider::default());
    let guarded = credential_guard::OwnerCredentialProvider::new(
        capture.clone(),
        Arc::new(move || Ok(vec![secret.into()])),
    );
    let delivered = run_agent_with_permission_sink(
        &guarded,
        &tools,
        forwarded,
        CancellationToken::new(),
        &RejectPermission,
        |_| futures::future::ready(Ok(())),
    )
    .await?;
    assert_eq!(delivered.final_text, "continued");
    let sent = capture.0.lock().unwrap();
    let results = sent[0]
        .messages
        .iter()
        .filter(|message| message.is_conversation_history_result())
        .collect::<Vec<_>>();
    assert_eq!(results.len(), 2);
    assert!(
        results
            .iter()
            .all(|message| !message.content.contains(&secret[..6])
                && !message.content.contains(&secret[6..]))
    );
    for id in ["page-a", "page-b"] {
        let stored = tool_calls::find_state(fixture.store.conn(), id)?.ok_or("missing page")?;
        assert_eq!(
            stored.output_text.as_deref(),
            Some(before[id].output.as_str())
        );
    }
    Ok(())
}

#[tokio::test]
async fn safe_history_results_remain_available_to_the_compaction_provider() -> TestResult {
    let fixture = fixture()?;
    let tools = vega_tools::Tools::new(&fixture.directory)?;
    let anchor = "historical-recovery-anchor";
    let content = format!("{anchor}{}", " text".repeat(400));
    fixture
        .store
        .conn()
        .execute("UPDATE messages SET content=?1 WHERE id='user'", [&content])?;
    let history_provider = scripted_provider(vec![(
        "safe-page",
        "read_conversation",
        input("messages", Some("user"), 0),
    )]);
    let seq = Cell::new(tool_calls::next_seq(fixture.store.conn(), THREAD)?);
    let config = RuntimeToolConfig::default().with_conversation_history_reader(
        context_history::reader(fixture.path.clone(), THREAD.into()),
    );
    run_agent_with_permission_sink(
        &history_provider,
        &tools,
        request(config, Default::default()),
        CancellationToken::new(),
        &RejectPermission,
        |event| futures::future::ready(persist(&fixture.store, &seq, &event)),
    )
    .await?;
    let captured = CaptureProvider::default();
    compaction::compact_thread_manually_with_credential_reader(
        &fixture.store,
        &captured,
        THREAD,
        "synthetic",
        "system",
        Vec::new(),
        vega_runtime::ContextBudget::new(20_000, 1_000, false)?,
        CancellationToken::new(),
        None,
        None,
        Some(Arc::new(|| Ok(Vec::new()))),
    )
    .await
    .map_err(|failure| *failure.error)?;
    let requests = captured.0.lock().unwrap();
    let results = requests
        .iter()
        .flat_map(|request| &request.messages)
        .filter(|message| message.is_conversation_history_result())
        .collect::<Vec<_>>();
    assert!(!results.is_empty());
    assert!(
        results
            .iter()
            .any(|message| message.content.contains(anchor))
    );
    assert!(
        requests
            .iter()
            .all(|request| !format!("{request:?}").contains(anchor))
    );
    Ok(())
}

#[tokio::test]
async fn permanent_clean_invalidates_split_pages_and_recovers_cleaned_running_call() -> TestResult {
    let fixture = fixture()?;
    let tools = vega_tools::Tools::new(&fixture.directory)?;
    let secret = "separate-sensitive-value-123456789";
    let content = format!("{}{}end", "界".repeat(1018), secret);
    fixture
        .store
        .conn()
        .execute("UPDATE messages SET content=?1 WHERE id='user'", [&content])?;
    let provider = scripted_provider(vec![
        (
            "clean-a",
            "read_conversation",
            input("messages", Some("user"), 0),
        ),
        (
            "clean-b",
            "read_conversation",
            input("messages", Some("user"), 1024),
        ),
    ]);
    let seq = Cell::new(tool_calls::next_seq(fixture.store.conn(), THREAD)?);
    let config = RuntimeToolConfig::default().with_conversation_history_reader(
        context_history::reader(fixture.path.clone(), THREAD.into()),
    );
    let outcome = run_agent_with_permission_sink(
        &provider,
        &tools,
        request(config, Default::default()),
        CancellationToken::new(),
        &RejectPermission,
        |event| futures::future::ready(persist(&fixture.store, &seq, &event)),
    )
    .await?;
    assert_eq!(outcome.final_text, "continued");
    let before = tool_calls::terminal_results(fixture.store.conn(), THREAD)?;
    assert!(!before["clean-a"].output.contains(secret));
    assert!(!before["clean-b"].output.contains(secret));
    assert!(before["clean-a"].output.contains(&secret[..6]));
    assert!(before["clean-b"].output.contains(&secret[6..]));
    let fingerprint =
        vega_store::credential_privacy::source_fingerprint(fixture.store.conn(), THREAD)?;
    vega_store::credential_privacy::clean(
        fixture.store.conn(),
        THREAD,
        &fingerprint,
        |text, offsets| {
            vega_runtime::redact_sensitive_credential_text_with_offsets(
                text,
                &[secret.into()],
                offsets,
            )
        },
    )?;
    for id in ["clean-a", "clean-b"] {
        let row = tool_calls::find_state(fixture.store.conn(), id)?.ok_or("missing call")?;
        assert_eq!(row.status, "success");
        assert_eq!(
            row.output_text.as_deref(),
            Some(vega_store::context_compaction::REDACTED_HISTORY_RESULT)
        );
    }
    let page = crate::history::latest_history_page(&fixture.store, THREAD, 100)?;
    for id in ["clean-a", "clean-b"] {
        assert!(page.entries.iter().any(|entry| matches!(entry,
            crate::history::HistoryEntry::Tool { call_id, result: Some(crate::types::ToolCardResultProjection::ReadOnly { output, .. }), .. }
            if call_id == id && output == vega_store::context_compaction::REDACTED_HISTORY_RESULT)));
    }
    let audit = RuntimeApprovalAudit {
        decision: RuntimeApprovalDecision::Once,
        note: None,
        source: RuntimeApprovalSource::ReadonlyTool,
        danger: None,
    };
    let call = RuntimeToolCall {
        id: "clean-running".into(),
        name: "read_conversation".into(),
        input_json: input("messages", None, 0),
        description: None,
    };
    persist(&fixture.store, &seq, &RuntimeEvent::ToolCallProposed(call))?;
    persist(
        &fixture.store,
        &seq,
        &RuntimeEvent::ToolCallApproved {
            call_id: "clean-running".into(),
            audit,
            remember_rule: None,
        },
    )?;
    persist(
        &fixture.store,
        &seq,
        &RuntimeEvent::ToolCallRunning {
            call_id: "clean-running".into(),
        },
    )?;
    let fingerprint =
        vega_store::credential_privacy::source_fingerprint(fixture.store.conn(), THREAD)?;
    vega_store::credential_privacy::clean(
        fixture.store.conn(),
        THREAD,
        &fingerprint,
        |text, offsets| {
            vega_runtime::redact_sensitive_credential_text_with_offsets(
                text,
                &["messages".into()],
                offsets,
            )
        },
    )?;
    let cleaned = tool_calls::find_state(fixture.store.conn(), "clean-running")?
        .ok_or("missing running call")?;
    assert_eq!(cleaned.input_json, "{}");
    vega_store::recovery::recover_thread(fixture.store.conn(), THREAD, 42)?;
    let recovered = tool_calls::find_state(fixture.store.conn(), "clean-running")?
        .ok_or("missing recovered call")?;
    assert_eq!(recovered.status, "cancelled");
    let approval =
        ApprovalAudit::from_json(recovered.approval.as_deref().ok_or("missing approval")?)?;
    pipeline::validate_recovered_projection(
        "",
        THREAD,
        "clean-running",
        &recovered.tool,
        &recovered.input_json,
        recovered.output_text.as_deref().unwrap(),
        RuntimeToolStatus::Cancelled,
        &approval,
        None,
        None,
    )?;
    Ok(())
}
