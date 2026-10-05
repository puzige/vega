use super::*;
use crate::history::{
    HistoryEntry, HistoryPage, SkillHistoryOrigin, SkillHistorySource, SkillHistoryStatus,
    SkillHistoryVerification,
};
use crate::types::{
    DangerAudit, PermissionMode, PlanStatus, SkillCardOutcome, SkillToolKind,
    ToolCardInputProjection, ToolCardResultProjection,
};
use std::sync::atomic::AtomicBool;
use vega_store::permissions;

const SCRIPT_COMMAND: &str = "sh scripts/harmless.sh";
const DANGEROUS_COMMAND: &str = "sh scripts/harmless.sh && git push --force";
const SCRIPT_BYTES: &str = "printf 'S16_DURABLE_PRIVATE_SCRIPT' > s16-result.txt\n";
const SKILL_BODY_MARKER: &str = "S16_DURABLE_PRIVATE_GUIDANCE";
const USER_TASK: &str = "Please follow the approved Skill for this owned task.";
const MOCK_OUTPUT: &str = "s16-mock-output";
const LOAD_CALL_ID: &str = "s16-durable-load";

fn write_allowed_tools_skill(root: &Path, command: &str) {
    let directory = root.join("runner");
    fs::create_dir_all(&directory).unwrap();
    let bytes = format!(
        "---\nname: runner\ndescription: Run an explicitly requested owned script.\nallowed-tools: Bash(*)\n---\nWhen asked, run `{command}` through ordinary Bash approval.\n{SKILL_BODY_MARKER}\n"
    );
    fs::write(directory.join("SKILL.md"), &bytes).unwrap();
    let document = vega_runtime::skills::parse_skill_md(bytes.as_bytes(), "runner").unwrap();
    assert_eq!(document.metadata.allowed_tools.as_deref(), Some("Bash(*)"));
}

fn assert_allowed_tools_source(source: &SkillSource) {
    let discovered = source.discover().unwrap();
    assert!(discovered.diagnostics.is_empty());
    assert_eq!(discovered.candidates.len(), 1);
    let candidate = &discovered.candidates[0];
    let document = vega_runtime::skills::parse_skill_md(
        &source.load_candidate(candidate).unwrap(),
        &candidate.name,
    )
    .unwrap();
    assert_eq!(document.metadata.allowed_tools.as_deref(), Some("Bash(*)"));
}

fn pin_allowed_tools_skill(store: &Store, project: &Path, project_id: &str) -> String {
    let root = project.join(".agents/skills");
    write_allowed_tools_skill(&root, SCRIPT_COMMAND);
    fs::create_dir_all(project.join("scripts")).unwrap();
    fs::write(project.join("scripts/harmless.sh"), SCRIPT_BYTES).unwrap();
    let source = SkillSource::project_approved(project).unwrap().unwrap();
    assert_allowed_tools_source(&source);
    let settings = skills::read_settings(store.conn()).unwrap();
    skills::set_project_settings(
        store.conn(),
        settings.consent_generation,
        project_id,
        true,
        false,
    )
    .unwrap();
    let (name, hash) = approve(
        store,
        &source,
        "source-s16-project",
        Some(project_id),
        &root,
        false,
    );
    let identity = source.identity();
    let root_dev = identity.device().to_string();
    let root_ino = identity.inode().to_string();
    let settings = skills::read_settings(store.conn()).unwrap();
    skills::save_thread_pin(
        store.conn(),
        settings.consent_generation,
        NewThreadSkillPin {
            thread_id: "thread-1",
            scope: "project",
            canonical_root: identity.canonical_root().to_str().unwrap(),
            root_dev: &root_dev,
            root_ino: &root_ino,
            name: &name,
            approved_sha256: &hash,
            source_label: "source-s16-project",
            pinned_at: 3,
        },
    )
    .unwrap();
    hash
}

fn script_provider(call_id: &str, command: Option<&str>, load: bool) -> MockProvider {
    let mut rounds = Vec::new();
    if load {
        rounds.push(vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: LOAD_CALL_ID.into(),
                name: "load_skill".into(),
                input_json: r#"{"name":"runner"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])]);
    }
    if let Some(command) = command {
        rounds.push(vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: call_id.into(),
                name: "bash".into(),
                input_json: serde_json::json!({"cmd": command, "timeout_ms": null}).to_string(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])]);
    }
    rounds.push(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("Owned task complete.".into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]);
    MockProvider::new_rounds(rounds)
}

type ScriptDispatches = Arc<Mutex<Vec<(String, bool)>>>;

fn script_tools(
    project: &Path,
    command: &str,
    executions: ScriptDispatches,
    activated: Arc<AtomicBool>,
) -> vega_tools::Tools {
    let expected_command = command.to_owned();
    let result = project.join("s16-result.txt");
    vega_tools::Tools::new(project)
        .unwrap()
        .with_bash_test_executor(Arc::new(move |command, full_access, cancel| {
            assert_eq!(command, expected_command);
            assert!(activated.load(Ordering::SeqCst));
            assert!(!cancel.is_cancelled());
            let mut calls = executions.lock().unwrap();
            assert!(calls.is_empty());
            calls.push((command, full_access));
            assert!(!result.exists());
            fs::write(&result, MOCK_OUTPUT).unwrap();
            Box::pin(async {
                Ok(vega_tools::BashOutput {
                    text: MOCK_OUTPUT.into(),
                    exit_code: 0,
                    duration_ms: 1,
                    truncated: false,
                })
            })
        }))
}

struct RecordingScriptPermission {
    requests: Arc<Mutex<Vec<PermissionRequest>>>,
    decision: PermissionDecision,
}

impl PermissionHook for RecordingScriptPermission {
    fn request(
        &self,
        request: PermissionRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<PermissionDecision, VegaError>> {
        self.requests.lock().unwrap().push(request);
        let decision = self.decision.clone();
        Box::pin(async move { Ok(decision) })
    }
}

async fn run_skill_task(
    store: &Store,
    provider: &MockProvider,
    tools: &vega_tools::Tools,
    hook: &RecordingScriptPermission,
    executions: ScriptDispatches,
    activated: Arc<AtomicBool>,
) -> ConversationRun {
    let run = run_thread_task_with_images_and_reasoning(
        store,
        provider,
        tools,
        "thread-1",
        USER_TASK,
        "System",
        CancellationToken::new(),
        hook,
        move |event| {
            if matches!(event, ConversationEvent::SkillActivated { .. }) {
                assert!(executions.lock().unwrap().is_empty());
                activated.store(true, Ordering::SeqCst);
            }
            Ok(())
        },
        PersistenceActorConfig::default(),
        None,
        None,
        None,
        Vec::new(),
    )
    .await
    .unwrap();
    assert!(!run.failed);
    assert!(!run.interrupted);
    run
}

fn ordinary_audit(source: ApprovalSource, decision: Approval) -> ApprovalAudit {
    ApprovalAudit {
        decision,
        note: None,
        source,
        danger: None,
    }
}

fn assert_content_free(value: &str) {
    assert!(!value.contains(SKILL_BODY_MARKER));
    assert!(!value.contains("S16_DURABLE_PRIVATE_SCRIPT"));
    assert!(!value.contains("allowed-tools: Bash(*)"));
}

fn assert_bash_state(
    state: &tool_calls::ToolCallState,
    command: &str,
    status: ToolCallStatus,
    approval: &ApprovalAudit,
) {
    assert_eq!(state.thread_id, "thread-1");
    assert_eq!(state.tool, "bash");
    assert_eq!(
        vega_tools::bash_permission_signature(&state.input_json).unwrap(),
        command
    );
    assert_eq!(ToolCallStatus::parse(&state.status), Some(status));
    assert_eq!(
        ApprovalAudit::from_json(state.approval.as_deref().unwrap()).unwrap(),
        *approval
    );
    assert_content_free(&state.input_json);
    assert_content_free(state.approval.as_deref().unwrap());
    assert_content_free(state.output_text.as_deref().unwrap());
    assert!(state.output_full_path.is_none());
    if status == ToolCallStatus::Success {
        assert_eq!(state.status, "success");
        assert_eq!(state.output_text.as_deref(), Some(MOCK_OUTPUT));
        assert_eq!(state.exit_code, Some(0));
        assert_eq!(state.duration_ms, Some(1));
    } else {
        assert_eq!(state.status, "rejected");
        let output = if approval.source == ApprovalSource::RunMode {
            "Tool error: denied by run mode"
        } else {
            "Tool error: permission denied"
        };
        assert_eq!(state.output_text.as_deref(), Some(output));
        assert!(state.exit_code.is_none());
        assert!(state.duration_ms.is_none());
    }
}

fn assert_reopened_task(
    store: &Store,
    run: &ConversationRun,
    hash: &str,
    scope: SkillHistorySource,
    origin: SkillHistoryOrigin,
) -> HistoryPage {
    let user = messages::find(store.conn(), &run.user_message_id)
        .unwrap()
        .unwrap();
    assert_eq!(user.thread_id, "thread-1");
    assert_eq!(user.role, "user");
    assert_eq!(user.content, USER_TASK);
    let audits = skills::list_activation_audits(store.conn(), "thread-1").unwrap();
    assert_eq!(audits.len(), 1);
    let audit = &audits[0];
    assert_eq!(audit.run_id, run.assistant_message_id);
    assert_eq!(audit.thread_id, "thread-1");
    assert_eq!(audit.name, "runner");
    assert_eq!(audit.content_sha256.as_deref(), Some(hash));
    assert_eq!(audit.status, "loaded");
    assert_eq!(
        audit.source_scope.as_deref(),
        Some(match scope {
            SkillHistorySource::Project => "project",
            SkillHistorySource::Imported => "imported",
            SkillHistorySource::VegaGlobal => "vega_global",
        })
    );
    assert_eq!(
        audit.origin,
        match origin {
            SkillHistoryOrigin::ExplicitUser => "explicit_user",
            SkillHistoryOrigin::Model => "model",
        }
    );
    assert_content_free(&format!("{audits:?}"));
    let page = crate::history::restart_history_page(store, "thread-1", 20).unwrap();
    let users = page
        .entries
        .iter()
        .filter_map(|entry| match entry {
            HistoryEntry::UserText {
                message_id,
                content,
                ..
            } => Some((message_id.as_str(), content.as_str())),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(users, vec![(run.user_message_id.as_str(), USER_TASK)]);
    let activations = page
        .entries
        .iter()
        .filter_map(|entry| match entry {
            HistoryEntry::SkillActivation { activation, .. } => Some(activation),
            _ => None,
        })
        .collect::<Vec<_>>();
    let projected_owners = page
        .entries
        .iter()
        .map(|entry| match entry {
            HistoryEntry::AssistantText { message_id, .. } => {
                format!("AssistantText(owner={message_id})")
            }
            HistoryEntry::Plan { plan, .. } => format!("Plan(owner={})", plan.id),
            HistoryEntry::Tool {
                message_id,
                call_id,
                ..
            } => format!("Tool(owner={message_id}, call={call_id})"),
            HistoryEntry::SkillActivation { activation, .. } => {
                format!("SkillActivation(owner={})", activation.run_id)
            }
            _ => "Other".to_string(),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        activations.len(),
        1,
        "Skill history missing: run={}, mode={}, assistant kind={}, entries={projected_owners:?}",
        run.assistant_message_id,
        vega_store::threads::find(store.conn(), "thread-1")
            .unwrap()
            .unwrap()
            .mode,
        messages::find(store.conn(), &run.assistant_message_id)
            .unwrap()
            .unwrap()
            .kind
    );
    let activation = activations[0];
    assert_eq!(activation.run_id, run.assistant_message_id);
    assert_eq!(activation.name, "runner");
    assert_eq!(activation.source_scope, scope);
    assert_eq!(activation.origin, origin);
    assert_eq!(activation.content_sha256, hash);
    assert_eq!(activation.status, SkillHistoryStatus::Loaded);
    assert_eq!(activation.verification, SkillHistoryVerification::Verified);
    assert_content_free(&format!("{page:?}"));
    page
}

fn assert_bash_history(
    page: &HistoryPage,
    run: &ConversationRun,
    call_id: &str,
    command: &str,
    state: &tool_calls::ToolCallState,
    approval: &ApprovalAudit,
) {
    let cards = page
        .entries
        .iter()
        .filter_map(|entry| match entry {
            HistoryEntry::Tool {
                message_id,
                call_id: id,
                status,
                approval,
                input,
                result,
                ..
            } if id == call_id => Some((message_id, status, approval, input, result)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(cards.len(), 1);
    let (message_id, status, projected_approval, input, result) = cards[0];
    assert_eq!(message_id, &run.assistant_message_id);
    assert_eq!(Some(*status), ToolCallStatus::parse(&state.status));
    assert_eq!(*projected_approval, Some(approval.decision));
    assert_eq!(
        *input,
        Some(ToolCardInputProjection::Bash {
            command: command.into()
        })
    );
    match result {
        Some(ToolCardResultProjection::Bash {
            status: result_status,
            output,
            exit_code,
            duration_ms,
            ..
        }) => {
            assert_eq!(result_status, status);
            assert_eq!(Some(output.as_str()), state.output_text.as_deref());
            assert_eq!(*exit_code, state.exit_code);
            assert_eq!(
                *duration_ms,
                state
                    .duration_ms
                    .map(|duration| u64::try_from(duration).unwrap())
            );
        }
        _ => panic!("expected a durable Bash result projection"),
    }
}

fn assert_preserved_plan(page: &HistoryPage, assistant: &vega_store::messages::MessageRow) {
    let plans = page
        .entries
        .iter()
        .filter_map(|entry| match entry {
            HistoryEntry::Plan { plan, .. } if plan.id == assistant.id => Some(plan),
            _ => None,
        })
        .collect::<Vec<_>>();
    if assistant.kind == "plan" {
        assert_eq!(plans.len(), 1);
        let plan = plans[0];
        assert_eq!(plan.thread_id, assistant.thread_id);
        assert_eq!(plan.content, assistant.content);
        assert_eq!(
            Some(plan.status),
            PlanStatus::parse(assistant.plan_status.as_deref().unwrap())
        );
        assert_eq!(plan.review_note, assistant.plan_review_note);
        assert_eq!(plan.reviewed_at, assistant.plan_reviewed_at);
        assert!(!page.entries.iter().any(|entry| matches!(
            entry,
            HistoryEntry::AssistantText { message_id, .. } if message_id == &assistant.id
        )));
    } else {
        assert!(plans.is_empty());
    }
}

#[tokio::test]
async fn issue87_s16_durable_pinned_activation_does_not_execute_a_script() {
    for mode in [ThreadMode::Execute, ThreadMode::Plan] {
        let (store, project, project_id) = setup();
        let hash = pin_allowed_tools_skill(&store, project.path(), &project_id);
        assert_eq!(
            vega_store::threads::set_mode(store.conn(), "thread-1", mode.as_str(), 4).unwrap(),
            1
        );
        assert_eq!(
            vega_store::threads::set_permission_mode(store.conn(), "thread-1", "full_access", 5)
                .unwrap(),
            1
        );
        let executions = Arc::new(Mutex::new(Vec::new()));
        let activated = Arc::new(AtomicBool::new(false));
        let tools = script_tools(
            project.path(),
            SCRIPT_COMMAND,
            executions.clone(),
            activated.clone(),
        );
        let provider = script_provider("s16-durable-idle", None, false);
        let requests = Arc::new(Mutex::new(Vec::new()));
        let hook = RecordingScriptPermission {
            requests: requests.clone(),
            decision: PermissionDecision::Once,
        };
        let run = run_skill_task(
            &store,
            &provider,
            &tools,
            &hook,
            executions.clone(),
            activated.clone(),
        )
        .await;
        assert!(activated.load(Ordering::SeqCst));
        assert_eq!(provider.requests().len(), 1);
        assert!(
            provider.requests()[0].messages[0]
                .content
                .contains(SKILL_BODY_MARKER)
        );
        assert!(executions.lock().unwrap().is_empty());
        assert!(requests.lock().unwrap().is_empty());
        assert!(!project.path().join("s16-result.txt").exists());
        assert_eq!(
            fs::read_to_string(project.path().join("scripts/harmless.sh")).unwrap(),
            SCRIPT_BYTES
        );
        assert!(
            !run.events
                .iter()
                .any(|event| matches!(event, ConversationEvent::ToolCallProposed { .. }))
        );
        assert!(
            tool_calls::find_state(store.conn(), "s16-durable-idle")
                .unwrap()
                .is_none()
        );
        let original_audits = skills::list_activation_audits(store.conn(), "thread-1").unwrap();
        let original_assistant = messages::find(store.conn(), &run.assistant_message_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            original_assistant.kind,
            if mode == ThreadMode::Plan {
                "plan"
            } else {
                "text"
            }
        );
        let skill_file = project.path().join(".agents/skills/runner/SKILL.md");
        fs::remove_file(&skill_file).unwrap();
        assert!(
            permissions::list_exact(store.conn(), &project_id)
                .unwrap()
                .is_empty()
        );
        let database = store.database_path().unwrap().to_path_buf();
        drop(store);
        let reopened = Store::open(database).unwrap();
        assert_eq!(
            messages::find(reopened.conn(), &run.assistant_message_id)
                .unwrap()
                .unwrap(),
            original_assistant
        );
        assert_eq!(
            skills::list_activation_audits(reopened.conn(), "thread-1").unwrap(),
            original_audits
        );
        let page = assert_reopened_task(
            &reopened,
            &run,
            &hash,
            SkillHistorySource::Project,
            SkillHistoryOrigin::ExplicitUser,
        );
        assert_preserved_plan(&page, &original_assistant);
        assert!(
            !page
                .entries
                .iter()
                .any(|entry| matches!(entry, HistoryEntry::Tool { .. }))
        );
        assert!(
            tool_calls::find_state(reopened.conn(), "s16-durable-idle")
                .unwrap()
                .is_none()
        );
        let canonical_root = fs::canonicalize(project.path()).unwrap();
        assert!(!format!("{page:?}").contains(canonical_root.to_str().unwrap()));
        assert!(!format!("{original_audits:?}").contains(canonical_root.to_str().unwrap()));
        assert!(executions.lock().unwrap().is_empty());
        assert_eq!(provider.requests().len(), 1);
        assert!(!skill_file.exists());
        assert!(
            permissions::list_exact(reopened.conn(), &project_id)
                .unwrap()
                .is_empty()
        );
    }
}

#[tokio::test]
async fn issue87_s16_durable_pinned_bash_authority_survives_store_reopen() {
    let danger_audit = ApprovalAudit {
        decision: Approval::Deny,
        note: None,
        source: ApprovalSource::Danger,
        danger: Some(DangerAudit {
            rule_id: "git-force-push".into(),
            decision: Approval::Deny,
            note: None,
        }),
    };
    for (mode, permission, decision, command, audit, prompt_count, dispatches) in [
        (
            ThreadMode::Ask,
            PermissionMode::FullAccess,
            PermissionDecision::Once,
            SCRIPT_COMMAND,
            ordinary_audit(ApprovalSource::RunMode, Approval::Deny),
            0,
            0,
        ),
        (
            ThreadMode::Plan,
            PermissionMode::FullAccess,
            PermissionDecision::Once,
            SCRIPT_COMMAND,
            ordinary_audit(ApprovalSource::RunMode, Approval::Deny),
            0,
            0,
        ),
        (
            ThreadMode::Execute,
            PermissionMode::ReadOnly,
            PermissionDecision::Once,
            SCRIPT_COMMAND,
            ordinary_audit(ApprovalSource::ReadOnly, Approval::Deny),
            0,
            0,
        ),
        (
            ThreadMode::Execute,
            PermissionMode::Confirm,
            PermissionDecision::Deny { note: None },
            SCRIPT_COMMAND,
            ordinary_audit(ApprovalSource::User, Approval::Deny),
            1,
            0,
        ),
        (
            ThreadMode::Execute,
            PermissionMode::Confirm,
            PermissionDecision::Once,
            SCRIPT_COMMAND,
            ordinary_audit(ApprovalSource::User, Approval::Once),
            1,
            1,
        ),
        (
            ThreadMode::Execute,
            PermissionMode::Auto,
            PermissionDecision::Deny { note: None },
            SCRIPT_COMMAND,
            ordinary_audit(ApprovalSource::Auto, Approval::Once),
            0,
            1,
        ),
        (
            ThreadMode::Execute,
            PermissionMode::FullAccess,
            PermissionDecision::Deny { note: None },
            SCRIPT_COMMAND,
            ordinary_audit(ApprovalSource::FullAccess, Approval::Once),
            0,
            1,
        ),
        (
            ThreadMode::Execute,
            PermissionMode::FullAccess,
            PermissionDecision::Deny { note: None },
            DANGEROUS_COMMAND,
            danger_audit,
            1,
            0,
        ),
    ] {
        let (store, project, project_id) = setup();
        let hash = pin_allowed_tools_skill(&store, project.path(), &project_id);
        assert_eq!(
            vega_store::threads::set_mode(store.conn(), "thread-1", mode.as_str(), 4).unwrap(),
            1
        );
        assert_eq!(
            vega_store::threads::set_permission_mode(
                store.conn(),
                "thread-1",
                permission.as_str(),
                5
            )
            .unwrap(),
            1
        );
        let executions = Arc::new(Mutex::new(Vec::new()));
        let activated = Arc::new(AtomicBool::new(false));
        let tools = script_tools(
            project.path(),
            command,
            executions.clone(),
            activated.clone(),
        );
        let call_id = "s16-durable-pinned-bash";
        let provider = script_provider(call_id, Some(command), false);
        let requests = Arc::new(Mutex::new(Vec::new()));
        let hook = RecordingScriptPermission {
            requests: requests.clone(),
            decision,
        };
        let run = run_skill_task(
            &store,
            &provider,
            &tools,
            &hook,
            executions.clone(),
            activated.clone(),
        )
        .await;
        assert!(activated.load(Ordering::SeqCst));
        assert_eq!(provider.requests().len(), 2);
        assert!(
            provider.requests()[0].messages[0]
                .content
                .contains(SKILL_BODY_MARKER)
        );
        let expected_status = if dispatches == 1 {
            ToolCallStatus::Success
        } else {
            ToolCallStatus::Rejected
        };
        let observed = executions.lock().unwrap().clone();
        assert_eq!(observed.len(), dispatches);
        if dispatches == 1 {
            assert_eq!(
                observed[0],
                (
                    command.to_string(),
                    permission == PermissionMode::FullAccess
                )
            );
        }
        assert_eq!(
            project.path().join("s16-result.txt").exists(),
            dispatches == 1
        );
        assert_eq!(
            fs::read_to_string(project.path().join("scripts/harmless.sh")).unwrap(),
            SCRIPT_BYTES
        );
        let prompts = requests.lock().unwrap().clone();
        assert_eq!(prompts.len(), prompt_count);
        for prompt in &prompts {
            assert_eq!(prompt.call_id, call_id);
            assert_eq!(prompt.tool, "bash");
            assert_eq!(prompt.display_target, command);
            assert!(prompt.external.is_none());
            assert_eq!(
                prompt.danger_rule_id.as_deref(),
                audit.danger.as_ref().map(|danger| danger.rule_id.as_str())
            );
        }
        let finished = run
            .events
            .iter()
            .filter_map(|event| match event {
                ConversationEvent::ToolCallFinished {
                    call_id: id,
                    result,
                } if id == call_id => Some(result),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(finished.len(), 1);
        assert_eq!(finished[0].status, expected_status);
        let persisted = tool_calls::find_state(store.conn(), call_id)
            .unwrap()
            .unwrap();
        assert_bash_state(&persisted, command, expected_status, &audit);
        assert_eq!(
            finished[0].output,
            persisted.output_text.as_deref().unwrap()
        );
        assert!(
            permissions::list_exact(store.conn(), &project_id)
                .unwrap()
                .is_empty()
        );
        let original_audits = skills::list_activation_audits(store.conn(), "thread-1").unwrap();
        let original_assistant = messages::find(store.conn(), &run.assistant_message_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            original_assistant.kind,
            if mode == ThreadMode::Plan {
                "plan"
            } else {
                "text"
            }
        );
        let skill_file = project.path().join(".agents/skills/runner/SKILL.md");
        fs::remove_file(&skill_file).unwrap();
        let database = store.database_path().unwrap().to_path_buf();
        drop(store);
        let reopened = Store::open(database).unwrap();
        assert_eq!(
            messages::find(reopened.conn(), &run.assistant_message_id)
                .unwrap()
                .unwrap(),
            original_assistant
        );
        assert_eq!(
            tool_calls::find_state(reopened.conn(), call_id)
                .unwrap()
                .unwrap(),
            persisted
        );
        assert_eq!(
            skills::list_activation_audits(reopened.conn(), "thread-1").unwrap(),
            original_audits
        );
        let page = assert_reopened_task(
            &reopened,
            &run,
            &hash,
            SkillHistorySource::Project,
            SkillHistoryOrigin::ExplicitUser,
        );
        assert_preserved_plan(&page, &original_assistant);
        assert_bash_history(&page, &run, call_id, command, &persisted, &audit);
        assert_eq!(
            page.entries
                .iter()
                .filter(|entry| matches!(entry, HistoryEntry::Tool { .. }))
                .count(),
            1
        );
        assert!(
            permissions::list_exact(reopened.conn(), &project_id)
                .unwrap()
                .is_empty()
        );
        let canonical_root = fs::canonicalize(project.path()).unwrap();
        assert!(!format!("{original_audits:?}").contains(canonical_root.to_str().unwrap()));
        let provenance = page
            .entries
            .iter()
            .find_map(|entry| match entry {
                HistoryEntry::SkillActivation { activation, .. } => Some(activation),
                _ => None,
            })
            .unwrap();
        assert!(!format!("{provenance:?}").contains(canonical_root.to_str().unwrap()));
        assert_eq!(*executions.lock().unwrap(), observed);
        assert_eq!(provider.requests().len(), 2);
        assert!(!skill_file.exists());
    }
}

#[tokio::test]
async fn issue87_s16_durable_imported_script_rejection_and_receipt_survive_reopen() {
    let (store, project, project_id) = setup();
    let imported = tempdir().unwrap();
    let script = imported.path().join("runner/scripts/harmless.sh");
    fs::create_dir_all(script.parent().unwrap()).unwrap();
    fs::write(&script, SCRIPT_BYTES).unwrap();
    assert!(!script.starts_with(project.path()));
    let command = format!("sh {}", script.display());
    write_allowed_tools_skill(imported.path(), &command);
    let source = SkillSource::imported_approved(imported.path(), 0).unwrap();
    assert_allowed_tools_source(&source);
    let (_, hash) = approve(
        &store,
        &source,
        "source-s16-imported",
        None,
        imported.path(),
        true,
    );
    let settings = skills::read_settings(store.conn()).unwrap();
    skills::set_global_settings(store.conn(), settings.consent_generation, true, true).unwrap();
    assert_eq!(
        vega_store::threads::set_permission_mode(store.conn(), "thread-1", "readonly", 4).unwrap(),
        1
    );
    let executions = Arc::new(Mutex::new(Vec::new()));
    let activated = Arc::new(AtomicBool::new(false));
    let tools = script_tools(
        project.path(),
        &command,
        executions.clone(),
        activated.clone(),
    );
    let call_id = "s16-durable-imported-bash";
    let provider = script_provider(call_id, Some(&command), true);
    let requests = Arc::new(Mutex::new(Vec::new()));
    let hook = RecordingScriptPermission {
        requests: requests.clone(),
        decision: PermissionDecision::Once,
    };
    let run = run_skill_task(
        &store,
        &provider,
        &tools,
        &hook,
        executions.clone(),
        activated.clone(),
    )
    .await;
    assert!(activated.load(Ordering::SeqCst));
    assert_eq!(provider.requests().len(), 3);
    assert!(
        !provider.requests()[0].messages[0]
            .content
            .contains(SKILL_BODY_MARKER)
    );
    assert!(
        provider.requests()[1].messages[0]
            .content
            .contains(SKILL_BODY_MARKER)
    );
    assert!(
        provider.requests()[1].messages[0]
            .content
            .contains(&command)
    );
    assert!(executions.lock().unwrap().is_empty());
    assert!(requests.lock().unwrap().is_empty());
    assert!(!project.path().join("s16-result.txt").exists());
    assert_eq!(fs::read_to_string(&script).unwrap(), SCRIPT_BYTES);
    let audit = ordinary_audit(ApprovalSource::ReadOnly, Approval::Deny);
    let persisted = tool_calls::find_state(store.conn(), call_id)
        .unwrap()
        .unwrap();
    assert_bash_state(&persisted, &command, ToolCallStatus::Rejected, &audit);
    let load = tool_calls::find_state(store.conn(), LOAD_CALL_ID)
        .unwrap()
        .unwrap();
    assert_eq!(load.tool, "load_skill");
    assert_eq!(load.status, "success");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(load.output_text.as_deref().unwrap()).unwrap(),
        serde_json::json!({"name":"runner", "status":"loaded"})
    );
    assert_eq!(
        ApprovalAudit::from_json(load.approval.as_deref().unwrap()).unwrap(),
        ordinary_audit(ApprovalSource::ReadonlyTool, Approval::Once)
    );
    assert_content_free(&load.input_json);
    assert_content_free(load.output_text.as_deref().unwrap());
    let canonical_imported = fs::canonicalize(imported.path()).unwrap();
    assert!(
        !load
            .output_text
            .as_ref()
            .unwrap()
            .contains(canonical_imported.to_str().unwrap())
    );
    assert!(
        permissions::list_exact(store.conn(), &project_id)
            .unwrap()
            .is_empty()
    );
    let original_audits = skills::list_activation_audits(store.conn(), "thread-1").unwrap();
    assert!(!format!("{original_audits:?}").contains(canonical_imported.to_str().unwrap()));
    let skill_file = imported.path().join("runner/SKILL.md");
    fs::remove_file(&skill_file).unwrap();
    let database = store.database_path().unwrap().to_path_buf();
    drop(store);
    let reopened = Store::open(database).unwrap();
    assert_eq!(
        tool_calls::find_state(reopened.conn(), call_id)
            .unwrap()
            .unwrap(),
        persisted
    );
    assert_eq!(
        tool_calls::find_state(reopened.conn(), LOAD_CALL_ID)
            .unwrap()
            .unwrap(),
        load
    );
    assert_eq!(
        skills::list_activation_audits(reopened.conn(), "thread-1").unwrap(),
        original_audits
    );
    let page = assert_reopened_task(
        &reopened,
        &run,
        &hash,
        SkillHistorySource::Imported,
        SkillHistoryOrigin::Model,
    );
    assert_bash_history(&page, &run, call_id, &command, &persisted, &audit);
    let load_cards = page
        .entries
        .iter()
        .filter_map(|entry| match entry {
            HistoryEntry::Tool {
                call_id,
                status,
                approval,
                input,
                result,
                ..
            } if call_id == LOAD_CALL_ID => Some((status, approval, input, result)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(load_cards.len(), 1);
    let (status, approval, input, result) = load_cards[0];
    assert_eq!(*status, ToolCallStatus::Success);
    assert_eq!(*approval, Some(Approval::Once));
    assert_eq!(
        *input,
        Some(ToolCardInputProjection::Skill {
            kind: SkillToolKind::Load,
            name: Some("runner".into()),
            path_bytes: None,
            path_sha256: None
        })
    );
    assert!(matches!(
        result,
        Some(ToolCardResultProjection::Skill {
            status: ToolCallStatus::Success,
            outcome: SkillCardOutcome::Loaded,
            ..
        })
    ));
    assert_eq!(
        page.entries
            .iter()
            .filter(|entry| matches!(entry, HistoryEntry::Tool { .. }))
            .count(),
        2
    );
    let provenance = page
        .entries
        .iter()
        .find_map(|entry| match entry {
            HistoryEntry::SkillActivation { activation, .. } => Some(activation),
            _ => None,
        })
        .unwrap();
    assert!(!format!("{provenance:?}").contains(canonical_imported.to_str().unwrap()));
    assert!(
        permissions::list_exact(reopened.conn(), &project_id)
            .unwrap()
            .is_empty()
    );
    assert!(executions.lock().unwrap().is_empty());
    assert_eq!(provider.requests().len(), 3);
    assert!(!skill_file.exists());
    assert_eq!(fs::read_to_string(&script).unwrap(), SCRIPT_BYTES);
}
