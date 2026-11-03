//! App-wide project worker liveness, independent of UI route ownership.

use std::{
    collections::HashMap,
    sync::{
        Arc, Weak,
        atomic::{AtomicU8, Ordering},
    },
};

/// Weak ownership records for workers using a registered project's folder.
///
/// A worker holds the returned strong token until its entry point actually
/// returns. Cancelling a UI route, closing its window, or receiving a terminal
/// event does not release the token while a tool may still be executing.
#[derive(Default)]
pub struct ProjectWorkerActivity {
    workers: HashMap<String, Vec<Weak<()>>>,
    removals: HashMap<String, Arc<AtomicU8>>,
}

impl ProjectWorkerActivity {
    /// Register before spawning the worker. The caller must move this token
    /// into the worker closure; failed spawns drop it without a live worker.
    pub fn register(&mut self, project_id: &str) -> Option<Arc<()>> {
        if self
            .removals
            .get(project_id)
            .is_some_and(|state| state.load(Ordering::Acquire) != 2)
        {
            return None;
        }
        self.workers.retain(|_, workers| {
            workers.retain(|worker| worker.strong_count() > 0);
            !workers.is_empty()
        });
        let token = Arc::new(());
        if !project_id.is_empty() {
            self.workers
                .entry(project_id.to_owned())
                .or_default()
                .push(Arc::downgrade(&token));
        }
        Some(token)
    }

    pub fn begin_removal(&mut self, project_id: &str) -> Option<ProjectRemoval> {
        if self.is_active(project_id)
            || self
                .removals
                .get(project_id)
                .is_some_and(|state| state.load(Ordering::Acquire) != 2)
        {
            return None;
        }
        let state = Arc::new(AtomicU8::new(0));
        self.removals.insert(project_id.to_owned(), state.clone());
        Some(ProjectRemoval { state })
    }

    /// A registration is busy until *every* worker for that project exits.
    pub fn is_active(&self, project_id: &str) -> bool {
        self.workers
            .get(project_id)
            .is_some_and(|workers| workers.iter().any(|worker| worker.upgrade().is_some()))
    }
}

pub struct ProjectRemoval {
    state: Arc<AtomicU8>,
}

impl ProjectRemoval {
    pub fn commit(&self) {
        self.state.store(1, Ordering::Release);
    }
}

impl Drop for ProjectRemoval {
    fn drop(&mut self) {
        let _ = self
            .state
            .compare_exchange(0, 2, Ordering::AcqRel, Ordering::Acquire);
    }
}
