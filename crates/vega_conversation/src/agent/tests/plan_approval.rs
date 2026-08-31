use super::*;

#[tokio::test]
async fn plan_success_is_promoted_only_at_durable_completion() {
    let (store, dir, _project_id) = setup();
    vega_store::threads::set_mode(store.conn(), "thread-1", "plan", 2).unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("1. inspect\n2. change".into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]);
    let run = run_thread_task(
        &store,
        &provider,
        &tools,
        "thread-1",
        "make a plan",
        "system",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let row = messages::find(store.conn(), &run.assistant_message_id)
        .unwrap()
        .unwrap();
    assert_eq!(row.kind, "plan");
    assert_eq!(row.status, "done");
    assert_eq!(row.plan_status.as_deref(), Some("pending"));
    drop(store);
    let reopened = Store::open(dir.path().join("vega.db")).unwrap();
    reopened.migrate().unwrap();
    assert_eq!(
        messages::plans_for_thread(reopened.conn(), "thread-1")
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn plan_cancel_and_provider_error_restart_as_non_plan_history() {
    for (provider, cancel) in [
        (
            MockProvider::new(vec![ScriptStep::Cancelled]),
            CancellationToken::new(),
        ),
        (
            MockProvider::new(vec![ScriptStep::Error {
                status: Some(500),
                message: "mock failure".into(),
                retryable: false,
            }]),
            CancellationToken::new(),
        ),
    ] {
        let (store, dir, _project_id) = setup();
        vega_store::threads::set_mode(store.conn(), "thread-1", "plan", 2).unwrap();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let _ = run_thread_task(
            &store,
            &provider,
            &tools,
            "thread-1",
            "make a plan",
            "system",
            cancel,
        )
        .await;
        let assistant: (String, String) = store
            .conn()
            .query_row(
                "SELECT kind,status FROM messages WHERE role='assistant' ORDER BY seq DESC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(assistant.0, "text");
        assert!(matches!(assistant.1.as_str(), "interrupted" | "failed"));
        drop(store);
        let reopened = Store::open(dir.path().join("vega.db")).unwrap();
        reopened.migrate().unwrap();
        assert!(messages::recent(reopened.conn(), "thread-1", 10).is_ok());
    }
}

#[tokio::test]
async fn approved_instruction_starts_execute_turn_without_duplicate_user_row() {
    let (store, dir, _project_id) = setup();
    vega_store::threads::set_mode(store.conn(), "thread-1", "plan", 2).unwrap();
    messages::insert(
        store.conn(),
        &messages::MessageRow {
            id: "plan".into(),
            thread_id: "thread-1".into(),
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
    messages::complete_plan(store.conn(), "thread-1", "plan", "steps", 3).unwrap();
    let outcome = crate::plans::review_plan(
        &store,
        "thread-1",
        "plan",
        crate::types::PlanReviewAction::Approve,
    )
    .unwrap();
    let crate::types::PlanReviewOutcome::Applied {
        instruction_message_id: Some(instruction_id),
    } = outcome
    else {
        panic!("approval must create an instruction")
    };
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(vec![ProviderEvent::Done {
        stop_reason: StopReason::End,
    }])]);
    let run = run_approved_plan_task(
        &store,
        &provider,
        &tools,
        "thread-1",
        &instruction_id,
        "system",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(run.user_message_id, instruction_id);
    let user_count: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE thread_id='thread-1' AND role='user'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(user_count, 1);
    assert_eq!(provider.requests().len(), 1);
    let replay = run_approved_plan_task(
        &store,
        &provider,
        &tools,
        "thread-1",
        &instruction_id,
        "system",
        CancellationToken::new(),
    )
    .await;
    assert!(replay.is_err());
    assert_eq!(provider.requests().len(), 1);
}

#[tokio::test]
async fn forged_or_tampered_user_rows_cannot_start_approved_turn() {
    let (store, dir, _project_id) = setup();
    messages::insert(
        store.conn(),
        &messages::MessageRow {
            id: "forged".into(),
            thread_id: "thread-1".into(),
            seq: 1,
            role: "user".into(),
            kind: "text".into(),
            content: crate::plans::APPROVAL_INSTRUCTION.into(),
            status: "done".into(),
            created_at: 10,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(vec![ProviderEvent::Done {
        stop_reason: StopReason::End,
    }])]);
    assert!(
        run_approved_plan_task(
            &store,
            &provider,
            &tools,
            "thread-1",
            "forged",
            "system",
            CancellationToken::new(),
        )
        .await
        .is_err()
    );
    assert!(provider.requests().is_empty());

    store
        .conn()
        .execute("DELETE FROM messages WHERE id='forged'", [])
        .unwrap();
    vega_store::threads::set_mode(store.conn(), "thread-1", "plan", 11).unwrap();
    messages::insert(
        store.conn(),
        &messages::MessageRow {
            id: "plan".into(),
            thread_id: "thread-1".into(),
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
    messages::complete_plan(store.conn(), "thread-1", "plan", "steps", 12).unwrap();
    let outcome = crate::plans::review_plan(
        &store,
        "thread-1",
        "plan",
        crate::types::PlanReviewAction::Approve,
    )
    .unwrap();
    let crate::types::PlanReviewOutcome::Applied {
        instruction_message_id: Some(instruction_id),
    } = outcome
    else {
        panic!("approval must create instruction")
    };
    store
        .conn()
        .execute(
            "UPDATE messages SET content='tampered' WHERE id=?1",
            [&instruction_id],
        )
        .unwrap();
    assert!(
        run_approved_plan_task(
            &store,
            &provider,
            &tools,
            "thread-1",
            &instruction_id,
            "system",
            CancellationToken::new(),
        )
        .await
        .is_err()
    );
    assert!(provider.requests().is_empty());
}

#[tokio::test]
async fn approval_winner_executes_after_late_plan_completion_loses() {
    let (store, dir, _project_id) = setup();
    vega_store::threads::set_mode(store.conn(), "thread-1", "plan", 2).unwrap();
    for (id, seq) in [("old", 1), ("late", 2)] {
        messages::insert(
            store.conn(),
            &messages::MessageRow {
                id: id.into(),
                thread_id: "thread-1".into(),
                seq,
                role: "assistant".into(),
                kind: "text".into(),
                content: String::new(),
                status: "streaming".into(),
                created_at: seq,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .unwrap();
        if id == "old" {
            messages::complete_plan(store.conn(), "thread-1", id, "old plan", 3).unwrap();
        }
    }
    let outcome = crate::plans::review_plan(
        &store,
        "thread-1",
        "old",
        crate::types::PlanReviewAction::Approve,
    )
    .unwrap();
    let crate::types::PlanReviewOutcome::Applied {
        instruction_message_id: Some(instruction_id),
    } = outcome
    else {
        panic!("approval must create instruction")
    };
    assert!(messages::complete_plan(store.conn(), "thread-1", "late", "late plan", 4).is_err());
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(vec![ProviderEvent::Done {
        stop_reason: StopReason::End,
    }])]);
    run_approved_plan_task(
        &store,
        &provider,
        &tools,
        "thread-1",
        &instruction_id,
        "system",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(provider.requests().len(), 1);
    assert!(
        run_approved_plan_task(
            &store,
            &provider,
            &tools,
            "thread-1",
            &instruction_id,
            "system",
            CancellationToken::new(),
        )
        .await
        .is_err()
    );
    assert_eq!(provider.requests().len(), 1);
}

#[tokio::test]
async fn concurrent_approved_instruction_claim_starts_provider_once() {
    let (store, dir, _project_id) = setup();
    vega_store::threads::set_mode(store.conn(), "thread-1", "plan", 2).unwrap();
    messages::insert(
        store.conn(),
        &messages::MessageRow {
            id: "plan".into(),
            thread_id: "thread-1".into(),
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
    messages::complete_plan(store.conn(), "thread-1", "plan", "steps", 3).unwrap();
    let outcome = crate::plans::review_plan(
        &store,
        "thread-1",
        "plan",
        crate::types::PlanReviewAction::Approve,
    )
    .unwrap();
    let crate::types::PlanReviewOutcome::Applied {
        instruction_message_id: Some(instruction_id),
    } = outcome
    else {
        panic!("approval must create instruction")
    };
    drop(store);
    let first_store = Store::open(dir.path().join("vega.db")).unwrap();
    let second_store = Store::open(dir.path().join("vega.db")).unwrap();
    first_store
        .conn()
        .busy_timeout(Duration::from_secs(5))
        .unwrap();
    second_store
        .conn()
        .busy_timeout(Duration::from_secs(5))
        .unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(vec![ProviderEvent::Done {
        stop_reason: StopReason::End,
    }])]);
    let first_provider = provider.clone();
    let second_provider = provider.clone();
    let first_tools = vega_tools::Tools::new(dir.path()).unwrap();
    let second_tools = vega_tools::Tools::new(dir.path()).unwrap();
    let first_id = instruction_id.clone();
    let second_id = instruction_id.clone();
    let first = async {
        run_approved_plan_task(
            &first_store,
            &first_provider,
            &first_tools,
            "thread-1",
            &first_id,
            "system",
            CancellationToken::new(),
        )
        .await
    };
    let second = async {
        run_approved_plan_task(
            &second_store,
            &second_provider,
            &second_tools,
            "thread-1",
            &second_id,
            "system",
            CancellationToken::new(),
        )
        .await
    };
    let (first, second) = tokio::join!(first, second);
    assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
    assert_eq!(provider.requests().len(), 1);
}

#[tokio::test]
async fn ambiguous_same_timestamp_approved_plans_reject_instruction() {
    let (store, dir, _project_id) = setup();
    vega_store::threads::set_mode(store.conn(), "thread-1", "plan", 2).unwrap();
    messages::insert(
        store.conn(),
        &messages::MessageRow {
            id: "plan".into(),
            thread_id: "thread-1".into(),
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
    messages::complete_plan(store.conn(), "thread-1", "plan", "steps", 3).unwrap();
    let outcome = crate::plans::review_plan(
        &store,
        "thread-1",
        "plan",
        crate::types::PlanReviewAction::Approve,
    )
    .unwrap();
    let crate::types::PlanReviewOutcome::Applied {
        instruction_message_id: Some(instruction_id),
    } = outcome
    else {
        panic!("approval must create instruction")
    };
    let timestamp: i64 = store
        .conn()
        .query_row(
            "SELECT created_at FROM messages WHERE id=?1",
            [&instruction_id],
            |row| row.get(0),
        )
        .unwrap();
    store
            .conn()
            .execute(
                "INSERT INTO messages (id,thread_id,seq,role,kind,content,status,created_at,plan_status,plan_reviewed_at) \
                 VALUES ('ambiguous','thread-1',0,'assistant','plan','other','done',0,'approved',?1)",
                [timestamp],
            )
            .unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(vec![ProviderEvent::Done {
        stop_reason: StopReason::End,
    }])]);
    assert!(
        run_approved_plan_task(
            &store,
            &provider,
            &tools,
            "thread-1",
            &instruction_id,
            "system",
            CancellationToken::new(),
        )
        .await
        .is_err()
    );
    assert!(provider.requests().is_empty());
}
