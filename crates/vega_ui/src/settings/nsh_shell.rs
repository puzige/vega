use gpui_kit::{App, Context, Window};
use nsh_core::element::ImageSrc;
use nsh_core::prelude::*;
use tokio::sync::{mpsc, oneshot};
use vega_i18n::t;
use vega_theme::{Layout, ThemeColors, Typography};

use super::{SettingsDestination, SettingsView};
use crate::icons::Icon;
use crate::nsh::{ProjectionId, Surface};

const SECTIONS: [usize; 6] = [1, 0, 5, 7, 6, 4];
fn section_label(section: usize) -> &'static str {
    t(match section {
        0 | 2 | 3 => "settings.nav.providers",
        1 => "settings.nav.general",
        5 => "settings.nav.mcp",
        6 => "settings.nav.skills",
        7 => "settings.nav.agents",
        _ => "settings.nav.usage",
    })
}
fn destination(value: Option<SettingsDestination>) -> Option<(u8, usize)> {
    value.map(|value| match value {
        SettingsDestination::Section(section) => (0, section),
        SettingsDestination::ModelSection(section) => (1, section),
        SettingsDestination::ProviderCommand => (2, 0),
    })
}
#[derive(Clone, PartialEq, Eq)]
struct Authority {
    visible: bool,
    section: usize,
    route: Option<(String, String)>,
    pending: Option<(u8, usize)>,
    closing: bool,
    permission_saving: bool,
}
#[derive(Clone)]
struct Snapshot {
    id: ProjectionId,
    authority: Authority,
    selected: usize,
    error: Option<String>,
    colors: ThemeColors,
}
#[derive(Clone, Copy)]
enum Action {
    Back,
    Section(usize),
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
pub(super) struct Presentation {
    surface: Surface<Snapshot, Request>,
    id: ProjectionId,
    height: f32,
}
fn accepts(current: &Snapshot, observed: &Snapshot, action: Action) -> bool {
    current.id.matches(&observed.id)
        && current.authority.visible
        && current.authority == observed.authority
        && match action {
            Action::Back => true,
            Action::Section(section) => {
                !current.authority.permission_saving && SECTIONS.contains(&section)
            }
        }
}
impl SettingsView {
    fn shell_snapshot(&self, id: ProjectionId, cx: &App) -> Snapshot {
        Snapshot {
            id,
            authority: Authority {
                visible: cx
                    .try_global::<super::SettingsOpen>()
                    .is_some_and(|open| open.0),
                section: self.section,
                route: self.model_settings.clone(),
                pending: destination(self.pending_section),
                closing: self.pending_settings_close,
                permission_saving: self.default_permission.saving(),
            },
            selected: self.navigation_section(),
            error: self.error.as_ref().map(|error| error.render()),
            colors: vega_theme::theme(cx).colors,
        }
    }
    pub(super) fn sync_shell_presentations(&self, cx: &App) {
        for page in [&self.shell_navigation, &self.shell_heading]
            .into_iter()
            .flatten()
        {
            page.surface
                .publish(self.shell_snapshot(page.id.clone(), cx));
        }
    }
    fn render_shell(
        &mut self,
        heading: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        use gpui_kit::{IntoElement, ParentElement, Styled};
        if if heading {
            self.shell_heading.is_none()
        } else {
            self.shell_navigation.is_none()
        } {
            let id = ProjectionId::new();
            let renderer: fn(Signal<Snapshot>, mpsc::Sender<Request>) -> AnyElement = if heading {
                |snapshot, requests| render(snapshot, requests, true)
            } else {
                |snapshot, requests| render(snapshot, requests, false)
            };
            let (surface, mut requests) =
                Surface::new(self.shell_snapshot(id.clone(), cx), renderer, window, cx);
            let page = Presentation {
                surface,
                id,
                height: Typography::SETTINGS_TITLE * 1.6,
            };
            if heading {
                self.shell_heading = Some(page);
            } else {
                self.shell_navigation = Some(page);
            }
            cx.spawn_in(window, async move |this, cx| {
                while let Some(request) = requests.recv().await {
                    if this
                        .update_in(cx, |this, window, cx| {
                            match request {
                                Request::Decide(command) => {
                                    let page = if heading {
                                        &this.shell_heading
                                    } else {
                                        &this.shell_navigation
                                    };
                                    let accepted = page.as_ref().is_some_and(|page| {
                                        accepts(
                                            &this.shell_snapshot(page.id.clone(), cx),
                                            &command.observed,
                                            command.action,
                                        )
                                    });
                                    if accepted {
                                        match command.action {
                                            Action::Back => this.on_back(
                                                &gpui_kit::MouseUpEvent {
                                                    button: gpui_kit::MouseButton::Left,
                                                    position: gpui_kit::point(
                                                        gpui_kit::px(0.),
                                                        gpui_kit::px(0.),
                                                    ),
                                                    modifiers: Default::default(),
                                                    click_count: 1,
                                                },
                                                window,
                                                cx,
                                            ),
                                            Action::Section(section) => {
                                                this.cancel_provider_operation(cx);
                                                if this.section == 5
                                                    && section != 5
                                                    && (this.mcp.oauth_discovery.is_some()
                                                        || this.mcp.oauth_flow_id.is_some())
                                                {
                                                    this.cancel_mcp_oauth(cx);
                                                }
                                                this.request_settings_section(section, cx);
                                                cx.notify();
                                            }
                                        }
                                    }
                                    let _ = command.reply.send(accepted);
                                }
                                Request::Layout(height)
                                    if heading && height.is_finite() && height > 0. =>
                                {
                                    if let Some(page) = &mut this.shell_heading
                                        && (page.height - height).abs() > 0.5
                                    {
                                        page.height = height;
                                        cx.notify();
                                    }
                                }
                                Request::Layout(_) => {}
                            }
                            this.sync_shell_presentations(cx);
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();
        }
        let page = if heading {
            &mut self.shell_heading
        } else {
            &mut self.shell_navigation
        };
        let Some(page) = page else {
            return gpui_kit::div().into_any_element();
        };
        page.surface.sync_environment(cx);
        let container = gpui_kit::div().w_full().child(page.surface.entity());
        if heading {
            container
                .flex_shrink_0()
                .h(gpui_kit::px(page.height))
                .into_any_element()
        } else {
            container.h_full().into_any_element()
        }
    }
    pub(super) fn render_shell_navigation(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        self.render_shell(false, window, cx)
    }
    pub(super) fn render_shell_heading(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        self.render_shell(true, window, cx)
    }
}
fn button(
    state: Signal<Snapshot>,
    commands: mpsc::Sender<Request>,
    pending: Signal<bool>,
    failed: Signal<bool>,
    section: Option<usize>,
    icon: Icon,
) -> AnyElement {
    let target = element_ref();
    let focused = signal(false);
    target.on_focus_in(move || focused.set(true));
    target.on_focus_out(move || focused.set(false));
    let action = section.map_or(Action::Back, Action::Section);
    let label = move || {
        state.with(|_| section.map_or_else(|| t("settings.navigation.back_app"), section_label))
    };
    view()
        .element_ref(&target)
        .role(AccessibilityRole::Button)
        .focusable(true)
        .label(label)
        .accessibility_state(move || AccessibilityState {
            selected: section.is_some_and(|section| state.get().selected == section),
            ..Default::default()
        })
        .disabled(move || pending.get() || !accepts(&state.get(), &state.get(), action))
        .flex_row()
        .items_center()
        .gap_2()
        .px_2()
        .py_2()
        .w_full()
        .min_h(px(Typography::SIDEBAR_LINE_HEIGHT))
        .border(px(1.))
        .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
        .border_color(move || {
            crate::nsh::color(if focused.get() {
                state.get().colors.accent
            } else {
                state.get().colors.bg_base
            })
        })
        .bg(move || {
            crate::nsh::color(
                if section.is_some_and(|section| state.get().selected == section) {
                    state.get().colors.bg_active
                } else {
                    state.get().colors.bg_base
                },
            )
        })
        .on_press(move |_: &PressEvent| {
            let observed = state.get_untracked();
            if pending.get_untracked() || !accepts(&observed, &observed, action) {
                return;
            }
            pending.set(true);
            let commands = commands.clone();
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
                pending.set(false);
                failed.set(!accepted);
            });
        })
        .child(when(
            move || crate::icons::nsh_source(icon).is_some(),
            move || {
                svg(ImageSrc::svg_markup(
                    &crate::icons::nsh_source(icon).unwrap_or_default(),
                ))
                .size(px(Layout::ICON_SIZE))
                .text_color(move || crate::nsh::color(state.get().colors.text_secondary))
            },
        ))
        .child(text(label))
        .into_element()
}
fn render(state: Signal<Snapshot>, commands: mpsc::Sender<Request>, heading: bool) -> AnyElement {
    if heading {
        let measured = signal(0.0f32);
        return view()
            .w_full()
            .flex_shrink_0()
            .gap_2()
            .on_layout(move |bounds: &Bounds<Points>| {
                let height = bounds.size.height.0;
                if height.is_finite()
                    && height > 0.
                    && (height - measured.get_untracked()).abs() > 0.5
                {
                    measured.set(height);
                    let commands = commands.clone();
                    spawn(async move {
                        let _ = commands.send(Request::Layout(height)).await;
                    });
                }
            })
            .child(
                text(move || state.with(|snapshot| section_label(snapshot.authority.section)))
                    .text_size(px(Typography::SETTINGS_TITLE))
                    .text_color(move || crate::nsh::color(state.get().colors.text_primary)),
            )
            .child(when(
                move || state.get().error.is_some(),
                move || {
                    text(move || state.get().error.unwrap_or_default())
                        .text_size(px(Typography::BODY))
                        .text_color(move || crate::nsh::color(state.get().colors.danger))
                },
            ))
            .into_element();
    }
    let pending = signal(false);
    let failed = signal(false);
    scroll()
        .w_full()
        .h_full()
        .child(
            view()
                .w_full()
                .pt(px(Layout::MAIN_HEADER_HEIGHT))
                .px_3()
                .gap_2()
                .child(button(
                    state,
                    commands.clone(),
                    pending,
                    failed,
                    None,
                    Icon::ArrowLeft,
                ))
                .child(view().h(px(24.)))
                .children(SECTIONS.into_iter().map(|section| {
                    button(
                        state,
                        commands.clone(),
                        pending,
                        failed,
                        Some(section),
                        match section {
                            0 => Icon::Connection,
                            1 => Icon::Settings,
                            4 => Icon::Chart,
                            5 => Icon::Plug,
                            6 => Icon::Blocks,
                            _ => Icon::Bot,
                        },
                    )
                }))
                .child(when(
                    move || failed.get(),
                    move || {
                        text(move || state.with(|_| t("ui.command_failed")))
                            .text_color(move || crate::nsh::color(state.get().colors.danger))
                    },
                )),
        )
        .into_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot() -> Snapshot {
        Snapshot {
            id: ProjectionId::new(),
            authority: Authority {
                visible: true,
                section: 3,
                route: Some(("provider".into(), "model".into())),
                pending: None,
                closing: false,
                permission_saving: false,
            },
            selected: 0,
            error: None,
            colors: vega_theme::Theme::light().colors,
        }
    }
    #[test]
    fn shell_navigation_rejects_old_routes_pending_destinations_and_hidden_views() {
        let observed = snapshot();
        assert!(accepts(&observed, &observed, Action::Section(1)));
        let mut current = observed.clone();
        current.authority.route.as_mut().unwrap().1 = "other-model".into();
        assert!(!accepts(&current, &observed, Action::Back));
        current = observed.clone();
        current.authority.pending = Some((0, 1));
        assert!(!accepts(&current, &observed, Action::Section(4)));
        current = observed.clone();
        current.authority.visible = false;
        assert!(!accepts(&current, &observed, Action::Back));
        assert!(!accepts(&observed, &observed, Action::Section(42)));
    }
    #[test]
    fn permission_save_blocks_section_changes_without_removing_back_recovery() {
        let mut current = snapshot();
        current.authority.permission_saving = true;
        assert!(!accepts(&current, &current, Action::Section(1)));
        assert!(accepts(&current, &current, Action::Back));
    }
}
