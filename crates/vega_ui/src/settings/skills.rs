use std::path::PathBuf;

use gpui_kit::PathPromptOptions;
use gpui_kit::prelude::*;
use vega_conversation::types::{
    SkillRootPreview, SkillSettingsMutation, SkillSettingsProjection, SkillSourceView, SkillUiScope,
};
use vega_conversation::{SkillSettingsError, SkillSettingsService};
use vega_i18n::{t, t_args};

use super::{SettingsText, SettingsView};

pub(super) mod nsh_skills;

#[derive(Default)]
pub(crate) struct SkillsSettingsState {
    service: Option<SkillSettingsService>,
    projection: Option<SkillSettingsProjection>,
    root_preview: Option<SkillRootPreview>,
    busy: bool,
    message: Option<SettingsText>,
    request_generation: u64,
    presentation: Option<nsh_skills::Presentation>,
    show_sources: bool,
    show_add: bool,
}

#[derive(Clone, PartialEq, Eq)]
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
