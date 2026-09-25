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
            Self::User => "用户消息",
            Self::Assistant => "助手消息",
            Self::Tool => "工具活动",
            Self::Plan => "计划消息",
            Self::Summary => "任务结果",
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
                StreamEntry::User { lines, .. } => (
                    MessageAnchorKind::User,
                    preview_from_lines(
                        lines
                            .iter()
                            .filter(|line| matches!(line.kind, LineKind::UserLine { .. })),
                    ),
                ),
                StreamEntry::UserImages { .. } => (MessageAnchorKind::User, "图片消息".to_string()),
                StreamEntry::Assistant { model, .. } => (
                    MessageAnchorKind::Assistant,
                    preview_from_lines(model.committed_lines.iter().chain(&model.pending_lines)),
                ),
                StreamEntry::Tool { .. }
                | StreamEntry::ToolGroup { .. }
                | StreamEntry::Artifact { .. } => (MessageAnchorKind::Tool, "工具活动".to_string()),
                StreamEntry::Plan { .. } => (MessageAnchorKind::Plan, "计划消息".to_string()),
                StreamEntry::Summary { .. } => (MessageAnchorKind::Summary, "任务结果".to_string()),
                StreamEntry::ContextCompaction { .. }
                | StreamEntry::RunActivity { .. }
                | StreamEntry::RunActivitySegment { .. }
                | StreamEntry::Permission { .. }
                | StreamEntry::SkillActivation { .. } => continue,
            };
            if !seen.insert(message_id) {
                continue;
            }
            projections.push(MessageAnchorProjection {
                entry_index,
                message_id: message_id.to_string(),
                kind,
                preview: sanitize_anchor_preview(&preview),
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
            // During a list remeasure GPUI can temporarily return stale row
            // bounds with a zero width. Those sizes no longer describe a
            // useful line-wrap measurement, so retain the last valid cache
            // entry (or use the bounded content estimate below).
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
        cx: &App,
        fallback_viewport_height: f32,
    ) -> MessageAnchorGeometry {
        let entry_heights = self
            .entries
            .iter()
            .zip(&self.entry_identities)
            .map(|(entry, identity)| {
                self.measured_entry_heights
                    .get(&identity.key)
                    .copied()
                    .unwrap_or_else(|| estimate_entry_height(entry, cx, self.workspace_width))
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
        let top = self.list.logical_scroll_top();
        let current_prefix = entry_heights
            .iter()
            .take(top.item_ix.min(entry_heights.len()))
            .sum::<f32>();
        let current_offset = if top.item_ix < entry_heights.len() {
            f32::from(top.offset_in_item).clamp(0.0, entry_heights[top.item_ix])
        } else {
            0.0
        };
        let measured_viewport_height = f32::from(self.list.viewport_bounds().size.height);
        let viewport_height = if measured_viewport_height > 0.0 {
            measured_viewport_height
        } else {
            fallback_viewport_height.max(0.0)
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

    pub(crate) fn render_message_anchor_rail(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.capture_visible_entry_heights();
        let window_height = f32::from(window.bounds().size.height);
        let fallback_viewport_height = (window_height - 260.0).max(0.0);
        let geometry = self.message_anchor_geometry(cx, fallback_viewport_height);
        let shown = geometry.should_show();
        if self.message_anchor_rail_last_shown != shown {
            self.message_anchor_rail_last_shown = shown;
            self.measured_entry_heights.clear();
            if !self.entries.is_empty() {
                self.list.remeasure();
            }
            cx.notify();
        }
        if !shown {
            return div().into_any_element();
        }

        let anchors = geometry
            .anchors
            .iter()
            .map(|anchor| (anchor.message_id.clone(), anchor.fraction))
            .collect::<Vec<_>>();
        let is_focused = self.message_anchor_focus.is_focused(window);
        let selected_id = self
            .message_anchor_hovered
            .as_ref()
            .filter(|id| anchors.iter().any(|(candidate, _)| candidate == *id))
            .or_else(|| {
                self.message_anchor_keyboard_id
                    .as_ref()
                    .filter(|id| anchors.iter().any(|(candidate, _)| candidate == *id))
            })
            .cloned()
            .or_else(|| {
                if is_focused {
                    nearest_anchor_id(&anchors, geometry.current_fraction)
                } else {
                    None
                }
            });
        let selected = selected_id.as_deref().and_then(|id| {
            geometry
                .anchors
                .iter()
                .find(|anchor| anchor.message_id == id)
        });
        let tooltip_text = selected
            .map(|anchor| format!("{} · {}", anchor.kind.label(), anchor.preview))
            .unwrap_or_else(|| "按上下键选择消息，按 Enter 跳转".to_string());
        let accessible_label = selected
            .map(|anchor| {
                format!(
                    "消息导航，当前为{}。{}。使用上下键选择，按 Enter 跳转。",
                    anchor.kind.label(),
                    anchor.preview
                )
            })
            .unwrap_or_else(|| "消息导航。使用上下键选择，按 Enter 跳转。".to_string());
        let default_keyboard_id = selected_id
            .clone()
            .or_else(|| nearest_anchor_id(&anchors, geometry.current_fraction));
        let selected_for_paint = selected_id.clone();
        let points_for_paint = geometry
            .anchors
            .iter()
            .map(|anchor| (anchor.message_id.clone(), anchor.fraction, anchor.kind))
            .collect::<Vec<_>>();
        let current_fraction = geometry.current_fraction;
        let colors = theme(cx).colors;
        let track_bounds = std::rc::Rc::new(std::cell::Cell::new(None));
        let prepaint_bounds = track_bounds.clone();
        let mouse_track_bounds = track_bounds.clone();
        let click_track_bounds = track_bounds.clone();
        let mouse_positions = anchors.clone();
        let click_positions = anchors.clone();
        let default_keyboard_id = default_keyboard_id.clone();
        let rail_canvas = gpui_kit::canvas(
            move |bounds, _, _| {
                prepaint_bounds.set(Some(bounds));
                bounds
            },
            move |bounds, _, window, _| {
                let center_x = bounds.center().x;
                let track = gpui_kit::Bounds::new(
                    gpui_kit::point(center_x - px(1.), bounds.top() + px(3.)),
                    gpui_kit::size(px(2.), (bounds.size.height - px(6.)).max(px(0.))),
                );
                window.paint_quad(gpui_kit::fill(track, colors.border_subtle));
                let mut last_y = f32::NEG_INFINITY;
                for (message_id, fraction, kind) in &points_for_paint {
                    let y = bounds.top() + bounds.size.height * *fraction;
                    let selected = selected_for_paint.as_deref() == Some(message_id.as_str());
                    if !selected && f32::from(y) - last_y < 2.5 {
                        continue;
                    }
                    last_y = f32::from(y);
                    let color = anchor_color(*kind, &colors);
                    let side = if selected { px(7.) } else { px(4.) };
                    let marker = gpui_kit::Bounds::new(
                        gpui_kit::point(center_x - side / 2., y - side / 2.),
                        gpui_kit::size(side, side),
                    );
                    window.paint_quad(gpui_kit::fill(marker, color));
                }
                let current_y = bounds.top() + bounds.size.height * current_fraction;
                let current = gpui_kit::Bounds::new(
                    gpui_kit::point(center_x - px(4.), current_y - px(4.)),
                    gpui_kit::size(px(8.), px(8.)),
                );
                window.paint_quad(gpui_kit::fill(current, colors.accent));
            },
        )
        .size_full();
        div()
            .id("message-anchor-rail")
            .debug_selector(|| "message-anchor-rail".into())
            .aria_label(accessible_label)
            .tab_stop(true)
            .track_focus(&self.message_anchor_focus)
            .when(is_focused, |rail| {
                rail.border_1().border_color(colors.accent).rounded_md()
            })
            .w(px(20.))
            .h_full()
            .flex_shrink_0()
            .relative()
            .tooltip(move |_, cx| crate::icons::tooltip(tooltip_text.clone(), cx))
            .on_mouse_down(
                gpui_kit::MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    window.focus(&this.message_anchor_focus, cx);
                    cx.stop_propagation();
                }),
            )
            .on_scroll_wheel(
                cx.listener(|this, event: &gpui_kit::ScrollWheelEvent, _, cx| {
                    let delta = event
                        .delta
                        .pixel_delta(px(Typography::MESSAGE_LINE_HEIGHT * Typography::MESSAGE));
                    this.list.scroll_by(-delta.y);
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .on_mouse_move(
                cx.listener(move |this, event: &gpui_kit::MouseMoveEvent, _, cx| {
                    let Some(bounds) = mouse_track_bounds.get() else {
                        return;
                    };
                    let fraction = track_fraction_at_y(bounds, event.position.y);
                    let hovered = nearest_anchor_id(&mouse_positions, fraction);
                    if this.message_anchor_hovered != hovered {
                        this.message_anchor_hovered = hovered;
                        cx.notify();
                    }
                }),
            )
            .on_mouse_exit(cx.listener(|this, _: &gpui_kit::MouseExitEvent, _, cx| {
                if this.message_anchor_hovered.take().is_some() {
                    cx.notify();
                }
            }))
            .on_mouse_up(
                gpui_kit::MouseButton::Left,
                cx.listener(move |this, event: &gpui_kit::MouseUpEvent, _, cx| {
                    let Some(bounds) = click_track_bounds.get() else {
                        return;
                    };
                    let fraction = track_fraction_at_y(bounds, event.position.y);
                    let Some(message_id) = nearest_anchor_id(&click_positions, fraction) else {
                        return;
                    };
                    this.message_anchor_hovered = Some(message_id.clone());
                    this.message_anchor_keyboard_id = Some(message_id.clone());
                    this.request_message_location(&message_id, cx);
                    cx.stop_propagation();
                }),
            )
            .on_key_down(
                cx.listener(move |this, event: &gpui_kit::KeyDownEvent, window, cx| {
                    if anchors.is_empty() {
                        return;
                    }
                    let start_id = this
                        .message_anchor_keyboard_id
                        .clone()
                        .or_else(|| default_keyboard_id.clone());
                    let start_index = start_id
                        .as_ref()
                        .and_then(|id| anchors.iter().position(|(candidate, _)| candidate == id))
                        .unwrap_or(0);
                    match event.keystroke.key.as_str() {
                        "up" => {
                            let next = start_index.saturating_sub(1);
                            this.message_anchor_keyboard_id = Some(anchors[next].0.clone());
                            this.message_anchor_hovered = None;
                        }
                        "down" => {
                            let next = (start_index + 1).min(anchors.len() - 1);
                            this.message_anchor_keyboard_id = Some(anchors[next].0.clone());
                            this.message_anchor_hovered = None;
                        }
                        "enter" | "space" => {
                            if let Some(message_id) = start_id {
                                this.message_anchor_keyboard_id = Some(message_id.clone());
                                this.request_message_location(&message_id, cx);
                            }
                        }
                        _ => return,
                    }
                    let _ = window;
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .child(rail_canvas)
            .into_any_element()
    }
}

fn nearest_anchor_id(anchors: &[(String, f32)], fraction: f32) -> Option<String> {
    anchors
        .iter()
        .min_by(|(_, left), (_, right)| {
            (*left - fraction)
                .abs()
                .total_cmp(&(*right - fraction).abs())
        })
        .map(|(message_id, _)| message_id.clone())
}

fn track_fraction_at_y(bounds: gpui_kit::Bounds<Pixels>, y: Pixels) -> f32 {
    if bounds.size.height <= px(0.) {
        return 0.0;
    }
    (f32::from(y - bounds.top()) / f32::from(bounds.size.height)).clamp(0.0, 1.0)
}

fn anchor_color(kind: MessageAnchorKind, colors: &ThemeColors) -> Rgba {
    match kind {
        MessageAnchorKind::User => colors.brand_primary,
        MessageAnchorKind::Assistant | MessageAnchorKind::Summary => colors.text_secondary,
        MessageAnchorKind::Tool | MessageAnchorKind::Plan => colors.text_tertiary,
    }
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
            safe.push("[凭据已隐藏]");
            words.next();
        } else if looks_like_credential(word) {
            safe.push("[凭据已隐藏]");
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

fn estimate_entry_height(entry: &StreamEntry, cx: &App, workspace_width: Option<f32>) -> f32 {
    let width = workspace_width.unwrap_or(Layout::CONTENT_MAX_WIDTH);
    let text_width = (width - 48.0).max(160.0);
    let line_height = Typography::MESSAGE * Typography::MESSAGE_LINE_HEIGHT;
    let chars_per_line = (text_width / (Typography::MESSAGE * 0.56)).max(12.0) as usize;
    match entry {
        StreamEntry::User { lines, .. } => {
            let bubble_width = text_width * Layout::USER_MESSAGE_MAX_WIDTH_RATIO;
            let user_chars_per_line =
                (bubble_width / (Typography::MESSAGE * 0.56)).max(12.0) as usize;
            let rows = lines
                .iter()
                .filter(|line| matches!(line.kind, LineKind::UserLine { .. }))
                .map(|line| {
                    let chars = line
                        .spans
                        .iter()
                        .map(|span| display_width(&span.text))
                        .sum::<usize>();
                    chars.div_ceil(user_chars_per_line).max(1)
                })
                .sum::<usize>();
            (rows.max(1) as f32 * line_height + 34.0).max(44.0)
        }
        StreamEntry::Assistant { model, failure, .. } => {
            let rows = model
                .committed_lines
                .iter()
                .chain(&model.pending_lines)
                .map(|line| {
                    let chars = line
                        .spans
                        .iter()
                        .map(|span| display_width(&span.text))
                        .sum::<usize>();
                    chars.div_ceil(chars_per_line).max(1)
                })
                .sum::<usize>()
                + usize::from(failure.is_some());
            (rows.max(1) as f32 * line_height + 12.0).max(30.0)
        }
        StreamEntry::UserImages { .. } => 128.0,
        StreamEntry::Summary { card }
            if card.read(cx).summary().outcome
                == vega_conversation::types::TaskSummaryOutcome::Completed =>
        {
            1.0
        }
        StreamEntry::ContextCompaction { .. } | StreamEntry::SkillActivation { .. } => 30.0,
        _ => (entry.row_count(cx).max(1) as f32 * ROW_HEIGHT + 12.0).max(30.0),
    }
}
