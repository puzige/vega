//! Audited tool-call activities over strict `vega_conversation` projections.

use std::time::{Duration, Instant};

use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, Entity, MouseButton, MouseUpEvent, ScrollHandle, Task, div, px};
use vega_conversation::types::{
    Approval, InvalidToolKind, ReadOnlyToolKind, SkillCardOutcome, ToolCall, ToolCallStatus,
    ToolCardInputProjection, ToolCardResultProjection, ToolResult, tool_card_input_projection,
    tool_card_result_projection,
};
use vega_theme::{Layout, ThemeColors, Typography, theme};

use crate::conversation_stream::{MONOFONT, ROW_HEIGHT};
use crate::icons::Icon;

const CORRUPT_LABEL: &str = "Tool result corrupted";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ToolActivityCategory {
    Shell,
    Read,
    Find,
    Search,
    Write,
    Edit,
    Mcp,
    CodexAcp,
    ComputerUse,
    Skill,
    Other,
}

impl ToolActivityCategory {
    pub(crate) const fn action_phrase(self) -> &'static str {
        match self {
            Self::Shell => "run commands",
            Self::Read => "read files",
            Self::Find => "find files",
            Self::Search => "search content",
            Self::Write => "write files",
            Self::Edit => "edit files",
            Self::Mcp => "call MCP",
            Self::CodexAcp => "Codex ACP",
            Self::ComputerUse => "use computer or browser",
            Self::Skill => "use Skill",
            Self::Other => "use tools",
        }
    }

    pub(crate) const fn icon(self) -> Icon {
        match self {
            Self::Shell => Icon::Terminal,
            Self::Search => Icon::Search,
            Self::Read => Icon::BookOpen,
            Self::Find => Icon::Folder,
            Self::Write => Icon::Document,
            Self::Edit => Icon::Pencil,
            Self::Mcp => Icon::Plug,
            Self::CodexAcp => Icon::Bot,
            Self::ComputerUse => Icon::Monitor,
            Self::Skill | Self::Other => Icon::Wrench,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ToolActivityState {
    Superseded,
    Success,
    Active,
    Cancelled,
    Rejected,
    Failed,
}

impl ToolActivityState {
    pub(crate) const fn priority(self) -> u8 {
        match self {
            Self::Superseded => 0,
            Self::Success => 1,
            Self::Active => 2,
            Self::Cancelled => 3,
            Self::Rejected => 4,
            Self::Failed => 5,
        }
    }

    pub(crate) const fn terminal_icon(self) -> Option<Icon> {
        match self {
            Self::Success => Some(Icon::Check),
            Self::Failed => Some(Icon::Close),
            _ => None,
        }
    }

    pub(crate) fn terminal_color(self, colors: &ThemeColors) -> gpui_kit::Rgba {
        match self {
            Self::Success => colors.success,
            Self::Cancelled | Self::Rejected | Self::Failed => colors.danger,
            Self::Active | Self::Superseded => colors.text_secondary,
        }
    }
}

#[derive(Clone)]
struct ToolDetail {
    title: Option<String>,
    command: Option<String>,
    output: Vec<String>,
    footer: Option<(String, ToolActivityState)>,
}

impl ToolDetail {
    fn logical_rows(&self) -> usize {
        usize::from(self.title.is_some())
            + usize::from(self.command.is_some())
            + self.output.len()
            + usize::from(self.footer.is_some())
    }

    fn append_visible_text(&self, text: &mut String) {
        for row in self
            .title
            .iter()
            .chain(self.command.iter())
            .chain(self.output.iter())
            .chain(self.footer.iter().map(|(text, _)| text))
        {
            text.push('\n');
            text.push_str(row);
        }
    }
}

/// UI-only audited tool call. It never retains a provider call id, raw
/// write/edit input, fingerprint, checkpoint reference, or checkpoint path.
pub struct ToolCard {
    input: Option<ToolCardInputProjection>,
    status: ToolCallStatus,
    approval: Option<Approval>,
    result: Option<ToolCardResultProjection>,
    summary: String,
    output_rows: Vec<String>,
    expanded: bool,
    detail_scroll: ScrollHandle,
    running_started_at: Option<Instant>,
    running_elapsed_seconds: Option<u64>,
    elapsed_refresh_task: Option<Task<()>>,
}

impl ToolCard {
    /// Creates a pending card from a durable safe proposal.
    pub fn proposed(call: &ToolCall) -> Self {
        let input = tool_card_input_projection(call);
        let corrupt = matches!(input, ToolCardInputProjection::Corrupt);
        let mut card = Self {
            input: Some(input),
            status: if corrupt {
                ToolCallStatus::Failed
            } else {
                ToolCallStatus::PendingApproval
            },
            approval: None,
            result: corrupt.then_some(ToolCardResultProjection::Corrupt),
            summary: String::new(),
            output_rows: Vec::new(),
            expanded: false,
            detail_scroll: ScrollHandle::new(),
            running_started_at: None,
            running_elapsed_seconds: None,
            elapsed_refresh_task: None,
        };
        card.refresh_summary();
        card
    }

    /// Creates a validated proposal-free invalid-input card.
    pub fn invalid_terminal(result: &ToolResult) -> Self {
        let projection = tool_card_result_projection(None, result);
        let status = match projection {
            ToolCardResultProjection::InvalidRejected { .. } => ToolCallStatus::Rejected,
            _ => ToolCallStatus::Failed,
        };
        let mut card = Self {
            input: None,
            status,
            approval: None,
            result: Some(projection),
            summary: String::new(),
            output_rows: Vec::new(),
            expanded: false,
            detail_scroll: ScrollHandle::new(),
            running_started_at: None,
            running_elapsed_seconds: None,
            elapsed_refresh_task: None,
        };
        card.refresh_summary();
        card
    }

    /// Fixed content-free corrupt card for an unknown or illegal transition.
    pub fn corrupt() -> Self {
        let mut card = Self {
            input: None,
            status: ToolCallStatus::Failed,
            approval: None,
            result: Some(ToolCardResultProjection::Corrupt),
            summary: String::new(),
            output_rows: Vec::new(),
            expanded: false,
            detail_scroll: ScrollHandle::new(),
            running_started_at: None,
            running_elapsed_seconds: None,
            elapsed_refresh_task: None,
        };
        card.refresh_summary();
        card
    }

    /// Builds the durable hydrated card. Expansion is deliberately reset.
    pub fn hydrated(
        input: Option<ToolCardInputProjection>,
        status: ToolCallStatus,
        approval: Option<Approval>,
        result: Option<ToolCardResultProjection>,
    ) -> Self {
        let mut card = Self {
            output_rows: result
                .as_ref()
                .map(projection_output_rows)
                .unwrap_or_default(),
            input,
            status,
            approval,
            result,
            summary: String::new(),
            expanded: false,
            detail_scroll: ScrollHandle::new(),
            running_started_at: None,
            running_elapsed_seconds: None,
            elapsed_refresh_task: None,
        };
        card.refresh_summary();
        card
    }

    /// Whether a duplicate proposal is semantically identical.
    pub fn matches_call(&self, call: &ToolCall) -> bool {
        self.status == ToolCallStatus::PendingApproval
            && self.result.is_none()
            && self
                .input
                .as_ref()
                .is_some_and(|input| input == &tool_card_input_projection(call))
    }

    /// Converts an illegal transition to the fixed corrupt state.
    pub fn fail_corrupt(&mut self, cx: &mut gpui_kit::Context<Self>) {
        self.set_corrupt();
        cx.notify();
    }

    fn set_corrupt(&mut self) {
        self.stop_live_elapsed();
        self.input = None;
        self.status = ToolCallStatus::Failed;
        self.approval = None;
        self.result = Some(ToolCardResultProjection::Corrupt);
        self.output_rows.clear();
        self.expanded = false;
        self.refresh_summary();
    }

    /// Marks the post-commit approval visible without starting elapsed time.
    pub fn apply_approved(&mut self, approval: Approval) -> bool {
        if let Some(existing) = self.approval {
            if existing == approval
                && self.status == ToolCallStatus::Approved
                && self.result.is_none()
            {
                return true;
            }
            self.set_corrupt();
            return false;
        }
        if self.result.is_some()
            || self.status != ToolCallStatus::PendingApproval
            || approval == Approval::Deny
        {
            self.set_corrupt();
            return false;
        }
        self.approval = Some(approval);
        self.status = ToolCallStatus::Approved;
        self.refresh_summary();
        true
    }

    /// Applies the content-free runtime execution boundary. Only Bash owns a
    /// live elapsed clock; other tools still advance to the truthful Running
    /// lifecycle state without adding time copy.
    pub fn apply_running(&mut self, cx: &mut gpui_kit::Context<Self>) -> bool {
        if self.result.is_some()
            || self.approval.is_none()
            || !matches!(
                self.status,
                ToolCallStatus::Approved | ToolCallStatus::Running
            )
        {
            self.set_corrupt();
            cx.notify();
            return false;
        }
        self.status = ToolCallStatus::Running;
        if self.is_bash() && self.running_started_at.is_none() {
            let started_at = cx.background_executor().now();
            self.running_started_at = Some(started_at);
            self.running_elapsed_seconds = Some(0);
            let task = cx.spawn(async move |this, cx| {
                loop {
                    let elapsed = cx
                        .background_executor()
                        .now()
                        .saturating_duration_since(started_at);
                    let next_boundary = Duration::from_secs(elapsed.as_secs().saturating_add(1));
                    cx.background_executor()
                        .timer(next_boundary.saturating_sub(elapsed))
                        .await;
                    let keep_refreshing = this
                        .update(cx, |card, cx| {
                            let Some(started_at) = card.running_started_at else {
                                return false;
                            };
                            if card.status != ToolCallStatus::Running || !card.is_bash() {
                                return false;
                            }
                            let elapsed = cx
                                .background_executor()
                                .now()
                                .saturating_duration_since(started_at)
                                .as_secs();
                            if card.running_elapsed_seconds != Some(elapsed) {
                                card.running_elapsed_seconds = Some(elapsed);
                                card.refresh_summary();
                                cx.notify();
                            }
                            true
                        })
                        .unwrap_or(false);
                    if !keep_refreshing {
                        break;
                    }
                }
            });
            self.elapsed_refresh_task = Some(task);
        }
        self.refresh_summary();
        cx.notify();
        true
    }

    /// Applies one terminal result with strict projection validation.
    pub fn apply_finished(&mut self, result: &ToolResult) -> bool {
        let projection = tool_card_result_projection(self.input.as_ref(), result);
        if let Some(existing) = &self.result {
            if existing == &projection && self.status == result.status {
                return true;
            }
            self.set_corrupt();
            return false;
        }
        let transition_valid = match result.status {
            ToolCallStatus::Rejected => self.status == ToolCallStatus::PendingApproval,
            ToolCallStatus::Success | ToolCallStatus::Failed | ToolCallStatus::Cancelled => {
                self.status == ToolCallStatus::Approved
                    || self.status == ToolCallStatus::Running
                    || (result.reused && self.status == ToolCallStatus::PendingApproval)
            }
            ToolCallStatus::PendingApproval
            | ToolCallStatus::Approved
            | ToolCallStatus::Running => false,
        };
        if !transition_valid || matches!(projection, ToolCardResultProjection::Corrupt) {
            self.set_corrupt();
            return false;
        }
        self.stop_live_elapsed();
        self.status = result.status;
        self.output_rows = projection_output_rows(&projection);
        self.result = Some(projection);
        self.refresh_summary();
        true
    }

    /// Exact mutating permission target associated with the safe proposal.
    pub fn permission_identity(&self) -> Option<(&str, &str)> {
        if self.status != ToolCallStatus::PendingApproval
            || self.approval.is_some()
            || self.result.is_some()
        {
            return None;
        }
        let input = self.input.as_ref()?;
        Some((input.tool()?, input.permission_target()?))
    }

    pub(crate) fn set_expanded(&mut self, expanded: bool, cx: &mut gpui_kit::Context<Self>) {
        if self.expanded != expanded {
            self.expanded = expanded;
            cx.notify();
        }
    }

    /// Number of logical compact/detail rows carried by this call.
    pub fn row_count(&self) -> usize {
        1 + if self.expanded {
            self.detail_row_count()
        } else {
            0
        }
    }

    pub(crate) fn estimated_height(&self) -> f32 {
        let detail_rows = self.row_count().saturating_sub(1);
        Layout::CONVERSATION_TOOL_HEIGHT
            + if detail_rows > 0 {
                Layout::PROCESS_ITEM_GAP
                    + (detail_rows as f32 * ROW_HEIGHT + Layout::TOOL_DETAIL_PADDING * 2.)
                        .min(Layout::DISCLOSURE_CONTENT_MAX_HEIGHT)
                    + 2.
            } else {
                0.
            }
    }

    /// Whether this card is the atomic invalid terminal and must never prompt.
    pub fn is_invalid_terminal(&self) -> bool {
        matches!(
            self.result,
            Some(ToolCardResultProjection::InvalidRejected { .. })
        )
    }

    pub(crate) fn activity_category(&self) -> ToolActivityCategory {
        match (&self.input, &self.result) {
            (_, Some(ToolCardResultProjection::Corrupt)) => ToolActivityCategory::Other,
            (Some(ToolCardInputProjection::ReadOnly { tool, .. }), _) => match tool {
                ReadOnlyToolKind::Read => ToolActivityCategory::Read,
                ReadOnlyToolKind::Glob => ToolActivityCategory::Find,
                ReadOnlyToolKind::Grep => ToolActivityCategory::Search,
            },
            (Some(ToolCardInputProjection::Bash { .. }), _) => ToolActivityCategory::Shell,
            (Some(ToolCardInputProjection::Write { .. }), _) => ToolActivityCategory::Write,
            (Some(ToolCardInputProjection::Edit { .. }), _) => ToolActivityCategory::Edit,
            (Some(ToolCardInputProjection::Mcp { .. }), _) => ToolActivityCategory::Mcp,
            (Some(ToolCardInputProjection::CodexAcp { .. }), _) => ToolActivityCategory::CodexAcp,
            (Some(ToolCardInputProjection::Skill { .. }), _) => ToolActivityCategory::Skill,
            (Some(ToolCardInputProjection::ComputerUse { .. }), _) => {
                ToolActivityCategory::ComputerUse
            }
            (None, Some(ToolCardResultProjection::InvalidRejected { tool, .. })) => match tool {
                InvalidToolKind::Bash => ToolActivityCategory::Shell,
                InvalidToolKind::Write => ToolActivityCategory::Write,
                InvalidToolKind::Edit => ToolActivityCategory::Edit,
            },
            _ => ToolActivityCategory::Other,
        }
    }

    pub(crate) fn stage_activity_category(&self) -> ToolActivityCategory {
        match &self.input {
            Some(ToolCardInputProjection::ReadOnly { tool, .. }) => match tool {
                ReadOnlyToolKind::Read => ToolActivityCategory::Read,
                ReadOnlyToolKind::Glob => ToolActivityCategory::Find,
                ReadOnlyToolKind::Grep => ToolActivityCategory::Search,
            },
            Some(ToolCardInputProjection::Bash { .. }) => ToolActivityCategory::Shell,
            Some(ToolCardInputProjection::Write { .. }) => ToolActivityCategory::Write,
            Some(ToolCardInputProjection::Edit { .. }) => ToolActivityCategory::Edit,
            Some(ToolCardInputProjection::Mcp { .. }) => ToolActivityCategory::Mcp,
            Some(ToolCardInputProjection::CodexAcp { .. }) => ToolActivityCategory::CodexAcp,
            Some(ToolCardInputProjection::Skill { .. }) => ToolActivityCategory::Skill,
            Some(ToolCardInputProjection::ComputerUse { .. }) => ToolActivityCategory::ComputerUse,
            None => match &self.result {
                Some(ToolCardResultProjection::InvalidRejected { tool, .. }) => match tool {
                    InvalidToolKind::Bash => ToolActivityCategory::Shell,
                    InvalidToolKind::Write => ToolActivityCategory::Write,
                    InvalidToolKind::Edit => ToolActivityCategory::Edit,
                },
                _ => ToolActivityCategory::Other,
            },
            Some(ToolCardInputProjection::Corrupt) => ToolActivityCategory::Other,
        }
    }

    pub(crate) fn activity_state(&self) -> ToolActivityState {
        if self.status == ToolCallStatus::Cancelled
            && matches!(
                &self.input,
                Some(ToolCardInputProjection::ComputerUse { .. })
            )
            && self.approval == Some(Approval::Once)
            && matches!(&self.result, Some(ToolCardResultProjection::ComputerUse { output, .. }) if output == vega_conversation::types::SUPERSEDED_CONTROL_OUTPUT)
        {
            return ToolActivityState::Superseded;
        }
        if self.bash_exit_failed() || matches!(self.result, Some(ToolCardResultProjection::Corrupt))
        {
            return ToolActivityState::Failed;
        }
        match self.status {
            ToolCallStatus::PendingApproval
            | ToolCallStatus::Approved
            | ToolCallStatus::Running => ToolActivityState::Active,
            ToolCallStatus::Success => ToolActivityState::Success,
            ToolCallStatus::Rejected => ToolActivityState::Rejected,
            ToolCallStatus::Failed => ToolActivityState::Failed,
            ToolCallStatus::Cancelled => ToolActivityState::Cancelled,
        }
    }

    #[cfg(test)]
    pub(crate) fn needs_attention(&self) -> bool {
        self.status == ToolCallStatus::PendingApproval
            || matches!(
                self.activity_state(),
                ToolActivityState::Rejected | ToolActivityState::Failed
            )
    }

    pub(crate) fn leading_icon(&self) -> Icon {
        if self.activity_category() == ToolActivityCategory::ComputerUse
            && let Some(ToolCardInputProjection::ComputerUse { action, .. }) = &self.input
        {
            return match action.as_str() {
                "screenshot" => Icon::Camera,
                "click" | "scroll" => Icon::MousePointer,
                "type_text" | "press_key" | "hotkey" => Icon::Keyboard,
                "navigate" => Icon::Globe,
                _ => Icon::Monitor,
            };
        }
        self.activity_category().icon()
    }

    pub(crate) fn leading_icon_color(&self, colors: &ThemeColors) -> gpui_kit::Rgba {
        colors.conversation_tool
    }

    /// Content rendered by the card, used by leak-focused tests.
    pub fn visible_text(&self) -> String {
        let mut text = self.compact_row_text();
        if self.expanded
            && let Some(detail) = self.detail()
        {
            detail.append_visible_text(&mut text);
        }
        text
    }

    #[cfg(test)]
    pub(crate) fn live_elapsed_active(&self) -> bool {
        self.running_started_at.is_some() && self.elapsed_refresh_task.is_some()
    }

    /// whether this card's bounded detail is currently disclosed.
    pub(crate) fn is_expanded(&self) -> bool {
        self.expanded
    }

    /// The resting compact row copy, without any disclosed detail rows.
    /// opens the newest unit's detail by default, so elapsed-copy assertions
    /// read this instead of [`Self::visible_text`].
    #[cfg(test)]
    pub(crate) fn compact_visible_text(&self) -> String {
        self.compact_row_text()
    }

    fn compact_row_text(&self) -> String {
        if !self.expanded
            || !self.is_bash()
            || matches!(
                self.result,
                Some(
                    ToolCardResultProjection::Corrupt
                        | ToolCardResultProjection::InvalidRejected { .. }
                )
            )
        {
            return self.summary.clone();
        }
        let (label, connector) = match self.activity_state() {
            ToolActivityState::Success => ("Ran command", "in"),
            ToolActivityState::Failed => ("Command failed", "after"),
            ToolActivityState::Cancelled => ("Command cancelled", "after"),
            ToolActivityState::Rejected => ("Command rejected", "after"),
            ToolActivityState::Superseded => ("Not run after takeover", "after"),
            ToolActivityState::Active if self.status == ToolCallStatus::PendingApproval => {
                ("Awaiting command approval", "for")
            }
            ToolActivityState::Active if self.status == ToolCallStatus::Approved => {
                ("Ready to run command", "for")
            }
            ToolActivityState::Active => ("Running command", "for"),
        };
        self.bash_duration().map_or_else(
            || label.to_owned(),
            |duration| format!("{label} {connector} {duration}"),
        )
    }

    fn bash_duration(&self) -> Option<String> {
        if !self.is_bash() {
            return None;
        }
        if self.status == ToolCallStatus::Running {
            return self.running_elapsed_seconds.map(human_elapsed);
        }
        match &self.result {
            Some(ToolCardResultProjection::Bash {
                duration_ms: Some(duration),
                ..
            }) => Some(human_duration(*duration)),
            _ => None,
        }
    }

    pub(crate) fn render(
        card: Entity<Self>,
        selector: String,
        indented: bool,
        window: &mut gpui_kit::Window,
        cx: &mut App,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let (title, leading_icon, leading_icon_color, expanded, expandable, detail) = {
            let card_ref = card.read(cx);
            let expanded = card_ref.expanded;
            let title = card_ref.compact_row_text();
            (
                title,
                card_ref.leading_icon(),
                card_ref.leading_icon_color(&colors),
                expanded,
                card_ref.has_detail(),
                if expanded { card_ref.detail() } else { None },
            )
        };
        let detail_scroll = card.read(cx).detail_scroll.clone();
        let detail_id = format!("tool-detail-{}", card.entity_id());
        let toggle_card = card.clone();
        let keyboard_card = card.clone();
        let debug_selector = selector.clone();
        let icon_selector = format!("{selector}-icon");
        let title_selector = format!("{selector}-title");
        let chevron_selector = format!("{selector}-chevron");
        let disclosure_group: gpui_kit::SharedString =
            format!("tool-disclosure-{}", card.entity_id()).into();
        let focus =
            crate::disclosure::focus(disclosure_group.clone(), window, cx).tab_stop(expandable);
        let row = div()
            .id(("tool-activity-toggle", card.entity_id()))
            .group(disclosure_group.clone())
            .track_focus(&focus)
            .aria_expanded(expanded)
            .aria_label(format!(
                "{title}, {} tool details",
                if expanded { "collapse" } else { "expand" }
            ))
            .tab_stop(expandable)
            .focus_visible(move |row| row.bg(colors.bg_hover))
            .debug_selector(move || debug_selector.clone())
            .h(px(Layout::CONVERSATION_TOOL_HEIGHT))
            .text_size(px(Typography::CONVERSATION))
            .line_height(gpui_kit::relative(Typography::CONVERSATION_LINE_HEIGHT))
            .w_full()
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(Layout::CONVERSATION_INLINE_GAP))
            .on_key_down(move |event, _, cx| {
                if expandable && matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    keyboard_card.update(cx, |card, cx| {
                        card.expanded = !card.expanded;
                        cx.notify();
                    });
                    cx.stop_propagation();
                }
            })
            .when(expandable, |row| {
                row.cursor_pointer().on_mouse_up(
                    MouseButton::Left,
                    move |_: &MouseUpEvent, _, cx| {
                        toggle_card.update(cx, |card, cx| {
                            card.expanded = !card.expanded;
                            cx.notify();
                        });
                    },
                )
            })
            .child(
                div()
                    .debug_selector(move || icon_selector.clone())
                    .size(px(Layout::ICON_SIZE))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(crate::icons::sized_icon(
                        leading_icon,
                        leading_icon_color,
                        Layout::ICON_SIZE,
                    )),
            )
            .child(
                div()
                    .debug_selector(move || title_selector.clone())
                    .min_w_0()
                    .flex_shrink(1.)
                    .truncate()
                    .text_size(px(Typography::CONVERSATION))
                    .text_color(colors.conversation_tool)
                    .child(title),
            )
            .child(crate::disclosure::inline_chevron(
                disclosure_group,
                chevron_selector,
                &focus,
                expanded,
                expandable,
                colors,
                window,
            ));
        div()
            .w_full()
            .min_w_0()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .when(indented, |card| card.pl_4())
            .child(row)
            .when_some(expanded.then_some(detail).flatten(), move |card, detail| {
                card.child(render_detail(
                    detail,
                    format!("{selector}-detail"),
                    detail_id,
                    &detail_scroll,
                    &colors,
                    cx,
                ))
            })
            .into_any_element()
    }

    fn refresh_summary(&mut self) {
        self.summary = self.build_summary();
    }

    fn build_summary(&self) -> String {
        match (&self.input, &self.result) {
            (_, Some(ToolCardResultProjection::Corrupt)) => CORRUPT_LABEL.to_string(),
            (
                _,
                Some(ToolCardResultProjection::InvalidRejected {
                    tool: InvalidToolKind::Bash,
                    ..
                }),
            ) => "Command rejected: invalid input · cmd must be a non-empty string; timeout_ms must be a positive integer when provided; other fields are unsupported"
                .to_string(),
            (_, Some(ToolCardResultProjection::InvalidRejected { tool, code, .. })) => {
                format!("Rejected {}: {}", invalid_action(*tool), code.as_str())
            }
            (Some(ToolCardInputProjection::Bash { .. }), _) => self.build_bash_summary(),
            (Some(ToolCardInputProjection::ReadOnly { tool, display_target, .. }), _) => {
                let summary = readonly_summary(*tool, self.activity_state(), self.status, display_target.is_some());
                display_target.as_ref().map_or(summary.clone(), |target| format!("{summary} {}", compact_object(target)))
            }
            (
                Some(ToolCardInputProjection::Write {
                    path,
                    content_bytes,
                }),
                None,
            ) => format!(
                "{} {path} · {content_bytes} bytes",
                mutation_verb("write", self.activity_state(), self.status)
            ),
            (
                Some(ToolCardInputProjection::Edit {
                    path,
                    old_string_bytes,
                    new_string_bytes,
                }),
                None,
            ) => format!(
                "{} {path} · {old_string_bytes}→{new_string_bytes} bytes",
                mutation_verb("edit", self.activity_state(), self.status)
            ),
            (
                _,
                Some(ToolCardResultProjection::WriteSuccess {
                    path,
                    bytes_written,
                    ..
                }),
            ) => format!("Wrote {path} · {bytes_written} bytes"),
            (
                _,
                Some(ToolCardResultProjection::EditSuccess {
                    path,
                    bytes_written,
                    replacements,
                    ..
                }),
            ) => format!(
                "Edited {path} · {bytes_written} bytes · {replacements} replacements"
            ),
            (
                Some(ToolCardInputProjection::Write { path, .. }),
                Some(ToolCardResultProjection::MutationTerminal { .. }),
            ) => format!(
                "{} {path}",
                mutation_verb("write", self.activity_state(), self.status)
            ),
            (
                Some(ToolCardInputProjection::Edit { path, .. }),
                Some(ToolCardResultProjection::MutationTerminal { .. }),
            ) => format!(
                "{} {path}",
                mutation_verb("edit", self.activity_state(), self.status)
            ),
            (
                Some(ToolCardInputProjection::Skill { name, .. }),
                Some(ToolCardResultProjection::Skill { outcome, .. }),
            ) => skill_summary(name.as_deref(), outcome),
            (Some(ToolCardInputProjection::Skill { name, .. }), None) => format!(
                "{} Skill {}",
                generic_verb("use", self.activity_state(), self.status),
                name.as_deref().unwrap_or("unknown")
            ),
            (Some(ToolCardInputProjection::ComputerUse { tool, action }), _) => {
                if self.activity_state() == ToolActivityState::Superseded {
                    return "Not run after takeover".into();
                }
                let verb = match tool.as_str() {
                    "ComputerObserve" | "BrowserObserve" => "observe",
                    "ComputerScreenshot" => "capture",
                    _ => "control",
                };
                format!(
                    "{} {} · {}{}",
                    generic_verb(verb, self.activity_state(), self.status),
                    if tool.starts_with("Computer") { "computer" } else { "browser" },
                    action,
                    if verb == "control" && self.approval == Some(Approval::Once) {
                        " · approved for this action"
                    } else {
                        ""
                    }
                )
            }
            (Some(ToolCardInputProjection::Mcp { identity, .. }), _) => format!(
                "{} {} · server {}",
                generic_verb("call", self.activity_state(), self.status),
                identity.exact_tool_name,
                identity.server_id
            ),
            (Some(ToolCardInputProjection::CodexAcp { identity, .. }), _) => format!(
                "{} Codex ACP · {}",
                generic_verb("run", self.activity_state(), self.status),
                identity.kind
            ),
            _ => CORRUPT_LABEL.to_string(),
        }
    }

    fn bash_exit_failed(&self) -> bool {
        matches!(
            self.result,
            Some(ToolCardResultProjection::Bash {
                exit_code: Some(code),
                ..
            }) if code != 0
        )
    }

    fn is_bash(&self) -> bool {
        matches!(self.input, Some(ToolCardInputProjection::Bash { .. }))
    }

    fn build_bash_summary(&self) -> String {
        let action = match self.activity_state() {
            ToolActivityState::Superseded => "Not run after takeover",
            ToolActivityState::Success => "Ran",
            ToolActivityState::Active if self.status == ToolCallStatus::PendingApproval => {
                "Awaiting approval to run"
            }
            ToolActivityState::Active if self.status == ToolCallStatus::Approved => "Ready to run",
            ToolActivityState::Active => "Running",
            ToolActivityState::Rejected => "Command rejected:",
            ToolActivityState::Cancelled => "Command cancelled:",
            ToolActivityState::Failed => "Command failed:",
        };
        let Some(ToolCardInputProjection::Bash { command }) = &self.input else {
            return action.into();
        };
        format!("{action} {}", compact_object(command))
    }

    fn stop_live_elapsed(&mut self) {
        self.running_started_at = None;
        self.running_elapsed_seconds = None;
        self.elapsed_refresh_task = None;
    }

    fn bash_detail_title(&self) -> String {
        let mut title = "Shell".to_string();
        if let Some(ToolCardResultProjection::Bash {
            truncated, reused, ..
        }) = &self.result
        {
            if *truncated == Some(true) {
                title.push_str(" · truncated");
            }
            if *reused {
                title.push_str(" · reused");
            }
        }
        title
    }

    fn terminal_footer(&self) -> String {
        match self.activity_state() {
            ToolActivityState::Superseded => "Not run after takeover",
            ToolActivityState::Success => "Success",
            ToolActivityState::Active if self.status == ToolCallStatus::PendingApproval => {
                "Awaiting approval"
            }
            ToolActivityState::Active if self.status == ToolCallStatus::Approved => "Ready to run",
            ToolActivityState::Active => "Running",
            ToolActivityState::Rejected => "Rejected",
            ToolActivityState::Cancelled => "Cancelled",
            ToolActivityState::Failed => "Failure",
        }
        .to_string()
    }

    fn has_detail(&self) -> bool {
        match &self.input {
            Some(ToolCardInputProjection::Bash { .. }) => true,
            Some(ToolCardInputProjection::ReadOnly { .. })
            | Some(ToolCardInputProjection::Mcp { .. })
            | Some(ToolCardInputProjection::ComputerUse { .. })
            | Some(ToolCardInputProjection::CodexAcp { .. }) => !self.output_rows.is_empty(),
            _ => false,
        }
    }

    fn detail_row_count(&self) -> usize {
        match &self.input {
            Some(ToolCardInputProjection::Bash { .. }) => 3 + self.output_rows.len(),
            Some(ToolCardInputProjection::ReadOnly { .. })
            | Some(ToolCardInputProjection::Mcp { .. })
            | Some(ToolCardInputProjection::ComputerUse { .. })
            | Some(ToolCardInputProjection::CodexAcp { .. }) => self.output_rows.len(),
            _ => 0,
        }
    }

    fn detail(&self) -> Option<ToolDetail> {
        match &self.input {
            Some(ToolCardInputProjection::Bash { command }) => Some(ToolDetail {
                title: Some(self.bash_detail_title()),
                command: Some(format!("$ {command}")),
                output: if self.output_rows.is_empty()
                    && self.activity_state() != ToolActivityState::Active
                {
                    vec!["No output".to_string()]
                } else {
                    self.output_rows.clone()
                },
                footer: Some((self.terminal_footer(), self.activity_state())),
            }),
            Some(ToolCardInputProjection::ReadOnly { .. }) if !self.output_rows.is_empty() => {
                Some(ToolDetail {
                    title: None,
                    command: None,
                    output: self.output_rows.clone(),
                    footer: None,
                })
            }
            Some(
                ToolCardInputProjection::Mcp { .. } | ToolCardInputProjection::ComputerUse { .. },
            ) if !self.output_rows.is_empty() => Some(ToolDetail {
                title: None,
                command: None,
                output: self.output_rows.clone(),
                footer: None,
            }),
            Some(ToolCardInputProjection::CodexAcp { .. }) if !self.output_rows.is_empty() => {
                Some(ToolDetail {
                    title: Some("Codex ACP".to_string()),
                    command: None,
                    output: self.output_rows.clone(),
                    footer: None,
                })
            }
            _ => None,
        }
    }
}

fn invalid_action(tool: InvalidToolKind) -> &'static str {
    match tool {
        InvalidToolKind::Bash => "run",
        InvalidToolKind::Write => "write",
        InvalidToolKind::Edit => "edit",
    }
}

fn generic_verb(action: &'static str, state: ToolActivityState, status: ToolCallStatus) -> String {
    let (past, ongoing) = match action {
        "write" => ("Wrote", "Writing"),
        "edit" => ("Edited", "Editing"),
        "call" => ("Called", "Calling"),
        "run" => ("Ran", "Running"),
        "use" => ("Used", "Using"),
        "observe" => ("Observed", "Observing"),
        "capture" => ("Captured", "Capturing"),
        "control" => ("Controlled", "Controlling"),
        "read" => ("Read", "Reading"),
        "read files" => ("Read files", "Reading files"),
        "find files" => ("Found files", "Finding files"),
        "search" => ("Searched", "Searching"),
        "search content" => ("Searched content", "Searching content"),
        _ => (action, action),
    };
    match state {
        ToolActivityState::Superseded => "Not run after takeover".to_string(),
        ToolActivityState::Success => past.to_owned(),
        ToolActivityState::Active if status == ToolCallStatus::PendingApproval => {
            format!("Awaiting approval to {action}")
        }
        ToolActivityState::Active if status == ToolCallStatus::Approved => {
            format!("Ready to {action}")
        }
        ToolActivityState::Active => ongoing.to_owned(),
        ToolActivityState::Rejected => format!("Rejected: {action}"),
        ToolActivityState::Cancelled => format!("Cancelled: {action}"),
        ToolActivityState::Failed => format!("Failed to {action}"),
    }
}

fn mutation_verb(action: &'static str, state: ToolActivityState, status: ToolCallStatus) -> String {
    generic_verb(action, state, status)
}

fn compact_object(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect()
}

fn readonly_summary(
    tool: ReadOnlyToolKind,
    state: ToolActivityState,
    status: ToolCallStatus,
    has_target: bool,
) -> String {
    let action = match tool {
        ReadOnlyToolKind::Read if has_target => "read",
        ReadOnlyToolKind::Grep if has_target => "search",
        ReadOnlyToolKind::Read => "read files",
        ReadOnlyToolKind::Glob => "find files",
        ReadOnlyToolKind::Grep => "search content",
    };
    generic_verb(action, state, status)
}

fn skill_summary(name: Option<&str>, outcome: &SkillCardOutcome) -> String {
    let name = name.unwrap_or("unknown");
    match outcome {
        SkillCardOutcome::Loaded => format!("Loaded Skill {name}"),
        SkillCardOutcome::AlreadyLoaded => format!("Loaded Skill {name} · reused"),
        SkillCardOutcome::ResourceRead { text_bytes, sha256 } => format!(
            "Read Skill {name} reference · {text_bytes} bytes · SHA-256 {}…",
            sha256.get(..12).unwrap_or("invalid")
        ),
        SkillCardOutcome::AssetMetadata { size_bytes } => {
            format!(
                "Viewed Skill {name} asset metadata · {size_bytes} bytes · unknown type · content not read"
            )
        }
        SkillCardOutcome::Failed { code } => format!("Skill {name} failed · {code}"),
        SkillCardOutcome::Rejected => format!("Rejected Skill {name}"),
        SkillCardOutcome::Cancelled => format!("Cancelled Skill {name}"),
    }
}

fn human_duration(duration_ms: u64) -> String {
    if duration_ms < 1000 {
        return format!("{duration_ms}ms");
    }
    if duration_ms < 60_000 && !duration_ms.is_multiple_of(1000) {
        return format!("{:.1}s", duration_ms as f64 / 1000.0);
    }
    human_elapsed(duration_ms / 1000)
}

fn human_elapsed(seconds: u64) -> String {
    if seconds < 60 {
        return format!("{seconds}s");
    }
    let minutes = seconds / 60;
    let remaining_seconds = seconds % 60;
    if minutes < 60 {
        return if remaining_seconds == 0 {
            format!("{minutes}m")
        } else {
            format!("{minutes}m {remaining_seconds}s")
        };
    }
    let hours = minutes / 60;
    let remaining_minutes = minutes % 60;
    match (remaining_minutes, remaining_seconds) {
        (0, 0) => format!("{hours}h"),
        (_, 0) => format!("{hours}h {remaining_minutes}m"),
        (0, _) => format!("{hours}h {remaining_seconds}s"),
        _ => format!("{hours}h {remaining_minutes}m {remaining_seconds}s"),
    }
}

fn projection_output_rows(projection: &ToolCardResultProjection) -> Vec<String> {
    let output = match projection {
        ToolCardResultProjection::Bash { output, .. }
        | ToolCardResultProjection::ReadOnly { output, .. }
        | ToolCardResultProjection::Mcp { output, .. }
        | ToolCardResultProjection::ComputerUse { output, .. }
        | ToolCardResultProjection::CodexAcp { output, .. } => output,
        _ => return Vec::new(),
    };
    output.lines().map(str::to_string).collect()
}

pub(crate) fn contain_disclosure_scroll(
    scroll: &ScrollHandle,
) -> impl Fn(&gpui_kit::ScrollWheelEvent, &mut gpui_kit::Window, &mut App) + 'static {
    let scroll = scroll.clone();
    move |event, window, cx| {
        let delta = event.delta.pixel_delta(window.line_height());
        let dy = if delta.y != px(0.) { delta.y } else { delta.x };
        let max = scroll.max_offset().y;
        let after = scroll.offset().y;
        let before = (after - dy).clamp(-max, px(0.));
        if before != after.clamp(-max, px(0.)) {
            cx.stop_propagation();
        }
    }
}

fn render_detail(
    detail: ToolDetail,
    selector: String,
    id: String,
    scroll: &ScrollHandle,
    colors: &ThemeColors,
    cx: &App,
) -> AnyElement {
    let debug_selector = selector.clone();
    let mut rows = Vec::with_capacity(detail.logical_rows());
    if let Some(title) = detail.title {
        rows.push(detail_row(
            "tool-detail-type",
            title,
            colors.text_secondary,
            false,
        ));
    }
    if let Some(command) = detail.command {
        rows.push(detail_row(
            "tool-detail-command",
            command,
            colors.text_primary,
            true,
        ));
    }
    rows.extend(detail.output.into_iter().enumerate().map(|(index, line)| {
        detail_row(
            gpui_kit::SharedString::from(format!("tool-output-{index}")),
            line,
            colors.text_primary,
            true,
        )
    }));
    if let Some((footer, state)) = detail.footer {
        let color = if state == ToolActivityState::Success {
            colors.text_secondary
        } else {
            state.terminal_color(colors)
        };
        let row_selector = format!("{selector}-footer-row");
        let status_selector = format!("{selector}-footer-status");
        let icon_selector = format!("{selector}-footer-icon");
        rows.push(
            div()
                .debug_selector(move || row_selector.clone())
                .w_full()
                .min_h(px(ROW_HEIGHT))
                .flex_shrink_0()
                .flex()
                .justify_end()
                .child(
                    div()
                        .debug_selector(move || status_selector.clone())
                        .flex()
                        .items_center()
                        .gap(px(Layout::CONVERSATION_INLINE_GAP))
                        .text_size(px(Typography::CONVERSATION))
                        .text_color(color)
                        .when_some(state.terminal_icon(), |row, icon| {
                            row.child(
                                div()
                                    .debug_selector(move || icon_selector.clone())
                                    .size(px(Layout::ICON_SIZE))
                                    .flex_shrink_0()
                                    .child(crate::icons::sized_icon(
                                        icon,
                                        color,
                                        Layout::ICON_SIZE,
                                    )),
                            )
                        })
                        .child(footer),
                )
                .into_any_element(),
        );
    }
    div()
        .relative()
        .w_full()
        .min_w_0()
        .mt(px(Layout::PROCESS_ITEM_GAP))
        .rounded(px(Layout::TOOL_DETAIL_RADIUS))
        .bg(colors.code_bg)
        .border_1()
        .border_color(colors.border_subtle)
        .child(
            div()
                .id(gpui_kit::SharedString::from(id))
                .debug_selector(move || debug_selector.clone())
                .max_h(px(Layout::DISCLOSURE_CONTENT_MAX_HEIGHT))
                .track_scroll(scroll)
                .on_scroll_wheel(contain_disclosure_scroll(scroll))
                .w_full()
                .min_w_0()
                .p(px(Layout::TOOL_DETAIL_PADDING))
                .pr(px(
                    Layout::TOOL_DETAIL_PADDING + Layout::DISCLOSURE_SCROLLBAR_GUTTER
                ))
                .overflow_x_hidden()
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .children(rows),
        )
        .child(disclosure_scrollbar(scroll, cx))
        .into_any_element()
}

pub(crate) fn disclosure_scrollbar(scroll: &ScrollHandle, cx: &App) -> gpui_kit::base::Scrollbar {
    gpui_kit::base::Scrollbar::vertical(scroll).mode(if cx.should_auto_hide_scrollbars() {
        gpui_kit::base::ScrollbarMode::Hover
    } else {
        gpui_kit::base::ScrollbarMode::Always
    })
}

fn detail_row(
    id: impl Into<gpui_kit::ElementId>,
    text: String,
    color: gpui_kit::Rgba,
    code: bool,
) -> AnyElement {
    div()
        .w_full()
        .min_w_0()
        .min_h(px(ROW_HEIGHT))
        .flex_shrink_0()
        .line_height(gpui_kit::relative(Typography::CODE_LINE_HEIGHT))
        .when(code, |row| row.font_family(MONOFONT))
        .text_size(px(if code {
            Typography::CODE
        } else {
            Typography::BODY
        }))
        .text_color(color)
        .child(gpui_kit::base::SelectableText::new(id, text))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use vega_conversation::types::{InvalidToolCode, InvalidToolKind, InvalidToolProjection};

    fn result(status: ToolCallStatus, output: &str) -> ToolResult {
        ToolResult {
            status,
            output: output.to_string(),
            reused: false,
            exit_code: None,
            duration_ms: None,
            truncated: None,
            invalid: None,
        }
    }

    #[test]
    fn tool_activity_leading_visual_is_category_owned_and_neutral_for_every_state() {
        for status in [
            ToolCallStatus::PendingApproval,
            ToolCallStatus::Approved,
            ToolCallStatus::Running,
            ToolCallStatus::Success,
            ToolCallStatus::Rejected,
            ToolCallStatus::Cancelled,
            ToolCallStatus::Failed,
        ] {
            for (input, expected) in [
                (
                    ToolCardInputProjection::Bash {
                        command: "true".into(),
                    },
                    Icon::Terminal,
                ),
                (
                    ToolCardInputProjection::ComputerUse {
                        tool: "ComputerAct".into(),
                        action: "click".into(),
                    },
                    Icon::MousePointer,
                ),
            ] {
                let card = ToolCard::hydrated(Some(input), status, None, None);
                assert_eq!(
                    card.leading_icon(),
                    expected,
                    "operation icon for {status:?}"
                );
                for colors in [vega_theme::LIGHT, vega_theme::DARK] {
                    assert_eq!(card.leading_icon_color(&colors), colors.conversation_tool);
                }
            }
        }

        for (category, expected) in [
            (ToolActivityCategory::Shell, Icon::Terminal),
            (ToolActivityCategory::Read, Icon::BookOpen),
            (ToolActivityCategory::Find, Icon::Folder),
            (ToolActivityCategory::Search, Icon::Search),
            (ToolActivityCategory::Write, Icon::Document),
            (ToolActivityCategory::Edit, Icon::Pencil),
            (ToolActivityCategory::Mcp, Icon::Plug),
            (ToolActivityCategory::CodexAcp, Icon::Bot),
            (ToolActivityCategory::ComputerUse, Icon::Monitor),
            (ToolActivityCategory::Skill, Icon::Wrench),
            (ToolActivityCategory::Other, Icon::Wrench),
        ] {
            assert_eq!(category.icon(), expected, "{category:?}");
        }

        for (action, expected) in [
            ("observe", Icon::Monitor),
            ("screenshot", Icon::Camera),
            ("click", Icon::MousePointer),
            ("scroll", Icon::MousePointer),
            ("type_text", Icon::Keyboard),
            ("press_key", Icon::Keyboard),
            ("hotkey", Icon::Keyboard),
            ("navigate", Icon::Globe),
            ("invalid", Icon::Monitor),
        ] {
            let tool = match action {
                "observe" => "ComputerObserve",
                "screenshot" => "ComputerScreenshot",
                "navigate" => "BrowserAct",
                _ => "ComputerAct",
            };
            let call = ToolCall {
                id: action.into(),
                tool: tool.into(),
                input_json: serde_json::json!({"kind":"computer_use", "version":1, "action":action, "argument_bytes":2, "argument_digest":"a".repeat(64)}).to_string(),
            };
            let mut card = ToolCard::proposed(&call);
            assert_eq!(card.leading_icon(), expected);
            assert!(card.apply_approved(Approval::Once));
            let mut finished = result(ToolCallStatus::Success, "Synthetic result");
            finished.truncated = Some(false);
            assert!(card.apply_finished(&finished));
            assert_eq!(card.leading_icon(), expected);
            let restored = ToolCard::hydrated(
                card.input.clone(),
                card.status,
                card.approval,
                card.result.clone(),
            );
            assert_eq!(restored.leading_icon(), expected);
            assert_eq!(restored.visible_text(), card.visible_text());
        }
        for action in ["", "future_action"] {
            let card = ToolCard::hydrated(
                Some(ToolCardInputProjection::ComputerUse {
                    tool: "ComputerAct".into(),
                    action: action.into(),
                }),
                ToolCallStatus::Running,
                None,
                None,
            );
            assert_eq!(card.leading_icon(), Icon::Monitor);
        }
        for input in [
            None,
            Some(ToolCardInputProjection::Corrupt),
            Some(ToolCardInputProjection::ComputerUse {
                tool: "ComputerObserve".into(),
                action: "observe".into(),
            }),
        ] {
            let card = ToolCard::hydrated(
                input,
                ToolCallStatus::Failed,
                None,
                Some(ToolCardResultProjection::Corrupt),
            );
            assert_eq!(card.leading_icon(), Icon::Wrench);
            assert_eq!(card.activity_state(), ToolActivityState::Failed);
            assert!(card.visible_text().contains(CORRUPT_LABEL));
        }
    }

    #[test]
    fn skill_load_and_reference_cards_show_success_without_reference_body() {
        let load = ToolCall {
            id: "load-reviewer".into(),
            tool: "load_skill".into(),
            input_json: r#"{"name":"reviewer"}"#.into(),
        };
        let mut load_card = ToolCard::proposed(&load);
        assert!(load_card.apply_approved(Approval::Once));
        let mut loaded = result(
            ToolCallStatus::Success,
            r#"{"name":"reviewer","status":"loaded"}"#,
        );
        loaded.truncated = Some(false);
        assert!(load_card.apply_finished(&loaded));
        assert!(load_card.visible_text().contains("Loaded Skill reviewer"));
        assert!(!load_card.visible_text().contains(CORRUPT_LABEL));

        let read = ToolCall {
            id: "read-reviewer".into(),
            tool: "read_skill_resource".into(),
            input_json: r#"{"name":"reviewer","path_bytes":19,"path_sha256":"00f28c21b21007f540efab680c48c95d3914621a9ef1e064ebbdb1277d34ef88"}"#.into(),
        };
        let mut read_card = ToolCard::proposed(&read);
        assert!(read_card.apply_approved(Approval::Once));
        let mut returned = result(
            ToolCallStatus::Success,
            "[Lower-trust Skill reference]\n{\"name\":\"reviewer\",\"path\":\"references/notes.md\",\"text\":\"PRIVATE REFERENCE\",\"lower_trust\":true,\"content_sha256\":\"f30f856c035ac9d081141af0397093625c492f488189b61f67f7b258540c75d7\"}",
        );
        returned.truncated = Some(false);
        assert!(read_card.apply_finished(&returned));
        let visible = read_card.visible_text();
        assert!(visible.contains("Read Skill reviewer reference"));
        assert!(!visible.contains(CORRUPT_LABEL));
        assert!(!visible.contains("PRIVATE REFERENCE"));
        assert!(!visible.contains("references/notes.md"));
    }

    #[test]
    fn malformed_skill_receipt_or_reference_stays_content_free_corrupt() {
        let load = ToolCall {
            id: "load-reviewer".into(),
            tool: "load_skill".into(),
            input_json: r#"{"name":"reviewer"}"#.into(),
        };
        let mut load_card = ToolCard::proposed(&load);
        assert!(load_card.apply_approved(Approval::Once));
        let mut wrong_receipt = result(
            ToolCallStatus::Success,
            r#"{"name":"other","status":"loaded","private":"PRIVATE BODY"}"#,
        );
        wrong_receipt.truncated = Some(false);
        assert!(!load_card.apply_finished(&wrong_receipt));
        assert!(load_card.visible_text().contains(CORRUPT_LABEL));
        assert!(!load_card.visible_text().contains("PRIVATE BODY"));

        let raw_path = ToolCard::proposed(&ToolCall {
            id: "unsafe-input".into(),
            tool: "read_skill_resource".into(),
            input_json: r#"{"name":"reviewer","path":"/SECRET_ROOT/notes.md"}"#.into(),
        });
        assert!(raw_path.visible_text().contains(CORRUPT_LABEL));
        assert!(!raw_path.visible_text().contains("SECRET_ROOT"));

        let read = ToolCall {
            id: "read-reviewer".into(),
            tool: "read_skill_resource".into(),
            input_json: r#"{"name":"reviewer","path_bytes":19,"path_sha256":"00f28c21b21007f540efab680c48c95d3914621a9ef1e064ebbdb1277d34ef88"}"#.into(),
        };
        let mut read_card = ToolCard::proposed(&read);
        assert!(read_card.apply_approved(Approval::Once));
        let mut forged_reference = result(
            ToolCallStatus::Success,
            "[Lower-trust Skill reference]\n{\"name\":\"reviewer\",\"path\":\"references/notes.md\",\"text\":\"PRIVATE REFERENCE\",\"lower_trust\":true,\"content_sha256\":\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"}",
        );
        forged_reference.truncated = Some(false);
        assert!(!read_card.apply_finished(&forged_reference));
        assert!(read_card.visible_text().contains(CORRUPT_LABEL));
        assert!(!read_card.visible_text().contains("PRIVATE REFERENCE"));
    }

    #[test]
    fn write_success_hides_fingerprint_and_checkpoint_ref() {
        let call = ToolCall {
            id: "SECRET_CALL_ID".into(),
            tool: "write".into(),
            input_json: r#"{"audit_version":"write_edit_v1","tool":"write","path":"src/lib.rs","content_bytes":3,"fingerprint_v1":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#.into(),
        };
        let mut card = ToolCard::proposed(&call);
        assert!(card.apply_approved(Approval::Once));
        let mut terminal = result(
            ToolCallStatus::Success,
            r#"{"path":"src/lib.rs","bytes_written":3,"checkpoint_ref":"preimage-v1/id-70/id-74/id-63"}"#,
        );
        terminal.truncated = Some(false);
        assert!(card.apply_finished(&terminal));
        let visible = card.visible_text();
        assert!(visible.contains("src/lib.rs · 3 bytes"));
        assert!(!visible.contains("SECRET_CALL_ID"));
        assert!(!visible.contains("aaaaaaaa"));
        assert!(!visible.contains("preimage-v1"));
    }

    #[test]
    fn corrupt_success_is_content_free() {
        let call = ToolCall {
            id: "call".into(),
            tool: "edit".into(),
            input_json: r#"{"audit_version":"write_edit_v1","tool":"edit","path":"src/lib.rs","old_string_bytes":3,"new_string_bytes":4,"fingerprint_v1":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#.into(),
        };
        let mut card = ToolCard::proposed(&call);
        assert!(card.apply_approved(Approval::Once));
        assert!(!card.apply_finished(&result(
            ToolCallStatus::Success,
            r#"{"path":"/SECRET_ROOT/file","bytes_written":4,"replacements":2,"checkpoint_ref":"SECRET_REF"}"#,
        )));
        let visible = card.visible_text();
        assert!(visible.contains(CORRUPT_LABEL));
        assert!(!visible.contains("SECRET_ROOT"));
        assert!(!visible.contains("SECRET_REF"));
    }

    #[test]
    fn invalid_terminal_uses_typed_projection_only() {
        let result = ToolResult {
            status: ToolCallStatus::Rejected,
            output: "Tool error: invalid write input (malformed_json)".into(),
            reused: false,
            exit_code: None,
            duration_ms: None,
            truncated: None,
            invalid: Some(InvalidToolProjection::new(
                InvalidToolKind::Write,
                InvalidToolCode::MalformedJson,
            )),
        };
        let card = ToolCard::invalid_terminal(&result);
        assert!(card.is_invalid_terminal());
        assert_eq!(card.visible_text(), "Rejected write: malformed_json");
    }

    #[test]
    fn codex_tool_rows_identify_safe_objects_and_keep_metadata_in_details() {
        for (tool, input, expected) in [
            (
                "bash",
                r#"{"cmd":"printf 'ok'"}"#,
                "Awaiting approval to run printf 'ok'",
            ),
            (
                "read",
                r#"{"file_path":"src/main.rs"}"#,
                "Awaiting approval to read main.rs",
            ),
            (
                "glob",
                r#"{"pattern":"src/**/*.rs"}"#,
                "Awaiting approval to find files",
            ),
            (
                "grep",
                r#"{"pattern":"TODO","path":"src"}"#,
                "Awaiting approval to search content",
            ),
        ] {
            let card = ToolCard::proposed(&ToolCall {
                id: "object-row".into(),
                tool: tool.into(),
                input_json: input.into(),
            });
            assert_eq!(card.visible_text(), expected);
            assert_eq!(card.row_count(), 1);
        }
    }

    #[test]
    fn compact_duration_uses_english_units_without_changing_clock_values() {
        for (milliseconds, expected) in [
            (0, "0ms"),
            (12, "12ms"),
            (1234, "1.2s"),
            (3000, "3s"),
            (65000, "1m 5s"),
            (3600000, "1h"),
            (3665000, "1h 1m 5s"),
        ] {
            assert_eq!(human_duration(milliseconds), expected);
        }
        assert_eq!(human_elapsed(0), "0s");
    }

    #[test]
    fn expanded_shell_title_projects_current_state_without_mutating_command() {
        for (exit_code, collapsed, expanded) in [
            (0, "Ran echo hello", "Ran command in 3s"),
            (1, "Command failed: echo hello", "Command failed after 3s"),
        ] {
            let mut card = ToolCard::hydrated(
                Some(ToolCardInputProjection::Bash {
                    command: "echo hello".into(),
                }),
                ToolCallStatus::Success,
                Some(Approval::Once),
                Some(ToolCardResultProjection::Bash {
                    status: ToolCallStatus::Success,
                    output: "原始输出".into(),
                    exit_code: Some(exit_code),
                    duration_ms: Some(3000),
                    truncated: Some(false),
                    reused: false,
                }),
            );
            assert_eq!(card.compact_visible_text(), collapsed);
            card.expanded = true;
            assert_eq!(card.compact_visible_text(), expanded);
            assert!(card.visible_text().contains("$ echo hello\n原始输出"));
            card.expanded = false;
            assert_eq!(card.compact_visible_text(), collapsed);
        }
    }

    #[test]
    fn hydrated_terminal_without_duration_keeps_status_without_inventing_time() {
        let mut card = ToolCard::hydrated(
            Some(ToolCardInputProjection::Bash {
                command: "true".into(),
            }),
            ToolCallStatus::Success,
            Some(Approval::Once),
            Some(ToolCardResultProjection::Bash {
                status: ToolCallStatus::Success,
                output: String::new(),
                exit_code: Some(0),
                duration_ms: None,
                truncated: Some(false),
                reused: false,
            }),
        );
        assert_eq!(card.bash_duration(), None);
        assert_eq!(card.visible_text(), "Ran true");
        assert!(!card.live_elapsed_active());
        card.expanded = true;
        assert!(card.visible_text().ends_with("Success"));
        assert_eq!(card.compact_visible_text(), "Ran command");
    }

    #[test]
    fn bash_output_starts_collapsed_and_metadata_is_structured() {
        let call = ToolCall {
            id: "bash-call".into(),
            tool: "bash".into(),
            input_json: r#"{"cmd":"printf 'ok'"}"#.into(),
        };
        let mut card = ToolCard::proposed(&call);
        assert!(card.apply_approved(Approval::Once));
        let mut terminal = result(ToolCallStatus::Success, "ok");
        terminal.exit_code = Some(0);
        terminal.duration_ms = Some(12);
        terminal.truncated = Some(false);
        assert!(card.apply_finished(&terminal));
        assert_eq!(card.row_count(), 1);
        assert_eq!(card.visible_text(), "Ran printf 'ok'");
        assert!(card.visible_text().contains("printf 'ok'"));
        assert!(!card.visible_text().contains("\nok"));
        card.expanded = true;
        assert_eq!(card.row_count(), 5);
        assert!(
            card.visible_text()
                .contains("\nShell\n$ printf 'ok'\nok\nSuccess")
        );
    }

    #[test]
    fn nonzero_bash_exit_is_presented_as_failure_without_changing_status() {
        let call = ToolCall {
            id: "bash-call".into(),
            tool: "bash".into(),
            input_json: r#"{"cmd":"false"}"#.into(),
        };
        let mut card = ToolCard::proposed(&call);
        assert!(card.apply_approved(Approval::Once));
        let mut terminal = result(ToolCallStatus::Success, "");
        terminal.exit_code = Some(1);
        terminal.duration_ms = Some(4);
        terminal.truncated = Some(false);
        assert!(card.apply_finished(&terminal));
        assert_eq!(card.status, ToolCallStatus::Success);
        assert!(card.visible_text().contains("Command failed: false"));
    }

    #[test]
    fn bash_pending_running_and_terminal_failure_preserve_safe_command() {
        let command = "printf 'a  b'\nprintf\t'done'";
        for (status, result, expected) in [
            (
                ToolCallStatus::PendingApproval,
                None,
                "Awaiting approval to run",
            ),
            (ToolCallStatus::Approved, None, "Ready to run"),
            (ToolCallStatus::Running, None, "Running"),
            (
                ToolCallStatus::Rejected,
                Some(ToolCardResultProjection::Bash {
                    status: ToolCallStatus::Rejected,
                    output: "denied".into(),
                    exit_code: None,
                    duration_ms: None,
                    truncated: Some(false),
                    reused: false,
                }),
                "Command rejected:",
            ),
            (
                ToolCallStatus::Failed,
                Some(ToolCardResultProjection::Bash {
                    status: ToolCallStatus::Failed,
                    output: "spawn failed".into(),
                    exit_code: None,
                    duration_ms: None,
                    truncated: Some(false),
                    reused: false,
                }),
                "Command failed:",
            ),
            (
                ToolCallStatus::Cancelled,
                Some(ToolCardResultProjection::Bash {
                    status: ToolCallStatus::Cancelled,
                    output: "cancelled".into(),
                    exit_code: None,
                    duration_ms: None,
                    truncated: Some(false),
                    reused: false,
                }),
                "Command cancelled:",
            ),
        ] {
            let mut card = ToolCard::hydrated(
                Some(ToolCardInputProjection::Bash {
                    command: command.into(),
                }),
                status,
                None,
                result,
            );
            assert_eq!(
                card.visible_text(),
                format!("{expected} printf 'a  b' printf 'done'")
            );
            assert_eq!(card.row_count(), 1);
            card.expanded = true;
            let expanded = match status {
                ToolCallStatus::PendingApproval => "Awaiting command approval",
                ToolCallStatus::Approved => "Ready to run command",
                ToolCallStatus::Running => "Running command",
                ToolCallStatus::Rejected => "Command rejected",
                ToolCallStatus::Failed => "Command failed",
                ToolCallStatus::Cancelled => "Command cancelled",
                _ => unreachable!(),
            };
            assert_eq!(card.compact_visible_text(), expanded);
            assert!(card.visible_text().contains(&format!("$ {command}")));
            assert_eq!(card.status, status);
            assert!(!card.live_elapsed_active());
            card.expanded = false;
            assert_eq!(
                card.compact_visible_text(),
                format!("{expected} printf 'a  b' printf 'done'")
            );
        }
    }

    #[test]
    fn bash_compact_summary_identifies_command_and_preserves_terminal_metadata() {
        let command = "printf 'a  b'\nprintf done";
        for (status, exit, action) in [
            (ToolCallStatus::Success, Some(0), "Ran"),
            (ToolCallStatus::Success, Some(1), "Command failed:"),
            (ToolCallStatus::Success, Some(-1), "Command failed:"),
        ] {
            let mut card = ToolCard::hydrated(
                Some(ToolCardInputProjection::Bash {
                    command: command.into(),
                }),
                status,
                Some(Approval::Once),
                Some(ToolCardResultProjection::Bash {
                    status,
                    output: "result".into(),
                    exit_code: exit,
                    duration_ms: Some(1200),
                    truncated: Some(true),
                    reused: true,
                }),
            );
            assert_eq!(
                card.visible_text(),
                format!("{action} printf 'a  b' printf done")
            );
            assert_eq!(card.row_count(), 1);
            card.expanded = true;
            assert!(card.visible_text().contains(&format!("$ {command}")));
            assert!(card.visible_text().contains("Shell · truncated · reused"));
            assert_eq!(
                card.terminal_footer(),
                if exit == Some(0) {
                    "Success"
                } else {
                    "Failure"
                }
            );
            assert!(card.visible_text().contains("result"));
            if let Some(exit) = exit {
                assert!(!card.visible_text().contains(&format!("exit {exit}")));
                assert!(
                    matches!(card.result, Some(ToolCardResultProjection::Bash { exit_code: Some(actual), .. }) if actual == exit)
                );
            }
        }
    }

    #[test]
    fn lifecycle_replays_are_idempotent_but_regressions_fail_closed() {
        let call = ToolCall {
            id: "write-call".into(),
            tool: "write".into(),
            input_json: r#"{"audit_version":"write_edit_v1","tool":"write","path":"a.txt","content_bytes":1,"fingerprint_v1":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#.into(),
        };
        let mut card = ToolCard::proposed(&call);
        assert!(card.matches_call(&call));
        assert!(card.apply_approved(Approval::Once));
        assert!(card.apply_approved(Approval::Once));
        assert!(!card.matches_call(&call));
        assert!(!card.apply_approved(Approval::Always));

        let mut denied = ToolCard::proposed(&call);
        assert!(!denied.apply_approved(Approval::Deny));
        assert!(matches!(
            denied.result,
            Some(ToolCardResultProjection::Corrupt)
        ));
    }

    #[test]
    fn strict_success_truncation_shape_rejects_impossible_mutation_metadata() {
        let call = ToolCall {
            id: "write-call".into(),
            tool: "write".into(),
            input_json: r#"{"audit_version":"write_edit_v1","tool":"write","path":"a.txt","content_bytes":1,"fingerprint_v1":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#.into(),
        };
        let success_json = r#"{"path":"a.txt","bytes_written":1,"checkpoint_ref":"preimage-v1/id-70/id-74/id-63"}"#;

        for truncated in [None, Some(true)] {
            let mut card = ToolCard::proposed(&call);
            assert!(card.apply_approved(Approval::Once));
            let mut terminal = result(ToolCallStatus::Success, success_json);
            terminal.truncated = truncated;
            assert!(!card.apply_finished(&terminal));
        }

        let mut reused = result(ToolCallStatus::Success, success_json);
        reused.reused = true;
        assert!(matches!(
            tool_card_result_projection(Some(&tool_card_input_projection(&call)), &reused),
            ToolCardResultProjection::WriteSuccess { reused: true, .. }
        ));
    }

    #[test]
    fn mutation_audit_projection_rejects_each_corrupt_numeric_and_shape_class() {
        let bad_write_inputs = [
            r#"{}"#,
            r#"{"audit_version":"write_edit_v1","tool":"write","path":"a.txt","content_bytes":1,"fingerprint_v1":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","extra":true}"#,
            r#"{"audit_version":"write_edit_v1","tool":"write","path":1,"content_bytes":1,"fingerprint_v1":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#,
            r#"{"audit_version":"write_edit_v1","tool":"write","path":"a.txt","content_bytes":-1,"fingerprint_v1":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#,
            r#"{"audit_version":"write_edit_v1","tool":"write","path":"a.txt","content_bytes":1.5,"fingerprint_v1":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#,
            r#"{"audit_version":"write_edit_v1","tool":"write","path":"a.txt","content_bytes":18446744073709551616,"fingerprint_v1":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#,
            r#"{"audit_version":"write_edit_v1","tool":"write","path":"/SECRET_DATA_ROOT/../a.txt","content_bytes":1,"fingerprint_v1":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#,
            r#"{"audit_version":"write_edit_v1","tool":"write","path":"a.txt","content_bytes":1,"fingerprint_v1":"SECRET_HASH"}"#,
            r#"{"audit_version":"write_edit_v1","tool":"write","path":"a.txt","content_bytes":1,"fingerprint_v1":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"}"#,
            r#"{"path":"a.txt","content":"SECRET_RAW_BODY"}"#,
        ];
        for input_json in bad_write_inputs {
            let card = ToolCard::proposed(&ToolCall {
                id: "SECRET_CALL_ID".into(),
                tool: "write".into(),
                input_json: input_json.into(),
            });
            let visible = card.visible_text();
            assert!(
                visible.contains(CORRUPT_LABEL),
                "input was accepted: {input_json}"
            );
            assert!(!visible.contains("SECRET_CALL_ID"));
            assert!(!visible.contains("SECRET_DATA_ROOT"));
            assert!(!visible.contains("SECRET_HASH"));
            assert!(!visible.contains("SECRET_RAW_BODY"));
        }

        let bad_edit_inputs = [
            r#"{"audit_version":"write_edit_v1","tool":"edit","path":"a.txt","old_string_bytes":1,"new_string_bytes":2,"fingerprint_v1":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","extra":true}"#,
            r#"{"audit_version":"write_edit_v1","tool":"edit","path":"a.txt","old_string_bytes":"1","new_string_bytes":2,"fingerprint_v1":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#,
            r#"{"audit_version":"write_edit_v1","tool":"edit","path":"a.txt","old_string_bytes":-1,"new_string_bytes":2,"fingerprint_v1":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#,
            r#"{"audit_version":"write_edit_v1","tool":"edit","path":"a.txt","old_string_bytes":1.5,"new_string_bytes":2,"fingerprint_v1":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#,
            r#"{"audit_version":"write_edit_v1","tool":"edit","path":"a.txt","old_string_bytes":1,"new_string_bytes":18446744073709551616,"fingerprint_v1":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#,
        ];
        for input_json in bad_edit_inputs {
            let card = ToolCard::proposed(&ToolCall {
                id: "call".into(),
                tool: "edit".into(),
                input_json: input_json.into(),
            });
            assert!(card.visible_text().contains(CORRUPT_LABEL));
        }
    }

    #[test]
    fn mutation_success_projection_rejects_each_corrupt_output_field() {
        let write_call = ToolCall {
            id: "write-call".into(),
            tool: "write".into(),
            input_json: r#"{"audit_version":"write_edit_v1","tool":"write","path":"a.txt","content_bytes":1,"fingerprint_v1":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#.into(),
        };
        let bad_write_outputs = [
            r#"{}"#,
            r#"{"path":"a.txt","bytes_written":1,"checkpoint_ref":"preimage-v1/id-70/id-74/id-63","extra":true}"#,
            r#"{"path":1,"bytes_written":1,"checkpoint_ref":"preimage-v1/id-70/id-74/id-63"}"#,
            r#"{"path":"a.txt","bytes_written":-1,"checkpoint_ref":"preimage-v1/id-70/id-74/id-63"}"#,
            r#"{"path":"a.txt","bytes_written":1.5,"checkpoint_ref":"preimage-v1/id-70/id-74/id-63"}"#,
            r#"{"path":"a.txt","bytes_written":18446744073709551616,"checkpoint_ref":"preimage-v1/id-70/id-74/id-63"}"#,
            r#"{"path":"a.txt","bytes_written":1,"checkpoint_ref":1}"#,
            r#"{"path":"a.txt","bytes_written":1,"checkpoint_ref":"SECRET_CHECKPOINT_REF"}"#,
            r#"{"path":"other.txt","bytes_written":1,"checkpoint_ref":"preimage-v1/id-70/id-74/id-63"}"#,
            r#"{"path":"/SECRET_DATA_ROOT/../a.txt","bytes_written":1,"checkpoint_ref":"preimage-v1/id-70/id-74/id-63"}"#,
            r#"{"path":"a.txt","bytes_written":2,"checkpoint_ref":"preimage-v1/id-70/id-74/id-63"}"#,
        ];
        for output in bad_write_outputs {
            let mut card = ToolCard::proposed(&write_call);
            assert!(card.apply_approved(Approval::Once));
            let mut terminal = result(ToolCallStatus::Success, output);
            terminal.truncated = Some(false);
            assert!(
                !card.apply_finished(&terminal),
                "output was accepted: {output}"
            );
            let visible = card.visible_text();
            assert!(visible.contains(CORRUPT_LABEL));
            assert!(!visible.contains("SECRET_CHECKPOINT_REF"));
            assert!(!visible.contains("SECRET_DATA_ROOT"));
        }

        let edit_call = ToolCall {
            id: "edit-call".into(),
            tool: "edit".into(),
            input_json: r#"{"audit_version":"write_edit_v1","tool":"edit","path":"a.txt","old_string_bytes":1,"new_string_bytes":2,"fingerprint_v1":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#.into(),
        };
        let bad_edit_outputs = [
            r#"{}"#,
            r#"{"path":"a.txt","bytes_written":2,"replacements":1,"checkpoint_ref":"preimage-v1/id-70/id-74/id-63","extra":true}"#,
            r#"{"path":"a.txt","bytes_written":"2","replacements":1,"checkpoint_ref":"preimage-v1/id-70/id-74/id-63"}"#,
            r#"{"path":"a.txt","bytes_written":-1,"replacements":1,"checkpoint_ref":"preimage-v1/id-70/id-74/id-63"}"#,
            r#"{"path":"a.txt","bytes_written":1.5,"replacements":1,"checkpoint_ref":"preimage-v1/id-70/id-74/id-63"}"#,
            r#"{"path":"a.txt","bytes_written":18446744073709551616,"replacements":1,"checkpoint_ref":"preimage-v1/id-70/id-74/id-63"}"#,
            r#"{"path":"a.txt","bytes_written":2,"replacements":0,"checkpoint_ref":"preimage-v1/id-70/id-74/id-63"}"#,
            r#"{"path":"a.txt","bytes_written":2,"replacements":-1,"checkpoint_ref":"preimage-v1/id-70/id-74/id-63"}"#,
            r#"{"path":"a.txt","bytes_written":2,"replacements":1.5,"checkpoint_ref":"preimage-v1/id-70/id-74/id-63"}"#,
            r#"{"path":"a.txt","bytes_written":2,"replacements":18446744073709551616,"checkpoint_ref":"preimage-v1/id-70/id-74/id-63"}"#,
            r#"{"path":"a.txt","bytes_written":2,"replacements":1,"checkpoint_ref":"SECRET_CHECKPOINT_REF"}"#,
        ];
        for output in bad_edit_outputs {
            let mut card = ToolCard::proposed(&edit_call);
            assert!(card.apply_approved(Approval::Once));
            let mut terminal = result(ToolCallStatus::Success, output);
            terminal.truncated = Some(false);
            assert!(
                !card.apply_finished(&terminal),
                "output was accepted: {output}"
            );
            assert!(card.visible_text().contains(CORRUPT_LABEL));
        }
    }

    #[test]
    fn mutation_terminal_allowlist_and_invalid_projection_fail_closed() {
        let call = ToolCall {
            id: "write-call".into(),
            tool: "write".into(),
            input_json: r#"{"audit_version":"write_edit_v1","tool":"write","path":"a.txt","content_bytes":1,"fingerprint_v1":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#.into(),
        };

        let mut rejected = ToolCard::proposed(&call);
        assert!(rejected.apply_finished(&result(
            ToolCallStatus::Rejected,
            "Tool error: permission denied",
        )));
        assert!(!rejected.visible_text().contains("permission denied"));

        for (status, output) in [
            (ToolCallStatus::Failed, "Tool error: write failed"),
            (ToolCallStatus::Cancelled, "Tool error: tool worker failed"),
        ] {
            let mut card = ToolCard::proposed(&call);
            assert!(card.apply_approved(Approval::Once));
            assert!(card.apply_finished(&result(status, output)));
        }

        let mut corrupt = ToolCard::proposed(&call);
        assert!(corrupt.apply_approved(Approval::Once));
        assert!(
            !corrupt.apply_finished(&result(ToolCallStatus::Failed, "SECRET_RAW_FAILURE_BODY",))
        );
        assert!(corrupt.visible_text().contains(CORRUPT_LABEL));
        assert!(!corrupt.visible_text().contains("SECRET_RAW_FAILURE_BODY"));

        let forged_invalid = ToolResult {
            status: ToolCallStatus::Rejected,
            output: "SECRET_INVALID_BODY".into(),
            reused: false,
            exit_code: None,
            duration_ms: None,
            truncated: None,
            invalid: Some(InvalidToolProjection::new(
                InvalidToolKind::Write,
                InvalidToolCode::MalformedJson,
            )),
        };
        let card = ToolCard::invalid_terminal(&forged_invalid);
        assert!(card.visible_text().contains(CORRUPT_LABEL));
        assert!(!card.visible_text().contains("SECRET_INVALID_BODY"));

        let mut known = ToolCard::proposed(&call);
        assert!(!known.apply_finished(&ToolResult {
            output: "Tool error: invalid write input (malformed_json)".into(),
            ..forged_invalid
        }));
        assert!(known.visible_text().contains(CORRUPT_LABEL));
    }

    #[test]
    fn identical_terminal_only_is_idempotent_and_late_events_are_corrupt() {
        let call = ToolCall {
            id: "write-call".into(),
            tool: "write".into(),
            input_json: r#"{"audit_version":"write_edit_v1","tool":"write","path":"a.txt","content_bytes":1,"fingerprint_v1":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#.into(),
        };
        let mut terminal = result(
            ToolCallStatus::Success,
            r#"{"path":"a.txt","bytes_written":1,"checkpoint_ref":"preimage-v1/id-70/id-74/id-63"}"#,
        );
        terminal.truncated = Some(false);
        let mut card = ToolCard::proposed(&call);
        assert!(card.apply_approved(Approval::Once));
        assert!(card.apply_finished(&terminal));
        assert!(card.apply_finished(&terminal));
        assert!(!card.matches_call(&call));
        assert!(!card.apply_approved(Approval::Once));
        assert!(card.visible_text().contains(CORRUPT_LABEL));
    }

    #[test]
    fn long_bash_command_stays_one_compact_row_and_detail_keeps_the_full_command() {
        let command = format!("printf '{}{}'", "中".repeat(70), "a".repeat(90));
        let call = ToolCall {
            id: "SECRET_CALL_ID".into(),
            tool: "bash".into(),
            input_json: serde_json::json!({ "cmd": command }).to_string(),
        };
        let card = ToolCard::proposed(&call);
        assert!(!card.summary.contains('\n'));
        assert_eq!(card.row_count(), 1);
        assert_eq!(
            card.visible_text(),
            format!("Awaiting approval to run {command}")
        );
        assert!(card.visible_text().contains(&command));
        assert!(!card.visible_text().contains("SECRET_CALL_ID"));
        let mut card = card;
        card.expanded = true;
        assert!(card.visible_text().contains(&format!("$ {command}")));
    }

    #[test]
    fn expanded_bash_command_preserves_quoted_spaces_and_multiline_whitespace() {
        let command = "printf 'a  b'\nprintf\t'done'";
        let call = ToolCall {
            id: "bash-spacing".into(),
            tool: "bash".into(),
            input_json: serde_json::json!({ "cmd": command }).to_string(),
        };
        let mut card = ToolCard::proposed(&call);
        assert_eq!(
            card.summary,
            "Awaiting approval to run printf 'a  b' printf 'done'"
        );
        assert!(!card.summary.contains('\n'));
        assert!(!card.summary.contains('\r'));
        assert!(!card.summary.contains('\t'));
        card.expanded = true;
        assert!(card.visible_text().contains(&format!("$ {command}")));
    }

    #[test]
    fn late_approval_clears_expanded_bash_output_to_fixed_corrupt_card() {
        let call = ToolCall {
            id: "bash-call".into(),
            tool: "bash".into(),
            input_json: r#"{"cmd":"printf SECRET_COMMAND"}"#.into(),
        };
        let mut card = ToolCard::proposed(&call);
        assert!(card.apply_approved(Approval::Once));
        let mut terminal = result(ToolCallStatus::Success, "SECRET_BASH_OUTPUT");
        terminal.exit_code = Some(0);
        terminal.duration_ms = Some(1);
        terminal.truncated = Some(false);
        assert!(card.apply_finished(&terminal));
        card.expanded = true;
        assert!(card.visible_text().contains("SECRET_BASH_OUTPUT"));

        assert!(!card.apply_approved(Approval::Once));
        assert_eq!(card.visible_text(), "Tool result corrupted");
        assert_eq!(card.row_count(), 1);
        assert!(!card.visible_text().contains("SECRET_BASH_OUTPUT"));
        assert!(!card.visible_text().contains("SECRET_COMMAND"));
        assert!(!card.summary.contains("SECRET_COMMAND"));
        assert!(card.input.is_none());
        assert!(card.output_rows.is_empty());
        assert!(!card.expanded);
        assert!(card.permission_identity().is_none());
    }
}
