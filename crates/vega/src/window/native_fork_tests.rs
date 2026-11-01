use super::*;
use gpui_kit as gpui;
use gpui_kit::TestAppContext;
use vega_store::Store;

struct Fixture {
    _root: tempfile::TempDir,
    store: Store,
    source: Thread,
    stream: Entity<ConversationStream>,
    window: Entity<VegaWindow>,
    request: ForkRequested,
}

impl Fixture {
    fn new(cx: &mut TestAppContext) -> Self {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("vega.db");
        let workspace = vega_conversation::startup::prepare_workspace(
            &database,
            &root.path().join("config.yaml"),
        )
        .unwrap();
        let store = Store::open_existing(&database).unwrap();
        store.conn().execute_batch(
            "INSERT INTO projects(id,path,name,created_at,last_opened_at) VALUES('project','/fixture','Fixture',1,1);
             INSERT INTO threads(id,project_id,title,model,created_at,updated_at) VALUES('source','project','Source','model',1,1);
             INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES
               ('question','source',1,'user','Question','done',1),
               ('answer','source',2,'assistant','Answer','streaming',2);"
        ).unwrap();
        assert!(
            vega_store::messages::record_assistant_response(
                store.conn(),
                "answer",
                "Answer",
                &vega_store::messages::AssistantResponseRow {
                    message_id: "answer".into(),
                    response_id: "final".into(),
                    text_start_bytes: 0,
                    text_end_bytes: 6,
                    kind: "final".into(),
                },
            )
            .unwrap()
        );
        vega_store::messages::finish(store.conn(), "answer", "Answer", "done").unwrap();
        let source = vega_conversation::threads::get_thread(&store, "source").unwrap();
        let page = vega_conversation::history::latest_history_page(&store, "source", 20).unwrap();
        let request = ForkRequested {
            thread_id: "source".into(),
            message_id: "answer".into(),
        };
        let (window, stream) = cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_global(vega_theme::Theme::light());
            cx.set_global(SettingsOpen(false));
            vega_ui::init(cx);
            vega_ui::sidebar::init(workspace, cx);
            cx.set_global(SelectedProject(Some("project".into())));
            cx.set_global(OpenedThread(Some(source.clone())));
            let stream = cx.new(|cx| ConversationStream::new(source.clone(), cx));
            stream.update(cx, |stream, cx| stream.apply_history_page(page, cx));
            assert!(stream.read(cx).accepts_fork_request(&request));
            assert!(stream.read(cx).fork_is_idle());
            let window = cx.new(|cx| VegaWindow::with_background_services(false, cx));
            window.update(cx, |window, _| {
                window.stream_view = Some((source.id.clone(), stream.clone()));
                window.update_message_location_route(Some(&source.id));
            });
            (window, stream)
        });
        Self {
            _root: root,
            store,
            source,
            stream,
            window,
            request,
        }
    }

    fn request(&self, cx: &mut TestAppContext) {
        cx.update(|cx| {
            self.window.update(cx, |window, cx| {
                window.fork_conversation(self.stream.clone(), &self.request, cx);
            })
        });
    }

    fn drain(&self, cx: &mut TestAppContext) {
        for _ in 0..10000 {
            if !cx.read(|cx| self.window.read(cx).trusted_actions.is_busy()) {
                return;
            }
            assert!(
                cx.executor().tick(),
                "fork work stopped before releasing its lease"
            );
        }
        panic!("fork work did not release its lease");
    }

    fn forks(&self) -> Vec<String> {
        self.store
            .conn()
            .prepare("SELECT id FROM threads WHERE forked_from_thread_id='source' ORDER BY id")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    fn opened(&self, cx: &TestAppContext) -> String {
        cx.read(|cx| cx.global::<OpenedThread>().0.as_ref().unwrap().id.clone())
    }

    fn assert_rejected(&self, cx: &TestAppContext) {
        assert!(self.forks().is_empty());
        assert_eq!(self.opened(cx), self.source.id);
        assert!(!cx.read(|cx| self.window.read(cx).trusted_actions.is_busy()));
    }

    fn assert_success(&self, cx: &TestAppContext) {
        let forks = self.forks();
        assert_eq!(forks.len(), 1);
        assert_eq!(self.opened(cx), forks[0]);
        let source: Vec<(String, String)> = self
            .store
            .conn()
            .prepare("SELECT id,content FROM messages WHERE thread_id='source' ORDER BY seq")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            source,
            [
                ("question".into(), "Question".into()),
                ("answer".into(), "Answer".into())
            ]
        );
        let target: Vec<String> = self
            .store
            .conn()
            .prepare("SELECT content FROM messages WHERE thread_id=?1 ORDER BY seq")
            .unwrap()
            .query_map([&forks[0]], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(target, ["Question", "Answer"]);
    }
}

#[gpui_kit::test]
fn duplicate_requests_copy_once_and_navigate_after_completion(cx: &mut TestAppContext) {
    let fixture = Fixture::new(cx);
    cx.update(|cx| {
        fixture.window.update(cx, |window, cx| {
            window.fork_conversation(fixture.stream.clone(), &fixture.request, cx);
            assert!(window.trusted_actions.is_busy());
            window.fork_conversation(fixture.stream.clone(), &fixture.request, cx);
        })
    });
    fixture.drain(cx);
    fixture.assert_success(cx);
}

#[gpui_kit::test]
fn shared_database_admission_refuses_fork_then_allows_retry(cx: &mut TestAppContext) {
    let fixture = Fixture::new(cx);
    let peer = Store::open_existing(fixture.store.database_path().unwrap()).unwrap();
    let admission = vega_store::thread_admission::try_acquire(peer.database_identity(), "source")
        .unwrap()
        .unwrap();
    fixture.request(cx);
    fixture.drain(cx);
    fixture.assert_rejected(cx);
    drop(admission);
    fixture.request(cx);
    fixture.drain(cx);
    fixture.assert_success(cx);
}

#[gpui_kit::test]
fn storage_failure_is_atomic_releases_lease_and_permits_same_request_retry(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new(cx);
    fixture.store.conn().execute_batch("CREATE TRIGGER reject_fork BEFORE INSERT ON threads WHEN NEW.forked_from_thread_id IS NOT NULL BEGIN SELECT RAISE(ABORT,'fixture storage failure'); END;").unwrap();
    fixture.request(cx);
    fixture.drain(cx);
    fixture.assert_rejected(cx);
    fixture
        .store
        .conn()
        .execute_batch("DROP TRIGGER reject_fork;")
        .unwrap();
    fixture.request(cx);
    fixture.drain(cx);
    fixture.assert_success(cx);
}

#[gpui_kit::test]
fn away_and_back_generation_prevents_committed_fork_from_stealing_route(cx: &mut TestAppContext) {
    let fixture = Fixture::new(cx);
    cx.update(|cx| {
        fixture.window.update(cx, |window, cx| {
            window.fork_conversation(fixture.stream.clone(), &fixture.request, cx);
            let original = window.message_location_route_generation;
            window.update_message_location_route(Some("other"));
            window.update_message_location_route(Some("source"));
            assert_ne!(window.message_location_route_generation, original);
            assert!(window.owns_stream_request(&fixture.stream, "source", cx));
        })
    });
    fixture.drain(cx);
    assert_eq!(fixture.forks().len(), 1);
    assert_eq!(fixture.opened(cx), "source");
}

#[gpui_kit::test]
fn replaced_stream_prevents_stale_completion_and_stale_request_creates_nothing(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new(cx);
    cx.update(|cx| {
        fixture.window.update(cx, |window, cx| {
            let replacement = cx.new(|cx| ConversationStream::new(fixture.source.clone(), cx));
            window.stream_view = Some(("source".into(), replacement));
            window.fork_conversation(fixture.stream.clone(), &fixture.request, cx);
        })
    });
    fixture.assert_rejected(cx);
    cx.update(|cx| {
        fixture.window.update(cx, |window, cx| {
            window.stream_view = Some(("source".into(), fixture.stream.clone()));
            window.fork_conversation(fixture.stream.clone(), &fixture.request, cx);
            let replacement = cx.new(|cx| ConversationStream::new(fixture.source.clone(), cx));
            window.stream_view = Some(("source".into(), replacement));
        })
    });
    fixture.drain(cx);
    assert_eq!(fixture.forks().len(), 1);
    assert_eq!(fixture.opened(cx), "source");
}

#[gpui_kit::test]
fn controller_running_refuses_fork_and_cleared_owner_allows_retry(cx: &mut TestAppContext) {
    let fixture = Fixture::new(cx);
    cx.update(|cx| {
        fixture.window.update(cx, |window, cx| {
            assert!(
                window
                    .agent_controller
                    .begin("source".into(), fixture.stream.clone(), None, None)
                    .is_some()
            );
        })
    });
    fixture.request(cx);
    fixture.assert_rejected(cx);
    cx.update(|cx| {
        fixture.window.update(cx, |window, _| {
            window.agent_controller.active.remove("source");
        })
    });
    fixture.request(cx);
    fixture.drain(cx);
    fixture.assert_success(cx);
}

#[gpui_kit::test]
fn global_running_refuses_fork_and_cleared_owner_allows_retry(cx: &mut TestAppContext) {
    let fixture = Fixture::new(cx);
    cx.update(|cx| vega_ui::sidebar::set_thread_running("source", true, cx));
    fixture.request(cx);
    fixture.assert_rejected(cx);
    cx.update(|cx| vega_ui::sidebar::set_thread_running("source", false, cx));
    fixture.request(cx);
    fixture.drain(cx);
    fixture.assert_success(cx);
}

#[gpui_kit::test]
fn stream_running_refuses_fork_without_starting_copy(cx: &mut TestAppContext) {
    let fixture = Fixture::new(cx);
    cx.update(|cx| {
        fixture.stream.update(cx, |stream, cx| {
            stream.apply_event(
                ConversationEvent::MessageStarted {
                    message_id: "in-memory-run".into(),
                    seq: 3,
                },
                cx,
            );
            assert!(!stream.fork_is_idle());
        })
    });
    fixture.request(cx);
    fixture.assert_rejected(cx);
}
