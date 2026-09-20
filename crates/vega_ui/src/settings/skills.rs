//! Issue #74 Skills Settings. This view only talks to the conversation-owned
//! service; no Skill path, file or SQLite row is trusted on the UI thread.

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
use vega_theme::{Typography, theme};

use super::{SettingsView, section_title};

#[derive(Default)]
pub(crate) struct SkillsSettingsState {
    service: Option<SkillSettingsService>,
    projection: Option<SkillSettingsProjection>,
    root_preview: Option<SkillRootPreview>,
    body_preview: Option<SkillBodyPreview>,
    busy: bool,
    message: Option<String>,
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
            self.skills.message = Some("Skills 数据库不可用".into());
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
        let Some(service) = self.skills.service.clone() else {
            self.skills.message = Some("Skills 数据库不可用".into());
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
                        this.skills.message = Some(skill_error_label(error).into());
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
            prompt: Some("选择要导入的 Skills 目录".into()),
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
                        this.skills.message = Some("文件夹选择失败，请重试".into());
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
        let mut column = div()
            .id("settings-skills")
            .debug_selector(|| "settings-skills".into())
            .flex()
            .flex_col()
            .gap_4()
            .child(section_title("Agent Skills", colors.text_primary))
            .child(
                div()
                    .text_color(colors.text_secondary)
                    .child("Skills 是低信任的工作流程说明，不授予脚本、MCP 或文件工具额外权限。全局与自动触发默认关闭。"),
            )
            .child(skill_button(
                "刷新".into(),
                "skills-reload".into(),
                SkillAction::Reload,
                colors,
                cx,
            ));
        if self.skills.busy {
            column = column.child(
                div()
                    .text_color(colors.text_secondary)
                    .child("正在检查 Skills…"),
            );
        }
        if let Some(message) = &self.skills.message {
            column = column.child(div().text_color(colors.warning).child(message.clone()));
        }
        if let Some(projection) = self.skills.projection.clone() {
            column = column
                .child(section_title("来源与触发", colors.text_primary))
                .child(skill_toggle_row(
                    "使用全局 Skills",
                    projection.global_enabled,
                    "skills-global-enabled",
                    SkillSettingsMutation::SetGlobal {
                        enabled: !projection.global_enabled,
                        automatic: projection.automatic_enabled,
                    },
                    colors,
                    cx,
                ))
                .child(skill_toggle_row(
                    "自动选择全局 Skills",
                    projection.automatic_enabled,
                    "skills-global-automatic",
                    SkillSettingsMutation::SetGlobal {
                        enabled: projection.global_enabled,
                        automatic: !projection.automatic_enabled,
                    },
                    colors,
                    cx,
                ));
            if let Some(project_id) = projection.selected_project_id.clone() {
                column = column
                    .child(skill_toggle_row(
                        "使用当前项目 Skills",
                        projection.project_enabled,
                        "skills-project-enabled",
                        SkillSettingsMutation::SetProject {
                            project_id: project_id.clone(),
                            enabled: !projection.project_enabled,
                            automatic: projection.project_automatic,
                        },
                        colors,
                        cx,
                    ))
                    .child(skill_toggle_row(
                        "自动选择当前项目 Skills",
                        projection.project_automatic,
                        "skills-project-automatic",
                        SkillSettingsMutation::SetProject {
                            project_id,
                            enabled: projection.project_enabled,
                            automatic: !projection.project_automatic,
                        },
                        colors,
                        cx,
                    ))
                    .child(skill_button(
                        "检查当前项目 .agents/skills".into(),
                        "skills-preview-project".into(),
                        SkillAction::PreviewProject,
                        colors,
                        cx,
                    ));
            }
            column = column
                .child(skill_button(
                    "检查 Vega 全局 Skills".into(),
                    "skills-preview-global".into(),
                    SkillAction::PreviewGlobal,
                    colors,
                    cx,
                ))
                .child(skill_button(
                    "导入外部 Skills 目录…".into(),
                    "skills-import-folder".into(),
                    SkillAction::ImportFolder,
                    colors,
                    cx,
                ));
            if projection.sources.is_empty() {
                column = column.child(
                    div()
                        .text_color(colors.text_secondary)
                        .child("尚未关联任何 Skills 目录"),
                );
            }
            for source in &projection.sources {
                column = column.child(self.render_skill_source(source, cx));
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
        cx: &mut gpui_kit::Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let scope = scope_label(source.scope);
        let mut card = div()
            .id(format!("skills-source-{}", source.id))
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .rounded_md()
            .border_1()
            .border_color(colors.border_subtle)
            .bg(colors.bg_elevated)
            .child(section_title(scope, colors.text_primary))
            .child(
                div()
                    .text_color(colors.text_secondary)
                    .child(format!("来源标签：{}", source.source_label)),
            )
            .child(
                div()
                    .text_color(colors.text_secondary)
                    .child(format!("目录：{}", source.configured_root.display())),
            )
            .child(
                div()
                    .text_color(colors.text_secondary)
                    .child(format!("实际目标：{}", source.canonical_root.display())),
            )
            .child(skill_toggle_row(
                "启用此目录",
                source.enabled,
                &format!("skills-source-enabled-{}", source.id),
                SkillSettingsMutation::SetSource {
                    source_id: source.id.clone(),
                    enabled: !source.enabled,
                    automatic: source.automatic,
                },
                colors,
                cx,
            ))
            .child(skill_toggle_row(
                "允许此目录自动触发",
                source.automatic,
                &format!("skills-source-auto-{}", source.id),
                SkillSettingsMutation::SetSource {
                    source_id: source.id.clone(),
                    enabled: source.enabled,
                    automatic: !source.automatic,
                },
                colors,
                cx,
            ))
            .child(skill_button(
                "移除关联（保留源文件）".into(),
                format!("skills-unlink-{}", source.id),
                SkillAction::Mutate(SkillSettingsMutation::UnlinkSource {
                    source_id: source.id.clone(),
                }),
                colors,
                cx,
            ));
        if let Some(diagnostic) = &source.diagnostic {
            card = card.child(
                div()
                    .text_color(colors.warning)
                    .child(format!("来源不可用：{diagnostic}")),
            );
        }
        for candidate in &source.candidates {
            let status = candidate.diagnostic.clone().unwrap_or_else(|| {
                if candidate.model_winner {
                    "自动目录优先项"
                } else {
                    "已审阅"
                }
                .into()
            });
            let mut row = div()
                .id(format!("skills-candidate-{}-{}", source.id, candidate.name))
                .flex()
                .flex_col()
                .gap_2()
                .pt_2()
                .border_t_1()
                .border_color(colors.border_subtle)
                .child(div().child(format!("{} · {status}", candidate.name)));
            if let Some(description) = &candidate.description {
                row = row.child(
                    div()
                        .text_color(colors.text_secondary)
                        .child(description.clone()),
                );
            }
            if candidate.content_sha256.is_some() {
                row = row.child(skill_button(
                    "预览并审阅当前 SHA".into(),
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
                        "启用此 Skill",
                        candidate.enabled,
                        &format!("skills-enabled-{}-{}", source.id, candidate.name),
                        SkillSettingsMutation::SetSkill {
                            source_id: source.id.clone(),
                            name: candidate.name.clone(),
                            enabled: !candidate.enabled,
                            automatic: candidate.automatic,
                        },
                        colors,
                        cx,
                    ))
                    .child(skill_toggle_row(
                        "允许自动触发",
                        candidate.automatic,
                        &format!("skills-auto-{}-{}", source.id, candidate.name),
                        SkillSettingsMutation::SetSkill {
                            source_id: source.id.clone(),
                            name: candidate.name.clone(),
                            enabled: candidate.enabled,
                            automatic: !candidate.automatic,
                        },
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
            .debug_selector(|| "skills-root-preview".into())
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .rounded_md()
            .border_1()
            .border_color(colors.warning)
            .child(section_title("确认关联目录", colors.text_primary))
            .child(format!("选择目录：{}", preview.configured_root.display()))
            .child(format!("实际目标：{}", preview.canonical_root.display()))
            .child(format!("Vega 来源标签：{}", preview.source_label))
            .child(format!("发现 {} 个候选项", preview.candidates.len()))
            .children(preview.candidates.iter().map(|candidate| {
                div().text_color(colors.text_secondary).child(format!(
                    "{} · {}",
                    candidate.name,
                    candidate
                        .diagnostic
                        .as_deref()
                        .unwrap_or("待逐项审阅")
                ))
            }))
            .child("关联仅允许 Vega 列出候选项；每个 Skill 仍需单独预览并批准 SHA。不会复制、执行或删除源文件。")
            .child(skill_button(
                "关联此精确目录".into(),
                "skills-link-root".into(),
                SkillAction::Mutate(SkillSettingsMutation::LinkRoot {
                    preview_token: preview.token.clone(),
                }),
                colors,
                cx,
            ))
            .child(skill_button(
                "取消".into(),
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
            .debug_selector(|| "skills-body-preview".into())
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .rounded_md()
            .border_1()
            .border_color(colors.warning)
            .child(section_title("审阅 Skill", colors.text_primary))
            .child(format!("{} · {}", preview.name, preview.description))
            .child(format!("SHA-256：{}", preview.content_sha256))
            .child("以下是低信任指令内容。批准它不会执行脚本，也不会授予额外工具权限。引用文件会在首次读取时单独校验。")
            .child(
                div()
                    .id("skills-body-text")
                    .max_h(px(280.))
                    .overflow_y_scroll()
                    .p_2()
                    .rounded_md()
                    .bg(colors.code_bg)
                    .text_size(px(Typography::CODE))
                    .child(preview.body.clone()),
            )
            .child(skill_button(
                "批准当前 SHA".into(),
                "skills-approve-body".into(),
                SkillAction::Mutate(SkillSettingsMutation::ApproveSkill {
                    preview_token: preview.token.clone(),
                }),
                colors,
                cx,
            ))
            .child(skill_button(
                "取消".into(),
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
    div()
        .id(id.clone())
        .debug_selector(move || id.clone())
        .focusable()
        .tab_stop(true)
        .focus_visible(move |style| style.border_1().border_color(colors.brand_primary))
        .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                this.handle_skills_action(key_action.clone(), cx);
                cx.stop_propagation();
            }
        }))
        .cursor_pointer()
        .hover(move |style| style.bg(colors.bg_hover))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseUpEvent, _, cx| {
                this.handle_skills_action(action.clone(), cx);
            }),
        )
        .px_2()
        .py_1()
        .rounded_md()
        .border_1()
        .border_color(colors.border_subtle)
        .bg(colors.bg_elevated)
        .child(label)
        .into_any_element()
}

fn skill_toggle_row(
    label: &'static str,
    value: bool,
    id: &str,
    mutation: SkillSettingsMutation,
    colors: vega_theme::ThemeColors,
    cx: &mut gpui_kit::Context<SettingsView>,
) -> Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(label)
        .child(skill_button(
            if value { "已开启" } else { "已关闭" }.into(),
            id.to_owned(),
            SkillAction::Mutate(mutation),
            colors,
            cx,
        ))
}

fn scope_label(scope: SkillUiScope) -> &'static str {
    match scope {
        SkillUiScope::Project => "当前项目",
        SkillUiScope::VegaGlobal => "Vega 全局",
        SkillUiScope::Imported => "外部导入（按引用）",
    }
}

fn skill_error_label(error: SkillSettingsError) -> &'static str {
    match error {
        SkillSettingsError::Store => "Skills 数据库读写失败，请重试",
        SkillSettingsError::NotFound => "目录或 Skill 不存在，请刷新",
        SkillSettingsError::Stale => "来源或文件已变化，请重新预览并审阅",
        SkillSettingsError::Invalid => "目录不安全或内容无效，未更改授权",
        SkillSettingsError::PreviewRequired => "请先预览当前内容，再确认授权",
        SkillSettingsError::SelectionLimit => "一个任务最多可选择三个 Skills",
    }
}

#[cfg(test)]
mod tests;
