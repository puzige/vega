use super::controller::{Geometry, Panes};
use gpui_kit::{Modifiers, MouseButton, MouseMoveEvent, TestAppContext, point, px, size};
use vega_theme::Layout;

fn movement(x: f32, y: f32, pressed_button: Option<MouseButton>) -> MouseMoveEvent {
    MouseMoveEvent {
        position: point(px(x), px(y)),
        pressed_button,
        modifiers: Modifiers::default(),
    }
}

pub(super) fn displayed_extents_preserve_clamps_defaults_and_raw_drag(_: &mut TestAppContext) {
    let mut geometry = Geometry::default();
    let mut reveal = [false; 2];
    let bounds = size(px(1200.), px(900.));
    assert_eq!(
        geometry.dimensions(1000., 900., Layout::BOTTOM_WORKSPACE_HEIGHT, [false; 2]),
        (430., 272.)
    );
    assert_eq!(
        geometry.dimensions(610., 480., 272., [false; 2]),
        (270., 240.)
    );
    assert_eq!(
        geometry.dimensions(400., 300., 272., [false; 2]),
        (270., 150.)
    );

    geometry.begin_drag(false);
    assert!(geometry.move_pointer(
        &movement(1190., 0., Some(MouseButton::Left)),
        bounds,
        &mut reveal
    ));
    geometry.begin_drag(true);
    assert!(geometry.move_pointer(
        &movement(0., 890., Some(MouseButton::Left)),
        bounds,
        &mut reveal
    ));
    assert_eq!(
        geometry.dimensions(1000., 900., 272., [false; 2]),
        (270., 150.)
    );
    assert_eq!(
        geometry.dimensions(1000., 900., 272., [true; 2]),
        (700., 660.)
    );

    geometry.begin_drag(false);
    assert!(geometry.move_pointer(
        &movement(100., 0., Some(MouseButton::Left)),
        bounds,
        &mut reveal
    ));
    geometry.begin_drag(true);
    assert!(geometry.move_pointer(
        &movement(0., 100., Some(MouseButton::Left)),
        bounds,
        &mut reveal
    ));
    assert_eq!(
        geometry.dimensions(1000., 900., 272., [false; 2]),
        (700., 660.)
    );
    assert_eq!(
        geometry.dimensions(1800., 1400., 272., [false; 2]),
        (1100., 800.)
    );
    assert_eq!(
        geometry.dimensions(1000., 900., 272., [true; 2]),
        (700., 660.)
    );
    assert_eq!(
        geometry.dimensions(400., 300., 272., [true; 2]),
        (270., 150.)
    );
    assert_eq!(
        geometry.dimensions(1800., 1400., 272., [false; 2]),
        (1100., 800.)
    );
}

pub(super) fn drag_release_cancel_and_window_resize_preserve_reveal(_: &mut TestAppContext) {
    let mut geometry = Geometry::default();
    let bounds = size(px(1200.), px(900.));
    let mut reveal = [false; 2];
    let held = movement(700., 500., Some(MouseButton::Left));
    assert!(!geometry.move_pointer(&held, bounds, &mut reveal));
    assert_eq!(reveal, [false; 2]);
    geometry.begin_drag(false);
    assert_eq!(reveal, [false; 2]);
    assert!(geometry.move_pointer(&held, bounds, &mut reveal));
    assert_eq!(reveal, [true; 2]);
    assert_eq!(
        geometry.dimensions(1000., 900., 272., [false; 2]),
        (500., 272.)
    );
    reveal = [false; 2];
    geometry.end_drag();
    assert!(!geometry.move_pointer(
        &movement(0., 0., Some(MouseButton::Left)),
        bounds,
        &mut reveal
    ));
    assert_eq!(reveal, [false; 2]);
    assert_eq!(
        geometry.dimensions(1000., 900., 272., [false; 2]),
        (500., 272.)
    );

    geometry.begin_drag(true);
    assert!(geometry.move_pointer(&held, bounds, &mut reveal));
    assert_eq!(reveal, [true; 2]);
    assert_eq!(
        geometry.dimensions(1000., 900., 272., [false; 2]),
        (500., 400.)
    );
    assert_eq!(
        geometry.dimensions(1000., 900., 272., [true, false]),
        (700., 400.)
    );
    assert_eq!(
        geometry.dimensions(1000., 900., 272., [false, true]),
        (500., 660.)
    );
    for button in [None, Some(MouseButton::Right)] {
        geometry.begin_drag(true);
        reveal = [false, true];
        assert!(!geometry.move_pointer(&movement(0., 0., button), bounds, &mut reveal));
        assert!(!geometry.move_pointer(
            &movement(0., 0., Some(MouseButton::Left)),
            bounds,
            &mut reveal
        ));
        assert_eq!(reveal, [false, true]);
        assert_eq!(
            geometry.dimensions(1000., 900., 272., [false; 2]),
            (500., 400.)
        );
    }

    assert!(geometry.window_resized(bounds, &mut reveal));
    assert_eq!(reveal, [true; 2]);
    reveal = [false, true];
    assert!(!geometry.window_resized(bounds, &mut reveal));
    assert_eq!(reveal, [false, true]);
    assert!(geometry.window_resized(size(px(1201.), px(900.)), &mut reveal));
    assert_eq!(reveal, [true; 2]);
    reveal = [false; 2];
    assert!(geometry.window_resized(size(px(1201.), px(901.)), &mut reveal));
    assert_eq!(reveal, [true; 2]);

    geometry.begin_drag(false);
    let mut next_route = geometry.for_route();
    reveal = [false; 2];
    assert!(!next_route.move_pointer(&held, bounds, &mut reveal));
    assert_eq!(
        next_route.dimensions(1000., 900., 272., [false; 2]),
        (500., 400.)
    );
    assert!(next_route.window_resized(size(px(1201.), px(901.)), &mut reveal));
    assert_eq!(reveal, [true; 2]);
}

pub(super) fn visibility_thresholds_and_fullscreen_suppress_dock_separators(
    _: &mut TestAppContext,
) {
    let mut panes = Panes {
        selected: [true; 2],
        hidden: [false; 2],
        maximized: [false; 2],
    };
    assert_eq!(panes.fullscreen(), None);
    for (index, below, at) in [(0, 609.5, 610.), (1, 479.5, 480.)] {
        assert!(!panes.dock_visible(index, below));
        assert!(!panes.separator_visible(index, below));
        assert!(!panes.visible(index, below));
        assert!(panes.dock_visible(index, at));
        assert!(panes.separator_visible(index, at));
        assert!(panes.visible(index, at));
        panes.hidden[index] = true;
        assert!(!panes.visible(index, at));
        assert!(!panes.separator_visible(index, at));
        panes.hidden[index] = false;
        panes.selected[index] = false;
        assert!(!panes.visible(index, at));
        assert!(!panes.separator_visible(index, at));
        panes.selected[index] = true;
    }
    panes.maximized = [true; 2];
    assert_eq!(panes.fullscreen(), Some(0));
    assert!(panes.visible(0, 200.));
    assert!(!panes.visible(1, 900.));
    assert!(!panes.separator_visible(0, 1000.));
    assert!(!panes.separator_visible(1, 900.));
    assert!(panes.dock_visible(0, 1000.));
    panes.hidden[0] = true;
    assert_eq!(panes.fullscreen(), Some(1));
    assert!(!panes.visible(0, 1000.));
    assert!(panes.visible(1, 200.));
    assert!(!panes.separator_visible(1, 900.));
    panes.hidden[0] = false;
    panes.selected[0] = false;
    assert_eq!(panes.fullscreen(), Some(1));
    panes.hidden[1] = true;
    assert_eq!(panes.fullscreen(), None);
    assert!(!panes.visible(0, 1000.));
    assert!(!panes.visible(1, 900.));
    panes.selected = [true; 2];
    panes.hidden = [false; 2];
    panes.maximized = [false; 2];
    assert!(panes.separator_visible(0, 610.));
    assert!(panes.separator_visible(1, 480.));
}
