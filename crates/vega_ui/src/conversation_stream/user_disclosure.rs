use super::inline_code::{RunIndex, shape_flow};
use super::*;
use std::rc::Rc;

mod native;
pub(super) use native::control;

#[derive(Default)]
pub(super) struct UserDisclosure {
    pub(super) expanded: bool,
    generation: u64,
    width: f32,
    pub(super) revision: u64,
    preview: Option<Rc<UserPreview>>,
}

pub(super) struct UserPreview {
    pub(super) model: StreamModel,
    pub(super) collapsible: bool,
    pub(super) content_width: f32,
    pub(super) height: f32,
    full_height: f32,
}

pub(super) fn bubble_width(text_width: f32) -> f32 {
    (text_width * Layout::CONVERSATION_USER_RATIO - 2. * Layout::CONVERSATION_USER_PADDING).max(1.)
}

pub(super) fn preview(
    model: &StreamModel,
    copy: &MessageCopy,
    width: f32,
    colors: &ThemeColors,
    window: &Window,
) -> Rc<UserPreview> {
    let mut state = copy.user_disclosure.borrow_mut();
    if state.generation == copy.content_generation()
        && state.width == width
        && let Some(preview) = &state.preview
    {
        return preview.clone();
    }
    let lines = model
        .committed_lines
        .iter()
        .chain(&model.pending_lines)
        .collect::<Vec<_>>();
    let line_height = Typography::CONVERSATION * Typography::CONVERSATION_USER_LINE_HEIGHT;
    let target = (Layout::USER_MESSAGE_COLLAPSE_LINES - 1) as f32 * line_height;
    let mut height = 0.;
    let mut visible_height = 0.;
    let mut content_width: f32 = 0.;
    let mut visible = Vec::new();
    let mut stopped = false;
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        let previous = index.checked_sub(1).map(|index| lines[index]);
        let gap = render_rows::markdown_block_gap(
            previous,
            line,
            line.in_list,
            previous.is_some_and(|line| line.in_list),
            true,
        );
        let mut end = index + 1;
        let mut table_preview = None;
        let (block_height, cutoff) = if line.kind == LineKind::Code {
            content_width = width;
            while end < lines.len() && lines[end].continues_code_block(line) {
                end += 1;
            }
            let header = if line.code_block.is_some() {
                Layout::COPY_ACTION_SIZE + Layout::COPY_ACTION_GAP
            } else {
                0.
            };
            (
                (end - index) as f32 * Typography::CODE * Typography::CODE_LINE_HEIGHT
                    + 2. * Layout::MARKDOWN_CODE_PADDING
                    + header,
                None,
            )
        } else if let Some(table) = &line.table {
            content_width = width;
            let row_heights = table_row_heights(line, table, width, colors, window);
            let mut preview_height = 0.;
            let mut rows = 0;
            for row_height in &row_heights {
                if height + gap + preview_height + row_height > target {
                    break;
                }
                preview_height += row_height;
                rows += 1;
            }
            if rows > 0 {
                let mut projected = table.as_ref().clone();
                projected.rows.truncate(rows);
                let mut projected_line = line.clone();
                projected_line.table = Some(Arc::new(projected));
                table_preview = Some((projected_line, preview_height));
            }
            (row_heights.iter().sum(), None)
        } else if line.kind == LineKind::Rule {
            content_width = width;
            (9., None)
        } else {
            let text: String = line.spans.iter().map(|span| span.text.as_str()).collect();
            let style = if let LineKind::Heading(level) = line.kind {
                let mut style = render_rows::heading_run_style(level, colors);
                style.line_height =
                    DefiniteLength::Fraction(Typography::CONVERSATION_USER_LINE_HEIGHT);
                style
            } else {
                render_rows::user_message_run_style(colors.text_primary)
            };
            let font_size = style.font_size.to_pixels(window.rem_size());
            let row_height = window.pixel_snap(
                style
                    .line_height
                    .to_pixels(font_size.into(), window.rem_size()),
            );
            let runs = render_rows::block_text_runs(&line.spans, style, colors);
            let inset = line.list_content_inset()
                + line.quotes.len() as f32 * (2. + Layout::MARKDOWN_CODE_PADDING);
            let flow = measure_text(
                &text,
                &line.spans,
                runs,
                font_size,
                row_height,
                px((width - inset).max(1.)),
                window,
            );
            content_width = content_width.max(inset + f32::from(flow.size.width));
            let remaining = target - height - gap;
            let cutoff = flow
                .fragments
                .iter()
                .filter(|fragment| f32::from(fragment.origin.y + row_height) <= remaining)
                .map(|fragment| fragment.range.end)
                .max();
            (f32::from(flow.size.height.max(row_height)), cutoff)
        };
        if !stopped {
            if height + gap + block_height <= target || line.kind == LineKind::Code {
                visible.extend(lines[index..end].iter().map(|line| (*line).clone()));
                visible_height = height + gap + block_height;
                stopped = visible_height >= target;
            } else {
                if line.table.is_some() {
                    if let Some((projected, preview_height)) = table_preview {
                        visible.push(projected);
                        visible_height = height + gap + preview_height;
                    }
                } else if let Some(cutoff) = cutoff.filter(|cutoff| *cutoff > 0) {
                    visible.push(truncate_line(line, cutoff));
                    visible_height = target;
                } else if visible.is_empty() {
                    visible.push(line.clone());
                    visible_height = gap + block_height;
                }
                stopped = true;
            }
        }
        height += gap + block_height;
        index = end;
    }
    let collapsible = height > Layout::USER_MESSAGE_COLLAPSE_LINES as f32 * line_height
        || copy.original_text().chars().count() > Layout::USER_MESSAGE_COLLAPSE_CHARACTERS;
    let preview = Rc::new(UserPreview {
        model: StreamModel::from_visible_lines(visible),
        collapsible,
        content_width: content_width.ceil().clamp(1., width.max(1.)),
        height: visible_height,
        full_height: height,
    });
    state.generation = copy.content_generation();
    state.width = width;
    state.preview = Some(preview.clone());
    state.revision = state.revision.wrapping_add(1);
    preview
}

fn measure_text(
    text: &str,
    spans: &[StreamSpan],
    runs: Vec<TextRun>,
    font_size: Pixels,
    line_height: Pixels,
    width: Pixels,
    window: &Window,
) -> super::inline_code::FlowLayout {
    let links = super::links::link_ranges(spans);
    if !links.is_empty() || runs.iter().any(|run| run.background_color.is_some()) {
        return shape_flow(
            &text.to_owned().into(),
            &RunIndex::new(runs).with_links(links),
            font_size,
            line_height,
            Some(width),
            window.text_system(),
        );
    }
    let mut result = super::inline_code::FlowLayout::default();
    let Ok(lines) = window.text_system().shape_text(
        text.to_owned().into(),
        font_size,
        &runs,
        Some(width),
        None,
    ) else {
        return result;
    };
    let mut offset = 0;
    let mut y = px(0.);
    let mut widest = px(0.);
    for line in lines {
        widest = widest.max(line.width());
        let mut from = 0;
        for to in line
            .wrap_boundaries
            .iter()
            .map(|boundary| line.runs()[boundary.run_ix].glyphs[boundary.glyph_ix].index)
            .chain(std::iter::once(line.len()))
        {
            result.fragments.push(super::inline_code::Fragment {
                range: offset + from..offset + to,
                origin: gpui_kit::point(px(0.), y),
                width: line.width(),
                line_width: line.width(),
            });
            y += line_height;
            from = to;
        }
        offset += line.len() + 1;
    }
    result.size = gpui_kit::size(widest.min(width), y);
    result
}

fn truncate_line(line: &StreamLine, end: usize) -> StreamLine {
    let mut result = line.clone();
    let mut offset = 0;
    result.spans.retain_mut(|span| {
        let start = offset;
        offset += span.text.len();
        if start >= end {
            return false;
        }
        if offset > end {
            span.text.truncate(end - start);
            span.link = None;
            span.link_occurrence = None;
            if span.style == SpanStyle::Link {
                span.style = SpanStyle::Plain;
            }
        }
        true
    });
    let mut offset = 0;
    let hidden_links = line
        .spans
        .iter()
        .filter_map(|span| {
            offset += span.text.len();
            (offset > end)
                .then(|| span.link_occurrence.zip(span.link.as_ref()))
                .flatten()
        })
        .collect::<Vec<_>>();
    for span in &mut result.spans {
        if span
            .link_occurrence
            .zip(span.link.as_ref())
            .is_some_and(|link| hidden_links.contains(&link))
        {
            span.link = None;
            span.link_occurrence = None;
            if span.style == SpanStyle::Link {
                span.style = SpanStyle::Plain;
            }
        }
    }
    result
}

fn table_row_heights(
    line: &StreamLine,
    table: &StreamTable,
    width: f32,
    colors: &ThemeColors,
    window: &Window,
) -> Vec<f32> {
    let widths = table.column_widths(
        width
            - line.list_content_inset()
            - line.quotes.len() as f32 * (2. + Layout::MARKDOWN_CODE_PADDING),
    );
    table
        .rows
        .iter()
        .enumerate()
        .map(|(row_index, row)| {
            row.iter()
                .zip(&widths)
                .map(|(spans, width)| {
                    let text: String = spans.iter().map(|span| span.text.as_str()).collect();
                    let mut style = render_rows::user_message_run_style(colors.text_primary);
                    if row_index == 0 {
                        style.font_weight = Typography::HEADING_CARD_WEIGHT;
                    }
                    let runs = render_rows::block_text_runs(spans, style, colors);
                    let line_height =
                        px(Typography::CONVERSATION * Typography::CONVERSATION_USER_LINE_HEIGHT);
                    f32::from(
                        shape_flow(
                            &text.into(),
                            &RunIndex::new(runs),
                            px(Typography::CONVERSATION),
                            line_height,
                            Some(px((width - 2. * Layout::MARKDOWN_TABLE_PADDING_X).max(1.))),
                            window.text_system(),
                        )
                        .size
                        .height
                        .max(line_height),
                    )
                })
                .fold(0., f32::max)
                + 2. * Layout::MARKDOWN_TABLE_PADDING_Y
                + 1.
        })
        .collect()
}

pub(super) fn estimated_height(copy: &MessageCopy, width: f32, fallback: f32) -> f32 {
    let state = copy.user_disclosure.borrow();
    let line_height = Typography::CONVERSATION * Typography::CONVERSATION_USER_LINE_HEIGHT;
    let Some(preview) = state
        .preview
        .as_ref()
        .filter(|_| state.width == width && state.generation == copy.content_generation())
    else {
        return fallback;
    };
    if !preview.collapsible {
        return preview.full_height;
    }
    let content = if state.expanded {
        preview.full_height
    } else {
        preview.height + line_height
    };
    content + Layout::USER_MESSAGE_TOGGLE_GAP + line_height
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DisclosureStamp {
    source: u64,
    generation: u64,
    revision: u64,
    expanded: bool,
}

impl DisclosureStamp {
    fn capture(copy: &MessageCopy) -> Self {
        let state = copy.user_disclosure.borrow();
        Self {
            source: copy.id,
            generation: copy.content_generation(),
            revision: state.revision,
            expanded: state.expanded,
        }
    }

    fn matches(self, copy: &MessageCopy) -> bool {
        self == Self::capture(copy)
    }

    fn toggle(self, copy: &MessageCopy) -> bool {
        if !self.matches(copy) {
            return false;
        }
        let mut state = copy.user_disclosure.borrow_mut();
        if state.generation != self.generation
            || !state
                .preview
                .as_ref()
                .is_some_and(|preview| preview.collapsible)
        {
            return false;
        }
        state.expanded = !state.expanded;
        state.revision = state.revision.wrapping_add(1);
        drop(state);
        copy.invalidate_selection();
        true
    }
}
