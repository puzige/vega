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

struct HeightView(gpui_kit::ListState);

impl Render for HeightView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui_kit::list(self.0.clone(), |index, _, _| {
            div().h(px(variable_item_height(index))).w_full().into_any()
        })
        .w_full()
        .h_full()
    }
}

fn draw_height_view(cx: &mut gpui_kit::VisualTestContext, view: &Entity<HeightView>) {
    cx.draw(
        gpui_kit::point(px(0.), px(0.)),
        gpui_kit::size(px(100.), px(200.)),
        |_, _| view.clone().into_any_element(),
    );
}

#[gpui_kit::test]
fn variable_height_geometry_items_measure_at_natural_heights(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();
    // 9 items: total = 3×(40+96+64) = 600px; viewport 200px. The overdraw is
    // large enough to measure every item in one frame so the geometry
    // assertions cover the whole list.
    let count = 9usize;
    let state = gpui_kit::ListState::new(count, gpui_kit::ListAlignment::Top, px(600.0));

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
