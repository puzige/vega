//! Run-local primary input usage accounting. Fingerprints never leave memory.

use sha2::{Digest, Sha256};

use crate::{
    ChatMessage, ChatRequest, ContextEstimate, ContextEstimateError, FrozenReasoning,
    ToolDefinition,
};

#[derive(Clone)]
pub(super) struct InputAnchor {
    prefix_digest: [u8; 32],
    covered_messages: usize,
    prefix_estimate: u64,
    input: u64,
    model: String,
    reasoning: Option<FrozenReasoning>,
    max_tokens: Option<u32>,
}

pub(super) struct PendingInputAnchor {
    prefix_digest: [u8; 32],
    covered_messages: usize,
    prefix_estimate: u64,
    model: String,
    reasoning: Option<FrozenReasoning>,
    max_tokens: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct InputDecision {
    pub estimate: ContextEstimate,
    pub source: super::ContextAccountingSource,
    pub baseline: Option<u64>,
    pub incremental: u64,
    pub predicted: u64,
    pub revision: u64,
    pub covered_messages: usize,
}

impl PendingInputAnchor {
    pub fn from_request(request: &ChatRequest) -> Result<Option<Self>, ContextEstimateError> {
        let prefix_estimate =
            crate::estimate_wire_context(&request.messages, &request.tools)?.input_tokens;
        let Some(prefix_digest) = fingerprint(&request.messages, &request.tools) else {
            return Ok(None);
        };
        Ok(Some(Self {
            prefix_digest,
            covered_messages: request.messages.len(),
            prefix_estimate,
            model: request.model.clone(),
            reasoning: request.reasoning.clone(),
            max_tokens: request.max_tokens,
        }))
    }

    pub fn complete(self, input: u64) -> Option<InputAnchor> {
        (input > 0).then_some(InputAnchor {
            prefix_digest: self.prefix_digest,
            covered_messages: self.covered_messages,
            prefix_estimate: self.prefix_estimate,
            input,
            model: self.model,
            reasoning: self.reasoning,
            max_tokens: self.max_tokens,
        })
    }
}

impl InputAnchor {
    pub fn decide(
        anchor: Option<&Self>,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        model: &str,
        reasoning: Option<&FrozenReasoning>,
        max_tokens: Option<u32>,
        revision: u64,
    ) -> Result<InputDecision, ContextEstimateError> {
        let raw = crate::estimate_wire_context(messages, tools)?;
        if let Some(anchor) = anchor
            && messages.len() >= anchor.covered_messages
            && model == anchor.model
            && reasoning == anchor.reasoning.as_ref()
            && max_tokens == anchor.max_tokens
            && fingerprint(&messages[..anchor.covered_messages], tools)
                == Some(anchor.prefix_digest)
        {
            let incremental = raw
                .input_tokens
                .checked_sub(anchor.prefix_estimate)
                .ok_or(ContextEstimateError::Overflow)?;
            let predicted = anchor
                .input
                .checked_add(incremental)
                .ok_or(ContextEstimateError::Overflow)?;
            return Ok(InputDecision {
                estimate: ContextEstimate {
                    input_tokens: predicted,
                    ..raw
                },
                source: super::ContextAccountingSource::UsageAnchored,
                baseline: Some(anchor.input),
                incremental,
                predicted,
                revision,
                covered_messages: anchor.covered_messages,
            });
        }
        Ok(InputDecision {
            estimate: raw,
            source: super::ContextAccountingSource::Estimated,
            baseline: None,
            incremental: raw.input_tokens,
            predicted: raw.input_tokens,
            revision,
            covered_messages: 0,
        })
    }
}

fn tagged(hasher: &mut Sha256, tag: u8, bytes: &[u8]) {
    hasher.update([tag]);
    hasher.update((bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

fn fingerprint(messages: &[ChatMessage], tools: &[ToolDefinition]) -> Option<[u8; 32]> {
    let mut hasher = Sha256::new();
    tagged(&mut hasher, 1, &(messages.len() as u64).to_le_bytes());
    let projected = crate::project_model_messages(messages).ok()?;
    for message in &projected {
        tagged(&mut hasher, 2, message.role.as_str().as_bytes());
        tagged(&mut hasher, 3, message.content.as_bytes());
        match &message.reasoning_content {
            Some(value) => tagged(&mut hasher, 4, value.as_bytes()),
            None => tagged(&mut hasher, 5, &[]),
        }
        if let Some(raw) = &message.chat_reasoning {
            tagged(&mut hasher, 38, &[1]);
            if let Some((source, text)) = &raw.text {
                tagged(&mut hasher, 39, source.field().as_bytes());
                tagged(&mut hasher, 40, text.as_bytes());
            }
            if let Some(details) = &raw.details {
                tagged(&mut hasher, 41, &serde_json::to_vec(details).ok()?);
            }
        } else {
            tagged(&mut hasher, 38, &[0]);
        }
        tagged(
            &mut hasher,
            22,
            &(message.response_reasoning.len() as u64).to_le_bytes(),
        );
        for item in &message.response_reasoning {
            tagged(&mut hasher, 23, item.to_string().as_bytes());
        }
        match &message.response_replay_layout {
            Some(layout) => {
                tagged(&mut hasher, 24, &[1]);
                match &layout.response_id {
                    Some(response_id) => tagged(&mut hasher, 25, response_id.as_bytes()),
                    None => tagged(&mut hasher, 26, &[]),
                }
                tagged(&mut hasher, 27, &(layout.slots.len() as u64).to_le_bytes());
                for slot in &layout.slots {
                    match slot {
                        crate::ResponseReplaySlot::Reasoning(item) => {
                            tagged(&mut hasher, 28, &serde_json::to_vec(item).ok()?);
                        }
                        crate::ResponseReplaySlot::Message(item) => {
                            tagged(&mut hasher, 29, &serde_json::to_vec(item).ok()?);
                        }
                        crate::ResponseReplaySlot::FunctionCall {
                            item,
                            call_id,
                            name,
                            arguments,
                        } => {
                            tagged(&mut hasher, 30, &serde_json::to_vec(item).ok()?);
                            tagged(&mut hasher, 31, call_id.as_bytes());
                            tagged(&mut hasher, 32, name.as_bytes());
                            tagged(&mut hasher, 33, arguments.as_bytes());
                        }
                    }
                }
            }
            None => tagged(&mut hasher, 34, &[]),
        }
        match &message.tool_call_id {
            Some(value) => tagged(&mut hasher, 6, value.as_bytes()),
            None => tagged(&mut hasher, 7, &[]),
        }
        tagged(
            &mut hasher,
            8,
            &(message.tool_calls.len() as u64).to_le_bytes(),
        );
        for call in &message.tool_calls {
            tagged(&mut hasher, 9, call.id.as_bytes());
            tagged(&mut hasher, 10, call.name.as_bytes());
            tagged(&mut hasher, 11, call.input_json.as_bytes());
        }
        tagged(
            &mut hasher,
            12,
            &(message.images.len() as u64).to_le_bytes(),
        );
        for image in &message.images {
            tagged(&mut hasher, 13, image.mime_type().as_bytes());
            tagged(&mut hasher, 14, &image.width().to_le_bytes());
            tagged(&mut hasher, 15, &image.height().to_le_bytes());
            tagged(&mut hasher, 16, image.bytes());
        }
    }
    tagged(&mut hasher, 17, &(tools.len() as u64).to_le_bytes());
    for tool in tools {
        tagged(&mut hasher, 18, tool.name.as_bytes());
        tagged(&mut hasher, 19, tool.description.as_bytes());
        tagged(
            &mut hasher,
            20,
            &serde_json::to_vec(&tool.input_schema).ok()?,
        );
        tagged(&mut hasher, 21, &[u8::from(tool.strict)]);
    }
    Some(hasher.finalize().into())
}

#[cfg(test)]
mod post_tool_diagnostics_tests {
    use super::*;
    #[test]
    fn anchored_increment_overflow_is_rejected_without_mutating_messages() {
        let messages = vec![ChatMessage::new(crate::ChatRole::User, "task")];
        let raw = crate::estimate_wire_context(&messages, &[]).unwrap();
        let anchor = InputAnchor {
            prefix_digest: fingerprint(&messages, &[]).unwrap(),
            covered_messages: messages.len(),
            prefix_estimate: raw.input_tokens - 1,
            input: u64::MAX,
            model: "model".into(),
            reasoning: None,
            max_tokens: None,
        };
        assert!(matches!(
            InputAnchor::decide(Some(&anchor), &messages, &[], "model", None, None, 0),
            Err(ContextEstimateError::Overflow)
        ));
        assert_eq!(messages[0].content, "task");
    }
}
