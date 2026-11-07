use super::{Action, Request, Snapshot};
use crate::nsh::ProjectionId;
use nsh_core::prelude::*;
use tokio::sync::{mpsc, oneshot};

const ATTEMPTS_PER_OBSERVATION: usize = 2;
const ATTEMPTS_PER_SELECTION: usize = 6;

#[derive(Clone, Copy, PartialEq)]
pub(super) struct Observation {
    pub(super) scroll: ScrollEvent,
    pub(super) layout: u64,
}

pub(super) struct Attempt {
    id: ProjectionId,
    observed: Snapshot,
    observation: Observation,
}

#[derive(Default)]
pub(super) struct Attempts {
    selection: Option<(ProjectionId, ProjectionId)>,
    observation: Option<Observation>,
    total: usize,
    consecutive: usize,
    flight: Option<ProjectionId>,
    completed: bool,
    rejected: bool,
}

pub(super) enum Completion {
    Retry,
    Rejected,
    Measured,
}

impl Attempts {
    pub(super) fn begin(
        &mut self,
        observed: &Snapshot,
        action: &Action,
        observation: Observation,
    ) -> Option<Attempt> {
        if !observed.accepts_selection(observed, action) {
            return None;
        }
        if !self.selection.as_ref().is_some_and(|(surface, selection)| {
            surface.matches(&observed.id) && selection.matches(&observed.selection)
        }) {
            *self = Self {
                selection: Some((observed.id.clone(), observed.selection.clone())),
                ..Self::default()
            };
        }
        if self.flight.is_some() || self.completed || self.rejected {
            return None;
        }
        if self.observation != Some(observation) {
            self.observation = Some(observation);
            self.consecutive = 0;
        }
        if self.total >= ATTEMPTS_PER_SELECTION || self.consecutive >= ATTEMPTS_PER_OBSERVATION {
            return None;
        }
        self.total += 1;
        self.consecutive += 1;
        let id = ProjectionId::new();
        self.flight = Some(id.clone());
        Some(Attempt {
            id,
            observed: observed.clone(),
            observation,
        })
    }

    pub(super) fn finish(
        &mut self,
        attempt: &Attempt,
        current: &Snapshot,
        action: &Action,
        observation: Option<Observation>,
        completion: Completion,
    ) -> bool {
        if !self
            .flight
            .as_ref()
            .is_some_and(|id| id.matches(&attempt.id))
        {
            return false;
        }
        self.flight = None;
        if matches!(completion, Completion::Rejected) {
            self.rejected = true;
            return false;
        }
        if !current.accepts_selection(&attempt.observed, action) {
            return false;
        }
        match completion {
            Completion::Rejected => {}
            Completion::Measured if observation == Some(attempt.observation) => {
                self.completed = true;
                return true;
            }
            Completion::Retry | Completion::Measured => {}
        }
        false
    }
}

#[derive(Clone, Copy)]
pub(super) struct Tracker {
    attempts: Stored<Attempts>,
    wake: Signal<u64>,
    owner: Owner,
}

impl Tracker {
    pub(super) fn new() -> Self {
        Self {
            attempts: stored(Attempts::default()),
            wake: signal(0),
            owner: Owner::current(),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Viewport {
    pub(super) target: ElementRef,
    scroll: Signal<Option<ScrollEvent>>,
    revision: Signal<u64>,
    bounds: Signal<Option<Bounds<Points>>>,
}

impl Viewport {
    pub(super) fn new(target: ElementRef) -> Self {
        let scroll = signal(None);
        let revision = signal(0u64);
        watch_scroll(
            move || target.node(),
            move |event| {
                if scroll.get_untracked() != Some(*event) {
                    scroll.set(Some(*event));
                    revision.update(|revision| *revision = revision.saturating_add(1));
                }
            },
        );
        Self {
            target,
            scroll,
            revision,
            bounds: signal(None),
        }
    }

    pub(super) fn invalidate(self) {
        self.revision
            .update(|revision| *revision = revision.saturating_add(1));
    }

    pub(super) fn layout(self, bounds: Bounds<Points>) {
        if self.bounds.get_untracked() != Some(bounds) {
            self.bounds.set(Some(bounds));
            self.invalidate();
        }
    }

    fn observation(self) -> Option<Observation> {
        self.scroll.get_untracked().map(|scroll| Observation {
            scroll,
            layout: self.revision.get_untracked(),
        })
    }

    pub(super) fn observe(
        self,
        state: Signal<Snapshot>,
        tracker: Tracker,
        commands: mpsc::Sender<Request>,
        action: Action,
        target: ElementRef,
    ) {
        effect(move || {
            let observed = state.get();
            let scroll = self.scroll.get();
            let layout = self.revision.get();
            tracker.wake.get();
            let Some(scroll) = scroll else {
                return;
            };
            let Some(attempt) = tracker.attempts.with(|attempts| {
                attempts.begin(&observed, &action, Observation { scroll, layout })
            }) else {
                return;
            };
            let commands = commands.clone();
            let action = action.clone();
            tracker.owner.run(move || {
                spawn(async move {
                    let completion = self
                        .measure(state, &commands, &action, target, &attempt)
                        .await;
                    let displayed = tracker.attempts.with(|attempts| {
                        attempts.finish(
                            &attempt,
                            &state.get_untracked(),
                            &action,
                            self.observation(),
                            completion,
                        )
                    });
                    if displayed {
                        let _ = commands
                            .send(Request::Displayed(Box::new(attempt.observed), action))
                            .await;
                    } else {
                        tracker.wake.update(|wake| *wake = wake.saturating_add(1));
                    }
                });
            });
        });
    }

    async fn measure(
        self,
        state: Signal<Snapshot>,
        commands: &mpsc::Sender<Request>,
        action: &Action,
        target: ElementRef,
        attempt: &Attempt,
    ) -> Completion {
        let control_measure = target.measure();
        let viewport_measure = self.target.measure();
        let (Some(control), Some(viewport)) = (control_measure.await, viewport_measure.await)
        else {
            return Completion::Retry;
        };
        if !state
            .get_untracked()
            .accepts_selection(&attempt.observed, action)
            || self.observation() != Some(attempt.observation)
        {
            return Completion::Retry;
        }
        if attempt.observed.reveal
            && let Some(destination) = destination(control, viewport, attempt.observation.scroll)
        {
            let (reply, response) = oneshot::channel();
            if commands
                .send(Request::Reveal(
                    Box::new(attempt.observed.clone()),
                    action.clone(),
                    reply,
                ))
                .await
                .is_err()
                || !matches!(response.await, Ok(true))
            {
                return Completion::Rejected;
            }
            if !state
                .get_untracked()
                .accepts_reveal(&attempt.observed, action)
                || self.observation() != Some(attempt.observation)
            {
                return Completion::Retry;
            }
            self.target.scroll_to(destination, false);
            let _ = target.measure().await;
            return Completion::Retry;
        }
        if displayed(control, viewport, attempt.observation.scroll) {
            Completion::Measured
        } else {
            Completion::Retry
        }
    }
}

fn displayed(control: Measurement, viewport: Measurement, scroll: ScrollEvent) -> bool {
    let top = control.page.y.0;
    let height = control.frame.size.height.0;
    let start = viewport.page.y.0;
    let extent = viewport.frame.size.height.0;
    [
        control.page.x.0,
        top,
        control.frame.size.width.0,
        height,
        viewport.page.x.0,
        start,
        viewport.frame.size.width.0,
        extent,
        scroll.offset.y.0,
        scroll.content_size.height.0,
    ]
    .iter()
    .all(|value| value.is_finite())
        && control.frame.size.width.0 > 0.
        && viewport.frame.size.width.0 > 0.
        && height > 0.
        && extent > 0.
        && scroll.offset.y.0 >= 0.
        && scroll.content_size.height.0 >= extent
        && top >= start
        && (top + height <= start + extent || (height > extent && top == start))
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
