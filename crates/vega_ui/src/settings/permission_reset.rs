use gpui_kit::{Context, Window};
use nsh_core::prelude::*;
use tokio::sync::{mpsc, oneshot};
use vega_conversation::types::{
    PermissionResetAction as Action, PermissionResetError, PermissionResetProjection as Projection,
    PermissionResetRequested,
};
use vega_theme::{Layout, ThemeColors, Typography};

use super::SettingsView;
use crate::nsh::{ProjectionId, Surface};

pub(crate) struct ResetState {
    pub(crate) projection: Projection,
    id: ProjectionId,
    revision: u64,
    pending: bool,
    focus_completed: bool,
    presentation: Option<Presentation>,
}

struct Presentation {
    surface: Surface<Snapshot, Request>,
    height: f32,
}

#[derive(Clone)]
struct Snapshot {
    id: ProjectionId,
    revision: u64,
    projection: Projection,
    pending: bool,
    colors: ThemeColors,
}

struct Decision {
    id: ProjectionId,
    revision: u64,
    projection: Projection,
    action: Action,
    reply: oneshot::Sender<bool>,
}

enum Request {
    Decide(Decision),
    Layout(f32),
}

fn allowed(projection: &Projection, action: Action) -> bool {
    match (projection, action) {
        (Projection::Idle | Projection::Completed, Action::Prepare)
        | (Projection::Confirming, Action::Confirm)
        | (Projection::Preparing | Projection::Confirming, Action::Cancel)
        | (Projection::Blocked { .. }, Action::Retry) => true,
        (Projection::Blocked { can_cancel, .. }, Action::Cancel) => *can_cancel,
        (Projection::Blocked { thread_id, .. }, Action::Inspect) => thread_id.is_some(),
        _ => false,
    }
}

impl ResetState {
    pub(super) fn new() -> Self {
        Self {
            projection: Projection::Idle,
            id: ProjectionId::new(),
            revision: 0,
            pending: false,
            focus_completed: false,
            presentation: None,
        }
    }
}

impl SettingsView {
    fn reset_snapshot(&self, cx: &gpui_kit::App) -> Snapshot {
        Snapshot {
            id: self.permission_reset.id.clone(),
            revision: self.permission_reset.revision,
            projection: self.permission_reset.projection.clone(),
            pending: self.permission_reset.pending,
            colors: vega_theme::theme(cx).colors,
        }
    }

    pub fn apply_permission_reset_projection(
        &mut self,
        projection: Projection,
        cx: &mut Context<Self>,
    ) {
        if self.permission_reset.projection != projection || self.permission_reset.pending {
            self.permission_reset.focus_completed = projection == Projection::Completed;
            self.permission_reset.projection = projection;
            self.permission_reset.revision = self.permission_reset.revision.wrapping_add(1);
            self.permission_reset.pending = false;
            if let Some(page) = &self.permission_reset.presentation {
                page.surface.publish(self.reset_snapshot(cx));
            }
            cx.notify();
        }
    }

    pub(crate) fn prepare_permission_reset_focus(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.section == 1 && std::mem::take(&mut self.permission_reset.focus_completed) {
            self.focus_permission_picker(window, cx);
        }
    }

    pub(crate) fn render_permission_reset(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        use gpui_kit::{IntoElement, ParentElement, Styled};
        if self.permission_reset.presentation.is_none() {
            let (surface, mut requests) = Surface::new(self.reset_snapshot(cx), render, window, cx);
            self.permission_reset.presentation = Some(Presentation {
                surface,
                height: Layout::SETTINGS_NOTES_MAX_HEIGHT,
            });
            cx.spawn_in(window, async move |this, cx| {
                while let Some(request) = requests.recv().await {
                    if this
                        .update_in(cx, |this, _, cx| {
                            match request {
                                Request::Decide(command) => {
                                    let accepted = this.section == 1
                                        && this.permission_reset.id.matches(&command.id)
                                        && this.permission_reset.revision == command.revision
                                        && this.permission_reset.projection == command.projection
                                        && !this.permission_reset.pending
                                        && allowed(
                                            &this.permission_reset.projection,
                                            command.action,
                                        );
                                    if accepted {
                                        this.permission_reset.pending =
                                            command.action != Action::Inspect;
                                        cx.emit(PermissionResetRequested(command.action));
                                    }
                                    let _ = command.reply.send(accepted);
                                }
                                Request::Layout(height) if height.is_finite() && height > 0. => {
                                    if let Some(page) = &mut this.permission_reset.presentation
                                        && (page.height - height).abs() > 0.5
                                    {
                                        page.height = height;
                                        cx.notify();
                                    }
                                }
                                Request::Layout(_) => {}
                            }
                            if let Some(page) = &this.permission_reset.presentation {
                                page.surface.publish(this.reset_snapshot(cx));
                            }
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();
        }
        let snapshot = self.reset_snapshot(cx);
        let Some(page) = &mut self.permission_reset.presentation else {
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
}

#[derive(Clone, Copy)]
struct Actions {
    state: Signal<Snapshot>,
    waiting: Signal<bool>,
    failed: Signal<bool>,
}

impl Actions {
    fn send(self, commands: mpsc::Sender<Request>, action: Action) {
        let snapshot = self.state.get_untracked();
        if self.waiting.get_untracked()
            || snapshot.pending
            || !allowed(&snapshot.projection, action)
        {
            return;
        }
        self.waiting.set(true);
        self.failed.set(false);
        spawn(async move {
            let (reply, response) = oneshot::channel();
            if commands
                .send(Request::Decide(Decision {
                    id: snapshot.id,
                    revision: snapshot.revision,
                    projection: snapshot.projection,
                    action,
                    reply,
                }))
                .await
                .is_err()
                || !matches!(response.await, Ok(true))
            {
                self.failed.set(true);
            }
            self.waiting.set(false);
        });
    }
}

fn button(
    actions: Actions,
    commands: mpsc::Sender<Request>,
    action: Action,
    target: ElementRef,
    focused_revision: Signal<Option<u64>>,
) -> AnyElement {
    let key = match action {
        Action::Prepare => "ui.permission_reset_action",
        Action::Cancel => "ui.permission_reset_cancel",
        Action::Confirm => "ui.permission_reset_confirm",
        Action::Retry => "ui.permission_reset_retry",
        Action::Inspect => "ui.permission_reset_inspect",
    };
    let focused = signal(false);
    target.on_focus_in(move || focused.set(true));
    target.on_focus_out(move || focused.set(false));
    view()
        .role(AccessibilityRole::Button)
        .element_ref(&target)
        .label(move || actions.state.with(|_| vega_i18n::t(key)))
        .disabled(move || {
            actions.waiting.get()
                || actions
                    .state
                    .with(|s| s.pending || !allowed(&s.projection, action))
        })
        .px_3()
        .py_2()
        .border(px(1.))
        .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
        .border_color(move || {
            crate::nsh::color(if focused.get() {
                actions.state.get().colors.accent
            } else {
                actions.state.get().colors.border_subtle
            })
        })
        .bg(move || crate::nsh::color(actions.state.get().colors.bg_elevated))
        .on_layout(move |_: &Bounds<Points>| {
            let snapshot = actions.state.get_untracked();
            let default_action = match snapshot.projection {
                Projection::Preparing | Projection::Confirming => Some(Action::Cancel),
                Projection::Blocked { .. } => Some(Action::Retry),
                _ => None,
            };
            if !snapshot.pending
                && default_action == Some(action)
                && focused_revision.get_untracked() != Some(snapshot.revision)
            {
                focused_revision.set(Some(snapshot.revision));
                target.focus();
            }
        })
        .on_press(move |_: &nsh_core::PressEvent| actions.send(commands.clone(), action))
        .child(text(move || actions.state.with(|_| vega_i18n::t(key))))
        .into_element()
}

fn message(projection: &Projection) -> &'static str {
    match projection {
        Projection::Preparing => "ui.permission_reset_preparing",
        Projection::Confirming => "ui.permission_reset_title",
        Projection::Pausing | Projection::Stopping => "ui.permission_reset_stopping",
        Projection::Refreshing => "ui.permission_reset_refreshing",
        Projection::Blocked { error, .. } => match error {
            PermissionResetError::Busy => "ui.permission_reset_busy",
            PermissionResetError::UnsettledWork => "ui.permission_reset_unsettled",
            PermissionResetError::ConfigurationConflict => "ui.permission_reset_conflict",
            PermissionResetError::ConfigurationUnavailable => "ui.permission_reset_configuration",
            PermissionResetError::OutcomeUnknown => "ui.permission_reset_unknown",
            PermissionResetError::Unavailable => "ui.permission_reset_unavailable",
        },
        Projection::Idle | Projection::Completed => "ui.permission_reset_action",
    }
}

fn render(state: Signal<Snapshot>, commands: mpsc::Sender<Request>) -> AnyElement {
    let actions = Actions {
        state,
        waiting: signal(false),
        failed: signal(false),
    };
    let cancel = element_ref();
    let confirm = element_ref();
    let retry = element_ref();
    let inspect = element_ref();
    let root = element_ref();
    let within = signal(false);
    root.on_focus_in(move || within.set(true));
    root.on_focus_out(move || within.set(false));
    let cancel_focused = signal(false);
    cancel.on_focus_in(move || cancel_focused.set(true));
    cancel.on_focus_out(move || cancel_focused.set(false));
    let measured = signal(0.0f32);
    let focused_revision = signal(None::<u64>);
    let layout = commands.clone();
    let escape = commands.clone();
    let cancel_commands = commands.clone();
    let confirm_commands = commands.clone();
    let retry_commands = commands.clone();
    view()
        .element_ref(&root)
        .w_full()
        .flex_shrink_0()
        .gap_3()
        .p(px(Layout::SETTINGS_GROUP_PADDING))
        .border(px(1.))
        .rounded(px(Layout::PANEL_RADIUS))
        .border_color(move || {
            crate::nsh::color(if state.get().projection == Projection::Confirming {
                state.get().colors.warning
            } else {
                state.get().colors.border_subtle
            })
        })
        .text_color(move || crate::nsh::color(state.get().colors.text_primary))
        .text_size(px(Typography::BODY))
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
            move || {
                within.get()
                    && !state.get().pending
                    && matches!(
                        state.get().projection,
                        Projection::Preparing | Projection::Confirming
                    )
            },
            move || {
                let commands = escape.clone();
                on_key_with(
                    "escape",
                    KeyOptions {
                        when: nsh_core::keymap::When::NotEditing,
                        ..Default::default()
                    },
                    move |_| actions.send(commands.clone(), Action::Cancel),
                );
                view()
            },
        ))
        .child(when(
            move || {
                within.get()
                    && !state.get().pending
                    && state.get().projection == Projection::Confirming
            },
            move || {
                for key in ["tab", "shift+tab"] {
                    on_key_with(
                        key,
                        KeyOptions {
                            when: nsh_core::keymap::When::Always,
                            ..Default::default()
                        },
                        move |_| {
                            if cancel_focused.get_untracked() {
                                confirm.focus();
                            } else {
                                cancel.focus();
                            }
                        },
                    );
                }
                view()
            },
        ))
        .child(
            text(move || state.with(|snapshot| vega_i18n::t(message(&snapshot.projection))))
                .text_color(move || {
                    crate::nsh::color(
                        if matches!(state.get().projection, Projection::Blocked { .. }) {
                            state.get().colors.danger
                        } else {
                            state.get().colors.text_primary
                        },
                    )
                }),
        )
        .child(when(
            move || state.get().projection == Projection::Confirming,
            move || {
                view()
                    .gap_2()
                    .child(text(move || {
                        state.with(|_| vega_i18n::t("ui.permission_reset_scope"))
                    }))
                    .child(
                        text(move || state.with(|_| vega_i18n::t("ui.permission_reset_effects")))
                            .text_color(move || crate::nsh::color(state.get().colors.warning)),
                    )
            },
        ))
        .child(
            view()
                .flex_row()
                .flex_wrap()
                .gap_2()
                .child(when(
                    move || allowed(&state.get().projection, Action::Cancel),
                    move || {
                        button(
                            actions,
                            cancel_commands.clone(),
                            Action::Cancel,
                            cancel,
                            focused_revision,
                        )
                    },
                ))
                .child(when(
                    move || allowed(&state.get().projection, Action::Confirm),
                    move || {
                        button(
                            actions,
                            confirm_commands.clone(),
                            Action::Confirm,
                            confirm,
                            focused_revision,
                        )
                    },
                ))
                .child(when(
                    move || allowed(&state.get().projection, Action::Retry),
                    move || {
                        button(
                            actions,
                            retry_commands.clone(),
                            Action::Retry,
                            retry,
                            focused_revision,
                        )
                    },
                ))
                .child(when(
                    move || allowed(&state.get().projection, Action::Inspect),
                    move || {
                        button(
                            actions,
                            commands.clone(),
                            Action::Inspect,
                            inspect,
                            focused_revision,
                        )
                    },
                )),
        )
        .child(when(
            move || actions.failed.get(),
            move || {
                text(move || state.with(|_| vega_i18n::t("ui.command_failed")))
                    .text_color(move || crate::nsh::color(state.get().colors.danger))
            },
        ))
        .into_element()
}
