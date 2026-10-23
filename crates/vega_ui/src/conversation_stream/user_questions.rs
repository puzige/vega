use super::*;
use gpui_kit::Focusable;
use vega_conversation::agent::UserQuestionQueue;
use vega_conversation::types::{
    ToolCall, USER_QUESTION_TOOL, UserQuestionAttention, UserQuestionResolution,
};

impl EventEmitter<UserQuestionAttention> for ConversationStream {}

impl ConversationStream {
    #[cfg(feature = "question-test-support")]
    pub fn question_test_label(&self, cx: &gpui_kit::App) -> Option<String> {
        self.active_run_activity
            .as_ref()
            .map(|(_, group)| group.read(cx).question_test_label())
    }

    #[cfg(feature = "question-test-support")]
    pub fn question_test_run_label(&self, message_id: &str, cx: &gpui_kit::App) -> Option<String> {
        self.run_activity_groups
            .get(message_id)
            .map(|group| group.read(cx).question_test_label())
    }

    #[cfg(feature = "question-test-support")]
    pub fn question_test_panel_present(&self) -> bool {
        self.active_user_question.is_some()
    }

    #[cfg(feature = "question-test-support")]
    pub fn question_test_submission(
        &self,
        cx: &gpui_kit::App,
    ) -> Option<(
        bool,
        Vec<Option<vega_conversation::types::UserQuestionAnswer>>,
    )> {
        self.active_user_question
            .as_ref()
            .map(|panel| panel.read(cx).question_test_submission(cx))
    }

    #[cfg(feature = "question-test-support")]
    pub fn question_test_panel(&self) -> Option<gpui_kit::AnyEntity> {
        self.active_user_question
            .as_ref()
            .map(|panel| panel.clone().into_any())
    }

    #[cfg(feature = "question-test-support")]
    pub fn question_test_answer(
        &mut self,
        skip: bool,
        window: &mut gpui_kit::Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(panel) = &self.active_user_question {
            panel.update(cx, |panel, cx| panel.question_test_answer(skip, window, cx));
        }
    }

    #[cfg(feature = "question-test-support")]
    pub fn question_test_focus_composer(
        &self,
        window: &mut gpui_kit::Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::FocusHandle {
        let focus = self.input.read(cx).focus_handle(cx);
        focus.focus(window, cx);
        focus
    }

    #[cfg(feature = "question-test-support")]
    pub fn question_test_reconcile(&mut self, cx: &mut Context<Self>) {
        self.install_pending_user_question(cx);
    }

    pub fn user_question_queue(&self) -> UserQuestionQueue {
        self.user_question_queue.clone()
    }

    pub fn has_pending_user_question(&self) -> bool {
        self.user_question_queue.has_pending() || self.active_user_question.is_some()
    }

    pub fn pending_user_question_id(
        &self,
        cx: &gpui_kit::App,
    ) -> Option<vega_conversation::types::UserQuestionId> {
        self.user_question_attention.clone().filter(|_| {
            self.active_user_question
                .as_ref()
                .is_some_and(|panel| panel.read(cx).resolution() == UserQuestionResolution::Pending)
        })
    }

    pub fn cancel_user_questions(&mut self, cx: &mut Context<Self>) {
        self.user_question_queue.cancel_active();
        self.deferred_user_question = None;
        self.remove_user_question(cx);
    }

    pub(crate) fn install_pending_user_question(&mut self, cx: &mut Context<Self>) {
        if let Some(panel) = &self.active_user_question {
            match panel.read(cx).resolution() {
                UserQuestionResolution::Pending => {}
                UserQuestionResolution::Answered => self.finish_question_attention(cx),
                UserQuestionResolution::Cancelled => self.remove_user_question(cx),
            }
        }
        let Some(pending) = self
            .deferred_user_question
            .take()
            .filter(|pending| !pending.is_resolved())
            .or_else(|| self.user_question_queue.take_pending())
        else {
            return;
        };
        if pending.is_resolved() {
            return;
        }
        let request = pending.request();
        let call_id = request.call_id.clone();
        let Some(card) = self.tool_cards.get(&call_id) else {
            self.deferred_user_question = Some(pending);
            return;
        };
        let Ok(input_json) = request.input.to_json() else {
            return;
        };
        if !card.read(cx).matches_pending_user_question(&ToolCall {
            id: call_id.clone(),
            tool: USER_QUESTION_TOOL.into(),
            input_json,
        }) {
            return;
        }
        self.remove_user_question(cx);
        let request_id = pending.id();
        let composer_focus = self.input.read(cx).focus_handle(cx);
        self.active_user_question = Some(cx.new(|cx| {
            crate::user_question_panel::UserQuestionPanel::new(pending, composer_focus, cx)
        }));
        self.active_user_question_call_id = Some(call_id);
        self.user_question_attention = Some(request_id.clone());
        if let Some((_, group)) = &self.active_run_activity {
            group.update(cx, |group, cx| group.set_question_waiting(true, cx));
        }
        cx.emit(UserQuestionAttention::Pending { request_id });
        cx.notify();
    }

    pub(crate) fn finish_user_question(&mut self, call_id: &str, cx: &mut Context<Self>) {
        if self
            .deferred_user_question
            .as_ref()
            .is_some_and(|pending| pending.request().call_id == call_id)
        {
            self.deferred_user_question = None;
        }
        if self.active_user_question_call_id.as_deref() == Some(call_id) {
            self.remove_user_question(cx);
        }
    }

    fn finish_question_attention(&mut self, cx: &mut Context<Self>) {
        if let Some(request_id) = self.user_question_attention.take() {
            if let Some((_, group)) = &self.active_run_activity {
                group.update(cx, |group, cx| group.set_question_waiting(false, cx));
            }
            cx.emit(UserQuestionAttention::Resolved { request_id });
            cx.notify();
        }
    }

    fn remove_user_question(&mut self, cx: &mut Context<Self>) {
        self.finish_question_attention(cx);
        self.active_user_question = None;
        self.active_user_question_call_id = None;
        cx.notify();
    }
}
