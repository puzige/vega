use gpui_kit::{App, Context, Window};
use nsh_core::prelude::*;
use tokio::sync::{mpsc, oneshot};
use vega_theme::{Layout, ThemeColors, Typography};

use super::SettingsView;
use crate::nsh::{ProjectionId, Surface};

pub(super) struct Presentation {
    surface: Surface<Snapshot, Request>,
    id: ProjectionId,
    height: f32,
}

#[derive(Clone)]
struct Snapshot {
    id: ProjectionId,
    theme: String,
    language: String,
    sidebar: bool,
    enabled: bool,
    colors: ThemeColors,
}

#[derive(Clone, Copy)]
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
    Decide(Decision),
    Layout(f32),
}

impl SettingsView {
    fn appearance_snapshot(&self, cx: &App, id: ProjectionId) -> Snapshot {
        Snapshot {
            id,
            theme: self.config.ui.theme.clone(),
            language: vega_i18n::language().to_string(),
            sidebar: !cx
                .try_global::<crate::sidebar::SidebarCollapsed>()
                .is_some_and(|value| value.0),
            enabled: !self.provider_management.saving && !self.default_permission.io_pending(),
            colors: vega_theme::theme(cx).colors,
        }
    }

    pub(super) fn render_appearance(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        use gpui_kit::{IntoElement, ParentElement, Styled};
        if self.appearance_presentation.is_none() {
            let id = ProjectionId::new();
            let (surface, mut requests) =
                Surface::new(self.appearance_snapshot(cx, id.clone()), render, window, cx);
            self.appearance_presentation = Some(Presentation {
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
                                    let accepted = this.section == 1
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
                .send(Request::Decide(Decision {
                    snapshot,
                    action,
                    reply,
                }))
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
    let target = element_ref();
    let focused = signal(false);
    target.on_focus_in(move || focused.set(true));
    target.on_focus_out(move || focused.set(false));
    view()
        .element_ref(&target)
        .role(AccessibilityRole::Button)
        .label(move || actions.state.with(|_| vega_i18n::t(key)))
        .accessibility_state(move || nsh_core::AccessibilityState {
            selected: selected(&actions.state.get(), action),
            ..Default::default()
        })
        .disabled(move || actions.pending.get() || !actions.state.get().enabled)
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
        .bg(move || {
            crate::nsh::color(if selected(&actions.state.get(), action) {
                actions.state.get().colors.bg_active
            } else {
                actions.state.get().colors.bg_elevated
            })
        })
        .text_color(move || {
            crate::nsh::color(if selected(&actions.state.get(), action) {
                actions.state.get().colors.brand_primary
            } else {
                actions.state.get().colors.text_primary
            })
        })
        .on_press(move |_: &nsh_core::PressEvent| actions.send(commands.clone(), action))
        .child(text(move || actions.state.with(|_| vega_i18n::t(key))))
        .into_element()
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
    let target = element_ref();
    let focused = signal(false);
    target.on_focus_in(move || focused.set(true));
    target.on_focus_out(move || focused.set(false));
    view()
        .w_full()
        .flex_shrink_0()
        .gap_3()
        .p(px(Layout::SETTINGS_GROUP_PADDING))
        .border(px(1.))
        .rounded(px(Layout::PANEL_RADIUS))
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
            text(move || state.with(|_| vega_i18n::t("settings.appearance.title")))
                .text_size(px(Typography::HEADING_CARD)),
        )
        .child(text(move || {
            state.with(|_| vega_i18n::t("settings.appearance.theme"))
        }))
        .child(
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
                )),
        )
        .child(text(move || {
            state.with(|_| vega_i18n::t("settings.language"))
        }))
        .child(
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
                )),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_3()
                .child(text(move || {
                    state.with(|_| vega_i18n::t("settings.appearance.sidebar"))
                }))
                .child(
                    view()
                        .element_ref(&target)
                        .role(AccessibilityRole::Switch)
                        .label(move || {
                            state.with(|snapshot| {
                                vega_i18n::t(if snapshot.sidebar {
                                    "settings.sidebar.hide"
                                } else {
                                    "settings.sidebar.show"
                                })
                            })
                        })
                        .accessibility_state(move || nsh_core::AccessibilityState {
                            checked: Some(if state.get().sidebar {
                                nsh_core::Checked::True
                            } else {
                                nsh_core::Checked::False
                            }),
                            ..Default::default()
                        })
                        .disabled(actions.pending)
                        .border(px(1.))
                        .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
                        .p_2()
                        .border_color(move || {
                            crate::nsh::color(if focused.get() {
                                state.get().colors.accent
                            } else {
                                state.get().colors.border_subtle
                            })
                        })
                        .on_press(move |_: &nsh_core::PressEvent| {
                            actions.send(sidebar.clone(), Action::Sidebar)
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
                                                Layout::SETTINGS_SWITCH_WIDTH
                                                    - Layout::ICON_SIZE
                                                    - 2.
                                            } else {
                                                2.
                                            })
                                        })
                                        .size(px(Layout::ICON_SIZE))
                                        .rounded(px(Layout::ICON_SIZE / 2.))
                                        .bg(move || {
                                            crate::nsh::color(state.get().colors.bg_elevated)
                                        }),
                                ),
                        ),
                ),
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

#[cfg(test)]
mod tests {
    use super::{Action, Snapshot, accepts};

    fn snapshot() -> Snapshot {
        Snapshot {
            id: crate::nsh::ProjectionId::new(),
            theme: "system".into(),
            language: "en".into(),
            sidebar: true,
            enabled: true,
            colors: vega_theme::Theme::light().colors,
        }
    }

    #[test]
    fn stale_preferences_cannot_overwrite_newer_appearance() {
        let observed = snapshot();
        let mut current = observed.clone();
        current.theme = "dark".into();
        assert!(!accepts(&current, &observed, Action::Theme("light")));
        assert!(accepts(&current, &current, Action::Theme("light")));
        current.language = "zh-CN".into();
        assert!(!accepts(&current, &observed, Action::Language("en")));
        assert!(accepts(&current, &current, Action::Language("en")));
        assert!(!accepts(&current, &current, Action::Theme("unknown")));
        assert!(!accepts(&current, &current, Action::Language("unknown")));
    }

    #[test]
    fn saving_blocks_preferences_but_preserves_independent_sidebar_action() {
        let observed = snapshot();
        let mut current = observed.clone();
        current.enabled = false;
        assert!(!accepts(&current, &observed, Action::Theme("dark")));
        assert!(!accepts(&current, &observed, Action::Language("zh-CN")));
        assert!(accepts(&current, &observed, Action::Sidebar));
        current.sidebar = false;
        assert!(!accepts(&current, &observed, Action::Sidebar));
        assert!(accepts(&current, &current, Action::Sidebar));
    }
}
