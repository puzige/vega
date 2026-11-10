use super::{GroupHost, GroupMount, Source};
use crate::nsh::{ProjectionId, Surface};
use gpui_kit::{prelude::*, *};
use nsh_platform_gpui::NativeComponent;
use std::cell::Cell;
use std::rc::{Rc, Weak};
use vega_theme::ThemeColors;

#[derive(Clone, Copy, PartialEq)]
struct Geometry {
    bounds: Bounds<Pixels>,
    visible: Bounds<Pixels>,
    parent: Bounds<Pixels>,
    viewport: Bounds<Pixels>,
    foreground: Pixels,
    window: Size<Pixels>,
}

#[derive(Clone)]
struct Snapshot {
    id: ProjectionId,
    source: Source,
    geometry: Option<Geometry>,
    focused: bool,
    colors: ThemeColors,
}

pub(super) struct Removal {
    owner: WeakEntity<super::ConversationStream>,
    source: Source,
    group: WeakEntity<GroupHost>,
    focus: Option<FocusHandle>,
}

impl Removal {
    pub(super) fn new(
        owner: WeakEntity<super::ConversationStream>,
        source: Source,
        group: WeakEntity<GroupHost>,
    ) -> Self {
        Self {
            owner,
            source,
            group,
            focus: None,
        }
    }
}

impl NativeComponent for Removal {
    fn render(&mut self, window: &mut Window, cx: &mut App) -> Option<AnyElement> {
        let group = self.group.upgrade()?;
        let parent = group.read(cx).lease.clone();
        let key = format!(
            "nsh-attachment-remove-{}-{}-{}",
            self.source.image, self.source.attachment, self.source.generation
        );
        let host = window.use_keyed_state(key, cx, |window, cx| {
            let focus = cx.focus_handle().tab_stop(true);
            let subscriptions = vec![
                cx.on_release(|this: &mut RemoveHost, cx| this.stop(cx)),
                cx.on_focus_in(&focus, window, |_, _, cx| cx.notify()),
                cx.on_focus_out(&focus, window, |_, _, _, cx| cx.notify()),
            ];
            RemoveHost {
                owner: self.owner.clone(),
                parent: parent.clone(),
                group: self.group.clone(),
                snapshot: Snapshot {
                    id: ProjectionId::new(),
                    source: self.source.clone(),
                    geometry: None,
                    focused: false,
                    colors: vega_theme::theme(cx).colors,
                },
                surface: None,
                focus,
                lease: Weak::new(),
                _subscriptions: subscriptions,
            }
        });
        host.update(cx, |this, cx| {
            if !this.snapshot.source.matches(&self.source) || !this.parent.ptr_eq(&parent) {
                this.stop(cx);
                this.snapshot.source = self.source.clone();
                this.parent = parent;
                this.group = self.group.clone();
            }
        });
        self.focus = Some(host.read(cx).focus.clone());
        Some(host.into_any_element())
    }
    fn focus_handle(&self) -> Option<FocusHandle> {
        self.focus.clone()
    }
    fn set_prop(&mut self, _: &str, _: Option<&serde_json::Value>, _: &mut Window, _: &mut App) {}
}

#[derive(Default)]
struct Mount {
    geometry: Cell<Option<Geometry>>,
}

fn accepts_group_mount(
    parent: &Weak<GroupMount>,
    current: &Weak<GroupMount>,
    errors: Option<tokio::sync::watch::Receiver<Option<String>>>,
) -> bool {
    parent.ptr_eq(current)
        && parent.upgrade().is_some_and(|mount| mount.active.get())
        && errors.is_some_and(|errors| errors.borrow().is_none())
}

struct RemoveHost {
    owner: WeakEntity<super::ConversationStream>,
    parent: Weak<GroupMount>,
    group: WeakEntity<GroupHost>,
    snapshot: Snapshot,
    surface: Option<Surface<Snapshot, Snapshot>>,
    focus: FocusHandle,
    lease: Weak<Mount>,
    _subscriptions: Vec<Subscription>,
}

impl RemoveHost {
    fn stop(&mut self, cx: &App) {
        if let Some(surface) = self.surface.take() {
            surface.entity().read(cx).stop();
        }
        if let Some(mount) = self.lease.upgrade() {
            mount.geometry.set(None);
        }
        self.snapshot.id = ProjectionId::new();
        self.snapshot.geometry = None;
        self.lease = Weak::new();
    }

    fn activate(&mut self, observed: &Snapshot, window: &mut Window, cx: &mut Context<Self>) {
        let Some(owner) = self.owner.upgrade() else {
            self.stop(cx);
            return;
        };
        if !self.snapshot.id.matches(&observed.id)
            || !self.snapshot.source.matches(&observed.source)
            || !observed.source.current(owner.read(cx), cx)
            || !window.is_window_active()
            || !self.group.upgrade().is_some_and(|group| {
                let group = group.read(cx);
                accepts_group_mount(
                    &self.parent,
                    &group.lease,
                    group.surface.as_ref().map(Surface::errors),
                )
            })
            || self
                .surface
                .as_ref()
                .is_none_or(|surface| surface.errors().borrow().is_some())
        {
            return;
        }
        let Some(geometry) = self.lease.upgrade().and_then(|mount| mount.geometry.get()) else {
            return;
        };
        let parent = self
            .parent
            .upgrade()
            .filter(|mount| mount.active.get())
            .and_then(|mount| mount.visible.get());
        if observed.geometry != Some(geometry)
            || parent != Some(geometry.parent)
            || geometry.visible.is_empty()
            || geometry.window != window.viewport_size()
        {
            return;
        }
        owner.update(cx, |stream, cx| {
            if observed.source.current(stream, cx)
                && geometry.viewport == stream.list.viewport_bounds()
                && geometry.foreground == stream.foreground_height
            {
                stream.remove_image(observed.source.attachment, cx);
            }
        });
    }

    fn mount(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.surface.is_some() {
            return;
        }
        let (surface, mut requests) = Surface::new(self.snapshot.clone(), render, window, cx);
        cx.spawn(async move |this, cx| {
            while let Some(observed) = requests.recv().await {
                if this
                    .update_in(cx, |this, window, cx| this.activate(&observed, window, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        let mut errors = surface.errors();
        let id = self.snapshot.id.clone();
        cx.spawn(async move |this, cx| {
            while errors.changed().await.is_ok() {
                if errors.borrow_and_update().is_some() {
                    let _ = this.update(cx, |this, cx| {
                        if !this.snapshot.id.matches(&id) {
                            return;
                        }
                        if let Some(group) = this.group.upgrade() {
                            group.update(cx, |group, cx| {
                                group.stop(cx);
                                group.snapshot.recovery = true;
                                cx.notify();
                            });
                        }
                        this.stop(cx);
                        cx.notify();
                    });
                    break;
                }
            }
        })
        .detach();
        self.surface = Some(surface);
    }
}

impl Render for RemoveHost {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(owner) = self.owner.upgrade() else {
            self.stop(cx);
            return div().into_any_element();
        };
        if !self.snapshot.source.current(owner.read(cx), cx)
            || !self
                .parent
                .upgrade()
                .is_some_and(|mount| mount.active.get())
        {
            self.stop(cx);
            return div().into_any_element();
        }
        let lease = self.lease.upgrade().unwrap_or_else(|| {
            self.stop(cx);
            Rc::new(Mount::default())
        });
        self.lease = Rc::downgrade(&lease);
        self.snapshot.focused = self.focus.is_focused(window);
        self.snapshot.colors = vega_theme::theme(cx).colors;
        self.mount(window, cx);
        let weak = cx.weak_entity();
        let observed = self.snapshot.clone();
        let ax_lease = Rc::downgrade(&lease);
        let mut host = div()
            .id("attachment-remove-native")
            .size_full()
            .track_focus(&self.focus)
            .role(Role::Button)
            .aria_label(vega_i18n::t_args(
                "ui.remove_image_id",
                &[("id", &self.snapshot.source.attachment.to_string())],
            ))
            .capture_key_down(cx.listener(|this, event, window, cx| {
                if matches!(
                    crate::nsh::keyboard::Key::from_event(event),
                    crate::nsh::keyboard::Key::Activate
                ) && this.focus.is_focused(window)
                {
                    this.activate(&this.snapshot.clone(), window, cx);
                    cx.stop_propagation();
                }
            }))
            .on_a11y_action(AccessibleAction::Click, move |_, window, cx| {
                let mut observed = observed.clone();
                observed.geometry = ax_lease.upgrade().and_then(|mount| mount.geometry.get());
                let _ = weak.update(cx, |this, cx| this.activate(&observed, window, cx));
            });
        if let Some(surface) = &mut self.surface {
            surface.sync_environment(cx);
            surface.publish(self.snapshot.clone());
            host = host.child(surface.entity());
        }
        Mounted {
            child: host.into_any_element(),
            host: cx.entity(),
            lease,
        }
        .into_any_element()
    }
}

struct Mounted {
    child: AnyElement,
    host: Entity<RemoveHost>,
    lease: Rc<Mount>,
}

impl IntoElement for Mounted {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for Mounted {
    type RequestLayoutState = ();
    type PrepaintState = (Bounds<Pixels>, Bounds<Pixels>, Size<Pixels>);
    fn id(&self) -> Option<ElementId> {
        Some("attachment-removal-mounted".into())
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
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let mask = window.content_mask().bounds;
        let viewport = window.viewport_size();
        self.lease.geometry.set(None);
        if let Some(id) = id {
            window.with_element_state::<Rc<Mount>, _>(id, |_, _| ((), self.lease.clone()));
        }
        self.child.prepaint(window, cx);
        (bounds, mask, viewport)
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        state: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let (bounds, mask, viewport) = *state;
        let visible = bounds.intersect(&mask);
        let geometry = {
            let host = self.host.read(cx);
            host.owner.upgrade().and_then(|owner| {
                let stream = owner.read(cx);
                host.parent
                    .upgrade()
                    .filter(|mount| mount.active.get())
                    .and_then(|mount| mount.visible.get())
                    .and_then(|parent| {
                        let visible = visible.intersect(&parent);
                        (!visible.is_empty() && host.snapshot.source.current(stream, cx)).then_some(
                            Geometry {
                                bounds,
                                visible,
                                parent,
                                viewport: stream.list.viewport_bounds(),
                                foreground: stream.foreground_height,
                                window: viewport,
                            },
                        )
                    })
            })
        };
        self.lease.geometry.set(geometry);
        self.host.update(cx, |this, _| {
            if this.snapshot.geometry != geometry {
                this.snapshot.geometry = geometry;
                if let Some(surface) = &this.surface {
                    surface.publish(this.snapshot.clone());
                }
            }
        });
        self.child.paint(window, cx);
    }
}

fn render(
    state: nsh_core::prelude::Signal<Snapshot>,
    sender: tokio::sync::mpsc::Sender<Snapshot>,
) -> nsh_core::prelude::AnyElement {
    use ns::{
        Accessible as _, Interactive as _, IntoElement as _, ParentElement as _, Styled as _,
    };
    use nsh_core::prelude as ns;
    let commands = crate::nsh::OrderedCommands::new(sender);
    let owner = nsh_core::element::Owner::current();
    let observed = state.get_untracked();
    let hover = ns::signal(false);
    let pressed = ns::signal(false);
    let activate = commands.clone();
    let label = vega_i18n::t_args(
        "ui.remove_image_id",
        &[("id", &observed.source.attachment.to_string())],
    );
    let icon = crate::icons::nsh_source(crate::icons::Icon::Close).unwrap_or_default();
    ns::view()
        .w_full()
        .h_full()
        .rounded(ns::px(12.))
        .role(nsh_core::AccessibilityRole::Button)
        .label(label)
        .border(ns::px(1.))
        .border_color(move || {
            let snapshot = state.get();
            crate::nsh::color(if snapshot.focused {
                snapshot.colors.accent
            } else {
                snapshot.colors.bg_elevated
            })
        })
        .bg(move || {
            let colors = state.get().colors;
            crate::nsh::color(if pressed.get() {
                colors.bg_active
            } else if hover.get() {
                colors.bg_hover
            } else {
                colors.bg_elevated
            })
        })
        .flex_row()
        .items_center()
        .justify_center()
        .on_mouse_enter(move |_: &nsh_core::MouseEvent| hover.set(true))
        .on_mouse_leave(move |_: &nsh_core::MouseEvent| {
            hover.set(false);
            pressed.set(false);
        })
        .on_press_down(move || pressed.set(true))
        .on_press_up(move || pressed.set(false))
        .on_press(move || {
            owner.run(|| {
                let mut displayed = observed.clone();
                displayed.geometry = state.get_untracked().geometry;
                activate.send(displayed);
            })
        })
        .child(
            ns::svg(nsh_core::element::ImageSrc::svg_markup(&icon))
                .w(ns::px(16.))
                .h(ns::px(16.))
                .text_color(move || crate::nsh::color(state.get().colors.text_secondary)),
        )
        .into_element()
}
