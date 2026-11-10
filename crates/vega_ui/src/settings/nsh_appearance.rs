mod keyboard;

use gpui_kit::{App, Context, Window};
use nsh_core::prelude::*;
use tokio::sync::{mpsc, oneshot};
use vega_theme::{Layout, ThemeColors, Typography};

use super::SettingsView;
use crate::nsh::{ProjectionId, Surface};

pub(super) struct Presentation {
    surface: Surface<Snapshot, Request>,
    id: ProjectionId,
    epoch: u64,
    height: f32,
    focused: Option<Action>,
    targets: [super::nsh_control::FocusTarget; 6],
    _focus_events: Vec<gpui_kit::Subscription>,
}

impl Presentation {
    pub(super) fn revoke(&mut self) {
        self.id = ProjectionId::new();
        self.epoch += 1;
        self.focused = None;
    }
}

#[derive(Clone)]
struct Snapshot {
    epoch: u64,
    id: ProjectionId,
    theme: String,
    language: String,
    sidebar: bool,
    enabled: bool,
    focused: Option<Action>,
    colors: ThemeColors,
}

#[derive(Clone, Copy, PartialEq)]
enum Action {
    Theme(&'static str),
    Language(&'static str),
    Sidebar,
}

struct Decision {
    snapshot: Snapshot,
    action: Action,
    reply: oneshot::Sender<bool>,
}

enum Request {
    Decide(Box<Decision>),
    Layout(f32),
}

impl SettingsView {
    fn appearance_snapshot(&self, cx: &App, id: ProjectionId) -> Snapshot {
        Snapshot {
            id,
            epoch: self
                .appearance_presentation
                .as_ref()
                .map_or(0, |page| page.epoch),
            focused: self
                .appearance_presentation
                .as_ref()
                .and_then(|page| page.focused),
            theme: self.config.ui.theme.clone(),
            language: vega_i18n::language().to_string(),
            sidebar: !cx
                .try_global::<crate::sidebar::SidebarCollapsed>()
                .is_some_and(|value| value.0),
            enabled: !self.provider_management.saving && !self.default_permission.io_pending(),
            colors: vega_theme::theme(cx).colors,
        }
    }

    fn appearance_visible(&self, cx: &App) -> bool {
        !self.palette_open
            && self.section == 1
            && self.pending_section.is_none()
            && !self.pending_settings_close
            && cx
                .try_global::<super::SettingsOpen>()
                .is_some_and(|open| open.0)
    }

    fn first_appearance_target(&self, cx: &App) -> Option<&super::nsh_control::FocusTarget> {
        let page = self.appearance_presentation.as_ref()?;
        if !self.appearance_visible(cx) {
            return None;
        }
        let current = self.appearance_snapshot(cx, page.id.clone());
        page.targets
            .iter()
            .zip(keyboard::CONTROLS)
            .find_map(|(target, action)| {
                keyboard::available(target, &current, action).then_some(target)
            })
    }

    pub(super) fn focus_first_appearance(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(target) = self.first_appearance_target(cx) else {
            return false;
        };
        window.focus(&target.handle, cx);
        true
    }

    pub(super) fn render_appearance(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        use gpui_kit::{IntoElement, ParentElement, Styled};
        if self.appearance_presentation.is_none() {
            let id = ProjectionId::new();
            let (surface, mut requests) = Surface::new_transparent(
                self.appearance_snapshot(cx, id.clone()),
                render,
                window,
                cx,
            );
            let mut focus_events = Vec::new();
            let targets = std::array::from_fn(|_| super::nsh_control::FocusTarget::new(cx));
            for (action, target) in keyboard::CONTROLS.into_iter().zip(targets.iter().cloned()) {
                let focus = target.handle.clone();
                let owner = cx.weak_entity();
                focus_events.push(cx.on_focus(&focus, window, move |this, _, cx| {
                    if let Some(page) = &mut this.appearance_presentation {
                        page.focused = Some(action);
                        cx.notify();
                    }
                }));
                focus_events.push(cx.on_blur(&focus, window, move |this, _, cx| {
                    if let Some(page) = &mut this.appearance_presentation
                        && page.focused == Some(action)
                    {
                        page.focused = None;
                        cx.notify();
                    }
                }));
                keyboard::register(&target, surface.entity().read(cx), action, owner);
            }
            let owner = cx.weak_entity();
            let window_id = window.window_handle().window_id();
            focus_events.push(cx.intercept_keystrokes(move |event, window, cx| {
                let modifiers = event.keystroke.modifiers;
                if event.keystroke.key != "tab"
                    || modifiers
                        != (gpui_kit::Modifiers {
                            shift: modifiers.shift,
                            ..Default::default()
                        })
                    || window.window_handle().window_id() != window_id
                    || !window.is_window_active()
                {
                    return;
                }
                let Some(owner) = owner.upgrade() else {
                    return;
                };
                let handled = owner.update(cx, |settings, cx| {
                    if modifiers.shift
                        && settings
                            .first_appearance_target(cx)
                            .is_some_and(|t| t.handle.is_focused(window))
                    {
                        return settings.focus_navigation_from_content(window, cx);
                    }
                    let Some(page) = &settings.appearance_presentation else {
                        return false;
                    };
                    let Some(last) = page.targets.last() else {
                        return false;
                    };
                    let current = settings.appearance_snapshot(cx, page.id.clone());
                    if !settings.appearance_visible(cx)
                        || !keyboard::available(last, &current, Action::Sidebar)
                    {
                        return false;
                    }
                    let Some(picker) = settings.permission_trigger_handle(cx) else {
                        return false;
                    };
                    if !modifiers.shift && last.handle.is_focused(window) {
                        window.focus(picker, cx);
                        return true;
                    }
                    if modifiers.shift && picker.is_focused(window) {
                        window.focus(&last.handle, cx);
                        return true;
                    }
                    false
                });
                if handled {
                    cx.stop_propagation();
                }
            }));
            self.appearance_presentation = Some(Presentation {
                surface,
                id,
                focused: None,
                _focus_events: focus_events,
                targets,
                epoch: 0,
                height: Layout::SETTINGS_NOTES_MAX_HEIGHT,
            });
            cx.spawn_in(window, async move |this, cx| {
                while let Some(request) = requests.recv().await {
                    if this
                        .update_in(cx, |this, window, cx| {
                            match request {
                                Request::Decide(command) => {
                                    let accepted = this.appearance_visible(cx)
                                        && this.appearance_presentation.as_ref().is_some_and(
                                            |page| {
                                                if !page.id.matches(&command.snapshot.id) {
                                                    return false;
                                                }
                                                let current =
                                                    this.appearance_snapshot(cx, page.id.clone());
                                                accepts(&current, &command.snapshot, command.action)
                                            },
                                        );
                                    if accepted {
                                        match command.action {
                                            Action::Theme(value) => this.set_appearance(value, cx),
                                            Action::Language(value) => {
                                                this.set_ui_language(value, cx)
                                            }
                                            Action::Sidebar => window.dispatch_action(
                                                Box::new(crate::sidebar::ToggleSidebar),
                                                cx,
                                            ),
                                        }
                                    }
                                    let _ = command.reply.send(accepted);
                                }
                                Request::Layout(height) if height.is_finite() && height > 0. => {
                                    if let Some(page) = &mut this.appearance_presentation
                                        && (page.height - height).abs() > 0.5
                                    {
                                        page.height = height;
                                        cx.notify();
                                    }
                                }
                                Request::Layout(_) => {}
                            }
                            if let Some(page) = &this.appearance_presentation {
                                page.surface
                                    .publish(this.appearance_snapshot(cx, page.id.clone()));
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
        let Some(page) = &self.appearance_presentation else {
            return gpui_kit::div().into_any_element();
        };
        let snapshot = self.appearance_snapshot(cx, page.id.clone());
        let Some(page) = &mut self.appearance_presentation else {
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
}

impl Actions {
    fn send(self, commands: mpsc::Sender<Request>, action: Action) {
        let snapshot = self.state.get_untracked();
        if self.pending.get_untracked() || (!matches!(action, Action::Sidebar) && !snapshot.enabled)
        {
            return;
        }
        self.pending.set(true);
        self.failed.set(false);
        spawn(async move {
            let (reply, response) = oneshot::channel();
            if commands
                .send(Request::Decide(Box::new(Decision {
                    snapshot,
                    action,
                    reply,
                })))
                .await
                .is_err()
                || !matches!(response.await, Ok(true))
            {
                self.failed.set(true);
            }
            self.pending.set(false);
        });
    }
}

fn accepts(current: &Snapshot, observed: &Snapshot, action: Action) -> bool {
    match action {
        Action::Theme(value) => {
            current.enabled
                && observed.theme == current.theme
                && matches!(value, "light" | "dark" | "system")
        }
        Action::Language(value) => {
            current.enabled
                && observed.language == current.language
                && matches!(value, "en" | "zh-CN")
        }
        Action::Sidebar => current.sidebar == observed.sidebar,
    }
}

fn selected(snapshot: &Snapshot, action: Action) -> bool {
    match action {
        Action::Theme(value) => snapshot.theme == value,
        Action::Language(value) => snapshot.language == value,
        Action::Sidebar => snapshot.sidebar,
    }
}

fn control(
    actions: Actions,
    commands: mpsc::Sender<Request>,
    action: Action,
    key: &'static str,
) -> AnyElement {
    view()
        .relative()
        .self_start()
        .min_h(px(Layout::SETTINGS_CONTROL_MIN_HEIGHT))
        .px(px(Layout::SETTINGS_CONTROL_PADDING_X))
        .py_1()
        .border(px(1.))
        .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
        .border_color(move || {
            crate::nsh::color(if actions.state.get().focused == Some(action) {
                actions.state.get().colors.accent
            } else {
                actions.state.get().colors.border_subtle
            })
        })
        .bg(move || {
            crate::nsh::color(if selected(&actions.state.get(), action) {
                actions.state.get().colors.bg_active
            } else {
                actions.state.get().colors.bg_elevated
            })
        })
        .text_color(move || crate::nsh::color(actions.state.get().colors.text_primary))
        .child(text(move || actions.state.with(|_| vega_i18n::t(key))).accessible(false))
        .child(keyboard::target(action, actions, commands, move || {
            actions.state.with(|_| vega_i18n::t(key))
        }))
        .into_element()
}

fn field_row(state: Signal<Snapshot>, key: &'static str, control: AnyElement) -> AnyElement {
    super::nsh_form::field(
        move || state.get().colors,
        text(move || state.with(|_| vega_i18n::t(key))),
        None,
        control,
    )
}

fn render(state: Signal<Snapshot>, commands: mpsc::Sender<Request>) -> AnyElement {
    let actions = Actions {
        state,
        pending: signal(false),
        failed: signal(false),
    };
    let measured = signal(0.0f32);
    let layout = commands.clone();
    let sidebar = commands.clone();
    view()
        .w_full()
        .flex_shrink_0()
        .gap_0()
        .p(px(Layout::SETTINGS_GROUP_PADDING))
        .border(px(1.))
        .rounded(px(Layout::PANEL_RADIUS))
        .border_color(move || crate::nsh::color(state.get().colors.border_subtle))
        .bg(move || crate::nsh::color(state.get().colors.bg_subtle))
        .text_color(move || crate::nsh::color(state.get().colors.text_primary))
        .text_size(px(Typography::BODY))
        .line_height(px((Typography::BODY * Typography::BODY_LINE_HEIGHT).round()))
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
            text(move || state.with(|_| vega_i18n::t("settings.appearance.title")))
                .text_size(px(Typography::HEADING_BLOCK))
                .line_height(px(Typography::HEADING_BLOCK * Typography::BODY_LINE_HEIGHT))
                .font_weight(FontWeight(Typography::HEADING_BLOCK_WEIGHT.0 as u16)),
        )
        .child(field_row(
            state,
            "settings.appearance.theme",
            view()
                .flex_row()
                .flex_wrap()
                .gap_2()
                .child(control(
                    actions,
                    commands.clone(),
                    Action::Theme("light"),
                    "settings.appearance.light",
                ))
                .child(control(
                    actions,
                    commands.clone(),
                    Action::Theme("dark"),
                    "settings.appearance.dark",
                ))
                .child(control(
                    actions,
                    commands.clone(),
                    Action::Theme("system"),
                    "settings.appearance.system",
                ))
                .into_element(),
        ))
        .child(field_row(
            state,
            "settings.language",
            view()
                .flex_row()
                .flex_wrap()
                .gap_2()
                .child(control(
                    actions,
                    commands.clone(),
                    Action::Language("en"),
                    "settings.language.english",
                ))
                .child(control(
                    actions,
                    commands,
                    Action::Language("zh-CN"),
                    "settings.language.chinese",
                ))
                .into_element(),
        ))
        .child(field_row(
            state,
            "settings.appearance.sidebar",
            view()
                .relative()
                .self_start()
                .border(px(1.))
                .rounded(px(Layout::SETTINGS_SWITCH_HEIGHT / 2.))
                .border_color(move || {
                    crate::nsh::color(if state.get().focused == Some(Action::Sidebar) {
                        state.get().colors.accent
                    } else {
                        state.get().colors.border_subtle
                    })
                })
                .child(
                    view()
                        .relative()
                        .w(px(Layout::SETTINGS_SWITCH_WIDTH))
                        .h(px(Layout::SETTINGS_SWITCH_HEIGHT))
                        .rounded(px(Layout::SETTINGS_SWITCH_HEIGHT / 2.))
                        .bg(move || {
                            crate::nsh::color(if state.get().sidebar {
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
                                    px(if state.get().sidebar {
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
                .child(keyboard::target(
                    Action::Sidebar,
                    actions,
                    sidebar,
                    move || {
                        state.with(|snapshot| {
                            vega_i18n::t(if snapshot.sidebar {
                                "settings.sidebar.hide"
                            } else {
                                "settings.sidebar.show"
                            })
                        })
                    },
                ))
                .into_element(),
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
