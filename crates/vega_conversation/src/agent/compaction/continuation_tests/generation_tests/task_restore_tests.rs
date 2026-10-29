use super::*;

#[tokio::test]
async fn compacted_delegation_sources_match_install_reload_and_retained_calls() -> TestResult {
    let (_directory, path, store) = source_fixture()?;
    store.conn().execute(
        "UPDATE tool_calls SET tool='spawn_agent',input_json='{}' WHERE id='call-1'",
        [],
    )?;
    let (source, request) = input(&store)?;
    let provider = ScriptedSummaryProvider::new([SummaryReply::Text("TASK_SUMMARY".into())]);
    let hook =
        ConversationCompactionHook::new(&provider, path.clone(), "thread", "model", None, None)
            .with_full_on_threshold(true);
    let (generated, guard) = hook
        .generate_compaction(request, CancellationToken::new(), &mut None)
        .await
        .map_err(|failure| *failure.error)?;
    let mut expected = crate::agent::pipeline::historical_summary_message("TASK_SUMMARY");
    expected.attach_context_file_references(vec![("call-2".into(), "notes.txt".into())]);
    expected.attach_context_skill_runs(vec!["owner".into()]);
    expected.attach_context_task_sources(vec!["call-1".into()]);
    assert_eq!(generated.result.messages[0], expected);
    let installed = hook
        .install_generated(generated, guard, CancellationToken::new())
        .await
        .map_err(|failure| *failure.error)?;
    let reopened = Store::open(&path)?;
    let checkpoint = latest_checkpoint(reopened.conn(), "thread", "model")?.ok_or("checkpoint")?;
    assert_eq!(
        crate::agent::pipeline::history_from_context_source_with_checkpoint(
            &load_source(reopened.conn(), "thread")?,
            Some(&checkpoint),
            "__none__"
        )?,
        installed.messages
    );
    assert_eq!(load_source(reopened.conn(), "thread")?, source);
    let plan = crate::agent::compaction::projection::prefix_plan(&source, 1, None, None, false)
        .map_err(|failure| *failure.error)?;
    let retained = crate::agent::pipeline::interpret_projection(
        &source,
        &plan,
        "retained",
        &Default::default(),
        "__none__",
    )?;
    assert_eq!(
        retained[0],
        crate::agent::pipeline::historical_summary_message("retained")
    );
    Ok(())
}

#[test]
fn task_reader_binds_exact_execution_parent_call_and_independent_release_state() -> TestResult {
    let (_directory, path, store) = source_fixture()?;
    store.conn().execute(
        "UPDATE tool_calls SET tool='spawn_agent',input_json='{}' WHERE id='call-1'",
        [],
    )?;
    threads::create_standalone(
        store.conn(),
        threads::NewThread {
            id: "child",
            project_id: "",
            title: "Synthetic child",
            mode: "execute",
            permission_mode: crate::types::PermissionMode::ReadOnly.as_str(),
            permission_revision: 0,
            full_access_grant: None,
            reasoning_preferences: None,
            model: "model",
            status: "active",
            pinned: false,
            unread: false,
            created_at: 1,
            updated_at: 1,
        },
    )?;
    for (id, role, seq) in [
        ("child-task", "user", 1),
        ("child-run", "assistant", 2),
        ("child-next-task", "user", 3),
        ("child-next-run", "assistant", 4),
    ] {
        messages::insert(
            store.conn(),
            &messages::MessageRow {
                id: id.into(),
                thread_id: "child".into(),
                seq,
                role: role.into(),
                kind: "text".into(),
                content: if role == "user" {
                    "bounded task"
                } else {
                    "private result excluded"
                }
                .into(),
                status: "done".into(),
                created_at: seq,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )?;
    }
    let first = vega_store::subagents::DelegationRow {
        child_thread_id: "child".into(),
        parent_thread_id: "thread".into(),
        parent_message_id: "owner".into(),
        spawn_call_id: "call-1".into(),
        child_message_id: "child-run".into(),
        name: "worker".into(),
        task: "bounded task".into(),
        state: "completed".into(),
        result: "private result excluded".into(),
        truncated: false,
        created_at: 1,
    };
    vega_store::subagents::insert(store.conn(), &first)?;
    store.conn().execute("INSERT INTO delegated_execution_recovery(child_thread_id,child_message_id,release_state,authority) VALUES('child','child-run','held','{}')", [])?;
    let reader = crate::agent::subagents::context_task_reader(path.clone(), "thread".into());
    let current = reader("call-1").ok_or("current task")?;
    assert_eq!(current.agent_id, "child");
    assert_eq!(current.message_id, "child-run");
    assert_eq!(current.state, "completed");
    assert_eq!(current.release_state, "held");
    assert!(reader("call-2").is_none());
    assert!(
        crate::agent::subagents::context_task_reader(path, "other-parent".into())("call-1")
            .is_none()
    );
    store.conn().execute(
        "UPDATE tool_calls SET tool='followup_agent',input_json='{}' WHERE id='call-2'",
        [],
    )?;
    vega_store::subagents::append(
        store.conn(),
        &vega_store::subagents::DelegationRow {
            spawn_call_id: "call-2".into(),
            child_message_id: "child-next-run".into(),
            state: "running".into(),
            ..first.clone()
        },
        "child-run",
        "child-next-task",
    )?;
    assert_eq!(
        reader("call-1").ok_or("original execution")?.message_id,
        "child-run"
    );
    assert_eq!(reader("call-1").ok_or("original state")?.state, "completed");
    let next = reader("call-2").ok_or("next execution")?;
    assert_eq!(next.message_id, "child-next-run");
    assert_eq!(next.state, "running");
    assert_eq!(next.release_state, "unknown");
    for release in ["released", "quarantined", "recovered_abandoned"] {
        store.conn().execute("UPDATE delegated_execution_recovery SET release_state=?1 WHERE child_message_id='child-run'", [release])?;
        assert_eq!(
            reader("call-1").ok_or("release state")?.release_state,
            release
        );
    }
    store.conn().execute(
        "UPDATE delegated_executions SET state='interrupted' WHERE child_message_id='child-run'",
        [],
    )?;
    assert_eq!(
        reader("call-1").ok_or("interrupted task")?.state,
        "interrupted"
    );
    store.conn().execute(
        "UPDATE tool_calls SET status='failed' WHERE id='call-1'",
        [],
    )?;
    assert!(reader("call-1").is_none());
    store.conn().execute(
        "UPDATE tool_calls SET status='success' WHERE id='call-1'",
        [],
    )?;
    assert!(reader("call-1").is_some());
    store
        .conn()
        .execute("DELETE FROM messages WHERE id='child-run'", [])?;
    assert!(reader("call-1").is_none());
    assert_eq!(checkpoints(&store)?, 0);
    Ok(())
}
