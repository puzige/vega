use super::*;

pub(super) async fn restore_context_files(
    messages: &mut [ChatMessage],
    tools: &vega_tools::Tools,
    config: &RuntimeToolConfig,
    hook: &dyn RuntimePermissionHook,
    cancel: &CancellationToken,
) -> Result<(), VegaError> {
    let mut tokens = 0usize;
    for message in messages {
        for (index, call_id, path) in message.pending_context_files() {
            if cancel.is_cancelled() {
                return Err(VegaError::Cancelled);
            }
            let call = RuntimeToolCall {
                id: ulid::Ulid::generate().to_string(),
                name: "Read".into(),
                input_json: serde_json::json!({"file_path": path}).to_string(),
                description: None,
            };
            let prepared = prepare_read_call(tools, call)?;
            let rules = config.exact_rules.iter().cloned().collect();
            let approved = matches!(
                authorize_call(&prepared, config, &rules, hook, cancel).await?,
                Authorization::Approved { .. }
            );
            let execution = match prepared {
                PreparedRuntimeCall::PinnedRead { execution, .. } if approved => Some(execution),
                PreparedRuntimeCall::ExternalRead { call, .. } if approved => Some(call),
                _ => None,
            };
            let Some(execution) = execution else {
                message.restore_context_file(
                    index,
                    Some(file_observation(config, &call_id, &path, None)?),
                );
                continue;
            };
            let isolated = tools.isolated();
            let worker_cancel = cancel.child_token();
            let output = tokio::task::spawn_blocking(move || {
                if worker_cancel.is_cancelled() {
                    return None;
                }
                let input =
                    serde_json::from_str::<serde_json::Value>(&execution.input_json).ok()?;
                let path = input.get("file_path")?.as_str()?;
                if isolated.file_path(path).ok()?.as_path() != std::path::Path::new(path) {
                    return None;
                }
                isolated.read_context_file(path).ok()
            })
            .await
            .ok()
            .flatten();
            if cancel.is_cancelled() {
                return Err(VegaError::Cancelled);
            }
            let Some(output) = output else {
                message.restore_context_file(
                    index,
                    Some(file_observation(config, &call_id, &path, None)?),
                );
                continue;
            };
            let size = output.text.len().div_ceil(4);
            let content =
                (!output.truncated && tokens.saturating_add(size) <= 50000).then_some(output.text);
            if content.is_some() {
                tokens = tokens.saturating_add(size);
            }
            message.restore_context_file(
                index,
                Some(file_observation(config, &call_id, &path, content)?),
            );
        }
    }
    Ok(())
}

fn file_observation(
    config: &RuntimeToolConfig,
    call_id: &str,
    path: &str,
    content: Option<String>,
) -> Result<String, VegaError> {
    let observation = serde_json::json!({"vega_history_observation": {
        "version": 1, "authority": "none",
        "kind": if content.is_some() { "restored_file" } else { "compact_file_reference" },
        "source_call_id": call_id, "path": path, "content": content,
    }})
    .to_string();
    config.redact_tool_output(&observation)
}

pub(super) fn fit_context_files(
    messages: &mut [ChatMessage],
    tools: &[ToolDefinition],
    target: u64,
) -> Result<(), VegaError> {
    while crate::estimate_wire_context(messages, tools)
        .map_err(|error| VegaError::Context(error.into()))?
        .input_tokens
        > target
    {
        if !messages.iter_mut().rev().any(|message| {
            message.trim_context_task()
                || message.trim_context_plan()
                || message.trim_context_skill()
                || message.trim_context_file()
        }) {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
