use super::*;
use gpui_kit::{Focusable, TestAppContext, WindowHandle};
use vega_conversation::types::{ContextCompactionUsageState, ThreadMode, ThreadStatus};

struct EscapeHost(Entity<ConversationStream>);
impl Render for EscapeHost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        // The real application has an Escape action handler above Composer.
        // Without it, unhandled actions fall back to raw key listeners and
        // incorrectly make the isolated stream test appear sufficient.
        div()
            .size_full()
            .on_action(|_: &crate::DismissEnvironmentOverlay, _, cx| cx.stop_propagation())
            .child(self.0.clone())
    }
}

async fn escape_with_host(cx: &mut TestAppContext, input: bool) {
    let initial = setup(cx);
    let thread = initial
        .update(cx, |stream, _, _| stream.thread.clone())
        .expect("thread");
    let stream = cx.new(|cx| ConversationStream::new(thread, cx));
    let child = stream.clone();
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| cx.new(|_| EscapeHost(child)))
            .expect("host")
    });
    cx.run_until_parked();
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    let trigger = visual.debug_bounds("composer-context").expect("trigger");
    visual.simulate_click(trigger.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    if input {
        let bounds = visual.debug_bounds("context-limit-input").expect("input");
        visual.simulate_click(bounds.center(), gpui_kit::Modifiers::default());
        visual.run_until_parked();
    }
    assert!(stream.read_with(cx, |stream, _| stream.context_control.open));
    window
        .update(cx, |_, window, cx| {
            let stream = stream.read(cx);
            let focused = if input {
                stream
                    .context_control
                    .limit
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(window)
            } else {
                stream.context_control.trigger.is_focused(window)
            };
            assert!(
                focused,
                "real click must establish the requested focus owner"
            );
        })
        .expect("clicked focus owner");
    cx.simulate_keystrokes(window.into(), "escape");
    cx.run_until_parked();
    assert!(
        !stream.read_with(cx, |stream, _| stream.context_control.open),
        "one Escape must dismiss with an ancestor action handler"
    );
    window
        .update(cx, |_, window, cx| {
            assert!(stream.read(cx).context_control.trigger.is_focused(window))
        })
        .expect("focus restored");
}

#[gpui_kit::test]
async fn context_ui_escape_trigger_with_application_action(cx: &mut TestAppContext) {
    escape_with_host(cx, false).await;
}

#[gpui_kit::test]
async fn context_ui_escape_input_with_application_action(cx: &mut TestAppContext) {
    escape_with_host(cx, true).await;
}

fn setup(cx: &mut TestAppContext) -> WindowHandle<ConversationStream> {
    setup_sized(cx, 1200.0, 900.0)
}
fn setup_sized(
    cx: &mut TestAppContext,
    width: f32,
    height: f32,
) -> WindowHandle<ConversationStream> {
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(SettingsOpen(false));
        crate::init(cx);
        cx.open_window(
            gpui_kit::WindowOptions {
                window_bounds: Some(gpui_kit::WindowBounds::Windowed(
                    gpui_kit::Bounds::centered(None, gpui_kit::size(px(width), px(height)), cx),
                )),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|cx| {
                    ConversationStream::new(
                        Thread {
                            id: "context-thread".into(),
                            project_id: "project".into(),
                            title: "Context".into(),
                            model: "mock".into(),
                            mode: ThreadMode::Execute,
                            permission_mode: PermissionMode::Confirm,
                            status: ThreadStatus::Active,
                            pinned: false,
                            unread: false,
                            created_at: 1,
                            updated_at: 1,
                        },
                        cx,
                    )
                })
            },
        )
        .expect("window")
    })
}
fn settings() -> ContextSettings {
    ContextSettings {
        thread_id: "context-thread".into(),
        model: "mock".into(),
        context_limit: Some(8000),
        output_reserve: 1000,
        automatic_compaction: true,
        updated_at: 1,
    }
}
fn status(generation: u64, status: Status) -> ContextCompactionStatusRecord {
    ContextCompactionStatusRecord {
        generation,
        status,
        updated_at: 1,
        estimated_tokens: Some(5000),
        input_budget: Some(7000),
        target_tokens: Some(4900),
        source_version: Some(1),
        failure: None,
        usage: ContextCompactionUsageState::Unknown,
    }
}

#[gpui_kit::test]
async fn context_ui_unknown_usage_restore_is_idempotent_and_load_error_is_not_usage(
    cx: &mut TestAppContext,
) {
    let window = setup(cx);
    window
        .update(cx, |stream, _, cx| {
            stream.restore_meter(
                RestoredUsage {
                    tokens: 50,
                    cost: Some(vega_conversation::types::Microcents(10)),
                },
                cx,
            );
            let before = stream.meter_snapshot();
            assert!(stream.apply_context_load_error("context-thread", "mock", cx));
            assert_eq!(stream.meter_snapshot(), before);
            assert!(!stream.restore_context_unknown_usage("other", "mock", true, cx));
            assert_eq!(stream.meter_snapshot(), before);
            assert!(stream.restore_context_unknown_usage("context-thread", "mock", true, cx));
            let unknown = stream.meter_snapshot();
            assert_eq!(unknown.tokens, 50);
            assert!(unknown.display().ends_with("—"));
            stream.restore_context_unknown_usage("context-thread", "mock", true, cx);
            stream.restore_context_unknown_usage("context-thread", "mock", false, cx);
            assert_eq!(stream.meter_snapshot(), unknown);
        })
        .expect("meter");
}

#[gpui_kit::test]
async fn context_ui_narrow_light_dark_popup_keeps_real_cancel_inside_clip(cx: &mut TestAppContext) {
    for (width, height, dark) in [(960.0, 600.0, false), (320.0, 400.0, true)] {
        let window = setup_sized(cx, width, height);
        cx.update(|cx| {
            cx.set_global(if dark {
                vega_theme::Theme::dark()
            } else {
                vega_theme::Theme::light()
            });
        });
        window
            .update(cx, |stream, window, cx| {
                stream.reset_context_control(cx);
                stream.apply_context_projection(
                    "context-thread",
                    "mock",
                    Some(settings()),
                    Some(7000),
                    true,
                    cx,
                );
                stream.compact_context(cx);
                stream.toggle_context(window, cx);
            })
            .expect("open busy");
        cx.run_until_parked();
        let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
        let popup = visual.debug_bounds("context-popup").expect("popup");
        let cancel = visual.debug_bounds("context-cancel").expect("cancel");
        assert!(popup.left() >= px(0.0) && popup.top() >= px(0.0));
        assert!(
            popup.right() <= px(width) && popup.bottom() <= px(height),
            "popup {popup:?} requested {width}x{height}"
        );
        assert!(
            cancel.top() >= popup.top() && cancel.bottom() <= popup.bottom(),
            "cancel must not be clipped"
        );
        visual.simulate_click(cancel.center(), gpui_kit::Modifiers::default());
        visual.run_until_parked();
        window
            .update(cx, |stream, _, _| {
                assert!(stream.context_control.cancel_pending, "real cancel click")
            })
            .expect("cancelled");
    }
}

#[gpui_kit::test]
async fn context_ui_validates_real_inputs_and_exact_save_ack(cx: &mut TestAppContext) {
    let window = setup(cx);
    window
        .update(cx, |stream, _, cx| {
            for value in ["", "-1", "0", "1.5", "4294967296", "18446744073709551616"] {
                stream
                    .context_control
                    .limit
                    .update(cx, |input, cx| input.set_text(value, cx));
                stream
                    .context_control
                    .reserve
                    .update(cx, |input, cx| input.set_text("1", cx));
                stream.save_context(cx);
                assert!(stream.context_control.save_pending.is_none(), "{value}");
                assert!(stream.context_control.error.is_some());
            }
            stream
                .context_control
                .limit
                .update(cx, |input, cx| input.set_text("1000", cx));
            stream
                .context_control
                .reserve
                .update(cx, |input, cx| input.set_text("1000", cx));
            stream.save_context(cx);
            assert!(stream.context_control.save_pending.is_none());
            stream
                .context_control
                .reserve
                .update(cx, |input, cx| input.set_text("100", cx));
            stream.save_context(cx);
            let id = stream
                .context_control
                .save_pending
                .expect("real save queued");
            assert!(stream.context_operation_busy());
            assert!(!stream.finish_context_settings("wrong", "mock", id, Some(settings()), cx));
            assert!(!stream.finish_context_settings(
                "context-thread",
                "mock",
                id + 1,
                Some(settings()),
                cx
            ));
            assert!(stream.finish_context_settings("context-thread", "mock", id, None, cx));
            assert_eq!(stream.context_control.limit.read(cx).text(), "1000");
            stream.save_context(cx);
            let retry = stream.context_control.save_pending.expect("retry");
            assert!(retry > id);
            assert!(stream.finish_context_settings(
                "context-thread",
                "mock",
                retry,
                Some(settings()),
                cx
            ));
            assert!(!stream.context_operation_busy());
        })
        .expect("update");
}

#[gpui_kit::test]
async fn context_ui_terminal_is_fixed_retry_and_aba_are_fenced(cx: &mut TestAppContext) {
    let window = setup(cx);
    window
        .update(cx, |stream, _, cx| {
            assert!(stream.apply_context_projection(
                "context-thread",
                "mock",
                Some(settings()),
                Some(7000),
                true,
                cx
            ));
            stream
                .input
                .update(cx, |input, cx| input.set_text("preserved draft", cx));
            stream.compact_context(cx);
            let id = stream
                .context_control
                .compact_pending
                .expect("manual operation");
            assert!(stream.apply_context_status(
                "context-thread",
                "mock",
                status(id, Status::Compacting),
                cx
            ));
            stream.cancel_context(cx);
            assert!(stream.context_control.cancel_pending);
            assert!(stream.apply_context_status(
                "context-thread",
                "mock",
                status(id, Status::Cancelled),
                cx
            ));
            for next in [Status::Succeeded, Status::Failed, Status::Compacting] {
                let mut late = status(id, next);
                late.updated_at = 99;
                assert!(!stream.apply_context_status("context-thread", "mock", late, cx));
            }
            assert_eq!(stream.input.read(cx).text(), "preserved draft");
            stream.compact_context(cx);
            let retry = stream.context_control.compact_pending.expect("retry");
            assert!(retry > id);
            assert!(!stream.apply_context_status(
                "context-thread",
                "mock",
                status(id, Status::Succeeded),
                cx
            ));
            assert!(stream.apply_context_status(
                "context-thread",
                "mock",
                status(retry, Status::Failed),
                cx
            ));
            let auto = stream
                .reserve_context_operation_id(cx)
                .expect("next run auto");
            assert!(auto > retry);
            assert!(stream.apply_context_status(
                "context-thread",
                "mock",
                status(auto, Status::Compacting),
                cx
            ));
            assert!(!stream.apply_context_status(
                "context-thread",
                "other",
                status(auto, Status::Succeeded),
                cx
            ));
            stream.reset_context_control(cx);
            assert!(!stream.apply_context_status(
                "context-thread",
                "mock",
                status(auto, Status::Succeeded),
                cx
            ));
            let after = stream
                .reserve_context_operation_id(cx)
                .expect("ABA generation");
            assert!(after > auto);
        })
        .expect("update");
}

#[gpui_kit::test]
async fn context_ui_real_trigger_popup_and_escape(cx: &mut TestAppContext) {
    let window = setup(cx);
    let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let captured = events.clone();
    window
        .update(cx, |stream, _, cx| {
            stream.apply_context_projection(
                "context-thread",
                "mock",
                Some(settings()),
                Some(5000),
                true,
                cx,
            );
            cx.subscribe(
                &cx.entity(),
                move |_, _, event: &ContextSettingsRequested, _| {
                    captured.lock().expect("events").push(event.clone());
                },
            )
            .detach();
        })
        .expect("subscribe");
    cx.run_until_parked();
    let bounds = gpui_kit::VisualTestContext::from_window(window.into(), cx)
        .debug_bounds("composer-context")
        .expect("real context trigger");
    gpui_kit::VisualTestContext::from_window(window.into(), cx)
        .simulate_click(bounds.center(), gpui_kit::Modifiers::default());
    cx.run_until_parked();
    assert!(
        gpui_kit::VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("context-popup")
            .is_some()
    );
    let save = gpui_kit::VisualTestContext::from_window(window.into(), cx)
        .debug_bounds("context-save")
        .expect("save button");
    gpui_kit::VisualTestContext::from_window(window.into(), cx)
        .simulate_click(save.center(), gpui_kit::Modifiers::default());
    cx.run_until_parked();
    let saved = events.lock().expect("events").clone();
    assert_eq!(saved.len(), 1, "real click must emit save intent");
    assert_eq!(saved[0].settings.context_limit, Some(8000));
    window
        .update(cx, |stream, window, cx| {
            stream
                .context_control
                .limit
                .read(cx)
                .focus_handle(cx)
                .focus(window, cx);
        })
        .expect("focus input");
    cx.simulate_keystrokes(window.into(), "tab");
    cx.run_until_parked();
    window
        .update(cx, |stream, window, cx| {
            assert!(
                stream
                    .context_control
                    .reserve
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(window),
                "Tab reaches next settings input"
            );
        })
        .expect("settings tab order");
    cx.simulate_keystrokes(window.into(), "escape");
    cx.run_until_parked();
    window
        .update(cx, |stream, window, _| {
            assert!(!stream.context_control.open);
            assert!(stream.context_control.trigger.is_focused(window));
        })
        .expect("closed focus");
}

#[gpui_kit::test]
async fn context_ui_manual_gates_draft_primary_run_and_missing_history(cx: &mut TestAppContext) {
    let window = setup(cx);
    window
        .update(cx, |stream, _, cx| {
            stream.compact_context(cx);
            assert!(stream.context_control.compact_pending.is_none());
            stream.apply_context_projection(
                "context-thread",
                "mock",
                Some(settings()),
                None,
                false,
                cx,
            );
            stream.compact_context(cx);
            assert!(stream.context_control.compact_pending.is_none());
            stream.apply_context_projection(
                "context-thread",
                "mock",
                Some(settings()),
                Some(7000),
                true,
                cx,
            );
            stream.set_draft_route(true, cx);
            stream.compact_context(cx);
            assert!(stream.context_control.compact_pending.is_none());
            stream.set_draft_route(false, cx);
            stream.actions.running = true;
            stream.compact_context(cx);
            assert!(stream.context_control.compact_pending.is_none());
            stream.actions.running = false;
            stream.compact_context(cx);
            assert!(stream.context_control.compact_pending.is_some());
            assert!(stream.model_selection_blocked(cx));
            let id = stream.context_control.compact_pending;
            stream.compact_context(cx);
            assert_eq!(stream.context_control.compact_pending, id);
        })
        .expect("update");
}

#[gpui_kit::test]
async fn context_ui_popup_closes_siblings_and_is_closed_by_shared_dismiss(cx: &mut TestAppContext) {
    let window = setup(cx);
    window
        .update(cx, |stream, window, cx| {
            stream.utility_projects_open = true;
            stream.toggle_context(window, cx);
            assert!(stream.context_control.open);
            assert!(!stream.utility_projects_open);
            assert!(stream.context_control.trigger.is_focused(window));
            stream
                .branch_selector
                .update(cx, |branch, cx| assert!(branch.request_open(cx)));
        })
        .expect("branch opens");
    cx.run_until_parked();
    window
        .update(cx, |stream, window, cx| {
            assert!(
                !stream.context_control.open,
                "branch open event closes context"
            );
            stream.toggle_context(window, cx);
            assert!(
                !stream.branch_selector.read(cx).is_open(),
                "context closes branch"
            );
            stream.close_composer_popovers(cx);
            assert!(!stream.context_control.open);
            stream.toggle_context(window, cx);
            stream.toggle_context(window, cx);
            assert!(!stream.context_control.open);
        })
        .expect("update");
    cx.run_until_parked();
}
