use super::{display_layout, frame_decorations, should_animate};
use gpui_kit::{FontId, GlyphId, LineLayout, Rgba, ShapedGlyph, ShapedRun, point, px, rgba};
use std::{collections::HashMap, sync::Arc};

fn line(text: &str, width: f32, runs: Vec<ShapedRun>) -> Arc<LineLayout> {
    Arc::new(LineLayout {
        font_size: px(14.),
        width: px(width),
        ascent: px(11.),
        descent: px(3.),
        runs,
        len: text.len(),
    })
}

fn run(font: usize, glyphs: &[(u32, usize, f32, f32, bool)]) -> ShapedRun {
    ShapedRun {
        font_id: FontId(font),
        glyphs: glyphs
            .iter()
            .map(|&(id, index, x, y, is_emoji)| ShapedGlyph {
                id: GlyphId(id),
                position: point(px(x), px(y)),
                index,
                is_emoji,
            })
            .collect(),
    }
}

#[test]
fn moving_colors_preserve_mixed_text_geometry_and_utf8_spans() {
    let text = "ffi中e\u{301}🙂";
    let layout = line(
        text,
        100.,
        vec![
            run(1, &[(11, 0, 0., 0., false)]),
            run(2, &[(22, 3, 30., 0., false)]),
            run(1, &[(33, 6, 50., 0., false), (34, 6, 50., -3., false)]),
            run(3, &[(44, 9, 80., 2., true)]),
        ],
    );
    let geometry = || {
        let metrics = (layout.font_size, layout.width, layout.len);
        let glyphs = layout
            .runs
            .iter()
            .flat_map(|run| {
                run.glyphs
                    .iter()
                    .map(move |g| (run.font_id, g.id, g.position, g.index, g.is_emoji))
            })
            .collect::<Vec<_>>();
        (metrics, (layout.ascent, layout.descent), glyphs)
    };
    let original = geometry();
    let base = rgba(0x33333366);
    let highlight = rgba(0xffffffff);
    let mut frames = Vec::new();
    for phase in [0., 0.5, 0.6875, 1.5] {
        let decorations =
            frame_decorations(text, &layout, layout.width, phase, base, highlight, true);
        assert_eq!(decorations.len(), text.chars().count());
        let mut end = 0;
        let mut colors = Vec::new();
        for (decoration, ch) in decorations.iter().zip(text.chars()) {
            assert_eq!(decoration.len as usize, ch.len_utf8());
            end += decoration.len as usize;
            assert!(text.is_char_boundary(end));
            assert!(decoration.background_color.is_none());
            assert!(decoration.underline.is_none());
            assert!(decoration.strikethrough.is_none());
            let color = Rgba::from(decoration.color);
            assert_eq!(color.a, base.a);
            assert!((color.r - color.g).abs() < 1e-5 && (color.r - color.b).abs() < 1e-5);
            colors.push(color);
        }
        assert_eq!(end, text.len());
        assert_eq!(geometry(), original);
        frames.push(colors);
    }
    assert!(frames[0].iter().all(|c| (c.r - base.r).abs() < 1e-5));
    assert!((frames[1][3].r - 1. / 3.).abs() < 1e-5);
    assert!((frames[1][4].r - 1.).abs() < 1e-5);
    assert!((frames[1][6].r - base.r).abs() < 1e-5);
    assert!((frames[2][4].r - base.r).abs() < 1e-5);
    assert!((frames[2][6].r - 1.).abs() < 1e-5);
    assert_ne!(frames[1][0], frames[1][4]);
    assert_ne!(frames[1], frames[2]);
    assert_eq!(frames[1], frames[3]);
    let tail = run(2, &[(22, 0, 0., 0., false), (55, 3, 30., 0., false)]);
    let cut = line("中…", 40., vec![tail]);
    let d = frame_decorations("中…", &cut, layout.width, 0.375, base, highlight, true);
    assert_eq!(d.iter().map(|run| run.len).collect::<Vec<_>>(), [3, 3]);
    assert!((Rgba::from(d[1].color).r - 1.).abs() < 1e-5);
}

#[test]
fn display_width_boundaries_shape_candidates_and_retreat_without_stale_output() {
    let text = "ffi中x";
    let glyphs = [
        (11, 0, 0., 0., false),
        (22, 3, 30., 0., false),
        (33, 6, 60., 0., false),
        (34, 6, 60., -2., false),
    ];
    let full = line(text, 90., vec![run(1, &glyphs)]);
    let shapes: HashMap<_, _> = [
        ("…", 10., vec![(55, 0, 0., 0., false)]),
        (
            "ffi…",
            45.,
            vec![(11, 0, 0., 0., false), (55, 3, 35., 0., false)],
        ),
        (
            "ffi中…",
            75.5,
            vec![
                (11, 0, 0., 0., false),
                (22, 3, 30., 0., false),
                (55, 6, 65.5, 0., false),
            ],
        ),
    ]
    .into_iter()
    .map(|(text, width, glyphs)| (text, line(text, width, vec![run(1, &glyphs)])))
    .collect();
    let mut shape = |candidate: &str| shapes[candidate].clone();
    assert!(display_layout("", line("", 0., vec![]), px(100.), &mut shape).is_none());
    for (available, expected) in [
        (-1., None),
        (0., None),
        (9., None),
        (10., Some("…")),
        (44., Some("…")),
        (45., Some("ffi…")),
        (70., Some("ffi…")),
        (75., Some("ffi…")),
        (75.5, Some("ffi中…")),
        (88.9, Some("ffi中…")),
        (89., Some(text)),
        (90., Some(text)),
        (10., Some("…")),
        (120., Some(text)),
    ] {
        let result = display_layout(text, full.clone(), px(available), &mut shape);
        assert_eq!(result.as_ref().map(|(shown, _)| shown.as_str()), expected);
        if let Some((shown, layout)) = result {
            let expected_layout = shapes.get(shown.as_str()).unwrap_or(&full);
            assert!(Arc::ptr_eq(&layout, expected_layout));
            assert!(layout.width <= px(available + if shown == text { 1. } else { 0. }));
            assert_eq!(layout.len, shown.len());
        }
    }
}

#[test]
fn empty_and_motion_gates_stop_colors_and_resume_on_the_fixed_layout() {
    let empty = line("", 0., vec![]);
    let glyphless = line("x", 100., vec![run(1, &[])]);
    let layout = line("x", 100., vec![run(1, &[(11, 0, 50., -2., false)])]);
    let base = rgba(0x33333366);
    let highlight = rgba(0xffffffff);
    assert!(!should_animate(None, true, true, false));
    for empty_layout in [&empty, &glyphless] {
        assert!(!should_animate(Some(empty_layout), true, true, false));
    }
    assert!(frame_decorations("", &empty, px(0.), 0.5, base, highlight, true).is_empty());
    for (visible, active, reduce_motion, expected) in [
        (true, true, false, true),
        (false, true, false, false),
        (true, false, false, false),
        (true, true, true, false),
        (false, false, false, false),
        (false, true, true, false),
        (true, false, true, false),
        (false, false, true, false),
        (true, true, false, true),
    ] {
        let moving = should_animate(Some(&layout), visible, active, reduce_motion);
        assert_eq!(moving, expected);
        let decorations =
            frame_decorations("x", &layout, layout.width, 0.5, base, highlight, moving);
        assert_eq!(decorations.len(), 1);
        let color = Rgba::from(decorations[0].color);
        for channel in [color.r, color.g, color.b] {
            assert!((channel - if expected { 1. } else { base.r }).abs() < 1e-5);
        }
        assert_eq!(color.a, base.a);
        assert_eq!(layout.runs[0].glyphs[0].position, point(px(50.), px(-2.)));
        assert_eq!(
            (layout.width, layout.ascent, layout.descent),
            (px(100.), px(11.), px(3.))
        );
    }
}
