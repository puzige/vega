use std::{
    cell::RefCell,
    rc::{Rc, Weak},
    time::{Duration, Instant},
};

use gpui_kit::{prelude::*, *};
use vega_theme::{Motion, theme};
use vega_ui::nsh::{ProjectionId, Surface};

use super::{
    VegaWindow,
    model::{Observation, Request, Snapshot},
    view,
};

#[derive(Default)]
struct Geometry {
    bounds: Option<Bounds<Pixels>>,
    clip: Option<Bounds<Pixels>>,
    viewport: Option<Size<Pixels>>,
    scale: f32,
}

impl Geometry {
    fn visible(&self, root: [f32; 4], control: [f32; 4], window: &Window) -> bool {
        if !root
            .iter()
            .chain(control.iter())
            .all(|value| value.is_finite())
            || root[2] <= 0.
            || root[3] <= 0.
            || control[2] <= 0.
            || control[3] <= 0.
            || self.viewport != Some(window.viewport_size())
            || self.scale != window.scale_factor()
        {
            return false;
        }
        let (Some(bounds), Some(clip)) = (self.bounds, self.clip) else {
            return false;
        };
        let visible = bounds.intersect(&clip);
        let left = bounds.left() + px(control[0] - root[0]);
        let top = bounds.top() + px(control[1] - root[1]);
        (f32::from(bounds.size.width) - root[2]).abs() <= 0.5
            && (f32::from(bounds.size.height) - root[3]).abs() <= 0.5
            && left >= visible.left()
            && top >= visible.top()
            && left + px(control[2]) <= visible.right()
            && top + px(control[3]) <= visible.bottom()
    }
}

pub(super) struct NativeHost {
    owner: WeakEntity<VegaWindow>,
    id: EntityId,
    observed: Observation,
    surface: Option<Surface<Snapshot, Request>>,
    revision: ProjectionId,
    published: Option<Snapshot>,
    displayed: Option<(Snapshot, [f32; 4], [f32; 4])>,
    focus: FocusHandle,
    lease: Weak<()>,
    geometry: Rc<RefCell<Geometry>>,
    animation_pending: bool,
    consumed: bool,
}

impl NativeHost {
    pub(super) fn new(
        owner: WeakEntity<VegaWindow>,
        observed: Observation,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            owner,
            id: cx.entity_id(),
            observed,
            surface: None,
            revision: ProjectionId::new(),
            published: None,
            displayed: None,
            focus: cx.focus_handle(),
            lease: Weak::new(),
            geometry: Rc::new(RefCell::new(Geometry::default())),
            animation_pending: false,
            consumed: false,
        }
    }

    fn invalidate(&mut self) {
        self.revision = ProjectionId::new();
        self.displayed = None;
        self.consumed = false;
    }

    fn live(&self, window: &Window, cx: &App) -> bool {
        self.lease.upgrade().is_some()
            && window.is_window_active()
            && window.root::<VegaWindow>().flatten().is_some_and(|root| {
                self.owner
                    .upgrade()
                    .is_some_and(|owner| root.entity_id() == owner.entity_id())
                    && root.read(cx).loading_view_matches(&self.observed, cx)
                    && root
                        .read(cx)
                        .conversation_loading
                        .presentation
                        .as_ref()
                        .is_some_and(|host| host.entity_id() == self.id)
            })
    }

    fn snapshot(&self, window: &Window, cx: &App) -> Snapshot {
        let epoch = self.observed.visit_started.or(self.observed.started);
        let show_logo = !self.observed.failed()
            && epoch.is_some_and(|epoch| {
                epoch.elapsed() >= Duration::from_millis(Motion::CONVERSATION_LOADING_DELAY_MS)
            });
        let moving = show_logo
            && self.live(window, cx)
            && !cx.reduce_motion()
            && self
                .geometry
                .borrow()
                .bounds
                .zip(self.geometry.borrow().clip)
                .is_some_and(|(bounds, clip)| !bounds.intersect(&clip).is_empty());
        Snapshot {
            revision: self.revision.clone(),
            observed: self.observed.clone(),
            colors: theme(cx).colors,
            appearance: theme(cx).appearance,
            language: vega_i18n::language(),
            focused: self.focus.is_focused(window),
            show_logo,
            phase: moving.then(|| {
                (epoch.unwrap_or_else(Instant::now).elapsed().as_secs_f32() * 1000.
                    / Motion::THINKING_SHIMMER_MS as f32)
                    .fract()
            }),
        }
    }

    fn accepts(&self, observed: &Snapshot, window: &Window, cx: &App) -> bool {
        !self.consumed
            && observed.observed.failed()
            && self.live(window, cx)
            && observed.same_frame(&self.snapshot(window, cx))
            && self
                .surface
                .as_ref()
                .is_some_and(|surface| surface.errors().borrow().is_none())
            && self
                .displayed
                .as_ref()
                .is_some_and(|(shown, root, control)| {
                    shown.same_frame(observed)
                        && self.geometry.borrow().visible(*root, *control, window)
                })
    }

    fn dispatch(
        &mut self,
        observed: &Snapshot,
        focused: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if (focused && !self.focus.is_focused(window)) || !self.accepts(observed, window, cx) {
            return;
        }
        self.consumed = true;
        self.displayed = None;
        window.focus(&self.focus, cx);
        let owner = self.owner.clone();
        let observation = observed.observed.clone();
        let host_id = cx.entity_id();
        let _ = owner.update(cx, |owner, cx| {
            if owner
                .conversation_loading
                .presentation
                .as_ref()
                .is_some_and(|host| host.entity_id() == host_id)
                && owner.loading_view_matches(&observation, cx)
                && let Some(thread_id) = observation.thread_id
            {
                owner.start_conversation_load(thread_id, None, cx);
            }
        });
        cx.notify();
    }

    fn keyboard(
        &mut self,
        observed: &Snapshot,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.focus.is_focused(window)
            || !self.live(window, cx)
            || !self.observed.failed()
            || event.keystroke.modifiers.platform
            || event.keystroke.modifiers.control
            || event.keystroke.modifiers.alt
            || event.is_held
        {
            return;
        }
        match event.keystroke.key.as_str() {
            "tab" => {
                if event.keystroke.modifiers.shift {
                    window.focus_prev(cx);
                } else {
                    window.focus_next(cx);
                }
            }
            "enter" | "space" | " " if !event.keystroke.modifiers.shift => {
                self.dispatch(observed, true, window, cx);
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    fn mount(&mut self, snapshot: Snapshot, window: &mut Window, cx: &mut Context<Self>) {
        let focus = self.focus.clone();
        cx.on_focus(&focus, window, |this, _, cx| {
            this.invalidate();
            cx.notify();
        })
        .detach();
        cx.on_blur(&focus, window, |this, _, cx| {
            this.invalidate();
            cx.notify();
        })
        .detach();
        cx.observe_window_activation(window, |this, _, cx| {
            this.invalidate();
            cx.notify();
        })
        .detach();
        cx.observe_window_bounds(window, |this, _, cx| {
            this.invalidate();
            cx.notify();
        })
        .detach();
        let (surface, mut requests) = Surface::new(snapshot, view::render, window, cx);
        let mut errors = surface.errors();
        cx.spawn(async move |this, cx| {
            while errors.changed().await.is_ok() {
                if this
                    .update(cx, |this, cx| {
                        this.invalidate();
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        cx.spawn_in(window, async move |this, cx| {
            while let Some(request) = requests.recv().await {
                if this
                    .update_in(cx, |this, window, cx| match request {
                        Request::LaidOut(observed, root, control) => {
                            if this.live(window, cx)
                                && !this.consumed
                                && observed.same_frame(&this.snapshot(window, cx))
                                && this.geometry.borrow().visible(root, control, window)
                            {
                                if this.displayed.as_ref().is_some_and(
                                    |(_, previous_root, previous_control)| {
                                        *previous_root != root || *previous_control != control
                                    },
                                ) {
                                    this.invalidate();
                                } else {
                                    this.displayed = Some((observed, root, control));
                                }
                                cx.notify();
                            }
                        }
                        Request::Retry(observed) => this.dispatch(&observed, false, window, cx),
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        self.surface = Some(surface);
    }
}

impl Render for NativeHost {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut snapshot = self.snapshot(window, cx);
        if self.published.as_ref().is_some_and(|old| {
            old.observed != snapshot.observed
                || old.focused != snapshot.focused
                || old.show_logo != snapshot.show_logo
                || old.language != snapshot.language
                || old.appearance != snapshot.appearance
        }) {
            self.invalidate();
            snapshot.revision = self.revision.clone();
        }
        if self.surface.is_none() {
            self.mount(snapshot.clone(), window, cx);
        }
        let waiting = !snapshot.observed.failed() && !snapshot.show_logo;
        if self.live(window, cx) && (snapshot.phase.is_some() || waiting) && !self.animation_pending
        {
            self.animation_pending = true;
            cx.on_next_frame(window, |this, _, cx| {
                this.animation_pending = false;
                cx.notify();
            });
        }
        self.published = Some(snapshot.clone());
        let surface = self.surface.as_mut().map(|surface| {
            surface.sync_environment(cx);
            surface.publish(snapshot.clone());
            let _ = surface.focus_handle(cx).tab_stop(false);
            surface.entity()
        });
        let geometry = self.geometry.clone();
        let weak = cx.weak_entity();
        let keyboard = snapshot.clone();
        let mut host = div()
            .id("conversation-loading-native-host")
            .relative()
            .size_full()
            .track_focus(&self.focus)
            .tab_stop(snapshot.observed.failed())
            .capture_key_down(cx.listener(move |this, event, window, cx| {
                this.keyboard(&keyboard, event, window, cx)
            }))
            .children(surface)
            .child(
                canvas(
                    move |bounds, window, cx| {
                        let clip = window.content_mask().bounds;
                        let viewport = window.viewport_size();
                        let scale = window.scale_factor();
                        let mut geometry = geometry.borrow_mut();
                        let changed = geometry.bounds != Some(bounds)
                            || geometry.clip != Some(clip)
                            || geometry.viewport != Some(viewport)
                            || geometry.scale != scale;
                        geometry.bounds = Some(bounds);
                        geometry.clip = Some(clip);
                        geometry.viewport = Some(viewport);
                        geometry.scale = scale;
                        drop(geometry);
                        if changed {
                            let _ = weak.update(cx, |this, cx| {
                                this.invalidate();
                                cx.notify();
                            });
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            );
        if self.accepts(&snapshot, window, cx)
            && let Some((_, root, control)) = &self.displayed
        {
            let observed = snapshot;
            let pointer = observed.clone();
            let weak = cx.weak_entity();
            host = host.child(
                div()
                    .id("conversation-loading-retry-ax")
                    .absolute()
                    .left(px(control[0] - root[0]))
                    .top(px(control[1] - root[1]))
                    .w(px(control[2]))
                    .h(px(control[3]))
                    .role(Role::Button)
                    .aria_label(vega_i18n::t("ui.retry"))
                    .aria_active_descendant()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.dispatch(&pointer, false, window, cx)
                    }))
                    .on_a11y_action(AccessibleAction::Click, move |_, window, cx| {
                        let _ =
                            weak.update(cx, |this, cx| this.dispatch(&observed, false, window, cx));
                    }),
            );
        }
        host
    }
}

#[derive(gpui_kit::IntoElement)]
pub(super) struct Deferred {
    pub(super) host: Entity<NativeHost>,
    pub(super) observed: Observation,
}

impl RenderOnce for Deferred {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let lease = Rc::new(());
        self.host.update(cx, |host, cx| {
            host.observed = self.observed;
            host.lease = Rc::downgrade(&lease);
            host.invalidate();
            cx.notify();
        });
        Mounted {
            child: self.host.into_any_element(),
            lease,
        }
    }
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
        Some("conversation-loading-mounted".into())
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
