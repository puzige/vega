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
    fn tool_preview_and_reference_share_the_wire_usage_anchor_without_recharging_the_prefix() {
        let pair = |id: &str| {
            let call = crate::RuntimeToolCall {
                id: id.into(),
                name: "read".into(),
                input_json: r#"{"path":"large.txt"}"#.into(),
            };
            let result = crate::RuntimeToolResult {
                call_id: id.into(),
                output: "complete evidence ".repeat(1_000),
                status: crate::RuntimeToolStatus::Success,
                reused: false,
                exit_code: None,
                duration_ms: None,
                truncated: Some(false),
                approval: None,
                remember_rule: None,
            };
            [
                ChatMessage::assistant_with_tools(
                    "",
                    vec![ChatToolCall {
                        id: id.into(),
                        name: "read".into(),
                        input_json: call.input_json.clone(),
                    }],
                ),
                ChatMessage::runtime_tool_result(&call, &result),
            ]
        };
        let mut sent = request();
        sent.messages.extend(pair("first"));
        sent.messages.last_mut().unwrap().attach_progress_reminder();
        let canonical = sent.messages.clone();
        sent.messages = crate::project_model_messages(&canonical).unwrap();
        let anchor = PendingInputAnchor::from_request(&sent)
            .unwrap()
            .unwrap()
            .complete(123)
            .unwrap();
        let mut current = canonical.clone();
        current.extend(pair("second"));
        let projected = crate::project_model_messages(&current).unwrap();
        assert_eq!(
            crate::project_model_messages(&projected).unwrap(),
            projected
        );
        assert_eq!(&projected[..sent.messages.len()], sent.messages.as_slice());
        assert!(
            projected
                .last()
                .unwrap()
                .content
                .contains("vega_tool_result_reference")
        );
        let canonical_decision = InputAnchor::decide(
            Some(&anchor),
            &current,
            &sent.tools,
            &sent.model,
            sent.reasoning.as_ref(),
            sent.max_tokens,
            1,
        )
        .unwrap();
        let projected_decision = InputAnchor::decide(
            Some(&anchor),
            &projected,
            &sent.tools,
            &sent.model,
            sent.reasoning.as_ref(),
            sent.max_tokens,
            1,
        )
        .unwrap();
        assert_eq!(canonical_decision, projected_decision);
        assert_eq!(
            canonical_decision.source,
            super::super::ContextAccountingSource::UsageAnchored
        );
        assert_eq!(canonical_decision.baseline, Some(123));
        let before = crate::estimate_wire_context(&canonical, &sent.tools)
            .unwrap()
            .input_tokens;
        let after = crate::estimate_wire_context(&projected, &sent.tools)
            .unwrap()
            .input_tokens;
        assert_eq!(canonical_decision.incremental, after - before);
        assert_eq!(canonical_decision.predicted, 123 + after - before);
        assert!(canonical_decision.incremental < 1_000);
        let mut changed = current.clone();
        changed[3].content.replace_range(8_000..8_001, "x");
        assert_eq!(
            InputAnchor::decide(
                Some(&anchor),
                &changed,
                &sent.tools,
                &sent.model,
                sent.reasoning.as_ref(),
                sent.max_tokens,
                1
            )
            .unwrap()
            .source,
            super::super::ContextAccountingSource::Estimated
        );
        assert_eq!(
            InputAnchor::decide(
                Some(&anchor),
                &current,
                &sent.tools,
                "changed-model",
                sent.reasoning.as_ref(),
                sent.max_tokens,
                1
            )
            .unwrap()
            .source,
            super::super::ContextAccountingSource::Estimated
        );
        assert_eq!(canonical[3].content, "complete evidence ".repeat(1_000));
    }

    #[test]
    fn automatic_replay_responses_layout_is_estimated_and_invalidates_anchor_on_phase_or_order() {
        let reasoning_item = serde_json::json!({
            "type":"reasoning",
            "id":"rs_1",
            "phase":"analysis",
            "summary":[],
            "encrypted_content":"opaque-replay-payload"
        });
        let message_item = serde_json::json!({
            "type":"message",
            "id":"msg_1",
            "role":"assistant",
            "status":"completed",
            "phase":"final",
            "content":[{"type":"output_text","text":"answer"}]
        });
        let mut assistant = ChatMessage::new(crate::ChatRole::Assistant, "answer");
        assistant.response_reasoning = vec![reasoning_item.clone()];
        assistant.response_replay_layout = Some(crate::ResponseReplayLayout {
            response_id: Some("resp_1".into()),
            slots: vec![
                crate::ResponseReplaySlot::Reasoning(reasoning_item),
                crate::ResponseReplaySlot::Message(message_item),
            ],
        });
        let messages = vec![
            ChatMessage::new(crate::ChatRole::System, "system"),
            assistant.clone(),
        ];
        let wire = crate::estimate_wire_context(&messages, &[]).unwrap();
        let standalone = crate::estimate_chat_context("system", &[assistant.clone()], &[]).unwrap();
        assert_eq!(wire, standalone);
        let mut without_layout = assistant.clone();
        without_layout.response_reasoning.clear();
        without_layout.response_replay_layout = None;
        assert!(
            wire.input_tokens
                > crate::estimate_wire_context(
                    &[
                        ChatMessage::new(crate::ChatRole::System, "system"),
                        without_layout,
                    ],
                    &[]
                )
                .unwrap()
                .input_tokens
        );

        let mut sent = request();
        sent.messages = messages;
        let anchor = PendingInputAnchor::from_request(&sent)
            .unwrap()
            .unwrap()
            .complete(wire.input_tokens)
            .unwrap();
        let mut changed_phase = sent.messages.clone();
        let Some(crate::ResponseReplaySlot::Message(item)) = changed_phase[1]
            .response_replay_layout
            .as_mut()
            .and_then(|layout| layout.slots.get_mut(1))
        else {
            panic!("message replay slot missing");
        };
        item["phase"] = serde_json::json!("commentary");
        assert_eq!(
            InputAnchor::decide(
                Some(&anchor),
                &changed_phase,
                &sent.tools,
                &sent.model,
                sent.reasoning.as_ref(),
                sent.max_tokens,
                1,
            )
            .unwrap()
            .source,
            super::super::ContextAccountingSource::Estimated
        );
        let mut changed_order = sent.messages.clone();
        changed_order[1]
            .response_replay_layout
            .as_mut()
            .unwrap()
            .slots
            .swap(0, 1);
        assert_eq!(
            InputAnchor::decide(
                Some(&anchor),
                &changed_order,
                &sent.tools,
                &sent.model,
                sent.reasoning.as_ref(),
                sent.max_tokens,
                1,
            )
            .unwrap()
            .source,
            super::super::ContextAccountingSource::Estimated
        );
    }
}
