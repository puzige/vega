//! UI-only grouping for adjacent audited tool calls.

use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, Context, Entity, MouseButton, MouseUpEvent, div, px};
use vega_theme::{Typography, theme};

use crate::conversation_stream::ROW_HEIGHT;
use crate::tool_card::{ToolActivityCategory, ToolActivityState, ToolCard};

/// Ordered, memory-only presentation state for one adjacent tool run.
///
/// The children remain the exact entities owned by the call-id map. The group
/// carries only UI expansion state and never retains a provider call id.
pub(crate) struct ToolActivityGroup {
    children: Vec<Entity<ToolCard>>,
    expanded: bool,
}

impl ToolActivityGroup {
    pub(crate) fn new(first: Entity<ToolCard>, second: Entity<ToolCard>) -> Self {
        Self {
            children: vec![first, second],
            expanded: false,
        }
    }

    pub(crate) fn from_children(children: Vec<Entity<ToolCard>>, expanded: bool) -> Self {
        debug_assert!(children.len() >= 2);
        Self { children, expanded }
    }

    pub(crate) fn append(&mut self, card: Entity<ToolCard>, cx: &mut Context<Self>) {
        self.children.push(card);
        cx.notify();
    }

    pub(crate) fn extend(
        &mut self,
        cards: impl IntoIterator<Item = Entity<ToolCard>>,
        expanded: bool,
        cx: &mut Context<Self>,
    ) {
        self.children.extend(cards);
        self.expanded |= expanded;
        cx.notify();
    }

    pub(crate) fn contains(&self, card: &Entity<ToolCard>) -> bool {
        self.children.iter().any(|child| child == card)
    }

    pub(crate) fn children(&self) -> Vec<Entity<ToolCard>> {
        self.children.clone()
    }

    pub(crate) fn expanded(&self) -> bool {
        self.expanded
    }

    pub(crate) fn toggle_expanded(&mut self, cx: &mut Context<Self>) {
        self.expanded = !self.expanded;
        cx.notify();
    }

    pub(crate) fn len(&self) -> usize {
        self.children.len()
    }

    pub(crate) fn row_count(&self, cx: &App) -> usize {
        1 + if self.expanded {
            self.children
                .iter()
                .map(|card| card.read(cx).row_count())
                .sum()
        } else {
            0
        }
    }

    #[cfg(test)]
    pub(crate) fn visible_text(&self, cx: &App) -> String {
        let mut text = self.aggregate_summary(cx);
        if self.expanded {
            for child in &self.children {
                text.push('\n');
                text.push_str(&child.read(cx).visible_text());
            }
        }
        text
    }

    fn aggregate_state(&self, cx: &App) -> ToolActivityState {
        self.children
            .iter()
            .map(|card| card.read(cx).activity_state())
            .max_by_key(|state| state.priority())
            .unwrap_or(ToolActivityState::Failed)
    }

    fn categories(&self, cx: &App) -> Vec<ToolActivityCategory> {
        let mut categories = Vec::new();
        for card in &self.children {
            let category = card.read(cx).activity_category();
            if !categories.contains(&category) {
                categories.push(category);
            }
        }
        categories
    }

    pub(crate) fn aggregate_summary(&self, cx: &App) -> String {
        let categories = self.categories(cx);
        let actions = categories
            .iter()
            .map(|category| category.action_phrase())
            .collect::<Vec<_>>()
            .join("、");
        match self.aggregate_state(cx) {
            ToolActivityState::Success => format!("已{actions}"),
            ToolActivityState::Active => format!("正在处理：{actions}"),
            ToolActivityState::Rejected => format!("存在已拒绝的调用：{actions}"),
            ToolActivityState::Cancelled => format!("存在已取消的调用：{actions}"),
            ToolActivityState::Failed => format!("存在失败的调用：{actions}"),
        }
    }

    pub(crate) fn render(group: Entity<Self>, cx: &App) -> AnyElement {
        let colors = theme(cx).colors;
        let (expanded, children, child_count, summary, state) = {
            let group_ref = group.read(cx);
            (
                group_ref.expanded,
                group_ref.children.clone(),
                group_ref.len(),
                group_ref.aggregate_summary(cx),
                group_ref.aggregate_state(cx),
            )
        };
        let icon = state.icon(
            children
                .first()
                .map(|card| card.read(cx).activity_category())
                .unwrap_or(ToolActivityCategory::Other),
        );
        let icon_color = state.color(&colors);
        let toggle_group = group.clone();
        let mut rows = Vec::with_capacity(1 + child_count);
        rows.push(
            div()
                .debug_selector(|| "tool-activity-group-toggle".to_string())
                .h(px(ROW_HEIGHT))
                .w_full()
                .min_w_0()
                .flex()
                .items_center()
                .gap_2()
                .cursor_pointer()
                .on_mouse_up(MouseButton::Left, move |_: &MouseUpEvent, _, cx| {
                    toggle_group.update(cx, ToolActivityGroup::toggle_expanded);
                })
                .child(crate::icons::icon(icon, icon_color))
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .truncate()
                        .text_size(px(Typography::BODY))
                        .text_color(colors.text_secondary)
                        .child(summary),
                )
                .child(
                    div()
                        .debug_selector(|| "tool-activity-group-chevron".to_string())
                        .child(crate::icons::icon(
                            if expanded {
                                crate::icons::Icon::ChevronDown
                            } else {
                                crate::icons::Icon::ChevronRight
                            },
                            colors.text_tertiary,
                        )),
                )
                .into_any_element(),
        );
        if expanded {
            rows.extend(children.into_iter().enumerate().map(|(index, card)| {
                ToolCard::render(card, format!("tool-activity-child-{index}"), true, cx)
            }));
        }
        div()
            .debug_selector(|| "tool-activity-group".to_string())
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .children(rows)
            .into_any_element()
    }
}
