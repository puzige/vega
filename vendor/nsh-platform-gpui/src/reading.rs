use std::{cell::Cell, ops::Range, sync::Arc};

use crate::{surface::Surface, view::NshView};

thread_local! {
    static REVISION: Cell<u64> = const { Cell::new(0) };
}

pub(crate) fn changed() {
    REVISION.with(|revision| revision.set(revision.get().wrapping_add(1)));
}

use nsh_core::AttributedText;

mod geometry;
pub(crate) mod opacity;
pub use geometry::ReadingGeometry;
pub use opacity::{ReadingOpacityFence, ReadingOpacityRange};

pub(crate) fn geometry_invalidated(surface: &Surface) {
    opacity::invalidated(surface);
    geometry::invalidated(surface);
}

#[derive(Clone)]
pub struct ReadingText {
    pub node: u32,
    pub content: Arc<AttributedText>,
    pub(crate) view: NshView,
}

#[derive(Clone)]
pub struct ReadingSnapshot {
    pub surface: u64,
    pub generation: u64,
    pub batch: u64,
    pub area: u32,
    pub texts: Vec<ReadingText>,
    pub ranges: Vec<(usize, Range<u32>)>,
    pub endpoints: Option<((usize, u32), (usize, u32))>,
    revision: u64,
    pub(crate) area_view: NshView,
    pub(crate) cross_text: bool,
}

impl ReadingSnapshot {
    pub(crate) fn new(
        surface: &Surface,
        area_view: NshView,
        texts: Vec<ReadingText>,
        ranges: Vec<(usize, Range<u32>)>,
        cross_text: bool,
    ) -> Self {
        Self {
            surface: surface.key(),
            generation: surface.generation.get(),
            batch: surface.batches.get(),
            area: area_view.id(),
            texts,
            ranges,
            endpoints: None,
            revision: REVISION.with(Cell::get),
            area_view,
            cross_text,
        }
    }

    pub(crate) fn matches(&self, other: &Self) -> bool {
        self.surface == other.surface
            && self.generation == other.generation
            && self.batch == other.batch
            && self.area == other.area
            && self.area_view == other.area_view
            && self.revision == other.revision
            && self.cross_text == other.cross_text
            && self.ranges == other.ranges
            && self.endpoints == other.endpoints
            && self.texts.len() == other.texts.len()
            && self.texts.iter().zip(&other.texts).all(|(a, b)| {
                a.node == b.node && a.view == b.view && Arc::ptr_eq(&a.content, &b.content)
            })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadingIntent {
    Copy,
    ContextMenu,
    PrimarySelection,
}

#[derive(Clone)]
pub struct ReadingRequest {
    pub intent: ReadingIntent,
    pub snapshot: ReadingSnapshot,
}

pub(crate) type ReadingHandler = std::rc::Rc<dyn Fn(ReadingRequest)>;
pub(crate) type SelectionObserver = std::rc::Rc<dyn Fn(Option<ReadingSnapshot>)>;

pub(crate) struct SelectionListener {
    area: NshView,
    generation: u64,
    observer: SelectionObserver,
    published: std::cell::RefCell<Option<Option<ReadingSnapshot>>>,
}

pub(crate) fn set_listener(
    surface: &Surface,
    area: u32,
    observer: Option<SelectionObserver>,
) -> bool {
    if surface.reading_listener_mutating.replace(true) {
        return false;
    }
    let generation = surface.generation.get();
    let target = surface
        .view(area)
        .filter(|view| snapshot(surface, view).is_some());
    let previous = surface.reading_listener.borrow_mut().take();
    if let Some(previous) = previous {
        (previous.observer)(None);
    }
    let valid = match (target, observer) {
        (_, None) => true,
        (Some(area), Some(observer))
            if generation == surface.generation.get() && snapshot(surface, &area).is_some() =>
        {
            surface
                .reading_listener
                .replace(Some(std::rc::Rc::new(SelectionListener {
                    area,
                    generation: surface.generation.get(),
                    observer,
                    published: std::cell::RefCell::new(None),
                })));
            true
        }
        _ => false,
    };
    surface.reading_listener_mutating.set(false);
    publish(surface);
    valid
}

pub(crate) fn publish(surface: &Surface) {
    let listener = surface.reading_listener.borrow().clone();
    let Some(listener) = listener else { return };
    let current = (listener.generation == surface.generation.get())
        .then(|| snapshot(surface, &listener.area))
        .flatten()
        .filter(|snapshot| snapshot.ranges.iter().any(|(_, range)| !range.is_empty()));
    let unchanged = listener
        .published
        .borrow()
        .as_ref()
        .is_some_and(|previous| match (previous, &current) {
            (Some(a), Some(b)) => a.matches(b),
            (None, None) => true,
            _ => false,
        });
    if unchanged {
        return;
    }
    listener.published.replace(Some(current.clone()));
    (listener.observer)(current);
}

pub(crate) fn stopped(surface: &Surface) {
    opacity::invalidated(surface);
    let was_mutating = surface.reading_listener_mutating.replace(true);
    let listener = surface.reading_listener.borrow_mut().take();
    if let Some(listener) = listener {
        (listener.observer)(None);
    }
    surface.reading_listener_mutating.set(was_mutating);
}

pub(crate) fn set_handler(surface: &Surface, handler: Option<ReadingHandler>) {
    surface
        .reading_epoch
        .set(surface.reading_epoch.get().wrapping_add(1));
    *surface.reading_handler.borrow_mut() = handler;
}

#[derive(Clone, Copy)]
pub(crate) struct ClipboardLease {
    generation: u64,
    epoch: u64,
}

impl ClipboardLease {
    pub(crate) fn capture(surface: &Surface) -> Option<Self> {
        surface.reading_handler.borrow().is_none().then(|| Self {
            generation: surface.generation.get(),
            epoch: surface.reading_epoch.get(),
        })
    }

    pub(crate) fn current(self, surface: &Surface) -> bool {
        self.generation == surface.generation.get()
            && self.epoch == surface.reading_epoch.get()
            && surface.reading_handler.borrow().is_none()
    }
}

pub(crate) fn snapshot(surface: &Surface, view: &NshView) -> Option<ReadingSnapshot> {
    if surface.view(view.id()).as_ref() != Some(view) || !view.attached() {
        return None;
    }
    if view.state().props.selectable_view {
        crate::selectable_view::reading_snapshot(surface, view)
    } else {
        crate::text::reading_snapshot(surface, view)
    }
}

pub(crate) fn install(
    surface: &Surface,
    expected: &ReadingSnapshot,
    anchor: (usize, u32),
    focus: (usize, u32),
) -> Option<ReadingSnapshot> {
    if !is_current(surface, expected) {
        return None;
    }
    let installed = if expected.cross_text {
        crate::selectable_view::install_selection(surface, &expected.area_view, anchor, focus)
    } else {
        anchor.0 == 0
            && focus.0 == 0
            && crate::text::install_reading_selection(
                surface,
                &expected.area_view,
                anchor.1,
                focus.1,
            )
    };
    if !installed {
        return None;
    }
    snapshot(surface, &expected.area_view)
}

pub(crate) fn is_current(surface: &Surface, captured: &ReadingSnapshot) -> bool {
    snapshot(surface, &captured.area_view).is_some_and(|now| captured.matches(&now))
}

pub(crate) fn intercept(surface: &Surface, view: &NshView, intent: ReadingIntent) -> bool {
    let handler = surface.reading_handler.borrow().clone();
    let Some(handler) = handler else {
        return false;
    };
    if let Some(snapshot) = snapshot(surface, view) {
        handler(ReadingRequest { intent, snapshot });
    }
    true
}
