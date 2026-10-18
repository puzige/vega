use super::*;

fn read_round(id: &str) -> Vec<ScriptStep> {
    vec![ScriptStep::events(vec![
        ProviderEvent::ToolUse {
            id: id.into(),
            name: "read".into(),
            input_json: r#"{"path":"large.txt","limit":6000}"#.into(),
        },
        ProviderEvent::Done {
            stop_reason: StopReason::ToolUse,
        },
    ])]
}

fn final_round() -> Vec<ScriptStep> {
    vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("Checked the complete evidence.".into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]
}

fn content(request: &ChatRequest, id: &str) -> String {
    request
        .messages
        .iter()
        .find(|message| message.tool_call_id.as_deref() == Some(id))
        .unwrap()
        .content
        .clone()
}

fn reference(content: &str) -> Option<serde_json::Value> {
    let body = content.split("\n\n<vega_runtime_notice>").next().unwrap();
    serde_json::from_str::<serde_json::Value>(body)
        .ok()?
        .get("vega_tool_result_reference")
        .cloned()
}

fn results(outcome: &AgentOutcome) -> Vec<&RuntimeToolResult> {
    outcome
        .events
        .iter()
        .filter_map(|event| match event {
            RuntimeEvent::ToolCallFinished(result) => Some(result),
            _ => None,
        })
        .collect()
}

fn assert_stable(requests: &[ChatRequest]) {
    for pair in requests.windows(2) {
        assert_eq!(
            pair[0].messages.as_slice(),
            &pair[1].messages[..pair[0].messages.len()]
        );
        assert_eq!(pair[0].tools, pair[1].tools);
    }
    for request in requests {
        assert_eq!(
            crate::project_model_messages(&request.messages).unwrap(),
            request.messages
        );
    }
}

#[tokio::test]
async fn real_repeated_reads_execute_every_time_and_keep_full_audit_with_one_guidance() {
    let project = tempdir().unwrap();
    let file = "evidence ".repeat(100) + "\n";
    fs::write(project.path().join("large.txt"), file.repeat(30)).unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        read_round("first"),
        read_round("second"),
        read_round("third"),
        final_round(),
    ]);
    let outcome = run_agent(
        &provider,
        &tools,
        request(Vec::new()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(!outcome.failed && !outcome.interrupted);
    assert_eq!(outcome.executed_tool_call_count, 3);
    assert_eq!(outcome.final_text, "Checked the complete evidence.");
    let audit = results(&outcome);
    assert_eq!(audit.len(), 3);
    assert!(audit[0].output.len() > 20_000);
    assert!(audit.iter().all(|result| result.output == audit[0].output
        && result.truncated == Some(false)
        && result.status == RuntimeToolStatus::Success
        && !result.reused));
    let requests = provider.requests();
    assert_eq!(requests.len(), 4);
    let first = content(&requests[1], "first");
    assert!(first.contains("vega_tool_preview"));
    assert!(first.len() <= 8192 + 1024 + PROGRESS_NOTICE.len());
    assert_eq!(first.matches("<vega_runtime_notice>").count(), 1);
    assert_eq!(
        reference(&content(&requests[2], "second")).unwrap()["call_id"],
        "first"
    );
    let third = content(&requests[3], "third");
    assert_eq!(reference(&third).unwrap()["call_id"], "first");
    assert_eq!(third.matches("<vega_runtime_notice>").count(), 1);
    assert!(third.contains("The same tool inputs produced the same successful result"));
    assert_stable(&requests);
    let canonical = outcome
        .messages
        .iter()
        .filter(|message| message.role == ChatRole::Tool)
        .collect::<Vec<_>>();
    assert_eq!(canonical.len(), 3);
    for (message, result) in canonical.iter().zip(audit) {
        assert_eq!(message.content, result.output);
        assert!(!message.content.contains("vega_runtime_notice"));
    }
}

#[tokio::test]
async fn changed_middle_references_only_the_new_complete_read_result() {
    let project = tempdir().unwrap();
    let file = format!("{}\n", "x".repeat(900)).repeat(30);
    let mut changed = file.clone();
    changed.replace_range(12_000..12_001, "y");
    let path = project.path().join("large.txt");
    fs::write(&path, file).unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        read_round("first"),
        read_round("changed"),
        read_round("same-again"),
        final_round(),
    ]);
    let outcome = run_agent_with_sink(&provider, &tools, request(Vec::new()), CancellationToken::new(), move |event| {
        if matches!(event, RuntimeEvent::ToolCallFinished(ref result) if result.call_id == "first") {
            fs::write(&path, &changed).unwrap();
        }
        async { Ok(()) }
    }).await.unwrap();
    assert!(!outcome.failed && !outcome.interrupted);
    assert_eq!(outcome.executed_tool_call_count, 3);
    let audit = results(&outcome);
    assert_ne!(audit[0].output, audit[1].output);
    assert_eq!(audit[1].output, audit[2].output);
    assert!(audit.iter().all(|result| result.truncated == Some(false)));
    let requests = provider.requests();
    assert!(reference(&content(&requests[2], "changed")).is_none());
    assert_eq!(
        reference(&content(&requests[3], "same-again")).unwrap()["call_id"],
        "changed"
    );
    let first = content(&requests[1], "first");
    let first = first.strip_suffix(PROGRESS_NOTICE).unwrap();
    assert_eq!(first, content(&requests[2], "changed"));
    assert_stable(&requests);
}

#[tokio::test]
async fn actual_line_truncation_is_marked_and_never_becomes_complete_evidence() {
    let project = tempdir().unwrap();
    let lines = (1..=5_000)
        .map(|index| format!("line-{index}"))
        .collect::<Vec<_>>();
    let path = project.path().join("large.txt");
    fs::write(&path, lines.join("\n")).unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        read_round("first"),
        read_round("changed-middle"),
        final_round(),
    ]);
    let outcome = run_agent_with_sink(&provider, &tools, request(Vec::new()), CancellationToken::new(), move |event| {
        if matches!(event, RuntimeEvent::ToolCallFinished(ref result) if result.call_id == "first") {
            let mut changed = lines.clone();
            changed[2_499] = "changed omitted detail".into();
            fs::write(&path, changed.join("\n")).unwrap();
        }
        async { Ok(()) }
    }).await.unwrap();
    assert!(!outcome.failed && !outcome.interrupted);
    assert_eq!(outcome.executed_tool_call_count, 2);
    let audit = results(&outcome);
    assert_eq!(audit.len(), 2);
    assert_eq!(audit[0].output, audit[1].output);
    assert!(audit.iter().all(
        |result| result.truncated == Some(true) && result.status == RuntimeToolStatus::Success
    ));
    let numbered = (1..=5_000)
        .map(|index| format!("{index:4} | line-{index}"))
        .collect::<Vec<_>>()
        .join("\n");
    let (expected, cut) = truncate_output_lines(&numbered);
    assert!(cut);
    assert_eq!(audit[0].output, expected);
    assert_eq!(audit[0].output.lines().count(), 4_001);
    let requests = provider.requests();
    let changed = content(&requests[2], "changed-middle");
    assert!(reference(&changed).is_none());
    assert!(changed.contains("\"truncated\":true"));
    assert!(changed.len() <= 8192 + 1024);
    assert_stable(&requests);
}

#[tokio::test]
async fn literal_truncation_markers_do_not_change_runtime_completeness() {
    let project = tempdir().unwrap();
    let (_, cut) = truncate_output_lines(OUTPUT_TRUNCATION_MARKER);
    assert!(!cut);
    fs::write(
        project.path().join("large.txt"),
        format!("{OUTPUT_TRUNCATION_MARKER}\n").repeat(40),
    )
    .unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        read_round("first"),
        read_round("second"),
        final_round(),
    ]);
    let outcome = run_agent(
        &provider,
        &tools,
        request(Vec::new()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(!outcome.failed && !outcome.interrupted);
    assert_eq!(outcome.executed_tool_call_count, 2);
    assert!(
        results(&outcome)
            .iter()
            .all(|result| result.truncated == Some(false))
    );
    assert_eq!(
        reference(&content(&provider.requests()[2], "second")).unwrap()["call_id"],
        "first"
    );
}

#[tokio::test]
async fn installed_compaction_drops_old_reference_and_retains_a_new_first_result() {
    let project = tempdir().unwrap();
    fs::write(
        project.path().join("large.txt"),
        format!("{}\n", "x".repeat(900)).repeat(30),
    )
    .unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        read_round("old-first"),
        read_round("old-reference"),
        vec![ScriptStep::Error { status: Some(400), message: r#"responses request failed (HTTP 400): {"error":{"code":"context_length_exceeded"}}"#.into(), retryable: false }],
        read_round("new-first"),
        read_round("new-reference"),
        final_round(),
    ]);
    let compactions = Arc::new(AtomicUsize::new(0));
    let mut req = request(vec![
        ChatMessage::new(ChatRole::User, "Inspect the file"),
        ChatMessage::new(ChatRole::Assistant, "Previous analysis ".repeat(1_000)),
        ChatMessage::new(ChatRole::User, "Continue"),
    ]);
    req.context_budget = Some(ContextBudget::new(100_000, 1_000, true).unwrap());
    req.context_compaction_hook = Some(Arc::new(RecordingCompactionHook {
        calls: compactions.clone(),
        messages: vec![ChatMessage::new(
            ChatRole::User,
            "Earlier observations retained; continue reading",
        )],
        source_version: 1,
    }));
    let outcome = run_agent(&provider, &tools, req, CancellationToken::new())
        .await
        .unwrap();
    assert!(!outcome.failed && !outcome.interrupted);
    assert_eq!(outcome.executed_tool_call_count, 4);
    assert_eq!(compactions.load(Ordering::SeqCst), 1);
    let requests = provider.requests();
    assert_eq!(requests.len(), 6);
    assert_eq!(
        reference(&content(&requests[2], "old-reference")).unwrap()["call_id"],
        "old-first"
    );
    assert!(
        requests[3..]
            .iter()
            .flat_map(|request| &request.messages)
            .all(
                |message| message.tool_call_id.as_deref() != Some("old-first")
                    && !message.content.contains("old-first")
            )
    );
    assert!(reference(&content(&requests[4], "new-first")).is_none());
    assert!(content(&requests[4], "new-first").contains("vega_tool_preview"));
    assert_eq!(
        reference(&content(&requests[5], "new-reference")).unwrap()["call_id"],
        "new-first"
    );
    assert_stable(&requests[..3]);
    assert_stable(&requests[3..]);
}
