use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

use crate::context_compaction::{ContextCheckpointError, ContextSource, ContextToolCallRow};

pub fn eligible(call: &ContextToolCallRow) -> bool {
    matches!(
        call.tool.as_str(),
        "read" | "Read" | "grep" | "glob" | "bash"
    ) && matches!(call.status.as_str(), "success" | "failed")
        && call.output_text.is_some()
}

pub fn call_digest(call: &ContextToolCallRow) -> String {
    let mut hash = Sha256::new();
    for value in [
        call.thread_id.as_bytes(),
        call.message_id.as_bytes(),
        call.id.as_bytes(),
        call.tool.as_bytes(),
        call.input_json.as_bytes(),
        call.status.as_bytes(),
        call.output_text.as_deref().unwrap_or_default().as_bytes(),
    ] {
        hash.update((value.len() as u64).to_le_bytes());
        hash.update(value);
    }
    hash.update(call.seq.to_le_bytes());
    hash.update(call.created_at.to_le_bytes());
    for value in [
        call.approval.as_deref(),
        call.response_id.as_deref(),
        Some(call.output_full_path_fingerprint.as_str()),
    ] {
        hash.update([u8::from(value.is_some())]);
        if let Some(value) = value {
            hash.update((value.len() as u64).to_le_bytes());
            hash.update(value.as_bytes());
        }
    }
    for value in [
        call.exit_code.map(i64::from),
        call.duration_ms,
        call.finished_at,
        call.text_offset_bytes,
    ] {
        hash.update([u8::from(value.is_some())]);
        if let Some(value) = value {
            hash.update(value.to_le_bytes());
        }
    }
    format!("{:x}", hash.finalize())
}

pub fn install<F>(
    conn: &Connection,
    source: &ContextSource,
    model: &str,
    previous_id: Option<i64>,
    call_ids: &[String],
    created_at: i64,
    should_cancel: F,
) -> Result<String, ContextCheckpointError>
where
    F: Fn() -> bool,
{
    if call_ids.is_empty() || call_ids.len() > 4096 {
        return Err(ContextCheckpointError::Invalid);
    }
    let transaction = crate::write_coordination::WriteTransaction::new_with_cancel(
        conn,
        crate::write_coordination::WriteCategory::ContextCompaction,
        &should_cancel,
    )
    .map_err(|error| match error {
        rusqlite::Error::SqliteFailure(ref failure, _)
            if failure.extended_code == rusqlite::ffi::SQLITE_INTERRUPT =>
        {
            ContextCheckpointError::Cancelled
        }
        error => ContextCheckpointError::Store(error),
    })?;
    if should_cancel() {
        return Err(ContextCheckpointError::Cancelled);
    }
    let current =
        crate::context_compaction::load_source_in_transaction(&transaction, &source.thread_id)?;
    let current_model: Option<String> = transaction
        .query_row(
            "SELECT model FROM threads WHERE id=?1",
            [&source.thread_id],
            |row| row.get(0),
        )
        .optional()?;
    let previous = crate::context_compaction::latest_checkpoint_in_transaction(
        &transaction,
        &source.thread_id,
        model,
    )?;
    if current.source_version != source.source_version
        || current.fingerprint != source.fingerprint
        || current_model.as_deref() != Some(model)
        || previous.map(|checkpoint| checkpoint.id) != previous_id
    {
        return Err(ContextCheckpointError::Stale);
    }
    let mut unique = std::collections::HashSet::new();
    for id in call_ids {
        if !unique.insert(id) {
            return Err(ContextCheckpointError::Invalid);
        }
        let call = current
            .tool_calls
            .iter()
            .find(|call| &call.id == id)
            .ok_or(ContextCheckpointError::Invalid)?;
        if !eligible(call) || call.micro_compacted {
            return Err(ContextCheckpointError::Invalid);
        }
        transaction.execute(
            "INSERT INTO context_micro_compactions(call_id,thread_id,source_digest,created_at) VALUES(?1,?2,?3,?4) ON CONFLICT(call_id) DO UPDATE SET source_digest=excluded.source_digest,created_at=excluded.created_at",
            params![id, source.thread_id, call_digest(call), created_at],
        )?;
    }
    if should_cancel() {
        return Err(ContextCheckpointError::Cancelled);
    }
    let installed =
        crate::context_compaction::load_source_in_transaction(&transaction, &source.thread_id)?;
    if should_cancel() {
        return Err(ContextCheckpointError::Cancelled);
    }
    transaction.commit()?;
    Ok(installed.fingerprint)
}
