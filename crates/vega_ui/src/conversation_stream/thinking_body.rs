use super::super::{inline_code, selection::SelectionDocumentBuilder};
use super::*;
use gpui_kit::{
    AvailableSpace, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, LayoutId,
    Pixels, Point, Size, TextAlign, TextStyleRefinement, WindowTextSystem, point, size,
};
use std::{cell::Cell, ops::Range, rc::Rc};

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
    prepared_epoch: u64,
    pub(super) painted: Rc<Cell<bool>>,
    rendered: Rc<Cell<usize>>,
}

impl ThinkingBody {
    pub(super) fn replace_source(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
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
        let source = self.source_text();
        let revision = self.source_revision;
        let epoch = self.body.epoch;
        let system = cx.text_system().clone();
        let work = cx.background_executor().spawn(async move {
            let text_system = WindowTextSystem::new(system);
            PreparedBody::new(&source, &style, &text_system)
        });
        self.body.task = Some(cx.spawn(async move |this, cx| {
            let prepared = work.await;
            let _ = this.update(cx, |card, cx| {
                card.body.task = None;
                if card.expanded
                    && card.ancestor_visible
                    && card.body.painted.get()
                    && card.body.epoch == epoch
                {
                    let previous_height = card.body.height();
                    let following = card.body.prepared.is_some()
                        && card.scroll.offset().y + card.scroll.max_offset().y <= px(1.);
                    let anchor = card
                        .body
                        .prepared
                        .as_ref()
                        .and_then(|old| old.anchor(-card.scroll.offset().y));
                    card.body.prepared = Some(Arc::new(prepared));
                    card.body.prepared_revision = revision;
                    card.body.prepared_epoch = epoch;
                    if following {
                        card.scroll.set_offset(point(px(0.), px(-f32::MAX)));
                    } else if let Some((byte, offset)) = anchor
                        && let Some(y) = card
                            .body
                            .prepared
                            .as_ref()
                            .and_then(|body| body.anchor_y(byte))
                    {
                        card.scroll.set_offset(point(px(0.), -(y + offset)));
                    }
                    let geometry = previous_height.min(Layout::DISCLOSURE_CONTENT_MAX_HEIGHT)
                        != card
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
                card.prepare_body(cx);
            });
        }));
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
        let painted = self.body.painted.clone();
        let rendered = self.body.rendered.clone();
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
            );
            let element = BodyElement {
                prepared,
                document: document.clone(),
                rendered,
            };
            document.wrap(element.into_any_element(), window, cx)
        } else {
            div().w_full().h(px(VIEWPORT_OVERSCAN)).into_any_element()
        };
        render_rows::selection_context_menu(
            div()
                .id("thinking-content")
                .debug_selector(|| "thinking-content".into())
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
                .child(
                    gpui_kit::canvas(
                        |_, _, _| (),
                        move |bounds, _, window, cx| {
                            let visible =
                                !bounds.intersect(&window.content_mask().bounds).is_empty();
                            painted.set(visible);
                            let width = bounds.size.width
                                - px(Layout::DISCLOSURE_PADDING_X
                                    + Layout::DISCLOSURE_SCROLLBAR_GUTTER);
                            if visible && width > px(0.) {
                                let _ = card.update(cx, |card, cx| {
                                    let changed = card.body.style.as_ref().is_none_or(|old| {
                                        old.width != width
                                            || old.scale != style.scale
                                            || old.font != style.font
                                            || old.colors.bg_base != style.colors.bg_base
                                            || old.colors.text_primary != style.colors.text_primary
                                    });
                                    if changed {
                                        card.body.epoch = card.body.epoch.wrapping_add(1);
                                        card.body.style = Some(PreparationStyle {
                                            width,
                                            ..style.clone()
                                        });
                                    }
                                    card.prepare_body(cx);
                                });
                            }
                        },
                    )
                    .absolute()
                    .inset_0(),
                )
                .into_any_element(),
            &self.source_copy,
            true,
        )
    }

    #[cfg(test)]
    pub(crate) fn test_body_preparing(&self) -> bool {
        self.body.task.is_some()
    }
    #[cfg(test)]
    pub(crate) fn test_body_fragment_count(&self) -> usize {
        self.body.prepared.as_ref().map_or(0, |body| {
            body.blocks.iter().map(|block| block.fragments.len()).sum()
        })
    }
    #[cfg(test)]
    pub(crate) fn test_body_rendered_count(&self) -> usize {
        self.body.rendered.get()
    }
    #[cfg(test)]
    pub(crate) fn test_body_revision(&self) -> u64 {
        self.body.prepared_revision
    }
    #[cfg(test)]
    pub(crate) fn test_replace_source(&mut self, source: &str, cx: &mut Context<Self>) {
        self.text = source.into();
        self.thinking_has_content = source.chars().any(|ch| !ch.is_whitespace());
        self.body.replace_source();
        self.source_copy.replace_text(source);
        self.source_revision = self.source_revision.wrapping_add(1);
        self.prepare_body(cx);
        cx.notify();
    }
}

struct PreparedFragment {
    range: Range<usize>,
    logical_range: Range<usize>,
    bounds: Bounds<Pixels>,
    runs: Vec<TextRun>,
    font_size: Pixels,
    line_height: Pixels,
}

struct Decoration {
    bounds: Bounds<Pixels>,
    color: Rgba,
}

#[derive(Default)]
struct PreparedBlock {
    origin: Point<Pixels>,
    size: Size<Pixels>,
    horizontal: bool,
    code: bool,
    fragments: Vec<PreparedFragment>,
    decorations: Vec<Decoration>,
}

struct PreparedBody {
    text: Arc<str>,
    width: Pixels,
    height: Pixels,
    blocks: Vec<PreparedBlock>,
    colors: ThemeColors,
}

impl PreparedBody {
    fn new(source: &str, style: &PreparationStyle, text_system: &WindowTextSystem) -> Self {
        let mut stream = MarkdownStream::new();
        stream.append(source);
        let mut model = StreamModel::default();
        model.sync(&stream.snapshot(), &StreamCounters::default());
        let lines: Vec<_> = model
            .committed_lines
            .iter()
            .chain(&model.pending_lines)
            .collect();
        let lists: HashSet<_> = lines
            .iter()
            .filter(|line| line.kind == LineKind::ListItem)
            .map(|line| line.block_id)
            .collect();
        let mut content = String::new();
        let mut blocks = Vec::new();
        let mut y = px(0.);
        let mut previous_text: Option<&StreamLine> = None;
        let mut index = 0;
        while index < lines.len() {
            let line = lines[index];
            let previous = index.checked_sub(1).map(|index| lines[index]);
            y += px(render_rows::markdown_block_gap(
                previous,
                line,
                lists.contains(&line.block_id),
                previous.is_some_and(|line| lists.contains(&line.block_id)),
            ));
            let mut block = PreparedBlock {
                origin: point(px(0.), y),
                ..Default::default()
            };
            if line.kind == LineKind::Code {
                block.horizontal = true;
                block.code = true;
                let mut row_y = px(Layout::MARKDOWN_CODE_PADDING);
                let mut width = style.width;
                while index < lines.len()
                    && lines[index].block_id == line.block_id
                    && lines[index].kind == LineKind::Code
                {
                    let row = lines[index];
                    append_separator(&mut content, previous_text, row);
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
                let mut x = if lists.contains(&line.block_id) && line.kind != LineKind::ListItem {
                    px((line.depth + 1) as f32 * Layout::MARKDOWN_LIST_INDENT)
                } else {
                    px(0.)
                };
                if line.kind == LineKind::Rule {
                    block.size = size(style.width, px(9.));
                    block.decorations.push(Decoration {
                        bounds: Bounds::new(point(px(0.), px(4.)), size(style.width, px(1.))),
                        color: style.colors.border_subtle,
                    });
                } else {
                    append_separator(&mut content, previous_text, line);
                    previous_text = Some(line);
                    if line.kind == LineKind::ListItem {
                        content.push_str(&"  ".repeat(line.depth));
                        x = px(line.depth as f32 * Layout::MARKDOWN_LIST_INDENT);
                        layout_text(
                            &mut block,
                            &mut content,
                            &[StreamSpan {
                                text: format!("{} ", line.marker),
                                style: SpanStyle::Plain,
                            }],
                            LineKind::Paragraph,
                            point(x, px(0.)),
                            None,
                            false,
                            style,
                            text_system,
                        );
                        x += px(Layout::MARKDOWN_LIST_INDENT);
                        if let Some(checked) = line.checked {
                            let start = block.fragments.len();
                            layout_text(
                                &mut block,
                                &mut content,
                                &[StreamSpan {
                                    text: if checked { "[x]" } else { "[ ]" }.into(),
                                    style: SpanStyle::Plain,
                                }],
                                LineKind::Paragraph,
                                point(x, px(0.)),
                                None,
                                false,
                                style,
                                text_system,
                            );
                            x = block
                                .fragments
                                .get(start)
                                .map_or(x, |fragment| fragment.bounds.right() + px(4.));
                            content.push(' ');
                        }
                    }
                    if line.kind == LineKind::Quote {
                        x += px(2. + Layout::MARKDOWN_CODE_PADDING);
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
                    if line.kind == LineKind::Quote {
                        block.decorations.push(Decoration {
                            bounds: Bounds::new(point(px(0.), px(0.)), size(px(2.), height)),
                            color: style.colors.border_subtle,
                        });
                    }
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
            blocks.push(block);
        }
        Self {
            text: content.into(),
            width: style.width,
            height: y,
            blocks,
            colors: style.colors,
        }
    }

    fn anchor(&self, y: Pixels) -> Option<(usize, Pixels)> {
        let block = self
            .blocks
            .iter()
            .find(|block| block.origin.y + block.size.height > y)?;
        let fragment = block
            .fragments
            .iter()
            .find(|fragment| block.origin.y + fragment.bounds.bottom() > y)?;
        Some((
            fragment.range.start,
            y - block.origin.y - fragment.bounds.top(),
        ))
    }

    fn anchor_y(&self, byte: usize) -> Option<Pixels> {
        self.blocks.iter().find_map(|block| {
            block
                .fragments
                .iter()
                .find(|fragment| fragment.range.contains(&byte))
                .map(|fragment| block.origin.y + fragment.bounds.top())
        })
    }
}

fn append_separator(content: &mut String, previous: Option<&StreamLine>, line: &StreamLine) {
    if let Some(previous) = previous {
        let same = previous.block_id == line.block_id
            && matches!(
                (previous.kind, line.kind),
                (LineKind::Code, LineKind::Code) | (LineKind::ListItem, LineKind::ListItem)
            );
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
                let extra = cell_width
                    - px(2. * Layout::MARKDOWN_TABLE_PADDING_X)
                    - fragment.bounds.size.width;
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
    let run_index = inline_code::RunIndex::new(runs.clone());
    if runs.iter().any(|run| run.background_color.is_some()) {
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
                start,
                logical_range.clone(),
                fragment.range,
                origin + fragment.origin,
                fragment.width,
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
                start,
                logical_range.clone(),
                offset + from..offset + to,
                point(origin.x, y),
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
    start: usize,
    logical: Range<usize>,
    range: Range<usize>,
    origin: Point<Pixels>,
    width: Pixels,
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
            font_size,
            line_height,
        });
        from = end;
    }
}

struct BodyElement {
    prepared: Arc<PreparedBody>,
    document: SelectionDocumentBuilder,
    rendered: Rc<Cell<usize>>,
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
        self.rendered.set(0);
        let mask = window.content_mask().bounds;
        let first = self.prepared.blocks.partition_point(|block| {
            bounds.top() + block.origin.y + block.size.height < mask.top() - px(VIEWPORT_OVERSCAN)
        });
        let mut elements = Vec::new();
        for (index, block) in self.prepared.blocks.iter().enumerate().skip(first) {
            let origin = bounds.origin + block.origin;
            if origin.y > mask.bottom() + px(VIEWPORT_OVERSCAN) {
                break;
            }
            let content = FragmentsElement {
                prepared: self.prepared.clone(),
                block: index,
                document: self.document.clone(),
                rendered: self.rendered.clone(),
            };
            let mut element = if block.horizontal {
                div()
                    .id(("thinking-horizontal", index))
                    .w(self.prepared.width)
                    .h(block.size.height)
                    .overflow_x_scroll()
                    .overflow_y_hidden()
                    .when(block.code, |row| {
                        row.bg(self.prepared.colors.code_bg)
                            .rounded(px(Layout::TOOL_DETAIL_RADIUS))
                    })
                    .child(content)
                    .into_any_element()
            } else {
                content.into_any_element()
            };
            element.prepaint_as_root(
                origin,
                size(
                    AvailableSpace::Definite(self.prepared.width),
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
        _: Bounds<Pixels>,
        _: &mut (),
        elements: &mut Vec<AnyElement>,
        window: &mut Window,
        cx: &mut App,
    ) {
        for element in elements {
            element.paint(window, cx);
        }
    }
}

struct FragmentsElement {
    prepared: Arc<PreparedBody>,
    block: usize,
    document: SelectionDocumentBuilder,
    rendered: Rc<Cell<usize>>,
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
            let text = &self.prepared.text[fragment.range.clone()];
            let mut element = self.document.append_positioned(
                text,
                fragment.runs.clone(),
                fragment.range.clone(),
                fragment.logical_range.clone(),
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
            self.rendered.set(self.rendered.get() + 1);
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
