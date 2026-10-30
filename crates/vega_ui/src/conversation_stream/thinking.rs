//! Bounded, memory-only reasoning content, separate from answer Markdown.
use super::*;

#[path = "thinking_body.rs"]
mod body;
#[path = "thinking_window.rs"]
mod window;

pub(crate) const THINKING_DELTA_BYTES: usize = 64 * 1024;
pub(crate) const THINKING_BLOCK_BYTES: usize = 256 * 1024;
pub(crate) const THINKING_VIEW_BYTES: usize = 1024 * 1024;
pub(crate) const THINKING_VIEW_BLOCKS: usize = 256;

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
    pub(crate) progress_finished: bool,
    summary: String,
    raw_window: window::SourceWindow,
    summary_window: window::SourceWindow,
    source_identity: u64,
    summary_has_content: bool,
    thinking_has_content: bool,
    thinking_preview: Preview,
    summary_preview: Preview,
    body: body::ThinkingBody,
    pub(super) source_copy: MessageCopy,
    source_revision: u64,
    ancestor_visible: bool,
    pub(crate) expanded: bool,
    pub(crate) truncated: bool,
    message_actions_enabled: bool,
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

    pub(super) fn contains_focus(&self, window: &Window, cx: &App) -> bool {
        self.focus.contains_focused(window, cx)
    }

    pub(crate) fn title(&self) -> &str {
        if !self.summary_preview.title.is_empty() {
            &self.summary_preview.title
        } else if !self.thinking_preview.title.is_empty() {
            &self.thinking_preview.title
        } else {
            vega_i18n::t("ui.thinking")
        }
    }

    pub(crate) fn visible_text(&self) -> &str {
        if self.summary_has_content {
            &self.summary
        } else {
            &self.text
        }
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
        let scope_id = format!("thinking-{}", card.entity_id());
        div()
            .relative()
            .w_full()
            .min_w_0()
            .pt(px(Layout::PROCESS_ITEM_GAP))
            .track_focus(&focus)
            .on_action(move |_: &CopySelectedText, _, cx| {
                card.update(cx, ThinkingBlock::copy_selected_source);
            })
            .child(
                crate::link_preview::MarkdownReadingScope::new(scope_id, focus.clone())
                    .track_scroll(scroll.clone())
                    .child(content),
            )
            .child(crate::tool_card::disclosure_scrollbar(&scroll, cx))
            .into_any_element()
    }

    fn render_source(card: Entity<Self>, window: &mut Window, cx: &mut App) -> AnyElement {
        card.update(cx, |card, cx| card.render_body(window, cx))
    }

    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            text: String::new(),
            shimmer_epoch: None,
            progress_finished: false,
            summary: String::new(),
            raw_window: window::SourceWindow::default(),
            summary_window: window::SourceWindow::default(),
            source_identity: 0,
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
            message_actions_enabled: true,
            focus: cx.focus_handle().tab_stop(true),
            scroll: ScrollHandle::new(),
        }
    }

    pub(crate) fn set_message_actions_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.message_actions_enabled != enabled {
            self.message_actions_enabled = enabled;
            cx.notify();
        }
    }
    pub(crate) fn set_ancestor_visible(&mut self, visible: bool) {
        if self.ancestor_visible != visible {
            self.ancestor_visible = visible;
            self.body.visibility_changed();
        }
    }

    pub(crate) fn set_expanded(&mut self, expanded: bool, cx: &mut Context<Self>) {
        let expanded = expanded && self.has_source_text();
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

    pub(crate) fn append(&mut self, delta: &str, summary: bool, _remaining: usize) -> usize {
        let was_summary = self.summary_has_content;
        let previous_start = self.visible_window().start;
        let previous_len = self.visible_text().len();
        let removed = if summary {
            if self.summary.len().saturating_add(delta.len())
                > THINKING_BLOCK_BYTES.saturating_sub(self.text.len())
            {
                self.raw_window.clear(&mut self.text);
                self.thinking_has_content = false;
                self.truncated = true;
            }
            self.summary_preview.append(delta);
            let removed = self.summary_window.append(
                &mut self.summary,
                delta,
                THINKING_BLOCK_BYTES - self.text.len(),
            );
            self.summary_has_content = if removed {
                self.summary.chars().any(|ch| !ch.is_whitespace())
            } else {
                self.summary_has_content || delta.chars().any(|ch| !ch.is_whitespace())
            };
            removed
        } else {
            self.thinking_preview.append(delta);
            let removed = self.raw_window.append(
                &mut self.text,
                delta,
                THINKING_BLOCK_BYTES - self.summary.len(),
            );
            self.thinking_has_content = if removed {
                self.text.chars().any(|ch| !ch.is_whitespace())
            } else {
                self.thinking_has_content || delta.chars().any(|ch| !ch.is_whitespace())
            };
            removed
        };
        self.truncated |= removed;
        if was_summary != self.summary_has_content {
            self.source_identity = self.source_identity.wrapping_add(1);
            self.body.replace_source();
            self.source_copy.replace_text(self.visible_text());
            self.source_revision = self.source_revision.wrapping_add(1);
        } else if previous_start != self.visible_window().start
            || previous_len != self.visible_text().len()
        {
            let window_changed = previous_start != self.visible_window().start;
            if window_changed {
                self.source_copy.replace_retained(self.visible_text());
                self.slide_body();
            } else {
                self.source_copy
                    .append_retained(&self.visible_text()[previous_len..]);
            }
            self.source_revision = self.source_revision.wrapping_add(1);
        }
        self.retained_bytes()
    }

    fn visible_window(&self) -> &window::SourceWindow {
        if self.summary_has_content {
            &self.summary_window
        } else {
            &self.raw_window
        }
    }

    fn retained_bytes(&self) -> usize {
        self.text.len() + self.summary.len()
    }

    fn evict(&mut self, cx: &mut Context<Self>) {
        self.text = String::new();
        self.summary = String::new();
        self.raw_window = window::SourceWindow::default();
        self.summary_window = window::SourceWindow::default();
        self.thinking_preview = Preview {
            title: std::mem::take(&mut self.thinking_preview.title),
            ..Default::default()
        };
        self.summary_preview = Preview {
            title: std::mem::take(&mut self.summary_preview.title),
            ..Default::default()
        };
        self.summary_has_content = false;
        self.thinking_has_content = false;
        self.source_identity = self.source_identity.wrapping_add(1);
        self.source_revision = self.source_revision.wrapping_add(1);
        self.body.evict();
        self.source_copy.clear_retained();
        self.scroll = ScrollHandle::new();
        self.expanded = false;
        self.truncated = true;
        cx.emit(ThinkingChanged {
            header: false,
            stage: true,
            geometry: true,
        });
        cx.notify();
    }
}

impl Render for ThinkingBlock {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        let title = self.title().to_owned();
        let can_expand = self.has_source_text();
        self.focus = self.focus.clone().tab_stop(can_expand);
        div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(Layout::PROCESS_ITEM_GAP))
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
                    .when(can_expand, |row| row.aria_expanded(self.expanded))
                    .tab_stop(can_expand)
                    .focus_visible(move |row| row.bg(colors.bg_hover))
                    .aria_label(if can_expand {
                        vega_i18n::t_args("ui.thinking_disclosure", &[("title", &title)])
                    } else {
                        title.clone()
                    })
                    .flex()
                    .items_center()
                    .gap(px(Layout::CONVERSATION_INLINE_GAP))
                    .h(px(Layout::CONVERSATION_TOOL_HEIGHT))
                    .line_height(gpui_kit::relative(Typography::CONVERSATION_LINE_HEIGHT))
                    .rounded(px(Layout::DISCLOSURE_RADIUS))
                    .when(can_expand, |row| row.cursor_pointer())
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
                    .child(div().min_w_0().flex_shrink(1.).truncate().child(
                        if let Some(epoch) = self.shimmer_epoch {
                            shimmer::text(title.clone(), epoch, window, cx)
                        } else {
                            div().child(title).into_any_element()
                        },
                    ))
                    .child(crate::disclosure::chevron(
                        format!("thinking-disclosure-{}", cx.entity_id()).into(),
                        &self.focus,
                        self.expanded,
                        self.has_source_text(),
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
                        .text_size(px(Typography::METADATA))
                        .text_color(colors.text_tertiary)
                        .child(vega_i18n::t("ui.earlier_thinking_omitted")),
                )
            })
    }
}

impl ConversationStream {
    pub(crate) fn sync_retained_thinking_actions(&mut self, cx: &mut Context<Self>) {
        let enabled = self.message_actions_enabled();
        for card in self
            .retained_thinking
            .iter()
            .filter_map(|card| card.upgrade())
        {
            card.update(cx, |card, cx| card.set_message_actions_enabled(enabled, cx));
        }
    }

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
            if self.active_response_start.is_none() || self.active_segment_has_text {
                self.close_active_segment_before_tool();
            }
            let Some((group, segment)) = self.ensure_live_run_activity_segment(message_id, cx)
            else {
                return;
            };
            let card = cx.new(ThinkingBlock::new);
            let actions_enabled = self.message_actions_enabled();
            card.update(cx, |card, cx| {
                card.set_message_actions_enabled(actions_enabled, cx)
            });
            group.update(cx, |group, cx| {
                group.append_thinking(segment, card.clone(), cx)
            });
            self.thinking_blocks += 1;
            self.retained_thinking.push_back(card.downgrade());
            self.active_thinking = Some(card);
        }
        if let Some(card) = self.active_thinking.clone() {
            let before = card.read(cx).retained_bytes();
            let (retained, summary_preview_changed) = card.update(cx, |card, cx| {
                let title = card.title().to_owned();
                let summary_title = card.summary_title().map(str::to_owned);
                let source_present = card.has_source_text();
                let truncated = card.truncated;
                let source_start = card.visible_window().start;
                let previous_height = card
                    .body
                    .height()
                    .min(Layout::DISCLOSURE_CONTENT_MAX_HEIGHT);
                let appended = card.append(delta, summary, THINKING_BLOCK_BYTES);
                let window_changed = source_start != card.visible_window().start
                    && card.expanded
                    && card.ancestor_visible
                    && card.body.painted.get();
                let header_changed = title != card.title() || window_changed;
                let summary_preview_changed = summary
                    && summary_title.as_deref() != card.summary_title()
                    && card.summary_title().is_some();
                let stage_changed = summary_title.as_deref() != card.summary_title()
                    || source_present != card.has_source_text()
                    || truncated != card.truncated;
                if header_changed || stage_changed {
                    cx.emit(ThinkingChanged {
                        header: header_changed,
                        stage: stage_changed,
                        geometry: truncated != card.truncated
                            || card.expanded
                                && (summary_title.is_some() != card.summary_title().is_some()
                                    || source_present != card.has_source_text()
                                    || previous_height
                                        != card
                                            .body
                                            .height()
                                            .min(Layout::DISCLOSURE_CONTENT_MAX_HEIGHT)),
                    });
                }
                if window_changed {
                    cx.notify();
                }
                card.prepare_body(cx);
                (appended, summary_preview_changed)
            });
            if !card.read(cx).progress_finished
                && let Some((_, group)) = &self.active_run_activity
            {
                group.update(cx, |group, cx| {
                    group.set_thinking(card.clone(), summary_preview_changed, cx)
                });
            }
            self.thinking_bytes = self.thinking_bytes.saturating_sub(before) + retained;
            while self.thinking_bytes > THINKING_VIEW_BYTES
                || self.thinking_blocks > THINKING_VIEW_BLOCKS
            {
                let Some(old) = self.retained_thinking.pop_front() else {
                    break;
                };
                if let Some(old) = old.upgrade() {
                    self.thinking_bytes = self
                        .thinking_bytes
                        .saturating_sub(old.read(cx).retained_bytes());
                    old.update(cx, ThinkingBlock::evict);
                }
                self.thinking_blocks = self.thinking_blocks.saturating_sub(1);
            }
        }
    }
}
