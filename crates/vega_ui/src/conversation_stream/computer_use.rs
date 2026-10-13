use super::*;
use crate::interaction::InteractionExt;
use vega_conversation::types::{ComputerUseControl, ControlPhase};

impl ConversationStream {
    pub fn set_computer_use_control(
        &mut self,
        control: Option<ComputerUseControl>,
        cx: &mut Context<Self>,
    ) {
        self.computer_use_control = control;
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
            notice = notice.child(error.to_string());
            if error.needs_permissions() {
                notice = notice.child(div().flex().gap_2().children([
                    ("辅助功能设置", "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility", "conversation-accessibility-settings"),
                    ("屏幕录制设置", "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture", "conversation-screen-recording-settings"),
                ].into_iter().map(|(label,url,selector)| div().id(selector).debug_selector(move || selector.into()).tab_stop(true).focusable().cursor_pointer().px_2().py_1().interaction_surface(colors.bg_base,colors.bg_hover,colors.bg_active,true).focus_visible(move |style| style.border_1().border_color(colors.brand_primary)).on_key_down(move |event: &gpui_kit::KeyDownEvent, _, cx| { if matches!(event.keystroke.key.as_str(), "enter" | "space") { cx.open_url(url); cx.stop_propagation(); } }).on_click(move |_, _, cx| cx.open_url(url)).child(label))));
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
