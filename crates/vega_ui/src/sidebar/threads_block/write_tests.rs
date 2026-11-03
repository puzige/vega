use super::*;
use gpui_kit::{AppContext, TestAppContext};

fn setup(cx: &mut TestAppContext) -> (tempfile::TempDir, Entity<ThreadsBlock>, Thread) {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(directory.path().join("sidebar.db")).unwrap();
    store.migrate().unwrap();
    let project = projects::create(store.conn(), "project", "project", None).unwrap();
    let thread = conversation::create_thread(&store, &project.id, "model", "confirm").unwrap();
    cx.update(|cx| {
        cx.set_global(VegaStore(Ok(store)));
        cx.set_global(SelectedProject(Some(project.id)));
        cx.set_global(OpenedThread(None));
        cx.set_global(SettingsOpen(false));
    });
    let block = cx.new(ThreadsBlock::new);
    (directory, block, thread)
}

fn stored(cx: &TestAppContext, id: &str) -> Thread {
    cx.read(|cx| {
        conversation::get_thread(cx.global::<VegaStore>().0.as_ref().unwrap(), id).unwrap()
    })
}

fn pending(cx: &TestAppContext) -> usize {
    cx.read(|cx| cx.global::<crate::navigation::TaskMutationState>().pending)
}

#[gpui_kit::test]
fn row_updates_return_under_lock_and_commit_only_after_release(cx: &mut TestAppContext) {
    let (directory, block, thread) = setup(cx);
    let blocker = Store::open_existing(directory.path().join("sidebar.db")).unwrap();
    let permit = vega_store::write_coordination::acquire(
        blocker.conn(),
        vega_store::write_coordination::WriteCategory::Messages,
    )
    .unwrap();
    block.update(cx, |block, cx| {
        block.set_thread_status(&thread.id, ThreadStatus::Archived, cx);
        assert_eq!(block.threads[0].status, ThreadStatus::Active);
        block.set_thread_status(&thread.id, ThreadStatus::Archived, cx);
    });
    assert_eq!(pending(cx), 1);
    assert_eq!(stored(cx, &thread.id).status, ThreadStatus::Active);
    drop(permit);
    cx.run_until_parked();
    assert_eq!(stored(cx, &thread.id).status, ThreadStatus::Archived);
    cx.read(|cx| {
        assert!(block.read(cx).threads.is_empty());
        assert_eq!(block.read(cx).archived[0].status, ThreadStatus::Archived);
    });
    block.update(cx, |block, cx| {
        block.set_thread_status(&thread.id, ThreadStatus::Active, cx)
    });
    cx.run_until_parked();
    let before = stored(cx, &thread.id).updated_at;
    block.update(cx, |block, cx| {
        block.apply_update(
            &thread,
            ThreadUpdate {
                unread: Some(true),
                ..Default::default()
            },
            cx,
        )
    });
    cx.run_until_parked();
    assert!(stored(cx, &thread.id).unread);
    assert_eq!(stored(cx, &thread.id).updated_at, before);
    cx.read(|cx| assert!(cx.global::<OpenedThread>().0.is_none()));
    assert_eq!(pending(cx), 0);
}

#[gpui_kit::test]
fn status_admission_failure_preserves_state_and_retry_commits(cx: &mut TestAppContext) {
    let (_directory, block, thread) = setup(cx);
    let admission = cx.read(|cx| {
        vega_store::thread_admission::try_acquire(
            cx.global::<VegaStore>()
                .0
                .as_ref()
                .unwrap()
                .database_identity(),
            &thread.id,
        )
        .unwrap()
        .unwrap()
    });
    block.update(cx, |block, cx| {
        block.set_thread_status(&thread.id, ThreadStatus::Archived, cx)
    });
    cx.run_until_parked();
    assert_eq!(stored(cx, &thread.id).status, ThreadStatus::Active);
    cx.read(|cx| assert!(block.read(cx).error.is_some()));
    assert_eq!(pending(cx), 0);
    drop(admission);
    block.update(cx, |block, cx| {
        block.set_thread_status(&thread.id, ThreadStatus::Archived, cx)
    });
    cx.run_until_parked();
    assert_eq!(stored(cx, &thread.id).status, ThreadStatus::Archived);
    cx.read(|cx| assert!(block.read(cx).error.is_none()));
}

#[gpui_kit::test]
fn unread_storage_failure_retries_without_recreating_missing_database(cx: &mut TestAppContext) {
    let (directory, block, thread) = setup(cx);
    let path = directory.path().join("sidebar.db");
    let moved = directory.path().join("moved.db");
    std::fs::rename(&path, &moved).unwrap();
    block.update(cx, |block, cx| {
        block.apply_update(
            &thread,
            ThreadUpdate {
                unread: Some(true),
                ..Default::default()
            },
            cx,
        )
    });
    cx.run_until_parked();
    assert!(!path.exists());
    cx.read(|cx| {
        assert!(!block.read(cx).threads[0].unread);
        assert!(block.read(cx).error.is_some());
    });
    std::fs::rename(moved, path).unwrap();
    block.update(cx, |block, cx| {
        block.apply_update(
            &thread,
            ThreadUpdate {
                unread: Some(true),
                ..Default::default()
            },
            cx,
        )
    });
    cx.run_until_parked();
    assert!(stored(cx, &thread.id).unread);
}

#[gpui_kit::test]
fn row_completion_ignores_replaced_database_and_releases_fence(cx: &mut TestAppContext) {
    let (directory, block, thread) = setup(cx);
    block.update(cx, |block, cx| {
        block.set_thread_status(&thread.id, ThreadStatus::Archived, cx)
    });
    let replacement = Store::open(directory.path().join("replacement.db")).unwrap();
    replacement.migrate().unwrap();
    cx.update(|cx| cx.set_global(VegaStore(Ok(replacement))));
    cx.run_until_parked();
    cx.read(|cx| assert_eq!(block.read(cx).threads[0].status, ThreadStatus::Active));
    assert_eq!(pending(cx), 0);
}

#[gpui_kit::test]
fn departed_route_preserves_error_and_dropped_view_releases_row_fence(cx: &mut TestAppContext) {
    let (directory, block, thread) = setup(cx);
    std::fs::rename(
        directory.path().join("sidebar.db"),
        directory.path().join("moved.db"),
    )
    .unwrap();
    block.update(cx, |block, cx| {
        block.set_thread_status(&thread.id, ThreadStatus::Archived, cx);
        cx.set_global(SelectedProject(None));
        block.error = Some("new page failure".to_string().into());
    });
    cx.run_until_parked();
    cx.read(|cx| {
        assert_eq!(
            block.read(cx).error.as_ref().unwrap().render(),
            "new page failure"
        )
    });
    std::fs::rename(
        directory.path().join("moved.db"),
        directory.path().join("sidebar.db"),
    )
    .unwrap();
    block.update(cx, |block, cx| block.toggle_pin(&thread.id, false, cx));
    drop(block);
    cx.run_until_parked();
    assert!(stored(cx, &thread.id).pinned);
    assert_eq!(pending(cx), 0);
}

#[gpui_kit::test]
fn deletion_returns_under_lock_and_never_clears_newly_opened_thread(cx: &mut TestAppContext) {
    let (directory, _block, thread) = setup(cx);
    let other = cx.read(|cx| {
        conversation::create_thread(
            cx.global::<VegaStore>().0.as_ref().unwrap(),
            &thread.project_id,
            "model",
            "confirm",
        )
        .unwrap()
    });
    let sidebar = cx.new(Sidebar::new);
    cx.run_until_parked();
    let blocker = Store::open_existing(directory.path().join("sidebar.db")).unwrap();
    let permit = vega_store::write_coordination::acquire(
        blocker.conn(),
        vega_store::write_coordination::WriteCategory::Messages,
    )
    .unwrap();
    cx.update(|cx| cx.set_global(PendingDeleteConfirm(Some(thread.clone()))));
    sidebar.update(cx, |sidebar, cx| sidebar.confirm_pending_delete(cx));
    assert_eq!(pending(cx), 1);
    assert_eq!(stored(cx, &thread.id).id, thread.id);
    cx.update(|cx| cx.set_global(OpenedThread(Some(other.clone()))));
    drop(permit);
    cx.run_until_parked();
    cx.read(|cx| {
        assert!(
            conversation::get_thread(cx.global::<VegaStore>().0.as_ref().unwrap(), &thread.id)
                .is_err()
        );
        assert_eq!(cx.global::<OpenedThread>().0.as_ref().unwrap().id, other.id);
    });
    assert_eq!(pending(cx), 0);
}

#[gpui_kit::test]
fn deletion_safety_failure_keeps_target_and_allows_confirmation_retry(cx: &mut TestAppContext) {
    let (_directory, _block, thread) = setup(cx);
    let sidebar = cx.new(Sidebar::new);
    cx.run_until_parked();
    let admission = cx.read(|cx| {
        vega_store::thread_admission::try_acquire(
            cx.global::<VegaStore>()
                .0
                .as_ref()
                .unwrap()
                .database_identity(),
            &thread.id,
        )
        .unwrap()
        .unwrap()
    });
    cx.update(|cx| cx.set_global(PendingDeleteConfirm(Some(thread.clone()))));
    sidebar.update(cx, |sidebar, cx| sidebar.confirm_pending_delete(cx));
    cx.run_until_parked();
    assert_eq!(stored(cx, &thread.id).id, thread.id);
    cx.read(|cx| assert!(sidebar.read(cx).sessions_block.read(cx).error.is_some()));
    drop(admission);
    cx.update(|cx| cx.set_global(PendingDeleteConfirm(Some(thread.clone()))));
    sidebar.update(cx, |sidebar, cx| sidebar.confirm_pending_delete(cx));
    cx.run_until_parked();
    cx.read(|cx| {
        assert!(
            conversation::get_thread(cx.global::<VegaStore>().0.as_ref().unwrap(), &thread.id)
                .is_err()
        )
    });
    assert_eq!(pending(cx), 0);
}

#[gpui_kit::test]
fn project_removal_returns_under_lock_and_preserves_unbound_conversations(cx: &mut TestAppContext) {
    let (directory, _block, thread) = setup(cx);
    let block = cx.new(ProjectsBlock::new);
    let blocker = Store::open_existing(directory.path().join("sidebar.db")).unwrap();
    let permit = vega_store::write_coordination::acquire(
        blocker.conn(),
        vega_store::write_coordination::WriteCategory::Messages,
    )
    .unwrap();
    block.update(cx, |block, cx| block.remove_project(&thread.project_id, cx));
    assert_eq!(pending(cx), 1);
    assert_eq!(stored(cx, &thread.id).project_id, thread.project_id);
    drop(permit);
    cx.run_until_parked();
    assert_eq!(stored(cx, &thread.id).project_id, "");
    cx.read(|cx| {
        assert!(
            projects::find(
                cx.global::<VegaStore>().0.as_ref().unwrap().conn(),
                &thread.project_id
            )
            .unwrap()
            .is_none()
        )
    });
    assert_eq!(pending(cx), 0);
}

#[gpui_kit::test]
fn project_removal_failure_retries_and_old_database_completion_does_not_clear_route(
    cx: &mut TestAppContext,
) {
    let (directory, _block, thread) = setup(cx);
    let sidebar = cx.new(Sidebar::new);
    cx.run_until_parked();
    let path = directory.path().join("sidebar.db");
    let moved = directory.path().join("moved.db");
    std::fs::rename(&path, &moved).unwrap();
    sidebar.update(cx, |sidebar, cx| {
        sidebar
            .projects_block
            .update(cx, |block, cx| block.remove_project(&thread.project_id, cx))
    });
    cx.run_until_parked();
    cx.read(|cx| assert!(sidebar.read(cx).projects_block.read(cx).error.is_some()));
    std::fs::rename(moved, path).unwrap();
    sidebar.update(cx, |sidebar, cx| {
        sidebar
            .projects_block
            .update(cx, |block, cx| block.remove_project(&thread.project_id, cx))
    });
    let replacement = Store::open(directory.path().join("replacement.db")).unwrap();
    replacement.migrate().unwrap();
    cx.update(|cx| cx.set_global(VegaStore(Ok(replacement))));
    cx.run_until_parked();
    cx.read(|cx| {
        assert_eq!(
            cx.global::<SelectedProject>().0.as_deref(),
            Some(thread.project_id.as_str())
        )
    });
    assert_eq!(pending(cx), 0);
}

#[gpui_kit::test]
fn archive_pauses_automation_in_the_same_persisted_operation(cx: &mut TestAppContext) {
    let (_directory, block, thread) = setup(cx);
    let automation = cx.read(|cx| {
        vega_conversation::automation::execute(
            cx.global::<VegaStore>().0.as_ref().unwrap(),
            vega_conversation::types::AutomationCommand::Create(
                vega_conversation::types::AutomationCreateInput {
                    thread_id: thread.id.clone(),
                    title: "scheduled".to_string(),
                    prompt: "run".to_string(),
                    kind: vega_conversation::types::AutomationKind::Once,
                    once_at: Some(chrono::Utc::now() + chrono::Duration::days(1)),
                    cron_expr: None,
                    timezone: "UTC".to_string(),
                    status: vega_conversation::types::AutomationStatus::Active,
                    inherit_active_context: false,
                },
            ),
        )
        .unwrap()
    });
    let vega_conversation::types::AutomationCommandResult::Task(automation) = automation else {
        panic!("created task expected");
    };
    block.update(cx, |block, cx| {
        block.set_thread_status(&thread.id, ThreadStatus::Archived, cx)
    });
    cx.run_until_parked();
    cx.read(|cx| {
        let result = vega_conversation::automation::execute(
            cx.global::<VegaStore>().0.as_ref().unwrap(),
            vega_conversation::types::AutomationCommand::Detail {
                task_id: automation.id,
            },
        )
        .unwrap();
        let vega_conversation::types::AutomationCommandResult::Task(task) = result else {
            panic!("task expected");
        };
        assert_eq!(
            task.status,
            vega_conversation::types::AutomationStatus::Paused
        );
    });
    assert_eq!(stored(cx, &thread.id).status, ThreadStatus::Archived);
}
