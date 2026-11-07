use std::sync::{Arc, Mutex};

use gpui_kit::{App, Context, Window};
use nsh_core::prelude::*;
use tokio::sync::{mpsc, oneshot};
use vega_conversation::types::{
    PricingDraftReason, PricingEntryKind, PricingEntryProjection, PricingMutation,
    PricingSettingsProjection,
};
use vega_theme::{Layout, ThemeColors, Typography};

use super::{PricingEditor, PricingEditorKind, SettingsView};
use crate::nsh::{ProjectionId, Surface};

#[derive(Clone, PartialEq, Eq)]
struct Values {
    text: [String; 9],
    composing: [bool; 9],
    revision: u64,
    frozen: bool,
}

#[derive(Clone)]
pub(super) struct Draft {
    session: ProjectionId,
    values: Arc<Mutex<Values>>,
}

impl Draft {
    fn new(text: [String; 9]) -> Self {
        Self {
            session: ProjectionId::new(),
            values: Arc::new(Mutex::new(Values {
                text,
                composing: [false; 9],
                revision: 0,
                frozen: false,
            })),
        }
    }

    fn read(&self) -> Values {
        self.values
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    fn edit(&self, index: usize, text: Option<&str>, composing: Option<bool>) -> bool {
        let mut values = self
            .values
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if values.frozen || index >= values.text.len() {
            return false;
        }
        if let Some(text) = text {
            values.text[index] = text.to_owned();
        }
        if let Some(composing) = composing {
            values.composing[index] = composing;
        }
        values.revision = values.revision.wrapping_add(1);
        true
    }

    fn claim(&self, expected: Option<&Values>) -> Option<[String; 9]> {
        let mut values = self
            .values
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if values.frozen
            || values.composing.contains(&true)
            || expected.is_some_and(|expected| *expected != *values)
        {
            return None;
        }
        values.frozen = true;
        Some(values.text.clone())
    }

    fn release(&self) {
        self.values
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .frozen = false;
    }
}

pub(super) struct Presentation {
    surface: Surface<Snapshot, Request>,
    id: ProjectionId,
    height: f32,
}

#[derive(Clone)]
struct Editor {
    descriptor: PricingEditor,
    draft: Draft,
    observed: Values,
}

impl PartialEq for Editor {
    fn eq(&self, other: &Self) -> bool {
        self.descriptor == other.descriptor
            && self.draft.session.matches(&other.draft.session)
            && self.observed == other.observed
    }
}

#[derive(Clone)]
struct Snapshot {
    id: ProjectionId,
    projection: Arc<PricingSettingsProjection>,
    route: Option<(String, String)>,
    visible: bool,
    advanced: bool,
    editor: Option<Editor>,
    colors: ThemeColors,
}

#[derive(Clone)]
enum Action {
    Reload,
    Add,
    Edit(Box<PricingEntryProjection>),
    Secondary(Box<PricingEntryProjection>),
    Retry,
    Discard,
    Save,
    Cancel,
    Advanced,
}

struct Decision {
    observed: Snapshot,
    action: Action,
    reply: oneshot::Sender<bool>,
}
enum Request {
    Decide(Box<Decision>),
    Layout(f32),
}

fn editing(snapshot: &Snapshot) -> bool {
    matches!(
        &*snapshot.projection,
        PricingSettingsProjection::Ready {
            draft_reason: None,
            ..
        }
    )
}

fn allowed(snapshot: &Snapshot, action: &Action) -> bool {
    match action {
        Action::Reload => !matches!(
            &*snapshot.projection,
            PricingSettingsProjection::Saving { .. }
                | PricingSettingsProjection::Loading
                | PricingSettingsProjection::Reloading
        ),
        Action::Retry | Action::Discard => matches!(
            &*snapshot.projection,
            PricingSettingsProjection::Ready {
                draft_reason: Some(_),
                ..
            }
        ),
        Action::Add => editing(snapshot) && snapshot.editor.is_none(),
        Action::Edit(entry) | Action::Secondary(entry) => {
            editing(snapshot)
                && snapshot.editor.is_none()
                && snapshot
                    .route
                    .as_ref()
                    .is_none_or(|(_, model)| *model == entry.model)
                && matches!(&*snapshot.projection, PricingSettingsProjection::Ready { entries, .. } if entries.contains(entry))
        }
        Action::Save => {
            editing(snapshot)
                && snapshot.editor.as_ref().is_some_and(|editor| {
                    !editor.observed.frozen && !editor.observed.composing.contains(&true)
                })
        }
        Action::Cancel => {
            editing(snapshot)
                && snapshot
                    .editor
                    .as_ref()
                    .is_some_and(|editor| !editor.observed.frozen)
        }
        Action::Advanced => {
            editing(snapshot)
                && snapshot.editor.as_ref().is_some_and(|editor| {
                    editor.descriptor.kind == PricingEditorKind::UpdateDeepSeek
                        && !editor.observed.frozen
                        && !editor.observed.composing.contains(&true)
                })
        }
    }
}

fn same_editor(current: &Option<Editor>, observed: &Option<Editor>) -> bool {
    match (current, observed) {
        (None, None) => true,
        (Some(current), Some(observed)) => {
            current.descriptor == observed.descriptor
                && current.draft.session.matches(&observed.draft.session)
                && current.observed == observed.observed
        }
        _ => false,
    }
}

fn accepts(current: &Snapshot, observed: &Snapshot, action: &Action) -> bool {
    current.visible
        && current.route == observed.route
        && current.projection == observed.projection
        && current.advanced == observed.advanced
        && same_editor(&current.editor, &observed.editor)
        && allowed(current, action)
}

impl SettingsView {
    pub(super) fn reset_pricing_draft(&mut self, cx: &App) {
        self.pricing_nsh_draft = self.pricing_editor.as_ref().map(|_| {
            Draft::new(std::array::from_fn(|index| {
                if index == 0 {
                    self.pricing_model_input.read(cx).text().to_string()
                } else {
                    self.pricing_rate_inputs[index - 1]
                        .read(cx)
                        .text()
                        .to_string()
                }
            }))
        });
    }

    pub(super) fn sync_pricing_draft_for_save(&mut self, cx: &mut Context<Self>) -> bool {
        self.claim_pricing_draft(None, cx)
    }

    fn claim_pricing_draft(&mut self, expected: Option<&Values>, cx: &mut Context<Self>) -> bool {
        if !self.pricing_allows_editing() {
            return false;
        }
        let Some(draft) = &self.pricing_nsh_draft else {
            return false;
        };
        let Some(values) = draft.claim(expected) else {
            return false;
        };
        self.pricing_model_input
            .update(cx, |input, cx| input.set_text(&values[0], cx));
        for (input, value) in self.pricing_rate_inputs.iter().zip(&values[1..]) {
            input.update(cx, |input, cx| input.set_text(value, cx));
        }
        true
    }

    pub(super) fn release_pricing_draft(&self) {
        if let Some(draft) = &self.pricing_nsh_draft {
            draft.release();
        }
    }

    fn pricing_snapshot(&self, id: ProjectionId, cx: &App) -> Snapshot {
        Snapshot {
            id,
            projection: Arc::new(self.pricing.clone()),
            route: self.model_settings.clone(),
            visible: self.section == 3
                && cx
                    .try_global::<super::SettingsOpen>()
                    .is_some_and(|open| open.0),
            advanced: self.pricing_advanced,
            editor: self
                .pricing_editor
                .clone()
                .zip(self.pricing_nsh_draft.clone())
                .map(|(descriptor, draft)| Editor {
                    descriptor,
                    observed: draft.read(),
                    draft,
                }),
            colors: vega_theme::theme(cx).colors,
        }
    }

    pub(super) fn sync_pricing_presentation(&self, cx: &App) {
        if let Some(page) = &self.pricing_presentation {
            page.surface
                .publish(self.pricing_snapshot(page.id.clone(), cx));
        }
    }

    pub(crate) fn render_pricing(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        use gpui_kit::{IntoElement, ParentElement, Styled};
        if self.pricing_presentation.is_none() {
            let id = ProjectionId::new();
            let (surface, mut requests) =
                Surface::new(self.pricing_snapshot(id.clone(), cx), render, window, cx);
            self.pricing_presentation = Some(Presentation {
                surface,
                id,
                height: Layout::SETTINGS_NOTES_MAX_HEIGHT,
            });
            cx.spawn_in(window, async move |this, cx| {
                while let Some(request) = requests.recv().await {
                    if this
                        .update_in(cx, |this, _, cx| {
                            match request {
                                Request::Decide(command) => {
                                    let mut accepted =
                                        this.pricing_presentation.as_ref().is_some_and(|page| {
                                            page.id.matches(&command.observed.id)
                                                && accepts(
                                                    &this.pricing_snapshot(page.id.clone(), cx),
                                                    &command.observed,
                                                    &command.action,
                                                )
                                        });
                                    if accepted {
                                        match command.action {
                                            Action::Reload => {
                                                cx.emit(super::PricingReloadRequested)
                                            }
                                            Action::Add => this.begin_add_custom(cx),
                                            Action::Edit(entry) => {
                                                this.begin_edit_pricing(*entry, cx)
                                            }
                                            Action::Secondary(entry) => this.emit_pricing_mutation(
                                                if entry.kind == PricingEntryKind::CustomStatic {
                                                    PricingMutation::DeleteCustom {
                                                        model: entry.model,
                                                    }
                                                } else {
                                                    PricingMutation::ResetBuiltin {
                                                        model: entry.model,
                                                    }
                                                },
                                                cx,
                                            ),
                                            Action::Retry => {
                                                if let Some(generation) = this.pricing_generation()
                                                {
                                                    cx.emit(super::PricingRetryRequested {
                                                        generation,
                                                    });
                                                }
                                            }
                                            Action::Discard => {
                                                if let Some(generation) = this.pricing_generation()
                                                {
                                                    cx.emit(super::PricingDiscardRequested {
                                                        generation,
                                                    });
                                                }
                                            }
                                            Action::Save => {
                                                accepted = this.claim_pricing_draft(
                                                    command
                                                        .observed
                                                        .editor
                                                        .as_ref()
                                                        .map(|editor| &editor.observed),
                                                    cx,
                                                );
                                                if accepted {
                                                    this.submit_pricing_editor_values(cx);
                                                }
                                            }
                                            Action::Cancel => {
                                                this.pricing_editor = None;
                                                this.pricing_nsh_draft = None;
                                            }
                                            Action::Advanced => {
                                                this.pricing_advanced = !this.pricing_advanced
                                            }
                                        }
                                        cx.notify();
                                    }
                                    let _ = command.reply.send(accepted);
                                }
                                Request::Layout(height) if height.is_finite() && height > 0. => {
                                    if let Some(page) = &mut this.pricing_presentation
                                        && (page.height - height).abs() > 0.5
                                    {
                                        page.height = height;
                                        cx.notify();
                                    }
                                }
                                Request::Layout(_) => {}
                            }
                            this.sync_pricing_presentation(cx);
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();
        }
        let Some(page) = &self.pricing_presentation else {
            return gpui_kit::div().into_any_element();
        };
        let snapshot = self.pricing_snapshot(page.id.clone(), cx);
        let Some(page) = &mut self.pricing_presentation else {
            return gpui_kit::div().into_any_element();
        };
        page.surface.sync_environment(cx);
        page.surface.publish(snapshot);
        gpui_kit::div()
            .w_full()
            .flex_shrink_0()
            .h(gpui_kit::px(page.height))
            .child(page.surface.entity())
            .into_any_element()
    }
}

#[derive(Clone, Copy)]
struct Actions {
    state: Signal<Snapshot>,
    pending: Signal<bool>,
    failed: Signal<bool>,
    edits: Signal<u64>,
}
impl Actions {
    fn snapshot(self) -> Snapshot {
        self.edits.get();
        let mut snapshot = self.state.get();
        if let Some(editor) = &mut snapshot.editor {
            editor.observed = editor.draft.read();
        }
        snapshot
    }

    fn send(self, commands: mpsc::Sender<Request>, action: Action) {
        let observed = self.snapshot();
        if self.pending.get_untracked() || !allowed(&observed, &action) || !observed.visible {
            return;
        }
        self.pending.set(true);
        self.failed.set(false);
        spawn(async move {
            let (reply, response) = oneshot::channel();
            let accepted = commands
                .send(Request::Decide(Box::new(Decision {
                    observed,
                    action,
                    reply,
                })))
                .await
                .is_ok()
                && matches!(response.await, Ok(true));
            self.failed.set(!accepted);
            self.pending.set(false);
        });
    }
}

fn label(snapshot: &Snapshot, action: &Action) -> &'static str {
    vega_i18n::t(match action {
        Action::Reload => "settings.action.reload",
        Action::Add => "settings.pricing.add_custom",
        Action::Edit(_) => "settings.action.edit",
        Action::Secondary(entry) => {
            if entry.kind == PricingEntryKind::CustomStatic {
                "settings.action.delete"
            } else {
                "settings.action.reset"
            }
        }
        Action::Retry => "settings.pricing.retry_draft",
        Action::Discard => "settings.pricing.discard_draft",
        Action::Save => "settings.pricing.save",
        Action::Cancel => "settings.action.cancel",
        Action::Advanced => {
            if snapshot.advanced {
                "settings.pricing.collapse_peak"
            } else {
                "settings.pricing.peak_rates"
            }
        }
    })
}

fn button(
    actions: Actions,
    commands: mpsc::Sender<Request>,
    action: impl Fn() -> Action + Copy + 'static,
) -> AnyElement {
    let target = element_ref();
    let focused = signal(false);
    target.on_focus_in(move || focused.set(true));
    target.on_focus_out(move || focused.set(false));
    view().element_ref(&target).role(AccessibilityRole::Button).focusable(true)
        .label(move || label(&actions.snapshot(), &action()))
        .disabled(move || actions.pending.get() || !allowed(&actions.snapshot(), &action()))
        .flex_row().items_center().gap_2().border(px(1.)).rounded(px(Layout::SETTINGS_CONTROL_RADIUS)).px_3().py_2()
        .border_color(move || crate::nsh::color(if focused.get() { actions.state.get().colors.accent } else { actions.state.get().colors.border_subtle }))
        .bg(move || crate::nsh::color(if matches!(action(), Action::Save) { actions.state.get().colors.brand_primary } else { actions.state.get().colors.bg_elevated }))
        .text_color(move || crate::nsh::color(if matches!(action(), Action::Save) { actions.state.get().colors.brand_on_accent } else if matches!(action(), Action::Secondary(entry) if entry.kind == PricingEntryKind::CustomStatic) { actions.state.get().colors.danger } else { actions.state.get().colors.text_primary }))
        .on_press(move |_: &PressEvent| actions.send(commands.clone(), action()))
        .child(text(move || label(&actions.snapshot(), &action()))).into_element()
}

fn group(state: Signal<Snapshot>) -> nsh_core::element::View {
    view()
        .w_full()
        .gap_3()
        .p(px(Layout::SETTINGS_GROUP_PADDING))
        .border(px(1.))
        .rounded(px(Layout::PANEL_RADIUS))
        .border_color(move || crate::nsh::color(state.get().colors.border_subtle))
        .bg(move || crate::nsh::color(state.get().colors.bg_subtle))
}

const FIELD_KEYS: [&str; 9] = [
    "settings.field.model_id",
    "settings.pricing.base_input",
    "settings.pricing.base_output",
    "settings.pricing.base_cache_read",
    "settings.pricing.base_cache_write",
    "settings.pricing.peak_input",
    "settings.pricing.peak_output",
    "settings.pricing.peak_cache_read",
    "settings.pricing.peak_cache_write",
];

fn field(actions: Actions, editor: Editor, index: usize) -> AnyElement {
    let value = signal(editor.draft.read().text[index].clone());
    let draft = editor.draft.clone();
    let composition = editor.draft.clone();
    let target = element_ref();
    let focused = signal(false);
    target.on_focus_in(move || focused.set(true));
    target.on_focus_out(move || focused.set(false));
    view()
        .gap_1()
        .w_full()
        .child(text(move || {
            actions.state.with(|_| vega_i18n::t(FIELD_KEYS[index]))
        }))
        .child(when(
            move || index > 0,
            move || {
                text(move || {
                    actions
                        .state
                        .with(|_| vega_i18n::t("settings.pricing.units"))
                })
                .text_size(px(Typography::METADATA))
                .text_color(move || crate::nsh::color(actions.state.get().colors.text_secondary))
            },
        ))
        .child(
            nsh_base::input()
                .element_ref(&target)
                .label(move || actions.state.with(|_| vega_i18n::t(FIELD_KEYS[index])))
                .value(move || value.get())
                .max_length(super::PRICING_INPUT_BYTES_LIMIT as u32)
                .read_only(move || {
                    actions.pending.get() || !editing(&actions.state.get()) || {
                        actions.edits.get();
                        editor.draft.read().frozen
                    }
                })
                .border(px(1.))
                .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
                .px_3()
                .py_2()
                .w_full()
                .border_color(move || {
                    crate::nsh::color(if focused.get() {
                        actions.state.get().colors.accent
                    } else {
                        actions.state.get().colors.border_subtle
                    })
                })
                .bg(move || crate::nsh::color(actions.state.get().colors.bg_elevated))
                .on_change(move |text: &str| {
                    if draft.edit(index, Some(text), None) {
                        value.set(text.to_owned());
                        actions
                            .edits
                            .update(|revision| *revision = revision.wrapping_add(1));
                    }
                })
                .on_composition_change(move |event: &Composition| {
                    if composition.edit(index, None, Some(event.composing)) {
                        actions
                            .edits
                            .update(|revision| *revision = revision.wrapping_add(1));
                    }
                }),
        )
        .into_element()
}

fn editor(actions: Actions, commands: mpsc::Sender<Request>, editor: Editor) -> AnyElement {
    let base = editor.clone();
    let peak = editor.clone();
    group(actions.state)
        .child(when(
            {
                let model = editor.descriptor.model.clone();
                move || model.is_some()
            },
            {
                let model = editor.descriptor.model.clone();
                move || text(model.clone().unwrap_or_default())
            },
        ))
        .child(when(
            move || editor.descriptor.kind == PricingEditorKind::AddCustom,
            {
                let editor = editor.clone();
                move || field(actions, editor.clone(), 0)
            },
        ))
        .children((1..5).map(move |index| field(actions, base.clone(), index)))
        .child(when(
            move || peak.descriptor.kind == PricingEditorKind::UpdateDeepSeek,
            {
                let peak = peak.clone();
                let commands = commands.clone();
                move || {
                    view()
                        .gap_3()
                        .child(button(actions, commands.clone(), || Action::Advanced))
                        .child(when(move || actions.state.get().advanced, {
                            let peak = peak.clone();
                            move || {
                                view().gap_3().children((5..9).map({
                                    let peak = peak.clone();
                                    move |index| field(actions, peak.clone(), index)
                                }))
                            }
                        }))
                }
            },
        ))
        .child(
            view()
                .flex_row()
                .flex_wrap()
                .gap_2()
                .child(button(actions, commands.clone(), || Action::Save))
                .child(button(actions, commands, || Action::Cancel)),
        )
        .into_element()
}

fn render(state: Signal<Snapshot>, commands: mpsc::Sender<Request>) -> AnyElement {
    let actions = Actions {
        state,
        pending: signal(false),
        failed: signal(false),
        edits: signal(0),
    };
    let measured = signal(0.0f32);
    let layout = commands.clone();
    let rows = commands.clone();
    let form = commands.clone();
    let status = computed(move || {
        state.with(|snapshot| match &*snapshot.projection {
            PricingSettingsProjection::Loading | PricingSettingsProjection::Reloading => {
                vec![("settings.pricing.loading", false)]
            }
            PricingSettingsProjection::Invalid(_) => Vec::new(),
            PricingSettingsProjection::Saving { .. } => {
                vec![("settings.pricing.saving_verifying", false)]
            }
            PricingSettingsProjection::Ready { draft_reason, .. } => draft_reason
                .map(|reason| {
                    (
                        match reason {
                            PricingDraftReason::RetryPending => "settings.pricing.draft.retry",
                            PricingDraftReason::ExternalConflict => {
                                "settings.pricing.draft.conflict"
                            }
                        },
                        true,
                    )
                })
                .into_iter()
                .collect(),
        })
    });
    let session = signal(None::<ProjectionId>);
    let form_revision = signal(0u64);
    let forms = signal(Vec::<(u64, Editor)>::new());
    effect(move || {
        let snapshot = state.get();
        let previous = session.get_untracked();
        match snapshot.editor {
            Some(editor)
                if previous
                    .as_ref()
                    .is_none_or(|previous| !previous.matches(&editor.draft.session)) =>
            {
                session.set(Some(editor.draft.session.clone()));
                let revision = form_revision.get_untracked().wrapping_add(1);
                form_revision.set(revision);
                forms.set(vec![(revision, editor)]);
            }
            None if previous.is_some() => {
                session.set(None);
                forms.set(Vec::new());
            }
            _ => {}
        }
    });
    view()
        .w_full()
        .flex_shrink_0()
        .gap(px(Layout::SETTINGS_SECTION_GAP))
        .text_size(px(Typography::BODY))
        .text_color(move || crate::nsh::color(state.get().colors.text_primary))
        .on_layout(move |bounds: &Bounds<Points>| {
            let height = bounds.size.height.0;
            if height.is_finite() && height > 0. && (measured.get_untracked() - height).abs() > 0.5
            {
                measured.set(height);
                let commands = layout.clone();
                spawn(async move {
                    if commands.send(Request::Layout(height)).await.is_err() {
                        actions.failed.set(true);
                    }
                });
            }
        })
        .child(text(move || {
            state.with(|snapshot| {
                vega_i18n::t(if snapshot.route.is_some() {
                    "settings.pricing.shared_model"
                } else {
                    "settings.pricing.library"
                })
            })
        }))
        .child(
            view()
                .flex_row()
                .flex_wrap()
                .justify_between()
                .gap_2()
                .child(
                    text(move || state.with(|_| vega_i18n::t("settings.pricing.title")))
                        .text_size(px(Typography::HEADING_BLOCK)),
                )
                .child(button(actions, commands.clone(), || Action::Reload))
                .child(button(actions, commands.clone(), || Action::Add)),
        )
        .child(each(
            move || status.get(),
            |(key, _)| *key,
            move |item, _| {
                text(move || vega_i18n::t(item.get().0)).text_color(move || {
                    crate::nsh::color(if item.get().1 {
                        state.get().colors.warning
                    } else {
                        state.get().colors.text_secondary
                    })
                })
            },
        ))
        .child(when(
            move || {
                matches!(
                    &*state.get().projection,
                    PricingSettingsProjection::Invalid(_)
                ) || matches!(
                    &*state.get().projection,
                    PricingSettingsProjection::Ready { error: Some(_), .. }
                )
            },
            move || {
                text(move || {
                    state.with(|snapshot| match &*snapshot.projection {
                        PricingSettingsProjection::Invalid(code)
                        | PricingSettingsProjection::Ready {
                            error: Some(code), ..
                        } => super::helpers::pricing_error_label(*code),
                        _ => "",
                    })
                })
                .text_color(move || crate::nsh::color(state.get().colors.danger))
            },
        ))
        .child(when(
            move || {
                matches!(
                    &*state.get().projection,
                    PricingSettingsProjection::Ready {
                        notice: Some(_),
                        ..
                    }
                )
            },
            move || {
                text(move || {
                    state.with(|snapshot| match &*snapshot.projection {
                        PricingSettingsProjection::Ready {
                            notice: Some(notice),
                            ..
                        } => super::helpers::pricing_notice_label(*notice),
                        _ => "",
                    })
                })
                .text_color(move || crate::nsh::color(state.get().colors.warning))
            },
        ))
        .child(when(
            move || {
                matches!(
                    &*state.get().projection,
                    PricingSettingsProjection::Ready {
                        draft_reason: Some(_),
                        ..
                    }
                )
            },
            {
                let commands = commands.clone();
                move || {
                    view()
                        .flex_row()
                        .flex_wrap()
                        .gap_2()
                        .child(button(actions, commands.clone(), || Action::Retry))
                        .child(button(actions, commands.clone(), || Action::Discard))
                }
            },
        ))
        .child(each(
            move || {
                state.with(|snapshot| {
                    if snapshot.editor.is_some() {
                        Vec::new()
                    } else {
                        match &*snapshot.projection {
                            PricingSettingsProjection::Ready { entries, .. }
                            | PricingSettingsProjection::Saving { entries, .. } => entries
                                .iter()
                                .filter(|entry| {
                                    snapshot
                                        .route
                                        .as_ref()
                                        .is_none_or(|(_, model)| *model == entry.model)
                                })
                                .cloned()
                                .collect(),
                            _ => Vec::new(),
                        }
                    }
                })
            },
            |entry| entry.model.clone(),
            move |entry, _| {
                group(state)
                    .flex_row()
                    .flex_wrap()
                    .items_center()
                    .gap_3()
                    .child(
                        view()
                            .flex_1()
                            .min_w(px(Layout::SETTINGS_LABEL_WIDTH))
                            .gap_1()
                            .child(text(move || entry.get().model))
                            .child(
                                text(move || {
                                    state.with(|_| {
                                        vega_i18n::t(match entry.get().kind {
                                            PricingEntryKind::BuiltInScheduled => {
                                                "settings.pricing.kind.peak"
                                            }
                                            PricingEntryKind::CustomStatic => {
                                                "settings.pricing.kind.custom"
                                            }
                                            _ => "settings.pricing.kind.builtin",
                                        })
                                    })
                                })
                                .text_color(move || {
                                    crate::nsh::color(state.get().colors.text_secondary)
                                }),
                            ),
                    )
                    .child(button(actions, rows.clone(), move || {
                        Action::Edit(Box::new(entry.get()))
                    }))
                    .child(button(actions, rows.clone(), move || {
                        Action::Secondary(Box::new(entry.get()))
                    }))
            },
        ))
        .child(each(
            move || forms.get(),
            |(revision, _)| *revision,
            move |current, _| editor(actions, form.clone(), current.get_untracked().1),
        ))
        .child(when(
            move || actions.failed.get(),
            move || {
                text(move || state.with(|_| vega_i18n::t("ui.command_failed")))
                    .text_color(move || crate::nsh::color(state.get().colors.danger))
            },
        ))
        .into_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pricing_draft_claim_preserves_decimal_text_and_rejects_late_or_composing_edits() {
        let mut values = std::array::from_fn(|_| String::new());
        values[1] = "0001.234567890123456789".into();
        let draft = Draft::new(values.clone());
        let observed = draft.read();
        assert!(draft.edit(1, Some("0.000000000000001"), None));
        assert!(draft.claim(Some(&observed)).is_none());
        assert!(draft.edit(0, None, Some(true)));
        assert!(draft.claim(None).is_none());
        assert!(draft.edit(0, None, Some(false)));
        values[1] = "0.000000000000001".into();
        assert_eq!(draft.claim(None), Some(values));
        assert!(!draft.edit(1, Some("late"), None));
        assert!(draft.claim(None).is_none());
        assert_eq!(draft.read().text[1], "0.000000000000001");
    }

    #[test]
    fn rejected_pricing_save_releases_exact_draft_without_reusing_another_session() {
        let values = std::array::from_fn(|index| format!("000{index}.1234567890123456789"));
        let draft = Draft::new(values.clone());
        assert_eq!(draft.claim(None), Some(values.clone()));
        draft.release();
        assert_eq!(draft.read().text, values);
        assert!(draft.edit(1, Some("0.00000000000001"), None));
        let replacement = Draft::new(std::array::from_fn(|_| String::new()));
        assert!(!draft.session.matches(&replacement.session));
        assert!(draft.edit(2, Some("old session"), None));
        assert!(replacement.read().text.iter().all(String::is_empty));
        assert_eq!(draft.claim(None).unwrap()[1], "0.00000000000001");
    }

    fn snapshot() -> Snapshot {
        Snapshot {
            id: ProjectionId::new(),
            projection: Arc::new(PricingSettingsProjection::Ready {
                generation: 1,
                entries: vec![],
                notice: None,
                draft_reason: None,
                error: None,
            }),
            route: None,
            visible: true,
            advanced: false,
            editor: None,
            colors: vega_theme::Theme::light().colors,
        }
    }

    #[test]
    fn pricing_actions_require_current_projection_route_and_editor_session() {
        let observed = snapshot();
        assert!(accepts(&observed, &observed, &Action::Add));
        let mut current = observed.clone();
        current.projection = Arc::new(PricingSettingsProjection::Saving {
            generation: 1,
            entries: vec![],
        });
        assert!(!accepts(&current, &observed, &Action::Add));
        assert!(!allowed(&current, &Action::Reload));
        current = observed.clone();
        current.route = Some(("provider".into(), "model".into()));
        assert!(!accepts(&current, &observed, &Action::Add));
        current = observed.clone();
        current.visible = false;
        assert!(!accepts(&current, &observed, &Action::Reload));
        let draft = Draft::new(std::array::from_fn(|_| String::new()));
        current = observed.clone();
        current.editor = Some(Editor {
            descriptor: PricingEditor {
                kind: PricingEditorKind::AddCustom,
                model: None,
            },
            observed: draft.read(),
            draft,
        });
        let editor = current.clone();
        assert!(accepts(&current, &editor, &Action::Save));
        current.editor.as_mut().unwrap().draft = Draft::new(std::array::from_fn(|_| String::new()));
        assert!(!accepts(&current, &editor, &Action::Save));
    }
}
