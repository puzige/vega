use super::workspace::nsh_workspace::{
    Action, AgentIdentity, Control, Kind, Line, PanelAuthority, Snapshot,
};
use super::*;

pub(super) struct SubagentPanel {
    parent: Option<String>,
    epoch: u64,
    load: u64,
    revision: u64,
    revisions: std::collections::HashMap<String, u64>,
    agents: Vec<SubagentSnapshot>,
    loading: bool,
    error: bool,
    stop_error: Option<String>,
    selection: u64,
    history_epoch: u64,
    preview_visible: bool,
    detail: Option<SubagentDetail>,
    pub(super) focus: FocusHandle,
    pub(super) previous_maximized: Option<[bool; 2]>,
}

struct SubagentDetail {
    id: String,
    loading: bool,
    error: bool,
    model: Option<String>,
    stream: Option<Entity<ConversationStream>>,
}

#[derive(Clone)]
struct PanelReadOwner {
    parent: String,
    epoch: u64,
    selection: u64,
    child: String,
    identity: Option<AgentIdentity>,
}

impl PanelReadOwner {
    fn same_request(&self, other: &Self) -> bool {
        self.parent == other.parent
            && self.epoch == other.epoch
            && self.selection == other.selection
            && self.child == other.child
    }
}

impl SubagentDetail {
    fn accept_completion(
        &mut self,
        observed: &PanelReadOwner,
        current: Option<&PanelReadOwner>,
    ) -> bool {
        let Some(current) =
            current.filter(|current| observed.same_request(current) && self.id == current.child)
        else {
            return false;
        };
        if current.identity != observed.identity {
            if self.loading {
                self.loading = false;
                self.error = true;
            }
            return false;
        }
        true
    }
}

struct PanelHistoryRead {
    owner: PanelReadOwner,
    epoch: u64,
    stream: Entity<ConversationStream>,
    newer: bool,
}

impl SubagentPanel {
    pub(super) fn cached_stream(&self) -> Option<Entity<ConversationStream>> {
        self.detail
            .as_ref()
            .and_then(|detail| detail.stream.clone())
    }

    pub(super) fn new(cx: &mut App) -> Self {
        Self {
            parent: None,
            epoch: 0,
            load: 0,
            revision: 0,
            revisions: Default::default(),
            agents: Vec::new(),
            loading: false,
            error: false,
            stop_error: None,
            selection: 0,
            history_epoch: 0,
            preview_visible: false,
            detail: None,
            focus: cx.focus_handle(),
            previous_maximized: None,
        }
    }

    fn insert(&mut self, snapshot: SubagentSnapshot) {
        if let Some(current) = self
            .agents
            .iter_mut()
            .find(|row| row.agent_id == snapshot.agent_id)
        {
            *current = snapshot;
        } else {
            self.agents.push(snapshot);
        }
    }
}

impl VegaWindow {
    fn subagent_parent_current(&self, parent: &str, cx: &App) -> bool {
        !cx.global::<SettingsOpen>().0
            && cx
                .global::<OpenedThread>()
                .0
                .as_ref()
                .is_some_and(|thread| {
                    thread.id == parent
                        && thread.backend == TaskBackend::Native
                        && thread.parent_thread_id.is_none()
                })
            && self.subagent_panel.parent.as_deref() == Some(parent)
    }

    pub(super) fn sync_subagent_panel_route(&mut self, cx: &mut Context<Self>) {
        let parent = cx
            .global::<OpenedThread>()
            .0
            .as_ref()
            .filter(|thread| {
                !cx.global::<SettingsOpen>().0
                    && !self.is_draft_route(&thread.id)
                    && thread.backend == TaskBackend::Native
                    && thread.parent_thread_id.is_none()
            })
            .map(|thread| thread.id.clone());
        if self.subagent_panel.parent == parent {
            return;
        }
        self.clear_subagent_detail(cx);
        self.subagent_panel.epoch = self.subagent_panel.epoch.wrapping_add(1);
        self.subagent_panel.parent = parent;
        self.subagent_panel.agents.clear();
        self.subagent_panel.revisions.clear();
        self.subagent_panel.loading = false;
        self.subagent_panel.error = false;
        self.subagent_panel.stop_error = None;
        self.subagent_panel.previous_maximized = None;
        if self.subagent_panel.parent.is_some() {
            self.load_subagent_panel(cx);
        }
    }

    fn load_subagent_panel(&mut self, cx: &mut Context<Self>) {
        let Some(parent) = self.subagent_panel.parent.clone() else {
            return;
        };
        if !self.subagent_parent_current(&parent, cx) {
            return;
        }
        self.subagent_panel.load = self.subagent_panel.load.wrapping_add(1);
        let load = self.subagent_panel.load;
        let epoch = self.subagent_panel.epoch;
        let revision = self.subagent_panel.revision;
        self.subagent_panel.loading = true;
        self.subagent_panel.error = false;
        let Some(database) = self.context_database(cx) else {
            self.subagent_panel.loading = false;
            self.subagent_panel.error = false;
            cx.notify();
            return;
        };
        let source = parent.clone();
        let work = cx.background_executor().spawn(async move {
            let store = vega_store::Store::open(database).map_err(|_| ())?;
            vega_conversation::subagents::children(&store, &source).map_err(|_| ())
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.finish_subagent_panel_load(&parent, epoch, load, revision, result, cx)
            });
        })
        .detach();
    }

    pub(super) fn refresh_subagents_after_privacy(&mut self, thread: &str, cx: &mut Context<Self>) {
        if self.subagent_panel.parent.as_deref() != Some(thread)
            && !self
                .subagent_panel
                .agents
                .iter()
                .any(|agent| agent.agent_id == thread)
        {
            return;
        }
        self.clear_subagent_detail(cx);
        self.subagent_panel.epoch = self.subagent_panel.epoch.wrapping_add(1);
        self.subagent_panel.agents.clear();
        self.subagent_panel.revisions.clear();
        self.load_subagent_panel(cx);
        cx.notify();
    }

    fn finish_subagent_panel_load(
        &mut self,
        parent: &str,
        epoch: u64,
        load: u64,
        revision: u64,
        result: Result<Vec<SubagentSnapshot>, ()>,
        cx: &mut Context<Self>,
    ) {
        if !self.subagent_parent_current(parent, cx)
            || self.subagent_panel.epoch != epoch
            || self.subagent_panel.load != load
        {
            return;
        }
        self.subagent_panel.loading = false;
        match result {
            Ok(rows) => {
                self.subagent_panel.agents.retain(|row| {
                    rows.iter().any(|loaded| loaded.agent_id == row.agent_id)
                        || self
                            .subagent_panel
                            .revisions
                            .get(&row.agent_id)
                            .is_some_and(|value| *value > revision)
                });
                for row in rows {
                    if row.parent_thread_id == parent
                        && self
                            .subagent_panel
                            .revisions
                            .get(&row.agent_id)
                            .is_none_or(|value| *value <= revision)
                    {
                        self.subagent_panel.insert(row);
                    }
                }
                self.subagent_panel.error = false;
            }
            Err(()) => self.subagent_panel.error = true,
        }
        cx.notify();
    }

    pub(super) fn rebind_subagent_detail(
        &mut self,
        child: &str,
        stream: &Entity<ConversationStream>,
        cx: &mut Context<Self>,
    ) {
        let Some(detail) = self
            .subagent_panel
            .detail
            .as_mut()
            .filter(|detail| detail.id == child)
        else {
            return;
        };
        if let Some(previous) = detail.stream.replace(stream.clone())
            && previous != *stream
        {
            previous.update(cx, |previous, cx| previous.set_reading_preview(false, cx));
        }
        detail.loading = false;
        detail.error = false;
        detail.model = Some(stream.read(cx).displayed_model().to_owned());
        stream.update(cx, |stream, cx| {
            stream.set_reading_preview(self.subagent_panel.preview_visible, cx)
        });
        self.subagent_panel.history_epoch = self.subagent_panel.history_epoch.wrapping_add(1);
        self.subagent_panel.selection = self.subagent_panel.selection.wrapping_add(1);
        cx.notify();
    }

    pub(super) fn apply_subagent_panel_update(
        &mut self,
        snapshot: SubagentSnapshot,
        cx: &mut Context<Self>,
    ) {
        if !self.subagent_parent_current(&snapshot.parent_thread_id, cx) {
            return;
        }
        self.subagent_panel.revision = self.subagent_panel.revision.wrapping_add(1);
        self.subagent_panel
            .revisions
            .insert(snapshot.agent_id.clone(), self.subagent_panel.revision);
        self.subagent_panel.insert(snapshot);
        cx.notify();
    }

    pub(super) fn has_subagents(&self, cx: &App) -> bool {
        self.subagent_panel
            .parent
            .as_deref()
            .is_some_and(|parent| self.subagent_parent_current(parent, cx))
            && !self.subagent_panel.agents.is_empty()
    }

    pub(super) fn environment_available(&self, cx: &App) -> bool {
        self.shell_project_id(cx).is_some() || self.has_subagents(cx) || self.subagent_panel.error
    }

    fn subagent_display_state(&self, snapshot: &SubagentSnapshot) -> SubagentState {
        self.agent_controller
            .active
            .get(&snapshot.agent_id)
            .map_or(snapshot.state, |active| {
                if active.cancel.is_cancelled() {
                    SubagentState::Cancelling
                } else {
                    SubagentState::Running
                }
            })
    }

    fn subagent_counts(&self) -> String {
        [
            SubagentState::Running,
            SubagentState::Cancelling,
            SubagentState::Completed,
            SubagentState::Failed,
            SubagentState::Interrupted,
        ]
        .into_iter()
        .filter_map(|state| {
            let count = self
                .subagent_panel
                .agents
                .iter()
                .filter(|agent| self.subagent_display_state(agent) == state)
                .count();
            (count > 0).then(|| {
                vega_i18n::t_args(
                    "app.subagent.count",
                    &[("count", &count.to_string()), ("state", state_label(state))],
                )
            })
        })
        .collect::<Vec<_>>()
        .join(" · ")
    }

    pub(super) fn nsh_subagent_authority(&self) -> PanelAuthority {
        PanelAuthority {
            parent: self.subagent_panel.parent.clone(),
            epoch: self.subagent_panel.epoch,
            selection: self.subagent_panel.selection,
            revision: self.subagent_panel.revision,
            history_epoch: self.subagent_panel.history_epoch,
            detail: self
                .subagent_panel
                .detail
                .as_ref()
                .map(|detail| detail.id.clone()),
            agents: self
                .subagent_panel
                .agents
                .iter()
                .map(|agent| self.nsh_subagent_identity(agent))
                .collect(),
        }
    }

    fn nsh_subagent_identity(&self, agent: &SubagentSnapshot) -> AgentIdentity {
        let active = self.agent_controller.active.get(&agent.agent_id);
        AgentIdentity {
            child: agent.agent_id.clone(),
            parent: agent.parent_thread_id.clone(),
            parent_message: agent.parent_message_id.clone(),
            message: agent.message_id.clone(),
            generation: active.map(|active| active.generation),
            operation: active.map(|active| active.queue_generation.clone()),
        }
    }

    pub(super) fn nsh_subagent_overview(&self, snapshot: &mut Snapshot, cx: &App) {
        if !self.has_subagents(cx) && !self.subagent_panel.error {
            return;
        }
        snapshot.controls.push(Control::new(
            Action::Subagents,
            vega_i18n::t("app.workspace.subagents"),
            Some(vega_ui::icons::Icon::Bot),
        ));
        let counts = self.subagent_counts();
        if !counts.is_empty() {
            snapshot.lines.push(Line::new(counts, false));
        }
        if self.subagent_panel.error {
            self.nsh_subagent_load_error(snapshot);
        }
    }

    fn nsh_subagent_load_error(&self, snapshot: &mut Snapshot) {
        snapshot
            .lines
            .push(Line::new(vega_i18n::t("app.workspace.load_failed"), true));
        snapshot.controls.push(Control::new(
            Action::RetryAgents,
            vega_i18n::t("app.workspace.retry"),
            None,
        ));
    }

    pub(super) fn nsh_subagent_clear_maximized(&mut self, index: usize) {
        if self.workspace.tabs.iter().any(|(key, bottom)| {
            *key == super::workspace::TabKey::Subagents && usize::from(*bottom) == index
        }) {
            self.subagent_panel.previous_maximized = None;
        }
    }

    pub(super) fn nsh_subagent_controls(&self, snapshot: &mut Snapshot, cx: &App) {
        if let Some(child) = &self.subagent_panel.stop_error {
            snapshot
                .lines
                .push(Line::new(vega_i18n::t("app.workspace.stop_failed"), true));
            snapshot.controls.push(Control::new(
                Action::Open(child.clone()),
                vega_i18n::t("app.workspace.open_conversation"),
                None,
            ));
        }
        if let Some(detail) = &self.subagent_panel.detail {
            let agent = self
                .subagent_panel
                .agents
                .iter()
                .find(|agent| agent.agent_id == detail.id);
            snapshot.controls.push(Control::new(
                Action::Back,
                vega_i18n::t("app.workspace.back"),
                None,
            ));
            snapshot.lines.push(Line::new(
                agent
                    .map(|agent| agent.name.clone())
                    .unwrap_or_else(|| vega_i18n::t("app.workspace.subagent").into()),
                false,
            ));
            let model = detail
                .stream
                .as_ref()
                .map(|stream| stream.read(cx).displayed_model().to_string())
                .filter(|model| !model.is_empty())
                .or_else(|| detail.model.clone());
            if let Some(model) = model {
                snapshot.lines.push(Line::new(model, false));
            }
            if let Some(agent) = agent {
                snapshot.lines.push(Line::new(
                    state_label(self.subagent_display_state(agent)),
                    self.subagent_display_state(agent) == SubagentState::Failed,
                ));
            }
            snapshot.controls.push(Control::new(
                Action::Open(detail.id.clone()),
                vega_i18n::t("app.workspace.open_conversation"),
                None,
            ));
            self.nsh_subagent_stop(snapshot, &detail.id);
            if detail.loading {
                snapshot
                    .lines
                    .push(Line::new(vega_i18n::t("app.workspace.loading"), false));
            }
            if detail.error {
                snapshot
                    .lines
                    .push(Line::new(vega_i18n::t("app.workspace.open_failed"), true));
                snapshot.controls.push(Control::new(
                    Action::Detail(detail.id.clone()),
                    vega_i18n::t("app.workspace.retry"),
                    None,
                ));
            }
        } else {
            if self.subagent_panel.error {
                self.nsh_subagent_load_error(snapshot);
            }
            if self.subagent_panel.loading {
                snapshot
                    .lines
                    .push(Line::new(vega_i18n::t("app.workspace.loading"), false));
            }
            for agent in &self.subagent_panel.agents {
                snapshot.controls.push(Control::new(
                    Action::Detail(agent.agent_id.clone()),
                    format!(
                        "{} · {}",
                        agent.name,
                        state_label(self.subagent_display_state(agent))
                    ),
                    None,
                ));
                self.nsh_subagent_stop(snapshot, &agent.agent_id);
            }
        }
    }

    fn nsh_subagent_stop(&self, snapshot: &mut Snapshot, child: &str) {
        if let Some(active) = self.agent_controller.active.get(child)
            && !active.cancel.is_cancelled()
        {
            snapshot.controls.push(Control::new(
                Action::Stop(child.into(), active.generation),
                vega_i18n::t("app.workspace.stop"),
                None,
            ));
        }
    }

    pub(super) fn nsh_subagent_action(
        &mut self,
        action: Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            Action::RetryAgents => self.load_subagent_panel(cx),
            Action::Detail(child) => self.select_subagent_detail(&child, window, cx),
            Action::Back => {
                self.clear_subagent_detail(cx);
                window.focus(&self.subagent_panel.focus, cx);
            }
            Action::Open(child) => self.open_panel_child_conversation(&child, cx),
            Action::Stop(child, generation) => self.stop_panel_child(&child, generation, cx),
            _ => {}
        }
    }

    pub(super) fn open_subagent_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_subagent_panel_route(cx);
        if !self.has_subagents(cx) && !self.subagent_panel.error {
            return;
        }
        self.show_subagent_workspace(window, cx);
    }

    pub(super) fn open_subagent_preview(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &SubagentOpenRequested,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.owns_stream_request(&stream, &request.source_thread_id, cx) {
            return;
        }
        if cx
            .global::<OpenedThread>()
            .0
            .as_ref()
            .is_some_and(|thread| thread.parent_thread_id.as_deref() == Some(&request.thread_id))
        {
            self.open_subagent(stream, request, cx);
            return;
        }
        self.sync_subagent_panel_route(cx);
        if !self.subagent_parent_current(&request.source_thread_id, cx) {
            return;
        }
        self.show_subagent_workspace(window, cx);
        self.select_subagent_detail(&request.thread_id, window, cx);
    }

    pub(super) fn clear_subagent_detail(&mut self, cx: &mut Context<Self>) {
        if let Some(stream) = self
            .subagent_panel
            .detail
            .take()
            .and_then(|detail| detail.stream)
        {
            stream.update(cx, |stream, cx| stream.set_reading_preview(false, cx));
        }
        self.subagent_panel.selection = self.subagent_panel.selection.wrapping_add(1);
    }

    pub(super) fn set_subagent_preview_visible(
        &mut self,
        visible: bool,
        width: f32,
        cx: &mut Context<Self>,
    ) {
        if self.subagent_panel.preview_visible != visible {
            self.subagent_panel.history_epoch = self.subagent_panel.history_epoch.wrapping_add(1);
            self.subagent_panel.preview_visible = visible;
        }
        if let Some(stream) = self
            .subagent_panel
            .detail
            .as_ref()
            .and_then(|detail| detail.stream.as_ref())
        {
            stream.update(cx, |stream, cx| {
                stream.set_reading_preview(visible, cx);
                if visible {
                    stream.set_workspace_width(width, cx);
                }
            });
        }
    }

    fn panel_read_owner(&self, child: &str) -> Option<PanelReadOwner> {
        Some(PanelReadOwner {
            parent: self.subagent_panel.parent.clone()?,
            epoch: self.subagent_panel.epoch,
            selection: self.subagent_panel.selection,
            child: child.into(),
            identity: self
                .subagent_panel
                .agents
                .iter()
                .find(|agent| agent.agent_id == child)
                .map(|agent| self.nsh_subagent_identity(agent)),
        })
    }

    fn panel_owner_current(&self, owner: &PanelReadOwner, cx: &App) -> bool {
        self.subagent_parent_current(&owner.parent, cx)
            && self.subagent_panel.epoch == owner.epoch
            && self.subagent_panel.selection == owner.selection
            && owner.identity
                == self
                    .subagent_panel
                    .agents
                    .iter()
                    .find(|agent| agent.agent_id == owner.child)
                    .map(|agent| self.nsh_subagent_identity(agent))
            && self
                .subagent_panel
                .detail
                .as_ref()
                .is_some_and(|detail| detail.id == owner.child)
    }

    fn select_subagent_detail(&mut self, child: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.clear_subagent_detail(cx);
        window.focus(&self.subagent_panel.focus, cx);
        self.subagent_panel.detail = Some(SubagentDetail {
            id: child.into(),
            loading: true,
            error: false,
            model: None,
            stream: None,
        });
        let Some(owner) = self.panel_read_owner(child) else {
            return;
        };
        let Some(database) = self.context_database(cx) else {
            if let Some(detail) = self.subagent_panel.detail.as_mut() {
                detail.loading = false;
                detail.error = true;
            }
            cx.notify();
            return;
        };
        let retained = self.agent_controller.active_stream_for_thread(child);
        let read_owner = owner.clone();
        let has_stream = retained.is_some();
        let work = cx.background_executor().spawn(async move {
            let store = vega_store::Store::open(database).map_err(|_| ())?;
            let thread = vega_conversation::subagents::related_thread(
                &store,
                &read_owner.parent,
                &read_owner.child,
            )
            .map_err(|_| ())?;
            if thread.parent_thread_id.as_deref() != Some(read_owner.parent.as_str())
                || thread.backend != TaskBackend::Native
            {
                return Err(());
            }
            let page = if has_stream {
                None
            } else {
                Some(
                    vega_conversation::history::latest_history_page(
                        &store,
                        &thread.id,
                        vega_store::messages::PAGE_LIMIT,
                    )
                    .map_err(|_| ())?,
                )
            };
            Ok((thread, page))
        });
        let window = window.window_handle();
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = window.update(cx, |_, window, cx| {
                this.update(cx, |this, cx| {
                    this.finish_subagent_detail(owner, retained, result, window, cx)
                })
            });
        })
        .detach();
        cx.notify();
    }

    fn finish_subagent_detail(
        &mut self,
        owner: PanelReadOwner,
        retained: Option<Entity<ConversationStream>>,
        result: Result<(Thread, Option<vega_conversation::history::HistoryPage>), ()>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = self
            .subagent_parent_current(&owner.parent, cx)
            .then(|| self.panel_read_owner(&owner.child))
            .flatten();
        if !self
            .subagent_panel
            .detail
            .as_mut()
            .is_some_and(|detail| detail.accept_completion(&owner, current.as_ref()))
        {
            cx.notify();
            return;
        }
        let Ok((thread, page)) = result else {
            if let Some(detail) = self.subagent_panel.detail.as_mut() {
                detail.loading = false;
                detail.error = true;
            }
            cx.notify();
            return;
        };
        let cached = self
            .agent_controller
            .active_stream_for_thread(&owner.child)
            .or(retained);
        let stream = if let Some(stream) = cached {
            self.bind_subagent_detail_stream(&owner.child, &stream, window, cx);
            stream
        } else {
            let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
            if let Some(page) = page {
                stream.update(cx, |stream, cx| stream.apply_history_page(page, cx));
            }
            cx.subscribe(
                &stream,
                |this, stream, request: &HistoryPageRequested, cx| {
                    this.request_history_page(stream.clone(), request, cx);
                },
            )
            .detach();
            cx.subscribe(
                &stream,
                |this, stream, request: &NewerHistoryPageRequested, cx| {
                    this.request_newer_history_page(stream.clone(), request, cx);
                },
            )
            .detach();
            cx.subscribe(&stream, |this, stream, _: &ImagePreviewRequested, cx| {
                this.open_panel_stream_conversation(&stream, cx);
            })
            .detach();
            stream
        };
        let model = stream.read(cx).displayed_model().to_string();
        if let Some(detail) = self.subagent_panel.detail.as_mut() {
            detail.loading = false;
            detail.error = false;
            detail.model = (!model.is_empty()).then_some(model);
            detail.stream = Some(stream);
        }
        cx.notify();
    }

    pub(super) fn owns_subagent_read(
        &self,
        stream: &Entity<ConversationStream>,
        child: &str,
        cx: &App,
    ) -> bool {
        self.subagent_panel.preview_visible
            && self
                .panel_read_owner(child)
                .is_some_and(|owner| self.panel_owner_current(&owner, cx))
            && self
                .subagent_panel
                .detail
                .as_ref()
                .and_then(|detail| detail.stream.as_ref())
                == Some(stream)
    }

    pub(super) fn request_subagent_history(
        &mut self,
        stream: Entity<ConversationStream>,
        child: &str,
        cursor: i64,
        newer: bool,
        cx: &mut Context<Self>,
    ) {
        if !self.owns_subagent_read(&stream, child, cx) {
            return;
        }
        let Some(owner) = self.panel_read_owner(child) else {
            return;
        };
        let history_epoch = self.subagent_panel.history_epoch;
        let Some(database) = self.context_database(cx) else {
            return;
        };
        let source = owner.clone();
        let work = cx.background_executor().spawn(async move {
            let store = vega_store::Store::open(database).map_err(|_| ())?;
            let thread =
                vega_conversation::subagents::related_thread(&store, &source.parent, &source.child)
                    .map_err(|_| ())?;
            if thread.parent_thread_id.as_deref() != Some(source.parent.as_str()) {
                return Err(());
            }
            if newer {
                vega_conversation::history::history_page_after(
                    &store,
                    &source.child,
                    cursor,
                    vega_store::messages::PAGE_LIMIT,
                )
            } else {
                vega_conversation::history::history_page_before(
                    &store,
                    &source.child,
                    vega_store::messages::PageCursor::Before(cursor),
                    vega_store::messages::PAGE_LIMIT,
                )
            }
            .map_err(|_| ())
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.finish_subagent_history(
                    PanelHistoryRead {
                        owner,
                        epoch: history_epoch,
                        stream,
                        newer,
                    },
                    result,
                    cx,
                );
            });
        })
        .detach();
    }

    fn finish_subagent_history(
        &mut self,
        read: PanelHistoryRead,
        result: Result<vega_conversation::history::HistoryPage, ()>,
        cx: &mut Context<Self>,
    ) {
        if self.subagent_panel.history_epoch != read.epoch
            || !self.panel_owner_current(&read.owner, cx)
            || !self.owns_subagent_read(&read.stream, &read.owner.child, cx)
        {
            return;
        }
        if result.is_err()
            && let Some(detail) = self.subagent_panel.detail.as_mut()
        {
            detail.error = true;
        }
        read.stream.update(cx, |stream, cx| match result {
            Ok(page) if read.newer => stream.apply_newer_history_page(page, cx),
            Ok(page) => stream.apply_history_page(page, cx),
            Err(()) if read.newer => stream.apply_newer_history_load_failed(cx),
            Err(()) => stream.apply_history_load_failed(cx),
        });
        cx.notify();
    }

    pub(super) fn open_panel_stream_conversation(
        &mut self,
        stream: &Entity<ConversationStream>,
        cx: &mut Context<Self>,
    ) {
        let Some(child) = self
            .subagent_panel
            .detail
            .as_ref()
            .filter(|detail| detail.stream.as_ref() == Some(stream))
            .map(|detail| detail.id.clone())
        else {
            return;
        };
        self.open_panel_child_conversation(&child, cx);
    }

    pub(super) fn panel_image_requested(
        &mut self,
        stream: &Entity<ConversationStream>,
        cx: &mut Context<Self>,
    ) -> bool {
        if self
            .subagent_panel
            .detail
            .as_ref()
            .is_none_or(|detail| !self.owns_subagent_read(stream, &detail.id, cx))
        {
            return false;
        }
        self.open_panel_stream_conversation(stream, cx);
        true
    }

    fn open_panel_child_conversation(&mut self, child: &str, cx: &mut Context<Self>) {
        let Some(parent) = self.subagent_panel.parent.clone() else {
            return;
        };
        if !self.subagent_parent_current(&parent, cx) {
            return;
        }
        let Some((source, stream)) = self
            .stream_view
            .clone()
            .filter(|(source, _)| *source == parent)
        else {
            return;
        };
        let Some(database) = self.context_database(cx) else {
            return;
        };
        let epoch = self.subagent_panel.epoch;
        let selection = self.subagent_panel.selection;
        let target = child.to_string();
        let identity = self
            .subagent_panel
            .agents
            .iter()
            .find(|agent| agent.agent_id == child)
            .map(|agent| self.nsh_subagent_identity(agent));
        let identity_child = child.to_string();
        let history_epoch = self.subagent_panel.history_epoch;
        let worker_source = source.clone();
        let work = cx.background_executor().spawn(async move {
            let store = vega_store::Store::open(database).map_err(|_| ())?;
            let thread =
                vega_conversation::subagents::related_thread(&store, &worker_source, &target)
                    .map_err(|_| ())?;
            if thread.parent_thread_id.as_deref() != Some(worker_source.as_str()) {
                return Err(());
            }
            Ok(thread)
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                if !this.owns_stream_request(&stream, &source, cx)
                    || !this.subagent_parent_current(&source, cx)
                    || this.subagent_panel.epoch != epoch
                    || this.subagent_panel.selection != selection
                    || this.subagent_panel.history_epoch != history_epoch
                    || !this.subagent_panel.preview_visible
                    || identity
                        != this
                            .subagent_panel
                            .agents
                            .iter()
                            .find(|agent| agent.agent_id == identity_child)
                            .map(|agent| this.nsh_subagent_identity(agent))
                {
                    return;
                }
                match result {
                    Ok(thread) => {
                        if this.accept_palette_thread(thread, cx) {
                            this.clear_subagent_detail(cx);
                        }
                    }
                    Err(()) => {
                        if let Some(detail) = this.subagent_panel.detail.as_mut() {
                            detail.error = true;
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn stop_panel_child(&mut self, child: &str, generation: u64, cx: &mut Context<Self>) {
        let Some(parent) = self.subagent_panel.parent.clone() else {
            return;
        };
        if !self.subagent_parent_current(&parent, cx)
            || !self
                .subagent_panel
                .agents
                .iter()
                .any(|agent| agent.agent_id == child)
        {
            return;
        }
        let epoch = self.subagent_panel.epoch;
        let history_epoch = self.subagent_panel.history_epoch;
        let selection = self.subagent_panel.selection;
        let identity = self
            .subagent_panel
            .agents
            .iter()
            .find(|agent| agent.agent_id == child)
            .map(|agent| self.nsh_subagent_identity(agent));
        let Some(database) = self.context_database(cx) else {
            return;
        };
        let target = child.to_string();
        let source = parent.clone();
        let read_target = target.clone();
        let work = cx.background_executor().spawn(async move {
            let store = vega_store::Store::open(database).map_err(|_| ())?;
            let thread =
                vega_conversation::subagents::related_thread(&store, &source, &read_target)
                    .map_err(|_| ())?;
            if thread.parent_thread_id.as_deref() == Some(source.as_str()) {
                Ok(())
            } else {
                Err(())
            }
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                if !this.subagent_parent_current(&parent, cx)
                    || this.subagent_panel.epoch != epoch
                    || this.subagent_panel.history_epoch != history_epoch
                    || this.subagent_panel.selection != selection
                    || !this.subagent_panel.preview_visible
                    || identity
                        != this
                            .subagent_panel
                            .agents
                            .iter()
                            .find(|agent| agent.agent_id == target)
                            .map(|agent| this.nsh_subagent_identity(agent))
                {
                    return;
                }
                if result.is_err() {
                    this.subagent_panel.stop_error = Some(target.clone());
                } else if this
                    .agent_controller
                    .active
                    .get(&target)
                    .is_some_and(|active| active.generation == generation)
                {
                    this.subagent_panel.stop_error = None;
                    this.pause_automation_all(&target, "stopped", cx);
                    this.agent_controller.request_active_cancel(&target);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn bind_subagent_detail_stream(
        &mut self,
        child: &str,
        stream: &Entity<ConversationStream>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.agent_controller.delegated_unbound.remove(child) {
            return;
        }
        let models = self.model_options_for_pricing();
        let model = stream.read(cx).displayed_model().to_owned();
        let label = self
            .sidebar
            .read(cx)
            .project_label(stream.read(cx).route_project_id(), cx)
            .unwrap_or_default();
        stream.update(cx, |stream, cx| {
            stream.apply_model_options(models, cx);
            stream.set_project_label(label, cx);
        });
        self.apply_reasoning_profile_to_stream(stream, &model, cx);
        self.bind_conversation_stream(stream, window, cx);
    }

    pub(super) fn render_subagent_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if let Some((child, stream)) = self.subagent_panel.detail.as_ref().and_then(|detail| {
            detail
                .stream
                .clone()
                .map(|stream| (detail.id.clone(), stream))
        }) {
            self.bind_subagent_detail_stream(&child, &stream, window, cx);
        }
        let stream = self
            .subagent_panel
            .detail
            .as_ref()
            .and_then(|detail| detail.stream.clone());
        let chrome = self.render_nsh_workspace(Kind::Subagents, window, cx);
        div()
            .id("subagent-panel")
            .track_focus(&self.subagent_panel.focus)
            .size_full()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(super::workspace::nsh_workspace::detail_body(
                chrome,
                stream.map(|stream| stream.into_any_element()),
            ))
            .into_any_element()
    }
}

#[cfg(test)]
#[path = "nsh_workspace/detail_tests.rs"]
mod detail_tests;

fn state_label(state: SubagentState) -> &'static str {
    match state {
        SubagentState::Running => vega_i18n::t("app.subagent.running"),
        SubagentState::Cancelling => vega_i18n::t("app.subagent.stopping"),
        SubagentState::Completed => vega_i18n::t("app.subagent.completed"),
        SubagentState::Failed => vega_i18n::t("app.subagent.failed"),
        SubagentState::Interrupted => vega_i18n::t("app.subagent.interrupted"),
    }
}
