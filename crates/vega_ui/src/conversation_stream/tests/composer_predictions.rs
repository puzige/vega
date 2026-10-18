use super::*;
use gpui_kit::EntityInputHandler;
use vega_conversation::types::ComposerPredictionFence;

fn install_prediction(
    stream: &Entity<ConversationStream>,
    message: &str,
    cx: &mut TestAppContext,
) -> (ComposerPredictionFence, CancellationToken) {
    stream.update(cx, |stream, cx| {
        let (fence, cancel) = stream.begin_composer_prediction(message, cx).unwrap();
        assert!(stream.apply_composer_prediction(&fence, "请继续检查中文😀边界".into(), cx));
        assert!(stream.input.read(cx).text().is_empty());
        assert!(!stream.can_submit_message(cx));
        (fence, cancel)
    })
}

#[gpui_kit::test]
async fn prediction_bare_accept_keys_fill_editable_draft_once_without_submission(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "prediction-keys");
    let submissions = Arc::new(Mutex::new(Vec::new()));
    let captured = submissions.clone();
    let _subscription = cx.update(|cx| {
        cx.subscribe(&stream, move |_, event: &ComposerSubmitted, _| {
            captured.lock().unwrap().push(event.content.clone())
        })
    });
    focus_composer(window, &stream, cx);
    for key in ["tab", "right", "enter"] {
        stream.update(cx, |stream, cx| stream.input.update(cx, TextInput::clear));
        let (_, cancel) = install_prediction(&stream, key, cx);
        cx.run_until_parked();
        assert!(submissions.lock().unwrap().is_empty());
        cx.simulate_keystrokes(window.into(), key);
        cx.run_until_parked();
        assert!(submissions.lock().unwrap().is_empty());
        stream.read_with(cx, |stream, cx| {
            assert_eq!(stream.input.read(cx).text(), "请继续检查中文😀边界");
            assert!(stream.input.read(cx).prediction_text().is_none());
            assert!(stream.can_submit_message(cx));
        });
        window
            .update(cx, |_, window, cx| {
                let input = stream.read(cx).input.clone();
                input.update(cx, |input, cx| {
                    let selection = input.selected_text_range(false, window, cx).unwrap();
                    let end = "请继续检查中文😀边界".encode_utf16().count();
                    assert_eq!(selection.range, end..end);
                    input.replace_text_in_range(None, "，再确认", window, cx);
                    assert_eq!(input.text(), "请继续检查中文😀边界，再确认");
                });
            })
            .unwrap();
        assert!(cancel.is_cancelled());
    }
}

#[gpui_kit::test]
async fn prediction_modifiers_keep_original_keys_and_escape_invalidates_result(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "prediction-modifiers");
    focus_composer(window, &stream, cx);
    let (fence, cancel) = install_prediction(&stream, "first", cx);
    cx.run_until_parked();
    for key in [
        "shift-right",
        "alt-right",
        "ctrl-right",
        "cmd-right",
        "cmd-enter",
    ] {
        cx.simulate_keystrokes(window.into(), key);
        cx.run_until_parked();
        assert!(stream.read_with(cx, |stream, cx| stream.input.read(cx).text().is_empty()));
    }
    cx.simulate_keystrokes(window.into(), "escape");
    cx.run_until_parked();
    assert!(cancel.is_cancelled());
    stream.update(cx, |stream, cx| {
        assert!(!stream.apply_composer_prediction(&fence, "late".into(), cx));
        assert!(stream.input.read(cx).text().is_empty());
        assert!(stream.input.read(cx).prediction_text().is_none());
    });
    cx.simulate_keystrokes(window.into(), "shift-enter");
    cx.run_until_parked();
    assert_eq!(
        stream.read_with(cx, |stream, cx| stream.input.read(cx).text().to_owned()),
        "\n"
    );
    stream.update(cx, |stream, cx| stream.input.update(cx, TextInput::clear));
    install_prediction(&stream, "second", cx);
    cx.run_until_parked();
    cx.simulate_keystrokes(window.into(), "shift-enter");
    cx.run_until_parked();
    assert_eq!(
        stream.read_with(cx, |stream, cx| stream.input.read(cx).text().to_owned()),
        "\n"
    );
    let submits = Arc::new(Mutex::new(Vec::new()));
    let captured = submits.clone();
    let _subscription = cx.update(|cx| {
        cx.subscribe(&stream, move |_, event: &ComposerSubmitted, _| {
            captured.lock().unwrap().push(event.content.clone())
        })
    });
    stream.update(cx, |stream, cx| {
        stream
            .input
            .update(cx, |input, cx| input.set_text("原始发送", cx))
    });
    cx.run_until_parked();
    cx.simulate_keystrokes(window.into(), "enter");
    assert_eq!(*submits.lock().unwrap(), ["原始发送"]);
}

#[gpui_kit::test]
async fn prediction_ime_and_utf16_queries_never_expose_or_accept_ghost(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "prediction-ime");
    focus_composer(window, &stream, cx);
    let (fence, cancel) = install_prediction(&stream, "ime", cx);
    cx.run_until_parked();
    window
        .update(cx, |_, window, cx| {
            let input = stream.read(cx).input.clone();
            input.update(cx, |input, cx| {
                let mut actual = None;
                assert_eq!(
                    input
                        .text_for_range(0..200, &mut actual, window, cx)
                        .as_deref(),
                    Some("")
                );
                assert_eq!(actual, Some(0..0));
                input.replace_and_mark_text_in_range(Some(0..0), "中😀文", Some(1..3), window, cx);
                assert!(input.is_composing());
                assert!(input.prediction_text().is_none());
                assert_eq!(input.marked_text_range(window, cx), Some(0..4));
                assert_eq!(
                    input.selected_text_range(false, window, cx).unwrap().range,
                    1..3
                );
            });
        })
        .unwrap();
    cx.run_until_parked();
    for key in ["enter", "tab"] {
        cx.simulate_keystrokes(window.into(), key);
        cx.run_until_parked();
        assert!(stream.read_with(cx, |stream, _| !stream.composer_submit_pending));
        assert_eq!(
            stream.read_with(cx, |stream, cx| stream.input.read(cx).text().to_owned()),
            "中😀文"
        );
    }
    assert!(cancel.is_cancelled());
    window
        .update(cx, |_, window, cx| {
            let input = stream.read(cx).input.clone();
            input.update(cx, |input, cx| input.unmark_text(window, cx));
        })
        .unwrap();
    stream.update(cx, |stream, cx| {
        assert!(!stream.apply_composer_prediction(&fence, "stale".into(), cx))
    });
}

#[gpui_kit::test]
async fn prediction_revisions_preserve_whitespace_and_never_revive_after_delete(
    cx: &mut TestAppContext,
) {
    let (_window, stream, _) = open_controller_stream(cx, "prediction-revisions");
    let (fence, cancel) = stream.update(cx, |stream, cx| {
        stream.begin_composer_prediction("completed", cx).unwrap()
    });
    stream.update(cx, |stream, cx| {
        stream.input.update(cx, |input, cx| input.set_text(" ", cx))
    });
    cx.run_until_parked();
    assert!(cancel.is_cancelled());
    stream.update(cx, |stream, cx| {
        assert!(!stream.apply_composer_prediction(&fence, "late".into(), cx));
        assert!(stream.begin_composer_prediction("another", cx).is_none());
        assert_eq!(stream.input.read(cx).text(), " ");
        stream.input.update(cx, TextInput::clear);
        assert!(!stream.apply_composer_prediction(&fence, "late again".into(), cx));
        assert!(stream.begin_composer_prediction("completed", cx).is_none());
    });
}

#[gpui_kit::test]
async fn prediction_blockers_cancel_pending_and_visible_results(cx: &mut TestAppContext) {
    type Blocker = fn(&mut ConversationStream);
    let blockers: &[Blocker] = &[
        |stream| stream.attachment_import_pending = true,
        |stream| stream.pending_queue.busy = true,
        |stream| stream.composer_submit_pending = true,
        |stream| stream.trusted_action_busy = true,
        |stream| stream.model_selection_pending = Some((1, "other".into())),
        |stream| stream.reading_preview = true,
        |stream| stream.actions.running = true,
        |stream| stream.thread.mode = ThreadMode::Plan,
        |stream| stream.thread.id = "other-thread".into(),
        |stream| stream.composer_defaults.model = "other-model".into(),
        |stream| stream.thread.backend = TaskBackend::Codex,
        |stream| stream.thread.parent_thread_id = Some("parent".into()),
        |stream| stream.delegated_run_active = true,
        |stream| stream.context_control.open = true,
        |stream| {
            stream
                .pending_queue
                .messages
                .push(vega_conversation::types::PendingMessage {
                    id: "pending".into(),
                    thread_id: stream.thread.id.clone(),
                    generation: "generation".into(),
                    behavior: PendingMessageBehavior::Steer,
                    content: "steer".into(),
                    images: Vec::new(),
                })
        },
    ];
    for (index, blocker) in blockers.iter().enumerate() {
        let (_window, stream, _) = open_controller_stream(cx, &format!("blocker-{index}"));
        let (fence, cancel) = install_prediction(&stream, "completed", cx);
        stream.update(cx, |stream, cx| {
            blocker(stream);
            cx.notify();
        });
        cx.run_until_parked();
        assert!(cancel.is_cancelled(), "blocker {index}");
        stream.update(cx, |stream, cx| {
            assert!(!stream.apply_composer_prediction(&fence, "late".into(), cx));
            assert!(stream.input.read(cx).prediction_text().is_none());
            assert!(!stream.composer_prediction_eligible(cx));
        });
    }
}

#[gpui_kit::test]
async fn prediction_setting_updates_all_cached_streams_without_catchup(cx: &mut TestAppContext) {
    let (_window, first, _) = open_controller_stream(cx, "prediction-setting-first");
    let (_window, second, _) = open_controller_stream(cx, "prediction-setting-second");
    let (first_fence, first_cancel) = install_prediction(&first, "first", cx);
    let (second_fence, second_cancel) = install_prediction(&second, "second", cx);
    cx.update(|cx| set_composer_predictions_enabled(false, cx));
    cx.run_until_parked();
    assert!(first_cancel.is_cancelled() && second_cancel.is_cancelled());
    for (stream, fence) in [(&first, &first_fence), (&second, &second_fence)] {
        stream.update(cx, |stream, cx| {
            assert!(!stream.apply_composer_prediction(fence, "late".into(), cx));
            assert!(stream.begin_composer_prediction("disabled", cx).is_none());
        });
    }
    cx.update(|cx| set_composer_predictions_enabled(true, cx));
    cx.run_until_parked();
    first.update(cx, |stream, cx| {
        assert!(stream.input.read(cx).prediction_text().is_none());
        assert!(!stream.apply_composer_prediction(&first_fence, "stale".into(), cx));
        assert!(
            stream
                .begin_composer_prediction("next-completion", cx)
                .is_some()
        );
    });
}

#[gpui_kit::test]
async fn prediction_preserves_tab_traversal_and_shift_tab_with_a_visible_suggestion(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "prediction-tab-traversal");
    focus_composer(window, &stream, cx);
    cx.simulate_keystrokes(window.into(), "tab");
    window
        .update(cx, |_, window, cx| {
            assert!(stream.read(cx).action_focus[0].is_focused(window))
        })
        .unwrap();
    focus_composer(window, &stream, cx);
    install_prediction(&stream, "tab-traversal", cx);
    cx.run_until_parked();
    cx.simulate_keystrokes(window.into(), "shift-tab");
    window
        .update(cx, |_, window, cx| {
            assert!(
                !stream
                    .read(cx)
                    .input
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(window)
            );
            assert!(stream.read(cx).input.read(cx).text().is_empty());
            assert!(stream.read(cx).input.read(cx).prediction_text().is_some());
        })
        .unwrap();
}

gpui_kit::actions!(prediction_shortcut_tests, [InputBack, WindowBack]);

struct ShortcutHarness {
    stream: Entity<ConversationStream>,
    input_calls: Arc<Mutex<usize>>,
    window_calls: Arc<Mutex<usize>>,
}

impl Render for ShortcutHarness {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context("VegaWindow")
            .size_full()
            .on_action(
                cx.listener(|this, _: &InputBack, _, _| *this.input_calls.lock().unwrap() += 1),
            )
            .on_action(
                cx.listener(|this, _: &WindowBack, _, _| *this.window_calls.lock().unwrap() += 1),
            )
            .child(self.stream.clone())
    }
}

#[gpui_kit::test]
async fn prediction_keeps_text_input_shortcut_context_over_window_navigation(
    cx: &mut TestAppContext,
) {
    init_permission_test(cx);
    cx.update(|cx| {
        cx.bind_keys([
            gpui_kit::KeyBinding::new("cmd-[", WindowBack, Some("VegaWindow")),
            gpui_kit::KeyBinding::new("cmd-[", InputBack, Some("TextInput")),
        ])
    });
    let stream = cx.new(|cx| ConversationStream::new(permission_thread(), cx));
    let input_calls = Arc::new(Mutex::new(0));
    let window_calls = Arc::new(Mutex::new(0));
    let root_stream = stream.clone();
    let observed_input = input_calls.clone();
    let observed_window = window_calls.clone();
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), move |_, cx| {
            cx.new(|_| ShortcutHarness {
                stream: root_stream,
                input_calls: observed_input,
                window_calls: observed_window,
            })
        })
        .unwrap()
    });
    window
        .update(cx, |_, window, cx| {
            stream.update(cx, |stream, cx| stream.focus_composer(window, cx))
        })
        .unwrap();
    install_prediction(&stream, "shortcut", cx);
    cx.run_until_parked();
    cx.simulate_keystrokes(window.into(), "cmd-[");
    assert_eq!(*input_calls.lock().unwrap(), 1);
    assert_eq!(*window_calls.lock().unwrap(), 0);
    assert!(stream.read_with(cx, |stream, cx| stream.input.read(cx).text().is_empty()));
}

#[gpui_kit::test]
async fn permission_and_question_latches_revoke_predictions_before_their_cards_arrive(
    cx: &mut TestAppContext,
) {
    use vega_runtime::{RuntimeUserQuestionHook, RuntimeUserQuestionRequest, UserQuestionInput};
    let (_window, stream, _) = open_controller_stream(cx, "prediction-permission-latch");
    let (_, permission_cancel) = install_prediction(&stream, "permission", cx);
    let queue = stream.read_with(cx, |stream, _| stream.permission_queue());
    let _permission = request_permission(&queue, "prediction-permission", "fixture target");
    cx.run_until_parked();
    assert!(permission_cancel.is_cancelled());
    assert!(
        stream.read_with(cx, |stream, cx| !stream.composer_prediction_eligible(cx)
            && stream.input.read(cx).prediction_text().is_none())
    );
    let (_window, stream, _) = open_controller_stream(cx, "prediction-question-latch");
    let (_, question_cancel) = install_prediction(&stream, "question", cx);
    let queue = stream.read_with(cx, |stream, _| stream.user_question_queue());
    let _question = queue.request(RuntimeUserQuestionRequest {
        call_id:"prediction-question".into(),
        input: UserQuestionInput::from_json(r#"{"questions":[{"id":"choice","header":"Choice","question":"Choose?","options":[{"label":"A","description":"A"},{"label":"B","description":"B"}]}]}"#).unwrap(),
    }, CancellationToken::new());
    cx.run_until_parked();
    assert!(question_cancel.is_cancelled());
    assert!(
        stream.read_with(cx, |stream, cx| !stream.composer_prediction_eligible(cx)
            && stream.input.read(cx).prediction_text().is_none())
    );
}

#[gpui_kit::test]
async fn prediction_attachment_addition_cancels_owner_and_removal_cannot_revive_it(
    cx: &mut TestAppContext,
) {
    let (_window, stream, _) = open_controller_stream(cx, "prediction-attachment");
    let (fence, cancel) = install_prediction(&stream, "attachment", cx);
    let image = ImageAttachment::from_bytes(vec![
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6,
        0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 96, 96, 96, 248, 15,
        0, 1, 4, 1, 0, 95, 229, 195, 75, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ])
    .unwrap();
    stream.update(cx, |stream, cx| {
        stream.attachments.push((
            1,
            attachments::ImagePreview::new(image, "fixture.png".into(), cx),
        ));
        cx.notify();
    });
    cx.run_until_parked();
    assert!(cancel.is_cancelled());
    stream.update(cx, |stream, cx| {
        assert!(stream.input.read(cx).prediction_text().is_none());
        assert!(!stream.composer_prediction_eligible(cx));
        stream.attachments.clear();
        assert!(!stream.apply_composer_prediction(&fence, "late".into(), cx));
        assert!(stream.input.read(cx).text().is_empty());
    });
}

#[gpui_kit::test]
async fn context_compaction_revokes_prediction_and_terminal_status_cannot_restore_it(
    cx: &mut TestAppContext,
) {
    let (_window, stream, _) = open_controller_stream(cx, "prediction-compaction");
    let (fence, cancel) = install_prediction(&stream, "before-compaction", cx);
    let mut record = vega_conversation::types::ContextCompactionStatusRecord {
        generation: 1,
        status: vega_conversation::types::ContextCompactionStatus::Compacting,
        updated_at: 1,
        estimated_tokens: None,
        input_budget: None,
        target_tokens: None,
        source_version: None,
        failure: None,
        usage: vega_conversation::types::ContextCompactionUsageState::Pending,
    };
    stream.update(cx, |stream, cx| {
        assert!(stream.apply_context_status("prediction-compaction", "mock", record.clone(), cx));
    });
    cx.run_until_parked();
    assert!(cancel.is_cancelled());
    record.status = vega_conversation::types::ContextCompactionStatus::Succeeded;
    record.updated_at = 2;
    stream.update(cx, |stream, cx| {
        assert!(stream.apply_context_status("prediction-compaction", "mock", record, cx));
        assert!(!stream.apply_composer_prediction(&fence, "stale".into(), cx));
        assert!(stream.input.read(cx).prediction_text().is_none());
        assert!(stream.input.read(cx).text().is_empty());
    });
}
