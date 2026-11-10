#![cfg(feature = "vega-host-regressions")]

use std::{rc::Rc, sync::Arc};

use gpui::TextSystem;
use nsh_core::{
    Affinity, AttributedText, Command, MountItem, Points, ResolvedTextStyle, TextEngine,
    TextPosition, TransformKind, ViewKind,
};

use super::{ReadingGeometry, snapshot};
use crate::{geom::rect, surface::Surface, text::GpuiTextEngine, view::NshView};

struct Fixture {
    surface: Rc<Surface>,
    scroll: NshView,
    text: NshView,
}

impl Fixture {
    fn new(system: Arc<TextSystem>) -> Self {
        assert!(!system.all_font_names().is_empty(), "real fonts required");
        let engine = GpuiTextEngine::new(system, Arc::from(".SystemUIFont"));
        let mut content = AttributedText {
            selectable: Some(true),
            ..Default::default()
        };
        content.push(
            &"Wide proportional אבגד 😀 source caret text wraps below the viewport. ".repeat(20),
            ResolvedTextStyle {
                font_size: Points(18.0),
                ..Default::default()
            },
        );
        let content = Arc::new(content);
        let prepared = engine.prepare(&content);
        let size = engine.measure(&prepared, Some(Points(180.0)));
        let layout = engine.layout(&prepared, size);
        assert!(!engine.lines(&layout).is_empty(), "real layout required");
        let surface = Surface::new(nsh_engine::Environment::default());
        surface.resized(400.0, 240.0, 1.0);
        for (id, kind, parent) in [
            (2, ViewKind::Scroll, nsh_core::ROOT_ID),
            (3, ViewKind::Text, 2),
            (4, ViewKind::Scroll, nsh_core::ROOT_ID),
        ] {
            surface
                .mounter
                .borrow_mut()
                .apply(&surface, &MountItem::Create { id, kind });
            surface.mounter.borrow_mut().apply(
                &surface,
                &MountItem::Insert {
                    parent,
                    child: id,
                    before: None,
                },
            );
        }
        let scroll = surface.view(2).expect("mounted scroll");
        let text = surface.view(3).expect("mounted text");
        scroll.state_mut().frame = rect(10.0, 20.0, 220.0, 100.0);
        {
            let mut state = scroll.state_mut();
            let scroll = state.scroll.as_mut().expect("scroll state");
            scroll.content = nsh_core::Size::new(Points(300.0), Points(1000.0));
            scroll.behavior.enabled = true;
            scroll.x = Points(17.0);
            scroll.y = Points(40.0);
        }
        text.state_mut().frame = rect(7.0, 9.0, 190.0, size.height.0 + 10.0);
        surface.mounter.borrow_mut().apply(
            &surface,
            &MountItem::Text {
                id: 3,
                text: content,
                layout,
                content: rect(3.0, 4.0, size.width.0, size.height.0),
            },
        );
        text.changed();
        surface.drawing();
        Self {
            surface,
            scroll,
            text,
        }
    }

    fn geometry(&self) -> ReadingGeometry {
        ReadingGeometry::capture(
            &self.surface,
            &snapshot(&self.surface, &self.text).expect("current snapshot"),
        )
        .expect("current real geometry")
    }

    fn state(&self) -> (f32, f32, u32) {
        let state = self.scroll.state();
        let scroll = state.scroll.as_ref().expect("scroll state");
        (scroll.x.0, scroll.y.0, scroll.applied)
    }

    fn redrawn(&self) {
        self.text.changed();
        self.surface.drawing();
    }
}

#[doc = "Checks actual shaped caret displacement, core accounting, horizontal preservation and one-shot lease revocation."]
pub fn anchor_moves_real_caret_once_and_reports_core_application(system: Arc<TextSystem>) {
    let fixture = Fixture::new(system);
    let geometry = fixture.geometry();
    let position = TextPosition::new(0, Affinity::Downstream);
    let before = geometry.caret_rect(0, position).expect("caret");
    assert!(geometry.scroll_anchor(&fixture.surface, 2, 25.0));
    assert_eq!(fixture.state(), (17.0, 65.0, 1));
    assert!(fixture.surface.invalidated());
    assert!(!geometry.current_for(&fixture.surface));
    assert!(!geometry.scroll_anchor(&fixture.surface, 2, 25.0));
    assert_eq!(fixture.state(), (17.0, 65.0, 1));
    fixture.surface.drawing();
    let after = fixture
        .geometry()
        .caret_rect(0, position)
        .expect("new caret");
    assert_eq!(after[0], before[0]);
    assert_eq!(after[1], before[1] - 25.0);
    assert_eq!(&after[2..], &before[2..]);
    assert!(crate::scroll::reading_anchor_busy(
        &fixture.surface,
        &fixture.scroll
    ));
    assert!(!fixture.geometry().scroll_anchor(&fixture.surface, 2, 10.0));
    fixture.surface.tick();
    fixture.surface.drawing();
    assert!(!crate::scroll::reading_anchor_busy(
        &fixture.surface,
        &fixture.scroll
    ));
    assert!(fixture.geometry().scroll_anchor(&fixture.surface, 2, -25.0));
    assert_eq!(fixture.state(), (17.0, 40.0, 2));
}

#[doc = "Checks clamping against native content insets and extent without off-axis mutation."]
pub fn anchor_clamps_to_actual_inset_extent(system: Arc<TextSystem>) {
    for (delta, expected) in [(10000.0, 930.0), (-10000.0, 0.0)] {
        let fixture = Fixture::new(system.clone());
        {
            let mut state = fixture.scroll.state_mut();
            let scroll = state.scroll.as_mut().expect("scroll");
            scroll.behavior.content_inset.top = Points(10.0);
            scroll.behavior.content_inset.bottom = Points(20.0);
        }
        fixture.redrawn();
        assert!(fixture.geometry().scroll_anchor(&fixture.surface, 2, delta));
        assert_eq!(fixture.state(), (17.0, expected, 1));
    }
}

#[doc = "Checks invalid numbers, no-op requests, wrong owners and nonancestor nodes do not mutate core accounting."]
pub fn anchor_rejects_invalid_foreign_and_noop_requests(system: Arc<TextSystem>) {
    let fixture = Fixture::new(system.clone());
    let foreign = Fixture::new(system);
    let geometry = fixture.geometry();
    let original = fixture.state();
    for delta in [
        0.0,
        -0.0,
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::MIN_POSITIVE,
    ] {
        assert!(!geometry.scroll_anchor(&fixture.surface, 2, delta));
        assert_eq!(fixture.state(), original);
        assert!(geometry.current_for(&fixture.surface));
    }
    for node in [3, 4, 9999, nsh_core::ROOT_ID] {
        assert!(!geometry.scroll_anchor(&fixture.surface, node, 10.0));
        assert_eq!(fixture.state(), original);
    }
    assert!(geometry.current_for(&fixture.surface));
    assert!(!geometry.scroll_anchor(&foreign.surface, 2, 10.0));
    assert_eq!(foreign.state(), original);
    fixture
        .scroll
        .state_mut()
        .scroll
        .as_mut()
        .expect("scroll")
        .y = Points(900.0);
    fixture.redrawn();
    let edge = fixture.geometry();
    assert!(!edge.scroll_anchor(&fixture.surface, 2, 10.0));
    assert_eq!(fixture.state(), (17.0, 900.0, 0));
    assert!(edge.current_for(&fixture.surface));
}

#[doc = "Checks repaint, resize, equal-offset newer follow command, and scroll-position ABA revoke old leases."]
pub fn anchor_rejects_stale_resize_follow_and_scroll_aba(system: Arc<TextSystem>) {
    for action in 0..5 {
        let fixture = Fixture::new(system.clone());
        let geometry = fixture.geometry();
        match action {
            0 => fixture.surface.invalidate(),
            1 => {
                fixture.surface.resized(410.0, 250.0, 1.0);
                fixture.surface.resized(400.0, 240.0, 1.0);
            }
            2 => crate::scroll::command(
                &fixture.surface,
                &fixture.scroll,
                &Command::ScrollTo {
                    x: Points(17.0),
                    y: Points(40.0),
                    animated: false,
                },
            ),
            3 => {
                crate::scroll::scroll_by(&fixture.surface, &fixture.scroll, 0.0, 10.0);
                crate::scroll::scroll_by(&fixture.surface, &fixture.scroll, 0.0, -10.0);
            }
            4 => {
                crate::scroll::wheel_begin(&fixture.surface, &fixture.scroll);
                crate::scroll::wheel_end(&fixture.surface, &fixture.scroll, 0.0, 0.0);
            }
            _ => unreachable!(),
        }
        fixture.surface.drawing();
        let before = fixture.state();
        assert!(!geometry.scroll_anchor(&fixture.surface, 2, 10.0));
        assert_eq!(fixture.state(), before);
        assert!(!geometry.current_for(&fixture.surface));
    }
}

#[doc = "Checks detached and reused native node identities cannot act through an old geometry lease."]
pub fn anchor_rejects_detached_and_reused_scroll_nodes(system: Arc<TextSystem>) {
    let fixture = Fixture::new(system);
    let geometry = fixture.geometry();
    fixture.scroll.unparent();
    fixture.surface.drawing();
    assert!(!geometry.scroll_anchor(&fixture.surface, 2, 10.0));
    fixture.scroll.insert_before(&fixture.surface.root, None);
    fixture.surface.drawing();
    assert!(!geometry.scroll_anchor(&fixture.surface, 2, 10.0));
    let current = fixture.geometry();
    fixture
        .surface
        .mounter
        .borrow_mut()
        .apply(&fixture.surface, &MountItem::Delete { id: 2 });
    fixture.surface.mounter.borrow_mut().apply(
        &fixture.surface,
        &MountItem::Create {
            id: 2,
            kind: ViewKind::Scroll,
        },
    );
    fixture.surface.mounter.borrow_mut().apply(
        &fixture.surface,
        &MountItem::Insert {
            parent: nsh_core::ROOT_ID,
            child: 2,
            before: None,
        },
    );
    fixture.surface.drawing();
    assert!(!current.scroll_anchor(&fixture.surface, 2, 10.0));
    assert_eq!(fixture.state(), (17.0, 40.0, 0));
    assert_ne!(
        fixture.surface.view(2).expect("replacement"),
        fixture.scroll
    );
}

#[doc = "Checks native touchpad ownership, glide, animated follow, hidden hosts, refreshing, horizontal scrolling and scaled geometry fail closed."]
pub fn anchor_rejects_active_input_motion_and_unsupported_geometry(system: Arc<TextSystem>) {
    for action in 0..9 {
        let fixture = Fixture::new(system.clone());
        match action {
            0 => {
                crate::input::touchpad(
                    &fixture.surface,
                    crate::input::WheelEvent {
                        x: 50.0,
                        y: 50.0,
                        dx: 0.0,
                        dy: 0.0,
                        precise: true,
                        modifiers: crate::input::Modifiers::default(),
                        time: fixture.surface.clock(),
                    },
                    crate::input::GesturePhase::Began,
                );
            }
            1 => {
                crate::scroll::wheel_begin(&fixture.surface, &fixture.scroll);
                crate::scroll::wheel_end(&fixture.surface, &fixture.scroll, 0.0, 1000.0);
            }
            2 => crate::scroll::command(
                &fixture.surface,
                &fixture.scroll,
                &Command::ScrollTo {
                    x: Points(17.0),
                    y: Points(400.0),
                    animated: true,
                },
            ),
            3 => {}
            4 => {
                fixture
                    .scroll
                    .state_mut()
                    .scroll
                    .as_mut()
                    .expect("scroll")
                    .live
                    .refreshing = true
            }
            5 => {
                fixture
                    .scroll
                    .state_mut()
                    .scroll
                    .as_mut()
                    .expect("scroll")
                    .horizontal = true
            }
            6 => {
                let mut transform = nsh_core::Transform::default();
                assert!(transform.push(TransformKind::Scale, 2.0).is_some());
                fixture.text.state_mut().style.transform = transform;
            }
            7 => fixture.surface.input_pressed.set(1),
            8 => {
                fixture
                    .scroll
                    .state_mut()
                    .scroll
                    .as_mut()
                    .expect("scroll")
                    .behavior
                    .max_zoom = 2.0;
            }
            _ => unreachable!(),
        }
        fixture.redrawn();
        let geometry = fixture.geometry();
        if action == 3 {
            fixture.surface.window_hidden.set(true);
        }
        let before = fixture.state();
        assert!(!geometry.scroll_anchor(&fixture.surface, 2, 10.0));
        assert_eq!(fixture.state(), before);
    }
}
