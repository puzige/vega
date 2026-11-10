use super::{Mount, Target};
use std::cell::RefCell;
use vega_theme::Theme;

struct MountedHeaderFixture {
    group: gpui_kit::Entity<super::RunActivityGroup>,
    neighbor: gpui_kit::Entity<super::RunActivityGroup>,
}

impl gpui_kit::Render for MountedHeaderFixture {
    fn render(
        &mut self,
        window: &mut gpui_kit::Window,
        cx: &mut gpui_kit::Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        use gpui_kit::{ParentElement as _, Styled as _, div, px};
        div()
            .w(px(640.))
            .flex()
            .flex_col()
            .child(super::render_header(
                self.group.clone(),
                Target::Run,
                false,
                window,
                cx,
            ))
            .child(super::render_header(
                self.group.clone(),
                Target::Stage(0),
                false,
                window,
                cx,
            ))
            .child(super::render_header(
                self.neighbor.clone(),
                Target::Run,
                false,
                window,
                cx,
            ))
    }
}

pub fn repeated_production_header_render_retains_unowned_mount_and_projection(
    cx: &mut gpui_kit::TestAppContext,
) {
    use gpui_kit::{AppContext as _, point, px, size};
    cx.update(|cx| cx.set_global(Theme::light()));
    let group = cx.new(|cx| {
        super::RunActivityGroup::new(super::RunActivityStatus::Completed, None, false, cx)
    });
    group.update(cx, |group, cx| {
        group.start_segment(cx);
    });
    let neighbor = cx.new(|cx| {
        super::RunActivityGroup::new(super::RunActivityStatus::Completed, None, false, cx)
    });
    let source = group.clone();
    let adjacent = neighbor.clone();
    let (_, visual) = cx.add_window_view(move |_, _| MountedHeaderFixture {
        group: source,
        neighbor: adjacent,
    });
    let mut stable = None;
    let mut adjacent_stable = None;
    let mut stage_stable = None;
    for _ in 0..8 {
        visual.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        visual.update(|_, cx| {
            let mut headers = group.read(cx).headers.borrow_mut();
            let page = headers
                .run
                .as_mut()
                .expect("production header presentation");
            let current = page.publication.published.clone();
            assert!(current.owner.is_none());
            assert!(current.layout.is_some());
            assert!(!page.publication.accepts(&current));
            assert!(page.divider.is_some());
            assert!(
                !page.publish(current.clone()),
                "neither header nor divider may republish stable render"
            );
            let mount = page.mount.upgrade().expect("GPUI retains mounted state");
            if let Some((previous, previous_mount)) = &stable {
                assert!(current.same_projection(previous));
                assert!(std::rc::Weak::ptr_eq(
                    &std::rc::Rc::downgrade(&mount),
                    previous_mount
                ));
            }
            stable = Some((current, std::rc::Rc::downgrade(&mount)));
            let stage_id = group.read(cx).segments[0].title_view.entity_id();
            let page = headers.stages.get_mut(&stage_id).expect("co-mounted stage");
            let stage = page.publication.published.clone();
            let stage_mount = page.mount.upgrade().expect("stage retains own mount");
            assert!(!std::rc::Rc::ptr_eq(&mount, &stage_mount));
            assert!(stage.layout.is_some());
            assert!(!page.publish(stage.clone()));
            if let Some(previous) = &stage_stable {
                assert!(stage.same_projection(previous));
            }
            stage_stable = Some(stage);
            drop(headers);
            let mut headers = neighbor.read(cx).headers.borrow_mut();
            let page = headers.run.as_mut().expect("co-mounted neighboring run");
            let adjacent = page.publication.published.clone();
            let adjacent_mount = page.mount.upgrade().expect("neighbor retains own mount");
            assert!(!std::rc::Rc::ptr_eq(&mount, &adjacent_mount));
            assert!(adjacent.layout.is_some());
            assert!(!page.publish(adjacent.clone()));
            if let Some(previous) = &adjacent_stable {
                assert!(adjacent.same_projection(previous));
            }
            adjacent_stable = Some(adjacent);
        });
    }
    visual.update(|window, cx| {
        let mut headers = group.read(cx).headers.borrow_mut();
        let page = headers.run.as_mut().unwrap();
        let mut changed = page.publication.published.clone();
        changed.label.push_str(" updated");
        assert!(page.publish(changed));
        assert!(!page.publish(page.publication.published.clone()));
        let old_id = page.id.clone();
        let lease = page.mount();
        *lease.borrow_mut() = Mount::default();
        assert!(std::rc::Rc::ptr_eq(&lease, &page.mount()));
        assert!(
            page.id.matches(&old_id),
            "live prepaint-pending lease is not a remount"
        );
        page.revoke();
        assert!(!page.id.matches(&old_id));
        let revoked_id = page.id.clone();
        lease.borrow_mut().update(
            gpui_kit::Bounds::new(point(px(0.), px(0.)), size(px(640.), px(28.))),
            gpui_kit::Bounds::new(point(px(0.), px(0.)), size(px(640.), px(28.))),
            window.viewport_size(),
            window.scale_factor(),
        );
        assert!(std::rc::Rc::ptr_eq(&lease, &page.mount()));
        assert!(page.id.matches(&revoked_id));
        assert!(!page.id.matches(&old_id));
        let transient = std::rc::Rc::new(RefCell::new(Mount::default()));
        page.mount = std::rc::Rc::downgrade(&transient);
        drop(transient);
        let replacement = page.mount();
        assert!(
            !page.id.matches(&revoked_id),
            "dropped lease rotates identity"
        );
        assert!(!std::rc::Rc::ptr_eq(&replacement, &lease));
        assert!(replacement.borrow().layout().is_none());
        assert!(page.publication.displayed.is_none());
    });
}
