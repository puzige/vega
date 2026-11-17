use gpui_kit::*;

use nsh_platform_gpui::ScrollRemainder;

pub(crate) fn consume_list(
    list: &ListState,
    remaining: ScrollRemainder,
    window: &mut Window,
    _cx: &mut App,
) -> bool {
    let delta = remaining.delta.pixel_delta(px(20.)).y;
    let before = -list.scroll_px_offset_for_scrollbar().y;
    let maximum = list.max_offset_for_scrollbar().y.max(px(0.));
    if !f32::from(delta).is_finite()
        || !f32::from(before).is_finite()
        || !f32::from(maximum).is_finite()
        || list.viewport_bounds().is_empty()
    {
        return false;
    }
    let target = (before - delta).max(px(0.)).min(maximum);
    if target == before {
        return false;
    }
    list.scroll_by(target - before);
    window.refresh();
    true
}
