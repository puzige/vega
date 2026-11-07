use super::*;
use std::collections::{BTreeMap, BTreeSet};

const MAX_IDENTITIES: usize = 128;

#[cfg(test)]
#[path = "session_process_tests.rs"]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Identity {
    pid: i32,
    parent: u32,
    group: u32,
    birth: (u64, u64),
    zombie: bool,
}

pub(super) trait ProcessApi {
    fn now(&self) -> tokio::time::Instant {
        tokio::time::Instant::now()
    }
    fn inspect(&self, pid: i32) -> Result<Option<Identity>, ()>;
    fn list(&self, pid: i32, group: bool) -> Result<Vec<i32>, &'static str>;
    fn signal(&self, target: i32, signal: i32) -> Result<(), ()>;
}

pub(super) struct NativeProcessApi;

impl ProcessApi for NativeProcessApi {
    fn inspect(&self, pid: i32) -> Result<Option<Identity>, ()> {
        inspect(pid)
    }
    fn list(&self, pid: i32, group: bool) -> Result<Vec<i32>, &'static str> {
        list(pid, group)
    }
    fn signal(&self, target: i32, signal: i32) -> Result<(), ()> {
        send_signal(target, signal)
    }
}

pub(super) struct ProcessTracker<A = NativeProcessApi> {
    api: A,
    group: i32,
    known: BTreeMap<i32, Identity>,
    failures: BTreeSet<&'static str>,
    invalid: BTreeSet<i32>,
    group_invalid: bool,
}

impl ProcessTracker {
    pub(super) fn new(pid: u32) -> Self {
        Self::with_api(pid, NativeProcessApi)
    }
}

impl<A: ProcessApi> ProcessTracker<A> {
    fn with_api(pid: u32, api: A) -> Self {
        let mut tracker = Self {
            api,
            group: i32::try_from(pid).unwrap_or(0),
            known: BTreeMap::new(),
            failures: BTreeSet::new(),
            invalid: BTreeSet::new(),
            group_invalid: false,
        };
        match tracker.api.inspect(tracker.group) {
            Ok(Some(identity)) if identity.group == pid => {
                tracker.known.insert(identity.pid, identity);
            }
            _ => {
                tracker.failures.insert("initial_identity_unavailable");
                tracker.group_invalid = true;
            }
        }
        tracker
    }

    fn remember(&mut self, identity: Identity) {
        if let Some(prior) = self.known.get(&identity.pid) {
            if prior.birth != identity.birth {
                self.failures.insert("process_identity_changed");
                self.invalid.insert(identity.pid);
                self.group_invalid |= identity.group == self.group as u32;
            }
        } else if self.known.len() == MAX_IDENTITIES {
            self.failures.insert("process_identity_capacity");
        } else {
            self.known.insert(identity.pid, identity);
        }
    }

    pub(super) fn discover(&mut self) {
        self.discover_until(None);
    }

    fn discover_until(&mut self, deadline: Option<tokio::time::Instant>) {
        let mut visited = BTreeSet::new();
        loop {
            let next = self
                .known
                .values()
                .find(|identity| !visited.contains(&identity.pid))
                .copied();
            let Some(parent) = next else {
                break;
            };
            visited.insert(parent.pid);
            if self.invalid.contains(&parent.pid) {
                continue;
            }
            if !self.observation_allowed(deadline) {
                return;
            }
            match self.api.inspect(parent.pid) {
                Ok(Some(current)) if current.birth == parent.birth => {
                    if !self.observation_allowed(deadline) {
                        return;
                    }
                    match self.api.list(parent.pid, false) {
                        Ok(children) => {
                            for pid in children {
                                if !self.observation_allowed(deadline) {
                                    return;
                                }
                                match self.api.inspect(pid) {
                                    Ok(Some(child)) if child.parent == parent.pid as u32 => {
                                        self.remember(child)
                                    }
                                    Ok(None) => {}
                                    _ => {
                                        self.failures.insert("descendant_observation_failed");
                                    }
                                }
                            }
                        }
                        Err(reason) => {
                            self.failures.insert(reason);
                        }
                    }
                }
                Ok(None) => {}
                _ => {
                    self.failures.insert("process_identity_changed");
                    self.invalid.insert(parent.pid);
                    self.group_invalid |= parent.group == self.group as u32;
                }
            }
        }
        if !self.observation_allowed(deadline) {
            return;
        }
        match self.api.list(self.group, true) {
            Ok(members) => {
                for pid in members {
                    if !self.observation_allowed(deadline) {
                        return;
                    }
                    match self.api.inspect(pid) {
                        Ok(Some(identity))
                            if self
                                .known
                                .get(&pid)
                                .is_some_and(|known| known.birth == identity.birth) => {}
                        Ok(None) => {}
                        _ => {
                            self.failures.insert("group_continuity_lost");
                            self.group_invalid = true;
                            if self.known.contains_key(&pid) {
                                self.invalid.insert(pid);
                            }
                        }
                    }
                }
            }
            Err(reason) => {
                self.failures.insert(reason);
                self.group_invalid = true;
            }
        }
    }

    fn matching(&mut self, identity: Identity, deadline: tokio::time::Instant) -> Option<Identity> {
        if !self.within_budget(deadline) || self.invalid.contains(&identity.pid) {
            return None;
        }
        match self.api.inspect(identity.pid) {
            Ok(Some(current)) if current.birth == identity.birth => Some(current),
            Ok(None) => None,
            _ => {
                self.failures.insert("process_identity_changed");
                self.invalid.insert(identity.pid);
                self.group_invalid |= identity.group == self.group as u32;
                None
            }
        }
    }

    pub(super) fn signal(&mut self, signal: i32, deadline: tokio::time::Instant) {
        if !self.within_budget(deadline) {
            return;
        }
        self.discover_until(Some(deadline));
        if !self.within_budget(deadline) {
            return;
        }
        let members = self.api.list(self.group, true);
        let mut group_verified =
            !self.group_invalid && members.as_ref().is_ok_and(|members| !members.is_empty());
        if let Ok(members) = &members {
            for pid in members {
                if !self.within_budget(deadline) {
                    return;
                }
                let current = self
                    .known
                    .get(pid)
                    .copied()
                    .and_then(|known| self.matching(known, deadline));
                group_verified &= current.is_some_and(|current| current.group == self.group as u32);
            }
        }
        if !self.within_budget(deadline) {
            return;
        }
        if group_verified {
            if self.api.signal(-self.group, signal).is_err() {
                self.failures.insert("group_signal_failed");
            }
        } else if !members.as_ref().is_ok_and(Vec::is_empty) {
            self.failures.insert("group_continuity_lost");
            self.group_invalid = true;
        }
        let known: Vec<_> = self.known.values().copied().collect();
        for identity in known {
            if !self.within_budget(deadline) {
                return;
            }
            if let Some(current) = self.matching(identity, deadline)
                && self.within_budget(deadline)
                && !current.zombie
                && (!group_verified || current.group != self.group as u32)
                && self.api.signal(current.pid, signal).is_err()
            {
                self.failures.insert("descendant_signal_failed");
            }
        }
    }

    fn within_budget(&mut self, deadline: tokio::time::Instant) -> bool {
        if self.api.now() < deadline {
            true
        } else {
            self.failures.insert("cleanup_budget_exhausted");
            false
        }
    }

    fn observation_allowed(&mut self, deadline: Option<tokio::time::Instant>) -> bool {
        deadline.is_none_or(|deadline| self.within_budget(deadline))
    }

    pub(super) fn settled(&mut self, deadline: tokio::time::Instant) -> bool {
        self.discover_until(Some(deadline));
        let identities: Vec<_> = self.known.values().copied().collect();
        let mut gone = true;
        for identity in identities {
            if !self.within_budget(deadline) {
                return false;
            }
            if self
                .matching(identity, deadline)
                .is_some_and(|current| !current.zombie)
            {
                gone = false;
            }
        }
        if !self.within_budget(deadline) {
            return false;
        }
        match self.api.list(self.group, true) {
            Ok(members) if members.is_empty() => {}
            Ok(_) => gone = false,
            Err(reason) => {
                self.failures.insert(reason);
                gone = false;
            }
        }
        self.within_budget(deadline) && gone && self.failures.is_empty()
    }

    pub(super) fn reasons(&self) -> Vec<String> {
        self.failures
            .iter()
            .map(|reason| (*reason).to_owned())
            .collect()
    }
}

pub(super) async fn execute(
    prepared: PreparedBash,
    command: StdCommand,
    cancel: CancellationToken,
    temp_root: &TempRoot,
    writer: SessionWriter,
) -> BashAttempt {
    if cancel.is_cancelled() {
        return BashAttempt::safe(Err(BashError::new(BashErrorCode::Cancelled)));
    }
    let mut command = Command::from(command);
    command.kill_on_drop(true);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => return BashAttempt::safe(Err(BashError::new(BashErrorCode::SpawnFailed))),
    };
    let started = Instant::now();
    let mut tracker = ProcessTracker::new(child.id().unwrap_or(0));
    writer.started(started);
    let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
        let deadline = writer.begin_cleanup("outcome_unknown");
        tracker.signal(libc::SIGKILL, deadline);
        writer.retain_child(child);
        writer.process_cleanup(false, vec!["output_pipes_unavailable".into()]);
        return BashAttempt {
            result: Err(BashError::new(BashErrorCode::OutputFailed)),
            cleanup_safe: false,
        };
    };
    let stop = CancellationToken::new();
    let mut reader = tokio::spawn(read_output(
        stdout,
        stderr,
        stop.clone(),
        Some(writer.clone()),
        temp_root.param().into(),
    ));
    let execution_deadline =
        tokio::time::Instant::now() + Duration::from_millis(prepared.timeout_ms);
    let mut sampling = tokio::time::interval(Duration::from_millis(50));
    sampling.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut status = None;
    let mut reader_result = None;
    let reason = loop {
        tokio::select! {
            biased;
            result = child.wait() => {
                match result { Ok(exit) => { writer.exited(exit); status = Some(exit); }, Err(_) => { tracker.failures.insert("child_wait_failed"); } }
                break if status.is_some() { "exited" } else { "outcome_unknown" };
            }
            _ = cancel.cancelled() => break "cancelled",
            _ = tokio::time::sleep_until(execution_deadline) => break "timed_out",
            result = &mut reader, if reader_result.is_none() => {
                reader_result = Some(join_output(result));
                if reader_result.as_ref().is_some_and(Result::is_err) { break "output_failed"; }
            }
            _ = sampling.tick() => tracker.discover(),
        }
    };
    let deadline = writer.begin_cleanup(reason);
    tracker.discover();
    if status.is_none() || !tracker.settled(deadline) {
        tracker.signal(libc::SIGTERM, deadline);
        let grace = std::cmp::min(deadline, tokio::time::Instant::now() + TERMINATION_GRACE);
        loop {
            if status.is_none() {
                match child.try_wait() {
                    Ok(Some(exit)) => {
                        writer.exited(exit);
                        status = Some(exit);
                    }
                    Ok(None) => {}
                    Err(_) => {
                        tracker.failures.insert("child_wait_failed");
                    }
                }
            }
            if status.is_some() && tracker.settled(deadline) {
                break;
            }
            if tokio::time::Instant::now() >= grace {
                break;
            }
            tokio::time::sleep_until(std::cmp::min(
                grace,
                tokio::time::Instant::now() + Duration::from_millis(10),
            ))
            .await;
        }
        if status.is_none() || !tracker.settled(deadline) {
            tracker.signal(libc::SIGKILL, deadline);
        }
    }
    let process_deadline = std::cmp::min(deadline, tokio::time::Instant::now() + REAP_GRACE);
    loop {
        if status.is_none() {
            match child.try_wait() {
                Ok(Some(exit)) => {
                    writer.exited(exit);
                    status = Some(exit);
                }
                Ok(None) => {}
                Err(_) => {
                    tracker.failures.insert("child_wait_failed");
                }
            }
        }
        if status.is_some() && tracker.settled(deadline) {
            break;
        }
        if tokio::time::Instant::now() >= process_deadline {
            break;
        }
        tokio::time::sleep_until(std::cmp::min(
            process_deadline,
            tokio::time::Instant::now() + Duration::from_millis(10),
        ))
        .await;
    }
    let resolved = status.is_some() && tracker.settled(deadline);
    let mut reasons = tracker.reasons();
    if !resolved {
        reasons.push("process_cleanup_unresolved".into());
    }
    writer.process_cleanup(resolved, reasons);
    let mut reader_closed = true;
    if reader_result.is_none() {
        let drain = std::cmp::min(deadline, tokio::time::Instant::now() + OUTPUT_DRAIN_GRACE);
        reader_result = Some(match tokio::time::timeout_at(drain, &mut reader).await {
            Ok(result) => join_output(result),
            Err(_) => {
                stop.cancel();
                reader_closed = tokio::time::timeout_at(deadline, &mut reader).await.is_ok();
                Err(BashError::new(BashErrorCode::OutputIncomplete))
            }
        });
    }
    let output = reader_result.unwrap_or_else(|| Err(BashError::new(BashErrorCode::OutputFailed)));
    let incomplete = output.is_err();
    writer.reader_closed(incomplete);
    if !reader_closed {
        writer.retain_reader(reader);
    }
    if status.is_none() {
        writer.retain_child(child);
    }
    if !resolved {
        writer.retain_tracker(tracker);
    }
    let result = if !resolved {
        Err(BashError::new(BashErrorCode::ProcessControlFailed))
    } else if incomplete {
        Err(BashError::new(BashErrorCode::OutputIncomplete))
    } else {
        match reason {
            "cancelled" => Err(BashError::new(BashErrorCode::Cancelled)),
            "timed_out" => Err(BashError::new(BashErrorCode::TimedOut)),
            "exited" => match (status, output) {
                (Some(exit), Ok(collected)) => Ok(build_output(exit, started, collected)),
                _ => Err(BashError::new(BashErrorCode::ProcessControlFailed)),
            },
            _ => Err(BashError::new(BashErrorCode::OutputFailed)),
        }
    };
    BashAttempt {
        result,
        cleanup_safe: resolved
            && reader_closed
            && !incomplete
            && tokio::time::Instant::now() < deadline,
    }
}

#[cfg(target_os = "macos")]
fn inspect(pid: i32) -> Result<Option<Identity>, ()> {
    if pid <= 0 {
        return Err(());
    }
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let size = std::mem::size_of::<libc::proc_bsdinfo>();
    // SAFETY: the output buffer is writable for exactly the supplied structure size.
    let read = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size as i32,
        )
    };
    if read == size as i32 {
        // SAFETY: proc_pidinfo returned the complete initialized structure.
        let info = unsafe { info.assume_init() };
        if info.pbi_pid != pid as u32 {
            return Err(());
        }
        Ok(Some(Identity {
            pid,
            parent: info.pbi_ppid,
            group: info.pbi_pgid,
            birth: (info.pbi_start_tvsec, info.pbi_start_tvusec),
            zombie: info.pbi_status == 5,
        }))
    } else if std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
        Ok(None)
    } else {
        Err(())
    }
}

#[cfg(target_os = "macos")]
fn list(pid: i32, group: bool) -> Result<Vec<i32>, &'static str> {
    if pid <= 0 {
        return Err("process_observation_failed");
    }
    let mut pids = [0_i32; MAX_IDENTITIES + 1];
    let size = std::mem::size_of_val(&pids) as i32;
    // SAFETY: errno is thread-local and both APIs receive a valid writable PID buffer.
    let count = unsafe {
        *libc::__error() = 0;
        if group {
            libc::proc_listpgrppids(pid, pids.as_mut_ptr().cast(), size)
        } else {
            libc::proc_listchildpids(pid, pids.as_mut_ptr().cast(), size)
        }
    };
    if count < 0
        || (count == 0
            && std::io::Error::last_os_error()
                .raw_os_error()
                .is_some_and(|error| error != 0 && error != libc::ESRCH))
    {
        return Err("process_observation_failed");
    }
    let count = count as usize;
    if count >= pids.len() {
        return Err("process_identity_capacity");
    }
    Ok(pids[..count]
        .iter()
        .copied()
        .filter(|pid| *pid > 0)
        .collect())
}

#[cfg(target_os = "macos")]
fn send_signal(target: i32, signal: i32) -> Result<(), ()> {
    if target == 0 || target == -1 {
        return Err(());
    }
    // SAFETY: target is a freshly checked PID or a verified operation process group.
    let result = unsafe { libc::kill(target, signal) };
    if result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(())
    }
}

#[cfg(not(target_os = "macos"))]
fn inspect(_: i32) -> Result<Option<Identity>, ()> {
    Err(())
}
#[cfg(not(target_os = "macos"))]
fn list(_: i32, _: bool) -> Result<Vec<i32>, &'static str> {
    Err("process_observation_unsupported")
}
#[cfg(not(target_os = "macos"))]
fn send_signal(_: i32, _: i32) -> Result<(), ()> {
    Err(())
}
