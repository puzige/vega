use super::*;
use crate::interaction::InteractionExt;
use vega_conversation::types::{UpdatePhase, UpdateProjection, UpdateRequest};

impl EventEmitter<UpdateRequest> for SettingsView {}

impl SettingsView {
    pub fn apply_update_projection(
        &mut self,
        projection: UpdateProjection,
        cx: &mut Context<Self>,
    ) {
        if self.updater.notes != projection.notes {
            self.updater_notes_scroll
                .set_offset(gpui_kit::point(px(0.), px(0.)));
            self.updater_notes = if projection.notes.is_empty() {
                None
            } else {
                let mut stream = vega_markdown::MarkdownStream::new();
                stream.append(&projection.notes);
                stream.finish();
                let mut model = crate::conversation_stream::StreamModel::default();
                model.sync(
                    &stream.snapshot(),
                    &crate::conversation_stream::StreamCounters::default(),
                );
                Some(model)
            };
        }
        self.updater = projection;
        cx.notify();
    }

    pub fn show_general(&mut self, cx: &mut Context<Self>) {
        self.request_settings_section(1, cx);
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
        let automatic = self.updater.automatic;
        let automatic_switch = settings_switch(
            "updater-automatic",
            "自动检查并下载",
            automatic,
            !busy,
            colors,
        )
        .when(!busy, |switch| {
            switch
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(move |_, _, _, cx| cx.emit(UpdateRequest::Automatic(!automatic))),
                )
                .on_key_down(
                    cx.listener(move |_, event: &gpui_kit::KeyDownEvent, _, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            cx.emit(UpdateRequest::Automatic(!automatic));
                            cx.stop_propagation();
                        }
                    }),
                )
        });
        settings_group(colors)
            .debug_selector(|| "settings-general-updater".into())
            .child(
                settings_actions()
                    .justify_between()
                    .child(section_title("Vega 更新", colors.text_primary))
                    .child(self.settings_help(
                        "updater-help",
                        "更新帮助",
                        "每天自动检查一次更新。下载后需确认重启才能安装；更新包会校验签名。",
                        cx,
                    )),
            )
            .child(settings_field(
                "updater-version",
                "版本",
                Some(checked),
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(format!("当前 {}", self.updater.current_version))
                    .when_some(self.updater.latest_version.clone(), |view, version| {
                        view.child(
                            div()
                                .text_color(colors.text_secondary)
                                .child(format!("最新 {version}")),
                        )
                    }),
                colors,
            ))
            .child(settings_field(
                "updater-auto",
                "自动检查并下载",
                None,
                automatic_switch,
                colors,
            ))
            .when(
                !self.updater.message.is_empty()
                    && self.updater.message != "更新包将通过内置公钥验证；安装前需要确认重启",
                |view| {
                    view.child(
                        settings_status(
                            self.updater.message.clone(),
                            if self.updater.phase == UpdatePhase::Error {
                                colors.danger
                            } else if ready {
                                colors.success
                            } else {
                                colors.text_secondary
                            },
                            colors,
                        )
                        .mt_3(),
                    )
                },
            )
            .when_some(progress, |view, progress| {
                view.child(
                    div()
                        .mt_2()
                        .text_color(colors.text_secondary)
                        .child(progress),
                )
            })
            .child(
                settings_actions()
                    .mt_3()
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
            .when_some(self.updater_notes.as_ref(), |view, notes| {
                view.child(
                    div()
                        .id("updater-notes-toggle")
                        .interactive(colors, true, false)
                        .debug_selector(|| "updater-notes-toggle".into())
                        .mt_3()
                        .min_h(px(Layout::SETTINGS_CONTROL_MIN_HEIGHT))
                        .px_2()
                        .self_start()
                        .flex()
                        .items_center()
                        .gap_2()
                        .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
                        .border_1()
                        .border_color(gpui_kit::transparent_black())
                        .focusable()
                        .tab_stop(true)
                        .cursor_pointer()
                        .focus_visible(move |style| style.border_color(colors.accent))
                        .aria_label(if self.updater_notes_open {
                            "收起发行说明"
                        } else {
                            "展开发行说明"
                        })
                        .child(crate::icons::icon(
                            if self.updater_notes_open {
                                crate::icons::Icon::ChevronDown
                            } else {
                                crate::icons::Icon::ChevronRight
                            },
                            colors.text_secondary,
                        ))
                        .child("发行说明")
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.updater_notes_open = !this.updater_notes_open;
                                cx.notify();
                            }),
                        )
                        .on_key_down(cx.listener(|this, event: &gpui_kit::KeyDownEvent, _, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                this.updater_notes_open = !this.updater_notes_open;
                                cx.stop_propagation();
                                cx.notify();
                            }
                        })),
                )
                .when(self.updater_notes_open, |view| {
                    view.child(
                        div()
                            .id("updater-notes-body")
                            .debug_selector(|| "updater-notes-body".into())
                            .min_w_0()
                            .max_h(px(Layout::SETTINGS_NOTES_MAX_HEIGHT))
                            .overflow_y_scroll()
                            .track_scroll(&self.updater_notes_scroll)
                            .on_scroll_wheel(crate::tool_card::contain_disclosure_scroll(
                                &self.updater_notes_scroll,
                            ))
                            .mt_2()
                            .child(crate::conversation_stream::markdown_item(
                                notes, None, &colors,
                            )),
                    )
                })
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
        settings_action_chrome(colors)
            .id(id)
            .interaction_surface(
                if id == "updater-install" {
                    colors.brand_primary
                } else {
                    colors.bg_elevated
                },
                if id == "updater-install" {
                    colors.brand_primary_strong
                } else {
                    colors.bg_hover
                },
                if id == "updater-install" {
                    colors.brand_primary_strong
                } else {
                    colors.bg_active
                },
                enabled,
            )
            .debug_selector(move || id.into())
            .text_color(if enabled && id == "updater-install" {
                colors.brand_on_accent
            } else if enabled {
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
