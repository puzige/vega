use super::GuidanceCard;
use crate::cua_onboarding::controller::{CardAction, CardView, CheckState};
use crate::cua_onboarding::session::CleanupStatus;
use crate::icons::{Icon, icon};
use crate::interaction::InteractionExt;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Context, ExternalDragPayload, FileDragPaths, KeyDownEvent, Render, Window,
    div, px,
};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use vega_conversation::types::{ComponentState, DirectCapture, PermissionState};
use vega_theme::{Layout, ThemeColors, Typography, theme};

#[derive(Clone)]
struct AppDrag {
    bundle: Rc<RefCell<Option<PathBuf>>>,
}

struct DragPreview;

impl Render for DragPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        div()
            .flex()
            .items_center()
            .gap(px(Layout::IMAGE_PREVIEW_GAP))
            .p(px(Layout::CONTENT_PADDING))
            .rounded(px(Layout::PANEL_RADIUS))
            .bg(colors.bg_elevated)
            .text_color(colors.text_primary)
            .text_size(px(Typography::BODY))
            .child(icon(Icon::Monitor, colors.text_secondary))
            .child("Vega.app")
    }
}

impl GuidanceCard {
    fn action_button(
        &self,
        id: &'static str,
        label: &'static str,
        action: CardAction,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let focus = match action {
            CardAction::RevealInFinder => Some(&self.body_action_focuses[0]),
            CardAction::AccessibilitySettings => Some(&self.body_action_focuses[1]),
            CardAction::ScreenRecordingSettings => Some(&self.body_action_focuses[2]),
            _ => None,
        };
        div()
            .id(id)
            .role(gpui_kit::Role::Button)
            .aria_label(label)
            .tab_stop(enabled)
            .focusable()
            .when_some(focus, |button, focus| {
                button.track_focus(&focus.clone().tab_stop(enabled))
            })
            .px(px(Layout::IMAGE_PREVIEW_GAP))
            .py(px(Layout::TITLEBAR_CONTROL_GAP))
            .rounded(px(Layout::PANEL_RADIUS))
            .text_color(if enabled {
                colors.text_primary
            } else {
                colors.text_tertiary
            })
            .when(enabled, |button| button.cursor_pointer())
            .interaction_surface(
                colors.bg_elevated,
                colors.bg_hover,
                colors.bg_active,
                enabled,
            )
            .focus_visible(move |style| style.border_1().border_color(colors.brand_primary))
            .on_click(cx.listener(move |card, _, window, cx| {
                if enabled {
                    card.dispatch(action, window, cx);
                }
            }))
            .child(label)
            .into_any_element()
    }

    pub(super) fn reveal_body_focus(&self, window: &Window, cx: &mut Context<Self>) {
        if let Some(index) = self
            .body_action_focuses
            .iter()
            .position(|focus| focus.is_focused(window))
        {
            self.body_scroll
                .scroll_to_item(self.body_action_items[index]);
            cx.notify();
        }
    }

    fn scroll_body(&self, key: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.body_focus.contains_focused(window, cx) {
            return false;
        }
        let viewport = self.body_scroll.bounds().size.height;
        let max = self.body_scroll.max_offset().y.max(px(0.));
        let current = self.body_scroll.offset();
        let line = px(Typography::BODY * Typography::BODY_LINE_HEIGHT);
        let target = match key {
            "up" => current.y + line,
            "down" => current.y - line,
            "pageup" => current.y + viewport,
            "pagedown" => current.y - viewport,
            "home" => px(0.),
            "end" => -max,
            _ => return false,
        };
        self.body_scroll
            .set_offset(gpui_kit::point(current.x, target.clamp(-max, px(0.))));
        window.focus(&self.body_focus, cx);
        cx.notify();
        true
    }

    fn app_row(&self, view: &CardView, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let mut app = div()
            .id("preparation-app")
            .flex()
            .items_center()
            .gap(px(Layout::IMAGE_PREVIEW_GAP))
            .child(icon(Icon::Monitor, colors.text_secondary))
            .child(if view.can_drag {
                "Vega.app · 拖到权限列表"
            } else {
                "当前运行的 Vega"
            });
        if view.can_drag {
            let drag = AppDrag {
                bundle: Rc::new(RefCell::new(None)),
            };
            let weak = cx.weak_entity();
            let payload_owner = weak.clone();
            app = app
                .cursor_pointer()
                .on_drag(drag, move |drag, _, _, cx| {
                    let bundle = weak
                        .update(cx, |card, cx| {
                            let bundle = card.controller.begin_drag();
                            cx.notify();
                            bundle
                        })
                        .ok()
                        .flatten();
                    *drag.bundle.borrow_mut() = bundle;
                    cx.new(|_| DragPreview)
                })
                .external_drag_payload(move |drag: &AppDrag, _, cx: &mut App| {
                    if !payload_owner
                        .read_with(cx, |card, _| card.controller.view().open)
                        .unwrap_or(false)
                    {
                        return None;
                    }
                    let bundle = drag.bundle.borrow().clone()?;
                    Some(ExternalDragPayload::Files(FileDragPaths::new([(
                        bundle, true,
                    )])))
                });
        }
        div()
            .flex()
            .flex_col()
            .gap(px(Layout::IMAGE_PREVIEW_GAP))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(Layout::IMAGE_PREVIEW_GAP))
                    .child(app)
                    .child(self.action_button(
                        "preparation-finder",
                        "在 Finder 中显示",
                        CardAction::RevealInFinder,
                        view.open,
                        cx,
                    )),
            )
            .when(!view.can_drag, |row| {
                row.child(
                    div()
                        .text_color(colors.text_secondary)
                        .child("当前可执行文件不在 .app 包中，无法作为应用拖入权限列表。"),
                )
            })
            .into_any_element()
    }

    fn permission_row(
        &self,
        label: &'static str,
        state: PermissionState,
        action: CardAction,
        view: &CardView,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let (id, button_label) = match action {
            CardAction::AccessibilitySettings => ("preparation-accessibility", "辅助功能设置"),
            _ => ("preparation-screen-recording", "屏幕录制设置"),
        };
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(Layout::IMAGE_PREVIEW_GAP))
            .child(div().flex().flex_col().child(label).child(
                div().text_color(colors.text_secondary).child(match state {
                    PermissionState::Granted => "已开启",
                    PermissionState::Denied => "未授权",
                    PermissionState::Unknown => "未知",
                }),
            ))
            .child(self.action_button(id, button_label, action, view.open, cx))
            .into_any_element()
    }

    fn feedback(&self, view: &CardView, colors: ThemeColors) -> Vec<AnyElement> {
        let mut feedback = Vec::new();
        let check = match view.check_state {
            CheckState::Idle => Some("尚未检测，请重新检测权限状态。".into()),
            CheckState::Checking => Some("正在检测权限…".into()),
            CheckState::Observed => None,
            CheckState::Busy => {
                Some("另一个任务正在使用电脑操控。请先停止该任务，再重新检测。".into())
            }
            CheckState::Unknown => Some("部分权限状态未知。请检查系统设置后重新检测。".into()),
            CheckState::Failed(error) => {
                Some(format!("{}。处理后可重新检测。", error.user_message()))
            }
        };
        if let Some(message) = check {
            feedback.push(div().child(message).into_any_element());
        }
        if let Some(error) = &self.adapter_error {
            feedback.push(
                div()
                    .text_color(colors.danger)
                    .child(error.clone())
                    .into_any_element(),
            );
        }
        if let Some(error) = &self.service_status.failure {
            feedback.push(
                div()
                    .text_color(colors.danger)
                    .child(error.clone())
                    .into_any_element(),
            );
        }
        if let Some(error) = self.service_status.last_close_error {
            let recovery = if self.service_status.cleanup == CleanupStatus::Clear {
                "后续回收已完成，可重新检测。"
            } else {
                "回收尚未确认，请重新检测以重试。"
            };
            feedback.push(
                div()
                    .text_color(colors.warning)
                    .child(format!(
                        "关闭检测时发生错误：{}。{recovery}",
                        error.user_message()
                    ))
                    .into_any_element(),
            );
        }
        match self.service_status.cleanup {
            CleanupStatus::Clear => {}
            CleanupStatus::Reclaiming => feedback.push(
                div().child("正在回收准备检测资源…").into_any_element(),
            ),
            CleanupStatus::Failed(error) => feedback.push(
                div()
                    .text_color(colors.danger)
                    .child(format!("准备检测回收失败：{}。电脑操控可能仍被占用。请重新检测以重试；仍失败时请重启 Vega。", error.user_message()))
                    .into_any_element(),
            ),
        }
        if let Some(message) = &self.external_dispatch {
            feedback.push(
                div()
                    .text_color(colors.text_secondary)
                    .child(message.clone())
                    .into_any_element(),
            );
        }
        if let Some(fault) = self.panel_fault {
            feedback.push(
                div()
                    .text_color(colors.warning)
                    .child(fault.message())
                    .into_any_element(),
            );
        }
        feedback
    }
}

impl Render for GuidanceCard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        let view = self.controller.view();
        let feedback = self.feedback(&view, colors);
        let body_action_items = [feedback.len(), feedback.len() + 2, feedback.len() + 3];
        if self.body_action_items != body_action_items {
            self.body_action_items = body_action_items;
            self.reveal_body_focus(window, cx);
        }
        let heading = div()
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_between()
            .child(
                div()
                    .text_size(px(Typography::HEADING_BLOCK))
                    .font_weight(Typography::HEADING_BLOCK_WEIGHT)
                    .child("电脑操控准备"),
            )
            .child(self.action_button(
                "preparation-close",
                "关闭",
                CardAction::Close,
                view.open,
                cx,
            ));
        let mut footer = div()
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_end()
            .gap(px(Layout::IMAGE_PREVIEW_GAP))
            .child(self.action_button(
                "preparation-recheck",
                "重新检测",
                CardAction::Recheck,
                view.can_recheck,
                cx,
            ));
        if self.has_task {
            footer = footer.child(self.action_button(
                "preparation-return",
                "返回任务",
                CardAction::ReturnToTask,
                view.can_return_to_task,
                cx,
            ));
        }
        let body = div()
            .id("preparation-scroll-body")
            .tab_stop(true)
            .track_focus(&self.body_focus)
            .track_scroll(&self.body_scroll)
            .role(gpui_kit::Role::ScrollView)
            .aria_label("准备状态与操作")
            .focus_visible(move |style| style.border_1().border_color(colors.brand_primary))
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(Layout::CONTENT_PADDING))
            .children(feedback)
            .child(self.app_row(&view, cx))
            .child(
                div().flex().justify_between().child("电脑操控组件").child(
                    div()
                        .text_color(colors.text_secondary)
                        .child(match view.component {
                            None => "未检测",
                            Some(ComponentState::Available) => "可用",
                            Some(ComponentState::Missing) => "未安装",
                            Some(ComponentState::Incompatible) => "版本不兼容",
                            Some(ComponentState::UnsupportedPlatform) => "系统不支持",
                        }),
                ),
            )
            .child(self.permission_row(
                "辅助功能",
                view.accessibility,
                CardAction::AccessibilitySettings,
                &view,
                cx,
            ))
            .child(self.permission_row(
                "屏幕录制",
                view.screen_recording,
                CardAction::ScreenRecordingSettings,
                &view,
                cx,
            ))
            .child(
                div()
                    .text_color(colors.text_secondary)
                    .child("修改屏幕录制权限后，如系统提示重启，请退出并重新打开 Vega。"),
            )
            .child(
                div().flex().justify_between().child("实际屏幕抓取").child(
                    div()
                        .text_color(colors.text_secondary)
                        .child(match view.direct_capture {
                            DirectCapture::NotChecked => "未验证",
                            DirectCapture::NotReady(_) => "未就绪",
                            DirectCapture::Ready => "已验证",
                        }),
                ),
            )
            .child(
                div()
                    .flex()
                    .justify_between()
                    .child("浏览器扩展")
                    .child(div().text_color(colors.text_secondary).child("未评估")),
            );
        div()
            .id("preparation-card-window")
            .size_full()
            .track_focus(&self.focus)
            .capture_key_down(cx.listener(|card, event: &KeyDownEvent, window, cx| {
                match event.keystroke.key.as_str() {
                    "escape" => {
                        card.dispatch(CardAction::Close, window, cx);
                        cx.stop_propagation();
                    }
                    "tab" => {
                        if event.keystroke.modifiers.shift {
                            window.focus_prev(cx);
                        } else {
                            window.focus_next(cx);
                        }
                        card.reveal_body_focus(window, cx);
                        cx.stop_propagation();
                    }
                    key if !event.keystroke.modifiers.modified()
                        && card.scroll_body(key, window, cx) =>
                    {
                        cx.stop_propagation();
                    }
                    _ => {}
                }
            }))
            .p(px(Layout::PREPARATION_CARD_SHADOW_PADDING))
            .text_size(px(Typography::BODY))
            .line_height(gpui_kit::relative(Typography::BODY_LINE_HEIGHT))
            .text_color(colors.text_primary)
            .child(
                div()
                    .w(px(Layout::PREPARATION_CARD_WIDTH))
                    .h(px(Layout::PREPARATION_CARD_HEIGHT))
                    .p(px(Layout::CONTENT_PADDING))
                    .rounded(px(Layout::PANEL_RADIUS))
                    .border_1()
                    .border_color(colors.border_subtle)
                    .bg(colors.bg_elevated)
                    .shadow_lg()
                    .flex()
                    .flex_col()
                    .gap(px(Layout::CONTENT_PADDING))
                    .child(heading)
                    .child(body)
                    .child(footer),
            )
    }
}
