use super::nsh_view::{self, Action, Draft, Request, Snapshot};
use super::*;
use crate::nsh::{ProjectionId, Surface};
use gpui_kit::{
    AnyElement, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, KeyDownEvent,
    LayoutId, Pixels, Subscription, anchored, div, point, px,
};
use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::sync::Arc;

#[derive(Default)]
struct Geometry {
    visible: bool,
    bounds: Option<Bounds<Pixels>>,
}

struct Page {
    surface: Surface<Snapshot, Request>,
    recovery: Option<Surface<Snapshot, Request>>,
    id: ProjectionId,
    lease: Weak<()>,
    geometry: Rc<RefCell<Geometry>>,
    shown: Snapshot,
    displayed: Option<Snapshot>,
    failed: bool,
}

impl Page {
    fn live(&self) -> bool {
        self.lease.upgrade().is_some() && self.geometry.borrow().visible
    }

    fn entity(&self) -> Entity<nsh_platform_gpui::AppView> {
        self.recovery.as_ref().unwrap_or(&self.surface).entity()
    }
}

pub(super) struct State {
    trigger: Option<Page>,
    popup: Option<Page>,
    rows: Arc<[BranchItem]>,
    dirty: bool,
    revision: ProjectionId,
    draft: Draft,
    carrier: String,
    keyboard: bool,
    input_focused: bool,
    subscriptions: Vec<Subscription>,
    reveal: Option<usize>,
    reveal_id: Option<BranchId>,
    measured: Option<(Snapshot, BranchId, ProjectionId)>,
    row_geometry: nsh_view::RowGeometry,
    draft_revision: u64,
}

impl Default for State {
    fn default() -> Self {
        Self {
            trigger: None,
            popup: None,
            rows: Arc::from([]),
            dirty: true,
            revision: ProjectionId::new(),
            draft: Draft::default(),
            carrier: String::new(),
            keyboard: false,
            input_focused: false,
            subscriptions: Vec::new(),
            reveal: None,
            reveal_id: None,
            measured: None,
            row_geometry: nsh_view::RowGeometry::default(),
            draft_revision: 0,
        }
    }
}

impl State {
    pub(super) fn invalidate_rows(&mut self) {
        self.dirty = true;
        self.revision = ProjectionId::new();
    }

    pub(super) fn close_popup(&mut self) {
        if self.popup.take().is_some() {
            self.revision = ProjectionId::new();
            if let Ok(mut draft) = self.draft.0.lock() {
                draft.page = None;
                draft.composing = false;
            }
        }
        self.input_focused = false;
        self.measured = None;
        let _ = self.row_geometry.invalidate();
    }

    pub(super) fn reconcile_open(&mut self, model: &BranchSelectorModel) {
        self.carrier = model.filter().to_owned();
        if let Ok(mut draft) = self.draft.0.lock() {
            draft.text.clone_from(&self.carrier);
            draft.composing = false;
            draft.page = None;
            draft.revision = draft.revision.saturating_add(1);
            self.draft_revision = draft.revision;
        }
        self.input_focused = false;
        self.measured = None;
        let _ = self.row_geometry.invalidate();
    }

    pub(super) fn reset(&mut self) {
        let subscriptions = std::mem::take(&mut self.subscriptions);
        *self = Self::default();
        self.subscriptions = subscriptions;
    }
}

impl BranchSelector {
    fn snapshot(&self, page: ProjectionId, failure: bool, cx: &App) -> Snapshot {
        Snapshot {
            page,
            revision: self.presentation.revision.clone(),
            thread: self.thread_id.clone(),
            project: self.project_id.clone(),
            open: self.model.is_open(),
            disabled: self.disabled,
            pending: self.model.pending_key(),
            generation: self.model.snapshot_generation(),
            rows: self.presentation.rows.clone(),
            status: self.model.status,
            total: self
                .model
                .snapshot
                .as_ref()
                .map_or(0, |snapshot| snapshot.branches.len()),
            focused: self.model.focused(),
            visual: self.model.visual_focused,
            reveal: self.presentation.reveal,
            label: self.current_head_label().to_owned(),
            chip: self.chip_chrome,
            keyboard: self.presentation.keyboard,
            failure,
            focus_input: self.presentation.input_focused,
            draft_revision: self.presentation.draft_revision,
            draft: self.presentation.draft.clone(),
            geometry: self.presentation.row_geometry.clone(),
            colors: theme(cx).colors,
            appearance: theme(cx).appearance,
            language: vega_i18n::language(),
        }
    }

    fn refresh_projection(&mut self, cx: &App) {
        let carrier = self.search.read(cx).text().to_owned();
        if carrier != self.presentation.carrier {
            self.presentation.carrier.clone_from(&carrier);
            if let Ok(mut draft) = self.presentation.draft.0.lock() {
                draft.text = carrier;
                draft.composing = self.search.read(cx).is_composing();
                draft.revision = draft.revision.saturating_add(1);
            }
        }
        if let Ok(draft) = self.presentation.draft.0.lock() {
            self.presentation.draft_revision = draft.revision;
        }
        if self.presentation.dirty || self.presentation.reveal_id != self.model.visual_focused {
            self.presentation.reveal_id = self.model.visual_focused;
            self.presentation.reveal = self
                .model
                .visual_focused
                .and_then(|_| self.model.focused_position());
        }
        if self.presentation.dirty {
            self.presentation.rows = self
                .model
                .visible_rows(0..self.model.visible_count())
                .into_iter()
                .map(|(_, row)| row)
                .collect();
            self.presentation.dirty = false;
        }
    }

    fn watch_requests(
        &self,
        mut requests: tokio::sync::mpsc::Receiver<Request>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        cx.spawn_in(window, async move |this, cx| {
            while let Some(request) = requests.recv().await {
                if this
                    .update_in(cx, |this, window, cx| this.receive(request, window, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }

    fn create_page(&self, popup: bool, window: &mut Window, cx: &mut Context<Self>) -> Page {
        let id = ProjectionId::new();
        if popup && let Ok(mut draft) = self.presentation.draft.0.lock() {
            draft.page = Some(id.clone());
        }
        let snapshot = self.snapshot(id.clone(), false, cx);
        let (surface, requests) = if popup {
            Surface::new(snapshot.clone(), nsh_view::popup, window, cx)
        } else {
            Surface::new_transparent(snapshot.clone(), nsh_view::trigger, window, cx)
        };
        self.watch_requests(requests, window, cx);
        let mut errors = surface.errors();
        let owner = id.clone();
        cx.spawn(async move |this, cx| {
            while errors.changed().await.is_ok() {
                if errors.borrow_and_update().is_some() {
                    let _ = this.update(cx, |this, cx| {
                        let page = if popup {
                            &mut this.presentation.popup
                        } else {
                            &mut this.presentation.trigger
                        };
                        if let Some(page) = page
                            && page.id.matches(&owner)
                        {
                            page.failed = true;
                            cx.notify();
                        }
                    });
                    break;
                }
            }
        })
        .detach();
        Page {
            surface,
            recovery: None,
            id,
            lease: Weak::new(),
            geometry: Rc::new(RefCell::new(Geometry::default())),
            shown: snapshot,
            displayed: None,
            failed: false,
        }
    }

    fn accept(&self, observed: &Snapshot, popup: bool, cx: &App) -> bool {
        let page = if popup {
            &self.presentation.popup
        } else {
            &self.presentation.trigger
        };
        let Some(page) = page else {
            return false;
        };
        if !page.live()
            || !page.shown.same_authority(observed)
            || !self
                .snapshot(page.id.clone(), page.failed, cx)
                .same_authority(observed)
            || matches!(
                self.current_head.state,
                Some(vega_conversation::types::ProjectBranchState::NonGit)
            )
        {
            return false;
        }
        !popup || self.model.is_open()
    }

    fn receive(&mut self, request: Request, window: &mut Window, cx: &mut Context<Self>) {
        match request {
            Request::Displayed(observed) => {
                for page in [&mut self.presentation.trigger, &mut self.presentation.popup]
                    .into_iter()
                    .flatten()
                {
                    if page.id.matches(&observed.page) && page.shown.same_authority(&observed) {
                        page.displayed = Some(observed.clone());
                    }
                }
            }
            Request::Filter(page, revision) => {
                if !self
                    .presentation
                    .popup
                    .as_ref()
                    .is_some_and(|popup| popup.live() && popup.id.matches(&page))
                    || !self.model.is_open()
                    || self.model.is_pending()
                {
                    return;
                }
                let value = self.presentation.draft.0.lock().ok().and_then(|draft| {
                    (draft.revision >= revision && !draft.composing).then(|| draft.text.clone())
                });
                let Some(text) = value else {
                    return;
                };
                self.presentation.carrier.clone_from(&text);
                self.search
                    .update(cx, |input, cx| input.set_text(&text, cx));
                if self.model.set_filter(&text) {
                    self.presentation.invalidate_rows();
                }
                cx.notify();
            }
            Request::GeometryChanged(page) => {
                if self
                    .presentation
                    .popup
                    .as_ref()
                    .is_some_and(|popup| popup.id.matches(&page))
                {
                    cx.notify();
                }
            }
            Request::Measured(observed, id, rect, viewport, lease, revision) => {
                if self.accept(&observed, true, cx)
                    && observed.visual == Some(id)
                    && !observed.disabled
                    && observed.pending.is_none()
                    && observed.rows.iter().any(|row| row.id == id && !row.current)
                {
                    if self
                        .presentation
                        .row_geometry
                        .install(&revision, rect, viewport, lease)
                    {
                        self.presentation.measured = Some((observed, id, revision));
                    }
                    cx.notify();
                }
            }
            Request::InputFocused(page) => {
                if self
                    .presentation
                    .popup
                    .as_ref()
                    .is_some_and(|popup| popup.live() && popup.id.matches(&page))
                {
                    self.presentation.input_focused = true;
                    cx.notify();
                }
            }
            Request::Action(observed, action, row, draft_revision) => {
                let popup = self
                    .presentation
                    .popup
                    .as_ref()
                    .is_some_and(|page| page.id.matches(&observed.page));
                if matches!(
                    action,
                    Action::Activate(_) | Action::Previous | Action::Next
                ) && !popup
                {
                    return;
                }
                self.guarded_action(observed, action, popup, (row, draft_revision), window, cx);
            }
        }
    }

    fn guarded_action(
        &mut self,
        observed: Snapshot,
        action: Action,
        popup: bool,
        interaction: (Option<std::sync::Weak<()>>, Option<u64>),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (row, draft_revision) = interaction;
        if !self.accept(&observed, popup, cx) {
            return;
        }
        if matches!(action, Action::Close | Action::Reload) {
            self.perform(action, popup, window, cx);
            return;
        }
        let filter = popup.then(|| self.model.filter().to_owned());
        let _ = observed
            .draft
            .interact(filter.as_deref(), draft_revision, row.as_ref(), || {
                self.perform(action, popup, window, cx);
            });
    }

    fn perform(
        &mut self,
        action: Action,
        popup: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            Action::Toggle => {
                if self.model.is_open() {
                    self.request_close(cx);
                } else if self.request_open(cx) {
                    self.presentation.keyboard = true;
                    window.focus(&self.focus, cx);
                }
            }
            Action::Activate(id) => {
                if self.presentation.draft.0.lock().map_or(true, |draft| {
                    draft.composing || draft.text.trim().to_lowercase() != self.model.filter()
                }) {
                    return;
                }
                if let Some(page) = &self.presentation.popup {
                    let native = page.entity();
                    if native.update(cx, |input, cx| {
                        gpui_kit::EntityInputHandler::marked_text_range(input, window, cx).is_some()
                    }) {
                        return;
                    }
                }
                if self.model.is_open()
                    && self.model.contains_switchable(
                        self.model.snapshot_generation().unwrap_or_default(),
                        id,
                    )
                {
                    self.activate(id, cx);
                }
            }
            Action::Previous => self.previous(&PreviousBranch, window, cx),
            Action::Next => self.next(&NextBranch, window, cx),
            Action::Close => {
                self.request_close(cx);
                window.focus(&self.focus, cx);
            }
            Action::Reload => {
                if popup {
                    self.presentation.popup = None;
                } else {
                    self.presentation.trigger = None;
                    self.presentation.popup = None;
                }
                if let Ok(mut draft) = self.presentation.draft.0.lock() {
                    draft.page = None;
                    draft.composing = false;
                }
                self.presentation.input_focused = false;
                self.presentation.measured = None;
                self.presentation.revision = ProjectionId::new();
                cx.notify();
            }
        }
    }

    pub(super) fn poll_presentation(&mut self, cx: &mut Context<Self>) {
        let detached = self
            .presentation
            .trigger
            .as_ref()
            .is_some_and(|page| !page.live());
        if detached {
            self.presentation.close_popup();
            if let Ok(mut draft) = self.presentation.draft.0.lock() {
                draft.page = None;
                draft.composing = false;
            }
            if self.model.is_open() {
                self.request_close(cx);
            }
        }
        let revision = self
            .presentation
            .draft
            .0
            .lock()
            .ok()
            .map(|draft| draft.revision);
        if revision.is_some_and(|revision| revision != self.presentation.draft_revision) {
            cx.notify();
        }
    }

    fn host_key(
        &mut self,
        popup: bool,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let popup = popup || self.model.is_open();
        let page = if popup {
            &self.presentation.popup
        } else {
            &self.presentation.trigger
        };
        let Some(page) = page else {
            return;
        };
        if !page.live() {
            return;
        }
        let Some(observed) = page.displayed.clone() else {
            return;
        };
        if !self.accept(&observed, popup, cx) {
            return;
        }
        if popup && let Some(page) = &self.presentation.popup {
            let native = page.entity();
            let composing = native.update(cx, |input, cx| {
                gpui_kit::EntityInputHandler::marked_text_range(input, window, cx).is_some()
            });
            if composing {
                return;
            }
        }
        let modifiers = event.keystroke.modifiers;
        if modifiers.control
            || modifiers.alt
            || modifiers.platform
            || modifiers.function
            || (event.prefer_character_input && !popup)
        {
            return;
        }
        if self
            .presentation
            .draft
            .interact(popup.then(|| self.model.filter()), None, None, || ())
            .is_none()
        {
            return;
        }
        match event.keystroke.key.as_str() {
            "escape" if popup => {
                self.perform(Action::Close, true, window, cx);
                cx.stop_propagation();
            }
            "tab" if popup => {
                if self.presentation.input_focused && modifiers.shift {
                    self.presentation.input_focused = false;
                    window.focus(&self.focus, cx);
                } else if !self.presentation.input_focused && !modifiers.shift {
                    self.presentation.input_focused = true;
                } else {
                    self.request_close(cx);
                    crate::nsh::keyboard::leave(&self.focus, modifiers.shift, window, cx);
                }
                cx.notify();
                cx.stop_propagation();
            }
            "tab" => {
                crate::nsh::keyboard::leave(&self.focus, modifiers.shift, window, cx);
                cx.stop_propagation();
            }
            "up" | "down" if popup => {
                self.presentation.input_focused = false;
                window.focus(&self.focus, cx);
                self.perform(
                    if event.keystroke.key == "up" {
                        Action::Previous
                    } else {
                        Action::Next
                    },
                    true,
                    window,
                    cx,
                );
                cx.stop_propagation();
            }
            "enter" | "space" | " "
                if !event.is_held
                    && (event.keystroke.key == "enter" || !self.presentation.input_focused) =>
            {
                let action = if observed.failure {
                    Action::Reload
                } else if popup {
                    let Some(id) = observed.focused else {
                        return;
                    };
                    Action::Activate(id)
                } else {
                    Action::Toggle
                };
                self.perform(action, popup, window, cx);
                cx.stop_propagation();
            }
            _ => {}
        }
    }

    fn host_action(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let event = KeyDownEvent {
            keystroke: gpui_kit::Keystroke {
                key: key.into(),
                ..Default::default()
            },
            is_held: false,
            prefer_character_input: false,
        };
        self.host_key(false, &event, window, cx);
    }

    pub(super) fn render_nsh(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if self.presentation.subscriptions.is_empty() {
            self.focus = self.focus.clone().tab_stop(true);
            self.presentation.subscriptions.push(cx.on_focus_in(
                &self.focus.clone(),
                window,
                |this, window, cx| {
                    this.presentation.keyboard = true;
                    if !this.presentation.input_focused && this.focus.contains_focused(window, cx) {
                        window.focus(&this.focus, cx);
                    }
                    cx.notify();
                },
            ));
            self.presentation.subscriptions.push(cx.on_focus_out(
                &self.focus.clone(),
                window,
                |this, _, _, cx| {
                    this.presentation.keyboard = false;
                    cx.notify();
                },
            ));
        }
        let non_git = matches!(
            self.current_head.state,
            Some(vega_conversation::types::ProjectBranchState::NonGit)
        );
        if non_git {
            self.presentation.trigger = None;
            self.presentation.close_popup();
            return div().hidden().into_any_element();
        }
        self.refresh_projection(cx);
        if self
            .presentation
            .trigger
            .as_ref()
            .is_some_and(|page| page.lease.upgrade().is_none())
        {
            self.presentation.trigger = None;
            self.presentation.close_popup();
            if self.model.is_open() {
                self.request_close(cx);
            }
        }
        if self.presentation.trigger.is_none() {
            self.presentation.trigger = Some(self.create_page(false, window, cx));
        }
        if !self.model.is_open() {
            self.presentation.close_popup();
        } else if self.presentation.popup.is_none() {
            self.presentation.popup = Some(self.create_page(true, window, cx));
        }
        for popup in [false, true] {
            let slot = if popup {
                &mut self.presentation.popup
            } else {
                &mut self.presentation.trigger
            };
            let Some(mut page) = slot.take() else {
                continue;
            };
            let snapshot = self.snapshot(page.id.clone(), page.failed, cx);
            page.surface.sync_environment(cx);
            if !page.shown.same_render(&snapshot) {
                page.surface.publish(snapshot.clone());
            }
            if page.failed && page.recovery.is_none() {
                let create = if popup {
                    Surface::new
                } else {
                    Surface::new_transparent
                };
                let (recovery, requests) = create(snapshot.clone(), nsh_view::trigger, window, cx);
                self.watch_requests(requests, window, cx);
                page.recovery = Some(recovery);
            }
            if let Some(recovery) = &mut page.recovery {
                recovery.sync_environment(cx);
                if !page.shown.same_render(&snapshot) {
                    recovery.publish(snapshot.clone());
                }
            }
            page.shown = snapshot;
            if popup {
                self.presentation.popup = Some(page);
            } else {
                self.presentation.trigger = Some(page);
            }
        }
        let Some(trigger) = self.presentation.trigger.as_mut() else {
            return div().into_any_element();
        };
        let lease = trigger.lease.upgrade().unwrap_or_else(|| Rc::new(()));
        trigger.lease = Rc::downgrade(&lease);
        let trigger_geometry = trigger.geometry.clone();
        let outside_trigger_geometry = trigger_geometry.clone();
        let trigger_view = trigger.entity();
        trigger_view.read(cx).focus_handle(cx).tab_stop(false);
        let trigger_observed = trigger.shown.clone();
        let height = if self.chip_chrome {
            Layout::COMPOSER_UTILITY_CHIP_HEIGHT
        } else {
            BRANCH_ROW_HEIGHT
        };
        let label: gpui_kit::SharedString = self.current_head_label().to_owned().into();
        let style = window.text_style();
        let run = style.to_run(label.len());
        let label_width = f32::from(
            window
                .text_system()
                .shape_line(label, px(Typography::SIDEBAR), &[run], None)
                .width,
        );
        let width = (label_width
            + if self.chip_chrome {
                Layout::ICON_SIZE + 8. + Layout::COMPOSER_UTILITY_CHIP_PADDING_X * 2.
            } else {
                16.
            })
        .min(180.);
        let weak = cx.weak_entity();
        let trigger_host = div()
            .id("branch-selector-trigger-host")
            .key_context("BranchSelector")
            .capture_any_mouse_down(cx.listener(|this, _, window, cx| {
                this.presentation.keyboard = false;
                window.focus(&this.focus, cx);
            }))
            .w(px(width))
            .h(px(height))
            .child(trigger_view);
        let mut root =
            div()
                .id("branch-selector-focus-host")
                .role(gpui_kit::Role::Button)
                .aria_label(self.current_head_label().to_owned())
                .aria_expanded(self.model.is_open())
                .on_action(cx.listener(|this, _: &ActivateBranch, window, cx| {
                    this.host_action("enter", window, cx)
                }))
                .on_action(cx.listener(|this, _: &PreviousBranch, window, cx| {
                    this.host_action("up", window, cx)
                }))
                .on_action(cx.listener(|this, _: &NextBranch, window, cx| {
                    this.host_action("down", window, cx)
                }))
                .on_action(cx.listener(|this, _: &CloseBranchSelector, window, cx| {
                    this.host_action("escape", window, cx)
                }))
                .on_a11y_action(gpui_kit::AccessibleAction::Click, move |_, window, cx| {
                    let _ = weak.update(cx, |this, cx| {
                        if this.accept(&trigger_observed, false, cx) {
                            this.perform(Action::Toggle, false, window, cx);
                        }
                    });
                })
                .key_context("BranchSelector")
                .capture_key_down(
                    cx.listener(|this, event, window, cx| this.host_key(false, event, window, cx)),
                )
                .relative()
                .track_focus(&self.focus)
                .min_w_0()
                .child(Mounted {
                    child: trigger_host.into_any_element(),
                    lease,
                    geometry: trigger_geometry,
                });
        let proxy = self
            .presentation
            .measured
            .as_ref()
            .and_then(|(shown, id, revision)| {
                let (rect, lease) = self.presentation.row_geometry.bounds(revision)?;
                (self.model.visual_focused == Some(*id)
                    && self.accept(shown, true, cx)
                    && !self.disabled
                    && !self.model.is_pending()
                    && !self.presentation.input_focused)
                    .then(|| {
                        shown
                            .rows
                            .iter()
                            .find(|row| row.id == *id && !row.current)
                            .map(|branch| {
                                (
                                    shown.clone(),
                                    *id,
                                    rect,
                                    branch.label.clone(),
                                    lease,
                                    revision.clone(),
                                )
                            })
                    })
                    .flatten()
            });
        if let Some(popup) = &mut self.presentation.popup {
            let popup_lease = popup.lease.upgrade().unwrap_or_else(|| Rc::new(()));
            popup.lease = Rc::downgrade(&popup_lease);
            let popup_geometry = popup.geometry.clone();
            let view = popup.entity();
            view.read(cx).focus_handle(cx).tab_stop(false);
            let banner = usize::from(
                popup.shown.total > 0 && matches!(popup.shown.status, SelectorStatus::Failed(_)),
            );
            let popup_width = px(320.).min((window.viewport_size().width - px(16.)).max(px(1.)));
            let popup_height = px((popup.shown.rows.len().max(1) + banner) as f32
                * BRANCH_ROW_HEIGHT
                + BRANCH_CHROME_HEIGHT)
            .min(px(240.))
            .min((window.viewport_size().height - px(16.)).max(px(1.)));
            let observed = popup.shown.clone();
            let mut popup_host = div()
                .id("branch-selector-popup-host")
                .occlude()
                .w(popup_width)
                .h(popup_height)
                .on_mouse_down_out(cx.listener(
                    move |this, event: &gpui_kit::MouseDownEvent, _, cx| {
                        if outside_trigger_geometry
                            .borrow()
                            .bounds
                            .is_some_and(|bounds| bounds.contains(&event.position))
                        {
                            return;
                        }
                        if this.accept(&observed, true, cx) {
                            this.request_close(cx);
                        }
                    },
                ))
                .on_mouse_down(gpui_kit::MouseButton::Left, |_, _, cx| {
                    cx.stop_propagation()
                })
                .child(view);
            if let Some((observed, id, rect, label, row_lease, geometry_revision)) = proxy {
                let weak = cx.weak_entity();
                popup_host = popup_host.child(
                    div()
                        .id("branch-selector-active-row")
                        .absolute()
                        .top(px(rect[1]))
                        .left(px(rect[0]))
                        .w(px(rect[2]))
                        .h(px(rect[3]))
                        .role(gpui_kit::Role::MenuItem)
                        .aria_label(label)
                        .aria_active_descendant()
                        .on_a11y_action(gpui_kit::AccessibleAction::Click, move |_, window, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                if this.presentation.input_focused
                                    || this.model.visual_focused != Some(id)
                                    || this
                                        .presentation
                                        .row_geometry
                                        .bounds(&geometry_revision)
                                        .is_none()
                                {
                                    return;
                                }
                                this.guarded_action(
                                    observed.clone(),
                                    Action::Activate(id),
                                    true,
                                    (Some(row_lease.clone()), Some(observed.draft_revision)),
                                    window,
                                    cx,
                                );
                            });
                        }),
                );
            }
            root = root.child(
                div().absolute().top_0().left_0().size_full().child(
                    anchored()
                        .anchor(if self.menu_below {
                            Anchor::TopLeft
                        } else {
                            Anchor::BottomLeft
                        })
                        .position_mode(AnchoredPositionMode::Local)
                        .position(point(
                            px(0.),
                            px(if self.menu_below {
                                BRANCH_ROW_HEIGHT + 4.
                            } else {
                                -4.
                            }),
                        ))
                        .snap_to_window_with_margin(px(8.))
                        .child(
                            gpui_kit::deferred(Mounted {
                                child: popup_host.into_any_element(),
                                lease: popup_lease,
                                geometry: popup_geometry,
                            })
                            .with_priority(2),
                        ),
                ),
            );
        }
        root.into_any_element()
    }
}

struct Mounted {
    child: AnyElement,
    lease: Rc<()>,
    geometry: Rc<RefCell<Geometry>>,
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
        Some("branch-selector-mounted".into())
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
        if let Some(id) = id {
            window.with_element_state::<Rc<()>, _>(id, |_, _| ((), self.lease.clone()));
        }
        let clip = window.content_mask().bounds;
        let visible = bounds.intersect(&clip);
        *self.geometry.borrow_mut() = Geometry {
            visible: visible.size.width > px(0.) && visible.size.height > px(0.),
            bounds: Some(visible),
        };
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
