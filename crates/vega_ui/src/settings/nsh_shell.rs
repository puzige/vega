use gpui_kit::{App, Context, FocusHandle, KeyDownEvent, Window};
use nsh_core::element::ImageSrc;
use nsh_core::prelude::*;
use tokio::sync::{mpsc, oneshot};
use vega_i18n::t;
use vega_theme::{Layout, ThemeColors, Typography};

use super::{SettingsDestination, SettingsView};
use crate::icons::Icon;
use crate::nsh::keyboard::{Key, Selection, Step};
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
    keyboard: Option<Action>,
    keyboard_focus_visible: bool,
}
#[derive(Clone, Copy, PartialEq, Eq)]
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
    focus: FocusHandle,
    entered_from_content: bool,
    keyboard_session: ProjectionId,
    keyboard: Option<Selection<Action>>,
    keyboard_observed: Option<Snapshot>,
    _focus_subscriptions: Vec<gpui_kit::Subscription>,
}
fn enabled(snapshot: &Snapshot) -> Vec<Action> {
    std::iter::once(Action::Back)
        .chain(SECTIONS.into_iter().map(Action::Section))
        .filter(|action| accepts(snapshot, snapshot, *action))
        .collect()
}
fn selected_section(snapshot: &Snapshot, session: &ProjectionId) -> Option<Selection<Action>> {
    let action = Action::Section(snapshot.selected);
    accepts(snapshot, snapshot, action)
        .then(|| Selection::reconcile(None, session, &[action]))
        .flatten()
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
            keyboard: None,
            keyboard_focus_visible: false,
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
            let snapshot = self.shell_snapshot(id.clone(), cx);
            let opening_focus = self
                .section_focuses
                .iter()
                .find(|focus| !heading && focus.is_focused(window));
            let keyboard_session = ProjectionId::new();
            let keyboard =
                opening_focus.and_then(|_| selected_section(&snapshot, &keyboard_session));
            let focus = opening_focus
                .cloned()
                .unwrap_or_else(|| cx.focus_handle().tab_stop(!heading));
            let (surface, mut requests) = Surface::new_transparent(snapshot, renderer, window, cx);
            let focus_subscriptions = if heading {
                Vec::new()
            } else {
                vec![
                    cx.on_focus_in(&focus, window, |this, window, cx| {
                        let Some(page) = &this.shell_navigation else {
                            return;
                        };
                        if page.focus.contains_focused(window, cx) {
                            let focus = page.focus.clone();
                            let snapshot = this.shell_snapshot(page.id.clone(), cx);
                            let next = Selection::advance(
                                None,
                                &page.keyboard_session,
                                &enabled(&snapshot),
                                page.entered_from_content || window.modifiers().shift,
                            );
                            if let Some(page) = &mut this.shell_navigation
                                && let Step::Selected(next) = next
                            {
                                page.entered_from_content = false;
                                page.keyboard = Some(next);
                            }
                            window.focus(&focus, cx);
                        }
                        cx.notify();
                    }),
                    cx.on_focus_out(&focus, window, |_, _, _, cx| cx.notify()),
                ]
            };
            let page = Presentation {
                surface,
                id,
                height: Typography::SETTINGS_TITLE * 1.6,
                focus,
                entered_from_content: false,
                keyboard_session,
                keyboard,
                keyboard_observed: None,
                _focus_subscriptions: focus_subscriptions,
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
                                    let accepted = !heading
                                        && this.shell_keyboard_activate(
                                            &command.observed,
                                            command.action,
                                            window,
                                            cx,
                                        );
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
        let projected = if heading {
            self.shell_heading.as_ref()
        } else {
            self.shell_navigation.as_ref()
        }
        .map(|page| self.shell_snapshot(page.id.clone(), cx));
        let page = if heading {
            &mut self.shell_heading
        } else {
            &mut self.shell_navigation
        };
        let Some(page) = page else {
            return gpui_kit::div().into_any_element();
        };
        page.surface.sync_environment(cx);
        if !heading && let Some(mut projected) = projected {
            if page
                .keyboard_observed
                .as_ref()
                .is_some_and(|previous| previous.authority != projected.authority)
            {
                let keep_section = page.focus.is_focused(window)
                    && page.keyboard_observed.as_ref().is_some_and(|previous| {
                        previous.keyboard == Some(Action::Section(projected.selected))
                            && previous.authority.section != projected.authority.section
                            && projected.authority.pending.is_none()
                            && !projected.authority.closing
                    });
                page.keyboard_session = ProjectionId::new();
                page.keyboard = if keep_section {
                    selected_section(&projected, &page.keyboard_session)
                } else {
                    None
                };
            }
            page.keyboard = Selection::reconcile(
                page.keyboard.as_ref(),
                &page.keyboard_session,
                &enabled(&projected),
            );
            projected.keyboard_focus_visible = window.last_input_was_keyboard();
            if page.focus.is_focused(window) {
                projected.keyboard = page.keyboard.as_ref().and_then(|selection| {
                    selection.action(&page.keyboard_session, &enabled(&projected))
                });
            }
            page.keyboard_observed = Some(projected.clone());
            page.surface.publish(projected);
        }
        let focus = page.focus.clone();
        let selected = page
            .keyboard_observed
            .as_ref()
            .and_then(|snapshot| snapshot.keyboard.map(|action| (snapshot.clone(), action)));
        let container = gpui_kit::div().w_full().child(page.surface.entity());
        if !heading {
            use gpui_kit::{InteractiveElement, StatefulInteractiveElement};
            let mut container = container
                .id("settings-nsh-navigation-host")
                .relative()
                .track_focus(&focus)
                .tab_stop(true)
                .role(gpui_kit::Role::Group)
                .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    this.shell_keyboard(event, window, cx);
                }))
                .capture_any_mouse_down(cx.listener(|this, _, _, cx| {
                    if let Some(page) = &mut this.shell_navigation {
                        page.keyboard = None;
                    }
                    cx.notify();
                }));
            if let Some((observed, action)) = selected {
                let weak = cx.weak_entity();
                container = container.child(
                    gpui_kit::div()
                        .id("settings-nsh-navigation-active")
                        .absolute()
                        .inset_0()
                        .role(gpui_kit::Role::Button)
                        .aria_label(match action {
                            Action::Back => t("settings.navigation.back_app"),
                            Action::Section(section) => section_label(section),
                        })
                        .aria_active_descendant()
                        .on_a11y_action(gpui_kit::AccessibleAction::Click, move |_, window, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.shell_keyboard_activate(&observed, action, window, cx);
                            });
                        }),
                );
            }
            return container.h_full().into_any_element();
        }
        if heading {
            container
                .flex_shrink_0()
                .h(gpui_kit::px(page.height))
                .into_any_element()
        } else {
            container.h_full().into_any_element()
        }
    }
    fn shell_keyboard_activate(
        &mut self,
        observed: &Snapshot,
        action: Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(page) = &self.shell_navigation else {
            return false;
        };
        if !accepts(&self.shell_snapshot(page.id.clone(), cx), observed, action) {
            return false;
        }
        match action {
            Action::Back => self.on_back(
                &gpui_kit::MouseUpEvent {
                    button: gpui_kit::MouseButton::Left,
                    position: gpui_kit::point(gpui_kit::px(0.), gpui_kit::px(0.)),
                    modifiers: Default::default(),
                    click_count: 1,
                },
                window,
                cx,
            ),
            Action::Section(section) => {
                self.cancel_provider_operation(cx);
                if self.section == 5
                    && section != 5
                    && (self.mcp.oauth_discovery.is_some() || self.mcp.oauth_flow_id.is_some())
                {
                    self.cancel_mcp_oauth(cx);
                }
                self.request_settings_section(section, cx);
                cx.notify();
            }
        }
        true
    }
    pub(super) fn focus_navigation_from_content(
        &mut self,
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        let Some(page) = &mut self.shell_navigation else {
            return false;
        };
        page.entered_from_content = true;
        window.focus(&page.focus, cx);
        true
    }

    fn shell_keyboard(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(page) = &self.shell_navigation else {
            return;
        };
        let current = self.shell_snapshot(page.id.clone(), cx);
        if !current.authority.visible {
            return;
        }
        let controls = enabled(&current);
        match Key::from_event(event) {
            Key::Next(backwards) => {
                let previous = page
                    .focus
                    .is_focused(window)
                    .then_some(page.keyboard.as_ref())
                    .flatten();
                let next =
                    Selection::advance(previous, &page.keyboard_session, &controls, backwards);
                let focus = page.focus.clone();
                match next {
                    Step::Selected(next) => {
                        if let Some(page) = &mut self.shell_navigation {
                            page.keyboard = Some(next);
                        }
                        window.focus(&focus, cx);
                    }
                    Step::Exit if !backwards && self.focus_first_appearance(window, cx) => {}
                    Step::Exit => crate::nsh::keyboard::leave(&focus, backwards, window, cx),
                }
                cx.stop_propagation();
                cx.notify();
            }
            Key::Activate if page.focus.is_focused(window) => {
                let observed = page.keyboard_observed.clone();
                let action = page
                    .keyboard
                    .as_ref()
                    .and_then(|selection| selection.action(&page.keyboard_session, &controls));
                if let (Some(observed), Some(action)) = (observed, action) {
                    self.shell_keyboard_activate(&observed, action, window, cx);
                }
                cx.stop_propagation();
            }
            _ => {}
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
    let focused = computed(move || {
        state.with(|snapshot| {
            snapshot.keyboard_focus_visible
                && snapshot.keyboard == Some(section.map_or(Action::Back, Action::Section))
        })
    });
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
        .w_full()
        .h(px(Typography::SIDEBAR_LINE_HEIGHT))
        .flex_shrink_0()
        .text_size(px(Typography::SIDEBAR))
        .text_color(move || crate::nsh::color(state.get().colors.text_primary))
        .border(px(1.))
        .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
        .border_color(move || {
            crate::nsh::color(if focused.get() {
                state.get().colors.accent
            } else {
                state.get().colors.bg_base.opacity(0.)
            })
        })
        .bg(move || {
            crate::nsh::color(
                if section.is_some_and(|section| state.get().selected == section) {
                    state.get().colors.bg_active
                } else {
                    state.get().colors.bg_base.opacity(0.)
                },
            )
        })
        .hover(|style| {
            style.bg(move || {
                crate::nsh::color(
                    if section.is_some_and(|section| state.get().selected == section) {
                        state.get().colors.bg_active
                    } else {
                        state.get().colors.bg_hover
                    },
                )
            })
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
                    .line_height(px(Typography::SETTINGS_TITLE * Typography::BODY_LINE_HEIGHT))
                    .font_weight(FontWeight(Typography::HEADING_PAGE_WEIGHT.0 as u16))
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
        .bg(move || crate::nsh::color(state.get().colors.bg_sidebar))
        .border_r_1()
        .border_r_color(move || crate::nsh::color(state.get().colors.border_subtle))
        .child(
            view()
                .w_full()
                .pt(px(Layout::MAIN_HEADER_HEIGHT))
                .px(px(Layout::SIDEBAR_PADDING))
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
