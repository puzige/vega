use super::*;
use crate::cua_onboarding::PreparationRequested;
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
        if !same_control || control.is_none() {
            let outgoing_error = self
                .computer_use_control
                .as_ref()
                .and_then(|current| current.status().error);
            self.computer_use_entry
                .detach(&self.computer_use_task, outgoing_error);
            self.computer_use_diagnostic = None;
            self.computer_use_notice_generation =
                self.computer_use_notice_generation.wrapping_add(1);
        }
        if !same_control {
            self.actions.invalidate_computer_takeover();
        }
        if let Some(error) = control.as_ref().and_then(|current| current.status().error) {
            self.computer_use_entry
                .observe(&self.computer_use_task, error);
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
        outgoing_error: Option<ComputerUseError>,
        cx: &mut Context<Self>,
    ) {
        if context != &self.computer_use_task {
            return;
        }
        self.computer_use_entry.detach(context, outgoing_error);
        self.set_computer_use_control(None, cx);
    }

    pub(super) fn begin_computer_use_task(&mut self, cx: &mut Context<Self>) {
        self.set_computer_use_control(None, cx);
        self.computer_use_task = self.computer_use_entry.begin_task(&self.thread.id);
    }

    fn activate_computer_use_preparation(
        &mut self,
        context: &TaskContext,
        dismiss: bool,
        cx: &mut Context<Self>,
    ) {
        if context != &self.computer_use_task || self.computer_use_entry.visible().is_none() {
            return;
        }
        if dismiss {
            self.computer_use_entry.dismiss(context);
            cx.notify();
        } else {
            cx.emit(PreparationRequested {
                thread_id: self.thread.id.clone(),
                context: context.clone(),
            });
        }
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

    pub(super) fn toggle_computer_use_diagnostic(
        &mut self,
        context: &TaskContext,
        generation: u64,
        key: (u64, String),
        cx: &mut Context<Self>,
    ) {
        if context != &self.computer_use_task
            || self.computer_use_notice_generation != generation
            || !self.computer_use_control.as_ref().is_some_and(|control| {
                let status = control.status();
                status.epoch == key.0
                    && status.error.is_some_and(|error| error.to_string() == key.1)
            })
        {
            return;
        }
        self.computer_use_diagnostic = if self.computer_use_diagnostic.as_ref() == Some(&key) {
            None
        } else {
            Some(key)
        };
        cx.notify();
    }

    pub(crate) fn render_computer_use_notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let preparation = self.render_computer_use_preparation(cx);
        let active = self.render_active_computer_use_notice(cx);
        if preparation.is_none() && active.is_none() {
            return None;
        }
        Some(
            div()
                .w_full()
                .flex_shrink_0()
                .flex()
                .flex_col()
                .children(preparation)
                .children(active)
                .into_any_element(),
        )
    }

    fn render_computer_use_preparation(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.computer_use_entry.visible()?;
        let colors = theme(cx).colors;
        let context = self.computer_use_task.clone();
        Some(
            div()
                .id("conversation-computer-use-preparation")
                .w_full()
                .flex_shrink_0()
                .py_2()
                .flex()
                .gap_2()
                .text_size(px(Typography::BODY))
                .text_color(colors.text_secondary)
                .children(
                    [
                        (
                            "conversation-computer-use-prepare",
                            vega_i18n::t("ui.computer_use_setup"),
                            false,
                        ),
                        (
                            "conversation-computer-use-prepare-dismiss",
                            vega_i18n::t("ui.close_setup_entry"),
                            true,
                        ),
                    ]
                    .into_iter()
                    .map(|(id, label, dismiss)| {
                        let click_context = context.clone();
                        let key_context = context.clone();
                        div()
                            .id(id)
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
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.activate_computer_use_preparation(&click_context, dismiss, cx);
                            }))
                            .on_key_down(cx.listener(
                                move |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                        this.activate_computer_use_preparation(
                                            &key_context,
                                            dismiss,
                                            cx,
                                        );
                                        cx.stop_propagation();
                                    }
                                },
                            ))
                            .child(label)
                    }),
                )
                .into_any_element(),
        )
    }

    fn render_active_computer_use_notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let control = self.computer_use_control.as_ref()?;
        let status = control.status();
        if status.error.is_none()
            && !matches!(
                status.phase,
                ControlPhase::AwaitingApproval
                    | ControlPhase::Paused
                    | ControlPhase::Pausing
                    | ControlPhase::Failed
            )
        {
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
        if let Some(error) = status.error {
            notice = notice.child(div().child(error.user_message()));
            let (accessibility, screen_recording) = error.permission_settings();
            if accessibility || screen_recording {
                notice = notice.child(div().flex().gap_2().children(
                    [
                        (accessibility, vega_i18n::t("ui.accessibility_settings"), "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility", "conversation-accessibility-settings"),
                        (screen_recording, vega_i18n::t("ui.screen_recording_settings"), "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture", "conversation-screen-recording-settings"),
                    ]
                    .into_iter()
                    .filter(|(needed, _, _, _)| *needed)
                    .map(|(_, label, url, selector)| {
                        let click_context = context.clone();
                        let key_context = context.clone();
                        div()
                            .id(selector)
                            .tab_stop(true)
                            .focusable()
                            .cursor_pointer()
                            .px_2()
                            .py_1()
                            .interaction_surface(colors.bg_base, colors.bg_hover, colors.bg_active, true)
                            .focus_visible(move |style| style.border_1().border_color(colors.brand_primary))
                            .on_key_down(cx.listener(move |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                                if this.computer_use_task == key_context
                                    && matches!(event.keystroke.key.as_str(), "enter" | "space")
                                {
                                    cx.open_url(url);
                                    cx.stop_propagation();
                                }
                            }))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if this.computer_use_task == click_context {
                                    cx.open_url(url);
                                }
                            }))
                            .child(label)
                    }),
                ));
            }
            let key = (status.epoch, error.to_string());
            let expanded = self.computer_use_diagnostic.as_ref() == Some(&key);
            let click_key = key.clone();
            let generation = self.computer_use_notice_generation;
            let click_context = context.clone();
            let key_context = context.clone();
            notice = notice.child(
                div()
                    .id("conversation-computer-use-diagnostic-toggle")
                    .track_focus(&self.computer_use_diagnostic_focus)
                    .tab_stop(true)
                    .cursor_pointer()
                    .px_2()
                    .py_1()
                    .interaction_surface(colors.bg_base, colors.bg_hover, colors.bg_active, true)
                    .focus_visible(move |style| style.border_1().border_color(colors.brand_primary))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_computer_use_diagnostic(
                            &click_context,
                            generation,
                            click_key.clone(),
                            cx,
                        );
                    }))
                    .on_key_down(
                        cx.listener(move |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                this.toggle_computer_use_diagnostic(
                                    &key_context,
                                    generation,
                                    key.clone(),
                                    cx,
                                );
                                cx.stop_propagation();
                            }
                        }),
                    )
                    .child(if expanded {
                        vega_i18n::t("ui.hide_diagnostics")
                    } else {
                        vega_i18n::t("ui.view_diagnostics")
                    }),
            );
            if expanded {
                notice = notice.child(
                    div()
                        .text_color(colors.text_tertiary)
                        .child(error.to_string()),
                );
            }
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
