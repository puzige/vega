use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use futures::future::BoxFuture;
use tokio_util::sync::CancellationToken;
use vega_runtime::{
    ChatRequest, ChatRole, EventStream, Provider, ProviderEvent, StopReason, VegaError,
};
use vega_store::{Store, projects, recovery, threads, tool_calls};
use vega_tools::Tools;

use super::{ConversationRun, PermissionHook, run_thread_task_with_permission_sink};
use crate::history::{self, HistoryEntry};
use crate::types::{
    Approval, ApprovalAudit, ApprovalSource, ConversationEvent, PermissionDecision,
    PermissionRequest, ToolCallStatus, ToolCardResultProjection,
};

const INVALID_READ_ERROR: &str = r#"Tool error: read requires path identifying a regular text file. Expected {"path":"src/lib.rs","offset":null,"limit":null}"#;
const INVALID_CALL_ID: &str = "invalid-read";
const VALID_CALL_ID: &str = "valid-read";
const READ_OUTPUT: &str = "1 | owned read fixture";
const MISSING_PATH_INPUT: &str = r#"{"offset":null,"limit":null}"#;
const VALID_READ_INPUT: &str = r#"{"path":"fixture.txt","offset":null,"limit":null}"#;
const SENSITIVE_MARKER: &str = "owned-sensitive-read-marker";
const SENSITIVE_FIELD: &str = "sensitive_read_payload";
const SENSITIVE_FILE_NAME: &str = "owned-sensitive-read-path.txt";
const PROTECTED_PATH: &str = ".git/owned-sensitive-read-path.txt";
const INVALID_READ_INPUTS: [(&str, &str); 10] = [
    ("missing_path", MISSING_PATH_INPUT),
    (
        "non_string_path",
        r#"{"path":7,"offset":null,"limit":null}"#,
    ),
    (
        "malformed_json",
        r#"{"path":"owned-sensitive-read-path.txt","offset":null,"limit":null"#,
    ),
    ("non_object", r#"["owned-sensitive-read-marker"]"#),
    (
        "zero_offset",
        r#"{"path":"fixture.txt","offset":0,"limit":null}"#,
    ),
    (
        "negative_offset",
        r#"{"path":"fixture.txt","offset":-1,"limit":null}"#,
    ),
    (
        "string_offset",
        r#"{"path":"fixture.txt","offset":"1","limit":null}"#,
    ),
    (
        "non_integer_limit",
        r#"{"path":"fixture.txt","offset":null,"limit":1.5}"#,
    ),
    (
        "both_paths",
        r#"{"path":"fixture.txt","file_path":"owned-sensitive-read-path.txt","offset":null,"limit":null}"#,
    ),
    (
        "unsupported_sensitive_field",
        r#"{"path":"fixture.txt","offset":null,"limit":null,"sensitive_read_payload":"owned-sensitive-read-marker"}"#,
    ),
];

struct OwnedTempDir {
    path: PathBuf,
}

impl OwnedTempDir {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("vega-read-validation-{}", ulid::Ulid::generate()));
        fs::create_dir(&path).expect("create owned fixture directory");
        Self { path }
    }
}

impl Drop for OwnedTempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

struct ReadFixture {
    store: Store,
    tools: Tools,
    thread_id: String,
    root: PathBuf,
    database_path: PathBuf,
    _directory: OwnedTempDir,
}

impl ReadFixture {
    fn new() -> Self {
        let directory = OwnedTempDir::new();
        let root = directory.path.join("project");
        fs::create_dir(&root).expect("create fixture project");
        let root = root.canonicalize().expect("canonical fixture project");
        fs::write(root.join("fixture.txt"), "owned read fixture\n")
            .expect("write owned read fixture");
        fs::create_dir(root.join(".git")).expect("create owned protected directory");
        fs::write(root.join(PROTECTED_PATH), SENSITIVE_MARKER).expect("write owned protected file");
        let database_path = directory.path.join("state.sqlite");
        let store = Store::open(&database_path).expect("open fixture store");
        store.migrate().expect("migrate fixture store");
        let project = projects::create(
            store.conn(),
            root.to_str().expect("UTF-8 fixture project path"),
            "fixture",
            None,
        )
        .expect("create fixture project record");
        let thread_id = ulid::Ulid::generate().to_string();
        threads::create(
            store.conn(),
            threads::NewThread {
                id: &thread_id,
                project_id: &project.id,
                title: "fixture",
                mode: "execute",
                permission_mode: "full_access",
                model: "test",
                status: "active",
                pinned: false,
                unread: false,
                created_at: 1,
                updated_at: 1,
            },
        )
        .expect("create fixture thread");
        let tools = Tools::new(root.clone()).expect("create real fixture tools");
        Self {
            store,
            tools,
            thread_id,
            root,
            database_path,
            _directory: directory,
        }
    }

    fn reopen(self) -> Self {
        let Self {
            store,
            tools,
            thread_id,
            root,
            database_path,
            _directory,
        } = self;
        drop(store);
        let store = Store::open(&database_path).expect("reopen the same fixture database");
        store.migrate().expect("migrate reopened fixture store");
        Self {
            store,
            tools,
            thread_id,
            root,
            database_path,
            _directory,
        }
    }
}

struct ScriptedProvider {
    script: Arc<Mutex<VecDeque<Vec<ProviderEvent>>>>,
    requests: Arc<Mutex<Vec<ChatRequest>>>,
}

impl ScriptedProvider {
    fn new(tool_name: &str, invalid_input: &str) -> Self {
        Self::from_turns(vec![
            read_turn(tool_name, INVALID_CALL_ID, invalid_input),
            read_turn(tool_name, VALID_CALL_ID, VALID_READ_INPUT),
            answer_turn(),
        ])
    }

    fn from_turns(turns: Vec<Vec<ProviderEvent>>) -> Self {
        Self {
            script: Arc::new(Mutex::new(VecDeque::from(turns))),
            requests: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

fn read_turn(tool_name: &str, call_id: &str, input: &str) -> Vec<ProviderEvent> {
    vec![
        ProviderEvent::ToolUse {
            id: call_id.into(),
            name: tool_name.into(),
            input_json: input.into(),
        },
        ProviderEvent::Done {
            stop_reason: StopReason::ToolUse,
        },
    ]
}

fn answer_turn() -> Vec<ProviderEvent> {
    vec![
        ProviderEvent::TextDelta("Done".into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ]
}

impl Provider for ScriptedProvider {
    fn chat_stream(
        &self,
        req: ChatRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        self.requests
            .lock()
            .expect("capture provider request")
            .push(req);
        let events = self
            .script
            .lock()
            .expect("lock provider script")
            .pop_front()
            .expect("unexpected provider request beyond read recovery script");
        Box::pin(async move {
            let stream: EventStream = Box::pin(futures::stream::iter(events.into_iter().map(Ok)));
            Ok(stream)
        })
    }
}

#[derive(Default)]
struct RecordingPermissionHook {
    requests: Arc<Mutex<Vec<PermissionRequest>>>,
}

impl PermissionHook for RecordingPermissionHook {
    fn request(
        &self,
        request: PermissionRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<PermissionDecision, VegaError>> {
        self.requests
            .lock()
            .expect("record permission request")
            .push(request);
        Box::pin(async { Ok(PermissionDecision::Deny { note: None }) })
    }
}

async fn run_script(
    fixture: &ReadFixture,
    provider: &ScriptedProvider,
    permission_hook: &RecordingPermissionHook,
    context: &str,
) -> ConversationRun {
    let run = run_thread_task_with_permission_sink(
        &fixture.store,
        provider,
        &fixture.tools,
        &fixture.thread_id,
        "Read the fixture and answer Done.",
        "Use the read tool to inspect the fixture.",
        CancellationToken::new(),
        permission_hook,
        |_| Ok(()),
    )
    .await
    .unwrap_or_else(|error| {
        panic!(
            "{context}: invalid read must not abort the conversation persistence chain: {error:?}"
        )
    });
    assert!(!run.failed, "{context}: {run:?}");
    assert!(!run.interrupted, "{context}: {run:?}");
    assert_eq!(run.content, "Done", "{context}");
    assert!(
        permission_hook
            .requests
            .lock()
            .expect("permission requests")
            .is_empty(),
        "{context}"
    );
    assert!(
        provider
            .script
            .lock()
            .expect("remaining provider script")
            .is_empty(),
        "{context}"
    );
    run
}

fn assert_safe_projection(fixture: &ReadFixture, invalid_input: &str, projection: &str) {
    for forbidden in [
        invalid_input,
        SENSITIVE_MARKER,
        SENSITIVE_FIELD,
        SENSITIVE_FILE_NAME,
        fixture.root.to_str().expect("UTF-8 fixture root"),
    ] {
        assert!(
            !projection.contains(forbidden),
            "read feedback or audit must not echo raw input, sensitive paths, fields or contents"
        );
    }
}

fn assert_invalid_events(run: &ConversationRun, tool_name: &str, expected_reuse: &[bool]) {
    let proposals: Vec<_> = run
        .events
        .iter()
        .filter_map(|event| match event {
            ConversationEvent::ToolCallProposed { call } if call.id == INVALID_CALL_ID => {
                Some(call)
            }
            _ => None,
        })
        .collect();
    assert_eq!(proposals.len(), expected_reuse.len());
    for proposal in proposals {
        assert_eq!(proposal.tool, tool_name);
        assert_eq!(proposal.input_json, "{}");
    }
    assert!(run.events.iter().all(|event| !matches!(
        event,
        ConversationEvent::ToolCallApproved { call_id, .. }
            | ConversationEvent::ToolCallRunning { call_id }
            if call_id == INVALID_CALL_ID
    )));
    let terminals: Vec<_> = run
        .events
        .iter()
        .filter_map(|event| match event {
            ConversationEvent::ToolCallFinished { call_id, result }
                if call_id == INVALID_CALL_ID =>
            {
                Some(result)
            }
            _ => None,
        })
        .collect();
    assert_eq!(terminals.len(), expected_reuse.len());
    for (terminal, expected_reused) in terminals.into_iter().zip(expected_reuse) {
        assert_eq!(terminal.status, ToolCallStatus::Rejected);
        assert_eq!(terminal.output, INVALID_READ_ERROR);
        assert_eq!(terminal.exit_code, None);
        assert_eq!(terminal.duration_ms, None);
        assert_eq!(terminal.truncated, None);
        assert_eq!(terminal.reused, *expected_reused);
    }
}

fn assert_invalid_state(
    fixture: &ReadFixture,
    tool_name: &str,
    invalid_input: &str,
) -> tool_calls::ToolCallState {
    let rejected = tool_calls::find_state(fixture.store.conn(), INVALID_CALL_ID)
        .expect("query rejected read")
        .expect("persisted invalid read");
    assert_eq!(rejected.tool, tool_name);
    assert_eq!(rejected.status, "rejected");
    assert_eq!(rejected.input_json, "{}");
    assert_eq!(rejected.output_text.as_deref(), Some(INVALID_READ_ERROR));
    assert_eq!(rejected.exit_code, None);
    assert_eq!(rejected.duration_ms, None);
    assert_eq!(rejected.output_full_path, None);
    let approval = rejected.approval.as_deref().expect("validation audit");
    let audit = ApprovalAudit::from_json(approval).expect("strict validation audit");
    assert_eq!(
        audit,
        ApprovalAudit {
            decision: Approval::Deny,
            source: ApprovalSource::Validation,
            note: None,
            danger: None,
        }
    );
    assert_safe_projection(fixture, invalid_input, &rejected.input_json);
    assert_safe_projection(fixture, invalid_input, approval);
    assert_safe_projection(
        fixture,
        invalid_input,
        rejected
            .output_text
            .as_deref()
            .expect("persisted read error"),
    );
    rejected
}

fn tool_feedback(request: &ChatRequest, call_id: &str) -> Vec<String> {
    request
        .messages
        .iter()
        .filter_map(|message| {
            if message.role == ChatRole::Tool && message.tool_call_id.as_deref() == Some(call_id) {
                return Some(message.content.clone());
            }
            if message.role != ChatRole::User {
                return None;
            }
            let value: serde_json::Value = serde_json::from_str(&message.content).ok()?;
            let observation = value.get("vega_history_observation")?;
            if observation.get("kind").and_then(serde_json::Value::as_str) != Some("tool_result")
                || observation
                    .get("call_id")
                    .and_then(serde_json::Value::as_str)
                    != Some(call_id)
            {
                return None;
            }
            assert_eq!(observation["authority"], "none");
            assert_eq!(observation["version"], 1);
            Some(
                observation["output"]
                    .as_str()
                    .expect("tool result output")
                    .to_owned(),
            )
        })
        .collect()
}

fn assert_tool_feedback(request: &ChatRequest, call_id: &str, expected: &str) -> usize {
    let results = tool_feedback(request, call_id);
    assert!(!results.is_empty(), "provider must receive the tool result");
    for result in &results {
        assert_eq!(result, expected);
    }
    results.len()
}

async fn assert_invalid_then_valid_read(
    fixture: &ReadFixture,
    tool_name: &str,
    input: &str,
    case: &str,
) {
    let provider = ScriptedProvider::new(tool_name, input);
    let permission_hook = RecordingPermissionHook::default();
    let context = format!("{tool_name}/{case}");
    let run = run_script(fixture, &provider, &permission_hook, &context).await;
    assert_invalid_events(&run, tool_name, &[false]);
    assert_invalid_state(fixture, tool_name, input);
    let successful = tool_calls::find_state(fixture.store.conn(), VALID_CALL_ID)
        .expect("query valid read")
        .expect("persisted valid read");
    assert_eq!(successful.status, "success");
    assert_eq!(successful.output_text.as_deref(), Some(READ_OUTPUT));
    let requests = provider
        .requests
        .lock()
        .expect("captured provider requests");
    assert_eq!(requests.len(), 3);
    assert_tool_feedback(&requests[1], INVALID_CALL_ID, INVALID_READ_ERROR);
    assert_tool_feedback(&requests[2], VALID_CALL_ID, READ_OUTPUT);
    for feedback in tool_feedback(&requests[1], INVALID_CALL_ID) {
        assert_safe_projection(fixture, input, &feedback);
    }
}

fn tool_row_count(fixture: &ReadFixture) -> i64 {
    fixture
        .store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM tool_calls WHERE thread_id = ?1",
            [fixture.thread_id.as_str()],
            |row| row.get(0),
        )
        .expect("count durable tool calls")
}

fn assert_invalid_history(fixture: &ReadFixture) {
    let page = history::latest_history_page(&fixture.store, &fixture.thread_id, 200)
        .expect("read durable tool history");
    let tools: Vec<_> = page
        .entries
        .iter()
        .filter(|entry| matches!(entry, HistoryEntry::Tool { call_id, .. } if call_id == INVALID_CALL_ID))
        .collect();
    assert_eq!(tools.len(), 1);
    match tools[0] {
        HistoryEntry::Tool {
            status,
            approval,
            result,
            ..
        } => {
            assert_eq!(*status, ToolCallStatus::Rejected);
            assert_eq!(*approval, Some(Approval::Deny));
            assert_eq!(
                result.as_ref(),
                Some(&ToolCardResultProjection::ReadOnly {
                    status: ToolCallStatus::Rejected,
                    output: INVALID_READ_ERROR.into(),
                    reused: true,
                })
            );
        }
        _ => panic!("expected invalid read tool history entry"),
    }
}

#[tokio::test]
async fn invalid_read_is_safely_rejected_before_a_valid_read_and_final_answer() {
    for tool_name in ["read", "Read"] {
        for (case, input) in INVALID_READ_INPUTS {
            let fixture = ReadFixture::new();
            assert_invalid_then_valid_read(&fixture, tool_name, input, case).await;
        }
        let fixture = ReadFixture::new();
        assert!(fixture.tools.file_path(PROTECTED_PATH).is_err());
        let input =
            serde_json::json!({"path": PROTECTED_PATH, "offset": null, "limit": null}).to_string();
        assert_invalid_then_valid_read(&fixture, tool_name, &input, "protected_git_path").await;
    }
}

#[tokio::test]
async fn invalid_read_rejection_survives_reopen_recovery_history_and_call_id_reuse() {
    for tool_name in ["read", "Read"] {
        let fixture = ReadFixture::new();
        let permission_hook = RecordingPermissionHook::default();
        let provider = ScriptedProvider::from_turns(vec![
            read_turn(tool_name, INVALID_CALL_ID, MISSING_PATH_INPUT),
            read_turn(tool_name, INVALID_CALL_ID, MISSING_PATH_INPUT),
            answer_turn(),
        ]);
        let run = run_script(&fixture, &provider, &permission_hook, tool_name).await;
        assert_invalid_events(&run, tool_name, &[false, true]);
        let original = assert_invalid_state(&fixture, tool_name, MISSING_PATH_INPUT);
        assert_eq!(tool_row_count(&fixture), 1);
        {
            let requests = provider
                .requests
                .lock()
                .expect("first-run provider requests");
            assert_eq!(requests.len(), 3);
            assert_eq!(
                assert_tool_feedback(&requests[1], INVALID_CALL_ID, INVALID_READ_ERROR),
                1
            );
            assert_eq!(
                assert_tool_feedback(&requests[2], INVALID_CALL_ID, INVALID_READ_ERROR),
                2
            );
        }

        let fixture = fixture.reopen();
        assert_eq!(
            assert_invalid_state(&fixture, tool_name, MISSING_PATH_INPUT),
            original
        );
        let recovered = recovery::recover_thread(fixture.store.conn(), &fixture.thread_id, 1000)
            .expect("recover the reopened fixture thread");
        assert_eq!(recovered.tools_rejected, 0);
        assert_eq!(recovered.tools_cancelled, 0);
        assert_eq!(
            assert_invalid_state(&fixture, tool_name, MISSING_PATH_INPUT),
            original
        );
        assert_invalid_history(&fixture);
        assert_eq!(tool_row_count(&fixture), 1);

        let provider = ScriptedProvider::from_turns(vec![
            read_turn(tool_name, INVALID_CALL_ID, MISSING_PATH_INPUT),
            answer_turn(),
        ]);
        let run = run_script(&fixture, &provider, &permission_hook, tool_name).await;
        assert_invalid_events(&run, tool_name, &[true]);
        assert_eq!(
            assert_invalid_state(&fixture, tool_name, MISSING_PATH_INPUT),
            original
        );
        assert_eq!(tool_row_count(&fixture), 1);
        assert_invalid_history(&fixture);
        let requests = provider.requests.lock().expect("resumed provider requests");
        assert_eq!(requests.len(), 2);
        let history_results =
            assert_tool_feedback(&requests[0], INVALID_CALL_ID, INVALID_READ_ERROR);
        let resumed_results =
            assert_tool_feedback(&requests[1], INVALID_CALL_ID, INVALID_READ_ERROR);
        assert_eq!(resumed_results, history_results + 1);
    }
}
