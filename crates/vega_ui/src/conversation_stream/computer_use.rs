use super::*;
use crate::cua_onboarding::lifecycle::TaskContext;
use crate::interaction::InteractionExt;
use vega_conversation::types::{ComputerUseControl, ComputerUseError, ControlPhase};

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

    fn pause_computer_use(
        &mut self,
        context: &TaskContext,
        control: &ComputerUseControl,
        message_id: &str,
        version: (u64, u64),
        cx: &mut Context<Self>,
    ) {
        if self.owns_computer_use_control(context, control)
            && self.actions.control_generation == version.1
            && self
                .active_agent_message
                .as_ref()
                .is_some_and(|(current, _)| current == message_id)
        {
            let _ = control.pause_at(version.0);
            cx.notify();
        }
    }

    pub(crate) fn render_computer_use_notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let control = self.computer_use_control.as_ref()?;
        let status = control.status();
        if !matches!(
            status.phase,
            ControlPhase::Running
                | ControlPhase::AwaitingApproval
                | ControlPhase::Paused
                | ControlPhase::Pausing
        ) {
            return None;
        }
        let colors = theme(cx).colors;
        let context = self.computer_use_task.clone();
        let mut notice = div()
            .id("conversation-computer-use-notice")
            .w_full()
            .flex_shrink_0()
            .py_2()
            .flex()
            .flex_col()
            .gap_2()
            .text_size(px(Typography::BODY))
            .text_color(colors.text_secondary);
        if status.phase == ControlPhase::Running
            && let Some((message_id, _)) = self.active_agent_message.as_ref()
        {
            let owned = control.clone();
            let epoch = status.epoch;
            let generation = self.actions.control_generation;
            let message_id = message_id.clone();
            let click_context = context.clone();
            let key_context = context.clone();
            let key_message = message_id.clone();
            notice = notice.child(
                div()
                    .id("conversation-computer-takeover")
                    .role(gpui_kit::Role::Button)
                    .aria_label(vega_i18n::t("ui.take_over_computer_control"))
                    .tab_stop(true)
                    .focusable()
                    .cursor_pointer()
                    .px_2()
                    .py_1()
                    .interaction_surface(colors.bg_base, colors.bg_hover, colors.bg_active, true)
                    .focus_visible(move |style| style.border_1().border_color(colors.brand_primary))
                    .on_key_down(cx.listener({
                        let owned = owned.clone();
                        move |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                this.pause_computer_use(
                                    &key_context,
                                    &owned,
                                    &key_message,
                                    (epoch, generation),
                                    cx,
                                );
                                cx.stop_propagation();
                            }
                        }
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.pause_computer_use(
                            &click_context,
                            &owned,
                            &message_id,
                            (epoch, generation),
                            cx,
                        );
                    }))
                    .child(vega_i18n::t("ui.take_over_computer_control")),
            );
        }
        if status.phase == ControlPhase::AwaitingApproval
            && let Some(target) = status.browser_target
        {
            let owned = control.clone();
            let epoch = status.epoch;
            let click_context = context.clone();
            let key_context = context.clone();
            notice = notice
                .child(vega_i18n::t(
                    "ui.confirm_that_this_task_may_access_this_browser",
                ))
                .child(target.label())
                .child(
                    div()
                        .id("conversation-browser-approve")
                        .tab_stop(true)
                        .focusable()
                        .cursor_pointer()
                        .px_2()
                        .py_1()
                        .interaction_surface(
                            colors.bg_base,
                            colors.bg_hover,
                            colors.bg_active,
                            true,
                        )
                        .focus_visible(move |style| {
                            style.border_1().border_color(colors.brand_primary)
                        })
                        .on_key_down(cx.listener({
                            let owned = owned.clone();
                            let target = target.clone();
                            move |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                                if this.owns_computer_use_control(&key_context, &owned)
                                    && matches!(event.keystroke.key.as_str(), "enter" | "space")
                                {
                                    let _ = owned.approve_browser(epoch, &target);
                                    cx.notify();
                                    cx.stop_propagation();
                                }
                            }
                        }))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if this.owns_computer_use_control(&click_context, &owned) {
                                let _ = owned.approve_browser(epoch, &target);
                                cx.notify();
                            }
                        }))
                        .child(vega_i18n::t("ui.allow_this_tab")),
                );
        }
        if status.phase == ControlPhase::Paused {
            let owned = control.clone();
            let epoch = status.epoch;
            let click_context = context.clone();
            let key_context = context.clone();
            notice = notice
                .child(vega_i18n::t(
                    "ui.computer_control_is_paused_resume_this_task_when",
                ))
                .child(
                    div()
                        .id("conversation-computer-resume")
                        .tab_stop(true)
                        .focusable()
                        .cursor_pointer()
                        .px_2()
                        .py_1()
                        .interaction_surface(
                            colors.bg_base,
                            colors.bg_hover,
                            colors.bg_active,
                            true,
                        )
                        .focus_visible(move |style| {
                            style.border_1().border_color(colors.brand_primary)
                        })
                        .on_key_down(cx.listener({
                            let owned = owned.clone();
                            move |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                                if this.owns_computer_use_control(&key_context, &owned)
                                    && matches!(event.keystroke.key.as_str(), "enter" | "space")
                                {
                                    let _ = owned.resume(epoch);
                                    cx.notify();
                                    cx.stop_propagation();
                                }
                            }
                        }))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if this.owns_computer_use_control(&click_context, &owned) {
                                let _ = owned.resume(epoch);
                                cx.notify();
                            }
                        }))
                        .child(vega_i18n::t("ui.continue")),
                );
        } else if status.phase == ControlPhase::Pausing {
            notice = notice.child(vega_i18n::t("ui.returning_control"));
        }
        Some(notice.into_any_element())
    }
}
