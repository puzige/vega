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
    for message in messages {
        tagged(&mut hasher, 2, message.role.as_str().as_bytes());
        tagged(&mut hasher, 3, message.content.as_bytes());
        match &message.reasoning_content {
            Some(value) => tagged(&mut hasher, 4, value.as_bytes()),
            None => tagged(&mut hasher, 5, &[]),
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
mod tests {
    use super::*;
    use crate::{ChatRole, ChatToolCall};

    fn request() -> ChatRequest {
        ChatRequest {
            model: "mock".into(),
            messages: vec![
                ChatMessage::new(ChatRole::System, "system"),
                ChatMessage::new(ChatRole::User, "user"),
            ],
            tools: vec![ToolDefinition {
                name: "read".into(),
                description: "read file".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": [],
                    "additionalProperties": false
                }),
                strict: false,
            }],
            max_tokens: Some(100),
            reasoning: Some(FrozenReasoning::unknown("mock", "mock")),
        }
    }

    #[test]
    fn issue91_anchor_accounts_only_appended_tool_pair_and_rejects_changed_prefix() {
        let sent = request();
        let anchor = PendingInputAnchor::from_request(&sent)
            .unwrap()
            .unwrap()
            .complete(85)
            .unwrap();
        let mut current = sent.messages.clone();
        current.push(ChatMessage::assistant_with_tools(
            "",
            vec![
                ChatToolCall {
                    id: "one".into(),
                    name: "read".into(),
                    input_json: "{}".into(),
                },
                ChatToolCall {
                    id: "two".into(),
                    name: "read".into(),
                    input_json: "{}".into(),
                },
            ],
        ));
        current.push(ChatMessage::tool_result("one", "first"));
        current.push(ChatMessage::tool_result("two", "second"));
        let full = crate::estimate_wire_context(&current, &sent.tools)
            .unwrap()
            .input_tokens;
        let prefix = crate::estimate_wire_context(&sent.messages, &sent.tools)
            .unwrap()
            .input_tokens;
        let anchored = InputAnchor::decide(
            Some(&anchor),
            &current,
            &sent.tools,
            "mock",
            sent.reasoning.as_ref(),
            Some(100),
            1,
        )
        .unwrap();
        assert_eq!(
            anchored.source,
            super::super::ContextAccountingSource::UsageAnchored
        );
        assert_eq!(anchored.incremental, full - prefix);
        assert_eq!(anchored.predicted, 85 + full - prefix);
        assert_eq!(anchored.covered_messages, sent.messages.len());
        current[1].content.push('!');
        let changed = InputAnchor::decide(
            Some(&anchor),
            &current,
            &sent.tools,
            "mock",
            sent.reasoning.as_ref(),
            Some(100),
            2,
        )
        .unwrap();
        assert_eq!(
            changed.source,
            super::super::ContextAccountingSource::Estimated
        );
        current[1].content.pop();
        let changed_model = InputAnchor::decide(
            Some(&anchor),
            &current,
            &sent.tools,
            "other",
            sent.reasoning.as_ref(),
            Some(100),
            2,
        )
        .unwrap();
        assert_eq!(
            changed_model.source,
            super::super::ContextAccountingSource::Estimated
        );
        let changed_schema = [ToolDefinition {
            input_schema: serde_json::json!({"type":"string"}),
            ..sent.tools[0].clone()
        }];
        let changed_tools = InputAnchor::decide(
            Some(&anchor),
            &current,
            &changed_schema,
            "mock",
            sent.reasoning.as_ref(),
            Some(100),
            2,
        )
        .unwrap();
        assert_eq!(
            changed_tools.source,
            super::super::ContextAccountingSource::Estimated
        );
        let changed_reasoning = InputAnchor::decide(
            Some(&anchor),
            &current,
            &sent.tools,
            "mock",
            None,
            Some(100),
            2,
        )
        .unwrap();
        assert_eq!(
            changed_reasoning.source,
            super::super::ContextAccountingSource::Estimated
        );
    }

    #[test]
    fn issue85_strict_changes_tool_fingerprint_and_invalidates_anchor() {
        let sent = request();
        let anchor = PendingInputAnchor::from_request(&sent)
            .unwrap()
            .unwrap()
            .complete(85)
            .unwrap();
        let strict_tools = [ToolDefinition {
            strict: true,
            ..sent.tools[0].clone()
        }];

        assert_ne!(
            fingerprint(&sent.messages, &sent.tools),
            fingerprint(&sent.messages, &strict_tools)
        );
        let decision = InputAnchor::decide(
            Some(&anchor),
            &sent.messages,
            &strict_tools,
            &sent.model,
            sent.reasoning.as_ref(),
            sent.max_tokens,
            1,
        )
        .unwrap();
        assert_eq!(
            decision.source,
            super::super::ContextAccountingSource::Estimated
        );
    }

    #[test]
    fn issue91_matching_anchor_arithmetic_overflow_fails_closed() {
        let sent = request();
        let anchor = PendingInputAnchor::from_request(&sent)
            .unwrap()
            .unwrap()
            .complete(u64::MAX)
            .unwrap();
        let mut current = sent.messages.clone();
        current.push(ChatMessage::tool_result("one", "tail"));
        assert_eq!(
            InputAnchor::decide(
                Some(&anchor),
                &current,
                &sent.tools,
                "mock",
                sent.reasoning.as_ref(),
                Some(100),
                1
            ),
            Err(ContextEstimateError::Overflow)
        );
    }
}
