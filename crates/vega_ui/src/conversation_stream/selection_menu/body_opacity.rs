use super::super::super::stream_fade::{FadeFrame, StreamFade, TextPart};
use super::*;
use nsh_platform_gpui::reading::{ReadingOpacityFence, ReadingOpacityRange};

type PartKey = (u64, usize, usize, TextPart);
pub(in crate::conversation_stream) type FadeSource = Rc<RefCell<StreamFade>>;

#[derive(Clone)]
struct Segment {
    fragment: String,
    range: Range<usize>,
    part: Range<usize>,
}

#[derive(Clone)]
struct Part {
    key: PartKey,
    text: Arc<str>,
    segments: Vec<Segment>,
}

#[derive(Clone, Default)]
pub(super) struct Mapping(Vec<Part>);

impl Mapping {
    pub(super) fn build(
        content: &Content,
        model: &super::super::super::model::StreamModel,
    ) -> Self {
        let mut output = Self::default();
        for (block, _, blocks) in &content.rows {
            let lines = model
                .committed_lines
                .iter()
                .chain(&model.pending_lines)
                .filter(|line| line.block_id == *block)
                .collect::<Vec<_>>();
            let mut line = 0;
            output.blocks(content, *block, blocks, &lines, &mut line);
        }
        output
    }

    fn add(
        &mut self,
        content: &Content,
        key: PartKey,
        expected: String,
        slices: Vec<(String, Range<usize>)>,
    ) {
        let mut text = String::new();
        let mut segments = Vec::new();
        for (id, range) in slices {
            let Some(fragment) = content.fragments.iter().find(|fragment| fragment.id == id) else {
                return;
            };
            let Some(value) = fragment.text.get(range.clone()) else {
                return;
            };
            let start = text.len();
            text.push_str(value);
            segments.push(Segment {
                fragment: id,
                range,
                part: start..text.len(),
            });
        }
        if text == expected && !text.is_empty() {
            self.0.push(Part {
                key,
                text: text.into(),
                segments,
            });
        }
    }

    fn slices(content: &Content, inlines: &[Inline]) -> Vec<(String, Range<usize>)> {
        let mut output = Vec::new();
        for inline in inlines {
            match inline {
                Inline::Custom(node) => {
                    if let Some(id) = node.attributes.get("binding")
                        && let Some(fragment) =
                            content.fragments.iter().find(|fragment| &fragment.id == id)
                    {
                        output.push((id.clone(), 0..fragment.text.len()));
                    }
                }
                Inline::Strong { content: spans }
                | Inline::Emphasis { content: spans }
                | Inline::Strikethrough { content: spans }
                | Inline::Link { content: spans, .. } => {
                    output.extend(Self::slices(content, spans));
                }
                _ => {}
            }
        }
        output
    }

    fn blocks(
        &mut self,
        content: &Content,
        block: u64,
        blocks: &[Block],
        lines: &[&super::super::super::model::StreamLine],
        cursor: &mut usize,
    ) {
        for value in blocks {
            match value {
                Block::Paragraph { content: spans } | Block::Heading { content: spans, .. } => {
                    let text: String = spans.iter().map(Inline::plain_text).collect();
                    if text.is_empty() {
                        continue;
                    }
                    if let Some(line) = lines.get(*cursor) {
                        let expected: String =
                            line.spans.iter().map(|span| span.text.as_str()).collect();
                        self.add(
                            content,
                            (block, *cursor, 0, TextPart::Body),
                            expected,
                            Self::slices(content, spans),
                        );
                    }
                    *cursor += 1;
                }
                Block::CodeBlock(code) => {
                    if let Some(id) = &code.meta
                        && let Some(fragment) =
                            content.fragments.iter().find(|fragment| &fragment.id == id)
                    {
                        let mut byte = 0;
                        for text in fragment.text.split_inclusive('\n') {
                            let value = text.strip_suffix('\n').unwrap_or(text);
                            if let Some(line) = lines.get(*cursor) {
                                let expected: String =
                                    line.spans.iter().map(|span| span.text.as_str()).collect();
                                self.add(
                                    content,
                                    (block, *cursor, 0, TextPart::Body),
                                    expected,
                                    vec![(id.clone(), byte..byte + value.len())],
                                );
                            }
                            byte += text.len();
                            *cursor += 1;
                        }
                    }
                }
                Block::Blockquote { children, .. } => {
                    self.blocks(content, block, children, lines, cursor)
                }
                Block::Custom(node) => {
                    let start = *cursor;
                    self.blocks(content, block, &node.children, lines, cursor);
                    if node.name == "vega-reading-list-item"
                        && let Some(line) = lines.get(start)
                        && let Some(id) = node.attributes.get("binding")
                        && let Some(fragment) =
                            content.fragments.iter().find(|fragment| &fragment.id == id)
                    {
                        self.0.retain(|part| {
                            part.key.0 != block
                                || part.key.1 != start
                                || !matches!(part.key.3, TextPart::Marker | TextPart::Checkbox)
                        });
                        let (part, expected) = line.checked.map_or_else(
                            || (TextPart::Marker, line.marker.clone()),
                            |checked| {
                                (
                                    TextPart::Checkbox,
                                    if checked { "[x]" } else { "[ ]" }.to_owned(),
                                )
                            },
                        );
                        let expected = if part == TextPart::Marker {
                            format!("{expected} ")
                        } else {
                            expected
                        };
                        if part == TextPart::Marker
                            && expected.strip_suffix(' ') == Some(&fragment.text)
                        {
                            let text: Arc<str> = expected.into();
                            self.0.push(Part {
                                key: (block, start, 0, part),
                                text,
                                segments: vec![Segment {
                                    fragment: id.clone(),
                                    range: 0..fragment.text.len(),
                                    part: 0..fragment.text.len(),
                                }],
                            });
                        } else {
                            self.add(
                                content,
                                (block, start, 0, part),
                                expected,
                                vec![(id.clone(), 0..fragment.text.len())],
                            );
                        }
                    }
                }
                Block::Table(table) => {
                    if table.head.is_empty() {
                        continue;
                    }
                    if let Some(line) = lines.get(*cursor)
                        && let Some(display) = &line.table
                    {
                        for (row, cells) in
                            std::iter::once(&table.head).chain(&table.rows).enumerate()
                        {
                            for (column, cell) in cells.iter().enumerate() {
                                if let Some(spans) =
                                    display.rows.get(row).and_then(|cells| cells.get(column))
                                {
                                    let expected: String =
                                        spans.iter().map(|span| span.text.as_str()).collect();
                                    self.add(
                                        content,
                                        (block, *cursor, row, TextPart::Cell(column)),
                                        expected,
                                        Self::slices(content, &cell.content),
                                    );
                                }
                            }
                        }
                    }
                    *cursor += 1;
                }
                Block::Rule => *cursor += 1,
                _ => {}
            }
        }
    }

    pub(super) fn ranges(
        &self,
        content: &Content,
        frame: &FadeFrame,
        snapshot: &ReadingSnapshot,
    ) -> Option<Vec<ReadingOpacityRange>> {
        if !content.matches_snapshot(snapshot) {
            return None;
        }
        let mut output = Vec::new();
        for part in &self.0 {
            let (block, line, row, kind) = part.key;
            let Some(ranges) = frame.ranges(block, line, row, kind, &part.text) else {
                continue;
            };
            for segment in &part.segments {
                let fragment = content
                    .fragments
                    .iter()
                    .find(|fragment| fragment.id == segment.fragment)?;
                let (index, tag) =
                    snapshot
                        .texts
                        .iter()
                        .enumerate()
                        .find_map(|(index, text)| {
                            text.content
                                .test_ids
                                .iter()
                                .find(|tag| tag.test_id.as_ref() == segment.fragment)
                                .map(|tag| (index, tag))
                        })?;
                for fade in ranges {
                    if !fade.opacity.is_finite()
                        || !(0.0..=1.0).contains(&fade.opacity)
                        || part.text.get(fade.range.clone()).is_none()
                    {
                        return None;
                    }
                    let start = fade.range.start.max(segment.part.start);
                    let end = fade.range.end.min(segment.part.end);
                    if start >= end {
                        continue;
                    }
                    let start = segment.range.start + start - segment.part.start;
                    let end = segment.range.start + end - segment.part.start;
                    let range = native_range(&fragment.text, start..end, tag.start)?;
                    output.push(ReadingOpacityRange {
                        text: index,
                        range,
                        opacity: fade.opacity,
                    });
                }
            }
        }
        output.sort_by_key(|range| (range.text, range.range.start));
        if output
            .windows(2)
            .any(|pair| pair[0].text == pair[1].text && pair[0].range.end > pair[1].range.start)
        {
            return None;
        }
        Some(output)
    }
}

fn native_range(text: &str, range: Range<usize>, origin: u32) -> Option<Range<u32>> {
    text.get(range.clone())?;
    let start = u32::try_from(text.get(..range.start)?.encode_utf16().count()).ok()?;
    let end = u32::try_from(text.get(..range.end)?.encode_utf16().count()).ok()?;
    Some(origin.checked_add(start)?..origin.checked_add(end)?)
}

#[cfg(test)]
#[path = "body_opacity_tests.rs"]
mod tests;

pub(super) struct Publication {
    fence: Option<ReadingOpacityFence>,
    source: Option<FadeSource>,
    mount: Option<Rc<()>>,
}

impl Publication {
    pub(super) fn new() -> Self {
        Self {
            fence: None,
            source: None,
            mount: None,
        }
    }

    pub(super) fn set_source(&mut self, source: Option<FadeSource>) {
        self.source = source;
    }

    pub(super) fn clear(&mut self, surface: Option<&Surface<Projection, Request>>, cx: &App) {
        if let Some(fence) = self.fence.take()
            && let Some(surface) = surface
        {
            surface.entity().read(cx).clear_reading_opacity(&fence);
        }
    }
}

impl BodyState {
    pub(super) fn clear_opacity(&mut self, cx: &App) {
        self.opacity.clear(self.surface.as_ref(), cx);
    }

    pub(super) fn paint_opacity(&mut self, window: &Window, cx: &App) {
        if !self.valid(window, cx) {
            if self.acknowledged.is_some()
                && let Some(source) = &self.opacity.source
            {
                source.borrow_mut().hide();
            }
            self.clear_opacity(cx);
            return;
        }
        let Some(source) = &self.opacity.source else {
            self.clear_opacity(cx);
            return;
        };
        let frame = source.borrow_mut().frame(window, cx.reduce_motion());
        let Some((surface, area)) = self.surface.as_ref().zip(self.area) else {
            self.clear_opacity(cx);
            return;
        };
        let view = surface.entity();
        let view = view.read(cx);
        let Some(snapshot) = view.reading_snapshot(area) else {
            self.clear_opacity(cx);
            return;
        };
        let Some(ranges) =
            self.projection
                .content
                .opacity
                .ranges(&self.projection.content, &frame, &snapshot)
        else {
            self.clear_opacity(cx);
            return;
        };
        if ranges.is_empty() {
            self.clear_opacity(cx);
            return;
        }
        let Some(fence) = view.reading_opacity_fence(&snapshot) else {
            self.clear_opacity(cx);
            return;
        };
        if let Some(fence) = view.replace_reading_opacity(&fence, ranges) {
            self.opacity.fence = Some(fence);
        } else {
            self.clear_opacity(cx);
        }
    }
}

pub(super) struct Presence {
    state: WeakEntity<BodyState>,
    app: AsyncApp,
    mount: Rc<()>,
}

impl Presence {
    pub(super) fn new(state: WeakEntity<BodyState>, cx: &mut App) -> Self {
        let mount = Rc::new(());
        let _ = state.update(cx, |state, _| state.opacity.mount = Some(mount.clone()));
        Self {
            state,
            app: cx.to_async(),
            mount,
        }
    }
}

impl Drop for Presence {
    fn drop(&mut self) {
        let state = self.state.clone();
        let app = self.app.clone();
        let mount = self.mount.clone();
        self.app
            .foreground_executor()
            .spawn(async move {
                app.update(|cx| {
                    let _ = state.update(cx, |state, cx| {
                        if !state
                            .opacity
                            .mount
                            .as_ref()
                            .is_some_and(|current| Rc::ptr_eq(current, &mount))
                        {
                            return;
                        }
                        if let Some(source) = &state.opacity.source {
                            source.borrow_mut().hide();
                        }
                        state.clear_opacity(cx);
                        state.opacity.mount = None;
                    });
                });
            })
            .detach();
    }
}
