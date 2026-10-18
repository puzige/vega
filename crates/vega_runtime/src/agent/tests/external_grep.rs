use super::*;

struct GrepFixture {
    sandbox: tempfile::TempDir,
    tools: vega_tools::Tools,
    directory: PathBuf,
    file: PathBuf,
}

impl GrepFixture {
    fn new() -> Self {
        let sandbox = tempdir().unwrap();
        let project = sandbox.path().join("project");
        let directory = sandbox.path().join("external/scope");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(&directory).unwrap();
        let file = directory.join("sample.txt");
        fs::write(&file, "needle original\n").unwrap();
        fs::write(project.join("local.txt"), "needle local\n").unwrap();
        let tools = vega_tools::Tools::new(&project).unwrap();
        Self {
            sandbox,
            tools,
            directory: fs::canonicalize(directory).unwrap(),
            file: fs::canonicalize(file).unwrap(),
        }
    }

    fn target(&self, directory: bool) -> &std::path::Path {
        if directory {
            &self.directory
        } else {
            &self.file
        }
    }

    fn relative_target(&self, directory: bool) -> &str {
        if directory {
            "../external/scope"
        } else {
            "../external/scope/sample.txt"
        }
    }

    fn request(&self, mode: RuntimeRunMode, permission: RuntimePermissionMode) -> AgentRequest {
        let mut req = request(Vec::new());
        req.tool_config = tool_config(mode, permission, self.sandbox.path().join("checkpoints"));
        req
    }
}

struct GrepHook {
    prompts: Mutex<Vec<RuntimePermissionPrompt>>,
    tokens: Mutex<Vec<CancellationToken>>,
    decision: Option<RuntimeUserDecision>,
    action: Option<Box<dyn Fn() + Send + Sync>>,
}

impl GrepHook {
    fn new(decision: Option<RuntimeUserDecision>) -> Self {
        Self {
            prompts: Mutex::new(Vec::new()),
            tokens: Mutex::new(Vec::new()),
            decision,
            action: None,
        }
    }

    fn with_action(mut self, action: impl Fn() + Send + Sync + 'static) -> Self {
        self.action = Some(Box::new(action));
        self
    }
}

impl RuntimePermissionHook for GrepHook {
    fn request(
        &self,
        prompt: RuntimePermissionPrompt,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<RuntimeUserDecision, VegaError>> {
        self.prompts.lock().unwrap().push(prompt);
        self.tokens.lock().unwrap().push(cancel);
        if let Some(action) = &self.action {
            action();
        }
        let decision = self.decision.clone();
        async move {
            match decision {
                Some(decision) => Ok(decision),
                None => futures::future::pending().await,
            }
        }
        .boxed()
    }
}

fn grep_call(id: &str, input_json: String) -> ProviderEvent {
    ProviderEvent::ToolUse {
        id: id.into(),
        name: "grep".into(),
        input_json,
    }
}

fn grep_input(pattern: &str, path: &std::path::Path) -> String {
    serde_json::json!({"pattern":pattern,"path":path.to_str().unwrap()}).to_string()
}

fn grep_round(mut calls: Vec<ProviderEvent>) -> Vec<ScriptStep> {
    calls.push(ProviderEvent::Done {
        stop_reason: StopReason::ToolUse,
    });
    vec![ScriptStep::events(calls)]
}

fn grep_provider(calls: Vec<ProviderEvent>) -> MockProvider {
    MockProvider::new_rounds(vec![
        grep_round(calls),
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ])
}

fn grep_finished<'a>(outcome: &'a AgentOutcome, id: &str) -> &'a RuntimeToolResult {
    outcome
        .events
        .iter()
        .find_map(|event| match event {
            RuntimeEvent::ToolCallFinished(result)
            | RuntimeEvent::ToolCallValidationRejected { result, .. }
                if result.call_id == id =>
            {
                Some(result)
            }
            _ => None,
        })
        .unwrap()
}

fn grep_model_inputs(provider: &MockProvider, round: usize) -> Vec<(String, String)> {
    let requests = provider.requests();
    let body = crate::openai::build_request_body(&requests[round]);
    body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|message| message["tool_calls"].as_array())
        .flatten()
        .filter(|call| call["function"]["name"] == "grep")
        .map(|call| {
            (
                call["id"].as_str().unwrap().to_owned(),
                call["function"]["arguments"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

fn grep_signature(path: &std::path::Path, directory: bool) -> String {
    if directory {
        format!("grep-dir:v1:{}", path.to_str().unwrap())
    } else {
        path.to_str().unwrap().to_owned()
    }
}

#[tokio::test]
async fn issue426_external_grep_once_and_full_access_follow_read_in_all_modes() {
    for directory in [false, true] {
        for mode in [
            RuntimeRunMode::Ask,
            RuntimeRunMode::Plan,
            RuntimeRunMode::Execute,
        ] {
            for permission in [
                RuntimePermissionMode::Confirm,
                RuntimePermissionMode::Auto,
                RuntimePermissionMode::ReadOnly,
                RuntimePermissionMode::FullAccess,
            ] {
                let fixture = GrepFixture::new();
                let raw = format!(
                    "{{ \"path\":\"{}\", \"pattern\":\"needle\" }}",
                    fixture.relative_target(directory)
                );
                let provider = grep_provider(vec![grep_call("external", raw.clone())]);
                let hook = GrepHook::new(Some(RuntimeUserDecision::Once));
                let outcome = run_agent_with_permission_sink(
                    &provider,
                    &fixture.tools,
                    fixture.request(mode, permission),
                    CancellationToken::new(),
                    &hook,
                    |_| async { Ok(()) },
                )
                .await
                .unwrap();
                let result = grep_finished(&outcome, "external");
                assert_eq!(result.status, RuntimeToolStatus::Success);
                assert_eq!(
                    result.output,
                    format!("{}:1:needle original", fixture.file.to_str().unwrap())
                );
                assert_eq!(outcome.executed_tool_call_count, 1);
                let full_access = permission == RuntimePermissionMode::FullAccess;
                assert_eq!(
                    result.approval.as_ref().unwrap().source,
                    if full_access {
                        RuntimeApprovalSource::FullAccess
                    } else {
                        RuntimeApprovalSource::User
                    }
                );
                assert_eq!(
                    result.approval.as_ref().unwrap().decision,
                    RuntimeApprovalDecision::Once
                );
                let prompts = hook.prompts.lock().unwrap();
                assert_eq!(prompts.len(), usize::from(!full_access));
                if let Some(prompt) = prompts.first() {
                    assert_eq!(prompt.target.call_id, "external");
                    assert_eq!(prompt.target.tool, RuntimeMutatingTool::Read);
                    assert_eq!(
                        prompt.target.exact_pattern,
                        grep_signature(fixture.target(directory), directory)
                    );
                    assert_eq!(
                        prompt.target.display_target,
                        fixture.target(directory).to_str().unwrap()
                    );
                    assert!(prompt.danger.is_none());
                }
                assert!(
                    hook.tokens
                        .lock()
                        .unwrap()
                        .iter()
                        .all(|token| token.is_cancelled())
                );
                assert!(outcome.events.iter().any(|event| matches!(
                    event,
                    RuntimeEvent::ToolCallProposed(call)
                        if call.name == "grep"
                            && serde_json::from_str::<Value>(&call.input_json).unwrap()
                                == serde_json::json!({"pattern":"needle","path":fixture.target(directory)})
                )));
                assert_eq!(
                    grep_model_inputs(&provider, 1),
                    vec![("external".into(), raw)]
                );
            }
        }
    }
}

#[tokio::test]
async fn issue426_external_grep_always_deny_and_timeout_preserve_audits() {
    for directory in [false, true] {
        for decision in [
            Some(RuntimeUserDecision::Always),
            Some(RuntimeUserDecision::Deny { note: None }),
            None,
        ] {
            let fixture = GrepFixture::new();
            let provider = grep_provider(vec![grep_call(
                "decision",
                grep_input("needle", fixture.target(directory)),
            )]);
            let allowed = decision == Some(RuntimeUserDecision::Always);
            let timeout = decision.is_none();
            let hook = GrepHook::new(decision);
            let mut req = fixture.request(RuntimeRunMode::Execute, RuntimePermissionMode::Confirm);
            req.tool_config = req
                .tool_config
                .with_permission_timeout(Duration::from_millis(5));
            let outcome = run_agent_with_permission_sink(
                &provider,
                &fixture.tools,
                req,
                CancellationToken::new(),
                &hook,
                |_| async { Ok(()) },
            )
            .await
            .unwrap();
            let result = grep_finished(&outcome, "decision");
            assert_eq!(hook.prompts.lock().unwrap().len(), 1);
            assert!(
                hook.tokens
                    .lock()
                    .unwrap()
                    .iter()
                    .all(|token| token.is_cancelled())
            );
            assert_eq!(outcome.executed_tool_call_count, usize::from(allowed));
            assert_eq!(
                result.status,
                if allowed {
                    RuntimeToolStatus::Success
                } else {
                    RuntimeToolStatus::Rejected
                }
            );
            assert_eq!(
                result.approval.as_ref().unwrap().source,
                if timeout {
                    RuntimeApprovalSource::Timeout
                } else {
                    RuntimeApprovalSource::User
                }
            );
            assert_eq!(
                result.approval.as_ref().unwrap().decision,
                if allowed {
                    RuntimeApprovalDecision::Always
                } else {
                    RuntimeApprovalDecision::Deny
                }
            );
            let remembered = outcome.events.iter().find_map(|event| match event {
                RuntimeEvent::ToolCallApproved { remember_rule, .. } => remember_rule.as_ref(),
                _ => None,
            });
            assert_eq!(remembered.is_some(), allowed);
            if let Some(target) = remembered {
                assert_eq!(target.tool, RuntimeMutatingTool::Read);
                assert_eq!(
                    target.exact_pattern,
                    grep_signature(fixture.target(directory), directory)
                );
                assert_eq!(
                    target.display_target,
                    fixture.target(directory).to_str().unwrap()
                );
            } else {
                assert!(!result.output.contains("needle original"));
            }
        }
    }
}

#[tokio::test]
async fn issue426_external_grep_exact_rules_reuse_only_the_right_target_kind() {
    for directory in [false, true] {
        let fixture = GrepFixture::new();
        let provider = grep_provider(vec![grep_call(
            "rule",
            grep_input("needle", fixture.target(directory)),
        )]);
        let hook = GrepHook::new(Some(RuntimeUserDecision::Deny { note: None }));
        let mut req = fixture.request(RuntimeRunMode::Plan, RuntimePermissionMode::ReadOnly);
        req.tool_config.exact_rules.push(RuntimeExactRule {
            tool: RuntimeMutatingTool::Read,
            pattern: grep_signature(fixture.target(directory), directory),
        });
        let outcome = run_agent_with_permission_sink(
            &provider,
            &fixture.tools,
            req,
            CancellationToken::new(),
            &hook,
            |_| async { Ok(()) },
        )
        .await
        .unwrap();
        assert!(hook.prompts.lock().unwrap().is_empty());
        let result = grep_finished(&outcome, "rule");
        assert_eq!(result.status, RuntimeToolStatus::Success);
        assert_eq!(
            result.approval.as_ref().unwrap().source,
            RuntimeApprovalSource::Rule
        );
        assert_eq!(
            result.approval.as_ref().unwrap().decision,
            RuntimeApprovalDecision::Always
        );
        assert!(outcome.events.iter().any(|event| matches!(
            event,
            RuntimeEvent::ToolCallApproved {
                remember_rule: None,
                ..
            }
        )));
    }
}

#[tokio::test]
async fn issue426_bare_read_rules_never_upgrade_into_directory_grep() {
    for formerly_file in [false, true] {
        let fixture = GrepFixture::new();
        let target = if formerly_file {
            fs::remove_file(&fixture.file).unwrap();
            fs::create_dir(&fixture.file).unwrap();
            fs::write(fixture.file.join("replacement.txt"), "needle replacement\n").unwrap();
            &fixture.file
        } else {
            &fixture.directory
        };
        let provider = grep_provider(vec![grep_call("legacy-rule", grep_input("needle", target))]);
        let hook = GrepHook::new(Some(RuntimeUserDecision::Deny { note: None }));
        let mut req = fixture.request(RuntimeRunMode::Execute, RuntimePermissionMode::Auto);
        req.tool_config.exact_rules.push(RuntimeExactRule {
            tool: RuntimeMutatingTool::Read,
            pattern: target.to_str().unwrap().into(),
        });
        let outcome = run_agent_with_permission_sink(
            &provider,
            &fixture.tools,
            req,
            CancellationToken::new(),
            &hook,
            |_| async { Ok(()) },
        )
        .await
        .unwrap();
        assert_eq!(hook.prompts.lock().unwrap().len(), 1);
        assert_eq!(
            grep_finished(&outcome, "legacy-rule").status,
            RuntimeToolStatus::Rejected
        );
        assert_eq!(outcome.executed_tool_call_count, 0);
        assert!(
            !grep_finished(&outcome, "legacy-rule")
                .output
                .contains("needle")
        );
    }
}

#[tokio::test]
async fn issue426_directory_always_reuses_one_entry_not_parent_sibling_child_or_read() {
    let fixture = GrepFixture::new();
    let child = fixture.directory.join("child");
    let sibling = fixture.directory.parent().unwrap().join("sibling");
    fs::create_dir(&child).unwrap();
    fs::create_dir(&sibling).unwrap();
    fs::write(child.join("child.txt"), "needle child\n").unwrap();
    fs::write(sibling.join("sibling.txt"), "needle sibling\n").unwrap();
    let provider = grep_provider(vec![
        grep_call("first", grep_input("needle", &fixture.directory)),
        grep_call(
            "same",
            r#"{"pattern":"original","path":"../external/scope/../scope"}"#.into(),
        ),
        grep_call("child", grep_input("needle", &child)),
        grep_call("sibling", grep_input("needle", &sibling)),
        grep_call(
            "parent",
            grep_input("needle", fixture.directory.parent().unwrap()),
        ),
        ProviderEvent::ToolUse {
            id: "read-child".into(),
            name: "read".into(),
            input_json: serde_json::json!({"path":fixture.file}).to_string(),
        },
    ]);
    let hook = GrepHook::new(Some(RuntimeUserDecision::Always));
    let outcome = run_agent_with_permission_sink(
        &provider,
        &fixture.tools,
        fixture.request(RuntimeRunMode::Execute, RuntimePermissionMode::Confirm),
        CancellationToken::new(),
        &hook,
        |_| async { Ok(()) },
    )
    .await
    .unwrap();
    let prompts = hook.prompts.lock().unwrap();
    assert_eq!(prompts.len(), 5);
    assert_eq!(
        prompts
            .iter()
            .map(|prompt| prompt.target.call_id.as_str())
            .collect::<Vec<_>>(),
        vec!["first", "child", "sibling", "parent", "read-child"]
    );
    assert_eq!(outcome.executed_tool_call_count, 6);
    assert_eq!(
        grep_finished(&outcome, "same")
            .approval
            .as_ref()
            .unwrap()
            .source,
        RuntimeApprovalSource::Rule
    );
    for id in ["first", "child", "sibling", "parent", "read-child"] {
        assert_eq!(
            grep_finished(&outcome, id)
                .approval
                .as_ref()
                .unwrap()
                .source,
            RuntimeApprovalSource::User
        );
    }
    assert_eq!(
        prompts.last().unwrap().target.exact_pattern,
        fixture.file.to_str().unwrap()
    );
}

#[tokio::test]
async fn issue426_approval_hook_replacing_file_or_directory_fails_closed() {
    for directory in [false, true] {
        for replacement_directory in [false, true] {
            let fixture = GrepFixture::new();
            let target = fixture.target(directory).to_path_buf();
            let held = target.with_extension("held");
            let provider = grep_provider(vec![grep_call("changed", grep_input("needle", &target))]);
            let hook = GrepHook::new(Some(RuntimeUserDecision::Once)).with_action(move || {
                fs::rename(&target, &held).unwrap();
                if replacement_directory {
                    fs::create_dir(&target).unwrap();
                    fs::write(target.join("new.txt"), "needle REPLACEMENT_CANARY\n").unwrap();
                } else {
                    fs::write(&target, "needle REPLACEMENT_CANARY\n").unwrap();
                }
            });
            let outcome = run_agent_with_permission_sink(
                &provider,
                &fixture.tools,
                fixture.request(RuntimeRunMode::Execute, RuntimePermissionMode::Confirm),
                CancellationToken::new(),
                &hook,
                |_| async { Ok(()) },
            )
            .await
            .unwrap();
            let result = grep_finished(&outcome, "changed");
            assert_eq!(result.status, RuntimeToolStatus::Failed);
            assert_eq!(
                result.output,
                "Tool error: invalid input: grep target changed before execution"
            );
            assert!(!result.output.contains("REPLACEMENT_CANARY"));
            assert!(!result.output.contains("needle original"));
            assert_eq!(hook.prompts.lock().unwrap().len(), 1);
            assert_eq!(
                result.approval.as_ref().unwrap().source,
                RuntimeApprovalSource::User
            );
        }
    }
}

#[tokio::test]
async fn issue426_approval_hook_replacing_an_ancestor_fails_closed() {
    for directory in [false, true] {
        let fixture = GrepFixture::new();
        let ancestor = fixture.directory.parent().unwrap().to_path_buf();
        let held = ancestor.with_extension("held");
        let provider = grep_provider(vec![grep_call(
            "ancestor",
            grep_input("needle", fixture.target(directory)),
        )]);
        let hook = GrepHook::new(Some(RuntimeUserDecision::Once)).with_action(move || {
            fs::rename(&ancestor, &held).unwrap();
            fs::create_dir_all(ancestor.join("scope")).unwrap();
            fs::write(
                ancestor.join("scope/sample.txt"),
                "needle ANCESTOR_CANARY\n",
            )
            .unwrap();
        });
        let outcome = run_agent_with_permission_sink(
            &provider,
            &fixture.tools,
            fixture.request(RuntimeRunMode::Ask, RuntimePermissionMode::Confirm),
            CancellationToken::new(),
            &hook,
            |_| async { Ok(()) },
        )
        .await
        .unwrap();
        let result = grep_finished(&outcome, "ancestor");
        assert_eq!(result.status, RuntimeToolStatus::Failed);
        assert_eq!(
            result.output,
            "Tool error: invalid input: grep target changed before execution"
        );
        assert!(!result.output.contains("ANCESTOR_CANARY"));
    }
}

#[cfg(unix)]
#[tokio::test]
async fn issue426_approval_hook_retargeting_only_the_alias_cannot_redirect_grep() {
    for directory in [false, true] {
        let fixture = GrepFixture::new();
        let alias = fixture.tools.root().join("alias");
        let redirect = fixture.sandbox.path().join("redirect");
        fs::create_dir(&redirect).unwrap();
        fs::write(redirect.join("sample.txt"), "needle REDIRECT_CANARY\n").unwrap();
        std::os::unix::fs::symlink(fixture.target(directory), &alias).unwrap();
        let provider = grep_provider(vec![grep_call(
            "alias",
            r#"{"pattern":"needle","path":"alias"}"#.into(),
        )]);
        let hook = GrepHook::new(Some(RuntimeUserDecision::Once)).with_action(move || {
            fs::remove_file(&alias).unwrap();
            std::os::unix::fs::symlink(&redirect, &alias).unwrap();
        });
        let outcome = run_agent_with_permission_sink(
            &provider,
            &fixture.tools,
            fixture.request(RuntimeRunMode::Execute, RuntimePermissionMode::Confirm),
            CancellationToken::new(),
            &hook,
            |_| async { Ok(()) },
        )
        .await
        .unwrap();
        let result = grep_finished(&outcome, "alias");
        assert_eq!(result.status, RuntimeToolStatus::Success);
        assert_eq!(
            result.output,
            format!("{}:1:needle original", fixture.file.to_str().unwrap())
        );
        assert!(!result.output.contains("REDIRECT_CANARY"));
    }
}

#[tokio::test]
async fn issue426_same_inode_update_is_not_a_content_lock() {
    let fixture = GrepFixture::new();
    let file = fixture.file.clone();
    let provider = grep_provider(vec![grep_call("updated", grep_input("needle", &file))]);
    let hook = GrepHook::new(Some(RuntimeUserDecision::Once)).with_action(move || {
        fs::write(&file, "needle updated in place\n").unwrap();
    });
    let outcome = run_agent_with_permission_sink(
        &provider,
        &fixture.tools,
        fixture.request(RuntimeRunMode::Execute, RuntimePermissionMode::Confirm),
        CancellationToken::new(),
        &hook,
        |_| async { Ok(()) },
    )
    .await
    .unwrap();
    assert_eq!(
        grep_finished(&outcome, "updated").status,
        RuntimeToolStatus::Success
    );
    assert!(
        grep_finished(&outcome, "updated")
            .output
            .ends_with(":1:needle updated in place")
    );
}

#[tokio::test]
async fn issue426_invalid_grep_is_content_free_and_correctable_with_readonly_approval() {
    let fixture = GrepFixture::new();
    let invalids = [
        grep_input("SENSITIVE_REGEX_CANARY[", &fixture.file),
        r#"{"pattern":17,"path":"SENSITIVE_PATH_CANARY"}"#.into(),
        r#"{"pattern":"SENSITIVE_PATTERN_CANARY","path":17}"#.into(),
        r#"{"pattern":"needle","extra":"SENSITIVE_FIELD_CANARY"}"#.into(),
        r#"{"path":"SENSITIVE_MISSING_CANARY"}"#.into(),
        r#"{"pattern":"SENSITIVE_JSON_CANARY""#.into(),
        r#"["SENSITIVE_ARRAY_CANARY"]"#.into(),
        r#"["needle",null]"#.into(),
        r#"{"pattern":"SENSITIVE_DUPLICATE_CANARY","pattern":"needle","path":null}"#.into(),
        r#"{"pattern":"needle","path":"SENSITIVE_DUPLICATE_CANARY","path":null}"#.into(),
    ];
    let calls = invalids
        .iter()
        .enumerate()
        .map(|(index, raw)| grep_call(&format!("bad-{index}"), raw.clone()))
        .collect::<Vec<_>>();
    let provider = MockProvider::new_rounds(vec![
        grep_round(calls),
        grep_round(vec![grep_call(
            "corrected",
            r#"{"pattern":"needle","path":"local.txt"}"#.into(),
        )]),
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ]);
    let hook = GrepHook::new(Some(RuntimeUserDecision::Once));
    let outcome = run_agent_with_permission_sink(
        &provider,
        &fixture.tools,
        fixture.request(RuntimeRunMode::Execute, RuntimePermissionMode::Confirm),
        CancellationToken::new(),
        &hook,
        |_| async { Ok(()) },
    )
    .await
    .unwrap();
    assert!(hook.prompts.lock().unwrap().is_empty());
    assert_eq!(outcome.executed_tool_call_count, invalids.len() + 1);
    for index in 0..invalids.len() {
        let id = format!("bad-{index}");
        let result = grep_finished(&outcome, &id);
        assert_eq!(result.status, RuntimeToolStatus::Failed);
        assert_eq!(result.output, GREP_INVALID_INPUT_OUTPUT);
        assert_eq!(
            result.approval.as_ref().unwrap().source,
            RuntimeApprovalSource::ReadonlyTool
        );
        let lifecycle = outcome
            .events
            .iter()
            .filter_map(|event| match event {
                RuntimeEvent::ToolCallApproved {
                    call_id,
                    audit,
                    remember_rule,
                } if call_id == &id => {
                    assert_eq!(audit.decision, RuntimeApprovalDecision::Once);
                    assert_eq!(audit.source, RuntimeApprovalSource::ReadonlyTool);
                    assert!(remember_rule.is_none());
                    Some("approved")
                }
                RuntimeEvent::ToolCallRunning { call_id } if call_id == &id => Some("running"),
                RuntimeEvent::ToolCallFinished(result) if result.call_id == id => Some("failed"),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(lifecycle, ["approved", "running", "failed"]);
    }
    let audit = outcome
        .messages
        .iter()
        .flat_map(|message| &message.tool_calls)
        .map(|call| call.input_json.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let requests = provider.requests();
    let wire = serde_json::to_string(&crate::openai::build_request_body(&requests[1])).unwrap();
    assert!(!audit.contains("SENSITIVE_"));
    assert!(!wire.contains("SENSITIVE_"));
    assert!(grep_model_inputs(&provider, 1).is_empty());
    assert!(!format!("{:?}", outcome.events).contains("SENSITIVE_"));
    assert_eq!(
        grep_finished(&outcome, "corrected").status,
        RuntimeToolStatus::Success
    );
    assert_eq!(
        grep_finished(&outcome, "corrected").output,
        "local.txt:1:needle local"
    );
}

#[tokio::test]
async fn issue426_default_null_empty_and_internal_paths_use_readonly_frozen_grep() {
    let fixture = GrepFixture::new();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&fixture.directory, fixture.tools.root().join("outside")).unwrap();
    let inputs = [
        r#"{"pattern":"needle"}"#.to_owned(),
        r#"{"pattern":"needle","path":null}"#.to_owned(),
        r#"{"pattern":"needle","path":""}"#.to_owned(),
        r#"{"pattern":"needle","path":"."}"#.to_owned(),
        grep_input("needle", fixture.tools.root()),
    ];
    for mode in [
        RuntimeRunMode::Ask,
        RuntimeRunMode::Plan,
        RuntimeRunMode::Execute,
    ] {
        let calls = inputs
            .iter()
            .enumerate()
            .map(|(index, input)| grep_call(&format!("default-{index}"), input.clone()))
            .collect();
        let provider = grep_provider(calls);
        let hook = GrepHook::new(Some(RuntimeUserDecision::Deny { note: None }));
        let outcome = run_agent_with_permission_sink(
            &provider,
            &fixture.tools,
            fixture.request(mode, RuntimePermissionMode::ReadOnly),
            CancellationToken::new(),
            &hook,
            |_| async { Ok(()) },
        )
        .await
        .unwrap();
        assert!(hook.prompts.lock().unwrap().is_empty());
        assert_eq!(outcome.executed_tool_call_count, inputs.len());
        for index in 0..inputs.len() {
            let result = grep_finished(&outcome, &format!("default-{index}"));
            assert_eq!(result.status, RuntimeToolStatus::Success);
            assert_eq!(result.output, "local.txt:1:needle local");
            assert_eq!(
                result.approval.as_ref().unwrap().source,
                RuntimeApprovalSource::ReadonlyTool
            );
            let call = outcome
                .messages
                .iter()
                .flat_map(|message| &message.tool_calls)
                .find(|call| call.id == format!("default-{index}"))
                .unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(&call.input_json).unwrap(),
                serde_json::json!({"pattern":"needle","path":fixture.tools.root()})
            );
        }
        assert_eq!(
            grep_model_inputs(&provider, 1),
            inputs
                .iter()
                .enumerate()
                .map(|(index, input)| (format!("default-{index}"), input.clone()))
                .collect::<Vec<_>>()
        );
    }
}

#[tokio::test]
async fn issue426_internal_file_and_subdirectory_keep_project_relative_results() {
    let fixture = GrepFixture::new();
    let directory = fixture.tools.root().join("source");
    fs::create_dir(&directory).unwrap();
    let file = directory.join("code.txt");
    fs::write(&file, "heading\nneedle source\n").unwrap();
    let inputs = [
        r#"{"pattern":"needle","path":"source"}"#.to_owned(),
        r#"{"pattern":"needle","path":"source/code.txt"}"#.to_owned(),
        grep_input("needle", &directory),
        grep_input("needle", &file),
    ];
    let provider = grep_provider(
        inputs
            .iter()
            .enumerate()
            .map(|(index, input)| grep_call(&format!("internal-{index}"), input.clone()))
            .collect(),
    );
    let hook = GrepHook::new(Some(RuntimeUserDecision::Deny { note: None }));
    let outcome = run_agent_with_permission_sink(
        &provider,
        &fixture.tools,
        fixture.request(RuntimeRunMode::Plan, RuntimePermissionMode::ReadOnly),
        CancellationToken::new(),
        &hook,
        |_| async { Ok(()) },
    )
    .await
    .unwrap();
    assert!(hook.prompts.lock().unwrap().is_empty());
    assert_eq!(outcome.executed_tool_call_count, inputs.len());
    for index in 0..inputs.len() {
        let result = grep_finished(&outcome, &format!("internal-{index}"));
        assert_eq!(result.status, RuntimeToolStatus::Success);
        assert_eq!(result.output, "source/code.txt:2:needle source");
    }
    assert_eq!(
        grep_model_inputs(&provider, 1),
        inputs
            .iter()
            .enumerate()
            .map(|(index, input)| (format!("internal-{index}"), input.clone()))
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn issue426_internal_grep_does_not_reprepare_after_readonly_approval() {
    let fixture = GrepFixture::new();
    let local = fixture.tools.root().join("local.txt");
    let held = local.with_extension("held");
    let provider = grep_provider(vec![grep_call(
        "internal",
        r#"{"pattern":"needle","path":"local.txt"}"#.into(),
    )]);
    let hook = GrepHook::new(Some(RuntimeUserDecision::Deny { note: None }));
    let outcome = run_agent_with_permission_sink(
        &provider,
        &fixture.tools,
        fixture.request(RuntimeRunMode::Ask, RuntimePermissionMode::ReadOnly),
        CancellationToken::new(),
        &hook,
        move |event| {
            if matches!(event, RuntimeEvent::ToolCallApproved { .. }) {
                fs::rename(&local, &held).unwrap();
                fs::write(&local, "needle INTERNAL_REPLACEMENT_CANARY\n").unwrap();
            }
            async { Ok(()) }
        },
    )
    .await
    .unwrap();
    let result = grep_finished(&outcome, "internal");
    assert!(hook.prompts.lock().unwrap().is_empty());
    assert_eq!(result.status, RuntimeToolStatus::Failed);
    assert_eq!(
        result.output,
        "Tool error: invalid input: grep target changed before execution"
    );
    assert!(!result.output.contains("INTERNAL_REPLACEMENT_CANARY"));
}

#[tokio::test]
async fn issue426_grep_cancellation_preserves_read_authorization_lifecycle() {
    for directory in [false, true] {
        for stage in ["proposal", "approval", "running", "permission"] {
            let fixture = GrepFixture::new();
            let provider = grep_provider(vec![
                grep_call("cancelled", grep_input("needle", fixture.target(directory))),
                grep_call("next", r#"{"pattern":"needle","path":"local.txt"}"#.into()),
            ]);
            let cancel = CancellationToken::new();
            let hook_cancel = cancel.clone();
            let hook = GrepHook::new(Some(RuntimeUserDecision::Once)).with_action(move || {
                if stage == "permission" {
                    hook_cancel.cancel();
                }
            });
            let sink_cancel = cancel.clone();
            let outcome = run_agent_with_permission_sink(
                &provider,
                &fixture.tools,
                fixture.request(RuntimeRunMode::Execute, RuntimePermissionMode::Confirm),
                cancel,
                &hook,
                move |event| {
                    if matches!(
                        (stage, event),
                        ("proposal", RuntimeEvent::ToolCallProposed(_))
                            | ("approval", RuntimeEvent::ToolCallApproved { .. })
                            | ("running", RuntimeEvent::ToolCallRunning { .. })
                    ) {
                        sink_cancel.cancel();
                    }
                    async { Ok(()) }
                },
            )
            .await
            .unwrap();
            let result = grep_finished(&outcome, "cancelled");
            assert!(outcome.interrupted);
            assert_eq!(outcome.executed_tool_call_count, 0);
            let approval = result.approval.as_ref().unwrap();
            if matches!(stage, "proposal" | "permission") {
                assert_eq!(result.status, RuntimeToolStatus::Rejected);
                assert_eq!(result.output, "Tool error: permission denied");
                assert_eq!(approval.decision, RuntimeApprovalDecision::Deny);
                assert_eq!(approval.source, RuntimeApprovalSource::Timeout);
                assert!(!outcome.events.iter().any(|event| matches!(event, RuntimeEvent::ToolCallApproved { call_id, .. } | RuntimeEvent::ToolCallRunning { call_id } if call_id == "cancelled")));
            } else {
                assert_eq!(result.status, RuntimeToolStatus::Cancelled);
                assert_eq!(approval.decision, RuntimeApprovalDecision::Once);
                assert_eq!(approval.source, RuntimeApprovalSource::User);
            }
            assert!(!result.output.contains("needle original"));
            assert!(!outcome.events.iter().any(|event| matches!(event, RuntimeEvent::ToolCallRunning { call_id } if call_id == "next")));
            assert!(
                hook.tokens
                    .lock()
                    .unwrap()
                    .iter()
                    .all(|token| token.is_cancelled())
            );
        }
    }
}

#[tokio::test]
async fn issue426_grep_worker_cancellation_never_becomes_failed_or_success() {
    for directory in [false, true] {
        let fixture = GrepFixture::new();
        let provider = grep_provider(vec![
            grep_call("worker", grep_input("needle", fixture.target(directory))),
            grep_call("next", r#"{"pattern":"needle","path":"local.txt"}"#.into()),
        ]);
        let cancel = CancellationToken::new();
        let trigger = cancel.clone();
        let hook = GrepHook::new(Some(RuntimeUserDecision::Once));
        let (_scope, started, release) = register_readonly_execution_gate();
        let interrupt = async move {
            let observed = tokio::time::timeout(Duration::from_secs(5), started).await;
            trigger.cancel();
            let released = release.send(());
            (observed, released)
        };
        let (outcome, (observed, released)) = tokio::join!(
            run_agent_with_permission_sink(
                &provider,
                &fixture.tools,
                fixture.request(RuntimeRunMode::Execute, RuntimePermissionMode::Confirm),
                cancel,
                &hook,
                |_| async { Ok(()) },
            ),
            interrupt,
        );
        observed.unwrap().unwrap();
        released.unwrap();
        let outcome = outcome.unwrap();
        let result = grep_finished(&outcome, "worker");
        assert!(outcome.interrupted);
        assert_eq!(outcome.executed_tool_call_count, 1);
        assert_eq!(result.status, RuntimeToolStatus::Cancelled);
        assert_eq!(
            result.approval.as_ref().unwrap().source,
            RuntimeApprovalSource::User
        );
        assert!(!result.output.contains("needle original"));
        assert!(!outcome.events.iter().any(
            |event| matches!(event, RuntimeEvent::ToolCallRunning { call_id } if call_id == "next")
        ));
    }
}

#[tokio::test]
async fn issue426_grep_ledger_reuses_canonical_identity_and_rejects_conflicts() {
    for directory in [false, true] {
        let fixture = GrepFixture::new();
        let first_raw = grep_input(
            "needle",
            std::path::Path::new(fixture.relative_target(directory)),
        );
        let provider = MockProvider::new_rounds(vec![
            grep_round(vec![grep_call("ledger", first_raw.clone())]),
            grep_round(vec![
                grep_call("ledger", grep_input("needle", fixture.target(directory))),
                grep_call("ledger", grep_input("changed", fixture.target(directory))),
                grep_call(
                    "ledger",
                    r#"{"pattern":"needle","path":"local.txt"}"#.into(),
                ),
            ]),
            vec![ScriptStep::events(vec![ProviderEvent::Done {
                stop_reason: StopReason::End,
            }])],
        ]);
        let hook = GrepHook::new(Some(RuntimeUserDecision::Once));
        let file = fixture.file.clone();
        let outcome = run_agent_with_permission_sink(
            &provider,
            &fixture.tools,
            fixture.request(RuntimeRunMode::Execute, RuntimePermissionMode::Confirm),
            CancellationToken::new(),
            &hook,
            move |event| {
                if matches!(event, RuntimeEvent::ToolCallFinished(result) if result.call_id == "ledger" && !result.reused) {
                    fs::write(&file, "needle AFTER_FIRST_EXECUTION_CANARY\n").unwrap();
                }
                async { Ok(()) }
            },
        )
        .await
        .unwrap();
        assert_eq!(hook.prompts.lock().unwrap().len(), 1);
        assert_eq!(outcome.executed_tool_call_count, 1);
        let finished = outcome
            .events
            .iter()
            .filter_map(|event| match event {
                RuntimeEvent::ToolCallFinished(result) => Some(result),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(finished.len(), 2);
        assert!(!finished[0].reused);
        assert!(finished[1].reused);
        assert_eq!(finished[0].output, finished[1].output);
        assert!(!finished[1].output.contains("AFTER_FIRST_EXECUTION_CANARY"));
        assert_eq!(outcome.events.iter().filter(|event| matches!(event, RuntimeEvent::ToolCallConflict { result, .. } if result.output == CALL_ID_CONFLICT_OUTPUT)).count(), 2);
        assert_eq!(
            grep_model_inputs(&provider, 1),
            vec![("ledger".into(), first_raw)]
        );
    }
}

#[tokio::test]
async fn issue426_persisted_grep_results_reuse_without_authority_or_execution() {
    for directory in [false, true] {
        let fixture = GrepFixture::new();
        let raw = grep_input("needle", fixture.target(directory));
        let provider = grep_provider(vec![grep_call("persisted", raw.clone())]);
        let first_hook = GrepHook::new(Some(RuntimeUserDecision::Once));
        let first = run_agent_with_permission_sink(
            &provider,
            &fixture.tools,
            fixture.request(RuntimeRunMode::Execute, RuntimePermissionMode::Confirm),
            CancellationToken::new(),
            &first_hook,
            |_| async { Ok(()) },
        )
        .await
        .unwrap();
        let call = first
            .messages
            .iter()
            .flat_map(|message| &message.tool_calls)
            .find(|call| call.id == "persisted")
            .unwrap();
        let prior = CompletedToolCall {
            tool: call.name.clone(),
            input_json: call.input_json.clone(),
            result: grep_finished(&first, "persisted").clone(),
        };
        fs::write(&fixture.file, "needle PERSISTED_REEXECUTION_CANARY\n").unwrap();
        for case in ["reuse", "pattern", "path", "foreign"] {
            let input = match case {
                "pattern" => grep_input("different", fixture.target(directory)),
                "path" => r#"{"pattern":"needle","path":"local.txt"}"#.into(),
                _ => raw.clone(),
            };
            let provider = grep_provider(vec![grep_call("persisted", input)]);
            let mut req = fixture.request(RuntimeRunMode::Ask, RuntimePermissionMode::ReadOnly);
            req.completed_tool_results
                .insert("persisted".into(), prior.clone());
            if case == "foreign" {
                req.tool_config.foreign_call_ids.insert("persisted".into());
            }
            let hook = GrepHook::new(Some(RuntimeUserDecision::Deny { note: None }));
            let outcome = run_agent_with_permission_sink(
                &provider,
                &fixture.tools,
                req,
                CancellationToken::new(),
                &hook,
                |_| async { Ok(()) },
            )
            .await
            .unwrap();
            assert!(hook.prompts.lock().unwrap().is_empty());
            assert_eq!(outcome.executed_tool_call_count, 0);
            if case == "reuse" {
                let result = grep_finished(&outcome, "persisted");
                assert!(result.reused);
                assert_eq!(result.status, RuntimeToolStatus::Success);
                assert_eq!(result.output, prior.result.output);
                assert!(!result.output.contains("PERSISTED_REEXECUTION_CANARY"));
            } else {
                assert!(outcome.events.iter().any(|event| matches!(event,
                    RuntimeEvent::ToolCallConflict { result, .. }
                        if result.output == CALL_ID_CONFLICT_OUTPUT && !result.reused)));
            }
        }
    }
}

#[tokio::test]
async fn issue426_invalid_grep_ledger_reuses_only_the_same_failed_input() {
    let fixture = GrepFixture::new();
    let raw = grep_input("SENSITIVE_FIRST_CANARY[", &fixture.file);
    let provider = MockProvider::new_rounds(vec![
        grep_round(vec![grep_call("invalid-ledger", raw.clone())]),
        grep_round(vec![grep_call("invalid-ledger", raw)]),
        grep_round(vec![grep_call(
            "invalid-ledger",
            grep_input("SENSITIVE_SECOND_CANARY[", &fixture.file),
        )]),
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ]);
    let hook = GrepHook::new(Some(RuntimeUserDecision::Once));
    let outcome = run_agent_with_permission_sink(
        &provider,
        &fixture.tools,
        fixture.request(RuntimeRunMode::Execute, RuntimePermissionMode::Confirm),
        CancellationToken::new(),
        &hook,
        |_| async { Ok(()) },
    )
    .await
    .unwrap();
    assert!(hook.prompts.lock().unwrap().is_empty());
    assert_eq!(outcome.executed_tool_call_count, 1);
    let rejected = outcome
        .events
        .iter()
        .filter_map(|event| match event {
            RuntimeEvent::ToolCallFinished(result) => Some(result),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(rejected.len(), 2);
    assert!(!rejected[0].reused);
    assert!(rejected[1].reused);
    assert!(
        outcome
            .events
            .iter()
            .any(|event| matches!(event, RuntimeEvent::ToolCallConflict { .. }))
    );
    assert!(
        outcome
            .messages
            .iter()
            .flat_map(|message| &message.tool_calls)
            .all(|call| !call.input_json.contains("SENSITIVE_"))
    );
}

#[test]
fn issue426_grep_description_allows_read_authorized_external_targets_without_changing_glob() {
    let definitions = tool_definitions(RuntimeRunMode::Ask);
    let grep = definitions.iter().find(|tool| tool.name == "grep").unwrap();
    assert!(grep.description.contains("external"));
    assert!(grep.description.contains("Read permission"));
    assert!(
        !grep
            .description
            .contains("External targets are unsupported")
    );
    assert_eq!(
        grep.input_schema["required"],
        serde_json::json!(["pattern", "path"])
    );
    assert_eq!(
        grep.input_schema["properties"]["path"]["type"],
        serde_json::json!(["string", "null"])
    );
    let glob = definitions.iter().find(|tool| tool.name == "glob").unwrap();
    assert_eq!(
        glob.description,
        "Find files with a pattern relative to the project root. Use a focused pattern before reading matching files. Absolute patterns and parent traversal are unsupported. Example: {\"pattern\":\"src/**/*.rs\"}."
    );
}
