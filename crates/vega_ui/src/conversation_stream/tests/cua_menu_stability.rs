use super::*;
use gpui_kit::{Modifiers, VisualTestContext};
use vega_conversation::types::{ComputerUseControl, ControlPhase, ConversationStopReason};

fn start_run(
    stream: &Entity<ConversationStream>,
    control: &ComputerUseControl,
    id: &str,
    cx: &mut TestAppContext,
) {
    stream.update(cx, |stream, cx| {
        stream.begin_composer_run(cx);
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: id.into(),
                seq: 1,
            },
            cx,
        );
        stream.set_computer_use_control(Some(control.clone()), cx);
    });
}

fn finish_run(
    stream: &Entity<ConversationStream>,
    control: &ComputerUseControl,
    id: &str,
    cx: &mut TestAppContext,
) {
    control.stop();
    vega_runtime::computer_use::test_support::finish_cleanup(control);
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageFinished {
                message_id: id.into(),
                stop_reason: ConversationStopReason::End,
                execution_duration_ms: Some(100),
            },
            cx,
        );
        stream.finish_composer_run(false, cx);
    });
}

#[gpui_kit::test]
async fn issue75_menu_stability_finished_run_keeps_rows_and_cannot_open_image_picker(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "menu-finished");
    let control = vega_runtime::computer_use::test_support::connected_desktop().unwrap();
    start_run(&stream, &control, "first", cx);
    click_composer_add(window, cx);
    let (takeover, images) = {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        (
            visual
                .debug_bounds("composer-action-computer-takeover")
                .unwrap(),
            visual.debug_bounds("composer-action-images").unwrap(),
        )
    };
    finish_run(&stream, &control, "first", cx);
    cx.run_until_parked();
    VisualTestContext::from_window(window.into(), cx)
        .simulate_click(takeover.center(), Modifiers::default());
    cx.run_until_parked();
    assert!(
        !cx.did_prompt_for_paths(),
        "an expired takeover must not pick images"
    );
    assert!(!stream.read_with(cx, |stream, _| stream.attachment_import_pending));
    window
        .update(cx, |_, window, cx| {
            assert!(
                stream
                    .read(cx)
                    .input
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(window)
            );
        })
        .unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert_eq!(visual.debug_bounds("composer-action-images"), Some(images));
    assert_eq!(
        visual.debug_bounds("composer-action-computer-takeover"),
        Some(takeover)
    );
    assert_eq!(control.status().phase, ControlPhase::Stopped);

    cx.simulate_keystrokes(window.into(), "escape");
    click_composer_add(window, cx);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(
        visual
            .debug_bounds("composer-action-computer-takeover")
            .is_none()
    );
    let reopened_images = visual.debug_bounds("composer-action-images").unwrap();
    assert_eq!(reopened_images, takeover);
    visual.simulate_click(reopened_images.center(), Modifiers::default());
    visual.run_until_parked();
    assert!(cx.did_prompt_for_paths());
    cx.simulate_path_prompt_response(|options| {
        assert!(options.files && options.multiple && !options.directories);
        None
    });
    cx.run_until_parked();
    assert!(!stream.read_with(cx, |stream, _| stream.attachment_import_pending));
    assert!(stream.read_with(cx, |stream, _| stream.attachments.is_empty()));
}

#[gpui_kit::test]
async fn issue75_menu_stability_finish_between_mouse_down_and_up_keeps_action_identity(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "menu-pressed");
    let control = vega_runtime::computer_use::test_support::connected_desktop().unwrap();
    start_run(&stream, &control, "first", cx);
    click_composer_add(window, cx);
    let takeover = VisualTestContext::from_window(window.into(), cx)
        .debug_bounds("composer-action-computer-takeover")
        .unwrap();
    VisualTestContext::from_window(window.into(), cx).simulate_mouse_down(
        takeover.center(),
        MouseButton::Left,
        Modifiers::default(),
    );
    finish_run(&stream, &control, "first", cx);
    cx.run_until_parked();
    VisualTestContext::from_window(window.into(), cx).simulate_mouse_up(
        takeover.center(),
        MouseButton::Left,
        Modifiers::default(),
    );
    cx.run_until_parked();
    assert!(
        !cx.did_prompt_for_paths(),
        "mouse-up must not activate a different row"
    );
    assert_eq!(control.status().phase, ControlPhase::Stopped);
    assert_eq!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("composer-action-computer-takeover"),
        Some(takeover)
    );
}

#[gpui_kit::test]
async fn issue75_menu_stability_keyboard_highlight_remains_disabled_after_finish(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "menu-keyboard");
    let control = vega_runtime::computer_use::test_support::connected_desktop().unwrap();
    start_run(&stream, &control, "first", cx);
    click_composer_add(window, cx);
    cx.simulate_keystrokes(
        window.into(),
        "down down down down down down down down down",
    );
    let takeover = VisualTestContext::from_window(window.into(), cx)
        .debug_bounds("composer-action-computer-takeover")
        .unwrap();
    finish_run(&stream, &control, "first", cx);
    cx.run_until_parked();
    cx.simulate_keystrokes(window.into(), "enter");
    cx.run_until_parked();
    assert!(!cx.did_prompt_for_paths());
    assert_eq!(control.status().phase, ControlPhase::Stopped);
    assert_eq!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("composer-action-computer-takeover"),
        Some(takeover)
    );
}

#[gpui_kit::test]
async fn issue75_menu_stability_open_menu_cannot_pause_replacement_control(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "menu-replaced-control");
    let old = vega_runtime::computer_use::test_support::connected_desktop().unwrap();
    start_run(&stream, &old, "first", cx);
    click_composer_add(window, cx);
    let takeover = VisualTestContext::from_window(window.into(), cx)
        .debug_bounds("composer-action-computer-takeover")
        .unwrap();
    stream.update(cx, |stream, cx| {
        stream.set_computer_use_control(Some(ComputerUseControl::new()), cx);
    });
    cx.run_until_parked();
    VisualTestContext::from_window(window.into(), cx)
        .simulate_click(takeover.center(), Modifiers::default());
    cx.run_until_parked();
    assert_eq!(old.status().phase, ControlPhase::Running);
    stream.update(cx, |stream, cx| {
        stream.set_computer_use_control(Some(old.clone()), cx);
    });
    cx.run_until_parked();
    VisualTestContext::from_window(window.into(), cx)
        .simulate_click(takeover.center(), Modifiers::default());
    cx.run_until_parked();
    assert_eq!(
        old.status().phase,
        ControlPhase::Running,
        "a replaced projection cannot reactivate an old menu"
    );
    old.stop();
    vega_runtime::computer_use::test_support::finish_cleanup(&old);
    let next = vega_runtime::computer_use::test_support::connected_desktop().unwrap();
    stream.update(cx, |stream, cx| {
        stream.set_computer_use_control(Some(next.clone()), cx);
    });
    cx.run_until_parked();
    VisualTestContext::from_window(window.into(), cx)
        .simulate_click(takeover.center(), Modifiers::default());
    cx.run_until_parked();
    assert_eq!(
        next.status().phase,
        ControlPhase::Running,
        "old menu must retain its control owner"
    );
    assert!(!cx.did_prompt_for_paths());
    cx.simulate_keystrokes(window.into(), "escape");
    click_composer_add(window, cx);
    let takeover = VisualTestContext::from_window(window.into(), cx)
        .debug_bounds("composer-action-computer-takeover")
        .unwrap();
    VisualTestContext::from_window(window.into(), cx)
        .simulate_click(takeover.center(), Modifiers::default());
    cx.run_until_parked();
    assert_eq!(next.status().phase, ControlPhase::Pausing);
    next.stop();
    vega_runtime::computer_use::test_support::finish_cleanup(&next);
}

#[gpui_kit::test]
async fn issue75_menu_stability_old_press_cannot_activate_reopened_menu_for_next_run(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "menu-next-run");
    let old = vega_runtime::computer_use::test_support::connected_desktop().unwrap();
    start_run(&stream, &old, "first", cx);
    click_composer_add(window, cx);
    let takeover = VisualTestContext::from_window(window.into(), cx)
        .debug_bounds("composer-action-computer-takeover")
        .unwrap();
    VisualTestContext::from_window(window.into(), cx).simulate_mouse_down(
        takeover.center(),
        MouseButton::Left,
        Modifiers::default(),
    );
    finish_run(&stream, &old, "first", cx);
    let next = vega_runtime::computer_use::test_support::connected_desktop().unwrap();
    start_run(&stream, &next, "second", cx);
    window
        .update(cx, |_, window, cx| {
            stream.update(cx, |stream, cx| stream.open_composer_actions(window, cx));
        })
        .unwrap();
    cx.run_until_parked();
    let next_takeover = VisualTestContext::from_window(window.into(), cx)
        .debug_bounds("composer-action-computer-takeover")
        .unwrap();
    assert_eq!(next_takeover, takeover);
    VisualTestContext::from_window(window.into(), cx).simulate_mouse_up(
        takeover.center(),
        MouseButton::Left,
        Modifiers::default(),
    );
    cx.run_until_parked();
    assert_eq!(
        next.status().phase,
        ControlPhase::Running,
        "a previous menu press must not pause the next run"
    );
    assert!(!cx.did_prompt_for_paths());
    VisualTestContext::from_window(window.into(), cx)
        .simulate_click(next_takeover.center(), Modifiers::default());
    cx.run_until_parked();
    assert_eq!(next.status().phase, ControlPhase::Pausing);
    next.stop();
    vega_runtime::computer_use::test_support::finish_cleanup(&next);
}

#[gpui_kit::test]
async fn issue75_menu_stability_same_control_stream_projection_keeps_takeover_enabled(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "menu-stream-projection");
    let control = vega_runtime::computer_use::test_support::connected_desktop().unwrap();
    start_run(&stream, &control, "first", cx);
    click_composer_add(window, cx);
    let takeover = VisualTestContext::from_window(window.into(), cx)
        .debug_bounds("composer-action-computer-takeover")
        .unwrap();
    for _ in 0..3 {
        stream.update(cx, |stream, cx| {
            stream.set_computer_use_control(Some(control.clone()), cx);
        });
        cx.run_until_parked();
        assert_eq!(
            VisualTestContext::from_window(window.into(), cx)
                .debug_bounds("composer-action-computer-takeover"),
            Some(takeover)
        );
    }
    VisualTestContext::from_window(window.into(), cx)
        .simulate_click(takeover.center(), Modifiers::default());
    cx.run_until_parked();
    assert_eq!(control.status().phase, ControlPhase::Pausing);
    assert!(!cx.did_prompt_for_paths());
}
