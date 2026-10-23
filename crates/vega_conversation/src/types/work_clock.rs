use std::collections::HashSet;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub type WorkTimeSource = Arc<dyn Fn() -> Instant + Send + Sync>;

#[derive(Clone)]
pub struct WorkClock {
    source: WorkTimeSource,
    state: Arc<Mutex<WorkClockState>>,
}

struct WorkClockState {
    active_since: Instant,
    elapsed: Duration,
    pauses: HashSet<u64>,
    next_pause: u64,
    finished: bool,
}

impl WorkClock {
    pub(crate) fn new(source: Option<WorkTimeSource>) -> Self {
        let source = source.unwrap_or_else(|| Arc::new(Instant::now));
        Self {
            state: Arc::new(Mutex::new(WorkClockState {
                active_since: source(),
                elapsed: Duration::ZERO,
                pauses: HashSet::new(),
                next_pause: 0,
                finished: false,
            })),
            source,
        }
    }

    pub fn elapsed(&self) -> Duration {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        self.elapsed_at(&state, (self.source)())
    }

    fn elapsed_at(&self, state: &WorkClockState, now: Instant) -> Duration {
        if state.finished || !state.pauses.is_empty() {
            state.elapsed
        } else {
            state
                .elapsed
                .saturating_add(now.saturating_duration_since(state.active_since))
        }
    }

    pub(crate) fn finish(&self) -> Duration {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.elapsed = self.elapsed_at(&state, (self.source)());
        state.finished = true;
        state.elapsed
    }

    pub(crate) fn pause(&self) -> WorkPause {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.elapsed = self.elapsed_at(&state, (self.source)());
        state.next_pause = state.next_pause.wrapping_add(1);
        let id = state.next_pause;
        state.pauses.insert(id);
        WorkPause {
            clock: self.clone(),
            id,
        }
    }
}

impl fmt::Debug for WorkClock {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("WorkClock").finish_non_exhaustive()
    }
}

pub(crate) struct WorkPause {
    clock: WorkClock,
    id: u64,
}

impl Drop for WorkPause {
    fn drop(&mut self) {
        let mut state = self
            .clock
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if state.pauses.remove(&self.id) && state.pauses.is_empty() {
            state.active_since = (self.clock.source)();
        }
    }
}
