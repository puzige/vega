use super::*;
use gpui_kit::{Bounds, Pixels, VisualTestContext};

fn bounds(
    window: WindowHandle<StreamHarness>,
    selector: &'static str,
    cx: &mut TestAppContext,
) -> Option<Bounds<Pixels>> {
    cx.run_until_parked();
    VisualTestContext::from_window(window.into(), cx).debug_bounds(selector)
}

fn assert_error_above_composer(
    window: WindowHandle<StreamHarness>,
    cx: &mut TestAppContext,
) -> (Bounds<Pixels>, Bounds<Pixels>) {
    let error =
        bounds(window, "conversation-controller-error", cx).expect("controller error element");
    let composer = bounds(window, "composer-shell", cx).expect("composer card");
    assert!(
        error.bottom() <= composer.top(),
        "controller error must be above the Composer card: error={error:?} composer={composer:?}"
    );
    assert!(
        (f32::from(error.size.width) - f32::from(composer.size.width)).abs() <= 1.0,
        "controller error must use the Composer content width: error={error:?} composer={composer:?}"
    );
    assert!(
        (f32::from(error.left()) - f32::from(composer.left())).abs() <= 1.0,
        "controller error must align with the Composer content column: error={error:?} composer={composer:?}"
    );
    (error, composer)
}

#[gpui_kit::test]
async fn issue79_errors_stay_above_composer_across_draft_and_session_routes(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "issue79-composer-error");
    stream.update(cx, |stream, cx| stream.set_draft_route(true, cx));
    let draft_composer = bounds(window, "composer-shell", cx).expect("draft composer card");
    assert!(
        bounds(window, "composer-utility-bar", cx).is_some(),
        "the new-task utility bar remains visible"
    );

    stream.update(cx, |stream, cx| {
        stream
            .input
            .update(cx, |input, cx| input.set_text("keep this draft", cx));
        stream.submit_message(cx);
        stream.apply_credential_error(cx);
    });
    let credential_state = stream.read_with(cx, |stream, cx| {
        (
            stream.composer_submit_pending,
            stream.input.read(cx).text().to_string(),
            stream.entries.len(),
            stream.composer_history.len(),
        )
    });
    assert_eq!(credential_state, (false, "keep this draft".into(), 0, 0));
    let (credential_error, _) = assert_error_above_composer(window, cx);
    let utility_bar = bounds(window, "composer-utility-bar", cx).expect("draft utility bar");
    assert!(
        utility_bar.bottom() <= credential_error.top(),
        "the error follows the new-task utility bar"
    );

    stream.update(cx, ConversationStream::begin_composer_run);
    assert!(bounds(window, "conversation-controller-error", cx).is_none());
    assert!(bounds(window, "composer-stop", cx).is_some());
    let cleared_draft_composer =
        bounds(window, "composer-shell", cx).expect("cleared draft composer");
    assert!(
        (f32::from(cleared_draft_composer.top()) - f32::from(draft_composer.top())).abs() <= 1.0,
        "clearing the error leaves no reserved slot"
    );
    stream.update(cx, |stream, cx| stream.finish_composer_run(false, cx));

    stream.update(cx, |stream, cx| {
        stream.submit_message(cx);
        stream.accept_composer_submission("keep this draft", cx);
        stream.input.update(cx, |input, cx| {
            input.set_text("second accepted message", cx)
        });
        stream.submit_message(cx);
        stream.accept_composer_submission("second accepted message", cx);
    });
    assert!(bounds(window, "composer-utility-bar", cx).is_none());
    let successful_entries = stream.read_with(cx, |stream, cx| {
        (
            stream.entries.len(),
            stream.input.read(cx).text().to_string(),
            stream.composer_history.clone(),
        )
    });
    assert_eq!(
        successful_entries,
        (
            2,
            String::new(),
            vec!["keep this draft".into(), "second accepted message".into()]
        )
    );

    stream.update(cx, |stream, cx| {
        stream
            .input
            .update(cx, |input, cx| input.set_text("总结 @missing.txt", cx));
        stream.submit_message(cx);
        stream.reject_composer_submission(cx);
        stream.apply_reference_error(FileReferenceFailureCode::Missing, cx);
    });
    let reference_state = stream.read_with(cx, |stream, cx| {
        (
            stream.composer_submit_pending,
            stream.input.read(cx).text().to_string(),
            stream.entries.len(),
            stream.controller_error.clone(),
        )
    });
    assert!(!reference_state.0);
    assert_eq!(reference_state.1, "总结 @missing.txt");
    assert_eq!(reference_state.2, 2);
    assert_eq!(
        reference_state.3.as_deref(),
        Some(FileReferenceFailureCode::Missing.message())
    );
    assert_error_above_composer(window, cx);

    let diagnostic = vega_conversation::types::McpServerDiagnostic {
        server_id: "01J00000000000000000000000".into(),
        code: "authorization_required".into(),
    };
    stream.update(cx, |stream, cx| {
        stream.apply_mcp_unavailable(&[diagnostic], cx)
    });
    let composer = bounds(window, "composer-shell", cx).expect("session composer card");
    let warning = bounds(window, "conversation-mcp-warning", cx).expect("MCP warning");
    assert!(
        composer.bottom() <= warning.top(),
        "the MCP warning stays below the Composer card"
    );
    assert!(bounds(window, "conversation-controller-error", cx).is_some());

    stream.update(cx, ConversationStream::begin_composer_run);
    assert!(bounds(window, "conversation-controller-error", cx).is_none());
    assert!(bounds(window, "conversation-mcp-warning", cx).is_none());
    assert!(bounds(window, "composer-stop", cx).is_some());
    stream.update(cx, |stream, cx| stream.finish_composer_run(false, cx));
    assert!(bounds(window, "composer-send", cx).is_some());
}
