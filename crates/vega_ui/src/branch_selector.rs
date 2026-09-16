//! IO-free local branch selector backed only by safe, bounded projections.

use std::ops::Range;
mod current_head;
use current_head::CurrentHead;

use gpui_kit::prelude::*;
use gpui_kit::{
    Anchor, AnchoredPositionMode, App, Context, Entity, EventEmitter, FocusHandle, Focusable,
    MouseButton, MouseDownEvent, Render, ScrollStrategy, UniformListScrollHandle, Window, actions,
    anchored, div, point, px, uniform_list,
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

/// R62 R10: the fixed vertical band the popup spends on everything that is not
/// a branch row — the search field ([`MENU_ROW_HEIGHT`] plus its 4px bottom
/// margin), the separator above the trailing actions (1px plus 4px on each
/// side) and the trailing action row ([`MENU_ROW_HEIGHT`]).
///
/// It is a named sum rather than a magic number so the popup's height budget
/// and the elements it hosts cannot drift apart: change any band and the
/// popup's row capacity follows.
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
    /// R62 R10: the visible row filter, as a lowercase needle. It is applied
    /// to the already-bounded snapshot, so it can only hide rows — it never
    /// widens what the selector can reach, and `focused`/`contains_switchable`
    /// keep answering from the full snapshot (R62 R11).
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

    /// R62 R10: replaces the visible filter. The snapshot, the focus target and
    /// every switch capability are untouched — only which rows this frame
    /// lists changes (R62 R11).
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

    /// Recomputes the visible positions. Runs on snapshot/filter changes only.
    ///
    /// If the filter just hid the focused branch, the focus re-anchors to the
    /// first **visible** switchable row; if the filter hid every switchable
    /// row, the focus is cleared outright, because Enter must never activate a
    /// row the user cannot see. The switch capability itself is untouched —
    /// `contains_switchable` still validates against the full snapshot, so the
    /// set of branches the selector will actually switch to is unchanged
    /// (R62 R11).
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

    /// Number of rows the list currently shows (R62 R10: the filtered count,
    /// which is what the virtualized list is sized from).
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
        // R62 R10: the walk runs over the **visible** (filtered) projection, so
        // arrow keys cannot land on a row the filter hid. With an empty filter
        // `filtered` is `0..len`, which reproduces the pre-R62 walk exactly.
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

    /// R62 R10: where the focused branch sits in the **visible** projection —
    /// the coordinate `UniformListScrollHandle::scroll_to_item` speaks now that
    /// the list is sized from the filtered count.
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
        // R62 R10: the range indexes the **filtered** projection, and the
        // returned position is the snapshot index, which is what
        // `focused_index` and the scroll handle speak. With an empty filter
        // `filtered[i] == i`, so the pre-R62 behaviour is bit-identical.
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
    scroll: UniformListScrollHandle,
    /// R62 R10: the popup's search field. An entity (not a string) because the
    /// field is a real editable input with IME support; its text is mirrored
    /// into `model.filter`, which only hides rows.
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
                    .update(cx, |this, cx| this.poll_current_head(cx))
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
        // R62 R10: the popup's search field. Bare chrome, because the row
        // around it draws the surface; its text mirrors into `model.filter`,
        // which only decides which rows the list shows (R62 R11).
        let search = cx
            .new(|cx| crate::text_input::TextInput::new(cx, "搜索分支", false).with_bare_chrome());
        cx.observe(&search, |this, input, cx| {
            if this.model.set_filter(input.read(cx).text()) {
                this.scroll.scroll_to_item(0, ScrollStrategy::Top);
                cx.notify();
            }
        })
        .detach();
        Self {
            current_head: CurrentHead::new(),
            thread_id,
            project_id,
            model: BranchSelectorModel::default(),
            disabled: false,
            menu_below: false,
            // Default stays the bordered pill every existing mount point
            // (Environment card, tests) already renders.
            chip_chrome: false,
            focus: cx.focus_handle().tab_stop(true),
            scroll: UniformListScrollHandle::new(),
            search,
        }
    }

    /// Places the popup below a header trigger, or above a composer trigger.
    pub fn set_menu_below(&mut self, below: bool) {
        self.menu_below = below;
    }

    /// Switches the trigger chrome to the R49 composer utility-bar chip
    /// (borderless, transparent, hover surface only). Only the trigger's
    /// decoration changes: open/close, switching, pending, error codes,
    /// focus, scrolling and popup anchoring are untouched.
    pub fn set_chip_chrome(&mut self, chip: bool) {
        self.chip_chrome = chip;
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

    /// R62 R10: how many rows the popup currently lists (the filtered count).
    pub fn visible_count(&self) -> usize {
        self.model.visible_count()
    }

    /// R62 R10: the active row filter, as the normalized needle.
    pub fn filter(&self) -> &str {
        self.model.filter()
    }

    /// R62 R10: the popup's search field, so callers (and tests) can drive the
    /// one real input rather than a parallel string.
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

    fn toggle(&mut self, _: &gpui_kit::MouseUpEvent, window: &mut Window, cx: &mut Context<Self>) {
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
        // R62 R10: the list is sized from the filtered count, so the scroll
        // coordinate is the focused branch's position in the **visible**
        // projection, not its snapshot index.
        self.model.focused_position()
    }

    fn close_action(&mut self, _: &CloseBranchSelector, _: &mut Window, cx: &mut Context<Self>) {
        let _ = self.request_close(cx);
    }
}

impl Render for BranchSelector {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        let open = self.model.is_open();
        let pending = self.model.is_pending();
        let disabled = self.disabled || pending;
        let label = self.current_head_label().to_string();
        let non_git = matches!(
            self.current_head.state,
            Some(vega_conversation::types::ProjectBranchState::NonGit)
        );
        let row_count = self
            .model
            .snapshot
            .as_ref()
            .map_or(0, |snapshot| snapshot.branches.len());
        let filtered_count = self.model.visible_count();
        let view = cx.entity().clone();
        let banner_rows =
            usize::from(row_count > 0 && matches!(self.model.status, SelectorStatus::Failed(_)));
        let popup_width = px(320.0).min((window.viewport_size().width - px(16.0)).max(px(1.0)));
        // R62 R10: the popup now also carries the search row, the separator
        // and the trailing action row, so the row-count height gains exactly
        // those bands ([`BRANCH_CHROME_HEIGHT`]) and nothing else. R11's 240px
        // cap and the window bound are unchanged; the body simply scrolls
        // inside them.
        let popup_height = px(
            (filtered_count.max(1) + banner_rows) as f32 * BRANCH_ROW_HEIGHT + BRANCH_CHROME_HEIGHT,
        )
        .min(px(240.0))
        .min((window.viewport_size().height - px(16.0)).max(px(1.0)));

        div()
            .when(non_git, |root| root.hidden())
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
                    .debug_selector({
                        let label = label.clone();
                        move || format!("branch-current-{label}")
                    })
                    // R68 R3: the trigger claims the mouse-down in the
                    // **capture** phase, exactly like the composer's project
                    // chip. `toggle` below runs on mouse-up, so a bubble-phase
                    // `stop_propagation` would arrive too late to keep the
                    // popup's capture-phase out-handler (R1) from closing it,
                    // and the following mouse-up would reopen it — the trigger
                    // could never close its own popup (R68 §2). R6/R15: this
                    // changes no geometry and leaves the R64 `deferred` wrap
                    // untouched.
                    .capture_any_mouse_down(|_, _, cx| cx.stop_propagation())
                    .h(px(BRANCH_ROW_HEIGHT))
                    .max_w(px(180.0))
                    .min_w_0()
                    .overflow_hidden()
                    .flex()
                    .items_center()
                    // R49: the R19 bordered pill is the default chrome; the
                    // composer utility bar mounts the same selector as a
                    // borderless chip, so the mount point decides. The
                    // disabled affordance (§8) is identical in both.
                    .when(!self.chip_chrome, |trigger| {
                        trigger
                            .rounded_md()
                            .px_2()
                            .border_1()
                            .border_color(colors.border_subtle)
                    })
                    .when(self.chip_chrome, |trigger| {
                        trigger
                            .h(px(Layout::COMPOSER_UTILITY_CHIP_HEIGHT))
                            .rounded_full()
                            .gap_2()
                            .px(px(Layout::COMPOSER_UTILITY_CHIP_PADDING_X))
                            .text_color(if disabled {
                                colors.text_tertiary
                            } else {
                                colors.text_primary
                            })
                    })
                    .text_size(px(Typography::SIDEBAR))
                    .when(!self.chip_chrome, |trigger| {
                        trigger.text_color(if disabled {
                            colors.text_tertiary
                        } else {
                            colors.text_secondary
                        })
                    })
                    .when(self.chip_chrome && open, |trigger| {
                        trigger.bg(colors.bg_utility_chip_overlay).rounded_full()
                    })
                    .when(self.chip_chrome && !disabled, |trigger| {
                        trigger.cursor_pointer().hover(move |style| {
                            style.bg(colors.bg_utility_chip_overlay).rounded_full()
                        })
                    })
                    .when(!self.chip_chrome && !disabled, |trigger| {
                        trigger.cursor_pointer()
                    })
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::toggle))
                    .when(self.chip_chrome, |trigger| {
                        // R49 §2.3 chip structure: 16px icon in the secondary
                        // ink, label in the primary ink (tertiary while
                        // disabled, matching the pill's §8 degradation).
                        trigger.child(
                            div()
                                .debug_selector(|| "composer-utility-branch-icon".into())
                                .flex_shrink_0()
                                .child(crate::icons::icon(
                                    crate::icons::Icon::GitBranch,
                                    if disabled {
                                        colors.text_tertiary
                                    } else {
                                        colors.text_secondary
                                    },
                                )),
                        )
                    })
                    .child(label),
            )
            .when(open, |root| {
                let popup = div()
                    // R68 R1/R2/R4/R5: the popup closes when the pointer goes
                    // down **outside** it. `on_mouse_down_out` fires in the
                    // capture phase and only when the pointer is outside the
                    // element's own bounds, so a click on the popup's own
                    // surface — the search field, a row, the trailing action —
                    // leaves it open (R4) with no extra "am I inside?" logic.
                    // The close goes through the selector's existing
                    // [`BranchSelector::request_close`] (R2): it also emits
                    // `BranchSelectorClosed`, whose pending cleanup belongs to
                    // the controller, so the handler must not mutate
                    // `model.status` directly. R5: this touches only this
                    // popup's state; the project popup's flag lives in
                    // `ConversationStream`, not here.
                    .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        let _ = this.request_close(cx);
                    }))
                    // R68: the popup's own selector, so a test can tell "the
                    // pointer is outside the popup" from "the pointer is on the
                    // trigger" without re-deriving the popup's box. The
                    // `anchored` layer keeps this deferred surface attached to
                    // the trigger while window-margin constraints are applied,
                    // so the two boxes are measured independently.
                    .debug_selector(|| "branch-selector-popup".into())
                    .w(popup_width)
                    .flex_shrink_0()
                    .h(popup_height)
                    .occlude()
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .rounded(px(Layout::MENU_RADIUS))
                    .bg(colors.bg_elevated)
                    .text_color(colors.text_primary)
                    .shadow_sm()
                    // R62 R10: the search field, then the (filtered) rows.
                    // `filtered_count` is what the virtualized list is sized
                    // from, so a filter shortens the list instead of leaving
                    // blank measured space behind.
                    .child(crate::menu_list::search_field(
                        &self.search,
                        "branch-selector-search",
                        colors,
                    ))
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
                    .when(filtered_count > 0, |body| {
                        body.child(
                            uniform_list(
                                "branch-selector-rows",
                                filtered_count,
                                cx.processor(move |this: &mut BranchSelector, range, _, _| {
                                    this.model
                                        .visible_rows(range)
                                        .into_iter()
                                        .map(|(index, branch)| {
                                            render_branch_row(
                                                index,
                                                branch,
                                                this.model.visual_focused,
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
                            // R4: uniform_list lays each item out against its
                            // padded content width.  Keep the menu's 4px
                            // card-edge gutter in the list itself; applying
                            // `mx_1` to a full-width item is ignored by the
                            // virtualized root and makes the selected surface
                            // touch the popup edge.
                            .px_1()
                            .w_full(),
                        )
                    })
                    .when(filtered_count == 0, |body| {
                        let text = if row_count > 0 {
                            // R62 R10: the filter hid every row.
                            "没有匹配分支"
                        } else {
                            match self.model.status {
                                SelectorStatus::Loading => "Loading branches…",
                                SelectorStatus::Empty => "No local branches",
                                SelectorStatus::Failed(code) => branch_error_label(code),
                                SelectorStatus::Closed | SelectorStatus::Ready => {
                                    "No local branches"
                                }
                            }
                        };
                        body.flex().items_center().justify_center().child(
                            div()
                                .text_size(px(Typography::SIDEBAR))
                                .text_color(colors.text_tertiary)
                                .child(text),
                        )
                    })
                    // R62 R10: the trailing action group. `+ 新建并切换分支`
                    // has no Vega implementation path — the headless branch
                    // service exposes refresh / prepare_switch / execute_switch
                    // only, and no caller may invent a Git command outside it —
                    // so the row renders **disabled** rather than pretending to
                    // work (R62 R11).
                    .child(crate::menu_list::separator(colors))
                    .child(crate::menu_list::action_row(
                        "branch-selector-new",
                        crate::icons::Icon::Plus,
                        "新建并切换分支".into(),
                        false,
                        "暂不支持：分支服务只提供列出与切换",
                        colors,
                        |_, _, _| {},
                    ));
                root.child(
                    div().absolute().top_0().left_0().size_full().child(
                        div().relative().size_full().child(
                            anchored()
                                .anchor(if self.menu_below {
                                    Anchor::TopLeft
                                } else {
                                    Anchor::BottomLeft
                                })
                                .position_mode(AnchoredPositionMode::Local)
                                .position(point(
                                    px(0.0),
                                    px(if self.menu_below {
                                        BRANCH_ROW_HEIGHT + 4.0
                                    } else {
                                        -4.0
                                    }),
                                ))
                                .snap_to_window_with_margin(px(8.0))
                                .child(gpui_kit::deferred(popup).with_priority(2)),
                        ),
                    ),
                )
            })
    }
}

/// One branch row (R62 R10): branch icon + label, and the current branch
/// carries the checkmark and the light-grey rounded surface.
///
/// `index` is the **snapshot** index (what `visible_rows` returns), so the
/// selector is stable across filtering and matches what
/// `focused_index`/`scroll_to_item` speak.
fn render_branch_row(
    index: usize,
    branch: BranchItem,
    focused: Option<BranchId>,
    disabled: bool,
    colors: vega_theme::ThemeColors,
    view: gpui_kit::Entity<BranchSelector>,
) -> impl IntoElement {
    let id = branch.id;
    let current = branch.current;
    let check_selector = format!("branch-row-{index}-check");
    crate::menu_list::row_container(
        ("branch-row", index),
        current,
        !current && !disabled,
        colors,
    )
    // R4: uniform_list items otherwise measure to their intrinsic child width.
    // Give the branch row the popup's content width so its selected surface
    // and trailing marker occupy the same full-width row as the menu body;
    // the parent list's `px_1()` supplies the 4px edge gutters because the
    // virtualized root lays each item against its definite padded width.
    .w_full()
    .mx_0()
    .debug_selector(move || format!("branch-row-{index}"))
    .text_color(if current || disabled {
        colors.text_tertiary
    } else {
        colors.text_primary
    })
    // The keyboard-focus surface stays `bg_hover`; it is applied after the
    // selection surface so a focused non-current row reads as focus.
    .when(focused == Some(id), |row| row.bg(colors.bg_hover))
    .when(!current && !disabled, |row| {
        row.on_mouse_up(MouseButton::Left, move |_, _, cx| {
            view.update(cx, |selector, cx| selector.activate(id, cx));
        })
    })
    .child(crate::icons::icon(
        crate::icons::Icon::GitBranch,
        colors.text_secondary,
    ))
    .child(
        div()
            .min_w_0()
            .flex_1()
            .overflow_hidden()
            .child(branch.label),
    )
    .child(crate::menu_list::selection_marker(
        current,
        move || check_selector.clone(),
        colors,
    ))
}

fn branch_error_label(code: GitWorkspaceErrorCode) -> &'static str {
    match code {
        GitWorkspaceErrorCode::BranchDirty => "Working tree is not clean",
        GitWorkspaceErrorCode::BranchOperationInProgress => "Another Git operation is active",
        GitWorkspaceErrorCode::BranchDetached => "Detached HEAD is unsupported",
        GitWorkspaceErrorCode::BranchUnborn => "Repository has no initial commit",
        GitWorkspaceErrorCode::BranchUnsafeFilter => "Branch contains filtered files",
        GitWorkspaceErrorCode::BranchAlreadyCurrent => "Branch is already current",
        GitWorkspaceErrorCode::GitUnavailable => {
            "Git 2.40+ was not found; install Homebrew Git and retry"
        }
        GitWorkspaceErrorCode::GitUnsupported => "Git 2.40+ is required; upgrade Git and retry",
        GitWorkspaceErrorCode::GitExecutableChanged => {
            "Git changed while Vega was running; restart Vega and retry"
        }
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
        assert_eq!(BRANCH_ROW_HEIGHT, 32.0);
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
