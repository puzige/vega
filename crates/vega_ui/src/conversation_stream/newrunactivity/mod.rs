use super::*;
use crate::nsh::keyboard::Key;
use crate::nsh::{ProjectionId, Surface};
use gpui_kit::{
    AccessibleAction, Bounds, ContentMask, Element, ElementId, EntityId, GlobalElementId,
    InspectorElementId, KeyDownEvent, LayoutId, Role, Size, Subscription, WeakEntity,
};
use std::cell::RefCell;
use std::rc::{Rc, Weak};

#[cfg(feature = "nsh-reading-integration")]
mod header_integration;
#[cfg(feature = "nsh-reading-integration")]
pub use header_integration::repeated_production_header_render_retains_unowned_mount_and_projection;
mod pixels;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Target {
    Run,
    Stage(usize),
}

#[derive(Clone)]
struct Owner {
    stream: WeakEntity<ConversationStream>,
    thread: String,
    epoch: ProjectionId,
    session: ProjectionId,
}

impl Owner {
    fn current(&self, group: EntityId, target: Target, cx: &App) -> bool {
        let Some(stream) = self.stream.upgrade() else {
            return false;
        };
        let stream = stream.read(cx);
        stream.thread.id == self.thread
            && self.epoch.matches(&stream.run_notice_epoch)
            && !stream.permissions_revoked
            && !stream.permission_change_pending()
            && !stream.reading_preview
            && cx
                .try_global::<crate::settings::SettingsOpen>()
                .is_some_and(|state| !state.0)
            && cx
                .try_global::<crate::sidebar::OpenedThread>()
                .is_some_and(|route| {
                    route
                        .0
                        .as_ref()
                        .is_some_and(|thread| thread.id == self.thread)
                })
            && stream.entries.iter().enumerate().any(|(index, entry)| {
                let matches = match (target, entry) {
                    (Target::Run, StreamEntry::RunActivity { group: candidate }) => {
                        candidate.entity_id() == group
                    }
                    (
                        Target::Stage(segment),
                        StreamEntry::RunActivitySegment {
                            group: candidate,
                            segment: current,
                        },
                    ) => candidate.entity_id() == group && segment == *current,
                    _ => false,
                };
                matches
                    && stream.list.bounds_for_item(index).is_some_and(|bounds| {
                        !bounds.intersect(&stream.list.viewport_bounds()).is_empty()
                    })
            })
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Authority {
    target: Target,
    segment: Option<EntityId>,
    children: Vec<Child>,
    expanded: bool,
    can_expand: bool,
    title: String,
    icon: Option<crate::icons::Icon>,
    live: bool,
    status: RunActivityStatus,
    released: bool,
    details: bool,
}

#[derive(Clone, PartialEq, Eq)]
enum Child {
    Thinking(EntityId),
    Tool(EntityId),
    ToolGroup(EntityId, Vec<EntityId>),
    Compaction(String, u64),
}

#[derive(Clone)]
struct Snapshot {
    id: ProjectionId,
    projection: ProjectionId,
    layout: Option<LayoutStamp>,
    appearance: vega_theme::Appearance,
    language: &'static str,
    revision: ProjectionId,
    owner: Option<ProjectionId>,
    authority: Authority,
    label: String,
    colors: ThemeColors,
    focused: bool,
    hovered: bool,
    pressed: bool,
    title_width: f32,
    divider: bool,
    attached: bool,
}

impl Snapshot {
    fn same_identity(&self, other: &Self) -> bool {
        self.id.matches(&other.id)
            && self.revision.matches(&other.revision)
            && match (&self.owner, &other.owner) {
                (Some(a), Some(b)) => a.matches(b),
                (None, None) => true,
                _ => false,
            }
            && self.authority == other.authority
    }

    fn accepts(&self, other: &Self) -> bool {
        self.owner.is_some() && self.same_identity(other)
    }
}

#[derive(Clone)]
struct LayoutStamp {
    frame: ProjectionId,
    size: [f32; 2],
}

impl LayoutStamp {
    fn matches(&self, other: &Self) -> bool {
        self.frame.matches(&other.frame) && self.size == other.size
    }

    fn accepts_root(&self, root: [f32; 4]) -> bool {
        root.iter().all(|value| value.is_finite())
            && self
                .size
                .iter()
                .all(|value| value.is_finite() && *value > 0.)
            && root == [0., 0., self.size[0], self.size[1]]
    }
}

struct LayoutAck {
    snapshot: Snapshot,
    root: [f32; 4],
}

impl Snapshot {
    fn same_presentation(&self, other: &Self) -> bool {
        self.same_identity(other)
            && self.focused == other.focused
            && self.label == other.label
            && self.title_width == other.title_width
            && self.attached == other.attached
            && self.divider == other.divider
            && self.appearance == other.appearance
            && self.language == other.language
            && self.colors.bg_base == other.colors.bg_base
            && self.colors.bg_active == other.colors.bg_active
            && self.colors.bg_hover == other.colors.bg_hover
            && self.colors.conversation_stage == other.colors.conversation_stage
            && self.colors.conversation_clock == other.colors.conversation_clock
            && self.colors.conversation_tool == other.colors.conversation_tool
            && self.colors.conversation_divider == other.colors.conversation_divider
            && match (&self.layout, &other.layout) {
                (Some(a), Some(b)) => a.matches(b),
                (None, None) => true,
                _ => false,
            }
    }

    fn same_projection(&self, other: &Self) -> bool {
        self.projection.matches(&other.projection) && self.same_presentation(other)
    }
}

struct Publication {
    published: Snapshot,
    displayed: Option<Snapshot>,
}

impl Publication {
    fn new(published: Snapshot) -> Self {
        Self {
            published,
            displayed: None,
        }
    }

    fn publish(&mut self, mut next: Snapshot) -> bool {
        let changed = !self.published.same_presentation(&next);
        if changed {
            next.projection = ProjectionId::new();
            self.displayed = None;
        } else {
            next.projection = self.published.projection.clone();
        }
        let repaint = changed
            || next.hovered != self.published.hovered
            || next.pressed != self.published.pressed;
        self.published = next;
        repaint
    }

    fn acknowledge(&mut self, ack: &LayoutAck) -> bool {
        if !ack.snapshot.accepts(&self.published)
            || !self.published.same_projection(&ack.snapshot)
            || !ack
                .snapshot
                .layout
                .as_ref()
                .is_some_and(|layout| layout.accepts_root(ack.root))
            || self.accepts(&ack.snapshot)
        {
            return false;
        }
        self.displayed = Some(ack.snapshot.clone());
        true
    }

    fn accepts(&self, observed: &Snapshot) -> bool {
        observed.layout.is_some()
            && observed.accepts(&self.published)
            && observed.same_projection(&self.published)
            && self
                .displayed
                .as_ref()
                .is_some_and(|shown| shown.same_projection(observed))
    }
}

#[derive(Clone)]
struct ClockLease {
    id: ProjectionId,
    snapshot: Snapshot,
}

impl ClockLease {
    fn retains(&self, observed: &Snapshot) -> bool {
        self.snapshot.same_projection(observed)
    }
}

#[derive(Default)]
struct Mount {
    bounds: Option<Bounds<Pixels>>,
    clipped: Option<Bounds<Pixels>>,
    frame: Option<ProjectionId>,
    viewport: Option<Size<Pixels>>,
    scale: f32,
}

impl Mount {
    fn update(
        &mut self,
        bounds: Bounds<Pixels>,
        clipped: Bounds<Pixels>,
        viewport: Size<Pixels>,
        scale: f32,
    ) {
        let clipped = (!clipped.is_empty()).then_some(clipped);
        if self.bounds != Some(bounds)
            || self.clipped != clipped
            || self.viewport != Some(viewport)
            || self.scale != scale
            || self.frame.is_none()
        {
            self.frame = clipped.map(|_| ProjectionId::new());
        }
        self.bounds = Some(bounds);
        self.clipped = clipped;
        self.viewport = Some(viewport);
        self.scale = scale;
    }

    fn layout(&self) -> Option<LayoutStamp> {
        let bounds = self.bounds?;
        Some(LayoutStamp {
            frame: self.frame.clone()?,
            size: [f32::from(bounds.size.width), f32::from(bounds.size.height)],
        })
    }
}

impl Mount {
    fn visible(&self, window: &Window) -> bool {
        window.is_visible()
            && window.is_window_active()
            && self.viewport == Some(window.viewport_size())
            && self.scale == window.scale_factor()
            && self.frame.is_some()
            && self
                .clipped
                .is_some_and(|bounds| !bounds.intersect(&window.content_mask().bounds).is_empty())
    }
}

pub(super) struct Headers {
    owner: Option<Owner>,
    revision: ProjectionId,
    suspended: bool,
    clock: Option<ClockLease>,
    run: Option<Presentation>,
    stages: HashMap<EntityId, Presentation>,
}

impl Default for Headers {
    fn default() -> Self {
        Self {
            owner: None,
            revision: ProjectionId::new(),
            suspended: true,
            clock: None,
            run: None,
            stages: HashMap::new(),
        }
    }
}

struct Presentation {
    id: ProjectionId,
    surface: Surface<Snapshot, LayoutAck>,
    divider: Option<Surface<Snapshot, LayoutAck>>,
    publication: Publication,
    mount: Weak<RefCell<Mount>>,
    pressed: Option<(Snapshot, Bounds<Pixels>)>,
    _subscriptions: Vec<Subscription>,
}

impl Presentation {
    fn publish(&mut self, mut snapshot: Snapshot) -> bool {
        if !self.publication.published.same_presentation(&snapshot) {
            self.pressed = None;
            snapshot.pressed = false;
        }
        let changed = self.publication.publish(snapshot);
        if changed {
            self.surface.publish(self.publication.published.clone());
            if let Some(divider) = &self.divider {
                let mut line = self.publication.published.clone();
                line.divider = true;
                divider.publish(line);
            }
        }
        changed
    }

    fn mount(&mut self) -> Rc<RefCell<Mount>> {
        if let Some(mount) = self.mount.upgrade() {
            return mount;
        }
        self.revoke();
        let mount = Rc::new(RefCell::new(Mount::default()));
        self.mount = Rc::downgrade(&mount);
        mount
    }

    fn revoke(&mut self) {
        self.id = ProjectionId::new();
        self.publication.displayed = None;
        self.pressed = None;
        if let Some(mount) = self.mount.upgrade() {
            *mount.borrow_mut() = Mount::default();
        }
    }
}

impl Headers {
    fn retains_clock(&self, observed: &Snapshot, task_pending: bool) -> bool {
        task_pending
            && self
                .clock
                .as_ref()
                .is_some_and(|lease| lease.retains(observed))
    }

    fn finish_clock(&mut self, lease: &ClockLease) -> bool {
        if !self
            .clock
            .as_ref()
            .is_some_and(|current| current.id.matches(&lease.id))
        {
            return false;
        }
        self.clock = None;
        true
    }

    fn invalidate(&mut self) {
        self.revision = ProjectionId::new();
        self.clock = None;
        if let Some(page) = &mut self.run {
            page.publication.displayed = None;
            page.publication.published.pressed = false;
            page.pressed = None;
        }
        for page in self.stages.values_mut() {
            page.publication.displayed = None;
            page.publication.published.pressed = false;
            page.pressed = None;
        }
    }

    fn suspend(&mut self) {
        self.suspended = true;
        self.invalidate();
        if let Some(page) = &mut self.run {
            page.revoke();
        }
        for page in self.stages.values_mut() {
            page.revoke();
        }
    }

    fn page(&self, target: Target, segment: Option<EntityId>) -> Option<&Presentation> {
        match target {
            Target::Run => self.run.as_ref(),
            Target::Stage(_) => self.stages.get(&segment?),
        }
    }

    fn page_mut(&mut self, target: Target, segment: Option<EntityId>) -> Option<&mut Presentation> {
        match target {
            Target::Run => self.run.as_mut(),
            Target::Stage(_) => self.stages.get_mut(&segment?),
        }
    }
}

impl ConversationStream {
    pub(crate) fn rebind_run_activity_presentation(
        &self,
        group: &Entity<RunActivityGroup>,
        cx: &mut Context<Self>,
    ) {
        let group_id = group.entity_id();
        if self.permissions_revoked
            || self.permission_change_pending()
            || self.reading_preview
            || !cx.try_global::<crate::settings::SettingsOpen>().is_some_and(|state| !state.0)
            || !cx.try_global::<crate::sidebar::OpenedThread>().is_some_and(|route| {
                route.0.as_ref().is_some_and(|thread| thread.id == self.thread.id)
            })
            || !self.entries.iter().any(|entry| {
                matches!(entry, StreamEntry::RunActivity { group } | StreamEntry::RunActivitySegment { group, .. } if group.entity_id() == group_id)
            })
        {
            return;
        }
        let stream = cx.weak_entity();
        let thread = self.thread.id.clone();
        let epoch = self.run_notice_epoch.clone();
        group.update(cx, |group, _| {
            let headers = &mut group.headers.borrow_mut();
            if let Some(owner) = &headers.owner {
                if owner.thread != thread || owner.stream != stream {
                    return;
                }
                if owner.epoch.matches(&epoch) && !headers.suspended {
                    return;
                }
            }
            headers.suspend();
            headers.suspended = false;
            headers.owner = Some(Owner {
                stream,
                thread,
                epoch,
                session: ProjectionId::new(),
            });
        });
    }
}

impl RunActivityGroup {
    pub(super) fn invalidate_run_headers(&self) {
        self.headers.borrow_mut().invalidate();
    }

    pub(super) fn suspend_run_headers(&self) {
        self.headers.borrow_mut().suspend();
        let _ = self.header_focus.clone().tab_stop(false);
        for segment in &self.segments {
            let _ = segment.header_focus.clone().tab_stop(false);
        }
    }

    fn header_authority(&self, target: Target, attached: bool, cx: &App) -> Option<Authority> {
        let (segment, children, expanded, can_expand, title, icon, live) = match target {
            Target::Run => {
                if self.header_hidden() {
                    return None;
                }
                (
                    None,
                    self.children(),
                    self.expanded,
                    self.process_in_details()
                        && (self.has_children() || !self.process_copies.is_empty() || attached),
                    self.label(),
                    None,
                    false,
                )
            }
            Target::Stage(index) => {
                let segment = self.segments.get(index)?;
                if self.process_in_details() {
                    return None;
                }
                let headline = self.stage_headline(index, cx);
                let available = self.segment_can_expand(index, cx);
                (
                    Some(segment.title_view.entity_id()),
                    segment.children.clone(),
                    segment.expanded && available,
                    available,
                    headline.title,
                    headline.icon,
                    headline.live,
                )
            }
        };
        let children = children
            .into_iter()
            .map(|child| match child {
                RunActivityChild::Thinking(card) => Child::Thinking(card.entity_id()),
                RunActivityChild::Tool(card) => Child::Tool(card.entity_id()),
                RunActivityChild::ToolGroup(group) => Child::ToolGroup(
                    group.entity_id(),
                    group
                        .read(cx)
                        .children()
                        .iter()
                        .map(|card| card.entity_id())
                        .collect(),
                ),
                RunActivityChild::Compaction { model, generation } => {
                    Child::Compaction(model, generation)
                }
            })
            .collect();
        Some(Authority {
            target,
            segment,
            children,
            expanded,
            can_expand,
            title,
            icon,
            live,
            status: self.status,
            released: self.presentation_released,
            details: self.process_in_details(),
        })
    }

    fn accepts_run_header(&self, observed: &Snapshot, window: &Window, cx: &Context<Self>) -> bool {
        self.accepts_header(observed, false, window, cx)
            && self
                .headers
                .borrow()
                .page(observed.authority.target, observed.authority.segment)
                .is_some_and(|page| page.publication.accepts(observed))
    }

    fn accepts_header(
        &self,
        observed: &Snapshot,
        clock: bool,
        window: &Window,
        cx: &Context<Self>,
    ) -> bool {
        let headers = self.headers.borrow();
        let Some(page) = headers.page(observed.authority.target, observed.authority.segment) else {
            return false;
        };
        let Some(owner) = &headers.owner else {
            return false;
        };
        let Some(mut authority) = self.header_authority(
            observed.authority.target,
            page.publication.published.attached,
            cx,
        ) else {
            return false;
        };
        if clock && observed.authority.target == Target::Run {
            authority.title = observed.authority.title.clone();
        }
        let mut current = page.publication.published.clone();
        current.id = page.id.clone();
        current.revision = headers.revision.clone();
        current.owner = Some(owner.session.clone());
        current.authority = authority;
        let focus = match observed.authority.target {
            Target::Run => &self.header_focus,
            Target::Stage(index) => match self.segments.get(index) {
                Some(segment) => &segment.header_focus,
                None => return false,
            },
        };
        !headers.suspended
            && observed.same_projection(&page.publication.published)
            && observed.accepts(&current)
            && observed.focused == (focus.is_focused(window) && window.last_input_was_keyboard())
            && observed.appearance == theme(cx).appearance
            && observed.language == vega_i18n::language()
            && page.mount.upgrade().is_some_and(|mount| {
                let mount = mount.borrow();
                mount.visible(window)
                    && mount
                        .layout()
                        .zip(observed.layout.as_ref())
                        .is_some_and(|(current, observed)| current.matches(observed))
            })
            && page.surface.errors().borrow().is_none()
            && page
                .divider
                .as_ref()
                .is_none_or(|surface| surface.errors().borrow().is_none())
            && owner.current(cx.entity_id(), observed.authority.target, cx)
    }

    fn header_contains(&self, observed: &Snapshot, point: gpui_kit::Point<Pixels>) -> bool {
        self.headers
            .borrow()
            .page(observed.authority.target, observed.authority.segment)
            .and_then(|page| page.mount.upgrade())
            .and_then(|mount| mount.borrow().clipped)
            .is_some_and(|bounds| bounds.contains(&point))
    }

    fn activate_run_header(
        &mut self,
        observed: &Snapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !observed.authority.can_expand || !self.accepts_run_header(observed, window, cx) {
            return;
        }
        match observed.authority.target {
            Target::Run => {
                self.header_focus.focus(window, cx);
                self.toggle_expanded(cx);
            }
            Target::Stage(index) => {
                self.segments[index].header_focus.focus(window, cx);
                self.toggle_segment(index, cx);
            }
        }
    }

    fn acknowledge_run_header(&mut self, ack: LayoutAck, window: &Window, cx: &mut Context<Self>) {
        if !self.accepts_header(&ack.snapshot, true, window, cx) {
            return;
        }
        let changed = self
            .headers
            .borrow_mut()
            .page_mut(
                ack.snapshot.authority.target,
                ack.snapshot.authority.segment,
            )
            .is_some_and(|page| page.publication.acknowledge(&ack));
        if changed {
            cx.notify();
        }
    }

    fn schedule_clock(&mut self, observed: &Snapshot, window: &mut Window, cx: &mut Context<Self>) {
        if self.terminal || self.presentation_released || self.started_at.is_none() {
            return;
        }
        if self
            .headers
            .borrow()
            .retains_clock(observed, self.clock_task.is_some())
        {
            return;
        }
        self.clock_task = None;
        let lease = ClockLease {
            id: ProjectionId::new(),
            snapshot: observed.clone(),
        };
        self.headers.borrow_mut().clock = Some(lease.clone());
        self.clock_task = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            let _ = this.update_in(cx, |group, window, cx| {
                if !group.headers.borrow_mut().finish_clock(&lease) {
                    return;
                }
                group.clock_task = None;
                if !group.terminal
                    && !group.presentation_released
                    && group.accepts_header(&lease.snapshot, true, window, cx)
                    && group
                        .headers
                        .borrow()
                        .run
                        .as_ref()
                        .is_some_and(|page| page.publication.accepts(&lease.snapshot))
                {
                    cx.emit(RunActivityDisplayChanged {
                        segment: None,
                        geometry: false,
                    });
                }
            });
        }));
    }

    fn header_key(
        &mut self,
        observed: &Snapshot,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focus = match observed.authority.target {
            Target::Run => self.header_focus.clone(),
            Target::Stage(index) => match self.segments.get(index) {
                Some(segment) => segment.header_focus.clone(),
                None => return,
            },
        };
        if !focus.is_focused(window)
            || !observed.authority.can_expand
            || !self.accepts_run_header(observed, window, cx)
        {
            return;
        }
        match Key::from_event(event) {
            Key::Next(backwards) => {
                crate::nsh::keyboard::leave(&focus, backwards, window, cx);
                cx.stop_propagation();
            }
            Key::Activate => {
                self.activate_run_header(observed, window, cx);
                cx.stop_propagation();
            }
            Key::Ignore => {}
        }
    }
}

pub(super) fn render_header(
    group: Entity<RunActivityGroup>,
    target: Target,
    attached: bool,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    group.update(cx, |this, cx| {
        let Some(authority) = this.header_authority(target, attached, cx) else {
            return div().into_any_element();
        };
        let focus = match target {
            Target::Run => this.header_focus.clone(),
            Target::Stage(index) => this.segments[index].header_focus.clone(),
        };
        let enabled = !this.headers.borrow().suspended && this.headers.borrow().owner.is_some();
        let healthy = this
            .headers
            .borrow()
            .page(target, authority.segment)
            .is_none_or(|page| page.surface.errors().borrow().is_none());
        let enabled = enabled && healthy;
        let focus = focus.tab_stop(enabled && authority.can_expand);
        let colors = theme(cx).colors;
        let title_width = if matches!(target, Target::Stage(_)) {
            let text = authority.title.clone();
            let font = window.text_style().font();
            f32::from(
                window
                    .text_system()
                    .layout_line(
                        &text,
                        px(Typography::CONVERSATION),
                        &[TextRun {
                            len: text.len(),
                            font,
                            color: colors.conversation_stage.into(),
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        }],
                        None,
                    )
                    .width,
            ) + 1.
        } else {
            0.
        };
        let label = if authority.can_expand {
            vega_i18n::t_args(
                if target == Target::Run {
                    "ui.run_disclosure"
                } else {
                    "ui.stage_disclosure"
                },
                &[
                    (
                        if target == Target::Run {
                            "label"
                        } else {
                            "title"
                        },
                        &authority.title,
                    ),
                    (
                        "action",
                        vega_i18n::t(if authority.expanded {
                            "ui.collapse"
                        } else {
                            "ui.expand"
                        }),
                    ),
                ],
            )
        } else {
            authority.title.clone()
        };
        let snapshot = Snapshot {
            id: ProjectionId::new(),
            projection: ProjectionId::new(),
            layout: None,
            appearance: theme(cx).appearance,
            language: vega_i18n::language(),
            revision: this.headers.borrow().revision.clone(),
            owner: this
                .headers
                .borrow()
                .owner
                .as_ref()
                .map(|owner| owner.session.clone()),
            authority,
            label,
            colors,
            focused: focus.is_focused(window) && window.last_input_was_keyboard(),
            hovered: false,
            pressed: false,
            title_width,
            divider: false,
            attached,
        };
        if this
            .headers
            .borrow()
            .page(target, snapshot.authority.segment)
            .is_none()
        {
            let (surface, mut requests) =
                Surface::new_transparent(snapshot.clone(), pixels::render, window, cx);
            cx.spawn_in(window, async move |this, cx| {
                while let Some(ack) = requests.recv().await {
                    if this
                        .update_in(cx, |this, window, cx| {
                            this.acknowledge_run_header(ack, window, cx)
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();
            let _ = surface.focus_handle(cx).tab_stop(false);
            let divider = if target == Target::Run {
                let mut line = snapshot.clone();
                line.divider = true;
                let (surface, _) = Surface::new_transparent(line, pixels::render, window, cx);
                let _ = surface.focus_handle(cx).tab_stop(false);
                Some(surface)
            } else {
                None
            };
            let subscriptions = vec![
                cx.on_release(|this, cx| {
                    let headers = this.headers.borrow();
                    for page in headers.run.iter().chain(headers.stages.values()) {
                        page.surface.entity().read(cx).stop();
                        if let Some(divider) = &page.divider {
                            divider.entity().read(cx).stop();
                        }
                    }
                }),
                cx.on_focus_in(&focus, window, |this, _, cx| {
                    this.invalidate_run_headers();
                    cx.notify();
                }),
                cx.on_focus_out(&focus, window, |this, _, _, cx| {
                    this.invalidate_run_headers();
                    cx.notify();
                }),
                cx.observe_window_activation(window, |this, _, cx| {
                    this.invalidate_run_headers();
                    cx.notify();
                }),
                cx.observe_window_visibility(window, |this, _, _, cx| {
                    this.invalidate_run_headers();
                    cx.notify();
                }),
            ];
            let page = Presentation {
                id: snapshot.id.clone(),
                surface,
                divider,
                publication: Publication::new(snapshot.clone()),
                mount: Weak::new(),
                pressed: None,
                _subscriptions: subscriptions,
            };
            let mut headers = this.headers.borrow_mut();
            match target {
                Target::Run => headers.run = Some(page),
                Target::Stage(_) => {
                    if let Some(segment) = snapshot.authority.segment {
                        headers.stages.insert(segment, page);
                    }
                }
            }
        }
        let mut headers = this.headers.borrow_mut();
        let Some(page) = headers.page_mut(target, snapshot.authority.segment) else {
            return div().into_any_element();
        };
        let lease = page.mount();
        let mut snapshot = snapshot;
        snapshot.id = page.id.clone();
        snapshot.layout = lease.borrow().layout();
        snapshot.hovered = page.publication.published.hovered && enabled;
        snapshot.pressed = page.pressed.is_some();
        page.surface.sync_environment(cx);
        if let Some(divider) = &mut page.divider {
            divider.sync_environment(cx);
        }
        page.publish(snapshot);
        let snapshot = page.publication.published.clone();
        let surface = page.surface.entity();
        drop(headers);
        let keyboard = snapshot.clone();
        let pointer_down = snapshot.clone();
        let pointer_up = snapshot.clone();
        let hover = snapshot.clone();
        let ax = snapshot.clone();
        let weak = cx.weak_entity();
        let host = div()
            .id(match target {
                Target::Run => "run-activity-toggle".to_owned(),
                Target::Stage(index) => format!("run-stage-toggle-{index}"),
            })
            .relative()
            .w_full()
            .min_w_0()
            .h(px(if target == Target::Run {
                Layout::CONVERSATION_CLOCK_HEIGHT
            } else {
                Layout::CONVERSATION_TOOL_HEIGHT
            }))
            .track_focus(&focus)
            .capture_key_down(cx.listener(move |this, event, window, cx| {
                this.header_key(&keyboard, event, window, cx)
            }))
            .on_hover(cx.listener(move |this, hovered: &bool, window, cx| {
                if !hover.authority.can_expand || !this.accepts_run_header(&hover, window, cx) {
                    return;
                }
                if let Some(page) = this
                    .headers
                    .borrow_mut()
                    .page_mut(target, hover.authority.segment)
                {
                    let mut snapshot = page.publication.published.clone();
                    snapshot.hovered = *hovered;
                    if !hovered {
                        page.pressed = None;
                        snapshot.pressed = false;
                    }
                    page.publish(snapshot);
                }
            }))
            .when(enabled && snapshot.authority.can_expand, |host| {
                host.cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            if !this.header_contains(&pointer_down, event.position)
                                || !this.accepts_run_header(&pointer_down, window, cx)
                            {
                                return;
                            }
                            if let Some(page) = this
                                .headers
                                .borrow_mut()
                                .page_mut(target, pointer_down.authority.segment)
                            {
                                if let Some(bounds) = page
                                    .mount
                                    .upgrade()
                                    .and_then(|mount| mount.borrow().clipped)
                                {
                                    page.pressed = Some((pointer_down.clone(), bounds));
                                }
                                let mut snapshot = page.publication.published.clone();
                                snapshot.pressed = true;
                                page.publish(snapshot);
                            }
                        }),
                    )
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseUpEvent, window, cx| {
                            let pressed = this
                                .headers
                                .borrow_mut()
                                .page_mut(target, pointer_up.authority.segment)
                                .and_then(|page| {
                                    let mut snapshot = page.publication.published.clone();
                                    snapshot.pressed = false;
                                    page.publish(snapshot);
                                    page.pressed.take()
                                });
                            if let Some((pressed, bounds)) = pressed {
                                let geometry_current = this
                                    .headers
                                    .borrow()
                                    .page(target, pressed.authority.segment)
                                    .and_then(|page| page.mount.upgrade())
                                    .is_some_and(|mount| mount.borrow().clipped == Some(bounds));
                                if geometry_current
                                    && this.header_contains(&pressed, event.position)
                                {
                                    this.activate_run_header(&pressed, window, cx);
                                }
                            }
                        }),
                    )
            })
            .child(div().absolute().inset_0().child(Passive {
                child: surface.into_any_element(),
            }))
            .when(matches!(target, Target::Stage(_)), |host| {
                let Target::Stage(index) = target else {
                    return host;
                };
                host.child(
                    div()
                        .size_full()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap(px(Layout::CONVERSATION_INLINE_GAP))
                        .when(snapshot.authority.icon.is_some(), |row| {
                            row.child(div().size(px(Layout::ICON_SIZE)).flex_shrink_0())
                        })
                        .child(
                            div()
                                .w(px(snapshot.title_width))
                                .min_w_0()
                                .flex_shrink(1.)
                                .overflow_hidden()
                                .child(this.segments[index].title_view.clone()),
                        )
                        .when(snapshot.authority.can_expand, |row| {
                            row.child(
                                div()
                                    .size(px(Layout::CONVERSATION_CHEVRON_SIZE))
                                    .flex_shrink_0(),
                            )
                        }),
                )
            })
            .when(enabled && snapshot.authority.can_expand, |host| {
                host.child(
                    div()
                        .id("run-header-ax")
                        .absolute()
                        .inset_0()
                        .role(Role::Button)
                        .aria_label(snapshot.label.clone())
                        .aria_expanded(snapshot.authority.expanded)
                        .when(focus.is_focused(window), |proxy| {
                            proxy.aria_active_descendant()
                        })
                        .on_a11y_action(AccessibleAction::Click, move |_, window, cx| {
                            let _ = weak
                                .update(cx, |this, cx| this.activate_run_header(&ax, window, cx));
                        }),
                )
            });
        HeaderMount {
            element_id: header_mount_id(group.entity_id(), target, snapshot.authority.segment),
            child: host.into_any_element(),
            mount: lease,
            group: cx.weak_entity(),
            snapshot,
        }
        .into_any_element()
    })
}

pub(super) fn render_divider(group: &Entity<RunActivityGroup>, cx: &App) -> AnyElement {
    let headers = group.read(cx).headers.borrow();
    let Some(surface) = headers.run.as_ref().and_then(|page| page.divider.as_ref()) else {
        return div().into_any_element();
    };
    div()
        .w_full()
        .mt(px(Layout::CONVERSATION_DIVIDER_GAP))
        .h(px(Layout::CONVERSATION_DIVIDER_HEIGHT))
        .child(Passive {
            child: surface.entity().into_any_element(),
        })
        .into_any_element()
}

struct Passive {
    child: AnyElement,
}

impl IntoElement for Passive {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for Passive {
    type RequestLayoutState = ();
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        None
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
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let mask = ContentMask {
            bounds: Bounds {
                origin: window.content_mask().bounds.center(),
                size: Default::default(),
            },
        };
        window.with_content_mask(Some(mask), |window| self.child.prepaint(window, cx));
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let clipped = bounds.intersect(&window.content_mask().bounds);
        window.with_content_mask(Some(ContentMask { bounds: clipped }), |window| {
            self.child.paint(window, cx)
        });
    }
}

fn header_mount_id(group: EntityId, target: Target, segment: Option<EntityId>) -> ElementId {
    match (target, segment) {
        (Target::Run, _) => ("run-header-mount", group).into(),
        (Target::Stage(_), Some(segment)) => ("stage-header-mount", segment).into(),
        (Target::Stage(index), None) => {
            format!("stage-header-mount-{}-{index}", group.as_u64()).into()
        }
    }
}

struct HeaderMount {
    element_id: ElementId,
    child: AnyElement,
    mount: Rc<RefCell<Mount>>,
    group: WeakEntity<RunActivityGroup>,
    snapshot: Snapshot,
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
        Some(self.element_id.clone())
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
        let clipped = bounds.intersect(&window.content_mask().bounds);
        self.mount.borrow_mut().update(
            bounds,
            clipped,
            window.viewport_size(),
            window.scale_factor(),
        );
        let layout = self.mount.borrow().layout();
        let published = self.group.update(cx, |group, cx| {
            let mut headers = group.headers.borrow_mut();
            let page = headers.page_mut(
                self.snapshot.authority.target,
                self.snapshot.authority.segment,
            )?;
            if !page
                .mount
                .upgrade()
                .is_some_and(|mount| Rc::ptr_eq(&mount, &self.mount))
                || !page.id.matches(&self.snapshot.id)
                || !self.snapshot.same_projection(&page.publication.published)
            {
                return None;
            }
            let mut snapshot = self.snapshot.clone();
            snapshot.hovered = page.publication.published.hovered;
            snapshot.pressed = page.publication.published.pressed;
            snapshot.layout = layout;
            page.surface.sync_environment(cx);
            page.publish(snapshot);
            Some(page.publication.published.clone())
        });
        if let Ok(Some(snapshot)) = published {
            self.snapshot = snapshot;
        }
        if let Some(id) = id {
            window.with_element_state::<Rc<RefCell<Mount>>, _>(id, |_, _| ((), self.mount.clone()));
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
        if self.snapshot.authority.target == Target::Run {
            let observed = self.snapshot.clone();
            let _ = self.group.update(cx, |group, cx| {
                if group.accepts_header(&observed, true, window, cx)
                    && group
                        .headers
                        .borrow()
                        .run
                        .as_ref()
                        .is_some_and(|page| page.publication.accepts(&observed))
                {
                    group.schedule_clock(&observed, window, cx);
                }
            });
        }
    }
}
