use super::*;
use gpui_kit::VisualTestContext;
use vega_conversation::types::{ComputerUseControl, ControlPhase};

fn picture() -> ImageAttachment {
    ImageAttachment::from_bytes(
        include_bytes!("../../../../../assets/logo/raster/vega-icon-f1-original.png").to_vec(),
    )
    .unwrap()
}

#[gpui_kit::test]
async fn issue75_usability_screenshot_stays_visible_outside_folded_process_and_replays_once(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "screenshots");
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "assistant".into(),
                seq: 1,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ThinkingDelta {
                message_id: "assistant".into(),
                delta: "Capture the owned fixture".into(),
            },
            cx,
        );
        for _ in 0..2 {
            stream.apply_event(
                ConversationEvent::ScreenshotDelivered {
                    message_id: "assistant".into(),
                    call_id: "capture".into(),
                    image: picture(),
                },
                cx,
            );
        }
        stream.apply_event(
            ConversationEvent::ScreenshotDelivered {
                message_id: "other-assistant".into(),
                call_id: "wrong".into(),
                image: picture(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::MessageFinished {
                stop_reason: vega_conversation::types::ConversationStopReason::End,
                message_id: "assistant".into(),
                execution_duration_ms: Some(100),
            },
            cx,
        );
        assert_eq!(
            stream
                .entries
                .iter()
                .filter(|entry| matches!(entry, StreamEntry::Screenshot { .. }))
                .count(),
            1
        );
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("assistant-screenshot").is_some());
    let expand = visual.debug_bounds("screenshot-expand").unwrap();
    visual.simulate_click(expand.center(), Default::default());
    visual.run_until_parked();
    let save = visual.debug_bounds("screenshot-save").unwrap();
    visual.simulate_click(save.center(), Default::default());
    visual.run_until_parked();
    assert!(cx.did_prompt_for_new_path());
    cx.simulate_new_path_selection(|parent| {
        assert_ne!(parent, std::path::Path::new("/"));
        None
    });
    cx.run_until_parked();
    let save = VisualTestContext::from_window(window.into(), cx)
        .debug_bounds("screenshot-save")
        .unwrap();
    VisualTestContext::from_window(window.into(), cx)
        .simulate_click(save.center(), Default::default());
    cx.run_until_parked();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("screenshot.png");
    cx.simulate_new_path_selection(|_| Some(path.clone()));
    cx.run_until_parked();
    assert_eq!(std::fs::read(path).unwrap(), picture().bytes());
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("computer-use-control-bar")
            .is_none()
    );
}

#[gpui_kit::test]
async fn issue75_usability_only_actionable_notice_is_in_conversation_and_new_run_clears_it(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "notice");
    let control = ComputerUseControl::new();
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "first".into(),
                seq: 1,
            },
            cx,
        );
        stream.set_computer_use_control(Some(control.clone()), cx);
    });
    cx.run_until_parked();
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("conversation-computer-use-notice")
            .is_none()
    );
    control.pause();
    vega_runtime::computer_use::test_support::finish_cleanup(&control);
    stream.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    assert_eq!(control.status().phase, ControlPhase::Paused);
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("conversation-computer-use-notice")
            .is_some()
    );
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageFinished {
                stop_reason: vega_conversation::types::ConversationStopReason::End,
                message_id: "first".into(),
                execution_duration_ms: None,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "second".into(),
                seq: 2,
            },
            cx,
        );
    });
    cx.run_until_parked();
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("conversation-computer-use-notice")
            .is_none()
    );
    assert!(stream.read_with(cx, |stream, _| stream.computer_use_control.is_none()));
    control.stop();
    vega_runtime::computer_use::test_support::finish_cleanup(&control);
}

#[gpui_kit::test]
async fn issue75_usability_takeover_uses_existing_menu_and_resume_stays_in_conversation(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "takeover");
    let control = vega_runtime::computer_use::test_support::connected_desktop().unwrap();
    let captured_epoch = control.status().epoch;
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "active".into(),
                seq: 1,
            },
            cx,
        );
        stream.set_computer_use_control(Some(control.clone()), cx);
    });
    cx.run_until_parked();
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("conversation-computer-use-notice")
            .is_none()
    );
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("composer-action-computer-takeover")
            .is_none()
    );
    click_composer_add(window, cx);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let takeover = visual
        .debug_bounds("composer-action-computer-takeover")
        .unwrap();
    visual.simulate_click(takeover.center(), Default::default());
    visual.run_until_parked();
    assert_eq!(control.status().phase, ControlPhase::Pausing);
    vega_runtime::computer_use::test_support::finish_cleanup(&control);
    stream.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let resume = visual.debug_bounds("conversation-computer-resume").unwrap();
    visual.simulate_click(resume.center(), Default::default());
    visual.run_until_parked();
    assert_eq!(control.status().phase, ControlPhase::Disconnected);
    assert!(control.pause_at(captured_epoch).is_err());
    assert!(
        visual
            .debug_bounds("conversation-computer-use-notice")
            .is_none()
    );
    assert!(visual.debug_bounds("computer-use-control-bar").is_none());
    control.stop();
    vega_runtime::computer_use::test_support::finish_cleanup(&control);
}

#[gpui_kit::test]
async fn issue75_usability_permission_recovery_and_browser_approval_stay_reachable_in_conversation(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "permissions");
    let permissions = vega_runtime::computer_use::test_support::permission_failure().unwrap();
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "permissions".into(),
                seq: 1,
            },
            cx,
        );
        stream.set_computer_use_control(Some(permissions), cx);
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(
        visual
            .debug_bounds("conversation-accessibility-settings")
            .is_some()
    );
    assert!(
        visual
            .debug_bounds("conversation-screen-recording-settings")
            .is_some()
    );
    assert!(visual.debug_bounds("computer-use-control-bar").is_none());
    let browser = vega_runtime::computer_use::test_support::pending_browser().unwrap();
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "browser".into(),
                seq: 2,
            },
            cx,
        );
        stream.set_computer_use_control(Some(browser.clone()), cx);
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let approve = visual.debug_bounds("conversation-browser-approve").unwrap();
    visual.simulate_click(approve.center(), Default::default());
    visual.run_until_parked();
    assert_eq!(browser.status().phase, ControlPhase::Running);
    assert!(
        visual
            .debug_bounds("conversation-computer-use-notice")
            .is_none()
    );
    browser.stop();
    vega_runtime::computer_use::test_support::finish_cleanup(&browser);
}

#[gpui_kit::test]
async fn issue75_readonly_permissions_guidance_settings_and_diagnostic_are_scoped(
    cx: &mut TestAppContext,
) {
    for (index, ax, sr, label) in [
        (1, false, true, "辅助功能权限"),
        (2, true, false, "屏幕录制权限"),
        (3, false, false, "辅助功能和屏幕录制权限"),
    ] {
        let (window, stream, _) =
            open_controller_stream(cx, &format!("permission-details-{index}"));
        let control =
            vega_runtime::computer_use::test_support::permission_failure_for(ax, sr).unwrap();
        let error = control.status().error.unwrap();
        assert!(error.user_message().contains(label));
        assert!(!error.user_message().contains("executor="));
        stream.update(cx, |stream, cx| {
            stream.apply_event(
                ConversationEvent::MessageStarted {
                    message_id: format!("permission-{index}"),
                    seq: index,
                },
                cx,
            );
            stream.set_computer_use_control(Some(control.clone()), cx);
        });
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        assert_eq!(
            visual
                .debug_bounds("conversation-accessibility-settings")
                .is_some(),
            !ax
        );
        assert_eq!(
            visual
                .debug_bounds("conversation-screen-recording-settings")
                .is_some(),
            !sr
        );
        assert!(
            visual
                .debug_bounds("conversation-computer-use-guidance")
                .is_some()
        );
        assert!(
            visual
                .debug_bounds("conversation-computer-use-diagnostic")
                .is_none()
        );
        assert!(visual.debug_bounds("computer-use-control-bar").is_none());
        window
            .update(cx, |_, window, cx| {
                let focus = stream.read(cx).computer_use_diagnostic_focus.clone();
                focus.focus(window, cx);
            })
            .unwrap();
        cx.simulate_keystrokes(window.into(), "enter");
        cx.run_until_parked();
        assert!(
            VisualTestContext::from_window(window.into(), cx)
                .debug_bounds("conversation-computer-use-diagnostic")
                .is_some()
        );
        cx.simulate_keystrokes(window.into(), "space");
        cx.run_until_parked();
        assert!(
            VisualTestContext::from_window(window.into(), cx)
                .debug_bounds("conversation-computer-use-diagnostic")
                .is_none()
        );
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let toggle = visual
            .debug_bounds("conversation-computer-use-diagnostic-toggle")
            .unwrap();
        visual.simulate_click(toggle.center(), Default::default());
        visual.run_until_parked();
        assert!(
            visual
                .debug_bounds("conversation-computer-use-diagnostic")
                .is_some()
        );
        let generation = stream.read_with(cx, |stream, _| stream.computer_use_notice_generation);
        let key = (control.status().epoch, error.to_string());
        stream.update(cx, |stream, cx| {
            stream.apply_event(
                ConversationEvent::MessageFinished {
                    stop_reason: vega_conversation::types::ConversationStopReason::End,
                    message_id: format!("permission-{index}"),
                    execution_duration_ms: None,
                },
                cx,
            );
            stream.apply_event(
                ConversationEvent::MessageStarted {
                    message_id: format!("next-{index}"),
                    seq: 10 + index,
                },
                cx,
            );
            stream.toggle_computer_use_diagnostic(generation, key, cx);
        });
        cx.run_until_parked();
        assert!(stream.read_with(cx, |stream, _| stream.computer_use_diagnostic.is_none()));
        assert!(
            VisualTestContext::from_window(window.into(), cx)
                .debug_bounds("conversation-computer-use-notice")
                .is_none()
        );
    }
}
