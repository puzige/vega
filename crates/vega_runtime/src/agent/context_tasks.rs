use super::*;

pub struct HistoricalTask {
    pub agent_id: String,
    pub message_id: String,
    pub state: String,
    pub release_state: String,
}

pub type HistoricalTaskReader = dyn Fn(&str) -> Option<HistoricalTask> + Send + Sync;

pub(super) async fn restore_context_tasks(
    messages: &mut [ChatMessage],
    config: &RuntimeToolConfig,
    cancel: &CancellationToken,
) -> Result<(), VegaError> {
    let mut seen = HashSet::new();
    let mut bytes = 0usize;
    for message in messages {
        message.clear_context_tasks();
        let Some(reader) = &config.context_task_reader else {
            continue;
        };
        for source in message.context_task_sources().to_vec() {
            if seen.len() >= 16 || !seen.insert(source.clone()) {
                continue;
            }
            if cancel.is_cancelled() {
                return Err(VegaError::Cancelled);
            }
            let reader = Arc::clone(reader);
            let source_id = source.clone();
            let task = tokio::task::spawn_blocking(move || reader(&source_id))
                .await
                .map_err(|_| VegaError::Context(crate::ContextRuntimeError::InvalidProjection))?;
            if cancel.is_cancelled() {
                return Err(VegaError::Cancelled);
            }
            let Some(task) = task else { continue };
            if !matches!(
                task.state.as_str(),
                "running" | "cancelling" | "completed" | "failed" | "interrupted"
            ) || !matches!(
                task.release_state.as_str(),
                "held" | "released" | "recovered_abandoned" | "quarantined" | "unknown"
            ) {
                continue;
            }
            let observation = serde_json::json!({"vega_history_observation": {
                "version": 1, "authority": "none", "kind": "delegated_task_status",
                "source_call_id": source, "agent_id": task.agent_id, "message_id": task.message_id,
                "persisted_state": task.state, "release_state": task.release_state,
                "includes_result": false, "worker_liveness": "unknown",
            }})
            .to_string();
            let observation = config.redact_tool_output(&observation)?;
            if observation.len() <= 2048 && bytes.saturating_add(observation.len()) <= 32768 {
                bytes = bytes.saturating_add(observation.len());
                message.restore_context_task(observation);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
