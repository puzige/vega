use super::{ConversationStream, ResumeTail};
use crate::nsh::{ProjectionId, Surface};
use gpui_kit::{prelude::*, *};
use std::cell::Cell;
use std::rc::{Rc, Weak};
use vega_theme::{ThemeColors, theme};

mod mounted;
mod pixels;

pub(super) type Authority = fn(&WeakEntity<ConversationStream>, &Window, &App) -> bool;

#[derive(Clone)]
struct Source {
    epoch: ProjectionId,
    thread: String,
    preview: bool,
    permissions_revoked: bool,
}

impl Source {
    fn current(&self, stream: &ConversationStream, cx: &App) -> bool {
        let placement = stream.turn_placement.borrow();
        !stream.reading_navigation_blocked
            && stream.reading_navigation_epoch.matches(&self.epoch)
            && stream.thread.id == self.thread
            && stream.reading_preview == self.preview
            && stream.permissions_revoked == self.permissions_revoked
            && !stream.following_tail()
            && !placement.automatically_placed()
            && !placement.at_reading_bottom(&stream.list)
            && cx
                .try_global::<crate::settings::SettingsOpen>()
                .is_some_and(|settings| !settings.0)
            && cx
                .try_global::<crate::sidebar::PendingDeleteConfirm>()
                .is_none_or(|pending| pending.0.is_none())
    }
}

#[derive(Clone)]
struct Snapshot {
    id: ProjectionId,
    focused: bool,
    hovered: bool,
    pressed: bool,
    label: String,
    colors: ThemeColors,
    appearance: vega_theme::Appearance,
    language: &'static str,
}

impl Snapshot {
    fn matches(&self, other: &Self) -> bool {
        self.id.matches(&other.id)
            && self.focused == other.focused
            && self.label == other.label
            && self.appearance == other.appearance
            && self.language == other.language
    }
}

enum Request {
    Layout(Snapshot, [f32; 4], [f32; 4]),
    Activate(Snapshot),
}

#[derive(Clone, Copy, PartialEq)]
struct Geometry {
    bounds: Bounds<Pixels>,
    clip: Bounds<Pixels>,
    viewport: Size<Pixels>,
    scale: f32,
    list: Bounds<Pixels>,
    foreground: Pixels,
}

impl Geometry {
    fn visible(&self, root: [f32; 4], control: [f32; 4], window: &Window) -> bool {
        if self.viewport != window.viewport_size()
            || self.scale != window.scale_factor()
            || !root
                .iter()
                .chain(control.iter())
                .all(|value| value.is_finite())
            || root[2] <= 0.
            || root[3] <= 0.
            || control[2] <= 0.
            || control[3] <= 0.
        {
            return false;
        }
        let visible = self.bounds.intersect(&self.clip);
        let left = self.bounds.left() + px(control[0] - root[0]);
        let top = self.bounds.top() + px(control[1] - root[1]);
        !visible.is_empty()
            && (f32::from(self.bounds.size.width) - root[2]).abs() <= 0.5
            && (f32::from(self.bounds.size.height) - root[3]).abs() <= 0.5
            && left >= visible.left()
            && top >= visible.top()
            && left + px(control[2]) <= visible.right()
            && top + px(control[3]) <= visible.bottom()
    }
}

struct Mount {
    live: Cell<bool>,
    painted: Cell<Option<Geometry>>,
}

#[derive(Default)]
pub(super) struct ResumeTailUi {
    host: Option<Entity<Host>>,
}

impl ResumeTailUi {
    pub(super) fn hide(&mut self, cx: &mut App) {
        if let Some(host) = self.host.take() {
            host.update(cx, |host, cx| host.stop(cx));
        }
    }

    pub(super) fn element(
        &mut self,
        stream: WeakEntity<ConversationStream>,
        authority: Authority,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let Some(owner) = stream.upgrade() else {
            self.hide(cx);
            return div().size_full().into_any_element();
        };
        let source = Source {
            epoch: owner.read(cx).reading_navigation_epoch.clone(),
            thread: owner.read(cx).thread.id.clone(),
            preview: owner.read(cx).reading_preview,
            permissions_revoked: owner.read(cx).permissions_revoked,
        };
        if !source.current(owner.read(cx), cx) || !authority(&stream, window, cx) {
            self.hide(cx);
            return div().size_full().into_any_element();
        }
        let reuse = self.host.as_ref().is_some_and(|host| {
            let host = host.read(cx);
            host.owner == stream
                && host.source.epoch.matches(&source.epoch)
                && host.source.thread == source.thread
                && host.source.preview == source.preview
                && host.source.permissions_revoked == source.permissions_revoked
                && host.window == window.window_handle().window_id()
                && host.lease.upgrade().is_some_and(|lease| lease.live.get())
        });
        if !reuse {
            self.hide(cx);
            let focus = owner.read(cx).resume_tail_focus.clone();
            self.host = Some(cx.new(|cx| {
                let subscriptions = vec![
                    cx.on_focus(&focus, window, |host: &mut Host, _, cx| {
                        host.invalidate();
                        cx.notify();
                    }),
                    cx.on_blur(&focus, window, |host: &mut Host, _, cx| {
                        host.invalidate();
                        cx.notify();
                    }),
                    cx.on_release(|host: &mut Host, cx| host.stop(cx)),
                ];
                Host {
                    owner: stream,
                    source,
                    authority,
                    window: window.window_handle().window_id(),
                    revision: ProjectionId::new(),
                    published: None,
                    displayed: None,
                    positions: None,
                    surface: None,
                    hovered: false,
                    pressed: false,
                    focus,
                    lease: Weak::new(),
                    _subscriptions: subscriptions,
                }
            }));
        }
        let Some(host) = &self.host else {
            return div().size_full().into_any_element();
        };
        let lease = host.read(cx).lease.upgrade().unwrap_or_else(|| {
            Rc::new(Mount {
                live: Cell::new(true),
                painted: Cell::new(None),
            })
        });
        host.update(cx, |host, _| {
            host.authority = authority;
            host.lease = Rc::downgrade(&lease);
        });
        mounted::Mounted {
            child: host.clone().into_any_element(),
            lease,
        }
        .into_any_element()
    }
}

struct Host {
    owner: WeakEntity<ConversationStream>,
    source: Source,
    authority: Authority,
    window: WindowId,
    revision: ProjectionId,
    published: Option<Snapshot>,
    displayed: Option<(Snapshot, [f32; 4], [f32; 4])>,
    positions: Option<([f32; 4], [f32; 4])>,
    surface: Option<Surface<Snapshot, Request>>,
    focus: FocusHandle,
    hovered: bool,
    pressed: bool,
    lease: Weak<Mount>,
    _subscriptions: Vec<Subscription>,
}

impl Host {
    fn invalidate(&mut self) {
        self.revision = ProjectionId::new();
        self.published = None;
        self.displayed = None;
    }

    fn stop(&mut self, cx: &mut App) {
        self.invalidate();
        self.positions = None;
        self.hovered = false;
        self.pressed = false;
        if let Some(lease) = self.lease.upgrade() {
            lease.live.set(false);
            lease.painted.set(None);
        }
        self.lease = Weak::new();
        if let Some(surface) = self.surface.take() {
            surface.entity().read(cx).stop();
        }
    }

    fn live(&self, window: &Window, cx: &App) -> bool {
        self.window == window.window_handle().window_id()
            && self.lease.upgrade().is_some_and(|lease| lease.live.get())
            && (self.authority)(&self.owner, window, cx)
            && self
                .owner
                .upgrade()
                .is_some_and(|owner| self.source.current(owner.read(cx), cx))
    }

    fn snapshot(&self, window: &Window, cx: &App) -> Snapshot {
        Snapshot {
            id: self.revision.clone(),
            focused: self.focus.is_focused(window),
            hovered: self.hovered,
            pressed: self.pressed,
            label: vega_i18n::t("ui.back_to_bottom").into(),
            colors: theme(cx).colors,
            appearance: theme(cx).appearance,
            language: vega_i18n::language(),
        }
    }

    fn accepts_measurement(&self, observed: &Snapshot, window: &Window, cx: &App) -> bool {
        self.live(window, cx)
            && self.snapshot(window, cx).matches(observed)
            && self
                .published
                .as_ref()
                .is_some_and(|published| published.matches(observed))
            && self
                .surface
                .as_ref()
                .is_some_and(|surface| surface.errors().borrow().is_none())
    }

    fn dispatch(&mut self, observed: &Snapshot, window: &mut Window, cx: &mut Context<Self>) {
        if !window.is_window_active() || !self.accepts_measurement(observed, window, cx) {
            return;
        }
        let Some(geometry) = self.lease.upgrade().and_then(|lease| lease.painted.get()) else {
            return;
        };
        let Some(owner) = self.owner.upgrade() else {
            return;
        };
        if !self
            .displayed
            .as_ref()
            .is_some_and(|(displayed, root, control)| {
                displayed.matches(observed) && geometry.visible(*root, *control, window)
            })
            || geometry.list != owner.read(cx).list.viewport_bounds()
            || geometry.foreground != owner.read(cx).foreground_height
        {
            return;
        }
        self.invalidate();
        window.focus(&self.focus, cx);
        if self.live(window, cx) {
            owner.update(cx, |stream, cx| {
                if self.source.current(stream, cx) {
                    stream.resume_tail(cx);
                }
            });
        }
        self.stop(cx);
        cx.notify();
    }

    fn mount(&mut self, snapshot: Snapshot, window: &mut Window, cx: &mut Context<Self>) {
        if self.surface.is_some() {
            return;
        }
        let (surface, mut requests) =
            Surface::new_transparent(snapshot, pixels::render, window, cx);
        let mut errors = surface.errors();
        cx.spawn(async move |this, cx| {
            while let Some(request) = requests.recv().await {
                if this
                    .update_in(cx, |this, window, cx| match request {
                        Request::Activate(observed) => this.dispatch(&observed, window, cx),
                        Request::Layout(observed, root, control) => {
                            if this.accepts_measurement(&observed, window, cx)
                                && root
                                    .iter()
                                    .chain(control.iter())
                                    .all(|value| value.is_finite())
                                && root[2] > 0.
                                && root[3] > 0.
                                && control[2] > 0.
                                && control[3] > 0.
                            {
                                let changed = this.displayed.as_ref().is_none_or(|(old, r, c)| {
                                    !old.matches(&observed) || *r != root || *c != control
                                });
                                this.positions = Some((root, control));
                                this.displayed = Some((observed, root, control));
                                if changed {
                                    cx.notify();
                                }
                            }
                        }
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
                    let _ = this.update(cx, |this, cx| {
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

impl Render for Host {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.live(window, cx) {
            self.stop(cx);
            return div().size_full().into_any_element();
        }
        let mut snapshot = self.snapshot(window, cx);
        if self
            .published
            .as_ref()
            .is_some_and(|old| !old.matches(&snapshot))
        {
            self.invalidate();
            snapshot.id = self.revision.clone();
        }
        self.mount(snapshot.clone(), window, cx);
        self.published = Some(snapshot.clone());
        let child = self.surface.as_mut().map(|surface| {
            surface.sync_environment(cx);
            surface.publish(snapshot.clone());
            let _ = surface.focus_handle(cx).tab_stop(false);
            surface.entity()
        });
        let weak = cx.weak_entity();
        let lease = self.lease.clone();
        let mut host = div()
            .id("nsh-resume-tail-host")
            .relative()
            .size_full()
            .children(child)
            .child(
                canvas(
                    move |bounds, window, cx| {
                        let clip = window.content_mask().bounds;
                        let viewport = window.viewport_size();
                        let scale = window.scale_factor();
                        let _ = weak.update(cx, |this, cx| {
                            if !this.live(window, cx) {
                                return;
                            }
                            let Some(owner) = this.owner.upgrade() else {
                                return;
                            };
                            let stream = owner.read(cx);
                            let geometry = Geometry {
                                bounds,
                                clip,
                                viewport,
                                scale,
                                list: stream.list.viewport_bounds(),
                                foreground: stream.foreground_height,
                            };
                            if let Some(lease) = lease.upgrade() {
                                let previous = lease.painted.replace(None);
                                if previous.is_some_and(|old| old != geometry) {
                                    this.invalidate();
                                    cx.notify();
                                }
                            }
                        });
                        (bounds, clip, viewport, scale)
                    },
                    {
                        let weak = cx.weak_entity();
                        move |_, (bounds, clip, viewport, scale), window, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                if !this.live(window, cx) {
                                    return;
                                }
                                if let Some(owner) = this.owner.upgrade()
                                    && let Some(lease) = this.lease.upgrade()
                                {
                                    let stream = owner.read(cx);
                                    lease.painted.set(Some(Geometry {
                                        bounds,
                                        clip,
                                        viewport,
                                        scale,
                                        list: stream.list.viewport_bounds(),
                                        foreground: stream.foreground_height,
                                    }));
                                }
                            });
                        }
                    },
                )
                .absolute()
                .size_full(),
            );
        if let Some((root, rect)) = self.positions
            && self
                .lease
                .upgrade()
                .and_then(|lease| lease.painted.get())
                .is_some_and(|geometry| geometry.visible(root, rect, window))
        {
            let pointer = snapshot.clone();
            let keyboard = snapshot.clone();
            let action = snapshot.clone();
            let accessible = snapshot;
            let focus = self.focus.clone();
            let weak = cx.weak_entity();
            host = host.child(
                div()
                    .id("nsh-resume-tail-proxy")
                    .absolute()
                    .left(px(rect[0] - root[0]))
                    .top(px(rect[1] - root[1]))
                    .w(px(rect[2]))
                    .h(px(rect[3]))
                    .track_focus(&self.focus)
                    .tab_stop(true)
                    .key_context("ResumeTailButton")
                    .role(Role::Button)
                    .aria_label(vega_i18n::t("ui.back_to_bottom"))
                    .tooltip(|_, cx| crate::icons::tooltip(vega_i18n::t("ui.back_to_bottom"), cx))
                    .capture_any_mouse_down(|_, window, _| window.prevent_default())
                    .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                        this.hovered = *hovered;
                        if !hovered {
                            this.pressed = false;
                        }
                        cx.notify();
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.pressed = true;
                            cx.notify();
                        }),
                    )
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.pressed = false;
                            cx.notify();
                        }),
                    )
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.dispatch(&pointer, window, cx)),
                    )
                    .on_action(cx.listener(move |this, _: &ResumeTail, window, cx| {
                        this.dispatch(&action, window, cx);
                        cx.stop_propagation();
                    }))
                    .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                        let modifiers = event.keystroke.modifiers;
                        if focus.is_focused(window)
                            && !event.prefer_character_input
                            && !event.is_held
                            && !modifiers.control
                            && !modifiers.alt
                            && !modifiers.platform
                            && !modifiers.function
                            && !modifiers.shift
                            && matches!(event.keystroke.key.as_str(), "enter" | "space" | " ")
                        {
                            this.dispatch(&keyboard, window, cx);
                            cx.stop_propagation();
                        }
                    }))
                    .on_a11y_action(AccessibleAction::Click, move |_, window, cx| {
                        let _ = weak.update(cx, |this, cx| this.dispatch(&accessible, window, cx));
                    }),
            );
        }
        host.into_any_element()
    }
}
