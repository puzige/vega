//! Audited tool-call activities over strict `vega_conversation` projections.

use crate::scroll_edge_fade::{ScrollEdgeFadeExt, ScrollFade};
use std::time::{Duration, Instant};

use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, Entity, MouseButton, MouseUpEvent, ScrollHandle, Task, div, px};
use vega_conversation::types::{
    Approval, CodexAcpActivityIdentity, CodexArgumentPreview, CodexDisplayMetadata,
    InvalidToolKind, ReadOnlyToolKind, SkillCardOutcome, SubagentOpenRequested, SubagentSnapshot,
    SubagentState, ToolCall, ToolCallStatus, ToolCardInputProjection, ToolCardResultProjection,
    ToolResult, tool_card_description, tool_card_input_projection, tool_card_result_projection,
};
use vega_theme::{Layout, ThemeColors, Typography, theme};

use crate::conversation_stream::{MONOFONT, ROW_HEIGHT};
use crate::icons::Icon;

const CORRUPT_LABEL: &str = "ui.tool_result_corrupted";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ToolActivityCategory {
    Shell,
    Question,
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

pub(crate) fn sentence_initial(phrase: &str) -> String {
    let mut characters = phrase.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().collect::<String>() + characters.as_str(),
        None => String::new(),
    }
}

impl ToolActivityCategory {
    pub(crate) fn action_phrase(self) -> &'static str {
        match self {
            Self::Question => vega_i18n::t("ui.ask_questions"),
            Self::Shell => vega_i18n::t("ui.run_commands"),
            Self::Read => vega_i18n::t("ui.read_files"),
            Self::Find => vega_i18n::t("ui.find_files"),
            Self::Search => vega_i18n::t("ui.search_content"),
            Self::Write => vega_i18n::t("ui.write_files"),
            Self::Edit => vega_i18n::t("ui.edit_files"),
            Self::Mcp => vega_i18n::t("ui.call_mcp"),
            Self::CodexAcp => "Codex ACP",
            Self::ComputerUse => vega_i18n::t("ui.use_computer_or_browser"),
            Self::Skill => vega_i18n::t("ui.use_skill"),
            Self::Other => vega_i18n::t("ui.use_tools"),
        }
    }

    pub(crate) const fn icon(self) -> Icon {
        match self {
            Self::Question => Icon::Help,
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
    output_copy: Option<String>,
    footer: Option<(String, ToolActivityState)>,
}

impl ToolDetail {
    fn logical_rows(&self) -> usize {
        usize::from(self.title.is_some())
            + usize::from(self.command.is_some())
            + self.output.len()
            + usize::from(self.footer.is_some())
    }
}

/// UI-only audited tool call. It never retains a provider call id, raw
/// write/edit input, fingerprint, checkpoint reference, or checkpoint path.
pub struct ToolCard {
    input: Option<ToolCardInputProjection>,
    description: Option<String>,
    status: ToolCallStatus,
    approval: Option<Approval>,
    result: Option<ToolCardResultProjection>,
    summary: String,
    summary_language: &'static str,
    output_rows: Vec<String>,
    expanded: bool,
    detail_scroll: ScrollHandle,
    running_started_at: Option<Instant>,
    running_elapsed_seconds: Option<u64>,
    elapsed_refresh_task: Option<Task<()>>,
    presentation_released: bool,
    execution: Option<SubagentSnapshot>,
}

impl gpui_kit::EventEmitter<SubagentOpenRequested> for ToolCard {}

impl ToolCard {
    /// Creates a pending card from a durable safe proposal.
    pub fn proposed(call: &ToolCall) -> Self {
        let input = tool_card_input_projection(call);
        let corrupt = matches!(input, ToolCardInputProjection::Corrupt);
        let mut card = Self {
            input: Some(input),
            description: tool_card_description(call),
            status: if corrupt {
                ToolCallStatus::Failed
            } else {
                ToolCallStatus::PendingApproval
            },
            approval: None,
            result: corrupt.then_some(ToolCardResultProjection::Corrupt),
            summary: String::new(),
            summary_language: vega_i18n::language(),
            output_rows: Vec::new(),
            expanded: false,
            detail_scroll: ScrollHandle::new(),
            running_started_at: None,
            running_elapsed_seconds: None,
            elapsed_refresh_task: None,
            presentation_released: false,
            execution: None,
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
            description: None,
            status,
            approval: None,
            result: Some(projection),
            summary: String::new(),
            summary_language: vega_i18n::language(),
            output_rows: Vec::new(),
            expanded: false,
            detail_scroll: ScrollHandle::new(),
            running_started_at: None,
            running_elapsed_seconds: None,
            elapsed_refresh_task: None,
            presentation_released: true,
            execution: None,
        };
        card.refresh_summary();
        card
    }

    /// Fixed content-free corrupt card for an unknown or illegal transition.
    pub fn corrupt() -> Self {
        let mut card = Self {
            input: None,
            description: None,
            status: ToolCallStatus::Failed,
            approval: None,
            result: Some(ToolCardResultProjection::Corrupt),
            summary: String::new(),
            summary_language: vega_i18n::language(),
            output_rows: Vec::new(),
            expanded: false,
            detail_scroll: ScrollHandle::new(),
            running_started_at: None,
            running_elapsed_seconds: None,
            elapsed_refresh_task: None,
            presentation_released: true,
            execution: None,
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
        description: Option<String>,
    ) -> Self {
        let mut card = Self {
            output_rows: result
                .as_ref()
                .map(projection_output_rows)
                .unwrap_or_default(),
            input,
            description,
            status,
            approval,
            result,
            summary: String::new(),
            summary_language: vega_i18n::language(),
            expanded: false,
            detail_scroll: ScrollHandle::new(),
            running_started_at: None,
            running_elapsed_seconds: None,
            elapsed_refresh_task: None,
            presentation_released: true,
            execution: None,
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

    pub(crate) fn matches_pending_user_question(&self, call: &ToolCall) -> bool {
        matches!(
            self.status,
            ToolCallStatus::PendingApproval | ToolCallStatus::Approved | ToolCallStatus::Running
        ) && self.result.is_none()
            && self.input.as_ref().is_some_and(|input| {
                matches!(
                    input,
                    ToolCardInputProjection::UserQuestion { input: Ok(_) }
                ) && input == &tool_card_input_projection(call)
            })
    }

    /// Converts an illegal transition to the fixed corrupt state.
    pub fn fail_corrupt(&mut self, cx: &mut gpui_kit::Context<Self>) {
        self.set_corrupt();
        cx.notify();
    }

    fn set_corrupt(&mut self) {
        self.stop_live_elapsed();
        self.input = None;
        self.description = None;
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
        if !self.presentation_released && self.is_bash() && self.running_started_at.is_none() {
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
                    || (self.is_subagent_invocation()
                        && result.status == ToolCallStatus::Failed
                        && self.status == ToolCallStatus::PendingApproval)
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
        let tool = match input {
            ToolCardInputProjection::ReadOnly {
                tool: ReadOnlyToolKind::Grep,
                permission_path: Some(_),
                ..
            } => "read",
            _ => input.tool()?,
        };
        Some((tool, input.permission_target()?))
    }

    pub(crate) fn command_scope_identity(&self) -> Option<(&str, &str)> {
        if !matches!(
            self.status,
            ToolCallStatus::Approved | ToolCallStatus::Running
        ) || self.approval.is_none()
            || self.result.is_some()
        {
            return None;
        }
        let input = self.input.as_ref()?;
        (input.tool()? == "bash").then_some(("bash", input.permission_target()?))
    }

    pub(crate) fn codex_permission_display(
        &self,
    ) -> Option<(CodexAcpActivityIdentity, Option<CodexArgumentPreview>)> {
        match &self.input {
            Some(ToolCardInputProjection::CodexAcp {
                identity, preview, ..
            }) => Some((identity.clone(), *preview)),
            _ => None,
        }
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

    pub(crate) fn activity_category(&self) -> ToolActivityCategory {
        match (&self.input, &self.result) {
            (_, Some(ToolCardResultProjection::Corrupt)) => ToolActivityCategory::Other,
            (Some(ToolCardInputProjection::UserQuestion { .. }), _) => {
                ToolActivityCategory::Question
            }
            (Some(ToolCardInputProjection::ReadOnly { tool, .. }), _) => match tool {
                ReadOnlyToolKind::Read | ReadOnlyToolKind::ConversationHistory => {
                    ToolActivityCategory::Read
                }
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
            Some(ToolCardInputProjection::UserQuestion { .. }) => ToolActivityCategory::Question,
            Some(ToolCardInputProjection::ReadOnly { tool, .. }) => match tool {
                ReadOnlyToolKind::Read | ReadOnlyToolKind::ConversationHistory => {
                    ToolActivityCategory::Read
                }
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
            Some(ToolCardInputProjection::Subagent { .. })
            | Some(ToolCardInputProjection::Automation { .. })
            | Some(ToolCardInputProjection::Corrupt) => ToolActivityCategory::Other,
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

    pub(crate) fn is_running(&self) -> bool {
        self.status == ToolCallStatus::Running && self.approval.is_some() && self.result.is_none()
    }

    pub(crate) fn headline_title(&self) -> String {
        if self.summary_language == vega_i18n::language() {
            self.summary.clone()
        } else {
            self.build_summary()
        }
    }

    pub(crate) fn description_headline(&self) -> Option<(String, Icon)> {
        self.description_title()
            .map(|title| (title, self.leading_icon()))
    }

    pub(crate) fn completed_command_headline(&self) -> Option<String> {
        (!self.presentation_released
            && self.is_bash()
            && matches!(
                self.status,
                ToolCallStatus::Success | ToolCallStatus::Failed | ToolCallStatus::Cancelled
            )
            && matches!(
                &self.result,
                Some(ToolCardResultProjection::Bash { status, .. }) if *status == self.status
            ))
        .then(|| self.headline_title())
    }

    pub(crate) fn is_expanded(&self) -> bool {
        self.expanded
    }

    pub(crate) fn compact_title(&self) -> String {
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
            return self.headline_title();
        }
        let (label_key, duration_key) = match self.activity_state() {
            ToolActivityState::Success => ("ui.ran_command", "ui.command_success_duration"),
            ToolActivityState::Failed => ("ui.command_failed", "ui.command_failed_duration"),
            ToolActivityState::Cancelled => {
                ("ui.command_cancelled", "ui.command_cancelled_duration")
            }
            ToolActivityState::Rejected => ("ui.command_rejected", "ui.command_rejected_duration"),
            ToolActivityState::Superseded => (
                "ui.not_run_after_takeover",
                "ui.command_superseded_duration",
            ),
            ToolActivityState::Active if self.status == ToolCallStatus::PendingApproval => (
                "ui.awaiting_command_approval",
                "ui.command_approval_duration",
            ),
            ToolActivityState::Active if self.status == ToolCallStatus::Approved => {
                ("ui.ready_to_run_command", "ui.command_ready_duration")
            }
            ToolActivityState::Active => ("ui.running_command", "ui.command_running_duration"),
        };
        self.bash_duration().map_or_else(
            || vega_i18n::t(label_key).to_owned(),
            |duration| vega_i18n::t_args(duration_key, &[("duration", &duration)]),
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
        indented: bool,
        window: &mut gpui_kit::Window,
        cx: &mut App,
    ) -> AnyElement {
        Self::render_with_title(card, indented, None, window, cx)
    }

    pub(crate) fn render_with_title(
        card: Entity<Self>,
        indented: bool,
        headline: Option<(String, AnyElement)>,
        window: &mut gpui_kit::Window,
        cx: &mut App,
    ) -> AnyElement {
        if card.read(cx).is_subagent_invocation() {
            return Self::render_invocation(card, indented, headline, window, cx);
        }
        let colors = theme(cx).colors;
        let (title, leading_icon, leading_icon_color, expanded, expandable, detail) = {
            let card_ref = card.read(cx);
            let expanded = card_ref.expanded;
            let title = card_ref.compact_title();
            (
                title,
                card_ref.leading_icon(),
                card_ref.leading_icon_color(&colors),
                expanded,
                card_ref.has_detail(),
                if expanded { card_ref.detail() } else { None },
            )
        };
        let (title, title_element) =
            headline.unwrap_or_else(|| (title.clone(), div().child(title).into_any_element()));
        let detail_scroll = card.read(cx).detail_scroll.clone();
        let detail_id = format!("tool-detail-{}", card.entity_id());
        let toggle_card = card.clone();
        let keyboard_card = card.clone();
        let disclosure_group: gpui_kit::SharedString =
            format!("tool-disclosure-{}", card.entity_id()).into();
        let focus =
            crate::disclosure::focus(disclosure_group.clone(), window, cx).tab_stop(expandable);
        let row = div()
            .id(("tool-activity-toggle", card.entity_id()))
            .group(disclosure_group.clone());
        let row = row
            .track_focus(&focus)
            .aria_expanded(expanded)
            .aria_label(vega_i18n::t_args(
                "ui.tool_disclosure",
                &[
                    ("title", &title),
                    (
                        "action",
                        vega_i18n::t(if expanded { "ui.collapse" } else { "ui.expand" }),
                    ),
                ],
            ))
            .tab_stop(expandable)
            .focus_visible(move |row| row.bg(colors.bg_hover))
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
                    .min_w_0()
                    .flex_shrink(1.)
                    .truncate()
                    .text_size(px(Typography::CONVERSATION))
                    .text_color(colors.conversation_tool)
                    .child(title_element),
            )
            .child(crate::disclosure::inline_chevron(
                disclosure_group,
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
                    detail_id,
                    &detail_scroll,
                    &colors,
                    cx,
                ))
            })
            .into_any_element()
    }

    pub(crate) fn is_subagent_invocation(&self) -> bool {
        matches!(&self.input, Some(ToolCardInputProjection::Subagent { tool, .. }) if matches!(tool.as_str(), "spawn_agent" | "followup_agent"))
    }

    pub(crate) fn subagent(&self) -> Option<&SubagentSnapshot> {
        self.execution.as_ref().or(match &self.result {
            Some(ToolCardResultProjection::Subagent { agent, .. }) => agent.as_deref(),
            _ => None,
        })
    }

    pub(crate) fn apply_subagent(
        &mut self,
        snapshot: SubagentSnapshot,
        cx: &mut gpui_kit::Context<Self>,
    ) {
        if self.subagent().is_some_and(|current| {
            current.agent_id == snapshot.agent_id && current.message_id == snapshot.message_id
        }) {
            self.execution = Some(snapshot);
            cx.notify();
        }
    }

    fn render_invocation(
        card: Entity<Self>,
        indented: bool,
        headline: Option<(String, AnyElement)>,
        window: &mut gpui_kit::Window,
        cx: &mut App,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let current = card.read(cx);
        let agent = current.subagent().cloned();
        let title = current
            .description_title()
            .unwrap_or_else(|| match (&current.input, &agent) {
                (Some(ToolCardInputProjection::Subagent { tool, .. }), Some(agent)) => {
                    vega_i18n::t_args(
                        "ui.action_name",
                        &[
                            (
                                "action",
                                if tool == "spawn_agent" {
                                    vega_i18n::t("ui.delegate")
                                } else {
                                    vega_i18n::t("ui.continue")
                                },
                            ),
                            ("name", &agent.name),
                        ],
                    )
                }
                (Some(ToolCardInputProjection::Subagent { title, .. }), _) => title.clone(),
                _ => vega_i18n::t("ui.delegate_task").into(),
            });
        let (title, title_element) =
            headline.unwrap_or_else(|| (title.clone(), div().child(title).into_any_element()));
        let status = agent.as_ref().map_or_else(
            || match current.status {
                ToolCallStatus::PendingApproval => vega_i18n::t("ui.pending"),
                ToolCallStatus::Approved => vega_i18n::t("ui.ready"),
                ToolCallStatus::Running => vega_i18n::t("ui.running"),
                ToolCallStatus::Cancelled => vega_i18n::t("ui.interrupted"),
                ToolCallStatus::Rejected => vega_i18n::t("ui.rejected"),
                ToolCallStatus::Failed => vega_i18n::t("ui.failed"),
                ToolCallStatus::Success => vega_i18n::t("ui.failed"),
            },
            |agent| match agent.state {
                SubagentState::Running => vega_i18n::t("ui.running"),
                SubagentState::Cancelling => vega_i18n::t("ui.stopping"),
                SubagentState::Completed => vega_i18n::t("ui.completed"),
                SubagentState::Failed => vega_i18n::t("ui.failed"),
                SubagentState::Interrupted => vega_i18n::t("ui.interrupted"),
            },
        );
        let failed = agent.as_ref().map_or_else(
            || {
                matches!(
                    current.status,
                    ToolCallStatus::Failed | ToolCallStatus::Success
                )
            },
            |agent| agent.state == SubagentState::Failed,
        );
        let error_detail = agent.is_none() && !current.output_rows.is_empty();
        let expanded = error_detail && current.expanded;
        let detail = expanded.then(|| ToolDetail {
            title: None,
            command: None,
            output: current.output_rows.clone(),
            output_copy: None,
            footer: None,
        });
        let detail_scroll = current.detail_scroll.clone();
        let detail_id = format!("tool-detail-{}", card.entity_id());
        let enabled = agent.is_some() || error_detail;
        let group: gpui_kit::SharedString =
            format!("subagent-invocation-{}", card.entity_id()).into();
        let focus = crate::disclosure::focus(group, window, cx).tab_stop(enabled);
        let keyboard = card.clone();
        let keyboard_agent = agent.clone();
        let row = div()
            .id(("subagent-invocation", card.entity_id()))
            .role(gpui_kit::Role::Button)
            .aria_label(vega_i18n::t_args(
                "ui.title_status",
                &[("title", &title), ("status", status)],
            ))
            .when(error_detail, |row| row.aria_expanded(expanded))
            .track_focus(&focus)
            .tab_stop(enabled)
            .focus_visible(move |row| row.bg(colors.bg_hover))
            .h(px(Layout::CONVERSATION_TOOL_HEIGHT))
            .w_full()
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(Layout::CONVERSATION_INLINE_GAP))
            .text_size(px(Typography::CONVERSATION))
            .line_height(gpui_kit::relative(Typography::CONVERSATION_LINE_HEIGHT))
            .when(indented, |row| row.pl_4())
            .when(enabled, |row| row.cursor_pointer())
            .on_click(move |_, _, cx| {
                if let Some(agent) = &agent {
                    card.update(cx, |_, cx| {
                        cx.emit(SubagentOpenRequested {
                            source_thread_id: agent.parent_thread_id.clone(),
                            thread_id: agent.agent_id.clone(),
                        })
                    });
                    cx.stop_propagation();
                } else if error_detail {
                    card.update(cx, |card, cx| {
                        card.expanded = !card.expanded;
                        cx.notify();
                    });
                    cx.stop_propagation();
                }
            })
            .on_key_down(move |event, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    if let Some(agent) = &keyboard_agent {
                        keyboard.update(cx, |_, cx| {
                            cx.emit(SubagentOpenRequested {
                                source_thread_id: agent.parent_thread_id.clone(),
                                thread_id: agent.agent_id.clone(),
                            })
                        });
                        cx.stop_propagation();
                    } else if error_detail {
                        keyboard.update(cx, |card, cx| {
                            card.expanded = !card.expanded;
                            cx.notify();
                        });
                        cx.stop_propagation();
                    }
                }
            })
            .child(crate::icons::sized_icon(
                Icon::Bot,
                colors.text_secondary,
                Layout::ICON_SIZE,
            ))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_color(colors.conversation_tool)
                    .child(title_element),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .text_color(if failed {
                        colors.danger
                    } else {
                        colors.text_secondary
                    })
                    .child(status),
            )
            .when(error_detail, |row| {
                row.child(crate::icons::sized_icon(
                    if expanded {
                        Icon::ChevronDown
                    } else {
                        Icon::ChevronRight
                    },
                    colors.text_secondary,
                    Layout::ICON_SIZE,
                ))
            });
        div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .child(row)
            .when_some(detail, |view, detail| {
                view.child(render_detail(
                    detail,
                    detail_id,
                    &detail_scroll,
                    &colors,
                    cx,
                ))
            })
            .into_any_element()
    }

    fn description_title(&self) -> Option<String> {
        if matches!(self.input, None | Some(ToolCardInputProjection::Corrupt))
            || matches!(
                self.result,
                Some(
                    ToolCardResultProjection::Corrupt
                        | ToolCardResultProjection::InvalidRejected { .. }
                )
            )
        {
            return None;
        }
        let description = self.description.as_deref()?;
        let status_key = match self.activity_state() {
            ToolActivityState::Success => return Some(description.to_string()),
            ToolActivityState::Active if self.status != ToolCallStatus::PendingApproval => {
                return Some(description.to_string());
            }
            ToolActivityState::Active => "ui.awaiting_approval",
            ToolActivityState::Failed => "ui.failed",
            ToolActivityState::Cancelled => "ui.cancelled",
            ToolActivityState::Rejected => "ui.rejected",
            ToolActivityState::Superseded => "ui.not_run_after_takeover",
        };
        Some(vega_i18n::t_args(
            "ui.command_status_description",
            &[
                ("status", vega_i18n::t(status_key)),
                ("description", description),
            ],
        ))
    }

    fn refresh_summary(&mut self) {
        self.summary = self.build_summary();
        self.summary_language = vega_i18n::language();
    }

    fn build_summary(&self) -> String {
        if let Some(title) = self.description_title() {
            return title;
        }
        match (&self.input, &self.result) {
            (_, Some(ToolCardResultProjection::Corrupt)) => vega_i18n::t(CORRUPT_LABEL).to_string(),
            (
                _,
                Some(ToolCardResultProjection::InvalidRejected {
                    tool: InvalidToolKind::Bash,
                    ..
                }),
            ) => vega_i18n::t("ui.command_rejected_use_a_non_empty_command_and").to_string(),
            (_, Some(ToolCardResultProjection::InvalidRejected { tool, code, .. })) => {
                vega_i18n::t_args(
                    "ui.rejected_action_code",
                    &[("action", invalid_action(*tool)), ("code", code.as_str())],
                )
            }
            (Some(ToolCardInputProjection::Bash { .. }), _) => self.build_bash_summary(),
            (
                Some(ToolCardInputProjection::ReadOnly {
                    tool,
                    display_target,
                    ..
                }),
                _,
            ) => {
                let summary = readonly_summary(
                    *tool,
                    self.activity_state(),
                    self.status,
                    display_target.is_some(),
                );
                display_target.as_ref().map_or(summary.clone(), |target| {
                    vega_i18n::t_args(
                        "ui.summary_target",
                        &[("summary", &summary), ("target", &compact_object(target))],
                    )
                })
            }
            (
                Some(ToolCardInputProjection::Write {
                    path,
                    content_bytes,
                }),
                None,
            ) => vega_i18n::t_args(
                "ui.write_action_bytes",
                &[
                    (
                        "action",
                        &mutation_verb("write", self.activity_state(), self.status),
                    ),
                    ("path", path),
                    (
                        "content_bytes",
                        &crate::file_size::format_file_size(*content_bytes),
                    ),
                ],
            ),
            (
                Some(ToolCardInputProjection::Edit {
                    path,
                    old_string_bytes,
                    new_string_bytes,
                    ..
                }),
                None,
            ) => vega_i18n::t_args(
                "ui.edit_action_bytes",
                &[
                    (
                        "action",
                        &mutation_verb("edit", self.activity_state(), self.status),
                    ),
                    ("path", path),
                    ("old_string_bytes", &old_string_bytes.to_string()),
                    ("new_string_bytes", &new_string_bytes.to_string()),
                ],
            ),
            (
                _,
                Some(ToolCardResultProjection::WriteSuccess {
                    path,
                    bytes_written,
                    ..
                }),
            ) => vega_i18n::t_args(
                "ui.wrote_path_bytes_written_bytes",
                &[
                    ("path", path),
                    (
                        "bytes_written",
                        &crate::file_size::format_file_size(*bytes_written),
                    ),
                ],
            ),
            (
                _,
                Some(ToolCardResultProjection::EditSuccess {
                    path,
                    bytes_written,
                    replacements,
                    ..
                }),
            ) => vega_i18n::t_args(
                "ui.edited_path_bytes_written_bytes_replacements_replacements",
                &[
                    ("path", path),
                    ("bytes_written", &bytes_written.to_string()),
                    ("replacements", &replacements.to_string()),
                ],
            ),
            (
                Some(ToolCardInputProjection::Write { path, .. }),
                Some(ToolCardResultProjection::MutationTerminal { .. }),
            ) => vega_i18n::t_args(
                "ui.action_path",
                &[
                    (
                        "action",
                        &mutation_verb("write", self.activity_state(), self.status),
                    ),
                    ("path", path),
                ],
            ),
            (
                Some(ToolCardInputProjection::Edit { path, .. }),
                Some(ToolCardResultProjection::MutationTerminal { .. }),
            ) => vega_i18n::t_args(
                "ui.action_path",
                &[
                    (
                        "action",
                        &mutation_verb("edit", self.activity_state(), self.status),
                    ),
                    ("path", path),
                ],
            ),
            (
                Some(ToolCardInputProjection::Skill { name, .. }),
                Some(ToolCardResultProjection::Skill { outcome, .. }),
            ) => skill_summary(name.as_deref(), outcome),
            (Some(ToolCardInputProjection::Skill { name, .. }), None) => vega_i18n::t_args(
                "ui.action_skill",
                &[
                    (
                        "action",
                        &generic_verb("use", self.activity_state(), self.status),
                    ),
                    (
                        "name",
                        name.as_deref()
                            .unwrap_or_else(|| vega_i18n::t("ui.unknown")),
                    ),
                ],
            ),
            (Some(ToolCardInputProjection::UserQuestion { .. }), _) => match self.status {
                ToolCallStatus::Success => vega_i18n::t("ui.questions_completed").into(),
                ToolCallStatus::Cancelled => vega_i18n::t("ui.questions_interrupted").into(),
                ToolCallStatus::Failed | ToolCallStatus::Rejected => {
                    vega_i18n::t("ui.question_failed").into()
                }
                _ => vega_i18n::t("ui.waiting_for_your_answer").into(),
            },
            (Some(ToolCardInputProjection::ComputerUse { tool, action }), _) => {
                if self.activity_state() == ToolActivityState::Superseded {
                    return vega_i18n::t("ui.not_run_after_takeover").into();
                }
                let verb = match tool.as_str() {
                    "ComputerObserve" | "BrowserObserve" => "observe",
                    "ComputerScreenshot" => "capture",
                    _ => "control",
                };
                vega_i18n::t_args(
                    "ui.computer_action",
                    &[
                        (
                            "action",
                            &generic_verb(verb, self.activity_state(), self.status),
                        ),
                        (
                            "target",
                            if tool.starts_with("Computer") {
                                vega_i18n::t("ui.computer")
                            } else {
                                vega_i18n::t("ui.browser")
                            },
                        ),
                        ("operation", action),
                        (
                            "approval",
                            if verb == "control" && self.approval == Some(Approval::Once) {
                                vega_i18n::t("ui.approved_for_this_action")
                            } else {
                                ""
                            },
                        ),
                    ],
                )
            }
            (Some(ToolCardInputProjection::Mcp { identity, .. }), _) => vega_i18n::t_args(
                "ui.mcp_action",
                &[
                    (
                        "action",
                        &generic_verb("call", self.activity_state(), self.status),
                    ),
                    ("tool", &identity.exact_tool_name),
                    ("server", &identity.server_id),
                ],
            ),
            (Some(ToolCardInputProjection::Subagent { title, .. }), _) => title.clone(),
            (Some(ToolCardInputProjection::Automation { tool }), _) => {
                vega_i18n::t(match tool.as_str() {
                    "get_goal" => "ui.read_task_goal",
                    "update_goal" => "ui.update_task_goal",
                    _ => "ui.schedule_next_run",
                })
                .to_string()
            }
            (Some(ToolCardInputProjection::CodexAcp { identity, .. }), _) => vega_i18n::t_args(
                "ui.codex_action",
                &[
                    (
                        "action",
                        &generic_verb("run", self.activity_state(), self.status),
                    ),
                    ("kind", codex_kind_label(&identity.kind)),
                ],
            ),
            _ => vega_i18n::t(CORRUPT_LABEL).to_string(),
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
            ToolActivityState::Superseded => vega_i18n::t("ui.not_run_after_takeover"),
            ToolActivityState::Success => vega_i18n::t("ui.ran"),
            ToolActivityState::Active if self.status == ToolCallStatus::PendingApproval => {
                vega_i18n::t("ui.awaiting_approval_to_run")
            }
            ToolActivityState::Active if self.status == ToolCallStatus::Approved => {
                vega_i18n::t("ui.ready_to_run")
            }
            ToolActivityState::Active => vega_i18n::t("ui.running"),
            ToolActivityState::Rejected => vega_i18n::t("ui.command_rejected_en"),
            ToolActivityState::Cancelled => vega_i18n::t("ui.command_cancelled_en"),
            ToolActivityState::Failed => vega_i18n::t("ui.command_failed_en"),
        };
        let Some(ToolCardInputProjection::Bash { command }) = &self.input else {
            return action.into();
        };
        vega_i18n::t_args(
            "ui.command_action",
            &[("action", action), ("command", &compact_object(command))],
        )
    }

    pub(crate) fn release_presentation(&mut self) {
        self.presentation_released = true;
        self.elapsed_refresh_task = None;
    }

    fn stop_live_elapsed(&mut self) {
        self.running_started_at = None;
        self.running_elapsed_seconds = None;
        self.elapsed_refresh_task = None;
    }

    fn bash_detail_title(&self) -> String {
        let mut title = vega_i18n::t("ui.shell").to_string();
        if let Some(ToolCardResultProjection::Bash {
            truncated, reused, ..
        }) = &self.result
        {
            if *truncated == Some(true) {
                title.push_str(vega_i18n::t("ui.truncated"));
            }
            if *reused {
                title.push_str(vega_i18n::t("ui.reused"));
            }
        }
        title
    }

    fn terminal_footer(&self) -> String {
        match self.activity_state() {
            ToolActivityState::Superseded => vega_i18n::t("ui.not_run_after_takeover"),
            ToolActivityState::Success => vega_i18n::t("ui.success"),
            ToolActivityState::Active if self.status == ToolCallStatus::PendingApproval => {
                vega_i18n::t("ui.awaiting_approval")
            }
            ToolActivityState::Active if self.status == ToolCallStatus::Approved => {
                vega_i18n::t("ui.ready_to_run")
            }
            ToolActivityState::Active => vega_i18n::t("ui.running"),
            ToolActivityState::Rejected => vega_i18n::t("ui.rejected"),
            ToolActivityState::Cancelled => vega_i18n::t("ui.cancelled"),
            ToolActivityState::Failed => vega_i18n::t("ui.failure"),
        }
        .to_string()
    }

    fn has_detail(&self) -> bool {
        if self.description.is_some() && self.description_detail().is_some() {
            return true;
        }
        match &self.input {
            Some(ToolCardInputProjection::Bash { .. }) => true,
            Some(ToolCardInputProjection::Subagent { .. })
            | Some(ToolCardInputProjection::UserQuestion { .. })
            | Some(ToolCardInputProjection::ReadOnly { .. })
            | Some(ToolCardInputProjection::Mcp { .. })
            | Some(ToolCardInputProjection::ComputerUse { .. })
            | Some(ToolCardInputProjection::CodexAcp { .. })
            | Some(ToolCardInputProjection::Automation { .. }) => !self.output_rows.is_empty(),
            _ => false,
        }
    }

    fn detail_row_count(&self) -> usize {
        if self.description.is_some()
            && let Some(detail) = self.description_detail()
        {
            return detail.logical_rows();
        }
        match &self.input {
            Some(ToolCardInputProjection::Bash { .. }) => 3 + self.output_rows.len(),
            Some(ToolCardInputProjection::Subagent { .. })
            | Some(ToolCardInputProjection::UserQuestion { .. })
            | Some(ToolCardInputProjection::ReadOnly { .. })
            | Some(ToolCardInputProjection::Mcp { .. })
            | Some(ToolCardInputProjection::ComputerUse { .. })
            | Some(ToolCardInputProjection::CodexAcp { .. })
            | Some(ToolCardInputProjection::Automation { .. }) => self.output_rows.len(),
            _ => 0,
        }
    }

    fn description_detail(&self) -> Option<ToolDetail> {
        let target = match &self.input {
            Some(ToolCardInputProjection::ReadOnly { detail_target, .. }) => detail_target.clone(),
            Some(
                ToolCardInputProjection::Write { path, .. }
                | ToolCardInputProjection::Edit { path, .. },
            ) => Some(path.clone()),
            Some(ToolCardInputProjection::Skill { name, .. }) => name.clone(),
            _ => None,
        }?;
        Some(ToolDetail {
            title: None,
            command: Some(target),
            output: self.output_rows.clone(),
            output_copy: None,
            footer: Some((self.terminal_footer(), self.activity_state())),
        })
    }

    fn detail(&self) -> Option<ToolDetail> {
        if self.description.is_some()
            && let Some(detail) = self.description_detail()
        {
            return Some(detail);
        }
        match &self.input {
            Some(ToolCardInputProjection::Bash { command, .. }) => Some(ToolDetail {
                title: Some(self.bash_detail_title()),
                command: Some(command.clone()),
                output: if self.output_rows.is_empty()
                    && self.activity_state() != ToolActivityState::Active
                {
                    vec![vega_i18n::t("ui.no_output").to_string()]
                } else {
                    self.output_rows.clone()
                },
                output_copy: match &self.result {
                    Some(ToolCardResultProjection::Bash { output, .. }) if !output.is_empty() => {
                        Some(output.clone())
                    }
                    _ => None,
                },
                footer: Some((self.terminal_footer(), self.activity_state())),
            }),
            Some(
                ToolCardInputProjection::UserQuestion { .. }
                | ToolCardInputProjection::ReadOnly { .. }
                | ToolCardInputProjection::Automation { .. },
            ) if !self.output_rows.is_empty() => Some(ToolDetail {
                title: None,
                command: None,
                output: self.output_rows.clone(),
                output_copy: None,
                footer: None,
            }),
            Some(
                ToolCardInputProjection::Mcp { .. } | ToolCardInputProjection::ComputerUse { .. },
            ) if !self.output_rows.is_empty() => Some(ToolDetail {
                title: None,
                command: None,
                output: self.output_rows.clone(),
                output_copy: None,
                footer: None,
            }),
            Some(ToolCardInputProjection::CodexAcp { .. }) if !self.output_rows.is_empty() => {
                Some(ToolDetail {
                    title: Some("Codex ACP".to_string()),
                    command: None,
                    output: match &self.result {
                        Some(ToolCardResultProjection::CodexAcp {
                            reason: Some(reason),
                            ..
                        }) => {
                            vec![reason.message().to_owned()]
                        }
                        _ => self.output_rows.clone(),
                    },
                    output_copy: None,
                    footer: None,
                })
            }
            _ => None,
        }
    }
}

pub(crate) fn codex_identity_display(
    identity: &CodexAcpActivityIdentity,
    preview: Option<CodexArgumentPreview>,
) -> String {
    let preview = preview
        .filter(|preview| {
            CodexDisplayMetadata {
                preview: *preview,
                reason: None,
            }
            .validated_preview(identity)
            .is_some()
        })
        .map(|preview| preview.message())
        .unwrap_or_else(|| identity.argument_preview.clone());
    vega_i18n::t_args(
        "ui.codex.identity",
        &[
            ("kind", codex_kind_label(&identity.kind)),
            ("bytes", &identity.arguments_bytes.to_string()),
            ("sha256", &identity.arguments_sha256),
            ("preview", &preview),
        ],
    )
}

fn codex_kind_label(kind: &str) -> &str {
    let key = match kind {
        "read" => "ui.codex.kind.read",
        "edit" => "ui.codex.kind.edit",
        "delete" => "ui.codex.kind.delete",
        "move" => "ui.codex.kind.move",
        "search" => "ui.codex.kind.search",
        "execute" => "ui.codex.kind.execute",
        "think" => "ui.codex.kind.think",
        "fetch" => "ui.codex.kind.fetch",
        "other" => "ui.codex.kind.other",
        _ => return kind,
    };
    vega_i18n::t(key)
}

fn action_label(action: &'static str) -> &'static str {
    match action {
        "run" => vega_i18n::t("ui.action_run"),
        "write" => vega_i18n::t("ui.action_write"),
        "edit" => vega_i18n::t("ui.action_edit"),
        "call" => vega_i18n::t("ui.action_call"),
        "use" => vega_i18n::t("ui.action_use"),
        "observe" => vega_i18n::t("ui.action_observe"),
        "capture" => vega_i18n::t("ui.action_capture"),
        "control" => vega_i18n::t("ui.action_control"),
        "read" => vega_i18n::t("ui.action_read"),
        "search" => vega_i18n::t("ui.action_search"),
        "read files" => vega_i18n::t("ui.read_files"),
        "read conversation" => vega_i18n::t("ui.read_conversation"),
        "find files" => vega_i18n::t("ui.find_files"),
        "search content" => vega_i18n::t("ui.search_content"),
        _ => action,
    }
}

fn invalid_action(tool: InvalidToolKind) -> &'static str {
    match tool {
        InvalidToolKind::Bash => vega_i18n::t("ui.action_run"),
        InvalidToolKind::Write => vega_i18n::t("ui.action_write"),
        InvalidToolKind::Edit => vega_i18n::t("ui.action_edit"),
    }
}

fn generic_verb(action: &'static str, state: ToolActivityState, status: ToolCallStatus) -> String {
    let (past, ongoing) = match action {
        "write" => (vega_i18n::t("ui.wrote"), vega_i18n::t("ui.writing")),
        "edit" => (vega_i18n::t("ui.edited"), vega_i18n::t("ui.editing")),
        "call" => (vega_i18n::t("ui.called"), vega_i18n::t("ui.calling")),
        "run" => (vega_i18n::t("ui.ran"), vega_i18n::t("ui.running")),
        "use" => (vega_i18n::t("ui.used"), vega_i18n::t("ui.using")),
        "observe" => (vega_i18n::t("ui.observed"), vega_i18n::t("ui.observing")),
        "capture" => (vega_i18n::t("ui.captured"), vega_i18n::t("ui.capturing")),
        "control" => (
            vega_i18n::t("ui.controlled"),
            vega_i18n::t("ui.controlling"),
        ),
        "read" => (vega_i18n::t("ui.read"), vega_i18n::t("ui.reading")),
        "read files" => (
            vega_i18n::t("ui.read_files_en"),
            vega_i18n::t("ui.reading_files"),
        ),
        "read conversation" => (
            vega_i18n::t("ui.read_conversation_done"),
            vega_i18n::t("ui.reading_conversation"),
        ),
        "find files" => (
            vega_i18n::t("ui.found_files"),
            vega_i18n::t("ui.finding_files"),
        ),
        "search" => (vega_i18n::t("ui.searched"), vega_i18n::t("ui.searching_en")),
        "search content" => (
            vega_i18n::t("ui.searched_content"),
            vega_i18n::t("ui.searching_content"),
        ),
        _ => (action, action),
    };
    let action = action_label(action);
    match state {
        ToolActivityState::Superseded => vega_i18n::t("ui.not_run_after_takeover").to_string(),
        ToolActivityState::Success => past.to_owned(),
        ToolActivityState::Active if status == ToolCallStatus::PendingApproval => {
            vega_i18n::t_args("ui.awaiting_approval_to_action", &[("action", action)])
        }
        ToolActivityState::Active if status == ToolCallStatus::Approved => {
            vega_i18n::t_args("ui.ready_to_action", &[("action", action)])
        }
        ToolActivityState::Active => ongoing.to_owned(),
        ToolActivityState::Rejected => {
            vega_i18n::t_args("ui.rejected_action", &[("action", action)])
        }
        ToolActivityState::Cancelled => {
            vega_i18n::t_args("ui.cancelled_action", &[("action", action)])
        }
        ToolActivityState::Failed => {
            vega_i18n::t_args("ui.failed_to_action", &[("action", action)])
        }
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
        ReadOnlyToolKind::ConversationHistory => "read conversation",
    };
    generic_verb(action, state, status)
}

fn skill_summary(name: Option<&str>, outcome: &SkillCardOutcome) -> String {
    let name = name.unwrap_or_else(|| vega_i18n::t("ui.unknown"));
    match outcome {
        SkillCardOutcome::Loaded => vega_i18n::t_args("ui.loaded_skill_name", &[("name", name)]),
        SkillCardOutcome::AlreadyLoaded => {
            vega_i18n::t_args("ui.loaded_skill_name_reused", &[("name", name)])
        }
        SkillCardOutcome::ResourceRead { text_bytes, sha256 } => vega_i18n::t_args(
            "ui.read_skill_name_reference_text_bytes_bytes_sha",
            &[
                ("name", name),
                ("text_bytes", &text_bytes.to_string()),
                (
                    "digest",
                    sha256
                        .get(..12)
                        .unwrap_or_else(|| vega_i18n::t("ui.invalid")),
                ),
            ],
        ),
        SkillCardOutcome::AssetMetadata { size_bytes } => vega_i18n::t_args(
            "ui.viewed_skill_name_asset_metadata_size_bytes_bytes",
            &[("name", name), ("size_bytes", &size_bytes.to_string())],
        ),
        SkillCardOutcome::Failed { code } => vega_i18n::t_args(
            "ui.skill_name_failed_code",
            &[("name", name), ("code", &code.to_string())],
        ),
        SkillCardOutcome::Rejected => {
            vega_i18n::t_args("ui.rejected_skill_name", &[("name", name)])
        }
        SkillCardOutcome::Cancelled => {
            vega_i18n::t_args("ui.cancelled_skill_name", &[("name", name)])
        }
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
        ToolCardResultProjection::UserQuestion { output, .. } => {
            return output.text().lines().map(str::to_string).collect();
        }
        ToolCardResultProjection::Subagent { output, .. }
        | ToolCardResultProjection::Bash { output, .. }
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
    id: String,
    scroll: &ScrollHandle,
    colors: &ThemeColors,
    cx: &App,
) -> AnyElement {
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
        let text = detail_row(
            "tool-detail-command",
            format!("$ {command}"),
            colors.text_primary,
            true,
        );
        let source = (!command.is_empty()).then_some(command);
        rows.push(detail_copy_region(
            text,
            source,
            format!("{id}-copy-command"),
            vega_i18n::t("ui.copy_command"),
            colors,
        ));
    }
    let output = div()
        .w_full()
        .min_w_0()
        .flex()
        .flex_col()
        .children(detail.output.into_iter().enumerate().map(|(index, line)| {
            detail_row(
                gpui_kit::SharedString::from(format!("tool-output-{index}")),
                line,
                colors.text_primary,
                true,
            )
        }))
        .into_any_element();
    rows.push(detail_copy_region(
        output,
        detail.output_copy,
        format!("{id}-copy-output"),
        vega_i18n::t("ui.copy_output"),
        colors,
    ));
    if let Some((footer, state)) = detail.footer {
        let color = if state == ToolActivityState::Success {
            colors.text_secondary
        } else {
            state.terminal_color(colors)
        };
        rows.push(
            div()
                .w_full()
                .min_h(px(ROW_HEIGHT))
                .flex_shrink_0()
                .flex()
                .justify_end()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(Layout::CONVERSATION_INLINE_GAP))
                        .text_size(px(Typography::CONVERSATION))
                        .text_color(color)
                        .when_some(state.terminal_icon(), |row, icon| {
                            row.child(
                                div().size(px(Layout::ICON_SIZE)).flex_shrink_0().child(
                                    crate::icons::sized_icon(icon, color, Layout::ICON_SIZE),
                                ),
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
        .map(|mut surface| {
            let fade = ScrollFade::compact()
                .rounded_parent(surface.style())
                .vertical_gutter(px(Layout::DISCLOSURE_SCROLLBAR_GUTTER));
            surface.child(
                div()
                    .id(gpui_kit::SharedString::from(id))
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
                    .children(rows)
                    .tracked_scroll_edge_fade(scroll.clone(), colors.code_bg, fade),
            )
        })
        .child(disclosure_scrollbar(scroll, cx))
        .into_any_element()
}

fn detail_copy_region(
    content: AnyElement,
    source: Option<String>,
    id: String,
    label: &'static str,
    colors: &ThemeColors,
) -> AnyElement {
    let Some(source) = source else {
        return content;
    };
    let group: gpui_kit::SharedString = format!("{id}-region").into();
    div()
        .id(group.clone())
        .group(group.clone())
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .flex()
        .items_start()
        .gap(px(Layout::COPY_ACTION_GAP))
        .child(div().min_w_0().flex_1().child(content))
        .child(crate::conversation_stream::tool_text_copy_button(
            id.into(),
            group,
            source,
            label,
            *colors,
        ))
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
