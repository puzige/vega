use super::*;
use crate::native_subagents::NativeSubagentUpdate;
use core::prelude::v1::test;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{TestAppContext, VisualTestContext};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use vega_conversation::agent::{PermissionQueue, UserQuestionQueue};
use vega_conversation::types::SubagentAdmission;
use vega_store::Store;

struct Fixture {
    _directory: tempfile::TempDir,
    database: PathBuf,
    parent: Thread,
    first: Thread,
    other: Thread,
}

impl Fixture {
    fn new(cx: &mut TestAppContext) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("state.sqlite");
        let store = Store::open(&database).unwrap();
        store.migrate().unwrap();
        let project = vega_store::projects::create(
            store.conn(),
            directory.path().to_str().unwrap(),
            "Workspace",
            None,
        )
        .unwrap();
        let parent = threads::create_thread(&store, &project.id, "fixture", "confirm").unwrap();
        let first = threads::create_thread(&store, &project.id, "fixture", "confirm").unwrap();
        let other = threads::create_thread(&store, &project.id, "fixture", "confirm").unwrap();
        store.conn().execute("INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES('parent-run',?1,1,'assistant','','streaming',1)", [&parent.id]).unwrap();
        for (child, message, name, created) in [
            (&first, "first", "Worker", 10),
            (&other, "other-first", "Other worker", 20),
        ] {
            store.conn().execute("INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES(?1,?2,1,'user','Original task','done',1)", [&format!("{message}-input"), &child.id]).unwrap();
            store.conn().execute("INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES(?1,?2,2,'assistant','Original answer','done',2)", [message, &child.id]).unwrap();
            vega_store::subagents::insert(
                store.conn(),
                &vega_store::subagents::DelegationRow {
                    child_thread_id: child.id.clone(),
                    parent_thread_id: parent.id.clone(),
                    parent_message_id: "parent-run".into(),
                    spawn_call_id: format!("spawn-{message}"),
                    child_message_id: message.into(),
                    name: name.into(),
                    task: "Original task".into(),
                    state: "completed".into(),
                    result: "Original answer".into(),
                    truncated: false,
                    created_at: created,
                },
            )
            .unwrap();
        }
        let first = threads::get_thread(&store, &first.id).unwrap();
        let other = threads::get_thread(&store, &other.id).unwrap();
        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_global(Theme::light());
            cx.set_global(SettingsOpen(false));
            cx.set_global(SidebarCollapsed(true));
            cx.set_global(ProjectsCollapsed(false));
            cx.set_global(SessionsCollapsed(false));
            cx.set_global(PendingDeleteConfirm(None));
            cx.set_global(SelectedProject(Some(project.id)));
            cx.set_global(OpenedThread(Some(parent.clone())));
            cx.set_global(NewTaskDefaults::default());
            cx.set_global(VegaStore(Ok(store)));
            vega_ui::init(cx);
        });
        Self {
            _directory: directory,
            database,
            parent,
            first,
            other,
        }
    }

    fn mount<'a>(
        &self,
        cx: &'a mut TestAppContext,
    ) -> (
        Entity<VegaWindow>,
        Entity<ConversationStream>,
        &'a mut VisualTestContext,
    ) {
        let parent_thread = self.parent.clone();
        let mut parent_stream = None;
        let (root, cx) = cx.add_window_view(|window, cx| {
            let mut root = VegaWindow::new(cx);
            let stream = cx.new(|cx| ConversationStream::new(parent_thread.clone(), cx));
            root.agent_controller
                .begin(parent_thread.id.clone(), stream.clone(), None, None)
                .unwrap();
            root.agent_controller
                .active
                .get_mut(&parent_thread.id)
                .unwrap()
                .root_message_id = Some("parent-run".into());
            root.stream_view = Some((parent_thread.id.clone(), stream.clone()));
            root.bind_conversation_stream(&stream, window, cx);
            root.sync_workspace_route(cx);
            root.sync_subagent_panel_route(cx);
            parent_stream = Some(stream);
            root
        });
        (root, parent_stream.unwrap(), cx)
    }

    fn snapshot(&self, message: &str, state: SubagentState) -> SubagentSnapshot {
        SubagentSnapshot {
            agent_id: self.first.id.clone(),
            parent_thread_id: self.parent.id.clone(),
            parent_message_id: "parent-run".into(),
            message_id: message.into(),
            name: "Worker".into(),
            state,
            result: if state == SubagentState::Completed {
                "Original answer".into()
            } else {
                String::new()
            },
            truncated: false,
        }
    }

    fn admit(
        &self,
        root: &Entity<VegaWindow>,
        parent: &Entity<ConversationStream>,
        cx: &mut VisualTestContext,
    ) -> (
        oneshot::Receiver<Result<(), vega_runtime::VegaError>>,
        CancellationToken,
    ) {
        let store = Store::open(&self.database).unwrap();
        store.conn().execute("INSERT OR IGNORE INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES('second-input',?1,3,'user','Related follow-up','done',30)", [&self.first.id]).unwrap();
        let cancel = CancellationToken::new();
        let admission = SubagentAdmission {
            thread: self.first.clone(),
            snapshot: self.snapshot("second", SubagentState::Running),
            cancel: cancel.clone(),
            permission_queue: PermissionQueue::new(),
            user_questions: UserQuestionQueue::new(),
            task: "Related follow-up".into(),
            queue_generation: "second-queue".into(),
        };
        let (reply, receiver) = oneshot::channel();
        root.update(cx, |root, cx| {
            root.apply_subagent_update(
                &self.parent.id,
                parent,
                NativeSubagentUpdate::Admit {
                    admission: Box::new(admission),
                    reply,
                },
                cx,
            )
        });
        (receiver, cancel)
    }
}

#[gpui_kit::test]
async fn already_open_detail_follows_reuse_with_history_and_ignores_stale_generation_updates(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new(cx);
    let (root, parent, cx) = fixture.mount(cx);
    cx.condition(&root, |root, _| !root.subagent_panel.loading)
        .await;
    parent.update(cx, |_, cx| {
        cx.emit(SubagentOpenRequested {
            source_thread_id: fixture.parent.id.clone(),
            thread_id: fixture.first.id.clone(),
        });
    });
    cx.condition(&root, |root, _| {
        root.subagent_panel
            .detail
            .as_ref()
            .is_some_and(|detail| !detail.loading)
    })
    .await;
    let previous = root
        .read(cx)
        .subagent_panel
        .detail
        .as_ref()
        .unwrap()
        .stream
        .clone()
        .unwrap();
    assert!(previous.update(cx, |stream, cx| stream.reveal_loaded_message("first", cx)));
    let (receipt, cancel) = fixture.admit(&root, &parent, cx);
    assert!(receipt.await.unwrap().is_ok());
    cx.run_until_parked();
    let current = root
        .read(cx)
        .agent_controller
        .active_stream_for_thread(&fixture.first.id)
        .unwrap();
    root.update(cx, |root, cx| {
        for event in [
            ConversationEvent::MessageStarted {
                message_id: "second".into(),
                seq: 4,
            },
            ConversationEvent::RunStarted {
                message_id: "second".into(),
                started_at: std::time::Instant::now(),
            },
            ConversationEvent::TextDelta {
                message_id: "second".into(),
                delta: "Follow-up answer".into(),
            },
        ] {
            root.apply_subagent_update(
                &fixture.parent.id,
                &parent,
                NativeSubagentUpdate::Event {
                    thread_id: fixture.first.id.clone(),
                    message_id: "second".into(),
                    event,
                },
                cx,
            );
        }
    });
    assert_eq!(
        root.read(cx)
            .subagent_panel
            .detail
            .as_ref()
            .unwrap()
            .stream
            .as_ref(),
        Some(&current)
    );
    assert!(
        current.update(cx, |stream, cx| stream.reveal_loaded_message("first", cx)),
        "opening a follow-up must retain the first execution's transcript"
    );
    assert!(current.update(cx, |stream, cx| stream.reveal_loaded_message("second", cx)));
    root.update(cx, |root, cx| {
        for update in [
            NativeSubagentUpdate::Updated(fixture.snapshot("first", SubagentState::Completed)),
            NativeSubagentUpdate::Finished {
                snapshot: fixture.snapshot("first", SubagentState::Completed),
                usage_complete: true,
            },
        ] {
            root.apply_subagent_update(&fixture.parent.id, &parent, update, cx);
        }
        let active = root.agent_controller.active.get(&fixture.first.id).unwrap();
        assert_eq!(active.delegated_message_id.as_deref(), Some("second"));
        assert_eq!(active.stream, current);
        assert!(!active.cancel.is_cancelled());
        assert_eq!(
            root.subagent_panel.detail.as_ref().unwrap().stream.as_ref(),
            Some(&current)
        );
        assert_eq!(
            root.subagent_panel
                .agents
                .iter()
                .map(|agent| agent.agent_id.as_str())
                .collect::<Vec<_>>(),
            vec![fixture.first.id.as_str(), fixture.other.id.as_str()]
        );
        assert_eq!(root.subagent_panel.agents[0].message_id, "second");
        assert_eq!(root.subagent_panel.agents[0].state, SubagentState::Running);
        assert_eq!(
            root.workspace
                .tabs
                .iter()
                .filter(|(key, _)| *key == super::super::workspace::TabKey::Subagents)
                .count(),
            1
        );
    });
    cx.update(|window, cx| {
        window.render_frame(cx);
        assert_eq!(
            cx.global::<OpenedThread>().0.as_ref().unwrap().id,
            fixture.parent.id
        );
    });
    root.update(cx, |root, cx| {
        root.stop_subagent(
            parent.clone(),
            &SubagentStopRequested {
                source_thread_id: fixture.parent.id.clone(),
                thread_id: fixture.first.id.clone(),
                message_id: "first".into(),
            },
            cx,
        );
        assert!(!cancel.is_cancelled());
    });
    cx.update(|window, cx| {
        window.render_frame(cx);
        window.click(format!("subagent-panel-stop-{}", fixture.first.id), cx);
    });
    cx.condition(&root, |_, _| cancel.is_cancelled()).await;
    assert!(
        root.read(cx)
            .agent_controller
            .active
            .contains_key(&fixture.parent.id)
    );
}

#[gpui_kit::test]
async fn manual_and_delegated_child_runs_have_one_owner_in_both_admission_orders(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new(cx);
    let (root, parent, cx) = fixture.mount(cx);
    let manual_stream = cx.new(|cx| ConversationStream::new(fixture.first.clone(), cx));
    let (manual_generation, manual_cancel) = root.update(cx, |root, _| {
        root.agent_controller
            .begin(
                fixture.first.id.clone(),
                manual_stream.clone(),
                Some("Manual task".into()),
                None,
            )
            .unwrap()
    });
    cx.update(|window, cx| {
        cx.set_global(OpenedThread(Some(fixture.first.clone())));
        window.render_frame(cx);
        assert_eq!(
            root.read(cx).stream_view.as_ref().map(|(_, stream)| stream),
            Some(&manual_stream)
        );
    });
    let (receipt, rejected_cancel) = fixture.admit(&root, &parent, cx);
    assert!(receipt.await.unwrap().is_err());
    assert!(rejected_cancel.is_cancelled());
    assert!(!manual_cancel.is_cancelled());
    root.update(cx, |root, _| {
        assert_eq!(
            root.agent_controller
                .active
                .get(&fixture.first.id)
                .unwrap()
                .generation,
            manual_generation
        );
        assert_eq!(
            root.agent_controller
                .active_stream_for_thread(&fixture.first.id),
            Some(manual_stream.clone())
        );
        assert!(
            root.agent_controller
                .finish(manual_generation, &fixture.first.id, &manual_stream)
                .is_some()
        );
    });
    let (receipt, delegated_cancel) = fixture.admit(&root, &parent, cx);
    assert!(receipt.await.unwrap().is_ok());
    root.update(cx, |root, _| {
        let generation = root
            .agent_controller
            .active
            .get(&fixture.first.id)
            .unwrap()
            .generation;
        assert!(
            root.agent_controller
                .begin(
                    fixture.first.id.clone(),
                    manual_stream,
                    Some("Manual task".into()),
                    None
                )
                .is_none()
        );
        assert_eq!(
            root.agent_controller
                .active
                .get(&fixture.first.id)
                .unwrap()
                .generation,
            generation
        );
        assert_eq!(
            root.agent_controller
                .active
                .get(&fixture.first.id)
                .unwrap()
                .delegated_message_id
                .as_deref(),
            Some("second")
        );
        assert!(!delegated_cancel.is_cancelled());
    });
    cx.update(|window, cx| {
        window.render_frame(cx);
        let current = root
            .read(cx)
            .agent_controller
            .active_stream_for_thread(&fixture.first.id)
            .unwrap();
        assert_eq!(
            root.read(cx).stream_view.as_ref().map(|(_, stream)| stream),
            Some(&current)
        );
        assert!(current.update(cx, |stream, cx| stream.reveal_loaded_message("first", cx)));
    });
}
