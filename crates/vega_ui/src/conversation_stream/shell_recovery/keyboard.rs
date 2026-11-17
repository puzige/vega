use super::{Action, Snapshot};
use crate::conversation_stream::ConversationStream;
use crate::nsh::ProjectionId;
use gpui_kit::{Context, FocusHandle, KeyDownEvent, Window};

impl Snapshot {
    pub(super) fn initial_selection(&self) -> Option<Action> {
        let controls = self.controls();
        let preferred = if self.has_confirmation || self.recovery {
            Action::KeepPaused
        } else if self.opened {
            Action::NotNow
        } else {
            Action::Review
        };
        controls
            .iter()
            .find(|control| control.enabled && control.action == preferred)
            .or_else(|| {
                controls
                    .iter()
                    .find(|control| control.enabled && control.action != Action::Accept)
            })
            .map(|control| control.action.clone())
    }

    pub(super) fn next_selection(
        &self,
        selected: Option<&Action>,
        reverse: bool,
    ) -> Option<Action> {
        let controls: Vec<_> = self
            .controls()
            .into_iter()
            .filter(|control| control.enabled)
            .collect();
        if controls.is_empty() {
            return None;
        }
        let position = selected.and_then(|selected| {
            controls
                .iter()
                .position(|control| control.action == *selected)
        });
        let index = match (position, reverse) {
            (Some(index), true) => index.checked_sub(1)?,
            (Some(index), false) => index + 1,
            (None, true) => controls.len() - 1,
            (None, false) => 0,
        };
        controls.get(index).map(|control| control.action.clone())
    }
}

impl ConversationStream {
    pub(super) fn shell_focus(&self) -> &FocusHandle {
        if self.shell_recovery.opened {
            &self.shell_recovery.safe_focus
        } else {
            &self.shell_recovery.review_focus
        }
    }

    pub(super) fn shell_select(&mut self, action: Action, reveal: bool) {
        if let Some(page) = self.shell_page_mut() {
            page.displayed = None;
            page.selected = Some(action);
            page.selection = ProjectionId::new();
            page.reveal = reveal;
        }
    }

    pub(super) fn shell_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.shell_focus().is_focused(window) {
            return false;
        }
        let key = event.keystroke.key.as_str();
        let modifiers = event.keystroke.modifiers;
        if modifiers.control
            || modifiers.alt
            || modifiers.platform
            || modifiers.function
            || !matches!(key, "tab" | "enter" | "space" | " ")
        {
            return false;
        }
        let Some(page) = self.shell_page() else {
            return false;
        };
        let mut current = self.shell_snapshot(page.id.clone(), page.recovery, cx);
        current.keyboard_active = true;
        if page.fault || !current.same_authority(&page.shown) {
            cx.notify();
            return true;
        }
        if key == "tab" {
            if let Some(next) = current.next_selection(page.selected.as_ref(), modifiers.shift) {
                self.shell_select(next, true);
                cx.notify();
                return true;
            }
            return false;
        }
        if !event.is_held
            && let Some(action) = page.selected.clone()
            && let Some(displayed) = &page.displayed
            && current.accepts_selection(displayed, &action)
        {
            let observed = displayed.clone();
            self.shell_action(&observed, action, window, cx);
        }
        true
    }

    pub(super) fn shell_accessibility(
        &self,
        current: &Snapshot,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui_kit::AnyElement> {
        use gpui_kit::prelude::*;
        if !self.shell_focus().is_focused(window) {
            return None;
        }
        let action = current.selected.as_ref()?;
        let displayed = self.shell_page()?.displayed.as_ref()?;
        if !current.accepts_selection(displayed, action) || !self.accepts_shell(current, action, cx)
        {
            return None;
        }
        let control = current
            .controls()
            .into_iter()
            .find(|control| control.action == *action && control.enabled)?;
        let observed = current.clone();
        let action = action.clone();
        let weak = cx.weak_entity();
        Some(
            gpui_kit::div()
                .id("shell-recovery-active-control")
                .absolute()
                .inset_0()
                .role(gpui_kit::Role::Button)
                .aria_label(vega_i18n::t(control.key))
                .aria_active_descendant()
                .on_a11y_action(gpui_kit::AccessibleAction::Click, move |_, window, cx| {
                    let _ = weak.update(cx, |this, cx| {
                        let selected = this.shell_page().is_some_and(|page| {
                            page.displayed.as_ref().is_some_and(|displayed| {
                                observed.accepts_selection(displayed, &action)
                            }) && page.selection.matches(&observed.selection)
                                && page.selected.as_ref() == Some(&action)
                        });
                        if selected && this.shell_focus().is_focused(window) {
                            this.shell_action(&observed, action.clone(), window, cx);
                        }
                    });
                })
                .into_any_element(),
        )
    }
}
