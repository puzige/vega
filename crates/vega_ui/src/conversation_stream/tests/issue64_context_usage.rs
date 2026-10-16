use super::*;
use gpui_kit::{Modifiers, VisualTestContext, point, px};
use vega_conversation::types::ContextSettings;

fn settings(thread_id: &str, model: &str, context_limit: Option<u64>) -> ContextSettings {
    ContextSettings {
        thread_id: thread_id.to_string(),
        model: model.to_string(),
        context_limit,
        output_reserve: 8_000,
        automatic_compaction: false,
        updated_at: 1,
    }
}

fn bounds(
    window: WindowHandle<StreamHarness>,
    selector: &'static str,
    cx: &mut TestAppContext,
) -> Option<gpui_kit::Bounds<gpui_kit::Pixels>> {
    cx.run_until_parked();
    VisualTestContext::from_window(window.into(), cx).debug_bounds(selector)
}
