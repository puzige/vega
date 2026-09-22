use super::*;
use gpui_kit::{Modifiers, VisualTestContext};

fn click_copy(window: WindowHandle<StreamHarness>, role: &str, cx: &mut TestAppContext) {
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.run_until_parked();
    let button = visual
        .debug_bounds(if role == "user" {
            "message-copy-user"
        } else {
            "message-copy-assistant"
        })
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
    click_copy(window, "user", cx);
    assert_eq!(clipboard(cx), user);
    click_copy(window, "assistant", cx);
    assert_eq!(clipboard(cx), answer);
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
    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        assert!(visual.debug_bounds("message-copy-assistant").is_none());
    }
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
        click_copy(window, "assistant", cx);
        assert_eq!(clipboard(cx), expected);
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
    click_copy(window, "user", cx);
    assert_eq!(clipboard(cx), original);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("assistant-run-failure").is_some());
    assert!(visual.debug_bounds("message-copy-assistant").is_none());
}
