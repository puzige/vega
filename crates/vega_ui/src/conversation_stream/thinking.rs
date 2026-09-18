//! Bounded, memory-only reasoning content, separate from answer Markdown.
use super::*;

// Match the runtime's delta/turn/run byte ceilings, with an additional bound
// on retained block count. The view budget is not reset between messages.
pub(crate) const THINKING_DELTA_BYTES: usize = 64 * 1024;
pub(crate) const THINKING_BLOCK_BYTES: usize = 256 * 1024;
pub(crate) const THINKING_VIEW_BYTES: usize = 1024 * 1024;
pub(crate) const THINKING_VIEW_BLOCKS: usize = 256;

fn accepted_prefix_len(delta: &str, limit: usize) -> usize {
    let mut end = delta.len().min(THINKING_DELTA_BYTES).min(limit);
    while !delta.is_char_boundary(end) {
        end -= 1;
    }
    end
}

pub(crate) struct ThinkingBlock {
    pub(crate) text: String,
    pub(crate) expanded: bool,
    pub(crate) truncated: bool,
    focus: FocusHandle,
}

impl ThinkingBlock {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            text: String::new(),
            expanded: false,
            truncated: false,
            focus: cx.focus_handle(),
        }
    }
    pub(crate) fn append(&mut self, delta: &str, remaining: usize) -> usize {
        let end = accepted_prefix_len(
            delta,
            THINKING_BLOCK_BYTES
                .saturating_sub(self.text.len())
                .min(remaining),
        );
        self.text.push_str(&delta[..end]);
        self.truncated |= end < delta.len();
        end
    }
}

impl Render for ThinkingBlock {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .py_1()
            .debug_selector(|| "thinking-block".into())
            .child(
                div()
                    .id("thinking-toggle")
                    .track_focus(&self.focus)
                    .tab_stop(true)
                    .focus_visible(move |row| row.border_1().border_color(colors.accent))
                    .debug_selector(|| "thinking-toggle".into())
                    .aria_label("展开或收起思考过程")
                    .flex()
                    .items_center()
                    .gap_2()
                    .py_1()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(move |row| row.bg(colors.bg_hover))
                    .text_size(px(Typography::BODY))
                    .text_color(colors.text_secondary)
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            this.focus.focus(window, cx);
                            this.expanded = !this.expanded;
                            cx.notify();
                        }),
                    )
                    .on_key_down(cx.listener(|this, event: &gpui_kit::KeyDownEvent, _, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            this.expanded = !this.expanded;
                            cx.stop_propagation();
                            cx.notify();
                        }
                    }))
                    .child(crate::icons::icon(
                        if self.expanded {
                            crate::icons::Icon::ChevronDown
                        } else {
                            crate::icons::Icon::ChevronRight
                        },
                        colors.text_secondary,
                    ))
                    .child("思考过程"),
            )
            .when(self.expanded, |block| {
                block.child(
                    div()
                        .debug_selector(|| "thinking-content".into())
                        .w_full()
                        .min_w_0()
                        .text_size(px(Typography::BODY))
                        .text_color(colors.text_secondary)
                        .child(self.text.clone()),
                )
            })
            .when(self.truncated, |block| {
                block.child(
                    div()
                        .debug_selector(|| "thinking-truncated".into())
                        .text_size(px(Typography::METADATA))
                        .text_color(colors.text_tertiary)
                        .child("思考内容已达显示上限"),
                )
            })
    }
}

impl ConversationStream {
    pub(crate) fn append_thinking(
        &mut self,
        message_id: &str,
        delta: &str,
        cx: &mut Context<Self>,
    ) {
        if delta.is_empty()
            || !self
                .active_agent_message
                .as_ref()
                .is_some_and(|(active, _)| active == message_id)
        {
            return;
        }
        if self.active_thinking.is_none() {
            if accepted_prefix_len(
                delta,
                THINKING_VIEW_BYTES.saturating_sub(self.thinking_bytes),
            ) == 0
                || self.thinking_blocks >= THINKING_VIEW_BLOCKS
            {
                if let Some(card) = self.entries.iter().rev().find_map(|entry| match entry {
                    StreamEntry::Thinking { card } => Some(card.clone()),
                    _ => None,
                }) {
                    card.update(cx, |card, cx| {
                        if !card.truncated {
                            card.truncated = true;
                            cx.notify();
                        }
                    });
                }
                return;
            }
            self.close_active_segment_before_tool();
            let card = cx.new(ThinkingBlock::new);
            cx.observe(&card, |this, card, cx| {
                let index = this.entry_index_where(|entry| matches!(entry, StreamEntry::Thinking { card: owned } if owned == &card));
                this.invalidate_item(index);
                cx.notify();
            }).detach();
            let index = self.entries.len();
            self.entries
                .push(StreamEntry::Thinking { card: card.clone() });
            self.list_append(index);
            self.thinking_blocks += 1;
            self.active_thinking = Some(card);
        }
        if let Some(card) = &self.active_thinking {
            let remaining = THINKING_VIEW_BYTES.saturating_sub(self.thinking_bytes);
            self.thinking_bytes += card.update(cx, |card, cx| {
                let appended = card.append(delta, remaining);
                cx.notify();
                appended
            });
        }
    }
}
