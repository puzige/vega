use super::{GuidanceCard, Stage, Tracking};
use crate::cua_onboarding::controller::{CardAction, CardView, CheckState};
use crate::cua_onboarding::{CallbackIdentity, PermissionPane};
use crate::nsh::{ProjectionId, Surface};
use gpui_kit::{App, Context, Entity, Subscription, Window};
use nsh_core::element::ImageSrc;
use nsh_core::prelude::*;
use tokio::sync::{mpsc, oneshot};
use vega_conversation::types::PermissionState;
use vega_theme::{Layout, ThemeColors, Typography};

#[cfg(test)]
mod tests;

mod keyboard;
mod reveal;

pub(super) struct Presentation {
    surface: Surface<Snapshot, Request>,
    recovery: Option<Surface<Snapshot, Request>>,
    id: ProjectionId,
    keyboard: Option<keyboard::Selection>,
    pointer_pending: bool,
    selection_revision: u64,
    reveal_revision: Option<u64>,
    _focus: Subscription,
    _blur: Subscription,
    pub(super) drag_bounds: Option<DragBounds>,
}

impl Presentation {
    pub(super) fn reset_keyboard(&mut self) {
        self.keyboard = None;
        self.reveal_revision = None;
        self.pointer_pending = false;
        self.selection_revision = self.selection_revision.wrapping_add(1);
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct DragBounds {
    pub(super) x: f32,
    pub(super) y: f32,
    pub(super) width: f32,
    pub(super) height: f32,
}

impl DragBounds {
    fn valid(self, stage: Stage) -> bool {
        let (width, height) = stage.surface_size();
        [self.x, self.y, self.width, self.height]
            .iter()
            .all(|value| value.is_finite())
            && stage == Stage::Guide
            && self.x
                >= Layout::CONTENT_PADDING
                    + Layout::PREPARATION_GUIDE_CONTROL
                    + Layout::PREPARATION_GUIDE_GAP
            && self.y
                >= Layout::CONTENT_PADDING
                    + Layout::PREPARATION_GUIDE_CONTROL
                    + Layout::PREPARATION_GUIDE_GAP
            && self.width > 0.
            && self.height > 0.
            && self.x + self.width <= width
            && self.y + self.height <= height - Layout::CONTENT_PADDING
            && self.height <= Layout::PREPARATION_APP_TARGET_HEIGHT
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Authority {
    stage: Stage,
    stage_identity: CallbackIdentity,
    pane: PermissionPane,
    has_task: bool,
    placed: bool,
    controller: CardView,
    preparation: CardView,
    preparation_revision: u64,
    retained: bool,
    local_fault: Option<String>,
    warning: Option<String>,
    path_fault: bool,
}

#[derive(Clone)]
pub(super) struct Snapshot {
    id: ProjectionId,
    authority: Authority,
    icon: ImageSrc,
    feedback: Vec<(String, bool)>,
    colors: ThemeColors,
    appearance: vega_theme::Appearance,
    language: &'static str,
    selected: Option<Action>,
    selection_revision: u64,
    reveal_revision: Option<u64>,
    keyboard_active: bool,
    recovery: bool,
}

impl PartialEq for Snapshot {
    fn eq(&self, other: &Self) -> bool {
        self.id.matches(&other.id)
            && self.authority == other.authority
            && self.feedback == other.feedback
            && self.appearance == other.appearance
            && self.language == other.language
            && self.selection_revision == other.selection_revision
            && self.reveal_revision == other.reveal_revision
            && self.selected == other.selected
            && self.keyboard_active == other.keyboard_active
            && self.recovery == other.recovery
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    Controller(CardAction),
    Back,
    Diagnostics,
    Reload,
}

enum Request {
    Action {
        observed: Box<Snapshot>,
        action: Action,
        reply: oneshot::Sender<bool>,
    },
    DragBounds {
        observed: Box<Snapshot>,
        bounds: DragBounds,
    },
    Reveal {
        observed: Box<Snapshot>,
        action: Action,
        reply: oneshot::Sender<bool>,
    },
}

impl Snapshot {
    fn drag_available(&self) -> bool {
        self.authority.stage == Stage::Guide
            && self.authority.placed
            && self.authority.controller.open
            && self.authority.controller.can_drag
            && self.authority.local_fault.is_none()
            && !self.recovery
    }

    fn accepts_action(&self, observed: &Self, action: Action) -> bool {
        self.id.matches(&observed.id)
            && self.recovery == observed.recovery
            && self.authority.stage_identity == observed.authority.stage_identity
            && self.authority.stage == observed.authority.stage
            && self.authority.pane == observed.authority.pane
            && observed.permits(action)
            && self.permits(action)
            && (action != Action::Controller(CardAction::Recheck)
                || (observed.authority.preparation_revision == self.authority.preparation_revision
                    && observed.authority.preparation == self.authority.preparation))
    }

    fn accepts_drag(&self, observed: &Self) -> bool {
        self.id.matches(&observed.id)
            && self.drag_available()
            && observed.drag_available()
            && self.authority.stage_identity == observed.authority.stage_identity
            && self.authority.pane == observed.authority.pane
    }

    fn permits(&self, action: Action) -> bool {
        if !self.authority.controller.open {
            return false;
        }
        if self.recovery {
            return match action {
                Action::Reload | Action::Diagnostics => true,
                Action::Back => self.authority.stage == Stage::Guide,
                Action::Controller(CardAction::Close) => true,
                Action::Controller(CardAction::ReturnToTask) => self.authority.has_task,
                Action::Controller(CardAction::Recheck) => self.authority.preparation.can_recheck,
                _ => false,
            };
        }
        match action {
            Action::Controller(CardAction::Close) => true,
            Action::Controller(CardAction::Recheck) => {
                self.authority.stage == Stage::Overview && self.authority.preparation.can_recheck
            }
            Action::Controller(CardAction::ReturnToTask) => {
                self.authority.stage == Stage::Overview && self.authority.has_task
            }
            Action::Controller(CardAction::AccessibilitySettings)
            | Action::Controller(CardAction::ScreenRecordingSettings) => {
                self.authority.stage == Stage::Overview
                    || (self.authority.local_fault.is_some()
                        && matches!(
                            (action, self.authority.pane),
                            (
                                Action::Controller(CardAction::AccessibilitySettings),
                                PermissionPane::Accessibility
                            ) | (
                                Action::Controller(CardAction::ScreenRecordingSettings),
                                PermissionPane::ScreenRecording
                            )
                        ))
            }
            Action::Controller(CardAction::RevealInFinder) => {
                self.authority.stage == Stage::Guide
                    && (self.authority.local_fault.is_none()
                        || self.authority.path_fault
                        || !self.authority.controller.can_drag)
            }
            Action::Back => self.authority.stage == Stage::Guide,
            Action::Diagnostics => self.authority.warning.is_some(),
            Action::Reload => false,
        }
    }
}

impl GuidanceCard {
    fn nsh_snapshot(&self, id: ProjectionId, recovery: bool, cx: &App) -> Snapshot {
        let preparation = self.preparation.read(cx).snapshot();
        let local_fault = self
            .path_error
            .as_ref()
            .map(|copy| copy.render())
            .or_else(|| self.panel_fault.map(|fault| fault.message().to_owned()))
            .or_else(|| {
                matches!(self.tracking, Tracking::Waiting).then(|| {
                    vega_i18n::t("ui.system_settings_window_not_found_open_the_relevant").to_owned()
                })
            });
        let warning = local_fault
            .clone()
            .or_else(|| preparation.guide_warning.map(str::to_owned));
        Snapshot {
            id,
            authority: Authority {
                stage: self.stage,
                stage_identity: self.stage_identity.clone(),
                pane: self.permission_pane,
                has_task: self.has_task,
                placed: !self.placement_pending,
                controller: self.controller.view(),
                preparation: preparation.view,
                preparation_revision: self.preparation_revision,
                retained: preparation.retained,
                local_fault,
                warning,
                path_fault: self.path_error.is_some(),
            },
            icon: self.app_icon.clone(),
            feedback: preparation
                .feedback
                .into_iter()
                .map(|feedback| (feedback.message, feedback.warning))
                .collect(),
            colors: vega_theme::theme(cx).colors,
            appearance: vega_theme::theme(cx).appearance,
            language: vega_i18n::language(),
            selected: self
                .presentation
                .as_ref()
                .and_then(|page| page.keyboard.as_ref().map(|selection| selection.action)),
            selection_revision: self
                .presentation
                .as_ref()
                .map_or(0, |page| page.selection_revision),
            reveal_revision: self
                .presentation
                .as_ref()
                .and_then(|page| page.reveal_revision),
            keyboard_active: false,
            recovery,
        }
    }

    pub(super) fn observed_drag(&self, cx: &App) -> Option<Snapshot> {
        let page = self.presentation.as_ref()?;
        let snapshot = self.nsh_snapshot(page.id.clone(), page.recovery.is_some(), cx);
        snapshot.drag_available().then_some(snapshot)
    }

    pub(super) fn accepts_drag(&self, observed: &Snapshot, cx: &App) -> bool {
        let Some(page) = &self.presentation else {
            return false;
        };
        if !page.id.matches(&observed.id) || page.recovery.is_some() {
            return false;
        }
        let current = self.nsh_snapshot(page.id.clone(), false, cx);
        current.accepts_drag(observed)
    }

    fn accepts_nsh(&self, observed: &Snapshot, action: Action, cx: &App) -> bool {
        let Some(page) = &self.presentation else {
            return false;
        };
        if !page.id.matches(&observed.id) || page.recovery.is_some() != observed.recovery {
            return false;
        }
        let current = self.nsh_snapshot(page.id.clone(), observed.recovery, cx);
        current.accepts_action(observed, action)
    }

    pub(super) fn nsh_action(
        &mut self,
        observed: &Snapshot,
        action: Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.accepts_nsh(observed, action, cx) {
            return false;
        }
        self.select_nsh_control(observed, action, cx);
        match action {
            Action::Controller(action) => self.dispatch(action, window, cx),
            Action::Back => self.show_overview(window, cx),
            Action::Diagnostics => {
                cx.set_global(crate::settings::NativePreparationSettingsRequested(true));
                cx.set_global(crate::settings::SettingsOpen(true));
            }
            Action::Reload => {
                self.presentation = None;
                window.focus(&self.focus, cx);
            }
        }
        cx.notify();
        true
    }

    fn listen_nsh(
        &self,
        mut requests: mpsc::Receiver<Request>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.spawn_in(window, async move |this, cx| {
            while let Some(request) = requests.recv().await {
                if this
                    .update_in(cx, |card, window, cx| match request {
                        Request::Action {
                            observed,
                            action,
                            reply,
                        } => {
                            let accepted = card.nsh_action(&observed, action, window, cx);
                            let _ = reply.send(accepted);
                        }
                        Request::Reveal {
                            observed,
                            action,
                            reply,
                        } => {
                            let accepted = card.presentation.as_ref().is_some_and(|page| {
                                let mut current =
                                    card.nsh_snapshot(page.id.clone(), page.recovery.is_some(), cx);
                                current.keyboard_active = card.focus.is_focused(window);
                                current.accepts_reveal(&observed, action)
                            });
                            let _ = reply.send(accepted);
                        }
                        Request::DragBounds { observed, bounds } => {
                            if card.accepts_drag(&observed, cx) {
                                let bounds = bounds.valid(card.stage).then_some(bounds);
                                if let Some(page) = &mut card.presentation
                                    && page.drag_bounds != bounds
                                {
                                    page.drag_bounds = bounds;
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
    }

    pub(super) fn render_nsh(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<nsh_platform_gpui::AppView>> {
        if self.presentation.is_none() {
            let id = ProjectionId::new();
            let (surface, requests) =
                Surface::new(self.nsh_snapshot(id.clone(), false, cx), render, window, cx);
            transparent(&surface, cx);
            self.listen_nsh(requests, window, cx);
            let mut errors = surface.errors();
            cx.spawn(async move |card, cx| {
                while errors.changed().await.is_ok() {
                    if card.update(cx, |_, cx| cx.notify()).is_err() {
                        break;
                    }
                }
            })
            .detach();
            let focus = cx.on_focus(&self.focus, window, |_, _, cx| cx.notify());
            let blur = cx.on_blur(&self.focus, window, |_, _, cx| cx.notify());
            window.focus(&self.focus, cx);
            self.presentation = Some(Presentation {
                surface,
                recovery: None,
                id,
                keyboard: None,
                pointer_pending: false,
                selection_revision: 0,
                reveal_revision: None,
                _focus: focus,
                _blur: blur,
                drag_bounds: None,
            });
        }
        let failed = self
            .presentation
            .as_ref()
            .is_some_and(|page| page.surface.errors().borrow().is_some());
        if failed
            && self
                .presentation
                .as_ref()
                .is_some_and(|page| page.recovery.is_none())
        {
            let id = self
                .presentation
                .as_ref()
                .map(|page| page.id.clone())
                .unwrap_or_default();
            let (surface, requests) =
                Surface::new(self.nsh_snapshot(id, true, cx), render_recovery, window, cx);
            transparent(&surface, cx);
            self.listen_nsh(requests, window, cx);
            if let Some(page) = &mut self.presentation {
                page.recovery = Some(surface);
                page.drag_bounds = None;
                page.reset_keyboard();
            }
            window.focus(&self.focus, cx);
        }
        let page = self.presentation.as_ref();
        let snapshot = page.map(|page| {
            let mut snapshot = self.nsh_snapshot(page.id.clone(), page.recovery.is_some(), cx);
            snapshot.keyboard_active = self.focus.is_focused(window);
            snapshot
        });
        let snapshot = snapshot.map(|mut snapshot| {
            if let Some(page) = &mut self.presentation {
                if !page.pointer_pending {
                    page.keyboard =
                        keyboard::Selection::reconcile(page.keyboard.as_ref(), &snapshot);
                }
                snapshot.selected = page.keyboard.as_ref().map(|selection| selection.action);
            }
            snapshot
        });
        if let Some(page) = &mut self.presentation {
            let surface = page.recovery.as_mut().unwrap_or(&mut page.surface);
            surface.sync_environment(cx);
            if let Some(snapshot) = snapshot {
                surface.publish(snapshot);
            }
            Some(surface.entity())
        } else {
            None
        }
    }
}

pub(super) fn transparent<S: Clone + Send + Sync + 'static, C: Send + 'static>(
    surface: &Surface<S, C>,
    cx: &mut App,
) {
    let mut theme = nsh_platform_gpui::Theme::default();
    theme.background = AdaptiveColor::TRANSPARENT;
    surface
        .entity()
        .update(cx, |view, cx| view.set_theme(Some(theme), cx));
}

#[derive(Clone, Copy)]
struct Actions {
    state: Signal<Snapshot>,
    failed: Signal<bool>,
    viewport: Option<reveal::Viewport>,
    reveal_attempted: Signal<Option<u64>>,
}

impl Actions {
    fn send(self, commands: mpsc::Sender<Request>, observed: Snapshot, action: Action) {
        self.failed.set(false);
        spawn(async move {
            let (reply, response) = oneshot::channel();
            if commands
                .send(Request::Action {
                    observed: Box::new(observed),
                    action,
                    reply,
                })
                .await
                .is_err()
                || !matches!(response.await, Ok(true))
            {
                self.failed.set(true);
            }
        });
    }
}

fn control(
    actions: Actions,
    commands: mpsc::Sender<Request>,
    label: &'static str,
    action: Action,
) -> View {
    control_target(actions, commands, label, action, element_ref())
}

fn control_target(
    actions: Actions,
    commands: mpsc::Sender<Request>,
    label: &'static str,
    action: Action,
    target: ElementRef,
) -> View {
    let observed = actions.state.get_untracked();
    if let Some(viewport) = actions.viewport {
        viewport.observe(
            actions.state,
            actions.reveal_attempted,
            commands.clone(),
            action,
            target,
        );
    }

    let primary = matches!(
        action,
        Action::Controller(CardAction::AccessibilitySettings | CardAction::ScreenRecordingSettings)
    );
    view()
        .element_ref(&target)
        .role(AccessibilityRole::Button)
        .label(label)
        .focusable(true)
        .disabled(move || !actions.state.get().permits(action))
        .flex_shrink_0()
        .h(px(Layout::PREPARATION_GUIDE_CONTROL))
        .px(px(Layout::CONTENT_PADDING))
        .items_center()
        .justify_center()
        .rounded(px(Layout::PANEL_RADIUS))
        .border(px(1.))
        .border_color(move || {
            let snapshot = actions.state.get();
            crate::nsh::color(
                if snapshot.keyboard_active && snapshot.selected == Some(action) {
                    actions.state.get().colors.accent
                } else {
                    actions.state.get().colors.border_subtle
                },
            )
        })
        .bg(move || {
            crate::nsh::color(if primary {
                actions.state.get().colors.brand_primary
            } else {
                actions.state.get().colors.bg_subtle
            })
        })
        .text_color(move || {
            crate::nsh::color(if !actions.state.get().permits(action) {
                actions.state.get().colors.text_tertiary
            } else if primary {
                actions.state.get().colors.brand_on_accent
            } else {
                actions.state.get().colors.text_secondary
            })
        })
        .hover(move |style| {
            style.bg(move || {
                crate::nsh::color(if primary {
                    actions.state.get().colors.brand_primary_strong
                } else {
                    actions.state.get().colors.bg_hover
                })
            })
        })
        .active(move |style| {
            style.bg(move || {
                crate::nsh::color(if primary {
                    actions.state.get().colors.brand_primary_strong
                } else {
                    actions.state.get().colors.bg_active
                })
            })
        })
        .on_press(move || {
            let mut observed = observed.clone();
            observed.selection_revision = actions.state.get_untracked().selection_revision;
            actions.send(commands.clone(), observed, action);
        })
        .child(match action {
            Action::Controller(CardAction::Close) => glyph(
                crate::icons::Icon::Close,
                actions.state.get_untracked().colors,
                Layout::ICON_SIZE,
            ),
            Action::Back => glyph(
                crate::icons::Icon::ArrowLeft,
                actions.state.get_untracked().colors,
                Layout::ICON_SIZE,
            ),
            Action::Controller(CardAction::RevealInFinder)
                if label == vega_i18n::t("ui.vega_drag_into_the_permission_list_click_or") =>
            {
                view()
                    .flex_row()
                    .items_center()
                    .gap(px(Layout::IMAGE_PREVIEW_GAP))
                    .child(
                        image(actions.state.get_untracked().icon)
                            .size(px(Layout::PREPARATION_APP_ICON_SIZE))
                            .label("Vega"),
                    )
                    .child(text("Vega"))
                    .into_element()
            }
            _ => text(label).into_element(),
        })
}

fn glyph(kind: crate::icons::Icon, colors: ThemeColors, size: f32) -> AnyElement {
    match crate::icons::nsh_source(kind) {
        Some(source) => svg(ImageSrc::svg_markup(&source))
            .size(px(size))
            .text_color(crate::nsh::color(colors.brand_primary))
            .into_element(),
        None => view().size(px(size)).into_element(),
    }
}

fn permission_row(
    snapshot: &Snapshot,
    pane: PermissionPane,
    actions: Actions,
    commands: mpsc::Sender<Request>,
) -> AnyElement {
    let (title, detail, kind, state, action) = match pane {
        PermissionPane::Accessibility => (
            vega_i18n::t("ui.accessibility"),
            vega_i18n::t("ui.allow_vega_to_access_app_interfaces"),
            crate::icons::Icon::Hand,
            snapshot.authority.preparation.accessibility,
            CardAction::AccessibilitySettings,
        ),
        PermissionPane::ScreenRecording => (
            vega_i18n::t("ui.screen_recording"),
            vega_i18n::t("ui.use_screenshots_to_identify_interfaces_and_interaction_targets"),
            crate::icons::Icon::Camera,
            snapshot.authority.preparation.screen_recording,
            CardAction::ScreenRecordingSettings,
        ),
    };
    let status = if snapshot.authority.preparation.check_state == CheckState::Checking {
        vega_i18n::t("ui.checking_194")
    } else {
        match state {
            PermissionState::Granted => vega_i18n::t("ui.enabled"),
            PermissionState::Denied => vega_i18n::t("ui.not_authorized"),
            PermissionState::Unknown
                if snapshot.authority.preparation.check_state == CheckState::Idle
                    && !snapshot.authority.retained =>
            {
                vega_i18n::t("ui.not_checked")
            }
            PermissionState::Unknown => vega_i18n::t("ui.status_unknown"),
        }
    };
    view()
        .w_full()
        .min_h(px(Layout::PREPARATION_PERMISSION_HEIGHT))
        .flex_row()
        .items_center()
        .gap(px(Layout::CONTENT_PADDING))
        .p(px(Layout::CONTENT_PADDING))
        .rounded(px(Layout::ENVIRONMENT_CARD_RADIUS))
        .bg(crate::nsh::color(snapshot.colors.bg_subtle))
        .border(px(1.))
        .border_color(crate::nsh::color(snapshot.colors.border_subtle))
        .child(glyph(
            kind,
            snapshot.colors,
            Layout::PREPARATION_PERMISSION_ICON,
        ))
        .child(
            view()
                .flex_1()
                .min_w_0()
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(Layout::IMAGE_PREVIEW_GAP))
                        .child(
                            text(title)
                                .text_size(px(Typography::HEADING_PAGE))
                                .font_weight(FontWeight(Typography::HEADING_PAGE_WEIGHT.0 as u16)),
                        )
                        .child(
                            text(status)
                                .text_size(px(Typography::METADATA))
                                .text_color(crate::nsh::color(snapshot.colors.text_secondary)),
                        ),
                )
                .child(text(detail).text_color(crate::nsh::color(snapshot.colors.text_secondary))),
        )
        .child(control(
            actions,
            commands,
            if state == PermissionState::Granted {
                vega_i18n::t("ui.open_settings")
            } else {
                vega_i18n::t("ui.allow")
            },
            Action::Controller(action),
        ))
        .into_element()
}

fn overview(
    snapshot: &Snapshot,
    actions: Actions,
    commands: mpsc::Sender<Request>,
    focus_target: ElementRef,
) -> AnyElement {
    let viewport = reveal::Viewport::new(element_ref());
    let content_actions = Actions {
        viewport: Some(viewport),
        ..actions
    };
    let mut content = view()
        .p(px(Layout::PREPARATION_OVERVIEW_PADDING))
        .gap(px(Layout::CONTENT_PADDING));
    if snapshot.authority.local_fault.is_some() {
        content = content.child(
            text(vega_i18n::t(
                "ui.cannot_locate_the_setup_window_close_and_reopen",
            ))
            .text_color(crate::nsh::color(snapshot.colors.warning)),
        );
    }
    content = content
        .child(
            view()
                .items_center()
                .gap(px(Layout::IMAGE_PREVIEW_GAP))
                .child(
                    image(snapshot.icon.clone())
                        .size(px(Layout::PREPARATION_OVERVIEW_ICON))
                        .label("Vega"),
                )
                .child(
                    text(vega_i18n::t("ui.enable_vega_computer_use"))
                        .text_size(px(Typography::EMPTY_STATE_TITLE))
                        .font_weight(FontWeight(Typography::EMPTY_STATE_TITLE_WEIGHT.0 as u16)),
                )
                .child(
                    view()
                        .max_w(px(Layout::PREPARATION_OVERVIEW_DESCRIPTION_WIDTH))
                        .items_center()
                        .text_color(crate::nsh::color(snapshot.colors.text_secondary))
                        .child(text(vega_i18n::t(
                            "ui.vega_needs_the_following_permissions_to_control_apps",
                        )))
                        .child(text(vega_i18n::t(
                            "ui.used_only_when_you_request_computer_control",
                        ))),
                ),
        )
        .child(permission_row(
            snapshot,
            PermissionPane::Accessibility,
            content_actions,
            commands.clone(),
        ))
        .child(permission_row(
            snapshot,
            PermissionPane::ScreenRecording,
            content_actions,
            commands.clone(),
        ));
    let mut row = view()
        .flex_row()
        .flex_wrap()
        .gap(px(Layout::IMAGE_PREVIEW_GAP))
        .child(control(
            content_actions,
            commands.clone(),
            if snapshot.authority.preparation.can_recheck {
                vega_i18n::t("ui.check_again")
            } else {
                vega_i18n::t("ui.checking_194")
            },
            Action::Controller(CardAction::Recheck),
        ));
    if snapshot.authority.warning.is_some() {
        row = row.child(control(
            content_actions,
            commands.clone(),
            vega_i18n::t("ui.permissions_and_checks"),
            Action::Diagnostics,
        ));
    }
    if snapshot.authority.has_task {
        row = row.child(control(
            content_actions,
            commands.clone(),
            vega_i18n::t("ui.back_to_task"),
            Action::Controller(CardAction::ReturnToTask),
        ));
    }
    content = content.child(row);
    for (message, warning) in &snapshot.feedback {
        content = content.child(
            text(message.clone()).text_color(crate::nsh::color(if *warning {
                snapshot.colors.warning
            } else {
                snapshot.colors.text_secondary
            })),
        );
    }
    content = content.child(
        text(vega_i18n::t(
            "ui.after_changing_screen_recording_permissions_quit_and_reopen",
        ))
        .text_size(px(Typography::METADATA))
        .text_color(crate::nsh::color(snapshot.colors.text_secondary)),
    );
    view()
        .size_full()
        .relative()
        .child(
            scroll()
                .element_ref(&viewport.target)
                .size_full()
                .child(content),
        )
        .child(
            control_target(
                actions,
                commands,
                vega_i18n::t("ui.close_setup"),
                Action::Controller(CardAction::Close),
                focus_target,
            )
            .w(px(Layout::PREPARATION_CLOSE_SIZE))
            .h(px(Layout::PREPARATION_CLOSE_SIZE))
            .px(px(0.))
            .absolute()
            .top(px(Layout::CONTENT_PADDING))
            .right(px(Layout::CONTENT_PADDING)),
        )
        .into_element()
}

fn guide(
    snapshot: &Snapshot,
    actions: Actions,
    commands: mpsc::Sender<Request>,
    focus_target: ElementRef,
) -> AnyElement {
    let instruction = match snapshot.authority.pane {
        PermissionPane::Accessibility => vega_i18n::t("ui.drag_vega_into_the_list_above_to_allow"),
        PermissionPane::ScreenRecording => {
            vega_i18n::t("ui.drag_vega_into_the_list_above_to_allow_214")
        }
    };
    let header = view()
        .min_h(px(Layout::PREPARATION_GUIDE_CONTROL))
        .flex_shrink_0()
        .flex_row()
        .items_center()
        .gap(px(Layout::IMAGE_PREVIEW_GAP))
        .child(glyph(
            crate::icons::Icon::ArrowUp,
            snapshot.colors,
            Layout::PREPARATION_GUIDE_CONTROL,
        ))
        .child(
            text(
                snapshot
                    .authority
                    .warning
                    .as_deref()
                    .unwrap_or(instruction)
                    .to_owned(),
            )
            .flex_1()
            .min_w_0()
            .text_color(crate::nsh::color(if snapshot.authority.warning.is_some() {
                snapshot.colors.warning
            } else {
                snapshot.colors.text_secondary
            })),
        )
        .child(
            control(
                actions,
                commands.clone(),
                vega_i18n::t("ui.close_setup"),
                Action::Controller(CardAction::Close),
            )
            .w(px(Layout::PREPARATION_CLOSE_SIZE))
            .h(px(Layout::PREPARATION_CLOSE_SIZE))
            .px(px(0.)),
        );
    let drag_target = element_ref();
    let drag_viewport = element_ref();
    let fixed_actions = actions;
    let actions = Actions {
        viewport: Some(reveal::Viewport::new(drag_viewport)),
        ..actions
    };
    let observed_layout = snapshot.clone();
    let layout_commands = commands.clone();
    let update_drag = move || {
        let observed = observed_layout.clone();
        let commands = layout_commands.clone();
        spawn(async move {
            if let Some(measured) = drag_target.measure().await
                && let Some(viewport) = drag_viewport.measure().await
            {
                let x = measured.page.x.0.max(viewport.page.x.0);
                let y = measured.page.y.0.max(viewport.page.y.0);
                let right = (measured.page.x.0 + measured.frame.size.width.0)
                    .min(viewport.page.x.0 + viewport.frame.size.width.0);
                let bottom = (measured.page.y.0 + measured.frame.size.height.0)
                    .min(viewport.page.y.0 + viewport.frame.size.height.0);
                let bounds = DragBounds {
                    x: x - Layout::PREPARATION_CARD_SHADOW_PADDING,
                    y: y - Layout::PREPARATION_CARD_SHADOW_PADDING,
                    width: (right - x).max(0.),
                    height: (bottom - y).max(0.),
                };
                let _ = commands
                    .send(Request::DragBounds {
                        observed: Box::new(observed),
                        bounds,
                    })
                    .await;
            }
        });
    };
    let scroll_drag = update_drag.clone();
    let mut body = view().gap(px(Layout::PREPARATION_GUIDE_GAP));
    if snapshot.authority.local_fault.is_none() {
        if snapshot.authority.controller.can_drag {
            body = body.child(
                control_target(
                    actions,
                    commands.clone(),
                    vega_i18n::t("ui.vega_drag_into_the_permission_list_click_or"),
                    Action::Controller(CardAction::RevealInFinder),
                    drag_target,
                )
                .w_full()
                .h(px(Layout::PREPARATION_APP_TARGET_HEIGHT))
                .px(px(Layout::IMAGE_PREVIEW_GAP))
                .flex_row()
                .justify_start()
                .on_layout(move |_: &Bounds<Points>| update_drag()),
            );
        } else {
            body = body.child(
                view()
                    .gap(px(Layout::IMAGE_PREVIEW_GAP))
                    .text_color(crate::nsh::color(snapshot.colors.text_secondary))
                    .child(text(vega_i18n::t(
                        "ui.this_is_a_development_executable_and_cannot_be",
                    )))
                    .child(control(
                        actions,
                        commands.clone(),
                        vega_i18n::t("ui.show_in_finder"),
                        Action::Controller(CardAction::RevealInFinder),
                    )),
            );
        }
    }
    if snapshot.authority.warning.is_some() {
        let mut row = view()
            .flex_row()
            .flex_wrap()
            .gap(px(Layout::IMAGE_PREVIEW_GAP));
        if snapshot.authority.local_fault.is_some() {
            let action = match snapshot.authority.pane {
                PermissionPane::Accessibility => CardAction::AccessibilitySettings,
                PermissionPane::ScreenRecording => CardAction::ScreenRecordingSettings,
            };
            row = row.child(control(
                actions,
                commands.clone(),
                vega_i18n::t("ui.open_permission_settings"),
                Action::Controller(action),
            ));
            if snapshot.authority.path_fault || !snapshot.authority.controller.can_drag {
                row = row.child(control(
                    actions,
                    commands.clone(),
                    vega_i18n::t("ui.show_in_finder"),
                    Action::Controller(CardAction::RevealInFinder),
                ));
            }
        }
        body = body.child(row.child(control(
            actions,
            commands.clone(),
            vega_i18n::t("ui.permissions_and_checks"),
            Action::Diagnostics,
        )));
    }
    view()
        .size_full()
        .p(px(Layout::CONTENT_PADDING))
        .flex_row()
        .items_center()
        .gap(px(Layout::PREPARATION_GUIDE_GAP))
        .child(
            control_target(
                fixed_actions,
                commands,
                vega_i18n::t("ui.back_to_permission_setup"),
                Action::Back,
                focus_target,
            )
            .w(px(Layout::PREPARATION_GUIDE_CONTROL))
            .px(px(0.)),
        )
        .child(
            view()
                .flex_1()
                .min_w_0()
                .h_full()
                .gap(px(Layout::PREPARATION_GUIDE_GAP))
                .child(header)
                .child(
                    scroll()
                        .element_ref(&drag_viewport)
                        .flex_1()
                        .min_h_0()
                        .on_scroll(move |_: &ScrollEvent| scroll_drag())
                        .child(body),
                ),
        )
        .into_element()
}

fn page(incoming: Signal<Snapshot>, commands: mpsc::Sender<Request>, recovery: bool) -> AnyElement {
    let state = signal(incoming.get_untracked());
    let content = signal(incoming.get_untracked());
    effect(move || {
        let next = incoming.get();
        state.set(next.clone());
        let mut shown = content.get_untracked();
        shown.selected = next.selected;
        shown.selection_revision = next.selection_revision;
        shown.reveal_revision = next.reveal_revision;
        shown.keyboard_active = next.keyboard_active;
        if shown != next {
            content.set(next);
        }
    });
    let actions = Actions {
        state,
        failed: signal(false),
        viewport: None,
        reveal_attempted: signal(None),
    };
    let target = element_ref();
    view()
        .size_full()
        .p(px(Layout::PREPARATION_CARD_SHADOW_PADDING))
        .child(dynamic(move || {
            let snapshot = content.get();
            let (width, height) = snapshot.authority.stage.surface_size();
            let mut card = view()
                .w(px(width))
                .h(px(height))
                .relative()
                .rounded(px(Layout::PANEL_RADIUS))
                .border(px(1.))
                .border_color(crate::nsh::color(snapshot.colors.border_subtle))
                .bg(crate::nsh::color(snapshot.colors.bg_elevated))
                .shadow_lg()
                .text_size(px(Typography::BODY))
                .text_color(crate::nsh::color(snapshot.colors.text_primary));
            if recovery {
                let viewport = reveal::Viewport::new(element_ref());
                let actions = Actions {
                    viewport: Some(viewport),
                    ..actions
                };
                let mut controls = view()
                    .p(px(Layout::CONTENT_PADDING))
                    .flex_row()
                    .flex_wrap()
                    .gap(px(Layout::IMAGE_PREVIEW_GAP))
                    .child(text(vega_i18n::t("ui.command_failed")))
                    .child(control_target(
                        actions,
                        commands.clone(),
                        vega_i18n::t("ui.close_setup"),
                        Action::Controller(CardAction::Close),
                        target,
                    ))
                    .child(control(
                        actions,
                        commands.clone(),
                        vega_i18n::t("ui.check_again"),
                        Action::Controller(CardAction::Recheck),
                    ))
                    .child(control(
                        actions,
                        commands.clone(),
                        vega_i18n::t("ui.permissions_and_checks"),
                        Action::Diagnostics,
                    ))
                    .child(control(
                        actions,
                        commands.clone(),
                        vega_i18n::t("ui.retry"),
                        Action::Reload,
                    ));
                if snapshot.authority.stage == Stage::Guide {
                    controls = controls.child(control(
                        actions,
                        commands.clone(),
                        vega_i18n::t("ui.back_to_permission_setup"),
                        Action::Back,
                    ));
                }
                if snapshot.authority.has_task {
                    controls = controls.child(control(
                        actions,
                        commands.clone(),
                        vega_i18n::t("ui.back_to_task"),
                        Action::Controller(CardAction::ReturnToTask),
                    ));
                }
                card = card.child(
                    scroll()
                        .element_ref(&viewport.target)
                        .size_full()
                        .child(controls),
                );
            } else {
                card = card.child(match snapshot.authority.stage {
                    Stage::Overview => overview(&snapshot, actions, commands.clone(), target),
                    Stage::Guide => guide(&snapshot, actions, commands.clone(), target),
                });
            }
            card.child(when(
                move || actions.failed.get(),
                move || {
                    text(vega_i18n::t("ui.command_failed"))
                        .absolute()
                        .bottom_0()
                        .text_color(crate::nsh::color(state.get().colors.warning))
                },
            ))
            .into_element()
        }))
        .into_element()
}

fn render(state: Signal<Snapshot>, commands: mpsc::Sender<Request>) -> AnyElement {
    page(state, commands, false)
}
fn render_recovery(state: Signal<Snapshot>, commands: mpsc::Sender<Request>) -> AnyElement {
    page(state, commands, true)
}

#[derive(Clone)]
pub(super) struct PreviewSnapshot {
    pub(super) icon: ImageSrc,
    pub(super) colors: ThemeColors,
}

pub(super) fn render_preview(state: Signal<PreviewSnapshot>, _: mpsc::Sender<()>) -> AnyElement {
    view()
        .size_full()
        .flex_row()
        .items_center()
        .gap(px(Layout::IMAGE_PREVIEW_GAP))
        .text_size(px(Typography::BODY))
        .text_color(move || crate::nsh::color(state.get().colors.text_primary))
        .child(
            image(state.get_untracked().icon)
                .size(px(Layout::PREPARATION_APP_ICON_SIZE))
                .label("Vega"),
        )
        .child(text("Vega"))
        .into_element()
}
