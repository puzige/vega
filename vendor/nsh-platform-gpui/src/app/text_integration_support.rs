use std::{collections::HashSet, sync::Arc};

use nsh_core::{Bounds, Points, TextEngine, TextLine};

use crate::{
    AppView,
    text::{GpuiLayout, GpuiTextEngine},
};

#[doc = "An owned instantaneous observation of current nonselectable native text for development validation."]
#[doc = "This is not a painted-pixel, accessibility or cross-snapshot identity guarantee."]
#[derive(Clone, Debug)]
pub struct IntegrationTextObservation {
    #[doc = "The current native node id, diagnostic only and reusable after removal."]
    pub node: u32,
    #[doc = "The full current DrawnText string, not an event payload or input/editor value."]
    pub text: String,
    #[doc = "Actual backend layout line records in text-content points; line text may be empty."]
    pub lines: Vec<TextLine>,
    #[doc = "The native frame in parent-relative points, without window transforms or clipping."]
    pub frame: Bounds<Points>,
    #[doc = "The layout content bounds in text-view-local points."]
    pub content: Bounds<Points>,
}

fn finite_bounds(bounds: Bounds<Points>) -> bool {
    let values = [
        bounds.origin.x.0,
        bounds.origin.y.0,
        bounds.size.width.0,
        bounds.size.height.0,
        bounds.origin.x.0 + bounds.size.width.0,
        bounds.origin.y.0 + bounds.size.height.0,
    ];
    values.iter().all(|value| value.is_finite())
        && bounds.size.width.0 >= 0.0
        && bounds.size.height.0 >= 0.0
}

fn valid_lines(lines: &[TextLine], text: &str) -> bool {
    if lines.is_empty() {
        return false;
    }
    let Ok(length) = u32::try_from(text.encode_utf16().count()) else {
        return false;
    };
    let mut previous_end = 0;
    lines.iter().all(|line| {
        let values = [
            line.x.0,
            line.y.0,
            line.width.0,
            line.height.0,
            line.ascender.0,
            line.descender.0,
            line.cap_height.0,
            line.x_height.0,
            line.x.0 + line.width.0,
            line.y.0 + line.height.0,
        ];
        let valid = values.iter().all(|value| value.is_finite())
            && line.width.0 >= 0.0
            && line.height.0 > 0.0
            && previous_end <= line.start
            && line.start <= line.end
            && line.end <= length;
        previous_end = line.end;
        valid
    })
}

impl AppView {
    #[doc = "Reads the exact current surface subtree without events, engine commands or mounted-state mutation."]
    #[doc = "Prunes hidden, concealed and nonpositive-opacity subtrees; fails closed for missing control, busy mounting/view state, invalid identity, bounds or layout. An empty result does not establish fixture readiness."]
    #[doc = "Reacquire for every decision. Existing lifecycle APIs expose no retained error flag: the caller must independently reject app errors and wait for its expected projection within its existing deadline."]
    pub fn integration_text_observation(&self) -> Option<Vec<IntegrationTextObservation>> {
        let surface = self.surface();
        let generation = surface.generation.get();
        let _mount = surface.mounter.try_borrow().ok()?;
        let mut running = false;
        surface.control(|_| running = true);
        if !running {
            return None;
        }
        let system = surface.text_system.try_borrow().ok()?.clone()?;
        let engine = GpuiTextEngine::new(system, Arc::from(""));
        let mut pending = vec![(surface.root.clone(), None)];
        let mut visited = HashSet::new();
        let mut observations = Vec::new();
        while let Some((view, parent)) = pending.pop() {
            let state = view.try_state()?;
            if !visited.insert(state.id)
                || surface.view(state.id).as_ref() != Some(&view)
                || !view.attached()
                || view.parent_view() != parent
            {
                return None;
            }
            if state.style.hidden || state.concealed {
                continue;
            }
            let opacity = state.opacity();
            if !opacity.is_finite() || !finite_bounds(state.frame) {
                return None;
            }
            if opacity <= 0.0 {
                continue;
            }
            if let Some(drawn) = state
                .text
                .as_ref()
                .filter(|drawn| !drawn.text.is_selectable())
            {
                drawn.layout.downcast::<GpuiLayout>()?;
                let lines = engine.lines(&drawn.layout);
                if !finite_bounds(drawn.content)
                    || state.frame.size.width.0 <= 0.0
                    || state.frame.size.height.0 <= 0.0
                    || drawn.content.size.width.0 <= 0.0
                    || drawn.content.size.height.0 <= 0.0
                    || !valid_lines(&lines, &drawn.text.text)
                {
                    return None;
                }
                observations.push(IntegrationTextObservation {
                    node: state.id,
                    text: drawn.text.text.clone(),
                    lines,
                    frame: state.frame,
                    content: drawn.content,
                });
            }
            drop(state);
            view.with_children(|children| {
                pending.extend(
                    children
                        .iter()
                        .rev()
                        .map(|child| (child.clone(), Some(view.clone()))),
                );
            });
        }
        (generation == surface.generation.get()).then_some(observations)
    }
}
