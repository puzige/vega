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

type Redact = Arc<dyn Fn(&str) -> Result<String, String> + Send + Sync>;
type SafeEnd = Arc<dyn Fn(&str) -> Result<usize, String> + Send + Sync>;

#[derive(Clone)]
pub struct BashSessionRedactor {
    redact: Redact,
    safe_end: SafeEnd,
    max_pending_bytes: usize,
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
        })
    }

    fn buffered(redact: Redact) -> Self {
        Self {
            redact,
            safe_end: Arc::new(|_| Ok(0)),
            max_pending_bytes: MAX_PENDING_BYTES,
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
    output: OutputRing,
    _retained_temp: Option<TempRoot>,
    _retained_child: Option<Child>,
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
        })
    }
}

impl BashSession {
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
            },
        }
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
        self.inner.data().exit_code = Some(status.code().unwrap_or(-1));
    }

    pub(super) fn retain_child(&self, child: Child) {
        self.inner.data()._retained_child = Some(child);
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
        data._retained_temp = retained_temp;
        data.duration_ms = elapsed_ms(data.started_at);
        data.error = attempt.result.as_ref().err().copied();
        data.state = if !attempt.cleanup_safe {
            "outcome_unknown"
        } else {
            match attempt.result {
                Ok(output) => {
                    data.exit_code = Some(output.exit_code);
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
                output: OutputRing::new(),
                _retained_temp: None,
                _retained_child: None,
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
            if attempt.cleanup_safe {
                if let Err(error) = temp_root.cleanup() {
                    attempt.result = Err(error);
                    attempt.cleanup_safe = false;
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
        if inner.data().started_at.is_none() {
            let data = inner.data();
            return Err(if data.cleanup_safe {
                data.error
                    .unwrap_or_else(|| BashError::new(BashErrorCode::SpawnFailed))
            } else {
                BashError::new(BashErrorCode::CleanupFailed)
            });
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
