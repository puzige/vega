use std::sync::{Arc, Mutex};

use gpui_kit::{App, Context, Window};
use nsh_core::Checked;
use nsh_core::prelude::*;
use tokio::sync::{mpsc, oneshot};
use vega_store::config::CodexAcpProfileConfig;
use vega_theme::{Layout, ThemeColors, Typography};

use super::{NativeAgentAction, SettingsView};
use crate::nsh::{ProjectionId, Surface};

#[derive(Clone, PartialEq, Eq)]
struct Values {
    text: [String; 3],
    composing: [bool; 3],
    revision: u64,
    frozen: bool,
}

#[derive(Clone)]
pub(in crate::settings) struct Draft(Arc<Mutex<Values>>);

impl Draft {
    fn new(text: [String; 3]) -> Self {
        Self(Arc::new(Mutex::new(Values {
            text,
            composing: [false; 3],
            revision: 0,
            frozen: false,
        })))
    }

    fn read(&self) -> Values {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    fn edit(&self, index: usize, text: Option<&str>, composing: Option<bool>) -> bool {
        let mut values = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if values.frozen || index >= values.text.len() {
            return false;
        }
        if let Some(text) = text {
            values.text[index] = text.into();
        }
        if let Some(composing) = composing {
            values.composing[index] = composing;
        }
        values.revision = values.revision.wrapping_add(1);
        true
    }

    fn claim(&self, observed: &Values) -> Option<[String; 3]> {
        let mut values = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if *values != *observed || values.frozen || values.composing.contains(&true) {
            return None;
        }
        values.frozen = true;
        Some(values.text.clone())
    }

    fn release(&self) {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .frozen = false;
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Authority {
    desktop: bool,
    browser: bool,
    profile: Option<CodexAcpProfileConfig>,
    saving: bool,
    profile_saving: bool,
    component: &'static str,
    accessibility: &'static str,
    screen_recording: &'static str,
    can_recheck: bool,
    check_state: crate::cua_onboarding::controller::CheckState,
    retained: bool,
    feedback: Vec<(String, bool)>,
}

#[derive(Clone)]
struct Snapshot {
    id: ProjectionId,
    visible: bool,
    authority: Authority,
    draft: Draft,
    observed: Values,
    colors: ThemeColors,
}

#[derive(Clone, Copy)]
enum Action {
    Native(NativeAgentAction),
    Save,
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

pub(in crate::settings) struct Presentation {
    surface: Surface<Snapshot, Request>,
    id: ProjectionId,
    height: f32,
}

fn allowed(snapshot: &Snapshot, action: Action) -> bool {
    match action {
        Action::Save => {
            !snapshot.authority.saving
                && !snapshot.authority.profile_saving
                && !snapshot.observed.frozen
                && !snapshot.observed.composing.contains(&true)
        }
        Action::Native(NativeAgentAction::Desktop | NativeAgentAction::Browser) => {
            !snapshot.authority.saving && !snapshot.authority.profile_saving
        }
        Action::Native(NativeAgentAction::Check) => snapshot.authority.can_recheck,
        Action::Native(_) => true,
    }
}

fn accepts(current: &Snapshot, observed: &Snapshot, action: Action) -> bool {
    current.id.matches(&observed.id)
        && current.visible
        && current.authority == observed.authority
        && Arc::ptr_eq(&current.draft.0, &observed.draft.0)
        && current.observed == observed.observed
        && allowed(current, action)
}

impl SettingsView {
    pub(in crate::settings) fn release_agent_draft(&self) {
        if let Some(draft) = &self.agents_nsh_draft {
            draft.release();
        }
    }

    fn agents_snapshot(&self, id: ProjectionId, cx: &App) -> Snapshot {
        let preparation = self.preparation.read(cx).snapshot();
        let view = &preparation.view;
        let permission = |state| match state {
            vega_conversation::types::PermissionState::Granted => "settings.state.on",
            vega_conversation::types::PermissionState::Denied => "settings.state.denied",
            vega_conversation::types::PermissionState::Unknown
                if view.check_state == crate::cua_onboarding::controller::CheckState::Idle
                    && !preparation.retained =>
            {
                "settings.state.unchecked"
            }
            _ => "settings.state.unknown",
        };
        let component = match view.component {
            Some(vega_conversation::types::ComponentState::Available) => "settings.state.available",
            Some(vega_conversation::types::ComponentState::Missing) => {
                "settings.state.not_installed"
            }
            Some(vega_conversation::types::ComponentState::Incompatible) => {
                "settings.state.incompatible"
            }
            Some(vega_conversation::types::ComponentState::UnsupportedPlatform) => {
                "settings.state.unsupported_platform"
            }
            None if view.check_state == crate::cua_onboarding::controller::CheckState::Idle
                && !preparation.retained =>
            {
                "settings.state.unchecked"
            }
            None => "settings.state.unknown",
        };
        let draft = self.agents_nsh_draft.clone().unwrap_or_else(|| {
            Draft::new([
                self.codex_name_input.read(cx).text().into(),
                self.codex_executable_input.read(cx).text().into(),
                self.codex_args_input.read(cx).text().into(),
            ])
        });
        Snapshot {
            id,
            visible: self.section == 7
                && cx
                    .try_global::<crate::settings::SettingsOpen>()
                    .is_some_and(|open| open.0),
            authority: Authority {
                desktop: self.config.agent.desktop_use_enabled,
                browser: self.config.agent.browser_use_enabled,
                profile: self.config.agent.codex_acp_profile.clone(),
                saving: self.provider_is_saving() || self.default_permission.io_pending(),
                profile_saving: self.codex_profile_saving,
                component,
                accessibility: permission(view.accessibility),
                screen_recording: permission(view.screen_recording),
                can_recheck: view.can_recheck,
                check_state: view.check_state,
                retained: preparation.retained,
                feedback: preparation
                    .feedback
                    .into_iter()
                    .map(|feedback| (feedback.message, feedback.warning))
                    .collect(),
            },
            observed: draft.read(),
            draft,
            colors: vega_theme::theme(cx).colors,
        }
    }

    pub(in crate::settings) fn sync_agents_presentation(&self, cx: &App) {
        if let Some(page) = &self.agents_presentation {
            page.surface
                .publish(self.agents_snapshot(page.id.clone(), cx));
        }
    }

    pub(crate) fn render_agents(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        use gpui_kit::{IntoElement, ParentElement, Styled};
        if self.agents_nsh_draft.is_none() {
            self.agents_nsh_draft = Some(Draft::new([
                self.codex_name_input.read(cx).text().into(),
                self.codex_executable_input.read(cx).text().into(),
                self.codex_args_input.read(cx).text().into(),
            ]));
        }
        if self.agents_presentation.is_none() {
            let id = ProjectionId::new();
            let (surface, mut requests) =
                Surface::new_transparent(self.agents_snapshot(id.clone(), cx), render, window, cx);
            self.agents_presentation = Some(Presentation {
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
                                    let mut accepted =
                                        this.agents_presentation.as_ref().is_some_and(|page| {
                                            accepts(
                                                &this.agents_snapshot(page.id.clone(), cx),
                                                &command.observed,
                                                command.action,
                                            )
                                        });
                                    if accepted {
                                        match command.action {
                                            Action::Native(action) => {
                                                this.native_agent_action(action, cx)
                                            }
                                            Action::Save => {
                                                if let Some(values) = command
                                                    .observed
                                                    .draft
                                                    .claim(&command.observed.observed)
                                                {
                                                    this.codex_name_input
                                                        .update(cx, |input, cx| {
                                                            input.set_text(&values[0], cx)
                                                        });
                                                    this.codex_executable_input
                                                        .update(cx, |input, cx| {
                                                            input.set_text(&values[1], cx)
                                                        });
                                                    this.codex_args_input
                                                        .update(cx, |input, cx| {
                                                            input.set_text(&values[2], cx)
                                                        });
                                                    this.save_codex_profile(cx);
                                                    if !this.codex_profile_saving {
                                                        this.release_agent_draft();
                                                    }
                                                } else {
                                                    accepted = false;
                                                }
                                            }
                                        }
                                        cx.notify();
                                    }
                                    let _ = command.reply.send(accepted);
                                }
                                Request::Layout(height) if height.is_finite() && height > 0. => {
                                    if let Some(page) = &mut this.agents_presentation
                                        && (height - page.height).abs() > 0.5
                                    {
                                        page.height = height;
                                        cx.notify();
                                    }
                                }
                                Request::Layout(_) => {}
                            }
                            this.sync_agents_presentation(cx);
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();
        }
        let Some(page) = &self.agents_presentation else {
            return gpui_kit::div().into_any_element();
        };
        let snapshot = self.agents_snapshot(page.id.clone(), cx);
        let Some(page) = &mut self.agents_presentation else {
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
    pending: Signal<bool>,
    failed: Signal<bool>,
    edits: Signal<u64>,
}
impl Actions {
    fn snapshot(self) -> Snapshot {
        self.edits.get();
        let mut snapshot = self.state.get();
        snapshot.observed = snapshot.draft.read();
        snapshot
    }

    fn send(self, commands: mpsc::Sender<Request>, action: Action) {
        let observed = self.snapshot();
        if self.pending.get_untracked() || !observed.visible || !allowed(&observed, action) {
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

fn translated(state: Signal<Snapshot>, key: &'static str) -> AnyElement {
    text(move || state.with(|_| vega_i18n::t(key))).into_element()
}

fn button(
    actions: Actions,
    commands: mpsc::Sender<Request>,
    action: Action,
    key: &'static str,
) -> AnyElement {
    let target = element_ref();
    let focused = signal(false);
    target.on_focus_in(move || focused.set(true));
    target.on_focus_out(move || focused.set(false));
    view()
        .element_ref(&target)
        .role(AccessibilityRole::Button)
        .focusable(true)
        .label(move || actions.state.with(|_| vega_i18n::t(key)))
        .disabled(move || actions.pending.get() || !allowed(&actions.snapshot(), action))
        .self_start()
        .flex_row()
        .items_center()
        .justify_center()
        .min_h(px(Layout::SETTINGS_CONTROL_MIN_HEIGHT))
        .border(px(1.))
        .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
        .px(px(Layout::SETTINGS_CONTROL_PADDING_X))
        .py_1()
        .border_color(move || {
            crate::nsh::color(if focused.get() {
                actions.state.get().colors.accent
            } else {
                actions.state.get().colors.border_subtle
            })
        })
        .bg(move || {
            let colors = actions.state.get().colors;
            crate::nsh::color(if matches!(action, Action::Save) {
                if actions.pending.get() || !allowed(&actions.snapshot(), action) {
                    colors.bg_hover
                } else {
                    colors.brand_primary
                }
            } else {
                colors.bg_elevated
            })
        })
        .text_color(move || {
            let colors = actions.state.get().colors;
            crate::nsh::color(if matches!(action, Action::Save) {
                if actions.pending.get() || !allowed(&actions.snapshot(), action) {
                    colors.text_tertiary
                } else {
                    colors.brand_on_accent
                }
            } else {
                colors.text_primary
            })
        })
        .on_press(move |_: &PressEvent| actions.send(commands.clone(), action))
        .child(text(move || {
            actions.state.with(|snapshot| {
                vega_i18n::t(
                    if matches!(action, Action::Save) && snapshot.authority.profile_saving {
                        "settings.saving"
                    } else if matches!(action, Action::Native(NativeAgentAction::Check))
                        && !snapshot.authority.can_recheck
                    {
                        "settings.state.checking"
                    } else {
                        key
                    },
                )
            })
        }))
        .into_element()
}

fn field_row(
    state: Signal<Snapshot>,
    key: &'static str,
    description: Option<&'static str>,
    control: AnyElement,
) -> AnyElement {
    super::super::nsh_form::field(
        move || state.get().colors,
        translated(state, key),
        description.map(|key| translated(state, key)),
        control,
    )
}

fn setup_row(
    state: Signal<Snapshot>,
    key: &'static str,
    status: impl Fn() -> &'static str + 'static,
    action: AnyElement,
) -> AnyElement {
    view()
        .w_full()
        .py_2()
        .flex_row()
        .items_center()
        .flex_wrap()
        .gap_2()
        .child(
            view()
                .flex_1()
                .min_w(px(0.))
                .gap_1()
                .child(translated(state, key))
                .child(
                    text(status)
                        .text_size(px(Typography::METADATA))
                        .text_color(move || crate::nsh::color(state.get().colors.text_secondary)),
                ),
        )
        .child(action)
        .into_element()
}

fn heading(state: Signal<Snapshot>, key: &'static str) -> AnyElement {
    text(move || state.with(|_| vega_i18n::t(key)))
        .text_size(px(Typography::HEADING_BLOCK))
        .font_weight(FontWeight(Typography::HEADING_BLOCK_WEIGHT.0 as u16))
        .into_element()
}

fn field(actions: Actions, draft: Draft, index: usize, key: &'static str) -> AnyElement {
    let value = signal(draft.read().text[index].clone());
    let edits = draft.clone();
    let composition = draft.clone();
    let target = element_ref();
    let focused = signal(false);
    target.on_focus_in(move || focused.set(true));
    target.on_focus_out(move || focused.set(false));
    field_row(
        actions.state,
        key,
        (index == 2).then_some("settings.agents.codex.arguments_hint"),
        nsh_base::input()
            .element_ref(&target)
            .label(move || actions.state.with(|_| vega_i18n::t(key)))
            .placeholder(move || {
                actions.state.with(|_| {
                    vega_i18n::t(match index {
                        0 => "settings.agents.codex.name_placeholder",
                        1 => "settings.agents.codex.executable_placeholder",
                        _ => "settings.agents.codex.arguments_placeholder",
                    })
                    .to_owned()
                })
            })
            .value(move || value.get())
            .multiline(index == 2)
            .readonly(move || {
                actions.pending.get() || actions.state.get().authority.profile_saving || {
                    actions.edits.get();
                    draft.read().frozen
                }
            })
            .border(px(1.))
            .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
            .px_3()
            .py_1()
            .w_full()
            .min_w(px(0.))
            .min_h(px(Layout::SETTINGS_CONTROL_MIN_HEIGHT))
            .border_color(move || {
                crate::nsh::color(if focused.get() {
                    actions.state.get().colors.accent
                } else {
                    actions.state.get().colors.border_subtle
                })
            })
            .bg(move || crate::nsh::color(actions.state.get().colors.bg_elevated))
            .on_change(move |text: &str| {
                if edits.edit(index, Some(text), None) {
                    value.set(text.to_owned());
                    actions
                        .edits
                        .update(|revision| *revision = revision.wrapping_add(1));
                }
            })
            .on_composition_change(move |event: &Composition| {
                if composition.edit(index, None, Some(event.composing)) {
                    actions
                        .edits
                        .update(|revision| *revision = revision.wrapping_add(1));
                }
            })
            .into_element(),
    )
}

fn toggle(actions: Actions, commands: mpsc::Sender<Request>, desktop: bool) -> AnyElement {
    let key = if desktop {
        "settings.agents.desktop.title"
    } else {
        "settings.agents.browser.title"
    };
    let action = Action::Native(if desktop {
        NativeAgentAction::Desktop
    } else {
        NativeAgentAction::Browser
    });
    let enabled = move || {
        let authority = actions.state.get().authority;
        if desktop {
            authority.desktop
        } else {
            authority.browser
        }
    };
    let target = element_ref();
    let focused = signal(false);
    target.on_focus_in(move || focused.set(true));
    target.on_focus_out(move || focused.set(false));
    let state = actions.state;
    view()
        .w_full()
        .min_w(px(0.))
        .flex_row()
        .items_center()
        .gap_3()
        .py_2()
        .child(
            view()
                .flex_1()
                .min_w(px(0.))
                .gap_1()
                .child(translated(state, key))
                .child(
                    text(move || {
                        state.with(|_| {
                            vega_i18n::t(if desktop {
                                "settings.agents.desktop.description"
                            } else {
                                "settings.agents.browser.description"
                            })
                        })
                    })
                    .text_size(px(Typography::METADATA))
                    .text_color(move || crate::nsh::color(state.get().colors.text_secondary)),
                ),
        )
        .child(
            view()
                .element_ref(&target)
                .role(AccessibilityRole::Switch)
                .focusable(true)
                .label(move || actions.state.with(|_| vega_i18n::t(key)))
                .accessibility_state(move || AccessibilityState {
                    checked: Some(if enabled() {
                        Checked::True
                    } else {
                        Checked::False
                    }),
                    ..Default::default()
                })
                .disabled(move || actions.pending.get() || !allowed(&actions.snapshot(), action))
                .relative()
                .flex_shrink_0()
                .w(px(Layout::SETTINGS_SWITCH_WIDTH))
                .h(px(Layout::SETTINGS_SWITCH_HEIGHT))
                .rounded(px(Layout::SETTINGS_SWITCH_HEIGHT / 2.))
                .border(px(1.))
                .border_color(move || {
                    crate::nsh::color(if focused.get() {
                        state.get().colors.accent
                    } else {
                        state.get().colors.border_subtle
                    })
                })
                .bg(move || {
                    crate::nsh::color(if enabled() {
                        state.get().colors.brand_primary
                    } else {
                        state.get().colors.text_tertiary
                    })
                })
                .on_press(move |_: &PressEvent| actions.send(commands.clone(), action))
                .child(
                    view()
                        .absolute()
                        .top(px(1.))
                        .left(move || {
                            px(if enabled() {
                                Layout::SETTINGS_SWITCH_WIDTH - Layout::ICON_SIZE - 3.
                            } else {
                                1.
                            })
                        })
                        .size(px(Layout::ICON_SIZE))
                        .rounded(px(Layout::ICON_SIZE / 2.))
                        .bg(move || {
                            crate::nsh::color(if enabled() {
                                state.get().colors.brand_on_accent
                            } else {
                                state.get().colors.bg_elevated
                            })
                        }),
                ),
        )
        .into_element()
}

fn render(state: Signal<Snapshot>, commands: mpsc::Sender<Request>) -> AnyElement {
    let actions = Actions {
        state,
        pending: signal(false),
        failed: signal(false),
        edits: signal(0),
    };
    let draft = state.get_untracked().draft;
    let layout = commands.clone();
    let measured = signal(0.0f32);
    view()
        .w_full()
        .flex_shrink_0()
        .gap(px(Layout::SETTINGS_SECTION_GAP))
        .text_size(px(Typography::BODY))
        .line_height(px(Typography::BODY * Typography::BODY_LINE_HEIGHT))
        .text_color(move || crate::nsh::color(state.get().colors.text_primary))
        .on_layout(move |bounds: &Bounds<Points>| {
            let height = bounds.size.height.0;
            if height.is_finite() && height > 0. && (height - measured.get_untracked()).abs() > 0.5
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
                .w_full()
                .flex_shrink_0()
                .gap_2()
                .child(heading(state, "settings.agents.native.title"))
                .child(
                    super::super::nsh_form::card(move || state.get().colors)
                        .child(
                            text(move || {
                                state.with(|_| vega_i18n::t("settings.agents.native.description"))
                            })
                            .mt_2()
                            .text_size(px(Typography::METADATA))
                            .line_height(px(Typography::METADATA * Typography::BODY_LINE_HEIGHT))
                            .text_color(move || {
                                crate::nsh::color(state.get().colors.text_secondary)
                            }),
                        )
                        .child(toggle(actions, commands.clone(), true))
                        .child(toggle(actions, commands.clone(), false))
                        .child(button(
                            actions,
                            commands.clone(),
                            Action::Native(NativeAgentAction::Extension),
                            "settings.agents.browser.install_extension",
                        )),
                ),
        )
        .child(
            view()
                .w_full()
                .flex_shrink_0()
                .gap_2()
                .child(
                    view()
                        .w_full()
                        .flex_row()
                        .flex_wrap()
                        .items_center()
                        .gap_2()
                        .child(
                            view()
                                .flex_1()
                                .min_w(px(0.))
                                .child(heading(state, "settings.agents.desktop.setup")),
                        )
                        .child(button(
                            actions,
                            commands.clone(),
                            Action::Native(NativeAgentAction::Check),
                            "settings.action.recheck",
                        )),
                )
                .child(
                    super::super::nsh_form::card(move || state.get().colors)
                        .child(each(
                            move || state.get().authority.feedback,
                            |item| item.clone(),
                            move |item, _| {
                                text(move || item.get().0)
                                    .mt_2()
                                    .text_size(px(Typography::METADATA))
                                    .text_color(move || {
                                        crate::nsh::color(if item.get().1 {
                                            state.get().colors.warning
                                        } else {
                                            state.get().colors.text_secondary
                                        })
                                    })
                            },
                        ))
                        .child(setup_row(
                            state,
                            "settings.agents.desktop.component",
                            move || state.with(|s| vega_i18n::t(s.authority.component)),
                            button(
                                actions,
                                commands.clone(),
                                Action::Native(NativeAgentAction::Prepare),
                                "settings.agents.desktop.prepare",
                            ),
                        ))
                        .child(setup_row(
                            state,
                            "settings.agents.accessibility",
                            move || state.with(|s| vega_i18n::t(s.authority.accessibility)),
                            button(
                                actions,
                                commands.clone(),
                                Action::Native(NativeAgentAction::Accessibility),
                                "settings.agents.accessibility.settings",
                            ),
                        ))
                        .child(setup_row(
                            state,
                            "settings.agents.screen_recording",
                            move || state.with(|s| vega_i18n::t(s.authority.screen_recording)),
                            button(
                                actions,
                                commands.clone(),
                                Action::Native(NativeAgentAction::ScreenRecording),
                                "settings.agents.screen_recording.settings",
                            ),
                        ))
                        .child(
                            text(move || {
                                state.with(|_| {
                                    vega_i18n::t("settings.agents.screen_recording.restart")
                                })
                            })
                            .text_size(px(Typography::METADATA))
                            .text_color(move || {
                                crate::nsh::color(state.get().colors.text_secondary)
                            }),
                        )
                        .child(field_row(
                            state,
                            "settings.agents.capture",
                            None,
                            translated(state, "settings.state.unverified"),
                        )),
                ),
        )
        .child(
            view()
                .w_full()
                .flex_shrink_0()
                .gap_2()
                .child(
                    text("Codex")
                        .text_size(px(Typography::HEADING_BLOCK))
                        .font_weight(FontWeight(Typography::HEADING_BLOCK_WEIGHT.0 as u16)),
                )
                .child(
                    super::super::nsh_form::card(move || state.get().colors)
                        .child(
                            text(move || {
                                state.with(|snapshot| {
                                    vega_i18n::t(if snapshot.authority.profile.is_some() {
                                        "settings.agents.codex.configured"
                                    } else {
                                        "settings.agents.codex.setup"
                                    })
                                })
                            })
                            .text_size(px(Typography::METADATA))
                            .text_color(move || {
                                crate::nsh::color(state.get().colors.text_secondary)
                            }),
                        )
                        .child(field(actions, draft.clone(), 0, "settings.field.name"))
                        .child(field(
                            actions,
                            draft.clone(),
                            1,
                            "settings.field.executable",
                        ))
                        .child(field(actions, draft, 2, "settings.field.arguments"))
                        .child(
                            view()
                                .mt_3()
                                .flex_row()
                                .flex_wrap()
                                .items_center()
                                .gap_2()
                                .child(button(
                                    actions,
                                    commands,
                                    Action::Save,
                                    "settings.agents.codex.save",
                                )),
                        ),
                ),
        )
        .child(when(
            move || actions.failed.get(),
            move || translated(state, "ui.command_failed"),
        ))
        .into_element()
}
