use super::*;
use crate::sidebar::{Sidebar, SidebarExplicitlyShown, SidebarWidth};
use gpui_kit::{
    AppContext, Bounds, InteractiveElement, KeyBinding, Modifiers, Pixels, Point, Render,
    TestAppContext, VisualTestContext, size,
};
use std::cell::Cell;
use std::rc::Rc;
use tempfile::TempDir;
use vega_theme::Theme;

pub(super) fn observe_control<E: InteractiveElement>(control: E, id: impl Into<String>) -> E {
    let id = id.into();
    control.debug_selector(move || id.clone())
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) enum Projection {
    #[default]
    Sidebar,
    Projects,
    Groups,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct FixtureConfig {
    pub width: f32,
    pub height: f32,
    pub sidebar_width: f32,
    pub tasks_per_project: usize,
    pub groups: usize,
    pub dark: bool,
    pub projection: Projection,
}

impl Default for FixtureConfig {
    fn default() -> Self {
        Self {
            width: 1100.0,
            height: 900.0,
            sidebar_width: 280.0,
            tasks_per_project: 5,
            groups: 2,
            dark: false,
            projection: Projection::Sidebar,
        }
    }
}

pub(super) fn selector(id: impl Into<String>) -> &'static str {
    Box::leak(id.into().into_boxed_str())
}

pub(super) struct Fixture {
    pub visual: VisualTestContext,
    pub block: Entity<ThreadsBlock>,
    pub projects: Vec<String>,
    pub tasks: Vec<Vec<String>>,
    pub groups: Vec<String>,
    pub draft: Entity<TextInput>,
    pub outer_closes: Rc<Cell<usize>>,
    _directory: TempDir,
}

struct FixtureWindow {
    sidebar: Entity<Sidebar>,
    legacy: Option<Entity<LegacyProjection>>,
    draft: Entity<TextInput>,
    outside_focus: FocusHandle,
    outer_closes: Rc<Cell<usize>>,
}

impl FixtureWindow {
    fn close_fixture_route(&mut self, cx: &mut Context<Self>) {
        self.outer_closes.set(self.outer_closes.get() + 1);
        if cx.global::<PendingDeleteConfirm>().0.is_some() {
            cx.set_global(PendingDeleteConfirm(None));
        } else if cx.global::<SettingsOpen>().0 {
            cx.set_global(SettingsOpen(false));
        }
        cx.refresh_windows();
    }
}

impl Render for FixtureWindow {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let content = match &self.legacy {
            Some(legacy) => legacy.clone().into_any_element(),
            None => self.sidebar.clone().into_any_element(),
        };
        div()
            .id("menu-fixture")
            .key_context("VegaWindow")
            .size_full()
            .flex()
            .child(content)
            .child(
                div()
                    .id("fixture-outside")
                    .debug_selector(|| "fixture-outside".into())
                    .track_focus(&self.outside_focus)
                    .tab_stop(false)
                    .flex_1()
                    .h_full()
                    .flex()
                    .flex_col()
                    .justify_end()
                    .p_4()
                    .child(self.draft.clone()),
            )
    }
}

struct LegacyProjection {
    block: Entity<ThreadsBlock>,
    projection: Projection,
    scroll: gpui_kit::ScrollHandle,
}

impl Render for LegacyProjection {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let projection = self.projection;
        let scroll = self.scroll.clone();
        self.block.update(cx, |block, cx| {
            block.sync_contextual_focus(window, cx);
            block.menu_height =
                (window.viewport_size().height - px(16.0)).max(px(Typography::SIDEBAR_LINE_HEIGHT));
            let snapshot = block
                .organization
                .as_ref()
                .and_then(|org| org.snapshot.clone());
            let menu_open = block
                .organization
                .as_ref()
                .is_some_and(|org| org.menu.is_some());
            let body = match snapshot.as_ref() {
                Some(snapshot) => match projection {
                    Projection::Projects => block.render_project_projection(snapshot, cx),
                    Projection::Groups => block.render_group_projection(snapshot, cx),
                    Projection::Sidebar => unreachable!(),
                },
                None => div().into_any_element(),
            };
            div()
                .relative()
                .w(px(crate::sidebar::width(cx)))
                .h_full()
                .flex_shrink_0()
                .child(
                    div()
                        .id("legacy-projection-scroll")
                        .debug_selector(|| "legacy-projection-scroll".into())
                        .size_full()
                        .track_scroll(&scroll)
                        .overflow_y_scroll()
                        .child(body),
                )
                .children(menu_open.then(|| block.render_organization_menu(cx)))
                .into_any_element()
        })
    }
}

pub(super) fn seed_action(
    store: &Store,
    action: SidebarOrganizationAction,
) -> SidebarOrganizationSnapshot {
    let revision = service::snapshot(store)
        .expect("owned organization snapshot")
        .revision;
    service::apply(store, revision, action)
        .expect("owned organization seed")
        .snapshot
}

impl Fixture {
    pub async fn new(cx: &mut TestAppContext, config: FixtureConfig) -> Self {
        let directory = tempfile::tempdir().expect("owned fixture directory");
        let root = directory
            .path()
            .canonicalize()
            .expect("absolute fixture directory");
        let store = Store::open(root.join("sidebar.sqlite3")).expect("owned file database");
        store.migrate().expect("owned database migrations");
        let mut projects = Vec::new();
        let mut tasks = Vec::new();
        for index in 0..2 {
            let path = root.join(format!("project-{index}"));
            std::fs::create_dir(&path).expect("owned non-git project directory");
            let project = vega_store::projects::create(
                store.conn(),
                path.to_str().expect("UTF-8 owned project path"),
                &format!("Project {}", index + 1),
                None,
            )
            .expect("owned project seed");
            let mut project_tasks = Vec::new();
            for task in 0..config.tasks_per_project {
                let thread =
                    conversation::create_thread(&store, &project.id, "fixture", "readonly")
                        .expect("owned task seed");
                conversation::rename_thread(
                    &store,
                    &thread.id,
                    &format!("Project {} task {}", index + 1, task + 1),
                )
                .expect("owned task title");
                project_tasks.push(thread.id);
            }
            projects.push(project.id);
            tasks.push(project_tasks);
        }
        for project in &projects {
            seed_action(
                &store,
                SidebarOrganizationAction::MoveProject {
                    project_id: project.clone(),
                    before_id: None,
                },
            );
        }
        let mut groups = Vec::new();
        for index in 0..config.groups {
            let snapshot = seed_action(
                &store,
                SidebarOrganizationAction::CreateGroup {
                    name: format!("Group {}", index + 1),
                    color: SidebarGroupColor::Blue,
                },
            );
            groups.push(
                snapshot
                    .groups
                    .last()
                    .expect("created owned group")
                    .id
                    .clone(),
            );
        }
        if !groups.is_empty() {
            for (index, task) in tasks.iter().flatten().enumerate() {
                seed_action(
                    &store,
                    SidebarOrganizationAction::MoveThread {
                        thread_id: task.clone(),
                        group_id: Some(groups[index % groups.len()].clone()),
                        before_id: None,
                    },
                );
            }
        }
        let opened = tasks[0].first().map(|id| {
            conversation::list_threads(&store, &projects[0], None)
                .expect("owned initial project tasks")
                .into_iter()
                .find(|thread| &thread.id == id)
                .expect("owned initially opened task")
        });
        let outer_closes = Rc::new(Cell::new(0));
        let (sidebar, block, draft) = cx.update(|cx| {
            gpui_kit::component::init(cx);
            cx.set_global(if config.dark {
                Theme::dark()
            } else {
                Theme::light()
            });
            cx.set_global(VegaStore(Ok(store)));
            cx.set_global(SidebarCollapsed(false));
            cx.set_global(SidebarExplicitlyShown(true));
            cx.set_global(SidebarWidth(config.sidebar_width));
            cx.set_global(ProjectsCollapsed(false));
            cx.set_global(SessionsCollapsed(false));
            cx.set_global(SelectedProject(Some(projects[0].clone())));
            cx.set_global(OpenedThread(opened));
            cx.set_global(PendingDeleteConfirm(None));
            cx.set_global(SettingsOpen(true));
            cx.set_global(NewTaskDefaults::default());
            cx.set_global(ProjectWorkerActivity(Default::default()));
            cx.set_global(RunningThreadsGlobal(Default::default()));
            cx.set_global(crate::navigation::NavigationState::default());
            cx.set_global(crate::navigation::TaskMutationState::default());
            cx.bind_keys([
                KeyBinding::new("escape", CloseSettings, Some("VegaWindow")),
                KeyBinding::new("enter", ConfirmRename, Some("ThreadRename")),
                KeyBinding::new("cmd-a", crate::text_input::SelectAll, None),
                KeyBinding::new("enter", OpenThreadActions, Some("ThreadActionTrigger")),
                KeyBinding::new("space", OpenThreadActions, Some("ThreadActionTrigger")),
                KeyBinding::new("up", PreviousThreadAction, Some("ThreadActionsMenu")),
                KeyBinding::new("down", NextThreadAction, Some("ThreadActionsMenu")),
                KeyBinding::new("enter", ActivateThreadAction, Some("ThreadActionsMenu")),
                KeyBinding::new("space", ActivateThreadAction, Some("ThreadActionsMenu")),
                KeyBinding::new("escape", CloseThreadActions, Some("ThreadActionsMenu")),
            ]);
            let draft = cx.new(|cx| {
                let mut input = TextInput::new(cx, "", false);
                input.set_text("Unsent owned draft", cx);
                input
            });
            cx.set_global(crate::navigation::DraftNavigationGuard {
                task: tasks[0]
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "owned-draft".into()),
                input: draft.downgrade(),
                cached_tasks: 0,
                cached_bytes: 0,
            });
            let sidebar = cx.new(Sidebar::new);
            let block = sidebar.read(cx).sessions_block.clone();
            (sidebar, block, draft)
        });
        let window = cx.open_window(size(px(config.width), px(config.height)), |_, cx| {
            let legacy = match config.projection {
                Projection::Sidebar => None,
                projection => Some(cx.new(|cx| {
                    cx.observe(&block, |_, _, cx| cx.notify()).detach();
                    LegacyProjection {
                        block: block.clone(),
                        projection,
                        scroll: gpui_kit::ScrollHandle::new(),
                    }
                })),
            };
            FixtureWindow {
                sidebar: sidebar.clone(),
                legacy,
                draft: draft.clone(),
                outside_focus: cx.focus_handle(),
                outer_closes: outer_closes.clone(),
            }
        });
        cx.update(|cx| {
            let close = window.entity(cx).expect("created fixture root").downgrade();
            let window: gpui_kit::AnyWindowHandle = window.into();
            cx.on_action(move |_: &CloseSettings, cx| {
                let root = close.clone();
                cx.defer(move |cx| {
                    let _ = window.update(cx, |_, _, cx| {
                        let _ = root.update(cx, |root, cx| root.close_fixture_route(cx));
                    });
                });
            });
        });
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        visual
            .cx
            .condition(&block, |block, _| {
                assert!(block.error.is_none(), "fixture refresh: {:?}", block.error);
                block.organization.as_ref().is_some_and(|org| {
                    !org.pending && org.snapshot.as_ref().is_some_and(|s| s.projects.len() == 2)
                })
            })
            .await;
        let mut fixture = Self {
            visual,
            block,
            projects,
            tasks,
            groups,
            draft,
            outer_closes,
            _directory: directory,
        };
        fixture.settle();
        fixture
    }

    pub fn settle(&mut self) {
        for _ in 0..3 {
            self.visual.run_until_parked();
            self.visual.update(|window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
            });
        }
        self.visual.run_until_parked();
    }

    pub fn bounds(&mut self, id: impl Into<String>) -> Bounds<Pixels> {
        let id = selector(id);
        self.visual
            .debug_bounds(id)
            .unwrap_or_else(|| panic!("not painted: {id}"))
    }

    pub fn menu_bounds(&mut self) -> Bounds<Pixels> {
        self.bounds("organization-menu")
    }

    pub fn has_menu(&mut self) -> bool {
        self.visual.debug_bounds("organization-menu").is_some()
    }

    pub fn outside(&mut self) -> Point<Pixels> {
        let bounds = self.bounds("fixture-outside");
        point(bounds.right() - px(24.0), bounds.top() + px(40.0))
    }

    pub fn click(&mut self, id: impl Into<String>) {
        let bounds = self.bounds(id);
        self.visual
            .simulate_mouse_move(bounds.center(), None, Modifiers::none());
        self.visual
            .simulate_click(bounds.center(), Modifiers::none());
        self.settle();
    }

    pub fn key(&mut self, keys: &str) {
        for key in keys.split_whitespace() {
            let keystroke = gpui_kit::Keystroke::parse(key).expect("fixture keystroke");
            if keystroke.key == "tab" {
                self.visual.simulate_event(gpui_kit::KeyDownEvent {
                    keystroke: keystroke.clone(),
                    is_held: false,
                    prefer_character_input: false,
                });
            } else {
                self.visual.simulate_keystrokes(key);
            }
            self.visual
                .simulate_event(gpui_kit::KeyUpEvent { keystroke });
            self.settle();
        }
    }

    pub fn right_click(&mut self, position: Point<Pixels>) {
        self.visual
            .simulate_mouse_move(position, None, Modifiers::none());
        self.visual
            .simulate_mouse_down(position, MouseButton::Right, Modifiers::none());
        self.visual
            .simulate_mouse_up(position, MouseButton::Right, Modifiers::none());
        self.settle();
    }

    pub fn capture_focus(&mut self, id: impl Into<String>) -> FocusHandle {
        let previous = self.visual.update(|window, cx| window.focused(cx));
        let bounds = self.bounds(id);
        let outside = self.outside();
        self.visual
            .simulate_mouse_move(bounds.center(), None, Modifiers::none());
        self.visual
            .simulate_mouse_down(bounds.center(), MouseButton::Left, Modifiers::none());
        let focus = self.visual.update(|window, cx| {
            window
                .focused(cx)
                .expect("painted control receives mouse-down focus")
        });
        self.visual
            .simulate_mouse_up(outside, MouseButton::Left, Modifiers::none());
        self.visual
            .simulate_mouse_move(outside, None, Modifiers::none());
        self.visual.update(|window, cx| match &previous {
            Some(previous) => window.focus(previous, cx),
            None => window.blur(cx),
        });
        self.settle();
        focus
    }

    pub fn focus(&mut self, id: impl Into<String>) -> FocusHandle {
        let focus = self.capture_focus(id);
        self.visual.update(|window, cx| window.focus(&focus, cx));
        self.settle();
        focus
    }

    pub fn assert_button_anchor(&mut self, button: Bounds<Pixels>) {
        let menu = self.menu_bounds();
        assert!(
            (menu.right() - button.right()).abs() <= px(1.0),
            "menu {menu:?} must right-align with actual button {button:?}"
        );
        assert!(
            (menu.top() - button.bottom()).abs() <= px(1.0),
            "menu {menu:?} must start below actual button {button:?}"
        );
    }

    pub fn snapshot(&self) -> SidebarOrganizationSnapshot {
        self.visual.cx.read(|cx| {
            let store = cx.global::<VegaStore>().0.as_ref().expect("owned store");
            service::snapshot(store).expect("owned persisted snapshot")
        })
    }

    pub async fn wait_idle(&mut self) {
        self.visual.run_until_parked();
        self.visual
            .cx
            .condition(&self.block, |block, _| {
                assert!(
                    block.error.is_none(),
                    "organization request: {:?}",
                    block.error
                );
                block.organization.as_ref().is_some_and(|org| !org.pending)
            })
            .await;
        self.settle();
    }

    pub fn assert_focused(&mut self, expected: &FocusHandle) {
        self.visual.update(|window, cx| {
            assert_eq!(window.focused(cx).as_ref(), Some(expected));
        });
    }

    pub fn assert_highlight(&mut self, index: usize) {
        let item = self.bounds(format!("organization-menu-{index}"));
        self.visual.update(|window, cx| {
            let active: gpui_kit::Hsla = theme(cx).colors.bg_active.into();
            let painted = item.scale(window.scale_factor());
            assert!(
                window.painted_quads().iter().any(|quad| {
                    quad.bounds == painted && quad.background.as_solid() == Some(active)
                }),
                "menu item {index} must be visibly highlighted at {item:?}"
            );
        });
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct UnchangedState {
    pub organization: SidebarOrganizationSnapshot,
    pub selected_project: Option<String>,
    pub opened_thread: Option<Thread>,
    pub draft: String,
}

impl UnchangedState {
    pub fn read(fixture: &Fixture) -> Self {
        let organization = fixture.snapshot();
        fixture.visual.cx.read(|cx| Self {
            organization,
            selected_project: cx.global::<SelectedProject>().0.clone(),
            opened_thread: cx.global::<OpenedThread>().0.clone(),
            draft: fixture.draft.read(cx).text().to_string(),
        })
    }

    pub fn assert_unchanged(&self, fixture: &Fixture) {
        assert_eq!(*self, Self::read(fixture));
    }
}

#[gpui_kit::test]
async fn organization_menu_placement_project_buttons_follow_separated_rows_without_navigation(
    cx: &mut TestAppContext,
) {
    let mut fixture = Fixture::new(cx, FixtureConfig::default()).await;
    let before = UnchangedState::read(&fixture);
    let buttons: Vec<_> = fixture
        .projects
        .iter()
        .map(|id| format!("project-more-{id}"))
        .collect();
    let upper = fixture.bounds(buttons[0].clone());
    let lower = fixture.bounds(buttons[1].clone());
    assert!(
        lower.top() - upper.bottom() >= px(100.0),
        "owned task rows separate project buttons"
    );
    let mut opened_bounds = Vec::new();
    for button in buttons {
        let bounds = fixture.bounds(button.clone());
        fixture.click(button);
        fixture.assert_button_anchor(bounds);
        fixture.bounds("organization-menu-0");
        before.assert_unchanged(&fixture);
        opened_bounds.push(fixture.menu_bounds());
        fixture.key("escape");
        assert!(!fixture.has_menu());
    }
    assert!(opened_bounds[1].top() - opened_bounds[0].top() >= px(100.0));
    before.assert_unchanged(&fixture);
}

#[gpui_kit::test]
async fn organization_menu_placement_tab_and_shift_tab_reach_project_menu_from_project_row(
    cx: &mut TestAppContext,
) {
    let mut fixture = Fixture::new(cx, FixtureConfig::default()).await;
    let id = fixture.projects[0].clone();
    let add = fixture.capture_focus(format!("project-add-{id}"));
    let more = fixture.capture_focus(format!("project-more-{id}"));
    let row = fixture.focus(format!("project-header-{id}"));
    assert_ne!(row, add);
    assert_ne!(row, more);
    assert_ne!(add, more);
    let before = UnchangedState::read(&fixture);
    let outside = fixture.outside();
    fixture
        .visual
        .simulate_mouse_move(outside, None, Modifiers::none());
    fixture.key("tab");
    fixture.assert_focused(&add);
    fixture.key("tab");
    fixture.assert_focused(&more);
    fixture.key("shift-tab");
    fixture.assert_focused(&add);
    fixture.key("tab");
    fixture.assert_focused(&more);
    assert!(!fixture.has_menu());
    before.assert_unchanged(&fixture);
    fixture.key("enter");
    assert!(fixture.has_menu());
    before.assert_unchanged(&fixture);
    fixture.key("escape");
    assert_eq!(fixture.outer_closes.get(), 0);
}

#[gpui_kit::test]
async fn organization_menu_placement_keyboard_project_anchor_ignores_pointer(
    cx: &mut TestAppContext,
) {
    let mut fixture = Fixture::new(
        cx,
        FixtureConfig {
            height: 1400.0,
            ..FixtureConfig::default()
        },
    )
    .await;
    let before = UnchangedState::read(&fixture);
    for key in ["enter", "space"] {
        for project in fixture.projects.clone() {
            let button = format!("project-more-{project}");
            fixture.focus(button.clone());
            let bounds = fixture.bounds(button);
            let pointer = fixture.outside();
            fixture
                .visual
                .simulate_mouse_move(pointer, None, Modifiers::none());
            fixture.key(key);
            fixture.assert_button_anchor(bounds);
            assert!(!fixture.menu_bounds().contains(&pointer));
            before.assert_unchanged(&fixture);
            fixture.key("escape");
            assert!(!fixture.has_menu());
        }
    }
}

#[gpui_kit::test]
async fn organization_menu_placement_escape_consumes_window_action_without_closing_outer_route(
    cx: &mut TestAppContext,
) {
    let mut fixture = Fixture::new(cx, FixtureConfig::default()).await;
    let before = UnchangedState::read(&fixture);
    fixture.click(format!("project-more-{}", fixture.projects[0]));
    assert!(fixture.has_menu());
    fixture.key("escape");
    assert!(!fixture.has_menu());
    assert_eq!(fixture.outer_closes.get(), 0);
    assert!(fixture.visual.cx.read(|cx| cx.global::<SettingsOpen>().0));
    before.assert_unchanged(&fixture);
    fixture.focus("fixture-outside");
    fixture.key("escape");
    assert_eq!(
        fixture.outer_closes.get(),
        1,
        "Escape binding reaches application action handler outside the menu"
    );
    assert!(!fixture.visual.cx.read(|cx| cx.global::<SettingsOpen>().0));
    before.assert_unchanged(&fixture);
}

#[gpui_kit::test]
async fn organization_menu_placement_modified_tab_and_outside_focus_do_not_enter_root_traversal(
    cx: &mut TestAppContext,
) {
    let mut fixture = Fixture::new(cx, FixtureConfig::default()).await;
    let row = fixture.focus(format!("project-header-{}", fixture.projects[0]));
    let before = UnchangedState::read(&fixture);
    for key in [
        "ctrl-tab",
        "ctrl-shift-tab",
        "alt-tab",
        "alt-shift-tab",
        "cmd-tab",
        "cmd-shift-tab",
        "fn-tab",
    ] {
        fixture.key(key);
        fixture.assert_focused(&row);
        assert!(!fixture.has_menu());
        before.assert_unchanged(&fixture);
    }
    let draft = fixture.visual.update(|window, cx| {
        let focus = fixture.draft.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        focus
    });
    fixture.settle();
    for key in ["tab", "shift-tab"] {
        fixture.key(key);
        fixture.assert_focused(&draft);
        assert!(!fixture.has_menu());
        before.assert_unchanged(&fixture);
    }
}

#[gpui_kit::test]
async fn organization_menu_placement_inline_rename_keeps_tab_focus_text_and_escape_action(
    cx: &mut TestAppContext,
) {
    let mut fixture = Fixture::new(cx, FixtureConfig::default()).await;
    let before = UnchangedState::read(&fixture);
    let task = fixture.tasks[0][0].clone();
    let row = fixture.bounds(format!("project-thread-row-{task}"));
    let position = point(row.left() + px(48.0), row.center().y);
    fixture
        .visual
        .simulate_mouse_move(position, None, Modifiers::none());
    fixture.visual.simulate_event(gpui_kit::MouseDownEvent {
        position,
        button: MouseButton::Left,
        modifiers: Modifiers::none(),
        click_count: 2,
        first_mouse: false,
    });
    fixture.visual.simulate_event(MouseUpEvent {
        position,
        button: MouseButton::Left,
        modifiers: Modifiers::none(),
        click_count: 2,
    });
    fixture.settle();
    let input = fixture.visual.cx.read(|cx| {
        let session = fixture
            .block
            .read(cx)
            .editing
            .as_ref()
            .expect("real double click starts rename");
        assert_eq!(session.thread_id, task);
        session.input.clone()
    });
    let focus = fixture.visual.cx.read(|cx| input.read(cx).focus_handle(cx));
    fixture.assert_focused(&focus);
    fixture.key("cmd-a");
    fixture.visual.simulate_input("Edited task title");
    fixture.settle();
    for key in ["tab", "shift-tab"] {
        fixture.key(key);
        fixture.assert_focused(&focus);
        assert_eq!(
            fixture
                .visual
                .cx
                .read(|cx| input.read(cx).text().to_string()),
            "Edited task title"
        );
        before.assert_unchanged(&fixture);
        assert!(!fixture.has_menu());
    }
    fixture.key("escape");
    assert!(
        fixture
            .visual
            .cx
            .read(|cx| fixture.block.read(cx).editing.is_none())
    );
    assert_eq!(fixture.outer_closes.get(), 0);
    assert!(fixture.visual.cx.read(|cx| cx.global::<SettingsOpen>().0));
    before.assert_unchanged(&fixture);
}

#[gpui_kit::test]
async fn organization_menu_placement_pending_group_edit_blocks_root_traversal_without_saving(
    cx: &mut TestAppContext,
) {
    let mut fixture = Fixture::new(cx, FixtureConfig::default()).await;
    let before = UnchangedState::read(&fixture);
    let group = fixture.groups[0].clone();
    let original_title = before
        .organization
        .groups
        .iter()
        .find(|existing| existing.id == group)
        .expect("owned existing group")
        .name
        .clone();
    fixture.visual.update(|window, cx| {
        fixture.block.update(cx, |block, cx| {
            block.edit_group(Some(group.clone()), window, cx);
        });
    });
    let input = fixture.visual.cx.read(|cx| {
        let editor = fixture
            .block
            .read(cx)
            .organization
            .as_ref()
            .and_then(|org| org.editor.as_ref())
            .expect("production edit_group starts pending group edit");
        assert_eq!(editor.group_id.as_ref(), Some(&group));
        assert_eq!(editor.input.read(cx).text(), original_title);
        editor.input.clone()
    });
    let pending_title = "Unsaved group title";
    assert_ne!(pending_title, original_title);
    fixture.visual.update(|_, cx| {
        input.update(cx, |input, cx| input.set_text(pending_title, cx));
    });
    fixture.settle();
    let row = fixture.focus(format!("project-header-{}", fixture.projects[0]));
    for key in ["tab", "shift-tab"] {
        fixture.key(key);
        fixture.assert_focused(&row);
        assert!(!fixture.has_menu());
        before.assert_unchanged(&fixture);
        fixture.visual.cx.read(|cx| {
            let editor = fixture
                .block
                .read(cx)
                .organization
                .as_ref()
                .and_then(|org| org.editor.as_ref())
                .expect("root traversal must not save or cancel pending group edit");
            assert_eq!(editor.input, input);
            assert_eq!(editor.group_id.as_ref(), Some(&group));
            assert_eq!(editor.input.read(cx).text(), pending_title);
        });
    }
}

#[gpui_kit::test]
async fn organization_menu_placement_active_reorder_keeps_tab_focus_and_escape_cancels_without_route(
    cx: &mut TestAppContext,
) {
    let mut fixture = Fixture::new(cx, FixtureConfig::default()).await;
    let before = UnchangedState::read(&fixture);
    let row = fixture.bounds(format!("project-header-{}", fixture.projects[0]));
    let position = point(row.left() + px(64.0), row.center().y);
    fixture
        .visual
        .simulate_mouse_move(position, None, Modifiers::none());
    fixture
        .visual
        .simulate_mouse_down(position, MouseButton::Left, Modifiers::none());
    fixture.visual.simulate_mouse_move(
        point(position.x + px(12.0), position.y + px(4.0)),
        Some(MouseButton::Left),
        Modifiers::none(),
    );
    fixture.settle();
    assert!(
        fixture.visual.cx.read(|cx| cx.has_active_drag()),
        "real row drag starts reorder"
    );
    let drag_focus = fixture
        .visual
        .update(|window, cx| window.focused(cx).unwrap());
    for key in ["tab", "shift-tab"] {
        fixture.key(key);
        fixture.assert_focused(&drag_focus);
        assert!(fixture.visual.cx.read(|cx| cx.has_active_drag()));
        assert!(!fixture.has_menu());
        before.assert_unchanged(&fixture);
    }
    fixture.key("escape");
    assert!(!fixture.visual.cx.read(|cx| cx.has_active_drag()));
    assert_eq!(fixture.outer_closes.get(), 0);
    let outside = fixture.outside();
    fixture
        .visual
        .simulate_mouse_up(outside, MouseButton::Left, Modifiers::none());
    fixture.settle();
    before.assert_unchanged(&fixture);
    assert!(!fixture.has_menu());
}

#[gpui_kit::test]
async fn organization_menu_placement_tab_clears_pointer_projection_without_reordering(
    cx: &mut TestAppContext,
) {
    let mut fixture = Fixture::new(cx, FixtureConfig::default()).await;
    let before = UnchangedState::read(&fixture);
    let task = fixture.tasks[0][0].clone();
    let row = fixture.bounds(format!("project-thread-row-{task}"));
    let position = point(row.left() + px(48.0), row.center().y);
    let focus = fixture.visual.cx.read(|cx| {
        fixture
            .block
            .read(cx)
            .thread_action_focuses
            .get(&task)
            .unwrap()
            .clone()
    });
    for key in ["tab", "shift-tab"] {
        fixture.visual.update(|window, cx| focus.focus(window, cx));
        fixture.settle();
        fixture
            .visual
            .simulate_mouse_move(position, None, Modifiers::none());
        fixture
            .visual
            .simulate_mouse_down(position, MouseButton::Left, Modifiers::none());
        fixture.visual.cx.read(|cx| {
            assert!(
                fixture
                    .block
                    .read(cx)
                    .organization
                    .as_ref()
                    .unwrap()
                    .reorder
                    .pointer_order
                    .is_some()
            );
        });
        fixture.key(key);
        fixture.visual.cx.read(|cx| {
            assert!(
                fixture
                    .block
                    .read(cx)
                    .organization
                    .as_ref()
                    .unwrap()
                    .reorder
                    .pointer_order
                    .is_none()
            );
        });
        let outside = fixture.outside();
        fixture
            .visual
            .simulate_mouse_up(outside, MouseButton::Left, Modifiers::none());
        fixture.settle();
        before.assert_unchanged(&fixture);
    }
    assert!(!fixture.has_menu());
}
