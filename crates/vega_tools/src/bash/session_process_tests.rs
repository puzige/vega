use super::*;
use std::cell::RefCell;

#[derive(Default)]
struct ObservedProcesses {
    identities: BTreeMap<i32, Identity>,
    signals: Vec<(i32, i32)>,
    inspections: usize,
    change_at: Option<usize>,
    list_failure: bool,
    now: Option<tokio::time::Instant>,
    inspect_cost: Duration,
}

#[derive(Default)]
struct FakeProcesses(RefCell<ObservedProcesses>);

impl FakeProcesses {
    fn root() -> Self {
        let api = Self::default();
        api.0
            .borrow_mut()
            .identities
            .insert(10, identity(10, 1, 10));
        api
    }
}

impl ProcessApi for FakeProcesses {
    fn now(&self) -> tokio::time::Instant {
        self.0
            .borrow()
            .now
            .unwrap_or_else(tokio::time::Instant::now)
    }
    fn inspect(&self, pid: i32) -> Result<Option<Identity>, ()> {
        let mut state = self.0.borrow_mut();
        state.inspections += 1;
        let cost = state.inspect_cost;
        if let Some(now) = &mut state.now {
            *now += cost;
        }
        let mut current = state.identities.get(&pid).copied();
        if state.change_at == Some(state.inspections)
            && let Some(current) = &mut current
        {
            current.birth.0 += 1;
        }
        Ok(current)
    }

    fn list(&self, pid: i32, group: bool) -> Result<Vec<i32>, &'static str> {
        let state = self.0.borrow();
        if state.list_failure {
            return Err("process_observation_failed");
        }
        Ok(state
            .identities
            .values()
            .filter(|identity| {
                if group {
                    identity.group == pid as u32
                } else {
                    identity.parent == pid as u32
                }
            })
            .map(|identity| identity.pid)
            .collect())
    }

    fn signal(&self, target: i32, signal: i32) -> Result<(), ()> {
        self.0.borrow_mut().signals.push((target, signal));
        Ok(())
    }
}

fn identity(pid: i32, parent: u32, group: u32) -> Identity {
    Identity {
        pid,
        parent,
        group,
        birth: (100, pid as u64),
        zombie: false,
    }
}

#[test]
fn identity_change_during_signal_validation_permanently_revokes_authority() {
    let api = FakeProcesses::root();
    api.0.borrow_mut().change_at = Some(4);
    let mut tracker = ProcessTracker::with_api(10, api);
    tracker.signal(libc::SIGTERM, deadline());
    tracker.signal(libc::SIGKILL, deadline());
    assert!(tracker.api.0.borrow().signals.is_empty());
    assert!(!tracker.settled());
    assert!(
        tracker
            .reasons()
            .iter()
            .any(|reason| reason == "process_identity_changed")
    );
}

#[test]
fn group_observation_identity_mismatch_never_reacquires_pid_authority() {
    let api = FakeProcesses::root();
    api.0.borrow_mut().change_at = Some(3);
    let mut tracker = ProcessTracker::with_api(10, api);
    tracker.signal(libc::SIGTERM, deadline());
    tracker.signal(libc::SIGKILL, deadline());
    assert!(tracker.api.0.borrow().signals.is_empty());
    assert!(!tracker.settled());
}

#[test]
fn reused_descendant_never_receives_a_signal_after_identity_returns() {
    let api = FakeProcesses::root();
    api.0
        .borrow_mut()
        .identities
        .insert(20, identity(20, 10, 20));
    let mut tracker = ProcessTracker::with_api(10, api);
    tracker.discover();
    tracker
        .api
        .0
        .borrow_mut()
        .identities
        .get_mut(&20)
        .unwrap()
        .birth
        .0 += 1;
    tracker.signal(libc::SIGTERM, deadline());
    tracker
        .api
        .0
        .borrow_mut()
        .identities
        .insert(20, identity(20, 10, 20));
    tracker.signal(libc::SIGKILL, deadline());
    assert!(
        tracker
            .api
            .0
            .borrow()
            .signals
            .iter()
            .all(|(target, _)| *target != 20)
    );
    assert!(!tracker.settled());
}

#[test]
fn observed_reparented_descendant_remains_owned_after_root_disappears() {
    let api = FakeProcesses::root();
    api.0
        .borrow_mut()
        .identities
        .insert(20, identity(20, 10, 20));
    let mut tracker = ProcessTracker::with_api(10, api);
    tracker.discover();
    {
        let mut state = tracker.api.0.borrow_mut();
        state.identities.remove(&10);
        state.identities.get_mut(&20).unwrap().parent = 1;
    }
    tracker.signal(libc::SIGTERM, deadline());
    assert_eq!(tracker.api.0.borrow().signals, vec![(20, libc::SIGTERM)]);
    assert!(!tracker.settled());
    tracker.api.0.borrow_mut().identities.clear();
    assert!(tracker.settled());
}

#[test]
fn unknown_group_member_revokes_group_signals_across_retry() {
    let api = FakeProcesses::root();
    let mut tracker = ProcessTracker::with_api(10, api);
    tracker
        .api
        .0
        .borrow_mut()
        .identities
        .insert(30, identity(30, 1, 10));
    tracker.signal(libc::SIGTERM, deadline());
    tracker.api.0.borrow_mut().identities.remove(&30);
    tracker.signal(libc::SIGKILL, deadline());
    assert!(
        tracker
            .api
            .0
            .borrow()
            .signals
            .iter()
            .all(|(target, _)| *target != -10 && *target != 30)
    );
    assert!(!tracker.settled());
}

#[test]
fn observation_failure_stays_unresolved_after_later_empty_snapshot() {
    let api = FakeProcesses::root();
    let mut tracker = ProcessTracker::with_api(10, api);
    tracker.api.0.borrow_mut().list_failure = true;
    assert!(!tracker.settled());
    {
        let mut state = tracker.api.0.borrow_mut();
        state.list_failure = false;
        state.identities.clear();
    }
    assert!(!tracker.settled());
    tracker.signal(libc::SIGKILL, deadline());
    assert!(tracker.api.0.borrow().signals.is_empty());
}

#[test]
fn identity_limit_blocks_completion_without_signalling_unretained_children() {
    let api = FakeProcesses::root();
    for pid in 20..148 {
        api.0
            .borrow_mut()
            .identities
            .insert(pid, identity(pid, 10, pid as u32));
    }
    let mut tracker = ProcessTracker::with_api(10, api);
    tracker.signal(libc::SIGTERM, deadline());
    assert_eq!(tracker.known.len(), MAX_IDENTITIES);
    assert!(
        !tracker
            .api
            .0
            .borrow()
            .signals
            .iter()
            .any(|(target, _)| *target == 147)
    );
    tracker.api.0.borrow_mut().identities.clear();
    assert!(!tracker.settled());
    assert!(
        tracker
            .reasons()
            .iter()
            .any(|reason| reason == "process_identity_capacity")
    );
}

fn deadline() -> tokio::time::Instant {
    tokio::time::Instant::now() + Duration::from_secs(5)
}

#[test]
fn observation_consumes_shared_budget_and_prevents_late_signals() {
    let api = FakeProcesses::root();
    let start = tokio::time::Instant::now();
    let mut tracker = ProcessTracker::with_api(10, api);
    {
        let mut state = tracker.api.0.borrow_mut();
        state.now = Some(start);
        state.inspect_cost = Duration::from_secs(2);
    }
    let deadline = start + Duration::from_secs(5);
    tracker.signal(libc::SIGTERM, deadline);
    tracker.signal(libc::SIGKILL, deadline);
    assert!(tracker.api.0.borrow().signals.is_empty());
    assert!(!tracker.settled());
    assert!(
        tracker
            .reasons()
            .iter()
            .any(|reason| reason == "cleanup_budget_exhausted")
    );
}

#[test]
fn expired_cleanup_budget_performs_no_process_observation_or_signal() {
    let mut tracker = ProcessTracker::with_api(10, FakeProcesses::root());
    let before = tracker.api.0.borrow().inspections;
    tracker.signal(libc::SIGKILL, tokio::time::Instant::now());
    assert_eq!(tracker.api.0.borrow().inspections, before);
    assert!(tracker.api.0.borrow().signals.is_empty());
    assert!(!tracker.settled());
}
