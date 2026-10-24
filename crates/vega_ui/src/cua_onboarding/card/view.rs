use super::{GuidanceCard, Stage, Tracking};
use crate::cua_onboarding::controller::{CardAction, CardView, CheckState};
use crate::cua_onboarding::model::PreparationSnapshot;
use crate::cua_onboarding::{CallbackIdentity, PermissionPane};
use crate::icons::{Icon, icon, sized_icon};
use crate::interaction::InteractionExt;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Context, ExternalDragPayload, FileDragPaths, Image, KeyDownEvent, Render,
    Window, div, img, px,
};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use vega_conversation::types::PermissionState;
use vega_theme::{Layout, Typography, theme};

#[derive(Clone)]
struct AppDrag {
    bundle: Rc<RefCell<Option<PathBuf>>>,
    stage_identity: CallbackIdentity,
}

struct DragPreview {
    _gesture: Rc<()>,
    icon: Arc<Image>,
}

impl Render for DragPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        div()
            .flex()
            .items_center()
            .gap(px(Layout::IMAGE_PREVIEW_GAP))
            .text_color(colors.text_primary)
            .text_size(px(Typography::BODY))
            .child(img(self.icon.clone()).size(px(Layout::PREPARATION_APP_ICON_SIZE)))
            .child("Vega")
    }
}

impl GuidanceCard {
    fn action_button(
        &self,
        id: &'static str,
        label: &'static str,
        action: CardAction,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let close = action == CardAction::Close;
        let primary = matches!(
            action,
            CardAction::AccessibilitySettings | CardAction::ScreenRecordingSettings
        );
        let enabled =
            action != CardAction::Recheck || self.preparation.read(cx).snapshot().view.can_recheck;
        let background = if primary {
            colors.brand_primary
        } else {
            colors.bg_subtle
        };
        div()
            .id(id)
            .role(gpui_kit::Role::Button)
            .aria_label(label)
            .tab_stop(enabled)
            .focusable()
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .when(close, |button| {
                button.size(px(Layout::PREPARATION_CLOSE_SIZE))
            })
            .when(!close, |button| {
                button
                    .h(px(Layout::PREPARATION_GUIDE_CONTROL))
                    .px(px(Layout::CONTENT_PADDING))
            })
            .rounded(px(Layout::PANEL_RADIUS))
            .border_1()
            .border_color(background)
            .bg(background)
            .text_color(if !enabled {
                colors.text_tertiary
            } else if primary {
                colors.brand_on_accent
            } else {
                colors.text_secondary
            })
            .interaction_surface(
                background,
                if primary {
                    colors.brand_primary_strong
                } else {
                    colors.bg_hover
                },
                if primary {
                    colors.brand_primary_strong
                } else {
                    colors.bg_active
                },
                enabled,
            )
            .focus_visible(move |style| style.border_color(colors.accent))
            .when(enabled, |button| {
                button
                    .cursor_pointer()
                    .on_click(cx.listener(move |card, _, window, cx| {
                        card.dispatch(action, window, cx);
                    }))
                    .on_key_down(cx.listener(move |card, event: &KeyDownEvent, window, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            card.dispatch(action, window, cx);
                            cx.stop_propagation();
                        }
                    }))
            })
            .child(if close {
                icon(Icon::Close, colors.text_secondary)
            } else {
                div().child(label).into_any_element()
            })
            .into_any_element()
    }

    fn diagnostics_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        div()
            .id("preparation-diagnostics")
            .role(gpui_kit::Role::Button)
            .aria_label("权限与检测")
            .focusable()
            .tab_stop(true)
            .flex_shrink_0()
            .px(px(Layout::IMAGE_PREVIEW_GAP))
            .py(px(Layout::TITLEBAR_CONTROL_GAP))
            .cursor_pointer()
            .rounded(px(Layout::PANEL_RADIUS))
            .text_color(colors.text_secondary)
            .interaction_surface(colors.bg_elevated, colors.bg_hover, colors.bg_active, true)
            .focus_visible(move |style| style.border_1().border_color(colors.brand_primary))
            .on_click(cx.listener(|_, _, _, cx| {
                cx.set_global(crate::settings::NativePreparationSettingsRequested(true));
                cx.set_global(crate::settings::SettingsOpen(true));
            }))
            .on_key_down(cx.listener(|_, event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    cx.set_global(crate::settings::NativePreparationSettingsRequested(true));
                    cx.set_global(crate::settings::SettingsOpen(true));
                    cx.stop_propagation();
                }
            }))
            .child("权限与检测")
            .into_any_element()
    }

    fn back_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        div()
            .id("preparation-back")
            .role(gpui_kit::Role::Button)
            .aria_label("返回权限准备")
            .focusable()
            .tab_stop(true)
            .size(px(Layout::PREPARATION_GUIDE_CONTROL))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .cursor_pointer()
            .bg(colors.bg_subtle)
            .interaction_surface(colors.bg_subtle, colors.bg_hover, colors.bg_active, true)
            .focus_visible(move |style| style.border_1().border_color(colors.brand_primary))
            .on_click(cx.listener(|card, _, window, cx| card.show_overview(window, cx)))
            .on_key_down(cx.listener(|card, event: &KeyDownEvent, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    card.show_overview(window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(icon(Icon::ArrowLeft, colors.text_secondary))
            .into_any_element()
    }

    fn permission_row(
        &self,
        pane: PermissionPane,
        preparation: &PreparationSnapshot,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let (id, title, detail, symbol, state, action) = match pane {
            PermissionPane::Accessibility => (
                "preparation-accessibility",
                "辅助功能",
                "允许 Vega 访问应用界面",
                Icon::Hand,
                preparation.view.accessibility,
                CardAction::AccessibilitySettings,
            ),
            PermissionPane::ScreenRecording => (
                "preparation-screen-recording",
                "屏幕录制",
                "使用截图识别界面和操作位置",
                Icon::Camera,
                preparation.view.screen_recording,
                CardAction::ScreenRecordingSettings,
            ),
        };
        let status = if preparation.view.check_state == CheckState::Checking {
            "检测中…"
        } else {
            match state {
                PermissionState::Granted => "已开启",
                PermissionState::Denied => "未授权",
                PermissionState::Unknown
                    if preparation.view.check_state == CheckState::Idle
                        && !preparation.retained =>
                {
                    "未检测"
                }
                PermissionState::Unknown => "状态未知",
            }
        };
        div()
            .w_full()
            .min_h(px(Layout::PREPARATION_PERMISSION_HEIGHT))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(Layout::CONTENT_PADDING))
            .p(px(Layout::CONTENT_PADDING))
            .rounded(px(Layout::ENVIRONMENT_CARD_RADIUS))
            .bg(colors.bg_subtle)
            .border_1()
            .border_color(colors.border_subtle)
            .child(sized_icon(
                symbol,
                colors.brand_primary,
                Layout::PREPARATION_PERMISSION_ICON,
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(Layout::IMAGE_PREVIEW_GAP))
                            .child(
                                div()
                                    .text_size(px(Typography::HEADING_PAGE))
                                    .font_weight(Typography::HEADING_PAGE_WEIGHT)
                                    .child(title),
                            )
                            .child(
                                div()
                                    .text_size(px(Typography::METADATA))
                                    .text_color(colors.text_secondary)
                                    .child(status),
                            ),
                    )
                    .child(div().text_color(colors.text_secondary).child(detail)),
            )
            .child(self.action_button(
                id,
                if state == PermissionState::Granted {
                    "打开设置"
                } else {
                    "允许"
                },
                action,
                cx,
            ))
            .into_any_element()
    }

    fn overview(&self, preparation: &PreparationSnapshot, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        div()
            .id("preparation-overview")
            .size_full()
            .relative()
            .child(
                div()
                    .id("preparation-overview-content")
                    .size_full()
                    .overflow_y_scroll()
                    .p(px(Layout::PREPARATION_OVERVIEW_PADDING))
                    .when(self.panel_fault.is_some(), |content| {
                        content.child(
                            div()
                                .mb(px(Layout::IMAGE_PREVIEW_GAP))
                                .text_color(colors.warning)
                                .child("无法定位准备窗口。请关闭后重新打开引导。"),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .child(
                                img(self.app_icon.clone())
                                    .size(px(Layout::PREPARATION_OVERVIEW_ICON)),
                            )
                            .child(
                                div()
                                    .mt(px(Layout::CONTENT_PADDING))
                                    .line_height(px(Layout::PREPARATION_OVERVIEW_TITLE_LINE))
                                    .text_size(px(Typography::EMPTY_STATE_TITLE))
                                    .font_weight(Typography::EMPTY_STATE_TITLE_WEIGHT)
                                    .child("启用 Vega 电脑操控"),
                            )
                            .child(
                                div()
                                    .mt(px(Layout::IMAGE_PREVIEW_GAP))
                                    .max_w(px(Layout::PREPARATION_OVERVIEW_DESCRIPTION_WIDTH))
                                    .line_height(px(Layout::PREPARATION_OVERVIEW_DESCRIPTION_LINE))
                                    .text_center()
                                    .text_color(colors.text_secondary)
                                    .child(div().child("Vega 需要以下权限来操作 Mac 上的应用。"))
                                    .child(div().child("仅在你请求电脑操控时使用。")),
                            ),
                    )
                    .child(
                        div()
                            .mt(px(Layout::PREPARATION_CARD_SHADOW_PADDING))
                            .flex()
                            .flex_col()
                            .gap(px(Layout::CONTENT_PADDING))
                            .child(self.permission_row(
                                PermissionPane::Accessibility,
                                preparation,
                                cx,
                            ))
                            .child(self.permission_row(
                                PermissionPane::ScreenRecording,
                                preparation,
                                cx,
                            )),
                    )
                    .child(
                        div()
                            .mt(px(Layout::CONTENT_PADDING))
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(Layout::IMAGE_PREVIEW_GAP))
                            .child(self.action_button(
                                "preparation-recheck",
                                if preparation.view.can_recheck {
                                    "重新检测"
                                } else {
                                    "检测中…"
                                },
                                CardAction::Recheck,
                                cx,
                            ))
                            .when(preparation.guide_warning.is_some(), |row| {
                                row.child(self.diagnostics_button(cx))
                            })
                            .when(self.has_task, |row| {
                                row.child(self.action_button(
                                    "preparation-return-task",
                                    "返回任务",
                                    CardAction::ReturnToTask,
                                    cx,
                                ))
                            }),
                    )
                    .children(preparation.feedback.iter().map(|feedback| {
                        div()
                            .mt(px(Layout::IMAGE_PREVIEW_GAP))
                            .text_color(if feedback.warning {
                                colors.warning
                            } else {
                                colors.text_secondary
                            })
                            .child(feedback.message.clone())
                    }))
                    .child(
                        div()
                            .mt(px(Layout::IMAGE_PREVIEW_GAP))
                            .text_size(px(Typography::METADATA))
                            .text_color(colors.text_secondary)
                            .child("修改屏幕录制权限后，如系统提示重启，请退出并重新打开 Vega。"),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .top(px(Layout::CONTENT_PADDING))
                    .right(px(Layout::CONTENT_PADDING))
                    .child(self.action_button(
                        "preparation-close",
                        "关闭引导",
                        CardAction::Close,
                        cx,
                    )),
            )
            .into_any_element()
    }

    fn app_region(&self, view: &CardView, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        if !view.can_drag {
            return div()
                .flex_shrink_0()
                .flex()
                .flex_col()
                .gap(px(Layout::IMAGE_PREVIEW_GAP))
                .text_color(colors.text_secondary)
                .child("当前是开发可执行文件，不能拖入权限列表。")
                .child(self.action_button(
                    "preparation-finder",
                    "在 Finder 中显示",
                    CardAction::RevealInFinder,
                    cx,
                ))
                .into_any_element();
        }
        let drag = AppDrag {
            bundle: Rc::new(RefCell::new(None)),
            stage_identity: self.stage_identity.clone(),
        };
        let weak = cx.weak_entity();
        let payload_owner = weak.clone();
        let app_icon = self.app_icon.clone();
        div()
            .id("preparation-app")
            .role(gpui_kit::Role::Button)
            .aria_label("Vega，可拖入权限列表；点击或按回车在 Finder 中显示")
            .focusable()
            .tab_stop(true)
            .w_full()
            .h(px(Layout::PREPARATION_APP_TARGET_HEIGHT))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(Layout::IMAGE_PREVIEW_GAP))
            .px(px(Layout::IMAGE_PREVIEW_GAP))
            .rounded(px(Layout::PANEL_RADIUS))
            .border_1()
            .border_color(colors.border_subtle)
            .bg(colors.bg_base)
            .cursor_pointer()
            .interaction_surface(colors.bg_base, colors.bg_hover, colors.bg_active, true)
            .focus_visible(move |style| style.border_color(colors.brand_primary))
            .on_click(cx.listener(|card, _, window, cx| {
                card.dispatch(CardAction::RevealInFinder, window, cx);
            }))
            .on_key_down(cx.listener(|card, event: &KeyDownEvent, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    card.dispatch(CardAction::RevealInFinder, window, cx);
                    cx.stop_propagation();
                }
            }))
            .on_drag(drag, move |drag, _, _, cx| {
                let gesture = Rc::new(());
                let bundle = weak
                    .update(cx, |card, cx| {
                        if card.stage != Stage::Guide
                            || card.placement_pending
                            || card.stage_identity != drag.stage_identity
                        {
                            return None;
                        }
                        let bundle = card.controller.begin_drag();
                        if bundle.is_some() {
                            card.drag_gesture = Some(Rc::downgrade(&gesture));
                        }
                        let bundle = bundle.filter(|path| {
                            if path.exists() {
                                true
                            } else {
                                card.path_error =
                                    Some("当前 Vega 的位置已不存在。请重新打开应用。".into());
                                false
                            }
                        });
                        cx.notify();
                        bundle
                    })
                    .ok()
                    .flatten();
                *drag.bundle.borrow_mut() = bundle;
                cx.new(|_| DragPreview {
                    _gesture: gesture,
                    icon: app_icon.clone(),
                })
            })
            .external_drag_payload(move |drag: &AppDrag, _, cx: &mut App| {
                if !payload_owner
                    .read_with(cx, |card, _| {
                        card.controller.view().open
                            && !card.placement_pending
                            && card.stage == Stage::Guide
                            && card.stage_identity == drag.stage_identity
                    })
                    .unwrap_or(false)
                {
                    return None;
                }
                let bundle = drag.bundle.borrow().clone()?;
                if !bundle.exists() {
                    return None;
                }
                Some(ExternalDragPayload::Files(FileDragPaths::new([(
                    bundle, true,
                )])))
            })
            .child(
                img(self.app_icon.clone())
                    .size(px(Layout::PREPARATION_APP_ICON_SIZE))
                    .flex_shrink_0(),
            )
            .child("Vega")
            .into_any_element()
    }

    fn guide(&self, preparation: &PreparationSnapshot, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let view = self.controller.view();
        let waiting = matches!(self.tracking, Tracking::Waiting);
        let local_fault = self
            .path_error
            .as_deref()
            .or_else(|| self.panel_fault.map(|fault| fault.message()))
            .or(waiting.then_some("未找到系统设置窗口。请打开对应权限设置。"));
        let warning = local_fault.or(preparation.guide_warning);
        let action = match self.permission_pane {
            PermissionPane::Accessibility => CardAction::AccessibilitySettings,
            PermissionPane::ScreenRecording => CardAction::ScreenRecordingSettings,
        };
        let instruction = match self.permission_pane {
            PermissionPane::Accessibility => "将 Vega 拖入上方列表，以允许辅助功能",
            PermissionPane::ScreenRecording => "将 Vega 拖入上方列表，以允许屏幕录制",
        };
        div()
            .size_full()
            .p(px(Layout::CONTENT_PADDING))
            .flex()
            .items_center()
            .gap(px(Layout::PREPARATION_GUIDE_GAP))
            .child(self.back_button(cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .gap(px(Layout::PREPARATION_GUIDE_GAP))
                    .child(
                        div()
                            .min_h(px(Layout::PREPARATION_GUIDE_CONTROL))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .gap(px(Layout::IMAGE_PREVIEW_GAP))
                            .child(sized_icon(
                                Icon::ArrowUp,
                                colors.brand_primary,
                                Layout::PREPARATION_GUIDE_CONTROL,
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_color(if warning.is_some() {
                                        colors.warning
                                    } else {
                                        colors.text_secondary
                                    })
                                    .child(warning.unwrap_or(instruction).to_owned()),
                            )
                            .child(self.action_button(
                                "preparation-close",
                                "关闭引导",
                                CardAction::Close,
                                cx,
                            )),
                    )
                    .child(
                        div()
                            .id("preparation-guide-content")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .gap(px(Layout::PREPARATION_GUIDE_GAP))
                            .when(local_fault.is_none(), |column| {
                                column.child(self.app_region(&view, cx))
                            })
                            .when(warning.is_some(), |column| {
                                column.child(
                                    div()
                                        .flex_shrink_0()
                                        .flex()
                                        .flex_wrap()
                                        .gap(px(Layout::IMAGE_PREVIEW_GAP))
                                        .when(local_fault.is_some(), |row| {
                                            row.child(self.action_button(
                                                "preparation-open-settings",
                                                "打开权限设置",
                                                action,
                                                cx,
                                            ))
                                        })
                                        .when(
                                            local_fault.is_some()
                                                && (self.path_error.is_some() || !view.can_drag),
                                            |row| {
                                                row.child(self.action_button(
                                                    "preparation-finder",
                                                    "在 Finder 中显示",
                                                    CardAction::RevealInFinder,
                                                    cx,
                                                ))
                                            },
                                        )
                                        .child(self.diagnostics_button(cx)),
                                )
                            }),
                    ),
            )
            .into_any_element()
    }
}

impl Render for GuidanceCard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        let preparation = self.preparation.read(cx).snapshot();
        let (width, height) = self.stage.surface_size();
        let body = match self.stage {
            Stage::Overview => self.overview(&preparation, cx),
            Stage::Guide => self.guide(&preparation, cx),
        };
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
                    .w(px(width))
                    .h(px(height))
                    .rounded(px(Layout::PANEL_RADIUS))
                    .border_1()
                    .border_color(colors.border_subtle)
                    .bg(colors.bg_elevated)
                    .shadow_lg()
                    .child(body),
            )
    }
}
