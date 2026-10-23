use super::sticky_disclosure::{StickyDisclosure, StickySection, StickyState};
use super::*;
use crate::scroll_edge_fade::{ScrollEdgeFadeExt, ScrollFade};
use crate::tool_card::{ToolActivityCategory, sentence_initial};
use gpui_kit::{AnyElement, MouseButton, MouseUpEvent, ScrollHandle, div, px};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RunActivityStatus {
    Running,
    Completed,
    Failed,
    Interrupted,
}

#[derive(Clone)]
pub(crate) enum RunActivityChild {
    Thinking(Entity<ThinkingBlock>),
    Tool(Entity<ToolCard>),
    ToolGroup(Entity<ToolActivityGroup>),
}

struct RunActivitySegment {
    title_view: Entity<shimmer::StageText>,
    sticky: StickyState,
    expanded: bool,
    children: Vec<RunActivityChild>,
    tool_categories: Vec<(Entity<ToolCard>, ToolActivityCategory)>,
    compact_tool_groups: Vec<(Entity<ToolCard>, Entity<ToolActivityGroup>)>,
    focus: FocusHandle,
    header_focus: FocusHandle,
    scrolls: Vec<ScrollHandle>,
    latest_progress: Option<StageProgress>,
}

#[derive(Clone)]
enum StageProgress {
    Tool(Entity<ToolCard>),
    Thinking {
        card: Entity<ThinkingBlock>,
        completed_summary: Option<String>,
    },
}

struct StageHeadline {
    title: String,
    icon: Option<crate::icons::Icon>,
    live: bool,
    summary: Option<Entity<ThinkingBlock>>,
}

fn format_stage_tool_categories(categories: &[ToolActivityCategory]) -> String {
    let mut title = String::new();
    for (index, category) in categories.iter().enumerate() {
        let action = category.action_phrase();
        let candidate = if title.is_empty() {
            sentence_initial(action)
        } else {
            format!("{title}, {action}")
        };
        let ellipsis_length = if index + 1 < categories.len() { 3 } else { 0 };
        if candidate.chars().count() + ellipsis_length > 96 {
            if title.is_empty() {
                title = sentence_initial(action).chars().take(95).collect();
                title.push('…');
            } else {
                title.push_str(", …");
            }
            break;
        }
        title = candidate;
    }
    title
}

#[derive(Clone)]
pub(crate) struct RunActivityCheckpoint {
    segment_count: usize,
    active_segment: Option<usize>,
    children: Vec<RunActivityChild>,
    activity_order: u64,
    tool_starts: Vec<(Entity<ToolCard>, u64)>,
    tool_categories_len: usize,
    tool_group_children: Vec<(Entity<ToolActivityGroup>, Vec<Entity<ToolCard>>)>,
    scroll_count: usize,
    segment_progress: Vec<Option<StageProgress>>,
}

impl RunActivityCheckpoint {
    pub(crate) fn active_segment(&self) -> Option<usize> {
        self.active_segment
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RunPhase {
    Working,
    Requesting,
    Thinking,
    Responding,
    Compacting,
}

pub(crate) enum StickyCollapse {
    Run,
    Segment(usize),
}

#[derive(Clone, Copy)]
pub(crate) struct RunActivityDisplayChanged {
    pub(crate) segment: Option<usize>,
    pub(crate) geometry: bool,
}

impl gpui_kit::EventEmitter<RunActivityDisplayChanged> for RunActivityGroup {}

pub(crate) struct RunActivityGroup {
    sticky: StickyState,
    collapsed_sticky: Option<StickyCollapse>,
    started_at: Option<Instant>,
    animation_epoch: Instant,
    clock_task: Option<gpui_kit::Task<()>>,
    phase: RunPhase,
    waiting_for_user: bool,
    current_thinking: Option<Entity<ThinkingBlock>>,
    thinking_order: u64,
    activity_order: u64,
    tool_starts: Vec<(Entity<ToolCard>, u64)>,
    presentation_released: bool,
    continued: bool,
    process_copies: HashSet<u64>,
    final_confirmed: bool,
    body_in_details: bool,
    scroll: ScrollHandle,
    detail_anchor: Option<(usize, f32)>,
    focus: FocusHandle,
    header_focus: FocusHandle,
    collapse_pending: bool,
    status: RunActivityStatus,
    execution_duration_ms: Option<u64>,
    expanded: bool,
    terminal: bool,
    segments: Vec<RunActivitySegment>,
}

impl RunActivityGroup {
    pub(super) fn scroll_fade_pinned_indicator(
        &self,
        segment: Option<usize>,
    ) -> Option<std::rc::Rc<std::cell::Cell<bool>>> {
        match segment {
            None => Some(self.sticky.pinned_indicator()),
            Some(index) => self
                .segments
                .get(index)
                .map(|segment| segment.sticky.pinned_indicator()),
        }
    }

    fn sync_thinking_visibility(&self, cx: &mut App) {
        for (index, segment) in self.segments.iter().enumerate() {
            let visible = if self.process_in_details() {
                self.expanded
            } else {
                self.segment_can_expand(index, cx)
                    && (!self.stage_is_aggregate(index, cx) || segment.expanded)
            };
            for child in &segment.children {
                if let RunActivityChild::Thinking(card) = child {
                    card.update(cx, |card, _| card.set_ancestor_visible(visible));
                }
            }
        }
    }

    pub(crate) fn response_checkpoint(
        &mut self,
        active_segment: Option<usize>,
        cx: &mut Context<Self>,
    ) -> RunActivityCheckpoint {
        self.finish_thinking(cx);
        let children = active_segment
            .and_then(|segment| self.segments.get(segment))
            .map(|segment| segment.children.clone())
            .unwrap_or_default();
        let tool_group_children = children
            .iter()
            .filter_map(|child| match child {
                RunActivityChild::ToolGroup(group) => {
                    Some((group.clone(), group.read(cx).children()))
                }
                _ => None,
            })
            .collect();
        let scroll_count = active_segment
            .and_then(|segment| self.segments.get(segment))
            .map(|segment| segment.scrolls.len())
            .unwrap_or(0);
        let tool_categories_len = active_segment
            .and_then(|segment| self.segments.get(segment))
            .map(|segment| segment.tool_categories.len())
            .unwrap_or(0);
        RunActivityCheckpoint {
            segment_count: self.segments.len(),
            active_segment,
            children,
            activity_order: self.activity_order,
            tool_starts: self.tool_starts.clone(),
            tool_categories_len,
            tool_group_children,
            scroll_count,
            segment_progress: self
                .segments
                .iter()
                .map(|segment| segment.latest_progress.clone())
                .collect(),
        }
    }

    pub(crate) fn rollback_response(
        &mut self,
        checkpoint: &RunActivityCheckpoint,
        cx: &mut Context<Self>,
    ) {
        self.finish_thinking(cx);
        for (card, _) in &self.tool_starts {
            if !checkpoint
                .tool_starts
                .iter()
                .any(|(owned, _)| owned == card)
            {
                card.update(cx, |card, _| card.release_presentation());
            }
        }
        self.activity_order = checkpoint.activity_order;
        self.tool_starts = checkpoint.tool_starts.clone();
        self.segments.truncate(checkpoint.segment_count);
        if !self.terminal && !self.presentation_released {
            for (segment, progress) in self.segments.iter_mut().zip(&checkpoint.segment_progress) {
                segment.latest_progress = progress.clone();
            }
        }
        if let Some(segment) = checkpoint.active_segment
            && let Some(segment_state) = self.segments.get_mut(segment)
        {
            for (group, cards) in &checkpoint.tool_group_children {
                group.update(cx, |group, cx| group.replace_children(cards.clone(), cx));
            }
            segment_state.children = checkpoint.children.clone();
            segment_state
                .tool_categories
                .truncate(checkpoint.tool_categories_len);
            segment_state.scrolls.truncate(checkpoint.scroll_count);
            self.refresh_compact_tool_groups(segment, cx);
            if self.stage_process_entry_count(segment, cx) == 1
                && let Some(card) = self.stage_thinking(segment)
            {
                let expanded = self.segments[segment].expanded;
                card.update(cx, |card, cx| card.set_stage_expanded(expanded, cx));
            }
        }
        self.phase = RunPhase::Working;
        self.sync_thinking_visibility(cx);
        cx.notify();
    }

    pub(crate) fn finish_thinking(&mut self, cx: &mut Context<Self>) {
        if let Some(card) = self.current_thinking.take() {
            for segment in &mut self.segments {
                if let Some(StageProgress::Thinking {
                    card: source,
                    completed_summary,
                }) = &mut segment.latest_progress
                    && source == &card
                {
                    *completed_summary = card.read(cx).summary_title().map(str::to_owned);
                }
            }
            card.update(cx, |card, cx| {
                card.shimmer_epoch = None;
                card.progress_finished = true;
                cx.notify();
            });
            self.sync_thinking_visibility(cx);
            cx.notify();
        }
    }
    pub(crate) fn start_clock(&mut self, started_at: Instant, cx: &mut Context<Self>) {
        if self.started_at.is_none() && !self.terminal && !self.presentation_released {
            self.started_at = Some(started_at);
            self.animation_epoch = started_at;
            self.sync_thinking_visibility(cx);
            cx.notify();
        }
    }

    fn schedule_clock(&mut self, cx: &mut Context<Self>) {
        if self.terminal
            || self.presentation_released
            || self.started_at.is_none()
            || self.clock_task.is_some()
        {
            return;
        }
        self.clock_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            let _ = this.update(cx, |group, cx| {
                group.clock_task = None;
                if !group.terminal && !group.presentation_released {
                    cx.emit(RunActivityDisplayChanged {
                        segment: None,
                        geometry: false,
                    });
                }
            });
        }));
    }

    pub(crate) fn set_phase(&mut self, phase: RunPhase, cx: &mut Context<Self>) {
        if !self.terminal && self.phase != phase {
            self.phase = phase;
            self.sync_thinking_visibility(cx);
            cx.notify();
        }
    }

    pub(crate) fn set_compacting(&mut self, compacting: bool, cx: &mut Context<Self>) {
        if compacting {
            self.set_phase(RunPhase::Compacting, cx);
        } else if self.phase == RunPhase::Compacting {
            self.set_phase(RunPhase::Working, cx);
        }
    }

    pub(crate) fn set_waiting(&mut self, waiting: bool, cx: &mut Context<Self>) {
        if self.terminal || self.waiting_for_user == waiting {
            return;
        }
        self.waiting_for_user = waiting;
        if let Some(card) = &self.current_thinking {
            card.update(cx, |card, cx| {
                card.shimmer_epoch = None;
                cx.notify();
            });
        }
        self.sync_thinking_visibility(cx);
        cx.notify();
    }

    pub(crate) fn set_thinking(
        &mut self,
        card: Entity<ThinkingBlock>,
        preview_changed: bool,
        cx: &mut Context<Self>,
    ) {
        if self.terminal
            || self.presentation_released
            || !preview_changed && self.current_thinking.as_ref() == Some(&card)
        {
            return;
        }
        if self.phase != RunPhase::Compacting {
            self.phase = RunPhase::Thinking;
        }
        self.activity_order = self.activity_order.saturating_add(1);
        self.thinking_order = self.activity_order;
        if let Some(segment) = self.segments.iter_mut().find(|segment| {
            segment
                .children
                .iter()
                .any(|child| matches!(child, RunActivityChild::Thinking(owned) if owned == &card))
        }) {
            segment.latest_progress = Some(StageProgress::Thinking {
                card: card.clone(),
                completed_summary: None,
            });
        }
        card.update(cx, |card, _| card.shimmer_epoch = None);
        self.current_thinking = Some(card);
        self.sync_thinking_visibility(cx);
        cx.notify();
    }

    pub(crate) fn tool_started(&mut self, card: &Entity<ToolCard>, cx: &mut Context<Self>) {
        if self.terminal
            || self.presentation_released
            || !card.read(cx).is_running()
            || self.tool_starts.iter().any(|(owned, _)| owned == card)
        {
            return;
        }
        let Some(segment) = self.segments.iter_mut().find(|segment| {
            segment.children.iter().any(|child| match child {
                RunActivityChild::Tool(owned) => owned == card,
                RunActivityChild::ToolGroup(group) => group.read(cx).contains(card),
                _ => false,
            })
        }) else {
            return;
        };
        self.activity_order = self.activity_order.saturating_add(1);
        self.tool_starts.push((card.clone(), self.activity_order));
        segment.latest_progress = Some(StageProgress::Tool(card.clone()));
        self.sync_thinking_visibility(cx);
        cx.notify();
    }

    pub(crate) fn release_presentation(&mut self, cx: &mut Context<Self>) {
        if self.presentation_released {
            return;
        }
        self.presentation_released = true;
        for segment in 0..self.segments.len() {
            for card in self.stage_tool_calls(segment, cx) {
                card.update(cx, |card, _| card.release_presentation());
            }
        }
        self.tool_starts.clear();
        self.finish_thinking(cx);
        for segment in &mut self.segments {
            segment.latest_progress = None;
        }
        self.clock_task = None;
        self.sync_thinking_visibility(cx);
        cx.notify();
    }

    pub(crate) fn continue_run(&mut self, cx: &mut Context<Self>) {
        if self.continued {
            return;
        }
        self.continued = true;
        self.release_presentation(cx);
        cx.notify();
    }

    pub(crate) fn header_hidden(&self) -> bool {
        let has_details = !self.process_copies.is_empty()
            || self
                .segments
                .iter()
                .any(|segment| !segment.children.is_empty());
        self.continued && !(self.process_in_details() && has_details)
    }

    pub(crate) fn confirm_final(
        &mut self,
        copies: HashSet<u64>,
        pending: bool,
        cx: &mut Context<Self>,
    ) {
        if self.final_confirmed {
            return;
        }
        self.final_confirmed = true;
        self.process_copies = copies;
        self.collapse_pending = pending;
        if !pending {
            self.expanded = false;
            self.body_in_details = true;
        }
        self.sync_thinking_visibility(cx);
        cx.notify();
    }

    pub(crate) fn collapse_pending(&self) -> bool {
        self.collapse_pending
    }
    pub(crate) fn owns_process_copy(&self, id: u64) -> bool {
        self.process_copies.contains(&id)
    }
    pub(crate) fn hides_copy(&self, id: u64) -> bool {
        self.final_confirmed && self.body_in_details && self.process_copies.contains(&id)
    }

    pub(crate) fn settle_final(
        &mut self,
        focus_process_selection: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.collapse_pending {
            return;
        }
        let mut focused = focus_process_selection || self.focus.contains_focused(window, cx);
        let mut copies = Vec::new();
        for segment in &self.segments {
            focused |= segment.focus.contains_focused(window, cx)
                || segment.header_focus.is_focused(window);
            for child in &segment.children {
                if let RunActivityChild::Thinking(card) = child {
                    focused |= card.read(cx).contains_focus(window, cx);
                    copies.push(card.read(cx).source_copy.clone());
                }
            }
        }
        for copy in copies {
            copy.clear_hidden_selection(window, cx);
        }
        self.collapse_pending = false;
        self.expanded = false;
        self.body_in_details = true;
        self.detail_anchor = None;
        if focused {
            self.header_focus.focus(window, cx);
        }
        self.sync_thinking_visibility(cx);
        cx.notify();
    }

    pub(crate) fn process_in_details(&self) -> bool {
        self.final_confirmed && self.body_in_details
    }

    pub(crate) fn invalidate_final(&mut self, cx: &mut Context<Self>) {
        self.final_confirmed = false;
        self.collapse_pending = false;
        self.body_in_details = false;
        self.sync_thinking_visibility(cx);
        cx.notify();
    }

    pub(crate) fn reveal(&mut self, cx: &mut Context<Self>) {
        self.expanded = true;
        self.sync_thinking_visibility(cx);
        cx.notify();
    }

    pub(crate) fn reveal_detail(&mut self, index: usize, offset: f32, cx: &mut Context<Self>) {
        self.reveal(cx);
        self.detail_anchor = Some((index, offset.max(0.)));
        self.scroll.scroll_to_top_of_item(index);
    }

    fn settle_detail_anchor(&mut self, cx: &mut Context<Self>) {
        let Some((index, offset)) = self.detail_anchor else {
            return;
        };
        let Some(bounds) = self.scroll.bounds_for_item(index) else {
            return;
        };
        self.detail_anchor = None;
        let y = self.scroll.bounds().top() - bounds.top() - px(offset);
        self.scroll.set_offset(gpui_kit::point(
            px(0.),
            y.max(-self.scroll.max_offset().y).min(px(0.)),
        ));
        self.sync_thinking_visibility(cx);
        cx.notify();
    }

    fn visible_segment_children(&self, segment: usize, cx: &App) -> Vec<RunActivityChild> {
        let has_tools = self.stage_has_tools(segment);
        let outer_summary = self.stage_outer_summary_card(segment, cx);
        self.segment_children(segment)
            .into_iter()
            .filter(|child| {
                !self.process_in_details()
                    && !matches!(child, RunActivityChild::Thinking(card) if outer_summary.as_ref() == Some(card))
                    && (!has_tools
                        || !matches!(child, RunActivityChild::Thinking(card) if !card.read(cx).has_meaningful_source_text()))
            })
            .collect()
    }

    fn segment_can_expand(&self, segment: usize, cx: &App) -> bool {
        if self.stage_is_aggregate(segment, cx) {
            return true;
        }
        if !self.stage_tool_calls(segment, cx).is_empty() {
            return false;
        }
        self.stage_thinking(segment).is_some_and(|card| {
            self.stage_summary_title(segment, cx).is_none()
                && card.read(cx).has_meaningful_source_text()
        })
    }

    pub(crate) fn status(&self) -> RunActivityStatus {
        self.status
    }

    pub(crate) fn expanded(&self) -> bool {
        self.expanded
    }

    pub(crate) fn new(
        status: RunActivityStatus,
        execution_duration_ms: Option<u64>,
        expanded: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            sticky: StickyState::default(),
            collapsed_sticky: None,
            started_at: None,
            animation_epoch: Instant::now(),
            clock_task: None,
            phase: RunPhase::Working,
            waiting_for_user: false,
            current_thinking: None,
            thinking_order: 0,
            activity_order: 0,
            tool_starts: Vec::new(),
            presentation_released: false,
            continued: false,
            process_copies: HashSet::new(),
            final_confirmed: false,
            body_in_details: false,
            scroll: ScrollHandle::new(),
            detail_anchor: None,
            focus: cx.focus_handle(),
            header_focus: cx.focus_handle(),
            collapse_pending: false,
            status,
            execution_duration_ms,
            expanded,
            terminal: status != RunActivityStatus::Running,
            segments: Vec::new(),
        }
    }

    pub(crate) fn children(&self) -> Vec<RunActivityChild> {
        self.segments
            .iter()
            .flat_map(|segment| segment.children.iter().cloned())
            .collect()
    }

    pub(crate) fn has_children(&self) -> bool {
        self.segments
            .iter()
            .any(|segment| !segment.children.is_empty())
    }

    pub(crate) fn segment_children(&self, segment: usize) -> Vec<RunActivityChild> {
        self.segments
            .get(segment)
            .map(|segment| segment.children.clone())
            .unwrap_or_default()
    }

    fn stage_tool_calls(&self, segment: usize, cx: &App) -> Vec<Entity<ToolCard>> {
        self.segment_children(segment)
            .into_iter()
            .flat_map(|child| match child {
                RunActivityChild::Tool(card) => vec![card],
                RunActivityChild::ToolGroup(group) => group.read(cx).children(),
                RunActivityChild::Thinking(_) => Vec::new(),
            })
            .collect()
    }

    fn stage_has_tools(&self, segment: usize) -> bool {
        self.segments.get(segment).is_some_and(|state| {
            state.children.iter().any(|child| {
                matches!(
                    child,
                    RunActivityChild::Tool(_) | RunActivityChild::ToolGroup(_)
                )
            })
        })
    }

    fn stage_process_entry_count(&self, segment: usize, cx: &App) -> usize {
        let tools = self.stage_tool_calls(segment, cx).len();
        let thoughts = self
            .segments
            .get(segment)
            .map(|state| {
                state
                    .children
                    .iter()
                    .filter(|child| {
                        matches!(child, RunActivityChild::Thinking(card) if card.read(cx).has_meaningful_source_text())
                    })
                    .count()
            })
            .unwrap_or(0);
        if tools == 0 && thoughts == 0 && self.stage_thinking(segment).is_some() {
            1
        } else {
            tools + thoughts
        }
    }

    fn stage_is_aggregate(&self, segment: usize, cx: &App) -> bool {
        !self.process_in_details() && self.stage_process_entry_count(segment, cx) >= 2
    }

    fn stage_tool_categories(&self, segment: usize, cx: &App) -> Vec<ToolActivityCategory> {
        let mut categories = Vec::new();
        for tool in self.stage_tool_calls(segment, cx) {
            let category = self.segments[segment]
                .tool_categories
                .iter()
                .find(|(tracked, _)| tracked == &tool)
                .map(|(_, category)| *category)
                .unwrap_or_else(|| tool.read(cx).stage_activity_category());
            if !categories.contains(&category) {
                categories.push(category);
            }
        }
        categories
    }

    fn stage_tool_title(&self, segment: usize, cx: &App) -> Option<(String, crate::icons::Icon)> {
        let categories = self.stage_tool_categories(segment, cx);
        let icon = categories.first()?.icon();
        Some((format_stage_tool_categories(&categories), icon))
    }

    pub(crate) fn start_segment(&mut self, cx: &mut Context<Self>) -> usize {
        self.segments.push(RunActivitySegment {
            title_view: cx.new(|_| shimmer::StageText::new()),
            sticky: StickyState::default(),
            expanded: false,
            children: Vec::new(),
            tool_categories: Vec::new(),
            compact_tool_groups: Vec::new(),
            focus: cx.focus_handle(),
            header_focus: cx.focus_handle().tab_stop(true),
            scrolls: vec![ScrollHandle::new()],
            latest_progress: None,
        });
        self.sync_thinking_visibility(cx);
        cx.notify();
        self.segments.len() - 1
    }

    pub(crate) fn append_thinking(
        &mut self,
        segment: usize,
        card: Entity<ThinkingBlock>,
        cx: &mut Context<Self>,
    ) {
        let preserve_open = self.stage_process_entry_count(segment, cx) == 1
            && self
                .stage_tool_calls(segment, cx)
                .first()
                .is_some_and(|card| card.read(cx).is_expanded());
        cx.subscribe(
            &card,
            move |group, card, change: &thinking::ThinkingChanged, cx| {
                group.sync_thinking_visibility(cx);
                let details = group.process_in_details();
                let visible = if details {
                    group.expanded
                } else {
                    group
                        .segments
                        .get(segment)
                        .is_some_and(|state| state.expanded)
                };
                if change.stage || visible && (change.header || change.geometry) {
                    cx.emit(RunActivityDisplayChanged {
                        segment: (!details).then_some(segment),
                        geometry: change.geometry
                            && (visible
                                || change.stage
                                    && !details
                                    && !group.stage_is_aggregate(segment, cx)),
                    });
                }
                let _ = card;
            },
        )
        .detach();
        if let Some(segment) = self.segments.get_mut(segment) {
            segment.children.push(RunActivityChild::Thinking(card));
            if preserve_open {
                segment.expanded = true;
            }
        }
        self.sync_thinking_visibility(cx);
        cx.notify();
    }

    pub(crate) fn append_tool(
        &mut self,
        segment: usize,
        card: Entity<ToolCard>,
        proposed: bool,
        cx: &mut Context<Self>,
    ) {
        let preserve_open = self.stage_process_entry_count(segment, cx) == 1
            && self
                .stage_tool_calls(segment, cx)
                .first()
                .is_some_and(|card| card.read(cx).is_expanded());
        if proposed && self.phase != RunPhase::Compacting {
            self.phase = RunPhase::Working;
        }
        let Some(segment_state) = self.segments.get_mut(segment) else {
            return;
        };
        segment_state
            .tool_categories
            .push((card.clone(), card.read(cx).stage_activity_category()));
        match segment_state.children.last() {
            Some(RunActivityChild::Tool(first)) => {
                cx.observe(&card, |_, _, cx| cx.notify()).detach();
                let expanded = first.read(cx).is_expanded();
                let group = cx.new(|_| ToolActivityGroup::new(first.clone(), card));
                Self::observe_tool_group(&group, cx);
                group.update(cx, |group, cx| group.set_expanded(expanded, cx));
                if let Some(last) = segment_state.children.last_mut() {
                    *last = RunActivityChild::ToolGroup(group);
                }
            }
            Some(RunActivityChild::ToolGroup(group)) => {
                cx.observe(&card, |_, _, cx| cx.notify()).detach();
                group.update(cx, |group, cx| group.append(card, cx));
            }
            _ => {
                cx.observe(&card, |_, _, cx| cx.notify()).detach();
                card.update(cx, |card, cx| card.set_expanded(false, cx));
                segment_state.children.push(RunActivityChild::Tool(card));
            }
        }
        if preserve_open {
            segment_state.expanded = true;
        }
        self.refresh_compact_tool_groups(segment, cx);
        self.sync_thinking_visibility(cx);
        cx.notify();
    }

    pub(crate) fn append_hydrated_tool(
        &mut self,
        segment: usize,
        card: Entity<ToolCard>,
        cx: &mut Context<Self>,
    ) {
        let Some(segment_state) = self.segments.get_mut(segment) else {
            return;
        };
        match segment_state.children.last() {
            Some(RunActivityChild::Tool(first)) => {
                cx.observe(&card, |_, _, cx| cx.notify()).detach();
                let group = cx.new(|_| ToolActivityGroup::new(first.clone(), card));
                Self::observe_tool_group(&group, cx);
                if let Some(last) = segment_state.children.last_mut() {
                    *last = RunActivityChild::ToolGroup(group);
                }
            }
            Some(RunActivityChild::ToolGroup(group)) => {
                cx.observe(&card, |_, _, cx| cx.notify()).detach();
                group.update(cx, |group, cx| group.append(card, cx));
            }
            _ => {
                cx.observe(&card, |_, _, cx| cx.notify()).detach();
                segment_state.children.push(RunActivityChild::Tool(card));
            }
        }
        self.refresh_compact_tool_groups(segment, cx);
        self.sync_thinking_visibility(cx);
        cx.notify();
    }

    fn refresh_compact_tool_groups(&mut self, segment: usize, cx: &mut Context<Self>) {
        let Some(segment_state) = self.segments.get_mut(segment) else {
            return;
        };
        let mut runs = Vec::new();
        let mut current = Vec::new();
        for child in &segment_state.children {
            match child {
                RunActivityChild::Tool(card) => current.push(card.clone()),
                RunActivityChild::ToolGroup(group) => {
                    current.extend(group.read(cx).children());
                }
                RunActivityChild::Thinking(_) => {}
            }
        }
        runs.push(current);
        let old = std::mem::take(&mut segment_state.compact_tool_groups);
        for cards in runs.into_iter().filter(|cards| cards.len() >= 2) {
            let first = cards[0].clone();
            let group = if let Some((_, group)) = old.iter().find(|(card, _)| card == &first) {
                group.update(cx, |group, cx| group.replace_children(cards, cx));
                group.clone()
            } else {
                let group = cx.new(|_| ToolActivityGroup::from_children(cards, false));
                Self::observe_tool_group(&group, cx);
                group
            };
            segment_state.compact_tool_groups.push((first, group));
        }
    }

    fn observe_tool_group(group: &Entity<ToolActivityGroup>, cx: &mut Context<Self>) {
        cx.observe(group, |_, _, cx| cx.notify()).detach();
    }

    pub(crate) fn contains_tool(&self, card: &Entity<ToolCard>, cx: &App) -> bool {
        self.segments
            .iter()
            .flat_map(|segment| segment.children.iter())
            .any(|child| match child {
                RunActivityChild::Tool(owned) => owned == card,
                RunActivityChild::ToolGroup(group) => group.read(cx).contains(card),
                RunActivityChild::Thinking(_) => false,
            })
    }

    pub(crate) fn contains_tool_group(&self, group: &Entity<ToolActivityGroup>) -> bool {
        self.segments
            .iter()
            .flat_map(|segment| segment.children.iter())
            .any(|child| matches!(child, RunActivityChild::ToolGroup(owned) if owned == group))
    }

    pub(crate) fn segment_row_count(&self, segment: usize, cx: &App) -> usize {
        let aggregate = self.stage_is_aggregate(segment, cx);
        let single_thinking = !aggregate
            && self.stage_tool_calls(segment, cx).is_empty()
            && self.stage_thinking(segment).is_some();
        let children = self.visible_segment_children(segment, cx);
        let expanded = (aggregate || single_thinking)
            && self.segments[segment].expanded
            && self.segment_can_expand(segment, cx);
        if single_thinking {
            let card = self.stage_thinking(segment);
            return 1
                + usize::from(expanded)
                + usize::from(card.is_some_and(|card| card.read(cx).truncated));
        }
        let children = if expanded {
            Self::compact_children(children, cx)
        } else {
            children
        };
        usize::from(aggregate)
            + children
                .iter()
                .filter(|_| !aggregate || expanded)
                .map(|child| Self::child_row_count(child, cx))
                .sum::<usize>()
    }

    pub(crate) fn compact_children(
        children: Vec<RunActivityChild>,
        cx: &App,
    ) -> Vec<RunActivityChild> {
        children
            .into_iter()
            .flat_map(|child| match child {
                RunActivityChild::ToolGroup(group) => group
                    .read(cx)
                    .children()
                    .into_iter()
                    .map(RunActivityChild::Tool)
                    .collect(),
                child => vec![child],
            })
            .collect()
    }

    fn compact_sections(children: Vec<RunActivityChild>, cx: &App) -> Vec<Vec<RunActivityChild>> {
        Self::split_sections(Self::compact_children(children, cx))
    }

    fn split_sections(children: Vec<RunActivityChild>) -> Vec<Vec<RunActivityChild>> {
        if children.is_empty() {
            Vec::new()
        } else {
            vec![children]
        }
    }

    pub(crate) fn estimated_segment_height(&self, segment: usize, width: f32, cx: &App) -> f32 {
        self.estimated_segment_height_with_top(segment, width, Layout::CONVERSATION_STAGE_TOP, cx)
    }

    pub(crate) fn estimated_segment_height_with_top(
        &self,
        segment: usize,
        width: f32,
        top: f32,
        cx: &App,
    ) -> f32 {
        if self.stage_is_aggregate(segment, cx) {
            let children = self.visible_segment_children(segment, cx);
            let expanded = self.segments[segment].expanded && self.segment_can_expand(segment, cx);
            let sections = if expanded {
                Self::compact_sections(children, cx)
            } else {
                Self::split_sections(children)
            };
            let details_height = sections
                .iter()
                .map(|section| {
                    if expanded {
                        section
                            .iter()
                            .map(|child| Self::estimated_child_height(child, width, cx))
                            .sum::<f32>()
                            .min(Layout::TOOL_GROUP_MAX_HEIGHT)
                    } else {
                        0.
                    }
                })
                .sum::<f32>();
            return top + Layout::CONVERSATION_TOOL_HEIGHT + details_height;
        }
        if let Some(card) = self
            .stage_tool_calls(segment, cx)
            .is_empty()
            .then(|| self.stage_thinking(segment))
            .flatten()
        {
            let card = card.read(cx);
            let expanded = self.segments[segment].expanded && self.segment_can_expand(segment, cx);
            let source_height = if expanded && card.has_source_text() {
                Layout::PROCESS_ITEM_GAP + card.estimated_source_height(width)
            } else {
                0.
            };
            let truncated_height = if card.truncated {
                Layout::PROCESS_ITEM_GAP + Typography::METADATA * Typography::BODY_LINE_HEIGHT
            } else {
                0.
            };
            return top + Layout::CONVERSATION_TOOL_HEIGHT + source_height + truncated_height;
        }
        let children = self.visible_segment_children(segment, cx);
        let tool_leads = matches!(
            children.first(),
            Some(RunActivityChild::Tool(_) | RunActivityChild::ToolGroup(_))
        );
        children
            .iter()
            .map(|child| Self::estimated_child_height(child, width, cx))
            .sum::<f32>()
            + if tool_leads { top } else { 0. }
    }

    pub(crate) fn estimated_child_height(child: &RunActivityChild, width: f32, cx: &App) -> f32 {
        match child {
            RunActivityChild::Thinking(card) => card.read(cx).estimated_height(width),
            RunActivityChild::Tool(card) => card.read(cx).estimated_height(),
            RunActivityChild::ToolGroup(group) => group.read(cx).estimated_height(cx),
        }
    }

    pub(crate) fn estimated_header_height(&self) -> f32 {
        if self.header_hidden() {
            return 0.;
        }
        Layout::CONVERSATION_CLOCK_HEIGHT
            + Layout::CONVERSATION_DIVIDER_GAP
            + Layout::CONVERSATION_DIVIDER_HEIGHT
    }

    fn child_row_count(child: &RunActivityChild, cx: &App) -> usize {
        match child {
            RunActivityChild::Thinking(card) => card.read(cx).row_count(),
            RunActivityChild::Tool(card) => card.read(cx).row_count(),
            RunActivityChild::ToolGroup(group) => group.read(cx).row_count(cx),
        }
    }

    pub(crate) fn complete(
        &mut self,
        status: RunActivityStatus,
        execution_duration_ms: Option<u64>,
        cx: &mut Context<Self>,
    ) {
        if self.continued {
            return;
        }
        if self.terminal {
            if status == RunActivityStatus::Failed && self.status == RunActivityStatus::Interrupted
            {
                self.status = status;
                self.invalidate_final(cx);
                cx.notify();
            }
            return;
        }
        if status != RunActivityStatus::Completed {
            self.invalidate_final(cx);
        }
        self.status = status;
        self.execution_duration_ms = execution_duration_ms;
        self.terminal = true;
        self.release_presentation(cx);
        self.sync_thinking_visibility(cx);
        cx.notify();
    }

    pub(crate) fn toggle_expanded(&mut self, cx: &mut Context<Self>) {
        if self.expanded && self.sticky.pinned() {
            self.collapsed_sticky = Some(StickyCollapse::Run);
        }
        self.expanded = !self.expanded;
        if self.final_confirmed {
            self.body_in_details = true;
        }
        self.sync_thinking_visibility(cx);
        cx.notify();
    }

    pub(crate) fn take_sticky_collapse(&mut self) -> Option<StickyCollapse> {
        self.collapsed_sticky.take()
    }

    fn label(&self) -> String {
        if self.continued {
            return "Activity".into();
        }
        let duration = self.execution_duration_ms.or_else(|| {
            self.started_at
                .filter(|_| !self.terminal && !self.presentation_released)
                .map(|start| start.elapsed().as_millis().min(u128::from(u64::MAX)) as u64)
        });
        let status = match self.status {
            RunActivityStatus::Running if self.waiting_for_user => "Awaiting confirmation for",
            RunActivityStatus::Running => "Working for",
            RunActivityStatus::Completed => "Worked for",
            RunActivityStatus::Failed => "Failed after",
            RunActivityStatus::Interrupted => "Stopped after",
        };
        duration
            .map(|value| format!("{status} {}", format_run_clock(value)))
            .unwrap_or_else(|| match self.status {
                RunActivityStatus::Running if self.waiting_for_user => {
                    "Awaiting confirmation".into()
                }
                RunActivityStatus::Running => "Working".into(),
                RunActivityStatus::Completed => "Completed".into(),
                RunActivityStatus::Failed => "Failed".into(),
                RunActivityStatus::Interrupted => "Stopped".into(),
            })
    }

    pub(crate) fn render(group: Entity<Self>, window: &mut Window, cx: &mut App) -> AnyElement {
        let details = if group.read(cx).expanded && group.read(cx).process_in_details() {
            group
                .read(cx)
                .children()
                .into_iter()
                .map(|child| Self::render_child(child, window, cx))
                .collect()
        } else {
            Vec::new()
        };
        Self::render_with_details(group, details, window, cx)
    }

    pub(crate) fn render_with_details(
        group: Entity<Self>,
        details: Vec<AnyElement>,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let group_ref = group.read(cx);
        if group_ref.header_hidden() {
            return div().h_0().into_any_element();
        }
        let expanded = group_ref.expanded;
        let sticky = group_ref.sticky.clone();
        let label = group_ref.label();
        let status_color = colors.conversation_clock;
        let can_expand = group_ref.process_in_details()
            && (group_ref.has_children() || !group_ref.process_copies.is_empty());
        let scroll = group_ref.scroll.clone();
        let focus = group_ref.focus.clone();
        let clock = group.downgrade();
        let detail_anchor = group.downgrade();
        let toggle = group.clone();
        let keyboard_toggle = group.clone();
        let disclosure_group: gpui_kit::SharedString =
            format!("run-disclosure-{}", group.entity_id()).into();
        let header_focus = group_ref.header_focus.clone().tab_stop(can_expand);
        let header = div()
            .id(("run-activity-toggle", group.entity_id()))
            .group(disclosure_group.clone())
            .map(|row| {
                #[cfg(test)]
                let row = {
                    use gpui_kit::base::TestSupportExt as _;
                    row.test_support()
                };
                row
            })
            .track_focus(&header_focus)
            .tab_stop(can_expand)
            .aria_expanded(expanded)
            .aria_label(format!(
                "{label}, {} run activity",
                if expanded { "collapse" } else { "expand" }
            ))
            .focus_visible(move |row| row.bg(colors.bg_hover))
            .relative()
            .w_full()
            .h(px(Layout::CONVERSATION_CLOCK_HEIGHT))
            .text_size(px(Typography::CONVERSATION))
            .line_height(gpui_kit::relative(Typography::CONVERSATION_LINE_HEIGHT))
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(Layout::CONVERSATION_INLINE_GAP))
            .on_key_down(move |event, window, cx| {
                if can_expand && event.keystroke.key == "tab" {
                    if event.keystroke.modifiers.shift {
                        window.focus_prev(cx);
                    } else {
                        window.focus_next(cx);
                    }
                    cx.stop_propagation();
                } else if can_expand && matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    keyboard_toggle.update(cx, RunActivityGroup::toggle_expanded);
                    cx.stop_propagation();
                }
            })
            .when(can_expand, |row| {
                row.cursor_pointer().on_mouse_up(
                    MouseButton::Left,
                    move |_: &MouseUpEvent, _, cx| {
                        toggle.update(cx, RunActivityGroup::toggle_expanded);
                    },
                )
            })
            .child(
                div()
                    .min_w_0()
                    .flex_shrink(1.)
                    .truncate()
                    .text_color(status_color)
                    .child(label),
            )
            .child(crate::disclosure::inline_chevron(
                disclosure_group,
                &header_focus,
                expanded,
                can_expand,
                colors,
                window,
            ))
            .child(
                gpui_kit::canvas(
                    |_, _, _| (),
                    move |bounds, _, window, cx| {
                        if !bounds.intersect(&window.content_mask().bounds).is_empty()
                            && window.is_window_active()
                        {
                            let _ = clock.update(cx, |group, cx| group.schedule_clock(cx));
                        }
                    },
                )
                .absolute()
                .inset_0(),
            );
        let mut sections = vec![StickySection::plain(
            div()
                .mt(px(Layout::CONVERSATION_DIVIDER_GAP))
                .h(px(Layout::CONVERSATION_DIVIDER_HEIGHT))
                .w_full()
                .bg(colors.conversation_divider)
                .into_any_element(),
        )];
        if expanded && !details.is_empty() {
            sections.push(StickySection::content(
                div()
                    .relative()
                    .w_full()
                    .min_w_0()
                    .track_focus(&focus)
                    .child(
                        div()
                            .id(("run-activity-content", group.entity_id()))
                            .w_full()
                            .min_w_0()
                            .max_h(px(Layout::TOOL_GROUP_MAX_HEIGHT))
                            .overflow_x_hidden()
                            .overflow_y_scroll()
                            .track_scroll(&scroll)
                            .on_scroll_wheel(crate::tool_card::contain_disclosure_scroll(&scroll))
                            .flex()
                            .flex_col()
                            .gap_0()
                            .children(details)
                            .tracked_scroll_edge_fade(
                                scroll.clone(),
                                colors.bg_base,
                                ScrollFade::compact()
                                    .vertical_gutter(px(Layout::DISCLOSURE_SCROLLBAR_GUTTER)),
                            ),
                    )
                    .child(crate::tool_card::disclosure_scrollbar(&scroll, cx))
                    .child(
                        gpui_kit::canvas(
                            |_, _, _| (),
                            move |_, _, _, cx| {
                                let _ = detail_anchor
                                    .update(cx, |group, cx| group.settle_detail_anchor(cx));
                            },
                        )
                        .absolute()
                        .inset_0(),
                    )
                    .into_any_element(),
                true,
            ));
        }
        render_rows::process_surface(&colors)
            .gap_0()
            .child(StickyDisclosure::new(
                div()
                    .bg(colors.bg_base)
                    .block_mouse_except_scroll()
                    .child(header)
                    .into_any_element(),
                sections,
                sticky,
                expanded && can_expand,
            ))
            .into_any_element()
    }

    pub(crate) fn render_child(
        child: RunActivityChild,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        match child {
            RunActivityChild::Thinking(card) => {
                div().w_full().min_w_0().child(card).into_any_element()
            }
            RunActivityChild::Tool(card) => ToolCard::render(card, false, window, cx),
            RunActivityChild::ToolGroup(group) => ToolActivityGroup::render(group, window, cx),
        }
    }

    fn stage_thinking(&self, segment: usize) -> Option<Entity<ThinkingBlock>> {
        if self.process_in_details() {
            return None;
        }
        self.segments
            .get(segment)?
            .children
            .iter()
            .rev()
            .find_map(|child| match child {
                RunActivityChild::Thinking(card) => Some(card.clone()),
                _ => None,
            })
    }

    fn stage_summary_card(&self, segment: usize, cx: &App) -> Option<Entity<ThinkingBlock>> {
        self.segments
            .get(segment)?
            .children
            .iter()
            .rev()
            .find_map(|child| {
                if let RunActivityChild::Thinking(card) = child {
                    card.read(cx).summary_title().map(|_| card.clone())
                } else {
                    None
                }
            })
    }

    fn stage_summary_title(&self, segment: usize, cx: &App) -> Option<String> {
        self.stage_summary_card(segment, cx)
            .and_then(|card| card.read(cx).summary_title().map(str::to_owned))
    }

    fn stage_outer_summary_card(&self, segment: usize, cx: &App) -> Option<Entity<ThinkingBlock>> {
        self.stage_headline(segment, cx).summary
    }

    fn stage_headline(&self, segment: usize, cx: &App) -> StageHeadline {
        let tools = self.stage_tool_calls(segment, cx);
        let owns_progress =
            !self.terminal && !self.presentation_released && !self.process_in_details();
        let progress = self
            .segments
            .get(segment)
            .and_then(|state| state.latest_progress.as_ref());
        let live = owns_progress && !self.waiting_for_user && self.phase != RunPhase::Compacting;
        if owns_progress {
            let tool = self
                .tool_starts
                .iter()
                .filter(|(card, _)| tools.contains(card) && card.read(cx).is_running())
                .max_by_key(|(_, order)| *order);
            let thinking = self.current_thinking.as_ref().filter(|current| {
                self.segments.get(segment).is_some_and(|state| {
                    state.children.iter().any(|child| {
                        matches!(child, RunActivityChild::Thinking(card) if card == *current)
                    })
                })
            });
            if let Some(card) = thinking
                && tool.is_none_or(|(_, order)| self.thinking_order > *order)
            {
                let summary = card.read(cx).summary_title().map(str::to_owned);
                return StageHeadline {
                    title: summary.clone().unwrap_or_else(|| "Thinking".into()),
                    icon: None,
                    live,
                    summary: summary.map(|_| card.clone()),
                };
            }
            if let Some((card, _)) = tool {
                let card = card.read(cx);
                return StageHeadline {
                    title: card.headline_title().to_owned(),
                    icon: Some(card.leading_icon()),
                    live,
                    summary: None,
                };
            }
            match progress {
                Some(StageProgress::Tool(card)) => {
                    let card = card.read(cx);
                    if let Some(title) = card.completed_command_headline() {
                        return StageHeadline {
                            title: title.to_owned(),
                            icon: Some(card.leading_icon()),
                            live: false,
                            summary: None,
                        };
                    }
                }
                Some(StageProgress::Thinking {
                    card,
                    completed_summary: Some(title),
                }) => {
                    return StageHeadline {
                        title: title.clone(),
                        icon: None,
                        live: false,
                        summary: Some(card.clone()),
                    };
                }
                _ => {}
            }
        }
        if self.stage_is_aggregate(segment, cx)
            && let Some((title, icon)) = self.stage_tool_title(segment, cx)
        {
            return StageHeadline {
                title,
                icon: Some(icon),
                live: false,
                summary: None,
            };
        }
        let summary = (!owns_progress || progress.is_none())
            .then(|| self.stage_summary_card(segment, cx))
            .flatten();
        StageHeadline {
            title: summary
                .as_ref()
                .and_then(|card| card.read(cx).summary_title().map(str::to_owned))
                .unwrap_or_else(|| "Thought".into()),
            icon: None,
            live: false,
            summary,
        }
    }

    pub(crate) fn reveal_segment(&mut self, segment: usize, cx: &mut Context<Self>) -> bool {
        let aggregate = self.stage_is_aggregate(segment, cx);
        let single_card = (!aggregate && self.stage_tool_calls(segment, cx).is_empty())
            .then(|| self.stage_thinking(segment))
            .flatten();
        if (!aggregate && single_card.is_none()) || !self.segment_can_expand(segment, cx) {
            return false;
        }
        let state = &mut self.segments[segment];
        if !state.expanded {
            state.expanded = true;
            if let Some(card) = single_card {
                card.update(cx, |card, cx| card.set_stage_expanded(true, cx));
            }
            self.sync_thinking_visibility(cx);
            cx.notify();
        }
        true
    }

    fn toggle_segment(&mut self, segment: usize, cx: &mut Context<Self>) {
        let single_card = (!self.stage_is_aggregate(segment, cx)
            && self.stage_tool_calls(segment, cx).is_empty())
        .then(|| self.stage_thinking(segment))
        .flatten();
        let state = &mut self.segments[segment];
        if state.expanded && state.sticky.pinned() {
            self.collapsed_sticky = Some(StickyCollapse::Segment(segment));
        }
        state.expanded = !state.expanded;
        let expanded = state.expanded;
        if let Some(card) = single_card {
            card.update(cx, |card, cx| card.set_stage_expanded(expanded, cx));
        }
        self.sync_thinking_visibility(cx);
        cx.notify();
    }

    pub(crate) fn render_segment(
        group: Entity<Self>,
        segment_index: usize,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        Self::render_segment_with_top(
            group,
            segment_index,
            Layout::CONVERSATION_STAGE_TOP,
            window,
            cx,
        )
    }

    pub(crate) fn render_segment_with_top(
        group: Entity<Self>,
        segment_index: usize,
        top: f32,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let state = group.read(cx);
        let aggregate = state.stage_is_aggregate(segment_index, cx);
        let thinking = state.stage_thinking(segment_index);
        let single_thinking = !aggregate
            && state.stage_tool_calls(segment_index, cx).is_empty()
            && thinking.is_some();
        let visible_children = state.visible_segment_children(segment_index, cx);
        let can_expand = state.segment_can_expand(segment_index, cx);
        let expanded = state.segments[segment_index].expanded && can_expand;
        let header_focus = state.segments[segment_index].header_focus.clone();
        let title_view = state.segments[segment_index].title_view.clone();
        let tool_ids = state
            .stage_tool_calls(segment_index, cx)
            .iter()
            .map(|card| card.entity_id())
            .collect::<Vec<_>>();
        let headline = state.stage_headline(segment_index, cx);
        let title = (aggregate || single_thinking).then(|| headline.title.clone());
        let tool_leads = matches!(
            visible_children.first(),
            Some(RunActivityChild::Tool(_) | RunActivityChild::ToolGroup(_))
        );
        let focus = state.segments[segment_index].focus.clone();
        let sticky = state.segments[segment_index].sticky.clone();
        let scrolls = state.segments[segment_index].scrolls.clone();
        let shimmering = headline.live;
        let activity_icon = headline.icon;
        let epoch = state.animation_epoch;
        if (aggregate || single_thinking)
            && state
                .stage_outer_summary_card(segment_index, cx)
                .is_some_and(|card| card.read(cx).header_focused(window))
        {
            header_focus.focus(window, cx);
        }
        if aggregate && !expanded {
            let focused_hidden_tool = tool_ids.iter().any(|id| {
                crate::disclosure::focus(format!("tool-disclosure-{id}").into(), window, cx)
                    .is_focused(window)
            });
            if focused_hidden_tool {
                header_focus.focus(window, cx);
            }
        } else if !aggregate
            && !single_thinking
            && header_focus.is_focused(window)
            && let Some(id) = tool_ids.first()
        {
            crate::disclosure::focus(format!("tool-disclosure-{id}").into(), window, cx)
                .focus(window, cx);
        }
        let children = visible_children;
        if !aggregate && thinking.is_none() && children.is_empty() {
            return div().h_0().into_any_element();
        }
        let colors = theme(cx).colors;
        let mut header = None;
        let mut rows = Vec::new();
        if let Some(title) = title {
            title_view.update(cx, |view, cx| {
                view.set(title.clone(), shimmering.then_some(epoch), cx)
            });
            let disclosure_group: gpui_kit::SharedString =
                format!("run-stage-{}-{segment_index}", group.entity_id()).into();
            let toggle = group.clone();
            let click_focus = header_focus.clone();
            let keyboard = group.clone();
            header = Some(
                div()
                    .id(("run-stage-toggle", segment_index))
                    .group(disclosure_group.clone())
                    .map(|row| {
                        #[cfg(test)]
                        let row = {
                            use gpui_kit::base::TestSupportExt as _;
                            row.test_support()
                        };
                        row
                    })
                    .track_focus(&header_focus)
                    .tab_stop(can_expand)
                    .when(can_expand, |row| row.aria_expanded(expanded))
                    .aria_label(if can_expand {
                        format!(
                            "{title}, {} stage activity",
                            if expanded { "collapse" } else { "expand" }
                        )
                    } else {
                        title.clone()
                    })
                    .when(can_expand, |row| {
                        row.focus_visible(move |row| row.bg(colors.bg_hover))
                    })
                    .w_full()
                    .min_w_0()
                    .h(px(Layout::CONVERSATION_TOOL_HEIGHT))
                    .text_size(px(Typography::CONVERSATION))
                    .line_height(gpui_kit::relative(Typography::CONVERSATION_LINE_HEIGHT))
                    .text_color(colors.conversation_stage)
                    .flex()
                    .items_center()
                    .gap(px(Layout::CONVERSATION_INLINE_GAP))
                    .when(can_expand, |row| {
                        row.cursor_pointer().on_mouse_up(
                            MouseButton::Left,
                            move |_: &MouseUpEvent, window, cx| {
                                click_focus.focus(window, cx);
                                toggle
                                    .update(cx, |group, cx| group.toggle_segment(segment_index, cx))
                            },
                        )
                    })
                    .on_key_down(move |event, window, cx| {
                        if can_expand && event.keystroke.key == "tab" {
                            if event.keystroke.modifiers.shift {
                                window.focus_prev(cx);
                            } else {
                                window.focus_next(cx);
                            }
                            cx.stop_propagation();
                        } else if can_expand
                            && matches!(event.keystroke.key.as_str(), "enter" | "space")
                        {
                            keyboard
                                .update(cx, |group, cx| group.toggle_segment(segment_index, cx));
                            cx.stop_propagation();
                        }
                    })
                    .when_some(activity_icon, |row, icon| {
                        row.child(
                            div()
                                .size(px(Layout::ICON_SIZE))
                                .flex_shrink_0()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(crate::icons::icon(icon, colors.conversation_stage)),
                        )
                    })
                    .child(
                        div()
                            .min_w_0()
                            .flex_shrink(1.)
                            .truncate()
                            .child(title_view.clone()),
                    )
                    .when(can_expand, |row| {
                        row.child(crate::disclosure::inline_chevron(
                            disclosure_group,
                            &header_focus,
                            expanded,
                            true,
                            colors,
                            window,
                        ))
                    })
                    .into_any_element(),
            );
        }
        let mut operation_index = 0;
        let sections = if aggregate {
            if expanded {
                Self::compact_sections(children, cx)
            } else {
                Self::split_sections(children)
            }
        } else if single_thinking {
            Self::split_sections(children)
        } else {
            vec![children]
        };
        if single_thinking && let Some(card) = thinking.clone() {
            let (has_source, truncated) = {
                let card_state = card.read(cx);
                (card_state.has_source_text(), card_state.truncated)
            };
            if expanded && has_source {
                rows.push(StickySection::content(
                    ThinkingBlock::render_stage_source(card, window, cx),
                    true,
                ));
            }
            if truncated {
                rows.push(StickySection::plain(
                    div()
                        .pt(px(Layout::PROCESS_ITEM_GAP))
                        .text_size(px(Typography::METADATA))
                        .text_color(colors.text_tertiary)
                        .child("Earlier thinking omitted")
                        .into_any_element(),
                ));
            }
        }
        for section in sections {
            if !aggregate {
                rows.extend(
                    section
                        .into_iter()
                        .filter(|_| !single_thinking)
                        .map(|child| {
                            let element = match child {
                                RunActivityChild::Tool(card) if !aggregate && shimmering => {
                                    let title = headline.title.clone();
                                    title_view.update(cx, |view, cx| {
                                        view.set(title.clone(), Some(epoch), cx)
                                    });
                                    ToolCard::render_with_title(
                                        card,
                                        false,
                                        Some((title, title_view.clone().into_any_element())),
                                        window,
                                        cx,
                                    )
                                }
                                RunActivityChild::Tool(card) if !aggregate => {
                                    title_view.update(cx, |view, cx| {
                                        view.set(card.read(cx).compact_title(), None, cx)
                                    });
                                    ToolCard::render(card, false, window, cx)
                                }
                                child => Self::render_child(child, window, cx),
                            };
                            StickySection::plain(element)
                        }),
                );
                continue;
            }
            if !expanded {
                continue;
            }
            let scroll = scrolls
                .get(operation_index)
                .cloned()
                .unwrap_or_else(ScrollHandle::new);
            let section_index = operation_index;
            operation_index += 1;
            let details = section
                .into_iter()
                .map(|child| match child {
                    RunActivityChild::Tool(card) => ToolCard::render(card, false, window, cx),
                    child => Self::render_child(child, window, cx),
                })
                .collect::<Vec<_>>();
            let reaches_header = rows.is_empty();
            rows.push(StickySection::content(
                div()
                    .relative()
                    .w_full()
                    .min_w_0()
                    .child(
                        div()
                            .id(gpui_kit::SharedString::from(format!(
                                "run-stage-content-{segment_index}-{section_index}"
                            )))
                            .w_full()
                            .min_w_0()
                            .max_h(px(Layout::TOOL_GROUP_MAX_HEIGHT))
                            .overflow_x_hidden()
                            .overflow_y_scroll()
                            .track_scroll(&scroll)
                            .on_scroll_wheel(crate::tool_card::contain_disclosure_scroll(&scroll))
                            .flex()
                            .flex_col()
                            .gap_0()
                            .children(details)
                            .tracked_scroll_edge_fade(
                                scroll.clone(),
                                colors.bg_base,
                                ScrollFade::compact()
                                    .vertical_gutter(px(Layout::DISCLOSURE_SCROLLBAR_GUTTER)),
                            ),
                    )
                    .child(crate::tool_card::disclosure_scrollbar(&scroll, cx))
                    .into_any_element(),
                reaches_header,
            ));
        }
        let content = if let Some(header) = header {
            StickyDisclosure::new(
                div()
                    .bg(colors.bg_base)
                    .block_mouse_except_scroll()
                    .child(header)
                    .into_any_element(),
                rows,
                sticky,
                expanded,
            )
            .into_any_element()
        } else {
            div()
                .flex()
                .flex_col()
                .children(rows.into_iter().map(|section| section.into_element()))
                .into_any_element()
        };
        render_rows::process_surface(&colors)
            .gap_0()
            .when(aggregate || single_thinking || tool_leads, |surface| {
                surface.pt(px(top))
            })
            .track_focus(&focus)
            .child(content)
            .into_any_element()
    }
}

pub(crate) fn format_run_clock(duration_ms: u64) -> String {
    let seconds = duration_ms / 1000;
    if seconds < 60 {
        return format!("{seconds}s");
    }
    if seconds < 3600 {
        return format!("{}m {}s", seconds / 60, seconds % 60);
    }
    format!("{}h {}m", seconds / 3600, seconds % 3600 / 60)
}
