use super::*;
use gpui_kit::{TestAppContext, WindowHandle};
use vega_conversation::types::{TaskBackend, Thread, ThreadStatus, ToolCall};

mod completion_response;

struct StreamHarness {
    stream: Entity<ConversationStream>,
}

fn open_controller_stream(
    cx: &mut TestAppContext,
    thread_id: &str,
) -> (WindowHandle<StreamHarness>, Entity<ConversationStream>, ()) {
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(SettingsOpen(false));
        crate::init(cx);
    });
    let thread = Thread {
        parent_thread_id: None,
        id: thread_id.into(),
        backend: TaskBackend::Native,
        project_id: "project-safe-id".into(),
        title: "Synthetic response".into(),
        mode: ThreadMode::Execute,
        permission_mode: PermissionMode::Confirm,
        model: "mock".into(),
        status: ThreadStatus::Active,
        pinned: false,
        unread: false,
        created_at: 1,
        updated_at: 1,
    };
    let stream = cx.new(|cx| ConversationStream::new(thread, cx));
    let root_stream = stream.clone();
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), move |_, cx| {
            cx.new(|_| StreamHarness {
                stream: root_stream,
            })
        })
        .unwrap()
    });
    cx.run_until_parked();
    (window, stream, ())
}

impl Render for StreamHarness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context("VegaWindow")
            .size_full()
            .child(self.stream.clone())
    }
}
