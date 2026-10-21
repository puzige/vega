use super::display_layout;
use gpui_kit::{FontId, GlyphId, LineLayout, ShapedGlyph, ShapedRun, point, px};
use std::sync::Arc;

#[test]
fn rtl_logical_prefix_fits_with_bounded_shaping() {
    let line = |text: &str| {
        let glyphs = text
            .char_indices()
            .rev()
            .enumerate()
            .map(|(x, (index, ch))| ShapedGlyph {
                id: GlyphId(ch as u32),
                position: point(px(x as f32), px(0.)),
                index,
                is_emoji: false,
            })
            .collect::<Vec<_>>();
        Arc::new(LineLayout {
            font_size: px(14.),
            width: px(glyphs.len() as f32),
            ascent: px(11.),
            descent: px(3.),
            runs: vec![ShapedRun {
                font_id: FontId(1),
                glyphs,
            }],
            len: text.len(),
        })
    };
    let text = "א".repeat(1024);
    let full = line(&text);
    let geometry = || {
        let runs = full
            .runs
            .iter()
            .map(|run| {
                let glyphs = run
                    .glyphs
                    .iter()
                    .map(|g| (g.id, g.position, g.index, g.is_emoji))
                    .collect::<Vec<_>>();
                (run.font_id, glyphs)
            })
            .collect::<Vec<_>>();
        (
            (
                full.font_size,
                full.width,
                full.ascent,
                full.descent,
                full.len,
            ),
            runs,
        )
    };
    let original = geometry();
    let mut requests = 0;
    let (shown, layout) = display_layout(&text, full.clone(), px(10.), |candidate| {
        requests += 1;
        line(candidate)
    })
    .expect("a logical prefix and ellipsis fit");

    assert_eq!(shown, format!("{}…", "א".repeat(9)));
    assert_eq!(layout.width, px(10.));
    assert_eq!(layout.len, shown.len());
    assert_eq!(
        layout
            .runs
            .iter()
            .map(|run| run.glyphs.len())
            .sum::<usize>(),
        10
    );
    assert!(
        requests <= 16,
        "shape requests including ellipsis: {requests}"
    );
    assert_eq!(geometry(), original);
}
