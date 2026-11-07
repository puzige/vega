use super::{
    ConversationStream, CredentialContinuationRequested, RunFailureKind, RunNotice, RunNoticeKind,
};
use crate::nsh::keyboard::{Key, Selection, Step};
use crate::nsh::{ProjectionId, Surface};
use gpui_kit::{prelude::*, *};
use std::cell::Cell;
use std::rc::{Rc, Weak};
use vega_theme::ThemeColors;

mod pixels;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Continue,
    Settings,
}

impl Action {
    fn label(self) -> &'static str {
        vega_i18n::t(match self {
            Self::Continue => "ui.continue",
            Self::Settings => "ui.check_credential_settings",
        })
    }
}

#[derive(Clone)]
struct Source {
    epoch: ProjectionId,
    thread_id: String,
    index: usize,
    identity: super::super::model::StreamEntryIdentity,
    notice: RunNotice,
    reading_preview: bool,
}

impl Source {
    fn matches(&self, other: &Self) -> bool {
        self.epoch.matches(&other.epoch)
            && self.thread_id == other.thread_id
            && self.index == other.index
            && self.identity == other.identity
            && self.notice == other.notice
            && self.reading_preview == other.reading_preview
    }

    fn current(&self, stream: &ConversationStream, cx: &App) -> bool {
        if !stream.run_notice_epoch.matches(&self.epoch)
            || stream.thread.id != self.thread_id
            || stream.reading_preview
            || stream.permissions_revoked
            || self.reading_preview
            || stream.entry_identities.get(self.index) != Some(&self.identity)
            || stream.entries.get(self.index).is_none()
            || stream.process_entry_hidden(self.index, cx)
            || cx
                .try_global::<crate::settings::SettingsOpen>()
                .is_none_or(|settings| settings.0)
            || !cx
                .try_global::<crate::sidebar::OpenedThread>()
                .is_some_and(|route| {
                    route
                        .0
                        .as_ref()
                        .is_some_and(|thread| thread.id == self.thread_id)
                })
        {
            return false;
        }
        let (notices, mcp) = stream.run_notice_projection(cx);
        let current = if matches!(self.notice.kind, RunNoticeKind::McpUnavailable(_)) {
            &mcp
        } else {
            &notices
        };
        current.get(&self.index) == Some(&self.notice)
    }
}

#[derive(Clone)]
struct Snapshot {
    id: ProjectionId,
    source: Source,
    text: String,
    colors: ThemeColors,
    enabled: bool,
    selected: Option<Action>,
}

impl Snapshot {
    fn actions(&self) -> Vec<Action> {
        if !self.enabled || !self.source.notice.can_resume_credentials() {
            return Vec::new();
        }
        let mut actions = vec![Action::Continue];
        if matches!(
            self.source.notice.kind,
            RunNoticeKind::Failure(
                RunFailureKind::CredentialStorageUnavailable
                    | RunFailureKind::CredentialExposureBlocked
                    | RunFailureKind::CredentialProjectionInvalid(_)
            ) | RunNoticeKind::FailureWithCompaction(
                RunFailureKind::CredentialStorageUnavailable
                    | RunFailureKind::CredentialExposureBlocked
                    | RunFailureKind::CredentialProjectionInvalid(_),
                _
            )
        ) {
            actions.push(Action::Settings);
        }
        actions
    }
}

enum Request {
    Activate {
        observed: Box<Snapshot>,
        action: Action,
    },
    Layout {
        id: ProjectionId,
        width: f32,
        height: f32,
    },
}

#[derive(Clone, Copy)]
struct Geometry {
    visible: Bounds<Pixels>,
    viewport: Bounds<Pixels>,
    row: Bounds<Pixels>,
    foreground: Pixels,
}

#[derive(Default)]
struct Mount {
    geometry: Cell<Option<Geometry>>,
}

struct NoticeHost {
    owner: WeakEntity<ConversationStream>,
    snapshot: Snapshot,
    surface: Option<Surface<Snapshot, Request>>,
    focus: FocusHandle,
    selected: Option<Selection<Action>>,
    pointer_entry: Option<Action>,
    lease: Weak<Mount>,
    measured: Option<(f32, f32)>,
    _subscriptions: Vec<Subscription>,
}

impl NoticeHost {
    fn stop(&mut self, cx: &mut Context<Self>) {
        if let Some(surface) = self.surface.take() {
            surface.entity().read(cx).stop();
        }
        self.snapshot.id = ProjectionId::new();
        self.selected = None;
        self.pointer_entry = None;
        self.measured = None;
        self.lease = Weak::new();
    }

    fn activate(
        &mut self,
        observed: &Snapshot,
        action: Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(owner) = self.owner.upgrade() else {
            self.stop(cx);
            return;
        };
        if !owner
            .read(cx)
            .run_notice_epoch
            .matches(&self.snapshot.source.epoch)
        {
            self.stop(cx);
            cx.notify();
            return;
        }
        if !self.snapshot.id.matches(&observed.id)
            || !self.snapshot.source.matches(&observed.source)
            || !self.snapshot.actions().contains(&action)
            || !observed.actions().contains(&action)
            || self
                .surface
                .as_ref()
                .is_none_or(|surface| surface.errors().borrow().is_some())
            || !window.is_window_active()
        {
            return;
        }
        let Some(geometry) = self.lease.upgrade().and_then(|mount| mount.geometry.get()) else {
            return;
        };
        let accepted = owner.update(cx, |stream, cx| {
            let viewport = stream.list.viewport_bounds();
            if !observed.source.current(stream, cx)
                || geometry.visible.is_empty()
                || geometry.viewport != viewport
                || geometry.foreground != stream.foreground_height
                || stream.list.bounds_for_item(observed.source.index) != Some(geometry.row)
                || geometry.visible.intersect(&viewport).is_empty()
            {
                return false;
            }
            match action {
                Action::Continue => cx.emit(CredentialContinuationRequested {
                    thread_id: observed.source.thread_id.clone(),
                    message_id: observed.source.notice.message_id.clone(),
                }),
                Action::Settings => {
                    cx.set_global(crate::settings::SettingsOpen(true));
                    cx.refresh_windows();
                }
            }
            true
        });
        if accepted {
            if let Step::Selected(selected) =
                Selection::advance(None, &observed.id, &[action], false)
            {
                self.selected = Some(selected);
            }
            if !self.focus.contains_focused(window, cx) {
                self.pointer_entry = Some(action);
            }
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    fn mount(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.surface.is_some() {
            return;
        }
        let (surface, mut requests) =
            Surface::new(self.snapshot.clone(), pixels::render, window, cx);
        let mut errors = surface.errors();
        let id = self.snapshot.id.clone();
        cx.spawn(async move |this, cx| {
            while let Some(request) = requests.recv().await {
                if this
                    .update_in(cx, |this, window, cx| match request {
                        Request::Activate { observed, action } => {
                            this.activate(&observed, action, window, cx)
                        }
                        Request::Layout { id, width, height } => {
                            if this.snapshot.id.matches(&id)
                                && this.lease.upgrade().is_some()
                                && width.is_finite()
                                && width > 0.
                                && height.is_finite()
                                && height > 0.
                                && this
                                    .measured
                                    .is_none_or(|(previous_width, previous_height)| {
                                        (previous_width - width).abs() > 0.5
                                            || (previous_height - height).abs() > 0.5
                                    })
                                && let Some(owner) = this.owner.upgrade()
                                && owner
                                    .read(cx)
                                    .run_notice_epoch
                                    .matches(&this.snapshot.source.epoch)
                            {
                                this.measured = Some((width, height));
                                owner.update(cx, |stream, cx| {
                                    stream.invalidate_item(Some(this.snapshot.source.index));
                                    cx.notify();
                                });
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
        cx.spawn(async move |this, cx| {
            while errors.changed().await.is_ok() {
                if errors.borrow_and_update().is_some() {
                    let _ = this.update(cx, |this, cx| {
                        if this.snapshot.id.matches(&id) {
                            this.selected = None;
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

    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        match Key::from_event(event) {
            Key::Next(backwards) => {
                let actions = self.snapshot.actions();
                let previous = self
                    .focus
                    .is_focused(window)
                    .then_some(self.selected.as_ref())
                    .flatten();
                match Selection::advance(previous, &self.snapshot.id, &actions, backwards) {
                    Step::Selected(selected) => {
                        self.selected = Some(selected);
                        window.focus(&self.focus, cx);
                    }
                    Step::Exit => crate::nsh::keyboard::leave(&self.focus, backwards, window, cx),
                }
                cx.stop_propagation();
                cx.notify();
            }
            Key::Activate if self.focus.is_focused(window) => {
                if let Some(action) = self.selected.as_ref().and_then(|selected| {
                    selected.action(&self.snapshot.id, &self.snapshot.actions())
                }) {
                    self.activate(&self.snapshot.clone(), action, window, cx);
                }
                cx.stop_propagation();
            }
            _ => {}
        }
    }
}

#[derive(gpui_kit::IntoElement)]
pub(super) struct NoticeRow {
    pub(super) owner: WeakEntity<ConversationStream>,
    pub(super) notice: RunNotice,
    pub(super) thread_id: String,
    pub(super) reading_preview: bool,
    pub(super) colors: ThemeColors,
}

impl RenderOnce for NoticeRow {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Some(owner) = self.owner.upgrade() else {
            return div().into_any_element();
        };
        let stream = owner.read(cx);
        let notices = if matches!(self.notice.kind, RunNoticeKind::McpUnavailable(_)) {
            &stream.mcp_notices
        } else {
            &stream.run_notices
        };
        let Some(index) = notices
            .iter()
            .find_map(|(index, notice)| (notice == &self.notice).then_some(*index))
        else {
            return div().into_any_element();
        };
        let Some(identity) = stream.entry_identities.get(index).cloned() else {
            return div().into_any_element();
        };
        let source = Source {
            epoch: stream.run_notice_epoch.clone(),
            thread_id: self.thread_id,
            index,
            identity,
            notice: self.notice,
            reading_preview: self.reading_preview,
        };
        let snapshot = Snapshot {
            id: ProjectionId::new(),
            enabled: source.current(stream, cx),
            text: source.notice.text(),
            source,
            colors: self.colors,
            selected: None,
        };
        let key = format!(
            "nsh-run-notice-{}-{}-{}",
            owner.entity_id(),
            snapshot.source.identity.key,
            matches!(
                snapshot.source.notice.kind,
                RunNoticeKind::McpUnavailable(_)
            )
        );
        let state = window.use_keyed_state(key, cx, |window, cx| {
            let focus = cx.focus_handle().tab_stop(!snapshot.actions().is_empty());
            let subscriptions = vec![
                cx.on_release(|this: &mut NoticeHost, cx| {
                    if let Some(surface) = this.surface.take() {
                        surface.entity().read(cx).stop();
                    }
                }),
                cx.on_focus_in(&focus, window, |this: &mut NoticeHost, window, cx| {
                    if this.focus.contains_focused(window, cx) {
                        let actions = this
                            .pointer_entry
                            .take()
                            .filter(|action| this.snapshot.actions().contains(action))
                            .map(|action| vec![action])
                            .unwrap_or_else(|| this.snapshot.actions());
                        if let Step::Selected(selected) = Selection::advance(
                            None,
                            &this.snapshot.id,
                            &actions,
                            window.modifiers().shift,
                        ) {
                            this.selected = Some(selected);
                        }
                        window.focus(&this.focus, cx);
                    }
                    cx.notify();
                }),
                cx.on_focus_out(&focus, window, |_, _, _, cx| cx.notify()),
            ];
            NoticeHost {
                owner: self.owner.clone(),
                snapshot: snapshot.clone(),
                surface: None,
                focus,
                selected: None,
                pointer_entry: None,
                lease: Weak::new(),
                measured: None,
                _subscriptions: subscriptions,
            }
        });
        state.update(cx, |this, cx| {
            let mut snapshot = snapshot;
            if !this.snapshot.source.matches(&snapshot.source) {
                this.stop(cx);
            }
            snapshot.id = this.snapshot.id.clone();
            this.snapshot = snapshot;
        });
        state.into_any_element()
    }
}

impl Render for NoticeHost {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(owner) = self.owner.upgrade() else {
            self.stop(cx);
            return div().into_any_element();
        };
        if !owner
            .read(cx)
            .run_notice_epoch
            .matches(&self.snapshot.source.epoch)
        {
            self.stop(cx);
            return div().into_any_element();
        }
        let this = self;
        let lease = this.lease.upgrade().unwrap_or_else(|| {
            this.stop(cx);
            Rc::new(Mount::default())
        });
        let mut snapshot = this.snapshot.clone();
        snapshot.enabled = snapshot.source.current(owner.read(cx), cx);
        this.lease = Rc::downgrade(&lease);
        this.focus = this.focus.clone().tab_stop(!snapshot.actions().is_empty());
        this.selected =
            Selection::reconcile(this.selected.as_ref(), &snapshot.id, &snapshot.actions());
        snapshot.selected = this
            .focus
            .is_focused(window)
            .then(|| {
                this.selected
                    .as_ref()
                    .and_then(|selected| selected.action(&snapshot.id, &snapshot.actions()))
            })
            .flatten();
        this.snapshot = snapshot.clone();
        this.mount(window, cx);
        let Some(surface) = &mut this.surface else {
            return div().into_any_element();
        };
        surface.sync_environment(cx);
        surface.publish(snapshot.clone());
        let mut host = div()
            .id("nsh-run-notice-native")
            .relative()
            .w_full()
            .min_w_0()
            .h(px(this.measured.map(|(_, height)| height).unwrap_or_else(
                || {
                    snapshot.source.notice.estimated_height(f32::from(
                        owner.read(cx).list.viewport_bounds().size.width,
                    ))
                },
            )))
            .role(Role::Group)
            .aria_label(snapshot.text.clone())
            .track_focus(&this.focus)
            .capture_key_down(cx.listener(|this, event, window, cx| this.key(event, window, cx)))
            .child(surface.entity());
        for action in snapshot.actions() {
            let weak = cx.weak_entity();
            let observed = snapshot.clone();
            host = host.child(
                div()
                    .id(action.label())
                    .absolute()
                    .inset_0()
                    .role(Role::Button)
                    .aria_label(action.label())
                    .when(snapshot.selected == Some(action), |node| {
                        node.aria_active_descendant()
                    })
                    .on_a11y_action(AccessibleAction::Click, move |_, window, cx| {
                        let _ = weak
                            .update(cx, |this, cx| this.activate(&observed, action, window, cx));
                    }),
            );
        }
        Mounted {
            child: host.into_any_element(),
            lease,
            owner: this.owner.clone(),
            index: this.snapshot.source.index,
        }
        .into_any_element()
    }
}

struct Mounted {
    child: AnyElement,
    lease: Rc<Mount>,
    owner: WeakEntity<ConversationStream>,
    index: usize,
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
        Some("run-notice-mounted".into())
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
    ) {
        self.lease.geometry.set(None);
        if let Some(id) = id {
            window.with_element_state::<Rc<Mount>, _>(id, |_, _| ((), self.lease.clone()));
            if let Some(owner) = self.owner.upgrade() {
                let stream = owner.read(cx);
                let viewport = stream.list.viewport_bounds();
                if let Some(row) = stream.list.bounds_for_item(self.index) {
                    let mut reading_viewport = viewport;
                    reading_viewport.size.height =
                        (reading_viewport.size.height - stream.foreground_height).max(px(0.));
                    let visible = bounds
                        .intersect(&window.content_mask().bounds)
                        .intersect(&reading_viewport);
                    if !visible.is_empty() {
                        self.lease.geometry.set(Some(Geometry {
                            visible,
                            viewport,
                            row,
                            foreground: stream.foreground_height,
                        }));
                    }
                }
            }
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

#[cfg(test)]
mod tests;
