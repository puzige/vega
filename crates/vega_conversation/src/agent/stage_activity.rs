use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Notify;
use tokio::time::Instant;

const ACTIVE_LIMIT: Duration = Duration::from_secs(20 * 60);

struct State {
    active: Duration,
    since: Instant,
    pending: BTreeSet<u64>,
    generation: u64,
    finished: bool,
    expired: bool,
}

#[derive(Clone)]
pub(super) struct StageActivity {
    state: Arc<Mutex<State>>,
    pub changed: Arc<Notify>,
}

impl StageActivity {
    pub fn new(changed: Arc<Notify>) -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                active: Duration::ZERO,
                since: Instant::now(),
                pending: BTreeSet::new(),
                generation: 0,
                finished: false,
                expired: false,
            })),
            changed,
        }
    }

    pub fn pending(&self) -> PendingActivity {
        let id = self.state.lock().ok().map(|mut state| {
            if state.pending.is_empty() && !state.finished {
                state.active = state.active.saturating_add(state.since.elapsed());
            }
            state.generation = state.generation.saturating_add(1);
            let id = state.generation;
            state.pending.insert(id);
            id
        });
        self.changed.notify_waiters();
        PendingActivity {
            activity: self.clone(),
            id,
            resolved: AtomicBool::new(false),
        }
    }

    pub fn attention(&self) -> Option<u64> {
        self.state.lock().ok().and_then(|state| {
            (!state.pending.is_empty() && !state.finished).then_some(state.generation)
        })
    }

    pub fn remaining(&self) -> Option<Duration> {
        self.state.lock().ok().and_then(|state| {
            if state.finished || state.expired || !state.pending.is_empty() {
                return None;
            }
            Some(ACTIVE_LIMIT.saturating_sub(state.active.saturating_add(state.since.elapsed())))
        })
    }

    pub fn expire(&self) -> bool {
        let expired = self.state.lock().is_ok_and(|mut state| {
            let active = if state.pending.is_empty() {
                state.active.saturating_add(state.since.elapsed())
            } else {
                state.active
            };
            if state.finished || state.expired || active < ACTIVE_LIMIT {
                return false;
            }
            state.expired = true;
            true
        });
        if expired {
            self.changed.notify_waiters();
        }
        expired
    }

    pub fn expired(&self) -> bool {
        self.state.lock().is_ok_and(|state| state.expired)
    }

    pub fn finish(&self) {
        if let Ok(mut state) = self.state.lock() {
            if state.pending.is_empty() && !state.finished {
                state.active = state.active.saturating_add(state.since.elapsed());
            }
            state.finished = true;
        }
        self.changed.notify_waiters();
    }
}

pub(super) struct PendingActivity {
    activity: StageActivity,
    id: Option<u64>,
    resolved: AtomicBool,
}

impl PendingActivity {
    pub fn finish(&self) {
        if self.resolved.swap(true, Ordering::SeqCst) {
            return;
        }
        if let Some(id) = self.id
            && let Ok(mut state) = self.activity.state.lock()
        {
            state.pending.remove(&id);
            if state.pending.is_empty() {
                state.since = Instant::now();
            }
        }
        self.activity.changed.notify_waiters();
    }
}

impl Drop for PendingActivity {
    fn drop(&mut self) {
        self.finish();
    }
}
