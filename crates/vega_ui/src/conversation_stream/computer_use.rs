use super::*;
use crate::interaction::InteractionExt;
use vega_conversation::types::{ComputerUseControl, ControlPhase};

impl ConversationStream {
    pub fn set_computer_use_control(
        &mut self,
        control: Option<ComputerUseControl>,
        cx: &mut Context<Self>,
    ) {
        if control.is_none() {
            self.computer_use_diagnostic = None;
            self.computer_use_notice_generation =
                self.computer_use_notice_generation.wrapping_add(1);
        }
        self.computer_use_control = control;
        cx.notify();
    }

    pub(super) fn toggle_computer_use_diagnostic(
        &mut self,
        generation: u64,
        key: (u64, String),
        cx: &mut Context<Self>,
    ) {
        if self.computer_use_notice_generation != generation
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
        let mut notice = div()
            .id("conversation-computer-use-notice")
            .debug_selector(|| "conversation-computer-use-notice".into())
            .w_full()
            .flex_shrink_0()
            .py_2()
            .flex()
            .flex_col()
            .gap_2()
            .text_size(px(Typography::BODY))
            .text_color(colors.text_secondary);
        if let Some(error) = status.error {
            notice = notice.child(
                div()
                    .debug_selector(|| "conversation-computer-use-guidance".into())
                    .child(error.user_message()),
            );
            let (accessibility, screen_recording) = error.permission_settings();
            if accessibility || screen_recording {
                notice = notice.child(div().flex().gap_2().children([
                    (accessibility, "辅助功能设置", "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility", "conversation-accessibility-settings"),
                    (screen_recording, "屏幕录制设置", "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture", "conversation-screen-recording-settings"),
                ].into_iter().filter(|(needed, _, _, _)| *needed).map(|(_, label, url, selector)| div().id(selector).debug_selector(move || selector.into()).tab_stop(true).focusable().cursor_pointer().px_2().py_1().interaction_surface(colors.bg_base,colors.bg_hover,colors.bg_active,true).focus_visible(move |style| style.border_1().border_color(colors.brand_primary)).on_key_down(move |event: &gpui_kit::KeyDownEvent, _, cx| { if matches!(event.keystroke.key.as_str(), "enter" | "space") { cx.open_url(url); cx.stop_propagation(); } }).on_click(move |_, _, cx| cx.open_url(url)).child(label))));
            }
            let key = (status.epoch, error.to_string());
            let expanded = self.computer_use_diagnostic.as_ref() == Some(&key);
            let click_key = key.clone();
            let generation = self.computer_use_notice_generation;
            notice = notice.child(
                div()
                    .id("conversation-computer-use-diagnostic-toggle")
                    .debug_selector(|| "conversation-computer-use-diagnostic-toggle".into())
                    .track_focus(&self.computer_use_diagnostic_focus)
                    .tab_stop(true)
                    .cursor_pointer()
                    .px_2()
                    .py_1()
                    .interaction_surface(colors.bg_base, colors.bg_hover, colors.bg_active, true)
                    .focus_visible(move |style| style.border_1().border_color(colors.brand_primary))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_computer_use_diagnostic(generation, click_key.clone(), cx);
                    }))
                    .on_key_down(
                        cx.listener(move |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                this.toggle_computer_use_diagnostic(generation, key.clone(), cx);
                                cx.stop_propagation();
                            }
                        }),
                    )
                    .child(if expanded {
                        "收起诊断详情"
                    } else {
                        "查看诊断详情"
                    }),
            );
            if expanded {
                notice = notice.child(
                    div()
                        .debug_selector(|| "conversation-computer-use-diagnostic".into())
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
            notice = notice
                .child("请确认本任务可以访问这个浏览器标签页：")
                .child(target.label())
                .child(
                    div()
                        .id("conversation-browser-approve")
                        .debug_selector(|| "conversation-browser-approve".into())
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
                            move |_, event: &gpui_kit::KeyDownEvent, _, cx| {
                                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                    let _ = owned.approve_browser(epoch, &target);
                                    cx.notify();
                                    cx.stop_propagation();
                                }
                            }
                        }))
                        .on_click(cx.listener(move |_, _, _, cx| {
                            let _ = owned.approve_browser(epoch, &target);
                            cx.notify();
                        }))
                        .child("允许此标签页"),
                );
        }
        if status.phase == ControlPhase::Paused {
            let owned = control.clone();
            let epoch = status.epoch;
            notice = notice
                .child("电脑操作已暂停。准备好后可继续本任务。")
                .child(
                    div()
                        .id("conversation-computer-resume")
                        .debug_selector(|| "conversation-computer-resume".into())
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
                            move |_, event: &gpui_kit::KeyDownEvent, _, cx| {
                                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                    let _ = owned.resume(epoch);
                                    cx.notify();
                                    cx.stop_propagation();
                                }
                            }
                        }))
                        .on_click(cx.listener(move |_, _, _, cx| {
                            let _ = owned.resume(epoch);
                            cx.notify();
                        }))
                        .child("继续"),
                );
        } else if status.phase == ControlPhase::Pausing {
            notice = notice.child("正在交还控制…");
        }
        Some(notice.into_any_element())
    }
}
