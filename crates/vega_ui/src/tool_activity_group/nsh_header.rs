use super::ToolActivityGroup;
use crate::icons::{Icon, nsh_source};
use crate::nsh::keyboard::Key;
use crate::nsh::{OrderedCommands, ProjectionId, Surface};
use gpui_kit::{
    AccessibleAction, AnyElement, App, Bounds, Context, Element, ElementId, Entity, EntityId,
    FocusHandle, GlobalElementId, InspectorElementId, KeyDownEvent, LayoutId, Pixels, Role,
    Subscription, Window, div, prelude::*, px,
};
use ns::{Accessible as _, Interactive as _, IntoElement as _, ParentElement as _, Styled as _};
use nsh_core::prelude as ns;
use std::rc::{Rc, Weak};
use tokio::sync::mpsc;
use vega_theme::{Layout, ThemeColors, Typography, theme};

pub(super) struct Presentation {
    id: ProjectionId,
    surface: Surface<Snapshot, Request>,
    focus: FocusHandle,
    lease: Weak<()>,
    observed: Snapshot,
    _subscriptions: Vec<Subscription>,
}

#[derive(Clone)]
struct Snapshot {
    id: ProjectionId,
    children: Vec<EntityId>,
    expanded: bool,
    summary: String,
    label: String,
    icon: Icon,
    tint: gpui_kit::Rgba,
    colors: ThemeColors,
    keyboard_focus: bool,
}

impl Snapshot {
    fn same_content(&self, other: &Self) -> bool {
        self.children == other.children
            && self.expanded == other.expanded
            && self.summary == other.summary
            && self.label == other.label
            && self.icon == other.icon
            && self.tint == other.tint
    }

    fn accepts(&self, observed: &Self) -> bool {
        self.id.matches(&observed.id) && self.same_content(observed)
    }
}

struct Request {
    observed: Snapshot,
}

impl ToolActivityGroup {
    pub(super) fn invalidate_header(&mut self) {
        if let Some(page) = &mut self.presentation {
            page.id = ProjectionId::new();
        }
    }

    pub(crate) fn suspend_presentation(&mut self, cx: &App) {
        if let Some(page) = self.presentation.take() {
            page.surface.entity().read(cx).stop();
        }
    }

    fn header_snapshot(&self, id: ProjectionId, cx: &App) -> Snapshot {
        let colors = theme(cx).colors;
        let summary = self.aggregate_summary(cx);
        let label = vega_i18n::t_args(
            "ui.tool_group_disclosure",
            &[
                ("summary", &summary),
                (
                    "action",
                    vega_i18n::t(if self.expanded {
                        "ui.collapse"
                    } else {
                        "ui.expand"
                    }),
                ),
            ],
        );
        Snapshot {
            id,
            children: self.children.iter().map(|card| card.entity_id()).collect(),
            expanded: self.expanded,
            summary,
            label,
            icon: self.leading_icon(cx),
            tint: self.leading_icon_color(cx, &colors),
            colors,
            keyboard_focus: false,
        }
    }

    fn activate_header(
        &mut self,
        observed: &Snapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(page) = &self.presentation else {
            return;
        };
        if page.lease.upgrade().is_none()
            || page.surface.errors().borrow().is_some()
            || !self.header_snapshot(page.id.clone(), cx).accepts(observed)
        {
            return;
        }
        window.focus(&page.focus, cx);
        self.toggle_expanded(cx);
    }

    fn header_key(
        &mut self,
        observed: &Snapshot,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(page) = &self.presentation else {
            return;
        };
        if !page.focus.is_focused(window)
            || page.lease.upgrade().is_none()
            || !self.header_snapshot(page.id.clone(), cx).accepts(observed)
        {
            return;
        }
        match Key::from_event(event) {
            Key::Next(backwards) => {
                crate::nsh::keyboard::leave(&page.focus, backwards, window, cx);
                cx.stop_propagation();
                cx.notify();
            }
            Key::Activate => {
                self.activate_header(observed, window, cx);
                cx.stop_propagation();
            }
            Key::Ignore => {}
        }
    }
}

pub(super) fn render_host(
    group: Entity<ToolActivityGroup>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    group.update(cx, |this, cx| {
        if this.presentation.is_none() {
            let id = ProjectionId::new();
            let observed = this.header_snapshot(id.clone(), cx);
            let focus = cx.focus_handle().tab_stop(true);
            let subscriptions = vec![
                cx.on_focus_in(
                    &focus,
                    window,
                    |this: &mut ToolActivityGroup, window, cx| {
                        if let Some(page) = &this.presentation
                            && page.focus.contains_focused(window, cx)
                            && !page.focus.is_focused(window)
                        {
                            window.focus(&page.focus, cx);
                        }
                        cx.notify();
                    },
                ),
                cx.on_focus_out(&focus, window, |_, _, _, cx| cx.notify()),
            ];
            let (surface, mut commands) = Surface::new(observed.clone(), render, window, cx);
            this.presentation = Some(Presentation {
                id,
                surface,
                focus,
                lease: Weak::new(),
                observed,
                _subscriptions: subscriptions,
            });
            cx.spawn_in(window, async move |this, cx| {
                while let Some(request) = commands.recv().await {
                    if this
                        .update_in(cx, |this, window, cx| {
                            this.activate_header(&request.observed, window, cx);
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();
        }
        let page = this.presentation.as_ref();
        let Some(page) = page else {
            return div().into_any_element();
        };
        let lease = page.lease.upgrade().unwrap_or_else(|| Rc::new(()));
        let mut observed = this.header_snapshot(page.id.clone(), cx);
        if page.lease.upgrade().is_none() || !page.observed.same_content(&observed) {
            observed.id = ProjectionId::new();
        }
        let Some(page) = &mut this.presentation else {
            return div().into_any_element();
        };
        page.id = observed.id.clone();
        page.lease = Rc::downgrade(&lease);
        observed.keyboard_focus = page.focus.is_focused(window) && window.last_input_was_keyboard();
        page.observed = observed.clone();
        page.surface.sync_environment(cx);
        page.surface.publish(observed.clone());
        let keyboard_observed = observed.clone();
        let accessibility_observed = observed.clone();
        let focus = page.focus.clone();
        let weak = cx.weak_entity();
        let entity_id = cx.entity_id();
        let host = div()
            .id(("nsh-tool-group-header-host", entity_id))
            .relative()
            .w_full()
            .min_w_0()
            .h(px(Layout::CONVERSATION_TOOL_HEIGHT))
            .flex_shrink_0()
            .track_focus(&focus)
            .role(Role::Group)
            .capture_key_down(cx.listener(move |this, event, window, cx| {
                this.header_key(&keyboard_observed, event, window, cx);
            }))
            .child(page.surface.entity())
            .when(focus.is_focused(window), |host| {
                host.child(
                    div()
                        .id("nsh-tool-group-header-active")
                        .absolute()
                        .inset_0()
                        .role(Role::Button)
                        .aria_label(observed.label)
                        .aria_expanded(observed.expanded)
                        .aria_active_descendant()
                        .on_a11y_action(AccessibleAction::Click, move |_, window, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                if this
                                    .presentation
                                    .as_ref()
                                    .is_some_and(|page| page.focus.is_focused(window))
                                {
                                    this.activate_header(&accessibility_observed, window, cx);
                                }
                            });
                        }),
                )
            });
        Mounted {
            child: host.into_any_element(),
            lease,
            entity_id,
        }
        .into_any_element()
    })
}

struct Mounted {
    child: AnyElement,
    lease: Rc<()>,
    entity_id: EntityId,
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
        Some(("tool-group-header-mounted", self.entity_id).into())
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
    let commands = OrderedCommands::new(commands);
    let hover = ns::signal(false);
    let down = ns::signal(false);
    let pressed = ns::signal_with(None::<Snapshot>, ns::SignalEquals::Never);
    ns::view()
        .w_full()
        .min_w(ns::px(0.))
        .h(ns::px(Layout::CONVERSATION_TOOL_HEIGHT))
        .flex_row()
        .items_center()
        .gap(ns::px(Layout::CONVERSATION_INLINE_GAP))
        .text_size(ns::px(Typography::CONVERSATION))
        .line_height(ns::px(
            Typography::CONVERSATION * Typography::CONVERSATION_LINE_HEIGHT,
        ))
        .text_color(move || crate::nsh::color(state.get().colors.conversation_tool))
        .cursor_pointer()
        .role(nsh_core::AccessibilityRole::Button)
        .label(move || state.get().label)
        .accessibility_state(move || nsh_core::AccessibilityState {
            expanded: Some(state.get().expanded),
            ..Default::default()
        })
        .bg(move || {
            let observed = state.get();
            let colors = observed.colors;
            crate::nsh::color(if down.get() {
                colors.bg_active
            } else if hover.get() || observed.keyboard_focus {
                colors.bg_hover
            } else {
                colors.bg_base.opacity(0.)
            })
        })
        .on_mouse_enter(move |_: &nsh_core::MouseEvent| hover.set(true))
        .on_mouse_leave(move |_: &nsh_core::MouseEvent| {
            hover.set(false);
            down.set(false);
            pressed.set(None);
        })
        .on_press_down(move || {
            down.set(true);
            pressed.set(Some(state.get_untracked()));
        })
        .on_press_up(move || down.set(false))
        .on_press(move || {
            let observed = pressed
                .get_untracked()
                .unwrap_or_else(|| state.get_untracked());
            pressed.set(None);
            commands.send(Request { observed });
        })
        .child(ns::dynamic(move || {
            let observed = state.get();
            glyph(observed.icon, observed.tint, Layout::ICON_SIZE)
        }))
        .child(
            ns::text(move || state.get().summary)
                .min_w(ns::px(0.))
                .flex_shrink(1.)
                .truncate(),
        )
        .child(
            ns::view()
                .size(ns::px(Layout::CONVERSATION_CHEVRON_SIZE))
                .flex_shrink_0()
                .opacity(move || {
                    let observed = state.get();
                    if observed.expanded || observed.keyboard_focus || hover.get() {
                        1_f32
                    } else {
                        0_f32
                    }
                })
                .child(ns::dynamic(move || {
                    let observed = state.get();
                    glyph(
                        if observed.expanded {
                            Icon::ChevronDown
                        } else {
                            Icon::ChevronRight
                        },
                        observed.colors.conversation_tool,
                        Layout::CONVERSATION_CHEVRON_SIZE,
                    )
                })),
        )
        .into_element()
}

fn glyph(kind: Icon, tint: gpui_kit::Rgba, size: f32) -> ns::AnyElement {
    let source = nsh_source(kind).unwrap_or_default();
    ns::svg(nsh_core::element::ImageSrc::svg_markup(&source))
        .size(ns::px(size))
        .flex_shrink_0()
        .text_color(crate::nsh::color(tint))
        .tint_color(crate::nsh::color(tint))
        .into_element()
}

#[cfg(test)]
mod tests;
