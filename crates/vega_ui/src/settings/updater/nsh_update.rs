use gpui_kit::{App, Context, Window};
use nsh_core::prelude::*;
use tokio::sync::{mpsc, oneshot};
use vega_conversation::types::{UpdatePhase, UpdateRequest};
use vega_theme::{Layout, ThemeColors, Typography};

use super::super::SettingsView;
use crate::nsh::{ProjectionId, Surface};

mod controls;
use controls::Control;

pub(in crate::settings) struct Presentation {
    surface: Surface<Snapshot, Request>,
    height: f32,
    pending: bool,
    help_target: super::super::nsh_control::FocusTarget,
    help_epoch: u64,
    help_focused: bool,
    focused_control: Option<Control>,
    keyboard_modality: bool,
    _focus_events: Vec<gpui_kit::Subscription>,
}

#[derive(Clone)]
struct Snapshot {
    id: ProjectionId,
    revision: u64,
    phase: UpdatePhase,
    automatic: bool,
    focused_control: Option<Control>,
    help_open: bool,
    help_epoch: u64,
    help_visible: bool,
    help_focused: bool,
    pending: bool,
    version: String,
    latest: Option<String>,
    checked: String,
    progress: Option<String>,
    message: Option<String>,
    notes: String,
    colors: ThemeColors,
}

const HELP_ID: &str = "settings-updater-help";

struct Decision {
    epoch: u64,
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
    Help(u64, bool),
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
    pub(in crate::settings) fn invalidate_help_state(&mut self) {
        if let Some(page) = &mut self.updater_presentation {
            page.help_epoch = page.help_epoch.wrapping_add(1);
        }
    }
    fn updater_help_visible(&self, cx: &App) -> bool {
        self.section == 1
            && !self.palette_open
            && self.pending_section.is_none()
            && !self.pending_settings_close
            && cx
                .try_global::<super::super::SettingsOpen>()
                .is_some_and(|open| open.0)
    }

    pub(in crate::settings) fn close_updater_help(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.settings_help_open.as_deref() != Some(HELP_ID) {
            return false;
        }
        self.settings_help_open = None;
        self.invalidate_help_state();
        if let Some(page) = &self.updater_presentation {
            page.surface.publish(self.update_snapshot(cx));
            window.focus(&page.help_target.handle, cx);
        }
        cx.notify();
        true
    }

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
            help_epoch: self
                .updater_presentation
                .as_ref()
                .map_or(0, |p| p.help_epoch),
            help_visible: self.updater_help_visible(cx),
            help_focused: self
                .updater_presentation
                .as_ref()
                .is_some_and(|p| p.help_focused && p.keyboard_modality),
            focused_control: self
                .updater_presentation
                .as_ref()
                .and_then(|p| p.focused_control.filter(|_| p.keyboard_modality)),
            help_open: self.settings_help_open.as_deref() == Some(HELP_ID),
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
        use gpui_kit::{InteractiveElement as _, IntoElement, ParentElement, Styled};
        if self.updater_presentation.is_none() {
            let (surface, mut requests) =
                Surface::new_transparent(self.update_snapshot(cx), render, window, cx);
            let target = super::super::nsh_control::FocusTarget::new(cx);
            let owner = cx.weak_entity();
            target.register(
                surface.entity().read(cx),
                HELP_ID,
                super::super::nsh_control::Semantics::Disclosure,
                move |stamp, cx| {
                    let Some(owner) = owner.upgrade() else {
                        return false;
                    };
                    let settings = owner.read(cx);
                    settings.updater_help_visible(cx)
                        && settings
                            .updater_presentation
                            .as_ref()
                            .is_some_and(|p| stamp.as_u64() == Some(p.help_epoch))
                },
            );
            let gained = cx.on_focus(&target.handle, window, |settings, _, cx| {
                if let Some(page) = &mut settings.updater_presentation {
                    page.help_focused = true;
                    cx.notify();
                }
            });
            let lost = cx.on_blur(&target.handle, window, |settings, _, cx| {
                if let Some(page) = &mut settings.updater_presentation {
                    page.help_focused = false;
                    cx.notify();
                }
            });
            let mut focus_events = vec![gained, lost];
            for control in Control::ALL {
                let handle = control.register(surface.entity().read(cx), cx);
                focus_events.push(cx.on_focus(&handle, window, move |settings, _, cx| {
                    if let Some(page) = &mut settings.updater_presentation {
                        page.focused_control = Some(control);
                        cx.notify();
                    }
                }));
                focus_events.push(cx.on_blur(&handle, window, move |settings, _, cx| {
                    if let Some(page) = &mut settings.updater_presentation
                        && page.focused_control == Some(control)
                    {
                        page.focused_control = None;
                        cx.notify();
                    }
                }));
            }
            self.updater_presentation = Some(Presentation {
                help_target: target,
                help_epoch: 0,
                help_focused: false,
                keyboard_modality: window.last_input_was_keyboard(),
                focused_control: None,
                _focus_events: focus_events,
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
                                    let accepted = this.updater_help_visible(cx)
                                        && this.updater_identity.matches(&command.id)
                                        && (matches!(command.action, UpdateRequest::ReleasePage)
                                            || (this.updater_revision == command.revision
                                                && this.updater.phase == command.phase
                                                && this.updater.automatic == command.automatic))
                                        && this.updater_presentation.as_ref().is_some_and(|page| {
                                            !page.pending && page.help_epoch == command.epoch
                                        })
                                        && allowed(
                                            &this.updater.phase,
                                            this.updater.automatic,
                                            command.action,
                                        );
                                    if accepted {
                                        if this.settings_help_open.as_deref() == Some(HELP_ID) {
                                            this.settings_help_open = None;
                                            this.invalidate_help_state();
                                            cx.notify();
                                        }
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
                                Request::Help(epoch, open)
                                    if this.updater_help_visible(cx)
                                        && this
                                            .updater_presentation
                                            .as_ref()
                                            .is_some_and(|p| p.help_epoch == epoch) =>
                                {
                                    this.invalidate_help_state();
                                    this.settings_help_open = open.then(|| HELP_ID.to_owned());
                                    cx.notify();
                                }
                                Request::Layout(height) if height.is_finite() && height > 0. => {
                                    if let Some(page) = &mut this.updater_presentation
                                        && (page.height - height).abs() > 0.5
                                    {
                                        page.height = height;
                                        cx.notify();
                                    }
                                }
                                Request::OpenLink(target)
                                    if this.section == 1
                                        && crate::conversation_stream::links::allowed_link(
                                            &target,
                                        ) =>
                                {
                                    crate::link_preview::open_target(&target, cx);
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
        if let Some(page) = &mut self.updater_presentation {
            page.keyboard_modality = window.last_input_was_keyboard();
        }
        let snapshot = self.update_snapshot(cx);
        let Some(page) = &mut self.updater_presentation else {
            return gpui_kit::div().into_any_element();
        };
        page.surface.sync_environment(cx);
        page.surface.publish(snapshot);
        gpui_kit::div()
            .on_mouse_down_out(cx.listener(|this, _: &gpui_kit::MouseDownEvent, _, cx| {
                if this.settings_help_open.as_deref() == Some(HELP_ID) {
                    this.invalidate_help_state();
                    this.settings_help_open = None;
                    cx.notify();
                }
            }))
            .w_full()
            .flex_shrink_0()
            .h(gpui_kit::px(page.height))
            .child(page.surface.entity())
            .into_any_element()
    }
}

#[derive(Clone, Copy)]
struct Actions {
    owner: nsh_core::element::Owner,
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
        self.owner.run(|| {
            spawn(async move {
                let (reply, response) = oneshot::channel();
                if commands
                    .send(Request::Decide(Decision {
                        epoch: snapshot.help_epoch,
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
        });
    }
}

fn focus_ring(state: Signal<Snapshot>, quiet: bool) -> nsh_core::element::View {
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
            } else if quiet {
                state.get().colors.bg_base.opacity(0.)
            } else {
                state.get().colors.border_subtle
            })
        })
        .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
}

fn action_button(
    actions: Actions,
    commands: mpsc::Sender<Request>,
    control: Control,
) -> AnyElement {
    let state = actions.state;
    let pointer = controls::Pointer::new(control);
    let enabled = move || !actions.waiting.get() && state.with(|s| control.enabled(s));
    view()
        .relative()
        .self_start()
        .accessible(false)
        .min_h(px(Layout::SETTINGS_CONTROL_MIN_HEIGHT))
        .px(px(Layout::SETTINGS_CONTROL_PADDING_X))
        .py_1()
        .items_center()
        .justify_center()
        .border(px(1.))
        .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
        .border_color(move || {
            state.with(|s| {
                crate::nsh::color(if s.focused_control == Some(control) && enabled() {
                    s.colors.brand_primary
                } else {
                    s.colors.border_subtle
                })
            })
        })
        .bg(move || {
            state.with(|s| {
                let hovered = enabled() && pointer.hovered(s);
                crate::nsh::color(match (control, hovered) {
                    (Control::Install, true) => s.colors.brand_primary_strong,
                    (Control::Install, false) => s.colors.brand_primary,
                    (_, _) if enabled() && pointer.pressed(s) => s.colors.bg_active,
                    (_, true) => s.colors.bg_hover,
                    (_, false) => s.colors.bg_elevated,
                })
            })
        })
        .text_color(move || {
            state.with(|s| {
                crate::nsh::color(if !enabled() {
                    s.colors.text_tertiary
                } else if control == Control::Install {
                    s.colors.brand_on_accent
                } else {
                    s.colors.text_primary
                })
            })
        })
        .line_height(px((Typography::BODY * Typography::BODY_LINE_HEIGHT).round()))
        .child(text(move || state.with(|_| vega_i18n::t(control.label()))).accessible(false))
        .child(control.target(actions, commands, pointer))
        .into_element()
}

fn update_help(state: Signal<Snapshot>, commands: mpsc::Sender<Request>) -> AnyElement {
    let activate = commands.clone();
    let hover = signal(None::<u64>);
    let trigger = view()
        .relative()
        .accessible(false)
        .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
        .size(px(Layout::SETTINGS_CONTROL_MIN_HEIGHT))
        .items_center()
        .justify_center()
        .bg(move || {
            let colors = state.get().colors;
            crate::nsh::color(if state.get().help_open || state.get().help_focused {
                colors.bg_active
            } else if hover.get() == Some(state.get().help_epoch) {
                colors.bg_hover
            } else {
                colors.bg_base.opacity(0.)
            })
        })
        .child(
            svg(nsh_core::element::ImageSrc::svg_markup(
                &crate::icons::nsh_source(crate::icons::Icon::Help).unwrap_or_default(),
            ))
            .accessible(false)
            .size(px(Layout::ICON_SIZE))
            .text_color(move || crate::nsh::color(state.get().colors.text_secondary))
            .tint_color(move || crate::nsh::color(state.get().colors.text_secondary).to_hsla()),
        )
        .child(
            native(HELP_ID)
                .absolute()
                .inset_0()
                .prop("report-hover", serde_json::Value::Bool(true))
                .prop("tooltip", move || {
                    serde_json::Value::Bool(!state.get().help_open)
                })
                .prop("state", move || {
                    serde_json::Value::from(state.get().help_epoch)
                })
                .prop("enabled", move || {
                    serde_json::Value::from(state.get().help_visible)
                })
                .prop("selected", move || {
                    serde_json::Value::from(state.get().help_open)
                })
                .prop("label", move || {
                    serde_json::Value::from(
                        state.with(|_| vega_i18n::t("settings.updater.help_label")),
                    )
                })
                .prop("hint", move || {
                    serde_json::Value::from(state.with(|_| vega_i18n::t("settings.updater.help")))
                })
                .on_native_event(move |event: &NativeEvent| {
                    let current = state.get_untracked();
                    if &*event.name == "hover"
                        && current.help_visible
                        && event.data.get(0).and_then(serde_json::Value::as_u64)
                            == Some(current.help_epoch)
                    {
                        hover.set(
                            (event.data.get(1).and_then(serde_json::Value::as_bool) == Some(true))
                                .then_some(current.help_epoch),
                        );
                    }
                    if &*event.name == "activate"
                        && current.help_visible
                        && event.data.as_u64() == Some(current.help_epoch)
                    {
                        let commands = activate.clone();
                        spawn(async move {
                            let _ = commands
                                .send(Request::Help(current.help_epoch, !current.help_open))
                                .await;
                        });
                    }
                }),
        );
    nsh_base::popover()
        .open(move || Some(state.get().help_visible && state.get().help_open))
        .align(nsh_core::overlay::Align::End)
        .on_open_change(move |event: &nsh_base::PopoverEvent| {
            if let nsh_base::PopoverEvent::OpenChange(value, _) = event {
                let commands = commands.clone();
                let request = Request::Help(state.get_untracked().help_epoch, *value);
                spawn(async move {
                    let _ = commands.send(request).await;
                });
            }
        })
        .trigger(trigger)
        .child(
            view()
                .w(px(Layout::SETTINGS_LABEL_WIDTH))
                .max_w(relative(1.))
                .p_3()
                .rounded(px(Layout::MENU_RADIUS))
                .border(px(1.))
                .border_color(move || crate::nsh::color(state.get().colors.border_subtle))
                .bg(move || crate::nsh::color(state.get().colors.bg_elevated))
                .text_color(move || crate::nsh::color(state.get().colors.text_secondary))
                .child(text(move || {
                    state.with(|_| vega_i18n::t("settings.updater.help"))
                })),
        )
        .into_element()
}

fn render(state: Signal<Snapshot>, commands: mpsc::Sender<Request>) -> AnyElement {
    let actions = Actions {
        owner: nsh_core::element::Owner::current(),
        state,
        waiting: signal(false),
        failed: signal(false),
    };
    let notes_open = signal(false);
    let measured = signal(0.0f32);
    let layout = commands.clone();
    let automatic = commands.clone();
    let switch_pointer = controls::Pointer::new(Control::Automatic);
    let buttons = commands.clone();
    let help_commands = commands.clone();
    let links = commands;
    view()
        .w_full()
        .flex_shrink_0()
        .gap_2()
        .text_color(move || crate::nsh::color(state.get().colors.text_primary))
        .text_size(px(Typography::BODY))
        .line_height(px(Typography::BODY * Typography::BODY_LINE_HEIGHT))
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
                        .text_size(px(Typography::HEADING_BLOCK))
                        .line_height(px(Typography::HEADING_BLOCK * Typography::BODY_LINE_HEIGHT))
                        .font_weight(FontWeight(Typography::HEADING_BLOCK_WEIGHT.0 as u16)),
                )
                .child(update_help(state, help_commands)),
        )
        .child(
            super::super::nsh_form::card(move || state.get().colors)
                .child(super::super::nsh_form::preference(
                    move || state.get().colors,
                    text(move || state.with(|_| vega_i18n::t("settings.updater.version"))),
                    Some(text(move || state.get().checked).into_element()),
                    view()
                        .gap_1()
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
                                .text_color(move || {
                                    crate::nsh::color(state.get().colors.text_secondary)
                                })
                            },
                        )),
                ))
                .child(super::super::nsh_form::preference(
                    move || state.get().colors,
                    text(move || state.with(|_| vega_i18n::t("settings.updater.automatic"))),
                    None,
                    view()
                        .relative()
                        .self_start()
                        .accessible(false)
                        .w(px(Layout::SETTINGS_SWITCH_WIDTH))
                        .h(px(Layout::SETTINGS_SWITCH_HEIGHT))
                        .rounded(px(Layout::SETTINGS_SWITCH_HEIGHT / 2.))
                        .border(px(1.))
                        .border_color(move || {
                            state.with(|s| {
                                crate::nsh::color(
                                    if s.focused_control == Some(Control::Automatic)
                                        && Control::Automatic.enabled(s)
                                    {
                                        s.colors.text_primary
                                    } else if s.automatic {
                                        s.colors.brand_primary
                                    } else {
                                        s.colors.border_subtle
                                    },
                                )
                            })
                        })
                        .bg(move || {
                            state.with(|s| {
                                let hovered = Control::Automatic.enabled(s)
                                    && !actions.waiting.get()
                                    && switch_pointer.hovered(s);
                                crate::nsh::color(match (s.automatic, hovered) {
                                    (true, true) => s.colors.brand_primary_strong,
                                    (true, false) => s.colors.brand_primary,
                                    (false, _) if hovered && switch_pointer.pressed(s) => {
                                        s.colors.bg_active
                                    }
                                    (false, true) => s.colors.border_subtle,
                                    (false, false) => s.colors.bg_hover,
                                })
                            })
                        })
                        .opacity(move || -> f32 {
                            if actions.waiting.get() || state.with(|s| s.pending || busy(&s.phase))
                            {
                                0.5
                            } else {
                                1.
                            }
                        })
                        .child(
                            view()
                                .absolute()
                                .top(px(1.))
                                .left(move || {
                                    px(if state.get().automatic {
                                        Layout::SETTINGS_SWITCH_WIDTH - Layout::ICON_SIZE - 3.
                                    } else {
                                        1.
                                    })
                                })
                                .size(px(Layout::ICON_SIZE))
                                .rounded(px(Layout::ICON_SIZE / 2.))
                                .bg(move || crate::nsh::color(state.get().colors.bg_elevated)),
                        )
                        .child(Control::Automatic.target(actions, automatic, switch_pointer)),
                ))
                .child(when(
                    move || {
                        state.get().message.as_ref().is_some_and(|message| {
                            message != vega_i18n::t("settings.updater.verification_notice")
                        })
                    },
                    move || {
                        view().mt_3().child(super::super::nsh_form::status(
                            move || state.get().colors,
                            move || {
                                state.with(|s| match s.phase {
                                    UpdatePhase::Error => s.colors.danger,
                                    UpdatePhase::Ready => s.colors.success,
                                    _ => s.colors.text_secondary,
                                })
                            },
                            text(move || state.get().message.unwrap_or_default()),
                        ))
                    },
                ))
                .child(when(
                    move || state.get().progress.is_some(),
                    move || {
                        view()
                            .mt_2()
                            .text_color(move || {
                                crate::nsh::color(state.get().colors.text_secondary)
                            })
                            .child(text(move || state.get().progress.unwrap_or_default()))
                    },
                ))
                .child(
                    view()
                        .mt_3()
                        .flex_row()
                        .flex_wrap()
                        .gap_2()
                        .child(action_button(actions, buttons.clone(), Control::Check))
                        .child(action_button(
                            actions,
                            buttons.clone(),
                            Control::ReleasePage,
                        ))
                        .child(when(move || state.get().phase == UpdatePhase::Ready, {
                            let buttons = buttons.clone();
                            move || action_button(actions, buttons.clone(), Control::Later)
                        }))
                        .child(when(
                            move || state.get().phase == UpdatePhase::Ready,
                            move || action_button(actions, buttons.clone(), Control::Install),
                        )),
                )
                .child(when(
                    move || !state.get().notes.is_empty(),
                    move || {
                        let links = links.clone();
                        view()
                            .gap_2()
                            .child(
                                focus_ring(state, false)
                                    .px_3()
                                    .py_2()
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
                                        state.with(|_| {
                                            vega_i18n::t("settings.updater.release_notes")
                                        })
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
                                        .markdown(move || Some(state.get().notes.into()))
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
                )),
        )
        .into_element()
}
