use super::*;
use std::{
    cell::RefCell,
    collections::HashMap,
    rc::{Rc, Weak},
};
use vega_ui::nsh::{ProjectionId, Surface};

mod mounted;
mod view;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Kind {
    Header,
    Slots,
    Error,
    PermissionReset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Update,
    Environment,
    Terminal,
    Panel,
    PermissionSettings,
}

#[derive(Clone, PartialEq, Eq)]
struct Control {
    action: Action,
    label: String,
    enabled: bool,
    selected: bool,
}

#[derive(Clone, PartialEq, Eq)]
struct Observation {
    kind: Kind,
    thread: Option<String>,
    project: Option<String>,
    phase: UpdatePhase,
    version: Option<String>,
    dismissed: bool,
    settings: bool,
    blocked: bool,
    mutation: vega_ui::navigation::TaskMutationState,
    permission_reset: Option<(
        vega_conversation::types::PermissionResetProjection,
        u64,
        bool,
    )>,
    title: String,
    controls: Vec<Control>,
}

#[derive(Clone)]
struct Snapshot {
    revision: ProjectionId,
    observation: Observation,
    focused: Option<Action>,
    colors: ThemeColors,
    appearance: Appearance,
    language: &'static str,
}

impl Snapshot {
    fn matches(&self, other: &Self) -> bool {
        self.revision.matches(&other.revision)
            && self.observation == other.observation
            && self.focused == other.focused
            && self.appearance == other.appearance
            && self.language == other.language
    }
    fn allows(&self, action: Action) -> bool {
        !self.observation.settings
            && !self.observation.blocked
            && self
                .observation
                .controls
                .iter()
                .any(|control| control.action == action && control.enabled)
    }
}

type ControlRects = Vec<(Action, [f32; 4])>;
type Displayed = (Snapshot, [f32; 4], ControlRects);
type Positions = ([f32; 4], ControlRects);

enum Request {
    Layout(Snapshot, [f32; 4], Vec<(Action, [f32; 4])>),
    Activate(Snapshot, Action),
    Height(Snapshot, f32),
}

#[derive(Default)]
pub(super) struct Pages {
    hosts: HashMap<Kind, Entity<Host>>,
}

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

struct Host {
    owner: WeakEntity<VegaWindow>,
    kind: Kind,
    id: EntityId,
    revision: ProjectionId,
    surface: Option<Surface<Snapshot, Request>>,
    published: Option<Snapshot>,
    displayed: Option<Displayed>,
    positions: Option<Positions>,
    height: f32,
    focuses: Vec<(Action, FocusHandle)>,
    lease: Weak<()>,
    geometry: Rc<RefCell<Geometry>>,
    subscriptions: Vec<Subscription>,
}

impl VegaWindow {
    fn shell_chrome_observation(&self, kind: Kind, window: &Window, cx: &App) -> Observation {
        let mut controls = Vec::new();
        let title = match kind {
            Kind::Header => {
                if !self.update_notice_dismissed
                    && matches!(
                        self.updater.state.phase,
                        UpdatePhase::Ready | UpdatePhase::Available
                    )
                {
                    controls.push(Control {
                        action: Action::Update,
                        label: vega_i18n::t(if self.updater.state.phase == UpdatePhase::Ready {
                            "app.shell.update_ready"
                        } else {
                            "app.shell.update_available"
                        })
                        .into(),
                        enabled: true,
                        selected: false,
                    });
                }
                self.main_header_title(cx)
            }
            Kind::Slots => {
                let right = self.persistent_right_workspace_visible(window, cx);
                let available = self.environment_available(cx);
                let visible = if self.environment_is_wide(window, cx) {
                    !self.environment_collapsed
                } else {
                    self.environment_overlay_open
                };
                controls.push(Control {
                    action: Action::Environment,
                    label: vega_i18n::t("app.shell.toggle_environment").into(),
                    enabled: available && !right,
                    selected: available
                        && !right
                        && self.workspace_fullscreen_index().is_none()
                        && visible,
                });
                controls.push(Control {
                    action: Action::Terminal,
                    label: vega_i18n::t("app.shell.toggle_terminal").into(),
                    enabled: self.shell_project_id(cx).is_some(),
                    selected: self.workspace_recent_terminal_is_rendered(window, cx),
                });
                controls.push(Control {
                    action: Action::Panel,
                    label: vega_i18n::t("app.shell.toggle_panel").into(),
                    enabled: self.right_workspace_slot_available(window, cx),
                    selected: self.right_workspace_rendered_non_terminal(window, cx),
                });
                String::new()
            }
            Kind::Error => cx
                .try_global::<vega_ui::navigation::NavigationState>()
                .and_then(|state| state.error_message())
                .unwrap_or_default()
                .into(),
            Kind::PermissionReset => {
                if self.permission_reset.projection.authority_blocked() {
                    controls.push(Control {
                        action: Action::PermissionSettings,
                        label: vega_i18n::t("ui.permission_reset_open_settings").into(),
                        enabled: true,
                        selected: false,
                    });
                }
                vega_i18n::t(if self.permission_reset.session.is_some() {
                    "ui.permission_reset_paused_notice"
                } else {
                    "ui.permission_reset_failed_notice"
                })
                .into()
            }
        };
        Observation {
            kind,
            thread: cx
                .global::<OpenedThread>()
                .0
                .as_ref()
                .map(|thread| thread.id.clone()),
            project: cx.global::<SelectedProject>().0.clone(),
            phase: self.updater.state.phase.clone(),
            version: self.updater.state.latest_version.clone(),
            dismissed: self.update_notice_dismissed,
            settings: cx.global::<SettingsOpen>().0,
            blocked: self.palette.view.is_some() || cx.global::<PendingDeleteConfirm>().0.is_some(),
            mutation: cx
                .try_global::<vega_ui::navigation::TaskMutationState>()
                .copied()
                .unwrap_or_default(),
            permission_reset: (kind == Kind::PermissionReset).then(|| {
                (
                    self.permission_reset.projection.clone(),
                    self.permission_reset.operation,
                    self.permission_reset.session.is_some(),
                )
            }),
            title,
            controls,
        }
    }

    pub(super) fn hide_shell_chrome(&mut self, cx: &mut Context<Self>) {
        for (kind, host) in &self.shell_chrome.hosts {
            if *kind != Kind::Error {
                host.update(cx, |host, cx| {
                    host.invalidate();
                    host.lease = Weak::new();
                    if let Some(surface) = host.surface.take() {
                        surface.entity().read(cx).stop();
                    }
                });
            }
        }
    }

    pub(super) fn render_shell_chrome(
        &mut self,
        kind: Kind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let observed = self.shell_chrome_observation(kind, window, cx);
        let notice_focus = if kind == Kind::PermissionReset {
            Some(
                self.permission_reset
                    .notice_focus
                    .get_or_insert_with(|| cx.focus_handle().tab_stop(true))
                    .clone(),
            )
        } else {
            None
        };
        let owner = cx.weak_entity();
        let host = self
            .shell_chrome
            .hosts
            .entry(kind)
            .or_insert_with(|| {
                cx.new(|cx| {
                    let mut subscriptions = Vec::new();
                    let focuses = if let Some(focus) = notice_focus {
                        subscriptions.push(cx.on_focus(
                            &focus,
                            window,
                            |host: &mut Host, _, cx| {
                                host.invalidate();
                                cx.notify();
                            },
                        ));
                        subscriptions.push(cx.on_blur(&focus, window, |host: &mut Host, _, cx| {
                            host.invalidate();
                            cx.notify();
                        }));
                        vec![(Action::PermissionSettings, focus)]
                    } else {
                        Vec::new()
                    };
                    Host {
                        owner,
                        kind,
                        id: cx.entity_id(),
                        revision: ProjectionId::new(),
                        surface: None,
                        published: None,
                        displayed: None,
                        positions: None,
                        height: Typography::METADATA * Typography::BODY_LINE_HEIGHT + 8.,
                        focuses,
                        lease: Weak::new(),
                        geometry: Rc::new(RefCell::new(Geometry::default())),
                        subscriptions,
                    }
                })
            })
            .clone();
        Deferred { host, observed }.into_any_element()
    }
}

impl Host {
    fn invalidate(&mut self) {
        self.revision = ProjectionId::new();
        self.displayed = None;
    }
    fn current(&self, window: &Window, cx: &App) -> Option<Snapshot> {
        let owner = self.owner.upgrade()?;
        let observation = owner
            .read(cx)
            .shell_chrome_observation(self.kind, window, cx);
        Some(Snapshot {
            revision: self.revision.clone(),
            observation,
            focused: self
                .focuses
                .iter()
                .find(|(_, focus)| focus.is_focused(window))
                .map(|(action, _)| *action),
            colors: theme(cx).colors,
            appearance: theme(cx).appearance,
            language: vega_i18n::language(),
        })
    }
    fn live(&self, window: &Window, cx: &App) -> bool {
        self.lease.upgrade().is_some()
            && window.root::<VegaWindow>().flatten().is_some_and(|root| {
                self.owner.upgrade().is_some_and(|owner| owner == root)
                    && root
                        .read(cx)
                        .shell_chrome
                        .hosts
                        .get(&self.kind)
                        .is_some_and(|host| host.entity_id() == self.id)
            })
    }
    fn accepts(&self, observed: &Snapshot, action: Action, window: &Window, cx: &App) -> bool {
        self.live(window, cx)
            && window.is_window_active()
            && observed.allows(action)
            && self
                .current(window, cx)
                .is_some_and(|current| current.matches(observed))
            && self
                .surface
                .as_ref()
                .is_some_and(|surface| surface.errors().borrow().is_none())
            && self
                .published
                .as_ref()
                .is_some_and(|snapshot| snapshot.matches(observed))
            && self
                .displayed
                .as_ref()
                .is_some_and(|(snapshot, root, controls)| {
                    snapshot.matches(observed)
                        && controls.iter().any(|(owned, rect)| {
                            *owned == action && self.geometry.borrow().visible(*root, *rect, window)
                        })
                })
    }
    fn dispatch(
        &mut self,
        observed: &Snapshot,
        action: Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.accepts(observed, action, window, cx) {
            return;
        }
        self.invalidate();
        if let Some((_, focus)) = self.focuses.iter().find(|(owned, _)| *owned == action) {
            window.focus(focus, cx);
        }
        let _ = self.owner.update(cx, |owner, cx| {
            if owner.shell_chrome_observation(self.kind, window, cx) != observed.observation {
                return;
            }
            match action {
                Action::Update => {
                    cx.set_global(SettingsOpen(true));
                    cx.set_global(PricingSettingsRequested(false));
                    if let Some(view) = &owner.settings_view {
                        view.update(cx, |view, cx| view.show_general(cx));
                    }
                    cx.notify();
                }
                Action::Environment => owner.toggle_environment(window, cx),
                Action::Terminal => owner.workspace_toggle_bottom(window, cx),
                Action::Panel => owner.workspace_toggle_right(window, cx),
                Action::PermissionSettings => owner.open_permission_reset_settings(cx),
            }
        });
        cx.notify();
    }
    fn request(&mut self, request: Request, window: &mut Window, cx: &mut Context<Self>) {
        match request {
            Request::Layout(snapshot, root, controls) => {
                if self.live(window, cx)
                    && self
                        .current(window, cx)
                        .is_some_and(|current| current.matches(&snapshot))
                    && self
                        .published
                        .as_ref()
                        .is_some_and(|current| current.matches(&snapshot))
                    && self
                        .surface
                        .as_ref()
                        .is_some_and(|surface| surface.errors().borrow().is_none())
                {
                    let changed =
                        self.displayed
                            .as_ref()
                            .is_none_or(|(old, old_root, old_controls)| {
                                !old.matches(&snapshot)
                                    || *old_root != root
                                    || *old_controls != controls
                            });
                    self.positions = Some((root, controls.clone()));
                    self.displayed = Some((snapshot, root, controls));
                    if changed {
                        cx.notify();
                    }
                }
            }
            Request::Activate(snapshot, action) => self.dispatch(&snapshot, action, window, cx),
            Request::Height(snapshot, height) => {
                if matches!(self.kind, Kind::Error | Kind::PermissionReset)
                    && height.is_finite()
                    && height > 0.
                    && self.live(window, cx)
                    && self
                        .current(window, cx)
                        .is_some_and(|current| current.matches(&snapshot))
                    && (self.height - height).abs() > 0.5
                {
                    self.height = height;
                    cx.notify();
                    window.refresh();
                }
            }
        }
    }
    fn mount(&mut self, snapshot: Snapshot, window: &mut Window, cx: &mut Context<Self>) {
        if self.surface.is_some() {
            return;
        }
        let (surface, mut requests) = Surface::new(snapshot, view::render, window, cx);
        let mut errors = surface.errors();
        cx.spawn(async move |this, cx| {
            while let Some(request) = requests.recv().await {
                if this
                    .update_in(cx, |this, window, cx| this.request(request, window, cx))
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
                        this.invalidate();
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
        let Some(mut snapshot) = self.current(window, cx) else {
            return div().into_any_element();
        };
        for control in &snapshot.observation.controls {
            if !self
                .focuses
                .iter()
                .any(|(action, _)| *action == control.action)
            {
                let focus = cx.focus_handle();
                self.subscriptions
                    .push(cx.on_focus(&focus, window, |host, _, cx| {
                        host.invalidate();
                        cx.notify();
                    }));
                self.subscriptions
                    .push(cx.on_blur(&focus, window, |host, _, cx| {
                        host.invalidate();
                        cx.notify();
                    }));
                self.focuses.push((control.action, focus));
            }
        }
        if self
            .published
            .as_ref()
            .is_some_and(|old| !old.matches(&snapshot))
        {
            self.invalidate();
            snapshot.revision = self.revision.clone();
        }
        self.mount(snapshot.clone(), window, cx);
        let publish = self
            .published
            .as_ref()
            .is_none_or(|previous| !previous.matches(&snapshot));
        self.published = Some(snapshot.clone());
        let child = self.surface.as_mut().map(|surface| {
            surface.sync_environment(cx);
            if publish {
                surface.publish(snapshot.clone());
            }
            let _ = surface.focus_handle(cx).tab_stop(false);
            surface.entity()
        });
        let geometry = self.geometry.clone();
        let weak = cx.weak_entity();
        let mut host = div()
            .id("shell-chrome-native-host")
            .relative()
            .size_full()
            .when(
                matches!(self.kind, Kind::Error | Kind::PermissionReset),
                |host| host.h(px(self.height)).flex_shrink_0(),
            )
            .children(child)
            .child(
                canvas(
                    move |bounds, window, cx| {
                        let mut current = geometry.borrow_mut();
                        let clip = window.content_mask().bounds;
                        let viewport = window.viewport_size();
                        let scale = window.scale_factor();
                        let changed = current.bounds != Some(bounds)
                            || current.clip != Some(clip)
                            || current.viewport != Some(viewport)
                            || current.scale != scale;
                        current.bounds = Some(bounds);
                        current.clip = Some(clip);
                        current.viewport = Some(viewport);
                        current.scale = scale;
                        drop(current);
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
        if let Some((root, controls)) = self.positions.clone() {
            let shown = snapshot.clone();
            for (action, rect) in controls {
                if !shown.allows(action) || !self.geometry.borrow().visible(root, rect, window) {
                    continue;
                }
                let Some((_, focus)) = self.focuses.iter().find(|(owned, _)| *owned == action)
                else {
                    continue;
                };
                let Some(control) = shown
                    .observation
                    .controls
                    .iter()
                    .find(|control| control.action == action)
                else {
                    continue;
                };
                let pointer = shown.clone();
                let keyboard = shown.clone();
                let observed = shown.clone();
                let weak = cx.weak_entity();
                let focus = focus.clone();
                host = host.child(
                    div()
                        .id(format!("shell-chrome-{action:?}"))
                        .absolute()
                        .left(px(rect[0] - root[0]))
                        .top(px(rect[1] - root[1]))
                        .w(px(rect[2]))
                        .h(px(rect[3]))
                        .track_focus(&focus)
                        .tab_stop(true)
                        .role(Role::Button)
                        .aria_label(control.label.clone())
                        .capture_any_mouse_down(|_, window, _| window.prevent_default())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.dispatch(&pointer, action, window, cx)
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
                                this.dispatch(&keyboard, action, window, cx);
                                cx.stop_propagation();
                            }
                        }))
                        .on_a11y_action(AccessibleAction::Click, move |_, window, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.dispatch(&observed, action, window, cx)
                            });
                        }),
                );
            }
        }
        host.into_any_element()
    }
}

#[derive(gpui_kit::IntoElement)]
struct Deferred {
    host: Entity<Host>,
    observed: Observation,
}
impl RenderOnce for Deferred {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let lease = self
            .host
            .read(cx)
            .lease
            .upgrade()
            .unwrap_or_else(|| Rc::new(()));
        self.host.update(cx, |host, cx| {
            if host.lease.upgrade().is_none()
                || host
                    .published
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.observation != self.observed)
            {
                host.invalidate();
            }
            host.lease = Rc::downgrade(&lease);
            cx.notify();
        });
        mounted::Mounted {
            child: self.host.into_any_element(),
            lease,
        }
    }
}
