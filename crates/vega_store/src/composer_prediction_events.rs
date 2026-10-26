use rusqlite::{Connection, params};
use std::time::{SystemTime, UNIX_EPOCH};

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

pub fn record(
    conn: &Connection,
    thread_id: &str,
    completed_message_id: &str,
    event: PredictionEvent,
    attempt: u8,
) {
    let result = (|| -> rusqlite::Result<()> {
        let _permit = crate::write_coordination::acquire(
            conn,
            crate::write_coordination::WriteCategory::Diagnostics,
        )?;
        let occurred_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| {
                duration.as_millis().min(i64::MAX as u128) as i64
            });
        conn.execute(
            "INSERT INTO composer_prediction_events (thread_id, completed_message_id, event, attempt, occurred_at)
             SELECT ?1, ?2, ?3, ?4, ?5 WHERE EXISTS (
                 SELECT 1 FROM messages WHERE id = ?2 AND thread_id = ?1
             )",
            params![thread_id, completed_message_id, event.as_str(), attempt, occurred_at],
        )?;
        Ok(())
    })();
    if result.is_err() {
        tracing::warn!("composer prediction diagnostic persistence failed");
    }
}
