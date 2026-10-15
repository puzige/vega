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

#[gpui_kit::test]
fn issue395_window_mapping_keeps_repeated_markdown_anchor_and_selection(cx: &mut TestAppContext) {
    let paragraph = "**Repeated** paragraph with `code` and &amp; unicode 界.\n\n";
    let source = paragraph.repeat(4000);
    let (card, mut visual) = open(cx, &source);
    let scroll = card.read_with(&visual, |card, _| card.scroll.clone());
    scroll.set_offset(point(px(0.), -scroll.max_offset().y / 2.));
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
    let (anchor, offset, selected) = card.read_with(&visual, |card, _| {
        let prepared = card.body.prepared.as_ref().expect("prepared");
        let (byte, offset) = prepared.anchor(-scroll.offset().y).expect("middle anchor");
        let anchor = prepared.source_position(byte).expect("source position");
        let end = byte + "Repeated".len();
        let selected = prepared.text[byte..end].to_owned();
        card.source_copy.set_projected_selection(Some(byte..end));
        card.source_copy.set_local_selection_range(byte..end);
        card.source_copy
            .set_selected_text(card.source_copy.selection_generation(), selected.clone());
        (anchor, offset, selected)
    });
    card.update(&mut visual, |card, cx| {
        card.append(&paragraph.repeat(1400), false, THINKING_BLOCK_BYTES);
        card.prepare_body(cx);
        cx.notify();
    });
    visual.run_until_parked();
    card.read_with(&visual, |card, _| {
        let prepared = card.body.prepared.as_ref().expect("new prepared");
        let byte = prepared
            .rendered_position(&anchor)
            .expect("retained source identity");
        let y = prepared.anchor_y(byte).expect("retained anchor geometry");
        assert!((f32::from(-scroll.offset().y - y - offset)).abs() <= 1.);
        let range = card
            .source_copy
            .local_selection_range()
            .expect("retained selection");
        assert_eq!(&prepared.text[range], selected);
        assert!(card.source_copy.has_selected_text());
    });
    scroll.set_offset(point(px(0.), px(0.)));
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
    card.update(&mut visual, |card, cx| {
        card.append(&paragraph.repeat(4000), false, THINKING_BLOCK_BYTES);
        card.prepare_body(cx);
        cx.notify();
    });
    visual.run_until_parked();
    assert_eq!(scroll.offset().y, px(0.));
    assert!(!card.read_with(&visual, |card, _| card.source_copy.has_selected_text()));
    scroll.set_offset(point(px(0.), -scroll.max_offset().y));
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
    card.update(&mut visual, |card, cx| {
        card.append(&paragraph.repeat(1400), false, THINKING_BLOCK_BYTES);
        card.prepare_body(cx);
        cx.notify();
    });
    visual.run_until_parked();
    assert!(f32::from(scroll.offset().y + scroll.max_offset().y).abs() <= 1.);
    assert!(card.read_with(&visual, |card, _| card.test_body_rendered_count()) < 100);
}

#[gpui_kit::test]
fn issue395_worker_publishes_retained_overlap_during_streaming_and_rejects_evicted_work(
    cx: &mut TestAppContext,
) {
    let (card, mut visual) = open(cx, &"old paragraph\n\n".repeat(12000));
    for index in 0..3 {
        card.update(&mut visual, |card, cx| {
            let prepared = prepare(card, cx);
            let revision = card.source_revision;
            let epoch = card.body.epoch;
            let identity = card.source_identity;
            let end = prepared.source.end;
            card.append(
                &format!("{}latest-{index}\n\n", "new paragraph\n\n".repeat(5000)),
                false,
                THINKING_BLOCK_BYTES,
            );
            assert!(card.visible_window().start < end);
            assert!(card.complete_preparation(prepared, revision, epoch, identity, cx));
            assert_eq!(card.body.prepared_revision, revision);
            assert!(card.body.prepared_revision < card.source_revision);
            let shown = card.body.prepared.as_ref().expect("overlap published");
            assert_eq!(shown.source.start, card.visible_window().start);
            assert_eq!(shown.source.end, end);
        });
    }
    visual.run_until_parked();
    let scroll = card.read_with(&visual, |card, _| card.scroll.clone());
    assert!(scroll.max_offset().y > px(0.));
    scroll.set_offset(point(px(0.), -scroll.max_offset().y));
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
    card.update(&mut visual, |card, cx| {
        let prepared = prepare(card, cx);
        let revision = card.source_revision;
        let epoch = card.body.epoch;
        let identity = card.source_identity;
        card.append(
            &"replacement\n\n".repeat(40000),
            false,
            THINKING_BLOCK_BYTES,
        );
        assert!(!card.complete_preparation(prepared, revision, epoch, identity, cx));
        assert!(card.body.prepared.is_none());
        let prepared = prepare(card, cx);
        assert!(card.complete_preparation(prepared, card.source_revision, epoch, identity, cx));
        assert!(
            card.body
                .prepared
                .as_ref()
                .expect("caught up")
                .text
                .starts_with("replacement")
        );
    });
    visual.run_until_parked();
    assert!(scroll.max_offset().y > px(0.));
    assert!(f32::from(scroll.offset().y + scroll.max_offset().y).abs() <= 1.);
    card.update(&mut visual, |card, cx| {
        let revision = card.source_revision;
        let epoch = card.body.epoch;
        let identity = card.source_identity;
        let prepared = prepare(card, cx);
        card.source_copy.set_projected_selection(Some(0..11));
        card.source_copy.set_local_selection_range(0..11);
        card.source_copy.set_selected_text(
            card.source_copy.selection_generation(),
            "replacement".into(),
        );
        assert!(card.source_copy.has_selected_text());
        assert!(card.source_copy.visible_text().is_some());
        card.evict(cx);
        assert!(!card.complete_preparation(prepared, revision, epoch, identity, cx));
        assert!(card.body.prepared.is_none());
        assert_eq!(card.source_copy.visible_text(), None);
        assert!(!card.source_copy.has_selected_text());
        assert!(card.source_copy.original_text().is_empty());
        assert!(!card.has_meaningful_source_text());
        card.set_expanded(true, cx);
        assert!(!card.expanded);
    });
}

#[gpui_kit::test]
fn issue395_window_preserves_fence_context_and_original_copy(cx: &mut TestAppContext) {
    let source = format!("```rust\n{}\nlet latest = 1;", "identifier_".repeat(30000));
    let (card, mut visual) = open(cx, &source);
    card.read_with(&visual, |card, _| {
        assert!(!card.source_text().starts_with("```"));
        assert_eq!(card.source_text(), card.source_copy.original_text());
        let prepared = card.body.prepared.as_ref().expect("code body");
        assert!(prepared.blocks.iter().all(|block| block.code));
        assert!(prepared.text.ends_with("let latest = 1;"));
        assert!(card.test_body_rendered_count() < 100);
    });
    card.update(&mut visual, |card, cx| {
        card.append("\n```\n\nlatest prose", false, THINKING_BLOCK_BYTES);
        card.prepare_body(cx);
        cx.notify();
    });
    visual.run_until_parked();
    card.read_with(&visual, |card, _| {
        assert!(
            !card
                .body
                .prepared
                .as_ref()
                .expect("closed body")
                .blocks
                .last()
                .expect("prose")
                .code
        );
    });
    let source = format!("```\na\n```\n{}", "x".repeat(THINKING_BLOCK_BYTES - 3));
    card.update(&mut visual, |card, cx| {
        card.test_replace_source("", cx);
        card.append(&source, false, THINKING_BLOCK_BYTES);
        card.prepare_body(cx);
        cx.notify();
    });
    visual.run_until_parked();
    card.read_with(&visual, |card, _| {
        assert_eq!(card.visible_window().prefix(), "");
        assert!(card.source_text().starts_with('x'));
        assert!(
            card.body
                .prepared
                .as_ref()
                .expect("prose body")
                .blocks
                .iter()
                .all(|block| !block.code)
        );
        assert_eq!(card.source_copy.original_text(), card.source_text());
    });
}

#[gpui_kit::test]
fn issue395_retained_code_keeps_horizontal_scroll_when_window_moves(cx: &mut TestAppContext) {
    let source = format!(
        "{}\n\n```rust\n{}\n```",
        "paragraph\n\n".repeat(17000),
        "identifier_".repeat(3000)
    );
    let (card, mut visual) = open(cx, &source);
    let scroll = card.read_with(&visual, |card, _| card.scroll.clone());
    scroll.set_offset(point(px(0.), -scroll.max_offset().y));
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
    let horizontal = card
        .read_with(&visual, |card, _| card.test_horizontal_scroll())
        .expect("code scroll");
    horizontal.set_offset(point(px(-120.), px(0.)));
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
    card.update(&mut visual, |card, cx| {
        card.append(
            &format!("\n\n{}", "later prose ".repeat(6000)),
            false,
            THINKING_BLOCK_BYTES,
        );
        card.prepare_body(cx);
        cx.notify();
    });
    visual.run_until_parked();
    let code_y = card.read_with(&visual, |card, _| {
        card.body
            .prepared
            .as_ref()
            .expect("prepared")
            .blocks
            .iter()
            .find(|block| block.code)
            .expect("retained code")
            .origin
            .y
    });
    scroll.set_offset(point(px(0.), -code_y));
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
    let current = card
        .read_with(&visual, |card, _| card.test_horizontal_scroll())
        .expect("retained horizontal handle");
    assert_eq!(current.offset().x, px(-120.));
    assert_eq!(horizontal.offset().x, px(-120.));
}
