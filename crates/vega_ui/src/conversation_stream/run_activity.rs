use super::*;
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
    Artifact(Entity<ArtifactCard>),
}

struct RunActivitySegment {
    children: Vec<RunActivityChild>,
    scroll: ScrollHandle,
}

pub(crate) struct RunActivityGroup {
    status: RunActivityStatus,
    execution_duration_ms: Option<u64>,
    expanded: bool,
    terminal: bool,
    segments: Vec<RunActivitySegment>,
}

impl RunActivityGroup {
    pub(crate) fn new(
        status: RunActivityStatus,
        execution_duration_ms: Option<u64>,
        expanded: bool,
    ) -> Self {
        Self {
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
    pub(crate) fn test_scroll_handle(&self) -> ScrollHandle {
        self.segments
            .first()
            .map(|segment| segment.scroll.clone())
            .unwrap_or_default()
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
            scroll: ScrollHandle::new(),
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
        self.collapse_latest_activity(cx);
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
        if !self
            .segments
            .get(segment)
            .and_then(|segment| segment.children.last())
            .is_some_and(|child| {
                matches!(
                    child,
                    RunActivityChild::Tool(_) | RunActivityChild::ToolGroup(_)
                )
            })
        {
            self.collapse_latest_activity(cx);
        }
        let Some(segment_state) = self.segments.get_mut(segment) else {
            return;
        };
        match segment_state.children.last() {
            Some(RunActivityChild::Tool(first)) => {
                cx.observe(&card, |_, _, cx| cx.notify()).detach();
                first.update(cx, |first, cx| first.set_expanded(false, cx));
                let group = cx.new(|_| ToolActivityGroup::new(first.clone(), card));
                Self::observe_tool_group(&group, cx);
                group.update(cx, |group, cx| group.set_expanded(true, cx));
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
                card.update(cx, |card, cx| card.set_expanded(true, cx));
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

    pub(crate) fn collapse_latest_activity(&mut self, cx: &mut Context<Self>) {
        match self
            .segments
            .iter()
            .rev()
            .find_map(|segment| segment.children.last())
        {
            Some(RunActivityChild::Thinking(card)) => {
                card.update(cx, |card, cx| card.set_expanded(false, cx));
            }
            Some(RunActivityChild::Tool(card)) => {
                card.update(cx, |card, cx| card.set_expanded(false, cx));
            }
            Some(RunActivityChild::ToolGroup(group)) => {
                group.update(cx, |group, cx| group.set_expanded(false, cx));
            }
            Some(RunActivityChild::Artifact(_)) | None => {}
        }
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
        if !self.expanded {
            return 0;
        }
        self.segments
            .get(segment)
            .map(|segment| {
                segment
                    .children
                    .iter()
                    .map(|child| Self::child_row_count(child, cx))
                    .sum()
            })
            .unwrap_or_default()
    }

    fn child_row_count(child: &RunActivityChild, cx: &App) -> usize {
        match child {
            RunActivityChild::Thinking(card) => card.read(cx).row_count(),
            RunActivityChild::Tool(card) => card.read(cx).row_count(),
            RunActivityChild::ToolGroup(group) => group.read(cx).row_count(cx),
            RunActivityChild::Artifact(card) => card.read(cx).row_count(),
        }
    }

    pub(crate) fn segment_scroll_handle(&self, segment: usize) -> Option<ScrollHandle> {
        self.segments
            .get(segment)
            .map(|segment| segment.scroll.clone())
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
        self.status = status;
        self.execution_duration_ms = execution_duration_ms;
        self.terminal = true;
        self.expanded = false;
        cx.notify();
    }

    pub(crate) fn toggle_expanded(&mut self, cx: &mut Context<Self>) {
        self.expanded = !self.expanded;
        cx.notify();
    }

    fn label(&self) -> String {
        let status = match self.status {
            RunActivityStatus::Running => "正在执行".to_string(),
            RunActivityStatus::Completed => "已完成".to_string(),
            RunActivityStatus::Failed => "已失败".to_string(),
            RunActivityStatus::Interrupted => "已停止".to_string(),
        };
        self.execution_duration_ms
            .map(|duration| format!("{status} · 用时 {}", format_run_duration(duration)))
            .unwrap_or(status)
    }

    pub(crate) fn render(group: Entity<Self>, _window: &mut Window, cx: &mut App) -> AnyElement {
        let colors = theme(cx).colors;
        let group_ref = group.read(cx);
        let expanded = group_ref.expanded;
        let label = group_ref.label();
        let status = group_ref.status;
        let children = group_ref.children();
        let toggle = group.clone();
        let can_expand = !children.is_empty();
        let header = div()
            .debug_selector(|| "run-activity-toggle".to_string())
            .w_full()
            .h(px(ROW_HEIGHT))
            .min_w_0()
            .flex()
            .items_center()
            .gap_2()
            .cursor_pointer()
            .when(can_expand, |row| {
                row.on_mouse_up(MouseButton::Left, move |_: &MouseUpEvent, _, cx| {
                    toggle.update(cx, RunActivityGroup::toggle_expanded);
                })
            })
            .child(crate::icons::icon(
                if status == RunActivityStatus::Running {
                    crate::icons::Icon::Refresh
                } else {
                    crate::icons::Icon::Summary
                },
                if matches!(status, RunActivityStatus::Failed) {
                    colors.danger
                } else {
                    colors.text_secondary
                },
            ))
            .child(
                div()
                    .debug_selector(|| "run-activity-status".to_string())
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .text_size(px(Typography::METADATA))
                    .text_color(colors.text_secondary)
                    .child(label),
            )
            .when(can_expand, |row| {
                row.child(crate::icons::icon(
                    if expanded {
                        crate::icons::Icon::ChevronDown
                    } else {
                        crate::icons::Icon::ChevronRight
                    },
                    colors.text_tertiary,
                ))
            });
        div()
            .debug_selector(|| "run-activity-group".to_string())
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .child(header)
            .into_any_element()
    }

    pub(crate) fn render_segment(
        group: Entity<Self>,
        segment_index: usize,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let group_ref = group.read(cx);
        if !group_ref.expanded {
            return div().h_0().into_any_element();
        }
        let children = group_ref.segment_children(segment_index);
        let scroll = group_ref
            .segment_scroll_handle(segment_index)
            .unwrap_or_default();
        div()
            .id(format!(
                "run-activity-{}-{segment_index}",
                group.entity_id()
            ))
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
            .children(children.into_iter().map(|child| match child {
                RunActivityChild::Thinking(card) => {
                    div().w_full().min_w_0().child(card).into_any_element()
                }
                RunActivityChild::Tool(card) => {
                    ToolCard::render(card, "tool-activity-single-row".to_string(), true, cx)
                }
                RunActivityChild::ToolGroup(group) => ToolActivityGroup::render(group, cx),
                RunActivityChild::Artifact(card) => {
                    let row_count = card.read(cx).row_count();
                    let mut rows = Vec::with_capacity(row_count);
                    for row in 0..row_count {
                        rows.push(ArtifactCard::render_row(card.clone(), row, window, cx));
                    }
                    div()
                        .w_full()
                        .flex_shrink_0()
                        .pt(px(4.0))
                        .pb(px(8.0))
                        .flex()
                        .flex_col()
                        .children(rows)
                        .into_any_element()
                }
            }))
            .into_any_element()
    }
}

pub(crate) fn format_run_duration(duration_ms: u64) -> String {
    let seconds = (duration_ms / 1000 + u64::from(!duration_ms.is_multiple_of(1000))).max(1);
    if seconds < 60 {
        return format!("{seconds} 秒");
    }
    if seconds < 3600 {
        return format!("{} 分 {:02} 秒", seconds / 60, seconds % 60);
    }
    format!("{} 小时 {} 分", seconds / 3600, (seconds % 3600) / 60)
}
