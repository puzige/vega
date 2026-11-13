use super::*;

const SHADOW_OUTSET: f32 = 8.;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Popup,
    Dimmer,
    Sidebar,
    Fade(bool),
    Insertion,
}

#[derive(Clone)]
enum Geometry {
    None,
    Scroll(gpui_kit::ScrollHandle),
    Insertion(bool),
}

#[derive(Clone, PartialEq)]
struct ChromeSnapshot {
    kind: Kind,
    colors: ThemeColors,
    geometry: [f32; 4],
}

struct Chrome {
    snapshot: ChromeSnapshot,
    surface: Option<Surface<ChromeSnapshot, ()>>,
    mount: ProjectionId,
    lease: Weak<ChromeLease>,
    failed: bool,
}

type SidebarView = gpui_kit::Entity<nsh_platform_gpui::AppView>;

#[derive(Clone, Default)]
struct SidebarRetirement(Rc<RetiredViews>);
impl gpui_kit::Global for SidebarRetirement {}

#[derive(Default)]
struct RetiredViews {
    queue: std::cell::RefCell<std::collections::VecDeque<SidebarView>>,
    draining: std::cell::Cell<bool>,
    mounted: std::cell::Cell<usize>,
    peak_mounted: std::cell::Cell<usize>,
}

impl RetiredViews {
    fn on_mount(&self, cx: &App) {
        let mounted = self.mounted.get() + 1;
        self.mounted.set(mounted);
        let peak = self.peak_mounted.get().max(mounted);
        self.peak_mounted.set(peak);
        // Leave room for a full collapse and immediate reopen, even for a large
        // list. Repeated mounts beyond that capacity pay back one retirement each.
        let old = if self.queue.borrow().len() > peak.max(64) {
            self.queue.borrow_mut().pop_front()
        } else {
            None
        };
        if let Some(old) = old {
            old.read(cx).stop();
        }
    }

    fn retire(self: &Rc<Self>, view: SidebarView, app: &gpui_kit::AsyncApp) {
        self.mounted.set(self.mounted.get() - 1);
        self.queue.borrow_mut().push_back(view);
        if self.draining.replace(true) {
            return;
        }
        let owner = Rc::downgrade(self);
        app.spawn(async move |cx| {
            loop {
                let Some(retired) = owner.upgrade() else {
                    return;
                };
                let next = retired.queue.borrow_mut().pop_front();
                let Some(view) = next else {
                    retired.draining.set(false);
                    return;
                };
                drop(retired);
                view.update(cx, |view, _| view.stop());
                drop(view);
                // Give the main event loop time for input between blocking engine joins.
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(4))
                    .await;
            }
        })
        .detach();
    }
}

pub(super) struct ChromeLease {
    view: SidebarView,
    skip_stop: std::cell::Cell<bool>,
    retirement: Weak<RetiredViews>,
    app: gpui_kit::AsyncApp,
}

impl ChromeLease {
    pub(super) fn transferred(&self) {
        self.skip_stop.set(true);
    }
    pub(super) fn new<S, C>(surface: &Surface<S, C>, cx: &mut App) -> Self
    where
        S: Clone + Send + Sync + 'static,
        C: Send + 'static,
    {
        let retirement = cx.default_global::<SidebarRetirement>().0.clone();
        retirement.on_mount(cx);
        Self {
            view: surface.entity(),
            skip_stop: Default::default(),
            retirement: Rc::downgrade(&retirement),
            app: cx.to_async(),
        }
    }
}

impl Drop for ChromeLease {
    fn drop(&mut self) {
        if let Some(retirement) = self.retirement.upgrade() {
            if self.skip_stop.get() {
                retirement.mounted.set(retirement.mounted.get() - 1);
            } else {
                retirement.retire(self.view.clone(), &self.app);
            }
        }
    }
}

#[derive(gpui_kit::IntoElement)]
struct Decoration {
    key: gpui_kit::SharedString,
    kind: Kind,
    geometry: Geometry,
}

impl gpui_kit::RenderOnce for Decoration {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window.use_keyed_state(self.key.clone(), cx, |_, cx| Chrome {
            snapshot: ChromeSnapshot {
                kind: self.kind,
                colors: theme(cx).colors,
                geometry: [0.; 4],
            },
            surface: None,
            mount: ProjectionId::new(),
            lease: Weak::new(),
            failed: false,
        });
        state.update(cx, |this, cx| {
            if this.lease.upgrade().is_none() {
                if let Some(surface) = this.surface.take() {
                    surface.entity().read(cx).stop();
                }
                this.mount = ProjectionId::new();
                this.snapshot.geometry = [0.; 4];
                this.failed = false;
            }
            this.snapshot.kind = self.kind;
            this.snapshot.colors = theme(cx).colors;
            if this.failed {
                let weak = cx.weak_entity();
                let mount = this.mount.clone();
                let recovery = host(
                    format!("{}-reload", self.key),
                    None,
                    vec![Item::action(
                        vega_i18n::t("ui.reload_view"),
                        Some(Icon::Refresh),
                    )],
                    false,
                    Typography::SIDEBAR_LINE_HEIGHT,
                    {
                        let weak = weak.clone();
                        let mount = mount.clone();
                        move |cx| {
                            weak.upgrade().is_some_and(|state| {
                                let this = state.read(cx);
                                this.failed && this.mount.matches(&mount)
                            })
                        }
                    },
                    move |_, _, _, cx| {
                        let _ = weak.update(cx, |this, cx| {
                            if this.failed && this.mount.matches(&mount) {
                                if let Some(surface) = this.surface.take() {
                                    surface.entity().read(cx).stop();
                                }
                                this.mount = ProjectionId::new();
                                this.lease = Weak::new();
                                this.failed = false;
                                cx.notify();
                            }
                        });
                    },
                );
                let Some(lease) = this.lease.upgrade() else {
                    return div().into_any_element();
                };
                return Mounted {
                    id: self.key,
                    child: div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .w_full()
                        .child(deferred(recovery).with_priority(4))
                        .into_any_element(),
                    lease: Rc::new(lease),
                }
                .into_any_element();
            }
            if this.surface.is_none() {
                let (surface, _requests) =
                    Surface::new_transparent(this.snapshot.clone(), render, window, cx);
                let _ = surface.focus_handle(cx).tab_stop(false);
                let mut errors = surface.errors();
                let mount = this.mount.clone();
                cx.spawn(async move |this, cx| {
                    while errors.changed().await.is_ok() {
                        if errors.borrow_and_update().is_some() {
                            let _ = this.update(cx, |this, cx| {
                                if this.mount.matches(&mount) && this.lease.upgrade().is_some() {
                                    if let Some(surface) = &this.surface {
                                        surface.entity().read(cx).stop();
                                    }
                                    this.failed = true;
                                    cx.notify();
                                }
                            });
                            break;
                        }
                    }
                })
                .detach();
                this.surface = Some(surface);
            }
            let Some(surface) = this.surface.as_mut() else {
                return div().into_any_element();
            };
            surface.sync_environment(cx);
            surface.publish_if_changed(this.snapshot.clone());
            let lease = this
                .lease
                .upgrade()
                .unwrap_or_else(|| Rc::new(ChromeLease::new(surface, cx)));
            this.lease = Rc::downgrade(&lease);
            let outset = if self.kind == Kind::Popup {
                SHADOW_OUTSET
            } else {
                0.
            };
            let child = div()
                .absolute()
                .top(px(-outset))
                .bottom(px(-outset))
                .left(px(-outset))
                .right(px(-outset))
                .child(surface.entity())
                .into_any_element();
            let weak = cx.weak_entity();
            let mount = this.mount.clone();
            Mounted {
                id: self.key,
                child: super::geometry::sample_decoration(child, move |bounds, window, cx| {
                    let geometry = match &self.geometry {
                        Geometry::None => [0.; 4],
                        Geometry::Scroll(scroll) => {
                            let hidden = f32::from(-scroll.offset().y).max(0.);
                            let remaining =
                                f32::from(scroll.max_offset().y + scroll.offset().y).max(0.);
                            [
                                f32::from(bounds.size.width),
                                f32::from(bounds.size.height),
                                hidden,
                                remaining,
                            ]
                        }
                        Geometry::Insertion(valid) => {
                            let height = px(Layout::SIDEBAR_REORDER_LINE_HEIGHT);
                            let top = if window.mouse_position().y < bounds.center().y {
                                0.
                            } else {
                                f32::from(bounds.size.height - height).max(0.)
                            };
                            [
                                f32::from(bounds.size.width),
                                f32::from(bounds.size.height),
                                top,
                                if *valid
                                    && cx.has_active_drag()
                                    && bounds.contains(&window.mouse_position())
                                {
                                    1.
                                } else {
                                    0.
                                },
                            ]
                        }
                    };
                    let _ = weak.update(cx, |this, _cx| {
                        if this.mount.matches(&mount)
                            && this.lease.upgrade().is_some()
                            && !this.failed
                            && geometry
                                .iter()
                                .all(|value| value.is_finite() && *value >= 0.)
                            && this.snapshot.geometry != geometry
                        {
                            this.snapshot.geometry = geometry;
                            if let Some(surface) = &this.surface
                                && surface.errors().borrow().is_none()
                            {
                                surface.publish_if_changed(this.snapshot.clone());
                            }
                        }
                    });
                }),
                lease: Rc::new(lease),
            }
            .into_any_element()
        })
    }
}

pub(in crate::sidebar) fn popup(
    key: impl Into<gpui_kit::SharedString>,
    content: AnyElement,
) -> AnyElement {
    div()
        .relative()
        .w(px(Layout::TASK_MENU_WIDTH))
        .child(Decoration {
            key: key.into(),
            kind: Kind::Popup,
            geometry: Geometry::None,
        })
        .child(content)
        .into_any_element()
}

pub(in crate::sidebar) fn dimmer() -> AnyElement {
    Decoration {
        key: "sidebar-delete-dimmer".into(),
        kind: Kind::Dimmer,
        geometry: Geometry::None,
    }
    .into_any_element()
}

pub(in crate::sidebar) fn background() -> AnyElement {
    Decoration {
        key: "sidebar-background-nsh".into(),
        kind: Kind::Sidebar,
        geometry: Geometry::None,
    }
    .into_any_element()
}

pub(in crate::sidebar) fn insertion_overlay<E: Element>(
    base: E,
    key: impl Into<gpui_kit::SharedString>,
    valid: bool,
) -> AnyElement {
    if !valid {
        return base.into_any_element();
    }
    super::geometry::overlay(
        base,
        Decoration {
            key: key.into(),
            kind: Kind::Insertion,
            geometry: Geometry::Insertion(valid),
        }
        .into_any_element(),
    )
    .into_any_element()
}

pub(in crate::sidebar) trait ScrollFadeExt: IntoElement + Sized
where
    Self::Element: gpui_kit::Styled,
{
    fn sidebar_scroll_fade(
        self,
        key: impl Into<gpui_kit::SharedString>,
        scroll: gpui_kit::ScrollHandle,
        elevated: bool,
    ) -> super::geometry::Overlay<Self::Element> {
        super::geometry::overlay(
            self.into_element(),
            Decoration {
                key: key.into(),
                kind: Kind::Fade(elevated),
                geometry: Geometry::Scroll(scroll),
            }
            .into_any_element(),
        )
    }
}

impl<T: IntoElement> ScrollFadeExt for T where T::Element: gpui_kit::Styled {}

fn render(state: ns::Signal<ChromeSnapshot>, _: mpsc::Sender<()>) -> ns::AnyElement {
    ns::view()
        .size_full()
        .accessible(false)
        .accessibility_elements_hidden(true)
        .pointer_events(nsh_core::PointerEvents::None)
        .child(ns::dynamic(move || {
            let snapshot = state.get();
            match snapshot.kind {
                Kind::Popup => ns::view()
                    .size_full()
                    .p(ns::px(SHADOW_OUTSET))
                    .child(
                        ns::view()
                            .size_full()
                            .rounded(ns::px(Layout::MENU_RADIUS))
                            .border_1()
                            .border_color(crate::nsh::color(snapshot.colors.border_subtle))
                            .bg(crate::nsh::color(snapshot.colors.bg_elevated))
                            .shadow_sm(),
                    )
                    .into_element(),
                Kind::Sidebar => ns::view()
                    .size_full()
                    .bg(crate::nsh::color(snapshot.colors.bg_sidebar))
                    .into_element(),
                Kind::Insertion => ns::view()
                    .size_full()
                    .child(
                        ns::view()
                            .absolute()
                            .left(ns::px(0.))
                            .top(ns::px(snapshot.geometry[2]))
                            .w(ns::px(snapshot.geometry[0]))
                            .h(ns::px(if snapshot.geometry[3] > 0. {
                                Layout::SIDEBAR_REORDER_LINE_HEIGHT
                            } else {
                                0.
                            }))
                            .bg(crate::nsh::color(snapshot.colors.accent)),
                    )
                    .into_element(),
                Kind::Fade(elevated) => {
                    let [width, height, hidden, remaining] = snapshot.geometry;
                    let length = Layout::SCROLL_EDGE_FADE_COMPACT.min(height / 4.).max(0.);
                    let color = if elevated {
                        snapshot.colors.bg_elevated
                    } else {
                        snapshot.colors.bg_sidebar
                    };
                    let mut fades = ns::view().size_full().overflow_hidden();
                    if elevated {
                        fades = fades.rounded(ns::px(Layout::MENU_RADIUS));
                    }
                    if length > 0. {
                        for (top, hidden, angle) in [
                            (0., hidden, 180.),
                            ((height - length).max(0.), remaining, 0.),
                        ] {
                            let strength = ((hidden - Layout::SCROLL_EDGE_FADE_EPSILON) / length)
                                .clamp(0., 1.);
                            fades = fades.child(
                                ns::view()
                                    .absolute()
                                    .left(ns::px(0.))
                                    .top(ns::px(top))
                                    .w(ns::px(width))
                                    .h(ns::px(length))
                                    .bg_linear(ns::deg(angle))
                                    .gradient_from(crate::nsh::color(color.opacity(strength)))
                                    .gradient_to(crate::nsh::color(color.opacity(0.))),
                            );
                        }
                    }
                    fades.into_element()
                }
                Kind::Dimmer => ns::view()
                    .size_full()
                    .bg(crate::nsh::color(snapshot.colors.text_primary.opacity(0.4)))
                    .into_element(),
            }
        }))
        .into_element()
}
