use crate::{ChatMessage, ChatRole, ContextRuntimeError, VegaError};

pub const MICRO_MIN_SAVINGS: u64 = 20_000;
pub const MICRO_KEEP_RECENT: usize = 5;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MicroCompactionTarget {
    pub call_id: String,
    pub status: String,
}

#[derive(Clone)]
pub struct MicroCompactionResult {
    pub source_version: u64,
    pub source_fingerprint: String,
    pub targets: Vec<MicroCompactionTarget>,
    pub estimated_savings: u64,
}

pub fn micro_result_content(target: &MicroCompactionTarget) -> String {
    serde_json::json!({"vega_compacted_tool_result": {
        "authority": "none", "call_id": target.call_id, "status": target.status,
        "content": "Old tool result content cleared from model context; original evidence remains in this conversation audit. Do not infer missing content or replay side effects."
    }}).to_string()
}

pub fn apply_micro_compaction(
    messages: &mut [ChatMessage],
    targets: &[MicroCompactionTarget],
) -> Result<(), VegaError> {
    let invalid = || VegaError::Context(ContextRuntimeError::InvalidProjection);
    if targets.is_empty() || targets.len() > 4096 {
        return Err(invalid());
    }
    let mut selected = Vec::new();
    let mut ids = std::collections::HashSet::new();
    for target in targets {
        if !ids.insert(&target.call_id) || !matches!(target.status.as_str(), "success" | "failed") {
            return Err(invalid());
        }
        let calls = messages
            .iter()
            .filter(|message| message.role == ChatRole::Assistant)
            .flat_map(|message| &message.tool_calls)
            .filter(|call| call.id == target.call_id)
            .collect::<Vec<_>>();
        if calls.len() != 1
            || !matches!(
                calls[0].name.as_str(),
                "read" | "Read" | "grep" | "glob" | "bash"
            )
        {
            return Err(invalid());
        }
        let results = messages
            .iter()
            .enumerate()
            .filter(|(_, message)| {
                message.role == ChatRole::Tool
                    && message.tool_call_id.as_deref() == Some(target.call_id.as_str())
            })
            .collect::<Vec<_>>();
        if results.len() != 1 {
            return Err(invalid());
        }
        selected.push((results[0].0, micro_result_content(target)));
    }
    for (index, content) in selected {
        messages[index].replace_micro_result(content);
    }
    Ok(())
}
