use super::{Action, Request, Snapshot};
use nsh_core::prelude::*;
use tokio::sync::{mpsc, oneshot};

#[derive(Clone, Copy)]
pub(super) struct Viewport {
    pub(super) target: ElementRef,
    scroll: Signal<Option<ScrollEvent>>,
}

impl Viewport {
    pub(super) fn new(target: ElementRef) -> Self {
        let scroll = signal(None);
        watch_scroll(move || target.node(), move |event| scroll.set(Some(*event)));
        Self { target, scroll }
    }

    pub(super) fn observe(
        self,
        state: Signal<Snapshot>,
        attempted: Signal<Option<u64>>,
        commands: mpsc::Sender<Request>,
        action: Action,
        target: ElementRef,
    ) {
        effect(move || {
            let observed = state.get();
            if !observed.accepts_reveal(&observed, action)
                || attempted.get_untracked() == Some(observed.selection_revision)
            {
                return;
            }
            attempted.set(Some(observed.selection_revision));
            let commands = commands.clone();
            spawn(async move {
                let offset = self.scroll.get_untracked();
                let control_measure = target.measure();
                let viewport_measure = self.target.measure();
                let (Some(control), Some(viewport), Some(scroll)) =
                    (control_measure.await, viewport_measure.await, offset)
                else {
                    return;
                };
                let Some(destination) = destination(control, viewport, scroll) else {
                    return;
                };
                let (reply, response) = oneshot::channel();
                if commands
                    .send(Request::Reveal {
                        observed: Box::new(observed.clone()),
                        action,
                        reply,
                    })
                    .await
                    .is_err()
                    || !matches!(response.await, Ok(true))
                {
                    return;
                }
                if state.get_untracked().accepts_reveal(&observed, action)
                    && self.scroll.get_untracked() == Some(scroll)
                {
                    self.target.scroll_to(destination, false);
                }
            });
        });
    }
}

impl Snapshot {
    pub(super) fn accepts_reveal(&self, observed: &Self, action: Action) -> bool {
        self.keyboard_active
            && observed.keyboard_active
            && self.selected == Some(action)
            && observed.selected == Some(action)
            && self.selection_revision == observed.selection_revision
            && self.reveal_revision == Some(self.selection_revision)
            && observed.reveal_revision == Some(observed.selection_revision)
            && self.authority == observed.authority
            && self.accepts_action(observed, action)
            && self
                .controls()
                .iter()
                .any(|control| control.action == action)
    }
}

pub(super) fn destination(
    control: Measurement,
    viewport: Measurement,
    scroll: ScrollEvent,
) -> Option<Point<Points>> {
    let top = control.page.y.0;
    let height = control.frame.size.height.0;
    let start = viewport.page.y.0;
    let extent = viewport.frame.size.height.0;
    let offset = scroll.offset.y.0;
    let content = scroll.content_size.height.0;
    if ![
        top,
        height,
        start,
        extent,
        offset,
        content,
        scroll.offset.x.0,
    ]
    .iter()
    .all(|value| value.is_finite())
        || height <= 0.
        || extent <= 0.
        || content < extent
        || offset < 0.
    {
        return None;
    }
    let delta = if top < start || height > extent {
        top - start
    } else if top + height > start + extent {
        top + height - start - extent
    } else {
        return None;
    };
    let next = (offset + delta).clamp(0., content - extent);
    (next != offset).then_some(Point {
        x: scroll.offset.x,
        y: Points(next),
    })
}
