use gpui_kit::{App, Context, Window};
use nsh_core::prelude::*;
use tokio::sync::{mpsc, oneshot};
use vega_conversation::types::{UpdatePhase, UpdateRequest};
use vega_theme::{Layout, ThemeColors, Typography};

use super::super::SettingsView;
use crate::nsh::{ProjectionId, Surface};

pub(in crate::settings) struct Presentation {
    surface: Surface<Snapshot, Request>,
    height: f32,
    pending: bool,
}

#[derive(Clone)]
struct Snapshot {
    id: ProjectionId,
    revision: u64,
    phase: UpdatePhase,
    automatic: bool,
    pending: bool,
    version: String,
    latest: Option<String>,
    checked: String,
    progress: Option<String>,
    message: Option<String>,
    notes: String,
    colors: ThemeColors,
}

struct Decision {
    id: ProjectionId,
    revision: u64,
    phase: UpdatePhase,
    automatic: bool,
    action: UpdateRequest,
    reply: oneshot::Sender<bool>,
}

enum Request {
    Decide(Decision),
    Layout(f32),
    OpenLink(String),
}

fn busy(phase: &UpdatePhase) -> bool {
    matches!(
        phase,
        UpdatePhase::Loading
            | UpdatePhase::Checking
            | UpdatePhase::Downloading { .. }
            | UpdatePhase::Installing
    )
}

fn allowed(phase: &UpdatePhase, automatic: bool, action: UpdateRequest) -> bool {
    match action {
        UpdateRequest::Check => !busy(phase) && *phase != UpdatePhase::Ready,
        UpdateRequest::Automatic(next) => !busy(phase) && automatic != next,
        UpdateRequest::Install | UpdateRequest::Later => *phase == UpdatePhase::Ready,
        UpdateRequest::ReleasePage => true,
    }
}

impl SettingsView {
    pub(in crate::settings) fn publish_updater(&mut self, cx: &App) {
        if let Some(presentation) = &mut self.updater_presentation {
            presentation.pending = false;
        }
        if let Some(presentation) = &self.updater_presentation {
            presentation.surface.publish(self.update_snapshot(cx));
        }
    }

    fn update_snapshot(&self, cx: &App) -> Snapshot {
        let checked = self
            .updater
            .last_checked
            .map(|last| {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|time| time.as_secs())
                    .unwrap_or_default();
                let minutes = now.saturating_sub(last) / 60;
                if minutes == 0 {
                    vega_i18n::t("settings.updater.checked_now").into()
                } else if minutes < 60 {
                    vega_i18n::t_args(
                        "settings.updater.checked_minutes",
                        &[("minutes", &minutes.to_string())],
                    )
                } else {
                    vega_i18n::t_args(
                        "settings.updater.checked_hours",
                        &[("hours", &(minutes / 60).to_string())],
                    )
                }
            })
            .unwrap_or_else(|| vega_i18n::t("settings.updater.never_checked").into());
        let progress = match self.updater.phase {
            UpdatePhase::Downloading { received, total } if total > 0 => Some(vega_i18n::t_args(
                "settings.updater.downloaded",
                &[(
                    "percent",
                    &(received.saturating_mul(100) / total).to_string(),
                )],
            )),
            _ => None,
        };
        Snapshot {
            id: self.updater_identity.clone(),
            revision: self.updater_revision,
            phase: self.updater.phase.clone(),
            automatic: self.updater.automatic,
            pending: self
                .updater_presentation
                .as_ref()
                .is_some_and(|page| page.pending),
            version: self.updater_version.as_ref().map_or_else(
                || self.updater.current_version_label().to_owned(),
                super::super::SettingsText::render,
            ),
            latest: self.updater.latest_version.clone(),
            checked,
            progress,
            message: self
                .updater_message
                .as_ref()
                .map(super::super::SettingsText::render),
            notes: self.updater.notes.clone(),
            colors: vega_theme::theme(cx).colors,
        }
    }

    pub(crate) fn render_updater(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        use gpui_kit::{IntoElement, ParentElement, Styled};
        if self.updater_presentation.is_none() {
            let (surface, mut requests) =
                Surface::new(self.update_snapshot(cx), render, window, cx);
            self.updater_presentation = Some(Presentation {
                surface,
                height: Layout::SETTINGS_NOTES_MAX_HEIGHT,
                pending: false,
            });
            cx.spawn_in(window, async move |this, cx| {
                while let Some(request) = requests.recv().await {
                    if this
                        .update_in(cx, |this, _, cx| {
                            match request {
                                Request::Decide(command) => {
                                    let accepted = this.section == 1
                                        && this.updater_identity.matches(&command.id)
                                        && this.updater_revision == command.revision
                                        && this.updater.phase == command.phase
                                        && this.updater.automatic == command.automatic
                                        && this
                                            .updater_presentation
                                            .as_ref()
                                            .is_some_and(|page| !page.pending)
                                        && allowed(
                                            &this.updater.phase,
                                            this.updater.automatic,
                                            command.action,
                                        );
                                    if accepted {
                                        if matches!(
                                            command.action,
                                            UpdateRequest::Check
                                                | UpdateRequest::Automatic(_)
                                                | UpdateRequest::Install
                                        ) && let Some(page) = &mut this.updater_presentation
                                        {
                                            page.pending = true;
                                        }
                                        cx.emit(command.action);
                                    }
                                    let _ = command.reply.send(accepted);
                                }
                                Request::Layout(height) if height.is_finite() && height > 0. => {
                                    if let Some(page) = &mut this.updater_presentation
                                        && (page.height - height).abs() > 0.5
                                    {
                                        page.height = height;
                                        cx.notify();
                                    }
                                }
                                Request::OpenLink(target) if this.section == 1 => {
                                    if crate::conversation_stream::links::allowed_link(&target) {
                                        crate::link_preview::open_target(&target, cx);
                                    }
                                }
                                _ => {}
                            }
                            if let Some(page) = &this.updater_presentation {
                                page.surface.publish(this.update_snapshot(cx));
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
        let snapshot = self.update_snapshot(cx);
        let Some(page) = &mut self.updater_presentation else {
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
    fn send(self, commands: mpsc::Sender<Request>, action: UpdateRequest) {
        let snapshot = self.state.get_untracked();
        if self.waiting.get_untracked()
            || snapshot.pending
            || !allowed(&snapshot.phase, snapshot.automatic, action)
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
                    phase: snapshot.phase,
                    automatic: snapshot.automatic,
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

fn control(state: Signal<Snapshot>) -> nsh_core::element::View {
    let target = element_ref();
    let focused = signal(false);
    target.on_focus_in(move || focused.set(true));
    target.on_focus_out(move || focused.set(false));
    view()
        .element_ref(&target)
        .border(px(1.))
        .border_color(move || {
            crate::nsh::color(if focused.get() {
                state.get().colors.accent
            } else {
                state.get().colors.border_subtle
            })
        })
        .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
        .px_3()
        .py_2()
}

fn action_button(
    actions: Actions,
    commands: mpsc::Sender<Request>,
    key: &'static str,
    action: UpdateRequest,
) -> AnyElement {
    control(actions.state)
        .role(AccessibilityRole::Button)
        .label(move || actions.state.with(|_| vega_i18n::t(key)))
        .disabled(move || {
            actions.waiting.get()
                || actions
                    .state
                    .with(|s| s.pending || !allowed(&s.phase, s.automatic, action))
        })
        .px_3()
        .py_2()
        .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
        .bg(move || crate::nsh::color(actions.state.get().colors.bg_elevated))
        .on_press(move |_: &nsh_core::PressEvent| actions.send(commands.clone(), action))
        .child(text(move || actions.state.with(|_| vega_i18n::t(key))))
        .into_element()
}

fn render(state: Signal<Snapshot>, commands: mpsc::Sender<Request>) -> AnyElement {
    let actions = Actions {
        state,
        waiting: signal(false),
        failed: signal(false),
    };
    let help = signal(false);
    let notes_open = signal(false);
    let measured = signal(0.0f32);
    let layout = commands.clone();
    let automatic = commands.clone();
    let buttons = commands.clone();
    let links = commands;
    view()
        .w_full()
        .flex_shrink_0()
        .gap_3()
        .p(px(Layout::SETTINGS_GROUP_PADDING))
        .rounded(px(Layout::PANEL_RADIUS))
        .border(px(1.))
        .border_color(move || crate::nsh::color(state.get().colors.border_subtle))
        .bg(move || crate::nsh::color(state.get().colors.bg_subtle))
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
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    text(move || state.with(|_| vega_i18n::t("settings.updater.title")))
                        .text_size(px(Typography::HEADING_CARD)),
                )
                .child(
                    control(state)
                        .role(AccessibilityRole::Button)
                        .label(move || state.with(|_| vega_i18n::t("settings.updater.help_label")))
                        .accessibility_state(move || nsh_core::AccessibilityState {
                            expanded: Some(help.get()),
                            ..Default::default()
                        })
                        .on_press(move |_: &nsh_core::PressEvent| {
                            help.update(|value| *value = !*value)
                        })
                        .child(text(move || {
                            state.with(|_| vega_i18n::t("settings.updater.help_label"))
                        })),
                ),
        )
        .child(when(
            move || help.get(),
            move || text(move || state.with(|_| vega_i18n::t("settings.updater.help"))),
        ))
        .child(text(move || {
            vega_i18n::t_args(
                "settings.updater.current_version",
                &[("version", &state.get().version)],
            )
        }))
        .child(when(
            move || state.get().latest.is_some(),
            move || {
                text(move || {
                    vega_i18n::t_args(
                        "settings.updater.latest_version",
                        &[("version", &state.get().latest.unwrap_or_default())],
                    )
                })
            },
        ))
        .child(
            text(move || state.get().checked)
                .text_color(move || crate::nsh::color(state.get().colors.text_secondary)),
        )
        .child(
            control(state)
                .flex_row()
                .items_center()
                .gap_2()
                .role(AccessibilityRole::Switch)
                .label(move || state.with(|_| vega_i18n::t("settings.updater.automatic")))
                .accessibility_state(move || nsh_core::AccessibilityState {
                    checked: Some(if state.get().automatic {
                        nsh_core::Checked::True
                    } else {
                        nsh_core::Checked::False
                    }),
                    ..Default::default()
                })
                .disabled(move || {
                    actions.waiting.get() || state.with(|s| s.pending || busy(&s.phase))
                })
                .bg(move || {
                    crate::nsh::color(if state.get().automatic {
                        state.get().colors.bg_active
                    } else {
                        state.get().colors.bg_elevated
                    })
                })
                .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
                .px_3()
                .py_2()
                .on_press(move |_: &nsh_core::PressEvent| {
                    actions.send(
                        automatic.clone(),
                        UpdateRequest::Automatic(!state.get_untracked().automatic),
                    )
                })
                .child(
                    view()
                        .w(px(Layout::SETTINGS_SWITCH_WIDTH))
                        .h(px(Layout::SETTINGS_SWITCH_HEIGHT))
                        .relative()
                        .rounded(px(Layout::SETTINGS_SWITCH_HEIGHT / 2.))
                        .bg(move || {
                            crate::nsh::color(if state.get().automatic {
                                state.get().colors.brand_primary
                            } else {
                                state.get().colors.bg_hover
                            })
                        })
                        .child(
                            view()
                                .absolute()
                                .top(px(2.))
                                .left(move || {
                                    px(if state.get().automatic {
                                        Layout::SETTINGS_SWITCH_WIDTH - Layout::ICON_SIZE - 2.
                                    } else {
                                        2.
                                    })
                                })
                                .size(px(Layout::ICON_SIZE))
                                .rounded(px(Layout::ICON_SIZE / 2.))
                                .bg(move || crate::nsh::color(state.get().colors.bg_elevated)),
                        ),
                )
                .child(text(move || {
                    state.with(|_| vega_i18n::t("settings.updater.automatic"))
                })),
        )
        .child(when(
            move || {
                state.get().message.as_ref().is_some_and(|message| {
                    message != vega_i18n::t("settings.updater.verification_notice")
                })
            },
            move || {
                text(move || state.get().message.unwrap_or_default()).text_color(move || {
                    crate::nsh::color(if state.get().phase == UpdatePhase::Error {
                        state.get().colors.danger
                    } else {
                        state.get().colors.text_secondary
                    })
                })
            },
        ))
        .child(when(
            move || state.get().progress.is_some(),
            move || text(move || state.get().progress.unwrap_or_default()),
        ))
        .child(
            view()
                .flex_row()
                .flex_wrap()
                .gap_2()
                .child(action_button(
                    actions,
                    buttons.clone(),
                    "settings.updater.check",
                    UpdateRequest::Check,
                ))
                .child(action_button(
                    actions,
                    buttons.clone(),
                    "settings.updater.release_page",
                    UpdateRequest::ReleasePage,
                ))
                .child(when(
                    move || state.get().phase == UpdatePhase::Ready,
                    move || {
                        view()
                            .flex_row()
                            .gap_2()
                            .child(action_button(
                                actions,
                                buttons.clone(),
                                "settings.updater.later",
                                UpdateRequest::Later,
                            ))
                            .child(action_button(
                                actions,
                                buttons.clone(),
                                "settings.updater.install",
                                UpdateRequest::Install,
                            ))
                    },
                )),
        )
        .child(when(
            move || !state.get().notes.is_empty(),
            move || {
                let links = links.clone();
                view()
                    .gap_2()
                    .child(
                        control(state)
                            .role(AccessibilityRole::Button)
                            .label(move || {
                                state.with(|_| {
                                    vega_i18n::t(if notes_open.get() {
                                        "settings.updater.hide_notes"
                                    } else {
                                        "settings.updater.show_notes"
                                    })
                                })
                            })
                            .accessibility_state(move || nsh_core::AccessibilityState {
                                expanded: Some(notes_open.get()),
                                ..Default::default()
                            })
                            .on_press(move |_: &nsh_core::PressEvent| {
                                notes_open.update(|open| *open = !*open)
                            })
                            .child(text(move || {
                                state.with(|_| vega_i18n::t("settings.updater.release_notes"))
                            })),
                    )
                    .child(when(
                        move || notes_open.get(),
                        move || {
                            let links = links.clone();
                            scroll()
                                .w_full()
                                .max_h(px(Layout::SETTINGS_NOTES_MAX_HEIGHT))
                                .child(
                                    nsh_base::text_view()
                                        .markdown(move || state.get().notes)
                                        .selectable(true)
                                        .render_image(|image: &nsh_base::text_view::Image| {
                                            text(image.alt.clone())
                                        })
                                        .on_link_press(
                                            move |event: &nsh_base::TextViewLinkPressEvent| {
                                                if crate::conversation_stream::links::allowed_link(
                                                    &event.url,
                                                ) {
                                                    let commands = links.clone();
                                                    let target = event.url.to_string();
                                                    spawn(async move {
                                                        if commands
                                                            .send(Request::OpenLink(target))
                                                            .await
                                                            .is_err()
                                                        {
                                                            actions.failed.set(true);
                                                        }
                                                    });
                                                }
                                            },
                                        ),
                                )
                        },
                    ))
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

#[cfg(test)]
mod tests {
    use super::allowed;
    use vega_conversation::types::{UpdatePhase, UpdateRequest};

    #[test]
    fn install_and_later_require_verified_ready_state() {
        for phase in [
            UpdatePhase::Loading,
            UpdatePhase::Idle,
            UpdatePhase::Checking,
            UpdatePhase::Available,
            UpdatePhase::Downloading {
                received: 10,
                total: 20,
            },
            UpdatePhase::Installing,
            UpdatePhase::Error,
        ] {
            assert!(!allowed(&phase, true, UpdateRequest::Install));
            assert!(!allowed(&phase, true, UpdateRequest::Later));
        }
        assert!(allowed(&UpdatePhase::Ready, true, UpdateRequest::Install));
        assert!(allowed(&UpdatePhase::Ready, true, UpdateRequest::Later));
        assert!(!allowed(&UpdatePhase::Ready, true, UpdateRequest::Check));
    }

    #[test]
    fn busy_updates_reject_preferences_and_checks_but_keep_release_link_available() {
        for phase in [
            UpdatePhase::Loading,
            UpdatePhase::Checking,
            UpdatePhase::Downloading {
                received: 2,
                total: 10,
            },
            UpdatePhase::Installing,
        ] {
            assert!(!allowed(&phase, true, UpdateRequest::Automatic(false)));
            assert!(!allowed(&phase, false, UpdateRequest::Automatic(true)));
            assert!(!allowed(&phase, true, UpdateRequest::Check));
            assert!(allowed(&phase, true, UpdateRequest::ReleasePage));
        }
        assert!(!allowed(
            &UpdatePhase::Idle,
            true,
            UpdateRequest::Automatic(true)
        ));
        assert!(allowed(
            &UpdatePhase::Idle,
            true,
            UpdateRequest::Automatic(false)
        ));
        assert!(allowed(&UpdatePhase::Error, false, UpdateRequest::Check));
    }
}
