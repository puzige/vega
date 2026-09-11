//! App-owned command routing and bounded background search/preview ownership.
use crate::window::VegaWindow;
use gpui_kit::*;
use std::{sync::mpsc, time::Duration};
use tokio_util::sync::CancellationToken;
use vega_conversation::{palette::PaletteService, types::*};
use vega_ui::{command_palette::*, file_preview::*, settings::SettingsOpen, sidebar::*};

type Route = (Option<String>, Option<String>);
/// Palette lifetime and worker fences, retained above the transient UI entity.
#[derive(Default)]
pub(crate) struct AppPalette {
    pub(crate) view: Option<Entity<CommandPalette>>,
    prior_focus: Option<FocusHandle>,
    route: Option<Route>,
    generation: u64,
    cancel: CancellationToken,
    pub(crate) search_busy: bool,
    pending_query: Option<String>,
    target: Option<PaletteTarget>,
    activation: Option<u64>,
    close_requested: bool,
    preview: Option<(Route, String, PaletteFilePreview)>,
}
fn route(cx: &App) -> Route {
    (
        cx.global::<SelectedProject>().0.clone(),
        cx.global::<OpenedThread>()
            .0
            .as_ref()
            .map(|thread| thread.id.clone()),
    )
}
fn service(cx: &App) -> Option<PaletteService> {
    cx.global::<VegaStore>()
        .0
        .as_ref()
        .ok()
        .and_then(|store| store.database_path())
        .map(|path| PaletteService::new(path.to_path_buf()))
}

/// Install focus-independent application shortcuts for the current main window.
/// This also covers the short interval while task navigation replaces focus nodes.
pub(crate) fn bind_shortcuts(window: AnyWindowHandle, root: WeakEntity<VegaWindow>, cx: &mut App) {
    crate::window::navigation::bind_shortcuts(window, root.clone(), cx);
    cx.bind_keys([KeyBinding::new("cmd-n", NewThread, None)]);
    let new_root = root.clone();
    cx.on_action(move |_: &NewThread, cx| {
        let root = new_root.clone();
        cx.defer(move |cx| {
            let _ = window.update(cx, |_, window, cx| {
                let _ = root.update(cx, |root, cx| root.open_new_thread(window, cx));
            });
        });
    });
    let search_root = root.clone();
    cx.on_action(move |_: &OpenPalette, cx| {
        let root = search_root.clone();
        cx.defer(move |cx| {
            let _ = window.update(cx, |_, window, cx| {
                let _ = root.update(cx, |root, cx| root.open_palette(&OpenPalette, window, cx));
            });
        });
    });
    let picker_root = root.clone();
    cx.on_action(move |_: &OpenWorkspacePicker, cx| {
        let root = picker_root.clone();
        cx.defer(move |cx| {
            let _ = window.update(cx, |_, window, cx| {
                let _ = root.update(cx, |root, cx| {
                    root.palette_open_workspace(&OpenWorkspacePicker, window, cx)
                });
            });
        });
    });
    cx.on_action(move |_: &ToggleWorkspaceTerminal, cx| {
        let root = root.clone();
        cx.defer(move |cx| {
            let _ = window.update(cx, |_, window, cx| {
                let _ = root.update(cx, |root, cx| {
                    root.palette_toggle_terminal(&ToggleWorkspaceTerminal, window, cx)
                });
            });
        });
    });
}

impl VegaWindow {
    pub(crate) fn palette_toggle_terminal(
        &mut self,
        _: &ToggleWorkspaceTerminal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.palette.view.is_some() {
            self.close_palette(window, cx);
        }
        cx.set_global(SettingsOpen(false));
        // R45: ⌘J routes through the unified bottom-dock toggle so the
        // shortcut and the shell slot share one deterministic priority.
        self.workspace_toggle_bottom(window, cx);
        cx.refresh_windows();
    }

    pub(crate) fn palette_open_workspace(
        &mut self,
        _: &OpenWorkspacePicker,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.palette.view.is_some() {
            self.close_palette(window, cx);
        }
        self.sidebar
            .update(cx, |sidebar, cx| sidebar.open_project_picker(cx));
        cx.refresh_windows();
    }

    pub(crate) fn open_palette(
        &mut self,
        _: &OpenPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.palette.view.is_some()
            && (self.palette.close_requested || self.palette.route.as_ref() != Some(&route(cx)))
        {
            self.close_palette(window, cx);
        }
        if let Some(view) = &self.palette.view {
            window.focus(&view.read(cx).focus_handle(cx), cx);
            return;
        }
        self.palette.generation = self.palette.generation.wrapping_add(1);
        self.palette.route = Some(route(cx));
        self.palette.prior_focus = window.focused(cx);
        let mut actions = vec![
            PaletteAction::OpenWorkspace,
            PaletteAction::Settings,
            PaletteAction::ToggleSidebar,
        ];
        if cx.global::<SelectedProject>().0.is_some() {
            actions.insert(0, PaletteAction::NewTask);
            actions.push(PaletteAction::Terminal);
        }
        if !cx.global::<SettingsOpen>().0 {
            if self.stream_view.is_some() {
                actions.push(PaletteAction::Review);
            }
            if self.workspace_has_preview() {
                actions.push(PaletteAction::Preview);
            }
        }
        let view = cx.new(|cx| CommandPalette::new(actions, cx));
        cx.subscribe(&view, |this, _, event: &PaletteQueryChanged, cx| {
            this.search_palette(event.0.clone(), cx)
        })
        .detach();
        cx.subscribe(&view, |this, _, event: &PaletteActivated, cx| {
            this.palette.target = Some(event.0.clone());
            cx.notify();
        })
        .detach();
        cx.subscribe(&view, |this, _, _: &PaletteClosed, cx| {
            this.palette.close_requested = true;
            cx.notify();
        })
        .detach();
        window.focus(&view.read(cx).focus_handle(cx), cx);
        self.palette.view = Some(view);
        self.search_palette(String::new(), cx);
        cx.notify();
    }
    fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.palette.cancel.cancel();
        self.palette.generation = self.palette.generation.wrapping_add(1);
        self.palette.view = None;
        self.palette.pending_query = None;
        self.palette.target = None;
        self.palette.activation = None;
        self.palette.close_requested = false;
        if let Some(focus) = self.palette.prior_focus.take() {
            window.focus(&focus, cx);
        }
        cx.notify();
    }
    fn search_palette(&mut self, query: String, cx: &mut Context<Self>) {
        self.palette.generation = self.palette.generation.wrapping_add(1);
        self.palette.cancel.cancel();
        if self.palette.search_busy {
            self.palette.pending_query = Some(query);
            return;
        }
        let Some(service) = service(cx) else {
            if let Some(view) = &self.palette.view {
                view.update(cx, |view, cx| {
                    view.apply(Err(PaletteError::Unavailable), cx)
                });
            }
            return;
        };
        let current = route(cx);
        let project = current.0.clone();
        let generation = self.palette.generation;
        self.palette.cancel = CancellationToken::new();
        let cancel = self.palette.cancel.clone();
        let worker_cancel = cancel.clone();
        self.palette.search_busy = true;
        worker(
            cx,
            move || service.search(project.as_deref(), &query, &worker_cancel),
            move |this, result, cx| {
                this.palette.search_busy = false;
                if let Some(query) = this.palette.pending_query.take() {
                    if this.palette.view.is_some() {
                        this.search_palette(query, cx);
                    }
                    return;
                }
                if !cancel.is_cancelled()
                    && this.palette.generation == generation
                    && route(cx) == current
                    && let Some(view) = &this.palette.view
                {
                    view.update(cx, |view, cx| view.apply(result, cx));
                }
            },
        );
    }
    /// Render-loop hook applies window-bearing actions and completed read-only panes.
    pub(crate) fn sync_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.view.is_some()
            && (self.palette.close_requested || self.palette.route.as_ref() != Some(&route(cx)))
        {
            self.close_palette(window, cx);
        }
        if let Some((origin, project, preview)) = self.palette.preview.take() {
            if route(cx) != origin {
                return;
            }
            let path = preview.relative_path.clone();
            let view = cx.new(|cx| FilePreview::new(preview, cx));
            cx.subscribe(&view, move |_, _, event: &RevealFileRequested, cx| {
                if event.relative_path != path || route(cx).0.as_deref() != Some(&project) {
                    return;
                }
                if let Some(service) = service(cx) {
                    let project = project.clone();
                    let path = path.clone();
                    let _ = std::thread::Builder::new()
                        .name("vega-file-reveal".into())
                        .spawn(move || {
                            if let Ok(path) = service.reveal_path(&project, &path) {
                                let _ = std::process::Command::new("/usr/bin/open")
                                    .arg("-R")
                                    .arg(path)
                                    .status();
                            }
                        });
                }
            })
            .detach();
            cx.set_global(SettingsOpen(false));
            self.workspace_open_file(view, window, cx);
        }
        if self.palette.activation.is_some() {
            self.palette.target = None;
            return;
        }
        let Some(target) = self.palette.target.take() else {
            return;
        };
        match target {
            PaletteTarget::Action(action) => {
                self.close_palette(window, cx);
                match action {
                    PaletteAction::NewTask => self
                        .sidebar
                        .update(cx, |sidebar, cx| sidebar.create_thread(cx)),
                    PaletteAction::OpenWorkspace => {
                        self.palette_open_workspace(&OpenWorkspacePicker, window, cx)
                    }
                    PaletteAction::Settings => {
                        cx.set_global(SettingsOpen(true));
                        cx.refresh_windows();
                    }
                    PaletteAction::ToggleSidebar => toggle_persisted(cx),
                    PaletteAction::Terminal => {
                        self.palette_toggle_terminal(&ToggleWorkspaceTerminal, window, cx)
                    }
                    PaletteAction::Review => self.workspace_open_diff(cx),
                    PaletteAction::Preview => self.workspace_open_preview(window, cx),
                }
            }
            PaletteTarget::Task(task) => {
                let mutation = cx
                    .try_global::<vega_ui::navigation::TaskMutationState>()
                    .copied()
                    .unwrap_or_default();
                if mutation.pending > 0 {
                    if let Some(view) = &self.palette.view {
                        view.update(cx, |view, cx| {
                            view.apply(Err(PaletteError::Unavailable), cx)
                        });
                    }
                    return;
                }
                if !vega_ui::navigation::allow_task_navigation(Some(&task.id), cx) {
                    return;
                }
                let Some(service) = service(cx) else {
                    return;
                };
                let current = route(cx);
                let generation = self.palette.generation;
                self.palette.activation = Some(generation);
                if let Some(view) = &self.palette.view {
                    view.update(cx, |view, cx| view.begin_activation(cx));
                }
                self.palette.cancel.cancel();
                worker(
                    cx,
                    move || service.open_task(&task),
                    move |this, result, cx| {
                        if this.palette.activation == Some(generation) {
                            this.palette.activation = None;
                        }
                        if this.palette.generation != generation || route(cx) != current {
                            return;
                        }
                        let latest = cx
                            .try_global::<vega_ui::navigation::TaskMutationState>()
                            .copied()
                            .unwrap_or_default();
                        if latest != mutation {
                            if let Some(view) = &this.palette.view {
                                view.update(cx, |view, cx| {
                                    view.apply(Err(PaletteError::Unavailable), cx)
                                });
                            }
                            return;
                        }
                        match result {
                            Ok(thread) => {
                                if this.accept_palette_thread(thread, cx) {
                                    this.palette.close_requested = true;
                                    cx.refresh_windows();
                                } else if let Some(view) = &this.palette.view {
                                    view.update(cx, |view, cx| {
                                        view.apply(Err(PaletteError::Unavailable), cx)
                                    });
                                }
                            }
                            Err(error) => {
                                if let Some(view) = &this.palette.view {
                                    view.update(cx, |view, cx| view.apply(Err(error), cx));
                                }
                            }
                        }
                    },
                );
            }
            PaletteTarget::File(path) => {
                let Some(service) = service(cx) else {
                    return;
                };
                let current = route(cx);
                let Some(project) = current.0.clone() else {
                    return;
                };
                let generation = self.palette.generation;
                self.palette.activation = Some(generation);
                if let Some(view) = &self.palette.view {
                    view.update(cx, |view, cx| view.begin_activation(cx));
                }
                let result_project = project.clone();
                self.palette.cancel.cancel();
                worker(
                    cx,
                    move || service.preview(&project, &path),
                    move |this, result, cx| {
                        if this.palette.activation == Some(generation) {
                            this.palette.activation = None;
                        }
                        if this.palette.generation != generation || route(cx) != current {
                            return;
                        }
                        match result {
                            Ok(preview) => {
                                this.palette.preview = Some((current, result_project, preview));
                                this.palette.close_requested = true;
                                cx.notify();
                            }
                            Err(error) => {
                                if let Some(view) = &this.palette.view {
                                    view.update(cx, |view, cx| view.apply(Err(error), cx));
                                }
                            }
                        }
                    },
                );
            }
        }
    }
}
fn worker<T: Send + 'static>(
    cx: &mut Context<VegaWindow>,
    work: impl FnOnce() -> Result<T, PaletteError> + Send + 'static,
    done: impl FnOnce(&mut VegaWindow, Result<T, PaletteError>, &mut Context<VegaWindow>) + 'static,
) {
    let (sender, receiver) = mpsc::sync_channel(1);
    let spawned = std::thread::Builder::new()
        .name("vega-palette".into())
        .spawn(move || {
            let _ = sender.send(work());
        });
    let _ = spawned;
    cx.spawn(async move |owner, cx| {
        let result = loop {
            match receiver.try_recv() {
                Ok(result) => break result,
                Err(mpsc::TryRecvError::Disconnected) => break Err(PaletteError::Unavailable),
                Err(mpsc::TryRecvError::Empty) => {
                    cx.background_executor()
                        .timer(Duration::from_millis(25))
                        .await
                }
            }
        };
        let _ = owner.update(cx, move |owner, cx| done(owner, result, cx));
    })
    .detach();
}
