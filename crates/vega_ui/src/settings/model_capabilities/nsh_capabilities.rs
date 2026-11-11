use gpui_kit::{App, Context, Window};
use nsh_core::prelude::*;
use tokio::sync::{mpsc, oneshot};
use vega_conversation::types::{
    ModelCapabilityOverride, ModelCapabilitySettingsErrorCode, ModelCapabilitySettingsProjection,
    ModelIdentity, REASONING_EFFORTS,
};
use vega_theme::{Layout, ThemeColors, Typography};

use super::SettingsView;
use crate::nsh::{ProjectionId, Surface};

#[derive(Default)]
pub(in crate::settings) struct Presentations([Option<Presentation>; 2]);

struct Presentation {
    surface: Surface<Snapshot, Request>,
    id: ProjectionId,
    height: f32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pane {
    Efforts,
    Image,
}

impl Pane {
    fn index(self) -> usize {
        match self {
            Self::Efforts => 0,
            Self::Image => 1,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Loading,
    Ready(u64),
    Saving(u64, u64),
}

#[derive(Clone, PartialEq, Eq)]
struct Row {
    index: usize,
    identity: ModelIdentity,
    fingerprint: String,
    encodable: Vec<String>,
    allowed: Vec<String>,
    image: bool,
    editable: ModelCapabilityOverride,
    dirty: bool,
}

#[derive(Clone)]
struct Snapshot {
    id: ProjectionId,
    pane: Pane,
    phase: Phase,
    error: Option<ModelCapabilitySettingsErrorCode>,
    section: usize,
    route: Option<(String, String)>,
    visible: bool,
    rows: Vec<Row>,
    draft: Option<ModelCapabilityOverride>,
    colors: ThemeColors,
}

#[derive(Clone, Copy)]
enum Action {
    Reload,
    Effort(usize, &'static str),
    Image(usize),
    Save(usize),
    Cancel(usize),
    Reset(usize),
}

struct Decision {
    observed: Snapshot,
    action: Action,
    reply: oneshot::Sender<bool>,
}

enum Request {
    Decide(Box<Decision>),
    Layout(f32),
}

fn allowed(snapshot: &Snapshot, action: Action) -> bool {
    if matches!(action, Action::Reload) {
        return snapshot.error.is_some()
            && (snapshot.draft.is_none() || snapshot.rows.is_empty())
            && !matches!(snapshot.phase, Phase::Saving(..));
    }
    if !matches!(snapshot.phase, Phase::Ready(_)) {
        return false;
    }
    let index = match action {
        Action::Effort(index, _)
        | Action::Image(index)
        | Action::Save(index)
        | Action::Cancel(index)
        | Action::Reset(index) => index,
        Action::Reload => return false,
    };
    let Some(row) = snapshot.rows.iter().find(|row| row.index == index) else {
        return false;
    };
    match action {
        Action::Effort(_, effort) => {
            snapshot.pane == Pane::Efforts && row.encodable.iter().any(|value| value == effort)
        }
        Action::Image(_) => snapshot.pane == Pane::Image,
        Action::Save(_) | Action::Cancel(_) => row.dirty,
        Action::Reset(_) => true,
        Action::Reload => false,
    }
}

fn accepts(current: &Snapshot, observed: &Snapshot, action: Action) -> bool {
    current.visible
        && current.pane == observed.pane
        && current.section == observed.section
        && current.route == observed.route
        && current.phase == observed.phase
        && current.error == observed.error
        && current.rows == observed.rows
        && current.draft == observed.draft
        && allowed(current, action)
}

impl SettingsView {
    fn capabilities_snapshot(&self, pane: Pane, id: ProjectionId, cx: &App) -> Snapshot {
        let (phase, error) = match &self.model_capabilities {
            ModelCapabilitySettingsProjection::Loading => (Phase::Loading, None),
            ModelCapabilitySettingsProjection::Ready {
                generation, error, ..
            } => (Phase::Ready(*generation), *error),
            ModelCapabilitySettingsProjection::Saving {
                generation,
                operation_id,
                ..
            } => (Phase::Saving(*generation, *operation_id), None),
        };
        let rows = self
            .model_capability_models
            .iter()
            .enumerate()
            .filter_map(|(index, model)| {
                let identity = &model.resolved.identity;
                let matches = self
                    .model_settings
                    .as_ref()
                    .is_some_and(|(provider, name)| {
                        *provider == identity.provider && *name == identity.model
                    });
                if (self.model_settings.is_some() && !matches) || (pane == Pane::Image && !matches)
                {
                    return None;
                }
                let editable = self.editable_capability(index)?;
                Some(Row {
                    index,
                    identity: identity.clone(),
                    fingerprint: model.resolved.fingerprint.clone(),
                    encodable: model.resolved.capabilities.encodable_efforts.clone(),
                    allowed: editable
                        .allowed_efforts
                        .clone()
                        .unwrap_or_else(|| model.resolved.capabilities.allowed_efforts.clone()),
                    image: editable
                        .image_input
                        .unwrap_or(model.resolved.capabilities.image_input),
                    editable,
                    dirty: self
                        .model_capability_draft
                        .as_ref()
                        .is_some_and(|draft| draft.identity == *identity),
                })
            })
            .collect();
        Snapshot {
            id,
            pane,
            phase,
            error,
            section: self.section,
            route: self.model_settings.clone(),
            visible: cx
                .try_global::<super::super::SettingsOpen>()
                .is_some_and(|open| open.0)
                && match pane {
                    Pane::Efforts => self.section == 2,
                    Pane::Image => self.model_editor_visible(),
                },
            rows,
            draft: self.model_capability_draft.clone(),
            colors: vega_theme::theme(cx).colors,
        }
    }

    pub(crate) fn sync_capability_presentations(&self, cx: &App) {
        for pane in [Pane::Efforts, Pane::Image] {
            if let Some(page) = &self.capability_presentations.0[pane.index()] {
                page.surface
                    .publish(self.capabilities_snapshot(pane, page.id.clone(), cx));
            }
        }
    }

    fn render_capabilities(
        &mut self,
        pane: Pane,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        use gpui_kit::{IntoElement, ParentElement, Styled};
        let index = pane.index();
        if self.capability_presentations.0[index].is_none() {
            let id = ProjectionId::new();
            let (surface, mut requests) = Surface::new(
                self.capabilities_snapshot(pane, id.clone(), cx),
                render,
                window,
                cx,
            );
            self.capability_presentations.0[index] = Some(Presentation {
                surface,
                id,
                height: Layout::SETTINGS_NOTES_MAX_HEIGHT,
            });
            cx.spawn_in(window, async move |this, cx| {
                while let Some(request) = requests.recv().await {
                    if this
                        .update_in(cx, |this, _, cx| {
                            match request {
                                Request::Decide(command) => {
                                    let accepted = this.capability_presentations.0[index]
                                        .as_ref()
                                        .is_some_and(|page| {
                                            page.id.matches(&command.observed.id)
                                                && accepts(
                                                    &this.capabilities_snapshot(
                                                        pane,
                                                        page.id.clone(),
                                                        cx,
                                                    ),
                                                    &command.observed,
                                                    command.action,
                                                )
                                        });
                                    if accepted {
                                        match command.action {
                                            Action::Reload => {
                                                if this.model_capability_draft.is_some() {
                                                    this.cancel_model_capability(cx);
                                                }
                                                cx.emit(
                                                    super::super::ModelCapabilityReloadRequested,
                                                );
                                            }
                                            Action::Effort(index, effort) => {
                                                this.toggle_capability_effort(index, effort, cx)
                                            }
                                            Action::Image(index) => {
                                                this.toggle_capability_image(index, cx)
                                            }
                                            Action::Save(index) => {
                                                this.save_model_capability(index, false, cx)
                                            }
                                            Action::Cancel(_) => this.cancel_model_capability(cx),
                                            Action::Reset(index) => {
                                                this.save_model_capability(index, true, cx)
                                            }
                                        }
                                    }
                                    let _ = command.reply.send(accepted);
                                }
                                Request::Layout(height) if height.is_finite() && height > 0. => {
                                    if let Some(page) = &mut this.capability_presentations.0[index]
                                        && (page.height - height).abs() > 0.5
                                    {
                                        page.height = height;
                                        cx.notify();
                                    }
                                }
                                Request::Layout(_) => {}
                            }
                            this.sync_capability_presentations(cx);
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();
        }
        let Some(page) = &self.capability_presentations.0[index] else {
            return gpui_kit::div().into_any_element();
        };
        let snapshot = self.capabilities_snapshot(pane, page.id.clone(), cx);
        let Some(page) = &mut self.capability_presentations.0[index] else {
            return gpui_kit::div().into_any_element();
        };
        page.surface.sync_environment(cx);
        page.surface.publish(snapshot);
        gpui_kit::div()
            .w_full()
            .flex_shrink_0()
            .h(gpui_kit::px(page.height))
            .child(page.surface.entity())
            .into_any_element()
    }

    pub(crate) fn render_model_capability_efforts(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        self.render_capabilities(Pane::Efforts, window, cx)
    }

    pub(crate) fn render_model_capability_image(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        self.render_capabilities(Pane::Image, window, cx)
    }
}

#[derive(Clone, Copy)]
struct Actions {
    owner: nsh_core::element::Owner,
    state: Signal<Snapshot>,
    pending: Signal<bool>,
    failed: Signal<bool>,
}

impl Actions {
    fn send(self, commands: mpsc::Sender<Request>, action: Action) {
        let observed = self.state.get_untracked();
        if self.pending.get_untracked() || !accepts(&observed, &observed, action) {
            return;
        }
        self.pending.set(true);
        self.failed.set(false);
        self.owner.run(|| {
            spawn(async move {
                let (reply, response) = oneshot::channel();
                let accepted = commands
                    .send(Request::Decide(Box::new(Decision {
                        observed,
                        action,
                        reply,
                    })))
                    .await
                    .is_ok()
                    && matches!(response.await, Ok(true));
                self.failed.set(!accepted);
                self.pending.set(false);
            });
        });
    }
}

fn action_label(snapshot: &Snapshot, action: Action) -> String {
    match action {
        Action::Reload if snapshot.draft.is_some() => vega_i18n::t("ui.discard_and_reload").into(),
        Action::Reload => vega_i18n::t("ui.retry").into(),
        Action::Save(_) => vega_i18n::t("settings.action.save").into(),
        Action::Cancel(_) => vega_i18n::t("settings.action.cancel").into(),
        Action::Reset(_) => vega_i18n::t("settings.capabilities.reset").into(),
        Action::Image(_) => vega_i18n::t("settings.capabilities.image_input").into(),
        Action::Effort(index, effort) => {
            let label = super::effort_label(effort);
            if snapshot
                .rows
                .iter()
                .find(|row| row.index == index)
                .is_some_and(|row| row.encodable.iter().any(|value| value == effort))
            {
                label.into()
            } else {
                vega_i18n::t_args("settings.capabilities.unsupported", &[("label", label)])
            }
        }
    }
}

fn selected(snapshot: &Snapshot, action: Action) -> Option<bool> {
    match action {
        Action::Image(index) => snapshot
            .rows
            .iter()
            .find(|row| row.index == index)
            .map(|row| row.image),
        Action::Effort(index, effort) => snapshot
            .rows
            .iter()
            .find(|row| row.index == index)
            .map(|row| row.allowed.iter().any(|value| value == effort)),
        _ => None,
    }
}

fn current_row_index(snapshot: &Snapshot, observed: &Row) -> usize {
    snapshot
        .rows
        .iter()
        .find(|current| *current == observed)
        .map_or(usize::MAX, |current| current.index)
}

fn row_index(state: Signal<Snapshot>, row: ReadSignal<Row>) -> usize {
    let observed = row.get();
    state.with(|snapshot| current_row_index(snapshot, &observed))
}

fn control(
    actions: Actions,
    commands: mpsc::Sender<Request>,
    action: impl Fn() -> Action + Copy + 'static,
) -> AnyElement {
    let target = element_ref();
    let focused = signal(false);
    target.on_focus_in(move || focused.set(true));
    target.on_focus_out(move || focused.set(false));
    view()
        .element_ref(&target)
        .role(AccessibilityRole::Button)
        .focusable(true)
        .label(move || {
            actions
                .state
                .with(|snapshot| action_label(snapshot, action()))
        })
        .accessibility_state(move || AccessibilityState {
            pressed: selected(&actions.state.get(), action()).map(|checked| {
                if checked {
                    nsh_core::Checked::True
                } else {
                    nsh_core::Checked::False
                }
            }),
            ..Default::default()
        })
        .disabled(move || actions.pending.get() || !allowed(&actions.state.get(), action()))
        .flex_row()
        .items_center()
        .gap_2()
        .border(px(1.))
        .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
        .px_3()
        .py_2()
        .border_color(move || {
            crate::nsh::color(if focused.get() {
                actions.state.get().colors.accent
            } else {
                actions.state.get().colors.border_subtle
            })
        })
        .bg(move || {
            crate::nsh::color(if selected(&actions.state.get(), action()) == Some(true) {
                actions.state.get().colors.bg_active
            } else {
                actions.state.get().colors.bg_elevated
            })
        })
        .text_color(move || {
            crate::nsh::color(if !allowed(&actions.state.get(), action()) {
                actions.state.get().colors.text_tertiary
            } else {
                actions.state.get().colors.text_primary
            })
        })
        .on_press(move |_: &PressEvent| actions.send(commands.clone(), action()))
        .child(when(
            move || selected(&actions.state.get(), action()) == Some(true),
            || text("✓"),
        ))
        .child(text(move || {
            actions
                .state
                .with(|snapshot| action_label(snapshot, action()))
        }))
        .into_element()
}

fn error_key(error: ModelCapabilitySettingsErrorCode) -> &'static str {
    match error {
        ModelCapabilitySettingsErrorCode::Io => "settings.capabilities.error.io",
        ModelCapabilitySettingsErrorCode::Invalid => "settings.capabilities.error.invalid",
        ModelCapabilitySettingsErrorCode::Conflict => "settings.capabilities.error.conflict",
        ModelCapabilitySettingsErrorCode::Busy => "settings.capabilities.error.busy",
        ModelCapabilitySettingsErrorCode::MissingModel => "settings.capabilities.error.missing",
    }
}

fn render(state: Signal<Snapshot>, commands: mpsc::Sender<Request>) -> AnyElement {
    let actions = Actions {
        owner: nsh_core::element::Owner::current(),
        state,
        pending: signal(false),
        failed: signal(false),
    };
    let measured = signal(0.0f32);
    let layout = commands.clone();
    let rows = commands.clone();
    view()
        .w_full()
        .flex_shrink_0()
        .gap(px(Layout::SETTINGS_SECTION_GAP))
        .text_size(px(Typography::BODY))
        .text_color(move || crate::nsh::color(state.get().colors.text_primary))
        .on_layout(move |bounds: &Bounds<Points>| {
            let height = bounds.size.height.0;
            if height.is_finite() && height > 0. && (measured.get_untracked() - height).abs() > 0.5
            {
                measured.set(height);
                let commands = layout.clone();
                spawn(async move {
                    if commands.send(Request::Layout(height)).await.is_err() {
                        actions.failed.set(true);
                    }
                });
            }
        })
        .child(when(
            move || state.get().pane == Pane::Efforts,
            move || text(move || state.with(|_| vega_i18n::t("settings.capabilities.efforts"))),
        ))
        .child(when(
            move || !matches!(state.get().phase, Phase::Ready(_)),
            move || {
                text(move || {
                    state.with(|snapshot| {
                        vega_i18n::t(if snapshot.phase == Phase::Loading {
                            "settings.capabilities.loading"
                        } else {
                            "settings.saving"
                        })
                    })
                })
            },
        ))
        .child(when(
            move || state.get().error.is_some(),
            move || {
                text(move || {
                    state.with(|snapshot| {
                        snapshot
                            .error
                            .map(|error| vega_i18n::t(error_key(error)))
                            .unwrap_or_default()
                    })
                })
                .text_color(move || crate::nsh::color(state.get().colors.danger))
            },
        ))
        .child(when(
            move || allowed(&state.get(), Action::Reload),
            move || control(actions, commands.clone(), || Action::Reload),
        ))
        .child(each(
            move || state.get().rows,
            |row| (row.identity.provider.clone(), row.identity.model.clone()),
            move |row, _| {
                let options = rows.clone();
                let save = rows.clone();
                let cancel = rows.clone();
                let reset = rows.clone();
                view()
                    .w_full()
                    .gap_3()
                    .child(when(
                        move || state.get().route.is_none(),
                        move || {
                            text(move || {
                                let row = row.get();
                                format!("{} / {}", row.identity.provider, row.identity.model)
                            })
                        },
                    ))
                    .child(when(
                        move || state.get().pane == Pane::Efforts,
                        move || {
                            let commands = options.clone();
                            view().flex_row().flex_wrap().gap_2().children(
                                REASONING_EFFORTS.iter().copied().map(move |effort| {
                                    control(actions, commands.clone(), move || {
                                        Action::Effort(row_index(state, row), effort)
                                    })
                                }),
                            )
                        },
                    ))
                    .child(when(move || state.get().pane == Pane::Image, {
                        let commands = rows.clone();
                        move || {
                            control(actions, commands.clone(), move || {
                                Action::Image(row_index(state, row))
                            })
                        }
                    }))
                    .child(
                        view()
                            .flex_row()
                            .flex_wrap()
                            .gap_2()
                            .child(control(actions, save, move || {
                                Action::Save(row_index(state, row))
                            }))
                            .child(control(actions, cancel, move || {
                                Action::Cancel(row_index(state, row))
                            }))
                            .child(control(actions, reset, move || {
                                Action::Reset(row_index(state, row))
                            })),
                    )
            },
        ))
        .child(when(
            move || actions.failed.get(),
            move || {
                text(move || state.with(|_| vega_i18n::t("ui.command_failed")))
                    .text_color(move || crate::nsh::color(state.get().colors.danger))
            },
        ))
        .into_element()
}
