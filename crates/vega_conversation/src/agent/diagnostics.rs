use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};
use std::time::Instant;

use vega_runtime::{
    RuntimeDiagnosticAttempt, RuntimeDiagnosticFailure, RuntimeDiagnosticPhase,
    RuntimeDiagnosticState, StopReason,
};
use vega_store::run_diagnostics::{
    DiagnosticFailureCode, DiagnosticMetrics, DiagnosticPhase, DiagnosticState,
    DiagnosticStopReason, NewDiagnosticEvent,
};

const DIAGNOSTIC_CHANNEL_CAPACITY: usize = 64;

#[derive(Clone)]
pub(crate) struct DiagnosticsContext {
    pub(crate) writer: Arc<DiagnosticsWriter>,
    pub(crate) thread_id: String,
    pub(crate) run_id: String,
    pub(crate) root_attempt_id: String,
}

impl DiagnosticsContext {
    pub(crate) fn stage_event(
        &self,
        attempt_id: &str,
        phase: DiagnosticPhase,
        state: DiagnosticState,
        failure_code: Option<DiagnosticFailureCode>,
        started_at: Instant,
        metrics: DiagnosticMetrics,
    ) {
        self.writer.record(NewDiagnosticEvent {
            thread_id: self.thread_id.clone(),
            run_id: self.run_id.clone(),
            attempt_id: attempt_id.to_string(),
            parent_attempt_id: Some(self.root_attempt_id.clone()),
            tool_call_id: None,
            phase,
            state,
            failure_code: failure_code.or_else(|| terminal_failure(state)),
            occurred_at: now_ms(),
            duration_ms: (state != DiagnosticState::Started)
                .then(|| u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX)),
            metrics,
        });
    }
}

pub(crate) struct DiagnosticsWriter {
    sender: SyncSender<NewDiagnosticEvent>,
    #[cfg(test)]
    worker_done: Arc<std::sync::atomic::AtomicBool>,
}

pub(crate) struct ToolDiagnosticEvent<'a> {
    pub(crate) thread_id: &'a str,
    pub(crate) run_id: &'a str,
    pub(crate) root_attempt_id: &'a str,
    pub(crate) attempt_id: &'a str,
    pub(crate) tool_call_id: &'a str,
    pub(crate) state: DiagnosticState,
    pub(crate) failure_code: Option<DiagnosticFailureCode>,
    pub(crate) started_at: Instant,
    pub(crate) output_bytes: Option<u64>,
    pub(crate) truncated: Option<bool>,
    pub(crate) duration_ms: Option<u64>,
}

impl DiagnosticsWriter {
    pub(crate) fn start(database_path: PathBuf) -> Self {
        let (sender, receiver) = sync_channel(DIAGNOSTIC_CHANNEL_CAPACITY);
        #[cfg(test)]
        let worker_done = Arc::new(std::sync::atomic::AtomicBool::new(false));
        #[cfg(test)]
        let worker_done_thread = Arc::clone(&worker_done);
        let spawn_result = std::thread::Builder::new()
            .name("vega-run-diagnostics".into())
            .spawn(move || {
                if let Ok(store) = vega_store::Store::open(database_path) {
                    while let Ok(event) = receiver.recv() {
                        let _ = vega_store::run_diagnostics::insert(store.conn(), &event);
                    }
                }
                #[cfg(test)]
                worker_done_thread.store(true, std::sync::atomic::Ordering::Release);
            });
        #[cfg(test)]
        if spawn_result.is_err() {
            worker_done.store(true, std::sync::atomic::Ordering::Release);
        }
        let _ = spawn_result;
        Self {
            sender,
            #[cfg(test)]
            worker_done,
        }
    }

    #[cfg(test)]
    fn worker_done(&self) -> Arc<std::sync::atomic::AtomicBool> {
        Arc::clone(&self.worker_done)
    }

    pub(crate) fn record(&self, event: NewDiagnosticEvent) {
        match self.sender.try_send(event) {
            Ok(()) | Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {}
        }
    }

    pub(crate) fn root_event(
        &self,
        thread_id: &str,
        run_id: &str,
        attempt_id: &str,
        state: DiagnosticState,
        failure_code: Option<DiagnosticFailureCode>,
        started_at: Instant,
    ) {
        self.record(NewDiagnosticEvent {
            thread_id: thread_id.to_string(),
            run_id: run_id.to_string(),
            attempt_id: attempt_id.to_string(),
            parent_attempt_id: None,
            tool_call_id: None,
            phase: DiagnosticPhase::Run,
            state,
            failure_code: failure_code.or_else(|| terminal_failure(state)),
            occurred_at: now_ms(),
            duration_ms: (state != DiagnosticState::Started)
                .then(|| u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX)),
            metrics: DiagnosticMetrics::default(),
        });
    }

    pub(crate) fn runtime_attempt(
        &self,
        thread_id: &str,
        run_id: &str,
        root_attempt_id: &str,
        attempt: &RuntimeDiagnosticAttempt,
    ) {
        self.record(NewDiagnosticEvent {
            thread_id: thread_id.to_string(),
            run_id: run_id.to_string(),
            attempt_id: attempt.attempt_id.clone(),
            parent_attempt_id: Some(root_attempt_id.to_string()),
            tool_call_id: None,
            phase: match attempt.phase {
                RuntimeDiagnosticPhase::PrimaryModel => DiagnosticPhase::PrimaryModel,
                RuntimeDiagnosticPhase::ContextSummary => DiagnosticPhase::ContextSummary,
                RuntimeDiagnosticPhase::Tool => DiagnosticPhase::Tool,
            },
            state: match attempt.state {
                RuntimeDiagnosticState::Started => DiagnosticState::Started,
                RuntimeDiagnosticState::Succeeded => DiagnosticState::Succeeded,
                RuntimeDiagnosticState::Failed => DiagnosticState::Failed,
                RuntimeDiagnosticState::Cancelled => DiagnosticState::Cancelled,
                RuntimeDiagnosticState::Interrupted => DiagnosticState::Interrupted,
            },
            failure_code: attempt.failure.map(runtime_failure).or_else(|| {
                terminal_failure(match attempt.state {
                    RuntimeDiagnosticState::Started => DiagnosticState::Started,
                    RuntimeDiagnosticState::Succeeded => DiagnosticState::Succeeded,
                    RuntimeDiagnosticState::Failed => DiagnosticState::Failed,
                    RuntimeDiagnosticState::Cancelled => DiagnosticState::Cancelled,
                    RuntimeDiagnosticState::Interrupted => DiagnosticState::Interrupted,
                })
            }),
            occurred_at: now_ms(),
            duration_ms: attempt.duration_ms,
            metrics: DiagnosticMetrics {
                stop_reason: attempt.metrics.stop_reason.map(stop_reason),
                input_tokens: attempt.metrics.input_tokens,
                output_tokens: attempt.metrics.output_tokens,
                cache_read_tokens: attempt.metrics.cache_read_tokens,
                cache_write_tokens: attempt.metrics.cache_write_tokens,
                visible_output_bytes: attempt.metrics.visible_output_bytes,
                tool_output_bytes: None,
                tool_truncated: None,
                http_status: attempt.metrics.provider.http_status,
                request_id: attempt.metrics.provider.request_id.clone(),
                retry_count: attempt.metrics.provider.retry_count,
            },
        });
    }

    pub(crate) fn tool_event(&self, event: ToolDiagnosticEvent<'_>) {
        let tool_call_id = safe_tool_call_id(event.tool_call_id).map(str::to_owned);
        self.record(NewDiagnosticEvent {
            thread_id: event.thread_id.to_string(),
            run_id: event.run_id.to_string(),
            attempt_id: event.attempt_id.to_string(),
            parent_attempt_id: Some(event.root_attempt_id.to_string()),
            tool_call_id,
            phase: DiagnosticPhase::Tool,
            state: event.state,
            failure_code: event.failure_code.or_else(|| terminal_failure(event.state)),
            occurred_at: now_ms(),
            duration_ms: (event.state != DiagnosticState::Started).then(|| {
                event.duration_ms.unwrap_or_else(|| {
                    u64::try_from(event.started_at.elapsed().as_millis()).unwrap_or(u64::MAX)
                })
            }),
            metrics: DiagnosticMetrics {
                tool_output_bytes: event.output_bytes,
                tool_truncated: event.truncated,
                ..DiagnosticMetrics::default()
            },
        });
    }
}

fn runtime_failure(value: RuntimeDiagnosticFailure) -> DiagnosticFailureCode {
    match value {
        RuntimeDiagnosticFailure::ContextOverLimit => DiagnosticFailureCode::ContextOverLimit,
        RuntimeDiagnosticFailure::ReasoningLimit => DiagnosticFailureCode::ReasoningLimit,
        RuntimeDiagnosticFailure::ProviderHttp => DiagnosticFailureCode::ProviderHttp,
        RuntimeDiagnosticFailure::ProviderTransportOrStream => {
            DiagnosticFailureCode::ProviderTransportOrStream
        }
        RuntimeDiagnosticFailure::ProviderProtocol => DiagnosticFailureCode::ProviderProtocol,
        RuntimeDiagnosticFailure::ProviderRejected => DiagnosticFailureCode::ProviderRejected,
        RuntimeDiagnosticFailure::UnknownSafeFailure => DiagnosticFailureCode::UnknownSafeFailure,
    }
}

fn terminal_failure(state: DiagnosticState) -> Option<DiagnosticFailureCode> {
    match state {
        DiagnosticState::Cancelled => Some(DiagnosticFailureCode::Cancelled),
        DiagnosticState::Interrupted => Some(DiagnosticFailureCode::Interrupted),
        DiagnosticState::Started | DiagnosticState::Succeeded | DiagnosticState::Failed => None,
    }
}

fn stop_reason(value: StopReason) -> DiagnosticStopReason {
    match value {
        StopReason::End => DiagnosticStopReason::End,
        StopReason::ToolUse => DiagnosticStopReason::ToolUse,
        StopReason::Length => DiagnosticStopReason::Length,
    }
}

pub(crate) fn safe_tool_call_id(value: &str) -> Option<&str> {
    (!value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte)))
    .then_some(value)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event() -> NewDiagnosticEvent {
        NewDiagnosticEvent {
            thread_id: "thread".into(),
            run_id: "run".into(),
            attempt_id: "attempt".into(),
            parent_attempt_id: None,
            tool_call_id: None,
            phase: DiagnosticPhase::Run,
            state: DiagnosticState::Started,
            failure_code: None,
            occurred_at: 1,
            duration_ms: None,
            metrics: DiagnosticMetrics::default(),
        }
    }

    #[test]
    fn best_effort_queue_drops_full_and_closed_events_without_returning_errors() {
        let (sender, receiver) = sync_channel(1);
        sender.try_send(event()).unwrap();
        let writer = DiagnosticsWriter {
            sender,
            worker_done: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        };
        writer.record(event());
        assert!(receiver.try_recv().is_ok());
        assert!(receiver.try_recv().is_err());

        let (sender, receiver) = sync_channel(1);
        drop(receiver);
        let writer = DiagnosticsWriter {
            sender,
            worker_done: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        };
        writer.record(event());
    }

    #[test]
    fn dropping_last_sender_drains_terminal_event_and_allows_reopen() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        {
            let store = vega_store::Store::open(temp.path()).unwrap();
            store.migrate().unwrap();
            vega_store::threads::create_standalone(
                store.conn(),
                vega_store::threads::NewThread {
                    id: "thread",
                    project_id: "",
                    title: "fixture",
                    mode: "execute",
                    permission_mode: "readonly",
                    model: "mock",
                    status: "active",
                    pinned: false,
                    unread: false,
                    created_at: 1,
                    updated_at: 1,
                },
            )
            .unwrap();
        }
        let writer = DiagnosticsWriter::start(temp.path().to_path_buf());
        let worker_done = writer.worker_done();
        let mut started = event();
        started.thread_id = "thread".into();
        started.run_id = "run".into();
        started.attempt_id = "attempt".into();
        writer.record(started.clone());
        let mut terminal = started;
        terminal.state = DiagnosticState::Succeeded;
        terminal.occurred_at = 2;
        terminal.duration_ms = Some(1);
        writer.record(terminal);
        drop(writer);
        for _ in 0..100 {
            if worker_done.load(std::sync::atomic::Ordering::Acquire) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(worker_done.load(std::sync::atomic::Ordering::Acquire));

        let reopened = vega_store::Store::open(temp.path()).unwrap();
        reopened.migrate().unwrap();
        let events =
            vega_store::run_diagnostics::read_by_run(reopened.conn(), "thread", "run").unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event.state, DiagnosticState::Started);
        assert_eq!(events[1].event.state, DiagnosticState::Succeeded);
    }
}
