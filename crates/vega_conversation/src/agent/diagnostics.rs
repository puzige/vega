use std::collections::VecDeque;
use std::sync::Arc;
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
}

pub(crate) struct ToolDiagnosticEvent<'a> {
    pub(crate) thread_id: &'a str,
    pub(crate) run_id: &'a str,
    pub(crate) root_attempt_id: &'a str,
    pub(crate) attempt_id: &'a str,
    pub(crate) tool_call_id: &'a str,
    pub(crate) state: DiagnosticState,
    pub(crate) failure_code: Option<DiagnosticFailureCode>,
    pub(crate) failure: Option<&'a vega_runtime::VegaError>,
    pub(crate) started_at: Instant,
    pub(crate) output_bytes: Option<u64>,
    pub(crate) truncated: Option<bool>,
    pub(crate) duration_ms: Option<u64>,
}

impl DiagnosticsWriter {
    pub(crate) fn for_persistence_actor(
        sender: tokio::sync::mpsc::Sender<super::persistence::PersistenceCommand>,
    ) -> Self {
        Self {
            sender: std::sync::Mutex::new(Some(DiagnosticsSink::Persistence(sender))),
        }
    }

    pub(crate) fn buffered() -> Self {
        Self {
            sender: std::sync::Mutex::new(Some(DiagnosticsSink::Buffered(VecDeque::new()))),
        }
    }

    pub(crate) fn record(&self, event: NewDiagnosticEvent) {
        let Ok(mut sender) = self.sender.lock() else {
            return;
        };
        match sender.as_mut() {
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
                runtime_stage: attempt.metrics.runtime_stage.map(str::to_owned),
                runtime_reason: attempt.metrics.runtime_reason.map(str::to_owned),
                sqlite_code: attempt.metrics.sqlite_code,
                sqlite_extended_code: attempt.metrics.sqlite_extended_code,
                io_kind: attempt.metrics.io_kind.map(str::to_owned),
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
        let metrics = DiagnosticMetrics {
            tool_output_bytes: event.output_bytes,
            tool_truncated: event.truncated,
            ..DiagnosticMetrics::default()
        };
        let metrics = match event.failure {
            Some(error) => with_failure_metrics(metrics, error),
            None => metrics,
        };
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
            metrics,
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
        VegaError::Context(ContextRuntimeError::Estimate(
            ContextEstimateError::InvalidBudget | ContextEstimateError::Overflow,
        )) => Some(DiagnosticFailureCode::RuntimeFailure),
        VegaError::RuntimeFailure(_) | VegaError::Store(_) | VegaError::Io(_) => {
            Some(DiagnosticFailureCode::RuntimeFailure)
        }
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
            ProviderFailureKind::Transport => DiagnosticFailureCode::ProviderTransport,
            ProviderFailureKind::StreamInterrupted => {
                DiagnosticFailureCode::ProviderStreamInterrupted
            }
            ProviderFailureKind::Timeout => DiagnosticFailureCode::ProviderTimeout,
            ProviderFailureKind::Overloaded => DiagnosticFailureCode::ProviderOverloaded,
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
        runtime_stage: detail.runtime_stage.map(str::to_owned),
        runtime_reason: detail.runtime_reason.map(str::to_owned),
        sqlite_code: detail.sqlite_code,
        sqlite_extended_code: detail.sqlite_extended_code,
        io_kind: detail.io_kind.map(str::to_owned),
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
    metrics.runtime_stage = detail.runtime_stage;
    metrics.runtime_reason = detail.runtime_reason;
    metrics.sqlite_code = detail.sqlite_code;
    metrics.sqlite_extended_code = detail.sqlite_extended_code;
    metrics.io_kind = detail.io_kind;
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
        RuntimeDiagnosticFailure::ProviderTransport => DiagnosticFailureCode::ProviderTransport,
        RuntimeDiagnosticFailure::ProviderStreamInterrupted => {
            DiagnosticFailureCode::ProviderStreamInterrupted
        }
        RuntimeDiagnosticFailure::ProviderTimeout => DiagnosticFailureCode::ProviderTimeout,
        RuntimeDiagnosticFailure::ProviderOverloaded => DiagnosticFailureCode::ProviderOverloaded,
        RuntimeDiagnosticFailure::ProviderProtocol => DiagnosticFailureCode::ProviderProtocol,
        RuntimeDiagnosticFailure::ProviderRejected => DiagnosticFailureCode::ProviderRejected,
        RuntimeDiagnosticFailure::RuntimeFailure => DiagnosticFailureCode::RuntimeFailure,
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
