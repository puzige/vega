//! App-wide project worker liveness, independent of UI route ownership.

use std::{
    collections::HashMap,
    sync::{Arc, Weak},
};

/// Weak ownership records for workers using a registered project's folder.
///
/// A worker holds the returned strong token until its entry point actually
/// returns. Cancelling a UI route, closing its window, or receiving a terminal
/// event does not release the token while a tool may still be executing.
#[derive(Default)]
pub struct ProjectWorkerActivity {
    workers: HashMap<String, Vec<Weak<()>>>,
}

impl ProjectWorkerActivity {
    /// Register before spawning the worker. The caller must move this token
    /// into the worker closure; failed spawns drop it without a live worker.
    pub fn register(&mut self, project_id: &str) -> Arc<()> {
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
        token
    }

    /// A registration is busy until *every* worker for that project exits.
    pub fn is_active(&self, project_id: &str) -> bool {
        self.workers
            .get(project_id)
            .is_some_and(|workers| workers.iter().any(|worker| worker.upgrade().is_some()))
    }
}
