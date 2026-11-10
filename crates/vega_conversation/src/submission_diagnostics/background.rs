use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

type Job = Box<dyn FnOnce() -> Result<(), String> + Send>;

#[derive(Default)]
struct State {
    jobs: VecDeque<Job>,
    active: usize,
    closing: bool,
    failures: usize,
}

struct Pool {
    state: Mutex<State>,
    changed: Condvar,
    capacity: usize,
}

static POOL: OnceLock<Result<Arc<Pool>, String>> = OnceLock::new();

impl Pool {
    fn start(workers: usize, capacity: usize) -> Result<Arc<Self>, String> {
        let pool = Arc::new(Self {
            state: Mutex::new(State::default()),
            changed: Condvar::new(),
            capacity,
        });
        for index in 0..workers {
            let worker = pool.clone();
            if std::thread::Builder::new()
                .name(format!("vega-write-coordinator-{index}"))
                .spawn(move || worker.run())
                .is_err()
            {
                if let Ok(mut state) = pool.state.lock() {
                    state.closing = true;
                    pool.changed.notify_all();
                }
                return Err("background write coordinator unavailable".into());
            }
        }
        Ok(pool)
    }

    fn run(&self) {
        loop {
            let job = {
                let Ok(mut state) = self.state.lock() else {
                    return;
                };
                while state.jobs.is_empty() && !state.closing {
                    let Ok(next) = self.changed.wait(state) else {
                        return;
                    };
                    state = next;
                }
                let Some(job) = state.jobs.pop_front() else {
                    return;
                };
                state.active += 1;
                job
            };
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
            let succeeded = matches!(outcome, Ok(Ok(())));
            if !succeeded {
                tracing::warn!(outcome = "accepted_background_write_not_confirmed");
            }
            if let Ok(mut state) = self.state.lock() {
                state.active -= 1;
                state.failures += usize::from(!succeeded);
                self.changed.notify_all();
            }
        }
    }

    fn submit(&self, job: Job) -> Result<(), String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "background coordinator unavailable")?;
        if state.closing {
            return Err("background coordinator closing".into());
        }
        if state.jobs.len() >= self.capacity {
            return Err("background coordinator capacity exhausted".into());
        }
        state.jobs.push_back(job);
        self.changed.notify_one();
        Ok(())
    }

    fn shutdown(&self, timeout: Duration) -> Result<(), String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "background coordinator unavailable")?;
        state.closing = true;
        self.changed.notify_all();
        let deadline = Instant::now() + timeout;
        while state.active != 0 || !state.jobs.is_empty() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(format!(
                    "background writes unsettled: {} queued, {} active, {} failed",
                    state.jobs.len(), state.active, state.failures
                ));
            }
            state = self
                .changed
                .wait_timeout(state, remaining)
                .map_err(|_| "background coordinator unavailable")?
                .0;
        }
        if state.failures != 0 {
            return Err(format!(
                "{} accepted background writes failed",
                state.failures
            ));
        }
        Ok(())
    }
}

pub(crate) fn submit(
    job: impl FnOnce() -> Result<(), String> + Send + 'static,
) -> Result<(), String> {
    POOL.get_or_init(|| Pool::start(2, 1024))
        .as_ref()
        .map_err(Clone::clone)?
        .submit(Box::new(job))
}

pub fn shutdown() -> Result<(), String> {
    POOL.get_or_init(|| Pool::start(2, 1024)).as_ref()
        .map_err(Clone::clone)?
        .shutdown(Duration::from_secs(15))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn bounded_pool_rejects_before_acceptance_and_drains_accepted_jobs() {
        let pool = Pool::start(1, 1).unwrap();
        let (started, observed) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        pool.submit(Box::new(move || {
            started.send(()).unwrap();
            gate.recv().unwrap();
            Ok(())
        })).unwrap();
        observed.recv_timeout(Duration::from_secs(2)).unwrap();
        let (committed, receipt) = mpsc::channel();
        pool.submit(Box::new(move || {
            committed.send(()).unwrap();
            Ok(())
        })).unwrap();
        assert!(pool.submit(Box::new(|| panic!("rejected job executed"))).is_err());
        assert!(pool.shutdown(Duration::from_millis(10)).unwrap_err().contains("unsettled"));
        assert!(pool.submit(Box::new(|| Ok(()))).unwrap_err().contains("closing"));
        release.send(()).unwrap();
        pool.shutdown(Duration::from_secs(2)).unwrap();
        receipt.recv_timeout(Duration::from_secs(2)).unwrap();
    }

    #[test]
    fn shutdown_reports_actual_job_failure_without_abandoning_later_jobs() {
        let pool = Pool::start(1, 2).unwrap();
        let (started, observed) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        pool.submit(Box::new(move || {
            started.send(()).unwrap();
            gate.recv().unwrap();
            Err("injected persistence failure".into())
        })).unwrap();
        observed.recv_timeout(Duration::from_secs(2)).unwrap();
        let (committed, receipt) = mpsc::channel();
        pool.submit(Box::new(move || {
            committed.send(()).unwrap();
            Ok(())
        })).unwrap();
        release.send(()).unwrap();
        assert!(pool.shutdown(Duration::from_secs(2)).unwrap_err().contains("1 accepted background writes failed"));
        receipt.recv_timeout(Duration::from_secs(2)).unwrap();
    }
}
