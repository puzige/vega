use super::*;

fn archive_toolbar_bounds(
    f: &Fixture,
    cx: &mut gpui_kit::TestAppContext,
) -> gpui_kit::Bounds<gpui_kit::Pixels> {
    let new_task = bounds(f, cx, "organization-new-session");
    gpui_kit::Bounds::new(
        point(
            new_task.left() - px(Layout::SIDEBAR_ACTION_GAP + Layout::SIDEBAR_ACTION_SIZE),
            new_task.top(),
        ),
        gpui_kit::size(
            px(Layout::SIDEBAR_ACTION_SIZE),
            px(Layout::SIDEBAR_ACTION_SIZE),
        ),
    )
}

fn hover_recents_header(f: &Fixture, cx: &mut gpui_kit::TestAppContext) {
    let header = bounds(f, cx, "organization-recents-header");
    gpui_kit::VisualTestContext::from_window(f.window.into(), cx).simulate_mouse_move(
        point(header.left() + px(2.), header.center().y),
        None,
        Default::default(),
    );
    cx.run_until_parked();
}

pub(super) fn click_archive_toolbar(f: &Fixture, cx: &mut gpui_kit::TestAppContext) {
    hover_recents_header(f, cx);
    let toolbar = archive_toolbar_bounds(f, cx);
    assert!(bounds(f, cx, "sidebar-scroll").contains(&toolbar.center()));
    gpui_kit::VisualTestContext::from_window(f.window.into(), cx)
        .simulate_click(toolbar.center(), Default::default());
    cx.run_until_parked();
}

fn archive_toolbar_has_active_background(f: &Fixture, cx: &mut gpui_kit::TestAppContext) -> bool {
    let toolbar = archive_toolbar_bounds(f, cx);
    f.window
        .update(cx, |_, window, cx| {
            let scale = window.scale_factor();
            let active: gpui_kit::Hsla = theme(cx).colors.bg_active.into();
            window.painted_quads().iter().any(|quad| {
                quad.background.as_solid() == Some(active)
                    && (quad.bounds.left().as_f32() / scale - f32::from(toolbar.left())).abs() < 0.2
                    && (quad.bounds.top().as_f32() / scale - f32::from(toolbar.top())).abs() < 0.2
                    && (quad.bounds.size.width.as_f32() / scale - f32::from(toolbar.size.width))
                        .abs()
                        < 0.2
                    && (quad.bounds.size.height.as_f32() / scale - f32::from(toolbar.size.height))
                        .abs()
                        < 0.2
            })
        })
        .unwrap()
}

struct ArchiveRows {
    archived: [String; 3],
    active: [String; 3],
}

fn archived_rows_fixture(cx: &mut gpui_kit::TestAppContext) -> (Fixture, ArchiveRows) {
    let f = fixture_with_size(cx, 1200., 1400.);
    let store = Store::open(f.dir.path().join("organization.db")).unwrap();
    let project = conversation::list_threads(&store, "p", None)
        .unwrap()
        .into_iter()
        .find(|thread| thread.id == "p-b")
        .unwrap();
    let pinned = create_reorder_threads(&store, None, 2, true);
    let recent = create_reorder_threads(&store, None, 2, false);
    for id in [&project.id, &pinned[0], &recent[0]] {
        conversation::set_thread_status(&store, id, ThreadStatus::Archived).unwrap();
    }
    sessions(&f, cx).update(cx, ThreadsBlock::refresh_organization);
    cx.run_until_parked();
    let rows = ArchiveRows {
        archived: [
            format!("project-thread-row-{}", project.id),
            format!("pinned-thread-row-{}", pinned[0]),
            format!("standalone-thread-row-{}", recent[0]),
        ],
        active: [
            format!("project-thread-row-{}", f.first.id),
            format!("pinned-thread-row-{}", pinned[1]),
            format!("standalone-thread-row-{}", recent[1]),
        ],
    };
    (f, rows)
}

fn assert_archive_rows(
    f: &Fixture,
    cx: &mut gpui_kit::TestAppContext,
    rows: &ArchiveRows,
    shown: bool,
) {
    for selector in &rows.archived {
        assert_eq!(!absent(f, cx, selector), shown, "{selector}");
    }
    for selector in &rows.active {
        assert!(!absent(f, cx, selector), "{selector}");
    }
    assert!(absent(f, cx, "organization-menu"));
    assert_reorder_navigation_unchanged(f, cx);
}

#[gpui_kit::test]
async fn sidebar_archive_toolbar_mouse_toggles_all_scopes_without_sorting_menu(
    cx: &mut gpui_kit::TestAppContext,
) {
    let (f, rows) = archived_rows_fixture(cx);
    let before = snapshot(&f);
    assert_archive_rows(&f, cx, &rows, false);
    for dark in [false, true] {
        cx.update(|cx| {
            cx.set_global(if dark {
                vega_theme::Theme::dark()
            } else {
                vega_theme::Theme::light()
            });
        });
        cx.run_until_parked();
        hover_recents_header(&f, cx);
        let header = bounds(&f, cx, "organization-recents-header");
        assert!(!archive_toolbar_has_active_background(&f, cx));
        click_archive_toolbar(&f, cx);
        assert_archive_rows(&f, cx, &rows, true);
        hover_recents_header(&f, cx);
        assert!(archive_toolbar_has_active_background(&f, cx));
        assert_eq!(
            bounds(&f, cx, "organization-recents-header").size,
            header.size
        );
        assert_eq!(snapshot(&f), before);
        sessions(&f, cx).update(cx, ThreadsBlock::refresh_organization);
        cx.run_until_parked();
        assert_archive_rows(&f, cx, &rows, true);
        click_archive_toolbar(&f, cx);
        assert_archive_rows(&f, cx, &rows, false);
        hover_recents_header(&f, cx);
        assert!(!archive_toolbar_has_active_background(&f, cx));
        assert_eq!(snapshot(&f), before);
    }
}

#[gpui_kit::test]
async fn sidebar_archive_toolbar_tab_enter_and_space_toggle_without_navigation_or_draft_loss(
    cx: &mut gpui_kit::TestAppContext,
) {
    let (f, rows) = archived_rows_fixture(cx);
    let before = snapshot(&f);
    let last_project_action = sessions(&f, cx).read_with(cx, |block, _| {
        block.thread_action_focuses[&f.other.id].clone()
    });
    f.window
        .update(cx, |_, window, cx| last_project_action.focus(window, cx))
        .unwrap();
    cx.run_until_parked();
    cx.simulate_keystrokes(f.window.into(), "tab");
    cx.run_until_parked();
    assert!(
        archive_toolbar_has_active_background(&f, cx),
        "archive toolbar must be reachable by Tab"
    );
    let focus = f
        .window
        .update(cx, |_, window, cx| window.focused(cx).unwrap())
        .unwrap();
    for (key, shown) in [("enter", true), ("space", false)] {
        cx.simulate_keystrokes(f.window.into(), key);
        f.window
            .update(cx, |_, window, cx| {
                window.dispatch_event(
                    gpui_kit::PlatformInput::KeyUp(gpui_kit::KeyUpEvent {
                        keystroke: gpui_kit::Keystroke::parse(key).unwrap(),
                    }),
                    cx,
                );
            })
            .unwrap();
        cx.run_until_parked();
        assert_archive_rows(&f, cx, &rows, shown);
        assert_eq!(snapshot(&f), before);
        f.window
            .update(cx, |_, window, cx| {
                assert!(focus.contains_focused(window, cx));
            })
            .unwrap();
    }
    click(&f, cx, format!("project-thread-row-{}", f.other.id));
    cx.update(|cx| {
        assert_eq!(
            cx.global::<OpenedThread>().0.as_ref().unwrap().id,
            f.other.id
        );
        assert_eq!(cx.global::<SelectedProject>().0.as_deref(), Some("q"));
        assert!(cx.global::<PendingDeleteConfirm>().0.is_none());
    });
    assert_eq!(
        f.draft.read_with(cx, |draft, _| draft.text().to_string()),
        "owned unsent draft"
    );
    assert!(
        !conversation::list_threads(
            &Store::open(f.dir.path().join("organization.db")).unwrap(),
            "q",
            None,
        )
        .unwrap()
        .into_iter()
        .find(|thread| thread.id == "q-a")
        .unwrap()
        .unread
    );
}
