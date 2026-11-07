use super::{ConversationStream, ImagePreview};
use crate::nsh::{ProjectionId, Surface};
use gpui_kit::{prelude::*, *};
use nsh_platform_gpui::NativeComponent;
use std::cell::Cell;
use std::rc::{Rc, Weak};
use vega_theme::{Layout, ThemeColors};

mod pixels;
mod removal;

#[derive(Clone)]
struct Source {
    epoch: ProjectionId,
    thread: String,
    generation: u64,
    attachment: u64,
    image: u64,
    bytes: usize,
    length: usize,
}

impl Source {
    fn matches(&self, other: &Self) -> bool {
        self.epoch.matches(&other.epoch)
            && self.thread == other.thread
            && self.generation == other.generation
            && self.attachment == other.attachment
            && self.image == other.image
            && self.bytes == other.bytes
            && self.length == other.length
    }

    fn current(&self, stream: &ConversationStream, cx: &App) -> bool {
        self.epoch.matches(&stream.run_notice_epoch)
            && self.thread == stream.thread.id
            && self.generation == stream.attachment_generation
            && !stream.reading_preview
            && !stream.permissions_revoked
            && cx
                .try_global::<crate::settings::SettingsOpen>()
                .is_some_and(|settings| !settings.0)
            && cx
                .try_global::<crate::sidebar::OpenedThread>()
                .and_then(|route| route.0.as_ref())
                .is_some_and(|thread| thread.id == self.thread)
            && stream.attachments.iter().any(|(id, image)| {
                *id == self.attachment
                    && image.view.entity_id().as_u64() == self.image
                    && image.image.bytes().as_ptr() as usize == self.bytes
                    && image.image.bytes().len() == self.length
            })
    }
}

#[derive(Clone)]
struct Snapshot {
    id: ProjectionId,
    items: Vec<u64>,
    history: bool,
    pending: bool,
    error: Option<&'static str>,
    recovery: bool,
    focused: bool,
    language: &'static str,
    colors: ThemeColors,
}

enum Request {
    Layout { id: ProjectionId, height: f32 },
    Reload(ProjectionId),
}

#[derive(gpui_kit::IntoElement)]
pub(super) struct AttachmentGroup {
    owner: Option<WeakEntity<ConversationStream>>,
    source: Vec<Source>,
    images: Vec<ImagePreview>,
    pending: bool,
    error: Option<&'static str>,
}

impl AttachmentGroup {
    pub(super) fn composer(stream: &ConversationStream, cx: &Context<ConversationStream>) -> Self {
        Self {
            owner: Some(cx.weak_entity()),
            source: stream
                .attachments
                .iter()
                .map(|(id, image)| Source {
                    epoch: stream.run_notice_epoch.clone(),
                    thread: stream.thread.id.clone(),
                    generation: stream.attachment_generation,
                    attachment: *id,
                    image: image.view.entity_id().as_u64(),
                    bytes: image.image.bytes().as_ptr() as usize,
                    length: image.image.bytes().len(),
                })
                .collect(),
            images: stream
                .attachments
                .iter()
                .map(|(_, image)| image.clone())
                .collect(),
            pending: stream.attachment_import_pending,
            error: stream.attachment_error,
        }
    }

    pub(super) fn history(images: &[ImagePreview]) -> Self {
        Self {
            owner: None,
            source: Vec::new(),
            images: images.to_vec(),
            pending: false,
            error: None,
        }
    }
}

struct GroupHost {
    owner: Option<WeakEntity<ConversationStream>>,
    source: Vec<Source>,
    epoch: Option<ProjectionId>,
    images: Vec<ImagePreview>,
    snapshot: Snapshot,
    surface: Option<Surface<Snapshot, Request>>,
    height: Option<f32>,
    lease: Weak<GroupMount>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

#[derive(Default)]
struct GroupMount {
    visible: Cell<Option<Bounds<Pixels>>>,
    active: Cell<bool>,
}

impl GroupHost {
    fn stop(&mut self, cx: &App) {
        if let Some(mount) = self.lease.upgrade() {
            mount.active.set(false);
            mount.visible.set(None);
        }
        if let Some(surface) = self.surface.take() {
            surface.entity().read(cx).stop();
        }
        self.snapshot.id = ProjectionId::new();
        self.height = None;
        self.lease = Weak::new();
    }

    fn current(&self, cx: &App) -> bool {
        match (&self.owner, &self.epoch) {
            (Some(owner), Some(epoch)) => owner.upgrade().is_some_and(|owner| {
                let stream = owner.read(cx);
                stream.run_notice_epoch.matches(epoch)
                    && !stream.reading_preview
                    && !stream.permissions_revoked
                    && cx
                        .try_global::<crate::settings::SettingsOpen>()
                        .is_some_and(|settings| !settings.0)
                    && cx
                        .try_global::<crate::sidebar::OpenedThread>()
                        .and_then(|route| route.0.as_ref())
                        .is_some_and(|thread| thread.id == stream.thread.id)
                    && self.source.iter().all(|source| source.current(stream, cx))
            }),
            (None, None) => true,
            _ => false,
        }
    }

    fn mount(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.surface.is_some() {
            return;
        }
        let (surface, mut requests) =
            Surface::new(self.snapshot.clone(), pixels::render, window, cx);
        if !self.snapshot.recovery {
            for (index, image) in self.images.iter().enumerate() {
                let view = image.view.clone();
                let name = format!("attachment-thumbnail-{index}");
                surface.entity().read(cx).register_component(
                    &name,
                    Rc::new(move |_| {
                        Box::new(Thumbnail {
                            view: view.clone(),
                            focus: None,
                        })
                    }),
                );
                if let (Some(owner), Some(source)) = (&self.owner, self.source.get(index)) {
                    let owner = owner.clone();
                    let source = source.clone();
                    let group = cx.weak_entity();
                    surface.entity().read(cx).register_component(
                        &format!("attachment-remove-{index}"),
                        Rc::new(move |_| {
                            Box::new(removal::Removal::new(
                                owner.clone(),
                                source.clone(),
                                group.clone(),
                            ))
                        }),
                    );
                }
            }
        } else {
            let weak = cx.weak_entity();
            surface.entity().read(cx).register_component(
                "attachment-recovery-focus",
                Rc::new(move |_| {
                    Box::new(RecoveryFocus {
                        host: weak.clone(),
                        focus: None,
                    })
                }),
            );
        }
        let mut errors = surface.errors();
        let id = self.snapshot.id.clone();
        cx.spawn(async move |this, cx| {
            while let Some(request) = requests.recv().await {
                if this
                    .update_in(cx, |this, window, cx| match request {
                        Request::Layout { id, height } => {
                            if this.snapshot.id.matches(&id)
                                && height.is_finite()
                                && height >= 0.
                                && this
                                    .height
                                    .is_none_or(|previous| (previous - height).abs() > 0.5)
                            {
                                this.height = Some(height);
                                if let Some(owner) =
                                    this.owner.as_ref().and_then(WeakEntity::upgrade)
                                {
                                    owner.update(cx, |_, cx| cx.notify());
                                }
                                cx.notify();
                            }
                        }
                        Request::Reload(id) => this.reload(&id, window, cx),
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
                        if this.snapshot.id.matches(&id) {
                            this.stop(cx);
                            this.snapshot.recovery = true;
                            cx.notify();
                        }
                    });
                    break;
                }
            }
        })
        .detach();
        self.surface = Some(surface);
    }

    fn reload(&mut self, id: &ProjectionId, window: &mut Window, cx: &mut Context<Self>) {
        if self.snapshot.id.matches(id)
            && self.snapshot.recovery
            && self.current(cx)
            && window.is_window_active()
            && self.lease.upgrade().is_some_and(|lease| {
                lease.active.get() && lease.visible.get().is_some_and(|bounds| !bounds.is_empty())
            })
        {
            self.stop(cx);
            self.snapshot.recovery = false;
            cx.notify();
        }
    }
}

impl RenderOnce for AttachmentGroup {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let history = self.owner.is_none();
        let items = self
            .images
            .iter()
            .map(|image| image.view.entity_id().as_u64())
            .collect::<Vec<_>>();
        let epoch = self
            .owner
            .as_ref()
            .and_then(WeakEntity::upgrade)
            .map(|owner| owner.read(cx).run_notice_epoch.clone());
        let key = if let Some(owner) = &self.owner {
            format!("nsh-composer-attachments-{}", owner.entity_id())
        } else {
            format!("nsh-history-attachments-{items:?}")
        };
        let snapshot = Snapshot {
            id: ProjectionId::new(),
            items,
            history,
            pending: self.pending,
            error: self.error,
            recovery: false,
            focused: false,
            language: vega_i18n::language(),
            colors: vega_theme::theme(cx).colors,
        };
        let host = window.use_keyed_state(key, cx, |window, cx| {
            let focus = cx.focus_handle().tab_stop(true);
            let subscriptions = vec![
                cx.on_release(|this: &mut GroupHost, cx| this.stop(cx)),
                cx.on_focus_in(&focus, window, |_, _, cx| cx.notify()),
                cx.on_focus_out(&focus, window, |_, _, _, cx| cx.notify()),
            ];
            GroupHost {
                owner: self.owner.clone(),
                source: self.source.clone(),
                epoch: epoch.clone(),
                images: self.images.clone(),
                snapshot: snapshot.clone(),
                surface: None,
                height: None,
                lease: Weak::new(),
                focus,
                _subscriptions: subscriptions,
            }
        });
        host.update(cx, |this, cx| {
            let same_epoch = match (&this.epoch, &epoch) {
                (Some(old), Some(new)) => old.matches(new),
                (None, None) => true,
                _ => false,
            };
            if this.snapshot.items != snapshot.items
                || !same_epoch
                || this.source.len() != self.source.len()
                || !this
                    .source
                    .iter()
                    .zip(&self.source)
                    .all(|(old, new)| old.matches(new))
            {
                this.stop(cx);
                this.snapshot.recovery = false;
            }
            let mut snapshot = snapshot;
            snapshot.id = this.snapshot.id.clone();
            snapshot.recovery = this.snapshot.recovery;
            this.snapshot = snapshot;
            this.owner = self.owner;
            this.source = self.source;
            this.images = self.images;
            this.epoch = epoch;
        });
        GroupElement { host }.into_any_element()
    }
}

#[derive(gpui_kit::IntoElement)]
struct GroupElement {
    host: Entity<GroupHost>,
}

impl RenderOnce for GroupElement {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        GroupLayout { host: self.host }
    }
}

struct GroupLayout {
    host: Entity<GroupHost>,
}

impl IntoElement for GroupLayout {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for GroupLayout {
    type RequestLayoutState = ();
    type PrepaintState = Option<AnyElement>;
    fn id(&self) -> Option<ElementId> {
        Some("nsh-attachment-layout".into())
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        _: &mut App,
    ) -> (LayoutId, ()) {
        let host = self.host.clone();
        let style = Style {
            flex_shrink: 0.,
            ..Default::default()
        };
        (
            window.request_measured_layout(style, move |known, available, _, cx| {
                let width = known
                    .width
                    .or(match available.width {
                        AvailableSpace::Definite(width) => Some(width),
                        _ => None,
                    })
                    .unwrap_or(px(Layout::ATTACHMENT_THUMBNAIL));
                let state = host.read(cx);
                let count = state.snapshot.items.len();
                let columns = (((f32::from(width) + 8.) / (Layout::ATTACHMENT_THUMBNAIL + 8.))
                    .floor() as usize)
                    .max(1);
                let rows = count.div_ceil(columns);
                let grid = rows as f32 * (Layout::ATTACHMENT_THUMBNAIL + 8.)
                    - if rows > 0 { 8. } else { 0. };
                let inset = if state.snapshot.history {
                    Layout::USER_MESSAGE_PADDING_TOP
                } else {
                    0.
                };
                let status = usize::from(state.snapshot.pending)
                    + usize::from(state.snapshot.error.is_some());
                let estimated = if state.snapshot.recovery {
                    32.
                } else {
                    inset + grid + status as f32 * 32.
                };
                size(
                    width,
                    known
                        .height
                        .unwrap_or(px(state.height.unwrap_or(estimated).max(1.))),
                )
            }),
            (),
        )
    }
    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        let (mut element, lease) = self.host.update(cx, |this, cx| {
            if !this.current(cx) {
                this.stop(cx);
                return (None, None);
            }
            let lease = this.lease.upgrade().unwrap_or_else(|| {
                this.stop(cx);
                Rc::new(GroupMount::default())
            });
            this.lease = Rc::downgrade(&lease);
            this.mount(window, cx);
            this.snapshot.focused = this.focus.is_focused(window);
            let snapshot = this.snapshot.clone();
            let element = this.surface.as_mut().map(|surface| {
                surface.sync_environment(cx);
                surface.publish(snapshot);
                surface.entity().into_any_element()
            });
            (element, Some(lease))
        });
        if let Some(lease) = lease {
            lease.active.set(true);
            let visible = bounds.intersect(&window.content_mask().bounds);
            lease.visible.set((!visible.is_empty()).then_some(visible));
            if let Some(id) = id {
                window.with_element_state::<Rc<GroupMount>, _>(id, |_, _| ((), lease));
            }
        }
        if let Some(element) = &mut element {
            element.prepaint_as_root(
                bounds.origin,
                size(
                    AvailableSpace::Definite(bounds.size.width),
                    AvailableSpace::Definite(bounds.size.height),
                ),
                window,
                cx,
            );
        }
        element
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        element: &mut Option<AnyElement>,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(element) = element {
            element.paint(window, cx);
        }
    }
}

struct Thumbnail {
    view: Entity<crate::image_preview::ImagePreview>,
    focus: Option<FocusHandle>,
}

impl NativeComponent for Thumbnail {
    fn render(&mut self, _: &mut Window, cx: &mut App) -> Option<AnyElement> {
        self.focus = Some(self.view.read(cx).thumbnail_focus());
        Some(crate::image_preview::ImagePreview::thumbnail(
            &self.view, cx,
        ))
    }
    fn focus_handle(&self) -> Option<FocusHandle> {
        self.focus.clone()
    }
    fn set_prop(&mut self, _: &str, _: Option<&serde_json::Value>, _: &mut Window, _: &mut App) {}
}

struct RecoveryFocus {
    host: WeakEntity<GroupHost>,
    focus: Option<FocusHandle>,
}

impl NativeComponent for RecoveryFocus {
    fn render(&mut self, _: &mut Window, cx: &mut App) -> Option<AnyElement> {
        let host = self.host.upgrade()?;
        let state = host.read(cx);
        let focus = state.focus.clone();
        self.focus = Some(focus.clone());
        let id = state.snapshot.id.clone();
        let weak = self.host.clone();
        let pointer = self.host.clone();
        let pointer_id = id.clone();
        Some(
            div()
                .id("attachment-recovery-action")
                .size_full()
                .track_focus(&focus)
                .role(Role::Button)
                .aria_label(vega_i18n::t("nsh.preview.reload"))
                .on_mouse_up(MouseButton::Left, move |_, window, cx| {
                    let _ = pointer.update(cx, |this, cx| this.reload(&pointer_id, window, cx));
                    cx.stop_propagation();
                })
                .on_a11y_action(AccessibleAction::Click, move |_, window, cx| {
                    let _ = weak.update(cx, |this, cx| this.reload(&id, window, cx));
                })
                .capture_key_down(move |event, window, cx| {
                    use crate::nsh::keyboard::Key;
                    if matches!(Key::from_event(event), Key::Activate) && focus.is_focused(window) {
                        host.update(cx, |this, cx| {
                            this.reload(&this.snapshot.id.clone(), window, cx)
                        });
                        cx.stop_propagation();
                    }
                })
                .into_any_element(),
        )
    }
    fn focus_handle(&self) -> Option<FocusHandle> {
        self.focus.clone()
    }
    fn set_prop(&mut self, _: &str, _: Option<&serde_json::Value>, _: &mut Window, _: &mut App) {}
}
