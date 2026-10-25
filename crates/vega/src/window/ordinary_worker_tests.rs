use super::VegaWindow;
use crate::app_agent::{AgentBatch, AgentBatchIngress, PendingAgentRun, UserSubmission};
use base64::Engine;
use futures::FutureExt;
use futures::future::BoxFuture;
use gpui_kit::test::{TestAppContextExt, TestWindowExt};
use gpui_kit::{App, AppContext, ElementId, Entity, Window};
use gpui_kit::{ClipboardItem, Image, ImageFormat, TestAppContext, WindowHandle, px, size};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use vega_conversation::ordinary_submissions::for_thread;
use vega_conversation::types::{CommitFact, PrimaryFact, RetryBlocker, RetryEligibility};
use vega_conversation::types::{ConversationEvent, Thread};
use vega_runtime::{
    ChatRequest, ChatRole, EventStream, Provider, ProviderEvent, StopReason, VegaError,
};
use vega_store::thread_admission::try_acquire;
use vega_ui::conversation_stream::{
    ComposerStopRequested, ComposerSubmitted, ConversationStream, PendingQueueRequested,
};
use vega_ui::sidebar::{OpenedThread, VegaStore};

const DEADLINE: Duration = Duration::from_secs(10);
const MODEL: &str = "gpt-4o";
const FIRST: &str = "first real worker input";
const STEER: &str = "actual running send now";

struct ScriptProvider {
    model: vega_runtime::model_compat::ResolvedModel,
    requests: Arc<Mutex<Vec<ChatRequest>>>,
    auxiliary: Arc<Mutex<Vec<&'static str>>>,
    releases: Vec<CancellationToken>,
}

impl Provider for ScriptProvider {
    fn model_snapshot(&self) -> Option<vega_runtime::model_compat::ResolvedModel> {
        Some(self.model.clone())
    }

    fn chat_stream(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        let requests = self.requests.clone();
        let releases = self.releases.clone();
        Box::pin(async move {
            let index = {
                let mut requests = requests.lock().unwrap();
                let index = requests.len();
                requests.push(request);
                index
            };
            let release = releases.get(index).expect("unexpected primary request");
            tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(VegaError::Cancelled),
                _ = release.cancelled() => {},
                _ = tokio::time::sleep(DEADLINE) => return Err(VegaError::Io(std::io::Error::other("primary gate deadline"))),
            }
            Ok(response(if index == 0 {
                "first answer"
            } else {
                "second answer"
            }))
        })
    }

    fn chat_stream_once(
        &self,
        request: ChatRequest,
        _: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        let system = &request
            .messages
            .first()
            .expect("auxiliary system prompt")
            .content;
        let (kind, answer) = if system.starts_with("Generate a concise conversation title") {
            ("title", "Worker fixture")
        } else if system.starts_with("Predict one useful next user input") {
            ("prediction", r#"{"suggestion":null}"#)
        } else {
            panic!("unrecognized auxiliary request");
        };
        self.auxiliary.lock().unwrap().push(kind);
        Box::pin(async move { Ok(response(answer)) })
    }
}

fn response(text: &str) -> EventStream {
    Box::pin(futures::stream::iter([
        Ok(ProviderEvent::TextDelta(text.into())),
        Ok(ProviderEvent::Done {
            stop_reason: StopReason::End,
        }),
    ]))
}

pub(super) struct WorkerLaunch {
    pub(super) provider: Arc<dyn Provider>,
    script: Arc<ScriptProvider>,
    cancel: Mutex<Option<CancellationToken>>,
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
    hold_poll: AtomicBool,
    poll_waiting: AtomicBool,
    duplicate_batches: Mutex<Vec<AgentBatch>>,
    events: Mutex<Vec<ConversationEvent>>,
    finished: AtomicBool,
    title_watch_drained: AtomicBool,
    poll_drained: AtomicBool,
    record_duplicates: bool,
}

impl WorkerLaunch {
    fn new(model: vega_runtime::model_compat::ResolvedModel, record_duplicates: bool) -> Arc<Self> {
        let script = Arc::new(ScriptProvider {
            model,
            requests: Arc::new(Mutex::new(Vec::new())),
            auxiliary: Arc::new(Mutex::new(Vec::new())),
            releases: vec![CancellationToken::new(), CancellationToken::new()],
        });
        Arc::new(Self {
            provider: script.clone(),
            script,
            cancel: Mutex::new(None),
            worker: Mutex::new(None),
            hold_poll: AtomicBool::new(false),
            poll_waiting: AtomicBool::new(false),
            duplicate_batches: Mutex::new(Vec::new()),
            events: Mutex::new(Vec::new()),
            finished: AtomicBool::new(false),
            title_watch_drained: AtomicBool::new(false),
            poll_drained: AtomicBool::new(false),
            record_duplicates,
        })
    }

    pub(super) fn bind_cancel(&self, cancel: CancellationToken) {
        *self.cancel.lock().unwrap() = Some(cancel);
    }

    pub(super) fn bind_worker(&self, worker: Option<std::thread::JoinHandle<()>>) {
        *self.worker.lock().unwrap() = worker;
    }

    pub(super) fn poll_ready(&self, batch: &AgentBatch) -> bool {
        if self.hold_poll.load(Ordering::SeqCst)
            && (!batch.events.is_empty() || batch.finished.is_some())
        {
            self.poll_waiting.store(true, Ordering::SeqCst);
            false
        } else {
            true
        }
    }

    pub(super) fn observe(&self, batch: &AgentBatch) {
        self.events
            .lock()
            .unwrap()
            .extend(batch.events.iter().cloned());
        if self.record_duplicates && (!batch.events.is_empty() || batch.finished.is_some()) {
            assert!(batch.composer_prediction.is_none());
            assert!(batch.subagents.is_empty());
            self.duplicate_batches.lock().unwrap().push(AgentBatch {
                composer_prediction: None,
                subagents: Vec::new(),
                events: batch.events.clone(),
                codex_mode_confirmed: batch.codex_mode_confirmed,
                computer_use_ready: batch.computer_use_ready.clone(),
                mcp_unavailable: batch.mcp_unavailable.clone(),
                reference_failure: batch.reference_failure,
                credential_failure: batch.credential_failure,
                finished: batch.finished,
            });
        }
        if batch.finished.is_some() {
            self.finished.store(true, Ordering::SeqCst);
        }
    }

    fn primary_count(&self) -> usize {
        self.script.requests.lock().unwrap().len()
    }

    fn cancel_and_release(&self) {
        if let Some(cancel) = self
            .cancel
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
        {
            cancel.cancel();
        }
        for release in &self.script.releases {
            release.cancel();
        }
        self.hold_poll.store(false, Ordering::SeqCst);
    }

    fn worker_finished(&self) -> bool {
        self.worker
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|worker| worker.is_finished())
    }

    fn try_join(&self) -> Result<(), String> {
        let mut slot = self
            .worker
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(worker) = slot.as_ref() {
            let deadline = std::time::Instant::now() + DEADLINE;
            while !worker.is_finished() && std::time::Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(1));
            }
            if !worker.is_finished() {
                return Err("worker join deadline exceeded".into());
            }
        }
        if let Some(worker) = slot.take() {
            worker
                .join()
                .map_err(|error| format!("worker panicked: {}", panic_message(&*error)))?;
        }
        Ok(())
    }

    fn join(&self) {
        self.try_join().unwrap();
    }
}

pub(super) struct TitleWatchDrain(pub(super) Arc<WorkerLaunch>);

impl Drop for TitleWatchDrain {
    fn drop(&mut self) {
        self.0.title_watch_drained.store(true, Ordering::SeqCst);
    }
}

pub(super) struct PollDrain(pub(super) Arc<WorkerLaunch>);

impl Drop for PollDrain {
    fn drop(&mut self) {
        self.0.poll_drained.store(true, Ordering::SeqCst);
    }
}

struct Fixture {
    directory: Option<tempfile::TempDir>,
    safely_drained: bool,
    thread: Thread,
    database: vega_store::thread_admission::DatabaseIdentity,
    controller: Entity<VegaWindow>,
    stream: Entity<ConversationStream>,
    window: WindowHandle<ConversationStream>,
    launches: Vec<Arc<WorkerLaunch>>,
}

impl Fixture {
    fn new(cx: &mut TestAppContext) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let workspace = directory.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let store = vega_store::Store::open(directory.path().join("store.sqlite")).unwrap();
        store.migrate().unwrap();
        let project = vega_store::projects::create(
            store.conn(),
            workspace.to_str().unwrap(),
            "fixture",
            None,
        )
        .unwrap();
        let thread =
            vega_conversation::threads::create_thread(&store, &project.id, MODEL, "confirm")
                .unwrap();
        let database = store.database_identity();
        let config_path = directory.path().join("config/config.toml");
        let config = vega_store::config::AppConfig {
            providers: vec![vega_store::config::ProviderConfig {
                api: vega_store::config::ProviderApi::ChatCompletions,
                enabled: true,
                name: "openai".into(),
                base_url: "https://api.openai.com/v1".into(),
                models: vec![MODEL.into()],
                key_ref: "worker-fixture".into(),
            }],
            ..Default::default()
        };
        config.save_to(&config_path).unwrap();
        vega_store::keystore::set_key(
            config_path.parent().unwrap(),
            "worker-fixture",
            "temporary-worker-credential",
        )
        .unwrap();
        let model = vega_conversation::model_capabilities::load_configured_model(
            &config,
            &config_path,
            "openai",
            MODEL,
        )
        .unwrap();
        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_global(vega_theme::Theme::light());
            cx.set_global(vega_ui::settings::SettingsOpen(false));
            cx.set_global(VegaStore(Ok(store)));
            cx.set_global(vega_ui::sidebar::SelectedProject(None));
            cx.set_global(OpenedThread(Some(thread.clone())));
            cx.set_global(vega_ui::sidebar::SidebarCollapsed(false));
            cx.set_global(vega_ui::sidebar::ProjectsCollapsed(false));
            cx.set_global(vega_ui::sidebar::SessionsCollapsed(false));
            cx.set_global(vega_ui::sidebar::PendingDeleteConfirm(None));
            cx.set_global(vega_ui::sidebar::NewTaskDefaults::default());
            vega_ui::init(cx);
        });
        let controller = cx.update(|cx| {
            cx.new(|cx| {
                let mut controller = VegaWindow::new_with_services(cx, false);
                controller.composer_test_config_path = Some(config_path);
                assert!(controller.ordinary_test_preparations.is_none());
                assert!(controller.agent_test_launches.is_none());
                assert!(controller.mcp_settings.is_none());
                controller
            })
        });
        let window = cx.open_window(size(px(1100.), px(900.)), |_, cx| {
            ConversationStream::new(thread.clone(), cx)
        });
        let stream = window.root(cx).unwrap();
        stream.update(cx, |stream, cx| {
            stream.apply_reasoning_profile(
                vega_conversation::types::ReasoningProfileProjection::from_resolved_model(
                    &model,
                    vega_conversation::types::ReasoningChoice::ProviderDefault,
                ),
                cx,
            )
        });
        controller.update(cx, |controller, cx| {
            controller.stream_view = Some((thread.id.clone(), stream.clone()));
            cx.subscribe(
                &stream,
                |controller, stream, request: &ComposerSubmitted, cx| {
                    controller.submit_composer(stream, request, cx)
                },
            )
            .detach();
            cx.subscribe(
                &stream,
                |controller, stream, request: &ComposerStopRequested, cx| {
                    controller.stop_composer(stream, request, cx)
                },
            )
            .detach();
            cx.subscribe(
                &stream,
                |controller, stream, request: &PendingQueueRequested, cx| {
                    controller.pending_queue_requested(stream, request, cx)
                },
            )
            .detach();
        });
        Self {
            directory: Some(directory),
            safely_drained: false,
            thread,
            database,
            controller,
            stream,
            window,
            launches: Vec::new(),
        }
    }

    fn launch(&mut self, duplicates: bool, cx: &mut TestAppContext) -> Arc<WorkerLaunch> {
        let model = self.stream.read_with(cx, |stream, _| {
            stream.model_snapshot_for_submit().unwrap().unwrap()
        });
        let launch = WorkerLaunch::new(*model, duplicates);
        self.controller.update(cx, |controller, _| {
            controller.worker_test_launches.push_back(launch.clone())
        });
        self.launches.push(launch.clone());
        launch
    }

    fn draft(&self, text: &str, cx: &mut TestAppContext) {
        cx.update_window(self.window.into(), |_, window, cx| {
            self.stream.update(cx, |stream, cx| {
                stream
                    .composer_input()
                    .update(cx, |input, cx| input.set_text(text, cx));
                stream.focus_composer(window, cx);
            });
            window.render_frame(cx);
        })
        .unwrap();
    }

    fn click(&self, id: &str, cx: &mut TestAppContext) {
        cx.update_window(self.window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find(id.to_owned()).visible());
            window.click(id.to_owned(), cx);
        })
        .unwrap();
    }

    fn submit(&self, cx: &mut TestAppContext) {
        cx.update_window(self.window.into(), |_, window, cx| {
            window.press("cmd-enter", cx)
        })
        .unwrap();
    }

    async fn wait_window(
        &self,
        stage: &str,
        window: gpui_kit::AnyWindowHandle,
        cx: &mut TestAppContext,
        predicate: impl FnMut(&mut Window, &mut App) -> bool,
    ) {
        let started = std::time::Instant::now();
        eprintln!("worker fixture wait started: {stage}");
        let result = std::panic::AssertUnwindSafe(cx.wait_for(window, DEADLINE, predicate))
            .catch_unwind()
            .await;
        if result.is_err() {
            eprintln!(
                "worker fixture wait failed: {stage}; wall_elapsed={:?}",
                started.elapsed()
            );
            self.controller.read_with(cx, |controller, _| {
                eprintln!(
                    "active={} preparing={} prediction_pending={}",
                    controller.agent_controller.active.len(),
                    controller.agent_controller.ordinary_preparing.len(),
                    controller.composer_prediction.is_some()
                );
            });
            for (index, launch) in self.launches.iter().enumerate() {
                let (cancel_bound, cancelled) = {
                    let cancel = launch
                        .cancel
                        .lock()
                        .unwrap_or_else(|error| error.into_inner());
                    (
                        cancel.is_some(),
                        cancel.as_ref().is_some_and(CancellationToken::is_cancelled),
                    )
                };
                let (worker_bound, worker_finished) = {
                    let worker = launch
                        .worker
                        .lock()
                        .unwrap_or_else(|error| error.into_inner());
                    (
                        worker.is_some(),
                        worker.as_ref().is_some_and(|worker| worker.is_finished()),
                    )
                };
                eprintln!(
                    "launch={index} primary={} cancel_bound={} cancelled={} worker_bound={} worker_finished={} hold_poll={} poll_waiting={} finished={} poll_drained={} title_drained={} events={}",
                    launch.primary_count(),
                    cancel_bound,
                    cancelled,
                    worker_bound,
                    worker_finished,
                    launch.hold_poll.load(Ordering::SeqCst),
                    launch.poll_waiting.load(Ordering::SeqCst),
                    launch.finished.load(Ordering::SeqCst),
                    launch.poll_drained.load(Ordering::SeqCst),
                    launch.title_watch_drained.load(Ordering::SeqCst),
                    launch.events.lock().unwrap().len()
                );
            }
        }
        if let Err(error) = result {
            std::panic::resume_unwind(error);
        }
        eprintln!(
            "worker fixture wait completed: {stage}; wall_elapsed={:?}",
            started.elapsed()
        );
    }

    async fn wait(
        &self,
        stage: &str,
        cx: &mut TestAppContext,
        predicate: impl Fn(&mut App) -> bool + 'static,
    ) {
        self.wait_window(stage, self.window.into(), cx, move |_, cx| predicate(cx))
            .await;
    }

    async fn primary(&self, launch: &Arc<WorkerLaunch>, count: usize, cx: &mut TestAppContext) {
        let launch = launch.clone();
        self.wait(&format!("primary request count {count}"), cx, move |_| {
            launch.primary_count() == count
        })
        .await;
    }

    async fn cleanup(&mut self, cx: &mut TestAppContext) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        self.safely_drained = false;
        for launch in &self.launches {
            launch.cancel_and_release();
        }
        if let Err(error) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.controller.update(cx, |controller, cx| {
                controller.cancel_composer_prediction(cx);
                for cancel in controller.agent_controller.ordinary_preparing.values() {
                    cancel.cancel();
                }
            });
        })) {
            errors.push(format!(
                "controller cancellation: {}",
                panic_message(&*error)
            ));
        }
        for (index, launch) in self.launches.iter().enumerate() {
            if let Err(error) = launch.try_join() {
                errors.push(format!("worker {index}: {error}"));
            }
        }
        let controller = self.controller.clone();
        let database = self.database;
        let thread = self.thread.id.clone();
        let launches = self.launches.clone();
        let drain = std::panic::AssertUnwindSafe(self.wait(
            "cleanup lifecycle/admission drain",
            cx,
            move |cx| {
                launches.iter().all(|launch| {
                    launch
                        .cancel
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .is_none()
                        || (launch.poll_drained.load(Ordering::SeqCst)
                            && launch.title_watch_drained.load(Ordering::SeqCst))
                }) && controller.read(cx).composer_prediction.is_none()
                    && controller.read(cx).agent_controller.active.is_empty()
                    && controller
                        .read(cx)
                        .agent_controller
                        .ordinary_preparing
                        .is_empty()
                    && try_acquire(database, &thread).is_ok_and(|permit| permit.is_some())
            },
        ))
        .catch_unwind()
        .await;
        match drain {
            Ok(()) => {
                self.safely_drained = self.launches.iter().all(|launch| {
                    launch
                        .worker
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .is_none()
                });
            }
            Err(error) => errors.push(format!(
                "lifecycle/admission drain: {}",
                panic_message(&*error)
            )),
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    async fn drained(&self, launch: &Arc<WorkerLaunch>, cx: &mut TestAppContext) {
        let controller = self.controller.clone();
        let thread = self.thread.id.clone();
        let database = self.database;
        let launch = launch.clone();
        self.wait("finished worker lifecycle/admission drain", cx, move |cx| {
            launch.finished.load(Ordering::SeqCst)
                && launch.poll_drained.load(Ordering::SeqCst)
                && launch.title_watch_drained.load(Ordering::SeqCst)
                && !controller
                    .read(cx)
                    .agent_controller
                    .active
                    .contains_key(&thread)
                && try_acquire(database, &thread).unwrap().is_some()
        })
        .await;
    }

    fn rows(&self, cx: &mut TestAppContext) -> Vec<vega_store::messages::MessageRow> {
        cx.update(|cx| {
            vega_store::context_compaction::load_source(
                cx.global::<VegaStore>().0.as_ref().unwrap().conn(),
                &self.thread.id,
            )
            .unwrap()
            .messages
        })
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        for launch in &self.launches {
            launch.cancel_and_release();
        }
        if !self.safely_drained
            && let Some(directory) = self.directory.take()
        {
            eprintln!(
                "fixture drain not established; retaining {}",
                directory.keep().display()
            );
        }
    }
}

fn panic_message(error: &(dyn std::any::Any + Send)) -> &str {
    error
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| error.downcast_ref::<&str>().copied())
        .unwrap_or("non-string panic payload")
}

fn finish_test(body: std::thread::Result<()>, cleanup: Result<(), Vec<String>>) {
    if let Err(errors) = &cleanup {
        eprintln!("fixture cleanup errors: {}", errors.join("; "));
    }
    if let Err(error) = body {
        std::panic::resume_unwind(error);
    }
    if let Err(errors) = cleanup {
        panic!("fixture cleanup failed: {}", errors.join("; "));
    }
}

fn image() -> vega_conversation::types::ImageAttachment {
    let bytes = base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=").unwrap();
    vega_conversation::types::ImageAttachment::from_bytes(bytes).unwrap()
}

fn user_echo_paths(window: &Window) -> Vec<Vec<ElementId>> {
    let mut paths = gpui_kit::base::test_support::snapshots(window)
        .into_iter()
        .filter(|snapshot| snapshot.label() == Some(vega_i18n::t("ui.copy_message")))
        .map(|snapshot| snapshot.path().to_vec())
        .collect::<Vec<_>>();
    paths.sort_by_cached_key(|path| format!("{path:?}"));
    paths
}

fn attachment_paths(window: &Window) -> Vec<Vec<ElementId>> {
    let mut paths = gpui_kit::base::test_support::snapshots(window)
        .into_iter()
        .filter(|snapshot| {
            snapshot.path().last().is_some_and(
                |id| matches!(id, ElementId::Name(name) if name.starts_with("remove-image-")),
            )
        })
        .map(|snapshot| snapshot.path().to_vec())
        .collect::<Vec<_>>();
    paths.sort_by_cached_key(|path| format!("{path:?}"));
    paths
}

async fn attach(fixture: &Fixture, cx: &mut TestAppContext) {
    let image = image();
    cx.update(|cx| {
        cx.write_to_clipboard(ClipboardItem::new_image(&Image::from_bytes(
            ImageFormat::Png,
            image.bytes().to_vec(),
        )))
    });
    cx.update_window(fixture.window.into(), |_, window, cx| {
        window.press("cmd-v", cx)
    })
    .unwrap();
    fixture
        .wait_window(
            "composer image attachment visible",
            fixture.window.into(),
            cx,
            |window, _| attachment_paths(window).len() == 1,
        )
        .await;
}

#[gpui_kit::test]
async fn ordinary_stop_drains_real_worker_and_duplicate_late_batches_cannot_touch_new_owner(
    cx: &mut TestAppContext,
) {
    let mut fixture = Fixture::new(cx);
    let result = std::panic::AssertUnwindSafe(async {
        let first = fixture.launch(true, cx);
        first.hold_poll.store(true, Ordering::SeqCst);
        fixture.draft(FIRST, cx);
        attach(&fixture, cx).await;
        fixture.submit(cx);
        fixture.primary(&first, 1, cx).await;
        let submission = for_thread(fixture.database, &fixture.thread.id)
            .unwrap()
            .pop()
            .unwrap();
        let snapshot = submission.snapshot();
        let attempt = snapshot.attempts.last().unwrap();
        assert_eq!(attempt.primary, PrimaryFact::Started);
        let CommitFact::Committed(receipt) = &attempt.commit else {
            panic!("real worker must commit before first drive")
        };
        let receipt = receipt.clone();
        assert_eq!(receipt.persisted_text, FIRST);
        assert_eq!(receipt.persisted_images, vec![image()]);
        let old_generation = fixture.controller.read_with(cx, |controller, _| {
            controller.agent_controller.active[&fixture.thread.id].generation
        });
        fixture.click("composer-stop", cx);
        assert!(
            first
                .cancel
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .is_cancelled()
        );
        let waiting = first.clone();
        fixture
            .wait("held poll and worker/title completion", cx, move |_| {
                waiting.poll_waiting.load(Ordering::SeqCst)
                    && waiting.worker_finished()
                    && waiting.title_watch_drained.load(Ordering::SeqCst)
            })
            .await;
        let stopped = submission.snapshot();
        assert_eq!(
            stopped.retry,
            RetryEligibility::Blocked(RetryBlocker::PrimaryStarted)
        );
        assert!(
            submission
                .try_retry(stopped.attempts.last().unwrap().id)
                .is_err()
        );
        assert!(
            try_acquire(fixture.database, &fixture.thread.id)
                .unwrap()
                .is_some()
        );
        first.hold_poll.store(false, Ordering::SeqCst);
        fixture.drained(&first, cx).await;
        first.join();
        let snapshot = submission.snapshot();
        let attempt = snapshot.attempts.last().unwrap();
        assert!(attempt.sealed);
        assert_eq!(attempt.workers, 0);
        assert_eq!(attempt.primary, PrimaryFact::Started);
        assert_eq!(
            snapshot.retry,
            RetryEligibility::Blocked(RetryBlocker::Retired)
        );
        assert!(submission.try_retry(attempt.id).is_err());
        let rows = fixture.rows(cx);
        assert_eq!(rows.iter().filter(|row| row.role == "user").count(), 1);
        assert_eq!(
            rows.iter().find(|row| row.role == "user").unwrap().id,
            receipt.user_message_id
        );
        assert_eq!(
            rows.iter()
                .find(|row| row.id == receipt.assistant_message_id)
                .unwrap()
                .status,
            "interrupted"
        );
        cx.update(|cx| {
            let images = vega_store::image_attachments::for_messages(
                cx.global::<VegaStore>().0.as_ref().unwrap().conn(),
                &fixture.thread.id,
                &rows,
            )
            .unwrap();
            assert_eq!(images.len(), 1);
            assert_eq!(images[0].message_id, receipt.user_message_id);
            assert_eq!(images[0].encoded, image().bytes());
        });
        let second = fixture.launch(false, cx);
        fixture.draft("new owner input", cx);
        fixture.submit(cx);
        fixture.primary(&second, 1, cx).await;
        let new_generation = fixture.controller.read_with(cx, |controller, _| {
            controller.agent_controller.active[&fixture.thread.id].generation
        });
        assert_ne!(old_generation, new_generation);
        fixture.draft("new owner draft", cx);
        attach(&fixture, cx).await;
        let before = fixture.rows(cx);
        let owner_anchor = fixture
            .stream
            .read_with(cx, |stream, _| stream.scroll_anchor_snapshot());
        let owner_attachments = cx
            .update_window(fixture.window.into(), |_, window, cx| {
                window.render_frame(cx);
                attachment_paths(window)
            })
            .unwrap();
        let b_thread = cx.update(|cx| {
            vega_conversation::threads::create_thread(
                cx.global::<VegaStore>().0.as_ref().unwrap(),
                &fixture.thread.project_id,
                MODEL,
                "confirm",
            )
            .unwrap()
        });
        let b_window = cx.open_window(size(px(1100.), px(900.)), |_, cx| {
            ConversationStream::new(b_thread.clone(), cx)
        });
        let b_stream = b_window.root(cx).unwrap();
        b_stream.update(cx, |stream, cx| {
            stream.apply_history_page(
                vega_conversation::history::HistoryPage {
                    subagents: Vec::new(),
                    entries: vec![vega_conversation::history::HistoryEntry::UserText {
                        seq: 1,
                        message_id: "b-history".into(),
                        content: "B existing row".into(),
                    }],
                    older_cursor: None,
                    newer_cursor: None,
                    newest_seq: Some(1),
                },
                cx,
            )
        });
        fixture.controller.update(cx, |controller, cx| {
            controller.stream_view = Some((b_thread.id.clone(), b_stream.clone()));
            cx.set_global(OpenedThread(Some(b_thread.clone())));
        });
        cx.update_window(b_window.into(), |_, window, cx| {
            b_stream.update(cx, |stream, cx| {
                stream
                    .composer_input()
                    .update(cx, |input, cx| input.set_text("B route draft", cx));
                stream.focus_composer(window, cx);
            });
            cx.write_to_clipboard(ClipboardItem::new_image(&Image::from_bytes(
                ImageFormat::Png,
                image().bytes().to_vec(),
            )));
            window.render_frame(cx);
            window.press("cmd-v", cx);
        })
        .unwrap();
        fixture
            .wait_window(
                "other-thread image attachment visible",
                b_window.into(),
                cx,
                |window, _| attachment_paths(window).len() == 1,
            )
            .await;
        let b_anchor = b_stream.read_with(cx, |stream, _| stream.scroll_anchor_snapshot());
        let (b_attachments, b_echoes) = cx
            .update_window(b_window.into(), |_, window, _| {
                (attachment_paths(window), user_echo_paths(window))
            })
            .unwrap();
        assert_eq!(b_echoes.len(), 1);
        let batches = std::mem::take(&mut *first.duplicate_batches.lock().unwrap());
        assert!(batches.iter().any(|batch| batch.finished.is_some()));
        assert!(batches.iter().any(|batch| {
            batch
                .events
                .iter()
                .any(|event| matches!(event, ConversationEvent::MessageStarted { .. }))
        }));
        fixture.controller.update(cx, |controller, cx| {
            for batch in batches {
                assert!(matches!(
                    controller.apply_agent_batch_ingress(
                        old_generation,
                        &fixture.thread.id,
                        &fixture.stream,
                        batch,
                        cx
                    ),
                    AgentBatchIngress::Stale
                ));
            }
            let active = &controller.agent_controller.active[&fixture.thread.id];
            assert_eq!(active.generation, new_generation);
            assert!(!active.cancel.is_cancelled());
            assert_eq!(
                fixture.stream.read(cx).composer_input().read(cx).text(),
                "new owner draft"
            );
        });
        assert!(fixture.rows(cx) == before);
        assert_eq!(
            fixture
                .stream
                .read_with(cx, |stream, _| stream.scroll_anchor_snapshot().identity),
            owner_anchor.identity
        );
        assert_eq!(
            b_stream.read_with(cx, |stream, _| stream.scroll_anchor_snapshot().identity),
            b_anchor.identity
        );
        assert_eq!(
            b_stream.read_with(cx, |stream, cx| stream
                .composer_input()
                .read(cx)
                .text()
                .to_owned()),
            "B route draft"
        );
        assert!(!b_stream.read_with(cx, |stream, _| stream.has_active_agent()));
        cx.update(|cx| {
            assert!(
                vega_store::context_compaction::load_source(
                    cx.global::<VegaStore>().0.as_ref().unwrap().conn(),
                    &b_thread.id
                )
                .unwrap()
                .messages
                .is_empty()
            );
        });
        cx.update_window(b_window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(attachment_paths(window), b_attachments);
            assert_eq!(user_echo_paths(window), b_echoes);
            assert!(window.try_find("composer-stop").is_none());
        })
        .unwrap();
        cx.update_window(fixture.window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find("composer-stop").visible());
            assert_eq!(attachment_paths(window), owner_attachments);
        })
        .unwrap();
        fixture.controller.update(cx, |controller, cx| {
            controller.stream_view = Some((fixture.thread.id.clone(), fixture.stream.clone()));
            cx.set_global(OpenedThread(Some(fixture.thread.clone())));
        });
        fixture.click("composer-stop", cx);
        fixture.drained(&second, cx).await;
        second.join();
        assert_eq!(first.primary_count(), 1);
        assert_eq!(second.primary_count(), 1);
    })
    .catch_unwind()
    .await;
    let cleanup = fixture.cleanup(cx).await;
    finish_test(result, cleanup);
}

#[gpui_kit::test]
async fn running_send_now_is_consumed_by_real_runtime_once_without_ordinary_admission(
    cx: &mut TestAppContext,
) {
    let mut fixture = Fixture::new(cx);
    let result = std::panic::AssertUnwindSafe(async {
    let launch = fixture.launch(false, cx);
    fixture.controller.update(cx, |controller, cx| controller.start_agent_run(fixture.stream.clone(), &fixture.thread.id, PendingAgentRun::UserMessage(UserSubmission { content: FIRST.into(), images: vec![image()], ordinary: None }), cx));
    fixture.primary(&launch, 1, cx).await;
    let first_rows = fixture.rows(cx);
    let old_assistant = first_rows.iter().find(|row| row.role == "assistant").unwrap().id.clone();
    let first_user = first_rows.iter().find(|row| row.role == "user").unwrap().id.clone();
    cx.update(|cx| cx.set_global(vega_ui::conversation_stream::RunningMessageDefault(vega_conversation::types::PendingMessageBehavior::Steer)));
    fixture.draft(STEER, cx);
    attach(&fixture, cx).await;
    fixture.click("composer-running-send", cx);
    let stream = fixture.stream.clone();
    fixture.wait("steering queue accepted and idle", cx, move |cx| !stream.read(cx).pending_queue().busy && stream.read(cx).pending_queue().messages.len() == 1).await;
    let pending = fixture.stream.read_with(cx, |stream, _| stream.pending_queue().messages[0].clone());
    assert_eq!(pending.content, STEER);
    assert_eq!(pending.images, vec![image()]);
    assert_eq!(pending.behavior, vega_conversation::types::PendingMessageBehavior::Steer);
    assert_eq!(fixture.stream.read_with(cx, |stream, _| stream.projected_pending_ids()), vec![pending.id.clone()]);
    assert!(for_thread(fixture.database, &fixture.thread.id).unwrap().is_empty());
    fixture.controller.update(cx, |controller, cx| {
        assert!(controller.agent_controller.ordinary_preparing.is_empty());
        assert!(controller.agent_controller.ordinary_requests.is_empty());
        assert!(controller.agent_controller.active[&fixture.thread.id].ordinary_attempt.is_none());
        assert!(controller.ordinary_test_preparations.is_none());
        assert!(controller.agent_test_launches.is_none());
        let durable = vega_conversation::pending_messages::load(cx.global::<VegaStore>().0.as_ref().unwrap(), &fixture.thread.id).unwrap();
        assert!(durable == vec![pending.clone()]);
    });
    launch.script.releases[0].cancel();
    fixture.primary(&launch, 2, cx).await;
    let events = launch.clone();
    fixture.wait("pending delivery event observed", cx, move |_| events.events.lock().unwrap().iter().any(|event| matches!(event, ConversationEvent::PendingMessageDelivered { .. }))).await;
    let requests = launch.script.requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 2);
    assert!(!requests[0].messages.iter().any(|message| message.role == ChatRole::User && message.content == STEER));
    let steers = requests[1].messages.iter().filter(|message| message.role == ChatRole::User && message.content == STEER).collect::<Vec<_>>();
    assert_eq!(steers.len(), 1);
    assert_eq!(steers[0].images, vec![image()]);
    let next_assistant = {
    let events = launch.events.lock().unwrap().clone();
    let continued = events.iter().filter_map(|event| match event {
        ConversationEvent::RunContinued { previous_assistant_id, user_message_id, next_assistant_id } => Some((previous_assistant_id, user_message_id, next_assistant_id)),
        _ => None,
    }).collect::<Vec<_>>();
    assert_eq!(continued.len(), 1);
    assert_eq!(continued[0].0, &old_assistant);
    assert_eq!(continued[0].1, &pending.id);
    let next_assistant = continued[0].2.to_owned();
    assert_eq!(events.iter().filter(|event| matches!(event, ConversationEvent::PendingMessageDelivered { pending_id, message_id, images, .. } if pending_id == &pending.id && message_id == &pending.id && images == &vec![image()])).count(), 1);
    assert_eq!(events.iter().filter(|event| matches!(event, ConversationEvent::MessageStarted { message_id, .. } if message_id == &next_assistant)).count(), 1);
    next_assistant
    };
    fixture.stream.update(cx, |stream, cx| {
        assert!(stream.reveal_loaded_message(&pending.id, cx));
        assert!(stream.projected_pending_ids().is_empty());
    });
    cx.update_window(fixture.window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(user_echo_paths(window).len(), 2);
        assert!(window.find("composer-stop").visible());
    }).unwrap();
    launch.script.releases[1].cancel();
    fixture.drained(&launch, cx).await;
    launch.join();
    let rows = fixture.rows(cx);
    let users = rows.iter().filter(|row| row.role == "user").collect::<Vec<_>>();
    assert_eq!(users.len(), 2);
    assert_eq!(users[0].id, first_user);
    assert_eq!(users[1].id, pending.id);
    assert_eq!(users[1].content, STEER);
    assert_eq!(rows.iter().find(|row| row.id == old_assistant).unwrap().status, "done");
    assert_eq!(rows.iter().find(|row| row.id == old_assistant).unwrap().content, "first answer");
    assert_eq!(rows.iter().find(|row| row.id == next_assistant).unwrap().status, "done");
    assert_eq!(rows.iter().find(|row| row.id == next_assistant).unwrap().content, "second answer");
    cx.update(|cx| {
        let store = cx.global::<VegaStore>().0.as_ref().unwrap();
        assert!(vega_conversation::pending_messages::load(store, &fixture.thread.id).unwrap().is_empty());
        let images = vega_store::image_attachments::for_messages(store.conn(), &fixture.thread.id, &rows).unwrap();
        assert_eq!(images.len(), 2);
        assert_eq!(images.iter().filter(|image| image.message_id == first_user).count(), 1);
        assert_eq!(images.iter().filter(|image| image.message_id == pending.id).count(), 1);
        assert!(images.iter().all(|row| row.encoded == image().bytes()));
    });
    assert!(for_thread(fixture.database, &fixture.thread.id).unwrap().is_empty());
    assert_eq!(launch.primary_count(), 2);
    cx.update_window(fixture.window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(user_echo_paths(window).len(), 2);
        assert!(window.try_find("composer-stop").is_none());
    }).unwrap();
    assert!(launch.script.auxiliary.lock().unwrap().contains(&"title"));
    }).catch_unwind().await;
    let cleanup = fixture.cleanup(cx).await;
    finish_test(result, cleanup);
}

#[gpui_kit::test]
async fn body_panic_cleanup_releases_held_real_worker_poll(cx: &mut TestAppContext) {
    let mut fixture = Fixture::new(cx);
    let launch = fixture.launch(false, cx);
    launch.hold_poll.store(true, Ordering::SeqCst);
    let body = std::panic::AssertUnwindSafe(async {
        fixture.draft(FIRST, cx);
        fixture.submit(cx);
        fixture.primary(&launch, 1, cx).await;
        panic!("intentional worker body failure");
    })
    .catch_unwind()
    .await;
    let cleanup = fixture.cleanup(cx).await;
    assert!(cleanup.is_ok(), "{cleanup:?}");
    assert!(fixture.safely_drained);
    assert!(!launch.hold_poll.load(Ordering::SeqCst));
    assert!(
        launch
            .cancel
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .is_cancelled()
    );
    assert!(launch.worker.lock().unwrap().is_none());
    assert!(launch.poll_drained.load(Ordering::SeqCst));
    assert!(launch.title_watch_drained.load(Ordering::SeqCst));
    assert!(
        try_acquire(fixture.database, &fixture.thread.id)
            .unwrap()
            .is_some()
    );
    let resumed =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| finish_test(body, cleanup)));
    assert_eq!(
        panic_message(&*resumed.unwrap_err()),
        "intentional worker body failure"
    );
}

#[gpui_kit::test]
async fn held_admission_cleanup_reports_failure_and_preserves_body_panic(cx: &mut TestAppContext) {
    let mut fixture = Fixture::new(cx);
    let admission = try_acquire(fixture.database, &fixture.thread.id)
        .unwrap()
        .unwrap();
    let body = std::panic::catch_unwind(|| panic!("original body failure"));
    let cleanup = fixture.cleanup(cx).await;
    assert!(
        cleanup
            .as_ref()
            .unwrap_err()
            .iter()
            .any(|error| error.contains("lifecycle/admission drain"))
    );
    assert!(!fixture.safely_drained);
    let path = fixture.directory.as_ref().unwrap().path().to_owned();
    assert!(path.exists());
    let resumed =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| finish_test(body, cleanup)));
    assert_eq!(
        panic_message(&*resumed.unwrap_err()),
        "original body failure"
    );
    drop(admission);
    assert!(fixture.cleanup(cx).await.is_ok());
    assert!(fixture.safely_drained);
}
