use super::VegaWindow;
use gpui_kit::{AppContext, Entity, TestAppContext};
use vega_conversation::{threads as conversation, types::Thread};
use vega_ui::{
    settings::SettingsOpen,
    sidebar::{OpenedThread, SelectedProject, VegaStore},
};

fn setup(cx: &mut TestAppContext) -> (tempfile::TempDir, Entity<VegaWindow>, Thread) {
    let directory = tempfile::tempdir().unwrap();
    let store = vega_store::Store::open(directory.path().join("navigation.db")).unwrap();
    store.migrate().unwrap();
    let project = vega_store::projects::create(store.conn(), "project", "project", None).unwrap();
    let thread = conversation::create_thread(&store, &project.id, "model", "confirm").unwrap();
    conversation::update_thread(
        &store,
        &thread.id,
        &vega_conversation::types::ThreadUpdate {
            unread: Some(true),
            ..Default::default()
        },
    )
    .unwrap();
    let thread = conversation::get_thread(&store, &thread.id).unwrap();
    cx.update(|cx| {
        cx.set_global(VegaStore(Ok(store)));
        cx.set_global(SelectedProject(Some(project.id)));
        cx.set_global(OpenedThread(Some(thread.clone())));
        cx.set_global(SettingsOpen(false));
    });
    let root = cx.new(|cx| VegaWindow::new_with_services(cx, false));
    cx.run_until_parked();
    (directory, root, thread)
}

fn pending(cx: &TestAppContext) -> usize {
    cx.read(|cx| {
        cx.global::<vega_ui::navigation::TaskMutationState>()
            .pending
    })
}

#[gpui_kit::test]
fn navigation_visit_returns_while_writer_is_busy_then_updates_unread_and_time(
    cx: &mut TestAppContext,
) {
    let (directory, root, thread) = setup(cx);
    let blocker = vega_store::Store::open_existing(directory.path().join("navigation.db")).unwrap();
    let permit = vega_store::write_coordination::acquire(
        blocker.conn(),
        vega_store::write_coordination::WriteCategory::Messages,
    )
    .unwrap();
    root.update(cx, |root, cx| {
        root.record_navigation_visit(thread.clone(), cx)
    });
    assert_eq!(pending(cx), 1);
    cx.read(|cx| assert!(cx.global::<OpenedThread>().0.as_ref().unwrap().unread));
    drop(permit);
    cx.run_until_parked();
    cx.read(|cx| {
        let stored =
            conversation::get_thread(cx.global::<VegaStore>().0.as_ref().unwrap(), &thread.id)
                .unwrap();
        assert!(!stored.unread);
        assert!(stored.updated_at >= thread.updated_at);
        assert!(!cx.global::<OpenedThread>().0.as_ref().unwrap().unread);
    });
    assert_eq!(pending(cx), 0);
}

#[gpui_kit::test]
fn visit_failure_keeps_route_and_retry_saves_without_recreating_database(cx: &mut TestAppContext) {
    let (directory, root, thread) = setup(cx);
    let path = directory.path().join("navigation.db");
    let moved = directory.path().join("moved.db");
    std::fs::rename(&path, &moved).unwrap();
    root.update(cx, |root, cx| {
        root.record_navigation_visit(thread.clone(), cx)
    });
    cx.run_until_parked();
    assert!(!path.exists());
    cx.read(|cx| {
        assert!(root.read(cx).navigation.error.is_some());
        assert_eq!(
            cx.global::<OpenedThread>().0.as_ref().unwrap().id,
            thread.id
        );
        assert!(cx.global::<OpenedThread>().0.as_ref().unwrap().unread);
    });
    std::fs::rename(moved, path).unwrap();
    root.update(cx, |root, cx| {
        root.record_navigation_visit(thread.clone(), cx)
    });
    cx.run_until_parked();
    cx.read(|cx| {
        assert!(root.read(cx).navigation.error.is_none());
        assert!(!cx.global::<OpenedThread>().0.as_ref().unwrap().unread);
    });
    assert_eq!(pending(cx), 0);
}

#[gpui_kit::test]
fn visit_completion_does_not_overwrite_a_departed_and_returned_route(cx: &mut TestAppContext) {
    let (_directory, root, thread) = setup(cx);
    root.update(cx, |root, cx| {
        root.record_navigation_visit(thread.clone(), cx);
        root.navigation.generation = root.navigation.generation.wrapping_add(2);
        root.navigation.error = Some("new route failure");
    });
    cx.run_until_parked();
    cx.read(|cx| {
        assert_eq!(root.read(cx).navigation.error, Some("new route failure"));
        assert!(cx.global::<OpenedThread>().0.as_ref().unwrap().unread);
    });
    assert_eq!(pending(cx), 0);
}

#[gpui_kit::test]
fn visit_completion_ignores_replaced_database_and_dropped_window_releases_fence(
    cx: &mut TestAppContext,
) {
    let (directory, root, thread) = setup(cx);
    root.update(cx, |root, cx| {
        root.record_navigation_visit(thread.clone(), cx)
    });
    let replacement = vega_store::Store::open(directory.path().join("replacement.db")).unwrap();
    replacement.migrate().unwrap();
    cx.update(|cx| cx.set_global(VegaStore(Ok(replacement))));
    cx.run_until_parked();
    cx.read(|cx| assert!(cx.global::<OpenedThread>().0.as_ref().unwrap().unread));
    assert_eq!(pending(cx), 0);
    let original =
        vega_store::Store::open_existing(directory.path().join("navigation.db")).unwrap();
    cx.update(|cx| cx.set_global(VegaStore(Ok(original))));
    root.update(cx, |root, cx| root.record_navigation_visit(thread, cx));
    drop(root);
    cx.run_until_parked();
    assert_eq!(pending(cx), 0);
}
