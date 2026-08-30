//! IO-free local branch selector backed only by safe, bounded projections.

use std::ops::Range;

use gpui::prelude::*;
use gpui::{
    App, Context, EventEmitter, FocusHandle, Focusable, MouseButton, Render, ScrollStrategy,
    UniformListScrollHandle, Window, actions, div, px, uniform_list,
};
use vega_conversation::types::{BranchId, BranchItem, BranchSnapshot, GitWorkspaceErrorCode};
use vega_theme::{Typography, theme};

actions!(
    vega_branch_selector,
    [
        ActivateBranch,
        PreviousBranch,
        NextBranch,
        CloseBranchSelector
    ]
);

pub const BRANCH_ROW_HEIGHT: f32 = 24.0;
pub const BRANCH_LIMIT: usize = 10_000;

fn branch_count_allowed(count: usize) -> bool {
    count <= BRANCH_LIMIT
}

/// UI-local exact capability for one pending activation. It contains no ref,
/// OID, path, or repository data and can only be minted by the selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BranchOperationId(u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchListRequested {
    pub thread_id: String,
    pub project_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchSwitchRequested {
    pub thread_id: String,
    pub project_id: String,
    pub snapshot_generation: u64,
    pub branch_id: BranchId,
    pub operation_id: BranchOperationId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchSelectorClosed {
    pub thread_id: String,
    pub project_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectorStatus {
    Closed,
    Loading,
    Ready,
    Empty,
    Failed(GitWorkspaceErrorCode),
}

/// Pure bounded selector state. It stores only the safe headless projection.
pub struct BranchSelectorModel {
    snapshot: Option<BranchSnapshot>,
    current_label: Option<String>,
    status: SelectorStatus,
    focused: Option<BranchId>,
    next_operation: u64,
    pending: Option<(BranchOperationId, u64, BranchId)>,
}

impl Default for BranchSelectorModel {
    fn default() -> Self {
        Self {
            snapshot: None,
            current_label: None,
            status: SelectorStatus::Closed,
            focused: None,
            next_operation: 0,
            pending: None,
        }
    }
}

impl BranchSelectorModel {
    pub fn is_open(&self) -> bool {
        self.status != SelectorStatus::Closed
    }

    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    pub fn snapshot_generation(&self) -> Option<u64> {
        self.snapshot.as_ref().map(|snapshot| snapshot.generation)
    }

    pub fn current_label(&self) -> Option<&str> {
        self.current_label.as_deref()
    }

    pub fn open(&mut self) -> bool {
        if self.is_open() || self.pending.is_some() {
            return false;
        }
        self.snapshot = None;
        self.focused = None;
        self.status = SelectorStatus::Loading;
        true
    }

    pub fn close(&mut self) -> bool {
        if !self.is_open() {
            return false;
        }
        self.status = SelectorStatus::Closed;
        self.focused = None;
        true
    }

    pub fn apply_snapshot(&mut self, snapshot: BranchSnapshot) -> bool {
        if !self.is_open()
            || self.pending.is_some()
            || !branch_count_allowed(snapshot.branches.len())
        {
            if self.is_open() {
                self.snapshot = None;
                self.focused = None;
                self.status = SelectorStatus::Failed(GitWorkspaceErrorCode::OutputTooLarge);
            }
            return false;
        }
        let preserve = self.focused.filter(|id| {
            snapshot
                .branches
                .iter()
                .any(|branch| branch.id == *id && !branch.current)
        });
        self.focused = preserve.or_else(|| {
            snapshot
                .branches
                .iter()
                .find(|branch| !branch.current)
                .map(|branch| branch.id)
        });
        self.status = if snapshot.branches.is_empty() {
            SelectorStatus::Empty
        } else {
            SelectorStatus::Ready
        };
        self.current_label = snapshot
            .branches
            .iter()
            .find(|branch| branch.current)
            .map(|branch| branch.label.clone());
        self.snapshot = Some(snapshot);
        true
    }

    pub fn apply_error(&mut self, code: GitWorkspaceErrorCode) {
        if self.is_open() {
            self.snapshot = None;
            self.focused = None;
            self.status = SelectorStatus::Failed(code);
        }
    }

    pub fn contains_switchable(&self, generation: u64, id: BranchId) -> bool {
        self.snapshot.as_ref().is_some_and(|snapshot| {
            snapshot.generation == generation
                && snapshot
                    .branches
                    .iter()
                    .any(|branch| branch.id == id && !branch.current)
        })
    }

    pub fn begin_switch(&mut self, generation: u64, id: BranchId) -> Option<BranchOperationId> {
        if self.pending.is_some() || !self.contains_switchable(generation, id) {
            return None;
        }
        let sequence = self.next_operation.checked_add(1)?;
        let operation = BranchOperationId(sequence);
        self.next_operation = sequence;
        self.pending = Some((operation, generation, id));
        Some(operation)
    }

    pub fn pending_key(&self) -> Option<(BranchOperationId, u64, BranchId)> {
        self.pending
    }

    pub fn owns_pending(
        &self,
        operation: BranchOperationId,
        generation: u64,
        id: BranchId,
    ) -> bool {
        self.pending == Some((operation, generation, id))
    }

    pub fn finish_switch(
        &mut self,
        operation: BranchOperationId,
        generation: u64,
        id: BranchId,
        snapshot: Option<BranchSnapshot>,
        error: Option<GitWorkspaceErrorCode>,
    ) -> bool {
        if !self.owns_pending(operation, generation, id) {
            return false;
        }
        self.pending = None;
        if !self.is_open() {
            return true;
        }
        if let Some(snapshot) = snapshot
            && !self.apply_snapshot(snapshot)
        {
            return false;
        }
        if let Some(code) = error {
            self.status = SelectorStatus::Failed(code);
        } else {
            self.status = SelectorStatus::Closed;
            self.focused = None;
        }
        true
    }

    pub fn reject_switch(
        &mut self,
        operation: BranchOperationId,
        generation: u64,
        id: BranchId,
        code: GitWorkspaceErrorCode,
    ) -> bool {
        if !self.owns_pending(operation, generation, id) {
            return false;
        }
        self.pending = None;
        if !self.is_open() {
            return true;
        }
        self.status = SelectorStatus::Failed(code);
        true
    }

    pub fn clear_pending(
        &mut self,
        operation: BranchOperationId,
        generation: u64,
        id: BranchId,
    ) -> bool {
        if !self.owns_pending(operation, generation, id) {
            return false;
        }
        self.pending = None;
        true
    }

    pub fn focused(&self) -> Option<BranchId> {
        self.focused
    }

    pub fn move_focus(&mut self, direction: isize) {
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        let current = self.focused.and_then(|focused| {
            snapshot
                .branches
                .iter()
                .position(|branch| branch.id == focused && !branch.current)
        });
        let next = if direction < 0 {
            current.and_then(|index| {
                snapshot.branches[..index]
                    .iter()
                    .rposition(|branch| !branch.current)
            })
        } else {
            let start = current.map_or(0, |index| index.saturating_add(1));
            snapshot.branches[start..]
                .iter()
                .position(|branch| !branch.current)
                .map(|offset| start + offset)
        };
        if let Some(index) = next {
            self.focused = Some(snapshot.branches[index].id);
        } else if self.focused.is_none() {
            self.focused = snapshot
                .branches
                .iter()
                .find(|branch| !branch.current)
                .map(|branch| branch.id);
        }
    }

    pub fn visible_rows(&self, range: Range<usize>) -> Vec<(usize, BranchItem)> {
        self.snapshot.as_ref().map_or_else(Vec::new, |snapshot| {
            range
                .filter_map(|index| {
                    snapshot
                        .branches
                        .get(index)
                        .cloned()
                        .map(|branch| (index, branch))
                })
                .collect()
        })
    }
}

pub struct BranchSelector {
    thread_id: String,
    project_id: String,
    model: BranchSelectorModel,
    disabled: bool,
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
}

impl EventEmitter<BranchListRequested> for BranchSelector {}
impl EventEmitter<BranchSwitchRequested> for BranchSelector {}
impl EventEmitter<BranchSelectorClosed> for BranchSelector {}

impl Focusable for BranchSelector {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl BranchSelector {
    pub fn new(thread_id: String, project_id: String, cx: &mut Context<Self>) -> Self {
        Self {
            thread_id,
            project_id,
            model: BranchSelectorModel::default(),
            disabled: false,
            focus: cx.focus_handle().tab_stop(true),
            scroll: UniformListScrollHandle::new(),
        }
    }

    pub fn route(&self) -> (&str, &str) {
        (&self.thread_id, &self.project_id)
    }

    pub fn is_open(&self) -> bool {
        self.model.is_open()
    }

    pub fn is_pending(&self) -> bool {
        self.model.is_pending()
    }

    pub fn snapshot_generation(&self) -> Option<u64> {
        self.model.snapshot_generation()
    }

    pub fn contains_switchable(&self, generation: u64, id: BranchId) -> bool {
        self.model.contains_switchable(generation, id)
    }

    pub fn pending_key(&self) -> Option<(BranchOperationId, u64, BranchId)> {
        self.model.pending_key()
    }

    pub fn owns_pending(
        &self,
        operation: BranchOperationId,
        generation: u64,
        id: BranchId,
    ) -> bool {
        self.model.owns_pending(operation, generation, id)
    }

    pub fn focused_branch(&self) -> Option<BranchId> {
        self.model.focused()
    }

    pub fn visible_rows(&self, range: Range<usize>) -> Vec<(usize, BranchItem)> {
        self.model.visible_rows(range)
    }

    pub fn set_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        self.disabled = disabled;
        cx.notify();
    }

    /// Opens the read-only list and emits exactly one content-free refresh request.
    pub fn request_open(&mut self, cx: &mut Context<Self>) -> bool {
        if self.disabled || self.model.is_pending() || !self.model.open() {
            return false;
        }
        cx.emit(BranchListRequested {
            thread_id: self.thread_id.clone(),
            project_id: self.project_id.clone(),
        });
        cx.notify();
        true
    }

    pub fn apply_snapshot(&mut self, snapshot: BranchSnapshot, cx: &mut Context<Self>) -> bool {
        let accepted = self.model.apply_snapshot(snapshot);
        cx.notify();
        accepted
    }

    pub fn apply_error(&mut self, code: GitWorkspaceErrorCode, cx: &mut Context<Self>) {
        self.model.apply_error(code);
        cx.notify();
    }

    pub fn begin_switch(
        &mut self,
        generation: u64,
        id: BranchId,
        cx: &mut Context<Self>,
    ) -> Option<BranchOperationId> {
        let operation = self.model.begin_switch(generation, id);
        if operation.is_some() {
            cx.notify();
        }
        operation
    }

    pub fn finish_switch(
        &mut self,
        operation: BranchOperationId,
        generation: u64,
        id: BranchId,
        snapshot: Option<BranchSnapshot>,
        error: Option<GitWorkspaceErrorCode>,
        cx: &mut Context<Self>,
    ) -> bool {
        let accepted = self
            .model
            .finish_switch(operation, generation, id, snapshot, error);
        if accepted {
            cx.notify();
        }
        accepted
    }

    pub fn reject_switch(
        &mut self,
        operation: BranchOperationId,
        generation: u64,
        id: BranchId,
        code: GitWorkspaceErrorCode,
        cx: &mut Context<Self>,
    ) -> bool {
        let accepted = self.model.reject_switch(operation, generation, id, code);
        if accepted {
            cx.notify();
        }
        accepted
    }

    pub fn clear_pending(
        &mut self,
        operation: BranchOperationId,
        generation: u64,
        id: BranchId,
        cx: &mut Context<Self>,
    ) -> bool {
        let accepted = self.model.clear_pending(operation, generation, id);
        if accepted {
            cx.notify();
        }
        accepted
    }

    /// Closes the visible selector without discarding an in-flight request.
    /// The matching controller terminal owns exact pending cleanup.
    pub fn request_close(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.model.close() {
            return false;
        }
        cx.emit(BranchSelectorClosed {
            thread_id: self.thread_id.clone(),
            project_id: self.project_id.clone(),
        });
        cx.notify();
        true
    }

    pub fn close_route(&mut self, code: GitWorkspaceErrorCode, cx: &mut Context<Self>) {
        if self.model.is_pending() {
            self.model.apply_error(code);
        }
        let _ = self.model.close();
        self.disabled = false;
        cx.notify();
    }

    fn toggle(&mut self, _: &gpui::MouseUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled || self.model.is_pending() {
            return;
        }
        if self.model.is_open() {
            let _ = self.request_close(cx);
        } else if self.request_open(cx) {
            window.focus(&self.focus, cx);
        }
    }

    fn activate(&mut self, id: BranchId, cx: &mut Context<Self>) {
        if self.disabled || self.model.is_pending() {
            return;
        }
        let Some(generation) = self.model.snapshot_generation() else {
            return;
        };
        if let Some(operation_id) = self.model.begin_switch(generation, id) {
            cx.emit(BranchSwitchRequested {
                thread_id: self.thread_id.clone(),
                project_id: self.project_id.clone(),
                snapshot_generation: generation,
                branch_id: id,
                operation_id,
            });
            cx.notify();
        }
    }

    fn activate_focused(
        &mut self,
        _: &ActivateBranch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.model.is_open() {
            if self.request_open(cx) {
                window.focus(&self.focus, cx);
            }
            return;
        }
        if let Some(id) = self.model.focused() {
            self.activate(id, cx);
        }
    }

    fn previous(&mut self, _: &PreviousBranch, _: &mut Window, cx: &mut Context<Self>) {
        if self.model.is_open() && !self.model.is_pending() {
            self.model.move_focus(-1);
            if let Some(index) = self.focused_index() {
                self.scroll.scroll_to_item(index, ScrollStrategy::Nearest);
            }
            cx.notify();
        }
    }

    fn next(&mut self, _: &NextBranch, _: &mut Window, cx: &mut Context<Self>) {
        if self.model.is_open() && !self.model.is_pending() {
            self.model.move_focus(1);
            if let Some(index) = self.focused_index() {
                self.scroll.scroll_to_item(index, ScrollStrategy::Nearest);
            }
            cx.notify();
        }
    }

    fn focused_index(&self) -> Option<usize> {
        let focused = self.model.focused()?;
        self.model
            .snapshot
            .as_ref()?
            .branches
            .iter()
            .position(|branch| branch.id == focused)
    }

    fn close_action(&mut self, _: &CloseBranchSelector, _: &mut Window, cx: &mut Context<Self>) {
        let _ = self.request_close(cx);
    }
}

impl Render for BranchSelector {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        let open = self.model.is_open();
        let pending = self.model.is_pending();
        let disabled = self.disabled || pending;
        let label = self.model.current_label().unwrap_or("Branch").to_string();
        let row_count = self
            .model
            .snapshot
            .as_ref()
            .map_or(0, |snapshot| snapshot.branches.len());
        let view = cx.entity().clone();

        div()
            .relative()
            .min_w_0()
            .track_focus(&self.focus)
            .key_context("BranchSelector")
            .on_action(cx.listener(Self::activate_focused))
            .on_action(cx.listener(Self::previous))
            .on_action(cx.listener(Self::next))
            .on_action(cx.listener(Self::close_action))
            .child(
                div()
                    .id("branch-selector-trigger")
                    .h(px(BRANCH_ROW_HEIGHT))
                    .max_w(px(180.0))
                    .min_w_0()
                    .overflow_hidden()
                    .px_2()
                    .flex()
                    .items_center()
                    .rounded_md()
                    .border_1()
                    .border_color(colors.border_subtle)
                    .text_size(px(Typography::SIDEBAR))
                    .text_color(if disabled {
                        colors.text_tertiary
                    } else {
                        colors.text_secondary
                    })
                    .when(!disabled, |trigger| trigger.cursor_pointer())
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::toggle))
                    .child(label),
            )
            .when(open, |root| {
                root.child(
                    div()
                        .absolute()
                        .bottom(px(BRANCH_ROW_HEIGHT + 4.0))
                        .left_0()
                        .w(px(320.0))
                        .max_w_full()
                        .h(px(240.0))
                        .overflow_hidden()
                        .flex()
                        .flex_col()
                        .rounded_md()
                        .border_1()
                        .border_color(colors.border_subtle)
                        .bg(colors.bg_elevated)
                        .text_color(colors.text_primary)
                        .when_some(
                            match self.model.status {
                                SelectorStatus::Failed(code) if row_count > 0 => Some(code),
                                _ => None,
                            },
                            |body, code| {
                                body.child(
                                    div()
                                        .h(px(BRANCH_ROW_HEIGHT))
                                        .flex_shrink_0()
                                        .px_2()
                                        .flex()
                                        .items_center()
                                        .text_size(px(Typography::SIDEBAR))
                                        .text_color(colors.danger)
                                        .child(branch_error_label(code)),
                                )
                            },
                        )
                        .when(row_count > 0, |body| {
                            body.child(
                                uniform_list(
                                    "branch-selector-rows",
                                    row_count,
                                    cx.processor(move |this: &mut BranchSelector, range, _, _| {
                                        this.model
                                            .visible_rows(range)
                                            .into_iter()
                                            .map(|(index, branch)| {
                                                render_branch_row(
                                                    index,
                                                    branch,
                                                    this.model.focused(),
                                                    disabled,
                                                    colors,
                                                    view.clone(),
                                                )
                                            })
                                            .collect()
                                    }),
                                )
                                .track_scroll(&self.scroll)
                                .flex_1()
                                .min_h_0()
                                .w_full(),
                            )
                        })
                        .when(row_count == 0, |body| {
                            let text = match self.model.status {
                                SelectorStatus::Loading => "Loading branches…",
                                SelectorStatus::Empty => "No local branches",
                                SelectorStatus::Failed(code) => branch_error_label(code),
                                SelectorStatus::Closed | SelectorStatus::Ready => {
                                    "No local branches"
                                }
                            };
                            body.flex().items_center().justify_center().child(
                                div()
                                    .text_size(px(Typography::SIDEBAR))
                                    .text_color(colors.text_tertiary)
                                    .child(text),
                            )
                        }),
                )
            })
    }
}

fn render_branch_row(
    index: usize,
    branch: BranchItem,
    focused: Option<BranchId>,
    disabled: bool,
    colors: vega_theme::ThemeColors,
    view: gpui::Entity<BranchSelector>,
) -> impl IntoElement {
    let id = branch.id;
    let current = branch.current;
    div()
        .id(("branch-row", index))
        .h(px(BRANCH_ROW_HEIGHT))
        .flex_shrink_0()
        .min_w_0()
        .overflow_hidden()
        .px_2()
        .flex()
        .items_center()
        .gap_2()
        .text_size(px(Typography::SIDEBAR))
        .text_color(if current || disabled {
            colors.text_tertiary
        } else {
            colors.text_primary
        })
        .when(focused == Some(id), |row| row.bg(colors.bg_hover))
        .when(!current && !disabled, |row| {
            row.cursor_pointer()
                .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                    view.update(cx, |selector, cx| selector.activate(id, cx));
                })
        })
        .child(
            div()
                .min_w_0()
                .flex_1()
                .overflow_hidden()
                .child(branch.label),
        )
        .when(current, |row| {
            row.child(
                div()
                    .flex_shrink_0()
                    .text_color(colors.success)
                    .child("Current"),
            )
        })
}

fn branch_error_label(code: GitWorkspaceErrorCode) -> &'static str {
    match code {
        GitWorkspaceErrorCode::BranchDirty => "Working tree is not clean",
        GitWorkspaceErrorCode::BranchOperationInProgress => "Another Git operation is active",
        GitWorkspaceErrorCode::BranchDetached => "Detached HEAD is unsupported",
        GitWorkspaceErrorCode::BranchUnborn => "Repository has no initial commit",
        GitWorkspaceErrorCode::BranchUnsafeFilter => "Branch contains filtered files",
        GitWorkspaceErrorCode::BranchAlreadyCurrent => "Branch is already current",
        GitWorkspaceErrorCode::TimedOut => "Branch operation timed out",
        GitWorkspaceErrorCode::Cancelled => "Branch operation cancelled",
        GitWorkspaceErrorCode::OutputTooLarge | GitWorkspaceErrorCode::ArtifactLimit => {
            "Too many branches"
        }
        _ => "Branches unavailable",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_selector_fixed_geometry_and_limit_are_exact() {
        assert_eq!(BRANCH_ROW_HEIGHT, 24.0);
        assert_eq!(BRANCH_LIMIT, 10_000);
        assert!(branch_count_allowed(10_000));
        assert!(!branch_count_allowed(10_001));
    }

    #[test]
    fn branch_selector_open_and_close_are_single_shot() {
        let mut model = BranchSelectorModel::default();
        assert!(model.open());
        assert!(!model.open());
        assert!(model.is_open());
        assert!(model.close());
        assert!(!model.close());
        assert!(!model.is_open());
    }

    #[test]
    fn branch_selector_error_is_typed_and_clears_partial_state() {
        let mut model = BranchSelectorModel::default();
        assert!(model.open());
        model.apply_error(GitWorkspaceErrorCode::BranchUnsafeFilter);
        assert_eq!(model.snapshot_generation(), None);
        assert_eq!(model.focused(), None);
        assert!(!model.is_pending());
        assert_eq!(
            branch_error_label(GitWorkspaceErrorCode::BranchUnsafeFilter),
            "Branch contains filtered files"
        );
    }

    #[test]
    fn branch_selector_safe_events_redact_branch_content() {
        let request = BranchListRequested {
            thread_id: "thread-a".into(),
            project_id: "project-a".into(),
        };
        let debug = format!("{request:?}");
        assert!(!debug.contains("refs/"));
        assert!(!debug.contains("oid"));
        assert!(!debug.contains("path"));
    }
}
