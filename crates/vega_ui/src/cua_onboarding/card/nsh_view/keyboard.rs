use super::{Action, GuidanceCard, Snapshot, Stage};
use crate::cua_onboarding::PermissionPane;
use crate::cua_onboarding::controller::CardAction;
use gpui_kit::prelude::*;
use gpui_kit::{Context, KeyDownEvent, Window, div};
use vega_conversation::types::PermissionState;

pub(super) struct Control {
    pub(super) action: Action,
    pub(super) label: &'static str,
}

impl Snapshot {
    pub(super) fn controls(&self) -> Vec<Control> {
        let mut controls = Vec::new();
        let mut add = |action, key| {
            controls.push(Control {
                action,
                label: vega_i18n::t(key),
            });
        };
        if self.recovery {
            add(Action::Controller(CardAction::Close), "ui.close_setup");
            add(Action::Controller(CardAction::Recheck), "ui.check_again");
            add(Action::Diagnostics, "ui.permissions_and_checks");
            add(Action::Reload, "ui.retry");
            if self.authority.stage == Stage::Guide {
                add(Action::Back, "ui.back_to_permission_setup");
            }
            if self.authority.has_task {
                add(
                    Action::Controller(CardAction::ReturnToTask),
                    "ui.back_to_task",
                );
            }
        } else if self.authority.stage == Stage::Overview {
            add(Action::Controller(CardAction::Close), "ui.close_setup");
            for (action, state) in [
                (
                    CardAction::AccessibilitySettings,
                    self.authority.preparation.accessibility,
                ),
                (
                    CardAction::ScreenRecordingSettings,
                    self.authority.preparation.screen_recording,
                ),
            ] {
                add(
                    Action::Controller(action),
                    if state == PermissionState::Granted {
                        "ui.open_settings"
                    } else {
                        "ui.allow"
                    },
                );
            }
            add(
                Action::Controller(CardAction::Recheck),
                if self.authority.preparation.can_recheck {
                    "ui.check_again"
                } else {
                    "ui.checking_194"
                },
            );
            if self.authority.warning.is_some() {
                add(Action::Diagnostics, "ui.permissions_and_checks");
            }
            if self.authority.has_task {
                add(
                    Action::Controller(CardAction::ReturnToTask),
                    "ui.back_to_task",
                );
            }
        } else {
            add(Action::Back, "ui.back_to_permission_setup");
            add(Action::Controller(CardAction::Close), "ui.close_setup");
            if self.authority.local_fault.is_none() {
                add(
                    Action::Controller(CardAction::RevealInFinder),
                    if self.authority.controller.can_drag {
                        "ui.vega_drag_into_the_permission_list_click_or"
                    } else {
                        "ui.show_in_finder"
                    },
                );
            }
            if self.authority.warning.is_some() {
                if self.authority.local_fault.is_some() {
                    add(
                        Action::Controller(match self.authority.pane {
                            PermissionPane::Accessibility => CardAction::AccessibilitySettings,
                            PermissionPane::ScreenRecording => CardAction::ScreenRecordingSettings,
                        }),
                        "ui.open_permission_settings",
                    );
                    if self.authority.path_fault || !self.authority.controller.can_drag {
                        add(
                            Action::Controller(CardAction::RevealInFinder),
                            "ui.show_in_finder",
                        );
                    }
                }
                add(Action::Diagnostics, "ui.permissions_and_checks");
            }
        }
        controls
    }

    fn initial_control(&self) -> Option<Action> {
        let initial = if !self.recovery && self.authority.stage == Stage::Guide {
            Action::Back
        } else {
            Action::Controller(CardAction::Close)
        };
        self.permits(initial).then_some(initial)
    }
}

pub(super) struct Selection {
    pub(super) observed: Snapshot,
    pub(super) action: Action,
}

impl Selection {
    fn same_route(&self, current: &Snapshot) -> bool {
        current.id.matches(&self.observed.id)
            && current.recovery == self.observed.recovery
            && current.authority.stage_identity == self.observed.authority.stage_identity
            && current.authority.stage == self.observed.authority.stage
            && current.authority.pane == self.observed.authority.pane
    }

    pub(super) fn reconcile(previous: Option<&Self>, current: &Snapshot) -> Option<Self> {
        let action = previous
            .filter(|previous| {
                previous.same_route(current)
                    && current.permits(previous.action)
                    && current
                        .controls()
                        .iter()
                        .any(|control| control.action == previous.action)
            })
            .map(|previous| previous.action)
            .or_else(|| current.initial_control())?;
        Some(Self {
            observed: current.clone(),
            action,
        })
    }

    pub(super) fn advance(
        previous: Option<&Self>,
        current: &Snapshot,
        backwards: bool,
    ) -> Option<Self> {
        let enabled: Vec<_> = current
            .controls()
            .into_iter()
            .filter(|control| current.permits(control.action))
            .collect();
        if enabled.is_empty() {
            return None;
        }
        let index = previous
            .filter(|previous| previous.same_route(current))
            .and_then(|previous| {
                enabled
                    .iter()
                    .position(|control| control.action == previous.action)
            });
        let index = match (index, backwards) {
            (Some(index), true) => (index + enabled.len() - 1) % enabled.len(),
            (Some(index), false) => (index + 1) % enabled.len(),
            (None, true) => enabled.len() - 1,
            (None, false) => 0,
        };
        Some(Self {
            observed: current.clone(),
            action: enabled[index].action,
        })
    }

    pub(super) fn activatable(&self, current: &Snapshot) -> bool {
        self.same_route(current)
            && current
                .controls()
                .iter()
                .any(|control| control.action == self.action)
            && current.accepts_action(&self.observed, self.action)
    }
}

impl GuidanceCard {
    pub(in crate::cua_onboarding::card) fn select_nsh_control(
        &mut self,
        observed: &Snapshot,
        action: Action,
        cx: &mut Context<Self>,
    ) {
        if self.accepts_nsh(observed, action, cx)
            && observed
                .controls()
                .iter()
                .any(|control| control.action == action)
            && let Some(page) = &mut self.presentation
            && observed.selection_revision == page.selection_revision
        {
            page.keyboard = Some(Selection {
                observed: observed.clone(),
                action,
            });
            page.reveal_revision = None;
            page.pointer_pending = false;
            cx.notify();
        }
    }

    pub(in crate::cua_onboarding::card) fn keyboard_pointer_down(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        if let Some(page) = &mut self.presentation {
            page.keyboard = None;
            page.reveal_revision = None;
            page.pointer_pending = true;
            cx.notify();
        }
    }

    pub(in crate::cua_onboarding::card) fn keyboard_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let modifiers = event.keystroke.modifiers;
        if modifiers.control || modifiers.alt || modifiers.platform || modifiers.function {
            return false;
        }
        let key = event.keystroke.key.as_str();
        if key != "tab" && key != "enter" && key != "space" && key != " " {
            return false;
        }
        let Some(page) = &self.presentation else {
            return false;
        };
        let current = self.nsh_snapshot(page.id.clone(), page.recovery.is_some(), cx);
        if key == "tab" {
            let next = Selection::advance(page.keyboard.as_ref(), &current, modifiers.shift);
            if let Some(page) = &mut self.presentation {
                page.selection_revision = page.selection_revision.wrapping_add(1);
                page.reveal_revision = Some(page.selection_revision);
                page.keyboard = next;
                page.pointer_pending = false;
            }
            window.focus(&self.focus, cx);
            cx.notify();
        } else if !event.is_held && self.focus.is_focused(window) {
            let selected = page
                .keyboard
                .as_ref()
                .filter(|selection| selection.activatable(&current))
                .map(|selection| (selection.observed.clone(), selection.action));
            if let Some((observed, action)) = selected {
                self.nsh_action(&observed, action, window, cx);
            }
        }
        true
    }

    pub(in crate::cua_onboarding::card) fn keyboard_accessibility_host(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui_kit::AnyElement> {
        let page = self.presentation.as_ref()?;
        let selection = page.keyboard.as_ref()?;
        let current = self.nsh_snapshot(page.id.clone(), page.recovery.is_some(), cx);
        if !selection.activatable(&current) || !self.focus.is_focused(window) {
            return None;
        }
        let control = current
            .controls()
            .into_iter()
            .find(|control| control.action == selection.action)?;
        let observed = selection.observed.clone();
        let action = selection.action;
        let weak = cx.weak_entity();
        Some(
            div()
                .id("preparation-keyboard-active-control")
                .absolute()
                .inset_0()
                .role(gpui_kit::Role::Button)
                .aria_label(control.label)
                .aria_active_descendant()
                .on_a11y_action(gpui_kit::AccessibleAction::Click, move |_, window, cx| {
                    let _ = weak.update(cx, |card, cx| {
                        card.nsh_action(&observed, action, window, cx);
                    });
                })
                .into_any_element(),
        )
    }
}
