use gpui_kit::{App, Context, Window};
use nsh_core::prelude::*;
use tokio::sync::{mpsc, oneshot};
use vega_theme::{Layout, ThemeColors, Typography};

use super::{DefaultPermissionError, PermissionFocus, PermissionPhase, SettingsView};
use crate::nsh::{ProjectionId, Surface};

pub(super) struct Presentation {
    surface: Surface<Snapshot, Request>,
    id: ProjectionId,
    height: f32,
}

#[derive(Clone)]
struct Snapshot {
    id: ProjectionId,
    mode: String,
    permission_revision: i64,
    operation: u64,
    phase: PermissionPhase,
    error: Option<DefaultPermissionError>,
    open: bool,
    visible: bool,
    enabled: bool,
    reload_enabled: bool,
    focus: Option<(u64, PermissionFocus)>,
    colors: ThemeColors,
}

#[derive(Clone, Copy)]
enum Action {
    Toggle,
    Select(&'static str),
    Cancel,
    Reload,
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
    match action {
        Action::Toggle => snapshot.enabled,
        Action::Select(mode) => {
            snapshot.enabled && snapshot.open && super::super::PERMISSION_MODES.contains(&mode)
        }
        Action::Cancel => {
            !matches!(snapshot.phase, PermissionPhase::Saving(_))
                && (snapshot.open
                    || matches!(
                        snapshot.phase,
                        PermissionPhase::Preparing(_) | PermissionPhase::Reloading(_)
                    ))
        }
        Action::Reload => {
            snapshot.reload_enabled
                && snapshot.error.is_some()
                && snapshot.phase == PermissionPhase::Idle
        }
    }
}

fn accepts(current: &Snapshot, observed: &Snapshot, action: Action) -> bool {
    current.visible
        && current.operation == observed.operation
        && current.phase == observed.phase
        && current.mode == observed.mode
        && current.permission_revision == observed.permission_revision
        && current.error == observed.error
        && current.open == observed.open
        && allowed(current, action)
}

impl SettingsView {
    fn permission_snapshot(&self, cx: &App, id: ProjectionId) -> Snapshot {
        Snapshot {
            id,
            mode: self.config.defaults.permission_mode.clone(),
            permission_revision: self.config.defaults.permission_revision,
            operation: self.default_permission.operation,
            phase: self.default_permission.phase,
            error: self.default_permission.error.or_else(|| {
                self.default_permission
                    .service
                    .is_none()
                    .then_some(DefaultPermissionError::Unavailable)
            }),
            open: self.mode_open,
            visible: self.section == 1
                && cx
                    .try_global::<super::super::SettingsOpen>()
                    .is_some_and(|open| open.0),
            enabled: self.permission_picker_enabled(),
            reload_enabled: self.default_permission.service.is_some()
                && !self.provider_management.saving
                && !self.codex_profile_saving,
            focus: self.default_permission.nsh_focus,
            colors: vega_theme::theme(cx).colors,
        }
    }

    pub(crate) fn sync_permission_presentation(&self, cx: &App) {
        if let Some(page) = &self.default_permission.presentation {
            page.surface
                .publish(self.permission_snapshot(cx, page.id.clone()));
        }
    }

    pub(crate) fn focus_permission_presentation(
        &mut self,
        focus: PermissionFocus,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.section != 1
            || !cx
                .try_global::<super::super::SettingsOpen>()
                .is_some_and(|open| open.0)
        {
            return;
        }
        self.default_permission.focus_sequence =
            self.default_permission.focus_sequence.wrapping_add(1);
        self.default_permission.nsh_focus = Some((self.default_permission.focus_sequence, focus));
        if let Some(page) = &self.default_permission.presentation {
            page.surface.focus(window, cx);
        }
        self.sync_permission_presentation(cx);
    }

    pub(crate) fn focus_permission_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_permission_presentation(PermissionFocus::Picker, window, cx);
    }

    pub(crate) fn render_permission_picker(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        use gpui_kit::{IntoElement, ParentElement, Styled};
        if self.default_permission.presentation.is_none() {
            let id = ProjectionId::new();
            let (surface, mut requests) =
                Surface::new(self.permission_snapshot(cx, id.clone()), render, window, cx);
            self.default_permission.presentation = Some(Presentation {
                surface,
                id,
                height: Layout::SETTINGS_NOTES_MAX_HEIGHT,
            });
            cx.spawn_in(window, async move |this, cx| {
                while let Some(request) = requests.recv().await {
                    if this
                        .update_in(cx, |this, window, cx| {
                            match request {
                                Request::Decide(command) => {
                                    let accepted = this
                                        .default_permission
                                        .presentation
                                        .as_ref()
                                        .is_some_and(|page| {
                                            page.id.matches(&command.observed.id)
                                                && accepts(
                                                    &this.permission_snapshot(cx, page.id.clone()),
                                                    &command.observed,
                                                    command.action,
                                                )
                                        });
                                    if accepted {
                                        match command.action {
                                            Action::Toggle => {
                                                this.mode_open = !this.mode_open;
                                                cx.notify();
                                            }
                                            Action::Select(mode) => this.select_mode(mode, cx),
                                            Action::Cancel => {
                                                this.guard_default_permission_close(window, cx);
                                            }
                                            Action::Reload => this.reload_default_permission(cx),
                                        }
                                    }
                                    let _ = command.reply.send(accepted);
                                }
                                Request::Layout(height) if height.is_finite() && height > 0. => {
                                    if let Some(page) = &mut this.default_permission.presentation
                                        && (page.height - height).abs() > 0.5
                                    {
                                        page.height = height;
                                        cx.notify();
                                    }
                                }
                                Request::Layout(_) => {}
                            }
                            this.sync_permission_presentation(cx);
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();
        }
        let Some(page) = &self.default_permission.presentation else {
            return gpui_kit::div().into_any_element();
        };
        let snapshot = self.permission_snapshot(cx, page.id.clone());
        let Some(page) = &mut self.default_permission.presentation else {
            return gpui_kit::div().into_any_element();
        };
        page.surface.sync_environment(cx);
        page.surface.publish(snapshot);
        let mut host = gpui_kit::div()
            .flex()
            .flex_col()
            .w_full()
            .flex_shrink_0()
            .child(
                gpui_kit::div()
                    .w_full()
                    .flex_shrink_0()
                    .h(gpui_kit::px(page.height))
                    .child(page.surface.entity()),
            );
        if !matches!(
            self.permission_reset.projection,
            vega_conversation::types::PermissionResetProjection::Idle
                | vega_conversation::types::PermissionResetProjection::Completed
        ) {
            host = host.child(self.render_permission_reset(window, cx));
        }
        host.into_any_element()
    }
}

#[derive(Clone, Copy)]
struct Actions {
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
    }
}

fn control(
    actions: Actions,
    target: ElementRef,
    focus: Option<PermissionFocus>,
    focused_request: Signal<Option<u64>>,
) -> nsh_core::element::View {
    let focused = signal(false);
    target.on_focus_in(move || focused.set(true));
    target.on_focus_out(move || focused.set(false));
    let focus_target = move || {
        let snapshot = actions.state.get();
        let Some((sequence, requested)) = snapshot.focus else {
            return;
        };
        let enabled = match requested {
            PermissionFocus::Picker => snapshot.enabled,
            PermissionFocus::Cancel => allowed(&snapshot, Action::Cancel),
            PermissionFocus::Reload => allowed(&snapshot, Action::Reload),
        };
        if snapshot.visible
            && !actions.pending.get()
            && enabled
            && focus == Some(requested)
            && focused_request.get_untracked() != Some(sequence)
            && target.node().is_some()
        {
            focused_request.set(Some(sequence));
            target.focus();
        }
    };
    effect(focus_target);
    view()
        .element_ref(&target)
        .role(AccessibilityRole::Button)
        .focusable(true)
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
        .bg(move || crate::nsh::color(actions.state.get().colors.bg_elevated))
        .on_layout(move |_: &Bounds<Points>| focus_target())
}

fn icon(kind: crate::icons::Icon, state: Signal<Snapshot>, warning: bool) -> AnyElement {
    let source = crate::icons::nsh_source(kind).unwrap_or_default();
    svg(nsh_core::element::ImageSrc::svg_markup(&source))
        .size(px(Layout::ICON_SIZE))
        .flex_shrink_0()
        .tint_color(move || {
            crate::nsh::color(if warning {
                state.get().colors.warning
            } else {
                state.get().colors.text_secondary
            })
            .to_hsla()
        })
        .into_element()
}

fn error_key(error: DefaultPermissionError) -> &'static str {
    match error {
        DefaultPermissionError::Unavailable => "settings.permission.error.unavailable",
        DefaultPermissionError::Conflict => "settings.permission.error.conflict",
        DefaultPermissionError::ResetPending => "settings.permission.error.reset",
        DefaultPermissionError::ConfirmationRequired => "settings.permission.error.confirmation",
        DefaultPermissionError::OutcomeUnknown => "settings.permission.error.unknown",
    }
}

fn action_button(
    actions: Actions,
    commands: mpsc::Sender<Request>,
    action: Action,
    focus: PermissionFocus,
    key: &'static str,
    focused: Signal<Option<u64>>,
) -> AnyElement {
    control(actions, element_ref(), Some(focus), focused)
        .label(move || actions.state.with(|_| vega_i18n::t(key)))
        .disabled(move || actions.pending.get() || !allowed(&actions.state.get(), action))
        .on_press(move |_: &PressEvent| actions.send(commands.clone(), action))
        .child(text(move || actions.state.with(|_| vega_i18n::t(key))))
        .into_element()
}

fn render(state: Signal<Snapshot>, commands: mpsc::Sender<Request>) -> AnyElement {
    let actions = Actions {
        state,
        pending: signal(false),
        failed: signal(false),
    };
    let focused = signal(None::<u64>);
    let feedback = computed(move || state.with(|snapshot| (snapshot.phase, snapshot.error)));
    effect(move || {
        let snapshot = state.get();
        if !snapshot.visible {
            focused.set(snapshot.focus.map(|(sequence, _)| sequence));
        }
    });
    let root = element_ref();
    let within = signal(false);
    root.on_focus_in(move || within.set(true));
    root.on_focus_out(move || within.set(false));
    let measured = signal(0.0f32);
    let layout = commands.clone();
    let toggle = commands.clone();
    let escape = commands.clone();
    let options = commands.clone();
    view()
        .element_ref(&root)
        .w_full()
        .flex_shrink_0()
        .gap_2()
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
            move || within.get() && allowed(&state.get(), Action::Cancel),
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
        .child(
            control(
                actions,
                element_ref(),
                Some(PermissionFocus::Picker),
                focused,
            )
            .label(move || {
                state.with(|snapshot| {
                    format!(
                        "{}: {}",
                        vega_i18n::t("settings.defaults.permission_mode"),
                        super::super::permission_mode_label(&snapshot.mode)
                    )
                })
            })
            .accessibility_state(move || AccessibilityState {
                expanded: Some(state.get().open),
                ..Default::default()
            })
            .disabled(move || actions.pending.get() || !state.get().enabled)
            .flex_row()
            .items_center()
            .justify_between()
            .gap_2()
            .on_press(move |_: &PressEvent| actions.send(toggle.clone(), Action::Toggle))
            .child(text(move || {
                state.with(|snapshot| super::super::permission_mode_label(&snapshot.mode))
            }))
            .child(dynamic(move || {
                icon(
                    if state.get().open {
                        crate::icons::Icon::ChevronDown
                    } else {
                        crate::icons::Icon::ChevronRight
                    },
                    state,
                    false,
                )
            })),
        )
        .child(when(
            move || state.get().open && state.get().enabled,
            move || {
                let commands = options.clone();
                view()
                    .gap_1()
                    .children(super::super::PERMISSION_MODES.into_iter().map(move |mode| {
                        let commands = commands.clone();
                        control(actions, element_ref(), None, focused)
                            .label(move || {
                                state.with(|_| super::super::permission_mode_label(mode))
                            })
                            .accessibility_state(move || AccessibilityState {
                                selected: state.get().mode == mode,
                                ..Default::default()
                            })
                            .disabled(move || actions.pending.get() || !state.get().enabled)
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .bg(move || {
                                crate::nsh::color(if state.get().mode == mode {
                                    state.get().colors.bg_active
                                } else {
                                    state.get().colors.bg_elevated
                                })
                            })
                            .text_color(move || {
                                crate::nsh::color(if mode == "full_access" {
                                    state.get().colors.warning
                                } else {
                                    state.get().colors.text_primary
                                })
                            })
                            .on_press(move |_: &PressEvent| {
                                actions.send(commands.clone(), Action::Select(mode))
                            })
                            .child(when(
                                move || mode == "full_access",
                                move || icon(crate::icons::Icon::Warning, state, true),
                            ))
                            .child(text(move || {
                                state.with(|_| super::super::permission_mode_label(mode))
                            }))
                            .child(when(
                                move || state.get().mode == mode,
                                move || icon(crate::icons::Icon::Check, state, false),
                            ))
                    }))
            },
        ))
        .child(dynamic(move || {
            let commands = commands.clone();
            let (phase, error) = feedback.get();
            match phase {
                PermissionPhase::Saving(_) => {
                    text(move || state.with(|_| vega_i18n::t("settings.permission.saving")))
                        .into_element()
                }
                PermissionPhase::Preparing(_) | PermissionPhase::Reloading(_) => view()
                    .gap_2()
                    .child(text(move || {
                        state.with(|_| vega_i18n::t("settings.permission.reading"))
                    }))
                    .child(action_button(
                        actions,
                        commands,
                        Action::Cancel,
                        PermissionFocus::Cancel,
                        "settings.action.cancel",
                        focused,
                    ))
                    .into_element(),
                PermissionPhase::Idle => {
                    if let Some(error) = error {
                        view()
                            .gap_2()
                            .child(
                                text(move || state.with(|_| vega_i18n::t(error_key(error))))
                                    .text_color(move || {
                                        crate::nsh::color(state.get().colors.danger)
                                    }),
                            )
                            .child(action_button(
                                actions,
                                commands,
                                Action::Reload,
                                PermissionFocus::Reload,
                                "settings.permission.reload",
                                focused,
                            ))
                            .into_element()
                    } else {
                        view().into_element()
                    }
                }
            }
        }))
        .child(when(
            move || actions.failed.get(),
            move || {
                text(move || state.with(|_| vega_i18n::t("ui.command_failed")))
                    .text_color(move || crate::nsh::color(state.get().colors.danger))
            },
        ))
        .into_element()
}
