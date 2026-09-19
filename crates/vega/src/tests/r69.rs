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
use vega_conversation::types::{PermissionMode, ThreadMode, ThreadStatus};
use vega_ui::branch_selector::BranchListRequested;
use vega_ui::conversation_stream::{ComposerDefaultsRequested, ThreadSettingsRequested};
use vega_ui::sidebar::SelectedProject;

/// Issue 63: actual clipboard → lazy standalone Composer → preflight → worker
/// → durable message → provider boundary. Only network/provider is replaced.
#[gpui_kit::test]
async fn issue63_standalone_first_image_submit_crosses_app_worker_and_persists(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.executor().allow_parking();
    let f = DraftFixture::home(cx, false);
    let input = f.input(cx);
    let focus = input.read_with(cx, |input, cx| input.focus_handle(cx));
    f.window
        .update(cx, |_, window, cx| window.focus(&focus, cx))
        .expect("image composer focus");
    let bytes = include_bytes!("../../../../assets/logo/raster/vega-icon-f1-original.png").to_vec();
    cx.update(|cx| {
        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_image(
            &gpui_kit::Image::from_bytes(gpui_kit::ImageFormat::Png, bytes.clone()),
        ))
    });
    cx.dispatch_action(f.window.into(), vega_ui::text_input::Paste);
    cx.run_until_parked();
    assert!(!f.absent("composer-attachments", cx));
    assert_eq!(f.standalone_rows(), 0);
    f.disable_provider();
    f.submit("", cx);
    assert!(!f.wait_for_preflight_error(cx).is_empty());
    assert!(!f.absent("composer-attachments", cx));
    assert_eq!(f.standalone_rows(), 0);
    assert!(f.provider.requests().is_empty());
    f.repair_provider();
    f.submit("", cx);
    pump_test_app(cx, |cx| {
        !f.provider.requests().is_empty()
            && f.root
                .read_with(cx, |root, _| root.agent_controller.active.is_none())
    });
    assert_eq!(f.standalone_rows(), 1);
    let requests = f.provider.requests();
    let user = requests[0]
        .messages
        .iter()
        .find(|message| message.role == vega_runtime::ChatRole::User)
        .expect("image user message");
    assert_eq!(user.images.len(), 1);
    assert_eq!(user.images[0].bytes(), bytes);
    assert_eq!(user.content, "");
    let count: i64 = f
        .store()
        .conn()
        .query_row("SELECT COUNT(*) FROM image_attachments", [], |row| {
            row.get(0)
        })
        .expect("durable image count");
    assert_eq!(count, 1);
    assert!(f.absent("composer-attachments", cx));
}

/// Owned project + config + window at the production viewport. The window is
/// mounted exactly like `r45_mount_project_window`, but the home route is the
/// subject, so the fixture leaves `OpenedThread` empty (the draft is installed
/// by the first frame).
struct DraftFixture {
    _repo: TempDir,
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
        repo: TempDir,
    ) -> Self {
        Self::open_with_repo_and_model(cx, with_project, repo, None)
    }

    fn open_with_repo_and_model(
        cx: &mut gpui_kit::TestAppContext,
        with_project: bool,
        repo: TempDir,
        unpriced_model: Option<&str>,
    ) -> Self {
        Self::open_with_registration(cx, with_project, with_project, repo, None, unpriced_model)
    }

    fn open_with_registration(
        cx: &mut gpui_kit::TestAppContext,
        register_project: bool,
        select_project: bool,
        repo: TempDir,
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
        repo: TempDir,
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

/// A1: the home route renders the real composer (`composer-shell`) and its
/// input accepts text — no click required.
#[gpui_kit::test]
async fn r69_a1_home_route_renders_a_usable_composer(cx: &mut gpui_kit::TestAppContext) {
    let f = DraftFixture::home(cx, true);
    let composer = f.bounds("composer-shell", cx);
    assert!(
        f32::from(composer.size.height) >= Layout::COMPOSER_MIN_HEIGHT,
        "home composer keeps its 100px minimum"
    );
    // R13: the R49 utility bar renders normally on the draft route.
    let bar = f.bounds("composer-utility-bar", cx);
    assert_eq!(
        f32::from(bar.size.height),
        Layout::COMPOSER_UTILITY_BAR_HEIGHT
    );
    // R17: the frozen bottom row is mounted.
    f.bounds("composer-add", cx);
    f.bounds("composer-model", cx);
    f.bounds("composer-send", cx);

    let input = f.input(cx);
    input.update(cx, |input, cx| {
        input.set_text("typed on the home route", cx)
    });
    assert_eq!(
        input.read_with(cx, |input, _| input.text().to_string()),
        "typed on the home route",
        "the home composer is a real editable input"
    );
}

/// A2 (core): typing on the home route writes nothing to the store (R5).
#[gpui_kit::test]
async fn r69_a2_typing_on_the_home_route_writes_no_row(cx: &mut gpui_kit::TestAppContext) {
    let f = DraftFixture::home(cx, true);
    let before = f.thread_rows();
    assert_eq!(before, 0, "the fixture starts with no durable task");

    let input = f.input(cx);
    input.update(cx, |input, cx| {
        input.set_text("a draft that is never sent", cx)
    });
    cx.run_until_parked();
    // Give any stray worker a chance to land a write before asserting.
    pump_test_app(cx, |cx| {
        f.root.read_with(cx, |root, _| {
            !root.model_catalog_loading && root.configured_models.is_some()
        })
    });
    assert_eq!(
        f.thread_rows(),
        before,
        "typing on the home route must not INSERT a threads row (R5)"
    );
    assert_eq!(f.standalone_rows(), 0);
    assert_eq!(
        input.read_with(cx, |input, _| input.text().to_string()),
        "a draft that is never sent"
    );
}

/// A3 (core): first submit writes exactly one row whose id is the draft id
/// captured before submit (R8/R9).
#[gpui_kit::test]
async fn r69_a3_first_submit_materializes_the_draft_under_its_own_id(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = DraftFixture::home(cx, true);
    let draft = f.draft(cx);
    assert_eq!(f.thread_rows(), 0, "no row before submit");

    f.submit("materialize me", cx);

    let store = f.store();
    let rows = vega_store::threads::list_by_project(store.conn(), &f.project_id, None)
        .expect("r69 durable rows");
    assert_eq!(rows.len(), 1, "first submit writes exactly one row");
    assert_eq!(
        rows[0].id, draft.id,
        "materialization must reuse the draft's own id (no alias layer)"
    );
    // R2: every other field is the one the draft carried. The store returns
    // DDL strings, so compare against the same vocabulary the draft encodes.
    assert_eq!(rows[0].project_id, draft.project_id);
    // #65 R1: durable first submission now installs its fallback atomically.
    assert_eq!(rows[0].title, "materialize me");
    assert_eq!(rows[0].model, draft.model);
    assert_eq!(rows[0].permission_mode, draft.permission_mode.as_str());
    assert_eq!(rows[0].mode, ThreadMode::Execute.as_str());
    assert_eq!(rows[0].status, ThreadStatus::Active.as_str());
    assert!(!rows[0].pinned);
    assert!(!rows[0].unread);
    // R9: the route identity did not change, and the window no longer holds a
    // draft — so a repeated submit cannot write a second row (R11).
    let opened = cx.update(|cx| cx.global::<OpenedThread>().0.clone());
    assert_eq!(
        opened.as_ref().map(|thread| thread.id.clone()),
        Some(draft.id.clone())
    );
    assert!(f.root.read_with(cx, |root, _| root.draft.is_none()));
    // R69 M4: the durable row's `created_at` is the submit instant. The draft
    // was constructed earlier (when the home route first rendered), so a stale
    // value would be visibly earlier than the draft's own timestamp.
    assert!(
        rows[0].created_at >= draft.created_at,
        "the durable row is stamped at submit, never before the draft"
    );
    // `updated_at` is last-activity, so the run that follows materialization
    // legitimately bumps it past `created_at`; only the ordering is fixed.
    assert!(rows[0].updated_at >= rows[0].created_at);
    assert_eq!(
        f.thread_rows(),
        1,
        "exactly one row: no duplicate from the submit path"
    );
}

/// A4 (core): materialization does not rebuild the cached stream — the
/// `stream_view` key and entity identity are unchanged (R9).
#[gpui_kit::test]
async fn r69_a4_submit_does_not_rebuild_the_cached_stream(cx: &mut gpui_kit::TestAppContext) {
    let f = DraftFixture::home(cx, true);
    let draft = f.draft(cx);
    let stream = f.stream(cx);
    let key_before = f.root.read_with(cx, |root, _| {
        root.stream_view.as_ref().map(|(id, _)| id.clone())
    });
    assert_eq!(key_before, Some(draft.id.clone()));
    let entity_before = stream.entity_id();

    f.submit("do not rebuild me", cx);

    let key_after = f.root.read_with(cx, |root, _| {
        root.stream_view.as_ref().map(|(id, _)| id.clone())
    });
    assert_eq!(
        key_after, key_before,
        "the stream_view cache key must not change across materialization"
    );
    let stream_after = f.stream(cx);
    assert_eq!(
        stream_after.entity_id(),
        entity_before,
        "the cached ConversationStream entity must not be rebuilt"
    );
    assert!(
        f.root.read_with(cx, |root, cx| root.owns_stream_request(
            &stream_after,
            &draft.id,
            cx
        )),
        "owns_stream_request must keep holding for the same entity and id"
    );
    // The run must have started on this same entity. A rebuilt entity would
    // have failed `owns_stream_request` above, so the run reaching the provider
    // is what proves the identity chain held end to end. This is asserted
    // without a wall-clock wait: `pump_test_app`'s bounded retry is only
    // reliable for the async stages that settle on their own, and the
    // keystroke-driven submit here is the timing-sensitive step (AGENTS.md:
    // synthetic key events cannot reliably drive GPUI's focus chain).
    // A4 stops here on purpose. The R9 claim — materialization does not rebuild
    // the cached stream — is fully proven by the three deterministic assertions
    // above (cache key, entity id, `owns_stream_request`).
    //
    // Waiting for the agent run to reach the provider is NOT asserted: probing
    // showed the run is still in flight at this point (`reqs=0 rows=1
    // draft_held=false` — i.e. materialization already succeeded and the draft
    // was released). That wait measures the pre-existing agent-run worker under
    // parallel test load, not R69, and it flaked ~2/12 here while the identity
    // assertions never did. Provider reachability is covered by the agent-run
    // tests (`crates/vega/src/tests/agent.rs`) and by A3, which asserts the
    // durable row that materialization must write.
}

/// A5: the draft route surfaces no controller error (R7). The hydration block
/// that would report `NotFound` for a missing row never runs.
#[gpui_kit::test]
async fn r69_a5_draft_route_has_no_controller_error(cx: &mut gpui_kit::TestAppContext) {
    let f = DraftFixture::home(cx, true);
    let stream = f.stream(cx);
    pump_test_app(cx, |cx| {
        f.root.read_with(cx, |root, _| {
            !root.model_catalog_loading && root.configured_models.is_some()
        })
    });
    assert_eq!(
        stream.read_with(cx, |stream, _| stream.controller_error_message()),
        None,
        "a draft must not render a hydration NotFound error bar (R7)"
    );
    // And it stays clean across a re-render of the same route.
    cx.update(|cx| cx.refresh_windows());
    cx.run_until_parked();
    assert_eq!(
        stream.read_with(cx, |stream, _| stream.controller_error_message()),
        None
    );
}

/// A6: model / permission changes on the draft update memory and
/// `OpenedThread` without writing a row (R5).
#[gpui_kit::test]
async fn r69_a6_draft_settings_update_memory_without_a_write(cx: &mut gpui_kit::TestAppContext) {
    let f = DraftFixture::home(cx, true);
    let draft = f.draft(cx);
    let stream = f.stream(cx);
    let before = f.thread_rows();

    // The permission path goes through the real `+`-menu intent
    // (`request_permission_mode` → `ThreadSettingsRequested`) and the real
    // `persist_thread_settings` handler.
    let draft_id = draft.id.clone();
    stream.update(cx, |_, cx| {
        cx.emit(ThreadSettingsRequested {
            thread_id: draft_id.clone(),
            mode: None,
            permission_mode: Some(PermissionMode::ReadOnly),
        });
    });
    cx.run_until_parked();

    let opened = cx
        .update(|cx| cx.global::<OpenedThread>().0.clone())
        .expect("draft stays installed");
    assert_eq!(
        opened.permission_mode,
        PermissionMode::ReadOnly,
        "the draft's permission change lands in OpenedThread"
    );
    assert_eq!(opened.id, draft.id, "the draft id is stable (R4)");
    assert_eq!(
        f.root
            .read_with(cx, |root, _| root.draft.as_ref().map(|d| d.permission_mode)),
        Some(PermissionMode::ReadOnly),
        "the window's draft mirrors the accepted projection so the INSERT carries it"
    );
    assert_eq!(
        f.thread_rows(),
        before,
        "a draft permission change must not write a row (R5)"
    );
    assert_eq!(
        stream.read_with(cx, |stream, _| stream.controller_error_message()),
        None,
        "an accepted in-memory settings change is not an error"
    );

    // Materialization then carries the accepted value into the row (R2/R8).
    f.submit("permission carried", cx);
    let store = f.store();
    let rows =
        vega_store::threads::list_by_project(store.conn(), &f.project_id, None).expect("r69 rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].permission_mode,
        PermissionMode::ReadOnly.as_str(),
        "the materialized row keeps the draft's in-memory permission change"
    );
}

/// A6 companion: the thinking preference path is in-memory on the draft too.
#[gpui_kit::test]
async fn r69_a6b_draft_thinking_update_writes_no_row(cx: &mut gpui_kit::TestAppContext) {
    let f = DraftFixture::home(cx, true);
    let draft = f.draft(cx);
    let stream = f.stream(cx);
    let before = f.thread_rows();
    let (defaults_before, model_before) = stream.read_with(cx, |stream, _| {
        (
            stream.thinking_choice().to_string(),
            stream.displayed_model().to_string(),
        )
    });

    // Read the model before entering the update closure: GPUI forbids
    // re-reading an entity while it is already being updated.
    stream.update(cx, |_, cx| {
        cx.emit(ComposerDefaultsRequested {
            thread_id: draft.id.clone(),
            defaults: vega_conversation::types::ComposerDefaults {
                model: model_before.clone(),
                thinking: defaults_before.clone(),
                reasoning: None,
                reasoning_unavailable: false,
            },
        });
    });
    cx.run_until_parked();

    assert_eq!(
        f.thread_rows(),
        before,
        "a draft thinking change must not write a row (R5)"
    );
    assert_eq!(
        cx.update(|cx| cx
            .global::<OpenedThread>()
            .0
            .as_ref()
            .map(|thread| thread.id.clone())),
        Some(draft.id.clone()),
        "the draft route is unchanged"
    );
}

/// A7: a project-bound draft keeps artifact access absent but can use the
/// existing branch controller against the selected project's real repository.
#[gpui_kit::test]
async fn r69_a7_project_draft_lists_and_switches_without_materializing(
    cx: &mut gpui_kit::TestAppContext,
) {
    let repo = artifact_controller_repo();
    run_fixture_git(repo.path(), &["branch", "r69-draft-target"]);
    let f = DraftFixture::home_with_repo(cx, true, repo);
    let input = f.input(cx);
    input.update(cx, |input, cx| {
        input.set_text("draft text survives branch switching", cx)
    });
    cx.run_until_parked();
    assert_eq!(f.thread_rows(), 0, "the draft starts without a durable row");

    pump_test_app(cx, |cx| {
        f.root.read_with(cx, |root, _| {
            !root.model_catalog_loading && root.configured_models.is_some()
        })
    });
    assert!(
        f.root
            .read_with(cx, |root, _| root.artifact_controller.active.is_none()),
        "ensure_artifact_route must not begin on the draft route (R6)"
    );
    assert!(
        f.root
            .read_with(cx, |root, _| root.branch_controller.active.is_some()),
        "a project-bound draft must begin the existing branch route"
    );
    let selector = f
        .stream(cx)
        .read_with(cx, |stream, _| stream.branch_selector());
    let trigger = f.bounds("composer-utility-branch-chip", cx);
    {
        let mut visual = VisualTestContext::from_window(f.window.into(), cx);
        visual.simulate_click(trigger.center(), Modifiers::default());
        visual.run_until_parked();
    }
    pump_test_app(cx, |cx| {
        selector.read_with(cx, |selector, _| selector.snapshot_generation().is_some())
    });
    assert_eq!(
        f.thread_rows(),
        0,
        "listing branches must not materialize the draft"
    );
    let (target_index, target_label) = selector
        .read_with(cx, |selector, _| {
            selector
                .visible_rows(0..64)
                .into_iter()
                .find(|(_, branch)| branch.label == "r69-draft-target")
                .map(|(index, branch)| (index, branch.label))
        })
        .expect("the production branch list contains the owned target");
    let row_selector: &'static str =
        Box::leak(format!("branch-row-{target_index}").into_boxed_str());
    let row = f.bounds(row_selector, cx);
    {
        let mut visual = VisualTestContext::from_window(f.window.into(), cx);
        visual.simulate_click(row.center(), Modifiers::default());
        visual.run_until_parked();
    }
    pump_test_app(cx, |cx| {
        selector.read_with(cx, |selector, _| !selector.is_pending())
    });
    let head = fixture_git_command(f._repo.path(), &["symbolic-ref", "--short", "HEAD"])
        .output()
        .expect("read owned draft repository HEAD");
    assert!(head.status.success());
    assert_eq!(
        String::from_utf8_lossy(&head.stdout).trim(),
        target_label,
        "branch switching uses the existing controller and owned repository"
    );
    assert_eq!(
        f.thread_rows(),
        0,
        "branch switching must not materialize draft"
    );
    assert_eq!(
        f.input(cx)
            .read_with(cx, |input, _| input.text().to_string()),
        "draft text survives branch switching",
        "branch switching must preserve composer text"
    );
    assert_eq!(
        selector.read_with(cx, |selector, _| selector.pending_key()),
        None,
        "the existing switch controller clears its operation"
    );
    // R6 corollary: artifact-only review stays unoffered; the branch chip is
    // the deliberate project-bound draft exception covered above.
    assert!(
        f.absent("environment-review", cx),
        "the draft route must not offer the Changes/Review entry"
    );
    // Project context itself still renders (the folder row and the R49 bar).
    f.bounds("environment-project", cx);
    f.bounds("composer-utility-bar", cx);
}

/// A8: with a project selected the R49 utility bar is visible on the draft
/// route (R13) at its frozen geometry.
#[gpui_kit::test]
async fn r69_a8_draft_route_renders_the_utility_bar(cx: &mut gpui_kit::TestAppContext) {
    let f = DraftFixture::home(cx, true);
    let card = f.bounds("composer-shell", cx);
    let bar = f.bounds("composer-utility-bar", cx);
    assert_eq!(
        f32::from(bar.size.height),
        Layout::COMPOSER_UTILITY_BAR_HEIGHT,
        "utility bar keeps its frozen height"
    );
    assert!(
        (f32::from(bar.left() - card.left()) - Layout::COMPOSER_UTILITY_BAR_INSET).abs() <= 1.0,
        "utility bar keeps its frozen left inset"
    );
    assert!(
        (f32::from(bar.bottom()) - f32::from(card.top())).abs() <= 1.0,
        "utility bar stays flush against the card top"
    );
    f.bounds("composer-utility-project-chip", cx);
    f.bounds("composer-utility-branch-chip", cx);
}

/// A9: the main header reads `新建任务` on the draft route (R12), not
/// `未命名任务`.
#[gpui_kit::test]
async fn r69_a9_draft_route_header_reads_new_task(cx: &mut gpui_kit::TestAppContext) {
    let f = DraftFixture::home(cx, true);
    let draft = f.draft(cx);
    assert!(draft.title.is_empty(), "a draft starts with an empty title");
    assert_eq!(
        f.root.read_with(cx, |root, cx| root.main_header_title(cx)),
        "新建任务",
        "the draft route must not read 未命名任务 (R12)"
    );
    // The header node itself is mounted at its frozen band.
    let header = f.bounds("main-header", cx);
    assert_eq!(
        f32::from(header.size.height),
        Layout::MAIN_HEADER_HEIGHT,
        "R16: the header geometry is untouched"
    );
    // Control: a durable untitled task still reads 未命名任务.
    let store = f.store();
    let durable =
        vega_conversation::threads::create_thread(&store, &f.project_id, "mock", "confirm")
            .expect("r69 durable control thread");
    cx.update(|cx| cx.set_global(OpenedThread(Some(durable))));
    cx.run_until_parked();
    assert_eq!(
        f.root.read_with(cx, |root, cx| root.main_header_title(cx)),
        "未命名任务",
        "a durable untitled task keeps the existing title"
    );
}

/// A10: type on the home route, navigate away, come back — the text survives
/// (R4: the draft id is stable so the draft cache is not stranded).
#[gpui_kit::test]
async fn r69_a10_draft_text_survives_leaving_and_returning(cx: &mut gpui_kit::TestAppContext) {
    let f = DraftFixture::home(cx, true);
    let draft_id = f.draft(cx).id;
    let input = f.input(cx);
    input.update(cx, |input, cx| {
        input.set_text("keep me across navigation", cx)
    });
    cx.run_until_parked();

    let store = f.store();
    let durable =
        vega_conversation::threads::create_thread(&store, &f.project_id, "mock", "confirm")
            .expect("r69 navigation destination");
    // Leave: open a durable task. The window keeps its single draft id.
    cx.update(|cx| cx.set_global(OpenedThread(Some(durable.clone()))));
    cx.run_until_parked();
    assert!(f.root.read_with(cx, |root, _| root.draft.is_some()));

    // Return: the home route reuses the same draft id.
    cx.update(|cx| cx.set_global(OpenedThread(None)));
    pump_test_app(cx, |cx| {
        f.root.read_with(cx, |root, _| {
            root.stream_view
                .as_ref()
                .is_some_and(|(id, _)| id == &draft_id)
        })
    });
    assert_eq!(
        f.root
            .read_with(cx, |root, _| root.draft.as_ref().map(|d| d.id.clone())),
        Some(draft_id.clone()),
        "re-entering the home route reuses the same draft id (R4)"
    );
    assert_eq!(
        f.input(cx)
            .read_with(cx, |input, _| input.text().to_string()),
        "keep me across navigation",
        "the draft text keyed by the stable id is restored"
    );
    assert_eq!(f.thread_rows(), 1, "navigation wrote only the durable task");
}

/// A11: with no project selected the home route still renders a real, usable
/// composer (R15), which materializes as a standalone task.
#[gpui_kit::test]
async fn r69_a11_no_project_home_route_is_still_a_usable_composer(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = DraftFixture::home(cx, false);
    let draft = f.draft(cx);
    assert!(
        draft.is_standalone(),
        "no project selection → standalone draft"
    );
    assert!(
        f.root
            .read_with(cx, |root, _| root.branch_controller.active.is_none()),
        "a standalone draft must not create a branch controller"
    );

    let composer = f.bounds("composer-shell", cx);
    assert!(f32::from(composer.size.height) >= Layout::COMPOSER_MIN_HEIGHT);
    // A8-01 supersedes R15: the project choice is in the real Composer,
    // with no competing folder-picker prompt on the home route.
    f.bounds("composer-utility-bar", cx);
    f.bounds("composer-utility-project-chip", cx);
    assert!(f.absent("composer-utility-branch-chip", cx));
    assert!(f.absent("home-guidance-add-project", cx));
    let input = f.input(cx);
    input.update(cx, |input, cx| input.set_text("standalone draft", cx));
    assert_eq!(
        input.read_with(cx, |input, _| input.text().to_string()),
        "standalone draft"
    );

    // It materializes as a standalone task under the same id.
    f.submit("standalone submit", cx);
    let store = f.store();
    let standalone =
        vega_store::threads::list_standalone(store.conn(), Some(ThreadStatus::Active.as_str()))
            .expect("r69 standalone rows");
    assert_eq!(standalone.len(), 1);
    assert_eq!(standalone[0].id, draft.id);
    assert_eq!(standalone[0].project_id, "");
}

/// A8-01: registered folders are reachable from a project-less draft's real
/// Composer. Choosing one rebinds the cached stream without materializing a
/// task or replacing its input/focus, and first submit uses that final choice.
#[gpui_kit::test]
async fn a8_unbound_home_draft_chooses_registered_project_in_composer(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = DraftFixture::home_with_project_choices(cx, false, None);
    let draft = f.draft(cx);
    let stream = f.stream(cx);
    let input = f.input(cx);
    assert!(draft.is_standalone());
    assert_eq!(
        cx.update(|cx| cx.global::<SelectedProject>().0.clone()),
        None
    );
    f.bounds("composer-shell", cx);
    f.bounds("composer-utility-bar", cx);
    f.bounds("composer-utility-project-chip", cx);
    assert!(f.absent("composer-utility-branch-chip", cx));
    assert!(f.absent("home-guidance-add-project", cx));

    input.update(cx, |input, cx| {
        input.set_text("choose a registered project", cx)
    });
    let focus = input.read_with(cx, |input, cx| input.focus_handle(cx));
    f.window
        .update(cx, |_, window, cx| window.focus(&focus, cx))
        .expect("a8 composer focus");
    f.click("composer-utility-project-chip", cx);
    f.bounds("composer-utility-project-menu", cx);
    let row = DraftFixture::project_row_selector(&f.project_id);
    f.bounds(row, cx);
    f.click(row, cx);

    pump_test_app(cx, |cx| {
        f.root.read_with(cx, |root, _| {
            root.draft
                .as_ref()
                .is_some_and(|draft| draft.project_id == f.project_id)
                && root
                    .branch_controller
                    .active
                    .as_ref()
                    .is_some_and(|active| active.identity.project_id == f.project_id)
        })
    });
    assert_eq!(f.draft(cx).id, draft.id);
    assert_eq!(f.stream(cx).entity_id(), stream.entity_id());
    assert_eq!(f.input(cx).entity_id(), input.entity_id());
    assert_eq!(
        input.read_with(cx, |input, _| input.text().to_owned()),
        "choose a registered project"
    );
    assert!(
        f.window
            .update(cx, |_, window, _| focus.is_focused(window))
            .expect("a8 focus state"),
        "project choice returns focus to the same Composer input"
    );
    assert_eq!(
        stream.read_with(cx, |stream, _| stream.route_project_id().to_owned()),
        f.project_id
    );
    f.bounds("composer-utility-branch-chip", cx);
    assert_eq!(f.thread_rows(), 0);
    assert_eq!(f.standalone_rows(), 0);

    f.submit("choose a registered project", cx);
    let rows = vega_store::threads::list_by_project(f.store().conn(), &f.project_id, None)
        .expect("a8 chosen-project task rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, draft.id);
    assert_eq!(rows[0].project_id, f.project_id);
    assert_eq!(f.standalone_rows(), 0);
}

/// A8-01: A→B is one draft, not a label-only switch. The very next branch
/// request must carry B's project id, and B is the sole durable binding.
#[gpui_kit::test]
async fn a8_draft_switches_a_to_b_and_branches_only_in_b(cx: &mut gpui_kit::TestAppContext) {
    let second_repo = artifact_controller_repo();
    let f = DraftFixture::home_with_project_choices(cx, true, Some(second_repo.path()));
    let second = f.second_project_id.clone().expect("a8 B project");
    let draft = f.draft(cx);
    let stream = f.stream(cx);
    let selector = stream.read_with(cx, |stream, _| stream.branch_selector());
    let input = f.input(cx);
    input.update(cx, |input, cx| input.set_text("project B work", cx));
    // The existing draft settings path owns the permission choice; project
    // switching must carry it, and the initial model/mode, into the INSERT.
    stream.update(cx, |_, cx| {
        cx.emit(ThreadSettingsRequested {
            thread_id: draft.id.clone(),
            mode: None,
            permission_mode: Some(PermissionMode::ReadOnly),
        });
    });
    cx.run_until_parked();
    assert_eq!(f.draft(cx).permission_mode, PermissionMode::ReadOnly);

    f.click("composer-utility-project-chip", cx);
    f.click(DraftFixture::project_row_selector(&second), cx);
    pump_test_app(cx, |cx| {
        f.root.read_with(cx, |root, _| {
            root.branch_controller
                .active
                .as_ref()
                .is_some_and(|active| active.identity.project_id == second)
        })
    });
    assert_eq!(f.draft(cx).id, draft.id);
    assert_eq!(f.draft(cx).project_id, second);
    assert_eq!(f.draft(cx).permission_mode, PermissionMode::ReadOnly);
    assert_eq!(f.draft(cx).mode, draft.mode);
    assert_eq!(f.draft(cx).model, draft.model);
    assert_eq!(f.stream(cx).entity_id(), stream.entity_id());
    assert_eq!(f.input(cx).entity_id(), input.entity_id());
    assert_eq!(
        input.read_with(cx, |input, _| input.text().to_owned()),
        "project B work"
    );
    assert_eq!(
        selector.read_with(cx, |selector, _| selector.route().1.to_owned()),
        second
    );

    let requests = Arc::new(Mutex::new(Vec::<BranchListRequested>::new()));
    let observed = requests.clone();
    cx.update(|cx| {
        cx.subscribe(&selector, move |_, request: &BranchListRequested, _| {
            observed
                .lock()
                .expect("a8 branch requests")
                .push(request.clone());
        })
        .detach();
    });
    f.click("composer-utility-branch-chip", cx);
    assert!(selector.read_with(cx, |selector, _| selector.is_open()));
    assert_eq!(requests.lock().expect("a8 immediate requests").len(), 1);
    pump_test_app(cx, |cx| {
        selector.read_with(cx, |selector, _| selector.snapshot_generation().is_some())
    });
    let requests = requests.lock().expect("a8 observed requests");
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].thread_id, draft.id);
    assert_eq!(requests[0].project_id, second);
    drop(requests);
    selector.update(cx, |selector, cx| {
        assert!(selector.request_close(cx));
    });
    cx.run_until_parked();
    assert_eq!(f.thread_rows(), 0);
    assert_eq!(f.standalone_rows(), 0);

    f.submit("project B work", cx);
    let store = f.store();
    let rows =
        vega_store::threads::list_by_project(store.conn(), &second, None).expect("a8 B task rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, draft.id);
    assert_eq!(rows[0].project_id, second);
    assert_eq!(rows[0].permission_mode, PermissionMode::ReadOnly.as_str());
    assert_eq!(rows[0].mode, draft.mode.as_str());
    assert_eq!(rows[0].model, draft.model);
    assert_eq!(f.thread_rows(), 0, "A receives no task row");
}

/// A8-01: detaching the same A→B draft clears project-scoped controller and
/// popup state, yet keeps the Composer and writes exactly one standalone row.
#[gpui_kit::test]
async fn a8_draft_switches_a_to_b_to_detached_without_stale_branch(
    cx: &mut gpui_kit::TestAppContext,
) {
    let second_repo = artifact_controller_repo();
    let f = DraftFixture::home_with_project_choices(cx, true, Some(second_repo.path()));
    let second = f.second_project_id.clone().expect("a8 B project");
    let draft = f.draft(cx);
    let stream = f.stream(cx);
    let selector = stream.read_with(cx, |stream, _| stream.branch_selector());
    let input = f.input(cx);
    input.update(cx, |input, cx| input.set_text("detached work", cx));

    f.click("composer-utility-project-chip", cx);
    f.click(DraftFixture::project_row_selector(&second), cx);
    pump_test_app(cx, |cx| f.draft(cx).project_id == second);
    f.click("composer-utility-branch-chip", cx);
    pump_test_app(cx, |cx| {
        selector.read_with(cx, |selector, _| selector.snapshot_generation().is_some())
    });
    assert!(selector.read_with(cx, |selector, _| selector.is_open()));
    f.click("composer-utility-project-chip", cx);
    assert!(!selector.read_with(cx, |selector, _| selector.is_open()));
    f.bounds("composer-utility-project-menu", cx);
    assert!(f.absent("branch-selector-popup", cx));
    f.click("composer-utility-project-detach", cx);

    pump_test_app(cx, |cx| {
        f.draft(cx).is_standalone()
            && f.root
                .read_with(cx, |root, _| root.branch_controller.active.is_none())
    });
    assert_eq!(f.draft(cx).id, draft.id);
    assert_eq!(f.stream(cx).entity_id(), stream.entity_id());
    assert_eq!(f.input(cx).entity_id(), input.entity_id());
    assert_eq!(
        input.read_with(cx, |input, _| input.text().to_owned()),
        "detached work"
    );
    assert_eq!(
        selector.read_with(cx, |selector, _| selector.route().1.to_owned()),
        ""
    );
    assert_eq!(
        stream
            .read_with(cx, |stream, _| stream.commit_panel())
            .read_with(cx, |panel, _| panel.route().1.to_owned()),
        ""
    );
    f.bounds("composer-utility-bar", cx);
    f.bounds("composer-utility-project-chip", cx);
    assert!(f.absent("composer-utility-branch-chip", cx));
    assert!(f.absent("composer-utility-project-menu", cx));
    assert!(f.absent("branch-selector-popup", cx));
    assert_eq!(f.thread_rows(), 0);
    assert_eq!(f.standalone_rows(), 0);

    f.submit("detached work", cx);
    let rows =
        vega_store::threads::list_standalone(f.store().conn(), Some(ThreadStatus::Active.as_str()))
            .expect("a8 standalone task rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, draft.id);
    assert_eq!(rows[0].project_id, "");
    assert_eq!(f.thread_rows(), 0);
}

/// A8-01 does not turn the shared selection chip into a durable task move.
/// An existing empty session can still expose the older R49 bar, but choosing
/// another project changes only navigation selection, never its stored row.
#[gpui_kit::test]
async fn a8_existing_empty_task_keeps_its_durable_project_binding(
    cx: &mut gpui_kit::TestAppContext,
) {
    let second_repo = artifact_controller_repo();
    let f = DraftFixture::home_with_project_choices(cx, true, Some(second_repo.path()));
    let second = f.second_project_id.clone().expect("a8 B project");
    let store = f.store();
    let committed = vega_conversation::threads::create_thread(
        &store,
        &f.project_id,
        "gpt-5.6-terra",
        PermissionMode::Confirm.as_str(),
    )
    .expect("a8 existing task");
    cx.update(|cx| {
        cx.set_global(OpenedThread(Some(committed.clone())));
        cx.refresh_windows();
    });
    cx.run_until_parked();
    f.bounds("composer-utility-bar", cx);
    f.click("composer-utility-project-chip", cx);
    f.click(DraftFixture::project_row_selector(&second), cx);

    assert_eq!(
        cx.update(|cx| cx.global::<SelectedProject>().0.clone()),
        Some(second.clone())
    );
    let opened = cx
        .update(|cx| cx.global::<OpenedThread>().0.clone())
        .expect("a8 durable route still open");
    assert_eq!(opened.id, committed.id);
    assert_eq!(opened.project_id, f.project_id);
    let stored = vega_store::threads::find(store.conn(), &committed.id)
        .expect("a8 durable read")
        .expect("a8 durable row");
    assert_eq!(stored.project_id, f.project_id);
    assert_eq!(f.thread_rows(), 1);
    assert_eq!(
        vega_store::threads::list_by_project(store.conn(), &second, None)
            .expect("a8 B rows")
            .len(),
        0
    );
    assert_eq!(f.message_rows(&committed.id), 0);
    assert!(f.absent("composer-utility-bar", cx));
}

/// A12: the sidebar's [新建任务] entry lands on the draft route without
/// writing a row (R14). This is the entry point that used to pile up empty
/// `未命名任务` rows.
#[gpui_kit::test]
async fn r69_a12_sidebar_new_task_opens_the_draft_without_a_row(cx: &mut gpui_kit::TestAppContext) {
    let f = DraftFixture::home(cx, true);
    let before = f.thread_rows();
    let draft_before = f.draft(cx).id;

    f.window
        .update(cx, |_, window, cx| {
            window.dispatch_action(Box::new(vega_ui::sidebar::NewThread), cx)
        })
        .expect("r69 cmd-n dispatch");
    pump_test_app(cx, |cx| {
        f.root.read_with(cx, |root, _| {
            root.draft.is_some() && root.stream_view.is_some()
        })
    });

    assert_eq!(
        f.thread_rows(),
        before,
        "the sidebar new-task entry must not INSERT a row (R14)"
    );
    let draft_after = f.draft(cx).id;
    assert_eq!(
        draft_after, draft_before,
        "re-entering the home route reuses the same draft id (R4)"
    );
    assert_eq!(
        f.root.read_with(cx, |root, cx| root.main_header_title(cx)),
        "新建任务"
    );
}

/// A13: a failed materialization keeps the draft and its text, surfaces the
/// error, and leaves no half-written row (R10).
#[gpui_kit::test]
async fn r69_a13_materialization_failure_preserves_the_draft(cx: &mut gpui_kit::TestAppContext) {
    let f = DraftFixture::home(cx, true);
    let draft = f.draft(cx);
    let stream = f.stream(cx);
    let input = f.input(cx);
    input.update(cx, |input, cx| {
        input.set_text("must survive the failure", cx)
    });
    cx.run_until_parked();

    // The store global is the production failure seam: `materialize_draft`
    // reads it exactly like every other conversation-layer write.
    cx.update(|cx| {
        cx.set_global(VegaStore(Err("owned unavailable database".into())));
    });
    f.submit("must survive the failure", cx);

    assert!(
        f.root.read_with(cx, |root, _| root
            .draft
            .as_ref()
            .is_some_and(|d| d.id == draft.id)),
        "a failed materialization keeps the draft installed (R10)"
    );
    assert_eq!(
        cx.update(|cx| cx
            .global::<OpenedThread>()
            .0
            .as_ref()
            .map(|thread| thread.id.clone())),
        Some(draft.id.clone()),
        "the route identity is unchanged by the failure"
    );
    assert_eq!(
        input.read_with(cx, |input, _| input.text().to_string()),
        "must survive the failure",
        "the draft text is preserved for retry"
    );
    assert!(
        stream.read_with(cx, |stream, _| stream.controller_error_message().is_some()),
        "the failure is surfaced instead of being swallowed"
    );
    assert!(
        !stream.read_with(cx, |stream, _| stream.has_pending_permission()),
        "no run started, so no half-written run state exists"
    );
    assert_eq!(
        f.thread_rows(),
        0,
        "no half-written row: the store was unavailable"
    );

    // Retrying with a working store succeeds and writes exactly one row.
    cx.update(|cx| {
        cx.set_global(VegaStore(Ok(
            Store::open(&f.database_path).expect("r69 retry store")
        )));
    });
    f.submit("must survive the failure", cx);
    assert_eq!(f.thread_rows(), 1, "the retry writes exactly one row");
    let store = f.store();
    let rows = vega_store::threads::list_by_project(store.conn(), &f.project_id, None)
        .expect("r69 retry rows");
    assert_eq!(rows[0].id, draft.id);
    let _ = f.data_root.path();
    let _ = f.provider.requests();
}

/// A7-01: a disabled provider is rejected in the production submit path
/// before the lazy draft is materialized or an agent worker starts.
#[gpui_kit::test]
async fn a7_first_submit_disabled_provider_rejects_before_materialization(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = DraftFixture::home(cx, true);
    let draft_id = f.draft(cx).id;
    f.disable_provider();

    f.submit("disabled provider draft", cx);
    let error = f.wait_for_preflight_error(cx);
    f.assert_rejected_before_start(&draft_id, "disabled provider draft", cx);
    assert!(error.contains("gpt-5.6-terra"));
    assert!(error.contains("owned"));
    assert!(error.contains("设置 → Providers"));
    assert!(!error.contains("执行未完成"));
}

/// A7-01: removing the selected model from the enabled-provider catalog is a
/// typed configuration rejection, not a network/agent failure.
#[gpui_kit::test]
async fn a7_first_submit_unavailable_model_rejects_before_materialization(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = DraftFixture::home(cx, true);
    let draft_id = f.draft(cx).id;
    f.remove_model();

    f.submit("unavailable model draft", cx);
    let error = f.wait_for_preflight_error(cx);
    f.assert_rejected_before_start(&draft_id, "unavailable model draft", cx);
    assert!(error.contains("gpt-5.6-terra"));
    assert!(error.contains("没有可用的供应商"));
    assert!(error.contains("设置 → Providers"));
    assert!(!error.contains("执行未完成"));
}

/// A7-01: two enabled providers claiming the selected model are ambiguous and
/// must not be allowed to materialize a draft without a unique authority.
#[gpui_kit::test]
async fn a7_first_submit_ambiguous_provider_rejects_before_materialization(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = DraftFixture::home(cx, true);
    let draft_id = f.draft(cx).id;
    f.add_ambiguous_provider();

    f.submit("ambiguous provider draft", cx);
    let error = f.wait_for_preflight_error(cx);
    f.assert_rejected_before_start(&draft_id, "ambiguous provider draft", cx);
    assert!(error.contains("gpt-5.6-terra"));
    assert!(error.contains("owned"));
    assert!(error.contains("second"));
    assert!(error.contains("设置 → Providers"));
    assert!(!error.contains("执行未完成"));
}

/// A7-01: a missing owner-only credential keeps the exact draft editable and
/// uses the credential-repair guidance before any durable row or run exists.
#[gpui_kit::test]
async fn a7_first_submit_missing_credential_rejects_before_materialization(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = DraftFixture::home(cx, true);
    let draft_id = f.draft(cx).id;
    f.remove_credential();

    f.submit("missing credential draft", cx);
    let error = f.wait_for_preflight_error(cx);
    f.assert_rejected_before_start(&draft_id, "missing credential draft", cx);
    assert!(error.contains("gpt-5.6-terra"));
    assert!(error.contains("owned"));
    assert!(error.contains("本地凭据缺失或无法读取"));
    assert!(error.contains("设置 → Providers"));
    assert!(!error.contains("执行未完成"));
}

/// A7-01: malformed provider endpoints are not treated as usable merely
/// because a model and credential reference are present.
#[gpui_kit::test]
async fn a7_first_submit_malformed_provider_rejects_before_materialization(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = DraftFixture::home(cx, true);
    let draft_id = f.draft(cx).id;
    f.set_malformed_base_url();

    f.submit("malformed provider draft", cx);
    let error = f.wait_for_preflight_error(cx);
    f.assert_rejected_before_start(&draft_id, "malformed provider draft", cx);
    assert!(error.contains("gpt-5.6-terra"));
    assert!(error.contains("owned"));
    assert!(error.contains("设置 → Providers"));
    assert!(!error.contains("执行未完成"));
}

/// A7-01: repeated clicks after a rejected preflight stay single-flight and
/// cannot create duplicate rows or start a provider request.
#[gpui_kit::test]
async fn a7_repeated_rejected_submit_creates_no_rows_or_run(cx: &mut gpui_kit::TestAppContext) {
    let f = DraftFixture::home(cx, true);
    let draft_id = f.draft(cx).id;
    f.disable_provider();

    f.submit("retryable disabled draft", cx);
    let first_error = f.wait_for_preflight_error(cx);
    f.submit("retryable disabled draft", cx);
    let second_error = f.wait_for_preflight_error(cx);
    f.assert_rejected_before_start(&draft_id, "retryable disabled draft", cx);
    assert_eq!(first_error, second_error);
    assert_eq!(f.thread_rows(), 0);
    assert_eq!(f.standalone_rows(), 0);
}

/// A7-01: after readiness is repaired through the owned provider authority,
/// the same retained draft id is accepted once and follows the ordinary run
/// path with the provider/network boundary mocked.
#[gpui_kit::test]
async fn a7_repaired_readiness_submits_retained_draft_once(cx: &mut gpui_kit::TestAppContext) {
    let f = DraftFixture::home(cx, true);
    let draft_id = f.draft(cx).id;
    f.disable_provider();

    f.submit("repair and submit", cx);
    let _ = f.wait_for_preflight_error(cx);
    f.assert_rejected_before_start(&draft_id, "repair and submit", cx);

    // This uses the same owned config/credential authority that Settings
    // updates, with a synthetic fixture credential and no host-user state.
    f.repair_provider();
    f.submit("repair and submit", cx);
    pump_test_app(cx, |cx| {
        f.root.read_with(cx, |root, _| {
            root.agent_worker_start_probe.load() == 1
                && f.thread_rows() == 1
                && root.agent_controller.active.is_none()
        })
    });

    let rows = vega_store::threads::list_by_project(f.store().conn(), &f.project_id, None)
        .expect("r69 repaired rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, draft_id);
    assert_eq!(f.thread_rows(), 1);
    assert_eq!(f.standalone_rows(), 0);
    assert_eq!(f.provider.requests().len(), 1);
    assert!(
        !f.stream(cx)
            .read_with(cx, |stream, _| stream.composer_submission_pending())
    );
}

/// A7-03: the production home Composer accepts the first message, the
/// provider rejects it, and both the live transcript and composer explain
/// the durable failed turn without echoing the untrusted response body.
#[gpui_kit::test]
async fn a7_first_submit_provider_quota_failure_is_visible_and_durable(
    cx: &mut gpui_kit::TestAppContext,
) {
    const SENTINEL: &str = "PRIVATE_PROVIDER_BODY_SENTINEL";
    let f = DraftFixture::home(cx, true);
    let draft_id = f.draft(cx).id;
    let failed_provider = Arc::new(vega_runtime::MockProvider::new(vec![
        vega_runtime::ScriptStep::Error {
            status: Some(400),
            message: format!(
                "chat/completions request failed (HTTP 400): {{\"error\":{{\"code\":\"insufficient_user_quota\",\"message\":\"{SENTINEL}\"}}}}"
            ),
            retryable: false,
        },
    ]));
    f.root.update(cx, |root, _| {
        root.agent_provider_override = Some(with_auxiliary_title_fixture(failed_provider.clone()));
    });

    f.submit("hi", cx);
    pump_test_app(cx, |cx| {
        f.root.read_with(cx, |root, _| {
            root.agent_worker_start_probe.load() == 1
                && root.agent_controller.active.is_none()
                && failed_provider.requests().len() == 1
        })
    });
    let stream = f.stream(cx);
    let visible = stream
        .read_with(cx, |stream, _| stream.controller_error_message())
        .expect("quota failure is visible after app refresh");
    assert!(visible.contains("额度不足"), "wrong failure: {visible}");
    assert!(visible.contains("设置 → Providers"));
    assert!(!visible.contains(SENTINEL));
    assert!(!visible.contains("本地凭据缺失"));
    f.bounds("assistant-run-failure", cx);
    assert_eq!(f.thread_rows(), 1, "post-start failure retains its task");
    let store = f.store();
    let (status, content): (String, String) = store
        .conn()
        .query_row(
            "SELECT status, content FROM messages WHERE thread_id = ?1 AND role = 'assistant' ORDER BY seq DESC LIMIT 1",
            [&draft_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("durable assistant result");
    assert_eq!(status, "failed");
    assert!(
        content.is_empty(),
        "a provider failure emitted no fake reply"
    );
    let history = vega_conversation::history::restart_history_page(&store, &draft_id, 200)
        .expect("typed restart projection");
    assert!(history.entries.iter().any(|entry| matches!(
        entry,
        vega_conversation::history::HistoryEntry::AssistantText {
            status: vega_conversation::history::AssistantStatus::Failed,
            ..
        }
    )));
}

/// A7-03: an arbitrary provider body never becomes user-visible text when
/// the response lacks the exact allowlisted quota code.
#[gpui_kit::test]
async fn a7_provider_http_failure_uses_safe_fallback(cx: &mut gpui_kit::TestAppContext) {
    let f = DraftFixture::home(cx, true);
    let failed_provider = Arc::new(vega_runtime::MockProvider::new(vec![
        vega_runtime::ScriptStep::Error {
            status: Some(503),
            message: "PRIVATE_PROVIDER_BODY_SENTINEL".into(),
            retryable: false,
        },
    ]));
    f.root.update(cx, |root, _| {
        root.agent_provider_override = Some(with_auxiliary_title_fixture(failed_provider.clone()));
    });
    f.submit("status fallback", cx);
    pump_test_app(cx, |cx| {
        f.root.read_with(cx, |root, _| {
            root.agent_worker_start_probe.load() == 1
                && root.agent_controller.active.is_none()
                && failed_provider.requests().len() == 1
        })
    });
    let visible = f
        .stream(cx)
        .read_with(cx, |stream, _| stream.controller_error_message())
        .expect("provider status is visible");
    assert!(visible.contains("HTTP 503"));
    assert!(!visible.contains("PRIVATE_PROVIDER_BODY_SENTINEL"));
    assert!(!visible.contains("额度不足"));
    f.bounds("assistant-run-failure", cx);
}

/// A7-03: an out-of-order error is not allowed to terminate or annotate the
/// current assistant message. The event still belongs to the prior run only.
#[gpui_kit::test]
async fn a7_foreign_runtime_error_cannot_mark_current_turn(cx: &mut gpui_kit::TestAppContext) {
    let f = DraftFixture::home(cx, true);
    let stream = f.stream(cx);
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "current".into(),
                seq: 1,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::Error {
                message_id: Some("foreign".into()),
                error: Arc::new(vega_runtime::VegaError::Provider {
                    status: Some(503),
                    message: "PRIVATE_PROVIDER_BODY_SENTINEL".into(),
                    retryable: false,
                }),
            },
            cx,
        );
    });
    assert!(stream.read_with(cx, |stream, _| stream.has_active_agent()));
    assert_eq!(
        stream.read_with(cx, |stream, _| stream.controller_error_message()),
        None
    );
    assert!(f.absent("assistant-run-failure", cx));

    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::Error {
                message_id: Some("current".into()),
                error: Arc::new(vega_runtime::VegaError::Io(std::io::Error::other(
                    "PRIVATE_RUNTIME_SENTINEL",
                ))),
            },
            cx,
        );
    });
    assert!(!stream.read_with(cx, |stream, _| stream.has_active_agent()));
    let visible = stream
        .read_with(cx, |stream, _| stream.controller_error_message())
        .expect("owned runtime error is visible");
    assert!(visible.contains("运行环境"));
    assert!(!visible.contains("PRIVATE_RUNTIME_SENTINEL"));
    f.bounds("assistant-run-failure", cx);
}

/// A7-01 rule 8: the first-message pricing gate is a readiness failure, not
/// permission to persist an empty task. The mounted window routes to the
/// actual Pricing settings view; a settings-originated mutation repairs the
/// exact model, and its visible Back control returns to the same editable
/// draft before a second submit uses the ordinary provider boundary.
#[gpui_kit::test]
async fn a7_unpriced_first_submit_preserves_draft_through_pricing_repair(
    cx: &mut gpui_kit::TestAppContext,
) {
    let model = "a7-unpriced";
    let content = "Reply with exactly VEGA_E2E_OK.";
    let f = DraftFixture::open_with_repo_and_model(cx, true, diff_controller_repo(), Some(model));
    pump_test_app(cx, |cx| {
        f.root.read_with(cx, |root, _| {
            root.draft.is_some()
                && root.stream_view.is_some()
                && matches!(
                    root.pricing_controller.state,
                    PricingControllerState::Ready { .. }
                )
                && root.configured_models.is_some()
        })
    });
    let draft = f.draft(cx);
    assert_eq!(draft.model, model);
    assert_eq!(draft.project_id, f.project_id);
    f.submit(content, cx);
    pump_test_app(cx, |cx| {
        f.root.read_with(cx, |root, cx| {
            cx.global::<SettingsOpen>().0 && root.settings_view.is_some()
        })
    });
    f.bounds("settings-page-pricing", cx);
    assert_eq!(f.thread_rows(), 0, "an unpriced draft writes no empty task");
    assert_eq!(f.message_rows(&draft.id), 0);
    assert_eq!(f.draft(cx).id, draft.id);
    assert_eq!(
        f.input(cx)
            .read_with(cx, |input, _| input.text().to_owned()),
        content
    );
    assert!(f.provider.requests().is_empty());
    assert_eq!(
        f.root
            .read_with(cx, |root, _| root.agent_worker_start_probe.load()),
        0
    );

    let settings = f
        .root
        .read_with(cx, |root, _| root.settings_view.clone())
        .expect("mounted pricing settings");
    let generation = f
        .root
        .read_with(cx, |root, _| match &root.pricing_controller.state {
            PricingControllerState::Ready { generation, .. } => *generation,
            _ => panic!("pricing authority must be Ready"),
        });
    settings.update(cx, |_, cx| {
        cx.emit(PricingMutationRequested {
            generation,
            mutation: Ok(vega_conversation::types::PricingMutation::AddCustom {
                model: model.to_owned(),
                rates: vega_conversation::types::PricingRateInputs {
                    input_usd_per_million: "1".into(),
                    output_usd_per_million: "1".into(),
                    cache_read_usd_per_million: "1".into(),
                    cache_write_usd_per_million: "1".into(),
                },
            }),
        });
    });
    pump_test_app(cx, |cx| {
        f.root.read_with(cx, |root, _| {
            matches!(
                &root.pricing_controller.state,
                PricingControllerState::Ready { authority, .. }
                    if authority.contains_exact_model(model)
            )
        })
    });
    assert_eq!(f.thread_rows(), 0, "adding a price is not a submit");
    let mut visual = VisualTestContext::from_window(f.window.into(), cx);
    let back = visual
        .debug_bounds("settings-back")
        .expect("visible Back to app");
    visual.simulate_click(back.center(), Modifiers::default());
    visual.run_until_parked();
    pump_test_app(cx, |cx| {
        f.root.read_with(cx, |root, cx| {
            !cx.global::<SettingsOpen>().0
                && root.settings_view.is_none()
                && root
                    .stream_view
                    .as_ref()
                    .is_some_and(|(id, _)| id == &draft.id)
        })
    });
    let retained = f.draft(cx);
    assert_eq!(retained.id, draft.id);
    assert_eq!(retained.project_id, f.project_id);
    assert_eq!(retained.model, model);
    assert_eq!(
        f.input(cx)
            .read_with(cx, |input, _| input.text().to_owned()),
        content
    );
    assert_eq!(f.thread_rows(), 0);

    f.submit(content, cx);
    pump_test_app(cx, |cx| {
        f.root.read_with(cx, |root, _| {
            root.agent_worker_start_probe.load() == 1
                && root.agent_controller.active.is_none()
                && f.thread_rows() == 1
        })
    });
    let rows = vega_store::threads::list_by_project(f.store().conn(), &f.project_id, None)
        .expect("priced task rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, draft.id);
    assert_eq!(rows[0].model, model);
    assert_eq!(f.provider.requests().len(), 1);
    let user_content: String = f
        .store()
        .conn()
        .query_row(
            "SELECT content FROM messages WHERE thread_id = ?1 AND role = 'user' ORDER BY seq LIMIT 1",
            [&draft.id],
            |row| row.get(0),
        )
        .expect("first durable user message");
    assert_eq!(user_content, content);
}

/// A3 companion: repeated submits after materialization still produce exactly
/// one row (R11).
#[gpui_kit::test]
async fn r69_a3b_second_submit_writes_no_second_row(cx: &mut gpui_kit::TestAppContext) {
    let f = DraftFixture::home(cx, true);
    f.submit("first", cx);
    pump_test_app(cx, |cx| {
        f.root
            .read_with(cx, |root, _| root.agent_controller.active.is_none())
    });
    assert_eq!(f.thread_rows(), 1);

    // A second submit on the now-durable route is a plain durable submit: it
    // appends a message, never a second thread row.
    f.submit("second", cx);
    pump_test_app(cx, |cx| {
        f.root
            .read_with(cx, |root, _| root.agent_controller.active.is_none())
    });
    assert_eq!(
        f.thread_rows(),
        1,
        "materialization happens at most once (R11)"
    );
}

/// R14/R2 companion: with a project selected, the draft binds to it and the
/// materialized row carries that binding.
#[gpui_kit::test]
async fn r69_draft_binds_to_the_selected_project(cx: &mut gpui_kit::TestAppContext) {
    let f = DraftFixture::home(cx, true);
    let draft = f.draft(cx);
    assert_eq!(draft.project_id, f.project_id);
    // Switching the selection re-binds the draft without changing its id, so
    // the composer text is never stranded (R4).
    let store = f.store();
    let other = vega_store::projects::create(
        store.conn(),
        f.data_root.path().join("other").to_str().expect("UTF-8"),
        "r69-other",
        None,
    )
    .expect("r69 second project");
    cx.update(|cx| {
        cx.set_global(SelectedProject(Some(other.id.clone())));
        // The production selection path (`threads_block.rs:237-241`) always
        // refreshes the windows after writing the global; the re-bind itself
        // happens on the next render of the home route.
        cx.refresh_windows();
    });
    cx.run_until_parked();
    pump_test_app(cx, |cx| {
        f.root.read_with(cx, |root, _| {
            root.draft.as_ref().is_some_and(|d| d.id == draft.id)
        })
    });
    assert_eq!(
        f.root.read_with(cx, |root, _| root
            .draft
            .as_ref()
            .map(|d| d.project_id.clone())),
        Some(other.id.clone()),
        "the draft re-binds to the newly selected project"
    );
    f.submit("bound to the other project", cx);
    let rows = vega_store::threads::list_by_project(f.store().conn(), &other.id, None)
        .expect("r69 rebound rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, draft.id);
}

/// R69 × R68 integration: the R68 utility-bar dropdowns must still work on the
/// home draft route.
///
/// This combination is covered by neither suite alone. R68's own tests mount a
/// bare `StreamHarness` (`conversation_stream/tests/r68_popup_dismiss.rs`),
/// which bypasses the window route entirely; R69's A8 only asserts that the
/// utility bar is *mounted* on the draft route. So before this test, nothing
/// proved that clicking the project chip on the home route actually opens the
/// popup, or that an outside click closes it there — the exact surface the user
/// now sees on launch, since R69 made the composer (and therefore the R49
/// utility bar) render before any task exists.
#[gpui_kit::test]
async fn r69_r68_project_popup_opens_and_dismisses_on_the_draft_route(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = DraftFixture::home(cx, true);
    let chip = f.bounds("composer-utility-project-chip", cx);
    assert!(
        f.absent("composer-utility-project-menu", cx),
        "the popup starts closed on the draft route"
    );

    // R68 A1: the chip click opens it. `VisualTestContext` drives a real
    // pointer event, so this exercises R68's capture-phase trigger claim.
    let chip_point = gpui_kit::point(
        chip.origin.x + chip.size.width / 2.,
        chip.origin.y + chip.size.height / 2.,
    );
    {
        let mut visual = gpui_kit::VisualTestContext::from_window(f.window.into(), cx);
        visual.simulate_click(chip_point, gpui_kit::Modifiers::default());
        visual.run_until_parked();
    }
    assert!(
        !f.absent("composer-utility-project-menu", cx),
        "R68 A1 must hold on the R69 draft route: the chip click opens the popup"
    );

    // R68 A2: an outside click closes it. The window's top-left corner is far
    // from the bottom-anchored composer column, and the guard keeps that true.
    let outside = gpui_kit::point(gpui_kit::px(4.), gpui_kit::px(4.));
    let popup = f.bounds("composer-utility-project-menu", cx);
    assert!(
        !popup.contains(&outside),
        "the outside point must not be inside the popup: popup={popup:?}"
    );
    {
        let mut visual = gpui_kit::VisualTestContext::from_window(f.window.into(), cx);
        visual.simulate_click(outside, gpui_kit::Modifiers::default());
        visual.run_until_parked();
    }
    assert!(
        f.absent("composer-utility-project-menu", cx),
        "R68 A2 must hold on the R69 draft route: an outside click closes the popup"
    );

    // Opening a popup must not materialize the draft: the popup is pure UI.
    assert_eq!(
        f.thread_rows(),
        0,
        "interacting with the utility bar must not write a row (R5)"
    );
    assert!(
        f.root.read_with(cx, |root, _| root.draft.is_some()),
        "the draft route is unchanged by a popup interaction"
    );
}

/// A8-02: the real Sidebar menu unregisters a project with a durable task,
/// but the task and its messages survive as a standalone route. This covers
/// the window's cached route and authority, not just the SQL primitive.
#[gpui_kit::test]
async fn a8_remove_project_keeps_open_task_and_clears_project_authority(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = DraftFixture::home(cx, true);
    let removed_project = f.project_id.clone();
    let project_path = f._repo.path().to_path_buf();
    f.submit("retained task", cx);
    pump_test_app(cx, |cx| {
        f.root
            .read_with(cx, |root, _| root.agent_controller.active.is_none())
            && f.thread_rows() == 1
    });
    let opened = cx.update(|cx| {
        cx.global::<OpenedThread>()
            .0
            .clone()
            .expect("durable route")
    });
    assert_eq!(opened.project_id, removed_project);
    assert!(!f.absent("environment-rail", cx));
    let message_count = f.message_rows(&opened.id);
    assert!(message_count > 0);
    f.input(cx).update(cx, |input, cx| {
        input.set_text("unsent before unregister", cx)
    });
    let project_menu: &'static str =
        Box::leak(format!("project-more-{}", removed_project).into_boxed_str());
    pump_test_app(cx, |cx| !f.absent(project_menu, cx));

    // Drive the production project action menu. A single registered project
    // has just its Remove Project entry at index zero.
    f.click(project_menu, cx);
    assert!(!f.absent("organization-menu-0", cx));
    f.click("organization-menu-0", cx);
    let standalone_row: &'static str =
        Box::leak(format!("standalone-thread-row-{}", opened.id).into_boxed_str());
    let stale_project_row: &'static str =
        Box::leak(format!("project-header-{}", removed_project).into_boxed_str());
    pump_test_app(cx, |cx| {
        f.absent(stale_project_row, cx) && !f.absent(standalone_row, cx)
    });

    assert!(
        project_path.is_dir(),
        "unregister never removes the local folder"
    );
    assert_eq!(f.thread_rows(), 0);
    assert_eq!(f.standalone_rows(), 1);
    assert_eq!(f.message_rows(&opened.id), message_count);
    assert!(
        vega_store::projects::find(f.store().conn(), &removed_project)
            .expect("project lookup")
            .is_none()
    );
    cx.update(|cx| {
        assert!(cx.global::<SelectedProject>().0.is_none());
        assert!(
            cx.global::<OpenedThread>()
                .0
                .as_ref()
                .is_none_or(|thread| thread.project_binding().is_none()),
            "old project must not remain the workspace authority"
        );
    });
    assert!(
        f.absent("environment-rail", cx),
        "the project-only Environment rail must close"
    );
    let removed_project_task: &'static str =
        Box::leak(format!("project-thread-row-{}", opened.id).into_boxed_str());
    assert!(f.absent(removed_project_task, cx));

    f.click(standalone_row, cx);
    pump_test_app(cx, |cx| {
        cx.update(|cx| {
            cx.global::<OpenedThread>()
                .0
                .as_ref()
                .is_some_and(|thread| thread.id == opened.id && thread.is_standalone())
        })
    });
    assert!(f.absent("environment-rail", cx));
    assert_eq!(f.message_rows(&opened.id), message_count);
    assert_eq!(
        f.input(cx)
            .read_with(cx, |input, _| input.text().to_owned()),
        "unsent before unregister",
        "the normal draft-navigation guard must preserve the open task's unsent input"
    );
}

/// A8-02: another window's real provider worker retains its registered
/// workspace until it exits. The production menu must refuse to detach its
/// task while blocked, then accept the same action after worker termination.
#[gpui_kit::test]
async fn a8_other_window_cannot_remove_project_while_worker_is_alive(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = DraftFixture::home(cx, true);
    f.submit("first durable task", cx);
    pump_test_app(cx, |cx| {
        f.thread_rows() == 1
            && f.root
                .read_with(cx, |root, _| root.agent_controller.active.is_none())
    });

    // A second real Vega window shares App globals and the same mounted
    // Sidebar/store, but does not own the first window's Agent controller.
    let second_root = cx.new(VegaWindow::new);
    second_root.update(cx, |root, _| {
        root.model_selection_config_override = Some(f.config_path.clone());
    });
    let second_window_root = second_root.clone();
    let second_window = cx.update(|cx| {
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1403.), px(860.)),
                    cx,
                ))),
                ..Default::default()
            },
            move |_, _| second_window_root,
        )
        .expect("second project window")
    });
    cx.run_until_parked();

    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let probe = f
        .root
        .read_with(cx, |root, _| root.agent_worker_start_probe.clone());
    *probe
        .provider_construction_gate
        .lock()
        .expect("existing worker gate") = Some((entered_tx, release_rx));
    f.submit("run blocked at provider construction", cx);
    pump_test_app(cx, |cx| {
        f.root
            .read_with(cx, |root, _| root.agent_controller.active.is_some())
    });
    entered_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("real worker reached provider construction");
    assert!(cx.update(|cx| { vega_ui::sidebar::project_worker_is_active(&f.project_id, cx) }));
    let route_before = cx.update(|cx| cx.global::<OpenedThread>().0.clone().expect("run route"));

    let project_menu: &'static str =
        Box::leak(format!("project-more-{}", f.project_id).into_boxed_str());
    let click_second = |selector: &'static str, cx: &mut gpui_kit::TestAppContext| {
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(second_window.into(), cx);
        let target = visual
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("second window missing {selector}"));
        visual.simulate_click(target.center(), Modifiers::default());
        visual.run_until_parked();
    };
    click_second(project_menu, cx);
    click_second("organization-menu-0", cx);
    assert!(
        vega_store::projects::find(f.store().conn(), &f.project_id)
            .expect("blocked project lookup")
            .is_some()
    );
    assert_eq!(f.thread_rows(), 1);
    assert_eq!(f.standalone_rows(), 0);
    cx.update(|cx| {
        assert_eq!(
            cx.global::<SelectedProject>().0.as_deref(),
            Some(f.project_id.as_str())
        );
        assert_eq!(cx.global::<OpenedThread>().0.as_ref(), Some(&route_before));
    });
    assert!(
        f.root
            .read_with(cx, |root, _| root.agent_controller.active.is_some())
    );

    // Stop is asynchronous: cancellation alone must not release the folder
    // while the blocked worker still owns its execution context.
    f.root.update(cx, |root, cx| root.cancel_active_agent(cx));
    assert!(f.root.read_with(cx, |root, _| {
        root.agent_controller
            .active
            .as_ref()
            .is_some_and(|active| active.cancel.is_cancelled())
    }));
    click_second(project_menu, cx);
    click_second("organization-menu-0", cx);
    assert!(
        vega_store::projects::find(f.store().conn(), &f.project_id)
            .expect("cancelled worker still owns project")
            .is_some()
    );
    assert_eq!(f.thread_rows(), 1);
    assert_eq!(f.standalone_rows(), 0);

    release_tx.send(()).expect("release exact worker");
    pump_test_app(cx, |cx| {
        f.root
            .read_with(cx, |root, _| root.agent_controller.active.is_none())
    });
    assert!(!cx.update(|cx| { vega_ui::sidebar::project_worker_is_active(&f.project_id, cx) }));
    click_second(project_menu, cx);
    click_second("organization-menu-0", cx);
    pump_test_app(cx, |_| {
        vega_store::projects::find(f.store().conn(), &f.project_id)
            .expect("removed project lookup")
            .is_none()
    });
    assert_eq!(f.thread_rows(), 0);
    assert_eq!(f.standalone_rows(), 1);
}
