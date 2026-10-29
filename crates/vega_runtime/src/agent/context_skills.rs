use super::*;

pub(super) async fn restore_context_skills(
    messages: &mut [ChatMessage],
    config: &RuntimeToolConfig,
    cancel: &CancellationToken,
) -> Result<(), VegaError> {
    let mut names = config
        .active_skill_summaries()?
        .into_iter()
        .map(|skill| skill.name)
        .collect::<HashSet<_>>();
    let mut runs = HashSet::new();
    let mut tokens = 0usize;
    for message in messages {
        message.clear_context_skills();
        let Some(reader) = &config.context_skill_reader else {
            continue;
        };
        for run_id in message.context_skill_runs().to_vec() {
            if !runs.insert(run_id.clone()) {
                continue;
            }
            if cancel.is_cancelled() {
                return Err(VegaError::Cancelled);
            }
            let reader = Arc::clone(reader);
            let source_run = run_id.clone();
            let bodies = tokio::task::spawn_blocking(move || reader(&source_run))
                .await
                .map_err(|_| VegaError::Context(crate::ContextRuntimeError::InvalidProjection))?;
            if cancel.is_cancelled() {
                return Err(VegaError::Cancelled);
            }
            for (skill, body) in bodies {
                if !names.insert(skill.name.clone()) {
                    continue;
                }
                let size = body.len().div_ceil(4);
                let content =
                    (size <= 5000 && tokens.saturating_add(size) <= 25000).then_some(body);
                if content.is_some() {
                    tokens = tokens.saturating_add(size);
                }
                let observation = serde_json::json!({"vega_history_observation": {
                    "version": 1, "authority": "none",
                    "kind": if content.is_some() { "restored_skill" } else { "compact_skill_reference" },
                    "source_run_id": run_id, "name": skill.name,
                    "source_label": skill.source_label, "content_sha256": skill.content_sha256,
                    "content": content,
                }}).to_string();
                message.restore_context_skill(config.redact_tool_output(&observation)?);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
