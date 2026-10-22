use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use futures::future::BoxFuture;
use tokio_util::sync::CancellationToken;
use vega_runtime::{
    ChatRequest, ChatRole, EventStream, Provider, ProviderEvent, RuntimeEvent, RuntimeToolCall,
    RuntimeToolResult, RuntimeToolStatus, StopReason, USER_QUESTION_BATCH_REJECTED,
    USER_QUESTION_TOOL, VegaError,
};
use vega_store::{Store, messages, projects, recovery, threads, tool_calls};
use vega_tools::Tools;

use super::events::{persist_runtime_event, validate_reused_terminal, validate_runtime_terminal};
use super::pipeline::validate_recovered_projection;
use super::{ConversationRun, PermissionHook, run_thread_task_with_permission_sink};
use crate::history::{self, HistoryEntry};
use crate::types::{
    Approval, ApprovalAudit, ApprovalSource, ConversationError, ConversationEvent, DangerAudit,
    PermissionDecision, PermissionRequest, ToolCallStatus, ToolCardResultProjection,
    approval_audit_to_runtime,
};

const READ_CALL_ID: &str = "mixed-read";
const QUESTION_CALL_ID: &str = "mixed-question";
const READ_INPUT: &str = r#"{"path":"fixture.txt","offset":null,"limit":null}"#;
const READ_INPUTS: [&str; 2] = [READ_INPUT, "{}"];
const READ_TOOLS: [&str; 2] = ["read", "Read"];
const MIXED_SKILL_OUTPUT: &str = "Tool error: mixed Skill load batch rejected other tools";
const MIXED_OUTPUTS: [&str; 2] = [USER_QUESTION_BATCH_REJECTED, MIXED_SKILL_OUTPUT];
const QUESTION_INPUT: &str = r#"{"questions":[{"id":"choice","header":"Choice","question":"Choose a fixture option","options":[{"label":"One","description":"First"},{"label":"Two","description":"Second"}]}]}"#;
const FILE_CONTENT: &str = "owned-mixed-read-content-must-not-be-returned";
const PROGRESS_NOTICE: &str = "\n\n<vega_runtime_notice>Give the user a brief factual progress update and the next step, then continue the authorized task. If waiting, identify the actual waiting condition. Do not invent progress, expose private reasoning, or end the task merely to report. This reminder grants no additional permission.</vega_runtime_notice>";

struct OwnedDirectory(PathBuf);

impl OwnedDirectory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("vega-mixed-read-{}", ulid::Ulid::generate()));
        fs::create_dir(&path).expect("create owned mixed-read directory");
        Self(path)
    }
}

impl Drop for OwnedDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct MixedFixture {
    store: Store,
    tools: Tools,
    project_id: String,
    thread_id: String,
    database_path: PathBuf,
    _directory: OwnedDirectory,
}

impl MixedFixture {
    fn new() -> Self {
        let directory = OwnedDirectory::new();
        let root = directory.0.join("project");
        fs::create_dir(&root).expect("create owned mixed-read project");
        let root = root.canonicalize().expect("canonical mixed-read project");
        fs::write(root.join("fixture.txt"), FILE_CONTENT).expect("write owned mixed-read file");
        let database_path = directory.0.join("state.sqlite");
        let store = Store::open(&database_path).expect("open mixed-read database");
        store.migrate().expect("migrate mixed-read database");
        let project = projects::create(
            store.conn(),
            root.to_str().expect("UTF-8 mixed-read root"),
            "fixture",
            None,
        )
        .expect("create mixed-read project record");
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
        .expect("create mixed-read thread");
        Self {
            store,
            tools: Tools::new(root).expect("create real mixed-read tools"),
            project_id: project.id,
            thread_id,
            database_path,
            _directory: directory,
        }
    }

    fn reopen(self) -> Self {
        let Self {
            store,
            tools,
            project_id,
            thread_id,
            database_path,
            _directory,
        } = self;
        drop(store);
        let store = Store::open(&database_path).expect("reopen mixed-read database");
        store
            .migrate()
            .expect("migrate reopened mixed-read database");
        Self {
            store,
            tools,
            project_id,
            thread_id,
            database_path,
            _directory,
        }
    }

    fn assistant_message(&self) -> String {
        let id = ulid::Ulid::generate().to_string();
        messages::insert(
            self.store.conn(),
            &messages::MessageRow {
                id: id.clone(),
                thread_id: self.thread_id.clone(),
                seq: messages::next_seq(self.store.conn(), &self.thread_id)
                    .expect("next mixed-read message sequence"),
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
        .expect("insert mixed-read assistant message");
        id
    }

    fn persist(
        &self,
        message_id: &str,
        next_seq: &mut i64,
        event: &RuntimeEvent,
    ) -> Result<(), VegaError> {
        persist_runtime_event(
            &self.store,
            &self.project_id,
            &self.thread_id,
            message_id,
            "test",
            false,
            "",
            next_seq,
            event,
            None,
        )
    }

    fn state(&self, call_id: &str) -> tool_calls::ToolCallState {
        tool_calls::find_state(self.store.conn(), call_id)
            .expect("query mixed-read state")
            .expect("durable mixed-read call")
    }

    fn tool_count(&self) -> i64 {
        self.store
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM tool_calls WHERE thread_id = ?1",
                [self.thread_id.as_str()],
                |row| row.get(0),
            )
            .expect("count mixed-read records")
    }
}

struct ScriptedProvider {
    script: Arc<Mutex<VecDeque<Vec<ProviderEvent>>>>,
    requests: Arc<Mutex<Vec<ChatRequest>>>,
}

impl ScriptedProvider {
    fn new(turns: Vec<Vec<ProviderEvent>>) -> Self {
        Self {
            script: Arc::new(Mutex::new(VecDeque::from(turns))),
            requests: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl Provider for ScriptedProvider {
    fn chat_stream(
        &self,
        req: ChatRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        self.requests
            .lock()
            .expect("capture mixed-read request")
            .push(req);
        let events = self
            .script
            .lock()
            .expect("lock mixed-read script")
            .pop_front()
            .expect("unexpected request beyond mixed-read script");
        Box::pin(async move {
            let stream: EventStream = Box::pin(futures::stream::iter(events.into_iter().map(Ok)));
            Ok(stream)
        })
    }
}

#[derive(Default)]
struct RecordingPermissionHook(Mutex<Vec<PermissionRequest>>);

impl PermissionHook for RecordingPermissionHook {
    fn request(
        &self,
        request: PermissionRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<PermissionDecision, VegaError>> {
        self.0
            .lock()
            .expect("record mixed-read permission")
            .push(request);
        Box::pin(async { Ok(PermissionDecision::Deny { note: None }) })
    }
}

fn tool_use(id: &str, name: &str, input: &str) -> ProviderEvent {
    ProviderEvent::ToolUse {
        id: id.into(),
        name: name.into(),
        input_json: input.into(),
    }
}

fn answer_turn() -> Vec<ProviderEvent> {
    vec![
        ProviderEvent::TextDelta("Done".into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ]
}

async fn run_script(
    fixture: &MixedFixture,
    provider: &ScriptedProvider,
    hook: &RecordingPermissionHook,
) -> ConversationRun {
    let run = run_thread_task_with_permission_sink(
        &fixture.store,
        provider,
        &fixture.tools,
        &fixture.thread_id,
        "Handle the requested batch and answer Done.",
        "Use the supplied tools and finish the turn.",
        CancellationToken::new(),
        hook,
        |_| Ok(()),
    )
    .await
    .expect("mixed-batch read rejection must not abort conversation persistence");
    assert!(!run.failed, "{run:?}");
    assert!(!run.interrupted, "{run:?}");
    assert_eq!(run.content, "Done");
    assert!(hook.0.lock().expect("mixed-read permissions").is_empty());
    assert!(
        provider
            .script
            .lock()
            .expect("remaining mixed-read script")
            .is_empty()
    );
    run
}

fn assert_read_events(run: &ConversationRun, tool: &str, input: &str, reused: bool) {
    let proposals: Vec<_> = run
        .events
        .iter()
        .filter_map(|event| match event {
            ConversationEvent::ToolCallProposed { call } if call.id == READ_CALL_ID => Some(call),
            _ => None,
        })
        .collect();
    assert_eq!(proposals.len(), 1);
    assert_eq!(proposals[0].tool, tool);
    assert_eq!(proposals[0].input_json, input);
    assert!(run.events.iter().all(|event| !matches!(
        event,
        ConversationEvent::ToolCallApproved { call_id, .. }
            | ConversationEvent::ToolCallRunning { call_id }
            if call_id == READ_CALL_ID || call_id == QUESTION_CALL_ID
    )));
    let terminals: Vec<_> = run
        .events
        .iter()
        .filter_map(|event| match event {
            ConversationEvent::ToolCallFinished { call_id, result } if call_id == READ_CALL_ID => {
                Some(result)
            }
            _ => None,
        })
        .collect();
    assert_eq!(terminals.len(), 1);
    let terminal = terminals[0];
    assert_eq!(terminal.status, ToolCallStatus::Rejected);
    assert_eq!(terminal.output, USER_QUESTION_BATCH_REJECTED);
    assert_eq!(terminal.reused, reused);
    assert_eq!(terminal.exit_code, None);
    assert_eq!(terminal.duration_ms, None);
    assert_eq!(terminal.truncated, None);
    assert!(!terminal.output.contains(FILE_CONTENT));
}

fn validation_audit() -> ApprovalAudit {
    ApprovalAudit {
        decision: Approval::Deny,
        source: ApprovalSource::Validation,
        note: None,
        danger: None,
    }
}

fn assert_state(
    fixture: &MixedFixture,
    tool: &str,
    input: &str,
    output: &str,
) -> tool_calls::ToolCallState {
    let state = fixture.state(READ_CALL_ID);
    assert_eq!(state.tool, tool);
    assert_eq!(state.input_json, input);
    assert_eq!(state.status, "rejected");
    assert_eq!(state.output_text.as_deref(), Some(output));
    assert_eq!(state.exit_code, None);
    assert_eq!(state.duration_ms, None);
    assert_eq!(state.output_full_path, None);
    let audit = ApprovalAudit::from_json(state.approval.as_deref().expect("mixed-read audit"))
        .expect("strict mixed-read approval codec");
    assert_eq!(audit, validation_audit());
    assert_eq!(
        ApprovalAudit::from_json(&audit.to_json().expect("encode mixed-read audit"))
            .expect("decode mixed-read audit"),
        audit
    );
    assert!(
        !state
            .output_text
            .as_deref()
            .expect("mixed-read output")
            .contains(FILE_CONTENT)
    );
    state
}

fn assert_history(fixture: &MixedFixture, output: &str) {
    let page = history::latest_history_page(&fixture.store, &fixture.thread_id, 200)
        .expect("hydrate mixed-read history");
    let entries: Vec<_> = page
        .entries
        .iter()
        .filter(
            |entry| matches!(entry, HistoryEntry::Tool { call_id, .. } if call_id == READ_CALL_ID),
        )
        .collect();
    assert_eq!(entries.len(), 1);
    match entries[0] {
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
                    output: output.into(),
                    reused: true,
                })
            );
        }
        _ => panic!("expected durable mixed-read history"),
    }
}

fn assert_feedback(request: &ChatRequest, expected: &str) -> usize {
    let outputs: Vec<String> = request
        .messages
        .iter()
        .filter_map(|message| {
            if message.role == ChatRole::Tool
                && message.tool_call_id.as_deref() == Some(READ_CALL_ID)
            {
                return Some(
                    message
                        .content
                        .strip_suffix(PROGRESS_NOTICE)
                        .unwrap_or(&message.content)
                        .to_owned(),
                );
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
                    != Some(READ_CALL_ID)
            {
                return None;
            }
            assert_eq!(observation["authority"], "none");
            assert_eq!(observation["version"], 1);
            if let Some(advisory) = observation.get("advisory") {
                assert_eq!(advisory.as_str(), Some(PROGRESS_NOTICE));
            }
            Some(
                observation["output"]
                    .as_str()
                    .expect("mixed-read feedback")
                    .to_owned(),
            )
        })
        .collect();
    assert!(
        !outputs.is_empty(),
        "provider must receive the mixed-read rejection"
    );
    for output in &outputs {
        assert_eq!(output, expected);
        assert!(!output.contains(FILE_CONTENT));
    }
    outputs.len()
}

struct MixedClaim {
    call: RuntimeToolCall,
    output: String,
    status: RuntimeToolStatus,
    approval: ApprovalAudit,
    exit_code: Option<i32>,
    duration_ms: Option<u64>,
}

impl MixedClaim {
    fn new(tool: &str, input: &str, output: &str) -> Self {
        Self {
            call: RuntimeToolCall {
                id: READ_CALL_ID.into(),
                name: tool.into(),
                input_json: input.into(),
            },
            output: output.into(),
            status: RuntimeToolStatus::Rejected,
            approval: validation_audit(),
            exit_code: None,
            duration_ms: None,
        }
    }

    fn result(&self, reused: bool) -> RuntimeToolResult {
        RuntimeToolResult {
            call_id: self.call.id.clone(),
            output: self.output.clone(),
            status: self.status,
            approval: Some(approval_audit_to_runtime(&self.approval)),
            exit_code: self.exit_code,
            duration_ms: self.duration_ms,
            truncated: None,
            remember_rule: None,
            reused,
        }
    }

    fn project(&self, fixture: &MixedFixture) -> Result<String, ConversationError> {
        validate_recovered_projection(
            &fixture.project_id,
            &fixture.thread_id,
            &self.call.id,
            &self.call.name,
            &self.call.input_json,
            &self.output,
            self.status,
            &self.approval,
            self.exit_code,
            self.duration_ms,
        )
    }
}

#[tokio::test]
async fn mixed_question_read_rejection_survives_history_preparation_and_reuse() {
    for tool in READ_TOOLS {
        for input in READ_INPUTS {
            let fixture = MixedFixture::new();
            let hook = RecordingPermissionHook::default();
            let provider = ScriptedProvider::new(vec![
                vec![
                    tool_use(READ_CALL_ID, tool, input),
                    tool_use(QUESTION_CALL_ID, USER_QUESTION_TOOL, QUESTION_INPUT),
                    ProviderEvent::Done {
                        stop_reason: StopReason::ToolUse,
                    },
                ],
                answer_turn(),
            ]);
            let run = run_script(&fixture, &provider, &hook).await;
            assert_read_events(&run, tool, input, false);
            let original = assert_state(&fixture, tool, input, USER_QUESTION_BATCH_REJECTED);
            assert_eq!(fixture.state(QUESTION_CALL_ID).status, "rejected");
            assert_eq!(fixture.tool_count(), 2);
            {
                let requests = provider.requests.lock().expect("mixed-question requests");
                assert_eq!(requests.len(), 2);
                assert_eq!(
                    assert_feedback(&requests[1], USER_QUESTION_BATCH_REJECTED),
                    1
                );
            }

            let fixture = fixture.reopen();
            let recovered =
                recovery::recover_thread(fixture.store.conn(), &fixture.thread_id, 1000)
                    .expect("recover completed mixed-question batch");
            assert_eq!(recovered.tools_rejected, 0);
            assert_eq!(recovered.tools_cancelled, 0);
            assert_eq!(
                assert_state(&fixture, tool, input, USER_QUESTION_BATCH_REJECTED),
                original
            );
            assert_history(&fixture, USER_QUESTION_BATCH_REJECTED);
            let provider = ScriptedProvider::new(vec![
                vec![
                    tool_use(READ_CALL_ID, tool, input),
                    ProviderEvent::Done {
                        stop_reason: StopReason::ToolUse,
                    },
                ],
                answer_turn(),
            ]);
            let run = run_script(&fixture, &provider, &hook).await;
            assert_read_events(&run, tool, input, true);
            assert_eq!(
                assert_state(&fixture, tool, input, USER_QUESTION_BATCH_REJECTED),
                original
            );
            assert_eq!(fixture.tool_count(), 2);
            assert_history(&fixture, USER_QUESTION_BATCH_REJECTED);
            let requests = provider
                .requests
                .lock()
                .expect("resumed mixed-read requests");
            assert_eq!(requests.len(), 2);
            let historical = assert_feedback(&requests[0], USER_QUESTION_BATCH_REJECTED);
            assert_eq!(
                assert_feedback(&requests[1], USER_QUESTION_BATCH_REJECTED),
                historical + 1
            );
        }
    }
}

#[test]
fn mixed_read_rejections_persist_project_and_reuse_both_fixed_outputs() {
    for output in MIXED_OUTPUTS {
        for tool in READ_TOOLS {
            for input in READ_INPUTS {
                let fixture = MixedFixture::new();
                let message_id = fixture.assistant_message();
                let claim = MixedClaim::new(tool, input, output);
                let mut next_seq = 1;
                fixture
                    .persist(
                        &message_id,
                        &mut next_seq,
                        &RuntimeEvent::ToolCallProposed(claim.call.clone()),
                    )
                    .expect("persist real mixed-read proposal");
                let pending = fixture.state(READ_CALL_ID);
                assert_eq!(pending.status, "pending_approval");
                assert_eq!(pending.approval, None);
                let result = claim.result(false);
                validate_runtime_terminal(
                    &fixture.project_id,
                    &fixture.thread_id,
                    READ_CALL_ID,
                    &pending,
                    &result,
                    &claim.approval,
                )
                .expect("mixed-read rejection can terminate directly from pending");
                fixture
                    .persist(
                        &message_id,
                        &mut next_seq,
                        &RuntimeEvent::ToolCallFinished(result),
                    )
                    .expect("persist fixed mixed-read rejection");
                let original = assert_state(&fixture, tool, input, output);
                assert_eq!(
                    claim
                        .project(&fixture)
                        .expect("recover fixed mixed-read projection"),
                    input
                );
                let result = claim.result(true);
                validate_reused_terminal(
                    &fixture.project_id,
                    &fixture.thread_id,
                    &original,
                    &result,
                )
                .expect("validate reused fixed mixed-read rejection");
                fixture
                    .persist(
                        &message_id,
                        &mut next_seq,
                        &RuntimeEvent::ToolCallProposed(claim.call.clone()),
                    )
                    .expect("persist repeated mixed-read proposal");
                fixture
                    .persist(
                        &message_id,
                        &mut next_seq,
                        &RuntimeEvent::ToolCallFinished(result),
                    )
                    .expect("persist reused mixed-read rejection");
                assert_eq!(next_seq, 2);
                assert_eq!(fixture.state(READ_CALL_ID), original);
                assert_eq!(fixture.tool_count(), 1);
                let fixture = fixture.reopen();
                assert_eq!(assert_state(&fixture, tool, input, output), original);
                assert_eq!(
                    claim
                        .project(&fixture)
                        .expect("reopened mixed-read projection"),
                    input
                );
                assert_history(&fixture, output);
            }
        }
    }
}

type ClaimMutation = fn(&mut MixedClaim);

const INVALID_CLAIMS: [(&str, ClaimMutation); 14] = [
    ("failed status", |claim| {
        claim.status = RuntimeToolStatus::Failed
    }),
    ("success status", |claim| {
        claim.status = RuntimeToolStatus::Success
    }),
    ("cancelled status", |claim| {
        claim.status = RuntimeToolStatus::Cancelled
    }),
    ("once decision", |claim| {
        claim.approval.decision = Approval::Once
    }),
    ("always decision", |claim| {
        claim.approval.decision = Approval::Always
    }),
    ("readonly source", |claim| {
        claim.approval.source = ApprovalSource::ReadonlyTool
    }),
    ("recovery source", |claim| {
        claim.approval.source = ApprovalSource::Recovery
    }),
    ("timeout source", |claim| {
        claim.approval.source = ApprovalSource::Timeout
    }),
    ("user source", |claim| {
        claim.approval.source = ApprovalSource::User
    }),
    ("full access source", |claim| {
        claim.approval.source = ApprovalSource::FullAccess
    }),
    ("exit code", |claim| claim.exit_code = Some(0)),
    ("duration", |claim| claim.duration_ms = Some(0)),
    ("non-object input", |claim| {
        claim.call.input_json = "[]".into()
    }),
    ("unknown output", |claim| {
        claim.output = "Tool error: unrecognized mixed batch rejection".into()
    }),
];

#[test]
fn mixed_read_rejection_forgeries_fail_projection_and_persistence_without_state_changes() {
    for output in MIXED_OUTPUTS {
        for tool in READ_TOOLS {
            for input in READ_INPUTS {
                let fixture = MixedFixture::new();
                let message_id = fixture.assistant_message();
                let mut next_seq = 1;
                for (name, mutate) in INVALID_CLAIMS {
                    let mut claim = MixedClaim::new(tool, input, output);
                    claim.call.id = ulid::Ulid::generate().to_string();
                    mutate(&mut claim);
                    fixture
                        .persist(
                            &message_id,
                            &mut next_seq,
                            &RuntimeEvent::ToolCallProposed(claim.call.clone()),
                        )
                        .expect("record proposal for mixed-read safety boundary");
                    let pending = fixture.state(&claim.call.id);
                    let result = claim.result(false);
                    assert!(
                        claim.project(&fixture).is_err(),
                        "{tool}/{input}/{output}: {name}"
                    );
                    assert!(
                        validate_runtime_terminal(
                            &fixture.project_id,
                            &fixture.thread_id,
                            &claim.call.id,
                            &pending,
                            &result,
                            &claim.approval
                        )
                        .is_err(),
                        "{tool}: {name}"
                    );
                    assert!(
                        fixture
                            .persist(
                                &message_id,
                                &mut next_seq,
                                &RuntimeEvent::ToolCallFinished(result)
                            )
                            .is_err(),
                        "{tool}: {name}"
                    );
                    assert_eq!(fixture.state(&claim.call.id), pending, "{tool}: {name}");
                }
            }
        }
    }
}

#[test]
fn mixed_read_validation_notes_and_danger_fail_the_strict_codec_and_persistence() {
    for output in MIXED_OUTPUTS {
        for tool in READ_TOOLS {
            for input in READ_INPUTS {
                let fixture = MixedFixture::new();
                let message_id = fixture.assistant_message();
                let mut next_seq = 1;
                for field in ["note", "danger"] {
                    let mut claim = MixedClaim::new(tool, input, output);
                    claim.call.id = ulid::Ulid::generate().to_string();
                    let mut wire: serde_json::Value = serde_json::from_str(
                        &claim
                            .approval
                            .to_json()
                            .expect("encode valid mixed-read audit"),
                    )
                    .expect("parse valid mixed-read audit");
                    match field {
                        "note" => {
                            claim.approval.note = Some(String::new());
                            wire["note"] = serde_json::json!("");
                        }
                        "danger" => {
                            claim.approval.danger = Some(DangerAudit {
                                rule_id: "fixture-danger".into(),
                                decision: Approval::Deny,
                                note: None,
                            });
                            wire["danger"] = serde_json::json!({"rule_id":"fixture-danger","decision":"deny","note":null});
                        }
                        _ => unreachable!(),
                    }
                    assert!(claim.approval.to_json().is_err(), "{tool}: {field}");
                    assert!(
                        ApprovalAudit::from_json(&wire.to_string()).is_err(),
                        "{tool}: {field}"
                    );
                    fixture
                        .persist(
                            &message_id,
                            &mut next_seq,
                            &RuntimeEvent::ToolCallProposed(claim.call.clone()),
                        )
                        .expect("persist proposal before invalid mixed-read audit");
                    let pending = fixture.state(&claim.call.id);
                    assert!(
                        fixture
                            .persist(
                                &message_id,
                                &mut next_seq,
                                &RuntimeEvent::ToolCallFinished(claim.result(false))
                            )
                            .is_err(),
                        "{tool}: {field}"
                    );
                    assert_eq!(fixture.state(&claim.call.id), pending, "{tool}: {field}");
                }
            }
        }
    }
}
