use super::*;

#[gpui::test]
async fn r11_composer_mode_ack_preserves_later_edits_and_terminal_precedence(
    cx: &mut TestAppContext,
) {
    let (window, stream, events) = open_controller_stream(cx, "actions-fence");
    stream.update(cx, |stream, cx| {
        stream
            .input
            .update(cx, |input, cx| input.set_text("/plan old draft", cx))
    });
    focus_composer(window, &stream, cx);
    cx.simulate_keystrokes(window.into(), "enter");
    assert_eq!(events.lock().expect("events").len(), 1);
    assert_eq!(
        stream.read_with(cx, |stream, cx| stream.input.read(cx).text().to_owned()),
        "/plan old draft",
        "no optimistic prefix deletion"
    );
    stream.update(cx, |stream, cx| {
        stream
            .input
            .update(cx, |input, cx| input.set_text("later unsent draft", cx));
        let mut persisted = stream.thread.clone();
        persisted.mode = ThreadMode::Plan;
        stream.apply_thread(persisted, cx);
    });
    assert_eq!(
        stream.read_with(cx, |stream, cx| stream.input.read(cx).text().to_owned()),
        "later unsent draft"
    );
    stream.update(cx, |stream, cx| {
        stream.begin_composer_run(cx);
        assert!(stream.actions.running);
        assert!(
            !stream.finish_composer_run(false, cx),
            "spawn failure release is not a cancellation"
        );
        assert!(!stream.actions.running);
        stream.begin_composer_run(cx);
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "finished-before-click".into(),
                seq: 1,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::MessageFinished {
                message_id: "finished-before-click".into(),
                stop_reason: vega_conversation::types::ConversationStopReason::End,
            },
            cx,
        );
        assert!(
            !stream.finish_composer_run(true, cx),
            "accepted successful terminal outranks a later token cancellation"
        );
        assert!(!stream.actions.running);
        stream.begin_composer_run(cx);
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "disconnected".into(),
                seq: 2,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "disconnected".into(),
                delta: "retained disconnected partial".into(),
            },
            cx,
        );
        let before = stream.entries.len();
        assert!(!stream.finish_composer_run(false, cx));
        assert!(stream.active_agent_message.is_none());
        assert!(!stream.actions.running);
        assert_eq!(
            stream.entries.len(),
            before,
            "terminal presentation release retains partial message"
        );
    });
}

#[gpui::test]
async fn r11_composer_marked_text_is_not_a_command(cx: &mut TestAppContext) {
    use gpui::EntityInputHandler;
    let (window, stream, events) = open_controller_stream(cx, "actions-ime");
    focus_composer(window, &stream, cx);
    window
        .update(cx, |_, window, cx| {
            let input = stream.read(cx).composer_input();
            input.update(cx, |input, cx| {
                input.replace_and_mark_text_in_range(None, "/plan", Some(5..5), window, cx)
            });
        })
        .expect("platform marked text");
    cx.run_until_parked();
    assert!(!stream.read_with(cx, |stream, _| stream.actions.visible()));
    assert!(stream.read_with(cx, |stream, cx| stream.input.read(cx).is_composing()));
    // Exercise the new scoped handlers directly while IME owns the text. Native
    // OS candidate windows are outside the headless test platform's scope.
    window
        .update(cx, |_, window, cx| {
            stream.update(cx, |stream, cx| {
                stream.accept_composer_action(&AcceptComposerAction, window, cx);
                stream.next_composer_action(&NextComposerAction, window, cx);
                stream.previous_composer_action(&PreviousComposerAction, window, cx);
                stream.close_composer_actions(&CloseComposerActions, window, cx);
                stream.next_composer_control(&NextComposerControl, window, cx);
                stream.open_composer_actions(window, cx);
                assert!(!stream.actions.visible());
                assert!(stream.input.read(cx).focus_handle(cx).is_focused(window));
            })
        })
        .expect("scoped IME guards");
    assert!(events.lock().expect("events").is_empty());
    assert_eq!(
        stream.read_with(cx, |stream, cx| stream.input.read(cx).text().to_owned()),
        "/plan"
    );
    assert!(stream.read_with(cx, |stream, cx| stream.input.read(cx).is_composing()));
}
