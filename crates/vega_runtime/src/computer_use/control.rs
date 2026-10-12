use super::{Capability, ComputerUseError, DesktopTarget, Observation, Target};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

static OWNER: OnceLock<Mutex<Option<String>>> = OnceLock::new();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlPhase {
    Disconnected,
    Connecting,
    AwaitingApproval,
    Running,
    Pausing,
    Paused,
    Stopping,
    Stopped,
    Failed,
}

#[derive(Clone)]
pub struct ControlSnapshot {
    pub phase: ControlPhase,
    pub capability: Option<Capability>,
    pub epoch: u64,
    pub target_label: String,
    pub browser_target: Option<Target>,
    pub error: Option<ComputerUseError>,
    pub observations: Vec<Observation>,
}

struct State {
    phase: ControlPhase,
    capability: Option<Capability>,
    epoch: u64,
    windows: Vec<DesktopTarget>,
    directory_started: Option<Instant>,
    next_cursor: Option<(String, usize)>,
    browser_target: Option<Target>,
    desktop: Option<Target>,
    browser: Option<Target>,
    target_label: String,
    grant_started: [Option<Instant>; 2],
    last_used: [Option<Instant>; 2],
    error: Option<ComputerUseError>,
    action_cancel: CancellationToken,
    observations: VecDeque<Observation>,
}

struct Inner {
    run_id: String,
    state: Mutex<State>,
    changed: watch::Sender<u64>,
}

#[derive(Clone)]
pub struct ComputerUseControl(Arc<Inner>);

impl std::fmt::Debug for ComputerUseControl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ComputerUseControl").finish_non_exhaustive()
    }
}

impl ComputerUseControl {
    pub fn new() -> Self {
        let (changed, _) = watch::channel(0);
        Self(Arc::new(Inner {
            run_id: ulid::Ulid::generate().to_string(),
            state: Mutex::new(State {
                phase: ControlPhase::Disconnected,
                capability: None,
                epoch: 1,
                windows: Vec::new(),
                directory_started: None,
                next_cursor: None,
                browser_target: None,
                desktop: None,
                browser: None,
                target_label: String::new(),
                grant_started: [None; 2],
                last_used: [None; 2],
                error: None,
                action_cancel: CancellationToken::new(),
                observations: VecDeque::new(),
            }),
            changed,
        }))
    }

    pub(crate) fn run_id(&self) -> &str {
        &self.0.run_id
    }

    pub fn status(&self) -> ControlSnapshot {
        self.snapshot_with_media(false)
    }

    pub fn snapshot(&self) -> ControlSnapshot {
        self.snapshot_with_media(true)
    }

    fn snapshot_with_media(&self, include_media: bool) -> ControlSnapshot {
        let state = self
            .0
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        ControlSnapshot {
            phase: state.phase,
            capability: state.capability,
            epoch: state.epoch,
            target_label: state.target_label.clone(),
            browser_target: state.browser_target.clone(),
            error: state.error,
            observations: if include_media {
                state.observations.iter().cloned().collect()
            } else {
                Vec::new()
            },
        }
    }

    pub fn pause(&self) {
        self.revoke(ControlPhase::Pausing);
    }

    pub fn stop(&self) {
        self.revoke(ControlPhase::Stopping);
    }

    fn revoke(&self, phase: ControlPhase) {
        if let Ok(mut state) = self.0.state.lock() {
            if matches!(state.phase, ControlPhase::Stopped | ControlPhase::Stopping) {
                return;
            }
            state.epoch = state.epoch.saturating_add(1);
            state.action_cancel.cancel();
            state.observations.clear();
            state.windows.clear();
            state.directory_started = None;
            state.next_cursor = None;
            state.browser_target = None;
            state.phase = phase;
        }
        self.notify();
    }

    pub fn resume(&self, expected_epoch: u64) -> Result<(), ComputerUseError> {
        let mut state = self
            .0
            .state
            .lock()
            .map_err(|_| ComputerUseError::Unavailable)?;
        if state.phase != ControlPhase::Paused || state.epoch != expected_epoch {
            return Err(ComputerUseError::NotReady);
        }
        state.epoch = state.epoch.saturating_add(1);
        state.action_cancel = CancellationToken::new();
        state.observations.clear();
        state.phase = ControlPhase::Disconnected;
        state.error = None;
        drop(state);
        self.notify();
        Ok(())
    }

    pub(crate) fn select_window(
        &self,
        expected_epoch: u64,
        target_id: &str,
    ) -> Result<(), ComputerUseError> {
        let mut state = self
            .0
            .state
            .lock()
            .map_err(|_| ComputerUseError::Unavailable)?;
        if state.epoch != expected_epoch
            || state.action_cancel.is_cancelled()
            || state.phase != ControlPhase::Running
        {
            return Err(ComputerUseError::NotReady);
        }
        if state
            .directory_started
            .is_none_or(|started| started.elapsed().as_secs() >= 60)
        {
            return Err(ComputerUseError::StaleObservation);
        }
        let window = state
            .windows
            .iter()
            .find(|window| window.selection_id == target_id)
            .cloned()
            .ok_or(ComputerUseError::StaleObservation)?;
        state
            .observations
            .retain(|item| item.capability != Capability::Desktop);
        state.target_label = window.label();
        state.desktop = Some(Target::Desktop(window));
        state.capability = Some(Capability::Desktop);
        drop(state);
        self.notify();
        Ok(())
    }

    pub(crate) fn catalog_page(
        &self,
        epoch: u64,
        cursor: Option<&str>,
    ) -> Result<serde_json::Value, ComputerUseError> {
        let mut state = self
            .0
            .state
            .lock()
            .map_err(|_| ComputerUseError::Unavailable)?;
        if state.epoch != epoch
            || state.action_cancel.is_cancelled()
            || state.phase != ControlPhase::Running
        {
            return Err(ComputerUseError::Cancelled);
        }
        if state
            .directory_started
            .is_none_or(|started| started.elapsed().as_secs() >= 60)
        {
            return Err(ComputerUseError::StaleObservation);
        }
        let start = match cursor {
            None => 0,
            Some(cursor) => state
                .next_cursor
                .as_ref()
                .filter(|(expected, _)| expected == cursor)
                .map(|(_, offset)| *offset)
                .ok_or(ComputerUseError::StaleObservation)?,
        };
        let mut page = Vec::new();
        let mut bytes = 0;
        for window in state.windows.iter().skip(start).take(200) {
            let entry = serde_json::json!({"target_id":window.selection_id,"application":window.application,"title":window.title,"pid":window.pid,"window_id":window.window_id});
            bytes += entry.to_string().len();
            if bytes > 60 * 1024 {
                break;
            }
            page.push(entry);
        }
        let end = start + page.len();
        state.next_cursor =
            (end < state.windows.len()).then(|| (ulid::Ulid::generate().to_string(), end));
        let next = state.next_cursor.as_ref().map(|(cursor, _)| cursor.clone());
        state
            .observations
            .retain(|item| item.capability != Capability::Desktop);
        state.desktop = Some(Target::DesktopCatalog);
        state.target_label = Target::DesktopCatalog.label();
        let result = serde_json::json!({"windows":page,"next_cursor":next,"instruction":"Use ComputerObserve with target_id from this directory and cursor:null before any ComputerAct. A directory observation cannot authorize an action. Refresh with target_id:null,cursor:null after stale errors."});
        drop(state);
        self.notify();
        Ok(result)
    }

    pub fn approve_browser(
        &self,
        expected_epoch: u64,
        expected_target: &Target,
    ) -> Result<(), ComputerUseError> {
        let mut state = self
            .0
            .state
            .lock()
            .map_err(|_| ComputerUseError::Unavailable)?;
        if state.phase != ControlPhase::AwaitingApproval
            || state.capability != Some(Capability::Browser)
            || state.epoch != expected_epoch
            || state.browser_target.as_ref() != Some(expected_target)
        {
            return Err(ComputerUseError::NotReady);
        }
        let target = state
            .browser_target
            .take()
            .ok_or(ComputerUseError::TargetChanged)?;
        state.target_label = target.label();
        state.browser = Some(target);
        state.grant_started[1] = Some(Instant::now());
        state.last_used[1] = state.grant_started[1];
        state.phase = ControlPhase::Running;
        drop(state);
        self.notify();
        Ok(())
    }

    pub(crate) fn subscribe(&self) -> watch::Receiver<u64> {
        self.0.changed.subscribe()
    }

    fn notify(&self) {
        self.0
            .changed
            .send_modify(|value| *value = value.wrapping_add(1));
    }

    pub(crate) fn begin(&self, capability: Capability) -> Result<u64, ComputerUseError> {
        let mut owner = OWNER
            .get_or_init(|| Mutex::new(None))
            .lock()
            .map_err(|_| ComputerUseError::Unavailable)?;
        if owner.as_ref().is_some_and(|id| id != &self.0.run_id) {
            return Err(ComputerUseError::Busy);
        }
        let mut state = self
            .0
            .state
            .lock()
            .map_err(|_| ComputerUseError::Unavailable)?;
        if matches!(
            state.phase,
            ControlPhase::Pausing
                | ControlPhase::Paused
                | ControlPhase::Stopping
                | ControlPhase::Stopped
        ) {
            return Err(ComputerUseError::NotReady);
        }
        *owner = Some(self.0.run_id.clone());
        state.capability = Some(capability);
        state.phase = ControlPhase::Connecting;
        state.error = None;
        let epoch = state.epoch;
        drop(state);
        drop(owner);
        self.notify();
        Ok(epoch)
    }

    pub(crate) fn set_windows(
        &self,
        epoch: u64,
        windows: Vec<DesktopTarget>,
    ) -> Result<(), ComputerUseError> {
        self.check_epoch(epoch)?;
        let mut state = self
            .0
            .state
            .lock()
            .map_err(|_| ComputerUseError::Unavailable)?;
        if state.epoch != epoch || state.action_cancel.is_cancelled() {
            return Err(ComputerUseError::Cancelled);
        }
        state.windows = windows;
        state.directory_started = Some(Instant::now());
        state.next_cursor = None;
        state
            .observations
            .retain(|item| item.capability != Capability::Desktop);
        state.desktop = Some(Target::DesktopCatalog);
        state.target_label = Target::DesktopCatalog.label();
        state.capability = Some(Capability::Desktop);
        state.phase = ControlPhase::Running;
        drop(state);
        self.notify();
        Ok(())
    }

    pub(crate) fn offer_browser(&self, epoch: u64, target: Target) -> Result<(), ComputerUseError> {
        self.check_epoch(epoch)?;
        let mut state = self
            .0
            .state
            .lock()
            .map_err(|_| ComputerUseError::Unavailable)?;
        if state.epoch != epoch || state.action_cancel.is_cancelled() {
            return Err(ComputerUseError::Cancelled);
        }
        state.browser_target = Some(target);
        state.capability = Some(Capability::Browser);
        state.phase = ControlPhase::AwaitingApproval;
        drop(state);
        self.notify();
        Ok(())
    }

    pub(crate) fn connected(
        &self,
        epoch: u64,
        capability: Capability,
        target: Target,
    ) -> Result<(), ComputerUseError> {
        let mut state = self
            .0
            .state
            .lock()
            .map_err(|_| ComputerUseError::Unavailable)?;
        if state.epoch != epoch || state.action_cancel.is_cancelled() {
            return Err(ComputerUseError::Cancelled);
        }
        state.target_label = target.label();
        match capability {
            Capability::Desktop => state.desktop = Some(target),
            Capability::Browser => state.browser = Some(target),
        }
        state.capability = Some(capability);
        state.phase = ControlPhase::Running;
        drop(state);
        self.notify();
        Ok(())
    }

    pub(crate) fn target(
        &self,
        capability: Capability,
    ) -> Result<Option<Target>, ComputerUseError> {
        let state = self
            .0
            .state
            .lock()
            .map_err(|_| ComputerUseError::Unavailable)?;
        let index = usize::from(capability == Capability::Browser);
        if capability == Capability::Browser
            && (state.grant_started[index]
                .is_some_and(|started| started.elapsed().as_secs() >= 7200)
                || state.last_used[index].is_some_and(|used| used.elapsed().as_secs() >= 900))
        {
            return Err(ComputerUseError::GrantExpired);
        }
        Ok(match capability {
            Capability::Desktop => state.desktop.clone(),
            Capability::Browser => state.browser.clone(),
        })
    }

    pub(crate) fn action_token(&self, epoch: u64) -> Result<CancellationToken, ComputerUseError> {
        self.check_epoch(epoch)?;
        let state = self
            .0
            .state
            .lock()
            .map_err(|_| ComputerUseError::Unavailable)?;
        if state.epoch != epoch || state.action_cancel.is_cancelled() {
            return Err(ComputerUseError::Cancelled);
        }
        Ok(state.action_cancel.clone())
    }

    pub(crate) fn check_epoch(&self, epoch: u64) -> Result<(), ComputerUseError> {
        let state = self
            .0
            .state
            .lock()
            .map_err(|_| ComputerUseError::Unavailable)?;
        if state.epoch != epoch
            || state.action_cancel.is_cancelled()
            || matches!(
                state.phase,
                ControlPhase::Pausing
                    | ControlPhase::Paused
                    | ControlPhase::Stopping
                    | ControlPhase::Stopped
            )
        {
            return Err(ComputerUseError::Cancelled);
        }
        Ok(())
    }

    pub(crate) fn publish(&self, observation: Observation) -> Result<(), ComputerUseError> {
        let mut state = self
            .0
            .state
            .lock()
            .map_err(|_| ComputerUseError::Unavailable)?;
        if state.epoch != observation.epoch
            || state.action_cancel.is_cancelled()
            || state.phase != ControlPhase::Running
        {
            return Err(ComputerUseError::Cancelled);
        }
        let target = match observation.capability {
            Capability::Desktop => &state.desktop,
            Capability::Browser => &state.browser,
        };
        if target.as_ref() != Some(&observation.target) {
            return Err(ComputerUseError::StaleObservation);
        }
        state.last_used[usize::from(observation.capability == Capability::Browser)] =
            Some(Instant::now());
        state
            .observations
            .retain(|item| item.capability != observation.capability);
        state.observations.push_back(observation);
        while state.observations.len() > 2
            || state
                .observations
                .iter()
                .map(Observation::image_bytes)
                .sum::<usize>()
                > 8 * 1024 * 1024
        {
            state.observations.pop_front();
        }
        drop(state);
        self.notify();
        Ok(())
    }

    pub(crate) fn observation(
        &self,
        capability: Capability,
        id: &str,
    ) -> Result<Observation, ComputerUseError> {
        let state = self
            .0
            .state
            .lock()
            .map_err(|_| ComputerUseError::Unavailable)?;
        if state.phase != ControlPhase::Running || state.action_cancel.is_cancelled() {
            return Err(ComputerUseError::NotReady);
        }
        state
            .observations
            .iter()
            .find(|observation| {
                observation.capability == capability
                    && observation.id == id
                    && observation.epoch == state.epoch
            })
            .cloned()
            .ok_or(ComputerUseError::StaleObservation)
    }

    pub(crate) fn discard_observations(&self) {
        if let Ok(mut state) = self.0.state.lock() {
            state.observations.clear();
            state.windows.clear();
            state.directory_started = None;
            state.next_cursor = None;
        }
        self.notify();
    }

    pub(crate) fn finish_cleanup(&self) {
        let mut release = false;
        if let Ok(mut state) = self.0.state.lock() {
            if !matches!(
                state.phase,
                ControlPhase::Pausing | ControlPhase::Stopping | ControlPhase::Failed
            ) {
                return;
            }
            state.observations.clear();
            state.windows.clear();
            state.directory_started = None;
            state.next_cursor = None;
            if state.phase != ControlPhase::Pausing {
                state.desktop = None;
                state.browser = None;
                state.grant_started = [None; 2];
                state.last_used = [None; 2];
            }
            if state.phase == ControlPhase::Pausing {
                state.phase = ControlPhase::Paused;
            } else if state.phase == ControlPhase::Stopping {
                state.phase = ControlPhase::Stopped;
                release = true;
            } else if state.phase == ControlPhase::Failed {
                release = true;
            }
        }
        if release {
            self.release();
        }
        self.notify();
    }

    pub(crate) fn fail(&self, expected_epoch: u64, error: ComputerUseError) {
        if let Ok(mut state) = self.0.state.lock() {
            if state.epoch != expected_epoch {
                return;
            }
            if matches!(
                state.phase,
                ControlPhase::Pausing
                    | ControlPhase::Paused
                    | ControlPhase::Stopping
                    | ControlPhase::Stopped
            ) {
                return;
            }
            state.epoch = state.epoch.saturating_add(1);
            state.action_cancel.cancel();
            state.observations.clear();
            state.windows.clear();
            state.directory_started = None;
            state.next_cursor = None;
            state.desktop = None;
            state.browser = None;
            state.phase = ControlPhase::Failed;
            state.error = Some(error);
        }
        self.notify();
    }

    pub(crate) fn release(&self) {
        if let Ok(mut owner) = OWNER.get_or_init(|| Mutex::new(None)).lock()
            && owner.as_ref() == Some(&self.0.run_id)
        {
            *owner = None;
        }
    }
}

impl Default for ComputerUseControl {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Ok(state) = self.state.lock() {
            state.action_cancel.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn issue75_desktop_access_directory_expiry_size_and_failure_cleanup() {
        let control = ComputerUseControl::new();
        let epoch = control.begin(Capability::Desktop).unwrap();
        let windows = (0..201)
            .map(|index| DesktopTarget {
                selection_id: format!("target-{index}"),
                application: "应".repeat(100),
                bundle_id: String::new(),
                pid: 1,
                window_id: index,
                title: "窗".repeat(200),
            })
            .collect();
        control.set_windows(epoch, windows).unwrap();
        let mut cursor = None;
        let mut count = 0;
        loop {
            let page = control.catalog_page(epoch, cursor.as_deref()).unwrap();
            assert!(page.to_string().len() <= 64 * 1024);
            count += page["windows"].as_array().unwrap().len();
            cursor = page["next_cursor"].as_str().map(str::to_owned);
            if cursor.is_none() {
                break;
            }
        }
        assert_eq!(count, 201);
        control.0.state.lock().unwrap().directory_started =
            Some(Instant::now() - std::time::Duration::from_secs(61));
        assert_eq!(
            control.select_window(epoch, "target-200"),
            Err(ComputerUseError::StaleObservation)
        );
        assert_eq!(
            control.catalog_page(epoch, None),
            Err(ComputerUseError::StaleObservation)
        );
        assert_eq!(control.status().phase, ControlPhase::Running);
        control.fail(epoch, ComputerUseError::Permissions);
        {
            let state = control.0.state.lock().unwrap();
            assert!(state.windows.is_empty());
            assert!(state.directory_started.is_none());
            assert!(state.next_cursor.is_none());
        }
        control.finish_cleanup();
    }

    #[test]
    fn issue75_desktop_access_has_no_window_grant_expiry() {
        let control = ComputerUseControl::new();
        let epoch = control.begin(Capability::Desktop).unwrap();
        let target = DesktopTarget {
            selection_id: "choice".into(),
            application: "Fixture".into(),
            bundle_id: "fixture".into(),
            pid: 1,
            window_id: 2,
            title: "Window".into(),
        };
        control.set_windows(epoch, vec![target]).unwrap();
        control.select_window(epoch, "choice").unwrap();
        control.0.state.lock().unwrap().grant_started[0] =
            Some(Instant::now() - std::time::Duration::from_secs(7201));
        let browser = Target::Browser {
            connection: "fixture".into(),
            index: 0,
            title: "Page".into(),
            url: "https://example.test/".into(),
        };
        control.offer_browser(epoch, browser.clone()).unwrap();
        control.approve_browser(epoch, &browser).unwrap();
        assert!(matches!(
            control.target(Capability::Desktop).unwrap(),
            Some(Target::Desktop(_))
        ));
        assert_eq!(control.target(Capability::Browser).unwrap(), Some(browser));
        control.0.state.lock().unwrap().last_used[1] =
            Some(Instant::now() - std::time::Duration::from_secs(901));
        assert_eq!(
            control.target(Capability::Browser),
            Err(ComputerUseError::GrantExpired)
        );
        control.stop();
        control.finish_cleanup();
    }
}
