//! Bounded, memory-only reasoning content, separate from answer Markdown.
use super::*;

#[path = "thinking_body.rs"]
mod body;

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

#[derive(Clone, Copy)]
pub(crate) struct ThinkingChanged {
    pub(crate) header: bool,
    pub(crate) stage: bool,
    pub(crate) geometry: bool,
}

impl gpui_kit::EventEmitter<ThinkingChanged> for ThinkingBlock {}

pub(crate) struct ThinkingBlock {
    pub(crate) text: String,
    pub(crate) shimmer_epoch: Option<Instant>,
    summary: String,
    summary_has_content: bool,
    thinking_has_content: bool,
    thinking_preview: Preview,
    summary_preview: Preview,
    body: body::ThinkingBody,
    source_copy: MessageCopy,
    source_revision: u64,
    ancestor_visible: bool,
    pub(crate) expanded: bool,
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
            "Thinking"
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
        self.summary_has_content || self.thinking_has_content
    }

    pub(crate) fn copy_selected_source(&mut self, cx: &mut Context<Self>) {
        if self.source_copy.has_selected_text() {
            self.source_copy.copy_selected_text(cx);
            cx.stop_propagation();
        }
    }

    pub(crate) fn render_stage_source(
        card: Entity<Self>,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let (focus, scroll) = {
            let state = card.read(cx);
            (state.focus.clone(), state.scroll.clone())
        };
        let content = Self::render_source(card.clone(), window, cx);
        div()
            .relative()
            .w_full()
            .min_w_0()
            .pt(px(Layout::PROCESS_ITEM_GAP))
            .track_focus(&focus)
            .on_action(move |_: &CopySelectedText, _, cx| {
                card.update(cx, ThinkingBlock::copy_selected_source);
            })
            .child(content)
            .child(crate::tool_card::disclosure_scrollbar(&scroll, cx))
            .into_any_element()
    }

    fn render_source(card: Entity<Self>, window: &mut Window, cx: &mut App) -> AnyElement {
        card.update(cx, |card, cx| card.render_body(window, cx))
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
            thinking_has_content: false,
            thinking_preview: Preview::default(),
            summary_preview: Preview::default(),
            body: body::ThinkingBody::default(),
            source_copy: MessageCopy::default(),
            source_revision: 0,
            ancestor_visible: true,
            expanded: false,
            truncated: false,
            focus: cx.focus_handle().tab_stop(true),
            scroll: ScrollHandle::new(),
        }
    }
    pub(crate) fn set_ancestor_visible(&mut self, visible: bool) {
        if self.ancestor_visible != visible {
            self.ancestor_visible = visible;
            self.body.visibility_changed();
        }
    }

    pub(crate) fn set_expanded(&mut self, expanded: bool, cx: &mut Context<Self>) {
        if self.expanded != expanded {
            self.expanded = expanded;
            self.body.visibility_changed();
            cx.emit(ThinkingChanged {
                header: false,
                stage: false,
                geometry: true,
            });
            cx.notify();
        }
    }

    pub(crate) fn set_stage_expanded(&mut self, expanded: bool, cx: &mut Context<Self>) {
        self.set_expanded(expanded, cx);
    }

    pub(crate) fn row_count(&self) -> usize {
        1 + usize::from(self.expanded) + usize::from(self.truncated)
    }

    pub(crate) fn estimated_source_height(&self, width: f32) -> f32 {
        let _ = width;
        self.body
            .height()
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
        let source_switched = summary && !self.summary_has_content;
        if summary {
            self.summary_has_content |= delta[..end].chars().any(|ch| !ch.is_whitespace());
            self.summary.push_str(&delta[..end]);
            self.summary_preview.append(&delta[..end]);
        } else {
            self.thinking_has_content |= delta[..end].chars().any(|ch| !ch.is_whitespace());
            self.text.push_str(&delta[..end]);
            self.thinking_preview.append(&delta[..end]);
        }
        if end > 0 {
            if summary && self.summary_has_content {
                if source_switched {
                    self.body.replace_source();
                }
                if self.source_copy.original_len() == self.summary.len() - end && !source_switched {
                    self.source_copy.append(&delta[..end]);
                } else {
                    self.source_copy.replace_text(&self.summary);
                }
                self.source_revision = self.source_revision.wrapping_add(1);
            } else if !summary && !self.summary_has_content {
                self.source_copy.append(&delta[..end]);
                self.source_revision = self.source_revision.wrapping_add(1);
            }
        }
        self.truncated |= end < delta.len();
        end
    }
}

impl Render for ThinkingBlock {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
                    .aria_label(format!("{}, expand or collapse thinking", title))
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
                        .child(self.render_body(window, cx))
                        .child(crate::tool_card::disclosure_scrollbar(&self.scroll, cx)),
                )
            })
            .when(self.truncated, |block| {
                block.child(
                    div()
                        .debug_selector(|| "thinking-truncated".into())
                        .text_size(px(Typography::METADATA))
                        .text_color(colors.text_tertiary)
                        .child("Thinking display limit reached"),
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
                let title = card.title().to_owned();
                let summary_title = card.summary_title().map(str::to_owned);
                let source_present = card.has_source_text();
                let truncated = card.truncated;
                let appended = card.append(delta, summary, remaining);
                let header_changed = title != card.title();
                let stage_changed = summary_title.as_deref() != card.summary_title()
                    || source_present != card.has_source_text()
                    || truncated != card.truncated;
                if header_changed || stage_changed {
                    cx.emit(ThinkingChanged {
                        header: header_changed,
                        stage: stage_changed,
                        geometry: truncated != card.truncated,
                    });
                }
                card.prepare_body(cx);
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
