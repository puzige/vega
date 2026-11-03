use super::*;
use gpui_kit::{AppContext, TestAppContext};

fn reject_old_refresh(cx: &mut TestAppContext, success: bool) {
    let directory = tempfile::tempdir().unwrap();
    let old_path = directory.path().join("old.db");
    let store = Store::open(&old_path).unwrap();
    store.migrate().unwrap();
    let project = projects::create(store.conn(), "project", "project", None).unwrap();
    let thread = conversation::create_thread(&store, &project.id, "model", "confirm").unwrap();
    for collapsed in [true, false, true, false, true] {
        let revision = service::snapshot(&store).unwrap().revision;
        service::apply(
            &store,
            revision,
            SidebarOrganizationAction::SetCollapsed {
                target: SidebarCollapseTarget::Project(project.id.clone()),
                collapsed,
            },
        )
        .unwrap();
    }
    cx.update(|cx| {
        cx.set_global(VegaStore(Ok(store)));
        cx.set_global(SelectedProject(Some(project.id.clone())));
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
    let request = block.update(cx, |block, cx| {
        let route = ThreadsBlock::current_visit_route(cx);
        let org = block.organization.as_mut().unwrap();
        assert_eq!(org.snapshot.as_ref().unwrap().revision, committed.revision);
        assert!(!org.snapshot.as_ref().unwrap().projects.is_empty());
        org.generation = org.generation.wrapping_add(1);
        org.pending = true;
        OrganizationRequestToken {
            database: old_path,
            generation: org.generation,
            epoch: cx.global::<crate::navigation::TaskMutationState>().epoch,
            origin_route: route,
            submitted_editor: None,
            was_mutation: false,
        }
    });
    let new_path = directory.path().join("new.db");
    let replacement = Store::open(&new_path).unwrap();
    replacement.migrate().unwrap();
    let expected = service::snapshot(&replacement).unwrap();
    assert!(expected.revision < committed.revision);
    cx.update(|cx| cx.set_global(VegaStore(Ok(replacement))));
    block.update(cx, |block, cx| {
        block.error = Some("current failure".to_string().into());
        let result = if success {
            Ok(SidebarOrganizationOutcome {
                snapshot: committed,
                created_thread: None,
            })
        } else {
            Err("old database failure".to_string().into())
        };
        block.finish_organization_request(request, result, cx);
        assert!(block.organization.as_ref().unwrap().snapshot.is_none());
        assert!(block.threads.is_empty());
        assert!(block.archived.is_empty());
        assert_eq!(block.error.as_ref().unwrap().render(), "current failure");
    });
    cx.run_until_parked();
    cx.read(|cx| {
        let block = block.read(cx);
        let org = block.organization.as_ref().unwrap();
        assert_eq!(org.database.as_ref(), Some(&new_path));
        assert_eq!(org.snapshot.as_ref().unwrap(), &expected);
        assert!(block.threads.is_empty());
        assert!(block.archived.is_empty());
        assert_eq!(block.error.as_ref().unwrap().render(), "current failure");
    });
}

#[gpui_kit::test]
fn post_write_refresh_success_replaces_higher_revision_old_database_cache(cx: &mut TestAppContext) {
    reject_old_refresh(cx, true);
}

#[gpui_kit::test]
fn post_write_refresh_failure_replaces_higher_revision_old_database_cache(cx: &mut TestAppContext) {
    reject_old_refresh(cx, false);
}
