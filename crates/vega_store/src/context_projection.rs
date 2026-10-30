use serde::{Deserialize, Serialize};

use crate::context_compaction::{ContextCheckpointError, ContextSource};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssistantCursor {
    pub text_bytes: u64,
    pub tool_seq: u64,
    pub tool_id: String,
    pub response_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceCursor {
    pub message_id: String,
    pub message_seq: u64,
    pub assistant: Option<AssistantCursor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRange {
    pub after: Option<SourceCursor>,
    pub through: Option<SourceCursor>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionStrategy {
    Automatic,
    Full,
    Manual,
    PartialUpTo,
    PartialFrom,
    Reactive,
    Precompute,
    Micro,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContinuationMode {
    ContinueExistingRun,
    NoContinuation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProjectionFragment {
    Source { range: SourceRange },
    Summary,
    PreviousSummary { checkpoint_id: i64 },
    TaskReference { message_id: String },
    HistoricalImages { through_seq: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectionPlan {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_dependencies: Option<Vec<String>>,
    pub schema: u32,
    pub strategy: ProjectionStrategy,
    pub continuation: ContinuationMode,
    pub source_owner_id: Option<String>,
    pub task_message_id: Option<String>,
    pub covered: Vec<SourceRange>,
    pub omitted: Vec<SourceRange>,
    pub fragments: Vec<ProjectionFragment>,
}

impl SourceCursor {
    pub fn validate(&self, source: &ContextSource) -> Result<(), ContextCheckpointError> {
        let invalid = ContextCheckpointError::IncompleteCoverage;
        let message = source
            .messages
            .iter()
            .find(|message| {
                message.id == self.message_id
                    && u64::try_from(message.seq).ok() == Some(self.message_seq)
            })
            .ok_or(invalid)?;
        if let Some(cursor) = &self.assistant {
            let bytes =
                usize::try_from(cursor.text_bytes).map_err(|_| ContextCheckpointError::Invalid)?;
            let call = source
                .tool_calls
                .iter()
                .find(|call| {
                    call.message_id == message.id
                        && call.id == cursor.tool_id
                        && u64::try_from(call.seq).ok() == Some(cursor.tool_seq)
                })
                .ok_or(ContextCheckpointError::IncompleteCoverage)?;
            if message.role != "assistant"
                || !message.content.is_char_boundary(bytes)
                || call.text_offset_bytes != i64::try_from(bytes).ok()
                || call.response_id != cursor.response_id
                || source.tool_calls.iter().any(|other| {
                    other.message_id == message.id
                        && other.seq > call.seq
                        && match (&other.response_id, &call.response_id) {
                            (Some(left), Some(right)) => left == right,
                            _ => other.text_offset_bytes == call.text_offset_bytes,
                        }
                })
            {
                return Err(ContextCheckpointError::IncompleteCoverage);
            }
        }
        Ok(())
    }

    fn key(&self) -> (u64, bool, u64, u64) {
        match &self.assistant {
            Some(cursor) => (self.message_seq, false, cursor.tool_seq, cursor.text_bytes),
            None => (self.message_seq, true, 0, 0),
        }
    }
}

impl SourceRange {
    pub fn validate(&self, source: &ContextSource) -> Result<(), ContextCheckpointError> {
        if let Some(after) = &self.after {
            after.validate(source)?;
        }
        if let Some(through) = &self.through {
            through.validate(source)?;
        }
        if let (Some(after), Some(through)) = (&self.after, &self.through)
            && after.key() >= through.key()
        {
            return Err(ContextCheckpointError::Invalid);
        }
        Ok(())
    }

    pub fn project(&self, source: &ContextSource) -> Result<ContextSource, ContextCheckpointError> {
        self.validate(source)?;
        let mut projected = source.clone();
        projected.messages.retain(|message| {
            let seq = message.seq as u64;
            self.after.as_ref().is_none_or(|after| {
                seq > after.message_seq || (seq == after.message_seq && after.assistant.is_some())
            }) && self
                .through
                .as_ref()
                .is_none_or(|through| seq <= through.message_seq)
        });
        projected.tool_calls.retain(|call| {
            projected
                .messages
                .iter()
                .any(|message| message.id == call.message_id)
                && self.after.as_ref().is_none_or(|after| {
                    call.message_id != after.message_id
                        || after
                            .assistant
                            .as_ref()
                            .is_none_or(|cursor| call.seq as u64 > cursor.tool_seq)
                })
                && self.through.as_ref().is_none_or(|through| {
                    call.message_id != through.message_id
                        || through
                            .assistant
                            .as_ref()
                            .is_none_or(|cursor| call.seq as u64 <= cursor.tool_seq)
                })
        });
        for message in &mut projected.messages {
            let start = self
                .after
                .as_ref()
                .filter(|after| after.message_id == message.id)
                .and_then(|after| after.assistant.as_ref())
                .map_or(0, |cursor| cursor.text_bytes as usize);
            let end = self
                .through
                .as_ref()
                .filter(|through| through.message_id == message.id)
                .and_then(|through| through.assistant.as_ref())
                .map_or(message.content.len(), |cursor| cursor.text_bytes as usize);
            let text = message
                .content
                .get(start..end)
                .ok_or(ContextCheckpointError::IncompleteCoverage)?;
            message.content = text.to_owned();
            if (start != 0
                || self.after.as_ref().is_some_and(|after| {
                    after.message_id == message.id && after.assistant.is_some()
                })
                || self.through.as_ref().is_some_and(|through| {
                    through.message_id == message.id && through.assistant.is_some()
                }))
                && message.status == "streaming"
            {
                message.status = "done".into();
            }
            for call in projected
                .tool_calls
                .iter_mut()
                .filter(|call| call.message_id == message.id)
            {
                if let Some(offset) = call.text_offset_bytes {
                    if offset < start as i64 || offset > end as i64 {
                        return Err(ContextCheckpointError::IncompleteCoverage);
                    }
                    call.text_offset_bytes = Some(offset - start as i64);
                }
            }
        }
        projected.images.retain(|image| {
            projected
                .messages
                .iter()
                .any(|message| message.id == image.message_id)
                && image.artifact_call_id.as_ref().is_none_or(|id| {
                    projected
                        .tool_calls
                        .iter()
                        .any(|call| call.id == *id && call.message_id == image.message_id)
                })
        });
        if let Some(after) = &self.after
            && after.assistant.is_some()
        {
            projected.messages.retain(|message| {
                message.id != after.message_id
                    || !message.content.is_empty()
                    || projected
                        .tool_calls
                        .iter()
                        .any(|call| call.message_id == message.id)
                    || projected
                        .images
                        .iter()
                        .any(|image| image.message_id == message.id)
            });
        }
        Ok(projected)
    }

    pub fn validate_terminal(&self, source: &ContextSource) -> Result<(), ContextCheckpointError> {
        if self.through.is_none() {
            return Err(ContextCheckpointError::Invalid);
        }
        let projected = self.project(source)?;
        if projected
            .messages
            .iter()
            .any(|message| !matches!(message.status.as_str(), "done" | "failed" | "interrupted"))
            || projected.tool_calls.iter().any(|call| {
                !matches!(
                    call.status.as_str(),
                    "success" | "failed" | "cancelled" | "rejected"
                ) || call.output_text.is_none()
            })
        {
            return Err(ContextCheckpointError::IncompleteCoverage);
        }
        Ok(())
    }
}

impl ProjectionPlan {
    pub fn encode(&self) -> Result<String, ContextCheckpointError> {
        let encoded = serde_json::to_string(self).map_err(|_| ContextCheckpointError::Invalid)?;
        if encoded.len() > 256 * 1024 {
            return Err(ContextCheckpointError::Invalid);
        }
        Ok(encoded)
    }

    pub fn decode(encoded: &str) -> Result<Self, ContextCheckpointError> {
        if encoded.len() > 256 * 1024 {
            return Err(ContextCheckpointError::Invalid);
        }
        serde_json::from_str(encoded).map_err(|_| ContextCheckpointError::Invalid)
    }

    pub fn validate(&self, source: &ContextSource) -> Result<(), ContextCheckpointError> {
        if self.schema != 1
            || self.covered.is_empty()
            || self.covered.len() + self.omitted.len() + self.fragments.len() > 256
            || self
                .fragments
                .iter()
                .filter(|fragment| matches!(fragment, ProjectionFragment::Summary))
                .count()
                != 1
        {
            return Err(ContextCheckpointError::Invalid);
        }
        if let Some(owner) = &self.source_owner_id
            && !source
                .messages
                .iter()
                .any(|message| message.id == *owner && message.role == "assistant")
        {
            return Err(ContextCheckpointError::Invalid);
        }
        if self.continuation == ContinuationMode::ContinueExistingRun
            && self.source_owner_id.is_none()
        {
            return Err(ContextCheckpointError::Invalid);
        }
        for id in self
            .task_message_id
            .iter()
            .chain(self.fragments.iter().filter_map(|fragment| match fragment {
                ProjectionFragment::TaskReference { message_id } => Some(message_id),
                _ => None,
            }))
        {
            if !source.messages.iter().any(|message| {
                message.id == *id && message.role == "user" && message.status == "done"
            }) {
                return Err(ContextCheckpointError::Invalid);
            }
        }
        let mut partition = self.covered.iter().chain(&self.omitted).collect::<Vec<_>>();
        for range in &partition {
            range.validate(source)?;
            if range.through.is_none() {
                return Err(ContextCheckpointError::Invalid);
            }
        }
        for fragment in &self.fragments {
            match fragment {
                ProjectionFragment::Source { range } => {
                    range.validate(source)?;
                    partition.push(range);
                }
                ProjectionFragment::PreviousSummary { checkpoint_id } if *checkpoint_id <= 0 => {
                    return Err(ContextCheckpointError::Invalid);
                }
                ProjectionFragment::HistoricalImages { through_seq }
                    if *through_seq > source.source_version =>
                {
                    return Err(ContextCheckpointError::Invalid);
                }
                _ => {}
            }
        }
        partition.sort_by_key(|range| range.after.as_ref().map(SourceCursor::key));
        if partition.first().is_none_or(|range| range.after.is_some())
            || partition.last().is_none_or(|range| range.through.is_some())
            || partition
                .windows(2)
                .any(|pair| pair[0].through.is_none() || pair[0].through != pair[1].after)
        {
            return Err(ContextCheckpointError::Invalid);
        }
        Ok(())
    }

    pub fn validate_install(&self, source: &ContextSource) -> Result<(), ContextCheckpointError> {
        self.validate(source)?;
        for range in self.covered.iter().chain(&self.omitted) {
            range.validate_terminal(source)?;
        }
        Ok(())
    }
}
