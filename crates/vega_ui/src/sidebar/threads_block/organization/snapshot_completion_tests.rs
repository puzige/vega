use super::*;
use crate::navigation::{TaskMutationState, begin_task_mutation, finish_task_mutation};
use std::cell::RefCell;
use std::rc::Rc;
use tokio::sync::oneshot;

struct MountedSnapshotSidebar {
    sidebar: Entity<Sidebar>,
    draft: Entity<TextInput>,
}

impl Render for MountedSnapshotSidebar {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .child(self.sidebar.clone())
            .child(div().flex_1().child(self.draft.clone()))
    }
}

struct SnapshotFixture {
    dir: tempfile::TempDir,
    block: Entity<ThreadsBlock>,
    draft: Entity<TextInput>,
    window: gpui_kit::WindowHandle<MountedSnapshotSidebar>,
    opened: Thread,
}

fn fixture(cx: &mut gpui_kit::TestAppContext) -> SnapshotFixture {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("snapshot.db")).unwrap();
    store.migrate().unwrap();
    store
        .conn()
        .execute(
            "INSERT INTO projects(id,path,name,created_at,last_opened_at) VALUES(?1,?2,?1,0,0)",
            (
                "snapshot-project",
                dir.path().join("project").to_string_lossy().to_string(),
            ),
        )
        .unwrap();
    for (id, title, timestamp, status) in [
        ("a", "A", 400, "active"),
        ("b", "B", 300, "active"),
        ("c", "old C", 200, "active"),
        ("archived", "old archived", 100, "archived"),
    ] {
        vega_store::threads::create(
            store.conn(),
            vega_store::threads::NewThread {
                id,
                project_id: "snapshot-project",
                title,
                mode: "ask",
                permission_mode: "confirm",
                model: "snapshot-model",
                status,
                pinned: false,
                unread: false,
                created_at: timestamp,
                updated_at: timestamp,
            },
        )
        .unwrap();
    }
    let initial = service::snapshot(&store).unwrap();
    let initial = service::apply(
        &store,
        initial.revision,
        SidebarOrganizationAction::ReorderThread {
            thread_id: "a".into(),
            before_id: Some("b".into()),
        },
    )
    .unwrap()
    .snapshot;
    assert_eq!(
        initial.preferences.manual_thread_order,
        ["a", "b", "c", "archived"]
    );
    let opened = initial
        .threads
        .iter()
        .find(|row| row.id == "a")
        .unwrap()
        .clone();
    cx.update(|cx| {
        gpui_kit::component::init(cx);
        crate::init(cx);
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(VegaStore(Ok(store)));
        cx.set_global(SelectedProject(Some("snapshot-project".into())));
        cx.set_global(OpenedThread(Some(opened.clone())));
        cx.set_global(ProjectsCollapsed(false));
        cx.set_global(SessionsCollapsed(false));
        cx.set_global(SidebarCollapsed(false));
        cx.set_global(PendingDeleteConfirm(None));
        cx.set_global(SettingsOpen(false));
        cx.set_global(TaskMutationState::default());
    });
    let sidebar = cx.new(Sidebar::new);
    let block = sidebar.read_with(cx, |sidebar, _| sidebar.sessions_block.clone());
    let draft = cx.new(|cx| TextInput::new(cx, "Draft", false));
    draft.update(cx, |draft, cx| draft.set_text("snapshot unsent draft", cx));
    let mounted_draft = draft.clone();
    let window = cx.update(|cx| {
        cx.open_window(
            gpui_kit::WindowOptions {
                window_bounds: Some(gpui_kit::WindowBounds::Windowed(
                    gpui_kit::Bounds::centered(None, gpui_kit::size(px(1100.), px(900.)), cx),
                )),
                ..Default::default()
            },
            move |_, cx| {
                cx.new(|_| MountedSnapshotSidebar {
                    sidebar,
                    draft: mounted_draft,
                })
            },
        )
        .unwrap()
    });
    cx.run_until_parked();
    let fixture = SnapshotFixture {
        dir,
        block,
        draft,
        window,
        opened,
    };
    assert_projection(&fixture, cx, &initial, ["a", "b", "c"]);
    fixture
}

fn store(f: &SnapshotFixture) -> Store {
    Store::open(f.dir.path().join("snapshot.db")).unwrap()
}

fn snapshot(f: &SnapshotFixture) -> SidebarOrganizationSnapshot {
    service::snapshot(&store(f)).unwrap()
}

fn reorder(f: &SnapshotFixture, revision: u64) -> SidebarOrganizationOutcome {
    let outcome = service::apply(
        &store(f),
        revision,
        SidebarOrganizationAction::ReorderThread {
            thread_id: "c".into(),
            before_id: Some("a".into()),
        },
    )
    .unwrap();
    assert_eq!(outcome.snapshot.revision, revision + 1);
    assert_eq!(
        outcome.snapshot.preferences.manual_thread_order,
        ["c", "a", "b", "archived"]
    );
    assert_eq!(snapshot(f), outcome.snapshot);
    outcome
}

fn update_metadata(f: &SnapshotFixture, c_title: &str, archived_title: &str, unread: bool) {
    let store = store(f);
    for (id, title) in [("c", c_title), ("archived", archived_title)] {
        conversation::update_thread(
            &store,
            id,
            &ThreadUpdate {
                title: Some(title.into()),
                unread: Some(unread),
                ..Default::default()
            },
        )
        .unwrap();
    }
    let state = service::snapshot(&store).unwrap();
    for (id, title) in [("c", c_title), ("archived", archived_title)] {
        let row = state.threads.iter().find(|row| row.id == id).unwrap();
        assert_eq!(row.title, title);
        assert_eq!(row.unread, unread);
    }
}

fn request_fixture(
    f: &SnapshotFixture,
    cx: &mut gpui_kit::TestAppContext,
    generation: u64,
) -> OrganizationRequestToken {
    f.block.update(cx, |block, cx| {
        let org = block.organization.as_mut().unwrap();
        assert!(!org.refresh_queued);
        org.pending = true;
        org.generation = generation;
        OrganizationRequestToken {
            generation,
            epoch: cx.global::<TaskMutationState>().epoch,
            origin_route: (
                cx.global::<SelectedProject>().0.clone(),
                cx.global::<OpenedThread>()
                    .0
                    .as_ref()
                    .map(|row| row.id.clone()),
                cx.global::<SettingsOpen>().0,
            ),
            submitted_editor: None,
            was_mutation: false,
        }
    })
}

struct CompletionObservation {
    snapshot: SidebarOrganizationSnapshot,
    active: Vec<Thread>,
    archived: Vec<Thread>,
    pending: bool,
    refresh_queued: bool,
    generation: u64,
}

struct DelayedCompletion {
    release: oneshot::Sender<()>,
    observed: Rc<RefCell<Option<CompletionObservation>>>,
}

impl DelayedCompletion {
    fn deliver(self, cx: &mut gpui_kit::TestAppContext) -> CompletionObservation {
        self.release.send(()).unwrap();
        cx.run_until_parked();
        self.observed.borrow_mut().take().unwrap()
    }
}

fn delay_completion(
    f: &SnapshotFixture,
    cx: &mut gpui_kit::TestAppContext,
    request: OrganizationRequestToken,
    outcome: SidebarOrganizationOutcome,
) -> DelayedCompletion {
    let (release, delivery) = oneshot::channel();
    let observed = Rc::new(RefCell::new(None));
    let completion = observed.clone();
    f.block.update(cx, |_, cx| {
        cx.spawn(async move |this, cx| {
            delivery.await.unwrap();
            this.update(cx, |block, cx| {
                block.finish_organization_request(request, Ok(outcome), cx);
                let org = block.organization.as_ref().unwrap();
                *completion.borrow_mut() = Some(CompletionObservation {
                    snapshot: org.snapshot.clone().unwrap(),
                    active: block.threads.clone(),
                    archived: block.archived.clone(),
                    pending: org.pending,
                    refresh_queued: org.refresh_queued,
                    generation: org.generation,
                });
            })
            .unwrap();
        })
        .detach();
    });
    DelayedCompletion { release, observed }
}

fn read_outcome(snapshot: SidebarOrganizationSnapshot) -> SidebarOrganizationOutcome {
    SidebarOrganizationOutcome {
        snapshot,
        created_thread: None,
    }
}

fn assert_projection(
    f: &SnapshotFixture,
    cx: &mut gpui_kit::TestAppContext,
    expected: &SidebarOrganizationSnapshot,
    order: [&str; 3],
) {
    f.block.read_with(cx, |block, _| {
        let org = block.organization.as_ref().unwrap();
        assert_eq!(org.snapshot.as_ref(), Some(expected));
        assert_eq!(block.threads.len(), 3);
        assert_eq!(block.archived.len(), 1);
        for id in ["a", "b", "c", "archived"] {
            assert_eq!(
                block
                    .threads
                    .iter()
                    .chain(block.archived.iter())
                    .find(|row| row.id == id),
                expected.threads.iter().find(|row| row.id == id)
            );
        }
        assert!(block.error.is_none());
    });
    let mut visual = gpui_kit::VisualTestContext::from_window(f.window.into(), cx);
    let rows = order.map(|id| {
        let selector: &'static str = Box::leak(format!("project-thread-row-{id}").into_boxed_str());
        visual.debug_bounds(selector).unwrap()
    });
    for row in rows {
        assert_eq!(row.size.height, px(Typography::SIDEBAR_LINE_HEIGHT));
        assert!(row.size.width > px(0.));
    }
    for pair in rows.windows(2) {
        assert!(pair[0].bottom() <= pair[1].top());
        assert!(pair[0].top() < pair[1].top());
        assert_eq!(pair[0].left(), pair[1].left());
    }
    assert!(visual.debug_bounds("project-thread-row-archived").is_none());
    cx.update(|cx| {
        assert_eq!(
            cx.global::<SelectedProject>().0.as_deref(),
            Some("snapshot-project")
        );
        assert_eq!(cx.global::<OpenedThread>().0.as_ref(), Some(&f.opened));
        assert!(!cx.global::<SettingsOpen>().0);
        assert!(cx.global::<PendingDeleteConfirm>().0.is_none());
    });
    assert_eq!(
        f.draft.read_with(cx, |draft, _| draft.text().to_string()),
        "snapshot unsent draft"
    );
}

#[gpui_kit::test]
async fn late_snapshot_revision_cannot_roll_back_rendered_committed_order(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx);
    let s0 = snapshot(&f);
    let old_request = request_fixture(&f, cx, 40);
    let old = delay_completion(&f, cx, old_request, read_outcome(s0.clone()));
    update_metadata(&f, "current C", "current archived", true);
    let s1 = reorder(&f, s0.revision);
    let current_request = request_fixture(&f, cx, 40);
    let current = delay_completion(&f, cx, current_request, s1.clone());
    let accepted = current.deliver(cx);
    assert_eq!(accepted.snapshot, s1.snapshot);
    assert!(!accepted.pending);
    assert!(!accepted.refresh_queued);
    assert!(old.observed.borrow().is_none());
    assert_projection(&f, cx, &s1.snapshot, ["c", "a", "b"]);
    let late = old.deliver(cx);
    assert_eq!(late.snapshot, s1.snapshot);
    assert_eq!(late.generation, 40);
    assert!(!late.pending);
    assert!(!late.refresh_queued);
    assert_eq!(snapshot(&f), s1.snapshot);
    assert_projection(&f, cx, &s1.snapshot, ["c", "a", "b"]);
}

#[gpui_kit::test]
async fn late_request_generation_preserves_current_pending_request_and_rendered_snapshot(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx);
    let s0 = snapshot(&f);
    let old_request = request_fixture(&f, cx, 40);
    let old = delay_completion(&f, cx, old_request, read_outcome(s0.clone()));
    update_metadata(&f, "current C", "current archived", true);
    let s1 = reorder(&f, s0.revision);
    let current_request = request_fixture(&f, cx, 41);
    let current = delay_completion(&f, cx, current_request, s1.clone());
    current.deliver(cx);
    assert!(old.observed.borrow().is_none());
    assert_projection(&f, cx, &s1.snapshot, ["c", "a", "b"]);
    let pending_request = request_fixture(&f, cx, 42);
    let pending = delay_completion(&f, cx, pending_request, read_outcome(snapshot(&f)));
    let late = old.deliver(cx);
    assert_eq!(late.snapshot, s1.snapshot);
    assert!(late.pending);
    assert_eq!(late.generation, 42);
    assert!(!late.refresh_queued);
    assert!(pending.observed.borrow().is_none());
    assert_projection(&f, cx, &s1.snapshot, ["c", "a", "b"]);
    let completed = pending.deliver(cx);
    assert!(!completed.pending);
    assert_eq!(completed.generation, 42);
    assert_eq!(snapshot(&f), s1.snapshot);
    assert_projection(&f, cx, &s1.snapshot, ["c", "a", "b"]);
}

#[gpui_kit::test]
async fn task_mutation_epoch_preserves_cached_metadata_then_refreshes_authoritatively(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx);
    let reordered = reorder(&f, snapshot(&f).revision);
    let s0 = snapshot(&f);
    assert_eq!(s0, reordered.snapshot);
    let old_request = request_fixture(&f, cx, 40);
    let old_epoch = old_request.epoch;
    let old = delay_completion(&f, cx, old_request, read_outcome(s0.clone()));
    cx.update(begin_task_mutation);
    update_metadata(&f, "current C", "current archived", true);
    cx.update(finish_task_mutation);
    let s1 = snapshot(&f);
    assert_eq!(s0.revision, s1.revision);
    assert_ne!(s0.threads, s1.threads);
    let current_request = request_fixture(&f, cx, 40);
    assert_ne!(current_request.epoch, old_epoch);
    let current = delay_completion(&f, cx, current_request, read_outcome(s1.clone()));
    current.deliver(cx);
    assert!(old.observed.borrow().is_none());
    assert_projection(&f, cx, &s1, ["c", "a", "b"]);
    cx.update(begin_task_mutation);
    update_metadata(&f, "authoritative C", "authoritative archived", false);
    cx.update(finish_task_mutation);
    let authoritative = snapshot(&f);
    assert_eq!(authoritative.revision, s1.revision);
    let late = old.deliver(cx);
    assert_eq!(late.snapshot, s0);
    assert!(late.pending);
    assert!(!late.refresh_queued);
    for id in ["c", "archived"] {
        assert_eq!(
            late.active
                .iter()
                .chain(late.archived.iter())
                .find(|row| row.id == id),
            s1.threads.iter().find(|row| row.id == id)
        );
    }
    f.block.read_with(cx, |block, cx| {
        let org = block.organization.as_ref().unwrap();
        assert!(!org.pending);
        assert!(!org.refresh_queued);
        let mutation = cx.global::<TaskMutationState>();
        assert_eq!(mutation.pending, 0);
        assert_ne!(mutation.epoch, old_epoch);
    });
    assert_projection(&f, cx, &authoritative, ["c", "a", "b"]);
    assert_eq!(snapshot(&f), authoritative);
}
