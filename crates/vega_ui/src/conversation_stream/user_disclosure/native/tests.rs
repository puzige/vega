use super::*;
use ::core::prelude::v1::test;

fn geometry() -> Geometry {
    Geometry {
        bounds: Bounds::new(point(px(20.), px(30.)), size(px(160.), px(24.))),
        clip: Bounds::new(point(px(0.), px(0.)), size(px(320.), px(200.))),
        list: Bounds::new(point(px(0.), px(0.)), size(px(320.), px(200.))),
        viewport: size(px(320.), px(200.)),
        foreground: px(0.),
        scroll: (0, px(0.)),
        scale: 1.,
        window: WindowId::from(0),
    }
}

#[test]
fn measured_disclosure_requires_current_root_size_and_fully_visible_control() {
    let witness = Witness {
        revision: 1,
        control: [0., 0., 100., 24.],
        root: [0., 0., 160., 24.],
    };
    let current = geometry();
    assert!(measured_visible(&witness, current));
    let mut clipped = current;
    clipped.clip.origin.x = px(21.);
    assert!(!measured_visible(&witness, clipped));
    let mut resized = current;
    resized.bounds.size.width = px(180.);
    assert!(!measured_visible(&witness, resized));
    assert!(!measured_visible(
        &Witness {
            root: [2., 0., 160., 24.],
            ..witness
        },
        current
    ));
    assert!(!measured_visible(
        &Witness {
            control: [0., 0., f32::NAN, 24.],
            ..witness
        },
        current
    ));
    assert!(!measured_visible(
        &Witness {
            control: [0., 0., 0., 24.],
            ..witness
        },
        current
    ));
}
