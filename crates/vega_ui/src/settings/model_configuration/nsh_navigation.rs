mod guard_controls;

use gpui_kit::{App, Context, Window};
use nsh_core::prelude::*;
use tokio::sync::{mpsc, oneshot};
use vega_conversation::types::{
    ModelCapabilityOverride, ModelCapabilityProjection, ModelCapabilitySettingsProjection,
    PricingSettingsProjection, ReasoningProfileProjection,
};
use vega_theme::{Layout, ThemeColors, Typography};

use super::{SettingsDestination, SettingsView};
use crate::nsh::{ProjectionId, Surface};
use crate::settings::nsh_pricing::navigation_bridge::NavigationDraft;
use crate::settings::provider_management::navigation_bridge::NavigationAuthority;
use crate::settings::{ReasoningSettingsProjection, SettingsOpen};

pub(in crate::settings) struct Presentation {
    surface: Surface<Snapshot, Request>,
    id: ProjectionId,
    height: f32,
    submitted: Option<Authority>,
    intent: Intent,
    guard: Option<guard_controls::Controls>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pane {
    Guard,
    Model,
}

#[derive(Clone, PartialEq, Eq)]
struct Intent {
    section: usize,
    model: Option<(String, String)>,
    pending: Option<SettingsDestination>,
    closing: bool,
}

#[derive(Clone, PartialEq)]
struct Authority {
    intent: Intent,
    available: bool,
    saving: bool,
    permission_pending: bool,
    has_draft: bool,
    provider: NavigationAuthority,
    pricing: PricingSettingsProjection,
    pricing_draft: Option<NavigationDraft>,
    reasoning: ReasoningSettingsProjection,
    reasoning_draft: Option<ReasoningProfileProjection>,
    reasoning_operation: u64,
    capabilities: ModelCapabilitySettingsProjection,
    capability_models: Vec<ModelCapabilityProjection>,
    capability_draft: Option<ModelCapabilityOverride>,
    capability_operation: u64,
    error: Option<String>,
}

#[derive(Clone)]
struct Snapshot {
    id: ProjectionId,
    pane: Pane,
    visible: bool,
    submitted: bool,
    guard_revision: u64,
    guard_focus: Option<Action>,
    authority: Authority,
    colors: ThemeColors,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Save,
    Discard,
    Stay,
    Back,
    Tab(usize),
}

struct Decision {
    observed: Snapshot,
    action: Action,
    reply: oneshot::Sender<bool>,
}

enum Request {
    Decide(Box<Decision>),
    Layout { id: ProjectionId, height: f32 },
}

fn allowed(snapshot: &Snapshot, action: Action) -> bool {
    if !snapshot.visible {
        return false;
    }
    let authority = &snapshot.authority;
    match (snapshot.pane, action) {
        (Pane::Guard, Action::Stay) => authority.intent.pending.is_some(),
        (Pane::Guard, Action::Save | Action::Discard) => {
            authority.intent.pending.is_some()
                && !authority.saving
                && !authority.permission_pending
                && !snapshot.submitted
                && (action != Action::Save
                    || (!authority.provider.composing()
                        && authority
                            .pricing_draft
                            .as_ref()
                            .is_none_or(NavigationDraft::can_claim)))
        }
        (Pane::Model, Action::Back) => !authority.permission_pending && !authority.saving,
        (Pane::Model, Action::Tab(section)) => {
            !authority.permission_pending
                && !authority.saving
                && authority.intent.model.is_some()
                && matches!(section, 0 | 2 | 3)
                && section != authority.intent.section
                && (section != 0 || authority.available)
        }
        _ => false,
    }
}

fn accepts(current: &Snapshot, observed: &Snapshot, action: Action) -> bool {
    current.id.matches(&observed.id)
        && current.guard_revision == observed.guard_revision
        && current.pane == observed.pane
        && current.authority == observed.authority
        && allowed(current, action)
        && allowed(observed, action)
}

impl SettingsView {
    fn navigation_authority(&self, cx: &App) -> Authority {
        Authority {
            intent: Intent {
                section: self.section,
                model: self.model_settings.clone(),
                pending: self.pending_section,
                closing: self.pending_settings_close,
            },
            available: self
                .model_settings
                .as_ref()
                .is_some_and(|(provider, model)| {
                    self.config.providers.iter().any(|candidate| {
                        candidate.name == *provider && candidate.models.contains(model)
                    })
                }),
            saving: self.settings_navigation_saving(),
            permission_pending: self.default_permission.io_pending(),
            has_draft: self.settings_have_draft(cx),
            provider: self.navigation_provider_authority(cx),
            pricing: self.pricing.clone(),
            pricing_draft: self.navigation_pricing_draft(),
            reasoning: self.reasoning.clone(),
            reasoning_draft: self.reasoning_draft.clone(),
            reasoning_operation: self.next_reasoning_operation,
            capabilities: self.model_capabilities.clone(),
            capability_models: self.model_capability_models.clone(),
            capability_draft: self.model_capability_draft.clone(),
            capability_operation: self.next_model_capability_operation,
            error: self.error.as_ref().map(|error| error.render().to_string()),
        }
    }

    fn navigation_visible(&self, pane: Pane, cx: &App) -> bool {
        cx.try_global::<SettingsOpen>().is_some_and(|open| open.0)
            && match pane {
                Pane::Guard => self.pending_section.is_some(),
                Pane::Model => {
                    (self.section == 0 && self.model_settings.is_some())
                        || matches!(self.section, 2 | 3)
                }
            }
    }

    fn navigation_page(&self, pane: Pane) -> Option<&Presentation> {
        match pane {
            Pane::Guard => self.settings_navigation_presentation.as_ref(),
            Pane::Model => self.model_navigation_presentation.as_ref(),
        }
    }

    fn navigation_page_mut(&mut self, pane: Pane) -> &mut Option<Presentation> {
        match pane {
            Pane::Guard => &mut self.settings_navigation_presentation,
            Pane::Model => &mut self.model_navigation_presentation,
        }
    }

    fn navigation_snapshot(&self, pane: Pane, id: ProjectionId, cx: &App) -> Snapshot {
        let authority = self.navigation_authority(cx);
        let submitted = self
            .navigation_page(pane)
            .and_then(|page| page.submitted.as_ref())
            .is_some_and(|submitted| *submitted == authority);
        let guard = self
            .navigation_page(pane)
            .and_then(|page| page.guard.as_ref());
        Snapshot {
            guard_revision: guard.map_or(0, |controls| controls.revision),
            guard_focus: guard
                .filter(|controls| controls.keyboard)
                .and_then(|controls| controls.focused),
            id,
            pane,
            visible: !self.palette_open && self.navigation_visible(pane, cx),
            submitted,
            authority,
            colors: vega_theme::theme(cx).colors,
        }
    }

    pub(in crate::settings) fn navigation_guard_control_focused(&self) -> bool {
        self.settings_navigation_presentation
            .as_ref()
            .and_then(|p| p.guard.as_ref())
            .is_some_and(|guard| guard.focused.is_some())
    }

    pub(in crate::settings) fn revoke_navigation_input(&mut self, cx: &App) {
        for pane in [Pane::Guard, Pane::Model] {
            if let Some(page) = self.navigation_page_mut(pane) {
                page.id = ProjectionId::new();
                if let Some(guard) = &mut page.guard {
                    guard.revision = guard.revision.wrapping_add(1);
                }
            }
        }
        self.sync_settings_navigation_presentations(cx);
    }

    pub(in crate::settings) fn sync_settings_navigation_presentations(&mut self, cx: &App) {
        for pane in [Pane::Guard, Pane::Model] {
            if !self.navigation_visible(pane, cx) {
                *self.navigation_page_mut(pane) = None;
                continue;
            }
            let Some(page) = self.navigation_page(pane) else {
                continue;
            };
            let mut snapshot = self.navigation_snapshot(pane, page.id.clone(), cx);
            if let Some(page) = self.navigation_page_mut(pane) {
                if page.intent != snapshot.authority.intent {
                    page.id = ProjectionId::new();
                    page.intent = snapshot.authority.intent.clone();
                    page.submitted = None;
                    snapshot.id = page.id.clone();
                    snapshot.submitted = false;
                } else if page
                    .submitted
                    .as_ref()
                    .is_some_and(|submitted| *submitted != snapshot.authority)
                {
                    page.submitted = None;
                    snapshot.submitted = false;
                }
                if let Some(guard) = &mut page.guard {
                    if guard.authority != snapshot.authority {
                        guard.revision = guard.revision.wrapping_add(1);
                        guard.authority = snapshot.authority.clone();
                    }
                    snapshot.guard_revision = guard.revision;
                }
                page.surface.publish(snapshot);
            }
        }
    }

    fn decide_settings_navigation(
        &mut self,
        pane: Pane,
        observed: &Snapshot,
        action: Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(page) = self.navigation_page(pane) else {
            return false;
        };
        let current = self.navigation_snapshot(pane, page.id.clone(), cx);
        if !accepts(&current, observed, action) {
            return false;
        }
        // An incomplete provider form has no save operation to await.
        if action == Action::Save
            && self.section == 0
            && self.model_capability_draft.is_none()
            && self.provider_form_save_unavailable(cx)
        {
            return true;
        }
        let accepted = match action {
            Action::Save => self.save_settings_before_navigation(
                observed.authority.pricing_draft.as_ref(),
                observed.authority.provider.draft(),
                cx,
            ),
            Action::Discard => self.discard_settings_before_navigation(
                observed.authority.pricing_draft.as_ref(),
                observed.authority.provider.draft(),
                window,
                cx,
            ),
            Action::Stay => {
                self.stay_in_settings(cx);
                self.focus_settings_after_guard(window, cx);
                true
            }
            Action::Back => {
                self.request_settings_section(0, cx);
                true
            }
            Action::Tab(section) => {
                self.request_settings_destination(SettingsDestination::ModelSection(section), cx);
                true
            }
        };
        if accepted && matches!(action, Action::Save | Action::Discard) {
            let submitted = if action == Action::Discard {
                self.navigation_authority(cx)
            } else {
                observed.authority.clone()
            };
            if let Some(page) = self.navigation_page_mut(pane) {
                page.submitted = Some(submitted);
            }
        }
        if accepted {
            cx.notify();
        }
        accepted
    }

    fn focus_settings_after_guard(&self, window: &mut Window, cx: &mut App) {
        if self.navigation_visible(Pane::Model, cx)
            && let Some(page) = self.navigation_page(Pane::Model)
        {
            page.surface.focus(window, cx);
        } else if self.section == 0
            && let Some(focus) = self.provider_surface_focus_handle(cx)
        {
            window.focus(&focus, cx);
        } else {
            window.focus(&self.section_focuses[self.section], cx);
        }
    }

    pub(crate) fn render_settings_navigation_guard(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        self.render_navigation_surface(Pane::Guard, window, cx)
    }

    pub(crate) fn render_model_navigation(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        self.render_navigation_surface(Pane::Model, window, cx)
    }

    fn render_navigation_surface(
        &mut self,
        pane: Pane,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        use gpui_kit::{IntoElement, ParentElement, Styled};
        if let Some(guard) = self
            .navigation_page_mut(pane)
            .as_mut()
            .and_then(|p| p.guard.as_mut())
        {
            guard.keyboard = window.last_input_was_keyboard();
        }
        self.sync_settings_navigation_presentations(cx);
        if !self.navigation_visible(pane, cx) {
            return gpui_kit::div().into_any_element();
        }
        if self.navigation_page(pane).is_none() {
            let id = ProjectionId::new();
            let snapshot = self.navigation_snapshot(pane, id.clone(), cx);
            let intent = snapshot.authority.intent.clone();
            let authority = snapshot.authority.clone();
            let (surface, mut requests) = Surface::new_transparent(snapshot, render, window, cx);
            let guard = (pane == Pane::Guard)
                .then(|| guard_controls::Controls::new(&surface, authority, window, cx));
            *self.navigation_page_mut(pane) = Some(Presentation {
                guard,
                surface,
                id,
                height: Layout::SETTINGS_NOTES_MAX_HEIGHT,
                submitted: None,
                intent,
            });
            if pane == Pane::Guard
                && !self.palette_open
                && let Some(page) = self.navigation_page(pane)
            {
                page.surface.focus(window, cx);
            }
            cx.spawn_in(window, async move |this, cx| {
                while let Some(request) = requests.recv().await {
                    if this
                        .update_in(cx, |this, window, cx| {
                            match request {
                                Request::Decide(command) => {
                                    let accepted = this.decide_settings_navigation(
                                        pane,
                                        &command.observed,
                                        command.action,
                                        window,
                                        cx,
                                    );
                                    let _ = command.reply.send(accepted);
                                }
                                Request::Layout { id, height } => {
                                    if height.is_finite()
                                        && height > 0.
                                        && let Some(page) = this.navigation_page_mut(pane)
                                        && page.id.matches(&id)
                                        && (page.height - height).abs() > 0.5
                                    {
                                        page.height = height;
                                        cx.notify();
                                    }
                                }
                            }
                            this.sync_settings_navigation_presentations(cx);
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();
        }
        let Some(page) = self.navigation_page_mut(pane) else {
            return gpui_kit::div().into_any_element();
        };
        page.surface.sync_environment(cx);
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
}

impl Actions {
    fn send(self, commands: mpsc::Sender<Request>, action: Action) {
        let observed = self.state.get_untracked();
        if self.pending.get_untracked() || !allowed(&observed, action) {
            return;
        }
        self.pending.set(true);
        self.failed.set(false);
        spawn(async move {
            let (reply, response) = oneshot::channel();
            if commands
                .send(Request::Decide(Box::new(Decision {
                    observed,
                    action,
                    reply,
                })))
                .await
                .is_err()
                || !matches!(response.await, Ok(true))
            {
                self.failed.set(true);
            }
            self.pending.set(false);
        });
    }
}

fn selected(snapshot: &Snapshot, action: Action) -> bool {
    matches!(action, Action::Tab(section) if section == snapshot.authority.intent.section)
}

fn route_button(
    actions: Actions,
    commands: mpsc::Sender<Request>,
    action: Action,
    key: &'static str,
) -> AnyElement {
    let target = element_ref();
    let focused = signal(false);
    target.on_focus_in(move || focused.set(true));
    target.on_focus_out(move || focused.set(false));
    let enabled = move || !actions.pending.get() && allowed(&actions.state.get(), action);
    view()
        .element_ref(&target)
        .role(AccessibilityRole::Button)
        .focusable(true)
        .label(move || actions.state.with(|_| vega_i18n::t(key)))
        .accessibility_state(move || nsh_core::AccessibilityState {
            selected: selected(&actions.state.get(), action),
            ..Default::default()
        })
        .disabled(move || !enabled())
        .px(px(Layout::SETTINGS_CONTROL_PADDING_X))
        .py_1()
        .items_center()
        .justify_center()
        .line_height(px((Typography::BODY * Typography::BODY_LINE_HEIGHT).round()))
        .min_h(px(Layout::SETTINGS_CONTROL_MIN_HEIGHT))
        .border(px(1.))
        .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
        .border_color(move || {
            crate::nsh::color(if focused.get() || selected(&actions.state.get(), action) {
                actions.state.get().colors.accent
            } else {
                actions.state.get().colors.border_subtle
            })
        })
        .bg(move || {
            crate::nsh::color(if selected(&actions.state.get(), action) {
                actions.state.get().colors.bg_active
            } else {
                actions.state.get().colors.bg_base.opacity(0.)
            })
        })
        .style_when(enabled, |style| style.cursor_pointer())
        .hover(|style| {
            style.bg(move || {
                actions.state.with(|s| {
                    crate::nsh::color(if selected(s, action) {
                        s.colors.bg_active
                    } else if enabled() {
                        s.colors.bg_hover
                    } else {
                        s.colors.bg_base.opacity(0.)
                    })
                })
            })
        })
        .active(|style| {
            style.bg(move || {
                actions.state.with(|s| {
                    crate::nsh::color(if selected(s, action) || enabled() {
                        s.colors.bg_active
                    } else {
                        s.colors.bg_base.opacity(0.)
                    })
                })
            })
        })
        .text_color(move || {
            crate::nsh::color(if !allowed(&actions.state.get(), action) {
                if selected(&actions.state.get(), action) {
                    actions.state.get().colors.text_primary
                } else {
                    actions.state.get().colors.text_tertiary
                }
            } else {
                actions.state.get().colors.text_primary
            })
        })
        .on_press(move |_: &nsh_core::PressEvent| actions.send(commands.clone(), action))
        .child(text(move || actions.state.with(|_| vega_i18n::t(key))))
        .into_element()
}

fn render(state: Signal<Snapshot>, commands: mpsc::Sender<Request>) -> AnyElement {
    let actions = Actions {
        state,
        pending: signal(false),
        failed: signal(false),
    };
    let measured = signal(0.0f32);
    let layout = commands.clone();
    let guard = commands.clone();
    let model = commands.clone();
    view()
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .gap(px(Layout::SETTINGS_CONTENT_GAP))
        .text_size(px(Typography::BODY))
        .text_color(move || crate::nsh::color(state.get().colors.text_primary))
        .on_layout(move |bounds: &Bounds<Points>| {
            let height = bounds.size.height.0;
            if height.is_finite() && height > 0. && (measured.get_untracked() - height).abs() > 0.5
            {
                measured.set(height);
                let commands = layout.clone();
                let id = state.get_untracked().id;
                spawn(async move {
                    if commands.send(Request::Layout { id, height }).await.is_err() {
                        actions.failed.set(true);
                    }
                });
            }
        })
        .child(when(
            move || state.get().pane == Pane::Guard,
            move || {
                view()
                    .gap(px(Layout::SETTINGS_CONTENT_GAP))
                    .child(super::super::nsh_form::status(
                        move || state.get().colors,
                        move || state.get().colors.warning,
                        text(move || state.with(|_| vega_i18n::t("settings.navigation.unsaved"))),
                    ))
                    .child(view().flex_row().flex_wrap().gap_2().children(
                        guard_controls::BUTTONS.map(|(action, key)| {
                            guard_controls::button(actions, guard.clone(), action, key)
                        }),
                    ))
            },
        ))
        .child(when(
            move || state.get().pane == Pane::Model,
            move || {
                let tabs = model.clone();
                view()
                    .gap(px(Layout::SETTINGS_CONTENT_GAP))
                    .child(
                        view()
                            .flex_row()
                            .flex_wrap()
                            .items_center()
                            .gap_2()
                            .child(route_button(
                                actions,
                                model.clone(),
                                Action::Back,
                                "settings.navigation.back_providers",
                            ))
                            .child(
                                text(move || {
                                    let snapshot = state.get();
                                    if let Some((provider, model)) =
                                        &snapshot.authority.intent.model
                                    {
                                        format!("{provider} / {model}")
                                    } else if snapshot.authority.intent.section == 3 {
                                        vega_i18n::t("settings.pricing.library").to_string()
                                    } else {
                                        vega_i18n::t("settings.reasoning.choose_model").to_string()
                                    }
                                })
                                .min_w_0()
                                .text_color(move || {
                                    crate::nsh::color(state.get().colors.text_secondary)
                                }),
                            ),
                    )
                    .child(when(
                        move || state.get().authority.intent.model.is_some(),
                        move || {
                            view()
                                .flex_row()
                                .flex_wrap()
                                .gap_2()
                                .child(route_button(
                                    actions,
                                    tabs.clone(),
                                    Action::Tab(0),
                                    "settings.model.basic",
                                ))
                                .child(route_button(
                                    actions,
                                    tabs.clone(),
                                    Action::Tab(2),
                                    "settings.model.reasoning",
                                ))
                                .child(route_button(
                                    actions,
                                    tabs.clone(),
                                    Action::Tab(3),
                                    "settings.model.pricing",
                                ))
                        },
                    ))
            },
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
