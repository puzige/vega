use std::{cell::RefCell, collections::BTreeMap, ops::Range, rc::Rc};

use gpui_kit::{
    AnyElement, App, AvailableSpace, BorderStyle, Bounds, Corners, Edges, Element, ElementId,
    GlobalElementId, Hitbox, HitboxBehavior, Hsla, InspectorElementId, IntoElement, LayoutId,
    PaintQuad, Pixels, SharedString, StyledText, TextAlign, TextLayout, TextRun, TextStyle,
    TextStyleRefinement, WhiteSpace, Window, WrappedLine, WrappedLineLayout, point, px, size,
    transparent_black,
};
use vega_theme::{Layout, Typography};

use super::links::{LinkRange, fragment_link_ranges, link_element_id, register_link_interactions};

pub(super) fn styled_text(content: String, runs: Vec<TextRun>) -> AnyElement {
    let text = StyledText::new(content.clone()).with_runs(runs.clone());
    if runs.iter().any(|run| run.background_color.is_some()) {
        flow(content.into(), runs, |_, content, runs| {
            InlineCodeText {
                id: None,
                text: StyledText::new(content.clone()).with_runs(runs.clone()),
                content,
                runs,
                links: Vec::new(),
            }
            .into_any_element()
        })
    } else {
        text.into_any_element()
    }
}

pub(super) fn interactive_text(
    content: String,
    runs: Vec<TextRun>,
    mut links: Vec<LinkRange>,
    id_prefix: String,
) -> AnyElement {
    if links.is_empty() {
        return styled_text(content, runs);
    }
    super::links::own_links(&mut links, &id_prefix);
    flow_with_links(
        content.into(),
        runs,
        links.clone(),
        move |range, content, runs| {
            let fragment_links = fragment_link_ranges(&links, &range);
            InlineCodeText {
                id: (!fragment_links.is_empty()).then(|| link_element_id(&id_prefix, range.start)),
                text: StyledText::new(content.clone()).with_runs(runs.clone()),
                content,
                runs,
                links: fragment_links,
            }
            .into_any_element()
        },
    )
}

struct InlineCodePrepaint {
    lines: Option<Vec<WrappedLine>>,
    hitbox: Option<Hitbox>,
    targets: Vec<AnyElement>,
}

struct InlineCodeText {
    id: Option<ElementId>,
    text: StyledText,
    content: SharedString,
    runs: Vec<TextRun>,
    links: Vec<LinkRange>,
}

impl IntoElement for InlineCodeText {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for InlineCodeText {
    type RequestLayoutState = ();
    type PrepaintState = InlineCodePrepaint;

    fn id(&self) -> Option<ElementId> {
        self.id.clone()
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
        InlineCodePrepaint {
            lines: shape(self.content.clone(), &self.runs, self.text.layout(), window),
            hitbox: (!self.links.is_empty()).then(|| {
                window.insert_hitbox(
                    crate::link_preview::interactive_bounds(bounds, &self.links),
                    HitboxBehavior::Normal,
                )
            }),
            targets: crate::link_preview::prepaint_link_targets(
                id,
                self.text.layout(),
                &self.links,
                window,
                cx,
            ),
        }
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(lines) = prepaint.lines.as_ref() {
            paint_backgrounds(self.text.layout(), &self.runs, window);
            paint_text(lines, self.text.layout(), window, cx);
        } else {
            self.text
                .paint(id, inspector, bounds, &mut (), &mut (), window, cx);
        }
        for target in &mut prepaint.targets {
            target.paint(window, cx);
        }
        if let Some(hitbox) = prepaint.hitbox.clone() {
            register_link_interactions(
                id,
                hitbox,
                self.text.layout().clone(),
                self.links.clone(),
                window,
                cx,
            );
        }
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
    paint_inline_backgrounds(layout, runs, false, window);
}

pub(super) fn paint_inline_backgrounds(
    layout: &TextLayout,
    runs: &[TextRun],
    assistant: bool,
    window: &mut Window,
) {
    let padding = if assistant {
        Layout::ASSISTANT_INLINE_CODE_PADDING_X
    } else {
        Layout::MARKDOWN_INLINE_CODE_PADDING_X
    };
    let radius = if assistant {
        Layout::ASSISTANT_INLINE_CODE_RADIUS
    } else {
        Layout::MARKDOWN_INLINE_CODE_RADIUS
    };
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
        let height = if assistant {
            line_height + px(Layout::ASSISTANT_INLINE_CODE_PADDING_Y * 2.)
        } else {
            (line.font_size() + px(Layout::MARKDOWN_INLINE_CODE_EXTRA_HEIGHT)).min(line_height)
        };
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
                    let left = x + x_for_index(from - line_offset) - start_x - px(padding);
                    let right = x + x_for_index(to - line_offset) - start_x + px(padding);
                    if right > left {
                        let fragment_size = size(right - left, height);
                        window.paint_quad(PaintQuad {
                            bounds: Bounds::new(
                                point(left, y + (line_height - height) / 2.),
                                fragment_size,
                            ),
                            corner_radii: Corners::all(px(radius))
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

pub(super) fn flow(
    content: SharedString,
    runs: Vec<TextRun>,
    fragment: impl Fn(Range<usize>, SharedString, Vec<TextRun>) -> AnyElement + 'static,
) -> AnyElement {
    flow_with_links(content, runs, Vec::new(), fragment)
}

pub(super) fn flow_with_links(
    content: SharedString,
    runs: Vec<TextRun>,
    links: Vec<LinkRange>,
    fragment: impl Fn(Range<usize>, SharedString, Vec<TextRun>) -> AnyElement + 'static,
) -> AnyElement {
    flow_with_punctuation(content, runs, links, false, fragment)
}

pub(super) fn flow_with_punctuation(
    content: SharedString,
    runs: Vec<TextRun>,
    links: Vec<LinkRange>,
    punctuation_rules: bool,
    fragment: impl Fn(Range<usize>, SharedString, Vec<TextRun>) -> AnyElement + 'static,
) -> AnyElement {
    InlineFlow {
        content,
        runs: Rc::new(
            RunIndex::new(runs)
                .with_links(links)
                .with_punctuation_rules(punctuation_rules),
        ),
        fragment: Box::new(fragment),
    }
    .into_any_element()
}

pub(super) struct RunIndex {
    runs: Vec<(Range<usize>, TextRun)>,
    code: Vec<Range<usize>>,
    links: Vec<LinkRange>,
    punctuation_rules: bool,
}

impl RunIndex {
    pub(super) fn new(runs: Vec<TextRun>) -> Self {
        let mut offset = 0;
        let runs = runs
            .into_iter()
            .map(|run| {
                let start = offset;
                offset += run.len;
                (start..offset, run)
            })
            .collect::<Vec<_>>();
        let code = runs
            .iter()
            .filter(|(range, run)| !range.is_empty() && run.background_color.is_some())
            .map(|(range, _)| range.clone())
            .collect();
        Self {
            runs,
            code,
            links: Vec::new(),
            punctuation_rules: false,
        }
    }

    pub(super) fn with_punctuation_rules(mut self, enabled: bool) -> Self {
        self.punctuation_rules = enabled;
        self
    }

    pub(super) fn with_links(mut self, links: Vec<LinkRange>) -> Self {
        self.links = links;
        self
    }

    pub(super) fn slice(&self, range: Range<usize>) -> Vec<TextRun> {
        let start = self
            .runs
            .partition_point(|(source, _)| source.end <= range.start);
        let end = self
            .runs
            .partition_point(|(source, _)| source.start < range.end);
        self.runs[start..end]
            .iter()
            .map(|(source, run)| {
                let mut run = run.clone();
                run.len = source.end.min(range.end) - source.start.max(range.start);
                run
            })
            .collect()
    }

    fn code_ranges(&self, range: Range<usize>) -> &[Range<usize>] {
        let start = self
            .code
            .partition_point(|source| source.end <= range.start);
        let end = self.code.partition_point(|source| source.start < range.end);
        &self.code[start..end]
    }

    fn code_padding(&self) -> f32 {
        if self.punctuation_rules {
            Layout::ASSISTANT_INLINE_CODE_PADDING_X
        } else {
            Layout::MARKDOWN_INLINE_CODE_PADDING_X
        }
    }

    fn padding(&self, range: Range<usize>) -> Pixels {
        px(
            self.code_ranges(range.clone()).len() as f32 * self.code_padding() * 2.
                + self
                    .links
                    .iter()
                    .filter(|link| link.starts_here && range.contains(&link.range.start))
                    .count() as f32
                    * Layout::MARKDOWN_LINK_LEADING_WIDTH,
        )
    }
}

#[derive(Clone)]
pub(super) struct Fragment {
    pub(super) range: Range<usize>,
    pub(super) origin: gpui_kit::Point<Pixels>,
    pub(super) width: Pixels,
    pub(super) line_width: Pixels,
    font_size: Pixels,
    line_height: Pixels,
    baseline: Pixels,
    code: bool,
}

#[derive(Default)]
pub(super) struct FlowLayout {
    pub(super) fragments: Vec<Fragment>,
    pub(super) size: gpui_kit::Size<Pixels>,
}

struct FlowMeasurement {
    width: Option<Pixels>,
    layout: FlowLayout,
}

#[derive(Default)]
struct FlowMeasurements {
    entries: Vec<FlowMeasurement>,
}

impl FlowMeasurements {
    fn insert(&mut self, width: Option<Pixels>, layout: FlowLayout) -> gpui_kit::Size<Pixels> {
        let size = layout.size;
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.width == width) {
            entry.layout = layout;
        } else {
            self.entries.push(FlowMeasurement { width, layout });
        }
        size
    }

    fn for_bounds(&self, bounds: gpui_kit::Size<Pixels>, scale: f32) -> Option<&FlowLayout> {
        let tolerance = px(1. / scale);
        self.entries
            .iter()
            .filter(|entry| (entry.layout.size.height - bounds.height).abs() <= tolerance)
            .filter_map(|entry| {
                let distance = (entry.layout.size.width - bounds.width).abs();
                let available_matches = entry
                    .width
                    .is_some_and(|width| (width - bounds.width).abs() <= tolerance);
                if available_matches {
                    Some((0u8, distance, entry))
                } else if distance <= tolerance {
                    Some((1u8, distance, entry))
                } else {
                    None
                }
            })
            .min_by(|left, right| {
                left.0.cmp(&right.0).then(
                    left.1
                        .partial_cmp(&right.1)
                        .unwrap_or(std::cmp::Ordering::Equal),
                )
            })
            .map(|(_, _, entry)| &entry.layout)
    }
}

type FragmentRenderer = dyn Fn(Range<usize>, SharedString, Vec<TextRun>) -> AnyElement;

struct InlineFlow {
    content: SharedString,
    runs: Rc<RunIndex>,
    fragment: Box<FragmentRenderer>,
}

impl IntoElement for InlineFlow {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for InlineFlow {
    type RequestLayoutState = Rc<RefCell<FlowMeasurements>>;
    type PrepaintState = Vec<(AnyElement, TextStyleRefinement)>;

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
    ) -> (LayoutId, Self::RequestLayoutState) {
        let state = Rc::new(RefCell::new(FlowMeasurements::default()));
        let measured = state.clone();
        let content = self.content.clone();
        let runs = self.runs.clone();
        let style = window.text_style();
        let id = window.request_measured_layout(
            Default::default(),
            move |known, available, window, _| {
                let width = if style.white_space == WhiteSpace::Normal {
                    known.width.or(match available.width {
                        AvailableSpace::Definite(width) => Some(width),
                        _ => None,
                    })
                } else {
                    None
                };
                let layout = layout_flow(&content, &runs, &style, width, window);
                measured.borrow_mut().insert(width, layout)
            },
        );
        (id, state)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let align = window.text_style().text_align;
        let matched = {
            let measurements = state.borrow();
            measurements
                .for_bounds(bounds.size, window.scale_factor())
                .map(|layout| layout.fragments.clone())
        };
        let fragments = matched.unwrap_or_else(|| {
            let style = window.text_style();
            let width = if style.white_space == WhiteSpace::Normal {
                Some(bounds.size.width)
            } else {
                None
            };
            layout_flow(&self.content, &self.runs, &style, width, window).fragments
        });
        let mut elements = Vec::with_capacity(fragments.len());
        window.with_text_style(
            Some(TextStyleRefinement {
                white_space: Some(WhiteSpace::Nowrap),
                text_align: Some(TextAlign::Left),
                ..Default::default()
            }),
            |window| {
                for fragment in fragments {
                    let offset = match align {
                        TextAlign::Left => px(0.),
                        TextAlign::Center => (bounds.size.width - fragment.line_width) / 2.,
                        TextAlign::Right => bounds.size.width - fragment.line_width,
                    };
                    let refinement = TextStyleRefinement {
                        font_size: Some(fragment.font_size.into()),
                        line_height: Some(gpui_kit::relative(
                            f32::from(fragment.line_height) / f32::from(fragment.font_size),
                        )),
                        white_space: Some(WhiteSpace::Nowrap),
                        text_align: Some(TextAlign::Left),
                        ..Default::default()
                    };
                    let content =
                        SharedString::from(self.content[fragment.range.clone()].to_owned());
                    let mut element = (self.fragment)(
                        fragment.range.clone(),
                        content,
                        self.runs.slice(fragment.range),
                    );
                    window.with_text_style(Some(refinement.clone()), |window| {
                        element.prepaint_as_root(
                            bounds.origin + fragment.origin + point(offset, px(0.)),
                            size(
                                AvailableSpace::Definite(fragment.width),
                                AvailableSpace::Definite(fragment.line_height),
                            ),
                            window,
                            cx,
                        );
                    });
                    elements.push((element, refinement));
                }
            },
        );
        elements
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        elements: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.with_text_style(
            Some(TextStyleRefinement {
                text_align: Some(TextAlign::Left),
                ..Default::default()
            }),
            |window| {
                for (element, refinement) in elements {
                    window.with_text_style(Some(refinement.clone()), |window| {
                        element.paint(window, cx);
                    });
                }
            },
        );
    }
}

fn layout_flow(
    content: &SharedString,
    runs: &RunIndex,
    style: &TextStyle,
    width: Option<Pixels>,
    window: &mut Window,
) -> FlowLayout {
    let font_size = style.font_size.to_pixels(window.rem_size());
    let line_height = window.pixel_snap(
        style
            .line_height
            .to_pixels(font_size.into(), window.rem_size()),
    );
    shape_flow_with_metrics(
        content,
        runs,
        (font_size, line_height),
        width,
        window.text_system(),
        &|height| window.pixel_snap(height),
    )
}

pub(super) fn shape_flow(
    content: &SharedString,
    runs: &RunIndex,
    font_size: Pixels,
    line_height: Pixels,
    width: Option<Pixels>,
    text_system: &gpui_kit::WindowTextSystem,
) -> FlowLayout {
    shape_flow_with_metrics(
        content,
        runs,
        (font_size, line_height),
        width,
        text_system,
        &|height| height,
    )
}

fn shape_flow_with_metrics(
    content: &SharedString,
    runs: &RunIndex,
    metrics: (Pixels, Pixels),
    width: Option<Pixels>,
    text_system: &gpui_kit::WindowTextSystem,
    snap: &dyn Fn(Pixels) -> Pixels,
) -> FlowLayout {
    let (font_size, line_height) = metrics;
    let all_runs = runs.slice(0..content.len());
    let lines = match text_system.shape_text(content.clone(), font_size, &all_runs, None, None) {
        Ok(lines) => lines,
        Err(error) => {
            tracing::error!(%error, "failed to measure inline styled text");
            return FlowLayout::default();
        }
    };
    let mut result = FlowLayout::default();
    let mut offset = 0;
    let mut y = px(0.);
    for line in lines {
        let mut indices = line
            .runs()
            .iter()
            .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.index))
            .chain([line.len()])
            .collect::<Vec<_>>();
        indices.sort_unstable();
        indices.dedup();
        let mut positions = vec![line.width(); indices.len()];
        let mut next = 0;
        for glyph in line.runs().iter().flat_map(|run| &run.glyphs) {
            while indices.get(next).is_some_and(|index| *index <= glyph.index) {
                positions[next] = glyph.position.x;
                next += 1;
            }
        }
        let mut start_ix = 0;
        if line.len() == 0 {
            y += line_height;
        }
        while indices
            .get(start_ix)
            .is_some_and(|index| *index < line.len())
        {
            let start = indices[start_ix];
            let mut fit_ix = start_ix + 1;
            while fit_ix + 1 < indices.len()
                && width.is_none_or(|width| {
                    positions[fit_ix + 1] - positions[start_ix]
                        + runs.padding(offset + start..offset + indices[fit_ix + 1])
                        <= width
                })
            {
                fit_ix += 1;
            }
            if runs.punctuation_rules
                && let Some(width) = width
            {
                let mut low = start_ix + 1;
                let mut high = indices.len();
                while low + 1 < high {
                    let middle = low + (high - low) / 2;
                    let (_, measured) = position_fragments(
                        content,
                        runs,
                        offset + start..offset + indices[middle],
                        font_size,
                        y,
                        text_system,
                    );
                    if measured <= width {
                        low = middle;
                    } else {
                        high = middle;
                    }
                }
                fit_ix = low;
            }
            let mut end = indices[fit_ix];
            if runs.punctuation_rules && end < line.len() {
                let remaining = offset + start..offset + line.len();
                let text = SharedString::from(content[remaining.clone()].to_owned());
                let row_runs = runs.slice(remaining);
                let shaped = text_system.shape_line(text.clone(), font_size, &row_runs, None);
                let budget = shaped.x_for_index(end - start);
                if let Ok(wrapped) = text_system.shape_text(
                    text,
                    font_size,
                    &row_runs,
                    Some(budget.max(px(0.01))),
                    None,
                ) && let Some(first) = wrapped.first()
                    && let Some(boundary) = first.wrap_boundaries.first()
                {
                    let candidate = first.runs()[boundary.run_ix].glyphs[boundary.glyph_ix].index;
                    if candidate > 0 && start + candidate <= end {
                        end = start + candidate;
                    }
                }
            } else if fit_ix + 1 < indices.len() {
                let prefix_end = offset + indices[fit_ix + 1];
                let prefix_start = offset + start;
                let prefix = SharedString::from(content[prefix_start..prefix_end].to_owned());
                if let Ok(wrapped) = text_system.shape_text(
                    prefix,
                    font_size,
                    &runs.slice(prefix_start..prefix_end),
                    Some((positions[fit_ix] - positions[start_ix]).max(px(0.01))),
                    None,
                ) && let Some(first) = wrapped.first()
                    && let Some(boundary) = first.wrap_boundaries.first()
                {
                    let candidate = first.runs()[boundary.run_ix].glyphs[boundary.glyph_ix].index;
                    if candidate > 0 {
                        end = start + candidate;
                    }
                }
            }
            if runs.punctuation_rules {
                end = punctuation_boundary(
                    &content[offset..offset + line.len()],
                    &indices,
                    start,
                    end,
                );
            }
            let (mut fragments, mut actual_width) = position_fragments(
                content,
                runs,
                offset + start..offset + end,
                font_size,
                y,
                text_system,
            );
            while let Some(width) = width.filter(|width| actual_width > *width) {
                let range = offset + start..offset + end;
                let text = SharedString::from(content[range.clone()].to_owned());
                let row_runs = runs.slice(range.clone());
                let shaped = text_system.shape_line(text.clone(), font_size, &row_runs, None);
                let budget = (shaped.width() - (actual_width - width) - px(0.01)).max(px(0.01));
                let wrapped =
                    text_system.shape_text(text, font_size, &row_runs, Some(budget), None);
                let candidate = wrapped.ok().and_then(|lines| {
                    lines.first().and_then(|line| {
                        line.wrap_boundaries.first().map(|boundary| {
                            line.runs()[boundary.run_ix].glyphs[boundary.glyph_ix].index
                        })
                    })
                });
                let candidate = candidate.map(|candidate| {
                    if runs.punctuation_rules {
                        punctuation_boundary(
                            &content[offset..offset + line.len()],
                            &indices,
                            start,
                            start + candidate,
                        ) - start
                    } else {
                        candidate
                    }
                });
                let Some(candidate) =
                    candidate.filter(|candidate| *candidate > 0 && *candidate < end - start)
                else {
                    break;
                };
                end = start + candidate;
                (fragments, actual_width) = position_fragments(
                    content,
                    runs,
                    offset + start..offset + end,
                    font_size,
                    y,
                    text_system,
                );
            }
            let row_height =
                align_fragments(&mut fragments, line_height, y, runs.punctuation_rules, snap);
            result.fragments.extend(fragments);
            result.size.width = result.size.width.max(actual_width);
            y += row_height;
            start_ix = indices.partition_point(|index| *index < end);
        }
        offset += line.len() + 1;
    }
    result.size.height = y;
    result
}

fn align_fragments(
    fragments: &mut [Fragment],
    line_height: Pixels,
    y: Pixels,
    assistant: bool,
    snap: &dyn Fn(Pixels) -> Pixels,
) -> Pixels {
    if !assistant {
        for fragment in fragments {
            fragment.line_height = line_height;
        }
        return line_height;
    }
    for fragment in fragments.iter_mut() {
        let height = snap(fragment.line_height);
        fragment.baseline += (height - fragment.line_height) / 2.;
        fragment.line_height = height;
    }
    let padding = |fragment: &Fragment| {
        if fragment.code {
            px(Layout::ASSISTANT_INLINE_CODE_PADDING_Y)
        } else {
            px(0.)
        }
    };
    let ascent = fragments.iter().fold(px(0.), |height, fragment| {
        height.max(fragment.baseline + padding(fragment))
    });
    let descent = fragments.iter().fold(px(0.), |height, fragment| {
        height.max(fragment.line_height - fragment.baseline + padding(fragment))
    });
    let row_height = line_height.max(ascent + descent);
    let baseline = (row_height - ascent - descent) / 2. + ascent;
    for fragment in fragments {
        fragment.origin.y = y + baseline - fragment.baseline;
    }
    row_height
}

fn punctuation_boundary(text: &str, boundaries: &[usize], start: usize, proposed: usize) -> usize {
    let valid = |boundary: usize| {
        boundary == text.len()
            || (!text[boundary..]
                .chars()
                .next()
                .is_some_and(|ch| "，。、；：！？）》】」』〕〉〗〙〛…,.!?;:%”’".contains(ch))
                && !text[..boundary]
                    .chars()
                    .next_back()
                    .is_some_and(|ch| "（《【「『〔〈〖〘〚(“‘".contains(ch)))
    };
    boundaries
        .iter()
        .copied()
        .rev()
        .find(|boundary| *boundary > start && *boundary <= proposed && valid(*boundary))
        .or_else(|| {
            boundaries
                .iter()
                .copied()
                .find(|boundary| *boundary > proposed && valid(*boundary))
        })
        .unwrap_or(text.len())
}

fn position_fragments(
    content: &str,
    runs: &RunIndex,
    range: Range<usize>,
    font_size: Pixels,
    y: Pixels,
    text_system: &gpui_kit::WindowTextSystem,
) -> (Vec<Fragment>, Pixels) {
    let mut boundaries = vec![range.start];
    for code in runs.code_ranges(range.clone()) {
        boundaries.push(code.start.max(range.start));
        boundaries.push(code.end.min(range.end));
    }
    for link in &runs.links {
        if link.range.start > range.start && link.range.start < range.end {
            boundaries.push(link.range.start);
        }
        if link.range.end > range.start && link.range.end < range.end {
            boundaries.push(link.range.end);
        }
    }
    boundaries.push(range.end);
    boundaries.sort_unstable();
    boundaries.dedup();
    let mut fragments = Vec::new();
    let mut x = px(0.);
    for edge in boundaries.windows(2) {
        let range = edge[0]..edge[1];
        if range.is_empty() {
            continue;
        }
        let fragment_runs = runs.slice(range.clone());
        let padding = if fragment_runs
            .first()
            .is_some_and(|run| run.background_color.is_some())
        {
            px(runs.code_padding())
        } else {
            px(0.)
        };
        let code = padding > px(0.);
        let fragment_font = if code && runs.punctuation_rules {
            font_size * Typography::ASSISTANT_INLINE_CODE_SCALE
        } else {
            font_size
        };
        let shaped = text_system.shape_line(
            SharedString::from(content[range.clone()].to_owned()),
            fragment_font,
            &fragment_runs,
            None,
        );
        if runs
            .links
            .iter()
            .any(|link| link.starts_here && link.range.start == range.start)
        {
            x += px(Layout::MARKDOWN_LINK_LEADING_WIDTH);
        }
        x += padding;
        fragments.push(Fragment {
            range,
            origin: point(x, y),
            width: shaped.width(),
            line_width: px(0.),
            font_size: fragment_font,
            line_height: shaped.ascent + shaped.descent,
            baseline: shaped.ascent,
            code,
        });
        x += shaped.width() + padding;
    }
    for fragment in &mut fragments {
        fragment.line_width = x;
    }
    (fragments, x)
}

#[cfg(all(test, feature = "assistant-layout-regressions"))]
pub(super) mod layout_tests;
