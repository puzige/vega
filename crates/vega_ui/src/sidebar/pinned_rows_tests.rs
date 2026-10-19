use super::{
    NewTaskDefaults, OpenedThread, PendingDeleteConfirm, ProjectsCollapsed, RunningThreadsGlobal,
    SelectedProject, SessionsCollapsed, Sidebar, SidebarCollapsed, SidebarWidth, VegaStore,
    set_thread_running,
};
use crate::settings::SettingsOpen;
use gpui_kit::component::Root;
use gpui_kit::test::{ElementSnapshot, TestWindowExt};
use gpui_kit::{
    App, AppContext, BorrowAppContext, Bounds, Entity, Global, Hsla, InputEvent, MouseMoveEvent,
    Pixels, Point, ScrollDelta, ScrollWheelEvent, TestAppContext, VisualTestContext, Window,
    WindowHandle, point, px, size,
};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use tempfile::TempDir;
use vega_conversation::sidebar_organization as organization;
use vega_conversation::threads as conversation;
use vega_conversation::types::{SidebarOrganizationAction, Thread, ThreadStatus};
use vega_store::{Store, projects};
use vega_theme::Theme;

#[derive(Clone, Copy)]
struct PaintedRegion {
    clip: Bounds<Pixels>,
    font_size: Pixels,
}

#[derive(Default)]
struct PaintedRegions(HashMap<String, PaintedRegion>);

impl Global for PaintedRegions {}

pub(super) fn record_region(id: &str, window: &Window, cx: &mut App) {
    let region = PaintedRegion {
        clip: window.content_mask().bounds,
        font_size: window.text_style().font_size.to_pixels(window.rem_size()),
    };
    cx.update_global::<PaintedRegions, _>(|regions, _| {
        regions.0.insert(id.to_owned(), region);
    });
}

struct Fixture {
    directory: TempDir,
    database: PathBuf,
    window: WindowHandle<Root>,
    sidebar: Entity<Sidebar>,
    pinned: Vec<Thread>,
    project_rows: Vec<Thread>,
    recent_rows: Vec<Thread>,
    order: Vec<String>,
}

impl Fixture {
    fn new(cx: &mut TestAppContext, count: usize, width: f32, dark: bool) -> Self {
        let directory = tempfile::tempdir().expect("temporary sidebar fixture");
        let database = directory.path().join("sidebar.db");
        let project_path = directory.path().join("project");
        std::fs::create_dir(&project_path).expect("isolated project directory");
        let store = Store::open(&database).expect("file store");
        store.migrate().expect("fixture migrations");
        let project = projects::create(
            store.conn(),
            project_path.to_str().expect("fixture path"),
            "Geometry project",
            None,
        )
        .expect("fixture project");
        let pinned: Vec<_> = (0..count)
            .map(|index| {
                let binding = (index % 2 == 0).then_some(project.id.as_str());
                let thread = create_task(&store, binding, &format!("Pinned {:02}", index + 1));
                conversation::set_thread_pinned(&store, &thread.id, true)
                    .expect("pin fixture task");
                conversation::get_thread(&store, &thread.id).expect("pinned fixture task")
            })
            .collect();
        let project_rows = vec![
            create_task(&store, Some(&project.id), "Project first"),
            create_task(&store, Some(&project.id), "Project second"),
        ];
        let recent_rows = vec![
            create_task(&store, None, "Recent first"),
            create_task(&store, None, "Recent second"),
        ];
        let order: Vec<_> = pinned
            .iter()
            .chain(&project_rows)
            .chain(&recent_rows)
            .map(|thread| thread.id.clone())
            .collect();
        let mut snapshot = organization::snapshot(&store).expect("initial organization");
        for id in &order {
            snapshot = organization::apply(
                &store,
                snapshot.revision,
                SidebarOrganizationAction::ReorderThread {
                    thread_id: id.clone(),
                    before_id: None,
                },
            )
            .expect("explicit fixture order")
            .snapshot;
        }
        assert_eq!(snapshot.preferences.manual_thread_order, order);
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::init(cx);
            cx.set_reduce_motion(true);
            cx.set_global(if dark { Theme::dark() } else { Theme::light() });
            cx.set_global(SidebarWidth(width));
            cx.set_global(SidebarCollapsed(false));
            cx.set_global(ProjectsCollapsed(false));
            cx.set_global(SessionsCollapsed(false));
            cx.set_global(SelectedProject(None));
            cx.set_global(OpenedThread(None));
            cx.set_global(PendingDeleteConfirm(None));
            cx.set_global(SettingsOpen(false));
            cx.set_global(NewTaskDefaults::default());
            cx.set_global(RunningThreadsGlobal(Default::default()));
            cx.set_global(PaintedRegions::default());
            cx.set_global(VegaStore(Ok(store)));
        });
        let mut sidebar = None;
        let window = cx.open_window(size(px(960.), px(600.)), |window, cx| {
            let view = cx.new(Sidebar::new);
            let projects = view.read(cx).projects_block.clone();
            projects.update(cx, |projects, _| projects.disable_branch_probe());
            sidebar = Some(view.clone());
            Root::new(view, window, cx)
        });
        let fixture = Self {
            directory,
            database,
            window,
            sidebar: sidebar.expect("mounted Sidebar"),
            pinned,
            project_rows,
            recent_rows,
            order,
        };
        fixture.settle(cx);
        cx.read(|cx| {
            let sidebar = fixture.sidebar.read(cx);
            assert_eq!(sidebar.sessions_block.read(cx).threads.len(), count + 4);
        });
        fixture
    }

    fn visual(&self, cx: &TestAppContext) -> VisualTestContext {
        VisualTestContext::from_window(self.window.into(), cx)
    }

    fn update<R>(
        &self,
        cx: &mut TestAppContext,
        update: impl FnOnce(&mut Window, &mut App) -> R,
    ) -> R {
        cx.update_window(self.window.into(), |_, window, cx| update(window, cx))
            .expect("mounted test window")
    }

    fn settle(&self, cx: &mut TestAppContext) {
        cx.run_until_parked();
        self.update(cx, |window, cx| {
            cx.update_global::<PaintedRegions, _>(|regions, _| regions.0.clear());
            window.render_frame(cx);
        });
        cx.run_until_parked();
        self.update(cx, |window, cx| {
            cx.update_global::<PaintedRegions, _>(|regions, _| regions.0.clear());
            window.render_frame(cx);
        });
        cx.read(|cx| {
            let sidebar = self.sidebar.read(cx);
            assert!(sidebar.new_task_error.is_none(), "new task error");
            assert!(
                sidebar.projects_block.read(cx).error.is_none(),
                "project error"
            );
            assert!(
                sidebar.sessions_block.read(cx).error.is_none(),
                "thread error: {:?}",
                sidebar.sessions_block.read(cx).error
            );
        });
    }

    fn selector(&self, cx: &TestAppContext, selector: &'static str) -> Bounds<Pixels> {
        self.visual(cx)
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("missing rendered selector {selector}"))
    }

    fn viewport(&self, cx: &TestAppContext) -> Bounds<Pixels> {
        self.selector(cx, "organization-pinned-scroll")
    }

    fn node(&self, cx: &mut TestAppContext, id: &str) -> ElementSnapshot {
        self.update(cx, |window, _| {
            window.find(gpui_kit::SharedString::from(id.to_owned()))
        })
    }

    fn optional_node(&self, cx: &mut TestAppContext, id: &str) -> Option<ElementSnapshot> {
        self.update(cx, |window, _| {
            window.try_find(gpui_kit::SharedString::from(id.to_owned()))
        })
    }

    fn click(&self, cx: &mut TestAppContext, id: &str) {
        self.update(cx, |window, cx| {
            window.click(gpui_kit::SharedString::from(id.to_owned()), cx);
        });
        self.settle(cx);
    }

    fn hover(&self, cx: &mut TestAppContext, id: &str) {
        self.move_outside(cx);
        self.update(cx, |window, cx| {
            window.hover(gpui_kit::SharedString::from(id.to_owned()), cx);
        });
        self.settle(cx);
    }

    fn move_outside(&self, cx: &mut TestAppContext) {
        self.update(cx, |window, cx| {
            window.dispatch_event(
                MouseMoveEvent {
                    position: point(px(900.), px(550.)),
                    pressed_button: None,
                    modifiers: Default::default(),
                }
                .to_platform_input(),
                cx,
            );
            window.render_frame(cx);
        });
        self.settle(cx);
    }

    fn scroll(&self, cx: &mut TestAppContext, amount: f32) {
        let position = self.viewport(cx).center();
        self.visual(cx).simulate_event(ScrollWheelEvent {
            position,
            delta: ScrollDelta::Pixels(point(px(0.), px(amount))),
            ..Default::default()
        });
        self.settle(cx);
    }

    fn outer_offset(&self, cx: &TestAppContext) -> Point<Pixels> {
        cx.read(|cx| self.sidebar.read(cx).sidebar_scroll.offset())
    }

    fn persisted(&self, id: &str) -> Thread {
        let store = Store::open(&self.database).expect("new persistence connection");
        conversation::get_thread(&store, id).expect("persisted task")
    }

    fn assert_persistence(&self, pinned_ids: &[String]) {
        let store = Store::open(&self.database).expect("new organization connection");
        let snapshot = organization::snapshot(&store).expect("persisted organization");
        assert_eq!(snapshot.preferences.manual_thread_order, self.order);
        assert_eq!(snapshot.threads.len(), self.order.len());
        let actual: HashSet<_> = snapshot
            .threads
            .iter()
            .filter(|thread| thread.pinned)
            .map(|thread| thread.id.clone())
            .collect();
        assert_eq!(actual, pinned_ids.iter().cloned().collect::<HashSet<_>>());
        let all: HashSet<_> = snapshot.threads.iter().map(|thread| &thread.id).collect();
        assert_eq!(all.len(), self.order.len());
    }

    fn close(self, cx: &mut TestAppContext) {
        self.update(cx, |window, _| window.remove_window());
        drop(self.sidebar);
        cx.run_until_parked();
        cx.update(|cx| cx.set_global(VegaStore(Err("fixture closed".into()))));
        drop(self.directory);
    }
}

fn create_task(store: &Store, project: Option<&str>, title: &str) -> Thread {
    let thread = match project {
        Some(project) => conversation::create_thread(store, project, "fixture-model", "confirm"),
        None => conversation::create_standalone_thread(store, "fixture-model", "confirm"),
    }
    .expect("fixture task");
    conversation::rename_thread(store, &thread.id, title).expect("fixture title")
}

fn near(actual: Pixels, expected: f32) {
    assert!(
        (f32::from(actual) - expected).abs() <= 0.5,
        "actual {actual:?}, expected {expected}px"
    );
}

fn contained(outer: Bounds<Pixels>, inner: Bounds<Pixels>) {
    assert!(
        inner.left() >= outer.left() - px(0.5)
            && inner.top() >= outer.top() - px(0.5)
            && inner.right() <= outer.right() + px(0.5)
            && inner.bottom() <= outer.bottom() + px(0.5),
        "bounds {inner:?} escape {outer:?}"
    );
}

fn same_bounds(actual: Bounds<Pixels>, expected: Bounds<Pixels>) {
    near(actual.left() - expected.left(), 0.);
    near(actual.top() - expected.top(), 0.);
    near(actual.size.width - expected.size.width, 0.);
    near(actual.size.height - expected.size.height, 0.);
}

fn ids(threads: &[Thread]) -> Vec<String> {
    threads.iter().map(|thread| thread.id.clone()).collect()
}

fn row_id(prefix: &str, id: &str) -> String {
    format!("{prefix}thread-row-{id}")
}

fn title_id(prefix: &str, id: &str) -> String {
    format!("{prefix}thread-row-title-{id}")
}

fn action_id(prefix: &str, action: &str, id: &str) -> String {
    format!("{prefix}thread-{action}-{id}")
}

fn assert_projection(fixture: &Fixture, cx: &mut TestAppContext, id: &str, expected: &str) {
    for prefix in ["pinned-", "project-", "standalone-"] {
        let node = fixture.optional_node(cx, &row_id(prefix, id));
        if prefix == expected {
            assert!(node.is_some(), "task {id} missing from {expected}");
        } else {
            assert!(node.is_none(), "task {id} duplicated in {prefix}");
        }
    }
}

fn assert_row(
    fixture: &Fixture,
    cx: &mut TestAppContext,
    prefix: &str,
    id: &str,
    viewport: Bounds<Pixels>,
    inset: f32,
) -> Bounds<Pixels> {
    let row = fixture.node(cx, &row_id(prefix, id));
    assert!(row.visible(), "row {id} must be visible");
    near(row.bounds().size.height, 32.);
    contained(viewport, row.bounds());
    let title_id = title_id(prefix, id);
    let title = fixture.node(cx, &title_id);
    assert!(title.visible(), "title {id} must be visible");
    contained(row.bounds(), title.bounds());
    near(title.bounds().left() - row.bounds().left(), inset);
    let painted = cx.read(|cx| {
        *cx.global::<PaintedRegions>()
            .0
            .get(&title_id)
            .expect("title prepaint observation")
    });
    near(painted.font_size, 13.);
    contained(painted.clip, title.bounds());
    let slot_id = action_id(prefix, "slot", id);
    let slot = fixture.node(cx, &slot_id);
    assert!(slot.visible(), "action slot {id}");
    contained(row.bounds(), slot.bounds());
    let slot_clip = cx.read(|cx| {
        cx.global::<PaintedRegions>()
            .0
            .get(&slot_id)
            .expect("action slot prepaint observation")
            .clip
    });
    contained(slot_clip, slot.bounds());
    near(slot.bounds().size.width, 52.);
    near(slot.bounds().size.height, 24.);
    near(row.bounds().right() - slot.bounds().right(), 8.);
    assert_projection(fixture, cx, id, prefix);
    row.bounds()
}

fn assert_page(
    fixture: &Fixture,
    cx: &mut TestAppContext,
    tasks: &[Thread],
    first: usize,
    count: usize,
) {
    let viewport = fixture.viewport(cx);
    near(
        viewport.size.height,
        if tasks.len() == 1 { 32. } else { 160. },
    );
    let mut previous = None;
    for thread in &tasks[first..first + count] {
        let row = assert_row(fixture, cx, "pinned-", &thread.id, viewport, 8.);
        let label = fixture.node(cx, &row_id("pinned-", &thread.id));
        assert_eq!(label.label(), Some(thread.title.as_str()));
        if let Some(previous) = previous {
            near(row.top() - previous, 32.);
        } else {
            near(row.top() - viewport.top(), 0.);
        }
        previous = Some(row.top());
    }
    let last = fixture.node(cx, &row_id("pinned-", &tasks[first + count - 1].id));
    near(last.bounds().bottom() - viewport.top(), count as f32 * 32.);
}

fn assert_opened(cx: &TestAppContext, id: &str) {
    cx.read(|cx| {
        assert_eq!(
            cx.global::<OpenedThread>()
                .0
                .as_ref()
                .map(|thread| thread.id.as_str()),
            Some(id)
        );
    });
}

#[derive(Clone, Copy)]
struct RowGeometry {
    row: Bounds<Pixels>,
    title: Bounds<Pixels>,
    slot: Bounds<Pixels>,
}

impl RowGeometry {
    fn observe(fixture: &Fixture, cx: &mut TestAppContext, id: &str) -> Self {
        let viewport = fixture.viewport(cx);
        Self {
            row: assert_row(fixture, cx, "pinned-", id, viewport, 8.),
            title: fixture.node(cx, &title_id("pinned-", id)).bounds(),
            slot: fixture.node(cx, &action_id("pinned-", "slot", id)).bounds(),
        }
    }

    fn assert_unchanged(self, fixture: &Fixture, cx: &mut TestAppContext, id: &str) {
        let current = Self::observe(fixture, cx, id);
        same_bounds(current.row, self.row);
        same_bounds(current.title, self.title);
        same_bounds(current.slot, self.slot);
    }
}

fn assert_selected_quad(fixture: &Fixture, cx: &mut TestAppContext, row: Bounds<Pixels>) {
    fixture.update(cx, |window, cx| {
        let color: Hsla = cx.global::<Theme>().colors.bg_active.into();
        let scale = window.scale_factor();
        assert!(
            window.painted_quads().iter().any(|quad| {
                let bounds = quad.bounds.map(|value| px(value.as_f32() / scale));
                let matches = (bounds.left() - row.left()).abs() <= px(0.5)
                    && (bounds.top() - row.top()).abs() <= px(0.5)
                    && (bounds.size.width - row.size.width).abs() <= px(0.5)
                    && (bounds.size.height - row.size.height).abs() <= px(0.5)
                    && quad.background.as_solid() == Some(color);
                if matches {
                    contained(
                        quad.content_mask
                            .bounds
                            .map(|value| px(value.as_f32() / scale)),
                        row,
                    );
                }
                matches
            }),
            "selected row has no fully visible active-background quad"
        );
    });
}

#[gpui_kit::test]
fn pinned_rows_keep_navigation_height_at_count_boundaries(cx: &mut TestAppContext) {
    for count in [0, 1, 5, 6, 7, 20] {
        let fixture = Fixture::new(cx, count, 304., false);
        if count == 0 {
            let mut visual = fixture.visual(cx);
            for selector in [
                "organization-pinned",
                "organization-pinned-label",
                "organization-pinned-scroll",
            ] {
                assert!(
                    visual.debug_bounds(selector).is_none(),
                    "empty pinned section"
                );
            }
        } else {
            near(
                fixture.viewport(cx).size.height,
                if count == 1 { 32. } else { 160. },
            );
            assert_page(&fixture, cx, &fixture.pinned, 0, count.min(5));
            let mut first = 0;
            while first + 5 < count {
                let next = (first + 4).min(count - 5);
                fixture.scroll(cx, -((next - first) as f32 * 32.));
                assert_page(&fixture, cx, &fixture.pinned, next, 5);
                first = next;
            }
        }
        fixture.assert_persistence(&ids(&fixture.pinned));
        fixture.close(cx);
    }
}

#[gpui_kit::test]
fn pinned_rows_scroll_internally_and_open_first_last_first(cx: &mut TestAppContext) {
    for (count, distance) in [(7, 64.), (20, 480.)] {
        let fixture = Fixture::new(cx, count, 304., false);
        let first = &fixture.pinned[0];
        let last = &fixture.pinned[count - 1];
        fixture.click(cx, &title_id("pinned-", &first.id));
        assert_opened(cx, &first.id);
        fixture.move_outside(cx);
        let initial_viewport = fixture.viewport(cx);
        let initial_outer_viewport = fixture.selector(cx, "sidebar-scroll");
        let outer_offset = fixture.outer_offset(cx);
        let mut visited: HashSet<_> = fixture.pinned[..5].iter().map(|t| t.id.clone()).collect();
        assert_page(&fixture, cx, &fixture.pinned, 0, 5);
        let mut first_visible = 0;
        let mut measured_scroll = px(0.);
        while first_visible + 5 < count {
            let next = (first_visible + 4).min(count - 5);
            let anchor_id = row_id("pinned-", &fixture.pinned[next].id);
            let before = fixture.node(cx, &anchor_id);
            assert!(before.visible());
            let step = (next - first_visible) as f32 * 32.;
            fixture.scroll(cx, -step);
            same_bounds(fixture.viewport(cx), initial_viewport);
            same_bounds(
                fixture.selector(cx, "sidebar-scroll"),
                initial_outer_viewport,
            );
            assert_eq!(fixture.outer_offset(cx), outer_offset);
            assert_page(&fixture, cx, &fixture.pinned, next, 5);
            let after = fixture.node(cx, &anchor_id);
            assert!(after.visible());
            let displacement = before.bounds().top() - after.bounds().top();
            near(displacement, step);
            measured_scroll += displacement;
            visited.extend(fixture.pinned[next..next + 5].iter().map(|t| t.id.clone()));
            first_visible = next;
        }
        near(measured_scroll, distance);
        assert_eq!(
            visited,
            ids(&fixture.pinned).into_iter().collect::<HashSet<_>>()
        );
        let final_row = fixture.node(cx, &row_id("pinned-", &last.id)).bounds();
        near(
            final_row.top() - initial_viewport.top(),
            (count - 1) as f32 * 32. - distance,
        );
        fixture.click(cx, &title_id("pinned-", &last.id));
        assert_opened(cx, &last.id);
        while first_visible > 0 {
            let next = first_visible.saturating_sub(4);
            let anchor_id = row_id("pinned-", &fixture.pinned[first_visible].id);
            let before = fixture.node(cx, &anchor_id);
            assert!(before.visible());
            let step = (first_visible - next) as f32 * 32.;
            fixture.scroll(cx, step);
            let after = fixture.node(cx, &anchor_id);
            assert!(after.visible());
            let displacement = after.bounds().top() - before.bounds().top();
            near(displacement, step);
            measured_scroll -= displacement;
            same_bounds(fixture.viewport(cx), initial_viewport);
            same_bounds(
                fixture.selector(cx, "sidebar-scroll"),
                initial_outer_viewport,
            );
            assert_eq!(fixture.outer_offset(cx), outer_offset);
            assert_page(&fixture, cx, &fixture.pinned, next, 5);
            first_visible = next;
        }
        near(measured_scroll, 0.);
        fixture.click(cx, &title_id("pinned-", &first.id));
        assert_opened(cx, &first.id);
        near(
            fixture
                .node(cx, &row_id("pinned-", &first.id))
                .bounds()
                .top()
                - fixture.viewport(cx).top(),
            0.,
        );
        fixture.assert_persistence(&ids(&fixture.pinned));
        fixture.close(cx);
    }
}

#[gpui_kit::test]
fn pinning_sixth_row_and_unpinning_restores_full_viewport(cx: &mut TestAppContext) {
    let fixture = Fixture::new(cx, 5, 304., false);
    let candidate = &fixture.recent_rows[0];
    let original_viewport = fixture.viewport(cx);
    let original_outer_offset = fixture.outer_offset(cx);
    assert_page(&fixture, cx, &fixture.pinned, 0, 5);
    fixture.assert_persistence(&ids(&fixture.pinned));
    {
        let store = Store::open(&fixture.database).expect("pin action connection");
        conversation::set_thread_pinned(&store, &candidate.id, true).expect("pin sixth task");
    }
    fixture.sidebar.update(cx, Sidebar::reload_sessions);
    fixture.settle(cx);
    let mut six = fixture.pinned.clone();
    six.push(fixture.persisted(&candidate.id));
    fixture.assert_persistence(&ids(&six));
    same_bounds(fixture.viewport(cx), original_viewport);
    assert_page(&fixture, cx, &six, 0, 5);
    let anchor_id = row_id("pinned-", &six[1].id);
    let before_scroll = fixture.node(cx, &anchor_id).bounds();
    fixture.scroll(cx, -32.);
    assert_page(&fixture, cx, &six, 1, 5);
    near(
        before_scroll.top() - fixture.node(cx, &anchor_id).bounds().top(),
        32.,
    );
    assert_eq!(fixture.outer_offset(cx), original_outer_offset);
    fixture.click(cx, &title_id("pinned-", &candidate.id));
    assert_opened(cx, &candidate.id);
    fixture.hover(cx, &title_id("pinned-", &candidate.id));
    fixture.click(cx, &action_id("pinned-", "pin", &candidate.id));
    assert!(!fixture.persisted(&candidate.id).pinned);
    assert_opened(cx, &candidate.id);
    fixture.move_outside(cx);
    same_bounds(fixture.viewport(cx), original_viewport);
    assert_eq!(fixture.outer_offset(cx), original_outer_offset);
    assert_page(&fixture, cx, &fixture.pinned, 0, 5);
    assert_projection(&fixture, cx, &candidate.id, "standalone-");
    fixture.scroll(cx, -32.);
    assert_page(&fixture, cx, &fixture.pinned, 0, 5);
    assert_eq!(fixture.outer_offset(cx), original_outer_offset);
    fixture.assert_persistence(&ids(&fixture.pinned));
    fixture.close(cx);
}

#[gpui_kit::test]
fn sidebar_rows_keep_geometry_across_states_and_widths(cx: &mut TestAppContext) {
    for (width, dark) in [(304., false), (240., true), (365., false)] {
        let fixture = Fixture::new(cx, 7, width, dark);
        fixture.scroll(cx, -64.);
        fixture.move_outside(cx);
        let target = &fixture.pinned[6];
        let viewport = fixture.viewport(cx);
        let geometry = RowGeometry::observe(&fixture, cx, &target.id);
        let row = geometry.row;
        let slot = geometry.slot;
        assert!(
            fixture
                .optional_node(cx, &action_id("pinned-", "running", &target.id))
                .is_none()
        );
        fixture.click(cx, &title_id("pinned-", &target.id));
        fixture.move_outside(cx);
        assert_opened(cx, &target.id);
        geometry.assert_unchanged(&fixture, cx, &target.id);
        assert_selected_quad(&fixture, cx, row);
        cx.update(|cx| set_thread_running(&target.id, true, cx));
        fixture.settle(cx);
        let running = fixture.node(cx, &action_id("pinned-", "running", &target.id));
        assert!(running.visible());
        assert_eq!(running.label(), Some("任务正在运行"));
        contained(row, running.bounds());
        contained(slot, running.bounds());
        geometry.assert_unchanged(&fixture, cx, &target.id);
        fixture.hover(cx, &title_id("pinned-", &target.id));
        assert!(
            fixture
                .optional_node(cx, &action_id("pinned-", "running", &target.id))
                .is_none()
        );
        for action in ["pin", "archive"] {
            let control = fixture.node(cx, &action_id("pinned-", action, &target.id));
            assert!(control.visible());
            contained(row, control.bounds());
            contained(slot, control.bounds());
            near(control.bounds().size.height, 24.);
        }
        geometry.assert_unchanged(&fixture, cx, &target.id);
        fixture.update(cx, |window, cx| {
            window.right_click(
                gpui_kit::SharedString::from(title_id("pinned-", &target.id)),
                cx,
            );
        });
        fixture.settle(cx);
        let trigger = fixture.node(cx, &action_id("pinned-", "actions", &target.id));
        assert!(trigger.visible());
        assert_eq!(trigger.focused(), Some(true));
        contained(row, trigger.bounds());
        contained(slot, trigger.bounds());
        geometry.assert_unchanged(&fixture, cx, &target.id);
        let menu = fixture.node(cx, &action_id("pinned-", "actions-menu", &target.id));
        assert!(menu.visible());
        let item_id = format!("pinned-thread-action-{}-3", target.id);
        let item = fixture.node(cx, &item_id);
        assert!(item.visible());
        contained(menu.bounds(), item.bounds());
        fixture.click(cx, &item_id);
        assert!(fixture.persisted(&target.id).unread);
        assert!(!fixture.persisted(&fixture.pinned[5].id).unread);
        assert_opened(cx, &target.id);
        cx.update(|cx| set_thread_running(&target.id, false, cx));
        fixture.update(cx, |window, cx| window.blur(cx));
        fixture.move_outside(cx);
        assert!(
            fixture
                .optional_node(cx, &action_id("pinned-", "running", &target.id))
                .is_none()
        );
        same_bounds(fixture.viewport(cx), viewport);
        geometry.assert_unchanged(&fixture, cx, &target.id);
        fixture.hover(cx, &title_id("pinned-", &target.id));
        fixture.click(cx, &action_id("pinned-", "archive", &target.id));
        assert_eq!(fixture.persisted(&target.id).status, ThreadStatus::Archived);
        assert_eq!(
            fixture.persisted(&fixture.pinned[5].id).status,
            ThreadStatus::Active
        );
        let outer = fixture.selector(cx, "sidebar-scroll");
        fixture.visual(cx).simulate_event(ScrollWheelEvent {
            position: point(outer.right() - px(2.), outer.bottom() - px(2.)),
            delta: ScrollDelta::Pixels(point(px(0.), px(-160.))),
            ..Default::default()
        });
        fixture.settle(cx);
        let outer = fixture.selector(cx, "sidebar-scroll");
        for (prefix, threads, inset) in [
            ("project-", &fixture.project_rows, 24.),
            ("standalone-", &fixture.recent_rows, 0.),
        ] {
            let mut previous = None;
            for thread in threads {
                let bounds = assert_row(&fixture, cx, prefix, &thread.id, outer, inset);
                if let Some(previous) = previous {
                    near(bounds.top() - previous, 32.);
                }
                previous = Some(bounds.top());
            }
        }
        let pinned = fixture.selector(cx, "organization-pinned");
        let projects = fixture.selector(cx, "organization-projects");
        let recents = fixture.selector(cx, "organization-recents");
        near(projects.top() - pinned.bottom(), 12.);
        near(recents.top() - projects.bottom(), 12.);
        fixture.close(cx);
    }
}
