use super::*;
use crate::interaction::InteractionExt;
use gpui_kit::{AnyElement, MouseButton, MouseUpEvent, ScrollHandle, div, px};
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
    children: Vec<RunActivityChild>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RunPhase {
    Thinking,
    Responding,
    Tool,
    #[cfg(test)]
    Waiting,
}

pub(crate) struct RunActivityGroup {
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
        if !self.collapse_pending {
            return;
        }
        self.collapse_pending = false;
        if !reading && !self.manually_expanded {
            self.expanded = false;
            self.body_in_details = true;
        }
        cx.notify();
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
            || self.segments.iter().any(|segment| {
                segment.children.iter().any(|child| match child {
                    RunActivityChild::Thinking(card) => card.read(cx).expanded,
                    RunActivityChild::Tool(card) => card.read(cx).is_expanded(),
                    RunActivityChild::ToolGroup(group) => {
                        group.read(cx).expanded()
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

    fn visible_segment_children(&self, segment: usize, _cx: &App) -> Vec<RunActivityChild> {
        self.segment_children(segment)
            .into_iter()
            .filter(|child| matches!(child, RunActivityChild::Artifact(_)))
            .collect()
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
            children: Vec::new(),
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
        self.visible_segment_children(segment, cx)
            .iter()
            .map(|child| Self::child_row_count(child, cx))
            .sum()
    }

    pub(crate) fn estimated_segment_height(&self, segment: usize, width: f32, cx: &App) -> f32 {
        let children = self.visible_segment_children(segment, cx);
        let mut height = 0.;
        for (index, child) in children.iter().enumerate() {
            if index > 0 {
                height += Layout::PROCESS_ITEM_GAP;
            }
            height += Self::estimated_child_height(child, width, cx);
            if height >= Layout::TOOL_GROUP_MAX_HEIGHT {
                break;
            }
        }
        height.min(Layout::TOOL_GROUP_MAX_HEIGHT)
    }

    pub(crate) fn estimated_child_height(child: &RunActivityChild, width: f32, cx: &App) -> f32 {
        match child {
            RunActivityChild::Thinking(card) => card.read(cx).estimated_height(width),
            RunActivityChild::Tool(card) => card.read(cx).estimated_height(),
            RunActivityChild::ToolGroup(group) => group.read(cx).estimated_height(cx),
            RunActivityChild::Artifact(card) => card.read(cx).row_count() as f32 * ROW_HEIGHT + 12.,
        }
    }

    pub(crate) fn estimated_header_height(&self) -> f32 {
        Layout::DISCLOSURE_HEADER_HEIGHT
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
        self.expanded = !self.expanded;
        self.manually_expanded = self.expanded;
        if !self.expanded && self.final_confirmed {
            self.body_in_details = true;
        }
        cx.notify();
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
                RunActivityStatus::Running => "Working".into(),
                RunActivityStatus::Completed => "已完成".into(),
                RunActivityStatus::Failed => "已失败".into(),
                RunActivityStatus::Interrupted => "已停止".into(),
            })
    }

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

    pub(crate) fn exception_label(&self, cx: &App) -> Option<&'static str> {
        use crate::tool_card::ToolActivityState;
        let state = self
            .children()
            .into_iter()
            .flat_map(|child| match child {
                RunActivityChild::Tool(card) => vec![card],
                RunActivityChild::ToolGroup(group) => group.read(cx).children(),
                _ => Vec::new(),
            })
            .map(|card| card.read(cx).activity_state())
            .filter(|state| {
                matches!(
                    state,
                    ToolActivityState::Failed
                        | ToolActivityState::Rejected
                        | ToolActivityState::Cancelled
                )
            })
            .max_by_key(|state| state.priority());
        state.map(|state| match state {
            ToolActivityState::Failed => "含失败操作",
            ToolActivityState::Rejected => "含已拒绝操作",
            _ => "含已取消操作",
        })
    }

    pub(crate) fn render(group: Entity<Self>, window: &mut Window, cx: &mut App) -> AnyElement {
        let details = if group.read(cx).expanded {
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
        let (label, glyph, shimmering) = group_ref.activity_presentation(cx);
        let clock_label = (!group_ref.terminal).then(|| {
            let label = group_ref.label();
            label
                .strip_prefix("等待确认 · ")
                .unwrap_or(&label)
                .to_owned()
        });
        let exception = group_ref.exception_label(cx);
        let status_color = if group_ref.status == RunActivityStatus::Failed {
            colors.danger
        } else {
            colors.text_secondary
        };
        let epoch = group_ref.animation_epoch;
        let can_expand = group_ref.has_children() || !group_ref.process_copies.is_empty();
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
            .border_1()
            .border_color(gpui_kit::transparent_black())
            .focus_visible(move |row| row.border_color(colors.accent))
            .debug_selector(|| "run-activity-toggle".to_string())
            .relative()
            .w_full()
            .h(px(Layout::DISCLOSURE_HEADER_HEIGHT))
            .text_size(px(Typography::BODY))
            .line_height(gpui_kit::relative(Typography::BODY_LINE_HEIGHT))
            .min_w_0()
            .flex()
            .items_center()
            .gap_2()
            .px(px(Layout::DISCLOSURE_PADDING_X))
            .rounded(px(Layout::DISCLOSURE_RADIUS))
            .interactive(colors, can_expand, false)
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
                    .debug_selector(|| "run-activity-icon".into())
                    .size(px(Layout::ICON_SIZE))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(crate::icons::icon(glyph, status_color)),
            )
            .child(
                div()
                    .debug_selector(|| "run-activity-status".into())
                    .min_w_0()
                    .flex_shrink(1.)
                    .truncate()
                    .text_color(status_color)
                    .child(if shimmering {
                        shimmer::text(label, epoch, window, cx)
                    } else {
                        div().child(label).into_any_element()
                    }),
            )
            .when_some(clock_label, |row, label| {
                row.child(
                    div()
                        .debug_selector(|| "run-activity-clock".into())
                        .flex_shrink_0()
                        .whitespace_nowrap()
                        .text_size(px(Typography::METADATA))
                        .text_color(colors.text_tertiary)
                        .child(format!("· {label}")),
                )
            })
            .when_some(exception, |row, label| {
                row.child(
                    div()
                        .debug_selector(|| "run-activity-exception".into())
                        .min_w_0()
                        .truncate()
                        .text_size(px(Typography::METADATA))
                        .text_color(colors.danger)
                        .child(label),
                )
            })
            .child(div().flex_1().min_w_0())
            .child(crate::disclosure::chevron(
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
        render_rows::process_surface(&colors)
            .child(header)
            .when(expanded && !details.is_empty(), |column| {
                column.child(
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
                                .on_scroll_wheel(crate::tool_card::contain_disclosure_scroll(
                                    &scroll,
                                ))
                                .flex()
                                .flex_col()
                                .gap(px(Layout::PROCESS_ITEM_GAP))
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
                        ),
                )
            })
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
                div().w_full().min_w_0().child(card).into_any_element()
            }
            RunActivityChild::Tool(card) => {
                ToolCard::render(card, "tool-activity-single-row".into(), false, window, cx)
            }
            RunActivityChild::ToolGroup(group) => ToolActivityGroup::render(group, window, cx),
            RunActivityChild::Artifact(card) => {
                let count = card.read(cx).row_count();
                div()
                    .w_full()
                    .children(
                        (0..count)
                            .map(|row| ArtifactCard::render_row(card.clone(), row, window, cx)),
                    )
                    .into_any_element()
            }
        }
    }

    pub(crate) fn render_segment(
        group: Entity<Self>,
        segment_index: usize,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let children = group.read(cx).visible_segment_children(segment_index, cx);
        if children.is_empty() {
            return div().h_0().into_any_element();
        }
        div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .children(
                children
                    .into_iter()
                    .map(|child| Self::render_child(child, window, cx)),
            )
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
