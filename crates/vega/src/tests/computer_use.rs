use super::*;
use crate::window::computer_use::{ComputerUseAction, ComputerUseOwner};
use gpui_kit::{TestAppContext, VisualTestContext};

fn fixture(
    cx: &mut TestAppContext,
) -> (
    TempDir,
    Entity<VegaWindow>,
    Entity<ConversationStream>,
    Thread,
) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(":memory:").unwrap();
    store.migrate().unwrap();
    let project = vega_store::projects::create(
        store.conn(),
        dir.path().to_str().unwrap(),
        "computer-use",
        None,
    )
    .unwrap();
    let thread =
        vega_conversation::threads::create_thread(&store, &project.id, "mock", "confirm").unwrap();
    cx.update(|cx| install_diff_window_globals(store, thread.clone(), cx));
    let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
    let config_path = dir.path().join("config.toml");
    vega_store::config::AppConfig::default()
        .save_to(&config_path)
        .unwrap();
    let root = cx.new(VegaWindow::new);
    root.update(cx, |root, _| {
        root.model_selection_config_override = Some(config_path)
    });
    (dir, root, stream, thread)
}

#[gpui_kit::test]
async fn issue75_late_ready_after_stop_is_revoked_and_stale_generation_is_ignored(
    cx: &mut TestAppContext,
) {
    let (_dir, root, stream, thread) = fixture(cx);
    let (generation, cancel) = root.update(cx, |root, _| {
        root.agent_controller
            .begin(thread.id.clone(), stream.clone(), None, None)
            .unwrap()
    });
    root.update(cx, |root, _| {
        root.agent_controller.request_active_cancel(&thread.id)
    });
    assert!(cancel.is_cancelled());
    let control = ComputerUseControl::new();
    let (sender, receiver) = mpsc::sync_channel(4);
    sender
        .send(AgentUpdate::ComputerUseReady {
            control: control.clone(),
            owner: "Owner".into(),
        })
        .unwrap();
    root.update(cx, |root, cx| {
        root.apply_agent_batch_ingress(
            generation,
            &thread.id,
            &stream,
            drain_agent_updates(&receiver),
            cx,
        );
    });
    assert_eq!(control.status().phase, ControlPhase::Stopping);
    root.update(cx, |root, _| {
        root.agent_controller
            .finish(generation, &thread.id, &stream);
    });
    let (next, _) = root.update(cx, |root, _| {
        root.agent_controller
            .begin(thread.id.clone(), stream.clone(), None, None)
            .unwrap()
    });
    assert_ne!(generation, next);
    sender
        .send(AgentUpdate::ComputerUseReady {
            control: ComputerUseControl::new(),
            owner: "Stale".into(),
        })
        .unwrap();
    assert!(root.update(cx, |root, cx| matches!(
        root.apply_agent_batch_ingress(
            generation,
            &thread.id,
            &stream,
            drain_agent_updates(&receiver),
            cx
        ),
        AgentBatchIngress::Stale
    )));
    assert!(root.read_with(cx, |root, _| {
        root.agent_controller.active[&thread.id]
            .computer_use
            .is_none()
    }));
}

#[gpui_kit::test]
async fn issue75_control_bar_survives_settings_and_stop_has_epoch_and_owner_fences(
    cx: &mut TestAppContext,
) {
    let (_dir, root, stream, thread) = fixture(cx);
    let control = ComputerUseControl::new();
    control.pause();
    let (generation, cancel) = root.update(cx, |root, _| {
        let result = root
            .agent_controller
            .begin(thread.id.clone(), stream.clone(), None, None)
            .unwrap();
        let active = root.agent_controller.active.get_mut(&thread.id).unwrap();
        active.computer_use = Some(control.clone());
        active.computer_use_owner = "Original owner".into();
        result
    });
    let rendered = root.clone();
    let window = cx
        .update(|cx| {
            cx.open_window(
                gpui_kit::WindowOptions {
                    window_bounds: Some(gpui_kit::WindowBounds::Windowed(
                        gpui_kit::Bounds::centered(
                            None,
                            gpui_kit::size(gpui_kit::px(960.), gpui_kit::px(600.)),
                            cx,
                        ),
                    )),
                    ..Default::default()
                },
                move |_, _| rendered,
            )
        })
        .unwrap();
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("computer-use-control-bar").is_some());
    assert!(visual.debug_bounds("computer-use-resume").is_none());
    cx.update(|cx| {
        cx.set_global(OpenedThread(None));
        cx.set_global(SettingsOpen(true));
    });
    cx.run_until_parked();
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("computer-use-control-bar")
            .is_some()
    );
    let status = control.status();
    let mut owner = ComputerUseOwner {
        thread_id: thread.id.clone(),
        generation,
        epoch: status.epoch + 1,
        phase: status.phase,
        capability: status.capability,
    };
    root.update(cx, |root, cx| {
        root.computer_use_action(&owner, &ComputerUseAction::Stop, cx)
    });
    assert!(!cancel.is_cancelled());
    owner.epoch = status.epoch;
    owner.generation += 1;
    root.update(cx, |root, cx| {
        root.computer_use_action(&owner, &ComputerUseAction::Stop, cx)
    });
    assert!(!cancel.is_cancelled());
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let stop = visual.debug_bounds("computer-use-stop").unwrap();
    visual.simulate_click(stop.center(), Default::default());
    cx.run_until_parked();
    assert!(cancel.is_cancelled());
    assert_eq!(control.status().phase, ControlPhase::Stopping);
}

#[gpui_kit::test]
async fn issue75_window_cleanup_revokes_every_control_before_cancelling(cx: &mut TestAppContext) {
    let (_dir, root, stream, thread) = fixture(cx);
    let control = ComputerUseControl::new();
    let (_, cancel) = root.update(cx, |root, _| {
        let result = root
            .agent_controller
            .begin(thread.id.clone(), stream.clone(), None, None)
            .unwrap();
        root.agent_controller
            .active
            .get_mut(&thread.id)
            .unwrap()
            .computer_use = Some(control.clone());
        result
    });
    root.update(cx, |root, _| root.window_terminal_cleanup());
    assert!(cancel.is_cancelled());
    assert_eq!(control.status().phase, ControlPhase::Stopping);
}

#[gpui_kit::test]
async fn issue75_confirming_background_task_deletion_revokes_its_control(cx: &mut TestAppContext) {
    let (_dir, root, stream, thread) = fixture(cx);
    let control = ComputerUseControl::new();
    control.pause();
    let (_, cancel) = root.update(cx, |root, _| {
        let result = root
            .agent_controller
            .begin(thread.id.clone(), stream.clone(), None, None)
            .unwrap();
        root.agent_controller
            .active
            .get_mut(&thread.id)
            .unwrap()
            .computer_use = Some(control.clone());
        result
    });
    cx.update(|cx| {
        cx.set_global(OpenedThread(None));
        cx.set_global(PendingDeleteConfirm(Some(thread.clone())));
    });
    let rendered = root.clone();
    let window = cx
        .update(|cx| cx.open_window(Default::default(), move |_, _| rendered))
        .unwrap();
    cx.run_until_parked();
    assert!(!cancel.is_cancelled());
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let delete = visual.debug_bounds("delete-confirm-action").unwrap();
    visual.simulate_click(delete.center(), Default::default());
    cx.run_until_parked();
    assert!(cancel.is_cancelled());
    assert_eq!(control.status().phase, ControlPhase::Stopping);
}
