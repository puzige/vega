use std::{
    cell::RefCell,
    ops::Range,
    rc::{Rc, Weak},
};

use crate::{surface::Surface, text::DrawnText};

use super::{ReadingSnapshot, is_current, snapshot};

#[derive(Clone, Debug)]
pub struct ReadingOpacityRange {
    pub text: usize,
    pub range: Range<u32>,
    pub opacity: f32,
}

#[derive(Clone)]
pub struct ReadingOpacityFence {
    surface: Weak<Surface>,
    token: Rc<()>,
    snapshot: ReadingSnapshot,
}

pub(crate) struct OpacityState {
    token: Rc<()>,
    publication: Option<Rc<Publication>>,
}

impl Default for OpacityState {
    fn default() -> Self {
        Self {
            token: Rc::new(()),
            publication: None,
        }
    }
}

struct Publication {
    fence: ReadingOpacityFence,
    ranges: Vec<ReadingOpacityRange>,
}

pub(crate) struct TextOpacityLease {
    publication: Weak<Publication>,
    text: usize,
}

#[derive(Default)]
pub(crate) struct TextOpacity {
    ranges: Vec<(Range<u32>, f32)>,
}

impl TextOpacity {
    pub(crate) fn at(&self, origin: u32) -> f32 {
        self.ranges
            .iter()
            .find(|(range, _)| range.contains(&origin))
            .map_or(1.0, |(_, opacity)| *opacity)
    }
}

impl ReadingOpacityFence {
    fn owns(&self, surface: &Rc<Surface>) -> bool {
        self.surface
            .upgrade()
            .is_some_and(|owner| Rc::ptr_eq(&owner, surface))
            && Rc::ptr_eq(&self.token, &surface.reading_opacity.borrow().token)
    }

    pub(crate) fn capture(surface: &Rc<Surface>, expected: &ReadingSnapshot) -> Option<Self> {
        let current = snapshot(surface, &expected.area_view)?;
        if !expected.matches(&current) {
            return None;
        }
        Some(Self {
            surface: Rc::downgrade(surface),
            token: surface.reading_opacity.borrow().token.clone(),
            snapshot: current,
        })
    }
}

fn scalar_boundary(text: &str, offset: u32) -> bool {
    let mut current = 0u64;
    for ch in text.chars() {
        if current == u64::from(offset) {
            return true;
        }
        current += ch.len_utf16() as u64;
        if current > u64::from(offset) {
            return false;
        }
    }
    current == u64::from(offset)
}

fn valid_ranges(snapshot: &ReadingSnapshot, ranges: &[ReadingOpacityRange]) -> bool {
    let mut previous: Option<(usize, u32)> = None;
    for range in ranges {
        let Some(text) = snapshot.texts.get(range.text) else {
            return false;
        };
        if range.range.start >= range.range.end
            || !range.opacity.is_finite()
            || !(0.0..=1.0).contains(&range.opacity)
            || !scalar_boundary(&text.content.text, range.range.start)
            || !scalar_boundary(&text.content.text, range.range.end)
            || previous.is_some_and(|(index, end)| {
                index > range.text || (index == range.text && end > range.range.start)
            })
        {
            return false;
        }
        previous = Some((range.text, range.range.end));
    }
    true
}

pub(crate) fn replace(
    surface: &Rc<Surface>,
    expected: &ReadingOpacityFence,
    ranges: Vec<ReadingOpacityRange>,
) -> Option<ReadingOpacityFence> {
    if !expected.owns(surface)
        || !is_current(surface, &expected.snapshot)
        || !valid_ranges(&expected.snapshot, &ranges)
    {
        return None;
    }
    let fence = ReadingOpacityFence {
        surface: Rc::downgrade(surface),
        token: Rc::new(()),
        snapshot: expected.snapshot.clone(),
    };
    let publication = Rc::new(Publication {
        fence: fence.clone(),
        ranges,
    });
    for (index, text) in fence.snapshot.texts.iter().enumerate() {
        let state = text.view.state();
        if let Some(drawn) = state.text.as_ref() {
            drawn.opacity.replace(Some(TextOpacityLease {
                publication: Rc::downgrade(&publication),
                text: index,
            }));
        }
    }
    surface.reading_opacity.replace(OpacityState {
        token: fence.token.clone(),
        publication: Some(publication),
    });
    surface.repaint_reading();
    Some(fence)
}

pub(crate) fn clear(surface: &Rc<Surface>, expected: &ReadingOpacityFence) -> bool {
    if !expected.owns(surface) {
        return false;
    }
    surface.reading_opacity.replace(OpacityState::default());
    surface.repaint_reading();
    true
}

pub(crate) fn invalidated(surface: &Surface) {
    surface.reading_opacity.replace(OpacityState::default());
}

pub(crate) fn for_draw(drawn: &DrawnText) -> TextOpacity {
    let lease = drawn.opacity.borrow();
    let Some(lease) = lease.as_ref() else {
        return TextOpacity::default();
    };
    let Some(publication) = lease.publication.upgrade() else {
        return TextOpacity::default();
    };
    let Some(surface) = publication.fence.surface.upgrade() else {
        return TextOpacity::default();
    };
    if !publication.fence.owns(&surface)
        || !surface
            .reading_opacity
            .borrow()
            .publication
            .as_ref()
            .is_some_and(|current| Rc::ptr_eq(current, &publication))
        || !{ is_current(&surface, &publication.fence.snapshot) }
        || !publication
            .fence
            .snapshot
            .texts
            .get(lease.text)
            .is_some_and(|text| std::sync::Arc::ptr_eq(&text.content, &drawn.text))
    {
        return TextOpacity::default();
    }
    TextOpacity {
        ranges: publication
            .ranges
            .iter()
            .filter(|range| range.text == lease.text)
            .map(|range| (range.range.clone(), range.opacity))
            .collect(),
    }
}

pub(crate) type OpacitySlot = RefCell<Option<TextOpacityLease>>;
