//! S8-T44 变高虚拟化窄测（决策 7 从简：GPUI 窄测恰 2 条）。
//!
//! 1. `variable_height_geometry_items_measure_at_natural_heights`：变高几何
//!    —— 混合语义项在一帧内按各自自然高度测量，item 边界连续、总高与
//!    逐项高一致（截断会表现为高度与内容行数无关，此断言拒斥定高回归）。
//! 2. `prepend_splice_preserves_page_boundary_anchor`：前插锚定 ——
//!    detached 视口上方 splice 前插 N 个变高 item 后，scroll-top item 的
//!    像素偏移保持不变（页边界保 anchor，漂移 0 < 1px）。

use super::*;

/// One item per index; heights vary by index (40px / 96px / 64px) so a
/// uniform-height regression cannot satisfy the geometry assertions.
fn variable_item_height(index: usize) -> f32 {
    match index % 3 {
        0 => 40.0,
        1 => 96.0,
        _ => 64.0,
    }
}

struct HeightView(gpui::ListState);

impl Render for HeightView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::list(self.0.clone(), |index, _, _| {
            div().h(px(variable_item_height(index))).w_full().into_any()
        })
        .w_full()
        .h_full()
    }
}

fn draw_height_view(cx: &mut gpui::VisualTestContext, view: &Entity<HeightView>) {
    cx.draw(
        gpui::point(px(0.), px(0.)),
        gpui::size(px(100.), px(200.)),
        |_, _| view.clone().into_any_element(),
    );
}

#[gpui::test]
fn variable_height_geometry_items_measure_at_natural_heights(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();
    // 9 items: total = 3×(40+96+64) = 600px; viewport 200px. The overdraw is
    // large enough to measure every item in one frame so the geometry
    // assertions cover the whole list.
    let count = 9usize;
    let state = gpui::ListState::new(count, gpui::ListAlignment::Top, px(600.0));

    let view = cx.update(|_, cx| cx.new(|_| HeightView(state.clone())));
    draw_height_view(cx, &view);

    // Item boundaries must be contiguous and natural: bounds_for_item(ix)
    // starts exactly where the previous item ended, at the item's own height.
    let mut expected_top = 0.0;
    for index in 0..count {
        let bounds = state
            .bounds_for_item(index)
            .unwrap_or_else(|| panic!("item {index} must be laid out"));
        assert_eq!(f32::from(bounds.top()), expected_top, "item {index} top");
        assert_eq!(
            f32::from(bounds.bottom()) - f32::from(bounds.top()),
            variable_item_height(index),
            "item {index} must measure at its natural height (C4: 禁止以截断凑高度)"
        );
        expected_top += variable_item_height(index);
    }
    let total = f32::from(
        state
            .bounds_for_item(count - 1)
            .expect("last item laid out")
            .bottom(),
    );
    let expected_total: f32 = (0..count).map(variable_item_height).sum();
    assert_eq!(total, expected_total, "total height is the exact item sum");
}

#[gpui::test]
fn prepend_splice_preserves_page_boundary_anchor(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();
    // 12 items × mixed heights; viewport 200px.
    let state = gpui::ListState::new(12, gpui::ListAlignment::Top, px(600.0));

    let view = cx.update(|_, cx| cx.new(|_| HeightView(state.clone())));
    draw_height_view(cx, &view);

    // The user reads item 5 (not at the page top): scroll so item 5 sits at
    // the viewport top with 8px offset into the item (a sub-pixel-exact
    // reading position).
    state.scroll_to(gpui::ListOffset {
        item_ix: 5,
        offset_in_item: px(8.0),
    });
    draw_height_view(cx, &view);
    let before = state.logical_scroll_top();
    assert_eq!(before.item_ix, 5);
    assert_eq!(before.offset_in_item, px(8.0));

    // Older history arrives: 4 items PREPEND above the loaded history
    // (S8-T45/C7 page boundary). The splice must shift the scroll-top item
    // index by the prepend count while keeping the pixel offset into the
    // item — the content the user was reading stays put (drift 0 < 1px).
    state.splice(0..0, 4);
    draw_height_view(cx, &view);

    let after = state.logical_scroll_top();
    assert_eq!(after.item_ix, 9, "scroll-top item follows the prepend");
    assert_eq!(
        after.offset_in_item,
        px(8.0),
        "pixel offset into the item is preserved exactly (<1px anchor drift)"
    );

    // The item_ix shift + exact offset preservation above ARE the page-boundary
    // anchor contract: the same content item stays at the same pixel offset in
    // the viewport (drift 0 < 1px). (Absolute-pixel arithmetic is not checked
    // here because this synthetic view binds heights to indices, so a splice
    // also changes the index->height mapping.)
}
