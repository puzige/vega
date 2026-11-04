use super::*;
use gpui_kit::{AppContext, TestAppContext};
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;
use vega_conversation::types::{SidebarGroup, SidebarPreferences, SidebarProjectView};
use vega_store::write_coordination::{self, WriteCategory};

fn project(id: &str) -> SidebarCollapseTarget {
    SidebarCollapseTarget::Project(id.into())
}

fn update(revision: u64, collapsed: Vec<SidebarCollapseTarget>) -> SidebarCollapseUpdate {
    SidebarCollapseUpdate {
        revision,
        collapsed,
    }
}

fn failure(message: &str) -> crate::localized_copy::LocalizedCopy {
    crate::localized_copy::LocalizedCopy::Text(message.into())
}

fn snapshot_fixture() -> SidebarOrganizationSnapshot {
    SidebarOrganizationSnapshot {
        revision: 0,
        projects: vec![SidebarProject {
            id: "project-a".into(),
            name: "A".into(),
            path: "/isolated/a".into(),
            git_default_branch: None,
            created_at: 1,
            last_opened_at: 2,
        }],
        threads: Vec::new(),
        groups: vec![SidebarGroup {
            id: "group-a".into(),
            name: "A".into(),
            color: SidebarGroupColor::Blue,
        }],
        memberships: Vec::new(),
        project_order: vec!["project-a".into()],
        preferences: SidebarPreferences::default(),
        collapsed: Vec::new(),
    }
}

#[test]
fn rapid_even_and_odd_toggles_keep_one_latest_intent() {
    for initially_collapsed in [false, true] {
        for count in [2, 3, 100, 101] {
            let mut state = CollapseState::default();
            state.accept(update(
                4,
                if initially_collapsed {
                    vec![project("project-a")]
                } else {
                    Vec::new()
                },
            ));
            for _ in 0..count {
                state.toggle(project("project-a"));
                assert_eq!(state.intents.len(), 1);
            }
            let expected = initially_collapsed ^ (count % 2 == 1);
            assert_eq!(state.effective().contains(&project("project-a")), expected);
            let batch = state.intents.clone();
            state.in_flight = true;
            state.finish(
                &batch,
                Ok(update(
                    5,
                    if expected {
                        vec![project("project-a")]
                    } else {
                        Vec::new()
                    },
                )),
            );
            assert!(state.intents.is_empty());
            assert!(!state.in_flight);
            assert_eq!(state.effective().contains(&project("project-a")), expected);
        }
    }
}

#[test]
fn interleaved_targets_coalesce_independently() {
    let mut state = CollapseState::default();
    for target in [
        project("project-a"),
        SidebarCollapseTarget::Pinned,
        project("project-a"),
        SidebarCollapseTarget::Ungrouped,
        SidebarCollapseTarget::Pinned,
        SidebarCollapseTarget::Ungrouped,
        SidebarCollapseTarget::Ungrouped,
    ] {
        state.toggle(target);
        assert!(state.intents.len() <= 3);
    }
    assert_eq!(state.intents.len(), 3);
    assert_eq!(state.effective(), vec![SidebarCollapseTarget::Ungrouped]);
    let batch = state.intents.clone();
    state.finish(
        &batch,
        Ok(update(1, vec![SidebarCollapseTarget::Ungrouped])),
    );
    assert_eq!(state.effective(), vec![SidebarCollapseTarget::Ungrouped]);
    assert!(state.intents.is_empty());
}

#[test]
fn older_success_cannot_consume_newer_same_value_intent() {
    let mut state = CollapseState::default();
    state.toggle(project("project-a"));
    let older = state.intents.clone();
    state.in_flight = true;
    state.toggle(project("project-a"));
    state.toggle(project("project-a"));
    state.toggle(SidebarCollapseTarget::Pinned);
    let newer = state.intents.clone();
    state.finish(&older, Ok(update(1, vec![project("project-a")])));
    assert_eq!(state.intents, newer);
    assert_eq!(
        state.effective(),
        vec![project("project-a"), SidebarCollapseTarget::Pinned]
    );
    assert!(!state.in_flight);
    state.finish(
        &newer,
        Ok(update(
            2,
            vec![project("project-a"), SidebarCollapseTarget::Pinned],
        )),
    );
    assert!(state.intents.is_empty());
    assert_eq!(
        state.effective(),
        vec![project("project-a"), SidebarCollapseTarget::Pinned]
    );
}

#[test]
fn older_success_keeps_newer_expand_intent_visible_until_its_own_save() {
    let mut state = CollapseState::default();
    state.toggle(project("project-a"));
    let older = state.intents.clone();
    state.toggle(project("project-a"));
    let newer = state.intents.clone();
    state.finish(&older, Ok(update(1, vec![project("project-a")])));
    assert_eq!(state.intents, newer);
    assert!(state.effective().is_empty());
    state.finish(&newer, Ok(update(2, Vec::new())));
    assert!(state.intents.is_empty());
    assert!(state.effective().is_empty());
}

#[test]
fn older_failure_preserves_newer_opposite_intent_and_unrelated_error() {
    let mut state = CollapseState::default();
    state
        .errors
        .push((SidebarCollapseTarget::Ungrouped, failure("unrelated")));
    state.toggle(project("project-a"));
    let older = state.intents.clone();
    state.in_flight = true;
    state.toggle(project("project-a"));
    let newer = state.intents.clone();
    state.finish(&older, Err(failure("older save")));
    assert_eq!(state.intents, newer);
    assert!(state.effective().is_empty());
    assert!(!state.in_flight);
    assert_eq!(state.errors.len(), 1);
    assert_eq!(state.errors[0].0, SidebarCollapseTarget::Ungrouped);
    assert_eq!(state.errors[0].1.render(), "unrelated");
}

#[test]
fn latest_failure_rolls_back_to_latest_persisted_and_retry_clears_only_its_error() {
    let mut state = CollapseState::default();
    state.accept(update(4, vec![project("project-a")]));
    state
        .errors
        .push((SidebarCollapseTarget::Ungrouped, failure("unrelated")));
    state.toggle(project("project-a"));
    let batch = state.intents.clone();
    state.in_flight = true;
    state.accept(update(
        5,
        vec![project("project-a"), SidebarCollapseTarget::Pinned],
    ));
    state.finish(&batch, Err(failure("retry this project")));
    assert_eq!(
        state.effective(),
        vec![project("project-a"), SidebarCollapseTarget::Pinned]
    );
    assert!(state.intents.is_empty());
    assert!(!state.in_flight);
    assert_eq!(state.errors.len(), 2);
    assert_eq!(state.errors[1].0, project("project-a"));
    assert_eq!(state.errors[1].1.render(), "retry this project");
    state.accept(update(
        6,
        vec![project("project-a"), SidebarCollapseTarget::Pinned],
    ));
    assert_eq!(state.errors.len(), 2);
    state.toggle(project("project-a"));
    assert_eq!(state.errors.len(), 2);
    let retry = state.intents.clone();
    state.finish(&retry, Ok(update(7, vec![SidebarCollapseTarget::Pinned])));
    assert_eq!(state.effective(), vec![SidebarCollapseTarget::Pinned]);
    assert!(state.intents.is_empty());
    assert_eq!(state.errors.len(), 1);
    assert_eq!(state.errors[0].0, SidebarCollapseTarget::Ungrouped);
    assert_eq!(state.errors[0].1.render(), "unrelated");
}

#[test]
fn stale_revision_cannot_replace_persisted_state_or_local_overlay() {
    let mut state = CollapseState::default();
    state.accept(update(9, vec![SidebarCollapseTarget::Pinned]));
    state.toggle(project("project-a"));
    state.accept(update(8, vec![SidebarCollapseTarget::Ungrouped]));
    assert_eq!(state.revision, 9);
    assert_eq!(state.persisted, vec![SidebarCollapseTarget::Pinned]);
    assert_eq!(
        state.effective(),
        vec![SidebarCollapseTarget::Pinned, project("project-a")]
    );
    let batch = state.intents.clone();
    state.finish(&batch, Ok(update(7, vec![project("project-a")])));
    assert_eq!(state.revision, 9);
    assert_eq!(state.effective(), vec![SidebarCollapseTarget::Pinned]);
}

#[test]
fn deleted_projects_and_groups_are_removed_from_all_local_collapse_layers() {
    let mut state = CollapseState::default();
    let deleted_project = project("project-deleted");
    let deleted_group = SidebarCollapseTarget::Group("group-deleted".into());
    let existing_group = SidebarCollapseTarget::Group("group-a".into());
    state.accept(update(
        3,
        vec![
            deleted_project.clone(),
            deleted_group.clone(),
            project("project-a"),
            existing_group.clone(),
            SidebarCollapseTarget::Pinned,
        ],
    ));
    for target in [
        deleted_project.clone(),
        deleted_group.clone(),
        project("project-a"),
        existing_group.clone(),
        SidebarCollapseTarget::Ungrouped,
    ] {
        state.toggle(target.clone());
        state.errors.push((target, failure("save failed")));
    }
    let older = state.intents.clone();
    state.retain_targets(&snapshot_fixture());
    assert_eq!(
        state.persisted,
        vec![
            project("project-a"),
            existing_group,
            SidebarCollapseTarget::Pinned
        ]
    );
    assert_eq!(state.intents.len(), 3);
    assert_eq!(state.errors.len(), 3);
    state.finish(&older, Err(failure("late failure")));
    assert!(!state.effective().contains(&deleted_project));
    assert!(!state.effective().contains(&deleted_group));
    assert!(
        state
            .errors
            .iter()
            .all(|(target, _)| target != &deleted_project && target != &deleted_group)
    );
}

struct DatabaseFixture {
    _directory: tempfile::TempDir,
    path: PathBuf,
}

impl DatabaseFixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("isolated temporary directory");
        let path = directory
            .path()
            .canonicalize()
            .expect("canonical temporary directory")
            .join("store.sqlite");
        let store = Store::open(&path).expect("create isolated database");
        store.migrate().expect("migrate isolated database");
        let tx = store.immediate_transaction().expect("seed transaction");
        tx.execute_batch("INSERT INTO projects(id,path,name,created_at,last_opened_at) VALUES ('project-a','/isolated/a','A',1,2),('project-b','/isolated/b','B',3,4); INSERT INTO threads(id,project_id,model,created_at,updated_at) VALUES ('thread-a','project-a','test-model',1,2),('thread-b','project-b','test-model',3,4);").expect("seed sidebar metadata");
        tx.commit().expect("commit sidebar metadata");
        drop(store);
        Self {
            _directory: directory,
            path,
        }
    }

    fn open(&self) -> Store {
        Store::open_existing(&self.path).expect("open isolated database")
    }

    fn snapshot(&self) -> SidebarOrganizationSnapshot {
        service::snapshot(&self.open()).expect("read actual service snapshot")
    }
}

struct WriterGate {
    release: Option<Sender<()>>,
    finished: Receiver<()>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl WriterGate {
    fn hold(database: &DatabaseFixture) -> Self {
        let path = database.path.clone();
        let (ready_tx, ready_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (finished_tx, finished_rx) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            let store = Store::open_existing(path).expect("open writer connection off UI thread");
            let permit = write_coordination::acquire(store.conn(), WriteCategory::Sidebar)
                .expect("hold shared writer permit off UI thread");
            ready_tx.send(()).expect("announce held writer permit");
            let released = release_rx.recv_timeout(Duration::from_secs(5));
            drop(permit);
            finished_tx
                .send(())
                .expect("announce released writer permit");
            assert!(
                released.is_ok(),
                "UI callback failed to return before writer gate deadline"
            );
        });
        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("writer permit acquired before callback");
        Self {
            release: Some(release_tx),
            finished: finished_rx,
            thread: Some(thread),
        }
    }

    fn release(mut self) {
        self.release
            .take()
            .expect("live writer gate")
            .send(())
            .expect("release writer thread");
        self.finished
            .recv_timeout(Duration::from_secs(5))
            .expect("writer permit released");
        self.thread
            .take()
            .expect("writer thread")
            .join()
            .expect("writer gate succeeded");
    }
}

impl Drop for WriterGate {
    fn drop(&mut self) {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn block_fixture(database: &DatabaseFixture, cx: &mut TestAppContext) -> Entity<ThreadsBlock> {
    let snapshot = database.snapshot();
    cx.set_global(SelectedProject(None));
    cx.set_global(OpenedThread(None));
    cx.set_global(SettingsOpen(false));
    cx.set_global(VegaStore(Ok(database.open())));
    cx.set_global(vega_theme::Theme::dark());
    let projects = cx.new(ProjectsBlock::new);
    cx.new(|cx| {
        let mut block = ThreadsBlock::new(cx);
        block.threads = snapshot.threads.clone();
        let mut collapse = CollapseState::default();
        collapse.accept(update(snapshot.revision, snapshot.collapsed.clone()));
        block.organization = Some(Organization {
            snapshot: Some(Rc::new(snapshot.clone())),
            collapse,
            task_projection: None,
            focus_dirty: false,
            database: Some(database.path.clone()),
            projects,
            pending: false,
            reorder: ReorderState::new(cx),
            refresh_queued: false,
            generation: 1,
            project_threads_visible: HashMap::new(),
            project_threads_total: HashMap::new(),
            project_growth_paused: HashSet::new(),
            editor: None,
            menu: None,
            menu_anchor: None,
            menu_focus: cx.focus_handle(),
            menu_index: 0,
            menu_scroll: gpui_kit::ScrollHandle::new(),
            archive: false,
            reveal_project: None,
        });
        block.prepare_task_projection(&snapshot);
        block
    })
}

fn pump_until(cx: &mut TestAppContext, mut done: impl FnMut(&App) -> bool) {
    for _ in 0..1000 {
        if cx.read(&mut done) {
            return;
        }
        assert!(
            cx.background_executor.tick(),
            "expected production completion has no runnable work"
        );
    }
    panic!("production completion exceeded bounded executor ticks");
}

fn await_saves(block: &Entity<ThreadsBlock>, cx: &mut TestAppContext) {
    pump_until(cx, |cx| {
        let org = block.read(cx).organization.as_ref().expect("organization");
        !org.collapse.in_flight && org.collapse.intents.is_empty() && !org.pending
    });
}

fn request_token(
    block: &ThreadsBlock,
    database: &DatabaseFixture,
    was_mutation: bool,
) -> OrganizationRequestToken {
    OrganizationRequestToken {
        database: database.path.clone(),
        generation: block
            .organization
            .as_ref()
            .expect("organization")
            .generation,
        epoch: 0,
        origin_route: (None, None, false),
        submitted_editor: None,
        was_mutation,
    }
}

#[gpui_kit::test]
fn real_toggle_is_immediate_under_writer_contention_and_persists_latest_clicks(
    cx: &mut TestAppContext,
) {
    let database = DatabaseFixture::new();
    let block = block_fixture(&database, cx);
    let gate = WriterGate::hold(&database);
    block.update(cx, |block, cx| {
        let projection = block.task_projection();
        block
            .organization
            .as_mut()
            .expect("organization")
            .focus_dirty = false;
        block.toggle_organization_collapse(project("project-a"), cx);
        let org = block.organization.as_ref().expect("organization");
        assert!(
            org.snapshot
                .as_ref()
                .expect("snapshot")
                .collapsed
                .contains(&project("project-a"))
        );
        assert!(org.collapse.in_flight);
        assert!(!org.pending);
        assert!(org.reorder.pointer_order.is_none());
        assert!(Rc::ptr_eq(&projection, &block.task_projection()));
        assert!(!org.focus_dirty);
        block.toggle_organization_collapse(project("project-a"), cx);
        block.toggle_organization_collapse(project("project-b"), cx);
        block.toggle_organization_collapse(project("project-b"), cx);
        block.toggle_organization_collapse(project("project-b"), cx);
        let org = block.organization.as_ref().expect("organization");
        assert_eq!(
            org.snapshot.as_ref().expect("snapshot").collapsed,
            vec![project("project-b")]
        );
        assert!(org.collapse.in_flight);
        assert_eq!(org.collapse.intents.len(), 2);
        assert!(org.reorder.pointer_order.is_none());
        assert!(Rc::ptr_eq(&projection, &block.task_projection()));
    });
    assert!(matches!(
        gate.finished.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    assert!(database.snapshot().collapsed.is_empty());
    gate.release();
    await_saves(&block, cx);
    assert_eq!(database.snapshot().collapsed, vec![project("project-b")]);
    block.read_with(cx, |block, _| {
        let org = block.organization.as_ref().expect("organization");
        assert_eq!(
            org.snapshot.as_ref().expect("snapshot").collapsed,
            vec![project("project-b")]
        );
        assert!(org.collapse.errors.is_empty());
        assert!(org.reorder.pointer_order.is_none());
    });
    drop(block);
    cx.run_until_parked();
}

#[gpui_kit::test]
fn actual_refresh_and_mutation_completions_overlay_clicks_while_pending(cx: &mut TestAppContext) {
    for was_mutation in [false, true] {
        let database = DatabaseFixture::new();
        let block = block_fixture(&database, cx);
        let gate = WriterGate::hold(&database);
        let outcome = SidebarOrganizationOutcome {
            snapshot: database.snapshot(),
            created_thread: None,
        };
        block.update(cx, |block, cx| {
            let projection = block.task_projection();
            let token = request_token(block, &database, was_mutation);
            let org = block.organization.as_mut().expect("organization");
            org.pending = true;
            org.collapse
                .errors
                .push((SidebarCollapseTarget::Ungrouped, failure("unrelated")));
            block.toggle_organization_collapse(project("project-a"), cx);
            let org = block.organization.as_ref().expect("organization");
            assert!(org.pending);
            assert!(!org.collapse.in_flight);
            assert_eq!(
                org.snapshot.as_ref().expect("snapshot").collapsed,
                vec![project("project-a")]
            );
            block.finish_organization_request(token, Ok(outcome), cx);
            let org = block.organization.as_ref().expect("organization");
            assert!(!org.pending);
            assert!(org.collapse.in_flight);
            assert_eq!(
                org.snapshot.as_ref().expect("snapshot").collapsed,
                vec![project("project-a")]
            );
            assert!(Rc::ptr_eq(&projection, &block.task_projection()));
            assert_eq!(org.collapse.errors.len(), 1);
            assert_eq!(org.collapse.errors[0].1.render(), "unrelated");
        });
        gate.release();
        await_saves(&block, cx);
        assert_eq!(database.snapshot().collapsed, vec![project("project-a")]);
        drop(block);
        cx.run_until_parked();
    }
}

#[gpui_kit::test]
fn old_database_collapse_callback_cannot_change_current_database_state(cx: &mut TestAppContext) {
    let previous = DatabaseFixture::new();
    let current = DatabaseFixture::new();
    let block = block_fixture(&previous, cx);
    let gate = WriterGate::hold(&previous);
    block.update(cx, |block, cx| {
        block.toggle_organization_collapse(project("project-a"), cx)
    });
    cx.set_global(VegaStore(Ok(current.open())));
    let previous_state = block.update(cx, |block, _| {
        let org = block.organization.as_mut().expect("organization");
        org.database = Some(current.path.clone());
        org.snapshot = Some(Rc::new(current.snapshot()));
        let previous_state = std::mem::take(&mut org.collapse);
        org.collapse.errors.push((
            SidebarCollapseTarget::Ungrouped,
            failure("current database error"),
        ));
        block.error = Some(failure("current route error"));
        previous_state
    });
    gate.release();
    cx.run_until_parked();
    assert_eq!(previous.snapshot().collapsed, vec![project("project-a")]);
    assert!(current.snapshot().collapsed.is_empty());
    drop(previous_state);
    block.read_with(cx, |block, _| {
        let org = block.organization.as_ref().expect("organization");
        assert_eq!(org.database.as_ref(), Some(&current.path));
        assert!(
            org.snapshot
                .as_ref()
                .expect("snapshot")
                .collapsed
                .is_empty()
        );
        assert!(org.collapse.effective().is_empty());
        assert_eq!(org.collapse.errors.len(), 1);
        assert_eq!(org.collapse.errors[0].1.render(), "current database error");
        assert_eq!(
            block.error.as_ref().expect("route error").render(),
            "current route error"
        );
    });
    drop(block);
    cx.run_until_parked();
}

#[gpui_kit::test]
fn old_generation_collapse_callback_cannot_acknowledge_new_generation_intents(
    cx: &mut TestAppContext,
) {
    let database = DatabaseFixture::new();
    let block = block_fixture(&database, cx);
    let gate = WriterGate::hold(&database);
    block.update(cx, |block, cx| {
        block.toggle_organization_collapse(project("project-a"), cx)
    });
    let previous_state = block.update(cx, |block, _| {
        let org = block.organization.as_mut().expect("organization");
        org.generation += 1;
        let previous_state = std::mem::take(&mut org.collapse);
        org.collapse
            .accept(update(9, vec![SidebarCollapseTarget::Ungrouped]));
        org.collapse.toggle(SidebarCollapseTarget::Pinned);
        org.collapse.errors.push((
            SidebarCollapseTarget::Pinned,
            failure("new generation error"),
        ));
        block.apply_local_collapse();
        previous_state
    });
    gate.release();
    cx.run_until_parked();
    assert_eq!(database.snapshot().collapsed, vec![project("project-a")]);
    drop(previous_state);
    block.read_with(cx, |block, _| {
        let org = block.organization.as_ref().expect("organization");
        assert_eq!(org.collapse.revision, 9);
        assert_eq!(org.collapse.intents.len(), 1);
        assert_eq!(
            org.collapse.effective(),
            vec![
                SidebarCollapseTarget::Ungrouped,
                SidebarCollapseTarget::Pinned
            ]
        );
        assert_eq!(
            org.snapshot.as_ref().expect("snapshot").collapsed,
            org.collapse.effective()
        );
        assert_eq!(org.collapse.errors.len(), 1);
        assert_eq!(org.collapse.errors[0].1.render(), "new generation error");
        assert!(!org.collapse.in_flight);
    });
    drop(block);
    cx.run_until_parked();
}

#[gpui_kit::test]
fn real_save_failure_rolls_back_and_retry_preserves_unrelated_errors(cx: &mut TestAppContext) {
    let database = DatabaseFixture::new();
    let store = database.open();
    store.conn().execute_batch("CREATE TRIGGER reject_collapse BEFORE UPDATE OF collapsed ON sidebar_organization BEGIN SELECT RAISE(ABORT,'injected collapse failure'); END;").expect("inject real persistence failure");
    let block = block_fixture(&database, cx);
    block.update(cx, |block, cx| {
        block
            .organization
            .as_mut()
            .expect("organization")
            .collapse
            .errors
            .push((
                SidebarCollapseTarget::Ungrouped,
                failure("unrelated collapse error"),
            ));
        block.error = Some(failure("unrelated route error"));
        block.toggle_organization_collapse(project("project-a"), cx);
        assert_eq!(
            block
                .organization
                .as_ref()
                .expect("organization")
                .snapshot
                .as_ref()
                .expect("snapshot")
                .collapsed,
            vec![project("project-a")]
        );
    });
    await_saves(&block, cx);
    assert!(database.snapshot().collapsed.is_empty());
    block.read_with(cx, |block, _| {
        let org = block.organization.as_ref().expect("organization");
        assert!(
            org.snapshot
                .as_ref()
                .expect("snapshot")
                .collapsed
                .is_empty()
        );
        assert_eq!(org.collapse.errors.len(), 2);
        assert!(
            org.collapse
                .errors
                .iter()
                .any(|(target, _)| target == &project("project-a"))
        );
        assert_eq!(
            block.error.as_ref().expect("route error").render(),
            "unrelated route error"
        );
    });
    store
        .conn()
        .execute_batch("DROP TRIGGER reject_collapse;")
        .expect("restore writable persistence");
    block.update(cx, |block, cx| {
        block.toggle_organization_collapse(project("project-a"), cx);
        assert_eq!(
            block
                .organization
                .as_ref()
                .expect("organization")
                .collapse
                .errors
                .len(),
            2
        );
    });
    await_saves(&block, cx);
    assert_eq!(database.snapshot().collapsed, vec![project("project-a")]);
    block.read_with(cx, |block, _| {
        let org = block.organization.as_ref().expect("organization");
        assert_eq!(
            org.snapshot.as_ref().expect("snapshot").collapsed,
            vec![project("project-a")]
        );
        assert_eq!(org.collapse.errors.len(), 1);
        assert_eq!(org.collapse.errors[0].0, SidebarCollapseTarget::Ungrouped);
        assert_eq!(
            org.collapse.errors[0].1.render(),
            "unrelated collapse error"
        );
        assert_eq!(
            block.error.as_ref().expect("route error").render(),
            "unrelated route error"
        );
    });
    drop(block);
    cx.run_until_parked();
}

#[gpui_kit::test]
fn collapse_completion_cannot_certify_stale_metadata_or_lose_external_preferences(
    cx: &mut TestAppContext,
) {
    let database = DatabaseFixture::new();
    let block = block_fixture(&database, cx);
    let mut preferences = database.snapshot().preferences;
    preferences.hide_project_threads_from_recents = true;
    let external = service::apply(
        &database.open(),
        0,
        SidebarOrganizationAction::SetPreferences(preferences),
    )
    .expect("external preference update");
    assert_eq!(external.snapshot.revision, 1);
    block.update(cx, |block, cx| {
        block.toggle_organization_collapse(project("project-a"), cx)
    });
    pump_until(cx, |cx| {
        !block
            .read(cx)
            .organization
            .as_ref()
            .expect("organization")
            .collapse
            .in_flight
    });
    block.read_with(cx, |block, _| {
        let org = block.organization.as_ref().expect("organization");
        let snapshot = org.snapshot.as_ref().expect("snapshot");
        assert_eq!(snapshot.revision, 0);
        assert!(!snapshot.preferences.hide_project_threads_from_recents);
        assert_eq!(snapshot.collapsed, vec![project("project-a")]);
        assert!(
            org.pending,
            "intervening metadata must be reconciled before generic writes"
        );
        assert!(matches!(
            service::apply(
                &database.open(),
                snapshot.revision,
                SidebarOrganizationAction::SetPreferences(snapshot.preferences.clone())
            ),
            Err(
                vega_conversation::types::SidebarOrganizationError::Conflict {
                    expected: 0,
                    actual: 2
                }
            )
        ));
    });
    await_saves(&block, cx);
    block.update(cx, |block, cx| {
        let snapshot = block
            .organization
            .as_ref()
            .expect("organization")
            .snapshot
            .as_ref()
            .expect("snapshot");
        assert_eq!(snapshot.revision, 2);
        assert!(snapshot.preferences.hide_project_threads_from_recents);
        let mut preferences = snapshot.preferences.clone();
        preferences.project_view = SidebarProjectView::Timeline;
        block.submit_organization(SidebarOrganizationAction::SetPreferences(preferences), cx);
    });
    await_saves(&block, cx);
    let persisted = database.snapshot();
    assert!(persisted.preferences.hide_project_threads_from_recents);
    assert_eq!(
        persisted.preferences.project_view,
        SidebarProjectView::Timeline
    );
    assert!(persisted.preferences.manual_thread_order.is_empty());
    assert_eq!(persisted.collapsed, vec![project("project-a")]);
    drop(block);
    cx.run_until_parked();
}

#[gpui_kit::test]
fn metadata_completion_invalidates_projection_but_collapse_does_not(cx: &mut TestAppContext) {
    let database = DatabaseFixture::new();
    let block = block_fixture(&database, cx);
    let mut snapshot = database.snapshot();
    for change in 0..4 {
        snapshot.revision += 1;
        match change {
            0 => snapshot.preferences.sort = SidebarTaskSort::Created,
            1 => snapshot.threads[0].pinned = true,
            2 => snapshot.threads[0].status = ThreadStatus::Archived,
            3 => snapshot.projects[0].name = "Renamed project".into(),
            _ => unreachable!(),
        }
        block.update(cx, |block, cx| {
            let previous = block.task_projection();
            let token = request_token(block, &database, false);
            block.organization.as_mut().expect("organization").pending = true;
            block.finish_organization_request(
                token,
                Ok(SidebarOrganizationOutcome {
                    snapshot: snapshot.clone(),
                    created_thread: None,
                }),
                cx,
            );
            let org = block.organization.as_ref().expect("organization");
            assert!(org.task_projection.is_none());
            assert!(org.focus_dirty);
            block.prepare_task_projection(&snapshot);
            assert!(!Rc::ptr_eq(&previous, &block.task_projection()));
            if change >= 2 {
                assert_eq!(block.archived.len(), 1);
                assert_eq!(block.archived[0].id, snapshot.threads[0].id);
                assert!(
                    block
                        .threads
                        .iter()
                        .all(|thread| thread.id != snapshot.threads[0].id)
                );
            }
        });
    }
    drop(block);
    cx.run_until_parked();
}
