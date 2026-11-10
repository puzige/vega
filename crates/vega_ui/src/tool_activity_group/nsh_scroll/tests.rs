use super::{ChromeSnapshot as Snapshot, ThumbGeometry, render_chrome as render};
use nsh_core::prelude as ns;
use nsh_core::{ApproxTextEngine, Core, MountItem};
use tokio::sync::mpsc;
use vega_theme::{Layout, Theme};

fn snapshot(width: f32, height: f32, before: f32, after: f32) -> Snapshot {
    Snapshot {
        width,
        height,
        before,
        after,
        color: Theme::light().colors.bg_base,
        thumb: Theme::light().colors.text_primary,
        thumb_start: 0.,
        thumb_length: 0.,
        thumb_width: 6.,
        visible: false,
    }
}

#[test]
fn fades_only_show_actual_hidden_content_and_respect_short_viewports() {
    let start = snapshot(320., 240., 0., 100.);
    assert_eq!(start.strength(start.before), 0.);
    assert_eq!(start.strength(start.after), 1.);
    let end = snapshot(320., 240., 100., 0.);
    assert_eq!(end.strength(end.before), 1.);
    assert_eq!(end.strength(end.after), 0.);
    let short = snapshot(110., 20., 100., 100.);
    assert_eq!(short.fade_length(), 5.);
    let empty = snapshot(0., 0., 0., 0.);
    assert_eq!(empty.fade_length(), 0.);
    assert_eq!(empty.strength(100.), 0.);
    let epsilon = snapshot(320., 240., Layout::SCROLL_EDGE_FADE_EPSILON, 0.);
    assert_eq!(epsilon.strength(epsilon.before), 0.);
}

#[test]
fn thumb_drag_and_track_jump_map_actual_extent_without_a_second_scroll_state() {
    let geometry = ThumbGeometry::new(240., 720.);
    assert_eq!(geometry.length, 52.);
    assert_eq!(geometry.start(0.), 4.);
    assert_eq!(geometry.start(720.), 184.);
    assert_eq!(geometry.offset(geometry.start(0.) + 13., 13.), 0.);
    assert_eq!(geometry.offset(geometry.start(720.) + 13., 13.), -720.);
    assert_eq!(geometry.offset(-100., 13.), 0.);
    assert_eq!(geometry.offset(10000., 13.), -720.);
    let middle = geometry.offset(120., geometry.length / 2.);
    assert_eq!(middle, -360.);
    let short = ThumbGeometry::new(20., 100.);
    assert_eq!(short.travel, 0.);
    assert_eq!(short.offset(100., 0.), 0.);
    let empty = ThumbGeometry::new(0., 0.);
    assert_eq!(empty.length, 0.);
    assert_eq!(empty.offset(1., 0.), 0.);
}

#[ns::component]
fn FadeFixture() -> impl ns::IntoElement {
    let (commands, _) = mpsc::channel(crate::nsh::COMMAND_CAPACITY);
    render(ns::signal(snapshot(110., 20., 100., 100.)), commands)
}

#[test]
fn production_fade_geometry_keeps_scrollbar_gutter_and_no_input_actions() {
    let mut core = Core::new(ApproxTextEngine, ns::size(ns::px(110.), ns::px(20.)));
    core.mount_root(&nsh_core::element::Root::of::<FadeFixture>());
    let batch = core.commit().expect("production scroll fade mount");
    let edges = batch
        .items
        .iter()
        .filter_map(|item| match item {
            MountItem::Frame { frame, .. } if (frame.size.height.0 - 5.).abs() < 0.01 => {
                Some(frame)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(edges.len(), 2);
    assert!(edges.iter().all(|frame| {
        (frame.size.width.0 - (110. - Layout::DISCLOSURE_SCROLLBAR_GUTTER)).abs() < 0.01
    }));
    assert!(edges.iter().any(|frame| frame.origin.y.0.abs() < 0.01));
    assert!(
        edges
            .iter()
            .any(|frame| (frame.origin.y.0 - 15.).abs() < 0.01)
    );
    for item in &batch.items {
        if let MountItem::Events { events, .. } = item {
            assert_eq!(
                events.bits(),
                0,
                "scroll fade must not consume host gestures"
            );
        }
    }
}
