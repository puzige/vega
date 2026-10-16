//! Actual root subscriptions, owned DB/project and real worker; provider boundary only is mocked.
use super::*;
use gpui_kit::{Bounds, VisualTestContext, WindowBounds, WindowHandle, WindowOptions, px, size};

#[path = "context_compaction.rs"]
mod context_compaction;

#[path = "pending_messages.rs"]
mod pending_messages;

struct Fixture {
    _data: TempDir,
    repo: ControllerRepo,
    store: Store,
    thread: Thread,
    root: Entity<VegaWindow>,
    stream: Entity<ConversationStream>,
    window: WindowHandle<VegaWindow>,
}

fn draft(f: &Fixture, cx: &mut gpui_kit::TestAppContext) -> String {
    f.stream.read_with(cx, |stream, cx| {
        stream.composer_input().read(cx).text().to_owned()
    })
}
