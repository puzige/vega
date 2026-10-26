use std::{
    path::PathBuf,
    sync::{
        OnceLock,
        mpsc::{self, SyncSender},
    },
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

pub fn record_prediction_event(
    database: &std::path::Path,
    thread_id: &str,
    message_id: &str,
    event: PredictionEvent,
    attempt: u8,
) {
    static WRITER: OnceLock<Option<SyncSender<PendingPredictionEvent>>> = OnceLock::new();
    let writer = WRITER.get_or_init(|| {
        let (sender, receiver) = mpsc::sync_channel::<PendingPredictionEvent>(1024);
        std::thread::Builder::new()
            .name("prediction-diagnostics".into())
            .spawn(move || {
                for event in receiver {
                    match vega_store::Store::open(&event.database) {
                        Ok(store) => vega_store::composer_prediction_events::record(
                            store.conn(),
                            &event.thread_id,
                            &event.message_id,
                            event.event.as_str(),
                            event.attempt,
                            event.occurred_at,
                        ),
                        Err(_) => {
                            tracing::warn!("composer prediction diagnostic store unavailable")
                        }
                    }
                }
            })
            .ok()
            .map(|_| sender)
    });
    let occurred_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            duration.as_millis().min(i64::MAX as u128) as i64
        });
    let queued = writer.as_ref().is_some_and(|writer| {
        writer
            .try_send(PendingPredictionEvent {
                database: database.to_path_buf(),
                thread_id: thread_id.to_string(),
                message_id: message_id.to_string(),
                event,
                attempt,
                occurred_at,
            })
            .is_ok()
    });
    if !queued {
        tracing::warn!("composer prediction diagnostic queue unavailable");
    }
}
