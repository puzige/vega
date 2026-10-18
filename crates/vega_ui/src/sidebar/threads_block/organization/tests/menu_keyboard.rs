use super::*;

fn open_thread_menu(f: &Fixture, cx: &mut gpui_kit::TestAppContext, id: &str) {
    let focus = sessions(f, cx).read_with(cx, |block, _| block.thread_action_focuses[id].clone());
    f.window
        .update(cx, |_, window, cx| focus.focus(window, cx))
        .unwrap();
    cx.run_until_parked();
    keystroke(f, cx, "enter");
    assert!(!absent(f, cx, format!("project-thread-actions-menu-{id}")));
}

fn assert_project_order(f: &Fixture, cx: &mut gpui_kit::TestAppContext, expected: &[&str]) {
    assert_rendered_order(f, cx, "project-", expected);
    assert_saved_order(f, Some("p"), false, expected);
    assert_reorder_navigation_unchanged(f, cx);
}

#[gpui_kit::test]
async fn thread_menu_endpoint_moves_and_keyboard_round_trips_preserve_rows_and_draft(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture_with_size(cx, 1200., 1000.);
    let before = snapshot(&f);
    assert_project_order(&f, cx, &["p-a", "p-b", "p-c", "p-d", "p-e"]);

    open_thread_menu(&f, cx, "p-a");
    assert!(!absent(&f, cx, "project-thread-action-p-a-9"));
    assert!(absent(&f, cx, "project-thread-action-p-a-10"));
    keystroke(&f, cx, "up");
    click(&f, cx, "project-thread-action-p-a-9");
    assert_project_order(&f, cx, &["p-b", "p-a", "p-c", "p-d", "p-e"]);
    assert_eq!(snapshot(&f).revision, before.revision + 1);

    open_thread_menu(&f, cx, "p-a");
    assert!(!absent(&f, cx, "project-thread-action-p-a-9"));
    assert!(!absent(&f, cx, "project-thread-action-p-a-10"));
    keystroke(&f, cx, "up");
    keystroke(&f, cx, "up");
    keystroke(&f, cx, "space");
    assert_project_order(&f, cx, &["p-a", "p-b", "p-c", "p-d", "p-e"]);

    open_thread_menu(&f, cx, "p-e");
    assert!(!absent(&f, cx, "project-thread-action-p-e-9"));
    assert!(absent(&f, cx, "project-thread-action-p-e-10"));
    keystroke(&f, cx, "up");
    keystroke(&f, cx, "enter");
    assert_project_order(&f, cx, &["p-a", "p-b", "p-c", "p-e", "p-d"]);

    open_thread_menu(&f, cx, "p-e");
    assert!(!absent(&f, cx, "project-thread-action-p-e-10"));
    keystroke(&f, cx, "up");
    keystroke(&f, cx, "up");
    keystroke(&f, cx, "down");
    keystroke(&f, cx, "enter");
    assert_project_order(&f, cx, &["p-a", "p-b", "p-c", "p-d", "p-e"]);
    let after = snapshot(&f);
    assert_eq!(after.revision, before.revision + 4);
    assert_eq!(after.threads, before.threads);
    assert_eq!(after.project_order, before.project_order);
    cx.update(|cx| assert!(!cx.global::<SettingsOpen>().0));
}

#[gpui_kit::test]
async fn thread_and_project_menu_escape_is_consumed_before_parent_settings_and_navigation(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture_with_size(cx, 1200., 1000.);
    let before = snapshot(&f);
    cx.update(|cx| cx.set_global(SettingsOpen(true)));

    open_thread_menu(&f, cx, "p-c");
    keystroke(&f, cx, "down");
    keystroke(&f, cx, "escape");
    assert!(absent(&f, cx, "project-thread-actions-menu-p-c"));
    assert_reorder_navigation_unchanged(&f, cx);
    cx.update(|cx| assert!(cx.global::<SettingsOpen>().0));
    assert_eq!(snapshot(&f), before);

    click(&f, cx, "project-more-p");
    assert!(!absent(&f, cx, "organization-menu"));
    keystroke(&f, cx, "down");
    keystroke(&f, cx, "escape");
    assert!(absent(&f, cx, "organization-menu"));
    assert_reorder_navigation_unchanged(&f, cx);
    cx.update(|cx| assert!(cx.global::<SettingsOpen>().0));
    assert_eq!(snapshot(&f), before);

    let focus = f.draft.read_with(cx, |input, cx| input.focus_handle(cx));
    f.window
        .update(cx, |_, window, cx| focus.focus(window, cx))
        .unwrap();
    keystroke(&f, cx, "escape");
    cx.update(|cx| {
        assert!(!cx.global::<SettingsOpen>().0);
        assert!(cx.global::<SelectedProject>().0.is_none());
        assert!(cx.global::<OpenedThread>().0.is_none());
    });
    assert_eq!(snapshot(&f), before);
    assert_eq!(
        f.draft.read_with(cx, |draft, _| draft.text().to_string()),
        "owned unsent draft"
    );
}

#[gpui_kit::test]
async fn ordinary_thread_click_clears_unread_without_reordering_or_discarding_draft(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture_with_size(cx, 1200., 1000.);
    let before = snapshot(&f);
    let clicked_before = before
        .threads
        .iter()
        .find(|thread| thread.id == "p-c")
        .unwrap();
    assert!(clicked_before.unread);
    assert_eq!(clicked_before.updated_at, 8998);
    assert_project_order(&f, cx, &["p-a", "p-b", "p-c", "p-d", "p-e"]);

    click(&f, cx, "project-thread-row-p-c");
    cx.update(|cx| {
        assert_eq!(cx.global::<OpenedThread>().0.as_ref().unwrap().id, "p-c");
        assert_eq!(cx.global::<SelectedProject>().0.as_deref(), Some("p"));
        assert!(!cx.global::<SettingsOpen>().0);
        assert!(cx.global::<PendingDeleteConfirm>().0.is_none());
    });
    let after = snapshot(&f);
    let clicked_after = after
        .threads
        .iter()
        .find(|thread| thread.id == "p-c")
        .unwrap();
    assert!(!clicked_after.unread);
    assert!(clicked_after.updated_at > clicked_before.updated_at);
    assert_eq!(clicked_after.created_at, clicked_before.created_at);
    assert_eq!(
        f.draft.read_with(cx, |draft, _| draft.text().to_string()),
        "owned unsent draft"
    );
    assert!(absent(&f, cx, "project-thread-actions-menu-p-c"));
    assert!(sessions(&f, cx).read_with(cx, |block, _| block.editing.is_none()));
    assert_eq!(after.revision, before.revision);
    assert_rendered_order(&f, cx, "project-", &["p-a", "p-b", "p-c", "p-d", "p-e"]);
    assert_saved_order(&f, Some("p"), false, &["p-a", "p-b", "p-c", "p-d", "p-e"]);
}
