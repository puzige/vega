use super::*;
use crate::skills::{
    ActivationOrigin, SkillApproval, SkillCatalog, SkillRun, SkillSource, SourceScope,
};
use std::sync::atomic::AtomicBool;

const SCRIPT_COMMAND: &str = "sh ./s16-sentinel.sh";
const DANGEROUS_SCRIPT_COMMAND: &str = "sh ./s16-sentinel.sh && git push --force";
const SCRIPT_BYTES: &str = "printf 'PRIVATE S16 SCRIPT BODY' > s16-sentinel-result.txt\n";
const SKILL_BODY_MARKER: &str = "PRIVATE S16 GUIDANCE";

struct ScriptCase {
    run_mode: RuntimeRunMode,
    permission_mode: RuntimePermissionMode,
    command: Option<&'static str>,
    decision: RuntimeUserDecision,
    stop_before_dispatch: bool,
    stop_during_execution: bool,
    hardlinked_script: bool,
}

impl ScriptCase {
    fn new(run_mode: RuntimeRunMode, permission_mode: RuntimePermissionMode) -> Self {
        Self {
            run_mode,
            permission_mode,
            command: Some(SCRIPT_COMMAND),
            decision: RuntimeUserDecision::Once,
            stop_before_dispatch: false,
            stop_during_execution: false,
            hardlinked_script: false,
        }
    }
}

struct ScriptObservation {
    outcome: AgentOutcome,
    prompts: Vec<RuntimePermissionPrompt>,
    dispatches: usize,
}

struct RecordingScriptPermission {
    prompts: Arc<Mutex<Vec<RuntimePermissionPrompt>>>,
    decision: RuntimeUserDecision,
}

impl RuntimePermissionHook for RecordingScriptPermission {
    fn request(
        &self,
        prompt: RuntimePermissionPrompt,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<RuntimeUserDecision, VegaError>> {
        self.prompts.lock().unwrap().push(prompt);
        let decision = self.decision.clone();
        Box::pin(async move { Ok(decision) })
    }
}

async fn run_script_skill_case(case: ScriptCase) -> ScriptObservation {
    let project = tempdir().unwrap();
    let skill_root = project.path().join(".agents/skills/reviewer");
    fs::create_dir_all(&skill_root).unwrap();
    let guidance_command = case.command.unwrap_or(SCRIPT_COMMAND);
    fs::write(
        skill_root.join("SKILL.md"),
        format!(
            "---\nname: reviewer\ndescription: Run the owned sentinel script.\nallowed-tools: Bash(*)\n---\n{SKILL_BODY_MARKER}: use bash to run `{guidance_command}`.\n"
        ),
    )
    .unwrap();
    let source = SkillSource::project_approved(project.path())
        .unwrap()
        .unwrap();
    let candidate = source.discover().unwrap().candidates.remove(0);
    let document =
        crate::skills::parse_skill_md(&source.load_candidate(&candidate).unwrap(), &candidate.name)
            .unwrap();
    assert_eq!(document.metadata.allowed_tools.as_deref(), Some("Bash(*)"));
    let approved_hash = candidate.sha256.clone();
    let canonical_project_root = fs::canonicalize(project.path())
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let approval = SkillApproval::reviewed(&candidate, "project-approved", true, true).unwrap();
    let run = SkillRun::new(
        SkillCatalog::freeze(vec![candidate], &[approval], true).unwrap(),
        true,
    );
    let script = project.path().join("s16-sentinel.sh");
    fs::write(&script, SCRIPT_BYTES).unwrap();
    if case.hardlinked_script {
        fs::hard_link(&script, project.path().join(".script-hardlink")).unwrap();
    }
    let sentinel_result = project.path().join("s16-sentinel-result.txt");
    let dispatches = Arc::new(AtomicUsize::new(0));
    let load_finished = Arc::new(AtomicBool::new(false));
    let execution_completed = Arc::new(AtomicUsize::new(0));
    let recorded_dispatches = dispatches.clone();
    let observed_load_finished = load_finished.clone();
    let observed_result = sentinel_result.clone();
    let observed_completion = execution_completed.clone();
    let (entered, started_execution) = tokio::sync::oneshot::channel();
    let entered = Arc::new(Mutex::new(Some(entered)));
    let tools = vega_tools::Tools::new(project.path())
        .unwrap()
        .with_bash_test_executor(Arc::new(move |command, full_access, cancel| {
            assert_eq!(Some(command.as_str()), case.command);
            assert_eq!(
                full_access,
                case.permission_mode == RuntimePermissionMode::FullAccess
            );
            assert!(observed_load_finished.load(Ordering::SeqCst));
            assert!(!cancel.is_cancelled());
            assert_eq!(recorded_dispatches.fetch_add(1, Ordering::SeqCst), 0);
            assert!(!observed_result.exists());
            if case.stop_during_execution {
                entered.lock().unwrap().take().unwrap().send(()).unwrap();
                let completed = observed_completion.clone();
                Box::pin(async move {
                    cancel.cancelled().await;
                    tokio::task::yield_now().await;
                    completed.fetch_add(1, Ordering::SeqCst);
                    Err(vega_tools::BashError::for_test(
                        vega_tools::BashErrorCode::Cancelled,
                    ))
                })
            } else {
                fs::write(&observed_result, "sentinel-ran").unwrap();
                Box::pin(async {
                    Ok(vega_tools::BashOutput {
                        text: "sentinel-ran".into(),
                        exit_code: 0,
                        duration_ms: 1,
                        truncated: false,
                    })
                })
            }
        }));
    let mut rounds = vec![vec![ScriptStep::events(vec![
        ProviderEvent::ToolUse {
            id: "s16-load".into(),
            name: "load_skill".into(),
            input_json: r#"{"name":"reviewer"}"#.into(),
        },
        ProviderEvent::Done {
            stop_reason: StopReason::ToolUse,
        },
    ])]];
    if let Some(command) = case.command {
        rounds.push(vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "s16-bash".into(),
                name: "bash".into(),
                input_json: serde_json::json!({"cmd": command, "timeout_ms": null}).to_string(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])]);
    }
    rounds.push(vec![ScriptStep::events(vec![ProviderEvent::Done {
        stop_reason: StopReason::End,
    }])]);
    let provider = MockProvider::new_rounds(rounds);
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let hook = RecordingScriptPermission {
        prompts: prompts.clone(),
        decision: case.decision,
    };
    let user_task = "Follow the approved Skill for this owned task.";
    let mut req = request(vec![ChatMessage::new(ChatRole::User, user_task)]);
    req.tool_config = tool_config(
        case.run_mode,
        case.permission_mode,
        project.path().join("checkpoints"),
    )
    .with_skill_run(run, Vec::new());
    let cancel = CancellationToken::new();
    let canceller = if case.stop_during_execution {
        let execution_cancel = cancel.clone();
        Some(tokio::spawn(async move {
            tokio::time::timeout(Duration::from_secs(2), started_execution)
                .await
                .unwrap()
                .unwrap();
            execution_cancel.cancel();
        }))
    } else {
        None
    };
    let sink_cancel = cancel.clone();
    let activation_dispatches = dispatches.clone();
    let activation_result = sentinel_result.clone();
    let recorded_load_finished = load_finished.clone();
    let outcome =
        run_agent_with_permission_sink(&provider, &tools, req, cancel, &hook, move |event| {
            match &event {
                RuntimeEvent::SkillActivation { audit, .. } => {
                    assert_eq!(audit.name, "reviewer");
                    assert_eq!(audit.status, "loaded");
                    assert_eq!(audit.origin, ActivationOrigin::Model);
                    assert_eq!(audit.source_scope, Some(SourceScope::Project));
                    assert_eq!(
                        audit.content_sha256.as_deref(),
                        Some(approved_hash.as_str())
                    );
                    assert!(audit.source_label.is_some());
                    assert_eq!(activation_dispatches.load(Ordering::SeqCst), 0);
                    assert!(!activation_result.exists());
                    let audit_json = serde_json::to_string(audit).unwrap();
                    assert!(!audit_json.contains(SKILL_BODY_MARKER));
                    assert!(!audit_json.contains("PRIVATE S16 SCRIPT BODY"));
                    assert!(!audit_json.contains(guidance_command));
                    assert!(!audit_json.contains(&canonical_project_root));
                }
                RuntimeEvent::ToolCallFinished(result) if result.call_id == "s16-load" => {
                    assert_eq!(result.status, RuntimeToolStatus::Success);
                    assert_eq!(activation_dispatches.load(Ordering::SeqCst), 0);
                    assert!(!activation_result.exists());
                    recorded_load_finished.store(true, Ordering::SeqCst);
                }
                RuntimeEvent::ToolCallProposed(call)
                    if call.id == "s16-bash" && case.stop_before_dispatch =>
                {
                    sink_cancel.cancel();
                }
                _ => {}
            }
            async { Ok(()) }
        })
        .await
        .unwrap();
    if let Some(canceller) = canceller {
        canceller.await.unwrap();
    }
    assert!(!outcome.failed);
    assert_eq!(
        outcome.interrupted,
        case.stop_before_dispatch || case.stop_during_execution
    );
    assert_eq!(
        execution_completed.load(Ordering::SeqCst),
        usize::from(case.stop_during_execution)
    );
    assert!(load_finished.load(Ordering::SeqCst));
    assert_eq!(fs::read_to_string(&script).unwrap(), SCRIPT_BYTES);
    let dispatch_count = dispatches.load(Ordering::SeqCst);
    assert_eq!(
        sentinel_result.exists(),
        dispatch_count == 1 && !case.stop_during_execution
    );
    if sentinel_result.exists() {
        assert_eq!(
            fs::read_to_string(&sentinel_result).unwrap(),
            "sentinel-ran"
        );
    }
    assert!(!project.path().join("checkpoints").exists());
    let requests = provider.requests();
    let expected_request_count = if case.command.is_some() && !outcome.interrupted {
        3
    } else {
        2
    };
    assert_eq!(requests.len(), expected_request_count);
    assert!(!requests[0].messages[0].content.contains(SKILL_BODY_MARKER));
    assert!(requests[1].messages[0].content.contains(SKILL_BODY_MARKER));
    assert!(requests[1].messages[0].content.contains(guidance_command));
    for provider_request in &requests {
        let native_definitions = provider_request
            .tools
            .iter()
            .filter(|tool| !matches!(tool.name.as_str(), "load_skill" | "read_skill_resource"))
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(native_definitions, tool_definitions(case.run_mode));
        assert_eq!(provider_request.tools.len(), native_definitions.len() + 2);
        assert!(
            provider_request
                .tools
                .iter()
                .filter(|tool| {
                    matches!(tool.name.as_str(), "load_skill" | "read_skill_resource")
                })
                .all(|tool| tool.strict)
        );
        assert_eq!(
            provider_request
                .tools
                .iter()
                .any(|tool| tool.name == "bash"),
            case.run_mode == RuntimeRunMode::Execute
        );
        assert!(provider_request.messages.iter().all(|message| {
            !message.content.contains("PRIVATE S16 SCRIPT BODY")
                && !message.content.contains("allowed-tools: Bash(*)")
        }));
    }
    assert_eq!(
        outcome
            .events
            .iter()
            .filter(|event| matches!(event, RuntimeEvent::SkillActivation { .. }))
            .count(),
        1
    );
    let load_result = outcome
        .events
        .iter()
        .find_map(|event| match event {
            RuntimeEvent::ToolCallFinished(result) if result.call_id == "s16-load" => Some(result),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        load_result.approval,
        Some(expected_audit(
            RuntimeApprovalSource::ReadonlyTool,
            RuntimeApprovalDecision::Once,
            None
        ))
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&load_result.output).unwrap(),
        serde_json::json!({"name":"reviewer", "status":"loaded"})
    );
    assert!(load_result.remember_rule.is_none());
    for (call_id, output) in std::iter::once(("s16-load", load_result.output.as_str())).chain(
        outcome.events.iter().filter_map(|event| match event {
            RuntimeEvent::ToolCallFinished(result) if result.call_id == "s16-bash" => {
                Some(("s16-bash", result.output.as_str()))
            }
            _ => None,
        }),
    ) {
        assert_eq!(
            outcome
                .messages
                .iter()
                .filter(|message| message.role == ChatRole::Tool
                    && message.tool_call_id.as_deref() == Some(call_id)
                    && message.content == output)
                .count(),
            1
        );
        if !outcome.interrupted {
            assert_eq!(
                requests
                    .last()
                    .unwrap()
                    .messages
                    .iter()
                    .filter(|message| message.role == ChatRole::Tool
                        && message.tool_call_id.as_deref() == Some(call_id)
                        && message.content == output)
                    .count(),
                1
            );
        }
    }
    assert_eq!(
        outcome
            .messages
            .iter()
            .filter(|message| message.role == ChatRole::User && message.content == user_task)
            .count(),
        1
    );
    assert!(outcome.events.iter().all(|event| !matches!(event,
        RuntimeEvent::ToolCallFinished(result) if result.output.contains(SKILL_BODY_MARKER) || result.output.contains("PRIVATE S16 SCRIPT BODY")
    )));
    let prompts = prompts.lock().unwrap().clone();
    for prompt in &prompts {
        assert_eq!(prompt.target.call_id, "s16-bash");
        assert_eq!(prompt.target.tool, RuntimeMutatingTool::Bash);
        assert_eq!(Some(prompt.target.exact_pattern.as_str()), case.command);
        assert_eq!(prompt.target.display_target, prompt.target.exact_pattern);
    }
    ScriptObservation {
        outcome,
        prompts,
        dispatches: dispatch_count,
    }
}

fn expected_audit(
    source: RuntimeApprovalSource,
    decision: RuntimeApprovalDecision,
    danger: Option<crate::RuntimeDangerAudit>,
) -> RuntimeApprovalAudit {
    RuntimeApprovalAudit {
        decision,
        note: None,
        source,
        danger,
    }
}

fn assert_script_terminal(
    observed: &ScriptObservation,
    status: RuntimeToolStatus,
    approval: RuntimeApprovalAudit,
    running_calls: usize,
    dispatches: usize,
) {
    let results = observed
        .outcome
        .events
        .iter()
        .filter_map(|event| match event {
            RuntimeEvent::ToolCallFinished(result) if result.call_id == "s16-bash" => Some(result),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(results.len(), 1);
    let result = results[0];
    assert_eq!(result.status, status);
    assert_eq!(result.approval.as_ref(), Some(&approval));
    assert!(!result.reused);
    assert!(result.remember_rule.is_none());
    assert_eq!(observed.dispatches, dispatches);
    assert_eq!(observed.outcome.executed_tool_call_count, 1 + running_calls);
    assert_eq!(observed.outcome.events.iter().filter(|event| matches!(event, RuntimeEvent::ToolCallProposed(call) if call.id == "s16-bash")).count(), 1);
    let approvals = observed
        .outcome
        .events
        .iter()
        .filter_map(|event| match event {
            RuntimeEvent::ToolCallApproved {
                call_id,
                audit,
                remember_rule,
            } if call_id == "s16-bash" => {
                assert!(remember_rule.is_none());
                Some(audit)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(approvals.len(), running_calls);
    assert!(approvals.iter().all(|audit| **audit == approval));
    assert_eq!(observed.outcome.events.iter().filter(|event| matches!(event, RuntimeEvent::ToolCallRunning { call_id } if call_id == "s16-bash")).count(), running_calls);
    if status == RuntimeToolStatus::Success {
        assert_eq!(result.output, "sentinel-ran");
        assert_eq!(result.exit_code, Some(0));
        assert_eq!(result.duration_ms, Some(1));
        assert_eq!(result.truncated, Some(false));
    } else {
        assert!(result.exit_code.is_none());
        assert!(result.duration_ms.is_none());
        assert!(result.truncated.is_none());
    }
}

#[tokio::test]
async fn issue74_s16_activation_alone_never_executes_the_skill_script() {
    let mut case = ScriptCase::new(RuntimeRunMode::Execute, RuntimePermissionMode::FullAccess);
    case.command = None;
    let observed = run_script_skill_case(case).await;
    assert_eq!(observed.dispatches, 0);
    assert!(observed.prompts.is_empty());
    assert_eq!(observed.outcome.executed_tool_call_count, 1);
    assert!(
        !observed.outcome.events.iter().any(
            |event| matches!(event, RuntimeEvent::ToolCallProposed(call) if call.name == "bash")
        )
    );
}

#[tokio::test]
async fn issue74_s16_allowed_tools_cannot_add_bash_in_ask_or_plan() {
    for run_mode in [RuntimeRunMode::Ask, RuntimeRunMode::Plan] {
        let observed =
            run_script_skill_case(ScriptCase::new(run_mode, RuntimePermissionMode::FullAccess))
                .await;
        assert!(observed.prompts.is_empty());
        assert_script_terminal(
            &observed,
            RuntimeToolStatus::Rejected,
            expected_audit(
                RuntimeApprovalSource::RunMode,
                RuntimeApprovalDecision::Deny,
                None,
            ),
            0,
            0,
        );
    }
}

#[tokio::test]
async fn issue74_s16_execute_keeps_native_script_approval_and_sandbox_modes() {
    for (permission_mode, decision, prompt_count, status, source, dispatches) in [
        (
            RuntimePermissionMode::Confirm,
            RuntimeUserDecision::Once,
            1,
            RuntimeToolStatus::Success,
            RuntimeApprovalSource::User,
            1,
        ),
        (
            RuntimePermissionMode::Confirm,
            RuntimeUserDecision::Deny { note: None },
            1,
            RuntimeToolStatus::Rejected,
            RuntimeApprovalSource::User,
            0,
        ),
        (
            RuntimePermissionMode::Auto,
            RuntimeUserDecision::Deny { note: None },
            0,
            RuntimeToolStatus::Success,
            RuntimeApprovalSource::Auto,
            1,
        ),
        (
            RuntimePermissionMode::FullAccess,
            RuntimeUserDecision::Deny { note: None },
            0,
            RuntimeToolStatus::Success,
            RuntimeApprovalSource::FullAccess,
            1,
        ),
    ] {
        let mut case = ScriptCase::new(RuntimeRunMode::Execute, permission_mode);
        case.decision = decision;
        let observed = run_script_skill_case(case).await;
        assert_eq!(observed.prompts.len(), prompt_count);
        assert!(
            observed
                .prompts
                .iter()
                .all(|prompt| prompt.danger.is_none())
        );
        assert_script_terminal(
            &observed,
            status,
            expected_audit(
                source,
                if dispatches == 1 {
                    RuntimeApprovalDecision::Once
                } else {
                    RuntimeApprovalDecision::Deny
                },
                None,
            ),
            dispatches,
            dispatches,
        );
    }
}

#[tokio::test]
async fn issue74_s16_dangerous_skill_commands_keep_explicit_danger_approval() {
    for (permission_mode, decision, status, source, danger_decision, dispatches) in [
        (
            RuntimePermissionMode::Auto,
            RuntimeUserDecision::Once,
            RuntimeToolStatus::Success,
            RuntimeApprovalSource::Danger,
            RuntimeApprovalDecision::Once,
            1,
        ),
        (
            RuntimePermissionMode::Auto,
            RuntimeUserDecision::Deny { note: None },
            RuntimeToolStatus::Rejected,
            RuntimeApprovalSource::Danger,
            RuntimeApprovalDecision::Deny,
            0,
        ),
        (
            RuntimePermissionMode::FullAccess,
            RuntimeUserDecision::Once,
            RuntimeToolStatus::Success,
            RuntimeApprovalSource::Danger,
            RuntimeApprovalDecision::Once,
            1,
        ),
        (
            RuntimePermissionMode::FullAccess,
            RuntimeUserDecision::Deny { note: None },
            RuntimeToolStatus::Rejected,
            RuntimeApprovalSource::Danger,
            RuntimeApprovalDecision::Deny,
            0,
        ),
        (
            RuntimePermissionMode::ReadOnly,
            RuntimeUserDecision::Once,
            RuntimeToolStatus::Rejected,
            RuntimeApprovalSource::ReadOnly,
            RuntimeApprovalDecision::Once,
            0,
        ),
    ] {
        let mut case = ScriptCase::new(RuntimeRunMode::Execute, permission_mode);
        case.command = Some(DANGEROUS_SCRIPT_COMMAND);
        case.decision = decision;
        let observed = run_script_skill_case(case).await;
        assert_eq!(observed.prompts.len(), 1);
        let danger = observed.prompts[0].danger.as_ref().unwrap();
        assert_eq!(danger.rule_id, "git-force-push");
        assert_eq!(danger.reason, "forced git push can rewrite remote history");
        assert_script_terminal(
            &observed,
            status,
            expected_audit(
                source,
                if dispatches == 1 {
                    RuntimeApprovalDecision::Once
                } else {
                    RuntimeApprovalDecision::Deny
                },
                Some(crate::RuntimeDangerAudit {
                    rule_id: "git-force-push".into(),
                    decision: danger_decision,
                    note: None,
                }),
            ),
            dispatches,
            dispatches,
        );
    }
}

#[tokio::test]
async fn issue74_s16_stop_before_dispatch_keeps_script_rejection_and_history() {
    for (permission_mode, command, prompt_count, danger) in [
        (RuntimePermissionMode::Confirm, SCRIPT_COMMAND, 0, None),
        (
            RuntimePermissionMode::FullAccess,
            DANGEROUS_SCRIPT_COMMAND,
            1,
            Some(crate::RuntimeDangerAudit {
                rule_id: "git-force-push".into(),
                decision: RuntimeApprovalDecision::Deny,
                note: None,
            }),
        ),
    ] {
        let mut case = ScriptCase::new(RuntimeRunMode::Execute, permission_mode);
        case.command = Some(command);
        case.stop_before_dispatch = true;
        let observed = run_script_skill_case(case).await;
        assert_eq!(observed.prompts.len(), prompt_count);
        assert_script_terminal(
            &observed,
            RuntimeToolStatus::Rejected,
            expected_audit(
                RuntimeApprovalSource::Timeout,
                RuntimeApprovalDecision::Deny,
                danger,
            ),
            0,
            0,
        );
    }
}

#[tokio::test]
async fn issue74_s16_stop_during_script_waits_for_boundary_cancellation() {
    let mut case = ScriptCase::new(RuntimeRunMode::Execute, RuntimePermissionMode::Auto);
    case.stop_during_execution = true;
    let observed = run_script_skill_case(case).await;
    assert!(observed.prompts.is_empty());
    assert_script_terminal(
        &observed,
        RuntimeToolStatus::Cancelled,
        expected_audit(
            RuntimeApprovalSource::Auto,
            RuntimeApprovalDecision::Once,
            None,
        ),
        1,
        1,
    );
    let result = observed
        .outcome
        .events
        .iter()
        .find_map(|event| match event {
            RuntimeEvent::ToolCallFinished(result) if result.call_id == "s16-bash" => Some(result),
            _ => None,
        })
        .unwrap();
    assert_eq!(result.output, "Tool error: bash failed (cancelled)");
}

#[tokio::test]
async fn issue74_s16_skill_cannot_bypass_bash_sandbox_preflight() {
    let mut case = ScriptCase::new(RuntimeRunMode::Execute, RuntimePermissionMode::Auto);
    case.hardlinked_script = true;
    let observed = run_script_skill_case(case).await;
    assert!(observed.prompts.is_empty());
    assert_script_terminal(
        &observed,
        RuntimeToolStatus::Failed,
        expected_audit(
            RuntimeApprovalSource::Auto,
            RuntimeApprovalDecision::Once,
            None,
        ),
        1,
        0,
    );
    let result = observed
        .outcome
        .events
        .iter()
        .find_map(|event| match event {
            RuntimeEvent::ToolCallFinished(result) if result.call_id == "s16-bash" => Some(result),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        result.output,
        "Tool error: bash failed (hardlink_preflight)"
    );
}
