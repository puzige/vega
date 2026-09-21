//! Audited tool-call activities over strict `vega_conversation` projections.

use std::time::{Duration, Instant};

use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, Entity, MouseButton, MouseUpEvent, Task, div, px};
use vega_conversation::types::{
    Approval, InvalidToolKind, ReadOnlyToolKind, SkillCardOutcome, ToolCall, ToolCallStatus,
    ToolCardInputProjection, ToolCardResultProjection, ToolResult, tool_card_input_projection,
    tool_card_result_projection,
};
use vega_theme::{ThemeColors, Typography, theme};

use crate::conversation_stream::{MONOFONT, ROW_HEIGHT};
use crate::icons::Icon;

const CORRUPT_LABEL: &str = "工具结果损坏";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ToolActivityCategory {
    Shell,
    Read,
    Find,
    Search,
    Write,
    Edit,
    Mcp,
    Skill,
    Other,
}

impl ToolActivityCategory {
    pub(crate) const fn action_phrase(self) -> &'static str {
        match self {
            Self::Shell => "运行命令",
            Self::Read => "读取文件",
            Self::Find => "查找文件",
            Self::Search => "搜索内容",
            Self::Write => "写入文件",
            Self::Edit => "编辑文件",
            Self::Mcp => "调用 MCP",
            Self::Skill => "使用 Skill",
            Self::Other => "处理工具",
        }
    }

    pub(crate) const fn icon(self) -> Icon {
        match self {
            Self::Shell => Icon::Terminal,
            Self::Search => Icon::Search,
            Self::Read | Self::Find | Self::Write | Self::Edit | Self::Skill => Icon::Document,
            Self::Mcp | Self::Other => Icon::Summary,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ToolActivityState {
    Success,
    Active,
    Cancelled,
    Rejected,
    Failed,
}

impl ToolActivityState {
    pub(crate) const fn priority(self) -> u8 {
        match self {
            Self::Success => 0,
            Self::Active => 1,
            Self::Cancelled => 2,
            Self::Rejected => 3,
            Self::Failed => 4,
        }
    }

    pub(crate) fn terminal_color(self, colors: &ThemeColors) -> gpui_kit::Rgba {
        match self {
            Self::Success => colors.success,
            Self::Cancelled | Self::Rejected | Self::Failed => colors.danger,
            Self::Active => colors.text_secondary,
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

    /// Number of logical compact/detail rows carried by this call.
    pub fn row_count(&self) -> usize {
        1 + if self.expanded {
            self.detail_row_count()
        } else {
            0
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
            (Some(ToolCardInputProjection::ReadOnly { tool }), _) => match tool {
                ReadOnlyToolKind::Read => ToolActivityCategory::Read,
                ReadOnlyToolKind::Glob => ToolActivityCategory::Find,
                ReadOnlyToolKind::Grep => ToolActivityCategory::Search,
            },
            (Some(ToolCardInputProjection::Bash { .. }), _) => ToolActivityCategory::Shell,
            (Some(ToolCardInputProjection::Write { .. }), _) => ToolActivityCategory::Write,
            (Some(ToolCardInputProjection::Edit { .. }), _) => ToolActivityCategory::Edit,
            (Some(ToolCardInputProjection::Mcp { .. }), _) => ToolActivityCategory::Mcp,
            (Some(ToolCardInputProjection::Skill { .. }), _) => ToolActivityCategory::Skill,
            (None, Some(ToolCardResultProjection::InvalidRejected { tool, .. })) => match tool {
                InvalidToolKind::Bash => ToolActivityCategory::Shell,
                InvalidToolKind::Write => ToolActivityCategory::Write,
                InvalidToolKind::Edit => ToolActivityCategory::Edit,
            },
            _ => ToolActivityCategory::Other,
        }
    }

    pub(crate) fn activity_state(&self) -> ToolActivityState {
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
        self.activity_category().icon()
    }

    pub(crate) fn leading_icon_color(&self, colors: &ThemeColors) -> gpui_kit::Rgba {
        colors.text_secondary
    }

    /// Content rendered by the card, used by leak-focused tests.
    pub fn visible_text(&self) -> String {
        let mut text = self.summary.clone();
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

    pub(crate) fn render(
        card: Entity<Self>,
        selector: String,
        indented: bool,
        cx: &App,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let (summary, leading_icon, leading_icon_color, expanded, expandable, detail) = {
            let card_ref = card.read(cx);
            let expanded = card_ref.expanded;
            (
                card_ref.summary.clone(),
                card_ref.leading_icon(),
                card_ref.leading_icon_color(&colors),
                expanded,
                card_ref.has_detail(),
                if expanded { card_ref.detail() } else { None },
            )
        };
        let toggle_card = card.clone();
        let debug_selector = selector.clone();
        let chevron_selector = format!("{selector}-chevron");
        let row = div()
            .debug_selector(move || debug_selector.clone())
            .h(px(ROW_HEIGHT))
            .w_full()
            .min_w_0()
            .flex()
            .items_center()
            .gap_2()
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
            .child(crate::icons::icon(leading_icon, leading_icon_color))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .text_size(px(Typography::BODY))
                    .text_color(colors.text_secondary)
                    .child(summary),
            )
            .when(expandable, |row| {
                row.child(
                    div()
                        .debug_selector(move || chevron_selector.clone())
                        .child(crate::icons::icon(
                            if expanded {
                                Icon::ChevronDown
                            } else {
                                Icon::ChevronRight
                            },
                            colors.text_tertiary,
                        )),
                )
            });
        div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .when(indented, |card| card.pl_4())
            .child(row)
            .when_some(expanded.then_some(detail).flatten(), move |card, detail| {
                card.child(render_detail(detail, format!("{selector}-detail"), &colors))
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
            ) => "已拒绝运行：参数无效 · cmd 需为非空字符串；timeout_ms 若提供须为正整数；不支持其他字段"
                .to_string(),
            (_, Some(ToolCardResultProjection::InvalidRejected { tool, code, .. })) => {
                format!("已拒绝{}：{}", invalid_action(*tool), code.as_str())
            }
            (Some(ToolCardInputProjection::Bash { command }), _) => {
                let command = one_line(command);
                let verb = match self.activity_state() {
                    ToolActivityState::Success => "已运行",
                    ToolActivityState::Active => {
                        if self.status == ToolCallStatus::PendingApproval {
                            "等待批准运行"
                        } else {
                            "正在运行"
                        }
                    }
                    ToolActivityState::Rejected => "已拒绝运行",
                    ToolActivityState::Cancelled => "已取消运行",
                    ToolActivityState::Failed => "运行失败",
                };
                let mut summary = format!("{verb} {command}");
                if self.status == ToolCallStatus::Running
                    && let Some(elapsed_seconds) = self.running_elapsed_seconds
                {
                    summary.push_str(" · ");
                    summary.push_str(&human_elapsed(elapsed_seconds));
                }
                self.push_bash_metadata(&mut summary, true);
                summary
            }
            (Some(ToolCardInputProjection::ReadOnly { tool }), _) => {
                readonly_summary(*tool, self.activity_state(), self.status)
            }
            (
                Some(ToolCardInputProjection::Write {
                    path,
                    content_bytes,
                }),
                None,
            ) => format!(
                "{} {path} · {content_bytes} bytes",
                mutation_verb("写入", self.activity_state(), self.status)
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
                mutation_verb("编辑", self.activity_state(), self.status)
            ),
            (
                _,
                Some(ToolCardResultProjection::WriteSuccess {
                    path,
                    bytes_written,
                    ..
                }),
            ) => format!("已写入 {path} · {bytes_written} bytes"),
            (
                _,
                Some(ToolCardResultProjection::EditSuccess {
                    path,
                    bytes_written,
                    replacements,
                    ..
                }),
            ) => format!(
                "已编辑 {path} · {bytes_written} bytes · {replacements} replacements"
            ),
            (
                Some(ToolCardInputProjection::Write { path, .. }),
                Some(ToolCardResultProjection::MutationTerminal { .. }),
            ) => format!(
                "{} {path}",
                mutation_verb("写入", self.activity_state(), self.status)
            ),
            (
                Some(ToolCardInputProjection::Edit { path, .. }),
                Some(ToolCardResultProjection::MutationTerminal { .. }),
            ) => format!(
                "{} {path}",
                mutation_verb("编辑", self.activity_state(), self.status)
            ),
            (
                Some(ToolCardInputProjection::Skill { name, .. }),
                Some(ToolCardResultProjection::Skill { outcome, .. }),
            ) => skill_summary(name.as_deref(), outcome),
            (Some(ToolCardInputProjection::Skill { name, .. }), None) => format!(
                "{} Skill {}",
                if self.status == ToolCallStatus::PendingApproval {
                    "等待使用"
                } else {
                    "正在使用"
                },
                name.as_deref().unwrap_or("未知")
            ),
            (Some(ToolCardInputProjection::Mcp { identity, .. }), _) => format!(
                "{} {} · server {}",
                generic_verb("调用", self.activity_state(), self.status),
                identity.exact_tool_name,
                identity.server_id
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

    fn stop_live_elapsed(&mut self) {
        self.running_started_at = None;
        self.running_elapsed_seconds = None;
        self.elapsed_refresh_task = None;
    }

    fn push_bash_metadata(&self, label: &mut String, compact: bool) {
        let Some(ToolCardResultProjection::Bash {
            exit_code,
            duration_ms,
            truncated,
            reused,
            ..
        }) = &self.result
        else {
            return;
        };
        if let Some(code) = exit_code
            && (!compact || *code != 0)
        {
            label.push_str(&format!(" · exit {code}"));
        }
        if let Some(duration_ms) = duration_ms {
            label.push_str(" · ");
            label.push_str(&human_duration(*duration_ms));
        }
        if *truncated == Some(true) {
            label.push_str(" · 已截断");
        }
        if *reused {
            label.push_str(" · 已复用");
        }
    }

    fn terminal_footer(&self) -> String {
        let mut footer = match self.activity_state() {
            ToolActivityState::Success => "已完成".to_string(),
            ToolActivityState::Active => {
                if self.status == ToolCallStatus::PendingApproval {
                    "等待批准".to_string()
                } else {
                    "正在运行".to_string()
                }
            }
            ToolActivityState::Rejected => "已拒绝".to_string(),
            ToolActivityState::Cancelled => "已取消".to_string(),
            ToolActivityState::Failed => "运行失败".to_string(),
        };
        self.push_bash_metadata(&mut footer, false);
        footer
    }

    fn has_detail(&self) -> bool {
        match &self.input {
            Some(ToolCardInputProjection::Bash { .. }) => true,
            Some(ToolCardInputProjection::ReadOnly { .. })
            | Some(ToolCardInputProjection::Mcp { .. }) => !self.output_rows.is_empty(),
            _ => false,
        }
    }

    fn detail_row_count(&self) -> usize {
        match &self.input {
            Some(ToolCardInputProjection::Bash { .. }) => 3 + self.output_rows.len(),
            Some(ToolCardInputProjection::ReadOnly { .. })
            | Some(ToolCardInputProjection::Mcp { .. }) => self.output_rows.len(),
            _ => 0,
        }
    }

    fn detail(&self) -> Option<ToolDetail> {
        match &self.input {
            Some(ToolCardInputProjection::Bash { command }) => Some(ToolDetail {
                title: Some("Shell".to_string()),
                command: Some(format!("$ {command}")),
                output: self.output_rows.clone(),
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
            Some(ToolCardInputProjection::Mcp { .. }) if !self.output_rows.is_empty() => {
                Some(ToolDetail {
                    title: None,
                    command: None,
                    output: self.output_rows.clone(),
                    footer: None,
                })
            }
            _ => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn detail_footer_state(&self) -> Option<ToolActivityState> {
        self.detail()
            .and_then(|detail| detail.footer.map(|(_, state)| state))
    }
}

fn one_line(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character != ' ' && character.is_whitespace() {
                ' '
            } else {
                character
            }
        })
        .collect()
}

fn invalid_action(tool: InvalidToolKind) -> &'static str {
    match tool {
        InvalidToolKind::Bash => "运行",
        InvalidToolKind::Write => "写入",
        InvalidToolKind::Edit => "编辑",
    }
}

fn generic_verb(action: &'static str, state: ToolActivityState, status: ToolCallStatus) -> String {
    match state {
        ToolActivityState::Success => format!("已{action}"),
        ToolActivityState::Active if status == ToolCallStatus::PendingApproval => {
            format!("等待批准{action}")
        }
        ToolActivityState::Active => format!("正在{action}"),
        ToolActivityState::Rejected => format!("已拒绝{action}"),
        ToolActivityState::Cancelled => format!("已取消{action}"),
        ToolActivityState::Failed => format!("{action}失败"),
    }
}

fn mutation_verb(action: &'static str, state: ToolActivityState, status: ToolCallStatus) -> String {
    generic_verb(action, state, status)
}

fn readonly_summary(
    tool: ReadOnlyToolKind,
    state: ToolActivityState,
    status: ToolCallStatus,
) -> String {
    let action = match tool {
        ReadOnlyToolKind::Read => "读取文件",
        ReadOnlyToolKind::Glob => "查找文件",
        ReadOnlyToolKind::Grep => "搜索内容",
    };
    match state {
        ToolActivityState::Success => format!("已{action}"),
        ToolActivityState::Active if status == ToolCallStatus::PendingApproval => {
            format!("等待{action}")
        }
        ToolActivityState::Active => format!("正在{action}"),
        ToolActivityState::Rejected => format!("已拒绝{action}"),
        ToolActivityState::Cancelled => format!("已取消{action}"),
        ToolActivityState::Failed => format!("{action}失败"),
    }
}

fn skill_summary(name: Option<&str>, outcome: &SkillCardOutcome) -> String {
    let name = name.unwrap_or("未知");
    match outcome {
        SkillCardOutcome::Loaded => format!("已加载 Skill {name}"),
        SkillCardOutcome::AlreadyLoaded => format!("已加载 Skill {name} · 已复用"),
        SkillCardOutcome::ResourceRead { text_bytes, sha256 } => format!(
            "已读取 Skill {name} 引用 · {text_bytes} bytes · SHA-256 {}…",
            sha256.get(..12).unwrap_or("invalid")
        ),
        SkillCardOutcome::Failed { code } => format!("Skill {name} 失败 · {code}"),
        SkillCardOutcome::Rejected => format!("已拒绝 Skill {name}"),
        SkillCardOutcome::Cancelled => format!("已取消 Skill {name}"),
    }
}

fn human_duration(duration_ms: u64) -> String {
    if duration_ms < 1_000 {
        return format!("{duration_ms} 毫秒");
    }
    if duration_ms < 60_000 {
        let seconds = duration_ms as f64 / 1_000.0;
        return if duration_ms.is_multiple_of(1_000) {
            format!("{} 秒", duration_ms / 1_000)
        } else {
            format!("{seconds:.1} 秒")
        };
    }
    let minutes = duration_ms / 60_000;
    let seconds = duration_ms % 60_000 / 1_000;
    if seconds == 0 {
        format!("{minutes} 分钟")
    } else {
        format!("{minutes} 分 {seconds} 秒")
    }
}

fn human_elapsed(seconds: u64) -> String {
    if seconds < 60 {
        return format!("{seconds} 秒");
    }
    let minutes = seconds / 60;
    let seconds = seconds % 60;
    if seconds == 0 {
        format!("{minutes} 分钟")
    } else {
        format!("{minutes} 分 {seconds} 秒")
    }
}

fn projection_output_rows(projection: &ToolCardResultProjection) -> Vec<String> {
    let output = match projection {
        ToolCardResultProjection::Bash { output, .. }
        | ToolCardResultProjection::ReadOnly { output, .. }
        | ToolCardResultProjection::Mcp { output, .. } => output,
        _ => return Vec::new(),
    };
    output.lines().map(str::to_string).collect()
}

fn render_detail(detail: ToolDetail, selector: String, colors: &ThemeColors) -> AnyElement {
    let debug_selector = selector.clone();
    let mut rows = Vec::with_capacity(detail.logical_rows());
    if let Some(title) = detail.title {
        rows.push(detail_row(title, colors.text_primary, false));
    }
    if let Some(command) = detail.command {
        rows.push(detail_row(command, colors.text_primary, true));
    }
    rows.extend(
        detail
            .output
            .into_iter()
            .map(|line| detail_row(line, colors.text_secondary, true)),
    );
    if let Some((footer, state)) = detail.footer {
        rows.push(detail_row(footer, state.terminal_color(colors), false));
    }
    div()
        .debug_selector(move || debug_selector.clone())
        .w_full()
        .min_w_0()
        .mt_1()
        .mb_1()
        .rounded_lg()
        .border_1()
        .border_color(colors.border_subtle)
        .bg(colors.code_bg)
        .overflow_hidden()
        .flex()
        .flex_col()
        .children(rows)
        .into_any_element()
}

fn detail_row(text: String, color: gpui_kit::Rgba, code: bool) -> AnyElement {
    div()
        .w_full()
        .min_w_0()
        .min_h(px(ROW_HEIGHT))
        .px_2()
        .py_1()
        .when(code, |row| row.font_family(MONOFONT))
        .text_size(px(if code {
            Typography::CODE
        } else {
            Typography::METADATA
        }))
        .text_color(color)
        .child(text)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use vega_conversation::types::{InvalidToolCode, InvalidToolKind, InvalidToolProjection};
    use vega_theme::LIGHT;

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
            let card = ToolCard::hydrated(
                Some(ToolCardInputProjection::Bash {
                    command: "true".into(),
                }),
                status,
                None,
                None,
            );
            assert!(
                matches!(card.leading_icon(), Icon::Terminal),
                "Shell must keep Terminal for {status:?}"
            );
            assert_eq!(
                card.leading_icon_color(&LIGHT),
                LIGHT.text_secondary,
                "leading icon must stay neutral for {status:?}"
            );
        }

        for (category, expected) in [
            (ToolActivityCategory::Shell, Icon::Terminal),
            (ToolActivityCategory::Read, Icon::Document),
            (ToolActivityCategory::Find, Icon::Document),
            (ToolActivityCategory::Search, Icon::Search),
            (ToolActivityCategory::Write, Icon::Document),
            (ToolActivityCategory::Edit, Icon::Document),
            (ToolActivityCategory::Mcp, Icon::Summary),
            (ToolActivityCategory::Skill, Icon::Document),
            (ToolActivityCategory::Other, Icon::Summary),
        ] {
            assert!(
                matches!(
                    (category.icon(), expected),
                    (Icon::Terminal, Icon::Terminal)
                        | (Icon::Document, Icon::Document)
                        | (Icon::Search, Icon::Search)
                        | (Icon::Summary, Icon::Summary)
                ),
                "category icon mapping changed"
            );
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
        assert!(load_card.visible_text().contains("已加载 Skill reviewer"));
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
        assert!(visible.contains("已读取 Skill reviewer 引用"));
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
        assert_eq!(card.visible_text(), "已拒绝写入：malformed_json");
    }

    #[test]
    fn issue90_hydrated_bash_validation_card_is_safe_and_actionable() {
        let card = ToolCard::hydrated(
            None,
            ToolCallStatus::Rejected,
            Some(Approval::Deny),
            Some(ToolCardResultProjection::InvalidRejected {
                tool: InvalidToolKind::Bash,
                code: InvalidToolCode::InvalidInput,
                reused: true,
            }),
        );
        assert!(card.is_invalid_terminal());
        assert_eq!(
            card.visible_text(),
            "已拒绝运行：参数无效 · cmd 需为非空字符串；timeout_ms 若提供须为正整数；不支持其他字段"
        );
        assert!(card.permission_identity().is_none());
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
        assert!(card.visible_text().contains("已运行 printf 'ok'"));
        assert!(!card.visible_text().contains("\nok"));
        card.expanded = true;
        assert_eq!(card.row_count(), 5);
        assert!(
            card.visible_text()
                .contains("\nShell\n$ printf 'ok'\nok\n已完成")
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
        assert!(
            card.visible_text()
                .contains("运行失败 false · exit 1 · 4 毫秒")
        );
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
            r#"{"audit_version":"write_edit_v1","tool":"write","path":"/SECRET_DATA_ROOT/a.txt","content_bytes":1,"fingerprint_v1":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#,
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
            r#"{"path":"/SECRET_DATA_ROOT/a.txt","bytes_written":1,"checkpoint_ref":"preimage-v1/id-70/id-74/id-63"}"#,
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
            r#"{"path":"a.txt","bytes_written":2,"replacements":2,"checkpoint_ref":"preimage-v1/id-70/id-74/id-63"}"#,
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
        assert!(card.visible_text().contains(&command));
        assert!(!card.visible_text().contains("SECRET_CALL_ID"));
        let mut card = card;
        card.expanded = true;
        assert!(card.visible_text().contains(&format!("$ {command}")));
    }

    #[test]
    fn compact_bash_command_preserves_quoted_spaces_and_flattens_layout_whitespace() {
        let command = "printf 'a  b'\nprintf\t'done'";
        let call = ToolCall {
            id: "bash-spacing".into(),
            tool: "bash".into(),
            input_json: serde_json::json!({ "cmd": command }).to_string(),
        };
        let mut card = ToolCard::proposed(&call);
        assert_eq!(
            card.summary, "等待批准运行 printf 'a  b' printf 'done'",
            "compact copy preserves meaningful ordinary spaces while staying on one line"
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
        assert_eq!(card.visible_text(), "工具结果损坏");
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
