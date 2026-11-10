use super::{Snapshot, ns, render};
use crate::conversation_stream::shimmer::{display_layout, frame_colors, should_animate};
use gpui_kit::{FontId, GlyphId, LineLayout, ShapedGlyph, ShapedRun, point, px};
use nsh_core::{ApproxTextEngine, Core, MountItem};
use std::sync::Arc;
use vega_theme::{Layout, Theme, Typography};

fn layout(text: &str, width: f32, indices: &[usize]) -> Arc<LineLayout> {
    Arc::new(LineLayout {
        width: px(width),
        len: text.len(),
        runs: vec![ShapedRun {
            font_id: FontId(0),
            glyphs: indices
                .iter()
                .enumerate()
                .map(|(position, &index)| ShapedGlyph {
                    id: GlyphId(0),
                    position: point(px(position as f32 * 10.), px(0.)),
                    index,
                    is_emoji: false,
                })
                .collect(),
        }],
        ..Default::default()
    })
}

#[test]
fn layout_acknowledgements_keep_phase_until_one_pending_animation_frame_is_delivered() {
    let epoch = std::time::Instant::now();
    let mut clock = super::FrameClock::default();
    assert_eq!(clock.phase(Some(epoch)), 0.);
    assert!(clock.request());
    for _ in 0..128 {
        assert!(!clock.request());
        assert_eq!(clock.phase(Some(epoch)), 0.);
    }
    let delivered = epoch + std::time::Duration::from_millis(500);
    assert!(clock.delivered(delivered));
    let phase = 500. / vega_theme::Motion::THINKING_SHIMMER_MS as f32;
    assert!((clock.phase(Some(epoch)) - phase).abs() < 0.00001);
    assert!(!clock.delivered(delivered + std::time::Duration::from_secs(1)));
    assert!((clock.phase(Some(epoch)) - phase).abs() < 0.00001);
    assert_eq!(clock.phase(None), 0.);
    assert!(clock.request());
    assert!(clock.delivered(delivered + std::time::Duration::from_millis(250)));
    assert!(clock.phase(Some(epoch)) > phase);
}

#[test]
fn truncation_keeps_shaped_clusters_and_measures_ellipsis() {
    let source = "e\u{301}中👩\u{200d}💻tail";
    let boundaries = [0, 3, 6, 17, 18, 19, 20];
    let full = layout(source, 100., &boundaries);
    let mut probes = Vec::new();
    let shown = display_layout(source, full, px(35.), |candidate| {
        probes.push(candidate.to_owned());
        let width = match candidate {
            "…" => 10.,
            "e\u{301}…" => 20.,
            "e\u{301}中…" => 30.,
            _ => 60.,
        };
        layout(candidate, width, &[0])
    })
    .expect("fitted text");
    assert_eq!(shown.0, "e\u{301}中…");
    assert!(shown.1.width <= px(35.));
    assert!(probes.len() <= 6);
    assert!(probes.iter().all(|candidate| {
        candidate == "…" || boundaries.contains(&(candidate.len() - '…'.len_utf8()))
    }));
}

#[test]
fn too_narrow_empty_and_one_pixel_fit_keep_original_contract() {
    let source = "中文";
    let full = layout(source, 30., &[0, 3]);
    let shape = |candidate: &str| layout(candidate, 10., &[0]);
    assert!(display_layout(source, full.clone(), px(9.), shape).is_none());
    assert!(display_layout(source, full.clone(), px(0.), shape).is_none());
    assert!(display_layout("", full.clone(), px(30.), shape).is_none());
    assert_eq!(
        display_layout(source, full.clone(), px(10.), shape)
            .expect("ellipsis")
            .0,
        "…"
    );
    let fitted = display_layout(source, full.clone(), px(29.), shape).expect("fit tolerance");
    assert_eq!(fitted.0, source);
    assert!(Arc::ptr_eq(&fitted.1, &full));
}

#[test]
fn canonical_sweep_colors_displayed_ellipsis_using_original_width() {
    let shown = "a…";
    let line = layout(shown, 20., &[0, 1]);
    let base = gpui_kit::Rgba {
        r: 0.,
        g: 0.2,
        b: 0.4,
        a: 0.6,
    };
    let high = gpui_kit::Rgba {
        r: 1.,
        g: 0.8,
        b: 0.6,
        a: 1.,
    };
    let colors = frame_colors(shown, &line, px(100.), 0.25, base, high, true);
    assert_eq!(colors.len(), 2);
    assert!((colors[0].r - (1. - 0.1 / 0.24)).abs() < 0.0001);
    assert!((colors[1].r - 1.).abs() < 0.0001);
    assert_eq!(colors[1].a, base.a);
    assert!(
        frame_colors(shown, &line, px(100.), 0.25, base, high, false)
            .iter()
            .all(|color| *color == base)
    );
    let repeated = frame_colors(shown, &line, px(100.), 1.25, base, high, true);
    assert!(repeated.iter().zip(colors).all(|(left, right)| {
        (left.r - right.r).abs() < 0.0001
            && (left.g - right.g).abs() < 0.0001
            && (left.b - right.b).abs() < 0.0001
            && left.a == right.a
    }));
}

#[test]
fn motion_requires_visible_active_shaped_text_without_reduced_motion() {
    let line = layout("a", 10., &[0]);
    assert!(should_animate(Some(&line), true, true, false));
    assert!(!should_animate(Some(&line), false, true, false));
    assert!(!should_animate(Some(&line), true, false, false));
    assert!(!should_animate(Some(&line), true, true, true));
    assert!(!should_animate(None, true, true, false));
    assert!(!should_animate(
        Some(&LineLayout::default()),
        true,
        true,
        false
    ));
}

thread_local! {
    static UPDATES: std::cell::RefCell<Option<tokio::sync::mpsc::Sender<Snapshot>>> = const { std::cell::RefCell::new(None) };
}

fn snapshot(shown: &str) -> Snapshot {
    Snapshot {
        shown: shown.into(),
        width: 180.,
        label: "完整标题 e\u{301} 👩\u{200d}💻".into(),
        colors: shown
            .chars()
            .map(|_| Theme::light().colors.conversation_stage)
            .collect(),
        font: gpui_kit::font(".SystemUIFont"),
    }
}

#[ns::component]
fn ProductionText() -> impl ns::IntoElement {
    let state = ns::signal_with(snapshot("e\u{301}中…"), ns::SignalEquals::Never);
    let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
    UPDATES.with(|updates| *updates.borrow_mut() = Some(sender));
    ns::spawn(async move {
        while let Some(next) = receiver.recv().await {
            state.set(next);
        }
    });
    let (commands, _) = tokio::sync::mpsc::channel(1);
    render(state, commands)
}

#[test]
fn production_native_text_keeps_content_style_label_and_replaces_old_projection() {
    let mut core = Core::new(
        ApproxTextEngine,
        ns::size(ns::px(180.), ns::px(Layout::CONVERSATION_TOOL_HEIGHT)),
    );
    core.mount_root(&nsh_core::element::Root::of::<ProductionText>());
    let batch = core.commit().expect("mounted text");
    assert_passive_text(&batch);
    let text = batch
        .items
        .iter()
        .find_map(|item| match item {
            MountItem::Text { text, .. } if text.text == "e\u{301}中…" => Some(text),
            _ => None,
        })
        .expect("actual production NSH text");
    assert_eq!(text.max_lines, 1);
    assert_eq!(text.ellipsize, nsh_core::EllipsizeMode::Clip);
    assert!(text.spans.iter().all(|span| {
        span.style.font_size.0 == Typography::CONVERSATION
            && span.style.line_height.0 == Layout::CONVERSATION_TOOL_HEIGHT
    }));
    assert!(batch.items.iter().any(|item| matches!(item,
        MountItem::ViewProps { props, .. }
            if props.accessibility.label.as_deref() == Some("完整标题 e\u{301} 👩\u{200d}💻")
    )));
    UPDATES.with(|updates| {
        updates
            .borrow()
            .as_ref()
            .expect("sender")
            .try_send(snapshot("新标题 👩\u{200d}💻"))
            .expect("new source")
    });
    core.run_component_tasks();
    let batch = core.commit().expect("updated text");
    assert_passive_text(&batch);
    assert!(batch.items.iter().any(|item| matches!(item,
        MountItem::Text { text, .. } if text.text == "新标题 👩\u{200d}💻"
    )));
    assert!(!batch.items.iter().any(|item| matches!(item,
        MountItem::Text { text, .. } if text.text == "e\u{301}中…"
    )));
    UPDATES.with(|updates| *updates.borrow_mut() = None);
}

fn assert_passive_text(batch: &nsh_core::MountBatch) {
    for item in &batch.items {
        match item {
            MountItem::Events { events, .. } => assert!(events.is_empty()),
            MountItem::ViewProps { props, .. } => {
                assert!(props.accessibility.actions.is_empty());
                assert_eq!(props.accessibility.role, nsh_core::AccessibilityRole::None);
                assert!(!props.accessibility.hidden);
            }
            _ => {}
        }
    }
}
