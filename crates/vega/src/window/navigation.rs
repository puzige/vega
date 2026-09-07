//! Window-local route history and text-only draft ownership.
use super::*;
use std::{collections::HashMap, sync::mpsc, time::Duration};
use vega_ui::navigation::{
    EditorNavigateBack, EditorNavigateForward, NavigateBack, NavigateForward, NavigationState,
};
fn task_mutation_state(cx: &App) -> vega_ui::navigation::TaskMutationState {
    cx.try_global::<vega_ui::navigation::TaskMutationState>()
        .copied()
        .unwrap_or_default()
}
const HISTORY_LIMIT: usize = 100;
const DRAFT_BYTES: usize = 1024 * 1024;

#[derive(Clone)]
struct Projection {
    project: Option<String>,
    thread: Option<Thread>,
    settings: bool,
}
impl Projection {
    fn read(cx: &App) -> Self {
        Self {
            project: cx.global::<SelectedProject>().0.clone(),
            thread: cx.global::<OpenedThread>().0.clone(),
            settings: cx.global::<SettingsOpen>().0,
        }
    }
    fn route(&self) -> NavigationRoute {
        if self.settings {
            NavigationRoute::Settings
        } else if let Some(thread) = &self.thread {
            NavigationRoute::Task {
                project: thread.project_id.clone(),
                task: thread.id.clone(),
            }
        } else {
            NavigationRoute::Project(self.project.clone())
        }
    }
    fn apply(self, cx: &mut App) {
        cx.set_global(SelectedProject(self.project));
        cx.set_global(OpenedThread(self.thread));
        cx.set_global(SettingsOpen(self.settings));
    }
}

pub(super) struct Navigation {
    entries: Vec<NavigationRoute>,
    cursor: usize,
    current: Projection,
    generation: u64,
    pending: bool,
    drafts: HashMap<String, String>,
    error: Option<&'static str>,
}
impl Navigation {
    pub(super) fn new(cx: &App) -> Self {
        let current = Projection::read(cx);
        Self {
            entries: vec![current.route()],
            cursor: 0,
            current,
            generation: 0,
            pending: false,
            drafts: HashMap::new(),
            error: None,
        }
    }
}

/// Register deferred handlers: GPUI global action dispatch must not reenter a window.
pub(crate) fn bind_shortcuts(window: AnyWindowHandle, root: WeakEntity<VegaWindow>, cx: &mut App) {
    let editor_back = root.clone();
    cx.on_action(move |_: &EditorNavigateBack, cx| {
        let root = editor_back.clone();
        cx.defer(move |cx| {
            let _ = window.update(cx, |_, window, cx| {
                let _ = root.update(cx, |root, cx| root.navigate_from_editor(false, window, cx));
            });
        });
    });
    let editor_forward = root.clone();
    cx.on_action(move |_: &EditorNavigateForward, cx| {
        let root = editor_forward.clone();
        cx.defer(move |cx| {
            let _ = window.update(cx, |_, window, cx| {
                let _ = root.update(cx, |root, cx| root.navigate_from_editor(true, window, cx));
            });
        });
    });
    let close = root.clone();
    cx.on_action(move |_: &CloseSettings, cx| {
        let root = close.clone();
        cx.defer(move |cx| {
            let _ = window.update(cx, |_, _, cx| {
                let _ = root.update(cx, |root, cx| {
                    if cx.global::<PendingDeleteConfirm>().0.is_some() {
                        cx.set_global(PendingDeleteConfirm(None));
                    } else if cx.global::<SettingsOpen>().0 {
                        root.sync_navigation(cx);
                        if root.navigation.cursor > 0 {
                            root.navigate(false, cx);
                        } else {
                            cx.set_global(SettingsOpen(false));
                        }
                    }
                    cx.refresh_windows();
                });
            });
        });
    });
    let back = root.clone();
    cx.on_action(move |_: &NavigateBack, cx| {
        let root = back.clone();
        cx.defer(move |cx| {
            let _ = window.update(cx, |_, _, cx| {
                let _ = root.update(cx, |root, cx| root.navigate(false, cx));
            });
        });
    });
    cx.on_action(move |_: &NavigateForward, cx| {
        let root = root.clone();
        cx.defer(move |cx| {
            let _ = window.update(cx, |_, _, cx| {
                let _ = root.update(cx, |root, cx| root.navigate(true, cx));
            });
        });
    });
}

impl VegaWindow {
    /// Final palette acceptance shares the same draft reservation and visit ordering as history.
    pub(crate) fn accept_palette_thread(&mut self, thread: Thread, cx: &mut Context<Self>) -> bool {
        let destination = Projection {
            project: Some(thread.project_id.clone()),
            thread: Some(thread.clone()),
            settings: false,
        };
        if !self.retain_departing_draft(&destination, cx) {
            self.navigation.error = Some("草稿已达容量上限，请先发送或清空当前草稿再导航。");
            self.publish_navigation(cx);
            return false;
        }
        destination.apply(cx);
        self.sync_navigation(cx);
        self.record_navigation_visit(thread, cx);
        true
    }

    // The short shared write guard blocks later task-menu writes, never typing.
    fn record_navigation_visit(&mut self, thread: Thread, cx: &mut Context<Self>) {
        let Some(path) = cx
            .global::<VegaStore>()
            .0
            .as_ref()
            .ok()
            .and_then(|store| store.database_path())
            .map(PathBuf::from)
        else {
            return;
        };
        vega_ui::navigation::begin_task_mutation(cx);
        let mutation = task_mutation_state(cx);
        let task = thread.id.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        let _ = std::thread::Builder::new()
            .name("vega-navigation-visit".into())
            .spawn(move || {
                let result = vega_store::Store::open(path)
                    .map_err(|_| NavigationError::Unavailable)
                    .and_then(|store| {
                        vega_conversation::threads::visit_thread(&store, &task)
                            .map_err(|_| NavigationError::Unavailable)
                    });
                let _ = sender.send(result);
            });
        cx.spawn(async move |owner, cx| {
            let result = loop {
                match receiver.try_recv() {
                    Ok(result) => break result,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        break Err(NavigationError::Unavailable);
                    }
                    Err(mpsc::TryRecvError::Empty) => {
                        cx.background_executor()
                            .timer(Duration::from_millis(25))
                            .await
                    }
                }
            };
            cx.update(|cx| {
                let unchanged = task_mutation_state(cx) == mutation;
                vega_ui::navigation::finish_task_mutation(cx);
                let _ = owner.update(cx, |this, cx| {
                    if !unchanged {
                        return;
                    }
                    match result {
                        Ok(visited) => {
                            if let Some(mut current) = cx.global::<OpenedThread>().0.clone()
                                && current.id == thread.id
                                && current.project_id == thread.project_id
                            {
                                current.unread = visited.unread;
                                current.updated_at = visited.updated_at;
                                cx.set_global(OpenedThread(Some(current)));
                            }
                        }
                        Err(_) => this.navigation.error = Some("页面已打开，但访问状态保存失败。"),
                    }
                    this.publish_navigation(cx);
                    cx.refresh_windows();
                    cx.notify();
                });
            });
        })
        .detach();
    }

    fn navigate_from_editor(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let allowed = !cx.global::<SettingsOpen>().0
            && self.stream_view.as_ref().is_some_and(|(_, stream)| {
                let input = stream.read(cx).composer_input();
                input.read(cx).focus_handle(cx).is_focused(window) && !input.read(cx).is_composing()
            });
        if allowed {
            self.navigate(forward, cx);
        }
    }

    fn publish_navigation(&self, cx: &mut Context<Self>) {
        if let Some((id, stream)) = &self.stream_view {
            let guard = vega_ui::navigation::DraftNavigationGuard {
                task: id.clone(),
                input: stream.read(cx).composer_input().downgrade(),
                cached_tasks: self
                    .navigation
                    .drafts
                    .keys()
                    .filter(|key| *key != id)
                    .count(),
                cached_bytes: self
                    .navigation
                    .drafts
                    .iter()
                    .filter(|(key, _)| *key != id)
                    .map(|(_, text)| text.len())
                    .sum(),
            };
            if cx.try_global::<vega_ui::navigation::DraftNavigationGuard>() != Some(&guard) {
                cx.set_global(guard);
            }
        } else if cx
            .try_global::<vega_ui::navigation::DraftNavigationGuard>()
            .is_some()
        {
            cx.remove_global::<vega_ui::navigation::DraftNavigationGuard>();
        }
        let state = NavigationState {
            back: !self.navigation.pending
                && task_mutation_state(cx).pending == 0
                && self.navigation.cursor > 0,
            forward: !self.navigation.pending
                && task_mutation_state(cx).pending == 0
                && self.navigation.cursor + 1 < self.navigation.entries.len(),
            error: self.navigation.error.or_else(|| {
                cx.try_global::<NavigationState>()
                    .and_then(|state| state.error)
            }),
        };
        if cx.try_global::<NavigationState>() != Some(&state) {
            cx.set_global(state);
        }
    }

    // Capacity checks precede teardown. The active editor is never evicted.
    fn retain_departing_draft(&mut self, destination: &Projection, cx: &App) -> bool {
        let Some((id, stream)) = &self.stream_view else {
            return true;
        };
        if destination
            .thread
            .as_ref()
            .is_some_and(|thread| &thread.id == id)
        {
            return true;
        }
        let input = stream.read(cx).composer_input();
        let text = input.read(cx).text();
        let bytes: usize = self
            .navigation
            .drafts
            .iter()
            .filter(|(key, _)| *key != id)
            .map(|(_, text)| text.len())
            .sum();
        let count =
            self.navigation.drafts.len() - usize::from(self.navigation.drafts.contains_key(id));
        if !text.is_empty()
            && (count >= HISTORY_LIMIT || bytes.saturating_add(text.len()) > DRAFT_BYTES)
        {
            return false;
        }
        if text.is_empty() {
            self.navigation.drafts.remove(id);
        } else {
            self.navigation.drafts.insert(id.clone(), text.to_string());
        }
        true
    }

    /// Coalesce all route globals before render tears down the departing editor.
    pub(super) fn sync_navigation(&mut self, cx: &mut Context<Self>) {
        let projection = Projection::read(cx);
        let route = projection.route();
        if route == self.navigation.current.route() {
            self.navigation.current = projection;
            self.publish_navigation(cx);
            return;
        }
        self.navigation.generation = self.navigation.generation.wrapping_add(1);
        if !self.retain_departing_draft(&projection, cx) {
            self.navigation.error = Some("草稿已达容量上限，请先发送或清空当前草稿再导航。");
            self.navigation.current.clone().apply(cx);
            self.publish_navigation(cx);
            return;
        }
        self.navigation.entries.truncate(self.navigation.cursor + 1);
        self.navigation.entries.push(route);
        if self.navigation.entries.len() > HISTORY_LIMIT {
            self.navigation.entries.remove(0);
        }
        self.navigation.cursor = self.navigation.entries.len() - 1;
        self.navigation.current = projection;
        self.navigation.error = None;
        cx.set_global(NavigationState::default());
        self.publish_navigation(cx);
    }

    pub(super) fn restore_navigation_draft(
        &mut self,
        id: &str,
        stream: &Entity<ConversationStream>,
        cx: &mut Context<Self>,
    ) {
        if let Some(text) = self.navigation.drafts.remove(id) {
            let input = stream.read(cx).composer_input();
            input.update(cx, |input, cx| input.set_text(&text, cx));
        }
    }

    pub(crate) fn navigate(&mut self, forward: bool, cx: &mut Context<Self>) {
        self.sync_navigation(cx);
        if self.navigation.pending {
            return;
        }
        let mutation = task_mutation_state(cx);
        if mutation.pending > 0 {
            self.navigation.error = Some("任务操作尚未完成，请稍后重试导航。");
            self.publish_navigation(cx);
            return;
        }
        let cursor = self.navigation.cursor;
        let candidates: Vec<_> = if forward {
            ((cursor + 1)..self.navigation.entries.len()).collect()
        } else {
            (0..cursor).rev().collect()
        };
        if candidates.is_empty() {
            return;
        }
        // Refuse before any task visit can clear unread or touch timestamps.
        // Settings retains this editor, so even an oversized live draft may enter/leave it.
        let target_task = match &self.navigation.entries[candidates[0]] {
            NavigationRoute::Task { task, .. } => Some(task.clone()),
            NavigationRoute::Settings => self.stream_view.as_ref().map(|(id, _)| id.clone()),
            NavigationRoute::Project(_) => None,
        };
        if !vega_ui::navigation::allow_task_navigation(target_task.as_deref(), cx) {
            return;
        }
        let path = cx
            .global::<VegaStore>()
            .0
            .as_ref()
            .ok()
            .and_then(|s| s.database_path())
            .map(PathBuf::from);
        self.navigation.generation = self.navigation.generation.wrapping_add(1);
        let generation = self.navigation.generation;
        let origin = self.navigation.current.route();
        let entries = self.navigation.entries.clone();
        self.navigation.pending = true;
        self.navigation.error = None;
        cx.set_global(NavigationState::default());
        self.publish_navigation(cx);
        let (sender, receiver) = mpsc::sync_channel(1);
        let _ = std::thread::Builder::new()
            .name("vega-navigation".into())
            .spawn(move || {
                let service = path.map(vega_conversation::navigation::NavigationService::new);
                let mut invalid = Vec::new();
                let mut result = Ok(None);
                for index in candidates {
                    let visited = match &entries[index] {
                        NavigationRoute::Settings | NavigationRoute::Project(None) => Ok(None),
                        route => service
                            .as_ref()
                            .ok_or(NavigationError::Unavailable)
                            .and_then(|service| service.resolve(route)),
                    };
                    match visited {
                        Ok(thread) => {
                            result = Ok(Some((index, entries[index].clone(), thread)));
                            break;
                        }
                        Err(NavigationError::InvalidRoute) => invalid.push(index),
                        Err(error) => {
                            result = Err(error);
                            break;
                        }
                    }
                }
                let _ = sender.send((invalid, result));
            });
        cx.spawn(async move |owner, cx| {
            let (invalid, result) = loop {
                match receiver.try_recv() {
                    Ok(result) => break result,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        break (Vec::new(), Err(NavigationError::Unavailable));
                    }
                    Err(mpsc::TryRecvError::Empty) => {
                        cx.background_executor()
                            .timer(Duration::from_millis(25))
                            .await
                    }
                }
            };
            let _ = owner.update(cx, |this, cx| {
                this.navigation.pending = false;
                if this.navigation.generation != generation
                    || Projection::read(cx).route() != origin
                {
                    this.publish_navigation(cx);
                    cx.notify();
                    return;
                }
                if task_mutation_state(cx) != mutation {
                    this.navigation.error = Some("任务已更新，请重试导航。");
                    this.publish_navigation(cx);
                    cx.notify();
                    return;
                }
                match result {
                    Err(_) => this.navigation.error = Some("导航失败，请重试；当前页面已保留。"),
                    Ok(target) => {
                        let mut destination = this.navigation.current.clone();
                        let target_index = target.as_ref().map(|(index, _, _)| *index);
                        if let Some((_, route, thread)) = target {
                            match route {
                                NavigationRoute::Settings => destination.settings = true,
                                NavigationRoute::Project(project) => {
                                    destination.project = project;
                                    destination.thread = None;
                                    destination.settings = false;
                                }
                                NavigationRoute::Task { project, .. } => {
                                    destination.project = Some(project);
                                    destination.thread = thread;
                                    destination.settings = false;
                                }
                            }
                            if !this.retain_departing_draft(&destination, cx) {
                                this.navigation.error =
                                    Some("草稿已达容量上限，请先发送或清空当前草稿再导航。");
                                this.publish_navigation(cx);
                                cx.notify();
                                return;
                            }
                        }
                        let selected = target_index.unwrap_or(this.navigation.cursor);
                        this.navigation.cursor =
                            selected - invalid.iter().filter(|i| **i < selected).count();
                        let mut index = 0;
                        this.navigation.entries.retain(|_| {
                            let keep = !invalid.contains(&index);
                            index += 1;
                            keep
                        });
                        if target_index.is_some() {
                            cx.set_global(NavigationState::default());
                            let visited = (!destination.settings)
                                .then(|| destination.thread.clone())
                                .flatten();
                            this.navigation.current = destination.clone();
                            destination.apply(cx);
                            if let Some(thread) = visited {
                                this.record_navigation_visit(thread, cx);
                            }
                        } else {
                            this.navigation.error =
                                Some("没有可返回的有效页面，已跳过移除或归档的任务。");
                        }
                    }
                }
                this.publish_navigation(cx);
                cx.refresh_windows();
                cx.notify();
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests;
