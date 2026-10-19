use super::*;
use crate::settings::SettingsOpen;
use gpui_kit::component::Root;
use gpui_kit::test::{ElementSnapshot, TestWindowExt};
use gpui_kit::{
    AppContext, BorrowAppContext, Bounds, Pixels, ScrollDelta, ScrollWheelEvent, SharedString,
    TestAppContext, VisualTestContext, WindowHandle, size,
};
use std::collections::{HashMap, HashSet};
use tempfile::TempDir;
use vega_conversation::sidebar_organization as organization;
use vega_conversation::types::SidebarOrganizationAction;
use vega_store::projects;
use vega_theme::Theme;

#[derive(Clone, Copy)]
struct RowObservation {
    full: Bounds<Pixels>,
    clip: Bounds<Pixels>,
}

#[derive(Default)]
struct Rows(HashMap<String, RowObservation>);

impl Global for Rows {}

pub(super) fn record_row(id: &str, full: Bounds<Pixels>, window: &Window, cx: &mut App) {
    let clip = window.content_mask().bounds;
    cx.update_global::<Rows, _>(|rows, _| {
        rows.0.insert(id.to_owned(), RowObservation { full, clip });
    });
}

struct Fixture {
    _directory: TempDir,
    window: WindowHandle<Root>,
    sidebar: Entity<Sidebar>,
    tasks: Vec<Thread>,
    project_task: Thread,
}

impl Fixture {
    fn new(cx: &mut TestAppContext, total: usize, pinned: usize, height: f32) -> Self {
        let directory = tempfile::tempdir().expect("temporary Sidebar store");
        let project_path = directory.path().join("project");
        std::fs::create_dir(&project_path).expect("isolated project");
        let store = Store::open(&directory.path().join("sidebar.db")).expect("file Store");
        store.migrate().expect("Store migrations");
        let project = projects::create(
            store.conn(),
            project_path.to_str().expect("project path"),
            "Project below pinned tasks",
            None,
        )
        .expect("project");
        let tasks: Vec<_> = (0..total)
            .map(|index| {
                let task = conversation::create_standalone_thread(&store, "model", "confirm")
                    .expect("standalone task");
                conversation::rename_thread(&store, &task.id, &format!("Pinned {index}"))
                    .expect("task title");
                conversation::set_thread_pinned(&store, &task.id, index < pinned)
                    .expect("initial pinned state");
                conversation::get_thread(&store, &task.id).expect("stored task")
            })
            .collect();
        let project_task = conversation::create_thread(&store, &project.id, "model", "confirm")
            .expect("project task");
        let mut snapshot = organization::snapshot(&store).expect("organization");
        for task in &tasks {
            snapshot = organization::apply(
                &store,
                snapshot.revision,
                SidebarOrganizationAction::ReorderThread {
                    thread_id: task.id.clone(),
                    before_id: None,
                },
            )
            .expect("stable task order")
            .snapshot;
        }
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::init(cx);
            cx.set_reduce_motion(true);
            cx.set_global(Theme::light());
            cx.set_global(Rows::default());
            cx.set_global(SidebarWidth(304.));
            cx.set_global(SidebarCollapsed(false));
            cx.set_global(ProjectsCollapsed(false));
            cx.set_global(SessionsCollapsed(false));
            cx.set_global(SelectedProject(None));
            cx.set_global(OpenedThread(None));
            cx.set_global(PendingDeleteConfirm(None));
            cx.set_global(SettingsOpen(false));
            cx.set_global(NewTaskDefaults::default());
            cx.set_global(RunningThreadsGlobal(Default::default()));
            cx.set_global(VegaStore(Ok(store)));
        });
        let mut sidebar = None;
        let window = cx.open_window(size(px(960.), px(height)), |window, cx| {
            let view = cx.new(Sidebar::new);
            view.read(cx)
                .projects_block
                .clone()
                .update(cx, |block, _| block.disable_branch_probe());
            sidebar = Some(view.clone());
            Root::new(view, window, cx)
        });
        let fixture = Self {
            _directory: directory,
            window,
            sidebar: sidebar.expect("real Sidebar"),
            tasks,
            project_task,
        };
        fixture.settle(cx);
        fixture
    }

    fn update<R>(
        &self,
        cx: &mut TestAppContext,
        action: impl FnOnce(&mut Window, &mut App) -> R,
    ) -> R {
        cx.update_window(self.window.into(), |_, window, cx| action(window, cx))
            .expect("Sidebar window")
    }

    fn settle(&self, cx: &mut TestAppContext) {
        for _ in 0..2 {
            cx.run_until_parked();
            self.update(cx, |window, cx| {
                cx.update_global::<Rows, _>(|rows, _| rows.0.clear());
                window.render_frame(cx);
            });
        }
        cx.read(|cx| {
            let sidebar = self.sidebar.read(cx);
            assert!(sidebar.new_task_error.is_none());
            assert!(sidebar.projects_block.read(cx).error.is_none());
            assert!(sidebar.sessions_block.read(cx).error.is_none());
        });
    }

    fn node(&self, cx: &mut TestAppContext, id: &str) -> Option<ElementSnapshot> {
        self.update(cx, |window, _| {
            window.try_find(SharedString::from(id.to_owned()))
        })
    }

    fn bounds(&self, cx: &mut TestAppContext, id: &'static str) -> Bounds<Pixels> {
        if id == "organization-pinned-scroll" {
            return VisualTestContext::from_window(self.window.into(), cx)
                .debug_bounds(id)
                .unwrap_or_else(|| panic!("missing observed element {id}"));
        }
        self.node(cx, id)
            .unwrap_or_else(|| panic!("missing observed element {id}"))
            .bounds()
    }

    fn row(&self, cx: &TestAppContext, task: &Thread) -> Option<RowObservation> {
        cx.read(|cx| cx.global::<Rows>().0.get(&task.id).copied())
    }

    fn outer_bounds(&self, cx: &TestAppContext) -> Bounds<Pixels> {
        cx.read(|cx| self.sidebar.read(cx).sidebar_scroll.bounds())
    }

    fn outer_offset(&self, cx: &TestAppContext) -> Pixels {
        cx.read(|cx| self.sidebar.read(cx).sidebar_scroll.offset().y)
    }

    fn scroll(&self, cx: &mut TestAppContext, position: gpui_kit::Point<Pixels>, delta: f32) {
        VisualTestContext::from_window(self.window.into(), cx).simulate_event(ScrollWheelEvent {
            position,
            delta: ScrollDelta::Pixels(point(px(0.), px(delta))),
            ..Default::default()
        });
        self.settle(cx);
    }

    fn pin_count(&self, cx: &mut TestAppContext, count: usize) {
        cx.update(|cx| {
            let store = cx.global::<VegaStore>().0.as_ref().expect("mounted Store");
            for (index, task) in self.tasks.iter().enumerate() {
                conversation::set_thread_pinned(store, &task.id, index < count)
                    .expect("change pinned state");
            }
            self.sidebar
                .read(cx)
                .sessions_block
                .clone()
                .update(cx, ThreadsBlock::reload);
        });
        self.settle(cx);
    }

    fn assert_geometry(&self, cx: &mut TestAppContext, count: usize) -> Option<Pixels> {
        let projects = self.bounds(cx, "organization-projects");
        if count == 0 {
            assert!(self.node(cx, "organization-pinned").is_none());
            assert!(
                VisualTestContext::from_window(self.window.into(), cx)
                    .debug_bounds("organization-pinned-scroll")
                    .is_none()
            );
            let task = self
                .row(cx, &self.project_task)
                .expect("project task without pins");
            assert!(fully_visible(self.outer_bounds(cx), &task));
            near(task.full.size.height, 32.);
            return None;
        }
        let content = self.bounds(cx, "organization-pinned-scroll");
        near(content.size.height, 32. * count as f32);
        assert!(projects.top() >= content.bottom() - px(0.5));
        for (index, task) in self.tasks.iter().take(count).enumerate() {
            let row = self.row(cx, task).expect("visible pinned task");
            assert!(fully_visible(self.outer_bounds(cx), &row));
            near(row.full.size.height, 32.);
            near(row.full.top() - content.top(), 32. * index as f32);
        }
        Some(projects.top() - content.top())
    }

    fn click_task(&self, cx: &mut TestAppContext, task: &Thread) {
        let row = self.row(cx, task).expect("clickable task");
        assert!(fully_visible(self.outer_bounds(cx), &row));
        VisualTestContext::from_window(self.window.into(), cx)
            .simulate_click(row.full.center(), Default::default());
        self.settle(cx);
        cx.read(|cx| {
            assert_eq!(
                cx.global::<OpenedThread>()
                    .0
                    .as_ref()
                    .expect("opened task")
                    .id,
                task.id
            );
        });
    }
}

fn near(actual: Pixels, expected: f32) {
    assert!(
        (f32::from(actual) - expected).abs() <= 0.5,
        "{actual:?} != {expected}px"
    );
}

fn fully_visible(viewport: Bounds<Pixels>, row: &RowObservation) -> bool {
    let bounds = row.full;
    bounds.top() >= viewport.top()
        && bounds.bottom() <= viewport.bottom()
        && bounds.top() >= row.clip.top()
        && bounds.bottom() <= row.clip.bottom()
        && bounds.size.width > px(0.)
}

#[gpui_kit::test]
fn pinned_content_grows_and_moves_projects_on_the_same_mount(cx: &mut TestAppContext) {
    let fixture = Fixture::new(cx, 51, 0, 3000.);
    let _ = fixture.assert_geometry(cx, 0);
    let mut five = None;
    for count in [1, 5, 6, 5, 7, 20, 51] {
        fixture.pin_count(cx, count);
        let relative = fixture.assert_geometry(cx, count).expect("pinned content");
        match count {
            5 => {
                if let Some(previous) = five {
                    near(relative - previous, 0.);
                } else {
                    five = Some(relative);
                }
            }
            6 => near(relative - five.expect("five rows measured"), 32.),
            _ => {}
        }
    }
}

#[gpui_kit::test]
fn wheel_over_pinned_content_does_not_create_an_inner_offset(cx: &mut TestAppContext) {
    let fixture = Fixture::new(cx, 7, 7, 1000.);
    near(
        cx.read(|cx| fixture.sidebar.read(cx).sidebar_scroll.max_offset().y),
        0.,
    );
    let _ = fixture.assert_geometry(cx, 7);
    let content = fixture.bounds(cx, "organization-pinned-scroll");
    let projects = fixture.bounds(cx, "organization-projects");
    let offset = fixture.outer_offset(cx);
    for delta in [-96., 96.] {
        fixture.scroll(cx, content.center(), delta);
        let _ = fixture.assert_geometry(cx, 7);
        near(
            fixture.bounds(cx, "organization-projects").top() - projects.top(),
            0.,
        );
        near(fixture.outer_offset(cx) - offset, 0.);
    }
}

fn assert_outer_scroll_reaches_tasks(cx: &mut TestAppContext, count: usize) {
    let fixture = Fixture::new(cx, count, count, 600.);
    let first = &fixture.tasks[0];
    fixture.click_task(cx, first);
    let viewport = fixture.outer_bounds(cx);
    let initial = fixture.outer_offset(cx);
    let mut seen = HashSet::new();
    let mut clicked_last = false;
    let mut clicked_project = false;
    for _ in 0..64 {
        let content = fixture.bounds(cx, "organization-pinned-scroll");
        near(content.size.height, count as f32 * 32.);
        for (index, task) in fixture.tasks.iter().enumerate() {
            if let Some(row) = fixture.row(cx, task)
                && fully_visible(viewport, &row)
            {
                near(row.full.size.height, 32.);
                near(row.full.top() - content.top(), index as f32 * 32.);
                seen.insert(task.id.clone());
                if index == count - 1 && !clicked_last {
                    fixture.click_task(cx, task);
                    clicked_last = true;
                }
            }
        }
        if let Some(row) = fixture.row(cx, &fixture.project_task)
            && fully_visible(viewport, &row)
        {
            fixture.click_task(cx, &fixture.project_task);
            clicked_project = true;
        }
        if seen.len() == count && clicked_last && clicked_project {
            break;
        }
        fixture.scroll(
            cx,
            point(viewport.center().x, viewport.top() + px(48.)),
            -96.,
        );
    }
    assert_eq!(seen.len(), count, "every pinned task must be reachable");
    assert!(
        clicked_last && clicked_project,
        "last pinned and project task must be clickable"
    );
    assert!(
        fixture.outer_offset(cx) < initial - px(32.),
        "outer Sidebar must consume wheel events"
    );
}

#[gpui_kit::test]
fn outer_sidebar_scroll_reaches_twenty_pinned_tasks_and_project(cx: &mut TestAppContext) {
    assert_outer_scroll_reaches_tasks(cx, 20);
}

#[gpui_kit::test]
fn outer_sidebar_scroll_reaches_fifty_one_pinned_tasks_and_project(cx: &mut TestAppContext) {
    assert_outer_scroll_reaches_tasks(cx, 51);
}
