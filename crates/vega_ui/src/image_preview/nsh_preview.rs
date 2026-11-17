use super::{ImagePreview, ImagePreviewAction};
use crate::nsh::{OrderedCommands, ProjectionId, Surface};
use gpui_kit::{App, Context, Entity, RenderOnce, Subscription, Window};
use nsh_core::element::SharedString;
use nsh_core::prelude::*;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use vega_conversation::types::ImageAttachment;
use vega_theme::{Layout, ThemeColors, Typography};

pub(super) struct Presentation {
    surface: Surface<Snapshot, Request>,
    id: ProjectionId,
    _focus: Subscription,
    recovery: Option<Surface<Snapshot, Request>>,
    focus_revision: u64,
}

#[derive(Clone)]
struct Snapshot {
    id: ProjectionId,
    revision: u64,
    focus_revision: u64,
    source: Option<SharedString>,
    name: String,
    width: f32,
    height: f32,
    thumbnail: bool,
    screenshot: bool,
    busy: bool,
    requested: bool,
    status: Option<&'static str>,
    applications: Arc<[String]>,
    application_id: Option<ProjectionId>,
    menu_revision: u64,
    colors: ThemeColors,
    host_failed: bool,
}

enum Request {
    Action {
        observed: Box<Snapshot>,
        action: ImagePreviewAction,
        reply: oneshot::Sender<bool>,
    },
    Reload(ProjectionId),
}

pub(super) fn encoded_source(image: &ImageAttachment) -> SharedString {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut source = String::with_capacity(image.bytes().len() * 3 + 32);
    source.push_str("data:");
    source.push_str(image.mime_type());
    source.push(',');
    for byte in image.bytes() {
        source.push('%');
        source.push(HEX[usize::from(byte >> 4)] as char);
        source.push(HEX[usize::from(byte & 15)] as char);
    }
    source.into()
}

impl ImagePreview {
    fn presentation(&self, thumbnail: bool) -> Option<&Presentation> {
        if thumbnail {
            self.thumbnail_presentation.as_ref()
        } else {
            self.presentation.as_ref()
        }
    }

    fn snapshot(&self, thumbnail: bool, id: ProjectionId, cx: &App) -> Snapshot {
        Snapshot {
            id,
            revision: self.revision,
            focus_revision: self
                .presentation(thumbnail)
                .map_or(0, |page| page.focus_revision),
            source: self.pixels.get().and_then(|_| self.source.get().cloned()),
            name: self.name.clone(),
            width: self.image.width() as f32,
            height: self.image.height() as f32,
            thumbnail,
            screenshot: thumbnail && self.thumbnail_screenshot,
            busy: self.busy,
            requested: self.applications_requested,
            status: self.status(),
            applications: self.applications.clone().unwrap_or_default().into(),
            application_id: self.application_id.clone(),
            menu_revision: self.menu_revision,
            colors: vega_theme::theme(cx).colors,
            host_failed: self
                .presentation(thumbnail)
                .is_some_and(|page| page.surface.errors().borrow().is_some()),
        }
    }

    fn accepts(&self, observed: &Snapshot, action: ImagePreviewAction) -> bool {
        if self
            .presentation(observed.thumbnail)
            .is_none_or(|page| !page.id.matches(&observed.id))
            || (self.revision != observed.revision
                && !matches!(action, ImagePreviewAction::ChooseApplication(_)))
        {
            return false;
        }
        if observed.thumbnail {
            return matches!(action, ImagePreviewAction::Open);
        }
        match action {
            ImagePreviewAction::ChooseApplication(index) => {
                !self.busy
                    && self.application_id.as_ref().is_some_and(|current| {
                        observed
                            .application_id
                            .as_ref()
                            .is_some_and(|shown| current.matches(shown))
                    })
                    && self.applications.as_ref().is_some_and(|names| {
                        index < names.len() && observed.applications.get(index).is_some()
                    })
            }
            ImagePreviewAction::Applications => {
                !self.busy || self.applications_requested || self.applications.is_some()
            }
            ImagePreviewAction::Save => !self.busy,
            ImagePreviewAction::DismissApplications => {
                self.applications.is_some() || self.applications_requested
            }
            ImagePreviewAction::Close => true,
            ImagePreviewAction::Open => false,
        }
    }

    pub(super) fn render_nsh(
        &mut self,
        thumbnail: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        use gpui_kit::{IntoElement, ParentElement, Styled};
        if self.presentation(thumbnail).is_none() {
            let id = ProjectionId::new();
            let (surface, mut requests) =
                Surface::new(self.snapshot(thumbnail, id.clone(), cx), render, window, cx);
            let legacy_focus = if thumbnail {
                self.thumbnail_focus.clone()
            } else {
                self.focus.clone()
            };
            let focus = cx.on_focus(&legacy_focus, window, move |this, window, cx| {
                let page = if thumbnail {
                    this.thumbnail_presentation.as_mut()
                } else {
                    this.presentation.as_mut()
                };
                if let Some(page) = page {
                    page.focus_revision = page.focus_revision.wrapping_add(1);
                    page.recovery
                        .as_ref()
                        .unwrap_or(&page.surface)
                        .focus(window, cx);
                    let id = page.id.clone();
                    let snapshot = this.snapshot(thumbnail, id, cx);
                    if let Some(page) = this.presentation(thumbnail) {
                        page.recovery
                            .as_ref()
                            .unwrap_or(&page.surface)
                            .publish(snapshot);
                    }
                }
            });
            let mut errors = surface.errors();
            let page = Presentation {
                surface,
                id,
                _focus: focus,
                recovery: None,
                focus_revision: 0,
            };
            if thumbnail {
                self.thumbnail_presentation = Some(page);
            } else {
                self.presentation = Some(page);
            }
            cx.spawn(async move |this, cx| {
                while errors.changed().await.is_ok() {
                    if this.update(cx, |_, cx| cx.notify()).is_err() {
                        break;
                    }
                }
            })
            .detach();
            cx.spawn_in(window, async move |this, cx| {
                while let Some(request) = requests.recv().await {
                    if this
                        .update_in(cx, |this, _, cx| match request {
                            Request::Action {
                                observed,
                                action,
                                reply,
                            } => {
                                let accepted = this.accepts(&observed, action);
                                if accepted {
                                    this.action(action, cx);
                                }
                                if let Some(page) = this.presentation(thumbnail) {
                                    page.surface.publish(this.snapshot(
                                        thumbnail,
                                        page.id.clone(),
                                        cx,
                                    ));
                                }
                                let _ = reply.send(accepted);
                            }
                            Request::Reload(id) => {
                                if this
                                    .presentation(thumbnail)
                                    .is_some_and(|page| page.id.matches(&id))
                                {
                                    if thumbnail {
                                        this.thumbnail_presentation = None;
                                    } else {
                                        this.presentation = None;
                                    }
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
        }
        let Some(page) = self.presentation(thumbnail) else {
            return gpui_kit::div().into_any_element();
        };
        let mut snapshot = self.snapshot(thumbnail, page.id.clone(), cx);
        if snapshot.host_failed && page.recovery.is_none() {
            let (recovery, mut requests) =
                Surface::new(snapshot.clone(), render_recovery, window, cx);
            if let Some(page) = if thumbnail {
                self.thumbnail_presentation.as_mut()
            } else {
                self.presentation.as_mut()
            } {
                page.recovery = Some(recovery);
            }
            cx.spawn(async move |this, cx| {
                while let Some(request) = requests.recv().await {
                    if this
                        .update(cx, |this, cx| match request {
                            Request::Action {
                                observed,
                                action,
                                reply,
                            } => {
                                let accepted = matches!(action, ImagePreviewAction::Save)
                                    && this.accepts(&observed, action);
                                if accepted {
                                    this.action(action, cx);
                                }
                                if let Some(page) = this.presentation(thumbnail)
                                    && let Some(recovery) = &page.recovery
                                {
                                    recovery.publish(this.snapshot(thumbnail, page.id.clone(), cx));
                                }
                                let _ = reply.send(accepted);
                            }
                            Request::Reload(id) => {
                                if this
                                    .presentation(thumbnail)
                                    .is_some_and(|page| page.id.matches(&id))
                                {
                                    if thumbnail {
                                        this.thumbnail_presentation = None;
                                    } else {
                                        this.presentation = None;
                                    }
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
        }
        let legacy_focused = if thumbnail {
            self.thumbnail_focus.is_focused(window)
        } else {
            self.focus.is_focused(window)
        };
        let page = if thumbnail {
            self.thumbnail_presentation.as_mut()
        } else {
            self.presentation.as_mut()
        };
        let Some(page) = page else {
            return gpui_kit::div().into_any_element();
        };
        if legacy_focused {
            page.focus_revision = page.focus_revision.wrapping_add(1);
            snapshot.focus_revision = page.focus_revision;
        }
        let surface = page.recovery.as_mut().unwrap_or(&mut page.surface);
        surface.sync_environment(cx);
        surface.publish(snapshot);
        if legacy_focused {
            surface.focus(window, cx);
        }
        gpui_kit::div()
            .size_full()
            .min_w_0()
            .min_h_0()
            .child(surface.entity())
            .into_any_element()
    }
}

#[derive(gpui_kit::IntoElement)]
pub(super) struct ThumbnailHost {
    pub(super) view: Entity<ImagePreview>,
    pub(super) size: (f32, f32),
    pub(super) screenshot: bool,
}

impl RenderOnce for ThumbnailHost {
    fn render(self, window: &mut Window, cx: &mut App) -> impl gpui_kit::IntoElement {
        use gpui_kit::{ParentElement, Styled};
        let content = self.view.update(cx, |view, cx| {
            if view.thumbnail_screenshot != self.screenshot {
                view.thumbnail_presentation = None;
                view.thumbnail_screenshot = self.screenshot;
            }
            view.render_nsh(true, window, cx)
        });
        let host = gpui_kit::div()
            .relative()
            .w(gpui_kit::px(self.size.0))
            .max_w_full()
            .flex_shrink_0();
        let host = if self.screenshot {
            host.aspect_ratio(self.size.0 / self.size.1)
        } else {
            host.h(gpui_kit::px(self.size.1))
        };
        host.child(content)
            .child(ImagePreview::thumbnail_tracker(&self.view))
    }
}

#[derive(Clone, Copy)]
struct Actions {
    state: Signal<Snapshot>,
    pending: Signal<bool>,
    failed: Signal<bool>,
    trigger: ElementRef,
    restore_focus: Signal<bool>,
}

impl Actions {
    fn send(self, commands: mpsc::Sender<Request>, action: ImagePreviewAction) {
        if self.pending.get_untracked() {
            return;
        }
        self.pending.set(true);
        self.failed.set(false);
        let observed = self.state.get_untracked();
        spawn(async move {
            let (reply, response) = oneshot::channel();
            if commands
                .send(Request::Action {
                    observed: Box::new(observed),
                    action,
                    reply,
                })
                .await
                .is_err()
                || !matches!(response.await, Ok(true))
            {
                self.failed.set(true);
            }
            self.pending.set(false);
        });
    }
}

fn control(state: Signal<Snapshot>, target: ElementRef) -> View {
    let focused = signal(false);
    target.on_focus_in(move || focused.set(true));
    target.on_focus_out(move || focused.set(false));
    view()
        .element_ref(&target)
        .role(AccessibilityRole::Button)
        .focusable(true)
        .px_2()
        .h(px(Layout::IMAGE_PREVIEW_CONTROL))
        .items_center()
        .justify_center()
        .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
        .border(px(1.))
        .border_color(move || {
            crate::nsh::color(if focused.get() {
                state.get().colors.brand_primary
            } else {
                state.get().colors.bg_base
            })
        })
        .hover(move |style| style.bg(move || crate::nsh::color(state.get().colors.bg_hover)))
}

fn image_content(state: Signal<Snapshot>) -> AnyElement {
    view()
        .size_full()
        .child(when(
            move || state.get().source.is_none(),
            move || text(move || vega_i18n::t("ui.loading")),
        ))
        .child(when(
            move || state.get().source.is_some(),
            move || {
                let snapshot = state.get_untracked();
                let source = snapshot.source.unwrap_or_default();
                let loaded = signal(false);
                let failed = signal(false);
                view()
                    .size_full()
                    .relative()
                    .items_center()
                    .justify_center()
                    .child(
                        image(source)
                            .size_full()
                            .max_w(px(snapshot.width))
                            .max_h(px(snapshot.height))
                            .object_fit(ResizeMode::Contain)
                            .label(snapshot.name)
                            .on_load(move |_: &Size<DevicePixels>| loaded.set(true))
                            .on_error(move |_: &str| failed.set(true)),
                    )
                    .child(when(
                        move || !loaded.get(),
                        move || {
                            view()
                                .absolute()
                                .inset_0()
                                .items_center()
                                .justify_center()
                                .child(text(move || {
                                    if failed.get() {
                                        vega_i18n::t(
                                            "ui.this_image_cannot_be_displayed_save_locally_then",
                                        )
                                    } else {
                                        vega_i18n::t("ui.loading")
                                    }
                                }))
                        },
                    ))
                    .into_element()
            },
        ))
        .into_element()
}

fn applications(actions: Actions, commands: mpsc::Sender<Request>) -> AnyElement {
    let revision = computed(move || actions.state.with(|snapshot| snapshot.menu_revision));
    dynamic(move || {
        let _ = revision.get();
        let snapshot = actions.state.get_untracked();
        if snapshot.applications.is_empty() {
            return view().into_element();
        }
        let items = snapshot.applications.clone();
        let targets = items.iter().map(|_| element_ref()).collect::<Vec<_>>();
        let focused = signal(0usize);
        let first = signal(true);
        let menu_scroll = element_ref();
        let row_bounds = signal(vec![(0.0f32, 0.0f32); targets.len()]);
        let scroll_offset = signal(0.0f32);
        let scroll_height = signal(Layout::TOOL_GROUP_MAX_HEIGHT);
        let reveal = move |index: usize| {
            let (top, height) = row_bounds.get_untracked()[index];
            let offset = scroll_offset.get_untracked();
            let viewport = scroll_height.get_untracked();
            let next = if top < offset {
                top
            } else if top + height > offset + viewport {
                top + height - viewport
            } else {
                offset
            };
            if (next - offset).abs() > 0.5 {
                scroll_offset.set(next.max(0.));
                menu_scroll.scroll_to(point(px(0.), px(next.max(0.))), false);
            }
        };
        let popup = element_ref();
        let within = signal(false);
        popup.on_focus_in(move || within.set(true));
        popup.on_focus_out(move || within.set(false));
        let shortcuts = targets.clone();
        let keyboard = when(
            move || within.get(),
            move || {
                for (key, direction) in [
                    ("down", 1isize),
                    ("up", -1),
                    ("home", 0),
                    ("end", 2),
                    ("tab", 1),
                    ("shift+tab", -1),
                ] {
                    let targets = shortcuts.clone();
                    on_key(key, move |_| {
                        let len = targets.len();
                        let index = match direction {
                            0 => 0,
                            2 => len - 1,
                            delta => (focused.get_untracked() as isize + delta)
                                .rem_euclid(len as isize)
                                as usize,
                        };
                        focused.set(index);
                        targets[index].focus();
                        reveal(index);
                    });
                }
                view()
            },
        );
        let dismiss = commands.clone();
        let cancel = commands.clone();
        overlay()
            .element_ref(&popup)
            .child(keyboard)
            .anchor_content(&actions.trigger)
            .align(Align::End)
            .margin(px(Layout::IMAGE_PREVIEW_HEADER_PADDING))
            .w(px(Layout::SIDEBAR_WIDTH))
            .bg(move || crate::nsh::color(actions.state.get().colors.bg_elevated))
            .border(px(1.))
            .border_color(move || crate::nsh::color(actions.state.get().colors.border_subtle))
            .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
            .role(AccessibilityRole::Menu)
            .on_press_outside(move |_: &Point<Points>| {
                actions.restore_focus.set(true);
                actions.send(dismiss.clone(), ImagePreviewAction::DismissApplications);
            })
            .on_cancel(move || {
                actions.restore_focus.set(true);
                actions.send(cancel.clone(), ImagePreviewAction::DismissApplications);
            })
            .child(
                scroll()
                    .element_ref(&menu_scroll)
                    .on_layout(move |frame: &Bounds<Points>| scroll_height.set(frame.size.height.0))
                    .on_scroll(move |event: &ScrollEvent| scroll_offset.set(event.offset.y.0))
                    .max_h(px(Layout::TOOL_GROUP_MAX_HEIGHT))
                    .children(items.iter().enumerate().map(|(index, _)| {
                        let target = targets[index];
                        let selected = commands.clone();
                        let mut observed = snapshot.clone();
                        observed.applications = items.clone();
                        let highlighted = signal(false);
                        target.on_focus_in(move || {
                            focused.set(index);
                            highlighted.set(true);
                            reveal(index);
                        });
                        target.on_focus_out(move || highlighted.set(false));
                        view()
                            .element_ref(&target)
                            .role(AccessibilityRole::Menuitem)
                            .focusable(true)
                            .label(move || {
                                actions
                                    .state
                                    .get()
                                    .applications
                                    .get(index)
                                    .cloned()
                                    .unwrap_or_default()
                            })
                            .px_2()
                            .py_2()
                            .bg(move || {
                                crate::nsh::color(if highlighted.get() {
                                    actions.state.get().colors.bg_active
                                } else {
                                    actions.state.get().colors.bg_elevated
                                })
                            })
                            .hover(move |style| {
                                style.bg(move || {
                                    crate::nsh::color(actions.state.get().colors.bg_hover)
                                })
                            })
                            .on_layout(move |frame: &Bounds<Points>| {
                                row_bounds.update(|bounds| {
                                    bounds[index] = (frame.origin.y.0, frame.size.height.0)
                                });
                                if index == 0 && first.get_untracked() {
                                    first.set(false);
                                    target.focus();
                                }
                            })
                            .on_press(move || {
                                if actions.pending.get_untracked() {
                                    return;
                                }
                                actions.restore_focus.set(true);
                                actions.trigger.focus();
                                let (reply, response) = oneshot::channel();
                                let request = Request::Action {
                                    observed: Box::new(observed.clone()),
                                    action: ImagePreviewAction::ChooseApplication(index),
                                    reply,
                                };
                                let commands = selected.clone();
                                actions.pending.set(true);
                                spawn(async move {
                                    if commands.send(request).await.is_err()
                                        || response.await.is_err()
                                    {
                                        actions.failed.set(true);
                                    }
                                    actions.pending.set(false);
                                });
                            })
                            .child(text(move || {
                                actions
                                    .state
                                    .get()
                                    .applications
                                    .get(index)
                                    .cloned()
                                    .unwrap_or_default()
                            }))
                    })),
            )
            .into_element()
    })
    .into_element()
}

fn render_recovery(state: Signal<Snapshot>, commands: mpsc::Sender<Request>) -> AnyElement {
    let actions = Actions {
        state,
        pending: signal(false),
        failed: signal(false),
        trigger: element_ref(),
        restore_focus: signal(false),
    };
    let business = commands.clone();
    let commands = OrderedCommands::new(commands);
    let target = element_ref();
    let focused = signal(None::<u64>);
    let focus = move || {
        let revision = state.get_untracked().focus_revision;
        if focused.get_untracked() != Some(revision) && target.node().is_some() {
            focused.set(Some(revision));
            target.focus();
        }
    };
    effect(move || {
        let _ = state.get().focus_revision;
        focus();
    });
    view()
        .on_layout(move |_: &Bounds<Points>| focus())
        .size_full()
        .p_2()
        .bg(move || crate::nsh::color(state.get().colors.bg_subtle))
        .text_color(move || crate::nsh::color(state.get().colors.text_primary))
        .text_size(px(Typography::BODY))
        .child(text(move || {
            vega_i18n::t("ui.this_image_cannot_be_displayed_save_locally_then")
        }))
        .child(when(
            move || !state.get().thumbnail,
            move || {
                let business = business.clone();
                control(state, actions.trigger)
                    .label(move || vega_i18n::t("ui.save_locally"))
                    .disabled(move || actions.pending.get() || state.get().busy)
                    .on_press(move || actions.send(business.clone(), ImagePreviewAction::Save))
                    .child(text(move || vega_i18n::t("ui.save_locally")))
            },
        ))
        .child(dynamic(move || {
            state.get().status.map_or_else(
                || view().into_element(),
                |status| {
                    text(status)
                        .text_color(crate::nsh::color(state.get().colors.text_secondary))
                        .into_element()
                },
            )
        }))
        .child(when(
            move || actions.failed.get(),
            move || text(move || vega_i18n::t("ui.command_failed")),
        ))
        .child(
            control(state, target)
                .label(move || vega_i18n::t("ui.retry"))
                .on_press(move || commands.send(Request::Reload(state.get_untracked().id)))
                .child(text(move || vega_i18n::t("ui.retry"))),
        )
        .into_element()
}

fn viewport(state: Signal<Snapshot>) -> AnyElement {
    let extent = signal((0.0f32, 0.0f32));
    view()
        .flex_1()
        .min_w_0()
        .min_h_0()
        .on_layout(move |frame: &Bounds<Points>| {
            let size = (frame.size.width.0, frame.size.height.0);
            if size.0.is_finite() && size.1.is_finite() && size.0 > 0. && size.1 > 0. {
                extent.set(size);
            }
        })
        .child(
            scroll().size_full().child(
                view()
                    .w_full()
                    .h(move || px(extent.get().1.max(Layout::IMAGE_PREVIEW_CONTROL)))
                    .p(px(Layout::IMAGE_PREVIEW_PADDING))
                    .child(image_content(state)),
            ),
        )
        .into_element()
}

fn render(state: Signal<Snapshot>, commands: mpsc::Sender<Request>) -> AnyElement {
    let actions = Actions {
        state,
        pending: signal(false),
        failed: signal(false),
        trigger: element_ref(),
        restore_focus: signal(false),
    };
    let root = element_ref();
    let thumbnail_target = element_ref();
    let focused_revision = signal(0u64);
    let focus_due = move || {
        let snapshot = state.get_untracked();
        let target = if snapshot.thumbnail {
            thumbnail_target
        } else {
            actions.trigger
        };
        if snapshot.focus_revision != focused_revision.get_untracked() && target.node().is_some() {
            focused_revision.set(snapshot.focus_revision);
            target.focus();
        }
    };
    effect(move || {
        let _ = state.get().focus_revision;
        focus_due();
    });
    let within = signal(false);
    root.on_focus_in(move || within.set(true));
    root.on_focus_out(move || within.set(false));
    let reload = OrderedCommands::new(commands.clone());
    effect(move || {
        let snapshot = state.get();
        if actions.restore_focus.get() && snapshot.applications.is_empty() && !actions.pending.get()
        {
            actions.restore_focus.set(false);
            actions.trigger.focus();
        }
    });
    let escape = commands.clone();
    let thumbnail = commands.clone();
    let toggle = commands.clone();
    let save = commands.clone();
    let cancel = commands.clone();
    let menu = commands.clone();
    view()
        .element_ref(&root)
        .on_layout(move |_: &Bounds<Points>| focus_due())
        .on_press(move || root.focus())
        .size_full()
        .min_w_0()
        .min_h_0()
        .bg(move || crate::nsh::color(state.get().colors.bg_subtle))
        .text_color(move || crate::nsh::color(state.get().colors.text_primary))
        .text_size(px(Typography::BODY))
        .child(when(
            move || within.get() && state.get().applications.is_empty() && !state.get().thumbnail,
            move || {
                let commands = escape.clone();
                on_key("escape", move |_| {
                    actions.send(
                        commands.clone(),
                        if state.get_untracked().requested {
                            ImagePreviewAction::DismissApplications
                        } else {
                            ImagePreviewAction::Close
                        },
                    );
                });
                view()
            },
        ))
        .child(when(
            move || state.get().host_failed || actions.failed.get(),
            move || {
                let reload = reload.clone();
                view()
                    .p_2()
                    .child(text(vega_i18n::t(
                        "ui.this_image_cannot_be_displayed_save_locally_then",
                    )))
                    .child(
                        control(state, element_ref())
                            .label(vega_i18n::t("ui.retry"))
                            .on_press(move || {
                                reload.send(Request::Reload(state.get_untracked().id))
                            })
                            .child(text(vega_i18n::t("ui.retry"))),
                    )
            },
        ))
        .child(when(
            move || state.get().thumbnail,
            move || {
                let commands = thumbnail.clone();
                let focused = signal(false);
                let hovered = signal(false);
                thumbnail_target.on_focus_in(move || focused.set(true));
                thumbnail_target.on_focus_out(move || focused.set(false));
                control(state, thumbnail_target)
                    .size_full()
                    .label(move || {
                        if state.get().screenshot {
                            vega_i18n::t("ui.view_full_screenshot").to_owned()
                        } else {
                            vega_i18n::t_args("ui.preview_name", &[("name", &state.get().name)])
                        }
                    })
                    .tooltip(
                        nsh_base::tooltip()
                            .open(move || Some(focused.get() || hovered.get()))
                            .on_open_change(move |event: &nsh_base::TooltipEvent| {
                                if let nsh_base::TooltipEvent::OpenChange(open, _) = event {
                                    hovered.set(*open);
                                }
                            })
                            .p_2()
                            .bg(move || crate::nsh::color(state.get().colors.bg_elevated))
                            .text_color(move || crate::nsh::color(state.get().colors.text_primary))
                            .child(text(move || vega_i18n::t("ui.view_full_image_enter"))),
                    )
                    .px_0()
                    .border(move || {
                        px(if state.get().screenshot {
                            Layout::SCREENSHOT_BORDER
                        } else {
                            1.
                        })
                    })
                    .border_color(move || {
                        crate::nsh::color(if focused.get() {
                            state.get().colors.brand_primary
                        } else if hovered.get() {
                            state.get().colors.text_secondary
                        } else {
                            state.get().colors.border_subtle
                        })
                    })
                    .rounded(move || {
                        px(if state.get().screenshot {
                            Layout::SCREENSHOT_RADIUS
                        } else {
                            Layout::SETTINGS_CONTROL_RADIUS
                        })
                    })
                    .overflow_hidden()
                    .bg(move || crate::nsh::color(state.get().colors.bg_elevated))
                    .on_press(move || actions.send(commands.clone(), ImagePreviewAction::Open))
                    .child(image_content(state))
            },
        ))
        .child(when(
            move || !state.get().thumbnail,
            move || {
                let toggle = toggle.clone();
                let save = save.clone();
                let cancel = cancel.clone();
                let menu = menu.clone();
                view()
                    .size_full()
                    .min_w_0()
                    .min_h_0()
                    .child(
                        view()
                            .flex_row()
                            .flex_wrap()
                            .items_center()
                            .gap(px(Layout::IMAGE_PREVIEW_GAP))
                            .px(px(Layout::IMAGE_PREVIEW_HEADER_PADDING))
                            .min_h(px(Layout::IMAGE_PREVIEW_HEADER))
                            .flex_shrink_0()
                            .bg(move || crate::nsh::color(state.get().colors.bg_base))
                            .child(text(move || state.get().name).flex_1().min_w_0())
                            .child(
                                control(state, actions.trigger)
                                    .label(vega_i18n::t("ui.open_with"))
                                    .disabled(move || {
                                        actions.pending.get()
                                            || (state.get().busy && !state.get().requested)
                                    })
                                    .accessibility_state(move || AccessibilityState {
                                        expanded: Some(!state.get().applications.is_empty()),
                                        busy: state.get().requested,
                                        ..Default::default()
                                    })
                                    .on_press(move || {
                                        actions
                                            .send(toggle.clone(), ImagePreviewAction::Applications);
                                    })
                                    .child(text(move || vega_i18n::t("ui.open_with"))),
                            )
                            .child(
                                control(state, element_ref())
                                    .label(vega_i18n::t("ui.save_locally"))
                                    .disabled(move || state.get().busy || actions.pending.get())
                                    .on_press(move || {
                                        actions.send(save.clone(), ImagePreviewAction::Save)
                                    })
                                    .child(text(move || vega_i18n::t("ui.save_locally"))),
                            ),
                    )
                    .child(when(
                        move || state.get().busy,
                        move || {
                            let cancel = cancel.clone();
                            view()
                                .flex_row()
                                .items_center()
                                .child(text(move || vega_i18n::t("ui.loading")))
                                .child(when(
                                    move || state.get().requested,
                                    move || {
                                        let cancel = cancel.clone();
                                        control(state, element_ref())
                                            .label(vega_i18n::t("ui.cancel"))
                                            .on_press(move || {
                                                actions.send(
                                                    cancel.clone(),
                                                    ImagePreviewAction::DismissApplications,
                                                )
                                            })
                                            .child(text(move || vega_i18n::t("ui.cancel")))
                                    },
                                ))
                        },
                    ))
                    .child(dynamic(move || {
                        state.get().status.map_or_else(
                            || view().into_element(),
                            |status| {
                                text(status)
                                    .px_3()
                                    .text_size(px(Typography::METADATA))
                                    .text_color(crate::nsh::color(
                                        state.get().colors.text_secondary,
                                    ))
                                    .into_element()
                            },
                        )
                    }))
                    .child(viewport(state))
                    .child(applications(actions, menu))
            },
        ))
        .into_element()
}
