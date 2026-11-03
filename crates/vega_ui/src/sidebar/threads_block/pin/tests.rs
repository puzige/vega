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

fn stored_pin(cx: &TestAppContext, id: &str) -> bool {
    cx.read(|cx| {
        conversation::get_thread(cx.global::<VegaStore>().0.as_ref().unwrap(), id)
            .unwrap()
            .pinned
    })
}

#[gpui_kit::test]
fn pin_returns_before_write_permit_and_rejects_duplicate_submission(cx: &mut TestAppContext) {
    let (directory, block, thread) = setup(cx);
    let blocker = Store::open_existing(directory.path().join("sidebar.db")).unwrap();
    let permit = vega_store::write_coordination::acquire(
        blocker.conn(),
        vega_store::write_coordination::WriteCategory::Messages,
    )
    .unwrap();
    block.update(cx, |block, cx| {
        block.toggle_pin(&thread.id, false, cx);
        assert_eq!(block.pin_pending.as_deref(), Some(thread.id.as_str()));
        assert_eq!(
            cx.global::<crate::navigation::TaskMutationState>().pending,
            1
        );
        assert!(!block.threads[0].pinned);
        block.toggle_pin(&thread.id, false, cx);
        assert_eq!(
            cx.global::<crate::navigation::TaskMutationState>().pending,
            1
        );
    });
    assert!(!stored_pin(cx, &thread.id));
    drop(permit);
    cx.run_until_parked();
    assert!(stored_pin(cx, &thread.id));
    cx.read(|cx| {
        assert!(block.read(cx).threads[0].pinned);
        assert!(block.read(cx).pin_pending.is_none());
        assert_eq!(
            cx.global::<crate::navigation::TaskMutationState>().pending,
            0
        );
    });
    block.update(cx, |block, cx| block.toggle_pin(&thread.id, true, cx));
    cx.run_until_parked();
    assert!(!stored_pin(cx, &thread.id));
    cx.read(|cx| assert!(!block.read(cx).threads[0].pinned));
}

#[gpui_kit::test]
fn failed_pin_preserves_state_and_allows_retry(cx: &mut TestAppContext) {
    let (directory, block, thread) = setup(cx);
    let path = directory.path().join("sidebar.db");
    let moved = directory.path().join("moved.db");
    std::fs::rename(&path, &moved).unwrap();
    block.update(cx, |block, cx| block.toggle_pin(&thread.id, false, cx));
    cx.run_until_parked();
    assert!(!path.exists());
    cx.read(|cx| {
        assert!(!block.read(cx).threads[0].pinned);
        assert!(block.read(cx).error.is_some());
        assert!(block.read(cx).pin_pending.is_none());
        assert_eq!(
            cx.global::<crate::navigation::TaskMutationState>().pending,
            0
        );
    });
    std::fs::rename(moved, path).unwrap();
    block.update(cx, |block, cx| block.toggle_pin(&thread.id, false, cx));
    cx.run_until_parked();
    assert!(stored_pin(cx, &thread.id));
    cx.read(|cx| assert!(block.read(cx).error.is_none()));
}

#[gpui_kit::test]
fn database_switch_rejects_old_pin_completion(cx: &mut TestAppContext) {
    let (directory, block, thread) = setup(cx);
    block.update(cx, |block, cx| block.toggle_pin(&thread.id, false, cx));
    let replacement = Store::open(directory.path().join("replacement.db")).unwrap();
    replacement.migrate().unwrap();
    cx.update(|cx| cx.set_global(VegaStore(Ok(replacement))));
    cx.run_until_parked();
    cx.read(|cx| {
        assert!(!block.read(cx).threads[0].pinned);
        assert!(block.read(cx).pin_pending.is_none());
        assert_eq!(
            cx.global::<crate::navigation::TaskMutationState>().pending,
            0
        );
    });
}

#[gpui_kit::test]
fn dropped_sidebar_still_releases_pin_mutation_fence(cx: &mut TestAppContext) {
    let (_directory, block, thread) = setup(cx);
    block.update(cx, |block, cx| block.toggle_pin(&thread.id, false, cx));
    drop(block);
    cx.run_until_parked();
    assert!(stored_pin(cx, &thread.id));
    cx.read(|cx| {
        assert_eq!(
            cx.global::<crate::navigation::TaskMutationState>().pending,
            0
        )
    });
}

#[gpui_kit::test]
fn pin_updates_organization_snapshot_without_navigation(cx: &mut TestAppContext) {
    let (_directory, block, thread) = setup(cx);
    let projects = cx.new(ProjectsBlock::new);
    block.update(cx, |block, cx| block.enable_organization(projects, cx));
    cx.run_until_parked();
    block.update(cx, |block, cx| block.toggle_pin(&thread.id, false, cx));
    cx.run_until_parked();
    cx.read(|cx| {
        let block = block.read(cx);
        assert!(
            block
                .threads
                .iter()
                .find(|row| row.id == thread.id)
                .unwrap()
                .pinned
        );
        assert!(
            block
                .organization
                .as_ref()
                .unwrap()
                .snapshot
                .as_ref()
                .unwrap()
                .threads
                .iter()
                .find(|row| row.id == thread.id)
                .unwrap()
                .pinned
        );
        assert_eq!(
            cx.global::<SelectedProject>().0.as_deref(),
            Some(thread.project_id.as_str())
        );
        assert!(cx.global::<OpenedThread>().0.is_none());
    });
}

#[gpui_kit::test]
fn pin_failure_after_route_change_preserves_current_error(cx: &mut TestAppContext) {
    let (directory, block, thread) = setup(cx);
    std::fs::rename(
        directory.path().join("sidebar.db"),
        directory.path().join("moved.db"),
    )
    .unwrap();
    block.update(cx, |block, cx| {
        block.toggle_pin(&thread.id, false, cx);
        cx.set_global(SelectedProject(None));
        block.error = Some("current route error".to_string().into());
    });
    cx.run_until_parked();
    cx.read(|cx| {
        assert_eq!(
            block.read(cx).error.as_ref().unwrap().render(),
            "current route error"
        );
        assert!(cx.global::<SelectedProject>().0.is_none());
        assert_eq!(
            cx.global::<crate::navigation::TaskMutationState>().pending,
            0
        );
    });
}
