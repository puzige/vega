use crate::window::VegaWindow;
use gpui_kit::{AppContext, Focusable, TestAppContext, WindowHandle};
use vega_conversation::types::PermissionMode;
use vega_store::{Store, config};
use vega_ui::conversation_stream::ConversationStream;
use vega_ui::settings::SettingsOpen;
use vega_ui::sidebar::{
    NewTaskDefaults, OpenedThread, PendingDeleteConfirm, ProjectsCollapsed, SelectedProject,
    SessionsCollapsed, SidebarCollapsed, SidebarWidth, VegaStore,
};

#[gpui_kit::test]
fn full_access_picker_and_confirmation_use_real_keyboard_and_preserve_the_draft(
    cx: &mut TestAppContext,
) {
    for activation in ["enter", "space"] {
        let root =
            std::env::temp_dir().join(format!("vega-permission-window-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&root).expect("isolated fixture");
        let path = root.join("vega.db");
        let store = Store::open(&path).expect("fixture database");
        store.migrate().expect("fixture schema");
        let config_path = root.join("config.toml");
        config::load_from(&config_path).expect("fixture config");
        let source = vega_conversation::threads::draft_thread(None, "synthetic", "confirm");
        cx.update(|cx| {
            gpui_kit::init(cx);
            vega_ui::init(cx);
            cx.set_global(vega_theme::Theme::light());
            cx.set_global(VegaStore(Ok(
                Store::open_existing(&path).expect("UI fixture connection")
            )));
            cx.set_global(SelectedProject(None));
            cx.set_global(OpenedThread(Some(source.clone())));
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
            owner.draft = Some(source.clone());
            let stream = cx.new(|cx| ConversationStream::new(source.clone(), cx));
            stream.update(cx, |stream, cx| {
                stream.set_draft_route(true, cx);
                stream
                    .composer_input()
                    .update(cx, |input, cx| input.set_text("unsent fixture", cx));
            });
            owner.bind_conversation_stream(&stream, window, cx);
            owner.stream_view = Some((source.id.clone(), stream));
            owner
        });
        let stream = window
            .read_with(cx, |owner, _| {
                owner.stream_view.as_ref().expect("stream").1.clone()
            })
            .expect("read stream");
        let open = |operation, cx: &mut TestAppContext| {
            window
                .update(cx, |_, window, cx| {
                    stream.read(cx).focus_handle(cx).focus(window, cx)
                })
                .expect("focus input");
            draw(window, cx);
            cx.simulate_keystrokes(window.into(), "tab tab enter");
            draw(window, cx);
            cx.simulate_keystrokes(window.into(), "down down enter");
            let mut ready = false;
            for _ in 0..300 {
                cx.run_until_parked();
                ready = window
                    .read_with(cx, |owner, cx| {
                        !owner.trusted_actions.is_busy()
                            && stream.read(cx).owns_thread_permission_operation(operation)
                    })
                    .expect("read confirmation readiness");
                if ready {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            assert!(ready, "real app preparation must open a confirmation");
            draw(window, cx);
        };
        open(1, cx);
        cx.simulate_keystrokes(window.into(), "escape");
        cx.run_until_parked();
        assert!(
            vega_store::threads::find(store.conn(), &source.id)
                .expect("cancelled intent has no row")
                .is_none()
        );
        assert!(
            !window
                .read_with(cx, |_, cx| stream
                    .read(cx)
                    .owns_thread_permission_operation(1))
                .expect("cancelled operation")
        );
        open(2, cx);
        cx.simulate_keystrokes(window.into(), "tab shift-tab");
        draw(window, cx);
        cx.simulate_keystrokes(window.into(), activation);
        cx.run_until_parked();
        assert!(
            vega_store::threads::find(store.conn(), &source.id)
                .expect("default cancel focus must not grant")
                .is_none()
        );
        open(3, cx);
        cx.simulate_keystrokes(window.into(), "tab");
        draw(window, cx);
        cx.simulate_keystrokes(window.into(), activation);
        let mut saved = false;
        for _ in 0..300 {
            cx.run_until_parked();
            saved = window
                .read_with(cx, |owner, cx| {
                    owner.draft.is_none()
                        && stream
                            .read(cx)
                            .thread_snapshot()
                            .full_access_grant
                            .is_some()
                })
                .expect("read persisted result");
            if saved {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(
            saved,
            "explicit keyboard confirmation must commit through the app"
        );
        let durable = vega_conversation::threads::get_thread(&store, &source.id)
            .expect("durable authorized thread");
        assert_eq!(durable.permission_mode, PermissionMode::FullAccess);
        assert!(
            durable
                .full_access_grant
                .as_ref()
                .expect("grant")
                .is_valid()
        );
        window
            .read_with(cx, |_, cx| {
                assert_eq!(
                    stream.read(cx).composer_input().read(cx).text(),
                    "unsent fixture"
                )
            })
            .expect("input preserved");
        let weak = stream.downgrade();
        window
            .update(cx, |_, window, _| window.remove_window())
            .expect("close fixture window");
        drop(stream);
        for _ in 0..300 {
            cx.run_until_parked();
            if weak.upgrade().is_none() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(
            weak.upgrade().is_none(),
            "window and listener ownership must drain"
        );
        std::fs::remove_dir_all(root).expect("fixture cleanup");
    }
}

fn draw(window: WindowHandle<VegaWindow>, cx: &mut TestAppContext) {
    cx.update_window(window.into(), |_, window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    })
    .expect("draw through the real window arena");
}
