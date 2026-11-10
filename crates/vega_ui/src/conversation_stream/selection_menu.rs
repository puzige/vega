use super::{ConversationStream, ForkRequested, MessageCopy, StreamEntry, ThinkingBlock};
use crate::nsh::{ProjectionId, Surface};
use gpui_kit::IntoElement;
use gpui_kit::{prelude::*, *};
use std::cell::Cell;
use std::rc::{Rc, Weak};
use vega_conversation::types::PartialCompactionSelection;
use vega_theme::{ThemeColors, theme};

mod accessibility;
pub(super) mod body;
pub(super) mod mounted;
mod pixels;

const WIDTH: f32 = 256.;
const ROW: f32 = 30.;
const PAD: f32 = 4.;

#[derive(Clone, PartialEq, Eq)]
pub(super) struct ForkCommand {
    pub(super) request: ForkRequested,
    pub(super) pending: bool,
    pub(super) enabled: bool,
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct CompactCommand {
    pub(super) thread: String,
    pub(super) model: String,
    pub(super) source: u64,
    pub(super) selection: PartialCompactionSelection,
}

#[derive(Clone)]
pub(super) enum Target {
    Message(Option<super::model::StreamEntryIdentity>),
    Thinking(WeakEntity<ThinkingBlock>, EntityId),
}

#[derive(Clone)]
pub(super) struct ReadingOwner {
    stream: WeakEntity<ConversationStream>,
    thread: String,
    epoch: ProjectionId,
    preview: bool,
    revoked: bool,
    pending: bool,
    target: Target,
}

impl ReadingOwner {
    pub(super) fn thinking(
        owner: &ConversationStream,
        stream: WeakEntity<ConversationStream>,
        card: WeakEntity<ThinkingBlock>,
        group: EntityId,
    ) -> Self {
        Self {
            target: Target::Thinking(card, group),
            ..Self::message(owner, stream, None)
        }
    }

    pub(super) fn message(
        owner: &ConversationStream,
        stream: WeakEntity<ConversationStream>,
        identity: Option<super::model::StreamEntryIdentity>,
    ) -> Self {
        Self {
            stream,
            thread: owner.thread.id.clone(),
            epoch: owner.reading_navigation_epoch.clone(),
            preview: owner.reading_preview,
            revoked: owner.permissions_revoked,
            pending: owner.permission_change_pending(),
            target: Target::Message(identity),
        }
    }

    pub(super) fn same_binding(&self, other: &Self) -> bool {
        self.stream == other.stream
            && self.thread == other.thread
            && self.epoch.matches(&other.epoch)
            && self.preview == other.preview
            && self.revoked == other.revoked
            && self.pending == other.pending
    }

    pub(super) fn root_current(&self, window: &Window, cx: &App) -> bool {
        let Some(entity) = self.stream.upgrade() else {
            return false;
        };
        let stream = entity.read(cx);
        self.epoch.matches(&stream.reading_navigation_epoch)
            && stream.thread.id == self.thread
            && stream.reading_preview == self.preview
            && stream.permissions_revoked == self.revoked
            && stream.permission_change_pending() == self.pending
            && window.is_window_active()
            && cx
                .try_global::<crate::settings::SettingsOpen>()
                .is_some_and(|settings| !settings.0)
            && cx
                .try_global::<crate::sidebar::PendingDeleteConfirm>()
                .is_none_or(|pending| pending.0.is_none())
            && stream
                .resume_tail_authority
                .is_some_and(|authority| authority(&self.stream, window, cx))
    }

    fn current_for_frame(&self, source: u64, window: &Window, cx: &App) -> bool {
        if mounted::list_bounds(&self.stream, window, cx).is_none() {
            return self.current(source, window, cx);
        }
        self.root_current(window, cx)
            && self.stream.upgrade().is_some_and(|entity| {
                let stream = entity.read(cx);
                (0..stream.entries.len())
                    .any(|index| self.source_matches(source, index, stream, cx))
            })
    }

    fn source_matches(
        &self,
        source: u64,
        index: usize,
        stream: &ConversationStream,
        cx: &App,
    ) -> bool {
        let Some(entry) = stream.entries.get(index) else {
            return false;
        };
        match &self.target {
                Target::Message(identity) => {
                    identity.as_ref().is_some_and(|identity| stream.entry_identities.get(index) == Some(identity))
                        && (matches!(entry, StreamEntry::User { copy, .. } | StreamEntry::Assistant { copy, .. } if copy.id == source)
                            || stream.copy_source_for_entry(index).is_some_and(|copy| copy.id == source))
                }
                Target::Thinking(card, group_id) => {
                    match entry {
                        StreamEntry::RunActivity { group } | StreamEntry::RunActivitySegment { group, .. } if group.entity_id() == *group_id => {
                            card.upgrade().is_some_and(|card| {
                                group.read(cx).children().iter().any(|child| matches!(child, super::RunActivityChild::Thinking(child) if child.entity_id() == card.entity_id()))
                                    && card.read(cx).reading_menu_current(source)
                            })
                        }
                        _ => false,
                    }
                }
        }
    }

    pub(super) fn current(&self, source: u64, window: &Window, cx: &App) -> bool {
        let Some(entity) = self.stream.upgrade() else {
            return false;
        };
        let stream = entity.read(cx);
        if !self.root_current(window, cx) {
            return false;
        }
        (0..stream.entries.len()).any(|index| {
            self.source_matches(source, index, stream, cx)
                && stream.list.bounds_for_item(index).is_some_and(|bounds| {
                    !bounds.intersect(&stream.list.viewport_bounds()).is_empty()
                })
        })
    }
}

#[derive(Clone, PartialEq, Eq)]
enum Command {
    CopyOriginal,
    CopySelection,
    Fork(ForkCommand),
    Compact(CompactCommand),
}

#[derive(Clone, PartialEq, Eq)]
struct Item {
    label: String,
    enabled: bool,
    command: Command,
}

fn items(copy: &MessageCopy, original: bool) -> Vec<Item> {
    let mut rows = Vec::new();
    if original {
        rows.push(Item {
            label: vega_i18n::t("ui.copy_original").into(),
            enabled: copy.has_text(),
            command: Command::CopyOriginal,
        });
    }
    rows.push(Item {
        label: vega_i18n::t("ui.copy").into(),
        enabled: copy.has_selected_text(),
        command: Command::CopySelection,
    });
    if let Some(command) = copy.fork_menu.borrow().clone() {
        rows.push(Item {
            label: vega_i18n::t(if command.pending {
                "ui.fork_creating"
            } else {
                "ui.branch_in_new_chat"
            })
            .into(),
            enabled: command.enabled,
            command: Command::Fork(command),
        });
    }
    for command in copy.compaction_menu.borrow().iter() {
        rows.push(Item {
            label: vega_i18n::t(match command.selection.direction {
                vega_conversation::types::PartialCompactionDirection::UpTo => {
                    "ui.compact_before_message"
                }
                vega_conversation::types::PartialCompactionDirection::From => {
                    "ui.compact_from_message"
                }
            })
            .into(),
            enabled: true,
            command: Command::Compact(command.clone()),
        });
    }
    rows
}

#[derive(Clone)]
struct Snapshot {
    id: ProjectionId,
    content: u64,
    selection: u64,
    original: String,
    selected: Option<String>,
    items: Vec<Item>,
    colors: ThemeColors,
    focused: Option<usize>,
    hovered: Option<usize>,
    pressed: Option<usize>,
    language: &'static str,
    appearance: vega_theme::Appearance,
    width: f32,
    height: f32,
}

impl Snapshot {
    fn matches(&self, other: &Self) -> bool {
        self.id.matches(&other.id)
            && self.content == other.content
            && self.selection == other.selection
            && self.original == other.original
            && self.selected == other.selected
            && self.items == other.items
            && self.language == other.language
            && self.appearance == other.appearance
            && self.width == other.width
            && self.height == other.height
    }
}

enum Request {
    Layout(Snapshot, [f32; 4], Vec<[f32; 4]>),
    Activate(Snapshot, usize),
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

struct Lease {
    frame: std::cell::RefCell<Option<Rc<()>>>,
    live: Cell<bool>,
    geometry: Cell<Option<Geometry>>,
}

impl Drop for Lease {
    fn drop(&mut self) {
        self.live.set(false);
        self.geometry.set(None);
    }
}

struct Open {
    source: ReadingOwner,
    snapshot: Snapshot,
    surface: Surface<Snapshot, Request>,
    previous_focus: Option<FocusHandle>,
    position: Point<Pixels>,
    focus: Vec<FocusHandle>,
    menu_focus: FocusHandle,
    displayed: Option<([f32; 4], Vec<[f32; 4]>)>,
    lease: Weak<Lease>,
    anchor: Geometry,
    focus_started: bool,
    subscriptions: Vec<Subscription>,
}

struct MenuState {
    copy: MessageCopy,
    original: bool,
    enabled: bool,
    owner: Option<ReadingOwner>,
    open: Option<Open>,
    lease: Weak<Lease>,
    window: WindowId,
    subscriptions: Vec<Subscription>,
}

impl MenuState {
    fn close(&mut self, restore: bool, window: &mut Window, cx: &mut App) {
        if let Some(open) = self.open.take() {
            let held = open
                .focus
                .iter()
                .any(|focus| focus.contains_focused(window, cx))
                || open.menu_focus.contains_focused(window, cx);
            open.surface.entity().read(cx).stop();
            if restore
                && held
                && let Some(previous) = open.previous_focus
            {
                previous.focus(window, cx);
            }
        }
    }

    fn body_geometry(&self) -> Option<Geometry> {
        self.lease
            .upgrade()
            .filter(|lease| lease.live.get())
            .and_then(|lease| lease.geometry.get())
    }

    fn valid(&self, observed: &Snapshot, window: &Window, cx: &App) -> bool {
        let Some(open) = &self.open else {
            return false;
        };
        self.enabled
            && self.window == window.window_handle().window_id()
            && open.snapshot.matches(observed)
            && self.copy.content_generation() == observed.content
            && self.copy.selection_generation() == observed.selection
            && self.copy.display_text() == observed.original
            && self.copy.selected_text_snapshot() == observed.selected
            && (observed.selected.is_none()
                || self
                    .copy
                    .nsh_reading
                    .borrow()
                    .as_ref()
                    .is_none_or(|binding| binding.selection_current(cx)))
            && items(&self.copy, self.original) == observed.items
            && vega_i18n::language() == observed.language
            && theme(cx).appearance == observed.appearance
            && open.source.current_for_frame(self.copy.id, window, cx)
            && self.lease.upgrade().is_some_and(|lease| {
                mounted::lease_current(&lease, &open.source.stream, window, cx)
            })
            && self.body_geometry() == Some(open.anchor)
            && open.anchor.viewport == window.viewport_size()
            && open.anchor.scale == window.scale_factor()
            && open.surface.errors().borrow().is_none()
            && open.source.stream.upgrade().is_some_and(|entity| {
                let stream = entity.read(cx);
                mounted::list_bounds(&open.source.stream, window, cx)
                    .unwrap_or_else(|| stream.list.viewport_bounds())
                    == open.anchor.list
                    && stream.foreground_height == open.anchor.foreground
            })
    }

    fn accepts(&self, observed: &Snapshot, index: usize, window: &Window, cx: &App) -> bool {
        self.valid(observed, window, cx)
            && observed.items.get(index).is_some_and(|item| item.enabled)
            && self.open.as_ref().is_some_and(|open| {
                let Some((root, rows)) = &open.displayed else {
                    return false;
                };
                let Some(row) = rows.get(index) else {
                    return false;
                };
                open.lease
                    .upgrade()
                    .filter(|lease| lease.live.get())
                    .and_then(|lease| lease.geometry.get())
                    .is_some_and(|geometry| geometry.accepts(root, row, window))
            })
    }

    fn dispatch(
        &mut self,
        observed: &Snapshot,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.accepts(observed, index, window, cx) {
            return;
        }
        let command = observed.items[index].command.clone();
        let owner = self.open.as_ref().map(|open| open.source.clone());
        match command {
            Command::CopyOriginal => self.copy.copy_original(cx),
            Command::CopySelection => self.copy.copy_selected_text(cx),
            Command::Fork(command) => {
                if let Some(owner) = owner
                    && !owner.preview
                    && !owner.revoked
                    && !owner.pending
                {
                    let _ = owner.stream.update(cx, |stream, cx| {
                        if !stream.permission_change_pending() {
                            stream.request_fork(&command.request, cx);
                        }
                    });
                }
            }
            Command::Compact(command) => {
                if let Some(owner) = owner
                    && !owner.preview
                    && !owner.revoked
                    && !owner.pending
                {
                    let _ = owner.stream.update(cx, |stream, cx| {
                        stream.request_reading_compaction(&command, cx)
                    });
                }
            }
        }
        self.close(true, window, cx);
        cx.notify();
    }

    fn open(&mut self, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let binding = self.copy.nsh_reading.borrow().clone();
        if let Some(binding) = binding
            && !binding.refresh(window, cx)
        {
            self.copy.invalidate_selection();
        }
        let Some(owner) = self.owner.clone() else {
            return;
        };
        if !self.enabled || !owner.current(self.copy.id, window, cx) {
            return;
        }
        let Some(anchor) = self.body_geometry() else {
            return;
        };
        if !anchor.bounds.intersect(&anchor.clip).contains(&position) {
            return;
        }
        let previous_focus = self
            .open
            .as_ref()
            .and_then(|open| {
                open.focus
                    .iter()
                    .any(|focus| focus.contains_focused(window, cx))
                    .then(|| open.previous_focus.clone())
                    .flatten()
            })
            .or_else(|| window.focused(cx));
        self.close(false, window, cx);
        let rows = items(&self.copy, self.original);
        let width = WIDTH.min((f32::from(window.viewport_size().width) - 16.).max(1.));
        let height = rows.len() as f32 * ROW + PAD * 2. + 2.;
        let snapshot = Snapshot {
            id: ProjectionId::new(),
            content: self.copy.content_generation(),
            selection: self.copy.selection_generation(),
            original: self.copy.display_text(),
            selected: self.copy.selected_text_snapshot(),
            items: rows,
            colors: theme(cx).colors,
            focused: None,
            hovered: None,
            pressed: None,
            language: vega_i18n::language(),
            appearance: theme(cx).appearance,
            width,
            height,
        };
        let focus: Vec<_> = snapshot
            .items
            .iter()
            .map(|item| cx.focus_handle().tab_stop(item.enabled))
            .collect();
        let menu_focus = cx.focus_handle().tab_stop(false);
        let mut subscriptions = Vec::new();
        for handle in focus.iter().chain(std::iter::once(&menu_focus)) {
            subscriptions.push(cx.on_focus(handle, window, |_, _, cx| cx.notify()));
            subscriptions.push(cx.on_blur(handle, window, |this, window, cx| {
                let session = this.open.as_ref().map(|open| open.snapshot.id.clone());
                let weak = cx.weak_entity();
                window.defer(cx, move |window, cx| {
                    let _ = weak.update(cx, |this, cx| {
                        if this.open.as_ref().is_some_and(|open| {
                            session
                                .as_ref()
                                .is_some_and(|id| open.snapshot.id.matches(id))
                                && !open.menu_focus.contains_focused(window, cx)
                                && !open
                                    .focus
                                    .iter()
                                    .any(|focus| focus.contains_focused(window, cx))
                        }) {
                            this.close(false, window, cx);
                            cx.notify();
                        }
                    });
                });
            }));
        }
        let (surface, mut requests) =
            Surface::new_transparent(snapshot.clone(), pixels::render, window, cx);
        let _ = surface.focus_handle(cx).tab_stop(false);
        let mut errors = surface.errors();
        let session = snapshot.id.clone();
        cx.spawn(async move |this, cx| {
            while let Some(request) = requests.recv().await {
                if this
                    .update_in(cx, |this, window, cx| match request {
                        Request::Activate(observed, index) => {
                            this.dispatch(&observed, index, window, cx)
                        }
                        Request::Layout(observed, root, rows) => {
                            if this.valid(&observed, window, cx)
                                && valid_layout(&observed, &root, &rows)
                            {
                                let changed = this.open.as_ref().is_some_and(|open| {
                                    open.displayed.as_ref() != Some(&(root, rows.clone()))
                                });
                                if let Some(open) = &mut this.open {
                                    open.displayed = Some((root, rows));
                                }
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
                    let _ = this.update_in(cx, |this, window, cx| {
                        if this
                            .open
                            .as_ref()
                            .is_some_and(|open| open.snapshot.id.matches(&session))
                        {
                            this.close(true, window, cx);
                            cx.notify();
                        }
                    });
                    break;
                }
            }
        })
        .detach();
        self.open = Some(Open {
            source: owner,
            snapshot,
            surface,
            previous_focus,
            position,
            focus,
            menu_focus,
            displayed: None,
            lease: Weak::new(),
            anchor,
            focus_started: false,
            subscriptions,
        });
        cx.notify();
    }
}

fn valid_layout(snapshot: &Snapshot, root: &[f32; 4], rows: &[[f32; 4]]) -> bool {
    root.iter().all(|value| value.is_finite())
        && root[2] > 0.
        && root[3] > 0.
        && (root[2] - snapshot.width).abs() <= 0.5
        && (root[3] - snapshot.height).abs() <= 0.5
        && rows.len() == snapshot.items.len()
        && rows.iter().all(|row| {
            row.iter().all(|value| value.is_finite())
                && row[2] > 0.
                && row[3] > 0.
                && row[0] >= root[0]
                && row[1] >= root[1]
                && row[0] + row[2] <= root[0] + root[2] + 0.5
                && row[1] + row[3] <= root[1] + root[3] + 0.5
        })
}

impl Geometry {
    fn accepts(&self, root: &[f32; 4], row: &[f32; 4], window: &Window) -> bool {
        let rect = Bounds::new(
            point(
                self.bounds.left() + px(row[0] - root[0]),
                self.bounds.top() + px(row[1] - root[1]),
            ),
            size(px(row[2]), px(row[3])),
        );
        self.viewport == window.viewport_size()
            && self.scale == window.scale_factor()
            && (f32::from(self.bounds.size.width) - root[2]).abs() <= 0.5
            && (f32::from(self.bounds.size.height) - root[3]).abs() <= 0.5
            && !rect.is_empty()
            && rect.intersect(&self.clip) == rect
    }
}

#[derive(IntoElement)]
pub(super) struct SelectionMenu {
    body: AnyElement,
    copy: MessageCopy,
    original: bool,
    enabled: bool,
    stream: Option<WeakEntity<ConversationStream>>,
}

impl SelectionMenu {
    pub(super) fn new(
        body: AnyElement,
        copy: MessageCopy,
        original: bool,
        stream: Option<WeakEntity<ConversationStream>>,
    ) -> Self {
        Self {
            body,
            copy,
            original,
            enabled: true,
            stream,
        }
    }

    pub(super) fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

impl RenderOnce for SelectionMenu {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let id = self.copy.id;
        let state = window.use_keyed_state(("nsh-selection-menu-state", id), cx, |window, _cx| {
            MenuState {
                copy: self.copy.clone(),
                original: self.original,
                enabled: self.enabled,
                owner: None,
                open: None,
                lease: Weak::new(),
                window: window.window_handle().window_id(),
                subscriptions: Vec::new(),
            }
        });
        let lease = state.read(cx).lease.upgrade().unwrap_or_else(|| {
            Rc::new(Lease {
                frame: Default::default(),
                live: Cell::new(true),
                geometry: Cell::new(None),
            })
        });
        state.update(cx, |this, _| {
            this.copy = self.copy.clone();
            this.original = self.original;
            this.enabled = self.enabled;
            this.lease = Rc::downgrade(&lease);
        });
        let weak = state.downgrade();
        if let Some(binding) = self.copy.nsh_reading.borrow().as_ref() {
            binding.attach_menu(weak.clone(), cx);
        }
        let source = self.copy;
        let frozen_owner = source.reading_owner.borrow().clone().filter(|owner| {
            self.stream
                .as_ref()
                .is_none_or(|stream| owner.stream == *stream)
        });
        let enabled = self.enabled;
        let body_lease = Rc::downgrade(&lease);
        let body = div()
            .id(("nsh-selection-menu-body", id))
            .relative()
            .child(self.body)
            .on_mouse_down(MouseButton::Right, {
                let weak = weak.clone();
                move |event, window, cx| {
                    let _ = weak.update(cx, |this, cx| this.open(event.position, window, cx));
                }
            })
            .child(
                canvas(
                    move |bounds, window, cx| {
                        let geometry = source
                            .reading_owner
                            .borrow()
                            .as_ref()
                            .and_then(|owner| owner.stream.upgrade().map(|entity| (owner, entity)))
                            .map(|(owner, entity)| {
                                let stream = entity.read(cx);
                                Geometry {
                                    bounds,
                                    clip: window.content_mask().bounds,
                                    viewport: window.viewport_size(),
                                    scale: window.scale_factor(),
                                    list: mounted::list_bounds(&owner.stream, window, cx)
                                        .unwrap_or_else(|| stream.list.viewport_bounds()),
                                    foreground: stream.foreground_height,
                                }
                            });
                        if let Some(lease) = body_lease.upgrade() {
                            lease.geometry.set(geometry);
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            );
        let popup_state = state.clone();
        let popup = container_query(move |_, window, cx| {
            popup_state.update(cx, |this, cx| {
                this.owner = frozen_owner.clone();
                if !enabled
                    || this
                        .open
                        .as_ref()
                        .is_some_and(|open| !this.valid(&open.snapshot, window, cx))
                {
                    this.close(true, window, cx);
                }
            });
            popup_state.clone().into_any_element()
        })
        .absolute()
        .size_full();
        mounted::Mounted {
            child: body.child(popup).into_any_element(),
            lease,
            id: ("nsh-selection-menu-mounted", id).into(),
        }
    }
}

impl Render for MenuState {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.subscriptions.is_empty() {
            self.subscriptions = vec![
                cx.on_release(|this, cx| {
                    if let Some(open) = this.open.take() {
                        open.surface.entity().read(cx).stop();
                    }
                }),
                cx.observe_window_activation(window, |this, window, cx| {
                    this.close(false, window, cx);
                    cx.notify();
                }),
                cx.observe_window_visibility(window, |this, _, window, cx| {
                    this.close(false, window, cx);
                    cx.notify();
                }),
            ];
        }
        if self
            .open
            .as_ref()
            .is_some_and(|open| !self.valid(&open.snapshot, window, cx))
        {
            self.close(true, window, cx);
        }
        let Some(open) = &mut self.open else {
            return div().into_any_element();
        };
        let _ = &open.subscriptions;
        if open.displayed.is_some() && !open.focus_started {
            open.focus_started = true;
            let session = open.snapshot.id.clone();
            let prior = open.previous_focus.clone();
            let weak = cx.weak_entity();
            window.defer(cx, move |window, cx| {
                let _ = weak.update(cx, |this, cx| {
                    if this
                        .open
                        .as_ref()
                        .is_some_and(|open| open.snapshot.id.matches(&session))
                        && window.focused(cx) == prior
                        && let Some(open) = &this.open
                        && this.valid(&open.snapshot, window, cx)
                    {
                        if let Some((index, _)) = open
                            .snapshot
                            .items
                            .iter()
                            .enumerate()
                            .find(|(_, item)| item.enabled)
                        {
                            open.focus[index].focus(window, cx);
                        } else {
                            open.menu_focus.focus(window, cx);
                        }
                    }
                });
            });
        }
        let lease = open.lease.upgrade().unwrap_or_else(|| {
            Rc::new(Lease {
                frame: Default::default(),
                live: Cell::new(true),
                geometry: Cell::new(None),
            })
        });
        open.lease = Rc::downgrade(&lease);
        open.snapshot.focused = open.focus.iter().position(|focus| focus.is_focused(window));
        open.surface.sync_environment(cx);
        open.surface.publish(open.snapshot.clone());
        let observed = open.snapshot.clone();
        let position = open.position;
        let view = open.surface.entity();
        let weak = cx.weak_entity();
        let paint_lease = Rc::downgrade(&lease);
        let mut host = div()
            .id("nsh-selection-menu-popup")
            .occlude()
            .relative()
            .w(px(observed.width))
            .h(px(observed.height))
            .role(Role::Menu)
            .track_focus(&open.menu_focus)
            .tab_stop(false)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                let modifiers = event.keystroke.modifiers;
                if event.prefer_character_input
                    || event.is_held
                    || modifiers.control
                    || modifiers.alt
                    || modifiers.platform
                    || modifiers.function
                {
                    return;
                }
                if event.keystroke.key == "escape" {
                    this.close(true, window, cx);
                    cx.notify();
                    cx.stop_propagation();
                } else if event.keystroke.key == "tab" {
                    if let Some(open) = &this.open {
                        let enabled: Vec<_> = open
                            .snapshot
                            .items
                            .iter()
                            .enumerate()
                            .filter_map(|(index, item)| item.enabled.then_some(index))
                            .collect();
                        let current = enabled
                            .iter()
                            .position(|index| open.focus[*index].is_focused(window));
                        if !enabled.is_empty() {
                            let next = if modifiers.shift {
                                current.map_or(enabled.len() - 1, |index| {
                                    (index + enabled.len() - 1) % enabled.len()
                                })
                            } else {
                                current.map_or(0, |index| (index + 1) % enabled.len())
                            };
                            open.focus[enabled[next]].focus(window, cx);
                        }
                    }
                    cx.stop_propagation();
                }
            }))
            .on_mouse_down_out(cx.listener(|this, _, window, cx| {
                this.close(false, window, cx);
                cx.notify();
            }))
            .on_scroll_wheel(cx.listener(|this, _, window, cx| {
                this.close(true, window, cx);
                cx.notify();
                cx.stop_propagation();
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .child(view)
            .child(
                canvas(
                    move |bounds, window, cx| {
                        let _ = weak.update(cx, |this, _| {
                            if let Some(lease) = paint_lease.upgrade() {
                                let geometry = this.open.as_ref().map(|open| Geometry {
                                    bounds,
                                    clip: window.content_mask().bounds,
                                    viewport: window.viewport_size(),
                                    scale: window.scale_factor(),
                                    list: open.anchor.list,
                                    foreground: open.anchor.foreground,
                                });
                                lease.geometry.set(geometry);
                            }
                        });
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            );
        if let Some((root, rows)) = &open.displayed {
            for (index, (row, item)) in rows.iter().zip(&observed.items).enumerate() {
                let focus = open.focus[index].clone();
                let keyboard_focus = focus.clone();
                let action = observed.clone();
                let pointer = observed.clone();
                let accessible = observed.clone();
                let weak = cx.weak_entity();
                host = host.child(accessibility::disabled(
                    div()
                        .id(("nsh-selection-menu-item", index))
                        .absolute()
                        .left(px(row[0] - root[0]))
                        .top(px(row[1] - root[1]))
                        .w(px(row[2]))
                        .h(px(row[3]))
                        .track_focus(&focus)
                        .tab_stop(item.enabled)
                        .role(Role::MenuItem)
                        .aria_label(item.label.clone())
                        .capture_any_mouse_down(|_, window, _| window.prevent_default())
                        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                            if let Some(open) = &mut this.open {
                                open.snapshot.hovered = hovered.then_some(index);
                                if !hovered {
                                    open.snapshot.pressed = None;
                                }
                                cx.notify();
                            }
                        }))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                if let Some(open) = &mut this.open {
                                    open.snapshot.pressed = Some(index);
                                    cx.notify();
                                }
                            }),
                        )
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                if let Some(open) = &mut this.open {
                                    open.snapshot.pressed = None;
                                    cx.notify();
                                }
                            }),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.dispatch(&pointer, index, window, cx)
                        }))
                        .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                            let modifiers = event.keystroke.modifiers;
                            if !keyboard_focus.is_focused(window)
                                || event.prefer_character_input
                                || event.is_held
                                || modifiers.control
                                || modifiers.alt
                                || modifiers.platform
                                || modifiers.function
                            {
                                return;
                            }
                            match event.keystroke.key.as_str() {
                                "enter" | "space" | " " if !modifiers.shift => {
                                    this.dispatch(&action, index, window, cx);
                                    cx.stop_propagation();
                                }
                                "escape" => {
                                    this.close(true, window, cx);
                                    cx.notify();
                                    cx.stop_propagation();
                                }
                                _ => {}
                            }
                        }))
                        .on_a11y_action(AccessibleAction::Click, move |_, window, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.dispatch(&accessible, index, window, cx)
                            });
                        }),
                    !item.enabled,
                ));
            }
        }
        let mount = mounted::Mounted {
            child: host.into_any_element(),
            lease,
            id: "nsh-selection-menu-popup-mounted".into(),
        };
        deferred(
            anchored()
                .position(position)
                .snap_to_window_with_margin(px(8.))
                .anchor(Anchor::TopLeft)
                .child(mount),
        )
        .with_priority(gpui_kit::base::POPUP_PRIORITY)
        .into_any_element()
    }
}
