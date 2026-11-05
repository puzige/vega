use crate::interaction::InteractionExt;
use std::path::PathBuf;

use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Div, KeyDownEvent, MouseButton, MouseUpEvent, PathPromptOptions, div, px,
};
use vega_conversation::types::{
    SkillRootPreview, SkillSettingsMutation, SkillSettingsProjection, SkillSourceView, SkillUiScope,
};
use vega_conversation::{SkillSettingsError, SkillSettingsService};
use vega_i18n::{t, t_args};
use vega_theme::{Layout, Typography, theme};

use super::{
    SettingsText, SettingsView, TextInput, section_title, settings_action_chrome, settings_actions,
    settings_field, settings_group, settings_section, settings_status, settings_switch,
};

#[derive(Default)]
pub(crate) struct SkillsSettingsState {
    service: Option<SkillSettingsService>,
    projection: Option<SkillSettingsProjection>,
    root_preview: Option<SkillRootPreview>,
    busy: bool,
    message: Option<SettingsText>,
    request_generation: u64,
    search: Option<gpui_kit::Entity<TextInput>>,
    show_sources: bool,
    show_add: bool,
}

#[derive(Clone)]
enum SkillAction {
    ToggleSources,
    ToggleAdd,
    Reload,
    PreviewProject,
    PreviewGlobal,
    PreviewSource { source_id: String },
    ImportFolder,
    Mutate(SkillSettingsMutation),
    DismissPreview,
}

enum SkillOperation {
    Reload,
    PreviewProject,
    PreviewGlobal,
    PreviewSource {
        source_id: String,
    },
    PreviewImported(PathBuf),
    Mutate {
        generation: u64,
        mutation: SkillSettingsMutation,
    },
}

enum SkillOperationResult {
    Projection(SkillSettingsProjection),
    Root(SkillRootPreview),
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
                SkillOperation::PreviewSource { source_id } => service
                    .preview_source_root(&source_id)
                    .map(SkillOperationResult::Root),
                SkillOperation::PreviewImported(path) => service
                    .preview_imported_root(&path)
                    .map(SkillOperationResult::Root),
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
                    }
                    Ok(SkillOperationResult::Root(preview)) => {
                        this.skills.root_preview = Some(preview);
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
            SkillAction::ToggleSources => {
                self.skills.show_sources = !self.skills.show_sources;
                cx.notify();
            }
            SkillAction::ToggleAdd => {
                self.skills.show_add = !self.skills.show_add;
                cx.notify();
            }
            SkillAction::Reload => self.skills_operation(SkillOperation::Reload, cx),
            SkillAction::PreviewProject => {
                self.skills_operation(SkillOperation::PreviewProject, cx)
            }
            SkillAction::PreviewGlobal => self.skills_operation(SkillOperation::PreviewGlobal, cx),
            SkillAction::PreviewSource { source_id } => {
                if !self.skills.busy {
                    self.skills_operation(SkillOperation::PreviewSource { source_id }, cx);
                }
            }
            SkillAction::ImportFolder => self.pick_skills_root(cx),
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
                if let Some(service) = &self.skills.service {
                    service.clear_previews();
                }
                cx.notify();
            }
        }
    }

    pub(crate) fn render_skills(&mut self, cx: &mut gpui_kit::Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        if self.skills.search.is_none() {
            let search = cx.new(|cx| {
                TextInput::new(cx, "", false)
                    .with_placeholder_key("settings.skills.search")
                    .with_tab_stop(true)
            });
            cx.observe(&search, |_, _, cx| cx.notify()).detach();
            self.skills.search = Some(search);
        }
        let query = self
            .skills
            .search
            .as_ref()
            .map(|input| input.read(cx).text().trim().to_lowercase())
            .unwrap_or_default();
        let mut column = settings_section().id("settings-skills").child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .flex_wrap()
                .gap_2()
                .child(section_title(
                    t("settings.skills.title"),
                    colors.text_primary,
                ))
                .child(
                    settings_actions()
                        .child(skill_button(
                            t("settings.action.refresh").into(),
                            "skills-reload".into(),
                            SkillAction::Reload,
                            colors,
                            cx,
                        ))
                        .child(skill_button(
                            t("settings.skills.sources").into(),
                            "skills-source-settings".into(),
                            SkillAction::ToggleSources,
                            colors,
                            cx,
                        ))
                        .child(skill_button(
                            t("settings.skills.add").into(),
                            "skills-add".into(),
                            SkillAction::ToggleAdd,
                            colors,
                            cx,
                        )),
                ),
        );
        if let Some(search) = &self.skills.search {
            column = column.child(
                div().flex().justify_end().child(
                    div()
                        .w(px(Layout::SETTINGS_LABEL_WIDTH))
                        .max_w_full()
                        .child(search.clone()),
                ),
            );
        }
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
            if self.skills.show_add {
                column = column.child(
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
            }
            if self.skills.show_sources {
                column = column.child(preferences);
                for source in &projection.sources {
                    column = column.child(self.render_skill_source(source, &projection, cx));
                }
            }
            if projection.sources.is_empty() {
                column = column.child(settings_status(
                    t("settings.skills.no_sources"),
                    colors.text_secondary,
                    colors,
                ));
            }
            let mut matched = 0;
            for source in &projection.sources {
                if source.candidates.is_empty()
                    && let Some(diagnostic) = &source.diagnostic
                {
                    column = column.child(settings_status(
                        format!(
                            "{} · {}",
                            scope_label(source.scope),
                            skill_diagnostic_label(diagnostic)
                        ),
                        colors.warning,
                        colors,
                    ));
                }
                for candidate in &source.candidates {
                    let searchable = format!(
                        "{} {} {} {}",
                        candidate.name,
                        candidate.description.as_deref().unwrap_or_default(),
                        scope_label(source.scope),
                        source.configured_root.display()
                    )
                    .to_lowercase();
                    if !query.is_empty() && !searchable.contains(&query) {
                        continue;
                    }
                    matched += 1;
                    column =
                        column.child(self.render_skill_row(source, candidate, &projection, cx));
                }
            }
            if matched == 0 && !query.is_empty() {
                column = column.child(settings_status(
                    t("settings.skills.no_matches"),
                    colors.text_secondary,
                    colors,
                ));
            }
        }
        if let Some(preview) = self.skills.root_preview.clone() {
            column = column.child(self.render_root_preview(&preview, cx));
        }
        column.into_any_element()
    }

    fn render_skill_source(
        &self,
        source: &SkillSourceView,
        _projection: &SkillSettingsProjection,
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
            .child(
                if !source.enabled
                    && source.diagnostic.as_deref() == Some("changed_review_required")
                {
                    settings_field(
                        t("settings.skills.enable_directory"),
                        None,
                        skill_button(
                            t("settings.skills.review_directory").into(),
                            format!("skills-source-review-{}", source.id),
                            SkillAction::PreviewSource {
                                source_id: source.id.clone(),
                            },
                            colors,
                            cx,
                        ),
                        colors,
                    )
                } else {
                    skill_toggle_row(
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
                    )
                },
            )
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
        card.into_any_element()
    }

    fn render_skill_row(
        &self,
        source: &SkillSourceView,
        candidate: &vega_conversation::types::SkillCandidateView,
        projection: &SkillSettingsProjection,
        cx: &mut gpui_kit::Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let source_name = skill_source_name(source, projection);
        let toggle_label = format!("{} · {}", candidate.name, source.configured_root.display());
        let mut full_label = format!("{} ({source_name})", candidate.name);
        let mut text = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_1()
            .child(div().min_w_0().truncate().child(candidate.name.clone()));
        if let Some(description) = &candidate.description {
            full_label.push_str(&format!(": {description}"));
            text = text.child(
                div()
                    .min_w_0()
                    .text_size(px(Typography::METADATA))
                    .text_color(colors.text_secondary)
                    .truncate()
                    .child(description.clone()),
            );
        }
        let source_enabled = source.enabled
            && match source.scope {
                SkillUiScope::Project => projection.project_enabled,
                _ => projection.global_enabled,
            };
        if let Some(diagnostic) = candidate
            .diagnostic
            .as_deref()
            .or(source.diagnostic.as_deref())
        {
            let label = skill_diagnostic_label(diagnostic).to_owned();
            full_label.push_str(&format!(": {label}"));
            text = text.child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(px(Typography::METADATA))
                    .text_color(colors.warning)
                    .child(label),
            );
        } else if !source_enabled {
            let label = t("settings.skills.source_disabled");
            full_label.push_str(&format!(": {label}"));
            text = text.child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(px(Typography::METADATA))
                    .text_color(colors.text_secondary)
                    .child(label),
            );
        } else if let Some(label) = skill_shadow_label(projection, candidate) {
            full_label.push_str(&format!(": {label}"));
            text = text.child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(px(Typography::METADATA))
                    .text_color(colors.text_secondary)
                    .child(label),
            );
        }
        let mut row = div()
            .id(format!("skills-candidate-{}-{}", source.id, candidate.name))
            .aria_label(full_label.clone())
            .tooltip(move |_, cx| crate::icons::tooltip(full_label.clone(), cx))
            .flex()
            .items_center()
            .gap(px(Layout::SETTINGS_CONTENT_GAP))
            .min_w_0()
            .py(px(Layout::SETTINGS_CONTENT_GAP))
            .child(
                div()
                    .size(px(Layout::SETTINGS_CONTROL_MIN_HEIGHT))
                    .flex_shrink_0()
                    .rounded_full()
                    .border_1()
                    .border_color(colors.border_subtle)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(crate::icons::icon(
                        crate::icons::Icon::Blocks,
                        colors.text_secondary,
                    )),
            )
            .child(text)
            .child(
                div()
                    .flex_shrink_0()
                    .text_size(px(Typography::METADATA))
                    .text_color(colors.text_secondary)
                    .max_w(px(Layout::SETTINGS_LABEL_WIDTH))
                    .truncate()
                    .child(source_name),
            );
        if candidate.content_sha256.is_some() {
            row = row.child(skill_switch(
                &toggle_label,
                candidate.enabled,
                &format!("skills-enabled-{}-{}", source.id, candidate.name),
                SkillSettingsMutation::SetSkill {
                    source_id: source.id.clone(),
                    name: candidate.name.clone(),
                    enabled: !candidate.enabled,
                    automatic: !candidate.enabled,
                },
                !self.skills.busy,
                colors,
                cx,
            ));
        }
        row.into_any_element()
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
                let label = candidate.diagnostic.as_deref().map_or_else(
                    || candidate.name.clone(),
                    |diagnostic| {
                        format!(
                            "{} · {}",
                            candidate.name,
                            skill_diagnostic_label(diagnostic)
                        )
                    },
                );
                div().text_color(colors.text_secondary).child(label)
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

fn skill_switch(
    label: &str,
    value: bool,
    id: &str,
    mutation: SkillSettingsMutation,
    enabled: bool,
    colors: vega_theme::ThemeColors,
    cx: &mut gpui_kit::Context<SettingsView>,
) -> crate::interaction::Interaction {
    let key_mutation = mutation.clone();
    settings_switch(
        id,
        t_args(
            "settings.skills.toggle_label",
            &[
                ("label", label),
                (
                    "state",
                    if value {
                        t("settings.state.on")
                    } else {
                        t("settings.state.off")
                    },
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
    })
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
    settings_field(
        label,
        None,
        skill_switch(label, value, id, mutation, enabled, colors, cx),
        colors,
    )
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
        "invalid_metadata" => t("settings.skills.diagnostic.invalid_metadata"),
        "invalid_name_field" => t("settings.skills.diagnostic.invalid_name_field"),
        "invalid_description" => t("settings.skills.diagnostic.invalid_description"),
        "invalid_license" => t("settings.skills.diagnostic.invalid_license"),
        "invalid_compatibility" => t("settings.skills.diagnostic.invalid_compatibility"),
        "invalid_allowed_tools" => t("settings.skills.diagnostic.invalid_allowed_tools"),
        "forbidden_tag" => t("settings.skills.diagnostic.forbidden_tag"),
        "stale" => t("settings.skills.diagnostic.stale"),
        "io" => t("settings.skills.diagnostic.io"),
        _ => diagnostic,
    }
}

fn skill_source_name(source: &SkillSourceView, projection: &SkillSettingsProjection) -> String {
    if source.scope != SkillUiScope::Imported {
        return scope_label(source.scope).into();
    }
    let components = source.configured_root.components().collect::<Vec<_>>();
    for count in 1..=components.len() {
        let suffix = components[components.len() - count..]
            .iter()
            .collect::<PathBuf>();
        let unique = projection
            .sources
            .iter()
            .all(|other| other.id == source.id || !other.configured_root.ends_with(&suffix));
        if unique || count == components.len() {
            return suffix.display().to_string();
        }
    }
    source.source_label.clone()
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
                && other.content_sha256.is_some()
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
