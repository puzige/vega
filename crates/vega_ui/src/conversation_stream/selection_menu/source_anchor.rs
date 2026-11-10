#[cfg(test)]
use super::utf16_byte;
use super::{BodyState, Content, Geometry, ProjectionId, SourceSpan};
use nsh_core::{Affinity, TextPosition};
use nsh_platform_gpui::reading::{ReadingGeometry, ReadingSnapshot};
use std::{cell::Cell, rc::Rc};
use vega_markdown::SourcePosition;

#[cfg(test)]
#[path = "source_anchor_tests.rs"]
mod tests;

pub(super) struct Presence(pub(super) Rc<Cell<bool>>);

impl Drop for Presence {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

pub(super) struct Saved {
    pub(super) anchor: Anchor,
    pub(super) session: ProjectionId,
    pub(super) pending: bool,
    pub(super) host: Geometry,
    pub(super) geometry: ReadingGeometry,
    pub(super) area: u32,
    pub(super) mount: Rc<Cell<bool>>,
    pub(super) intent: u64,
    pub(super) observation: u64,
}

impl Saved {
    pub(super) fn current(&self, state: &BodyState) -> bool {
        !state.follow
            && self.session.matches(&state.projection.session)
            && self.intent == state.scroll_intent
            && self.observation == state.observation.get()
            && self.mount.get()
            && state
                .source_mount
                .as_ref()
                .is_some_and(|mount| Rc::ptr_eq(mount, &self.mount))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Anchor {
    pub(super) position: SourcePosition,
    pub(super) affinity: Affinity,
    pub(super) inset: f32,
}

impl Anchor {
    fn from_display(
        content: &Content,
        mapping: &[SourceSpan],
        byte: usize,
        affinity: Affinity,
        inset: f32,
    ) -> Option<Self> {
        if !inset.is_finite() || !content.text.is_char_boundary(byte) {
            return None;
        }
        let span = mapping
            .iter()
            .find(|span| match affinity {
                Affinity::Upstream => span.rendered.start < byte && byte <= span.rendered.end,
                Affinity::Downstream => span.rendered.start <= byte && byte < span.rendered.end,
            })
            .or_else(|| {
                mapping
                    .iter()
                    .find(|span| span.rendered.start <= byte && byte <= span.rendered.end)
            })?;
        Some(Self {
            position: span.position(byte),
            affinity,
            inset,
        })
    }

    pub(super) fn capture(
        content: &Content,
        source: &str,
        snapshot: &ReadingSnapshot,
        geometry: &ReadingGeometry,
        viewport: [f32; 4],
    ) -> Option<Self> {
        if !content.matches_snapshot(snapshot)
            || !viewport.iter().all(|value| value.is_finite())
            || viewport[2] <= 0.
            || viewport[3] <= 0.
        {
            return None;
        }
        let mapping = content.mapping(source);
        let mut candidate: Option<(f32, f32, Self)> = None;
        for (index, text) in snapshot.texts.iter().enumerate() {
            for tag in &text.content.test_ids {
                for rect in geometry.range_rects(index, tag.start..tag.end)? {
                    let left = rect[0].max(viewport[0]);
                    let top = rect[1].max(viewport[1]);
                    let right = (rect[0] + rect[2]).min(viewport[0] + viewport[2]);
                    let bottom = (rect[1] + rect[3]).min(viewport[1] + viewport[3]);
                    if left >= right || top >= bottom {
                        continue;
                    }
                    if candidate.as_ref().is_some_and(|(best_top, best_left, _)| {
                        !top.total_cmp(best_top)
                            .then(left.total_cmp(best_left))
                            .is_lt()
                    }) {
                        continue;
                    }
                    let hit = geometry.hit_test(
                        index,
                        [left + (right - left).min(1.) / 2., (top + bottom) / 2.],
                    )?;
                    if hit.offset < tag.start || hit.offset > tag.end {
                        continue;
                    }
                    let caret = geometry.caret_rect(index, hit)?;
                    if caret[1] + caret[3] <= top || caret[1] >= bottom {
                        continue;
                    }
                    let Some(byte) = content.display_endpoint(snapshot, (index, hit.offset)) else {
                        continue;
                    };
                    if let Some(anchor) = Self::from_display(
                        content,
                        &mapping,
                        byte,
                        hit.affinity,
                        caret[1] - viewport[1],
                    ) {
                        candidate = Some((top, left, anchor));
                    }
                }
            }
        }
        candidate.map(|(_, _, anchor)| anchor)
    }

    pub(super) fn delta(
        &self,
        content: &Content,
        source: &str,
        snapshot: &ReadingSnapshot,
        geometry: &ReadingGeometry,
        viewport_top: f32,
    ) -> Option<f32> {
        if !content.matches_snapshot(snapshot) {
            return None;
        }
        let byte = content.mapping(source).iter().find_map(|span| {
            span.rendered_position(&self.position)
                .filter(|byte| content.text.is_char_boundary(*byte))
        })?;
        let (index, offset) = snapshot
            .texts
            .iter()
            .enumerate()
            .find_map(|(index, text)| {
                text.content.test_ids.iter().find_map(|tag| {
                    let fragment = content.fragments.iter().find(|fragment| {
                        fragment.id == tag.test_id.as_ref()
                            && match self.affinity {
                                Affinity::Upstream => {
                                    fragment.range.start < byte && byte <= fragment.range.end
                                }
                                Affinity::Downstream => {
                                    fragment.range.start <= byte && byte < fragment.range.end
                                }
                            }
                    })?;
                    let start = super::utf16_byte(&text.content.text, tag.start)?;
                    let end = super::utf16_byte(&text.content.text, tag.end)?;
                    if text.content.text.get(start..end) != Some(fragment.text.as_str()) {
                        return None;
                    }
                    let prefix = fragment.text.get(..byte - fragment.range.start)?;
                    let units = u32::try_from(prefix.encode_utf16().count()).ok()?;
                    Some((index, tag.start.checked_add(units)?))
                })
            })
            .or_else(|| content.native_endpoint(snapshot, byte))?;
        let caret = geometry.caret_rect(index, TextPosition::new(offset, self.affinity))?;
        correction(caret[1], viewport_top, self.inset)
    }

    pub(super) fn remap(
        &self,
        current: &Content,
        previous: &Content,
        source: &str,
        previous_source: &str,
    ) -> Option<Self> {
        if !self.inset.is_finite() || !current.source_continues(previous, source, previous_source) {
            return None;
        }
        let mapping = current.mapping(source);
        if mapping.iter().any(|span| {
            span.rendered_position(&self.position)
                .is_some_and(|byte| current.text.is_char_boundary(byte))
        }) {
            return Some(self.clone());
        }
        if self.position.source.start >= current.source_start {
            return None;
        }
        let span = mapping
            .iter()
            .find(|span| span.source.start >= self.position.source.end.max(current.source_start))?;
        Some(Self {
            position: span.position(span.rendered.start),
            affinity: Affinity::Downstream,
            inset: 0.,
        })
    }
}

fn correction(caret_top: f32, viewport_top: f32, inset: f32) -> Option<f32> {
    let delta = caret_top - viewport_top - inset;
    (caret_top.is_finite() && viewport_top.is_finite() && inset.is_finite() && delta.is_finite())
        .then_some(delta)
}
