use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use super::*;
use gpui_kit::{Focusable, TestAppContext, WindowHandle};
use tokio_util::sync::CancellationToken;
use vega_conversation::agent::PermissionHook;
use vega_conversation::types::{
    Approval, PermissionDecision, PermissionMode, PermissionRequest, ThreadMode, ThreadStatus,
    ToolCall, ToolCallStatus, ToolResult,
};

type DecisionFuture = Pin<Box<dyn Future<Output = PermissionDecision> + Send>>;

mod composer_predictions;

mod issue426_external_grep;

struct StreamHarness {
    stream: Entity<ConversationStream>,
}

fn permission_thread() -> Thread {
    Thread {
        parent_thread_id: None,
        id: "thread-safe-id".into(),
        backend: TaskBackend::Native,
        project_id: "project-safe-id".into(),
        title: "Permission test".into(),
        mode: ThreadMode::Execute,
        permission_mode: PermissionMode::Confirm,
        model: "mock".into(),
        status: ThreadStatus::Active,
        pinned: false,
        unread: false,
        created_at: 1,
        updated_at: 1,
    }
}

fn init_permission_test(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(SettingsOpen(false));
        crate::init(cx);
    });
}

fn open_permission_stream(
    cx: &mut TestAppContext,
) -> (WindowHandle<ConversationStream>, PermissionQueue) {
    let queue = PermissionQueue::new();
    let stream_queue = queue.clone();
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), move |_, cx| {
            cx.new(|cx| {
                ConversationStream::new_with_permission_queue(permission_thread(), stream_queue, cx)
            })
        })
        .expect("test window")
    });
    cx.run_until_parked();
    (window, queue)
}

fn open_controller_stream(
    cx: &mut TestAppContext,
    thread_id: &str,
) -> (
    WindowHandle<StreamHarness>,
    Entity<ConversationStream>,
    Arc<Mutex<Vec<ThreadSettingsRequested>>>,
) {
    init_permission_test(cx);
    let mut thread = permission_thread();
    thread.id = thread_id.to_string();
    let stream = cx.new(|cx| ConversationStream::new(thread, cx));
    let root_stream = stream.clone();
    let events = Arc::new(Mutex::new(Vec::new()));
    let captured = events.clone();
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), move |_, cx| {
            cx.new(|cx| {
                cx.subscribe(
                    &root_stream,
                    move |_, _, event: &ThreadSettingsRequested, _| {
                        if let Ok(mut events) = captured.lock() {
                            events.push(event.clone());
                        }
                    },
                )
                .detach();
                StreamHarness {
                    stream: root_stream,
                }
            })
        })
        .expect("controller stream window")
    });
    cx.run_until_parked();
    (window, stream, events)
}

fn focus_composer(
    window: WindowHandle<StreamHarness>,
    stream: &Entity<ConversationStream>,
    cx: &mut TestAppContext,
) {
    window
        .update(cx, |_, window, cx| {
            let focus = stream.read_with(cx, |stream, cx| stream.input.read(cx).focus_handle(cx));
            window.focus(&focus, cx);
        })
        .expect("composer stream window");
}

fn propose(window: WindowHandle<ConversationStream>, cx: &mut TestAppContext, call: ToolCall) {
    window
        .update(cx, |stream, _, cx| {
            stream.apply_event(ConversationEvent::ToolCallProposed { call }, cx);
        })
        .expect("stream window");
}

fn request_permission(queue: &PermissionQueue, call_id: &str, target: &str) -> DecisionFuture {
    let future = queue.request(
        PermissionRequest {
            call_id: call_id.into(),
            tool: "bash".into(),
            display_target: target.into(),
            danger_rule_id: None,
            danger_reason: None,
            external: None,
            acp_options: None,
        },
        CancellationToken::new(),
    );
    Box::pin(async move { future.await.unwrap_or(PermissionDecision::Timeout) })
}

fn has_active_permission(
    window: WindowHandle<ConversationStream>,
    cx: &mut TestAppContext,
) -> bool {
    window
        .update(cx, |stream, _, _| stream.active_permission.is_some())
        .unwrap_or(false)
}

impl Render for StreamHarness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context("VegaWindow")
            .size_full()
            .child(self.stream.clone())
    }
}
