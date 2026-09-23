use super::*;
use gpui_kit::{Modifiers, VisualTestContext};

fn copy_selector(role: &str) -> &'static str {
    if role == "user" {
        "message-copy-user"
    } else {
        "message-copy-assistant"
    }
}

fn copy_bounds(
    window: WindowHandle<StreamHarness>,
    role: &str,
    cx: &mut TestAppContext,
) -> Option<gpui_kit::Bounds<gpui_kit::Pixels>> {
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.run_until_parked();
    visual.debug_bounds(copy_selector(role))
}

fn click_copy(window: WindowHandle<StreamHarness>, role: &str, cx: &mut TestAppContext) {
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.run_until_parked();
    let button = visual
        .debug_bounds(copy_selector(role))
        .expect("copy action");
    visual.simulate_mouse_move(button.center(), None, Modifiers::default());
    visual.simulate_click(button.center(), Modifiers::default());
    visual.run_until_parked();
}

fn clipboard(cx: &mut TestAppContext) -> String {
    cx.update(|cx| {
        cx.read_from_clipboard()
            .and_then(|item| item.text())
            .unwrap_or_default()
    })
}

fn body_selector(role: &str) -> &'static str {
    if role == "user" {
        "user-message-bubble"
    } else {
        "assistant-message"
    }
}

fn assert_message_body_present(
    window: WindowHandle<StreamHarness>,
    role: &str,
    cx: &mut TestAppContext,
) {
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.run_until_parked();
    assert!(
        visual.debug_bounds(body_selector(role)).is_some(),
        "message body must keep rendering without the copy action"
    );
}

#[gpui_kit::test]
async fn issue78_hover_copy_history_preserves_raw_sources_and_draft(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "copy-history");
    let user = "中文 first\n\nlast\n\n";
    let answer = "**中文**\n\n- [链接](https://example.test/a)\n\n```rust\nlet x = 1;\n```\n";
    stream.update(cx, |stream, cx| {
        stream.apply_history_page(
            hydration_page(
                vec![hydration_user(1, user), hydration_assistant(2, answer)],
                None,
            ),
            cx,
        );
        stream
            .input
            .update(cx, |input, cx| input.set_text("unsent draft", cx));
    });
    cx.run_until_parked();
    if MESSAGE_COPY_ACTIONS_ENABLED {
        click_copy(window, "user", cx);
        assert_eq!(clipboard(cx), user);
        click_copy(window, "assistant", cx);
        assert_eq!(clipboard(cx), answer);
    } else {
        // Disabled contract: no action row is mounted on either side, the
        // bodies keep rendering, and no copy reaches the clipboard.
        assert!(copy_bounds(window, "user", cx).is_none());
        assert!(copy_bounds(window, "assistant", cx).is_none());
        assert_message_body_present(window, "user", cx);
        assert_message_body_present(window, "assistant", cx);
        cx.update(|cx| {
            cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("sentinel".into()))
        });
        assert_eq!(clipboard(cx), "sentinel");
    }
    assert_eq!(
        stream.read_with(cx, |stream, cx| stream.input.read(cx).text().to_string()),
        "unsent draft"
    );
}

#[gpui_kit::test]
async fn issue78_hover_copy_live_reads_latest_delta_and_hides_empty(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "copy-live");
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "m".into(),
                seq: 1,
            },
            cx,
        )
    });
    cx.run_until_parked();
    // An empty assistant turn never exposes a copy action, in either state.
    assert!(copy_bounds(window, "assistant", cx).is_none());
    let mut expected = String::new();
    for delta in ["**first**\n", "\n[second](https://example.test)\n"] {
        expected.push_str(delta);
        stream.update(cx, |stream, cx| {
            stream.apply_event(
                ConversationEvent::TextDelta {
                    message_id: "m".into(),
                    delta: delta.into(),
                },
                cx,
            )
        });
        cx.run_until_parked();
        if MESSAGE_COPY_ACTIONS_ENABLED {
            click_copy(window, "assistant", cx);
            assert_eq!(clipboard(cx), expected);
        } else {
            assert!(copy_bounds(window, "assistant", cx).is_none());
            assert_message_body_present(window, "assistant", cx);
        }
    }
}

#[gpui_kit::test]
async fn issue78_hover_copy_new_user_keeps_trailing_newlines_and_empty_failure_has_no_action(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "copy-new-user");
    let original = "用户\n\n尾行\n\n";
    stream.update(cx, |stream, cx| {
        stream.composer_submit_pending = true;
        stream.accept_composer_submission(original, cx);
        stream.apply_history_page(
            hydration_page(
                vec![HistoryEntry::AssistantText {
                    seq: 1,
                    message_id: "empty-failure".into(),
                    content: String::new(),
                    status: vega_conversation::history::AssistantStatus::Failed,
                }],
                None,
            ),
            cx,
        );
    });
    cx.run_until_parked();
    if MESSAGE_COPY_ACTIONS_ENABLED {
        click_copy(window, "user", cx);
        assert_eq!(clipboard(cx), original);
    } else {
        assert!(copy_bounds(window, "user", cx).is_none());
        assert_message_body_present(window, "user", cx);
    }
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("assistant-run-failure").is_some());
    assert!(visual.debug_bounds("message-copy-assistant").is_none());
}
