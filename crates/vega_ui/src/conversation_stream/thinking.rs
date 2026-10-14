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

#[derive(Default)]
struct Preview {
    line: String,
    title: String,
    continued: bool,
    continued_heading: bool,
    long_bold_marker: Option<&'static str>,
    long_bold_start: String,
    fence: Option<char>,
    continued_fence: Option<char>,
}
impl Preview {
    fn append(&mut self, delta: &str) -> bool {
        let mut updated = false;
        for ch in delta.chars() {
            if ch == '\n' {
                updated |= self.update();
                let trimmed = self.line.trim_start();
                let marker = if let Some(marker) = self.continued_fence {
                    Some(marker)
                } else if !self.continued && trimmed.starts_with("```") {
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
                self.continued_heading = false;
                self.long_bold_marker = None;
                self.long_bold_start.clear();
                self.continued_fence = None;
            } else {
                self.line.push(ch);
                if self.line.len() > 1024 {
                    if !self.continued {
                        let line = self.line.trim();
                        self.continued_fence = if line.starts_with("```") {
                            Some('`')
                        } else if line.starts_with("~~~") {
                            Some('~')
                        } else {
                            None
                        };
                        let hashes = line.bytes().take_while(|ch| *ch == b'#').count();
                        self.continued_heading = (1..=6).contains(&hashes)
                            && line
                                .as_bytes()
                                .get(hashes)
                                .is_some_and(u8::is_ascii_whitespace);
                        if !self.continued_heading {
                            self.long_bold_marker = ["**", "__"].into_iter().find(|marker| {
                                line.starts_with(*marker) && !line[2..].contains(*marker)
                            });
                            if self.long_bold_marker.is_some() {
                                self.long_bold_start = line[2..]
                                    .chars()
                                    .filter(|ch| !ch.is_control() && *ch != '`')
                                    .take(95)
                                    .collect();
                            }
                        }
                    }
                    updated |= self.update();
                    let mut cut = self.line.len() - 768;
                    while !self.line.is_char_boundary(cut) {
                        cut += 1;
                    }
                    self.line.drain(..cut);
                    self.continued = true;
                }
            }
        }
        updated | self.update()
    }

    fn update(&mut self) -> bool {
        let line = self.line.trim();
        if self.fence.is_some()
            || self.continued_fence.is_some()
            || line.starts_with("```")
            || line.starts_with("~~~")
        {
            return false;
        }
        if let Some(marker) = self.long_bold_marker {
            if line.ends_with(marker) && !self.long_bold_start.trim().is_empty() {
                let title = format!("{}…", self.long_bold_start);
                let updated = self.title != title;
                self.title = title;
                self.continued_heading = true;
                return updated;
            }
            return false;
        }
        if self.continued && self.continued_heading {
            return false;
        }
        if ["**", "__"]
            .into_iter()
            .any(|marker| line.starts_with(marker) && !line[2..].contains(marker))
        {
            return false;
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
            return false;
        }
        let chars: Vec<_> = readable.chars().collect();
        let title = if chars.len() <= 96 {
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
        let updated = self.title != title;
        self.title = title;
        updated
    }
}

pub(crate) struct ThinkingBlock {
    pub(crate) text: String,
    pub(crate) shimmer_epoch: Option<Instant>,
    summary: String,
    summary_has_content: bool,
    thinking_preview: Preview,
    summary_preview: Preview,
    source_stream: MarkdownStream,
    source_model: StreamModel,
    source_copy: MessageCopy,
    source_counters: StreamCounters,
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

    pub(crate) fn header_focused(&self, window: &Window) -> bool {
        self.focus.is_focused(window)
    }

    #[cfg(test)]
    pub(crate) fn test_focus(&self) -> FocusHandle {
        self.focus.clone()
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

    pub(crate) fn source_text(&self) -> String {
        self.visible_text().to_owned()
    }

    pub(crate) fn has_source_text(&self) -> bool {
        self.source_copy.has_text()
    }

    pub(crate) fn has_meaningful_source_text(&self) -> bool {
        self.visible_text()
            .chars()
            .any(|character| !character.is_whitespace())
    }

    pub(crate) fn render_stage_source(
        card: Entity<Self>,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let (model, copy, focus, scroll) = {
            let state = card.read(cx);
            (
                StreamModel {
                    committed_lines: state.source_model.committed_lines.clone(),
                    pending_lines: state.source_model.pending_lines.clone(),
                    ..StreamModel::default()
                },
                state.source_copy.clone(),
                state.focus.clone(),
                state.scroll.clone(),
            )
        };
        let colors = theme(cx).colors;
        let content = render_rows::selectable_markdown_content(
            &model, &copy, &focus, &colors, true, window, cx,
        );
        div()
            .relative()
            .w_full()
            .min_w_0()
            .pt(px(Layout::PROCESS_ITEM_GAP))
            .on_action(move |_: &CopySelectedText, _, cx| {
                card.update(cx, |card, cx| {
                    if card.source_copy.has_selected_text() {
                        card.source_copy.copy_selected_text(cx);
                        cx.stop_propagation();
                    }
                });
            })
            .child(
                div()
                    .id("thinking-content")
                    .debug_selector(|| "thinking-content".into())
                    .max_h(px(Layout::DISCLOSURE_CONTENT_MAX_HEIGHT))
                    .overflow_x_hidden()
                    .overflow_y_scroll()
                    .track_scroll(&scroll)
                    .on_scroll_wheel(crate::tool_card::contain_disclosure_scroll(&scroll))
                    .w_full()
                    .min_w_0()
                    .pl(px(Layout::DISCLOSURE_PADDING_X))
                    .pr(px(Layout::DISCLOSURE_SCROLLBAR_GUTTER))
                    .text_size(px(Typography::CONVERSATION))
                    .line_height(gpui_kit::relative(Typography::CONVERSATION_LINE_HEIGHT))
                    .text_color(colors.text_secondary)
                    .child(content),
            )
            .child(crate::tool_card::disclosure_scrollbar(&scroll, cx))
            .into_any_element()
    }

    #[cfg(test)]
    pub(crate) fn scroll_handle(&self) -> ScrollHandle {
        self.scroll.clone()
    }

    #[cfg(test)]
    pub(crate) fn test_source_copy(&self) -> MessageCopy {
        self.source_copy.clone()
    }

    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            text: String::new(),
            shimmer_epoch: None,
            summary: String::new(),
            summary_has_content: false,
            thinking_preview: Preview::default(),
            summary_preview: Preview::default(),
            source_stream: MarkdownStream::new(),
            source_model: StreamModel::default(),
            source_copy: MessageCopy::default(),
            source_counters: StreamCounters::default(),
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

    pub(crate) fn set_stage_expanded(&mut self, expanded: bool, cx: &mut Context<Self>) {
        self.set_expanded(expanded, cx);
        self.expanded_rendered = expanded;
    }

    pub(crate) fn row_count(&self) -> usize {
        1 + usize::from(self.expanded) + usize::from(self.truncated)
    }

    pub(crate) fn estimated_source_height(&self, width: f32) -> f32 {
        message_anchors::estimate_markdown_height(
            &self.source_model,
            width - Layout::DISCLOSURE_PADDING_X - Layout::DISCLOSURE_SCROLLBAR_GUTTER,
        )
        .min(Layout::DISCLOSURE_CONTENT_MAX_HEIGHT)
    }

    pub(crate) fn estimated_height(&self, width: f32) -> f32 {
        Layout::CONVERSATION_TOOL_HEIGHT
            + if self.expanded && self.source_copy.has_text() {
                Layout::PROCESS_ITEM_GAP + self.estimated_source_height(width)
            } else {
                0.
            }
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
        let source = self.source_text();
        let previous = self.source_copy.original_text();
        if source != previous {
            if let Some(delta) = source.strip_prefix(&previous) {
                self.source_stream.append(delta);
            } else {
                self.source_stream = MarkdownStream::new();
                self.source_model = StreamModel::default();
                self.source_stream.append(&source);
            }
            self.source_copy.replace_text(&source);
            self.source_model
                .sync(&self.source_stream.snapshot(), &self.source_counters);
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
        let title = self.title().to_owned();
        div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(Layout::PROCESS_ITEM_GAP))
            .debug_selector(|| "thinking-block".into())
            .on_action(cx.listener(|this, _: &CopySelectedText, _, cx| {
                if this.source_copy.has_selected_text() {
                    this.source_copy.copy_selected_text(cx);
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .id("thinking-toggle")
                    .group(gpui_kit::SharedString::from(format!(
                        "thinking-disclosure-{}",
                        cx.entity_id()
                    )))
                    .track_focus(&self.focus)
                    .aria_expanded(self.expanded)
                    .tab_stop(true)
                    .focus_visible(move |row| row.bg(colors.bg_hover))
                    .debug_selector(|| "thinking-toggle".into())
                    .aria_label(format!("{}，展开或收起思考过程", title))
                    .flex()
                    .items_center()
                    .gap(px(Layout::CONVERSATION_INLINE_GAP))
                    .h(px(Layout::CONVERSATION_TOOL_HEIGHT))
                    .line_height(gpui_kit::relative(Typography::CONVERSATION_LINE_HEIGHT))
                    .rounded(px(Layout::DISCLOSURE_RADIUS))
                    .cursor_pointer()
                    .text_size(px(Typography::CONVERSATION))
                    .text_color(colors.conversation_tool)
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            this.focus.focus(window, cx);
                            this.set_expanded(!this.expanded, cx);
                        }),
                    )
                    .on_key_down(cx.listener(|this, event: &gpui_kit::KeyDownEvent, _, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            this.set_expanded(!this.expanded, cx);
                            cx.stop_propagation();
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
                                crate::icons::Icon::Bot,
                                colors.conversation_tool,
                            )),
                    )
                    .child(
                        div()
                            .debug_selector(|| "thinking-title".into())
                            .min_w_0()
                            .flex_shrink(1.)
                            .truncate()
                            .child(if let Some(epoch) = self.shimmer_epoch {
                                shimmer::text(title.clone(), epoch, window, cx)
                            } else {
                                div().child(title).into_any_element()
                            }),
                    )
                    .child(crate::disclosure::chevron(
                        format!("thinking-disclosure-{}", cx.entity_id()).into(),
                        "thinking-chevron".into(),
                        &self.focus,
                        self.expanded,
                        true,
                        colors,
                        window,
                    )),
            )
            .when(self.expanded && self.source_copy.has_text(), |block| {
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
                                .text_size(px(Typography::CONVERSATION))
                                .line_height(gpui_kit::relative(
                                    Typography::CONVERSATION_LINE_HEIGHT,
                                ))
                                .text_color(colors.text_secondary)
                                .child(render_rows::selectable_markdown_content(
                                    &self.source_model,
                                    &self.source_copy,
                                    &self.focus,
                                    &colors,
                                    true,
                                    window,
                                    cx,
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
            if self.active_response_start.is_none() || self.active_segment_has_text {
                self.close_active_segment_before_tool();
            }
            let Some((group, segment)) = self.ensure_live_run_activity_segment(message_id, cx)
            else {
                return;
            };
            let card = cx.new(ThinkingBlock::new);
            group.update(cx, |group, cx| {
                group.append_thinking(segment, card.clone(), cx)
            });
            self.thinking_blocks += 1;
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
        assert_eq!(preview.title, "more body");
        preview.append("\n```\n## fake\n```\n**下一");
        assert_eq!(preview.title, "more body");
        preview.append("阶段**\nbody");
        assert_eq!(preview.title, "body");
        assert!(preview.line.len() <= 1024);
    }

    #[test]
    fn i343_heading_yields_to_later_prose_but_not_markers_or_fenced_code() {
        let mut preview = Preview::default();
        preview.append("## 阿里云服务：");
        assert_eq!(preview.title, "阿里云服务：");
        preview.append("\nBest: check keeper DB");
        assert_eq!(preview.title, "Best: check keeper DB");
        preview.append(" and compare accounts");
        assert_eq!(preview.title, "Best: check keeper DB and compare accounts");
        preview.append("\n\n### ");
        assert_eq!(preview.title, "Best: check keeper DB and compare accounts");
        preview.append("\n```rust\n## fake heading\nlet token = 42;\n```\n**下个");
        assert_eq!(preview.title, "Best: check keeper DB and compare accounts");
        preview.append("阶段**");
        assert_eq!(preview.title, "下个阶段");
        preview.append(&format!("\n```{}\n## fenced\n```\n", "x".repeat(1100)));
        assert_eq!(preview.title, "下个阶段");
        preview.append("围栏之后的正文");
        assert_eq!(preview.title, "围栏之后的正文");
    }

    #[test]
    fn i343_long_heading_keeps_start_and_long_prose_advances_tail() {
        let mut heading = Preview::default();
        heading.append(&format!("## 开头{}", "中".repeat(500)));
        assert!(heading.title.starts_with("开头"));
        assert_eq!(heading.title.chars().count(), 96);
        heading.append("\n新的普通段落");
        assert_eq!(heading.title, "新的普通段落");

        let mut bold = Preview::default();
        let long_bold = format!("**起点{}**", "文".repeat(500));
        bold.append(&long_bold[..long_bold.len() - 2]);
        assert_eq!(bold.title, "");
        bold.append("**");
        assert!(bold.title.starts_with("起点"));
        assert_eq!(bold.title.chars().count(), 96);

        let mut prose = Preview::default();
        prose.append(&"😀汉字".repeat(500));
        prose.append("🦀最后");
        assert!(prose.title.ends_with("🦀最后"));
        assert_eq!(prose.title.chars().count(), 96);
    }
}
