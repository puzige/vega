use std::{rc::Rc, sync::Arc};

use gpui::TextSystem;
use nsh_core::{
    AttributedText, MountItem, Points, ResolvedTextStyle, TextEngine, TextLayout, ViewKind,
};

use super::{
    ReadingGeometry, ReadingOpacityFence, ReadingOpacityRange, ReadingSnapshot, opacity, snapshot,
};
use crate::{
    geom::rect,
    surface::Surface,
    text::{GpuiTextEngine, OpacityObservation},
    view::NshView,
};

struct Fixture {
    surface: Rc<Surface>,
    text: NshView,
    content: Arc<AttributedText>,
    layout: TextLayout,
}

impl Fixture {
    fn new(system: Arc<TextSystem>, source: &str) -> Self {
        assert!(
            !system.all_font_names().is_empty(),
            "real font-backed TextSystem required"
        );
        let engine = GpuiTextEngine::new(system, Arc::from(".SystemUIFont"));
        let mut content = AttributedText {
            selectable: Some(true),
            ..Default::default()
        };
        content.push(
            source,
            ResolvedTextStyle {
                font_size: Points(18.0),
                decoration: nsh_core::TextDecoration {
                    underline: true,
                    line_through: true,
                },
                background: Some(nsh_core::TextBackground {
                    color: nsh_core::Color::BLACK,
                    radius: Points(5.0),
                    node: 3,
                }),
                ..Default::default()
            },
        );
        let content = Arc::new(content);
        let prepared = engine.prepare(&content);
        let size = engine.measure(&prepared, Some(Points(200.0)));
        let layout = engine.layout(&prepared, size);
        let surface = Surface::new(nsh_engine::Environment::default());
        surface.resized(400.0, 240.0, 1.0);
        let mount = |item| surface.mounter.borrow_mut().apply(&surface, &item);
        mount(MountItem::Create {
            id: 3,
            kind: ViewKind::Text,
        });
        mount(MountItem::Insert {
            parent: nsh_core::ROOT_ID,
            child: 3,
            before: None,
        });
        let text = surface.view(3).expect("text mount");
        text.state_mut().frame = rect(0.0, 0.0, 220.0, 220.0);
        mount(MountItem::Text {
            id: 3,
            text: content.clone(),
            layout: layout.clone(),
            content: rect(3.0, 4.0, size.width.0, size.height.0),
        });
        surface.drawing();
        Self {
            surface,
            text,
            content,
            layout,
        }
    }

    fn snapshot(&self) -> ReadingSnapshot {
        snapshot(&self.surface, &self.text).expect("exact current snapshot")
    }

    fn fence(&self) -> ReadingOpacityFence {
        ReadingOpacityFence::capture(&self.surface, &self.snapshot()).expect("opacity fence")
    }

    fn observe(&self) -> OpacityObservation {
        let state = self.text.state();
        crate::text::observe_reading_opacity(state.text.as_ref().expect("drawn text"))
    }
}

fn range(start: u32, end: u32, opacity: f32) -> ReadingOpacityRange {
    ReadingOpacityRange {
        text: 0,
        range: start..end,
        opacity,
    }
}

pub fn opacity_rejects_invalid_ranges_atomically_and_full_replaces(system: Arc<TextSystem>) {
    let fixture = Fixture::new(system, "a😀bcdef");
    let fence = opacity::replace(&fixture.surface, &fixture.fence(), vec![range(0, 1, 0.2)])
        .expect("first publication");
    for invalid in [
        vec![range(1, 2, 0.5)],
        vec![range(2, 3, 0.5)],
        vec![range(0, 1000, 0.5)],
        vec![range(0, 0, 0.5)],
        vec![range(3, 4, f32::NAN)],
        vec![range(3, 4, f32::INFINITY)],
        vec![range(3, 4, -0.1)],
        vec![range(3, 4, 1.1)],
        vec![range(0, 3, 0.5), range(1, 4, 0.7)],
        vec![range(4, 5, 0.5), range(0, 1, 0.7)],
        vec![ReadingOpacityRange {
            text: 1,
            range: 0..1,
            opacity: 0.5,
        }],
    ] {
        assert!(opacity::replace(&fixture.surface, &fence, invalid).is_none());
        assert!(
            fixture
                .observe()
                .glyphs
                .iter()
                .any(|(origin, _, alpha)| *origin == 0 && *alpha == 0.2)
        );
        assert_eq!(opacity::retained_publications(&fixture.surface), 1);
    }
    let next = opacity::replace(&fixture.surface, &fence, vec![range(3, 4, 0.7)])
        .expect("full replacement");
    assert!(
        fixture
            .observe()
            .glyphs
            .iter()
            .any(|(origin, _, alpha)| *origin == 0 && *alpha == 1.0)
    );
    assert!(!opacity::clear(&fixture.surface, &fence));
    assert!(opacity::replace(&fixture.surface, &fence, Vec::new()).is_none());
    assert!(opacity::clear(&fixture.surface, &next));
    assert!(!opacity::clear(&fixture.surface, &next));
    assert_eq!(opacity::retained_publications(&fixture.surface), 0);
    assert!(
        fixture
            .observe()
            .glyphs
            .iter()
            .all(|(_, _, alpha)| *alpha == 1.0)
    );
}

pub fn opacity_keeps_snapshot_layout_and_geometry_exact(system: Arc<TextSystem>) {
    let fixture = Fixture::new(system, "office e\u{301} 😀 אבגד wrapped text");
    let snapshot = fixture.snapshot();
    let geometry =
        ReadingGeometry::capture(&fixture.surface, &snapshot).expect("real layout geometry");
    let rects = geometry.range_rects(0, 0..6).expect("actual rectangles");
    let mut fence = fixture.fence();
    for alpha in [0.0, 0.25, 0.5, 1.0] {
        fence = opacity::replace(&fixture.surface, &fence, vec![range(0, 6, alpha)])
            .expect("frame publication");
        assert!(super::is_current(&fixture.surface, &snapshot));
        assert!(geometry.current_for(&fixture.surface));
        assert_eq!(
            geometry.range_rects(0, 0..6).expect("unchanged geometry"),
            rects
        );
        let state = fixture.text.state();
        let drawn = state.text.as_ref().expect("drawn text");
        assert!(Arc::ptr_eq(&drawn.text, &fixture.content));
        assert!(Arc::ptr_eq(&drawn.layout.0, &fixture.layout.0));
        assert!(!fixture.surface.invalidated());
        drop(state);
        fixture.surface.drawing();
    }
    assert!(opacity::clear(&fixture.surface, &fence));
    assert!(geometry.current_for(&fixture.surface));
}

pub fn opacity_uses_native_cluster_origins_and_original_background_shape(system: Arc<TextSystem>) {
    let fixture = Fixture::new(system, "office e\u{301} 😀 אבגד");
    let baseline = fixture.observe();
    assert!(!baseline.glyphs.is_empty());
    assert!(
        baseline.glyphs.iter().any(|(_, emoji, _)| *emoji),
        "font fixture must actually shape color emoji"
    );
    assert!(!baseline.backgrounds.is_empty());
    let end = fixture.content.text.encode_utf16().count() as u32;
    let ranges = vec![range(0, 1, 0.0), range(1, 8, 0.3), range(8, end, 0.7)];
    let _fence = opacity::replace(&fixture.surface, &fixture.fence(), ranges.clone())
        .expect("cluster publication");
    let actual = fixture.observe();
    for ((origin, emoji, alpha), (old_origin, old_emoji, _)) in
        actual.glyphs.iter().zip(&baseline.glyphs)
    {
        assert_eq!((origin, emoji), (old_origin, old_emoji));
        assert_eq!(
            *alpha,
            ranges
                .iter()
                .find(|range| range.range.contains(origin))
                .map_or(1.0, |range| range.opacity)
        );
    }
    assert_eq!(actual.glyphs.len(), baseline.glyphs.len());
    assert!(
        actual
            .decoration_bands
            .iter()
            .any(|(_, _, alpha)| *alpha == 0.3)
    );
    for paint in actual.backgrounds {
        assert!(
            baseline
                .backgrounds
                .iter()
                .any(|original| original.rect == paint.rect
                    && original.radius == paint.radius
                    && original.color == paint.color)
        );
        assert!(paint.band.0 >= paint.rect.0);
        assert!(paint.band.0 + paint.band.1 <= paint.rect.0 + paint.rect.2 + 0.001);
    }
}

pub fn opacity_retires_selection_content_mount_stop_and_surface_aba(system: Arc<TextSystem>) {
    let fixture = Fixture::new(system.clone(), "a😀bcdef");
    let first = opacity::replace(&fixture.surface, &fixture.fence(), vec![range(0, 1, 0.0)])
        .expect("first publication");
    let captured = fixture.snapshot();
    let selected =
        super::install(&fixture.surface, &captured, (0, 0), (0, 1)).expect("selection mutation");
    assert!(opacity::replace(&fixture.surface, &first, vec![range(0, 1, 0.1)]).is_none());
    assert!(
        fixture
            .observe()
            .glyphs
            .iter()
            .all(|(_, _, alpha)| *alpha == 1.0)
    );
    let next =
        ReadingOpacityFence::capture(&fixture.surface, &selected).expect("new selection fence");
    let next = opacity::replace(&fixture.surface, &next, vec![range(0, 1, 0.2)])
        .expect("new selection publication");
    assert!(!opacity::clear(&fixture.surface, &first));
    fixture.surface.mounter.borrow_mut().apply(
        &fixture.surface,
        &MountItem::Text {
            id: 3,
            text: Arc::new((*fixture.content).clone()),
            layout: fixture.layout.clone(),
            content: rect(3.0, 4.0, 200.0, 100.0),
        },
    );
    assert!(!opacity::clear(&fixture.surface, &next));
    assert!(
        fixture
            .observe()
            .glyphs
            .iter()
            .all(|(_, _, alpha)| *alpha == 1.0)
    );
    fixture.surface.drawing();
    let current = opacity::replace(&fixture.surface, &fixture.fence(), vec![range(0, 1, 0.0)])
        .expect("fresh content publication");
    fixture
        .surface
        .mounter
        .borrow_mut()
        .apply(&fixture.surface, &MountItem::Remove { id: 3 });
    fixture.surface.mounter.borrow_mut().apply(
        &fixture.surface,
        &MountItem::Insert {
            parent: nsh_core::ROOT_ID,
            child: 3,
            before: None,
        },
    );
    fixture.surface.drawing();
    assert!(opacity::replace(&fixture.surface, &current, Vec::new()).is_none());
    let other = Fixture::new(system, "a😀bcdef");
    assert!(!opacity::clear(&other.surface, &current));
    let last = opacity::replace(&fixture.surface, &fixture.fence(), vec![range(0, 1, 0.0)])
        .expect("fresh mount publication");
    fixture.surface.stop();
    assert!(!opacity::clear(&fixture.surface, &last));
    assert_eq!(opacity::retained_publications(&fixture.surface), 0);
}

pub fn opacity_cross_text_publication_rejects_forged_snapshot_and_old_area_clear(
    system: Arc<TextSystem>,
) {
    let fixture = Fixture::new(system, "first 😀 second");
    let area = fixture.surface.root.clone();
    area.state_mut().props.selectable_view = true;
    fixture.surface.mounter.borrow_mut().apply(
        &fixture.surface,
        &MountItem::Create {
            id: 4,
            kind: ViewKind::Text,
        },
    );
    fixture.surface.mounter.borrow_mut().apply(
        &fixture.surface,
        &MountItem::Insert {
            parent: nsh_core::ROOT_ID,
            child: 4,
            before: None,
        },
    );
    let second = fixture.surface.view(4).expect("second mount");
    second.state_mut().frame = rect(0.0, 50.0, 220.0, 100.0);
    fixture.surface.mounter.borrow_mut().apply(
        &fixture.surface,
        &MountItem::Text {
            id: 4,
            text: fixture.content.clone(),
            layout: fixture.layout.clone(),
            content: rect(0.0, 0.0, 200.0, 100.0),
        },
    );
    fixture.surface.drawing();
    let single = opacity::replace(&fixture.surface, &fixture.fence(), vec![range(0, 1, 0.2)])
        .expect("single text publication");
    let current = snapshot(&fixture.surface, &area).expect("cross text snapshot");
    assert_eq!(current.texts.len(), 2);
    let mut forged = current.clone();
    forged.texts[0].content = Arc::new((*fixture.content).clone());
    assert!(ReadingOpacityFence::capture(&fixture.surface, &forged).is_none());
    let fence = ReadingOpacityFence::capture(&fixture.surface, &current).expect("cross text fence");
    assert!(
        opacity::replace(
            &fixture.surface,
            &fence,
            vec![
                ReadingOpacityRange {
                    text: 1,
                    range: 0..1,
                    opacity: 0.4
                },
                range(0, 1, 0.7),
            ]
        )
        .is_none()
    );
    let fence = opacity::replace(
        &fixture.surface,
        &fence,
        vec![
            range(0, 1, 0.4),
            ReadingOpacityRange {
                text: 1,
                range: 0..1,
                opacity: 0.7,
            },
        ],
    )
    .expect("ordered cross text publication");
    assert!(!opacity::clear(&fixture.surface, &single));
    assert!(
        fixture
            .observe()
            .glyphs
            .iter()
            .any(|(origin, _, alpha)| *origin == 0 && *alpha == 0.4)
    );
    let state = second.state();
    let observation =
        crate::text::observe_reading_opacity(state.text.as_ref().expect("second text"));
    assert!(
        observation
            .glyphs
            .iter()
            .any(|(origin, _, alpha)| *origin == 0 && *alpha == 0.7)
    );
    drop(state);
    assert!(opacity::clear(&fixture.surface, &fence));
    assert_eq!(opacity::retained_publications(&fixture.surface), 0);
}

pub fn opacity_publication_lifetime_is_bounded_and_does_not_retain_surface(
    system: Arc<TextSystem>,
) {
    let fixture = Fixture::new(system, "bounded current publication");
    let weak = Rc::downgrade(&fixture.surface);
    let mut fence = fixture.fence();
    for _ in 0..256 {
        fence = opacity::replace(&fixture.surface, &fence, vec![range(0, 1, 0.2)])
            .expect("replace publication");
        assert_eq!(opacity::retained_publications(&fixture.surface), 1);
    }
    drop(fixture);
    assert!(weak.upgrade().is_none());
    drop(fence);
}
