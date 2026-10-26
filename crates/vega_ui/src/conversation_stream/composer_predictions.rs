use super::*;
use gpui_kit::Global;
use tokio_util::sync::CancellationToken;
use vega_conversation::types::ComposerPredictionFence;

#[derive(Clone, Copy)]
pub(crate) struct ComposerPredictionsEnabled(pub bool);

impl Default for ComposerPredictionsEnabled {
    fn default() -> Self {
        Self(true)
    }
}

impl Global for ComposerPredictionsEnabled {}

pub fn set_composer_predictions_enabled(enabled: bool, cx: &mut App) {
    if cx
        .try_global::<ComposerPredictionsEnabled>()
        .is_none_or(|current| current.0)
        != enabled
    {
        cx.set_global(ComposerPredictionsEnabled(enabled));
    }
}

pub(crate) struct ComposerPredictionOwner {
    fence: ComposerPredictionFence,
    cancel: CancellationToken,
}

impl Drop for ComposerPredictionOwner {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

impl ConversationStream {
    pub fn composer_prediction_eligible(&self, cx: &App) -> bool {
        !self.pending_queue.busy && self.prediction_environment_eligible(cx)
    }

    fn prediction_environment_eligible(&self, cx: &App) -> bool {
        cx.try_global::<ComposerPredictionsEnabled>()
            .is_none_or(|enabled| enabled.0)
            && self.thread.backend == TaskBackend::Native
            && self.thread.mode != ThreadMode::Plan
            && self.thread.parent_thread_id.is_none()
            && !self.reading_preview
            && !self.delegated_run_active
            && self.input.read(cx).text().is_empty()
            && !self.input.read(cx).is_composing()
            && !self.actions.running
            && !self.composer_submit_pending
            && !self.approved_not_started
            && self.active_agent_message.is_none()
            && self.attachments.is_empty()
            && !self.attachment_import_pending
            && self.pending_queue.messages.is_empty()
            && self.pending_queue.error.is_none()
            && !self.context_operation_busy()
            && !self.trusted_action_busy
            && !self.skill_mutation_pending
            && self.model_selection_pending.is_none()
            && !self.model_selection_save_busy()
            && self.actions.pending_mode.is_none()
            && !self.permission_queue.has_pending()
            && !self.has_pending_user_question()
            && self.active_permission.is_none()
            && self.deferred_permission.is_none()
            && !self.actions.visible()
            && !self.model_picker_level.is_open()
            && !self.permission_picker_open
            && !self.backend_selector_open
            && !self.context_control.open
            && !self.file_selector_wanted
    }

    pub fn begin_composer_prediction(
        &mut self,
        completed_message_id: &str,
        cx: &mut Context<Self>,
    ) -> Option<(ComposerPredictionFence, CancellationToken)> {
        if self.prediction_completed.as_deref() == Some(completed_message_id)
            || !self.composer_prediction_eligible(cx)
        {
            return None;
        }
        self.invalidate_composer_prediction(cx);
        self.prediction_completed = Some(completed_message_id.into());
        let fence = ComposerPredictionFence {
            thread_id: self.thread.id.clone(),
            model: self.displayed_model().to_string(),
            mode: self.thread.mode,
            backend: self.thread.backend,
            completed_message_id: completed_message_id.into(),
            generation: self.prediction_generation,
            input_revision: self.input.read(cx).content_revision(),
        };
        let cancel = CancellationToken::new();
        self.prediction_owner = Some(ComposerPredictionOwner {
            fence: fence.clone(),
            cancel: cancel.clone(),
        });
        Some((fence, cancel))
    }

    pub fn prepare_composer_prediction(
        &mut self,
        completed_message_id: &str,
        cx: &mut Context<Self>,
    ) -> Option<(ComposerPredictionFence, CancellationToken)> {
        let owner = self.begin_composer_prediction(completed_message_id, cx)?;
        self.prediction_waiting_for_queue = true;
        Some(owner)
    }

    pub fn ready_composer_prediction(
        &mut self,
        fence: &ComposerPredictionFence,
        cx: &mut Context<Self>,
    ) -> bool {
        self.prediction_waiting_for_queue = false;
        if self.composer_prediction_matches(fence, cx) {
            return true;
        }
        self.invalidate_composer_prediction(cx);
        false
    }

    pub fn composer_prediction_matches(&self, fence: &ComposerPredictionFence, cx: &App) -> bool {
        self.prediction_owner
            .as_ref()
            .is_some_and(|owner| owner.fence == *fence && !owner.cancel.is_cancelled())
            && self.prediction_generation == fence.generation
            && self.thread.id == fence.thread_id
            && self.displayed_model() == fence.model
            && self.thread.mode == fence.mode
            && self.thread.backend == fence.backend
            && self.input.read(cx).content_revision() == fence.input_revision
            && (!self.pending_queue.busy || self.prediction_waiting_for_queue)
            && self.prediction_environment_eligible(cx)
    }

    pub fn apply_composer_prediction(
        &mut self,
        fence: &ComposerPredictionFence,
        suggestion: String,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.composer_prediction_matches(fence, cx) {
            return false;
        }
        self.input
            .update(cx, |input, cx| input.set_prediction(Some(suggestion), cx));
        cx.notify();
        true
    }

    pub fn invalidate_composer_prediction(&mut self, cx: &mut Context<Self>) {
        self.prediction_generation = self.prediction_generation.wrapping_add(1);
        self.prediction_owner = None;
        self.prediction_waiting_for_queue = false;
        self.input
            .update(cx, |input, cx| input.set_prediction(None, cx));
    }

    pub(crate) fn reconcile_composer_prediction(&mut self, cx: &mut Context<Self>) {
        if self
            .prediction_owner
            .as_ref()
            .is_some_and(|owner| !self.composer_prediction_matches(&owner.fence, cx))
        {
            self.invalidate_composer_prediction(cx);
            cx.notify();
        }
    }
}
