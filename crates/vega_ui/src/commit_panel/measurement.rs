use super::nsh_view::{Request, Snapshot};
use crate::nsh::OrderedCommands;
use nsh_core::prelude::*;
use std::sync::{Arc, Weak};

#[derive(Clone)]
pub(super) struct MeasurementLease {
    element: Weak<()>,
    geometry: Weak<()>,
}

impl MeasurementLease {
    pub(super) fn live(&self) -> bool {
        self.element.strong_count() > 0 && self.geometry.strong_count() > 0
    }
}

#[derive(Clone, Copy)]
pub(super) struct Viewport {
    state: Signal<Snapshot>,
    pub(super) root: ElementRef,
    offset: Signal<f32>,
    list_offset: Signal<f32>,
    size: Signal<(f32, f32)>,
    list_size: Signal<(f32, f32)>,
    revision: Signal<Arc<()>>,
    revealed: Signal<Option<u64>>,
}

impl Viewport {
    pub(super) fn new(state: Signal<Snapshot>) -> Self {
        Self {
            state,
            root: element_ref(),
            offset: signal(0.),
            list_offset: signal(0.),
            size: signal((0., 0.)),
            list_size: signal((0., 0.)),
            revision: signal_with(Arc::new(()), SignalEquals::Never),
            revealed: signal(None),
        }
    }

    pub(super) fn scrolled(self, offset: f32, list: bool) {
        let current = if list { self.list_offset } else { self.offset };
        if current.get_untracked() != offset {
            current.set(offset);
            self.invalidate();
        }
    }

    pub(super) fn resized(self, bounds: &Bounds<Points>, list: bool) {
        let previous = if list { self.list_size } else { self.size };
        let size = (bounds.size.width.0, bounds.size.height.0);
        if previous.get_untracked() != size {
            previous.set(size);
            self.revealed.set(None);
            self.invalidate();
        }
    }

    pub(super) fn invalidate(self) {
        self.revision.set(Arc::new(()));
    }

    pub(super) fn follow_input(self, target: ElementRef) {
        effect(move || {
            let observed = self.state.get();
            let geometry = Arc::downgrade(&self.revision.get());
            if observed.focus != super::CommitPanelFocus::Draft
                || self.revealed.get_untracked() == Some(observed.focus_revision)
            {
                return;
            }
            spawn(async move {
                let (Some(field), Some(viewport)) =
                    (target.measure().await, self.root.measure().await)
                else {
                    return;
                };
                let current = self.state.get_untracked();
                if geometry.strong_count() == 0
                    || target.node().is_none()
                    || current.focus != super::CommitPanelFocus::Draft
                    || current.focus_revision != observed.focus_revision
                    || !current.page.matches(&observed.page)
                {
                    return;
                }
                self.revealed.set(Some(current.focus_revision));
                let y = field.page.y.0 - viewport.page.y.0;
                let delta = if y < 0. {
                    y
                } else {
                    (y + field.frame.size.height.0 - viewport.frame.size.height.0).max(0.)
                };
                if delta.abs() > 0.5 {
                    self.root
                        .scroll_to_offset(px((self.offset.get_untracked() + delta).max(0.)), false);
                }
            });
        });
    }

    pub(super) fn follow(
        self,
        target: ElementRef,
        list: Option<ElementRef>,
        observed: Snapshot,
        commands: OrderedCommands<Request>,
        element: Weak<()>,
    ) {
        effect(move || {
            let lease = MeasurementLease {
                element: element.clone(),
                geometry: Arc::downgrade(&self.revision.get()),
            };
            let observed = observed.clone();
            let commands = commands.clone();
            spawn(async move {
                let (Some(control), Some(viewport)) =
                    (target.measure().await, self.root.measure().await)
                else {
                    return;
                };
                let mut x = control.page.x.0;
                let mut y = control.page.y.0;
                let mut right = x + control.frame.size.width.0;
                let mut bottom = y + control.frame.size.height.0;
                if let Some(list) = list {
                    let Some(clip) = list.measure().await else {
                        return;
                    };
                    x = x.max(clip.page.x.0);
                    y = y.max(clip.page.y.0);
                    right = right.min(clip.page.x.0 + clip.frame.size.width.0);
                    bottom = bottom.min(clip.page.y.0 + clip.frame.size.height.0);
                }
                if !lease.live()
                    || target.node().is_none()
                    || !self.state.get_untracked().same_authority(&observed)
                {
                    return;
                }
                let top = viewport.page.y.0;
                let end = top + viewport.frame.size.height.0;
                if right > x
                    && bottom > y
                    && self.revealed.get_untracked() != Some(observed.focus_revision)
                {
                    self.revealed.set(Some(observed.focus_revision));
                    let delta = if y < top {
                        y - top
                    } else {
                        (bottom - end).max(0.)
                    };
                    if delta.abs() > 0.5 {
                        self.root.scroll_to_offset(
                            px((self.offset.get_untracked() + delta).max(0.)),
                            false,
                        );
                    }
                }
                x = x.max(viewport.page.x.0);
                right = right.min(viewport.page.x.0 + viewport.frame.size.width.0);
                y = y.max(top);
                bottom = bottom.min(end);
                let rect = (right > x && bottom > y).then_some([x, y, right - x, bottom - y]);
                commands.send(Request::Measured(observed, rect, lease));
            });
        });
    }
}
