use super::draft::Draft;
use super::nsh_view::{self, Action, Request, Row, Snapshot};
use super::*;
use crate::nsh::{ProjectionId, Surface};
use gpui_kit::{
    AnyElement, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, KeyDownEvent,
    LayoutId, Pixels, div, px,
};
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

struct Page {
    surface: Surface<Snapshot, Request>,
    recovery: Option<Surface<Snapshot, Request>>,
    id: ProjectionId,
    lease: Weak<()>,
    visible: Arc<AtomicBool>,
    shown: Snapshot,
    displayed: Option<Snapshot>,
    failed: bool,
    measured: Option<(Snapshot, [f32; 4], super::measurement::MeasurementLease)>,
    language: &'static str,
    appearance: vega_theme::Appearance,
}

impl Page {
    fn live(&self) -> bool {
        self.lease.upgrade().is_some() && self.visible.load(Ordering::Acquire)
    }
}

pub(super) struct State {
    page: Option<Page>,
    pub draft: Draft,
    carrier: String,
    focus_revision: u64,
    rows: Arc<[Row]>,
    row_authority: Option<(CommitChecklist, HashSet<WorkspaceFileId>)>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            page: None,
            draft: Draft::default(),
            carrier: String::new(),
            focus_revision: 0,
            rows: Arc::from([]),
            row_authority: None,
        }
    }
}

impl State {
    pub fn unmount(&mut self) {
        if let Some(page) = self.page.take() {
            page.visible.store(false, Ordering::Release);
        }
        if let Ok(mut draft) = self.draft.0.lock() {
            draft.page = None;
            draft.mount = std::sync::Weak::new();
            draft.composing = false;
            draft.editable = false;
            draft.visible.store(false, Ordering::Release);
        }
    }
}

impl CommitPanel {
    pub(super) fn reconcile_carrier(&mut self, cx: &App) {
        let text = self.message.read(cx).text().to_string();
        if text == self.presentation.carrier {
            return;
        }
        self.presentation.carrier.clone_from(&text);
        if let Ok(mut value) = self.presentation.draft.0.lock() {
            value.replace(&text);
            self.editor_revision = value.revision;
            self.editor_revision_overflow = value.overflow;
        }
    }

    pub(super) fn update_carrier(&mut self, text: &str, cx: &mut Context<Self>) {
        self.presentation.carrier.clear();
        self.presentation.carrier.push_str(text);
        self.message.update(cx, |message, cx| {
            if message.text() != text {
                message.set_text(text, cx);
            }
        });
    }

    fn snapshot(&self, page: ProjectionId, failure: bool, cx: &App) -> Snapshot {
        let (draft_revision, draft_text, draft_overflow) = self
            .presentation
            .draft
            .0
            .lock()
            .map_or((self.editor_revision, String::new(), true), |draft| {
                (draft.revision, draft.text.clone(), draft.overflow)
            });
        Snapshot {
            page,
            thread: self.thread_id.clone(),
            project: self.project_id.clone(),
            stage: self.model.stage(),
            pending: self.model.pending,
            checklist: self.model.checklist.as_ref().map(|value| value.id),
            generation: self
                .model
                .checklist
                .as_ref()
                .map(|value| value.workspace_generation),
            prepared: self.model.prepared.clone(),
            selected: self.model.selected.clone(),
            disabled: self.disabled,
            focus: self.model.focus(),
            focus_revision: self.presentation.focus_revision,
            rows: self.presentation.rows.clone(),
            draft: self.presentation.draft.clone(),
            draft_revision,
            draft_text,
            draft_overflow,
            failure,
            colors: theme(cx).colors,
        }
    }

    fn accept(&self, observed: &Snapshot, cx: &App) -> bool {
        if cx
            .try_global::<crate::settings::SettingsOpen>()
            .is_some_and(|state| state.0)
            || cx
                .try_global::<crate::sidebar::PendingDeleteConfirm>()
                .is_some_and(|modal| modal.0.is_some())
        {
            return false;
        }
        self.presentation.page.as_ref().is_some_and(|page| {
            self.model.is_open()
                && page.live()
                && page.shown.same_authority(observed)
                && page
                    .displayed
                    .as_ref()
                    .is_some_and(|shown| shown.same_authority(observed))
                && self
                    .snapshot(page.id.clone(), page.failed, cx)
                    .same_authority(observed)
        })
    }

    fn create_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Page {
        let id = ProjectionId::new();
        if let Ok(mut value) = self.presentation.draft.0.lock() {
            value.page = Some(id.clone());
            value.composing = false;
        }
        let snapshot = self.snapshot(id.clone(), false, cx);
        let (surface, requests) = Surface::new(snapshot.clone(), nsh_view::panel, window, cx);
        self.watch_requests(requests, window, cx);
        let mut errors = surface.errors();
        let owner = id.clone();
        cx.spawn(async move |this, cx| {
            while errors.changed().await.is_ok() {
                if errors.borrow_and_update().is_some() {
                    let _ = this.update(cx, |this, cx| {
                        if let Some(page) = &mut this.presentation.page
                            && page.id.matches(&owner)
                        {
                            page.failed = true;
                            page.displayed = None;
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
            visible: Arc::new(AtomicBool::new(false)),
            shown: snapshot,
            displayed: None,
            failed: false,
            measured: None,
            language: vega_i18n::language(),
            appearance: theme(cx).appearance,
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

    fn receive(&mut self, request: Request, window: &mut Window, cx: &mut Context<Self>) {
        self.reconcile_carrier(cx);
        match request {
            Request::Displayed(observed) => {
                if let Some(page) = &mut self.presentation.page
                    && page.live()
                    && page.shown.same_authority(&observed)
                {
                    page.displayed = Some(observed);
                }
            }
            Request::Edit(owner) => {
                if !self
                    .presentation
                    .page
                    .as_ref()
                    .is_some_and(|page| page.live() && page.id.matches(&owner))
                {
                    return;
                }
                let shared = self.presentation.draft.clone();
                let Ok(value) = shared.0.lock() else {
                    return;
                };
                if !value.owns(&owner) {
                    return;
                }
                self.editor_revision = value.revision;
                self.editor_revision_overflow = value.overflow;
                let text = value.text.clone();
                drop(value);
                self.update_carrier(&text, cx);
                cx.notify();
            }
            Request::Focused(observed) => {
                if self.accept(&observed, cx) && observed.editable() {
                    self.model.focus = CommitPanelFocus::Draft;
                    cx.notify();
                }
            }
            Request::Measured(observed, rect, lease) => {
                if self.accept(&observed, cx)
                    && lease.live()
                    && let Some(page) = &mut self.presentation.page
                {
                    let rect =
                        rect.filter(|r| r.iter().all(|v| v.is_finite()) && r[2] > 0. && r[3] > 0.);
                    let changed = match (&page.measured, rect) {
                        (Some((before, old_rect, old_lease)), Some(rect)) => {
                            !before.same_authority(&observed)
                                || *old_rect != rect
                                || !old_lease.live()
                        }
                        (None, None) => false,
                        _ => true,
                    };
                    page.measured = rect.map(|rect| (observed, rect, lease));
                    if changed {
                        cx.notify();
                    }
                }
            }
            Request::Action(observed, action, row) => {
                let row_lease = match row {
                    Some(row) => match row.upgrade() {
                        Some(lease) => Some(lease),
                        None => return,
                    },
                    None => None,
                };
                if !self.accept(&observed, cx) {
                    return;
                }
                let _row_lease = row_lease;
                self.perform(observed, action, window, cx);
            }
        }
    }

    fn perform(
        &mut self,
        observed: Snapshot,
        action: Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            Action::Cancel => {
                self.request_close(cx);
            }
            Action::Recover => {
                self.presentation.unmount();
                self.model.focus = CommitPanelFocus::Cancel;
                window.focus(&self.cancel_focus, cx);
                cx.notify();
            }
            Action::Toggle(id) => {
                if observed.actionable() && observed.stage == CommitPanelStage::Checklist {
                    self.toggle_row(id, cx);
                }
            }
            Action::Generate | Action::Confirm => {
                if !observed.actionable() {
                    return;
                }
                let Some(page) = self.presentation.page.as_ref() else {
                    return;
                };
                let Some(displayed) = page.displayed.clone() else {
                    return;
                };
                let fresh = self.snapshot(page.id.clone(), page.failed, cx);
                if observed
                    .with_current_draft(&displayed, &fresh, |value| {
                        self.editor_revision = value.revision;
                        self.editor_revision_overflow = value.overflow;
                        if matches!(action, Action::Generate) {
                            self.draft_message(value.revision, value.overflow, cx);
                        } else if observed.can_confirm() {
                            self.confirm_message(value.text.clone(), cx);
                        }
                        value.editable = !self.disabled
                            && matches!(
                                self.model.stage(),
                                CommitPanelStage::CommitReady | CommitPanelStage::Drafting
                            );
                    })
                    .is_none()
                {
                    return;
                }
                window.focus(&self.cancel_focus, cx);
                cx.notify();
            }
        }
    }

    fn displayed(&self, cx: &App) -> Option<Snapshot> {
        let observed = self.presentation.page.as_ref()?.displayed.clone()?;
        self.accept(&observed, cx).then_some(observed)
    }

    fn move_host_focus(&mut self, backwards: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(observed) = self.displayed(cx) else {
            return;
        };
        let composing = observed
            .draft
            .0
            .lock()
            .map_or(true, |draft| draft.composing);
        if composing {
            return;
        }
        let moved = if observed.failure {
            let next = if backwards {
                CommitPanelFocus::Cancel
            } else {
                CommitPanelFocus::Confirm
            };
            let moved = self.model.focus != next;
            self.model.focus = next;
            moved
        } else {
            self.model.move_focus(backwards)
        };
        if !moved {
            crate::nsh::keyboard::leave(&self.cancel_focus, backwards, window, cx);
        } else {
            self.presentation.focus_revision = self.presentation.focus_revision.saturating_add(1);
            if self.model.focus == CommitPanelFocus::Draft {
                if let Some(page) = &self.presentation.page {
                    page.surface.focus(window, cx);
                }
            } else {
                window.focus(&self.cancel_focus, cx);
            }
        }
        cx.notify();
    }

    fn activate_host(
        &mut self,
        action: Option<Action>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.reconcile_carrier(cx);
        let Some(observed) = self.displayed(cx) else {
            return;
        };
        if !self.cancel_focus.is_focused(window) {
            return;
        }
        if action.is_none()
            && !self
                .presentation
                .page
                .as_ref()
                .and_then(|page| page.measured.as_ref())
                .is_some_and(|(shown, _, lease)| shown.same_authority(&observed) && lease.live())
        {
            return;
        }
        if observed.focus == CommitPanelFocus::Draft
            || self
                .presentation
                .draft
                .0
                .lock()
                .map_or(true, |draft| draft.composing)
        {
            return;
        }
        if matches!(action, Some(Action::Generate)) && observed.focus != CommitPanelFocus::Generate
        {
            return;
        }
        if matches!(action, Some(Action::Toggle(_)))
            && !matches!(observed.focus, CommitPanelFocus::Optional(_))
        {
            return;
        }
        let action = match action {
            Some(action) => action,
            None => match observed.focus {
                CommitPanelFocus::Cancel => Action::Cancel,
                CommitPanelFocus::Optional(index) => {
                    let Some(row) = observed.rows.iter().find(|row| row.optional == Some(index))
                    else {
                        return;
                    };
                    Action::Toggle(row.id)
                }
                CommitPanelFocus::Generate => Action::Generate,
                _ if observed.failure => Action::Recover,
                _ => Action::Confirm,
            },
        };
        self.perform(observed, action, window, cx);
    }

    fn host_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.cancel_focus.is_focused(window) && self.model.focus != CommitPanelFocus::Draft {
            return;
        }
        let modifiers = event.keystroke.modifiers;
        match event.keystroke.key.as_str() {
            "tab"
                if !modifiers.control
                    && !modifiers.alt
                    && !modifiers.platform
                    && !modifiers.function =>
            {
                self.move_host_focus(modifiers.shift, window, cx);
                cx.stop_propagation();
            }
            "enter" | "space" | " "
                if !event.is_held
                    && !event.prefer_character_input
                    && !modifiers.control
                    && !modifiers.alt
                    && !modifiers.platform
                    && !modifiers.shift
                    && self.model.focus != CommitPanelFocus::Draft =>
            {
                self.activate_host(None, window, cx);
                cx.stop_propagation();
            }
            "escape"
                if !event.is_held
                    && !modifiers.control
                    && !modifiers.alt
                    && !modifiers.platform
                    && self.model.focus != CommitPanelFocus::Draft =>
            {
                self.activate_host(Some(Action::Cancel), window, cx);
                cx.stop_propagation();
            }
            _ => {}
        }
    }

    pub(super) fn render_nsh(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        self.reconcile_carrier(cx);
        if !self.model.is_open() {
            self.presentation.unmount();
            return div().hidden().into_any_element();
        }
        let row_authority = self
            .model
            .checklist
            .clone()
            .map(|checklist| (checklist, self.model.selected.clone()));
        if self.presentation.row_authority != row_authority {
            self.presentation.rows = self.model.checklist.as_ref().map_or_else(
                || Arc::from([]),
                |checklist| {
                    checklist
                        .staged
                        .iter()
                        .map(|row| Row {
                            id: row.file_id,
                            label: row.label.clone(),
                            forced: true,
                            selected: true,
                            optional: None,
                        })
                        .chain(
                            checklist
                                .optional
                                .iter()
                                .enumerate()
                                .map(|(index, row)| Row {
                                    id: row.file_id,
                                    label: row.label.clone(),
                                    forced: false,
                                    selected: self.model.selected.contains(&row.file_id),
                                    optional: Some(index),
                                }),
                        )
                        .collect::<Vec<_>>()
                        .into()
                },
            );
            self.presentation.row_authority = row_authority;
        }
        if self
            .presentation
            .page
            .as_ref()
            .is_some_and(|page| !page.live())
        {
            self.presentation.unmount();
        }
        if self.presentation.page.is_none() {
            self.presentation.page = Some(self.create_page(window, cx));
        }
        if self.focus_cancel_pending {
            self.model.focus = CommitPanelFocus::Cancel;
            self.presentation.focus_revision = self.presentation.focus_revision.saturating_add(1);
            window.focus(&self.cancel_focus, cx);
            self.focus_cancel_pending = false;
        }
        let Some(page) = self.presentation.page.as_ref() else {
            return div().hidden().into_any_element();
        };
        let snapshot = self.snapshot(page.id.clone(), page.failed, cx);
        let panel_height = snapshot.panel_height();
        let needs_recovery = page.failed && page.recovery.is_none();
        if needs_recovery {
            let (surface, requests) =
                Surface::new(snapshot.clone(), nsh_view::recovery, window, cx);
            self.watch_requests(requests, window, cx);
            if let Some(page) = &mut self.presentation.page {
                page.recovery = Some(surface);
                page.displayed = None;
            }
        }
        let proxy = self.presentation.page.as_ref().and_then(|page| {
            let (observed, rect, lease) = page.measured.as_ref()?;
            (self.accept(observed, cx) && lease.live())
                .then(|| (observed.clone(), *rect, lease.clone()))
        });
        let Some(page) = &mut self.presentation.page else {
            return div().hidden().into_any_element();
        };
        let language = vega_i18n::language();
        let appearance = theme(cx).appearance;
        let changed = !page.shown.same_authority(&snapshot)
            || page.language != language
            || page.appearance != appearance;
        page.shown = snapshot.clone();
        page.language = language;
        page.appearance = appearance;
        if changed {
            page.surface.publish(snapshot.clone());
        }
        page.surface.sync_environment(cx);
        let entity = if let Some(recovery) = &mut page.recovery {
            if changed {
                recovery.publish(snapshot);
            }
            recovery.sync_environment(cx);
            recovery.entity()
        } else {
            page.surface.entity()
        };
        entity.read(cx).focus_handle(cx).tab_stop(false);
        let lease = Rc::new(());
        let mount = Arc::new(());
        page.lease = Rc::downgrade(&lease);
        let visible = page.visible.clone();
        if let Ok(mut draft) = self.presentation.draft.0.lock() {
            draft.mount = Arc::downgrade(&mount);
            draft.visible = visible.clone();
            draft.editable = !self.disabled
                && matches!(
                    self.model.stage(),
                    CommitPanelStage::CommitReady | CommitPanelStage::Drafting
                );
        }
        let mut root = div()
            .id("commit-panel-focus-host")
            .when(self.cancel_focus.is_focused(window), |host| {
                host.key_context("CommitPanel")
            })
            .track_focus(&self.cancel_focus)
            .capture_key_down(cx.listener(Self::host_key))
            .on_action(cx.listener(|this, _: &ConfirmCommitStage, window, cx| {
                this.activate_host(Some(Action::Confirm), window, cx)
            }))
            .on_action(cx.listener(|this, _: &RequestCommitDraft, window, cx| {
                this.activate_host(Some(Action::Generate), window, cx)
            }))
            .on_action(cx.listener(|this, _: &CloseCommitPanel, window, cx| {
                this.activate_host(Some(Action::Cancel), window, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleCommitSelection, window, cx| {
                if matches!(this.model.focus, CommitPanelFocus::Optional(_)) {
                    this.activate_host(None, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ActivateCommitEnter, window, cx| {
                this.activate_host(None, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ActivateCommitSpace, window, cx| {
                if matches!(
                    this.model.focus,
                    CommitPanelFocus::Generate | CommitPanelFocus::Optional(_)
                ) {
                    this.activate_host(None, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &NextCommitFocus, window, cx| {
                this.move_host_focus(false, window, cx)
            }))
            .on_action(cx.listener(|this, _: &PreviousCommitFocus, window, cx| {
                this.move_host_focus(true, window, cx)
            }))
            .absolute()
            .left_0()
            .right_0()
            .bottom_0()
            .h(px(panel_height))
            .max_h_full()
            .occlude()
            .child(entity);
        if let Some((observed, rect, lease)) = proxy
            && observed.focus != CommitPanelFocus::Draft
        {
            let label = match observed.focus {
                CommitPanelFocus::Cancel => vega_i18n::t("ui.cancel").to_owned(),
                CommitPanelFocus::Generate => vega_i18n::t("ui.generate_message").to_owned(),
                CommitPanelFocus::Optional(index) => observed
                    .rows
                    .iter()
                    .find(|row| row.optional == Some(index))
                    .map_or_else(String::new, |row| {
                        format!(
                            "{} {}",
                            super::commit_row_status(false, row.selected),
                            row.label
                        )
                    }),
                _ if observed.failure => vega_i18n::t("settings.action.reload").to_owned(),
                _ if observed.prepared.is_some() => vega_i18n::t("ui.commit").to_owned(),
                _ => vega_i18n::t("ui.prepare").to_owned(),
            };
            let weak = cx.weak_entity();
            root = root.child(
                div()
                    .id("commit-panel-focused-control")
                    .absolute()
                    .left(px(rect[0]))
                    .top(px(rect[1]))
                    .w(px(rect[2]))
                    .h(px(rect[3]))
                    .role(gpui_kit::Role::Button)
                    .aria_label(label)
                    .aria_active_descendant()
                    .on_a11y_action(gpui_kit::AccessibleAction::Click, move |_, window, cx| {
                        let _ = weak.update(cx, |this, cx| {
                            if lease.live() && this.accept(&observed, cx) {
                                this.activate_host(None, window, cx);
                            }
                        });
                    }),
            );
        }
        Mounted {
            child: root.into_any_element(),
            lease,
            mount,
            visible,
        }
        .into_any_element()
    }
}

struct Mounted {
    child: AnyElement,
    lease: Rc<()>,
    mount: Arc<()>,
    visible: Arc<AtomicBool>,
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
        Some("commit-panel-mounted".into())
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
            window.with_element_state::<(Rc<()>, Arc<()>), _>(id, |_, _| {
                ((), (self.lease.clone(), self.mount.clone()))
            });
        }
        let visible = bounds.intersect(&window.content_mask().bounds);
        let visible = visible.size.width > px(0.) && visible.size.height > px(0.);
        self.visible.store(visible, Ordering::Release);
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
