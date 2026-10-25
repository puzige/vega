//! Fail-closed GPUI permission card over the conversation-layer handoff.

use crate::interaction::InteractionExt;
use std::fmt;

use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Entity, EventEmitter, FocusHandle, Focusable, MouseButton, MouseUpEvent, Task,
    Window, actions, div, px,
};
use vega_conversation::agent::{PERMISSION_TIMEOUT, PermissionLease};
use vega_conversation::types::{PermissionDecision, PermissionOptionChoice, PermissionRequest};
use vega_theme::{Layout, Typography, theme};

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
    Acp(usize),
}

/// Safe presentation copied from a transient request after call-id matching.
#[derive(Clone, PartialEq, Eq)]
struct SafePermissionPrompt {
    tool: String,
    display_target: String,
    command_rows: Vec<String>,
    codex_identity: Option<vega_conversation::types::CodexAcpActivityIdentity>,
    danger_rule_id: Option<String>,
    danger_reason: Option<String>,
    allow_persistent: bool,
    acp_options: Option<Vec<PermissionOptionChoice>>,
}

impl SafePermissionPrompt {
    fn from_request(
        request: &PermissionRequest,
        allow_persistent: bool,
        codex_identity: Option<vega_conversation::types::CodexAcpActivityIdentity>,
    ) -> Self {
        Self {
            tool: request.tool.clone(),
            display_target: request.display_target.clone(),
            command_rows: wrap_command_rows(&request.display_target),
            codex_identity: codex_identity.filter(|identity| {
                request.tool == "codex_acp"
                    && identity.is_valid()
                    && identity.permission_target() == request.display_target
            }),
            danger_rule_id: request.danger_rule_id.clone(),
            danger_reason: request.danger_reason.clone(),
            allow_persistent: allow_persistent && request.external.is_none(),
            acp_options: request.acp_options.clone(),
        }
    }

    fn command_rows(&self) -> Vec<String> {
        self.codex_identity
            .as_ref()
            .map(|identity| wrap_command_rows(&crate::tool_card::codex_identity_display(identity)))
            .unwrap_or_else(|| self.command_rows.clone())
    }

    fn is_danger(&self) -> bool {
        self.danger_rule_id.is_some()
    }

    fn danger_label(&self) -> &str {
        let internal_danger = self.tool == "bash" && self.acp_options.is_none();
        let key = internal_danger
            .then_some(self.danger_rule_id.as_deref())
            .flatten()
            .and_then(|rule| match rule {
                "rm-root-recursive-force" => Some("ui.danger.root_recursive_delete"),
                "git-force-push" => Some("ui.danger.force_push"),
                "raw-device-write" => Some("ui.danger.raw_device_write"),
                "filesystem-format" => Some("ui.danger.filesystem_format"),
                "diskutil-destructive" => Some("ui.danger.diskutil_destructive"),
                _ => None,
            });
        key.map(vega_i18n::t).unwrap_or_else(|| {
            self.danger_reason
                .as_deref()
                .unwrap_or_else(|| vega_i18n::t("ui.dangerous_command"))
        })
    }

    fn display_tool(&self) -> &str {
        if self.tool == "codex_acp" {
            "Codex ACP"
        } else {
            &self.tool
        }
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
    acp_option_focuses: Vec<FocusHandle>,
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
        allow_persistent: bool,
        codex_identity: Option<vega_conversation::types::CodexAcpActivityIdentity>,
        cx: &mut gpui_kit::Context<Self>,
    ) -> Self {
        let note = cx.new(|cx| {
            TextInput::new(cx, "", false).with_placeholder_key("ui.reason_for_rejection_optional")
        });
        cx.observe(&note, |_, _, cx| cx.notify()).detach();
        let danger = request.danger_rule_id.is_some();
        let acp_option_focuses = request
            .acp_options
            .as_ref()
            .map(|options| {
                (0..options.len())
                    .map(|index| {
                        cx.focus_handle()
                            .tab_index(4 + index as isize)
                            .tab_stop(true)
                    })
                    .collect()
            })
            .unwrap_or_default();
        let timeout_task = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(PERMISSION_TIMEOUT).await;
            let _ = this.update(cx, |this, cx| {
                this.resolve(PermissionDecision::Timeout, cx);
            });
        });
        Self {
            prompt: SafePermissionPrompt::from_request(request, allow_persistent, codex_identity),
            lease: Some(lease),
            note,
            once_focus: cx.focus_handle().tab_index(1).tab_stop(true),
            always_focus: cx.focus_handle().tab_index(2).tab_stop(true),
            reject_focus: cx.focus_handle().tab_index(3).tab_stop(true),
            acp_option_focuses,
            logical_focus: if request
                .acp_options
                .as_ref()
                .is_some_and(|options| !options.is_empty())
            {
                PermissionFocus::Acp(0)
            } else if danger {
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
        if let Some(options) = &self.prompt.acp_options {
            1 + self.prompt.command_rows().len() + options.len()
        } else {
            1 + self.prompt.command_rows().len() + if self.prompt.is_danger() { 2 } else { 0 } + 2
        }
    }

    /// Exact tool/target pair used for durable-card matching.
    pub fn identity(&self) -> (&str, &str) {
        (&self.prompt.tool, &self.prompt.display_target)
    }

    /// Whether another path already closed the first-wins latch.
    pub fn is_resolved(&self) -> bool {
        self.resolved || self.lease.as_ref().is_none_or(PermissionLease::is_resolved)
    }

    /// Resolves this card's own lease without touching the queue's current
    /// active slot, which may already belong to a newer call.
    pub(crate) fn timeout(&mut self, cx: &mut gpui_kit::Context<Self>) {
        self.resolve(PermissionDecision::Timeout, cx);
    }

    fn resolve(&mut self, decision: PermissionDecision, cx: &mut gpui_kit::Context<Self>) {
        if self.resolved
            || (!self.prompt.allow_persistent && matches!(decision, PermissionDecision::Always))
        {
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

    fn deny(&mut self, cx: &mut gpui_kit::Context<Self>) {
        if let Some(option) = self.prompt.acp_options.as_ref().and_then(|options| {
            options.iter().find(|option| {
                option
                    .kind
                    .as_deref()
                    .is_some_and(|kind| kind.starts_with("reject"))
            })
        }) {
            self.resolve(
                PermissionDecision::AcpOption {
                    option_id: option.option_id.clone(),
                },
                cx,
            );
            return;
        }
        if self.prompt.acp_options.is_some() {
            self.resolve(PermissionDecision::Timeout, cx);
            return;
        }
        let note = self.note.read(cx).text().trim().to_string();
        self.resolve(
            PermissionDecision::Deny {
                note: (!note.is_empty()).then_some(note),
            },
            cx,
        );
    }

    fn sync_external_resolution(&mut self, cx: &mut gpui_kit::Context<Self>) {
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
            PermissionFocus::Acp(index) => {
                if let Some(focus) = self.acp_option_focuses.get(index) {
                    focus.focus(window, cx);
                }
            }
        }
    }

    fn cycle_focus(&mut self, reverse: bool, window: &mut Window, cx: &mut App) {
        if let Some(options) = &self.prompt.acp_options {
            if options.is_empty() {
                return;
            }
            let current = match self.logical_focus {
                PermissionFocus::Acp(index) => index,
                _ => 0,
            };
            let next = if reverse {
                current.checked_sub(1).unwrap_or(options.len() - 1)
            } else {
                (current + 1) % options.len()
            };
            self.focus(PermissionFocus::Acp(next), window, cx);
            return;
        }
        if self.note.read(cx).focus_handle(cx).is_focused(window) {
            self.logical_focus = PermissionFocus::Note;
        }
        if !self.prompt.allow_persistent {
            let next = match (self.logical_focus, reverse) {
                (PermissionFocus::AllowOnce, false) => PermissionFocus::Reject,
                (PermissionFocus::Reject, false) => PermissionFocus::Note,
                (PermissionFocus::Note | PermissionFocus::Always, false) => {
                    PermissionFocus::AllowOnce
                }
                (PermissionFocus::Acp(_), false) => PermissionFocus::AllowOnce,
                (PermissionFocus::AllowOnce, true) => PermissionFocus::Note,
                (PermissionFocus::Reject, true) => PermissionFocus::AllowOnce,
                (PermissionFocus::Note | PermissionFocus::Always, true) => PermissionFocus::Reject,
                (PermissionFocus::Acp(_), true) => PermissionFocus::Reject,
            };
            self.focus(next, window, cx);
            return;
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
            (true, PermissionFocus::Acp(_), _) => PermissionFocus::AllowOnce,
            (false, PermissionFocus::AllowOnce, false) => PermissionFocus::Always,
            (false, PermissionFocus::Always, false) => PermissionFocus::Reject,
            (false, PermissionFocus::Reject, false) => PermissionFocus::Note,
            (false, PermissionFocus::Note, false) => PermissionFocus::AllowOnce,
            (false, PermissionFocus::AllowOnce, true) => PermissionFocus::Note,
            (false, PermissionFocus::Always, true) => PermissionFocus::AllowOnce,
            (false, PermissionFocus::Reject, true) => PermissionFocus::Always,
            (false, PermissionFocus::Note, true) => PermissionFocus::Reject,
            (false, PermissionFocus::Acp(_), _) => PermissionFocus::AllowOnce,
        };
        self.focus(next, window, cx);
    }

    fn activate_focused(&mut self, window: &Window, cx: &mut gpui_kit::Context<Self>) -> bool {
        if let PermissionFocus::Acp(index) = self.logical_focus
            && let Some(option) = self
                .prompt
                .acp_options
                .as_ref()
                .and_then(|options| options.get(index))
        {
            self.resolve(
                PermissionDecision::AcpOption {
                    option_id: option.option_id.clone(),
                },
                cx,
            );
            return true;
        }
        if self.once_focus.is_focused(window) {
            self.resolve(PermissionDecision::Once, cx);
            true
        } else if self.always_focus.is_focused(window) {
            if self.prompt.allow_persistent {
                self.resolve(PermissionDecision::Always, cx);
            }
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
            .min_h(px(ROW_HEIGHT))
            .flex_shrink_0()
            .w_full()
            .flex()
            .items_center()
            .line_height(gpui_kit::relative(Typography::BODY_LINE_HEIGHT))
            .gap_2()
            .key_context("PermissionCard")
            .on_action(move |_: &PermissionEnter, window, cx| {
                action_card.update(cx, |card, cx| {
                    if card.prompt.acp_options.is_some() {
                        card.activate_focused(window, cx);
                    } else if card.prompt.is_danger() {
                        card.deny(cx);
                    } else {
                        card.resolve(PermissionDecision::Once, cx);
                    }
                });
            });
        let action_card = card.clone();
        container = container.on_action(move |_: &PermissionAlways, _, cx| {
            action_card.update(cx, |card, cx| {
                if card.prompt.allow_persistent {
                    card.resolve(PermissionDecision::Always, cx);
                }
            });
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
            let color = if is_danger {
                colors.danger
            } else {
                colors.warning
            };
            let label = if is_danger {
                vega_i18n::t_args(
                    "ui.dangerous_command_requires_confirmation_tool",
                    &[("tool", card_ref.prompt.display_tool())],
                )
            } else {
                vega_i18n::t_args(
                    "ui.permission_required_tool",
                    &[("tool", card_ref.prompt.display_tool())],
                )
            };
            return container
                .items_start()
                .text_size(px(Typography::HEADING_CARD))
                .font_weight(Typography::HEADING_CARD_WEIGHT)
                .text_color(color)
                .child(
                    div()
                        .mt(px((Typography::BODY * Typography::BODY_LINE_HEIGHT
                            - Layout::ICON_SIZE)
                            / 2.0))
                        .size(px(Layout::ICON_SIZE))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(crate::icons::icon(crate::icons::Icon::Warning, color)),
                )
                .child(div().min_w_0().flex_1().child(label))
                .into_any_element();
        }
        let command_rows = card_ref.prompt.command_rows();
        let command_end = 1 + command_rows.len();
        if (1..command_end).contains(&row) {
            return container
                .bg(colors.code_bg)
                .font_family(MONOFONT)
                .text_size(px(Typography::CODE))
                .text_color(colors.text_primary)
                .child(command_rows[row - 1].clone())
                .into_any_element();
        }
        if is_danger && row == command_end {
            return container
                .text_size(px(Typography::SIDEBAR))
                .text_color(colors.danger)
                .child(card_ref.prompt.danger_label().to_owned())
                .into_any_element();
        }
        if is_danger && row == command_end + 1 {
            return container
                .text_size(px(Typography::SIDEBAR))
                .text_color(colors.warning)
                .child(vega_i18n::t(
                    "ui.always_allow_still_requires_confirmation_for_future_dangerous",
                ))
                .into_any_element();
        }
        if let Some(options) = &card_ref.prompt.acp_options {
            let option_index = row.saturating_sub(command_end);
            if let Some(option) = options.get(option_index) {
                let option = option.clone();
                let focus = card.read(cx).acp_option_focuses[option_index].clone();
                let option_card = card.clone();
                return container
                    .justify_end()
                    .child(permission_button(
                        format!("permission-option-{}-{option_index}", card.entity_id()),
                        option.name.clone(),
                        focus,
                        option_index == 0,
                        colors,
                        window,
                        move |_: &MouseUpEvent, _, cx| {
                            option_card.update(cx, |card, cx| {
                                card.resolve(
                                    PermissionDecision::AcpOption {
                                        option_id: option.option_id.clone(),
                                    },
                                    cx,
                                );
                            });
                        },
                    ))
                    .into_any_element();
            }
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
        let mut container = container.justify_end().gap_2().child(permission_button(
            format!("permission-once-{}", card.entity_id()),
            vega_i18n::t("ui.allow_once").to_string(),
            once,
            true,
            colors,
            window,
            move |_: &MouseUpEvent, _, cx| {
                once_card.update(cx, |card, cx| card.resolve(PermissionDecision::Once, cx));
            },
        ));
        if card.read(cx).prompt.allow_persistent {
            container = container.child(permission_button(
                format!("permission-always-{}", card.entity_id()),
                vega_i18n::t("ui.always_allow").to_string(),
                always,
                false,
                colors,
                window,
                move |_: &MouseUpEvent, _, cx| {
                    always_card.update(cx, |card, cx| card.resolve(PermissionDecision::Always, cx));
                },
            ));
        }
        container
            .child(permission_button(
                format!("permission-deny-{}", card.entity_id()),
                vega_i18n::t("ui.reject").to_string(),
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
    id: String,
    label: String,
    focus: FocusHandle,
    primary: bool,
    colors: vega_theme::ThemeColors,
    window: &Window,
    listener: impl Fn(&MouseUpEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let focused = focus.is_focused(window);
    let button = div()
        .id(gpui_kit::SharedString::from(id))
        .min_h(px(Layout::TITLEBAR_CONTROL_SIZE))
        .flex()
        .items_center()
        .justify_center()
        .px(px(Layout::DISCLOSURE_PADDING_X))
        .rounded(px(Layout::DISCLOSURE_RADIUS))
        .border_1()
        .border_color(if focused {
            colors.warning
        } else {
            colors.border_subtle
        })
        .text_size(px(Typography::SIDEBAR))
        .text_color(if primary {
            colors.brand_on_accent
        } else {
            colors.text_primary
        })
        .cursor_pointer()
        .track_focus(&focus)
        .on_mouse_up(MouseButton::Left, listener)
        .child(label);
    let button = if primary {
        button.interaction_surface(
            colors.accent,
            colors.brand_primary_strong,
            colors.accent,
            true,
        )
    } else {
        button.interactive(colors, true, false)
    };
    button.into_any_element()
}
