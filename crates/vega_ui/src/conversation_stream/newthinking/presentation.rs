use super::*;
use crate::nsh::{ProjectionId, Surface};
use gpui_kit::{Bounds, Element, ElementId, GlobalElementId, InspectorElementId, LayoutId};
use std::cell::Cell;
use std::rc::{Rc, Weak};

#[path = "pixels.rs"]
mod pixels;
#[cfg(test)]
#[path = "tests.rs"]
mod tests;

pub(super) fn transition_visibility(
    current: &mut bool,
    visible: bool,
    source: &MessageCopy,
) -> bool {
    if *current == visible {
        return false;
    }
    *current = visible;
    if !visible {
        source.invalidate_selection();
    }
    true
}

pub(super) struct OwnerBinding {
    pub(super) epoch: ProjectionId,
    session: ProjectionId,
}

impl OwnerBinding {
    pub(super) fn new(epoch: ProjectionId) -> Self {
        Self {
            epoch,
            session: ProjectionId::new(),
        }
    }

    pub(super) fn rebind(&mut self, epoch: &ProjectionId, source: &MessageCopy) -> bool {
        if self.epoch.matches(epoch) {
            return false;
        }
        source.invalidate_selection();
        self.epoch = epoch.clone();
        self.session = ProjectionId::new();
        true
    }

    fn current(&self, epoch: &ProjectionId) -> bool {
        self.epoch.matches(epoch)
    }

    fn accepts(&self, epoch: &ProjectionId, session: &ProjectionId) -> bool {
        self.current(epoch) && self.session.matches(session)
    }
}

pub(super) struct Owner {
    pub(super) stream: gpui_kit::WeakEntity<ConversationStream>,
    pub(super) thread: String,
    pub(super) binding: OwnerBinding,
    pub(super) group: gpui_kit::EntityId,
}

impl Owner {
    pub(super) fn current(&self, cx: &App) -> bool {
        let Some(owner) = self.stream.upgrade() else {
            return false;
        };
        let stream = owner.read(cx);
        stream.thread.id == self.thread
            && self.binding.current(&stream.run_notice_epoch)
            && !stream.permissions_revoked
            && !stream.permission_change_pending()
            && !stream.reading_preview
            && stream.entries.iter().enumerate().any(|(index, entry)| {
                matches!(entry, StreamEntry::RunActivity { group } | StreamEntry::RunActivitySegment { group, .. } if group.entity_id() == self.group)
                    && stream.list.bounds_for_item(index).is_some_and(|bounds| {
                        !bounds.intersect(&stream.list.viewport_bounds()).is_empty()
                    })
            })
            && cx.try_global::<crate::settings::SettingsOpen>().is_some_and(|state| !state.0)
            && cx.try_global::<crate::sidebar::OpenedThread>().is_some_and(|route| {
                route.0.as_ref().is_some_and(|thread| thread.id == self.thread)
            })
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Authority {
    source: u64,
    revision: u64,
    start: usize,
    expanded: bool,
    available: bool,
    actions: bool,
    selection: u64,
    visible: bool,
    title: String,
}

#[derive(Clone)]
struct Snapshot {
    session: ProjectionId,
    owner_session: Option<ProjectionId>,
    authority: Authority,
    colors: ThemeColors,
    focused: bool,
    epoch: Option<Instant>,
    omission: String,
    label: String,
}

impl Snapshot {
    fn accepts(&self, session: &ProjectionId, current: &Authority, mounted: bool) -> bool {
        mounted
            && self.session.matches(session)
            && self.authority == *current
            && current.available
            && current.visible
    }
}

enum Request {
    Activate(Snapshot),
}

pub(super) struct Presentation {
    header: Surface<Snapshot, Request>,
    omission: Surface<Snapshot, Request>,
    session: ProjectionId,
    shown: Snapshot,
    mount: Weak<Cell<bool>>,
    _subscriptions: Vec<gpui_kit::Subscription>,
}

impl Presentation {
    pub(super) fn unmount(&self) {
        if let Some(mount) = self.mount.upgrade() {
            mount.set(false);
        }
        let mut hidden = self.shown.clone();
        hidden.epoch = None;
        hidden.authority.visible = false;
        self.header.publish(hidden);
    }

    pub(super) fn revoke(&mut self) {
        self.session = ProjectionId::new();
        self.unmount();
        self.shown.authority.visible = false;
        self.shown.epoch = None;
        self.header.publish(self.shown.clone());
    }
}

impl ThinkingBlock {
    fn thinking_authority(&self) -> Authority {
        Authority {
            source: self.source_identity,
            revision: self.source_revision,
            start: self.visible_window().start,
            expanded: self.expanded,
            available: self.has_source_text(),
            actions: self.message_actions_enabled,
            selection: self.source_copy.selection_generation(),
            visible: self.ancestor_visible,
            title: self.title().to_owned(),
        }
    }

    fn thinking_snapshot(&self, session: ProjectionId, window: &Window, cx: &App) -> Snapshot {
        let authority = self.thinking_authority();
        Snapshot {
            session,
            owner_session: self
                .owner
                .as_ref()
                .map(|owner| owner.binding.session.clone()),
            colors: theme(cx).colors,
            focused: self.disclosure_focus.is_focused(window),
            epoch: self
                .ancestor_visible
                .then_some(
                    self.shimmer_epoch
                        .filter(|_| window.is_visible() && window.is_window_active()),
                )
                .flatten(),
            omission: vega_i18n::t("ui.earlier_thinking_omitted").to_owned(),
            label: vega_i18n::t_args("ui.thinking_disclosure", &[("title", &authority.title)]),
            authority,
        }
    }

    fn accepts_thinking(&self, observed: &Snapshot, window: &Window, cx: &App) -> bool {
        self.presentation.as_ref().is_some_and(|page| {
            observed.accepts(
                &page.session,
                &self.thinking_authority(),
                page.mount.upgrade().is_some_and(|mount| mount.get()),
            ) && page.shown.authority == observed.authority
                && page.header.errors().borrow().is_none()
        }) && observed.authority.available
            && self.ancestor_visible
            && self.owner.as_ref().is_some_and(|owner| {
                owner.current(cx)
                    && observed
                        .owner_session
                        .as_ref()
                        .is_some_and(|session| owner.binding.accepts(&owner.binding.epoch, session))
            })
            && !self.source_copy.has_selected_text()
            && window.is_window_active()
    }

    fn activate_thinking(
        &mut self,
        observed: &Snapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.accepts_thinking(observed, window, cx) {
            return;
        }
        window.focus(&self.disclosure_focus, cx);
        self.set_expanded(!observed.authority.expanded, cx);
    }

    fn thinking_key(
        &mut self,
        event: &gpui_kit::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.disclosure_focus.is_focused(window) {
            return;
        }
        if matches!(
            crate::nsh::keyboard::Key::from_event(event),
            crate::nsh::keyboard::Key::Activate
        ) {
            if let Some(observed) = self.presentation.as_ref().map(|page| page.shown.clone()) {
                self.activate_thinking(&observed, window, cx);
            }
            cx.stop_propagation();
        }
    }

    pub(super) fn render_thinking_header(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.presentation.is_none() {
            let session = ProjectionId::new();
            let snapshot = self.thinking_snapshot(session.clone(), window, cx);
            let (header, mut requests) = Surface::new(snapshot.clone(), pixels::header, window, cx);
            let (omission, _) = Surface::new(snapshot.clone(), pixels::omission, window, cx);
            cx.spawn_in(window, async move |this, cx| {
                while let Some(Request::Activate(observed)) = requests.recv().await {
                    if this
                        .update_in(cx, |this, window, cx| {
                            this.activate_thinking(&observed, window, cx)
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();
            let focus = cx.on_focus(&self.disclosure_focus, window, |_, _, cx| cx.notify());
            let blur = cx.on_blur(&self.disclosure_focus, window, |_, _, cx| cx.notify());
            let release = cx.on_release(|this: &mut Self, cx| {
                if let Some(page) = this.presentation.take() {
                    page.header.entity().read(cx).stop();
                    page.omission.entity().read(cx).stop();
                }
            });
            let activation = cx.observe_window_activation(window, |_, _, cx| cx.notify());
            let visibility = cx.observe_window_visibility(window, |_, _, _, cx| cx.notify());
            self.presentation = Some(Presentation {
                header,
                omission,
                session,
                shown: snapshot,
                mount: Weak::new(),
                _subscriptions: vec![focus, blur, release, activation, visibility],
            });
        }
        let Some(page) = self.presentation.as_mut() else {
            return div().into_any_element();
        };
        if page.mount.upgrade().is_none_or(|mount| !mount.get()) {
            page.session = ProjectionId::new();
        }
        let session = page.session.clone();
        let snapshot = self.thinking_snapshot(session, window, cx);
        let enabled = snapshot.authority.available
            && self.ancestor_visible
            && self.owner.as_ref().is_some_and(|owner| owner.current(cx));
        self.disclosure_focus = self.disclosure_focus.clone().tab_stop(enabled);
        let Some(page) = self.presentation.as_mut() else {
            return div().into_any_element();
        };
        page.header.sync_environment(cx);
        page.omission.sync_environment(cx);
        page.header.publish(snapshot.clone());
        page.omission.publish(snapshot.clone());
        page.shown = snapshot.clone();
        let mount = Rc::new(Cell::new(false));
        page.mount = Rc::downgrade(&mount);
        let header = page.header.entity();
        let _ = page.header.focus_handle(cx).tab_stop(false);
        let _ = page.omission.focus_handle(cx).tab_stop(false);
        let observed = snapshot.clone();
        let weak = cx.weak_entity();
        let proxy = div()
            .id("thinking-native-disclosure")
            .absolute()
            .inset_0()
            .role(gpui_kit::Role::Button)
            .aria_label(snapshot.label)
            .when(snapshot.focused, |node| node.aria_active_descendant())
            .aria_expanded(snapshot.authority.expanded)
            .on_a11y_action(gpui_kit::AccessibleAction::Click, move |_, window, cx| {
                let _ = weak.update(cx, |this, cx| this.activate_thinking(&observed, window, cx));
            });
        HeaderMount {
            child: div()
                .relative()
                .w_full()
                .h(px(Layout::CONVERSATION_TOOL_HEIGHT))
                .track_focus(&self.disclosure_focus)
                .capture_key_down(cx.listener(Self::thinking_key))
                .child(header)
                .when(enabled, |row| row.child(proxy))
                .into_any_element(),
            mount,
        }
        .into_any_element()
    }

    pub(super) fn render_thinking_omission(&self) -> AnyElement {
        self.presentation.as_ref().map_or_else(
            || div().into_any_element(),
            |page| {
                div()
                    .w_full()
                    .h(px(
                        Typography::METADATA * Typography::CONVERSATION_LINE_HEIGHT
                    ))
                    .child(page.omission.entity())
                    .into_any_element()
            },
        )
    }
}

struct HeaderMount {
    child: AnyElement,
    mount: Rc<Cell<bool>>,
}

impl IntoElement for HeaderMount {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for HeaderMount {
    type RequestLayoutState = ();
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        Some("thinking-native-mount".into())
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
        self.mount.set(false);
        if let Some(id) = id {
            window.with_element_state::<Rc<Cell<bool>>, _>(id, |_, _| ((), self.mount.clone()));
            self.mount
                .set(!bounds.intersect(&window.content_mask().bounds).is_empty());
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
