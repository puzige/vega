//! UI-only grouping for adjacent audited tool calls.

use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Context, Entity, MouseButton, MouseUpEvent, ScrollHandle, div, px,
};
use vega_theme::{Layout, ThemeColors, Typography, theme};

use crate::icons::Icon;
use crate::tool_card::{ToolActivityCategory, ToolActivityState, ToolCard};

/// Ordered, memory-only presentation state for one adjacent tool run.
///
/// The children remain the exact entities owned by the call-id map. The group
/// carries only UI expansion state and never retains a provider call id.
pub(crate) struct ToolActivityGroup {
    children: Vec<Entity<ToolCard>>,
    expanded: bool,
    scroll: ScrollHandle,
}

impl ToolActivityGroup {
    #[cfg(test)]
    pub(crate) fn scroll_handle(&self) -> ScrollHandle {
        self.scroll.clone()
    }

    pub(crate) fn new(first: Entity<ToolCard>, second: Entity<ToolCard>) -> Self {
        Self {
            children: vec![first, second],
            expanded: false,
            scroll: ScrollHandle::new(),
        }
    }

    pub(crate) fn from_children(children: Vec<Entity<ToolCard>>, expanded: bool) -> Self {
        debug_assert!(children.len() >= 2);
        Self {
            children,
            expanded,
            scroll: ScrollHandle::new(),
        }
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

    /// #151: the newest live activity unit is expanded; older units step down.
    pub(crate) fn set_expanded(&mut self, expanded: bool, cx: &mut Context<Self>) {
        if self.expanded != expanded {
            self.expanded = expanded;
            cx.notify();
        }
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

    pub(crate) fn estimated_height(&self, cx: &App) -> f32 {
        if !self.expanded || self.children.is_empty() {
            return Layout::CONVERSATION_TOOL_HEIGHT;
        }
        let mut height = 0.;
        for (index, card) in self.children.iter().enumerate() {
            if index > 0 {
                height += Layout::PROCESS_ITEM_GAP;
            }
            height += card.read(cx).estimated_height();
            if height >= Layout::TOOL_GROUP_MAX_HEIGHT {
                break;
            }
        }
        Layout::CONVERSATION_TOOL_HEIGHT
            + Layout::PROCESS_ITEM_GAP
            + height.min(Layout::TOOL_GROUP_MAX_HEIGHT)
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

    pub(crate) fn leading_icon(&self, cx: &App) -> Icon {
        self.children
            .first()
            .map(|card| card.read(cx).leading_icon())
            .unwrap_or_else(|| ToolActivityCategory::Other.icon())
    }

    pub(crate) fn leading_icon_color(&self, cx: &App, colors: &ThemeColors) -> gpui_kit::Rgba {
        self.children
            .first()
            .map(|card| card.read(cx).leading_icon_color(colors))
            .unwrap_or(colors.text_secondary)
    }

    pub(crate) fn render(
        group: Entity<Self>,
        window: &mut gpui_kit::Window,
        cx: &mut App,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let (expanded, children, child_count, summary, icon, icon_color) = {
            let group_ref = group.read(cx);
            (
                group_ref.expanded,
                group_ref.children.clone(),
                group_ref.len(),
                group_ref.aggregate_summary(cx),
                group_ref.leading_icon(cx),
                group_ref.leading_icon_color(cx, &colors),
            )
        };
        let scroll = group.read(cx).scroll.clone();
        let scroll_id = format!("tool-group-{}", group.entity_id());
        let toggle_group = group.clone();
        let keyboard_group = group.clone();
        let disclosure_group: gpui_kit::SharedString =
            format!("tool-group-disclosure-{}", group.entity_id()).into();
        let focus = crate::disclosure::focus(disclosure_group.clone(), window, cx);
        let mut rows = Vec::with_capacity(1 + child_count);
        rows.push(
            div()
                .id(("tool-activity-group-toggle", group.entity_id()))
                .group(disclosure_group.clone())
                .track_focus(&focus)
                .aria_expanded(expanded)
                .aria_label(format!(
                    "{summary}，{}工具组",
                    if expanded { "收起" } else { "展开" }
                ))
                .tab_stop(true)
                .focus_visible(move |row| row.bg(colors.bg_hover))
                .debug_selector(|| "tool-activity-group-toggle".to_string())
                .h(px(Layout::CONVERSATION_TOOL_HEIGHT))
                .text_size(px(Typography::CONVERSATION))
                .line_height(gpui_kit::relative(Typography::CONVERSATION_LINE_HEIGHT))
                .w_full()
                .min_w_0()
                .flex()
                .items_center()
                .gap(px(Layout::CONVERSATION_INLINE_GAP))
                .on_key_down(move |event, _, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        keyboard_group.update(cx, ToolActivityGroup::toggle_expanded);
                        cx.stop_propagation();
                    }
                })
                .cursor_pointer()
                .on_mouse_up(MouseButton::Left, move |_: &MouseUpEvent, _, cx| {
                    toggle_group.update(cx, ToolActivityGroup::toggle_expanded);
                })
                .child(
                    div()
                        .debug_selector(|| "tool-activity-group-icon".into())
                        .size(px(Layout::ICON_SIZE))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(crate::icons::sized_icon(
                            icon,
                            icon_color,
                            Layout::ICON_SIZE,
                        )),
                )
                .child(
                    div()
                        .debug_selector(|| "tool-activity-group-label".into())
                        .min_w_0()
                        .flex_shrink(1.)
                        .truncate()
                        .text_size(px(Typography::CONVERSATION))
                        .text_color(colors.conversation_tool)
                        .child(summary),
                )
                .child(crate::disclosure::inline_chevron(
                    disclosure_group,
                    "tool-activity-group-chevron".into(),
                    &focus,
                    expanded,
                    true,
                    colors,
                    window,
                ))
                .into_any_element(),
        );
        if expanded {
            rows.push(
                div()
                    .relative()
                    .w_full()
                    .min_w_0()
                    .child(
                        div()
                            .id(gpui_kit::SharedString::from(scroll_id))
                            .debug_selector(|| "tool-activity-group-content".into())
                            .w_full()
                            .min_w_0()
                            .max_h(px(Layout::TOOL_GROUP_MAX_HEIGHT))
                            .overflow_x_hidden()
                            .overflow_y_scroll()
                            .track_scroll(&scroll)
                            .on_scroll_wheel(crate::tool_card::contain_disclosure_scroll(&scroll))
                            .flex()
                            .flex_col()
                            .gap(px(Layout::PROCESS_ITEM_GAP))
                            .children(children.into_iter().enumerate().map(|(index, card)| {
                                ToolCard::render(
                                    card,
                                    format!("tool-activity-child-{index}"),
                                    true,
                                    window,
                                    cx,
                                )
                            })),
                    )
                    .child(crate::tool_card::disclosure_scrollbar(&scroll, cx))
                    .into_any_element(),
            );
        }
        div()
            .debug_selector(|| "tool-activity-group".to_string())
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(Layout::PROCESS_ITEM_GAP))
            .children(rows)
            .into_any_element()
    }
}
