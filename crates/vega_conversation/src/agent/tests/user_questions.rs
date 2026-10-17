use super::*;
use vega_runtime::{
    RuntimeUserQuestionHook, RuntimeUserQuestionRequest, RuntimeUserQuestionResponse,
    USER_QUESTION_TOOL, UserQuestionAnswers, UserQuestionInput,
};

fn questions() -> UserQuestionInput {
    UserQuestionInput::from_json(r#"{"questions":[{"id":"layout","header":"Layout","question":"Which layout?","options":[{"label":"Keep (Recommended)","description":"Keep it"},{"label":"Replace","description":"Replace it"}]},{"id":"scope","header":"Scope","question":"Which scope?","options":[{"label":"Small","description":"One page"},{"label":"Large","description":"All pages"}]}]}"#).unwrap()
}

struct StoredQuestionHook {
    database: PathBuf,
    count: Arc<AtomicUsize>,
}

impl RuntimeUserQuestionHook for StoredQuestionHook {
    fn request(
        &self,
        request: RuntimeUserQuestionRequest,
        _: CancellationToken,
    ) -> BoxFuture<'static, RuntimeUserQuestionResponse> {
        let store = Store::open(&self.database).unwrap();
        let (status, input): (String, String) = store
            .conn()
            .query_row(
                "SELECT status, input_json FROM tool_calls WHERE id = ?1",
                [&request.call_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, "running");
        assert_eq!(UserQuestionInput::from_json(&input).unwrap(), request.input);
        self.count.fetch_add(1, Ordering::SeqCst);
        let answers = UserQuestionAnswers::from_json(r#"{"answers":{"layout":{"answers":["Keep (Recommended)","user_note: Keep spacing"]},"scope":{"answers":[]}}}"#, &request.input).unwrap();
        async move { RuntimeUserQuestionResponse::Answered(answers) }.boxed()
    }
}

fn question_provider() -> MockProvider {
    MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "question-1".into(),
                name: USER_QUESTION_TOOL.into(),
                input_json: questions().to_json().unwrap(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("Recorded".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
    ])
}

#[tokio::test]
async fn question_answer_is_durable_before_event_and_provider_continuation_and_replays_readonly() {
    let (store, dir, _) = setup();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let config =
        PersistenceActorConfig::default().with_user_questions(Arc::new(StoredQuestionHook {
            database: dir.path().join("vega.db"),
            count: count.clone(),
        }));
    let provider = question_provider();
    let mut finished = false;
    run_thread_task_with_pricing(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Choose layout",
        "System",
        CancellationToken::new(),
        &RejectPermissionHook,
        |event| {
            if let ConversationEvent::ToolCallFinished { call_id, result } = event {
                let (status, output): (String, String) = store
                    .conn()
                    .query_row(
                        "SELECT status, output_text FROM tool_calls WHERE id = ?1",
                        [call_id],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .unwrap();
                assert_eq!(status, "success");
                assert_eq!(output, result.output);
                finished = true;
            }
            Ok(())
        },
        config.clone(),
        None,
        None,
    )
    .await
    .unwrap();
    assert!(finished);
    assert_eq!(count.load(Ordering::SeqCst), 1);
    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    let output = &requests[1]
        .messages
        .iter()
        .find(|message| message.role == vega_runtime::ChatRole::Tool)
        .unwrap()
        .content;
    let answers = UserQuestionAnswers::from_json(output, &questions()).unwrap();
    assert_eq!(
        answers.answers["layout"].answers,
        ["Keep (Recommended)", "user_note: Keep spacing"]
    );
    assert!(answers.answers["scope"].answers.is_empty());
    let history = crate::history::latest_history_page(&store, "thread-1", 20).unwrap();
    assert!(history.entries.iter().any(|entry| matches!(entry, crate::history::HistoryEntry::Tool { result: Some(crate::types::ToolCardResultProjection::UserQuestion { output, .. }), .. } if output.text().contains("Which layout?") && output.text().contains("Keep spacing") && output.text().contains("Skipped"))));
    let replay = question_provider();
    let run = run_thread_task_with_pricing(
        &store,
        &replay,
        &tools,
        "thread-1",
        "Continue",
        "System",
        CancellationToken::new(),
        &RejectPermissionHook,
        |_| Ok(()),
        config,
        None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert!(run.events.iter().any(|event| matches!(event, ConversationEvent::ToolCallFinished { result, .. } if result.reused && result.status == ToolCallStatus::Success)));
}

#[tokio::test]
async fn question_finished_persistence_failure_stops_before_second_provider_call() {
    let (store, dir, _) = setup();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let config = PersistenceActorConfig {
        fail_event: Some(InjectedPersistenceFailure::Finished),
        ..Default::default()
    }
    .with_user_questions(Arc::new(StoredQuestionHook {
        database: dir.path().join("vega.db"),
        count: count.clone(),
    }));
    let provider = question_provider();
    let mut success_events = 0;
    let result = run_thread_task_with_pricing(&store, &provider, &tools, "thread-1", "Choose layout", "System", CancellationToken::new(), &RejectPermissionHook, |event| {
        if matches!(event, ConversationEvent::ToolCallFinished { result, .. } if result.status == ToolCallStatus::Success) { success_events += 1; }
        Ok(())
    }, config, None, None).await;
    assert!(result.is_err());
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert_eq!(provider.requests().len(), 1);
    assert_eq!(success_events, 0);
    let successes: i64 = store.conn().query_row("SELECT COUNT(*) FROM tool_calls WHERE tool = 'ask_user_question' AND status = 'success'", [], |row| row.get(0)).unwrap();
    assert_eq!(successes, 0);
}

#[tokio::test]
async fn question_nonterminal_history_is_interrupted_and_never_a_live_request() {
    let (store, dir, _) = setup();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = question_provider();
    let config =
        PersistenceActorConfig::default().with_user_questions(Arc::new(StoredQuestionHook {
            database: dir.path().join("vega.db"),
            count: Arc::new(AtomicUsize::new(0)),
        }));
    run_thread_task_with_pricing(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Choose layout",
        "System",
        CancellationToken::new(),
        &RejectPermissionHook,
        |_| Ok(()),
        config,
        None,
        None,
    )
    .await
    .unwrap();
    store.conn().execute("UPDATE tool_calls SET status='running', output_text=NULL, finished_at=NULL WHERE id='question-1'", []).unwrap();
    let history = crate::history::latest_history_page(&store, "thread-1", 20).unwrap();
    assert!(history.entries.iter().any(|entry| matches!(entry, crate::history::HistoryEntry::Tool { status: ToolCallStatus::Cancelled, result: Some(crate::types::ToolCardResultProjection::UserQuestion { output, .. }), .. } if output.text().contains("Interrupted"))));
}

#[test]
fn question_history_rejects_truncation_and_debug_never_contains_answers() {
    let input = crate::types::ToolCardInputProjection::UserQuestion {
        input: Ok(questions()),
    };
    let mut result = crate::types::ToolResult { status: ToolCallStatus::Success, output: r#"{"answers":{"layout":{"answers":["Keep (Recommended)","user_note: Private note"]},"scope":{"answers":[]}}}"#.into(), reused: false, exit_code: None, duration_ms: None, truncated: Some(false), invalid: None };
    let projection = crate::types::tool_card_result_projection(Some(&input), &result);
    assert!(matches!(
        projection,
        crate::types::ToolCardResultProjection::UserQuestion { .. }
    ));
    assert!(!format!("{projection:?}").contains("Private note"));
    assert!(!format!("{input:?}").contains("Which layout"));
    result.truncated = Some(true);
    assert_eq!(
        crate::types::tool_card_result_projection(Some(&input), &result),
        crate::types::ToolCardResultProjection::Corrupt
    );
}
