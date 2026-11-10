use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy, Debug)]
pub enum PredictionEvent {
    Prepared,
    PreparationBlocked,
    StreamIneligible,
    CatalogDeferred,
    CatalogResumed,
    Started,
    Cancelled,
    Stale,
    LocalFailure,
    AttemptStarted,
    Suggestion,
    Abstained,
    InvalidOutput,
    StreamInterrupted,
    Timeout,
    ProviderRetryable,
    ProviderStopped,
    SafetyStopped,
    UnexpectedEvent,
    Exhausted,
    Applied,
    ApplyRejected,
}

impl PredictionEvent {
    fn as_str(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::PreparationBlocked => "preparation_blocked",
            Self::StreamIneligible => "stream_ineligible",
            Self::CatalogDeferred => "catalog_deferred",
            Self::CatalogResumed => "catalog_resumed",
            Self::Started => "started",
            Self::Cancelled => "cancelled",
            Self::Stale => "stale",
            Self::LocalFailure => "local_failure",
            Self::AttemptStarted => "attempt_started",
            Self::Suggestion => "suggestion",
            Self::Abstained => "abstained",
            Self::InvalidOutput => "invalid_output",
            Self::StreamInterrupted => "stream_interrupted",
            Self::Timeout => "timeout",
            Self::ProviderRetryable => "provider_retryable",
            Self::ProviderStopped => "provider_stopped",
            Self::SafetyStopped => "safety_stopped",
            Self::UnexpectedEvent => "unexpected_event",
            Self::Exhausted => "exhausted",
            Self::Applied => "applied",
            Self::ApplyRejected => "apply_rejected",
        }
    }
}

struct PendingPredictionEvent {
    database: PathBuf,
    thread_id: String,
    message_id: String,
    event: PredictionEvent,
    attempt: u8,
    occurred_at: i64,
}

impl crate::database_writer::Sealed for PendingPredictionEvent {}

impl crate::database_writer::DatabaseCommand for PendingPredictionEvent {
    type Output = Result<(), String>;

    fn apply(self, store: &vega_store::Store) -> Self::Output {
        vega_store::composer_prediction_events::record(
            store.conn(),
            &self.thread_id,
            &self.message_id,
            self.event.as_str(),
            self.attempt,
            self.occurred_at,
        ).map_err(|_| "prediction diagnostic persistence failed".to_owned())
    }
}

pub fn record_prediction_event(
    database: &std::path::Path,
    thread_id: &str,
    message_id: &str,
    event: PredictionEvent,
    attempt: u8,
) {
    let occurred_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            duration.as_millis().min(i64::MAX as u128) as i64
        });
    let event = PendingPredictionEvent { database: database.to_path_buf(), thread_id: thread_id.to_owned(), message_id: message_id.to_owned(), event, attempt, occurred_at };
    if let Err(error) = crate::submission_diagnostics::background::submit(move || {
        let succeeded = vega_store::Store::open_read_only(&event.database).ok().is_some_and(|store| {
            let path = event.database.clone();
            crate::database_writer::commit(&path, store.database_identity(), event).is_ok_and(|result| result.is_ok())
        });
        if !succeeded {
            tracing::warn!(outcome="prediction_diagnostic_not_confirmed");
            return Err("prediction diagnostic persistence not confirmed".into());
        }
        Ok(())
    }) {
        tracing::warn!(%error, outcome="prediction_diagnostic_not_accepted");
    }
}
