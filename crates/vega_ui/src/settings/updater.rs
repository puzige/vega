use super::*;
use vega_conversation::types::{UpdatePhase, UpdateProjection, UpdateRequest};

impl EventEmitter<UpdateRequest> for SettingsView {}

impl SettingsView {
    pub fn apply_update_projection(
        &mut self,
        projection: UpdateProjection,
        cx: &mut Context<Self>,
    ) {
        self.updater = projection;
        cx.notify();
    }

    pub fn show_general(&mut self, cx: &mut Context<Self>) {
        self.section = 1;
        cx.notify();
    }

    pub(crate) fn render_updater(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let busy = matches!(
            self.updater.phase,
            UpdatePhase::Loading
                | UpdatePhase::Checking
                | UpdatePhase::Downloading { .. }
                | UpdatePhase::Installing
        );
        let ready = self.updater.phase == UpdatePhase::Ready;
        let checked = self
            .updater
            .last_checked
            .map(|last| {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|time| time.as_secs())
                    .unwrap_or_default();
                let minutes = now.saturating_sub(last) / 60;
                if minutes == 0 {
                    "最后检查：刚刚".into()
                } else if minutes < 60 {
                    format!("最后检查：{minutes} 分钟前")
                } else {
                    format!("最后检查：{} 小时前", minutes / 60)
                }
            })
            .unwrap_or_else(|| "尚未检查更新".into());
        let progress = match self.updater.phase {
            UpdatePhase::Downloading { received, total } if total > 0 => {
                Some(format!("已下载 {}%", received.saturating_mul(100) / total))
            }
            _ => None,
        };
        div()
            .flex()
            .flex_col()
            .gap_2()
            .mt_4()
            .child(section_title("Vega 更新", colors.text_primary))
            .child(format!("当前版本：{}", self.updater.current_version))
            .when_some(self.updater.latest_version.clone(), |view, version| {
                view.child(format!("最新版本：{version}"))
            })
            .child(
                div()
                    .text_color(colors.text_secondary)
                    .text_size(px(Typography::METADATA))
                    .child(checked),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child("自动检查并下载")
                            .child(
                                div()
                                    .text_color(colors.text_secondary)
                                    .text_size(px(Typography::METADATA))
                                    .child("每天检查一次；安装需要你确认重启"),
                            ),
                    )
                    .child(self.update_button(
                        "updater-automatic",
                        if self.updater.automatic {
                            "已开启"
                        } else {
                            "已关闭"
                        },
                        UpdateRequest::Automatic(!self.updater.automatic),
                        !busy,
                        cx,
                    )),
            )
            .child(
                div()
                    .text_color(if self.updater.phase == UpdatePhase::Error {
                        colors.danger
                    } else {
                        colors.text_secondary
                    })
                    .child(self.updater.message.clone()),
            )
            .when_some(progress, |view, progress| view.child(progress))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(self.update_button(
                        "updater-check",
                        "检查更新",
                        UpdateRequest::Check,
                        !busy && !ready,
                        cx,
                    ))
                    .child(self.update_button(
                        "updater-release",
                        "官方发布页",
                        UpdateRequest::ReleasePage,
                        true,
                        cx,
                    ))
                    .when(ready, |view| {
                        view.child(self.update_button(
                            "updater-later",
                            "稍后",
                            UpdateRequest::Later,
                            true,
                            cx,
                        ))
                        .child(self.update_button(
                            "updater-install",
                            "重启以安装",
                            UpdateRequest::Install,
                            true,
                            cx,
                        ))
                    }),
            )
            .when(!self.updater.notes.is_empty(), |view| {
                view.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .mt_2()
                        .child(section_title("发行说明", colors.text_primary))
                        .child(
                            div()
                                .whitespace_normal()
                                .text_color(colors.text_secondary)
                                .child(self.updater.notes.clone()),
                        ),
                )
            })
            .into_any_element()
    }

    fn update_button(
        &self,
        id: &'static str,
        label: &'static str,
        request: UpdateRequest,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        div()
            .id(id)
            .px_3()
            .py_2()
            .rounded_md()
            .border_1()
            .border_color(colors.border_subtle)
            .bg(colors.bg_elevated)
            .text_color(if enabled {
                colors.text_primary
            } else {
                colors.text_tertiary
            })
            .child(label)
            .when(enabled, |button| {
                button
                    .focusable()
                    .tab_stop(true)
                    .cursor_pointer()
                    .hover(move |style| style.bg(colors.bg_hover))
                    .focus_visible(move |style| style.border_color(colors.brand_primary))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |_, _, _, cx| cx.emit(request)),
                    )
                    .on_key_down(
                        cx.listener(move |_, event: &gpui_kit::KeyDownEvent, _, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                cx.emit(request);
                                cx.stop_propagation();
                            }
                        }),
                    )
            })
            .into_any_element()
    }
}
