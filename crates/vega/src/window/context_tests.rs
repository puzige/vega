use super::{ContextOwner, ManualContextRun, context_record};
use crate::trusted_action::TrustedActionKind;
use crate::window::VegaWindow;
use gpui_kit::{AppContext, Entity, TestAppContext};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;
use vega_conversation::types::{
    ContextCompactionCancelRequested, ContextCompactionFailureCode, ContextCompactionRequested,
    ContextCompactionStatus, ContextCompactionStatusRecord, ContextCompactionUsageState,
    ConversationEvent, PermissionMode, TaskBackend, Thread, ThreadMode, ThreadStatus,
};
use vega_theme::Theme;
use vega_ui::conversation_stream::ConversationStream;
use vega_ui::settings::SettingsOpen;
use vega_ui::sidebar::{OpenedThread, SelectedProject, VegaStore};

#[path = "context_tests/precompute_lifecycle_tests.rs"]
mod precompute_lifecycle_tests;

type WorkerResult = Result<Vec<ConversationEvent>, ()>;

struct Fixture {
    window: Entity<VegaWindow>,
    stream: Entity<ConversationStream>,
    started: mpsc::Receiver<CancellationToken>,
    completion: mpsc::SyncSender<WorkerResult>,
    _directory: tempfile::TempDir,
}

fn thread() -> Thread {
    Thread {
        parent_thread_id: None,
        forked_from_thread_id: None,
        forked_from_message_id: None,
        id: "synthetic-thread".into(),
        backend: TaskBackend::Native,
        project_id: String::new(),
        title: "Synthetic context lifecycle".into(),
        mode: ThreadMode::Execute,
        permission_mode: PermissionMode::ReadOnly,
        permission_revision: 0,
        draft_authority_epoch: None,
        full_access_grant: None,
        default_permission: None,
        reasoning_preferences: Some(Vec::new()),
        model: "gpt-4.1".into(),
        status: ThreadStatus::Active,
        pinned: false,
        unread: false,
        created_at: 1,
        updated_at: 1,
    }
}

impl Fixture {
    fn new(cx: &mut TestAppContext) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let store = vega_store::Store::open(directory.path().join("context.sqlite")).unwrap();
        store.migrate().unwrap();
        let (started_tx, started) = mpsc::sync_channel(1);
        let (completion, completed) = mpsc::sync_channel(1);
        let completed = Mutex::new(completed);
        let (window, stream) = cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_global(Theme::light());
            cx.set_global(OpenedThread(Some(thread())));
            cx.set_global(SelectedProject(None));
            cx.set_global(SettingsOpen(false));
            cx.set_global(VegaStore(Ok(store)));
            let stream = cx.new(|cx| ConversationStream::new(thread(), cx));
            let window = cx.new(|cx| VegaWindow::with_background_services(false, cx));
            window.update(cx, |window, cx| {
                window.context_controller.config_path = Some(directory.path().join("config.toml"));
                window.stream_view = Some((thread().id, stream.clone()));
                window.context_controller.owner = Some(ContextOwner {
                    epoch: 0,
                    thread_id: thread().id,
                    model: thread().model,
                    stream: stream.clone(),
                });
                window.context_controller.worker = Some(Arc::new(move |cancel| {
                    started_tx.send(cancel).map_err(|_| ())?;
                    completed
                        .lock()
                        .map_err(|_| ())?
                        .recv_timeout(Duration::from_secs(10))
                        .map_err(|_| ())?
                }));
                window.bind_context_compaction(&stream, cx);
            });
            (window, stream)
        });
        Self {
            window,
            stream,
            started,
            completion,
            _directory: directory,
        }
    }

    fn request(&self, cx: &mut TestAppContext, model: &str, id: u64) {
        self.stream.update(cx, |_, cx| {
            cx.emit(ContextCompactionRequested {
                thread_id: thread().id,
                model: model.into(),
                request_id: id,
                selection: None,
                summary_requirements: String::new(),
            });
        });
        cx.run_until_parked();
    }

    fn navigate(&self, cx: &mut TestAppContext, next: Thread, stream: &Entity<ConversationStream>) {
        cx.update(|cx| {
            stream.update(cx, |stream, cx| stream.apply_thread(next.clone(), cx));
            cx.set_global(OpenedThread(Some(next.clone())));
            self.window.update(cx, |window, _| {
                window.stream_view = Some((next.id, stream.clone()));
            });
        });
        cx.run_until_parked();
        self.window.update(cx, |window, cx| {
            window.sync_context_route(stream, cx);
        });
        cx.run_until_parked();
    }

    fn active(&self, cx: &TestAppContext) -> ManualContextRun {
        self.window.read_with(cx, |window, _| {
            window.context_controller.manual.clone().unwrap()
        })
    }

    fn status(&self, cx: &TestAppContext) -> ContextCompactionStatusRecord {
        self.stream
            .read_with(cx, |stream, _| stream.context_status_snapshot().unwrap())
    }

    fn assert_released(&self, cx: &TestAppContext) {
        assert!(
            !self
                .window
                .read_with(cx, |window, _| window.trusted_actions.is_busy())
        );
        assert!(
            !self
                .stream
                .read_with(cx, |stream, _| stream.context_operation_busy())
        );
    }

    fn drain(&self, cx: &mut TestAppContext) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            cx.background_executor
                .advance_clock(Duration::from_millis(20));
            cx.run_until_parked();
            if self
                .window
                .read_with(cx, |window, _| window.context_controller.manual.is_none())
            {
                break;
            }
            assert!(Instant::now() < deadline, "manual worker did not drain");
            std::thread::yield_now();
        }
    }
}

#[gpui_kit::test]
fn compaction_request_completion_releases_lease_and_applies_terminal_state(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new(cx);
    fixture.request(cx, "gpt-4.1", 1);
    let cancel = fixture
        .started
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    assert!(!cancel.is_cancelled());
    assert!(
        fixture
            .window
            .read_with(cx, |window, _| window.trusted_actions.is_busy())
    );
    assert!(
        fixture
            .stream
            .read_with(cx, |stream, _| stream.context_operation_busy())
    );
    let lease = fixture.window.read_with(cx, |window, _| {
        window.context_controller.manual.as_ref().unwrap().lease
    });
    for request_id in [1, 2] {
        fixture.request(cx, "gpt-4.1", request_id);
        assert!(matches!(
            fixture.started.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
        assert!(
            fixture
                .stream
                .read_with(cx, |stream, _| stream.context_operation_busy())
        );
        fixture.window.read_with(cx, |window, _| {
            let active = window.context_controller.manual.as_ref().unwrap();
            assert_eq!(active.request_id, 1);
            assert_eq!(active.lease, lease);
            assert!(window.trusted_actions.is_busy());
        });
        assert!(!cancel.is_cancelled());
    }
    let mut success = context_record(91, ContextCompactionStatus::Succeeded);
    success.usage = ContextCompactionUsageState::Known { priced: false };
    success.estimated_tokens = Some(120);
    success.source_version = Some(7);
    fixture
        .completion
        .send(Ok(vec![ConversationEvent::ContextCompactionStatus {
            record: success,
        }]))
        .unwrap();
    fixture.drain(cx);
    assert!(
        !fixture
            .window
            .read_with(cx, |window, _| window.trusted_actions.is_busy())
    );
    assert!(
        !fixture
            .stream
            .read_with(cx, |stream, _| stream.context_operation_busy())
    );
    let actual = fixture.status(cx);
    assert_eq!(actual.generation, 1);
    assert_eq!(actual.status, ContextCompactionStatus::Succeeded);
    assert_eq!(actual.failure, None);
    assert_eq!(
        actual.usage,
        ContextCompactionUsageState::Known { priced: false }
    );
    assert_eq!(actual.estimated_tokens, Some(120));
    assert_eq!(actual.source_version, Some(7));
    fixture.stream.update(cx, |stream, cx| {
        assert!(!stream.apply_context_status(
            &thread().id,
            "gpt-4.1",
            context_record(1, ContextCompactionStatus::Compacting),
            cx,
        ));
    });
}

#[gpui_kit::test]
fn cancel_event_reaches_exact_worker_and_keeps_lease_until_drain(cx: &mut TestAppContext) {
    let fixture = Fixture::new(cx);
    fixture.request(cx, "gpt-4.1", 1);
    let cancel = fixture
        .started
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    for id in [2, 1] {
        fixture.stream.update(cx, |_, cx| {
            cx.emit(ContextCompactionCancelRequested {
                thread_id: thread().id,
                model: "gpt-4.1".into(),
                request_id: id,
            })
        });
        cx.run_until_parked();
        assert_eq!(cancel.is_cancelled(), id == 1);
    }
    assert!(
        fixture
            .window
            .read_with(cx, |window, _| window.trusted_actions.is_busy())
    );
    fixture.completion.send(Err(())).unwrap();
    fixture.drain(cx);
    fixture.assert_released(cx);
    let actual = fixture.status(cx);
    assert_eq!(actual.generation, 1);
    assert_eq!(actual.status, ContextCompactionStatus::Cancelled);
    assert_eq!(
        actual.failure,
        Some(ContextCompactionFailureCode::Cancelled)
    );
    assert_eq!(actual.usage, ContextCompactionUsageState::Unknown);
}

#[gpui_kit::test]
fn route_changes_cancel_worker_and_reject_old_completion(cx: &mut TestAppContext) {
    let fixture = Fixture::new(cx);
    for (case, (change_model, return_before_completion)) in
        [(false, false), (false, true), (true, false), (true, true)]
            .into_iter()
            .enumerate()
    {
        fixture.navigate(cx, thread(), &fixture.stream);
        let id = case as u64 * 2 + 1;
        fixture.request(cx, "gpt-4.1", id);
        let cancel = fixture
            .started
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        let old = fixture.active(cx);
        let mut next = thread();
        let next_stream = if change_model {
            next.model = "other-model".into();
            fixture.stream.clone()
        } else {
            next.id = "other-thread".into();
            cx.new(|cx| ConversationStream::new(next.clone(), cx))
        };
        fixture.navigate(cx, next, &next_stream);
        assert!(cancel.is_cancelled());
        assert!(
            fixture
                .window
                .read_with(cx, |window, _| window.trusted_actions.is_busy())
        );
        if return_before_completion {
            fixture.navigate(cx, thread(), &fixture.stream);
            fixture.window.read_with(cx, |window, cx| {
                let current = window.context_controller.owner.as_ref().unwrap();
                assert!(window.context_owner_matches(current, cx));
                assert_ne!(old.owner.epoch, current.epoch);
            });
        }
        fixture
            .completion
            .send(Ok(vec![ConversationEvent::ContextCompactionStatus {
                record: context_record(id, ContextCompactionStatus::Succeeded),
            }]))
            .unwrap();
        fixture.drain(cx);
        fixture.assert_released(cx);
        fixture.stream.read_with(cx, |stream, _| {
            assert!(stream.context_status_snapshot().is_none());
            assert!(
                stream
                    .context_status_rows()
                    .iter()
                    .all(|(_, record)| { record.generation != id })
            );
        });
        if !change_model {
            next_stream.read_with(cx, |stream, _| {
                assert!(stream.context_status_snapshot().is_none());
                assert!(stream.context_status_rows().is_empty());
                assert!(!stream.context_operation_busy());
            });
        }
        if !return_before_completion {
            fixture.navigate(cx, thread(), &fixture.stream);
        }
        fixture.stream.update(cx, |stream, cx| {
            assert!(!stream.apply_context_status(
                &thread().id,
                "gpt-4.1",
                context_record(id, ContextCompactionStatus::Succeeded),
                cx,
            ));
        });
        fixture.request(cx, "gpt-4.1", id + 1);
        let new_cancel = fixture
            .started
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        assert!(!new_cancel.is_cancelled());
        assert_ne!(fixture.active(cx).lease, old.lease);
        let expected = fixture.status(cx);
        fixture.window.update(cx, |window, cx| {
            window.finish_context_compaction(old, Err(()), cx);
        });
        assert_eq!(fixture.status(cx), expected);
        assert!(!new_cancel.is_cancelled());
        fixture.completion.send(Err(())).unwrap();
        fixture.drain(cx);
        fixture.assert_released(cx);
        assert_eq!(fixture.status(cx).generation, id + 1);
        assert_eq!(fixture.status(cx).status, ContextCompactionStatus::Failed);
    }
}

#[gpui_kit::test]
fn retiring_running_row_preserves_newer_context_status(cx: &mut TestAppContext) {
    let fixture = Fixture::new(cx);
    fixture.stream.update(cx, |stream, cx| {
        for id in [1, 2] {
            assert!(stream.apply_context_status(
                &thread().id,
                "gpt-4.1",
                context_record(id, ContextCompactionStatus::Compacting),
                cx,
            ));
        }
        let expected = stream.context_status_snapshot();
        stream.retire_context_operation("other-thread", "gpt-4.1", 2, cx);
        stream.retire_context_operation(&thread().id, "other-model", 2, cx);
        assert_eq!(stream.context_status_rows().len(), 2);
        stream.retire_context_operation(&thread().id, "gpt-4.1", 1, cx);
        assert_eq!(stream.context_status_snapshot(), expected);
        assert!(stream.context_operation_busy());
        let rows = stream.context_status_rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1.generation, 2);
        assert!(stream.apply_context_status(
            &thread().id,
            "gpt-4.1",
            context_record(2, ContextCompactionStatus::Succeeded),
            cx,
        ));
        let expected = stream.context_status_snapshot();
        stream.retire_context_operation(&thread().id, "gpt-4.1", 2, cx);
        assert_eq!(stream.context_status_snapshot(), expected);
        assert_eq!(stream.context_status_rows().len(), 1);
        assert!(!stream.context_operation_busy());
    });
}

#[gpui_kit::test]
fn worker_failure_is_failed_and_allows_a_fresh_request(cx: &mut TestAppContext) {
    let fixture = Fixture::new(cx);
    fixture.request(cx, "gpt-4.1", 1);
    fixture
        .started
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    fixture.completion.send(Err(())).unwrap();
    fixture.drain(cx);
    fixture.assert_released(cx);
    let actual = fixture.status(cx);
    assert_eq!(actual.generation, 1);
    assert_eq!(actual.status, ContextCompactionStatus::Failed);
    assert_eq!(
        actual.failure,
        Some(ContextCompactionFailureCode::Unavailable)
    );
    assert_eq!(actual.usage, ContextCompactionUsageState::Unknown);
    fixture.request(cx, "gpt-4.1", 2);
    assert!(
        !fixture
            .started
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .is_cancelled()
    );
    assert_eq!(
        fixture.status(cx).status,
        ContextCompactionStatus::Compacting
    );
    fixture.completion.send(Err(())).unwrap();
    fixture.drain(cx);
    fixture.assert_released(cx);
    assert_eq!(fixture.status(cx).generation, 2);
}

#[gpui_kit::test]
fn retired_completion_cannot_release_or_update_a_new_run(cx: &mut TestAppContext) {
    let fixture = Fixture::new(cx);
    fixture.request(cx, "gpt-4.1", 1);
    fixture
        .started
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    let old = fixture.active(cx);
    fixture.completion.send(Err(())).unwrap();
    fixture.drain(cx);
    fixture.request(cx, "gpt-4.1", 2);
    let cancel = fixture
        .started
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    let new = fixture.active(cx);
    assert_ne!(old.lease, new.lease);
    let expected = fixture.status(cx);
    for completion in [
        Ok(vec![ConversationEvent::ContextCompactionStatus {
            record: context_record(1, ContextCompactionStatus::Succeeded),
        }]),
        Err(()),
    ] {
        fixture.window.update(cx, |window, cx| {
            window.finish_context_compaction(old.clone(), completion, cx);
        });
        assert_eq!(fixture.active(cx).lease, new.lease);
        assert_eq!(fixture.active(cx).request_id, 2);
        assert!(!cancel.is_cancelled());
        assert_eq!(fixture.status(cx), expected);
        assert!(
            fixture
                .stream
                .read_with(cx, |stream, _| stream.context_operation_busy())
        );
        assert!(
            fixture
                .window
                .read_with(cx, |window, _| window.trusted_actions.is_busy())
        );
    }
    fixture.completion.send(Err(())).unwrap();
    fixture.drain(cx);
    fixture.assert_released(cx);
}

#[gpui_kit::test]
fn competing_trusted_action_lease_is_not_released_by_rejected_compaction(cx: &mut TestAppContext) {
    let fixture = Fixture::new(cx);
    let lease = fixture.window.read_with(cx, |window, _| {
        window
            .trusted_actions
            .acquire(TrustedActionKind::ModelSelection, 0, 8)
            .unwrap()
    });
    fixture.request(cx, "gpt-4.1", 1);
    assert!(matches!(
        fixture.started.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    assert_eq!(fixture.status(cx).status, ContextCompactionStatus::Failed);
    assert_eq!(
        fixture.status(cx).failure,
        Some(ContextCompactionFailureCode::Unavailable)
    );
    fixture.window.read_with(cx, |window, _| {
        assert!(window.trusted_actions.is_busy());
        assert!(window.trusted_actions.release(lease));
    });
    fixture.assert_released(cx);
}

#[gpui_kit::test]
fn wrong_model_request_cannot_start_worker_or_occupy_lease(cx: &mut TestAppContext) {
    let fixture = Fixture::new(cx);
    fixture.request(cx, "other-model", 1);
    assert!(matches!(
        fixture.started.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    assert!(
        !fixture
            .window
            .read_with(cx, |window, _| window.trusted_actions.is_busy())
    );
    assert!(
        !fixture
            .stream
            .read_with(cx, |stream, _| stream.context_operation_busy())
    );
}
