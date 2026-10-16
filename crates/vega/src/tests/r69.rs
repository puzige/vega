//! R69 acceptance (docs/vega-r69-home-lazy-draft-composer.md §5).
//!
//! Every case drives the real production route: a real [`VegaWindow`] over an
//! owned temp store, the real `ConversationStream`, and the real
//! `ComposerSubmitted` → `VegaWindow::submit_composer` path. No test-only seam
//! decides whether the draft materializes; the seams used here are the same
//! owned config/provider overrides the existing R1/R57 fixtures already use.
//!
//! A2/A3/A4 are the core evidence: they turn "lazy" from "looks about right"
//! into falsifiable assertions — A2 proves nothing is written before submit,
//! A3 proves materialization reuses the draft id, A4 proves submit does not
//! rebuild the stream.

use super::model_selection::model_selection_config;
use super::*;
use gpui_kit::{
    Bounds, Modifiers, Pixels, VisualTestContext, WindowBounds, WindowOptions, px, size,
};
use tokio_util::sync::CancellationToken;
use vega_ui::sidebar::SelectedProject;

struct StreamingObservationProvider {
    inner: Arc<vega_runtime::MockProvider>,
    release: CancellationToken,
    waiting: CancellationToken,
}

struct StreamingReleaseGuard(CancellationToken);

impl StreamingReleaseGuard {
    fn release(&self) {
        self.0.cancel();
    }
}

impl Drop for StreamingReleaseGuard {
    fn drop(&mut self) {
        self.release();
    }
}

impl StreamingObservationProvider {
    fn new(inner: Arc<vega_runtime::MockProvider>) -> (Arc<Self>, StreamingReleaseGuard) {
        let release = CancellationToken::new();
        (
            Arc::new(Self {
                inner,
                release: release.clone(),
                waiting: CancellationToken::new(),
            }),
            StreamingReleaseGuard(release),
        )
    }
}

impl vega_runtime::Provider for StreamingObservationProvider {
    fn chat_stream(
        &self,
        request: vega_runtime::ChatRequest,
        cancel: CancellationToken,
    ) -> futures::future::BoxFuture<
        'static,
        Result<vega_runtime::EventStream, vega_runtime::VegaError>,
    > {
        use futures::StreamExt;
        let future =
            vega_runtime::Provider::chat_stream(self.inner.as_ref(), request, cancel.clone());
        let release = self.release.clone();
        let waiting = self.waiting.clone();
        Box::pin(async move {
            let stream = future.await?;
            let gated = futures::stream::unfold(
                (stream, Some(release), false, false),
                move |(mut stream, mut gate, observed_text, finished)| {
                    let cancel = cancel.clone();
                    let waiting = waiting.clone();
                    async move {
                        if finished {
                            return None;
                        }
                        if observed_text && let Some(release) = gate.take() {
                            waiting.cancel();
                            let released = tokio::select! {
                                biased;
                                _ = cancel.cancelled() => false,
                                _ = release.cancelled() => true,
                            };
                            if !released {
                                return Some((
                                    Err(vega_runtime::VegaError::Cancelled),
                                    (stream, None, true, true),
                                ));
                            }
                        }
                        stream.next().await.map(|event| {
                            let observed_text = observed_text
                                || matches!(&event, Ok(vega_runtime::ProviderEvent::TextDelta(text)) if !text.is_empty());
                            (event, (stream, gate, observed_text, false))
                        })
                    }
                },
            );
            Ok(Box::pin(gated) as vega_runtime::EventStream)
        })
    }
}

struct StreamingGateWakeProbe(AtomicBool);

impl futures::task::ArcWake for StreamingGateWakeProbe {
    fn wake_by_ref(probe: &Arc<Self>) {
        probe.0.store(true, Ordering::SeqCst);
    }
}

/// Owned project + config + window at the production viewport. The window is
/// mounted exactly like `r45_mount_project_window`, but the home route is the
/// subject, so the fixture leaves `OpenedThread` empty (the draft is installed
/// by the first frame).
struct DraftFixture {
    _repo: ControllerRepo,
    _config_root: TempDir,
    config_path: std::path::PathBuf,
    data_root: TempDir,
    database_path: std::path::PathBuf,
    project_id: String,
    second_project_id: Option<String>,
    root: Entity<VegaWindow>,
    window: gpui_kit::WindowHandle<VegaWindow>,
    provider: Arc<vega_runtime::MockProvider>,
}

impl DraftFixture {
    fn open_with_repo(
        cx: &mut gpui_kit::TestAppContext,
        with_project: bool,
        repo: ControllerRepo,
    ) -> Self {
        Self::open_with_repo_and_model(cx, with_project, repo, None)
    }

    fn open_with_repo_and_model(
        cx: &mut gpui_kit::TestAppContext,
        with_project: bool,
        repo: ControllerRepo,
        unpriced_model: Option<&str>,
    ) -> Self {
        Self::open_with_registration(cx, with_project, with_project, repo, None, unpriced_model)
    }

    fn open_with_registration(
        cx: &mut gpui_kit::TestAppContext,
        register_project: bool,
        select_project: bool,
        repo: ControllerRepo,
        second_repo: Option<&std::path::Path>,
        unpriced_model: Option<&str>,
    ) -> Self {
        let config_root = tempfile::tempdir().expect("r69 config root");
        let config_path = config_root.path().join("config.toml");
        model_selection_config(&config_path);
        if let Some(model) = unpriced_model {
            let mut config =
                vega_store::config::read_from(&config_path).expect("r69 unpriced config");
            config.providers[0].models.push(model.to_owned());
            config.defaults.model = model.to_owned();
            config
                .save_to(&config_path)
                .expect("r69 unpriced config save");
        }
        vega_store::keystore::set_key(config_root.path(), "owned", "r69-test-key")
            .expect("r69 test credential");
        let data_root = tempfile::tempdir().expect("r69 data root");
        let database_path = data_root.path().join("vega.db");
        let store = Store::open(&database_path).expect("r69 store");
        store.migrate().expect("r69 migrations");
        let project_id = if register_project {
            vega_store::projects::create(
                store.conn(),
                repo.path().to_str().expect("UTF-8 r69 repo"),
                "r69-draft-e2e",
                None,
            )
            .expect("r69 project")
            .id
        } else {
            String::new()
        };
        let second_project_id = second_repo.map(|path| {
            vega_store::projects::create(
                store.conn(),
                path.to_str().expect("UTF-8 a8 second repo"),
                "a8-second-project",
                None,
            )
            .expect("a8 second project")
            .id
        });

        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_global(Theme::light());
            cx.set_global(SettingsOpen(false));
            cx.set_global(SidebarCollapsed(false));
            cx.set_global(SidebarWidth(Layout::SIDEBAR_WIDTH));
            cx.set_global(PendingDeleteConfirm(None));
            cx.set_global(vega_ui::sidebar::ProjectsCollapsed(false));
            cx.set_global(vega_ui::sidebar::SessionsCollapsed(false));
            cx.set_global(VegaStore(Ok(store)));
            cx.set_global(SelectedProject(select_project.then(|| project_id.clone())));
            // The home route: no opened thread, so the window installs its draft.
            cx.set_global(OpenedThread(None));
            vega_ui::init(cx);
        });
        let root = cx.new(VegaWindow::new);
        let provider = Arc::new(vega_runtime::MockProvider::new(vec![
            vega_runtime::ScriptStep::events(vec![
                vega_runtime::ProviderEvent::TextDelta("r69 ok".into()),
                vega_runtime::ProviderEvent::Done {
                    stop_reason: vega_runtime::StopReason::End,
                },
            ]),
        ]));
        root.update(cx, |root, _| {
            root.model_selection_config_override = Some(config_path.clone());
            root.agent_provider_override = Some(with_auxiliary_title_fixture(provider.clone()));
        });
        let window_root = root.clone();
        let window = cx.update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(1403.), px(860.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                move |_, _| window_root,
            )
            .expect("r69 root window")
        });
        // Match main.rs: Settings' Back button dispatches the app-level
        // CloseSettings action through these production shortcut bindings.
        cx.update(|cx| crate::app_palette::bind_shortcuts(window.into(), root.downgrade(), cx));
        cx.run_until_parked();
        Self {
            _repo: repo,
            _config_root: config_root,
            config_path,
            data_root,
            database_path,
            project_id,
            second_project_id,
            root,
            window,
            provider,
        }
    }

    fn home(cx: &mut gpui_kit::TestAppContext, with_project: bool) -> Self {
        Self::home_with_repo(cx, with_project, diff_controller_repo())
    }

    fn home_with_repo(
        cx: &mut gpui_kit::TestAppContext,
        with_project: bool,
        repo: ControllerRepo,
    ) -> Self {
        let fixture = Self::open_with_repo(cx, with_project, repo);
        pump_test_app(cx, |cx| {
            fixture.root.read_with(cx, |root, _| {
                root.draft.is_some()
                    && root.stream_view.is_some()
                    && !root.model_catalog_loading
                    && root.configured_models.is_some()
            })
        });
        fixture
    }

    fn home_with_project_choices(
        cx: &mut gpui_kit::TestAppContext,
        select_project: bool,
        second_repo: Option<&std::path::Path>,
    ) -> Self {
        let fixture = Self::open_with_registration(
            cx,
            true,
            select_project,
            diff_controller_repo(),
            second_repo,
            None,
        );
        pump_test_app(cx, |cx| {
            fixture.root.read_with(cx, |root, _| {
                root.draft.is_some()
                    && root.stream_view.is_some()
                    && !root.model_catalog_loading
                    && root.configured_models.is_some()
            })
        });
        fixture
    }

    fn store(&self) -> Store {
        Store::open(&self.database_path).expect("r69 observer store")
    }

    /// The project's durable row count — the A2/A3 subject.
    fn thread_rows(&self) -> i64 {
        self.store()
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM threads WHERE project_id = ?1",
                [&self.project_id],
                |row| row.get(0),
            )
            .expect("r69 thread count")
    }

    fn standalone_rows(&self) -> i64 {
        self.store()
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM threads WHERE project_id IS NULL",
                [],
                |row| row.get(0),
            )
            .expect("r69 standalone count")
    }

    fn message_rows(&self, thread_id: &str) -> i64 {
        self.store()
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE thread_id = ?1",
                [thread_id],
                |row| row.get(0),
            )
            .expect("r69 message count")
    }

    fn draft(&self, cx: &mut gpui_kit::TestAppContext) -> Thread {
        self.root
            .read_with(cx, |root, _| root.draft.clone())
            .expect("home route installs a draft")
    }

    fn stream(&self, cx: &mut gpui_kit::TestAppContext) -> Entity<ConversationStream> {
        self.root
            .read_with(cx, |root, _| {
                root.stream_view.as_ref().map(|(_, stream)| stream.clone())
            })
            .expect("home route mounts a stream")
    }

    fn input(&self, cx: &mut gpui_kit::TestAppContext) -> Entity<vega_ui::text_input::TextInput> {
        self.stream(cx)
            .read_with(cx, |stream, _| stream.composer_input())
    }

    fn bounds(&self, selector: &'static str, cx: &mut gpui_kit::TestAppContext) -> Bounds<Pixels> {
        cx.run_until_parked();
        gpui_kit::VisualTestContext::from_window(self.window.into(), cx)
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("missing r69 selector: {selector}"))
    }

    fn absent(&self, selector: &'static str, cx: &mut gpui_kit::TestAppContext) -> bool {
        cx.run_until_parked();
        gpui_kit::VisualTestContext::from_window(self.window.into(), cx)
            .debug_bounds(selector)
            .is_none()
    }

    fn click(&self, selector: &'static str, cx: &mut gpui_kit::TestAppContext) {
        let bounds = self.bounds(selector, cx);
        let mut visual = VisualTestContext::from_window(self.window.into(), cx);
        visual.simulate_click(bounds.center(), Modifiers::default());
        visual.run_until_parked();
    }

    fn project_row_selector(project_id: &str) -> &'static str {
        Box::leak(format!("composer-utility-project-row-{project_id}").into_boxed_str())
    }

    /// Types into the real composer and submits through the production key
    /// binding (`cmd-enter` → `SendMessage` → `submit_message` →
    /// `ComposerSubmitted` → `VegaWindow::submit_composer`).
    fn submit(&self, text: &str, cx: &mut gpui_kit::TestAppContext) {
        let input = self.input(cx);
        input.update(cx, |input, cx| input.set_text(text, cx));
        let focus = input.read_with(cx, |input, cx| input.focus_handle(cx));
        self.window
            .update(cx, |_, window, cx| window.focus(&focus, cx))
            .expect("r69 composer focus");
        cx.simulate_keystrokes(self.window.into(), "cmd-enter");
        let stream = self.stream(cx);
        pump_test_app(cx, |cx| {
            self.root.read_with(cx, |root, _| {
                root.draft.is_none()
                    || stream.read_with(cx, |stream, _| !stream.composer_submission_pending())
            })
        });
    }

    fn save_config(&self, mutate: impl FnOnce(&mut vega_store::config::AppConfig)) {
        let mut config =
            vega_store::config::read_from(&self.config_path).expect("r69 preflight config");
        mutate(&mut config);
        config
            .save_to(&self.config_path)
            .expect("r69 preflight config save");
    }

    fn disable_provider(&self) {
        self.save_config(|config| config.providers[0].enabled = false);
    }

    fn remove_model(&self) {
        self.save_config(|config| config.providers[0].models.clear());
    }

    fn set_malformed_base_url(&self) {
        self.save_config(|config| config.providers[0].base_url = "not-a-url".to_owned());
    }

    fn add_ambiguous_provider(&self) {
        self.save_config(|config| {
            let mut duplicate = config.providers[0].clone();
            duplicate.name = "second".to_owned();
            duplicate.key_ref = "second".to_owned();
            config.providers.push(duplicate);
        });
        vega_store::keystore::set_key(
            self.config_path.parent().expect("r69 config parent"),
            "second",
            "r69-ambiguous-key",
        )
        .expect("r69 ambiguous test credential");
    }

    fn remove_credential(&self) {
        vega_store::keystore::delete_key(
            self.config_path.parent().expect("r69 config parent"),
            "owned",
        )
        .expect("r69 delete test credential");
    }

    fn repair_provider(&self) {
        let service = vega_conversation::ProviderSettingsService::new(self.config_path.clone());
        let current = service.load().expect("r69 settings provider");
        let provider = current.providers[0].clone();
        if !provider.enabled {
            service
                .patch(ProviderPatchRequest {
                    provider: provider.clone(),
                    action: ProviderPatchAction::SetEnabled(true),
                })
                .expect("r69 settings provider enable");
        }
        let repaired = service
            .load()
            .expect("r69 settings provider reload")
            .providers[0]
            .clone();
        service
            .save_provider(
                Some(repaired.clone()),
                repaired,
                Some("r69-repaired-key".to_owned()),
            )
            .expect("r69 settings credential repair");
    }

    fn wait_for_preflight_error(&self, cx: &mut gpui_kit::TestAppContext) -> String {
        let stream = self.stream(cx);
        pump_test_app(cx, |cx| {
            stream.read_with(cx, |stream, _| {
                !stream.composer_submission_pending() && stream.controller_error_message().is_some()
            })
        });
        stream.read_with(cx, |stream, _| {
            stream.controller_error_message().expect("preflight error")
        })
    }

    fn assert_rejected_before_start(
        &self,
        draft_id: &str,
        text: &str,
        cx: &mut gpui_kit::TestAppContext,
    ) {
        let stream = self.stream(cx);
        assert_eq!(self.thread_rows(), 0);
        assert_eq!(self.standalone_rows(), 0);
        assert_eq!(self.draft(cx).id, draft_id);
        assert_eq!(
            self.input(cx)
                .read_with(cx, |input, _| input.text().to_string()),
            text
        );
        assert_eq!(
            cx.update(|cx| cx
                .global::<OpenedThread>()
                .0
                .as_ref()
                .map(|thread| thread.id.clone())),
            Some(draft_id.to_owned())
        );
        assert!(!stream.read_with(cx, |stream, _| stream.composer_submission_pending()));
        assert_eq!(
            self.root
                .read_with(cx, |root, _| root.agent_worker_start_probe.load()),
            0
        );
        assert!(self.provider.requests().is_empty());
    }
}
