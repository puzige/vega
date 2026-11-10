use super::*;
use crate::nsh::{ProjectionId, Surface};
use gpui_kit::{
    AccessibleAction, Element, ElementId, GlobalElementId, InspectorElementId, KeyDownEvent,
    LayoutId, RenderOnce, Size, Subscription, WeakEntity,
};
use std::cell::Cell;
use std::rc::{Rc, Weak};

mod pixels;
pub(super) mod shell;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Menu,
    Stop,
    Primary,
    ResumePending,
    RunningSend,
    Trigger,
}

#[derive(Clone, PartialEq, Eq)]
struct Source {
    thread: Thread,
    input: gpui_kit::EntityId,
    revision: u64,
    generation: u64,
    control_generation: u64,
    menu: bool,
    slash: Option<String>,
    skill_query: Option<(std::ops::Range<usize>, String)>,
    projection_generation: u64,
    projection: Option<SkillComposerProjection>,
    models: Vec<ModelOption>,
    defaults: ComposerDefaults,
    pending_model: Option<(u64, String)>,
    model_owner: Option<u64>,
    next_model: Option<String>,
    active_agent: Option<(String, usize)>,
    active_sequence: Option<i64>,
    active_response: Option<String>,
    pending_mode: Option<(ThreadMode, String, String)>,
    pending_mode_revision: u64,
    state: Vec<bool>,
    attachments: u64,
    interrupted: Option<vega_conversation::types::InterruptedAssistant>,
    resume: bool,
    pending_behavior: PendingMessageBehavior,
    appearance: vega_theme::Appearance,
}

impl Source {
    fn capture(stream: &ConversationStream, cx: &App) -> Self {
        Self {
            thread: stream.thread.clone(),
            input: stream.input.entity_id(),
            revision: stream.input.read(cx).content_revision(),
            generation: stream.actions.menu_generation,
            control_generation: stream.actions.control_generation,
            menu: stream.actions.menu,
            slash: stream.actions.slash.clone(),
            skill_query: stream.actions.skill_query.clone(),
            projection_generation: stream.skill_projection_generation,
            projection: stream.skill_projection.clone(),
            models: stream.model_options.clone(),
            defaults: stream.composer_defaults.clone(),
            pending_model: stream.model_selection_pending.clone(),
            model_owner: stream.model_selection_save_owner,
            next_model: stream.next_turn_model.clone(),
            active_agent: stream.active_agent_message.clone(),
            active_sequence: stream.active_agent_sequence,
            active_response: stream.active_response_id.clone(),
            pending_mode: stream.actions.pending_mode.clone(),
            pending_mode_revision: stream.actions.pending_mode_revision,
            state: vec![
                stream.actions.running,
                stream.actions.stopping,
                stream.composer_submit_pending,
                stream.composer_resume_pending,
                stream.ordinary_pending_before_run(),
                stream.has_cancellable_queue_wait(),
                stream.permissions_revoked,
                stream.permission_change_pending(),
                stream.reading_preview,
                stream.draft_route,
                stream.input.read(cx).is_composing(),
                stream.input.read(cx).composer_editor_ready(),
                stream.skill_mutation_pending,
                stream.actions.skill_projection_requested,
                stream.trusted_action_busy,
                stream.approved_not_started,
                stream.attachment_import_pending,
                stream.model_selection_blocked(cx),
                stream.has_pending_model_selection(),
                stream.context_operation_busy(),
            ],
            attachments: stream.attachment_generation,
            interrupted: stream.interrupted_tail.clone(),
            resume: stream.shows_composer_resume(cx),
            pending_behavior: stream.running_message_behavior(cx),
            appearance: theme(cx).appearance,
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Row {
    action: Option<ComposerActionRow>,
    label: String,
    accessible: String,
    description: Option<String>,
    source: Option<String>,
    token: Option<&'static str>,
    reason: Option<&'static str>,
    checked: Option<bool>,
    enabled: bool,
}

#[derive(Clone)]
struct Snapshot {
    id: ProjectionId,
    epoch: ProjectionId,
    source: Source,
    kind: Kind,
    rows: Vec<Row>,
    query: Option<String>,
    status: Option<&'static str>,
    highlight: usize,
    navigation: u64,
    selected: Option<usize>,
    selection: ProjectionId,
    colors: ThemeColors,
    geometry: Option<Geometry>,
    activation: Option<Activation>,
}

impl Snapshot {
    fn capture(stream: &ConversationStream, kind: Kind, cx: &App) -> Self {
        let rows = match kind {
            Kind::Trigger => vec![Row {
                action: None,
                label: vega_i18n::t("ui.add_context").to_owned(),
                accessible: vega_i18n::t("ui.add_context").to_owned(),
                description: None,
                source: None,
                token: None,
                reason: None,
                checked: None,
                enabled: stream.context_menu_available(),
            }],
            Kind::RunningSend => {
                let label = vega_i18n::t(match stream.running_message_behavior(cx) {
                    PendingMessageBehavior::Queue => "ui.queue_message",
                    PendingMessageBehavior::Steer => "ui.steer",
                });
                vec![Row {
                    action: None,
                    label: label.to_owned(),
                    accessible: label.to_owned(),
                    description: None,
                    source: None,
                    token: None,
                    reason: None,
                    checked: None,
                    enabled: stream.can_submit_message(cx) && !stream.input.read(cx).is_composing(),
                }]
            }
            Kind::Primary | Kind::ResumePending => {
                let resume = kind == Kind::ResumePending || stream.shows_composer_resume(cx);
                let label = vega_i18n::t(if resume {
                    "ui.composer_resume"
                } else {
                    "ui.send"
                });
                vec![Row {
                    action: None,
                    label: label.to_owned(),
                    accessible: label.to_owned(),
                    description: None,
                    source: None,
                    token: None,
                    reason: None,
                    checked: None,
                    enabled: kind == Kind::Primary
                        && if resume {
                            stream.can_resume_composer(cx)
                        } else {
                            stream.can_submit_message(cx)
                        },
                }]
            }
            Kind::Stop => vec![Row {
                action: None,
                label: vega_i18n::t(if stream.actions.stopping {
                    "ui.stopping"
                } else {
                    "ui.stop"
                })
                .to_owned(),
                accessible: vega_i18n::t(if stream.actions.stopping {
                    "ui.stopping"
                } else {
                    "ui.stop"
                })
                .to_owned(),
                description: None,
                source: None,
                token: None,
                reason: None,
                checked: None,
                enabled: !stream.actions.stopping
                    && (stream.actions.running
                        || stream.composer_submit_pending
                        || stream.ordinary_pending_before_run()
                        || stream.has_cancellable_queue_wait()),
            }],
            Kind::Menu => stream
                .composer_action_rows(cx)
                .into_iter()
                .map(|action| {
                    let candidate = match action {
                        ComposerActionRow::Skill(index) => stream
                            .skill_projection
                            .as_ref()
                            .and_then(|projection| projection.candidates.get(index)),
                        _ => None,
                    };
                    let reason = match action {
                        ComposerActionRow::Command(id) => {
                            match stream.command_availability(id.command(), cx) {
                                CommandAvailability::Disabled(reason) => Some(reason),
                                _ => None,
                            }
                        }
                        _ => None,
                    };
                    let source = candidate.map(|candidate| {
                        let label = vega_i18n::t(match candidate.scope {
                            vega_conversation::types::SkillUiScope::Project => {
                                "ui.skill_source_project"
                            }
                            vega_conversation::types::SkillUiScope::VegaGlobal => {
                                "ui.skill_source_local"
                            }
                            vega_conversation::types::SkillUiScope::Imported => {
                                "ui.skill_source_imported"
                            }
                        });
                        let peers: Vec<_> = stream
                            .skill_projection
                            .as_ref()
                            .into_iter()
                            .flat_map(|projection| &projection.candidates)
                            .filter(|peer| {
                                peer.name == candidate.name && peer.scope == candidate.scope
                            })
                            .collect();
                        if peers.len() > 1 {
                            let ordinal = peers
                                .iter()
                                .position(|peer| peer.source_id == candidate.source_id)
                                .map_or(1, |index| index + 1);
                            format!("{label} {ordinal}")
                        } else {
                            label.to_owned()
                        }
                    });
                    let label = candidate.map_or_else(
                        || action.label().to_owned(),
                        |candidate| candidate.name.clone(),
                    );
                    let mut accessible = candidate.map_or_else(
                        || label.clone(),
                        |candidate| {
                            format!(
                                "{}: {} ({})",
                                candidate.name,
                                candidate.description,
                                source.as_deref().unwrap_or_default()
                            )
                        },
                    );
                    if let Some(reason) = reason {
                        accessible = format!("{accessible}: {}", vega_i18n::t(reason));
                    }
                    Row {
                        action: Some(action),
                        label,
                        accessible,
                        description: candidate.map(|candidate| candidate.description.clone()),
                        source,
                        token: match action {
                            ComposerActionRow::Command(id) => Some(id.command().token),
                            _ => None,
                        },
                        reason,
                        checked: candidate.map(|candidate| candidate.selected).or_else(|| {
                            matches!(action, ComposerActionRow::Command(_))
                                .then(|| action.is_selected(&stream.thread))
                        }),
                        enabled: reason.is_none() && !stream.input.read(cx).is_composing(),
                    }
                })
                .collect(),
        };
        let status = (kind == Kind::Menu)
            .then_some({
                if stream.actions.skill_projection_requested {
                    Some("ui.loading")
                } else if rows.is_empty() {
                    Some("ui.no_matching_results")
                } else {
                    None
                }
            })
            .flatten();
        Self {
            id: ProjectionId::new(),
            epoch: stream.run_notice_epoch.clone(),
            source: Source::capture(stream, cx),
            kind,
            rows,
            query: (kind == Kind::Menu)
                .then(|| {
                    stream
                        .actions
                        .skill_query
                        .as_ref()
                        .map(|(_, query)| query.clone())
                        .or_else(|| stream.actions.slash.clone())
                })
                .flatten(),
            status,
            highlight: stream.actions.highlight,
            navigation: stream.actions.native_navigation,
            selected: None,
            selection: ProjectionId::new(),
            colors: theme(cx).colors,
            geometry: None,
            activation: None,
        }
    }

    fn same_source(&self, other: &Self) -> bool {
        self.epoch.matches(&other.epoch)
            && self.source == other.source
            && self.kind == other.kind
            && self.highlight == other.highlight
            && self.navigation == other.navigation
            && self.rows == other.rows
            && self.status == other.status
    }

    fn same_presentation(&self, other: &Self) -> bool {
        self.colors.bg_elevated == other.colors.bg_elevated
            && self.colors.bg_hover == other.colors.bg_hover
            && self.colors.bg_active == other.colors.bg_active
            && self.colors.accent == other.colors.accent
            && self.colors.border_subtle == other.colors.border_subtle
            && self.colors.text_primary == other.colors.text_primary
            && self.colors.text_secondary == other.colors.text_secondary
            && self.colors.text_tertiary == other.colors.text_tertiary
            && self.colors.brand_primary == other.colors.brand_primary
            && self.colors.brand_primary_strong == other.colors.brand_primary_strong
            && self.colors.brand_on_accent == other.colors.brand_on_accent
            && self.id.matches(&other.id)
            && self.same_source(other)
            && self.selected == other.selected
            && self.selection.matches(&other.selection)
            && self.geometry == other.geometry
            && self.activation == other.activation
    }

    fn current(&self, stream: &ConversationStream, cx: &App) -> bool {
        !stream.input.read(cx).is_composing() && self.presentation_current(stream, cx)
    }

    fn presentation_current(&self, stream: &ConversationStream, cx: &App) -> bool {
        self.epoch.matches(&stream.run_notice_epoch)
            && !stream.reading_preview
            && cx
                .try_global::<crate::settings::SettingsOpen>()
                .is_some_and(|settings| !settings.0)
            && cx
                .try_global::<crate::sidebar::OpenedThread>()
                .is_some_and(|route| match &route.0 {
                    Some(thread) => thread.id == stream.thread.id,
                    None => stream.draft_route,
                })
            && match self.kind {
                Kind::Menu => stream.actions.visible(),
                Kind::Stop => true,
                Kind::Trigger => stream.context_menu_available(),
                Kind::RunningSend => {
                    !stream.composer_resume_pending && stream.shows_running_send(cx)
                }
                Kind::Primary => {
                    !stream.actions.running
                        && !stream.composer_submit_pending
                        && !stream.composer_resume_pending
                        && !stream.ordinary_pending_before_run()
                        && !stream.has_cancellable_queue_wait()
                }
                Kind::ResumePending => {
                    stream.composer_resume_pending
                        && !stream.actions.running
                        && !stream.composer_submit_pending
                        && !stream.ordinary_pending_before_run()
                        && !stream.has_cancellable_queue_wait()
                }
            }
            && self.same_source(&Self::capture(stream, self.kind, cx))
    }

    fn reveal_target(&self) -> Option<usize> {
        self.selected.or_else(|| {
            (self.kind == Kind::Menu && self.highlight < self.rows.len()).then_some(self.highlight)
        })
    }

    fn cancel_intent(&mut self) {
        self.activation = None;
        self.selected = None;
        self.selection = ProjectionId::new();
    }

    fn request_activation(&mut self, index: usize, origin: Origin) {
        self.selected = Some(index);
        self.selection = ProjectionId::new();
        self.activation = Some(Activation { index, origin });
    }

    fn accepts_input_display(&self, observed: &Self, index: usize, displayed: bool) -> bool {
        self.kind == Kind::Menu
            && self.id.matches(&observed.id)
            && self.same_source(observed)
            && self.selection.matches(&observed.selection)
            && index == self.highlight
            && displayed
            && self.rows.get(index).is_some_and(|row| row.enabled)
    }

    fn accepts_activation(&self, observed: &Self, index: usize, origin: Origin) -> bool {
        self.id.matches(&observed.id)
            && self.same_source(observed)
            && self.selection.matches(&observed.selection)
            && self.kind != Kind::ResumePending
            && self.rows.get(index).is_some_and(|row| row.enabled)
            && match origin {
                Origin::Pointer => true,
                _ => {
                    let activation = Activation { index, origin };
                    self.selected == Some(index)
                        && self.activation == Some(activation)
                        && observed.activation == Some(activation)
                }
            }
    }

    fn enabled(&self) -> Vec<usize> {
        self.rows
            .iter()
            .enumerate()
            .filter_map(|(index, row)| row.enabled.then_some(index))
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Origin {
    Keyboard,
    Accessibility,
    Pointer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Activation {
    index: usize,
    origin: Origin,
}

pub(super) struct DisplayFence {
    host: WeakEntity<Host>,
    observed: Snapshot,
    index: usize,
}

pub(super) fn input_displayed(stream: &ConversationStream, window: &Window, cx: &App) -> bool {
    let Some(fence) = &stream.actions.native_display else {
        return false;
    };
    let Some(host) = fence.host.upgrade() else {
        return false;
    };
    let host = host.read(cx);
    stream.input.read(cx).focus_handle(cx).is_focused(window)
        && fence.index == stream.actions.highlight
        && host.valid_for_stream(&fence.observed, stream, window, cx)
        && host.snapshot.accepts_input_display(
            &fence.observed,
            fence.index,
            host.displayed.get(fence.index).copied().unwrap_or(false),
        )
}

enum Request {
    Activate(Box<Snapshot>, usize, Origin),
    Displayed(Box<Snapshot>, usize, bool),
    Layout(ProjectionId, f32),
}

#[derive(Clone, Copy, PartialEq)]
struct Geometry {
    bounds: gpui_kit::Bounds<Pixels>,
    viewport: Size<Pixels>,
    trigger: Option<gpui_kit::Bounds<Pixels>>,
    foreground: Pixels,
}

#[derive(Default)]
struct Mount {
    geometry: Cell<Option<Geometry>>,
}

struct Host {
    owner: WeakEntity<ConversationStream>,
    snapshot: Snapshot,
    surface: Option<Surface<Snapshot, Request>>,
    published: Option<Snapshot>,
    focus: FocusHandle,
    lease: Weak<Mount>,
    height: Option<f32>,
    displayed: Vec<bool>,
    activation_focus: Option<FocusHandle>,
    surface_subscriptions: Vec<Subscription>,
    _subscriptions: Vec<Subscription>,
}

impl Host {
    fn stop(&mut self, cx: &App) {
        self.surface_subscriptions.clear();
        self.published = None;
        if let Some(surface) = self.surface.take() {
            surface.entity().read(cx).stop();
        }
        self.snapshot.id = ProjectionId::new();
        self.snapshot.cancel_intent();
        self.activation_focus = None;
        self.displayed.clear();
        self.lease = Weak::new();
    }

    fn valid(&self, observed: &Snapshot, window: &Window, cx: &App) -> bool {
        let Some(owner) = self.owner.upgrade() else {
            return false;
        };
        self.valid_for_stream(observed, owner.read(cx), window, cx)
    }

    fn valid_for_stream(
        &self,
        observed: &Snapshot,
        stream: &ConversationStream,
        window: &Window,
        cx: &App,
    ) -> bool {
        if !window.is_window_active()
            || !self.snapshot.id.matches(&observed.id)
            || !self.snapshot.same_source(observed)
            || self
                .surface
                .as_ref()
                .is_none_or(|surface| surface.errors().borrow().is_some())
        {
            return false;
        }
        let Some(geometry) = self.lease.upgrade().and_then(|mount| mount.geometry.get()) else {
            return false;
        };
        observed.current(stream, cx)
            && observed.geometry == Some(geometry)
            && !geometry.bounds.is_empty()
            && geometry.viewport == window.viewport_size()
            && geometry.foreground == stream.foreground_height
            && geometry.trigger == stream.actions.trigger_bounds.get()
    }

    fn activate(
        &mut self,
        observed: &Snapshot,
        index: usize,
        origin: Origin,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.valid(observed, window, cx)
            || !self.snapshot.accepts_activation(observed, index, origin)
            || (origin == Origin::Keyboard
                && (!self.focus.contains_focused(window, cx)
                    || !self
                        .activation_focus
                        .as_ref()
                        .is_some_and(|focus| focus.is_focused(window))))
            || !self.displayed.get(index).copied().unwrap_or(false)
        {
            return;
        }
        let Some(owner) = self.owner.upgrade() else {
            return;
        };
        let row = observed.rows[index].action;
        owner.update(cx, |stream, cx| {
            if !observed.current(stream, cx) {
                return;
            }
            match observed.kind {
                Kind::Menu => {
                    if let Some(row) = row {
                        stream.select_composer_row(row, window, cx);
                    }
                }
                Kind::Stop => stream.stop_composer_action(&StopComposer, window, cx),
                Kind::Primary => stream.activate_composer_primary(cx),
                Kind::Trigger => stream.open_composer_actions(window, cx),
                Kind::RunningSend => {
                    if stream.shows_running_send(cx) && !stream.input.read(cx).is_composing() {
                        stream.submit_message(cx);
                    }
                }
                Kind::ResumePending => (),
            }
        });
        self.snapshot.activation = None;
        self.activation_focus = None;
        self.snapshot.selection = ProjectionId::new();
        self.displayed.fill(false);
        cx.notify();
    }

    fn select(&mut self, index: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        self.snapshot.selected = index;
        self.activation_focus = None;
        self.snapshot.activation = None;
        self.snapshot.selection = ProjectionId::new();
        self.displayed.fill(false);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.focus.contains_focused(window, cx) || !self.valid(&self.snapshot, window, cx) {
            return;
        }
        match crate::nsh::keyboard::Key::from_event(event) {
            crate::nsh::keyboard::Key::Next(backwards) => {
                let enabled = self.snapshot.enabled();
                let position = self
                    .snapshot
                    .selected
                    .and_then(|index| enabled.iter().position(|item| *item == index));
                let next = match (position, backwards) {
                    (Some(position), true) => position
                        .checked_sub(1)
                        .and_then(|position| enabled.get(position)),
                    (Some(position), false) => enabled.get(position + 1),
                    (None, true) => enabled.last(),
                    (None, false) => enabled.first(),
                }
                .copied();
                if let Some(index) = next {
                    self.select(Some(index), window, cx);
                } else {
                    crate::nsh::keyboard::leave(&self.focus, backwards, window, cx);
                }
                cx.stop_propagation();
            }
            crate::nsh::keyboard::Key::Activate => {
                if let Some(index) = self.snapshot.selected
                    && self.displayed.get(index).copied().unwrap_or(false)
                {
                    self.activation_focus = window.focused(cx);
                    self.snapshot.request_activation(index, Origin::Keyboard);
                    self.displayed.fill(false);
                    cx.notify();
                }
                cx.stop_propagation();
            }
            _ => {
                if self.snapshot.kind == Kind::Menu
                    && event.keystroke.key == "escape"
                    && !event.prefer_character_input
                    && !event.is_held
                    && event.keystroke.modifiers == Default::default()
                {
                    self.dismiss(window, cx);
                    cx.stop_propagation();
                }
            }
        }
    }

    fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.valid(&self.snapshot, window, cx) {
            return;
        }
        if let Some(owner) = self.owner.upgrade() {
            owner.update(cx, |stream, cx| {
                stream.close_composer_actions(&CloseComposerActions, window, cx);
                stream.focus_composer(window, cx);
            });
        }
    }

    fn mount(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.surface.is_some() {
            return;
        }
        let (surface, mut requests) =
            Surface::new(self.snapshot.clone(), pixels::render, window, cx);
        cx.spawn(async move |this, cx| {
            while let Some(request) = requests.recv().await {
                if this
                    .update_in(cx, |this, window, cx| match request {
                        Request::Activate(observed, index, origin) => {
                            this.activate(&observed, index, origin, window, cx)
                        }
                        Request::Displayed(observed, index, visible) => {
                            if this.valid(&observed, window, cx)
                                && this.snapshot.selection.matches(&observed.selection)
                                && let Some(displayed) = this.displayed.get_mut(index)
                            {
                                *displayed = visible;
                                if visible
                                    && observed.kind == Kind::Menu
                                    && index == observed.highlight
                                    && let Some(owner) = this.owner.upgrade()
                                {
                                    let fence = DisplayFence {
                                        host: cx.weak_entity(),
                                        observed: (*observed).clone(),
                                        index,
                                    };
                                    owner.update(cx, |stream, _| {
                                        stream.actions.native_display = Some(fence)
                                    });
                                }
                            }
                        }
                        Request::Layout(id, height) => {
                            if this.snapshot.id.matches(&id)
                                && height.is_finite()
                                && height > 0.
                                && this.height.is_none_or(|old| (old - height).abs() > 0.5)
                            {
                                this.height = Some(height.min(Layout::COMPOSER_PICKER_MAX_HEIGHT));
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
        let child_focus = surface.focus_handle(cx).tab_stop(false);
        for focus in [self.focus.clone(), child_focus] {
            self.surface_subscriptions
                .push(cx.on_blur(&focus, window, |this, _, cx| {
                    if this
                        .snapshot
                        .activation
                        .is_some_and(|activation| activation.origin == Origin::Keyboard)
                    {
                        this.snapshot.cancel_intent();
                        this.activation_focus = None;
                        this.displayed.fill(false);
                        cx.notify();
                    }
                }));
        }
        self.published = Some(self.snapshot.clone());
        self.surface = Some(surface);
    }
}

#[derive(gpui_kit::IntoElement)]
pub(super) struct NativeChrome {
    pub(super) owner: WeakEntity<ConversationStream>,
    pub(super) kind: Kind,
}

impl RenderOnce for NativeChrome {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Some(owner) = self.owner.upgrade() else {
            return div().into_any_element();
        };
        let snapshot = Snapshot::capture(owner.read(cx), self.kind, cx);
        if self.kind == Kind::Menu && !owner.read(cx).actions.visible() {
            return div().into_any_element();
        }
        let key = format!(
            "nsh-composer-chrome-{}-{}",
            owner.entity_id(),
            match self.kind {
                Kind::Menu => "menu",
                Kind::Stop => "stop",
                Kind::Primary => "primary",
                Kind::ResumePending => "resume-pending",
                Kind::RunningSend => "running-send",
                Kind::Trigger => "trigger",
            }
        );
        let state = window.use_keyed_state(key, cx, |window, cx| {
            let focus = if self.kind == Kind::Trigger {
                owner.read(cx).action_focus[0].clone()
            } else if self.kind != Kind::Menu {
                owner.read(cx).action_focus[1].clone()
            } else {
                cx.focus_handle()
            }
            .tab_stop(self.kind == Kind::ResumePending || !snapshot.enabled().is_empty());
            let subscriptions = vec![
                cx.on_release(|this: &mut Host, cx| this.stop(cx)),
                cx.on_focus_in(&focus, window, |this: &mut Host, window, cx| {
                    if this.focus.contains_focused(window, cx) {
                        let enabled = this.snapshot.enabled();
                        let index = if window.modifiers().shift {
                            enabled.last().copied()
                        } else {
                            enabled.first().copied()
                        };
                        this.snapshot.selected = index;
                        this.snapshot.activation = None;
                        this.activation_focus = None;
                        this.displayed.fill(false);
                        cx.notify();
                    }
                }),
                cx.on_focus_out(&focus, window, |this: &mut Host, _, _, cx| {
                    this.snapshot.cancel_intent();
                    this.activation_focus = None;
                    this.displayed.fill(false);
                    cx.notify();
                }),
            ];
            Host {
                owner: self.owner.clone(),
                snapshot: snapshot.clone(),
                surface: None,
                published: None,
                focus,
                lease: Weak::new(),
                height: None,
                displayed: vec![false; snapshot.rows.len()],
                activation_focus: None,
                surface_subscriptions: Vec::new(),
                _subscriptions: subscriptions,
            }
        });
        state.update(cx, |host, cx| {
            let same = host.snapshot.same_source(&snapshot);
            let previous = host.snapshot.clone();
            if !same {
                host.stop(cx);
                host.height = None;
            }
            host.snapshot = snapshot;
            host.snapshot.id = if same {
                previous.id
            } else {
                host.snapshot.id.clone()
            };
            if same {
                host.snapshot.selected = previous.selected;
                host.snapshot.selection = previous.selection;
                host.snapshot.activation = previous.activation;
                host.snapshot.geometry = previous.geometry;
            }
            host.displayed.resize(host.snapshot.rows.len(), false);
        });
        if self.kind == Kind::Menu {
            gpui_kit::deferred(
                div()
                    .absolute()
                    .bottom(gpui_kit::relative(1.0))
                    .mb_2()
                    .left_0()
                    .w_full()
                    .max_w_full()
                    .occlude()
                    .child(state),
            )
            .with_priority(2)
            .into_any_element()
        } else {
            state.into_any_element()
        }
    }
}

impl Render for Host {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(owner) = self.owner.upgrade() else {
            self.stop(cx);
            return div().into_any_element();
        };
        if !self.snapshot.presentation_current(owner.read(cx), cx) {
            self.stop(cx);
            return div().into_any_element();
        }
        let lease = self.lease.upgrade().unwrap_or_else(|| {
            self.stop(cx);
            Rc::new(Mount::default())
        });
        self.lease = Rc::downgrade(&lease);
        let geometry = lease.geometry.get();
        if self.snapshot.geometry != geometry {
            self.snapshot.geometry = geometry;
            self.snapshot.selection = ProjectionId::new();
            self.snapshot.activation = None;
            self.activation_focus = None;
            self.displayed.fill(false);
        }
        self.displayed.resize(self.snapshot.rows.len(), false);
        self.focus = self.focus.clone().tab_stop(
            self.snapshot.kind == Kind::ResumePending || !self.snapshot.enabled().is_empty(),
        );
        self.mount(window, cx);
        let Some(surface) = &mut self.surface else {
            return div().into_any_element();
        };
        surface.sync_environment(cx);
        if self
            .published
            .as_ref()
            .is_none_or(|previous| !previous.same_presentation(&self.snapshot))
        {
            surface.publish(self.snapshot.clone());
            self.published = Some(self.snapshot.clone());
        }
        let stop = self.snapshot.kind != Kind::Menu;
        let mut host = div()
            .id("nsh-composer-chrome-host")
            .role(Role::Group)
            .relative()
            .min_w_0()
            .track_focus(&self.focus)
            .capture_key_down(cx.listener(Self::key))
            .when(self.snapshot.kind == Kind::RunningSend, |host| {
                host.key_context("PendingMessageControl")
                    .on_action(cx.listener(
                        |this, _: &ActivatePendingMessageControl, window, cx| {
                            if this.focus.contains_focused(window, cx)
                                && this.valid(&this.snapshot, window, cx)
                                && this.snapshot.rows.first().is_some_and(|row| row.enabled)
                                && this.displayed.first() == Some(&true)
                            {
                                this.activation_focus = window.focused(cx);
                                this.snapshot.request_activation(0, Origin::Keyboard);
                                this.displayed.fill(false);
                                cx.notify();
                            }
                            cx.stop_propagation();
                        },
                    ))
            })
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(surface.entity());
        host = if stop {
            host.size(px(if self.snapshot.kind == Kind::Trigger {
                Layout::TITLEBAR_CONTROL_SIZE
            } else {
                Layout::COMPOSER_SEND_SIZE
            }))
            .flex_shrink_0()
        } else {
            let estimate =
                (self.snapshot.rows.len().max(1) as f32 * Typography::SIDEBAR_LINE_HEIGHT * 2.
                    + 32.)
                    .min(Layout::COMPOSER_PICKER_MAX_HEIGHT);
            host.w_full()
                .h(px(self.height.unwrap_or(estimate)))
                .on_mouse_down_out(cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    let on_trigger = this.owner.upgrade().is_some_and(|owner| {
                        owner
                            .read(cx)
                            .actions
                            .trigger_bounds
                            .get()
                            .is_some_and(|bounds| bounds.contains(&event.position))
                    });
                    if !on_trigger {
                        this.dismiss(window, cx);
                    }
                }))
        };
        for (index, row) in self.snapshot.rows.iter().enumerate() {
            let observed = self.snapshot.clone();
            let weak = cx.weak_entity();
            let focus_owner = weak.clone();
            let focus_observed = observed.clone();
            host = host.child(
                div()
                    .id(format!("nsh-composer-action-ax-{index}"))
                    .absolute()
                    .inset_0()
                    .role(if row.checked.is_some() {
                        Role::CheckBox
                    } else {
                        Role::Button
                    })
                    .aria_label(row.accessible.clone())
                    .when_some(row.checked, |node, checked| {
                        node.aria_toggled(if checked {
                            gpui_kit::Toggled::True
                        } else {
                            gpui_kit::Toggled::False
                        })
                    })
                    .when(!row.enabled, |node| {
                        node.aria_description(row.reason.map(vega_i18n::t).unwrap_or(&row.label))
                    })
                    .when(self.snapshot.selected == Some(index), |node| {
                        node.aria_active_descendant()
                    })
                    .on_a11y_action(AccessibleAction::Focus, move |_, window, cx| {
                        let _ = focus_owner.update(cx, |host, cx| {
                            if host.valid(&focus_observed, window, cx)
                                && host.snapshot.enabled().contains(&index)
                            {
                                host.select(Some(index), window, cx);
                            }
                        });
                    })
                    .on_a11y_action(AccessibleAction::Click, move |_, window, cx| {
                        let _ = weak.update(cx, |host, cx| {
                            if host.valid(&observed, window, cx)
                                && host.snapshot.selection.matches(&observed.selection)
                                && host.snapshot.enabled().contains(&index)
                                && host.displayed.get(index).copied().unwrap_or(false)
                            {
                                host.activation_focus = None;
                                host.snapshot
                                    .request_activation(index, Origin::Accessibility);
                                host.displayed.fill(false);
                                cx.notify();
                            }
                        });
                    }),
            );
        }
        Mounted {
            child: host.into_any_element(),
            lease,
            owner: self.owner.clone(),
            host: cx.weak_entity(),
        }
        .into_any_element()
    }
}

struct Mounted {
    child: AnyElement,
    lease: Rc<Mount>,
    owner: WeakEntity<ConversationStream>,
    host: WeakEntity<Host>,
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
        Some("nsh-composer-chrome-mounted".into())
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
        bounds: gpui_kit::Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let previous = self.lease.geometry.replace(None);
        if let Some(id) = id {
            window.with_element_state::<Rc<Mount>, _>(id, |_, _| ((), self.lease.clone()));
            if let Some(owner) = self.owner.upgrade() {
                let stream = owner.read(cx);
                if self
                    .host
                    .upgrade()
                    .is_some_and(|host| host.read(cx).snapshot.kind == Kind::Trigger)
                {
                    stream.actions.trigger_bounds.set(Some(bounds));
                }
                let visible = bounds.intersect(&window.content_mask().bounds);
                if !visible.is_empty() {
                    self.lease.geometry.set(Some(Geometry {
                        bounds: visible,
                        viewport: window.viewport_size(),
                        trigger: stream.actions.trigger_bounds.get(),
                        foreground: stream.foreground_height,
                    }));
                }
            }
        }
        if previous != self.lease.geometry.get() {
            let _ = self.host.update(cx, |_, cx| cx.notify());
        }
        self.child.prepaint(window, cx);
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: gpui_kit::Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
    }
}
