use super::*;
use gpui_kit::{Modifiers, MouseButton, Pixels, Point, VisualTestContext, size};

struct SelectionHarness {
    stream: Entity<ConversationStream>,
}

impl Render for SelectionHarness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context("VegaWindow")
            .size_full()
            .child(gpui_kit::base::TextSelectionLayer)
            .child(self.stream.clone())
    }
}

fn drag(visual: &mut VisualTestContext, start: Point<Pixels>, end: Point<Pixels>) {
    visual.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    visual.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
    visual.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
}

struct InlineReadView {
    model: StreamModel,
    colors: vega_theme::ThemeColors,
}

impl Render for InlineReadView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(markdown_item(&self.model, None, &self.colors))
    }
}

struct NarrowInlineView;

impl Render for NarrowInlineView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let style = TextStyle {
            font_family: MONOFONT.into(),
            background_color: Some(vega_theme::Theme::light().colors.conversation_code.into()),
            ..TextStyle::default()
        };
        div()
            .size_full()
            .text_size(px(10.))
            .line_height(px(23.))
            .child(inline_code::styled_text("x".into(), vec![style.to_run(1)]))
    }
}
