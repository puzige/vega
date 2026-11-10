use super::*;
use gpui_kit::{point, size};

fn metrics() -> ScrollFadeMetrics {
    ScrollFadeMetrics::new(
        Bounds::new(point(px(10.), px(20.)), size(px(200.), px(120.))),
        point(px(200.), px(200.)),
        point(px(200.), px(200.)),
    )
}

fn color() -> Rgba {
    Rgba {
        r: 0.2,
        g: 0.4,
        b: 0.6,
        a: 0.8,
    }
}

#[test]
fn gradient_keeps_outer_geometry_stops_and_gutter_corner_resets() {
    let style = ScrollFade {
        inset: px(2.),
        radius: px(12.),
        vertical_gutter: px(7.),
        horizontal_gutter: px(5.),
        bottom_extension: px(9.),
        ..ScrollFade::compact()
    };
    let metrics = metrics();
    let bottom = metrics
        .regions(style)
        .into_iter()
        .find(|region| region.angle == 0.)
        .unwrap();
    let gradient = Gradient::new(metrics, style, color(), &bottom);
    assert_eq!(gradient.bounds.origin, point(px(12.), px(22.)));
    assert_eq!(gradient.bounds.size, size(px(189.), px(120.)));
    assert_eq!(gradient.radii.top_left, px(12.));
    assert_eq!(gradient.radii.top_right, px(0.));
    assert_eq!(gradient.radii.bottom_left, px(0.));
    assert_eq!(gradient.radii.bottom_right, px(0.));
    assert_eq!(gradient.start, 9. / 120.);
    assert_eq!(
        gradient.stop,
        (9. + f32::from(bottom.bounds.size.height)) / 120.
    );
    assert_eq!(gradient.color, color());
    assert!(gradient.valid());
}

#[test]
fn pinned_top_changes_side_masks_without_changing_the_prepared_gradient() {
    let metrics = metrics();
    let style = ScrollFade::compact();
    let pinned = ScrollFade {
        top: false,
        ..style
    };
    let normal = metrics.regions(style);
    let pinned_regions = metrics.regions(pinned);
    assert!(!pinned_regions.iter().any(|region| region.angle == 180.));
    for angle in [90., 270.] {
        let before = normal.iter().find(|region| region.angle == angle).unwrap();
        let after = pinned_regions
            .iter()
            .find(|region| region.angle == angle)
            .unwrap();
        assert!(before.bounds.top() > after.bounds.top());
        assert_eq!(after.bounds.top(), metrics.bounds.top());
        assert_eq!(before.bounds.bottom(), after.bounds.bottom());
        assert_eq!(
            Gradient::new(metrics, style, color(), before),
            Gradient::new(metrics, pinned, color(), after),
        );
    }
}

#[test]
fn protected_cursor_removes_only_the_overlapping_edge() {
    let metrics = metrics();
    let style = ScrollFade::compact();
    let protected = metrics.protect(
        Bounds::new(point(px(108.), px(20.)), size(px(2.), px(2.))),
        style,
    );
    let angles: Vec<_> = protected
        .regions(style)
        .into_iter()
        .map(|region| region.angle)
        .collect();
    assert_eq!(angles, vec![0., 90., 270.]);
}

#[test]
fn opacity_multiplies_source_alpha_and_corner_radius_is_clamped() {
    let metrics = metrics();
    let style = ScrollFade {
        radius: px(500.),
        ..ScrollFade::compact()
    };
    let region = FadeRegion {
        bounds: Bounds::new(metrics.bounds.origin, size(px(200.), px(10.))),
        strength: 0.25,
        angle: 180.,
    };
    let gradient = Gradient::new(metrics, style, color(), &region);
    assert_eq!(gradient.color.a, 0.2);
    assert_eq!(gradient.radii, Corners::all(px(60.)));
    assert_eq!(gradient.start, 0.);
    assert_eq!(gradient.stop, 10. / 120.);
}

#[test]
fn current_geometry_changes_invalidate_gradient_identity() {
    let metrics = metrics();
    let style = ScrollFade::compact();
    let region = metrics.regions(style).remove(0);
    let first = Gradient::new(metrics, style, color(), &region);
    assert_eq!(first, Gradient::new(metrics, style, color(), &region));
    let mut moved = metrics;
    moved.bounds.origin.x += px(1.);
    assert_ne!(first, Gradient::new(moved, style, color(), &region));
    let mut resized = metrics;
    resized.bounds.size.width += px(1.);
    assert_ne!(first, Gradient::new(resized, style, color(), &region));
}
