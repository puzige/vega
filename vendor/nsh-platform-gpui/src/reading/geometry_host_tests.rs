#![allow(missing_docs)]

use std::{rc::Rc, sync::Arc};

use gpui::TextSystem;
use nsh_core::{
    Affinity, AttributedText, MountItem, Points, ResolvedTextStyle, TextEngine, TextLayout,
    TextPosition, TransformKind, ViewKind,
};

use super::{ReadingGeometry, snapshot};
use crate::{geom::rect, surface::Surface, text::GpuiTextEngine, view::NshView};

struct Fixture {
    surface: Rc<Surface>,
    scroll: NshView,
    text: NshView,
    engine: GpuiTextEngine,
    content: Arc<AttributedText>,
    layout: TextLayout,
}

impl Fixture {
    fn new(system: Arc<TextSystem>, source: &str, width: f32) -> Self {
        assert!(
            !system.all_font_names().is_empty(),
            "geometry regressions require a real font-backed TextSystem"
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
                ..Default::default()
            },
        );
        let content = Arc::new(content);
        let prepared = engine.prepare(&content);
        let size = engine.measure(&prepared, Some(Points(width)));
        let layout = engine.layout(&prepared, size);
        assert!(
            !engine.lines(&layout).is_empty(),
            "actual GpuiLayout required"
        );
        let surface = Surface::new(nsh_engine::Environment::default());
        surface.resized(400.0, 240.0, 1.0);
        let mount = |item| surface.mounter.borrow_mut().apply(&surface, &item);
        for (id, kind, parent) in [
            (2, ViewKind::Scroll, nsh_core::ROOT_ID),
            (3, ViewKind::Text, 2),
        ] {
            mount(MountItem::Create { id, kind });
            mount(MountItem::Insert {
                parent,
                child: id,
                before: None,
            });
        }
        let scroll = surface.view(2).expect("scroll mount");
        let text = surface.view(3).expect("text mount");
        scroll.state_mut().frame = rect(10.0, 20.0, 220.0, 100.0);
        {
            let mut state = scroll.state_mut();
            let scroll = state.scroll.as_mut().expect("scroll state");
            scroll.content = nsh_core::Size::new(Points(300.0), Points(1000.0));
        }
        text.state_mut().frame = rect(7.0, 9.0, width + 10.0, size.height.0 + 10.0);
        mount(MountItem::Text {
            id: 3,
            text: content.clone(),
            layout: layout.clone(),
            content: rect(3.0, 4.0, size.width.0, size.height.0),
        });
        text.changed();
        surface.drawing();
        Self {
            surface,
            scroll,
            text,
            engine,
            content,
            layout,
        }
    }

    fn geometry(&self) -> ReadingGeometry {
        ReadingGeometry::capture(
            &self.surface,
            &snapshot(&self.surface, &self.text).expect("current snapshot"),
        )
        .expect("geometry requires parent Surface invalidation hook")
    }

    fn moved(&self) {
        self.text.changed();
        self.surface.drawing();
    }
}

fn assert_revoked(geometry: &ReadingGeometry, surface: &Rc<Surface>) {
    assert!(!geometry.current_for(surface));
    assert!(geometry.range_rects(0, 0..1).is_none());
    assert!(geometry.caret_rect(0, TextPosition::default()).is_none());
    assert!(geometry.hit_test(0, [21.0, 34.0]).is_none());
}

pub fn geometry_uses_shaped_wrap_affinity_bidi_and_utf16(system: Arc<TextSystem>) {
    let fixture = Fixture::new(
        system,
        "Wi office e\u{301} 😀 אבגד text with wrapping across several lines",
        110.0,
    );
    let geometry = fixture.geometry();
    let lines = fixture.engine.lines(&fixture.layout);
    assert!(lines.len() > 1);
    let wrap = lines
        .windows(2)
        .find(|pair| pair[0].end == pair[1].start)
        .expect("soft wrap");
    let offset = wrap[0].end;
    for affinity in [Affinity::Upstream, Affinity::Downstream] {
        let position = TextPosition::new(offset, affinity);
        let actual = geometry.caret_rect(0, position).expect("caret");
        let expected = fixture
            .engine
            .caret_rect(&fixture.layout, position)
            .expect("layout caret");
        assert_eq!(
            actual,
            [
                expected.origin.x.0 + 20.0,
                expected.origin.y.0 + 33.0,
                0.0,
                expected.size.height.0
            ]
        );
    }
    let up = geometry
        .caret_rect(0, TextPosition::new(offset, Affinity::Upstream))
        .expect("upstream");
    let down = geometry
        .caret_rect(0, TextPosition::new(offset, Affinity::Downstream))
        .expect("downstream");
    assert!(up[1] < down[1]);
    let hit = geometry
        .hit_test(0, [up[0], up[1] + up[3] * 0.5])
        .expect("wrapped hit");
    assert_eq!(hit, TextPosition::new(offset, Affinity::Upstream));
    let first = fixture
        .engine
        .caret_rect(&fixture.layout, TextPosition::new(0, Affinity::Downstream))
        .expect("W start");
    let second = fixture
        .engine
        .caret_rect(&fixture.layout, TextPosition::new(1, Affinity::Downstream))
        .expect("i start");
    let third = fixture
        .engine
        .caret_rect(&fixture.layout, TextPosition::new(2, Affinity::Downstream))
        .expect("space start");
    assert!(
        second.origin.x.0 - first.origin.x.0 > third.origin.x.0 - second.origin.x.0,
        "proportional shaping required"
    );
    let emoji_start = fixture.content.text[..fixture.content.text.find('😀').expect("emoji")]
        .encode_utf16()
        .count() as u32;
    assert!(
        geometry
            .range_rects(0, emoji_start..emoji_start + 2)
            .is_some()
    );
    assert!(
        geometry
            .range_rects(0, emoji_start..emoji_start + 1)
            .is_none()
    );
    assert!(
        geometry
            .caret_rect(0, TextPosition::new(emoji_start + 1, Affinity::Downstream))
            .is_none()
    );
    assert!(geometry.range_rects(1, 0..1).is_none());
    assert!(geometry.range_rects(0, 9999..10000).is_none());
    let mut direct = Vec::new();
    fixture
        .engine
        .range_rects(&fixture.layout, 0..offset, &mut direct);
    let expected: Vec<_> = direct
        .into_iter()
        .map(|rect| {
            [
                rect.origin.x.0 + 20.0,
                rect.origin.y.0 + 33.0,
                rect.size.width.0,
                rect.size.height.0,
            ]
        })
        .collect();
    assert_eq!(
        geometry
            .range_rects(0, 0..offset)
            .expect("exact shaped range"),
        expected
    );
}

pub fn geometry_clips_ranges_and_hits_but_preserves_unclipped_carets(system: Arc<TextSystem>) {
    let fixture = Fixture::new(
        system,
        "Wide content wraps and remains available below the scroll viewport for source anchoring. "
            .repeat(8)
            .as_str(),
        150.0,
    );
    fixture.scroll.state_mut().frame = rect(10.0, 20.0, 80.0, 38.0);
    fixture
        .scroll
        .state_mut()
        .scroll
        .as_mut()
        .expect("scroll")
        .y = Points(30.0);
    fixture.moved();
    let geometry = fixture.geometry();
    let area = fixture.scroll.clone();
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
            parent: 2,
            child: 4,
            before: None,
        },
    );
    let second = fixture.surface.view(4).expect("second mounted text");
    second.state_mut().frame = rect(7.0, 9.0, 160.0, 1000.0);
    fixture.surface.mounter.borrow_mut().apply(
        &fixture.surface,
        &MountItem::Text {
            id: 4,
            text: fixture.content.clone(),
            layout: fixture.layout.clone(),
            content: rect(3.0, 4.0, 150.0, 1000.0),
        },
    );
    fixture.moved();
    assert_revoked(&geometry, &fixture.surface);
    let captured = snapshot(&fixture.surface, &area).expect("cross-text snapshot");
    assert_eq!(captured.texts.len(), 2);
    let geometry =
        ReadingGeometry::capture(&fixture.surface, &captured).expect("cross-text geometry");
    let position = TextPosition::default();
    assert_eq!(
        geometry.caret_rect(0, position),
        geometry.caret_rect(1, position)
    );
    let end = fixture.content.text.encode_utf16().count() as u32;
    let rectangles = geometry.range_rects(0, 0..end).expect("range");
    assert!(!rectangles.is_empty());
    for [x, y, width, height] in rectangles {
        assert!(x >= 10.0 && y >= 20.0 && x + width <= 90.0 && y + height <= 58.0);
    }
    let caret = geometry
        .caret_rect(0, TextPosition::default())
        .expect("unclipped caret");
    assert_eq!(caret[1], 3.0);
    assert!(geometry.hit_test(0, [caret[0], caret[1]]).is_none());
    assert!(geometry.hit_test(0, [21.0, 34.0]).is_some());
    for point in [
        [f32::NAN, 30.0],
        [21.0, f32::INFINITY],
        [9.0, 34.0],
        [90.0, 34.0],
    ] {
        assert!(geometry.hit_test(0, point).is_none());
    }
}

pub fn geometry_survives_real_scrollbar_fade_but_rejects_scroll_aba(system: Arc<TextSystem>) {
    let fixture = Fixture::new(
        system,
        "Stationary wrapped thinking link remains current during native scrollbar fade.",
        110.0,
    );
    crate::scroll::reading_geometry_scroll_step(&fixture.surface, &fixture.scroll, 10.0);
    fixture.surface.drawing();
    let geometry = fixture.geometry();
    let ranges = geometry.range_rects(0, 0..10).expect("visible range");
    let start = fixture.surface.clock() * 1000.0;
    for elapsed in [400.0, 700.0, 750.0, 850.0, 999.0, 1001.0] {
        let before = fixture
            .scroll
            .state()
            .scroll
            .as_ref()
            .expect("scroll")
            .live
            .alpha;
        crate::scroll::reading_geometry_scroll_frame(
            &fixture.surface,
            &fixture.scroll,
            start + elapsed,
        );
        let after = fixture
            .scroll
            .state()
            .scroll
            .as_ref()
            .expect("scroll")
            .live
            .alpha;
        if after != before {
            assert!(fixture.surface.invalidated());
        }
        assert!(geometry.current_for(&fixture.surface));
        assert_eq!(
            geometry.range_rects(0, 0..10).expect("stationary range"),
            ranges
        );
        fixture.surface.drawing();
    }
    assert_eq!(
        fixture
            .scroll
            .state()
            .scroll
            .as_ref()
            .expect("scroll")
            .live
            .alpha,
        0.0
    );
    crate::scroll::reading_geometry_scroll_step(&fixture.surface, &fixture.scroll, 10.0);
    assert_revoked(&geometry, &fixture.surface);
    crate::scroll::reading_geometry_scroll_step(&fixture.surface, &fixture.scroll, -10.0);
    fixture.surface.drawing();
    assert_revoked(&geometry, &fixture.surface);
}

pub fn geometry_revokes_scroll_resize_origin_scale_and_repaint_aba(system: Arc<TextSystem>) {
    let fixture = Fixture::new(
        system,
        "A mounted exact geometry lease wraps across retained text lines.",
        110.0,
    );
    let geometry = fixture.geometry();
    fixture
        .scroll
        .state_mut()
        .scroll
        .as_mut()
        .expect("scroll")
        .y = Points(10.0);
    fixture.moved();
    assert_revoked(&geometry, &fixture.surface);
    fixture
        .scroll
        .state_mut()
        .scroll
        .as_mut()
        .expect("scroll")
        .y = Points(0.0);
    fixture.moved();
    assert_revoked(&geometry, &fixture.surface);
    for (width, height, scale) in [(410.0, 240.0, 1.0), (400.0, 240.0, 2.0)] {
        let geometry = fixture.geometry();
        fixture.surface.resized(width, height, scale);
        fixture.surface.resized(400.0, 240.0, 1.0);
        fixture.surface.drawing();
        assert_revoked(&geometry, &fixture.surface);
    }
    let geometry = fixture.geometry();
    fixture.surface.origin.set((11.0, 12.0));
    assert_revoked(&geometry, &fixture.surface);
    super::geometry_invalidated(&fixture.surface);
    fixture.surface.origin.set((0.0, 0.0));
    assert_revoked(&geometry, &fixture.surface);
    let geometry = fixture.geometry();
    fixture.surface.invalidate();
    assert_revoked(&geometry, &fixture.surface);
    fixture.surface.drawing();
    assert_revoked(&geometry, &fixture.surface);
}

pub fn geometry_rejects_layout_content_mount_selection_and_surface_retirement(
    system: Arc<TextSystem>,
) {
    let fixture = Fixture::new(system, "Retained exact layout and content identity.", 170.0);
    let geometry = fixture.geometry();
    let other = Surface::new(nsh_engine::Environment::default());
    assert!(!geometry.current_for(&other));
    fixture.text.state_mut().text.as_mut().expect("text").layout =
        TextLayout(Arc::new(crate::text::GpuiLayout::default()));
    assert_revoked(&geometry, &fixture.surface);
    fixture.moved();
    assert_revoked(&geometry, &fixture.surface);
    fixture.text.state_mut().text.as_mut().expect("text").layout = fixture.layout.clone();
    fixture.moved();
    assert_revoked(&geometry, &fixture.surface);
    let geometry = fixture.geometry();
    fixture.text.state_mut().text.as_mut().expect("text").text =
        Arc::new((*fixture.content).clone());
    fixture.moved();
    assert_revoked(&geometry, &fixture.surface);
    fixture.text.state_mut().text.as_mut().expect("text").text = fixture.content.clone();
    fixture.moved();
    assert_revoked(&geometry, &fixture.surface);
    let geometry = fixture.geometry();
    let current = snapshot(&fixture.surface, &fixture.text).expect("snapshot");
    assert!(super::install(&fixture.surface, &current, (0, 0), (0, 3)).is_some());
    assert_revoked(&geometry, &fixture.surface);
    fixture.surface.drawing();
    let geometry = fixture.geometry();
    fixture.text.unparent();
    fixture.text.insert_before(&fixture.scroll, None);
    fixture.surface.drawing();
    assert_revoked(&geometry, &fixture.surface);
    let geometry = fixture.geometry();
    fixture.surface.stop();
    assert_revoked(&geometry, &fixture.surface);
}

pub fn geometry_accepts_translation_uniform_scale_and_rejects_unsupported_clips(
    system: Arc<TextSystem>,
) {
    let fixture = Fixture::new(
        system,
        "אבגד proportional text and exact transformed rectangles",
        180.0,
    );
    fixture.text.state_mut().style.transform_origin = nsh_core::TransformOrigin {
        x: Points::ZERO.into(),
        y: Points::ZERO.into(),
        z: Points::ZERO,
    };
    let rtl_start = fixture
        .geometry()
        .caret_rect(0, TextPosition::new(0, Affinity::Downstream))
        .expect("rtl start");
    let rtl_next = fixture
        .geometry()
        .caret_rect(0, TextPosition::new(1, Affinity::Downstream))
        .expect("rtl next");
    assert!(rtl_start[0] > rtl_next[0]);
    let point = [rtl_start[0] - 1.0, rtl_start[1] + rtl_start[3] * 0.5];
    let expected = fixture.engine.hit_test(
        &fixture.layout,
        nsh_core::geometry::point(Points(point[0] - 20.0), Points(point[1] - 33.0)),
    );
    assert_eq!(fixture.geometry().hit_test(0, point), Some(expected));
    let mut transform = nsh_core::Transform::default();
    assert!(transform.push(TransformKind::TranslateX, 5.0).is_some());
    assert!(transform.push(TransformKind::Scale, 2.0).is_some());
    fixture.text.state_mut().style.transform = transform;
    fixture.scroll.state_mut().frame = rect(10.0, 20.0, 380.0, 100.0);
    fixture.moved();
    let geometry = fixture.geometry();
    let position = TextPosition::new(0, Affinity::Downstream);
    let direct = fixture
        .engine
        .caret_rect(&fixture.layout, position)
        .expect("layout caret");
    let matrix = fixture.text.to_root().expect("mounted transform");
    let mapped = matrix
        .map(direct.origin.x.0 + 3.0, direct.origin.y.0 + 4.0)
        .expect("affine caret");
    assert_eq!(
        geometry.caret_rect(0, position).expect("scaled caret"),
        [mapped.0, mapped.1, 0.0, direct.size.height.0 * 2.0]
    );
    assert!(
        !geometry
            .range_rects(0, 0..4)
            .expect("scaled range")
            .is_empty()
    );
    for kind in [
        TransformKind::Rotate,
        TransformKind::RotateX,
        TransformKind::RotateY,
        TransformKind::SkewX,
        TransformKind::Perspective,
        TransformKind::ScaleX,
    ] {
        let mut transform = nsh_core::Transform::default();
        assert!(
            transform
                .push(
                    kind,
                    if kind == TransformKind::Perspective {
                        400.0
                    } else {
                        15.0
                    }
                )
                .is_some()
        );
        fixture.text.state_mut().style.transform = transform;
        fixture.moved();
        let snapshot = snapshot(&fixture.surface, &fixture.text).expect("snapshot");
        assert!(ReadingGeometry::capture(&fixture.surface, &snapshot).is_none());
    }
    fixture.text.state_mut().style.transform = Default::default();
    fixture.moved();
    let geometry = fixture.geometry();
    fixture.scroll.state_mut().style.border_radius = Points(8.0);
    fixture.moved();
    let snapshot = snapshot(&fixture.surface, &fixture.text).expect("snapshot");
    assert!(ReadingGeometry::capture(&fixture.surface, &snapshot).is_none());
    assert_revoked(&geometry, &fixture.surface);
    fixture.scroll.state_mut().style.border_radius = Points(0.0);
    fixture.text.state_mut().shift.0 = f32::NAN;
    fixture.moved();
    assert!(ReadingGeometry::capture(&fixture.surface, &snapshot).is_none());
}
