use super::menu_placement_tests::*;
use super::*;
use gpui_kit::{Element, KeyDownEvent, KeyUpEvent, Modifiers, TestAppContext};
use vega_store::messages::MessageRow;

async fn refresh(fixture: &mut Fixture) {
    fixture.block.update(&mut fixture.visual.cx, |block, cx| {
        block.refresh_organization(cx);
    });
    fixture.wait_idle().await;
}

fn assert_archive_semantics(fixture: &mut Fixture, selected: bool) {
    fixture.block.update(&mut fixture.visual.cx, |block, cx| {
        let control = block.archive_control(false, cx);
        let role = control.a11y_role().expect("actual archive element role");
        assert_eq!(role, gpui_kit::Role::Button);
        let mut node = gpui_kit::accesskit::Node::new(role);
        control.write_a11y_info(&mut node);
        assert_eq!(
            node.label(),
            Some(if selected {
                "隐藏已归档"
            } else {
                "显示已归档"
            })
        );
        assert!(node.supports_action(gpui_kit::AccessibleAction::Click));
        assert!(node.supports_action(gpui_kit::AccessibleAction::Focus));
    });
    let bounds = fixture.bounds("organization-archive");
    let outside = fixture.outside();
    fixture
        .visual
        .simulate_mouse_move(outside, None, Modifiers::none());
    fixture.settle();
    fixture.visual.update(|window, cx| {
        let active = theme(cx).colors.bg_active.into();
        let selected_surface = window.painted_quads().iter().any(|quad| {
            quad.bounds == bounds.scale(window.scale_factor())
                && quad.background.as_solid() == Some(active)
        });
        assert_eq!(
            selected_surface, selected,
            "actual archive selected surface"
        );
    });
}

fn assert_archive_state(fixture: &mut Fixture, selected: bool, archived: &str) {
    fixture.visual.cx.read(|cx| {
        assert_eq!(
            fixture
                .block
                .read(cx)
                .organization
                .as_ref()
                .unwrap()
                .archive,
            selected
        );
        assert!(fixture.block.read(cx).actions_open.is_none());
    });
    assert!(!fixture.has_menu());
    assert_eq!(
        fixture
            .visual
            .debug_bounds(selector(format!("project-thread-row-{archived}")))
            .is_some(),
        selected,
        "archive visibility follows the real control"
    );
}

#[gpui_kit::test]
async fn organization_menu_placement_direct_archive_tab_from_neighbor_does_not_activate(
    cx: &mut TestAppContext,
) {
    let mut fixture = Fixture::new(cx, FixtureConfig::default()).await;
    let archive = fixture.capture_focus("organization-archive");
    let neighbor = fixture.focus("organization-new-session");
    assert_ne!(archive, neighbor);
    let before = UnchangedState::read(&fixture);
    let pointer = fixture.outside();
    fixture
        .visual
        .simulate_mouse_move(pointer, None, Modifiers::none());
    fixture.key("shift-tab");
    fixture.assert_focused(&archive);
    fixture.key("tab");
    fixture.assert_focused(&neighbor);
    fixture.key("shift-tab");
    fixture.assert_focused(&archive);
    before.assert_unchanged(&fixture);
    assert!(!fixture.has_menu());
    fixture.visual.cx.read(|cx| {
        assert!(
            !fixture
                .block
                .read(cx)
                .organization
                .as_ref()
                .unwrap()
                .archive
        );
    });
}

#[gpui_kit::test]
async fn organization_menu_placement_direct_archive_real_button_click_and_key_release_toggle_once(
    cx: &mut TestAppContext,
) {
    let mut fixture = Fixture::new(
        cx,
        FixtureConfig {
            tasks_per_project: 1,
            ..Default::default()
        },
    )
    .await;
    let archived = fixture.tasks[1][0].clone();
    fixture.visual.cx.read(|cx| {
        let store = cx.global::<VegaStore>().0.as_ref().unwrap();
        conversation::set_thread_status(store, &archived, ThreadStatus::Archived)
            .expect("owned archived task");
    });
    refresh(&mut fixture).await;
    let before = UnchangedState::read(&fixture);
    let mut selected = false;
    assert_archive_semantics(&mut fixture, selected);
    for activation in ["click", "enter", "space", "click", "enter", "space"] {
        assert_archive_state(&mut fixture, selected, &archived);
        let focus = fixture.focus("organization-archive");
        if activation == "click" {
            let button = fixture.bounds("organization-archive");
            fixture.visual.simulate_mouse_down(
                button.center(),
                MouseButton::Left,
                Modifiers::none(),
            );
            fixture.settle();
            assert_archive_state(&mut fixture, selected, &archived);
            fixture
                .visual
                .simulate_mouse_up(button.center(), MouseButton::Left, Modifiers::none());
        } else {
            let keystroke = gpui_kit::Keystroke::parse(activation).unwrap();
            fixture.visual.simulate_event(KeyDownEvent {
                keystroke: keystroke.clone(),
                is_held: false,
                prefer_character_input: false,
            });
            fixture.settle();
            fixture.assert_focused(&focus);
            assert_archive_state(&mut fixture, selected, &archived);
            fixture.visual.simulate_event(KeyUpEvent { keystroke });
        }
        fixture.settle();
        fixture.wait_idle().await;
        selected = !selected;
        fixture.assert_focused(&focus);
        assert_archive_state(&mut fixture, selected, &archived);
        before.assert_unchanged(&fixture);
        let outside = fixture.focus("fixture-outside");
        fixture.assert_focused(&outside);
        assert_archive_semantics(&mut fixture, selected);
    }
    assert!(!selected);
    assert_eq!(fixture.outer_closes.get(), 0);
}

fn assert_order(fixture: &Fixture, expected: &[String]) {
    let snapshot = fixture.snapshot();
    assert_eq!(snapshot.preferences.sort, SidebarTaskSort::Manual);
    assert_eq!(snapshot.preferences.manual_thread_order, expected);
}

fn assert_task_rows(fixture: &mut Fixture, ids: &[String]) {
    let rows: Vec<_> = ids
        .iter()
        .map(|id| fixture.bounds(format!("project-thread-row-{id}")))
        .collect();
    for pair in rows.windows(2) {
        assert!(
            pair[0].bottom() <= pair[1].top() + px(1.0),
            "real rows preserve declared order: {rows:?}"
        );
    }
}

fn message(id: &str, task: &str, status: &str, body: &str) -> MessageRow {
    MessageRow {
        id: id.into(),
        thread_id: task.into(),
        seq: 1,
        role: "assistant".into(),
        kind: "text".into(),
        content: body.into(),
        status: status.into(),
        created_at: 10,
        plan_status: None,
        plan_review_note: None,
        plan_reviewed_at: None,
    }
}

#[gpui_kit::test]
async fn organization_menu_placement_committed_messages_manual_order_and_ui_preserve_compatibility(
    cx: &mut TestAppContext,
) {
    let mut fixture = Fixture::new(
        cx,
        FixtureConfig {
            tasks_per_project: 3,
            ..Default::default()
        },
    )
    .await;
    let [a, b, c]: [String; 3] = fixture.tasks[0]
        .clone()
        .try_into()
        .expect("three same-region tasks");
    let [d, e, f]: [String; 3] = fixture.tasks[1]
        .clone()
        .try_into()
        .expect("three other-region tasks");
    let standalone = fixture.visual.cx.read(|cx| {
        let store = cx.global::<VegaStore>().0.as_ref().unwrap();
        let standalone =
            conversation::create_standalone_thread(store, "fixture", "readonly").unwrap();
        conversation::set_thread_status(store, &e, ThreadStatus::Archived).unwrap();
        for id in [&d, &e, &f, &standalone.id, &a, &b, &c] {
            seed_action(
                store,
                SidebarOrganizationAction::ReorderThread {
                    thread_id: id.clone(),
                    before_id: None,
                },
            );
        }
        standalone.id
    });
    refresh(&mut fixture).await;
    let initial = vec![
        d.clone(),
        e.clone(),
        f.clone(),
        standalone.clone(),
        a.clone(),
        b.clone(),
        c.clone(),
    ];
    assert_order(&fixture, &initial);
    assert_task_rows(&mut fixture, &[a.clone(), b.clone(), c.clone()]);
    let baseline = fixture.snapshot();
    let route = UnchangedState::read(&fixture);
    let row = message(
        "owned-streaming-c",
        &c,
        "streaming",
        "Initial committed C body",
    );
    fixture.visual.cx.read(|cx| {
        let store = cx.global::<VegaStore>().0.as_ref().unwrap();
        vega_store::messages::insert(store.conn(), &row).expect("committed formal C message");
        assert_eq!(
            vega_store::messages::find(store.conn(), &row.id).unwrap(),
            Some(row.clone())
        );
    });
    let promoted = vec![
        d.clone(),
        e.clone(),
        f.clone(),
        standalone.clone(),
        c.clone(),
        a.clone(),
        b.clone(),
    ];
    assert_order(&fixture, &promoted);
    refresh(&mut fixture).await;
    assert_order(&fixture, &promoted);
    assert_task_rows(&mut fixture, &[c.clone(), a.clone(), b.clone()]);
    fixture.visual.cx.read(|cx| {
        let store = cx.global::<VegaStore>().0.as_ref().unwrap();
        seed_action(
            store,
            SidebarOrganizationAction::ReorderThread {
                thread_id: c.clone(),
                before_id: None,
            },
        );
    });
    refresh(&mut fixture).await;
    assert_order(&fixture, &initial);
    assert_task_rows(&mut fixture, &[a.clone(), b.clone(), c.clone()]);
    let stable = fixture.snapshot();
    for project in fixture.projects.clone() {
        fixture.click(format!("project-more-{project}"));
        assert!(fixture.has_menu());
        assert_order(&fixture, &initial);
        fixture.key("tab shift-tab escape");
        assert!(!fixture.has_menu());
        assert_eq!(fixture.snapshot(), stable);
    }
    let project = fixture.projects[0].clone();
    fixture.focus(format!("project-header-{project}"));
    fixture.key("tab tab shift-tab");
    assert!(!fixture.has_menu());
    assert_eq!(fixture.snapshot(), stable);
    fixture.focus("organization-archive");
    fixture.key("tab shift-tab");
    assert!(!fixture.has_menu());
    assert_eq!(fixture.snapshot(), stable);
    let archive = fixture.focus("organization-archive");
    fixture.key("enter");
    fixture.wait_idle().await;
    assert_archive_state(&mut fixture, true, &e);
    assert_eq!(fixture.snapshot(), stable);
    fixture.assert_focused(&archive);
    fixture.key("space");
    fixture.wait_idle().await;
    assert_archive_state(&mut fixture, false, &e);
    assert_eq!(fixture.snapshot(), stable);
    fixture.visual.cx.read(|cx| {
        let store = cx.global::<VegaStore>().0.as_ref().unwrap();
        assert_eq!(conversation::open_thread(store, &a).unwrap().id, a);
        assert_eq!(
            service::snapshot(store)
                .unwrap()
                .preferences
                .manual_thread_order,
            initial
        );
        assert_eq!(conversation::visit_thread(store, &b).unwrap().id, b);
        assert_eq!(
            service::snapshot(store)
                .unwrap()
                .preferences
                .manual_thread_order,
            initial
        );
        let renamed =
            conversation::rename_thread(store, &c, "Renamed C without promotion").unwrap();
        assert_eq!(renamed.title, "Renamed C without promotion");
        assert_eq!(
            service::snapshot(store)
                .unwrap()
                .preferences
                .manual_thread_order,
            initial
        );
        assert_eq!(
            vega_store::messages::update_streaming_content(
                store.conn(),
                &row.id,
                "Committed updated C body"
            )
            .unwrap(),
            1
        );
        let updated = vega_store::messages::find(store.conn(), &row.id)
            .unwrap()
            .unwrap();
        assert_eq!(updated.content, "Committed updated C body");
        assert_eq!(updated.status, "streaming");
        let reopened =
            Store::open(store.database_path().unwrap()).expect("independent owned-store reader");
        let committed = vega_store::messages::find(reopened.conn(), &row.id)
            .unwrap()
            .unwrap();
        assert_eq!(committed, updated);
        assert_eq!(
            service::snapshot(&reopened)
                .unwrap()
                .preferences
                .manual_thread_order,
            initial
        );
    });
    assert_order(&fixture, &initial);
    refresh(&mut fixture).await;
    assert_order(&fixture, &initial);
    assert_task_rows(&mut fixture, &[a.clone(), b.clone(), c.clone()]);
    let next = message("owned-new-b", &b, "done", "A distinct committed B message");
    fixture.visual.cx.read(|cx| {
        let store = cx.global::<VegaStore>().0.as_ref().unwrap();
        vega_store::messages::insert(store.conn(), &next).expect("committed new B message");
        assert_eq!(
            vega_store::messages::find(store.conn(), &next.id).unwrap(),
            Some(next.clone())
        );
        let updated = vega_store::messages::find(store.conn(), &row.id)
            .unwrap()
            .unwrap();
        assert_eq!(updated.content, "Committed updated C body");
    });
    let final_order = vec![
        d.clone(),
        e.clone(),
        f.clone(),
        standalone,
        b.clone(),
        a.clone(),
        c.clone(),
    ];
    assert_order(&fixture, &final_order);
    refresh(&mut fixture).await;
    assert_order(&fixture, &final_order);
    assert_task_rows(&mut fixture, &[b, a, c]);
    let after = fixture.snapshot();
    assert_eq!(after.groups, baseline.groups);
    assert_eq!(after.memberships, baseline.memberships);
    assert_eq!(after.collapsed, baseline.collapsed);
    assert_eq!(after.project_order, baseline.project_order);
    for thread in &baseline.threads {
        let saved = after
            .threads
            .iter()
            .find(|saved| saved.id == thread.id)
            .unwrap();
        assert_eq!(saved.project_id, thread.project_id);
        assert_eq!(saved.pinned, thread.pinned);
        assert_eq!(saved.status, thread.status);
        if [&d, &e, &f].contains(&&thread.id) {
            assert_eq!(saved, thread);
        }
    }
    let current = UnchangedState::read(&fixture);
    assert_eq!(current.selected_project, route.selected_project);
    assert_eq!(current.opened_thread, route.opened_thread);
    assert_eq!(current.draft, route.draft);
    assert_eq!(fixture.outer_closes.get(), 0);
}
