use super::*;
use gpui_kit::{AppContext, TestAppContext, size};

struct ControlsHarness;

impl Render for ControlsHarness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

#[gpui_kit::test]
fn collapse_and_selection_have_independent_durable_effects(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("sidebar.db");
    let store = Store::open(&database).unwrap();
    store.migrate().unwrap();
    let project = projects::create(store.conn(), "project", "project", None).unwrap();
    store
        .conn()
        .execute("UPDATE projects SET last_opened_at = 0", [])
        .unwrap();
    cx.update(|cx| {
        cx.set_global(VegaStore(Ok(store)));
        cx.set_global(SelectedProject(None));
        cx.set_global(OpenedThread(None));
        cx.set_global(SettingsOpen(false));
    });
    let projects = cx.new(ProjectsBlock::new);
    let threads = cx.new(ThreadsBlock::new);
    threads.update(cx, |threads, cx| {
        threads.enable_organization(projects.clone(), cx)
    });
    let window = cx.open_window(size(px(400.), px(400.)), |_, _| ControlsHarness);
    cx.run_until_parked();
    for view in [SidebarView::Projects, SidebarView::Groups] {
        threads.update(cx, |threads, cx| {
            let mut preferences = threads
                .organization
                .as_ref()
                .unwrap()
                .snapshot
                .as_ref()
                .unwrap()
                .preferences
                .clone();
            preferences.view = view;
            threads.submit_organization(SidebarOrganizationAction::SetPreferences(preferences), cx);
        });
        cx.run_until_parked();
        let before = service::snapshot(&Store::open_existing(&database).unwrap()).unwrap();
        window
            .update(cx, |_, window, cx| {
                threads.update(cx, |threads, cx| {
                    threads.control_action(Control::ToggleProject(project.id.clone()), window, cx);
                });
            })
            .unwrap();
        cx.run_until_parked();
        let after = service::snapshot(&Store::open_existing(&database).unwrap()).unwrap();
        let target = SidebarCollapseTarget::Project(project.id.clone());
        assert_ne!(
            before.collapsed.contains(&target),
            after.collapsed.contains(&target)
        );
        assert_eq!(
            before.projects[0].last_opened_at,
            after.projects[0].last_opened_at
        );
        cx.read(|cx| assert!(cx.global::<SelectedProject>().0.is_none()));
        window
            .update(cx, |_, window, cx| {
                threads.update(cx, |threads, cx| {
                    threads.control_action(Control::Project(project.id.clone()), window, cx);
                });
            })
            .unwrap();
        cx.read(|cx| {
            assert_eq!(
                cx.global::<SelectedProject>().0.as_deref(),
                Some(project.id.as_str())
            )
        });
        cx.run_until_parked();
        let selected = service::snapshot(&Store::open_existing(&database).unwrap()).unwrap();
        assert_eq!(selected.collapsed, after.collapsed);
        assert!(selected.projects[0].last_opened_at > 0);
        cx.update(|cx| cx.set_global(SelectedProject(None)));
    }
}
