use super::*;

fn input() -> UserQuestionInput {
    UserQuestionInput::from_json(r#"{"questions":[{"id":"approach","header":"Approach","question":"Which approach?","options":[{"label":"Keep (Recommended)","description":"Keep the layout"},{"label":"Change","description":"Replace the layout"}]}]}"#).unwrap()
}

struct AnswerHook(Arc<AtomicUsize>);

impl RuntimeUserQuestionHook for AnswerHook {
    fn request(
        &self,
        request: RuntimeUserQuestionRequest,
        _: CancellationToken,
    ) -> BoxFuture<'static, RuntimeUserQuestionResponse> {
        self.0.fetch_add(1, Ordering::SeqCst);
        let answers = UserQuestionAnswers {
            answers: request
                .input
                .questions
                .iter()
                .map(|question| {
                    (
                        question.id.clone(),
                        UserQuestionAnswer {
                            answers: vec![
                                question.options[0].label.clone(),
                                "user_note: Keep spacing".into(),
                            ],
                        },
                    )
                })
                .collect(),
        };
        async move { RuntimeUserQuestionResponse::Answered(answers) }.boxed()
    }
}

fn provider(calls: Vec<ProviderEvent>) -> MockProvider {
    let mut events = calls;
    events.push(ProviderEvent::Done {
        stop_reason: StopReason::ToolUse,
    });
    MockProvider::new_rounds(vec![
        vec![ScriptStep::events(events)],
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("Acknowledged".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
    ])
}

fn question_call(id: &str) -> ProviderEvent {
    ProviderEvent::ToolUse {
        id: id.into(),
        name: USER_QUESTION_TOOL.into(),
        input_json: input().to_json().unwrap(),
    }
}

#[test]
fn strict_input_and_answer_contract() {
    let input = input();
    let valid = serde_json::to_value(&input).unwrap();
    for (pointer, value) in [
        ("/questions", serde_json::json!([])),
        ("/questions/0/id", serde_json::json!("Wrong ID")),
        ("/questions/0/header", serde_json::json!("Header too long")),
        ("/questions/0/question", serde_json::json!(" ")),
        ("/questions/0/options", serde_json::json!([])),
        ("/questions/0/options/0/label", serde_json::json!("Other")),
    ] {
        let mut changed = valid.clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        assert!(UserQuestionInput::from_json(&changed.to_string()).is_err());
    }
    let mut duplicate = input.clone();
    duplicate.questions.push(duplicate.questions[0].clone());
    assert!(duplicate.validate().is_err());
    let mut unknown = valid.clone();
    unknown["questions"][0]["secret"] = serde_json::json!(true);
    assert!(UserQuestionInput::from_json(&unknown.to_string()).is_err());
    let mut unknown = valid.clone();
    unknown["questions"][0]["options"][0]["preview"] = serde_json::json!("ignored?");
    assert!(UserQuestionInput::from_json(&unknown.to_string()).is_err());
    for raw in [
        r#"{"answers":{"approach":{"answers":["Keep (Recommended)","Change"]}}}"#,
        r#"{"answers":{"unknown":{"answers":[]}}}"#,
        r#"{"answers":{"approach":{"answers":["invented"]}}}"#,
        r#"{"answers":{"approach":{"answers":["user_note: "]}}}"#,
        r#"{"answers":{"approach":{"answers":[]},"approach":{"answers":[]}}}"#,
    ] {
        assert!(UserQuestionAnswers::from_json(raw, &input).is_err());
    }
    for values in [
        vec![],
        vec!["user_note: Custom"],
        vec!["Keep (Recommended)", "user_note: Keep spacing"],
    ] {
        let raw = serde_json::json!({"answers":{"approach":{"answers":values}}}).to_string();
        assert!(UserQuestionAnswers::from_json(&raw, &input).is_ok());
    }
    let oversized = serde_json::json!({"answers":{"approach":{"answers":[format!("user_note: {}", "x".repeat(USER_QUESTION_MAX_BYTES))]}}}).to_string();
    assert!(UserQuestionAnswers::from_json(&oversized, &input).is_err());
    assert!(!format!("{input:?}").contains("Which approach"));
}

#[tokio::test]
async fn answer_continues_all_modes_without_permission_approval() {
    for mode in [
        RuntimeRunMode::Ask,
        RuntimeRunMode::Plan,
        RuntimeRunMode::Execute,
    ] {
        for permission in [
            RuntimePermissionMode::ReadOnly,
            RuntimePermissionMode::Confirm,
            RuntimePermissionMode::Auto,
            RuntimePermissionMode::FullAccess,
        ] {
            let dir = tempdir().unwrap();
            let tools = vega_tools::Tools::new(dir.path()).unwrap();
            let calls = Arc::new(AtomicUsize::new(0));
            let mut req = request(Vec::new());
            req.tool_config = tool_config(mode, permission, dir.path().join("checkpoints"))
                .with_user_questions(Some(Arc::new(AnswerHook(calls.clone()))));
            let provider = provider(vec![question_call("question-1")]);
            let run = run_agent(&provider, &tools, req, CancellationToken::new())
                .await
                .unwrap();
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            assert_eq!(provider.requests().len(), 2);
            assert_eq!(run.executed_tool_call_count, 1);
            let requests = provider.requests();
            let result = requests[1]
                .messages
                .iter()
                .find(|message| message.role == ChatRole::Tool)
                .unwrap();
            let answers = UserQuestionAnswers::from_json(&result.content, &input()).unwrap();
            assert_eq!(
                answers.answers["approach"].answers,
                ["Keep (Recommended)", "user_note: Keep spacing"]
            );
        }
    }
}

#[tokio::test]
async fn mixed_batches_reject_all_members_without_wait_or_mutation() {
    for names in [
        vec!["question", "write"],
        vec!["write", "question"],
        vec!["question", "question"],
    ] {
        let dir = tempdir().unwrap();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut req = request(Vec::new());
        req.tool_config = tool_config(
            RuntimeRunMode::Execute,
            RuntimePermissionMode::FullAccess,
            dir.path().join("checkpoints"),
        )
        .with_user_questions(Some(Arc::new(AnswerHook(calls.clone()))));
        let provider = provider(
            names
                .iter()
                .enumerate()
                .map(|(index, name)| {
                    if *name == "question" {
                        question_call(&format!("q-{index}"))
                    } else {
                        ProviderEvent::ToolUse {
                            id: format!("w-{index}"),
                            name: "write".into(),
                            input_json: r#"{"path":"never-created.txt","content":"never"}"#.into(),
                        }
                    }
                })
                .collect(),
        );
        let run = run_agent(&provider, &tools, req, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(run.executed_tool_call_count, 0);
        assert!(!dir.path().join("never-created.txt").exists());
        let results = run
            .events
            .iter()
            .filter_map(|event| {
                if let RuntimeEvent::ToolCallFinished(result) = event {
                    Some(result)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(results.len(), 2);
        assert!(
            results
                .iter()
                .all(|result| result.status == RuntimeToolStatus::Rejected
                    && result.output == USER_QUESTION_BATCH_REJECTED)
        );
    }
}

#[tokio::test]
async fn missing_hook_and_invalid_input_are_recoverable_not_empty_answers() {
    for raw in [input().to_json().unwrap(), "{\"questions\":[]}".into()] {
        let dir = tempdir().unwrap();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let provider = provider(vec![ProviderEvent::ToolUse {
            id: "q".into(),
            name: USER_QUESTION_TOOL.into(),
            input_json: raw,
        }]);
        let run = run_agent(
            &provider,
            &tools,
            request(Vec::new()),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(provider.requests().len(), 2);
        assert!(run.events.iter().any(|event| matches!(event, RuntimeEvent::ToolCallFinished(result) if matches!(result.status, RuntimeToolStatus::Failed | RuntimeToolStatus::Rejected) && result.output.starts_with("Tool error:"))));
    }
}
