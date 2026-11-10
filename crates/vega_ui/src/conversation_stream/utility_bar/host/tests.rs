use super::{Draft, Geometry, Measurement};
use crate::conversation_stream::utility_bar::view::{page_bounds, reveal, visible};
use nsh_core::prelude::{Bounds, Point, Size, px};
use std::sync::Arc;

fn rect(x: f32, y: f32, width: f32, height: f32) -> Bounds<nsh_core::Points> {
    Bounds {
        origin: Point { x: px(x), y: px(y) },
        size: Size {
            width: px(width),
            height: px(height),
        },
    }
}

#[test]
fn newer_search_or_composition_blocks_project_choice_until_acknowledged() {
    let mut draft = Draft {
        text: "project".to_owned(),
        composing: false,
        revision: 7,
    };
    assert!(draft.accepts(7));
    draft.revision = 8;
    assert!(!draft.accepts(7));
    draft.composing = true;
    assert!(!draft.accepts(8));
    draft.composing = false;
    assert!(draft.accepts(8));
}

#[test]
fn removed_or_scrolled_rows_cannot_reuse_their_measurement() {
    let row = Arc::new(());
    let measurement = Measurement {
        bounds: rect(0., 0., 200., 32.),
        scroll: 11,
        lease: Arc::downgrade(&row),
    };
    assert!(measurement.current(11));
    assert!(!measurement.current(12));
    drop(row);
    assert!(!measurement.current(11));
}

#[test]
fn scrolled_measurement_uses_root_page_position_instead_of_parent_frame() {
    let measured = nsh_core::Measurement {
        frame: rect(0., 640., 200., 32.),
        page: Point {
            x: px(12.),
            y: px(44.),
        },
    };
    let bounds = page_bounds(measured);
    assert_eq!(bounds, rect(12., 44., 200., 32.));
    assert!(visible(bounds, rect(12., 12., 200., 96.)));
}

#[test]
fn native_row_bounds_use_surface_origin_and_reject_clipping() {
    let row = Arc::new(());
    let measurement = Measurement {
        bounds: rect(8., 40., 160., 32.),
        scroll: 0,
        lease: Arc::downgrade(&row),
    };
    let geometry = Geometry {
        origin: gpui_kit::point(gpui_kit::px(100.), gpui_kit::px(200.)),
        bounds: gpui_kit::Bounds {
            origin: gpui_kit::point(gpui_kit::px(100.), gpui_kit::px(220.)),
            size: gpui_kit::size(gpui_kit::px(200.), gpui_kit::px(100.)),
        },
        viewport: gpui_kit::size(gpui_kit::px(800.), gpui_kit::px(600.)),
    };
    assert_eq!(
        measurement.native_bounds(geometry),
        Some(gpui_kit::Bounds {
            origin: gpui_kit::point(gpui_kit::px(108.), gpui_kit::px(240.)),
            size: gpui_kit::size(gpui_kit::px(160.), gpui_kit::px(32.)),
        })
    );
    let mut clipped = geometry;
    clipped.bounds.origin.y = gpui_kit::px(250.);
    assert!(measurement.native_bounds(clipped).is_none());
    let invalid = Measurement {
        bounds: rect(f32::NAN, 40., 160., 32.),
        ..measurement
    };
    assert!(invalid.native_bounds(geometry).is_none());
}

#[test]
fn clipped_or_invalid_geometry_never_authorizes_a_control() {
    let viewport = rect(10., 20., 200., 96.);
    assert!(visible(rect(10., 20., 200., 32.), viewport));
    assert!(!visible(rect(10., 100., 200., 32.), viewport));
    assert!(!visible(rect(10., 20., 0., 32.), viewport));
    assert!(!visible(rect(f32::NAN, 20., 200., 32.), viewport));
    assert!(!visible(
        rect(10., 20., 200., 32.),
        rect(10., 20., 200., 0.)
    ));
}

#[test]
fn reveal_clamps_and_settles_an_oversized_target_at_viewport_start() {
    let viewport = rect(0., 100., 200., 64.);
    assert_eq!(reveal(rect(0., 70., 200., 128.), viewport, 20.), Some(0.));
    assert_eq!(reveal(rect(0., 100., 200., 128.), viewport, 20.), None);
    assert_eq!(reveal(rect(0., 164., 200., 32.), viewport, 0.), Some(32.));
    assert_eq!(reveal(rect(0., 100., 200., 32.), viewport, 20.), None);
    assert_eq!(reveal(rect(0., 100., 200., 32.), viewport, f32::NAN), None);
}
