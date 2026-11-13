use super::*;
use crate::nsh::{OrderedCommands, ProjectionId, Surface};
use gpui_kit::{
    AccessibleAction, Element, ElementId, GlobalElementId, InspectorElementId, LayoutId,
};
use ns::{Accessible as _, Interactive as _, IntoElement as _, ParentElement as _, Styled as _};
use nsh_core::prelude as ns;
use std::rc::{Rc, Weak};
use tokio::sync::mpsc;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Open,
    Reload,
}

#[derive(Clone)]
struct Snapshot {
    id: ProjectionId,
    source: String,
    parent: String,
    recovery: bool,
    focused: bool,
    colors: ThemeColors,
}

impl Snapshot {
    fn action(&self) -> Action {
        if self.recovery {
            Action::Reload
        } else {
            Action::Open
        }
    }

    fn accepts(&self, observed: &Self, action: Action, mounted: bool, visible: bool) -> bool {
        mounted
            && visible
            && self.id.matches(&observed.id)
            && self.source == observed.source
            && self.parent == observed.parent
            && self.recovery == observed.recovery
            && action == self.action()
    }
}

struct Request {
    observed: Snapshot,
    action: Action,
}

#[derive(Default)]
pub(crate) struct ParentNavigation {
    surface: Option<Surface<Snapshot, Request>>,
    snapshot: Option<Snapshot>,
    lease: Weak<()>,
    subscriptions: Vec<gpui_kit::Subscription>,
}

impl ParentNavigation {
    pub(super) fn hide(&mut self, cx: &App) {
        if let Some(surface) = self.surface.take() {
            surface.entity().read(cx).stop();
        }
        self.snapshot = None;
        self.lease = Weak::new();
    }
}

impl ConversationStream {
    fn parent_request(&mut self, request: Request, window: &mut Window, cx: &mut Context<Self>) {
        let visible = !self.reading_preview
            && cx
                .try_global::<SettingsOpen>()
                .is_some_and(|settings| !settings.0)
            && self.thread.id == request.observed.source
            && self.thread.parent_thread_id.as_deref() == Some(request.observed.parent.as_str());
        let Some(current) = &self.parent_navigation.snapshot else {
            return;
        };
        if !current.accepts(
            &request.observed,
            request.action,
            self.parent_navigation.lease.upgrade().is_some(),
            visible,
        ) {
            return;
        }
        let failed = self
            .parent_navigation
            .surface
            .as_ref()
            .is_none_or(|surface| surface.errors().borrow().is_some());
        if request.action == Action::Reload {
            self.parent_navigation.hide(cx);
            window.focus(&self.subagent_parent_focus, cx);
            cx.notify();
        } else if !failed {
            cx.emit(vega_conversation::types::SubagentOpenRequested {
                source_thread_id: self.thread.id.clone(),
                thread_id: request.observed.parent,
            });
        }
    }

    pub(super) fn render_parent_navigation(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let Some(parent) = self.thread.parent_thread_id.clone() else {
            self.parent_navigation.hide(cx);
            return None;
        };
        if self.reading_preview
            || cx
                .try_global::<SettingsOpen>()
                .is_none_or(|settings| settings.0)
        {
            self.parent_navigation.hide(cx);
            return None;
        }
        if self.parent_navigation.subscriptions.is_empty() {
            self.parent_navigation.subscriptions = vec![
                cx.on_focus_in(&self.subagent_parent_focus, window, |this, window, cx| {
                    window.focus(&this.subagent_parent_focus, cx);
                    cx.notify();
                }),
                cx.on_focus_out(&self.subagent_parent_focus, window, |_, _, _, cx| {
                    cx.notify()
                }),
            ];
        }
        let route_changed = self
            .parent_navigation
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.source != self.thread.id || snapshot.parent != parent);
        if route_changed {
            self.parent_navigation.hide(cx);
        }
        let lease = self.parent_navigation.lease.upgrade().unwrap_or_else(|| {
            self.parent_navigation.hide(cx);
            Rc::new(())
        });
        self.parent_navigation.lease = Rc::downgrade(&lease);
        let failed = self
            .parent_navigation
            .surface
            .as_ref()
            .is_some_and(|surface| surface.errors().borrow().is_some());
        let mut snapshot = self
            .parent_navigation
            .snapshot
            .clone()
            .unwrap_or_else(|| Snapshot {
                id: ProjectionId::new(),
                source: self.thread.id.clone(),
                parent,
                recovery: false,
                focused: false,
                colors: theme(cx).colors,
            });
        if failed && !snapshot.recovery {
            if let Some(surface) = self.parent_navigation.surface.take() {
                surface.entity().read(cx).stop();
            }
            snapshot.id = ProjectionId::new();
            snapshot.recovery = true;
        }
        snapshot.focused = self.subagent_parent_focus.is_focused(window);
        snapshot.colors = theme(cx).colors;
        self.parent_navigation.snapshot = Some(snapshot.clone());
        if self.parent_navigation.surface.is_none() {
            let (surface, mut requests) = Surface::new(snapshot.clone(), render, window, cx);
            let mut errors = surface.errors();
            cx.spawn(async move |this, cx| {
                while let Some(request) = requests.recv().await {
                    if this
                        .update_in(cx, |this, window, cx| {
                            this.parent_request(request, window, cx)
                        })
                        .is_err()
                    {
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
            self.parent_navigation.surface = Some(surface);
        }
        let surface = self.parent_navigation.surface.as_mut()?;
        surface.sync_environment(cx);
        surface.publish(snapshot.clone());
        let keyboard = snapshot.clone();
        let accessibility = snapshot.clone();
        let label = if snapshot.recovery {
            vega_i18n::t("ui.reload_view")
        } else {
            vega_i18n::t("ui.back_to_parent")
        };
        let weak = cx.weak_entity();
        let host = div()
            .id("subagent-parent-host")
            .relative()
            .w_full()
            .h(px(Layout::SETTINGS_CONTROL_MIN_HEIGHT))
            .track_focus(&self.subagent_parent_focus)
            .tab_stop(true)
            .role(gpui_kit::Role::Group)
            .capture_key_down(cx.listener(
                move |this, event: &gpui_kit::KeyDownEvent, window, cx| {
                    if this.subagent_parent_focus.contains_focused(window, cx) {
                        match crate::nsh::keyboard::Key::from_event(event) {
                            crate::nsh::keyboard::Key::Activate => {
                                this.parent_request(
                                    Request {
                                        observed: keyboard.clone(),
                                        action: keyboard.action(),
                                    },
                                    window,
                                    cx,
                                );
                                cx.stop_propagation();
                            }
                            crate::nsh::keyboard::Key::Next(backwards) => {
                                crate::nsh::keyboard::leave(
                                    &this.subagent_parent_focus,
                                    backwards,
                                    window,
                                    cx,
                                );
                                cx.stop_propagation();
                            }
                            _ => {}
                        }
                    }
                },
            ))
            .child(surface.entity())
            .child(
                div()
                    .id("subagent-parent-active")
                    .absolute()
                    .inset_0()
                    .role(gpui_kit::Role::Button)
                    .aria_label(label)
                    .aria_active_descendant()
                    .on_a11y_action(AccessibleAction::Click, move |_, window, cx| {
                        let _ = weak.update(cx, |this, cx| {
                            this.parent_request(
                                Request {
                                    observed: accessibility.clone(),
                                    action: accessibility.action(),
                                },
                                window,
                                cx,
                            );
                        });
                    }),
            );
        Some(
            Mounted {
                child: host.into_any_element(),
                lease,
            }
            .into_any_element(),
        )
    }
}

fn render(state: ns::Signal<Snapshot>, sender: mpsc::Sender<Request>) -> ns::AnyElement {
    let commands = OrderedCommands::new(sender);
    ns::dynamic(move || {
        let snapshot = state.get();
        let colors = snapshot.colors;
        let hover = ns::signal(false);
        let pressed = ns::signal(false);
        let activate = commands.clone();
        let accessible_commands = commands.clone();
        let accessible_observed = snapshot.clone();
        let failed = commands.failed;
        let observed = snapshot.clone();
        let label = if snapshot.recovery {
            vega_i18n::t("ui.reload_view")
        } else {
            vega_i18n::t("ui.back_to_parent")
        };
        ns::view()
            .w_full()
            .h(ns::px(Layout::SETTINGS_CONTROL_MIN_HEIGHT))
            .flex_row()
            .items_center()
            .px(ns::px(8.))
            .rounded(ns::px(Layout::SETTINGS_CONTROL_RADIUS))
            .border(ns::px(1.))
            .border_color(vega_ui_color(if snapshot.focused {
                colors.accent
            } else {
                colors.bg_base.opacity(0.)
            }))
            .bg(move || {
                vega_ui_color(if pressed.get() {
                    colors.bg_active
                } else if hover.get() {
                    colors.bg_hover
                } else {
                    colors.bg_base
                })
            })
            .role(nsh_core::AccessibilityRole::Button)
            .label(label)
            .accessibility_actions(vec![nsh_core::AccessibilityActionDef {
                name: "activate".into(),
                label: None,
            }])
            .on_accessibility_action(move |name: &str| {
                if name == "activate" {
                    accessible_commands.send(Request {
                        observed: accessible_observed.clone(),
                        action: accessible_observed.action(),
                    });
                }
            })
            .on_mouse_enter(move |_: &nsh_core::MouseEvent| hover.set(true))
            .on_mouse_leave(move |_: &nsh_core::MouseEvent| {
                hover.set(false);
                pressed.set(false);
            })
            .on_press_down(move || pressed.set(true))
            .on_press_up(move || pressed.set(false))
            .on_press(move || {
                activate.send(Request {
                    action: observed.action(),
                    observed: observed.clone(),
                })
            })
            .child(
                ns::text(label)
                    .text_size(ns::px(Typography::BODY))
                    .text_color(vega_ui_color(colors.text_secondary)),
            )
            .child(ns::dynamic(move || {
                failed.get().then(|| {
                    ns::text(vega_i18n::t("ui.command_failed"))
                        .text_color(vega_ui_color(colors.danger))
                })
            }))
    })
    .into_element()
}

fn vega_ui_color(color: gpui_kit::Rgba) -> nsh_core::Rgba {
    crate::nsh::color(color)
}

struct Mounted {
    child: AnyElement,
    lease: Rc<()>,
}

impl gpui_kit::IntoElement for Mounted {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for Mounted {
    type RequestLayoutState = ();
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        Some("subagent-parent-mounted".into())
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
        _: gpui_kit::Bounds<Pixels>,
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
        _: gpui_kit::Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
    }
}
