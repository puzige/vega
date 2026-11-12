mod nsh_tool;
use std::time::Duration;

use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, Entity, ScrollHandle, Task, div, px};
use vega_conversation::types::{
    Approval, BashStartClock, CodexAcpActivityIdentity, CodexArgumentPreview, CodexDisplayMetadata,
    InvalidToolKind, ReadOnlyToolKind, SkillCardOutcome, SubagentOpenRequested, SubagentSnapshot,
    SubagentState, ToolCall, ToolCallStatus, ToolCardInputProjection, ToolCardResultProjection,
    ToolResult, tool_card_description, tool_card_input_projection, tool_card_result_projection,
};
use vega_theme::{Layout, ThemeColors, theme};

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
            Self::Skill => vega_i18n::t("ui.read_skill"),
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
            Self::Skill => Icon::Wrench,
            Self::Other => Icon::Wrench,
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

    pub(crate) fn terminal_color(self, colors: &ThemeColors) -> gpui_kit::Rgba {
        match self {
            Self::Success => colors.success,
            Self::Cancelled | Self::Rejected | Self::Failed => colors.danger,
            Self::Active | Self::Superseded => colors.text_secondary,
        }
    }
}

#[derive(Clone, PartialEq)]
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

pub struct ShellControlRequested {
    pub operation_id: String,
}

pub struct ToolCard {
    input: Option<ToolCardInputProjection>,
    description: Option<String>,
    status: ToolCallStatus,
    approval: Option<Approval>,
    result: Option<ToolCardResultProjection>,
    shell_command: Option<vega_conversation::types::ShellCommandProjection>,
    shell_control_pending: bool,
    shell_control_failed: bool,
    summary: String,
    summary_language: &'static str,
    output_rows: Vec<String>,
    expanded: bool,
    presentation: Option<nsh_tool::Presentation>,
    running_started_at: Option<BashStartClock>,
    running_elapsed_seconds: Option<u64>,
    elapsed_refresh_task: Option<Task<()>>,
    presentation_released: bool,
    execution: Option<SubagentSnapshot>,
}

impl gpui_kit::EventEmitter<SubagentOpenRequested> for ToolCard {}
impl gpui_kit::EventEmitter<ShellControlRequested> for ToolCard {}

impl ToolCard {
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
            shell_command: None,
            shell_control_pending: false,
            shell_control_failed: false,
            summary: String::new(),
            summary_language: vega_i18n::language(),
            output_rows: Vec::new(),
            expanded: false,
            presentation: None,
            running_started_at: None,
            running_elapsed_seconds: None,
            elapsed_refresh_task: None,
            presentation_released: false,
            execution: None,
        };
        card.refresh_summary();
        card
    }

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
            shell_command: None,
            shell_control_pending: false,
            shell_control_failed: false,
            summary: String::new(),
            summary_language: vega_i18n::language(),
            output_rows: Vec::new(),
            expanded: false,
            presentation: None,
            running_started_at: None,
            running_elapsed_seconds: None,
            elapsed_refresh_task: None,
            presentation_released: true,
            execution: None,
        };
        card.refresh_summary();
        card
    }

    pub fn corrupt() -> Self {
        let mut card = Self {
            input: None,
            description: None,
            status: ToolCallStatus::Failed,
            approval: None,
            result: Some(ToolCardResultProjection::Corrupt),
            shell_command: None,
            shell_control_pending: false,
            shell_control_failed: false,
            summary: String::new(),
            summary_language: vega_i18n::language(),
            output_rows: Vec::new(),
            expanded: false,
            presentation: None,
            running_started_at: None,
            running_elapsed_seconds: None,
            elapsed_refresh_task: None,
            presentation_released: true,
            execution: None,
        };
        card.refresh_summary();
        card
    }

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
            shell_command: None,
            shell_control_pending: false,
            shell_control_failed: false,
            summary: String::new(),
            summary_language: vega_i18n::language(),
            expanded: false,
            presentation: None,
            running_started_at: None,
            running_elapsed_seconds: None,
            elapsed_refresh_task: None,
            presentation_released: true,
            execution: None,
        };
        card.refresh_summary();
        card
    }

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

    pub fn apply_running(
        &mut self,
        started_at: Option<BashStartClock>,
        cx: &mut gpui_kit::Context<Self>,
    ) -> bool {
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
        if !self.presentation_released
            && self.is_bash()
            && self.running_started_at.is_none()
            && let Some(started_at) = started_at
            && !started_at.is_cancelled()
        {
            self.running_started_at = Some(started_at.clone());
            if let Some(started) = started_at.started_at()
                && !started_at.is_cancelled()
            {
                self.running_elapsed_seconds = Some(
                    cx.background_executor()
                        .now()
                        .saturating_duration_since(started)
                        .as_secs(),
                );
            }
            let task = cx.spawn(async move |this, cx| {
                loop {
                    let delay = this
                        .update(cx, |card, cx| {
                            if card.status != ToolCallStatus::Running
                                || !card.is_bash()
                                || card.running_started_at.is_none()
                            {
                                return None;
                            }
                            let started = started_at.started_at();
                            if started_at.is_cancelled() {
                                card.running_started_at = None;
                                card.running_elapsed_seconds = None;
                                card.refresh_summary();
                                cx.notify();
                                return None;
                            }
                            let Some(started) = started else {
                                return Some(Duration::from_millis(250));
                            };
                            let elapsed = cx
                                .background_executor()
                                .now()
                                .saturating_duration_since(started);
                            if card.running_elapsed_seconds != Some(elapsed.as_secs()) {
                                card.running_elapsed_seconds = Some(elapsed.as_secs());
                                card.refresh_summary();
                                cx.notify();
                            }
                            Some(
                                Duration::from_secs(elapsed.as_secs().saturating_add(1))
                                    .saturating_sub(elapsed),
                            )
                        })
                        .ok()
                        .flatten();
                    let Some(delay) = delay else {
                        break;
                    };
                    cx.background_executor().timer(delay).await;
                }
            });
            self.elapsed_refresh_task = Some(task);
        }
        self.refresh_summary();
        cx.notify();
        true
    }

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
            self.invalidate_layout();
            cx.notify();
        }
    }

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
            (
                Some(
                    ToolCardInputProjection::Bash { .. }
                    | ToolCardInputProjection::ShellControl { .. },
                ),
                _,
            ) => ToolActivityCategory::Shell,
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
            Some(
                ToolCardInputProjection::Bash { .. } | ToolCardInputProjection::ShellControl { .. },
            ) => ToolActivityCategory::Shell,
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
        if let Some(state) = self.shell_session_state() {
            return match state {
                "running" | "finishing" | "stopping" => ToolActivityState::Active,
                "succeeded" => ToolActivityState::Success,
                "cancelled" => ToolActivityState::Cancelled,
                _ => ToolActivityState::Failed,
            };
        }
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
        if self.summary_language == vega_i18n::language()
            && !(self.is_bash() && self.status == ToolCallStatus::Running)
        {
            self.summary.clone()
        } else {
            self.build_summary()
        }
    }

    pub(crate) fn description_headline(&self) -> Option<(String, Icon)> {
        if matches!(self.input, Some(ToolCardInputProjection::Skill { .. })) {
            return Some((self.headline_title(), self.leading_icon()));
        }
        self.description_title()
            .map(|title| (title, self.leading_icon()))
    }

    pub(crate) fn headline_notice(&self, title: &str) -> Option<String> {
        let safety_title = self.headline_title();
        (title != safety_title
            && (self.status == ToolCallStatus::PendingApproval
                || !matches!(
                    self.activity_state(),
                    ToolActivityState::Active | ToolActivityState::Success
                )))
        .then_some(safety_title)
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

    fn live_elapsed_seconds(&self) -> Option<u64> {
        if self
            .running_started_at
            .as_ref()
            .is_some_and(BashStartClock::is_cancelled)
        {
            None
        } else {
            self.running_elapsed_seconds
        }
    }

    pub(crate) fn compact_title(&self) -> String {
        if self.shell_session_state().is_some() {
            return self.build_bash_summary();
        }
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
        if self.status == ToolCallStatus::Running && self.live_elapsed_seconds().is_none() {
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
            return self.live_elapsed_seconds().map(human_elapsed);
        }
        if let Some(command) = &self.shell_command {
            return command.duration_ms.map(human_duration);
        }
        match &self.result {
            Some(ToolCardResultProjection::Bash {
                duration_ms: Some(duration),
                ..
            }) => Some(human_duration(*duration)),
            _ => None,
        }
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
            self.invalidate_layout();
            cx.notify();
        }
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
        if matches!(self.input, Some(ToolCardInputProjection::Skill { .. })) {
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
        self.invalidate_layout();
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
            (
                Some(
                    ToolCardInputProjection::Bash { .. }
                    | ToolCardInputProjection::ShellControl { .. },
                ),
                _,
            ) => self.build_bash_summary(),
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
            ) => match outcome {
                SkillCardOutcome::Loaded
                | SkillCardOutcome::AlreadyLoaded
                | SkillCardOutcome::ResourceRead { .. }
                | SkillCardOutcome::AssetMetadata { .. } => vega_i18n::t_args(
                    "ui.loaded_skill_name",
                    &[(
                        "name",
                        name.as_deref()
                            .unwrap_or_else(|| vega_i18n::t("ui.unknown")),
                    )],
                ),
                _ => skill_summary(name.as_deref(), outcome),
            },
            (Some(ToolCardInputProjection::Skill { name, .. }), None) => vega_i18n::t_args(
                "ui.action_skill",
                &[
                    (
                        "action",
                        &generic_verb("read", self.activity_state(), self.status),
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
        matches!(
            self.input,
            Some(
                ToolCardInputProjection::Bash { .. } | ToolCardInputProjection::ShellControl { .. }
            )
        )
    }

    pub(crate) fn apply_shell_command(
        &mut self,
        command: &vega_conversation::types::ShellCommandProjection,
    ) -> bool {
        let matches = matches!(&self.result, Some(ToolCardResultProjection::Bash { operation_id: Some(operation), .. }) if operation.as_ref() == command.operation_id.as_str());
        if !matches || !matches!(&self.input, Some(ToolCardInputProjection::Bash { .. })) {
            return false;
        }
        if matches!(
            command.state.as_str(),
            "succeeded" | "failed" | "cancelled" | "timed_out"
        ) && !command.cleanup_unresolved
        {
            self.shell_control_failed = false;
        }
        self.shell_command = Some(command.clone());
        self.refresh_summary();
        true
    }

    pub(crate) fn matches_shell_control_source(
        &self,
        operation_id: &str,
        call_id: &str,
        thread_id: &str,
    ) -> bool {
        self.shell_command.as_ref().is_some_and(|command| {
            command.operation_id == operation_id
                && command.call_id == call_id
                && command.thread_id == thread_id
        })
    }

    pub(crate) fn finish_shell_control(&mut self, operation_id: &str, failed: bool) -> bool {
        if !self
            .shell_command
            .as_ref()
            .is_some_and(|command| command.operation_id == operation_id)
        {
            return false;
        }
        self.shell_control_pending = false;
        self.shell_control_failed = failed;
        true
    }

    fn shell_session_state(&self) -> Option<&str> {
        if let Some(command) = &self.shell_command {
            return Some(&command.state);
        }
        match &self.result {
            Some(ToolCardResultProjection::Bash { session_state, .. }) => session_state.as_deref(),
            _ => None,
        }
    }

    fn build_bash_summary(&self) -> String {
        if let Some(state) = self.shell_session_state() {
            return vega_i18n::t(match state {
                "running" => "ui.shell_session.running",
                "finishing" => "ui.shell_session.finishing",
                "stopping" => "ui.shell_session.stopping",
                "succeeded" => "ui.shell_session.succeeded",
                "failed" => "ui.shell_session.failed",
                "cancelled" => "ui.shell_session.cancelled",
                "timed_out" => "ui.shell_session.timed_out",
                _ => "ui.shell_recovery.unknown",
            })
            .into();
        }
        if self.status == ToolCallStatus::Running && self.live_elapsed_seconds().is_none() {
            let Some(ToolCardInputProjection::Bash { command }) = &self.input else {
                return vega_i18n::t("ui.shell").into();
            };
            return vega_i18n::t_args(
                "ui.command_action",
                &[
                    ("action", vega_i18n::t("ui.shell")),
                    ("command", &compact_object(command)),
                ],
            );
        }
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

    pub(crate) fn cancel_pending_bash_clock(&mut self) {
        if self.is_bash() {
            self.stop_live_elapsed();
            self.refresh_summary();
        }
    }

    pub(crate) fn release_presentation(&mut self) {
        self.invalidate_layout();
        self.presentation_released = true;
        self.stop_live_elapsed();
    }

    fn stop_live_elapsed(&mut self) {
        self.running_started_at = None;
        self.running_elapsed_seconds = None;
        self.elapsed_refresh_task = None;
    }

    fn bash_detail_title(&self) -> String {
        let mut title = vega_i18n::t("ui.shell").to_string();
        if self
            .shell_command
            .as_ref()
            .is_some_and(|command| command.concurrent && command.cleanup_unresolved)
        {
            title.push_str(" · ");
            title.push_str(vega_i18n::t("ui.shell_session.cleanup_unresolved"));
        }
        if let Some(ToolCardResultProjection::Bash {
            truncated,
            reused,
            output_incomplete,
            ..
        }) = &self.result
        {
            if *truncated == Some(true) {
                title.push_str(vega_i18n::t("ui.truncated"));
            }
            if *reused {
                title.push_str(vega_i18n::t("ui.reused"));
            }
            if *output_incomplete
                || self
                    .shell_command
                    .as_ref()
                    .is_some_and(|command| command.output_incomplete)
            {
                title.push_str(" · ");
                title.push_str(vega_i18n::t("ui.shell_session.output_incomplete"));
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
            ToolActivityState::Active
                if self.is_bash() && self.live_elapsed_seconds().is_none() =>
            {
                vega_i18n::t("ui.pending")
            }
            ToolActivityState::Active => vega_i18n::t("ui.running"),
            ToolActivityState::Rejected => vega_i18n::t("ui.rejected"),
            ToolActivityState::Cancelled => vega_i18n::t("ui.cancelled"),
            ToolActivityState::Failed => vega_i18n::t("ui.failure"),
        }
        .to_string()
    }

    fn has_detail(&self) -> bool {
        if self.uses_target_detail() && self.description_detail().is_some() {
            return true;
        }
        match &self.input {
            Some(
                ToolCardInputProjection::Bash { .. } | ToolCardInputProjection::ShellControl { .. },
            ) => true,
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
        if self.uses_target_detail()
            && let Some(detail) = self.description_detail()
        {
            return detail.logical_rows();
        }
        match &self.input {
            Some(
                ToolCardInputProjection::Bash { .. } | ToolCardInputProjection::ShellControl { .. },
            ) => 3 + self.output_rows.len(),
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

    fn uses_target_detail(&self) -> bool {
        self.description.is_some()
            || matches!(self.input, Some(ToolCardInputProjection::Skill { .. }))
    }

    fn description_detail(&self) -> Option<ToolDetail> {
        if let Some(ToolCardInputProjection::Skill { name, .. }) = &self.input {
            let Some(ToolCardResultProjection::Skill { outcome, .. }) = &self.result else {
                return None;
            };
            return match outcome {
                SkillCardOutcome::AlreadyLoaded
                | SkillCardOutcome::ResourceRead { .. }
                | SkillCardOutcome::AssetMetadata { .. } => Some(ToolDetail {
                    title: None,
                    command: None,
                    output: vec![skill_summary(name.as_deref(), outcome)],
                    output_copy: None,
                    footer: None,
                }),
                _ => None,
            };
        }
        let target = match &self.input {
            Some(ToolCardInputProjection::ReadOnly { detail_target, .. }) => detail_target.clone(),
            Some(
                ToolCardInputProjection::Write { path, .. }
                | ToolCardInputProjection::Edit { path, .. },
            ) => Some(path.clone()),
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
        if self.uses_target_detail()
            && let Some(detail) = self.description_detail()
        {
            return Some(detail);
        }
        match &self.input {
            Some(ToolCardInputProjection::ShellControl { .. }) => Some(ToolDetail {
                title: Some(self.bash_detail_title()),
                command: None,
                output: self.output_rows.clone(),
                output_copy: match &self.result {
                    Some(ToolCardResultProjection::Bash { output, .. }) if !output.is_empty() => {
                        Some(output.clone())
                    }
                    _ => None,
                },
                footer: Some((self.build_bash_summary(), self.activity_state())),
            }),
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
            Some(ToolCardInputProjection::Subagent { tool, .. })
                if tool == "historical_delegation" && !self.output_rows.is_empty() =>
            {
                Some(ToolDetail {
                    title: None,
                    command: None,
                    output: self.output_rows.clone(),
                    output_copy: None,
                    footer: Some((self.terminal_footer(), self.activity_state())),
                })
            }
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
) -> impl Fn(&gpui_kit::ScrollEvent, &mut gpui_kit::Window, &mut App) + 'static {
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
