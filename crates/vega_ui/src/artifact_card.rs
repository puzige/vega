//! Compact, safe artifact cards embedded in the virtual conversation stream.

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
    MouseButton, Window, actions, div, px,
};
use vega_conversation::types::{
    ArtifactCard as ArtifactProjection, ArtifactCardId, ArtifactPreviewProjection, ArtifactSource,
    GitWorkspaceErrorCode, OpenInOutcome, OpenInTarget, WorkspaceFileId,
};
use vega_theme::{ThemeColors, Typography, theme};

use crate::conversation_stream::{MONOFONT, ROW_HEIGHT};

actions!(vega_artifact_card, [ArtifactActivate, ArtifactClear]);

const OPEN_TARGETS: [(OpenInTarget, &str); 6] = [
    (OpenInTarget::VisualStudioCode, "VS Code"),
    (OpenInTarget::Cursor, "Cursor"),
    (OpenInTarget::Zed, "Zed"),
    (OpenInTarget::Terminal, "Terminal"),
    (OpenInTarget::DefaultApplication, "Default"),
    (OpenInTarget::RevealInFinder, "Finder"),
];

/// Explicit request for the sole bounded artifact body channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactPreviewRequested {
    pub thread_id: String,
    pub project_id: String,
    pub card_id: ArtifactCardId,
    pub file_id: WorkspaceFileId,
}

/// Explicit request for one fixed external handoff target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactOpenRequested {
    pub thread_id: String,
    pub project_id: String,
    pub card_id: ArtifactCardId,
    pub file_id: WorkspaceFileId,
    pub target: OpenInTarget,
}

/// Clears preview/inline state and cancels requests without deleting history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactCleared {
    pub thread_id: String,
    pub project_id: String,
    pub card_id: ArtifactCardId,
}

/// Route-owned presentation state; it never reads a path or filesystem.
pub struct ArtifactCard {
    thread_id: String,
    project_id: String,
    projection: ArtifactProjection,
    preview_lines: Vec<String>,
    inline_error: Option<GitWorkspaceErrorCode>,
    opening: Option<OpenInTarget>,
    focus: [FocusHandle; 7],
}

impl EventEmitter<ArtifactPreviewRequested> for ArtifactCard {}
impl EventEmitter<ArtifactOpenRequested> for ArtifactCard {}
impl EventEmitter<ArtifactCleared> for ArtifactCard {}

impl Focusable for ArtifactCard {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus[0].clone()
    }
}

impl ArtifactCard {
    pub fn new(
        thread_id: String,
        project_id: String,
        projection: ArtifactProjection,
        cx: &mut Context<Self>,
    ) -> Self {
        let preview_enabled = projection.preview_available && projection.current_file_id.is_some();
        Self {
            thread_id,
            project_id,
            projection,
            preview_lines: Vec::new(),
            inline_error: None,
            opening: None,
            focus: std::array::from_fn(|index| {
                cx.focus_handle()
                    .tab_index(index as isize + 1)
                    .tab_stop(index != 0 || preview_enabled)
            }),
        }
    }

    pub fn id(&self) -> ArtifactCardId {
        self.projection.id
    }

    pub fn projection(&self) -> &ArtifactProjection {
        &self.projection
    }

    #[doc(hidden)]
    pub fn inline_error_code(&self) -> Option<GitWorkspaceErrorCode> {
        self.inline_error
    }

    pub fn row_count(&self) -> usize {
        2 + self.preview_lines.len() + usize::from(self.inline_error.is_some())
    }

    pub fn apply_metadata(
        &mut self,
        projection: ArtifactProjection,
        cx: &mut Context<Self>,
    ) -> bool {
        if projection.id != self.projection.id {
            return false;
        }
        let capability_changed = projection.current_file_id != self.projection.current_file_id
            || !projection.preview_available;
        self.projection = projection;
        if capability_changed {
            self.preview_lines.clear();
            self.inline_error = None;
            self.opening = None;
        }
        cx.notify();
        true
    }

    pub fn apply_preview(
        &mut self,
        preview: ArtifactPreviewProjection,
        cx: &mut Context<Self>,
    ) -> bool {
        if preview.card_id() != self.projection.id
            || self.projection.current_file_id != Some(preview.file_id())
            || !self.projection.preview_available
        {
            return false;
        }
        self.preview_lines = preview_rows(preview.text());
        self.inline_error = None;
        cx.notify();
        true
    }

    pub fn apply_preview_error(
        &mut self,
        card_id: ArtifactCardId,
        file_id: WorkspaceFileId,
        code: GitWorkspaceErrorCode,
        cx: &mut Context<Self>,
    ) -> bool {
        if card_id != self.projection.id || self.projection.current_file_id != Some(file_id) {
            return false;
        }
        self.preview_lines.clear();
        self.inline_error = Some(code);
        cx.notify();
        true
    }

    pub fn set_opening(&mut self, target: Option<OpenInTarget>, cx: &mut Context<Self>) {
        self.opening = target;
        if target.is_some() {
            self.inline_error = None;
        }
        cx.notify();
    }

    pub fn apply_open_outcome(&mut self, outcome: OpenInOutcome, cx: &mut Context<Self>) -> bool {
        if outcome.card_id != self.projection.id || self.opening != Some(outcome.target) {
            return false;
        }
        self.opening = None;
        self.inline_error = None;
        cx.notify();
        true
    }

    pub fn apply_open_error(
        &mut self,
        card_id: ArtifactCardId,
        target: OpenInTarget,
        code: GitWorkspaceErrorCode,
        cx: &mut Context<Self>,
    ) -> bool {
        if card_id != self.projection.id || self.opening != Some(target) {
            return false;
        }
        self.opening = None;
        self.inline_error = Some(code);
        cx.notify();
        true
    }

    pub fn fail_corrupt(&mut self, cx: &mut Context<Self>) {
        self.preview_lines.clear();
        self.opening = None;
        self.inline_error = Some(GitWorkspaceErrorCode::ArtifactConflict);
        cx.notify();
    }

    /// Cancels one UI-local operation while retaining the current capability.
    pub fn fail_request(&mut self, code: GitWorkspaceErrorCode, cx: &mut Context<Self>) {
        self.preview_lines.clear();
        self.opening = None;
        self.inline_error = Some(code);
        cx.notify();
    }

    /// Permanently disables a historical card after its owning route closes.
    pub fn invalidate(&mut self, code: GitWorkspaceErrorCode, cx: &mut Context<Self>) {
        self.projection.current_file_id = None;
        self.projection.preview_available = false;
        self.fail_request(code, cx);
    }

    fn preview(&mut self, cx: &mut Context<Self>) {
        if !self.projection.preview_available || self.opening.is_some() {
            return;
        }
        let Some(file_id) = self.projection.current_file_id else {
            return;
        };
        self.inline_error = None;
        cx.emit(ArtifactPreviewRequested {
            thread_id: self.thread_id.clone(),
            project_id: self.project_id.clone(),
            card_id: self.projection.id,
            file_id,
        });
        cx.notify();
    }

    fn open(&mut self, target: OpenInTarget, cx: &mut Context<Self>) {
        if self.opening.is_some() {
            return;
        }
        let Some(file_id) = self.projection.current_file_id else {
            return;
        };
        self.opening = Some(target);
        self.inline_error = None;
        cx.emit(ArtifactOpenRequested {
            thread_id: self.thread_id.clone(),
            project_id: self.project_id.clone(),
            card_id: self.projection.id,
            file_id,
            target,
        });
        cx.notify();
    }

    fn clear(&mut self, cx: &mut Context<Self>) {
        self.preview_lines.clear();
        self.inline_error = None;
        self.opening = None;
        cx.emit(ArtifactCleared {
            thread_id: self.thread_id.clone(),
            project_id: self.project_id.clone(),
            card_id: self.projection.id,
        });
        cx.notify();
    }

    fn activate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focused = self
            .focus
            .iter()
            .position(|handle| handle.is_focused(window));
        match focused {
            Some(0) => self.preview(cx),
            Some(index) => self.open(OPEN_TARGETS[index - 1].0, cx),
            None if self.projection.preview_available => self.preview(cx),
            None => self.open(OPEN_TARGETS[0].0, cx),
        }
    }

    pub fn render_row(
        card: Entity<Self>,
        row: usize,
        _window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let (projection, preview_lines, inline_error, opening, focus) = {
            let card = card.read(cx);
            (
                card.projection.clone(),
                card.preview_lines.clone(),
                card.inline_error,
                card.opening,
                card.focus.clone(),
            )
        };
        let base = div()
            .h(px(ROW_HEIGHT))
            .w_full()
            .min_w_0()
            .flex()
            .items_center()
            .overflow_hidden()
            .px_3()
            .bg(colors.bg_elevated)
            .border_color(colors.border_subtle)
            .border_l_1()
            .border_r_1();
        if row == 0 {
            return base
                .border_t_1()
                .rounded_tl_lg()
                .rounded_tr_lg()
                .gap_2()
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .truncate()
                        .font_family(MONOFONT.to_string())
                        .text_size(px(Typography::CODE))
                        .child(projection.label),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .text_size(px(Typography::SIDEBAR))
                        .text_color(source_color(projection.source, &colors))
                        .font_weight(FontWeight::MEDIUM)
                        .child(source_label(projection.source)),
                )
                .into_any_element();
        }
        if row == 1 {
            let preview_enabled = projection.preview_available
                && projection.current_file_id.is_some()
                && opening.is_none();
            let open_enabled = projection.current_file_id.is_some() && opening.is_none();
            let preview_card = card.clone();
            let activate_card = card.clone();
            let clear_card = card.clone();
            let mut actions = div()
                .w_full()
                .min_w_0()
                .flex()
                .gap_1()
                .key_context("ArtifactCard")
                .on_action(move |_: &ArtifactActivate, window, cx| {
                    activate_card.update(cx, |card, cx| card.activate(window, cx));
                })
                .on_action(move |_: &ArtifactClear, _, cx| {
                    clear_card.update(cx, ArtifactCard::clear);
                })
                .child(
                    artifact_button("Preview", preview_enabled, focus[0].clone(), colors)
                        .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                            preview_card.update(cx, ArtifactCard::preview);
                        }),
                );
            for (index, (target, label)) in OPEN_TARGETS.iter().copied().enumerate() {
                let open_card = card.clone();
                actions = actions.child(
                    artifact_button(label, open_enabled, focus[index + 1].clone(), colors)
                        .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                            open_card.update(cx, |card, cx| card.open(target, cx));
                        }),
                );
            }
            return base
                .when(preview_lines.is_empty() && inline_error.is_none(), |row| {
                    row.border_b_1().rounded_bl_lg().rounded_br_lg()
                })
                .child(actions)
                .into_any_element();
        }
        let preview_index = row - 2;
        if let Some(line) = preview_lines.get(preview_index) {
            return base
                .bg(colors.code_bg)
                .font_family(MONOFONT.to_string())
                .text_size(px(Typography::CODE))
                .text_color(colors.text_primary)
                .child(div().min_w_0().w_full().truncate().child(line.clone()))
                .into_any_element();
        }
        base.border_b_1()
            .rounded_bl_lg()
            .rounded_br_lg()
            .text_size(px(Typography::SIDEBAR))
            .text_color(colors.danger)
            .child(error_label(inline_error))
            .into_any_element()
    }
}

fn artifact_button(
    label: &'static str,
    enabled: bool,
    focus: FocusHandle,
    colors: ThemeColors,
) -> gpui::Div {
    div()
        .track_focus(&focus)
        .flex_shrink_0()
        .px_2()
        .rounded_md()
        .border_1()
        .border_color(colors.border_subtle)
        .text_size(px(Typography::SIDEBAR))
        .when(enabled, |button| {
            button
                .cursor_pointer()
                .text_color(colors.text_primary)
                .hover(move |style| style.bg(colors.bg_hover))
        })
        .when(!enabled, |button| button.text_color(colors.text_tertiary))
        .child(label)
}

fn source_label(source: ArtifactSource) -> &'static str {
    match source {
        ArtifactSource::AgentArtifact => "agent artifact",
        ArtifactSource::WorkspaceChange => "workspace change",
    }
}

fn source_color(source: ArtifactSource, colors: &ThemeColors) -> gpui::Rgba {
    match source {
        ArtifactSource::AgentArtifact => colors.success,
        ArtifactSource::WorkspaceChange => colors.text_secondary,
    }
}

fn error_label(code: Option<GitWorkspaceErrorCode>) -> String {
    code.map_or_else(String::new, |code| {
        format!("Artifact unavailable ({})", code.as_str())
    })
}

fn preview_rows(text: &str) -> Vec<String> {
    text.split_inclusive('\n')
        .map(|line| line.strip_suffix('\n').unwrap_or(line).to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_open_targets_are_exact_and_ordered() {
        assert_eq!(
            OPEN_TARGETS,
            [
                (OpenInTarget::VisualStudioCode, "VS Code"),
                (OpenInTarget::Cursor, "Cursor"),
                (OpenInTarget::Zed, "Zed"),
                (OpenInTarget::Terminal, "Terminal"),
                (OpenInTarget::DefaultApplication, "Default"),
                (OpenInTarget::RevealInFinder, "Finder"),
            ]
        );
    }

    #[test]
    fn source_labels_are_authoritative_and_closed() {
        assert_eq!(
            source_label(ArtifactSource::AgentArtifact),
            "agent artifact"
        );
        assert_eq!(
            source_label(ArtifactSource::WorkspaceChange),
            "workspace change"
        );
    }

    #[test]
    fn errors_are_typed_and_content_free() {
        let label = error_label(Some(GitWorkspaceErrorCode::MetadataOnly));
        assert_eq!(label, "Artifact unavailable (metadata_only)");
        assert!(!label.contains('/'));
    }

    #[test]
    fn preview_rows_match_headless_line_semantics() {
        assert!(preview_rows("").is_empty());
        assert_eq!(preview_rows("x\n"), ["x"]);
        assert_eq!(preview_rows("x\ny"), ["x", "y"]);
        assert_eq!(preview_rows(&"x\n".repeat(10_000)).len(), 10_000);
    }
}
