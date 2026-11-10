use gpui::{Bounds, Pixels, Point, point, px, size};
use nsh_core::{Command, Points};

use crate::{
    AppView,
    geom::Mat4,
    reading::{ReadingGeometry, ReadingSnapshot},
    surface::Surface,
    view::NshView,
};

#[doc = "Actual mounted reading scroll state for development-only native integration cases."]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IntegrationScrollObservation {
    #[doc = "The movable native scroll node, or None for a verified no-target viewport."]
    pub node: Option<u32>,
    #[doc = "Visible target bounds intersected with the reading area, in window logical pixels."]
    pub viewport: Bounds<Pixels>,
    #[doc = "Actual native content offsets; unit content points equal logical pixels."]
    pub offset: Point<Pixels>,
    #[doc = "Actual inset-inclusive native maximum offsets, without inferred wheel consumption."]
    pub maximum: Point<Pixels>,
}

struct NativeScrollState {
    horizontal: bool,
    offset: Point<Pixels>,
    maximum: Point<Pixels>,
    inset: nsh_core::Point<Points>,
}

fn native_scroll_state(view: &NshView) -> Option<NativeScrollState> {
    let state = view.try_state()?;
    let scroll = state.scroll.as_ref()?;
    let inset = scroll.behavior.content_inset;
    let values = [
        state.frame.size.width.0,
        state.frame.size.height.0,
        scroll.content.width.0,
        scroll.content.height.0,
        scroll.x.0,
        scroll.y.0,
        inset.left.0,
        inset.right.0,
        inset.top.0,
        inset.bottom.0,
        scroll.content.width.0 + inset.left.0 + inset.right.0 - state.frame.size.width.0,
        scroll.content.height.0 + inset.top.0 + inset.bottom.0 - state.frame.size.height.0,
    ];
    if values.iter().any(|value| !value.is_finite())
        || state.frame.size.width.0 <= 0.0
        || state.frame.size.height.0 <= 0.0
        || scroll.content.width.0 < 0.0
        || scroll.content.height.0 < 0.0
        || scroll.behavior.min_zoom != 1.0
        || scroll.behavior.max_zoom != 1.0
        || scroll.live.refreshing
    {
        return None;
    }
    let maximum = scroll.max(state.frame.size);
    if !(0.0..=maximum.x.0).contains(&scroll.x.0) || !(0.0..=maximum.y.0).contains(&scroll.y.0) {
        return None;
    }
    Some(NativeScrollState {
        horizontal: scroll.horizontal,
        offset: point(px(scroll.x.0), px(scroll.y.0)),
        maximum: point(px(maximum.x.0), px(maximum.y.0)),
        inset: nsh_core::geometry::point(inset.left, inset.top),
    })
}

fn mounted_unit_chain(surface: &Surface, view: &NshView) -> Option<()> {
    let mut current = view.clone();
    loop {
        if !current.attached() || surface.view(current.id()).as_ref() != Some(&current) {
            return None;
        }
        let state = current.try_state()?;
        let transform = state.transform().unwrap_or_default();
        let mut translation = Mat4::IDENTITY;
        translation.m[3][0] = transform.m[3][0];
        translation.m[3][1] = transform.m[3][1];
        let frame = state.frame;
        let opacity = state.opacity();
        let values = [
            frame.origin.x.0,
            frame.origin.y.0,
            frame.size.width.0,
            frame.size.height.0,
            frame.origin.x.0 + frame.size.width.0,
            frame.origin.y.0 + frame.size.height.0,
            state.shift.0,
            state.shift.1,
            opacity,
        ];
        if values.iter().any(|value| !value.is_finite())
            || frame.size.width.0 <= 0.0
            || frame.size.height.0 <= 0.0
            || state.style.hidden
            || state.concealed
            || opacity <= 0.0
            || transform != translation
            || transform.m.iter().flatten().any(|value| !value.is_finite())
            || (state.clips() && state.radii().iter().any(|radius| *radius != 0.0))
        {
            return None;
        }
        if state.scroll.is_some() {
            native_scroll_state(&current)?;
        }
        drop(state);
        if current == surface.root {
            return Some(());
        }
        current = current.parent_view()?;
    }
}

fn mounted_viewport(surface: &Surface, view: &NshView) -> Option<Bounds<Pixels>> {
    mounted_unit_chain(surface, view)?;
    let matrix = view.to_root()?;
    let mut translation = Mat4::IDENTITY;
    translation.m[3][0] = matrix.m[3][0];
    translation.m[3][1] = matrix.m[3][1];
    if matrix != translation || matrix.m.iter().flatten().any(|value| !value.is_finite()) {
        return None;
    }
    let visible = view.visible_bounds();
    let (x, y) = matrix.map(visible.origin.x.0, visible.origin.y.0)?;
    let origin = surface.origin.get();
    let x = x + origin.0;
    let y = y + origin.1;
    let width = visible.size.width.0;
    let height = visible.size.height.0;
    let values = [x, y, width, height, x + width, y + height];
    if values.iter().any(|value| !value.is_finite()) || width <= 0.0 || height <= 0.0 {
        return None;
    }
    let root_size = surface.root.try_state()?.frame.size;
    let viewport =
        Bounds::new(point(px(x), px(y)), size(px(width), px(height))).intersect(&Bounds::new(
            point(px(origin.0), px(origin.1)),
            size(px(root_size.width.0), px(root_size.height.0)),
        ));
    (!viewport.is_empty()).then_some(viewport)
}

fn ancestor_of(ancestor: &NshView, view: &NshView) -> bool {
    let mut current = Some(view.clone());
    while let Some(view) = current {
        if view == *ancestor {
            return true;
        }
        current = view.parent_view();
    }
    false
}

fn vertical_target(
    surface: &Surface,
    area: &NshView,
    requested: Option<u32>,
) -> Option<Option<NshView>> {
    let target = if let Some(node) = requested {
        let view = surface.view(node)?;
        if !ancestor_of(&view, area) || native_scroll_state(&view)?.horizontal {
            return None;
        }
        Some(view)
    } else {
        None
    };
    let mut current = Some(area.clone());
    while let Some(view) = current {
        let state = view.try_state()?;
        if state.scroll.is_some() {
            let scroll = native_scroll_state(&view)?;
            if !scroll.horizontal && scroll.maximum.y > px(0.0) && Some(view.id()) != requested {
                return None;
            }
        }
        drop(state);
        current = view.parent_view();
    }
    Some(target)
}

fn horizontal_target(
    surface: &Surface,
    area: &NshView,
    requested: Option<u32>,
) -> Option<Option<NshView>> {
    let mut pending = area.child_views();
    let mut target = None;
    while let Some(view) = pending.pop() {
        if !view.attached() || surface.view(view.id()).as_ref() != Some(&view) {
            return None;
        }
        let state = view.try_state()?;
        if state.scroll.is_some() {
            let scroll = native_scroll_state(&view)?;
            if scroll.horizontal && scroll.maximum.x > px(0.0) {
                if target.is_some() {
                    return None;
                }
                target = Some(view.clone());
            }
        }
        drop(state);
        pending.extend(view.child_views());
    }
    if requested.is_some() && target.as_ref().map(NshView::id) != requested {
        return None;
    }
    Some(target)
}

impl AppView {
    fn integration_scroll_snapshot(
        &self,
        expected: &ReadingGeometry,
        area: u32,
    ) -> Option<ReadingSnapshot> {
        if !self.reading_geometry_is_current(expected) {
            return None;
        }
        let snapshot = self.reading_snapshot(area)?;
        expected.matches_snapshot(&snapshot).then_some(snapshot)
    }

    #[doc = "Observes the real mounted native scroll target for a fresh exact reading geometry lease."]
    #[doc = "Vertical requests name the exact scroll ancestor; horizontal requests discover the unique movable horizontal descendant, optionally requiring its node. No movable target returns a real viewport with None and zero offset/maximum; invalid or ambiguous requests return None."]
    #[doc = "The viewport is clipped to the reading area in window logical pixels. Actual offsets and inset-inclusive maxima use native content points, numerically equal to logical pixels only under the supported translation-only geometry. This method never changes scroll state."]
    pub fn integration_reading_scroll(
        &self,
        expected: &ReadingGeometry,
        area: u32,
        node: Option<u32>,
        horizontal: bool,
    ) -> Option<IntegrationScrollObservation> {
        let snapshot = self.integration_scroll_snapshot(expected, area)?;
        let surface = self.surface();
        let area_view = &snapshot.area_view;
        let area_viewport = mounted_viewport(surface, area_view)?;
        let target = if horizontal {
            horizontal_target(surface, area_view, node)?
        } else {
            vertical_target(surface, area_view, node)?
        };
        let mut observation = IntegrationScrollObservation {
            node: None,
            viewport: area_viewport,
            offset: point(px(0.0), px(0.0)),
            maximum: point(px(0.0), px(0.0)),
        };
        if let Some(target) = target {
            let scroll = native_scroll_state(&target)?;
            let viewport = mounted_viewport(surface, &target)?.intersect(&area_viewport);
            if viewport.is_empty() {
                return None;
            }
            observation.viewport = viewport;
            let movable = if horizontal {
                scroll.maximum.x > px(0.0)
            } else {
                scroll.maximum.y > px(0.0)
            };
            if movable {
                observation.node = Some(target.id());
                observation.offset = scroll.offset;
                observation.maximum = scroll.maximum;
            } else if scroll.maximum != point(px(0.0), px(0.0))
                || scroll.offset != point(px(0.0), px(0.0))
            {
                return None;
            }
        }
        Some(observation)
    }

    #[doc = "Positions a fresh exact reading scroll target synchronously for development integration setup."]
    #[doc = "Desired positive native offsets must be finite and in range, with the off-axis unchanged. The existing nonanimated native command interrupts target motion, applies and reports the actual main-axis position, and revokes the geometry even for an equal-position setup. Reacquire geometry before observing again."]
    #[doc = "A verified no-target viewport accepts only a zero position without issuing a command. Returns false for stale, foreign, detached, unsupported, ambiguous, out-of-range or off-axis-changing requests; never synthesizes wheel input."]
    pub fn integration_position_reading_scroll(
        &self,
        expected: &ReadingGeometry,
        area: u32,
        node: Option<u32>,
        desired: Point<Pixels>,
    ) -> bool {
        let desired_x = f32::from(desired.x);
        let desired_y = f32::from(desired.y);
        if !desired_x.is_finite() || !desired_y.is_finite() {
            return false;
        }
        let Some(node) = node else {
            return desired == point(px(0.0), px(0.0))
                && self
                    .integration_reading_scroll(expected, area, None, false)
                    .is_some_and(|observation| observation.node.is_none());
        };
        let surface = self.surface();
        let Some(view) = surface.view(node) else {
            return false;
        };
        let Some(scroll) = native_scroll_state(&view) else {
            return false;
        };
        let Some(observation) =
            self.integration_reading_scroll(expected, area, Some(node), scroll.horizontal)
        else {
            return false;
        };
        if observation.node != Some(node)
            || desired.x < px(0.0)
            || desired.y < px(0.0)
            || desired.x > observation.maximum.x
            || desired.y > observation.maximum.y
            || (scroll.horizontal && desired.y != observation.offset.y)
            || (!scroll.horizontal && desired.x != observation.offset.x)
        {
            return false;
        }
        let x = desired_x - scroll.inset.x.0;
        let y = desired_y - scroll.inset.y.0;
        if !x.is_finite()
            || !y.is_finite()
            || (scroll.horizontal && x + scroll.inset.x.0 != desired_x)
            || (!scroll.horizontal && y + scroll.inset.y.0 != desired_y)
        {
            return false;
        }
        crate::scroll::command(
            surface,
            &view,
            &Command::ScrollTo {
                x: Points(x),
                y: Points(y),
                animated: false,
            },
        );
        native_scroll_state(&view).is_some_and(|state| state.offset == desired)
    }
}
