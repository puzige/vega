use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;

struct State {
    pending: BTreeSet<u64>,
    generation: u64,
    finished: bool,
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
                pending: BTreeSet::new(),
                generation: 0,
                finished: false,
            })),
            changed,
        }
    }

    pub fn pending(&self) -> PendingActivity {
        let id = self.state.lock().ok().map(|mut state| {
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

    pub fn finish(&self) {
        if let Ok(mut state) = self.state.lock() {
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
        }
        self.activity.changed.notify_waiters();
    }
}

impl Drop for PendingActivity {
    fn drop(&mut self) {
        self.finish();
    }
}
