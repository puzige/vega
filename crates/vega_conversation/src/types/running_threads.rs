//! App-wide, thread-scoped Agent liveness for UI running indicators.
//!
//! The authoritative source of "which thread is running" is the application's
//! `AppAgentController.active` map (thread id → active run). This type is only
//! the read-only projection that the UI consumes, so a sidebar row can show a
//! real running indicator without inferring liveness from `unread`,
//! timestamps or selection (design guidelines §2.3: state must be real).

use std::collections::BTreeSet;

/// The set of threads that currently own a live Agent worker.
///
/// A `BTreeSet` (not a `HashSet`) keeps the value comparable and its `Debug`
/// output stable, so a repeated write of the same liveness can be detected
/// and skipped instead of causing a repaint.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunningThreads {
    ids: BTreeSet<String>,
}

impl RunningThreads {
    /// Whether `thread_id` currently owns a live worker.
    pub fn is_running(&self, thread_id: &str) -> bool {
        self.ids.contains(thread_id)
    }

    /// Records the liveness of one thread. Idempotent for a repeated value.
    pub fn set(&mut self, thread_id: &str, running: bool) {
        if running {
            self.ids.insert(thread_id.to_owned());
        } else {
            self.ids.remove(thread_id);
        }
    }

    /// Number of threads currently running.
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    /// Whether no thread is running.
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::RunningThreads;

    #[test]
    fn tracks_each_thread_independently() {
        let mut running = RunningThreads::default();
        assert!(running.is_empty());
        running.set("a", true);
        running.set("b", true);
        running.set("a", false);
        assert!(!running.is_running("a"));
        assert!(running.is_running("b"));
        assert_eq!(running.len(), 1);
        assert!(!running.is_empty());
    }

    #[test]
    fn repeated_writes_are_idempotent() {
        let mut running = RunningThreads::default();
        running.set("a", true);
        let started = running.clone();
        running.set("a", true);
        assert_eq!(
            running, started,
            "a repeated true must not change the value"
        );

        running.set("a", false);
        let stopped = running.clone();
        running.set("a", false);
        assert_eq!(
            running, stopped,
            "a repeated false must not change the value"
        );
        assert!(running.is_empty());
    }
}
