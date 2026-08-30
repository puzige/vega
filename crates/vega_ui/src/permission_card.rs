//! Fail-closed GPUI permission card over the conversation-layer handoff.

use std::fmt;

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Entity, EventEmitter, FocusHandle, Focusable, MouseButton, MouseUpEvent, Task,
    Window, actions, div, px,
};
use vega_conversation::agent::{PERMISSION_TIMEOUT, PermissionLease};
use vega_conversation::types::{PermissionDecision, PermissionRequest};
use vega_theme::{Typography, theme};

use crate::conversation_stream::{MONOFONT, ROW_HEIGHT, display_width};
use crate::text_input::TextInput;

actions!(
    vega_permission_card,
    [
        PermissionEnter,
        PermissionAlways,
        PermissionDeny,
        PermissionNextFocus,
        PermissionPreviousFocus,
        PermissionActivate
    ]
);

/// One permission card resolved or was invalidated externally.
pub struct PermissionCardResolved;

/// Button identity used by the explicit keyboard focus cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PermissionFocus {
    AllowOnce,
    Always,
    Reject,
    Note,
}

/// Safe presentation copied from a transient request after call-id matching.
#[derive(Clone, PartialEq, Eq)]
struct SafePermissionPrompt {
    tool: String,
    display_target: String,
    command_rows: Vec<String>,
    danger_rule_id: Option<String>,
    danger_reason: Option<String>,
}

impl SafePermissionPrompt {
    fn from_request(request: &PermissionRequest) -> Self {
        Self {
            tool: request.tool.clone(),
            display_target: request.display_target.clone(),
            command_rows: wrap_command_rows(&request.display_target),
            danger_rule_id: request.danger_rule_id.clone(),
            danger_reason: request.danger_reason.clone(),
        }
    }

    fn is_danger(&self) -> bool {
        self.danger_rule_id.is_some()
    }
}

impl fmt::Debug for SafePermissionPrompt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SafePermissionPrompt")
            .field("tool", &self.tool)
            .field("danger", &self.is_danger())
            .field("display_target", &"[REDACTED]")
            .finish()
    }
}

/// UI entity that owns the responder-only lease. It never retains a provider
/// call id, write/edit body, fingerprint, or checkpoint reference.
pub struct PermissionCard {
    prompt: SafePermissionPrompt,
    lease: Option<PermissionLease>,
    note: Entity<TextInput>,
    once_focus: FocusHandle,
    always_focus: FocusHandle,
    reject_focus: FocusHandle,
    logical_focus: PermissionFocus,
    initial_focus_pending: bool,
    resolved: bool,
    _timeout_task: Task<()>,
}

impl PermissionCard {
    /// Creates a card after the stream has matched the transient call id,
    /// exact tool, and exact display target to the durable ToolCard.
    pub(crate) fn new(
        request: &PermissionRequest,
        lease: PermissionLease,
        cx: &mut gpui::Context<Self>,
    ) -> Self {
        let note = cx.new(|cx| TextInput::new(cx, "拒绝原因（可选）", false));
        cx.observe(&note, |_, _, cx| cx.notify()).detach();
        let danger = request.danger_rule_id.is_some();
        let timeout_task = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(PERMISSION_TIMEOUT).await;
            let _ = this.update(cx, |this, cx| {
                this.resolve(PermissionDecision::Timeout, cx);
            });
        });
        Self {
            prompt: SafePermissionPrompt::from_request(request),
            lease: Some(lease),
            note,
            once_focus: cx.focus_handle().tab_index(1).tab_stop(true),
            always_focus: cx.focus_handle().tab_index(2).tab_stop(true),
            reject_focus: cx.focus_handle().tab_index(3).tab_stop(true),
            logical_focus: if danger {
                PermissionFocus::Reject
            } else {
                PermissionFocus::AllowOnce
            },
            initial_focus_pending: true,
            resolved: false,
            _timeout_task: timeout_task,
        }
    }

    /// Fixed-height rows, including UTF-8-safe wrapped command rows.
    pub fn row_count(&self) -> usize {
        1 + self.prompt.command_rows.len() + if self.prompt.is_danger() { 2 } else { 0 } + 2
    }

    /// Exact tool/target pair used for durable-card matching.
    pub fn identity(&self) -> (&str, &str) {
        (&self.prompt.tool, &self.prompt.display_target)
    }

    /// Content rendered by the card, used by leak-focused tests.
    pub fn visible_text(&self) -> String {
        let mut text = format!(
            "需要权限 {} {}",
            self.prompt.tool, self.prompt.display_target
        );
        if let Some(reason) = &self.prompt.danger_reason {
            text.push(' ');
            text.push_str(reason);
            text.push_str(" 总是允许仍会在下次危险命令时再次确认");
        }
        text.push_str(" 允许一次 总是允许 拒绝");
        text
    }

    /// Whether another path already closed the first-wins latch.
    pub fn is_resolved(&self) -> bool {
        self.resolved || self.lease.as_ref().is_none_or(PermissionLease::is_resolved)
    }

    fn resolve(&mut self, decision: PermissionDecision, cx: &mut gpui::Context<Self>) {
        if self.resolved {
            return;
        }
        if let Some(lease) = self.lease.as_mut() {
            lease.respond(decision);
        }
        self.lease.take();
        self.resolved = true;
        cx.emit(PermissionCardResolved);
        cx.notify();
    }

    fn deny(&mut self, cx: &mut gpui::Context<Self>) {
        let note = self.note.read(cx).text().trim().to_string();
        self.resolve(
            PermissionDecision::Deny {
                note: (!note.is_empty()).then_some(note),
            },
            cx,
        );
    }

    fn sync_external_resolution(&mut self, cx: &mut gpui::Context<Self>) {
        if !self.resolved
            && self
                .lease
                .as_ref()
                .is_some_and(PermissionLease::is_resolved)
        {
            self.lease.take();
            self.resolved = true;
            cx.emit(PermissionCardResolved);
            cx.notify();
        }
    }

    fn focus(&mut self, focus: PermissionFocus, window: &mut Window, cx: &mut App) {
        self.logical_focus = focus;
        match focus {
            PermissionFocus::AllowOnce => self.once_focus.focus(window, cx),
            PermissionFocus::Always => self.always_focus.focus(window, cx),
            PermissionFocus::Reject => self.reject_focus.focus(window, cx),
            PermissionFocus::Note => self.note.read(cx).focus_handle(cx).focus(window, cx),
        }
    }

    fn cycle_focus(&mut self, reverse: bool, window: &mut Window, cx: &mut App) {
        if self.note.read(cx).focus_handle(cx).is_focused(window) {
            self.logical_focus = PermissionFocus::Note;
        }
        let next = match (self.prompt.is_danger(), self.logical_focus, reverse) {
            (true, PermissionFocus::AllowOnce, false) => PermissionFocus::Always,
            (true, PermissionFocus::Always, false) => PermissionFocus::Reject,
            (true, PermissionFocus::Reject | PermissionFocus::Note, false) => {
                PermissionFocus::AllowOnce
            }
            (true, PermissionFocus::AllowOnce | PermissionFocus::Note, true) => {
                PermissionFocus::Reject
            }
            (true, PermissionFocus::Always, true) => PermissionFocus::AllowOnce,
            (true, PermissionFocus::Reject, true) => PermissionFocus::Always,
            (false, PermissionFocus::AllowOnce, false) => PermissionFocus::Always,
            (false, PermissionFocus::Always, false) => PermissionFocus::Reject,
            (false, PermissionFocus::Reject, false) => PermissionFocus::Note,
            (false, PermissionFocus::Note, false) => PermissionFocus::AllowOnce,
            (false, PermissionFocus::AllowOnce, true) => PermissionFocus::Note,
            (false, PermissionFocus::Always, true) => PermissionFocus::AllowOnce,
            (false, PermissionFocus::Reject, true) => PermissionFocus::Always,
            (false, PermissionFocus::Note, true) => PermissionFocus::Reject,
        };
        self.focus(next, window, cx);
    }

    fn activate_focused(&mut self, window: &Window, cx: &mut gpui::Context<Self>) -> bool {
        if self.once_focus.is_focused(window) {
            self.resolve(PermissionDecision::Once, cx);
            true
        } else if self.always_focus.is_focused(window) {
            self.resolve(PermissionDecision::Always, cx);
            true
        } else if self.reject_focus.is_focused(window) {
            self.deny(cx);
            true
        } else {
            false
        }
    }

    /// Renders one row inside the conversation uniform list.
    pub(crate) fn render_row(
        card: Entity<Self>,
        row: usize,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        card.update(cx, |card, cx| card.sync_external_resolution(cx));
        let colors = theme(cx).colors;
        let is_danger = card.read(cx).prompt.is_danger();
        let button_row = card.read(cx).row_count() - 1;
        if row == button_row && card.read(cx).initial_focus_pending {
            card.update(cx, |card, cx| {
                card.initial_focus_pending = false;
                let focus = card.logical_focus;
                card.focus(focus, window, cx);
            });
        }

        let action_card = card.clone();
        let mut container = div()
            .h(px(ROW_HEIGHT))
            .w_full()
            .flex()
            .items_center()
            .px_2()
            .border_l_3()
            .border_color(colors.warning)
            .bg(colors.bg_elevated)
            .key_context("PermissionCard")
            .on_action(move |_: &PermissionEnter, _, cx| {
                action_card.update(cx, |card, cx| {
                    if card.prompt.is_danger() {
                        card.deny(cx);
                    } else {
                        card.resolve(PermissionDecision::Once, cx);
                    }
                });
            });
        let action_card = card.clone();
        container = container.on_action(move |_: &PermissionAlways, _, cx| {
            action_card.update(cx, |card, cx| card.resolve(PermissionDecision::Always, cx));
        });
        let action_card = card.clone();
        container = container.on_action(move |_: &PermissionDeny, _, cx| {
            action_card.update(cx, PermissionCard::deny);
        });
        let action_card = card.clone();
        container = container.on_action(move |_: &PermissionNextFocus, window, cx| {
            action_card.update(cx, |card, cx| card.cycle_focus(false, window, cx));
        });
        let action_card = card.clone();
        container = container.on_action(move |_: &PermissionPreviousFocus, window, cx| {
            action_card.update(cx, |card, cx| card.cycle_focus(true, window, cx));
        });
        let action_card = card.clone();
        container = container.on_action(move |_: &PermissionActivate, window, cx| {
            let activated = action_card.update(cx, |card, cx| card.activate_focused(window, cx));
            if !activated {
                cx.propagate();
            }
        });

        let card_ref = card.read(cx);
        if row == 0 {
            return container
                .rounded_tl_lg()
                .rounded_tr_lg()
                .text_size(px(Typography::HEADING_CARD))
                .font_weight(Typography::HEADING_CARD_WEIGHT)
                .text_color(if is_danger {
                    colors.danger
                } else {
                    colors.warning
                })
                .child(if is_danger {
                    format!("危险命令需要确认 · {}", card_ref.prompt.tool)
                } else {
                    format!("需要权限 · {}", card_ref.prompt.tool)
                })
                .into_any_element();
        }
        let command_end = 1 + card_ref.prompt.command_rows.len();
        if (1..command_end).contains(&row) {
            return container
                .bg(colors.code_bg)
                .font_family(MONOFONT)
                .text_size(px(Typography::CODE))
                .text_color(colors.text_primary)
                .child(card_ref.prompt.command_rows[row - 1].clone())
                .into_any_element();
        }
        if is_danger && row == command_end {
            return container
                .text_size(px(Typography::SIDEBAR))
                .text_color(colors.danger)
                .child(
                    card_ref
                        .prompt
                        .danger_reason
                        .clone()
                        .unwrap_or_else(|| "危险命令".to_string()),
                )
                .into_any_element();
        }
        if is_danger && row == command_end + 1 {
            return container
                .text_size(px(Typography::SIDEBAR))
                .text_color(colors.warning)
                .child("总是允许仍会在下次危险命令时再次确认")
                .into_any_element();
        }
        let note_row = command_end + if is_danger { 2 } else { 0 };
        if row == note_row {
            return container
                .child(
                    div()
                        .flex_1()
                        .border_1()
                        .border_color(colors.border_subtle)
                        .child(card_ref.note.clone()),
                )
                .into_any_element();
        }
        let once = card.read(cx).once_focus.clone();
        let always = card.read(cx).always_focus.clone();
        let reject = card.read(cx).reject_focus.clone();
        let once_card = card.clone();
        let always_card = card.clone();
        let reject_card = card.clone();
        container
            .rounded_bl_lg()
            .rounded_br_lg()
            .justify_end()
            .gap_2()
            .child(permission_button(
                "允许一次",
                once,
                true,
                colors,
                window,
                move |_: &MouseUpEvent, _, cx| {
                    once_card.update(cx, |card, cx| card.resolve(PermissionDecision::Once, cx));
                },
            ))
            .child(permission_button(
                "总是允许",
                always,
                false,
                colors,
                window,
                move |_: &MouseUpEvent, _, cx| {
                    always_card.update(cx, |card, cx| card.resolve(PermissionDecision::Always, cx));
                },
            ))
            .child(permission_button(
                "拒绝",
                reject,
                false,
                colors,
                window,
                move |_: &MouseUpEvent, _, cx| {
                    reject_card.update(cx, PermissionCard::deny);
                },
            ))
            .into_any_element()
    }
}

impl EventEmitter<PermissionCardResolved> for PermissionCard {}

fn wrap_command_rows(command: &str) -> Vec<String> {
    const MAX_CHARS: usize = 80;
    let mut rows = Vec::new();
    for physical in command.split('\n') {
        if physical.is_empty() {
            rows.push(String::new());
            continue;
        }
        let mut row = String::new();
        let mut width = 0usize;
        for character in physical.chars() {
            let character_width = display_width(&character.to_string());
            if !row.is_empty() && width + character_width > MAX_CHARS {
                rows.push(std::mem::take(&mut row));
                width = 0;
            }
            row.push(character);
            width += character_width;
            if width == MAX_CHARS {
                rows.push(std::mem::take(&mut row));
                width = 0;
            }
        }
        if !row.is_empty() {
            rows.push(row);
        }
    }
    if rows.is_empty() {
        rows.push(String::new());
    }
    rows
}

impl fmt::Debug for PermissionCard {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PermissionCard")
            .field("prompt", &self.prompt)
            .field("resolved", &self.is_resolved())
            .finish()
    }
}

fn permission_button(
    label: &'static str,
    focus: FocusHandle,
    primary: bool,
    colors: vega_theme::ThemeColors,
    window: &Window,
    listener: impl Fn(&MouseUpEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let focused = focus.is_focused(window);
    let button = div()
        .px_2()
        .rounded_md()
        .border_1()
        .border_color(if focused {
            colors.warning
        } else {
            colors.border_subtle
        })
        .text_size(px(Typography::SIDEBAR))
        .text_color(if primary {
            colors.bg_base
        } else {
            colors.text_primary
        })
        .cursor_pointer()
        .hover(move |style| style.bg(colors.bg_hover))
        .track_focus(&focus)
        .on_mouse_up(MouseButton::Left, listener)
        .child(label);
    if primary {
        button.bg(colors.accent).into_any_element()
    } else {
        button.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::Pin;

    use gpui::{Render, TestAppContext, WindowHandle, div};
    use tokio_util::sync::CancellationToken;
    use vega_conversation::agent::{PermissionHook, PermissionQueue, PermissionQueueListener};
    use vega_theme::Theme;

    use super::*;

    struct Harness {
        card: Entity<PermissionCard>,
    }

    type DecisionFuture = Pin<Box<dyn Future<Output = PermissionDecision> + Send>>;

    impl Render for Harness {
        fn render(
            &mut self,
            window: &mut Window,
            cx: &mut gpui::Context<Self>,
        ) -> impl IntoElement {
            let rows = self.card.read(cx).row_count();
            div().flex().flex_col().children(
                (0..rows).map(|row| PermissionCard::render_row(self.card.clone(), row, window, cx)),
            )
        }
    }

    fn init_test(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.set_global(Theme::light());
            crate::init(cx);
        });
    }

    fn open_card(
        cx: &mut TestAppContext,
        danger: bool,
        target: &str,
    ) -> (
        WindowHandle<Harness>,
        Entity<PermissionCard>,
        DecisionFuture,
        PermissionQueueListener,
    ) {
        let queue = PermissionQueue::new();
        let listener = queue.subscribe();
        let request = PermissionRequest {
            call_id: "SECRET_CALL_ID".into(),
            tool: "bash".into(),
            display_target: target.into(),
            danger_rule_id: danger.then(|| "danger.git_force_push".into()),
            danger_reason: danger.then(|| "强制推送可能覆盖远端历史".into()),
        };
        let future = queue.request(request, CancellationToken::new());
        let future: DecisionFuture =
            Box::pin(async move { future.await.unwrap_or(PermissionDecision::Timeout) });
        let (request, lease) = queue.take_pending().unwrap().into_parts().unwrap();
        let card = cx.new(|cx| PermissionCard::new(&request, lease, cx));
        let root_card = card.clone();
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), move |_, cx| {
                cx.new(|_| Harness { card: root_card })
            })
            .unwrap()
        });
        cx.run_until_parked();
        (window, card, future, listener)
    }

    fn focused(
        window: WindowHandle<Harness>,
        card: &Entity<PermissionCard>,
        cx: &mut TestAppContext,
    ) -> PermissionFocus {
        window
            .update(cx, |_, window, cx| {
                let card = card.read(cx);
                if card.once_focus.is_focused(window) {
                    PermissionFocus::AllowOnce
                } else if card.always_focus.is_focused(window) {
                    PermissionFocus::Always
                } else if card.reject_focus.is_focused(window) {
                    PermissionFocus::Reject
                } else {
                    PermissionFocus::Note
                }
            })
            .unwrap()
    }

    #[gpui::test]
    async fn ordinary_focus_note_and_keyboard_decisions(cx: &mut TestAppContext) {
        init_test(cx);
        for tabs in 0..4 {
            let (window, _card, future, _listener) = open_card(cx, false, "printf enter");
            for _ in 0..tabs {
                cx.simulate_keystrokes(window.into(), "tab");
            }
            cx.simulate_keystrokes(window.into(), "enter");
            assert_eq!(future.await, PermissionDecision::Once);
        }

        let expected = [
            PermissionDecision::Once,
            PermissionDecision::Always,
            PermissionDecision::Deny { note: None },
        ];
        for (tabs, expected) in expected.into_iter().enumerate() {
            let (window, _card, future, _listener) = open_card(cx, false, "printf space");
            for _ in 0..tabs {
                cx.simulate_keystrokes(window.into(), "tab");
            }
            cx.simulate_keystrokes(window.into(), "space");
            assert_eq!(future.await, expected);
        }

        let (window, card, future, _listener) = open_card(cx, false, "printf ok");
        assert_eq!(focused(window, &card, cx), PermissionFocus::AllowOnce);

        cx.simulate_keystrokes(window.into(), "tab tab tab");
        assert_eq!(focused(window, &card, cx), PermissionFocus::Note);
        cx.simulate_keystrokes(window.into(), "h i space x");
        let note = card.read_with(cx, |card, cx| card.note.read(cx).text().to_string());
        assert_eq!(note, "hi x");
        assert!(!card.read_with(cx, |card, _| card.is_resolved()));
        cx.simulate_keystrokes(window.into(), "escape");
        assert_eq!(
            future.await,
            PermissionDecision::Deny {
                note: Some("hi x".into())
            }
        );
    }

    #[gpui::test]
    async fn ordinary_tab_wrap_cmd_enter_and_escape_matrix(cx: &mut TestAppContext) {
        init_test(cx);
        let (window, card, future, _listener) = open_card(cx, false, "printf cycle");
        let forward = [
            PermissionFocus::Always,
            PermissionFocus::Reject,
            PermissionFocus::Note,
            PermissionFocus::AllowOnce,
        ];
        for expected in forward {
            cx.simulate_keystrokes(window.into(), "tab");
            assert_eq!(focused(window, &card, cx), expected);
        }
        cx.simulate_keystrokes(window.into(), "shift-tab");
        assert_eq!(focused(window, &card, cx), PermissionFocus::Note);
        cx.simulate_keystrokes(window.into(), "cmd-enter");
        assert_eq!(future.await, PermissionDecision::Always);

        for tabs in 0..4 {
            let (window, _card, future, _listener) = open_card(cx, false, "printf escape");
            for _ in 0..tabs {
                cx.simulate_keystrokes(window.into(), "tab");
            }
            cx.simulate_keystrokes(window.into(), "escape");
            assert_eq!(future.await, PermissionDecision::Deny { note: None });
        }

        for tabs in 0..4 {
            let (window, _card, future, _listener) = open_card(cx, false, "printf always");
            for _ in 0..tabs {
                cx.simulate_keystrokes(window.into(), "tab");
            }
            cx.simulate_keystrokes(window.into(), "cmd-enter");
            assert_eq!(future.await, PermissionDecision::Always);
        }
    }

    #[gpui::test]
    async fn danger_focus_ring_enter_and_space_matrix(cx: &mut TestAppContext) {
        init_test(cx);
        for tabs in 0..3 {
            let (window, _card, future, _listener) = open_card(cx, true, "git push --force");
            for _ in 0..tabs {
                cx.simulate_keystrokes(window.into(), "tab");
            }
            cx.simulate_keystrokes(window.into(), "enter");
            assert_eq!(future.await, PermissionDecision::Deny { note: None });
        }

        let expected = [
            PermissionDecision::Deny { note: None },
            PermissionDecision::Once,
            PermissionDecision::Always,
        ];
        for (tabs, expected) in expected.into_iter().enumerate() {
            let (window, _card, future, _listener) = open_card(cx, true, "git push --force");
            for _ in 0..tabs {
                cx.simulate_keystrokes(window.into(), "tab");
            }
            cx.simulate_keystrokes(window.into(), "space");
            assert_eq!(future.await, expected);
        }

        let (window, card, future, _listener) = open_card(cx, true, "git push --force");
        cx.simulate_keystrokes(window.into(), "shift-tab");
        assert_eq!(focused(window, &card, cx), PermissionFocus::Always);
        cx.simulate_keystrokes(window.into(), "shift-tab");
        assert_eq!(focused(window, &card, cx), PermissionFocus::AllowOnce);
        cx.simulate_keystrokes(window.into(), "shift-tab");
        assert_eq!(focused(window, &card, cx), PermissionFocus::Reject);
        cx.simulate_keystrokes(window.into(), "cmd-enter");
        assert_eq!(future.await, PermissionDecision::Always);

        let (window, card, future, _listener) = open_card(cx, true, "git push --force");
        let forward = [
            PermissionFocus::AllowOnce,
            PermissionFocus::Always,
            PermissionFocus::Reject,
        ];
        for expected in forward {
            cx.simulate_keystrokes(window.into(), "tab");
            assert_eq!(focused(window, &card, cx), expected);
        }
        cx.simulate_keystrokes(window.into(), "escape");
        assert_eq!(future.await, PermissionDecision::Deny { note: None });

        for tabs in 0..3 {
            let (window, _card, future, _listener) = open_card(cx, true, "git push --force");
            for _ in 0..tabs {
                cx.simulate_keystrokes(window.into(), "tab");
            }
            cx.simulate_keystrokes(window.into(), "cmd-enter");
            assert_eq!(future.await, PermissionDecision::Always);
        }

        for tabs in 0..3 {
            let (window, _card, future, _listener) = open_card(cx, true, "git push --force");
            for _ in 0..tabs {
                cx.simulate_keystrokes(window.into(), "tab");
            }
            cx.simulate_keystrokes(window.into(), "escape");
            assert_eq!(future.await, PermissionDecision::Deny { note: None });
        }
    }

    #[gpui::test]
    async fn timeout_duplicate_and_window_release_are_fail_closed(cx: &mut TestAppContext) {
        init_test(cx);
        let (window, card, future, _listener) = open_card(cx, false, "printf ok");
        cx.simulate_keystrokes(window.into(), "enter cmd-enter");
        assert_eq!(future.await, PermissionDecision::Once);
        assert!(card.read_with(cx, |card, _| card.is_resolved()));

        let (_window, card, future, _listener) = open_card(cx, false, "printf timeout");
        cx.executor().advance_clock(PERMISSION_TIMEOUT);
        cx.run_until_parked();
        assert_eq!(future.await, PermissionDecision::Timeout);
        assert!(card.read_with(cx, |card, _| card.is_resolved()));

        let (window, card, future, _listener) = open_card(cx, false, "printf close");
        drop(card);
        window
            .update(cx, |_, window, _| window.remove_window())
            .unwrap();
        cx.run_until_parked();
        assert_eq!(future.await, PermissionDecision::Timeout);
    }

    #[gpui::test]
    fn danger_copy_long_utf8_command_and_debug_are_complete_and_redacted(cx: &mut TestAppContext) {
        init_test(cx);
        let command = format!("printf '{}{}'", "中".repeat(70), "a".repeat(90));
        let (_window, card, _future, _listener) = open_card(cx, true, &command);
        let (visible, debug, rows) = card.read_with(cx, |card, _| {
            (
                card.visible_text(),
                format!("{card:?}"),
                card.prompt.command_rows.clone(),
            )
        });
        assert!(visible.contains(&command));
        assert!(visible.contains("总是允许仍会在下次危险命令时再次确认"));
        assert!(visible.contains("bash"));
        assert!(!debug.contains("SECRET_CALL_ID"));
        assert!(!debug.contains(&command));
        assert!(rows.iter().all(|row| display_width(row) <= 80));
        assert_eq!(rows.concat(), command);
        assert_eq!(
            card.read_with(cx, |card, _| card.row_count()),
            rows.len() + 5
        );
    }
}
