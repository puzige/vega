//! IO-free local branch selector backed only by safe, bounded projections.

use std::ops::Range;
mod current_head;
mod host;
mod nsh_view;
use current_head::CurrentHead;

use gpui_kit::prelude::*;
use gpui_kit::{
    Anchor, AnchoredPositionMode, App, Context, Entity, EventEmitter, FocusHandle, Focusable,
    Render, Window, actions,
};
use vega_conversation::types::{BranchId, BranchItem, BranchSnapshot, GitWorkspaceErrorCode};
use vega_theme::{Layout, Typography, theme};

actions!(
    vega_branch_selector,
    [
        ActivateBranch,
        PreviousBranch,
        NextBranch,
        CloseBranchSelector
    ]
);

pub const BRANCH_ROW_HEIGHT: f32 = Typography::SIDEBAR_LINE_HEIGHT;
pub const BRANCH_LIMIT: usize = 10_000;

pub const BRANCH_CHROME_HEIGHT: f32 = (crate::menu_list::MENU_ROW_HEIGHT + 4.0)
    + (4.0 + 1.0 + 4.0)
    + crate::menu_list::MENU_ROW_HEIGHT;

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
    /// Logical Enter candidate. This may be seeded from the first switchable
    /// row before the user has navigated; it must not imply visual keyboard
    /// intent (C1).
    focused: Option<BranchId>,
    /// Branch that has received explicit keyboard navigation in the currently
    /// visible menu. Keeping this separate from `focused` preserves the
    /// existing Enter candidate without painting an unearned second row
    /// highlight on open or refresh.
    visual_focused: Option<BranchId>,
    next_operation: u64,
    pending: Option<(BranchOperationId, u64, BranchId)>,
    filter: String,
    /// Positions into `snapshot.branches` that match `filter`, in snapshot
    /// order. Recomputed only when the snapshot or the filter changes, so a
    /// frame never walks the whole branch list.
    filtered: Vec<usize>,
}

impl Default for BranchSelectorModel {
    fn default() -> Self {
        Self {
            snapshot: None,
            current_label: None,
            status: SelectorStatus::Closed,
            focused: None,
            visual_focused: None,
            next_operation: 0,
            pending: None,
            filter: String::new(),
            filtered: Vec::new(),
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
        self.visual_focused = None;
        self.filter.clear();
        self.filtered.clear();
        self.status = SelectorStatus::Loading;
        true
    }

    pub fn close(&mut self) -> bool {
        if !self.is_open() {
            return false;
        }
        self.status = SelectorStatus::Closed;
        self.focused = None;
        self.visual_focused = None;
        self.filter.clear();
        self.filtered.clear();
        true
    }

    pub fn set_filter(&mut self, query: &str) -> bool {
        let needle = query.trim().to_lowercase();
        if self.filter == needle {
            return false;
        }
        self.filter = needle;
        self.rebuild_filtered();
        true
    }

    pub fn filter(&self) -> &str {
        &self.filter
    }

    fn rebuild_filtered(&mut self) {
        self.filtered = match &self.snapshot {
            None => Vec::new(),
            Some(snapshot) => snapshot
                .branches
                .iter()
                .enumerate()
                .filter(|(_, branch)| {
                    self.filter.is_empty() || branch.label.to_lowercase().contains(&self.filter)
                })
                .map(|(index, _)| index)
                .collect(),
        };
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        let visible_focus = self.focused.is_some_and(|focused| {
            self.filtered.iter().any(|index| {
                snapshot.branches[*index].id == focused && !snapshot.branches[*index].current
            })
        });
        if !visible_focus {
            self.focused = self
                .filtered
                .iter()
                .find(|index| !snapshot.branches[**index].current)
                .map(|index| snapshot.branches[*index].id);
        }
        let visual_visible = self.visual_focused.is_some_and(|focused| {
            self.filtered.iter().any(|index| {
                snapshot.branches[*index].id == focused && !snapshot.branches[*index].current
            })
        });
        if !visual_visible {
            self.visual_focused = None;
        }
    }

    pub fn visible_count(&self) -> usize {
        self.filtered.len()
    }

    pub fn apply_snapshot(&mut self, snapshot: BranchSnapshot) -> bool {
        if !self.is_open()
            || self.pending.is_some()
            || !branch_count_allowed(snapshot.branches.len())
        {
            if self.is_open() {
                self.snapshot = None;
                self.focused = None;
                self.visual_focused = None;
                self.filtered.clear();
                self.status = SelectorStatus::Failed(GitWorkspaceErrorCode::BranchCountLimit);
            }
            return false;
        }
        let preserve = self.focused.filter(|id| {
            snapshot
                .branches
                .iter()
                .any(|branch| branch.id == *id && !branch.current)
        });
        let visual_preserve = self.visual_focused.filter(|id| {
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
        self.visual_focused = visual_preserve;
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
        self.rebuild_filtered();
        true
    }

    pub fn apply_error(&mut self, code: GitWorkspaceErrorCode) {
        if self.is_open() {
            self.snapshot = None;
            self.focused = None;
            self.visual_focused = None;
            self.filtered.clear();
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
            self.visual_focused = None;
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

        let switchable: Vec<usize> = self
            .filtered
            .iter()
            .copied()
            .filter(|index| {
                snapshot
                    .branches
                    .get(*index)
                    .is_some_and(|branch| !branch.current)
            })
            .collect();
        // The first arrow key is the user's intent to enter keyboard
        // navigation, not an instruction to skip the seeded Enter candidate.
        // Reveal that candidate first; subsequent arrows advance normally.
        if self.visual_focused.is_none() {
            if self.focused.is_none() {
                self.focused = switchable.first().map(|index| snapshot.branches[*index].id);
            }
            self.visual_focused = self.focused;
            return;
        }
        let current = self.focused.and_then(|focused| {
            switchable
                .iter()
                .position(|index| snapshot.branches[*index].id == focused)
        });
        let next = if direction < 0 {
            current.and_then(|position| position.checked_sub(1))
        } else {
            match current {
                Some(position) => position.checked_add(1).filter(|p| *p < switchable.len()),
                None => (!switchable.is_empty()).then_some(0),
            }
        };
        if let Some(position) = next {
            self.focused = Some(snapshot.branches[switchable[position]].id);
        } else if self.focused.is_none() {
            self.focused = switchable.first().map(|index| snapshot.branches[*index].id);
        }
        // Arrow-key navigation is the evidence needed to paint focus. The
        // logical candidate above remains available to Enter even while this
        // is `None` on an untouched opening frame.
        self.visual_focused = self.focused;
    }

    pub fn focused_position(&self) -> Option<usize> {
        let focused = self.focused?;
        let snapshot = self.snapshot.as_ref()?;
        self.filtered
            .iter()
            .position(|index| snapshot.branches[*index].id == focused)
    }

    pub fn visible_rows(&self, range: Range<usize>) -> Vec<(usize, BranchItem)> {
        let Some(snapshot) = self.snapshot.as_ref() else {
            return Vec::new();
        };

        range
            .filter_map(|position| {
                let index = *self.filtered.get(position)?;
                snapshot
                    .branches
                    .get(index)
                    .cloned()
                    .map(|branch| (index, branch))
            })
            .collect()
    }
}

pub struct BranchSelector {
    current_head: CurrentHead,
    thread_id: String,
    project_id: String,
    model: BranchSelectorModel,
    disabled: bool,
    menu_below: bool,
    chip_chrome: bool,
    focus: FocusHandle,
    presentation: host::State,
    search: Entity<crate::text_input::TextInput>,
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
        cx.spawn(async move |this, cx| {
            loop {
                if this
                    .update(cx, |this, cx| {
                        this.poll_current_head(cx);
                        this.poll_presentation(cx);
                    })
                    .is_err()
                {
                    break;
                }
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(100))
                    .await;
            }
        })
        .detach();

        let search = cx.new(|cx| {
            crate::text_input::TextInput::new(cx, "", false)
                .with_placeholder_key("ui.search_branches")
                .with_bare_chrome()
        });
        cx.observe(&search, |this, input, cx| {
            if this.model.set_filter(input.read(cx).text()) {
                this.presentation.invalidate_rows();
            }
            cx.notify();
        })
        .detach();
        Self {
            current_head: CurrentHead::new(cx.background_executor().clone()),
            thread_id,
            project_id,
            model: BranchSelectorModel::default(),
            disabled: false,
            menu_below: false,
            // Default stays the bordered pill every existing mount point
            // (Environment card, tests) already renders.
            chip_chrome: false,
            focus: cx.focus_handle().tab_stop(true),
            presentation: host::State::default(),
            search,
        }
    }

    /// Places the popup below a header trigger, or above a composer trigger.
    pub fn set_menu_below(&mut self, below: bool) {
        self.menu_below = below;
    }

    pub fn set_chip_chrome(&mut self, chip: bool) {
        self.chip_chrome = chip;
    }

    pub fn route(&self) -> (&str, &str) {
        (&self.thread_id, &self.project_id)
    }

    /// Changes the project of the window-owned unmaterialized draft while
    /// retaining this entity and its event subscriptions. Old snapshots and
    /// pending operations cannot be reused for the new repository.
    pub fn rebind_draft_project(&mut self, project_id: &str, cx: &mut Context<Self>) {
        if self.project_id == project_id {
            return;
        }
        self.project_id = project_id.to_owned();
        self.presentation.reset();
        self.model = BranchSelectorModel::default();
        self.current_head.reset_route();
        self.disabled = false;
        self.search.update(cx, |search, cx| search.clear(cx));
        cx.notify();
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

    pub fn visible_rows(&self, range: Range<usize>) -> Vec<(usize, BranchItem)> {
        self.model.visible_rows(range)
    }

    pub fn visible_count(&self) -> usize {
        self.model.visible_count()
    }

    pub fn filter(&self) -> &str {
        self.model.filter()
    }

    pub fn search_input(&self) -> Entity<crate::text_input::TextInput> {
        self.search.clone()
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
        self.presentation.reconcile_open(&self.model);
        self.search
            .update(cx, |input, cx| input.set_text(self.model.filter(), cx));
        self.presentation.invalidate_rows();
        cx.emit(BranchListRequested {
            thread_id: self.thread_id.clone(),
            project_id: self.project_id.clone(),
        });
        cx.notify();
        true
    }

    pub fn apply_snapshot(&mut self, snapshot: BranchSnapshot, cx: &mut Context<Self>) -> bool {
        let accepted = self.model.apply_snapshot(snapshot);
        self.presentation.invalidate_rows();
        cx.notify();
        accepted
    }

    pub fn apply_error(&mut self, code: GitWorkspaceErrorCode, cx: &mut Context<Self>) {
        self.model.apply_error(code);
        self.presentation.invalidate_rows();
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
            self.current_head.invalidate();
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
            self.presentation.invalidate_rows();
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
        self.presentation.close_popup();
        self.presentation.invalidate_rows();
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
            self.presentation.invalidate_rows();
        }
        let _ = self.model.close();
        self.presentation.close_popup();
        self.presentation.invalidate_rows();
        self.disabled = false;
        cx.notify();
    }

    fn activate(&mut self, id: BranchId, cx: &mut Context<Self>) {
        if self.disabled || self.model.is_pending() {
            return;
        }
        let Some(generation) = self.model.snapshot_generation() else {
            return;
        };
        if let Some(operation_id) = self.model.begin_switch(generation, id) {
            self.current_head.invalidate();
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

    fn previous(&mut self, _: &PreviousBranch, _: &mut Window, cx: &mut Context<Self>) {
        if self.model.is_open() && !self.model.is_pending() {
            self.model.move_focus(-1);
            cx.notify();
        }
    }

    fn next(&mut self, _: &NextBranch, _: &mut Window, cx: &mut Context<Self>) {
        if self.model.is_open() && !self.model.is_pending() {
            self.model.move_focus(1);
            cx.notify();
        }
    }
}

impl Render for BranchSelector {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_nsh(window, cx)
    }
}

fn branch_error_label(code: GitWorkspaceErrorCode) -> &'static str {
    match code {
        GitWorkspaceErrorCode::BranchDirty => vega_i18n::t("ui.working_tree_is_not_clean"),
        GitWorkspaceErrorCode::BranchOperationInProgress => {
            vega_i18n::t("ui.another_git_operation_is_active")
        }
        GitWorkspaceErrorCode::BranchDetached => vega_i18n::t("ui.detached_head_is_unsupported"),
        GitWorkspaceErrorCode::BranchUnborn => vega_i18n::t("ui.repository_has_no_initial_commit"),
        GitWorkspaceErrorCode::BranchUnsafeFilter => {
            vega_i18n::t("ui.branch_contains_filtered_files")
        }
        GitWorkspaceErrorCode::BranchAlreadyCurrent => vega_i18n::t("ui.branch_is_already_current"),
        GitWorkspaceErrorCode::GitUnavailable => {
            vega_i18n::t("ui.git_2_40_was_not_found_install_homebrew_en")
        }
        GitWorkspaceErrorCode::GitUnsupported => {
            vega_i18n::t("ui.git_2_40_is_required_upgrade_git_and_en")
        }
        GitWorkspaceErrorCode::GitExecutableChanged => {
            vega_i18n::t("ui.git_changed_while_vega_was_running_restart_vega_en")
        }
        GitWorkspaceErrorCode::TimedOut => vega_i18n::t("ui.branch_operation_timed_out"),
        GitWorkspaceErrorCode::Cancelled => vega_i18n::t("ui.branch_operation_cancelled"),
        GitWorkspaceErrorCode::BranchPathLimit => {
            vega_i18n::t("ui.branch_listing_supports_at_most_25_000_tracked")
        }
        GitWorkspaceErrorCode::BranchCountLimit => {
            vega_i18n::t("ui.too_many_local_branches_to_display_10_000")
        }
        GitWorkspaceErrorCode::OutputTooLarge | GitWorkspaceErrorCode::ArtifactLimit => {
            vega_i18n::t("ui.branch_data_exceeded_a_safety_limit_reduce_repository")
        }
        _ => vega_i18n::t("ui.branches_unavailable"),
    }
}
