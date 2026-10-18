use super::*;
use gpui_kit::{TestAppContext, VisualTestContext};

struct Harness(Entity<ThinkingBlock>);

impl Render for Harness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context("VegaWindow")
            .w_full()
            .child(self.0.clone())
    }
}

fn open(cx: &mut TestAppContext, source: &str) -> (Entity<ThinkingBlock>, VisualTestContext) {
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        crate::init(cx);
    });
    let card = cx.new(ThinkingBlock::new);
    card.update(cx, |card, cx| {
        card.append(source, false, THINKING_BLOCK_BYTES);
        card.set_expanded(true, cx);
    });
    let child = card.clone();
    let window = cx
        .update(|cx| cx.open_window(Default::default(), move |_, cx| cx.new(|_| Harness(child))))
        .expect("thinking window");
    cx.run_until_parked();
    (card, VisualTestContext::from_window(window.into(), cx))
}

fn prepare(card: &ThinkingBlock, cx: &App) -> PreparedBody {
    let prefix = card.visible_window().prefix();
    PreparedBody::new(
        &format!("{prefix}{}", card.visible_text()),
        prefix.len(),
        card.visible_window().start,
        card.body.style.as_ref().expect("visible style"),
        &WindowTextSystem::new(cx.text_system().clone()),
    )
}
