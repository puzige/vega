use super::*;
use crate::nsh::{OrderedCommands, Surface};
use ns::{Accessible as _, Interactive as _, IntoElement as _, ParentElement as _, Styled as _};
use nsh_core::{element::Element as _, prelude as ns};
use std::rc::{Rc, Weak};
use std::sync::{Arc, Mutex, MutexGuard};
use tokio::sync::mpsc;
use vega_theme::{Layout, ThemeColors, Typography};

#[derive(Clone)]
pub(super) struct Draft {
    pub(super) text: String,
    pub(super) revision: u64,
    pub(super) composing: bool,
    session: ProjectionId,
}

impl Draft {
    pub(super) fn committed(&self, query: &str, revision: u64) -> bool {
        !self.composing && self.text == query && self.revision == revision
    }
}

#[derive(Clone)]
pub(super) struct SharedDraft(Arc<Mutex<Draft>>);

impl SharedDraft {
    pub(super) fn new() -> Self {
        Self(Arc::new(Mutex::new(Draft {
            text: String::new(),
            revision: 0,
            composing: false,
            session: ProjectionId::new(),
        })))
    }

    pub(super) fn read(&self) -> Option<Draft> {
        self.0.lock().ok().map(|draft| draft.clone())
    }

    pub(super) fn locked(&self) -> Option<MutexGuard<'_, Draft>> {
        self.0.lock().ok()
    }

    fn edit(
        &self,
        session: &ProjectionId,
        text: Option<&str>,
        composing: Option<bool>,
    ) -> Option<Draft> {
        let mut draft = self.0.lock().ok()?;
        if !draft.session.matches(session) {
            return None;
        }
        if let Some(text) = text {
            draft.text = bounded(text);
        }
        if let Some(composing) = composing {
            draft.composing = composing;
        }
        draft.revision = draft.revision.wrapping_add(1);
        Some(draft.clone())
    }

    fn renew(&self) -> Option<Draft> {
        let mut draft = self.0.lock().ok()?;
        draft.session = ProjectionId::new();
        draft.composing = false;
        Some(draft.clone())
    }

    fn accepts(&self, observed: &Draft) -> bool {
        self.read().is_some_and(|current| {
            current.session.matches(&observed.session)
                && current.revision == observed.revision
                && current.text == observed.text
                && !current.composing
                && !observed.composing
        })
    }
}

fn bounded(text: &str) -> String {
    text.chars().take(256).collect()
}

#[derive(Clone)]
struct Snapshot {
    projection: ProjectionId,
    draft: SharedDraft,
    observed: Draft,
    query: String,
    scope: PaletteScope,
    entries: Vec<(String, String, PaletteTarget)>,
    selected: usize,
    loading: bool,
    error: Option<PaletteError>,
    files_unavailable: bool,
    colors: ThemeColors,
    width: f32,
    left: f32,
    height: f32,
    recovery: bool,
}

#[derive(Clone)]
enum Operation {
    Commit,
    Next,
    Previous,
    Accept,
    Activate(usize, PaletteTarget),
    Dismiss,
    Scope(PaletteScope),
    CycleScope(bool),
    Rebuild,
}

struct Request {
    session: ProjectionId,
    projection: ProjectionId,
    observed: Draft,
    operation: Operation,
}

impl Request {
    fn authorized(
        &self,
        draft: &SharedDraft,
        session: &ProjectionId,
        projection: &ProjectionId,
        mounted: bool,
        carrier_composing: bool,
        blocked: bool,
    ) -> bool {
        if !mounted
            || !session.matches(&self.session)
            || !draft
                .read()
                .is_some_and(|current| current.session.matches(&self.session))
        {
            return false;
        }
        if matches!(self.operation, Operation::Commit) {
            return !blocked;
        }
        draft.accepts(&self.observed)
            && projection.matches(&self.projection)
            && !carrier_composing
            && (!blocked || matches!(self.operation, Operation::Dismiss | Operation::Rebuild))
    }
}

pub(super) struct Presentation {
    surface: Surface<Snapshot, Request>,
    recovery: Option<Surface<Snapshot, Request>>,
    session: ProjectionId,
    lease: Weak<()>,
}

impl Presentation {
    pub(super) fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.recovery
            .as_ref()
            .unwrap_or(&self.surface)
            .focus_handle(cx)
    }

    fn failed(&self) -> bool {
        self.surface.errors().borrow().is_some()
    }

    fn stop(&self, cx: &App) {
        self.surface.entity().read(cx).stop();
        if let Some(recovery) = &self.recovery {
            recovery.entity().read(cx).stop();
        }
    }
}

impl CommandPalette {
    fn snapshot(&self, window: &Window, cx: &App) -> Option<Snapshot> {
        Some(Snapshot {
            projection: self.projection.clone(),
            draft: self.draft.clone(),
            observed: self.draft.read()?,
            query: self.query.clone(),
            scope: self.scope,
            entries: self.entries(),
            selected: self.selected,
            loading: self.loading,
            error: self.error,
            files_unavailable: self.results.files_unavailable,
            colors: vega_theme::theme(cx).colors,
            width: Layout::COMMAND_PALETTE_WIDTH.min(
                (f32::from(window.viewport_size().width) - 2. * Layout::COMMAND_PALETTE_SIDE_INSET)
                    .max(0.),
            ),
            left: ((f32::from(window.viewport_size().width) - Layout::COMMAND_PALETTE_WIDTH) / 2.)
                .max(Layout::COMMAND_PALETTE_SIDE_INSET),
            height: Layout::COMMAND_PALETTE_MAX_HEIGHT.min(
                (f32::from(window.viewport_size().height)
                    - Layout::COMMAND_PALETTE_VERTICAL_RESERVE)
                    .max(Layout::COMMAND_PALETTE_MIN_HEIGHT),
            ),
            recovery: false,
        })
    }

    fn mount_surface(
        &self,
        snapshot: Snapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Surface<Snapshot, Request> {
        let (surface, mut requests) = Surface::new(snapshot, render, window, cx);
        let mut errors = surface.errors();
        cx.spawn(async move |this, cx| {
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
        cx.spawn(async move |this, cx| {
            while errors.changed().await.is_ok() {
                if errors.borrow_and_update().is_some()
                    && this.update(cx, |_, cx| cx.notify()).is_err()
                {
                    break;
                }
            }
        })
        .detach();
        surface
    }

    fn receive(&mut self, request: Request, window: &mut Window, cx: &mut Context<Self>) {
        let Some(presentation) = &self.presentation else {
            return;
        };
        if !request.authorized(
            &self.draft,
            &presentation.session,
            &self.projection,
            presentation.lease.upgrade().is_some(),
            self.input.read(cx).is_composing(),
            presentation.failed() || presentation.recovery.is_some(),
        ) {
            return;
        }
        if matches!(request.operation, Operation::Dismiss) {
            cx.emit(PaletteClosed);
            return;
        }
        if matches!(request.operation, Operation::Rebuild) {
            let lease = presentation.lease.clone();
            if self.draft.renew().is_none() {
                return;
            }
            self.projection = ProjectionId::new();
            let Some(snapshot) = self.snapshot(window, cx) else {
                return;
            };
            let surface = self.mount_surface(snapshot.clone(), window, cx);
            if let Some(presentation) = &self.presentation {
                presentation.stop(cx);
            }
            surface.focus(window, cx);
            self.presentation = Some(Presentation {
                surface,
                recovery: None,
                session: snapshot.observed.session,
                lease,
            });
            cx.notify();
            return;
        }
        if matches!(request.operation, Operation::Commit) {
            self.sync_query(cx);
            return;
        }
        let shared = self.draft.clone();
        let Some(current) = shared.locked() else {
            return;
        };
        if !current.session.matches(&request.observed.session)
            || current.revision != request.observed.revision
            || current.text != request.observed.text
            || current.composing
            || request.observed.composing
            || self.query != request.observed.text
            || self.query_revision != request.observed.revision
            || !self.projection.matches(&request.projection)
            || self.input.read(cx).is_composing()
        {
            return;
        }
        match request.operation {
            Operation::Next => {
                self.selected = (self.selected + 1).min(self.entries().len().saturating_sub(1));
            }
            Operation::Previous => self.selected = self.selected.saturating_sub(1),
            Operation::Accept => {
                if let Some((_, _, target)) = self.entries().get(self.selected) {
                    cx.emit(PaletteActivated(target.clone()));
                }
            }
            Operation::Activate(index, target) => {
                if self
                    .entries()
                    .get(index)
                    .is_some_and(|entry| entry.2 == target)
                {
                    self.selected = index;
                    cx.emit(PaletteActivated(target));
                }
            }
            Operation::Dismiss => cx.emit(PaletteClosed),
            Operation::Scope(scope) => self.select_scope(scope),
            Operation::CycleScope(backwards) => {
                self.select_scope(next_scope(self.scope, backwards));
            }
            Operation::Commit | Operation::Rebuild => {}
        }
        cx.notify();
    }

    fn select_scope(&mut self, scope: PaletteScope) {
        self.scope = scope;
        self.selected = 0;
        self.projection = ProjectionId::new();
    }

    pub(super) fn render_nsh(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if self
            .presentation
            .as_ref()
            .is_some_and(|p| p.lease.upgrade().is_none())
        {
            if let Some(presentation) = self.presentation.take() {
                presentation.stop(cx);
            }
            self.draft.renew();
            self.projection = ProjectionId::new();
        }
        let Some(mut snapshot) = self.snapshot(window, cx) else {
            return div().size_full().into_any_element();
        };
        if self.presentation.is_none() {
            let surface = self.mount_surface(snapshot.clone(), window, cx);
            self.presentation = Some(Presentation {
                surface,
                recovery: None,
                session: snapshot.observed.session.clone(),
                lease: Weak::new(),
            });
        }
        let failed = self.presentation.as_ref().is_some_and(Presentation::failed);
        if failed {
            snapshot.recovery = true;
            let needs_recovery = self
                .presentation
                .as_ref()
                .is_some_and(|p| p.recovery.is_none());
            if needs_recovery && let Some(draft) = self.draft.renew() {
                snapshot.observed = draft.clone();
                let recovery = self.mount_surface(snapshot.clone(), window, cx);
                recovery.focus(window, cx);
                if let Some(presentation) = &mut self.presentation {
                    presentation.surface.entity().read(cx).stop();
                    presentation.session = draft.session;
                    presentation.recovery = Some(recovery);
                }
            }
        }
        let Some(presentation) = &mut self.presentation else {
            return div().size_full().into_any_element();
        };
        let lease = Rc::new(());
        presentation.lease = Rc::downgrade(&lease);
        let surface = presentation
            .recovery
            .as_mut()
            .unwrap_or(&mut presentation.surface);
        surface.sync_environment(cx);
        surface.publish(snapshot.clone());
        let composing = self.draft.clone();
        let recovery = snapshot.recovery;
        let host = div()
            .id("nsh-command-palette-host")
            .absolute()
            .inset_0()
            .capture_any_mouse_down(move |_, _, cx| {
                if composing.read().is_none_or(|draft| draft.composing) {
                    cx.stop_propagation();
                }
            })
            .capture_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                if recovery
                    && this.draft.accepts(&snapshot.observed)
                    && !this.input.read(cx).is_composing()
                    && matches!(event.keystroke.key.as_str(), "enter" | "escape" | "tab")
                {
                    if event.keystroke.key != "tab" {
                        this.receive(
                            Request {
                                session: snapshot.observed.session.clone(),
                                projection: snapshot.projection.clone(),
                                observed: snapshot.observed.clone(),
                                operation: if event.keystroke.key == "enter" {
                                    Operation::Rebuild
                                } else {
                                    Operation::Dismiss
                                },
                            },
                            window,
                            cx,
                        );
                    }
                    cx.stop_propagation();
                }
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(surface.entity());
        Mounted {
            child: host.into_any_element(),
            lease,
        }
        .into_any_element()
    }
}

fn next_scope(scope: PaletteScope, backwards: bool) -> PaletteScope {
    match (scope, backwards) {
        (PaletteScope::All, false) | (PaletteScope::Tasks, true) => PaletteScope::Actions,
        (PaletteScope::Actions, false) | (PaletteScope::Files, true) => PaletteScope::Tasks,
        (PaletteScope::Tasks, false) | (PaletteScope::All, true) => PaletteScope::Files,
        (PaletteScope::Files, false) | (PaletteScope::Actions, true) => PaletteScope::All,
    }
}

#[derive(Clone)]
struct Dispatch {
    state: ns::Signal<Snapshot>,
    commands: OrderedCommands<Request>,
    session: ProjectionId,
}

impl Dispatch {
    fn send(&self, operation: Operation, observed: &Snapshot) {
        if self.commands.failed.get_untracked() {
            return;
        }
        let Some(draft) = observed.draft.read() else {
            return;
        };
        if !draft.session.matches(&self.session) || draft.composing {
            return;
        }
        if !matches!(
            operation,
            Operation::Commit | Operation::Rebuild | Operation::Dismiss
        ) && (draft.text != observed.query || draft.revision != observed.observed.revision)
        {
            return;
        }
        self.commands.send(Request {
            session: self.session.clone(),
            projection: observed.projection.clone(),
            observed: draft,
            operation,
        });
    }

    fn current(&self, operation: Operation) {
        self.send(operation, &self.state.get_untracked());
    }
}

fn register_keys(
    composing: ns::Signal<bool>,
    focused: ns::Signal<bool>,
    recovery: bool,
    handler: impl Fn(Operation) + 'static,
) {
    let handler = Rc::new(handler);
    ns::effect(move || {
        if composing.get() || !focused.get() || recovery {
            return;
        }
        for (key, operation, repeat) in [
            ("down", Operation::Next, true),
            ("up", Operation::Previous, true),
            ("enter", Operation::Accept, false),
            ("escape", Operation::Dismiss, false),
            ("tab", Operation::CycleScope(false), false),
            ("shift+tab", Operation::CycleScope(true), false),
        ] {
            let handler = handler.clone();
            ns::on_key_with(
                key,
                ns::KeyOptions {
                    when: nsh_core::keymap::When::Always,
                    repeat,
                },
                move |_| {
                    if focused.get_untracked() && !composing.get_untracked() {
                        handler(operation.clone());
                    }
                },
            );
        }
    });
}

fn render(state: ns::Signal<Snapshot>, sender: mpsc::Sender<Request>) -> ns::AnyElement {
    let initial = state.get_untracked();
    let commands = OrderedCommands::new(sender);
    let dispatch = Dispatch {
        state,
        commands: commands.clone(),
        session: initial.observed.session.clone(),
    };
    let announced = ns::signal(None::<(u64, usize, PaletteScope)>);
    ns::effect(move || {
        let snapshot = state.get();
        let selection = (
            snapshot.observed.revision,
            snapshot.selected,
            snapshot.scope,
        );
        if announced.get_untracked() != Some(selection) {
            announced.set(Some(selection));
            if let Some((label, detail, _)) = snapshot.entries.get(snapshot.selected) {
                ns::announce_for_accessibility(format!("{} {}", clean(label), clean(detail)));
            }
        }
    });
    let value = ns::signal(initial.observed.text.clone());
    let composing = ns::signal(false);
    let focused = ns::signal(false);
    let keyboard = dispatch.clone();
    register_keys(composing, focused, initial.recovery, move |operation| {
        keyboard.current(operation);
    });
    #[cfg(feature = "nsh-reading-integration")]
    ns::effect(move || {
        mounted_regressions::observe_projection(&state.get(), composing.get(), focused.get());
    });
    let input_ref = ns::element_ref();
    let change = dispatch.clone();
    let composition = dispatch.clone();
    let changed_draft = initial.draft.clone();
    let composed_draft = initial.draft.clone();
    let changed_session = dispatch.session.clone();
    let composed_session = dispatch.session.clone();
    let input = nsh_base::input()
        .element_ref(&input_ref)
        .label(vega_i18n::t("ui.search_actions_tasks_or_files"))
        .placeholder(vega_i18n::t("ui.search_actions_tasks_or_files"))
        .value(value)
        .max_length(256)
        .auto_focus(true)
        .auto_correct(false)
        .disabled(move || state.get().recovery)
        .submit_behavior(nsh_core::SubmitBehavior::Submit)
        .w_full()
        .h(ns::px(36.))
        .text_size(ns::px(Typography::BODY))
        .text_color(move || crate::nsh::color(state.get().colors.text_primary))
        .on_focus(move || focused.set(true))
        .on_blur(move || focused.set(false))
        .on_change(move |text: &str| {
            if let Some(draft) = changed_draft.edit(&changed_session, Some(text), None) {
                value.set(draft.text);
                if !draft.composing {
                    change.current(Operation::Commit);
                }
            }
        })
        .on_composition_change(move |event: &nsh_core::element::Composition| {
            composing.set(event.composing);
            if composed_draft
                .edit(&composed_session, None, Some(event.composing))
                .is_some()
                && !event.composing
            {
                composition.current(Operation::Commit);
            }
        });
    let reset_value = initial.draft.clone();
    ns::effect(move || {
        let observed = state.get();
        if let Some(draft) = reset_value.read()
            && !draft.composing
            && draft.session.matches(&observed.observed.session)
        {
            value.set(draft.text);
        }
    });
    let backdrop = dispatch.clone();
    let body = dispatch.clone();
    ns::view()
        .w_full()
        .h_full()
        .relative()
        .child(
            ns::view()
                .absolute()
                .inset(ns::px(0.))
                .bg(move || crate::nsh::color(state.get().colors.bg_sidebar.opacity(0.85)))
                .label(vega_i18n::t("ui.close"))
                .on_press(move || backdrop.current(Operation::Dismiss)),
        )
        .child(
            ns::view()
                .absolute()
                .top(ns::px(Layout::COMMAND_PALETTE_TOP_OFFSET))
                .left(move || ns::px(state.get().left))
                .w(move || ns::px(state.get().width))
                .max_h(move || ns::px(state.get().height))
                .flex_col()
                .rounded(ns::px(Layout::MENU_RADIUS))
                .shadow_sm()
                .border(ns::px(1.))
                .border_color(move || crate::nsh::color(state.get().colors.border_subtle))
                .bg(move || crate::nsh::color(state.get().colors.bg_elevated))
                .role(nsh_core::AccessibilityRole::Group)
                .label(vega_i18n::t("ui.search_actions_tasks_or_files"))
                .on_press(|| {})
                .child(if initial.recovery {
                    ns::view().into_element()
                } else {
                    ns::view().p_3().child(input).into_element()
                })
                .child(ns::dynamic(move || render_body(state.get(), body.clone())))
                .child(ns::dynamic(move || {
                    if commands.failed.get() {
                        ns::text(vega_i18n::t("ui.command_failed"))
                            .text_color(crate::nsh::color(state.get().colors.text_secondary))
                            .into_element()
                    } else {
                        ns::view().into_element()
                    }
                })),
        )
        .into_element()
}

fn render_body(snapshot: Snapshot, dispatch: Dispatch) -> ns::AnyElement {
    let colors = snapshot.colors;
    if snapshot.recovery {
        let reload = dispatch.clone();
        return ns::view()
            .p_3()
            .gap_2()
            .child(
                ns::text(vega_i18n::t("ui.command_failed"))
                    .text_color(crate::nsh::color(colors.text_primary)),
            )
            .child(
                ns::view()
                    .role(nsh_core::AccessibilityRole::Button)
                    .label(vega_i18n::t("app.nsh.reload"))
                    .on_press(move || reload.send(Operation::Rebuild, &snapshot))
                    .child(ns::text(vega_i18n::t("app.nsh.reload"))),
            )
            .into_element();
    }
    let mut body = ns::view().flex_col().min_h(ns::px(0.));
    let mut scopes = ns::view()
        .flex_row()
        .gap_2()
        .px_3()
        .pb_2()
        .role(nsh_core::AccessibilityRole::Tablist);
    for (scope, key) in [
        (PaletteScope::All, "ui.all"),
        (PaletteScope::Actions, "ui.actions"),
        (PaletteScope::Tasks, "ui.tasks"),
        (PaletteScope::Files, "ui.file"),
    ] {
        let sender = dispatch.clone();
        let observed = snapshot.clone();
        let selected = scope == snapshot.scope;
        scopes = scopes.child(
            ns::view()
                .role(nsh_core::AccessibilityRole::Tab)
                .label(vega_i18n::t(key))
                .accessibility_state(nsh_core::AccessibilityState {
                    selected,
                    ..Default::default()
                })
                .px_3()
                .py_1()
                .rounded(ns::px(6.))
                .bg(crate::nsh::color(if selected {
                    colors.bg_active
                } else {
                    colors.bg_elevated
                }))
                .on_press(move || sender.send(Operation::Scope(scope), &observed))
                .child(
                    ns::text(vega_i18n::t(key))
                        .text_size(ns::px(Typography::METADATA))
                        .text_color(crate::nsh::color(colors.text_primary)),
                ),
        );
    }
    body = body.child(scopes);
    let list = ns::element_ref();
    let selected = snapshot.selected;
    let row_height = Typography::SIDEBAR_LINE_HEIGHT;
    let viewport = (snapshot.height - 150.).max(row_height);
    let offset = ns::signal(0.0f32);
    ns::watch_scroll(
        move || list.node(),
        move |event: &nsh_core::element::ScrollEvent| offset.set(event.offset.y.0),
    );
    let reveal = move || {
        let current = offset.get_untracked();
        let top = selected as f32 * row_height;
        let bottom = top + row_height;
        let next = if top < current {
            top
        } else if bottom > current + viewport {
            bottom - viewport
        } else {
            current
        };
        if next != current {
            list.scroll_to(ns::point(ns::px(0.), ns::px(next)), false);
        }
    };
    let mut results = ns::scroll()
        .element_ref(&list)
        .on_layout(move |_: &ns::Bounds<ns::Points>| reveal())
        .max_h(ns::px(viewport))
        .flex_shrink_0()
        .role(nsh_core::AccessibilityRole::Listbox)
        .label(vega_i18n::t("ui.search_actions_tasks_or_files"));
    for (index, (label, detail, target)) in snapshot.entries.iter().enumerate() {
        let sender = dispatch.clone();
        let observed = snapshot.clone();
        let target = target.clone();
        let selected = index == snapshot.selected;
        results = results.child(
            ns::view()
                .role(nsh_core::AccessibilityRole::Option)
                .label(format!("{} {}", clean(label), clean(detail)))
                .accessibility_state(nsh_core::AccessibilityState {
                    selected,
                    ..Default::default()
                })
                .h(ns::px(row_height))
                .px_3()
                .flex_row()
                .items_center()
                .gap_3()
                .bg(crate::nsh::color(if selected {
                    colors.bg_hover
                } else {
                    colors.bg_elevated
                }))
                .on_press(move || {
                    sender.send(Operation::Activate(index, target.clone()), &observed)
                })
                .child(
                    ns::view()
                        .flex_1()
                        .min_w(ns::px(0.))
                        .overflow_hidden()
                        .child(
                            ns::text(clean(label))
                                .text_size(ns::px(Typography::SIDEBAR))
                                .text_color(crate::nsh::color(colors.text_primary)),
                        ),
                )
                .child(
                    ns::text(clean(detail))
                        .text_size(ns::px(Typography::METADATA))
                        .text_color(crate::nsh::color(colors.text_tertiary)),
                ),
        );
    }
    body = body.child(results);
    if snapshot.loading {
        body = body.child(status(vega_i18n::t("ui.searching"), colors));
    }
    if snapshot.error.is_some() || snapshot.files_unavailable {
        body = body.child(status(error_label(snapshot.error), colors));
    }
    if !snapshot.loading && snapshot.entries.is_empty() {
        body = body.child(status(vega_i18n::t("ui.no_matching_results"), colors));
    }
    body.child(
        ns::view()
            .px_4()
            .py_2()
            .border_t(ns::px(1.))
            .border_color(crate::nsh::color(colors.border_subtle))
            .child(
                ns::text(vega_i18n::t("ui.select_open_tab_change_scope_esc_close"))
                    .text_size(ns::px(Typography::METADATA))
                    .text_color(crate::nsh::color(colors.text_tertiary)),
            ),
    )
    .into_element()
}

fn clean(text: &str) -> String {
    text.replace(['\n', '\r'], " ")
}

fn status(label: &'static str, colors: ThemeColors) -> ns::AnyElement {
    ns::view()
        .px_4()
        .py_2()
        .child(
            ns::text(label)
                .text_size(ns::px(Typography::METADATA))
                .text_color(crate::nsh::color(colors.text_secondary)),
        )
        .into_element()
}

fn error_label(error: Option<PaletteError>) -> &'static str {
    vega_i18n::t(match error {
        Some(PaletteError::Binary) => "ui.binary_files_cannot_be_previewed_as_text_view",
        Some(PaletteError::TooLarge) => "ui.files_larger_than_128_kib_cannot_be_previewed",
        Some(PaletteError::UnsafePath) => "ui.the_file_is_outside_the_project_or_has",
        Some(PaletteError::ProjectUnavailable) => {
            "ui.the_project_is_inaccessible_reopen_the_workspace"
        }
        Some(PaletteError::Cancelled) => "ui.search_cancelled_try_again",
        Some(PaletteError::Unavailable) => "ui.cannot_read_the_result_try_again_or_check",
        None => "ui.some_file_results_are_unavailable_search_again",
    })
}

struct Mounted {
    child: AnyElement,
    lease: Rc<()>,
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
        Some("command-palette-mounted".into())
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
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(id) = id {
            window.with_element_state::<Rc<()>, _>(id, |_, _| ((), self.lease.clone()));
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

#[cfg(test)]
mod tests;

#[cfg(feature = "nsh-reading-integration")]
pub(super) mod mounted_regressions;
