use super::*;

#[tokio::test]
async fn compacted_latest_plan_source_survives_install_and_reload_without_approval() -> TestResult {
    let (_directory, path, store) = source_fixture()?;
    store.conn().execute("UPDATE messages SET kind='plan',status='done',content='Original bounded plan',plan_status='pending' WHERE id='owner'", [])?;
    let (source, request) = input(&store)?;
    let provider = ScriptedSummaryProvider::new([SummaryReply::Text("PLAN_SUMMARY".into())]);
    let hook =
        ConversationCompactionHook::new(&provider, path.clone(), "thread", "model", None, None)
            .with_full_on_threshold(true);
    let (generated, guard) = hook
        .generate_compaction(request, CancellationToken::new(), &mut None)
        .await
        .map_err(|failure| *failure.error)?;
    let mut expected = crate::agent::pipeline::historical_summary_message("PLAN_SUMMARY");
    expected.attach_context_file_references(vec![("call-2".into(), "notes.txt".into())]);
    expected.attach_context_skill_runs(vec!["owner".into()]);
    expected.attach_context_plan_source("owner".into());
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
    assert!(crate::plans::recoverable_approved_instruction(&reopened, "thread")?.is_none());
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
fn current_plan_reader_rechecks_review_shape_thread_latest_identity_and_bounded_body() -> TestResult
{
    let (_directory, path, store) = source_fixture()?;
    store.conn().execute("UPDATE messages SET kind='plan',status='done',content='Historical plan body',plan_status='pending' WHERE id='owner'", [])?;
    let reader = crate::plans::context_plan_reader(path.clone(), "thread".into());
    let pending = reader("owner").ok_or("plan")?;
    assert_eq!(pending.status, "pending");
    assert_eq!(pending.content.as_deref(), Some("Historical plan body"));
    assert!(pending.reviewed_at.is_none());
    assert!(crate::plans::context_plan_reader(path, "other-thread".into())("owner").is_none());
    for status in ["approved", "changes_requested", "abandoned"] {
        store.conn().execute(
            "UPDATE messages SET plan_status=?1,plan_reviewed_at=10 WHERE id='owner'",
            [status],
        )?;
        assert_eq!(reader("owner").ok_or("reviewed plan")?.status, status);
    }
    store.conn().execute(
        "UPDATE messages SET plan_status='approved',plan_review_note='invalid' WHERE id='owner'",
        [],
    )?;
    assert!(reader("owner").is_none());
    store.conn().execute("UPDATE messages SET plan_status='changes_requested',plan_review_note=?1,content=?2 WHERE id='owner'", ("n".repeat(4097), "x".repeat(20001)))?;
    let bounded = reader("owner").ok_or("reference")?;
    assert!(bounded.content.is_none());
    assert!(bounded.review_note.is_none());
    store.conn().execute("INSERT INTO messages(id,thread_id,seq,role,kind,content,status,created_at,plan_status) VALUES('new-plan','thread',5,'assistant','plan','new plan','done',5,'pending')", [])?;
    assert!(reader("owner").is_none());
    assert_eq!(
        reader("new-plan").ok_or("new plan")?.content.as_deref(),
        Some("new plan")
    );
    store
        .conn()
        .execute("DELETE FROM messages WHERE id='new-plan'", [])?;
    assert!(reader("new-plan").is_none());
    Ok(())
}
