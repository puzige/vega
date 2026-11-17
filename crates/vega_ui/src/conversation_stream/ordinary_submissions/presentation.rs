use super::{
    AttemptId, CommitFact, ConversationStream, OrdinaryAction, OrdinaryActionRequested,
    OrdinaryControls, OrdinaryStatus, PrimaryFact, RetryEligibility, SubmissionFailure,
    SubmissionId,
};
use crate::conversation_stream::ActivatePendingMessageControl;
use crate::conversation_stream::model::StreamEntryIdentity;
use crate::nsh::{ProjectionId, Surface};
use gpui_kit::{prelude::*, *};
use std::cell::Cell;
use std::rc::{Rc, Weak};
use vega_conversation::types::{CleanupFact, OrdinarySubmission};
use vega_theme::{ThemeColors, Typography, theme};

mod pixels;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Submission(OrdinaryAction),
    Diagnostics,
}

#[derive(Clone, PartialEq, Eq)]
struct Facts {
    latest: AttemptId,
    state: OrdinaryStatus,
    commit: CommitFact,
    primary: PrimaryFact,
    cleanup: CleanupFact,
    workers: usize,
    sealed: bool,
    abnormal: bool,
    history_changed: bool,
    failure: Option<SubmissionFailure>,
    retry: RetryEligibility,
    unavailable: bool,
    retired: bool,
}

impl Facts {
    fn capture(snapshot: &vega_conversation::types::SubmissionSnapshot) -> Option<Self> {
        let latest = snapshot.attempts.last()?;
        Some(Self {
            latest: latest.id,
            state: OrdinaryStatus::from_attempt(snapshot, latest),
            commit: latest.commit.clone(),
            primary: latest.primary,
            cleanup: latest.cleanup,
            workers: latest.workers,
            sealed: latest.sealed,
            abnormal: latest.abnormal,
            history_changed: latest.history_changed,
            failure: latest.failure,
            retry: snapshot.retry,
            unavailable: snapshot.unavailable,
            retired: snapshot.retired,
        })
    }
}

#[derive(Clone)]
struct Source {
    epoch: ProjectionId,
    thread: String,
    index: usize,
    identity: StreamEntryIdentity,
    submission: SubmissionId,
    attempt: AttemptId,
    state: OrdinaryStatus,
    checked: Option<&'static str>,
    controls: Option<OrdinaryControls>,
    diagnostics: bool,
    facts: Facts,
}

impl Source {
    fn capture(stream: &ConversationStream, index: usize, cx: &App) -> Option<Self> {
        let (row, controls, diagnostics) = stream.ordinary_projection(index, cx)?;
        let snapshot = row.submission.snapshot();
        Some(Self {
            epoch: stream.run_notice_epoch.clone(),
            thread: stream.thread.id.clone(),
            index,
            identity: stream.entry_identities.get(index)?.clone(),
            submission: row.submission.id(),
            attempt: row.attempt,
            state: row.state,
            checked: row.checked,
            controls,
            diagnostics,
            facts: Facts::capture(&snapshot)?,
        })
    }

    fn matches(&self, other: &Self) -> bool {
        self.epoch.matches(&other.epoch)
            && self.thread == other.thread
            && self.index == other.index
            && self.identity == other.identity
            && self.submission == other.submission
            && self.attempt == other.attempt
            && self.state == other.state
            && self.checked == other.checked
            && self.controls == other.controls
            && self.diagnostics == other.diagnostics
            && self.facts == other.facts
    }

    fn current(&self, stream: &ConversationStream, cx: &App) -> bool {
        self.attempt == self.facts.latest
            && self.state == self.facts.state
            && !stream.ui_presentations_suspended
            && !self.facts.retired
            && !stream.reading_preview
            && !stream.process_entry_hidden(self.index, cx)
            && cx
                .try_global::<crate::settings::SettingsOpen>()
                .is_some_and(|settings| !settings.0)
            && cx
                .try_global::<crate::sidebar::OpenedThread>()
                .is_some_and(|route| {
                    route
                        .0
                        .as_ref()
                        .is_some_and(|thread| thread.id == self.thread)
                })
            && Self::capture(stream, self.index, cx).is_some_and(|other| self.matches(&other))
    }
}

#[derive(Clone)]
struct Snapshot {
    id: ProjectionId,
    source: Source,
    message: Option<String>,
    detail: Option<String>,
    buttons: Vec<(Action, String)>,
    colors: ThemeColors,
    family: String,
    appearance: vega_theme::Appearance,
    language: &'static str,
    selected: Option<Action>,
    geometry: Option<Geometry>,
    width: f32,
    enabled: bool,
}

impl Snapshot {
    fn project(source: Source, stream: &ConversationStream, family: String, cx: &App) -> Self {
        Self::content(
            source.clone(),
            source.current(stream, cx),
            family,
            theme(cx).colors,
            theme(cx).appearance,
        )
    }

    fn content(
        source: Source,
        enabled: bool,
        family: String,
        colors: ThemeColors,
        appearance: vega_theme::Appearance,
    ) -> Self {
        let mut buttons = Vec::new();
        if let Some((label, action)) = source
            .controls
            .as_ref()
            .and_then(|controls| controls.action)
        {
            buttons.push((Action::Submission(action), vega_i18n::t(label).into()));
        }
        if source.diagnostics {
            buttons.push((
                Action::Diagnostics,
                vega_i18n::t("ui.ordinary_send.copy_diagnostics").into(),
            ));
        }
        Self {
            id: ProjectionId::new(),
            message: source
                .controls
                .as_ref()
                .map(|controls| vega_i18n::t(controls.message).into()),
            detail: source
                .controls
                .as_ref()
                .and_then(|controls| controls.detail)
                .map(|detail| vega_i18n::t(detail).into()),
            enabled,
            source,
            buttons,
            colors,
            family,
            appearance,
            language: vega_i18n::language(),
            selected: None,
            geometry: None,
            width: 0.,
        }
    }

    fn same(&self, other: &Self) -> bool {
        self.source.matches(&other.source)
            && self.message == other.message
            && self.detail == other.detail
            && self.buttons == other.buttons
            && self.family == other.family
            && self.appearance == other.appearance
            && self.language == other.language
            && self.selected == other.selected
            && self.geometry == other.geometry
            && self.width == other.width
            && self.enabled == other.enabled
    }

    fn accepts(&self, observed: &Self, action: Action, displayed: bool) -> bool {
        displayed
            && self.id.matches(&observed.id)
            && self.same(observed)
            && observed.enabled
            && observed
                .buttons
                .iter()
                .any(|(available, _)| *available == action)
    }
}

enum Request {
    Displayed(Box<Snapshot>),
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

#[derive(Clone, Copy, PartialEq)]
struct Geometry {
    bounds: Bounds<Pixels>,
    visible: Bounds<Pixels>,
    viewport: Bounds<Pixels>,
    row: Bounds<Pixels>,
    foreground: Pixels,
}

#[derive(Default)]
struct Mount {
    geometry: Cell<Option<Geometry>>,
}

#[derive(gpui_kit::IntoElement)]
pub(super) struct ControlsRow {
    pub(super) owner: WeakEntity<ConversationStream>,
    pub(super) index: usize,
}

impl RenderOnce for ControlsRow {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Some(owner) = self.owner.upgrade() else {
            return div().into_any_element();
        };
        let stream = owner.read(cx);
        let Some(source) = Source::capture(stream, self.index, cx) else {
            return div().into_any_element();
        };
        let Some((row, _, _)) = stream.ordinary_projection(self.index, cx) else {
            return div().into_any_element();
        };
        let snapshot = Snapshot::project(
            source,
            stream,
            window.text_style().font_family.to_string(),
            cx,
        );
        let focus = row.focus.clone();
        let diagnostic_focus = row.diagnostic_focus.clone();
        let submission = row.submission.clone();
        let key = format!(
            "nsh-ordinary-controls-{}-{}",
            owner.entity_id(),
            snapshot.source.identity.key
        );
        let state = window.use_keyed_state(key, cx, |window, cx| {
            let subscriptions = vec![
                cx.on_release(|this: &mut Host, cx| this.stop(cx)),
                cx.on_focus_in(&focus, window, |_, _, cx| cx.notify()),
                cx.on_focus_out(&focus, window, |_, _, _, cx| cx.notify()),
                cx.on_focus_in(&diagnostic_focus, window, |_, _, cx| cx.notify()),
                cx.on_focus_out(&diagnostic_focus, window, |_, _, _, cx| cx.notify()),
            ];
            Host {
                owner: self.owner.clone(),
                snapshot: snapshot.clone(),
                submission: submission.clone(),
                surface: None,
                focus: focus.clone(),
                diagnostic_focus: diagnostic_focus.clone(),
                lease: Weak::new(),
                displayed: false,
                height: None,
                _subscriptions: subscriptions,
            }
        });
        state.update(cx, |this, cx| {
            let mut incoming = snapshot;
            incoming.width = this.snapshot.width;
            incoming.geometry = this.snapshot.geometry;
            incoming.selected = this.snapshot.selected;
            if !this.snapshot.same(&incoming) {
                this.stop(cx);
                incoming.geometry = None;
            } else {
                incoming.id = this.snapshot.id.clone();
            }
            this.snapshot = incoming;
            this.submission = submission;
            this.focus = focus;
            this.diagnostic_focus = diagnostic_focus;
        });
        state.into_any_element()
    }
}

struct Host {
    owner: WeakEntity<ConversationStream>,
    snapshot: Snapshot,
    submission: OrdinarySubmission,
    surface: Option<Surface<Snapshot, Request>>,
    focus: FocusHandle,
    diagnostic_focus: FocusHandle,
    lease: Weak<Mount>,
    displayed: bool,
    height: Option<f32>,
    _subscriptions: Vec<Subscription>,
}

impl Host {
    fn stop(&mut self, cx: &App) {
        if let Some(surface) = self.surface.take() {
            surface.entity().read(cx).stop();
        }
        self.snapshot.id = ProjectionId::new();
        self.snapshot.geometry = None;
        self.displayed = false;
        self.height = None;
        self.lease = Weak::new();
    }

    fn focus_for(&self, action: Action) -> &FocusHandle {
        match action {
            Action::Submission(_) => &self.focus,
            Action::Diagnostics => &self.diagnostic_focus,
        }
    }

    fn valid(&self, observed: &Snapshot, window: &Window, cx: &App) -> bool {
        let Some(owner) = self.owner.upgrade() else {
            return false;
        };
        let Some(geometry) = self.lease.upgrade().and_then(|mount| mount.geometry.get()) else {
            return false;
        };
        let stream = owner.read(cx);
        self.snapshot.id.matches(&observed.id)
            && self.snapshot.same(observed)
            && observed.geometry == Some(geometry)
            && observed.source.current(stream, cx)
            && self.submission.id() == observed.source.submission
            && !geometry.visible.is_empty()
            && geometry.viewport == stream.list.viewport_bounds()
            && geometry.foreground == stream.foreground_height
            && stream.list.bounds_for_item(observed.source.index) == Some(geometry.row)
            && geometry.visible.intersect(&geometry.viewport) == geometry.visible
            && window.is_window_active()
            && self
                .surface
                .as_ref()
                .is_some_and(|surface| surface.errors().borrow().is_none())
    }

    fn activate(
        &mut self,
        observed: &Snapshot,
        action: Action,
        keyboard: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.snapshot.accepts(observed, action, self.displayed)
            || !self.valid(observed, window, cx)
            || (keyboard && !self.focus_for(action).is_focused(window))
        {
            return;
        }
        let Some(owner) = self.owner.upgrade() else {
            return;
        };
        let accepted = owner.update(cx, |stream, cx| {
            if !observed.source.current(stream, cx) {
                return false;
            }
            match action {
                Action::Submission(action) => cx.emit(OrdinaryActionRequested {
                    thread_id: observed.source.thread.clone(),
                    submission: observed.source.submission,
                    attempt: observed.source.attempt,
                    action,
                }),
                Action::Diagnostics => {
                    if let Some(text) = self
                        .submission
                        .preparation_diagnostics(observed.source.attempt)
                    {
                        cx.write_to_clipboard(ClipboardItem::new_string(text));
                    }
                }
            }
            true
        });
        if accepted {
            window.focus(self.focus_for(action), cx);
            cx.notify();
        }
    }

    fn mount(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.surface.is_some() || self.snapshot.width <= 0. {
            return;
        }
        let (surface, mut requests) =
            Surface::new_transparent(self.snapshot.clone(), pixels::render, window, cx);
        surface.focus_handle(cx).tab_stop(false);
        cx.spawn_in(window, async move |this, cx| {
            while let Some(request) = requests.recv().await {
                if this
                    .update_in(cx, |this, window, cx| match request {
                        Request::Displayed(observed) => {
                            if this.valid(&observed, window, cx) {
                                this.displayed = true;
                            }
                        }
                        Request::Activate { observed, action } => {
                            this.activate(&observed, action, false, window, cx)
                        }
                        Request::Layout { id, width, height } => {
                            if this.snapshot.id.matches(&id)
                                && width == this.snapshot.width
                                && width.is_finite()
                                && width > 0.
                                && height.is_finite()
                                && height > 0.
                                && this.lease.upgrade().is_some()
                                && this
                                    .height
                                    .is_none_or(|previous| (previous - height).abs() > 0.5)
                            {
                                this.height = Some(height);
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
        self.surface = Some(surface);
    }
}

impl Render for Host {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(owner) = self.owner.upgrade() else {
            self.stop(cx);
            return div().into_any_element();
        };
        let source_current = self.snapshot.source.current(owner.read(cx), cx);
        let lease = self.lease.upgrade().unwrap_or_else(|| {
            self.stop(cx);
            Rc::new(Mount::default())
        });
        self.lease = Rc::downgrade(&lease);
        let selected = self.snapshot.buttons.iter().find_map(|(action, _)| {
            self.focus_for(*action)
                .is_focused(window)
                .then_some(*action)
        });
        let mut incoming = self.snapshot.clone();
        incoming.enabled = source_current;
        incoming.selected = selected;
        incoming.geometry = lease.geometry.get();
        if !self.snapshot.same(&incoming) {
            incoming.id = ProjectionId::new();
            self.displayed = false;
        }
        self.snapshot = incoming;
        self.mount(window, cx);
        if let Some(surface) = &mut self.surface {
            surface.sync_environment(cx);
            surface.publish(self.snapshot.clone());
        }
        let mut host = div()
            .id("nsh-ordinary-controls")
            .relative()
            .w_full()
            .min_w_0()
            .flex_shrink_0()
            .h(px(self.height.unwrap_or(
                Typography::BODY * Typography::BODY_LINE_HEIGHT + 8.,
            )))
            .role(Role::Group)
            .children(self.surface.as_ref().map(Surface::entity));
        for (action, label) in self.snapshot.buttons.clone() {
            let observed = self.snapshot.clone();
            let accessible_observed = observed.clone();
            let weak = cx.weak_entity();
            host = host.child(
                div()
                    .id(match action {
                        Action::Submission(_) => "ordinary-action",
                        Action::Diagnostics => "ordinary-diagnostics",
                    })
                    .absolute()
                    .inset_0()
                    .role(Role::Button)
                    .aria_label(label)
                    .key_context("PendingMessageControl")
                    .track_focus(self.focus_for(action))
                    .tab_stop(true)
                    .on_action(cx.listener(
                        move |this, _: &ActivatePendingMessageControl, window, cx| {
                            cx.stop_propagation();
                            this.activate(&observed, action, true, window, cx);
                        },
                    ))
                    .on_a11y_action(AccessibleAction::Click, move |_, window, cx| {
                        let _ = weak.update(cx, |this, cx| {
                            this.activate(&accessible_observed, action, false, window, cx)
                        });
                    }),
            );
        }
        Mounted {
            child: host.into_any_element(),
            lease,
            host: cx.weak_entity(),
            owner: self.owner.clone(),
            index: self.snapshot.source.index,
        }
        .into_any_element()
    }
}

struct Mounted {
    child: AnyElement,
    lease: Rc<Mount>,
    host: WeakEntity<Host>,
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
    type PrepaintState = (Option<Geometry>, Bounds<Pixels>, Bounds<Pixels>);

    fn id(&self) -> Option<ElementId> {
        Some("ordinary-controls-mounted".into())
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
    ) -> Self::PrepaintState {
        let previous = self.lease.geometry.replace(None);
        let mask = window.content_mask().bounds;
        if let Some(id) = id {
            window.with_element_state::<Rc<Mount>, _>(id, |_, _| ((), self.lease.clone()));
        }
        let width = f32::from(bounds.size.width);
        if self
            .host
            .upgrade()
            .is_some_and(|host| host.read(cx).snapshot.width != width)
        {
            let _ = self.host.update(cx, |host, cx| {
                if width.is_finite() && width > 0. && host.snapshot.width != width {
                    host.snapshot.width = width;
                    host.snapshot.id = ProjectionId::new();
                    host.displayed = false;
                    host.height = None;
                }
                cx.notify();
            });
        }
        self.child.prepaint(window, cx);
        (previous, bounds, mask)
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        state: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let (previous, bounds, mask) = *state;
        if id.is_some()
            && let Some(owner) = self.owner.upgrade()
        {
            let stream = owner.read(cx);
            let viewport = stream.list.viewport_bounds();
            if let Some(row) = stream.list.bounds_for_item(self.index) {
                let mut reading = viewport;
                reading.size.height = (reading.size.height - stream.foreground_height).max(px(0.));
                let visible = bounds.intersect(&mask).intersect(&reading);
                if !visible.is_empty() {
                    self.lease.geometry.set(Some(Geometry {
                        bounds,
                        visible,
                        viewport,
                        row,
                        foreground: stream.foreground_height,
                    }));
                }
            }
        }
        if previous != self.lease.geometry.get() {
            let _ = self.host.update(cx, |_, cx| cx.notify());
        }
        self.child.paint(window, cx);
    }
}
