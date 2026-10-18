use super::*;

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
