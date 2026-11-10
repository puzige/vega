use std::cell::Cell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use gpui_kit::{App, Context, Focusable, Window};
use nsh_core::prelude::*;
use tokio::sync::oneshot;
use vega_theme::{Layout, ThemeColors, Typography};

use super::{
    AutomationEditSubmission, AutomationEditor, AutomationEditorEvent, AutomationEditorKey,
};
use crate::nsh::{OrderedCommands, ProjectionId, Surface};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Values {
    text: String,
    composing: bool,
    revision: u64,
    epoch: u64,
    frozen: bool,
}

impl Values {
    fn valid(&self) -> bool {
        !self.frozen && !self.composing && !self.text.trim().is_empty() && self.text.len() <= 16384
    }
}

#[derive(Clone)]
pub(super) struct Draft {
    values: Arc<Mutex<Values>>,
    changed: Arc<AtomicBool>,
}

impl Draft {
    pub(super) fn new(text: String) -> Self {
        Self {
            values: Arc::new(Mutex::new(Values {
                text,
                composing: false,
                revision: 0,
                epoch: 0,
                frozen: false,
            })),
            changed: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(super) fn read(&self) -> Values {
        self.values
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub(super) fn text(&self) -> String {
        self.read().text
    }

    pub(super) fn can_save(&self, base: &str) -> bool {
        let values = self.read();
        values.valid() && values.text != base
    }

    fn edit(&self, epoch: u64, text: Option<&str>, composing: Option<bool>) -> bool {
        let mut values = self.values.lock().unwrap_or_else(|e| e.into_inner());
        if values.epoch != epoch || values.frozen {
            return false;
        }
        if let Some(text) = text {
            values.text = text.into();
        }
        if let Some(composing) = composing {
            values.composing = composing;
        }
        values.revision = values.revision.wrapping_add(1);
        true
    }

    pub(super) fn reset(&self, text: String) {
        let mut values = self.values.lock().unwrap_or_else(|e| e.into_inner());
        values.text = text;
        values.composing = false;
        values.frozen = false;
        values.revision = values.revision.wrapping_add(1);
        values.epoch = values.epoch.wrapping_add(1);
    }

    pub(super) fn rebase_if_clean(&self, base: &str, text: &str) -> bool {
        let mut values = self.values.lock().unwrap_or_else(|e| e.into_inner());
        if values.text != base || values.composing || values.frozen {
            return false;
        }
        if values.text != text {
            values.text = text.into();
            values.revision = values.revision.wrapping_add(1);
            values.epoch = values.epoch.wrapping_add(1);
        }
        true
    }

    pub(super) fn claim(&self, observed: &Values, base: &str) -> Option<String> {
        let mut values = self.values.lock().unwrap_or_else(|e| e.into_inner());
        if *values != *observed || !values.valid() || values.text == base {
            return None;
        }
        values.frozen = true;
        Some(values.text.clone())
    }

    fn claim_revert(&self, observed: &Values) -> bool {
        let mut values = self.values.lock().unwrap_or_else(|e| e.into_inner());
        if *values != *observed || values.frozen {
            return false;
        }
        values.frozen = true;
        true
    }

    pub(super) fn release(&self) {
        self.values.lock().unwrap_or_else(|e| e.into_inner()).frozen = false;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Authority {
    key: AutomationEditorKey,
    revision: u64,
    base: String,
    latest: Option<(AutomationEditorKey, u64)>,
    pending: Option<AutomationEditSubmission>,
    request: u64,
}

impl Authority {
    fn conflict(&self) -> bool {
        self.latest
            .as_ref()
            .is_some_and(|(key, revision)| *key != self.key || *revision != self.revision)
    }
}

#[derive(Clone)]
struct Snapshot {
    id: ProjectionId,
    authority: Authority,
    draft: Draft,
    error: Option<String>,
    details: Vec<String>,
    colors: ThemeColors,
    focus: u64,
}

#[derive(Clone, Copy)]
enum Action {
    Save,
    Revert,
    Close,
}

struct Decision {
    snapshot: Snapshot,
    observed: Values,
    action: Action,
    reply: oneshot::Sender<bool>,
}

enum Request {
    Changed(ProjectionId),
    Decide(Box<Decision>),
}

pub(super) struct Presentation {
    surface: Surface<Snapshot, Request>,
    id: ProjectionId,
    focus: u64,
}

fn allowed(snapshot: &Snapshot, values: &Values, action: Action) -> bool {
    match action {
        Action::Save => {
            snapshot.authority.latest.is_some()
                && !snapshot.authority.conflict()
                && snapshot.authority.pending.is_none()
                && values.valid()
                && values.text != snapshot.authority.base
        }
        Action::Revert => {
            snapshot.authority.pending.is_none()
                && !values.frozen
                && snapshot.authority.latest.is_some()
                && (values.text != snapshot.authority.base
                    || snapshot.authority.conflict()
                    || snapshot.error.is_some())
        }
        Action::Close => !values.composing,
    }
}

fn accepts(current: &Snapshot, observed: &Snapshot) -> bool {
    current.id.matches(&observed.id) && current.authority == observed.authority
}

impl AutomationEditor {
    fn nsh_snapshot(&self, id: ProjectionId, focus: u64, cx: &App) -> Snapshot {
        Snapshot {
            id,
            authority: Authority {
                key: self.key(),
                revision: self.base.revision,
                base: self.base.instruction.clone(),
                latest: self
                    .latest
                    .as_ref()
                    .map(|task| (AutomationEditorKey::for_task(task), task.revision)),
                pending: self.pending.clone(),
                request: self.request,
            },
            draft: self.draft.clone(),
            error: self.error.as_ref().map(|error| error.render()),
            details: self.details(),
            colors: vega_theme::theme(cx).colors,
            focus,
        }
    }

    pub(super) fn sync_carrier(&mut self, cx: &mut Context<Self>) {
        let text = self.draft.text();
        self.carrier_text = text.clone();
        if self.input.read(cx).text() != text {
            self.input.update(cx, |input, cx| input.set_text(&text, cx));
        }
    }

    pub(super) fn observe_carrier(&mut self, cx: &mut Context<Self>) {
        let text = self.input.read(cx).text().to_string();
        if text != self.carrier_text {
            self.carrier_text = text.clone();
            if self.pending.is_none() {
                self.draft.reset(text);
            } else {
                self.sync_carrier(cx);
            }
        }
    }

    fn handle_nsh(&mut self, request: Request, cx: &mut Context<Self>) {
        match request {
            Request::Changed(id) => {
                if !self
                    .presentation
                    .as_ref()
                    .is_some_and(|page| page.id.matches(&id))
                {
                    return;
                }
                self.draft.changed.store(false, Ordering::Release);
                self.observe_carrier(cx);
                self.sync_carrier(cx);
                cx.emit(AutomationEditorEvent::Changed);
            }
            Request::Decide(decision) => {
                self.observe_carrier(cx);
                let current = self
                    .presentation
                    .as_ref()
                    .map(|page| self.nsh_snapshot(page.id.clone(), page.focus, cx));
                let accepted = current.as_ref().is_some_and(|current| {
                    accepts(current, &decision.snapshot)
                        && allowed(current, &decision.observed, decision.action)
                });
                let accepted = accepted
                    && match decision.action {
                        Action::Save => self.save_observed(&decision.observed, cx),
                        Action::Revert => {
                            if self.draft.claim_revert(&decision.observed) {
                                self.revert(cx);
                                self.draft.release();
                                true
                            } else {
                                false
                            }
                        }
                        Action::Close => {
                            if self.draft.read() == decision.observed {
                                self.sync_carrier(cx);
                                cx.emit(AutomationEditorEvent::Close);
                                true
                            } else {
                                false
                            }
                        }
                    };
                let _ = decision.reply.send(accepted);
            }
        }
        self.publish_nsh(cx);
        cx.notify();
    }

    pub(super) fn publish_nsh(&self, cx: &App) {
        if let Some(page) = &self.presentation {
            page.surface
                .publish(self.nsh_snapshot(page.id.clone(), page.focus, cx));
        }
    }

    pub(super) fn render_nsh(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Entity<nsh_platform_gpui::AppView> {
        self.observe_carrier(cx);
        self.publish_nsh(cx);
        if let Some(page) = &mut self.presentation {
            page.surface.sync_environment(cx);
            return page.surface.entity();
        }
        {
            let id = ProjectionId::new();
            let focus = self.input.read(cx).focus_handle(cx);
            let initial_focus = u64::from(focus.is_focused(window));
            let (surface, mut requests) = Surface::new(
                self.nsh_snapshot(id.clone(), initial_focus, cx),
                render,
                window,
                cx,
            );
            let entity = surface.entity();
            self.presentation = Some(Presentation {
                surface,
                id,
                focus: initial_focus,
            });
            cx.on_focus(&focus, window, |this, window, cx| {
                if let Some(page) = &mut this.presentation {
                    page.focus = page.focus.wrapping_add(1);
                    page.surface.focus(window, cx);
                }
                this.publish_nsh(cx);
            })
            .detach();
            cx.spawn_in(window, async move |this, cx| {
                while let Some(request) = requests.recv().await {
                    if this
                        .update_in(cx, |this, _, cx| this.handle_nsh(request, cx))
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();
            entity
        }
    }
}

#[derive(Default)]
struct FocusRequests(Cell<u64>);
impl FocusRequests {
    fn consume(&self, request: u64, mounted: bool) -> bool {
        if !mounted || request == self.0.get() {
            return false;
        }
        self.0.set(request);
        true
    }
}

#[derive(Clone)]
struct Controls {
    focus_requests: Rc<FocusRequests>,
    state: Signal<Snapshot>,
    edits: Signal<u64>,
    pending: Signal<bool>,
    failed: Signal<bool>,
    commands: OrderedCommands<Request>,
}

impl Controls {
    fn values(&self) -> Values {
        self.edits.get();
        self.state.get().draft.read()
    }

    fn changed(&self, draft: &Draft, id: ProjectionId) {
        self.edits
            .update(|revision| *revision = revision.wrapping_add(1));
        if !draft.changed.swap(true, Ordering::AcqRel) {
            self.commands.send(Request::Changed(id));
        }
    }

    fn send(&self, action: Action) {
        let snapshot = self.state.get_untracked();
        let observed = snapshot.draft.read();
        if self.pending.get_untracked() || !allowed(&snapshot, &observed, action) {
            return;
        }
        self.pending.set(true);
        self.failed.set(false);
        let (reply, response) = oneshot::channel();
        self.commands.send(Request::Decide(Box::new(Decision {
            snapshot,
            observed,
            action,
            reply,
        })));
        let pending = self.pending;
        let failed = self.failed;
        spawn(async move {
            failed.set(!matches!(response.await, Ok(true)));
            pending.set(false);
        });
    }
}

fn action_label(state: Signal<Snapshot>, action: Action) -> &'static str {
    state.with(|snapshot| {
        vega_i18n::t(match action {
            Action::Save => "ui.save",
            Action::Revert if snapshot.authority.conflict() => "ui.load_latest",
            Action::Revert => "ui.revert",
            Action::Close => "ui.close",
        })
    })
}

fn button(controls: Controls, action: Action) -> AnyElement {
    let state = controls.state;
    let disabled = controls.clone();
    let pressed = controls.clone();
    let foreground = controls.clone();
    let background = controls.clone();
    let focused = signal(false);
    let target = element_ref();
    target.on_focus_in(move || focused.set(true));
    target.on_focus_out(move || focused.set(false));
    view()
        .element_ref(&target)
        .role(AccessibilityRole::Button)
        .label(move || action_label(state, action))
        .focusable(true)
        .disabled(move || {
            disabled.pending.get() || !allowed(&state.get(), &disabled.values(), action)
        })
        .h(px(Layout::AUTOMATION_ACTION_SIZE))
        .px_3()
        .flex_row()
        .items_center()
        .rounded(px(Layout::AUTOMATION_ACTION_SIZE / 2.))
        .border(px(1.))
        .border_color(move || {
            crate::nsh::color(if focused.get() {
                state.get().colors.accent
            } else {
                state.get().colors.border_subtle
            })
        })
        .text_color(move || {
            let snapshot = state.get();
            let enabled =
                !foreground.pending.get() && allowed(&snapshot, &foreground.values(), action);
            crate::nsh::color(if !enabled {
                snapshot.colors.text_tertiary
            } else if matches!(action, Action::Save) {
                snapshot.colors.bg_base
            } else {
                snapshot.colors.text_primary
            })
        })
        .bg(move || {
            let snapshot = state.get();
            let enabled =
                !background.pending.get() && allowed(&snapshot, &background.values(), action);
            crate::nsh::color(if enabled && matches!(action, Action::Save) {
                snapshot.colors.text_primary
            } else {
                snapshot.colors.bg_base
            })
        })
        .hover(|style| {
            style.bg(move || {
                crate::nsh::color(if matches!(action, Action::Save) {
                    state.get().colors.text_secondary
                } else {
                    state.get().colors.bg_hover
                })
            })
        })
        .active(|style| {
            style.bg(move || {
                crate::nsh::color(if matches!(action, Action::Save) {
                    state.get().colors.text_secondary
                } else {
                    state.get().colors.bg_active
                })
            })
        })
        .on_press(move |_: &PressEvent| pressed.send(action))
        .child(text(move || action_label(state, action)))
        .into_element()
}

fn error(snapshot: &Snapshot, values: &Values) -> Option<String> {
    if snapshot.authority.latest.is_none() {
        Some(vega_i18n::t("ui.goal_cleared").into())
    } else if snapshot.authority.conflict() {
        Some(vega_i18n::t("ui.goal_updated_load_the_latest_content_and_try").into())
    } else if values.text != snapshot.authority.base && values.text.trim().is_empty() {
        Some(vega_i18n::t("ui.enter_task_content").into())
    } else if values.text.len() > 16384 {
        Some(vega_i18n::t("ui.task_content_is_limited_to_16_kib_shorten").into())
    } else {
        snapshot.error.clone()
    }
}

fn field(controls: Controls, epoch: u64) -> AnyElement {
    let state = controls.state;
    let initial = state.get_untracked().draft.read();
    let value = signal(initial.text);
    let input_ref = element_ref();
    let focused = signal(false);
    input_ref.on_focus_in(move || focused.set(true));
    input_ref.on_focus_out(move || focused.set(false));
    let last_focus = controls.focus_requests.clone();
    let focus_effect = last_focus.clone();
    effect(move || {
        let next = state.get().focus;
        if focus_effect.consume(next, input_ref.node().is_some()) {
            input_ref.focus();
        }
    });
    let edit = controls.clone();
    let composition = controls.clone();
    nsh_base::input()
        .element_ref(&input_ref)
        .label(move || state.with(|_| vega_i18n::t("ui.task_content")))
        .placeholder(move || state.with(|_| vega_i18n::t("ui.task_content")))
        .value(value)
        .multiline(true)
        .submit_behavior(nsh_core::SubmitBehavior::Newline)
        .text_size(px(Typography::BODY))
        .line_height(px(Typography::BODY * Typography::BODY_LINE_HEIGHT))
        .w_full()
        .flex_1()
        .min_h(px(Typography::BODY * Typography::BODY_LINE_HEIGHT * 2.))
        .border(px(1.))
        .rounded(px(Layout::PANEL_RADIUS))
        .p_3()
        .border_color(move || {
            crate::nsh::color(if focused.get() {
                state.get().colors.accent
            } else {
                state.get().colors.border_subtle
            })
        })
        .bg(move || crate::nsh::color(state.get().colors.bg_elevated))
        .readonly(move || controls.pending.get() || controls.values().frozen)
        .on_layout(move |_: &Bounds<Points>| {
            let next = state.get_untracked().focus;
            if last_focus.consume(next, true) {
                input_ref.focus();
            }
        })
        .on_change(move |text: &str| {
            let snapshot = state.get_untracked();
            if snapshot.draft.edit(epoch, Some(text), None) {
                value.set(text.to_owned());
                edit.changed(&snapshot.draft, snapshot.id);
            }
        })
        .on_composition_change(move |event: &Composition| {
            let snapshot = state.get_untracked();
            if snapshot.draft.edit(epoch, None, Some(event.composing)) {
                composition.changed(&snapshot.draft, snapshot.id);
            }
        })
        .into_element()
}

fn render(state: Signal<Snapshot>, sender: tokio::sync::mpsc::Sender<Request>) -> AnyElement {
    let controls = Controls {
        focus_requests: Rc::new(FocusRequests::default()),
        state,
        edits: signal(0),
        pending: signal(false),
        failed: signal(false),
        commands: OrderedCommands::new(sender),
    };
    let keys = controls.clone();
    effect(move || {
        if !keys.values().composing {
            let save = keys.clone();
            on_key_with(
                "mod+s",
                KeyOptions {
                    when: nsh_core::keymap::When::Always,
                    ..Default::default()
                },
                move |_| save.send(Action::Save),
            );
            let close = keys.clone();
            on_key_with(
                "escape",
                KeyOptions {
                    when: nsh_core::keymap::When::Always,
                    ..Default::default()
                },
                move |_| close.send(Action::Close),
            );
        }
    });
    let field_controls = controls.clone();
    let status = controls.clone();
    let validation = controls.clone();
    let errors = controls.clone();
    view()
        .w_full()
        .h_full()
        .min_w_0()
        .min_h_0()
        .flex_col()
        .text_size(px(Typography::BODY))
        .text_color(move || crate::nsh::color(state.get().colors.text_primary))
        .bg(move || crate::nsh::color(state.get().colors.bg_base))
        .child(
            view()
                .flex_row()
                .flex_wrap()
                .items_center()
                .gap_2()
                .p_3()
                .child(
                    text(move || {
                        let snapshot = state.get();
                        let values = status.values();
                        if snapshot.authority.pending.is_some() {
                            vega_i18n::t("ui.saving")
                        } else if values.text != snapshot.authority.base {
                            vega_i18n::t("ui.unsaved")
                        } else {
                            ""
                        }
                    })
                    .flex_1()
                    .text_color(move || crate::nsh::color(state.get().colors.text_secondary)),
                )
                .child(button(controls.clone(), Action::Revert))
                .child(button(controls.clone(), Action::Save)),
        )
        .child(when(
            move || error(&state.get(), &validation.values()).is_some(),
            move || {
                let errors = errors.clone();
                text(move || error(&state.get(), &errors.values()).unwrap_or_default())
                    .px_3()
                    .text_color(move || crate::nsh::color(state.get().colors.danger))
            },
        ))
        .child(when(
            {
                let controls = controls.clone();
                move || controls.failed.get() || controls.commands.failed.get()
            },
            move || {
                text(move || state.with(|_| vega_i18n::t("ui.command_failed")))
                    .px_3()
                    .text_color(move || crate::nsh::color(state.get().colors.danger))
            },
        ))
        .child(
            view()
                .flex_1()
                .min_h_0()
                .w_full()
                .p_3()
                .gap_3()
                .child(
                    text(move || state.with(|_| vega_i18n::t("ui.task_content")))
                        .text_color(move || crate::nsh::color(state.get().colors.text_secondary)),
                )
                .child(each(
                    move || vec![state.get().draft.read().epoch],
                    |epoch| *epoch,
                    move |epoch, _| field(field_controls.clone(), epoch.get_untracked()),
                ))
                .child(
                    scroll().w_full().max_h(px(112.)).min_h_0().child(
                        view()
                            .gap_1()
                            .text_size(px(Typography::METADATA))
                            .text_color(move || {
                                crate::nsh::color(state.get().colors.text_secondary)
                            })
                            .child(each(
                                move || state.get().details,
                                |detail| detail.clone(),
                                |detail, _| text(move || detail.get()),
                            )),
                    ),
                ),
        )
        .into_element()
}
