use super::*;
use crate::cua_onboarding::lifecycle::TaskContext;
use vega_conversation::types::{ComputerUseControl, ComputerUseError};

mod presentation;

impl ConversationStream {
    pub fn set_computer_use_control(
        &mut self,
        control: Option<ComputerUseControl>,
        cx: &mut Context<Self>,
    ) {
        let same_control = match (&self.computer_use_control, &control) {
            (Some(current), Some(next)) => current.same_instance(next),
            (None, None) => true,
            _ => false,
        };
        if !same_control {
            self.actions.invalidate_computer_takeover();
        }
        self.computer_use_control = control;
        cx.notify();
    }

    pub fn computer_use_task_context(&self) -> TaskContext {
        self.computer_use_task.clone()
    }

    pub fn detach_computer_use_control(
        &mut self,
        context: &TaskContext,
        _: Option<ComputerUseError>,
        cx: &mut Context<Self>,
    ) {
        if context != &self.computer_use_task {
            return;
        }
        self.set_computer_use_control(None, cx);
    }

    pub(super) fn begin_computer_use_task(&mut self, cx: &mut Context<Self>) {
        self.set_computer_use_control(None, cx);
        self.computer_use_task = TaskContext::new(&self.thread.id);
    }

    fn owns_computer_use_control(
        &self,
        context: &TaskContext,
        control: &ComputerUseControl,
    ) -> bool {
        context == &self.computer_use_task
            && self
                .computer_use_control
                .as_ref()
                .is_some_and(|current| current.same_instance(control))
    }

    pub(crate) fn render_computer_use_notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.computer_use_control.as_ref()?;
        Some(
            presentation::NativeNotice {
                owner: cx.weak_entity(),
            }
            .into_any_element(),
        )
    }
}
