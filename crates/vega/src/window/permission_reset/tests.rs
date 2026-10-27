use crate::window::VegaWindow;
use gpui_kit::{AppContext, TestAppContext};
use vega_conversation::permission_contracts::change_thread_permission;
use vega_conversation::types::{PermissionMode, PermissionResetAction, PermissionResetProjection};
use vega_store::{Store, config, thread_admission};
use vega_ui::conversation_stream::ConversationStream;
use vega_ui::settings::{PricingSettingsRequested, SettingsOpen};
use vega_ui::sidebar::{
    NewTaskDefaults, OpenedThread, PendingDeleteConfirm, ProjectsCollapsed, SelectedProject,
    SessionsCollapsed, SidebarCollapsed, SidebarWidth, VegaStore,
};

#[gpui_kit::test]
fn reset_confirmation_keyboard_drives_the_app_and_keeps_unsent_input(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().expect("isolated reset fixture");
    let database = root.path().join("vega.db");
    let config_path = root.path().join("config.toml");
    config::load_from(&config_path).expect("isolated configuration");
    let store = Store::open(&database).expect("isolated database");
    store.migrate().expect("isolated schema");
    let thread = vega_conversation::threads::materialize_draft(
        &store,
        &vega_conversation::threads::draft_thread(None, "synthetic", "confirm"),
    )
    .expect("fixture conversation");
    let thread =
        change_thread_permission(&store, &thread.id, 0, PermissionMode::FullAccess, true, 1)
            .expect("explicit fixture grant");
    let original_grant = thread.full_access_grant.clone();
    cx.update(|cx| {
        gpui_kit::init(cx);
        vega_ui::init(cx);
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(VegaStore(Ok(
            Store::open_existing(&database).expect("UI connection")
        )));
        cx.set_global(SelectedProject(None));
        cx.set_global(OpenedThread(Some(thread.clone())));
        cx.set_global(SettingsOpen(true));
        cx.set_global(PricingSettingsRequested(false));
        cx.set_global(SidebarCollapsed(true));
        cx.set_global(SidebarWidth(240.0));
        cx.set_global(ProjectsCollapsed(false));
        cx.set_global(SessionsCollapsed(false));
        cx.set_global(PendingDeleteConfirm(None));
        cx.set_global(NewTaskDefaults::default());
        cx.set_global(crate::app_agent::AppReplaySessions::default());
        crate::automation_controller::startup_ready(cx);
    });
    let window = cx.add_window(|_, cx| {
        let mut owner = VegaWindow::new_with_services(cx, false);
        owner.fixture_config_path = Some(config_path.clone());
        owner.configured_models = Some(Vec::new());
        let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
        stream.update(cx, |stream, cx| {
            stream
                .composer_input()
                .update(cx, |input, cx| input.set_text("unsent reset fixture", cx))
        });
        owner.stream_view = Some((thread.id.clone(), stream));
        owner
    });
    let draw = |cx: &mut TestAppContext| {
        cx.update_window(window.into(), |_, window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        })
        .expect("real settings window draw");
    };
    draw(cx);
    let prepare = |cx: &mut TestAppContext| {
        window
            .update(cx, |owner, _, cx| {
                let settings = owner
                    .settings_view
                    .clone()
                    .expect("real settings subscriptions");
                owner.permission_reset_requested(settings, PermissionResetAction::Prepare, cx);
            })
            .expect("request current source");
        pump(window, cx, |owner| {
            owner.permission_reset.projection == PermissionResetProjection::Confirming
        });
        draw(cx);
    };
    prepare(cx);
    assert!(
        !thread_admission::is_database_paused(store.database_identity())
            .expect("confirmation cannot pause")
    );
    cx.simulate_keystrokes(window.into(), "escape");
    cx.run_until_parked();
    assert_eq!(
        vega_conversation::threads::get_thread(&store, &thread.id)
            .expect("cancelled source")
            .full_access_grant,
        original_grant
    );
    assert!(
        vega_store::permission_reset::load(store.conn())
            .expect("no cancelled journal")
            .is_none()
    );
    prepare(cx);
    cx.simulate_keystrokes(window.into(), "tab shift-tab enter");
    cx.run_until_parked();
    window
        .read_with(cx, |owner, _| {
            assert_eq!(
                owner.permission_reset.projection,
                PermissionResetProjection::Idle
            )
        })
        .expect("default cancel");
    assert!(
        !thread_admission::is_database_paused(store.database_identity())
            .expect("default cancel cannot pause")
    );
    prepare(cx);
    cx.simulate_keystrokes(window.into(), "tab");
    draw(cx);
    cx.simulate_keystrokes(window.into(), "space");
    pump(window, cx, |owner| {
        owner.permission_reset.projection == PermissionResetProjection::Completed
    });
    window
        .read_with(cx, |owner, cx| {
            assert!(!owner.trusted_actions.is_busy());
            let stream = owner
                .stream_view
                .as_ref()
                .expect("retained stream")
                .1
                .read(cx);
            assert_eq!(
                stream.composer_input().read(cx).text(),
                "unsent reset fixture"
            );
            assert_eq!(
                stream.thread_snapshot().permission_mode,
                PermissionMode::Confirm
            );
            assert!(stream.thread_snapshot().full_access_grant.is_none());
            assert!(
                cx.global::<crate::automation_controller::AutomationGlobal>()
                    .ready
            );
        })
        .expect("app refreshed before release");
    assert!(
        !thread_admission::is_database_paused(store.database_identity())
            .expect("completed refresh releases pause")
    );
    let record = vega_store::permission_reset::load(store.conn())
        .expect("journal")
        .expect("durable receipt");
    assert!(record.completed);
    window
        .update(cx, |_, window, _| window.remove_window())
        .expect("close reset fixture");
    for _ in 0..300 {
        cx.run_until_parked();
        cx.background_executor
            .advance_clock(std::time::Duration::from_millis(10));
    }
}

fn pump(
    window: gpui_kit::WindowHandle<VegaWindow>,
    cx: &mut TestAppContext,
    ready: impl Fn(&VegaWindow) -> bool,
) {
    for _ in 0..1000 {
        cx.run_until_parked();
        if window
            .read_with(cx, |owner, _| ready(owner))
            .expect("live fixture")
        {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    window
        .read_with(cx, |owner, _| {
            panic!(
                "reset callback did not reach expected phase: {:?}",
                owner.permission_reset.projection
            )
        })
        .expect("live fixture failure");
}
