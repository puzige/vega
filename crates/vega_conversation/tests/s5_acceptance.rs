use std::collections::VecDeque;
use std::error::Error;
use std::fs;
use std::sync::{Arc, Mutex};

use futures::future::BoxFuture;
use tempfile::tempdir;
use tokio_util::sync::CancellationToken;
use vega_conversation::agent::{PermissionHook, run_thread_task_with_permission_sink};
use vega_conversation::types::{
    Approval, ApprovalAudit, ApprovalSource, ConversationEvent, PermissionDecision,
    PermissionRequest, ToolCallStatus,
};
use vega_runtime::{ChatRole, MockProvider, ProviderEvent, ScriptStep, StopReason, VegaError};
use vega_store::{Store, permissions, projects, threads, tool_calls};
use vega_tools::{
    CheckpointRef, CreatedNewFileMetadata, EditSuccessOutput, MutationTool, Tools, WriteEditAudit,
    WriteSuccessOutput,
};

const THREAD_ID: &str = "s5-acceptance-thread";
const WRITE_CALL: &str = "s5-write-call";
const EDIT_ALWAYS_CALL: &str = "s5-edit-always-call";
const EDIT_RULE_CALL: &str = "s5-edit-rule-call";
const BASH_CALL: &str = "s5-bash-deny-call";
const WRITE_BODY: &str = "S5_WRITE_BODY_SENTINEL";
const EDIT_OLD: &str = "S5_EDIT_OLD_SENTINEL";
const EDIT_MIDDLE: &str = "S5_EDIT_MIDDLE_SENTINEL";
const EDIT_FINAL: &str = "S5_EDIT_FINAL_SENTINEL";
const DENIAL_NOTE: &str = "S5 operator denied the command";

#[derive(Clone)]
struct ScriptedPermissionHook {
    decisions: Arc<Mutex<VecDeque<PermissionDecision>>>,
    requests: Arc<Mutex<Vec<PermissionRequest>>>,
}

impl ScriptedPermissionHook {
    fn new(decisions: impl IntoIterator<Item = PermissionDecision>) -> Self {
        Self {
            decisions: Arc::new(Mutex::new(decisions.into_iter().collect())),
            requests: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn requests(&self) -> Vec<PermissionRequest> {
        self.requests
            .lock()
            .map_or_else(|_| Vec::new(), |requests| requests.clone())
    }
}

impl PermissionHook for ScriptedPermissionHook {
    fn request(
        &self,
        request: PermissionRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<PermissionDecision, VegaError>> {
        let decision = self
            .decisions
            .lock()
            .ok()
            .and_then(|mut decisions| decisions.pop_front());
        if let Ok(mut requests) = self.requests.lock() {
            requests.push(request);
        }
        Box::pin(async move { Ok(decision.unwrap_or(PermissionDecision::Timeout)) })
    }
}

fn assert_absent(haystack: &str, needles: &[&str]) {
    for needle in needles {
        assert!(
            !haystack.contains(needle),
            "audited projection leaked a protected sentinel"
        );
    }
}

fn lifecycle(events: &[ConversationEvent], call_id: &str) -> Vec<&'static str> {
    events
        .iter()
        .filter_map(|event| match event {
            ConversationEvent::ToolCallProposed { call } if call.id == call_id => Some("proposed"),
            ConversationEvent::ToolCallApproved {
                call_id: event_call_id,
                ..
            } if event_call_id == call_id => Some("approved"),
            ConversationEvent::ToolCallOutput {
                call_id: event_call_id,
                ..
            } if event_call_id == call_id => Some("output"),
            ConversationEvent::ToolCallFinished {
                call_id: event_call_id,
                ..
            } if event_call_id == call_id => Some("finished"),
            _ => None,
        })
        .collect()
}

fn call_root(
    checkpoint_root: &std::path::Path,
    checkpoint_ref: &CheckpointRef,
) -> std::path::PathBuf {
    checkpoint_ref
        .as_str()
        .split('/')
        .skip(1)
        .fold(checkpoint_root.to_path_buf(), |path, component| {
            path.join(component)
        })
}

fn event_position(events: &[ConversationEvent], call_id: &str, terminal: bool) -> Option<usize> {
    events.iter().position(|event| match event {
        ConversationEvent::ToolCallProposed { call } if !terminal => call.id == call_id,
        ConversationEvent::ToolCallFinished {
            call_id: event_call_id,
            ..
        } if terminal => event_call_id == call_id,
        _ => false,
    })
}

#[tokio::test]
async fn confirm_mutations_are_checkpointed_audited_and_content_free_end_to_end()
-> Result<(), Box<dyn Error>> {
    let project_dir = tempdir()?;
    let data_dir = tempdir()?;
    fs::write(project_dir.path().join("existing.txt"), EDIT_OLD)?;

    let store = Store::open(data_dir.path().join("vega.db"))?;
    store.migrate()?;
    let project = projects::create(
        store.conn(),
        project_dir
            .path()
            .to_str()
            .ok_or("project path is not UTF-8")?,
        "s5-acceptance",
        Some("master"),
    )?;
    threads::create(
        store.conn(),
        threads::NewThread {
            id: THREAD_ID,
            project_id: &project.id,
            title: "S5 acceptance",
            mode: "execute",
            permission_mode: "confirm",
            model: "mock-s5-acceptance",
            status: "active",
            pinned: false,
            unread: false,
            created_at: 1,
            updated_at: 1,
        },
    )?;

    let tools = Tools::new(project_dir.path())?;
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: WRITE_CALL.into(),
                name: "write".into(),
                input_json: format!(r#"{{"path":"created.txt","content":"{WRITE_BODY}"}}"#),
            },
            ProviderEvent::ToolUse {
                id: EDIT_ALWAYS_CALL.into(),
                name: "edit".into(),
                input_json: format!(
                    r#"{{"path":"existing.txt","old_string":"{EDIT_OLD}","new_string":"{EDIT_MIDDLE}"}}"#
                ),
            },
            ProviderEvent::ToolUse {
                id: EDIT_RULE_CALL.into(),
                name: "edit".into(),
                input_json: format!(
                    r#"{{"path":"existing.txt","old_string":"{EDIT_MIDDLE}","new_string":"{EDIT_FINAL}"}}"#
                ),
            },
            ProviderEvent::ToolUse {
                id: BASH_CALL.into(),
                name: "bash".into(),
                input_json: r#"{"cmd":"printf should-not-run > denied.txt"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("S5 mock acceptance complete.".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
    ]);
    let hook = ScriptedPermissionHook::new([
        PermissionDecision::Once,
        PermissionDecision::Always,
        PermissionDecision::Deny {
            note: Some(DENIAL_NOTE.into()),
        },
    ]);
    let streamed = Arc::new(Mutex::new(Vec::<ConversationEvent>::new()));
    let sink_events = Arc::clone(&streamed);

    let run = run_thread_task_with_permission_sink(
        &store,
        &provider,
        &tools,
        THREAD_ID,
        "exercise the S5 permission flow",
        "You are a deterministic acceptance fixture.",
        CancellationToken::new(),
        &hook,
        move |event| {
            sink_events
                .lock()
                .map_err(|_| VegaError::Io(std::io::Error::other("event sink poisoned")))?
                .push(event.clone());
            Ok(())
        },
    )
    .await?;

    assert!(!run.failed && !run.interrupted);
    assert_eq!(
        fs::read_to_string(project_dir.path().join("created.txt"))?,
        WRITE_BODY
    );
    assert_eq!(
        fs::read_to_string(project_dir.path().join("existing.txt"))?,
        EDIT_FINAL
    );
    assert!(!project_dir.path().join("denied.txt").exists());

    let requests = hook.requests();
    assert_eq!(
        requests.len(),
        3,
        "the second edit must reuse its exact Always rule"
    );
    assert_eq!(requests[0].tool, "write");
    assert_eq!(requests[1].tool, "edit");
    assert_eq!(requests[2].tool, "bash");
    assert_eq!(
        requests[2].display_target,
        "printf should-not-run > denied.txt"
    );

    let write = tool_calls::find_state(store.conn(), WRITE_CALL)?.ok_or("missing write row")?;
    let first_edit =
        tool_calls::find_state(store.conn(), EDIT_ALWAYS_CALL)?.ok_or("missing first edit row")?;
    let second_edit =
        tool_calls::find_state(store.conn(), EDIT_RULE_CALL)?.ok_or("missing second edit row")?;
    let bash = tool_calls::find_state(store.conn(), BASH_CALL)?.ok_or("missing bash row")?;
    assert_eq!(write.status, "success");
    assert_eq!(first_edit.status, "success");
    assert_eq!(second_edit.status, "success");
    assert_eq!(bash.status, "rejected");
    assert!(write.exit_code.is_none() && write.duration_ms.is_none());
    assert!(first_edit.exit_code.is_none() && first_edit.duration_ms.is_none());
    assert!(second_edit.exit_code.is_none() && second_edit.duration_ms.is_none());
    assert!(bash.exit_code.is_none() && bash.duration_ms.is_none());
    assert!(
        [&write, &first_edit, &second_edit, &bash]
            .iter()
            .all(|state| state.output_full_path.is_none())
    );

    let write_audit = WriteEditAudit::from_json(&write.input_json)?;
    assert_eq!(write_audit.tool(), MutationTool::Write);
    assert_eq!(write_audit.path(), "created.txt");
    match write_audit {
        WriteEditAudit::Write {
            content_bytes,
            fingerprint_v1,
            ..
        } => {
            assert_eq!(content_bytes, WRITE_BODY.len() as u64);
            assert_eq!(fingerprint_v1.len(), 64);
            assert!(
                fingerprint_v1
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            );
        }
        WriteEditAudit::Edit { .. } => return Err("write row decoded as edit".into()),
    }
    for (state, expected_old, expected_new) in [
        (&first_edit, EDIT_OLD, EDIT_MIDDLE),
        (&second_edit, EDIT_MIDDLE, EDIT_FINAL),
    ] {
        let audit = WriteEditAudit::from_json(&state.input_json)?;
        assert_eq!(audit.tool(), MutationTool::Edit);
        assert_eq!(audit.path(), "existing.txt");
        match audit {
            WriteEditAudit::Edit {
                old_string_bytes,
                new_string_bytes,
                fingerprint_v1,
                ..
            } => {
                assert_eq!(old_string_bytes, expected_old.len() as u64);
                assert_eq!(new_string_bytes, expected_new.len() as u64);
                assert_eq!(fingerprint_v1.len(), 64);
                assert!(
                    fingerprint_v1
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                );
            }
            WriteEditAudit::Write { .. } => return Err("edit row decoded as write".into()),
        }
    }

    let write_approval =
        ApprovalAudit::from_json(write.approval.as_deref().ok_or("missing write approval")?)?;
    let first_edit_approval = ApprovalAudit::from_json(
        first_edit
            .approval
            .as_deref()
            .ok_or("missing first edit approval")?,
    )?;
    let second_edit_approval = ApprovalAudit::from_json(
        second_edit
            .approval
            .as_deref()
            .ok_or("missing second edit approval")?,
    )?;
    let bash_approval =
        ApprovalAudit::from_json(bash.approval.as_deref().ok_or("missing bash approval")?)?;
    assert_eq!(
        (write_approval.decision, write_approval.source),
        (Approval::Once, ApprovalSource::User)
    );
    assert_eq!(
        (first_edit_approval.decision, first_edit_approval.source),
        (Approval::Always, ApprovalSource::User)
    );
    assert_eq!(
        (second_edit_approval.decision, second_edit_approval.source),
        (Approval::Always, ApprovalSource::Rule)
    );
    assert_eq!(
        (bash_approval.decision, bash_approval.source),
        (Approval::Deny, ApprovalSource::User)
    );
    assert_eq!(bash_approval.note.as_deref(), Some(DENIAL_NOTE));
    assert_eq!(
        bash.approval.as_deref().ok_or("missing bash approval")?,
        bash_approval.to_json()?
    );

    assert!(matches!(
        run.events.first(),
        Some(ConversationEvent::MessageStarted { .. })
    ));
    assert!(matches!(
        run.events.last(),
        Some(ConversationEvent::MessageFinished { .. })
    ));
    assert_eq!(
        lifecycle(&run.events, WRITE_CALL),
        ["proposed", "approved", "output", "finished"]
    );
    assert_eq!(
        lifecycle(&run.events, EDIT_ALWAYS_CALL),
        ["proposed", "approved", "output", "finished"]
    );
    assert_eq!(
        lifecycle(&run.events, EDIT_RULE_CALL),
        ["proposed", "approved", "output", "finished"]
    );
    assert_eq!(
        lifecycle(&run.events, BASH_CALL),
        ["proposed", "output", "finished"]
    );
    for (current, next) in [
        (WRITE_CALL, EDIT_ALWAYS_CALL),
        (EDIT_ALWAYS_CALL, EDIT_RULE_CALL),
        (EDIT_RULE_CALL, BASH_CALL),
    ] {
        assert!(
            event_position(&run.events, current, true).ok_or("missing terminal event")?
                < event_position(&run.events, next, false).ok_or("missing proposal event")?,
            "tool calls must remain strictly serial"
        );
    }
    assert!(run.events.iter().any(|event| matches!(
        event,
        ConversationEvent::ToolCallFinished { call_id, result }
            if call_id == BASH_CALL && result.status == ToolCallStatus::Rejected
    )));

    let write_output =
        WriteSuccessOutput::from_json(write.output_text.as_deref().ok_or("missing write output")?)?;
    let first_edit_output = EditSuccessOutput::from_json(
        first_edit
            .output_text
            .as_deref()
            .ok_or("missing first edit output")?,
    )?;
    let second_edit_output = EditSuccessOutput::from_json(
        second_edit
            .output_text
            .as_deref()
            .ok_or("missing second edit output")?,
    )?;
    for checkpoint_ref in [
        &write_output.checkpoint_ref,
        &first_edit_output.checkpoint_ref,
        &second_edit_output.checkpoint_ref,
    ] {
        CheckpointRef::parse(checkpoint_ref.as_str())?;
        assert_absent(
            checkpoint_ref.as_str(),
            &[
                &project.id,
                THREAD_ID,
                WRITE_CALL,
                EDIT_ALWAYS_CALL,
                EDIT_RULE_CALL,
            ],
        );
    }

    let checkpoint_root = data_dir.path().join("checkpoints");
    let write_call_root = call_root(&checkpoint_root, &write_output.checkpoint_ref);
    let metadata_json = fs::read_to_string(write_call_root.join("metadata.json"))?;
    assert_eq!(
        CreatedNewFileMetadata::from_json(&metadata_json)?.path(),
        "created.txt"
    );
    assert!(!write_call_root.join("files").exists());

    let first_edit_root = call_root(&checkpoint_root, &first_edit_output.checkpoint_ref);
    assert!(!first_edit_root.join("metadata.json").exists());
    assert_eq!(
        fs::read_to_string(first_edit_root.join("files/existing.txt"))?,
        EDIT_OLD
    );
    let second_edit_root = call_root(&checkpoint_root, &second_edit_output.checkpoint_ref);
    assert!(!second_edit_root.join("metadata.json").exists());
    assert_eq!(
        fs::read_to_string(second_edit_root.join("files/existing.txt"))?,
        EDIT_MIDDLE
    );

    let protected = [
        WRITE_BODY,
        EDIT_OLD,
        EDIT_MIDDLE,
        EDIT_FINAL,
        data_dir.path().to_str().ok_or("data path is not UTF-8")?,
    ];
    for state in [&write, &first_edit, &second_edit] {
        assert_absent(&state.input_json, &protected);
        assert_absent(
            state.output_text.as_deref().ok_or("missing output")?,
            &protected,
        );
    }
    let event_text = format!("{:?}", run.events);
    let sink_text = format!(
        "{:?}",
        streamed
            .lock()
            .map_err(|_| "event sink poisoned")?
            .as_slice()
    );
    assert_absent(&event_text, &protected);
    assert_absent(&sink_text, &protected);

    let provider_requests = provider.requests();
    assert_eq!(provider_requests.len(), 2);
    let observe = &provider_requests[1];
    let observed_results = observe
        .messages
        .iter()
        .filter(|message| message.role == ChatRole::Tool)
        .collect::<Vec<_>>();
    assert_eq!(observed_results.len(), 4);
    assert_eq!(
        observed_results
            .iter()
            .filter_map(|message| message.tool_call_id.as_deref())
            .collect::<Vec<_>>(),
        [WRITE_CALL, EDIT_ALWAYS_CALL, EDIT_RULE_CALL, BASH_CALL]
    );
    assert!(
        observed_results
            .last()
            .is_some_and(|message| message.content.contains("denied"))
    );
    assert_absent(&format!("{observe:?}"), &protected);

    let rules = permissions::list_exact(store.conn(), &project.id)?;
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].tool, "edit");
    assert_eq!(rules[0].pattern, "existing.txt");
    assert_eq!(rules[0].project_id, project.id);
    assert!(rules[0].id > 0 && rules[0].created_at > 0);
    Ok(())
}
