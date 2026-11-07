use std::collections::VecDeque;
use std::fmt;
use std::process::ExitStatus;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use tokio::process::Child;
use tokio_util::sync::CancellationToken;

use super::{
    BashAttempt, BashLaunchPolicy, BashObservation, PreparedBash, PreparedBashScope,
    redact_temp_path,
};
use crate::output::{BashOutputCollector, CollectedBashOutput};
use crate::sandbox::TempRoot;
use crate::{BashError, BashErrorCode, Tools};

const RING_BYTES: usize = 1024 * 1024;
const SNAPSHOT_BYTES: usize = 16 * 1024;
const MAX_PENDING_BYTES: usize = 64 * 1024;

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;

type Redact = Arc<dyn Fn(&str) -> Result<String, String> + Send + Sync>;
type SafeEnd = Arc<dyn Fn(&str) -> Result<usize, String> + Send + Sync>;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BashSessionResource {
    pub path: String,
    pub device: u64,
    pub inode: u64,
}

impl BashSessionResource {
    pub fn recover(&self) -> Result<(), BashError> {
        TempRoot::recover(self)
    }

    pub fn is_closed(&self) -> Result<bool, BashError> {
        match std::fs::symlink_metadata(&self.path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
            Err(_) => Err(BashError::new(BashErrorCode::CleanupFailed)),
            Ok(_) => Ok(false),
        }
    }
}

pub type BashSessionResourceHook = Arc<
    dyn Fn(
            BashSessionResource,
        )
            -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), BashError>> + Send>>
        + Send
        + Sync,
>;

#[derive(Clone)]
pub struct BashSessionRedactor {
    redact: Redact,
    safe_end: SafeEnd,
    max_pending_bytes: usize,
    resource_hook: Option<BashSessionResourceHook>,
}

impl BashSessionRedactor {
    pub fn new(
        redact: Redact,
        safe_end: SafeEnd,
        max_pending_bytes: usize,
    ) -> Result<Self, BashError> {
        if !(4..=MAX_PENDING_BYTES).contains(&max_pending_bytes) {
            return Err(BashError::new(BashErrorCode::InvalidInput));
        }
        Ok(Self {
            redact,
            safe_end,
            max_pending_bytes,
            resource_hook: None,
        })
    }

    pub fn with_resource_hook(mut self, hook: BashSessionResourceHook) -> Self {
        self.resource_hook = Some(hook);
        self
    }

    pub fn buffered(redact: Redact) -> Self {
        Self {
            redact,
            safe_end: Arc::new(|_| Ok(0)),
            max_pending_bytes: MAX_PENDING_BYTES,
            resource_hook: None,
        }
    }
}

impl fmt::Debug for BashSessionRedactor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BashSessionRedactor")
            .field("max_pending_bytes", &self.max_pending_bytes)
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
pub struct BashSessionSnapshot {
    pub state: String,
    pub exit_code: Option<i32>,
    pub output: String,
    pub next_cursor: u64,
    pub retained_from: u64,
    pub output_gap: bool,
    pub truncated: bool,
    pub duration_ms: u64,
    pub error: Option<String>,
    pub cleanup_safe: bool,
    pub command_result: Option<String>,
    pub exit_signal: Option<i32>,
    pub output_incomplete: bool,
    pub resources_closed: bool,
    pub process_resolved: bool,
    pub cleanup_reasons: Vec<String>,
    pub cleanup_remaining_ms: u64,
}

impl fmt::Debug for BashSessionSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BashSessionSnapshot")
            .field("state", &self.state)
            .field("exit_code", &self.exit_code)
            .field("output", &"[REDACTED]")
            .field("next_cursor", &self.next_cursor)
            .field("retained_from", &self.retained_from)
            .field("output_gap", &self.output_gap)
            .field("truncated", &self.truncated)
            .field("duration_ms", &self.duration_ms)
            .field("error", &self.error)
            .field("cleanup_safe", &self.cleanup_safe)
            .finish()
    }
}

#[derive(Clone)]
pub struct BashSession {
    owner: Arc<SessionOwner>,
}

struct SessionOwner {
    inner: Arc<SessionInner>,
}

impl Drop for SessionOwner {
    fn drop(&mut self) {
        self.inner.cancel.cancel();
    }
}

impl fmt::Debug for BashSession {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BashSession")
            .field("terminal", &self.is_terminal())
            .finish_non_exhaustive()
    }
}

struct SessionInner {
    data: Mutex<SessionData>,
    cancel: CancellationToken,
    launched: CancellationToken,
    settled: CancellationToken,
    redactor: BashSessionRedactor,
}

struct SessionData {
    state: &'static str,
    terminal: bool,
    started_at: Option<Instant>,
    duration_ms: u64,
    exit_code: Option<i32>,
    error: Option<BashError>,
    cleanup_safe: bool,
    command_result: Option<String>,
    exit_signal: Option<i32>,
    output_incomplete: bool,
    resources_closed: bool,
    process_resolved: bool,
    cleanup_reasons: Vec<String>,
    cleanup_deadline: Option<tokio::time::Instant>,
    output: OutputRing,
    _retained_temp: Option<TempRoot>,
    _retained_child: Option<Child>,
    retained_reader: Option<tokio::task::JoinHandle<Result<CollectedBashOutput, BashError>>>,
    retained_tracker: Option<super::session_process::ProcessTracker>,
    recovery_busy: bool,
}

struct CleanupRetryResources {
    inner: Arc<SessionInner>,
    armed: bool,
    tracker: Option<super::session_process::ProcessTracker>,
    child: Option<Child>,
    temp: Option<TempRoot>,
    reader: Option<tokio::task::JoinHandle<Result<CollectedBashOutput, BashError>>>,
}

impl Drop for CleanupRetryResources {
    fn drop(&mut self) {
        let mut data = self.inner.data();
        if self.armed {
            data.retained_tracker = self.tracker.take();
            data._retained_child = self.child.take();
            data._retained_temp = self.temp.take();
            data.retained_reader = self.reader.take();
            data.recovery_busy = false;
            data.resources_closed = false;
            data.cleanup_safe = false;
        }
    }
}

impl SessionInner {
    fn data(&self) -> MutexGuard<'_, SessionData> {
        self.data.lock().unwrap_or_else(|error| error.into_inner())
    }

    fn snapshot(&self, cursor: Option<u64>) -> Result<BashSessionSnapshot, BashError> {
        let data = self.data();
        let (output, next_cursor, output_gap) = data.output.read(cursor)?;
        Ok(BashSessionSnapshot {
            state: data.state.into(),
            exit_code: if data.terminal { data.exit_code } else { None },
            output,
            next_cursor,
            retained_from: data.output.retained_from,
            output_gap,
            truncated: data.output.truncated,
            duration_ms: if data.terminal {
                data.duration_ms
            } else {
                elapsed_ms(data.started_at)
            },
            error: data.error.map(|error| error.code().as_str().into()),
            cleanup_safe: data.terminal && data.cleanup_safe,
            command_result: data.command_result.clone(),
            exit_signal: data.exit_signal,
            output_incomplete: data.output_incomplete,
            resources_closed: data.resources_closed,
            process_resolved: data.process_resolved,
            cleanup_reasons: data.cleanup_reasons.clone(),
            cleanup_remaining_ms: data.cleanup_deadline.map_or(0, |deadline| {
                u64::try_from(
                    deadline
                        .saturating_duration_since(tokio::time::Instant::now())
                        .as_millis(),
                )
                .unwrap_or(0)
            }),
        })
    }
}

impl BashSession {
    pub fn snapshot(&self, cursor: Option<u64>) -> Result<BashSessionSnapshot, BashError> {
        self.owner.inner.snapshot(cursor)
    }

    pub async fn observe(
        &self,
        cursor: Option<u64>,
        wait_ms: u64,
    ) -> Result<BashSessionSnapshot, BashError> {
        if wait_ms > 300_000 {
            return Err(BashError::new(BashErrorCode::InvalidInput));
        }
        self.owner.inner.data().output.validate(cursor)?;
        if wait_ms != 0 {
            let _ = tokio::time::timeout(
                Duration::from_millis(wait_ms),
                self.owner.inner.settled.cancelled(),
            )
            .await;
        }
        self.owner.inner.snapshot(cursor)
    }

    pub async fn cancel_and_wait(&self) -> BashSessionSnapshot {
        self.owner.inner.cancel.cancel();
        self.wait_until_settled().await
    }

    pub async fn wait_until_settled(&self) -> BashSessionSnapshot {
        self.owner.inner.settled.cancelled().await;
        match self.owner.inner.snapshot(None) {
            Ok(snapshot) => snapshot,
            Err(_) => BashSessionSnapshot {
                state: "outcome_unknown".into(),
                exit_code: None,
                output: String::new(),
                next_cursor: 0,
                retained_from: 0,
                output_gap: false,
                truncated: true,
                duration_ms: 0,
                error: Some(BashErrorCode::OutputFailed.as_str().into()),
                cleanup_safe: false,
                command_result: None,
                exit_signal: None,
                output_incomplete: true,
                resources_closed: false,
                process_resolved: false,
                cleanup_reasons: vec!["snapshot_unavailable".into()],
                cleanup_remaining_ms: 0,
            },
        }
    }

    pub async fn retry_cleanup(&self) -> Result<BashSessionSnapshot, BashError> {
        let mut resources = {
            let mut data = self.owner.inner.data();
            if !data.terminal || data.recovery_busy {
                return Err(BashError::new(BashErrorCode::ProcessControlFailed));
            }
            data.recovery_busy = true;
            CleanupRetryResources {
                inner: self.owner.inner.clone(),
                armed: true,
                tracker: data.retained_tracker.take(),
                child: data._retained_child.take(),
                temp: data._retained_temp.take(),
                reader: data.retained_reader.take(),
            }
        };
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        if let Some(tracker) = &mut resources.tracker {
            tracker.signal(libc::SIGTERM, deadline);
            let grace = std::cmp::min(
                deadline,
                tokio::time::Instant::now() + super::TERMINATION_GRACE,
            );
            while !tracker.settled(deadline) && tokio::time::Instant::now() < grace {
                tokio::time::sleep_until(std::cmp::min(
                    grace,
                    tokio::time::Instant::now() + Duration::from_millis(10),
                ))
                .await;
            }
            if !tracker.settled(deadline) {
                tracker.signal(libc::SIGKILL, deadline);
            }
        }
        let status = if let Some(child) = &mut resources.child {
            tokio::time::timeout_at(deadline, child.wait())
                .await
                .ok()
                .and_then(Result::ok)
        } else {
            None
        };
        let reader_closed = if let Some(reader) = &mut resources.reader {
            tokio::time::timeout_at(deadline, reader).await.is_ok()
        } else {
            true
        };
        let resolved = resources
            .tracker
            .as_mut()
            .is_none_or(|tracker| tracker.settled(deadline));
        let temp_closed = if resolved && reader_closed {
            resources
                .temp
                .as_ref()
                .is_none_or(|temp| temp.cleanup().is_ok())
        } else {
            false
        };
        let mut data = self.owner.inner.data();
        data.recovery_busy = false;
        if let Some(status) = status {
            use std::os::unix::process::ExitStatusExt;
            data.exit_code = status.code();
            data.exit_signal = status.signal();
        }
        data.process_resolved |= resolved && (resources.child.is_none() || status.is_some());
        if !data.process_resolved {
            data.retained_tracker = resources.tracker.take();
            data._retained_child = resources.child.take();
        }
        if !temp_closed {
            data._retained_temp = resources.temp.take();
        }
        if !reader_closed {
            data.retained_reader = resources.reader.take();
        }
        data.resources_closed = temp_closed
            && reader_closed
            && data._retained_child.is_none()
            && !data
                .cleanup_reasons
                .iter()
                .any(|reason| reason == "credential_cleanup_failed");
        if data.process_resolved
            && data.resources_closed
            && !data.output_incomplete
            && tokio::time::Instant::now() < deadline
        {
            data.cleanup_safe = true;
        }
        data.cleanup_deadline = Some(deadline);
        resources.armed = false;
        drop(data);
        self.owner.inner.snapshot(None)
    }

    pub fn is_terminal(&self) -> bool {
        self.owner.inner.data().terminal
    }
}

#[derive(Clone)]
pub(super) struct SessionWriter {
    inner: Arc<SessionInner>,
}

impl SessionWriter {
    pub(super) fn started(&self, started_at: Instant) {
        self.inner.data().started_at = Some(started_at);
        self.inner.launched.cancel();
    }

    pub(super) fn transition(&self, state: &'static str) {
        let mut data = self.inner.data();
        if !data.terminal {
            data.state = state;
        }
    }

    pub(super) fn exited(&self, status: ExitStatus) {
        use std::os::unix::process::ExitStatusExt;
        let mut data = self.inner.data();
        data.exit_code = status.code();
        data.exit_signal = status.signal();
    }

    pub(super) fn begin_cleanup(&self, result: &str) -> tokio::time::Instant {
        let mut data = self.inner.data();
        let deadline = *data
            .cleanup_deadline
            .get_or_insert_with(|| tokio::time::Instant::now() + Duration::from_secs(5));
        data.command_result.get_or_insert_with(|| result.into());
        data.state = if result == "exited" {
            "finishing"
        } else {
            "stopping"
        };
        deadline
    }

    pub(super) fn process_cleanup(&self, resolved: bool, reasons: Vec<String>) {
        let mut data = self.inner.data();
        data.process_resolved = resolved;
        data.cleanup_reasons.extend(reasons);
    }

    pub(super) fn reader_closed(&self, incomplete: bool) {
        self.inner.data().output_incomplete |= incomplete;
    }

    pub(super) fn cleanup_deadline(&self) -> tokio::time::Instant {
        let mut data = self.inner.data();
        *data
            .cleanup_deadline
            .get_or_insert_with(|| tokio::time::Instant::now() + Duration::from_secs(5))
    }

    pub(super) fn resource_failure(&self, reason: &str) {
        let mut data = self.inner.data();
        data.resources_closed = false;
        data.cleanup_reasons.push(reason.into());
    }

    pub(super) fn retain_child(&self, child: Child) {
        self.inner.data()._retained_child = Some(child);
    }

    pub(super) fn retain_reader(
        &self,
        reader: tokio::task::JoinHandle<Result<CollectedBashOutput, BashError>>,
    ) {
        self.inner.data().retained_reader = Some(reader);
    }

    pub(super) fn retain_tracker(&self, tracker: super::session_process::ProcessTracker) {
        self.inner.data().retained_tracker = Some(tracker);
    }

    fn append(&self, text: &str) -> Result<(), BashError> {
        self.inner.data().output.append(text)
    }

    fn truncate(&self) {
        self.inner.data().output.truncated = true;
    }

    fn finish(&self, attempt: BashAttempt, retained_temp: Option<TempRoot>) {
        let mut data = self.inner.data();
        if data.terminal {
            return;
        }
        data.cleanup_safe = attempt.cleanup_safe;
        if data.started_at.is_none() && data._retained_child.is_none() {
            data.process_resolved = true;
        }
        if retained_temp.is_none()
            && !data
                .cleanup_reasons
                .iter()
                .any(|reason| reason == "credential_cleanup_failed")
        {
            data.resources_closed =
                data._retained_child.is_none() && data.retained_reader.is_none();
        }
        data._retained_temp = retained_temp;
        data.duration_ms = elapsed_ms(data.started_at);
        data.error = attempt.result.as_ref().err().copied();
        data.state = if !attempt.cleanup_safe {
            "outcome_unknown"
        } else {
            match attempt.result {
                Ok(output) => {
                    if data.exit_signal.is_none() {
                        data.exit_code = Some(output.exit_code);
                    }
                    if output.exit_code == 0 {
                        "succeeded"
                    } else {
                        "failed"
                    }
                }
                Err(error) => match error.code() {
                    BashErrorCode::Cancelled => "cancelled",
                    BashErrorCode::TimedOut => "timed_out",
                    _ => "failed",
                },
            }
        };
        data.terminal = true;
        drop(data);
        self.inner.settled.cancel();
    }
}

struct WorkerGuard(SessionWriter);

impl Drop for WorkerGuard {
    fn drop(&mut self) {
        if !self.0.inner.data().terminal {
            self.0.inner.cancel.cancel();
            self.0.finish(
                BashAttempt {
                    result: Err(BashError::new(BashErrorCode::ProcessControlFailed)),
                    cleanup_safe: false,
                },
                None,
            );
        }
    }
}

struct LaunchGuard {
    cancel: CancellationToken,
    armed: bool,
}

impl Drop for LaunchGuard {
    fn drop(&mut self) {
        if self.armed {
            self.cancel.cancel();
        }
    }
}

impl Tools {
    pub async fn start_bash_session(
        &self,
        prepared: PreparedBash,
        policy: BashLaunchPolicy,
        scope: Option<PreparedBashScope>,
        cancel: CancellationToken,
        redact: Redact,
    ) -> Result<BashSession, BashError> {
        self.start_bash_session_with_redactor(
            prepared,
            policy,
            scope,
            cancel,
            BashSessionRedactor::buffered(redact),
        )
        .await
    }

    pub async fn start_bash_session_with_redactor(
        &self,
        prepared: PreparedBash,
        policy: BashLaunchPolicy,
        scope: Option<PreparedBashScope>,
        cancel: CancellationToken,
        redactor: BashSessionRedactor,
    ) -> Result<BashSession, BashError> {
        self.validate_bash_launch(&prepared, &cancel)?;
        if policy == BashLaunchPolicy::FullAccess
            || prepared
                .requested_scope()
                .is_some_and(|scope| !scope.write_roots().is_empty())
            || scope
                .as_ref()
                .is_some_and(|scope| !scope.directories().is_empty())
        {
            return Err(BashError::new(BashErrorCode::ScopeDenied));
        }
        if let Some(scope) = &scope {
            scope.validate(&prepared, policy)?;
        } else if prepared.requested_scope().is_some() {
            return Err(BashError::new(BashErrorCode::ScopeMismatch));
        }
        let cancel = cancel.child_token();
        let inner = Arc::new(SessionInner {
            data: Mutex::new(SessionData {
                state: "running",
                terminal: false,
                started_at: None,
                duration_ms: 0,
                exit_code: None,
                error: None,
                cleanup_safe: false,
                command_result: None,
                exit_signal: None,
                output_incomplete: false,
                resources_closed: false,
                process_resolved: false,
                cleanup_reasons: Vec::new(),
                cleanup_deadline: None,
                output: OutputRing::new(),
                _retained_temp: None,
                _retained_child: None,
                retained_reader: None,
                retained_tracker: None,
                recovery_busy: false,
            }),
            cancel: cancel.clone(),
            launched: CancellationToken::new(),
            settled: CancellationToken::new(),
            redactor,
        });
        let mut launch_guard = LaunchGuard {
            cancel: cancel.clone(),
            armed: true,
        };
        let writer = SessionWriter {
            inner: inner.clone(),
        };
        let tools = self.clone();
        tokio::spawn(async move {
            let _guard = WorkerGuard(writer.clone());
            let temp_root = match TempRoot::create() {
                Ok(temp_root) => temp_root,
                Err(error) => {
                    writer.finish(BashAttempt::safe(Err(error)), None);
                    return;
                }
            };
            if let Some(hook) = &writer.inner.redactor.resource_hook
                && hook(temp_root.recovery_identity()).await.is_err()
            {
                let closed = temp_root.cleanup().is_ok();
                writer.finish(
                    BashAttempt {
                        result: Err(BashError::new(BashErrorCode::CleanupFailed)),
                        cleanup_safe: closed,
                    },
                    (!closed).then_some(temp_root),
                );
                return;
            }
            let mut attempt = tools
                .execute_bash_with_temp(
                    prepared,
                    policy,
                    scope,
                    cancel,
                    &temp_root,
                    BashObservation {
                        started_at: None,
                        session: Some(writer.clone()),
                    },
                )
                .await;
            let resources_settled = {
                let data = writer.inner.data();
                data.process_resolved && data.retained_reader.is_none()
            };
            if resources_settled || attempt.cleanup_safe {
                if let Err(error) = temp_root.cleanup() {
                    attempt.result = Err(error);
                    attempt.cleanup_safe = false;
                    writer.resource_failure("temp_cleanup_failed");
                    writer.finish(attempt, Some(temp_root));
                } else {
                    writer.finish(attempt, None);
                }
            } else {
                writer.finish(attempt, Some(temp_root));
            }
        });
        tokio::select! {
            _ = inner.launched.cancelled() => {},
            _ = inner.settled.cancelled() => {},
        }
        launch_guard.armed = false;
        Ok(BashSession {
            owner: Arc::new(SessionOwner { inner }),
        })
    }
}

fn elapsed_ms(started_at: Option<Instant>) -> u64 {
    started_at.map_or(0, |started| {
        u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
    })
}

struct OutputRing {
    bytes: VecDeque<u8>,
    retained_from: u64,
    end: u64,
    truncated: bool,
}

impl OutputRing {
    fn new() -> Self {
        Self {
            bytes: VecDeque::with_capacity(RING_BYTES),
            retained_from: 0,
            end: 0,
            truncated: false,
        }
    }

    fn append(&mut self, text: &str) -> Result<(), BashError> {
        let end = self
            .end
            .checked_add(text.len() as u64)
            .ok_or_else(|| BashError::new(BashErrorCode::OutputFailed))?;
        for byte in text.bytes() {
            if self.bytes.len() == RING_BYTES {
                self.bytes.pop_front();
                self.retained_from += 1;
                self.truncated = true;
            }
            self.bytes.push_back(byte);
        }
        while self.bytes.front().is_some_and(|byte| continuation(*byte)) {
            self.bytes.pop_front();
            self.retained_from += 1;
        }
        self.end = end;
        Ok(())
    }

    fn validate(&self, cursor: Option<u64>) -> Result<(), BashError> {
        let cursor = cursor.unwrap_or(0);
        if cursor > self.end {
            return Err(BashError::new(BashErrorCode::InvalidInput));
        }
        if cursor >= self.retained_from
            && self
                .bytes
                .get((cursor - self.retained_from) as usize)
                .is_some_and(|byte| continuation(*byte))
        {
            return Err(BashError::new(BashErrorCode::InvalidInput));
        }
        Ok(())
    }

    fn read(&self, cursor: Option<u64>) -> Result<(String, u64, bool), BashError> {
        self.validate(cursor)?;
        let cursor = cursor.unwrap_or(0);
        let start = cursor.max(self.retained_from);
        let offset = (start - self.retained_from) as usize;
        let mut count = SNAPSHOT_BYTES.min(self.bytes.len() - offset);
        while self
            .bytes
            .get(offset + count)
            .is_some_and(|byte| continuation(*byte))
        {
            count -= 1;
        }
        let bytes = self
            .bytes
            .iter()
            .skip(offset)
            .take(count)
            .copied()
            .collect();
        let text =
            String::from_utf8(bytes).map_err(|_| BashError::new(BashErrorCode::OutputFailed))?;
        Ok((text, start + count as u64, cursor < self.retained_from))
    }
}

fn continuation(byte: u8) -> bool {
    byte & 0xc0 == 0x80
}

pub(super) enum OutputCollector {
    Blocking(Box<BashOutputCollector>),
    Session(SessionOutput),
}

impl OutputCollector {
    pub(super) fn new(writer: Option<SessionWriter>, temp_path: String) -> Self {
        match writer {
            Some(writer) => Self::Session(SessionOutput {
                writer,
                pending: PendingStream::new(),
                closed: [false, false],
                temp_path,
                error: None,
            }),
            None => Self::Blocking(Box::new(BashOutputCollector::new())),
        }
    }

    pub(super) fn push(&mut self, stream: usize, bytes: &[u8], eof: bool) -> Result<(), BashError> {
        match self {
            Self::Blocking(collector) => collector.push(bytes),
            Self::Session(output) => output.push(stream, bytes, eof),
        }
        Ok(())
    }

    pub(super) fn finish(self) -> Result<CollectedBashOutput, BashError> {
        match self {
            Self::Blocking(collector) => Ok((*collector).finish()),
            Self::Session(output) => {
                if !output.pending.bytes.is_empty() {
                    output.writer.truncate();
                }
                if let Some(error) = output.error {
                    Err(error)
                } else {
                    Ok(CollectedBashOutput {
                        text: String::new(),
                        truncated: output.writer.inner.data().output.truncated,
                    })
                }
            }
        }
    }
}

struct PendingStream {
    bytes: Vec<u8>,
    suppressed: bool,
}

impl PendingStream {
    fn new() -> Self {
        Self {
            bytes: Vec::new(),
            suppressed: false,
        }
    }
}

pub(super) struct SessionOutput {
    writer: SessionWriter,
    pending: PendingStream,
    closed: [bool; 2],
    temp_path: String,
    error: Option<BashError>,
}

impl SessionOutput {
    fn push(&mut self, stream: usize, bytes: &[u8], eof: bool) {
        if self.error.is_some() || self.pending.suppressed {
            return;
        }
        self.closed[stream] = eof;
        let eof = self.closed.iter().all(|closed| *closed);
        let redactor = &self.writer.inner.redactor;
        let pending = &mut self.pending;
        let available = redactor
            .max_pending_bytes
            .saturating_sub(pending.bytes.len());
        if bytes.len() > available {
            pending.bytes.clear();
            pending.suppressed = true;
            self.writer.truncate();
            return;
        }
        pending.bytes.extend_from_slice(bytes);
        let valid_end = match std::str::from_utf8(&pending.bytes) {
            Ok(_) => pending.bytes.len(),
            Err(error) if error.error_len().is_none() && !eof => error.valid_up_to(),
            Err(_) => {
                let text = String::from_utf8_lossy(&pending.bytes).into_owned();
                pending.bytes = text.into_bytes();
                pending.bytes.len()
            }
        };
        let text = match std::str::from_utf8(&pending.bytes[..valid_end]) {
            Ok(text) => text,
            Err(_) => {
                self.error = Some(BashError::new(BashErrorCode::OutputFailed));
                return;
            }
        };
        let safe_end = if eof {
            text.len()
        } else {
            match stream_safe_end(text, redactor, &self.temp_path) {
                Ok(end) => end,
                Err(error) => {
                    pending.bytes.clear();
                    self.error = Some(error);
                    return;
                }
            }
        };
        if safe_end == 0 {
            return;
        }
        match (redactor.redact)(&text[..safe_end]) {
            Ok(text) if text.len() <= RING_BYTES => {
                let mut text = text;
                redact_temp_path(&mut text, &self.temp_path);
                if let Err(error) = self.writer.append(&text) {
                    self.error = Some(error);
                }
            }
            _ => self.error = Some(BashError::new(BashErrorCode::OutputFailed)),
        }
        pending.bytes.drain(..safe_end);
    }
}

fn stream_safe_end(
    text: &str,
    redactor: &BashSessionRedactor,
    path: &str,
) -> Result<usize, BashError> {
    let mut candidate = text.len();
    loop {
        let prefix = &text[..candidate];
        let end =
            (redactor.safe_end)(prefix).map_err(|_| BashError::new(BashErrorCode::OutputFailed))?;
        if end > candidate || !text.is_char_boundary(end) {
            return Err(BashError::new(BashErrorCode::OutputFailed));
        }
        let end = end.min(temp_safe_end(prefix, path));
        if end == candidate {
            return Ok(end);
        }
        candidate = end;
    }
}

fn temp_safe_end(text: &str, path: &str) -> usize {
    text.char_indices()
        .filter(|(start, _)| *start >= text.len().saturating_sub(path.len()))
        .find_map(|(start, _)| path.starts_with(&text[start..]).then_some(start))
        .unwrap_or(text.len())
}
