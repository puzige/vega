use super::*;
use gpui_kit::TestAppContext;
use tempfile::TempDir;

struct Fixture {
    _directory: TempDir,
    store: Store,
    a: Thread,
    b: Thread,
    block: Entity<ThreadsBlock>,
}

fn fixture(cx: &mut TestAppContext) -> Fixture {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("unread.db");
    let store = Store::open(&database).unwrap();
    store.migrate().unwrap();
    let project = projects::create(
        store.conn(),
        directory.path().to_str().unwrap(),
        "Synthetic",
        None,
    )
    .unwrap();
    let mut a = conversation::create_thread(&store, &project.id, "mock", "confirm").unwrap();
    let b = conversation::create_thread(&store, &project.id, "mock", "confirm").unwrap();
    conversation::update_thread(
        &store,
        &a.id,
        &ThreadUpdate {
            unread: Some(true),
            ..Default::default()
        },
    )
    .unwrap();
    a.unread = true;
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(SelectedProject(Some(project.id)));
        cx.set_global(OpenedThread(Some(a.clone())));
        cx.set_global(SettingsOpen(false));
        cx.set_global(SessionsCollapsed(false));
        cx.set_global(VegaStore(Ok(Store::open(&database).unwrap())));
        crate::init(cx);
    });
    let block = cx.new(ThreadsBlock::new);
    Fixture {
        _directory: directory,
        store,
        a,
        b,
        block,
    }
}

impl Fixture {
    fn row(&self, id: &str) -> vega_store::threads::ThreadRow {
        vega_store::threads::find(self.store.conn(), id)
            .unwrap()
            .unwrap()
    }

    fn mark(&self, unread: bool, cx: &mut TestAppContext) {
        self.block.update(cx, |block, cx| {
            block.apply_update(
                &self.a,
                ThreadUpdate {
                    unread: Some(unread),
                    ..Default::default()
                },
                cx,
            )
        });
    }
}

#[gpui_kit::test]
async fn manual_read_commits_before_a_following_completion_and_a_visit_clears_only_its_task(
    cx: &mut TestAppContext,
) {
    let f = fixture(cx);
    let before = f.row(&f.a.id);
    let b = f.row(&f.b.id);
    f.mark(false, cx);
    assert!(!f.row(&f.a.id).unread);
    assert_eq!(f.row(&f.a.id).updated_at, before.updated_at);
    assert!(!cx.read(|cx| cx.global::<OpenedThread>().0.as_ref().unwrap().unread));
    conversation::update_thread(
        &f.store,
        &f.a.id,
        &ThreadUpdate {
            unread: Some(true),
            ..Default::default()
        },
    )
    .unwrap();
    cx.run_until_parked();
    assert!(f.row(&f.a.id).unread);
    f.block
        .update(cx, |block, cx| block.open_thread(&f.a.id, cx));
    assert!(!f.row(&f.a.id).unread);
    assert!(!cx.read(|cx| cx.global::<OpenedThread>().0.as_ref().unwrap().unread));
    assert_eq!(f.row(&f.b.id), b);
    f.mark(true, cx);
    assert!(f.row(&f.a.id).unread);
    let marked = f.row(&f.a.id);
    f.block.update(cx, ThreadsBlock::reload);
    cx.run_until_parked();
    assert_eq!(f.row(&f.a.id), marked);
}

#[gpui_kit::test]
async fn manual_unread_preserves_mutation_guards_and_reports_storage_failure(
    cx: &mut TestAppContext,
) {
    let f = fixture(cx);
    let before = f.row(&f.a.id);
    cx.update(crate::navigation::begin_task_mutation);
    f.mark(false, cx);
    assert_eq!(f.row(&f.a.id), before);
    assert!(f.block.read_with(cx, |block, _| block.error.is_some()));
    cx.update(crate::navigation::finish_task_mutation);
    f.block.update(cx, |block, _| block.action_pending = true);
    f.mark(false, cx);
    assert_eq!(f.row(&f.a.id), before);
    f.block.update(cx, |block, _| block.action_pending = false);
    cx.update(|cx| cx.set_global(VegaStore(Err("synthetic unavailable store".into()))));
    f.mark(false, cx);
    assert_eq!(f.row(&f.a.id), before);
    assert!(f.block.read_with(cx, |block, _| block.error.is_some()));
    assert!(cx.read(|cx| cx.global::<OpenedThread>().0.as_ref().unwrap().unread));
    assert_eq!(
        cx.read(|cx| cx.global::<crate::navigation::TaskMutationState>().pending),
        0
    );
}
