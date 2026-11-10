use super::{NavigateBack, NavigateForward, NavigationState, TaskMutationState};
use crate::icons::{Icon, nsh_source};
use crate::nsh::keyboard::{Key, Selection, Step};
use crate::nsh::{ProjectionId, Surface};
use gpui_kit::{prelude::*, *};
use ns::{Accessible as _, Interactive as _, IntoElement as _, ParentElement as _, Styled as _};
use nsh_core::prelude as ns;
use std::rc::{Rc, Weak};
use tokio::sync::mpsc;
use vega_theme::{Layout, ThemeColors};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Sidebar,
    Search,
    Back,
    Forward,
    Reload,
}

impl Action {
    fn label(self, sidebar: bool) -> &'static str {
        vega_i18n::t(match self {
            Self::Sidebar if sidebar => "ui.hide_sidebar_b",
            Self::Sidebar => "ui.show_sidebar_b",
            Self::Search => "ui.search_k",
            Self::Back => "ui.back",
            Self::Forward => "ui.forward",
            Self::Reload => "ui.reload_view",
        })
    }

    fn icon(self) -> Icon {
        match self {
            Self::Sidebar => Icon::Sidebar,
            Self::Search => Icon::Search,
            Self::Back => Icon::ArrowLeft,
            Self::Forward => Icon::ArrowRight,
            Self::Reload => Icon::ArrowLeft,
        }
    }
}

#[derive(Clone)]
struct Snapshot {
    id: ProjectionId,
    navigation: NavigationState,
    mutation: TaskMutationState,
    task: Option<String>,
    project: Option<String>,
    sidebar: bool,
    only_sidebar: bool,
    recovery: bool,
    settings_open: bool,
    collapsed: bool,
    selected: Option<Action>,
    colors: ThemeColors,
}

impl Snapshot {
    fn actions(&self) -> Vec<Action> {
        if self.recovery {
            return vec![Action::Reload];
        }
        let mut actions = vec![Action::Sidebar];
        if !self.only_sidebar {
            actions.push(Action::Search);
            if self.navigation.back {
                actions.push(Action::Back);
            }
            if self.navigation.forward {
                actions.push(Action::Forward);
            }
        }
        actions
    }

    fn accepts(&self, observed: &Self, action: Action) -> bool {
        self.id.matches(&observed.id)
            && self.navigation == observed.navigation
            && self.mutation == observed.mutation
            && self.task == observed.task
            && self.project == observed.project
            && self.sidebar == observed.sidebar
            && self.only_sidebar == observed.only_sidebar
            && self.recovery == observed.recovery
            && !self.settings_open
            && self.settings_open == observed.settings_open
            && self.collapsed == observed.collapsed
            && self.actions().contains(&action)
    }
}

struct Request {
    observed: Snapshot,
    action: Action,
}

struct Controls {
    surface: Option<Surface<Snapshot, Request>>,
    snapshot: Snapshot,
    focus: FocusHandle,
    selected: Option<Selection<Action>>,
    lease: Weak<()>,
    pointer_entry: Option<Action>,
    _subscriptions: Vec<Subscription>,
}

impl Controls {
    fn current(&self, cx: &App) -> Snapshot {
        Snapshot {
            navigation: cx
                .try_global::<NavigationState>()
                .cloned()
                .unwrap_or_default(),
            mutation: cx
                .try_global::<TaskMutationState>()
                .copied()
                .unwrap_or_default(),
            task: cx
                .try_global::<super::DraftNavigationGuard>()
                .map(|guard| guard.task.clone()),
            project: cx
                .try_global::<crate::sidebar::SelectedProject>()
                .and_then(|project| project.0.clone()),
            settings_open: cx
                .try_global::<crate::settings::SettingsOpen>()
                .is_none_or(|state| state.0),
            collapsed: cx
                .try_global::<crate::sidebar::SidebarCollapsed>()
                .is_some_and(|state| state.0),
            colors: vega_theme::theme(cx).colors,
            ..self.snapshot.clone()
        }
    }

    fn activate(
        &mut self,
        observed: &Snapshot,
        action: Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.lease.upgrade().is_none()
            || self
                .surface
                .as_ref()
                .is_some_and(|surface| surface.errors().borrow().is_some())
            || !self.current(cx).accepts(observed, action)
        {
            return;
        }
        if let Step::Selected(selected) = Selection::advance(None, &observed.id, &[action], false) {
            self.selected = Some(selected);
        }
        if !self.focus.contains_focused(window, cx) {
            self.pointer_entry = Some(action);
        }
        window.focus(&self.focus, cx);
        cx.notify();
        match action {
            Action::Sidebar => window.dispatch_action(Box::new(crate::sidebar::ToggleSidebar), cx),
            Action::Search => {
                window.dispatch_action(Box::new(crate::command_palette::OpenPalette), cx)
            }
            Action::Back => window.dispatch_action(Box::new(NavigateBack), cx),
            Action::Forward => window.dispatch_action(Box::new(NavigateForward), cx),
            Action::Reload => {
                if let Some(surface) = self.surface.take() {
                    surface.entity().read(cx).stop();
                }
                self.snapshot.id = ProjectionId::new();
                self.snapshot.recovery = false;
                self.selected = None;
                cx.notify();
            }
        }
    }

    fn mount(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.surface.is_some() {
            return;
        }
        let (surface, mut requests) = Surface::new(self.snapshot.clone(), render, window, cx);
        let mut errors = surface.errors();
        cx.spawn(async move |this, cx| {
            while let Some(request) = requests.recv().await {
                let result = this.update_in(cx, |this, window, cx| {
                    this.activate(&request.observed, request.action, window, cx)
                });
                if result.is_err() {
                    break;
                }
            }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            while errors.changed().await.is_ok() {
                if errors.borrow_and_update().is_some() {
                    let _ = this.update(cx, |_, cx| cx.notify());
                    break;
                }
            }
        })
        .detach();
        self.surface = Some(surface);
    }

    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.current(cx);
        match Key::from_event(event) {
            Key::Next(backwards) => {
                let previous = self
                    .focus
                    .is_focused(window)
                    .then_some(self.selected.as_ref())
                    .flatten();
                match Selection::advance(previous, &current.id, &current.actions(), backwards) {
                    Step::Selected(selected) => {
                        self.selected = Some(selected);
                        window.focus(&self.focus, cx);
                    }
                    Step::Exit => crate::nsh::keyboard::leave(&self.focus, backwards, window, cx),
                }
                cx.stop_propagation();
                cx.notify();
            }
            Key::Activate if self.focus.is_focused(window) => {
                if let Some(action) = self
                    .selected
                    .as_ref()
                    .and_then(|selected| selected.action(&current.id, &current.actions()))
                {
                    self.activate(&self.snapshot.clone(), action, window, cx);
                }
                cx.stop_propagation();
            }
            _ => {}
        }
    }
}

pub(super) fn controls(
    window: &mut Window,
    cx: &mut App,
    sidebar: bool,
    only_sidebar: bool,
) -> AnyElement {
    let state = window.use_keyed_state(
        if only_sidebar {
            "nsh-sidebar-only"
        } else {
            "nsh-window-navigation"
        },
        cx,
        |window, cx| {
            let focus = cx.focus_handle().tab_stop(true);
            let subscriptions = vec![
                cx.on_focus_in(&focus, window, |this: &mut Controls, window, cx| {
                    if this.focus.contains_focused(window, cx) {
                        let current = this.current(cx);
                        let actions = this
                            .pointer_entry
                            .take()
                            .filter(|action| current.actions().contains(action))
                            .map(|action| vec![action])
                            .unwrap_or_else(|| current.actions());
                        if let Step::Selected(selected) = Selection::advance(
                            None,
                            &current.id,
                            &actions,
                            window.modifiers().shift,
                        ) {
                            this.selected = Some(selected);
                        }
                        window.focus(&this.focus, cx);
                    }
                    cx.notify();
                }),
                cx.on_focus_out(&focus, window, |_, _, _, cx| cx.notify()),
            ];
            Controls {
                surface: None,
                snapshot: Snapshot {
                    id: ProjectionId::new(),
                    navigation: NavigationState::default(),
                    mutation: TaskMutationState::default(),
                    task: None,
                    project: None,
                    sidebar,
                    only_sidebar,
                    recovery: false,
                    settings_open: false,
                    collapsed: false,
                    selected: None,
                    colors: vega_theme::theme(cx).colors,
                },
                focus,
                selected: None,
                lease: Weak::new(),
                pointer_entry: None,
                _subscriptions: subscriptions,
            }
        },
    );
    state.update(cx, |this, cx| {
        let lease = match this.lease.upgrade() {
            Some(lease) => lease,
            None => {
                this.snapshot.id = ProjectionId::new();
                this.selected = None;
                Rc::new(())
            }
        };
        this.lease = Rc::downgrade(&lease);
        if this
            .surface
            .as_ref()
            .is_some_and(|surface| surface.errors().borrow().is_some())
        {
            if let Some(surface) = this.surface.take() {
                surface.entity().read(cx).stop();
            }
            this.snapshot.id = ProjectionId::new();
            this.snapshot.recovery = true;
            this.selected = None;
        }
        if this.snapshot.sidebar != sidebar {
            this.snapshot.id = ProjectionId::new();
            this.snapshot.sidebar = sidebar;
            this.selected = None;
        }
        let mut snapshot = this.current(cx);
        this.selected =
            Selection::reconcile(this.selected.as_ref(), &snapshot.id, &snapshot.actions());
        snapshot.selected = this
            .focus
            .is_focused(window)
            .then(|| {
                this.selected
                    .as_ref()
                    .and_then(|selected| selected.action(&snapshot.id, &snapshot.actions()))
            })
            .flatten();
        this.snapshot = snapshot.clone();
        this.mount(window, cx);
        let Some(surface) = &mut this.surface else {
            return div().into_any_element();
        };
        surface.sync_environment(cx);
        surface.publish(snapshot.clone());
        let mut host = div()
            .id("nsh-window-navigation-host")
            .relative()
            .track_focus(&this.focus)
            .role(Role::Group)
            .w(px(if only_sidebar {
                Layout::TITLEBAR_CONTROL_SIZE
            } else {
                4. * Layout::TITLEBAR_CONTROL_SIZE + 3. * Layout::TITLEBAR_CONTROL_GAP
            }))
            .h(px(Layout::TITLEBAR_CONTROL_SIZE))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .capture_key_down(cx.listener(|this, event, window, cx| this.key(event, window, cx)))
            .child(surface.entity());
        if let Some(action) = snapshot.selected {
            let weak = cx.weak_entity();
            host = host.child(
                div()
                    .id("nsh-window-navigation-active")
                    .absolute()
                    .inset_0()
                    .role(Role::Button)
                    .aria_label(action.label(sidebar))
                    .aria_active_descendant()
                    .on_a11y_action(AccessibleAction::Click, move |_, window, cx| {
                        let _ = weak.update(cx, |this, cx| {
                            if this.focus.is_focused(window)
                                && this.selected.as_ref().and_then(|selected| {
                                    selected.action(&snapshot.id, &snapshot.actions())
                                }) == Some(action)
                            {
                                this.activate(&snapshot, action, window, cx);
                            }
                        });
                    }),
            );
        }
        Mounted {
            child: host.into_any_element(),
            lease,
        }
        .into_any_element()
    })
}

struct Mounted {
    child: AnyElement,
    lease: Rc<()>,
}

impl IntoElement for Mounted {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for Mounted {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some("navigation-mounted".into())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(id) = id {
            window.with_element_state::<Rc<()>, _>(id, |_, _| ((), self.lease.clone()));
        }
        self.child.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
    }
}

fn render(state: ns::Signal<Snapshot>, commands: mpsc::Sender<Request>) -> ns::AnyElement {
    let commands = crate::nsh::OrderedCommands::new(commands);
    ns::view()
        .flex_row()
        .items_center()
        .gap(ns::px(Layout::TITLEBAR_CONTROL_GAP))
        .child(ns::dynamic(move || {
            let observed = state.get();
            let actions = if observed.recovery {
                vec![Action::Reload]
            } else if observed.only_sidebar {
                vec![Action::Sidebar]
            } else {
                vec![
                    Action::Sidebar,
                    Action::Search,
                    Action::Back,
                    Action::Forward,
                ]
            };
            actions
                .into_iter()
                .map(|action| {
                    let enabled = observed.actions().contains(&action);
                    let selected = observed.selected == Some(action);
                    let colors = observed.colors;
                    let hover = ns::signal(false);
                    let down = ns::signal(false);
                    let displayed = observed.clone();
                    let sender = commands.clone();
                    let source = nsh_source(action.icon()).unwrap_or_default();
                    ns::view()
                        .w(ns::px(Layout::TITLEBAR_CONTROL_SIZE))
                        .h(ns::px(Layout::TITLEBAR_CONTROL_SIZE))
                        .flex_row()
                        .items_center()
                        .justify_center()
                        .rounded(ns::px(6.))
                        .role(nsh_core::AccessibilityRole::Button)
                        .label(action.label(observed.sidebar))
                        .disabled(!enabled)
                        .accessibility_state(nsh_core::AccessibilityState {
                            disabled: !enabled,
                            ..Default::default()
                        })
                        .tooltip(
                            ns::view()
                                .px_2()
                                .py_1()
                                .rounded(ns::px(Layout::PANEL_RADIUS))
                                .bg(crate::nsh::color(colors.bg_elevated))
                                .child(
                                    ns::text(action.label(observed.sidebar))
                                        .text_color(crate::nsh::color(colors.text_primary)),
                                ),
                        )
                        .border(ns::px(2.))
                        .border_color(crate::nsh::color(if selected {
                            colors.accent
                        } else {
                            colors.bg_base.opacity(0.)
                        }))
                        .bg(move || {
                            crate::nsh::color(if enabled && down.get() {
                                colors.bg_active
                            } else if enabled && hover.get() {
                                colors.bg_hover
                            } else {
                                colors.bg_base.opacity(0.)
                            })
                        })
                        .on_mouse_enter(move |_: &nsh_core::MouseEvent| hover.set(true))
                        .on_mouse_leave(move |_: &nsh_core::MouseEvent| {
                            hover.set(false);
                            down.set(false);
                        })
                        .on_press_down(move || down.set(true))
                        .on_press_up(move || down.set(false))
                        .on_press(move || {
                            if enabled {
                                sender.send(Request {
                                    observed: displayed.clone(),
                                    action,
                                });
                            }
                        })
                        .child(
                            ns::svg(nsh_core::element::ImageSrc::svg_markup(&source))
                                .w(ns::px(Layout::ICON_SIZE))
                                .h(ns::px(Layout::ICON_SIZE))
                                .text_color(crate::nsh::color(if enabled {
                                    colors.text_secondary
                                } else {
                                    colors.text_tertiary
                                })),
                        )
                        .into_element()
                })
                .collect::<Vec<_>>()
        }))
        .into_element()
}
