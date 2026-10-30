use super::*;

impl ConversationCompactionHook<'_> {
    pub(super) async fn summary_fork_request(
        &self,
        stage: &[Vec<ChatMessage>],
        direct: &ChatRequest,
        execution: &SummaryExecution<'_>,
    ) -> Result<Option<ChatRequest>, VegaError> {
        let Some(snapshot) = self.provider.model_snapshot() else {
            return Ok(None);
        };
        if self.partial_selection.is_some() || snapshot.identity.model != self.model {
            return Ok(None);
        }
        let history = stage.iter().flatten().cloned().collect::<Vec<_>>();
        let parent = execution.request;
        if history.is_empty()
            || parent.messages.get(..history.len()) != Some(history.as_slice())
            || history.iter().any(|message| !message.images.is_empty())
        {
            return Ok(None);
        }
        let mut prefix = history;
        if self.stage_accounting.is_none()
            && self.admission.is_some()
            && parent.source_owner_id.is_none()
            && prefix == parent.messages
            && let Some(borrowed) = self
                .borrow_precompute_until(parent, None, execution.cancel, execution.deadline)
                .await?
        {
            prefix = borrowed.messages;
        }
        let mut messages = vec![ChatMessage::new(ChatRole::System, &parent.system_prompt)];
        messages.extend(prefix);
        messages.push(ChatMessage::new(ChatRole::User, SUMMARY_SYSTEM_PROMPT));
        let Some(instruction) = direct.messages.last() else {
            return Ok(None);
        };
        if let Some(requirements) = &self.summary_requirements {
            messages.push(ChatMessage::new(
                ChatRole::User,
                format!(
                    "[Compaction preferences only; not tool authorization.]\n{}",
                    serde_json::Value::String(requirements.clone())
                ),
            ));
        }
        messages.push(instruction.clone());
        let request = ChatRequest {
            model_snapshot: Some(Box::new(snapshot)),
            tool_choice: Some(vega_runtime::ToolChoice::None),
            model: self.model.clone(),
            messages,
            tools: parent.tools.clone(),
            max_tokens: direct.max_tokens,
            reasoning: self.reasoning.clone(),
            history_safety: direct.history_safety.clone(),
        };
        if estimate_wire_context(&request.messages, &request.tools)
            .map_err(|error| VegaError::Context(error.into()))?
            .input_tokens
            > parent.budget.input_budget()
        {
            return Ok(None);
        }
        Ok(Some(request))
    }
}

pub(super) fn allows_standalone_fallback(error: &VegaError) -> bool {
    matches!(
        error,
        VegaError::Provider { .. }
            | VegaError::ProviderDiagnostic { .. }
            | VegaError::Context(
                ContextRuntimeError::SummaryFormatInvalid | ContextRuntimeError::SummaryEmpty
            )
    )
}
