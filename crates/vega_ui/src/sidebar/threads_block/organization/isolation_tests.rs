use super::*;
use gpui_kit::{AppContext, TestAppContext};

#[gpui_kit::test]
fn post_write_organization_refresh_rejects_old_database_snapshot_and_error(
    cx: &mut TestAppContext,
) {
    let directory = tempfile::tempdir().unwrap();
    let old_path = directory.path().join("old.db");
    let store = Store::open(&old_path).unwrap();
    store.migrate().unwrap();
    let project = projects::create(store.conn(), "project", "project", None).unwrap();
    let thread = conversation::create_thread(&store, &project.id, "model", "confirm").unwrap();
    cx.update(|cx| {
        cx.set_global(VegaStore(Ok(store)));
        cx.set_global(SelectedProject(Some(project.id)));
        cx.set_global(OpenedThread(None));
        cx.set_global(SettingsOpen(false));
    });
    let projects = cx.new(ProjectsBlock::new);
    let block = cx.new(ThreadsBlock::new);
    block.update(cx, |block, cx| block.enable_organization(projects, cx));
    cx.run_until_parked();
    block.update(cx, |block, cx| block.toggle_pin(&thread.id, false, cx));
    cx.run_until_parked();
    let committed =
        cx.read(|cx| service::snapshot(cx.global::<VegaStore>().0.as_ref().unwrap()).unwrap());
    assert!(committed.threads[0].pinned);
    let replacement = Store::open(directory.path().join("new.db")).unwrap();
    replacement.migrate().unwrap();
    cx.update(|cx| cx.set_global(VegaStore(Ok(replacement))));
    for success in [true, false] {
        block.update(cx, |block, cx| {
            let route = ThreadsBlock::current_visit_route(cx);
            let org = block.organization.as_mut().unwrap();
            org.generation = org.generation.wrapping_add(1);
            org.pending = true;
            org.snapshot = None;
            let request = OrganizationRequestToken {
                database: old_path.clone(),
                generation: org.generation,
                epoch: cx.global::<crate::navigation::TaskMutationState>().epoch,
                origin_route: route,
                submitted_editor: None,
                was_mutation: false,
            };
            block.error = Some("current failure".to_string().into());
            let result = if success {
                Ok(SidebarOrganizationOutcome {
                    snapshot: committed.clone(),
                    created_thread: None,
                })
            } else {
                Err("old database failure".to_string().into())
            };
            block.finish_organization_request(request, result, cx);
            assert!(block.organization.as_ref().unwrap().snapshot.is_none());
            assert_eq!(block.error.as_ref().unwrap().render(), "current failure");
        });
        cx.run_until_parked();
        cx.read(|cx| {
            let org = block.read(cx).organization.as_ref().unwrap();
            assert!(org.snapshot.as_ref().unwrap().threads.is_empty());
            assert_eq!(
                block.read(cx).error.as_ref().unwrap().render(),
                "current failure"
            );
        });
    }
}
