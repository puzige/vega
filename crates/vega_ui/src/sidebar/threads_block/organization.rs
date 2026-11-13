#![allow(dead_code)]

use super::*;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};
use vega_conversation::sidebar_organization as service;
use vega_conversation::types::{
    SidebarCollapseTarget, SidebarGroupColor, SidebarOrganizationAction,
    SidebarOrganizationOutcome, SidebarOrganizationSnapshot, SidebarProject, SidebarProjectView,
    SidebarTaskSort, SidebarTimelineBucket, SidebarView,
};
mod collapse;
mod menu;
mod projections;
mod reorder;
mod task_projection;
pub(super) use reorder::ReorderItem;
use reorder::ReorderState;
mod render;

struct OrganizationRequestToken {
    database: PathBuf,
    generation: u64,
    epoch: u64,
    origin_route: (Option<String>, Option<String>, bool),
    submitted_editor: Option<(gpui_kit::EntityId, Option<String>, String)>,
    was_mutation: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PendingRequest {
    Read,
    Write,
}

pub(super) struct Organization {
    pub(super) snapshot: Option<std::rc::Rc<SidebarOrganizationSnapshot>>,
    collapse: collapse::CollapseState,
    pub(super) task_projection: Option<std::rc::Rc<task_projection::TaskProjection>>,
    pub(super) focus_dirty: bool,
    database: Option<PathBuf>,
    projects: Entity<ProjectsBlock>,
    pending: Option<PendingRequest>,
    reorder: ReorderState,
    refresh_queued: bool,
    generation: u64,
    presentation_generation: u64,
    last_clock_ms: i64,
    pub(super) next_time_change: std::cell::Cell<i64>,
    project_threads_visible: HashMap<String, usize>,
    project_threads_total: HashMap<String, usize>,
    project_growth_paused: HashSet<String>,
    editor: Option<GroupEditor>,
    menu: Option<OrganizationMenu>,
    menu_anchor: Option<(gpui_kit::Point<gpui_kit::Pixels>, Anchor)>,
    menu_focus: FocusHandle,
    menu_index: usize,
    menu_scroll: gpui_kit::ScrollHandle,
    archive: bool,
    reveal_project: Option<(String, std::rc::Rc<std::cell::Cell<bool>>)>,
}
impl Organization {
    fn has_project_tasks_to_grow(
        &self,
        visible_project_ids: &HashSet<String>,
        collapsed: &[SidebarCollapseTarget],
    ) -> bool {
        self.project_threads_total
            .iter()
            .any(|(project_id, total)| {
                visible_project_ids.contains(project_id)
                    && !self.project_growth_paused.contains(project_id)
                    && !collapsed.contains(&SidebarCollapseTarget::Project(project_id.clone()))
                    && self
                        .project_threads_visible
                        .get(project_id)
                        .copied()
                        .unwrap_or(PROJECT_TASKS_PAGE)
                        < *total
            })
    }

    fn grow_project_tasks(
        &mut self,
        visible_project_ids: &HashSet<String>,
        collapsed: &[SidebarCollapseTarget],
    ) -> bool {
        let mut changed = false;
        for (project_id, total) in &self.project_threads_total {
            if !visible_project_ids.contains(project_id)
                || self.project_growth_paused.contains(project_id)
                || collapsed.contains(&SidebarCollapseTarget::Project(project_id.clone()))
            {
                continue;
            }
            let visible = self
                .project_threads_visible
                .entry(project_id.clone())
                .or_insert(PROJECT_TASKS_PAGE);
            if *visible < *total {
                *visible = visible.saturating_add(PROJECT_TASKS_PAGE).min(*total);
                changed = true;
            }
        }
        changed
    }

    fn has_project_task_page(&self, project_id: &str) -> bool {
        self.project_threads_total
            .get(project_id)
            .is_some_and(|total| {
                self.project_threads_visible
                    .get(project_id)
                    .copied()
                    .unwrap_or(PROJECT_TASKS_PAGE)
                    < *total
            })
    }

    fn grow_project_task_page(&mut self, project_id: &str) -> bool {
        let Some(total) = self.project_threads_total.get(project_id).copied() else {
            return false;
        };
        let visible = self
            .project_threads_visible
            .entry(project_id.to_owned())
            .or_insert(PROJECT_TASKS_PAGE);
        if *visible >= total {
            return false;
        }
        *visible = visible.saturating_add(PROJECT_TASKS_PAGE).min(total);
        self.project_growth_paused.remove(project_id);
        true
    }

    fn sync_project_task_totals(&mut self, totals: HashMap<String, usize>) {
        self.project_threads_visible
            .retain(|id, _| totals.contains_key(id));
        for project_id in totals.keys() {
            self.project_threads_visible
                .entry(project_id.clone())
                .or_insert(PROJECT_TASKS_PAGE);
        }
        self.project_threads_total = totals;
    }
}
struct GroupEditor {
    group_id: Option<String>,
    input: Entity<TextInput>,
}
#[derive(Clone, PartialEq, Eq)]
enum OrganizationMenu {
    Group(String),
    Project(String),
    Recents,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum OrganizationSection {
    Projects,
    Recents,
}
pub(crate) const RECENTS_PAGE: usize = 10;
pub(crate) const PROJECT_TASKS_PAGE: usize = 5;
#[derive(Clone)]
enum MenuCommand {
    Apply(SidebarOrganizationAction),
    Rename(String),
    NewTask(String),
    RemoveProject(String),
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

fn project_row_is_active(project_id: &str, cx: &App) -> bool {
    cx.global::<SelectedProject>().0.as_deref() == Some(project_id)
        && cx
            .try_global::<OpenedThread>()
            .and_then(|opened| opened.0.as_ref())
            .is_none_or(|thread| thread.project_binding() != Some(project_id))
}

impl Render for OrganizationDrag {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _ = cx;
        div().w(px(Layout::SIDEBAR_WIDTH)).child(nsh_leaf::host(
            "sidebar-organization-drag-preview",
            Some(self.label.clone()),
            Vec::new(),
            false,
            Typography::SIDEBAR_LINE_HEIGHT,
            |_| true,
            |_, _, _, _| {},
        ))
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
        cx.observe_global::<VegaStore>(|this, cx| this.refresh_organization(cx))
            .detach();
        self.organization = Some(Organization {
            snapshot: None,
            collapse: collapse::CollapseState::default(),
            task_projection: None,
            focus_dirty: true,
            database: None,
            projects,
            pending: None,
            reorder: ReorderState::new(cx),
            refresh_queued: false,
            generation: 0,
            presentation_generation: 0,
            last_clock_ms: now_ms(),
            next_time_change: std::cell::Cell::new(i64::MAX),
            project_threads_visible: HashMap::new(),
            project_threads_total: HashMap::new(),
            project_growth_paused: HashSet::new(),
            editor: None,
            menu: None,
            menu_anchor: None,
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
    pub(super) fn sync_project_task_windows(
        &mut self,
        projects: &[&SidebarProject],
        show_archived: bool,
    ) {
        let mut totals = HashMap::new();
        for project in projects {
            let total = self
                .threads
                .iter()
                .chain(self.archived.iter())
                .filter(|thread| thread.project_id == project.id && !thread.pinned)
                .filter(|thread| thread.status == ThreadStatus::Active || show_archived)
                .count();
            totals.insert(project.id.clone(), total);
        }
        if let Some(organization) = self.organization.as_mut() {
            organization.sync_project_task_totals(totals);
        }
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

    fn visible_project_ids(
        snapshot: &SidebarOrganizationSnapshot,
        projects_expanded: bool,
    ) -> HashSet<String> {
        let mut projects: Vec<_> = snapshot
            .project_order
            .iter()
            .filter_map(|id| snapshot.projects.iter().find(|project| &project.id == id))
            .collect();
        projects.extend(
            snapshot
                .projects
                .iter()
                .filter(|project| !snapshot.project_order.contains(&project.id)),
        );
        let visible_count = if projects_expanded { projects.len() } else { 5 };
        projects
            .into_iter()
            .take(visible_count)
            .filter(|project| {
                !snapshot
                    .collapsed
                    .contains(&SidebarCollapseTarget::Project(project.id.clone()))
            })
            .map(|project| project.id.clone())
            .collect()
    }

    pub(crate) fn resume_project_scroll_growth(&mut self) {
        if let Some(org) = self.organization.as_mut()
            && let Some(snapshot) = org.snapshot.as_ref()
        {
            let visible = Self::visible_project_ids(snapshot, self.projects_expanded);
            org.project_growth_paused.retain(|id| !visible.contains(id));
        }
    }

    pub(crate) fn has_lazy_list_growth(&self) -> bool {
        self.recents_visible < self.recents_total
            || self.organization.as_ref().is_some_and(|organization| {
                let Some(snapshot) = organization.snapshot.as_ref() else {
                    return false;
                };
                let projects = Self::visible_project_ids(snapshot, self.projects_expanded);
                organization.has_project_tasks_to_grow(&projects, &snapshot.collapsed)
            })
    }

    pub(crate) fn grow_lazy_lists(&mut self, cx: &mut Context<Self>) -> bool {
        let mut changed = false;
        if self.recents_visible < self.recents_total {
            self.recents_visible = self
                .recents_visible
                .saturating_add(organization::RECENTS_PAGE)
                .min(self.recents_total);
            changed = true;
        }
        let projects_expanded = self.projects_expanded;
        let project_growth = self.organization.as_ref().and_then(|organization| {
            let snapshot = organization.snapshot.as_ref()?;
            Some((
                Self::visible_project_ids(snapshot, projects_expanded),
                snapshot.collapsed.clone(),
            ))
        });
        if let (Some(organization), Some((visible_project_ids, collapsed))) =
            (self.organization.as_mut(), project_growth)
        {
            changed |= organization.grow_project_tasks(&visible_project_ids, &collapsed);
        }
        if changed {
            cx.notify();
        }
        changed
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
        let database = with_store(cx, |store| {
            store.database_path().map(Path::to_path_buf).ok_or_else(|| {
                crate::localized_copy::LocalizedCopy::new(
                    "ui.organization_storage_requires_a_file_backed_database",
                )
            })
        });
        let current_database = database.as_ref().ok().cloned();
        let Some(org) = self.organization.as_mut() else {
            return;
        };
        if org.database != current_database {
            org.database = current_database;
            org.generation = org.generation.wrapping_add(1);
            org.presentation_generation = org.presentation_generation.wrapping_add(1);
            org.pending = None;
            org.refresh_queued = false;
            org.collapse = collapse::CollapseState::default();
            org.task_projection = None;
            org.focus_dirty = true;
            org.snapshot = None;
            org.project_threads_visible.clear();
            org.project_threads_total.clear();
            org.project_growth_paused.clear();
            org.editor = None;
            org.menu = None;
            org.menu_anchor = None;
            org.reveal_project = None;
            org.reorder = ReorderState::new(cx);
            self.threads.clear();
            self.archived.clear();
            self.loaded_project = None;
            self.recents_total = 0;
            self.selected_row = None;
            self.close_actions();
            cx.notify();
            if action.is_some() {
                self.refresh_organization(cx);
                return;
            }
        }
        let Some(org) = self.organization.as_mut() else {
            return;
        };
        let was_mutation = action.is_some();
        if org.pending == Some(PendingRequest::Write)
            || (org.pending.is_some() && !was_mutation)
            || org.collapse.in_flight
            || !org.collapse.intents.is_empty()
            || task_mutation_busy(cx)
        {
            org.refresh_queued = true;
            if action.is_some() {
                self.error = Some(crate::localized_copy::LocalizedCopy::new(
                    "ui.organization_is_being_saved_try_again_shortly",
                ));
                cx.notify();
            }
            return;
        }
        if action.is_some() && org.snapshot.is_none() {
            return;
        }
        let Ok(database) = database else {
            self.error = database.err();
            cx.notify();
            return;
        };
        let revision = org.snapshot.as_ref().map_or(0, |s| s.revision);
        let epoch = cx
            .try_global::<crate::navigation::TaskMutationState>()
            .map_or(0, |s| s.epoch);
        org.pending = Some(if was_mutation {
            PendingRequest::Write
        } else {
            PendingRequest::Read
        });
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
        if was_mutation {
            org.presentation_generation = org.presentation_generation.wrapping_add(1);
        }
        let request = OrganizationRequestToken {
            database: database.clone(),
            generation,
            epoch,
            origin_route,
            submitted_editor,
            was_mutation,
        };
        let worker = cx.background_executor().spawn(async move {
            let store = Store::open_existing(database)
                .map_err(crate::localized_copy::LocalizedCopy::storage)?;
            match action {
                Some(action) => service::apply(&store, revision, action),
                None => service::snapshot(&store).map(|snapshot| SidebarOrganizationOutcome {
                    snapshot,
                    created_thread: None,
                }),
            }
            .map_err(crate::localized_copy::LocalizedCopy::from)
        });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            this.update(cx, |this, cx| {
                this.finish_organization_request(request, result, cx);
            })
            .ok();
        })
        .detach();
        if was_mutation {
            cx.notify();
        }
    }
    fn finish_organization_request(
        &mut self,
        request: OrganizationRequestToken,
        result: Result<SidebarOrganizationOutcome, crate::localized_copy::LocalizedCopy>,
        cx: &mut Context<Self>,
    ) {
        let OrganizationRequestToken {
            database,
            generation,
            epoch,
            origin_route,
            submitted_editor,
            was_mutation,
        } = request;
        let Some(org) = self.organization.as_mut() else {
            return;
        };
        if generation != org.generation {
            return;
        }
        org.pending = None;
        let current_database =
            with_store(cx, |store| Ok(store.database_path().map(Path::to_path_buf)))
                .ok()
                .flatten();
        if current_database.as_ref() != Some(&database) {
            org.refresh_queued = false;
            self.refresh_organization(cx);
            return;
        }
        let current_epoch = cx
            .try_global::<crate::navigation::TaskMutationState>()
            .map_or(0, |s| s.epoch);
        let metadata_stale = epoch != current_epoch || task_mutation_busy(cx);
        let now = now_ms();
        let previous = std::mem::replace(&mut org.last_clock_ms, now);
        let clock_due = now >= org.next_time_change.get();
        if clock_due {
            org.next_time_change.set(i64::MAX);
        }
        let mut repaint = was_mutation || now < previous || clock_due;
        if !repaint
            && now.div_euclid(60_000) != previous.div_euclid(60_000)
            && org.snapshot.as_ref().is_some_and(|s| {
                s.preferences.view == SidebarView::Projects
                    && s.preferences.project_view == SidebarProjectView::Timeline
            })
        {
            repaint = service::local_calendar_bucket(previous, now).ok()
                != Some(SidebarTimelineBucket::Today);
        }
        let mut created_id = None;
        match result {
            Ok(outcome) => {
                let mut snapshot = outcome.snapshot;
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
                    let mut totals: HashMap<String, usize> = snapshot
                        .projects
                        .iter()
                        .map(|project| (project.id.clone(), 0))
                        .collect();
                    for thread in &snapshot.threads {
                        if !thread.pinned
                            && (thread.status == ThreadStatus::Active || org.archive)
                            && let Some(total) = totals.get_mut(&thread.project_id)
                        {
                            *total += 1;
                        }
                    }
                    org.collapse
                        .accept(vega_conversation::types::SidebarCollapseUpdate {
                            revision: snapshot.revision,
                            collapsed: snapshot.collapsed.clone(),
                        });
                    org.collapse.retain_targets(&snapshot);
                    snapshot.collapsed = org.collapse.effective();
                    org.sync_project_task_totals(totals);
                    org.project_growth_paused
                        .retain(|id| org.project_threads_total.contains_key(id));
                    for target in &snapshot.collapsed {
                        if let SidebarCollapseTarget::Project(id) = target {
                            org.project_threads_visible
                                .insert(id.clone(), PROJECT_TASKS_PAGE);
                            org.project_growth_paused.insert(id.clone());
                        }
                    }
                    if !metadata_stale {
                        let active = snapshot
                            .threads
                            .iter()
                            .filter(|t| t.status == ThreadStatus::Active);
                        let archived = snapshot
                            .threads
                            .iter()
                            .filter(|t| t.status == ThreadStatus::Archived);
                        if !self.threads.iter().eq(active.clone())
                            || !self.archived.iter().eq(archived.clone())
                        {
                            repaint = true;
                            self.threads = active.cloned().collect();
                            self.archived = archived.cloned().collect();
                            org.task_projection = None;
                            org.focus_dirty = true;
                        }
                    }
                    if org.snapshot.as_ref().is_none_or(|old| {
                        old.threads != snapshot.threads
                            || old.preferences != snapshot.preferences
                            || old.projects != snapshot.projects
                    }) {
                        org.task_projection = None;
                        org.focus_dirty = true;
                    }
                    if org.snapshot.as_deref() != Some(&snapshot) {
                        repaint = true;
                        org.presentation_generation = org.presentation_generation.wrapping_add(1);
                        org.snapshot = Some(std::rc::Rc::new(snapshot));
                    }
                }
                if was_mutation {
                    self.error = None;
                }
            }
            Err(error) => {
                repaint = true;
                self.error = Some(error);
                if was_mutation {
                    org.refresh_queued = true;
                }
            }
        }
        let refresh = std::mem::take(&mut org.refresh_queued) || metadata_stale;
        self.start_collapse_save(cx);
        let route_matches = origin_route
            == (
                cx.global::<SelectedProject>().0.clone(),
                cx.global::<OpenedThread>().0.as_ref().map(|t| t.id.clone()),
                cx.try_global::<SettingsOpen>().is_some_and(|s| s.0),
            );
        if let Some(id) = created_id.filter(|_| route_matches) {
            self.open_thread(&id, cx);
        } else if refresh {
            self.refresh_organization(cx);
        }
        if repaint {
            cx.notify();
        }
    }
    pub(super) fn organization_actions(
        &self,
        thread_id: &str,
    ) -> Vec<(String, SidebarOrganizationAction)> {
        self.reorder_menu_actions(&ReorderItem::Thread(thread_id.into()))
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
        let input = cx.new(|cx| {
            TextInput::new(cx, "", false).with_placeholder_key("ui.group_name_1_64_characters")
        });
        input.update(cx, |input, cx| input.set_text(&name, cx));
        window.focus(&input.read(cx).focus_handle(cx), cx);
        org.editor = Some(GroupEditor { group_id, input });
        org.menu = None;
        org.menu_anchor = None;
        self.error = None;
        cx.notify();
    }
    fn commit_group(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.organization.as_ref().and_then(|o| o.editor.as_ref()) else {
            return;
        };
        let name = editor.input.read(cx).text().trim().to_string();
        if !(1..=64).contains(&name.chars().count()) {
            self.error = Some(crate::localized_copy::LocalizedCopy::new(
                "ui.group_names_must_contain_1_64_characters",
            ));
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
        let current_database =
            with_store(cx, |store| Ok(store.database_path().map(Path::to_path_buf)))
                .ok()
                .flatten();
        let Some(org) = self.organization.as_mut() else {
            return;
        };
        if org.database != current_database {
            self.refresh_organization(cx);
            return;
        }
        if org.snapshot.is_none() {
            return;
        }
        org.collapse.toggle(target);
        self.apply_local_collapse();
        cx.notify();
        self.start_collapse_save(cx);
    }
    fn new_group_task(&mut self, group_id: String, cx: &mut Context<Self>) {
        if !crate::navigation::allow_task_navigation(None, cx) {
            return;
        }
        let Some(project_id) = cx.global::<SelectedProject>().0.clone() else {
            self.error = Some(crate::localized_copy::LocalizedCopy::new(
                "ui.add_and_select_a_project_before_creating_a",
            ));
            if let Some(org) = &self.organization {
                org.projects.update(cx, ProjectsBlock::open_picker);
            }
            cx.notify();
            return;
        };
        let defaults = cx
            .try_global::<crate::sidebar::NewTaskDefaults>()
            .cloned()
            .unwrap_or_default();
        let mut draft = vega_conversation::threads::draft_thread_from_defaults(
            Some(&project_id),
            &defaults.model,
            defaults.permission,
        );
        draft.reasoning_preferences = defaults.reasoning_preferences;
        self.submit_organization(
            SidebarOrganizationAction::CreateThreadInGroup {
                group_id,
                draft: Box::new(draft),
            },
            cx,
        );
    }
}
