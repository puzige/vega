use crate::interaction::InteractionExt;
use crate::scroll_edge_fade::{ScrollFade, ScrollableFadeExt};
use std::path::PathBuf;

use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Div, KeyDownEvent, MouseButton, MouseUpEvent, PathPromptOptions, div, px,
};
use vega_conversation::types::{
    SkillBodyPreview, SkillRootPreview, SkillSettingsMutation, SkillSettingsProjection,
    SkillSourceView, SkillUiScope,
};
use vega_conversation::{SkillSettingsError, SkillSettingsService};
use vega_i18n::{t, t_args};
use vega_theme::{Layout, Typography, theme};

use super::{
    SettingsText, SettingsView, section_title, settings_action_chrome, settings_actions,
    settings_field, settings_group, settings_section, settings_status, settings_switch,
};

#[derive(Default)]
pub(crate) struct SkillsSettingsState {
    service: Option<SkillSettingsService>,
    projection: Option<SkillSettingsProjection>,
    root_preview: Option<SkillRootPreview>,
    body_preview: Option<SkillBodyPreview>,
    busy: bool,
    message: Option<SettingsText>,
    request_generation: u64,
}

#[derive(Clone)]
enum SkillAction {
    Reload,
    PreviewProject,
    PreviewGlobal,
    ImportFolder,
    PreviewSkill { source_id: String, name: String },
    Mutate(SkillSettingsMutation),
    DismissPreview,
}

enum SkillOperation {
    Reload,
    PreviewProject,
    PreviewGlobal,
    PreviewImported(PathBuf),
    PreviewSkill {
        source_id: String,
        name: String,
    },
    Mutate {
        generation: u64,
        mutation: SkillSettingsMutation,
    },
}

enum SkillOperationResult {
    Projection(SkillSettingsProjection),
    Root(SkillRootPreview),
    Body(SkillBodyPreview),
}

impl SettingsView {
    /// Injected by the app using the current Store/config root and selected
    /// project. A new Settings session gets new ephemeral preview receipts.
    pub fn set_skills_service(
        &mut self,
        service: Option<SkillSettingsService>,
        cx: &mut gpui_kit::Context<Self>,
    ) {
        self.close_skills_session();
        let request_generation = self.skills.request_generation;
        self.skills = SkillsSettingsState {
            service,
            request_generation,
            ..SkillsSettingsState::default()
        };
        if self.skills.service.is_some() {
            self.skills_operation(SkillOperation::Reload, cx);
        } else {
            self.skills.message = Some(SettingsText::key("settings.skills.database_unavailable"));
            cx.notify();
        }
    }

    pub(crate) fn close_skills_session(&mut self) {
        self.skills.request_generation = self.skills.request_generation.wrapping_add(1);
        self.skills.busy = false;
        self.skills.root_preview = None;
        self.skills.body_preview = None;
        self.skills.projection = None;
        if let Some(service) = self.skills.service.take() {
            service.close_session();
        }
    }

    pub(crate) fn skills_section_changed(
        &mut self,
        next_section: usize,
        cx: &mut gpui_kit::Context<Self>,
    ) {
        if self.section == 6 && next_section != 6 {
            self.skills.request_generation = self.skills.request_generation.wrapping_add(1);
            self.skills.busy = false;
            self.skills.root_preview = None;
            self.skills.body_preview = None;
            if let Some(service) = &self.skills.service {
                service.clear_previews();
            }
        } else if self.section != 6 && next_section == 6 {
            self.skills_operation(SkillOperation::Reload, cx);
        }
    }

    fn skills_operation(&mut self, operation: SkillOperation, cx: &mut gpui_kit::Context<Self>) {
        if matches!(&operation, SkillOperation::Reload) {
            self.skills.projection = None;
        }
        let Some(service) = self.skills.service.clone() else {
            self.skills.message = Some(SettingsText::key("settings.skills.database_unavailable"));
            cx.notify();
            return;
        };
        self.skills.request_generation = self.skills.request_generation.wrapping_add(1);
        let request_generation = self.skills.request_generation;
        self.skills.busy = true;
        self.skills.message = None;
        cx.notify();
        let task = cx.background_executor().spawn(async move {
            match operation {
                SkillOperation::Reload => {
                    service.projection().map(SkillOperationResult::Projection)
                }
                SkillOperation::PreviewProject => service
                    .preview_project_root()
                    .map(SkillOperationResult::Root),
                SkillOperation::PreviewGlobal => service
                    .preview_vega_global_root()
                    .map(SkillOperationResult::Root),
                SkillOperation::PreviewImported(path) => service
                    .preview_imported_root(&path)
                    .map(SkillOperationResult::Root),
                SkillOperation::PreviewSkill { source_id, name } => service
                    .preview_skill(&source_id, &name)
                    .map(SkillOperationResult::Body),
                SkillOperation::Mutate {
                    generation,
                    mutation,
                } => {
                    service.apply(generation, mutation)?;
                    service.projection().map(SkillOperationResult::Projection)
                }
            }
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if this.skills.request_generation != request_generation {
                    return;
                }
                this.skills.busy = false;
                match result {
                    Ok(SkillOperationResult::Projection(projection)) => {
                        this.skills.projection = Some(projection);
                        this.skills.root_preview = None;
                        this.skills.body_preview = None;
                    }
                    Ok(SkillOperationResult::Root(preview)) => {
                        this.skills.root_preview = Some(preview);
                        this.skills.body_preview = None;
                    }
                    Ok(SkillOperationResult::Body(preview)) => {
                        this.skills.body_preview = Some(preview);
                    }
                    Err(error) => {
                        this.skills.message = Some(skill_error_label(error));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn pick_skills_root(&mut self, cx: &mut gpui_kit::Context<Self>) {
        let request_generation = self.skills.request_generation;
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(t("settings.skills.choose_directory").into()),
        });
        cx.spawn(async move |this, cx| {
            let picked = match receiver.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Ok(None)) | Err(_) => None,
                Ok(Err(_)) => {
                    let _ = this.update(cx, |this, cx| {
                        if this.section != 6 || this.skills.request_generation != request_generation
                        {
                            return;
                        }
                        this.skills.message =
                            Some(SettingsText::key("settings.skills.folder_failed"));
                        cx.notify();
                    });
                    return;
                }
            };
            if let Some(path) = picked {
                let _ = this.update(cx, |this, cx| {
                    if this.section != 6 || this.skills.request_generation != request_generation {
                        return;
                    }
                    this.skills_operation(SkillOperation::PreviewImported(path), cx)
                });
            }
        })
        .detach();
    }

    fn handle_skills_action(&mut self, action: SkillAction, cx: &mut gpui_kit::Context<Self>) {
        match action {
            SkillAction::Reload => self.skills_operation(SkillOperation::Reload, cx),
            SkillAction::PreviewProject => {
                self.skills_operation(SkillOperation::PreviewProject, cx)
            }
            SkillAction::PreviewGlobal => self.skills_operation(SkillOperation::PreviewGlobal, cx),
            SkillAction::ImportFolder => self.pick_skills_root(cx),
            SkillAction::PreviewSkill { source_id, name } => {
                self.skills_operation(SkillOperation::PreviewSkill { source_id, name }, cx)
            }
            SkillAction::Mutate(mutation) => {
                if let Some(projection) = &self.skills.projection {
                    self.skills_operation(
                        SkillOperation::Mutate {
                            generation: projection.consent_generation,
                            mutation,
                        },
                        cx,
                    );
                }
            }
            SkillAction::DismissPreview => {
                self.skills.root_preview = None;
                self.skills.body_preview = None;
                if let Some(service) = &self.skills.service {
                    service.clear_previews();
                }
                cx.notify();
            }
        }
    }

    pub(crate) fn render_skills(&mut self, cx: &mut gpui_kit::Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let mut column = settings_section()
            .id("settings-skills")
            .child(section_title(
                t("settings.skills.title"),
                colors.text_primary,
            ))
            .child(
                div()
                    .id("skills-intro")
                    .text_color(colors.text_secondary)
                    .child(t("settings.skills.intro")),
            )
            .child(skill_button(
                t("settings.action.refresh").into(),
                "skills-reload".into(),
                SkillAction::Reload,
                colors,
                cx,
            ));
        if self.skills.busy {
            column = column.child(settings_status(
                t("settings.skills.checking"),
                colors.text_secondary,
                colors,
            ));
        }
        if let Some(message) = &self.skills.message {
            column = column.child(
                settings_status(message.render(), colors.warning, colors).id("skills-message"),
            );
        }
        if let Some(projection) = self.skills.projection.clone() {
            let mut preferences = settings_group(colors)
                .child(section_title(
                    t("settings.skills.sources"),
                    colors.text_primary,
                ))
                .child(skill_toggle_row(
                    t("settings.skills.use_global"),
                    projection.global_enabled,
                    "skills-global-enabled",
                    SkillSettingsMutation::SetGlobal {
                        enabled: !projection.global_enabled,
                        automatic: projection.automatic_enabled,
                    },
                    !self.skills.busy,
                    colors,
                    cx,
                ))
                .child(skill_toggle_row(
                    t("settings.skills.auto_global"),
                    projection.automatic_enabled,
                    "skills-global-automatic",
                    SkillSettingsMutation::SetGlobal {
                        enabled: projection.global_enabled,
                        automatic: !projection.automatic_enabled,
                    },
                    !self.skills.busy,
                    colors,
                    cx,
                ));
            if let Some(project_id) = projection.selected_project_id.clone() {
                preferences = preferences
                    .child(skill_toggle_row(
                        t("settings.skills.use_project"),
                        projection.project_enabled,
                        "skills-project-enabled",
                        SkillSettingsMutation::SetProject {
                            project_id: project_id.clone(),
                            enabled: !projection.project_enabled,
                            automatic: projection.project_automatic,
                        },
                        !self.skills.busy,
                        colors,
                        cx,
                    ))
                    .child(skill_toggle_row(
                        t("settings.skills.auto_project"),
                        projection.project_automatic,
                        "skills-project-automatic",
                        SkillSettingsMutation::SetProject {
                            project_id,
                            enabled: projection.project_enabled,
                            automatic: !projection.project_automatic,
                        },
                        !self.skills.busy,
                        colors,
                        cx,
                    ));
            }
            preferences = preferences.child(
                settings_actions()
                    .mt_3()
                    .when(projection.selected_project_id.is_some(), |actions| {
                        actions.child(skill_button(
                            t("settings.skills.inspect_project").into(),
                            "skills-preview-project".into(),
                            SkillAction::PreviewProject,
                            colors,
                            cx,
                        ))
                    })
                    .child(skill_button(
                        t("settings.skills.inspect_global").into(),
                        "skills-preview-global".into(),
                        SkillAction::PreviewGlobal,
                        colors,
                        cx,
                    ))
                    .child(skill_button(
                        t("settings.skills.import").into(),
                        "skills-import-folder".into(),
                        SkillAction::ImportFolder,
                        colors,
                        cx,
                    )),
            );
            column = column.child(preferences);
            if projection.sources.is_empty() {
                column = column.child(settings_status(
                    t("settings.skills.no_sources"),
                    colors.text_secondary,
                    colors,
                ));
            }
            for source in &projection.sources {
                column = column.child(self.render_skill_source(source, &projection, cx));
            }
        }
        if let Some(preview) = self.skills.root_preview.clone() {
            column = column.child(self.render_root_preview(&preview, cx));
        }
        if let Some(preview) = self.skills.body_preview.clone() {
            column = column.child(self.render_body_preview(&preview, cx));
        }
        column.into_any_element()
    }

    fn render_skill_source(
        &self,
        source: &SkillSourceView,
        projection: &SkillSettingsProjection,
        cx: &mut gpui_kit::Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let scope = scope_label(source.scope);
        let mut card = settings_group(colors)
            .id(format!("skills-source-{}", source.id))
            .child(section_title(scope, colors.text_primary))
            .child(
                div()
                    .mt_2()
                    .font_weight(Typography::HEADING_CARD_WEIGHT)
                    .child(source.source_label.clone()),
            )
            .child(skill_toggle_row(
                t("settings.skills.enable_directory"),
                source.enabled,
                &format!("skills-source-enabled-{}", source.id),
                SkillSettingsMutation::SetSource {
                    source_id: source.id.clone(),
                    enabled: !source.enabled,
                    automatic: source.automatic,
                },
                !self.skills.busy,
                colors,
                cx,
            ))
            .child(skill_toggle_row(
                t("settings.skills.auto_directory"),
                source.automatic,
                &format!("skills-source-auto-{}", source.id),
                SkillSettingsMutation::SetSource {
                    source_id: source.id.clone(),
                    enabled: source.enabled,
                    automatic: !source.automatic,
                },
                !self.skills.busy,
                colors,
                cx,
            ))
            .child(settings_actions().mt_3().child(skill_button(
                t("settings.skills.unlink").into(),
                format!("skills-unlink-{}", source.id),
                SkillAction::Mutate(SkillSettingsMutation::UnlinkSource {
                    source_id: source.id.clone(),
                }),
                colors,
                cx,
            )));
        if let Some(diagnostic) = &source.diagnostic {
            card = card.child(div().mt_3().text_color(colors.warning).child(t_args(
                "settings.skills.source_unavailable",
                &[("diagnostic", diagnostic)],
            )));
        }
        card = card
            .child(
                div()
                    .mt_3()
                    .text_size(px(Typography::METADATA))
                    .text_color(colors.text_secondary)
                    .child(t_args(
                        "settings.skills.directory",
                        &[("path", &(source.configured_root.display()).to_string())],
                    )),
            )
            .child(
                div()
                    .text_size(px(Typography::METADATA))
                    .text_color(colors.text_secondary)
                    .child(t_args(
                        "settings.skills.actual_target",
                        &[("path", &(source.canonical_root.display()).to_string())],
                    )),
            );
        for candidate in &source.candidates {
            let status = match candidate.diagnostic.as_deref() {
                Some(diagnostic) => skill_diagnostic_label(diagnostic),
                None if candidate.model_winner => t("settings.skills.priority"),
                None => t("settings.skills.reviewed"),
            };
            let mut row = div()
                .id(format!("skills-candidate-{}-{}", source.id, candidate.name))
                .flex()
                .flex_col()
                .gap_2()
                .pt_2()
                .border_t_1()
                .border_color(colors.border_subtle)
                .child(div().child(format!("{} · {status}", candidate.name)));
            if let Some(label) = skill_shadow_label(projection, candidate) {
                let id = format!("skills-shadow-{}-{}", source.id, candidate.name);
                row = row.child(
                    div()
                        .id(id)
                        .text_size(px(Typography::METADATA))
                        .text_color(colors.text_secondary)
                        .child(label),
                );
            }
            if let Some(description) = &candidate.description {
                row = row.child(
                    div()
                        .text_size(px(Typography::METADATA))
                        .text_color(colors.text_secondary)
                        .child(description.clone()),
                );
            }
            if candidate.content_sha256.is_some() {
                row = row.child(skill_button(
                    t("settings.skills.review_sha").into(),
                    format!("skills-review-{}-{}", source.id, candidate.name),
                    SkillAction::PreviewSkill {
                        source_id: source.id.clone(),
                        name: candidate.name.clone(),
                    },
                    colors,
                    cx,
                ));
            }
            if candidate.approved_sha256.is_some() {
                row = row
                    .child(skill_toggle_row(
                        t("settings.skills.enable"),
                        candidate.enabled,
                        &format!("skills-enabled-{}-{}", source.id, candidate.name),
                        SkillSettingsMutation::SetSkill {
                            source_id: source.id.clone(),
                            name: candidate.name.clone(),
                            enabled: !candidate.enabled,
                            automatic: candidate.automatic,
                        },
                        !self.skills.busy,
                        colors,
                        cx,
                    ))
                    .child(skill_toggle_row(
                        t("settings.skills.auto"),
                        candidate.automatic,
                        &format!("skills-auto-{}-{}", source.id, candidate.name),
                        SkillSettingsMutation::SetSkill {
                            source_id: source.id.clone(),
                            name: candidate.name.clone(),
                            enabled: candidate.enabled,
                            automatic: !candidate.automatic,
                        },
                        !self.skills.busy,
                        colors,
                        cx,
                    ));
            }
            card = card.child(row);
        }
        card.into_any_element()
    }

    fn render_root_preview(
        &self,
        preview: &SkillRootPreview,
        cx: &mut gpui_kit::Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        div()
            .id("skills-root-preview")
            .flex()
            .flex_col()
            .min_w_0()
            .flex_shrink_0()
            .gap(px(Layout::SETTINGS_CONTENT_GAP))
            .p(px(Layout::SETTINGS_CONTENT_GAP))
            .rounded(px(Layout::PANEL_RADIUS))
            .border_1()
            .border_color(colors.warning)
            .child(section_title(
                t("settings.skills.confirm_directory"),
                colors.text_primary,
            ))
            .child(t_args(
                "settings.skills.selected_directory",
                &[("path", &(preview.configured_root.display()).to_string())],
            ))
            .child(t_args(
                "settings.skills.actual_target",
                &[("path", &(preview.canonical_root.display()).to_string())],
            ))
            .child(t_args(
                "settings.skills.source_label",
                &[("label", &preview.source_label)],
            ))
            .child(t_args(
                "settings.skills.candidate_count",
                &[("count", &(preview.candidates.len()).to_string())],
            ))
            .children(preview.candidates.iter().map(|candidate| {
                div().text_color(colors.text_secondary).child(format!(
                    "{} · {}",
                    candidate.name,
                    candidate
                        .diagnostic
                        .as_deref()
                        .unwrap_or(t("settings.skills.awaiting_review"))
                ))
            }))
            .child(t("settings.skills.link_warning"))
            .child(skill_button(
                t("settings.skills.link_exact").into(),
                "skills-link-root".into(),
                SkillAction::Mutate(SkillSettingsMutation::LinkRoot {
                    preview_token: preview.token.clone(),
                }),
                colors,
                cx,
            ))
            .child(skill_button(
                t("settings.action.cancel").into(),
                "skills-cancel-root-preview".into(),
                SkillAction::DismissPreview,
                colors,
                cx,
            ))
            .into_any_element()
    }

    fn render_body_preview(
        &self,
        preview: &SkillBodyPreview,
        cx: &mut gpui_kit::Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        div()
            .id("skills-body-preview")
            .flex()
            .flex_col()
            .min_w_0()
            .flex_shrink_0()
            .gap(px(Layout::SETTINGS_CONTENT_GAP))
            .p(px(Layout::SETTINGS_CONTENT_GAP))
            .rounded(px(Layout::PANEL_RADIUS))
            .border_1()
            .border_color(colors.warning)
            .child(section_title(
                t("settings.skills.review_title"),
                colors.text_primary,
            ))
            .child(format!("{} · {}", preview.name, preview.description))
            .child(t_args(
                "settings.skills.sha256",
                &[("sha", &preview.content_sha256)],
            ))
            .child(t("settings.skills.review_warning"))
            .child(
                div()
                    .id("skills-body-text")
                    .max_h(px(280.))
                    .overflow_y_scroll()
                    .p_2()
                    .rounded_md()
                    .bg(colors.code_bg)
                    .text_size(px(Typography::CODE))
                    .child(preview.body.clone())
                    .scroll_edge_fade(colors.code_bg, ScrollFade::compact()),
            )
            .child(skill_button(
                t("settings.skills.approve_sha").into(),
                "skills-approve-body".into(),
                SkillAction::Mutate(SkillSettingsMutation::ApproveSkill {
                    preview_token: preview.token.clone(),
                }),
                colors,
                cx,
            ))
            .child(skill_button(
                t("settings.action.cancel").into(),
                "skills-cancel-body-preview".into(),
                SkillAction::DismissPreview,
                colors,
                cx,
            ))
            .into_any_element()
    }
}

fn skill_button(
    label: String,
    id: String,
    action: SkillAction,
    colors: vega_theme::ThemeColors,
    cx: &mut gpui_kit::Context<SettingsView>,
) -> AnyElement {
    let key_action = action.clone();
    settings_action_chrome(colors)
        .id(id.clone())
        .interaction_surface(colors.bg_elevated, colors.bg_hover, colors.bg_active, true)
        .focusable()
        .tab_stop(true)
        .focus_visible(move |style| style.border_color(colors.accent))
        .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                this.handle_skills_action(key_action.clone(), cx);
                cx.stop_propagation();
            }
        }))
        .cursor_pointer()
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseUpEvent, _, cx| {
                this.handle_skills_action(action.clone(), cx);
            }),
        )
        .child(label)
        .into_any_element()
}

fn skill_toggle_row(
    label: &'static str,
    value: bool,
    id: &str,
    mutation: SkillSettingsMutation,
    enabled: bool,
    colors: vega_theme::ThemeColors,
    cx: &mut gpui_kit::Context<SettingsView>,
) -> Div {
    let key_mutation = mutation.clone();
    let switch = settings_switch(
        id,
        t_args(
            "settings.skills.toggle_label",
            &[
                ("label", label),
                (
                    "state",
                    &(if value {
                        t("settings.state.on")
                    } else {
                        t("settings.state.off")
                    })
                    .to_string(),
                ),
            ],
        ),
        value,
        enabled,
        colors,
    )
    .when(enabled, |switch| {
        switch
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    this.handle_skills_action(SkillAction::Mutate(mutation.clone()), cx)
                }),
            )
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.handle_skills_action(SkillAction::Mutate(key_mutation.clone()), cx);
                    cx.stop_propagation();
                }
            }))
    });
    settings_field(label, None, switch, colors)
}

fn scope_label(scope: SkillUiScope) -> &'static str {
    match scope {
        SkillUiScope::Project => t("settings.skills.scope.project"),
        SkillUiScope::VegaGlobal => t("settings.skills.scope.global"),
        SkillUiScope::Imported => t("settings.skills.scope.imported"),
    }
}

fn skill_diagnostic_label(diagnostic: &str) -> &str {
    match diagnostic {
        "review_required" => t("settings.skills.diagnostic.review_required"),
        "changed_review_required" => t("settings.skills.diagnostic.changed_review_required"),
        "root_changed" => t("settings.skills.diagnostic.root_changed"),
        "unsafe_path" => t("settings.skills.diagnostic.unsafe_path"),
        "not_regular" => t("settings.skills.diagnostic.not_regular"),
        "hardlink" => t("settings.skills.diagnostic.hardlink"),
        "too_large" => t("settings.skills.diagnostic.too_large"),
        "invalid_utf8" => t("settings.skills.diagnostic.invalid_utf8"),
        "invalid_name" => t("settings.skills.diagnostic.invalid_name"),
        "invalid_format" => t("settings.skills.diagnostic.invalid_format"),
        "malformed_yaml" => t("settings.skills.diagnostic.malformed_yaml"),
        "forbidden_tag" => t("settings.skills.diagnostic.forbidden_tag"),
        "stale" => t("settings.skills.diagnostic.stale"),
        "io" => t("settings.skills.diagnostic.io"),
        _ => diagnostic,
    }
}

fn skill_shadow_label(
    projection: &SkillSettingsProjection,
    candidate: &vega_conversation::types::SkillCandidateView,
) -> Option<String> {
    if candidate.model_winner || candidate.content_sha256.is_none() {
        return None;
    }
    let winner = projection.sources.iter().find(|source| {
        source.candidates.iter().any(|other| {
            other.name == candidate.name
                && other.model_winner
                && other
                    .content_sha256
                    .as_ref()
                    .is_some_and(|sha| other.approved_sha256.as_ref() == Some(sha))
                && other.diagnostic.is_none()
        })
    })?;
    Some(match winner.scope {
        SkillUiScope::Project => t("settings.skills.shadow.project").into(),
        SkillUiScope::VegaGlobal => t("settings.skills.shadow.global").into(),
        SkillUiScope::Imported => t_args(
            "settings.skills.shadow.imported",
            &[("source", &winner.source_label)],
        ),
    })
}

fn skill_error_label(error: SkillSettingsError) -> SettingsText {
    match error {
        SkillSettingsError::Store => SettingsText::key("settings.skills.error.store"),
        SkillSettingsError::NotFound => SettingsText::key("settings.skills.error.missing"),
        SkillSettingsError::Stale => SettingsText::key("settings.skills.error.stale"),
        SkillSettingsError::Invalid => SettingsText::key("settings.skills.error.invalid"),
        SkillSettingsError::TooManyCandidates => SettingsText::key("settings.skills.error.limit"),
        SkillSettingsError::PreviewRequired => SettingsText::key("settings.skills.error.preview"),
        SkillSettingsError::SelectionLimit => SettingsText::key("settings.skills.error.selection"),
    }
}
