//! Bounded, memory-only reasoning content, separate from answer Markdown.
use super::*;
use crate::interaction::InteractionExt;

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

#[derive(Default)]
struct Preview {
    line: String,
    title: String,
    heading: bool,
    continued: bool,
    fence: Option<char>,
}
impl Preview {
    fn append(&mut self, delta: &str) {
        for ch in delta.chars() {
            if ch == '\n' {
                self.update();
                let trimmed = self.line.trim_start();
                let marker = if !self.continued && trimmed.starts_with("```") {
                    Some('`')
                } else if !self.continued && trimmed.starts_with("~~~") {
                    Some('~')
                } else {
                    None
                };
                if let Some(marker) = marker {
                    if self.fence == Some(marker) {
                        self.fence = None;
                    } else if self.fence.is_none() {
                        self.fence = Some(marker);
                    }
                }
                self.line.clear();
                self.continued = false;
            } else {
                self.line.push(ch);
                if self.line.len() > 1024 {
                    self.update();
                    let mut cut = self.line.len() - 768;
                    while !self.line.is_char_boundary(cut) {
                        cut += 1;
                    }
                    self.line.drain(..cut);
                    self.continued = true;
                }
            }
        }
        self.update();
    }

    fn update(&mut self) {
        let line = self.line.trim();
        if self.fence.is_some() || line.starts_with("```") || line.starts_with("~~~") {
            return;
        }
        let hashes = line.bytes().take_while(|ch| *ch == b'#').count();
        let atx = !self.continued
            && (1..=6).contains(&hashes)
            && line
                .as_bytes()
                .get(hashes)
                .is_some_and(u8::is_ascii_whitespace);
        let bold = !self.continued
            && line.len() > 4
            && ((line.starts_with("**") && line.ends_with("**"))
                || (line.starts_with("__") && line.ends_with("__")));
        let is_heading = atx || bold;
        if self.heading && !is_heading {
            return;
        }
        let line = if atx {
            line[hashes..].trim().trim_end_matches('#').trim()
        } else if bold {
            &line[2..line.len() - 2]
        } else {
            line
        };
        let readable: String = line
            .chars()
            .filter(|ch| !ch.is_control() && *ch != '`')
            .collect();
        let readable = readable.trim().trim_matches('*').trim_matches('_').trim();
        if readable.is_empty() || readable.chars().all(|ch| matches!(ch, '#' | '~' | '_')) {
            return;
        }
        let chars: Vec<_> = readable.chars().collect();
        self.title = if chars.len() <= 96 {
            readable.to_owned()
        } else if is_heading {
            chars[..95]
                .iter()
                .copied()
                .chain(std::iter::once('…'))
                .collect()
        } else {
            std::iter::once('…')
                .chain(chars[chars.len() - 95..].iter().copied())
                .collect()
        };
        self.heading |= is_heading;
    }
}

pub(crate) struct ThinkingBlock {
    pub(crate) text: String,
    pub(crate) shimmer_epoch: Option<Instant>,
    summary: String,
    summary_has_content: bool,
    thinking_preview: Preview,
    summary_preview: Preview,
    pub(crate) expanded: bool,
    expanded_rendered: bool,
    pub(crate) truncated: bool,
    focus: FocusHandle,
    scroll: ScrollHandle,
}

impl ThinkingBlock {
    pub(crate) fn summary_title(&self) -> Option<&str> {
        (!self.summary_preview.title.is_empty()).then_some(self.summary_preview.title.as_str())
    }

    pub(crate) fn title(&self) -> &str {
        if !self.summary_preview.title.is_empty() {
            &self.summary_preview.title
        } else if !self.thinking_preview.title.is_empty() {
            &self.thinking_preview.title
        } else {
            "思考过程"
        }
    }

    pub(crate) fn visible_text(&self) -> &str {
        if self.summary_has_content {
            &self.summary
        } else {
            &self.text
        }
    }

    #[cfg(test)]
    pub(crate) fn scroll_handle(&self) -> ScrollHandle {
        self.scroll.clone()
    }

    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            text: String::new(),
            shimmer_epoch: None,
            summary: String::new(),
            summary_has_content: false,
            thinking_preview: Preview::default(),
            summary_preview: Preview::default(),
            expanded: false,
            expanded_rendered: false,
            truncated: false,
            focus: cx.focus_handle().tab_stop(true),
            scroll: ScrollHandle::new(),
        }
    }
    pub(crate) fn set_expanded(&mut self, expanded: bool, cx: &mut Context<Self>) {
        if self.expanded != expanded {
            self.expanded = expanded;
            self.expanded_rendered = false;
            cx.notify();
        }
    }

    pub(crate) fn row_count(&self) -> usize {
        1 + usize::from(self.expanded) + usize::from(self.truncated)
    }

    pub(crate) fn estimated_height(&self, width: f32) -> f32 {
        let chars_per_line = ((width - 2. * Layout::DISCLOSURE_PADDING_X).max(1.)
            / (Typography::MESSAGE * 0.56))
            .max(1.) as usize;
        let content_height = if self.expanded {
            let max_lines = (Layout::DISCLOSURE_CONTENT_MAX_HEIGHT
                / (Typography::MESSAGE * Typography::MESSAGE_LINE_HEIGHT))
                .ceil() as usize;
            let mut completed_lines = 0;
            let mut line_width: usize = 0;
            for ch in self.visible_text().chars() {
                if ch == '\n' {
                    completed_lines += line_width.div_ceil(chars_per_line).max(1);
                    line_width = 0;
                } else {
                    line_width += display_width(ch.encode_utf8(&mut [0; 4]));
                }
                if completed_lines + line_width.div_ceil(chars_per_line) >= max_lines {
                    completed_lines = max_lines;
                    line_width = 0;
                    break;
                }
            }
            let lines = (completed_lines + line_width.div_ceil(chars_per_line)).max(1);
            Layout::PROCESS_ITEM_GAP
                + (lines as f32 * Typography::MESSAGE * Typography::MESSAGE_LINE_HEIGHT)
                    .min(Layout::DISCLOSURE_CONTENT_MAX_HEIGHT)
        } else {
            0.
        };
        Layout::DISCLOSURE_HEADER_HEIGHT
            + content_height
            + if self.truncated {
                Layout::PROCESS_ITEM_GAP + Typography::METADATA * Typography::BODY_LINE_HEIGHT
            } else {
                0.
            }
    }

    pub(crate) fn append(&mut self, delta: &str, summary: bool, remaining: usize) -> usize {
        let end = accepted_prefix_len(
            delta,
            THINKING_BLOCK_BYTES
                .saturating_sub(self.text.len() + self.summary.len())
                .min(remaining),
        );
        if summary {
            self.summary_has_content |= delta[..end].chars().any(|ch| !ch.is_whitespace());
            self.summary.push_str(&delta[..end]);
            self.summary_preview.append(&delta[..end]);
        } else {
            self.text.push_str(&delta[..end]);
            self.thinking_preview.append(&delta[..end]);
        }
        if self.expanded
            && self.expanded_rendered
            && self.scroll.offset().y + self.scroll.max_offset().y <= px(1.)
        {
            self.scroll
                .set_offset(gpui_kit::point(px(0.), px(-f32::MAX)));
        }
        self.truncated |= end < delta.len();
        end
    }
}

impl Render for ThinkingBlock {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.expanded_rendered = self.expanded;
        let colors = theme(cx).colors;
        div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(Layout::PROCESS_ITEM_GAP))
            .debug_selector(|| "thinking-block".into())
            .child(
                div()
                    .id("thinking-toggle")
                    .track_focus(&self.focus)
                    .tab_stop(true)
                    .border_1()
                    .border_color(gpui_kit::transparent_black())
                    .focus_visible(move |row| row.border_color(colors.accent))
                    .debug_selector(|| "thinking-toggle".into())
                    .aria_label(format!(
                        "{}，展开或收起思考过程",
                        if self.shimmer_epoch.is_some() {
                            self.summary_title().unwrap_or("Thinking")
                        } else {
                            self.title()
                        }
                    ))
                    .flex()
                    .items_center()
                    .gap_2()
                    .h(px(Layout::DISCLOSURE_HEADER_HEIGHT))
                    .line_height(gpui_kit::relative(Typography::BODY_LINE_HEIGHT))
                    .px(px(Layout::DISCLOSURE_PADDING_X))
                    .rounded(px(Layout::DISCLOSURE_RADIUS))
                    .cursor_pointer()
                    .interactive(colors, true, false)
                    .text_size(px(Typography::BODY))
                    .text_color(colors.text_secondary)
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            this.focus.focus(window, cx);
                            this.expanded = !this.expanded;
                            this.expanded_rendered = false;
                            cx.notify();
                        }),
                    )
                    .on_key_down(cx.listener(|this, event: &gpui_kit::KeyDownEvent, _, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            this.expanded = !this.expanded;
                            this.expanded_rendered = false;
                            cx.stop_propagation();
                            cx.notify();
                        }
                    }))
                    .child(
                        div()
                            .debug_selector(|| "thinking-icon".into())
                            .size(px(Layout::ICON_SIZE))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(crate::icons::icon(
                                if self.expanded {
                                    crate::icons::Icon::ChevronDown
                                } else {
                                    crate::icons::Icon::ChevronRight
                                },
                                colors.text_secondary,
                            )),
                    )
                    .child(
                        div()
                            .debug_selector(|| "thinking-title".into())
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .child(if let Some(epoch) = self.shimmer_epoch {
                                shimmer::text(
                                    self.summary_title().unwrap_or("Thinking").to_owned(),
                                    epoch,
                                    window,
                                    cx,
                                )
                            } else {
                                div().child(self.title().to_owned()).into_any_element()
                            }),
                    ),
            )
            .when(self.expanded, |block| {
                block.child(
                    div()
                        .relative()
                        .w_full()
                        .min_w_0()
                        .child(
                            div()
                                .id("thinking-content")
                                .debug_selector(|| "thinking-content".into())
                                .max_h(px(Layout::DISCLOSURE_CONTENT_MAX_HEIGHT))
                                .overflow_x_hidden()
                                .overflow_y_scroll()
                                .track_scroll(&self.scroll)
                                .on_scroll_wheel(crate::tool_card::contain_disclosure_scroll(
                                    &self.scroll,
                                ))
                                .w_full()
                                .min_w_0()
                                .pl(px(Layout::DISCLOSURE_PADDING_X))
                                .pr(px(Layout::DISCLOSURE_SCROLLBAR_GUTTER))
                                .text_size(px(Typography::MESSAGE))
                                .line_height(gpui_kit::relative(Typography::MESSAGE_LINE_HEIGHT))
                                .text_color(colors.text_secondary)
                                .child(gpui_kit::base::SelectableText::new(
                                    "thinking-text",
                                    self.visible_text().to_owned(),
                                )),
                        )
                        .child(crate::tool_card::disclosure_scrollbar(&self.scroll, cx)),
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
        self.append_reasoning(message_id, delta, false, cx);
    }

    pub(crate) fn append_reasoning(
        &mut self,
        message_id: &str,
        delta: &str,
        summary: bool,
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
                if let Some((active_id, group)) = self.active_run_activity.clone()
                    && active_id == message_id
                    && let Some(card) =
                        group
                            .read(cx)
                            .children()
                            .into_iter()
                            .rev()
                            .find_map(|child| match child {
                                RunActivityChild::Thinking(card) => Some(card),
                                _ => None,
                            })
                {
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
            let Some((group, segment)) = self.ensure_live_run_activity_segment(message_id, cx)
            else {
                return;
            };
            let card = cx.new(ThinkingBlock::new);
            group.update(cx, |group, cx| {
                group.append_thinking(segment, card.clone(), cx)
            });
            self.thinking_blocks += 1;
            card.update(cx, |card, cx| card.set_expanded(false, cx));
            self.active_thinking = Some(card);
        }
        if let Some(card) = &self.active_thinking {
            if let Some((_, group)) = &self.active_run_activity {
                group.update(cx, |group, cx| group.set_thinking(card.clone(), cx));
            }
            let remaining = THINKING_VIEW_BYTES.saturating_sub(self.thinking_bytes);
            self.thinking_bytes += card.update(cx, |card, cx| {
                let appended = card.append(delta, summary, remaining);
                cx.notify();
                appended
            });
        }
    }
}

#[cfg(test)]
mod preview_tests {
    use super::Preview;

    #[test]
    fn i71_preview_incremental_markdown_and_identifiers() {
        let mut preview = Preview::default();
        for chunk in ["`foo_", "bar` needs inspection"] {
            preview.append(chunk);
        }
        assert_eq!(preview.title, "foo_bar needs inspection");
        preview.append("\n10*7 + 9*11 = foo_bar");
        assert_eq!(preview.title, "10*7 + 9*11 = foo_bar");
        preview.append("\n#hashtag is prose");
        assert_eq!(preview.title, "#hashtag is prose");
        preview.append("\n## ");
        assert_eq!(preview.title, "#hashtag is prose");
        preview.append("真实标题\nbody\n\nmore body");
        assert_eq!(preview.title, "真实标题");
        preview.append("\n```\n## fake\n```\n**下一");
        assert_eq!(preview.title, "真实标题");
        preview.append("阶段**\nbody");
        assert_eq!(preview.title, "下一阶段");
        assert!(preview.line.len() <= 1024);
    }
}
