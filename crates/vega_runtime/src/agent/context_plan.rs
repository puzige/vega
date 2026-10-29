use super::*;

pub struct HistoricalPlan {
    pub status: String,
    pub review_note: Option<String>,
    pub reviewed_at: Option<i64>,
    pub content: Option<String>,
}

pub type HistoricalPlanReader = dyn Fn(&str) -> Option<HistoricalPlan> + Send + Sync;

pub(super) async fn restore_context_plan(
    messages: &mut [ChatMessage],
    config: &RuntimeToolConfig,
    cancel: &CancellationToken,
) -> Result<(), VegaError> {
    let mut restored = false;
    for message in messages {
        message.restore_context_plan(None);
        if restored {
            continue;
        }
        let Some(source) = message.context_plan_source().map(str::to_owned) else {
            continue;
        };
        let Some(reader) = &config.context_plan_reader else {
            continue;
        };
        if cancel.is_cancelled() {
            return Err(VegaError::Cancelled);
        }
        let reader = Arc::clone(reader);
        let source_id = source.clone();
        let plan = tokio::task::spawn_blocking(move || reader(&source_id))
            .await
            .map_err(|_| VegaError::Context(crate::ContextRuntimeError::InvalidProjection))?;
        if cancel.is_cancelled() {
            return Err(VegaError::Cancelled);
        }
        let Some(plan) = plan else { continue };
        let content = plan.content.filter(|content| content.len() <= 20000);
        let observation = serde_json::json!({"vega_history_observation": {
            "version": 1, "authority": "none",
            "kind": if content.is_some() { "restored_plan" } else { "compact_plan_reference" },
            "source_message_id": source, "review_status": plan.status,
            "review_note": plan.review_note.filter(|note| note.len() <= 4096),
            "reviewed_at": plan.reviewed_at, "content": content,
        }})
        .to_string();
        message.restore_context_plan(Some(config.redact_tool_output(&observation)?));
        restored = true;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
