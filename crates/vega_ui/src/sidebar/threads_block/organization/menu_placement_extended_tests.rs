use super::menu_placement_tests::*;
use super::*;
use crate::sidebar::SidebarWidth;
use gpui_kit::{
    Bounds, Modifiers, Pixels, Point, ScrollDelta, ScrollWheelEvent, TestAppContext, TouchPhase,
    size,
};
use tempfile::TempDir;

fn persisted(fixture: &Fixture) -> SidebarOrganizationSnapshot {
    fixture.visual.cx.read(|cx| {
        let store = cx
            .global::<VegaStore>()
            .0
            .as_ref()
            .expect("owned file store");
        assert!(store.database_path().is_some());
        service::snapshot(store).expect("persisted organization")
    })
}

fn route_state(fixture: &Fixture) -> (Option<String>, Option<Thread>, String) {
    fixture.visual.cx.read(|cx| {
        (
            cx.global::<SelectedProject>().0.clone(),
            cx.global::<OpenedThread>().0.clone(),
            fixture.draft.read(cx).text().to_owned(),
        )
    })
}

fn assert_near(actual: Pixels, expected: Pixels) {
    assert!(
        (actual - expected).abs() <= px(1.0),
        "actual {actual:?}, expected {expected:?} within one logical pixel"
    );
}

fn assert_same_bounds(actual: Bounds<Pixels>, expected: Bounds<Pixels>) {
    assert_near(actual.left(), expected.left());
    assert_near(actual.top(), expected.top());
    assert_near(actual.right(), expected.right());
    assert_near(actual.bottom(), expected.bottom());
}

fn assert_inside(inner: Bounds<Pixels>, outer: Bounds<Pixels>) {
    assert!(inner.size.width > px(0.0) && inner.size.height > px(0.0));
    assert!(
        inner.left() >= outer.left() - px(1.0),
        "{inner:?} outside {outer:?}"
    );
    assert!(
        inner.top() >= outer.top() - px(1.0),
        "{inner:?} outside {outer:?}"
    );
    assert!(
        inner.right() <= outer.right() + px(1.0),
        "{inner:?} outside {outer:?}"
    );
    assert!(
        inner.bottom() <= outer.bottom() + px(1.0),
        "{inner:?} outside {outer:?}"
    );
}

fn viewport(fixture: &mut Fixture) -> Bounds<Pixels> {
    fixture
        .visual
        .update(|window, _| Bounds::new(point(px(0.0), px(0.0)), window.viewport_size()))
}

fn painted_backgrounds(
    fixture: &mut Fixture,
    color: gpui_kit::Rgba,
) -> Vec<(Bounds<Pixels>, Bounds<Pixels>)> {
    fixture.visual.update(|window, _| {
        let scale = window.scale_factor();
        let logical = |bounds: Bounds<gpui_kit::ScaledPixels>| {
            Bounds::new(
                point(px(bounds.left().0 / scale), px(bounds.top().0 / scale)),
                size(
                    px(bounds.size.width.0 / scale),
                    px(bounds.size.height.0 / scale),
                ),
            )
        };
        let color = gpui_kit::Hsla::from(color);
        window
            .painted_quads()
            .into_iter()
            .filter(|quad| quad.background.as_solid() == Some(color))
            .map(|quad| (logical(quad.bounds), logical(quad.content_mask.bounds)))
            .collect()
    })
}

fn menu_surfaces(fixture: &mut Fixture) -> Vec<(Bounds<Pixels>, Bounds<Pixels>)> {
    let color = fixture.visual.cx.read(|cx| theme(cx).colors.bg_elevated);
    painted_backgrounds(fixture, color)
        .into_iter()
        .filter(|(bounds, _)| (bounds.size.width - px(Layout::TASK_MENU_WIDTH)).abs() <= px(1.0))
        .collect()
}

fn assert_single_menu(fixture: &mut Fixture) -> Bounds<Pixels> {
    let actual = fixture.menu_bounds();
    let surfaces = menu_surfaces(fixture);
    assert_eq!(surfaces.len(), 1, "one painted menu surface: {surfaces:?}");
    assert_same_bounds(surfaces[0].0, actual);
    assert_inside(actual, surfaces[0].1);
    assert_inside(actual, viewport(fixture));
    assert_near(actual.size.width, px(Layout::TASK_MENU_WIDTH));
    actual
}

fn assert_no_menu(fixture: &mut Fixture) {
    assert!(fixture.visual.debug_bounds("organization-menu").is_none());
    assert!(menu_surfaces(fixture).is_empty());
}

fn assert_pointer_anchor(fixture: &mut Fixture, position: Point<Pixels>) {
    let menu = assert_single_menu(fixture);
    assert_near(menu.left(), position.x);
    assert_near(menu.top(), position.y);
}

fn commands(fixture: &Fixture) -> Vec<(String, MenuCommand)> {
    fixture
        .visual
        .cx
        .read(|cx| fixture.block.read(cx).organization_menu_items())
}

fn command_index(fixture: &Fixture, predicate: impl Fn(&MenuCommand) -> bool) -> usize {
    commands(fixture)
        .iter()
        .position(|(_, command)| predicate(command))
        .expect("existing production menu command")
}

fn assert_project_target(fixture: &mut Fixture, project: &str) {
    let items = commands(fixture);
    assert!(matches!(
        items.first(),
        Some((label, MenuCommand::RemoveProject(id))) if label == "移除项目" && id == project
    ));
    let first = fixture.bounds("organization-menu-0");
    let menu = fixture.menu_bounds();
    assert_inside(first, menu);
}

fn assert_highlight(fixture: &mut Fixture, index: usize) {
    let item = fixture.bounds(format!("organization-menu-{index}"));
    let menu = fixture.menu_bounds();
    assert_inside(item, menu);
    let color = fixture.visual.cx.read(|cx| theme(cx).colors.bg_active);
    let highlights: Vec<_> = painted_backgrounds(fixture, color)
        .into_iter()
        .filter(|(bounds, mask)| {
            menu.contains(&bounds.center())
                && mask.contains(&bounds.center())
                && (bounds.size.width - item.size.width).abs() <= px(1.0)
                && (bounds.size.height - item.size.height).abs() <= px(1.0)
        })
        .collect();
    assert_eq!(
        highlights.len(),
        1,
        "one visible menu highlight: {highlights:?}"
    );
    assert_same_bounds(highlights[0].0, item);
}

async fn settle_commands(fixture: &mut Fixture) {
    let block = fixture.block.clone();
    fixture
        .visual
        .cx
        .condition(&block, |block, cx| {
            assert!(block.error.is_none(), "command error: {:?}", block.error);
            !block.action_pending
                && block.organization.as_ref().is_some_and(|org| {
                    assert!(org.projects.read(cx).error.is_none());
                    !org.pending && !org.refresh_queued
                })
        })
        .await;
    fixture.settle();
}

async fn insert_spacer_projects(fixture: &mut Fixture, before: &str, count: usize) -> TempDir {
    insert_spacer_projects_at(fixture, Some(before), count).await
}

async fn insert_spacer_projects_at(
    fixture: &mut Fixture,
    before_id: Option<&str>,
    count: usize,
) -> TempDir {
    let directory = tempfile::tempdir().expect("owned spacer directories");
    let root = directory
        .path()
        .canonicalize()
        .expect("absolute spacer root");
    let mut paths = Vec::new();
    for index in 0..count {
        let path = root.join(format!("spacer-{index}"));
        std::fs::create_dir(&path).expect("owned non-git spacer directory");
        assert!(!path.join(".git").exists());
        paths.push(path);
    }
    let block = fixture.block.clone();
    let revision = fixture.visual.update(|_, cx| {
        let store = cx
            .global::<VegaStore>()
            .0
            .as_ref()
            .expect("owned file store");
        let mut snapshot = service::snapshot(store).expect("spacer baseline");
        for (index, path) in paths.iter().enumerate() {
            let project = vega_store::projects::create(
                store.conn(),
                path.to_str().expect("owned spacer path"),
                &format!("Spacer {index}"),
                None,
            )
            .expect("owned spacer project");
            for _ in 0..4 {
                conversation::create_thread(store, &project.id, "fixture", "readonly")
                    .expect("owned spacer task");
            }
            snapshot = service::apply(
                store,
                snapshot.revision,
                SidebarOrganizationAction::MoveProject {
                    project_id: project.id,
                    before_id: before_id.map(str::to_owned),
                },
            )
            .expect("spacer order")
            .snapshot;
        }
        block.update(cx, |block, cx| block.refresh_organization(cx));
        snapshot.revision
    });
    let block = fixture.block.clone();
    fixture
        .visual
        .cx
        .condition(&block, |block, _| {
            assert!(block.error.is_none(), "spacer refresh: {:?}", block.error);
            block.organization.as_ref().is_some_and(|org| {
                !org.pending
                    && org
                        .snapshot
                        .as_ref()
                        .is_some_and(|s| s.revision == revision)
            })
        })
        .await;
    fixture.settle();
    directory
}

fn wheel(fixture: &mut Fixture, position: Point<Pixels>, delta: Pixels) {
    fixture
        .visual
        .simulate_mouse_move(position, None, Modifiers::none());
    fixture.visual.simulate_event(ScrollWheelEvent {
        position,
        delta: ScrollDelta::Pixels(point(px(0.0), delta)),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
    });
    fixture.settle();
}

fn place_control_in_scroll(fixture: &mut Fixture, id: &str, desired_y: Pixels) -> Bounds<Pixels> {
    let scroll = fixture.bounds("sidebar-scroll");
    let control = fixture.bounds(id);
    wheel(fixture, scroll.center(), desired_y - control.center().y);
    let current = fixture.bounds(id);
    assert_inside(current, scroll);
    assert_near(current.center().y, desired_y);
    current
}

#[gpui_kit::test]
async fn organization_menu_placement_sort_uses_current_button_and_applies_sort(
    cx: &mut TestAppContext,
) {
    let mut fixture = Fixture::new(
        cx,
        FixtureConfig {
            height: 1000.0,
            tasks_per_project: 3,
            ..Default::default()
        },
    )
    .await;
    let baseline = persisted(&fixture);
    let route = route_state(&fixture);
    let project = fixture.projects[0].clone();
    let project_button = fixture.bounds(format!("project-more-{project}"));
    fixture.click(format!("project-more-{project}"));
    fixture.assert_button_anchor(project_button);
    assert_project_target(&mut fixture, &project);
    let project_menu = assert_single_menu(&mut fixture);
    fixture.key("escape");
    assert_no_menu(&mut fixture);

    let sort_button = fixture.bounds("organization-session-sort");
    fixture.click("organization-session-sort");
    fixture.assert_button_anchor(sort_button);
    let sort_menu = assert_single_menu(&mut fixture);
    assert!((sort_menu.top() - project_menu.top()).abs() > px(1.0));
    let items = commands(&fixture);
    assert_eq!(items.len(), 4);
    for (index, expected) in [
        SidebarTaskSort::Updated,
        SidebarTaskSort::Created,
        SidebarTaskSort::Manual,
    ]
    .into_iter()
    .enumerate()
    {
        assert!(matches!(
            &items[index].1,
            MenuCommand::Apply(SidebarOrganizationAction::SetPreferences(preferences))
                if preferences.sort == expected
        ));
    }
    assert!(matches!(&items[3].1, MenuCommand::ToggleArchive));
    assert_highlight(&mut fixture, 0);
    fixture.click("organization-menu-1");
    settle_commands(&mut fixture).await;
    assert_no_menu(&mut fixture);
    let sorted = persisted(&fixture);
    assert_eq!(sorted.preferences.sort, SidebarTaskSort::Created);
    assert_eq!(sorted.project_order, baseline.project_order);
    assert_eq!(sorted.collapsed, baseline.collapsed);
    assert_eq!(sorted.threads, baseline.threads);
    assert_eq!(route_state(&fixture), route);

    let other = fixture.projects[1].clone();
    let other_button = fixture.bounds(format!("project-more-{other}"));
    fixture.click(format!("project-more-{other}"));
    fixture.assert_button_anchor(other_button);
    assert_single_menu(&mut fixture);
    assert_project_target(&mut fixture, &other);
    fixture.key("escape");
    fixture.click("organization-session-sort");
    let current_sort_button = fixture.bounds("organization-session-sort");
    fixture.assert_button_anchor(current_sort_button);
    assert!(commands(&fixture)[1].0.starts_with('✓'));
    fixture.key("escape");
    assert_no_menu(&mut fixture);
    assert_eq!(route_state(&fixture), route);
}

#[gpui_kit::test]
async fn organization_menu_placement_project_projection_uses_current_button_and_pointer(
    cx: &mut TestAppContext,
) {
    let mut fixture = Fixture::new(
        cx,
        FixtureConfig {
            projection: Projection::Projects,
            tasks_per_project: 3,
            ..Default::default()
        },
    )
    .await;
    let baseline = persisted(&fixture);
    let route = route_state(&fixture);
    let a = fixture.projects[0].clone();
    let b = fixture.projects[1].clone();
    let a_button = fixture.bounds(format!("project-menu-{a}"));
    fixture.click(format!("project-menu-{a}"));
    fixture.assert_button_anchor(a_button);
    assert_single_menu(&mut fixture);
    assert_project_target(&mut fixture, &a);
    fixture.key("escape");
    assert_no_menu(&mut fixture);

    let b_header = fixture.bounds(format!("project-header-{b}"));
    let b_pointer = point(b_header.left() + px(48.0), b_header.center().y);
    fixture.right_click(b_pointer);
    assert_pointer_anchor(&mut fixture, b_pointer);
    assert_project_target(&mut fixture, &b);
    fixture.key("escape");
    let b_button = fixture.bounds(format!("project-menu-{b}"));
    fixture.click(format!("project-menu-{b}"));
    fixture.assert_button_anchor(b_button);
    assert_single_menu(&mut fixture);
    assert_project_target(&mut fixture, &b);
    fixture.key("escape");

    let a_header = fixture.bounds(format!("project-header-{a}"));
    let a_pointer = point(a_header.left() + px(72.0), a_header.center().y);
    fixture.right_click(a_pointer);
    assert_pointer_anchor(&mut fixture, a_pointer);
    assert_project_target(&mut fixture, &a);
    assert!((a_pointer.y - b_pointer.y).abs() > px(1.0));
    let move_a = command_index(
        &fixture,
        |command| matches!(command, MenuCommand::Apply(SidebarOrganizationAction::MoveProject { project_id, .. }) if project_id == &a),
    );
    fixture.click(format!("organization-menu-{move_a}"));
    settle_commands(&mut fixture).await;
    let after = persisted(&fixture);
    assert_eq!(after.project_order, vec![b, a]);
    assert_eq!(after.projects, baseline.projects);
    assert_eq!(after.threads, baseline.threads);
    assert_eq!(after.collapsed, baseline.collapsed);
    assert_eq!(route_state(&fixture), route);
    assert_no_menu(&mut fixture);
}

#[gpui_kit::test]
async fn organization_menu_placement_group_projection_uses_current_button_and_pointer(
    cx: &mut TestAppContext,
) {
    let mut fixture = Fixture::new(
        cx,
        FixtureConfig {
            height: 1000.0,
            projection: Projection::Groups,
            tasks_per_project: 2,
            ..Default::default()
        },
    )
    .await;
    let baseline = persisted(&fixture);
    let route = route_state(&fixture);
    let a = fixture.groups[0].clone();
    let b = fixture.groups[1].clone();
    let a_button = fixture.bounds(format!("group-menu-{a}"));
    fixture.click(format!("group-menu-{a}"));
    fixture.assert_button_anchor(a_button);
    assert_single_menu(&mut fixture);
    assert!(matches!(&commands(&fixture)[0].1, MenuCommand::NewTask(id) if id == &a));
    assert!(matches!(&commands(&fixture)[1].1, MenuCommand::Rename(id) if id == &a));
    fixture.key("escape");
    assert_no_menu(&mut fixture);

    let b_header = fixture.bounds(format!("group-header-{b}"));
    let b_pointer = point(b_header.left() + px(48.0), b_header.center().y);
    fixture.right_click(b_pointer);
    assert_pointer_anchor(&mut fixture, b_pointer);
    assert!(matches!(&commands(&fixture)[1].1, MenuCommand::Rename(id) if id == &b));
    fixture.key("escape");
    let b_button = fixture.bounds(format!("group-menu-{b}"));
    fixture.click(format!("group-menu-{b}"));
    fixture.assert_button_anchor(b_button);
    assert_single_menu(&mut fixture);
    assert!(matches!(&commands(&fixture)[0].1, MenuCommand::NewTask(id) if id == &b));
    fixture.key("escape");

    let a_header = fixture.bounds(format!("group-header-{a}"));
    let a_pointer = point(a_header.left() + px(72.0), a_header.center().y);
    fixture.right_click(a_pointer);
    assert_pointer_anchor(&mut fixture, a_pointer);
    assert!((a_pointer.y - b_pointer.y).abs() > px(1.0));
    let red = command_index(
        &fixture,
        |command| matches!(command, MenuCommand::Apply(SidebarOrganizationAction::SetGroupColor { group_id, color: SidebarGroupColor::Red }) if group_id == &a),
    );
    fixture.click(format!("organization-menu-{red}"));
    settle_commands(&mut fixture).await;
    let after = persisted(&fixture);
    assert_eq!(
        after
            .groups
            .iter()
            .find(|group| group.id == a)
            .expect("target group")
            .color,
        SidebarGroupColor::Red
    );
    assert_eq!(
        after.groups.iter().find(|group| group.id == b),
        baseline.groups.iter().find(|group| group.id == b)
    );
    assert_eq!(after.memberships, baseline.memberships);
    assert_eq!(after.threads, baseline.threads);
    assert_eq!(after.collapsed, baseline.collapsed);
    assert_eq!(route_state(&fixture), route);
    assert_no_menu(&mut fixture);
}

#[gpui_kit::test]
async fn organization_menu_placement_reopen_uses_scrolled_resized_current_layout(
    cx: &mut TestAppContext,
) {
    let mut fixture = Fixture::new(
        cx,
        FixtureConfig {
            height: 800.0,
            ..Default::default()
        },
    )
    .await;
    let project = fixture.projects[1].clone();
    let _spacers = insert_spacer_projects(&mut fixture, &project, 3).await;
    let _trailing_spacers = insert_spacer_projects_at(&mut fixture, None, 3).await;
    fixture.block.update(&mut fixture.visual.cx, |block, cx| {
        block.projects_expanded = true;
        cx.notify();
    });
    fixture.settle();
    let baseline = persisted(&fixture);
    let route = route_state(&fixture);
    let id = format!("project-more-{project}");
    let scroll = fixture.bounds("sidebar-scroll");
    let first = place_control_in_scroll(&mut fixture, &id, scroll.top() + px(160.0));
    fixture.click(id.clone());
    fixture.assert_button_anchor(first);
    let first_menu = assert_single_menu(&mut fixture);
    assert_project_target(&mut fixture, &project);
    fixture.key("escape");
    assert_no_menu(&mut fixture);

    wheel(&mut fixture, scroll.center(), px(-48.0));
    let scrolled = fixture.bounds(id.clone());
    assert_near(scrolled.top(), first.top() - px(48.0));
    fixture.click(id.clone());
    fixture.assert_button_anchor(scrolled);
    let scrolled_menu = assert_single_menu(&mut fixture);
    assert_near(scrolled_menu.top(), first_menu.top() - px(48.0));
    fixture.key("escape");
    assert_no_menu(&mut fixture);

    fixture.visual.update(|_, cx| {
        cx.set_global(SidebarWidth(365.0));
        cx.refresh_windows();
    });
    fixture.settle();
    let wider = fixture.bounds(id.clone());
    assert!((wider.right() - scrolled.right()).abs() > px(1.0));
    fixture.click(id.clone());
    fixture.assert_button_anchor(wider);
    let wider_menu = assert_single_menu(&mut fixture);
    assert_near(
        wider_menu.right() - scrolled_menu.right(),
        wider.right() - scrolled.right(),
    );
    fixture.key("escape");

    fixture.visual.simulate_resize(size(px(1000.0), px(720.0)));
    fixture.settle();
    let scroll = fixture.bounds("sidebar-scroll");
    let resized = place_control_in_scroll(&mut fixture, &id, scroll.top() + px(160.0));
    fixture.click(id.clone());
    fixture.assert_button_anchor(resized);
    assert_single_menu(&mut fixture);
    fixture.key("escape");

    let scroll = fixture.bounds("sidebar-scroll");
    let low = place_control_in_scroll(&mut fixture, &id, scroll.top() + px(380.0));
    fixture.click(id);
    fixture.assert_button_anchor(low);
    let before_shrink = assert_single_menu(&mut fixture);
    fixture.visual.simulate_resize(size(px(960.0), px(600.0)));
    fixture.settle();
    let after_shrink = assert_single_menu(&mut fixture);
    assert!(after_shrink.top() < before_shrink.top() - px(1.0));
    let viewport = viewport(&mut fixture);
    assert!(after_shrink.bottom() <= viewport.bottom() - px(7.0));
    assert_project_target(&mut fixture, &project);
    fixture.key("escape");
    assert_no_menu(&mut fixture);
    assert_eq!(persisted(&fixture), baseline);
    assert_eq!(route_state(&fixture), route);
}

#[gpui_kit::test]
async fn organization_menu_placement_minimum_window_surface_escapes_sidebar_clip(
    cx: &mut TestAppContext,
) {
    for dark in [false, true] {
        for sidebar_width in [240.0, 365.0] {
            let mut isolated = cx.new_app();
            let mut fixture = Fixture::new(
                &mut isolated,
                FixtureConfig {
                    width: 960.0,
                    height: 600.0,
                    sidebar_width,
                    dark,
                    ..Default::default()
                },
            )
            .await;
            let target = fixture.projects[1].clone();
            let _spacers = insert_spacer_projects(&mut fixture, &target, 3).await;
            let baseline = persisted(&fixture);
            let route = route_state(&fixture);
            let scroll = fixture.bounds("sidebar-scroll");
            let button_id = format!("project-more-{target}");
            let low_button =
                place_control_in_scroll(&mut fixture, &button_id, scroll.bottom() - px(24.0));
            fixture.click(button_id);
            let menu = assert_single_menu(&mut fixture);
            assert_inside(menu, viewport(&mut fixture));
            let sidebar = fixture.bounds("sidebar");
            assert_near(sidebar.size.width, px(sidebar_width));
            assert!(low_button.bottom() > scroll.bottom() - px(24.0));
            assert!(menu.bottom() > scroll.bottom() + px(1.0));
            assert_project_target(&mut fixture, &target);
            let previous = baseline.project_order[baseline.project_order.len() - 2].clone();
            let move_up = command_index(
                &fixture,
                |command| matches!(command, MenuCommand::Apply(SidebarOrganizationAction::MoveProject { project_id, before_id }) if project_id == &target && before_id.as_ref() == Some(&previous)),
            );
            let item = fixture.bounds(format!("organization-menu-{move_up}"));
            let outside_scroll = point(item.center().x, item.bottom() - px(2.0));
            let outside_sidebar = point(sidebar.right() + px(2.0), item.center().y);
            let hit = if sidebar_width == 240.0 {
                assert!(menu.right() > sidebar.right() + px(1.0));
                outside_sidebar
            } else {
                outside_scroll
            };
            assert!(
                item.contains(&hit),
                "real command contains {hit:?}: {item:?}"
            );
            assert!(
                !scroll.contains(&hit),
                "command hit must escape sidebar scroll mask"
            );
            let surfaces = menu_surfaces(&mut fixture);
            assert!(surfaces[0].1.contains(&hit));
            fixture.visual.simulate_click(hit, Modifiers::none());
            fixture.settle();
            settle_commands(&mut fixture).await;
            let after = persisted(&fixture);
            let mut expected_order = baseline.project_order.clone();
            let last = expected_order.len() - 1;
            assert_eq!(expected_order[last], target);
            expected_order.swap(last - 1, last);
            assert_eq!(after.project_order, expected_order);
            assert_eq!(after.projects, baseline.projects);
            assert_eq!(after.threads, baseline.threads);
            assert_eq!(after.collapsed, baseline.collapsed);
            assert_eq!(route_state(&fixture), route);
            assert_no_menu(&mut fixture);
            isolated.quit();
        }
    }
}

#[gpui_kit::test]
async fn organization_menu_placement_long_group_menu_reaches_and_executes_last_item(
    cx: &mut TestAppContext,
) {
    for keyboard in [false, true] {
        let mut isolated = cx.new_app();
        let mut fixture = Fixture::new(
            &mut isolated,
            FixtureConfig {
                width: 960.0,
                height: 320.0,
                sidebar_width: 240.0,
                tasks_per_project: 1,
                groups: 3,
                projection: Projection::Groups,
                ..Default::default()
            },
        )
        .await;
        let target = fixture.groups[0].clone();
        let baseline = persisted(&fixture);
        let route = route_state(&fixture);
        fixture.click(format!("group-menu-{target}"));
        let menu = assert_single_menu(&mut fixture);
        assert_near(menu.size.height, px(304.0));
        let items = commands(&fixture);
        let last = items.len() - 1;
        assert!(matches!(
            &items[last],
            (label, MenuCommand::Apply(SidebarOrganizationAction::DissolveGroup { group_id }))
                if label == "取消分组并删除" && group_id == &target
        ));
        let last_id = format!("organization-menu-{last}");
        let before = fixture.bounds(last_id.clone());
        assert!(before.bottom() > menu.bottom() + px(1.0));
        if keyboard {
            fixture.key("up");
            assert_highlight(&mut fixture, last);
        } else {
            wheel(&mut fixture, menu.center(), -px(320.0));
        }
        let visible = fixture.bounds(last_id.clone());
        let menu = fixture.menu_bounds();
        assert!(visible.top() < before.top() - px(1.0));
        assert_inside(visible, menu);
        assert_single_menu(&mut fixture);
        if keyboard {
            fixture.key("enter");
        } else {
            fixture.click(last_id);
        }
        settle_commands(&mut fixture).await;
        let after = persisted(&fixture);
        assert_eq!(after.groups.len(), baseline.groups.len() - 1);
        assert!(!after.groups.iter().any(|group| group.id == target));
        assert_eq!(
            after.groups,
            baseline
                .groups
                .iter()
                .filter(|group| group.id != target)
                .cloned()
                .collect::<Vec<_>>()
        );
        assert_eq!(
            after.memberships,
            baseline
                .memberships
                .iter()
                .filter(|membership| membership.group_id != target)
                .cloned()
                .collect::<Vec<_>>()
        );
        assert_eq!(after.threads, baseline.threads);
        assert_eq!(route_state(&fixture), route);
        assert_no_menu(&mut fixture);
        isolated.quit();
    }
}

#[gpui_kit::test]
async fn organization_menu_placement_switch_close_remove_preserves_other_project_and_draft(
    cx: &mut TestAppContext,
) {
    let mut fixture = Fixture::new(cx, FixtureConfig::default()).await;
    let a = fixture.projects[0].clone();
    let b = fixture.projects[1].clone();
    let task_ids: Vec<_> = fixture.tasks.iter().flatten().cloned().collect();
    fixture.visual.update(|_, cx| {
        let store = cx
            .global::<VegaStore>()
            .0
            .as_ref()
            .expect("owned file store");
        for task_id in &task_ids {
            let row = vega_store::messages::MessageRow {
                id: format!("history-{task_id}"),
                thread_id: task_id.clone(),
                seq: 1,
                role: "user".into(),
                kind: "text".into(),
                content: format!("Retained history for {task_id}"),
                status: "done".into(),
                created_at: 1,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            };
            vega_store::messages::insert(store.conn(), &row).expect("owned history seed");
        }
    });
    let history = fixture.visual.cx.read(|cx| {
        let store = cx
            .global::<VegaStore>()
            .0
            .as_ref()
            .expect("owned file store");
        task_ids
            .iter()
            .map(|id| {
                let page = vega_conversation::history::latest_history_page(store, id, 20)
                    .expect("owned history");
                assert_eq!(page.entries.len(), 1);
                (id.clone(), page.entries)
            })
            .collect::<Vec<_>>()
    });
    let baseline = persisted(&fixture);
    let route = route_state(&fixture);
    assert_eq!(route.0.as_deref(), Some(a.as_str()));
    assert!(!route.2.is_empty());
    let a_id = format!("project-more-{a}");
    let b_id = format!("project-more-{b}");
    let a_button = fixture.bounds(a_id.clone());
    fixture.click(a_id);
    fixture.assert_button_anchor(a_button);
    assert_project_target(&mut fixture, &a);
    let a_menu = assert_single_menu(&mut fixture);
    assert_highlight(&mut fixture, 0);
    fixture.key("down");
    assert_highlight(&mut fixture, 1);
    assert_eq!(persisted(&fixture), baseline);
    assert_eq!(route_state(&fixture), route);

    let b_button = fixture.bounds(b_id.clone());
    fixture.click(b_id.clone());
    fixture.assert_button_anchor(b_button);
    let b_menu = assert_single_menu(&mut fixture);
    assert!((b_menu.top() - a_menu.top()).abs() > px(1.0));
    assert_project_target(&mut fixture, &b);
    assert_highlight(&mut fixture, 0);
    fixture.key("escape");
    assert_no_menu(&mut fixture);
    assert_eq!(fixture.outer_closes.get(), 0);
    assert!(fixture.visual.cx.read(|cx| cx.global::<SettingsOpen>().0));
    assert_eq!(route_state(&fixture), route);
    assert_eq!(persisted(&fixture), baseline);

    fixture.click(b_id.clone());
    fixture.assert_button_anchor(b_button);
    assert_single_menu(&mut fixture);
    let outside = fixture.outside();
    fixture.visual.simulate_click(outside, Modifiers::none());
    fixture.settle();
    assert_no_menu(&mut fixture);
    assert_eq!(route_state(&fixture), route);
    assert_eq!(persisted(&fixture), baseline);

    fixture.visual.update(|_, cx| {
        cx.set_global(SidebarWidth(365.0));
        cx.refresh_windows();
    });
    fixture.settle();
    let current_button = fixture.bounds(b_id.clone());
    assert!((current_button.right() - b_button.right()).abs() > px(1.0));
    fixture.click(b_id);
    fixture.assert_button_anchor(current_button);
    assert_single_menu(&mut fixture);
    assert_project_target(&mut fixture, &b);
    assert_highlight(&mut fixture, 0);
    fixture.click("organization-menu-0");
    settle_commands(&mut fixture).await;
    assert_no_menu(&mut fixture);
    let after = persisted(&fixture);
    assert_eq!(
        after.projects,
        baseline
            .projects
            .iter()
            .filter(|project| project.id != b)
            .cloned()
            .collect::<Vec<_>>()
    );
    assert_eq!(after.project_order, vec![a]);
    assert_eq!(after.groups, baseline.groups);
    assert_eq!(after.collapsed, baseline.collapsed);
    assert_eq!(after.threads.len(), baseline.threads.len());
    for thread in &baseline.threads {
        let mut expected = thread.clone();
        if expected.project_id == b {
            expected.project_id.clear();
        }
        assert_eq!(
            after.threads.iter().find(|saved| saved.id == thread.id),
            Some(&expected)
        );
    }
    fixture.visual.cx.read(|cx| {
        let store = cx
            .global::<VegaStore>()
            .0
            .as_ref()
            .expect("owned file store");
        for (id, entries) in history {
            let page = vega_conversation::history::latest_history_page(store, &id, 20)
                .expect("retained history");
            assert_eq!(page.entries, entries);
        }
    });
    assert!(
        fixture
            .visual
            .debug_bounds(selector(format!("project-header-{b}")))
            .is_none()
    );
    assert_eq!(route_state(&fixture), route);
    assert_eq!(fixture.outer_closes.get(), 0);
}

#[gpui_kit::test]
async fn organization_menu_placement_task_menu_preserves_pointer_keyboard_commands_and_close(
    cx: &mut TestAppContext,
) {
    let mut fixture = Fixture::new(
        cx,
        FixtureConfig {
            height: 1100.0,
            tasks_per_project: 9,
            ..Default::default()
        },
    )
    .await;
    let task = fixture.tasks[0][1].clone();
    let other = fixture.tasks[1][0].clone();
    let baseline = persisted(&fixture);
    let original = baseline
        .threads
        .iter()
        .find(|thread| thread.id == task)
        .expect("owned task")
        .clone();
    let route = route_state(&fixture);
    let focus = fixture.visual.cx.read(|cx| {
        fixture
            .block
            .read(cx)
            .thread_action_focuses
            .get(&task)
            .expect("real task trigger focus")
            .clone()
    });
    fixture.visual.update(|window, cx| focus.focus(window, cx));
    fixture.key("left");
    fixture.assert_focused(&focus);
    assert_no_menu(&mut fixture);
    let row = fixture.bounds(format!("project-thread-row-{task}"));
    let hover = fixture.visual.cx.read(|cx| theme(cx).colors.bg_hover);
    let triggers: Vec<_> = painted_backgrounds(&mut fixture, hover)
        .into_iter()
        .filter(|(bounds, mask)| {
            (bounds.size.width - px(Layout::SIDEBAR_ACTION_SIZE)).abs() <= px(1.0)
                && (bounds.size.height - px(Layout::SIDEBAR_ACTION_SIZE)).abs() <= px(1.0)
                && row.contains(&bounds.center())
                && mask.contains(&bounds.center())
        })
        .collect();
    assert_eq!(
        triggers.len(),
        1,
        "actual focused task trigger before opening: {triggers:?}"
    );
    let trigger = triggers[0].0;
    assert_inside(trigger, row);
    assert_inside(trigger, triggers[0].1);
    let anchor_gap = px(28.0) - px(Layout::SIDEBAR_ACTION_SIZE);
    fixture.key("space");
    let keyboard_surfaces = menu_surfaces(&mut fixture);
    assert_eq!(keyboard_surfaces.len(), 1);
    let keyboard_menu = keyboard_surfaces[0].0;
    assert_inside(keyboard_menu, keyboard_surfaces[0].1);
    assert_inside(keyboard_menu, viewport(&mut fixture));
    assert_near(keyboard_menu.right(), trigger.right() + anchor_gap);
    assert_near(keyboard_menu.top(), trigger.bottom() + anchor_gap);
    let first_item = fixture.visual.cx.read(|cx| {
        let scroll = &fixture.block.read(cx).actions_scroll;
        assert_eq!(scroll.top_item(), 0);
        assert!(
            scroll.bounds_for_item(8).is_some(),
            "complete existing task commands"
        );
        scroll.bounds_for_item(0).expect("real first task command")
    });
    assert_near(first_item.size.height, px(Typography::SIDEBAR_LINE_HEIGHT));
    fixture.assert_focused(&focus);
    for key in ["tab", "shift-tab"] {
        fixture.key(key);
        fixture.assert_focused(&focus);
        let surfaces = menu_surfaces(&mut fixture);
        assert_eq!(surfaces.len(), 1);
        assert_same_bounds(surfaces[0].0, keyboard_menu);
        assert_inside(surfaces[0].0, surfaces[0].1);
        let active = fixture.visual.cx.read(|cx| theme(cx).colors.bg_active);
        let highlights: Vec<_> = painted_backgrounds(&mut fixture, active)
            .into_iter()
            .filter(|(bounds, mask)| {
                keyboard_menu.contains(&bounds.center())
                    && mask.contains(&bounds.center())
                    && (bounds.size.width - first_item.size.width).abs() <= px(1.0)
                    && (bounds.size.height - first_item.size.height).abs() <= px(1.0)
            })
            .collect();
        assert_eq!(
            highlights.len(),
            1,
            "unchanged task command: {highlights:?}"
        );
        assert_same_bounds(highlights[0].0, first_item);
        assert_eq!(persisted(&fixture), baseline);
        assert_eq!(route_state(&fixture), route);
    }
    fixture.key("escape");
    assert_no_menu(&mut fixture);
    assert_eq!(fixture.outer_closes.get(), 0);
    assert_eq!(persisted(&fixture), baseline);
    assert_eq!(route_state(&fixture), route);

    let row = fixture.bounds(format!("project-thread-row-{task}"));
    let pointer = point(row.left() + px(48.0), row.center().y);
    fixture.right_click(pointer);
    let surfaces = menu_surfaces(&mut fixture);
    assert_eq!(surfaces.len(), 1);
    let pointer_menu = surfaces[0].0;
    assert_near(pointer_menu.right(), trigger.right() + anchor_gap);
    assert_near(pointer_menu.top(), trigger.bottom() + anchor_gap);
    assert!((pointer_menu.left() - pointer.x).abs() > px(1.0));
    assert_inside(pointer_menu, surfaces[0].1);
    let pin_item = fixture.visual.cx.read(|cx| {
        fixture
            .block
            .read(cx)
            .actions_scroll
            .bounds_for_item(0)
            .expect("real pointer-open pin command")
    });
    assert_near(pin_item.size.height, px(Typography::SIDEBAR_LINE_HEIGHT));
    assert_inside(pin_item, pointer_menu);
    let pin_hit = pin_item.center();
    assert!(surfaces[0].1.contains(&pin_hit));
    fixture.visual.simulate_click(pin_hit, Modifiers::none());
    fixture.settle();
    settle_commands(&mut fixture).await;
    let pinned = persisted(&fixture);
    let saved = pinned
        .threads
        .iter()
        .find(|thread| thread.id == task)
        .expect("pinned task persists");
    let mut expected = original.clone();
    expected.pinned = !original.pinned;
    assert_eq!(saved, &expected);
    assert_eq!(
        pinned.threads.iter().find(|thread| thread.id == other),
        baseline.threads.iter().find(|thread| thread.id == other)
    );
    assert_no_menu(&mut fixture);
    assert_eq!(route_state(&fixture), route);

    fixture.visual.update(|window, cx| focus.focus(window, cx));
    fixture.key("enter");
    let surfaces = menu_surfaces(&mut fixture);
    assert_eq!(surfaces.len(), 1);
    assert_inside(surfaces[0].0, surfaces[0].1);
    fixture.key("down down down enter");
    settle_commands(&mut fixture).await;
    let marked = persisted(&fixture);
    expected.unread = !original.unread;
    assert_eq!(
        marked.threads.iter().find(|thread| thread.id == task),
        Some(&expected)
    );
    assert_eq!(
        marked.threads.iter().find(|thread| thread.id == other),
        baseline.threads.iter().find(|thread| thread.id == other)
    );
    assert_no_menu(&mut fixture);
    assert_eq!(route_state(&fixture), route);

    fixture.visual.update(|window, cx| focus.focus(window, cx));
    fixture.key("space");
    assert_eq!(menu_surfaces(&mut fixture).len(), 1);
    let project = fixture.projects[1].clone();
    let button = fixture.bounds(format!("project-more-{project}"));
    fixture.click(format!("project-more-{project}"));
    fixture.assert_button_anchor(button);
    assert_single_menu(&mut fixture);
    assert_project_target(&mut fixture, &project);
    fixture.key("escape");
    assert_no_menu(&mut fixture);
    assert_eq!(persisted(&fixture), marked);
    assert_eq!(route_state(&fixture), route);
    assert_eq!(fixture.outer_closes.get(), 0);
}
