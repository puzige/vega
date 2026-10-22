use super::super::{
    inline_code,
    links::{LinkRange, fragment_link_ranges, link_ranges},
    selection::SelectionDocumentBuilder,
};
use super::*;
use crate::scroll_edge_fade::{ScrollEdgeFadeExt, ScrollFade};
use gpui_kit::{
    AvailableSpace, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, LayoutId,
    Pixels, Point, Size, TextAlign, TextStyleRefinement, WindowTextSystem, point, size,
};
use std::{
    cell::{Cell, RefCell},
    ops::Range,
    rc::Rc,
};

const HORIZONTAL_FRAGMENT_BYTES: usize = 1024;
const VIEWPORT_OVERSCAN: f32 = Typography::CONVERSATION * Typography::CONVERSATION_LINE_HEIGHT;

#[derive(Clone)]
struct PreparationStyle {
    width: Pixels,
    font: gpui_kit::Font,
    colors: ThemeColors,
    scale: f32,
}

#[derive(Default)]
pub(super) struct ThinkingBody {
    prepared: Option<Arc<PreparedBody>>,
    style: Option<PreparationStyle>,
    task: Option<gpui_kit::Task<()>>,
    epoch: u64,
    prepared_revision: u64,
    projection_revision: u64,
    pending_follow: Option<bool>,
    prepared_epoch: u64,
    pub(super) painted: Rc<Cell<bool>>,
    horizontal_scrolls: Rc<RefCell<std::collections::HashMap<usize, ScrollHandle>>>,
}

impl ThinkingBody {
    pub(super) fn replace_source(&mut self) {
        self.horizontal_scrolls.borrow_mut().clear();
        self.prepared = None;
        self.pending_follow = None;
        self.epoch = self.epoch.wrapping_add(1);
    }

    pub(super) fn evict(&mut self) {
        self.replace_source();
        self.painted.set(false);
    }

    pub(super) fn visibility_changed(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.painted.set(false);
    }

    pub(super) fn height(&self) -> f32 {
        self.prepared
            .as_ref()
            .map_or(VIEWPORT_OVERSCAN, |body| f32::from(body.height))
    }
}

impl ThinkingBlock {
    pub(super) fn prepare_body(&mut self, cx: &mut Context<Self>) {
        if !self.expanded
            || !self.ancestor_visible
            || !self.body.painted.get()
            || self.body.task.is_some()
            || !self.has_source_text()
            || self.body.prepared_revision == self.source_revision
                && self.body.prepared_epoch == self.body.epoch
        {
            return;
        }
        let Some(style) = self.body.style.clone() else {
            return;
        };
        let prefix = self.visible_window().prefix();
        let prefix_len = prefix.len();
        let source = format!("{prefix}{}", self.visible_text());
        let source_start = self.visible_window().start;
        let source_identity = self.source_identity;
        let revision = self.source_revision;
        let epoch = self.body.epoch;
        let system = cx.text_system().clone();
        let work = cx.background_executor().spawn(async move {
            let text_system = WindowTextSystem::new(system);
            PreparedBody::new(&source, prefix_len, source_start, &style, &text_system)
        });
        self.body.task = Some(cx.spawn(async move |this, cx| {
            let prepared = work.await;
            let _ = this.update(cx, |card, cx| {
                card.body.task = None;
                card.complete_preparation(prepared, revision, epoch, source_identity, cx);
                card.prepare_body(cx);
            });
        }));
    }

    fn complete_preparation(
        &mut self,
        prepared: PreparedBody,
        revision: u64,
        epoch: u64,
        source_identity: u64,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.expanded
            && self.ancestor_visible
            && self.body.painted.get()
            && self.body.epoch == epoch
            && self.source_identity == source_identity
            && let Some(prepared) = prepared.clip(self.visible_window().start)
        {
            self.publish_body(prepared, cx);
            self.body.prepared_revision = revision;
            self.body.prepared_epoch = epoch;
            true
        } else {
            false
        }
    }

    pub(super) fn slide_body(&mut self) {
        let Some(previous) = self.body.prepared.take() else {
            return;
        };
        let following = self.scroll.offset().y + self.scroll.max_offset().y <= px(1.);
        let anchor = previous
            .anchor(-self.scroll.offset().y)
            .and_then(|(byte, offset)| {
                previous
                    .source_position(byte)
                    .map(|position| (position, offset))
            });
        let prepared = (*previous).clone().clip(self.visible_window().start);
        if let Some(prepared) = prepared {
            self.body
                .horizontal_scrolls
                .borrow_mut()
                .retain(|index, _| {
                    prepared
                        .block_source_range(*index)
                        .is_some_and(|range| range.end > prepared.source.start)
                });
            self.body.projection_revision = self.body.projection_revision.wrapping_add(1);
            self.source_copy.publish_retained(
                self.body.projection_revision,
                prepared.text.clone(),
                |range| previous.map_selection(&prepared, range),
            );
            self.restore_body_anchor(&prepared, following, anchor);
            self.body.prepared = Some(Arc::new(prepared));
        } else {
            self.body.pending_follow = Some(following);
            self.body.horizontal_scrolls.borrow_mut().clear();
            self.source_copy.publish_retained(
                self.body.projection_revision.wrapping_add(1),
                Arc::from(""),
                |_| None,
            );
            self.scroll.set_offset(point(px(0.), px(0.)));
        }
    }

    fn restore_body_anchor(
        &self,
        prepared: &PreparedBody,
        following: bool,
        anchor: Option<(vega_markdown::SourcePosition, Pixels)>,
    ) {
        let y = if following {
            px(-f32::MAX)
        } else {
            anchor
                .and_then(|(position, offset)| {
                    prepared
                        .rendered_position(&position)
                        .and_then(|byte| prepared.anchor_y(byte))
                        .map(|y| -(y + offset))
                })
                .unwrap_or(px(0.))
        };
        self.scroll.set_offset(point(px(0.), y));
    }

    fn publish_body(&mut self, prepared: PreparedBody, cx: &mut Context<Self>) {
        let previous_height = self.body.height();
        let previous = self.body.prepared.take();
        let following = self.body.pending_follow.take().unwrap_or_else(|| {
            previous.is_some() && self.scroll.offset().y + self.scroll.max_offset().y <= px(1.)
        });
        let anchor = previous.as_ref().and_then(|body| {
            body.anchor(-self.scroll.offset().y)
                .and_then(|(byte, offset)| {
                    body.source_position(byte)
                        .map(|position| (position, offset))
                })
        });
        let mut horizontal = self.body.horizontal_scrolls.borrow_mut();
        let previous_scrolls = std::mem::take(&mut *horizontal);
        if let Some(previous) = &previous {
            for (index, scroll) in previous_scrolls {
                let code = previous.blocks[index].code;
                if let Some(old_range) = previous.block_source_range(index)
                    && let Some((range, index)) = prepared.horizontal_ranges.get(
                        prepared
                            .horizontal_ranges
                            .partition_point(|(range, _)| range.end <= old_range.start),
                    )
                    && range.start < old_range.end
                    && prepared.blocks[*index].code == code
                {
                    horizontal.insert(*index, scroll);
                }
            }
        }
        drop(horizontal);
        self.body.projection_revision = self.body.projection_revision.wrapping_add(1);
        self.source_copy.publish_retained(
            self.body.projection_revision,
            prepared.text.clone(),
            |range| {
                previous
                    .as_ref()
                    .and_then(|old| old.map_selection(&prepared, range))
            },
        );
        self.restore_body_anchor(&prepared, following, anchor);
        self.body.prepared = Some(Arc::new(prepared));
        let geometry = previous_height.min(Layout::DISCLOSURE_CONTENT_MAX_HEIGHT)
            != self
                .body
                .height()
                .min(Layout::DISCLOSURE_CONTENT_MAX_HEIGHT);
        cx.emit(ThinkingChanged {
            header: true,
            stage: false,
            geometry,
        });
        cx.notify();
    }

    pub(crate) fn reset_body_visibility(&self) {
        self.body.painted.set(false);
    }

    pub(super) fn render_body(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let card = cx.entity().downgrade();
        let scroll = self.scroll.clone();
        let colors = theme(cx).colors;
        let selection_color = gpui_kit::base::Theme::global(cx).tokens.colors.selection;
        let prepared = self.body.prepared.clone();
        let style = PreparationStyle {
            width: px(0.),
            font: window.text_style().font(),
            colors,
            scale: window.scale_factor(),
        };
        let body = if let Some(prepared) = prepared {
            let document = SelectionDocumentBuilder::projected(
                self.source_copy.clone(),
                self.focus.clone(),
                selection_color,
                prepared.text.clone(),
                self.body.projection_revision,
            );
            let element = BodyElement {
                prepared,
                document: document.clone(),
                horizontal_scrolls: self.body.horizontal_scrolls.clone(),
            };
            document.wrap(element.into_any_element(), window, cx)
        } else {
            div().w_full().h(px(VIEWPORT_OVERSCAN)).into_any_element()
        };
        let content = render_rows::selection_context_menu(
            div()
                .id("thinking-content")
                .relative()
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
                .child(body)
                .tracked_scroll_edge_fade(
                    scroll.clone(),
                    colors.bg_base,
                    ScrollFade::compact().vertical_gutter(px(Layout::DISCLOSURE_SCROLLBAR_GUTTER)),
                )
                .into_any_element(),
            &self.source_copy,
            true,
        );
        VisibilityElement {
            content,
            visible: self.body.painted.clone(),
            card,
            style,
        }
        .into_any_element()
    }
}

#[derive(Clone)]
struct PreparedFragment {
    range: Range<usize>,
    logical_range: Range<usize>,
    bounds: Bounds<Pixels>,
    runs: Vec<TextRun>,
    links: Vec<LinkRange>,
    font_size: Pixels,
    line_height: Pixels,
    line_width: Pixels,
}

#[derive(Clone)]
struct Decoration {
    bounds: Bounds<Pixels>,
    color: Rgba,
}

#[derive(Clone, Default)]
struct PreparedBlock {
    origin: Point<Pixels>,
    size: Size<Pixels>,
    horizontal: bool,
    code: bool,
    fragments: Vec<PreparedFragment>,
    decorations: Vec<Decoration>,
    source_ranges: Vec<Range<usize>>,
    source_range: Option<Range<usize>>,
}

#[derive(Clone)]
struct PreparedBody {
    text: Arc<str>,
    source: Range<usize>,
    mapping: Arc<[vega_markdown::SourceSpan]>,
    text_start: usize,
    y_offset: Pixels,
    width: Pixels,
    height: Pixels,
    blocks: Arc<[PreparedBlock]>,
    rails: Arc<[Decoration]>,
    horizontal_ranges: Arc<[(Range<usize>, usize)]>,
    colors: ThemeColors,
}

impl PreparedBody {
    fn new(
        source: &str,
        prefix: usize,
        source_start: usize,
        style: &PreparationStyle,
        text_system: &WindowTextSystem,
    ) -> Self {
        let mut stream = MarkdownStream::new();
        stream.append(source);
        let mut model = StreamModel::default();
        model.sync(&stream.snapshot(), &StreamCounters::default());
        #[cfg(test)]
        super::super::quote_behavior_tests::model_input(&mut model);
        let lines: Vec<_> = model
            .committed_lines
            .iter()
            .chain(&model.pending_lines)
            .collect();
        let mut content = String::new();
        let mut blocks = Vec::new();
        let mut rails: Vec<((u64, usize), Decoration)> = Vec::new();
        let mut y = px(0.);
        let mut previous_text: Option<&StreamLine> = None;
        let mut index = 0;
        while index < lines.len() {
            let line = lines[index];
            let previous = index.checked_sub(1).map(|index| lines[index]);
            y += px(render_rows::markdown_block_gap(
                previous,
                line,
                line.in_list,
                previous.is_some_and(|line| line.in_list),
            ));
            let quote_marker_depth = line.list_marker_quote_depth();
            let list_inset =
                if line.in_list && (!line.starts_list_item() || quote_marker_depth.is_some()) {
                    px(line.list_content_inset())
                } else {
                    px(0.)
                };
            let gutter = px(2. + Layout::MARKDOWN_CODE_PADDING);
            let mut inset = list_inset + gutter * line.quotes.len() as f32;
            if (line.kind == LineKind::Code && line.starts_list_item())
                || quote_marker_depth.is_some()
            {
                append_separator(&mut content, previous_text, line);
                let mut marker = PreparedBlock {
                    origin: point(
                        gutter * quote_marker_depth.unwrap_or(line.quotes.len()) as f32,
                        y,
                    ),
                    ..Default::default()
                };
                let width = layout_list_marker(&mut marker, &mut content, line, style, text_system);
                marker.size = size(
                    width,
                    px(Typography::CONVERSATION * Typography::CONVERSATION_LINE_HEIGHT),
                );
                if quote_marker_depth.is_none() {
                    inset += width;
                }
                blocks.push(marker);
            }
            let block_style = PreparationStyle {
                width: (style.width - inset).max(px(1.)),
                ..style.clone()
            };
            let style = &block_style;
            let mut block = PreparedBlock {
                origin: point(inset, y),
                ..Default::default()
            };
            if line.kind == LineKind::Code {
                block.horizontal = true;
                block.code = true;
                let start = index;
                let mut row_y = px(Layout::MARKDOWN_CODE_PADDING);
                let mut width = style.width;
                while index < lines.len()
                    && (index == start || lines[index].continues_code_block(line))
                {
                    let row = lines[index];
                    if index != start || !line.starts_list_item() {
                        append_separator(&mut content, previous_text, row);
                    }
                    previous_text = Some(row);
                    let height = layout_text(
                        &mut block,
                        &mut content,
                        &row.spans,
                        LineKind::Code,
                        point(px(Layout::MARKDOWN_CODE_PADDING), row_y),
                        None,
                        false,
                        style,
                        text_system,
                    );
                    row_y += height;
                    width = block.fragments.last().map_or(width, |fragment| {
                        width.max(fragment.bounds.right() + px(Layout::MARKDOWN_CODE_PADDING))
                    });
                    index += 1;
                }
                block.size = size(width, row_y + px(Layout::MARKDOWN_CODE_PADDING));
            } else if let Some(table) = &line.table {
                append_separator(&mut content, previous_text, line);
                previous_text = Some(line);
                layout_table(&mut block, &mut content, table, style, text_system);
                index += 1;
            } else {
                let mut x = px(0.);
                if line.kind == LineKind::Rule {
                    block.size = size(style.width, px(9.));
                    block.decorations.push(Decoration {
                        bounds: Bounds::new(point(px(0.), px(4.)), size(style.width, px(1.))),
                        color: style.colors.border_subtle,
                    });
                } else {
                    if quote_marker_depth.is_none() {
                        append_separator(&mut content, previous_text, line);
                    }
                    previous_text = Some(line);
                    if line.starts_list_item() && quote_marker_depth.is_none() {
                        x = layout_list_marker(&mut block, &mut content, line, style, text_system);
                    }
                    let height = layout_text(
                        &mut block,
                        &mut content,
                        &line.spans,
                        line.kind,
                        point(x, px(0.)),
                        Some((style.width - x).max(px(1.))),
                        false,
                        style,
                        text_system,
                    );
                    block.size = size(style.width, height);
                }
                index += 1;
            }
            block.fragments.sort_by(|left, right| {
                left.bounds
                    .top()
                    .partial_cmp(&right.bounds.top())
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(left.range.start.cmp(&right.range.start))
            });
            y += block.size.height;
            for (depth, owner) in line.quotes.iter().enumerate() {
                if let Some((_, rail)) = rails
                    .iter_mut()
                    .find(|(identity, _)| *identity == owner.identity)
                {
                    rail.bounds.size.height = y - rail.bounds.top();
                } else {
                    rails.push((
                        owner.identity,
                        Decoration {
                            bounds: Bounds::new(
                                point(
                                    px(owner.list_inset()) + gutter * depth as f32,
                                    block.origin.y,
                                ),
                                size(px(2.), block.size.height),
                            ),
                            color: style.colors.border_subtle,
                        },
                    ));
                }
            }
            blocks.push(block);
        }
        let mut mapper = vega_markdown::SourceTextMap::new(source);
        let mut mapping = Vec::new();
        for block in &blocks {
            for range in &block.source_ranges {
                for mut span in mapper.take(&content[range.clone()], range.start) {
                    if span.source.start >= prefix {
                        span.source = span.source.start - prefix + source_start
                            ..span.source.end - prefix + source_start;
                        mapping.push(span);
                    }
                }
            }
        }
        let mut horizontal_ranges = Vec::new();
        for (index, block) in blocks.iter_mut().enumerate() {
            if let (Some(first), Some(last)) = (block.fragments.first(), block.fragments.last()) {
                let start = mapping.partition_point(|span| span.rendered.end <= first.range.start);
                let end = mapping.partition_point(|span| span.rendered.start < last.range.end);
                if start < end {
                    block.source_range =
                        Some(mapping[start].source.start..mapping[end - 1].source.end);
                    if block.horizontal {
                        horizontal_ranges.push((
                            mapping[start].source.start..mapping[end - 1].source.end,
                            index,
                        ));
                    }
                }
            }
            block.source_ranges.clear();
            block.source_ranges.shrink_to_fit();
        }
        Self {
            text: content.into(),
            source: source_start..source_start + source.len() - prefix,
            mapping: mapping.into(),
            text_start: 0,
            y_offset: px(0.),
            width: style.width,
            height: y,
            blocks: blocks.into(),
            rails: rails.into_iter().map(|(_, rail)| rail).collect(),
            horizontal_ranges: horizontal_ranges.into(),
            colors: style.colors,
        }
    }

    fn block_source_range(&self, index: usize) -> Option<Range<usize>> {
        self.blocks
            .get(index)?
            .source_range
            .as_ref()
            .filter(|range| range.end > self.source.start)
            .map(|range| range.start.max(self.source.start)..range.end)
    }

    fn source_position(&self, byte: usize) -> Option<vega_markdown::SourcePosition> {
        let byte = byte + self.text_start;
        self.mapping
            .iter()
            .find(|span| span.rendered.contains(&byte))
            .or_else(|| {
                self.mapping
                    .iter()
                    .rev()
                    .find(|span| span.rendered.end == byte)
            })
            .map(|span| span.position(byte))
    }

    fn rendered_position(&self, position: &vega_markdown::SourcePosition) -> Option<usize> {
        self.mapping
            .iter()
            .find_map(|span| span.rendered_position(position))
            .and_then(|byte| byte.checked_sub(self.text_start))
            .filter(|byte| *byte <= self.text.len())
    }

    fn map_selection(&self, next: &Self, range: Range<usize>) -> Option<Range<usize>> {
        let start = next.rendered_position(&self.source_position(range.start)?)?;
        let end = next.rendered_position(&self.source_position(range.end)?)?;
        let mapped = start..end;
        (self.text.get(range)? == next.text.get(mapped.clone())?).then_some(mapped)
    }

    fn clip(mut self, start: usize) -> Option<Self> {
        if self.source.end <= start {
            return None;
        }
        if self.source.start >= start {
            return Some(self);
        }
        let first = self
            .mapping
            .iter()
            .find(|span| span.source.end > start && (span.literal || span.source.start >= start))?;
        let cut = first.rendered.start
            + if first.literal {
                start.saturating_sub(first.source.start)
            } else {
                0
            };
        let removed = cut - self.text_start;
        let y = self.anchor_y(removed).unwrap_or(px(0.));
        self.text = Arc::from(&self.text[removed..]);
        self.source.start = start;
        self.text_start = cut;
        self.y_offset += y;
        self.height = (self.height - y).max(px(0.));
        Some(self)
    }

    fn anchor(&self, y: Pixels) -> Option<(usize, Pixels)> {
        let y = y + self.y_offset;
        let block = self
            .blocks
            .iter()
            .find(|block| block.origin.y + block.size.height > y)?;
        let fragment = block
            .fragments
            .iter()
            .find(|fragment| block.origin.y + fragment.bounds.bottom() > y)?;
        let byte = fragment.range.start.max(self.text_start);
        let byte = self
            .mapping
            .iter()
            .find(|span| span.rendered.end > byte)
            .map_or(byte, |span| byte.max(span.rendered.start));
        let relative = byte - self.text_start;
        let mapped_y = self
            .anchor_y(relative)
            .unwrap_or(block.origin.y + fragment.bounds.top() - self.y_offset);
        Some((relative, y - self.y_offset - mapped_y))
    }

    fn anchor_y(&self, byte: usize) -> Option<Pixels> {
        let byte = byte + self.text_start;
        self.blocks.iter().find_map(|block| {
            block
                .fragments
                .iter()
                .find(|fragment| fragment.range.contains(&byte))
                .map(|fragment| block.origin.y + fragment.bounds.top() - self.y_offset)
        })
    }
}

fn layout_list_marker(
    block: &mut PreparedBlock,
    content: &mut String,
    line: &StreamLine,
    style: &PreparationStyle,
    text_system: &WindowTextSystem,
) -> Pixels {
    content.push_str(&"  ".repeat(line.depth));
    let mut x = px(line.depth as f32 * Layout::MARKDOWN_LIST_INDENT);
    layout_text(
        block,
        content,
        &[StreamSpan {
            text: format!("{} ", line.marker),
            style: SpanStyle::Plain,
            link: None,
            link_occurrence: None,
        }],
        LineKind::Paragraph,
        point(x, px(0.)),
        None,
        false,
        style,
        text_system,
    );
    block.source_ranges.pop();
    x += px(Layout::MARKDOWN_LIST_INDENT);
    if let Some(checked) = line.checked {
        let start = block.fragments.len();
        layout_text(
            block,
            content,
            &[StreamSpan {
                text: if checked { "[x]" } else { "[ ]" }.into(),
                style: SpanStyle::Plain,
                link: None,
                link_occurrence: None,
            }],
            LineKind::Paragraph,
            point(x, px(0.)),
            None,
            false,
            style,
            text_system,
        );
        block.source_ranges.pop();
        x = block
            .fragments
            .get(start)
            .map_or(x, |fragment| fragment.bounds.right() + px(4.));
        content.push(' ');
    }
    x
}

fn append_separator(content: &mut String, previous: Option<&StreamLine>, line: &StreamLine) {
    if let Some(previous) = previous {
        let same = line.continues_code_block(previous)
            || (previous.block_id == line.block_id
                && previous.kind == LineKind::ListItem
                && line.kind == LineKind::ListItem);
        content.push_str(if same { "\n" } else { "\n\n" });
    }
}

fn layout_table(
    block: &mut PreparedBlock,
    content: &mut String,
    table: &StreamTable,
    style: &PreparationStyle,
    system: &WindowTextSystem,
) {
    let widths = table.column_widths(f32::from(style.width));
    let width = widths.iter().sum::<f32>();
    let mut y = px(0.);
    block.horizontal = true;
    for (row, cells) in table.rows.iter().enumerate() {
        if row > 0 {
            content.push('\n');
        }
        let mut x = px(0.);
        let mut height = px(0.);
        for (column, spans) in cells.iter().enumerate() {
            if column > 0 {
                content.push('\t');
            }
            let cell_width = px(widths[column]);
            let start = block.fragments.len();
            let cell_height = layout_text(
                block,
                content,
                spans,
                LineKind::Paragraph,
                point(
                    x + px(Layout::MARKDOWN_TABLE_PADDING_X),
                    y + px(Layout::MARKDOWN_TABLE_PADDING_Y),
                ),
                Some((cell_width - px(2. * Layout::MARKDOWN_TABLE_PADDING_X)).max(px(1.))),
                row == 0,
                style,
                system,
            );
            height = height.max(cell_height + px(2. * Layout::MARKDOWN_TABLE_PADDING_Y));
            for fragment in &mut block.fragments[start..] {
                let extra =
                    cell_width - px(2. * Layout::MARKDOWN_TABLE_PADDING_X) - fragment.line_width;
                fragment.bounds.origin.x += match table.alignments[column] {
                    TableAlignment::Center => extra / 2.,
                    TableAlignment::Right => extra,
                    _ => px(0.),
                };
            }
            x += cell_width;
        }
        if row == 0 {
            block.decorations.push(Decoration {
                bounds: Bounds::new(point(px(0.), y), size(px(width), height)),
                color: style.colors.bg_hover,
            });
        }
        block.decorations.push(Decoration {
            bounds: Bounds::new(point(px(0.), y + height), size(px(width), px(1.))),
            color: style.colors.border_subtle,
        });
        y += height + px(1.);
    }
    block.size = size(px(width), y);
}

#[allow(clippy::too_many_arguments)]
fn layout_text(
    block: &mut PreparedBlock,
    content: &mut String,
    spans: &[StreamSpan],
    kind: LineKind,
    origin: Point<Pixels>,
    width: Option<Pixels>,
    strong: bool,
    style: &PreparationStyle,
    system: &WindowTextSystem,
) -> Pixels {
    let text: String = spans.iter().map(|span| span.text.as_str()).collect();
    let start = content.len();
    content.push_str(&text);
    let logical_range = start..content.len();
    block.source_ranges.push(logical_range.clone());
    let mut text_style = match kind {
        LineKind::Code => render_rows::code_run_style(style.colors.text_primary),
        LineKind::Heading(level) => render_rows::heading_run_style(level, &style.colors),
        _ => render_rows::message_run_style(style.colors.text_primary),
    };
    if kind != LineKind::Code {
        text_style.font_family = style.font.family.clone();
    }
    if strong {
        text_style.font_weight = Typography::HEADING_CARD_WEIGHT;
    }
    let font_size = match kind {
        LineKind::Code => px(Typography::CODE),
        LineKind::Heading(level) => px(render_rows::heading_style(level).0),
        _ => px(Typography::CONVERSATION),
    };
    let line_height = px((f32::from(font_size)
        * if kind == LineKind::Code {
            Typography::CODE_LINE_HEIGHT
        } else {
            Typography::CONVERSATION_LINE_HEIGHT
        }
        * style.scale)
        .ceil()
        / style.scale);
    let runs = render_rows::block_text_runs(spans, text_style, &style.colors);
    let mut links = link_ranges(spans);
    super::super::links::own_links(&mut links, &format!("thinking-{start}"));
    let run_index = inline_code::RunIndex::new(runs.clone()).with_links(links.clone());
    if !links.is_empty() || runs.iter().any(|run| run.background_color.is_some()) {
        let flow = inline_code::shape_flow(
            &text.clone().into(),
            &run_index,
            font_size,
            line_height,
            width,
            system,
        );
        for fragment in flow.fragments {
            add_fragment(
                block,
                &text,
                &run_index,
                &links,
                start,
                logical_range.clone(),
                fragment.range,
                origin + fragment.origin,
                fragment.width,
                fragment.line_width,
                font_size,
                line_height,
                system,
            );
        }
        return flow.size.height.max(line_height);
    }
    let Ok(lines) = system.shape_text(text.clone().into(), font_size, &runs, width, None) else {
        return line_height;
    };
    let mut offset = 0;
    let mut y = origin.y;
    for line in lines {
        let mut from = 0;
        for to in line
            .wrap_boundaries
            .iter()
            .map(|boundary| line.runs()[boundary.run_ix].glyphs[boundary.glyph_ix].index)
            .chain(std::iter::once(line.len()))
        {
            add_fragment(
                block,
                &text,
                &run_index,
                &links,
                start,
                logical_range.clone(),
                offset + from..offset + to,
                point(origin.x, y),
                line.unwrapped_layout.x_for_index(to) - line.unwrapped_layout.x_for_index(from),
                line.unwrapped_layout.x_for_index(to) - line.unwrapped_layout.x_for_index(from),
                font_size,
                line_height,
                system,
            );
            y += line_height;
            from = to;
        }
        offset += line.len() + 1;
    }
    (y - origin.y).max(line_height)
}

#[allow(clippy::too_many_arguments)]
fn add_fragment(
    block: &mut PreparedBlock,
    text: &str,
    runs: &inline_code::RunIndex,
    links: &[LinkRange],
    start: usize,
    logical: Range<usize>,
    range: Range<usize>,
    origin: Point<Pixels>,
    width: Pixels,
    line_width: Pixels,
    font_size: Pixels,
    line_height: Pixels,
    system: &WindowTextSystem,
) {
    if range.is_empty() {
        return;
    }
    let shaped = (range.len() > HORIZONTAL_FRAGMENT_BYTES).then(|| {
        system.shape_line(
            text[range.clone()].to_owned().into(),
            font_size,
            &runs.slice(range.clone()),
            None,
        )
    });
    let mut from = range.start;
    while from < range.end {
        let mut end = (from + HORIZONTAL_FRAGMENT_BYTES).min(range.end);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        if let Some(line) = &shaped {
            end = line
                .runs
                .iter()
                .flat_map(|run| &run.glyphs)
                .map(|glyph| range.start + glyph.index)
                .take_while(|index| *index <= end)
                .filter(|index| *index > from)
                .last()
                .unwrap_or(range.end);
        }
        let x = shaped
            .as_ref()
            .map_or(px(0.), |line| line.x_for_index(from - range.start));
        let right = shaped
            .as_ref()
            .map_or(width, |line| line.x_for_index(end - range.start));
        block.fragments.push(PreparedFragment {
            range: start + from..start + end,
            logical_range: logical.clone(),
            bounds: Bounds::new(
                origin + point(x, px(0.)),
                size((right - x).max(px(0.)), line_height),
            ),
            runs: runs.slice(from..end),
            links: fragment_link_ranges(links, &(from..end)),
            font_size,
            line_height,
            line_width,
        });
        from = end;
    }
}

struct BodyElement {
    horizontal_scrolls: Rc<RefCell<std::collections::HashMap<usize, ScrollHandle>>>,
    prepared: Arc<PreparedBody>,
    document: SelectionDocumentBuilder,
}

impl IntoElement for BodyElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for BodyElement {
    type RequestLayoutState = ();
    type PrepaintState = Vec<AnyElement>;
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        _: &mut App,
    ) -> (LayoutId, ()) {
        let dimensions = size(self.prepared.width, self.prepared.height);
        (
            window.request_measured_layout(Default::default(), move |_, _, _, _| dimensions),
            (),
        )
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Vec<AnyElement> {
        let mask = window.content_mask().bounds;
        let first = self.prepared.blocks.partition_point(|block| {
            bounds.top() + block.origin.y - self.prepared.y_offset + block.size.height
                < mask.top() - px(VIEWPORT_OVERSCAN)
        });
        let mut elements = Vec::new();
        for (index, block) in self.prepared.blocks.iter().enumerate().skip(first) {
            let origin = bounds.origin + block.origin - point(px(0.), self.prepared.y_offset);
            if origin.y > mask.bottom() + px(VIEWPORT_OVERSCAN) {
                break;
            }
            let content = FragmentsElement {
                prepared: self.prepared.clone(),
                block: index,
                document: self.document.clone(),
            };
            let mut element = if block.horizontal {
                let scroll = self
                    .horizontal_scrolls
                    .borrow_mut()
                    .entry(index)
                    .or_default()
                    .clone();
                div()
                    .id(("thinking-horizontal", index))
                    .w(self.prepared.width - block.origin.x)
                    .h(block.size.height)
                    .track_scroll(&scroll)
                    .overflow_x_scroll()
                    .overflow_y_hidden()
                    .when(block.code, |row| {
                        row.bg(self.prepared.colors.code_bg)
                            .rounded(px(Layout::TOOL_DETAIL_RADIUS))
                    })
                    .child(content)
                    .tracked_scroll_edge_fade(
                        scroll,
                        if block.code {
                            self.prepared.colors.code_bg
                        } else {
                            self.prepared.colors.bg_base
                        },
                        ScrollFade::compact(),
                    )
                    .into_any_element()
            } else {
                content.into_any_element()
            };
            element.prepaint_as_root(
                origin,
                size(
                    AvailableSpace::Definite(self.prepared.width - block.origin.x),
                    AvailableSpace::Definite(block.size.height),
                ),
                window,
                cx,
            );
            elements.push(element);
        }
        elements
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        elements: &mut Vec<AnyElement>,
        window: &mut Window,
        cx: &mut App,
    ) {
        for rail in self.prepared.rails.iter() {
            let rect = Bounds::new(
                bounds.origin + rail.bounds.origin - point(px(0.), self.prepared.y_offset),
                rail.bounds.size,
            );
            if !rect.intersect(&window.content_mask().bounds).is_empty() {
                window.paint_quad(gpui_kit::fill(rect, rail.color));
            }
        }
        for element in elements {
            element.paint(window, cx);
        }
    }
}

struct FragmentsElement {
    prepared: Arc<PreparedBody>,
    block: usize,
    document: SelectionDocumentBuilder,
}
impl IntoElement for FragmentsElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for FragmentsElement {
    type RequestLayoutState = ();
    type PrepaintState = Vec<AnyElement>;
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        _: &mut App,
    ) -> (LayoutId, ()) {
        let dimensions = self.prepared.blocks[self.block].size;
        (
            window.request_measured_layout(
                gpui_kit::Style {
                    size: dimensions.map(Into::into),
                    flex_shrink: 0.,
                    ..Default::default()
                },
                move |_, _, _, _| dimensions,
            ),
            (),
        )
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Vec<AnyElement> {
        let block = &self.prepared.blocks[self.block];
        let mask = window.content_mask().bounds;
        let first = block.fragments.partition_point(|fragment| {
            bounds.top() + fragment.bounds.bottom() < mask.top() - px(VIEWPORT_OVERSCAN)
        });
        let mut elements = Vec::new();
        for fragment in block.fragments.iter().skip(first) {
            let origin = bounds.origin + fragment.bounds.origin;
            if origin.y > mask.bottom() + px(VIEWPORT_OVERSCAN) {
                break;
            }
            if origin.x + fragment.bounds.size.width < mask.left() - px(VIEWPORT_OVERSCAN)
                || origin.x > mask.right() + px(VIEWPORT_OVERSCAN)
            {
                continue;
            }
            if fragment.range.end <= self.prepared.text_start {
                continue;
            }
            let start = fragment.range.start.max(self.prepared.text_start);
            let range =
                start - self.prepared.text_start..fragment.range.end - self.prepared.text_start;
            let text = &self.prepared.text[range.clone()];
            let runs = if start > fragment.range.start {
                inline_code::RunIndex::new(fragment.runs.clone())
                    .slice(start - fragment.range.start..fragment.range.len())
            } else {
                fragment.runs.clone()
            };
            let mut element = self.document.append_positioned(
                text,
                runs,
                fragment_link_ranges(
                    &fragment.links,
                    &(start - fragment.range.start..fragment.range.len()),
                ),
                range,
                fragment
                    .logical_range
                    .start
                    .saturating_sub(self.prepared.text_start)
                    ..fragment
                        .logical_range
                        .end
                        .saturating_sub(self.prepared.text_start),
            );
            window.with_text_style(
                Some(TextStyleRefinement {
                    font_size: Some(fragment.font_size.into()),
                    line_height: Some(gpui_kit::relative(
                        f32::from(fragment.line_height) / f32::from(fragment.font_size),
                    )),
                    white_space: Some(gpui_kit::WhiteSpace::Nowrap),
                    text_align: Some(TextAlign::Left),
                    ..Default::default()
                }),
                |window| {
                    element.prepaint_as_root(
                        origin,
                        size(
                            AvailableSpace::Definite(fragment.bounds.size.width),
                            AvailableSpace::Definite(fragment.line_height),
                        ),
                        window,
                        cx,
                    );
                },
            );
            elements.push(element);
        }
        elements
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        elements: &mut Vec<AnyElement>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let block = &self.prepared.blocks[self.block];
        for decoration in &block.decorations {
            let rect = Bounds::new(
                bounds.origin + decoration.bounds.origin,
                decoration.bounds.size,
            );
            if !rect.intersect(&window.content_mask().bounds).is_empty() {
                window.paint_quad(gpui_kit::fill(rect, decoration.color));
            }
        }
        window.with_text_style(
            Some(TextStyleRefinement {
                text_align: Some(TextAlign::Left),
                ..Default::default()
            }),
            |window| {
                for element in elements {
                    element.paint(window, cx);
                }
            },
        );
    }
}

struct VisibilityLease(Rc<Cell<bool>>);
impl Drop for VisibilityLease {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

struct VisibilityElement {
    content: AnyElement,
    visible: Rc<Cell<bool>>,
    card: gpui_kit::WeakEntity<ThinkingBlock>,
    style: PreparationStyle,
}
impl IntoElement for VisibilityElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for VisibilityElement {
    type RequestLayoutState = ();
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        Some("thinking-body-presence".into())
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.content.request_layout(window, cx), ())
    }
    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let visible = !bounds.intersect(&window.content_mask().bounds).is_empty();
        self.visible.set(visible);
        if let Some(id) = id {
            window.with_element_state(id, |lease: Option<VisibilityLease>, _| {
                let lease = lease.unwrap_or_else(|| VisibilityLease(self.visible.clone()));
                ((), lease)
            });
        }
        self.content.prepaint(window, cx);
        let width = bounds.size.width
            - px(Layout::DISCLOSURE_PADDING_X + Layout::DISCLOSURE_SCROLLBAR_GUTTER);
        if visible && width > px(0.) {
            let _ = self.card.update(cx, |card, cx| {
                let changed = card.body.style.as_ref().is_none_or(|old| {
                    old.width != width
                        || old.scale != self.style.scale
                        || old.font != self.style.font
                        || old.colors.bg_base != self.style.colors.bg_base
                        || old.colors.text_primary != self.style.colors.text_primary
                });
                if changed {
                    card.body.epoch = card.body.epoch.wrapping_add(1);
                    card.body.style = Some(PreparationStyle {
                        width,
                        ..self.style.clone()
                    });
                }
                card.prepare_body(cx);
            });
        }
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.content.paint(window, cx);
    }
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn quote_test_body(
    source: &str,
    width: Pixels,
    offset: Pixels,
    copy: MessageCopy,
    focus: FocusHandle,
    horizontal_scrolls: Rc<RefCell<std::collections::HashMap<usize, ScrollHandle>>>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let style = PreparationStyle {
        width,
        font: window.text_style().font(),
        colors: theme(cx).colors,
        scale: window.scale_factor(),
    };
    let mut prepared = PreparedBody::new(
        source,
        0,
        0,
        &style,
        &WindowTextSystem::new(cx.text_system().clone()),
    );
    prepared.y_offset = offset;
    let prepared = Arc::new(prepared);
    let document = SelectionDocumentBuilder::projected(
        copy,
        focus,
        gpui_kit::base::Theme::global(cx).tokens.colors.selection,
        prepared.text.clone(),
        1,
    );
    let element = BodyElement {
        prepared,
        document: document.clone(),
        horizontal_scrolls,
    };
    document.wrap(element.into_any_element(), window, cx)
}

#[cfg(test)]
mod quote_lifecycle_tests {
    use super::*;
    use crate::conversation_stream::quote_behavior_tests::{drag_selection, frame, text_bounds};
    use gpui_kit::{TestAppContext, WindowHandle};

    struct SelectionTestRoot {
        card: gpui_kit::Entity<ThinkingBlock>,
    }

    impl Render for SelectionTestRoot {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .child(gpui_kit::base::TextSelectionLayer)
                .child(self.card.clone())
        }
    }

    fn settle(
        handle: WindowHandle<SelectionTestRoot>,
        card: &gpui_kit::Entity<ThinkingBlock>,
        cx: &mut TestAppContext,
    ) {
        for _ in 0..6 {
            frame(handle, cx);
            cx.run_until_parked();
        }
        cx.update(|cx| {
            card.update(cx, |card, _| {
                assert!(card.body.task.is_none());
                assert_eq!(card.body.prepared_revision, card.source_revision);
                assert_eq!(card.body.prepared_epoch, card.body.epoch);
                assert!(card.body.prepared.is_some());
            });
        });
    }

    #[gpui_kit::test]
    fn thinking_publication_retains_source_anchored_selection_and_rejects_stale_layout(
        cx: &mut TestAppContext,
    ) {
        let source = "> 起点🙂\n>\n> > 内层é mixed中\n>\n> 终点Ω";
        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_global(vega_theme::Theme::light());
        });
        let handle = cx.open_window(size(px(320.), px(900.)), |_, cx| {
            let card = cx.new(|cx| {
                let mut card = ThinkingBlock::new(cx);
                card.append(source, false, usize::MAX);
                card.set_expanded(true, cx);
                card
            });
            SelectionTestRoot { card }
        });
        let card = handle.update(cx, |root, _, _| root.card.clone()).unwrap();
        settle(handle, &card, cx);
        let painted = frame(handle, cx);
        let first = painted
            .text
            .iter()
            .find(|run| run.text.contains("内层é"))
            .unwrap();
        let last = painted
            .text
            .iter()
            .find(|run| run.text.contains("终点Ω"))
            .unwrap();
        let from = first.layout.position_for_index(0).unwrap() + point(px(0.1), px(3.));
        let to = last.layout.position_for_index(last.text.len()).unwrap() + point(px(0.1), px(3.));
        let start_source = source.find("内层é").unwrap();
        let end_source = source.len();
        let (copy, stale, epoch, revision, identity) = handle
            .update(cx, |_, _, cx| {
                card.update(cx, |card, _| {
                    let prepared = card.body.prepared.as_ref().unwrap();
                    let start = prepared
                        .rendered_position(&vega_markdown::SourcePosition {
                            source: start_source..start_source,
                            offset: 0,
                        })
                        .unwrap();
                    let end = prepared
                        .rendered_position(&vega_markdown::SourcePosition {
                            source: end_source..end_source,
                            offset: 0,
                        })
                        .unwrap();
                    assert_eq!(&prepared.text[start..end], "内层é mixed中\n\n终点Ω");
                    (
                        card.source_copy.clone(),
                        prepared.as_ref().clone(),
                        card.body.epoch,
                        card.source_revision,
                        card.source_identity,
                    )
                })
            })
            .unwrap();
        drag_selection(handle, cx, from, to);
        cx.update(|cx| copy.copy_selected_text(cx));
        let expected = Some("内层é mixed中\n\n终点Ω".to_owned());
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            expected
        );
        handle
            .update(cx, |_, _, cx| {
                card.update(cx, |card, cx| {
                    card.append("\n>\n> 新增🙂 tail", false, usize::MAX);
                    cx.notify();
                })
            })
            .unwrap();
        cx.simulate_window_resize(handle.into(), size(px(620.), px(900.)));
        settle(handle, &card, cx);
        cx.update(|cx| {
            cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(
                "clipboard-sentinel".to_owned(),
            ));
            copy.copy_selected_text(cx);
        });
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            expected
        );
        let current = frame(handle, cx);
        assert!(!text_bounds(&current, "新增").is_empty());
        handle
            .update(cx, |_, _, cx| {
                card.update(cx, |card, cx| {
                    assert_ne!(epoch, card.body.epoch);
                    assert!(!card.complete_preparation(stale, revision, epoch, identity, cx));
                    assert_eq!(card.body.prepared_revision, card.source_revision);
                })
            })
            .unwrap();
        let after = frame(handle, cx);
        assert_eq!(text_bounds(&after, "新增"), text_bounds(&current, "新增"));
        cx.update(|cx| {
            cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(
                "clipboard-sentinel".to_owned(),
            ));
            copy.copy_selected_text(cx);
        });
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            expected
        );
        cx.simulate_window_resize(handle.into(), size(px(260.), px(900.)));
        settle(handle, &card, cx);
        cx.update(|cx| {
            cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(
                "clipboard-sentinel".to_owned(),
            ));
            copy.copy_selected_text(cx);
        });
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            expected
        );
    }
}
