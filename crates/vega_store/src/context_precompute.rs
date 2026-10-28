pub mod accounting;
pub mod consume;
pub mod ready;

use rusqlite::Transaction;
use serde::{Deserialize, Serialize};

use crate::context_compaction::{
    ContextCheckpointError, ContextSource, fingerprint_selected_source, load_source_in_transaction,
};
use crate::context_projection::{AssistantCursor, SourceCursor};

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextAppendFence {
    schema: u32,
    thread_id: String,
    through_message_seq: u64,
    through_tool_seq: u64,
    fingerprint: String,
    append_owner: Option<AppendOwner>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AppendOwner {
    message_id: String,
    message_seq: u64,
    text_bytes: u64,
    response: Option<SourceCursor>,
}

impl ContextAppendFence {
    pub fn encode(&self) -> Result<String, ContextCheckpointError> {
        self.validate_shape()?;
        let encoded = serde_json::to_string(self).map_err(|_| ContextCheckpointError::Invalid)?;
        if encoded.len() > 32 * 1024 {
            return Err(ContextCheckpointError::Invalid);
        }
        Ok(encoded)
    }

    pub fn decode(encoded: &str) -> Result<Self, ContextCheckpointError> {
        if encoded.len() > 32 * 1024 {
            return Err(ContextCheckpointError::Invalid);
        }
        let fence: Self =
            serde_json::from_str(encoded).map_err(|_| ContextCheckpointError::Invalid)?;
        fence.validate_shape()?;
        Ok(fence)
    }

    fn validate_shape(&self) -> Result<(), ContextCheckpointError> {
        if self.schema != 1
            || self.thread_id.is_empty()
            || self.thread_id.len() > 4096
            || self.through_message_seq > i64::MAX as u64
            || self.through_tool_seq > i64::MAX as u64
            || self.fingerprint.len() != 64
            || !self
                .fingerprint
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(ContextCheckpointError::Invalid);
        }
        if let Some(owner) = &self.append_owner {
            if owner.message_id.is_empty()
                || owner.message_id.len() > 4096
                || owner.message_seq > self.through_message_seq
                || owner.text_bytes > 64 * 1024 * 1024
            {
                return Err(ContextCheckpointError::Invalid);
            }
            match &owner.response {
                Some(cursor)
                    if cursor.message_id == owner.message_id
                        && cursor.message_seq == owner.message_seq
                        && cursor.assistant.as_ref().is_some_and(|response| {
                            response.text_bytes == owner.text_bytes
                                && response.tool_seq <= self.through_tool_seq
                                && !response.tool_id.is_empty()
                                && response.tool_id.len() <= 4096
                                && response
                                    .response_id
                                    .as_ref()
                                    .is_none_or(|id| id.len() <= 4096)
                        }) => {}
                None if owner.text_bytes == 0 => {}
                _ => return Err(ContextCheckpointError::Invalid),
            }
        }
        Ok(())
    }
}

pub fn capture_in_transaction(
    transaction: &Transaction<'_>,
    thread_id: &str,
    source_owner_id: Option<&str>,
) -> Result<(ContextSource, ContextAppendFence), ContextCheckpointError> {
    let source = load_source_in_transaction(transaction, thread_id)?;
    if source.messages.is_empty() {
        return Err(ContextCheckpointError::IncompleteCoverage);
    }
    let append_owner = source_owner_id
        .map(|id| capture_owner(&source, id))
        .transpose()?
        .flatten();
    let mut fence = ContextAppendFence {
        schema: 1,
        thread_id: source.thread_id.clone(),
        through_message_seq: u64::try_from(
            source.messages.iter().map(|row| row.seq).max().unwrap_or(0),
        )
        .map_err(|_| ContextCheckpointError::Invalid)?,
        through_tool_seq: u64::try_from(
            source
                .tool_calls
                .iter()
                .map(|row| row.seq)
                .max()
                .unwrap_or(0),
        )
        .map_err(|_| ContextCheckpointError::Invalid)?,
        fingerprint: "0".repeat(64),
        append_owner,
    };
    let captured = captured_prefix(&source, &fence)?;
    fence.fingerprint = fingerprint_selected_source(transaction, &captured)?;
    fence.encode()?;
    Ok((source, fence))
}

pub fn validate_in_transaction(
    transaction: &Transaction<'_>,
    fence: &ContextAppendFence,
) -> Result<ContextSource, ContextCheckpointError> {
    fence.validate_shape()?;
    let current = load_source_in_transaction(transaction, &fence.thread_id)?;
    let captured = captured_prefix(&current, fence)?;
    if fingerprint_selected_source(transaction, &captured)? != fence.fingerprint {
        return Err(ContextCheckpointError::Stale);
    }
    Ok(current)
}

fn capture_owner(
    source: &ContextSource,
    id: &str,
) -> Result<Option<AppendOwner>, ContextCheckpointError> {
    let owner = source
        .messages
        .iter()
        .find(|message| message.id == id && message.role == "assistant")
        .ok_or(ContextCheckpointError::IncompleteCoverage)?;
    if owner.status != "streaming" {
        return Ok(None);
    }
    if source.messages.last().map(|message| &message.id) != Some(&owner.id) {
        return Err(ContextCheckpointError::IncompleteCoverage);
    }
    let response = source
        .tool_calls
        .iter()
        .filter(|call| call.message_id == id)
        .max_by_key(|call| call.seq)
        .map(|call| {
            let cursor = SourceCursor {
                message_id: owner.id.clone(),
                message_seq: u64::try_from(owner.seq)
                    .map_err(|_| ContextCheckpointError::IncompleteCoverage)?,
                assistant: Some(AssistantCursor {
                    text_bytes: owner.content.len() as u64,
                    tool_seq: u64::try_from(call.seq)
                        .map_err(|_| ContextCheckpointError::IncompleteCoverage)?,
                    tool_id: call.id.clone(),
                    response_id: call.response_id.clone(),
                }),
            };
            cursor.validate(source)?;
            Ok::<_, ContextCheckpointError>(cursor)
        })
        .transpose()?;
    if response.is_none() && !owner.content.is_empty() {
        return Err(ContextCheckpointError::IncompleteCoverage);
    }
    Ok(Some(AppendOwner {
        message_id: owner.id.clone(),
        message_seq: u64::try_from(owner.seq)
            .map_err(|_| ContextCheckpointError::IncompleteCoverage)?,
        text_bytes: owner.content.len() as u64,
        response,
    }))
}

fn captured_prefix(
    current: &ContextSource,
    fence: &ContextAppendFence,
) -> Result<ContextSource, ContextCheckpointError> {
    if current.thread_id != fence.thread_id
        || current.source_version < fence.through_message_seq.max(fence.through_tool_seq)
    {
        return Err(ContextCheckpointError::Stale);
    }
    validate_current_source(current, fence)?;
    let through_message =
        i64::try_from(fence.through_message_seq).map_err(|_| ContextCheckpointError::Invalid)?;
    let through_tool =
        i64::try_from(fence.through_tool_seq).map_err(|_| ContextCheckpointError::Invalid)?;
    let mut captured = current.clone();
    captured
        .messages
        .retain(|message| message.seq <= through_message);
    captured.tool_calls.retain(|call| call.seq <= through_tool);
    let message_ids = captured
        .messages
        .iter()
        .map(|message| message.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    if captured.messages.iter().any(|message| message.seq < 0)
        || captured.tool_calls.iter().any(|call| {
            call.seq < 0
                || !message_ids.contains(call.message_id.as_str())
                || !matches!(
                    call.status.as_str(),
                    "success" | "failed" | "cancelled" | "rejected"
                )
                || call.output_text.is_none()
        })
        || current.tool_calls.iter().any(|call| {
            call.seq > through_tool
                && message_ids.contains(call.message_id.as_str())
                && fence
                    .append_owner
                    .as_ref()
                    .is_none_or(|owner| owner.message_id != call.message_id)
        })
    {
        return Err(ContextCheckpointError::Stale);
    }
    if let Some(owner) = &fence.append_owner {
        if let Some(response) = &owner.response {
            response
                .validate(current)
                .map_err(|_| ContextCheckpointError::Stale)?;
        } else if captured
            .tool_calls
            .iter()
            .any(|call| call.message_id == owner.message_id)
        {
            return Err(ContextCheckpointError::Stale);
        }
        let message = captured
            .messages
            .iter_mut()
            .find(|message| {
                message.id == owner.message_id
                    && u64::try_from(message.seq).ok() == Some(owner.message_seq)
            })
            .ok_or(ContextCheckpointError::Stale)?;
        if message.role != "assistant" || !matches!(message.status.as_str(), "streaming" | "done") {
            return Err(ContextCheckpointError::Stale);
        }
        let bytes =
            usize::try_from(owner.text_bytes).map_err(|_| ContextCheckpointError::Invalid)?;
        if !message.content.is_char_boundary(bytes) {
            return Err(ContextCheckpointError::Stale);
        }
        message.content.truncate(bytes);
        message.status = "streaming".into();
    }
    if captured.messages.iter().any(|message| {
        fence
            .append_owner
            .as_ref()
            .is_none_or(|owner| owner.message_id != message.id)
            && !matches!(message.status.as_str(), "done" | "failed" | "interrupted")
    }) {
        return Err(ContextCheckpointError::Stale);
    }
    Ok(captured)
}

fn validate_current_source(
    current: &ContextSource,
    fence: &ContextAppendFence,
) -> Result<(), ContextCheckpointError> {
    let messages = current
        .messages
        .iter()
        .map(|message| (message.id.as_str(), message))
        .collect::<std::collections::HashMap<_, _>>();
    let mut call_sequences = std::collections::HashSet::new();
    let mut prior_responses = std::collections::HashSet::new();
    for call in &current.tool_calls {
        if call.seq < 0
            || !call_sequences.insert(call.seq)
            || !matches!(
                call.status.as_str(),
                "success" | "failed" | "cancelled" | "rejected"
            )
            || call.output_text.is_none()
        {
            return Err(ContextCheckpointError::Stale);
        }
        let message = messages
            .get(call.message_id.as_str())
            .ok_or(ContextCheckpointError::Stale)?;
        if message.role != "assistant" {
            return Err(ContextCheckpointError::Stale);
        }
        if call.seq as u64 <= fence.through_tool_seq {
            if let Some(response) = &call.response_id {
                prior_responses.insert((call.message_id.as_str(), response.as_str()));
            }
            continue;
        }
        let offset = call
            .text_offset_bytes
            .and_then(|offset| usize::try_from(offset).ok())
            .ok_or(ContextCheckpointError::Stale)?;
        if !message.content.is_char_boundary(offset)
            || fence.append_owner.as_ref().is_some_and(|owner| {
                owner.message_id == call.message_id && (offset as u64) < owner.text_bytes
            })
            || call.response_id.as_ref().is_some_and(|response| {
                prior_responses.contains(&(call.message_id.as_str(), response.as_str()))
            })
        {
            return Err(ContextCheckpointError::Stale);
        }
    }
    for message in &current.messages {
        if message.status == "streaming" {
            capture_owner(current, &message.id)?;
        } else if !matches!(message.status.as_str(), "done" | "failed" | "interrupted") {
            return Err(ContextCheckpointError::Stale);
        }
    }
    Ok(())
}
