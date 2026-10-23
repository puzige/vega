use super::{GuidanceCard, Tracking};
use crate::cua_onboarding::controller::{CardAction, CardView};
use crate::icons::{Icon, icon};
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
use vega_theme::{Layout, Typography, theme};

#[derive(Clone)]
struct AppDrag {
    bundle: Rc<RefCell<Option<PathBuf>>>,
}

struct DragPreview {
    icon: Arc<Image>,
}

impl Render for DragPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        div()
            .flex()
            .flex_col()
            .items_center()
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
        div()
            .id(id)
            .role(gpui_kit::Role::Button)
            .aria_label(label)
            .tab_stop(true)
            .focusable()
            .flex_shrink_0()
            .when(close, |button| {
                button
                    .size(px(Layout::PREPARATION_CLOSE_SIZE))
                    .flex()
                    .items_center()
                    .justify_center()
            })
            .when(!close, |button| {
                button
                    .px(px(Layout::IMAGE_PREVIEW_GAP))
                    .py(px(Layout::TITLEBAR_CONTROL_GAP))
            })
            .rounded(px(Layout::PANEL_RADIUS))
            .text_color(colors.text_secondary)
            .cursor_pointer()
            .interaction_surface(colors.bg_elevated, colors.bg_hover, colors.bg_active, true)
            .focus_visible(move |style| style.border_1().border_color(colors.brand_primary))
            .on_click(cx.listener(move |card, _, window, cx| {
                card.dispatch(action, window, cx);
            }))
            .child(if close {
                icon(Icon::Close, colors.text_secondary).into_any_element()
            } else {
                div().child(label).into_any_element()
            })
            .into_any_element()
    }

    fn app_region(&self, view: &CardView, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        if !view.can_drag {
            return div()
                .flex()
                .flex_col()
                .items_start()
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
        };
        let weak = cx.weak_entity();
        let payload_owner = weak.clone();
        let app_icon = self.app_icon.clone();
        let app = div()
            .id("preparation-app")
            .role(gpui_kit::Role::Button)
            .aria_label("Vega，可拖入权限列表；点击或按回车在 Finder 中显示")
            .focusable()
            .tab_stop(true)
            .w(px(Layout::PREPARATION_APP_TARGET_WIDTH))
            .h(px(Layout::PREPARATION_APP_TARGET_HEIGHT))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .items_center()
            .rounded(px(Layout::PANEL_RADIUS))
            .cursor_pointer()
            .interaction_surface(colors.bg_elevated, colors.bg_hover, colors.bg_active, true)
            .focus_visible(move |style| style.underline().text_color(colors.brand_primary))
            .on_click(cx.listener(|card, _, window, cx| {
                card.dispatch(CardAction::RevealInFinder, window, cx);
            }))
            .on_drag(drag, move |drag, _, _, cx| {
                let bundle = weak
                    .update(cx, |card, cx| {
                        let bundle = card.controller.begin_drag();
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
                    icon: app_icon.clone(),
                })
            })
            .external_drag_payload(move |drag: &AppDrag, _, cx: &mut App| {
                if !payload_owner
                    .read_with(cx, |card, _| card.controller.view().open)
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
            .child(
                div()
                    .h(px(Layout::PREPARATION_APP_LABEL_HEIGHT))
                    .flex_shrink_0()
                    .line_height(px(Layout::PREPARATION_APP_LABEL_HEIGHT))
                    .child("Vega"),
            );
        div()
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .gap(px(Layout::IMAGE_PREVIEW_GAP))
            .child(app)
            .when(self.has_task, |row| {
                row.child(self.action_button(
                    "preparation-return",
                    "返回任务",
                    CardAction::ReturnToTask,
                    cx,
                ))
            })
            .into_any_element()
    }
}

impl Render for GuidanceCard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        let view = self.controller.view();
        let waiting = matches!(self.tracking, Tracking::Waiting);
        let preparation = self.preparation.read(cx).snapshot();
        let fault = self
            .path_error
            .as_deref()
            .or_else(|| self.panel_fault.map(|fault| fault.message()))
            .or(waiting.then_some("未找到系统设置窗口。请打开对应权限设置。"))
            .or(preparation.guide_fault);
        let heading = div()
            .flex()
            .flex_shrink_0()
            .items_start()
            .gap(px(Layout::IMAGE_PREVIEW_GAP))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_color(if fault.is_some() {
                        colors.warning
                    } else {
                        colors.text_primary
                    })
                    .child(
                        fault
                            .unwrap_or(if view.can_drag {
                                "将 Vega 拖入上方列表，然后打开开关"
                            } else {
                                "在 Finder 中定位当前 Vega"
                            })
                            .to_owned(),
                    ),
            )
            .child(self.action_button("preparation-close", "关闭引导", CardAction::Close, cx));
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
                    .gap(px(Layout::IMAGE_PREVIEW_GAP))
                    .child(heading)
                    .when(fault.is_none(), |card| card.child(self.app_region(&view, cx)))
                    .when(fault.is_some(), |card| {
                        card.child(
                            div().flex().flex_shrink_0().items_center().gap(px(Layout::IMAGE_PREVIEW_GAP))
                            .when(waiting, |row| {
                                row.child(self.action_button(
                                    "preparation-open-settings",
                                    "打开权限设置",
                                    match self.permission_pane {
                                        crate::cua_onboarding::PermissionPane::Accessibility => CardAction::AccessibilitySettings,
                                        crate::cua_onboarding::PermissionPane::ScreenRecording => CardAction::ScreenRecordingSettings,
                                    },
                                    cx,
                                ))
                            })
                            .when(!view.can_drag, |row| {
                                row.child(self.action_button("preparation-finder", "在 Finder 中显示", CardAction::RevealInFinder, cx))
                            })
                            .child(div()
                                .id("preparation-diagnostics")
                                .role(gpui_kit::Role::Button)
                                .aria_label("权限与检测")
                                .focusable()
                                .tab_stop(true)
                                .cursor_pointer()
                                .rounded(px(Layout::PANEL_RADIUS))
                                .focus_visible(move |style| style.border_1().border_color(colors.brand_primary))
                                .on_click(cx.listener(|_, _, _, cx| {
                                    cx.set_global(crate::settings::NativePreparationSettingsRequested(true));
                                    cx.set_global(crate::settings::SettingsOpen(true));
                                }))
                                .child("权限与检测")),
                        )
                    }),
            )
    }
}
