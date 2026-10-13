use std::{collections::BTreeMap, ops::Range};

use gpui_kit::{
    AnyElement, App, BorderStyle, Bounds, Corners, Edges, Element, ElementId, GlobalElementId,
    Hsla, InspectorElementId, IntoElement, LayoutId, PaintQuad, Pixels, SharedString, StyledText,
    TextAlign, TextLayout, TextRun, Window, WrappedLine, WrappedLineLayout, point, px, size,
    transparent_black,
};
use vega_theme::Layout;

pub(super) fn styled_text(content: String, runs: Vec<TextRun>) -> AnyElement {
    let text = StyledText::new(content.clone()).with_runs(runs.clone());
    if runs.iter().any(|run| run.background_color.is_some()) {
        InlineCodeText {
            text,
            content: content.into(),
            runs,
        }
        .into_any_element()
    } else {
        text.into_any_element()
    }
}

pub(super) fn shape(
    content: SharedString,
    runs: &[TextRun],
    layout: &TextLayout,
    window: &mut Window,
) -> Option<Vec<WrappedLine>> {
    let layouts = layout.line_layouts();
    let first = layouts.first()?;
    match window
        .text_system()
        .shape_text(content, first.font_size(), runs, first.wrap_width, None)
    {
        Ok(lines) => Some(lines.into_vec()),
        Err(error) => {
            tracing::error!(%error, "failed to shape inline styled text");
            None
        }
    }
}

fn endpoint_positions(
    line: &WrappedLineLayout,
    backgrounds: &[(Range<usize>, Hsla)],
    offset: usize,
) -> BTreeMap<usize, Pixels> {
    let mut indices = vec![0, line.len()];
    indices.extend(
        line.wrap_boundaries
            .iter()
            .map(|boundary| line.runs()[boundary.run_ix].glyphs[boundary.glyph_ix].index),
    );
    for (range, _) in backgrounds {
        indices.push(range.start.max(offset) - offset);
        indices.push(range.end.min(offset + line.len()) - offset);
    }
    indices.sort_unstable();
    indices.dedup();
    let mut positions = vec![line.unwrapped_layout.width; indices.len()];
    let mut next = 0;
    for glyph in line.runs().iter().flat_map(|run| &run.glyphs) {
        while indices.get(next).is_some_and(|index| *index <= glyph.index) {
            positions[next] = glyph.position.x;
            next += 1;
        }
    }
    indices.into_iter().zip(positions).collect()
}

pub(super) fn paint_backgrounds(layout: &TextLayout, runs: &[TextRun], window: &mut Window) {
    let mut offset = 0;
    let backgrounds = runs
        .iter()
        .filter_map(|run| {
            let start = offset;
            offset += run.len;
            run.background_color
                .filter(|_| start < offset)
                .map(|color| (start..offset, color))
        })
        .collect::<Vec<_>>();
    let bounds = layout.bounds();
    let line_height = layout.line_height();
    let align = window.text_style().text_align;
    let mut line_offset = 0;
    let mut y = bounds.top();
    for line in layout.line_layouts() {
        let unwrapped = &line.unwrapped_layout;
        let first = backgrounds.partition_point(|(range, _)| range.end <= line_offset);
        let last = backgrounds.partition_point(|(range, _)| range.start < line_offset + line.len());
        let line_backgrounds = &backgrounds[first..last];
        let positions = endpoint_positions(&line, line_backgrounds, line_offset);
        let x_for_index = |index| positions.get(&index).copied().unwrap_or(unwrapped.width);
        let height =
            (line.font_size() + px(Layout::MARKDOWN_INLINE_CODE_EXTRA_HEIGHT)).min(line_height);
        let mut start = 0;
        for end in line
            .wrap_boundaries
            .iter()
            .map(|boundary| unwrapped.runs[boundary.run_ix].glyphs[boundary.glyph_ix].index)
            .chain(std::iter::once(line.len()))
        {
            let start_x = x_for_index(start);
            let width = x_for_index(end) - start_x;
            let x = bounds.left()
                + match align {
                    TextAlign::Left => px(0.),
                    TextAlign::Center => (bounds.size.width - width) / 2.,
                    TextAlign::Right => bounds.size.width - width,
                };
            let first =
                line_backgrounds.partition_point(|(range, _)| range.end <= line_offset + start);
            let last =
                line_backgrounds.partition_point(|(range, _)| range.start < line_offset + end);
            for (range, color) in line_backgrounds.get(first..last).unwrap_or_default() {
                let from = range.start.max(line_offset + start);
                let to = range.end.min(line_offset + end);
                if from < to {
                    let left = x + x_for_index(from - line_offset) - start_x;
                    let right = x + x_for_index(to - line_offset) - start_x;
                    if right > left {
                        let fragment_size = size(right - left, height);
                        window.paint_quad(PaintQuad {
                            bounds: Bounds::new(
                                point(left, y + (line_height - height) / 2.),
                                fragment_size,
                            ),
                            corner_radii: Corners::all(px(Layout::MARKDOWN_INLINE_CODE_RADIUS))
                                .clamp_radii_for_quad_size(fragment_size),
                            background: (*color).into(),
                            border_widths: Edges::default(),
                            border_color: transparent_black(),
                            border_style: BorderStyle::default(),
                        });
                    }
                }
            }
            start = end;
            y += line_height;
        }
        line_offset += line.len() + 1;
    }
}

pub(super) fn paint_text(
    lines: &[WrappedLine],
    layout: &TextLayout,
    window: &mut Window,
    cx: &mut App,
) {
    let line_height = layout.line_height();
    let bounds = layout.bounds();
    let mut origin = bounds.origin;
    for line in lines {
        if let Err(error) = line.paint(
            origin,
            line_height,
            window.text_style().text_align,
            Some(bounds),
            window,
            cx,
        ) {
            tracing::error!(%error, "failed to paint inline styled text");
        }
        origin.y += line.size(line_height).height;
    }
}

struct InlineCodeText {
    text: StyledText,
    content: SharedString,
    runs: Vec<TextRun>,
}

impl IntoElement for InlineCodeText {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for InlineCodeText {
    type RequestLayoutState = ();
    type PrepaintState = Option<Vec<WrappedLine>>;

    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        self.text.request_layout(id, inspector, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.text
            .prepaint(id, inspector, bounds, &mut (), window, cx);
        shape(self.content.clone(), &self.runs, self.text.layout(), window)
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        lines: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(lines) = lines {
            paint_backgrounds(self.text.layout(), &self.runs, window);
            paint_text(lines, self.text.layout(), window, cx);
        } else {
            self.text
                .paint(id, inspector, bounds, &mut (), &mut (), window, cx);
        }
    }
}
