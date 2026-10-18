use super::*;

fn neighbor_fixture(cx: &mut gpui_kit::TestAppContext) -> Fixture {
    let f = fixture_with_size(cx, 1200., 760.);
    let store = Store::open(f.dir.path().join("organization.db")).unwrap();
    for (id, at) in [("p-f", 8995), ("p-g", 8994), ("p-h", 8993)] {
        create_fixture_thread(&store, id, Some("p"), false, false, at);
    }
    for id in ["p-b", "p-f"] {
        conversation::set_thread_status(&store, id, ThreadStatus::Archived).unwrap();
    }
    create_reorder_threads(&store, None, 10, false);
    sessions(&f, cx).update(cx, ThreadsBlock::refresh_organization);
    cx.run_until_parked();
    f
}

fn drag_to(
    f: &Fixture,
    cx: &mut gpui_kit::TestAppContext,
    source: &str,
    target: &str,
    after: bool,
) {
    let source = bounds(f, cx, source);
    {
        let mut visual = gpui_kit::VisualTestContext::from_window(f.window.into(), cx);
        let position = point(source.left() + px(48.), source.center().y);
        visual.simulate_mouse_move(position, None, Default::default());
        visual.simulate_mouse_down(position, MouseButton::Left, Default::default());
        visual.simulate_mouse_move(
            position + point(px(12.), px(0.)),
            Some(MouseButton::Left),
            Default::default(),
        );
        visual.run_until_parked();
    }
    assert!(cx.update(|cx| cx.has_active_drag()));
    let target = bounds(f, cx, target);
    let position = point(
        target.left() + px(48.),
        target.top() + target.size.height * if after { 0.75 } else { 0.25 },
    );
    assert!(bounds(f, cx, "sidebar-scroll").contains(&position));
    {
        let mut visual = gpui_kit::VisualTestContext::from_window(f.window.into(), cx);
        visual.simulate_mouse_move(position, Some(MouseButton::Left), Default::default());
        visual.run_until_parked();
        visual.simulate_mouse_up(position, MouseButton::Left, Default::default());
    }
    cx.run_until_parked();
    assert!(!cx.update(|cx| cx.has_active_drag()));
}

#[gpui_kit::test]
async fn dragging_preserves_archived_and_unrendered_neighbors_across_refresh_and_reopen(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = neighbor_fixture(cx);
    let before = snapshot(&f);
    assert_saved_order(
        &f,
        Some("p"),
        false,
        &["p-a", "p-b", "p-c", "p-d", "p-e", "p-f", "p-g", "p-h"],
    );
    assert_rendered_order(&f, cx, "project-", &["p-a", "p-c", "p-d", "p-e", "p-g"]);
    for id in ["p-b", "p-f", "p-h"] {
        assert!(absent(&f, cx, format!("project-thread-row-{id}")));
    }
    assert!(!absent(&f, cx, "project-p-show-more"));

    drag_to(
        &f,
        cx,
        "project-thread-row-p-a",
        "project-thread-row-p-e",
        true,
    );
    assert_rendered_order(&f, cx, "project-", &["p-c", "p-d", "p-e", "p-a", "p-g"]);
    assert_saved_order(
        &f,
        Some("p"),
        false,
        &["p-b", "p-c", "p-d", "p-e", "p-a", "p-f", "p-g", "p-h"],
    );
    assert_eq!(snapshot(&f).revision, before.revision + 1);
    assert_reorder_navigation_unchanged(&f, cx);
    sessions(&f, cx).update(cx, ThreadsBlock::refresh_organization);
    cx.run_until_parked();
    assert_rendered_order(&f, cx, "project-", &["p-c", "p-d", "p-e", "p-a", "p-g"]);
    assert!(absent(&f, cx, "project-thread-row-p-h"));

    archive_controls::click_archive_toolbar(&f, cx);
    assert!(!absent(&f, cx, "project-thread-row-p-b"));
    assert!(absent(&f, cx, "project-thread-row-p-f"));
    assert!(absent(&f, cx, "project-thread-row-p-g"));
    assert_rendered_order(&f, cx, "project-", &["p-b", "p-c", "p-d", "p-e", "p-a"]);
    click(&f, cx, "project-p-show-more");
    assert!(absent(&f, cx, "project-p-show-more"));
    assert_rendered_order(
        &f,
        cx,
        "project-",
        &["p-b", "p-c", "p-d", "p-e", "p-a", "p-f", "p-g", "p-h"],
    );

    drag_to(
        &f,
        cx,
        "project-thread-row-p-h",
        "project-thread-row-p-b",
        false,
    );
    assert_rendered_order(
        &f,
        cx,
        "project-",
        &["p-h", "p-b", "p-c", "p-d", "p-e", "p-a", "p-f", "p-g"],
    );
    assert_saved_order(
        &f,
        Some("p"),
        false,
        &["p-h", "p-b", "p-c", "p-d", "p-e", "p-a", "p-f", "p-g"],
    );
    sessions(&f, cx).update(cx, ThreadsBlock::refresh_organization);
    cx.run_until_parked();
    assert_rendered_order(
        &f,
        cx,
        "project-",
        &["p-h", "p-b", "p-c", "p-d", "p-e", "p-a", "p-f", "p-g"],
    );
    let after = snapshot(&f);
    assert_eq!(after.revision, before.revision + 2);
    assert_eq!(after.threads, before.threads);
    assert_eq!(after.project_order, before.project_order);
    assert_saved_order(&f, Some("q"), false, &["q-a"]);
    assert_saved_order(
        &f,
        None,
        false,
        &[
            "recent-0", "recent-1", "recent-2", "recent-3", "recent-4", "recent-5", "recent-6",
            "recent-7", "recent-8", "recent-9",
        ],
    );
    assert_reorder_navigation_unchanged(&f, cx);
}

#[gpui_kit::test]
async fn show_more_rejects_drops_and_pages_archived_rows_without_changing_persisted_order(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = neighbor_fixture(cx);
    let before = snapshot(&f);
    drag_to(
        &f,
        cx,
        "project-thread-row-p-a",
        "project-p-show-more",
        true,
    );
    assert_eq!(snapshot(&f), before);
    assert!(absent(&f, cx, "project-thread-row-p-h"));
    assert!(!absent(&f, cx, "project-p-show-more"));
    assert_rendered_order(&f, cx, "project-", &["p-a", "p-c", "p-d", "p-e", "p-g"]);
    assert_reorder_navigation_unchanged(&f, cx);

    click(&f, cx, "project-p-show-more");
    assert!(absent(&f, cx, "project-p-show-more"));
    assert_rendered_order(
        &f,
        cx,
        "project-",
        &["p-a", "p-c", "p-d", "p-e", "p-g", "p-h"],
    );
    assert_eq!(snapshot(&f), before);

    archive_controls::click_archive_toolbar(&f, cx);
    assert_rendered_order(
        &f,
        cx,
        "project-",
        &["p-a", "p-b", "p-c", "p-d", "p-e", "p-f"],
    );
    assert!(absent(&f, cx, "project-thread-row-p-g"));
    assert!(absent(&f, cx, "project-thread-row-p-h"));
    assert!(!absent(&f, cx, "project-p-show-more"));
    click(&f, cx, "project-p-show-more");
    assert!(absent(&f, cx, "project-p-show-more"));
    assert_rendered_order(
        &f,
        cx,
        "project-",
        &["p-a", "p-b", "p-c", "p-d", "p-e", "p-f", "p-g", "p-h"],
    );
    archive_controls::click_archive_toolbar(&f, cx);
    assert!(absent(&f, cx, "project-thread-row-p-b"));
    assert!(absent(&f, cx, "project-thread-row-p-f"));
    sessions(&f, cx).update(cx, ThreadsBlock::refresh_organization);
    cx.run_until_parked();
    assert_rendered_order(
        &f,
        cx,
        "project-",
        &["p-a", "p-c", "p-d", "p-e", "p-g", "p-h"],
    );
    assert_eq!(snapshot(&f), before);
    assert_reorder_navigation_unchanged(&f, cx);
}
