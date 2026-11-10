use super::*;
use core::prelude::v1::test;
use gpui_kit::{TestAppContext, VisualTestContext};
use std::cell::Cell;

fn link() -> LinkRange {
    LinkRange {
        range: 0..12,
        target: "https://example.com/path".into(),
        occurrence: 0,
        label: "wrapped link".into(),
        owner: "mounted-reading".into(),
        starts_here: true,
        text_identity: 7,
    }
}

fn rect(x: f32, y: f32, width: f32, height: f32) -> Bounds<Pixels> {
    Bounds::new(point(px(x), px(y)), size(px(width), px(height)))
}

fn register(
    cx: &mut VisualTestContext,
    link: LinkRange,
    rectangles: Vec<Bounds<Pixels>>,
    validity: Option<LinkValidity>,
) -> Entity<Coordinator> {
    let entity = cx.update(acquire);
    cx.draw(
        point(px(0.), px(0.)),
        size(
            AvailableSpace::Definite(px(300.)),
            AvailableSpace::Definite(px(200.)),
        ),
        move |_, _| {
            canvas(
                move |_, window, cx| {
                    prepaint_measured_link_target(
                        &link,
                        rectangles,
                        Vec::new(),
                        validity,
                        window,
                        cx,
                    )
                },
                |_, mut targets, window, cx| {
                    for target in &mut targets {
                        target.paint(window, cx);
                    }
                },
            )
            .size_full()
        },
    );
    entity
}

#[gpui_kit::test]
fn measured_wrapped_link_hits_only_actual_fragments_and_revokes_keyboard(cx: &mut TestAppContext) {
    let visual = cx.add_empty_window();
    let valid = Rc::new(Cell::new(true));
    let current = valid.clone();
    let entity = register(
        visual,
        link(),
        vec![rect(20., 20., 80., 15.), rect(20., 40., 35., 15.)],
        Some(Rc::new(move |_, _| current.get())),
    );
    visual.update(|window, cx| {
        entity.update(cx, |state, cx| {
            state.visible = std::mem::take(&mut state.staged);
            assert_eq!(state.visible.len(), 1);
            let target = &state.visible[0];
            assert_eq!(target.text_bounds.len(), 2);
            assert_eq!(target.role, Role::Link);
            assert_eq!(target.description, "https://example.com/path");
            assert_eq!(
                state.pointer_link(point(px(25.), px(22.)), window, cx),
                Some(target.key.clone())
            );
            assert_eq!(
                state.pointer_link(point(px(25.), px(44.)), window, cx),
                Some(target.key.clone())
            );
            assert!(
                state
                    .pointer_link(point(px(25.), px(37.)), window, cx)
                    .is_none()
            );
            assert!(
                state
                    .pointer_link(point(px(70.), px(44.)), window, cx)
                    .is_none()
            );
            window.focus(&target.focus, cx);
        });
        valid.set(false);
        assert!(
            entity
                .read(cx)
                .pointer_link(point(px(25.), px(22.)), window, cx)
                .is_none()
        );
        keyboard(&entity, "enter", false, window, cx);
        let key = entity.read(cx).visible[0].key.clone();
        arm(&entity, key, window, cx);
        assert!(entity.read(cx).armed.is_none());
    });
    assert!(cx.opened_url().is_none());
}

#[gpui_kit::test]
fn measured_link_duplicate_fragments_require_every_current_lease(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();
    let first = Rc::new(Cell::new(true));
    let second = Rc::new(Cell::new(true));
    let valid = first.clone();
    let entity = register(
        cx,
        link(),
        vec![rect(20., 20., 80., 15.)],
        Some(Rc::new(move |_, _| valid.get())),
    );
    let valid = second.clone();
    register(
        cx,
        link(),
        vec![rect(20., 40., 35., 15.)],
        Some(Rc::new(move |_, _| valid.get())),
    );
    cx.update(|window, cx| {
        entity.update(cx, |state, cx| {
            assert_eq!(state.staged.len(), 1);
            assert_eq!(state.staged[0].text_bounds.len(), 2);
            assert!(state.staged[0].current(window, cx));
            second.set(false);
            assert!(!state.staged[0].current(window, cx));
            second.set(true);
            first.set(false);
            assert!(!state.staged[0].current(window, cx));
        });
    });
}

#[gpui_kit::test]
fn revoked_measured_preview_cancels_retry_without_request_or_fallback(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();
    let valid = Rc::new(Cell::new(true));
    let current = valid.clone();
    let entity = register(
        cx,
        link(),
        vec![rect(20., 20., 80., 15.)],
        Some(Rc::new(move |_, _| current.get())),
    );
    let cancellation = CancellationToken::new();
    cx.update(|window, cx| {
        entity.update(cx, |state, cx| {
            state.visible = std::mem::take(&mut state.staged);
            let target = &state.visible[0];
            state.active = Some(ActivePreview {
                key: target.key.clone(),
                target: target.target.clone(),
                generation: 4,
                cancellation: cancellation.clone(),
                data: None,
                open: cx.focus_handle(),
                retry: cx.focus_handle(),
                address: cx.focus_handle(),
                expanded: false,
                validity: target.validity.clone(),
            });
            assert!(state.active_current(window, cx));
        });
        valid.set(false);
        assert!(build_card(&entity, window, cx).is_none());
        load(&entity, true, window, cx);
        assert!(entity.read(cx).active.is_none());
        assert!(cancellation.is_cancelled());
        assert!(entity.read(cx).armed.is_none());
    });
}

#[gpui_kit::test]
fn measured_link_registration_rejects_nonfinite_empty_and_revoked_rectangles(
    cx: &mut TestAppContext,
) {
    let cx = cx.add_empty_window();
    let entity = register(
        cx,
        link(),
        vec![rect(f32::NAN, 20., 80., 15.), rect(20., 20., 0., 15.)],
        None,
    );
    assert!(cx.update(|_, cx| entity.read(cx).staged.is_empty()));
    register(
        cx,
        link(),
        vec![rect(20., 20., 80., 15.)],
        Some(Rc::new(|_, _| false)),
    );
    assert!(cx.update(|_, cx| entity.read(cx).staged.is_empty()));
}

#[test]
fn measured_card_controls_reject_invalid_or_offscreen_hit_regions() {
    assert_eq!(
        native::measured_control([4., 8., 30., 24.], 200., 160.),
        Some(rect(4., 8., 30., 24.))
    );
    for bounds in [
        [f32::NAN, 8., 30., 24.],
        [4., f32::INFINITY, 30., 24.],
        [-2., 8., 30., 24.],
        [4., -2., 30., 24.],
        [4., 8., 0., 24.],
        [4., 8., 30., -1.],
        [190., 8., 30., 24.],
        [4., 150., 30., 24.],
    ] {
        assert!(native::measured_control(bounds, 200., 160.).is_none());
    }
}

#[gpui_kit::test]
fn preview_authority_rejects_scroll_changes_before_next_prepaint(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();
    let entity = register(cx, link(), vec![rect(20., 20., 80., 15.)], None);
    cx.update(|window, cx| {
        let scroll = ScrollHandle::new();
        entity.update(cx, |state, cx| {
            state.visible = std::mem::take(&mut state.staged);
            let target = &mut state.visible[0];
            target.scopes = vec![ReadingScope {
                id: "reading-root".into(),
                focus: cx.focus_handle(),
                scroll: ScrollSource::Handle(scroll.clone()),
            }];
            target.scroll = target
                .scopes
                .iter()
                .map(|scope| scope.scroll.stamp())
                .collect();
            state.active = Some(ActivePreview {
                key: target.key.clone(),
                target: target.target.clone(),
                generation: 1,
                cancellation: CancellationToken::new(),
                data: Some(Err(LinkPreviewFailure::Network)),
                open: cx.focus_handle(),
                retry: cx.focus_handle(),
                address: cx.focus_handle(),
                expanded: false,
                validity: None,
            });
            assert!(state.active_current(window, cx));
            scroll.set_offset(point(px(0.), px(-30.)));
            assert!(!state.active_current(window, cx));
        });
        load(&entity, true, window, cx);
        assert!(entity.read(cx).active.is_none());
    });
}
