#![allow(missing_docs)]

use std::{
    cell::RefCell,
    collections::HashMap,
    ops::Range,
    rc::{Rc, Weak},
    sync::Arc,
};

use nsh_core::{TextLayout, TextPosition};

use super::{ReadingSnapshot, is_current};
use crate::{geom::Mat4, surface::Surface, text::GpuiLayout, view::NshView};

struct Fence {
    surface: Weak<Surface>,
    token: Rc<()>,
}

thread_local! {
    static FENCES: RefCell<HashMap<u64, Fence>> = RefCell::new(HashMap::new());
}

pub(crate) fn invalidated(surface: &Surface) {
    let Some(owner) = Surface::by_key(surface.key()) else {
        return;
    };
    FENCES.with(|fences| {
        let mut fences = fences.borrow_mut();
        fences.retain(|_, fence| fence.surface.strong_count() != 0);
        fences.insert(
            surface.key(),
            Fence {
                surface: Rc::downgrade(&owner),
                token: Rc::new(()),
            },
        );
    });
}

fn fence(surface: &Rc<Surface>) -> Option<Rc<()>> {
    FENCES.with(|fences| {
        let fences = fences.borrow();
        let fence = fences.get(&surface.key())?;
        Rc::ptr_eq(&fence.surface.upgrade()?, surface).then(|| fence.token.clone())
    })
}

#[derive(Clone)]
pub struct ReadingGeometry {
    surface: Weak<Surface>,
    snapshot: ReadingSnapshot,
    fence: Rc<()>,
    stamp: SurfaceStamp,
    texts: Vec<TextGeometry>,
}

#[derive(Clone, PartialEq)]
struct SurfaceStamp {
    origin: (f32, f32),
    scale: f32,
    root: NshView,
    frame: [f32; 4],
}

#[derive(Clone, PartialEq)]
struct MountStamp {
    view: NshView,
    frame: [f32; 4],
    shift: (f32, f32),
    transform: Mat4,
    scroll: Option<[f32; 7]>,
    clip: bool,
    opacity: f32,
}

#[derive(Clone)]
struct TextGeometry {
    layout: TextLayout,
    content: [f32; 4],
    chain: Vec<MountStamp>,
    world: Mat4,
    clip: [f32; 4],
}

fn rect(bounds: nsh_core::Bounds<nsh_core::Points>) -> [f32; 4] {
    [
        bounds.origin.x.0,
        bounds.origin.y.0,
        bounds.size.width.0,
        bounds.size.height.0,
    ]
}

fn finite_rect(rect: [f32; 4]) -> bool {
    rect.iter().all(|v| v.is_finite())
        && rect[2] >= 0.0
        && rect[3] >= 0.0
        && (rect[0] + rect[2]).is_finite()
        && (rect[1] + rect[3]).is_finite()
}

fn supported(matrix: Mat4) -> bool {
    let mut expected = Mat4::IDENTITY;
    expected.m[0][0] = matrix.m[0][0];
    expected.m[1][1] = matrix.m[0][0];
    expected.m[3][0] = matrix.m[3][0];
    expected.m[3][1] = matrix.m[3][1];
    matrix == expected && matrix.m.iter().flatten().all(|v| v.is_finite()) && matrix.m[0][0] > 0.0
}

fn mapped(matrix: Mat4, rect: [f32; 4]) -> Option<[f32; 4]> {
    if !finite_rect(rect) || !supported(matrix) {
        return None;
    }
    let (x, y) = matrix.map(rect[0], rect[1])?;
    let output = [x, y, rect[2] * matrix.m[0][0], rect[3] * matrix.m[1][1]];
    finite_rect(output).then_some(output)
}

fn intersect(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let x = a[0].max(b[0]);
    let y = a[1].max(b[1]);
    [
        x,
        y,
        ((a[0] + a[2]).min(b[0] + b[2]) - x).max(0.0),
        ((a[1] + a[3]).min(b[1] + b[3]) - y).max(0.0),
    ]
}

fn surface_stamp(surface: &Surface) -> Option<SurfaceStamp> {
    let origin = surface.origin.get();
    let scale = surface.scale();
    let frame = rect(surface.root.state().frame);
    (origin.0.is_finite()
        && origin.1.is_finite()
        && scale.is_finite()
        && scale > 0.0
        && finite_rect(frame)
        && !surface.window_hidden.get())
    .then(|| SurfaceStamp {
        origin,
        scale,
        root: surface.root.clone(),
        frame,
    })
}

fn mount_stamp(surface: &Surface, view: &NshView) -> Option<MountStamp> {
    if surface.view(view.id()).as_ref() != Some(view) || !view.attached() {
        return None;
    }
    let state = view.state();
    let opacity = state.opacity();
    let frame = rect(state.frame);
    let transform = state.transform().unwrap_or_default();
    let clip = state.clips();
    if state.style.hidden
        || state.concealed
        || !opacity.is_finite()
        || opacity <= 0.0
        || !finite_rect(frame)
        || !state.shift.0.is_finite()
        || !state.shift.1.is_finite()
        || !supported(transform)
        || (clip && state.radii().iter().any(|&r| !r.is_finite() || r != 0.0))
    {
        return None;
    }
    let scroll = state.scroll.as_ref().map(|scroll| {
        let origin = scroll.origin();
        [
            origin.x.0,
            origin.y.0,
            scroll.x.0,
            scroll.y.0,
            scroll.content.width.0,
            scroll.content.height.0,
            if scroll.mirrored { 1.0 } else { 0.0 },
        ]
    });
    if scroll.is_some_and(|values| values.iter().any(|v| !v.is_finite())) {
        return None;
    }
    Some(MountStamp {
        view: view.clone(),
        frame,
        shift: state.shift,
        transform,
        scroll,
        clip,
        opacity,
    })
}

fn text_geometry(surface: &Surface, text: &super::ReadingText) -> Option<TextGeometry> {
    let (layout, content) = {
        let state = text.view.state();
        let drawn = state.text.as_ref()?;
        if state.id != text.node
            || !Arc::ptr_eq(&drawn.text, &text.content)
            || drawn.layout.downcast::<GpuiLayout>().is_none()
        {
            return None;
        }
        (drawn.layout.clone(), rect(drawn.content))
    };
    if !finite_rect(content) {
        return None;
    }
    let mut chain = Vec::new();
    let mut node = text.view.clone();
    loop {
        chain.push(mount_stamp(surface, &node)?);
        if node == surface.root {
            break;
        }
        node = node.parent_view()?;
    }
    let root_frame = rect(surface.root.state().frame);
    let mut clip = [0.0, 0.0, root_frame[2], root_frame[3]];
    let mut world = Mat4::IDENTITY;
    for index in (0..chain.len()).rev() {
        let stamp = &chain[index];
        let origin = if let Some(parent) = chain.get(index + 1) {
            let state = stamp.view.state();
            parent.view.state().child_origin(state.frame, state.shift)
        } else {
            (0.0, 0.0)
        };
        let local = Mat4::mul(&stamp.transform, &Mat4::new().translate(origin.0, origin.1));
        world = Mat4::mul(&local, &world);
        if !supported(world) {
            return None;
        }
        if stamp.clip || index == 0 {
            clip = intersect(
                clip,
                mapped(world, [0.0, 0.0, stamp.frame[2], stamp.frame[3]])?,
            );
        }
    }
    let world = Mat4::mul(&Mat4::new().translate(content[0], content[1]), &world);
    Some(TextGeometry {
        layout,
        content,
        chain,
        world,
        clip,
    })
}

impl ReadingGeometry {
    pub(crate) fn capture(surface: &Rc<Surface>, snapshot: &ReadingSnapshot) -> Option<Self> {
        #[cfg(feature = "vega-host-regressions")]
        let _diagnostic = crate::rt::diagnostics::Span::start(
            crate::rt::diagnostics::Stage::GeometryCapture,
            snapshot.texts.len(),
        );
        if !is_current(surface, snapshot) || surface.invalidated() {
            return None;
        }
        let token = fence(surface)?;
        let stamp = surface_stamp(surface)?;
        let texts = snapshot
            .texts
            .iter()
            .map(|text| text_geometry(surface, text))
            .collect::<Option<Vec<_>>>()?;
        let result = Self {
            surface: Rc::downgrade(surface),
            snapshot: snapshot.clone(),
            fence: token,
            stamp,
            texts,
        };
        result.current_for(surface).then_some(result)
    }

    pub(crate) fn current_for(&self, surface: &Rc<Surface>) -> bool {
        #[cfg(feature = "vega-host-regressions")]
        let _diagnostic = crate::rt::diagnostics::Span::start(
            crate::rt::diagnostics::Stage::GeometryCurrent,
            self.snapshot.texts.len(),
        );
        self.surface
            .upgrade()
            .is_some_and(|owner| Rc::ptr_eq(&owner, surface))
            && !surface.invalidated()
            && fence(surface).is_some_and(|token| Rc::ptr_eq(&token, &self.fence))
            && surface_stamp(surface).as_ref() == Some(&self.stamp)
            && is_current(surface, &self.snapshot)
            && self
                .snapshot
                .texts
                .iter()
                .zip(&self.texts)
                .all(|(text, old)| {
                    text_geometry(surface, text).is_some_and(|now| {
                        Arc::ptr_eq(&now.layout.0, &old.layout.0)
                            && now.content == old.content
                            && now.chain == old.chain
                            && now.world == old.world
                            && now.clip == old.clip
                    })
                })
    }

    pub(crate) fn scroll_anchor(
        &self,
        surface: &Rc<Surface>,
        scroll_node: u32,
        delta_y: f32,
    ) -> bool {
        if !delta_y.is_finite()
            || delta_y == 0.0
            || !self.current_for(surface)
            || surface.input.reading_anchor_blocked()
            || surface.input_pressed.get() != 0
            || surface.window_hidden.get()
            || self.texts.is_empty()
        {
            return false;
        }
        let Some(view) = surface.view(scroll_node) else {
            return false;
        };
        if !view.attached() || view.state().scroll.is_none() {
            return false;
        }
        for text in &self.texts {
            let Some(scroll_index) = text.chain.iter().position(|stamp| stamp.view == view) else {
                return false;
            };
            let Some(area_index) = text
                .chain
                .iter()
                .position(|stamp| stamp.view == self.snapshot.area_view)
            else {
                return false;
            };
            if scroll_index < area_index
                || text.chain.iter().any(|stamp| {
                    stamp.transform.m[0][0] != 1.0
                        || (stamp.scroll.is_some()
                            && crate::scroll::reading_anchor_busy(surface, &stamp.view))
                })
            {
                return false;
            }
        }
        crate::scroll::reading_anchor_by(surface, &view, delta_y)
    }

    #[cfg(feature = "vega-host-regressions")]
    pub(crate) fn matches_snapshot(&self, snapshot: &ReadingSnapshot) -> bool {
        self.surface
            .upgrade()
            .is_some_and(|surface| self.current_for(&surface))
            && self.snapshot.matches(snapshot)
    }

    fn text(&self, index: usize) -> Option<(&TextGeometry, &GpuiLayout)> {
        let surface = self.surface.upgrade()?;
        if !self.current_for(&surface) {
            return None;
        }
        let text = self.texts.get(index)?;
        Some((text, text.layout.downcast::<GpuiLayout>()?))
    }

    fn valid_offset(&self, index: usize, offset: u32) -> bool {
        self.snapshot.texts.get(index).is_some_and(|text| {
            let mut current = 0u32;
            if offset == 0 {
                return true;
            }
            for scalar in text.content.text.chars() {
                current += scalar.len_utf16() as u32;
                if current >= offset {
                    return current == offset;
                }
            }
            false
        })
    }

    pub fn range_rects(&self, text_index: usize, range: Range<u32>) -> Option<Vec<[f32; 4]>> {
        let (text, layout) = self.text(text_index)?;
        if range.start > range.end
            || !self.valid_offset(text_index, range.start)
            || !self.valid_offset(text_index, range.end)
        {
            return None;
        }
        let mut output = Vec::new();
        let mut valid = true;
        layout.selection_rects((range.start, range.end), |x, y, w, h| {
            if let Some(rect) = mapped(text.world, [x, y, w, h]) {
                let clipped = intersect(rect, text.clip);
                if clipped[2] > 0.0 && clipped[3] > 0.0 {
                    output.push(clipped)
                }
            } else {
                valid = false
            }
        });
        valid.then_some(output)
    }

    pub fn caret_rect(&self, text_index: usize, position: TextPosition) -> Option<[f32; 4]> {
        let (text, layout) = self.text(text_index)?;
        if !self.valid_offset(text_index, position.offset) {
            return None;
        }
        mapped(text.world, rect(layout.caret_rect(position)?))
    }

    pub fn hit_test(&self, text_index: usize, point: [f32; 2]) -> Option<TextPosition> {
        let (text, layout) = self.text(text_index)?;
        let [x, y] = point;
        let [cx, cy, w, h] = text.clip;
        if !x.is_finite()
            || !y.is_finite()
            || w <= 0.0
            || h <= 0.0
            || x < cx
            || y < cy
            || x >= cx + w
            || y >= cy + h
        {
            return None;
        }
        let (x, y) = text.world.unproject(x, y)?;
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        let position = layout.hit_test(x, y);
        self.valid_offset(text_index, position.offset)
            .then_some(position)
    }
}
