use super::*;
use gpui_kit::Focusable;
use vega_conversation::agent::UserQuestionQueue;
use vega_conversation::types::{ToolCall, USER_QUESTION_TOOL};

impl ConversationStream {
    pub fn user_question_queue(&self) -> UserQuestionQueue {
        self.user_question_queue.clone()
    }

    pub fn has_pending_user_question(&self) -> bool {
        self.user_question_queue.has_pending() || self.active_user_question.is_some()
    }

    pub fn cancel_user_questions(&mut self, cx: &mut Context<Self>) {
        self.user_question_queue.cancel_active();
        self.deferred_user_question = None;
        self.remove_user_question(cx);
    }

    pub(crate) fn install_pending_user_question(&mut self, cx: &mut Context<Self>) {
        let Some(pending) = self
            .deferred_user_question
            .take()
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
        if !card.read(cx).matches_call(&ToolCall {
            id: call_id.clone(),
            tool: USER_QUESTION_TOOL.into(),
            input_json,
        }) {
            return;
        }
        self.remove_user_question(cx);
        let composer_focus = self.input.read(cx).focus_handle(cx);
        self.active_user_question = Some(cx.new(|cx| {
            crate::user_question_panel::UserQuestionPanel::new(pending, composer_focus, cx)
        }));
        self.active_user_question_call_id = Some(call_id);
        if let Some((_, group)) = &self.active_run_activity {
            group.update(cx, |group, cx| group.set_waiting(true, cx));
        }
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

    fn remove_user_question(&mut self, cx: &mut Context<Self>) {
        self.active_user_question = None;
        self.active_user_question_call_id = None;
        if let Some((_, group)) = &self.active_run_activity {
            group.update(cx, |group, cx| group.set_waiting(false, cx));
        }
        cx.notify();
    }
}
