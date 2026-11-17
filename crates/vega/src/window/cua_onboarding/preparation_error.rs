use super::*;
use std::cell::RefCell;
use std::rc::{Rc, Weak};
use vega_ui::nsh::{ProjectionId, Surface};

mod mounted;
mod view;

#[derive(Clone)]
pub(super) struct Failure {
    id: ProjectionId,
    pub(super) origin: Origin,
    error: String,
}

impl Failure {
    pub(super) fn new(origin: Origin, error: String) -> Self {
        Self {
            id: ProjectionId::new(),
            origin,
            error,
        }
    }

    pub(super) fn matches(&self, other: &Self) -> bool {
        self.id.matches(&other.id)
            && self.error == other.error
            && match (&self.origin, &other.origin) {
                (Origin::Settings, Origin::Settings) => true,
                (
                    Origin::Task {
                        stream,
                        thread_id,
                        project,
                        context,
                    },
                    Origin::Task {
                        stream: other_stream,
                        thread_id: other_thread,
                        project: other_project,
                        context: other_context,
                    },
                ) => {
                    stream == other_stream
                        && thread_id == other_thread
                        && project == other_project
                        && context == other_context
                }
                _ => false,
            }
    }
}

impl OnboardingHost {
    pub(super) fn hide_error(&self, _: &mut App) {
        self.error_presentation.borrow_mut().take();
    }
}

pub(super) fn route_allows(owner: &VegaWindow, failure: &Failure, cx: &App) -> bool {
    if !failure.origin.is_current(owner, cx) || owner.palette.view.is_some() {
        return false;
    }
    match &failure.origin {
        Origin::Settings => cx.global::<SettingsOpen>().0,
        Origin::Task {
            stream,
            thread_id,
            project,
            ..
        } => {
            !cx.global::<SettingsOpen>().0
                && owner.sidebar.read(cx).opening_thread(cx).is_none()
                && cx
                    .global::<OpenedThread>()
                    .0
                    .as_ref()
                    .is_some_and(|thread| {
                        thread.id == *thread_id && thread.project_binding() == project.as_deref()
                    })
                && cx.global::<SelectedProject>().0 == *project
                && owner
                    .stream_view
                    .as_ref()
                    .is_some_and(|(id, visible)| id == thread_id && visible == stream)
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Route {
    project: Option<String>,
    thread: Option<String>,
    stream: Option<EntityId>,
    settings: bool,
    opening: Option<(String, std::time::Instant)>,
}

impl Route {
    fn read(owner: &VegaWindow, cx: &App) -> Self {
        Self {
            project: cx.global::<SelectedProject>().0.clone(),
            thread: cx
                .global::<OpenedThread>()
                .0
                .as_ref()
                .map(|thread| thread.id.clone()),
            stream: owner
                .stream_view
                .as_ref()
                .map(|(_, stream)| stream.entity_id()),
            settings: cx.global::<SettingsOpen>().0,
            opening: owner.sidebar.read(cx).opening_thread(cx),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Retry,
    Dismiss,
    Reload,
}

impl Action {
    fn label(self) -> &'static str {
        vega_i18n::t(match self {
            Self::Retry => "app.setup.retry_label",
            Self::Dismiss => "app.setup.dismiss_label",
            Self::Reload => "settings.action.reload",
        })
    }

    fn text(self) -> &'static str {
        vega_i18n::t(match self {
            Self::Retry => "app.setup.retry",
            Self::Dismiss => "app.setup.dismiss",
            Self::Reload => "settings.action.reload",
        })
    }
}

#[derive(Clone)]
struct Snapshot {
    id: ProjectionId,
    epoch: u64,
    error: String,
    fault: Option<String>,
    recovery: bool,
    selected: Action,
    focused: bool,
    colors: ThemeColors,
    appearance: Appearance,
    language: &'static str,
    max_error_height: f32,
}

impl Snapshot {
    fn same_frame(&self, other: &Self) -> bool {
        self.id.matches(&other.id)
            && self.epoch == other.epoch
            && self.error == other.error
            && self.fault == other.fault
            && self.recovery == other.recovery
            && self.selected == other.selected
            && self.focused == other.focused
            && self.appearance == other.appearance
            && self.language == other.language
            && self.max_error_height == other.max_error_height
    }

    fn actions(&self) -> [Action; 2] {
        [
            if self.recovery {
                Action::Reload
            } else {
                Action::Retry
            },
            Action::Dismiss,
        ]
    }
}

enum Request {
    Layout(Snapshot, [f32; 4], [[f32; 4]; 2]),
    Activate(Snapshot, Action),
}

#[derive(Default)]
struct Geometry {
    bounds: Option<Bounds<Pixels>>,
    clip: Option<Bounds<Pixels>>,
    viewport: Option<Size<Pixels>>,
    scale: f32,
}

impl Geometry {
    fn visible(&self, root: [f32; 4], controls: [[f32; 4]; 2], window: &Window) -> bool {
        if !root
            .iter()
            .chain(controls.iter().flatten())
            .all(|value| value.is_finite())
            || root[2] <= 0.
            || root[3] <= 0.
            || self.viewport != Some(window.viewport_size())
            || self.scale != window.scale_factor()
        {
            return false;
        }
        let (Some(bounds), Some(clip)) = (self.bounds, self.clip) else {
            return false;
        };
        let visible = bounds.intersect(&clip);
        (f32::from(bounds.size.width) - root[2]).abs() <= 0.5
            && (f32::from(bounds.size.height) - root[3]).abs() <= 0.5
            && controls.iter().all(|control| {
                let left = bounds.left() + px(control[0] - root[0]);
                let top = bounds.top() + px(control[1] - root[1]);
                control[2] > 0.
                    && control[3] > 0.
                    && left >= visible.left()
                    && top >= visible.top()
                    && left + px(control[2]) <= visible.right()
                    && top + px(control[3]) <= visible.bottom()
            })
    }
}

pub(super) struct NativeHost {
    owner: WeakEntity<VegaWindow>,
    focus_owner_id: EntityId,
    failure: Option<Failure>,
    route: Option<Route>,
    id: ProjectionId,
    epoch: u64,
    surface: Option<Surface<Snapshot, Request>>,
    fault: Option<String>,
    recovery: bool,
    focus: [FocusHandle; 2],
    selected: Action,
    published: Option<Snapshot>,
    displayed: Option<(Snapshot, [f32; 4], [[f32; 4]; 2])>,
    controls_geometry: Option<([f32; 4], [[f32; 4]; 2])>,
    geometry: Rc<RefCell<Geometry>>,
    lease: Weak<()>,
    hidden: bool,
    consumed: bool,
    height: f32,
    observing_window: bool,
}

impl NativeHost {
    pub(super) fn new(owner: WeakEntity<VegaWindow>, cx: &mut Context<Self>) -> Self {
        cx.observe_global::<SettingsOpen>(|host, cx| host.route_changed(cx))
            .detach();
        cx.observe_global::<OpenedThread>(|host, cx| host.route_changed(cx))
            .detach();
        cx.observe_global::<SelectedProject>(|host, cx| host.route_changed(cx))
            .detach();
        Self {
            owner,
            focus_owner_id: cx.entity_id(),
            failure: None,
            route: None,
            id: ProjectionId::new(),
            epoch: 0,
            surface: None,
            fault: None,
            recovery: false,
            focus: [cx.focus_handle(), cx.focus_handle()],
            selected: Action::Retry,
            published: None,
            displayed: None,
            controls_geometry: None,
            geometry: Rc::new(RefCell::new(Geometry::default())),
            lease: Weak::new(),
            hidden: true,
            consumed: false,
            height: Layout::SETTINGS_CONTROL_MIN_HEIGHT + Layout::CONTENT_PADDING * 2.,
            observing_window: false,
        }
    }

    fn invalidate(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.displayed = None;
    }

    fn route_changed(&mut self, cx: &mut Context<Self>) {
        self.hidden = true;
        self.invalidate();
        cx.notify();
    }

    fn live(&self, window: &Window, cx: &App) -> bool {
        !self.hidden
            && self.lease.upgrade().is_some()
            && window.is_window_active()
            && window.root::<VegaWindow>().flatten().is_some_and(|owner| {
                self.owner
                    .upgrade()
                    .is_some_and(|expected| expected == owner)
                    && self.failure.as_ref().is_some_and(|failure| {
                        let owner = owner.read(cx);
                        owner
                            .onboarding
                            .error_presentation
                            .borrow()
                            .as_ref()
                            .is_some_and(|host| host.entity_id() == self.focus_owner_id)
                            && owner
                                .onboarding
                                .failed
                                .as_ref()
                                .is_some_and(|current| current.matches(failure))
                            && route_allows(owner, failure, cx)
                            && self.route.as_ref() == Some(&Route::read(owner, cx))
                    })
            })
    }

    fn snapshot(&self, window: &Window, cx: &App) -> Snapshot {
        let error = self
            .failure
            .as_ref()
            .map(|failure| {
                crate::app_localization::translated(&failure.error)
                    .map(str::to_owned)
                    .unwrap_or_else(|| failure.error.clone())
            })
            .unwrap_or_default();
        Snapshot {
            id: self.id.clone(),
            epoch: self.epoch,
            error,
            fault: self.fault.clone(),
            recovery: self.recovery,
            selected: self.selected,
            focused: self.focus.iter().any(|focus| focus.is_focused(window)),
            colors: theme(cx).colors,
            appearance: theme(cx).appearance,
            language: vega_i18n::language(),
            max_error_height: f32::from(window.viewport_size().height) / 3.,
        }
    }

    fn accepts(&self, observed: &Snapshot, window: &Window, cx: &App) -> bool {
        !self.consumed
            && self.live(window, cx)
            && observed.same_frame(&self.snapshot(window, cx))
            && self
                .surface
                .as_ref()
                .is_some_and(|surface| surface.errors().borrow().is_none())
            && self
                .displayed
                .as_ref()
                .is_some_and(|(shown, root, controls)| {
                    shown.same_frame(observed)
                        && self.geometry.borrow().visible(*root, *controls, window)
                })
    }

    fn dispatch(
        &mut self,
        observed: &Snapshot,
        action: Action,
        keyboard: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !observed.actions().contains(&action)
            || (keyboard
                && (!self.focus.iter().any(|focus| focus.is_focused(window))
                    || action != self.selected))
            || !self.accepts(observed, window, cx)
        {
            return;
        }
        self.consumed = true;
        self.invalidate();
        self.selected = action;
        window.focus(&self.focus[usize::from(action == Action::Dismiss)], cx);
        if action == Action::Reload {
            self.surface = None;
            self.id = ProjectionId::new();
            self.recovery = false;
            self.fault = None;
            self.selected = Action::Retry;
            self.published = None;
            self.consumed = false;
            cx.notify();
            return;
        }
        let Some(failure) = self.failure.clone() else {
            return;
        };
        let _ = self.owner.update(cx, |owner, cx| {
            if owner
                .onboarding
                .failed
                .as_ref()
                .is_some_and(|current| current.matches(&failure))
                && route_allows(owner, &failure, cx)
            {
                match action {
                    Action::Retry => owner.retry_preparation(&failure, cx),
                    Action::Dismiss => {
                        owner.onboarding.failed = None;
                        cx.notify();
                    }
                    Action::Reload => {}
                }
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
        let modifiers = event.keystroke.modifiers;
        if !self.focus.iter().any(|focus| focus.is_focused(window))
            || !self.live(window, cx)
            || modifiers.platform
            || modifiers.control
            || modifiers.alt
            || event.is_held
        {
            return;
        }
        match event.keystroke.key.as_str() {
            "tab" => {
                if modifiers.shift {
                    window.focus_prev(cx);
                } else {
                    window.focus_next(cx);
                }
            }
            "enter" | "space" | " " if !modifiers.shift => {
                self.dispatch(observed, self.selected, true, window, cx);
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    fn mount_surface(&mut self, snapshot: Snapshot, window: &mut Window, cx: &mut Context<Self>) {
        let (surface, mut requests) = Surface::new(snapshot, view::render, window, cx);
        let id = self.id.clone();
        let mut errors = surface.errors();
        cx.spawn(async move |host, cx| {
            while errors.changed().await.is_ok() {
                let error = errors.borrow().clone();
                if host
                    .update(cx, |host, cx| {
                        if host.id.matches(&id)
                            && let Some(error) = error
                        {
                            host.invalidate();
                            if !host.recovery {
                                host.fault = Some(error);
                                host.recovery = true;
                                host.surface = None;
                                host.id = ProjectionId::new();
                                host.selected = Action::Reload;
                                host.published = None;
                            }
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        cx.spawn_in(window, async move |host, cx| {
            while let Some(request) = requests.recv().await {
                if host
                    .update_in(cx, |host, window, cx| match request {
                        Request::Activate(observed, action) => {
                            host.dispatch(&observed, action, false, window, cx)
                        }
                        Request::Layout(observed, root, controls) => {
                            if !host.live(window, cx)
                                || host.consumed
                                || !observed.same_frame(&host.snapshot(window, cx))
                            {
                                return;
                            }
                            if root.iter().all(|value| value.is_finite())
                                && root[3] > 0.
                                && root[3] <= f32::from(window.viewport_size().height)
                                && (host.height - root[3]).abs() > 0.5
                            {
                                host.height = root[3];
                                host.invalidate();
                                cx.notify();
                            } else if host.geometry.borrow().visible(root, controls, window) {
                                host.controls_geometry = Some((root, controls));
                                host.displayed = Some((observed, root, controls));
                                cx.notify();
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
        self.surface = Some(surface);
    }
}

#[derive(gpui_kit::IntoElement)]
pub(super) struct NativeRenderOnce {
    pub(super) host: Entity<NativeHost>,
    pub(super) failure: Failure,
}

impl RenderOnce for NativeRenderOnce {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let lease = Rc::new(());
        self.host.update(cx, |host, cx| {
            let route = host
                .owner
                .upgrade()
                .map(|owner| Route::read(owner.read(cx), cx));
            let replaced = host
                .failure
                .as_ref()
                .is_none_or(|failure| !failure.matches(&self.failure));
            if !host.observing_window {
                host.observing_window = true;
                for (index, focus) in host.focus.clone().into_iter().enumerate() {
                    cx.on_focus(&focus, window, move |host, _, cx| {
                        host.selected = if index == 1 {
                            Action::Dismiss
                        } else if host.recovery {
                            Action::Reload
                        } else {
                            Action::Retry
                        };
                        host.invalidate();
                        cx.notify();
                    })
                    .detach();
                    cx.on_blur(&focus, window, |host, _, cx| {
                        host.invalidate();
                        cx.notify();
                    })
                    .detach();
                }
                cx.observe_window_activation(window, |host, _, cx| {
                    host.invalidate();
                    cx.notify();
                })
                .detach();
                cx.observe_window_bounds(window, |host, _, cx| {
                    host.invalidate();
                    cx.notify();
                })
                .detach();
            }
            if replaced || host.hidden || host.route != route {
                host.invalidate();
                host.selected = if host.recovery {
                    Action::Reload
                } else {
                    Action::Retry
                };
            }
            if replaced {
                host.surface = None;
                host.id = ProjectionId::new();
                host.recovery = false;
                host.fault = None;
                host.selected = Action::Retry;
                host.published = None;
                host.controls_geometry = None;
                host.consumed = false;
            }
            host.failure = Some(self.failure);
            host.route = route;
            host.hidden = false;
            host.lease = Rc::downgrade(&lease);
            cx.notify();
        });
        mounted::Mounted {
            child: self.host.into_any_element(),
            lease,
        }
    }
}

impl Render for NativeHost {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused_action = if self.focus[1].is_focused(window) {
            Some(Action::Dismiss)
        } else if self.focus[0].is_focused(window) {
            Some(if self.recovery {
                Action::Reload
            } else {
                Action::Retry
            })
        } else {
            None
        };
        if let Some(action) = focused_action
            && self.selected != action
        {
            self.selected = action;
            self.invalidate();
        }
        let mut snapshot = self.snapshot(window, cx);
        if self.published.as_ref().is_some_and(|previous| {
            previous.epoch == snapshot.epoch && !previous.same_frame(&snapshot)
        }) {
            self.invalidate();
            snapshot.epoch = self.epoch;
        }
        if self.surface.is_none() {
            self.mount_surface(snapshot.clone(), window, cx);
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
        let mut host = div()
            .id("preparation-error-native-host")
            .relative()
            .w_full()
            .h(px(self.height))
            .role(Role::Group)
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
                            let _ = weak.update(cx, |host, cx| {
                                host.invalidate();
                                cx.notify();
                            });
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            );
        if self.live(window, cx)
            && let Some((root, controls)) = &self.controls_geometry
            && self.geometry.borrow().visible(*root, *controls, window)
        {
            for (index, action) in snapshot.actions().into_iter().enumerate() {
                let control = controls[index];
                let observed = snapshot.clone();
                let pointer = observed.clone();
                let weak = cx.weak_entity();
                let keyboard = snapshot.clone();
                let target = div()
                    .id(("preparation-error-target", index))
                    .size_full()
                    .role(Role::Button)
                    .aria_label(action.label())
                    .aria_active_descendant()
                    .on_a11y_action(AccessibleAction::Click, move |_, window, cx| {
                        let _ = weak.update(cx, |host, cx| {
                            host.dispatch(&observed, action, false, window, cx)
                        });
                    });
                host = host.child(
                    div()
                        .id(("preparation-error-ax", index))
                        .absolute()
                        .track_focus(&self.focus[index])
                        .tab_stop(true)
                        .capture_key_down(cx.listener(move |host, event, window, cx| {
                            host.keyboard(&keyboard, event, window, cx);
                        }))
                        .left(px(control[0] - root[0]))
                        .top(px(control[1] - root[1]))
                        .w(px(control[2]))
                        .h(px(control[3]))
                        .role(Role::Group)
                        .aria_label(action.label())
                        .on_click(cx.listener(move |host, _, window, cx| {
                            host.dispatch(&pointer, action, false, window, cx);
                        }))
                        .child(target),
                );
            }
        }
        host
    }
}
