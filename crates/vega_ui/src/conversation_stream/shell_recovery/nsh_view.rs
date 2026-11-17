#[path = "keyboard.rs"]
mod keyboard;
#[path = "reveal.rs"]
mod reveal;

use super::{ConversationStream, ShellRecoveryAction, ShellRecoveryError, TaskBackend};
use crate::nsh::{ProjectionId, Surface};
use gpui_kit::{App, Context, Window};
use nsh_core::prelude::*;
use tokio::sync::{mpsc, oneshot};
use vega_theme::{Layout, ThemeColors, Typography};

pub(super) struct Presentation {
    surface: Surface<Snapshot, Request>,
    authority: super::ShellRecoveryRequested,
    id: ProjectionId,
    recovery: bool,
    fault: bool,
    height: f32,
    selected: Option<Action>,
    selection: ProjectionId,
    reveal: bool,
    shown: Snapshot,
    displayed: Option<Snapshot>,
    _focus_subscriptions: Vec<gpui_kit::Subscription>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Action {
    Cancel(String),
    Read,
    Review,
    Check,
    Retry,
    Prepare,
    Accept,
    KeepPaused,
    NotNow,
    Reload,
}

#[derive(Clone, PartialEq, Eq)]
struct Active {
    operation: String,
    state: String,
}

#[derive(Clone, PartialEq, Eq)]
struct Row {
    operation: String,
    thread: String,
    command: String,
    exit_code: Option<i32>,
    reason: String,
    output_incomplete: bool,
    can_retry: bool,
    can_resume: bool,
}

#[derive(Clone)]
struct Snapshot {
    id: ProjectionId,
    observation: ProjectionId,
    projection: ProjectionId,
    confirmation: ProjectionId,
    thread: String,
    project: String,
    backend: TaskBackend,
    draft: bool,
    summary: bool,
    sequence: u64,
    pending: Option<&'static str>,
    error: Option<ShellRecoveryError>,
    opened: bool,
    has_projection: bool,
    has_confirmation: bool,
    submitted: bool,
    recovery: bool,
    presentation_eligible: bool,
    workspace: String,
    rows: Vec<Row>,
    active: Vec<Active>,
    colors: ThemeColors,
    appearance: vega_theme::Appearance,
    language: &'static str,
    selected: Option<Action>,
    selection: ProjectionId,
    keyboard_active: bool,
    reveal: bool,
}

struct Control {
    action: Action,
    key: &'static str,
    enabled: bool,
}

enum Request {
    Activate(Box<Snapshot>, Action),
    Layout(ProjectionId, f32),
    Fault(ProjectionId),
    Displayed(Box<Snapshot>, Action),
    Reveal(Box<Snapshot>, Action, oneshot::Sender<bool>),
}

impl Snapshot {
    fn visible(&self) -> bool {
        self.presentation_eligible
            && if self.draft {
                if self.summary {
                    !matches!(
                        super::recovery_presentation(
                            true,
                            !self.rows.is_empty(),
                            self.error.is_some()
                        ),
                        super::RecoveryPresentation::Hidden
                    )
                } else {
                    self.opened
                }
            } else {
                !self.rows.is_empty() || !self.active.is_empty() || self.error.is_some()
            }
    }

    fn same_authority(&self, other: &Self) -> bool {
        self.id.matches(&other.id)
            && self.observation.matches(&other.observation)
            && self.projection.matches(&other.projection)
            && self.confirmation.matches(&other.confirmation)
            && self.thread == other.thread
            && self.project == other.project
            && self.backend == other.backend
            && self.draft == other.draft
            && self.summary == other.summary
            && self.sequence == other.sequence
            && self.pending == other.pending
            && self.error == other.error
            && self.opened == other.opened
            && self.has_projection == other.has_projection
            && self.has_confirmation == other.has_confirmation
            && self.submitted == other.submitted
            && self.recovery == other.recovery
            && self.presentation_eligible == other.presentation_eligible
            && self.workspace == other.workspace
            && self.rows == other.rows
            && self.active == other.active
    }

    fn controls(&self) -> Vec<Control> {
        let mut controls = Vec::new();
        let busy = self.pending.is_some();
        let mut add = |action, key, enabled| {
            controls.push(Control {
                action,
                key,
                enabled,
            })
        };
        if self.summary {
            if self.recovery {
                add(Action::Reload, "ui.retry", true);
            }
            add(Action::Review, "ui.shell_recovery.review", !self.opened);
            return controls;
        }
        if self.draft && self.opened {
            add(Action::NotNow, "ui.shell_recovery.not_now", true);
        }
        for command in &self.active {
            add(
                Action::Cancel(command.operation.clone()),
                "ui.shell_session.cancel",
                !busy && command.state == "running",
            );
        }
        if self.recovery {
            add(Action::Reload, "ui.retry", true);
            if self.opened {
                add(Action::KeepPaused, "ui.shell_recovery.keep_paused", !busy);
            } else if self.rows.is_empty() && self.error.is_some() {
                add(Action::Read, "ui.shell_recovery.check", !busy);
            } else if !self.rows.is_empty() {
                add(Action::Review, "ui.shell_recovery.review", !busy);
            }
        } else if self.rows.is_empty() {
            if self.error.is_some() {
                add(Action::Read, "ui.shell_recovery.check", !busy);
            }
        } else if !self.opened {
            add(Action::Review, "ui.shell_recovery.review", !busy);
        } else {
            if self.has_confirmation {
                add(
                    Action::Accept,
                    "ui.shell_recovery.accept",
                    !busy && !self.submitted,
                );
                add(Action::KeepPaused, "ui.shell_recovery.keep_paused", !busy);
            } else if self.has_projection {
                add(Action::Check, "ui.shell_recovery.check", !busy);
                add(
                    Action::Retry,
                    "ui.shell_recovery.retry",
                    !busy && self.rows.iter().any(|row| row.can_retry),
                );
                add(
                    Action::Prepare,
                    "ui.shell_recovery.resume",
                    !busy && self.rows.iter().all(|row| row.can_resume),
                );
            }
            if !self.draft {
                add(Action::NotNow, "ui.shell_recovery.not_now", !busy);
            }
        }
        controls
    }

    fn accepts(&self, observed: &Self, action: &Action) -> bool {
        self.visible()
            && self.backend == TaskBackend::Native
            && !self.project.is_empty()
            && self.same_authority(observed)
            && self
                .controls()
                .iter()
                .any(|control| control.action == *action && control.enabled)
    }

    fn accepts_selection(&self, observed: &Self, action: &Action) -> bool {
        self.accepts(observed, action)
            && self.keyboard_active
            && observed.keyboard_active
            && self.selection.matches(&observed.selection)
            && self.selected.as_ref() == Some(action)
            && observed.selected.as_ref() == Some(action)
    }

    fn accepts_reveal(&self, observed: &Self, action: &Action) -> bool {
        self.accepts_selection(observed, action) && self.reveal && observed.reveal
    }
}

impl ConversationStream {
    fn shell_page(&self) -> Option<&Presentation> {
        if self.draft_route && !self.shell_recovery.opened {
            self.shell_recovery.summary.as_ref()
        } else {
            self.shell_recovery.presentation.as_ref()
        }
    }

    fn shell_page_mut(&mut self) -> Option<&mut Presentation> {
        if self.draft_route && !self.shell_recovery.opened {
            self.shell_recovery.summary.as_mut()
        } else {
            self.shell_recovery.presentation.as_mut()
        }
    }

    fn shell_snapshot(&self, id: ProjectionId, recovery: bool, cx: &App) -> Snapshot {
        let state = &self.shell_recovery;
        let projection = state.projection.as_ref();
        let page = self.shell_page();
        Snapshot {
            id,
            observation: state.observation.clone(),
            projection: state.projection_identity.clone(),
            confirmation: state.confirmation_identity.clone(),
            thread: self.thread.id.clone(),
            project: self.thread.project_id.clone(),
            backend: self.thread.backend,
            draft: self.draft_route,
            summary: self.draft_route && !state.opened,
            sequence: state.sequence,
            pending: state.pending,
            error: state.error,
            opened: state.opened,
            has_projection: projection.is_some(),
            has_confirmation: state.decision.confirmation.is_some(),
            submitted: state.decision.submitted,
            recovery,
            presentation_eligible: !self.reading_preview
                && !self.automation_permission.busy()
                && page.is_none_or(|page| {
                    page.authority.matches_route(
                        &self.thread,
                        self.draft_route,
                        page.authority.sequence,
                    )
                }),
            workspace: projection
                .map(|projection| projection.workspace.clone())
                .unwrap_or_default(),
            rows: projection
                .map(|projection| {
                    projection
                        .rows
                        .iter()
                        .map(|row| Row {
                            operation: row.operation_id.clone(),
                            thread: row.thread_id.clone(),
                            command: row.command.clone(),
                            exit_code: row.exit_code,
                            reason: row.reason.clone(),
                            output_incomplete: row.output_incomplete,
                            can_retry: row.can_retry,
                            can_resume: row.can_resume,
                        })
                        .collect()
                })
                .unwrap_or_default(),
            active: projection
                .map(|projection| {
                    projection
                        .active
                        .iter()
                        .map(|active| Active {
                            operation: active.operation_id.clone(),
                            state: active.state.clone(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            colors: vega_theme::theme(cx).colors,
            appearance: vega_theme::theme(cx).appearance,
            language: vega_i18n::language(),
            selected: page.and_then(|page| page.selected.clone()),
            selection: page.map(|page| page.selection.clone()).unwrap_or_default(),
            keyboard_active: false,
            reveal: page.is_some_and(|page| page.reveal),
        }
    }

    fn accepts_shell(&self, observed: &Snapshot, action: &Action, cx: &App) -> bool {
        self.shell_page().is_some_and(|page| {
            !self.reading_preview
                && !self.automation_permission.busy()
                && !page.fault
                && page.id.matches(&observed.id)
                && self
                    .shell_snapshot(page.id.clone(), page.recovery, cx)
                    .accepts(observed, action)
        })
    }

    fn shell_action(
        &mut self,
        observed: &Snapshot,
        action: Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.accepts_shell(observed, &action, cx) {
            return;
        }
        self.shell_select(action.clone(), false);
        self.shell_focus().focus(window, cx);
        match action {
            Action::Cancel(operation) => {
                self.request_shell_recovery(ShellRecoveryAction::Cancel(operation), cx)
            }
            Action::Read => self.request_shell_recovery(ShellRecoveryAction::Read, cx),
            Action::Review => {
                self.shell_recovery.observation = ProjectionId::new();
                self.shell_recovery.opened = true;
                self.shell_recovery.safe_focus.focus(window, cx);
                self.refresh_shell_recovery(cx);
            }
            Action::Check | Action::Retry | Action::Prepare => {
                if let Some(projection) = self.shell_recovery.projection.clone() {
                    let request = match action {
                        Action::Check => ShellRecoveryAction::Check(projection),
                        Action::Retry => ShellRecoveryAction::Retry(projection),
                        _ => {
                            self.shell_recovery.safe_focus.focus(window, cx);
                            ShellRecoveryAction::Prepare(projection)
                        }
                    };
                    self.request_shell_recovery(request, cx);
                }
            }
            Action::Accept => {
                if let Some(confirmation) = self.shell_recovery.decision.confirmation.clone() {
                    self.request_shell_recovery(ShellRecoveryAction::Resume(confirmation), cx);
                }
            }
            Action::KeepPaused | Action::NotNow => self.dismiss_shell_recovery(window, cx),
            Action::Reload => {
                if self.draft_route && !self.shell_recovery.opened {
                    self.shell_recovery.summary = None;
                } else {
                    self.shell_recovery.presentation = None;
                }
            }
        }
        cx.notify();
    }

    pub(in crate::conversation_stream) fn hide_shell_recovery_surface(&mut self) {
        self.shell_recovery.presentation = None;
        self.shell_recovery.summary = None;
    }

    fn create_shell_surface(
        &mut self,
        recovery: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = ProjectionId::new();
        let initial = self.shell_snapshot(id.clone(), recovery, cx);
        let selected = initial.initial_selection();
        let (surface, mut requests) = Surface::new(initial.clone(), render, window, cx);
        let mut errors = surface.errors();
        let error_id = id.clone();
        cx.spawn(async move |this, cx| {
            while errors.changed().await.is_ok() {
                if errors.borrow_and_update().is_some() {
                    let _ = this.update(cx, |this, cx| {
                        if let Some(page) = this.shell_page_mut()
                            && page.id.matches(&error_id)
                        {
                            page.fault = true;
                            cx.notify();
                        }
                    });
                    break;
                }
            }
        })
        .detach();
        let mut focus_subscriptions = Vec::new();
        for focus in [
            &self.shell_recovery.review_focus,
            &self.shell_recovery.safe_focus,
        ] {
            focus_subscriptions.push(cx.on_focus_in(focus, window, |_, _, cx| cx.notify()));
            focus_subscriptions.push(cx.on_focus_out(focus, window, |_, _, _, cx| cx.notify()));
        }
        let page = Presentation {
            surface,
            authority: super::ShellRecoveryRequested {
                source: self.thread.clone(),
                sequence: self.shell_recovery.sequence,
                draft: self.draft_route,
                action: ShellRecoveryAction::Read,
            },
            id,
            recovery,
            fault: false,
            height: Layout::TOOL_GROUP_MAX_HEIGHT,
            selected,
            selection: ProjectionId::new(),
            reveal: true,
            shown: initial,
            displayed: None,
            _focus_subscriptions: focus_subscriptions,
        };
        if self.draft_route && !self.shell_recovery.opened {
            self.shell_recovery.summary = Some(page);
        } else {
            self.shell_recovery.presentation = Some(page);
        }
        cx.spawn_in(window, async move |this, cx| {
            while let Some(request) = requests.recv().await {
                if this
                    .update_in(cx, |this, window, cx| match request {
                        Request::Activate(observed, action) => {
                            this.shell_action(&observed, action, window, cx)
                        }
                        Request::Layout(id, height) => {
                            if height.is_finite()
                                && height > 0.
                                && let Some(page) = this.shell_page_mut()
                                && page.id.matches(&id)
                                && (page.height - height).abs() > 0.5
                            {
                                page.height = height.min(Layout::TOOL_GROUP_MAX_HEIGHT);
                                cx.notify();
                            }
                        }
                        Request::Displayed(observed, action) => {
                            if let Some(page) = this.shell_page() {
                                let mut current =
                                    this.shell_snapshot(page.id.clone(), page.recovery, cx);
                                current.keyboard_active = this.shell_focus().is_focused(window);
                                if !page.fault
                                    && current.accepts_selection(&observed, &action)
                                    && let Some(page) = this.shell_page_mut()
                                {
                                    page.displayed = Some(*observed);
                                    cx.notify();
                                }
                            }
                        }
                        Request::Fault(id) => {
                            if let Some(page) = this.shell_page_mut()
                                && page.id.matches(&id)
                            {
                                page.fault = true;
                                cx.notify();
                            }
                        }
                        Request::Reveal(observed, action, reply) => {
                            let accepted = this.shell_page().is_some_and(|page| {
                                let mut current =
                                    this.shell_snapshot(page.id.clone(), page.recovery, cx);
                                current.keyboard_active = this.shell_focus().is_focused(window);
                                !page.fault && current.accepts_reveal(&observed, &action)
                            });
                            let _ = reply.send(accepted);
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

    pub(in crate::conversation_stream) fn render_shell_recovery(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        use gpui_kit::prelude::*;
        if self.draft_route && self.shell_recovery.opened {
            let Some(page) = self.shell_recovery.summary.as_ref() else {
                return gpui_kit::div().into_any_element();
            };
            let mut current = self.shell_snapshot(page.id.clone(), page.recovery, cx);
            current.summary = true;
            current.keyboard_active = false;
            if !current.visible() {
                return gpui_kit::div().into_any_element();
            }
            let Some(page) = self.shell_recovery.summary.as_mut() else {
                return gpui_kit::div().into_any_element();
            };
            page.surface.sync_environment(cx);
            page.surface.publish(current);
            return gpui_kit::div()
                .debug_selector(|| "shell-recovery-summary-native-host".to_owned())
                .w_full()
                .max_w(gpui_kit::px(Layout::COMPOSER_MAX_WIDTH))
                .mx_auto()
                .h(gpui_kit::px(page.height))
                .child(page.surface.entity())
                .into_any_element();
        }
        self.render_shell_recovery_leaf(window, cx)
    }

    pub(in crate::conversation_stream) fn render_shell_recovery_overlay(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        use gpui_kit::prelude::*;
        if !self.draft_route || !self.shell_recovery.opened {
            return gpui_kit::div().into_any_element();
        }
        if self.thread.backend != TaskBackend::Native
            || !self
                .shell_snapshot(ProjectionId::new(), false, cx)
                .visible()
        {
            self.hide_shell_recovery_surface();
            return gpui_kit::div().into_any_element();
        }
        let details = self.render_shell_recovery_leaf(window, cx);
        gpui_kit::deferred(
            gpui_kit::div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .occlude()
                .on_mouse_down(gpui_kit::MouseButton::Left, |_, _, cx| {
                    cx.stop_propagation()
                })
                .child(details),
        )
        .with_priority(3)
        .into_any_element()
    }

    fn render_shell_recovery_leaf(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        use gpui_kit::prelude::*;
        let visible = !self.reading_preview
            && self
                .shell_snapshot(ProjectionId::new(), false, cx)
                .visible()
            && self.thread.backend == TaskBackend::Native;
        if !visible {
            self.hide_shell_recovery_surface();
            return gpui_kit::div().into_any_element();
        }
        let previous_focus = if self.shell_recovery.opened {
            &self.shell_recovery.review_focus
        } else {
            &self.shell_recovery.safe_focus
        };
        if previous_focus.is_focused(window) {
            self.shell_focus().focus(window, cx);
        }
        let failed = self.shell_page().is_some_and(|page| page.fault);
        if self.shell_page().is_none() || failed {
            self.create_shell_surface(failed, window, cx);
        }
        let Some(page) = self.shell_page() else {
            return gpui_kit::div().into_any_element();
        };
        let mut current = self.shell_snapshot(page.id.clone(), page.recovery, cx);
        let selection_stale = !current.same_authority(&page.shown)
            || page
                .selected
                .as_ref()
                .is_some_and(|action| !current.accepts(&current, action));
        if selection_stale {
            let selected = current.initial_selection();
            if let Some(page) = self.shell_page_mut() {
                page.selected = selected;
                page.selection = ProjectionId::new();
                page.reveal = true;
            }
        }
        if let Some(page) = self.shell_page() {
            current.selected = page.selected.clone();
            current.selection = page.selection.clone();
            current.reveal = page.reveal;
        }
        current.keyboard_active = self.shell_focus().is_focused(window);
        let accessibility = self.shell_accessibility(&current, window, cx);
        let focus = self.shell_focus().clone();
        let Some(page) = self.shell_page_mut() else {
            return gpui_kit::div().into_any_element();
        };
        let surface = page.surface.entity();
        let height = page.height;
        page.surface.sync_environment(cx);
        page.surface.publish(current.clone());
        page.shown = current.clone();
        let host = if current.summary {
            "shell-recovery-summary-native-host"
        } else {
            "shell-recovery-native-host"
        };
        gpui_kit::div()
            .id(host)
            .debug_selector(move || host.to_owned())
            .relative()
            .w_full()
            .max_w(gpui_kit::px(Layout::COMPOSER_MAX_WIDTH))
            .mx_auto()
            .h(gpui_kit::px(height))
            .track_focus(&focus)
            .tab_stop(true)
            .role(gpui_kit::Role::Group)
            .capture_key_down(
                cx.listener(|this, event: &gpui_kit::KeyDownEvent, window, cx| {
                    if this.shell_key(event, window, cx) {
                        cx.stop_propagation();
                    }
                }),
            )
            .capture_any_mouse_up(cx.listener(
                |this, event: &gpui_kit::MouseUpEvent, window, cx| {
                    if event.button == gpui_kit::MouseButton::Left {
                        this.shell_focus().focus(window, cx);
                        cx.notify();
                    }
                },
            ))
            .child(surface)
            .children(accessibility)
            .into_any_element()
    }
}

fn render(incoming: Signal<Snapshot>, commands: mpsc::Sender<Request>) -> AnyElement {
    let state = signal_with(incoming.get_untracked(), SignalEquals::Never);
    let content = signal_with(incoming.get_untracked(), SignalEquals::Never);
    effect(move || {
        let next = incoming.get();
        let previous = content.get_untracked();
        state.set(next.clone());
        if !next.same_authority(&previous)
            || next.appearance != previous.appearance
            || next.language != previous.language
        {
            content.set(next);
        }
    });
    let viewport = reveal::Viewport::new(element_ref());
    let attempted = reveal::Tracker::new();
    let failed = signal(false);
    let layout_commands = commands.clone();
    let layout_id = incoming.get_untracked().id;
    let measured = signal(0.0f32);
    effect({
        let commands = commands.clone();
        move || {
            if failed.get() {
                let commands = commands.clone();
                let id = state.get_untracked().id;
                spawn(async move {
                    let _ = commands.send(Request::Fault(id)).await;
                });
            }
        }
    });
    scroll()
        .element_ref(&viewport.target)
        .on_layout(move |bounds: &Bounds<Points>| viewport.layout(*bounds))
        .w_full()
        .max_h(px(Layout::TOOL_GROUP_MAX_HEIGHT))
        .child(dynamic(move || {
            let snapshot = content.get();
            if snapshot.summary {
                let label = match super::recovery_presentation(
                    true,
                    !snapshot.rows.is_empty(),
                    snapshot.error.is_some(),
                ) {
                    super::RecoveryPresentation::Summary(label) => label,
                    _ => return view().into_element(),
                };
                let mut summary = view()
                    .w_full()
                    .px(px(Layout::CONVERSATION_PADDING))
                    .py_2()
                    .flex_row()
                    .flex_wrap()
                    .gap_2()
                    .text_size(px(Typography::BODY))
                    .text_color(crate::nsh::color(snapshot.colors.text_secondary))
                    .child(text(vega_i18n::t(label)));
                for control in snapshot.controls() {
                    summary = summary.child(control_view(
                        state,
                        snapshot.clone(),
                        control,
                        viewport,
                        attempted,
                        commands.clone(),
                        failed,
                    ));
                }
                return summary
                    .on_layout({
                        let commands = layout_commands.clone();
                        let id = layout_id.clone();
                        move |bounds: &Bounds<Points>| {
                            report_height(bounds.size.height.0, measured, &commands, &id, failed);
                        }
                    })
                    .into_element();
            }
            let mut panel = view()
                .w_full()
                .px(px(Layout::CONVERSATION_PADDING))
                .py_2()
                .flex_col()
                .gap_2()
                .text_size(px(Typography::BODY))
                .text_color(crate::nsh::color(snapshot.colors.text_secondary));
            if snapshot.draft {
                panel = panel
                    .rounded(px(Layout::MENU_RADIUS))
                    .border(px(1.))
                    .border_color(crate::nsh::color(snapshot.colors.border_subtle))
                    .bg(crate::nsh::color(snapshot.colors.bg_elevated));
                if let Some(close) = snapshot
                    .controls()
                    .into_iter()
                    .find(|control| control.action == Action::NotNow)
                {
                    panel = panel.child(control_view(
                        state,
                        snapshot.clone(),
                        close,
                        viewport,
                        attempted,
                        commands.clone(),
                        failed,
                    ));
                }
            }
            if snapshot.recovery {
                panel = panel.child(text(vega_i18n::t("ui.command_failed")));
            }
            for command in &snapshot.active {
                let label = match command.state.as_str() {
                    "stopping" => "ui.shell_session.stopping",
                    "finishing" => "ui.shell_session.finishing",
                    _ => "ui.shell_session.running",
                };
                let action = Action::Cancel(command.operation.clone());
                if let Some(control) = snapshot
                    .controls()
                    .into_iter()
                    .find(|control| control.action == action)
                {
                    panel = panel.child(
                        view()
                            .flex_row()
                            .flex_wrap()
                            .gap_2()
                            .child(text(vega_i18n::t(label)))
                            .child(control_view(
                                state,
                                snapshot.clone(),
                                control,
                                viewport,
                                attempted,
                                commands.clone(),
                                failed,
                            )),
                    );
                }
            }
            if !snapshot.rows.is_empty() {
                panel = panel.child(
                    text(vega_i18n::t("ui.shell_recovery.paused"))
                        .text_color(crate::nsh::color(snapshot.colors.danger)),
                );
            }
            if let Some(label) = snapshot.pending {
                panel = panel.child(text(vega_i18n::t(label)));
            }
            if let Some(error) = snapshot.error {
                panel = panel.child(text(vega_i18n::t(match error {
                    ShellRecoveryError::Changed => "ui.shell_recovery.changed",
                    ShellRecoveryError::KnownProcessOrResource => "ui.shell_recovery.unsettled",
                    ShellRecoveryError::Unavailable => "ui.shell_recovery.unavailable",
                })));
            }
            if snapshot.opened && !snapshot.recovery {
                panel = panel.child(text(snapshot.workspace.clone()));
                for (index, row) in snapshot.rows.iter().enumerate() {
                    let command = if row.command.is_empty() {
                        vega_i18n::t_args(
                            "ui.shell_recovery.operation",
                            &[("number", &(index + 1).to_string())],
                        )
                    } else {
                        row.command.clone()
                    };
                    let result = match row.exit_code {
                        Some(code) => vega_i18n::t_args(
                            "ui.shell_recovery.exit",
                            &[("code", &code.to_string())],
                        ),
                        None => vega_i18n::t("ui.shell_recovery.unknown").to_owned(),
                    };
                    let mut item = view()
                        .flex_col()
                        .gap_1()
                        .child(text(command))
                        .child(text(result))
                        .child(text(super::recovery_reason(&row.reason)));
                    if row.output_incomplete && row.can_resume {
                        item = item.child(text(vega_i18n::t("ui.shell_recovery.output_closed")));
                    }
                    if !row.can_resume {
                        item = item.child(text(vega_i18n::t("ui.shell_recovery.unsettled")));
                    }
                    panel = panel.child(item);
                }
                if snapshot.has_confirmation {
                    panel = panel
                        .child(text(vega_i18n::t("ui.shell_recovery.confirm_title")))
                        .child(text(vega_i18n::t("ui.shell_recovery.confirm_body")));
                    if snapshot.rows.iter().any(|row| row.output_incomplete) {
                        panel = panel.child(text(vega_i18n::t("ui.shell_recovery.confirm_output")));
                    }
                }
            }
            let mut controls = view().flex_row().flex_wrap().gap_2();
            for control in snapshot.controls().into_iter().filter(|control| {
                !matches!(control.action, Action::Cancel(_))
                    && !(snapshot.draft && control.action == Action::NotNow)
            }) {
                controls = controls.child(control_view(
                    state,
                    snapshot.clone(),
                    control,
                    viewport,
                    attempted,
                    commands.clone(),
                    failed,
                ));
            }
            panel
                .child(controls)
                .on_layout({
                    let commands = layout_commands.clone();
                    let id = layout_id.clone();
                    move |bounds: &Bounds<Points>| {
                        report_height(bounds.size.height.0, measured, &commands, &id, failed);
                    }
                })
                .into_element()
        }))
        .into_element()
}

fn report_height(
    height: f32,
    measured: Signal<f32>,
    commands: &mpsc::Sender<Request>,
    id: &ProjectionId,
    failed: Signal<bool>,
) {
    if height.is_finite() && height > 0. && (measured.get_untracked() - height).abs() > 0.5 {
        measured.set(height);
        let commands = commands.clone();
        let id = id.clone();
        spawn(async move {
            if commands.send(Request::Layout(id, height)).await.is_err() {
                failed.set(true);
            }
        });
    }
}

fn control_view(
    state: Signal<Snapshot>,
    observed: Snapshot,
    control: Control,
    viewport: reveal::Viewport,
    attempted: reveal::Tracker,
    commands: mpsc::Sender<Request>,
    failed: Signal<bool>,
) -> AnyElement {
    let target = element_ref();
    let layout = signal(None::<Bounds<Points>>);
    let action = control.action;
    viewport.observe(state, attempted, commands.clone(), action.clone(), target);
    let ring_action = action.clone();
    let state_action = action.clone();
    let enabled = control.enabled;
    view()
        .element_ref(&target)
        .on_layout(move |bounds: &Bounds<Points>| {
            if layout.get_untracked() != Some(*bounds) {
                layout.set(Some(*bounds));
                viewport.invalidate();
            }
        })
        .role(AccessibilityRole::Button)
        .label(vega_i18n::t(control.key))
        .disabled(!enabled)
        .accessibility_state(move || AccessibilityState {
            selected: state.get().keyboard_active
                && state.get().selected.as_ref() == Some(&state_action),
            disabled: !enabled,
            expanded: (state_action == Action::Review).then_some(state.get().opened),
            ..Default::default()
        })
        .px_2()
        .py_1()
        .rounded(px(Layout::PANEL_RADIUS))
        .border(px(1.))
        .border_color(move || {
            crate::nsh::color(
                if state.get().keyboard_active
                    && state.get().selected.as_ref() == Some(&ring_action)
                {
                    state.get().colors.accent
                } else {
                    state.get().colors.bg_base
                },
            )
        })
        .text_color(crate::nsh::color(if enabled {
            observed.colors.text_secondary
        } else {
            observed.colors.text_tertiary
        }))
        .hover(move |style| {
            style.bg(crate::nsh::color(if enabled {
                state.get().colors.bg_hover
            } else {
                state.get().colors.bg_base
            }))
        })
        .active(move |style| {
            style.bg(crate::nsh::color(if enabled {
                state.get().colors.bg_active
            } else {
                state.get().colors.bg_base
            }))
        })
        .on_press(move || {
            if !enabled {
                return;
            }
            let commands = commands.clone();
            let observed = observed.clone();
            let action = action.clone();
            spawn(async move {
                if commands
                    .send(Request::Activate(Box::new(observed), action))
                    .await
                    .is_err()
                {
                    failed.set(true);
                }
            });
        })
        .child(text(vega_i18n::t(control.key)))
        .into_element()
}
