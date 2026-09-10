#![allow(dead_code)]

//! R13 cached projections and one revision-checked background organization lane.
use super::*;
use std::collections::HashMap;
use vega_conversation::sidebar_organization as service;
use vega_conversation::types::{
    SidebarCollapseTarget, SidebarGroupColor, SidebarOrganizationAction,
    SidebarOrganizationOutcome, SidebarOrganizationSnapshot, SidebarProject, SidebarTaskSort,
    SidebarTimelineBucket, SidebarView,
};
mod menu;
mod projections;
mod render;
#[cfg(test)]
mod tests;

pub(super) struct Organization {
    pub(super) snapshot: Option<SidebarOrganizationSnapshot>,
    projects: Entity<ProjectsBlock>,
    pending: bool,
    refresh_queued: bool,
    generation: u64,
    more: HashMap<String, usize>,
    editor: Option<GroupEditor>,
    menu: Option<OrganizationMenu>,
    menu_focus: FocusHandle,
    menu_index: usize,
    menu_scroll: gpui_kit::ScrollHandle,
    archive: bool,
    reveal_project: Option<(String, std::rc::Rc<std::cell::Cell<bool>>)>,
}
struct GroupEditor {
    group_id: Option<String>,
    input: Entity<TextInput>,
}
#[derive(Clone)]
enum OrganizationMenu {
    Filter,
    Group(String),
    Project(String),
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum OrganizationSection {
    Projects,
    Recents,
}
#[derive(Clone)]
enum MenuCommand {
    Apply(SidebarOrganizationAction),
    Rename(String),
    NewTask(String),
    RemoveProject(String),
    ToggleArchive,
}
#[derive(Clone)]
struct OrganizationDrag {
    kind: DragKind,
    label: String,
}
#[derive(Clone)]
enum DragKind {
    Project(String),
    Group(String),
    Thread(String),
}
impl Render for OrganizationDrag {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        div()
            .px_3()
            .py_2()
            .rounded_md()
            .bg(colors.bg_elevated)
            .border_1()
            .border_color(colors.border_subtle)
            .text_size(px(Typography::SIDEBAR))
            .text_color(colors.text_primary)
            .child(self.label.clone())
    }
}

impl ThreadsBlock {
    pub(crate) fn enable_organization(
        &mut self,
        projects: Entity<ProjectsBlock>,
        cx: &mut Context<Self>,
    ) {
        projects.update(cx, |projects, _| projects.organization_mode = true);
        cx.observe(&projects, |_, _, cx| cx.notify()).detach();
        self.organization = Some(Organization {
            snapshot: None,
            projects,
            pending: false,
            refresh_queued: false,
            generation: 0,
            more: HashMap::new(),
            editor: None,
            menu: None,
            menu_focus: cx.focus_handle(),
            menu_index: 0,
            menu_scroll: gpui_kit::ScrollHandle::new(),
            archive: false,
            reveal_project: None,
        });
        self.refresh_organization(cx);
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(2))
                    .await;
                if this
                    .update(cx, |this, cx| this.refresh_organization(cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }
    pub(crate) fn reveal_registered_project(&mut self, project_id: &str, cx: &mut Context<Self>) {
        if let Some(org) = &mut self.organization {
            org.reveal_project = Some((
                project_id.to_owned(),
                std::rc::Rc::new(std::cell::Cell::new(true)),
            ));
        }
        self.refresh_organization(cx);
    }

    pub(super) fn refresh_organization(&mut self, cx: &mut Context<Self>) {
        self.organization_request(None, cx);
    }
    pub(super) fn submit_organization(
        &mut self,
        action: SidebarOrganizationAction,
        cx: &mut Context<Self>,
    ) {
        self.organization_request(Some(action), cx);
    }
    fn organization_request(
        &mut self,
        action: Option<SidebarOrganizationAction>,
        cx: &mut Context<Self>,
    ) {
        let Some(org) = self.organization.as_mut() else {
            return;
        };
        if org.pending || task_mutation_busy(cx) {
            org.refresh_queued = true;
            if action.is_some() {
                self.error = Some("组织正在保存，请稍后重试".into());
                cx.notify();
            }
            return;
        }
        if action.is_some() && org.snapshot.is_none() {
            return;
        }
        let database = with_store(cx, |store| {
            store
                .database_path()
                .map(Path::to_path_buf)
                .ok_or_else(|| "组织存储需要文件数据库".into())
        });
        let Ok(database) = database else {
            self.error = database.err();
            cx.notify();
            return;
        };
        let revision = org.snapshot.as_ref().map_or(0, |s| s.revision);
        let epoch = cx
            .try_global::<crate::navigation::TaskMutationState>()
            .map_or(0, |s| s.epoch);
        org.pending = true;
        org.generation = org.generation.wrapping_add(1);
        let generation = org.generation;
        let submitted_editor = if matches!(
            &action,
            Some(
                SidebarOrganizationAction::CreateGroup { .. }
                    | SidebarOrganizationAction::RenameGroup { .. }
            )
        ) {
            org.editor.as_ref().map(|editor| {
                (
                    editor.input.entity_id(),
                    editor.group_id.clone(),
                    editor.input.read(cx).text().to_string(),
                )
            })
        } else {
            None
        };
        let origin_route = (
            cx.global::<SelectedProject>().0.clone(),
            cx.global::<OpenedThread>().0.as_ref().map(|t| t.id.clone()),
            cx.try_global::<SettingsOpen>().is_some_and(|s| s.0),
        );
        let was_mutation = action.is_some();
        let worker = cx.background_executor().spawn(async move {
            let action = match action {
                Some(SidebarOrganizationAction::CreateThreadInGroup {
                    project_id,
                    group_id,
                    ..
                }) => {
                    let config = config::load().map_err(|e| e.to_string())?;
                    Some(SidebarOrganizationAction::CreateThreadInGroup {
                        project_id,
                        group_id,
                        model: config.defaults.model,
                        permission_mode: config.defaults.permission_mode,
                    })
                }
                action => action,
            };
            let store = Store::open(database).map_err(|e| e.to_string())?;
            match action {
                Some(action) => service::apply(&store, revision, action),
                None => service::snapshot(&store).map(|snapshot| SidebarOrganizationOutcome {
                    snapshot,
                    created_thread: None,
                }),
            }
            .map_err(|e| e.to_string())
        });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            this.update(cx, |this, cx| {
                let Some(org) = this.organization.as_mut() else {
                    return;
                };
                if generation != org.generation {
                    return;
                }
                org.pending = false;
                let current_epoch = cx
                    .try_global::<crate::navigation::TaskMutationState>()
                    .map_or(0, |s| s.epoch);
                let metadata_stale = epoch != current_epoch || task_mutation_busy(cx);
                let mut created_id = None;
                match result {
                    Ok(outcome) => {
                        let snapshot = outcome.snapshot;
                        created_id = outcome.created_thread.map(|t| t.id);
                        if submitted_editor
                            .as_ref()
                            .is_some_and(|(entity, group, text)| {
                                org.editor.as_ref().is_some_and(|editor| {
                                    editor.input.entity_id() == *entity
                                        && &editor.group_id == group
                                        && editor.input.read(cx).text() == text
                                })
                            })
                        {
                            org.editor = None;
                        }
                        if org
                            .snapshot
                            .as_ref()
                            .is_none_or(|old| old.revision <= snapshot.revision)
                        {
                            if !metadata_stale {
                                this.threads = snapshot
                                    .threads
                                    .iter()
                                    .filter(|t| t.status == ThreadStatus::Active)
                                    .cloned()
                                    .collect();
                                this.archived = snapshot
                                    .threads
                                    .iter()
                                    .filter(|t| t.status == ThreadStatus::Archived)
                                    .cloned()
                                    .collect();
                            }
                            org.snapshot = Some(snapshot);
                        }
                        if was_mutation {
                            this.error = None;
                        }
                    }
                    Err(error) => {
                        this.error = Some(error);
                        if was_mutation {
                            org.refresh_queued = true;
                        }
                    }
                }
                let refresh = std::mem::take(&mut org.refresh_queued) || metadata_stale;
                let route_matches = origin_route
                    == (
                        cx.global::<SelectedProject>().0.clone(),
                        cx.global::<OpenedThread>().0.as_ref().map(|t| t.id.clone()),
                        cx.try_global::<SettingsOpen>().is_some_and(|s| s.0),
                    );
                if let Some(id) = created_id.filter(|_| route_matches) {
                    this.open_thread(&id, cx);
                } else if refresh {
                    this.refresh_organization(cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }
    pub(super) fn organization_actions(
        &self,
        thread_id: &str,
    ) -> Vec<(String, SidebarOrganizationAction)> {
        // R15 has exactly two sidebar concepts: project folders and tasks.
        // Legacy group memberships remain readable for migration compatibility,
        // but no task action can expose or mutate that retired organization IA.
        let _ = thread_id;
        Vec::new()
    }
    fn edit_group(
        &mut self,
        group_id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_actions();
        let Some(org) = self.organization.as_mut() else {
            return;
        };
        let name = group_id
            .as_ref()
            .and_then(|id| org.snapshot.as_ref()?.groups.iter().find(|g| &g.id == id))
            .map(|g| g.name.clone())
            .unwrap_or_default();
        let input = cx.new(|cx| TextInput::new(cx, "分组名称（1–64字）", false));
        input.update(cx, |input, cx| input.set_text(&name, cx));
        window.focus(&input.read(cx).focus_handle(cx), cx);
        org.editor = Some(GroupEditor { group_id, input });
        org.menu = None;
        self.error = None;
        cx.notify();
    }
    fn commit_group(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.organization.as_ref().and_then(|o| o.editor.as_ref()) else {
            return;
        };
        let name = editor.input.read(cx).text().trim().to_string();
        if !(1..=64).contains(&name.chars().count()) {
            self.error = Some("分组名称需为 1–64 个字符".into());
            cx.notify();
            return;
        }
        let action = match &editor.group_id {
            Some(id) => SidebarOrganizationAction::RenameGroup {
                group_id: id.clone(),
                name,
            },
            None => SidebarOrganizationAction::CreateGroup {
                name,
                color: SidebarGroupColor::Gray,
            },
        };
        self.submit_organization(action, cx);
    }
    fn toggle_organization_collapse(
        &mut self,
        target: SidebarCollapseTarget,
        cx: &mut Context<Self>,
    ) {
        let collapsed = self
            .organization
            .as_ref()
            .and_then(|o| o.snapshot.as_ref())
            .is_some_and(|s| s.collapsed.contains(&target));
        self.submit_organization(
            SidebarOrganizationAction::SetCollapsed {
                target,
                collapsed: !collapsed,
            },
            cx,
        );
    }
    fn new_group_task(&mut self, group_id: String, cx: &mut Context<Self>) {
        if !crate::navigation::allow_task_navigation(None, cx) {
            return;
        }
        let Some(project_id) = cx.global::<SelectedProject>().0.clone() else {
            self.error = Some("请先添加并选择项目，再在分组中新建任务".into());
            if let Some(org) = &self.organization {
                org.projects.update(cx, ProjectsBlock::open_picker);
            }
            cx.notify();
            return;
        };
        self.submit_organization(
            SidebarOrganizationAction::CreateThreadInGroup {
                project_id,
                group_id,
                model: String::new(),
                permission_mode: String::new(),
            },
            cx,
        );
    }
}
