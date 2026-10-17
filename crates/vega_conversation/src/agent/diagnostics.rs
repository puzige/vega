use std::collections::VecDeque;
#[cfg(test)]
use std::path::PathBuf;
use std::sync::Arc;
#[cfg(test)]
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

enum DiagnosticsSink {
    #[cfg(test)]
    Worker(SyncSender<NewDiagnosticEvent>),
    Persistence(tokio::sync::mpsc::Sender<super::persistence::PersistenceCommand>),
    Buffered(VecDeque<NewDiagnosticEvent>),
}

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
    sender: std::sync::Mutex<Option<DiagnosticsSink>>,
    #[cfg(test)]
    worker: std::sync::Mutex<Option<std::thread::JoinHandle<()>>>,
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
    #[cfg(test)]
    pub(crate) fn start(database_path: PathBuf) -> Self {
        let (sender, receiver) = sync_channel(DIAGNOSTIC_CHANNEL_CAPACITY);
        #[cfg(test)]
        let worker_done = Arc::new(std::sync::atomic::AtomicBool::new(false));
        #[cfg(test)]
        let worker_done_thread = Arc::clone(&worker_done);
        let worker = std::thread::Builder::new()
            .name("vega-run-diagnostics".into())
            .spawn(move || {
                if let Ok(store) = vega_store::Store::open(database_path) {
                    while let Ok(event) = receiver.recv() {
                        let _ = vega_store::run_diagnostics::insert(store.conn(), &event);
                    }
                }
                #[cfg(test)]
                worker_done_thread.store(true, std::sync::atomic::Ordering::Release);
            })
            .ok();
        #[cfg(test)]
        if worker.is_none() {
            worker_done.store(true, std::sync::atomic::Ordering::Release);
        }
        Self {
            sender: std::sync::Mutex::new(Some(DiagnosticsSink::Worker(sender))),
            worker: std::sync::Mutex::new(worker),
            #[cfg(test)]
            worker_done,
        }
    }

    pub(crate) fn for_persistence_actor(
        sender: tokio::sync::mpsc::Sender<super::persistence::PersistenceCommand>,
    ) -> Self {
        Self {
            sender: std::sync::Mutex::new(Some(DiagnosticsSink::Persistence(sender))),
            #[cfg(test)]
            worker: std::sync::Mutex::new(None),
            #[cfg(test)]
            worker_done: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        }
    }

    pub(crate) fn buffered() -> Self {
        Self {
            sender: std::sync::Mutex::new(Some(DiagnosticsSink::Buffered(VecDeque::new()))),
            #[cfg(test)]
            worker: std::sync::Mutex::new(None),
            #[cfg(test)]
            worker_done: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        }
    }

    #[cfg(test)]
    fn worker_done(&self) -> Arc<std::sync::atomic::AtomicBool> {
        Arc::clone(&self.worker_done)
    }

    pub(crate) fn record(&self, event: NewDiagnosticEvent) {
        let Ok(mut sender) = self.sender.lock() else {
            return;
        };
        match sender.as_mut() {
            #[cfg(test)]
            Some(DiagnosticsSink::Worker(sender)) => match sender.try_send(event) {
                Ok(()) | Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {}
            },
            Some(DiagnosticsSink::Persistence(sender)) => {
                let _ = sender.try_send(super::persistence::PersistenceCommand::Diagnostic(
                    Box::new(event),
                ));
            }
            Some(DiagnosticsSink::Buffered(events))
                if events.len() < DIAGNOSTIC_CHANNEL_CAPACITY =>
            {
                events.push_back(event);
            }
            Some(DiagnosticsSink::Buffered(_)) => {}
            None => {}
        }
    }

    pub(crate) fn flush_to(&self, store: &vega_store::Store) {
        let sink = self.sender.lock().ok().and_then(|mut sender| sender.take());
        if let Some(DiagnosticsSink::Buffered(mut events)) = sink {
            while let Some(event) = events.pop_front() {
                let _ = vega_store::run_diagnostics::insert(store.conn(), &event);
            }
        }
    }

    pub(crate) async fn close(&self) {
        if let Ok(mut sender) = self.sender.lock() {
            sender.take();
        }
        #[cfg(test)]
        let worker = self.worker.lock().ok().and_then(|mut worker| worker.take());
        #[cfg(test)]
        if let Some(worker) = worker {
            let _ = tokio::task::spawn_blocking(move || worker.join()).await;
        }
    }

    pub(crate) fn root_event(
        &self,
        thread_id: &str,
        run_id: &str,
        attempt_id: &str,
        state: DiagnosticState,
        failure: (Option<DiagnosticFailureCode>, DiagnosticMetrics),
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
            failure_code: failure.0.or_else(|| terminal_failure(state)),
            occurred_at: now_ms(),
            duration_ms: (state != DiagnosticState::Started)
                .then(|| u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX)),
            metrics: failure.1,
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
                failure_tool_index: attempt.metrics.failure_tool_index,
                failure_source: attempt
                    .metrics
                    .failure_source
                    .map(str::to_owned)
                    .or_else(|| {
                        (attempt.failure == Some(RuntimeDiagnosticFailure::ReasoningReplay))
                            .then(|| "runtime".to_owned())
                    }),
                failure_kind: attempt.metrics.failure_kind.map(str::to_owned).or_else(|| {
                    (attempt.failure == Some(RuntimeDiagnosticFailure::ReasoningReplay))
                        .then(|| "protocol".to_owned())
                }),
                failure_scope: attempt
                    .metrics
                    .failure_scope
                    .map(str::to_owned)
                    .or_else(|| {
                        (attempt.failure == Some(RuntimeDiagnosticFailure::ReasoningReplay))
                            .then(|| "response".to_owned())
                    }),
                failure_observed: attempt.metrics.failure_observed,
                failure_limit: attempt.metrics.failure_limit,
                failure_units: attempt.metrics.failure_units.map(str::to_owned),
                failure_observed_items: attempt.metrics.failure_observed_items,
                failure_limit_items: attempt.metrics.failure_limit_items,
                stop_reason: attempt.metrics.stop_reason.and_then(stop_reason),
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

pub(crate) fn failure_code(error: &vega_runtime::VegaError) -> Option<DiagnosticFailureCode> {
    use vega_runtime::{ContextEstimateError, ContextRuntimeError, ProviderFailureKind, VegaError};
    if error.context_input_overflow().is_some() {
        return Some(DiagnosticFailureCode::ContextOverLimit);
    }
    match error {
        VegaError::Cancelled => None,
        VegaError::ProviderPayloadTooLarge { .. } => Some(DiagnosticFailureCode::ProviderProtocol),
        VegaError::ReasoningBudgetExceeded { .. }
        | VegaError::ReasoningReplayBudgetExceeded { .. } => {
            Some(DiagnosticFailureCode::ReasoningLimit)
        }
        VegaError::ReasoningReplayUnsupported | VegaError::ReasoningReplayIncomplete => {
            Some(DiagnosticFailureCode::ProviderProtocol)
        }
        VegaError::ProviderDiagnostic { kind, .. } => Some(match kind {
            ProviderFailureKind::Http => DiagnosticFailureCode::ProviderHttp,
            ProviderFailureKind::Transport => DiagnosticFailureCode::ProviderTransportOrStream,
            ProviderFailureKind::Protocol => DiagnosticFailureCode::ProviderProtocol,
            ProviderFailureKind::Rejected | ProviderFailureKind::OutputLimit => {
                DiagnosticFailureCode::ProviderRejected
            }
        }),
        VegaError::Provider {
            status: Some(_), ..
        } => Some(DiagnosticFailureCode::ProviderHttp),
        VegaError::Context(ContextRuntimeError::SummaryTimedOut) => {
            Some(DiagnosticFailureCode::SummaryTimeout)
        }
        VegaError::Context(ContextRuntimeError::SummaryOutputTruncated { .. }) => {
            Some(DiagnosticFailureCode::SummaryTruncated)
        }
        VegaError::Context(ContextRuntimeError::SummaryEmpty) => {
            Some(DiagnosticFailureCode::SummaryEmpty)
        }
        VegaError::Context(ContextRuntimeError::SummaryFormatInvalid)
        | VegaError::Context(ContextRuntimeError::InvalidSummary) => {
            Some(DiagnosticFailureCode::SummaryFormatInvalid)
        }
        VegaError::Context(
            ContextRuntimeError::SummaryProjectionInvalid
            | ContextRuntimeError::InvalidProjection
            | ContextRuntimeError::SystemMessageInResult,
        ) => Some(DiagnosticFailureCode::SummaryProjectionInvalid),
        VegaError::Context(ContextRuntimeError::SourceChanged) => {
            Some(DiagnosticFailureCode::SummarySourceChanged)
        }
        VegaError::Context(
            ContextRuntimeError::OverLimit { .. }
            | ContextRuntimeError::SummaryInputOverLimit { .. }
            | ContextRuntimeError::ResultOverLimit { .. }
            | ContextRuntimeError::Estimate(ContextEstimateError::InputTooLarge),
        ) => Some(DiagnosticFailureCode::ContextOverLimit),
        VegaError::Context(ContextRuntimeError::NoCompactablePrefix) => {
            Some(DiagnosticFailureCode::SummaryNoCompactablePrefix)
        }
        VegaError::Context(ContextRuntimeError::SourceTooLarge) => {
            Some(DiagnosticFailureCode::SummarySourceTooLarge)
        }
        VegaError::Context(ContextRuntimeError::AggregateTooLarge) => {
            Some(DiagnosticFailureCode::SummaryAggregateTooLarge)
        }
        VegaError::Context(ContextRuntimeError::ImagesUnsupported) => {
            Some(DiagnosticFailureCode::SummaryImagesUnsupported)
        }
        VegaError::Context(ContextRuntimeError::AlreadyAttempted) => {
            Some(DiagnosticFailureCode::SummaryAlreadyAttempted)
        }
        _ => None,
    }
}

pub(crate) fn run_failure(error: &vega_runtime::VegaError) -> DiagnosticFailureCode {
    failure_code(error).unwrap_or(DiagnosticFailureCode::UnknownSafeFailure)
}

pub(crate) fn failure_metrics(error: &vega_runtime::VegaError) -> DiagnosticMetrics {
    let detail = error.diagnostic_metrics();
    let mut metrics = DiagnosticMetrics {
        failure_tool_index: detail.failure_tool_index,
        failure_source: detail.failure_source.map(str::to_owned),
        failure_kind: detail.failure_kind.map(str::to_owned),
        failure_scope: detail.failure_scope.map(str::to_owned),
        failure_observed: detail.failure_observed,
        failure_limit: detail.failure_limit,
        failure_units: detail.failure_units.map(str::to_owned),
        failure_observed_items: detail.failure_observed_items,
        failure_limit_items: detail.failure_limit_items,
        http_status: detail.provider.http_status,
        retry_count: detail.provider.retry_count,
        request_id: detail.provider.request_id,
        ..DiagnosticMetrics::default()
    };
    metrics.failure_kind = Some(
        match crate::types::RunFailureKind::from_runtime(error) {
            crate::types::RunFailureKind::ProviderHttp(_) => "http",
            crate::types::RunFailureKind::ProviderQuota => "quota",
            crate::types::RunFailureKind::ProviderAuthorization => "authorization",
            crate::types::RunFailureKind::ProviderRateLimited => "rate_limited",
            crate::types::RunFailureKind::ContextOverLimit => "context_over_limit",
            crate::types::RunFailureKind::ReasoningReplay => "protocol",
            _ => return metrics,
        }
        .into(),
    );
    metrics
}

pub(crate) fn with_failure_metrics(
    mut metrics: DiagnosticMetrics,
    error: &vega_runtime::VegaError,
) -> DiagnosticMetrics {
    let detail = failure_metrics(error);
    metrics.failure_source = detail.failure_source;
    metrics.failure_kind = detail.failure_kind;
    metrics.failure_scope = detail.failure_scope;
    metrics.failure_observed = detail.failure_observed;
    metrics.failure_limit = detail.failure_limit;
    metrics.failure_units = detail.failure_units;
    metrics.failure_observed_items = detail.failure_observed_items;
    metrics.failure_limit_items = detail.failure_limit_items;
    metrics.failure_tool_index = detail.failure_tool_index;
    metrics.http_status = detail.http_status.or(metrics.http_status);
    metrics.retry_count = detail.retry_count.or(metrics.retry_count);
    metrics.request_id = detail.request_id.or(metrics.request_id);
    metrics
}

fn runtime_failure(value: RuntimeDiagnosticFailure) -> DiagnosticFailureCode {
    match value {
        RuntimeDiagnosticFailure::ContextOverLimit => DiagnosticFailureCode::ContextOverLimit,
        RuntimeDiagnosticFailure::ReasoningLimit => DiagnosticFailureCode::ReasoningLimit,
        RuntimeDiagnosticFailure::ReasoningReplay => DiagnosticFailureCode::ProviderProtocol,
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

fn stop_reason(value: StopReason) -> Option<DiagnosticStopReason> {
    match value {
        StopReason::End => Some(DiagnosticStopReason::End),
        StopReason::ToolUse => Some(DiagnosticStopReason::ToolUse),
        StopReason::Length => Some(DiagnosticStopReason::Length),
        StopReason::Unknown => None,
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

    #[test]
    fn compaction_failure_codes_preserve_closed_runtime_classification() {
        use vega_runtime::{ContextRuntimeError as ContextError, VegaError};

        let cases = [
            (
                VegaError::Context(ContextError::SourceChanged),
                DiagnosticFailureCode::SummarySourceChanged,
            ),
            (
                VegaError::Context(ContextError::SummaryTimedOut),
                DiagnosticFailureCode::SummaryTimeout,
            ),
            (
                VegaError::Context(ContextError::SummaryOutputTruncated {
                    visible_bytes: 12,
                    thinking_bytes: 8,
                    output_tokens: Some(3),
                }),
                DiagnosticFailureCode::SummaryTruncated,
            ),
            (
                VegaError::Context(ContextError::SummaryEmpty),
                DiagnosticFailureCode::SummaryEmpty,
            ),
            (
                VegaError::Context(ContextError::InvalidSummary),
                DiagnosticFailureCode::SummaryFormatInvalid,
            ),
            (
                VegaError::Context(ContextError::SummaryFormatInvalid),
                DiagnosticFailureCode::SummaryFormatInvalid,
            ),
            (
                VegaError::Context(ContextError::SummaryProjectionInvalid),
                DiagnosticFailureCode::SummaryProjectionInvalid,
            ),
            (
                VegaError::Context(ContextError::InvalidProjection),
                DiagnosticFailureCode::SummaryProjectionInvalid,
            ),
            (
                VegaError::Context(ContextError::SystemMessageInResult),
                DiagnosticFailureCode::SummaryProjectionInvalid,
            ),
            (
                VegaError::Context(ContextError::NoCompactablePrefix),
                DiagnosticFailureCode::SummaryNoCompactablePrefix,
            ),
            (
                VegaError::Context(ContextError::SourceTooLarge),
                DiagnosticFailureCode::SummarySourceTooLarge,
            ),
            (
                VegaError::Context(ContextError::AggregateTooLarge),
                DiagnosticFailureCode::SummaryAggregateTooLarge,
            ),
            (
                VegaError::Context(ContextError::ImagesUnsupported),
                DiagnosticFailureCode::SummaryImagesUnsupported,
            ),
            (
                VegaError::Context(ContextError::AlreadyAttempted),
                DiagnosticFailureCode::SummaryAlreadyAttempted,
            ),
            (
                VegaError::Context(ContextError::Estimate(
                    vega_runtime::ContextEstimateError::InputTooLarge,
                )),
                DiagnosticFailureCode::ContextOverLimit,
            ),
        ];

        for (error, expected) in cases {
            assert_eq!(failure_code(&error), Some(expected));
            assert_eq!(run_failure(&error), expected);
        }
        assert_eq!(failure_code(&VegaError::Cancelled), None);
        assert_eq!(
            run_failure(&VegaError::Cancelled),
            DiagnosticFailureCode::UnknownSafeFailure
        );
        assert_eq!(
            failure_code(&VegaError::Context(ContextError::MissingHook)),
            None
        );
    }

    #[test]
    fn compaction_failure_codes_preserve_provider_reasoning_and_unknown_fallbacks() {
        use vega_runtime::{ProviderFailureKind, ReasoningBudgetScope, VegaError};

        for (kind, expected) in [
            (
                ProviderFailureKind::Http,
                DiagnosticFailureCode::ProviderHttp,
            ),
            (
                ProviderFailureKind::Transport,
                DiagnosticFailureCode::ProviderTransportOrStream,
            ),
            (
                ProviderFailureKind::Protocol,
                DiagnosticFailureCode::ProviderProtocol,
            ),
            (
                ProviderFailureKind::Rejected,
                DiagnosticFailureCode::ProviderRejected,
            ),
            (
                ProviderFailureKind::OutputLimit,
                DiagnosticFailureCode::ProviderRejected,
            ),
        ] {
            let error = VegaError::ProviderDiagnostic {
                kind,
                status: None,
                message: "private provider payload".into(),
                retryable: false,
                retry_count: None,
                request_id: None,
            };
            assert_eq!(failure_code(&error), Some(expected));
            assert_eq!(run_failure(&error), expected);
        }

        let error = VegaError::ProviderPayloadTooLarge {
            observed_bytes: 4_200_000,
            limit_bytes: 4_194_304,
        };
        assert_eq!(
            failure_code(&error),
            Some(DiagnosticFailureCode::ProviderProtocol)
        );
        assert_eq!(run_failure(&error), DiagnosticFailureCode::ProviderProtocol);

        let error = VegaError::Provider {
            status: Some(503),
            message: "private provider payload".into(),
            retryable: false,
        };
        assert_eq!(
            failure_code(&error),
            Some(DiagnosticFailureCode::ProviderHttp)
        );
        let provider_without_status = VegaError::Provider {
            status: None,
            message: "private provider payload says quota exceeded".into(),
            retryable: false,
        };
        assert_eq!(failure_code(&provider_without_status), None);
        assert_eq!(
            run_failure(&provider_without_status),
            DiagnosticFailureCode::UnknownSafeFailure
        );

        let overflow = VegaError::Provider {
            status: Some(400),
            message: r#"{"error":{"code":"context_length_exceeded","requested_tokens":130000,"max_context_tokens":128000}}"#.into(),
            retryable: false,
        };
        assert_eq!(
            failure_code(&overflow),
            Some(DiagnosticFailureCode::ContextOverLimit)
        );
        for error in [
            VegaError::ReasoningBudgetExceeded {
                scope: ReasoningBudgetScope::Turn,
                observed_bytes: 262_145,
                limit_bytes: 262_144,
            },
            VegaError::ReasoningReplayBudgetExceeded {
                scope: ReasoningBudgetScope::Run,
                observed_bytes: 262_145,
                limit_bytes: 262_144,
                observed_items: 9,
                limit_items: 8,
            },
        ] {
            assert_eq!(
                failure_code(&error),
                Some(DiagnosticFailureCode::ReasoningLimit)
            );
            assert_eq!(run_failure(&error), DiagnosticFailureCode::ReasoningLimit);
        }
        for error in [
            VegaError::ReasoningReplayUnsupported,
            VegaError::ReasoningReplayIncomplete,
        ] {
            assert_eq!(
                failure_code(&error),
                Some(DiagnosticFailureCode::ProviderProtocol)
            );
            assert_eq!(run_failure(&error), DiagnosticFailureCode::ProviderProtocol);
            assert_eq!(
                failure_metrics(&error).failure_kind.as_deref(),
                Some("protocol")
            );
        }
        assert_eq!(
            failure_code(&VegaError::Cancelled),
            None,
            "cancelled errors are not reclassified as failures"
        );
    }

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
    fn classified_failures_preserve_live_kind_and_measurements_after_storage() {
        use crate::types::RunFailureKind;
        use vega_runtime::{ProviderFailureKind, VegaError};
        let errors = vec![
            VegaError::ReasoningBudgetExceeded {
                scope: vega_runtime::ReasoningBudgetScope::Turn,
                observed_bytes: 262145,
                limit_bytes: 262144,
            },
            VegaError::ProviderPayloadTooLarge {
                observed_bytes: 4194305,
                limit_bytes: 4194304,
            },
            VegaError::Context(vega_runtime::ContextRuntimeError::OverLimit {
                estimated_tokens: 101,
                input_budget: 100,
            }),
            VegaError::CredentialExposureBlocked,
            VegaError::CredentialCheckUnavailable,
            VegaError::CredentialContinuationUnavailable,
            VegaError::CredentialProjectionInvalid { tool_index: 4 },
            VegaError::Provider {
                status: None,
                message: "PRIVATE_PROVIDER_BODY".into(),
                retryable: false,
            },
        ]
        .into_iter()
        .chain(
            [
                ProviderFailureKind::Protocol,
                ProviderFailureKind::Transport,
                ProviderFailureKind::OutputLimit,
                ProviderFailureKind::Rejected,
            ]
            .into_iter()
            .map(|kind| VegaError::ProviderDiagnostic {
                kind,
                status: None,
                message: "PRIVATE_PROVIDER_BODY".into(),
                retryable: false,
                retry_count: Some(2),
                request_id: Some("safe-id".into()),
            }),
        );
        let temp = tempfile::NamedTempFile::new().unwrap();
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
        let mut expected = Vec::new();
        for (index, error) in errors.enumerate() {
            let mut start = event();
            start.attempt_id = format!("failure-{index}");
            start.run_id = start.attempt_id.clone();
            vega_store::messages::insert(
                store.conn(),
                &vega_store::messages::MessageRow {
                    id: start.run_id.clone(),
                    thread_id: "thread".into(),
                    seq: index as i64 + 1,
                    role: "assistant".into(),
                    kind: "text".into(),
                    content: String::new(),
                    status: "failed".into(),
                    created_at: 1,
                    plan_status: None,
                    plan_review_note: None,
                    plan_reviewed_at: None,
                },
            )
            .unwrap();
            vega_store::run_diagnostics::insert(store.conn(), &start).unwrap();
            let mut terminal = start;
            terminal.state = DiagnosticState::Failed;
            terminal.duration_ms = Some(1);
            terminal.failure_code = Some(run_failure(&error));
            terminal.metrics = failure_metrics(&error);
            expected.push((
                RunFailureKind::from_runtime(&error),
                terminal.metrics.clone(),
            ));
            vega_store::run_diagnostics::insert(store.conn(), &terminal).unwrap();
        }
        drop(store);
        let reopened = vega_store::Store::open(temp.path()).unwrap();
        let rows = vega_store::run_diagnostics::read_by_thread(reopened.conn(), "thread").unwrap();
        assert_eq!(
            rows.iter()
                .filter(|row| row.event.state == DiagnosticState::Failed)
                .count(),
            expected.len()
        );
        let projection = super::super::compaction::read_context_projection(
            &reopened, "thread", "mock", "system",
        )
        .unwrap();
        assert_eq!(projection.run_failures.len(), expected.len());
        for (index, (kind, _)) in expected.iter().enumerate() {
            assert!(
                projection
                    .run_failures
                    .contains(&(format!("failure-{index}"), *kind))
            );
        }
        for (row, (kind, metrics)) in rows
            .iter()
            .filter(|row| row.event.state == DiagnosticState::Failed)
            .zip(expected)
        {
            assert_eq!(row.event.metrics, metrics);
            let restored = RunFailureKind::from_diagnostic(&row.event.metrics);
            assert_eq!(restored, kind);
            assert_eq!(restored.message(), kind.message());
            assert!(!restored.message().contains("PRIVATE_PROVIDER_BODY"));
        }
        let export =
            vega_store::run_diagnostics::export_run(reopened.conn(), "thread", "failure-0")
                .unwrap();
        assert!(!export.contains("PRIVATE_PROVIDER_BODY"));
    }

    #[test]
    fn best_effort_queue_drops_full_and_closed_events_without_returning_errors() {
        let (sender, receiver) = sync_channel(1);
        sender.try_send(event()).unwrap();
        let writer = DiagnosticsWriter {
            sender: std::sync::Mutex::new(Some(DiagnosticsSink::Worker(sender))),
            worker: std::sync::Mutex::new(None),
            worker_done: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        };
        writer.record(event());
        assert!(receiver.try_recv().is_ok());
        assert!(receiver.try_recv().is_err());

        let (sender, receiver) = sync_channel(1);
        drop(receiver);
        let writer = DiagnosticsWriter {
            sender: std::sync::Mutex::new(Some(DiagnosticsSink::Worker(sender))),
            worker: std::sync::Mutex::new(None),
            worker_done: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        };
        writer.record(event());
    }

    #[tokio::test]
    async fn persistence_sink_drops_full_and_closed_events_without_returning_errors() {
        let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
        sender
            .try_send(super::super::persistence::PersistenceCommand::Diagnostic(
                Box::new(event()),
            ))
            .unwrap();
        let writer = DiagnosticsWriter::for_persistence_actor(sender);
        writer.record(event());
        assert!(matches!(
            receiver.try_recv(),
            Ok(super::super::persistence::PersistenceCommand::Diagnostic(_))
        ));
        assert!(receiver.try_recv().is_err());

        let (sender, receiver) = tokio::sync::mpsc::channel(1);
        drop(receiver);
        let writer = DiagnosticsWriter::for_persistence_actor(sender);
        writer.record(event());
    }

    #[test]
    fn buffered_writer_is_bounded_and_flushes_in_order_on_the_caller_store() {
        let temp = tempfile::NamedTempFile::new().unwrap();
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
        let writer = DiagnosticsWriter::buffered();
        for index in 1..=DIAGNOSTIC_CHANNEL_CAPACITY + 1 {
            let mut diagnostic = event();
            diagnostic.attempt_id = format!("attempt-{index}");
            diagnostic.occurred_at = index as i64;
            writer.record(diagnostic);
        }

        writer.flush_to(&store);

        let events =
            vega_store::run_diagnostics::read_by_run(store.conn(), "thread", "run").unwrap();
        assert_eq!(events.len(), DIAGNOSTIC_CHANNEL_CAPACITY);
        assert_eq!(
            events
                .iter()
                .map(|event| event.event.occurred_at)
                .collect::<Vec<_>>(),
            (1..=DIAGNOSTIC_CHANNEL_CAPACITY as i64).collect::<Vec<_>>()
        );

        store
            .conn()
            .execute_batch(
                "CREATE TRIGGER fail_buffered_diagnostic_insert
                 BEFORE INSERT ON run_diagnostic_events
                 BEGIN SELECT RAISE(ABORT, 'diagnostic insert failed'); END;",
            )
            .unwrap();
        let writer = DiagnosticsWriter::buffered();
        writer.record(event());
        writer.flush_to(&store);
        let count: i64 = store
            .conn()
            .query_row("SELECT COUNT(*) FROM run_diagnostic_events", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, DIAGNOSTIC_CHANNEL_CAPACITY as i64);
    }

    #[tokio::test]
    async fn closing_last_sender_drains_terminal_event_and_allows_reopen() {
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
        writer.close().await;
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
