use super::*;
use gpui_kit::{AppContext, TestAppContext};
use std::{cell::RefCell, rc::Rc};

fn setup(cx: &mut TestAppContext) -> (tempfile::TempDir, Entity<ProjectsBlock>, Vec<Project>) {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(directory.path().join("sidebar.db")).unwrap();
    store.migrate().unwrap();
    let rows = ["first", "second", "third"]
        .into_iter()
        .map(|name| projects::create(store.conn(), name, name, None).unwrap())
        .collect::<Vec<_>>();
    store
        .conn()
        .execute("UPDATE projects SET last_opened_at = 0", [])
        .unwrap();
    cx.update(|cx| {
        cx.set_global(VegaStore(Ok(store)));
        cx.set_global(SelectedProject(None));
    });
    let block = cx.new(ProjectsBlock::new);
    (directory, block, rows)
}

fn recorded_visits(block: &Entity<ProjectsBlock>, cx: &mut TestAppContext) -> Rc<RefCell<usize>> {
    let recorded = Rc::new(RefCell::new(0));
    let counter = recorded.clone();
    cx.update(|cx| {
        cx.subscribe(block, move |_, event, _| {
            if matches!(event, ProjectsBlockEvent::VisitRecorded) {
                *counter.borrow_mut() += 1;
            }
        })
        .detach();
    });
    recorded
}

#[gpui_kit::test]
fn selection_returns_before_the_write_permit_is_available(cx: &mut TestAppContext) {
    let (directory, block, rows) = setup(cx);
    let blocker = Store::open_existing(directory.path().join("sidebar.db")).unwrap();
    let permit = vega_store::write_coordination::acquire(
        blocker.conn(),
        vega_store::write_coordination::WriteCategory::Messages,
    )
    .unwrap();
    let recorded = recorded_visits(&block, cx);
    block.update(cx, |block, cx| block.select_project(&rows[0].id, cx));
    cx.read(|cx| {
        assert_eq!(
            cx.global::<SelectedProject>().0.as_deref(),
            Some(rows[0].id.as_str())
        );
        assert!(block.read(cx).selection_pending);
        assert_eq!(
            projects::list(
                cx.global::<VegaStore>().0.as_ref().unwrap().conn(),
                ProjectSort::Name
            )
            .unwrap()[0]
                .last_opened_at,
            0
        );
    });
    drop(permit);
    cx.run_until_parked();
    assert_eq!(*recorded.borrow(), 1);
    cx.read(|cx| {
        assert!(!block.read(cx).selection_pending);
        assert!(block.read(cx).error.is_none());
        assert!(
            block
                .read(cx)
                .projects
                .iter()
                .find(|project| project.id == rows[0].id)
                .unwrap()
                .last_opened_at
                > 0
        );
    });
}

#[gpui_kit::test]
fn rapid_selections_keep_only_the_latest_pending_visit(cx: &mut TestAppContext) {
    let (_directory, block, rows) = setup(cx);
    let recorded = recorded_visits(&block, cx);
    block.update(cx, |block, cx| {
        for project in &rows {
            block.select_project(&project.id, cx);
        }
        assert_eq!(
            block
                .selection_requested
                .as_ref()
                .map(|(id, _)| id.as_str()),
            Some(rows[2].id.as_str())
        );
    });
    cx.run_until_parked();
    assert_eq!(*recorded.borrow(), 1);
    cx.read(|cx| {
        assert_eq!(
            cx.global::<SelectedProject>().0.as_deref(),
            Some(rows[2].id.as_str())
        );
        let projects = &block.read(cx).projects;
        assert_eq!(
            projects
                .iter()
                .find(|project| project.id == rows[1].id)
                .unwrap()
                .last_opened_at,
            0
        );
        assert!(
            projects
                .iter()
                .find(|project| project.id == rows[2].id)
                .unwrap()
                .last_opened_at
                > 0
        );
        assert!(!block.read(cx).selection_pending);
    });
}

#[gpui_kit::test]
fn failed_visit_keeps_selection_and_can_be_retried(cx: &mut TestAppContext) {
    let (directory, block, rows) = setup(cx);
    let path = directory.path().join("sidebar.db");
    let moved = directory.path().join("moved.db");
    std::fs::rename(&path, &moved).unwrap();
    let recorded = recorded_visits(&block, cx);
    block.update(cx, |block, cx| block.select_project(&rows[0].id, cx));
    cx.run_until_parked();
    assert!(!path.exists());
    assert_eq!(*recorded.borrow(), 0);
    cx.read(|cx| {
        assert_eq!(
            cx.global::<SelectedProject>().0.as_deref(),
            Some(rows[0].id.as_str())
        );
        assert!(block.read(cx).error.is_some());
    });
    std::fs::rename(moved, path).unwrap();
    block.update(cx, |block, cx| block.select_project(&rows[0].id, cx));
    cx.run_until_parked();
    assert_eq!(*recorded.borrow(), 1);
    cx.read(|cx| assert!(block.read(cx).error.is_none()));
}

#[gpui_kit::test]
fn removed_project_does_not_receive_a_late_visit_completion(cx: &mut TestAppContext) {
    let (_directory, block, rows) = setup(cx);
    let recorded = recorded_visits(&block, cx);
    block.update(cx, |block, cx| {
        block.select_project(&rows[0].id, cx);
        block.projects.retain(|project| project.id != rows[0].id);
        cx.set_global(SelectedProject(None));
    });
    cx.run_until_parked();
    assert_eq!(*recorded.borrow(), 0);
    cx.read(|cx| {
        assert!(cx.global::<SelectedProject>().0.is_none());
        assert!(
            !block
                .read(cx)
                .projects
                .iter()
                .any(|project| project.id == rows[0].id)
        );
        assert!(block.read(cx).error.is_none());
    });
}

#[gpui_kit::test]
fn changed_database_does_not_accept_an_old_visit_completion(cx: &mut TestAppContext) {
    let (directory, block, rows) = setup(cx);
    let recorded = recorded_visits(&block, cx);
    block.update(cx, |block, cx| block.select_project(&rows[0].id, cx));
    let replacement = Store::open(directory.path().join("replacement.db")).unwrap();
    replacement.migrate().unwrap();
    cx.update(|cx| cx.set_global(VegaStore(Ok(replacement))));
    cx.run_until_parked();
    assert_eq!(*recorded.borrow(), 0);
    cx.read(|cx| assert!(!block.read(cx).selection_pending));
}
