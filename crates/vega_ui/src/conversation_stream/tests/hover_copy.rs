use super::*;
use gpui_kit::{Modifiers, VisualTestContext};

fn copy_selector(role: &str) -> &'static str {
    if role == "user" {
        "message-copy-user"
    } else {
        "message-copy-assistant"
    }
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
