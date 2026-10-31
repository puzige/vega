use super::*;

pub(crate) const MESSAGE_ANCHOR_PREVIEW_LIMIT: usize = 96;
const MESSAGE_ANCHOR_MEASUREMENT_SCAN_LIMIT: usize = 256;
pub(crate) const MESSAGE_ANCHOR_MEASURED_CACHE_LIMIT: usize = 512;
const MESSAGE_ANCHOR_CACHE_RADIUS: usize =
    (MESSAGE_ANCHOR_MEASURED_CACHE_LIMIT - MESSAGE_ANCHOR_MEASUREMENT_SCAN_LIMIT) / 2;
const MESSAGE_ANCHOR_PREVIEW_INPUT_LIMIT: usize = MESSAGE_ANCHOR_PREVIEW_LIMIT * 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MessageAnchorKind {
    User,
    Assistant,
    Tool,
    Plan,
    Summary,
}

impl MessageAnchorKind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::User => vega_i18n::t("ui.user_message"),
            Self::Assistant => vega_i18n::t("ui.assistant_message"),
            Self::Tool => vega_i18n::t("ui.tool_activity"),
            Self::Plan => vega_i18n::t("ui.plan_message"),
            Self::Summary => vega_i18n::t("ui.task_result"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MessageAnchorProjection {
    pub(crate) entry_index: usize,
    pub(crate) message_id: String,
    pub(crate) kind: MessageAnchorKind,
    pub(crate) preview: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PositionedMessageAnchor {
    pub(crate) entry_index: usize,
    pub(crate) message_id: String,
    pub(crate) kind: MessageAnchorKind,
    pub(crate) preview: String,
    pub(crate) fraction: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MessageAnchorGeometry {
    pub(crate) anchors: Vec<PositionedMessageAnchor>,
    pub(crate) entry_heights: Vec<f32>,
    pub(crate) current_fraction: f32,
    pub(crate) total_height: f32,
    pub(crate) viewport_height: f32,
}

impl MessageAnchorGeometry {
    pub(crate) fn should_show(&self) -> bool {
        self.anchors.len() >= 2
            && self.viewport_height > 0.0
            && self.total_height > self.viewport_height + 1.0
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct MessageAnchorLayout {
    top_index: usize,
    top_offset: Pixels,
    viewport: gpui_kit::Bounds<Pixels>,
    foreground_height: Pixels,
    item_count: usize,
    at_tail: bool,
    measured_heights: Vec<(usize, f32)>,
}

impl MessageAnchorLayout {
    fn capture(list: &gpui_kit::ListState, foreground_height: Pixels) -> Self {
        let viewport = list.viewport_bounds();
        let viewport_height = f32::from(viewport.size.height - foreground_height);
        if !f32::from(viewport.size.width).is_finite()
            || viewport.size.width <= px(0.)
            || !viewport_height.is_finite()
            || viewport_height <= 0.
            || foreground_height < px(0.)
        {
            return Self::default();
        }
        let top = list.logical_scroll_top();
        let item_count = list.item_count();
        let end = top
            .item_ix
            .saturating_add(MESSAGE_ANCHOR_MEASUREMENT_SCAN_LIMIT)
            .min(item_count);
        let mut measured_heights = Vec::new();
        for index in top.item_ix..end {
            let Some(bounds) = list.bounds_for_item(index) else {
                if index > top.item_ix {
                    break;
                }
                continue;
            };
            let height = f32::from(bounds.size.height);
            if bounds.size.width > px(0.) && height.is_finite() && height > 0. {
                measured_heights.push((index, height));
            }
        }
        let reading_bottom = viewport.bottom() - foreground_height;
        let at_tail = top.item_ix < item_count
            && (top.item_ix..item_count).all(|index| {
                list.bounds_for_item(index).is_some_and(|bounds| {
                    bounds.size.width > px(0.)
                        && bounds.size.height >= px(0.)
                        && bounds.bottom() <= reading_bottom
                })
            });
        Self {
            top_index: top.item_ix,
            top_offset: top.offset_in_item,
            viewport,
            foreground_height,
            item_count,
            at_tail,
            measured_heights,
        }
    }

    fn viewport_height(&self) -> f32 {
        f32::from(self.viewport.size.height - self.foreground_height).max(0.)
    }
}

impl ConversationStream {
    pub(crate) fn message_anchor_projections(&self) -> Vec<MessageAnchorProjection> {
        let mut projections = Vec::new();
        let mut seen = HashSet::<&str>::new();
        for (entry_index, (entry, identity)) in
            self.entries.iter().zip(&self.entry_identities).enumerate()
        {
            let Some(message_id) = identity.message_id.as_deref() else {
                continue;
            };
            let (kind, preview) = match entry {
                StreamEntry::User { lines, copy, .. } if !user_message_is_visible(copy, lines) => {
                    continue;
                }
                StreamEntry::User { .. } => (
                    MessageAnchorKind::User,
                    self.text_cache
                        .borrow_mut()
                        .preview(entry_index, &identity.key, entry)
                        .unwrap_or_default(),
                ),
                StreamEntry::UserImages { .. } => {
                    let text = self.entries.get(entry_index + 1).zip(self.entry_identities.get(entry_index + 1))
                        .filter(|(entry, next)| next.message_id.as_deref() == Some(message_id)
                            && matches!(entry, StreamEntry::User { copy, lines, .. } if user_message_is_visible(copy, lines)));
                    let preview = text
                        .and_then(|(entry, next)| {
                            self.text_cache
                                .borrow_mut()
                                .preview(entry_index + 1, &next.key, entry)
                        })
                        .unwrap_or_else(|| vega_i18n::t("ui.image_message").to_string());
                    (MessageAnchorKind::User, preview)
                }
                StreamEntry::Assistant { .. } => (
                    MessageAnchorKind::Assistant,
                    self.text_cache
                        .borrow_mut()
                        .preview(entry_index, &identity.key, entry)
                        .unwrap_or_default(),
                ),
                StreamEntry::Tool { .. }
                | StreamEntry::ToolGroup { .. }
                | StreamEntry::ArtifactGroup { .. } => (
                    MessageAnchorKind::Tool,
                    vega_i18n::t("ui.tool_activity").to_string(),
                ),
                StreamEntry::Plan { .. } => (
                    MessageAnchorKind::Plan,
                    vega_i18n::t("ui.plan_message").to_string(),
                ),
                StreamEntry::Summary { .. } => (
                    MessageAnchorKind::Summary,
                    vega_i18n::t("ui.task_result").to_string(),
                ),
                StreamEntry::Subagents { .. }
                | StreamEntry::ContextCompaction { .. }
                | StreamEntry::RunActivity { .. }
                | StreamEntry::RunActivitySegment { .. }
                | StreamEntry::Permission { .. }
                | StreamEntry::SkillActivation { .. }
                | StreamEntry::Screenshot { .. } => continue,
            };
            if !seen.insert(message_id) {
                continue;
            }
            projections.push(MessageAnchorProjection {
                entry_index,
                message_id: message_id.to_string(),
                kind,
                preview,
            });
        }
        projections
    }

    pub(crate) fn capture_visible_entry_heights(&mut self) {
        self.ensure_entry_identities();
        let start = self.list.logical_scroll_top().item_ix;
        let end = start
            .saturating_add(MESSAGE_ANCHOR_MEASUREMENT_SCAN_LIMIT)
            .min(self.entries.len());
        for index in start..end {
            let Some(bounds) = self.list.bounds_for_item(index) else {
                if index > start {
                    break;
                }
                continue;
            };
            if bounds.size.width <= px(0.) {
                continue;
            }
            let height = f32::from(bounds.size.height);
            if height <= 0.0 {
                continue;
            }
            if let Some(identity) = self.entry_identities.get(index) {
                self.measured_entry_heights
                    .insert(identity.key.clone(), height);
            }
        }
        let cache_start = start.saturating_sub(MESSAGE_ANCHOR_CACHE_RADIUS);
        let cache_end = end
            .saturating_add(MESSAGE_ANCHOR_CACHE_RADIUS)
            .min(self.entries.len());
        let retained_identities = self.entry_identities[cache_start..cache_end]
            .iter()
            .map(|identity| identity.key.as_str())
            .collect::<HashSet<_>>();
        self.measured_entry_heights
            .retain(|identity, _| retained_identities.contains(identity.as_str()));
    }

    pub(crate) fn message_anchor_geometry(
        &self,
        stage_top_insets: &HashMap<usize, f32>,
        cx: &App,
    ) -> MessageAnchorGeometry {
        let notices = &self.run_notices;
        let mcp_notices = &self.mcp_notices;
        let notice_width = self
            .workspace_width
            .map(|width| width - 2. * Layout::CONVERSATION_PADDING)
            .unwrap_or(Layout::CONVERSATION_MAX_WIDTH)
            .clamp(1., Layout::CONVERSATION_MAX_WIDTH);
        let entry_heights = self
            .entries
            .iter()
            .zip(&self.entry_identities)
            .enumerate()
            .map(|(index, (entry, identity))| {
                if self.process_entry_hidden(index, cx) {
                    return 0.;
                }
                if self.compaction_failure_is_represented(index) {
                    return notices.get(&index).map(|notice| notice.estimated_height(notice_width)).unwrap_or(0.)
                        + mcp_notices.get(&index).map(|notice| notice.estimated_height(notice_width)).unwrap_or(0.);
                }
                self.measured_entry_heights
                    .get(&identity.key)
                    .copied()
                    .unwrap_or_else(|| match entry {
                        StreamEntry::RunActivity { group } => self.estimated_run_height(group, cx) + stage_top_insets.get(&index).copied().unwrap_or(0.),
                        StreamEntry::RunActivitySegment { group, segment } => group.read(cx).estimated_segment_height_with_top(*segment, notice_width, stage_top_insets.get(&index).copied().unwrap_or(Layout::CONVERSATION_STAGE_TOP), cx),
                        StreamEntry::Tool { .. } | StreamEntry::ToolGroup { .. } | StreamEntry::ContextCompaction { .. } => self.estimated_entry_height(index, cx, identity.message_id.is_some()) + stage_top_insets.get(&index).copied().unwrap_or(0.),
                        _ => self.estimated_entry_height(index, cx, identity.message_id.is_some()) + if self.action_copy_source_for_entry(index).is_some() { Layout::MESSAGE_COPY_FOOTER } else { 0. },
                    } + notices.get(&index).map(|notice| notice.estimated_height(notice_width)).unwrap_or(0.)
                        + mcp_notices.get(&index).map(|notice| notice.estimated_height(notice_width)).unwrap_or(0.))
            })
            .collect::<Vec<_>>();
        let total_height = entry_heights.iter().sum::<f32>();
        let projections = self.message_anchor_projections();
        let mut prefix = 0.0;
        let mut projection_index = 0;
        let mut anchors = Vec::with_capacity(projections.len());
        for (entry_index, height) in entry_heights.iter().copied().enumerate() {
            if let Some(projection) = projections.get(projection_index)
                && projection.entry_index == entry_index
            {
                anchors.push(PositionedMessageAnchor {
                    entry_index,
                    message_id: projection.message_id.clone(),
                    kind: projection.kind,
                    preview: projection.preview.clone(),
                    fraction: if total_height > 0.0 {
                        ((prefix + height / 2.0) / total_height).clamp(0.0, 1.0)
                    } else {
                        0.0
                    },
                });
                projection_index += 1;
            }
            prefix += height;
        }
        let layout = &self.message_anchor_layout;
        let current_prefix = entry_heights
            .iter()
            .take(layout.top_index.min(entry_heights.len()))
            .sum::<f32>();
        let current_offset = if layout.top_index < entry_heights.len() {
            f32::from(layout.top_offset).clamp(0.0, entry_heights[layout.top_index])
        } else {
            0.0
        };
        let viewport_height = if layout.foreground_height == self.foreground_height
            && layout.item_count == self.entries.len()
        {
            layout.viewport_height()
        } else {
            0.0
        };
        MessageAnchorGeometry {
            anchors,
            entry_heights,
            current_fraction: if total_height > 0.0 {
                ((current_prefix + current_offset) / total_height).clamp(0.0, 1.0)
            } else {
                0.0
            },
            total_height,
            viewport_height,
        }
    }

    pub(crate) fn message_anchor_rail_has_space(&self) -> bool {
        self.workspace_width.is_some_and(|width| {
            width >= Layout::CONVERSATION_MAX_WIDTH + 2. * Layout::CONVERSATION_PADDING
        })
    }

    pub(crate) fn render_message_anchor_rail(
        &mut self,
        stage_top_insets: &HashMap<usize, f32>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (AnyElement, Option<AnyElement>, bool) {
        let list = self.list.clone();
        let foreground_height = self.foreground_height;
        let stream = cx.entity().downgrade();
        let observer = gpui_kit::canvas(
            move |_, _, cx| {
                let layout = MessageAnchorLayout::capture(&list, foreground_height);
                cx.defer(move |cx| {
                    let _ = stream.update(cx, |stream, cx| {
                        if stream.message_anchor_layout != layout {
                            stream.message_anchor_layout = layout;
                            cx.notify();
                        }
                    });
                });
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size(px(0.));
        self.capture_visible_entry_heights();
        let geometry = self.message_anchor_geometry(stage_top_insets, cx);
        let shown = self.message_anchor_rail_has_space() && geometry.should_show();
        self.message_anchor_rail_last_shown = shown;
        if !shown {
            self.message_anchor_rail_hovered = false;
            self.message_anchor_hovered = None;
            self.message_anchor_preview_hovered = None;
            self.message_anchor_keyboard_id = None;
            if self.message_anchor_focus.is_focused(window) {
                window.blur(cx);
            }
            return (observer.into_any_element(), None, false);
        }
        let anchors = geometry
            .anchors
            .iter()
            .map(|anchor| (anchor.message_id.clone(), anchor.fraction))
            .collect::<Vec<_>>();
        let current_id = if self.message_anchor_layout.at_tail {
            geometry.anchors.last()
        } else {
            geometry
                .anchors
                .iter()
                .rev()
                .find(|anchor| anchor.entry_index <= self.message_anchor_layout.top_index)
                .or_else(|| geometry.anchors.first())
        }
        .map(|anchor| anchor.message_id.clone());
        let keyboard_focused =
            self.message_anchor_focus.is_focused(window) && window.last_input_was_keyboard();
        let selected_id = self
            .message_anchor_preview_hovered
            .clone()
            .or_else(|| self.message_anchor_hovered.clone())
            .or_else(|| {
                keyboard_focused
                    .then(|| {
                        self.message_anchor_keyboard_id
                            .clone()
                            .or_else(|| current_id.clone())
                    })
                    .flatten()
            });
        let selected_index = selected_id
            .as_ref()
            .and_then(|id| anchors.iter().position(|(candidate, _)| candidate == id));
        let current_index = current_id
            .as_ref()
            .and_then(|id| anchors.iter().position(|(candidate, _)| candidate == id))
            .unwrap_or(0);
        let count = anchors.len().min(Layout::MESSAGE_ANCHOR_VISIBLE_COUNT);
        let max_start = anchors.len().saturating_sub(count);
        if !self.message_anchor_rail_hovered && self.message_anchor_preview_hovered.is_none() {
            let target = if keyboard_focused {
                selected_index.unwrap_or(current_index)
            } else {
                current_index
            };
            self.message_anchor_window_start = target.saturating_sub(count / 2).min(max_start);
        }
        self.message_anchor_window_start = self.message_anchor_window_start.min(max_start);
        let start = self.message_anchor_window_start;
        let rail_left = Layout::MESSAGE_ANCHOR_LEFT_INSET;
        let rail_height = count as f32 * Layout::CONVERSATION_MARKER_PITCH;
        let colors = theme(cx).colors;
        let selected = selected_index.and_then(|index| geometry.anchors.get(index));
        let preview = selected.map(|anchor| {
            let offset = selected_index
                .unwrap_or(start)
                .saturating_sub(start)
                .min(count - 1) as f32
                * Layout::CONVERSATION_MARKER_PITCH;
            let available_width = self
                .workspace_width
                .unwrap_or_else(|| f32::from(window.viewport_size().width));
            let preview_width = Layout::MESSAGE_ANCHOR_PREVIEW_WIDTH
                .min((available_width - Layout::CONVERSATION_PADDING * 2.).max(0.));
            let content = render_message_anchor_preview(
                anchor.message_id.clone(),
                anchor_preview_label(anchor),
                preview_width,
                colors,
                cx,
            );
            div()
                .absolute()
                .left(px(rail_left + Layout::CONVERSATION_NAVIGATION_GUTTER + 4.))
                .top_0()
                .bottom(self.foreground_height)
                .w(px(preview_width))
                .flex()
                .flex_col()
                .child(
                    div()
                        .h(gpui_kit::relative(0.5))
                        .min_h_0()
                        .mt(px(offset - rail_height / 2.)),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .child(gpui_kit::deferred(content).with_priority(2)),
                )
                .into_any_element()
        });
        let default_id = current_id.clone();
        let key_anchors = anchors.clone();
        let rail = div()
            .id("message-anchor-rail")
            .aria_label(vega_i18n::t(
                "ui.message_navigation_use_arrow_keys_to_select_enter",
            ))
            .tab_stop(true)
            .track_focus(&self.message_anchor_focus)
            .absolute()
            .left(px(rail_left))
            .top(gpui_kit::relative(0.5))
            .mt(px(-(rail_height + f32::from(self.foreground_height)) / 2.))
            .w(px(Layout::CONVERSATION_NAVIGATION_GUTTER))
            .h(px(rail_height))
            .flex()
            .flex_col()
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.message_anchor_rail_hovered = *hovered;
                if !hovered && this.message_anchor_preview_hovered.is_none() {
                    this.message_anchor_hovered = None;
                }
                cx.notify();
            }))
            .on_scroll_wheel(
                cx.listener(move |this, event: &gpui_kit::ScrollWheelEvent, _, cx| {
                    let delta = f32::from(
                        event
                            .delta
                            .pixel_delta(px(Layout::CONVERSATION_MARKER_PITCH))
                            .y,
                    );
                    if delta != 0. {
                        let step = (delta.abs() / Layout::CONVERSATION_MARKER_PITCH)
                            .ceil()
                            .max(1.) as usize;
                        this.message_anchor_window_start = if delta < 0. {
                            this.message_anchor_window_start
                                .saturating_add(step)
                                .min(max_start)
                        } else {
                            this.message_anchor_window_start.saturating_sub(step)
                        };
                        this.message_anchor_hovered = None;
                        this.message_anchor_preview_hovered = None;
                        cx.notify();
                    }
                    cx.stop_propagation();
                }),
            )
            .on_key_down(
                cx.listener(move |this, event: &gpui_kit::KeyDownEvent, window, cx| {
                    let target = this
                        .message_anchor_keyboard_id
                        .clone()
                        .or_else(|| default_id.clone());
                    let index = target
                        .as_ref()
                        .and_then(|id| {
                            key_anchors
                                .iter()
                                .position(|(candidate, _)| candidate == id)
                        })
                        .unwrap_or(0);
                    match event.keystroke.key.as_str() {
                        "up" | "down" => {
                            let next = if event.keystroke.key == "up" {
                                index.saturating_sub(1)
                            } else {
                                (index + 1).min(key_anchors.len() - 1)
                            };
                            this.message_anchor_keyboard_id = Some(key_anchors[next].0.clone());
                            this.message_anchor_hovered = None;
                            this.message_anchor_preview_hovered = None;
                            this.message_anchor_window_start =
                                next.saturating_sub(count / 2).min(max_start);
                        }
                        "enter" | "space" => {
                            if let Some(id) = target {
                                this.request_message_location(&id, cx);
                            }
                        }
                        "escape" => {
                            this.message_anchor_keyboard_id = None;
                            this.message_anchor_hovered = None;
                            this.message_anchor_preview_hovered = None;
                            window.blur(cx);
                        }
                        _ => return,
                    }
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .children(
                geometry
                    .anchors
                    .iter()
                    .skip(start)
                    .take(count)
                    .map(|anchor| {
                        let id = anchor.message_id.clone();
                        let hover_id = id.clone();
                        let click_id = id.clone();
                        let selected = selected_id.as_deref() == Some(id.as_str())
                            || current_id.as_deref() == Some(id.as_str());
                        div()
                            .id(gpui_kit::SharedString::from(format!("message-anchor-{id}")))
                            .aria_label(anchor_preview_label(anchor))
                            .w_full()
                            .h(px(Layout::CONVERSATION_MARKER_PITCH))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .on_mouse_move(cx.listener(move |this, _, _, cx| {
                                if this.message_anchor_hovered.as_deref() != Some(hover_id.as_str())
                                {
                                    this.message_anchor_hovered = Some(hover_id.clone());
                                    cx.notify();
                                }
                            }))
                            .on_mouse_down(
                                gpui_kit::MouseButton::Left,
                                cx.listener(|this, _, window, cx| {
                                    this.message_anchor_focus.focus(window, cx);
                                    cx.stop_propagation();
                                }),
                            )
                            .on_mouse_up(
                                gpui_kit::MouseButton::Left,
                                cx.listener(move |this, _, _, cx| {
                                    this.message_anchor_keyboard_id = Some(click_id.clone());
                                    this.message_anchor_hovered = Some(click_id.clone());
                                    this.request_message_location(&click_id, cx);
                                    cx.stop_propagation();
                                }),
                            )
                            .child(
                                div()
                                    .w(px(if selected {
                                        Layout::MESSAGE_ANCHOR_ACTIVE_SIZE
                                    } else {
                                        Layout::CONVERSATION_MARKER_WIDTH
                                    }))
                                    .h(px(Layout::MESSAGE_ANCHOR_TRACK_WIDTH))
                                    .bg(if selected {
                                        colors.text_secondary
                                    } else {
                                        colors.conversation_marker
                                    }),
                            )
                    }),
            )
            .child(observer)
            .into_any_element();
        (rail, preview, false)
    }
}

fn anchor_preview_label(anchor: &PositionedMessageAnchor) -> String {
    format!("{} · {}", anchor.kind.label(), anchor.preview)
}

fn render_message_anchor_preview(
    message_id: String,
    label: String,
    width: f32,
    colors: ThemeColors,
    cx: &mut Context<ConversationStream>,
) -> AnyElement {
    div()
        .id("message-anchor-preview")
        .aria_label(label.clone())
        .w(px(width))
        .max_h(px(Layout::MESSAGE_ANCHOR_PREVIEW_HEIGHT))
        .py_2()
        .flex()
        .items_center()
        .px_2()
        .rounded_md()
        .border_1()
        .border_color(colors.border_subtle)
        .bg(colors.bg_elevated)
        .shadow_sm()
        .text_size(px(Typography::METADATA))
        .text_color(colors.text_primary)
        .child(div().min_w_0().line_clamp(3).child(label))
        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
            if *hovered {
                if this.message_anchor_preview_hovered.as_deref() != Some(message_id.as_str()) {
                    this.message_anchor_preview_hovered = Some(message_id.clone());
                    cx.notify();
                }
            } else if this.message_anchor_preview_hovered.as_deref() == Some(message_id.as_str()) {
                this.message_anchor_preview_hovered = None;
                if this.message_anchor_hovered.as_deref() == Some(message_id.as_str()) {
                    this.message_anchor_hovered = None;
                }
                cx.notify();
            }
        }))
        .on_scroll_wheel(
            cx.listener(|this, event: &gpui_kit::ScrollWheelEvent, _, cx| {
                let delta = event.delta.pixel_delta(px(
                    Typography::CONVERSATION_LINE_HEIGHT * Typography::CONVERSATION
                ));
                this.list.scroll_by(-delta.y);
                cx.stop_propagation();
                cx.notify();
            }),
        )
        .into_any_element()
}

pub(crate) fn sanitize_anchor_preview(text: &str) -> String {
    let words = text.split_whitespace().collect::<Vec<_>>();
    let mut safe = Vec::with_capacity(words.len());
    let mut words = words.into_iter();
    while let Some(word) = words.next() {
        if word
            .trim_matches(|ch: char| !ch.is_alphanumeric())
            .eq_ignore_ascii_case("bearer")
        {
            safe.push(vega_i18n::t("ui.credentials_hidden"));
            words.next();
        } else if looks_like_credential(word) {
            safe.push(vega_i18n::t("ui.credentials_hidden"));
        } else {
            safe.push(word);
        }
    }
    let normalized = safe.join(" ");
    let mut chars = normalized.chars();
    let bounded = chars
        .by_ref()
        .take(MESSAGE_ANCHOR_PREVIEW_LIMIT)
        .collect::<String>();
    if chars.next().is_some() {
        format!("{bounded}…")
    } else {
        bounded
    }
}

fn looks_like_credential(word: &str) -> bool {
    let lower = word.to_ascii_lowercase();
    [
        "sk-",
        "ghp_",
        "gho_",
        "ghu_",
        "ghs_",
        "ghr_",
        "github_pat_",
        "xoxb-",
        "xoxp-",
        "xoxs-",
        "api_key=",
        "apikey=",
        "token=",
        "secret=",
        "password=",
        "authorization:",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

fn preview_from_lines<'a>(lines: impl Iterator<Item = &'a StreamLine>) -> String {
    let mut preview = String::with_capacity(MESSAGE_ANCHOR_PREVIEW_INPUT_LIMIT);
    let mut scanned_chars = 0;
    'lines: for line in lines {
        for span in &line.spans {
            if scanned_chars > 0 && scanned_chars < MESSAGE_ANCHOR_PREVIEW_INPUT_LIMIT {
                preview.push(' ');
                scanned_chars += 1;
            }
            for ch in span.text.chars() {
                if scanned_chars >= MESSAGE_ANCHOR_PREVIEW_INPUT_LIMIT {
                    break 'lines;
                }
                preview.push(ch);
                scanned_chars += 1;
            }
            if scanned_chars >= MESSAGE_ANCHOR_PREVIEW_INPUT_LIMIT {
                break 'lines;
            }
        }
    }
    preview
}

pub(super) fn estimate_entry_height(
    entry: &StreamEntry,
    cx: &App,
    workspace_width: Option<f32>,
) -> f32 {
    let text_width = workspace_width
        .map(|width| width - 2. * Layout::CONVERSATION_PADDING)
        .unwrap_or(Layout::CONVERSATION_MAX_WIDTH)
        .clamp(1., Layout::CONVERSATION_MAX_WIDTH);
    let line_height = Typography::CONVERSATION * Typography::CONVERSATION_LINE_HEIGHT;
    match entry {
        StreamEntry::Subagents { group } => {
            group.read(cx).row_count(cx) as f32 * Layout::DISCLOSURE_HEADER_HEIGHT
        }
        StreamEntry::RunActivity { group } => group.read(cx).estimated_header_height(),
        StreamEntry::RunActivitySegment { group, segment } => {
            group.read(cx).estimated_segment_height(
                *segment,
                text_width - 2. * Layout::PROCESS_PADDING_X - 1.,
                cx,
            )
        }
        StreamEntry::Tool { card } => card.read(cx).estimated_height(),
        StreamEntry::ToolGroup { group } => group.read(cx).estimated_height(cx),
        StreamEntry::ArtifactGroup { group } => group.read(cx).estimated_height(),
        StreamEntry::User { lines, copy, model } => {
            estimate_user_height(copy, lines, model, text_width)
        }
        StreamEntry::Assistant { model, failure, .. } => {
            estimate_markdown_height(model, text_width)
                + Layout::CONVERSATION_BODY_TOP
                + if failure.is_some() { line_height } else { 0. }
        }
        StreamEntry::UserImages { images } => {
            let gap = Layout::USER_ATTACHMENT_TEXT_GAP;
            let per_row = ((text_width + gap) / (Layout::ATTACHMENT_THUMBNAIL + gap))
                .floor()
                .max(1.) as usize;
            let rows = images.len().div_ceil(per_row);
            Layout::USER_MESSAGE_PADDING_TOP
                + rows as f32 * Layout::ATTACHMENT_THUMBNAIL
                + rows.saturating_sub(1) as f32 * gap
        }
        StreamEntry::Summary { card }
            if card.read(cx).summary().outcome
                == vega_conversation::types::TaskSummaryOutcome::Completed =>
        {
            1.0
        }
        StreamEntry::ContextCompaction {
            record, restored, ..
        } => {
            if matches!(
                record.status,
                vega_conversation::types::ContextCompactionStatus::Succeeded
                    | vega_conversation::types::ContextCompactionStatus::Cancelled
            ) {
                return 0.;
            }
            let label = context_compaction_label(record.status, record.failure, *restored);
            if record.status == vega_conversation::types::ContextCompactionStatus::Failed {
                let columns = (text_width / (Typography::BODY * 0.56)).max(1.) as usize;
                return (display_width(&label).div_ceil(columns).max(1) as f32
                    * Typography::BODY
                    * Typography::BODY_LINE_HEIGHT)
                    .max(Layout::DISCLOSURE_HEADER_HEIGHT);
            }
            estimate_compaction_activity_height(&label, text_width)
        }
        StreamEntry::SkillActivation { .. } => 30.0,
        _ => (entry.row_count(cx).max(1) as f32 * ROW_HEIGHT + 12.0).max(30.0),
    }
}

pub(super) fn estimate_compaction_activity_height(label: &str, width: f32) -> f32 {
    let line_height = Typography::CONVERSATION * Typography::CONVERSATION_LINE_HEIGHT;
    let width = (width - Layout::ICON_SIZE - Layout::CONVERSATION_INLINE_GAP).max(1.);
    let columns = (width / (Typography::CONVERSATION * 0.56)).max(1.) as usize;
    let vertical_padding = (Layout::CONVERSATION_TOOL_HEIGHT - line_height) / 2.;
    (display_width(label).div_ceil(columns).max(1) as f32 * line_height + 2. * vertical_padding)
        .max(Layout::CONVERSATION_TOOL_HEIGHT)
}

pub(crate) fn estimate_markdown_height(model: &StreamModel, width: f32) -> f32 {
    let line_height = Typography::CONVERSATION * Typography::CONVERSATION_LINE_HEIGHT;
    let lines = model
        .committed_lines
        .iter()
        .chain(&model.pending_lines)
        .collect::<Vec<_>>();
    let gaps = lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let previous = index.checked_sub(1).map(|index| lines[index]);
            if previous.is_some_and(|previous| line.continues_code_block(previous)) {
                return 0.;
            }
            render_rows::markdown_block_gap(
                previous,
                line,
                line.in_list,
                previous.is_some_and(|line| line.in_list),
            )
        })
        .sum::<f32>();
    let code_headers = lines
        .iter()
        .enumerate()
        .filter(|(index, line)| {
            line.kind == LineKind::Code
                && line.code_block.is_some()
                && index
                    .checked_sub(1)
                    .is_none_or(|previous| !line.continues_code_block(lines[previous]))
        })
        .count() as f32
        * (Layout::COPY_ACTION_SIZE
            + Layout::COPY_ACTION_GAP
            + 2.0 * Layout::MARKDOWN_CODE_PADDING);
    let content_height = model
        .committed_lines
        .iter()
        .chain(&model.pending_lines)
        .map(|line| {
            if line.kind == LineKind::Code {
                return Typography::CODE * Typography::CODE_LINE_HEIGHT;
            }
            if let Some(table) = &line.table {
                let inset = if line.kind != LineKind::ListItem {
                    line.list_content_inset()
                } else {
                    0.0
                };
                let widths = table.column_widths(
                    width - inset - line.quotes.len() as f32 * (2. + Layout::MARKDOWN_CODE_PADDING),
                );
                return table
                    .rows
                    .iter()
                    .map(|row| {
                        let rows = row
                            .iter()
                            .zip(&widths)
                            .map(|(spans, width)| {
                                let capacity = ((width - 2.0 * Layout::MARKDOWN_TABLE_PADDING_X)
                                    / (Typography::CONVERSATION * 0.56))
                                    .max(1.0)
                                    as usize;
                                let text: String =
                                    spans.iter().map(|span| span.text.as_str()).collect();
                                text.split('\n')
                                    .map(|line| display_width(line).div_ceil(capacity).max(1))
                                    .sum::<usize>()
                            })
                            .max()
                            .unwrap_or(1);
                        rows as f32 * line_height + 2.0 * Layout::MARKDOWN_TABLE_PADDING_Y + 1.0
                    })
                    .sum();
            }
            let chars = line
                .spans
                .iter()
                .map(|span| display_width(&span.text))
                .sum::<usize>();
            let quote_inset = line.quotes.len() as f32 * (2. + Layout::MARKDOWN_CODE_PADDING);
            let list_inset = line.list_content_inset();
            let minimum_capacity = if line.quotes.is_empty() { 12. } else { 1. };
            let chars_per_line = ((width - quote_inset - list_inset)
                / (Typography::CONVERSATION * 0.56))
                .max(minimum_capacity) as usize;
            chars.div_ceil(chars_per_line).max(1) as f32 * line_height
        })
        .sum::<f32>();
    content_height + gaps + code_headers
}

pub(super) fn estimate_user_height(
    copy: &MessageCopy,
    lines: &[StreamLine],
    model: &StreamModel,
    text_width: f32,
) -> f32 {
    if !user_message_is_visible(copy, lines) {
        return 0.;
    }
    let bubble_width =
        text_width * Layout::CONVERSATION_USER_RATIO - 2. * Layout::CONVERSATION_USER_PADDING;
    estimate_markdown_height(model, bubble_width.max(1.))
        + Layout::USER_MESSAGE_PADDING_TOP
        + Layout::CONVERSATION_USER_BOTTOM
        + 16.
}

pub(super) fn text_preview(entry: &StreamEntry) -> Option<String> {
    let preview = match entry {
        StreamEntry::User { lines, .. } => preview_from_lines(
            lines
                .iter()
                .filter(|line| matches!(line.kind, LineKind::UserLine { .. })),
        ),
        StreamEntry::Assistant { model, .. } => {
            preview_from_lines(model.committed_lines.iter().chain(&model.pending_lines))
        }
        _ => return None,
    };
    Some(sanitize_anchor_preview(&preview))
}
