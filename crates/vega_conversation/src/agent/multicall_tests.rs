use super::*;
use serde_json::{Value, json};
use std::collections::HashMap;
use vega_runtime::{
    ChatMessage, ChatRequest, ChatRole, CompletedToolCall, EventStream, ProviderEvent, StopReason,
};

const THREAD: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const MESSAGE: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAW";

type Step = Box<dyn FnOnce(ChatRequest) -> Vec<ProviderEvent> + Send>;

struct ScriptedProvider {
    steps: Mutex<VecDeque<Step>>,
}

impl Provider for ScriptedProvider {
    fn chat_stream(
        &self,
        request: ChatRequest,
        _: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        let step = self
            .steps
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected extra provider request");
        let events = step(request);
        Box::pin(async move {
            Ok(Box::pin(futures::stream::iter(events.into_iter().map(Ok))) as EventStream)
        })
    }
}

impl ScriptedProvider {
    fn new(steps: Vec<Step>) -> Self {
        Self {
            steps: Mutex::new(steps.into()),
        }
    }

    fn assert_complete(&self) {
        assert!(self.steps.lock().unwrap().is_empty());
    }
}

struct Fixture {
    directory: PathBuf,
    root: PathBuf,
    project: String,
    store: Arc<Mutex<Store>>,
    tools: vega_tools::Tools,
}

impl Fixture {
    fn new() -> Self {
        let directory =
            std::env::temp_dir().join(format!("vega-multicall-{}", ulid::Ulid::generate()));
        fs::create_dir(&directory).unwrap();
        let root = directory.join("workspace");
        fs::create_dir(&root).unwrap();
        fs::create_dir(directory.join("checkpoints")).unwrap();
        fs::write(root.join("a.txt"), "alpha\nsecond\n").unwrap();
        fs::write(root.join("b.txt"), "beta\n").unwrap();
        let store = Store::open(":memory:").unwrap();
        store.migrate().unwrap();
        let project =
            vega_store::projects::create(store.conn(), root.to_str().unwrap(), "fixture", None)
                .unwrap()
                .id;
        vega_store::threads::create(
            store.conn(),
            vega_store::threads::NewThread {
                reasoning_preferences: None,
                id: THREAD,
                project_id: &project,
                title: "test",
                mode: "execute",
                permission_mode: "full_access",
                permission_revision: 0,
                full_access_grant: None,
                model: "scripted",
                status: "active",
                pinned: false,
                unread: false,
                created_at: 1,
                updated_at: 1,
            },
        )
        .unwrap();
        messages::insert(
            store.conn(),
            &messages::MessageRow {
                id: MESSAGE.into(),
                thread_id: THREAD.into(),
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
        let tools = vega_tools::Tools::new(&root).unwrap();
        Self {
            directory,
            root,
            project,
            store: Arc::new(Mutex::new(store)),
            tools,
        }
    }

    fn request(&self, mode: RuntimePermissionMode) -> AgentRequest {
        let mut tool_config = RuntimeToolConfig::new(
            RuntimeRunMode::Execute,
            mode,
            self.project.clone(),
            THREAD.into(),
            self.directory.join("checkpoints"),
            Vec::new(),
        );
        tool_config.turn_limit = 5;
        AgentRequest {
            history_safety: None,
            model: "scripted".into(),
            system_prompt: "Use the provided tools.".into(),
            history: vec![ChatMessage::new(ChatRole::User, "Process the fixture.")],
            max_tokens: None,
            completed_tool_results: HashMap::new(),
            tool_config,
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
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.directory).unwrap();
    }
}

fn call(id: &str, name: &str, mut input: Value) -> ProviderEvent {
    input["description"] = json!("Process fixture data");
    ProviderEvent::ToolUse {
        id: id.into(),
        name: name.into(),
        input_json: input.to_string(),
    }
}

fn batch(calls: Vec<ProviderEvent>) -> Vec<ProviderEvent> {
    let mut events = calls;
    events.push(ProviderEvent::Done {
        stop_reason: StopReason::ToolUse,
    });
    events
}

fn finish() -> Vec<ProviderEvent> {
    vec![
        ProviderEvent::TextDelta("done".into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ]
}

fn results(events: &[RuntimeEvent]) -> Vec<vega_runtime::RuntimeToolResult> {
    events
        .iter()
        .filter_map(|event| match event {
            RuntimeEvent::ToolCallFinished(result)
            | RuntimeEvent::ToolReadFinished { result, .. }
            | RuntimeEvent::ToolCallValidationRejected { result, .. } => Some(result.clone()),
            _ => None,
        })
        .collect()
}

fn assert_next_request(request: &ChatRequest, expected: &[(&str, &str)]) {
    let assistant = request
        .messages
        .iter()
        .find(|message| !message.tool_calls.is_empty())
        .unwrap();
    assert_eq!(assistant.tool_calls.len(), expected.len());
    for (call, (id, name)) in assistant.tool_calls.iter().zip(expected) {
        assert_eq!((&*call.id, &*call.name), (*id, *name));
    }
    let tool_results: Vec<_> = request
        .messages
        .iter()
        .filter(|message| message.role == ChatRole::Tool)
        .collect();
    assert_eq!(tool_results.len(), expected.len());
    for (message, (id, _)) in tool_results.iter().zip(expected) {
        assert_eq!(message.tool_call_id.as_deref(), Some(*id));
        assert!(!message.content.is_empty());
    }
}

#[tokio::test]
async fn complete_batch_is_persisted_before_the_next_provider_request() {
    let fixture = Fixture::new();
    let store = fixture.store.clone();
    let provider = ScriptedProvider::new(vec![
        Box::new(|_| {
            batch(vec![
                call(
                    "first",
                    "read",
                    json!({"path":"a.txt","offset":1,"limit":1}),
                ),
                call(
                    "second",
                    "read",
                    json!({"path":"b.txt","offset":null,"limit":null}),
                ),
                call("third", "glob", json!({"pattern":"*.txt"})),
                call("fourth", "grep", json!({"pattern":"alpha","path":"a.txt"})),
            ])
        }),
        Box::new(move |request| {
            assert_next_request(
                &request,
                &[
                    ("first", "read"),
                    ("second", "read"),
                    ("third", "glob"),
                    ("fourth", "grep"),
                ],
            );
            let assistant = request
                .messages
                .iter()
                .find(|message| !message.tool_calls.is_empty())
                .unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(&assistant.tool_calls[0].input_json).unwrap()["path"],
                "a.txt"
            );
            assert_eq!(
                serde_json::from_str::<Value>(&assistant.tool_calls[1].input_json).unwrap()["path"],
                "b.txt"
            );
            let store = store.lock().unwrap();
            for message in request
                .messages
                .iter()
                .filter(|message| message.role == ChatRole::Tool)
            {
                let id = message.tool_call_id.as_deref().unwrap();
                let (status, output): (String, String) = store
                    .conn()
                    .query_row(
                        "SELECT status,output_text FROM tool_calls WHERE id=?1",
                        [id],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .unwrap();
                assert_eq!(status, "success");
                assert_eq!(output, message.content);
            }
            let (calls, rounds): (i64,i64) = store.conn().query_row("SELECT COUNT(*),COUNT(DISTINCT response_id) FROM tool_call_rounds WHERE message_id=?1", [MESSAGE], |row| Ok((row.get(0)?,row.get(1)?))).unwrap();
            assert_eq!((calls, rounds), (4, 1));
            finish()
        }),
    ]);
    let mut seq = 1;
    let outcome = vega_runtime::run_agent_with_permission_sink(
        &provider,
        &fixture.tools,
        fixture.request(RuntimePermissionMode::FullAccess),
        CancellationToken::new(),
        &RejectHook,
        |event| {
            match &event {
                RuntimeEvent::ToolCallProposed(_)
                | RuntimeEvent::ToolCallApproved { .. }
                | RuntimeEvent::ToolCallRunning { .. }
                | RuntimeEvent::ToolCallFinished(_)
                | RuntimeEvent::ToolReadFinished { .. }
                | RuntimeEvent::ToolRoundCompleted { .. } => {
                    persist_runtime_event(
                        &fixture.store.lock().unwrap(),
                        &fixture.project,
                        THREAD,
                        MESSAGE,
                        "scripted",
                        false,
                        "",
                        &mut seq,
                        &event,
                        None,
                        None,
                    )
                    .unwrap();
                }
                _ => {}
            }
            async { Ok(()) }
        },
    )
    .await
    .unwrap();
    assert!(!outcome.failed);
    assert_eq!(outcome.executed_tool_call_count, 4);
    assert_eq!(outcome.final_text, "done");
    provider.assert_complete();
}

struct RejectHook;

impl RuntimePermissionHook for RejectHook {
    fn request(
        &self,
        _: vega_runtime::RuntimePermissionPrompt,
        _: CancellationToken,
    ) -> BoxFuture<'static, Result<RuntimeUserDecision, VegaError>> {
        Box::pin(async { Ok(RuntimeUserDecision::Deny { note: None }) })
    }
}

#[tokio::test]
async fn failure_and_permission_denial_do_not_lose_neighboring_results() {
    let fixture = Fixture::new();
    let provider = ScriptedProvider::new(vec![
        Box::new(|_| {
            batch(vec![
                call(
                    "good",
                    "read",
                    json!({"path":"a.txt","offset":null,"limit":null}),
                ),
                call(
                    "missing",
                    "read",
                    json!({"path":"missing.txt","offset":null,"limit":null}),
                ),
                call(
                    "denied",
                    "write",
                    json!({"path":"denied.txt","content":"must not exist"}),
                ),
                call(
                    "after",
                    "read",
                    json!({"path":"b.txt","offset":null,"limit":null}),
                ),
            ])
        }),
        Box::new(|request| {
            assert_next_request(
                &request,
                &[
                    ("good", "read"),
                    ("missing", "read"),
                    ("denied", "write"),
                    ("after", "read"),
                ],
            );
            finish()
        }),
    ]);
    let outcome = vega_runtime::run_agent_with_permission_sink(
        &provider,
        &fixture.tools,
        fixture.request(RuntimePermissionMode::Confirm),
        CancellationToken::new(),
        &RejectHook,
        |_| async { Ok(()) },
    )
    .await
    .unwrap();
    let statuses: Vec<_> = results(&outcome.events)
        .into_iter()
        .map(|result| (result.call_id, result.status))
        .collect();
    assert_eq!(
        statuses,
        vec![
            ("good".into(), RuntimeToolStatus::Success),
            ("missing".into(), RuntimeToolStatus::Failed),
            ("denied".into(), RuntimeToolStatus::Rejected),
            ("after".into(), RuntimeToolStatus::Success)
        ]
    );
    assert!(!fixture.root.join("denied.txt").exists());
    assert!(!outcome.failed);
    provider.assert_complete();
}

#[tokio::test]
async fn completed_write_and_read_results_are_reused_without_repeating_side_effects() {
    let fixture = Fixture::new();
    let calls = vec![
        call(
            "written",
            "write",
            json!({"path":"result.txt","content":"original"}),
        ),
        call(
            "read",
            "read",
            json!({"path":"b.txt","offset":null,"limit":null}),
        ),
    ];
    let first_calls = calls.clone();
    let provider = ScriptedProvider::new(vec![
        Box::new(move |_| batch(first_calls)),
        Box::new(|_| finish()),
    ]);
    let first = vega_runtime::run_agent(
        &provider,
        &fixture.tools,
        fixture.request(RuntimePermissionMode::FullAccess),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(!first.failed);
    assert_eq!(
        fs::read_to_string(fixture.root.join("result.txt")).unwrap(),
        "original"
    );
    let completed: HashMap<_, _> = results(&first.events)
        .into_iter()
        .map(|result| {
            assert_eq!(result.status, RuntimeToolStatus::Success);
            let proposed = first
                .events
                .iter()
                .find_map(|event| match event {
                    RuntimeEvent::ToolCallProposed(call) if call.id == result.call_id => Some(call),
                    _ => None,
                })
                .unwrap();
            (
                result.call_id.clone(),
                CompletedToolCall {
                    tool: proposed.name.clone(),
                    input_json: proposed.input_json.clone(),
                    result,
                },
            )
        })
        .collect();
    fs::write(fixture.root.join("result.txt"), "user changed").unwrap();
    fs::remove_file(fixture.root.join("b.txt")).unwrap();
    let provider = ScriptedProvider::new(vec![
        Box::new(move |_| batch(calls)),
        Box::new(|request| {
            assert_next_request(&request, &[("written", "write"), ("read", "read")]);
            finish()
        }),
    ]);
    let mut request = fixture.request(RuntimePermissionMode::FullAccess);
    request.completed_tool_results = completed;
    let recovered =
        vega_runtime::run_agent(&provider, &fixture.tools, request, CancellationToken::new())
            .await
            .unwrap();
    assert!(!recovered.failed);
    assert_eq!(recovered.executed_tool_call_count, 0);
    let results = results(&recovered.events);
    assert_eq!(results.len(), 2);
    assert!(
        results
            .iter()
            .all(|result| result.reused && result.status == RuntimeToolStatus::Success)
    );
    assert_eq!(
        fs::read_to_string(fixture.root.join("result.txt")).unwrap(),
        "user changed"
    );
    provider.assert_complete();
}

#[tokio::test]
async fn cancellation_after_first_terminal_preserves_it_and_stops_remaining_calls() {
    let fixture = Fixture::new();
    let provider = ScriptedProvider::new(vec![Box::new(|_| {
        batch(vec![
            call(
                "first",
                "read",
                json!({"path":"a.txt","offset":null,"limit":null}),
            ),
            call(
                "unstarted",
                "write",
                json!({"path":"never.txt","content":"no"}),
            ),
        ])
    })]);
    let cancel = CancellationToken::new();
    let outcome = vega_runtime::run_agent_with_permission_sink(
        &provider,
        &fixture.tools,
        fixture.request(RuntimePermissionMode::FullAccess),
        cancel.clone(),
        &RejectHook,
        |event| {
            if matches!(
                event,
                RuntimeEvent::ToolReadFinished { .. } | RuntimeEvent::ToolCallFinished(_)
            ) {
                cancel.cancel();
            }
            async { Ok(()) }
        },
    )
    .await
    .unwrap();
    assert!(outcome.interrupted);
    let results = results(&outcome.events);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].call_id, "first");
    assert_eq!(results[0].status, RuntimeToolStatus::Success);
    assert!(!fixture.root.join("never.txt").exists());
    assert_eq!(outcome.executed_tool_call_count, 1);
    provider.assert_complete();
}

#[tokio::test]
async fn a_changed_read_identity_rejects_edits_without_overwriting_new_bytes() {
    let fixture = Fixture::new();
    let path = fixture.root.join("a.txt");
    let provider = ScriptedProvider::new(vec![
        Box::new(|_| {
            batch(vec![call(
                "read",
                "read",
                json!({"path":"a.txt","offset":null,"limit":null}),
            )])
        }),
        Box::new(move |_| {
            fs::write(&path, "external change\n").unwrap();
            batch(vec![
                call(
                    "edit",
                    "edit",
                    json!({"path":"a.txt","edits":[{"oldText":"alpha","newText":"modified"}]}),
                ),
                call(
                    "neighbor",
                    "read",
                    json!({"path":"b.txt","offset":null,"limit":null}),
                ),
            ])
        }),
        Box::new(|_| finish()),
    ]);
    let outcome = vega_runtime::run_agent(
        &provider,
        &fixture.tools,
        fixture.request(RuntimePermissionMode::FullAccess),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let results = results(&outcome.events);
    assert_eq!(results.len(), 3);
    assert_eq!(results[1].status, RuntimeToolStatus::Rejected);
    assert!(results[1].output.contains("target_changed"));
    assert_eq!(results[2].status, RuntimeToolStatus::Success);
    assert_eq!(
        fs::read_to_string(fixture.root.join("a.txt")).unwrap(),
        "external change\n"
    );
    provider.assert_complete();
}

struct QuestionHook(std::sync::atomic::AtomicUsize);

impl vega_runtime::RuntimeUserQuestionHook for QuestionHook {
    fn request(
        &self,
        _: vega_runtime::RuntimeUserQuestionRequest,
        _: CancellationToken,
    ) -> BoxFuture<'static, vega_runtime::RuntimeUserQuestionResponse> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Box::pin(async { vega_runtime::RuntimeUserQuestionResponse::Unavailable })
    }
}

#[tokio::test]
async fn questions_cannot_share_a_batch_with_ordinary_tools() {
    let fixture = Fixture::new();
    let question = json!({"questions":[{"id":"choice","header":"Choose","question":"Choose a value","options":[{"label":"A","description":"First"},{"label":"B","description":"Second"}]}]});
    let hook = Arc::new(QuestionHook(std::sync::atomic::AtomicUsize::new(0)));
    let provider = ScriptedProvider::new(vec![
        Box::new(move |_| {
            batch(vec![
                call("question", "ask_user_question", question),
                call("write", "write", json!({"path":"never.txt","content":"no"})),
            ])
        }),
        Box::new(|_| finish()),
    ]);
    let mut request = fixture.request(RuntimePermissionMode::FullAccess);
    request.tool_config = request.tool_config.with_user_questions(Some(hook.clone()));
    let outcome =
        vega_runtime::run_agent(&provider, &fixture.tools, request, CancellationToken::new())
            .await
            .unwrap();
    assert_eq!(hook.0.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(outcome.executed_tool_call_count, 0);
    assert!(!fixture.root.join("never.txt").exists());
    let results = results(&outcome.events);
    assert_eq!(results.len(), 2);
    assert!(
        results
            .iter()
            .all(|result| result.status == RuntimeToolStatus::Rejected)
    );
    provider.assert_complete();
}

#[tokio::test]
async fn reset_admission_pause_and_cancellation_prevent_remaining_batch_mutations() {
    let fixture = Fixture::new();
    let database = fixture.store.lock().unwrap().database_identity();
    let owner = vega_store::thread_admission::try_acquire(database, THREAD)
        .unwrap()
        .unwrap();
    let provider = ScriptedProvider::new(vec![Box::new(|_| {
        batch(vec![
            call(
                "first",
                "read",
                json!({"path":"a.txt","offset":null,"limit":null}),
            ),
            call(
                "unstarted",
                "write",
                json!({"path":"never.txt","content":"no"}),
            ),
        ])
    })]);
    let cancel = CancellationToken::new();
    let mut pause = None;
    let outcome = vega_runtime::run_agent_with_permission_sink(
        &provider,
        &fixture.tools,
        fixture.request(RuntimePermissionMode::FullAccess),
        cancel.clone(),
        &RejectHook,
        |event| {
            if matches!(event, RuntimeEvent::ToolReadFinished { .. }) {
                let guard =
                    crate::permission_reset::pause_admission(&fixture.store.lock().unwrap())
                        .unwrap();
                assert!(!guard.owners_released().unwrap());
                assert!(
                    vega_store::thread_admission::try_acquire(database, "new-thread")
                        .unwrap()
                        .is_none()
                );
                pause = Some(guard);
                cancel.cancel();
            }
            async { Ok(()) }
        },
    )
    .await
    .unwrap();
    assert!(outcome.interrupted);
    assert_eq!(outcome.executed_tool_call_count, 1);
    assert!(!fixture.root.join("never.txt").exists());
    drop(owner);
    assert!(pause.as_ref().unwrap().owners_released().unwrap());
    drop(pause);
    assert!(
        vega_store::thread_admission::try_acquire(database, THREAD)
            .unwrap()
            .is_some()
    );
    provider.assert_complete();
}

#[test]
fn skill_activation_keeps_its_batch_barrier() {
    use vega_runtime::skills::{BatchPolicy, classify_tool_batch};
    assert_eq!(
        classify_tool_batch(&["read", "glob"]),
        BatchPolicy::Ordinary
    );
    assert_eq!(
        classify_tool_batch(&["load_skill", "load_skill"]),
        BatchPolicy::SkillOnly
    );
    assert_eq!(
        classify_tool_batch(&["load_skill", "write"]),
        BatchPolicy::RejectOtherTools
    );
}
