use super::*;
use crate::interaction::InteractionExt;
use crate::scroll_edge_fade::{ScrollEdgeFadeExt, ScrollFade};
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
        self.updater_message = (!projection.message.is_empty()).then(|| {
            SettingsText::from_known(
                projection.message.clone(),
                &[
                    "app.update.app_file",
                    "app.update.architecture",
                    "app.update.archive_app_missing",
                    "app.update.archive_entries",
                    "app.update.archive_escape",
                    "app.update.archive_expanded",
                    "app.update.archive_invalid",
                    "app.update.archive_length",
                    "app.update.archive_paths",
                    "app.update.archive_read",
                    "app.update.archive_signature",
                    "app.update.archive_special",
                    "app.update.archive_unauthorized",
                    "app.update.asset_invalid",
                    "app.update.asset_missing",
                    "app.update.asset_origin",
                    "app.update.asset_signature",
                    "app.update.asset_url",
                    "app.update.automatic_retry",
                    "app.update.available_verifying",
                    "app.update.busy",
                    "app.update.cannot_install",
                    "app.update.canonical_path",
                    "app.update.checking",
                    "app.update.config_unavailable",
                    "app.update.current",
                    "app.update.developer_id",
                    "app.update.development",
                    "app.update.development_check_only",
                    "app.update.development_install",
                    "app.update.development_unsupported",
                    "app.update.download_integrity",
                    "app.update.download_interrupted",
                    "app.update.download_limit",
                    "app.update.downloading",
                    "app.update.executable_link",
                    "app.update.file_changed",
                    "app.update.file_invalid",
                    "app.update.identity_changed",
                    "app.update.identity_failed",
                    "app.update.identity_version",
                    "app.update.install_directory",
                    "app.update.install_location",
                    "app.update.install_read_only",
                    "app.update.installer_not_ready",
                    "app.update.later",
                    "app.update.manifest_format",
                    "app.update.manifest_identity",
                    "app.update.memory",
                    "app.update.network_init",
                    "app.update.not_ready",
                    "app.update.notarization",
                    "app.update.permissions",
                    "app.update.platform",
                    "app.update.preferences_invalid",
                    "app.update.preferences_read",
                    "app.update.preferences_retry",
                    "app.update.preferences_save",
                    "app.update.prepare_record",
                    "app.update.public_key",
                    "app.update.ready",
                    "app.update.release_metadata",
                    "app.update.release_unavailable",
                    "app.update.release_unstable",
                    "app.update.request_failed",
                    "app.update.response_limit",
                    "app.update.response_read",
                    "app.update.restart_unconfirmed",
                    "app.update.restarting",
                    "app.update.rolled_back",
                    "app.update.service_unavailable",
                    "app.update.signature_format",
                    "app.update.signature_incomplete",
                    "app.update.signature_length",
                    "app.update.signature_limit",
                    "app.update.signature_missing",
                    "app.update.signature_rejected",
                    "app.update.staging_permissions",
                    "app.update.start_failed",
                    "app.update.symlink_path",
                    "app.update.tasks_running",
                    "app.update.team_mismatch",
                    "app.update.verification_notice",
                    "app.update.verification_output",
                    "app.update.verification_timeout",
                    "app.update.verifying",
                    "app.update.version_format",
                    "app.update.version_range",
                    "app.update.version_unavailable",
                ],
            )
        });
        self.updater_version = Some(if projection.current_version.is_empty() {
            SettingsText::key("domain.updater.loading_version")
        } else {
            SettingsText::from_known(
                projection.current_version.clone(),
                &["app.update.development"],
            )
        });
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
                    t("settings.updater.checked_now").into()
                } else if minutes < 60 {
                    t_args(
                        "settings.updater.checked_minutes",
                        &[("minutes", &minutes.to_string())],
                    )
                } else {
                    t_args(
                        "settings.updater.checked_hours",
                        &[("hours", &(minutes / 60).to_string())],
                    )
                }
            })
            .unwrap_or_else(|| t("settings.updater.never_checked").into());
        let progress = match self.updater.phase {
            UpdatePhase::Downloading { received, total } if total > 0 => Some(t_args(
                "settings.updater.downloaded",
                &[(
                    "percent",
                    &(received.saturating_mul(100) / total).to_string(),
                )],
            )),
            _ => None,
        };
        let message = self.updater_message.as_ref().map(SettingsText::render);
        let current_version = self.updater_version.as_ref().map_or_else(
            || self.updater.current_version_label().to_string(),
            SettingsText::render,
        );
        let automatic = self.updater.automatic;
        let automatic_switch = settings_switch(
            "updater-automatic",
            t("settings.updater.automatic"),
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
            .child(
                settings_actions()
                    .justify_between()
                    .child(section_title(
                        t("settings.updater.title"),
                        colors.text_primary,
                    ))
                    .child(self.settings_help(
                        "updater-help",
                        t("settings.updater.help_label"),
                        t("settings.updater.help"),
                        cx,
                    )),
            )
            .child(settings_field(
                t("settings.updater.version"),
                Some(checked),
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(t_args(
                        "settings.updater.current_version",
                        &[("version", &current_version)],
                    ))
                    .when_some(self.updater.latest_version.clone(), |view, version| {
                        view.child(div().text_color(colors.text_secondary).child(t_args(
                            "settings.updater.latest_version",
                            &[("version", &version)],
                        )))
                    }),
                colors,
            ))
            .child(settings_field(
                t("settings.updater.automatic"),
                None,
                automatic_switch,
                colors,
            ))
            .when(
                message
                    .as_ref()
                    .is_some_and(|message| message != t("settings.updater.verification_notice")),
                |view| {
                    view.child(
                        settings_status(
                            message.clone().unwrap_or_default(),
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
                        t("settings.updater.check"),
                        UpdateRequest::Check,
                        !busy && !ready,
                        cx,
                    ))
                    .child(self.update_button(
                        "updater-release",
                        t("settings.updater.release_page"),
                        UpdateRequest::ReleasePage,
                        true,
                        cx,
                    ))
                    .when(ready, |view| {
                        view.child(self.update_button(
                            "updater-later",
                            t("settings.updater.later"),
                            UpdateRequest::Later,
                            true,
                            cx,
                        ))
                        .child(self.update_button(
                            "updater-install",
                            t("settings.updater.install"),
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
                            t("settings.updater.hide_notes")
                        } else {
                            t("settings.updater.show_notes")
                        })
                        .child(crate::icons::icon(
                            if self.updater_notes_open {
                                crate::icons::Icon::ChevronDown
                            } else {
                                crate::icons::Icon::ChevronRight
                            },
                            colors.text_secondary,
                        ))
                        .child(t("settings.updater.release_notes"))
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
                            .min_w_0()
                            .max_h(px(Layout::SETTINGS_NOTES_MAX_HEIGHT))
                            .overflow_y_scroll()
                            .track_scroll(&self.updater_notes_scroll)
                            .on_scroll_wheel(crate::tool_card::contain_disclosure_scroll(
                                &self.updater_notes_scroll,
                            ))
                            .mt_2()
                            .child(
                                crate::link_preview::MarkdownReadingScope::new(
                                    format!("updater-notes-{}", cx.entity_id()),
                                    self.section_focuses[1].clone(),
                                )
                                .track_scroll(self.updater_notes_scroll.clone())
                                .child(
                                    crate::conversation_stream::markdown_item(
                                        notes,
                                        None,
                                        &colors,
                                        &format!("updater-notes-{}", cx.entity_id()),
                                    ),
                                ),
                            )
                            .tracked_scroll_edge_fade(
                                self.updater_notes_scroll.clone(),
                                colors.bg_base,
                                ScrollFade::reading(),
                            ),
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
