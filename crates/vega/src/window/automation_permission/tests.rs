use crate::window::VegaWindow;
use gpui_kit::{AppContext, TestAppContext};
use vega_conversation::permission_contracts::change_thread_permission;
use vega_conversation::types::{
    AutomationCommand, AutomationControl, AutomationKind, AutomationProjection, AutomationStatus,
    PermissionMode,
};
use vega_store::{Store, config};
use vega_ui::conversation_stream::ConversationStream;
use vega_ui::settings::SettingsOpen;
use vega_ui::sidebar::{
    NewTaskDefaults, OpenedThread, PendingDeleteConfirm, ProjectsCollapsed, SelectedProject,
    SessionsCollapsed, SidebarCollapsed, SidebarWidth, VegaStore,
};

#[gpui_kit::test]
fn automation_confirmation_is_separate_from_thread_consent_and_preserves_input(
    cx: &mut TestAppContext,
) {
    let root = tempfile::tempdir().expect("isolated automation fixture");
    let database = root.path().join("vega.db");
    let config_path = root.path().join("config.toml");
    config::load_from(&config_path).expect("isolated configuration");
    let store = Store::open(&database).expect("isolated database");
    store.migrate().expect("fixture schema");
    let thread = vega_conversation::threads::materialize_draft(
        &store,
        &vega_conversation::threads::draft_thread(None, "synthetic", "confirm"),
    )
    .expect("fixture conversation");
    let thread =
        change_thread_permission(&store, &thread.id, 0, PermissionMode::FullAccess, true, 1)
            .expect("thread grant only");
    let task = vega_conversation::automation::command(
        &store,
        &thread.id,
        &AutomationCommand::Create {
            kind: AutomationKind::Loop,
            instruction: "synthetic recurring work".into(),
            token_budget: None,
            interval_seconds: Some(60),
        },
        vega_conversation::automation::now_ms(),
    )
    .expect("task without grant")
    .remove(0);
    let paused = vega_conversation::automation::command(
        &store,
        &thread.id,
        &AutomationCommand::Control(task.kind, AutomationControl::Pause),
        vega_conversation::automation::now_ms(),
    )
    .expect("paused target")
    .remove(0);
    assert!(paused.permission_authorization.is_none());
    cx.update(|cx| {
        gpui_kit::init(cx);
        vega_ui::init(cx);
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(VegaStore(Ok(
            Store::open_existing(&database).expect("UI connection")
        )));
        cx.set_global(SelectedProject(None));
        cx.set_global(OpenedThread(Some(thread.clone())));
        cx.set_global(SettingsOpen(false));
        cx.set_global(SidebarCollapsed(true));
        cx.set_global(SidebarWidth(240.0));
        cx.set_global(ProjectsCollapsed(false));
        cx.set_global(SessionsCollapsed(false));
        cx.set_global(PendingDeleteConfirm(None));
        cx.set_global(NewTaskDefaults::default());
    });
    let window = cx.add_window(|window, cx| {
        let mut owner = VegaWindow::new_with_services(cx, false);
        owner.fixture_config_path = Some(config_path);
        owner.configured_models = Some(Vec::new());
        let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
        stream.update(cx, |stream, cx| {
            stream.composer_input().update(cx, |input, cx| {
                input.set_text("unsent automation fixture", cx)
            });
            stream.apply_automation(
                AutomationProjection {
                    tasks: vec![paused.clone()],
                    ..Default::default()
                },
                cx,
            );
        });
        owner.bind_conversation_stream(&stream, window, cx);
        owner.stream_view = Some((thread.id.clone(), stream));
        owner
    });
    let stream = window
        .read_with(cx, |owner, _| {
            owner
                .stream_view
                .as_ref()
                .expect("original stream")
                .1
                .clone()
        })
        .expect("read stream");
    let draw = |cx: &mut TestAppContext| {
        cx.update_window(window.into(), |_, window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        })
        .expect("real conversation arena");
    };
    let open = |operation, cx: &mut TestAppContext| {
        window
            .update(cx, |_, _, cx| {
                stream.update(cx, |stream, cx| {
                    stream.request_automation_permission(paused.clone(), true, None, cx)
                })
            })
            .expect("request separate intent");
        let mut ready = false;
        for _ in 0..500 {
            cx.run_until_parked();
            ready = window
                .read_with(cx, |owner, cx| {
                    !owner.trusted_actions.is_busy()
                        && stream
                            .read(cx)
                            .owns_automation_permission_operation(operation)
                })
                .expect("read actual confirmation");
            if ready {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(ready, "independent automation confirmation must be ready");
        draw(cx);
    };
    open(1, cx);
    cx.simulate_keystrokes(window.into(), "tab shift-tab enter");
    cx.run_until_parked();
    assert_eq!(
        vega_conversation::automation::load(&store, &thread.id).expect("cancelled task"),
        vec![paused.clone()]
    );
    open(3, cx);
    cx.simulate_keystrokes(window.into(), "tab");
    draw(cx);
    cx.simulate_keystrokes(window.into(), "space");
    let mut saved = false;
    for _ in 0..500 {
        cx.run_until_parked();
        saved = window
            .read_with(cx, |owner, cx| {
                !owner.trusted_actions.is_busy()
                    && !stream.read(cx).permission_change_pending()
                    && stream
                        .read(cx)
                        .automation_projection()
                        .tasks
                        .iter()
                        .any(|task| {
                            task.status == AutomationStatus::Active
                                && task.permission_authorization.is_some()
                        })
            })
            .expect("save receipt");
        if saved {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(
        saved,
        "explicit task confirmation resumes and authorizes atomically"
    );
    let current = vega_conversation::automation::load(&store, &thread.id)
        .expect("durable automation")
        .remove(0);
    assert_eq!(current.revision, paused.revision + 1);
    assert_eq!(
        current
            .permission_authorization
            .as_ref()
            .expect("independent grant")
            .revision,
        current.revision
    );
    assert_eq!(
        vega_conversation::threads::get_thread(&store, &thread.id)
            .expect("unchanged thread grant")
            .full_access_grant,
        thread.full_access_grant
    );
    window
        .read_with(cx, |_, cx| {
            assert_eq!(
                stream.read(cx).composer_input().read(cx).text(),
                "unsent automation fixture"
            )
        })
        .expect("unsent input preserved");
    let weak = stream.downgrade();
    window
        .update(cx, |_, window, _| window.remove_window())
        .expect("close fixture");
    drop(stream);
    for _ in 0..500 {
        cx.run_until_parked();
        if weak.upgrade().is_none() {
            break;
        }
        cx.background_executor
            .advance_clock(std::time::Duration::from_millis(10));
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(
        weak.upgrade().is_none(),
        "automation callbacks must release the original view"
    );
}
