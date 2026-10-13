use super::sticky_disclosure::{StickyDisclosure, StickySection, StickyState};
use super::*;
use gpui_kit::{AnyElement, MouseButton, MouseUpEvent, ScrollHandle, div, px};
#[cfg(test)]
use vega_conversation::types::ToolCallStatus;

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
    Artifact(Entity<ArtifactCard>),
}

struct RunActivitySegment {
    sticky: StickyState,
    expanded: bool,
    manually_expanded: bool,
    children: Vec<RunActivityChild>,
    focus: FocusHandle,
    header_focus: FocusHandle,
    scrolls: Vec<ScrollHandle>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RunPhase {
    Thinking,
    Responding,
    Tool,
    #[cfg(test)]
    Waiting,
}

pub(crate) enum StickyCollapse {
    Run,
    Segment(usize),
}

pub(crate) struct RunActivityGroup {
    sticky: StickyState,
    collapsed_sticky: Option<StickyCollapse>,
    #[cfg(test)]
    clock_ticks: usize,
    started_at: Option<Instant>,
    animation_epoch: Instant,
    clock_task: Option<gpui_kit::Task<()>>,
    phase: RunPhase,
    waiting_for_user: bool,
    current_thinking: Option<Entity<ThinkingBlock>>,
    process_copies: HashSet<u64>,
    final_confirmed: bool,
    body_in_details: bool,
    scroll: ScrollHandle,
    detail_anchor: Option<(usize, f32)>,
    focus: FocusHandle,
    collapse_pending: bool,
    manually_expanded: bool,
    status: RunActivityStatus,
    execution_duration_ms: Option<u64>,
    expanded: bool,
    terminal: bool,
    segments: Vec<RunActivitySegment>,
}

impl RunActivityGroup {
    pub(crate) fn response_checkpoint(&mut self, cx: &mut Context<Self>) -> usize {
        if let Some(card) = self.current_thinking.take() {
            card.update(cx, |card, cx| {
                card.shimmer_epoch = None;
                cx.notify();
            });
        }
        self.segments.len()
    }

    pub(crate) fn rollback_response(&mut self, segments: usize, cx: &mut Context<Self>) {
        self.response_checkpoint(cx);
        self.segments.truncate(segments);
        self.phase = RunPhase::Thinking;
        cx.notify();
    }
    pub(crate) fn start_clock(&mut self, started_at: Instant, cx: &mut Context<Self>) {
        if self.started_at.is_none() && !self.terminal {
            self.started_at = Some(started_at);
            self.animation_epoch = started_at;
            cx.notify();
        }
    }

    fn schedule_clock(&mut self, cx: &mut Context<Self>) {
        if self.terminal || self.started_at.is_none() || self.clock_task.is_some() {
            return;
        }
        self.clock_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            let _ = this.update(cx, |group, cx| {
                group.clock_task = None;
                #[cfg(test)]
                {
                    group.clock_ticks += 1;
                }
                if !group.terminal {
                    cx.notify();
                }
            });
        }));
    }

    pub(crate) fn set_phase(&mut self, phase: RunPhase, cx: &mut Context<Self>) {
        if !self.terminal && self.phase != phase {
            self.phase = phase;
            if let Some(card) = self.current_thinking.take() {
                card.update(cx, |card, cx| {
                    card.shimmer_epoch = None;
                    cx.notify();
                });
            }
            cx.notify();
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
        cx.notify();
    }

    pub(crate) fn set_thinking(&mut self, card: Entity<ThinkingBlock>, cx: &mut Context<Self>) {
        self.phase = RunPhase::Thinking;
        card.update(cx, |card, cx| {
            card.shimmer_epoch = None;
            cx.notify();
        });
        self.current_thinking = Some(card);
        cx.notify();
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

    pub(crate) fn settle_final(&mut self, reading: bool, cx: &mut Context<Self>) {
        if !self.collapse_pending || reading || self.manually_expanded {
            return;
        }
        self.collapse_pending = false;
        self.expanded = false;
        self.body_in_details = true;
        cx.notify();
    }

    pub(crate) fn process_in_details(&self) -> bool {
        self.final_confirmed && self.body_in_details
    }

    pub(crate) fn invalidate_final(&mut self, cx: &mut Context<Self>) {
        self.final_confirmed = false;
        self.collapse_pending = false;
        self.body_in_details = false;
        cx.notify();
    }

    pub(crate) fn reveal(&mut self, cx: &mut Context<Self>) {
        self.expanded = true;
        self.manually_expanded = true;
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
        cx.notify();
    }

    pub(crate) fn reading(&self, window: &Window, cx: &App) -> bool {
        self.manually_expanded
            || self.focus.contains_focused(window, cx)
            || self.segments.iter().enumerate().any(|(index, segment)| {
                let stage_thinking = self.stage_thinking(index);
                let thinking_can_expand =
                    stage_thinking.is_some() && self.stage_summary_title(index, cx).is_none();
                let thinking_visible =
                    stage_thinking.is_none() || (segment.expanded && thinking_can_expand);
                (segment.manually_expanded && thinking_can_expand)
                    || (segment.focus.contains_focused(window, cx)
                        && (!segment.header_focus.is_focused(window) || thinking_can_expand))
                    || segment.children.iter().any(|child| match child {
                        RunActivityChild::Thinking(card) => {
                            thinking_visible && card.read(cx).expanded
                        }
                        RunActivityChild::Tool(card) => card.read(cx).is_expanded(),
                        RunActivityChild::ToolGroup(group) => {
                            (stage_thinking.is_none() && group.read(cx).expanded())
                                || group
                                    .read(cx)
                                    .children()
                                    .iter()
                                    .any(|card| card.read(cx).is_expanded())
                        }
                        RunActivityChild::Artifact(_) => false,
                    })
            })
    }

    fn visible_segment_children(&self, segment: usize, cx: &App) -> Vec<RunActivityChild> {
        let has_summary = self.stage_summary_title(segment, cx).is_some();
        self.segment_children(segment)
            .into_iter()
            .filter(|child| {
                (!self.process_in_details() || matches!(child, RunActivityChild::Artifact(_)))
                    && (!has_summary || !matches!(child, RunActivityChild::Thinking(_)))
            })
            .collect()
    }

    fn leading_segment_child_count(&self, segment: usize) -> usize {
        self.segments
            .get(segment)
            .map(|segment| {
                segment
                    .children
                    .iter()
                    .take_while(|child| !matches!(child, RunActivityChild::Thinking(_)))
                    .count()
            })
            .unwrap_or(0)
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
            #[cfg(test)]
            clock_ticks: 0,
            started_at: None,
            animation_epoch: Instant::now(),
            clock_task: None,
            phase: RunPhase::Thinking,
            waiting_for_user: false,
            current_thinking: None,
            process_copies: HashSet::new(),
            final_confirmed: false,
            body_in_details: false,
            scroll: ScrollHandle::new(),
            detail_anchor: None,
            focus: cx.focus_handle(),
            collapse_pending: false,
            manually_expanded: false,
            status,
            execution_duration_ms,
            expanded,
            terminal: status != RunActivityStatus::Running,
            segments: Vec::new(),
        }
    }

    #[cfg(test)]
    pub(crate) fn test_projection(&self) -> (RunActivityStatus, Option<u64>, bool, bool) {
        (
            self.status,
            self.execution_duration_ms,
            self.expanded,
            self.terminal,
        )
    }

    #[cfg(test)]
    pub(crate) fn test_label(&self) -> String {
        self.label()
    }

    #[cfg(test)]
    pub(crate) fn test_activity_label(&self, cx: &App) -> String {
        self.activity_presentation(cx).0
    }

    #[cfg(test)]
    pub(crate) fn test_activity_presentation(
        &self,
        cx: &App,
    ) -> (String, crate::icons::Icon, bool) {
        self.activity_presentation(cx)
    }

    #[cfg(test)]
    pub(crate) fn test_clock(&self) -> (Option<Instant>, bool, RunPhase) {
        (
            self.started_at,
            self.clock_task.is_some(),
            if self.waiting_for_user {
                RunPhase::Waiting
            } else {
                self.phase
            },
        )
    }

    #[cfg(test)]
    pub(crate) fn test_clock_ticks(&self) -> usize {
        self.clock_ticks
    }

    #[cfg(test)]
    pub(crate) fn test_focus(&self) -> Option<FocusHandle> {
        Some(self.focus.clone())
    }

    #[cfg(test)]
    pub(crate) fn test_scroll_handle(&self) -> ScrollHandle {
        self.scroll.clone()
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

    pub(crate) fn start_segment(&mut self, cx: &mut Context<Self>) -> usize {
        self.segments.push(RunActivitySegment {
            sticky: StickyState::default(),
            expanded: false,
            manually_expanded: false,
            children: Vec::new(),
            focus: cx.focus_handle(),
            header_focus: cx.focus_handle().tab_stop(true),
            scrolls: vec![ScrollHandle::new()],
        });
        cx.notify();
        self.segments.len() - 1
    }

    pub(crate) fn append_thinking(
        &mut self,
        segment: usize,
        card: Entity<ThinkingBlock>,
        cx: &mut Context<Self>,
    ) {
        cx.observe(&card, |_, _, cx| cx.notify()).detach();
        if let Some(segment) = self.segments.get_mut(segment) {
            segment.children.push(RunActivityChild::Thinking(card));
        }
        cx.notify();
    }

    pub(crate) fn append_tool(
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
        cx.notify();
    }

    fn observe_tool_group(group: &Entity<ToolActivityGroup>, cx: &mut Context<Self>) {
        cx.observe(group, |_, _, cx| cx.notify()).detach();
    }

    pub(crate) fn insert_after_tool(
        &mut self,
        tool: &Entity<ToolCard>,
        artifact: Entity<ArtifactCard>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some((segment_index, index)) =
            self.segments
                .iter()
                .enumerate()
                .find_map(|(segment_index, segment)| {
                    segment
                        .children
                        .iter()
                        .position(|child| match child {
                            RunActivityChild::Tool(card) => card == tool,
                            RunActivityChild::ToolGroup(group) => group.read(cx).contains(tool),
                            RunActivityChild::Thinking(_) | RunActivityChild::Artifact(_) => false,
                        })
                        .map(|index| (segment_index, index))
                })
        else {
            return false;
        };
        if matches!(
            self.segments[segment_index].children.get(index + 1),
            Some(RunActivityChild::Artifact(_))
        ) {
            return false;
        }
        cx.observe(&artifact, |_, _, cx| cx.notify()).detach();
        self.segments[segment_index]
            .children
            .insert(index + 1, RunActivityChild::Artifact(artifact));
        self.segments[segment_index]
            .scrolls
            .push(ScrollHandle::new());
        cx.notify();
        true
    }

    pub(crate) fn contains_tool(&self, card: &Entity<ToolCard>, cx: &App) -> bool {
        self.segments
            .iter()
            .flat_map(|segment| segment.children.iter())
            .any(|child| match child {
                RunActivityChild::Tool(owned) => owned == card,
                RunActivityChild::ToolGroup(group) => group.read(cx).contains(card),
                RunActivityChild::Thinking(_) | RunActivityChild::Artifact(_) => false,
            })
    }

    pub(crate) fn contains_tool_group(&self, group: &Entity<ToolActivityGroup>) -> bool {
        self.segments
            .iter()
            .flat_map(|segment| segment.children.iter())
            .any(|child| matches!(child, RunActivityChild::ToolGroup(owned) if owned == group))
    }

    pub(crate) fn contains_artifact(&self, card: &Entity<ArtifactCard>) -> bool {
        self.segments
            .iter()
            .flat_map(|segment| segment.children.iter())
            .any(|child| matches!(child, RunActivityChild::Artifact(owned) if owned == card))
    }

    pub(crate) fn artifact_follows_tool(
        &self,
        tool: &Entity<ToolCard>,
        artifact: &Entity<ArtifactCard>,
        cx: &App,
    ) -> bool {
        self.segments.iter().any(|segment| {
            segment.children.windows(2).any(|children| {
                let owns_tool = match &children[0] {
                    RunActivityChild::Tool(card) => card == tool,
                    RunActivityChild::ToolGroup(group) => group.read(cx).contains(tool),
                    RunActivityChild::Thinking(_) | RunActivityChild::Artifact(_) => false,
                };
                owns_tool
                    && matches!(&children[1], RunActivityChild::Artifact(card) if card == artifact)
            })
        })
    }

    #[cfg(test)]
    pub(crate) fn row_count(&self, cx: &App) -> usize {
        1 + self
            .segments
            .iter()
            .map(|segment| {
                if self.expanded {
                    segment
                        .children
                        .iter()
                        .map(|child| Self::child_row_count(child, cx))
                        .sum()
                } else {
                    0
                }
            })
            .sum::<usize>()
    }

    pub(crate) fn segment_row_count(&self, segment: usize, cx: &App) -> usize {
        let has_stage = self.stage_thinking(segment).is_some();
        let children = self.visible_segment_children(segment, cx);
        let children = if has_stage {
            Self::compact_children(children, cx)
        } else {
            children
        };
        let expanded = has_stage
            && self.segments[segment].expanded
            && children
                .iter()
                .any(|child| matches!(child, RunActivityChild::Thinking(_)));
        usize::from(has_stage)
            + children
                .iter()
                .filter(|child| {
                    !has_stage || expanded || !matches!(child, RunActivityChild::Thinking(_))
                })
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
        let mut sections = Vec::new();
        let mut operations = Vec::new();
        for child in Self::compact_children(children, cx) {
            if matches!(child, RunActivityChild::Artifact(_)) {
                if !operations.is_empty() {
                    sections.push(std::mem::take(&mut operations));
                }
                sections.push(vec![child]);
            } else {
                operations.push(child);
            }
        }
        if !operations.is_empty() {
            sections.push(operations);
        }
        sections
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
        if self.stage_thinking(segment).is_some() {
            let children = self.visible_segment_children(segment, cx);
            let (leading, details) = children.split_at(self.leading_segment_child_count(segment));
            let expanded = self.segments[segment].expanded
                && details
                    .iter()
                    .any(|child| matches!(child, RunActivityChild::Thinking(_)));
            let leading_height = Self::compact_children(leading.to_vec(), cx)
                .iter()
                .map(|child| Self::estimated_child_height(child, width, cx))
                .sum::<f32>();
            let details_height = Self::compact_sections(details.to_vec(), cx)
                .iter()
                .map(|section| {
                    if matches!(section.first(), Some(RunActivityChild::Artifact(_))) {
                        section
                            .iter()
                            .map(|child| Self::estimated_child_height(child, width, cx))
                            .sum::<f32>()
                    } else if expanded {
                        section
                            .iter()
                            .map(|child| Self::estimated_child_height(child, width, cx))
                            .sum::<f32>()
                            .min(Layout::TOOL_GROUP_MAX_HEIGHT)
                    } else {
                        section
                            .iter()
                            .filter(|child| matches!(child, RunActivityChild::Tool(_)))
                            .map(|child| Self::estimated_child_height(child, width, cx))
                            .sum::<f32>()
                    }
                })
                .sum::<f32>();
            return top + leading_height + Layout::CONVERSATION_TOOL_HEIGHT + details_height;
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
            RunActivityChild::Artifact(card) => card.read(cx).row_count() as f32 * ROW_HEIGHT,
        }
    }

    pub(crate) fn estimated_header_height(&self) -> f32 {
        Layout::CONVERSATION_CLOCK_HEIGHT
            + Layout::CONVERSATION_DIVIDER_GAP
            + Layout::CONVERSATION_DIVIDER_HEIGHT
    }

    fn child_row_count(child: &RunActivityChild, cx: &App) -> usize {
        match child {
            RunActivityChild::Thinking(card) => card.read(cx).row_count(),
            RunActivityChild::Tool(card) => card.read(cx).row_count(),
            RunActivityChild::ToolGroup(group) => group.read(cx).row_count(cx),
            RunActivityChild::Artifact(card) => card.read(cx).row_count(),
        }
    }

    pub(crate) fn complete(
        &mut self,
        status: RunActivityStatus,
        execution_duration_ms: Option<u64>,
        cx: &mut Context<Self>,
    ) {
        if self.terminal {
            return;
        }
        if status != RunActivityStatus::Completed {
            self.invalidate_final(cx);
        }
        self.status = status;
        self.execution_duration_ms = execution_duration_ms;
        self.terminal = true;
        if let Some(card) = self.current_thinking.take() {
            card.update(cx, |card, cx| {
                card.shimmer_epoch = None;
                cx.notify();
            });
        }
        self.clock_task = None;
        cx.notify();
    }

    pub(crate) fn toggle_expanded(&mut self, cx: &mut Context<Self>) {
        if self.expanded && self.sticky.pinned() {
            self.collapsed_sticky = Some(StickyCollapse::Run);
        }
        self.expanded = !self.expanded;
        self.manually_expanded = self.expanded;
        if self.final_confirmed {
            self.body_in_details = true;
        }
        cx.notify();
    }

    pub(crate) fn take_sticky_collapse(&mut self) -> Option<StickyCollapse> {
        self.collapsed_sticky.take()
    }

    fn label(&self) -> String {
        let duration = self.execution_duration_ms.or_else(|| {
            self.started_at
                .filter(|_| !self.terminal)
                .map(|start| start.elapsed().as_millis().min(u128::from(u64::MAX)) as u64)
        });
        let status = match self.status {
            RunActivityStatus::Running if self.waiting_for_user => "等待确认 · 已用",
            RunActivityStatus::Running => "Working for",
            RunActivityStatus::Completed => "Worked for",
            RunActivityStatus::Failed => "已失败 · 用时",
            RunActivityStatus::Interrupted => "已停止 · 用时",
        };
        duration
            .map(|value| format!("{status} {}", format_run_clock(value)))
            .unwrap_or_else(|| match self.status {
                RunActivityStatus::Running if self.waiting_for_user => "等待确认".into(),
                RunActivityStatus::Running => "运行中".into(),
                RunActivityStatus::Completed => "已完成".into(),
                RunActivityStatus::Failed => "已失败".into(),
                RunActivityStatus::Interrupted => "已停止".into(),
            })
    }

    #[cfg(test)]
    fn activity_presentation(&self, cx: &App) -> (String, crate::icons::Icon, bool) {
        use crate::icons::Icon;
        use crate::tool_card::ToolActivityState;
        if self.terminal {
            return (
                self.label(),
                match self.status {
                    RunActivityStatus::Completed => Icon::Check,
                    RunActivityStatus::Failed => Icon::Warning,
                    RunActivityStatus::Interrupted => Icon::Close,
                    RunActivityStatus::Running => Icon::Bot,
                },
                false,
            );
        }
        if self.waiting_for_user {
            return ("等待确认".into(), Icon::Hand, false);
        }
        let mut categories = Vec::new();
        let mut pending = false;
        let mut approved = false;
        for child in self.children() {
            let cards = match child {
                RunActivityChild::Tool(card) => vec![card],
                RunActivityChild::ToolGroup(group) => group.read(cx).children(),
                _ => Vec::new(),
            };
            for card in cards {
                let card = card.read(cx);
                pending |= card.call_status() == ToolCallStatus::PendingApproval;
                approved |= card.call_status() == ToolCallStatus::Approved;
                if card.call_status() == ToolCallStatus::Running
                    && card.activity_state() == ToolActivityState::Active
                {
                    let action = (card.activity_category(), card.activity_action_name());
                    if !categories.contains(&action) {
                        categories.push(action);
                    }
                }
            }
        }
        if !categories.is_empty() {
            let actions = categories
                .iter()
                .take(2)
                .map(|(_, action)| action.as_str())
                .collect::<Vec<_>>()
                .join("、");
            let suffix = if categories.len() > 2 {
                format!("等 {} 项", categories.len())
            } else {
                String::new()
            };
            let icon = if categories.len() == 1 {
                categories[0].0.icon()
            } else {
                Icon::Summary
            };
            return (format!("正在{actions}{suffix}"), icon, false);
        }
        if pending {
            return ("等待确认".into(), Icon::Hand, false);
        }
        if approved {
            return ("准备调用工具".into(), Icon::Summary, false);
        }
        let summary = (self.phase == RunPhase::Thinking)
            .then(|| {
                self.current_thinking
                    .as_ref()
                    .and_then(|card| card.read(cx).summary_title())
            })
            .flatten();
        (summary.unwrap_or("Thinking").to_owned(), Icon::Bot, true)
    }

    pub(crate) fn render(group: Entity<Self>, window: &mut Window, cx: &mut App) -> AnyElement {
        let details = if group.read(cx).expanded && group.read(cx).process_in_details() {
            group
                .read(cx)
                .children()
                .into_iter()
                .filter(|child| !matches!(child, RunActivityChild::Artifact(_)))
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
        let header_focus =
            crate::disclosure::focus(disclosure_group.clone(), window, cx).tab_stop(can_expand);
        let header = div()
            .id(("run-activity-toggle", group.entity_id()))
            .group(disclosure_group.clone())
            .track_focus(&header_focus)
            .tab_stop(can_expand)
            .aria_expanded(expanded)
            .aria_label(format!(
                "{label}，{}本轮过程",
                if expanded { "收起" } else { "展开" }
            ))
            .focus_visible(move |row| row.bg(colors.bg_hover))
            .debug_selector(|| "run-activity-toggle".to_string())
            .relative()
            .w_full()
            .h(px(Layout::CONVERSATION_CLOCK_HEIGHT))
            .text_size(px(Typography::CONVERSATION))
            .line_height(gpui_kit::relative(Typography::CONVERSATION_LINE_HEIGHT))
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(Layout::CONVERSATION_INLINE_GAP))
            .on_key_down(move |event, _, cx| {
                if can_expand && matches!(event.keystroke.key.as_str(), "enter" | "space") {
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
                    .debug_selector(|| "run-activity-status".into())
                    .min_w_0()
                    .flex_shrink(1.)
                    .truncate()
                    .text_color(status_color)
                    .child(label),
            )
            .child(crate::disclosure::inline_chevron(
                disclosure_group,
                "run-activity-chevron".into(),
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
                .debug_selector(|| "run-activity-divider".into())
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
                            .debug_selector(|| "run-activity-content".into())
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
                            .children(details),
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
            .debug_selector(|| "run-activity-group".into())
            .into_any_element()
    }

    pub(crate) fn render_child(
        child: RunActivityChild,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        match child {
            RunActivityChild::Thinking(card) => {
                let id = card.entity_id();
                div()
                    .debug_selector(move || format!("run-stage-source-{id}"))
                    .w_full()
                    .min_w_0()
                    .child(card)
                    .into_any_element()
            }
            RunActivityChild::Tool(card) => {
                ToolCard::render(card, "tool-activity-single-row".into(), false, window, cx)
            }
            RunActivityChild::ToolGroup(group) => ToolActivityGroup::render(group, window, cx),
            RunActivityChild::Artifact(card) => {
                let count = card.read(cx).row_count();
                let id = card.entity_id();
                div()
                    .debug_selector(move || format!("run-artifact-{id}"))
                    .w_full()
                    .children(
                        (0..count)
                            .map(|row| ArtifactCard::render_row(card.clone(), row, window, cx)),
                    )
                    .into_any_element()
            }
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

    fn stage_summary_title(&self, segment: usize, cx: &App) -> Option<String> {
        self.segments
            .get(segment)?
            .children
            .iter()
            .rev()
            .find_map(|child| {
                if let RunActivityChild::Thinking(card) = child {
                    card.read(cx).summary_title().map(str::to_owned)
                } else {
                    None
                }
            })
    }

    pub(crate) fn stage_title(&self, segment: usize, cx: &App) -> String {
        self.stage_summary_title(segment, cx)
            .unwrap_or_else(|| "Thinking".to_owned())
    }

    #[cfg(test)]
    pub(crate) fn segment_expanded(&self, segment: usize) -> bool {
        self.segments[segment].expanded
    }

    pub(crate) fn reveal_segment(&mut self, segment: usize, cx: &mut Context<Self>) -> bool {
        if self.stage_thinking(segment).is_none() || self.stage_summary_title(segment, cx).is_some()
        {
            return false;
        }
        let state = &mut self.segments[segment];
        if !state.expanded {
            state.expanded = true;
            cx.notify();
        }
        true
    }

    fn toggle_segment(&mut self, segment: usize, cx: &mut Context<Self>) {
        let state = &mut self.segments[segment];
        if state.expanded && state.sticky.pinned() {
            self.collapsed_sticky = Some(StickyCollapse::Segment(segment));
        }
        state.expanded = !state.expanded;
        state.manually_expanded = state.expanded;
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
        let thinking = state.stage_thinking(segment_index);
        let mut children = state.visible_segment_children(segment_index, cx);
        if thinking.is_none() && children.is_empty() {
            return div().h_0().into_any_element();
        }
        let tool_leads = matches!(
            children.first(),
            Some(RunActivityChild::Tool(_) | RunActivityChild::ToolGroup(_))
        );
        let leading_children = if thinking.is_some() {
            children
                .drain(..state.leading_segment_child_count(segment_index))
                .collect()
        } else {
            Vec::new()
        };
        let colors = theme(cx).colors;
        let focus = state.segments[segment_index].focus.clone();
        let can_expand = children
            .iter()
            .any(|child| matches!(child, RunActivityChild::Thinking(_)));
        let expanded = state.segments[segment_index].expanded && can_expand;
        let sticky = state.segments[segment_index].sticky.clone();
        let scrolls = state.segments[segment_index].scrolls.clone();
        let shimmering = !state.terminal
            && !state.waiting_for_user
            && state.phase == RunPhase::Thinking
            && thinking.is_some()
            && state.current_thinking == thinking;
        let epoch = state.animation_epoch;
        let mut header = None;
        let mut rows = Vec::new();
        if thinking.is_some() {
            let title = state.stage_title(segment_index, cx);
            let disclosure_group: gpui_kit::SharedString =
                format!("run-stage-{}-{segment_index}", group.entity_id()).into();
            let header_focus = state.segments[segment_index].header_focus.clone();
            let toggle = group.clone();
            let click_focus = header_focus.clone();
            let keyboard = group.clone();
            header = Some(
                div()
                    .id(("run-stage-toggle", segment_index))
                    .debug_selector(move || format!("run-stage-toggle-{segment_index}"))
                    .group(disclosure_group.clone())
                    .track_focus(&header_focus)
                    .tab_stop(can_expand)
                    .when(can_expand, |row| row.aria_expanded(expanded))
                    .aria_label(if can_expand {
                        format!(
                            "{title}，{}阶段过程",
                            if expanded { "收起" } else { "展开" }
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
                    .on_key_down(move |event, _, cx| {
                        if can_expand && matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            keyboard
                                .update(cx, |group, cx| group.toggle_segment(segment_index, cx));
                            cx.stop_propagation();
                        }
                    })
                    .child(
                        div()
                            .debug_selector(move || format!("run-stage-title-{segment_index}"))
                            .min_w_0()
                            .flex_shrink(1.)
                            .truncate()
                            .child(if shimmering {
                                shimmer::stage_text(title, epoch, window, cx)
                            } else {
                                div().child(title).into_any_element()
                            }),
                    )
                    .when(can_expand, |row| {
                        row.child(crate::disclosure::inline_chevron(
                            disclosure_group,
                            format!("run-stage-chevron-{segment_index}"),
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
        let mut tool_index = 0;
        let leading_rows = Self::compact_children(leading_children, cx)
            .into_iter()
            .map(|child| match child {
                RunActivityChild::Tool(card) => {
                    let row = ToolCard::render(
                        card,
                        format!("run-stage-tool-{segment_index}-{tool_index}"),
                        false,
                        window,
                        cx,
                    );
                    tool_index += 1;
                    row
                }
                child => Self::render_child(child, window, cx),
            })
            .collect::<Vec<_>>();
        let mut operation_index = 0;
        let sections = if thinking.is_some() {
            Self::compact_sections(children, cx)
        } else {
            vec![children]
        };
        for section in sections {
            if thinking.is_none() || matches!(section.first(), Some(RunActivityChild::Artifact(_)))
            {
                rows.extend(
                    section
                        .into_iter()
                        .map(|child| StickySection::plain(Self::render_child(child, window, cx))),
                );
                continue;
            }
            let scroll = scrolls
                .get(operation_index)
                .cloned()
                .unwrap_or_else(ScrollHandle::new);
            let section_index = operation_index;
            operation_index += 1;
            if !expanded {
                rows.extend(section.into_iter().filter_map(|child| {
                    if let RunActivityChild::Tool(card) = child {
                        let row = ToolCard::render(
                            card,
                            format!("run-stage-tool-{segment_index}-{tool_index}"),
                            false,
                            window,
                            cx,
                        );
                        tool_index += 1;
                        Some(StickySection::plain(row))
                    } else {
                        None
                    }
                }));
                continue;
            }
            let details = section
                .into_iter()
                .map(|child| match child {
                    RunActivityChild::Tool(card) => {
                        let row = ToolCard::render(
                            card,
                            format!("run-stage-tool-{segment_index}-{tool_index}"),
                            false,
                            window,
                            cx,
                        );
                        tool_index += 1;
                        row
                    }
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
                            .debug_selector(move || {
                                if section_index == 0 {
                                    format!("run-stage-content-{segment_index}")
                                } else {
                                    format!("run-stage-content-{segment_index}-{section_index}")
                                }
                            })
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
                            .children(details),
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
        let content = if leading_rows.is_empty() {
            content
        } else {
            div()
                .flex()
                .flex_col()
                .children(leading_rows)
                .child(content)
                .into_any_element()
        };
        render_rows::process_surface(&colors)
            .gap_0()
            .when(thinking.is_some() || tool_leads, |surface| {
                surface.pt(px(top))
            })
            .debug_selector(move || format!("run-timeline-segment-{segment_index}"))
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
