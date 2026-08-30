//! Audited tool-call cards over strict `vega_conversation` projections.

use gpui::prelude::*;
use gpui::{AnyElement, App, Entity, MouseButton, MouseUpEvent, div, px};
use vega_conversation::types::{
    Approval, ToolCall, ToolCallStatus, ToolCardInputProjection, ToolCardResultProjection,
    ToolResult, tool_card_input_projection, tool_card_result_projection,
};
use vega_theme::{ThemeColors, Typography, theme};

use crate::conversation_stream::{MONOFONT, ROW_HEIGHT};

const CORRUPT_LABEL: &str = "工具结果损坏";

/// UI-only audited tool card. It never retains a provider call id, raw
/// write/edit input, fingerprint, checkpoint reference, or checkpoint path.
pub struct ToolCard {
    input: Option<ToolCardInputProjection>,
    status: ToolCallStatus,
    approval: Option<Approval>,
    result: Option<ToolCardResultProjection>,
    output_rows: Vec<String>,
    expanded: bool,
}

impl ToolCard {
    /// Creates a pending card from a durable safe proposal.
    pub fn proposed(call: &ToolCall) -> Self {
        let input = tool_card_input_projection(call);
        let corrupt = matches!(input, ToolCardInputProjection::Corrupt);
        Self {
            input: Some(input),
            status: if corrupt {
                ToolCallStatus::Failed
            } else {
                ToolCallStatus::PendingApproval
            },
            approval: None,
            result: corrupt.then_some(ToolCardResultProjection::Corrupt),
            output_rows: Vec::new(),
            expanded: false,
        }
    }

    /// Creates the sole legal proposal-free card: atomic invalid write/edit.
    pub fn invalid_terminal(result: &ToolResult) -> Self {
        let projection = tool_card_result_projection(None, result);
        let status = match projection {
            ToolCardResultProjection::InvalidRejected { .. } => ToolCallStatus::Rejected,
            _ => ToolCallStatus::Failed,
        };
        Self {
            input: None,
            status,
            approval: None,
            result: Some(projection),
            output_rows: Vec::new(),
            expanded: false,
        }
    }

    /// Fixed content-free corrupt card for an unknown or illegal transition.
    pub fn corrupt() -> Self {
        Self {
            input: None,
            status: ToolCallStatus::Failed,
            approval: None,
            result: Some(ToolCardResultProjection::Corrupt),
            output_rows: Vec::new(),
            expanded: false,
        }
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
    pub fn fail_corrupt(&mut self, cx: &mut gpui::Context<Self>) {
        self.status = ToolCallStatus::Failed;
        self.approval = None;
        self.result = Some(ToolCardResultProjection::Corrupt);
        self.output_rows.clear();
        self.expanded = false;
        cx.notify();
    }

    /// Marks the post-commit approval visible. With the frozen shared event
    /// shape this is the UI's executing state; Running stays runtime/internal.
    pub fn apply_approved(&mut self, approval: Approval) -> bool {
        if let Some(existing) = self.approval {
            if existing == approval {
                return true;
            }
            self.status = ToolCallStatus::Failed;
            self.result = Some(ToolCardResultProjection::Corrupt);
            return false;
        }
        if self.result.is_some()
            || self.status != ToolCallStatus::PendingApproval
            || approval == Approval::Deny
        {
            self.status = ToolCallStatus::Failed;
            self.result = Some(ToolCardResultProjection::Corrupt);
            return false;
        }
        self.approval = Some(approval);
        self.status = ToolCallStatus::Approved;
        true
    }

    /// Applies one terminal result with strict projection validation.
    pub fn apply_finished(&mut self, result: &ToolResult) -> bool {
        let projection = tool_card_result_projection(self.input.as_ref(), result);
        if let Some(existing) = &self.result {
            if existing == &projection && self.status == result.status {
                return true;
            }
            self.result = Some(ToolCardResultProjection::Corrupt);
            self.status = ToolCallStatus::Failed;
            self.output_rows.clear();
            return false;
        }
        let transition_valid = match result.status {
            ToolCallStatus::Rejected => self.status == ToolCallStatus::PendingApproval,
            ToolCallStatus::Success | ToolCallStatus::Failed | ToolCallStatus::Cancelled => {
                self.status == ToolCallStatus::Approved
                    || (result.reused && self.status == ToolCallStatus::PendingApproval)
            }
            ToolCallStatus::PendingApproval
            | ToolCallStatus::Approved
            | ToolCallStatus::Running => false,
        };
        if !transition_valid {
            self.status = ToolCallStatus::Failed;
            self.result = Some(ToolCardResultProjection::Corrupt);
            self.output_rows.clear();
            return false;
        }
        if matches!(projection, ToolCardResultProjection::Corrupt) {
            self.status = ToolCallStatus::Failed;
        } else {
            self.status = result.status;
        }
        let valid = !matches!(projection, ToolCardResultProjection::Corrupt);
        self.output_rows = projection_output_rows(&projection);
        self.result = Some(projection);
        valid
    }

    /// Exact mutating permission target associated with the safe proposal.
    pub fn permission_identity(&self) -> Option<(&str, &str)> {
        let input = self.input.as_ref()?;
        Some((input.tool()?, input.permission_target()?))
    }

    /// Number of fixed-height virtual rows for this card.
    pub fn row_count(&self) -> usize {
        2 + if self.expanded {
            self.output_rows.len()
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

    /// Content rendered by the card, used by leak-focused tests.
    pub fn visible_text(&self) -> String {
        let mut text = self.header_label();
        if let Some(summary) = self.summary() {
            text.push(' ');
            text.push_str(&summary);
        }
        if self.expanded {
            for line in &self.output_rows {
                text.push('\n');
                text.push_str(line);
            }
        }
        text
    }

    pub(crate) fn render_row(card: Entity<Self>, row: usize, cx: &App) -> AnyElement {
        let colors = theme(cx).colors;
        let card_ref = card.read(cx);
        if row == 0 {
            let expandable = !card_ref.output_rows.is_empty();
            let status_color = card_ref.status_color(&colors);
            let label = card_ref.header_label();
            return div()
                .h(px(ROW_HEIGHT))
                .w_full()
                .flex()
                .items_center()
                .px_2()
                .border_l_1()
                .border_r_1()
                .border_t_1()
                .rounded_tl_lg()
                .rounded_tr_lg()
                .border_color(colors.border_subtle)
                .bg(colors.bg_elevated)
                .text_size(px(Typography::HEADING_CARD))
                .font_weight(Typography::HEADING_CARD_WEIGHT)
                .text_color(status_color)
                .when(expandable, |row| {
                    row.cursor_pointer().on_mouse_up(
                        MouseButton::Left,
                        move |_: &MouseUpEvent, _, cx| {
                            card.update(cx, |card, cx| {
                                card.expanded = !card.expanded;
                                cx.notify();
                            });
                        },
                    )
                })
                .child(label)
                .into_any_element();
        }
        if row == 1 {
            let mut summary = div()
                .h(px(ROW_HEIGHT))
                .w_full()
                .flex()
                .items_center()
                .overflow_hidden()
                .px_2()
                .border_l_1()
                .border_r_1()
                .border_color(colors.border_subtle)
                .bg(colors.code_bg)
                .font_family(MONOFONT)
                .text_size(px(Typography::CODE))
                .text_color(colors.text_primary)
                .child(
                    card_ref
                        .summary()
                        .unwrap_or_else(|| CORRUPT_LABEL.to_string()),
                );
            if card_ref.output_rows.is_empty() || !card_ref.expanded {
                summary = summary.border_b_1().rounded_bl_lg().rounded_br_lg();
            }
            return summary.into_any_element();
        }
        let line = card_ref
            .output_rows
            .get(row - 2)
            .cloned()
            .unwrap_or_default();
        let mut output = div()
            .h(px(ROW_HEIGHT))
            .w_full()
            .flex()
            .items_center()
            .overflow_hidden()
            .px_2()
            .border_l_1()
            .border_r_1()
            .border_color(colors.border_subtle)
            .bg(colors.code_bg)
            .font_family(MONOFONT)
            .text_size(px(Typography::CODE))
            .text_color(colors.text_secondary)
            .child(line);
        if row == card_ref.row_count() - 1 {
            output = output.border_b_1().rounded_bl_lg().rounded_br_lg();
        }
        output.into_any_element()
    }

    fn tool_label(&self) -> &'static str {
        match (&self.input, &self.result) {
            (Some(ToolCardInputProjection::ReadOnly { tool }), _) => match tool.as_str() {
                "read" => "read",
                "glob" => "glob",
                "grep" => "grep",
                _ => "tool",
            },
            (Some(ToolCardInputProjection::Bash { .. }), _) => "bash",
            (Some(ToolCardInputProjection::Write { .. }), _) => "write",
            (Some(ToolCardInputProjection::Edit { .. }), _) => "edit",
            (None, Some(ToolCardResultProjection::InvalidRejected { tool, .. })) => tool.as_str(),
            _ => "tool",
        }
    }

    fn status_label(&self) -> &'static str {
        if self.bash_exit_failed() {
            return "失败";
        }
        match self.status {
            ToolCallStatus::PendingApproval => "待批准",
            ToolCallStatus::Approved => "执行中",
            ToolCallStatus::Running => "执行中",
            ToolCallStatus::Rejected => "已拒绝",
            ToolCallStatus::Success => "已完成",
            ToolCallStatus::Failed => "失败",
            ToolCallStatus::Cancelled => "已取消",
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

    fn status_color(&self, colors: &ThemeColors) -> gpui::Rgba {
        if self.bash_exit_failed() {
            return colors.danger;
        }
        status_color(self.status, colors)
    }

    fn header_label(&self) -> String {
        let mut label = format!("{} · {}", self.tool_label(), self.status_label());
        if let Some(ToolCardResultProjection::Bash {
            exit_code,
            duration_ms,
            truncated,
            reused,
            ..
        }) = &self.result
        {
            if let Some(exit_code) = exit_code {
                label.push_str(&format!(" · exit {exit_code}"));
            }
            if let Some(duration_ms) = duration_ms {
                label.push_str(&format!(" · {duration_ms}ms"));
            }
            if *truncated == Some(true) {
                label.push_str(" · 已截断");
            }
            if *reused {
                label.push_str(" · 已复用");
            }
        }
        label
    }

    fn summary(&self) -> Option<String> {
        match (&self.input, &self.result) {
            (Some(ToolCardInputProjection::Bash { command }), _) => Some(format!("$ {command}")),
            (
                Some(ToolCardInputProjection::Write {
                    path,
                    content_bytes,
                }),
                None,
            ) => Some(format!("{path} · {content_bytes} bytes")),
            (
                Some(ToolCardInputProjection::Edit {
                    path,
                    old_string_bytes,
                    new_string_bytes,
                }),
                None,
            ) => Some(format!(
                "{path} · {old_string_bytes}→{new_string_bytes} bytes"
            )),
            (
                _,
                Some(ToolCardResultProjection::WriteSuccess {
                    path,
                    bytes_written,
                    ..
                }),
            ) => Some(format!("{path} · {bytes_written} bytes")),
            (
                _,
                Some(ToolCardResultProjection::EditSuccess {
                    path,
                    bytes_written,
                    replacements,
                    ..
                }),
            ) => Some(format!(
                "{path} · {bytes_written} bytes · {replacements} replacement"
            )),
            (_, Some(ToolCardResultProjection::MutationTerminal { .. })) => {
                Some(self.status_label().to_string())
            }
            (_, Some(ToolCardResultProjection::InvalidRejected { code, .. })) => {
                Some(code.as_str().to_string())
            }
            (Some(ToolCardInputProjection::ReadOnly { .. }), _) => {
                Some(self.status_label().to_string())
            }
            _ => Some(CORRUPT_LABEL.to_string()),
        }
    }
}

fn projection_output_rows(projection: &ToolCardResultProjection) -> Vec<String> {
    let output = match projection {
        ToolCardResultProjection::Bash { output, .. }
        | ToolCardResultProjection::ReadOnly { output, .. } => output,
        _ => return Vec::new(),
    };
    output.lines().map(str::to_string).collect()
}

fn status_color(status: ToolCallStatus, colors: &ThemeColors) -> gpui::Rgba {
    match status {
        ToolCallStatus::Success => colors.success,
        ToolCallStatus::Rejected | ToolCallStatus::Failed | ToolCallStatus::Cancelled => {
            colors.danger
        }
        ToolCallStatus::PendingApproval | ToolCallStatus::Approved | ToolCallStatus::Running => {
            colors.text_secondary
        }
    }
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
        assert_eq!(card.visible_text(), "write · 已拒绝 malformed_json");
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
        assert_eq!(card.row_count(), 2);
        assert!(card.visible_text().contains("$ printf 'ok'"));
        assert!(!card.visible_text().contains("\nok"));
        card.expanded = true;
        assert_eq!(card.row_count(), 3);
        assert!(card.visible_text().ends_with("\nok"));
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
        assert!(card.visible_text().contains("bash · 失败 · exit 1 · 4ms"));
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
}
