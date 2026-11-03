use std::{path::PathBuf, sync::Arc};

use gpui_kit::{App, Global};
use vega_store::config;

use crate::localized_copy::LocalizedCopy;

use super::{
    ProjectsCollapsed, SessionsCollapsed, SidebarCollapsed, SidebarExplicitlyShown, SidebarWidth,
};

#[derive(Clone, Copy, Debug)]
pub(crate) enum Patch {
    SidebarCollapsed(bool),
    Width(f32),
    ProjectsCollapsed(bool),
    SessionsCollapsed(bool),
    Show,
}

#[derive(Clone, Copy, Debug, Default)]
struct Fields {
    sidebar_collapsed: Option<bool>,
    width: Option<f32>,
    projects_collapsed: Option<bool>,
    sessions_collapsed: Option<bool>,
}

impl Fields {
    fn merge(&mut self, newer: Self) {
        self.sidebar_collapsed = newer.sidebar_collapsed.or(self.sidebar_collapsed);
        self.width = newer.width.or(self.width);
        self.projects_collapsed = newer.projects_collapsed.or(self.projects_collapsed);
        self.sessions_collapsed = newer.sessions_collapsed.or(self.sessions_collapsed);
    }

    fn is_empty(self) -> bool {
        self.sidebar_collapsed.is_none()
            && self.width.is_none()
            && self.projects_collapsed.is_none()
            && self.sessions_collapsed.is_none()
    }

    fn apply(self, config: &mut config::AppConfig) {
        if let Some(value) = self.sidebar_collapsed {
            config.ui.sidebar_collapsed = value;
        }
        if let Some(value) = self.width {
            config.ui.sidebar_width = value;
        }
        if let Some(value) = self.projects_collapsed {
            config.ui.projects_collapsed = value;
        }
        if let Some(value) = self.sessions_collapsed {
            config.ui.sessions_collapsed = value;
        }
    }
}

impl From<Patch> for Fields {
    fn from(patch: Patch) -> Self {
        let mut fields = Self::default();
        match patch {
            Patch::SidebarCollapsed(value) => fields.sidebar_collapsed = Some(value),
            Patch::Width(value) => fields.width = Some(config::clamp_sidebar_width(value)),
            Patch::ProjectsCollapsed(value) => fields.projects_collapsed = Some(value),
            Patch::SessionsCollapsed(value) => fields.sessions_collapsed = Some(value),
            Patch::Show => {
                fields.sidebar_collapsed = Some(false);
                fields.projects_collapsed = Some(false);
                fields.sessions_collapsed = Some(false);
            }
        }
        fields
    }
}

#[derive(Clone)]
struct Request {
    token: Arc<()>,
    identity: Arc<()>,
    path: Option<PathBuf>,
    fields: Fields,
}

#[derive(Default)]
struct SaveState {
    identity: Arc<()>,
    path: Option<PathBuf>,
    pending: Fields,
    in_flight: Option<Request>,
    error: Option<LocalizedCopy>,
}

impl Global for SaveState {}

pub(crate) struct Status {
    pub(crate) error: Option<LocalizedCopy>,
    pub(crate) saving: bool,
    pub(crate) can_retry: bool,
}

pub(crate) fn status(cx: &App) -> Status {
    let state = cx.try_global::<SaveState>();
    let saving = state.is_some_and(|state| state.in_flight.is_some());
    Status {
        error: state.and_then(|state| state.error.clone()),
        saving,
        can_retry: state
            .is_some_and(|state| state.error.is_some() && !state.pending.is_empty() && !saving),
    }
}

pub(crate) fn initialize(path: PathBuf, cx: &mut App) {
    ensure_state(cx);
    let state = cx.global_mut::<SaveState>();
    state.identity = Arc::new(());
    state.path = Some(path);
    state.pending = Fields::default();
    state.error = None;
    cx.refresh_windows();
}

pub(crate) fn submit(patch: Patch, cx: &mut App) {
    let fields = Fields::from(patch);
    if let Some(value) = fields.sidebar_collapsed {
        cx.set_global(SidebarCollapsed(value));
        cx.set_global(SidebarExplicitlyShown(!value));
    }
    if let Some(value) = fields.width {
        cx.set_global(SidebarWidth(value));
    }
    if let Some(value) = fields.projects_collapsed {
        cx.set_global(ProjectsCollapsed(value));
    }
    if let Some(value) = fields.sessions_collapsed {
        cx.set_global(SessionsCollapsed(value));
    }
    ensure_state(cx);
    cx.global_mut::<SaveState>().pending.merge(fields);
    start_next(cx);
    cx.refresh_windows();
}

pub(crate) fn retry(cx: &mut App) {
    if status(cx).can_retry {
        start_next(cx);
        cx.refresh_windows();
    }
}

fn ensure_state(cx: &mut App) {
    if cx.try_global::<SaveState>().is_none() {
        cx.set_global(SaveState::default());
    }
}

fn take_next(state: &mut SaveState) -> Option<Request> {
    if state.in_flight.is_some() || state.pending.is_empty() {
        return None;
    }
    let request = Request {
        token: Arc::new(()),
        identity: state.identity.clone(),
        path: state.path.clone(),
        fields: std::mem::take(&mut state.pending),
    };
    state.in_flight = Some(request.clone());
    Some(request)
}

fn start_next(cx: &mut App) {
    let Some(request) = take_next(cx.global_mut::<SaveState>()) else {
        return;
    };
    let worker_request = request.clone();
    let worker = cx.background_executor().spawn(async move {
        let fields = worker_request.fields;
        match worker_request.path {
            Some(path) => config::update_from(&path, move |config| fields.apply(config)),
            None => config::update(move |config| fields.apply(config)),
        }
        .map(|_| ())
        .map_err(|_| LocalizedCopy::new("settings.config.save_failed"))
    });
    cx.spawn(async move |cx| {
        let result = worker.await;
        cx.update(|cx| {
            if cx.try_global::<SaveState>().is_none() {
                return;
            }
            let continue_saving = complete(cx.global_mut::<SaveState>(), &request, result);
            if continue_saving {
                start_next(cx);
            }
            cx.refresh_windows();
        });
    })
    .detach();
}

fn complete(state: &mut SaveState, request: &Request, result: Result<(), LocalizedCopy>) -> bool {
    if !state
        .in_flight
        .as_ref()
        .is_some_and(|current| Arc::ptr_eq(&current.token, &request.token))
    {
        return false;
    }
    state.in_flight = None;
    if !Arc::ptr_eq(&state.identity, &request.identity) {
        return true;
    }
    match result {
        Ok(()) => {
            if state.pending.is_empty() {
                state.error = None;
            }
            true
        }
        Err(error) => {
            let mut unsaved = request.fields;
            unsaved.merge(state.pending);
            state.pending = unsaved;
            state.error = Some(error);
            false
        }
    }
}

#[cfg(test)]
mod tests;
